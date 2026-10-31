//! Pure helpers: file names, the line format and dates. None of them touches the global log, so
//! each one is unit tested on its own.

use crate::LOG_DIR;
use crate::value::{Value, write_json_str};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The end of every log file name.
pub(crate) const FILE_SUFFIX: &str = ".jsonl";
/// Keys every line has; a field of the same name is renamed with a leading underscore.
const RESERVED: [&str; 4] = ["t", "th", "cat", "ev"];

/// The start of the log file names of `app`: lower case letters, digits and `_`; anything else
/// becomes `-`. `CursorPie` gives `cursorpie`.
pub fn file_prefix(app: &str) -> String {
    let cleaned: String = app
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.trim_matches('-').is_empty() {
        "app".to_string()
    } else {
        cleaned
    }
}

/// The log folder: `dir_override` when it is not empty, else `<support>/logs`, else a folder of
/// the system's temporary files (for a program that has no folder of its own).
pub fn log_dir_from(dir_override: Option<&str>, support: Option<&Path>, app: &str) -> PathBuf {
    match dir_override.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => match support {
            Some(s) => s.join(LOG_DIR),
            None => std::env::temp_dir().join(format!("{}-logs", file_prefix(app))),
        },
    }
}

/// The name of the log file of a run. The zero-padded seconds make name order time order.
pub fn file_name(prefix: &str, unix_secs: u64, pid: u32) -> String {
    format!("{prefix}-{unix_secs:010}-{pid}{FILE_SUFFIX}")
}

/// Whether a file name is one of ours (so nothing else in the folder is ever deleted).
pub fn is_log_name(prefix: &str, name: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('-'))
        && name.ends_with(FILE_SUFFIX)
}

/// The log files to delete so that at most `keep` remain: the oldest of `names`.
pub fn stale(prefix: &str, names: &[String], keep: usize) -> Vec<String> {
    let mut logs: Vec<&String> = names.iter().filter(|n| is_log_name(prefix, n)).collect();
    logs.sort();
    let surplus = logs.len().saturating_sub(keep);
    logs.into_iter().take(surplus).cloned().collect()
}

/// Whether a line of `add` bytes still fits in a file of `max` bytes that holds `written`.
pub(crate) fn has_room(written: u64, add: u64, max: u64) -> bool {
    written.saturating_add(add) <= max
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

/// One log line: the fixed keys (`t`, `th`, `cat`, `ev`) first, then `fields` in the order they
/// were given, as a single line of JSON without the newline.
pub fn format_line(t: f64, thread: &str, cat: &str, ev: &str, fields: &[(&str, Value)]) -> String {
    let mut out = String::with_capacity(128);
    let t = if t.is_finite() { t } else { 0.0 };
    let _ = write!(out, "{{\"t\":{t:.3},\"th\":");
    write_json_str(thread, &mut out);
    out.push_str(",\"cat\":");
    write_json_str(cat, &mut out);
    out.push_str(",\"ev\":");
    write_json_str(ev, &mut out);
    for (key, value) in fields {
        out.push(',');
        if RESERVED.contains(key) {
            write_json_str(&format!("_{key}"), &mut out);
        } else {
            write_json_str(key, &mut out);
        }
        out.push(':');
        value.write_json(&mut out);
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prefix_is_a_safe_file_name_start() {
        assert_eq!(file_prefix("CursorPie"), "cursorpie");
        assert_eq!(file_prefix("my app/2"), "my-app-2");
        assert_eq!(file_prefix("my_app"), "my_app");
        assert_eq!(file_prefix("///"), "app");
        assert_eq!(file_prefix(""), "app");
    }

    #[test]
    fn the_log_folder_can_be_moved() {
        let support = Path::new("/home/u/.config/cursorpie");
        assert_eq!(log_dir_from(None, Some(support), "x"), support.join("logs"));
        assert_eq!(
            log_dir_from(Some("  "), Some(support), "x"),
            support.join("logs")
        );
        assert_eq!(
            log_dir_from(Some("/tmp/x"), Some(support), "x"),
            Path::new("/tmp/x")
        );
        assert_eq!(
            log_dir_from(None, None, "My App"),
            std::env::temp_dir().join("my-app-logs"),
            "no folder of its own: the temporary folder"
        );
    }

    #[test]
    fn file_names_sort_by_time_and_only_ours_are_recognized() {
        assert_eq!(
            file_name("cursorpie", 42, 7),
            "cursorpie-0000000042-7.jsonl"
        );
        assert!(file_name("a", 1_793_000_000, 1) > file_name("a", 999_999_999, 99999));
        assert!(is_log_name("cursorpie", "cursorpie-0000000042-7.jsonl"));
        assert!(!is_log_name("cursorpie", "config.json"));
        assert!(!is_log_name("cursorpie", "cursorpie-notes.txt"));
        assert!(
            !is_log_name("cursor", "cursorpie-0000000042-7.jsonl"),
            "another app's log"
        );
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
            stale("cursorpie", &names, 2),
            ["cursorpie-0000000100-1.jsonl".to_string()]
        );
        assert!(stale("cursorpie", &names, 3).is_empty());
        assert!(stale("cursorpie", &names, 10).is_empty());
        assert_eq!(
            stale("cursorpie", &names, 0).len(),
            3,
            "notes.txt is never listed"
        );
    }

    #[test]
    fn a_file_stops_growing_at_the_cap() {
        assert!(has_room(0, 100, 1000));
        assert!(has_room(990, 10, 1000));
        assert!(!has_room(990, 11, 1000));
        assert!(!has_room(u64::MAX, 1, u64::MAX - 1), "no overflow");
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
        assert_eq!(
            line,
            r#"{"t":1.235,"th":"main","cat":"ui","ev":"show_pie","x":812.5,"ok":true,"name":"a\"b","_t":9}"#
        );
        assert!(!line.contains('\n'));
    }

    #[test]
    fn a_line_without_fields_is_still_an_object() {
        assert_eq!(
            format_line(0.0, "?", "diag", "size_cap", &[]),
            r#"{"t":0.000,"th":"?","cat":"diag","ev":"size_cap"}"#
        );
        assert!(format_line(f64::NAN, "main", "a", "b", &[]).starts_with(r#"{"t":0.000,"#));
    }
}
