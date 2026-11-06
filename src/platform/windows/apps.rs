//! Running apps and the pointer: frontmost app, app under the pointer, app icons, activation.
//!
//! The **app id** of a window is the lower-case file name of its process's executable
//! (`chrome.exe`). UWP apps all report `applicationframehost.exe`.

use super::dpi::{last_scale, scale_at_physical};
use super::util::{exe_id_from_path, from_wide, logical_to_px, px_to_logical, wide};
use crate::geometry::Point;
use crate::platform::{NativeIcon, ScreenRect};
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT, S_FALSE, S_OK};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject,
    EnumDisplayMonitors, GetDC, GetDIBits, GetMonitorInfoW, GetObjectW, HDC, HMONITOR, MONITORINFO,
    ReleaseDC,
};
use windows_sys::Win32::Storage::FileSystem::SearchPathW;
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcessId, GetCurrentThreadId, OpenProcess,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, DestroyIcon, EnumWindows, GA_ROOT, GWL_EXSTYLE, GetAncestor, GetClassNameW,
    GetCursorPos, GetForegroundWindow, GetIconInfo, GetWindowLongW, GetWindowTextLengthW,
    GetWindowThreadProcessId, HICON, ICONINFO, IsIconic, IsWindowVisible, PrivateExtractIconsW,
    SW_RESTORE, SetForegroundWindow, ShowWindow, WS_EX_TOOLWINDOW, WindowFromPoint,
};

/// Process id of the window `hwnd`.
pub(super) fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0u32;
    // SAFETY: `pid` is a valid out-pointer.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid
}

/// Full path of the executable of process `pid`.
fn exe_path_of_pid(pid: u32) -> Option<String> {
    // SAFETY: the handle is closed before returning; the buffer length is passed in `len`.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        (ok != 0).then(|| from_wide(&buf[..len as usize]))
    }
}

/// App id of process `pid`.
fn app_id_of_pid(pid: u32) -> Option<String> {
    exe_id_from_path(&exe_path_of_pid(pid)?)
}

/// App id of the application in front.
pub fn frontmost_bundle_id() -> Option<String> {
    // SAFETY: plain query.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return None;
    }
    app_id_of_pid(pid_of(hwnd))
}

/// Window class of `hwnd`.
fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    // SAFETY: the buffer length is passed along.
    let n = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    from_wide(&buf[..n.max(0) as usize])
}

/// Classes of the desktop and the taskbar: they belong to `explorer.exe` but are not "File Explorer".
fn is_shell_surface(class: &str) -> bool {
    matches!(
        class,
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    )
}

/// App id of the app owning the window under `p` (logical points, as sent by the hook; the scale of
/// that same event is used to get back to pixels). `None` for the desktop, the taskbar, CursorPie's
/// own windows and anything that cannot be resolved.
pub fn bundle_id_at(p: Point) -> Option<String> {
    let s = last_scale();
    let pt = POINT {
        x: logical_to_px(p.x, s).round() as i32,
        y: logical_to_px(p.y, s).round() as i32,
    };
    // SAFETY: plain queries.
    unsafe {
        let hwnd = WindowFromPoint(pt);
        if hwnd.is_null() {
            return None;
        }
        let root = GetAncestor(hwnd, GA_ROOT);
        let root = if root.is_null() { hwnd } else { root };
        if is_shell_surface(&class_of(root)) {
            return None;
        }
        let pid = pid_of(root);
        if pid == GetCurrentProcessId() {
            return None;
        }
        app_id_of_pid(pid)
    }
}

/// App id of an `.exe` path chosen in the file chooser.
///
/// The folder of a program that exists is remembered, so its icon can be found later although the
/// program is not running (an app id alone is only a file name).
pub fn bundle_id_of(app_path: &str) -> Option<String> {
    let id = super::util::is_exe(app_path)
        .then(|| exe_id_from_path(app_path))
        .flatten()?;
    super::apppath::remember(app_path.trim().trim_matches('"'));
    Some(id)
}

/// Cursor position in logical points.
pub fn cursor_pos() -> Option<(f64, f64)> {
    let mut p = POINT { x: 0, y: 0 };
    // SAFETY: `p` is a valid out-pointer.
    if unsafe { GetCursorPos(&mut p) } == 0 {
        return None;
    }
    let s = scale_at_physical(p.x, p.y);
    Some((px_to_logical(p.x as f64, s), px_to_logical(p.y as f64, s)))
}

// ------------------------------------------------------------------ displays

unsafe extern "system" fn collect_monitor(
    mon: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> i32 {
    // SAFETY: `data` is the `Vec` passed by `display_rects`, alive for the whole enumeration.
    let out = unsafe { &mut *(data as *mut Vec<ScreenRect>) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(mon, &mut info) } != 0 {
        let (mut dx, mut dy) = (0u32, 0u32);
        let dpi_ok = unsafe { GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } == 0;
        let s = if dpi_ok {
            super::util::scale_from_dpi(dx)
        } else {
            1.0
        };
        let r = info.rcMonitor;
        out.push(ScreenRect {
            x: px_to_logical(r.left as f64, s),
            y: px_to_logical(r.top as f64, s),
            w: px_to_logical((r.right - r.left) as f64, s),
            h: px_to_logical((r.bottom - r.top) as f64, s),
        });
    }
    1
}

/// Bounds of every monitor in logical points.
pub fn display_rects() -> Vec<ScreenRect> {
    let mut out: Vec<ScreenRect> = Vec::new();
    // SAFETY: the callback only runs during this call and `out` outlives it.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect_monitor),
            &mut out as *mut Vec<ScreenRect> as LPARAM,
        );
    }
    out
}

unsafe extern "system" fn collect_scale(
    mon: HMONITOR,
    _dc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> i32 {
    // SAFETY: `data` is the `Vec` passed by `logical_to_physical`, alive for the whole enumeration.
    let out = unsafe { &mut *(data as *mut Vec<(ScreenRect, f64)>) };
    let mut info: MONITORINFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
    if unsafe { GetMonitorInfoW(mon, &mut info) } != 0 {
        let (mut dx, mut dy) = (0u32, 0u32);
        let dpi_ok = unsafe { GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } == 0;
        let s = if dpi_ok {
            super::util::scale_from_dpi(dx)
        } else {
            1.0
        };
        let r = info.rcMonitor;
        out.push((
            ScreenRect {
                x: px_to_logical(r.left as f64, s),
                y: px_to_logical(r.top as f64, s),
                w: px_to_logical((r.right - r.left) as f64, s),
                h: px_to_logical((r.bottom - r.top) as f64, s),
            },
            s,
        ));
    }
    1
}

/// A point in logical points as physical pixels, with the scale of the monitor that holds it (the
/// first monitor when none does). `None` when Windows lists no monitor.
pub(super) fn logical_to_physical(p: (f64, f64)) -> Option<(i32, i32)> {
    let mut rows: Vec<(ScreenRect, f64)> = Vec::new();
    // SAFETY: the callback only runs during this call and `rows` outlives it.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect_scale),
            &mut rows as *mut Vec<(ScreenRect, f64)> as LPARAM,
        );
    }
    let s = rows
        .iter()
        .find(|(r, _)| p.0 >= r.x && p.0 < r.x + r.w && p.1 >= r.y && p.1 < r.y + r.h)
        .or_else(|| rows.first())
        .map(|(_, s)| *s)?;
    Some((
        logical_to_px(p.0, s).round() as i32,
        logical_to_px(p.1, s).round() as i32,
    ))
}

// ---------------------------------------------------------------- activation

struct Find {
    pid_matches: Vec<u32>,
    /// Matching windows, top-most first.
    found: Vec<HWND>,
}

unsafe extern "system" fn pick_window(hwnd: HWND, data: LPARAM) -> i32 {
    // SAFETY: `data` is the `Find` passed by `windows_of`, alive during the enumeration.
    let find = unsafe { &mut *(data as *mut Find) };
    unsafe {
        if IsWindowVisible(hwnd) == 0 || GetWindowTextLengthW(hwnd) == 0 {
            return 1;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW != 0 {
            return 1;
        }
        if find.pid_matches.contains(&pid_of(hwnd)) {
            find.found.push(hwnd);
        }
    }
    1
}

/// Process ids of every running process whose executable is `exe` (a lower-case file name).
fn pids_of_exe(exe: &str) -> Vec<u32> {
    let mut out = Vec::new();
    // SAFETY: the snapshot handle is closed; `entry.dwSize` is set as the API requires.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap as isize == -1 {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry);
        while ok != 0 {
            if from_wide(&entry.szExeFile).eq_ignore_ascii_case(exe) {
                out.push(entry.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut entry);
        }
        CloseHandle(snap);
    }
    out
}

/// The windows of the app `exe` (a lower-case file name) that can be brought to the front, top-most
/// first: visible, with a title, and not a tool window. Windows on other virtual desktops are
/// included.
pub(super) fn windows_of(exe: &str) -> Vec<HWND> {
    let mut find = Find {
        pid_matches: pids_of_exe(exe),
        found: Vec::new(),
    };
    if !find.pid_matches.is_empty() {
        // SAFETY: the callback only runs during `EnumWindows`; `find` outlives it.
        unsafe { EnumWindows(Some(pick_window), &mut find as *mut Find as LPARAM) };
    }
    find.found
}

/// Brings the top-most window of the app `exe` to the front and waits (briefly) until Windows
/// really has it there: keys sent too early would reach the previous app. Windows may refuse
/// to change the foreground window, in which case this gives up after the wait.
pub fn activate_app(exe: &str) {
    if frontmost_bundle_id().as_deref() == Some(exe) {
        return;
    }
    if let Some(&hwnd) = windows_of(exe).first() {
        bring_to_front(exe, hwnd);
    }
}

/// Puts `hwnd`, a window of the app `exe`, in front and waits (briefly) until Windows reports the
/// app as the one in front, like [`activate_app`].
pub(super) fn bring_to_front(exe: &str, hwnd: HWND) {
    // SAFETY: thread attachment is undone right after the calls.
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        // a process may only take the foreground while it shares input state with it
        let fg = GetForegroundWindow();
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
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(15));
        if frontmost_bundle_id().as_deref() == Some(exe) {
            std::thread::sleep(Duration::from_millis(40)); // let its window take the keyboard focus
            return;
        }
    }
}

// --------------------------------------------------------------------- icons

/// Full path of the executable for an app id: a running process first, then the search path, then
/// everywhere else a program can be installed (see [`super::apppath`]).
fn exe_path_for(exe: &str) -> Option<String> {
    if let Some(path) = pids_of_exe(exe).into_iter().find_map(exe_path_of_pid) {
        return Some(path);
    }
    let name = wide(exe);
    let mut buf = [0u16; 1024];
    // SAFETY: the buffer length is passed along; null path/extension mean "default search".
    let n = unsafe {
        SearchPathW(
            std::ptr::null(),
            name.as_ptr(),
            std::ptr::null(),
            buf.len() as u32,
            buf.as_mut_ptr(),
            std::ptr::null_mut(),
        )
    };
    let on_path = (n > 0 && (n as usize) < buf.len()).then(|| from_wide(&buf[..n as usize]));
    on_path.or_else(|| super::apppath::find_exe(exe))
}

/// A 128 px icon of the executable (the size of the glyph texture), or the large shell icon.
fn icon_handle(path: &str) -> Option<HICON> {
    let w = wide(path);
    // SAFETY: `hicon` receives at most one handle, as requested by the `1`.
    unsafe {
        let mut hicon: HICON = std::ptr::null_mut();
        let got = PrivateExtractIconsW(
            w.as_ptr(),
            0,
            128,
            128,
            &mut hicon,
            std::ptr::null_mut(),
            1,
            0,
        );
        if got > 0 && got != u32::MAX && !hicon.is_null() {
            return Some(hicon);
        }
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let r = SHGetFileInfoW(
            w.as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        (r != 0 && !info.hIcon.is_null()).then_some(info.hIcon)
    }
}

/// Reads the pixels of a 32-bit icon bitmap as BGRA, top-down.
pub(super) unsafe fn bitmap_bgra(
    bmp: windows_sys::Win32::Graphics::Gdi::HBITMAP,
    w: i32,
    h: i32,
) -> Option<Vec<u8>> {
    let mut bmi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bmi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // negative: top-down rows
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = unsafe { GetDC(std::ptr::null_mut()) };
    if dc.is_null() || bmp.is_null() {
        return None;
    }
    let lines = unsafe {
        GetDIBits(
            dc,
            bmp,
            0,
            h as u32,
            px.as_mut_ptr().cast(),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    unsafe { ReleaseDC(std::ptr::null_mut(), dc) };
    (lines == h).then_some(px)
}

/// COM, started on the thread that asks for icons and ended with it. The shell's icon call
/// (`SHGetFileInfoW`, the fallback of [`icon_handle`]) needs COM on its thread. The UI thread has it
/// already; the worker threads that read icons for the app list (see `ui::icons`) get it here, the
/// first time they ask, so the caller does not have to know.
struct ComForThread {
    /// Whether this value has to balance a successful `CoInitializeEx`.
    uninitialize: bool,
}

impl ComForThread {
    fn enter() -> Self {
        // SAFETY: plain COM setup of this thread; `Drop` undoes it when the thread ends.
        let init = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        // S_OK and S_FALSE both count a use that has to be balanced; a thread that runs COM in the
        // other mode (RPC_E_CHANGED_MODE) can use it all the same, but is not ours to close
        Self {
            uninitialize: init == S_OK || init == S_FALSE,
        }
    }
}

impl Drop for ComForThread {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: balances the successful `CoInitializeEx` of `enter`, on the same thread.
            unsafe { CoUninitialize() };
        }
    }
}

thread_local! {
    static COM: ComForThread = ComForThread::enter();
}

/// Makes sure COM runs on the calling thread (it is ended with the thread).
pub(super) fn com_for_thread() {
    COM.with(|_| {});
}

/// The real icon of the app with this id as RGBA, or `None` when the app cannot be found.
///
/// Slow (a scan of the running processes, then the executable is read): call it off the UI thread
/// when many icons are needed, as the app list does.
pub fn app_icon(exe: &str) -> Option<NativeIcon> {
    com_for_thread(); // COM for this thread, once
    let path = exe_path_for(exe)?;
    // The shell's own icon first: the one Explorer shows, 256 px, with its alpha channel. The older
    // calls below stay as the fallback for a file the shell cannot parse.
    if let Some((width, height, data)) = super::shellitem::shell_icon_rgba(&path, 256) {
        return Some(NativeIcon::Rgba {
            width,
            height,
            data,
        });
    }
    let hicon = icon_handle(&path)?;
    // SAFETY: every GDI object created below is deleted, and the icon handle destroyed.
    unsafe {
        let mut ii: ICONINFO = std::mem::zeroed();
        if GetIconInfo(hicon, &mut ii) == 0 {
            DestroyIcon(hicon);
            return None;
        }
        let mut bm: BITMAP = std::mem::zeroed();
        let ok = GetObjectW(
            ii.hbmColor,
            std::mem::size_of::<BITMAP>() as i32,
            (&mut bm as *mut BITMAP).cast(),
        );
        // a sane icon: positive and not absurdly big (the buffers below are `w * h * 4` bytes)
        let sane = ok != 0 && (1..=4096).contains(&bm.bmWidth) && (1..=4096).contains(&bm.bmHeight);
        let result = sane
            .then(|| {
                let (w, h) = (bm.bmWidth, bm.bmHeight);
                let mut color = bitmap_bgra(ii.hbmColor, w, h)?;
                let mask = bitmap_bgra(ii.hbmMask, w, h).map(|m| {
                    m.as_chunks::<4>()
                        .0
                        .iter()
                        .map(|p| p[0])
                        .collect::<Vec<u8>>()
                });
                super::util::bgra_to_rgba(&mut color, mask.as_deref());
                Some(NativeIcon::Rgba {
                    width: w as u32,
                    height: h as u32,
                    data: color,
                })
            })
            .flatten();
        DeleteObject(ii.hbmColor);
        DeleteObject(ii.hbmMask);
        DestroyIcon(hicon);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_and_the_taskbar_are_not_apps() {
        for c in [
            "Progman",
            "WorkerW",
            "Shell_TrayWnd",
            "Shell_SecondaryTrayWnd",
        ] {
            assert!(is_shell_surface(c));
        }
        assert!(
            !is_shell_surface("CabinetWClass"),
            "a File Explorer window is an app"
        );
        assert!(!is_shell_surface("Chrome_WidgetWin_1"));
    }

    #[test]
    fn app_ids_come_from_exe_paths_only() {
        assert_eq!(bundle_id_of(r"C:\Apps\Code.exe"), Some("code.exe".into()));
        assert_eq!(bundle_id_of(r"C:\Apps\Code.lnk"), None);
    }
}
