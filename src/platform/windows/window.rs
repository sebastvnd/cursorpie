//! The frame of the one native window: makes it truly frameless, so nothing but what egui paints
//! is visible (no white box, no border, no shadow).
//!
//! winit builds every window from `WS_CAPTION | WS_BORDER` (plus `WS_EX_WINDOWEDGE`) and only
//! *hides* the title bar for `with_decorations(false)`, by answering `WM_NCCALCSIZE` with "the
//! client area is the whole window". The styles stay, so the DWM still treats the window as a
//! framed one: it draws its 1 px border (Windows 11) and shadow around it, and fills the frame with
//! the inactive-caption color, which is white. The pie window is never activated, so it always
//! looks like that. winit also writes the styles again from its own flags on every
//! visibility / click-through / window-level change, which is why [`clear_window_frame`] is
//! idempotent and is called on every frame (see `docs/UI_DESIGN.md`, "Transparent window on Windows").

use super::util::{frameless_ex_style, frameless_style, frameless_styles};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DWM_BB_BLURREGION, DWM_BB_ENABLE, DWM_BLURBEHIND, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND, DwmEnableBlurBehindWindow,
    DwmSetWindowAttribute,
};
use windows_sys::Win32::Graphics::Gdi::{CreateRectRgn, DeleteObject};
use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GWL_EXSTYLE, GWL_STYLE, GetForegroundWindow, GetWindowLongW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, STYLESTRUCT, SW_RESTORE,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetForegroundWindow,
    SetWindowLongW, SetWindowPos, ShowWindow, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCPAINT,
    WM_STYLECHANGING,
};

/// Id of our subclass (any value; it only has to be the same for install and removal).
const SUBCLASS_ID: usize = 0x0C0F_FEE0;

/// Runs before winit's window procedure.
///
/// * `WM_STYLECHANGING` arrives *before* `SetWindowLong` applies a new style, and the style it
///   carries can still be changed. winit rewrites the styles from its own flags on every show /
///   hide / click-through change and puts the caption back; fixing it here means the window is
///   **never** framed, not even for the moment between winit's write and the per-frame fix (that
///   moment was the old-style white window flashing up when the pie opens).
/// * `WM_NCCALCSIZE` is answered here, so winit's 1 px top strip (undecorated shadow) never exists.
/// * `WM_NCPAINT` is swallowed so the non-client area is never painted (there is none left, but
///   this also covers the first paint after a style change: the thin white line at the top).
unsafe extern "system" fn frameless_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        WM_STYLECHANGING if lparam != 0 => {
            // SAFETY: for this message `lparam` points to a STYLESTRUCT owned by the sender.
            let ss = unsafe { &mut *(lparam as *mut STYLESTRUCT) };
            match wparam as i32 {
                GWL_STYLE => ss.styleNew = frameless_style(ss.styleNew),
                GWL_EXSTYLE => ss.styleNew = frameless_ex_style(ss.styleNew),
                _ => {}
            }
        }
        // The client area is the whole window, always. egui-winit asks winit for the "undecorated
        // shadow" of every borderless window, and winit then answers WM_NCCALCSIZE by pushing the
        // client area down by 1 px, which leaves a 1 px strip at the top that the DWM paints
        // white: the thin line above the pie / Settings. Returning 0 keeps the rectangle as is.
        WM_NCCALCSIZE if wparam != 0 => return 0,
        WM_NCPAINT => return 0,
        WM_NCDESTROY => {
            // SAFETY: removing our own subclass while the window is being destroyed.
            unsafe { RemoveWindowSubclass(hwnd, Some(frameless_proc), SUBCLASS_ID) };
        }
        _ => {}
    }
    // SAFETY: passes the message on down the subclass chain, to winit's procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// The window the DWM attributes were last applied to (they stay on the window, so once is enough).
static DWM_DONE: AtomicIsize = AtomicIsize::new(0);

/// Whether the window should not take the focus when it is shown (the notice of an action, see
/// [`set_window_passive`]).
static PASSIVE: AtomicBool = AtomicBool::new(false);

/// The window of the notice on the screen (`ui::hud`) must not take the keyboard from the app the
/// action is for, so while it is on, the window gets `WS_EX_NOACTIVATE`: a click-through window
/// that is shown without being activated. Applied at once to the window the frame code has seen, and
/// again by every `clear_window_frame` (winit rewrites the styles when it shows or hides the window).
pub fn set_window_passive(on: bool) {
    PASSIVE.store(on, Ordering::Relaxed);
    let handle = DWM_DONE.load(Ordering::Relaxed);
    if handle != 0 {
        apply_passive(handle as HWND);
    }
}

/// Sets or clears `WS_EX_NOACTIVATE` on `hwnd` to match [`PASSIVE`].
fn apply_passive(hwnd: HWND) {
    use windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE;
    // SAFETY: only reads and writes the extended style of a window of this process, after IsWindow.
    unsafe {
        if IsWindow(hwnd) == 0 {
            return;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let want = if PASSIVE.load(Ordering::Relaxed) {
            ex | WS_EX_NOACTIVATE
        } else {
            ex & !WS_EX_NOACTIVATE
        };
        if want != ex {
            SetWindowLongW(hwnd, GWL_EXSTYLE, want as i32);
        }
    }
}

/// Removes the frame of the window `handle` (an `HWND`; 0 is ignored).
///
/// * the frame styles are stripped and the window becomes a plain `WS_POPUP`, which is what the
///   DWM treats as "nothing to decorate" (the same state GLFW's transparent framebuffer uses);
/// * once per window: no border color, no rounded corners (Windows 11), and the empty blur region
///   that makes the OpenGL alpha channel show through (winit sets it too; this only re-asserts it).
///
/// Cheap when there is nothing to do (two style reads), so it can run on every frame.
pub fn clear_window_frame(handle: isize) {
    if handle == 0 {
        return;
    }
    let hwnd = handle as HWND;
    // SAFETY: `hwnd` is checked with IsWindow; every call only reads or sets window state, and the
    // out-pointers (`pvattribute`, the blur struct) are valid for the duration of each call.
    unsafe {
        if IsWindow(hwnd) == 0 {
            return;
        }
        let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
        let style_ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if let Some((style, style_ex)) = frameless_styles(style, style_ex) {
            SetWindowLongW(hwnd, GWL_STYLE, style as i32);
            SetWindowLongW(hwnd, GWL_EXSTYLE, style_ex as i32);
            // the new styles only take effect after a "frame changed" notification; the window
            // keeps its position, size, stacking and focus
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        apply_passive(hwnd);
        take_pending_raise(hwnd);
        if DWM_DONE.swap(handle, Ordering::Relaxed) != handle {
            // keeps the frame away for good (see `frameless_proc`); the window belongs to the UI
            // thread, which is the thread that calls this
            SetWindowSubclass(hwnd, Some(frameless_proc), SUBCLASS_ID, 0);
            // Windows 11 only: Windows 10 rejects these attributes, which is fine
            let color: u32 = DWMWA_COLOR_NONE;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR as u32,
                (&raw const color).cast(),
                size_of::<u32>() as u32,
            );
            let corners: i32 = DWMWCP_DONOTROUND;
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                (&raw const corners).cast(),
                size_of::<i32>() as u32,
            );
            // an empty region = no blur, but the window composes with its alpha channel
            let region = CreateRectRgn(0, 0, -1, -1);
            let blur = DWM_BLURBEHIND {
                dwFlags: DWM_BB_ENABLE | DWM_BB_BLURREGION,
                fEnable: 1,
                hRgnBlur: region,
                fTransitionOnMaximized: 0,
            };
            DwmEnableBlurBehindWindow(hwnd, &blur);
            DeleteObject(region);
        }
    }
}

/// Windows keeps the position a hidden window was given.
pub fn window_manager_may_recenter() -> bool {
    false
}

/// A window is placed in the same points the pointer is in.
pub fn window_point(x: f64, y: f64) -> (f64, f64) {
    (x, y)
}

/// The pie is drawn in the same points the pointer moves in.
pub fn window_scale(_x: f64, _y: f64) -> f64 {
    1.0
}

/// How long a request to raise the window stays valid. A request made while the window is still
/// hidden (Settings is being shown) waits for it to appear; one that never gets the chance is
/// dropped, so it cannot bring Settings forward long after the person asked for something else.
const RAISE_WAIT: Duration = Duration::from_millis(600);

/// When the pending request to bring the window forward runs out (`None` = nothing pending).
static RAISE_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// Asks for the window to be brought in front of everything, without the toolkit's focus request.
///
/// winit's `focus_window` takes the foreground by injecting an Alt key press and release (the
/// classic trick to get past `SetForegroundWindow`'s restrictions). The target of that Alt tap is
/// the window that is in front when the next click arrives, and Windows treats the click as the end
/// of a menu activation: it is eaten. After Settings had been opened (or raised by a press of the
/// trigger while it was behind another window), the first click, and with it the first hold of the
/// pie, did nothing, and only the second one worked.
///
/// So on Windows the UI does not send `ViewportCommand::Focus`; it calls this instead. The request
/// is carried out by the next frame (`clear_window_frame`, which runs even while the window is
/// hidden), once the window is on screen, with `SetForegroundWindow` and no injected key.
pub fn raise_own_window() {
    if let Ok(mut until) = RAISE_UNTIL.lock() {
        *until = Some(Instant::now() + RAISE_WAIT);
    }
}

/// Carries out a pending [`raise_own_window`] as soon as the window is visible.
fn take_pending_raise(hwnd: HWND) {
    let Ok(mut until) = RAISE_UNTIL.lock() else {
        return;
    };
    let Some(deadline) = *until else {
        return;
    };
    // SAFETY: `hwnd` was checked with IsWindow by the caller; the calls only change window state.
    unsafe {
        if IsWindowVisible(hwnd) == 0 {
            if Instant::now() >= deadline {
                *until = None;
            }
            return; // not on screen yet: ask again next frame
        }
        *until = None;
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        let fg = GetForegroundWindow();
        if fg == hwnd {
            return;
        }
        // a process may only take the foreground while it shares input state with the window in
        // front: attach to its thread for the duration of the call (no synthetic key needed)
        let fg_thread = if fg.is_null() {
            0
        } else {
            GetWindowThreadProcessId(fg, std::ptr::null_mut())
        };
        let me = GetCurrentThreadId();
        let attached =
            fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, 1) != 0;
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        if attached {
            AttachThreadInput(me, fg_thread, 0);
        }
    }
}
