//! Desktop entries (`*.desktop`): the Linux counterpart of macOS bundle ids. The **app id** CursorPie
//! uses is the entry's file name, lower case, without `.desktop` (`firefox`, `org.mozilla.firefox`,
//! `google-chrome`). A window reports a raw class (`WM_CLASS`) that is mapped to that id through the
//! installed entries. Everything here is pure except [`scan`], which only reads files.

use std::path::{Path, PathBuf};

/// One installed application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Canonical app id.
    pub id: String,
    pub name: String,
    /// `StartupWMClass`: the window class the app says it uses.
    pub wm_class: Option<String>,
    /// The program the `Exec` line starts, lower case, without its directory (see
    /// [`exec_program`]). Many windows are named after it when the entry has no `StartupWMClass`.
    pub exec_name: Option<String>,
    /// Path of the `.desktop` file (what `gio launch` takes).
    pub path: String,
    /// `Icon`: an icon name (looked up in the icon themes, see `icontheme`) or an absolute path.
    pub icon: Option<String>,
    /// `NoDisplay=true`: the app is not meant to be listed in menus (URL handlers, helpers). It
    /// still has windows, so the index keeps it; a list of apps to pick from leaves it out.
    pub no_display: bool,
    /// `SingleMainWindow=true` (desktop entry spec 1.5): the app says it has one main window, so
    /// starting it again cannot give a new one.
    pub single_window: bool,
    /// The other names the entry has, lower case, one per line: `Name` in every language
    /// (`Name[id]=Berkas`), `GenericName` (`Web Browser`) and `Keywords`. Only the search box of the
    /// app list reads it: a menu finds `Files` as `Berkas` on an Indonesian desktop, so must this.
    pub search: String,
}

/// An app id in canonical form: trimmed, lower case, without a `.desktop` suffix.
pub fn id_key(s: &str) -> String {
    let t = s.trim().to_ascii_lowercase();
    match t.strip_suffix(".desktop") {
        Some(stem) => stem.to_string(),
        None => t,
    }
}

/// The app id of what the user typed or picked: a path is cut down to its file name (the `.desktop`
/// file or the program), then [`id_key`] applies.
pub fn name_key(s: &str) -> String {
    let t = s.trim();
    if t.contains('/') {
        return Path::new(t)
            .file_name()
            .and_then(|n| n.to_str())
            .map(id_key)
            .unwrap_or_default();
    }
    id_key(t)
}

/// The program an `Exec` line starts: lower case, without its directory (`firefox` for
/// `/usr/bin/firefox %u`). `env VAR=1 app` is looked through. A launcher that names nothing about
/// the window (`sh`, `flatpak`, `snap`...) gives `None`.
pub fn exec_program(exec: &str) -> Option<String> {
    let exec = exec.trim();
    // a quoted program may hold spaces: it ends at the closing quote
    let (mut first, rest) = match exec.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
        None => exec.split_once(char::is_whitespace).unwrap_or((exec, "")),
    };
    if first == "env" {
        first = rest
            .split_whitespace()
            .map(|w| w.trim_matches('"'))
            .find(|w| !w.contains('='))?;
    }
    let file = Path::new(first).file_name()?.to_str()?;
    let name = file.to_ascii_lowercase();
    if name.is_empty()
        || matches!(
            name.as_str(),
            "env" | "sh" | "bash" | "flatpak" | "snap" | "gio" | "xdg-open"
        )
    {
        None
    } else {
        Some(name)
    }
}

/// Parses the text of a desktop file. `file_name` is its name (`firefox.desktop`), `path` where it
/// is. Only applications that are not `Hidden` are returned.
pub fn parse(file_name: &str, path: &str, text: &str) -> Option<Entry> {
    let mut in_main = false;
    let mut kind: Option<String> = None;
    let mut name: Option<String> = None;
    let mut wm_class: Option<String> = None;
    let mut exec: Option<String> = None;
    let mut icon: Option<String> = None;
    let mut hidden = false;
    let mut no_display = false;
    let mut single_window = false;
    let mut search: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Type" => kind = Some(value.to_string()),
            "Name" => name = Some(value.to_string()),
            "StartupWMClass" if !value.is_empty() => wm_class = Some(value.to_string()),
            "Exec" => exec = Some(value.to_string()),
            "Icon" if !value.is_empty() => icon = Some(value.to_string()),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
            "SingleMainWindow" => single_window = value.eq_ignore_ascii_case("true"),
            k if k.starts_with("Name[") || k == "GenericName" || k.starts_with("GenericName[") => {
                if !value.is_empty() {
                    search.push(value.to_lowercase());
                }
            }
            k if k == "Keywords" || k.starts_with("Keywords[") => search.extend(
                value
                    .split(';')
                    .map(str::trim)
                    .filter(|w| !w.is_empty())
                    .map(str::to_lowercase),
            ),
            _ => {}
        }
    }
    if kind.as_deref() != Some("Application") || hidden {
        return None;
    }
    let id = id_key(file_name);
    if id.is_empty() {
        return None;
    }
    Some(Entry {
        name: name.unwrap_or_else(|| id.clone()),
        id,
        wm_class,
        exec_name: exec.as_deref().and_then(exec_program),
        path: path.to_string(),
        icon,
        no_display,
        single_window,
        search: search.join("\n"),
    })
}

/// The installed applications.
#[derive(Clone, Debug, Default)]
pub struct Index {
    entries: Vec<Entry>,
}

impl Index {
    pub fn new(entries: Vec<Entry>) -> Index {
        Index { entries }
    }

    /// Every installed app.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The entry with this app id (`firefox`, `Firefox.desktop` and `FIREFOX` are the same).
    pub fn find(&self, name: &str) -> Option<&Entry> {
        let key = id_key(name);
        self.entries.iter().find(|e| e.id == key)
    }

    /// The canonical app id for the raw identifiers a window reports (class, instance...), best
    /// candidate first. Order of preference: a `StartupWMClass` match, an exact id match, then an id
    /// that ends in `.<candidate>` when only one does (`org.mozilla.firefox` for `firefox`).
    pub fn canonical(&self, candidates: &[&str]) -> Option<String> {
        let keys: Vec<String> = candidates
            .iter()
            .map(|c| id_key(c))
            .filter(|k| !k.is_empty())
            .collect();
        for k in &keys {
            if let Some(e) = self.entries.iter().find(|e| {
                e.wm_class
                    .as_ref()
                    .is_some_and(|w| w.eq_ignore_ascii_case(k))
            }) {
                return Some(e.id.clone());
            }
            if let Some(e) = self.entries.iter().find(|e| &e.id == k) {
                return Some(e.id.clone());
            }
        }
        for k in &keys {
            let suffix = format!(".{k}");
            let mut hits = self.entries.iter().filter(|e| e.id.ends_with(&suffix));
            if let (Some(e), None) = (hits.next(), hits.next()) {
                return Some(e.id.clone());
            }
        }
        None
    }

    /// The installed entry that `app` stands for: `app` is what the user typed or picked, an app
    /// id, a window class or a program name (a path is cut down to its file name).
    pub fn entry_for(&self, app: &str) -> Option<&Entry> {
        let key = name_key(app);
        if key.is_empty() {
            return None;
        }
        self.find(&key)
            .or_else(|| {
                self.canonical(&[key.as_str()])
                    .and_then(|id| self.find(&id))
            })
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|e| e.exec_name.as_deref() == Some(key.as_str()))
            })
    }

    /// Every name a window of the app `app` may report, lower case. `app` is what the user typed or
    /// picked: an app id, a program name or a path. The result holds that name, and, when it names
    /// an installed entry, the entry's id, window class and program. The window manager's name for
    /// a window often differs from what was typed (`firefox` for `org.mozilla.firefox`,
    /// `gnome-terminal` for `org.gnome.terminal`), so "is this app running" compares sets of names.
    pub fn aliases(&self, app: &str) -> Vec<String> {
        let key = name_key(app);
        if key.is_empty() {
            return Vec::new();
        }
        let entry = self.entry_for(app);
        let mut out = vec![key.clone()];
        if let Some(e) = entry {
            out.push(e.id.clone());
            out.extend(e.wm_class.iter().map(|w| w.to_ascii_lowercase()));
            out.extend(e.exec_name.iter().cloned());
        }
        out.sort();
        out.dedup();
        out
    }
}

/// How many folders below an `applications` directory are read (`kde4/`, `wine/Programs/`...).
const SCAN_DEPTH: u32 = 3;

/// Reads the `*.desktop` files of the given `applications` directories and of the folders below
/// them, which the desktop entry spec allows: the id of `kde4/konsole.desktop` is
/// `kde4-konsole.desktop`. A directory listed earlier wins over a later one with the same id, so a
/// user's own entry overrides the system's.
pub fn scan(dirs: &[PathBuf]) -> Index {
    let mut entries: Vec<Entry> = Vec::new();
    for dir in dirs {
        scan_dir(dir, "", 0, &mut entries);
    }
    Index::new(entries)
}

/// Reads one directory: its own files first, then the folders in it. `prefix` is what the id of a
/// file in it starts with (the folder names joined with `-`).
fn scan_dir(dir: &Path, prefix: &str, depth: u32, entries: &mut Vec<Entry>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut folders: Vec<(PathBuf, String)> = Vec::new();
    for file in read.flatten() {
        let path = file.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if file_name.ends_with(".desktop") {
            let Some(text) = read_small(&path) else {
                continue;
            };
            if let Some(e) = parse(
                &format!("{prefix}{file_name}"),
                &path.to_string_lossy(),
                &text,
            ) && !entries.iter().any(|x| x.id == e.id)
            {
                entries.push(e);
            }
        } else if depth < SCAN_DEPTH && !file_name.starts_with('.') && path.is_dir() {
            folders.push((path.clone(), format!("{prefix}{file_name}-")));
        }
    }
    for (folder, prefix) in folders {
        scan_dir(&folder, &prefix, depth + 1, entries);
    }
}

/// The text of a file smaller than 64 KiB (desktop files are tiny). A byte that is not UTF-8
/// (a comment or a name in an old encoding) becomes a replacement character instead of making the
/// whole entry, and its app, vanish from the list.
fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > 64 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, wm: Option<&str>) -> Entry {
        Entry {
            id: id.into(),
            name: id.into(),
            wm_class: wm.map(str::to_string),
            exec_name: None,
            path: format!("/usr/share/applications/{id}.desktop"),
            icon: None,
            no_display: false,
            single_window: false,
            search: String::new(),
        }
    }

    const FIREFOX: &str = "# comment\n[Desktop Entry]\nName=Firefox\nName[de]=Feuerfuchs\nType=Application\nExec=firefox %u\nStartupWMClass=firefox\n\n[Desktop Action new-window]\nName=New Window\nType=Hidden\n";

    #[test]
    fn an_entry_can_say_it_has_one_main_window() {
        let one =
            "[Desktop Entry]\nType=Application\nName=Calc\nExec=calc\nSingleMainWindow=true\n";
        assert!(
            parse("calc.desktop", "/x/calc.desktop", one)
                .unwrap()
                .single_window
        );
        assert!(
            !parse("firefox.desktop", "/x/firefox.desktop", FIREFOX)
                .unwrap()
                .single_window
        );
    }

    #[test]
    fn ids_are_lower_case_without_the_suffix() {
        assert_eq!(id_key("  Firefox.desktop "), "firefox");
        assert_eq!(id_key("org.Mozilla.Firefox"), "org.mozilla.firefox");
        assert_eq!(id_key(".desktop"), "");
    }

    #[test]
    fn a_desktop_file_is_parsed() {
        let e = parse("Firefox.desktop", "/p/Firefox.desktop", FIREFOX).unwrap();
        assert_eq!(e.id, "firefox");
        assert_eq!(e.name, "Firefox", "the localized Name[de] is ignored");
        assert_eq!(e.wm_class.as_deref(), Some("firefox"));
        assert_eq!(e.path, "/p/Firefox.desktop");
    }

    #[test]
    fn the_icon_of_an_entry_is_read() {
        let text = "[Desktop Entry]\nType=Application\nName=A\nIcon=org.example.A\nIcon[de]=nope\n[Desktop Action x]\nIcon=other\n";
        let e = parse("a.desktop", "/a", text).unwrap();
        assert_eq!(e.icon.as_deref(), Some("org.example.A"));
        let none = parse(
            "b.desktop",
            "/b",
            "[Desktop Entry]\nType=Application\nName=B\nIcon=\n",
        );
        assert_eq!(none.unwrap().icon, None);
    }

    #[test]
    fn an_entry_is_found_by_id_class_or_program() {
        let mut firefox = entry("org.mozilla.firefox", Some("firefox"));
        firefox.exec_name = Some("firefox-bin".into());
        let idx = Index::new(vec![firefox, entry("gedit", None)]);
        for app in [
            "org.mozilla.firefox",
            "Firefox.desktop",
            "firefox",
            "/usr/bin/firefox-bin",
        ] {
            assert_eq!(
                idx.entry_for(app).map(|e| e.id.as_str()),
                Some("org.mozilla.firefox"),
                "{app}"
            );
        }
        assert_eq!(idx.entry_for("gedit").map(|e| e.id.as_str()), Some("gedit"));
        assert!(idx.entry_for("nothing").is_none());
        assert!(idx.entry_for("  ").is_none());
    }

    #[test]
    fn links_hidden_and_broken_entries_are_skipped() {
        assert!(parse("a.desktop", "/a", "[Desktop Entry]\nType=Link\nName=A").is_none());
        assert!(
            parse(
                "a.desktop",
                "/a",
                "[Desktop Entry]\nType=Application\nHidden=true"
            )
            .is_none()
        );
        assert!(parse("a.desktop", "/a", "garbage").is_none());
        assert!(parse(".desktop", "/a", "[Desktop Entry]\nType=Application").is_none());
        let named = parse("tool.desktop", "/t", "[Desktop Entry]\nType=Application").unwrap();
        assert_eq!(named.name, "tool", "falls back to the id");
    }

    #[test]
    fn a_window_class_maps_to_the_canonical_id() {
        let idx = Index::new(vec![
            entry("org.mozilla.firefox", Some("firefox")),
            entry("google-chrome", Some("Google-chrome")),
            entry("code", Some("Code")),
            entry("org.gnome.nautilus", None),
        ]);
        assert_eq!(
            idx.canonical(&["firefox"]).as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            idx.canonical(&["Google-chrome", "google-chrome"])
                .as_deref(),
            Some("google-chrome")
        );
        assert_eq!(idx.canonical(&["Code"]).as_deref(), Some("code"));
        assert_eq!(
            idx.canonical(&["nautilus"]).as_deref(),
            Some("org.gnome.nautilus")
        );
        assert_eq!(idx.canonical(&["xterm"]), None);
        assert_eq!(idx.canonical(&[]), None);
        assert_eq!(idx.canonical(&["  "]), None);
    }

    #[test]
    fn a_suffix_match_needs_a_unique_entry() {
        let idx = Index::new(vec![
            entry("org.a.player", None),
            entry("org.b.player", None),
        ]);
        assert_eq!(idx.canonical(&["player"]), None, "two candidates: no guess");
        let one = Index::new(vec![entry("org.a.player", None)]);
        assert_eq!(one.canonical(&["player"]).as_deref(), Some("org.a.player"));
    }

    #[test]
    fn the_wm_class_beats_an_id_of_the_same_name() {
        let idx = Index::new(vec![
            entry("editor", None),
            entry("my-editor", Some("editor")),
        ]);
        assert_eq!(idx.canonical(&["editor"]).as_deref(), Some("my-editor"));
    }

    #[test]
    fn find_ignores_case_and_the_suffix() {
        let idx = Index::new(vec![entry("firefox", None)]);
        assert!(idx.find("Firefox.desktop").is_some());
        assert!(idx.find("FIREFOX").is_some());
        assert!(idx.find("chrome").is_none());
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn typed_names_and_paths_are_cut_down_to_an_id() {
        assert_eq!(name_key(" Firefox.desktop "), "firefox");
        assert_eq!(name_key("/usr/share/applications/A.desktop"), "a");
        assert_eq!(name_key("/opt/App/Run"), "run");
        assert_eq!(name_key("/"), "");
        assert_eq!(name_key(""), "");
    }

    #[test]
    fn the_program_of_an_exec_line() {
        let prog = exec_program;
        assert_eq!(prog("firefox %u").as_deref(), Some("firefox"));
        assert_eq!(prog("/usr/bin/Code %F").as_deref(), Some("code"));
        assert_eq!(prog("\"/opt/My App/run\" %f").as_deref(), Some("run"));
        assert_eq!(prog("env A=1 app %U").as_deref(), Some("app"));
        assert_eq!(prog("flatpak run org.a.B"), None);
        assert_eq!(prog("sh -c app"), None);
        assert_eq!(prog(""), None);
        assert_eq!(prog("env A=1"), None);
    }

    #[test]
    fn parse_keeps_the_program_of_the_exec_line() {
        let e = parse("Firefox.desktop", "/p/Firefox.desktop", FIREFOX).unwrap();
        assert_eq!(e.exec_name.as_deref(), Some("firefox"));
        let bare = "[Desktop Entry]\nType=Application";
        assert_eq!(parse("a.desktop", "/a", bare).unwrap().exec_name, None);
    }

    #[test]
    fn aliases_cover_every_name_a_window_may_have() {
        let mut terminal = entry("org.gnome.terminal", None);
        terminal.exec_name = Some("gnome-terminal".into());
        let idx = Index::new(vec![
            entry("org.mozilla.firefox", Some("Firefox")),
            terminal,
        ]);
        let has = |list: &[String], name: &str| list.iter().any(|a| a == name);
        // a short name finds the entry through the suffix rule
        let a = idx.aliases("firefox");
        assert!(has(&a, "firefox"));
        assert!(has(&a, "org.mozilla.firefox"));
        // a program name finds it through the Exec line
        let t = idx.aliases("GNOME-Terminal");
        assert!(has(&t, "gnome-terminal"));
        assert!(has(&t, "org.gnome.terminal"));
        // a path to the desktop file names the same app
        let p = idx.aliases("/x/org.gnome.terminal.desktop");
        assert!(has(&p, "gnome-terminal"));
    }

    #[test]
    fn aliases_of_an_unknown_app_are_its_own_name() {
        let idx = Index::default();
        assert_eq!(idx.aliases("/usr/bin/Htop"), ["htop"]);
        assert_eq!(idx.aliases(" xterm "), ["xterm"]);
        assert!(idx.aliases("  ").is_empty());
    }

    #[test]
    fn scan_reads_files_and_the_first_directory_wins() {
        let base = std::env::temp_dir().join(format!("cursorpie-de-{}", std::process::id()));
        let user = base.join("user");
        let system = base.join("system");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(
            user.join("app.desktop"),
            "[Desktop Entry]\nType=Application\nName=Mine",
        )
        .unwrap();
        std::fs::write(
            system.join("app.desktop"),
            "[Desktop Entry]\nType=Application\nName=Theirs",
        )
        .unwrap();
        std::fs::write(system.join("notes.txt"), "x").unwrap();
        let idx = scan(&[user.clone(), system.clone(), base.join("missing")]);
        assert_eq!(idx.len(), 1);
        assert_eq!(idx.find("app").unwrap().name, "Mine");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_other_names_of_an_entry_are_kept_for_the_search() {
        let text = "[Desktop Entry]\nType=Application\nName=Files\nName[id]=Berkas\nName[de]=Dateien\nGenericName=File Manager\nKeywords=Folder;Manager;Explore;\nKeywords[id]=Folder;Jelajah;\n";
        let e = parse("org.gnome.nautilus.desktop", "/p", text).unwrap();
        assert_eq!(e.name, "Files");
        let found: Vec<&str> = e.search.lines().collect();
        for word in ["berkas", "dateien", "file manager", "explore", "jelajah"] {
            assert!(found.contains(&word), "{word} is searchable: {found:?}");
        }
    }

    #[test]
    fn scan_reads_the_folders_below_and_bytes_that_are_not_utf8() {
        let base = std::env::temp_dir().join(format!("cursorpie-de-sub-{}", std::process::id()));
        let sub = base.join("kde4");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("konsole.desktop"),
            "[Desktop Entry]\nType=Application\nName=Konsole\n",
        )
        .unwrap();
        // a Latin-1 byte in a comment: the entry is still there
        let mut old = b"[Desktop Entry]\nType=Application\nName=Old\n# caf".to_vec();
        old.push(0xE9);
        std::fs::write(base.join("old.desktop"), old).unwrap();
        let idx = scan(std::slice::from_ref(&base));
        assert!(idx.find("kde4-konsole").is_some(), "{:?}", idx.entries());
        assert_eq!(idx.find("old").unwrap().name, "Old");
        let _ = std::fs::remove_dir_all(&base);
    }
}
