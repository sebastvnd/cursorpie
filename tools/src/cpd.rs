//! `cpd`: the diagnostic logs of CursorPie in a terminal, under a name that is quick to type.
//!
//! It is the reading half of `cargo tools diag`, `cargo tools log` and `cargo tools clean-logs`
//! without the repository: it is attached to every release (`cpd-<system>`), so a person who runs the
//! log build (About, Developer mode) can read their own log, and a developer can read what a person
//! sent, with nothing else installed. The same code runs behind both names.
//!
//! ```text
//! cpd                what went wrong in the newest log (the list the Developer panel shows)
//! cpd d              the analysis of the pie: where it was shown, where it was cut
//! cpd s | t | g | sl | df | f | r      summary, timeline, grep, slow, diff, follow, report
//! cpd on | off       switch the log of the next start (the `debug` file)
//! cpd path           where the logs are, and the newest one
//! cpd clean          delete old logs
//! ```
//!
//! Every command takes a log file as its last argument; without one it reads the newest.

#![allow(dead_code)]

#[path = "clean.rs"]
mod clean;
#[path = "diag.rs"]
mod diag;
#[path = "git.rs"]
mod git;
#[path = "info.rs"]
mod info;
#[path = "logview.rs"]
mod logview;
#[path = "zip.rs"]
mod zip;

use logscan::problems;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const HELP: &str = "\
cpd - read the diagnostic logs of CursorPie

  cpd [FILE]                 what went wrong (errors first, repeats counted); exit 1 when something did
  cpd d    [FILE]            the analysis of the pie (shown, cut, never drawn)
  cpd s    [FILE]            summary: what the log holds
  cpd t    [FILE]            timeline: one row per press
  cpd g    TEXT|cat:ev ...   lines of a category (tap), an event (ui:show_pie) or a field (--field x=800)
  cpd sl   [MS] [FILE]       steps that took MS or more (default 250)
  cpd df   A B               what changed between two logs
  cpd f    [FILE]            follow a run as it is written
  cpd r    [FILE]            everything above as one text for a bug report
  cpd on   [SPEC]            log from the next start: all (default), tap,ui, all,trace, mem
  cpd off                    stop logging from the next start
  cpd path                   the logs folder and the newest log
  cpd clean [--keep N] [--older-than DAYS] [--all] [--dry-run]
  cpd info                   this program and where it looks
  cpd help | --version

FILE is a log (cursorpie-*.jsonl); without it the newest one is read.
The long names work too: summary, timeline, grep, slow, diff, follow, report, diag, errors.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("cpd: {message}");
            eprintln!("try: cpd help");
            ExitCode::from(2)
        }
    }
}

/// The view a short name stands for, as `logview` names it.
fn long_view(name: &str) -> Option<&'static str> {
    Some(match name {
        "s" | "sum" | "summary" => "summary",
        "t" | "tl" | "timeline" => "timeline",
        "g" | "grep" => "grep",
        "sl" | "slow" => "slow",
        "df" | "diff" => "diff",
        "f" | "follow" => "follow",
        "r" | "report" => "report",
        _ => return None,
    })
}

/// The arguments of a short view in the long form: `cpd sl 100` is `log slow --ms 100`.
fn short_arguments(view: &str, rest: &[String]) -> Vec<String> {
    match rest.split_first() {
        Some((ms, tail)) if view == "slow" && ms.parse::<u64>().is_ok() => {
            let mut out = vec!["--ms".to_string(), ms.clone()];
            out.extend(tail.iter().cloned());
            out
        }
        _ => rest.to_vec(),
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let Some((first, rest)) = args.split_first() else {
        return errors(None);
    };
    if let Some(view) = long_view(first) {
        let mut forwarded = vec![view.to_string()];
        forwarded.extend(short_arguments(view, rest));
        let options = logview::parse_args(&forwarded)?;
        return logview::run(Path::new("."), &options);
    }
    match first.as_str() {
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(ExitCode::SUCCESS)
        }
        "-V" | "--version" | "version" => {
            println!("cpd {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        "e" | "err" | "errors" | "problems" => errors(file_arg(rest)?),
        "d" | "diag" => diag::run(file_arg(rest)?.as_deref()),
        "on" => switch(Some(rest.first().map_or("all", String::as_str))),
        "off" => switch(None),
        "path" | "where" => path(),
        "clean" | "clean-logs" => clean::run(&clean::parse_args(rest)?),
        "info" => Ok(info::run(Path::new("."))),
        other if !other.starts_with('-') && Path::new(other).is_file() => {
            errors(Some(PathBuf::from(other)))
        }
        other => Err(format!("unknown command '{other}'")),
    }
}

/// At most one argument: the log file.
fn file_arg(rest: &[String]) -> Result<Option<PathBuf>, String> {
    match rest {
        [] => Ok(None),
        [file] => Ok(Some(PathBuf::from(file))),
        _ => Err("this command takes at most one argument: the log file".into()),
    }
}

/// What went wrong in a log: the report of the Developer panel. Exit code 1 when an error was found,
/// so a script can tell.
fn errors(file: Option<PathBuf>) -> Result<ExitCode, String> {
    let path = diag::pick_file(file.as_deref())?;
    let lines = problems::read_lines(&path, 64 * 1024 * 1024)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let report = problems::scan(&lines);
    println!("{}", path.display());
    print!("{}", problems::render(&report));
    Ok(if report.count(problems::Severity::Error) > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

/// The folder of the program's own files: the logs folder without its last part.
fn support_dir() -> Result<PathBuf, String> {
    let logs = diag::default_log_dir(&|k| std::env::var(k).ok())
        .ok_or("cannot tell where CursorPie keeps its files (no home folder)")?;
    // with CURSORPIE_DEBUG_DIR the logs are somewhere else, but the switch is still in the support
    // folder: look for it where the app does
    if std::env::var("CURSORPIE_DEBUG_DIR").is_ok_and(|d| !d.trim().is_empty()) {
        let unmoved = diag::default_log_dir(&|k| {
            (k != "CURSORPIE_DEBUG_DIR")
                .then(|| std::env::var(k).ok())
                .flatten()
        })
        .ok_or("cannot tell where CursorPie keeps its files (no home folder)")?;
        return unmoved
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "no support folder".into());
    }
    logs.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "no support folder".into())
}

/// Writes (or removes) the `debug` file that the log build reads when it starts.
fn switch(spec: Option<&str>) -> Result<ExitCode, String> {
    let file = support_dir()?.join(logscan::MARKER_FILE);
    match spec {
        Some(spec) => {
            std::fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))
                .map_err(|e| format!("cannot create {}: {e}", file.display()))?;
            std::fs::write(&file, spec)
                .map_err(|e| format!("cannot write {}: {e}", file.display()))?;
            println!(
                "log on from the next start of CursorPie ({spec}): {}",
                file.display()
            );
            println!("only the log build writes it (About, Developer mode)");
        }
        None => {
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("cannot remove {}: {e}", file.display())),
            }
            println!("log off from the next start of CursorPie");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Where the logs are.
fn path() -> Result<ExitCode, String> {
    let dir = diag::default_log_dir(&|k| std::env::var(k).ok())
        .ok_or("cannot tell where the logs are (no home folder)")?;
    println!("{}", dir.display());
    match diag::pick_file(None) {
        Ok(newest) => println!("{}", newest.display()),
        Err(_) => println!("(no log yet: turn it on with cpd on, restart CursorPie and use it)"),
    }
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_stand_for_the_views() {
        for (short, long) in [
            ("s", "summary"),
            ("t", "timeline"),
            ("tl", "timeline"),
            ("g", "grep"),
            ("sl", "slow"),
            ("df", "diff"),
            ("f", "follow"),
            ("r", "report"),
        ] {
            assert_eq!(long_view(short), Some(long));
            assert_eq!(long_view(long), Some(long));
        }
        assert_eq!(long_view("on"), None);
    }

    #[test]
    fn every_short_view_is_one_that_logview_knows() {
        for name in ["s", "t", "g", "sl", "f", "r"] {
            let view = long_view(name).unwrap();
            let args = match view {
                "grep" => vec![view.to_string(), "ui".to_string()],
                _ => vec![view.to_string()],
            };
            assert!(logview::parse_args(&args).is_ok(), "{name}");
        }
        let diff = ["diff".to_string(), "a".to_string(), "b".to_string()];
        assert!(logview::parse_args(&diff).is_ok());
    }

    #[test]
    fn the_help_names_every_command() {
        for word in [
            "cpd d",
            "cpd s",
            "cpd t",
            "cpd g",
            "cpd sl",
            "cpd df",
            "cpd f",
            "cpd r",
            "cpd on",
            "cpd off",
            "cpd path",
            "cpd clean",
            "cpd info",
        ] {
            assert!(HELP.contains(word), "{word}");
        }
    }

    #[test]
    fn a_bare_number_after_sl_is_the_limit() {
        let args = short_arguments("slow", &["100".into(), "log.jsonl".into()]);
        assert_eq!(args, ["--ms", "100", "log.jsonl"]);
        let parsed = logview::parse_args(&[vec!["slow".to_string()], args].concat()).unwrap();
        assert!(matches!(parsed.view, logview::View::Slow { ms: 100 }));
        assert_eq!(
            short_arguments("slow", &["log.jsonl".into()]),
            ["log.jsonl"]
        );
        assert_eq!(short_arguments("grep", &["100".into()]), ["100"]);
    }

    #[test]
    fn a_file_argument_is_at_most_one() {
        assert_eq!(file_arg(&[]), Ok(None));
        assert!(file_arg(&["a".into(), "b".into()]).is_err());
    }
}
