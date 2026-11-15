//! The apps `Applications` folders list, read from the `Info.plist` of each `.app` bundle.
//! The pure parts (reading the plist JSON, the language folders) are in
//! [`installed`](crate::platform::installed), where unit tests run on every OS.

use crate::platform::installed::{
    InstalledApp, app_from_plist_json, lproj_names, names_in_strings_json, parse_apple_languages,
    plist_is_agent,
};
use std::path::{Path, PathBuf};

/// The folders whose `.app` bundles are listed, with how many levels of plain folders below each
/// are searched too (Adobe, Setapp and the Chrome apps keep their bundles one folder down), and
/// whether helper programs without a window (`LSUIElement`) are left out. Finder itself, Screen
/// Sharing and the other system apps are in `CoreServices`, which the Finder shows as part of the
/// system although it is not in any "Applications" folder.
fn mac_app_dirs() -> Vec<(PathBuf, u32, bool)> {
    use PathBuf;
    let mut dirs: Vec<(PathBuf, u32, bool)> = vec![
        (PathBuf::from("/Applications"), 2, false),
        (PathBuf::from("/System/Applications"), 2, false),
        // Safari and the other apps that the system updates apart from the system volume
        (
            PathBuf::from("/System/Cryptexes/App/System/Applications"),
            1,
            false,
        ),
        (PathBuf::from("/System/Library/CoreServices"), 1, true),
    ];
    dirs.push((crate::config::home().join("Applications"), 2, false));
    dirs
}

/// The `.app` bundles in `dir` and in the plain folders below it (`depth` levels). A bundle is
/// never entered: what is inside it is not an app of its own.
fn bundles_in(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for file in read.flatten() {
        if out.len() >= 5000 {
            return;
        }
        if file.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = file.path();
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        {
            out.push(path);
        } else if depth > 0 && path.is_dir() {
            bundles_in(&path, depth - 1, out);
        }
    }
}

/// An `Info.plist` as JSON, by `plutil`, which reads the XML, the binary and the old-style forms
/// (and `.strings` files) alike. `None` when the file cannot be read.
fn plist_json(path: &Path) -> Option<Vec<u8>> {
    let output = std::process::Command::new("plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(path)
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// Where the `Info.plist` of a bundle is: normally `Contents/Info.plist`. An iPhone or iPad app
/// that runs on a Mac is a wrapper whose real bundle is `WrappedBundle`, with the plist at its top.
fn info_plist(bundle: &Path) -> Option<PathBuf> {
    [
        "Contents/Info.plist",
        "WrappedBundle/Info.plist",
        "Info.plist",
    ]
    .iter()
    .map(|rel| bundle.join(rel))
    .find(|p| p.is_file())
}

/// The languages the user prefers, most wanted first (`id-ID`, `en-ID`...), from the global
/// preference `AppleLanguages`. At most three; empty when it cannot be read.
fn preferred_languages() -> Vec<String> {
    std::process::Command::new("defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        .ok()
        .map(|o| parse_apple_languages(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

/// The names a bundle has in the user's languages: what Finder shows (`Kalkulator` for
/// Calculator on an Indonesian system), read from `InfoPlist.strings` of the matching `.lproj`.
fn localized_names(bundle: &Path, languages: &[String]) -> Vec<String> {
    let resources = bundle.join("Contents/Resources");
    let mut out: Vec<String> = Vec::new();
    for lang in languages {
        for dir in lproj_names(lang) {
            let file = resources.join(format!("{dir}.lproj/InfoPlist.strings"));
            if !file.is_file() {
                continue;
            }
            if let Some(json) = plist_json(&file) {
                out.extend(names_in_strings_json(&json));
                break; // the best folder of this language is enough
            }
        }
    }
    out
}

/// Every `.app` bundle in the usual folders, with its id and name read from `Info.plist`.
pub fn collect_installed_apps() -> Vec<InstalledApp> {
    let languages = preferred_languages();
    let mut out = Vec::new();
    for (dir, depth, no_agents) in mac_app_dirs() {
        let mut bundles = Vec::new();
        bundles_in(&dir, depth, &mut bundles);
        for path in bundles {
            let Some(json) = info_plist(&path).and_then(|p| plist_json(&p)) else {
                continue;
            };
            if no_agents && plist_is_agent(&json) {
                continue;
            }
            if let Some(mut app) = app_from_plist_json(&json, &path) {
                for name in localized_names(&path, &languages) {
                    let name = name.to_lowercase();
                    if !app.alt.split('\n').any(|a| a == name) {
                        app.alt.push('\n');
                        app.alt.push_str(&name);
                    }
                }
                out.push(app);
            }
        }
    }
    out
}
