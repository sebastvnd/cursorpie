//! `cargo tools log <view>`: other ways to read a debug log of the app than the verdict of
//! `cargo tools diag` (which says *where the chain broke*; these say *what happened*).
//!
//! | View | What it shows |
//! |---|---|
//! | `summary [FILE]` | how many lines, from which threads, how often each event, and the events that flood the log |
//! | `timeline [FILE]` | one row per press: how long the hold timer, the UI and the window took, where the pie was asked and seen |
//! | `grep PATTERN [FILE]` | the lines of one category or event (`ui`, `ui:show_pie`, `:press`), as readable text; no `jq` needed |
//! | `slow [--ms N] [FILE]` | the `diag::timed!` lines that took `N` ms or more, slowest first |
//! | `diff A B` | two logs side by side: findings that went away, findings that are new, events whose count changed |
//! | `follow [FILE]` | the log of a run that is still going, printed line by line as it is written |
//! | `report [FILE]` | the facts of `cargo tools info` and the verdict of `cargo tools diag`, as text for a bug report |
//!
//! Without a file, every view reads the newest log in the default log folder (see
//! [`crate::diag::pick_file`]). The views that print log lines write the name of the log on stderr,
//! so their output on stdout can be piped.
//!
//! The reading of a line is the one of `diag.rs`; the checks of [`extra_findings`] are added to
//! the verdict of `cargo tools diag`. Everything here that decides something is a pure function
//! of the lines, with a unit test; only [`run`] and [`follow`] touch the file system.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use crate::diag::{self, Rec, Val, flag, is, num, parse_line, text};

/// The pie should be on screen this long (ms) after the hold timer fired (or after the press, when
/// there was no hold timer). More means the UI thread or the tap thread was busy.
const SLOW_OPEN_MS: f64 = 300.0;
/// A `diag::timed!` step slower than this (ms) is reported by the verdict.
const SLOW_STEP_MS: f64 = 250.0;
/// An action that waited this long (ms) in the queue before the worker started it is reported.
const SLOW_WAIT_MS: f64 = 200.0;
/// An event written more often than this per second is said to flood the log.
const FLOOD_PER_SECOND: f64 = 50.0;
/// The default of `log slow --ms`.
const DEFAULT_SLOW_MS: u64 = 50;
/// How often `follow` looks for new lines.
const FOLLOW_POLL_MS: u64 = 200;

// ------------------------------------------------------------------------------------ options

/// What to show.
#[derive(Debug, PartialEq, Eq)]
pub enum View {
    Summary,
    Timeline,
    Grep(Filter),
    Slow { ms: u64 },
    Diff { a: PathBuf, b: PathBuf },
    Follow,
    Report,
}

/// A view and the log it reads (`None`: the newest one).
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub view: View,
    pub file: Option<PathBuf>,
}

/// Which lines `grep` keeps.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// Only this category.
    pub cat: Option<String>,
    /// Only this event.
    pub ev: Option<String>,
    /// Only lines whose field has this text (`--field x=800`).
    pub fields: Vec<(String, String)>,
    /// Only lines at or after this time since the start, in milliseconds (`--after`, given in s).
    pub after_ms: Option<u64>,
    /// Only lines at or before this time.
    pub before_ms: Option<u64>,
    /// At most this many lines.
    pub limit: Option<usize>,
    /// Print the lines as they are in the file, not as readable text.
    pub raw: bool,
}

/// Reads the arguments after `log`.
///
/// # Errors
/// On an unknown view or option, or a missing or wrong value.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let Some((view, rest)) = args.split_first() else {
        return Err(
            "log needs a view: summary, timeline, grep, slow, diff, follow or report".to_string(),
        );
    };
    match view.as_str() {
        "summary" => one_file(rest, View::Summary),
        "timeline" => one_file(rest, View::Timeline),
        "follow" => one_file(rest, View::Follow),
        "report" => one_file(rest, View::Report),
        "slow" => parse_slow(rest),
        "grep" => parse_grep(rest),
        "diff" => match rest {
            [a, b] => Ok(Options {
                view: View::Diff {
                    a: PathBuf::from(a),
                    b: PathBuf::from(b),
                },
                file: None,
            }),
            _ => Err("log diff needs two log files: the one before and the one after".to_string()),
        },
        other => Err(format!("unknown log view '{other}'")),
    }
}

/// A view that takes at most one argument, the log file.
fn one_file(rest: &[String], view: View) -> Result<Options, String> {
    match rest {
        [] => Ok(Options { view, file: None }),
        [file] if !file.starts_with("--") => Ok(Options {
            view,
            file: Some(PathBuf::from(file)),
        }),
        _ => Err("this view takes at most one argument: the log file".to_string()),
    }
}

fn parse_slow(rest: &[String]) -> Result<Options, String> {
    let mut ms = DEFAULT_SLOW_MS;
    let mut file = None;
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--ms" => {
                let n = args.next().ok_or("--ms needs a number of milliseconds")?;
                ms = n
                    .parse::<u64>()
                    .map_err(|_| format!("--ms: '{n}' is not a number of milliseconds"))?;
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown option '{other}' for log slow"));
            }
            other if file.is_none() => file = Some(PathBuf::from(other)),
            _ => return Err("log slow takes at most one log file".to_string()),
        }
    }
    Ok(Options {
        view: View::Slow { ms },
        file,
    })
}

fn parse_grep(rest: &[String]) -> Result<Options, String> {
    let mut filter = Filter::default();
    let mut positional: Vec<&String> = Vec::new();
    let mut args = rest.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--field" => {
                let pair = args.next().ok_or("--field needs name=value")?;
                let (k, v) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("--field: '{pair}' is not name=value"))?;
                filter.fields.push((k.to_string(), v.to_string()));
            }
            "--after" => {
                let s = args.next().ok_or("--after needs a time in seconds")?;
                filter.after_ms = Some(seconds_to_ms(s, "--after")?);
            }
            "--before" => {
                let s = args.next().ok_or("--before needs a time in seconds")?;
                filter.before_ms = Some(seconds_to_ms(s, "--before")?);
            }
            "--limit" => {
                let n = args.next().ok_or("--limit needs a number")?;
                filter.limit = Some(
                    n.parse::<usize>()
                        .map_err(|_| format!("--limit: '{n}' is not a number"))?,
                );
            }
            "--raw" => filter.raw = true,
            other if other.starts_with("--") => {
                return Err(format!("unknown option '{other}' for log grep"));
            }
            _ => positional.push(arg),
        }
    }
    let (pattern, file) = match positional.as_slice() {
        [pattern] => (pattern, None),
        [pattern, file] => (pattern, Some(PathBuf::from(file))),
        _ => {
            return Err(
                "log grep needs a pattern (`ui`, `ui:show_pie`, `:press` or `*`) and optionally a log file"
                    .to_string(),
            );
        }
    };
    let (cat, ev) = parse_pattern(pattern);
    filter.cat = cat;
    filter.ev = ev;
    Ok(Options {
        view: View::Grep(filter),
        file,
    })
}

/// `12.5` (seconds) as whole milliseconds.
fn seconds_to_ms(s: &str, option: &str) -> Result<u64, String> {
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| (v * 1000.0).round() as u64)
        .ok_or_else(|| format!("{option}: '{s}' is not a number of seconds"))
}

/// `ui` is a category, `ui:show_pie` a category and an event, `:press` an event of any category.
/// `*` or nothing means "any".
fn parse_pattern(pattern: &str) -> (Option<String>, Option<String>) {
    let part = |s: &str| (!s.is_empty() && s != "*").then(|| s.to_string());
    match pattern.split_once(':') {
        Some((cat, ev)) => (part(cat), part(ev)),
        None => (part(pattern), None),
    }
}

// ------------------------------------------------------------------------------------ reading

/// The records of a log in order, and how many lines could not be read.
pub fn read_records(lines: &[String]) -> (Vec<Rec>, usize) {
    let mut recs = Vec::new();
    let mut bad = 0;
    for line in lines.iter().filter(|l| !l.trim().is_empty()) {
        match parse_line(line) {
            Some(r) => recs.push(r),
            None => bad += 1,
        }
    }
    (recs, bad)
}

/// A value as short text. Whole numbers lose their `.0`; text with a space is quoted.
fn show(v: &Val) -> String {
    match v {
        Val::Num(n) => {
            let n = *n;
            if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{n:.0}")
            } else {
                format!("{}", (n * 1000.0).round() / 1000.0)
            }
        }
        Val::Str(s) if s.is_empty() || s.contains(' ') => format!("\"{s}\""),
        Val::Str(s) => s.clone(),
        Val::Bool(b) => b.to_string(),
        Val::Null => "null".to_string(),
    }
}

/// One line as readable text: `   5.300  main  ui.show_pie  left=650 top=350 ...`. The fields
/// come in alphabetical order, because the reader keeps them in a sorted map.
pub fn pretty(r: &Rec) -> String {
    let t = num(r, "t").unwrap_or(0.0);
    let thread = text(r, "th").unwrap_or("?");
    let cat = text(r, "cat").unwrap_or("?");
    let ev = text(r, "ev").unwrap_or("?");
    let mut out = format!("{t:>9.3}  {thread:<20} {cat}.{ev}");
    for (key, value) in r {
        if matches!(key.as_str(), "t" | "th" | "cat" | "ev") {
            continue;
        }
        out.push_str(&format!(" {key}={}", show(value)));
    }
    out
}

// ------------------------------------------------------------------------------------- grep

/// Whether a record passes the filter.
fn accepts(r: &Rec, f: &Filter) -> bool {
    if f.cat.as_deref().is_some_and(|c| text(r, "cat") != Some(c)) {
        return false;
    }
    if f.ev.as_deref().is_some_and(|e| text(r, "ev") != Some(e)) {
        return false;
    }
    for (key, want) in &f.fields {
        if r.get(key).map(show).as_deref() != Some(want.as_str()) {
            return false;
        }
    }
    let at_ms = num(r, "t").map(|t| (t * 1000.0).round().max(0.0) as u64);
    if f.after_ms.is_some_and(|a| at_ms.is_none_or(|t| t < a)) {
        return false;
    }
    if f.before_ms.is_some_and(|b| at_ms.is_none_or(|t| t > b)) {
        return false;
    }
    true
}

/// The lines that pass the filter, as readable text (or as they are with `raw`).
pub fn grep(lines: &[String], f: &Filter) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines.iter().filter(|l| !l.trim().is_empty()) {
        let Some(r) = parse_line(line) else { continue };
        if !accepts(&r, f) {
            continue;
        }
        out.push(if f.raw {
            line.trim().to_string()
        } else {
            pretty(&r)
        });
        if f.limit.is_some_and(|n| out.len() >= n) {
            break;
        }
    }
    out
}

// ----------------------------------------------------------------------------------- summary

/// What is known of one event.
struct EventStat {
    count: usize,
    first: f64,
    last: f64,
}

/// How many lines, from which threads, and how often each event was written.
pub fn summary(lines: &[String]) -> String {
    let (recs, bad) = read_records(lines);
    let mut threads: BTreeMap<String, usize> = BTreeMap::new();
    let mut events: BTreeMap<String, EventStat> = BTreeMap::new();
    let mut end = 0.0_f64;
    for r in &recs {
        let t = num(r, "t").unwrap_or(0.0);
        end = end.max(t);
        *threads
            .entry(text(r, "th").unwrap_or("?").to_string())
            .or_default() += 1;
        let key = format!(
            "{}.{}",
            text(r, "cat").unwrap_or("?"),
            text(r, "ev").unwrap_or("?")
        );
        let stat = events.entry(key).or_insert(EventStat {
            count: 0,
            first: t,
            last: t,
        });
        stat.count += 1;
        stat.first = stat.first.min(t);
        stat.last = stat.last.max(t);
    }

    let mut out = format!("{} line(s) over {end:.1} s", recs.len());
    if bad > 0 {
        out.push_str(&format!(", {bad} unreadable"));
    }
    out.push_str(&format!(", {} thread(s)\n\nThreads\n", threads.len()));
    for (name, count) in &threads {
        out.push_str(&format!("  {name}: {count}\n"));
    }

    let mut rows: Vec<(&String, &EventStat)> = events.iter().collect();
    rows.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
    let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    out.push_str("\nEvents (count, first and last time in seconds)\n");
    for (name, stat) in rows {
        out.push_str(&format!(
            "  {name:<width$} {:>6}  {:>9.3} -> {:>9.3}",
            stat.count, stat.first, stat.last
        ));
        let span = (stat.last - stat.first).max(1.0);
        if stat.count > 20 && stat.count as f64 / span > FLOOD_PER_SECOND {
            out.push_str(
                "   <- floods the log: write it with diag::trace! so it is off by default",
            );
        }
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------------- timeline

/// One press and what followed it, until the next press.
#[derive(Debug, Default, PartialEq)]
pub struct Gesture {
    /// When the press was logged (seconds since the start).
    pub press_t: f64,
    /// Where the press was.
    pub press_at: Option<(f64, f64)>,
    /// When the hold timer opened the pie.
    pub hold_t: Option<f64>,
    /// When the UI showed the pie.
    pub show_t: Option<f64>,
    /// When the button was released.
    pub release_t: Option<f64>,
    /// Where the pie window was asked to be (top-left).
    pub asked: Option<(f64, f64)>,
    /// Where the toolkit last reported the window.
    pub seen: Option<(f64, f64)>,
    /// How long after the request that was seen, in ms.
    pub seen_age_ms: Option<f64>,
    /// The window was Settings or Welcome, which are centered on purpose.
    pub card: bool,
}

/// Milliseconds between two times of the log. The times have a resolution of one millisecond, so
/// the result is rounded: `5.3 - 5.0` is not `300.00000000000006`.
fn ms_between(from: f64, to: f64) -> f64 {
    ((to - from) * 1000.0).round()
}

impl Gesture {
    /// Press to the hold timer.
    pub fn hold_ms(&self) -> Option<f64> {
        self.hold_t.map(|h| ms_between(self.press_t, h))
    }

    /// Press to the pie shown.
    pub fn open_ms(&self) -> Option<f64> {
        self.show_t.map(|s| ms_between(self.press_t, s))
    }

    /// From the moment the tap thread decided to open the pie (the hold timer, or the press
    /// itself) to the pie shown by the UI: the time the app needed, without the hold delay the
    /// person chose.
    pub fn lag_ms(&self) -> Option<f64> {
        let from = self.hold_t.unwrap_or(self.press_t);
        self.show_t.map(|s| ms_between(from, s))
    }

    /// Press to release.
    pub fn held_ms(&self) -> Option<f64> {
        self.release_t.map(|r| ms_between(self.press_t, r))
    }

    /// The last column of the timeline: where the window was asked and seen.
    fn window_note(&self) -> String {
        if self.card {
            return "Settings/Welcome card (centered on purpose)".to_string();
        }
        match (self.asked, self.seen) {
            (Some(asked), Some(seen)) => format!(
                "{} -> {} after {} ms",
                place(Some(asked)),
                place(Some(seen)),
                ms(self.seen_age_ms)
            ),
            (Some(asked), None) => format!("{} -> never seen", place(Some(asked))),
            (None, _) => "no pie".to_string(),
        }
    }
}

/// A time in ms as a whole number, or `-`.
fn ms(v: Option<f64>) -> String {
    v.map_or_else(|| "-".to_string(), |v| format!("{v:.0}"))
}

/// A point as `(x,y)`, or `-`.
fn place(p: Option<(f64, f64)>) -> String {
    p.map_or_else(|| "-".to_string(), |(x, y)| format!("({x:.0},{y:.0})"))
}

/// The presses of a log and what followed each. A press is the raw `tap.press` where the backend
/// logs one (Linux), else `tap.on_press`, which every backend writes.
pub fn gestures(recs: &[Rec]) -> Vec<Gesture> {
    let raw = recs.iter().any(|r| is(r, "tap", "press"));
    let (press_ev, release_ev) = if raw {
        ("press", "release")
    } else {
        ("on_press", "on_release")
    };
    let mut out: Vec<Gesture> = Vec::new();
    for r in recs {
        let t = num(r, "t").unwrap_or(0.0);
        let cat = text(r, "cat").unwrap_or("");
        let ev = text(r, "ev").unwrap_or("");
        if cat == "tap" && ev == press_ev {
            out.push(Gesture {
                press_t: t,
                press_at: num(r, "x").zip(num(r, "y")),
                ..Gesture::default()
            });
            continue;
        }
        let Some(g) = out.last_mut() else { continue };
        match (cat, ev) {
            ("tap", "hold_open") if g.hold_t.is_none() => g.hold_t = Some(t),
            ("tap", e) if e == release_ev && g.release_t.is_none() => g.release_t = Some(t),
            ("ui", "place_card") => g.card = true,
            ("ui", "show_pie") if g.show_t.is_none() => {
                g.show_t = Some(t);
                g.asked = num(r, "left").zip(num(r, "top"));
            }
            ("win", "win_obs") if g.show_t.is_some() && !g.card => {
                if let (Some(x), Some(y)) = (num(r, "ox"), num(r, "oy")) {
                    g.seen = Some((x, y));
                    g.seen_age_ms = num(r, "age_ms");
                }
            }
            _ => {}
        }
    }
    out
}

/// One row per press, with the times of each step.
pub fn timeline(lines: &[String]) -> String {
    let (recs, _) = read_records(lines);
    let list = gestures(&recs);
    if list.is_empty() {
        return "no press in this log: make the gesture while the log is on\n".to_string();
    }
    let mut out = String::from(
        "   #    at (s)  pointer         hold  ui lag    open    held  window asked -> seen\n",
    );
    for (i, g) in list.iter().enumerate() {
        out.push_str(&format!(
            "{:>4} {:>9.3}  {:<12} {:>6} {:>7} {:>7} {:>7}  {}\n",
            i + 1,
            g.press_t,
            place(g.press_at),
            ms(g.hold_ms()),
            ms(g.lag_ms()),
            ms(g.open_ms()),
            ms(g.held_ms()),
            g.window_note()
        ));
    }
    out.push_str(
        "\nTimes in ms. hold: press to the hold timer. ui lag: from the hold timer (or the press) \
         to the pie shown by the UI. open: press to the pie shown. held: press to release.\n",
    );
    out
}

// ------------------------------------------------------------------------------------- slow

/// The lines that carry `ms` (the ones `diag::timed!` writes) of at least `min_ms`, slowest first.
pub fn slow(lines: &[String], min_ms: u64) -> Vec<String> {
    let (recs, _) = read_records(lines);
    let mut found: Vec<(f64, String)> = recs
        .iter()
        .filter_map(|r| {
            let took = num(r, "ms")?;
            (took >= min_ms as f64).then(|| (took, pretty(r)))
        })
        .collect();
    found.sort_by(|a, b| b.0.total_cmp(&a.0));
    found.into_iter().map(|(_, line)| line).collect()
}

// ------------------------------------------------------------------------ findings for diag

/// Checks that need more than the position chain of `cargo tools diag`: slow openings, slow
/// steps, a config that could not be saved, an action that waited too long. Called by
/// [`diag::analyze`], so they appear in its verdict.
pub fn extra_findings(recs: &[Rec]) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(program_findings(recs));

    for (i, g) in gestures(recs).iter().enumerate() {
        if let Some(lag) = g.lag_ms()
            && lag > SLOW_OPEN_MS
        {
            let from = if g.hold_t.is_some() {
                "the hold timer fired"
            } else {
                "the press"
            };
            out.push(format!(
                "gesture #{}: the pie was shown {lag:.0} ms after {from} (more than {SLOW_OPEN_MS:.0} ms): the UI thread or the tap thread was busy (see `cargo tools log timeline`)",
                i + 1
            ));
        }
    }

    // steps timed with diag::timed!, except actions (a sequence legitimately takes a while)
    let mut slow_steps: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for r in recs {
        let (Some(cat), Some(ev), Some(took)) = (text(r, "cat"), text(r, "ev"), num(r, "ms"))
        else {
            continue;
        };
        if cat == "act" || took <= SLOW_STEP_MS {
            continue;
        }
        let entry = slow_steps.entry(format!("{cat}.{ev}")).or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 = entry.1.max(took);
    }
    for (name, (count, slowest)) in &slow_steps {
        out.push(format!(
            "{name} took more than {SLOW_STEP_MS:.0} ms {count} time(s) (slowest {slowest:.0} ms)"
        ));
    }

    for r in recs {
        if is(r, "cfg", "save") && flag(r, "ok") == Some(false) {
            out.push(format!(
                "saving the config failed ({}, {} save): the change stayed in memory and is lost when the app ends",
                text(r, "error").unwrap_or("no reason given"),
                text(r, "how").unwrap_or("?")
            ));
        } else if is(r, "cfg", "load") && text(r, "outcome") == Some("broken") {
            out.push(
                "config.json could not be read as a CursorPie config: the defaults were used and a copy was kept as config.broken.json"
                    .to_string(),
            );
        } else if is(r, "act", "exec")
            && let Some(wait) = num(r, "wait_ms")
            && wait > SLOW_WAIT_MS
        {
            out.push(format!(
                "a '{}' action waited {wait:.0} ms in the queue before the worker started it: an earlier action was still running",
                text(r, "kind").unwrap_or("?")
            ));
        }
    }
    out
}

// -------------------------------------------------------------------------------------- diff

/// Findings about the programs an action starts and about the input hook: a program that could
/// not be started, a `ShellExecute` that failed, a program that ended with an error code, and a
/// hook whose last state is not `running`. Part of [`extra_findings`].
fn program_findings(recs: &[Rec]) -> Vec<String> {
    let mut out = Vec::new();
    for r in recs {
        if is(r, "act", "spawn_failed") {
            out.push(format!(
                "the program '{}' could not be started ({}): the action did nothing",
                text(r, "program").unwrap_or("?"),
                text(r, "error").unwrap_or("no reason given")
            ));
        } else if is(r, "act", "shell_execute") && flag(r, "ok") == Some(false) {
            out.push(format!(
                "Windows could not open what an action asked for (ShellExecute code {:.0}; 2 and 3 mean not found, 5 access denied)",
                num(r, "code").unwrap_or(0.0)
            ));
        } else if is(r, "act", "exited")
            && let Some(code) = num(r, "code")
            && code != 0.0
        {
            out.push(format!(
                "the program '{}' ended with exit code {code:.0}: for `open` and `xdg-open` that usually means it did not find what it was asked to open",
                text(r, "program").unwrap_or("?")
            ));
        }
    }

    // the input hook: its last state must be "running", or no gesture ever reaches the app
    let mut hook: Option<(&str, &str)> = None;
    for r in recs {
        if is(r, "tap", "hook")
            && let Some(h) = text(r, "backend").zip(text(r, "state"))
        {
            hook = Some(h);
        }
    }
    if let Some((backend, state)) = hook
        && state != "running"
    {
        let why = match state {
            "blocked" => "macOS has not granted Accessibility / Input Monitoring to the app",
            "refused" => "Windows refused the low-level input hooks",
            "no_x_server" => "the X server cannot be reached (is DISPLAY set?)",
            "grab_failed" => "another program holds a grab of the trigger button",
            _ => "the hook ended",
        };
        out.push(format!(
            "the input hook ({backend}) ended in the state '{state}': {why}; no gesture reaches the app until it is running again"
        ));
    }
    out
}

/// A message with every run of digits replaced by `#`, so that two findings that differ only in
/// their numbers (the positions of another run) count as the same finding.
fn shape(message: &str) -> String {
    let mut out = String::new();
    let mut in_digits = false;
    for c in message.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}

/// How often each `category.event` was written.
fn event_counts(recs: &[Rec]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for r in recs {
        let key = format!(
            "{}.{}",
            text(r, "cat").unwrap_or("?"),
            text(r, "ev").unwrap_or("?")
        );
        *counts.entry(key).or_default() += 1;
    }
    counts
}

/// Two logs compared: what the verdict of `cargo tools diag` says of each, and which events
/// changed in number. Use it to see whether a fix worked: `a` is the log from before, `b` from after.
pub fn compare(name_a: &str, a: &[String], name_b: &str, b: &[String]) -> String {
    let (recs_a, _) = read_records(a);
    let (recs_b, _) = read_records(b);
    let report_a = diag::analyze(a);
    let report_b = diag::analyze(b);
    let shapes_a: BTreeSet<String> = report_a.findings.iter().map(|f| shape(f)).collect();
    let shapes_b: BTreeSet<String> = report_b.findings.iter().map(|f| shape(f)).collect();

    let mut out = format!("A (before): {name_a}\nB (after):  {name_b}\n");
    let gone: Vec<&String> = report_a
        .findings
        .iter()
        .filter(|f| !shapes_b.contains(&shape(f)))
        .collect();
    let new: Vec<&String> = report_b
        .findings
        .iter()
        .filter(|f| !shapes_a.contains(&shape(f)))
        .collect();
    let same = report_b
        .findings
        .iter()
        .filter(|f| shapes_a.contains(&shape(f)))
        .count();

    out.push_str("\nFindings only in A (gone in B)\n");
    push_list(&mut out, &gone);
    out.push_str("\nFindings only in B (new)\n");
    push_list(&mut out, &new);
    out.push_str(&format!("\nFindings in both: {same}\n"));

    let counts_a = event_counts(&recs_a);
    let counts_b = event_counts(&recs_b);
    let names: BTreeSet<&String> = counts_a.keys().chain(counts_b.keys()).collect();
    let mut changed = String::new();
    for name in names {
        let (na, nb) = (
            counts_a.get(name).copied().unwrap_or(0),
            counts_b.get(name).copied().unwrap_or(0),
        );
        if na != nb {
            changed.push_str(&format!("  {name}: {na} -> {nb}\n"));
        }
    }
    out.push_str("\nEvents whose count changed\n");
    if changed.is_empty() {
        out.push_str("  none\n");
    } else {
        out.push_str(&changed);
    }
    out
}

fn push_list(out: &mut String, items: &[&String]) {
    if items.is_empty() {
        out.push_str("  none\n");
    }
    for item in items {
        out.push_str(&format!("  - {item}\n"));
    }
}

// ------------------------------------------------------------------------------------ running

/// The lines of a file. A log cut in the middle of a character (a crash) is read as far as it goes.
fn read_lines(path: &Path) -> Result<Vec<String>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let body = String::from_utf8_lossy(&bytes);
    Ok(body.lines().map(str::to_string).collect())
}

/// The file name of a path, for output that is pasted somewhere (the folders hold a user name).
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Facts and verdict as text for a bug report: the block of `cargo tools info`, then the
/// verdict of `cargo tools diag`.
pub fn report(root: &Path, path: &Path, lines: &[String]) -> String {
    format!(
        "{}\nLog `{}` analyzed by `cargo tools diag` (the log holds positions, monitor layout and a few\nenvironment variables; no file names and no typed text):\n\n```text\n{}```\n",
        crate::info::render(&crate::info::gather(root)),
        file_name(path),
        diag::render(&diag::analyze(lines))
    )
}

/// Prints the lines of `path` that are written after this call, until the program is stopped.
/// Every line is printed as readable text. Never returns on its own.
fn follow(path: &Path) -> Result<ExitCode, String> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    eprintln!("following {} (Ctrl-C to stop)", path.display());
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk: Vec<u8> = Vec::new();
    loop {
        chunk.clear();
        file.read_to_end(&mut chunk)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        pending.extend_from_slice(&chunk);
        // only complete lines are printed; the rest waits for the next look
        while let Some(end) = pending.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&raw);
            match parse_line(&line) {
                Some(r) => println!("{}", pretty(&r)),
                None => println!("(unreadable) {}", line.trim_end()),
            }
        }
        std::thread::sleep(Duration::from_millis(FOLLOW_POLL_MS));
    }
}

/// Runs a view.
pub fn run(root: &Path, options: &Options) -> Result<ExitCode, String> {
    if let View::Diff { a, b } = &options.view {
        let (lines_a, lines_b) = (read_lines(a)?, read_lines(b)?);
        print!(
            "{}",
            compare(&file_name(a), &lines_a, &file_name(b), &lines_b)
        );
        return Ok(ExitCode::SUCCESS);
    }
    let path = diag::pick_file(options.file.as_deref())?;
    if matches!(options.view, View::Follow) {
        return follow(&path);
    }
    let lines = read_lines(&path)?;
    eprintln!("Log: {}", path.display());
    let out = match &options.view {
        View::Summary => summary(&lines),
        View::Timeline => timeline(&lines),
        View::Grep(filter) => {
            let found = grep(&lines, filter);
            if found.is_empty() {
                "no line matches\n".to_string()
            } else {
                found.iter().map(|l| format!("{l}\n")).collect()
            }
        }
        View::Slow { ms } => {
            let found = slow(&lines, *ms);
            if found.is_empty() {
                format!("no step took {ms} ms or more\n")
            } else {
                found.iter().map(|l| format!("{l}\n")).collect()
            }
        }
        View::Report => report(root, &path, &lines),
        // handled before the log was read
        View::Diff { .. } | View::Follow => String::new(),
    };
    print!("{out}");
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    /// Two presses: the first opens a pie that the window manager moved, the second is a
    /// side-button press that opens nothing.
    const SESSION: &str = r#"
{"cat":"diag","ev":"start","t":0.0,"th":"main","version":"0.5.3"}
{"cat":"tap","ev":"press","t":5.0,"th":"x11-tap","detail":1,"x":800.0,"y":500.0}
{"cat":"tap","ev":"hold_open","t":5.25,"th":"timer","x":800.0,"y":500.0}
{"cat":"ui","ev":"msg_show_pie","t":5.3,"th":"main","x":800.0,"y":500.0}
{"cat":"ui","ev":"show_pie","t":5.3,"th":"main","left":650.0,"top":350.0,"mon":"0,0 1920x1080"}
{"cat":"win","ev":"win_obs","t":5.31,"th":"main","age_ms":10,"ox":810.0,"oy":390.0,"ow":300.0,"oh":300.0}
{"cat":"tap","ev":"release","t":6.2,"th":"x11-tap","detail":1,"x":820.0,"y":510.0}
{"cat":"tap","ev":"press","t":9.0,"th":"x11-tap","detail":8,"x":100.0,"y":100.0}
{"cat":"tap","ev":"release","t":9.1,"th":"x11-tap","detail":8,"x":100.0,"y":100.0}
"#;

    #[test]
    fn views_and_their_arguments_are_read() {
        assert_eq!(
            parse_args(&args(&["summary"])),
            Ok(Options {
                view: View::Summary,
                file: None
            })
        );
        assert_eq!(
            parse_args(&args(&["timeline", "a.jsonl"])),
            Ok(Options {
                view: View::Timeline,
                file: Some("a.jsonl".into())
            })
        );
        assert_eq!(
            parse_args(&args(&["slow", "--ms", "120"])),
            Ok(Options {
                view: View::Slow { ms: 120 },
                file: None
            })
        );
        assert_eq!(
            parse_args(&args(&["diff", "a", "b"])),
            Ok(Options {
                view: View::Diff {
                    a: "a".into(),
                    b: "b".into()
                },
                file: None
            })
        );
        let Ok(Options {
            view: View::Grep(f),
            file,
        }) = parse_args(&args(&[
            "grep",
            "ui:show_pie",
            "--field",
            "x=800",
            "--after",
            "1.5",
            "--limit",
            "3",
            "--raw",
            "x.jsonl",
        ]))
        else {
            panic!("grep was not read");
        };
        assert_eq!(f.cat.as_deref(), Some("ui"));
        assert_eq!(f.ev.as_deref(), Some("show_pie"));
        assert_eq!(f.fields, vec![("x".to_string(), "800".to_string())]);
        assert_eq!(
            (f.after_ms, f.before_ms, f.limit, f.raw),
            (Some(1500), None, Some(3), true)
        );
        assert_eq!(file, Some(PathBuf::from("x.jsonl")));
    }

    #[test]
    fn wrong_arguments_are_errors() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&args(&["watch"])).is_err());
        assert!(parse_args(&args(&["summary", "a", "b"])).is_err());
        assert!(parse_args(&args(&["summary", "--fast"])).is_err());
        assert!(parse_args(&args(&["slow", "--ms"])).is_err());
        assert!(parse_args(&args(&["slow", "--ms", "x"])).is_err());
        assert!(parse_args(&args(&["diff", "a"])).is_err());
        assert!(parse_args(&args(&["grep"])).is_err());
        assert!(parse_args(&args(&["grep", "ui", "--field", "x"])).is_err());
        assert!(parse_args(&args(&["grep", "ui", "--after", "soon"])).is_err());
        assert!(parse_args(&args(&["grep", "ui", "--sort"])).is_err());
    }

    #[test]
    fn patterns_name_a_category_an_event_or_both() {
        assert_eq!(parse_pattern("ui"), (Some("ui".into()), None));
        assert_eq!(
            parse_pattern("ui:show_pie"),
            (Some("ui".into()), Some("show_pie".into()))
        );
        assert_eq!(parse_pattern(":press"), (None, Some("press".into())));
        assert_eq!(parse_pattern("*"), (None, None));
        assert_eq!(parse_pattern("tap:*"), (Some("tap".into()), None));
    }

    #[test]
    fn grep_keeps_the_lines_that_match() {
        let log = lines(SESSION);
        let tap = Filter {
            cat: Some("tap".into()),
            ..Filter::default()
        };
        assert_eq!(grep(&log, &tap).len(), 5);

        let press = Filter {
            ev: Some("press".into()),
            fields: vec![("detail".into(), "8".into())],
            ..Filter::default()
        };
        let found = grep(&log, &press);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("tap.press"), "{}", found[0]);
        assert!(found[0].contains("x=100"), "{}", found[0]);

        let late = Filter {
            after_ms: Some(6000),
            limit: Some(1),
            raw: true,
            ..Filter::default()
        };
        let found = grep(&log, &late);
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with('{'), "raw lines stay as written");
        assert!(found[0].contains("\"t\":6.2"), "{}", found[0]);
    }

    #[test]
    fn a_line_is_printed_as_readable_text() {
        let r = parse_line(
            r#"{"t":5.3,"th":"main","cat":"ui","ev":"show_pie","x":800.0,"ppp":1.25,"mon":"0,0 1920x1080","ok":true}"#,
        )
        .unwrap();
        let shown = pretty(&r);
        assert!(shown.contains("ui.show_pie"), "{shown}");
        assert!(
            shown.contains("x=800"),
            "whole numbers lose their .0: {shown}"
        );
        assert!(shown.contains("ppp=1.25"), "{shown}");
        assert!(shown.contains("mon=\"0,0 1920x1080\""), "{shown}");
        assert!(shown.contains("ok=true"), "{shown}");
        assert!(
            !shown.contains("th="),
            "the fixed fields are not repeated: {shown}"
        );
    }

    #[test]
    fn the_summary_counts_and_names_a_flood() {
        let mut log = lines(SESSION);
        for i in 0..400 {
            log.push(format!(
                r#"{{"cat":"tap","ev":"drag","t":{:.3},"th":"x11-tap","x":1.0,"y":2.0}}"#,
                10.0 + f64::from(i) * 0.001
            ));
        }
        log.push("{cut".to_string());
        let out = summary(&log);
        assert!(out.contains("1 unreadable"), "{out}");
        assert!(out.contains("tap.drag"), "{out}");
        assert!(out.contains("floods the log"), "{out}");
        assert!(out.contains("x11-tap: 404"), "{out}");
        let drag = out.lines().find(|l| l.contains("tap.drag")).unwrap();
        assert!(drag.contains("400"), "{drag}");
        let press = out.lines().find(|l| l.contains("tap.press")).unwrap();
        assert!(!press.contains("floods"), "{press}");
    }

    #[test]
    fn the_timeline_has_one_row_per_press() {
        let (recs, _) = read_records(&lines(SESSION));
        let list = gestures(&recs);
        assert_eq!(list.len(), 2);
        let first = &list[0];
        assert_eq!(first.hold_ms(), Some(250.0));
        assert_eq!(first.lag_ms(), Some(50.0), "from the hold timer to the pie");
        assert_eq!(first.open_ms(), Some(300.0));
        assert_eq!(first.held_ms(), Some(1200.0));
        assert_eq!(first.asked, Some((650.0, 350.0)));
        assert_eq!(first.seen, Some((810.0, 390.0)));
        assert_eq!(list[1].open_ms(), None);

        let out = timeline(&lines(SESSION));
        assert!(out.contains("(800,500)"), "{out}");
        assert!(out.contains("(650,350) -> (810,390) after 10 ms"), "{out}");
        assert!(out.contains("no pie"), "{out}");
        assert!(timeline(&[]).contains("no press"));
    }

    #[test]
    fn a_log_without_the_raw_press_uses_the_one_every_backend_writes() {
        let log = lines(
            r#"
{"cat":"tap","ev":"on_press","t":1.0,"x":10.0,"y":20.0,"button":3,"profile":0}
{"cat":"ui","ev":"show_pie","t":1.1,"left":0.0,"top":0.0}
{"cat":"tap","ev":"on_release","t":2.0,"x":10.0,"y":20.0}
"#,
        );
        let (recs, _) = read_records(&log);
        let list = gestures(&recs);
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].lag_ms(),
            Some(100.0),
            "no hold timer: from the press"
        );
        assert_eq!(list[0].held_ms(), Some(1000.0));
    }

    #[test]
    fn a_settings_card_is_not_a_pie() {
        let log = lines(
            r#"
{"cat":"tap","ev":"press","t":1.0,"x":10.0,"y":20.0}
{"cat":"ui","ev":"place_card","t":1.1}
{"cat":"win","ev":"win_obs","t":1.2,"age_ms":5,"ox":1.0,"oy":2.0}
"#,
        );
        let out = timeline(&log);
        assert!(out.contains("centered on purpose"), "{out}");
    }

    #[test]
    fn slow_steps_are_listed_slowest_first() {
        let log = lines(
            r#"
{"cat":"ui","ev":"load_icons","t":1.0,"th":"main","count":12,"ms":412.5}
{"cat":"ui","ev":"load_icons","t":2.0,"th":"main","count":3,"ms":9.0}
{"cat":"cfg","ev":"load","t":0.5,"th":"main","outcome":"ok","ms":80.0}
"#,
        );
        let found = slow(&log, 50);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].contains("ui.load_icons"),
            "the slowest is first: {found:?}"
        );
        assert!(found[1].contains("cfg.load"), "{found:?}");
        assert!(slow(&log, 1000).is_empty());
    }

    #[test]
    fn the_findings_of_the_new_events_are_added_to_the_verdict() {
        let log = r#"
{"cat":"tap","ev":"press","t":1.0,"x":10.0,"y":20.0}
{"cat":"tap","ev":"hold_open","t":1.2,"x":10.0,"y":20.0}
{"cat":"ui","ev":"show_pie","t":1.9,"left":0.0,"top":0.0,"mon":"0,0 100x100"}
{"cat":"ui","ev":"load_icons","t":2.0,"th":"main","ms":900.0}
{"cat":"cfg","ev":"save","t":3.0,"ok":false,"how":"backup","bytes":2048,"error":"PermissionDenied"}
{"cat":"cfg","ev":"load","t":3.1,"outcome":"broken","ms":1.0}
{"cat":"act","ev":"exec","t":4.0,"kind":"command","wait_ms":450.0,"ms":900.0}
"#;
        let (recs, _) = read_records(&lines(log));
        let all = extra_findings(&recs).join("\n");
        assert!(
            all.contains("gesture #1: the pie was shown 700 ms after the hold timer fired"),
            "{all}"
        );
        assert!(
            all.contains("ui.load_icons took more than 250 ms 1 time(s) (slowest 900 ms)"),
            "{all}"
        );
        assert!(
            all.contains("saving the config failed (PermissionDenied, backup save)"),
            "{all}"
        );
        assert!(all.contains("config.broken.json"), "{all}");
        assert!(all.contains("a 'command' action waited 450 ms"), "{all}");
        assert!(
            !all.contains("act.exec took"),
            "actions may take long: {all}"
        );
        // and the verdict of `diag` carries them
        let verdict = diag::analyze(&lines(log));
        assert!(
            verdict.findings.iter().any(|f| f.contains("waited 450 ms")),
            "{:?}",
            verdict.findings
        );
    }

    #[test]
    fn a_healthy_log_adds_no_findings() {
        let (recs, _) = read_records(&lines(SESSION));
        assert!(
            extra_findings(&recs).is_empty(),
            "{:?}",
            extra_findings(&recs)
        );
    }

    #[test]
    fn findings_that_differ_only_in_numbers_are_the_same_finding() {
        assert_eq!(
            shape("pie #1: asked (650,350) reports (810,390) after 10 ms"),
            shape("pie #2: asked (700,400) reports (1200,40) after 7 ms")
        );
        assert_ne!(shape("a 1"), shape("b 1"));
    }

    #[test]
    fn two_logs_are_compared() {
        let before = lines(
            r#"
{"cat":"tap","ev":"press","t":5.0,"x":800.0,"y":500.0,"scale":1.0,"query_px":"800,500"}
{"cat":"ui","ev":"show_pie","t":5.3,"left":650.0,"top":350.0,"mon":"0,0 1920x1080"}
{"cat":"win","ev":"win_obs","t":5.31,"age_ms":10,"ox":810.0,"oy":390.0,"ow":300.0,"oh":300.0}
"#,
        );
        let after = lines(
            r#"
{"cat":"tap","ev":"press","t":5.0,"x":800.0,"y":500.0,"scale":1.0,"query_px":"800,500"}
{"cat":"ui","ev":"show_pie","t":5.3,"left":650.0,"top":350.0,"mon":"0,0 1920x1080"}
{"cat":"win","ev":"win_obs","t":5.31,"age_ms":10,"ox":650.0,"oy":350.0,"ow":300.0,"oh":300.0}
{"cat":"tap","ev":"press","t":8.0,"x":100.0,"y":100.0,"scale":1.0,"query_px":"100,100"}
"#,
        );
        let out = compare("before.jsonl", &before, "after.jsonl", &after);
        assert!(out.contains("A (before): before.jsonl"), "{out}");
        let (only_a, rest) = out.split_once("Findings only in B").unwrap();
        assert!(
            only_a.contains("the window was asked to be at"),
            "the old finding is gone: {out}"
        );
        assert!(rest.contains("Findings in both: 0"), "{out}");
        assert!(rest.contains("tap.press: 1 -> 2"), "{out}");
        assert!(
            !rest.contains("ui.show_pie:"),
            "unchanged events are not listed: {out}"
        );
    }

    #[test]
    fn a_log_cut_in_the_middle_of_a_character_is_still_read() {
        let dir = std::env::temp_dir().join(format!("cursorpie-logview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("cursorpie-0000000001-1.jsonl");
        let mut bytes =
            b"{\"cat\":\"ui\",\"ev\":\"a\"}\n{\"cat\":\"ui\",\"ev\":\"b\",\"n\":\"".to_vec();
        bytes.push(0xC3); // half of a two-byte character, as a crash can leave it
        std::fs::write(&file, bytes).unwrap();
        let read = read_lines(&file).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(read.len(), 2);
        assert_eq!(
            read_records(&read).1,
            1,
            "the cut line is counted, not fatal"
        );
    }

    #[test]
    fn programs_that_fail_and_a_hook_that_is_down_are_findings() {
        let log = r#"
{"cat":"tap","ev":"hook","t":0.1,"backend":"macos","state":"blocked"}
{"cat":"act","ev":"spawn_failed","t":1.0,"program":"xdg-open","error":"NotFound"}
{"cat":"act","ev":"shell_execute","t":2.0,"ok":false,"code":2}
{"cat":"act","ev":"shell_execute","t":2.5,"ok":true,"code":42}
{"cat":"act","ev":"exited","t":3.0,"program":"open","code":1}
{"cat":"act","ev":"exited","t":3.5,"program":"open","code":0}
"#;
        let (recs, _) = read_records(&lines(log));
        let all = extra_findings(&recs).join("\n");
        assert!(
            all.contains("the program 'xdg-open' could not be started (NotFound)"),
            "{all}"
        );
        assert!(all.contains("ShellExecute code 2;"), "{all}");
        assert!(
            !all.contains("code 42"),
            "a success is not a finding: {all}"
        );
        assert!(
            all.contains("the program 'open' ended with exit code 1"),
            "{all}"
        );
        assert!(!all.contains("exit code 0"), "{all}");
        assert!(
            all.contains(
                "the input hook (macos) ended in the state 'blocked': macOS has not granted"
            ),
            "{all}"
        );

        // a hook that came up after a failed try is fine
        let up = format!(
            "{log}{}",
            r#"{"cat":"tap","ev":"hook","t":9.0,"backend":"macos","state":"running"}"#
        );
        let (recs, _) = read_records(&lines(&up));
        assert!(!extra_findings(&recs).join("\n").contains("input hook"));
    }
}
