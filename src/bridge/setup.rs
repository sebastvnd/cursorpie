//! Getting the extension into a browser with as few steps as the browsers allow.
//!
//! A browser does not let another program install an extension for it, so the app cannot do the
//! last click. What it can do is take the person to the exact place: the store page when the
//! extension is published there ([`CHROME_STORE_URL`], [`FIREFOX_ADDON_URL`]), otherwise the
//! extensions page of that very browser, with the files already written ([`start`]).
//!
//! **Updates.** An extension loaded from the folder the app writes to is kept current by the app
//! itself (see [`super::extension::refresh`] and the reload request in [`super`]). One installed
//! from a store is updated by the store, on its own schedule, so it can be a little older (review
//! takes time) or newer than the app: only an *older* one is reported as outdated (see
//! [`is_older`]), and the commands the app sends are always ones an older extension may ignore.

use super::extension;
use super::install::Family;
use crate::platform::Os;
use std::process::{Command, Stdio};

/// The page of the extension in the Chrome Web Store (it serves Chrome, Edge, Brave, Vivaldi and
/// Opera). `None` until it is published: the button then opens the extensions page instead.
pub const CHROME_STORE_URL: Option<&str> = None;
/// The page of the signed Firefox add-on. `None` until it is published.
pub const FIREFOX_ADDON_URL: Option<&str> = None;
/// Where a signed Firefox add-on (`.xpi`) is attached to every release.
pub const RELEASES_URL: &str = "https://github.com/sebastvnd/cursorpie/releases/latest";

/// What a browser name from the install report (`Chrome`, `Firefox`...) belongs to.
pub fn family_of_name(name: &str) -> Family {
    if name == "Firefox" {
        Family::Firefox
    } else {
        Family::Chromium
    }
}

/// The extensions page of a browser, the place to load an extension by hand.
pub fn extensions_page(name: &str) -> &'static str {
    match name {
        "Firefox" => "about:debugging#/runtime/this-firefox",
        "Edge" => "edge://extensions",
        "Brave" => "brave://extensions",
        "Vivaldi" => "vivaldi://extensions",
        _ => "chrome://extensions",
    }
}

/// The store page for `name`, when the extension is published for it.
pub fn store_url(name: &str) -> Option<&'static str> {
    match family_of_name(name) {
        Family::Chromium => CHROME_STORE_URL,
        Family::Firefox => FIREFOX_ADDON_URL,
    }
}

/// What the button of `name` says it will do.
pub fn button_label(name: &str) -> String {
    if store_url(name).is_some() {
        format!("Add to {name}")
    } else {
        format!("Set up in {name}")
    }
}

/// One way to start a browser on a page: the program and its arguments.
type Launch = (String, Vec<String>);

/// The commands to try, in order, to open `url` in the browser `name` on `os`.
fn launches(os: Os, name: &str, url: &str) -> Vec<Launch> {
    let one = |p: &str, a: Vec<String>| (p.to_string(), a);
    match os {
        Os::Linux => {
            let programs: &[&str] = match name {
                "Firefox" => &["firefox"],
                "Chromium" => &["chromium", "chromium-browser"],
                "Edge" => &["microsoft-edge-stable", "microsoft-edge"],
                "Brave" => &["brave-browser", "brave"],
                "Vivaldi" => &["vivaldi-stable", "vivaldi"],
                _ => &["google-chrome-stable", "google-chrome"],
            };
            programs
                .iter()
                .map(|p| one(p, vec![url.to_string()]))
                .collect()
        }
        Os::Mac => {
            let app = match name {
                "Firefox" => "Firefox",
                "Chromium" => "Chromium",
                "Edge" => "Microsoft Edge",
                "Brave" => "Brave Browser",
                "Vivaldi" => "Vivaldi",
                _ => "Google Chrome",
            };
            vec![one("open", vec!["-a".into(), app.into(), url.into()])]
        }
        Os::Windows => {
            // `start` finds the browser by its registered name, wherever it is installed
            let exe = match name {
                "Firefox" => "firefox",
                "Chromium" => "chromium",
                "Edge" => "msedge",
                "Brave" => "brave",
                "Vivaldi" => "vivaldi",
                _ => "chrome",
            };
            vec![one(
                "cmd",
                vec![
                    "/C".into(),
                    "start".into(),
                    String::new(),
                    exe.into(),
                    url.into(),
                ],
            )]
        }
    }
}

/// Opens `url` in the browser `name`. Whether a browser could be started.
fn open_in(name: &str, url: &str) -> bool {
    for (program, args) in launches(crate::platform::OS, name, url) {
        let mut cmd = Command::new(&program);
        cmd.args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::platform::hide_console(&mut cmd);
        if let Ok(mut child) = cmd.spawn() {
            // reaped in the background so no zombie is left behind
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            return true;
        }
    }
    false
}

/// What [`start`] did, for the line shown to the person.
#[derive(Debug, PartialEq, Eq)]
pub enum Started {
    /// The store page is open: one click on *Add* is left.
    Store,
    /// The files are written and the extensions page of the browser is open: *Load unpacked*
    /// (or *Load Temporary Add-on*) and the folder are left.
    Folder(std::path::PathBuf),
    /// The browser could not be started: the files are written, the person opens its page.
    FolderOnly(std::path::PathBuf),
}

/// Takes the person to the place where the extension is added in the browser `name`.
pub fn start(name: &str) -> std::io::Result<Started> {
    if let Some(url) = store_url(name)
        && open_in(name, url)
    {
        return Ok(Started::Store);
    }
    // the files are written here, so there is always something to load
    let dir = extension::default_dir();
    extension::export(&dir)?;
    let folder = dir.join(extension::family_of(match family_of_name(name) {
        Family::Firefox => "firefox",
        Family::Chromium => "chrome",
    }));
    if open_in(name, extensions_page(name)) {
        crate::platform::open_path(&folder);
        Ok(Started::Folder(folder))
    } else {
        crate::platform::open_path(&folder);
        Ok(Started::FolderOnly(folder))
    }
}

/// What the line under the buttons says after [`start`].
pub fn started_text(name: &str, how: &Started) -> String {
    match how {
        Started::Store => {
            format!("The store page is open in {name}: press Add. It connects within a minute.")
        }
        Started::Folder(p) if family_of_name(name) == Family::Firefox => format!(
            "The debugging page is open in {name}: choose Load Temporary Add-on and pick manifest.json in {}. (Firefox forgets a temporary add-on when it closes; the signed add-on from the release page stays.)",
            p.display()
        ),
        Started::Folder(p) => format!(
            "The extensions page is open in {name}: turn on Developer mode, choose Load unpacked and pick {}. After that, CursorPie keeps it up to date by itself.",
            p.display()
        ),
        Started::FolderOnly(p) => format!(
            "Could not start {name}. Open its extensions page yourself and load {}.",
            p.display()
        ),
    }
}

/// Whether the extension version `ext` is older than this build `build`. An empty or unreadable
/// version counts as older (an extension from before the version was sent); a newer one is fine:
/// a store may publish before the app does.
pub fn is_older(ext: &str, build: &str) -> bool {
    use crate::update::Version;
    match (Version::parse(ext), Version::parse(build)) {
        (Some(e), Some(b)) => e < b,
        (None, _) => true,
        (Some(_), None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_browser_gets_its_own_extensions_page() {
        assert_eq!(extensions_page("Chrome"), "chrome://extensions");
        assert_eq!(extensions_page("Edge"), "edge://extensions");
        assert_eq!(extensions_page("Brave"), "brave://extensions");
        assert!(extensions_page("Firefox").starts_with("about:debugging"));
    }

    #[test]
    fn the_launch_commands_follow_the_os() {
        let l = launches(Os::Linux, "Chrome", "chrome://extensions");
        assert_eq!(l[0].0, "google-chrome-stable");
        assert_eq!(l[1].0, "google-chrome");
        let m = launches(Os::Mac, "Brave", "brave://extensions");
        assert_eq!(m[0].1[..2], ["-a", "Brave Browser"]);
        let w = launches(Os::Windows, "Edge", "edge://extensions");
        assert_eq!(w[0].0, "cmd");
        assert!(w[0].1.contains(&"msedge".to_string()));
    }

    #[test]
    fn the_button_says_add_only_when_a_store_page_exists() {
        assert_eq!(
            button_label("Chrome").starts_with("Add to"),
            CHROME_STORE_URL.is_some()
        );
        assert!(button_label("Firefox").contains("Firefox"));
    }

    #[test]
    fn only_an_older_extension_is_outdated() {
        assert!(is_older("0.8.3", "0.8.22"));
        assert!(!is_older("0.8.22", "0.8.22"));
        assert!(
            !is_older("0.9.0", "0.8.22"),
            "a store may be ahead of the app"
        );
        assert!(is_older("", "0.8.22"));
    }

    #[test]
    fn the_text_after_start_names_the_folder() {
        let p = std::path::PathBuf::from("/x/chromium");
        let t = started_text("Chrome", &Started::Folder(p.clone()));
        assert!(t.contains("Load unpacked") && t.contains("/x/chromium"));
        let f = started_text("Firefox", &Started::Folder(p));
        assert!(f.contains("Temporary Add-on"));
    }
}
