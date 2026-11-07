//! The browser extension that ships inside the program.
//!
//! Its files live in `assets/browser-extension/` and are compiled into the executable
//! (`include_str!`), so every build of CursorPie carries the extension that matches it, on every OS,
//! with nothing to download. [`export`] writes them to a folder the browser can load.

use std::io;
use std::path::{Path, PathBuf};

/// The code both browsers run (event page in Firefox, service worker in Chromium browsers).
pub const BACKGROUND_JS: &str = include_str!("../../assets/browser-extension/background.js");
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

/// Writes the extension under `dir`: `chromium/` and `firefox/`, each with `manifest.json` and
/// `background.js`. Files that are there are replaced, so the folder always matches this build.
pub fn export(dir: &Path) -> io::Result<()> {
    for (sub, manifest) in [
        ("chromium", CHROMIUM_MANIFEST),
        ("firefox", FIREFOX_MANIFEST),
    ] {
        let target = dir.join(sub);
        std::fs::create_dir_all(&target)?;
        std::fs::write(target.join("manifest.json"), with_version(manifest))?;
        std::fs::write(target.join("background.js"), BACKGROUND_JS)?;
    }
    Ok(())
}

/// The folder the extension is written to by default: `<support folder>/browser-extension`.
pub fn default_dir() -> PathBuf {
    crate::config::app_support_dir().join("browser-extension")
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
    fn the_extension_is_written_for_both_families() {
        let dir = std::env::temp_dir().join(format!("cursorpie-ext-test-{}", std::process::id()));
        export(&dir).unwrap();
        for sub in ["chromium", "firefox"] {
            assert!(dir.join(sub).join("background.js").is_file());
            let m = std::fs::read_to_string(dir.join(sub).join("manifest.json")).unwrap();
            assert!(m.contains(env!("CARGO_PKG_VERSION")));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
