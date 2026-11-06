//! The extra click: while a pie opened by a trackpad hold is open, one more finger clicks.
//!
//! The system decides what a click with a finger resting on the trackpad *is* (a secondary click
//! for two fingers, or nothing), so CursorPie does not look at what the system made of it. It
//! counts the fingers itself: while a trackpad pie is open, a click of any kind that arrives while
//! two fingers touch the surface (the holding one and the one that clicks) is the extra click. It is
//! swallowed (its press, its drags and its release) and runs one action, whatever the system mapping is
//! ([`TouchClickAction`]).
//!
//! A **tap** of the extra finger (tap to click) is recognized by the recognizer itself
//! ([`TouchEvent::ExtraClick`](super::recognizer::TouchEvent::ExtraClick)); by then the finger has
//! lifted, so the click the system makes of the tap arrives with the old finger count. [`tap`] opens
//! a short window ([`TAP_CLICK_WINDOW_MS`]) in which that click is swallowed too. A press and the
//! tap that follows it, or the other way round, count once ([`DEDUP_MS`]).
//!
//! **Safety:** every other state returns `None` after one atomic read, so ordinary clicking costs
//! nothing and is never touched. Every press decides afresh whether it is swallowed, so a swallowed
//! click can never leave the next one stuck: if its release is lost, the next press resets the flag.

use super::super::gesture::{Gesture, Source};
use super::super::resolve;
use super::haptic::{self, Tick};
use super::{ACTIVE, FINGERS, now};
use crate::config::{TouchClickAction, cfg, lock};
use crate::msg::{Msg, Notifier};
use crate::tap::{Kind, Verdict, right_click};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Fingers on the surface at the extra click: the holding one and the one that clicks.
const EXTRA_CLICK_FINGERS: usize = 2;
/// Button number of the left button.
pub const LEFT_BUTTON: i64 = 0;
/// The click the system makes of a tap of the extra finger is swallowed for this long (ms).
pub const TAP_CLICK_WINDOW_MS: u64 = 250;
/// A press and a tap of the same extra finger count once when they come within this time (ms).
pub const DEDUP_MS: u64 = 400;

/// The press of a click was swallowed, so its drags and its release are too.
static SWALLOWED: AtomicBool = AtomicBool::new(false);
/// Until this time (ms on the touch clock, plus 1; 0 = never) the next click is the one a tap made.
static SWALLOW_UNTIL: AtomicU64 = AtomicU64::new(0);
/// When the extra click last ran (ms on the touch clock, plus 1; 0 = never).
static LAST_FIRED: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    now().as_millis() as u64 + 1
}

/// Forgets everything (the tap was disabled by the system, or the touch source stopped).
pub(crate) fn clear() {
    SWALLOWED.store(false, Ordering::SeqCst);
    SWALLOW_UNTIL.store(0, Ordering::SeqCst);
}

/// Whether a click event of this kind is worth handing to [`handle`]: the native tap calls this for
/// the left button before it reads anything from the event, so a left click or drag costs one
/// atomic read while no trackpad hold is running.
pub(crate) fn interested(kind: Kind) -> bool {
    match kind {
        Kind::Down => ACTIVE.load(Ordering::Relaxed),
        Kind::Up | Kind::Drag => SWALLOWED.load(Ordering::Relaxed),
    }
}

/// A click event (left or right button). `None` = not ours, carry on as usual.
pub(crate) fn handle(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    kind: Kind,
) -> Option<Verdict> {
    match kind {
        Kind::Up | Kind::Drag => {
            if !SWALLOWED.load(Ordering::SeqCst) {
                return None;
            }
            if kind == Kind::Up {
                SWALLOWED.store(false, Ordering::SeqCst);
            }
            Some(Verdict::Drop)
        }
        Kind::Down => {
            // every press decides again; a stale flag from a lost release ends here
            SWALLOWED.store(false, Ordering::SeqCst);
            if !ACTIVE.load(Ordering::SeqCst) {
                return None;
            }
            let physical = FINGERS.load(Ordering::SeqCst) == EXTRA_CLICK_FINGERS;
            let t = now_ms();
            let from_tap = {
                let until = SWALLOW_UNTIL.load(Ordering::SeqCst);
                until != 0 && t <= until
            };
            if !(physical || from_tap) {
                return None;
            }
            let open = matches!(
                &*lock(state),
                Gesture::ActivePie {
                    source: Source::Touch,
                    ..
                }
            );
            if !open {
                return None;
            }
            SWALLOWED.store(true, Ordering::SeqCst);
            SWALLOW_UNTIL.store(0, Ordering::SeqCst);
            diag::log!(
                "touch",
                "extra_click",
                via = if physical { "press" } else { "tap_click" }
            );
            fire(state, notifier);
            Some(Verdict::Drop)
        }
    }
}

/// The recognizer saw the extra finger tap. Runs the action and lets the click the system makes of
/// the tap (if tap to click is on) be swallowed.
pub(crate) fn tap(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) {
    let open = matches!(
        &*lock(state),
        Gesture::ActivePie {
            source: Source::Touch,
            ..
        }
    );
    if !open {
        return;
    }
    SWALLOW_UNTIL.store(now_ms() + TAP_CLICK_WINDOW_MS, Ordering::SeqCst);
    diag::log!("touch", "extra_click", via = "tap");
    fire(state, notifier);
}

/// Carries out [`TouchClickAction`] once (two reports within [`DEDUP_MS`] are one click).
fn fire(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) {
    if crate::tap::settings_open() {
        return; // the pie is off while Settings is open
    }
    let t = now_ms();
    let last = LAST_FIRED.swap(t, Ordering::SeqCst);
    if last != 0 && t.saturating_sub(last) < DEDUP_MS {
        diag::log!("touch", "extra_click_repeat");
        return;
    }
    let (profile, path, hover) = {
        let g = lock(state);
        match &*g {
            Gesture::ActivePie {
                profile,
                path,
                hover,
                source: Source::Touch,
                ..
            } => (*profile, path.clone(), *hover),
            _ => return,
        }
    };
    let action = cfg().touch_click_action;
    let did = match action {
        TouchClickAction::SwitchPie => right_click::switch_pie(notifier, hover),
        TouchClickAction::PickHovered => match hover {
            Some(h) if resolve::is_mode(profile, &path, h) => {
                resolve::pick_mode(profile, &path, h);
                notifier.send(Msg::Hover(hover)); // wakes the UI so the ring and the center are redrawn
                true
            }
            _ => false,
        },
        TouchClickAction::Nothing => false,
    };
    diag::log!(
        "touch",
        "extra_action",
        action = format!("{action:?}"),
        did = did
    );
    if did {
        haptic::emit(notifier, Tick::Mode);
    }
}
