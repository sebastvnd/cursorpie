//! The stamps of `cargo tools ci`: a step that already passed on these very files is not run again.
//!
//! The fingerprint is a SHA-256 over the CONTENT of every file of the working tree that Git knows or
//! would add (tracked, and untracked but not ignored), each one by path and content hash. Saved edits
//! change it, a `touch`, a branch switch back to the same files or a commit of what is already there do
//! not. Staged or not does not matter, so what the check saw is what is on disk.
//!
//! A step that passes writes an empty file named by the SHA-256 of its name, the fingerprint and the
//! toolchain (`rustc -Vv`, the OS) into `target/cargo-tools-ci/`. The next run finds it and says so
//! instead of running the step. Stamps are only local: `cargo clean` removes them, `--force` ignores
//! them, and on GitHub Actions they are not used (CI has its own, `scripts/ci-fingerprint.sh`).
//!
//! The files that only the website reads are left out, as in `scripts/ci-fingerprint.sh` (a test keeps
//! the two lists the same).

use crate::git::git;
use crate::sha256::{Sha256, hex_of};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

/// Bump when the way a stamp is made changes, so the old ones are not used.
const FORMAT: &str = "1";

/// Stamps older than this are deleted when a new one is written.
const KEEP: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// The files only the website reads: a directory (everything under it), a file, or a pattern with one
/// `*` that matches in the top folder only. The same list as `SITE` in `scripts/ci-fingerprint.sh`.
const SITE: &[&str] = &[
    "index.html",
    "sitemap.xml",
    "robots.txt",
    ".nojekyll",
    "README-WEBSITE.md",
    "google*.html",
    "flowhub",
    ".github/workflows/static.yml",
    ".github/pull_request_template.md",
];

/// Whether `path` (relative, with `/`) is a file that only the website reads.
fn is_site(path: &str) -> bool {
    SITE.iter().any(|entry| match entry.split_once('*') {
        Some((head, tail)) => {
            !path.contains('/')
                && path.len() >= head.len() + tail.len()
                && path.starts_with(head)
                && path.ends_with(tail)
        }
        None => {
            path == *entry
                || path
                    .strip_prefix(entry)
                    .is_some_and(|rest| rest.starts_with('/'))
        }
    })
}

/// The SHA-256 of the files of the working tree (see the top of this file).
///
/// # Errors
/// When `git` cannot list the files or a file cannot be read.
pub fn tree_fingerprint(root: &Path) -> Result<String, String> {
    let listing = git(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let mut paths: Vec<&str> = listing
        .split('\0')
        .filter(|p| !p.is_empty() && !is_site(p))
        .collect();
    paths.sort_unstable();
    paths.dedup();
    let mut all = Sha256::new();
    all.update(format!("cursorpie-ci-tree/{FORMAT}\n").as_bytes());
    for path in paths {
        let full = root.join(path);
        // a tracked file that was deleted is simply not there; a folder is a submodule
        if !full.is_file() {
            continue;
        }
        all.update(path.as_bytes());
        all.update(b"\0");
        all.update(file_hex(&full)?.as_bytes());
        all.update(b"\n");
    }
    Ok(all.finish_hex())
}

/// The SHA-256 of one file, read in pieces.
fn file_hex(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            return Ok(hash.finish_hex());
        }
        hash.update(&buf[..n]);
    }
}

/// What the compiler and the system are: a stamp must not outlive a new Rust.
fn toolchain() -> String {
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let version = Command::new(rustc)
        .arg("-Vv")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_else(|| "rustc unknown".into());
    format!(
        "{version}\n{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

/// The stamps of one run: what the files are now, and where the stamps are kept.
pub struct Stamps {
    dir: PathBuf,
    tree: String,
    toolchain: String,
}

impl Stamps {
    /// Fingerprints the tree of `root`.
    ///
    /// # Errors
    /// When the tree cannot be fingerprinted (no Git, an unreadable file).
    pub fn open(root: &Path) -> Result<Self, String> {
        Ok(Self {
            dir: root.join("target").join("cargo-tools-ci"),
            tree: tree_fingerprint(root)?,
            toolchain: toolchain(),
        })
    }

    /// The first characters of the fingerprint, for the output.
    pub fn short(&self) -> &str {
        &self.tree[..12]
    }

    /// The file that says `step` passed on this tree.
    fn file(&self, step: &str) -> PathBuf {
        let key = hex_of(format!("{FORMAT}\0{step}\0{}\0{}", self.tree, self.toolchain).as_bytes());
        self.dir.join(key)
    }

    /// Whether `step` already passed on exactly these files.
    pub fn passed(&self, step: &str) -> bool {
        self.file(step).is_file()
    }

    /// Notes that `step` passed on these files. A stamp that cannot be written costs only the saving.
    pub fn record(&self, step: &str) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let _ = fs::write(self.file(step), format!("{step}\n"));
        self.prune();
    }

    /// Deletes the stamps that nobody has asked for in two weeks.
    fn prune(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age > KEEP);
            if old {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throw-away Git repository with a few files; removed when dropped.
    struct Repo(PathBuf);

    impl Repo {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("cursorpie-stamp-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(dir.join("src")).unwrap();
            fs::write(dir.join("src/a.rs"), "fn a() {}\n").unwrap();
            fs::write(dir.join("index.html"), "<p>site</p>\n").unwrap();
            fs::write(dir.join(".gitignore"), "/target\n").unwrap();
            git(&dir, &["init", "-q"]).unwrap();
            Self(dir)
        }
        fn fingerprint(&self) -> String {
            tree_fingerprint(&self.0).unwrap()
        }
        fn write(&self, name: &str, text: &str) {
            let path = self.0.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_website_is_not_part_of_the_fingerprint() {
        let repo = Repo::new("site");
        let before = repo.fingerprint();
        repo.write("index.html", "<p>changed</p>\n");
        repo.write("flowhub/catalog.json", "{}\n");
        repo.write("google123.html", "x\n");
        assert_eq!(repo.fingerprint(), before);
    }

    #[test]
    fn a_saved_edit_changes_it_and_putting_it_back_restores_it() {
        let repo = Repo::new("edit");
        let before = repo.fingerprint();
        repo.write("src/a.rs", "fn a() { 1; }\n");
        assert_ne!(repo.fingerprint(), before);
        repo.write("src/a.rs", "fn a() {}\n");
        assert_eq!(repo.fingerprint(), before, "the content is what counts");
    }

    #[test]
    fn a_new_file_counts_but_an_ignored_one_does_not() {
        let repo = Repo::new("new");
        let before = repo.fingerprint();
        repo.write("target/out.bin", "build output\n");
        assert_eq!(repo.fingerprint(), before, "target/ is ignored");
        repo.write("src/b.rs", "fn b() {}\n");
        assert_ne!(repo.fingerprint(), before, "untracked, not ignored");
    }

    #[test]
    fn staging_does_not_change_it() {
        let repo = Repo::new("stage");
        repo.write("src/b.rs", "fn b() {}\n");
        let before = repo.fingerprint();
        git(&repo.0, &["add", "-A"]).unwrap();
        assert_eq!(repo.fingerprint(), before);
    }

    #[test]
    fn a_step_is_stamped_per_name_and_per_tree() {
        let repo = Repo::new("stamps");
        let stamps = Stamps::open(&repo.0).unwrap();
        assert!(!stamps.passed("tests"));
        stamps.record("tests");
        assert!(stamps.passed("tests"));
        assert!(!stamps.passed("clippy"), "another step");
        repo.write("src/a.rs", "fn a() { 2; }\n");
        let changed = Stamps::open(&repo.0).unwrap();
        assert!(!changed.passed("tests"), "other files, so run it again");
        assert!(Stamps::open(&repo.0).unwrap().short().len() == 12);
    }

    #[test]
    fn the_site_patterns_match_what_they_say() {
        assert!(is_site("index.html"));
        assert!(!is_site("docs/index.html"), "only the one in the root");
        assert!(is_site("flowhub"));
        assert!(is_site("flowhub/flows/a.json"));
        assert!(!is_site("flowhubx/a.json"));
        assert!(is_site("google0123abcd.html"));
        assert!(!is_site("docs/google0123.html"));
        assert!(!is_site("src/lib.rs"));
        assert!(!is_site("CHANGELOG.md"));
    }

    #[test]
    fn the_site_list_is_the_one_of_the_ci_fingerprint_script() {
        let script = include_str!("../../scripts/ci-fingerprint.sh");
        let block = script
            .split("SITE=(")
            .nth(1)
            .and_then(|rest| rest.split("\n)").next())
            .expect("the script has a SITE list");
        let mut in_script: Vec<&str> = block
            .lines()
            .filter_map(|l| {
                let l = l.trim().trim_matches('\'');
                l.strip_prefix(":(exclude,glob)")
                    .or_else(|| l.strip_prefix(":(exclude)"))
            })
            .collect();
        let mut here: Vec<&str> = SITE.to_vec();
        in_script.sort_unstable();
        here.sort_unstable();
        assert_eq!(here, in_script, "keep SITE here and in the script the same");
    }
}
