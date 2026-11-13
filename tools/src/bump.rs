//! `cargo tools bump` and `cargo tools check`: the release tool. It raises the version of
//! CursorPie and prepares the changelog, so nobody edits those files by hand.
//!
//! ```sh
//! cargo tools bump patch            # 0.3.0 -> 0.3.1  (fixes only)
//! cargo tools bump minor            # 0.3.1 -> 0.4.0  (new or changed behavior)
//! cargo tools bump major            # 0.4.0 -> 1.0.0  (breaking change, or the first stable)
//! cargo tools bump minor --dry-run  # show what would change, change nothing
//! cargo tools bump minor --commit   # also commit "Release 0.4.0" and tag v0.4.0 (from main)
//! cargo tools check                 # verify that Cargo.toml, Cargo.lock and CHANGELOG.md agree
//! cargo tools check --tag v0.4.0    # ... and that this tag is the release the files describe
//! ```
//!
//! What it changes: `version` in `Cargo.toml`, the `cursorpie` entry of `Cargo.lock` (when the file
//! exists) and the top of `CHANGELOG.md`. Everything else that carries the version reads it from
//! `Cargo.toml` (`about::VERSION`, `scripts/bundle.sh`, the Windows "Apps & features" entry).
//!
//! The changelog entry comes from, in this order:
//! 1. the notes written under `## Unreleased` (they win: a person wrote them for readers);
//! 2. otherwise the subjects of the commits since the last `v*` tag (`--since REV` picks another
//!    starting point), which is why commit subjects are written as imperative, user-facing lines
//!    (the `commit-msg` hook checks that, see `commit_msg`).
//!
//! A fresh, empty `## Unreleased` section is left on top for the notes of the next release, so a
//! contributor always has a place to write to and `check` can tell a hand-edited version from a
//! release: the newest *released* section must be the version of `Cargo.toml`.
//!
//! The tool refuses to start when the three files already disagree, and (with `--commit`) when
//! the working tree is dirty, when the branch is not `main` (`--any-branch` overrides this) or
//! when the tag exists. It writes all files or none: a failed write puts the earlier ones back.
//!
//! The tool cannot do some things, and it ends every run by listing them. See
//! `CONTRIBUTING.md`, section "Releasing", for the rules and the full checklist.
//!
//! Like the rest of `tools/`, it uses only the standard library on purpose (the project takes no
//! new dependencies) and keeps every decision in pure functions that the tests at the bottom
//! cover.

use std::fmt;
use std::path::Path;
use std::process::ExitCode;

use crate::git::git;

/// File names, relative to the repository root.
const CARGO_TOML: &str = "Cargo.toml";
const CARGO_LOCK: &str = "Cargo.lock";
const CHANGELOG: &str = "CHANGELOG.md";
const README: &str = "README.md";
/// The package whose `Cargo.lock` entry is updated.
const PACKAGE: &str = "cursorpie";
/// Heading of the section where notes collect between releases.
const UNRELEASED: &str = "## Unreleased";
/// The branch releases are cut from. `--commit` refuses any other, unless `--any-branch` is given.
const RELEASE_BRANCH: &str = "main";

// ---------------------------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------------------------

/// A plain `MAJOR.MINOR.PATCH` version. Pre-release and build suffixes are not used by this
/// project, so they are rejected rather than half understood.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

/// How far a release moves the version. The rules are in `CONTRIBUTING.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Fixes only: nothing new, nothing changed for the user.
    Patch,
    /// New or changed behavior. Below 1.0.0 this is also where a breaking change goes.
    Minor,
    /// A breaking change from 1.0.0 on, and the step to 1.0.0 itself.
    Major,
}

impl Version {
    /// Parses `1.2.3`.
    fn parse(text: &str) -> Result<Version, String> {
        let text = text.trim();
        if text.contains(['-', '+']) {
            return Err(format!(
                "'{text}' has a pre-release or build suffix, which this project does not use"
            ));
        }
        let parts: Vec<&str> = text.split('.').collect();
        let [major, minor, patch] = parts.as_slice() else {
            return Err(format!("'{text}' is not MAJOR.MINOR.PATCH"));
        };
        let number = |s: &str| {
            s.parse::<u32>()
                .map_err(|_| format!("'{text}' is not MAJOR.MINOR.PATCH"))
        };
        Ok(Version {
            major: number(major)?,
            minor: number(minor)?,
            patch: number(patch)?,
        })
    }

    /// The version after this one, one step of `level`; the lower parts reset to zero.
    fn bump(self, level: Level) -> Version {
        match level {
            Level::Patch => Version {
                patch: self.patch + 1,
                ..self
            },
            Level::Minor => Version {
                minor: self.minor + 1,
                patch: 0,
                ..self
            },
            Level::Major => Version {
                major: self.major + 1,
                minor: 0,
                patch: 0,
            },
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ---------------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------------

/// The flags of a bump.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Show the plan, write nothing.
    pub dry_run: bool,
    /// Commit the result and tag it.
    pub commit: bool,
    /// Allow `--commit` on a branch other than `main`.
    pub any_branch: bool,
    /// Where the generated changelog starts, instead of the last `v*` tag.
    pub since: Option<String>,
}

/// Reads the arguments that follow `bump`.
///
/// # Errors
/// On an unknown option, a missing level, or more than one level.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<(Level, Options), String> {
    let mut word: Option<String> = None;
    let mut options = Options::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--commit" => options.commit = true,
            "--any-branch" => options.any_branch = true,
            "--since" => {
                let rev = args.next().ok_or("--since needs a commit, tag or branch")?;
                options.since = Some(rev);
            }
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other => {
                if word.replace(other.to_string()).is_some() {
                    return Err("give one of patch, minor or major, not several".into());
                }
            }
        }
    }
    let level = match word.as_deref() {
        Some("patch") => Level::Patch,
        Some("minor") => Level::Minor,
        Some("major") => Level::Major,
        Some(other) => return Err(format!("'{other}' is not patch, minor or major")),
        None => return Err("say how far to move the version: patch, minor or major".into()),
    };
    Ok((level, options))
}

// ---------------------------------------------------------------------------------------------
// Editing the files (pure: text in, text out)
// ---------------------------------------------------------------------------------------------

/// The `version` of the `[package]` table of `Cargo.toml`.
fn package_version(cargo_toml: &str) -> Result<Version, String> {
    let mut in_package = false;
    for line in cargo_toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
        } else if in_package && let Some(v) = version_value(t) {
            return Version::parse(v);
        }
    }
    Err("no `version = \"...\"` in the [package] table of Cargo.toml".into())
}

/// `1.2.3` from a line `version = "1.2.3"` (and `None` for any other line, including
/// `rust-version`).
fn version_value(line: &str) -> Option<&str> {
    let (key, value) = line.split_once('=')?;
    if key.trim() != "version" {
        return None;
    }
    let value = value.trim().strip_prefix('"')?;
    Some(&value[..value.find('"')?])
}

/// `line` with the quoted version replaced, keeping the indentation and any trailing comment.
fn with_version(line: &str, new: Version) -> String {
    let open = line.find('"').map_or(line.len(), |i| i + 1);
    let close = line[open..].find('"').map_or(line.len(), |i| open + i);
    format!("{}{new}{}", &line[..open], &line[close..])
}

/// `Cargo.toml` with the package version replaced.
fn set_package_version(cargo_toml: &str, new: Version) -> Result<String, String> {
    let mut in_package = false;
    let mut done = false;
    let lines: Vec<String> = cargo_toml
        .lines()
        .map(|line| {
            let t = line.trim();
            if t.starts_with('[') {
                in_package = t == "[package]";
            } else if in_package && !done && version_value(t).is_some() {
                done = true;
                return with_version(line, new);
            }
            line.to_string()
        })
        .collect();
    if !done {
        return Err("no version line to replace in Cargo.toml".into());
    }
    Ok(join_lines(&lines, cargo_toml))
}

/// The version recorded for `name` in a `Cargo.lock`.
fn lock_version(lock: &str, name: &str) -> Option<Version> {
    let lines: Vec<&str> = lock.lines().collect();
    let i = lock_entry(&lines, name)?;
    Version::parse(version_value(lines.get(i + 1)?.trim())?).ok()
}

/// Index of the `name = "..."` line of the `[[package]]` entry called `name`.
fn lock_entry(lines: &[&str], name: &str) -> Option<usize> {
    let wanted = format!("name = \"{name}\"");
    (1..lines.len()).find(|&i| lines[i].trim() == wanted && lines[i - 1].trim() == "[[package]]")
}

/// `Cargo.lock` with the version of `name` replaced, or `None` when it has no such entry.
fn set_lock_version(lock: &str, name: &str, new: Version) -> Option<String> {
    let lines: Vec<&str> = lock.lines().collect();
    let i = lock_entry(&lines, name)? + 1;
    version_value(lines.get(i)?.trim())?;
    let edited: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(n, l)| {
            if n == i {
                with_version(l, new)
            } else {
                l.to_string()
            }
        })
        .collect();
    Some(join_lines(&edited, lock))
}

/// Joins lines and keeps the trailing newline `original` had.
fn join_lines(lines: &[String], original: &str) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Where the notes of a release came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Notes {
    /// Written by a person under `## Unreleased`.
    Written,
    /// Built from commit subjects: needs a human read before release.
    Generated,
}

/// Turns commit subjects into changelog bullets. Release commits and merges say nothing a reader
/// needs, and a repeated subject (a reverted-and-redone change) is listed once.
fn notes_from_subjects(subjects: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in subjects.iter().map(|s| s.trim()) {
        let skip = s.is_empty() || s.starts_with("Release ") || s.starts_with("Merge ");
        let bullet = format!("- {s}");
        if !skip && !out.contains(&bullet) {
            out.push(bullet);
        }
    }
    out
}

/// Index of the line after the `## ` heading at `from` where its section ends.
fn section_end(lines: &[&str], from: usize) -> usize {
    (from + 1..lines.len())
        .find(|&i| lines[i].starts_with("## "))
        .unwrap_or(lines.len())
}

/// Index of the `## Unreleased` heading.
fn unreleased_heading(lines: &[&str]) -> Option<usize> {
    lines
        .iter()
        .position(|l| l.trim().eq_ignore_ascii_case(UNRELEASED))
}

/// Whether someone wrote notes under `## Unreleased`.
fn has_written_notes(changelog: &str) -> bool {
    let lines: Vec<&str> = changelog.lines().collect();
    unreleased_heading(&lines).is_some_and(|i| {
        lines[i + 1..section_end(&lines, i)]
            .iter()
            .any(|l| !l.trim().is_empty())
    })
}

/// Pushes an empty line unless `out` is empty or already ends in one, so a heading never sticks
/// to the line above it.
fn ensure_blank(out: &mut Vec<String>) {
    if out.last().is_some_and(|l| !l.is_empty()) {
        out.push(String::new());
    }
}

/// `CHANGELOG.md` with the release `new` on top, below a fresh empty `## Unreleased` heading
/// where the notes of the next release collect.
///
/// Written notes under `## Unreleased` become the section of `new`. Without them, `generated`
/// bullets are used. With neither there is nothing honest to publish, so it is an error.
fn release_changelog(
    changelog: &str,
    new: Version,
    generated: &[String],
) -> Result<(String, Notes), String> {
    let heading = format!("## {new}");
    let lines: Vec<&str> = changelog.lines().collect();
    if lines.iter().any(|l| l.trim() == heading) {
        return Err(format!("CHANGELOG.md already has a section '{heading}'"));
    }
    let unreleased = unreleased_heading(&lines);
    let written = has_written_notes(changelog);

    let mut out: Vec<String> = Vec::new();
    let notes = if let (true, Some(i)) = (written, unreleased) {
        out.extend(lines[..i].iter().map(|l| l.to_string()));
        ensure_blank(&mut out);
        out.push(UNRELEASED.to_string());
        out.push(String::new());
        out.push(heading);
        out.extend(lines[i + 1..].iter().map(|l| l.to_string()));
        Notes::Written
    } else {
        if generated.is_empty() {
            return Err(
                "CHANGELOG.md has no notes for this release: write them under '## Unreleased', \
                 or let the tool list the commits since the last v* tag (see --since)"
                    .into(),
            );
        }
        // an empty "## Unreleased" heading is replaced, otherwise the entry goes below the title
        let (start, rest) = match unreleased {
            Some(i) => (i, section_end(&lines, i)),
            None => {
                let first = lines.iter().position(|l| l.starts_with("## "));
                let at = first.unwrap_or(lines.len());
                (at, at)
            }
        };
        out.extend(lines[..start].iter().map(|l| l.to_string()));
        ensure_blank(&mut out);
        out.push(UNRELEASED.to_string());
        out.push(String::new());
        out.push(heading);
        out.push(String::new());
        out.extend(generated.iter().cloned());
        out.push(String::new());
        out.extend(lines[rest..].iter().map(|l| l.to_string()));
        Notes::Generated
    };
    Ok((join_lines(&out, changelog), notes))
}

/// The heading, without the `## `, of the newest released section of the changelog: the first
/// `## ` section that is not `## Unreleased`.
fn released_heading(changelog: &str) -> Option<&str> {
    changelog
        .lines()
        .filter_map(|l| l.strip_prefix("## "))
        .map(str::trim)
        .find(|h| !h.eq_ignore_ascii_case("Unreleased"))
}

/// Line number (1-based) of a sentence in the README that announces `current` as the version
/// ("**Version 0.3.0 adds ...**"). Text like that is written for readers and goes stale on its
/// own, which a tool cannot fix.
fn stale_readme_line(readme: &str, current: Version) -> Option<usize> {
    let needle = format!("Version {current}");
    readme
        .lines()
        .position(|l| l.contains(&needle))
        .map(|i| i + 1)
}

/// The things only a person can do or judge, for a release of `level` from `current`.
/// Printed at the end of every run.
fn manual_steps(
    level: Level,
    notes: Notes,
    readme_line: Option<usize>,
    committed: bool,
) -> Vec<String> {
    let mut steps = Vec::new();
    if notes == Notes::Generated {
        steps.push(
            "WARNING: the CHANGELOG entry was generated from commit subjects. Read it, merge \
             duplicates, and reword lines a user would not understand."
                .to_string(),
        );
    }
    if let Some(line) = readme_line {
        steps.push(format!(
            "WARNING: README.md line {line} still announces the old version. Rewrite or remove \
             that sentence by hand."
        ));
    }
    steps.push(
        "Config format: if this release changes config.json in a way an old reader would \
         misread (a renamed or re-typed field), raise CONFIG_VERSION in src/config/mod.rs and \
         write a migration (guides::config_format). Adding a setting that has a Default needs \
         no change. The tool cannot tell."
            .to_string(),
    );
    match level {
        Level::Patch => {}
        Level::Minor => steps.push(
            "Check that README.md describes the new or changed behavior and settings.".to_string(),
        ),
        Level::Major => steps.push(
            "Breaking change: make sure the CHANGELOG entry says exactly what breaks and how to \
             migrate, and check README.md."
                .to_string(),
        ),
    }
    if !committed {
        steps.push(
            "Commit the changed files and tag the commit vX.Y.Z (or run again with --commit)."
                .to_string(),
        );
    }
    steps.push(
        "Build on each OS (scripts/bundle.sh, scripts/package-windows.ps1), run the manual \
         checklists (docs/developer/WINDOWS_MIGRATION.md, docs/developer/UI_DESIGN.md) and attach the .app (zipped) \
         and CursorPie-windows.zip to a GitHub release. Push with `git push --follow-tags`."
            .to_string(),
    );
    steps
}

// ---------------------------------------------------------------------------------------------
// Doing it
// ---------------------------------------------------------------------------------------------

/// What a bump did (or, with `--dry-run`, would do).
struct Report {
    current: Version,
    new: Version,
    notes: Notes,
    changed: Vec<&'static str>,
    manual: Vec<String>,
}

/// Plans the bump from the files under `root` and, unless `dry_run`, writes them. `subjects` lists
/// the commit subjects since the last release; it is only called when the changelog needs it.
fn bump(
    root: &Path,
    level: Level,
    dry_run: bool,
    committed: bool,
    subjects: impl FnOnce() -> Result<Vec<String>, String>,
) -> Result<Report, String> {
    let read =
        |name: &str| std::fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"));
    let cargo_toml = read(CARGO_TOML)?;
    let current = package_version(&cargo_toml)?;
    let new = current.bump(level);

    let changelog = read(CHANGELOG)?;
    // no lock file in the checkout: cargo writes it on the next build
    let lock = read(CARGO_LOCK).ok();
    let problems = disagreements(current, lock.as_deref(), &changelog);
    if !problems.is_empty() {
        return Err(format!(
            "the files disagree, so a release would build on a wrong base. Fix this first:\n  {}",
            problems.join("\n  ")
        ));
    }
    // commits are only read when no notes were written, so a plain run needs no git history
    let generated = if has_written_notes(&changelog) {
        Vec::new()
    } else {
        notes_from_subjects(&subjects()?)
    };
    let (new_changelog, notes) = release_changelog(&changelog, new, &generated)?;

    let new_toml = set_package_version(&cargo_toml, new)?;
    let new_lock = lock
        .as_deref()
        .and_then(|text| set_lock_version(text, PACKAGE, new));

    let mut files: Vec<(&'static str, &str)> = vec![(CARGO_TOML, new_toml.as_str())];
    if let Some(text) = &new_lock {
        files.push((CARGO_LOCK, text.as_str()));
    }
    files.push((CHANGELOG, new_changelog.as_str()));
    let changed: Vec<&'static str> = files.iter().map(|&(name, _)| name).collect();

    if !dry_run {
        write_all(root, &files)?;
    }

    let readme_line = read(README)
        .ok()
        .and_then(|r| stale_readme_line(&r, current));
    Ok(Report {
        current,
        new,
        notes,
        changed,
        manual: manual_steps(level, notes, readme_line, committed),
    })
}

/// What is wrong when `Cargo.toml` (`version`), `Cargo.lock` (`lock`, when there is one) and
/// `CHANGELOG.md` do not tell the same story. Empty means they agree.
fn disagreements(version: Version, lock: Option<&str>, changelog: &str) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(lock) = lock {
        match lock_version(lock, PACKAGE) {
            Some(v) if v == version => {}
            Some(v) => problems.push(format!(
                "Cargo.lock says {v} but Cargo.toml says {version}: \
                 run `cargo build` once to refresh it"
            )),
            None => problems.push("Cargo.lock has no entry for the package".to_string()),
        }
    }
    match released_heading(changelog) {
        Some(h) if h == version.to_string() => {}
        Some(h) => problems.push(format!(
            "the newest released CHANGELOG section is '{h}' but Cargo.toml says {version}: \
             the version was changed by hand, or the notes are missing"
        )),
        None => problems.push("CHANGELOG.md has no released '## X.Y.Z' section".to_string()),
    }
    problems
}

/// What is wrong with releasing `tag` for the files as they are: the tag must name the version of
/// `Cargo.toml`, and no notes may wait under `## Unreleased`, or the release would skip them.
fn tag_problems(version: Version, tag: &str, changelog: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let expected = format!("v{version}");
    if tag != expected {
        problems.push(format!(
            "the tag is '{tag}' but Cargo.toml says {version}: the tag must be '{expected}'"
        ));
    }
    if has_written_notes(changelog) {
        problems.push(
            "CHANGELOG.md still has notes under '## Unreleased': a release moves them into its \
             own section, so this tag does not describe what it ships"
                .to_string(),
        );
    }
    problems
}

/// Result of `check`: one line per disagreement. Empty means everything agrees. With `tag`, the
/// tag is checked against the files too (see `tag_problems`).
///
/// # Errors
/// When `Cargo.toml` or `CHANGELOG.md` cannot be read or understood.
pub fn check(root: &Path, tag: Option<&str>) -> Result<Vec<String>, String> {
    let read =
        |name: &str| std::fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"));
    let version = package_version(&read(CARGO_TOML)?)?;
    let lock = read(CARGO_LOCK).ok();
    let changelog = read(CHANGELOG)?;
    let mut problems = disagreements(version, lock.as_deref(), &changelog);
    if let Some(tag) = tag {
        problems.extend(tag_problems(version, tag, &changelog));
    }
    Ok(problems)
}

/// Writes every `(file name, text)` pair under `root`. The old contents are read first, so a file
/// that cannot be read stops the run before anything changed; when a write fails, the files
/// written before it are put back as they were. A failed release never leaves a half-done one.
fn write_all(root: &Path, files: &[(&str, &str)]) -> Result<(), String> {
    let mut before = Vec::new();
    for &(name, _) in files {
        let old = std::fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"))?;
        before.push(old);
    }
    apply(files, &before, |name, text| {
        std::fs::write(root.join(name), text)
    })
}

/// Calls `write` for each file in order. When one call fails, the files before it are written
/// again with their old contents (`before`, in the same order) and the error is returned.
fn apply(
    files: &[(&str, &str)],
    before: &[String],
    mut write: impl FnMut(&str, &str) -> std::io::Result<()>,
) -> Result<(), String> {
    for (i, &(name, text)) in files.iter().enumerate() {
        if let Err(e) = write(name, text) {
            for (k, &(earlier, _)) in files[..i].iter().enumerate() {
                // best effort: there is nothing more to do when even the restore fails
                let _ = write(earlier, &before[k]);
            }
            return Err(format!("{name}: {e}"));
        }
    }
    Ok(())
}

/// Commit subjects since `since`, or since the last `v*` tag, or since the commit that put the
/// current version into `Cargo.toml` (for a repository that has never tagged a release).
fn subjects_since(
    root: &Path,
    since: Option<&str>,
    current: Version,
) -> Result<Vec<String>, String> {
    let start = match since {
        Some(rev) => rev.to_string(),
        None => git(
            root,
            &["describe", "--tags", "--abbrev=0", "--match", "v[0-9]*"],
        )
        .or_else(|_| {
            let needle = format!("-Sversion = \"{current}\"");
            git(
                root,
                &["log", "-1", "--format=%H", &needle, "--", CARGO_TOML],
            )
            .and_then(|h| {
                if h.is_empty() {
                    Err(String::new())
                } else {
                    Ok(h)
                }
            })
        })
        .map_err(|_| {
            "cannot tell where the last release was (no v* tag): pass --since REV, or write \
                 the notes under '## Unreleased' in CHANGELOG.md"
                .to_string()
        })?,
    };
    let range = format!("{start}..HEAD");
    let log = git(root, &["log", "--no-merges", "--format=%s", &range])?;
    Ok(log.lines().map(str::to_string).collect())
}

/// Commits the changed files and tags the commit. `Cargo.lock` is added only when the repository
/// tracks it: a project that keeps it out of version control must not get it by surprise.
fn commit_release(root: &Path, report: &Report) -> Result<(), String> {
    let tag = format!("v{}", report.new);
    let tracked = |name: &str| git(root, &["ls-files", "--error-unmatch", name]).is_ok();
    let mut add = vec!["add", "--"];
    add.extend(
        report
            .changed
            .iter()
            .copied()
            .filter(|name| *name != CARGO_LOCK || tracked(CARGO_LOCK)),
    );
    git(root, &add)?;
    git(root, &["commit", "-m", &format!("Release {}", report.new)])?;
    git(
        root,
        &[
            "tag",
            "-a",
            &tag,
            "-m",
            &format!("CursorPie {}", report.new),
        ],
    )?;
    Ok(())
}

/// The reason `--commit` must stop on `branch`, if there is one. A detached head reports `HEAD`.
fn branch_problem(branch: &str, any_branch: bool) -> Option<String> {
    if any_branch || branch == RELEASE_BRANCH {
        return None;
    }
    Some(format!(
        "releases are cut from '{RELEASE_BRANCH}' but this is '{branch}': switch branch, or pass \
         --any-branch if you are sure"
    ))
}

/// Raises the version as `level` and `options` ask, and prints what was done.
///
/// # Errors
/// When a precondition fails (dirty tree, wrong branch, existing tag, disagreeing files, no notes)
/// or a file or `git` call fails. Nothing is written before the preconditions pass.
pub fn run(root: &Path, level: Level, options: Options) -> Result<ExitCode, String> {
    let committing = options.commit && !options.dry_run;
    if committing {
        let dirty = git(root, &["status", "--porcelain", "--untracked-files=no"])?;
        if !dirty.is_empty() {
            return Err(
                "--commit needs a clean working tree, so the release commit holds only \
                 the release. Commit or stash your changes first."
                    .into(),
            );
        }
        let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        if let Some(problem) = branch_problem(&branch, options.any_branch) {
            return Err(problem);
        }
    }
    let cargo_toml =
        std::fs::read_to_string(root.join(CARGO_TOML)).map_err(|e| format!("{CARGO_TOML}: {e}"))?;
    let current = package_version(&cargo_toml)?;
    if committing {
        let tag = format!("v{}", current.bump(level));
        if !git(root, &["tag", "--list", &tag])?.is_empty() {
            return Err(format!(
                "the tag {tag} already exists: a released version is never reused"
            ));
        }
    }
    let report = bump(root, level, options.dry_run, committing, || {
        subjects_since(root, options.since.as_deref(), current)
    })?;
    if committing {
        commit_release(root, &report)?;
    }
    print_report(&report, options.dry_run, committing);
    Ok(ExitCode::SUCCESS)
}

/// Prints the result of `check` and picks the exit code: success only when everything agrees.
///
/// # Errors
/// When the files cannot be read (see [`check`]).
pub fn run_check(root: &Path, tag: Option<&str>) -> Result<ExitCode, String> {
    let problems = check(root, tag)?;
    if problems.is_empty() {
        println!("Cargo.toml, Cargo.lock and CHANGELOG.md agree.");
        return Ok(ExitCode::SUCCESS);
    }
    problems.iter().for_each(|p| println!("MISMATCH: {p}"));
    Ok(ExitCode::FAILURE)
}

/// Prints what was done and the manual steps.
fn print_report(report: &Report, dry_run: bool, committed: bool) {
    let verb = if dry_run { "Would change" } else { "Changed" };
    println!("Version {} -> {}", report.current, report.new);
    println!("{verb}: {}", report.changed.join(", "));
    match report.notes {
        Notes::Written => println!(
            "CHANGELOG: your notes under '## Unreleased' are now '## {}'.",
            report.new
        ),
        Notes::Generated => println!("CHANGELOG: entry generated from commit subjects."),
    }
    if committed {
        println!(
            "Committed 'Release {0}' and tagged v{0}. Nothing was pushed.",
            report.new
        );
    }
    if dry_run {
        println!("Dry run: nothing was written.");
    }
    println!("\nStill to do by hand:");
    for (i, step) in report.manual.iter().enumerate() {
        println!("  {}. {step}", i + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn versions_parse_and_print() {
        assert_eq!(v("0.3.0").to_string(), "0.3.0");
        assert_eq!(
            v(" 12.0.7 "),
            Version {
                major: 12,
                minor: 0,
                patch: 7
            }
        );
        for bad in [
            "1",
            "1.2",
            "1.2.3.4",
            "a.b.c",
            "1.2.x",
            "",
            "1.2.3-beta",
            "1.2.3+5",
        ] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn each_level_resets_the_lower_parts() {
        let base = v("0.3.4");
        assert_eq!(base.bump(Level::Patch), v("0.3.5"));
        assert_eq!(base.bump(Level::Minor), v("0.4.0"));
        assert_eq!(base.bump(Level::Major), v("1.0.0"));
        assert_eq!(v("1.9.9").bump(Level::Major), v("2.0.0"));
    }

    #[test]
    fn a_bump_always_moves_forward() {
        let base = v("0.3.4");
        for level in [Level::Patch, Level::Minor, Level::Major] {
            assert!(base.bump(level) > base);
        }
        assert!(v("0.10.0") > v("0.9.9"), "numbers, not text, are compared");
    }

    #[test]
    fn arguments_are_read() {
        let args = |a: &[&str]| parse_args(strings(a));
        assert_eq!(args(&["patch"]), Ok((Level::Patch, Options::default())));
        assert_eq!(
            args(&[
                "minor",
                "--dry-run",
                "--commit",
                "--any-branch",
                "--since",
                "v0.3.0"
            ]),
            Ok((
                Level::Minor,
                Options {
                    dry_run: true,
                    commit: true,
                    any_branch: true,
                    since: Some("v0.3.0".into())
                }
            ))
        );
        assert!(args(&["--commit", "major"]).is_ok(), "flags go anywhere");
        assert!(args(&[]).is_err(), "a level is needed");
        assert!(args(&["huge"]).is_err());
        assert!(args(&["patch", "minor"]).is_err());
        assert!(args(&["patch", "--force"]).is_err());
        assert!(args(&["patch", "--since"]).is_err());
    }

    const TOML: &str = "[package]\nname = \"cursorpie\"\nversion = \"0.3.0\" # keep\nrust-version = \"1.88\"\n\n[dependencies]\nserde = { version = \"1\" }\nversion = \"9.9.9\"\n";

    #[test]
    fn only_the_package_version_is_read_and_replaced() {
        assert_eq!(package_version(TOML), Ok(v("0.3.0")));
        let out = set_package_version(TOML, v("0.4.0")).unwrap();
        assert!(out.contains("version = \"0.4.0\" # keep"), "{out}");
        assert!(out.contains("rust-version = \"1.88\""));
        assert!(
            out.contains("version = \"9.9.9\""),
            "other tables are untouched"
        );
        assert!(out.ends_with('\n'));
        assert_eq!(package_version(&out), Ok(v("0.4.0")));
        assert!(package_version("[dependencies]\nversion = \"1.0.0\"\n").is_err());
        assert!(set_package_version("[package]\nname = \"x\"\n", v("1.0.0")).is_err());
    }

    const LOCK: &str = "version = 4\n\n[[package]]\nname = \"serde\"\nversion = \"1.0.0\"\n\n[[package]]\nname = \"cursorpie\"\nversion = \"0.3.0\"\ndependencies = [\n \"serde\",\n]\n";

    #[test]
    fn the_lock_entry_of_the_package_is_found_and_replaced() {
        assert_eq!(lock_version(LOCK, "cursorpie"), Some(v("0.3.0")));
        assert_eq!(lock_version(LOCK, "serde"), Some(v("1.0.0")));
        let out = set_lock_version(LOCK, "cursorpie", v("0.4.0")).unwrap();
        assert_eq!(lock_version(&out, "cursorpie"), Some(v("0.4.0")));
        assert_eq!(
            lock_version(&out, "serde"),
            Some(v("1.0.0")),
            "other packages are untouched"
        );
        assert!(out.contains("dependencies = ["));
        assert_eq!(set_lock_version(LOCK, "missing", v("1.0.0")), None);
    }

    const LOG: &str =
        "# Changelog\n\n## Unreleased\n\n- Added a thing.\n- Fixed a bug.\n\n## 0.3.0\n\n- Old.\n";

    #[test]
    fn written_notes_become_the_release() {
        let (out, notes) = release_changelog(LOG, v("0.4.0"), &[]).unwrap();
        assert_eq!(notes, Notes::Written);
        assert!(out.contains("## 0.4.0\n\n- Added a thing."), "{out}");
        assert!(!has_written_notes(&out), "the notes moved into the release");
        assert!(
            out.contains("## 0.3.0\n\n- Old."),
            "older sections are untouched"
        );
        assert_eq!(released_heading(&out), Some("0.4.0"));
    }

    #[test]
    fn written_notes_win_over_generated_ones() {
        let gen_ = strings(&["- From a commit"]);
        let (out, notes) = release_changelog(LOG, v("0.4.0"), &gen_).unwrap();
        assert_eq!(notes, Notes::Written);
        assert!(!out.contains("From a commit"));
    }

    #[test]
    fn without_written_notes_the_generated_ones_are_used() {
        let gen_ = strings(&["- Install on Windows", "- Add a sidebar"]);
        let plain = "# Changelog\n\n## 0.3.0\n\n- Old.\n";
        let (out, notes) = release_changelog(plain, v("0.3.1"), &gen_).unwrap();
        assert_eq!(notes, Notes::Generated);
        assert_eq!(
            out,
            "# Changelog\n\n## Unreleased\n\n## 0.3.1\n\n- Install on Windows\n- Add a sidebar\n\n## 0.3.0\n\n- Old.\n"
        );
        // an empty Unreleased heading is replaced, not kept next to the new section
        let empty = "# Changelog\n\n## Unreleased\n\n## 0.3.0\n\n- Old.\n";
        let (out, notes) = release_changelog(empty, v("0.3.1"), &gen_).unwrap();
        assert_eq!(notes, Notes::Generated);
        assert_eq!(out.matches("## Unreleased").count(), 1, "{out}");
        assert_eq!(released_heading(&out), Some("0.3.1"));
    }

    #[test]
    fn a_release_without_any_notes_is_refused() {
        let plain = "# Changelog\n\n## 0.3.0\n\n- Old.\n";
        assert!(release_changelog(plain, v("0.3.1"), &[]).is_err());
        let empty = "# Changelog\n\n## Unreleased\n\n## 0.3.0\n\n- Old.\n";
        assert!(release_changelog(empty, v("0.3.1"), &[]).is_err());
    }

    #[test]
    fn a_version_cannot_be_released_twice() {
        assert!(release_changelog(LOG, v("0.3.0"), &[]).is_err());
    }

    #[test]
    fn commit_subjects_become_bullets() {
        let subjects = strings(&[
            "Fix double-click replay",
            "Release 0.3.0",
            "Merge branch 'x'",
            "",
            "Fix double-click replay",
            "  Add a sidebar ",
        ]);
        assert_eq!(
            notes_from_subjects(&subjects),
            ["- Fix double-click replay", "- Add a sidebar"]
        );
        assert!(notes_from_subjects(&[]).is_empty());
    }

    #[test]
    fn a_stale_readme_sentence_is_found() {
        let readme = "# CursorPie\n\n**Version 0.3.0 adds Windows 10/11 support.**\n";
        assert_eq!(stale_readme_line(readme, v("0.3.0")), Some(3));
        assert_eq!(stale_readme_line(readme, v("0.4.0")), None);
    }

    #[test]
    fn the_manual_steps_fit_the_release() {
        let patch = manual_steps(Level::Patch, Notes::Written, None, true);
        assert!(
            patch.iter().any(|s| s.contains("CONFIG_VERSION")),
            "always asked"
        );
        assert!(!patch.iter().any(|s| s.contains("WARNING")));
        assert!(!patch.iter().any(|s| s.contains("Commit the changed files")));

        let minor = manual_steps(Level::Minor, Notes::Generated, Some(5), false);
        assert!(
            minor
                .iter()
                .any(|s| s.contains("generated from commit subjects"))
        );
        assert!(minor.iter().any(|s| s.contains("README.md line 5")));
        assert!(minor.iter().any(|s| s.contains("new or changed behavior")));
        assert!(minor.iter().any(|s| s.contains("Commit the changed files")));

        let major = manual_steps(Level::Major, Notes::Written, None, true);
        assert!(major.iter().any(|s| s.contains("what breaks")));
    }

    /// A scratch repository layout under the system temp folder, removed when dropped.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str, toml: &str, lock: Option<&str>, log: &str) -> Scratch {
            let dir =
                std::env::temp_dir().join(format!("cursorpie-bump-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(CARGO_TOML), toml).unwrap();
            if let Some(lock) = lock {
                std::fs::write(dir.join(CARGO_LOCK), lock).unwrap();
            }
            std::fs::write(dir.join(CHANGELOG), log).unwrap();
            Scratch(dir)
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.0.join(name)).unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn no_commits() -> Result<Vec<String>, String> {
        panic!("written notes must not need the git history")
    }

    #[test]
    fn a_bump_rewrites_the_three_files_together() {
        let s = Scratch::new("all", TOML, Some(LOCK), LOG);
        let r = bump(&s.0, Level::Minor, false, false, no_commits).unwrap();
        assert_eq!((r.current, r.new), (v("0.3.0"), v("0.4.0")));
        assert_eq!(r.changed, [CARGO_TOML, CARGO_LOCK, CHANGELOG]);
        assert_eq!(package_version(&s.read(CARGO_TOML)), Ok(v("0.4.0")));
        assert_eq!(
            lock_version(&s.read(CARGO_LOCK), "cursorpie"),
            Some(v("0.4.0"))
        );
        assert_eq!(released_heading(&s.read(CHANGELOG)), Some("0.4.0"));
        assert_eq!(
            check(&s.0, None),
            Ok(Vec::new()),
            "the files agree afterwards"
        );
    }

    #[test]
    fn a_dry_run_writes_nothing() {
        let s = Scratch::new("dry", TOML, Some(LOCK), LOG);
        let r = bump(&s.0, Level::Patch, true, false, no_commits).unwrap();
        assert_eq!(r.new, v("0.3.1"));
        assert_eq!(s.read(CARGO_TOML), TOML);
        assert_eq!(s.read(CARGO_LOCK), LOCK);
        assert_eq!(s.read(CHANGELOG), LOG);
    }

    #[test]
    fn a_checkout_without_a_lock_file_still_bumps() {
        let s = Scratch::new("nolock", TOML, None, LOG);
        let r = bump(&s.0, Level::Patch, false, false, no_commits).unwrap();
        assert_eq!(r.changed, [CARGO_TOML, CHANGELOG]);
    }

    #[test]
    fn generated_notes_are_asked_for_only_when_needed() {
        let plain = "# Changelog\n\n## 0.3.0\n\n- Old.\n";
        let s = Scratch::new("gen", TOML, None, plain);
        let r = bump(&s.0, Level::Patch, false, false, || {
            Ok(strings(&["Fix a crash", "Release 0.3.0"]))
        })
        .unwrap();
        assert_eq!(r.notes, Notes::Generated);
        assert!(s.read(CHANGELOG).contains("## 0.3.1\n\n- Fix a crash\n"));
        assert!(r.manual.iter().any(|m| m.contains("WARNING")));
    }

    #[test]
    fn a_failed_bump_leaves_every_file_alone() {
        let plain = "# Changelog\n\n## 0.3.0\n\n- Old.\n";
        let s = Scratch::new("fail", TOML, Some(LOCK), plain);
        let r = bump(&s.0, Level::Patch, false, false, || Err("no tag".into()));
        assert!(r.is_err());
        assert_eq!(s.read(CARGO_TOML), TOML);
        assert_eq!(s.read(CARGO_LOCK), LOCK);
        assert_eq!(s.read(CHANGELOG), plain);
    }

    #[test]
    fn check_reports_each_disagreement() {
        let s = Scratch::new(
            "check",
            TOML,
            Some(LOCK),
            "# Changelog\n\n## 0.2.0\n\n- Old.\n",
        );
        let problems = check(&s.0, None).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("CHANGELOG"));

        let lock = LOCK.replace("0.3.0", "0.2.0");
        let s = Scratch::new("check2", TOML, Some(&lock), LOG);
        let problems = check(&s.0, None).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("Cargo.lock"));
    }

    #[test]
    fn a_fresh_unreleased_section_is_left_for_the_next_notes() {
        let (out, _) = release_changelog(LOG, v("0.4.0"), &[]).unwrap();
        assert!(
            out.starts_with("# Changelog\n\n## Unreleased\n\n## 0.4.0\n"),
            "{out}"
        );
        assert!(!has_written_notes(&out));
        let (again, _) = release_changelog(&out, v("0.4.1"), &strings(&["- Fix"])).unwrap();
        assert_eq!(again.matches("## Unreleased").count(), 1, "{again}");
        assert_eq!(released_heading(&again), Some("0.4.1"));
    }

    #[test]
    fn a_tag_must_name_the_released_version() {
        let released = "# Changelog\n\n## Unreleased\n\n## 0.3.0\n\n- Old.\n";
        assert!(tag_problems(v("0.3.0"), "v0.3.0", released).is_empty());
        assert_eq!(tag_problems(v("0.3.0"), "v0.3.1", released).len(), 1);
        assert_eq!(
            tag_problems(v("0.3.0"), "v0.3.0", LOG).len(),
            1,
            "notes still under Unreleased would not be in the release"
        );
    }

    #[test]
    fn releases_are_cut_from_main_unless_told_otherwise() {
        assert_eq!(branch_problem("main", false), None);
        assert!(branch_problem("feature/x", false).is_some());
        assert!(
            branch_problem("HEAD", false).is_some(),
            "a detached head is not main"
        );
        assert_eq!(branch_problem("feature/x", true), None);
    }

    #[test]
    fn a_bump_refuses_files_that_disagree() {
        let lock = LOCK.replace("0.3.0", "0.2.0");
        let s = Scratch::new("disagree", TOML, Some(&lock), LOG);
        let err = bump(&s.0, Level::Patch, false, false, no_commits)
            .err()
            .unwrap();
        assert!(err.contains("Cargo.lock"), "{err}");
        assert_eq!(s.read(CARGO_TOML), TOML);
        assert_eq!(s.read(CHANGELOG), LOG);
    }

    #[test]
    fn a_file_that_cannot_be_read_stops_the_run_before_any_write() {
        let s = Scratch::new("unreadable", TOML, None, LOG);
        // reading a folder fails
        std::fs::create_dir(s.0.join("blocked")).unwrap();
        let r = write_all(&s.0, &[(CARGO_TOML, "changed"), ("blocked", "x")]);
        assert!(r.is_err());
        assert_eq!(s.read(CARGO_TOML), TOML);
    }

    #[test]
    fn a_failed_write_puts_the_earlier_files_back() {
        let before = strings(&["old-a", "old-b", "old-c"]);
        let mut log: Vec<(String, String)> = Vec::new();
        let r = apply(
            &[("a", "new-a"), ("b", "new-b"), ("c", "new-c")],
            &before,
            |name, text| {
                if name == "c" {
                    return Err(std::io::Error::other("disk full"));
                }
                log.push((name.to_string(), text.to_string()));
                Ok(())
            },
        );
        assert!(r.is_err());
        let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            log,
            [
                pair("a", "new-a"),
                pair("b", "new-b"),
                pair("a", "old-a"),
                pair("b", "old-b")
            ],
            "a and b were written, then restored; c never was"
        );
    }
}
