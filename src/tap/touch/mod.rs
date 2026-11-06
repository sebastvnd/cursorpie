//! Trackpad touches: a second source of the same gesture (macOS and Windows).
//!
//! The state machine in [`trigger`] already turns "button down, held, moved,
//! released" into "the pie opens, a slice is hovered, a slice runs". A trackpad hold is the same
//! story with different words, so this module translates touches into the calls a mouse press, drag
//! and release make. The pie, the profiles, the app detection and the actions do not change, and
//! with the trackpad trigger off (the default) nothing here runs. The design, and what is still
//! to be confirmed on a real Mac and a real Windows laptop, is in `docs/TRACKPAD_HAPTICS_PLAN.md`.
//!
//! | Part | What it does | Where it is tested |
//! |---|---|---|
//! | [`recognizer`] | frames of finger positions -> `HoldArmed`, `Move`, `ExtraClick`, `Release`, `Cancel` (pure) | every OS |
//! | [`Bridge`] | the events -> `trigger::touch_press`, `touch_move`, `touch_release`, `touch_cancel` | every OS |
//! | [`extra`] | the extra click: one more finger clicks while the pie is open | every OS |
//! | [`haptic`] | which moments tick and how often (pure), `Msg::Haptic` | every OS |
//! | `multitouch` | macOS: the contact frames of the system, through the private `MultitouchSupport.framework` | by hand on a Mac (checklist) |
//! | `ptp` | Windows: what the HID reports of a Precision Touchpad mean (layout, hybrid reports, palms; pure) | every OS |
//! | `precision` | Windows: the reports of the touchpad, through Raw Input (`WM_INPUT`) and the `HidP_*` parser | by hand on Windows (checklist) |
//! | `evdev` | Linux: the fingers of a touchpad, read from `/dev/input/event*` (the parser is pure) | the parser everywhere, the reader by hand on a Linux laptop |
//!
//! **Mouse and trackpad together.** One gesture runs at a time, owned by whoever started it
//! ([`Source`](super::gesture::Source)): a trackpad hold does not start while a mouse gesture runs
//! (the first to arm wins), the mouse trigger passes through while a trackpad gesture runs, and the
//! mouse right-click still switches the pie because it acts on the shared pie, not on the source.
//! The hold time, the double-click window, the profiles and the app detection are the settings both share.
//!
//! **Privacy.** Positions of fingers stay in memory for the frame being processed and are never
//! written to the log or to disk; the `touch` category of the diagnostic log has counts and
//! decisions only.

#[cfg(any(target_os = "linux", test))]
#[cfg_attr(
    not(all(target_os = "linux", not(feature = "check-windows"))),
    allow(dead_code)
)]
mod evdev;
pub(crate) mod extra;
pub(crate) mod haptic;
#[cfg(any(target_os = "macos", test))]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod multitouch;
// Windows: the pure half is tested everywhere; the Win32 half is also compiled with
// `--features check-windows` so it can be type-checked on other OSes.
#[cfg(any(windows, feature = "check-windows"))]
#[cfg_attr(not(windows), allow(dead_code))]
mod precision;
#[cfg(any(windows, feature = "check-windows", test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod ptp;
pub(crate) mod recognizer;
pub(crate) mod scroll;

use super::gesture::Gesture;
use super::trigger;
use crate::config::cfg;
use crate::geometry::Point;
use crate::msg::Notifier;
use recognizer::{Frame, Params, Recognizer, TouchEvent};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A trackpad gesture started by this source is running (armed, open or in Settings). Read by the
/// event tap with one atomic load, see [`extra`].
pub(crate) static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Fingers on the trackpad that owns the gesture (palms and edges left out).
pub(crate) static FINGERS: AtomicUsize = AtomicUsize::new(0);
/// Time of the last key press, in ms on the touch clock, plus 1 (0 = never): the typing guard.
static LAST_KEY_MS: AtomicU64 = AtomicU64::new(0);

static EPOCH: OnceLock<Instant> = OnceLock::new();

/// Which pointer watcher is the current one (see [`watch_pointer`]): a watcher of an older
/// gesture sees another number and stops.
#[cfg(not(test))]
static WATCH_GEN: AtomicU64 = AtomicU64::new(0);
/// How often the watcher looks at the pointer while a trackpad hold runs.
#[cfg(not(test))]
const WATCH_EVERY: Duration = Duration::from_millis(10);

/// While a trackpad hold runs, a mouse that moves the pointer must aim the pie as well as the
/// finger does. A mouse press has its drag events for that; a trackpad hold has none, and the
/// finger reports nothing when it keeps still, so the pointer is looked at here, on its own
/// thread, until the gesture ends. Reading the pointer costs a few microseconds; nothing is added
/// to the event taps (`MouseMoved` is deliberately not listened to).
fn watch_pointer(state: &Arc<Mutex<Gesture>>, notifier: &Notifier) {
    #[cfg(not(test))]
    {
        let id = WATCH_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        let (state, notifier) = (Arc::clone(state), notifier.clone());
        let spawned = std::thread::Builder::new()
            .name("touch-pointer".into())
            .spawn(move || {
                let mut last = crate::platform::pointer_pos();
                while ACTIVE.load(Ordering::SeqCst) && WATCH_GEN.load(Ordering::SeqCst) == id {
                    std::thread::sleep(WATCH_EVERY);
                    let now = crate::platform::pointer_pos();
                    if let Some((x, y)) = now
                        && now != last
                    {
                        trigger::touch_move(&state, &notifier, Point::new(x, y));
                    }
                    last = now;
                }
            });
        if spawned.is_err() {
            diag::log!("touch", "pointer_watch_failed");
        }
    }
    #[cfg(test)]
    let _ = (state, notifier);
}

/// Whether the scroll wheel (and a touchpad's scrolling) is swallowed right now: while a trackpad
/// hold runs, and for the tail of a scroll that the pie began to swallow (see [`scroll`]). The finger
/// that makes the extra click is a second finger on the surface, and the system scrolls with two
/// fingers: that scroll, and the glide it leaves behind, must not reach the app under the pie. Two
/// atomic reads while nothing runs, asked for every wheel event.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub(crate) fn swallows_scroll() -> bool {
    scroll::swallows()
}

/// Time on the clock of this module: frames, key presses and the haptic limiter all use it.
pub(crate) fn now() -> Duration {
    EPOCH.get_or_init(Instant::now).elapsed()
}

/// A key was pressed (called by the event tap for every key press: one atomic store).
pub(crate) fn note_key() {
    LAST_KEY_MS.store(now().as_millis() as u64 + 1, Ordering::Relaxed);
}

/// What Settings says about the touch source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The trackpad trigger is off (or has not started yet): nothing is loaded.
    Off,
    /// macOS: the private framework could not be loaded, or its data failed the self-check.
    /// Windows: Raw Input refused the registration. The mouse keeps working, the trackpad does not.
    Unavailable,
    /// The source runs but found no trackpad (a Bluetooth trackpad that connects later is found
    /// within a few seconds). On Windows only a *precision* touchpad counts.
    NoDevice,
    /// The source runs and watches `devices` trackpads; `built_in` says whether one is the built-in one.
    Ready { devices: usize, built_in: bool },
    /// Linux: `devices` touchpads exist but `/dev/input/event*` may not be read by this user (the
    /// `input` group).
    NoPermission { devices: usize },
}

static STATUS: Mutex<Status> = Mutex::new(Status::Off);

/// The state of the touch source, for the Trackpad panel of Settings.
pub fn status() -> Status {
    *crate::config::lock(&STATUS)
}

#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
pub(crate) fn set_status(s: Status) {
    *crate::config::lock(&STATUS) = s;
}

/// The recognizer settings the config asks for.
pub(crate) fn params_from_config() -> Params {
    let c = cfg();
    Params {
        still_mm: c.touch_still_mm,
        hold: Duration::from_millis(c.pie_hold_ms),
        typing_guard: Duration::from_millis(c.touch_typing_guard_ms),
        double_click: Duration::from_millis(c.double_click_ms),
    }
}

/// What the bridge needs to reach the rest of the program.
pub(crate) struct Env<'a> {
    pub state: &'a Arc<Mutex<Gesture>>,
    pub notifier: &'a Notifier,
    /// Where the pointer is (a touch hold moves the system's pointer; the position is read when it is needed).
    pub cursor: &'a dyn Fn() -> Option<Point>,
}

/// Translates what the recognizer reports into the state machine's calls.
pub(crate) struct Bridge {
    rec: Recognizer,
    events: Vec<TouchEvent>,
    /// The gesture that is running was started by this source.
    ours: bool,
    /// The pie of that gesture opened (a hold that never opened leaves no scroll tail, see [`scroll`]).
    opened: bool,
}

impl Bridge {
    pub(crate) fn new() -> Self {
        Self {
            rec: Recognizer::new(params_from_config()),
            events: Vec::new(),
            ours: false,
            opened: false,
        }
    }

    /// One frame of a device.
    pub(crate) fn on_frame(&mut self, env: &Env<'_>, frame: &Frame<'_>) {
        if self.rec.idle() {
            // settings changes apply between touches
            let p = params_from_config();
            if p != self.rec.params() {
                self.rec.set_params(p);
            }
        }
        let key = LAST_KEY_MS.load(Ordering::Relaxed);
        if key != 0 {
            self.rec.note_key(Duration::from_millis(key - 1));
        }
        let mut events = std::mem::take(&mut self.events);
        events.clear();
        self.rec.frame(frame, &mut events);
        let fingers = self.rec.count();
        FINGERS.store(fingers, Ordering::SeqCst);
        self.apply(env, &events);
        // a finger that lands after a pie closed is a scroll of the person's own: the tail ends
        scroll::on_fingers(fingers);
        self.events = events;
    }

    /// Call a few times a second: a device that went quiet in the middle of a gesture cancels it.
    pub(crate) fn poll(&mut self, env: &Env<'_>) {
        let mut events = std::mem::take(&mut self.events);
        events.clear();
        self.rec.poll(now(), &mut events);
        self.apply(env, &events);
        self.events = events;
    }

    /// A device went away. Only the Linux and Windows sources report this; macOS never does.
    #[cfg_attr(
        not(any(target_os = "linux", windows, feature = "check-windows", test)),
        allow(dead_code)
    )]
    pub(crate) fn device_lost(&mut self, env: &Env<'_>, device: u64) {
        let mut events = std::mem::take(&mut self.events);
        events.clear();
        self.rec.device_lost(device, &mut events);
        FINGERS.store(self.rec.count(), Ordering::SeqCst);
        self.apply(env, &events);
        self.events = events;
    }

    /// The source stops: a running gesture is cancelled and everything is forgotten.
    pub(crate) fn reset(&mut self, env: &Env<'_>) {
        let mut events = std::mem::take(&mut self.events);
        events.clear();
        self.rec.reset(&mut events);
        self.apply(env, &events);
        self.events = events;
        self.ours = false;
        self.opened = false;
        ACTIVE.store(false, Ordering::SeqCst);
        FINGERS.store(0, Ordering::SeqCst);
        extra::clear();
        scroll::clear();
    }

    fn apply(&mut self, env: &Env<'_>, events: &[TouchEvent]) {
        for &ev in events {
            match ev {
                TouchEvent::HoldArmed { elapsed, double } => {
                    let started = match (env.cursor)() {
                        Some(p) => {
                            trigger::touch_press(env.state, env.notifier, p, elapsed, double)
                        }
                        None => false,
                    };
                    self.ours = started;
                    self.opened = false;
                    if started {
                        // the wheel is ours from now on; a tail of the last pie ends here
                        scroll::on_next_gesture();
                    }
                    ACTIVE.store(started, Ordering::SeqCst);
                    if started {
                        watch_pointer(env.state, env.notifier);
                    }
                    diag::log!(
                        "touch",
                        "arm",
                        elapsed_ms = elapsed,
                        double = double,
                        started = started
                    );
                }
                TouchEvent::HoldOpen => {
                    self.opened = self.ours;
                    diag::log!("touch", "open", ours = self.ours);
                }
                TouchEvent::Move => {
                    if self.ours
                        && let Some(p) = (env.cursor)()
                    {
                        trigger::touch_move(env.state, env.notifier, p);
                    }
                }
                TouchEvent::ExtraClick => {
                    if self.ours {
                        extra::tap(env.state, env.notifier);
                    }
                }
                TouchEvent::Release => {
                    if self.ours {
                        trigger::touch_release(env.state, env.notifier, (env.cursor)());
                    }
                    diag::log!("touch", "release", ours = self.ours);
                    self.end_gesture(true);
                }
                TouchEvent::Cancel(why) => {
                    if self.ours {
                        trigger::touch_cancel(env.state, env.notifier);
                    }
                    diag::log!("touch", "cancel", why = why.name(), ours = self.ours);
                    // a device that went quiet or away leaves no scroll behind; too many fingers does
                    self.end_gesture(matches!(why, recognizer::Why::TooManyFingers));
                }
            }
        }
    }

    /// The gesture is over (a release or a cancel). The wheel stops being swallowed at once only
    /// when nothing was scrolled away: after a pie that opened, the scroll its fingers started is
    /// swallowed to its end ([`scroll`]).
    fn end_gesture(&mut self, scrolling_may_follow: bool) {
        let tail = self.ours && self.opened && scrolling_may_follow;
        // the tail is armed before the flag drops, so no wheel event falls between the two
        if tail {
            scroll::begin_tail(FINGERS.load(Ordering::SeqCst));
        }
        ACTIVE.store(false, Ordering::SeqCst);
        if !tail {
            scroll::end_without_tail();
        }
        self.ours = false;
        self.opened = false;
    }
}

/// A [`Bridge`] with what it needs to reach the rest of the program, shared by the threads of a
/// native source (one reader per device on Linux, the framework's callback thread on macOS).
#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", test)),
    allow(dead_code)
)]
pub(crate) struct Sink {
    bridge: Mutex<Bridge>,
    state: Arc<Mutex<Gesture>>,
    notifier: Notifier,
    /// Where the pointer is. The system's, except in tests.
    cursor: fn() -> Option<Point>,
}

/// Where the system's pointer is.
fn system_cursor() -> Option<Point> {
    crate::platform::pointer_pos().map(|(x, y)| Point::new(x, y))
}

#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", test)),
    allow(dead_code)
)]
impl Sink {
    pub(crate) fn new(state: Arc<Mutex<Gesture>>, notifier: Notifier) -> Self {
        Self {
            bridge: Mutex::new(Bridge::new()),
            state,
            notifier,
            cursor: system_cursor,
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut Bridge, &Env<'_>) -> R) -> R {
        let cursor = || (self.cursor)();
        let env = Env {
            state: &self.state,
            notifier: &self.notifier,
            cursor: &cursor,
        };
        f(&mut crate::config::lock(&self.bridge), &env)
    }

    /// One frame of `device` (`size_mm` = its surface, `(0, 0)` when unknown), stamped now.
    pub(crate) fn feed(&self, device: u64, size_mm: (f32, f32), fingers: &[recognizer::Finger]) {
        self.with(|b, env| {
            b.on_frame(
                env,
                &Frame {
                    device,
                    time: now(),
                    size_mm,
                    fingers,
                },
            )
        });
    }

    /// Call a few times a second: a device that went quiet in the middle of a gesture cancels it.
    pub(crate) fn poll(&self) {
        self.with(|b, env| b.poll(env));
    }

    /// A device went away. Only the Linux source reports this; macOS never does.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub(crate) fn device_lost(&self, device: u64) {
        self.with(|b, env| b.device_lost(env, device));
    }

    /// The source stops.
    pub(crate) fn reset(&self) {
        self.with(|b, env| b.reset(env));
    }
}

/// The sink of the virtual trackpad (the floating one of the Tools tab): a source of frames like
/// the native ones, fed by the UI instead of by a device. It shares the gesture of the tap, so a
/// hold on it opens the pie, a drag aims it and a release runs the slice, exactly as on a real
/// trackpad. Set once, when the tap starts.
#[cfg(feature = "vtrackpad")]
static VIRTUAL: OnceLock<Sink> = OnceLock::new();

/// The device id of the virtual trackpad on the recognizer's frames ("VIRT").
#[cfg(feature = "vtrackpad")]
const VIRTUAL_DEVICE: u64 = 0x5649_5254;

/// Called by the native tap right after it creates the gesture, so the UI can feed it.
#[cfg(feature = "vtrackpad")]
pub(crate) fn register_virtual(state: Arc<Mutex<Gesture>>, notifier: Notifier) {
    let _ = VIRTUAL.set(Sink::new(state, notifier));
}

/// Whether the tap has started, i.e. whether the virtual trackpad has a gesture to feed.
#[cfg(feature = "vtrackpad")]
pub(crate) fn virtual_ready() -> bool {
    VIRTUAL.get().is_some()
}

/// One frame of the virtual trackpad. No fingers = all lifted. False when the tap has not started.
#[cfg(feature = "vtrackpad")]
pub(crate) fn virtual_feed(size_mm: (f32, f32), fingers: &[recognizer::Finger]) -> bool {
    VIRTUAL.get().is_some_and(|s| {
        s.feed(VIRTUAL_DEVICE, size_mm, fingers);
        true
    })
}

/// Time passes while a finger rests on the virtual trackpad (a resting finger sends no frames, and
/// the hold time is counted here).
#[cfg(feature = "vtrackpad")]
pub(crate) fn virtual_poll() {
    if let Some(s) = VIRTUAL.get() {
        s.poll();
    }
}

/// The virtual trackpad went away (closed, or Settings opened): a running gesture is cancelled.
#[cfg(feature = "vtrackpad")]
pub(crate) fn virtual_reset() {
    if let Some(s) = VIRTUAL.get() {
        s.reset();
    }
}

/// Starts the touch source on its own thread. It idles until the setting `touch_trigger` is on:
/// only then is anything loaded (macOS: the private framework) or opened (Linux: `/dev/input`).
#[cfg(target_os = "macos")]
pub(crate) fn start(state: Arc<Mutex<Gesture>>, notifier: Notifier) {
    multitouch::spawn(state, notifier);
}

/// Starts the touch source on its own thread (Windows). It idles until the setting `touch_trigger`
/// is on: only then is anything registered with Windows.
#[cfg(all(any(windows, feature = "check-windows"), not(target_os = "macos")))]
pub(crate) fn start(state: Arc<Mutex<Gesture>>, notifier: Notifier) {
    precision::spawn(state, notifier);
}

/// The `--touchpad-debug` command (see `evdev::debug`).
#[cfg(target_os = "linux")]
pub(crate) fn touchpad_debug_from_args() -> bool {
    evdev::debug::run_from_args()
}

/// Starts the touch source on its own thread (Linux, see [`evdev`]). With `check-windows` the
/// Windows source stands in, so both backends can be type-checked on one machine.
#[cfg(all(target_os = "linux", not(feature = "check-windows")))]
pub(crate) fn start(state: Arc<Mutex<Gesture>>, notifier: Notifier) {
    evdev::spawn(state, notifier);
}

#[cfg(test)]
mod tests {
    use super::recognizer::Finger;
    use super::*;
    use crate::config::Config;
    use crate::msg::Msg;
    use crate::tap::gesture::Source;
    use crate::tap::{Ev, Kind, Verdict, dispatch};
    use std::cell::Cell;
    use std::sync::MutexGuard;
    use std::sync::mpsc;

    /// These tests share the live config and the statics of this module (and of `tap`): one at a
    /// time, behind the same lock as the tests of `tap`.
    use crate::tap::TEST_SERIAL as SERIAL;

    /// Sets the live config for a test and puts it back afterwards.
    struct Cfg {
        _serial: MutexGuard<'static, ()>,
        saved: Config,
    }

    impl Cfg {
        fn set(edit: impl FnOnce(&mut Config)) -> Self {
            let serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            let mut c = cfg();
            let saved = c.clone();
            edit(&mut c);
            crate::tap::PAUSED.store(false, Ordering::SeqCst);
            crate::tap::SETTINGS_OPEN.store(false, Ordering::SeqCst);
            ACTIVE.store(false, Ordering::SeqCst);
            FINGERS.store(0, Ordering::SeqCst);
            extra::clear();
            scroll::clear();
            crate::tap::pie_clicks::clear();
            Self {
                _serial: serial,
                saved,
            }
        }
    }

    impl Drop for Cfg {
        fn drop(&mut self) {
            *cfg() = self.saved.clone();
            ACTIVE.store(false, Ordering::SeqCst);
            FINGERS.store(0, Ordering::SeqCst);
            extra::clear();
            scroll::clear();
            crate::tap::pie_clicks::clear();
        }
    }

    fn notifier() -> (Notifier, mpsc::Receiver<Msg>) {
        let (tx, rx) = mpsc::channel();
        (
            Notifier {
                tx,
                ctx: eframe::egui::Context::default(),
            },
            rx,
        )
    }

    fn fast(c: &mut Config) {
        c.pie_hold_ms = 150; // short, but the hold timer really sleeps
        c.settings_hold_ms = 30_000;
        c.touch_trigger = true;
        c.touch_typing_guard_ms = 0;
        c.app_gesture = crate::config::AppGesture::Merged;
        c.haptics = true;
        c.haptic_hover = true;
    }

    /// The messages that are not haptic ticks, as comparable text, until the channel is quiet.
    fn drain(rx: &mpsc::Receiver<Msg>, wait: Duration) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(m) = rx.recv_timeout(wait) {
            let s = match m {
                Msg::Warm { .. } => "warm".to_string(),
                Msg::ShowPie { x, y, path, .. } => format!("show {x} {y} {}", path.len()),
                Msg::Hover(h) => format!("hover {h:?}"),
                Msg::HidePie => "hide".to_string(),
                Msg::Haptic(k) => format!("haptic {k:?}"),
                _ => "other".to_string(),
            };
            out.push(s);
        }
        out
    }

    struct Rig {
        state: Arc<Mutex<Gesture>>,
        n: Notifier,
        rx: mpsc::Receiver<Msg>,
        bridge: Bridge,
        pointer: Cell<Point>,
        t: u64,
    }

    impl Rig {
        fn new() -> Self {
            let (n, rx) = notifier();
            Self {
                state: Arc::new(Mutex::new(Gesture::Idle)),
                n,
                rx,
                bridge: Bridge::new(),
                pointer: Cell::new(Point::new(500.0, 500.0)),
                t: 1000,
            }
        }

        /// `ms` of frames with these fingers, 10 ms apart, on the touch clock of the recognizer
        /// (it is driven by the frame times, not by the wall clock).
        fn frames(&mut self, ms: u64, fingers: &[Finger]) {
            let cursor = || Some(self.pointer.get());
            let env = Env {
                state: &self.state,
                notifier: &self.n,
                cursor: &cursor,
            };
            for _ in 0..ms / 10 {
                self.t += 10;
                self.bridge.on_frame(
                    &env,
                    &Frame {
                        device: 1,
                        time: Duration::from_millis(self.t),
                        size_mm: (100.0, 100.0),
                        fingers,
                    },
                );
            }
        }
    }

    fn finger(x: f32) -> Finger {
        Finger {
            id: 1,
            x,
            y: 0.5,
            size: 1.0,
        }
    }

    #[test]
    fn a_trackpad_hold_and_a_mouse_hold_send_the_same_messages() {
        let _c = Cfg::set(fast);

        // the mouse: press, wait for the hold, move to a slice, back to the center, release
        let (n, rx) = notifier();
        let st = Arc::new(Mutex::new(Gesture::Idle));
        let ev = |kind, x: f64| Ev::Mouse {
            kind,
            button: 2,
            pos: Point::new(x, 500.0),
        };
        assert_eq!(
            dispatch(&st, &n, ev(Kind::Down, 500.0), &|| {}),
            Verdict::Drop
        );
        std::thread::sleep(Duration::from_millis(400));
        dispatch(&st, &n, ev(Kind::Drag, 600.0), &|| {});
        dispatch(&st, &n, ev(Kind::Drag, 500.0), &|| {});
        dispatch(&st, &n, ev(Kind::Up, 500.0), &|| {});
        let mouse = drain(&rx, Duration::from_millis(100));

        // the trackpad: the same pointer path
        let mut rig = Rig::new();
        rig.frames(600, &[finger(0.5)]);
        // The hold timer opens the pie on its own thread, after a time that depends on the machine.
        // Wait until it is open, then past the rate limit of the haptic ticks: a hover that came
        // within `MIN_GAP` of the tick of the opening would be silent, and the test would flicker.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !matches!(*crate::config::lock(&rig.state), Gesture::ActivePie { .. })
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(haptic::MIN_GAP * 3);
        rig.pointer.set(Point::new(600.0, 500.0));
        rig.frames(30, &[finger(0.55)]);
        rig.pointer.set(Point::new(500.0, 500.0));
        rig.frames(30, &[finger(0.5)]);
        rig.frames(20, &[]);
        let touch = drain(&rig.rx, Duration::from_millis(100));

        assert!(mouse.len() >= 5, "{mouse:?}");
        let quiet = |v: &[String]| -> Vec<String> {
            v.iter()
                .filter(|s| !s.starts_with("haptic"))
                .cloned()
                .collect()
        };
        assert_eq!(
            quiet(&touch),
            quiet(&mouse),
            "touch {touch:?}\nmouse {mouse:?}"
        );
        // only the trackpad ticks: when the pie opened and when the slice changed
        assert!(touch.iter().any(|s| s == "haptic LevelChange"), "{touch:?}");
        assert!(touch.iter().any(|s| s == "haptic Alignment"), "{touch:?}");
        assert!(!mouse.iter().any(|s| s.starts_with("haptic")), "{mouse:?}");
        assert!(matches!(*crate::config::lock(&rig.state), Gesture::Idle));
    }

    fn second(x: f32) -> Finger {
        Finger {
            id: 2,
            x,
            y: 0.5,
            size: 1.0,
        }
    }

    #[cfg(feature = "vtrackpad")]
    /// A sink whose pointer stays put (there is no display to ask in a test).
    fn test_sink(state: &Arc<Mutex<Gesture>>, n: Notifier) -> Sink {
        let mut sink = Sink::new(Arc::clone(state), n);
        sink.cursor = || Some(Point::new(500.0, 500.0));
        sink
    }

    #[cfg(feature = "vtrackpad")]
    /// Holds one finger still on a virtual trackpad sink for `ms`, in real time (the sink stamps
    /// its frames with the clock, like the window does).
    fn hold_virtual(sink: &Sink, ms: u64) {
        let f = [finger(0.5)];
        for _ in 0..ms / 10 {
            sink.feed(VIRTUAL_DEVICE, (160.0, 100.0), &f);
            std::thread::sleep(Duration::from_millis(10));
            sink.poll();
        }
    }

    #[test]
    fn the_scroll_of_the_fingers_is_swallowed_to_its_end_after_the_pie_closes() {
        let _c = Cfg::set(fast);
        let mut rig = Rig::new();
        rig.frames(200, &[finger(0.5)]); // the pie is open
        assert!(swallows_scroll(), "while the hold runs");
        rig.frames(40, &[finger(0.5), second(0.7)]); // the extra finger lands (two fingers scroll)
        assert!(swallows_scroll());
        // both lift: the hold is over, but the glide the system makes of the scroll is not
        rig.frames(20, &[]);
        assert!(!ACTIVE.load(Ordering::SeqCst));
        assert!(scroll::tail_open());
        assert!(
            swallows_scroll(),
            "the glide must not reach the app under the pie"
        );
        // nothing more comes: the tail runs out and the scroll wheel is the person's again
        std::thread::sleep(Duration::from_millis(
            scroll::TAIL_MS + scroll::TAIL_GAP_MS + 80,
        ));
        assert!(!swallows_scroll());
        assert!(!scroll::tail_open());
    }

    #[test]
    fn a_scroll_the_person_starts_after_the_pie_closed_is_never_swallowed() {
        let _c = Cfg::set(fast);
        let mut rig = Rig::new();
        rig.frames(200, &[finger(0.5)]);
        rig.frames(20, &[]);
        assert!(swallows_scroll(), "the tail is open");
        // two fingers land on a surface that was empty: the person scrolls on purpose
        rig.frames(20, &[finger(0.4), second(0.6)]);
        assert!(!scroll::tail_open());
        assert!(!swallows_scroll());
    }

    #[test]
    fn a_hold_that_never_opened_leaves_the_scrolling_alone() {
        let _c = Cfg::set(fast);
        let mut rig = Rig::new();
        rig.frames(120, &[finger(0.5)]); // armed, not open
        assert!(swallows_scroll());
        rig.frames(20, &[]); // lifted: a tap, not a pie
        assert!(!scroll::tail_open());
        assert!(
            !swallows_scroll(),
            "nothing was scrolled away, nothing is swallowed"
        );
        // a second finger before the pie opened is the start of a scroll: it passes at once
        rig.frames(120, &[finger(0.5)]);
        rig.frames(20, &[finger(0.5), second(0.7)]);
        assert!(!swallows_scroll());
    }

    #[test]
    #[cfg(feature = "vtrackpad")]
    fn a_virtual_trackpad_hold_opens_the_pie_like_a_real_one() {
        let _c = Cfg::set(fast);
        let (n, rx) = notifier();
        let state = Arc::new(Mutex::new(Gesture::Idle));
        let sink = test_sink(&state, n);

        hold_virtual(&sink, 450);
        let opened = drain(&rx, Duration::from_millis(60));
        assert!(opened.iter().any(|s| s.starts_with("show")), "{opened:?}");
        assert!(
            opened.iter().any(|s| s == "haptic LevelChange"),
            "{opened:?}"
        );
        assert!(matches!(
            *crate::config::lock(&state),
            Gesture::ActivePie { .. }
        ));

        // lifting the finger closes it
        sink.feed(VIRTUAL_DEVICE, (160.0, 100.0), &[]);
        let closed = drain(&rx, Duration::from_millis(60));
        assert!(closed.iter().any(|s| s == "hide"), "{closed:?}");
        assert!(matches!(*crate::config::lock(&state), Gesture::Idle));
    }

    #[test]
    #[cfg(feature = "vtrackpad")]
    fn a_virtual_trackpad_hold_opens_nothing_while_settings_is_open() {
        let _c = Cfg::set(fast);
        crate::tap::SETTINGS_OPEN.store(true, Ordering::SeqCst);
        let (n, rx) = notifier();
        let state = Arc::new(Mutex::new(Gesture::Idle));
        let sink = test_sink(&state, n);

        hold_virtual(&sink, 450);
        sink.feed(VIRTUAL_DEVICE, (160.0, 100.0), &[]);
        let seen = drain(&rx, Duration::from_millis(60));
        crate::tap::SETTINGS_OPEN.store(false, Ordering::SeqCst);
        // Settings is brought forward (a `PieBlocked`), but no pie shows, hovers or runs
        assert!(!seen.iter().any(|s| s.starts_with("show")), "{seen:?}");
        // the same hold, with Settings closed, does open the pie: the absence above is the guard's
        let (n, rx) = notifier();
        let state = Arc::new(Mutex::new(Gesture::Idle));
        let sink = test_sink(&state, n);
        hold_virtual(&sink, 450);
        sink.feed(VIRTUAL_DEVICE, (160.0, 100.0), &[]);
        let open = drain(&rx, Duration::from_millis(60));
        assert!(open.iter().any(|s| s.starts_with("show")), "{open:?}");
        let _ = &rx;
        assert!(matches!(*crate::config::lock(&state), Gesture::Idle));
    }

    #[test]
    fn a_hold_that_slides_before_the_pie_opens_leaves_no_trace() {
        let _c = Cfg::set(fast);
        let mut rig = Rig::new();
        rig.frames(120, &[finger(0.5)]); // armed (100 ms), not open yet (150 ms)
        assert!(matches!(
            *crate::config::lock(&rig.state),
            Gesture::Pending {
                source: Source::Touch,
                ..
            }
        ));
        rig.frames(100, &[finger(0.6)]); // slid 10 mm
        assert!(matches!(*crate::config::lock(&rig.state), Gesture::Idle));
        std::thread::sleep(Duration::from_millis(250)); // the hold timer gives up silently
        let got = drain(&rig.rx, Duration::from_millis(50));
        assert!(!got.iter().any(|s| s.starts_with("show")), "{got:?}");
        assert!(!ACTIVE.load(Ordering::SeqCst));
    }

    #[test]
    fn lifting_before_the_pie_opens_does_not_replay_a_click() {
        let _c = Cfg::set(fast);
        let mut rig = Rig::new();
        rig.frames(130, &[finger(0.5)]);
        rig.frames(20, &[]);
        assert!(matches!(*crate::config::lock(&rig.state), Gesture::Idle));
        std::thread::sleep(Duration::from_millis(200));
        let got = drain(&rig.rx, Duration::from_millis(50));
        assert!(
            !got.iter().any(|s| s.starts_with("show") || s == "hide"),
            "{got:?}"
        );
    }

    #[test]
    fn a_mouse_gesture_that_is_running_keeps_the_pie() {
        let _c = Cfg::set(fast);
        let mut rig = Rig::new();
        *crate::config::lock(&rig.state) = Gesture::ActivePie {
            id: 1,
            center: Point::new(1.0, 1.0),
            profile: 0,
            path: Vec::new(),
            hover: None,
            source: Source::Mouse,
        };
        rig.frames(400, &[finger(0.5)]);
        rig.frames(20, &[]);
        assert!(
            matches!(
                *crate::config::lock(&rig.state),
                Gesture::ActivePie {
                    source: Source::Mouse,
                    ..
                }
            ),
            "the touch neither took the gesture over nor closed it"
        );
        assert!(!ACTIVE.load(Ordering::SeqCst));
    }

    #[test]
    fn while_a_trackpad_gesture_runs_the_mouse_trigger_passes_through() {
        let _c = Cfg::set(fast);
        let (n, rx) = notifier();
        let st = Arc::new(Mutex::new(Gesture::Pending {
            id: 1,
            pos: Point::new(1.0, 1.0),
            profile: 0,
            app: None,
            source: Source::Touch,
        }));
        for kind in [Kind::Down, Kind::Drag, Kind::Up] {
            let ev = Ev::Mouse {
                kind,
                button: 2,
                pos: Point::new(1.0, 1.0),
            };
            assert_eq!(dispatch(&st, &n, ev, &|| {}), Verdict::Keep, "{kind:?}");
        }
        assert!(matches!(
            *crate::config::lock(&st),
            Gesture::Pending {
                source: Source::Touch,
                ..
            }
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn paused_means_no_trackpad_hold() {
        let _c = Cfg::set(fast);
        crate::tap::PAUSED.store(true, Ordering::SeqCst);
        let mut rig = Rig::new();
        rig.frames(400, &[finger(0.5)]);
        crate::tap::PAUSED.store(false, Ordering::SeqCst);
        assert!(matches!(*crate::config::lock(&rig.state), Gesture::Idle));
        assert!(!ACTIVE.load(Ordering::SeqCst));
    }

    #[test]
    fn a_trackpad_hold_while_settings_is_open_brings_settings_forward_like_the_mouse() {
        let _c = Cfg::set(fast);
        crate::tap::SETTINGS_OPEN.store(true, Ordering::SeqCst);
        let mut rig = Rig::new();
        rig.frames(400, &[finger(0.5)]);
        crate::tap::SETTINGS_OPEN.store(false, Ordering::SeqCst);
        let mut blocked = 0;
        while let Ok(m) = rig.rx.recv_timeout(Duration::from_millis(100)) {
            match m {
                Msg::PieBlocked { .. } => blocked += 1,
                Msg::ShowPie { .. } => panic!("the pie opened while Settings was open"),
                _ => {}
            }
        }
        // Settings was told, and no gesture was started
        assert!(blocked >= 1, "no PieBlocked was sent");
        assert!(matches!(*crate::config::lock(&rig.state), Gesture::Idle));
        assert!(!ACTIVE.load(Ordering::SeqCst));
    }

    /// Opens a trackpad pie in `rig` and waits until it is open.
    fn open_pie(rig: &mut Rig) {
        rig.frames(180, &[finger(0.5)]);
        std::thread::sleep(Duration::from_millis(150));
        assert!(
            matches!(
                *crate::config::lock(&rig.state),
                Gesture::ActivePie {
                    source: Source::Touch,
                    ..
                }
            ),
            "the pie did not open"
        );
        let _ = drain(&rig.rx, Duration::from_millis(30));
    }

    fn click(rig: &Rig, kind: Kind, button: i64) -> Verdict {
        dispatch(
            &rig.state,
            &rig.n,
            Ev::Mouse {
                kind,
                button,
                pos: Point::new(500.0, 500.0),
            },
            &|| {},
        )
    }

    #[test]
    fn a_click_with_one_more_finger_is_swallowed_and_never_reaches_the_app() {
        let _c = Cfg::set(|c| {
            fast(c);
            c.touch_click_action = crate::config::TouchClickAction::Nothing;
        });
        let mut rig = Rig::new();
        open_pie(&mut rig);
        // a second finger lands and the trackpad is pressed: macOS calls it a right or a left click
        let two = [
            finger(0.5),
            Finger {
                id: 2,
                x: 0.8,
                y: 0.6,
                size: 1.0,
            },
        ];
        rig.frames(300, &two); // a press, longer than a tap
        assert_eq!(FINGERS.load(Ordering::SeqCst), 2);
        for button in [0, 1] {
            assert_eq!(
                click(&rig, Kind::Down, button),
                Verdict::Drop,
                "press {button}"
            );
            assert_eq!(
                click(&rig, Kind::Drag, button),
                Verdict::Drop,
                "drag {button}"
            );
            assert_eq!(
                click(&rig, Kind::Up, button),
                Verdict::Drop,
                "release {button}"
            );
            // the release ended it: the next click is judged afresh (fingers still two: swallowed again)
        }
        // the extra finger lifts, the pie is still open: a click of any kind is still not the
        // app's (it used to go through and click the page under the pie)
        rig.frames(30, &[finger(0.5)]);
        assert_eq!(click(&rig, Kind::Down, 0), Verdict::Drop);
        assert_eq!(click(&rig, Kind::Up, 0), Verdict::Drop);
        // the pie closes: the next click is the app's again
        rig.frames(30, &[]);
        assert_eq!(click(&rig, Kind::Down, 0), Verdict::Keep);
        assert_eq!(click(&rig, Kind::Up, 0), Verdict::Keep);
    }

    #[test]
    fn a_swallowed_click_never_leaves_the_next_one_stuck() {
        let _c = Cfg::set(|c| {
            fast(c);
            c.touch_click_action = crate::config::TouchClickAction::Nothing;
        });
        let mut rig = Rig::new();
        open_pie(&mut rig);
        let two = [
            finger(0.5),
            Finger {
                id: 2,
                x: 0.8,
                y: 0.6,
                size: 1.0,
            },
        ];
        rig.frames(200, &two);
        assert_eq!(click(&rig, Kind::Down, 0), Verdict::Drop);
        // its release is lost (the tap was disabled): the gesture ends, the next press starts afresh
        rig.frames(30, &[]);
        assert_eq!(click(&rig, Kind::Down, 0), Verdict::Keep, "a normal press");
        assert_eq!(
            click(&rig, Kind::Up, 0),
            Verdict::Keep,
            "and its release is not eaten"
        );
    }

    #[test]
    fn right_click_of_a_mouse_while_one_finger_holds_still_switches_the_pie_path() {
        // one finger only: the count is not "fingers + 1", so it is not the extra click and the
        // existing right-click rule decides (no app pie here: nothing to switch, the click passes)
        let _c = Cfg::set(fast);
        let mut rig = Rig::new();
        open_pie(&mut rig);
        assert_eq!(FINGERS.load(Ordering::SeqCst), 1);
        let v = click(&rig, Kind::Down, 1);
        assert!(matches!(
            *crate::config::lock(&rig.state),
            Gesture::ActivePie { .. }
        ));
        assert_eq!(
            v,
            Verdict::Drop,
            "the right-click rule swallows it while the pie is open"
        );
        assert_eq!(click(&rig, Kind::Up, 1), Verdict::Drop);
    }

    #[test]
    fn a_tap_of_the_extra_finger_swallows_the_click_the_system_makes_of_it() {
        let _c = Cfg::set(|c| {
            fast(c);
            c.touch_click_action = crate::config::TouchClickAction::Nothing;
        });
        let mut rig = Rig::new();
        open_pie(&mut rig);
        let two = [
            finger(0.5),
            Finger {
                id: 2,
                x: 0.8,
                y: 0.6,
                size: 1.0,
            },
        ];
        rig.frames(80, &two);
        rig.frames(20, &[finger(0.5)]); // the extra finger lifted: a tap
        assert_eq!(FINGERS.load(Ordering::SeqCst), 1);
        // the system's tap-to-click arrives with the old count
        assert_eq!(click(&rig, Kind::Down, 0), Verdict::Drop);
        assert_eq!(click(&rig, Kind::Up, 0), Verdict::Drop);
        // and it counted once: the window is spent, so a further click does not run the action
        // again (the pie is still open, so it is swallowed all the same)
        let again = rig.rx.try_iter().count();
        assert_eq!(click(&rig, Kind::Down, 0), Verdict::Drop);
        assert_eq!(click(&rig, Kind::Up, 0), Verdict::Drop);
        assert_eq!(rig.rx.try_iter().count(), again, "no second action");
        rig.frames(30, &[]); // the pie closes
        assert_eq!(click(&rig, Kind::Down, 0), Verdict::Keep);
        assert_eq!(click(&rig, Kind::Up, 0), Verdict::Keep);
    }

    #[test]
    fn the_extra_click_ticks_the_mode_switch_once() {
        let _c = Cfg::set(|c| {
            fast(c);
            c.touch_click_action = crate::config::TouchClickAction::SwitchPie;
            // only the general pie: whichever app happens to be under the pointer (or in front)
            // when the test runs must not own a pie of its own, or the click would switch it
            c.profiles.retain(|p| p.is_general());
        });
        let mut rig = Rig::new();
        open_pie(&mut rig);
        crate::side::set_target(Some("firefox".into()), false); // an app with a pie in the defaults
        // there is no app pie in this config, so nothing switches and nothing ticks
        let two = [
            finger(0.5),
            Finger {
                id: 2,
                x: 0.8,
                y: 0.6,
                size: 1.0,
            },
        ];
        rig.frames(200, &two);
        click(&rig, Kind::Down, 0);
        click(&rig, Kind::Up, 0);
        let got = drain(&rig.rx, Duration::from_millis(30));
        assert!(!got.iter().any(|s| s.starts_with("haptic")), "{got:?}");
    }

    #[test]
    fn a_gesture_ends_cleanly_when_the_source_stops() {
        let _c = Cfg::set(fast);
        let mut rig = Rig::new();
        open_pie(&mut rig);
        let cursor = || Some(rig.pointer.get());
        let env = Env {
            state: &rig.state,
            notifier: &rig.n,
            cursor: &cursor,
        };
        rig.bridge.reset(&env);
        assert!(matches!(*crate::config::lock(&rig.state), Gesture::Idle));
        assert!(!ACTIVE.load(Ordering::SeqCst));
        assert_eq!(FINGERS.load(Ordering::SeqCst), 0);
        let got = drain(&rig.rx, Duration::from_millis(30));
        assert!(got.iter().any(|s| s == "hide"), "{got:?}");
    }

    #[test]
    fn clicks_with_no_trackpad_gesture_are_never_touched() {
        let _c = Cfg::set(fast);
        let (n, rx) = notifier();
        let st = Arc::new(Mutex::new(Gesture::Idle));
        FINGERS.store(2, Ordering::SeqCst); // even with two fingers on the trackpad
        for button in [0, 1] {
            for kind in [Kind::Down, Kind::Drag, Kind::Up] {
                let ev = Ev::Mouse {
                    kind,
                    button,
                    pos: Point::new(1.0, 1.0),
                };
                assert_eq!(dispatch(&st, &n, ev, &|| {}), Verdict::Keep);
            }
        }
        assert!(rx.try_recv().is_err());
        assert!(!extra::interested(Kind::Down) && !extra::interested(Kind::Up));
    }
}
