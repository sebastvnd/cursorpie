//! Bringing the files that belong to the program, not to the person, up to date after an update.
//!
//! An update used to replace the program and nothing else. But CursorPie leaves files outside its own
//! executable, written by it for the person's convenience, and every release can change them: the
//! browser extension in `browser-extension/`, the login entry, and on Linux the desktop entry and the
//! GNOME Shell extension. This module rewrites them, once per version, at the first start of a new
//! version (so it makes no difference how the program got there: the update button, Setup, a disk
//! image, `install.sh`, a package manager).
//!
//! | What | Where | Rule |
//! |---|---|---|
//! | browser extension files | `<support folder>/browser-extension` | written again only when the person wrote them there before ([`bridge::extension::refresh`](crate::bridge::extension::refresh)); the running extension is then asked to reload itself (see [`bridge`](crate::bridge)) |
//! | login entry | LaunchAgent, `Run` key, autostart file | written again only when it is on, and only from an ordinary install (the same test as the update's: [`install_target`]), so a build folder never takes the entry over |
//! | Linux user files | `~/.local/share/applications/cursorpie.desktop`, the GNOME Shell extension folder | see `platform::linux::refresh` |
//!
//! **What is never touched** is what is the person's: `config.json` (profiles, slices, flows,
//! settings), its backups, `update_state.json`, the welcome marker. The slices that "Add a slice"
//! offers, the panel templates and the flow kit are compiled into the program, so a new version has
//! them without any file being written.
//!
//! What was written for is the file `program_version` in the support folder: the version of the
//! program that last did this. It is read once at start-up (a few bytes), and when it is the version
//! of this build nothing else is done. A copy from before this file existed has none, which counts
//! as "not done yet", so the first start of this version after an older one does it.
//! Everything is best-effort: a file that cannot be written is left as it was, and the start goes on.

use crate::bridge::extension;
use crate::config::{app_support_dir, read_text_capped};
use crate::platform::{self, linux};
use crate::update::install::install_target;
use std::path::{Path, PathBuf};

/// Name of the file that remembers which version did this last, inside the support folder.
pub const STAMP_FILE: &str = "program_version";

/// Biggest stamp that is read. A real one is a version number.
const MAX_STAMP_BYTES: u64 = 64;

/// What one start did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// This start was the first one of this version, so the files were looked at.
    pub ran: bool,
    /// The browser extension files were written again.
    pub extension_files: bool,
    /// The login entry was written again.
    pub login_entry: bool,
    /// Files of the person's account were written again (Linux).
    pub linux_files: bool,
}

/// Whether the files have to be looked at: the stamp is missing or says another version.
pub fn is_due(stamp: Option<&str>, build: &str) -> bool {
    stamp.map(str::trim) != Some(build)
}

/// What [`run_in`] works on, so a test can point it at a folder of its own.
pub struct Steps<'a> {
    /// Where the extension files were written (see [`extension::default_dir`]).
    pub extension_dir: &'a Path,
    /// The folders of the person's account on Linux; `None` everywhere else.
    pub linux: Option<&'a linux::refresh::UserDirs>,
    /// Writes the login entry again; says whether it did.
    pub login_entry: &'a dyn Fn() -> bool,
}

/// Does what is due for `build`, with the stamp in `support`, and remembers it.
pub fn run_in(support: &Path, build: &str, steps: &Steps) -> Report {
    let stamp = read_text_capped(support.join(STAMP_FILE), MAX_STAMP_BYTES);
    if !is_due(stamp.as_deref(), build) {
        return Report::default();
    }
    let report = Report {
        ran: true,
        extension_files: extension::refresh(steps.extension_dir).unwrap_or(false),
        login_entry: (steps.login_entry)(),
        linux_files: steps
            .linux
            .is_some_and(|dirs| linux::refresh::refresh(dirs).any()),
    };
    // Remembered even when a step failed: a file that cannot be written would only fail again at
    // every start. The person can still press *Show the extension files*. When the support folder
    // does not exist yet there is nothing to look after, and nothing is written.
    if support.is_dir() {
        let _ = std::fs::write(support.join(STAMP_FILE), build);
    }
    report
}

/// The first start of a new version: brings the files of the program up to date (see the top).
/// Cheap when there is nothing to do. Call it before the browser bridge starts, so the extension
/// files are the new ones when the extension connects.
pub fn run() -> Report {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let dirs = if cfg!(target_os = "linux") {
        home.map(|h| linux::refresh::user_dirs(&h, std::env::var("XDG_DATA_HOME").ok().as_deref()))
    } else {
        None
    };
    let report = run_in(
        &app_support_dir(),
        env!("CARGO_PKG_VERSION"),
        &Steps {
            extension_dir: &extension::default_dir(),
            linux: dirs.as_ref(),
            login_entry: &refresh_login_entry,
        },
    );
    if report.ran {
        diag::log!(
            "refresh",
            "done",
            extension = report.extension_files,
            login = report.login_entry,
            linux = report.linux_files
        );
    }
    report
}

/// Writes the login entry again when it is on and this copy is an ordinary install. The entry is
/// written by the program from what it is (its path, on Linux the delay), so after an update it can
/// be out of date; a copy run from a build folder must not take it over.
fn refresh_login_entry() -> bool {
    if !platform::launch_at_login_enabled() {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let local = std::env::var("LOCALAPPDATA").ok();
    if install_target(platform::OS, &exe.to_string_lossy(), local.as_deref()).is_none() {
        return false;
    }
    platform::set_launch_at_login(true).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("cursorpie-refresh-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_stamp_decides_whether_it_is_due() {
        assert!(
            is_due(None, "0.9.0"),
            "a copy from before the stamp existed"
        );
        assert!(is_due(Some("0.8.9"), "0.9.0"));
        assert!(is_due(Some(""), "0.9.0"));
        assert!(!is_due(Some("0.9.0"), "0.9.0"));
        assert!(
            !is_due(Some("0.9.0\n"), "0.9.0"),
            "a trailing newline does not matter"
        );
    }

    #[test]
    fn the_first_start_of_a_version_writes_again_what_was_written_before_and_remembers_it() {
        let support = temp("run");
        let ext = support.join("browser-extension");
        extension::export(&ext).unwrap();
        fs::write(ext.join("chromium/background.js"), "// an older release\n").unwrap();
        fs::write(support.join("config.json"), "{\"mine\":true}").unwrap();
        let logins = Cell::new(0);
        let login = || {
            logins.set(logins.get() + 1);
            true
        };
        let steps = Steps {
            extension_dir: &ext,
            linux: None,
            login_entry: &login,
        };

        let first = run_in(&support, "9.9.9", &steps);
        assert!(first.ran && first.extension_files && first.login_entry && !first.linux_files);
        assert_eq!(
            fs::read_to_string(ext.join("chromium/background.js")).unwrap(),
            extension::BACKGROUND_JS
        );
        assert_eq!(
            fs::read_to_string(support.join(STAMP_FILE)).unwrap(),
            "9.9.9"
        );
        // what is the person's is not touched
        assert_eq!(
            fs::read_to_string(support.join("config.json")).unwrap(),
            "{\"mine\":true}"
        );

        // the same version again: nothing is done
        fs::write(ext.join("chromium/background.js"), "// changed by hand\n").unwrap();
        assert_eq!(run_in(&support, "9.9.9", &steps), Report::default());
        assert_eq!(logins.get(), 1);
        assert_eq!(
            fs::read_to_string(ext.join("chromium/background.js")).unwrap(),
            "// changed by hand\n"
        );

        // the next version does it again
        assert!(run_in(&support, "9.9.10", &steps).ran);
        assert_eq!(logins.get(), 2);
        let _ = fs::remove_dir_all(&support);
    }

    #[test]
    fn nothing_is_created_for_a_person_who_never_exported_the_extension() {
        let support = temp("never");
        let ext = support.join("browser-extension");
        let steps = Steps {
            extension_dir: &ext,
            linux: None,
            login_entry: &|| false,
        };
        let report = run_in(&support, "9.9.9", &steps);
        assert!(report.ran && !report.extension_files);
        assert!(!ext.exists());
        let _ = fs::remove_dir_all(&support);
    }

    #[test]
    fn a_support_folder_that_does_not_exist_yet_is_not_created() {
        let support =
            std::env::temp_dir().join(format!("cursorpie-refresh-{}-missing", std::process::id()));
        let _ = fs::remove_dir_all(&support);
        let steps = Steps {
            extension_dir: &support.join("browser-extension"),
            linux: None,
            login_entry: &|| false,
        };
        let report = run_in(&support, "9.9.9", &steps);
        assert!(report.ran);
        assert!(!support.exists());
    }

    #[test]
    fn the_linux_files_are_part_of_it() {
        let support = temp("linux-support");
        let home = temp("linux-home");
        let dirs = linux::refresh::user_dirs(&home, None);
        fs::create_dir_all(&dirs.applications).unwrap();
        fs::write(
            dirs.applications.join("cursorpie.desktop"),
            "[Desktop Entry]\nName=CursorPie\nExec=cursorpie\n",
        )
        .unwrap();
        let steps = Steps {
            extension_dir: &support.join("browser-extension"),
            linux: Some(&dirs),
            login_entry: &|| false,
        };
        assert!(run_in(&support, "9.9.9", &steps).linux_files);
        assert_eq!(
            fs::read_to_string(dirs.applications.join("cursorpie.desktop")).unwrap(),
            linux::refresh::DESKTOP_ENTRY
        );
        let _ = fs::remove_dir_all(&support);
        let _ = fs::remove_dir_all(&home);
    }
}
