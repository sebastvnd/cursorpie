//! What a flow remembers between runs: the saved state of one flow and the data every flow shares.
//!
//! A run of a flow has variables ([`crate::flowrun::Vars`]), and they are gone when the run ends.
//! This module is the part that stays on disk, in the folder `flow-state` of the app's support
//! directory. There are two kinds of things in it:
//!
//! * **The state of a flow** (`<flow id>.state.json`): written when a run ends, read when the next
//!   one starts, also after CursorPie was closed and opened again. It holds
//!   - the variables whose name starts with `keep_` ([`KEEP_PREFIX`]): a flow that wants to
//!     remember a counter, the last file it handled or a choice the person made, keeps it in
//!     `keep_counter` and finds it there in the next run;
//!   - how many times the flow ran, when the last run ended and how it ended. The run sees these as
//!     the variables `run_count`, `last_run` and `last_ending` (see
//!     [`crate::flowrun`]).
//! * **The shared data** (`data.json`): names with a text as value that every flow can write
//!   (*Save data*), read (*Load data*) and remove (*Remove data*). It is how one flow leaves
//!   something for another, or for itself in a later run.
//!
//! Everything here is plain JSON, written through a temporary file and renamed, so a crash or a
//! full disk never leaves half a file. A file that cannot be read is never overwritten without a
//! trace: the shared data is moved to `data.corrupt.json` first, the state of a flow is simply
//! started again (it holds nothing a flow cannot make again).
//!
//! The functions take the directory as an argument, so the tests use a scratch folder.
//! [`state_dir`] is the real one.

use crate::config::{MAX_VALUE_CHARS, MAX_VARS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A variable whose name starts with this is saved when a run ends and is there again in the next
/// run of the same flow.
pub const KEEP_PREFIX: &str = "keep_";
/// The variables every run of a flow starts with when the host keeps state: how many times the flow
/// ran (this one included), when the last run ended and how.
pub const RUN_FACTS: [&str; 3] = ["run_count", "last_run", "last_ending"];
/// How many `keep_` variables a state holds: half of the variables of a run, so the ones the run
/// sets for itself always have room.
pub const MAX_KEPT_VARS: usize = MAX_VARS / 2;
/// How many names the shared data holds.
pub const MAX_STORE_KEYS: usize = 512;
/// The longest name in the shared data, in characters.
pub const MAX_KEY_CHARS: usize = 64;
/// The biggest state or data file that is read; a bigger one is not CursorPie's.
pub const MAX_STATE_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// The format of the files, for a later change of them.
pub const FORMAT_VERSION: u32 = 1;

/// Whether a variable is saved from run to run: it starts with [`KEEP_PREFIX`] and has more after it.
pub fn is_keep_var(name: &str) -> bool {
    name.len() > KEEP_PREFIX.len() && name.starts_with(KEEP_PREFIX)
}

/// Whether `key` is a name the shared data takes: letters, digits and `_ . - :`, at most
/// [`MAX_KEY_CHARS`] of them.
pub fn is_store_key(key: &str) -> bool {
    !key.is_empty()
        && key.chars().count() <= MAX_KEY_CHARS
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | ':'))
}

/// Cuts a text to [`MAX_VALUE_CHARS`] characters, like a variable.
fn cap(value: &str) -> String {
    if value.chars().count() > MAX_VALUE_CHARS {
        value.chars().take(MAX_VALUE_CHARS).collect()
    } else {
        value.to_string()
    }
}

/// What a flow leaves behind when a run ends.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowState {
    /// [`FORMAT_VERSION`] of the file.
    #[serde(default)]
    pub version: u32,
    /// How many runs of the flow ended so far (all endings count).
    #[serde(default)]
    pub runs: u64,
    /// When the last run ended, in Unix seconds; `0` when the flow never ran.
    #[serde(default)]
    pub last_run: u64,
    /// How the last run ended: `finished`, `stopped`, `out_of_steps`, `out_of_time` or
    /// `cancelled`; empty when the flow never ran.
    #[serde(default)]
    pub last_ending: String,
    /// The variables that were kept (see [`is_keep_var`]).
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
}

impl FlowState {
    /// Keeps `vars` (those [`is_keep_var`] says yes to, at most [`MAX_KEPT_VARS`], each cut to
    /// [`MAX_VALUE_CHARS`]) in a state that says the flow has run once more.
    pub fn after_run<'a>(
        &self,
        ending: &str,
        when: u64,
        vars: impl Iterator<Item = (&'a str, &'a str)>,
    ) -> FlowState {
        let kept: BTreeMap<String, String> = vars
            .filter(|(name, _)| is_keep_var(name))
            .take(MAX_KEPT_VARS)
            .map(|(name, value)| (name.to_string(), cap(value)))
            .collect();
        FlowState {
            version: FORMAT_VERSION,
            runs: self.runs.saturating_add(1),
            last_run: when,
            last_ending: ending.to_string(),
            vars: kept,
        }
    }
}

/// The names with their texts that every flow shares.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataStore {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub entries: BTreeMap<String, String>,
}

impl DataStore {
    /// The text saved under `key`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Saves a text under `key`, replacing what was there. `false` (and nothing changes) for a name
    /// [`is_store_key`] refuses, or when [`MAX_STORE_KEYS`] names are in use and this one is new.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let full = self.entries.len() >= MAX_STORE_KEYS && !self.entries.contains_key(key);
        if !is_store_key(key) || full {
            return false;
        }
        self.entries.insert(key.to_string(), cap(value));
        true
    }

    /// Removes `key`; whether there was something under it.
    pub fn remove(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }

    /// The names, sorted.
    pub fn keys(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }
}

// ------------------------------------------------------------------ on disk

/// The folder the real files are in: `flow-state` inside the app's support directory.
pub fn state_dir() -> PathBuf {
    crate::config::app_support_dir().join("flow-state")
}

/// A file name for a flow id: letters, digits, `-` and `_` stay, everything else becomes `%XX` for
/// each byte, so two ids never share a file and an id never leaves the folder.
fn stem(flow_id: &str) -> String {
    let mut out = String::with_capacity(flow_id.len());
    for b in flow_id.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    if out.is_empty() {
        out.push_str("%00");
    }
    out
}

/// The file with the state of the flow `flow_id`.
pub fn state_path(dir: &Path, flow_id: &str) -> PathBuf {
    dir.join(format!("{}.state.json", stem(flow_id)))
}

/// The file with the shared data.
pub fn data_path(dir: &Path) -> PathBuf {
    dir.join("data.json")
}

/// Reads a text file of at most [`MAX_STATE_FILE_BYTES`]; `None` when it is missing, too big or
/// not text.
fn read_capped(path: &Path) -> Option<String> {
    crate::config::read_text_capped(path, MAX_STATE_FILE_BYTES)
}

/// Writes `text` to `path` through a temporary file next to it (the folder is made when missing).
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// The saved state of a flow. A flow that never ran, and a file that cannot be read, give the
/// empty state (`runs` 0): the flow starts again from nothing. Never `None`; the caller does not
/// have to tell the two cases apart.
pub fn load_state(dir: &Path, flow_id: &str) -> FlowState {
    read_capped(&state_path(dir, flow_id))
        .and_then(|t| serde_json::from_str::<FlowState>(&t).ok())
        .unwrap_or_default()
}

/// Saves the state of a flow.
pub fn save_state(dir: &Path, flow_id: &str, state: &FlowState) -> io::Result<()> {
    let text = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    write_atomic(&state_path(dir, flow_id), &text)
}

/// Forgets the saved state of a flow (nothing to forget is fine).
pub fn clear_state(dir: &Path, flow_id: &str) -> io::Result<()> {
    match fs::remove_file(state_path(dir, flow_id)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Reads the shared data. Missing gives an empty store; a file that is there but cannot be read
/// is moved to `data.corrupt.json` (so the next save does not destroy it) and the store starts empty.
pub fn load_data(dir: &Path) -> DataStore {
    let path = data_path(dir);
    let Some(text) = read_capped(&path) else {
        if path.exists() {
            let _ = fs::rename(&path, dir.join("data.corrupt.json"));
        }
        return DataStore::default();
    };
    match serde_json::from_str::<DataStore>(&text) {
        Ok(mut data) => {
            // names written by hand that the shared data would refuse are left out
            data.entries.retain(|k, _| is_store_key(k));
            data.entries.iter_mut().for_each(|(_, v)| *v = cap(v));
            data
        }
        Err(_) => {
            let _ = fs::rename(&path, dir.join("data.corrupt.json"));
            DataStore::default()
        }
    }
}

/// Saves the shared data.
pub fn save_data(dir: &Path, data: &DataStore) -> io::Result<()> {
    let mut data = data.clone();
    data.version = FORMAT_VERSION;
    let text = serde_json::to_string_pretty(&data).map_err(io::Error::other)?;
    write_atomic(&data_path(dir), &text)
}

/// The shared data with its folder: every change is written to disk before it is reported done,
/// and undone in memory when the disk refuses.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    data: DataStore,
}

impl Store {
    /// Opens the store of `dir` (reads `data.json` when it is there).
    pub fn open(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        let data = load_data(&dir);
        Self { dir, data }
    }

    /// The text saved under `key`.
    pub fn get(&self, key: &str) -> Option<String> {
        self.data.get(key).map(str::to_string)
    }

    /// The names that are in use, sorted.
    pub fn keys(&self) -> Vec<String> {
        self.data.keys()
    }

    /// Saves a text under `key`; whether it is on disk now.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        let before = self.data.entries.get(key).cloned();
        if !self.data.set(key, value) {
            return false;
        }
        if save_data(&self.dir, &self.data).is_ok() {
            return true;
        }
        match before {
            Some(old) => {
                self.data.entries.insert(key.to_string(), old);
            }
            None => {
                self.data.entries.remove(key);
            }
        }
        false
    }

    /// Removes `key`; whether there was something under it and it is gone from disk too.
    pub fn remove(&mut self, key: &str) -> bool {
        let Some(old) = self.data.entries.remove(key) else {
            return false;
        };
        if save_data(&self.dir, &self.data).is_ok() {
            return true;
        }
        self.data.entries.insert(key.to_string(), old);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("cursorpie-flowstate-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn only_keep_variables_with_a_name_after_the_prefix_are_kept() {
        assert!(is_keep_var("keep_count"));
        assert!(is_keep_var("keep_x"));
        assert!(!is_keep_var("keep_"));
        assert!(!is_keep_var("keep"));
        assert!(!is_keep_var("count"));
        assert!(!is_keep_var("my_keep_count"));
    }

    #[test]
    fn store_keys_are_short_plain_names() {
        assert!(is_store_key("last.file"));
        assert!(is_store_key("app:counter-1"));
        assert!(!is_store_key(""));
        assert!(!is_store_key("a b"));
        assert!(!is_store_key("../x"));
        assert!(!is_store_key(&"k".repeat(MAX_KEY_CHARS + 1)));
        assert!(is_store_key(&"k".repeat(MAX_KEY_CHARS)));
    }

    #[test]
    fn an_after_run_state_counts_the_run_and_keeps_only_keep_variables() {
        let before = FlowState {
            runs: 4,
            ..FlowState::default()
        };
        let vars = [("keep_n", "7"), ("ok", "1"), ("tmp", "x")];
        let after = before.after_run("finished", 1_000, vars.into_iter());
        assert_eq!(after.runs, 5);
        assert_eq!(after.last_run, 1_000);
        assert_eq!(after.last_ending, "finished");
        assert_eq!(after.vars.len(), 1);
        assert_eq!(after.vars["keep_n"], "7");
        assert_eq!(after.version, FORMAT_VERSION);
    }

    #[test]
    fn a_kept_value_is_cut_like_a_variable() {
        let long = "é".repeat(MAX_VALUE_CHARS + 10);
        let vars = [("keep_a", long.as_str())];
        let after = FlowState::default().after_run("finished", 1, vars.into_iter());
        assert_eq!(after.vars["keep_a"].chars().count(), MAX_VALUE_CHARS);
    }

    #[test]
    fn the_state_of_a_flow_survives_a_restart() {
        let d = scratch("state");
        assert_eq!(load_state(&d, "f1"), FlowState::default(), "never ran");
        let s = FlowState::default().after_run("stopped", 42, [("keep_last", "a.txt")].into_iter());
        save_state(&d, "f1", &s).unwrap();
        // "restart": read again with nothing in memory
        assert_eq!(load_state(&d, "f1"), s);
        assert_eq!(
            load_state(&d, "f2"),
            FlowState::default(),
            "own file per flow"
        );
        clear_state(&d, "f1").unwrap();
        clear_state(&d, "f1").unwrap();
        assert_eq!(load_state(&d, "f1"), FlowState::default());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_flow_id_can_never_leave_the_folder_or_share_a_file() {
        let d = Path::new("/x");
        assert_eq!(state_path(d, "../evil"), d.join("%2E%2E%2Fevil.state.json"));
        assert_ne!(state_path(d, "a/b"), state_path(d, "a_b"));
        assert_ne!(state_path(d, "a.b"), state_path(d, "a%2Eb"));
        assert_eq!(state_path(d, "flow_1-x"), d.join("flow_1-x.state.json"));
        assert!(state_path(d, "").to_string_lossy().ends_with(".state.json"));
    }

    #[test]
    fn a_state_file_that_is_not_json_starts_the_flow_again() {
        let d = scratch("state-bad");
        fs::write(state_path(&d, "f1"), "not json {").unwrap();
        assert_eq!(load_state(&d, "f1"), FlowState::default());
        // a file from a newer version with more fields is still read
        fs::write(
            state_path(&d, "f1"),
            r#"{"version":9,"runs":3,"future":true,"vars":{"keep_a":"1"}}"#,
        )
        .unwrap();
        let s = load_state(&d, "f1");
        assert_eq!(s.runs, 3);
        assert_eq!(s.vars["keep_a"], "1");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_shared_data_is_there_after_the_store_is_opened_again() {
        let d = scratch("store");
        let mut s = Store::open(&d);
        assert_eq!(s.get("a"), None);
        assert!(s.set("a", "one"));
        assert!(s.set("a", "two"));
        assert!(s.set("b.c", "three"));
        assert!(!s.set("not allowed", "x"));
        assert!(!s.remove("missing"));
        drop(s);
        let mut s = Store::open(&d);
        assert_eq!(s.get("a").as_deref(), Some("two"));
        assert_eq!(s.keys(), vec!["a".to_string(), "b.c".to_string()]);
        assert!(s.remove("a"));
        drop(s);
        let s = Store::open(&d);
        assert_eq!(s.get("a"), None);
        assert_eq!(s.get("b.c").as_deref(), Some("three"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_shared_data_has_a_limit_and_a_full_store_still_changes_old_names() {
        let mut data = DataStore::default();
        for i in 0..MAX_STORE_KEYS {
            assert!(data.set(&format!("k{i}"), "v"));
        }
        assert!(!data.set("one_too_many", "v"));
        assert!(data.set("k0", "changed"));
        assert_eq!(data.get("k0"), Some("changed"));
        assert!(data.remove("k1"));
        assert!(data.set("one_too_many", "v"));
    }

    #[test]
    fn a_broken_data_file_is_moved_aside_and_not_overwritten() {
        let d = scratch("store-bad");
        fs::write(data_path(&d), "{ broken").unwrap();
        let mut s = Store::open(&d);
        assert_eq!(s.keys(), Vec::<String>::new());
        let aside = d.join("data.corrupt.json");
        assert_eq!(fs::read_to_string(&aside).unwrap(), "{ broken");
        assert!(s.set("a", "1"));
        assert!(fs::read_to_string(&aside).unwrap().contains("broken"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_write_is_undone_in_memory() {
        let d = scratch("store-fail");
        let mut s = Store::open(&d);
        assert!(s.set("a", "1"));
        // the folder is replaced by a file, so nothing can be written in it any more
        fs::remove_dir_all(&d).unwrap();
        fs::write(&d, "in the way").unwrap();
        assert!(!s.set("a", "2"));
        assert!(!s.set("b", "x"));
        assert_eq!(s.get("a").as_deref(), Some("1"));
        assert_eq!(s.get("b"), None);
        assert!(!s.remove("a"));
        assert_eq!(s.get("a").as_deref(), Some("1"));
        let _ = fs::remove_file(&d);
    }
}
