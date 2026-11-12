//! The browser extension that ships inside the program.
//!
//! Its files live in `assets/browser-extension/` and are compiled into the executable
//! (`include_str!`), so every build of CursorPie carries the extension that matches it, on every OS,
//! with nothing to download. [`export`] writes them to a folder the browser can load.

use std::io;
use std::path::{Path, PathBuf};

/// The code both browsers run (event page in Firefox, service worker in Chromium browsers).
pub const BACKGROUND_JS: &str = include_str!("../../assets/browser-extension/background.js");
/// The other files of the extension, written next to `background.js` (the manifests list them). The
/// order is the order the Firefox manifest loads them in.
pub const SUPPORT_FILES: &[(&str, &str)] = &[
    (
        "util.js",
        include_str!("../../assets/browser-extension/util.js"),
    ),
    (
        "page-media.js",
        include_str!("../../assets/browser-extension/page-media.js"),
    ),
    (
        "page-chat.js",
        include_str!("../../assets/browser-extension/page-chat.js"),
    ),
    (
        "ui.js",
        include_str!("../../assets/browser-extension/ui.js"),
    ),
    (
        "cmd-tab.js",
        include_str!("../../assets/browser-extension/cmd-tab.js"),
    ),
    (
        "cmd-media.js",
        include_str!("../../assets/browser-extension/cmd-media.js"),
    ),
    (
        "cmd-chat.js",
        include_str!("../../assets/browser-extension/cmd-chat.js"),
    ),
    (
        "cmd-ui.js",
        include_str!("../../assets/browser-extension/cmd-ui.js"),
    ),
    (
        "options.html",
        include_str!("../../assets/browser-extension/options.html"),
    ),
    (
        "options.js",
        include_str!("../../assets/browser-extension/options.js"),
    ),
];
/// `manifest.json` for Chrome, Chromium, Edge, Brave, Vivaldi (version still a placeholder).
pub const CHROMIUM_MANIFEST: &str =
    include_str!("../../assets/browser-extension/manifest.chromium.json");
/// `manifest.json` for Firefox (version still a placeholder).
pub const FIREFOX_MANIFEST: &str =
    include_str!("../../assets/browser-extension/manifest.firefox.json");

/// What stands for the version in the manifests of the repository: replaced by the version of this
/// build when the files are written, so the extension never needs a version bump of its own.
const VERSION_PLACEHOLDER: &str = "{{VERSION}}";

/// The id Chromium browsers give the extension: it comes from the `key` of the Chromium manifest,
/// so it is the same on every machine, and the host manifest can name it (`allowed_origins`).
pub const CHROMIUM_ID: &str = "mbfoipmefaenedenjnepgcdkgkmefpnk";

/// The id of the extension in Firefox (`browser_specific_settings.gecko.id`), named by the host
/// manifest (`allowed_extensions`).
pub const FIREFOX_ID: &str = "browser-bridge@cursorpie";

/// A manifest with this build's version in it.
fn with_version(manifest: &str) -> String {
    manifest.replace(VERSION_PLACEHOLDER, env!("CARGO_PKG_VERSION"))
}

/// Writes the extension under `dir`: `chromium/` and `firefox/`, each with `manifest.json`,
/// `background.js` and [`SUPPORT_FILES`]. Files that are there are replaced, so the folder always matches this build.
pub fn export(dir: &Path) -> io::Result<()> {
    for (sub, manifest) in [
        ("chromium", CHROMIUM_MANIFEST),
        ("firefox", FIREFOX_MANIFEST),
    ] {
        let target = dir.join(sub);
        std::fs::create_dir_all(&target)?;
        std::fs::write(target.join("manifest.json"), with_version(manifest))?;
        std::fs::write(target.join("background.js"), BACKGROUND_JS)?;
        for (name, text) in SUPPORT_FILES {
            std::fs::write(target.join(name), text)?;
        }
    }
    Ok(())
}

/// The folder the extension is written to by default: `<support folder>/browser-extension`.
pub fn default_dir() -> PathBuf {
    crate::config::app_support_dir().join("browser-extension")
}

/// Which of the two folders [`export`] writes a browser loads: Firefox its own, every other
/// browser (Chrome, Edge, Brave, Vivaldi, Opera...) the Chromium one. `browser` is what the
/// extension said in its `hello`.
pub fn family_of(browser: &str) -> &'static str {
    if browser == "firefox" {
        "firefox"
    } else {
        "chromium"
    }
}

/// The version that [`export`] wrote into the manifest `browser` loads from `dir`: the version an
/// extension loaded from there will report. `None` when nothing was written there.
pub fn exported_version(dir: &Path, browser: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(family_of(browser)).join("manifest.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&text).ok()?;
    manifest["version"].as_str().map(str::to_string)
}

/// Writes the extension to `dir` again, but only when the person has written it there before:
/// without the folder nothing is created (they never asked for the files). `Ok(true)` when the
/// files were written. This is what keeps the folder in step with the program after an update, so
/// the person does not have to press *Show the extension files* again (see [`crate::refresh`]).
pub fn refresh(dir: &Path) -> io::Result<bool> {
    if !dir.is_dir() {
        return Ok(false);
    }
    export(dir)?;
    Ok(true)
}

/// Writes the extension to [`default_dir`] and opens that folder in the file manager.
pub fn export_and_show() -> io::Result<PathBuf> {
    let dir = default_dir();
    export(&dir)?;
    crate::platform::open_path(&dir);
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::proto::HOST_NAME;

    #[test]
    fn both_manifests_are_valid_and_carry_the_version_of_the_build() {
        for text in [CHROMIUM_MANIFEST, FIREFOX_MANIFEST] {
            let v: serde_json::Value = serde_json::from_str(&with_version(text)).expect("json");
            assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
            assert!(
                v["permissions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|p| p == "nativeMessaging")
            );
        }
        assert!(!with_version(CHROMIUM_MANIFEST).contains(VERSION_PLACEHOLDER));
    }

    #[test]
    fn the_firefox_manifest_has_the_id_the_host_manifest_names() {
        let v: serde_json::Value = serde_json::from_str(FIREFOX_MANIFEST).unwrap();
        assert_eq!(v["browser_specific_settings"]["gecko"]["id"], FIREFOX_ID);
    }

    #[test]
    fn the_chromium_manifest_has_a_fixed_key_and_the_script_names_the_host() {
        let v: serde_json::Value = serde_json::from_str(CHROMIUM_MANIFEST).unwrap();
        assert!(v["key"].as_str().is_some_and(|k| k.len() > 300));
        assert_eq!(CHROMIUM_ID.len(), 32);
        assert!(CHROMIUM_ID.bytes().all(|b| (b'a'..=b'p').contains(&b)));
        assert!(BACKGROUND_JS.contains(&format!("\"{HOST_NAME}\"")));
    }

    #[test]
    fn every_file_the_manifests_and_the_worker_name_is_shipped() {
        let names: Vec<&str> = SUPPORT_FILES.iter().map(|(n, _)| *n).collect();
        let firefox: serde_json::Value = serde_json::from_str(FIREFOX_MANIFEST).unwrap();
        for script in firefox["background"]["scripts"].as_array().unwrap() {
            let script = script.as_str().unwrap();
            assert!(
                script == "background.js" || names.contains(&script),
                "{script}"
            );
        }
        // what the Chromium service worker loads is what Firefox lists, before background.js
        let loaded: Vec<&str> = BACKGROUND_JS
            .split("importScripts(")
            .nth(1)
            .and_then(|r| r.split(')').next())
            .unwrap()
            .split('"')
            .filter(|p| p.ends_with(".js"))
            .collect();
        let listed: Vec<&str> = firefox["background"]["scripts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .filter(|n| *n != "background.js")
            .collect();
        assert_eq!(loaded, listed);
        let chromium: serde_json::Value = serde_json::from_str(CHROMIUM_MANIFEST).unwrap();
        let page = chromium["options_ui"]["page"].as_str().unwrap();
        assert!(names.contains(&page));
    }

    #[test]
    fn a_browser_loads_the_folder_of_its_family() {
        assert_eq!(family_of("firefox"), "firefox");
        for chromium in ["chrome", "edge", "brave", "vivaldi", "opera", "browser"] {
            assert_eq!(family_of(chromium), "chromium", "{chromium}");
        }
    }

    #[test]
    fn the_exported_version_is_the_one_of_the_build_and_nothing_before_the_export() {
        let dir = std::env::temp_dir().join(format!("cursorpie-ext-ver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(exported_version(&dir, "chrome"), None);
        export(&dir).unwrap();
        for browser in ["chrome", "firefox"] {
            assert_eq!(
                exported_version(&dir, browser).as_deref(),
                Some(env!("CARGO_PKG_VERSION")),
                "{browser}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn refreshing_replaces_what_was_exported_and_creates_nothing() {
        let dir = std::env::temp_dir().join(format!("cursorpie-ext-ref-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // never exported: not created
        assert!(!refresh(&dir).unwrap());
        assert!(!dir.exists());
        // exported by an older build: the manifest says so, the refresh brings it up to date
        export(&dir).unwrap();
        let old = std::fs::read_to_string(dir.join("chromium/manifest.json"))
            .unwrap()
            .replace(env!("CARGO_PKG_VERSION"), "0.0.1");
        std::fs::write(dir.join("chromium/manifest.json"), old).unwrap();
        assert_eq!(exported_version(&dir, "chrome").as_deref(), Some("0.0.1"));
        assert!(refresh(&dir).unwrap());
        assert_eq!(
            exported_version(&dir, "chrome").as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_extension_is_written_for_both_families() {
        let dir = std::env::temp_dir().join(format!("cursorpie-ext-test-{}", std::process::id()));
        export(&dir).unwrap();
        for sub in ["chromium", "firefox"] {
            assert!(dir.join(sub).join("background.js").is_file());
            for (name, _) in SUPPORT_FILES {
                assert!(dir.join(sub).join(name).is_file(), "{sub}/{name}");
            }
            let m = std::fs::read_to_string(dir.join(sub).join("manifest.json")).unwrap();
            assert!(m.contains(env!("CARGO_PKG_VERSION")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
