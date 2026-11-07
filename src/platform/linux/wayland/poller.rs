//! The Linux-only half of `adapter.rs`: starting the adapter of this session, and the thread that
//! reads a compositor that is asked over a socket (Hyprland, Sway). Declared once, under a `cfg`,
//! in `mod.rs`, like `bus.rs` and `logind.rs`.

use super::adapter::{Adapter, current};
use super::{bus, hyprland_io, kwin, sway_io};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// What a polled adapter does. `start_poller` calls these from its own thread, so none of them
/// may block for long: a socket is opened with a timeout and closed again at once.
pub(super) trait Source: Send + 'static {
    /// Reads everything (windows, outputs, pointer) and publishes it to `kwin`. `false` if the
    /// compositor could not be reached.
    fn sync(&mut self) -> bool;
    /// Reads the pointer only and publishes it (called every few milliseconds during a hold).
    fn pointer(&mut self);
    /// Runs one queued command other than `arm` and `disarm` (`activate:<id>`).
    fn command(&mut self, command: &str);
    /// Whether `pointer` is worth calling during a hold.
    fn polls_pointer(&self) -> bool;
}

/// How often everything (windows, outputs, pointer) is read while nothing asks.
const IDLE_STEP: Duration = Duration::from_millis(1000);
/// How often the pointer alone is read while nothing asks, by an adapter that can (the KWin script
/// reports it at most every 100 ms too). The mouse layer follows the pointer by the motion it
/// reads and takes this answer as the correction, so a stale answer means a pie that opens off.
const IDLE_POINTER_STEP: Duration = Duration::from_millis(100);
/// How often the pointer is read during a hold (a little under 60 times a second).
const POINTER_STEP: Duration = Duration::from_millis(16);
/// A hold never keeps the pointer polling longer than this: if CursorPie's own release was lost,
/// the polling still stops.
const ARM_LIMIT: Duration = Duration::from_secs(30);

static SYNC: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

/// The loop of a polled adapter. It ends when the sender is dropped (never, in practice).
fn run<S: Source>(name: &'static str, mut source: S, rx: Receiver<()>) {
    let mut armed: Option<Instant> = None;
    let mut failing = false;
    let mut last_full = Instant::now();
    loop {
        if armed.is_some_and(|t| t.elapsed() > ARM_LIMIT) {
            armed = None;
        }
        let step = match (source.polls_pointer(), armed.is_some()) {
            (true, true) => POINTER_STEP,
            (true, false) => IDLE_POINTER_STEP,
            (false, _) => IDLE_STEP,
        };
        let woken = match rx.recv_timeout(step) {
            Ok(()) => {
                while rx.try_recv().is_ok() {} // one read serves every request waiting
                true
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        let commands = kwin::take_commands();
        for c in commands.split(';').filter(|c| !c.is_empty()) {
            match c {
                "arm" => armed = Some(Instant::now()),
                "disarm" => armed = None,
                other => source.command(other),
            }
        }
        // between two full reads (a request, or every IDLE_STEP) the pointer alone is read
        let full = woken || !source.polls_pointer() || last_full.elapsed() >= IDLE_STEP;
        if !full {
            source.pointer();
            continue;
        }
        last_full = Instant::now();
        if source.sync() {
            failing = false;
            kwin::set_loaded(true);
        } else {
            kwin::set_loaded(false);
            if !failing {
                failing = true;
                diag::log!("wayland", "adapter_unreachable", adapter = name);
            }
        }
    }
}

/// Starts the thread of a polled adapter.
fn start_poller<S: Source>(name: &'static str, source: S) {
    let (tx, rx) = channel();
    let _ = SYNC.set(Mutex::new(tx));
    let spawned = std::thread::Builder::new()
        .name(format!("cursorpie-{name}"))
        .spawn(move || run(name, source, rx));
    if spawned.is_err() {
        eprintln!("CursorPie: could not start the {name} adapter thread");
    }
}

/// Starts the adapter of this session (once). Does nothing for a compositor without one.
pub(crate) fn start() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let adapter = current();
    kwin::set_knows_pointer(adapter.knows_pointer());
    match adapter {
        Adapter::Kwin | Adapter::Gnome => bus::start(adapter),
        Adapter::Hyprland => start_poller("hyprland", hyprland_io::Hypr),
        Adapter::Sway => start_poller("sway", sway_io::Sway),
        Adapter::Off => {}
    }
}

/// Asks the adapter for a fresh answer now. Never waits.
pub(crate) fn request_sync() {
    match current() {
        Adapter::Kwin | Adapter::Gnome => bus::request_sync(),
        Adapter::Hyprland | Adapter::Sway => {
            if let Some(tx) = SYNC.get() {
                let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(());
            }
        }
        Adapter::Off => {}
    }
}
