//! Keeping the files CursorPie put in the person's own folders in step with the program, after an
//! update.
//!
//! An update replaces the program and nothing else of what `install.sh` put next to it. The files
//! that live in the person's own folders (`~/.local/share`) can be written by the program itself,
//! because it carries them: the desktop entry and the GNOME Shell extension. This writes them again
//! when they are there and not what this build ships. Rules, so that nothing the person did is lost:
//!
//! * a file that is not there is **never created**: the person did not install it, or put it
//!   somewhere else (a system folder such as `/usr/local/share` is not written, it is not theirs);
//! * a desktop entry is replaced only when it still is CursorPie's own (its `Name` and `Exec` lines
//!   are the shipped ones); one the person edited stays as it is;
//! * errors are ignored: a file that cannot be written stays as it was.
//!
//! The udev rule (`/etc/udev/rules.d`, needs root) and the icons are not touched; `install.sh` does
//! those. GNOME Shell reads an extension once per session, so the new GNOME extension code runs after
//! the next log in. Pure: the folders come in as parameters. Called by [`crate::refresh`].

use super::wayland::gnome::{EXTENSION_UUID, METADATA, SCRIPT};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The desktop entry of `packaging/linux/cursorpie.desktop`, compiled in.
pub const DESKTOP_ENTRY: &str = include_str!("../../../packaging/linux/cursorpie.desktop");

/// Where the files of the person's own account are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserDirs {
    /// `<data home>/applications`
    pub applications: PathBuf,
    /// `<data home>/gnome-shell/extensions/<uuid>`
    pub gnome_extension: PathBuf,
}

/// The folders for `home`: the data home is `$XDG_DATA_HOME` when it is an absolute path, else
/// `~/.local/share`.
pub fn user_dirs(home: &Path, xdg_data_home: Option<&str>) -> UserDirs {
    let data = match xdg_data_home.map(str::trim) {
        Some(p) if p.starts_with('/') => PathBuf::from(p),
        _ => home.join(".local/share"),
    };
    UserDirs {
        applications: data.join("applications"),
        gnome_extension: data.join("gnome-shell/extensions").join(EXTENSION_UUID),
    }
}

/// What [`refresh`] rewrote.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub desktop_entry: bool,
    pub gnome_extension: bool,
}

impl Report {
    /// Whether anything was written.
    pub fn any(self) -> bool {
        self.desktop_entry || self.gnome_extension
    }
}

/// Whether `text` is a desktop entry that CursorPie shipped (and the person did not change the
/// lines that say what it is and what it runs).
fn is_ours(text: &str) -> bool {
    let has = |line: &str| text.lines().any(|l| l.trim() == line);
    has("Name=CursorPie") && has("Exec=cursorpie")
}

/// Writes `new` over `path` when it is there and different. `Ok(false)`: nothing to do.
fn replace_if_different(path: &Path, new: &str) -> io::Result<bool> {
    let old = fs::read_to_string(path)?;
    if old == new {
        return Ok(false);
    }
    fs::write(path, new)?;
    Ok(true)
}

/// Brings the files in `dirs` up to date (see the rules at the top).
pub fn refresh(dirs: &UserDirs) -> Report {
    let mut report = Report::default();

    let entry = dirs.applications.join("cursorpie.desktop");
    if fs::read_to_string(&entry).is_ok_and(|old| is_ours(&old)) {
        report.desktop_entry = replace_if_different(&entry, DESKTOP_ENTRY).unwrap_or(false);
    }

    if dirs.gnome_extension.is_dir() {
        for (name, text) in [("extension.js", SCRIPT), ("metadata.json", METADATA)] {
            let path = dirs.gnome_extension.join(name);
            let changed = match fs::read_to_string(&path) {
                Ok(_) => replace_if_different(&path, text).unwrap_or(false),
                // a folder of the extension that lost a file: put it back
                Err(_) => fs::write(&path, text).is_ok(),
            };
            report.gnome_extension |= changed;
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "cursorpie-linux-refresh-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_folders_follow_xdg_data_home_when_it_is_absolute() {
        let home = Path::new("/home/ann");
        let d = user_dirs(home, None);
        assert_eq!(
            d.applications,
            Path::new("/home/ann/.local/share/applications")
        );
        assert!(d.gnome_extension.ends_with(EXTENSION_UUID));
        assert!(
            d.gnome_extension
                .starts_with("/home/ann/.local/share/gnome-shell/extensions")
        );
        let d = user_dirs(home, Some("/data/share"));
        assert_eq!(d.applications, Path::new("/data/share/applications"));
        // a relative or empty value is ignored, as the XDG specification says
        assert_eq!(user_dirs(home, Some("share")), user_dirs(home, None));
        assert_eq!(user_dirs(home, Some("  ")), user_dirs(home, None));
    }

    #[test]
    fn what_the_package_ships_is_recognized_as_ours() {
        assert!(is_ours(DESKTOP_ENTRY));
        assert!(!is_ours(
            "[Desktop Entry]\nName=CursorPie\nExec=env X=1 cursorpie\n"
        ));
        assert!(!is_ours(
            "[Desktop Entry]\nName=Something else\nExec=cursorpie\n"
        ));
    }

    #[test]
    fn nothing_is_created_that_the_person_did_not_install() {
        let home = temp("none");
        let dirs = user_dirs(&home, None);
        assert_eq!(refresh(&dirs), Report::default());
        assert!(!home.join(".local").exists());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn an_old_desktop_entry_is_replaced_and_an_edited_one_is_left_alone() {
        let home = temp("entry");
        let dirs = user_dirs(&home, None);
        fs::create_dir_all(&dirs.applications).unwrap();
        let path = dirs.applications.join("cursorpie.desktop");

        // an older release: the same program, fewer lines
        fs::write(
            &path,
            "[Desktop Entry]\nType=Application\nName=CursorPie\nExec=cursorpie\n",
        )
        .unwrap();
        let report = refresh(&dirs);
        assert!(report.desktop_entry && report.any());
        assert_eq!(fs::read_to_string(&path).unwrap(), DESKTOP_ENTRY);
        assert_eq!(
            refresh(&dirs),
            Report::default(),
            "the second time there is nothing to do"
        );

        // the person changed what it runs
        let edited = "[Desktop Entry]\nName=CursorPie\nExec=env GDK_BACKEND=x11 cursorpie\n";
        fs::write(&path, edited).unwrap();
        assert_eq!(refresh(&dirs), Report::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), edited);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn an_installed_gnome_extension_is_brought_up_to_date_and_only_that() {
        let home = temp("gnome");
        let dirs = user_dirs(&home, None);
        fs::create_dir_all(&dirs.gnome_extension).unwrap();
        fs::write(
            dirs.gnome_extension.join("extension.js"),
            "// an older release\n",
        )
        .unwrap();
        // metadata.json is missing: put back
        let report = refresh(&dirs);
        assert!(report.gnome_extension);
        assert_eq!(
            fs::read_to_string(dirs.gnome_extension.join("extension.js")).unwrap(),
            SCRIPT
        );
        assert_eq!(
            fs::read_to_string(dirs.gnome_extension.join("metadata.json")).unwrap(),
            METADATA
        );
        assert_eq!(refresh(&dirs), Report::default());
        let _ = fs::remove_dir_all(&home);
    }
}
