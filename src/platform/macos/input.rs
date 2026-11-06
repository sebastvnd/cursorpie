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
use std::time::Duration;

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

/// Clicks `button` (0 = left, 1 = right) at `pos` (logical points) and leaves the pointer where it
/// was: the events carry their own position, so the system's pointer is not moved. `clicks` is the
/// click count of a double click (1, 2, 3), which macOS wants in the event. Used by the virtual
/// trackpad, whose pointer is not the system's.
pub fn click_at(pos: (f64, f64), button: i64, clicks: u32) {
    let (down, up, which) = match button {
        0 => (
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGMouseButton::Left,
        ),
        1 => (
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGMouseButton::Right,
        ),
        _ => return,
    };
    let at = CGPoint::new(pos.0, pos.1);
    let Ok(src) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
        return;
    };
    for ty in [down, up] {
        if let Ok(ev) = CGEvent::new_mouse_event(src.clone(), ty, at, which) {
            ev.set_integer_value_field(
                EventField::MOUSE_EVENT_CLICK_STATE,
                i64::from(clicks.max(1)),
            );
            ev.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTH_TAG);
            ev.post(CGEventTapLocation::HID);
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}
