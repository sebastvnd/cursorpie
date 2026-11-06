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
use crate::geometry::Point;
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
            on_drag_pending(notifier, g, replay_drag, p, button, flick)
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
        notifier.send(Msg::PieBlocked { x: p.x, y: p.y });
        return Verdict::Drop;
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
    spawn_hold_timer(state.clone(), notifier.clone(), id, press.already);
    Verdict::Drop
}

/// The pies a press may open: the one of the hold and, when the app has its own, that one too.
fn profile_list(hold: usize, app: Option<usize>) -> Vec<usize> {
    let mut v = vec![hold];
    v.extend(app.filter(|a| *a != hold));
    v
}

/// Timer thread for one press: opens the pie after `pie_hold_ms`, and Settings after
/// `settings_hold_ms` if the cursor is still in the center. It gives up silently as soon as the
/// gesture is no longer the one it started with (`id` differs or the state moved on). `already` is
/// the time the hold has lasted before the timer starts (zero for a mouse).
fn spawn_hold_timer(st: Arc<Mutex<Gesture>>, n: Notifier, id: u64, already: Duration) {
    std::thread::spawn(move || {
        let (pie_ms, settings_ms) = {
            let c = cfg();
            (c.pie_hold_ms, c.settings_hold_ms)
        };

        std::thread::sleep(Duration::from_millis(pie_ms).saturating_sub(already));
        // one lock per step (locking twice in one expression would deadlock)
        let opened = {
            let mut gg = lock(&st);
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
        n.send(Msg::ShowPie {
            x: pos.x,
            y: pos.y,
            profile,
            path: Vec::new(),
        });
        if source == Source::Touch {
            haptic::emit(&n, Tick::Open);
        }

        std::thread::sleep(Duration::from_millis(settings_ms.saturating_sub(pie_ms)));
        let open_settings = {
            let mut gg = lock(&st);
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
                haptic::emit(&n, Tick::Open);
            }
            n.send(Msg::OpenSettings {
                x: pos.x,
                y: pos.y,
                about: false,
            });
        }
    });
}

/// After a quick click on an app with its own pie: if no second press arrives within `ms`, the
/// first click is replayed to the app.
fn spawn_double_timer(st: Arc<Mutex<Gesture>>, id: u64, button: i64, ms: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(ms));
        let replay = {
            let mut g = lock(&st);
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
            diag::log!("tap", "click_replay", x = p.x, y = p.y, waited_ms = ms);
            post_mouse(p, Kind::Down, button);
            post_mouse(p, Kind::Up, button);
        }
    });
}

/// Moved while pressed, before the pie opened: either a real drag (hand it back to the app) or,
/// with "flick" enabled, a request to open the pie instantly.
fn on_drag_pending(
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
            // dragging right away opens the pie instantly
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

/// Moved while the pie is open: update the hovered slice, or enter a submenu when the cursor
/// crosses the rim over one. Over an "Open app" slice whose app runs, the UI also learns which of
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
    let hit = eval(*profile, path, *center, p);
    if hit.enter {
        if let Some(h) = hit.hover {
            // crossed the rim over a submenu: open it, re-centered on the cursor
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
            spawn_double_timer(state.clone(), id, button, cfg().double_click_ms);
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
            notifier.send(Msg::PieBlocked { x: p.x, y: p.y });
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
