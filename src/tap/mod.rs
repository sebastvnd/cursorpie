//! Global mouse/keyboard hook and the gesture state machine.
//!
//! The hook runs on its own thread (`cursorpie-tap`). Which hook depends on the OS and lives in a
//! small native module: `macos` (`CGEventTap` + `CFRunLoop`) or `windows` (`WH_MOUSE_LL` and
//! `WH_KEYBOARD_LL` + a message loop). The native layer turns each event into an `Ev` and calls
//! `dispatch`, which routes it and answers with a [`Verdict`]:
//!
//! | Event | Handled by |
//! |---|---|
//! | pause | the top of `dispatch` |
//! | key down / up | `keyboard`: only used to record a shortcut in Settings |
//! | a click while a trackpad pie is open | `touch::extra`: one more finger clicks (macOS trackpad, Windows precision touchpad) |
//! | right button | `right_click`: switches the side-button pie while a pie is open |
//! | side buttons (3, 4) and the extra buttons a mode uses (5 and up) | `side_buttons`: run the active mode's actions |
//! | any extra button while Settings scans the mouse | the top of `dispatch`: reported and swallowed |
//! | the trigger button | `trigger`: the gesture state machine (`gesture::Gesture`) |
//!
//! `resolve` answers "what does this gesture mean": hovered slice, profile, target app, selection.
//! Button numbers are the macOS ones on every OS (0 left, 1 right, 2 middle, 3 back, 4 forward).
//!
//! **Safety rules for an active hook:** never block in the callback, never panic out of it (the
//! native layer wraps `dispatch` in `catch_unwind` and fails open), and pass events through when
//! unsure. The full list is in
//! [`guides::architecture`](crate::guides::architecture#rules-for-the-event-tap-callback).

mod gesture;
mod keyboard;
mod pie_clicks;
mod resolve;
mod right_click;
mod side_buttons;
mod trigger;
// The trackpad source runs on macOS, Windows and Linux; its portable core is built and tested everywhere.
#[cfg_attr(
    not(any(target_os = "macos", windows, target_os = "linux", test)),
    allow(dead_code)
)]
mod touch;

pub use touch::Status as TouchStatus;

/// Whether a pie that a trackpad hold opened is wanted right now (a hold is armed or open). The
/// pie window asks when it is shown: such a pie takes the pointer instead of letting it through.
/// Never for the virtual trackpad's: its pie is nowhere near the real pointer, which is the finger
/// on the trackpad's window and must stay there.
pub(crate) fn touch_pie_active() -> bool {
    touch::ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
        && !crate::platform::virtual_pointer_active()
}

/// What the touch source is doing, for the Trackpad panel of Settings.
pub fn touch_status() -> TouchStatus {
    touch::status()
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as native;

// Also compiled with `--features check-windows` so it can be type-checked and its pure helpers
// tested on other OSes.
#[cfg(any(windows, feature = "check-windows"))]
#[cfg_attr(not(windows), allow(dead_code))]
mod windows;
#[cfg(windows)]
use windows as native;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as native;

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod native {
    //! No hook on this OS (the test stub): nothing is ever captured.
    use crate::msg::Notifier;

    /// Does nothing; `TAP_ACTIVE` stays false.
    pub fn spawn(_notifier: Notifier) {}
}

use crate::config::{cfg, lock};
use crate::geometry::Point;
use crate::msg::{Msg, Notifier};
use gesture::Gesture;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Button number of the left button (only the trackpad's extra click looks at it).
const LEFT_BUTTON: i64 = touch::extra::LEFT_BUTTON;
/// Button number of the right button.
const RIGHT_BUTTON: i64 = 1;
/// Button number of the "back" side button.
const SIDE_BACK: i64 = crate::mouse::BACK;
/// Button number of the "forward" side button.
const SIDE_FORWARD: i64 = crate::mouse::FORWARD;
/// Movement (px) before a press counts as a drag and is handed back to the app.
const DRAG_TOLERANCE: f64 = 6.0;
/// Same, when "flick" is enabled: the pie opens instead.
const FLICK_DISTANCE: f64 = 14.0;

/// true = the next key press is captured by the tap (recording a shortcut in settings)
pub static RECORDING: AtomicBool = AtomicBool::new(false);
/// keycode being swallowed until its key-up so it doesn't leak to apps (-1 = none)
pub static SWALLOW_KEY: AtomicI32 = AtomicI32::new(-1);
/// Pause switch from the menu bar: everything passes through while true.
pub static PAUSED: AtomicBool = AtomicBool::new(false);
/// true while the tap is installed and running.
pub static TAP_ACTIVE: AtomicBool = AtomicBool::new(false);
/// true while Settings (or the welcome window) is open. The UI keeps it in step with
/// `App::settings_open` (see `App::set_settings_open`). While it is true the pie is off: a press of
/// the trigger starts no gesture, so nothing can open, hover or run, and the side buttons are left
/// to the app. Without it the gesture state machine, which lives on the tap thread and knows
/// nothing of the UI, kept running behind the Settings window and released slices ran their actions.
pub static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);

/// The statics above are process-wide, so the tests of `tap` and `tap::touch` that touch them run
/// one at a time, all behind this one lock.
#[cfg(test)]
pub(crate) static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// One finger on a trackpad, as the virtual trackpad reports it (see [`virtual_feed`]).
pub use touch::recognizer::Finger as TouchFinger;
/// A contact larger than this is a palm, not a finger.
pub const TOUCH_PALM_SIZE: f32 = touch::recognizer::PALM_SIZE;

/// Whether the virtual trackpad can be used: the tap, whose gesture it feeds, has started.
pub fn virtual_ready() -> bool {
    touch::virtual_ready()
}

/// One frame of the virtual trackpad: where its fingers are (`size_mm` is the surface it stands
/// for). An empty slice lifts every finger. False when the tap has not started.
pub fn virtual_feed(size_mm: (f32, f32), fingers: &[TouchFinger]) -> bool {
    touch::virtual_feed(size_mm, fingers)
}

/// Lets time pass for the virtual trackpad (the hold time of a finger that keeps still).
pub fn virtual_poll() {
    touch::virtual_poll();
}

/// Cancels the gesture the virtual trackpad is running, if any.
pub fn virtual_reset() {
    touch::virtual_reset();
}

/// Whether Settings is open, i.e. whether the pie must stay off. See [`SETTINGS_OPEN`].
pub(crate) fn settings_open() -> bool {
    SETTINGS_OPEN.load(Ordering::SeqCst)
}

/// Source of gesture ids; each press gets a new one so stale timers can recognise themselves.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// What a mouse button did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Pressed.
    Down,
    /// Released.
    Up,
    /// The pointer moved while the button was held.
    Drag,
}

/// What the hook does with an event: pass it on to the app or swallow it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Let the event through.
    Keep,
    /// Swallow it.
    Drop,
}

/// One event as seen by the portable state machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Ev {
    /// A key press or release. `code` is a macOS virtual key code, `mods` a `MOD_*` bitmask.
    Key { down: bool, code: i32, mods: u8 },
    /// A mouse button event at `pos` (logical points).
    Mouse { kind: Kind, button: i64, pos: Point },
}

/// `cursorpie --touchpad-debug [seconds]` (Linux): prints why the touchpad does or does not work
/// and shows the fingers live. `true` = it ran, so the app must not start.
#[cfg(target_os = "linux")]
pub fn touchpad_debug_from_args() -> bool {
    touch::touchpad_debug_from_args()
}

/// Starts the hook on its own thread (retrying while the OS refuses it).
pub fn spawn(notifier: Notifier) {
    native::spawn(notifier);
}

/// Routes one native event. `replay_drag` re-posts the original drag event to the app; it is
/// called when a press turns out to be a real drag that has to be handed back (macOS only needs it).
pub(crate) fn dispatch(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    ev: Ev,
    replay_drag: &dyn Fn(),
) -> Verdict {
    if PAUSED.load(Ordering::SeqCst) {
        let mut g = lock(state);
        if !matches!(*g, Gesture::Idle) {
            *g = Gesture::Idle;
            drop(g);
            notifier.send(Msg::HidePie);
        }
        return Verdict::Keep;
    }
    match ev {
        Ev::Key { down, code, mods } => {
            if down {
                touch::note_key(); // the typing guard of the trackpad hold
            }
            keyboard::handle_key(notifier, down, code, mods)
        }
        Ev::Mouse { kind, button, pos } => {
            let verdict = mouse_verdict(state, notifier, kind, button, pos, replay_drag);
            // the last word: while a pie is open nothing it did not take reaches the app under it
            pie_clicks::filter(state, kind, button, verdict)
        }
    }
}

/// What the paths of the state machine decide for one mouse button event.
fn mouse_verdict(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    kind: Kind,
    button: i64,
    pos: Point,
    replay_drag: &dyn Fn(),
) -> Verdict {
    // a click while a trackpad pie is open may be the extra click (one more finger)
    if (button == LEFT_BUTTON || button == RIGHT_BUTTON)
        && let Some(v) = touch::extra::handle(state, notifier, kind)
    {
        return v;
    }
    if button == RIGHT_BUTTON {
        return right_click::handle_right(state, notifier, kind);
    }
    // Settings is scanning the mouse: say which button it was and let nothing else happen
    if crate::mouse::is_extra(button) && crate::mouse::scanning() {
        if kind == Kind::Down {
            notifier.send(Msg::ButtonScanned(button));
        }
        return Verdict::Drop;
    }
    if button == SIDE_BACK || button == SIDE_FORWARD {
        return side_buttons::handle_side(state, kind, button, pos);
    }
    let (trigger, flick) = {
        let c = cfg();
        (c.trigger_button, c.flick)
    };
    if button != trigger {
        if side_buttons::is_mapped(button) {
            return side_buttons::handle_side(state, kind, button, pos);
        }
        return Verdict::Keep;
    }
    trigger::handle(state, notifier, kind, pos, button, flick, replay_drag)
}

/// Resets everything after the OS disabled the hook (a timeout, or the user): no gesture is in
/// progress any more and the pie is closed.
// Only the macOS tap calls this (the OS can disable its hook); the tests call it everywhere.
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) fn reset(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) {
    *lock(state) = Gesture::Idle;
    touch::extra::clear();
    pie_clicks::clear();
    touch::ACTIVE.store(false, Ordering::SeqCst);
    SWALLOW_KEY.store(-1, Ordering::SeqCst);
    notifier.send(Msg::HidePie);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::MOD_CMD;
    use std::sync::MutexGuard;
    use std::sync::mpsc;

    /// The statics above are process-wide, so tests that touch them run one at a time.
    fn serial() -> MutexGuard<'static, ()> {
        TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sets [`SETTINGS_OPEN`] for one test and clears it afterwards, even if the test panics.
    struct SettingsOpen;

    impl SettingsOpen {
        fn set() -> Self {
            SETTINGS_OPEN.store(true, Ordering::SeqCst);
            SettingsOpen
        }
    }

    impl Drop for SettingsOpen {
        fn drop(&mut self) {
            SETTINGS_OPEN.store(false, Ordering::SeqCst);
        }
    }

    fn notifier() -> (Notifier, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        (
            Notifier {
                tx,
                ctx: eframe::egui::Context::default(),
            },
            rx,
        )
    }

    fn idle() -> Arc<Mutex<Gesture>> {
        Arc::new(Mutex::new(Gesture::Idle))
    }

    fn key(down: bool, code: i32, mods: u8) -> Ev {
        Ev::Key { down, code, mods }
    }

    #[test]
    fn keys_pass_through_unless_recording() {
        let _s = serial();
        RECORDING.store(false, Ordering::SeqCst);
        SWALLOW_KEY.store(-1, Ordering::SeqCst);
        let (n, rx) = notifier();
        let st = idle();
        assert_eq!(
            dispatch(&st, &n, key(true, 6, MOD_CMD), &|| {}),
            Verdict::Keep
        );
        assert_eq!(
            dispatch(&st, &n, key(false, 6, MOD_CMD), &|| {}),
            Verdict::Keep
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn recording_captures_one_key_and_swallows_its_release() {
        let _s = serial();
        SWALLOW_KEY.store(-1, Ordering::SeqCst);
        RECORDING.store(true, Ordering::SeqCst);
        let (n, rx) = notifier();
        let st = idle();
        assert_eq!(
            dispatch(&st, &n, key(true, 6, MOD_CMD), &|| {}),
            Verdict::Drop
        );
        match rx.try_recv() {
            Ok(Msg::KeyCaptured(s)) => assert_eq!((s.key, s.mods), (6, MOD_CMD)),
            _ => panic!("expected the captured shortcut"),
        }
        assert!(!RECORDING.load(Ordering::SeqCst));
        // auto-repeat and the release of that key never reach the app
        assert_eq!(
            dispatch(&st, &n, key(true, 6, MOD_CMD), &|| {}),
            Verdict::Drop
        );
        assert_eq!(
            dispatch(&st, &n, key(false, 6, MOD_CMD), &|| {}),
            Verdict::Drop
        );
        assert_eq!(SWALLOW_KEY.load(Ordering::SeqCst), -1);
        assert_eq!(dispatch(&st, &n, key(true, 6, 0), &|| {}), Verdict::Keep);
    }

    #[test]
    fn plain_escape_cancels_recording() {
        let _s = serial();
        SWALLOW_KEY.store(-1, Ordering::SeqCst);
        RECORDING.store(true, Ordering::SeqCst);
        let (n, rx) = notifier();
        let st = idle();
        assert_eq!(dispatch(&st, &n, key(true, 53, 0), &|| {}), Verdict::Drop);
        assert!(matches!(rx.try_recv(), Ok(Msg::RecordCancelled)));
        SWALLOW_KEY.store(-1, Ordering::SeqCst);
    }

    #[test]
    fn pause_resets_the_gesture_and_passes_everything() {
        let _s = serial();
        PAUSED.store(true, Ordering::SeqCst);
        let (n, rx) = notifier();
        let st = Arc::new(Mutex::new(Gesture::InSettings(gesture::Source::Mouse)));
        let ev = Ev::Mouse {
            kind: Kind::Down,
            button: 2,
            pos: Point::new(1.0, 1.0),
        };
        assert_eq!(dispatch(&st, &n, ev, &|| {}), Verdict::Keep);
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(matches!(rx.try_recv(), Ok(Msg::HidePie)));
        PAUSED.store(false, Ordering::SeqCst);
    }

    #[test]
    fn buttons_that_are_not_the_trigger_pass_through() {
        let _s = serial();
        PAUSED.store(false, Ordering::SeqCst);
        let (n, rx) = notifier();
        let st = idle();
        for button in [0, 5, 9] {
            for kind in [Kind::Down, Kind::Up, Kind::Drag] {
                let ev = Ev::Mouse {
                    kind,
                    button,
                    pos: Point::new(0.0, 0.0),
                };
                assert_eq!(
                    dispatch(&st, &n, ev, &|| {}),
                    Verdict::Keep,
                    "{button} {kind:?}"
                );
            }
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn extra_buttons_are_swallowed_and_reported_only_while_scanning() {
        let _s = serial();
        PAUSED.store(false, Ordering::SeqCst);
        let (n, rx) = notifier();
        let st = idle();
        let ev = |kind| Ev::Mouse {
            kind,
            button: 6,
            pos: Point::new(0.0, 0.0),
        };
        let scan = crate::mouse::Scan::start();
        assert_eq!(dispatch(&st, &n, ev(Kind::Down), &|| {}), Verdict::Drop);
        assert!(matches!(rx.try_recv(), Ok(Msg::ButtonScanned(6))));
        // the release and the drag are swallowed too, but only the press is reported
        assert_eq!(dispatch(&st, &n, ev(Kind::Up), &|| {}), Verdict::Drop);
        assert_eq!(dispatch(&st, &n, ev(Kind::Drag), &|| {}), Verdict::Drop);
        assert!(rx.try_recv().is_err());
        // a button that is not an extra one is not part of a scan
        let left = Ev::Mouse {
            kind: Kind::Down,
            button: 0,
            pos: Point::new(0.0, 0.0),
        };
        assert_eq!(dispatch(&st, &n, left, &|| {}), Verdict::Keep);
        drop(scan);
        assert_eq!(dispatch(&st, &n, ev(Kind::Down), &|| {}), Verdict::Keep);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_right_click_without_a_pie_is_a_normal_click() {
        let _s = serial();
        PAUSED.store(false, Ordering::SeqCst);
        let (n, _rx) = notifier();
        let st = idle();
        for kind in [Kind::Down, Kind::Up, Kind::Drag] {
            let ev = Ev::Mouse {
                kind,
                button: RIGHT_BUTTON,
                pos: Point::new(0.0, 0.0),
            };
            assert_eq!(dispatch(&st, &n, ev, &|| {}), Verdict::Keep);
        }
    }

    #[test]
    fn reset_closes_the_pie_and_clears_state() {
        let _s = serial();
        SWALLOW_KEY.store(12, Ordering::SeqCst);
        let (n, rx) = notifier();
        let st = Arc::new(Mutex::new(Gesture::Passthrough));
        reset(&st, &n);
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert_eq!(SWALLOW_KEY.load(Ordering::SeqCst), -1);
        assert!(matches!(rx.try_recv(), Ok(Msg::HidePie)));
    }

    fn trigger_event(kind: Kind) -> Ev {
        let button = cfg().trigger_button;
        Ev::Mouse {
            kind,
            button,
            pos: Point::new(10.0, 10.0),
        }
    }

    #[test]
    fn a_press_while_settings_is_open_starts_no_gesture_and_runs_nothing() {
        let _s = serial();
        PAUSED.store(false, Ordering::SeqCst);
        let _open = SettingsOpen::set();
        let (n, rx) = notifier();
        let st = idle();

        // the press is swallowed with its whole gesture, but only says why nothing happens
        assert_eq!(
            dispatch(&st, &n, trigger_event(Kind::Down), &|| {}),
            Verdict::Drop
        );
        assert!(matches!(*lock(&st), Gesture::InSettings(_)));
        assert!(matches!(rx.try_recv(), Ok(Msg::PieBlocked { .. })));
        assert_eq!(
            dispatch(&st, &n, trigger_event(Kind::Drag), &|| {}),
            Verdict::Drop
        );
        // no hold timer was started: wait past the default pie hold and check nothing opened
        let hold_ms = cfg().pie_hold_ms;
        std::thread::sleep(std::time::Duration::from_millis(hold_ms + 100));
        assert!(matches!(*lock(&st), Gesture::InSettings(_)));
        assert!(rx.try_recv().is_err(), "no Warm, no ShowPie, nothing");
        assert_eq!(
            dispatch(&st, &n, trigger_event(Kind::Up), &|| {}),
            Verdict::Drop
        );
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn the_pie_works_again_once_settings_is_closed() {
        let _s = serial();
        PAUSED.store(false, Ordering::SeqCst);
        {
            let _open = SettingsOpen::set();
            let (n, _rx) = notifier();
            let st = idle();
            dispatch(&st, &n, trigger_event(Kind::Down), &|| {});
            dispatch(&st, &n, trigger_event(Kind::Up), &|| {});
        }
        let (n, rx) = notifier();
        let st = idle();
        assert_eq!(
            dispatch(&st, &n, trigger_event(Kind::Down), &|| {}),
            Verdict::Drop
        );
        assert!(matches!(*lock(&st), Gesture::Pending { .. }));
        assert!(matches!(rx.try_recv(), Ok(Msg::Warm { .. })));
    }

    #[test]
    fn side_buttons_send_no_action_while_settings_is_open() {
        let _s = serial();
        PAUSED.store(false, Ordering::SeqCst);
        let _open = SettingsOpen::set();
        let (n, rx) = notifier();
        let st = idle();
        for button in [SIDE_BACK, SIDE_FORWARD] {
            for kind in [Kind::Down, Kind::Drag, Kind::Up] {
                let ev = Ev::Mouse {
                    kind,
                    button,
                    pos: Point::new(0.0, 0.0),
                };
                assert_eq!(
                    dispatch(&st, &n, ev, &|| {}),
                    Verdict::Keep,
                    "{button} {kind:?}"
                );
            }
        }
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_pie_that_was_open_when_settings_opened_runs_nothing_on_release() {
        let _s = serial();
        PAUSED.store(false, Ordering::SeqCst);
        // one lock at a time: the config mutex is not reentrant
        let (button, size, count, before) = {
            let c = cfg();
            (
                c.trigger_button,
                c.pie_size,
                c.profiles[0].items.len(),
                c.profiles[0].active.clone(),
            )
        };
        // a point over a Mode slice other than the active one: picking it would change the mode
        let pick = (0..count)
            .find(|&i| resolve::is_mode(0, &[], i) && before != vec![i])
            .expect("a mode slice that is not the active one");
        let center = Point::new(100.0, 100.0);
        let inner = crate::geometry::pie_inner(size);
        let target = (0..360)
            .map(|deg| {
                let a = f64::from(deg).to_radians();
                let r = f64::from(inner) + 20.0;
                Point::new(center.x + r * a.cos(), center.y + r * a.sin())
            })
            .find(|&p| crate::geometry::slice_at(center, p, count, inner) == Some(pick))
            .expect("a point over that slice");

        let _open = SettingsOpen::set();
        let (n, rx) = notifier();
        let st = Arc::new(Mutex::new(Gesture::ActivePie {
            id: 1,
            center,
            profile: 0,
            path: Vec::new(),
            hover: None,
            source: gesture::Source::Mouse,
        }));
        let up = Ev::Mouse {
            kind: Kind::Up,
            button,
            pos: target,
        };
        assert_eq!(dispatch(&st, &n, up, &|| {}), Verdict::Drop);
        assert!(matches!(*lock(&st), Gesture::Idle));
        assert!(matches!(rx.try_recv(), Ok(Msg::HidePie)));
        let after = cfg().profiles[0].active.clone();
        cfg().profiles[0].active = before.clone(); // put the live config back either way (in memory only)
        assert_eq!(
            after, before,
            "the slice must not be applied while Settings is open"
        );
    }
}
