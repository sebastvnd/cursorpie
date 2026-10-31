//! [`Watch`]: compare where something was asked to be with where it really is.
//!
//! The classic use is a window: the app asks the window system for a position, and a moment
//! later the toolkit reports the position the window really has. The two numbers differ when the
//! window manager ignored the request or moved the window, and that difference is exactly what a
//! developer needs to see. A `Watch` is the small piece that does the bookkeeping: remember the
//! request, then log the answer for a short time, only when it changes.

use crate::runtime::{lock, write_owned};
use crate::{Value, enabled};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// What was asked for, and how much of the watching is used up.
struct Pending {
    expected: Vec<(String, f64)>,
    at: Instant,
    logged: u32,
    last: Option<Vec<f64>>,
}

/// Remembers a request ([`expect`](Watch::expect)) and logs what really happened
/// ([`observe`](Watch::observe)) for a short time afterwards.
///
/// A `Watch` is made to live in a `static`:
///
/// ```ignore
/// static WINDOW: diag::Watch = diag::Watch::new("win", "win_obs", 2000, 12);
///
/// // where the position is chosen
/// WINDOW.expect(&[("x", 650.0), ("y", 350.0)]);
///
/// // every frame, while the watch is active
/// if WINDOW.active() {
///     WINDOW.observe(Some(&[("x", ox), ("y", oy), ("w", ow), ("h", oh)]), &[]);
/// }
/// ```
///
/// Every observation is one line of the category and event given to [`Watch::new`] with these
/// fields: `age_ms` (milliseconds since the request), `e<name>` for each expected value,
/// `o<name>` for each observed one, `d<name>` (observed minus expected) for the names present in
/// both, and `missing: true` when the toolkit knew nothing yet. The example above writes `ex`,
/// `ey`, `ox`, `oy`, `ow`, `oh`, `dx` and `dy`, which is what `cargo tools diag` reads.
///
/// Nothing is watched while the category is off, so the whole thing costs one atomic read per
/// frame ([`Watch::active`]).
pub struct Watch {
    cat: &'static str,
    ev: &'static str,
    window_ms: u64,
    max_logs: u32,
    active: AtomicBool,
    state: Mutex<Option<Pending>>,
}

impl Watch {
    /// A watch that writes `cat` / `ev` lines, for `window_ms` milliseconds after each request
    /// and at most `max_logs` lines per request.
    pub const fn new(cat: &'static str, ev: &'static str, window_ms: u64, max_logs: u32) -> Watch {
        Watch {
            cat,
            ev,
            window_ms,
            max_logs,
            active: AtomicBool::new(false),
            state: Mutex::new(None),
        }
    }

    /// Says what was just asked for (named numbers, in any unit, the same names as in
    /// [`observe`](Watch::observe)). Starts the watching; a second request replaces the first.
    /// Does nothing while the category is off.
    pub fn expect(&self, expected: &[(&str, f64)]) {
        if !enabled(self.cat) {
            return;
        }
        *lock(&self.state) = Some(Pending {
            expected: expected
                .iter()
                .map(|(k, v)| ((*k).to_string(), *v))
                .collect(),
            at: Instant::now(),
            logged: 0,
            last: None,
        });
        self.active.store(true, Ordering::SeqCst);
    }

    /// Whether [`observe`](Watch::observe) has work to do (one atomic read, for a per-frame call).
    pub fn active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Logs what really is (`None` when it is not known yet), if it changed since the last line.
    /// `extra` are more fields for the line. The watching ends by itself when the time or the
    /// number of lines given to [`Watch::new`] is used up.
    pub fn observe(&self, actual: Option<&[(&str, f64)]>, extra: &[(&str, Value)]) {
        let (expected, age) = {
            let mut guard = lock(&self.state);
            let Some(p) = guard.as_mut() else {
                self.active.store(false, Ordering::SeqCst);
                return;
            };
            let age = p.at.elapsed().as_millis();
            if age > u128::from(self.window_ms) || p.logged >= self.max_logs {
                *guard = None;
                self.active.store(false, Ordering::SeqCst);
                return;
            }
            let now: Option<Vec<f64>> = actual.map(|a| a.iter().map(|(_, v)| *v).collect());
            if p.logged > 0 && p.last == now {
                return;
            }
            p.logged += 1;
            p.last = now;
            (p.expected.clone(), age)
        };
        let fields = fields_for(&expected, actual, age, extra);
        write_owned(self.cat, self.ev, &fields);
    }
}

/// The fields of one observation (see [`Watch`]).
fn fields_for(
    expected: &[(String, f64)],
    actual: Option<&[(&str, f64)]>,
    age_ms: u128,
    extra: &[(&str, Value)],
) -> Vec<(String, Value)> {
    let age = u64::try_from(age_ms).unwrap_or(u64::MAX);
    let mut fields = vec![("age_ms".to_string(), Value::from(age))];
    for (k, v) in expected {
        fields.push((format!("e{k}"), Value::from(*v)));
    }
    match actual {
        Some(actual) => {
            for (k, v) in actual {
                fields.push((format!("o{k}"), Value::from(*v)));
            }
            for (k, v) in actual {
                if let Some((_, e)) = expected.iter().find(|(name, _)| name.as_str() == *k) {
                    fields.push((format!("d{k}"), Value::from(*v - *e)));
                }
            }
        }
        None => fields.push(("missing".to_string(), Value::from(true))),
    }
    for (k, v) in extra {
        fields.push(((*k).to_string(), v.clone()));
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(fields: &[(String, Value)]) -> Vec<&str> {
        fields.iter().map(|(k, _)| k.as_str()).collect()
    }

    #[test]
    fn an_observation_has_the_names_the_analyzer_reads() {
        let expected = vec![("x".to_string(), 650.0), ("y".to_string(), 350.0)];
        let actual = [("x", 810.0), ("y", 390.0), ("w", 300.0), ("h", 300.0)];
        let fields = fields_for(&expected, Some(&actual), 12, &[("ppp", Value::from(1.0))]);
        assert_eq!(
            keys(&fields),
            [
                "age_ms", "ex", "ey", "ox", "oy", "ow", "oh", "dx", "dy", "ppp"
            ]
        );
        assert_eq!(fields[0].1, Value::UInt(12));
        assert_eq!(fields[7].1, Value::Float(160.0), "dx = 810 - 650");
        assert_eq!(fields[8].1, Value::Float(40.0), "dy = 390 - 350");
    }

    #[test]
    fn an_unknown_answer_is_marked_and_has_no_numbers() {
        let expected = vec![("x".to_string(), 1.0)];
        let fields = fields_for(&expected, None, 5, &[]);
        assert_eq!(keys(&fields), ["age_ms", "ex", "missing"]);
    }

    #[test]
    fn a_value_that_was_not_asked_for_has_no_difference() {
        let expected = vec![("x".to_string(), 1.0)];
        let fields = fields_for(&expected, Some(&[("x", 3.0), ("z", 9.0)]), 0, &[]);
        assert_eq!(keys(&fields), ["age_ms", "ex", "ox", "oz", "dx"]);
    }

    #[test]
    fn a_watch_that_was_never_asked_does_nothing() {
        static IDLE: Watch = Watch::new("win", "win_obs", 2000, 12);
        assert!(!IDLE.active());
        IDLE.observe(None, &[]); // no request: nothing to compare, nothing written
        assert!(!IDLE.active());
        IDLE.expect(&[("x", 1.0)]); // the log is off in unit tests, so this is ignored
        assert!(!IDLE.active());
    }
}
