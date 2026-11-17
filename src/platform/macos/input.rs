//! Posting keys and mouse buttons with `CGEvent`.

use crate::actions::SYNTH_TAG;
use crate::geometry::Point;
use crate::keys::*;
use crate::tap::Kind;
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton, EventField,
};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Presses modifiers, the key, then releases everything in reverse order.
pub fn press_shortcut(sc: Shortcut) {
    let Ok(src) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
        return;
    };
    let none = CGEventFlags::CGEventFlagNull;
    let table: [(u8, CGKeyCode, CGEventFlags); 4] = [
        (MOD_CTRL, 59, CGEventFlags::CGEventFlagControl),
        (MOD_ALT, 58, CGEventFlags::CGEventFlagAlternate),
        (MOD_SHIFT, 56, CGEventFlags::CGEventFlagShift),
        (MOD_CMD, 55, CGEventFlags::CGEventFlagCommand),
    ];

    // (modifier keycode, cumulative flags once that modifier is down)
    let mut seq: Vec<(CGKeyCode, CGEventFlags)> = Vec::new();
    let mut acc = none;
    // pressed in the order the shortcut was recorded in (the order its label shows)
    for bit in sc.ordered_mods() {
        if let Some((_, code, flag)) = table.iter().find(|(b, _, _)| *b == bit) {
            acc |= *flag;
            seq.push((*code, acc));
        }
    }

    for (code, flags) in &seq {
        post_key(&src, *code, true, *flags);
    }
    post_key(&src, sc.key, true, acc);
    post_key(&src, sc.key, false, acc);
    for i in (0..seq.len()).rev() {
        let after = if i == 0 { none } else { seq[i - 1].1 };
        post_key(&src, seq[i].0, false, after);
    }
}

/// Control+Left or Control+Right: the Mission Control shortcuts that move to the Space on the
/// left or on the right. A real arrow key also carries the `SecondaryFn` and `NumericPad` flags,
/// which the system shortcut is matched against, so they are set here too.
pub fn press_space_arrow(forward: bool) {
    let Ok(src) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
        return;
    };
    let ctrl = CGEventFlags::CGEventFlagControl;
    let arrow = ctrl | CGEventFlags::CGEventFlagSecondaryFn | CGEventFlags::CGEventFlagNumericPad;
    let code = if forward { key::RIGHT } else { key::LEFT };
    post_key(&src, 59, true, ctrl);
    post_key(&src, code, true, arrow);
    post_key(&src, code, false, arrow);
    post_key(&src, 59, false, CGEventFlags::CGEventFlagNull);
}

/// Posts one key event with the given modifier flags, tagged with [`SYNTH_TAG`].
fn post_key(src: &CGEventSource, code: CGKeyCode, down: bool, flags: CGEventFlags) {
    if let Ok(ev) = CGEvent::new_keyboard_event(src.clone(), code, down) {
        ev.set_flags(flags);
        ev.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTH_TAG);
        ev.post(CGEventTapLocation::HID);
        std::thread::sleep(Duration::from_millis(3));
    }
}

/// Brings back a pointer that an app hid, for a pie that opens on a pointer that has not moved.
///
/// A terminal, an editor, a browser or a notes app hides the pointer while the person types and
/// keeps it hidden "until the mouse moves"; a video player hides it after a moment without
/// movement. The pie opens on a pointer that has not moved, so the pie was on screen and the
/// pointer was not.
///
/// What brings the pointer back is a mouse move that really moved something, and the first
/// version of this function posted one with no distance at the place the pointer already was: an
/// event that says nothing moved, which the system has no reason to treat as movement. This one
/// posts a real move of one point and the move back (net distance: none, the pointer ends where
/// it was). Both events carry [`SYNTH_TAG`], so the tap lets them through untouched.
///
/// This only helps at the moment the pie opens. For the whole hold see [`relay_motion`], which is
/// the part that keeps the pointer from disappearing while the pie is open.
pub fn reveal_cursor() {
    let Some((x, y)) = super::cursor_pos() else {
        return;
    };
    let edge = super::display_rects()
        .into_iter()
        .find(|r| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h)
        .map(|r| r.x + r.w);
    let step = nudge_step(x, edge);
    post_move(x + step, y, step as i64, 0);
    post_move(x, y, -(step as i64), 0);
    LAST_MOTION_MS.store(clock_ms(), Ordering::Relaxed);
}

/// Plays a drag that the tap swallowed back to the system as a plain mouse move, at most every
/// `RELAY_EVERY_MS`. `(x, y)` is where the pointer is and `(dx, dy)` how far the swallowed drag
/// moved it.
///
/// THE ROOT OF THE MISSING POINTER. While a pie of a mouse hold is open, the tap swallows every
/// drag of the held button (`on_drag_in_pie`), at the HID level: neither the window server nor
/// any app ever hears that the pointer moved. An app that hides the pointer and shows it again
/// only when it hears the mouse move (a terminal that hides it while typing, a video player that
/// hides it when idle, a browser in full screen) therefore keeps it hidden for the whole hold,
/// however much the hand moves: the pie follows the hand and the pointer is gone. Posting one
/// event when the pie opens (see [`reveal_cursor`]) can not fix that, because the app hides it
/// again, and the hold goes on without any motion reaching it.
///
/// The drag itself must stay swallowed (a middle-button drag is a scroll or a pan in many apps),
/// so what is sent is a mouse move with no button, which is what the pointer would do with the
/// button up: no click state, no drag. It is throttled so that an app below the pie hears a few
/// moves a second (enough to keep its pointer shown, too few to matter to a hover effect), not
/// the whole stream. Called from the tap callback: it never waits.
pub fn relay_motion(x: f64, y: f64, dx: i64, dy: i64) {
    let now = clock_ms();
    if !motion_due(LAST_MOTION_MS.load(Ordering::Relaxed), now, RELAY_EVERY_MS) {
        return;
    }
    LAST_MOTION_MS.store(now, Ordering::Relaxed);
    post_move(x, y, dx, dy);
}

/// How often, at most, a swallowed drag is played back as a mouse move (see [`relay_motion`]).
const RELAY_EVERY_MS: u64 = 100;

/// When the system was last told that the pointer moved: milliseconds on [`clock_ms`], 0 for
/// never.
static LAST_MOTION_MS: AtomicU64 = AtomicU64::new(0);

/// Milliseconds since the first call, and never 0 (0 means "never" in [`LAST_MOTION_MS`]).
fn clock_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// Whether a motion may be sent at `now`: none was sent yet, or the last one is `every`
/// milliseconds old or more.
fn motion_due(last: u64, now: u64, every: u64) -> bool {
    last == 0 || now.saturating_sub(last) >= every
}

/// The direction (+1.0 or -1.0) of the one-point step [`reveal_cursor`] makes: to the right,
/// unless the pointer is at the right edge of its display (`right_edge`), where it goes left.
fn nudge_step(x: f64, right_edge: Option<f64>) -> f64 {
    match right_edge {
        Some(edge) if x + 2.0 > edge => -1.0,
        _ => 1.0,
    }
}

/// Posts a mouse move (no button down) at `(x, y)` that moved `(dx, dy)`, tagged with
/// [`SYNTH_TAG`] so that the tap lets it through.
fn post_move(x: f64, y: f64, dx: i64, dy: i64) {
    if let Ok(src) = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        && let Ok(ev) = CGEvent::new_mouse_event(
            src,
            CGEventType::MouseMoved,
            CGPoint::new(x, y),
            CGMouseButton::Left,
        )
    {
        ev.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_X, dx);
        ev.set_integer_value_field(EventField::MOUSE_EVENT_DELTA_Y, dy);
        ev.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTH_TAG);
        ev.post(CGEventTapLocation::HID);
    }
}

/// The mice macOS lists: none. There is no public call CursorPie uses to ask a Mac how many buttons
/// its mouse has, so the buttons are found by scanning the mouse in Settings (see [`mouse`](crate::mouse)).
pub fn mouse_devices() -> Vec<crate::mouse::Device> {
    Vec::new()
}

/// Re-posts a swallowed press or release of `button` at `pos` (used when a quick click must reach
/// the app after all). Only `Kind::Down` and `Kind::Up` are replayed.
pub fn post_mouse(pos: Point, kind: Kind, button: i64) {
    let ty = match kind {
        Kind::Down => CGEventType::OtherMouseDown,
        Kind::Up => CGEventType::OtherMouseUp,
        Kind::Drag => return,
    };
    let pos = CGPoint::new(pos.x, pos.y);
    if let Ok(src) = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        && let Ok(ev) = CGEvent::new_mouse_event(src, ty, pos, CGMouseButton::Center)
    {
        ev.set_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER, button);
        ev.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, 1);
        ev.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTH_TAG);
        ev.post(CGEventTapLocation::HID);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_motion_is_always_due() {
        assert!(motion_due(0, 1, RELAY_EVERY_MS));
    }

    #[test]
    fn motions_are_spaced_by_the_interval() {
        assert!(!motion_due(1000, 1000, RELAY_EVERY_MS));
        assert!(!motion_due(1000, 1000 + RELAY_EVERY_MS - 1, RELAY_EVERY_MS));
        assert!(motion_due(1000, 1000 + RELAY_EVERY_MS, RELAY_EVERY_MS));
    }

    #[test]
    fn a_clock_that_went_back_never_blocks_for_ever() {
        // saturating: an earlier "now" than "last" counts as no time passed, not as a huge gap
        assert!(!motion_due(2000, 1000, RELAY_EVERY_MS));
        assert!(motion_due(0, 0, RELAY_EVERY_MS));
    }

    #[test]
    fn the_clock_never_returns_zero() {
        assert!(clock_ms() >= 1);
    }

    #[test]
    fn the_nudge_goes_right_unless_the_pointer_is_at_the_right_edge() {
        assert_eq!(nudge_step(100.0, Some(1440.0)), 1.0);
        assert_eq!(nudge_step(100.0, None), 1.0);
        assert_eq!(nudge_step(1439.0, Some(1440.0)), -1.0);
        assert_eq!(nudge_step(1437.9, Some(1440.0)), 1.0);
    }
}
