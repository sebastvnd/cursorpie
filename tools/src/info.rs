//! `cargo tools info`: prints what a bug report needs to know about this machine and checkout, as
//! text to paste into an issue: the OS, the desktop session, the tool versions, the project
//! version and where the debug logs are.
//!
//! It only reads: nothing is written or changed.

use std::path::Path;
use std::process::{Command, ExitCode};

use crate::diag::{default_log_dir, is_log_name};

/// Session variables that decide how the app behaves on Linux (see `docs/DEBUGGING.md`).
const SESSION_VARS: [&str; 6] = [
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "WINIT_UNIX_BACKEND",
];

/// Everything `info` prints.
#[derive(Debug, PartialEq, Eq)]
pub struct Info {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub tools: Vec<(String, String)>,
    pub session: Vec<(String, String)>,
    pub log_dir: Option<String>,
    pub logs: usize,
}

/// The first line a program prints for its version flag, or `not found`.
fn tool_version(program: &str, flag: &str) -> String {
    Command::new(program)
        .arg(flag)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let text = String::from_utf8_lossy(&o.stdout).into_owned();
            text.lines().next().map(|l| l.trim().to_string())
        })
        .unwrap_or_else(|| "not found".to_string())
}

/// The text of the report, one `key: value` per line, in a block that pastes well into an issue.
pub fn render(info: &Info) -> String {
    let mut out = String::from("```text\n");
    out.push_str(&format!("CursorPie: {}\n", info.app_version));
    out.push_str(&format!("OS: {} ({})\n", info.os, info.arch));
    for (name, version) in &info.tools {
        out.push_str(&format!("{name}: {version}\n"));
    }
    for (name, value) in &info.session {
        let shown = if value.is_empty() {
            "-"
        } else {
            value.as_str()
        };
        out.push_str(&format!("{name}: {shown}\n"));
    }
    match &info.log_dir {
        Some(dir) => out.push_str(&format!("debug logs: {} in {dir}\n", info.logs)),
        None => out.push_str("debug logs: folder unknown\n"),
    }
    out.push_str("```\n");
    out
}

/// Collects the facts about this machine and checkout at `root`.
pub fn gather(root: &Path) -> Info {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let (_, version) = crate::zip::package_id(&manifest);
    let log_dir = default_log_dir(&|k| std::env::var(k).ok());
    let logs = log_dir
        .as_ref()
        .and_then(|d| std::fs::read_dir(d).ok())
        .map_or(0, |entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_str().is_some_and(is_log_name))
                .count()
        });
    Info {
        app_version: version.unwrap_or_else(|| "unknown".to_string()),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        tools: vec![
            ("rustc".to_string(), tool_version("rustc", "--version")),
            ("cargo".to_string(), tool_version("cargo", "--version")),
            ("git".to_string(), tool_version("git", "--version")),
        ],
        session: SESSION_VARS
            .iter()
            .map(|name| ((*name).to_string(), std::env::var(name).unwrap_or_default()))
            .collect(),
        log_dir: log_dir.map(|d| d.display().to_string()),
        logs,
    }
}

/// Prints the report.
pub fn run(root: &Path) -> ExitCode {
    print!("{}", render(&gather(root)));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_lists_every_fact_and_marks_empty_values() {
        let info = Info {
            app_version: "0.5.2".into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            tools: vec![("rustc".into(), "rustc 1.88.0".into())],
            session: vec![
                ("XDG_SESSION_TYPE".into(), "x11".into()),
                ("WAYLAND_DISPLAY".into(), String::new()),
            ],
            log_dir: Some("/home/u/.config/cursorpie/logs".into()),
            logs: 2,
        };
        assert_eq!(
            render(&info),
            "```text\nCursorPie: 0.5.2\nOS: linux (x86_64)\nrustc: rustc 1.88.0\n\
             XDG_SESSION_TYPE: x11\nWAYLAND_DISPLAY: -\n\
             debug logs: 2 in /home/u/.config/cursorpie/logs\n```\n"
        );
    }

    #[test]
    fn an_unknown_log_folder_is_said() {
        let info = Info {
            app_version: "?".into(),
            os: "x".into(),
            arch: "y".into(),
            tools: vec![],
            session: vec![],
            log_dir: None,
            logs: 0,
        };
        assert!(render(&info).contains("debug logs: folder unknown"));
    }
}
