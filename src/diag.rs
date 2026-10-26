//! The diagnostic log: a switch-on-demand trace of what CursorPie does, written for developers.
//!
//! It answers questions like "where did the pie think the pointer was, and where did the window
//! manager really put the window?" without a debugger and without touching the code again. It is
//! **off by default** and costs one atomic read per call site while off.
//!
//! ## Turning it on
//!
//! | Way | Example |
//! |---|---|
//! | environment variable | `CURSORPIE_DEBUG=1 cursorpie` |
//! | marker file, for a copy that starts at login | an empty file named `debug` next to `config.json` |
//!
//! The value is a list: `1` (or `all`) switches on every category, `tap,ui` only those categories,
//! `trace` adds the high-volume events (every pointer move of a drag), `0` or `off` switches it
//! off. A set variable wins over the marker file. The marker file may hold the same list.
//!
//! ## Where it goes
//!
//! One file per run, `cursorpie-<unix seconds>-<pid>.jsonl`, in the `logs` folder inside the
//! config folder (`~/.config/cursorpie/logs` on Linux), or in `CURSORPIE_DEBUG_DIR`. Only the
//! newest [`KEEP_FILES`] files are kept and a file stops growing at [`MAX_FILE_BYTES`]. Every line
//! is one flat JSON object, so `jq`, a spreadsheet and `cargo tools diag` all read it:
//!
//! ```text
//! {"cat":"ui","ev":"show_pie","t":12.345,"th":"main","x":812.0,"y":344.0,...}
//! ```
//!
//! `t` is seconds since the program started, `th` the thread, `cat` the category and `ev` the
//! event. The other keys belong to the event. Each line is written to the file at once (no
//! buffering), so the log is complete after a crash; a panic is logged too.
//!
//! ## Adding a log line
//!
//! ```ignore
//! crate::diag_log!("ui", "show_pie", x = x, y = y, profile = profile);
//! crate::diag_trace!("tap", "drag", x = pos.x, y = pos.y); // only with `trace`
//! ```
//!
//! The values are evaluated **only when the log is on**, so an argument may call something
//! expensive (a round trip to the X server, a `format!`). Use plain numbers, strings and booleans:
//! the log stays flat on purpose, the analyzer in `tools/src/diag.rs` does not read nesting. Name
//! the category after the layer (`tap`, `ui`, `x11`, `win`, `sys`) and the event after what
//! happened, in `snake_case`. Never log the user's data (file names, typed text, config content).

use crate::config;
use serde_json::Map;
pub use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The environment variable that switches the log on (see the module docs).
pub const ENV_SWITCH: &str = "CURSORPIE_DEBUG";
/// The environment variable that moves the log folder.
pub const ENV_DIR: &str = "CURSORPIE_DEBUG_DIR";
/// The marker file in the config folder that switches the log on.
pub const SWITCH_FILE: &str = "debug";
/// The log folder inside the config folder.
pub const LOG_DIR: &str = "logs";
/// How many log files are kept (the newest ones).
pub const KEEP_FILES: usize = 10;
/// The size at which a log file stops growing.
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

const FILE_PREFIX: &str = "cursorpie-";
const FILE_SUFFIX: &str = ".jsonl";
/// Keys every line has; a field of the same name is renamed with a leading underscore.
const RESERVED: [&str; 4] = ["t", "th", "cat", "ev"];
/// Environment variables copied into the first line: the ones that decide how a session behaves.
const ENV_REPORTED: [&str; 10] = [
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "WINIT_UNIX_BACKEND",
    "WINIT_X11_SCALE_FACTOR",
    "GDK_SCALE",
    "CURSORPIE_BACKEND",
    "CURSORPIE_NO_INSTALL",
];
/// How long, and how many lines, the window is watched after the pie asked for a position.
const OBSERVE_FOR_MS: u128 = 2000;
const MAX_OBSERVATIONS: u32 = 12;

// ----------------------------------------------------------------------------------- the switch

/// Which events are written: everything, or some categories, with or without `trace`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spec {
    all: bool,
    trace: bool,
    cats: Vec<String>,
}

impl Spec {
    /// Reads a list such as `1`, `tap,ui` or `all,trace`. `None` means off: nothing at all, an
    /// empty text, or `0` / `off` / `false` / `no` anywhere in the list.
    pub fn parse(text: &str) -> Option<Spec> {
        let tokens: Vec<String> = text
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|t| t.trim().to_ascii_lowercase())
            .filter(|t| !t.is_empty())
            .collect();
        if tokens.is_empty()
            || tokens
                .iter()
                .any(|t| matches!(t.as_str(), "0" | "off" | "false" | "no"))
        {
            return None;
        }
        let mut spec = Spec::default();
        for t in tokens {
            match t.as_str() {
                "1" | "on" | "true" | "yes" | "all" => spec.all = true,
                "trace" => spec.trace = true,
                _ => spec.cats.push(t),
            }
        }
        if spec.cats.is_empty() {
            spec.all = true; // `trace` alone means everything, with the trace events
        }
        Some(spec)
    }

    /// Whether an event of category `cat` is written. `trace` marks a high-volume event.
    pub fn allows(&self, cat: &str, trace: bool) -> bool {
        (!trace || self.trace) && (self.all || self.cats.iter().any(|c| c.as_str() == cat))
    }
}

/// The switch from its two sources. A set environment variable wins (so `CURSORPIE_DEBUG=0` turns
/// a marker file off for one run); the marker file counts as `1` when it is empty.
pub fn resolve_spec(env: Option<&str>, marker_file: Option<&str>) -> Option<Spec> {
    match env {
        Some(v) => Spec::parse(v),
        None => marker_file.and_then(|f| Spec::parse(if f.trim().is_empty() { "1" } else { f })),
    }
}

/// The log folder: `dir_override` when it is not empty, else `<support>/logs`.
pub fn log_dir_from(dir_override: Option<&str>, support: &Path) -> PathBuf {
    match dir_override.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => support.join(LOG_DIR),
    }
}

// ------------------------------------------------------------------------- pure text helpers

/// The name of the log file of a run. The zero-padded seconds make name order time order.
pub fn file_name(unix_secs: u64, pid: u32) -> String {
    format!("{FILE_PREFIX}{unix_secs:010}-{pid}{FILE_SUFFIX}")
}

/// Whether a file name is one of ours (so nothing else in the folder is ever deleted).
pub fn is_log_name(name: &str) -> bool {
    name.starts_with(FILE_PREFIX) && name.ends_with(FILE_SUFFIX)
}

/// The log files to delete so that at most `keep` remain: the oldest of `names`.
pub fn stale(names: &[String], keep: usize) -> Vec<String> {
    let mut logs: Vec<&String> = names.iter().filter(|n| is_log_name(n)).collect();
    logs.sort();
    let surplus = logs.len().saturating_sub(keep);
    logs.into_iter().take(surplus).cloned().collect()
}

/// Whether a line of `add` bytes still fits in a file that holds `written` bytes.
fn has_room(written: u64, add: u64) -> bool {
    written.saturating_add(add) <= MAX_FILE_BYTES
}

/// A UTC time as `2026-10-26T07:33:20Z` (no time zone database needed).
pub fn iso_utc(unix_secs: u64) -> String {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(0);
    let rem = unix_secs % 86_400;
    // civil date from a day count (H. Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let (h, m, s) = (rem / 3_600, rem % 3_600 / 60, rem % 60);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

/// A rectangle as the log writes it: `x,y WxH` (the analyzer reads the same form).
pub fn rect_str(x: f64, y: f64, w: f64, h: f64) -> String {
    format!("{x},{y} {w}x{h}")
}

/// One log line: the fixed keys plus `fields`, as a single line of JSON without the newline.
pub fn format_line(t: f64, thread: &str, cat: &str, ev: &str, fields: &[(&str, Value)]) -> String {
    let mut m = Map::new();
    m.insert("t".to_string(), Value::from((t * 1000.0).round() / 1000.0));
    m.insert("th".to_string(), Value::from(thread));
    m.insert("cat".to_string(), Value::from(cat));
    m.insert("ev".to_string(), Value::from(ev));
    for (k, v) in fields {
        let key = if RESERVED.contains(k) {
            format!("_{k}")
        } else {
            (*k).to_string()
        };
        m.insert(key, v.clone());
    }
    Value::Object(m).to_string()
}

// ------------------------------------------------------------------------------- the global

/// The open log file and how much was written to it.
struct Sink {
    file: File,
    path: PathBuf,
    written: u64,
    capped: bool,
}

impl Sink {
    /// Appends one line. A line is one `write` call, so it is on disk when this returns.
    fn push(&mut self, line: &str) {
        if self.capped {
            return;
        }
        let mut text = String::with_capacity(line.len() + 1);
        text.push_str(line);
        text.push('\n');
        let add = text.len() as u64;
        if !has_room(self.written, add) {
            self.capped = true;
            let _ = self
                .file
                .write_all(b"{\"cat\":\"diag\",\"ev\":\"size_cap\"}\n");
            return;
        }
        if self.file.write_all(text.as_bytes()).is_ok() {
            self.written += add;
        }
    }
}

struct State {
    spec: Spec,
    start: Instant,
    sink: Mutex<Option<Sink>>,
}

static STATE: OnceLock<State> = OnceLock::new();
/// `true` once the log is on: the only thing a call site reads while the log is off.
static ON: AtomicBool = AtomicBool::new(false);

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Opens the log if it is switched on (see the module docs). Call it once, early, after the
/// single-instance check; calling it again does nothing. Never fails: a log that cannot be
/// opened is reported on stderr and the app runs on.
pub fn init() {
    if STATE.get().is_some() {
        return;
    }
    let support = config::app_support_dir();
    let env = std::env::var(ENV_SWITCH).ok();
    let marker = config::read_text_capped(support.join(SWITCH_FILE), 1024);
    let Some(spec) = resolve_spec(env.as_deref(), marker.as_deref()) else {
        return;
    };
    let dir = log_dir_from(std::env::var(ENV_DIR).ok().as_deref(), &support);
    let sink = open_sink(&dir);
    if let Some(s) = &sink {
        eprintln!("CursorPie: debug log: {}", s.path.display());
    }
    let state = State {
        spec,
        start: Instant::now(),
        sink: Mutex::new(sink),
    };
    if STATE.set(state).is_err() {
        return;
    }
    ON.store(true, Ordering::SeqCst);
    install_panic_hook();
    write_header();
}

/// Creates the folder, deletes the oldest files and opens the file of this run.
fn open_sink(dir: &Path) -> Option<Sink> {
    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!(
            "CursorPie: cannot create the debug log folder {}: {e}",
            dir.display()
        );
        return None;
    }
    let names: Vec<String> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    // keep one place free for the file about to be created
    for old in stale(&names, KEEP_FILES.saturating_sub(1)) {
        let _ = fs::remove_file(dir.join(old));
    }
    let path = dir.join(file_name(unix_secs(), std::process::id()));
    match File::create(&path) {
        Ok(file) => Some(Sink {
            file,
            path,
            written: 0,
            capped: false,
        }),
        Err(e) => {
            eprintln!(
                "CursorPie: cannot create the debug log {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// Whether an event of category `cat` would be written right now.
pub fn enabled(cat: &str) -> bool {
    ON.load(Ordering::Relaxed) && STATE.get().is_some_and(|s| s.spec.allows(cat, false))
}

/// Like [`enabled`], for a high-volume event that needs `trace` as well.
pub fn trace_enabled(cat: &str) -> bool {
    ON.load(Ordering::Relaxed) && STATE.get().is_some_and(|s| s.spec.allows(cat, true))
}

/// The file the log is written to, while it is on and could be opened.
pub fn active_path() -> Option<PathBuf> {
    let st = STATE.get()?;
    let guard = config::lock(&st.sink);
    guard.as_ref().map(|s| s.path.clone())
}

/// Writes one line. It does not check the switch: the macros [`diag_log!`](crate::diag_log) and
/// [`diag_trace!`](crate::diag_trace) do, and only then evaluate their arguments.
pub fn write(cat: &str, ev: &str, fields: &[(&str, Value)]) {
    let Some(st) = STATE.get() else { return };
    let thread = std::thread::current();
    let line = format_line(
        st.start.elapsed().as_secs_f64(),
        thread.name().unwrap_or("?"),
        cat,
        ev,
        fields,
    );
    if let Some(sink) = config::lock(&st.sink).as_mut() {
        sink.push(&line);
    }
}

/// [`write`] for fields whose names are built at run time.
fn write_owned(cat: &str, ev: &str, owned: &[(String, Value)]) {
    let fields: Vec<(&str, Value)> = owned.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    write(cat, ev, &fields);
}

/// Writes a line of `cat` / `ev`. The fields are evaluated only while the log is on.
///
/// ```ignore
/// crate::diag_log!("ui", "show_pie", x = x, y = y);
/// ```
#[macro_export]
macro_rules! diag_log {
    ($cat:expr, $ev:expr $(, $k:ident = $v:expr)* $(,)?) => {
        if $crate::diag::enabled($cat) {
            $crate::diag::write(
                $cat,
                $ev,
                &[$((stringify!($k), $crate::diag::Value::from($v))),*],
            );
        }
    };
}

/// Like [`diag_log!`](crate::diag_log), for events that come in the hundreds (written only with
/// `trace` in the switch).
#[macro_export]
macro_rules! diag_trace {
    ($cat:expr, $ev:expr $(, $k:ident = $v:expr)* $(,)?) => {
        if $crate::diag::trace_enabled($cat) {
            $crate::diag::write(
                $cat,
                $ev,
                &[$((stringify!($k), $crate::diag::Value::from($v))),*],
            );
        }
    };
}

// -------------------------------------------------------------------------------- snapshots

/// The first line: the program, the machine and the session variables.
fn write_header() {
    let secs = unix_secs();
    let mut owned: Vec<(String, Value)> = vec![
        (
            "version".to_string(),
            Value::from(env!("CARGO_PKG_VERSION")),
        ),
        ("os".to_string(), Value::from(std::env::consts::OS)),
        ("arch".to_string(), Value::from(std::env::consts::ARCH)),
        ("pid".to_string(), Value::from(std::process::id())),
        ("unix".to_string(), Value::from(secs)),
        ("utc".to_string(), Value::from(iso_utc(secs))),
        (
            "debug_build".to_string(),
            Value::from(cfg!(debug_assertions)),
        ),
    ];
    for name in ENV_REPORTED {
        let value = std::env::var(name).unwrap_or_default();
        owned.push((
            format!("env_{}", name.to_ascii_lowercase()),
            Value::from(value),
        ));
    }
    write_owned("diag", "start", &owned);
}

/// The displays and the pointer as the platform reports them. Call it once the window exists
/// (inside the eframe creator).
pub fn startup_snapshot() {
    if !enabled("sys") {
        return;
    }
    let rects = crate::platform::display_rects();
    let mut owned: Vec<(String, Value)> = vec![("monitors".to_string(), Value::from(rects.len()))];
    for (i, r) in rects.iter().enumerate() {
        owned.push((format!("mon{i}"), Value::from(rect_str(r.x, r.y, r.w, r.h))));
    }
    let cursor = crate::platform::cursor_pos();
    owned.push(("cursor_ok".to_string(), Value::from(cursor.is_some())));
    if let Some((x, y)) = cursor {
        owned.push(("cursor_x".to_string(), Value::from(x)));
        owned.push(("cursor_y".to_string(), Value::from(y)));
    }
    write_owned("sys", "startup", &owned);
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let payload = info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            String::new()
        };
        write(
            "panic",
            "panic",
            &[("at", Value::from(at)), ("message", Value::from(message))],
        );
        previous(info);
    }));
}

// ------------------------------------------------------------------------ window observation

/// Where the pie asked for its window, while the window is being watched.
struct Expect {
    x: f32,
    y: f32,
    at: Instant,
    logged: u32,
    last: Option<[f32; 4]>,
}

static EXPECT: Mutex<Option<Expect>> = Mutex::new(None);
static OBSERVE: AtomicBool = AtomicBool::new(false);

/// Says where the window was just asked to be (top-left, in points). For the next two seconds
/// [`observe_window`] logs where the window manager really put it, which is how "the position was
/// ignored" is told apart from "the position was wrong".
pub fn expect_window(x: f32, y: f32) {
    if !enabled("win") {
        return;
    }
    *config::lock(&EXPECT) = Some(Expect {
        x,
        y,
        at: Instant::now(),
        logged: 0,
        last: None,
    });
    OBSERVE.store(true, Ordering::SeqCst);
}

/// Whether [`observe_window`] has work to do (one atomic read, for the per-frame call).
pub fn observing() -> bool {
    OBSERVE.load(Ordering::Relaxed)
}

/// Logs the window rectangle `[x, y, width, height]` (points) the toolkit reports, when it
/// changed. Call it every frame while [`observing`] is true.
pub fn observe_window(outer: Option<[f32; 4]>, pixels_per_point: Option<f32>) {
    let (ex, ey, age) = {
        let mut guard = config::lock(&EXPECT);
        let Some(e) = guard.as_mut() else {
            OBSERVE.store(false, Ordering::SeqCst);
            return;
        };
        let age = e.at.elapsed().as_millis();
        if age > OBSERVE_FOR_MS || e.logged >= MAX_OBSERVATIONS {
            *guard = None;
            OBSERVE.store(false, Ordering::SeqCst);
            return;
        }
        if e.logged > 0 && e.last == outer {
            return;
        }
        e.logged += 1;
        e.last = outer;
        (e.x, e.y, age)
    };
    let mut owned: Vec<(String, Value)> = vec![
        (
            "age_ms".to_string(),
            Value::from(u64::try_from(age).unwrap_or(u64::MAX)),
        ),
        ("ex".to_string(), Value::from(ex)),
        ("ey".to_string(), Value::from(ey)),
    ];
    match outer {
        Some([ox, oy, ow, oh]) => {
            owned.push(("ox".to_string(), Value::from(ox)));
            owned.push(("oy".to_string(), Value::from(oy)));
            owned.push(("ow".to_string(), Value::from(ow)));
            owned.push(("oh".to_string(), Value::from(oh)));
            owned.push(("dx".to_string(), Value::from(ox - ex)));
            owned.push(("dy".to_string(), Value::from(oy - ey)));
        }
        None => owned.push(("outer".to_string(), Value::from("none"))),
    }
    if let Some(p) = pixels_per_point {
        owned.push(("ppp".to_string(), Value::from(p)));
    }
    write_owned("win", "win_obs", &owned);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_reads_lists_and_off_words() {
        assert_eq!(Spec::parse(""), None);
        assert_eq!(Spec::parse("0"), None);
        assert_eq!(Spec::parse("tap,off"), None, "off anywhere means off");
        let all = Spec::parse("1").unwrap();
        assert!(all.allows("tap", false) && all.allows("anything", false));
        assert!(!all.allows("tap", true), "trace events need `trace`");
        let some = Spec::parse(" TAP, ui ").unwrap();
        assert!(some.allows("tap", false) && some.allows("ui", false));
        assert!(!some.allows("win", false));
    }

    #[test]
    fn trace_alone_means_everything_with_trace() {
        let t = Spec::parse("trace").unwrap();
        assert!(t.allows("tap", true) && t.allows("ui", false));
        let only_tap = Spec::parse("tap,trace").unwrap();
        assert!(only_tap.allows("tap", true));
        assert!(!only_tap.allows("ui", true) && !only_tap.allows("ui", false));
    }

    #[test]
    fn the_variable_wins_over_the_marker_file() {
        assert_eq!(resolve_spec(Some("0"), Some("1")), None);
        assert_eq!(resolve_spec(Some("tap"), Some("1")), Spec::parse("tap"));
        assert_eq!(
            resolve_spec(None, Some("")),
            Spec::parse("1"),
            "empty file = on"
        );
        assert_eq!(resolve_spec(None, Some("ui")), Spec::parse("ui"));
        assert_eq!(resolve_spec(None, None), None);
    }

    #[test]
    fn the_log_folder_can_be_moved() {
        let support = Path::new("/home/u/.config/cursorpie");
        assert_eq!(log_dir_from(None, support), support.join("logs"));
        assert_eq!(log_dir_from(Some("  "), support), support.join("logs"));
        assert_eq!(log_dir_from(Some("/tmp/x"), support), Path::new("/tmp/x"));
    }

    #[test]
    fn file_names_sort_by_time_and_only_ours_are_recognized() {
        assert_eq!(file_name(42, 7), "cursorpie-0000000042-7.jsonl");
        assert!(file_name(1_793_000_000, 1) > file_name(999_999_999, 99999));
        assert!(is_log_name("cursorpie-0000000042-7.jsonl"));
        assert!(!is_log_name("config.json"));
        assert!(!is_log_name("cursorpie-notes.txt"));
    }

    #[test]
    fn the_oldest_logs_go_first_and_other_files_stay() {
        let names: Vec<String> = [
            "cursorpie-0000000300-1.jsonl",
            "cursorpie-0000000100-1.jsonl",
            "notes.txt",
            "cursorpie-0000000200-1.jsonl",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            stale(&names, 2),
            ["cursorpie-0000000100-1.jsonl".to_string()]
        );
        assert!(stale(&names, 3).is_empty());
        assert!(stale(&names, 10).is_empty());
        assert_eq!(stale(&names, 0).len(), 3, "notes.txt is never listed");
    }

    #[test]
    fn a_file_stops_growing_at_the_cap() {
        assert!(has_room(0, 100));
        assert!(has_room(MAX_FILE_BYTES - 10, 10));
        assert!(!has_room(MAX_FILE_BYTES - 10, 11));
        assert!(!has_room(u64::MAX, 1), "no overflow");
    }

    #[test]
    fn times_are_written_in_utc() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_utc(1_793_000_000), "2026-10-26T07:33:20Z");
        assert_eq!(iso_utc(4_102_444_800), "2100-01-01T00:00:00Z");
    }

    #[test]
    fn rectangles_are_written_the_way_the_analyzer_reads_them() {
        assert_eq!(rect_str(0.0, 0.0, 1920.0, 1080.0), "0,0 1920x1080");
        assert_eq!(rect_str(-1280.0, 0.0, 1280.0, 1024.0), "-1280,0 1280x1024");
    }

    #[test]
    fn a_line_is_flat_json_and_reserved_names_are_kept_apart() {
        let line = format_line(
            1.23456,
            "main",
            "ui",
            "show_pie",
            &[
                ("x", Value::from(812.5)),
                ("ok", Value::from(true)),
                ("name", Value::from("a\"b")),
                ("t", Value::from(9)),
            ],
        );
        assert!(!line.contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        let o = v.as_object().unwrap();
        assert_eq!(o["t"], 1.235);
        assert_eq!(o["_t"], 9, "a field called `t` does not replace the time");
        assert_eq!(o["cat"], "ui");
        assert_eq!(o["ev"], "show_pie");
        assert_eq!(o["th"], "main");
        assert_eq!(o["x"], 812.5);
        assert_eq!(o["ok"], true);
        assert_eq!(o["name"], "a\"b");
        assert!(o.values().all(|v| !v.is_array() && !v.is_object()));
    }

    #[test]
    fn a_log_that_is_off_costs_nothing_and_writes_nothing() {
        // the tests never switch the global log on
        assert!(!enabled("ui"));
        assert!(!trace_enabled("ui"));
        assert!(active_path().is_none());
        crate::diag_log!("ui", "never", x = 1.0);
        crate::diag_trace!("ui", "never");
        assert!(!observing());
        expect_window(1.0, 2.0);
        assert!(!observing(), "nothing is watched while the log is off");
    }
}
