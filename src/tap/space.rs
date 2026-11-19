//! Space while the pie is open opens Settings. It is on by default, and it is the way into Settings
//! now that "Open settings by holding" is off by default.
//!
//! It works in every pie that is open: the top ring, a submenu at any depth (in the trail mode
//! too) and the pie of an app (opened by a double click). The hold in the center cannot do that
//! (it only counts for the untouched top ring of the pie that a hold opened).
//!
//! One or two presses of Space, as set in Behavior. The key is only taken while a pie is open and
//! the setting is on; in every other case it passes through untouched. A press that is taken is
//! swallowed together with its auto-repeat and its key-up, so it never reaches the app under the
//! pie. Counting is on the tap thread with a few atomics: nothing blocks.

use super::gesture::Gesture;
use super::{RECORDING, Verdict, trigger};
use crate::config::{cfg, lock};
use crate::keys::key;
use crate::msg::Notifier;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// How long after one press of Space the next one still counts toward the same series, in ms.
pub(crate) const WINDOW_MS: u64 = 500;

/// Space is down and CursorPie took its press (its auto-repeat and its key-up are swallowed too).
static HELD: AtomicBool = AtomicBool::new(false);
/// Presses counted in the current series.
static COUNT: AtomicU8 = AtomicU8::new(0);
/// When the last counted press happened, in ms since the tap's epoch, plus 1 (0 = none).
static LAST_MS: AtomicU64 = AtomicU64::new(0);

/// What a press does to a series: the new count, and whether Settings opens now. Pure, so it is
/// unit tested. `since_last_ms` is `None` for the first press (or after a pause too long for the
/// series); `needed` is the number of presses that open Settings (1 or 2).
pub(crate) fn step(count: u8, since_last_ms: Option<u64>, needed: u8) -> (u8, bool) {
    let continued = since_last_ms.is_some_and(|ms| ms <= WINDOW_MS);
    let now = if continued {
        count.saturating_add(1)
    } else {
        1
    };
    if now >= needed.max(1) {
        (0, true)
    } else {
        (now, false)
    }
}

/// Looks at one key event. `Some(verdict)` = Space was taken here and the verdict is final;
/// `None` = not ours, the normal key handling goes on.
pub(super) fn handle(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    down: bool,
    code: i32,
    mods: u8,
) -> Option<Verdict> {
    if code != i32::from(key::SPACE) {
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
        return None;
    }
    let (on, needed) = {
        let c = cfg();
        (c.settings_by_space, c.settings_space_presses)
    };
    if !on || !matches!(*lock(state), Gesture::ActivePie { .. }) {
        COUNT.store(0, Ordering::SeqCst);
        return None;
    }
    HELD.store(true, Ordering::SeqCst);
    let now = super::now_ticks();
    let last = LAST_MS.swap(now, Ordering::SeqCst);
    let since = (last != 0).then(|| now.saturating_sub(last));
    let (count, open) = step(COUNT.load(Ordering::SeqCst), since, needed);
    COUNT.store(count, Ordering::SeqCst);
    if open {
        LAST_MS.store(0, Ordering::SeqCst);
        trigger::open_settings_from_pie(state, notifier);
    }
    Some(Verdict::Drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_press_opens_settings_when_one_is_needed() {
        assert_eq!(step(0, None, 1), (0, true));
        assert_eq!(step(0, Some(100), 1), (0, true));
    }

    #[test]
    fn two_presses_must_come_close_together() {
        assert_eq!(step(0, None, 2), (1, false), "the first one only counts");
        assert_eq!(step(1, Some(200), 2), (0, true), "the second one opens");
        assert_eq!(
            step(1, Some(WINDOW_MS + 1), 2),
            (1, false),
            "too late: it starts a new series"
        );
    }

    #[test]
    fn the_number_of_presses_is_never_zero() {
        assert_eq!(step(0, None, 0), (0, true));
    }

    use crate::geometry::Point;
    use crate::msg::Msg;
    use crate::tap::TEST_SERIAL;
    use crate::tap::gesture::Source;
    use std::sync::mpsc;

    /// Puts the statics of this module back to "nothing happened".
    fn reset() {
        HELD.store(false, Ordering::SeqCst);
        COUNT.store(0, Ordering::SeqCst);
        LAST_MS.store(0, Ordering::SeqCst);
    }

    /// Switches Space on (with `presses`) for one test and puts the person's own setting back
    /// afterwards, even if the test panics.
    struct SpaceSetting(bool, u8);

    impl SpaceSetting {
        fn set(on: bool, presses: u8) -> Self {
            let mut c = cfg();
            let own = Self(c.settings_by_space, c.settings_space_presses);
            c.settings_by_space = on;
            c.settings_space_presses = presses;
            own
        }
    }

    impl Drop for SpaceSetting {
        fn drop(&mut self) {
            let mut c = cfg();
            c.settings_by_space = self.0;
            c.settings_space_presses = self.1;
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

    fn pie(profile: usize, path: Vec<usize>) -> Arc<Mutex<Gesture>> {
        Arc::new(Mutex::new(Gesture::ActivePie {
            id: 1,
            center: Point::new(40.0, 50.0),
            profile,
            path,
            hover: None,
            source: Source::Mouse,
        }))
    }

    fn space() -> i32 {
        i32::from(key::SPACE)
    }

    fn opened_settings(rx: &mpsc::Receiver<Msg>) -> bool {
        std::iter::from_fn(|| rx.try_recv().ok()).any(|m| matches!(m, Msg::OpenSettings { .. }))
    }

    #[test]
    fn space_opens_settings_in_the_top_ring_in_submenus_and_in_an_app_pie() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _on = SpaceSetting::set(true, 1);
        RECORDING.store(false, Ordering::SeqCst);
        // (profile, path): the top ring, a submenu, a submenu of a submenu, and the same in the
        // pie of an app (any profile other than the first)
        for (profile, path) in [
            (0, vec![]),
            (0, vec![2]),
            (0, vec![1, 3]),
            (1, vec![]),
            (1, vec![0]),
            (2, vec![4, 1, 0]),
        ] {
            reset();
            let st = pie(profile, path.clone());
            let (n, rx) = notifier();
            assert_eq!(
                handle(&st, &n, true, space(), 0),
                Some(Verdict::Drop),
                "Space is taken at profile {profile}, path {path:?}"
            );
            assert!(
                matches!(*lock(&st), Gesture::InSettings(Source::Mouse)),
                "the gesture became Settings at profile {profile}, path {path:?}"
            );
            assert!(
                opened_settings(&rx),
                "Settings was asked for at profile {profile}, path {path:?}"
            );
            // its repeat and its release never reach the app
            assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
            assert_eq!(handle(&st, &n, false, space(), 0), Some(Verdict::Drop));
            assert_eq!(
                handle(&st, &n, false, space(), 0),
                None,
                "the key is free again"
            );
        }
        reset();
    }

    #[test]
    fn two_presses_open_settings_in_a_submenu_too() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _on = SpaceSetting::set(true, 2);
        RECORDING.store(false, Ordering::SeqCst);
        reset();
        let st = pie(1, vec![3, 1]);
        let (n, rx) = notifier();
        assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
        assert_eq!(handle(&st, &n, false, space(), 0), Some(Verdict::Drop));
        assert!(!opened_settings(&rx), "one press only counts");
        assert!(matches!(*lock(&st), Gesture::ActivePie { .. }));
        assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
        assert!(opened_settings(&rx), "the second one opens");
        assert!(matches!(*lock(&st), Gesture::InSettings(_)));
        assert_eq!(handle(&st, &n, false, space(), 0), Some(Verdict::Drop));
        reset();
    }

    #[test]
    fn space_is_left_alone_without_an_open_pie_or_when_it_is_off() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        RECORDING.store(false, Ordering::SeqCst);
        let (n, rx) = notifier();
        {
            let _on = SpaceSetting::set(true, 1);
            reset();
            let idle = Arc::new(Mutex::new(Gesture::Idle));
            assert_eq!(
                handle(&idle, &n, true, space(), 0),
                None,
                "no pie, no Space"
            );
            assert_eq!(handle(&idle, &n, false, space(), 0), None);
            // a shortcut with Space (Ctrl+Space, ...) is never ours
            let st = pie(0, vec![1]);
            assert_eq!(handle(&st, &n, true, space(), 1), None);
            assert!(matches!(*lock(&st), Gesture::ActivePie { .. }));
        }
        {
            let _off = SpaceSetting::set(false, 1);
            reset();
            let st = pie(0, vec![1]);
            assert_eq!(handle(&st, &n, true, space(), 0), None, "switched off");
            assert!(matches!(*lock(&st), Gesture::ActivePie { .. }));
        }
        assert!(!opened_settings(&rx));
        reset();
    }
}
