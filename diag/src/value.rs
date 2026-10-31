//! The values a log line can hold, and how they are written as JSON.
//!
//! The log is flat on purpose: a field is a number, a text, a boolean or `null`, never a list or
//! an object, so `jq`, a spreadsheet and `cargo tools diag` read every line the same way. Anything
//! else is turned into a text first (see the `?` form of the macros, which uses `Debug`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// One value of a log field. Build it with `Value::from(..)` (the macros do that for you).
///
/// | From | Written as |
/// |---|---|
/// | `i8` ... `i64`, `isize`, `u8` ... `u64`, `usize` | a whole number |
/// | `f32`, `f64` | a number with a fraction (`812.0`); `NaN` and infinity become `null` |
/// | `bool` | `true` / `false` |
/// | `&str`, `String`, `char` | a text |
/// | `&Path`, `PathBuf` | a text (the path as the OS shows it) |
/// | `Duration` | milliseconds, with a fraction (name the field `ms`) |
/// | `Option<T>` | the value, or `null` for `None` |
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`: no value (a `None`, or a number JSON cannot hold).
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A whole number below zero or above it.
    Int(i64),
    /// A whole number too big for [`Value::Int`].
    UInt(u64),
    /// A number with a fraction.
    Float(f64),
    /// A text.
    Str(String),
}

impl Value {
    /// Appends this value as JSON to `out`.
    pub fn write_json(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(n) => {
                let _ = write!(out, "{n}");
            }
            Value::UInt(n) => {
                let _ = write!(out, "{n}");
            }
            // `{:?}` keeps the `.0` of a whole float, so a reader can tell 812.0 from 812
            Value::Float(f) if f.is_finite() => {
                let _ = write!(out, "{f:?}");
            }
            Value::Float(_) => out.push_str("null"),
            Value::Str(s) => write_json_str(s, out),
        }
    }
}

/// Appends `s` as a JSON string (with the quotes) to `out`.
pub(crate) fn write_json_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

macro_rules! from_signed {
    ($($t:ty),*) => {
        $(impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Value::Int(i64::from(v))
            }
        })*
    };
}

macro_rules! from_unsigned {
    ($($t:ty),*) => {
        $(impl From<$t> for Value {
            fn from(v: $t) -> Self {
                Value::UInt(u64::from(v))
            }
        })*
    };
}

from_signed!(i8, i16, i32);
from_unsigned!(u8, u16, u32);

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::UInt(v)
    }
}

impl From<isize> for Value {
    fn from(v: isize) -> Self {
        Value::Int(i64::try_from(v).unwrap_or(i64::MAX))
    }
}

impl From<usize> for Value {
    fn from(v: usize) -> Self {
        Value::UInt(u64::try_from(v).unwrap_or(u64::MAX))
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<f32> for Value {
    /// Goes through the shortest text of the `f32`, so `0.1_f32` is logged as `0.1`, not as
    /// `0.10000000149011612`.
    fn from(v: f32) -> Self {
        Value::Float(
            format!("{v}")
                .parse::<f64>()
                .unwrap_or_else(|_| f64::from(v)),
        )
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_owned())
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Str(v)
    }
}

impl From<&String> for Value {
    fn from(v: &String) -> Self {
        Value::Str(v.clone())
    }
}

impl From<char> for Value {
    fn from(v: char) -> Self {
        Value::Str(v.to_string())
    }
}

impl From<&Path> for Value {
    fn from(v: &Path) -> Self {
        Value::Str(v.display().to_string())
    }
}

impl From<PathBuf> for Value {
    fn from(v: PathBuf) -> Self {
        Value::Str(v.display().to_string())
    }
}

impl From<Duration> for Value {
    /// Milliseconds, with a fraction.
    fn from(v: Duration) -> Self {
        Value::Float(v.as_secs_f64() * 1000.0)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::Null, Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(v: impl Into<Value>) -> String {
        let mut out = String::new();
        v.into().write_json(&mut out);
        out
    }

    #[test]
    fn numbers_keep_their_kind() {
        assert_eq!(json(7_i32), "7");
        assert_eq!(json(-5_i64), "-5");
        assert_eq!(json(u64::MAX), "18446744073709551615");
        assert_eq!(json(3_usize), "3");
        assert_eq!(json(812.0_f64), "812.0", "a whole float keeps its fraction");
        assert_eq!(json(812.5_f64), "812.5");
        assert_eq!(
            json(0.1_f32),
            "0.1",
            "an f32 is not widened to 0.10000000149011612"
        );
    }

    #[test]
    fn what_json_cannot_hold_becomes_null() {
        assert_eq!(json(f64::NAN), "null");
        assert_eq!(json(f64::INFINITY), "null");
        assert_eq!(json(f32::NEG_INFINITY), "null");
        assert_eq!(json(None::<i32>), "null");
        assert_eq!(json(Some(3_u8)), "3");
    }

    #[test]
    fn texts_are_escaped() {
        assert_eq!(json("plain"), "\"plain\"");
        assert_eq!(json("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(json("one\ntwo\ttab\r"), "\"one\\ntwo\\ttab\\r\"");
        assert_eq!(json("\u{1}"), "\"\\u0001\"");
        assert_eq!(
            json("héllo ✓"),
            "\"héllo ✓\"",
            "other characters are written as they are"
        );
        assert_eq!(json('x'), "\"x\"");
    }

    #[test]
    fn booleans_paths_and_durations() {
        assert_eq!(json(true), "true");
        assert_eq!(json(false), "false");
        assert_eq!(json(Path::new("/tmp/a b")), "\"/tmp/a b\"");
        assert_eq!(json(Duration::from_millis(1500)), "1500.0");
    }
}
