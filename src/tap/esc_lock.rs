//! Esc locks CursorPie, and unlocks it again: the lock of the trackpad
//! ([`touch::lock`](super::touch)) for every trigger, the mouse button and the keyboard as well.
//!
//! ```text
//! a pie is open (any trigger) ── Esc ──► the pie closes, nothing runs, the padlock snaps shut
//! locked ── a press of the mouse trigger / a hold on the trackpad ──► no pie, a notice says why
//! locked, and the trigger is held ── Esc ──► the padlock opens and the pie opens at once
//! locked, and the mouse trigger is held ── left click ──► the same as Esc
//! ```
//!
//! Esc is the app's key at every other time, so it is taken in these two moments only:
//!
//! - **a pie is open** (the gesture is `ActivePie`, whoever opened it): Esc closes it and locks;
//! - **the lock is on and a trigger is held** ([`touch::lock::held`](super::touch)): Esc unlocks.
//!   A press of the mouse trigger while locked goes to the app as if CursorPie were not there
//!   ([`locked_hold`] only starts the timer of the notice), and a hold on the trackpad opens no
//!   pie; both set the flag until the button or the fingers are up.
//!
//! A key that is taken is swallowed together with its auto-repeat and its key-up, so it never
//! reaches the app under the pie. Everything here runs on the tap thread and never blocks.
//!
//! The setting [`Config::esc_lock`](crate::config::Config::esc_lock) (on by default) turns it off.
//! The lock itself is one flag in memory, shared with the three-finger tap of the trackpad.

use super::gesture::Gesture;
use super::{Kind, RECORDING, Verdict, pie_clicks, touch, trigger};
use crate::config::{cfg, lock};
use crate::geometry::Point;
use crate::keys::key;
use crate::msg::{Msg, Notifier};
use crate::ui::hud::HudNote;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Esc is down and CursorPie took its press (its auto-repeat and its key-up are swallowed too).
static HELD: AtomicBool = AtomicBool::new(false);
/// The token of the hold that may still show the notice (see [`locked_hold`]).
static HOLD_TOKEN: AtomicU64 = AtomicU64::new(0);
/// Hands out tokens.
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(0);

/// What Esc does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// Not ours: the key goes on to the app.
    Pass,
    /// Close the open pie and lock.
    Lock,
    /// Open the lock.
    Unlock,
}

/// Decides what a plain Esc does. Pure, so it is unit tested. `enabled` is the setting, `locked`
/// whether the lock is in force, `pie_open` whether a pie is open, `trigger_held` whether a trigger
/// is held down on the lock.
pub(crate) fn decide(enabled: bool, locked: bool, pie_open: bool, trigger_held: bool) -> Step {
    if !enabled {
        return Step::Pass;
    }
    if locked {
        if trigger_held {
            Step::Unlock
        } else {
            Step::Pass
        }
    } else if pie_open {
        Step::Lock
    } else {
        Step::Pass
    }
}

/// Looks at one key event. `Some(verdict)` = Esc was taken here and the verdict is final; `None` =
/// not ours, the normal key handling goes on.
pub(super) fn handle(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    down: bool,
    code: i32,
    mods: u8,
) -> Option<Verdict> {
    if code != i32::from(key::ESCAPE) {
        return None;
    }
    if !down {
        // the key-up of a press that was taken: swallowed, even if the pie is gone by now
        return HELD.swap(false, Ordering::SeqCst).then_some(Verdict::Drop);
    }
    if HELD.load(Ordering::SeqCst) {
        return Some(Verdict::Drop); // auto-repeat of a taken press: not a new one
    }
    if mods != 0 || RECORDING.load(Ordering::SeqCst) {
        return None; // Esc with a modifier is the app's, and Settings is recording a shortcut
    }
    let enabled = cfg().esc_lock;
    let locked = touch::lock::in_force();
    // the look at the gesture ends here, before anything below takes the lock of the state again
    let pie_open = matches!(*lock(state), Gesture::ActivePie { .. });
    match decide(enabled, locked, pie_open, touch::lock::held()) {
        Step::Pass => None,
        Step::Lock => {
            // the pie closes without running anything; if it is gone by now, Esc is not ours
            let at = trigger::esc_lock(state, notifier)?;
            touch::lock::set(true);
            crate::smart::end_gesture();
            HELD.store(true, Ordering::SeqCst);
            diag::log!("tap", "esc_lock", x = at.x, y = at.y);
            notifier.send(Msg::LockAnim {
                x: at.x,
                y: at.y,
                locked: true,
            });
            Some(Verdict::Drop)
        }
        Step::Unlock => {
            touch::lock::set(false);
            HELD.store(true, Ordering::SeqCst);
            diag::log!("tap", "esc_unlock");
            unlocked(state, notifier);
            Some(Verdict::Drop)
        }
    }
}

/// The lock was just opened while a trigger is held: a mouse trigger that is held opens the pie at
/// once (the padlock opens in its middle); anything else (a trackpad hold has its own two-finger
/// way, see [`touch`]) only shows the padlock opening, and a pill.
fn unlocked(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) {
    let at = trigger::read_pointer();
    if let Some((x, y)) = at
        && trigger::open_after_unlock(state, notifier, Point::new(x, y))
    {
        return;
    }
    match at {
        Some((x, y)) => notifier.send(Msg::LockAnim {
            x,
            y,
            locked: false,
        }),
        None => notifier.send(Msg::Hud(HudNote::unlocked())),
    }
}

/// A press of the left button while the locked mouse trigger is held opens the lock, as Esc does:
/// for a hand that is on the mouse and not on the keyboard. The click is taken (its drags and its
/// release too, see [`pie_clicks::swallow`]) and never reaches the app. Returns whether it was
/// taken here.
pub(super) fn click_unlock(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    kind: Kind,
    button: i64,
    p: Point,
) -> bool {
    if kind != Kind::Down || button != touch::extra::LEFT_BUTTON {
        return false;
    }
    if RECORDING.load(Ordering::SeqCst) || !cfg().esc_lock {
        return false;
    }
    // only the hold of the mouse trigger that began on the lock (`Passthrough`): the held flag is
    // the trackpad's as well, and that one has its own gesture
    if !(touch::lock::in_force() && touch::lock::held()) {
        return false;
    }
    if !matches!(&*lock(state), Gesture::Passthrough) {
        return false;
    }
    touch::lock::set(false);
    pie_clicks::swallow(button);
    diag::log!("tap", "click_unlock");
    if !trigger::open_after_unlock(state, notifier, p) {
        notifier.send(Msg::LockAnim {
            x: p.x,
            y: p.y,
            locked: false,
        });
    }
    true
}

/// The mouse trigger was pressed while locked (its press went to the app): marks the hold, and once
/// the hold time has passed, if the button is still down and the lock still on, the notice says
/// why no pie came, like a hold on the trackpad does.
pub(super) fn locked_hold(notifier: &Notifier) {
    touch::lock::set_held(true);
    let token = NEXT_TOKEN.fetch_add(1, Ordering::SeqCst) + 1;
    HOLD_TOKEN.store(token, Ordering::SeqCst);
    let ms = cfg().pie_hold_ms;
    let n = notifier.clone();
    let _ = super::timers::after(Duration::from_millis(ms), move || {
        if HOLD_TOKEN.load(Ordering::SeqCst) == token
            && touch::lock::held()
            && touch::lock::in_force()
        {
            n.send(Msg::Hud(HudNote::locked()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tap::TEST_SERIAL;
    use crate::tap::gesture::Source;
    use std::sync::mpsc;

    #[test]
    fn esc_locks_only_an_open_pie_and_unlocks_only_a_held_trigger() {
        assert_eq!(decide(true, false, true, false), Step::Lock);
        assert_eq!(decide(true, false, false, false), Step::Pass);
        assert_eq!(decide(true, true, false, true), Step::Unlock);
        assert_eq!(
            decide(true, true, false, false),
            Step::Pass,
            "locked, but no trigger held: Esc is the app's"
        );
        assert_eq!(
            decide(true, true, true, true),
            Step::Unlock,
            "the lock wins over an open pie"
        );
        assert_eq!(decide(false, false, true, false), Step::Pass);
        assert_eq!(decide(false, true, false, true), Step::Pass);
    }

    /// Puts the factory config in the live one for one test and gives the person's own back after.
    struct Factory(Option<Config>);

    impl Factory {
        fn set() -> Self {
            Factory(Some(std::mem::take(&mut *cfg())))
        }
    }

    impl Drop for Factory {
        fn drop(&mut self) {
            if let Some(own) = self.0.take() {
                *cfg() = own;
            }
            touch::lock::set(false);
            touch::lock::set_held(false);
            HELD.store(false, Ordering::SeqCst);
        }
    }

    fn rig(source: Source) -> (Arc<Mutex<Gesture>>, Notifier, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        let n = Notifier {
            tx,
            ctx: eframe::egui::Context::default(),
        };
        let st = Arc::new(Mutex::new(Gesture::ActivePie {
            id: 1,
            center: Point { x: 10.0, y: 20.0 },
            profile: 0,
            path: Vec::new(),
            hover: None,
            source,
        }));
        (st, n, rx)
    }

    fn esc(st: &Arc<Mutex<Gesture>>, n: &Notifier, down: bool) -> Option<Verdict> {
        handle(st, n, down, i32::from(key::ESCAPE), 0)
    }

    #[test]
    fn esc_closes_a_mouse_pie_and_locks_then_a_held_trigger_unlocks() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _f = Factory::set();
        touch::lock::set(false);
        touch::lock::set_held(false);
        let (st, n, rx) = rig(Source::Mouse);

        // Esc with the pie open: it closes, the lock is on, the key is swallowed with its key-up
        assert_eq!(esc(&st, &n, true), Some(Verdict::Drop));
        assert!(touch::lock::is_locked());
        assert!(
            matches!(*lock(&st), Gesture::InSettings(Source::Mouse)),
            "the rest of the button gesture is swallowed"
        );
        assert!(matches!(rx.try_recv(), Ok(Msg::HidePie)));
        assert!(matches!(
            rx.try_recv(),
            Ok(Msg::LockAnim {
                locked: true,
                x,
                y
            }) if x == 10.0 && y == 20.0
        ));
        assert_eq!(esc(&st, &n, false), Some(Verdict::Drop));
        assert_eq!(
            esc(&st, &n, false),
            None,
            "only the key-up of a taken press"
        );

        // locked and nothing held: Esc is the app's
        assert_eq!(esc(&st, &n, true), None);
        assert!(touch::lock::is_locked());

        // locked and the trigger is held: Esc unlocks
        touch::lock::set_held(true);
        assert_eq!(esc(&st, &n, true), Some(Verdict::Drop));
        assert!(!touch::lock::is_locked());
        assert!(matches!(rx.try_recv(), Ok(Msg::Hud(_))));
        assert_eq!(esc(&st, &n, false), Some(Verdict::Drop));
    }

    #[test]
    fn a_left_click_opens_the_lock_only_for_a_held_mouse_trigger() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _f = Factory::set();
        let p = Point { x: 10.0, y: 20.0 };
        let (st, n, _rx) = rig(Source::Mouse);
        touch::lock::set(true);
        touch::lock::set_held(true);

        // a trackpad hold leaves the gesture idle: it has its own two-finger way
        *lock(&st) = Gesture::Idle;
        assert!(!click_unlock(&st, &n, Kind::Down, 0, p));
        // the mouse trigger is held on the lock (its press went to the app)
        *lock(&st) = Gesture::Passthrough;
        assert!(
            !click_unlock(&st, &n, Kind::Down, 1, p),
            "not the left button"
        );
        assert!(!click_unlock(&st, &n, Kind::Up, 0, p), "not a press");
        // and nothing is held: the click is the app's
        touch::lock::set_held(false);
        assert!(!click_unlock(&st, &n, Kind::Down, 0, p));
        assert!(touch::lock::is_locked(), "none of these opened the lock");

        // switched off, the click is the app's as well
        touch::lock::set_held(true);
        cfg().esc_lock = false;
        assert!(!click_unlock(&st, &n, Kind::Down, 0, p));
        cfg().esc_lock = true;
        assert!(touch::lock::is_locked());
    }

    #[test]
    fn esc_closes_a_trackpad_pie_to_idle() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _f = Factory::set();
        touch::lock::set(false);
        let (st, n, _rx) = rig(Source::Touch);
        assert_eq!(esc(&st, &n, true), Some(Verdict::Drop));
        assert!(touch::lock::is_locked());
        assert!(matches!(*lock(&st), Gesture::Idle));
    }

    #[test]
    fn esc_with_no_pie_or_with_a_modifier_or_switched_off_is_the_apps() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _f = Factory::set();
        touch::lock::set(false);
        let (st, n, _rx) = rig(Source::Mouse);
        assert_eq!(handle(&st, &n, true, i32::from(key::ESCAPE), 1), None);
        assert!(!touch::lock::is_locked());

        cfg().esc_lock = false;
        assert_eq!(esc(&st, &n, true), None);
        assert!(!touch::lock::is_locked());
        assert!(matches!(*lock(&st), Gesture::ActivePie { .. }));
        cfg().esc_lock = true;

        *lock(&st) = Gesture::Idle;
        assert_eq!(esc(&st, &n, true), None, "no pie open");
        assert!(!touch::lock::is_locked());
    }

    #[test]
    fn other_keys_are_never_taken() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _f = Factory::set();
        let (st, n, _rx) = rig(Source::Mouse);
        assert_eq!(handle(&st, &n, true, i32::from(key::SPACE), 0), None);
        assert!(matches!(*lock(&st), Gesture::ActivePie { .. }));
    }
}
