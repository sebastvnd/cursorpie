//! Memory marks for the diagnostic log (category `mem`).
//!
//! A memory problem is only visible as a number that is too big at some moment, and by then the
//! program has moved on. This module writes the number at the moments where memory is gained or
//! given back (a press, the pie closing, Settings opening and closing, sleep, the trim thread) and
//! every `TICK` while the log is on, so a log shows *where* a footprint grew and whether it came
//! back. `cargo tools diag` reads the events and names the step that grew too much, a trim that
//! gave nothing back, and a footprint that kept rising while the program was idle.
//!
//! Everything here is **free while the log is off**: [`mark`], [`footprint`] and
//! [`start_sampler`] return at once, and in a release build without the `debug-log` feature the
//! log is compiled away altogether. The numbers are the ones the system's own monitor shows (see
//! [`platform::MemoryUse`]), in MiB with one decimal; they are never
//! tied to what the person has (no icon names, no paths).

use crate::platform;
use std::sync::Mutex;
use std::time::Duration;

const MIB: f64 = 1024.0 * 1024.0;

/// A footprint at or above this many MiB writes one `mem.high` event per run.
pub const HIGH_MB: f64 = 300.0;

/// How often the sampler thread writes a `tick` mark while the log is on.
const TICK: Duration = Duration::from_secs(30);

/// The footprint of the previous mark, for `delta_mb`.
static LAST: Mutex<Option<u64>> = Mutex::new(None);

/// Whether the memory category of the log is on.
pub fn enabled() -> bool {
    diag::enabled("mem")
}

/// Bytes as MiB with one decimal.
fn mib(bytes: u64) -> f64 {
    round1(bytes as f64 / MIB)
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// The footprint now, in bytes. None while the log is off (so a caller pays nothing for asking)
/// or when the system does not say.
pub fn footprint() -> Option<u64> {
    if !enabled() {
        return None;
    }
    platform::memory_use().map(|m| m.footprint)
}

/// How many MiB the footprint grew since `before` (negative: it fell). None if either number is
/// missing.
pub fn grew_mb(before: Option<u64>) -> Option<f64> {
    let (before, now) = (before?, footprint()?);
    Some(round1((now as i64 - before as i64) as f64 / MIB))
}

/// Writes one `mem.mark` line: the footprint, the resident size and the peak now, and how far the
/// footprint moved since the previous mark (from any thread). `stage` says what just happened.
pub fn mark(stage: &'static str) {
    if !enabled() {
        return;
    }
    let Some(m) = platform::memory_use() else {
        diag::once!("mem", "unavailable");
        return;
    };
    let before = LAST
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(m.footprint);
    let delta_mb = before.map_or(0.0, |b| {
        round1((m.footprint as i64 - b as i64) as f64 / MIB)
    });
    let footprint_mb = mib(m.footprint);
    diag::log!(
        "mem",
        "mark",
        stage,
        footprint_mb,
        resident_mb = mib(m.resident),
        peak_mb = mib(m.peak),
        delta_mb
    );
    if footprint_mb >= HIGH_MB {
        diag::once!("mem", "high", stage, footprint_mb);
    }
}

/// Writes one `mem.trim` line: what the allocator said it gave back, and the footprint before and
/// after, so a trim that frees nothing while the footprint is large can be seen.
pub fn log_trim(released: usize, before: Option<u64>, after: Option<u64>) {
    if let (Some(before), Some(after)) = (before, after) {
        diag::log!(
            "mem",
            "trim",
            released_mb = round1(released as f64 / MIB),
            before_mb = mib(before),
            after_mb = mib(after)
        );
    }
}

/// Starts the thread that writes a `tick` mark every `TICK`, so a footprint that keeps rising
/// while nothing is done shows up. Does nothing while the log is off, and starts at most once.
pub fn start_sampler() {
    static STARTED: std::sync::Once = std::sync::Once::new();
    if !enabled() {
        return;
    }
    STARTED.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("cursorpie-mem".into())
            .spawn(|| {
                while enabled() {
                    std::thread::sleep(TICK);
                    mark("tick");
                }
            });
        if spawned.is_err() {
            diag::log!("mem", "sampler_failed");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_mib_with_one_decimal() {
        assert_eq!(mib(0), 0.0);
        assert_eq!(mib(1024 * 1024), 1.0);
        assert_eq!(mib(1024 * 1024 * 3 / 2), 1.5);
        assert_eq!(round1(0.04), 0.0);
        assert_eq!(round1(-12.349), -12.3);
    }

    #[test]
    fn nothing_is_measured_while_the_log_is_off() {
        // the unit tests never switch the log on
        assert!(!enabled());
        assert_eq!(footprint(), None);
        assert_eq!(grew_mb(Some(1)), None);
        mark("test"); // returns at once
        start_sampler(); // starts nothing
        log_trim(1, Some(1), Some(1));
    }
}
