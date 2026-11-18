//! Developer mode: the tools for finding a problem on a machine, kept out of the way of everyone else.
//!
//! The diagnostic log (`docs/DEBUGGING.md`) is not in the ordinary download: a release build has
//! none of its code. A person who is asked to "send a log" turns on *Developer mode* at the bottom
//! of the About tab, and CursorPie offers to replace this copy with the **log build** of the same
//! release (the same program built with `--features debug-log`, attached to every release as
//! `CursorPie-devlog-*`). With that build running, the same place shows what the log found, in
//! plain words, and a way to go back to the ordinary build.
//!
//! This file holds what does not draw anything:
//!
//! | What | Where it lives |
//! |---|---|
//! | developer mode on or off | an empty file `developer` in the support folder, next to `config.json` (device state, like `mice.json`: Settings' Save, Undo and backups never touch it) |
//! | the log on or off, and which categories | the `debug` file of the log (`diag::MARKER_FILE`), read when CursorPie starts |
//! | whether this copy can write a log | [`LOG_BUILD`] |
//! | what the newest log says | [`snapshot`], made by `diag::problems`, the same reader the terminal command `cpd` uses |
//!
//! The panel is `ui/settings/developer.rs`; the swap of the build is the update's
//! ([`update::install::prepare_with`](crate::update::install::prepare_with)).

use crate::config::app_support_dir;
use std::io;
use std::path::PathBuf;

/// Whether this copy of CursorPie can write the diagnostic log: a debug build, or a release built
/// with `--features debug-log`. A constant, so the log code of an ordinary release is not kept.
pub const LOG_BUILD: bool = cfg!(any(debug_assertions, feature = "debug-log"));

/// Name of the file that means "developer mode is on".
pub const MODE_FILE: &str = "developer";

/// The switch that `diag` reads at start when the person asked for the log from the panel.
pub const DEFAULT_SPEC: &str = "all";

/// How much of the end of a log the panel reads (a log stops growing at 8 MiB).
#[cfg(any(debug_assertions, feature = "debug-log"))]
const READ_BYTES: u64 = 2 * 1024 * 1024;
/// How many of the last lines the panel lists under *Recent events*.
pub const RECENT: usize = 60;

fn mode_path() -> PathBuf {
    app_support_dir().join(MODE_FILE)
}

fn spec_path() -> PathBuf {
    app_support_dir().join(diag::MARKER_FILE)
}

/// Whether developer mode is on.
pub fn is_on() -> bool {
    mode_path().is_file()
}

/// Turns developer mode on or off. Turning it off does not touch the log switch: a log that was
/// asked for keeps being written until it is turned off on purpose.
pub fn set_on(on: bool) -> io::Result<()> {
    let path = mode_path();
    if on {
        std::fs::write(path, b"")
    } else {
        match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// What the `debug` file holds, or `None` when there is none (the log is off, unless the
/// environment switches it on): the categories the person asked for.
pub fn log_spec() -> Option<String> {
    let text = std::fs::read_to_string(spec_path()).ok()?;
    let text = text.trim();
    Some(if text.is_empty() { "1" } else { text }.to_string())
}

/// Asks for the log from the next start (`Some`, with the switch to write: `all`, `tap,ui`, `all,trace`,
/// see `docs/DEBUGGING.md`) or turns it off (`None`).
pub fn set_log_spec(spec: Option<&str>) -> io::Result<()> {
    let path = spec_path();
    match spec.map(str::trim).filter(|s| !s.is_empty()) {
        Some(spec) => std::fs::write(path, spec),
        None => match std::fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        },
    }
}

/// The folder the logs are written to (`CURSORPIE_DEBUG_DIR` moves it).
pub fn logs_dir() -> PathBuf {
    let moved = std::env::var("CURSORPIE_DEBUG_DIR").ok();
    diag::log_dir_from(moved.as_deref(), Some(&app_support_dir()), "cursorpie")
}

// ---------------------------------------------------------------------------------- reading

/// One problem the log found, as the panel draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProblemRow {
    /// Error (`true`) or warning.
    pub error: bool,
    pub title: String,
    pub detail: String,
    pub count: u32,
    /// When it first happened, seconds after start.
    pub first_t: f64,
}

/// One of the last lines of the log.
#[derive(Clone, Debug, PartialEq)]
pub struct RecentRow {
    pub t: f64,
    pub cat: String,
    pub text: String,
}

/// What the newest log says, in plain data (nothing of `diag` leaks into the drawing code, and the
/// type exists in every build).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// The file name.
    pub file: String,
    /// Whether this run writes it (`false`: the file of an earlier run).
    pub live: bool,
    pub lines: usize,
    pub seconds: f64,
    pub problems: Vec<ProblemRow>,
    pub recent: Vec<RecentRow>,
    /// The categories of the log, most lines first.
    pub categories: Vec<(String, usize)>,
    /// The report as text, to copy into a bug report.
    pub text: String,
}

/// Reads the log of this run, or the newest one. `Ok(None)`: there is no log (yet). An ordinary
/// release build has no reader at all and always answers `Ok(None)`.
#[cfg(any(debug_assertions, feature = "debug-log"))]
pub fn snapshot() -> io::Result<Option<Snapshot>> {
    use diag::problems::{self, Severity};
    let live = diag::active_path();
    let path = match &live {
        Some(p) => p.clone(),
        None => match problems::newest_log(&logs_dir(), "cursorpie") {
            Some(p) => p,
            None => return Ok(None),
        },
    };
    let lines = problems::read_lines(&path, READ_BYTES)?;
    let report = problems::scan(&lines);
    let recent = problems::tail(&lines, RECENT, None)
        .into_iter()
        .map(|e| RecentRow {
            t: e.t,
            cat: e.cat.clone(),
            text: format!("{} {}", e.ev, e.detail()).trim().to_string(),
        })
        .collect();
    let mut text = format!(
        "CursorPie {} ({} build)\nlog: {}\n\n",
        crate::about::VERSION,
        if LOG_BUILD { "log" } else { "standard" },
        path.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
    );
    text.push_str(&problems::render(&report));
    Ok(Some(Snapshot {
        file: path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
        live: live.is_some(),
        lines: report.lines,
        seconds: report.seconds,
        problems: report
            .problems
            .iter()
            .map(|p| ProblemRow {
                error: p.severity == Severity::Error,
                title: p.title.clone(),
                detail: p.detail.clone(),
                count: p.count,
                first_t: p.first_t,
            })
            .collect(),
        recent,
        categories: problems::categories(&lines),
        text,
    }))
}

/// See the build with the log.
#[cfg(not(any(debug_assertions, feature = "debug-log")))]
pub fn snapshot() -> io::Result<Option<Snapshot>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_build_is_known_at_compile_time() {
        // tests run as a debug build, which has the log
        const { assert!(LOG_BUILD) };
    }

    #[test]
    fn the_switch_in_the_marker_file_is_what_diag_reads() {
        // an empty file counts as `1` for diag; the panel shows the same
        assert!(diag::resolve_spec(None, Some("")).is_some());
        assert!(diag::resolve_spec(None, Some(DEFAULT_SPEC)).is_some());
    }
}
