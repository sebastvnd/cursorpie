//! Pure helpers of the Windows backend (no Win32 calls), unit tested on every OS.

use crate::platform::FileKind;

/// UTF-16 with a terminating zero, for Win32 `PCWSTR` parameters.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Text up to the first zero of a UTF-16 buffer.
pub fn from_wide(buf: &[u16]) -> String {
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..n])
}

/// The app id of an executable: its file name in lower case (`C:\Apps\Chrome.EXE` -> `chrome.exe`).
/// `None` for an empty name.
pub fn exe_id_from_path(path: &str) -> Option<String> {
    let name = path.rsplit(['\\', '/']).next()?.trim();
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// Whether a window class is the frame that `ApplicationFrameHost.exe` draws around a Store (UWP)
/// app.
pub fn is_uwp_frame_class(class: &str) -> bool {
    class == "ApplicationFrameWindow"
}

/// Whether a window class is the window a Store (UWP) app draws its content in, inside the frame of
/// [`is_uwp_frame_class`]. It belongs to the process of the app, not to the host.
pub fn is_core_window_class(class: &str) -> bool {
    class == "Windows.UI.Core.CoreWindow"
}

/// The app id that the text of an "Open app" slice stands for, to find that app among the running
/// ones: `chrome`, `Chrome.exe` and `C:\Tools\Chrome.exe` all give `chrome.exe`. The text is what
/// `ShellExecuteW` accepts, which is more than an executable name (`chrome` is resolved through App
/// Paths), so the `.exe` suffix is added when it is missing. `None` for blank text or a path that
/// ends in a separator.
pub fn app_id_for_open(app: &str) -> Option<String> {
    let id = exe_id_from_path(app.trim().trim_matches('"'))?;
    Some(if id.ends_with(".exe") {
        id
    } else {
        format!("{id}.exe")
    })
}

/// A rectangle in pixels: `(left, top, right, bottom)`.
pub type PxRect = (i32, i32, i32, i32);

/// Where a window goes when it is moved to another monitor: the same size, scaled by the ratio of
/// the two monitors' DPI (`src_dpi` of the monitor it is on, `dst_dpi` of the one it moves to; a
/// zero DPI counts as no scaling) and cut down to the work area `work` of the target if it would not
/// fit, centered in that work area. Used for the new window of an app, which Windows opens on the
/// monitor of its own choice (usually the main one), so it can follow the pie to the monitor the pie
/// was opened on.
pub fn centered_on_work(win: PxRect, src_dpi: u32, work: PxRect, dst_dpi: u32) -> PxRect {
    let ratio = if src_dpi == 0 || dst_dpi == 0 {
        1.0
    } else {
        f64::from(dst_dpi) / f64::from(src_dpi)
    };
    let (work_w, work_h) = ((work.2 - work.0).max(1), (work.3 - work.1).max(1));
    let w = (f64::from(win.2 - win.0) * ratio).round() as i32;
    let h = (f64::from(win.3 - win.1) * ratio).round() as i32;
    let (w, h) = (w.clamp(1, work_w), h.clamp(1, work_h));
    let left = work.0 + (work_w - w) / 2;
    let top = work.1 + (work_h - h) / 2;
    (left, top, left + w, top + h)
}

/// A GUID as one number, so two of them can be compared and sorted (`GUID` has no `PartialEq`).
pub fn guid_to_u128(g: &windows_sys::core::GUID) -> u128 {
    (g.data1 as u128) << 96
        | (g.data2 as u128) << 80
        | (g.data3 as u128) << 64
        | u64::from_be_bytes(g.data4) as u128
}

/// [`signature`](crate::platform::desktops::signature) of the desktop ids that the windows of a
/// desktop report. Zero ids (a window that reports none) are left out.
pub fn desktop_signature(mut ids: Vec<u128>) -> Option<u64> {
    ids.retain(|&id| id != 0);
    crate::platform::desktops::signature(ids)
}

/// Whether a path names an executable.
pub fn is_exe(path: &str) -> bool {
    path.trim().to_ascii_lowercase().ends_with(".exe")
}

/// Scale factor of a monitor from its DPI (96 dpi = 100 %).
pub fn scale_from_dpi(dpi: u32) -> f64 {
    if dpi == 0 { 1.0 } else { dpi as f64 / 96.0 }
}

/// Physical pixels to logical points at `scale`.
pub fn px_to_logical(px: f64, scale: f64) -> f64 {
    if scale <= 0.0 { px } else { px / scale }
}

/// Logical points to physical pixels at `scale`.
pub fn logical_to_px(pt: f64, scale: f64) -> f64 {
    if scale <= 0.0 { pt } else { pt * scale }
}

/// Window styles of a frameless window: the caption (title bar + border + dialog frame) and the
/// sizing frame are removed, the window becomes a `WS_POPUP`, and the 3D edge styles that make the
/// DWM draw a frame are cleared. `None` when `style` / `style_ex` are already like that.
pub fn frameless_styles(style: u32, style_ex: u32) -> Option<(u32, u32)> {
    let new = (frameless_style(style), frameless_ex_style(style_ex));
    (new != (style, style_ex)).then_some(new)
}

/// The `GWL_STYLE` half of [`frameless_styles`].
pub fn frameless_style(style: u32) -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{WS_CAPTION, WS_POPUP, WS_THICKFRAME};
    (style & !(WS_CAPTION | WS_THICKFRAME)) | WS_POPUP
}

/// The `GWL_EXSTYLE` half of [`frameless_styles`].
pub fn frameless_ex_style(style_ex: u32) -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WS_EX_CLIENTEDGE, WS_EX_DLGMODALFRAME, WS_EX_STATICEDGE, WS_EX_WINDOWEDGE,
    };
    style_ex & !(WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_DLGMODALFRAME | WS_EX_STATICEDGE)
}

/// The value written to the `Run` registry key: the executable path in quotes.
pub fn run_value(exe: &str) -> String {
    format!("\"{exe}\" {}", crate::about::AUTOSTART_ARG)
}

/// File name of the installed executable.
pub const EXE_NAME: &str = "cursorpie.exe";

/// File name of the uninstaller that the Setup wizard (`installer/cursorpie.iss`, built with Inno
/// Setup) writes next to the program.
pub const SETUP_UNINSTALLER: &str = "unins000.exe";

/// Whether the program at `exe` was installed by the Setup wizard: its folder holds the wizard's
/// uninstaller. Such an install is managed by Windows' own uninstall entry, so the program must not
/// install itself a second time or add a second "Apps & features" entry.
pub fn installed_by_setup(exe: &std::path::Path) -> bool {
    exe.parent()
        .is_some_and(|dir| dir.join(SETUP_UNINSTALLER).is_file())
}

/// Command-line switch that removes the installed copy (what "Apps & features" runs).
pub const UNINSTALL_ARG: &str = "--uninstall";

/// Folder of the per-user install under `local_app_data` (`%LOCALAPPDATA%`). It needs no
/// administrator rights, and Windows keeps other users out of it.
pub fn install_dir(local_app_data: &str, app_name: &str) -> String {
    format!(
        r"{}\Programs\{app_name}",
        local_app_data.trim_end_matches(['\\', '/'])
    )
}

/// Full path of the installed executable.
pub fn installed_exe(local_app_data: &str, app_name: &str) -> String {
    format!("{}\\{EXE_NAME}", install_dir(local_app_data, app_name))
}

/// A Windows path in the form used to compare two of them: without the `\\?\` prefix that
/// `fs::canonicalize` adds, with backslashes only, without a trailing separator and in lower case
/// (Unicode lower case, not just ASCII: a user name such as `Müller` is written `MÜLLER` in some
/// places).
pub fn normalize_path(s: &str) -> String {
    let s = s.trim();
    let s = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    };
    s.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// Whether two Windows paths name the same file: the comparison ignores case, the kind of slash,
/// a trailing separator and the `\\?\` prefix, because the file system does too.
pub fn same_path(a: &str, b: &str) -> bool {
    !a.trim().is_empty() && normalize_path(a) == normalize_path(b)
}

/// The `UninstallString` of the "Apps & features" entry.
pub fn uninstall_command(exe: &str) -> String {
    format!("\"{exe}\" {UNINSTALL_ARG}")
}

/// Whether the command line asks for an uninstall. `args` is `std::env::args()` without the
/// program name.
pub fn wants_uninstall<I: IntoIterator<Item = String>>(args: I) -> bool {
    args.into_iter()
        .any(|a| a.eq_ignore_ascii_case(UNINSTALL_ARG))
}

/// The `cmd.exe` command that finishes an uninstall after the process has exited: it ends any
/// other copy, waits a moment for this one to leave, then deletes the install folder (a program
/// cannot delete its own file while it runs, so a second process must do it).
pub fn cleanup_command(dir: &str, own_pid: u32) -> String {
    format!(
        "taskkill /F /FI \"IMAGENAME eq {EXE_NAME}\" /FI \"PID ne {own_pid}\" >nul 2>&1 & \
         ping -n 4 127.0.0.1 >nul & rmdir /S /Q \"{dir}\""
    )
}

/// Whether `exe` was built by `cargo` (it sits in `target\debug` or `target\release`, also below
/// a target-triple folder). Such builds are never installed, so `cargo run` cannot replace the
/// app that starts at login.
pub fn is_build_output(exe: &str) -> bool {
    let p = exe.replace('/', "\\").to_ascii_lowercase();
    p.contains("\\target\\") && (p.contains("\\release\\") || p.contains("\\debug\\"))
}

/// Whether a `StartupApproved\Run` value says the user switched the entry off (in Task Manager or
/// in Settings > Apps > Startup). Windows stores 12 bytes there; an even first byte means
/// enabled, an odd one disabled. An entry without that value is enabled.
pub fn startup_entry_disabled(data: &[u8]) -> bool {
    data.first().is_some_and(|b| b & 1 == 1)
}

/// The 12 bytes Windows stores for an enabled startup entry (`02 00 00 00` and a zero time stamp).
pub const STARTUP_ENABLED: [u8; 12] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// Parameters for `cmd.exe` that run `cmd` exactly as typed. `/S` makes cmd strip only the outer
/// pair of quotes, so a command that itself starts with a quote still works.
pub fn shell_params(cmd: &str) -> String {
    format!("/S /C \"{cmd}\"")
}

/// Parameters for `explorer.exe` that select a file in a folder window.
pub fn select_params(path: &str) -> String {
    format!("/select,\"{path}\"")
}

/// The file-type filter of the open dialog: pairs of `description\0pattern\0`, ended by a double
/// zero, as `OPENFILENAMEW::lpstrFilter` wants it.
pub fn filter_for(kind: FileKind) -> Vec<u16> {
    let images = crate::platform::IMAGE_EXTENSIONS
        .iter()
        .map(|e| format!("*.{e}"))
        .collect::<Vec<_>>()
        .join(";");
    let (desc, pattern) = match kind {
        FileKind::Image => ("Images", images.as_str()),
        FileKind::Json => ("JSON files", "*.json"),
        FileKind::App => ("Programs", "*.exe"),
    };
    let mut v: Vec<u16> = Vec::new();
    for part in [desc, pattern] {
        v.extend(part.encode_utf16());
        v.push(0);
    }
    v.push(0);
    v
}

/// Converts BGRA pixels (what `GetDIBits` returns) to straight RGBA in place. When no pixel has any
/// alpha (an old icon without an alpha channel) every pixel becomes opaque, except those the
/// 1-bit `mask` (one byte per pixel, nonzero = transparent) says are background.
pub fn bgra_to_rgba(px: &mut [u8], mask: Option<&[u8]>) {
    let has_alpha = px.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    for (i, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        p.swap(0, 2);
        if !has_alpha {
            let transparent = mask.and_then(|m| m.get(i)).is_some_and(|m| *m != 0);
            p[3] = if transparent { 0 } else { 255 };
        }
    }
}

/// The executable inside the value of a `Run` entry: `"C:\\Apps\\x.exe" --autostart` and
/// `C:\\Apps\\x.exe --autostart` both give `C:\\Apps\\x.exe`. `None` when no path can be told.
pub fn exe_of_run_value(value: &str) -> Option<String> {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix('"') {
        let path = &rest[..rest.find('"')?];
        return (!path.is_empty()).then(|| path.to_string());
    }
    // ASCII lower-casing keeps every byte offset, so the index is valid for `v` too
    let end = v.to_ascii_lowercase().find(".exe")? + 4;
    Some(v[..end].to_string())
}

/// Text of a registry value read as raw bytes (`REG_SZ` / `REG_EXPAND_SZ`: UTF-16, little endian).
pub fn utf16_bytes_to_string(bytes: &[u8]) -> String {
    let (pairs, _) = bytes.as_chunks::<2>(); // a trailing odd byte is dropped, as before
    let units: Vec<u16> = pairs.iter().map(|&c| u16::from_le_bytes(c)).collect();
    from_wide(&units)
}

/// Replaces every `%NAME%` that `lookup` knows (what `REG_EXPAND_SZ` values such as App Paths need).
/// An unknown or malformed `%NAME%` stays as it is.
pub fn expand_env_vars(text: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) if j > 0 => {
                let name = &after[..j];
                match lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whether an executable name (without `.exe`, lower case) belongs to an uninstaller, updater or
/// installer: such programs are never listed as apps.
pub fn is_helper_stem(stem: &str) -> bool {
    ["uninstall", "unins000", "update", "setup", "install"]
        .iter()
        .any(|w| stem.contains(w))
}

/// `chrome` -> `Chrome`: the name shown for an app that only has a file name.
pub fn capitalized(stem: &str) -> String {
    let mut chars = stem.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Converts the pixels of a bitmap from `IShellItemImageFactory` (BGRA, 32 bit) to straight RGBA
/// in place. The shell hands out premultiplied alpha; when a pixel has a color channel above its
/// alpha the data cannot be premultiplied and is left alone. A bitmap with no alpha at all becomes
/// opaque.
pub fn shell_bgra_to_rgba(px: &mut [u8]) {
    let px = px.as_chunks_mut::<4>().0;
    let has_alpha = px.iter().any(|p| p[3] != 0);
    let premultiplied = has_alpha && px.iter().all(|p| p[..3].iter().all(|&c| c <= p[3]));
    for p in px.iter_mut() {
        p.swap(0, 2);
        if !has_alpha {
            p[3] = 255;
        } else if premultiplied && p[3] != 0 && p[3] != 255 {
            let a = p[3] as u32;
            for v in &mut p[..3] {
                *v = ((*v as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

// ---------------------------------------------------------------------------- administrator mode

/// Command-line switch of the elevated step of *Reinstall as administrator* (About tab). Only the
/// program starts itself with it, through the system's question for permission (UAC); the other
/// switches below belong to it. See [`parse_admin_install`].
pub const ADMIN_INSTALL_ARG: &str = "--install-admin";

/// The switch that carries the account the logon task is made for.
const FOR_USER_ARG: &str = "--for-user";

/// The switch that carries the process id of the copy that quits to make room.
const WAIT_PID_ARG: &str = "--wait-pid";

/// The switch that carries whether the program starts at login (`1`) or not (`0`).
const LOGIN_ARG: &str = "--login";

/// Folder of the administrator install, below the program folder (`C:\Program Files`). Only
/// administrators can write there, which is the point: the logon task starts the program with
/// administrator rights, so a folder that an ordinary program could write to would let it replace
/// what runs elevated.
pub fn admin_install_dir(program_files: &str, app_name: &str) -> String {
    format!(
        r"{}\{app_name}",
        program_files.trim_end_matches(['\\', '/'])
    )
}

/// Full path of the executable of the administrator install.
pub fn admin_installed_exe(program_files: &str, app_name: &str) -> String {
    format!("{}\\{EXE_NAME}", admin_install_dir(program_files, app_name))
}

/// What the elevated step of *Reinstall as administrator* is asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminInstallRequest {
    /// The account (`DOMAIN\name`) whose logon starts the program. `None`: the account that runs
    /// the elevated step.
    pub user: Option<String>,
    /// The process that quits to make room: the step waits for it before it replaces files.
    pub wait_pid: Option<u32>,
    /// Whether the program starts when the account signs in.
    pub launch_at_login: bool,
}

/// The parameters that start the elevated step: `exe --install-admin --for-user "DOMAIN\name"
/// --wait-pid 1234 --login 1`. A double quote in the account name (there cannot be one) is dropped.
pub fn admin_install_params(user: &str, wait_pid: u32, launch_at_login: bool) -> String {
    format!(
        "{ADMIN_INSTALL_ARG} {FOR_USER_ARG} \"{}\" {WAIT_PID_ARG} {wait_pid} {LOGIN_ARG} {}",
        user.replace('"', ""),
        u8::from(launch_at_login)
    )
}

/// Reads the elevated step's request from the command line. `args` is `std::env::args()` without
/// the program name. `None` when the switch is not there. A missing launch flag means yes.
pub fn parse_admin_install<I: IntoIterator<Item = String>>(args: I) -> Option<AdminInstallRequest> {
    let args: Vec<String> = args.into_iter().collect();
    if !args
        .iter()
        .any(|a| a.eq_ignore_ascii_case(ADMIN_INSTALL_ARG))
    {
        return None;
    }
    let value_of = |name: &str| {
        let at = args.iter().position(|a| a.eq_ignore_ascii_case(name))?;
        args.get(at + 1).map(|v| v.trim().to_string())
    };
    Some(AdminInstallRequest {
        user: value_of(FOR_USER_ARG).filter(|u| !u.is_empty() && !u.starts_with("--")),
        wait_pid: value_of(WAIT_PID_ARG).and_then(|p| p.parse().ok()),
        launch_at_login: value_of(LOGIN_ARG).is_none_or(|v| v != "0"),
    })
}

/// `DOMAIN\name` from the two environment variables that name the account (`USERDOMAIN`,
/// `USERNAME`); just the name when there is no domain. `None` without a name.
pub fn account_name(domain: Option<&str>, user: Option<&str>) -> Option<String> {
    let user = user?.trim();
    if user.is_empty() {
        return None;
    }
    match domain.map(str::trim).filter(|d| !d.is_empty()) {
        Some(domain) => Some(format!(r"{domain}\{user}")),
        None => Some(user.to_string()),
    }
}

/// Text for the inside of an XML element.
pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// The Task Scheduler definition of the logon task of administrator mode, for `schtasks /Create
/// /XML`: starts `exe --autostart` when `user` signs in, with the highest rights that account has
/// and without a password (`InteractiveToken`: it runs in the signed-in session).
///
/// `at_login` switches the logon trigger on or off while the task itself stays enabled, so
/// `schtasks /Run` (what starts the program when an ordinary copy is started) always works, and
/// *Launch at login* in Settings is this one flag. The settings that a task made with the command
/// line would have are not wanted: the default stops the task after 72 hours, does not start it
/// on battery and gives it the priority of a background job, which a mouse hook must not have.
pub fn task_xml(user: &str, exe: &str, dir: &str, at_login: bool) -> String {
    let (user, exe, dir) = (xml_escape(user), xml_escape(exe), xml_escape(dir));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts CursorPie with administrator rights when you sign in, so the pie also works in windows that run as administrator.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>{at_login}</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe}</Command>
      <Arguments>{autostart}</Arguments>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#,
        autostart = crate::about::AUTOSTART_ARG,
    )
}

/// `text` as the bytes of a UTF-16 (little endian) file with a byte order mark, the form `schtasks
/// /XML` reads without guessing.
pub fn utf16le_with_bom(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

/// The `cmd.exe` command that deletes `dir` once the program that runs from it has quit: it waits a
/// moment first (a program cannot delete its own file while it runs). Unlike [`cleanup_command`] it
/// leaves other copies of the program running.
pub fn remove_dir_command(dir: &str) -> String {
    format!("ping -n 4 127.0.0.1 >nul & rmdir /S /Q \"{dir}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_moved_to_another_monitor_is_centered_there() {
        // 800x600 window, second monitor with a work area at 1920..3840 x 0..1040, same DPI
        let r = centered_on_work((100, 100, 900, 700), 96, (1920, 0, 3840, 1040), 96);
        assert_eq!(r, (2480, 220, 3280, 820));
    }

    #[test]
    fn a_window_is_scaled_by_the_dpi_of_the_monitors_and_cut_to_fit() {
        // from 100% to 200%: twice the pixels
        let r = centered_on_work((0, 0, 400, 300), 96, (0, 0, 3000, 2000), 192);
        assert_eq!((r.2 - r.0, r.3 - r.1), (800, 600));
        // bigger than the work area: cut down to it
        let r = centered_on_work((0, 0, 2000, 1500), 96, (-1280, 0, 0, 984), 96);
        assert_eq!(r, (-1280, 0, 0, 984));
        // an unknown DPI means no scaling
        let r = centered_on_work((0, 0, 400, 300), 0, (0, 0, 1000, 1000), 144);
        assert_eq!((r.2 - r.0, r.3 - r.1), (400, 300));
    }

    #[test]
    fn open_app_text_becomes_the_app_id_of_the_running_program() {
        assert_eq!(app_id_for_open("chrome"), Some("chrome.exe".into()));
        assert_eq!(app_id_for_open("  Chrome.EXE "), Some("chrome.exe".into()));
        assert_eq!(
            app_id_for_open(r"C:\Program Files\Tools\App.exe"),
            Some("app.exe".into())
        );
        assert_eq!(
            app_id_for_open(r#""C:\Tools\My App.exe""#),
            Some("my app.exe".into())
        );
        assert_eq!(app_id_for_open("C:/x/Code"), Some("code.exe".into()));
        assert_eq!(app_id_for_open(r"C:\dir\"), None);
        assert_eq!(app_id_for_open("   "), None);
    }

    #[test]
    fn a_guid_is_one_number_in_the_order_it_is_written() {
        let id = 0xa5cd92ff_29be_454c_8d04_d82879fb3f1b_u128;
        assert_eq!(guid_to_u128(&windows_sys::core::GUID::from_u128(id)), id);
        assert_eq!(guid_to_u128(&windows_sys::core::GUID::default()), 0);
    }

    #[test]
    fn a_desktop_is_told_by_the_set_of_ids_of_its_windows() {
        let a = desktop_signature(vec![1, 2, 3]);
        assert!(a.is_some());
        assert_eq!(
            a,
            desktop_signature(vec![3, 1, 2, 2]),
            "order and repeats do not matter"
        );
        assert_ne!(
            a,
            desktop_signature(vec![1, 2]),
            "another set is another desktop"
        );
        // a pinned window reports the same id on every desktop, the rest tells them apart
        assert_ne!(desktop_signature(vec![9, 1]), desktop_signature(vec![9, 2]));
        assert_eq!(desktop_signature(vec![0, 1]), desktop_signature(vec![1]));
        assert_eq!(desktop_signature(vec![0, 0]), None);
        assert_eq!(desktop_signature(vec![]), None);
    }

    #[test]
    fn a_folder_with_the_setup_uninstaller_is_a_setup_install() {
        let dir = std::env::temp_dir().join(format!("cursorpie-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join(EXE_NAME);
        assert!(!installed_by_setup(&exe), "a loose exe is not");
        std::fs::write(dir.join(SETUP_UNINSTALLER), b"").unwrap();
        assert!(installed_by_setup(&exe));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_framed_window_loses_its_frame_styles() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            WS_BORDER, WS_CAPTION, WS_CLIPSIBLINGS, WS_EX_ACCEPTFILES, WS_EX_TOPMOST,
            WS_EX_WINDOWEDGE, WS_POPUP, WS_SYSMENU, WS_THICKFRAME, WS_VISIBLE,
        };
        // what winit creates for `with_decorations(false)`: caption and border are still there
        let style = WS_CAPTION | WS_BORDER | WS_CLIPSIBLINGS | WS_SYSMENU | WS_VISIBLE;
        let ex = WS_EX_WINDOWEDGE | WS_EX_ACCEPTFILES | WS_EX_TOPMOST;
        let (s, e) = frameless_styles(style, ex).expect("has a frame");
        assert_eq!(s & (WS_CAPTION | WS_BORDER | WS_THICKFRAME), 0);
        assert_ne!(s & WS_POPUP, 0);
        assert_eq!(e & WS_EX_WINDOWEDGE, 0);
        // everything else is kept: visibility, clipping, system menu, always on top, file drop
        assert_eq!(
            s & (WS_CLIPSIBLINGS | WS_SYSMENU | WS_VISIBLE),
            WS_CLIPSIBLINGS | WS_SYSMENU | WS_VISIBLE
        );
        assert_eq!(
            e & (WS_EX_ACCEPTFILES | WS_EX_TOPMOST),
            WS_EX_ACCEPTFILES | WS_EX_TOPMOST
        );
    }

    #[test]
    fn a_frameless_window_is_left_alone() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WS_POPUP, WS_VISIBLE};
        // idempotent: the second call (every frame) must report "nothing to do"
        let (s, e) =
            frameless_styles(WS_POPUP | WS_VISIBLE, 0).unwrap_or((WS_POPUP | WS_VISIBLE, 0));
        assert_eq!(frameless_styles(s, e), None);
    }

    #[test]
    fn wide_strings_round_trip() {
        let w = wide("Chrome \u{e9}");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(from_wide(&w), "Chrome \u{e9}");
        assert_eq!(
            from_wide(&[104, 105, 0, 120, 121]),
            "hi",
            "stops at the first zero"
        );
        assert_eq!(from_wide(&[104, 105]), "hi", "no terminator needed");
        assert_eq!(wide(""), vec![0]);
    }

    #[test]
    fn the_windows_of_a_store_app_are_told_by_their_class() {
        assert!(is_uwp_frame_class("ApplicationFrameWindow"));
        assert!(!is_uwp_frame_class("Chrome_WidgetWin_1"));
        assert!(is_core_window_class("Windows.UI.Core.CoreWindow"));
        assert!(!is_core_window_class("ApplicationFrameWindow"));
    }

    #[test]
    fn exe_ids_are_lower_case_file_names() {
        assert_eq!(
            exe_id_from_path(r"C:\Program Files\Google\Chrome\Application\Chrome.EXE"),
            Some("chrome.exe".into())
        );
        assert_eq!(exe_id_from_path("C:/x/Code.exe"), Some("code.exe".into()));
        assert_eq!(exe_id_from_path("notepad.exe"), Some("notepad.exe".into()));
        assert_eq!(exe_id_from_path(r"C:\dir\"), None);
        assert_eq!(exe_id_from_path(""), None);
    }

    #[test]
    fn only_exe_files_count_as_apps() {
        assert!(is_exe(r"C:\a\B.EXE"));
        assert!(is_exe("  x.exe "));
        assert!(!is_exe("x.lnk"));
        assert!(!is_exe("exe"));
    }

    #[test]
    fn dpi_math() {
        assert_eq!(scale_from_dpi(96), 1.0);
        assert_eq!(scale_from_dpi(144), 1.5);
        assert_eq!(scale_from_dpi(192), 2.0);
        assert_eq!(scale_from_dpi(0), 1.0, "a failed lookup means 100 %");
        assert_eq!(px_to_logical(300.0, 1.5), 200.0);
        assert_eq!(logical_to_px(200.0, 1.5), 300.0);
        assert_eq!(px_to_logical(10.0, 0.0), 10.0);
        let v = 1234.0;
        assert!((logical_to_px(px_to_logical(v, 1.25), 1.25) - v).abs() < 1e-9);
    }

    #[test]
    fn registry_and_command_lines() {
        assert_eq!(
            run_value(r"C:\Apps\cursorpie.exe"),
            r#""C:\Apps\cursorpie.exe" --autostart"#
        );
        assert_eq!(shell_params("echo hi"), r#"/S /C "echo hi""#);
        assert_eq!(
            shell_params(r#""C:\a b\x.exe" /q"#),
            r#"/S /C ""C:\a b\x.exe" /q""#
        );
        assert_eq!(select_params(r"C:\a\b.json"), r#"/select,"C:\a\b.json""#);
    }

    #[test]
    fn the_install_lives_under_local_app_data() {
        let local = r"C:\Users\Ana\AppData\Local";
        assert_eq!(
            install_dir(local, "CursorPie"),
            r"C:\Users\Ana\AppData\Local\Programs\CursorPie"
        );
        assert_eq!(
            installed_exe(r"C:\Users\Ana\AppData\Local\", "CursorPie"),
            r"C:\Users\Ana\AppData\Local\Programs\CursorPie\cursorpie.exe",
            "a trailing separator is not doubled"
        );
    }

    #[test]
    fn paths_are_compared_the_way_windows_does() {
        assert!(same_path(r"C:\A\B\x.exe", "c:/a/b/X.EXE"));
        assert!(same_path(r"C:\A\B\", r"c:\a\b"));
        assert!(!same_path(r"C:\A\x.exe", r"C:\B\x.exe"));
        assert!(!same_path("", ""), "an unknown path matches nothing");
    }

    #[test]
    fn the_uninstall_entry_and_switch_agree() {
        let cmd = uninstall_command(r"C:\Apps\cursorpie.exe");
        assert_eq!(cmd, r#""C:\Apps\cursorpie.exe" --uninstall"#);
        assert!(wants_uninstall(["--uninstall".to_string()]));
        assert!(wants_uninstall(["--UNINSTALL".to_string()]));
        assert!(!wants_uninstall(["--help".to_string()]));
        assert!(!wants_uninstall(Vec::<String>::new()));
    }

    #[test]
    fn the_cleanup_spares_this_process_and_removes_the_folder() {
        let c = cleanup_command(r"C:\L\Programs\CursorPie", 4242);
        assert!(c.contains(r#""PID ne 4242""#), "{c}");
        assert!(
            c.contains(r#"rmdir /S /Q "C:\L\Programs\CursorPie""#),
            "{c}"
        );
        let kill = c.find("taskkill").unwrap();
        let wait = c.find("ping").unwrap();
        let remove = c.find("rmdir").unwrap();
        assert!(kill < wait && wait < remove, "order matters: {c}");
    }

    #[test]
    fn cargo_builds_are_never_installed() {
        assert!(is_build_output(
            r"C:\src\cursorpie\target\release\cursorpie.exe"
        ));
        assert!(is_build_output(
            r"C:\src\cursorpie\target\debug\cursorpie.exe"
        ));
        assert!(is_build_output(
            r"D:\x\target\x86_64-pc-windows-msvc\release\cursorpie.exe"
        ));
        assert!(is_build_output(
            "C:/src/cursorpie/target/release/cursorpie.exe"
        ));
        assert!(!is_build_output(
            r"C:\Users\Ana\Downloads\CursorPie\cursorpie.exe"
        ));
        assert!(!is_build_output(
            r"C:\Users\Ana\AppData\Local\Programs\CursorPie\cursorpie.exe"
        ));
    }

    #[test]
    fn a_startup_entry_switched_off_by_the_user_is_recognized() {
        assert!(!startup_entry_disabled(&STARTUP_ENABLED));
        assert!(!startup_entry_disabled(&[6, 0, 0, 0]), "06 is enabled too");
        assert!(startup_entry_disabled(&[
            3, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8
        ]));
        assert!(!startup_entry_disabled(&[]), "no data means enabled");
    }

    #[test]
    fn the_file_filter_is_double_zero_terminated() {
        for kind in [FileKind::Image, FileKind::Json, FileKind::App] {
            let f = filter_for(kind);
            assert_eq!(&f[f.len() - 2..], &[0, 0]);
            assert_eq!(
                f.iter().filter(|&&c| c == 0).count(),
                3,
                "description, pattern, end"
            );
        }
        let json = String::from_utf16_lossy(&filter_for(FileKind::Json));
        assert!(json.contains("*.json"));
        assert!(String::from_utf16_lossy(&filter_for(FileKind::Image)).contains("*.webp"));
    }

    #[test]
    fn bgra_becomes_rgba() {
        let mut px = [10, 20, 30, 128, 1, 2, 3, 255];
        bgra_to_rgba(&mut px, None);
        assert_eq!(px, [30, 20, 10, 128, 3, 2, 1, 255]);
    }

    #[test]
    fn an_icon_without_alpha_becomes_opaque_except_where_masked() {
        let mut px = [1, 2, 3, 0, 4, 5, 6, 0];
        bgra_to_rgba(&mut px, Some(&[0, 1]));
        assert_eq!(px, [3, 2, 1, 255, 6, 5, 4, 0]);
        let mut all = [1, 2, 3, 0];
        bgra_to_rgba(&mut all, None);
        assert_eq!(all[3], 255);
    }
    #[test]
    fn paths_with_the_extended_prefix_or_other_case_are_the_same_file() {
        assert!(same_path(r"\\?\C:\Users\Ann\x.exe", r"c:\users\ann\X.EXE"));
        assert!(same_path(r"\\?\UNC\srv\share\x.exe", r"\\srv\share\x.exe"));
        assert!(same_path(
            r"C:\Users\Müller\x.exe",
            r"c:\users\MÜLLER\x.exe"
        ));
        assert!(!same_path(
            r"C:\Users\Müller\x.exe",
            r"C:\Users\Muller\x.exe"
        ));
    }

    #[test]
    fn the_program_of_a_run_entry_is_found_quoted_or_not() {
        assert_eq!(
            exe_of_run_value(r#""C:\Program Files\A B\x.exe" --autostart"#).as_deref(),
            Some(r"C:\Program Files\A B\x.exe")
        );
        assert_eq!(
            exe_of_run_value(r"C:\Apps\X.EXE --autostart").as_deref(),
            Some(r"C:\Apps\X.EXE")
        );
        assert_eq!(exe_of_run_value(""), None);
        assert_eq!(exe_of_run_value(r#""" --autostart"#), None);
        assert_eq!(exe_of_run_value("notepad"), None);
    }

    #[test]
    fn registry_text_is_read_from_utf16_bytes() {
        let bytes: Vec<u8> = "C:\\a\u{e9}.exe\0junk"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(utf16_bytes_to_string(&bytes), "C:\\a\u{e9}.exe");
        assert_eq!(utf16_bytes_to_string(&[]), "");
        assert_eq!(
            utf16_bytes_to_string(&[65]),
            "",
            "a lone byte is not a character"
        );
    }

    #[test]
    fn environment_variables_are_expanded_only_when_known() {
        let lookup = |n: &str| (n == "ProgramFiles").then(|| r"C:\Program Files".to_string());
        assert_eq!(
            expand_env_vars(r"%ProgramFiles%\A\a.exe", &lookup),
            r"C:\Program Files\A\a.exe"
        );
        assert_eq!(expand_env_vars("%Nope%\\a.exe", &lookup), "%Nope%\\a.exe");
        assert_eq!(expand_env_vars("100%", &lookup), "100%");
        assert_eq!(expand_env_vars("%%", &lookup), "%%");
        assert_eq!(expand_env_vars("plain", &lookup), "plain");
    }

    #[test]
    fn helpers_are_not_apps_and_names_get_a_capital() {
        assert!(is_helper_stem("unins000"));
        assert!(is_helper_stem("my-updater-setup"));
        assert!(!is_helper_stem("chrome"));
        assert_eq!(capitalized("chrome"), "Chrome");
        assert_eq!(capitalized(""), "");
    }

    #[test]
    fn shell_bitmaps_become_straight_rgba() {
        // premultiplied: half transparent white is (128, 128, 128, 128)
        let mut px = [128, 128, 128, 128, 10, 20, 30, 255, 0, 0, 0, 0];
        shell_bgra_to_rgba(&mut px);
        assert_eq!(px[..4], [255, 255, 255, 128]);
        assert_eq!(px[4..8], [30, 20, 10, 255]);
        assert_eq!(px[8..], [0, 0, 0, 0]);
        // a channel above alpha: already straight, only the channel order changes
        let mut px = [200, 100, 50, 128];
        shell_bgra_to_rgba(&mut px);
        assert_eq!(px, [50, 100, 200, 128]);
        // no alpha at all: opaque
        let mut px = [1, 2, 3, 0, 4, 5, 6, 0];
        shell_bgra_to_rgba(&mut px);
        assert_eq!(px, [3, 2, 1, 255, 6, 5, 4, 255]);
    }

    #[test]
    fn the_administrator_install_lives_in_the_program_folder() {
        assert_eq!(
            admin_install_dir(r"C:\Program Files\", "CursorPie"),
            r"C:\Program Files\CursorPie"
        );
        assert_eq!(
            admin_installed_exe(r"C:\Program Files", "CursorPie"),
            r"C:\Program Files\CursorPie\cursorpie.exe"
        );
    }

    #[test]
    fn the_request_of_the_elevated_step_is_read_back() {
        let params = admin_install_params(r"PC\Ann Lee", 4242, false);
        assert_eq!(
            params,
            r#"--install-admin --for-user "PC\Ann Lee" --wait-pid 4242 --login 0"#
        );
        // what the command line parser of the elevated copy hands to `parse_admin_install`
        let args = [
            "--install-admin",
            "--for-user",
            r"PC\Ann Lee",
            "--wait-pid",
            "4242",
            "--login",
            "0",
        ]
        .map(String::from);
        let req = parse_admin_install(args).expect("a request");
        assert_eq!(req.user.as_deref(), Some(r"PC\Ann Lee"));
        assert_eq!(req.wait_pid, Some(4242));
        assert!(!req.launch_at_login);
    }

    #[test]
    fn a_request_without_details_asks_for_the_defaults() {
        let req = parse_admin_install(["--INSTALL-ADMIN".to_string()]).expect("a request");
        assert_eq!(
            req,
            AdminInstallRequest {
                user: None,
                wait_pid: None,
                launch_at_login: true
            }
        );
        // a switch that is not followed by a value, or by a number, gives nothing
        let args = ["--install-admin", "--for-user", "--wait-pid", "soon"].map(String::from);
        let req = parse_admin_install(args).expect("a request");
        assert_eq!((req.user, req.wait_pid), (None, None));
        // without the switch it is no request, whatever else is there
        assert_eq!(parse_admin_install(["--for-user".to_string()]), None);
        assert_eq!(parse_admin_install(Vec::<String>::new()), None);
    }

    #[test]
    fn an_account_is_named_with_its_domain_when_it_has_one() {
        assert_eq!(
            account_name(Some("PC"), Some("Ann")),
            Some(r"PC\Ann".into())
        );
        assert_eq!(account_name(Some(" "), Some("Ann")), Some("Ann".into()));
        assert_eq!(account_name(None, Some(" Ann ")), Some("Ann".into()));
        assert_eq!(account_name(Some("PC"), Some("")), None);
        assert_eq!(account_name(Some("PC"), None), None);
    }

    #[test]
    fn xml_text_is_escaped() {
        assert_eq!(
            xml_escape(r#"A&B <c> "d" 'e'"#),
            "A&amp;B &lt;c&gt; &quot;d&quot; &apos;e&apos;"
        );
        assert_eq!(xml_escape("plain"), "plain");
    }

    #[test]
    fn the_logon_task_starts_the_program_elevated_for_one_account() {
        let xml = task_xml(
            r"PC\Ann & Co",
            r"C:\Program Files\CursorPie\cursorpie.exe",
            r"C:\Program Files\CursorPie",
            true,
        );
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-16\"?>"));
        assert!(xml.trim_end().ends_with("</Task>"));
        // the highest rights, in the signed-in session, without a stored password
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        // for that account only, written so that XML reads it
        assert_eq!(xml.matches(r"<UserId>PC\Ann &amp; Co</UserId>").count(), 2);
        assert!(!xml.contains("Ann & Co"));
        // what it runs, and how
        assert!(xml.contains(r"<Command>C:\Program Files\CursorPie\cursorpie.exe</Command>"));
        assert!(xml.contains("<Arguments>--autostart</Arguments>"));
        // never stopped, never held back on battery, not a background priority
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<Priority>5</Priority>"));
        // a second start while it runs is ignored, so a double click cannot make two copies
        assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
    }

    #[test]
    fn launch_at_login_is_the_flag_of_the_logon_trigger_alone() {
        let on = task_xml("u", "e.exe", "d", true);
        let off = task_xml("u", "e.exe", "d", false);
        assert!(on.contains("<LogonTrigger>\n      <Enabled>true</Enabled>"));
        assert!(off.contains("<LogonTrigger>\n      <Enabled>false</Enabled>"));
        // the task itself stays enabled, so `schtasks /Run` still starts it
        let task = "<AllowStartOnDemand>true</AllowStartOnDemand>\n    <Enabled>true</Enabled>";
        assert!(off.contains(task));
    }

    #[test]
    fn the_task_file_is_utf16_with_a_byte_order_mark() {
        assert_eq!(utf16le_with_bom("A"), [0xFF, 0xFE, 0x41, 0x00]);
        assert_eq!(utf16le_with_bom("\u{e9}"), [0xFF, 0xFE, 0xE9, 0x00]);
        assert_eq!(utf16le_with_bom(""), [0xFF, 0xFE]);
    }

    #[test]
    fn removing_a_folder_leaves_the_other_copies_alone() {
        let cmd = remove_dir_command(r"C:\Program Files\CursorPie");
        assert!(cmd.ends_with(r#"rmdir /S /Q "C:\Program Files\CursorPie""#));
        assert!(!cmd.contains("taskkill"));
    }
}
