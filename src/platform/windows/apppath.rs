//! Where the program of an app id lives, and which apps are installed.
//!
//! An app id is the lower-case file name of the executable (`chrome.exe`), which says nothing
//! about the folder. The icon of an app, and the list of apps in Settings, both need the file, and
//! most programs are neither running nor on `PATH`. The places that know them:
//!
//! 1. programs the app already saw by their full path (a file the user picked);
//! 2. `App Paths` in the registry (per user, per machine and the 32-bit view);
//! 3. the shortcuts of the Start menu, the list Windows itself shows as "installed apps";
//! 4. the usual program folders, as a last resort, searched a few levels deep.
//!
//! Everything here is slow the first time and cached, so call it off the UI thread.

use super::registry::{self, Hive};
use super::util::{capitalized, exe_id_from_path, expand_env_vars, is_helper_stem};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Where programs register their own location.
const APP_PATHS: &str = r"Software\Microsoft\Windows\CurrentVersion\App Paths";

/// The registry views `App Paths` is read from: the user's, the machine's, the machine's 32-bit one.
const VIEWS: [(Hive, bool); 3] = [
    (Hive::CurrentUser, false),
    (Hive::LocalMachine, false),
    (Hive::LocalMachine, true),
];

/// One program found through a Start menu shortcut.
pub struct StartApp {
    /// The app id: the file name of the program, lower case.
    pub id: String,
    /// The name of the shortcut, as the Start menu shows it.
    pub name: String,
    /// The program file.
    pub path: String,
}

fn remembered() -> &'static Mutex<HashMap<String, String>> {
    static PATHS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    PATHS.get_or_init(Default::default)
}

fn missing() -> &'static Mutex<HashSet<String>> {
    static MISSING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    MISSING.get_or_init(Default::default)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Remembers the program at `path` for its app id, so its icon is found later even when it is not
/// running. Does nothing for a path that is not an existing file.
pub fn remember(path: &str) {
    let Some(id) = exe_id_from_path(path) else {
        return;
    };
    if Path::new(path).is_file()
        && let Ok(mut map) = remembered().lock()
    {
        map.insert(id, path.to_string());
    }
}

/// The program that `App Paths` names for `id`, when the file is there.
fn from_app_paths(id: &str) -> Option<String> {
    let key = format!(r"{APP_PATHS}\{id}");
    VIEWS.iter().find_map(|&(hive, wow32)| {
        let value = registry::default_string(hive, &key, wow32)?;
        let path = expand_env_vars(value.trim().trim_matches('"'), &env);
        Path::new(&path).is_file().then_some(path)
    })
}

/// The `.lnk` files below `dir`, at most four folders deep.
fn shortcuts_in(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 4 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            shortcuts_in(&path, depth + 1, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            out.push(path);
        }
    }
}

/// The programs that the Start menu has a shortcut to (all users and this user).
pub fn start_menu_apps() -> &'static [StartApp] {
    static APPS: OnceLock<Vec<StartApp>> = OnceLock::new();
    APPS.get_or_init(scan_start_menu)
}

fn scan_start_menu() -> Vec<StartApp> {
    super::apps::com_for_thread();
    let mut links = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Some(base) = env(var) {
            let dir = PathBuf::from(base).join(r"Microsoft\Windows\Start Menu\Programs");
            shortcuts_in(&dir, 0, &mut links);
        }
    }
    let mut out: Vec<StartApp> = Vec::new();
    for link in links {
        let Some(target) = super::shellitem::shortcut_target(&link.to_string_lossy()) else {
            continue;
        };
        let Some(id) = exe_id_from_path(&target) else {
            continue;
        };
        let Some(stem) = id.strip_suffix(".exe") else {
            continue;
        };
        let name = link
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| capitalized(stem));
        if is_helper_stem(stem)
            || is_helper_stem(&name.to_lowercase())
            || !Path::new(&target).is_file()
        {
            continue;
        }
        out.push(StartApp {
            id,
            name,
            path: target,
        });
    }
    out
}

/// A file called `id` in `dir` or a folder below it (at most three levels), nearest first.
/// `budget` counts the directory entries still allowed to be looked at.
fn find_below(dir: &Path, id: &str, depth: u32, budget: &mut u32) -> Option<String> {
    if depth > 3 || *budget == 0 {
        return None;
    }
    let read = std::fs::read_dir(dir).ok()?;
    let mut folders = Vec::new();
    for entry in read.flatten() {
        *budget = budget.saturating_sub(1);
        let path = entry.path();
        if entry.file_name().to_string_lossy().to_ascii_lowercase() == id && path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
        if path.is_dir() {
            folders.push(path);
        }
    }
    folders
        .iter()
        .find_map(|f| find_below(f, id, depth + 1, budget))
}

/// The last resort: the program folders. A miss is remembered, so a program that is not installed
/// is not searched for again at every redraw.
fn from_program_folders(id: &str) -> Option<String> {
    if missing().lock().ok()?.contains(id) {
        return None;
    }
    let roots = [
        env("ProgramFiles").map(PathBuf::from),
        env("ProgramFiles(x86)").map(PathBuf::from),
        env("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Programs")),
    ];
    let mut budget = 20_000u32;
    let found = roots
        .iter()
        .flatten()
        .find_map(|root| find_below(root, id, 0, &mut budget));
    match &found {
        Some(path) => remember(path),
        None => {
            if let Ok(mut set) = missing().lock() {
                set.insert(id.to_string());
            }
        }
    }
    found
}

/// The program file of the app `id` (a lower-case file name such as `chrome.exe`), wherever it is
/// installed. `None` when it cannot be found.
pub fn find_exe(id: &str) -> Option<String> {
    let id = id.trim().to_ascii_lowercase();
    if let Some(path) = remembered()
        .lock()
        .ok()
        .and_then(|map| map.get(&id).cloned())
        && Path::new(&path).is_file()
    {
        return Some(path);
    }
    from_app_paths(&id)
        .or_else(|| {
            start_menu_apps()
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.path.clone())
        })
        .or_else(|| from_program_folders(&id))
}

/// The installed apps as `(id, name)`: what the Start menu lists, then the programs that only
/// registered themselves under `App Paths`.
pub fn installed_list() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = start_menu_apps()
        .iter()
        .map(|a| (a.id.clone(), a.name.clone()))
        .collect();
    let mut seen: HashSet<String> = out.iter().map(|(id, _)| id.clone()).collect();
    for (hive, wow32) in VIEWS {
        for key in registry::subkeys(hive, APP_PATHS, wow32) {
            let id = key.trim().to_ascii_lowercase();
            let Some(stem) = id.strip_suffix(".exe").filter(|s| !s.is_empty()) else {
                continue;
            };
            if is_helper_stem(stem) || seen.contains(&id) || from_app_paths(&id).is_none() {
                continue;
            }
            seen.insert(id.clone());
            let name = capitalized(stem);
            out.push((id, name));
        }
    }
    out
}
