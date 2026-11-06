//! `SendInput`: shortcuts and replayed mouse buttons. Everything sent carries
//! `SYNTH_TAG` in `dwExtraInfo`, so the hook ignores it.

use crate::actions::SYNTH_TAG;
use crate::geometry::Point;
use crate::keys::*;
use crate::platform::keymap;
use crate::tap::Kind;
use std::time::Duration;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MapVirtualKeyW, SendInput,
};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CMOUSEBUTTONS, SetCursorPos,
};

/// `XBUTTON1` and `XBUTTON2` of `MOUSEINPUT::mouseData`.
const XBUTTON1: u32 = 1;
const XBUTTON2: u32 = 2;

/// A key to send: how it is identified and whether it is an extended key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Native {
    /// Sent by scan code (the key at a physical position; the layout decides what it types).
    Scan(u16),
    /// Sent by virtual-key code, with the scan code Windows reports for it.
    Vk { vk: u16, extended: bool },
}

/// The modifiers in the order they are pressed (the same order as on macOS), as
/// `(MOD_* bit, virtual-key code, extended)`.
pub(super) const MODIFIERS: [(u8, u16, bool); 4] = [
    (MOD_CTRL, 0xA2, false),  // VK_LCONTROL
    (MOD_ALT, 0xA4, false),   // VK_LMENU
    (MOD_SHIFT, 0xA0, false), // VK_LSHIFT
    (MOD_CMD, 0x5B, true),    // VK_LWIN
];

/// The native key for a CursorPie key code: layout keys by scan code, the rest by virtual-key code.
pub(super) fn native_key(code: u16) -> Option<Native> {
    if let Some(scan) = keymap::scancode_for(code) {
        return Some(Native::Scan(scan));
    }
    keymap::vk_for(code).map(|(vk, extended)| Native::Vk { vk, extended })
}

/// The modifier keys to press for `order` (`MOD_*` bits, the one pressed first at the front).
pub(super) fn modifier_sequence_in(order: &[u8]) -> Vec<(u16, bool)> {
    order
        .iter()
        .filter_map(|bit| MODIFIERS.iter().find(|(b, _, _)| b == bit))
        .map(|(_, vk, ext)| (*vk, *ext))
        .collect()
}

/// The modifier keys pressed for `mods`, in the usual order.
#[cfg(test)]
pub(super) fn modifier_sequence(mods: u8) -> Vec<(u16, bool)> {
    modifier_sequence_in(&Shortcut::new(0, mods).ordered_mods())
}

fn keyboard(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTH_TAG as usize,
            },
        },
    }
}

/// Sends one event and waits 3 ms, like the macOS backend does after each key event.
fn send(input: INPUT) {
    // SAFETY: one fully initialised INPUT and its exact size.
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
    std::thread::sleep(Duration::from_millis(3));
}

/// Sends a modifier or the main key down or up.
fn key_event(vk: u16, extended: bool, up: bool) {
    // SAFETY: plain lookup.
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    let mut flags = 0;
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    send(keyboard(vk, scan, flags));
}

fn native_event(key: Native, up: bool) {
    match key {
        Native::Scan(scan) => {
            let flags = KEYEVENTF_SCANCODE | if up { KEYEVENTF_KEYUP } else { 0 };
            send(keyboard(0, scan, flags));
        }
        Native::Vk { vk, extended } => key_event(vk, extended, up),
    }
}

/// Presses the modifiers, the key, then releases everything in reverse order.
pub fn press_shortcut(sc: Shortcut) {
    let Some(key) = native_key(sc.key) else {
        return;
    };
    // pressed in the order the shortcut was recorded in (the order its label shows)
    let mods = modifier_sequence_in(&sc.ordered_mods());
    for (vk, ext) in &mods {
        key_event(*vk, *ext, false);
    }
    native_event(key, false);
    native_event(key, true);
    for (vk, ext) in mods.iter().rev() {
        key_event(*vk, *ext, true);
    }
}

/// The mouse as Windows describes it: one device with the number of buttons the system reports
/// (`SM_CMOUSEBUTTONS`). Windows does not say which mouse it is, so a different number of buttons is
/// what tells a different mouse apart (see [`device_for_button_count`]).
pub fn mouse_devices() -> Vec<crate::mouse::Device> {
    // SAFETY: plain query.
    let buttons = unsafe { GetSystemMetrics(SM_CMOUSEBUTTONS) };
    device_for_button_count(buttons)
}

/// The device for a mouse with `buttons` buttons: left, right and middle, then the two side buttons
/// (`XBUTTON1`, `XBUTTON2`), which are the only extra buttons Windows reports to a hook. No mouse
/// (`buttons` zero or less) gives no device.
pub(super) fn device_for_button_count(buttons: i32) -> Vec<crate::mouse::Device> {
    if buttons <= 0 {
        return Vec::new();
    }
    let extra = (0..(buttons - 3).clamp(0, 2))
        .map(|i| crate::mouse::BACK + i64::from(i))
        .collect();
    vec![crate::mouse::Device {
        name: "Mouse".into(),
        hw_id: format!("buttons:{buttons}"),
        extra,
    }]
}

/// The `MOUSEINPUT` flags and `mouseData` that press or release `button` (macOS numbering:
/// 2 = middle, 3 = back, 4 = forward). `None` for buttons Windows has no event for.
pub(super) fn mouse_flags(kind: Kind, button: i64) -> Option<(u32, u32)> {
    let down = match kind {
        Kind::Down => true,
        Kind::Up => false,
        Kind::Drag => return None,
    };
    match button {
        2 => Some((
            if down {
                MOUSEEVENTF_MIDDLEDOWN
            } else {
                MOUSEEVENTF_MIDDLEUP
            },
            0,
        )),
        3 | 4 => Some((
            if down {
                MOUSEEVENTF_XDOWN
            } else {
                MOUSEEVENTF_XUP
            },
            if button == 3 { XBUTTON1 } else { XBUTTON2 },
        )),
        _ => None,
    }
}

/// Re-posts a swallowed press or release of `button` (used when a quick click must reach the app
/// after all). It is sent at the current pointer position, so `_pos` is not needed.
pub fn post_mouse(_pos: Point, kind: Kind, button: i64) {
    let Some((flags, data)) = mouse_flags(kind, button) else {
        return;
    };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTH_TAG as usize,
            },
        },
    };
    // SAFETY: one fully initialised INPUT and its exact size.
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
}

/// Sends one mouse event with the given `MOUSEINPUT` flags, tagged with [`SYNTH_TAG`].
fn send_mouse_flags(flags: u32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: SYNTH_TAG as usize,
            },
        },
    };
    // SAFETY: one fully initialised INPUT and its exact size.
    unsafe { SendInput(1, &input, std::mem::size_of::<INPUT>() as i32) };
}

/// The `MOUSEINPUT` flags that press and release `button` (0 = left, 1 = right).
pub(super) fn click_flags(button: i64) -> Option<(u32, u32)> {
    match button {
        0 => Some((MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP)),
        1 => Some((MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)),
        _ => None,
    }
}

/// Clicks `button` (0 = left, 1 = right) at `pos` (logical points) and leaves the pointer where it
/// was: the system has one pointer, so it is moved there, clicks, and comes back at once. The
/// number of clicks is not needed: Windows tells a double click from the time and place of two.
pub fn click_at(pos: (f64, f64), button: i64, _clicks: u32) {
    let Some((down, up)) = click_flags(button) else {
        return;
    };
    let Some((x, y)) = super::apps::logical_to_physical(pos) else {
        return;
    };
    let mut back = POINT { x: 0, y: 0 };
    // SAFETY: `back` is a valid out-pointer.
    let have_back = unsafe { GetCursorPos(&mut back) } != 0;
    // SAFETY: plain call.
    unsafe { SetCursorPos(x, y) };
    send_mouse_flags(down);
    std::thread::sleep(Duration::from_millis(12));
    send_mouse_flags(up);
    if have_back {
        // SAFETY: plain call.
        unsafe { SetCursorPos(back.x, back.y) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_has_a_press_and_a_release_for_left_and_right_only() {
        let (ld, lu) = click_flags(0).unwrap();
        let (rd, ru) = click_flags(1).unwrap();
        assert!(ld != lu && rd != ru && ld != rd);
        assert!(click_flags(2).is_none() && click_flags(-1).is_none());
    }

    #[test]
    fn the_mouse_buttons_windows_reports_become_extra_buttons() {
        assert!(device_for_button_count(0).is_empty(), "no mouse");
        assert!(device_for_button_count(-1).is_empty());
        let three = &device_for_button_count(3)[0];
        assert!(three.extra.is_empty());
        let five = &device_for_button_count(5)[0];
        assert_eq!(five.extra, [3, 4], "back and forward");
        assert_eq!(
            device_for_button_count(8)[0].extra,
            [3, 4],
            "the hook only sees XBUTTON1 and XBUTTON2"
        );
        assert_ne!(
            three.id(),
            five.id(),
            "another number of buttons, another mouse"
        );
    }

    #[test]
    fn modifiers_are_pressed_in_a_fixed_order() {
        let all = modifier_sequence(MOD_CTRL | MOD_ALT | MOD_SHIFT | MOD_CMD);
        assert_eq!(
            all,
            [(0xA2, false), (0xA4, false), (0xA0, false), (0x5B, true)]
        );
        assert_eq!(modifier_sequence(MOD_CMD), [(0x5B, true)]);
        assert_eq!(
            modifier_sequence(MOD_CTRL | MOD_SHIFT),
            [(0xA2, false), (0xA0, false)]
        );
        assert!(modifier_sequence(0).is_empty());
    }

    #[test]
    fn letters_are_sent_by_position_and_arrows_by_virtual_key() {
        assert_eq!(native_key(6), Some(Native::Scan(0x2C)), "Z");
        assert_eq!(
            native_key(49),
            Some(Native::Vk {
                vk: 0x20,
                extended: false
            }),
            "Space"
        );
        assert_eq!(
            native_key(126),
            Some(Native::Vk {
                vk: 0x26,
                extended: true
            }),
            "Up"
        );
        assert_eq!(native_key(102), None, "Eisu has no Windows key");
    }

    #[test]
    fn mouse_buttons_map_to_win32_flags() {
        assert_eq!(
            mouse_flags(Kind::Down, 2),
            Some((MOUSEEVENTF_MIDDLEDOWN, 0))
        );
        assert_eq!(mouse_flags(Kind::Up, 2), Some((MOUSEEVENTF_MIDDLEUP, 0)));
        assert_eq!(mouse_flags(Kind::Down, 3), Some((MOUSEEVENTF_XDOWN, 1)));
        assert_eq!(mouse_flags(Kind::Up, 4), Some((MOUSEEVENTF_XUP, 2)));
        assert_eq!(mouse_flags(Kind::Drag, 2), None);
        assert_eq!(mouse_flags(Kind::Down, 6), None);
    }
}
