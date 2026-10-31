//! Windows hook: `WH_MOUSE_LL` and `WH_KEYBOARD_LL` on their own thread with a message loop.
//!
//! [`spawn`] installs both hooks, publishes `TAP_ACTIVE` and pumps messages; when Windows refuses
//! the hooks it retries every 2 seconds. Native events are translated into the portable
//! `Ev` and routed by `dispatch`.
//!
//! Differences from the macOS tap, all handled here:
//!
//! * Windows has no "drag" event: a pointer move arrives as a plain move. Moves are only looked at
//!   while a gesture is in progress, and they are **never swallowed** (dropping them would freeze
//!   the cursor), whatever the state machine answers.
//! * Coordinates are physical pixels and are converted to logical points with the monitor scale.
//! * Windows silently removes a low-level hook whose callback takes too long (about 300 ms by
//!   default). The callback therefore only does atomic loads and short locks, like the macOS tap.

use super::{Ev, Gesture, Kind, Verdict, dispatch};
use crate::actions::SYNTH_TAG;
use crate::config::{cfg, lock};
use crate::geometry::Point;
use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT};
use crate::msg::Notifier;
use crate::platform::keymap;
use crate::platform::windows::{px_to_logical, scale_at_physical, set_last_scale};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, MSG, MSLLHOOKSTRUCT,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP,
    WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

/// What the hook callbacks need. Hooks have no user-data pointer, so it lives in a static.
struct Ctx {
    state: Arc<Mutex<Gesture>>,
    notifier: Notifier,
}

static CTX: OnceLock<Ctx> = OnceLock::new();

/// Classifies a mouse message: `(kind, macOS button number)`. `mouse_data` is
/// `MSLLHOOKSTRUCT::mouseData` (the high word names the X button). `None` for left-button events
/// and everything else CursorPie does not use. A plain move is `(Kind::Drag, 0)`: the caller decides
/// which button it belongs to.
pub(super) fn classify_mouse(msg: u32, mouse_data: u32) -> Option<(Kind, i64)> {
    let x_button = || match mouse_data >> 16 {
        1 => Some(super::SIDE_BACK),
        2 => Some(super::SIDE_FORWARD),
        _ => None,
    };
    match msg {
        WM_MBUTTONDOWN => Some((Kind::Down, 2)),
        WM_MBUTTONUP => Some((Kind::Up, 2)),
        WM_RBUTTONDOWN => Some((Kind::Down, super::RIGHT_BUTTON)),
        WM_RBUTTONUP => Some((Kind::Up, super::RIGHT_BUTTON)),
        WM_XBUTTONDOWN => x_button().map(|b| (Kind::Down, b)),
        WM_XBUTTONUP => x_button().map(|b| (Kind::Up, b)),
        WM_MOUSEMOVE => Some((Kind::Drag, 0)),
        _ => None,
    }
}

/// Whether a keyboard message is a key press (`Some(true)`), a release (`Some(false)`) or neither.
pub(super) fn key_is_down(msg: u32) -> Option<bool> {
    match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
        WM_KEYUP | WM_SYSKEYUP => Some(false),
        _ => None,
    }
}

/// Whether a modifier key is held right now.
fn held(vk: i32) -> bool {
    // SAFETY: plain query; the high bit means "down".
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

/// The `MOD_*` bitmask of the modifiers held right now.
fn current_mods() -> u8 {
    let mut m = 0;
    if held(0x10) {
        m |= MOD_SHIFT;
    }
    if held(0x11) {
        m |= MOD_CTRL;
    }
    if held(0x12) {
        m |= MOD_ALT;
    }
    if held(0x5B) || held(0x5C) {
        m |= MOD_CMD;
    }
    m
}

/// Handles one mouse event. `true` = swallow.
fn on_mouse(ctx: &Ctx, msg: u32, info: &MSLLHOOKSTRUCT) -> bool {
    if info.dwExtraInfo == SYNTH_TAG as usize {
        return false; // our own replay
    }
    let Some((kind, mut button)) = classify_mouse(msg, info.mouseData) else {
        return false;
    };
    let is_move = kind == Kind::Drag;
    if is_move {
        // a move only matters while a gesture is running, and belongs to the trigger button
        if matches!(*lock(&ctx.state), Gesture::Idle) {
            return false;
        }
        button = cfg().trigger_button;
    }
    let scale = scale_at_physical(info.pt.x, info.pt.y);
    set_last_scale(scale);
    let pos = Point::new(
        px_to_logical(info.pt.x as f64, scale),
        px_to_logical(info.pt.y as f64, scale),
    );
    // a real drag needs no replay: moves are never swallowed here
    let verdict = dispatch(
        &ctx.state,
        &ctx.notifier,
        Ev::Mouse { kind, button, pos },
        &|| {},
    );
    !is_move && verdict == Verdict::Drop
}

/// Handles one key event. `true` = swallow.
fn on_key(ctx: &Ctx, msg: u32, info: &KBDLLHOOKSTRUCT) -> bool {
    if info.dwExtraInfo == SYNTH_TAG as usize {
        return false;
    }
    let Some(down) = key_is_down(msg) else {
        return false;
    };
    let extended = info.flags & LLKHF_EXTENDED != 0;
    let Some(code) = keymap::code_for_event(info.vkCode as u16, info.scanCode as u16, extended)
    else {
        return false; // modifiers and keys without a code pass through
    };
    let ev = Ev::Key {
        down,
        code: code as i32,
        mods: current_mods(),
    };
    dispatch(&ctx.state, &ctx.notifier, ev, &|| {}) == Verdict::Drop
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(ctx) = CTX.get()
    {
        // SAFETY: for WH_MOUSE_LL with HC_ACTION, `lparam` points to a valid MSLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        // never panic out of a hook: fail open
        let swallow = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_mouse(ctx, wparam as u32, info)
        }))
        .unwrap_or(false);
        if swallow {
            return 1;
        }
    }
    // SAFETY: forwards the arguments unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn key_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(ctx) = CTX.get()
    {
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, `lparam` points to a valid KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let swallow = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_key(ctx, wparam as u32, info)
        }))
        .unwrap_or(false);
        if swallow {
            return 1;
        }
    }
    // SAFETY: forwards the arguments unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Starts the hooks on their own thread and keeps retrying until Windows accepts them.
pub fn spawn(notifier: Notifier) {
    let state = Arc::new(Mutex::new(Gesture::Idle));
    let _ = CTX.set(Ctx {
        state,
        notifier: notifier.clone(),
    });
    std::thread::Builder::new()
        .name("cursorpie-tap".into())
        .spawn(move || {
            let mut warned = false;
            loop {
                // SAFETY: both hooks are installed on this thread, which then pumps messages (a
                // low-level hook is only called while its thread does), and removed afterwards.
                unsafe {
                    let module = GetModuleHandleW(std::ptr::null());
                    let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
                    let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), module, 0);
                    if mouse.is_null() || keys.is_null() {
                        for h in [mouse, keys] {
                            if !h.is_null() {
                                UnhookWindowsHookEx(h);
                            }
                        }
                        super::TAP_ACTIVE.store(false, Ordering::SeqCst);
                        if !warned {
                            warned = true;
                            diag::log!(
                                "tap",
                                "hook",
                                backend = "windows",
                                state = "refused",
                                os_error = std::io::Error::last_os_error().to_string()
                            );
                            eprintln!(
                                "CursorPie: Windows refused the input hooks ({}). Retrying every 2 s...",
                                std::io::Error::last_os_error()
                            );
                        }
                        notifier.ctx.request_repaint();
                    } else {
                        super::TAP_ACTIVE.store(true, Ordering::SeqCst);
                        diag::log!("tap", "hook", backend = "windows", state = "running");
                        notifier.ctx.request_repaint();
                        let mut msg: MSG = std::mem::zeroed();
                        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {}
                        super::TAP_ACTIVE.store(false, Ordering::SeqCst);
                        diag::log!("tap", "hook", backend = "windows", state = "stopped");
                        UnhookWindowsHookEx(mouse);
                        UnhookWindowsHookEx(keys);
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("failed to spawn event tap thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_messages_map_to_macos_button_numbers() {
        assert_eq!(classify_mouse(WM_MBUTTONDOWN, 0), Some((Kind::Down, 2)));
        assert_eq!(classify_mouse(WM_MBUTTONUP, 0), Some((Kind::Up, 2)));
        assert_eq!(classify_mouse(WM_RBUTTONDOWN, 0), Some((Kind::Down, 1)));
        assert_eq!(classify_mouse(WM_RBUTTONUP, 0), Some((Kind::Up, 1)));
        assert_eq!(classify_mouse(WM_MOUSEMOVE, 0), Some((Kind::Drag, 0)));
    }

    #[test]
    fn x_buttons_are_back_and_forward() {
        let xb1 = 1u32 << 16;
        let xb2 = 2u32 << 16;
        assert_eq!(classify_mouse(WM_XBUTTONDOWN, xb1), Some((Kind::Down, 3)));
        assert_eq!(classify_mouse(WM_XBUTTONUP, xb1), Some((Kind::Up, 3)));
        assert_eq!(classify_mouse(WM_XBUTTONDOWN, xb2), Some((Kind::Down, 4)));
        assert_eq!(classify_mouse(WM_XBUTTONUP, xb2), Some((Kind::Up, 4)));
        assert_eq!(
            classify_mouse(WM_XBUTTONDOWN, 3 << 16),
            None,
            "no third X button"
        );
        assert_eq!(
            classify_mouse(WM_XBUTTONDOWN, 0xFFFF),
            None,
            "the low word is not the button"
        );
    }

    #[test]
    fn the_left_button_and_the_wheel_are_ignored() {
        assert_eq!(classify_mouse(0x0201, 0), None, "WM_LBUTTONDOWN");
        assert_eq!(classify_mouse(0x0202, 0), None, "WM_LBUTTONUP");
        assert_eq!(classify_mouse(0x020A, 0), None, "WM_MOUSEWHEEL");
    }

    #[test]
    fn key_messages() {
        assert_eq!(key_is_down(WM_KEYDOWN), Some(true));
        assert_eq!(key_is_down(WM_SYSKEYDOWN), Some(true));
        assert_eq!(key_is_down(WM_KEYUP), Some(false));
        assert_eq!(key_is_down(WM_SYSKEYUP), Some(false));
        assert_eq!(key_is_down(0x0102), None, "WM_CHAR");
    }
}
