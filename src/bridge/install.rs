//! Registers the native messaging host with the browsers, on every OS.
//!
//! A browser starts a *native messaging host* from a small JSON file (the host manifest) that it
//! finds in a fixed place. CursorPie is its own host: the manifest points at the CursorPie
//! executable, which recognizes how it was started (see [`host`](super::host)). The place depends
//! on the OS and the browser:
//!
//! | OS | Chromium browsers | Firefox |
//! |---|---|---|
//! | Linux | `<browser config folder>/NativeMessagingHosts/<name>.json` | `~/.mozilla/native-messaging-hosts/<name>.json` |
//! | macOS | `~/Library/Application Support/<browser folder>/NativeMessagingHosts/<name>.json` | `~/Library/Application Support/Mozilla/NativeMessagingHosts/<name>.json` |
//! | Windows | registry key `HKCU\Software\<browser>\NativeMessagingHosts\<name>` whose value is the path of a manifest file | `HKCU\SOFTWARE\Mozilla\NativeMessagingHosts\<name>` |
//!
//! A browser that is not installed (its folder, or its registry key, is not there) is skipped, so
//! nothing is created for it. Browsers in a sandbox (Flatpak, Snap) cannot start a program outside
//! it and are not covered.

use super::extension::{CHROMIUM_ID, FIREFOX_ID};
use super::proto::HOST_NAME;
use crate::platform::Os;
use std::path::{Path, PathBuf};

/// The two ways browsers describe who may call a host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    /// Chrome, Chromium, Edge, Brave, Vivaldi: `allowed_origins`.
    Chromium,
    /// Firefox: `allowed_extensions`.
    Firefox,
}

/// A supported browser and where it keeps things on each OS.
struct Row {
    name: &'static str,
    family: Family,
    /// Linux: folder of the browser under the config folder (Firefox: `.mozilla` under home).
    linux: &'static str,
    /// macOS: folder under `~/Library/Application Support`.
    mac: &'static str,
    /// Windows: registry key under `HKCU`.
    win: &'static str,
}

const BROWSERS: [Row; 6] = [
    Row {
        name: "Chrome",
        family: Family::Chromium,
        linux: "google-chrome",
        mac: "Google/Chrome",
        win: r"Software\Google\Chrome",
    },
    Row {
        name: "Chromium",
        family: Family::Chromium,
        linux: "chromium",
        mac: "Chromium",
        win: r"Software\Chromium",
    },
    Row {
        name: "Edge",
        family: Family::Chromium,
        linux: "microsoft-edge",
        mac: "Microsoft Edge",
        win: r"Software\Microsoft\Edge",
    },
    Row {
        name: "Brave",
        family: Family::Chromium,
        linux: "BraveSoftware/Brave-Browser",
        mac: "BraveSoftware/Brave-Browser",
        win: r"Software\BraveSoftware\Brave-Browser",
    },
    Row {
        name: "Vivaldi",
        family: Family::Chromium,
        linux: "vivaldi",
        mac: "Vivaldi",
        win: r"Software\Vivaldi",
    },
    Row {
        name: "Firefox",
        family: Family::Firefox,
        linux: ".mozilla",
        mac: "Mozilla",
        win: r"SOFTWARE\Mozilla",
    },
];

/// Where a host manifest goes for one browser on Linux or macOS.
#[derive(Debug, PartialEq, Eq)]
pub struct Location {
    pub name: &'static str,
    pub family: Family,
    /// The folder of the browser: when it is not there, the browser is not installed.
    pub root: PathBuf,
    /// The folder that holds the host manifests.
    pub dir: PathBuf,
}

/// What registering did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// The browsers that now know the host.
    pub registered: Vec<&'static str>,
    /// What went wrong, one line each.
    pub errors: Vec<String>,
}

/// The text of the host manifest for `family`, pointing at `exe`.
pub fn manifest_json(family: Family, exe: &Path) -> String {
    let mut v = serde_json::json!({
        "name": HOST_NAME,
        "description": "CursorPie: tells the pie menu which web pages are open",
        "path": exe.to_string_lossy(),
        "type": "stdio",
    });
    match family {
        Family::Chromium => {
            v["allowed_origins"] =
                serde_json::json!([format!("chrome-extension://{CHROMIUM_ID}/")]);
        }
        Family::Firefox => {
            v["allowed_extensions"] = serde_json::json!([FIREFOX_ID]);
        }
    }
    let mut text = serde_json::to_string_pretty(&v).unwrap_or_default();
    text.push('\n');
    text
}

/// The place of each browser on Linux and macOS (empty on Windows, which uses the registry).
/// `config_home` is `$XDG_CONFIG_HOME` or `~/.config`.
pub fn unix_locations(os: Os, home: &Path, config_home: &Path) -> Vec<Location> {
    BROWSERS
        .iter()
        .filter_map(|b| {
            let (root, dir) = match os {
                Os::Linux if b.family == Family::Firefox => {
                    let root = home.join(b.linux);
                    (root.clone(), root.join("native-messaging-hosts"))
                }
                Os::Linux => {
                    let root = config_home.join(b.linux);
                    (root.clone(), root.join("NativeMessagingHosts"))
                }
                Os::Mac => {
                    let root = home.join("Library").join("Application Support").join(b.mac);
                    (root.clone(), root.join("NativeMessagingHosts"))
                }
                Os::Windows => return None,
            };
            Some(Location {
                name: b.name,
                family: b.family,
                root,
                dir,
            })
        })
        .collect()
}

/// The registry key of the host under the browser key `base` (Windows).
pub fn registry_key(base: &str) -> String {
    format!(r"HKCU\{base}\NativeMessagingHosts\{HOST_NAME}")
}

fn config_home(home: &Path) -> PathBuf {
    match std::env::var("XDG_CONFIG_HOME") {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => home.join(".config"),
    }
}

/// Registers the host for the browsers that are installed, pointing at `exe`.
pub fn install(exe: &Path) -> Report {
    let home = crate::config::home();
    install_with(crate::platform::OS, exe, &home, &config_home(&home))
}

/// Registers the host for the running executable.
pub fn install_current() -> Report {
    match std::env::current_exe() {
        Ok(exe) => install(&exe),
        Err(e) => Report {
            registered: Vec::new(),
            errors: vec![format!("cannot tell where CursorPie is: {e}")],
        },
    }
}

fn install_with(os: Os, exe: &Path, home: &Path, config_home: &Path) -> Report {
    let mut rep = Report::default();
    if os == Os::Windows {
        install_windows(exe, &mut rep);
        return rep;
    }
    for loc in unix_locations(os, home, config_home) {
        if !loc.root.is_dir() {
            continue;
        }
        let file = loc.dir.join(format!("{HOST_NAME}.json"));
        let done = std::fs::create_dir_all(&loc.dir)
            .and_then(|()| std::fs::write(&file, manifest_json(loc.family, exe)));
        match done {
            Ok(()) => rep.registered.push(loc.name),
            Err(e) => rep.errors.push(format!("{}: {e}", loc.name)),
        }
    }
    rep
}

/// Where the manifest of `family` is kept on Windows (the registry points at it).
fn windows_manifest(family: Family) -> PathBuf {
    let sub = match family {
        Family::Chromium => "chromium",
        Family::Firefox => "firefox",
    };
    crate::config::app_support_dir()
        .join("native-messaging")
        .join(sub)
        .join(format!("{HOST_NAME}.json"))
}

fn install_windows(exe: &Path, rep: &mut Report) {
    for b in &BROWSERS {
        if !reg(&["query", &format!(r"HKCU\{}", b.win)]) {
            continue; // not installed
        }
        let file = windows_manifest(b.family);
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&file, manifest_json(b.family, exe)));
        if let Err(e) = written {
            rep.errors.push(format!("{}: {e}", b.name));
            continue;
        }
        let ok = reg(&[
            "add",
            &registry_key(b.win),
            "/ve",
            "/t",
            "REG_SZ",
            "/d",
            &file.to_string_lossy(),
            "/f",
        ]);
        if ok {
            rep.registered.push(b.name);
        } else {
            rep.errors
                .push(format!("{}: could not write the registry", b.name));
        }
    }
}

/// Removes what [`install`] wrote.
pub fn uninstall() {
    let home = crate::config::home();
    uninstall_with(crate::platform::OS, &home, &config_home(&home));
}

fn uninstall_with(os: Os, home: &Path, config_home: &Path) {
    if os == Os::Windows {
        for b in &BROWSERS {
            let _ = reg(&["delete", &registry_key(b.win), "/f"]);
        }
        let _ = std::fs::remove_dir_all(crate::config::app_support_dir().join("native-messaging"));
        return;
    }
    for loc in unix_locations(os, home, config_home) {
        let _ = std::fs::remove_file(loc.dir.join(format!("{HOST_NAME}.json")));
    }
}

/// Runs `reg.exe` quietly; whether it succeeded. (Only reached on Windows.)
fn reg(args: &[&str]) -> bool {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new("reg");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes on screen
    }
    cmd.status().is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cursorpie-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn each_family_names_its_extension() {
        let exe = Path::new("/opt/cursorpie/cursorpie");
        let c: serde_json::Value =
            serde_json::from_str(&manifest_json(Family::Chromium, exe)).unwrap();
        assert_eq!(c["name"], HOST_NAME);
        assert_eq!(c["type"], "stdio");
        assert_eq!(c["path"], "/opt/cursorpie/cursorpie");
        assert_eq!(
            c["allowed_origins"][0],
            format!("chrome-extension://{CHROMIUM_ID}/")
        );
        assert!(c.get("allowed_extensions").is_none());
        let f: serde_json::Value =
            serde_json::from_str(&manifest_json(Family::Firefox, exe)).unwrap();
        assert_eq!(f["allowed_extensions"][0], FIREFOX_ID);
        assert!(f.get("allowed_origins").is_none());
    }

    #[test]
    fn the_folders_follow_the_rules_of_each_os() {
        let (home, cfg) = (Path::new("/home/u"), Path::new("/home/u/.config"));
        let linux = unix_locations(Os::Linux, home, cfg);
        let chrome = &linux[0];
        assert_eq!(chrome.name, "Chrome");
        assert_eq!(
            chrome.dir,
            Path::new("/home/u/.config/google-chrome/NativeMessagingHosts")
        );
        let firefox = linux.iter().find(|l| l.name == "Firefox").unwrap();
        assert_eq!(
            firefox.dir,
            Path::new("/home/u/.mozilla/native-messaging-hosts")
        );
        let mac = unix_locations(Os::Mac, home, cfg);
        let firefox = mac.iter().find(|l| l.name == "Firefox").unwrap();
        assert_eq!(
            firefox.dir,
            Path::new("/home/u/Library/Application Support/Mozilla/NativeMessagingHosts")
        );
        assert!(unix_locations(Os::Windows, home, cfg).is_empty());
        assert_eq!(
            registry_key(r"Software\Google\Chrome"),
            r"HKCU\Software\Google\Chrome\NativeMessagingHosts\io.github.cursorpie.bridge"
        );
    }

    #[test]
    fn only_the_browsers_that_are_installed_get_the_host() {
        let home = temp("install-home");
        let cfg = home.join(".config");
        std::fs::create_dir_all(cfg.join("google-chrome")).unwrap();
        std::fs::create_dir_all(home.join(".mozilla")).unwrap();
        let exe = Path::new("/opt/cursorpie/cursorpie");
        let rep = install_with(Os::Linux, exe, &home, &cfg);
        assert_eq!(rep.registered, ["Chrome", "Firefox"]);
        assert!(rep.errors.is_empty());
        let file = format!("{HOST_NAME}.json");
        assert!(
            cfg.join("google-chrome/NativeMessagingHosts")
                .join(&file)
                .is_file()
        );
        assert!(
            home.join(".mozilla/native-messaging-hosts")
                .join(&file)
                .is_file()
        );
        assert!(
            !cfg.join("chromium").exists(),
            "nothing is made for a browser that is not there"
        );
        uninstall_with(Os::Linux, &home, &cfg);
        assert!(
            !cfg.join("google-chrome/NativeMessagingHosts")
                .join(&file)
                .exists()
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
