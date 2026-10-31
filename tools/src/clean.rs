//! `cargo tools clean-logs`: deletes old debug logs of the app (the files `cursorpie-*.jsonl`
//! that `CURSORPIE_DEBUG=1` writes, see `docs/DEBUGGING.md`).
//!
//! The app keeps only its newest ten logs by itself; this is for clearing them by hand: all of
//! them, or all but the newest few, or the ones older than some days. Only files whose names are
//! those of a log are ever deleted, so pointing `--dir` at a folder with other files is safe.
//!
//! | Option | Meaning |
//! |---|---|
//! | `--keep N` | keep the newest `N` logs (default 3) |
//! | `--older-than DAYS` | delete only logs older than that (the age comes from the file name) |
//! | `--all` | delete every log |
//! | `--dir PATH` | clean this folder instead of the app's log folder |
//! | `--dry-run` | say what would be deleted and delete nothing |

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::diag::{LOG_PREFIX, default_log_dir, is_log_name};

/// How many logs are kept when `--keep` is not given.
const DEFAULT_KEEP: usize = 3;
const SECS_PER_DAY: u64 = 86_400;

/// What to clean.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    /// How many of the newest logs stay.
    pub keep: usize,
    /// When set, only logs older than this many days are deleted.
    pub older_than_days: Option<u64>,
    /// The folder to clean; `None` is the app's log folder.
    pub dir: Option<PathBuf>,
    /// Say what would be deleted and delete nothing.
    pub dry_run: bool,
}

/// Reads the arguments after `clean-logs`.
///
/// # Errors
/// On an unknown option, a missing or wrong value, or `--all` together with `--keep` or
/// `--older-than`.
pub fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut keep = None;
    let mut older = None;
    let mut all = false;
    let mut dir = None;
    let mut dry_run = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--keep" => {
                let n = args.next().ok_or("--keep needs a number")?;
                keep = Some(
                    n.parse::<usize>()
                        .map_err(|_| format!("--keep: '{n}' is not a number"))?,
                );
            }
            "--older-than" => {
                let n = args.next().ok_or("--older-than needs a number of days")?;
                older = Some(
                    n.parse::<u64>()
                        .map_err(|_| format!("--older-than: '{n}' is not a number of days"))?,
                );
            }
            "--dir" => dir = Some(PathBuf::from(args.next().ok_or("--dir needs a folder")?)),
            "--all" => all = true,
            "--dry-run" => dry_run = true,
            other => return Err(format!("unknown option '{other}' for clean-logs")),
        }
    }
    if all && (keep.is_some() || older.is_some()) {
        return Err("--all cannot be combined with --keep or --older-than".to_string());
    }
    Ok(Options {
        keep: if all { 0 } else { keep.unwrap_or(DEFAULT_KEEP) },
        older_than_days: older,
        dir,
        dry_run,
    })
}

/// The time a log was started, in Unix seconds, read from its name
/// (`cursorpie-<seconds>-<pid>.jsonl`).
fn log_time(name: &str) -> Option<u64> {
    name.strip_prefix(LOG_PREFIX)?
        .split('-')
        .next()?
        .parse()
        .ok()
}

/// The logs among `names` to delete: the oldest ones, so that `keep` remain, and of those only
/// the ones older than `older_than_days` when that is given. `now` is the time in Unix seconds.
pub fn select(
    names: &[String],
    keep: usize,
    older_than_days: Option<u64>,
    now: u64,
) -> Vec<String> {
    let mut logs: Vec<&String> = names.iter().filter(|n| is_log_name(n)).collect();
    logs.sort(); // the names sort by time
    let removable = logs.len().saturating_sub(keep);
    logs.into_iter()
        .take(removable)
        .filter(|n| match older_than_days {
            None => true,
            Some(days) => log_time(n)
                .is_some_and(|t| now.saturating_sub(t) > days.saturating_mul(SECS_PER_DAY)),
        })
        .cloned()
        .collect()
}

/// A size for people: `1.5 KiB`.
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// The folder to clean: the one given, else the app's log folder.
fn target_dir(opts: &Options) -> Result<PathBuf, String> {
    match &opts.dir {
        Some(d) => Ok(d.clone()),
        None => default_log_dir(&|k| std::env::var(k).ok()).ok_or_else(|| {
            "cannot tell where the logs are (no home folder); use --dir".to_string()
        }),
    }
}

/// Deletes the logs in `dir` that [`select`] picks; returns how many and how many bytes.
fn clean(dir: &Path, opts: &Options) -> Result<(usize, u64), String> {
    let names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (mut count, mut bytes) = (0, 0);
    for name in select(&names, opts.keep, opts.older_than_days, now) {
        let path = dir.join(&name);
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        if opts.dry_run {
            println!("would delete {name} ({})", human_bytes(size));
        } else {
            std::fs::remove_file(&path)
                .map_err(|e| format!("cannot delete {}: {e}", path.display()))?;
        }
        count += 1;
        bytes += size;
    }
    Ok((count, bytes))
}

/// Cleans the logs and says what was done.
///
/// # Errors
/// When the folder cannot be read or a file cannot be deleted.
pub fn run(opts: &Options) -> Result<ExitCode, String> {
    let dir = target_dir(opts)?;
    if !dir.is_dir() {
        println!("No log folder at {}: nothing to clean.", dir.display());
        return Ok(ExitCode::SUCCESS);
    }
    let (count, bytes) = clean(&dir, opts)?;
    let verb = if opts.dry_run {
        "Would delete"
    } else {
        "Deleted"
    };
    println!(
        "{verb} {count} log{} ({}) in {}.",
        if count == 1 { "" } else { "s" },
        human_bytes(bytes),
        dir.display()
    );
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        words(list)
    }

    #[test]
    fn the_options_are_read() {
        let o = parse_args(&words(&[])).unwrap();
        assert_eq!((o.keep, o.older_than_days, o.dry_run), (3, None, false));
        let o = parse_args(&words(&["--keep", "5", "--older-than", "7", "--dry-run"])).unwrap();
        assert_eq!((o.keep, o.older_than_days, o.dry_run), (5, Some(7), true));
        let o = parse_args(&words(&["--all", "--dir", "/tmp/x"])).unwrap();
        assert_eq!((o.keep, o.dir), (0, Some(PathBuf::from("/tmp/x"))));
    }

    #[test]
    fn wrong_options_are_errors() {
        assert!(parse_args(&words(&["--keep"])).is_err());
        assert!(parse_args(&words(&["--keep", "many"])).is_err());
        assert!(parse_args(&words(&["--older-than", "-1"])).is_err());
        assert!(parse_args(&words(&["--all", "--keep", "2"])).is_err());
        assert!(parse_args(&words(&["--everything"])).is_err());
    }

    #[test]
    fn the_time_comes_from_the_name() {
        assert_eq!(log_time("cursorpie-0000000100-4242.jsonl"), Some(100));
        assert_eq!(log_time("notes.txt"), None);
        assert_eq!(log_time("cursorpie-abc-1.jsonl"), None);
    }

    #[test]
    fn the_oldest_go_and_other_files_stay() {
        let list = names(&[
            "cursorpie-0000000300-1.jsonl",
            "cursorpie-0000000100-1.jsonl",
            "notes.txt",
            "cursorpie-0000000200-1.jsonl",
        ]);
        assert_eq!(
            select(&list, 1, None, 0),
            [
                "cursorpie-0000000100-1.jsonl",
                "cursorpie-0000000200-1.jsonl"
            ]
        );
        assert!(select(&list, 3, None, 0).is_empty());
        assert_eq!(
            select(&list, 0, None, 0).len(),
            3,
            "notes.txt is never listed"
        );
    }

    #[test]
    fn only_logs_older_than_the_limit_are_deleted() {
        let day = SECS_PER_DAY;
        let list = names(&[
            "cursorpie-0000000000-1.jsonl",
            "cursorpie-0000172800-1.jsonl",
            "cursorpie-0000345600-1.jsonl",
        ]);
        // now = day 5: the logs are 5, 3 and 1 days old; keep none, delete those older than 2 days
        let gone = select(&list, 0, Some(2), 5 * day);
        assert_eq!(
            gone,
            [
                "cursorpie-0000000000-1.jsonl",
                "cursorpie-0000172800-1.jsonl"
            ]
        );
        // the newest is kept first, then the age decides
        assert_eq!(
            select(&list, 2, Some(2), 5 * day),
            ["cursorpie-0000000000-1.jsonl"]
        );
    }

    #[test]
    fn sizes_are_readable() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(1023), "1023 B");
        assert_eq!(human_bytes(1536), "1.5 KiB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }
}
