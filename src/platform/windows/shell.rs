//! Launching things and the file chooser: `ShellExecuteW`, `explorer`, `GetOpenFileNameW`.

use super::util::{filter_for, from_wide, select_params, shell_params, wide};
use crate::platform::FileKind;
use std::path::Path;
use windows_sys::Win32::System::Diagnostics::Debug::MessageBeep;
use windows_sys::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{MB_OK, SW_HIDE, SW_SHOWNORMAL};

/// `ShellExecuteW(open, file, params)`. Failures are ignored (an app that cannot start has nothing
/// to report to a pie slice); the call returns at once.
fn shell_execute(file: &str, params: Option<&str>, show: i32) {
    let verb = wide("open");
    let file = wide(file);
    let params = params.map(wide);
    // SAFETY: every string is zero terminated and outlives the call.
    unsafe {
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            params.as_ref().map_or(std::ptr::null(), |p| p.as_ptr()),
            std::ptr::null(),
            show,
        );
        // a value above 32 means it worked; below that it is an error code (2 and 3: not found,
        // 5: access denied). Only the code is logged, never what was opened.
        diag::log!(
            "act",
            "shell_execute",
            ok = (result as isize) > 32,
            code = result as isize
        );
    }
}

/// Launches an app by name (`chrome`, resolved through App Paths and `PATH`) or by path.
pub fn open_app(app: &str) {
    shell_execute(app, None, SW_SHOWNORMAL);
}

/// Opens a URL or a file with its default handler.
pub fn open_url(target: &str) {
    shell_execute(target, None, SW_SHOWNORMAL);
}

/// Opens a folder or file with its default app.
pub fn open_path(path: &Path) {
    shell_execute(&path.to_string_lossy(), None, SW_SHOWNORMAL);
}

/// Runs a shell command with `cmd.exe`, without a console window.
pub fn run_shell(cmd: &str) {
    shell_execute("cmd.exe", Some(&shell_params(cmd)), SW_HIDE);
}

/// Shows a file in an Explorer window.
pub fn reveal_in_file_manager(path: &Path) {
    shell_execute(
        "explorer.exe",
        Some(&select_params(&path.to_string_lossy())),
        SW_SHOWNORMAL,
    );
}

/// The standard system beep as hover feedback (returns at once).
pub fn tick() {
    // SAFETY: plain call.
    unsafe { MessageBeep(MB_OK) };
}

/// The native open dialog. Blocks until the user answers, so call it off the UI thread.
pub fn choose_file(kind: FileKind, prompt: &str) -> Option<String> {
    let filter = filter_for(kind);
    let title = wide(prompt);
    let mut file = [0u16; 1024];
    // SAFETY: an all-zero OPENFILENAMEW is valid; the pointers set below outlive the call.
    let ok = unsafe {
        let mut ofn: OPENFILENAMEW = std::mem::zeroed();
        ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        ofn.lpstrFilter = filter.as_ptr();
        ofn.nFilterIndex = 1;
        ofn.lpstrFile = file.as_mut_ptr();
        ofn.nMaxFile = file.len() as u32;
        ofn.lpstrTitle = title.as_ptr();
        ofn.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        GetOpenFileNameW(&mut ofn)
    };
    if ok == 0 {
        return None; // cancelled
    }
    let p = from_wide(&file);
    (!p.is_empty()).then_some(p)
}
