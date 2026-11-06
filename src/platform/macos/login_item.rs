//! "Launch at login" through a per-user LaunchAgent.

use crate::about::BUNDLE_ID;
use crate::config::home;
use std::fs;
use std::path::{Path, PathBuf};

/// Where the LaunchAgent plist lives: `~/Library/LaunchAgents/<bundle id>.plist`.
fn agent_path() -> PathBuf {
    home()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{BUNDLE_ID}.plist"))
}

/// Whether the LaunchAgent is installed.
pub fn launch_at_login_enabled() -> bool {
    agent_path().exists()
}

/// Escapes text for use inside the plist XML.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The `.app` bundle that contains `exe` (`.../CursorPie.app/Contents/MacOS/CursorPie`), if any.
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf)
}

/// Whether `path` is one of the random, temporary copies Gatekeeper runs a downloaded app from
/// ("App Translocation", used until the app is moved out of Downloads or the disk image). A login
/// entry that points there breaks at the next login, because that path no longer exists.
fn is_translocated(path: &Path) -> bool {
    let p = path.to_string_lossy();
    p.contains("/AppTranslocation/") || p.starts_with("/Volumes/")
}

/// Installs / removes a LaunchAgent. It takes effect at the next login (we don't start a second copy now).
///
/// Inside a `.app` the agent runs `/usr/bin/open -g -a <bundle>` instead of the executable itself:
/// LaunchServices then starts the app the way a double click does (Gatekeeper, the signature and
/// the Accessibility permission are all judged for the app, not for a bare binary started by
/// launchd), and the agent stays valid after the app is updated in place.
pub fn set_launch_at_login(on: bool) -> std::io::Result<()> {
    let path = agent_path();
    if !on {
        return match fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    }
    let exe = std::env::current_exe()?;
    let args: Vec<String> = match bundle_of(&exe) {
        Some(app) if is_translocated(&app) => {
            return Err(std::io::Error::other(
                "move CursorPie.app to the Applications folder first, then switch Launch at login on",
            ));
        }
        Some(app) => vec![
            "/usr/bin/open".into(),
            "-g".into(),
            "-a".into(),
            app.to_string_lossy().into_owned(),
        ],
        None => vec![exe.to_string_lossy().into_owned()],
    };
    let arguments: String = args
        .iter()
        .map(|a| format!("<string>{}</string>", xml_escape(a)))
        .collect();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{label}</string>
    <key>ProgramArguments</key>
    <array>{arguments}</array>
    <key>RunAtLoad</key><true/>
    <key>KeepAlive</key><false/>
    <key>LimitLoadToSessionType</key><string>Aqua</string>
    <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
        label = BUNDLE_ID,
    );
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, plist)
}

/// Nothing to install: the `.app` bundle is the installation.
pub fn ensure_installed() -> Option<PathBuf> {
    None
}

/// Nothing to wait for: the login entry already starts the app when the session is ready.
pub fn wait_for_login_ready() {}

/// No command-line switches.
pub fn handle_command_line() -> bool {
    false
}

/// Always the only one: macOS already refuses a second launch of the same `.app`.
pub fn single_instance() -> bool {
    true
}
