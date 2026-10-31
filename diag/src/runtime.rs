//! The real log: the switch, the file, the global state and the guards. Compiled in debug
//! builds, and in release builds only with the feature `force` (see the crate documentation).

use crate::text::has_room;
use crate::{
    DEFAULT_KEEP_FILES, DEFAULT_MAX_FILE_BYTES, MARKER_FILE, Spec, Value, file_name, file_prefix,
    format_line, iso_utc, log_dir_from, resolve_spec, stale,
};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// The most a marker file may hold; a longer one is ignored.
const MARKER_MAX_BYTES: u64 = 1024;

// ------------------------------------------------------------------------------- the builder

/// How the log is set up. Make one with [`Builder::new`], change what differs from the defaults
/// and finish with [`Builder::init`].
///
/// | Setting | Default |
/// |---|---|
/// | switch variable | `<APP>_DEBUG` (the app name in capitals, other characters as `_`) |
/// | folder variable | `<APP>_DEBUG_DIR` |
/// | support folder | none: the marker file is not looked for, logs go to the temporary folder |
/// | files kept | [`DEFAULT_KEEP_FILES`] |
/// | size of a file | [`DEFAULT_MAX_FILE_BYTES`] |
#[derive(Clone, Debug)]
#[must_use = "call `.init()` to switch the log on"]
pub struct Builder {
    app: String,
    support_dir: Option<PathBuf>,
    switch_var: String,
    dir_var: String,
    version: Option<String>,
    report_env: Vec<String>,
    header: Vec<(String, Value)>,
    keep_files: usize,
    max_file_bytes: u64,
}

/// `cursor pie` -> `CURSOR_PIE`: the part of the environment variable names that comes from the
/// name of the app.
fn env_prefix(app: &str) -> String {
    app.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

impl Builder {
    /// A builder for the program called `app` (it names the files and the variables).
    pub fn new(app: &str) -> Self {
        let prefix = env_prefix(app);
        Builder {
            app: app.to_string(),
            support_dir: None,
            switch_var: format!("{prefix}_DEBUG"),
            dir_var: format!("{prefix}_DEBUG_DIR"),
            version: None,
            report_env: Vec::new(),
            header: Vec::new(),
            keep_files: DEFAULT_KEEP_FILES,
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
        }
    }

    /// The folder of the program's own files. The marker file [`MARKER_FILE`] is looked for in it
    /// and the logs go to its [`LOG_DIR`] folder.
    pub fn support_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.support_dir = Some(dir.into());
        self
    }

    /// Uses another environment variable than `<APP>_DEBUG` as the switch.
    pub fn switch_var(mut self, name: &str) -> Self {
        self.switch_var = name.to_string();
        self
    }

    /// Uses another environment variable than `<APP>_DEBUG_DIR` to move the log folder.
    pub fn dir_var(mut self, name: &str) -> Self {
        self.dir_var = name.to_string();
        self
    }

    /// The version of the program, written into the first line.
    pub fn version(mut self, version: &str) -> Self {
        self.version = Some(version.to_string());
        self
    }

    /// Environment variables whose values are copied into the first line as `env_<name>` (in
    /// lower case, empty when the variable is not set): the ones that decide how a session
    /// behaves.
    pub fn report_env(mut self, names: &[&str]) -> Self {
        self.report_env = names.iter().map(|n| (*n).to_string()).collect();
        self
    }

    /// One more field for the first line (the build, the backend chosen, ...).
    pub fn header(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.header.push((key.to_string(), value.into()));
        self
    }

    /// How many log files are kept (the newest ones). At least one.
    pub fn keep_files(mut self, n: usize) -> Self {
        self.keep_files = n.max(1);
        self
    }

    /// The size at which a log file stops growing.
    pub fn max_file_bytes(mut self, n: u64) -> Self {
        self.max_file_bytes = n;
        self
    }

    /// Opens the log if it is switched on and returns whether it is on. Call it once, early;
    /// calling it again does nothing. Never fails: a log that cannot be opened is reported on
    /// stderr and the program runs on.
    pub fn init(self) -> bool {
        if STATE.get().is_some() {
            return true;
        }
        let env = std::env::var(self.switch_var.as_str()).ok();
        let marker = self
            .support_dir
            .as_deref()
            .and_then(|d| read_text_capped(&d.join(MARKER_FILE), MARKER_MAX_BYTES));
        let Some(spec) = resolve_spec(env.as_deref(), marker.as_deref()) else {
            return false;
        };
        let dir_override = std::env::var(self.dir_var.as_str()).ok();
        let dir = log_dir_from(
            dir_override.as_deref(),
            self.support_dir.as_deref(),
            &self.app,
        );
        let prefix = file_prefix(&self.app);
        let sink = open_sink(&dir, &prefix, self.keep_files, self.max_file_bytes);
        if let Some(s) = &sink {
            eprintln!("{}: debug log: {}", self.app, s.path.display());
        }
        let state = State {
            spec,
            start: Instant::now(),
            sink: Mutex::new(sink),
        };
        if STATE.set(state).is_err() {
            return true; // another thread was first; its log is the one in use
        }
        ON.store(true, Ordering::SeqCst);
        install_panic_hook();
        write_header(&self);
        true
    }
}

/// The text of a small file, or `None` when it cannot be read or is longer than `max` bytes.
fn read_text_capped(path: &Path, max: u64) -> Option<String> {
    let mut text = String::new();
    let n = File::open(path)
        .ok()?
        .take(max.saturating_add(1))
        .read_to_string(&mut text)
        .ok()?;
    ((n as u64) <= max).then_some(text)
}

// ------------------------------------------------------------------------------- the global

/// The open log file and how much was written to it.
struct Sink {
    file: File,
    path: PathBuf,
    written: u64,
    max: u64,
    capped: bool,
}

impl Sink {
    /// Appends one line. A line is one `write` call, so it is on disk when this returns.
    fn push(&mut self, line: &str) {
        if self.capped {
            return;
        }
        let mut text = String::with_capacity(line.len() + 1);
        text.push_str(line);
        text.push('\n');
        let add = text.len() as u64;
        if !has_room(self.written, add, self.max) {
            self.capped = true;
            let _ = self
                .file
                .write_all(b"{\"cat\":\"diag\",\"ev\":\"size_cap\"}\n");
            return;
        }
        if self.file.write_all(text.as_bytes()).is_ok() {
            self.written += add;
        }
    }
}

struct State {
    spec: Spec,
    start: Instant,
    sink: Mutex<Option<Sink>>,
}

static STATE: OnceLock<State> = OnceLock::new();
/// `true` once the log is on: the only thing a call site reads while the log is off.
static ON: AtomicBool = AtomicBool::new(false);

/// Locks a mutex even when another thread panicked while holding it: a log must keep working
/// during a panic, which is when it is needed most.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Creates the folder, deletes the oldest files and opens the file of this run.
fn open_sink(dir: &Path, prefix: &str, keep: usize, max: u64) -> Option<Sink> {
    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!(
            "{prefix}: cannot create the debug log folder {}: {e}",
            dir.display()
        );
        return None;
    }
    let names: Vec<String> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    // keep one place free for the file about to be created
    for old in stale(prefix, &names, keep.saturating_sub(1)) {
        let _ = fs::remove_file(dir.join(old));
    }
    let path = dir.join(file_name(prefix, unix_secs(), std::process::id()));
    match File::create(&path) {
        Ok(file) => Some(Sink {
            file,
            path,
            written: 0,
            max,
            capped: false,
        }),
        Err(e) => {
            eprintln!(
                "{prefix}: cannot create the debug log {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// Whether the log is on at all (a category may still be off, see [`enabled`]).
pub fn is_on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Whether an event of category `cat` would be written right now.
pub fn enabled(cat: &str) -> bool {
    ON.load(Ordering::Relaxed) && STATE.get().is_some_and(|s| s.spec.allows(cat, false))
}

/// Like [`enabled`], for a high-volume event that needs `trace` as well.
pub fn trace_enabled(cat: &str) -> bool {
    ON.load(Ordering::Relaxed) && STATE.get().is_some_and(|s| s.spec.allows(cat, true))
}

/// The file the log is written to, while it is on and could be opened.
pub fn active_path() -> Option<PathBuf> {
    let st = STATE.get()?;
    let guard = lock(&st.sink);
    guard.as_ref().map(|s| s.path.clone())
}

/// Writes one line. It does not check the switch: the macros do, and only then evaluate their
/// arguments. Use [`enabled`] first when you call it yourself.
pub fn write(cat: &str, ev: &str, fields: &[(&str, Value)]) {
    let Some(st) = STATE.get() else { return };
    let thread = std::thread::current();
    let line = format_line(
        st.start.elapsed().as_secs_f64(),
        thread.name().unwrap_or("?"),
        cat,
        ev,
        fields,
    );
    if st.spec.mirrors_to_stderr() {
        eprintln!("{line}");
    }
    if let Some(sink) = lock(&st.sink).as_mut() {
        sink.push(&line);
    }
}

/// [`write`] for fields whose names are built at run time.
pub(crate) fn write_owned(cat: &str, ev: &str, owned: &[(String, Value)]) {
    let fields: Vec<(&str, Value)> = owned.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    write(cat, ev, &fields);
}

// ------------------------------------------------------------------- events built at run time

/// A line whose field names are decided at run time. Make it with [`event`], add fields, and end
/// with [`Event::write`]; nothing is written before that.
#[derive(Clone, Debug)]
#[must_use = "an Event is only written by `.write()`"]
pub struct Event {
    cat: String,
    ev: String,
    trace: bool,
    fields: Vec<(String, Value)>,
}

/// Starts a line of `cat` / `ev` (see [`Event`]). It allocates even while the log is off; guard
/// a hot path with [`enabled`], or use [`log!`](crate::log).
pub fn event(cat: &str, ev: &str) -> Event {
    Event {
        cat: cat.to_string(),
        ev: ev.to_string(),
        trace: false,
        fields: Vec::new(),
    }
}

impl Event {
    /// Adds a field.
    pub fn field(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.fields.push((key.into(), value.into()));
        self
    }

    /// Adds many fields at once, for example from a map.
    pub fn fields<K, V>(mut self, pairs: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
    {
        for (k, v) in pairs {
            self.fields.push((k.into(), v.into()));
        }
        self
    }

    /// Marks the event as high-volume: it is then written only when the switch has `trace`.
    pub fn trace(mut self) -> Self {
        self.trace = true;
        self
    }

    /// Writes the line, if its category is on.
    pub fn write(self) {
        let on = if self.trace {
            trace_enabled(&self.cat)
        } else {
            enabled(&self.cat)
        };
        if on {
            write_owned(&self.cat, &self.ev, &self.fields);
        }
    }
}

// ------------------------------------------------------------------------------------ timers

/// What a running [`Timer`] remembers.
#[derive(Debug)]
struct Running {
    cat: String,
    ev: String,
    start: Instant,
    fields: Vec<(String, Value)>,
}

/// A guard that writes how long it lived. Make it with [`timed!`](crate::timed) (or
/// [`Timer::start`]); the line, with the field `ms`, is written when it is dropped.
#[derive(Debug)]
#[must_use = "a Timer writes its line when it is dropped: bind it to a variable"]
pub struct Timer {
    running: Option<Running>,
}

impl Timer {
    /// A guard that does nothing (what [`timed!`](crate::timed) returns while the log is off).
    pub fn off() -> Timer {
        Timer { running: None }
    }

    /// A guard that starts now. It does not look at the switch: [`timed!`](crate::timed) does.
    pub fn start(cat: &str, ev: &str, fields: &[(&str, Value)]) -> Timer {
        Timer {
            running: Some(Running {
                cat: cat.to_string(),
                ev: ev.to_string(),
                start: Instant::now(),
                fields: fields
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.clone()))
                    .collect(),
            }),
        }
    }

    /// Whether this guard will write a line.
    pub fn is_running(&self) -> bool {
        self.running.is_some()
    }

    /// Adds a field to the line, for something only known at the end (a result, a count).
    pub fn field(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        if let Some(r) = self.running.as_mut() {
            r.fields.push((key.into(), value.into()));
        }
    }

    /// Drops the guard without writing anything.
    pub fn cancel(mut self) {
        self.running = None;
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(mut r) = self.running.take() {
            let ms = r.start.elapsed().as_secs_f64() * 1000.0;
            r.fields.push((
                "ms".to_string(),
                Value::from((ms * 1000.0).round() / 1000.0),
            ));
            write_owned(&r.cat, &r.ev, &r.fields);
        }
    }
}

// -------------------------------------------------------------------------------- snapshots

/// The first line: the program, the machine and the variables asked for with
/// [`Builder::report_env`].
fn write_header(b: &Builder) {
    let secs = unix_secs();
    let mut fields: Vec<(String, Value)> = vec![
        ("app".to_string(), Value::from(b.app.as_str())),
        (
            "version".to_string(),
            Value::from(b.version.as_deref().unwrap_or("?")),
        ),
        ("os".to_string(), Value::from(std::env::consts::OS)),
        ("arch".to_string(), Value::from(std::env::consts::ARCH)),
        ("pid".to_string(), Value::from(std::process::id())),
        ("unix".to_string(), Value::from(secs)),
        ("utc".to_string(), Value::from(iso_utc(secs))),
        (
            "debug_build".to_string(),
            Value::from(cfg!(debug_assertions)),
        ),
    ];
    for name in &b.report_env {
        let value = std::env::var(name).unwrap_or_default();
        fields.push((
            format!("env_{}", name.to_ascii_lowercase()),
            Value::from(value),
        ));
    }
    fields.extend(b.header.iter().cloned());
    write_owned("diag", "start", &fields);
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let at = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let payload = info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            String::new()
        };
        write(
            "panic",
            "panic",
            &[("at", Value::from(at)), ("message", Value::from(message))],
        );
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_names_follow_the_name_of_the_app() {
        let b = Builder::new("CursorPie");
        assert_eq!(b.switch_var, "CURSORPIE_DEBUG");
        assert_eq!(b.dir_var, "CURSORPIE_DEBUG_DIR");
        let b = Builder::new("my app").switch_var("X_LOG").keep_files(0);
        assert_eq!(b.switch_var, "X_LOG");
        assert_eq!(b.dir_var, "MY_APP_DEBUG_DIR");
        assert_eq!(b.keep_files, 1, "at least one file is kept");
    }
}
