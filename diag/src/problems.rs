//! Reads a log back and says what went wrong in it: the part of the crate that the *Developer*
//! panel of the app and the `cpd` command share, so both name the same problems.
//!
//! It is part of the builds that have the log (debug builds and the feature `force`), like the
//! writer; a release build without the log has none of it.
//!
//! The rules are deliberately about the *shape* of an event, not about what an app calls it, so
//! they hold for any program that uses this crate:
//!
//! | A line that has | Is |
//! |---|---|
//! | category `panic` | an **error** |
//! | `ok = false` | an **error**: `<cat>.<event> failed` |
//! | an event name with `failed`, `refused`, `denied` or `no_space` in it | an **error** |
//! | `state` of `refused`, `failed` or `denied` | an **error**: `<cat>.<event> <state>` |
//! | an event name with `blocked`, `unavailable`, `unreachable`, `giveup` or `no_pie` in it | a **warning** |
//! | `ms` of 250 or more (a [`timed!`](crate::timed) step) | a **warning**: slow |
//!
//! A problem that repeats is listed once with its count and the time of its first line.
//!
//! The reader accepts what [`format_line`](crate::format_line) writes (one flat object per line)
//! and skips a line that is not one, such as the half-written last line after a crash.

use std::path::{Path, PathBuf};

/// How serious a problem is. Orders from the worst to the mildest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Something did not work.
    Error,
    /// Something worked badly, or was refused.
    Warning,
}

impl Severity {
    /// `error` or `warning`.
    pub fn word(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One value of a line.
#[derive(Clone, Debug, PartialEq)]
pub enum Field {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
}

impl Field {
    /// The value as the text a person would read (`800`, `refused`, `true`, nothing for null).
    pub fn show(&self) -> String {
        match self {
            Field::Num(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{}", *n as i64),
            Field::Num(n) => format!("{n}"),
            Field::Str(s) => s.clone(),
            Field::Bool(b) => b.to_string(),
            Field::Null => String::new(),
        }
    }
}

/// One line of a log.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Seconds since the program started.
    pub t: f64,
    /// The thread that wrote it.
    pub th: String,
    pub cat: String,
    pub ev: String,
    /// The other fields, in the order they were written.
    pub fields: Vec<(String, Field)>,
}

impl Entry {
    /// A field by name.
    pub fn get(&self, key: &str) -> Option<&Field> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// A number field.
    pub fn num(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            Field::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// The fields as `name=value` text, for one line of a report.
    pub fn detail(&self) -> String {
        self.fields
            .iter()
            .map(|(k, v)| format!("{k}={}", v.show()))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `cat.ev`, the name of the event.
    pub fn name(&self) -> String {
        format!("{}.{}", self.cat, self.ev)
    }
}

/// What is wrong with one kind of line, found once and counted.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub severity: Severity,
    /// What it is, in a few words: `tap.hook refused`, `ui.warm took 412 ms`.
    pub title: String,
    /// The event it was found in (`cat.ev`).
    pub event: String,
    /// The fields of its first line, as `name=value` text.
    pub detail: String,
    /// When its first line was written (seconds since start).
    pub first_t: f64,
    /// How many lines of this kind there are.
    pub count: u32,
}

/// What a whole log says.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    /// Lines that were read.
    pub lines: usize,
    /// Lines that were not one flat object and were skipped.
    pub skipped: usize,
    /// The time of the last line, in seconds.
    pub seconds: f64,
    /// The fields of the first line of the log (`diag.start`: version, OS, session), as text.
    pub header: String,
    /// The problems, worst first, then in the order they happened.
    pub problems: Vec<Problem>,
}

impl Report {
    /// How many problems of `severity` there are (kinds, not lines).
    pub fn count(&self, severity: Severity) -> usize {
        self.problems
            .iter()
            .filter(|p| p.severity == severity)
            .count()
    }

    /// Whether nothing was found.
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }
}

// ---------------------------------------------------------------------------------- reading

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, b: u8) -> Option<()> {
        (self.bytes.get(self.at) == Some(&b)).then(|| self.at += 1)
    }

    fn string(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let b = *self.bytes.get(self.at)?;
            self.at += 1;
            match b {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.bytes.get(self.at)?;
                    self.at += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = self.bytes.get(self.at..self.at + 4)?;
                            self.at += 4;
                            let code =
                                u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                            // a lone surrogate half is not a character: keep a placeholder
                            let c = char::from_u32(code).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                other => out.push(other),
            }
        }
    }

    fn value(&mut self) -> Option<Field> {
        match *self.bytes.get(self.at)? {
            b'"' => self.string().map(Field::Str),
            b't' if self.bytes[self.at..].starts_with(b"true") => {
                self.at += 4;
                Some(Field::Bool(true))
            }
            b'f' if self.bytes[self.at..].starts_with(b"false") => {
                self.at += 5;
                Some(Field::Bool(false))
            }
            b'n' if self.bytes[self.at..].starts_with(b"null") => {
                self.at += 4;
                Some(Field::Null)
            }
            _ => {
                let start = self.at;
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|b| matches!(b, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'))
                {
                    self.at += 1;
                }
                std::str::from_utf8(&self.bytes[start..self.at])
                    .ok()?
                    .parse()
                    .ok()
                    .map(Field::Num)
            }
        }
    }
}

/// Reads one line of a log. `None` when it is not a flat JSON object with `cat` and `ev`.
pub fn parse_line(line: &str) -> Option<Entry> {
    let mut r = Reader {
        bytes: line.trim().as_bytes(),
        at: 0,
    };
    r.eat(b'{')?;
    let mut entry = Entry {
        t: 0.0,
        th: String::new(),
        cat: String::new(),
        ev: String::new(),
        fields: Vec::new(),
    };
    r.space();
    if r.bytes.get(r.at) == Some(&b'}') {
        return None;
    }
    loop {
        r.space();
        let key = r.string()?;
        r.space();
        r.eat(b':')?;
        r.space();
        let value = r.value()?;
        match (key.as_str(), value) {
            ("t", Field::Num(n)) => entry.t = n,
            ("th", Field::Str(s)) => entry.th = s,
            ("cat", Field::Str(s)) => entry.cat = s,
            ("ev", Field::Str(s)) => entry.ev = s,
            (_, v) => entry.fields.push((key, v)),
        }
        r.space();
        match *r.bytes.get(r.at)? {
            b',' => r.at += 1,
            b'}' => {
                r.at += 1;
                break;
            }
            _ => return None,
        }
    }
    (r.at == r.bytes.len() && !entry.cat.is_empty() && !entry.ev.is_empty()).then_some(entry)
}

// ---------------------------------------------------------------------------------- rules

/// A step that took this long (`ms`) is slow.
pub const SLOW_MS: f64 = 250.0;

/// What one line says is wrong, if anything: its severity and a title.
pub fn judge(e: &Entry) -> Option<(Severity, String)> {
    if e.cat == "panic" {
        return Some((Severity::Error, format!("panic {}", e.ev)));
    }
    if e.get("ok") == Some(&Field::Bool(false)) {
        return Some((Severity::Error, format!("{} failed", e.name())));
    }
    if let Some(Field::Str(state)) = e.get("state")
        && ["refused", "failed", "denied"].contains(&state.as_str())
    {
        return Some((Severity::Error, format!("{} {state}", e.name())));
    }
    let ev = e.ev.as_str();
    if ["failed", "refused", "denied", "no_space"]
        .iter()
        .any(|w| ev.contains(w))
    {
        return Some((Severity::Error, e.name()));
    }
    if [
        "blocked",
        "unavailable",
        "unreachable",
        "giveup",
        "give_up",
        "no_pie",
    ]
    .iter()
    .any(|w| ev.contains(w))
    {
        return Some((Severity::Warning, e.name()));
    }
    // a timed step writes `ms`; `wait_ms`, `took_ms` and the like are other measures
    if let Some(ms) = e.num("ms")
        && ms >= SLOW_MS
    {
        return Some((
            Severity::Warning,
            format!("{} took {} ms", e.name(), ms.round() as i64),
        ));
    }
    None
}

/// Reads the lines of a log and lists what is wrong in them.
pub fn scan(lines: &[String]) -> Report {
    let mut report = Report::default();
    // the title of a slow step carries its time, which differs every time: count by event instead
    let mut seen: Vec<(Severity, String)> = Vec::new();
    for line in lines.iter().filter(|l| !l.trim().is_empty()) {
        let Some(e) = parse_line(line) else {
            report.skipped += 1;
            continue;
        };
        report.lines += 1;
        report.seconds = report.seconds.max(e.t);
        if report.header.is_empty() && e.cat == "diag" {
            report.header = e.detail();
        }
        let Some((severity, title)) = judge(&e) else {
            continue;
        };
        let slow = title.contains(" took ");
        let key = (
            severity,
            if slow {
                e.name() + " slow"
            } else {
                title.clone()
            },
        );
        match seen.iter().position(|k| *k == key) {
            Some(i) => {
                let p = &mut report.problems[i];
                p.count += 1;
                // for a slow step the worst one is the one to show
                if slow && e.num("ms") > p.detail_ms() {
                    p.title = title;
                    p.detail = e.detail();
                }
            }
            None => {
                seen.push(key);
                report.problems.push(Problem {
                    severity,
                    title,
                    event: e.name(),
                    detail: e.detail(),
                    first_t: e.t,
                    count: 1,
                });
            }
        }
    }
    report.problems.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then(a.first_t.total_cmp(&b.first_t))
    });
    report
}

impl Problem {
    /// The `ms` in the detail of a slow step, to compare two of them.
    fn detail_ms(&self) -> Option<f64> {
        self.detail
            .split_whitespace()
            .find_map(|w| w.strip_prefix("ms="))
            .and_then(|v| v.parse().ok())
    }
}

/// The last `n` lines of a log as entries, newest last, only of the category `cat` when one is
/// given. For a view of what just happened.
pub fn tail(lines: &[String], n: usize, cat: Option<&str>) -> Vec<Entry> {
    let mut out: Vec<Entry> = lines
        .iter()
        .rev()
        .filter_map(|l| parse_line(l))
        .filter(|e| cat.is_none_or(|c| e.cat == c))
        .take(n)
        .collect();
    out.reverse();
    out
}

/// The categories that appear in a log, most lines first.
pub fn categories(lines: &[String]) -> Vec<(String, usize)> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for e in lines.iter().filter_map(|l| parse_line(l)) {
        match counts.iter_mut().find(|(c, _)| *c == e.cat) {
            Some((_, n)) => *n += 1,
            None => counts.push((e.cat, 1)),
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts
}

// ---------------------------------------------------------------------------------- files

/// The newest log file of `app` (by name, which is time order) in `dir`.
pub fn newest_log(dir: &Path, app: &str) -> Option<PathBuf> {
    let prefix = crate::file_prefix(app);
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| crate::is_log_name(&prefix, n))
        })
        .map(|e| e.path())
        .max()
}

/// Reads the lines of a log file. The last `max_bytes` only when the file is longer (a log stops at
/// 8 MiB, but a panel should not read all of it every time).
pub fn read_lines(path: &Path, max_bytes: u64) -> std::io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let cut = len > max_bytes;
    if cut {
        file.seek(SeekFrom::Start(len - max_bytes))?;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines().map(str::to_string);
    if cut {
        lines.next(); // the first line is cut in the middle
    }
    Ok(lines.collect())
}

// ---------------------------------------------------------------------------------- text

/// The report as text for a terminal or a bug report.
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    let (errors, warnings) = (
        report.count(Severity::Error),
        report.count(Severity::Warning),
    );
    out.push_str(&format!(
        "{} lines, {:.1} s, {} {}, {} {}\n",
        report.lines,
        report.seconds,
        errors,
        if errors == 1 { "error" } else { "errors" },
        warnings,
        if warnings == 1 { "warning" } else { "warnings" },
    ));
    if !report.header.is_empty() {
        out.push_str(&format!("start: {}\n", report.header));
    }
    if report.skipped > 0 {
        out.push_str(&format!("{} lines could not be read\n", report.skipped));
    }
    if report.is_clean() {
        out.push_str("nothing wrong was found\n");
    }
    for p in &report.problems {
        out.push_str(&format!(
            "\n{:<7} {}  (x{}, first at {:.1} s)\n",
            p.severity.word().to_uppercase(),
            p.title,
            p.count,
            p.first_t
        ));
        if !p.detail.is_empty() {
            out.push_str(&format!("        {}\n", p.detail));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    #[test]
    fn a_line_is_read_with_its_fields_in_order() {
        let e = parse_line(
            r#"{"t":5.300,"th":"main","cat":"ui","ev":"show_pie","x":800.0,"ok":true,"why":"a \"b\"\n"}"#,
        )
        .unwrap();
        assert_eq!(
            (e.cat.as_str(), e.ev.as_str(), e.th.as_str()),
            ("ui", "show_pie", "main")
        );
        assert_eq!(e.t, 5.3);
        assert_eq!(e.detail(), "x=800 ok=true why=a \"b\"\n");
        assert_eq!(e.num("x"), Some(800.0));
    }

    #[test]
    fn a_line_that_is_not_one_is_skipped_not_fatal() {
        for bad in [
            "",
            "{",
            r#"{"t":1.0"#,
            "not json",
            r#"{"t":1.0}"#,
            r#"{"cat":"a","ev":"b"} x"#,
        ] {
            assert!(parse_line(bad).is_none(), "{bad:?}");
        }
        let report = scan(&lines(
            "{\"t\":1.0,\"th\":\"m\",\"cat\":\"a\",\"ev\":\"b\"}\n{\"t\":2.0,\"th\":",
        ));
        assert_eq!((report.lines, report.skipped), (1, 1));
    }

    #[test]
    fn unicode_escapes_are_read() {
        let e = parse_line(r#"{"cat":"a","ev":"b","s":"é中"}"#).unwrap();
        assert_eq!(e.get("s"), Some(&Field::Str("é中".into())));
    }

    #[test]
    fn the_rules_find_what_went_wrong() {
        let log = lines(
            r#"{"t":0.0,"th":"m","cat":"diag","ev":"start","version":"0.8.20","os":"windows"}
{"t":0.3,"th":"tap","cat":"tap","ev":"hook","state":"refused","os_error":"5"}
{"t":0.4,"th":"m","cat":"cfg","ev":"save","ok":false,"kind":"PermissionDenied"}
{"t":0.5,"th":"m","cat":"ui","ev":"show_pie","x":1.0}
{"t":1.0,"th":"m","cat":"tap","ev":"hold_settings_no_pie","id":3}
{"t":2.0,"th":"m","cat":"ui","ev":"warm","ms":412.4}
{"t":3.0,"th":"m","cat":"ui","ev":"warm","ms":900.0}
{"t":3.1,"th":"m","cat":"ui","ev":"warm","ms":10.0}
{"t":3.2,"th":"m","cat":"act","ev":"run","wait_ms":999.0}
{"t":4.0,"th":"m","cat":"panic","ev":"panic","location":"src/x.rs:1"}"#,
        );
        let r = scan(&log);
        assert_eq!(r.lines, 10);
        assert!(r.header.contains("version=0.8.20"));
        let titles: Vec<_> = r
            .problems
            .iter()
            .map(|p| (p.severity, p.title.as_str()))
            .collect();
        assert_eq!(
            titles,
            vec![
                (Severity::Error, "tap.hook refused"),
                (Severity::Error, "cfg.save failed"),
                (Severity::Error, "panic panic"),
                (Severity::Warning, "tap.hold_settings_no_pie"),
                (Severity::Warning, "ui.warm took 900 ms"),
            ],
            "errors first, then in order; the slowest slow step is the one shown"
        );
        let slow = r.problems.last().unwrap();
        assert_eq!(
            slow.count, 2,
            "two slow steps, the quick one and wait_ms do not count"
        );
        assert_eq!(
            (r.count(Severity::Error), r.count(Severity::Warning)),
            (3, 2)
        );
    }

    #[test]
    fn a_repeated_problem_is_one_row_with_a_count() {
        let log = lines(
            r#"{"t":1.0,"th":"m","cat":"x11","ev":"grab_failed","button":2}
{"t":2.0,"th":"m","cat":"x11","ev":"grab_failed","button":2}
{"t":3.0,"th":"m","cat":"x11","ev":"grab_failed","button":2}"#,
        );
        let r = scan(&log);
        assert_eq!(r.problems.len(), 1);
        assert_eq!((r.problems[0].count, r.problems[0].first_t), (3, 1.0));
    }

    #[test]
    fn a_clean_log_says_so() {
        let r = scan(&lines(r#"{"t":1.0,"th":"m","cat":"ui","ev":"show_pie"}"#));
        assert!(r.is_clean());
        assert!(render(&r).contains("nothing wrong was found"));
    }

    #[test]
    fn the_text_report_has_the_counts_and_the_details() {
        let r = scan(&lines(
            r#"{"t":0.3,"th":"m","cat":"tap","ev":"hook","state":"refused"}"#,
        ));
        let text = render(&r);
        assert!(text.contains("1 error, 0 warnings"), "{text}");
        assert!(text.contains("ERROR   tap.hook refused"), "{text}");
        assert!(text.contains("state=refused"), "{text}");
    }

    #[test]
    fn the_tail_keeps_the_newest_lines_and_can_pick_a_category() {
        let log = lines(
            r#"{"t":1.0,"th":"m","cat":"ui","ev":"a"}
{"t":2.0,"th":"m","cat":"tap","ev":"b"}
{"t":3.0,"th":"m","cat":"ui","ev":"c"}"#,
        );
        let all: Vec<_> = tail(&log, 2, None).into_iter().map(|e| e.ev).collect();
        assert_eq!(all, ["b", "c"]);
        let ui: Vec<_> = tail(&log, 5, Some("ui"))
            .into_iter()
            .map(|e| e.ev)
            .collect();
        assert_eq!(ui, ["a", "c"]);
        assert_eq!(
            categories(&log),
            vec![("ui".to_string(), 2), ("tap".to_string(), 1)]
        );
    }

    #[test]
    fn the_newest_log_is_found_by_name_and_big_files_are_read_from_the_end() {
        let dir = std::env::temp_dir().join(format!("diag-problems-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, body) in [
            (
                "cursorpie-0000000001-1.jsonl",
                "{\"cat\":\"a\",\"ev\":\"old\"}\n",
            ),
            (
                "cursorpie-0000000002-1.jsonl",
                "{\"cat\":\"a\",\"ev\":\"one\"}\n{\"cat\":\"a\",\"ev\":\"two\"}\n",
            ),
            ("other.txt", "x"),
        ] {
            std::fs::write(dir.join(name), body).unwrap();
        }
        let newest = newest_log(&dir, "CursorPie").unwrap();
        assert!(newest.ends_with("cursorpie-0000000002-1.jsonl"));
        assert_eq!(read_lines(&newest, 1 << 20).unwrap().len(), 2);
        // only the end: the first (cut) line is dropped
        let cut = read_lines(&newest, 30).unwrap();
        assert_eq!(cut, vec!["{\"cat\":\"a\",\"ev\":\"two\"}".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
