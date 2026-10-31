//! The developer tools of the CursorPie repository, run as `cargo tools <command>`.
//!
//! This is a separate crate of the workspace (not an `example`, not a `bin` of the app) so that
//! nothing here is ever shipped with, or built into, CursorPie, and so that `cargo test` of the app
//! does not run it by accident. `cargo tools` is an alias from `.cargo/config.toml` for
//! `cargo run --quiet --package cursorpie-tools --`.
//!
//! | Command | Module | What it does |
//! |---|---|---|
//! | `ci` | `ci.rs` | runs every check the CI workflow runs |
//! | `check` | `bump.rs` | `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md` agree |
//! | `bump` | `bump.rs` | raises the version and prepares the changelog |
//! | `hooks` | `hooks.rs` | turns the Git hooks of `.githooks/` on or off |
//! | `commit-msg` | `commit_msg.rs` | checks a commit message |
//! | `diag` | `diag.rs` | reads a debug log of the app (`CURSORPIE_DEBUG=1`) and says where the chain broke |
//! | `log` | `logview.rs` | other views of a debug log: `summary`, `timeline`, `grep`, `slow`, `diff`, `follow`, `report` |
//! | `diag-lint` | `lint.rs` | checks the log lines in `src/` against the rules and against what the analyzer reads |
//! | `clean-logs` | `clean.rs` | deletes old debug logs |
//! | `zip` | `zip.rs` | packs the project into a `.zip` file without `target/` and `.git/` |
//! | `info` | `info.rs` | prints the facts a bug report needs (OS, session, tool versions) |
//!
//! The tools use only the standard library, so they build in seconds and work on every OS the
//! project supports. Each one keeps its decisions in pure functions with unit tests; this file
//! only reads the command line and hands over. The rules the tools enforce are written in
//! `CONTRIBUTING.md`.

mod bump;
mod ci;
mod clean;
mod commit_msg;
mod diag;
mod git;
mod hooks;
mod info;
mod lint;
mod logview;
mod zip;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// The usage text.
const USAGE: &str = r#"CursorPie developer tools. Run them as `cargo tools <command>`.

Commands:
  ci [--quick]                 run the checks CI runs (format, clippy, tests, docs, Windows backend,
                               version files); --quick runs only the ones that take seconds
  check [--tag vX.Y.Z]         verify that Cargo.toml, Cargo.lock and CHANGELOG.md agree
                               (with --tag: and that the tag is the release they describe)
  bump <patch|minor|major> [--dry-run] [--commit] [--any-branch] [--since REV]
                               raise the version and prepare the changelog

      patch   fixes only                                0.3.0 -> 0.3.1
      minor   new or changed behavior                   0.3.1 -> 0.4.0
      major   breaking change, or the first stable      0.4.0 -> 1.0.0

      --dry-run      show what would change and change nothing
      --commit       commit as "Release X.Y.Z" and tag vX.Y.Z (needs a clean working tree and the
                     main branch; never pushes)
      --any-branch   allow --commit on a branch other than main
      --since REV    take the changelog from the commits after REV (default: the last v* tag)

  hooks <install|uninstall>    turn the Git hooks of .githooks/ on or off for this clone
  commit-msg <FILE>            check a commit message file (the commit-msg hook calls this)
  diag [FILE]                  read a debug log of the app (start it with CURSORPIE_DEBUG=1) and say
                               where positions stop agreeing; default: the newest log
  log <view> [...]             other ways to read a debug log (default: the newest log)
      summary [FILE]             lines, threads and how often each event was written; names a flood
      timeline [FILE]            one row per press: hold timer, UI lag, window asked and seen
      grep PATTERN [FILE]        the lines of `cat`, `cat:event` or `:event` as readable text
                                 [--field NAME=VALUE] [--after S] [--before S] [--limit N] [--raw]
      slow [--ms N] [FILE]       the timed steps that took N ms or more (default 50), slowest first
      diff BEFORE AFTER          what a change did: findings gone, findings new, events that changed
      follow [FILE]              print the lines of a run that is still going (Ctrl-C to stop)
      report [FILE]              facts and verdict as text to paste into a bug report
  diag-lint                    check the log lines in src/ against the rules of docs/DEBUGGING.md and
                               against what the analyzer reads (part of `ci --quick`)
  clean-logs [--keep N] [--older-than DAYS] [--all] [--dir PATH] [--dry-run]
                               delete old debug logs of the app (default: keep the newest 3)
  zip [-o FILE] [--list] [--no-git]
                               pack the project into a .zip file without target/ and .git/
                               (default file: NAME-VERSION.zip next to the project folder)
  info                         print the facts a bug report needs: OS, session, tool versions
  help                         show this text

Rules and workflow: CONTRIBUTING.md, sections "Development workflow" and "Releasing"."#;

/// What the user asked for.
#[derive(Debug, PartialEq, Eq)]
enum Task {
    /// Print the usage.
    Help,
    /// Run the CI checks, or only the quick ones.
    Ci { quick: bool },
    /// Verify that the version files agree, and optionally that a tag matches them.
    Check { tag: Option<String> },
    /// Raise the version.
    Bump(bump::Level, bump::Options),
    /// Turn the Git hooks on or off.
    Hooks(hooks::Action),
    /// Check the commit message in a file.
    CommitMsg(PathBuf),
    /// Read a debug log of the app: the file, or the newest one.
    Diag(Option<PathBuf>),
    /// Another view of a debug log (summary, timeline, grep, ...).
    Log(logview::Options),
    /// Check the log lines in `src/`.
    DiagLint,
    /// Delete old debug logs.
    CleanLogs(clean::Options),
    /// Pack the project into a zip file.
    Zip(zip::Options),
    /// Print the facts a bug report needs.
    Info,
}

/// Reads the arguments after the program name. `-h` or `--help` anywhere asks for the usage.
fn parse_command<I: IntoIterator<Item = String>>(args: I) -> Result<Task, String> {
    let args: Vec<String> = args.into_iter().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(Task::Help);
    }
    let Some((name, rest)) = args.split_first() else {
        return Ok(Task::Help);
    };
    match name.as_str() {
        "help" => Ok(Task::Help),
        "ci" => parse_ci(rest),
        "check" => parse_check(rest),
        "bump" => bump::parse_args(rest.iter().cloned())
            .map(|(level, options)| Task::Bump(level, options)),
        "hooks" => hooks::parse_args(rest).map(Task::Hooks),
        "commit-msg" => match rest {
            [file] => Ok(Task::CommitMsg(PathBuf::from(file))),
            _ => Err("commit-msg needs one argument: the file with the message".into()),
        },
        "diag" => match rest {
            [] => Ok(Task::Diag(None)),
            [file] => Ok(Task::Diag(Some(PathBuf::from(file)))),
            _ => Err("diag takes at most one argument: the log file".into()),
        },
        "log" => logview::parse_args(rest).map(Task::Log),
        "diag-lint" if rest.is_empty() => Ok(Task::DiagLint),
        "diag-lint" => Err("diag-lint takes no arguments".into()),
        "clean-logs" => clean::parse_args(rest).map(Task::CleanLogs),
        "zip" => zip::parse_args(rest).map(Task::Zip),
        "info" if rest.is_empty() => Ok(Task::Info),
        "info" => Err("info takes no arguments".into()),
        other => Err(format!("unknown command '{other}'")),
    }
}

/// The arguments of `ci`.
fn parse_ci(args: &[String]) -> Result<Task, String> {
    let mut quick = false;
    for arg in args {
        match arg.as_str() {
            "--quick" => quick = true,
            other => return Err(format!("unknown option '{other}' for ci")),
        }
    }
    Ok(Task::Ci { quick })
}

/// The arguments of `check`.
fn parse_check(args: &[String]) -> Result<Task, String> {
    let mut tag = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tag" => {
                tag = Some(
                    args.next()
                        .ok_or("--tag needs a tag, such as v0.4.0")?
                        .clone(),
                )
            }
            other => return Err(format!("unknown option '{other}' for check")),
        }
    }
    Ok(Task::Check { tag })
}

/// The repository root. `at_run_time` is the manifest folder cargo reports while it runs the
/// tools (the `tools/` folder), so a target folder shared between two checkouts never sends the
/// tool to the wrong one. The folder the tools were built in is only the fallback for starting
/// the compiled file by hand. The root is the folder above the manifest folder.
fn repo_root(at_run_time: Option<OsString>, built_in: &str) -> PathBuf {
    let manifest: PathBuf = at_run_time.map_or_else(|| built_in.into(), Into::into);
    manifest
        .parent()
        .map_or_else(|| manifest.clone(), Path::to_path_buf)
}

fn main() -> ExitCode {
    let root = repo_root(
        std::env::var_os("CARGO_MANIFEST_DIR"),
        env!("CARGO_MANIFEST_DIR"),
    );
    let task = match parse_command(std::env::args().skip(1)) {
        Ok(task) => task,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&root, task) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Executes a parsed command.
fn run(root: &Path, task: Task) -> Result<ExitCode, String> {
    match task {
        Task::Help => {
            println!("{USAGE}");
            Ok(ExitCode::SUCCESS)
        }
        Task::Ci { quick } => Ok(ci::run(root, quick)),
        Task::Check { tag } => bump::run_check(root, tag.as_deref()),
        Task::Bump(level, options) => bump::run(root, level, options),
        Task::Hooks(action) => hooks::run(root, action),
        Task::CommitMsg(file) => commit_msg::run(&file),
        Task::Diag(file) => diag::run(file.as_deref()),
        Task::Log(options) => logview::run(root, &options),
        Task::DiagLint => lint::run(root),
        Task::CleanLogs(options) => clean::run(&options),
        Task::Zip(options) => zip::run(root, &options),
        Task::Info => Ok(info::run(root)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Task, String> {
        parse_command(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn the_root_is_the_folder_above_the_tools_folder() {
        assert_eq!(
            repo_root(Some("/work/b/tools".into()), "/work/a/tools"),
            Path::new("/work/b"),
            "the run-time folder wins over the build folder"
        );
        assert_eq!(repo_root(None, "/work/a/tools"), Path::new("/work/a"));
    }

    #[test]
    fn commands_are_told_apart() {
        assert_eq!(parse(&[]), Ok(Task::Help));
        assert_eq!(parse(&["help"]), Ok(Task::Help));
        assert_eq!(
            parse(&["bump", "--help"]),
            Ok(Task::Help),
            "help works anywhere"
        );
        assert_eq!(parse(&["ci"]), Ok(Task::Ci { quick: false }));
        assert_eq!(parse(&["ci", "--quick"]), Ok(Task::Ci { quick: true }));
        assert_eq!(parse(&["check"]), Ok(Task::Check { tag: None }));
        assert_eq!(
            parse(&["check", "--tag", "v0.4.0"]),
            Ok(Task::Check {
                tag: Some("v0.4.0".into())
            })
        );
        assert_eq!(
            parse(&["bump", "minor", "--dry-run"]),
            Ok(Task::Bump(
                bump::Level::Minor,
                bump::Options {
                    dry_run: true,
                    ..bump::Options::default()
                }
            ))
        );
        assert_eq!(
            parse(&["commit-msg", ".git/COMMIT_EDITMSG"]),
            Ok(Task::CommitMsg(".git/COMMIT_EDITMSG".into()))
        );
        assert_eq!(parse(&["info"]), Ok(Task::Info));
        assert!(matches!(parse(&["zip", "--list"]), Ok(Task::Zip(o)) if o.list));
        assert!(matches!(parse(&["clean-logs", "--all"]), Ok(Task::CleanLogs(o)) if o.keep == 0));
        assert_eq!(parse(&["diag"]), Ok(Task::Diag(None)));
        assert_eq!(parse(&["diag-lint"]), Ok(Task::DiagLint));
        assert!(matches!(
            parse(&["log", "timeline"]),
            Ok(Task::Log(o)) if o.view == logview::View::Timeline && o.file.is_none()
        ));
        assert_eq!(
            parse(&["diag", "a.jsonl"]),
            Ok(Task::Diag(Some("a.jsonl".into())))
        );
    }

    #[test]
    fn wrong_arguments_are_errors() {
        assert!(parse(&["deploy"]).is_err());
        assert!(parse(&["ci", "--slow"]).is_err());
        assert!(parse(&["check", "--tag"]).is_err());
        assert!(parse(&["check", "v0.4.0"]).is_err());
        assert!(parse(&["bump"]).is_err(), "a level is needed");
        assert!(parse(&["commit-msg"]).is_err());
        assert!(parse(&["commit-msg", "a", "b"]).is_err());
        assert!(parse(&["hooks"]).is_err());
        assert!(parse(&["hooks", "enable"]).is_err());
        assert!(parse(&["diag", "a", "b"]).is_err());
        assert!(parse(&["diag-lint", "x"]).is_err());
        assert!(parse(&["log"]).is_err(), "a view is needed");
        assert!(parse(&["log", "watch"]).is_err());
        assert!(parse(&["info", "x"]).is_err());
        assert!(parse(&["zip", "--fast"]).is_err());
        assert!(parse(&["clean-logs", "--keep"]).is_err());
    }
}
