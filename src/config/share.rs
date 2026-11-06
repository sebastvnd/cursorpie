//! Sharing one profile, icons included.
//!
//! A profile file is JSON: the [`Profile`] itself plus every image icon it uses, embedded as
//! base64, so the receiver gets the pie exactly as it looks. `app:<bundle id>` icons are not
//! embedded (macOS provides them) and text or emoji icons are just text.
//!
//! ```text
//! { "cursorpie_profile": 1,
//!   "profile": { "name": "...", "apps": [...], "items": [...], "active": [...] },
//!   "icons":   { "<icon string as used in items>": { "file": "name.png", "data": "<base64>" } } }
//! ```
//!
//! Importing never trusts the file: the extension of an icon is chosen from the image *content*
//! (PNG, JPEG or WebP, nothing else is written), file names are reduced to safe characters, sizes
//! are capped, and the result is added as a **new** profile, so nothing is overwritten. The
//! caller warns about shell commands ([`profile_has_commands`](super::profile_has_commands)).
//!
//! Everything here works on text, bytes and directories passed in, so it is unit tested.

use super::flow::{Flow, flow_ids_used};
use super::{Config, Item, ItemKind, MAX_PROFILES, Profile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Value of `cursorpie_profile` in files written by this version.
pub const FORMAT: u32 = 1;
/// Larger icon files are left out when exporting and ignored when importing.
pub const MAX_ICON_BYTES: usize = 2 * 1024 * 1024;
/// Most icons embedded in one file.
pub const MAX_ICONS: usize = 128;
/// Larger profile files are refused without being read.
pub const MAX_FILE_BYTES: u64 = 24 * 1024 * 1024;

/// One embedded image.
#[derive(Serialize, Deserialize, Debug, Clone)]
struct SharedIcon {
    /// File name it had (only used to name the copy on import).
    file: String,
    /// Image bytes, standard base64.
    data: String,
}

/// The file format. See the module docs.
#[derive(Serialize, Deserialize, Debug)]
pub struct Bundle {
    cursorpie_profile: u32,
    pub profile: Profile,
    #[serde(default)]
    icons: BTreeMap<String, SharedIcon>,
    /// The flows the slices of the profile use (see [`flow`](super::flow)), so a shared profile
    /// keeps working. Files from before flows existed have none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    flows: Vec<Flow>,
}

/// `Some(path)` if an icon string points to an image file (`~/` allowed), `None` for text, emoji
/// and `app:` icons.
pub fn image_path(icon: &str) -> Option<PathBuf> {
    let s = icon.trim();
    let lower = s.to_lowercase();
    if ![".png", ".jpg", ".jpeg", ".webp"]
        .iter()
        .any(|e| lower.ends_with(e))
    {
        return None;
    }
    Some(match s.strip_prefix("~/") {
        Some(rest) => super::home().join(rest),
        None => PathBuf::from(s),
    })
}

// ------------------------------------------------------------------ base64

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding.
pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Decodes standard base64; whitespace is ignored. `None` for anything else that is not valid.
pub fn b64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits, mut pad) = (0u32, 0, 0);
    for b in text.bytes().filter(|b| !b.is_ascii_whitespace()) {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                pad += 1;
                continue;
            }
            _ => return None,
        };
        if pad > 0 {
            return None; // data after padding
        }
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    (pad <= 2).then_some(out)
}

/// The extension that matches the image *content* (png, jpg, webp), or `None` if it is none of those.
pub fn sniff_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("jpg")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

// ------------------------------------------------------------------ export

/// Calls `f` for the icon of every slice in `items`, submenus included.
fn each_icon(items: &[Item], f: &mut dyn FnMut(&str)) {
    for it in items {
        f(&it.icon);
        if let ItemKind::Submenu(children) = &it.kind {
            each_icon(children, f);
        }
    }
}

/// The profile file for `profile`, with its image icons embedded. Icons that are missing, too
/// large or not PNG / JPEG / WebP are left out (the receiver sees a "?" for them).
pub fn export_profile(profile: &Profile) -> Result<String, String> {
    export_profile_with(profile, &[])
}

/// [`export_profile`] that also embeds the flows of `flows` that the slices of the profile use.
pub fn export_profile_with(profile: &Profile, flows: &[Flow]) -> Result<String, String> {
    let used = flow_ids_used(&profile.items);
    let flows: Vec<Flow> = flows
        .iter()
        .filter(|f| used.contains(&f.id))
        .cloned()
        .collect();
    let mut icons = BTreeMap::new();
    each_icon(&profile.items, &mut |icon| {
        if icons.len() >= MAX_ICONS || icons.contains_key(icon) {
            return;
        }
        let Some(path) = image_path(icon) else { return };
        let Ok(bytes) = fs::read(&path) else { return };
        if bytes.len() > MAX_ICON_BYTES || sniff_extension(&bytes).is_none() {
            return;
        }
        let file = path
            .file_name()
            .map_or_else(|| "icon".into(), |n| n.to_string_lossy().to_string());
        icons.insert(
            icon.to_string(),
            SharedIcon {
                file,
                data: b64_encode(&bytes),
            },
        );
    });
    let bundle = Bundle {
        cursorpie_profile: FORMAT,
        profile: profile.clone(),
        icons,
        flows,
    };
    serde_json::to_string_pretty(&bundle).map_err(|e| e.to_string())
}

/// Writes the profile file to `<downloads>/<name>.cursorpie-profile.json` and returns its path.
pub fn export_to_downloads(profile: &Profile) -> Result<PathBuf, String> {
    export_to_downloads_with(profile, &[])
}

/// [`export_to_downloads`] that also embeds the flows the profile uses.
pub fn export_to_downloads_with(profile: &Profile, flows: &[Flow]) -> Result<PathBuf, String> {
    let text = export_profile_with(profile, flows)?;
    let mut p = super::downloads_dir();
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    p.push(export_file_name(&profile.name));
    fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(p)
}

/// A file name for the export of a profile: letters, digits, `-` and `_` only.
pub fn export_file_name(profile_name: &str) -> String {
    format!(
        "{}.cursorpie-profile.json",
        safe_stem(profile_name, "profile")
    )
}

/// `name` reduced to ASCII letters, digits, `-` and `_` (at most 40), or `fallback` if nothing is left.
pub(super) fn safe_stem(name: &str, fallback: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-');
    let s: String = s.chars().take(40).collect();
    if s.is_empty() { fallback.into() } else { s }
}

// ------------------------------------------------------------------ import

/// Parses a profile file. The error is a sentence for the status bar.
pub fn parse_bundle(text: &str) -> Result<Bundle, String> {
    let v: serde_json::Value = serde_json::from_str(text)
        .map_err(|_| "That file is not a CursorPie profile.".to_string())?;
    match v.get("cursorpie_profile").and_then(|x| x.as_u64()) {
        None => return Err("That file is not a CursorPie profile.".into()),
        Some(n) if n > FORMAT as u64 => {
            return Err("That profile was made by a newer CursorPie. Update the app first.".into());
        }
        Some(_) => {}
    }
    serde_json::from_value(v).map_err(|_| "That profile file is damaged.".to_string())
}

/// Writes the embedded icons of `bundle` into `icons_dir` and returns the profile with its icon
/// strings pointing at the written files. `as_icon` turns a written path into the icon string
/// (normally `~/...`). An identical file that is already there is reused, a different one with the
/// same name gets a number.
pub fn install_icons(
    bundle: Bundle,
    icons_dir: &Path,
    as_icon: &dyn Fn(&Path) -> String,
) -> Profile {
    let Bundle {
        mut profile, icons, ..
    } = bundle;
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for (key, icon) in icons.into_iter().take(MAX_ICONS) {
        let Some(bytes) = b64_decode(&icon.data) else {
            continue;
        };
        if bytes.len() > MAX_ICON_BYTES {
            continue;
        }
        let Some(ext) = sniff_extension(&bytes) else {
            continue;
        };
        let stem_src = Path::new(&icon.file)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let stem = safe_stem(&stem_src, "icon");
        if fs::create_dir_all(icons_dir).is_err() {
            break;
        }
        let mut n = 1;
        let path = loop {
            let name = if n == 1 {
                format!("{stem}.{ext}")
            } else {
                format!("{stem}-{n}.{ext}")
            };
            let p = icons_dir.join(name);
            match fs::read(&p) {
                Ok(existing) if existing == bytes => break Some(p), // already there
                Ok(_) => n += 1,
                Err(_) => break fs::write(&p, &bytes).ok().map(|_| p),
            }
        };
        if let Some(p) = path {
            map.insert(key, as_icon(&p));
        }
    }
    rewrite_icons(&mut profile.items, &map);
    profile
}

/// Replaces icon strings found in `map`; icons that could not be installed keep their old text.
fn rewrite_icons(items: &mut [Item], map: &BTreeMap<String, String>) {
    for it in items {
        if let Some(new) = map.get(&it.icon) {
            it.icon = new.clone();
        }
        if let ItemKind::Submenu(children) = &mut it.kind {
            rewrite_icons(children, map);
        }
    }
}

/// Reads a profile file and installs its icons. The error is a sentence for the status bar.
pub fn import_file(
    path: &str,
    icons_dir: &Path,
    as_icon: &dyn Fn(&Path) -> String,
) -> Result<Profile, String> {
    import_file_with_flows(path, icons_dir, as_icon).map(|(profile, _)| profile)
}

/// [`import_file`] that also returns the flows the file carries (not yet added to any config, see
/// [`merge_flows`](super::flow::merge_flows)).
pub fn import_file_with_flows(
    path: &str,
    icons_dir: &Path,
    as_icon: &dyn Fn(&Path) -> String,
) -> Result<(Profile, Vec<Flow>), String> {
    let meta = fs::metadata(path).map_err(|_| "Could not read that file.".to_string())?;
    if meta.len() > MAX_FILE_BYTES {
        return Err("That file is too large to be a profile.".into());
    }
    let text = fs::read_to_string(path).map_err(|_| "Could not read that file.".to_string())?;
    let mut bundle = parse_bundle(&text)?;
    let flows = std::mem::take(&mut bundle.flows);
    Ok((install_icons(bundle, icons_dir, as_icon), flows))
}

/// Adds an imported profile to `cfg` as a new profile and returns its index. The profile needs at
/// least one app (there is only one general profile), a free slot, and gets a unique name.
pub fn add_profile(cfg: &mut Config, mut profile: Profile) -> Result<usize, String> {
    profile.apps = clean_apps(&profile.apps);
    if profile.apps.is_empty() {
        return Err(
            "That profile lists no apps. Every profile except the general one needs at least one."
                .into(),
        );
    }
    if cfg.profiles.len() >= MAX_PROFILES {
        return Err(format!(
            "There are already {MAX_PROFILES} profiles. Delete one first."
        ));
    }
    let base = if profile.name.trim().is_empty() {
        "Imported"
    } else {
        profile.name.trim()
    };
    let mut name = base.to_string();
    let mut n = 2;
    while cfg.profiles.iter().any(|p| p.name == name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    profile.name = name;
    cfg.profiles.push(profile);
    Ok(cfg.profiles.len() - 1)
}

/// App identifiers in the canonical form of this OS (trimmed; lower case on Windows), without
/// blanks and duplicates (order kept).
pub fn clean_apps(apps: &[String]) -> Vec<String> {
    clean_apps_for(crate::platform::OS, apps)
}

/// [`clean_apps`] for a given OS.
pub fn clean_apps_for(os: crate::platform::Os, apps: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in apps
        .iter()
        .map(|a| os.normalize_app_id(a))
        .filter(|a| !a.is_empty())
    {
        if !out.iter().any(|o| os.app_ids_equal(o, &a)) {
            out.push(a);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Action, Config};

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cursorpie-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4];

    fn tilde(p: &Path) -> String {
        p.display().to_string()
    }

    fn profile_with(icons: &[&str]) -> Profile {
        let items = icons
            .iter()
            .enumerate()
            .map(|(i, ic)| Item::run(&format!("S{i}"), ic, Action::Nothing))
            .collect();
        Profile {
            name: "Photo".into(),
            apps: vec!["com.adobe.Photoshop".into()],
            items,
            active: vec![],
        }
    }

    #[test]
    fn base64_round_trips_every_length_and_rejects_garbage() {
        for len in 0..40usize {
            let data: Vec<u8> = (0..len as u32).map(|i| (i * 37 + 11) as u8).collect();
            assert_eq!(b64_decode(&b64_encode(&data)).unwrap(), data, "len {len}");
        }
        assert_eq!(b64_encode(b"Man"), "TWFu");
        assert_eq!(b64_encode(b"Ma"), "TWE=");
        assert_eq!(b64_decode("TW\nFu").unwrap(), b"Man");
        assert!(b64_decode("TW*u").is_none());
        assert!(b64_decode("TWE=A").is_none());
    }

    #[test]
    fn only_real_images_are_recognised() {
        assert_eq!(sniff_extension(PNG), Some("png"));
        assert_eq!(
            sniff_extension(&[0xff, 0xd8, 0xff, 0xe0, 0, 0]),
            Some("jpg")
        );
        assert_eq!(sniff_extension(b"RIFF\0\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(sniff_extension(b"#!/bin/sh\nrm -rf ~"), None);
        assert_eq!(sniff_extension(b""), None);
    }

    #[test]
    fn export_then_import_carries_the_icons() {
        let src = scratch("share-src");
        let dst = scratch("share-dst");
        let icon = src.join("my icon.png");
        fs::write(&icon, PNG).unwrap();
        let icon_s = icon.to_string_lossy().to_string();
        let mut p = profile_with(&[&icon_s, "<>", "app:com.apple.Safari", "/missing/none.png"]);
        p.items.push(Item::submenu(
            "Sub",
            "S",
            vec![Item::run("In", &icon_s, Action::Nothing)],
        ));

        let text = export_profile(&p).unwrap();
        let got = install_icons(parse_bundle(&text).unwrap(), &dst, &tilde);

        assert_eq!(got.name, "Photo");
        let new_icon = got.items[0].icon.clone();
        assert!(new_icon.starts_with(dst.to_str().unwrap()), "{new_icon}");
        assert_eq!(fs::read(&new_icon).unwrap(), PNG, "bytes are identical");
        assert_eq!(got.items[1].icon, "<>");
        assert_eq!(got.items[2].icon, "app:com.apple.Safari");
        assert_eq!(
            got.items[3].icon, "/missing/none.png",
            "a missing icon is left as it was"
        );
        match &got.items[4].kind {
            ItemKind::Submenu(c) => {
                assert_eq!(c[0].icon, new_icon, "submenu icons are rewritten too")
            }
            _ => panic!(),
        }
        assert_eq!(
            fs::read_dir(&dst).unwrap().count(),
            1,
            "one file for one image"
        );
    }

    #[test]
    fn importing_twice_reuses_identical_files_and_numbers_different_ones() {
        let dst = scratch("share-reuse");
        let mk = |bytes: &[u8]| {
            let mut b = parse_bundle(&export_profile(&profile_with(&["<>"])).unwrap()).unwrap();
            b.icons.insert(
                "k".into(),
                SharedIcon {
                    file: "a.png".into(),
                    data: b64_encode(bytes),
                },
            );
            b.profile.items[0].icon = "k".into();
            b
        };
        let a = install_icons(mk(PNG), &dst, &tilde);
        let b = install_icons(mk(PNG), &dst, &tilde);
        assert_eq!(a.items[0].icon, b.items[0].icon);
        let mut other = PNG.to_vec();
        other.push(9);
        let c = install_icons(mk(&other), &dst, &tilde);
        assert!(c.items[0].icon.ends_with("a-2.png"), "{}", c.items[0].icon);
    }

    #[test]
    fn hostile_icon_entries_write_nothing_outside_the_icons_folder() {
        let dst = scratch("share-hostile").join("icons");
        let mut b = parse_bundle(&export_profile(&profile_with(&["x.png"])).unwrap()).unwrap();
        b.icons.insert(
            "x.png".into(),
            SharedIcon {
                file: "../../evil.png".into(),
                data: b64_encode(PNG),
            },
        );
        b.icons.insert(
            "y.png".into(),
            SharedIcon {
                file: "run.sh".into(),
                data: b64_encode(b"#!/bin/sh\necho hi"),
            },
        );
        let p = install_icons(b, &dst, &tilde);
        let written: Vec<_> = fs::read_dir(&dst)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].parent().unwrap(), dst);
        assert_eq!(written[0].file_name().unwrap(), "evil.png");
        assert_eq!(p.items[0].icon, tilde(&written[0]));
    }

    #[test]
    fn parse_rejects_other_files_and_newer_formats() {
        assert!(parse_bundle("not json").is_err());
        assert!(
            parse_bundle(r#"{"version": 2, "profiles": []}"#).is_err(),
            "a whole config is not a profile"
        );
        let newer = r#"{"cursorpie_profile": 99}"#;
        assert!(parse_bundle(newer).unwrap_err().contains("newer"));
    }

    #[test]
    fn adding_a_profile_needs_an_app_and_a_free_slot_and_a_unique_name() {
        let mut cfg = Config::default();
        let before = cfg.profiles.len();
        let mut p = profile_with(&[]);
        p.apps = vec!["  ".into()];
        assert!(add_profile(&mut cfg, p).unwrap_err().contains("no apps"));
        assert_eq!(cfg.profiles.len(), before);

        let i = add_profile(&mut cfg, profile_with(&[])).unwrap();
        assert_eq!(cfg.profiles[i].name, "Photo");
        let j = add_profile(&mut cfg, profile_with(&[])).unwrap();
        assert_eq!(cfg.profiles[j].name, "Photo (2)");

        while cfg.profiles.len() < MAX_PROFILES {
            add_profile(&mut cfg, profile_with(&[])).unwrap();
        }
        assert!(
            add_profile(&mut cfg, profile_with(&[]))
                .unwrap_err()
                .contains("already")
        );
    }

    #[test]
    fn export_file_names_are_safe() {
        assert_eq!(
            export_file_name("Photo / Édit"),
            "Photo----dit.cursorpie-profile.json"
        );
        assert_eq!(export_file_name("../.."), "profile.cursorpie-profile.json");
        assert_eq!(export_file_name(""), "profile.cursorpie-profile.json");
    }

    #[test]
    fn clean_apps_trims_and_dedupes() {
        let v: Vec<String> = [" a ", "", "b", "a"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(clean_apps(&v), vec!["a", "b"]);
    }
    #[test]
    fn app_lists_are_cleaned_per_os() {
        use crate::platform::Os;
        let v: Vec<String> = [" Chrome.exe ", "chrome.EXE", "", "code.exe"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(clean_apps_for(Os::Windows, &v), ["chrome.exe", "code.exe"]);
        // macOS keeps bundle ids exactly as typed
        assert_eq!(
            clean_apps_for(Os::Mac, &v),
            ["Chrome.exe", "chrome.EXE", "code.exe"]
        );
    }

    #[test]
    fn image_path_only_accepts_image_files() {
        assert_eq!(image_path("hello"), None);
        assert_eq!(image_path("app:com.apple.Safari"), None);
        assert_eq!(image_path("<>"), None);
        assert_eq!(image_path("  a.PNG ").unwrap(), PathBuf::from("a.PNG"));
        assert_eq!(image_path("/x/y.webp").unwrap(), PathBuf::from("/x/y.webp"));
        assert_eq!(
            image_path("~/pics/u.jpeg").unwrap(),
            crate::config::home().join("pics/u.jpeg")
        );
        assert_eq!(image_path("notes.txt"), None);
    }
}
