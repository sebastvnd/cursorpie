//! The trigger button (middle button by default): press, hold, double-click, drag, release.
//!
//! [`handle`] looks at the current [`Gesture`] and hands the event to one small function per
//! transition. Every function that changes the state takes the lock guard by value, so it can
//! release the lock before posting messages or synthetic events (never do that while locked).

use super::gesture::{Gesture, Source};
use super::resolve::{eval, resolve_profiles, select};
use super::touch::haptic::{self, Tick};
use super::{DRAG_TOLERANCE, FLICK_DISTANCE, Kind, NEXT_ID, PAUSED, Verdict, settings_open};
use crate::actions::post_mouse;
use crate::config::{cfg, lock};
use crate::geometry::{Point, trail_layout, trail_node_at};
use crate::msg::{Msg, Notifier};
use crate::running;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Dispatches one event of the trigger button according to the current gesture state.
/// `replay_drag` re-posts the original drag event to the app (see `on_drag_pending`).
pub(super) fn handle(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    kind: Kind,
    p: Point,
    button: i64,
    flick: bool,
    replay_drag: &dyn Fn(),
) -> Verdict {
    let g = lock(state);

    // a trackpad gesture is running: the mouse trigger passes through untouched (its press, its
    // drag and its release all belong to the app); see `touch` and "Mouse and trackpad together"
    if g.source() == Some(Source::Touch) {
        return Verdict::Keep;
    }

    match (kind, &*g) {
        (Kind::Down, _) => on_press(state, notifier, g, p, button, Press::mouse()),
        (Kind::Drag, Gesture::Pending { .. }) => {
            on_drag_pending(state, notifier, g, replay_drag, p, button, flick)
        }
        (Kind::Drag, Gesture::ActivePie { .. }) => on_drag_in_pie(notifier, g, p),
        (Kind::Up, Gesture::Pending { .. }) => on_release_pending(state, g, button),
        (Kind::Up, Gesture::ActivePie { .. }) => on_release_in_pie(notifier, g, p),
        (Kind::Up, Gesture::InSettings(_)) => {
            release_to_idle(g);
            Verdict::Drop
        }
        (Kind::Up, Gesture::Passthrough) => {
            release_to_idle(g);
            Verdict::Keep
        }
        (Kind::Drag, Gesture::Passthrough) => Verdict::Keep,
        (Kind::Drag, Gesture::InSettings(_)) => Verdict::Drop,
        _ => Verdict::Keep,
    }
}

/// Resets the gesture to `Idle` (consumes the guard, releasing the lock).
fn release_to_idle(mut g: MutexGuard<'_, Gesture>) {
    *g = Gesture::Idle;
    // the button that was held on a lock is up: Esc no longer unlocks (see `esc_lock`)
    super::touch::lock::set_held(false);
    crate::smart::end_gesture();
}

/// What a press is made of besides where it happened: who pressed, how long the hold has already
/// lasted, and whether it is the second half of a double-click that was measured elsewhere.
#[derive(Clone, Copy)]
struct Press {
    source: Source,
    /// Time already spent holding when the press is reported (a trackpad hold is reported after
    /// `ARM_DELAY`): the hold timer counts only what is left of `pie_hold_ms`.
    already: Duration,
    /// A trackpad tap came just before: open the app's own pie at once, as a second click would.
    double: bool,
}

impl Press {
    fn mouse() -> Self {
        Self {
            source: Source::Mouse,
            already: Duration::ZERO,
            double: false,
        }
    }
}

/// Button pressed. Either the second press of a double-click (open the app's pie at once) or a
/// fresh press (start the hold timers).
fn on_press<'a>(
    state: &'a Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    mut g: MutexGuard<'a, Gesture>,
    p: Point,
    button: i64,
    press: Press,
) -> Verdict {
    let source = press.source;
    // The pie is off while Settings is open. The press is still swallowed with the rest of its
    // gesture (`InSettings`: its drags and its release are dropped), but nothing opens, hovers or
    // runs, and no timer starts. The UI only says why nothing happened.
    if settings_open() {
        // over the canvas of the Flow tab the button belongs to the window (it moves the canvas)
        if source == Source::Mouse && super::FLOW_CANVAS_HOVER.load(Ordering::SeqCst) {
            *g = Gesture::Passthrough;
            return Verdict::Keep;
        }
        diag::log!("tap", "press_blocked", reason = "settings_open");
        *g = Gesture::InSettings(source);
        drop(g);
        notifier.send(Msg::PieBlocked {
            x: p.x,
            y: p.y,
            touch: source == Source::Touch,
        });
        return Verdict::Drop;
    }
    // Locked (by Esc, or by three fingers on a trackpad): the press opens no pie. It goes to the app
    // as if CursorPie were not there; holding it says why, and Esc then unlocks (see `esc_lock`).
    if source == Source::Mouse && super::touch::lock::in_force() {
        *g = Gesture::Passthrough;
        drop(g);
        diag::log!("tap", "press_locked", x = p.x, y = p.y);
        super::esc_lock::locked_hold(notifier);
        return Verdict::Keep;
    }
    let (hold_profile, app) = resolve_profiles(p);
    diag::log!(
        "tap",
        "on_press",
        x = p.x,
        y = p.y,
        button = button,
        profile = hold_profile,
        touch = source == Source::Touch
    );
    // lets the UI load the icons while the hold timer runs (never blocks, see the tap rules)
    notifier.send(Msg::Warm {
        general: hold_profile,
        app,
    });
    // which apps of the "Open app" slices already run: asked once, off this thread, while the hold
    // timer runs (does nothing when no slice opens an app or the setting is off)
    running::warm(&profile_list(hold_profile, app));
    // the suggestions of the intelligent pie are made again for this gesture (never blocks)
    crate::smart::begin(&cfg());
    let dbl_ms = cfg().double_click_ms;

    // (A) second press inside the double-click window: open the app's own pie at once
    let prev = std::mem::replace(&mut *g, Gesture::Idle);
    let mut late: Option<Point> = None;
    if source == Source::Touch && press.double {
        // the recognizer measured the gap between the tap and this touch
        if let Some(ap) = app {
            let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
            *g = Gesture::ActivePie {
                id,
                center: p,
                profile: ap,
                path: Vec::new(),
                hover: None,
                source,
            };
            drop(g);
            notifier.send(Msg::ShowPie {
                x: p.x,
                y: p.y,
                profile: ap,
                path: Vec::new(),
            });
            haptic::emit(notifier, Tick::Open);
            return Verdict::Drop;
        }
    }
    if let Gesture::WaitSecond {
        pos: first,
        app: ap,
        at,
        ..
    } = prev
    {
        let gap = at.elapsed();
        diag::log!(
            "tap",
            "double_click",
            gap_ms = gap,
            window_ms = dbl_ms,
            in_time = gap < Duration::from_millis(dbl_ms)
        );
        if gap < Duration::from_millis(dbl_ms) {
            let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
            *g = Gesture::ActivePie {
                id,
                center: p,
                profile: ap,
                path: Vec::new(),
                hover: None,
                source,
            };
            drop(g);
            notifier.send(Msg::ShowPie {
                x: p.x,
                y: p.y,
                profile: ap,
                path: Vec::new(),
            });
            watch_pointer(state, notifier, id, source);
            return Verdict::Drop; // no "open Settings" timer for an app pie
        }
        late = Some(first); // too late: the first click still has to reach the app
    }
    if let Some(first) = late {
        drop(g);
        post_mouse(first, Kind::Down, button);
        post_mouse(first, Kind::Up, button);
        g = lock(state);
    }

    // (B) ordinary press: wait for the hold
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    *g = Gesture::Pending {
        id,
        pos: p,
        profile: hold_profile,
        app,
        source,
    };
    drop(g);
    if !spawn_hold_timer(state.clone(), notifier.clone(), id, press.already) {
        // no timer can run (the system would not start its thread): the press must not wait for
        // a pie that can never open, so it goes to the app and the gesture ends here
        let mut g = lock(state);
        if matches!(&*g, Gesture::Pending { id: cur, .. } if *cur == id) {
            *g = Gesture::Idle;
        }
        return Verdict::Keep;
    }
    Verdict::Drop
}

/// The pies a press may open: the one of the hold and, when the app has its own, that one too.
fn profile_list(hold: usize, app: Option<usize>) -> Vec<usize> {
    let mut v = vec![hold];
    v.extend(app.filter(|a| *a != hold));
    v
}

/// Timer for one press: opens the pie after `pie_hold_ms`, and Settings after `settings_hold_ms`
/// if the cursor is still in the center. Each step runs on the shared timer thread (see
/// [`super::timers`]) and gives up silently as soon as the gesture is no longer the one it started
/// with (`id` differs or the state moved on). `already` is the time the hold has lasted before the
/// timer starts (zero for a mouse). `false` = no timer could be started.
fn spawn_hold_timer(st: Arc<Mutex<Gesture>>, n: Notifier, id: u64, already: Duration) -> bool {
    let (pie_ms, settings_ms) = {
        let c = cfg();
        (c.pie_hold_ms, c.settings_hold_ms)
    };
    let wait = Duration::from_millis(pie_ms).saturating_sub(already);
    super::timers::after(wait, move || {
        hold_open_pie(&st, &n, id, pie_ms, settings_ms)
    })
}

/// First step of the hold timer: the hold lasted `pie_hold_ms`, so the pie opens, then the second
/// step is set for `settings_hold_ms`.
fn hold_open_pie(st: &Arc<Mutex<Gesture>>, n: &Notifier, id: u64, pie_ms: u64, settings_ms: u64) {
    // one lock per step (locking twice in one expression would deadlock)
    let opened = {
        let mut gg = lock(st);
        let found = match &*gg {
            Gesture::Pending {
                id: cur,
                pos,
                profile,
                source,
                ..
            } if *cur == id => Some((*pos, *profile, *source)),
            _ => None,
        };
        if let Some((_, _, source)) = found
            && settings_open()
        {
            // Settings opened (menu bar) while the button was held: the pie never opens
            *gg = Gesture::InSettings(source);
            return;
        }
        // the intelligent pie, when it is kept (see `smart::kept`), is the pie that opens
        let found = found.map(|(pos, profile, source)| {
            (
                pos,
                crate::smart::opening(id, profile, &cfg().smart),
                source,
            )
        });
        if let Some((pos, profile, source)) = found {
            *gg = Gesture::ActivePie {
                id,
                center: pos,
                profile,
                path: Vec::new(),
                hover: None,
                source,
            };
        }
        found
    };
    let Some((pos, profile, source)) = opened else {
        // released, dragged away or replaced by a newer press before the timer ran out
        diag::log!("tap", "hold_cancelled", id = id, after_ms = pie_ms);
        return;
    };
    diag::log!("tap", "hold_open", x = pos.x, y = pos.y, profile = profile);
    super::pie_requested(); // not drawn yet: Settings must wait for the UI to say it was
    n.send(Msg::ShowPie {
        x: pos.x,
        y: pos.y,
        profile,
        path: Vec::new(),
    });
    if source == Source::Touch {
        haptic::emit(n, Tick::Open);
    }
    watch_pointer(st, n, id, source);
    // "Hold to open settings" is off: the pie stays what it is until the button is released
    if !cfg().settings_by_hold {
        return;
    }
    let (st, n) = (Arc::clone(st), n.clone());
    // How long the pie must have been on screen, untouched, before Settings takes its place.
    let dwell = Duration::from_millis(settings_ms.saturating_sub(pie_ms));
    let hold = SettingsHold {
        id,
        pos,
        source,
        dwell,
        opened: Instant::now(),
        resent: false,
    };
    // if the second step cannot be scheduled the pie simply never turns into Settings
    let _ = super::timers::after(dwell, move || hold_open_settings(&st, &n, hold));
}

/// What the second step of the hold timer knows. It runs again and again (see
/// [`hold_open_settings`]) until the pie has been on screen long enough.
#[derive(Clone, Copy)]
struct SettingsHold {
    id: u64,
    pos: Point,
    source: Source,
    /// How long the pie has to be visible and untouched before Settings opens.
    dwell: Duration,
    /// When the tap asked the UI for the pie.
    opened: Instant,
    /// Whether the pie was asked for a second time already (see [`PIE_GIVE_UP`]).
    resent: bool,
}

/// If the UI has not drawn the pie this long after it was asked to, the request is sent once more
/// (and the UI is woken again): the first one may have reached a window that was not awake yet.
const PIE_RESEND: Duration = Duration::from_millis(350);
/// If the pie has still not been drawn after this long, Settings is **not** opened: nothing should
/// appear that the person did not ask for. The gesture stays what it is until the button is
/// released.
const PIE_GIVE_UP: Duration = Duration::from_millis(4000);
/// How often the second step looks again while the pie has not been drawn.
const PIE_POLL: Duration = Duration::from_millis(40);

/// Second step of the hold timer: the pie is still untouched after `settings_hold_ms`, so
/// Settings opens in its place.
///
/// "Still untouched" used to be judged by the clock alone, counted from the moment the tap sent
/// the request. When the window was slow to show the pie (the first press after the program
/// started or after a long idle time, a Mac that had put the hidden program to sleep, a window that
/// had not been mapped yet) the person saw **nothing**, kept holding, and the timer opened Settings
/// over the missing pie, or the press seemed to be lost and had to be repeated. Now Settings only
/// opens for a pie the UI says it has drawn, and only after it has been drawn for the full dwell
/// time. While it has not been drawn the step looks again every [`PIE_POLL`], wakes the UI, and after
/// [`PIE_RESEND`] asks for the pie a second time; after [`PIE_GIVE_UP`] it stops without opening
/// anything.
fn hold_open_settings(st: &Arc<Mutex<Gesture>>, n: &Notifier, mut hold: SettingsHold) {
    let SettingsHold {
        id,
        pos,
        source,
        dwell,
        ..
    } = hold;
    // Still the same untouched pie? (the profile is kept for a resend)
    let still = {
        let gg = lock(st);
        match &*gg {
            Gesture::ActivePie {
                id: cur,
                hover: None,
                path,
                profile,
                ..
            } if *cur == id && path.is_empty() => Some(*profile),
            _ => None,
        }
    };
    let Some(profile) = still else {
        diag::log!("tap", "hold_settings", id = id, opened = false);
        return;
    };
    // has the UI drawn the pie, and for how long
    let wait = match super::pie_painted_for() {
        Some(seen) if seen >= dwell => None,
        Some(seen) => Some(dwell - seen),
        None => {
            let waited = hold.opened.elapsed();
            if waited >= PIE_GIVE_UP {
                diag::log!("tap", "hold_settings_no_pie", id = id, waited_ms = waited);
                return;
            }
            if !hold.resent && waited >= PIE_RESEND {
                hold.resent = true;
                diag::log!("tap", "hold_pie_resend", id = id, waited_ms = waited);
                n.send(Msg::ShowPie {
                    x: pos.x,
                    y: pos.y,
                    profile,
                    path: Vec::new(),
                });
            } else {
                n.wake();
            }
            Some(PIE_POLL)
        }
    };
    if let Some(wait) = wait {
        let (st2, n2) = (Arc::clone(st), n.clone());
        if !super::timers::after(wait, move || hold_open_settings(&st2, &n2, hold)) {
            diag::log!("tap", "hold_settings_no_timer", id = id);
        }
        return;
    }
    let open_settings = {
        let mut gg = lock(st);
        let go = matches!(
            &*gg,
            Gesture::ActivePie { id: cur, hover: None, path, .. } if *cur == id && path.is_empty()
        );
        if go {
            *gg = Gesture::InSettings(source);
        }
        go
    };
    diag::log!("tap", "hold_settings", id = id, opened = open_settings);
    if open_settings {
        if source == Source::Touch {
            haptic::emit(n, Tick::Open);
        }
        n.send(Msg::OpenSettings {
            x: pos.x,
            y: pos.y,
            about: false,
        });
    }
}

/// Opens Settings in place of the pie that is open (Space does this, see [`super::space`]). The
/// gesture becomes `InSettings`, so the rest of it (the release of the trigger) is swallowed, the
/// same as when the hold opens Settings. `false` = no pie was open.
pub(super) fn open_settings_from_pie(st: &Arc<Mutex<Gesture>>, n: &Notifier) -> bool {
    let opened = {
        let mut gg = lock(st);
        let found = match &*gg {
            Gesture::ActivePie { center, source, .. } => Some((*center, *source)),
            _ => None,
        };
        if let Some((_, source)) = found {
            *gg = Gesture::InSettings(source);
        }
        found
    };
    let Some((pos, source)) = opened else {
        return false;
    };
    diag::log!("tap", "space_settings", x = pos.x, y = pos.y);
    if source == Source::Touch {
        haptic::emit(n, Tick::Open);
    }
    n.send(Msg::OpenSettings {
        x: pos.x,
        y: pos.y,
        about: false,
    });
    true
}

/// Space was held in the open pie: the pie turns into the intelligent pie, or, when it already is
/// one, back into the pie it came from (see [`smart`](crate::smart)). The gesture goes on in the new
/// pie: its top ring, nothing hovered, the same center. Returns whether there was a pie to turn.
pub(super) fn switch_smart_pie(st: &Arc<Mutex<Gesture>>, n: &Notifier) -> bool {
    let switched = {
        let mut gg = lock(st);
        match &mut *gg {
            Gesture::ActivePie {
                id,
                center,
                profile,
                path,
                hover,
                source,
            } => {
                // profile 0 is the general pie, where an origin that was lost goes back to
                let target = crate::smart::switch_target(*id, *profile, 0);
                *profile = target;
                path.clear();
                *hover = None;
                Some((*center, *source, target))
            }
            _ => None,
        }
    };
    let Some((pos, source, profile)) = switched else {
        return false;
    };
    diag::log!(
        "tap",
        "space_smart",
        smart = crate::smart::is_smart(profile)
    );
    if source == Source::Touch {
        haptic::emit(n, Tick::Open);
    }
    n.send(Msg::ShowPie {
        x: pos.x,
        y: pos.y,
        profile,
        path: Vec::new(),
    });
    true
}

/// After a quick click on an app with its own pie: if no second press arrives within `ms`, the
/// first click is replayed to the app. If no timer can be started the click is replayed at once.
fn spawn_double_timer(st: &Arc<Mutex<Gesture>>, id: u64, button: i64, ms: u64) {
    let later = Arc::clone(st);
    let started = super::timers::after(Duration::from_millis(ms), move || {
        replay_first_click(&later, id, button, ms);
    });
    if !started {
        replay_first_click(st, id, button, 0);
    }
}

/// The double-click window ended (or no timer could wait for it): when the gesture is still the
/// one that waits for a second click, it ends and the first click goes to the app after all.
fn replay_first_click(st: &Arc<Mutex<Gesture>>, id: u64, button: i64, waited_ms: u64) {
    let replay = {
        let mut g = lock(st);
        let found = match &*g {
            Gesture::WaitSecond { id: cur, pos, .. } if *cur == id => Some(*pos),
            _ => None,
        };
        if found.is_some() {
            *g = Gesture::Idle;
        }
        found
    };
    if let Some(p) = replay {
        // no second click came: the first one goes to the app after all
        diag::log!(
            "tap",
            "click_replay",
            x = p.x,
            y = p.y,
            waited_ms = waited_ms
        );
        post_mouse(p, Kind::Down, button);
        post_mouse(p, Kind::Up, button);
    }
}

/// Moved while pressed, before the pie opened: either a real drag (hand it back to the app) or,
/// with "flick" enabled, a request to open the pie instantly.
fn on_drag_pending(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    mut g: MutexGuard<'_, Gesture>,
    replay_drag: &dyn Fn(),
    p: Point,
    button: i64,
    flick: bool,
) -> Verdict {
    let Gesture::Pending {
        id,
        pos,
        profile,
        source,
        ..
    } = &*g
    else {
        return Verdict::Drop;
    };
    let (id, start, profile, source) = (*id, *pos, *profile, *source);
    let limit = if flick {
        FLICK_DISTANCE
    } else {
        DRAG_TOLERANCE
    };
    if (p.x - start.x).hypot(p.y - start.y) > limit {
        if flick {
            // dragging right away opens the pie instantly (the intelligent pie, when it is kept)
            let profile = crate::smart::opening(id, profile, &cfg().smart);
            *g = Gesture::ActivePie {
                id,
                center: start,
                profile,
                path: Vec::new(),
                hover: None,
                source,
            };
            drop(g);
            notifier.send(Msg::ShowPie {
                x: start.x,
                y: start.y,
                profile,
                path: Vec::new(),
            });
            watch_pointer(state, notifier, id, source);
        } else {
            // a real drag: hand the gesture back to the app (e.g. panning)
            *g = Gesture::Passthrough;
            drop(g);
            post_mouse(start, Kind::Down, button);
            replay_drag();
        }
    }
    Verdict::Drop
}

/// How often the pointer is read while the pie of a mouse press is open on Windows.
const POINTER_POLL: Duration = Duration::from_millis(16);

/// Whether the pointer is read on a timer while the pie of a mouse press is open: on Windows, where
/// the hook can miss moves (see [`watch_pointer`]). The code is compiled and tested on every system,
/// so that a change to it is checked wherever the tests run, not only on a Windows machine.
const POLL_POINTER: bool = cfg!(windows) || cfg!(test);

/// Where the pointer is now. The system's, except in tests: a test that opens a pie with made-up
/// positions must not hear the real pointer of the machine it runs on (on a CI runner it sits
/// wherever the desktop left it, and the timer would aim the pie at it), so there it is whatever
/// the test put in `TEST_POINTER`, and nowhere by default.
pub(super) fn read_pointer() -> Option<(f64, f64)> {
    #[cfg(test)]
    {
        *TEST_POINTER.lock().unwrap_or_else(|e| e.into_inner())
    }
    #[cfg(not(test))]
    {
        crate::platform::cursor_pos()
    }
}

/// The pointer of the tests (see [`read_pointer`]).
#[cfg(test)]
static TEST_POINTER: Mutex<Option<(f64, f64)>> = Mutex::new(None);

/// Windows: while the pie of a mouse press is open, the pointer is also read on a timer and fed to
/// the same hover code that the moves of the mouse hook feed.
///
/// The hover used to depend only on `WM_MOUSEMOVE` reaching the low-level hook. Windows drops a
/// move when the hook is late (a busy machine, the first press after a long idle time) and removes
/// the hook altogether when a callback times out; until the watchdog has installed it again (it
/// only looks every few seconds) the pointer moved and no slice was ever chosen. Reading the
/// pointer here makes the pie follow it whatever the hook delivered. It is cheap (a read of the
/// cursor position, the hover code only does something when the slice changes) and ends by itself
/// as soon as the gesture is no longer this pie. A trackpad hold has a watcher of its own
/// (`touch::watch_pointer`), and the other systems do not need one.
fn watch_pointer(st: &Arc<Mutex<Gesture>>, n: &Notifier, id: u64, source: Source) {
    if !POLL_POINTER || source != Source::Mouse {
        return;
    }
    let (st, n) = (Arc::clone(st), n.clone());
    // if no timer can be started the hook alone aims the pie, as before
    let _ = super::timers::after(POINTER_POLL, move || pointer_tick(st, n, id, None));
}

/// One step of [`watch_pointer`]: when the pie of mouse gesture `id` is still open, the pointer
/// (if it moved since `last`) aims it, and the next step is set.
fn pointer_tick(st: Arc<Mutex<Gesture>>, n: Notifier, id: u64, last: Option<(f64, f64)>) {
    let g = lock(&st);
    let ours = matches!(
        &*g,
        Gesture::ActivePie { id: cur, source: Source::Mouse, .. } if *cur == id
    );
    if !ours {
        return; // released, replaced by another press, or turned into Settings
    }
    let now = read_pointer();
    match now {
        Some((x, y)) if now != last => {
            on_drag_in_pie(&n, g, Point::new(x, y));
        }
        _ => drop(g),
    }
    let (st2, n2) = (Arc::clone(&st), n.clone());
    let _ = super::timers::after(POINTER_POLL, move || pointer_tick(st2, n2, id, now));
}

/// Trail ("web") mode: the centers of the rings that were left to open a submenu, outermost first.
/// It always has one entry per level of the open path, or is stale (see [`trail`]) and is not used.
static TRAIL: Mutex<Vec<Point>> = Mutex::new(Vec::new());

fn trail() -> MutexGuard<'static, Vec<Point>> {
    TRAIL.lock().unwrap_or_else(|e| e.into_inner())
}

/// Moved while the pie is open: update the hovered slice, or enter a submenu when the cursor
/// crosses the rim over one. In the trail mode (`submenu_trail`) the ring it came from stays, and
/// moving the pointer back onto its center returns to it. Over an "Open app" slice whose app runs, the UI also learns which of
/// the two badges the cursor is on (see [`running`]).
fn on_drag_in_pie(notifier: &Notifier, mut g: MutexGuard<'_, Gesture>, p: Point) -> Verdict {
    let Gesture::ActivePie {
        center,
        profile,
        path,
        hover,
        source,
        ..
    } = &mut *g
    else {
        return Verdict::Drop;
    };
    let touch = *source == Source::Touch;
    let (trail_on, win, shape) = {
        let c = cfg();
        (c.submenu_trail, c.pie_size, c.trail_shape())
    };
    let hit = eval(*profile, path, *center, p);
    if trail_on && !hit.enter && !path.is_empty() {
        // onto the node of a ring that was left: go straight back to that ring
        let scale = crate::platform::window_scale(center.x, center.y);
        let level = {
            let t = trail();
            (t.len() == path.len())
                .then(|| {
                    let offs: Vec<(f32, f32)> = t
                        .iter()
                        .map(|q| {
                            (
                                ((q.x - center.x) * scale) as f32,
                                ((q.y - center.y) * scale) as f32,
                            )
                        })
                        .collect();
                    trail_node_at(
                        &trail_layout(&offs, win, shape),
                        ((p.x - center.x) * scale) as f32,
                        ((p.y - center.y) * scale) as f32,
                    )
                })
                .flatten()
        };
        if let Some(level) = level {
            {
                // The ring comes back where the pointer is, so the pointer is in its middle and
                // nothing is hovered at once; the rings still behind it move with it, so the
                // whole web keeps its shape.
                let mut t = trail();
                let (dx, dy) = (p.x - t[level].x, p.y - t[level].y);
                t.truncate(level);
                for q in t.iter_mut() {
                    q.x += dx;
                    q.y += dy;
                }
            }
            path.truncate(level);
            *center = p;
            *hover = None;
            running::set_shown(None);
            let (pr, pa) = (*profile, path.clone());
            drop(g);
            notifier.send(Msg::ShowPie {
                x: p.x,
                y: p.y,
                profile: pr,
                path: pa,
            });
            if touch {
                haptic::emit(notifier, Tick::Hover(None));
            }
            return Verdict::Drop;
        }
    }
    if hit.enter {
        if let Some(h) = hit.hover {
            // crossed the rim over a submenu: open it, re-centered on the cursor. In the trail mode
            // the ring that is left is remembered, and the UI shrinks it to a node.
            if trail_on {
                let mut t = trail();
                if path.is_empty() {
                    t.clear(); // whatever an earlier gesture left
                }
                t.truncate(path.len());
                t.push(*center);
            }
            path.push(h);
            *center = p;
            *hover = None;
            running::set_shown(None);
            let (pr, pa) = (*profile, path.clone());
            drop(g);
            notifier.send(Msg::ShowPie {
                x: p.x,
                y: p.y,
                profile: pr,
                path: pa,
            });
        }
    } else {
        let slice_changed = hit.hover != *hover;
        if slice_changed || hit.choice != running::shown() {
            *hover = hit.hover;
            running::set_shown(hit.choice);
            drop(g);
            // also wakes the UI when only the badge changed: it reads `running::shown`
            notifier.send(Msg::Hover(hit.hover));
            if touch && slice_changed {
                haptic::emit(notifier, Tick::Hover(hit.hover));
            }
        }
    }
    Verdict::Drop
}

/// Released before the pie opened: a click. Pass it on, or wait for a second click if the app
/// has its own pie.
fn on_release_pending(
    state: &Arc<Mutex<Gesture>>,
    mut g: MutexGuard<'_, Gesture>,
    button: i64,
) -> Verdict {
    let Gesture::Pending {
        id,
        pos,
        app,
        source,
        ..
    } = &*g
    else {
        return Verdict::Drop;
    };
    let (id, start, app, source) = (*id, *pos, *app, *source);
    if source == Source::Touch {
        // a trackpad hold that ended early: nothing was pressed, so there is no click to hand back
        // (and a tap measures its own double-click gap, see `touch::recognizer`)
        diag::log!("tap", "on_release", pie = false, touch = true);
        *g = Gesture::Idle;
        return Verdict::Drop;
    }
    diag::log!(
        "tap",
        "on_release",
        x = start.x,
        y = start.y,
        button = button,
        pie = false,
        waits_for_second_click = app.is_some()
    );
    match app {
        Some(app) => {
            // this app has its own profile: wait for a possible second click
            *g = Gesture::WaitSecond {
                id,
                pos: start,
                app,
                at: Instant::now(),
            };
            drop(g);
            spawn_double_timer(state, id, button, cfg().double_click_ms);
        }
        None => {
            *g = Gesture::Idle;
            drop(g);
            post_mouse(start, Kind::Down, button);
            post_mouse(start, Kind::Up, button);
        }
    }
    Verdict::Drop
}

/// Released while the pie is open: run the slice under the cursor (if any) and close the pie.
fn on_release_in_pie(notifier: &Notifier, mut g: MutexGuard<'_, Gesture>, p: Point) -> Verdict {
    let Gesture::ActivePie {
        center,
        profile,
        path,
        ..
    } = &*g
    else {
        return Verdict::Drop;
    };
    let (center, profile, path) = (*center, *profile, path.clone());
    *g = Gesture::Idle;
    drop(g);
    let hit = eval(profile, &path, center, p);
    let hover = hit.hover;
    running::set_shown(None);
    diag::log!(
        "tap",
        "on_release",
        x = p.x,
        y = p.y,
        pie = true,
        slice = hover.is_some(),
        choice = ?hit.choice
    );
    if let Some(h) = hover {
        if settings_open() {
            // the pie was open when Settings opened (menu bar): closing it must not run anything
            diag::log!("tap", "select_blocked", reason = "settings_open");
        } else {
            select(profile, &path, h, hit.choice);
        }
    }
    notifier.send(Msg::HidePie);
    Verdict::Drop
}

// ------------------------------------------------------------------ trackpad entry points
//
// The touch source (`touch::Bridge`) does not have a state machine of its own: it calls these,
// which feed the same functions a mouse press, drag and release feed. They only act on a gesture
// that the touch source started (`Source::Touch`), so a stale or late touch event can never
// disturb a mouse gesture.

/// A trackpad hold was armed at `p`. Starts the gesture if nothing else is running (a mouse
/// gesture that is running owns the pie, and so does the pause switch); returns whether it did.
/// `already` is how long the fingers have been down, `double` whether a tap came just before.
pub(super) fn touch_press(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    p: Point,
    already: Duration,
    double: bool,
) -> bool {
    if PAUSED.load(Ordering::SeqCst) {
        return false;
    }
    // The pie is off while Settings is open. A mouse press says so (`on_press`: the UI brings
    // Settings forward); a trackpad hold has to do the same, or the two triggers behave
    // differently: with Settings behind another window, the mouse brings it back and the
    // trackpad does nothing. Nothing is swallowed here: the fingers only move the pointer.
    if settings_open() {
        let idle = matches!(&*lock(state), Gesture::Idle);
        if idle {
            diag::log!("tap", "touch_blocked", reason = "settings_open");
            notifier.send(Msg::PieBlocked {
                x: p.x,
                y: p.y,
                touch: true,
            });
        }
        return false;
    }
    let g = lock(state);
    if !matches!(&*g, Gesture::Idle) {
        return false;
    }
    let button = cfg().trigger_button;
    on_press(
        state,
        notifier,
        g,
        p,
        button,
        Press {
            source: Source::Touch,
            already,
            double,
        },
    );
    true
}

/// The holding fingers moved: the pointer is now at `p`.
pub(super) fn touch_move(state: &Arc<Mutex<Gesture>>, notifier: &Notifier, p: Point) {
    let g = lock(state);
    if matches!(
        &*g,
        Gesture::ActivePie {
            source: Source::Touch,
            ..
        }
    ) {
        on_drag_in_pie(notifier, g, p);
    }
}

/// A holding finger lifted. With the pointer at `p` the slice under it runs; without a position
/// (the pointer could not be read) the pie closes and nothing runs.
pub(super) fn touch_release(state: &Arc<Mutex<Gesture>>, notifier: &Notifier, p: Option<Point>) {
    let g = lock(state);
    match (&*g, p) {
        (
            Gesture::Pending {
                source: Source::Touch,
                ..
            },
            _,
        ) => {
            let button = cfg().trigger_button;
            on_release_pending(state, g, button);
        }
        (
            Gesture::ActivePie {
                source: Source::Touch,
                ..
            },
            Some(p),
        ) => {
            on_release_in_pie(notifier, g, p);
        }
        (
            Gesture::ActivePie {
                source: Source::Touch,
                ..
            },
            None,
        ) => {
            release_to_idle(g);
            notifier.send(Msg::HidePie);
        }
        (Gesture::InSettings(Source::Touch), _) => release_to_idle(g),
        _ => {}
    }
}

/// The hold was broken (too many fingers, a slide, the device went away): back to `Idle`, the pie
/// closed, nothing run.
pub(super) fn touch_cancel(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) {
    let mut g = lock(state);
    match &*g {
        Gesture::ActivePie {
            source: Source::Touch,
            ..
        } => {
            *g = Gesture::Idle;
            drop(g);
            notifier.send(Msg::HidePie);
        }
        Gesture::Pending {
            source: Source::Touch,
            ..
        }
        | Gesture::InSettings(Source::Touch) => *g = Gesture::Idle,
        _ => {}
    }
}

/// The hold was locked away (see [`touch::lock`](super::touch)): like [`touch_cancel`], back to
/// `Idle`, the pie closed, nothing run. Returns where the pie was (or where the hold began), so the
/// animation of the lock plays there; `None` when no trackpad gesture was running.
pub(super) fn touch_lock(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) -> Option<Point> {
    let mut g = lock(state);
    let at = match &*g {
        Gesture::ActivePie {
            source: Source::Touch,
            center,
            ..
        } => Some((*center, true)),
        Gesture::Pending {
            source: Source::Touch,
            pos,
            ..
        } => Some((*pos, false)),
        Gesture::InSettings(Source::Touch) => None,
        _ => return None,
    };
    *g = Gesture::Idle;
    drop(g);
    match at {
        Some((p, pie_open)) => {
            if pie_open {
                notifier.send(Msg::HidePie);
            }
            Some(p)
        }
        None => None,
    }
}

/// The lock was opened (Esc, or a click of the left button) while the mouse trigger is still held
/// (see [`esc_lock`](super::esc_lock)): the pie opens at once under the pointer, as if the hold had
/// just lasted long enough, and the padlock opens in its middle. Returns whether it was started;
/// when it was not (no locked hold, Settings open), the caller says so with the padlock alone.
///
/// The press that began the hold went to the app as if CursorPie were not there, and the release of
/// this pie belongs to the pie, so the app would be left with a button that never came up. It gets
/// the release now: the app sees the same press and release it would have seen from a locked hold.
pub(super) fn open_after_unlock(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    p: Point,
) -> bool {
    if settings_open() || PAUSED.load(Ordering::SeqCst) {
        return false;
    }
    let button = cfg().trigger_button;
    if !matches!(&*lock(state), Gesture::Passthrough) {
        return false;
    }
    // not while locked: the guard of the state is let go before anything is posted
    post_mouse(p, Kind::Up, button);
    super::touch::lock::mark_reveal();
    let g = lock(state);
    if !matches!(&*g, Gesture::Passthrough) {
        super::touch::lock::clear_reveal();
        return false; // the button came up while the release was posted: nothing is held any more
    }
    let press = Press {
        source: Source::Mouse,
        already: Duration::from_millis(cfg().pie_hold_ms),
        double: false,
    };
    on_press(state, notifier, g, p, button, press);
    true
}

/// Esc while a pie is open (see [`esc_lock`](super::esc_lock)): the pie closes without running
/// anything, like [`touch_lock`], but for a pie of any source. Returns where the pie was, so the
/// animation of the lock plays there; `None` when no pie is open (nothing is changed then).
///
/// A mouse pie has its button still down: the gesture goes to `InSettings`, which swallows the
/// rest of it (its drags and its release) so no stray button event reaches the app. A trackpad hold
/// goes to `Idle`, as in [`touch_lock`]; the touch source ends the hold when the fingers lift.
pub(super) fn esc_lock(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) -> Option<Point> {
    let mut g = lock(state);
    let (center, source) = match &*g {
        Gesture::ActivePie { center, source, .. } => (*center, *source),
        _ => return None,
    };
    *g = match source {
        Source::Mouse => Gesture::InSettings(Source::Mouse),
        Source::Touch => Gesture::Idle,
    };
    drop(g);
    notifier.send(Msg::HidePie);
    Some(center)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn setup(id: u64) -> (Arc<Mutex<Gesture>>, Notifier, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        let n = Notifier {
            tx,
            ctx: eframe::egui::Context::default(),
        };
        let st = Arc::new(Mutex::new(Gesture::ActivePie {
            id,
            center: Point { x: 10.0, y: 10.0 },
            profile: 0,
            path: Vec::new(),
            hover: None,
            source: Source::Mouse,
        }));
        (st, n, rx)
    }

    fn hold(id: u64, dwell_ms: u64) -> SettingsHold {
        SettingsHold {
            id,
            pos: Point { x: 10.0, y: 10.0 },
            source: Source::Mouse,
            dwell: Duration::from_millis(dwell_ms),
            opened: Instant::now(),
            resent: false,
        }
    }

    fn settings_opened(rx: &mpsc::Receiver<Msg>, within_ms: u64) -> bool {
        let end = Instant::now() + Duration::from_millis(within_ms);
        while Instant::now() < end {
            if let Ok(Msg::OpenSettings { .. }) = rx.recv_timeout(Duration::from_millis(20)) {
                return true;
            }
        }
        false
    }

    /// Puts the factory config in place for a test and the live one back afterwards: the hover
    /// depends on the pie, and the live config is the one of whoever runs the test.
    struct FactoryConfig(crate::config::Config);

    impl FactoryConfig {
        fn set() -> Self {
            let mut c = crate::config::cfg();
            let saved = c.clone();
            *c = crate::config::Config::default();
            Self(saved)
        }
    }

    impl Drop for FactoryConfig {
        fn drop(&mut self) {
            *crate::config::cfg() = self.0.clone();
            *TEST_POINTER.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    fn put_pointer(p: Option<(f64, f64)>) {
        *TEST_POINTER.lock().unwrap_or_else(|e| e.into_inner()) = p;
    }

    /// Whether a `Hover` message arrives within `within_ms`.
    fn hovered(rx: &mpsc::Receiver<Msg>, within_ms: u64) -> Option<Option<usize>> {
        let end = Instant::now() + Duration::from_millis(within_ms);
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            if let Ok(Msg::Hover(h)) = rx.recv_timeout(left) {
                return Some(h);
            }
        }
        None
    }

    #[test]
    fn the_pointer_timer_aims_the_pie_with_no_event_from_the_hook() {
        let _serial = super::super::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _config = FactoryConfig::set();
        let (st, n, rx) = setup(21);
        // 100 points to the right of the center of the pie
        put_pointer(Some((110.0, 10.0)));
        watch_pointer(&st, &n, 21, Source::Mouse);
        let hover = hovered(&rx, 5000);
        assert!(
            hover.is_some(),
            "the timer read the pointer and aimed the pie"
        );
        assert_ne!(hover, Some(None), "the pointer is over a slice");
        // the pointer back on the center: nothing is chosen
        put_pointer(Some((10.0, 10.0)));
        assert_eq!(hovered(&rx, 5000), Some(None));
        // the gesture ends: so does the timer
        *lock(&st) = Gesture::Idle;
        std::thread::sleep(Duration::from_millis(60));
        while rx.try_recv().is_ok() {}
        put_pointer(Some((110.0, 10.0)));
        assert_eq!(hovered(&rx, 150), None, "no gesture, no aiming");
    }

    #[test]
    fn the_pointer_timer_hears_nothing_where_the_test_put_no_pointer() {
        let _serial = super::super::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _config = FactoryConfig::set();
        let (st, n, rx) = setup(22);
        put_pointer(None); // the real pointer of the machine is never read
        watch_pointer(&st, &n, 22, Source::Mouse);
        assert_eq!(hovered(&rx, 200), None);
        *lock(&st) = Gesture::Idle;
        std::thread::sleep(Duration::from_millis(60));
    }

    #[test]
    fn a_trackpad_pie_has_no_pointer_timer_of_this_kind() {
        let _serial = super::super::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _config = FactoryConfig::set();
        let (st, n, rx) = setup(23);
        put_pointer(Some((110.0, 10.0)));
        watch_pointer(&st, &n, 23, Source::Touch); // the trackpad has its own watcher
        assert_eq!(hovered(&rx, 200), None);
    }

    #[test]
    fn settings_do_not_open_over_a_pie_that_was_never_drawn() {
        let _serial = super::super::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (st, n, rx) = setup(7);
        super::super::pie_requested();
        hold_open_settings(&st, &n, hold(7, 30));
        assert!(
            !settings_opened(&rx, 400),
            "the dwell time passed, but the UI never drew the pie"
        );
        assert!(
            matches!(&*lock(&st), Gesture::ActivePie { .. }),
            "the gesture is untouched"
        );
    }

    #[test]
    fn settings_open_after_the_pie_was_drawn_for_the_dwell_time() {
        let _serial = super::super::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (st, n, rx) = setup(8);
        super::super::pie_requested();
        hold_open_settings(&st, &n, hold(8, 120));
        std::thread::sleep(Duration::from_millis(100));
        super::super::pie_painted(); // the UI draws it late
        let t = Instant::now();
        assert!(
            settings_opened(&rx, 1500),
            "it opens once the pie has been seen long enough"
        );
        assert!(
            t.elapsed() >= Duration::from_millis(90),
            "not at once: the pie was seen for the whole dwell time first ({:?})",
            t.elapsed()
        );
        assert!(matches!(&*lock(&st), Gesture::InSettings(_)));
    }

    #[test]
    fn a_pie_that_was_touched_never_becomes_settings() {
        let _serial = super::super::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (st, n, rx) = setup(9);
        super::super::pie_requested();
        super::super::pie_painted();
        if let Gesture::ActivePie { hover, .. } = &mut *lock(&st) {
            *hover = Some(2);
        }
        hold_open_settings(&st, &n, hold(9, 0));
        assert!(!settings_opened(&rx, 150));
    }

    #[test]
    fn the_pie_is_asked_for_again_when_it_is_late() {
        let _serial = super::super::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let (st, n, rx) = setup(10);
        super::super::pie_requested();
        let mut h = hold(10, 30);
        h.opened = Instant::now() - PIE_RESEND; // already late
        hold_open_settings(&st, &n, h);
        let end = Instant::now() + Duration::from_millis(300);
        let mut asked = false;
        while Instant::now() < end && !asked {
            asked = matches!(
                rx.recv_timeout(Duration::from_millis(20)),
                Ok(Msg::ShowPie { .. })
            );
        }
        assert!(asked, "a second ShowPie was sent");
        super::super::pie_painted(); // let the polling job end
        std::thread::sleep(Duration::from_millis(100));
    }
}
