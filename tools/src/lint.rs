//! `cargo tools diag-lint`: checks the log lines written in `src/` against the rules of
//! `docs/DEBUGGING.md` and against what the analyzer reads.
//!
//! The analyzer (`diag.rs`, `logview.rs`) reads events by name: `ui` / `show_pie`, with the fields
//! `left` and `top`. Nothing connects those names to the `diag::log!` lines that write them, so a
//! rename in the app would leave the analyzer looking for an event that no longer exists, and it
//! would report "all is well" about a log it can no longer read. This command closes that gap, and
//! it is one of the quick steps of `cargo tools ci`, so it runs before every commit.
//!
//! What it checks, for every `diag::log!`, `trace!`, `once!`, `timed!`, `diag::event(..)` and
//! `diag::Watch::new(..)` in `src/`:
//!
//! | Rule | Why |
//! |---|---|
//! | the category is one of [`CATEGORIES`] and is in the table of `docs/DEBUGGING.md` | a reader of the docs finds every category |
//! | category and event are `snake_case` | the names the analyzer and `jq` filters rely on |
//! | no field is called `t`, `th`, `cat` or `ev` | the crate would rename it (it is the line's own) |
//! | no field has the name of private data (`path`, `file`, `text`, `cmd`, `url`, ...) | the log is pasted into bug reports |
//! | no field is written twice in one line | the reader keeps one of them and drops the other |
//! | every event of [`crate::diag::EXPECTED`] is written, with its fields | the analyzer is not blind |
//!
//! It reads the source as text and finds the calls by their names, the way `grep` would, so it
//! needs no compiler and takes milliseconds. It understands strings, comments and nested
//! brackets, and the one trick the app uses for an event name: an `if .. { "press" } else {
//! "release" }` as the event counts as both names. A call it cannot read (a category that is not
//! a plain string) is skipped, not an error.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::diag::{EXPECTED, Expected};

/// The categories of the log: the layers of the app. A new one is added here and to the table of
/// categories in `docs/DEBUGGING.md` (the lint checks that it is there).
pub const CATEGORIES: &[&str] = &[
    "diag", "sys", "x11", "tap", "ui", "win", "panic", "cfg", "act",
];

/// Written by the `diag` crate, not by a line in `src/`.
const BUILT_IN: &[(&str, &str)] = &[("diag", "start"), ("diag", "size_cap"), ("panic", "panic")];

/// Field names the crate keeps for the line itself.
const RESERVED_FIELDS: &[&str] = &["t", "th", "cat", "ev"];

/// Field names that would hold what belongs to the person: a file, typed text, a command.
const PRIVATE_FIELDS: &[&str] = &[
    "path", "file", "filename", "text", "typed", "content", "config", "cmd", "command", "url",
];

/// The event name of the unit tests of the log itself: it is not part of the app.
const TEST_EVENT: &str = "never";

/// What a call looks like at its start. The ones that end in `!(` are macros, whose fields are
/// written in the call; the others build the line in steps, so their fields are not known.
const CALLS: [&str; 6] = [
    "diag::log!(",
    "diag::trace!(",
    "diag::once!(",
    "diag::timed!(",
    "diag::event(",
    "Watch::new(",
];

/// One place in the source that writes (or watches, or times) a log line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// The file, with `/` as the separator, relative to the repository.
    pub file: String,
    /// The line of the call.
    pub line: usize,
    pub cat: String,
    /// The string literals of the event argument: one name, or two for an `if` that picks one.
    pub events: Vec<String>,
    /// The names of the fields written in the call.
    pub fields: Vec<String>,
    /// Whether `fields` is the whole list (false for `diag::event(..).field(..)` and `Watch`).
    pub fields_known: bool,
}

// ------------------------------------------------------------------------------------ scanning

/// If a string, a character or a comment starts at `i`, the index just after it.
fn skip_lit(b: &[u8], i: usize) -> Option<usize> {
    match *b.get(i)? {
        b'"' => {
            let mut j = i + 1;
            while j < b.len() {
                match b[j] {
                    b'\\' => j += 2,
                    b'"' => return Some(j + 1),
                    _ => j += 1,
                }
            }
            Some(b.len())
        }
        b'/' if b.get(i + 1) == Some(&b'/') => {
            let mut j = i;
            while j < b.len() && b[j] != b'\n' {
                j += 1;
            }
            Some(j)
        }
        b'/' if b.get(i + 1) == Some(&b'*') => {
            let mut j = i + 2;
            while j + 1 < b.len() {
                if b[j] == b'*' && b[j + 1] == b'/' {
                    return Some(j + 2);
                }
                j += 1;
            }
            Some(b.len())
        }
        b'\'' => {
            if b.get(i + 1) == Some(&b'\\') {
                // '\n', '\'', '\u{41}': the closing quote comes after the escaped character
                let mut j = i + 3;
                while j < b.len() && b[j] != b'\'' {
                    j += 1;
                }
                Some((j + 1).min(b.len()))
            } else if b.get(i + 2) == Some(&b'\'') {
                Some(i + 3)
            } else {
                None // a lifetime, not a character
            }
        }
        _ => None,
    }
}

/// The index of the bracket that closes the one opened just before `start`.
fn args_end(b: &[u8], start: usize) -> Option<usize> {
    let mut depth = 1usize;
    let mut i = start;
    while i < b.len() {
        if let Some(next) = skip_lit(b, i) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The arguments of a call: the text between its brackets cut at the commas that are not inside
/// a string, a comment or a bracket.
fn split_args(region: &str) -> Vec<&str> {
    let b = region.as_bytes();
    let mut parts = Vec::new();
    let (mut depth, mut from, mut i) = (0usize, 0usize, 0usize);
    while i < b.len() {
        if let Some(next) = skip_lit(b, i) {
            i = next;
            continue;
        }
        match b[i] {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                parts.push(region[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    let last = region[from..].trim();
    if !last.is_empty() {
        parts.push(last);
    }
    parts
}

/// The contents of every string literal in an expression, in order.
fn literals(expr: &str) -> Vec<String> {
    let b = expr.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match skip_lit(b, i) {
            Some(next) => {
                if b[i] == b'"' && next >= i + 2 && b[next - 1] == b'"' {
                    out.push(expr[i + 1..next - 1].to_string());
                }
                i = next;
            }
            None => i += 1,
        }
    }
    out
}

/// The name of the field one argument of a log macro writes: `name = value`, `name = ?value` or
/// a bare `name` (the variable of that name). `None` for anything else.
fn field_name(arg: &str) -> Option<String> {
    let end = arg
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(arg.len());
    let name = &arg[..end];
    if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let rest = arg[end..].trim_start();
    let plain = rest.is_empty() || (rest.starts_with('=') && !rest.starts_with("=="));
    plain.then(|| name.to_string())
}

/// Every log call in the text of one file.
pub fn scan(file: &str, text: &str) -> Vec<Site> {
    let b = text.as_bytes();
    let mut sites = Vec::new();
    for pattern in CALLS {
        for (pos, _) in text.match_indices(pattern) {
            // a call that is written in a comment (an example in the documentation) is not one
            let line_start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
            if text[line_start..pos].trim_start().starts_with("//") {
                continue;
            }
            let start = pos + pattern.len();
            let Some(end) = args_end(b, start) else {
                continue;
            };
            let args = split_args(&text[start..end]);
            let (Some(cat_arg), Some(event_arg)) = (args.first(), args.get(1)) else {
                continue;
            };
            let cats = literals(cat_arg);
            let events = literals(event_arg);
            if cats.len() != 1 || events.is_empty() {
                continue;
            }
            let is_macro = pattern.ends_with("!(");
            let fields: Vec<String> = if is_macro {
                args[2..].iter().filter_map(|a| field_name(a)).collect()
            } else {
                Vec::new()
            };
            sites.push(Site {
                file: file.to_string(),
                line: b[..pos].iter().filter(|c| **c == b'\n').count() + 1,
                cat: cats[0].clone(),
                events,
                fields,
                fields_known: is_macro,
            });
        }
    }
    sites.sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    sites
}

// ------------------------------------------------------------------------------------ checking

/// Whether a name is `snake_case`: lower-case letters, digits and `_`, starting with a letter.
fn is_snake_case(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// What is wrong with the log lines: the rules for every line, then the analyzer's contract.
/// `documented` is the text of `docs/DEBUGGING.md` (`None`: the table of categories is not
/// checked).
pub fn check(sites: &[Site], documented: Option<&str>, expected: &[Expected]) -> Vec<String> {
    let mut problems = Vec::new();
    let app_sites: Vec<&Site> = sites
        .iter()
        .filter(|s| !s.events.iter().all(|e| e == TEST_EVENT))
        .collect();

    for s in &app_sites {
        let at = format!("{}:{}", s.file, s.line);
        if !CATEGORIES.contains(&s.cat.as_str()) {
            problems.push(format!(
                "{at}: unknown category '{}': add it to CATEGORIES in tools/src/lint.rs and to the table in docs/DEBUGGING.md",
                s.cat
            ));
        }
        if !is_snake_case(&s.cat) {
            problems.push(format!("{at}: the category '{}' is not snake_case", s.cat));
        }
        for event in &s.events {
            if !is_snake_case(event) {
                problems.push(format!("{at}: the event '{event}' is not snake_case"));
            }
        }
        let mut seen: Vec<&str> = Vec::new();
        for field in &s.fields {
            if RESERVED_FIELDS.contains(&field.as_str()) {
                problems.push(format!(
                    "{at}: the field '{field}' is the line's own name; the log would rename it"
                ));
            }
            if PRIVATE_FIELDS.contains(&field.as_str()) {
                problems.push(format!(
                    "{at}: the field '{field}' sounds like the user's data; log a kind, a count or a length, never the content"
                ));
            }
            if seen.contains(&field.as_str()) {
                problems.push(format!("{at}: the field '{field}' is written twice"));
            }
            seen.push(field.as_str());
        }
    }

    if let Some(doc) = documented {
        let mut used: Vec<&str> = app_sites.iter().map(|s| s.cat.as_str()).collect();
        used.sort_unstable();
        used.dedup();
        for cat in used {
            if !doc.contains(&format!("| `{cat}` |")) {
                problems.push(format!(
                    "the category '{cat}' is not in the table of categories of docs/DEBUGGING.md"
                ));
            }
        }
    }

    for e in expected {
        if BUILT_IN.contains(&(e.cat, e.ev)) {
            continue;
        }
        let writers: Vec<&&Site> = app_sites
            .iter()
            .filter(|s| s.cat.as_str() == e.cat && s.events.iter().any(|x| x.as_str() == e.ev))
            .collect();
        if writers.is_empty() {
            problems.push(format!(
                "the analyzer (tools/src/diag.rs, EXPECTED) reads {}.{}, but no log line in src/ writes it: was it renamed?",
                e.cat, e.ev
            ));
            continue;
        }
        for field in e.fields {
            // a field is missing only when every line that writes the event is known to lack it
            let missing = writers
                .iter()
                .all(|s| s.fields_known && !s.fields.iter().any(|f| f.as_str() == *field));
            if missing {
                problems.push(format!(
                    "the analyzer reads the field '{field}' of {}.{}, but no log line in src/ writes it with that field",
                    e.cat, e.ev
                ));
            }
        }
    }
    problems
}

// -------------------------------------------------------------------------------------- running

/// Every `.rs` file under `dir`.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

/// The problems of the log lines in the repository at `root`.
pub fn problems(root: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    collect_rs(&root.join("src"), &mut files)?;
    files.sort();
    let mut sites = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read {}: {e}", file.display()))?;
        let relative = file
            .strip_prefix(root)
            .unwrap_or(file.as_path())
            .display()
            .to_string()
            .replace('\\', "/");
        sites.extend(scan(&relative, &text));
    }
    if sites.is_empty() {
        return Err("no log line found in src/: is this the repository root?".to_string());
    }
    let documented = std::fs::read_to_string(root.join("docs").join("DEBUGGING.md")).ok();
    Ok(check(&sites, documented.as_deref(), EXPECTED))
}

/// Prints the problems and returns the exit code.
pub fn run(root: &Path) -> Result<ExitCode, String> {
    let found = problems(root)?;
    if found.is_empty() {
        println!("the log lines in src/ follow the rules and match what the analyzer reads");
        return Ok(ExitCode::SUCCESS);
    }
    println!("{} problem(s) with the log lines:\n", found.len());
    for p in &found {
        println!("  - {p}");
    }
    println!("\nThe rules: docs/DEBUGGING.md, \"Add a log line\".");
    Ok(ExitCode::FAILURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
//! Docs may show a call: diag::log!("nope", "in_a_comment", x = 1);
fn demo() {
    diag::log!("ui", "show_pie", x = x, y = y, profile, kind = ?kind);
    diag::log!(
        "tap",
        if down { "press" } else { "release" },
        detail = e.detail,
        query_px = a.map_or_else(String::new, |(qx, qy)| format!("{qx},{qy}")),
        msg = "a, (b",
    );
    diag::trace!("tap", "drag", x = pos.x);
    let _t = diag::timed!("ui", "load_icons", count = n);
    diag::event("sys", "startup").field("monitors", 2).write();
    static W: diag::Watch = diag::Watch::new("win", "win_obs", 2000, 12);
    diag::log!(category_from_variable, "skipped");
}
"#;

    fn sites() -> Vec<Site> {
        scan("src/demo.rs", SAMPLE)
    }

    #[test]
    fn calls_are_found_with_their_names_and_fields() {
        let all = sites();
        assert_eq!(all.len(), 6, "{all:#?}");
        let find = |ev: &str| {
            all.iter()
                .find(|s| s.events.iter().any(|e| e == ev))
                .unwrap_or_else(|| panic!("no site writes {ev}"))
        };

        let show = find("show_pie");
        assert_eq!(show.cat, "ui");
        assert_eq!(show.fields, ["x", "y", "profile", "kind"]);
        assert!(show.fields_known);
        assert_eq!(show.line, 4);

        let press = find("press");
        assert_eq!(
            press.events,
            ["press", "release"],
            "an if picks one of two names"
        );
        assert_eq!(
            press.fields,
            ["detail", "query_px", "msg"],
            "{:?}",
            press.fields
        );

        assert_eq!(find("drag").fields, ["x"]);
        assert_eq!(find("load_icons").fields, ["count"]);

        let startup = find("startup");
        assert!(
            !startup.fields_known,
            "fields added with .field() are not read"
        );
        assert!(!find("win_obs").fields_known);
    }

    #[test]
    fn a_call_in_a_comment_or_with_a_variable_category_is_not_a_site() {
        let all = sites();
        assert!(all.iter().all(|s| s.cat != "nope"));
        assert!(all.iter().all(|s| s.events != ["skipped"]));
    }

    #[test]
    fn strings_comments_and_characters_do_not_confuse_the_brackets() {
        let text = r#"diag::log!("ui", "a", p = ')', q = "(", r = /* ) , */ '\'', s = 1);
        diag::log!("ui", "b", t = 2);"#;
        let all = scan("x.rs", text);
        assert_eq!(all.len(), 2, "{all:?}");
        assert_eq!(all[0].fields, ["p", "q", "r", "s"]);
        assert_eq!(all[1].fields, ["t"]);
    }

    #[test]
    fn names_are_snake_case_or_not() {
        assert!(is_snake_case("show_pie"));
        assert!(is_snake_case("x11"));
        assert!(!is_snake_case("ShowPie"));
        assert!(!is_snake_case("show-pie"));
        assert!(!is_snake_case("1st"));
        assert!(!is_snake_case(""));
    }

    fn bad(cat: &str, event: &str, fields: &[&str]) -> Site {
        Site {
            file: "src/a.rs".into(),
            line: 7,
            cat: cat.into(),
            events: vec![event.into()],
            fields: fields.iter().map(|f| f.to_string()).collect(),
            fields_known: true,
        }
    }

    #[test]
    fn the_rules_for_one_line_are_enforced() {
        let doc = "| `ui` | the interface |";
        let all = check(
            &[
                bad("ui", "ShowPie", &["x", "x", "cat", "path"]),
                bad("gpu", "frame", &[]),
            ],
            Some(doc),
            &[],
        )
        .join("\n");
        assert!(
            all.contains("src/a.rs:7: the event 'ShowPie' is not snake_case"),
            "{all}"
        );
        assert!(all.contains("the field 'x' is written twice"), "{all}");
        assert!(
            all.contains("the field 'cat' is the line's own name"),
            "{all}"
        );
        assert!(
            all.contains("the field 'path' sounds like the user's data"),
            "{all}"
        );
        assert!(all.contains("unknown category 'gpu'"), "{all}");
        assert!(all.contains("category 'gpu' is not in the table"), "{all}");
        assert!(!all.contains("category 'ui' is not in the table"), "{all}");
    }

    #[test]
    fn the_test_event_of_the_log_itself_is_ignored() {
        let found = check(&[bad("whatever", "never", &["path"])], None, &[]);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn what_the_analyzer_reads_must_be_written() {
        let expected = [
            Expected {
                cat: "ui",
                ev: "show_pie",
                fields: &["left", "top"],
            },
            Expected {
                cat: "ui",
                ev: "gone",
                fields: &[],
            },
            Expected {
                cat: "diag",
                ev: "start",
                fields: &[],
            },
        ];
        let found = check(&[bad("ui", "show_pie", &["left", "x"])], None, &expected).join("\n");
        assert!(
            found.contains("reads ui.gone, but no log line in src/ writes it"),
            "{found}"
        );
        assert!(
            found.contains("the field 'top' of ui.show_pie"),
            "a missing field is named: {found}"
        );
        assert!(!found.contains("'left'"), "{found}");
        assert!(
            !found.contains("diag.start"),
            "the crate writes that one: {found}"
        );
    }

    #[test]
    fn a_line_with_fields_added_later_may_have_any_field() {
        let mut open = bad("sys", "startup", &[]);
        open.fields_known = false;
        let expected = [Expected {
            cat: "sys",
            ev: "startup",
            fields: &["monitors"],
        }];
        assert!(check(&[open], None, &expected).is_empty());
    }

    /// The test that keeps the app and the analyzer together: this repository's own lines.
    #[test]
    fn the_log_lines_of_this_repository_follow_the_rules() {
        let root = crate::repo_root(
            std::env::var_os("CARGO_MANIFEST_DIR"),
            env!("CARGO_MANIFEST_DIR"),
        );
        let found = problems(&root).expect("the sources can be read");
        assert!(found.is_empty(), "\n{}", found.join("\n"));
    }
}
