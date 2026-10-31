//! The log of a release build: nothing. Every function of the real log (`runtime.rs`) exists here
//! with the same signature and does nothing, and `enabled` is a constant `false`, so a log line
//! (`if enabled(..) { write(..) }`) is dead code that the compiler removes together with its
//! strings and arguments. Callers need no `cfg` of their own.

use crate::Value;
use std::path::PathBuf;

/// Does nothing: the log is not part of this build.
#[derive(Clone, Debug)]
#[must_use = "call `.init()` to switch the log on"]
pub struct Builder;

impl Builder {
    /// Does nothing.
    #[inline(always)]
    pub fn new(_app: &str) -> Self {
        Builder
    }

    /// Does nothing.
    #[inline(always)]
    pub fn support_dir(self, _dir: impl Into<PathBuf>) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn switch_var(self, _name: &str) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn dir_var(self, _name: &str) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn version(self, _version: &str) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn report_env(self, _names: &[&str]) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn header(self, _key: &str, _value: impl Into<Value>) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn keep_files(self, _n: usize) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn max_file_bytes(self, _n: u64) -> Self {
        self
    }

    /// The log is never on in this build: always `false`.
    #[inline(always)]
    pub fn init(self) -> bool {
        false
    }
}

/// Always `false`.
#[inline(always)]
pub const fn is_on() -> bool {
    false
}

/// Always `false`.
#[inline(always)]
pub const fn enabled(_cat: &str) -> bool {
    false
}

/// Always `false`.
#[inline(always)]
pub const fn trace_enabled(_cat: &str) -> bool {
    false
}

/// Always `None`.
#[inline(always)]
pub fn active_path() -> Option<PathBuf> {
    None
}

/// Does nothing.
#[inline(always)]
pub fn write(_cat: &str, _ev: &str, _fields: &[(&str, Value)]) {}

/// Does nothing: a line that is never written.
#[derive(Clone, Debug)]
#[must_use = "an Event is only written by `.write()`"]
pub struct Event;

/// Starts a line that is never written.
#[inline(always)]
pub fn event(_cat: &str, _ev: &str) -> Event {
    Event
}

impl Event {
    /// Does nothing.
    #[inline(always)]
    pub fn field(self, _key: impl Into<String>, _value: impl Into<Value>) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn fields<K, V>(self, _pairs: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
    {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn trace(self) -> Self {
        self
    }

    /// Does nothing.
    #[inline(always)]
    pub fn write(self) {}
}

/// Does nothing: a guard that never writes.
#[derive(Debug)]
#[must_use = "a Timer writes its line when it is dropped: bind it to a variable"]
pub struct Timer;

impl Timer {
    /// A guard that does nothing.
    #[inline(always)]
    pub fn off() -> Timer {
        Timer
    }

    /// A guard that does nothing.
    #[inline(always)]
    pub fn start(_cat: &str, _ev: &str, _fields: &[(&str, Value)]) -> Timer {
        Timer
    }

    /// Always `false`.
    #[inline(always)]
    pub fn is_running(&self) -> bool {
        false
    }

    /// Does nothing.
    #[inline(always)]
    pub fn field(&mut self, _key: impl Into<String>, _value: impl Into<Value>) {}

    /// Does nothing.
    #[inline(always)]
    pub fn cancel(self) {}
}

/// Does nothing: nothing is watched in this build.
pub struct Watch;

impl Watch {
    /// A watch that never watches.
    pub const fn new(
        _cat: &'static str,
        _ev: &'static str,
        _window_ms: u64,
        _max_logs: u32,
    ) -> Watch {
        Watch
    }

    /// Does nothing.
    #[inline(always)]
    pub fn expect(&self, _expected: &[(&str, f64)]) {}

    /// Always `false`.
    #[inline(always)]
    pub fn active(&self) -> bool {
        false
    }

    /// Does nothing.
    #[inline(always)]
    pub fn observe(&self, _actual: Option<&[(&str, f64)]>, _extra: &[(&str, Value)]) {}
}
