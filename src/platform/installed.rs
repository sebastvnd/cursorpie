//! The applications installed on this computer, for the in-app app picker.
//!
//! Picking an app through the system's file chooser is the same on no two systems (a `.desktop`
//! file in `/usr/share/applications`, an `.exe` in Program Files, an `.app` bundle) and hard on all
//! of them. Settings therefore lists the installed apps itself, with their icons and a search box,
//! the same way on macOS, Windows and Linux. The id of an entry is exactly what a profile stores
//! (see the top of [`platform`](super)): the bundle id, the lower-case executable name, the desktop
//! entry id.
//!
//! Reading the list can take a while (it runs programs on macOS and Windows), so [`installed_apps`]
//! is meant for a background thread. The parsers below are pure and tested on every OS.

use super::linux::desktop_entry;

/// One installed application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledApp {
    /// The app id a profile stores.
    pub id: String,
    /// The name to show.
    pub name: String,
}

/// Sorts by name (case-insensitive) and drops entries with the same id.
pub fn tidy(mut apps: Vec<InstalledApp>) -> Vec<InstalledApp> {
    apps.sort_by_cached_key(|a| (a.name.to_lowercase(), a.id.clone()));
    let mut seen = std::collections::HashSet::new();
    apps.retain(|a| seen.insert(a.id.to_lowercase()));
    apps
}

/// Whether `app` matches what the user typed: every word of `query` is part of the name or the id.
pub fn matches(app: &InstalledApp, query: &str) -> bool {
    let name = app.name.to_lowercase();
    let id = app.id.to_lowercase();
    query
        .split_whitespace()
        .map(str::to_lowercase)
        .all(|w| name.contains(&w) || id.contains(&w))
}

/// What an "Open app" action stores for `app` when it is picked from the list: the name on macOS
/// (`open -a` and the "follow the app" lookup both work with names such as "Safari"), the id
/// everywhere else (the desktop entry id, the executable name).
pub fn action_text(app: &InstalledApp) -> String {
    if matches!(crate::platform::OS, crate::platform::Os::Mac) {
        app.name.clone()
    } else {
        app.id.clone()
    }
}

/// The installed applications of this system, sorted by name. Slow: call it off the UI thread.
pub fn installed_apps() -> Vec<InstalledApp> {
    tidy(collect())
}

// ---------------------------------------------------------------------------------------- Linux

/// The desktop entries that are meant to be listed.
#[cfg(target_os = "linux")]
fn collect() -> Vec<InstalledApp> {
    use crate::platform::linux::xdg;
    let home = crate::config::home();
    let dirs = xdg::application_dirs(
        &home,
        std::env::var("XDG_DATA_HOME").ok().as_deref(),
        std::env::var("XDG_DATA_DIRS").ok().as_deref(),
    );
    entries_to_apps(&desktop_entry::scan(&dirs))
}

/// The listed apps of an index: entries marked `NoDisplay` (URL handlers, helpers) are left out.
pub fn entries_to_apps(index: &desktop_entry::Index) -> Vec<InstalledApp> {
    index
        .entries()
        .iter()
        .filter(|e| !e.no_display)
        .map(|e| InstalledApp {
            id: e.id.clone(),
            name: e.name.clone(),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------- macOS

/// The folders whose `.app` bundles are listed (not recursively, except for the system's own).
#[cfg(target_os = "macos")]
fn mac_app_dirs() -> Vec<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut dirs: Vec<PathBuf> = [
        "/Applications",
        "/Applications/Utilities",
        "/System/Applications",
        "/System/Applications/Utilities",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    dirs.push(crate::config::home().join("Applications"));
    dirs
}

/// Every `.app` bundle in the usual folders, with its id and name read from `Info.plist`
/// (converted to JSON by `plutil`, which reads the XML and the binary form alike).
#[cfg(target_os = "macos")]
fn collect() -> Vec<InstalledApp> {
    use std::process::Command;
    let mut out = Vec::new();
    for dir in mac_app_dirs() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in read.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("app") {
                continue;
            }
            let plist = path.join("Contents/Info.plist");
            let Ok(output) = Command::new("plutil")
                .args(["-convert", "json", "-o", "-"])
                .arg(&plist)
                .output()
            else {
                continue;
            };
            if let Some(app) = app_from_plist_json(&output.stdout, &path) {
                out.push(app);
            }
        }
    }
    out
}

/// The id and the name of an app from the JSON form of its `Info.plist`. The name is the display
/// name, then the bundle name, then the bundle's file name. `None` for a bundle without an id, or
/// one that is not meant to be seen (`LSUIElement` helpers, `LSBackgroundOnly`).
pub fn app_from_plist_json(json: &[u8], bundle: &std::path::Path) -> Option<InstalledApp> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let text = |key: &str| {
        v.get(key)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let flag = |key: &str| {
        v.get(key).is_some_and(|x| {
            x.as_bool() == Some(true) || x.as_i64() == Some(1) || x.as_str() == Some("1")
        })
    };
    if flag("LSBackgroundOnly") {
        return None;
    }
    let id = text("CFBundleIdentifier")?.to_string();
    let stem = bundle
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&id)
        .to_string();
    let name = text("CFBundleDisplayName")
        .or_else(|| text("CFBundleName"))
        .map(str::to_string)
        .unwrap_or(stem);
    Some(InstalledApp { id, name })
}

// -------------------------------------------------------------------------------------- Windows

/// The apps that registered themselves under `App Paths`, per machine and per user. `reg.exe` is
/// asked for the default value of every key, which is the path of the program.
#[cfg(windows)]
fn collect() -> Vec<InstalledApp> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const NO_WINDOW: u32 = 0x0800_0000; // CREATE_NO_WINDOW
    let mut out = Vec::new();
    for hive in ["HKLM", "HKCU"] {
        let key = format!(r"{hive}\Software\Microsoft\Windows\CurrentVersion\App Paths");
        let Ok(output) = Command::new("reg")
            .args(["query", key.as_str(), "/s", "/ve"])
            .creation_flags(NO_WINDOW)
            .output()
        else {
            continue;
        };
        out.extend(parse_reg_app_paths(&String::from_utf8_lossy(
            &output.stdout,
        )));
    }
    out
}

/// The apps in the output of `reg query ...\App Paths /s /ve`:
///
/// ```text
/// HKEY_LOCAL_MACHINE\SOFTWARE\...\App Paths\chrome.exe
///     (Default)    REG_SZ    C:\Program Files\Google\Chrome\Application\chrome.exe
/// ```
///
/// The id is the key's name in lower case; the name is the program's file name without `.exe`,
/// with a capital. Keys that are not programs (`.exe` only), uninstallers and updaters are skipped.
pub fn parse_reg_app_paths(text: &str) -> Vec<InstalledApp> {
    let mut out = Vec::new();
    let mut key: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.to_ascii_uppercase().starts_with("HKEY_") {
            key = trimmed.rsplit('\\').next().map(str::to_string);
            continue;
        }
        let is_default = trimmed.starts_with("(Default)") || trimmed.starts_with("<NO NAME>");
        if !is_default || !trimmed.contains("REG_") {
            continue;
        }
        let Some(exe) = key.take() else { continue };
        let id = exe.trim().to_ascii_lowercase();
        let Some(stem) = id.strip_suffix(".exe").filter(|s| !s.is_empty()) else {
            continue;
        };
        if ["uninstall", "unins000", "update", "setup", "install"]
            .iter()
            .any(|w| stem.contains(w))
        {
            continue;
        }
        let mut chars = stem.chars();
        let name: String = match chars.next() {
            Some(c) => c.to_uppercase().chain(chars).collect(),
            None => continue,
        };
        out.push(InstalledApp { id, name });
    }
    out
}

// ------------------------------------------------------------------------------- anything else

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn collect() -> Vec<InstalledApp> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> InstalledApp {
        InstalledApp {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn the_list_is_sorted_by_name_and_has_each_id_once() {
        let list = tidy(vec![
            app("b", "zed"),
            app("a", "Alpha"),
            app("B", "zed"),
            app("c", "beta"),
        ]);
        let names: Vec<_> = list.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Alpha", "beta", "zed"]);
        assert_eq!(list.len(), 3, "ids b and B are the same app");
        assert_eq!(list[0].name, "Alpha");
    }

    #[test]
    fn search_words_must_all_match_the_name_or_the_id() {
        let code = app("com.visualstudio.code", "Visual Studio Code");
        assert!(matches(&code, ""));
        assert!(matches(&code, "studio code"));
        assert!(matches(&code, "VISUALSTUDIO"));
        assert!(!matches(&code, "studio firefox"));
    }

    #[test]
    fn an_action_stores_the_name_on_macos_and_the_id_elsewhere() {
        let a = app("com.apple.Safari", "Safari");
        let text = action_text(&a);
        if matches!(crate::platform::OS, crate::platform::Os::Mac) {
            assert_eq!(text, "Safari");
        } else {
            assert_eq!(text, "com.apple.Safari");
        }
    }

    #[test]
    fn hidden_entries_are_not_listed() {
        let shown = desktop_entry::parse(
            "a.desktop",
            "/a",
            "[Desktop Entry]\nType=Application\nName=Shown\n",
        )
        .unwrap();
        let helper = desktop_entry::parse(
            "b.desktop",
            "/b",
            "[Desktop Entry]\nType=Application\nName=Helper\nNoDisplay=true\n",
        )
        .unwrap();
        let apps = entries_to_apps(&desktop_entry::Index::new(vec![shown, helper]));
        assert_eq!(apps, vec![app("a", "Shown")]);
    }

    #[test]
    fn an_info_plist_gives_the_id_and_the_best_name() {
        let path = std::path::Path::new("/Applications/Visual Studio Code.app");
        let json = br#"{"CFBundleIdentifier":"com.microsoft.VSCode","CFBundleName":"Code"}"#;
        assert_eq!(
            app_from_plist_json(json, path),
            Some(app("com.microsoft.VSCode", "Code"))
        );
        let json =
            br#"{"CFBundleIdentifier":"x.y","CFBundleDisplayName":"Shown","CFBundleName":"Other"}"#;
        assert_eq!(app_from_plist_json(json, path).unwrap().name, "Shown");
        let json = br#"{"CFBundleIdentifier":"x.y"}"#;
        assert_eq!(
            app_from_plist_json(json, path).unwrap().name,
            "Visual Studio Code"
        );
        assert!(app_from_plist_json(br#"{"CFBundleName":"No id"}"#, path).is_none());
        assert!(
            app_from_plist_json(
                br#"{"CFBundleIdentifier":"x","LSBackgroundOnly":true}"#,
                path
            )
            .is_none()
        );
        assert!(app_from_plist_json(b"not json", path).is_none());
    }

    #[test]
    fn app_paths_output_gives_the_programs() {
        let text = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\r\n\
\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\chrome.exe\r\n    (Default)    REG_SZ    C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe\r\n\
\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Uninstall.exe\r\n    (Default)    REG_SZ    C:\\x\\Uninstall.exe\r\n\
\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\Code.exe\r\n    (Default)    REG_SZ    C:\\Code\\Code.exe\r\n";
        assert_eq!(
            parse_reg_app_paths(text),
            vec![app("chrome.exe", "Chrome"), app("code.exe", "Code")]
        );
    }
}
