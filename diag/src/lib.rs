#![cfg_attr(not(any(debug_assertions, feature = "force")), allow(dead_code))]

//! # diag: the diagnostic log
//!
//! A log for the problems that show up on one machine only ("the pie opens in the middle of the
//! screen on this desktop") and cannot be found with a debugger. It answers *what did each layer
//! think was happening, and what did the system really do?* without editing the code again.
//!
//! - **Off by default.** While it is off a call site costs one atomic read and its arguments are
//!   never evaluated, so a log line may call something expensive.
//! - **Easy to write.** One macro, any number of `name = value` fields, no formatting by hand.
//! - **Flat JSON lines.** One object per line, written at once (the file is complete after a
//!   crash), so `jq`, a spreadsheet and `cargo tools diag` all read it.
//! - **Separate from the app.** This is its own crate of the workspace (`diag/`), with no
//!   dependencies, and nothing in it knows what CursorPie is. The app configures it in one place
//!   (`src/debug.rs`) and calls the macros from anywhere.
//!
//! ## Debug builds only
//!
//! The log is part of **debug builds** (`cargo run`, `cargo test`). A **release build has none of
//! it**: the real log is not compiled, `enabled` is a constant `false`, and every log line is dead
//! code that the compiler removes with its strings and arguments, so nothing needs a `cfg` at the
//! call sites. `CURSORPIE_DEBUG=1` does nothing in such a binary.
//!
//! To find a problem on someone else's machine, build a release with the log kept in it by the
//! feature `force` of this crate (the app calls it `debug-log`):
//!
//! ```text
//! cargo build --release --features debug-log
//! ```
//!
//! ## Writing a log line
//!
//!
//! ```ignore
//! diag::log!("ui", "show_pie", x = x, y = y, profile = profile);
//! ```
//!
//! The first two arguments are the **category** (the layer: `tap`, `ui`, `x11`, `win`, `sys`) and
//! the **event** (what happened, in `snake_case`). The rest are fields. Every form below can be
//! mixed in one call:
//!
//! | Form | Example | Written as |
//! |---|---|---|
//! | `name = value` | `x = pos.x` | `"x":812.0` |
//! | `name` | `profile` | the variable's own name: `"profile":2` |
//! | `name = ?value` | `kind = ?kind` | the `Debug` text of any value: `"kind":"Down"` |
//!
//! A value is anything [`Value`] can be made from: numbers, `bool`, `&str`, `String`, `char`,
//! paths, `Duration` and `Option` of those. Nesting is not supported on purpose: wrap a list or a
//! struct with `?` and it becomes one text.
//!
//! | Macro | Use it for |
//! |---|---|
//! | [`log!`](crate::log) | an event that happens a few times per gesture |
//! | [`trace!`](crate::trace) | an event that comes in the hundreds (a pointer move); written only when the switch has `trace` |
//! | [`once!`](crate::once) | "this branch ran": written the first time this call site is reached, never again |
//! | [`timed!`](crate::timed) | how long a block took: logs `ms` when the returned guard is dropped |
//!
//! ```ignore
//! diag::trace!("tap", "drag", x = pos.x, y = pos.y);
//! diag::once!("x11", "no_xinput2");
//! {
//!     let _t = diag::timed!("ui", "load_icons", count = icons.len());
//!     load(&icons);
//! } // writes {"cat":"ui","ev":"load_icons","count":12,"ms":3.412}
//! ```
//!
//! ### When the field names are not known at compile time
//!
//! Use the builder: [`event`] gives an [`Event`] that takes any number of `field(name, value)`
//! calls, with names built at run time, and is written by `.write()` (and only when the category
//! is on).
//!
//! ```ignore
//! let mut e = diag::event("sys", "startup").field("monitors", rects.len());
//! for (i, r) in rects.iter().enumerate() {
//!     e = e.field(format!("mon{i}"), diag::rect_str(r.x, r.y, r.w, r.h));
//! }
//! e.write();
//! ```
//!
//! Guard anything expensive that is not an argument of a macro with [`enabled`]:
//! `if diag::enabled("sys") { ... }`.
//!
//! ### Comparing a request with what really happened
//!
//! A [`Watch`] remembers what was asked for (a window position, say) and logs the real value for
//! a short time afterwards, with the difference. See its documentation.
//!
//! ## Starting it
//!
//! Call [`Builder`] once, early, in the program that owns the log:
//!
//! ```ignore
//! diag::Builder::new("cursorpie")
//!     .support_dir(config::app_support_dir()) // marker file and `logs` folder live here
//!     .version(env!("CARGO_PKG_VERSION"))
//!     .report_env(&["XDG_SESSION_TYPE", "DISPLAY"]) // copied into the first line
//!     .init();
//! ```
//!
//! `init` does nothing (and returns `false`) unless the switch is on, and never fails: a log that
//! cannot be opened is reported on stderr and the program runs on. Calling it again does nothing.
//!
//! ## Turning it on
//!
//! For an app called `cursorpie` (the names follow the name given to [`Builder::new`]):
//!
//! | Way | Example |
//! |---|---|
//! | environment variable | `CURSORPIE_DEBUG=1 cursorpie` |
//! | marker file, for a copy that starts at login | a file named `debug` in the support folder, empty or holding the same list |
//!
//! The value is a list of words (see [`Spec::parse`]): `1` or `all` for every category, `tap,ui`
//! for some, `all,-tap` for all but one, `trace` to add the [`trace!`](crate::trace) events,
//! `stderr` to print every line on the terminal too, `0` or `off` to turn it off. A variable that
//! is set wins over the file, so `CURSORPIE_DEBUG=0` silences a `debug` file for one run.
//!
//! ## Where it goes
//!
//! One file per run, `cursorpie-<unix seconds>-<pid>.jsonl`, in the `logs` folder of the support
//! folder, or in the folder named by `CURSORPIE_DEBUG_DIR`. Without a support folder the system's
//! temporary folder is used. The path is printed on stderr when the file is opened. Only the
//! newest [`DEFAULT_KEEP_FILES`] files are kept (see [`Builder::keep_files`]) and one file stops
//! growing at [`DEFAULT_MAX_FILE_BYTES`] (see [`Builder::max_file_bytes`]). `cargo tools
//! clean-logs` deletes them by hand.
//!
//! ## The line
//!
//! ```text
//! {"t":12.345,"th":"main","cat":"ui","ev":"show_pie","x":812.0,"y":344.0}
//! ```
//!
//! `t` is seconds since the log was opened, `th` the name of the thread, `cat` the category and
//! `ev` the event; then the fields in the order they were written. A field called `t`, `th`,
//! `cat` or `ev` is renamed with a leading underscore. The first line of a file is
//! `diag` / `start`: the app, its version, the OS, the process and the environment variables
//! given to [`Builder::report_env`]. A panic is logged too (`panic` / `panic`, with its place).
//!
//! ## Conventions
//!
//! - Name the category after the **layer**, the event after **what happened**, in `snake_case`.
//! - Log the *input* and the *decision* of each layer next to each other, so a reader can see
//!   where two numbers that should agree stop agreeing.
//! - Never log the user's data: file names, typed text, the content of the configuration.
//! - An argument must not have a side effect: it is evaluated only while the log is on, so the
//!   program would behave differently with the log on.
//! - A new check for `cargo tools diag` is a branch in `analyze` (`tools/src/diag.rs`) and a test
//!   with a few sample lines.
//!
//! ## Testing code that logs
//!
//! The pure parts of this crate ([`Spec`], [`format_line`], [`file_name`], ...) are plain
//! functions, and a log that is off writes nothing, so ordinary unit tests need no set-up. A test
//! that wants to read the file back can switch the log on with a marker file in a temporary
//! folder (see `diag/tests/end_to_end.rs`); the log is global to the process, so it belongs in
//! an integration test of its own.

mod spec;
mod text;
mod value;

// Debug builds have the log. A release build has none of it: `runtime` is then a set of empty
// stand-ins (`off.rs`) whose `enabled` is always false, so every log line is dead code the
// compiler removes. The feature `force` brings the real log back into a release build.
#[cfg(any(debug_assertions, feature = "force"))]
mod runtime;
#[cfg(not(any(debug_assertions, feature = "force")))]
#[path = "off.rs"]
mod runtime;
#[cfg(any(debug_assertions, feature = "force"))]
mod watch;

#[cfg(not(any(debug_assertions, feature = "force")))]
pub use runtime::Watch;
pub use runtime::{
    Builder, Event, Timer, active_path, enabled, event, is_on, trace_enabled, write,
};
pub use spec::{Spec, resolve_spec};
pub use text::{
    file_name, file_prefix, format_line, is_log_name, iso_utc, log_dir_from, rect_str, stale,
};
pub use value::Value;
#[cfg(any(debug_assertions, feature = "force"))]
pub use watch::Watch;

/// The name of the marker file in the support folder that switches the log on.
pub const MARKER_FILE: &str = "debug";
/// The log folder inside the support folder.
pub const LOG_DIR: &str = "logs";
/// How many log files are kept by default (the newest ones).
pub const DEFAULT_KEEP_FILES: usize = 10;
/// The size at which a log file stops growing, by default.
pub const DEFAULT_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

// ---------------------------------------------------------------------------------- macros

/// Turns the fields of a log macro into an array of `(name, Value)` pairs. Not for direct use.
#[doc(hidden)]
#[macro_export]
macro_rules! __fields {
    (@done [$($acc:tt)*]) => {
        [$($acc)*]
    };
    (@acc [$($acc:tt)*]) => {
        $crate::__fields!(@done [$($acc)*])
    };
    (@acc [$($acc:tt)*] $k:ident = ? $v:expr $(, $($rest:tt)*)?) => {
        $crate::__fields!(
            @acc
            [$($acc)* (stringify!($k), $crate::Value::from(::std::format!("{:?}", $v))),]
            $($($rest)*)?
        )
    };
    (@acc [$($acc:tt)*] $k:ident = $v:expr $(, $($rest:tt)*)?) => {
        $crate::__fields!(
            @acc
            [$($acc)* (stringify!($k), $crate::Value::from($v)),]
            $($($rest)*)?
        )
    };
    (@acc [$($acc:tt)*] $k:ident $(, $($rest:tt)*)?) => {
        $crate::__fields!(
            @acc
            [$($acc)* (stringify!($k), $crate::Value::from($k)),]
            $($($rest)*)?
        )
    };
    ($($t:tt)*) => {
        $crate::__fields!(@acc [] $($t)*)
    };
}

/// Writes a line of `cat` / `ev` with the given fields. The fields are evaluated only while the
/// log is on and `cat` is allowed.
///
/// ```ignore
/// diag::log!("ui", "show_pie", x = x, y = y, profile, kind = ?kind);
/// ```
///
/// Each field is `name = value`, a bare `name` (the variable of that name), or `name = ?value`
/// (the `Debug` text of the value).
#[macro_export]
macro_rules! log {
    ($cat:expr, $ev:expr $(, $($fields:tt)*)?) => {
        if $crate::enabled($cat) {
            $crate::write($cat, $ev, &$crate::__fields!($($($fields)*)?));
        }
    };
}

/// Like [`log!`](crate::log), for events that come in the hundreds (written only when the switch
/// has `trace`).
#[macro_export]
macro_rules! trace {
    ($cat:expr, $ev:expr $(, $($fields:tt)*)?) => {
        if $crate::trace_enabled($cat) {
            $crate::write($cat, $ev, &$crate::__fields!($($($fields)*)?));
        }
    };
}

/// Like [`log!`](crate::log), but written only the first time this call site is reached: for
/// "this branch ran" in code that runs again and again.
#[macro_export]
macro_rules! once {
    ($cat:expr, $ev:expr $(, $($fields:tt)*)?) => {{
        static SEEN: ::std::sync::atomic::AtomicBool = ::std::sync::atomic::AtomicBool::new(false);
        if $crate::enabled($cat) && !SEEN.swap(true, ::std::sync::atomic::Ordering::Relaxed) {
            $crate::write($cat, $ev, &$crate::__fields!($($($fields)*)?));
        }
    }};
}

/// Starts a [`Timer`]: bind it to a variable, and when it is dropped a line of `cat` / `ev` is
/// written with the given fields and `ms`, the time since this macro ran. While the log is off
/// the guard does nothing and the fields are not evaluated.
///
/// ```ignore
/// let _t = diag::timed!("ui", "load_icons", count = icons.len());
/// ```
#[macro_export]
macro_rules! timed {
    ($cat:expr, $ev:expr $(, $($fields:tt)*)?) => {
        if $crate::enabled($cat) {
            $crate::Timer::start($cat, $ev, &$crate::__fields!($($($fields)*)?))
        } else {
            $crate::Timer::off()
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_take_all_three_forms() {
        let (b, d) = (2_u8, vec![1, 2]);
        let f = crate::__fields!(a = 1.5, b, c = ?d, e = "x",);
        assert_eq!(f.len(), 4);
        assert_eq!(f[0], ("a", Value::Float(1.5)));
        assert_eq!(f[1], ("b", Value::UInt(2)));
        assert_eq!(f[2], ("c", Value::Str("[1, 2]".to_string())));
        assert_eq!(f[3], ("e", Value::Str("x".to_string())));
    }

    #[test]
    fn a_log_that_is_off_costs_nothing_and_writes_nothing() {
        // the unit tests never switch the global log on
        assert!(!is_on());
        assert!(!enabled("ui"));
        assert!(!trace_enabled("ui"));
        assert!(active_path().is_none());
        let side_effect = std::cell::Cell::new(0);
        crate::log!("ui", "never", x = 1.0, n = side_effect.replace(1));
        crate::trace!("ui", "never");
        crate::once!("ui", "never", x = 1);
        assert_eq!(
            side_effect.get(),
            0,
            "an argument is not evaluated while the log is off"
        );
        let mut timer = crate::timed!("ui", "never", x = 1);
        assert!(!timer.is_running());
        timer.field("y", 2);
        drop(timer);
        event("ui", "never")
            .field("x", 1)
            .fields([("y", 2)])
            .trace()
            .write();
    }

    #[test]
    fn a_timer_can_be_cancelled() {
        let timer = Timer::start("ui", "never", &[("x", Value::from(1))]);
        assert!(timer.is_running());
        timer.cancel(); // writes nothing, and the log is off anyway
    }
}
