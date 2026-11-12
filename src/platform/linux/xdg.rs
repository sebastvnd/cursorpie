//! XDG base directories and user directories. Pure: the environment and file text come in as
//! parameters.

use std::path::{Path, PathBuf};

/// `$XDG_CONFIG_HOME` if it is an absolute path, else `$HOME/.config`.
pub fn config_home(home: &Path, xdg_config_home: Option<&str>) -> PathBuf {
    match xdg_config_home.map(str::trim) {
        Some(p) if p.starts_with('/') => PathBuf::from(p),
        _ => home.join(".config"),
    }
}

/// Where CursorPie keeps its files: `<config home>/cursorpie`.
pub fn support_dir(home: &Path, xdg_config_home: Option<&str>) -> PathBuf {
    config_home(home, xdg_config_home).join("cursorpie")
}

/// The data directories, most specific first: the user's (`XDG_DATA_HOME`, default
/// `~/.local/share`), the system's (`XDG_DATA_DIRS`, default `/usr/local/share:/usr/share`), then
/// the Flatpak, Snap and Nix directories and the usual system ones, in case the variable left them
/// out.
fn data_bases(
    home: &Path,
    xdg_data_home: Option<&str>,
    xdg_data_dirs: Option<&str>,
) -> Vec<PathBuf> {
    let data_home = match xdg_data_home.map(str::trim) {
        Some(p) if p.starts_with('/') => PathBuf::from(p),
        _ => home.join(".local/share"),
    };
    let mut bases: Vec<PathBuf> = vec![data_home];
    let dirs = xdg_data_dirs
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or("/usr/local/share:/usr/share");
    for d in dirs.split(':').filter(|d| d.starts_with('/')) {
        bases.push(PathBuf::from(d));
    }
    bases.push(home.join(".local/share/flatpak/exports/share"));
    bases.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    bases.push(PathBuf::from("/var/lib/snapd/desktop"));
    // Nix and NixOS keep their apps in profiles that `XDG_DATA_DIRS` only names when the session was
    // started by the login shell; a program started from the menu or at login may not have them
    bases.push(home.join(".nix-profile/share"));
    bases.push(PathBuf::from("/nix/var/nix/profiles/default/share"));
    bases.push(PathBuf::from("/run/current-system/sw/share"));
    // the usual system directories, for a session whose `XDG_DATA_DIRS` leaves them out
    bases.push(PathBuf::from("/usr/local/share"));
    bases.push(PathBuf::from("/usr/share"));
    bases
}

/// `<base>/<sub>` for each data directory, without repeats.
fn data_subdirs(bases: Vec<PathBuf>, sub: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for b in bases {
        let dir = b.join(sub);
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// The `applications` directories that may hold desktop entries, most specific first: the user's,
/// the system's (`XDG_DATA_DIRS`, default `/usr/local/share:/usr/share`), then the Flatpak and Snap
/// export directories.
pub fn application_dirs(
    home: &Path,
    xdg_data_home: Option<&str>,
    xdg_data_dirs: Option<&str>,
) -> Vec<PathBuf> {
    data_subdirs(
        data_bases(home, xdg_data_home, xdg_data_dirs),
        "applications",
    )
}

/// The `icons` directories that may hold icon themes, most specific first: the user's, the old
/// `~/.icons`, then the system's, Flatpak's and Snap's.
pub fn icon_dirs(
    home: &Path,
    xdg_data_home: Option<&str>,
    xdg_data_dirs: Option<&str>,
) -> Vec<PathBuf> {
    let mut dirs = data_subdirs(data_bases(home, xdg_data_home, xdg_data_dirs), "icons");
    let old = home.join(".icons");
    if !dirs.contains(&old) {
        dirs.insert(1usize.min(dirs.len()), old);
    }
    dirs
}

/// The `pixmaps` directories, where older applications keep an icon that is in no theme.
pub fn pixmap_dirs(
    home: &Path,
    xdg_data_home: Option<&str>,
    xdg_data_dirs: Option<&str>,
) -> Vec<PathBuf> {
    data_subdirs(data_bases(home, xdg_data_home, xdg_data_dirs), "pixmaps")
}

/// The user's download directory, from the text of `~/.config/user-dirs.dirs`. It is localized
/// (`~/Unduhan` on an Indonesian system), so the file is read; without it, `~/Downloads`.
pub fn download_dir_from(home: &Path, user_dirs: Option<&str>) -> PathBuf {
    let fallback = home.join("Downloads");
    let Some(text) = user_dirs else {
        return fallback;
    };
    for line in text.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("XDG_DOWNLOAD_DIR=") else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        let path = match value.strip_prefix("$HOME") {
            Some(rest) => format!("{}{}", home.to_string_lossy(), rest),
            None => value.to_string(),
        };
        // `$HOME` itself means "no download folder": use the fallback
        if path.starts_with('/') && Path::new(&path) != home {
            return PathBuf::from(path);
        }
        return fallback;
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path with `/` separators, so the tests hold on Windows too (`join` uses `\` there).
    fn u(p: &Path) -> String {
        p.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn config_lives_in_xdg_config_home_or_dot_config() {
        let home = Path::new("/home/ann");
        assert_eq!(u(&support_dir(home, None)), "/home/ann/.config/cursorpie");
        assert_eq!(
            u(&support_dir(home, Some("/data/cfg"))),
            "/data/cfg/cursorpie"
        );
        assert_eq!(
            u(&support_dir(home, Some("relative/path"))),
            "/home/ann/.config/cursorpie",
            "a relative XDG path is ignored, as the spec says"
        );
        assert_eq!(
            u(&support_dir(home, Some("  "))),
            "/home/ann/.config/cursorpie"
        );
    }

    #[test]
    fn application_dirs_are_ordered_and_unique() {
        let home = Path::new("/home/ann");
        let d: Vec<String> = application_dirs(home, None, None)
            .iter()
            .map(|p| u(p))
            .collect();
        assert_eq!(d[0], "/home/ann/.local/share/applications");
        assert_eq!(d[1], "/usr/local/share/applications");
        assert_eq!(d[2], "/usr/share/applications");
        assert!(d.contains(&"/var/lib/flatpak/exports/share/applications".to_string()));
        assert!(d.contains(&"/var/lib/snapd/desktop/applications".to_string()));
        let custom: Vec<String> =
            application_dirs(home, Some("/h"), Some("/usr/share:/usr/share:rel"))
                .iter()
                .map(|p| u(p))
                .collect();
        assert_eq!(custom[0], "/h/applications");
        assert_eq!(
            custom
                .iter()
                .filter(|p| *p == "/usr/share/applications")
                .count(),
            1
        );
        assert!(!custom.iter().any(|p| p.starts_with("rel")));
    }

    #[test]
    fn icon_and_pixmap_dirs_follow_the_data_dirs() {
        let home = Path::new("/home/ann");
        let icons: Vec<String> = icon_dirs(home, None, None).iter().map(|p| u(p)).collect();
        assert_eq!(icons[0], "/home/ann/.local/share/icons");
        assert_eq!(icons[1], "/home/ann/.icons");
        assert_eq!(icons[2], "/usr/local/share/icons");
        assert_eq!(icons[3], "/usr/share/icons");
        assert!(icons.contains(&"/var/lib/flatpak/exports/share/icons".to_string()));
        assert_eq!(icons.iter().filter(|p| *p == "/home/ann/.icons").count(), 1);
        let pixmaps: Vec<String> = pixmap_dirs(home, Some("/h"), Some("/usr/share:/usr/share"))
            .iter()
            .map(|p| u(p))
            .collect();
        assert_eq!(pixmaps[0], "/h/pixmaps");
        assert_eq!(
            pixmaps
                .iter()
                .filter(|p| *p == "/usr/share/pixmaps")
                .count(),
            1
        );
    }

    #[test]
    fn the_download_dir_follows_user_dirs() {
        let home = Path::new("/home/budi");
        assert_eq!(u(&download_dir_from(home, None)), "/home/budi/Downloads");
        let id =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_DOWNLOAD_DIR=\"$HOME/Unduhan\"\n";
        assert_eq!(u(&download_dir_from(home, Some(id))), "/home/budi/Unduhan");
        let abs = "XDG_DOWNLOAD_DIR=\"/mnt/data/dl\"\n";
        assert_eq!(u(&download_dir_from(home, Some(abs))), "/mnt/data/dl");
        let none = "XDG_DOWNLOAD_DIR=\"$HOME/\"\n";
        assert_eq!(
            u(&download_dir_from(home, Some(none))),
            "/home/budi/Downloads"
        );
        let off = "XDG_DOWNLOAD_DIR=\"$HOME\"\n";
        assert_eq!(
            u(&download_dir_from(home, Some(off))),
            "/home/budi/Downloads"
        );
        assert_eq!(
            u(&download_dir_from(
                home,
                Some("XDG_MUSIC_DIR=\"$HOME/Musik\"\n")
            )),
            "/home/budi/Downloads"
        );
    }
}
