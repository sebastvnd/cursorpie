//! `cargo tools diag [FILE]`: reads a debug log of the app and says where the chain broke.
//!
//! The app writes the log when it is started with `CURSORPIE_DEBUG=1` (the log is written by
//! the `diag` crate in `diag/`, see `docs/DEBUGGING.md`). Every line is a flat JSON object. This tool follows one
//! gesture through the layers and compares the numbers each layer reports:
//!
//! ```text
//! press (tap)  ->  msg_show_pie (UI)  ->  show_pie (asked position)  ->  win_obs (real position)
//! ```
//!
//! Without a file it reads the newest log in the default log folder. The tool uses only the
//! standard library (see `tools/Cargo.toml`), so it has a small JSON reader of its own: it reads
//! exactly what the app writes (flat objects of strings, numbers and booleans) and nothing else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// How far (in points) two positions that should be the same may differ before it is reported.
const TOLERANCE: f64 = 3.0;
/// How far the window centre may be from the monitor centre to count as "centered".
const CENTER_TOLERANCE: f64 = 6.0;

// ------------------------------------------------------------------------------- the contract

/// One event this tool (or `logview.rs`) reads, and the fields it reads from it.
pub struct Expected {
    pub cat: &'static str,
    pub ev: &'static str,
    pub fields: &'static [&'static str],
}

/// Every event the analysis depends on. `cargo tools diag-lint` (and `cargo test`) checks that a
/// log line in `src/` writes each of them with these fields, so renaming an event or a field in
/// the app without teaching the analyzer fails the checks instead of silently making the analysis
/// blind. An event or field the analyzer starts to read is added here, in the same commit.
///
/// The events written by the `diag` crate itself (`diag.start`, `diag.size_cap`, `panic.panic`)
/// and the fields of the ones built at run time (`sys.startup`, `win.win_obs`) are not looked
/// for in the source: the lint knows them (`lint::BUILT_IN`).
pub const EXPECTED: &[Expected] = &[
    Expected {
        cat: "diag",
        ev: "start",
        fields: &[],
    },
    Expected {
        cat: "diag",
        ev: "size_cap",
        fields: &[],
    },
    Expected {
        cat: "panic",
        ev: "panic",
        fields: &["at", "message"],
    },
    Expected {
        cat: "sys",
        ev: "startup",
        fields: &[],
    },
    Expected {
        cat: "x11",
        ev: "scale",
        fields: &["scale", "xft_scale_input", "env_override"],
    },
    Expected {
        cat: "tap",
        ev: "session",
        fields: &["server", "desktops", "has_x_display", "problem"],
    },
    Expected {
        cat: "tap",
        ev: "grabs",
        fields: &["wanted", "grabbed", "all_grabbed"],
    },
    Expected {
        cat: "tap",
        ev: "press",
        fields: &["x", "y", "scale", "query_px"],
    },
    Expected {
        cat: "tap",
        ev: "release",
        fields: &["x", "y"],
    },
    Expected {
        cat: "tap",
        ev: "on_press",
        fields: &["x", "y"],
    },
    Expected {
        cat: "tap",
        ev: "on_release",
        fields: &["x", "y"],
    },
    Expected {
        cat: "tap",
        ev: "hold_open",
        fields: &["x", "y"],
    },
    Expected {
        cat: "ui",
        ev: "msg_show_pie",
        fields: &["x", "y"],
    },
    Expected {
        cat: "ui",
        ev: "show_pie",
        fields: &["x", "y", "left", "top", "mon"],
    },
    Expected {
        cat: "ui",
        ev: "show_pie_ignored",
        fields: &["x", "y", "reason"],
    },
    Expected {
        cat: "ui",
        ev: "place_card",
        fields: &[],
    },
    Expected {
        cat: "ui",
        ev: "cursor_or_default",
        fields: &["ok", "x", "y"],
    },
    Expected {
        cat: "win",
        ev: "win_obs",
        fields: &[],
    },
    Expected {
        cat: "cfg",
        ev: "save",
        fields: &["ok", "how", "error"],
    },
    // `outcome`, `bytes` and `ms` are added to the line by its `Timer`, not written in the call
    Expected {
        cat: "cfg",
        ev: "load",
        fields: &[],
    },
    Expected {
        cat: "act",
        ev: "exec",
        fields: &["kind", "wait_ms"],
    },
    Expected {
        cat: "tap",
        ev: "hook",
        fields: &["backend", "state"],
    },
    Expected {
        cat: "act",
        ev: "spawn_failed",
        fields: &["program", "error"],
    },
    Expected {
        cat: "act",
        ev: "exited",
        fields: &["program", "code"],
    },
    Expected {
        cat: "act",
        ev: "shell_execute",
        fields: &["ok", "code"],
    },
];

// ------------------------------------------------------------------------------------ reading

/// One value of a log line.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
}

/// One log line.
pub type Rec = BTreeMap<String, Val>;

/// A cursor over the characters of one line.
struct Reader<'a> {
    chars: &'a [char],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += 1;
        Some(c)
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.at += 1;
        }
    }

    fn expect(&mut self, want: char) -> Option<()> {
        (self.bump()? == want).then_some(())
    }

    fn word(&mut self, word: &str) -> bool {
        let n = word.chars().count();
        let found = self
            .chars
            .get(self.at..self.at + n)
            .is_some_and(|s| s.iter().copied().eq(word.chars()));
        if found {
            self.at += n;
        }
        found
    }

    fn string(&mut self) -> Option<String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.bump()? {
                '"' => return Some(out),
                '\\' => match self.bump()? {
                    '"' => out.push('"'),
                    '\\' => out.push('\\'),
                    '/' => out.push('/'),
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    'r' => out.push('\r'),
                    'b' => out.push('\u{8}'),
                    'f' => out.push('\u{c}'),
                    'u' => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            code = code * 16 + self.bump()?.to_digit(16)?;
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                    }
                    _ => return None,
                },
                c => out.push(c),
            }
        }
    }

    fn number(&mut self) -> Option<Val> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
        {
            self.at += 1;
        }
        let text: String = self.chars[start..self.at].iter().collect();
        text.parse::<f64>().ok().map(Val::Num)
    }

    fn value(&mut self) -> Option<Val> {
        match self.peek()? {
            '"' => self.string().map(Val::Str),
            't' => self.word("true").then_some(Val::Bool(true)),
            'f' => self.word("false").then_some(Val::Bool(false)),
            'n' => self.word("null").then_some(Val::Null),
            _ => self.number(),
        }
    }
}

/// Reads one line of the log. `None` when it is not a flat JSON object (a half-written last line
/// after a crash, for one).
pub fn parse_line(line: &str) -> Option<Rec> {
    let chars: Vec<char> = line.trim().chars().collect();
    let mut r = Reader {
        chars: &chars,
        at: 0,
    };
    r.expect('{')?;
    let mut rec = Rec::new();
    r.skip_space();
    if r.peek() == Some('}') {
        r.at += 1;
    } else {
        loop {
            r.skip_space();
            let key = r.string()?;
            r.skip_space();
            r.expect(':')?;
            r.skip_space();
            let value = r.value()?;
            rec.insert(key, value);
            r.skip_space();
            match r.bump()? {
                ',' => {}
                '}' => break,
                _ => return None,
            }
        }
    }
    (r.at == chars.len()).then_some(rec)
}

/// A number field of a line.
pub fn num(r: &Rec, key: &str) -> Option<f64> {
    match r.get(key)? {
        Val::Num(n) => Some(*n),
        _ => None,
    }
}

/// A text field of a line.
pub fn text<'a>(r: &'a Rec, key: &str) -> Option<&'a str> {
    match r.get(key)? {
        Val::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

/// A boolean field of a line.
pub fn flag(r: &Rec, key: &str) -> Option<bool> {
    match r.get(key)? {
        Val::Bool(b) => Some(*b),
        _ => None,
    }
}

/// Whether a line is the event `ev` of the category `cat`.
pub fn is(r: &Rec, cat: &str, ev: &str) -> bool {
    text(r, "cat") == Some(cat) && text(r, "ev") == Some(ev)
}

/// `x,y WxH` (what `diag::rect_str` writes) as `[x, y, w, h]`.
pub fn parse_rect(s: &str) -> Option<[f64; 4]> {
    let (pos, size) = s.split_once(' ')?;
    let (x, y) = pos.split_once(',')?;
    let (w, h) = size.split_once('x')?;
    Some([
        x.parse().ok()?,
        y.parse().ok()?,
        w.parse().ok()?,
        h.parse().ok()?,
    ])
}

/// `a,b` as two numbers (the pointer the X server reported).
fn parse_pair(s: &str) -> Option<(f64, f64)> {
    let (a, b) = s.split_once(',')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

// ---------------------------------------------------------------------------------- analyzing

/// What was found in a log.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    /// What the log says about the machine and the session.
    pub facts: Vec<String>,
    /// What does not add up.
    pub findings: Vec<String>,
    /// Lines that could not be read.
    pub bad_lines: usize,
}

/// One `show_pie` and what was seen of its window afterwards.
struct Shown {
    left: f64,
    top: f64,
    monitor: Option<[f64; 4]>,
    first: Option<(f64, f64)>,
    last: Option<(u64, [f64; 4])>,
    seen_without_rect: bool,
}

/// Reads every line and compares the layers. Pure: the same lines give the same report.
pub fn analyze(lines: &[String]) -> Report {
    let mut report = Report::default();
    let mut recs: Vec<Rec> = Vec::new();
    for line in lines.iter().filter(|l| !l.trim().is_empty()) {
        match parse_line(line) {
            Some(r) => recs.push(r),
            None => report.bad_lines += 1,
        }
    }

    let mut presses = 0usize;
    let mut last_press: Option<(f64, f64)> = None;
    let mut press_unchecked = false;
    let mut shown: Vec<Shown> = Vec::new();
    let mut current: Option<usize> = None;
    let mut x11_scale: Option<f64> = None;
    let mut ppp: Option<f64> = None;
    let mut fallbacks = 0usize;

    for r in &recs {
        if is(r, "diag", "start") {
            report.facts.push(describe_start(r));
        } else if is(r, "sys", "startup") {
            report.facts.push(describe_startup(r));
        } else if is(r, "tap", "session") {
            report.facts.push(describe_session(r));
        } else if is(r, "x11", "scale") {
            x11_scale = num(r, "scale");
            report.facts.push(format!(
                "X11 display scale used for coordinates: {} (Xft input {}, WINIT_X11_SCALE_FACTOR '{}')",
                num(r, "scale").unwrap_or(0.0),
                num(r, "xft_scale_input").unwrap_or(0.0),
                text(r, "env_override").unwrap_or("")
            ));
        } else if is(r, "tap", "grabs") && flag(r, "all_grabbed") == Some(false) {
            report.findings.push(format!(
                "not every mouse button could be grabbed (wanted {}, grabbed {}): another program holds a grab",
                text(r, "wanted").unwrap_or("?"),
                text(r, "grabbed").unwrap_or("?")
            ));
        } else if is(r, "tap", "press") {
            presses += 1;
            let (Some(x), Some(y)) = (num(r, "x"), num(r, "y")) else {
                continue;
            };
            last_press = Some((x, y));
            press_unchecked = true;
            let scale = num(r, "scale").unwrap_or(1.0);
            if let Some((qx, qy)) = text(r, "query_px").and_then(parse_pair) {
                let (dx, dy) = (x - qx / scale, y - qy / scale);
                if dx.abs() > TOLERANCE + 1.0 || dy.abs() > TOLERANCE + 1.0 {
                    report.findings.push(format!(
                        "press #{presses}: the event says ({x:.0},{y:.0}) but the X server reports the pointer at ({:.0},{:.0}) a moment later (off by {dx:+.0},{dy:+.0}); the pointer coordinates are wrong before the UI even sees them",
                        qx / scale,
                        qy / scale
                    ));
                }
            }
        } else if is(r, "ui", "msg_show_pie") {
            if let (true, Some((px, py)), Some(x), Some(y)) =
                (press_unchecked, last_press, num(r, "x"), num(r, "y"))
            {
                press_unchecked = false;
                if (x - px).abs() > TOLERANCE || (y - py).abs() > TOLERANCE {
                    report.findings.push(format!(
                        "the pie was requested at ({x:.0},{y:.0}) but the press that opened it was at ({px:.0},{py:.0}): the position changed between the tap thread and the UI"
                    ));
                }
            }
        } else if is(r, "ui", "show_pie_ignored") {
            report.findings.push(format!(
                "a pie at ({:.0},{:.0}) was ignored: {}",
                num(r, "x").unwrap_or(0.0),
                num(r, "y").unwrap_or(0.0),
                text(r, "reason").unwrap_or("no reason given")
            ));
        } else if is(r, "ui", "show_pie") {
            shown.push(Shown {
                left: num(r, "left").unwrap_or(0.0),
                top: num(r, "top").unwrap_or(0.0),
                monitor: text(r, "mon").and_then(parse_rect),
                first: None,
                last: None,
                seen_without_rect: false,
            });
            current = Some(shown.len() - 1);
        } else if is(r, "ui", "place_card") {
            current = None; // Settings and Welcome are centered on purpose
        } else if is(r, "win", "win_obs") {
            ppp = num(r, "ppp").or(ppp);
            let Some(s) = current.and_then(|i| shown.get_mut(i)) else {
                continue;
            };
            match (num(r, "ox"), num(r, "oy"), num(r, "ow"), num(r, "oh")) {
                (Some(ox), Some(oy), Some(ow), Some(oh)) => {
                    s.first.get_or_insert((ox, oy));
                    // the age is written as a whole number of milliseconds
                    s.last = Some((num(r, "age_ms").unwrap_or(0.0) as u64, [ox, oy, ow, oh]));
                }
                _ => s.seen_without_rect = true,
            }
        } else if is(r, "ui", "cursor_or_default") && flag(r, "ok") == Some(false) {
            fallbacks += 1;
        } else if is(r, "panic", "panic") {
            report.findings.push(format!(
                "PANIC at {}: {}",
                text(r, "at").unwrap_or("?"),
                text(r, "message").unwrap_or("")
            ));
        } else if is(r, "diag", "size_cap") {
            report
                .findings
                .push("the log hit its size limit: later events are missing".to_string());
        }
    }

    if fallbacks > 0 {
        report.findings.push(format!(
            "the pointer position could not be read {fallbacks} time(s): the fallback (400,300) was used"
        ));
    }
    // the checks of the other views of the log (slow steps, failed saves, queued actions)
    report
        .findings
        .extend(crate::logview::extra_findings(&recs));
    if let (Some(scale), Some(p)) = (x11_scale, ppp)
        && (scale - p).abs() > 0.05
    {
        report.findings.push(format!(
            "the app computes coordinates with scale {scale} but the window toolkit uses {p}: positions drift by that ratio (set WINIT_X11_SCALE_FACTOR to make them agree)"
        ));
    }
    let any_observed = shown.iter().any(|s| s.last.is_some());
    for (i, s) in shown.iter().enumerate() {
        check_window(&mut report, i + 1, s, any_observed);
    }

    report.facts.push(format!(
        "{} line(s), {presses} press(es), {} pie window(s)",
        recs.len(),
        shown.len()
    ));
    if shown.is_empty() {
        report.findings.push(
            "no pie was ever shown in this log: nothing to compare (was the gesture made while the log was on?)"
                .to_string(),
        );
    }
    report
}

/// Compares where the window was asked to be with where it was seen.
fn check_window(report: &mut Report, n: usize, s: &Shown, any_observed: bool) {
    let Some((age, [ox, oy, ow, oh])) = s.last else {
        if any_observed {
            let why = if s.seen_without_rect {
                "the toolkit reported no window rectangle"
            } else {
                "no frame was drawn in the two seconds after it"
            };
            report
                .findings
                .push(format!("pie #{n}: the window was never observed: {why}"));
        }
        return;
    };
    let (dx, dy) = (ox - s.left, oy - s.top);
    if dx.abs() <= TOLERANCE && dy.abs() <= TOLERANCE {
        return;
    }
    let mut msg = format!(
        "pie #{n}: the window was asked to be at ({:.0},{:.0}) but the toolkit reports it at ({ox:.0},{oy:.0}) after {age} ms (off by {dx:+.0},{dy:+.0})",
        s.left, s.top
    );
    if let Some((fx, fy)) = s.first
        && ((fx - ox).abs() > TOLERANCE || (fy - oy).abs() > TOLERANCE)
    {
        msg.push_str(&format!("; it first appeared at ({fx:.0},{fy:.0})"));
    }
    if let Some([mx, my, mw, mh]) = s.monitor {
        let (wc_x, wc_y) = (ox + ow / 2.0, oy + oh / 2.0);
        let (mc_x, mc_y) = (mx + mw / 2.0, my + mh / 2.0);
        if (wc_x - mc_x).abs() <= CENTER_TOLERANCE && (wc_y - mc_y).abs() <= CENTER_TOLERANCE {
            msg.push_str(
                ". The window sits in the middle of its monitor: the window manager placed it itself and ignored the requested position",
            );
        }
    }
    report.findings.push(msg);
}

fn describe_start(r: &Rec) -> String {
    let env = |key: &str| {
        let v = text(r, key).unwrap_or("");
        let shown = if v.is_empty() { "-" } else { v };
        shown.to_string()
    };
    format!(
        "CursorPie {} on {}/{} ({}), session {}, desktop {}, DISPLAY {}, WAYLAND_DISPLAY {}, WINIT_UNIX_BACKEND {}",
        text(r, "version").unwrap_or("?"),
        text(r, "os").unwrap_or("?"),
        text(r, "arch").unwrap_or("?"),
        text(r, "utc").unwrap_or("?"),
        env("env_xdg_session_type"),
        env("env_xdg_current_desktop"),
        env("env_display"),
        env("env_wayland_display"),
        env("env_winit_unix_backend"),
    )
}

fn describe_startup(r: &Rec) -> String {
    let count = num(r, "monitors").unwrap_or(0.0) as usize;
    let list: Vec<String> = (0..count)
        .filter_map(|i| text(r, &format!("mon{i}")).map(str::to_string))
        .collect();
    format!(
        "monitors (logical points): {}; pointer at start: {}",
        if list.is_empty() {
            "none".to_string()
        } else {
            list.join(" | ")
        },
        if flag(r, "cursor_ok") == Some(true) {
            format!(
                "({:.0},{:.0})",
                num(r, "cursor_x").unwrap_or(0.0),
                num(r, "cursor_y").unwrap_or(0.0)
            )
        } else {
            "unknown".to_string()
        }
    )
}

fn describe_session(r: &Rec) -> String {
    let problem = text(r, "problem").unwrap_or("");
    format!(
        "session: server {}, desktops '{}', X display {}{}",
        text(r, "server").unwrap_or("?"),
        text(r, "desktops").unwrap_or(""),
        flag(r, "has_x_display").unwrap_or(false),
        if problem.is_empty() {
            String::new()
        } else {
            format!(" (the app itself warns: {problem})")
        }
    )
}

/// The report as text for the terminal.
pub fn render(report: &Report) -> String {
    let mut out = String::from("Facts\n");
    for f in &report.facts {
        out.push_str(&format!("  - {f}\n"));
    }
    out.push_str("\nFindings\n");
    if report.findings.is_empty() {
        out.push_str("  none: every layer agrees with the one before it\n");
    }
    for (i, f) in report.findings.iter().enumerate() {
        out.push_str(&format!("  {}. {f}\n", i + 1));
    }
    if report.bad_lines > 0 {
        out.push_str(&format!(
            "\n{} line(s) could not be read (the last line of a crashed run is often cut off)\n",
            report.bad_lines
        ));
    }
    out
}

// ------------------------------------------------------------------------------ finding a log

/// The start and the end of the file names of the app's logs (see `diag::file_name`).
pub const LOG_PREFIX: &str = "cursorpie-";
/// The end of the file names of the app's logs.
pub const LOG_SUFFIX: &str = ".jsonl";

/// Whether a file name is one of the app's logs, so no other file is ever read or deleted.
pub fn is_log_name(name: &str) -> bool {
    name.starts_with(LOG_PREFIX) && name.ends_with(LOG_SUFFIX)
}

/// The newest log among file names (the names sort by time, see `diag::file_name`).
fn newest(names: &[String]) -> Option<&String> {
    names.iter().filter(|n| is_log_name(n)).max()
}

/// Where the app writes its logs on this OS, from an environment lookup. Mirrors
/// `diag::log_dir_from` (crate `diag`) and `config::support_dir_from` of the app.
pub fn default_log_dir(get: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if let Some(d) = get("CURSORPIE_DEBUG_DIR").filter(|d| !d.trim().is_empty()) {
        return Some(PathBuf::from(d.trim()));
    }
    let home = get("HOME").map(PathBuf::from);
    let support = if cfg!(windows) {
        let base = get("APPDATA").filter(|d| !d.is_empty()).map(PathBuf::from);
        base.or_else(|| {
            get("USERPROFILE").map(|h| PathBuf::from(h).join("AppData").join("Roaming"))
        })?
        .join("CursorPie")
    } else if cfg!(target_os = "macos") {
        home?
            .join("Library")
            .join("Application Support")
            .join("CursorPie")
    } else {
        let config_home = match get("XDG_CONFIG_HOME").map(|p| p.trim().to_string()) {
            Some(p) if p.starts_with('/') => PathBuf::from(p),
            _ => home?.join(".config"),
        };
        config_home.join("cursorpie")
    };
    Some(support.join("logs"))
}

/// The file to read: the one named, else the newest in the default folder.
pub fn pick_file(given: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = given {
        return Ok(p.to_path_buf());
    }
    let dir = default_log_dir(&|k| std::env::var(k).ok())
        .ok_or("cannot tell where the logs are (no home folder); name the file")?;
    let names: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| {
            format!(
                "cannot read {}: {e}\nStart the app with CURSORPIE_DEBUG=1 first, or name a log file",
                dir.display()
            )
        })?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let name = newest(&names).ok_or_else(|| {
        format!(
            "no log in {}: start the app with CURSORPIE_DEBUG=1 and make the gesture",
            dir.display()
        )
    })?;
    Ok(dir.join(name))
}

/// Reads a log and prints what it shows.
pub fn run(file: Option<&Path>) -> Result<ExitCode, String> {
    let path = pick_file(file)?;
    let body = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let lines: Vec<String> = body.lines().map(str::to_string).collect();
    println!("Log: {}\n", path.display());
    print!("{}", render(&analyze(&lines)));
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn a_flat_line_is_read() {
        let r = parse_line(
            r#"{"cat":"ui","ev":"show_pie","t":1.5,"x":-12.25,"ok":true,"name":"a\"b\u00e9","none":null}"#,
        )
        .unwrap();
        assert_eq!(text(&r, "cat"), Some("ui"));
        assert_eq!(num(&r, "t"), Some(1.5));
        assert_eq!(num(&r, "x"), Some(-12.25));
        assert_eq!(flag(&r, "ok"), Some(true));
        assert_eq!(text(&r, "name"), Some("a\"bé"));
        assert_eq!(r.get("none"), Some(&Val::Null));
        assert_eq!(parse_line("{}"), Some(Rec::new()));
    }

    #[test]
    fn what_is_not_a_flat_object_is_refused() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("not json"), None);
        assert_eq!(parse_line(r#"{"a":1"#), None, "a cut-off line");
        assert_eq!(parse_line(r#"{"a":[1]}"#), None, "no nesting");
        assert_eq!(parse_line(r#"{"a":1} x"#), None, "nothing after the object");
        assert_eq!(parse_line(r#"{"a" 1}"#), None);
    }

    #[test]
    fn rectangles_and_pairs_are_read() {
        assert_eq!(
            parse_rect("0,0 1920x1080"),
            Some([0.0, 0.0, 1920.0, 1080.0])
        );
        assert_eq!(
            parse_rect("-1280,0 1280x1024"),
            Some([-1280.0, 0.0, 1280.0, 1024.0])
        );
        assert_eq!(parse_rect("nonsense"), None);
        assert_eq!(parse_pair("812, 344"), Some((812.0, 344.0)));
        assert_eq!(parse_pair("812"), None);
    }

    /// A pie that was asked for at (700,400) and ended up in the middle of the monitor.
    const CENTERED_BY_THE_WM: &str = r#"
{"cat":"diag","ev":"start","t":0.0,"th":"main","version":"0.5.0","os":"linux","arch":"x86_64","utc":"2026-10-26T07:33:20Z","env_xdg_session_type":"x11","env_xdg_current_desktop":"GNOME","env_display":":0","env_wayland_display":"","env_winit_unix_backend":""}
{"cat":"x11","ev":"scale","t":0.1,"th":"main","scale":1.0,"xft_scale_input":1.0,"env_override":""}
{"cat":"tap","ev":"press","t":5.0,"th":"cursorpie-x11-tap","detail":1,"root_x":800.0,"root_y":500.0,"scale":1.0,"x":800.0,"y":500.0,"query_px":"800,500"}
{"cat":"ui","ev":"msg_show_pie","t":5.3,"th":"main","x":800.0,"y":500.0,"profile":0,"depth":0}
{"cat":"ui","ev":"show_pie","t":5.3,"th":"main","x":800.0,"y":500.0,"win":300.0,"left":650.0,"top":350.0,"ppp":1.0,"visible_before":false,"mon":"0,0 1920x1080"}
{"cat":"win","ev":"win_obs","t":5.31,"th":"main","age_ms":10,"ex":650.0,"ey":350.0,"ox":810.0,"oy":390.0,"ow":300.0,"oh":300.0,"dx":160.0,"dy":40.0,"ppp":1.0}
"#;

    #[test]
    fn a_window_placed_by_the_window_manager_is_recognized() {
        let r = analyze(&lines(CENTERED_BY_THE_WM));
        assert_eq!(r.bad_lines, 0);
        assert_eq!(r.findings.len(), 1, "{:?}", r.findings);
        let f = &r.findings[0];
        assert!(f.contains("asked to be at (650,350)"), "{f}");
        assert!(f.contains("reports it at (810,390)"), "{f}");
        assert!(f.contains("ignored the requested position"), "{f}");
        assert!(r.facts.iter().any(|f| f.contains("GNOME")));
        assert!(r.facts.iter().any(|f| f.contains("1 press(es), 1 pie")));
    }

    #[test]
    fn a_window_that_went_where_it_was_told_is_not_a_finding() {
        let ok = CENTERED_BY_THE_WM.replace(r#""ox":810.0,"oy":390.0"#, r#""ox":650.5,"oy":350.0"#);
        let r = analyze(&lines(&ok));
        assert!(r.findings.is_empty(), "{:?}", r.findings);
        assert!(render(&r).contains("none: every layer agrees"));
    }

    #[test]
    fn a_position_that_changes_between_the_layers_is_named() {
        let moved = CENTERED_BY_THE_WM.replace(
            r#""ev":"msg_show_pie","t":5.3,"th":"main","x":800.0,"y":500.0"#,
            r#""ev":"msg_show_pie","t":5.3,"th":"main","x":960.0,"y":540.0"#,
        );
        let r = analyze(&lines(&moved));
        assert!(
            r.findings
                .iter()
                .any(|f| f.contains("changed between the tap thread and the UI")),
            "{:?}",
            r.findings
        );
    }

    #[test]
    fn a_pointer_that_the_server_disagrees_with_is_named() {
        let off = CENTERED_BY_THE_WM.replace(r#""query_px":"800,500""#, r#""query_px":"1200,750""#);
        let r = analyze(&lines(&off));
        assert!(
            r.findings
                .iter()
                .any(|f| f.contains("the X server reports the pointer at (1200,750)")),
            "{:?}",
            r.findings
        );
    }

    #[test]
    fn scale_disagreement_panics_and_fallbacks_are_reported() {
        let mut log = CENTERED_BY_THE_WM.replace(r#""ppp":1.0}"#, r#""ppp":2.0}"#);
        log.push_str(
            r#"{"cat":"ui","ev":"cursor_or_default","t":6.0,"th":"main","ok":false,"x":400.0,"y":300.0}
{"cat":"panic","ev":"panic","t":7.0,"th":"main","at":"src/x.rs:1","message":"boom"}
{"cat":"ui","ev":"show_pie_ig"#,
        );
        let r = analyze(&lines(&log));
        let all = r.findings.join("\n");
        assert!(
            all.contains("scale 1 but the window toolkit uses 2"),
            "{all}"
        );
        assert!(all.contains("PANIC at src/x.rs:1: boom"), "{all}");
        assert!(all.contains("fallback (400,300) was used"), "{all}");
        assert_eq!(
            r.bad_lines, 1,
            "the cut-off last line is counted, not fatal"
        );
    }

    #[test]
    fn a_log_without_a_pie_says_so() {
        let r = analyze(&lines(r#"{"cat":"diag","ev":"start","version":"0.5.0"}"#));
        assert!(
            r.findings
                .iter()
                .any(|f| f.contains("no pie was ever shown"))
        );
    }

    #[test]
    fn the_newest_log_is_the_one_with_the_latest_name() {
        let names: Vec<String> = [
            "cursorpie-0000000100-4.jsonl",
            "cursorpie-0000000300-2.jsonl",
            "notes.txt",
            "cursorpie-0000000200-9.jsonl",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            newest(&names).map(String::as_str),
            Some("cursorpie-0000000300-2.jsonl")
        );
        assert_eq!(newest(&["notes.txt".to_string()]), None);
    }

    #[test]
    fn the_default_log_folder_follows_the_app() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            default_log_dir(&env(&[("CURSORPIE_DEBUG_DIR", "/tmp/dbg")])),
            Some(PathBuf::from("/tmp/dbg"))
        );
        if cfg!(not(any(windows, target_os = "macos"))) {
            assert_eq!(
                default_log_dir(&env(&[("HOME", "/home/u")])),
                Some(PathBuf::from("/home/u/.config/cursorpie/logs"))
            );
            assert_eq!(
                default_log_dir(&env(&[("HOME", "/home/u"), ("XDG_CONFIG_HOME", "/cfg")])),
                Some(PathBuf::from("/cfg/cursorpie/logs"))
            );
            assert_eq!(
                default_log_dir(&env(&[("HOME", "/home/u"), ("XDG_CONFIG_HOME", "rel")])),
                Some(PathBuf::from("/home/u/.config/cursorpie/logs")),
                "a relative XDG_CONFIG_HOME is ignored, as in the app"
            );
        }
    }
}
