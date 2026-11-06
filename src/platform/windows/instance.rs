//! One running copy at a time.
//!
//! Two copies would each install their own mouse hook, so every gesture would fire twice. This
//! happens easily once the app starts at login: the user double-clicks the shortcut while the tray
//! icon is already there.

use super::util::wide;
use crate::about::APP_NAME;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowExW, FindWindowW};

/// Claims the right to run. Returns `false` when another copy of this app already runs in this
/// user session; the caller should exit quietly.
pub fn single_instance() -> bool {
    // `Local\` keeps the name per session, so two users on one PC do not block each other.
    let name = wide(&format!(r"Local\{APP_NAME}-single-instance"));
    // SAFETY: `name` is zero terminated; null security attributes are allowed.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return true; // cannot tell: running twice is better than not running
    }
    // SAFETY: plain call, made right after the one that sets the error.
    let taken = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // The handle is deliberately never closed: the mutex must live as long as the process does.
    !taken
}

/// Whether the taskbar exists and has its notification area, the place the tray icon goes to.
fn tray_ready(taskbar: &[u16], notify_area: &[u16]) -> bool {
    // SAFETY: both class names are zero terminated and outlive the calls; null window titles and a
    // null previous child are allowed.
    unsafe {
        let bar = FindWindowW(taskbar.as_ptr(), std::ptr::null());
        !bar.is_null()
            && !FindWindowExW(
                bar,
                std::ptr::null_mut(),
                notify_area.as_ptr(),
                std::ptr::null(),
            )
            .is_null()
    }
}

/// Waits until the desktop is ready after a login: the taskbar (`Shell_TrayWnd`) exists and has its
/// notification area (`TrayNotifyWnd`), plus a short pause. Called only when the startup entry
/// started the program (`--autostart`).
///
/// A Run-key program starts while Explorer is still building the desktop and while the antivirus
/// scans the new session. Creating the tray icon that early fails (the app then runs without a
/// tray, with no way to quit it), and the input hooks can be refused. The taskbar window exists
/// well before its notification area does, so both are waited for. Gives up after 90 seconds and
/// carries on, so a PC without the standard shell still starts the app.
pub fn wait_for_login_ready() {
    let taskbar = wide("Shell_TrayWnd");
    let notify_area = wide("TrayNotifyWnd");
    let deadline = Instant::now() + Duration::from_secs(90);
    while !tray_ready(&taskbar, &notify_area) {
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    std::thread::sleep(Duration::from_secs(3));
}
