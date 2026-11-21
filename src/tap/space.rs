//! Space while the pie is open opens Settings. It is on by default, and it is the way into Settings
//! now that "Open settings by holding" is off by default.
//!
//! It works in every pie that is open: the top ring, a submenu at any depth (in the trail mode
//! too) and the pie of an app (opened by a double click). The hold in the center cannot do that
//! (it only counts for the untouched top ring of the pie that a hold opened).
//!
//! # Holding Space: the intelligent pie
//!
//! With the intelligent pie switched on (Behavior > Intelligent) Space has two meanings, told by how
//! long it is held. *Held* for `smart.hold_ms` turns the pie into the intelligent pie (and, in the
//! intelligent pie, back into the pie it came from): a timer is started by the key-down and, if the
//! key is still down when it fires, switches ([`trigger::switch_smart_pie`]). *Pressed quickly*
//! (released before the timer) does what Space always did, open Settings, **but on the key-up**:
//! the key-down cannot know yet which of the two it is. With the intelligent pie off nothing
//! changes: Space opens Settings on the key-down, as before.
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
use std::time::Duration;

/// How long after one press of Space the next one still counts toward the same series, in ms.
pub(crate) const WINDOW_MS: u64 = 500;

/// Space is down and CursorPie took its press (its auto-repeat and its key-up are swallowed too).
static HELD: AtomicBool = AtomicBool::new(false);
/// Presses counted in the current series.
static COUNT: AtomicU8 = AtomicU8::new(0);
/// When the last counted press happened, in ms since the tap's epoch, plus 1 (0 = none).
static LAST_MS: AtomicU64 = AtomicU64::new(0);
/// The token of the hold timer that is still waiting (0 = none). Whoever swaps it to 0 first, the
/// timer or the key-up, decides what this press was.
static HOLD: AtomicU64 = AtomicU64::new(0);
/// Hands out tokens.
static HOLD_NEXT: AtomicU64 = AtomicU64::new(0);

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
        if !HELD.swap(false, Ordering::SeqCst) {
            return None;
        }
        // the timer did not get there first: this was a quick press, which counts toward Settings
        // now (it could not on the key-down, see the module docs)
        if HOLD.swap(0, Ordering::SeqCst) != 0 {
            let (on, needed) = {
                let c = cfg();
                (c.settings_by_space, c.settings_space_presses)
            };
            if on {
                count_press(state, notifier, needed);
            }
        }
        return Some(Verdict::Drop);
    }
    if HELD.load(Ordering::SeqCst) {
        return Some(Verdict::Drop); // auto-repeat of a taken press: not a new one
    }
    if mods != 0 || RECORDING.load(Ordering::SeqCst) {
        return None;
    }
    let (on, needed, smart_ms) = {
        let c = cfg();
        (
            c.settings_by_space,
            c.settings_space_presses,
            c.smart.enabled.then_some(c.smart.hold_ms),
        )
    };
    if !(on || smart_ms.is_some()) || !matches!(*lock(state), Gesture::ActivePie { .. }) {
        COUNT.store(0, Ordering::SeqCst);
        return None;
    }
    HELD.store(true, Ordering::SeqCst);
    match smart_ms {
        Some(ms) => {
            let token = HOLD_NEXT.fetch_add(1, Ordering::SeqCst) + 1;
            HOLD.store(token, Ordering::SeqCst);
            let (st, n) = (Arc::clone(state), notifier.clone());
            let started = super::timers::after(Duration::from_millis(ms), move || {
                hold_elapsed(&st, &n, token);
            });
            if !started {
                // no timer, no hold: a press is a press
                HOLD.store(0, Ordering::SeqCst);
                if on {
                    count_press(state, notifier, needed);
                }
            }
        }
        None => count_press(state, notifier, needed),
    }
    Some(Verdict::Drop)
}

/// The hold timer of press `token` ran out. When the key is still down and nobody took the press
/// back, the pie is switched.
fn hold_elapsed(state: &Arc<Mutex<Gesture>>, notifier: &Notifier, token: u64) {
    if HOLD
        .compare_exchange(token, 0, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
        || !HELD.load(Ordering::SeqCst)
    {
        return; // released in time, or another press
    }
    COUNT.store(0, Ordering::SeqCst);
    LAST_MS.store(0, Ordering::SeqCst);
    trigger::switch_smart_pie(state, notifier);
}

/// One counted press of Space: Settings opens when the series is complete.
fn count_press(state: &Arc<Mutex<Gesture>>, notifier: &Notifier, needed: u8) {
    let now = super::now_ticks();
    let last = LAST_MS.swap(now, Ordering::SeqCst);
    let since = (last != 0).then(|| now.saturating_sub(last));
    let (count, open) = step(COUNT.load(Ordering::SeqCst), since, needed);
    COUNT.store(count, Ordering::SeqCst);
    if open {
        LAST_MS.store(0, Ordering::SeqCst);
        trigger::open_settings_from_pie(state, notifier);
    }
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
    /// afterwards, even if the test panics. The intelligent pie is switched off meanwhile: the live
    /// config is the one of whoever runs the tests, and with it on Space is a hold, not a press.
    struct SpaceSetting(bool, u8, bool);

    impl SpaceSetting {
        fn set(on: bool, presses: u8) -> Self {
            let mut c = cfg();
            let own = Self(
                c.settings_by_space,
                c.settings_space_presses,
                c.smart.enabled,
            );
            c.settings_by_space = on;
            c.settings_space_presses = presses;
            c.smart.enabled = false;
            own
        }
    }

    impl Drop for SpaceSetting {
        fn drop(&mut self) {
            let mut c = cfg();
            c.settings_by_space = self.0;
            c.settings_space_presses = self.1;
            c.smart.enabled = self.2;
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

    /// Switches the intelligent pie on (with Space for Settings as `by_space`) for one test.
    struct SmartSetting(bool, bool, u8);

    impl SmartSetting {
        fn set(smart: bool, by_space: bool) -> Self {
            let mut c = cfg();
            let own = Self(
                c.smart.enabled,
                c.settings_by_space,
                c.settings_space_presses,
            );
            c.smart.enabled = smart;
            c.settings_by_space = by_space;
            c.settings_space_presses = 1;
            own
        }
    }

    impl Drop for SmartSetting {
        fn drop(&mut self) {
            let mut c = cfg();
            c.smart.enabled = self.0;
            c.settings_by_space = self.1;
            c.settings_space_presses = self.2;
        }
    }

    fn profile_of(st: &Arc<Mutex<Gesture>>) -> Option<usize> {
        match &*lock(st) {
            Gesture::ActivePie { profile, .. } => Some(*profile),
            _ => None,
        }
    }

    fn pie_shown(rx: &mpsc::Receiver<Msg>) -> Option<usize> {
        std::iter::from_fn(|| rx.try_recv().ok()).find_map(|m| match m {
            Msg::ShowPie { profile, path, .. } if path.is_empty() => Some(profile),
            _ => None,
        })
    }

    /// Lets the hold timer of the press that is waiting run out, without waiting for the clock.
    fn run_out(st: &Arc<Mutex<Gesture>>, n: &Notifier) {
        let token = HOLD.load(Ordering::SeqCst);
        assert_ne!(token, 0, "a hold timer must be waiting");
        hold_elapsed(st, n, token);
    }

    #[test]
    fn a_quick_press_opens_settings_on_the_key_up_when_the_intelligent_pie_is_on() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _on = SmartSetting::set(true, true);
        RECORDING.store(false, Ordering::SeqCst);
        reset();
        let st = pie(0, vec![]);
        let (n, rx) = notifier();
        assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
        assert!(!opened_settings(&rx), "the key-down cannot know yet");
        assert!(matches!(*lock(&st), Gesture::ActivePie { .. }));
        assert_eq!(handle(&st, &n, false, space(), 0), Some(Verdict::Drop));
        assert!(
            opened_settings(&rx),
            "released before the hold: it was a press"
        );
        reset();
    }

    #[test]
    fn a_held_space_turns_the_pie_into_the_intelligent_pie_and_back() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _on = SmartSetting::set(true, true);
        RECORDING.store(false, Ordering::SeqCst);
        crate::smart::end_gesture();
        crate::smart::set_kept(false);
        reset();
        let st = pie(2, vec![1]);
        let (n, rx) = notifier();
        // first hold: to the intelligent pie, at the top ring, same center
        assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
        run_out(&st, &n);
        assert_eq!(profile_of(&st), Some(crate::smart::SMART_PROFILE));
        assert!(
            matches!(&*lock(&st), Gesture::ActivePie { path, hover: None, .. } if path.is_empty())
        );
        assert_eq!(pie_shown(&rx), Some(crate::smart::SMART_PROFILE));
        assert_eq!(handle(&st, &n, false, space(), 0), Some(Verdict::Drop));
        assert!(
            !opened_settings(&rx),
            "a hold is not a press: Settings stays closed"
        );
        // second hold: back to the pie it came from
        assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
        run_out(&st, &n);
        assert_eq!(profile_of(&st), Some(2));
        assert_eq!(pie_shown(&rx), Some(2));
        assert_eq!(handle(&st, &n, false, space(), 0), Some(Verdict::Drop));
        assert!(!opened_settings(&rx));
        crate::smart::end_gesture();
        crate::smart::set_kept(false);
        reset();
    }

    #[test]
    fn a_hold_timer_of_an_earlier_press_does_nothing() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _on = SmartSetting::set(true, true);
        RECORDING.store(false, Ordering::SeqCst);
        reset();
        let st = pie(0, vec![]);
        let (n, rx) = notifier();
        handle(&st, &n, true, space(), 0);
        let old = HOLD.load(Ordering::SeqCst);
        handle(&st, &n, false, space(), 0); // quick press: the timer is taken back
        let _ = opened_settings(&rx);
        hold_elapsed(&st, &n, old);
        assert_ne!(
            profile_of(&st),
            Some(crate::smart::SMART_PROFILE),
            "released in time: no switch"
        );
        reset();
    }

    #[test]
    fn with_settings_by_space_off_a_quick_press_does_nothing_but_a_hold_still_switches() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _on = SmartSetting::set(true, false);
        RECORDING.store(false, Ordering::SeqCst);
        crate::smart::end_gesture();
        crate::smart::set_kept(false);
        reset();
        let st = pie(0, vec![]);
        let (n, rx) = notifier();
        assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
        assert_eq!(handle(&st, &n, false, space(), 0), Some(Verdict::Drop));
        assert!(!opened_settings(&rx));
        assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
        run_out(&st, &n);
        assert_eq!(profile_of(&st), Some(crate::smart::SMART_PROFILE));
        handle(&st, &n, false, space(), 0);
        crate::smart::end_gesture();
        crate::smart::set_kept(false);
        reset();
    }

    #[test]
    fn with_the_intelligent_pie_off_space_is_exactly_as_before() {
        let _s = TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _on = SmartSetting::set(false, true);
        RECORDING.store(false, Ordering::SeqCst);
        reset();
        let st = pie(0, vec![]);
        let (n, rx) = notifier();
        assert_eq!(handle(&st, &n, true, space(), 0), Some(Verdict::Drop));
        assert!(opened_settings(&rx), "on the key-down, as always");
        assert_eq!(HOLD.load(Ordering::SeqCst), 0, "no hold timer");
        handle(&st, &n, false, space(), 0);
        reset();
    }
}
