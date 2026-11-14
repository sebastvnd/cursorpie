//! Giving freed memory back soon after the pie was used, without waiting for a frame.
//!
//! Reading an app icon is the heaviest thing a press does. On macOS one icon is a multi-size TIFF
//! of several MiB, which is copied, decoded page by page and reduced to a 128 px texture; the big
//! buffers are freed at once, but the allocator does not hand them back at once: libmalloc keeps
//! recently freed large blocks in a cache and drops them only slowly, as *other* large blocks come
//! and go. A pie with a dozen app icons therefore showed up in Activity Monitor with hundreds of
//! MiB that nothing used any more, and the number fell only when the next press allocated again.
//!
//! [`ui::sleep`](crate::ui::sleep) does give the memory back, but only after `sleep_after_s`
//! (60 s by default) and only on a frame of the window, and a hidden window is not guaranteed a
//! frame. This module is the quick, frame-free part:
//!
//! * [`note`] says "something big was freed". It is called by the icon loader, from any thread.
//! * [`set_busy`] says whether the pie, Settings or a preview is on screen. It is called by
//!   `App::sleep_step` on every frame and costs one atomic load while nothing changes.
//! * A small thread (`cursorpie-trim`) waits [`SETTLE`] after the last note, and after the UI went
//!   idle, then calls [`platform::release_unused_memory`](crate::platform::release_unused_memory).
//!   It never trims while the UI is busy, so the pie is never drawn while the allocator is walking
//!   its zones, and it never touches memory that is in use.
//!
//! The policy is [`Debt`], a plain state machine over [`Instant`]s with no threads and no OS
//! types, so it is covered by unit tests like [`Sleeper`](crate::ui::sleep::Sleeper).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, Once};
use std::time::{Duration, Instant};

/// How long after the last [`note`] (and after the UI went idle) the pages are handed back. Long
/// enough that a burst of icon loads is trimmed once, short enough that the number in Activity
/// Monitor is back down a moment after the pie closed.
pub const SETTLE: Duration = Duration::from_secs(2);

/// What the trim thread should do next, see [`Debt::poll`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    /// Nothing is owed, or the UI is busy: sleep until something changes.
    Idle,
    /// Something is owed, but not yet: look again after this long.
    Wait(Duration),
    /// Hand the pages back now. Reported once per debt.
    Trim,
}

/// Whether memory is owed back to the OS, and from when. See the module docs.
#[derive(Debug)]
pub struct Debt {
    /// When the pages are due. None = nothing is owed.
    due: Option<Instant>,
    /// Whether the pie, Settings or a preview is on screen.
    busy: bool,
}

impl Default for Debt {
    fn default() -> Self {
        Self::new()
    }
}

impl Debt {
    /// Nothing owed, the UI idle.
    pub const fn new() -> Self {
        Self {
            due: None,
            busy: false,
        }
    }

    /// Something big was freed at `now`: the pages are due [`SETTLE`] after the last such moment.
    pub fn note(&mut self, now: Instant) {
        self.due = Some(now + SETTLE);
    }

    /// The UI became busy or idle at `now`. When it goes idle with something owed, the wait starts
    /// again from this moment: the pie that just closed was drawing until now.
    pub fn set_busy(&mut self, busy: bool, now: Instant) {
        if self.busy && !busy && self.due.is_some() {
            self.due = Some(now + SETTLE);
        }
        self.busy = busy;
    }

    /// What to do at `now`. [`Poll::Trim`] is returned once and clears the debt.
    pub fn poll(&mut self, now: Instant) -> Poll {
        match self.due {
            None => Poll::Idle,
            Some(_) if self.busy => Poll::Idle,
            Some(due) if now >= due => {
                self.due = None;
                Poll::Trim
            }
            Some(due) => Poll::Wait(due.saturating_duration_since(now)),
        }
    }
}

/// The state the trim thread shares with everything that calls [`note`] and [`set_busy`].
struct Shared {
    debt: Mutex<Debt>,
    wake: Condvar,
}

static SHARED: Shared = Shared {
    debt: Mutex::new(Debt::new()),
    wake: Condvar::new(),
};

/// Starts the thread on the first [`note`].
static THREAD: Once = Once::new();

/// The last value given to [`set_busy`], so that a frame that changes nothing takes no lock.
static BUSY: AtomicBool = AtomicBool::new(false);

/// The lock, recovered if a panic left it poisoned: the state is two plain values.
fn lock() -> MutexGuard<'static, Debt> {
    SHARED.debt.lock().unwrap_or_else(|e| e.into_inner())
}

/// Says that something big was freed (a decoded icon, a TIFF): the pages are handed back
/// [`SETTLE`] after the last call, once the UI is idle. Cheap, and safe to call from any thread.
pub fn note() {
    THREAD.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("cursorpie-trim".into())
            .spawn(run);
        if spawned.is_err() {
            diag::log!("ui", "trim_spawn_failed");
        }
    });
    lock().note(Instant::now());
    SHARED.wake.notify_one();
}

/// Says whether something is on screen (see the module docs). Call it on every frame: a call that
/// repeats the last value does nothing.
pub fn set_busy(busy: bool) {
    if BUSY.swap(busy, Ordering::Relaxed) == busy {
        return;
    }
    lock().set_busy(busy, Instant::now());
    SHARED.wake.notify_one();
}

/// The loop of the trim thread: waits for a debt to come due, then trims without holding the lock.
fn run() {
    let mut debt = lock();
    loop {
        match debt.poll(Instant::now()) {
            Poll::Idle => {
                debt = SHARED.wake.wait(debt).unwrap_or_else(|e| e.into_inner());
            }
            Poll::Wait(wait) => {
                debt = SHARED
                    .wake
                    .wait_timeout(debt, wait)
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            Poll::Trim => {
                drop(debt);
                let before = crate::ui::memlog::footprint();
                let released = crate::platform::release_unused_memory();
                diag::log!("ui", "trim", released = released);
                crate::ui::memlog::log_trim(released, before, crate::ui::memlog::footprint());
                debt = lock();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(t: Instant, ms: u64) -> Instant {
        t + Duration::from_millis(ms)
    }

    #[test]
    fn nothing_owed_means_nothing_to_do() {
        let t0 = Instant::now();
        let mut d = Debt::new();
        assert_eq!(d.poll(t0), Poll::Idle);
        assert_eq!(d.poll(after(t0, 100_000)), Poll::Idle);
    }

    #[test]
    fn the_pages_are_due_one_settle_after_the_note_and_only_once() {
        let t0 = Instant::now();
        let mut d = Debt::new();
        d.note(t0);
        assert_eq!(
            d.poll(after(t0, 500)),
            Poll::Wait(SETTLE - Duration::from_millis(500))
        );
        assert_eq!(d.poll(t0 + SETTLE), Poll::Trim);
        assert_eq!(
            d.poll(after(t0, 10_000)),
            Poll::Idle,
            "paid: nothing is owed now"
        );
    }

    #[test]
    fn every_note_pushes_the_due_time_back() {
        let t0 = Instant::now();
        let mut d = Debt::new();
        d.note(t0);
        d.note(after(t0, 1_500)); // another icon was loaded
        assert_eq!(
            d.poll(t0 + SETTLE),
            Poll::Wait(Duration::from_millis(1_500)),
            "the burst is trimmed once, after its last icon"
        );
        assert_eq!(d.poll(after(t0, 1_500) + SETTLE), Poll::Trim);
    }

    #[test]
    fn nothing_is_trimmed_while_the_ui_is_busy() {
        let t0 = Instant::now();
        let mut d = Debt::new();
        d.set_busy(true, t0);
        d.note(after(t0, 10));
        assert_eq!(
            d.poll(after(t0, 60_000)),
            Poll::Idle,
            "the pie is on screen"
        );
    }

    #[test]
    fn the_wait_starts_again_when_the_ui_goes_idle() {
        let t0 = Instant::now();
        let mut d = Debt::new();
        d.note(t0); // the press loaded the icons ...
        d.set_busy(true, after(t0, 150)); // ... and the pie opened
        let closed = after(t0, 5_000);
        d.set_busy(false, closed); // the pie closed long after the note was due
        assert_eq!(
            d.poll(closed),
            Poll::Wait(SETTLE),
            "the pie was drawing until now: wait a settle, then trim"
        );
        assert_eq!(d.poll(closed + SETTLE), Poll::Trim);
    }

    #[test]
    fn going_idle_with_nothing_owed_owes_nothing() {
        let t0 = Instant::now();
        let mut d = Debt::new();
        d.set_busy(true, t0);
        d.set_busy(false, after(t0, 400));
        assert_eq!(d.poll(after(t0, 100_000)), Poll::Idle);
    }

    #[test]
    fn a_busy_report_that_repeats_changes_nothing() {
        let t0 = Instant::now();
        let mut d = Debt::new();
        d.note(t0);
        d.set_busy(false, after(t0, 1_000)); // already idle: not a transition
        assert_eq!(
            d.poll(after(t0, 1_000)),
            Poll::Wait(SETTLE - Duration::from_millis(1_000))
        );
    }
}
