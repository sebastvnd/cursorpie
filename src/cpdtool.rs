//! `cpd` installed and removed from Settings (About > Developer tools > Terminal command).
//!
//! `cpd` is the small terminal program that reads the diagnostic logs (`tools/src/cpd.rs`, see
//! `docs/DEBUGGING.md`). It is attached to every release as `cpd-macos-universal`,
//! `cpd-windows-x86_64.exe` and `cpd-linux-x86_64`. It used to be a link to the release page: the
//! person had to find the right file, make it executable, put it somewhere and, to get rid of it,
//! remember where. Now one button downloads it, checks it and puts it in the `bin` folder next to
//! `config.json`, and one button removes it again.
//!
//! | What | Where |
//! |---|---|
//! | the program | `<support folder>/bin/cpd` (`cpd.exe` on Windows) |
//! | macOS and Linux only | a link `~/.local/bin/cpd` to it, so `cpd` works where that folder is in `PATH` |
//!
//! Nothing else is touched: no `PATH` edit, no registry, no administrator. The link is only made
//! where nothing is, and only a link that points to this program is ever removed. The download is
//! checked against the checksum of the release (the same check as an update, see
//! [`update::install`](crate::update::install)) and must start (`cpd --version`) before it replaces
//! anything.

use crate::about::LATEST_RELEASE_API_URL;
use crate::config::app_support_dir;
use crate::msg::{Msg, Notifier};
use crate::platform::{self, Os, http};
use crate::update::install::{InstallError, asset_url, expected_checksum, parse_release, verify};
use crate::update::{CHECK_TIMEOUT_S, MAX_ANSWER_BYTES};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Longest the download takes, in seconds. The program is a few MB.
const DOWNLOAD_TIMEOUT_S: u64 = 120;
/// Biggest file accepted as `cpd`.
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// The folder of the program inside the support folder.
const BIN_DIR: &str = "bin";

/// The file of a release that is `cpd` for a system, `None` when the release has none for it.
pub fn asset_name(os: Os, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        (Os::Mac, _) => Some("cpd-macos-universal"),
        (Os::Windows, "x86_64" | "aarch64") => Some("cpd-windows-x86_64.exe"),
        (Os::Linux, "x86_64") => Some("cpd-linux-x86_64"),
        _ => None,
    }
}

/// Whether the release has a `cpd` for this system at all.
pub fn available_here() -> bool {
    asset_name(platform::OS, std::env::consts::ARCH).is_some()
}

/// The name of the installed file.
fn file_name(os: Os) -> &'static str {
    if matches!(os, Os::Windows) {
        "cpd.exe"
    } else {
        "cpd"
    }
}

/// The folder `cpd` is installed to.
pub fn dir() -> PathBuf {
    app_support_dir().join(BIN_DIR)
}

/// Where `cpd` is installed (whether it is or not).
pub fn path() -> PathBuf {
    dir().join(file_name(platform::OS))
}

/// The installed program, `None` when it is not installed.
pub fn installed() -> Option<PathBuf> {
    let p = path();
    p.is_file().then_some(p)
}

/// What to type in a terminal to run the installed `cpd`, with its whole path so it works before
/// anything is in `PATH`: quoted for the shell of the system.
pub fn run_command(program: &Path) -> String {
    let text = program.display().to_string();
    if matches!(platform::OS, Os::Windows) {
        format!("& \"{text}\"")
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// `~/.local/bin/cpd`, where a link makes `cpd` a command on macOS and Linux.
#[cfg(unix)]
fn link_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| PathBuf::from(h).join(".local").join("bin").join("cpd"))
}

/// Makes the link, unless something is there already (a `cpd` the person put there is theirs).
#[cfg(unix)]
fn add_link(target: &Path) {
    let Some(link) = link_path() else {
        return;
    };
    // anything that is there already stays: our own link from an earlier install, or a `cpd` that
    // the person put there
    if fs::symlink_metadata(&link).is_ok() {
        return;
    }
    if let Some(parent) = link.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = std::os::unix::fs::symlink(target, &link);
}

/// Removes the link, only when it points to `target`.
#[cfg(unix)]
fn remove_link(target: &Path) {
    let Some(link) = link_path() else {
        return;
    };
    if fs::read_link(&link).is_ok_and(|to| to == target) {
        let _ = fs::remove_file(&link);
    }
}

#[cfg(not(unix))]
fn add_link(_target: &Path) {}

#[cfg(not(unix))]
fn remove_link(_target: &Path) {}

#[cfg(unix)]
fn make_executable(file: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(file, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_file: &Path) -> io::Result<()> {
    Ok(())
}

/// Whether the program starts on this system: `cpd --version` ends well.
fn starts(program: &Path) -> bool {
    Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn disk_words(e: &io::Error) -> String {
    if platform::is_disk_full(e) {
        "There is not enough free disk space.".to_string()
    } else {
        format!("The file could not be written: {e}")
    }
}

/// Downloads `cpd` from the newest release, checks it and puts it in place. Blocks (a download):
/// call it from a worker thread, see [`spawn_install`].
///
/// # Errors
///
/// A sentence in plain words: no package for this system, no connection, a download that is not the
/// file the release published, a program that does not start, a disk that is full. Whatever was
/// installed before stays as it was.
pub fn install() -> Result<PathBuf, String> {
    let os = platform::OS;
    let name = asset_name(os, std::env::consts::ARCH)
        .ok_or_else(|| "There is no cpd for this system yet.".to_string())?;
    let say = |e: InstallError| e.explain().to_string();

    let json = http::get_text(LATEST_RELEASE_API_URL, CHECK_TIMEOUT_S, MAX_ANSWER_BYTES)
        .map_err(|e| say(InstallError::from(e)))?;
    let release = parse_release(&json)
        .ok_or_else(|| "The answer of the release page could not be read.".to_string())?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| {
            format!(
                "The newest release ({}) has no {name} yet.",
                release.version
            )
        })?;
    let expected = expected_checksum(&release, asset).map_err(say)?;

    let folder = dir();
    fs::create_dir_all(&folder).map_err(|e| disk_words(&e))?;
    let target = folder.join(file_name(os));
    let staging = folder.join(format!(".{}.download", file_name(os)));
    let _ = fs::remove_file(&staging);

    let url = asset_url(&release, name);
    http::download(&url, &staging, DOWNLOAD_TIMEOUT_S, MAX_BYTES)
        .map_err(|e| say(InstallError::from(e)))?;
    if let Err(e) = verify(&staging, asset.size, &expected) {
        let _ = fs::remove_file(&staging);
        return Err(say(e));
    }
    let ready = make_executable(&staging).is_ok() && starts(&staging);
    if !ready {
        let _ = fs::remove_file(&staging);
        return Err(say(InstallError::SelfTest));
    }
    if let Err(e) = fs::rename(&staging, &target) {
        let _ = fs::remove_file(&staging);
        return Err(disk_words(&e));
    }
    add_link(&target);
    diag::log!("ui", "cpd_installed");
    Ok(target)
}

/// Removes the installed `cpd` (and the link to it). Quick: no worker thread needed.
///
/// # Errors
///
/// A sentence in plain words when the file could not be deleted (it is running in a terminal on
/// Windows, say).
pub fn uninstall() -> Result<PathBuf, String> {
    let target = path();
    match fs::remove_file(&target) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(format!(
                "cpd could not be removed (is it running in a terminal?): {e}"
            ));
        }
    }
    remove_link(&target);
    // the folder goes too when nothing else is in it
    let _ = fs::remove_dir(dir());
    diag::log!("ui", "cpd_removed");
    Ok(target)
}

/// Runs [`install`] on a worker thread; the result arrives as [`Msg::CpdDone`].
pub fn spawn_install(notifier: Notifier) {
    let started = std::thread::Builder::new()
        .name("cursorpie-cpd".into())
        .spawn({
            let notifier = notifier.clone();
            move || {
                let result = install();
                notifier.send(Msg::CpdDone {
                    installed: true,
                    result,
                });
            }
        });
    if started.is_err() {
        notifier.send(Msg::CpdDone {
            installed: true,
            result: Err("The download could not be started.".to_string()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_system_with_a_cpd_in_the_release_gets_its_file() {
        assert_eq!(asset_name(Os::Mac, "aarch64"), Some("cpd-macos-universal"));
        assert_eq!(asset_name(Os::Mac, "x86_64"), Some("cpd-macos-universal"));
        assert_eq!(
            asset_name(Os::Windows, "x86_64"),
            Some("cpd-windows-x86_64.exe")
        );
        assert_eq!(asset_name(Os::Linux, "x86_64"), Some("cpd-linux-x86_64"));
        assert_eq!(asset_name(Os::Linux, "aarch64"), None);
    }

    #[test]
    fn the_names_are_the_ones_the_release_workflow_builds() {
        let ci = include_str!("../.github/workflows/ci.yml");
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let arch = if matches!(os, Os::Mac) {
                "aarch64"
            } else {
                "x86_64"
            };
            let name = asset_name(os, arch).expect("a name");
            assert!(ci.contains(name), "{name} is not built by ci.yml");
        }
    }

    #[test]
    fn the_command_has_the_whole_path_and_survives_spaces_and_quotes() {
        let shown = run_command(Path::new(
            "/Users/me/Library/Application Support/CursorPie/bin/cpd",
        ));
        if matches!(platform::OS, Os::Windows) {
            assert!(shown.starts_with("& \""));
        } else {
            assert_eq!(
                shown,
                "'/Users/me/Library/Application Support/CursorPie/bin/cpd'"
            );
            assert_eq!(run_command(Path::new("/a'b/cpd")), "'/a'\\''b/cpd'");
        }
    }

    #[test]
    fn a_file_that_does_not_start_is_not_a_cpd() {
        assert!(!starts(Path::new("/definitely/not/a/program")));
    }
}
