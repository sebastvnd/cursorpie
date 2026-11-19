//! Which themes there are: the built-in ones and the files of the themes folder.
//!
//! The folder is `themes/` next to `config.json` (see [`crate::config::themes_dir`]). Every
//! `*.json` file in it is a theme (see `spec.rs`) whose id is the name of the file without the
//! extension. A file that cannot be read is skipped, and what was wrong with it is kept in
//! [`problems`] for Settings to show, so a typo never takes the app down or hides the other themes.
//!
//! The folder is read once, when the first theme is asked for, and again by [`reload`] (the button
//! of Behavior, Look & feel), so a person can edit a file and see the change without a restart.

use super::builtin;
use super::spec::ThemeFile;
use super::tokens::Theme;
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The biggest theme file that is read, in bytes. A theme is a few hundred bytes of JSON.
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
/// How many files are read: the rest are not looked at.
pub const MAX_THEMES: usize = 64;
/// How many themes a theme can extend in a row: `a` extends `b` extends `c`...
const MAX_DEPTH: usize = 8;
/// Longest id (the name of a file).
const MAX_ID: usize = 48;

/// What reading the folder found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loaded {
    /// The themes of the files, by name.
    pub themes: Vec<Theme>,
    /// One line for each file that was skipped, and why.
    pub problems: Vec<String>,
}

static USER: RwLock<Loaded> = RwLock::new(Loaded {
    themes: Vec::new(),
    problems: Vec::new(),
});
static READ: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// The id a file name gives, or `None` when it is not a theme file name: a `.json` file whose name,
/// without the extension, is made of letters, digits, `-` and `_` only (so that it is safe to keep
/// in the config) and is not too long. Upper case letters are folded.
pub fn id_of(file_name: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".json").or_else(|| {
        // the extension in any case
        let n = file_name.len().checked_sub(5)?;
        file_name
            .get(n..)
            .filter(|e| e.eq_ignore_ascii_case(".json"))
            .and_then(|_| file_name.get(..n))
    })?;
    let id = stem.to_ascii_lowercase();
    let ok = !id.is_empty()
        && id.len() <= MAX_ID
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    ok.then_some(id)
}

/// Whether `id` can be stored as a choice of theme: what [`id_of`] allows. A choice that is not
/// valid is cut back to the default when the config is sanitized.
pub fn is_valid_id(id: &str) -> bool {
    id_of(&format!("{id}.json")).is_some_and(|clean| clean == id)
}

/// Turns the parsed files into themes. A file extends the theme its `extends` names (sage when it
/// names none): a built-in one or another file, up to `MAX_DEPTH` deep. A name that does not
/// exist, a loop and a file whose name is a built-in id are reported and skipped.
pub fn resolve_all(files: Vec<(String, ThemeFile)>) -> Loaded {
    let mut out = Loaded::default();
    let files: HashMap<String, ThemeFile> = files.into_iter().collect();
    let mut ids: Vec<&String> = files.keys().collect();
    ids.sort();
    let mut done: HashMap<String, Result<Theme, String>> = HashMap::new();
    for id in ids {
        match resolve_one(id, &files, &mut done, 0) {
            Ok(theme) => out.themes.push(theme),
            Err(why) => out.problems.push(format!("{id}.json: {why}")),
        }
    }
    out.themes.sort_by_key(|t| t.name.to_lowercase());
    out
}

fn resolve_one(
    id: &str,
    files: &HashMap<String, ThemeFile>,
    done: &mut HashMap<String, Result<Theme, String>>,
    depth: usize,
) -> Result<Theme, String> {
    if let Some(known) = done.get(id) {
        return known.clone();
    }
    let result = (|| {
        if builtin::find(id).is_some() {
            return Err("this name belongs to a theme that ships with CursorPie".to_owned());
        }
        if depth >= MAX_DEPTH {
            return Err("it extends too many themes in a row (or itself)".to_owned());
        }
        let file = files.get(id).ok_or("no such theme")?;
        let base_id = file.extends.as_deref().unwrap_or(builtin::DEFAULT_ID);
        let base = match builtin::find(base_id) {
            Some(base) => base,
            None if files.contains_key(base_id) => {
                // mark this one as in progress, so that a loop ends at its second visit
                done.insert(id.to_owned(), Err("it extends itself".to_owned()));
                resolve_one(base_id, files, done, depth + 1)
                    .map_err(|why| format!("it extends '{base_id}', which is not usable ({why})"))?
            }
            None => return Err(format!("it extends '{base_id}', which does not exist")),
        };
        Ok(file.resolve(id, &base))
    })();
    done.insert(id.to_owned(), result.clone());
    result
}

/// Reads the theme files of `dir`. A folder that is not there has no themes and no problems.
pub fn load_dir(dir: &Path) -> Loaded {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Loaded::default();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.to_ascii_lowercase().ends_with(".json"))
        .collect();
    names.sort();
    let mut problems = Vec::new();
    if names.len() > MAX_THEMES {
        problems.push(format!(
            "{} theme files: only the first {MAX_THEMES} are read",
            names.len()
        ));
        names.truncate(MAX_THEMES);
    }
    let mut files = Vec::new();
    for name in names {
        let Some(id) = id_of(&name) else {
            problems.push(format!(
                "{name}: the name may only have letters, digits, - and _ (up to {MAX_ID})"
            ));
            continue;
        };
        let path = dir.join(&name);
        match std::fs::metadata(&path) {
            Ok(m) if m.len() > MAX_FILE_BYTES => {
                problems.push(format!("{name}: too big for a theme (over 64 KB)"));
                continue;
            }
            Err(_) => {
                problems.push(format!("{name}: cannot be read"));
                continue;
            }
            Ok(_) => {}
        }
        match std::fs::read_to_string(&path)
            .map_err(|_| "cannot be read".to_owned())
            .and_then(|text| ThemeFile::parse(&text))
        {
            Ok(file) => files.push((id, file)),
            Err(why) => problems.push(format!("{name}: {why}")),
        }
    }
    let mut loaded = resolve_all(files);
    problems.append(&mut loaded.problems);
    loaded.problems = problems;
    loaded
}

/// Reads the themes folder again. Everything that was drawn from the old list is drawn again on the
/// next frame (see [`generation`]).
pub fn reload() {
    let loaded = load_dir(&crate::config::themes_dir());
    *USER.write().unwrap_or_else(|e| e.into_inner()) = loaded;
    READ.store(true, Ordering::SeqCst);
    GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Reads the folder if it has not been read yet.
pub fn ensure_loaded() {
    if !READ.load(Ordering::SeqCst) {
        reload();
    }
}

/// Changes whenever the list of themes does, so a cache of a resolved theme knows when it is stale.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// Every theme: the built-in ones first, then the files, by name.
pub fn all() -> Vec<Theme> {
    ensure_loaded();
    let mut all = builtin::all();
    all.extend(
        USER.read()
            .unwrap_or_else(|e| e.into_inner())
            .themes
            .clone(),
    );
    all
}

/// The theme with this id. An id that is not there (a file that was removed, a config from a newer
/// version) is the default theme, and the id is left as it is in the config, so the theme comes
/// back when its file does.
pub fn find(id: &str) -> Theme {
    ensure_loaded();
    builtin::find(id)
        .or_else(|| {
            USER.read()
                .unwrap_or_else(|e| e.into_inner())
                .themes
                .iter()
                .find(|t| t.id == id)
                .cloned()
        })
        .unwrap_or_else(builtin::sage)
}

/// Whether a theme with this id exists right now.
pub fn exists(id: &str) -> bool {
    all().iter().any(|t| t.id == id)
}

/// What was wrong with the files of the last reading, one line for each.
pub fn problems() -> Vec<String> {
    ensure_loaded();
    USER.read()
        .unwrap_or_else(|e| e.into_inner())
        .problems
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> Vec<(String, ThemeFile)> {
        list.iter()
            .map(|(id, text)| (id.to_string(), ThemeFile::parse(text).unwrap()))
            .collect()
    }

    #[test]
    fn a_file_name_gives_an_id_only_when_it_is_safe_to_keep() {
        assert_eq!(id_of("bubblegum.json").as_deref(), Some("bubblegum"));
        assert_eq!(id_of("My-Theme_2.JSON").as_deref(), Some("my-theme_2"));
        for bad in [
            "theme.txt",
            ".json",
            "my theme.json",
            "t\u{e9}.json",
            "../x.json",
            "a/b.json",
            "json",
        ] {
            assert_eq!(id_of(bad), None, "{bad}");
        }
        assert_eq!(id_of(&format!("{}.json", "a".repeat(MAX_ID + 1))), None);
        assert!(id_of(&format!("{}.json", "a".repeat(MAX_ID))).is_some());
    }

    #[test]
    fn only_a_clean_id_can_be_chosen() {
        assert!(is_valid_id("sage") && is_valid_id("my-theme_2"));
        assert!(!is_valid_id("") && !is_valid_id("My") && !is_valid_id("a b"));
        assert!(!is_valid_id("a.json") && !is_valid_id("../x"));
    }

    #[test]
    fn a_file_without_extends_is_a_layer_over_sage() {
        let loaded = resolve_all(files(&[("mine", r##"{"accent":"#112233"}"##)]));
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let t = &loaded.themes[0];
        assert_eq!(t.shape, builtin::sage().shape);
        assert_eq!(t.accent, [0x11, 0x22, 0x33]);
    }

    #[test]
    fn a_file_can_extend_a_built_in_theme_or_another_file() {
        let loaded = resolve_all(files(&[
            ("a", r#"{"extends":"genz","name":"Alpha"}"#),
            ("b", r##"{"extends":"a","accent":"#010203"}"##),
        ]));
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let by = |id: &str| loaded.themes.iter().find(|t| t.id == id).unwrap();
        assert_eq!(by("a").shape, builtin::genz().shape);
        assert_eq!(by("b").shape, builtin::genz().shape, "through a");
        assert_eq!(by("b").accent, [1, 2, 3]);
        assert_eq!(by("b").light, builtin::genz().light);
    }

    #[test]
    fn a_theme_that_cannot_be_made_is_reported_and_the_others_still_are() {
        let loaded = resolve_all(files(&[
            ("good", "{}"),
            ("lost", r#"{"extends":"nowhere"}"#),
            ("loop1", r#"{"extends":"loop2"}"#),
            ("loop2", r#"{"extends":"loop1"}"#),
            ("self", r#"{"extends":"self"}"#),
            ("sage", "{}"),
        ]));
        let ids: Vec<&str> = loaded.themes.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["good"]);
        let said = |id: &str| loaded.problems.iter().any(|p| p.starts_with(id));
        for id in [
            "lost.json",
            "loop1.json",
            "loop2.json",
            "self.json",
            "sage.json",
        ] {
            assert!(said(id), "{id}: {:?}", loaded.problems);
        }
    }

    #[test]
    fn the_themes_are_listed_by_name() {
        let loaded = resolve_all(files(&[
            ("z", r#"{"name":"Apple"}"#),
            ("a", r#"{"name":"banana"}"#),
        ]));
        let names: Vec<&str> = loaded.themes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["Apple", "banana"]);
    }

    #[test]
    fn a_folder_is_read_and_what_is_wrong_in_it_is_said() {
        let dir = std::env::temp_dir().join(format!("cursorpie-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let put = |name: &str, text: &str| std::fs::write(dir.join(name), text).unwrap();
        put("ok.json", r##"{"extends":"genz","accent":"#ff5ea8"}"##);
        put("broken.json", "{ nope");
        put("badcolor.json", r#"{"accent":"banana"}"#);
        put("Bad Name.json", "{}");
        put("notes.txt", "not a theme");
        put("huge.json", &" ".repeat(MAX_FILE_BYTES as usize + 1));
        let loaded = load_dir(&dir);
        let _ = std::fs::remove_dir_all(&dir);
        let ids: Vec<&str> = loaded.themes.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["ok"]);
        assert_eq!(loaded.problems.len(), 4, "{:?}", loaded.problems);
        assert!(loaded.problems.iter().any(|p| p.contains("banana")));
        assert!(loaded.problems.iter().any(|p| p.contains("too big")));
    }

    #[test]
    fn a_folder_that_is_not_there_has_no_themes() {
        let loaded = load_dir(Path::new("/no/such/folder/for/themes"));
        assert_eq!(loaded, Loaded::default());
    }

    #[test]
    fn an_unknown_id_is_the_default_theme() {
        assert_eq!(find("no-such-theme-anywhere").id, builtin::DEFAULT_ID);
        assert_eq!(find("genz").id, "genz");
        assert!(exists("sage") && exists("genz") && !exists("no-such-theme-anywhere"));
    }

    #[test]
    fn the_example_of_the_docs_is_a_theme() {
        let text = include_str!("../../../../docs/examples/themes/bubblegum.json");
        let loaded = resolve_all(vec![("bubblegum".into(), ThemeFile::parse(text).unwrap())]);
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        assert_eq!(loaded.themes[0].name, "Bubblegum");
        assert_eq!(loaded.themes[0].shape.control_r, 14);
    }
}
