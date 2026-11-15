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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InstalledApp {
    /// The app id a profile stores.
    pub id: String,
    /// The name to show.
    pub name: String,
    /// Other names the app goes by, lower case, only for the search box: the bundle's file name,
    /// the names in the language of the system, a generic name ("Web Browser"), keywords. The
    /// system's own file manager finds an app by these, so the search here must too.
    pub alt: String,
}

/// Sorts by name (case-insensitive) and drops entries with the same id.
pub fn tidy(mut apps: Vec<InstalledApp>) -> Vec<InstalledApp> {
    apps.sort_by_cached_key(|a| (a.name.to_lowercase(), a.id.clone()));
    let mut seen = std::collections::HashSet::new();
    apps.retain(|a| seen.insert(a.id.to_lowercase()));
    apps
}

/// `text` in lower case with only letters and digits left, so `Whats App`, `whats-app` and
/// `WhatsApp` are the same.
fn compact(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `app` matches what the user typed: every word of `query` is part of the name, the id or
/// another name of the app (see [`InstalledApp::alt`]), or the whole query, with spaces and
/// punctuation left out, is part of one of them (`vs code` finds `VSCode`).
pub fn matches(app: &InstalledApp, query: &str) -> bool {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return true;
    }
    let hay = [
        app.name.to_lowercase(),
        app.id.to_lowercase(),
        app.alt.to_lowercase(),
    ];
    if words.iter().all(|w| hay.iter().any(|h| h.contains(w))) {
        return true;
    }
    let whole = compact(query);
    !whole.is_empty() && hay.iter().any(|h| compact(h).contains(&whole))
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
    tidy(super::collect_installed_apps())
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
            alt: [
                Some(e.search.as_str()),
                e.exec_name.as_deref(),
                e.wm_class.as_deref(),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase(),
        })
        .collect()
}

/// The language tags in the output of `defaults read -g AppleLanguages`, which looks like
/// `(\n    "id-ID",\n    "en-ID"\n)`. At most three.
pub fn parse_apple_languages(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().trim_end_matches(',').trim_matches('"').trim())
        .filter(|l| !l.is_empty() && *l != "(" && *l != ")")
        .map(str::to_string)
        .take(3)
        .collect()
}

/// The names of the folders (`<name>.lproj`) an app may keep its texts in for the language tag
/// `lang`: the tag as it is, with `_` for `-`, and with the last parts cut off (`zh-Hans-CN`,
/// `zh_Hans_CN`, `zh-Hans`, `zh`).
pub fn lproj_names(lang: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut parts: Vec<&str> = lang.split(['-', '_']).filter(|p| !p.is_empty()).collect();
    while !parts.is_empty() {
        for sep in ["-", "_"] {
            let name = parts.join(sep);
            if !out.contains(&name) {
                out.push(name);
            }
        }
        parts.pop();
    }
    out
}

/// The display name and the bundle name in the JSON form of an `InfoPlist.strings` file.
pub fn names_in_strings_json(json: &[u8]) -> Vec<String> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(json) else {
        return Vec::new();
    };
    ["CFBundleDisplayName", "CFBundleName"]
        .iter()
        .filter_map(|k| v.get(k).and_then(|x| x.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Whether the JSON form of an `Info.plist` is that of an agent: a program that runs without a
/// window or a Dock icon (`LSUIElement`), which is not something to pick from a list of apps.
pub fn plist_is_agent(json: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(json)
        .ok()
        .is_some_and(|v| plist_flag(&v, "LSUIElement"))
}

fn plist_flag(v: &serde_json::Value, key: &str) -> bool {
    v.get(key).is_some_and(|x| {
        x.as_bool() == Some(true) || x.as_i64() == Some(1) || x.as_str() == Some("1")
    })
}

/// The id and the name of an app from the JSON form of its `Info.plist`. The name is the display
/// name, then the bundle's file name, which is what Finder shows (`Visual Studio Code`, where
/// `CFBundleName` says only `Code`). The other names go to [`InstalledApp::alt`], so a search by
/// any of them finds the app. `None` for a bundle without an id, or one that is not meant to be
/// seen (`LSBackgroundOnly`).
pub fn app_from_plist_json(json: &[u8], bundle: &std::path::Path) -> Option<InstalledApp> {
    let v: serde_json::Value = serde_json::from_slice(json).ok()?;
    let text = |key: &str| {
        v.get(key)
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    if plist_flag(&v, "LSBackgroundOnly") {
        return None;
    }
    let id = text("CFBundleIdentifier")?.to_string();
    let stem = bundle
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&id)
        .to_string();
    let name = text("CFBundleDisplayName")
        .map(str::to_string)
        .unwrap_or_else(|| stem.clone());
    let mut alt: Vec<String> = Vec::new();
    for other in [Some(stem.as_str()), text("CFBundleName")]
        .into_iter()
        .flatten()
    {
        let other = other.to_lowercase();
        if other != name.to_lowercase() && !alt.contains(&other) {
            alt.push(other);
        }
    }
    Some(InstalledApp {
        id,
        name,
        alt: alt.join("\n"),
    })
}

/// The apps in the output of `reg query ...\App Paths /s /ve`. Not used by the Windows build any
/// more (see `platform::windows::collect_installed_apps`); kept, with its tests, for anyone who has such an output at hand:
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
        out.push(InstalledApp {
            id,
            name,
            alt: String::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> InstalledApp {
        InstalledApp {
            id: id.into(),
            name: name.into(),
            alt: String::new(),
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
    fn an_app_is_found_by_its_other_names_and_by_a_spelling_without_spaces() {
        let mut calc = app("com.apple.calculator", "Calculator");
        calc.alt = "kalkulator".into();
        assert!(
            matches(&calc, "kalk"),
            "the name in the language of the system"
        );
        let whats = app("net.whatsapp.WhatsApp", "WhatsApp");
        assert!(matches(&whats, "whats app"));
        assert!(matches(&whats, "whats-app"));
        let code = app("com.microsoft.VSCode", "Visual Studio Code");
        assert!(matches(&code, "vs code"));
        assert!(matches(&code, "visualstudio"));
        assert!(!matches(&code, "vs firefox"));
        assert!(!matches(&whats, "!!!"), "punctuation alone is not a match");
    }

    #[test]
    fn the_languages_of_the_system_are_read_from_the_defaults_output() {
        let out = "(\n    \"id-ID\",\n    \"en-ID\"\n)\n";
        assert_eq!(parse_apple_languages(out), ["id-ID", "en-ID"]);
        assert!(parse_apple_languages("").is_empty());
        let many = "(\n    a,\n    b,\n    c,\n    d\n)";
        assert_eq!(parse_apple_languages(many).len(), 3);
    }

    #[test]
    fn a_language_tag_gives_the_folders_an_app_may_use() {
        assert_eq!(lproj_names("id-ID"), ["id-ID", "id_ID", "id"]);
        assert_eq!(
            lproj_names("zh-Hans-CN"),
            ["zh-Hans-CN", "zh_Hans_CN", "zh-Hans", "zh_Hans", "zh"]
        );
        assert!(lproj_names("").is_empty());
    }

    #[test]
    fn localized_names_come_from_the_strings_file() {
        let json =
            br#"{"CFBundleDisplayName":" Kalkulator ","CFBundleName":"Kalkulator","Other":"x"}"#;
        assert_eq!(names_in_strings_json(json), ["Kalkulator", "Kalkulator"]);
        assert!(names_in_strings_json(b"nope").is_empty());
    }

    #[test]
    fn an_agent_is_told_from_a_normal_app() {
        assert!(plist_is_agent(br#"{"LSUIElement":true}"#));
        assert!(plist_is_agent(br#"{"LSUIElement":"1"}"#));
        assert!(!plist_is_agent(
            br#"{"CFBundleIdentifier":"com.apple.finder"}"#
        ));
        assert!(!plist_is_agent(b"nope"));
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
    fn an_info_plist_gives_the_id_and_the_name_finder_shows() {
        let path = std::path::Path::new("/Applications/Visual Studio Code.app");
        // no display name: the bundle's file name, which is what Finder shows; `Code` is still
        // a name the app is found by
        let json = br#"{"CFBundleIdentifier":"com.microsoft.VSCode","CFBundleName":"Code"}"#;
        let found = app_from_plist_json(json, path).unwrap();
        assert_eq!(found.id, "com.microsoft.VSCode");
        assert_eq!(found.name, "Visual Studio Code");
        assert_eq!(found.alt, "code");
        let json =
            br#"{"CFBundleIdentifier":"x.y","CFBundleDisplayName":"Shown","CFBundleName":"Other"}"#;
        let found = app_from_plist_json(json, path).unwrap();
        assert_eq!(found.name, "Shown");
        assert!(found.alt.contains("visual studio code") && found.alt.contains("other"));
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
