//! `cargo tools ci`: runs the checks the CI workflow runs, in the same order, so a change that
//! passes here on your machine passes there too. `.github/workflows/ci.yml` calls this very
//! command, which makes this file the one list of what "the checks pass" means.
//!
//! - `cargo tools ci` runs everything and stops at the first failure.
//! - `cargo tools ci --quick` runs only the steps that take seconds (the `pre-commit` hook).
//!
//! Some steps depend on the OS you run on, because Rust only compiles the code of its own OS
//! (see `CONTRIBUTING.md`, "What each OS can check"): the Windows backend is type-checked with
//! `--features check-windows` everywhere except on Windows, and the docs are built on macOS
//! only, the target docs.rs uses, where the intra-doc links to the macOS backend resolve.
//!
//! On GitHub Actions every step is wrapped in a log group, so a failure is easy to find.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, ExitCode};
use std::time::Instant;

/// What one step does.
enum Action {
    /// Runs `cargo` with these arguments and these extra environment variables.
    Cargo {
        args: &'static [&'static str],
        env: &'static [(&'static str, &'static str)],
    },
    /// The version check of the release tool (`cargo tools check`).
    Versions,
    /// The rules for the log lines in `src/` and what the analyzer reads (`cargo tools diag-lint`).
    DiagLint,
}

/// One line of the checklist.
struct Step {
    /// Shown in the output and in the failure message.
    name: &'static str,
    /// Whether `--quick` runs it too: only steps that take seconds.
    quick: bool,
    action: Action,
}

/// A step that runs `cargo` with `args`.
fn cargo_step(name: &'static str, quick: bool, args: &'static [&'static str]) -> Step {
    Step {
        name,
        quick,
        action: Action::Cargo { args, env: &[] },
    }
}

/// The checklist for the OS this runs on. Warnings are errors: the rule of the project is zero
/// warnings, so a warning that is let through today is a failure nobody remembers tomorrow.
fn steps() -> Vec<Step> {
    let mut steps = vec![
        cargo_step("format", true, &["fmt", "--all", "--", "--check"]),
        Step {
            name: "version files agree",
            quick: true,
            action: Action::Versions,
        },
        Step {
            name: "diagnostic log lines",
            quick: true,
            action: Action::DiagLint,
        },
        cargo_step(
            "clippy",
            false,
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        cargo_step("tests", false, &["test", "--workspace", "--all-targets"]),
        cargo_step("doc tests", false, &["test", "--doc"]),
    ];
    // docs.rs builds for macOS, and only there do the links to the macOS backend resolve
    if cfg!(target_os = "macos") {
        steps.push(Step {
            name: "docs",
            quick: false,
            action: Action::Cargo {
                args: &[
                    "doc",
                    "--workspace",
                    "--no-deps",
                    "--document-private-items",
                ],
                env: &[("RUSTDOCFLAGS", "-D warnings")],
            },
        });
    }
    // the Windows backend is compiled on Windows itself; everywhere else this feature checks it
    if !cfg!(windows) {
        steps.push(cargo_step(
            "Windows backend: clippy",
            false,
            &[
                "clippy",
                "--all-targets",
                "--features",
                "check-windows",
                "--",
                "-D",
                "warnings",
            ],
        ));
        steps.push(cargo_step(
            "Windows backend: tests",
            false,
            &["test", "--features", "check-windows"],
        ));
    }
    steps
}

/// The `cargo` that started the tools (cargo sets `CARGO`), so the checks use the same toolchain.
fn cargo() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())
}

/// Runs one step.
fn execute(root: &Path, action: &Action) -> Result<(), String> {
    match action {
        Action::Cargo { args, env } => {
            println!("      $ cargo {}", args.join(" "));
            let status = Command::new(cargo())
                .args(*args)
                .envs(env.iter().copied())
                .current_dir(root)
                .status()
                .map_err(|e| format!("could not start cargo: {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err(format!("cargo ended with {status}"))
            }
        }
        Action::Versions => {
            let problems = crate::bump::check(root, None)?;
            if problems.is_empty() {
                Ok(())
            } else {
                Err(problems.join("; "))
            }
        }
        Action::DiagLint => {
            let problems = crate::lint::problems(root)?;
            if problems.is_empty() {
                Ok(())
            } else {
                Err(format!(
                    "{} problem(s), run `cargo tools diag-lint` for the list: {}",
                    problems.len(),
                    problems.join("; ")
                ))
            }
        }
    }
}

/// Runs the checklist (only the quick steps when `quick`) and returns the exit code: success when
/// every step passed. The first failure stops the run, because the steps after it would only
/// repeat the same problem.
pub fn run(root: &Path, quick: bool) -> ExitCode {
    let on_github = std::env::var_os("GITHUB_ACTIONS").is_some();
    let steps: Vec<Step> = steps().into_iter().filter(|s| s.quick || !quick).collect();
    let total = steps.len();
    let started = Instant::now();
    for (i, step) in steps.iter().enumerate() {
        let label = format!("[{}/{total}] {}", i + 1, step.name);
        if on_github {
            println!("::group::{label}");
        } else {
            println!("{label}");
        }
        let begun = Instant::now();
        let outcome = execute(root, &step.action);
        if on_github {
            println!("::endgroup::");
        }
        match outcome {
            Ok(()) => println!("      ok ({:.1}s)", begun.elapsed().as_secs_f32()),
            Err(why) => {
                if on_github {
                    println!("::error::{} failed: {why}", step.name);
                }
                println!("      FAILED: {why}");
                let again = if quick { " --quick" } else { "" };
                println!(
                    "\n'{}' failed, so the steps after it did not run. Fix it and run \
                     `cargo tools ci{again}` again.",
                    step.name
                );
                return ExitCode::FAILURE;
            }
        }
    }
    println!(
        "\nAll {total} steps passed in {:.0}s.",
        started.elapsed().as_secs_f32()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_is_a_small_part_that_still_checks_format_and_versions() {
        let all = steps();
        let quick: Vec<&str> = all.iter().filter(|s| s.quick).map(|s| s.name).collect();
        assert!(quick.contains(&"format"));
        assert!(quick.contains(&"version files agree"));
        assert!(quick.contains(&"diagnostic log lines"));
        assert!(quick.len() < all.len(), "the full run does more");
    }

    #[test]
    fn step_names_are_unique() {
        let all = steps();
        let mut names: Vec<&str> = all.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }

    #[test]
    fn warnings_are_errors_in_every_lint_step() {
        for step in steps() {
            if let Action::Cargo { args, env } = &step.action {
                let lints = args.contains(&"clippy");
                let docs = args.contains(&"doc");
                if lints {
                    assert!(
                        args.contains(&"warnings"),
                        "{} must pass -D warnings",
                        step.name
                    );
                }
                if docs {
                    assert!(
                        env.iter()
                            .any(|(k, v)| *k == "RUSTDOCFLAGS" && v.contains("-D warnings")),
                        "{} must deny rustdoc warnings",
                        step.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_windows_backend_is_checked_where_it_is_not_compiled() {
        let names: Vec<&str> = steps().iter().map(|s| s.name).collect();
        assert_eq!(
            names.contains(&"Windows backend: tests"),
            !cfg!(windows),
            "{names:?}"
        );
    }
}
