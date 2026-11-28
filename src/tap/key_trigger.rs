//! The keyboard trigger: a key combination opens the pie where the pointer is.
//!
//! The mouse trigger needs a button held down and the trackpad's one needs a finger resting on the
//! surface. Many people would rather press keys. This is the third source of the same gesture
//! ([`Source::Key`](super::gesture::Source::Key)): the pie, the profiles, the hover and the actions
//! are the ones of the other two, and only the way the pie *opens* and *ends* is told here.
//!
//! ```text
//!   combination pressed ──► the pie opens at the pointer at once (no hold time: a key is not a click
//!                           that has to reach the app)
//!   the pointer is moved by whatever moves it (a mouse, one finger on the trackpad), and the slice
//!   under it is hovered, as in any pie
//!
//!   Hold  : key let go ──► the slice under the pointer runs, the pie closes
//!   Toggle: key let go ──► the pie stays open ──┬─ a click or a tap ──► the slice under the pointer
//!                                               │                      runs, the pie closes
//!                                               ├─ the combination again, or Esc ──► closes, nothing runs
//!                                               └─ Space ──► Settings (as in every pie)
//!   Auto  : let go within `KEY_TAP_MS` ──► as Toggle (it was a tap); later ──► as Hold
//! ```
//!
//! Two combinations can be set: the first opens the general pie (the one a hold of the mouse
//! button opens), the second the pie of the app in front (what a double-click opens), or the general
//! one when the app has none.
//!
//! # What the combination does and does not take
//!
//! * The combination needs at least one modifier and is matched exactly (Ctrl+Space is not
//!   Ctrl+Shift+Space). It is swallowed while this trigger is on: its key-down, its auto-repeat and
//!   its key-up never reach the app. It is the one thing this module takes; every other key is the
//!   app's, with three exceptions that exist while a pie is open: Esc (closes it, and does **not**
//!   lock CursorPie, see [`esc`]), Space (Settings, see [`space`](super::space)) and the combination.
//! * While Settings is recording a shortcut the combination is the recorder's, not ours.
//! * While Settings is open, or CursorPie is locked, the combination opens nothing: Settings says so
//!   (a pill), and a lock says so too. Holding the combination while locked and pressing Esc opens
//!   the lock, as holding the mouse trigger does.
//! * Another gesture that runs (a mouse hold, a trackpad hold) owns the pie: the combination does
//!   nothing until it is over.
//!
//! # Where it works
//!
//! macOS and Windows get every key from their hooks, so the combination is read there. On X11 the
//! layer grabs the combination passively (`XGrabKey`, see `tap::linux`) and takes the whole keyboard
//! while a pie stays open, so Esc and the combination reach CursorPie. A Wayland session does not
//! give a program the keyboard, so the combination is not available there (Settings says so).
//!
//! Everything here runs on the tap thread (and the pointer watcher on the timer thread) and never
//! blocks. The state is a few atomics: the gesture itself lives, as always, in the shared
//! [`Gesture`].

use super::gesture::Gesture;
use super::resolve::resolve_profiles;
use super::{Kind, NEXT_ID, RECORDING, Verdict, pie_clicks, settings_open, touch, trigger};
use crate::config::{KEY_TAP_MS, KeyPieMode, cfg};
use crate::geometry::Point;
use crate::keys::{MOD_MASK, Shortcut};
use crate::msg::{Msg, Notifier};
use crate::running;
use crate::ui::hud::HudNote;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A pie of the keyboard is open (the combination is held, or the pie stays open). One atomic load
/// for the pie window, which then takes the pointer instead of letting it through (the pointer
/// moves while no button is down, see `ui::app::windows`), and for the X11 layer, which then
/// grabs the left button for the click that chooses.
pub(crate) static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The key code whose press this module took (-1 = none yet). Kept after the pie is gone, until the
/// key comes up: its repeats and its release are swallowed too.
static KEY: AtomicI32 = AtomicI32::new(-1);
/// [`KEY`] is down.
static DOWN: AtomicBool = AtomicBool::new(false);
/// The id of the gesture of the pie that is open (0 = none).
static PIE: AtomicU64 = AtomicU64::new(0);
/// The key was let go and the pie stays open ([`KeyPieMode::Toggle`], or a tap in `Auto`).
static STICKY: AtomicBool = AtomicBool::new(false);
/// When the key went down, in ms since the tap's epoch, plus 1.
static PRESSED_AT: AtomicU64 = AtomicU64::new(0);
/// The combination went down while CursorPie is locked: Esc may open the lock until it comes up.
static LOCKED_HOLD: AtomicBool = AtomicBool::new(false);

/// How often the pointer is read while a pie of the keyboard is open (the same as the other pies).
#[cfg(not(test))]
const WATCH_EVERY: Duration = Duration::from_millis(16);

/// Which pie a combination opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Which {
    /// The general pie: the one the hold of the mouse trigger opens.
    General,
    /// The pie of the app in front, or the general one when the app has none.
    App,
}

/// Whether `chord` is what was pressed: the same key and exactly the same modifiers (the locks of
/// the keyboard are not modifiers). Pure, so it is unit tested.
pub(crate) fn matches(chord: &Shortcut, code: i32, mods: u8) -> bool {
    i32::from(chord.key) == code && (mods & MOD_MASK) == (chord.mods & MOD_MASK)
}

/// Which of the two combinations was pressed, if any. The general one wins when both are the same.
pub(crate) fn which(
    general: Option<Shortcut>,
    app: Option<Shortcut>,
    code: i32,
    mods: u8,
) -> Option<Which> {
    if general.is_some_and(|c| matches(&c, code, mods)) {
        Some(Which::General)
    } else if app.is_some_and(|c| matches(&c, code, mods)) {
        Some(Which::App)
    } else {
        None
    }
}

/// Whether the pie stays open when the key comes up, `held_ms` after it went down. Pure.
pub(crate) fn stays_open(mode: KeyPieMode, held_ms: u64) -> bool {
    match mode {
        KeyPieMode::Hold => false,
        KeyPieMode::Toggle => true,
        KeyPieMode::Auto => held_ms < KEY_TAP_MS,
    }
}

/// Whether a pie that stays open is open right now. The X11 layer takes the whole keyboard for as
/// long as this is true (and gives it back when it is not), so Esc reaches it.
#[allow(dead_code)] // read by the X11 layer only
pub(crate) fn sticky_open(state: &Arc<Mutex<Gesture>>) -> bool {
    STICKY.load(Ordering::SeqCst) && {
        let id = PIE.load(Ordering::SeqCst);
        id != 0 && trigger::key_pie_is_open(state, id)
    }
}

/// Forgets everything (the tap was disabled by the system, or CursorPie was paused).
#[cfg_attr(not(any(target_os = "macos", windows, test)), allow(dead_code))]
pub(crate) fn clear() {
    PIE.store(0, Ordering::SeqCst);
    ACTIVE.store(false, Ordering::SeqCst);
    STICKY.store(false, Ordering::SeqCst);
    DOWN.store(false, Ordering::SeqCst);
    KEY.store(-1, Ordering::SeqCst);
    if LOCKED_HOLD.swap(false, Ordering::SeqCst) {
        touch::lock::set_held(false);
    }
}

/// The pie of gesture `id` is over: the flags that said it was open go down. Does nothing when
/// another pie of the keyboard has taken the place of `id` since.
fn over(id: u64) {
    if PIE
        .compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        ACTIVE.store(false, Ordering::SeqCst);
        STICKY.store(false, Ordering::SeqCst);
        crate::smart::end_gesture();
    }
}

/// Looks at one key event. `Some(verdict)` = it was ours and the verdict is final; `None` = not
/// ours, the normal key handling goes on.
pub(super) fn handle(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    down: bool,
    code: i32,
    mods: u8,
) -> Option<Verdict> {
    // Settings is recording a shortcut: the combination is just another shortcut then
    if RECORDING.load(Ordering::SeqCst) {
        return None;
    }
    let mine = KEY.load(Ordering::SeqCst);
    if !down {
        // the release of a press that was taken: swallowed, even if the pie is gone by now
        if code == mine && DOWN.swap(false, Ordering::SeqCst) {
            key_up(state, notifier);
            return Some(Verdict::Drop);
        }
        return None;
    }
    if code == mine && DOWN.load(Ordering::SeqCst) {
        return Some(Verdict::Drop); // the auto-repeat of a press that was taken: not a new one
    }
    let (general, app) = {
        let c = cfg();
        (c.key_trigger, c.key_trigger_app)
    };
    let which = which(general, app, code, mods)?;
    // taken: its repeats and its release are swallowed from here on
    KEY.store(code, Ordering::SeqCst);
    DOWN.store(true, Ordering::SeqCst);
    // pressed again while its pie stays open: the same combination closes it, nothing runs
    let pie = PIE.load(Ordering::SeqCst);
    if STICKY.load(Ordering::SeqCst) && pie != 0 && trigger::key_finish(state, notifier, pie, None)
    {
        over(pie);
        diag::log!("tap", "key_pie_closed", by = "combination");
        return Some(Verdict::Drop);
    }
    press(state, notifier, which);
    Some(Verdict::Drop)
}

/// The combination went down and is ours: opens the pie, or says why it does not.
fn press(state: &Arc<Mutex<Gesture>>, notifier: &Notifier, which: Which) {
    // the pointer: where the pie opens. Without it nothing can open.
    let Some((x, y)) = trigger::read_pointer() else {
        diag::log!("tap", "key_pie_no_pointer");
        return;
    };
    let p = Point::new(x, y);
    // Settings is open: the pie is off, as it is for every trigger (see `trigger::on_press`)
    if settings_open() {
        diag::log!("tap", "key_blocked", reason = "settings_open");
        notifier.send(Msg::PieBlocked {
            x: p.x,
            y: p.y,
            touch: false,
        });
        return;
    }
    // locked: no pie. Holding the combination and pressing Esc opens the lock (see `esc_lock`).
    if touch::lock::in_force() {
        diag::log!("tap", "key_blocked", reason = "locked");
        LOCKED_HOLD.store(true, Ordering::SeqCst);
        touch::lock::set_held(true);
        notifier.send(Msg::Hud(HudNote::locked()));
        return;
    }
    // another gesture runs (a mouse hold, a trackpad hold): it owns the pie
    if !matches!(&*super::lock(state), Gesture::Idle) {
        return;
    }
    let (hold_profile, app) = resolve_profiles(p);
    diag::log!(
        "tap",
        "key_open",
        x = p.x,
        y = p.y,
        profile = hold_profile,
        app = format!("{which:?}")
    );
    // the same warming up a press of the mouse trigger does (never blocks, see the tap rules)
    notifier.send(Msg::Warm {
        general: hold_profile,
        app,
    });
    running::warm(&trigger::profile_list(hold_profile, app));
    crate::smart::begin(&cfg());
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let profile = match which {
        // the intelligent pie, when it is kept, is the pie that opens (see `smart::opening`)
        Which::General => crate::smart::opening(id, hold_profile, &cfg().smart),
        Which::App => app.unwrap_or(hold_profile),
    };
    // the flags go up before the pie is asked for: the window reads them when it shows the pie
    PIE.store(id, Ordering::SeqCst);
    STICKY.store(false, Ordering::SeqCst);
    PRESSED_AT.store(super::now_ticks(), Ordering::SeqCst);
    ACTIVE.store(true, Ordering::SeqCst);
    if !trigger::key_open(state, notifier, id, p, profile) {
        over(id); // somebody else started a gesture in the meantime
        return;
    }
    watch(state, notifier, id);
}

/// The key came up. Depending on the mode the pie stays open, or the slice under the pointer runs.
fn key_up(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) {
    if LOCKED_HOLD.swap(false, Ordering::SeqCst) {
        touch::lock::set_held(false); // Esc no longer opens the lock
    }
    let id = PIE.load(Ordering::SeqCst);
    if id == 0 {
        return;
    }
    if !trigger::key_pie_is_open(state, id) {
        over(id); // the pie ended by itself (Settings, a pause): nothing to decide
        return;
    }
    let held = super::now_ticks().saturating_sub(PRESSED_AT.load(Ordering::SeqCst));
    let mode = cfg().key_trigger_mode;
    if stays_open(mode, held) {
        STICKY.store(true, Ordering::SeqCst);
        diag::log!("tap", "key_pie_sticky", held_ms = held);
        return;
    }
    // the pointer decides; if it cannot be read the pie only closes, nothing runs
    let at = trigger::read_pointer().map(|(x, y)| Point::new(x, y));
    trigger::key_finish(state, notifier, id, at);
    over(id);
    diag::log!("tap", "key_pie_released", held_ms = held);
}

/// Esc while a pie of the keyboard is open: the pie closes and nothing runs. It does **not** lock
/// CursorPie, unlike Esc in a pie of the mouse or the trackpad: the lock is opened by holding a
/// trigger and pressing Esc, and a keyboard pie that stays open has no trigger that is held. Returns
/// whether it was taken (the key is swallowed then).
pub(super) fn esc(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) -> bool {
    let id = PIE.load(Ordering::SeqCst);
    if id == 0 {
        return false;
    }
    let closed = trigger::key_finish(state, notifier, id, None);
    over(id);
    if closed {
        diag::log!("tap", "key_pie_closed", by = "esc");
    }
    closed
}

/// A press of the left button. While a pie of the keyboard is open it chooses: the slice under the
/// pointer runs and the pie closes (a click outside the pie, or in its middle, only closes it). The
/// click is swallowed with its drags and its release. `None` = not ours.
pub(super) fn click(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    kind: Kind,
    pos: Point,
) -> Option<Verdict> {
    if kind != Kind::Down {
        return None; // the rest of a click this took is swallowed by `pie_clicks`
    }
    let id = PIE.load(Ordering::SeqCst);
    if id == 0 || !trigger::key_finish(state, notifier, id, Some(pos)) {
        return None;
    }
    over(id);
    pie_clicks::swallow(touch::extra::LEFT_BUTTON);
    diag::log!("tap", "key_pie_clicked", x = pos.x, y = pos.y);
    Some(Verdict::Drop)
}

/// Starts the watcher of the pointer of the pie `id`: nothing reports the moves of a pointer while
/// no button is down, so it is read on a timer, as the trackpad hold does (`touch::watch_pointer`).
/// It ends by itself when the pie is gone. Tests move the pointer by hand and call [`follow`].
fn watch(state: &Arc<Mutex<Gesture>>, notifier: &Notifier, id: u64) {
    #[cfg(not(test))]
    {
        let (st, n) = (Arc::clone(state), notifier.clone());
        if !super::timers::after(WATCH_EVERY, move || tick(st, n, id, None)) {
            // without a watcher the pie could not be aimed: it must not be left open
            trigger::key_finish(state, notifier, id, None);
            over(id);
        }
    }
    #[cfg(test)]
    let _ = (state, notifier, id);
}

/// One step of the watcher: looks at the pointer and sets the next step.
#[cfg(not(test))]
fn tick(st: Arc<Mutex<Gesture>>, n: Notifier, id: u64, last: Option<(f64, f64)>) {
    let Some(now) = follow(&st, &n, id, last) else {
        over(id); // the pie is gone: released, closed, replaced
        return;
    };
    let (st2, n2) = (Arc::clone(&st), n.clone());
    if !super::timers::after(WATCH_EVERY, move || tick(st2, n2, id, now)) {
        trigger::key_finish(&st, &n, id, None);
        over(id);
    }
}

/// What one step of the watcher does: when the pie of gesture `id` is open, a pointer that moved
/// since `last` aims it. Returns the pointer to compare with next time, or `None` when the pie is
/// not open any more (the watcher stops).
fn follow(
    st: &Arc<Mutex<Gesture>>,
    n: &Notifier,
    id: u64,
    last: Option<(f64, f64)>,
) -> Option<Option<(f64, f64)>> {
    let now = trigger::read_pointer();
    match now {
        Some((x, y)) if now != last => {
            trigger::key_move(st, n, id, Point::new(x, y)).then_some(now)
        }
        _ => trigger::key_pie_is_open(st, id).then_some(now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, lock};
    use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT, key};
    use crate::tap::TEST_SERIAL;
    use crate::tap::gesture::Source;
    use std::sync::mpsc;

    const SPACE: i32 = key::SPACE as i32;
    const CHORD: Shortcut = Shortcut::new(key::SPACE, MOD_CTRL);
    const APP_CHORD: Shortcut = Shortcut::new(key::SPACE, MOD_CTRL | MOD_SHIFT);

    /// The factory config in the live one, with the keyboard trigger on, for one test; the person's
    /// own comes back afterwards, even if the test panics. Everything this module keeps in memory
    /// starts clean and ends clean.
    struct Rig(Option<Config>);

    impl Rig {
        fn new(mode: KeyPieMode) -> Self {
            let own = std::mem::take(&mut *cfg());
            {
                let mut c = cfg();
                c.key_trigger = Some(CHORD);
                c.key_trigger_app = Some(APP_CHORD);
                c.key_trigger_mode = mode;
                c.smart.enabled = false;
            }
            clear();
            RECORDING.store(false, Ordering::SeqCst);
            touch::lock::set(false);
            touch::lock::set_held(false);
            *super::super::trigger::TEST_POINTER
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            Rig(Some(own))
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            if let Some(own) = self.0.take() {
                *cfg() = own;
            }
            clear();
            touch::lock::set(false);
            touch::lock::set_held(false);
            *super::super::trigger::TEST_POINTER
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
            pie_clicks::clear();
            crate::tap::SETTINGS_OPEN.store(false, Ordering::SeqCst);
        }
    }

    fn point_at(x: f64, y: f64) {
        *super::super::trigger::TEST_POINTER
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some((x, y));
    }

    fn rig_parts() -> (Arc<Mutex<Gesture>>, Notifier, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        (
            Arc::new(Mutex::new(Gesture::Idle)),
            Notifier {
                tx,
                ctx: eframe::egui::Context::default(),
            },
            rx,
        )
    }

    fn msgs(rx: &mpsc::Receiver<Msg>) -> Vec<Msg> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    fn shown(m: &[Msg]) -> bool {
        m.iter().any(|m| matches!(m, Msg::ShowPie { .. }))
    }

    fn hidden(m: &[Msg]) -> bool {
        m.iter().any(|m| matches!(m, Msg::HidePie))
    }

    fn open_key_pie(st: &Arc<Mutex<Gesture>>) -> bool {
        matches!(
            &*lock(st),
            Gesture::ActivePie {
                source: Source::Key,
                ..
            }
        )
    }

    // ------------------------------------------------------------ the pure parts

    #[test]
    fn the_combination_is_matched_exactly() {
        assert!(matches(&CHORD, SPACE, MOD_CTRL));
        assert!(!matches(&CHORD, SPACE, MOD_CTRL | MOD_SHIFT), "one more");
        assert!(!matches(&CHORD, SPACE, 0), "none");
        assert!(!matches(&CHORD, SPACE + 1, MOD_CTRL), "another key");
        assert!(
            matches(&CHORD, SPACE, MOD_CTRL | 0x40),
            "bits that are not modifiers do not count"
        );
    }

    #[test]
    fn two_combinations_open_two_pies() {
        let (g, a) = (Some(CHORD), Some(APP_CHORD));
        assert_eq!(which(g, a, SPACE, MOD_CTRL), Some(Which::General));
        assert_eq!(which(g, a, SPACE, MOD_CTRL | MOD_SHIFT), Some(Which::App));
        assert_eq!(which(g, a, SPACE, MOD_ALT), None);
        assert_eq!(which(None, a, SPACE, MOD_CTRL), None);
        assert_eq!(which(g, None, SPACE, MOD_CTRL | MOD_SHIFT), None);
        assert_eq!(
            which(g, g, SPACE, MOD_CTRL),
            Some(Which::General),
            "the same twice: the general pie wins"
        );
        assert_eq!(which(None, None, SPACE, MOD_CMD), None);
    }

    #[test]
    fn the_mode_decides_whether_the_pie_stays_open() {
        assert!(!stays_open(KeyPieMode::Hold, 0));
        assert!(!stays_open(KeyPieMode::Hold, 10));
        assert!(stays_open(KeyPieMode::Toggle, 0));
        assert!(stays_open(KeyPieMode::Toggle, 10_000));
        assert!(stays_open(KeyPieMode::Auto, 0));
        assert!(stays_open(KeyPieMode::Auto, KEY_TAP_MS - 1));
        assert!(!stays_open(KeyPieMode::Auto, KEY_TAP_MS));
        assert!(!stays_open(KeyPieMode::Auto, 5000));
    }

    // ------------------------------------------------------------ the whole path

    #[test]
    fn the_combination_opens_the_pie_at_the_pointer_and_swallows_its_key() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(open_key_pie(&st));
        assert!(ACTIVE.load(Ordering::SeqCst));
        let m = msgs(&rx);
        assert!(shown(&m), "the UI is asked for the pie");
        assert!(
            m.iter()
                .any(|m| matches!(m, Msg::ShowPie { x, y, .. } if *x == 300.0 && *y == 200.0)),
            "at the pointer"
        );
        // its auto-repeat is swallowed and opens nothing new
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(msgs(&rx).is_empty());
    }

    #[test]
    fn other_keys_and_other_combinations_are_the_apps() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        let (st, n, rx) = rig_parts();
        point_at(1.0, 1.0);
        for (code, mods) in [
            (SPACE, 0),
            (SPACE, MOD_ALT),
            (SPACE, MOD_CTRL | MOD_ALT),
            (SPACE + 1, MOD_CTRL),
            (0, MOD_CMD),
        ] {
            assert_eq!(handle(&st, &n, true, code, mods), None, "{code} {mods}");
            assert_eq!(handle(&st, &n, false, code, mods), None);
        }
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(msgs(&rx).is_empty());
    }

    #[test]
    fn it_does_nothing_while_the_trigger_is_off() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        cfg().key_trigger = None;
        cfg().key_trigger_app = None;
        let (st, n, _rx) = rig_parts();
        point_at(1.0, 1.0);
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), None);
        assert!(matches!(*lock(&st), Gesture::Idle));
    }

    #[test]
    fn hold_runs_the_slice_under_the_pointer_when_the_key_comes_up() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        let _ = msgs(&rx);
        // far outside the pie: nothing is under the pointer, so the pie only closes
        point_at(900.0, 900.0);
        assert_eq!(
            handle(&st, &n, false, SPACE, MOD_CTRL),
            Some(Verdict::Drop),
            "the release is swallowed"
        );
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(hidden(&msgs(&rx)));
        assert!(!ACTIVE.load(Ordering::SeqCst));
        // and the next release is the app's again
        assert_eq!(handle(&st, &n, false, SPACE, MOD_CTRL), None);
    }

    #[test]
    fn hold_closes_without_running_when_the_pointer_cannot_be_read() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        let _ = msgs(&rx);
        *super::super::trigger::TEST_POINTER
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        handle(&st, &n, false, SPACE, MOD_CTRL);
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(hidden(&msgs(&rx)));
    }

    #[test]
    fn nothing_opens_when_the_pointer_cannot_be_read_but_the_key_is_still_taken() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        let (st, n, rx) = rig_parts();
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(msgs(&rx).is_empty());
        assert_eq!(
            handle(&st, &n, false, SPACE, MOD_CTRL),
            Some(Verdict::Drop),
            "and its release too"
        );
    }

    #[test]
    fn toggle_keeps_the_pie_open_after_the_key_comes_up() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Toggle);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        assert_eq!(handle(&st, &n, false, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(open_key_pie(&st), "still open");
        assert!(sticky_open(&st));
        assert!(!hidden(&msgs(&rx)));
        assert!(ACTIVE.load(Ordering::SeqCst));
    }

    #[test]
    fn auto_is_a_tap_when_quick_and_a_hold_when_slow() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Auto);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        // a quick tap: it stays
        handle(&st, &n, true, SPACE, MOD_CTRL);
        handle(&st, &n, false, SPACE, MOD_CTRL);
        assert!(sticky_open(&st), "a tap leaves the pie open");
        // close it, then hold longer than a tap
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle), "pressed again: closed");
        handle(&st, &n, false, SPACE, MOD_CTRL);
        let _ = msgs(&rx);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        std::thread::sleep(Duration::from_millis(KEY_TAP_MS + 30));
        handle(&st, &n, false, SPACE, MOD_CTRL);
        assert!(matches!(*lock(&st), Gesture::Idle), "a hold ends the pie");
    }

    #[test]
    fn the_combination_again_closes_a_pie_that_stayed_open_and_runs_nothing() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Toggle);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        handle(&st, &n, false, SPACE, MOD_CTRL);
        let _ = msgs(&rx);
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(hidden(&msgs(&rx)));
        assert!(!ACTIVE.load(Ordering::SeqCst));
        // the release of that second press is swallowed, and it does not open a pie
        assert_eq!(handle(&st, &n, false, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle));
        // the next press opens again
        handle(&st, &n, true, SPACE, MOD_CTRL);
        assert!(open_key_pie(&st));
    }

    #[test]
    fn the_other_combination_closes_it_too() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Toggle);
        let (st, n, _rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        handle(&st, &n, false, SPACE, MOD_CTRL);
        handle(&st, &n, true, SPACE, MOD_CTRL | MOD_SHIFT);
        assert!(matches!(*lock(&st), Gesture::Idle));
    }

    #[test]
    fn a_click_chooses_in_a_pie_that_stayed_open_and_is_swallowed() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Toggle);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        handle(&st, &n, false, SPACE, MOD_CTRL);
        let _ = msgs(&rx);
        // a click far outside: the pie closes, nothing runs, the click never reaches the app
        let v = click(&st, &n, Kind::Down, Point::new(900.0, 900.0));
        assert_eq!(v, Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(hidden(&msgs(&rx)));
        assert!(!ACTIVE.load(Ordering::SeqCst));
        // its release is swallowed with it (see `pie_clicks`)
        assert_eq!(
            pie_clicks::filter(&st, Kind::Up, touch::extra::LEFT_BUTTON, Verdict::Keep),
            Verdict::Drop
        );
        // and a click with no pie of the keyboard open is not ours
        assert_eq!(click(&st, &n, Kind::Down, Point::new(1.0, 1.0)), None);
    }

    #[test]
    fn a_click_also_chooses_while_the_key_is_still_held() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        let (st, n, _rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        assert_eq!(
            click(&st, &n, Kind::Down, Point::new(900.0, 900.0)),
            Some(Verdict::Drop)
        );
        assert!(matches!(*lock(&st), Gesture::Idle));
        // the key that is still down: its release is swallowed and decides nothing
        assert_eq!(handle(&st, &n, false, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle));
    }

    #[test]
    fn esc_closes_the_pie_without_locking_cursorpie() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Toggle);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        handle(&st, &n, false, SPACE, MOD_CTRL);
        let _ = msgs(&rx);
        assert!(esc(&st, &n));
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(hidden(&msgs(&rx)));
        assert!(!touch::lock::in_force(), "the lock did not close");
        assert!(!esc(&st, &n), "with no pie open Esc is not ours");
    }

    #[test]
    fn esc_in_a_pie_of_the_keyboard_goes_through_the_esc_handler_and_does_not_lock() {
        use crate::tap::esc_lock;
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Toggle);
        cfg().esc_lock = true;
        let (st, n, _rx) = rig_parts();
        esc_lock::handle(&st, &n, false, i32::from(key::ESCAPE), 0); // no Esc is "taken" yet
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        handle(&st, &n, false, SPACE, MOD_CTRL);
        let v = esc_lock::handle(&st, &n, true, i32::from(key::ESCAPE), 0);
        assert_eq!(v, Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(!touch::lock::in_force());
        // its release is swallowed too
        assert_eq!(
            esc_lock::handle(&st, &n, false, i32::from(key::ESCAPE), 0),
            Some(Verdict::Drop)
        );
    }

    #[test]
    fn the_pointer_aims_the_pie_and_the_watcher_stops_with_it() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Toggle);
        let (st, n, _rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL);
        let id = PIE.load(Ordering::SeqCst);
        assert_ne!(id, 0);
        // the pointer did not move: the pie stays, and the same pointer is remembered
        assert_eq!(
            follow(&st, &n, id, Some((300.0, 200.0))),
            Some(Some((300.0, 200.0)))
        );
        // it moved: the pie is aimed (the hover is read from the pointer)
        point_at(330.0, 200.0);
        assert_eq!(
            follow(&st, &n, id, Some((300.0, 200.0))),
            Some(Some((330.0, 200.0)))
        );
        // another gesture's number, or no pie: the watcher stops
        assert_eq!(follow(&st, &n, id + 1, None), None);
        esc(&st, &n);
        assert_eq!(follow(&st, &n, id, None), None);
    }

    #[test]
    fn settings_open_blocks_the_pie_and_says_so() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        crate::tap::SETTINGS_OPEN.store(true, Ordering::SeqCst);
        let (st, n, rx) = rig_parts();
        point_at(10.0, 20.0);
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle));
        let m = msgs(&rx);
        assert!(m.iter().any(|m| matches!(m, Msg::PieBlocked { .. })));
        assert!(!shown(&m));
    }

    #[test]
    fn a_locked_cursorpie_opens_no_pie_and_esc_opens_the_lock_while_the_combination_is_held() {
        use crate::tap::esc_lock;
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        cfg().esc_lock = true;
        touch::lock::set(true);
        let (st, n, rx) = rig_parts();
        point_at(10.0, 20.0);
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(!shown(&msgs(&rx)));
        assert!(touch::lock::held(), "held: Esc may open the lock now");
        // Esc while it is held opens the lock
        let v = esc_lock::handle(&st, &n, true, i32::from(key::ESCAPE), 0);
        assert_eq!(v, Some(Verdict::Drop));
        assert!(!touch::lock::in_force());
        // letting go ends the hold
        handle(&st, &n, false, SPACE, MOD_CTRL);
        assert!(!touch::lock::held());
        // (the release of Esc: the handler forgets that it took the key)
        esc_lock::handle(&st, &n, false, i32::from(key::ESCAPE), 0);
    }

    #[test]
    fn a_running_mouse_gesture_keeps_the_pie_and_the_combination_adds_nothing() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        let (st, n, rx) = rig_parts();
        *lock(&st) = Gesture::Passthrough;
        point_at(10.0, 20.0);
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), Some(Verdict::Drop));
        assert!(matches!(*lock(&st), Gesture::Passthrough));
        assert!(!shown(&msgs(&rx)));
        assert!(!ACTIVE.load(Ordering::SeqCst));
    }

    #[test]
    fn the_recorder_of_settings_gets_the_combination() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        RECORDING.store(true, Ordering::SeqCst);
        let (st, n, _rx) = rig_parts();
        point_at(10.0, 20.0);
        assert_eq!(handle(&st, &n, true, SPACE, MOD_CTRL), None);
        RECORDING.store(false, Ordering::SeqCst);
    }

    #[test]
    fn the_app_combination_opens_the_pie_of_the_app_when_it_has_one_and_the_general_one_if_not() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _r = Rig::new(KeyPieMode::Hold);
        let (st, n, rx) = rig_parts();
        point_at(300.0, 200.0);
        handle(&st, &n, true, SPACE, MOD_CTRL | MOD_SHIFT);
        // in the tests no app is in front: the general pie opens
        let general = cfg().global_profile();
        let m = msgs(&rx);
        assert!(
            m.iter()
                .any(|m| matches!(m, Msg::ShowPie { profile, .. } if *profile == general)),
            "the general pie when the app has none"
        );
    }
}
