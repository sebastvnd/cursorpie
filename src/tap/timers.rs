//! One thread for every delayed job of the tap (the hold timer, the double-click timer).
//!
//! These jobs used to start a new OS thread for each press of the trigger button, from inside the
//! input hook. That is a poor place to create a thread, above all on Windows: a thread start runs
//! the attach routine of every loaded DLL (which antivirus and overlay programs hook), a hook that
//! answers too slowly is dropped by Windows, and when memory is short `std::thread::spawn` panics.
//! A press whose timer could not start was then left in `Gesture::Pending` for good, with the
//! button swallowed.
//!
//! Now one small thread, started with the tap, keeps a queue of jobs ordered by time and runs each
//! when it is due. Starting a job from the hook is a push into a channel: no allocation of a stack,
//! no system call that can wait. A job must be short (it takes a lock, sends a message, posts a
//! click); it must never sleep, because the jobs run one after the other. [`after`] says whether
//! the job was accepted, so the caller can fail open when it was not.

use std::cmp::{Ordering as Cmp, Reverse};
use std::collections::BinaryHeap;
use std::panic::AssertUnwindSafe;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// Stack of the timer thread: the jobs only lock, send and post, so a small one is plenty.
const STACK_BYTES: usize = 512 * 1024;

/// A job that runs once, on the timer thread.
type Job = Box<dyn FnOnce() + Send + 'static>;

/// A job and the time it is due. Ordered by time, then by arrival, so equal times keep their order.
struct Entry {
    at: Instant,
    seq: u64,
    job: Job,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        (self.at, self.seq) == (other.at, other.seq)
    }
}

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Cmp> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Cmp {
        (self.at, self.seq).cmp(&(other.at, other.seq))
    }
}

/// The queue's sender, or `None` when the system would not start the thread.
static SCHEDULER: OnceLock<Option<Sender<Entry>>> = OnceLock::new();
/// Arrival number of the next job.
static SEQ: AtomicU64 = AtomicU64::new(0);

fn scheduler() -> Option<&'static Sender<Entry>> {
    SCHEDULER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Entry>();
            std::thread::Builder::new()
                .name("cursorpie-timers".into())
                .stack_size(STACK_BYTES)
                .spawn(move || run(&rx))
                .ok()
                .map(|_| tx)
        })
        .as_ref()
}

/// Starts the timer thread now, so the first press does not have to (the tap calls this when it
/// starts). Harmless to call twice.
pub(crate) fn start() {
    let _ = scheduler();
}

/// Runs `job` on the timer thread after `delay`. `false` = the job was not accepted (the thread
/// could not be started, or is gone) and will never run: the caller must not wait for it.
pub(crate) fn after(delay: Duration, job: impl FnOnce() + Send + 'static) -> bool {
    let Some(tx) = scheduler() else {
        return false;
    };
    let entry = Entry {
        at: Instant::now() + delay,
        seq: SEQ.fetch_add(1, Ordering::Relaxed),
        job: Box::new(job),
    };
    tx.send(entry).is_ok()
}

/// The timer thread: runs what is due, then waits for the next due time or a new job.
fn run(rx: &mpsc::Receiver<Entry>) {
    let mut queue: BinaryHeap<Reverse<Entry>> = BinaryHeap::new();
    loop {
        let now = Instant::now();
        while queue.peek().is_some_and(|e| e.0.at <= now) {
            let Some(Reverse(due)) = queue.pop() else {
                break;
            };
            // one job that panics must not end the thread that every later press depends on
            let _ = std::panic::catch_unwind(AssertUnwindSafe(due.job));
        }
        let next = match queue.peek() {
            Some(e) => rx.recv_timeout(e.0.at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match next {
            Ok(entry) => queue.push(Reverse(entry)),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    #[test]
    fn jobs_run_in_the_order_of_their_time_not_of_their_arrival() {
        let (tx, rx) = channel();
        for (name, ms) in [("late", 120u64), ("early", 20), ("middle", 70)] {
            let tx = tx.clone();
            assert!(after(Duration::from_millis(ms), move || {
                tx.send(name).unwrap();
            }));
        }
        let got: Vec<_> = (0..3)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap())
            .collect();
        assert_eq!(got, ["early", "middle", "late"]);
    }

    #[test]
    fn a_job_does_not_run_before_it_is_due() {
        let (tx, rx) = channel();
        let started = Instant::now();
        assert!(after(Duration::from_millis(80), move || {
            tx.send(Instant::now()).unwrap();
        }));
        let ran = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(ran.duration_since(started) >= Duration::from_millis(80));
    }

    #[test]
    fn a_job_that_panics_does_not_stop_the_next_one() {
        let (tx, rx) = channel();
        assert!(after(Duration::from_millis(5), || panic!("on purpose")));
        assert!(after(Duration::from_millis(30), move || {
            tx.send(()).unwrap();
        }));
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok());
    }

    #[test]
    fn a_job_may_schedule_the_next_stage() {
        let (tx, rx) = channel();
        assert!(after(Duration::from_millis(10), move || {
            assert!(after(Duration::from_millis(10), move || {
                tx.send("second").unwrap();
            }));
        }));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "second");
    }
}
