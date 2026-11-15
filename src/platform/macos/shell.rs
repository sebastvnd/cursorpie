//! Helpers that shell out to tools shipped with macOS (`open`, `osascript`, `mdls`, `afplay`).

use crate::platform::{FileKind, chooser, spawn_reaped};
use std::process::Command;

/// Uniform Type Identifier that `osascript` filters on.
fn uti(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Image => "public.image",
        FileKind::Json => "public.json",
        FileKind::App => "com.apple.application-bundle",
    }
}

/// Native file chooser through osascript (no extra dependency).
pub fn choose_file(kind: FileKind, prompt: &str) -> Option<String> {
    let uti = uti(kind);
    let script =
        format!(r#"POSIX path of (choose file of type {{"{uti}"}} with prompt "{prompt}")"#);
    chooser::run(Command::new("osascript").arg("-e").arg(script))
}

/// CFBundleIdentifier of an .app bundle, via Spotlight metadata.
pub fn bundle_id_of(app_path: &str) -> Option<String> {
    let out = Command::new("mdls")
        .args(["-name", "kMDItemCFBundleIdentifier", "-raw", app_path])
        .output()
        .ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!id.is_empty() && id != "(null)").then_some(id)
}

/// Reveals a file in Finder.
pub fn reveal_in_file_manager(path: &std::path::Path) {
    spawn_reaped(Command::new("open").arg("-R").arg(path));
}

/// Opens a folder or file with its default app (`open <path>`).
pub fn open_path(path: &std::path::Path) {
    spawn_reaped(Command::new("open").arg(path));
}

/// Launches an app by name or path (`open -a`).
pub fn open_app(app: &str) {
    spawn_reaped(Command::new("open").arg("-a").arg(app));
}

/// Opens a URL or a file path with the default handler (`open`).
pub fn open_url(target: &str) {
    spawn_reaped(Command::new("open").arg(target));
}

/// Runs a shell command (`/bin/sh -c`).
pub fn run_shell(cmd: &str) {
    spawn_reaped(Command::new("/bin/sh").arg("-c").arg(cmd));
}

/// Soft tick used as hover feedback (afplay ships with macOS).
pub fn tick() {
    spawn_reaped(Command::new("afplay").args(["-v", "0.25", "/System/Library/Sounds/Tink.aiff"]));
}

/// Opens a web link in a new browser window: the link is opened as usual (the browser decides).
pub fn open_url_new_window(target: &str) {
    open_url(target);
}

/// Going to the browser that has a page is done by the extension alone on this OS.
pub fn raise_browser(_browser: &str) {}
