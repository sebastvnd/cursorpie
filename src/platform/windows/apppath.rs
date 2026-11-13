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
//! Everything here is slow the first time and cached for a minute (a program installed while
//! CursorPie runs is found; the list of apps always reads the Start menu again), so call it off the
//! UI thread.

use super::registry::{self, Hive};
use super::util::{capitalized, exe_id_from_path, expand_env_vars, is_helper_stem};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

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

/// Programs that were looked for in the program folders and not found, and when. A miss is
/// remembered for a while only: the program may be installed a minute later.
fn missing() -> &'static Mutex<HashMap<String, Instant>> {
    static MISSING: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    MISSING.get_or_init(Default::default)
}

/// How long a miss, and a read of the Start menu, are believed.
const FRESH_FOR: Duration = Duration::from_secs(60);

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

/// A read of the Start menu and the moment it was made.
type StartMenuRead = Option<(Instant, Arc<Vec<StartApp>>)>;

/// The Start menu as it was last read, and when.
fn start_menu_cache() -> &'static Mutex<StartMenuRead> {
    static APPS: OnceLock<Mutex<StartMenuRead>> = OnceLock::new();
    APPS.get_or_init(Default::default)
}

/// The programs that the Start menu has a shortcut to (all users and this user). The read is kept
/// for a minute, not for the life of the program: an app installed while CursorPie runs must show
/// up. `fresh` reads it again whatever its age (the list of apps asks for that). The lock is held
/// during the read, so threads that ask at the same time wait for one read instead of making many.
pub fn start_menu_apps(fresh: bool) -> Arc<Vec<StartApp>> {
    let Ok(mut cache) = start_menu_cache().lock() else {
        return Arc::new(scan_start_menu());
    };
    if let Some((at, apps)) = cache.as_ref()
        && !fresh
        && at.elapsed() < FRESH_FOR
    {
        return Arc::clone(apps);
    }
    let apps = Arc::new(scan_start_menu());
    *cache = Some((Instant::now(), Arc::clone(&apps)));
    apps
}

/// The program a shortcut of a Squirrel-installed app (Discord, GitHub Desktop, Slack, Postman...)
/// really starts. Those shortcuts point at `Update.exe` with `--processStart Name.exe`, so the
/// shortcut's target says nothing about the app. The program lives in a folder `app-<version>` next
/// to `Update.exe`; `names` are the `.exe` files found there. It is the one whose name (without
/// `.exe`, spaces and punctuation dropped, lower case) is the shortcut's name or the folder's.
pub fn pick_squirrel_exe(names: &[String], link_name: &str, folder: &str) -> Option<String> {
    let key = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let (link, folder) = (key(link_name), key(folder));
    let stem = |n: &String| {
        let lower = n.to_ascii_lowercase();
        key(lower.strip_suffix(".exe").unwrap_or(&lower))
    };
    names
        .iter()
        .find(|n| !link.is_empty() && stem(n) == link)
        .or_else(|| {
            names
                .iter()
                .find(|n| !folder.is_empty() && stem(n) == folder)
        })
        .cloned()
}

/// The real program behind the `Update.exe` at `target`, found on disk (see [`pick_squirrel_exe`]).
fn squirrel_program(target: &str, link_name: &str) -> Option<String> {
    let root = Path::new(target).parent()?;
    let folder = root.file_name()?.to_string_lossy().into_owned();
    // the newest `app-<version>` folder: versions sort well enough by their text for this
    let mut versions: Vec<PathBuf> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("app-"))
        })
        .collect();
    versions.sort();
    let dir = versions.pop()?;
    let names: Vec<String> = std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_ascii_lowercase().ends_with(".exe"))
        .collect();
    let exe = pick_squirrel_exe(&names, link_name, &folder)?;
    Some(dir.join(exe).to_string_lossy().into_owned())
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
        let Some(mut target) = super::shellitem::shortcut_target(&link.to_string_lossy()) else {
            continue;
        };
        let name_of_link = link.file_stem().map(|s| s.to_string_lossy().into_owned());
        // `Update.exe --processStart App.exe`: the app is the program next to it, not the updater
        if exe_id_from_path(&target).as_deref() == Some("update.exe")
            && let Some(real) = squirrel_program(&target, name_of_link.as_deref().unwrap_or(""))
        {
            target = real;
        }
        let Some(id) = exe_id_from_path(&target) else {
            continue;
        };
        let Some(stem) = id.strip_suffix(".exe") else {
            continue;
        };
        let name = name_of_link.unwrap_or_else(|| capitalized(stem));
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
    if missing()
        .lock()
        .ok()?
        .get(id)
        .is_some_and(|at| at.elapsed() < FRESH_FOR)
    {
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
            if let Ok(mut map) = missing().lock() {
                // a miss older than `FRESH_FOR` is not believed any more: drop it, or the table
                // grows with every name that was ever looked for
                map.retain(|_, at| at.elapsed() < FRESH_FOR);
                map.insert(id.to_string(), Instant::now());
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
            start_menu_apps(false)
                .iter()
                .find(|a| a.id == id)
                .map(|a| a.path.clone())
        })
        .or_else(|| from_program_folders(&id))
}

/// The installed apps as `(id, name)`: what the Start menu lists, then the programs that only
/// registered themselves under `App Paths`.
pub fn installed_list() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = start_menu_apps(true)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_squirrel_shortcut_names_the_program_next_to_the_updater() {
        let files = names(&["Discord.exe", "squirrel.exe", "crashpad_handler.exe"]);
        assert_eq!(
            pick_squirrel_exe(&files, "Discord", "Discord").as_deref(),
            Some("Discord.exe")
        );
        // the shortcut says "GitHub Desktop", the program is GitHubDesktop.exe
        let files = names(&["GitHubDesktop.exe", "squirrel.exe"]);
        assert_eq!(
            pick_squirrel_exe(&files, "GitHub Desktop", "GitHubDesktop").as_deref(),
            Some("GitHubDesktop.exe")
        );
        // no program of that name: the folder's name is the second guess, then nothing
        let files = names(&["app.exe", "squirrel.exe"]);
        assert_eq!(
            pick_squirrel_exe(&files, "Other", "app").as_deref(),
            Some("app.exe")
        );
        assert_eq!(pick_squirrel_exe(&files, "Other", "x"), None);
        assert_eq!(pick_squirrel_exe(&[], "", ""), None);
    }
}
