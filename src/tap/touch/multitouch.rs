//! The macOS touch source: contact frames from Apple's private `MultitouchSupport.framework`.
//!
//! **Everything here is a design assumption until spikes S1, S5, S7 and S10 of
//! `docs/developer/TRACKPAD_HAPTICS_PLAN.md` are answered on a real Mac.** No public API gives a background
//! app the number and position of the fingers on a trackpad; the private framework is the one the
//! open-source tools use. To keep a macOS update from ever stopping the app:
//!
//! - the framework is loaded at run time (`dlopen`, by its fixed system path) and only after the
//!   user turned `touch_trigger` on; a missing file or symbol means [`Status::Unavailable`];
//! - the touch record is read through one `repr(C)` view ([`RawTouch`]) and **self-checked** on the
//!   first frames ([`SelfCheck`]): out-of-range values switch the source off for the session;
//! - every callback is wrapped in `catch_unwind` and never blocks;
//! - the mouse is never affected.
//!
//! Layout of the record: as published in the open-source headers of the framework (96 bytes). It
//! was **not verified on a Mac in this change**; keep it in this file and note the macOS release it
//! was verified on when S1 is done.

use super::recognizer::Finger;
use super::{Sink, Status, set_status};
use crate::config::{cfg, lock};
use crate::msg::Notifier;
use crate::tap::gesture::Gesture;
use std::ffi::{CString, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

/// Fixed system path of the framework.
const FRAMEWORK: &str =
    "/System/Library/PrivateFrameworks/MultitouchSupport.framework/MultitouchSupport";
/// How often the config switch and stale gestures are looked at.
const TICK: Duration = Duration::from_millis(250);
/// How often the list of devices is read again (a Bluetooth trackpad appears after the start).
const RESCAN: Duration = Duration::from_secs(5);
/// A loop that was not run for this long means the Mac slept: the source is restarted.
const SLEEP_GAP: Duration = Duration::from_secs(3);

/// Whether the loop was not run for longer than [`SLEEP_GAP`], by the clock of the machine
/// (`wall`) or by the clock of the process (`mono`). `Instant` does not count the time the Mac is
/// asleep, so alone it never noticed a real sleep (the devices then stayed dead until the program
/// was restarted) and fired only when the thread itself was held back (App Nap), which restarted a
/// source that was fine. The wall clock sees the sleep; the process clock keeps the case of a
/// thread that stalls without the Mac sleeping, whose callbacks cannot be trusted either.
fn loop_stalled(wall: Option<Duration>, mono: Duration) -> bool {
    mono > SLEEP_GAP || wall.is_some_and(|w| w > SLEEP_GAP)
}

/// Most fingers read from one frame.
const MAX_FINGERS: usize = 16;
/// Frames with fingers that are checked before the data is trusted.
const CHECKED_FRAMES: u32 = 40;

/// A contact record of the framework. 96 bytes; only a few fields are read.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RawTouch {
    pub frame: i32,
    pub timestamp: f64,
    pub identifier: i32,
    pub state: i32,
    pub finger_id: i32,
    pub hand_id: i32,
    pub norm_x: f32,
    pub norm_y: f32,
    pub vel_x: f32,
    pub vel_y: f32,
    pub size: f32,
    pub zero1: i32,
    pub angle: f32,
    pub major: f32,
    pub minor: f32,
    pub mm: [f32; 4],
    pub zero2: [i32; 2],
    pub unknown: f32,
}

const _: () = assert!(std::mem::size_of::<RawTouch>() == 96);
const _: () = assert!(std::mem::offset_of!(RawTouch, norm_x) == 32);
const _: () = assert!(std::mem::offset_of!(RawTouch, size) == 48);

/// States 3 (making touch) and 4 (touching) are a finger on the surface. Design assumption (S1).
fn touching(state: i32) -> bool {
    state == 3 || state == 4
}

/// Whether one record looks like what the recognizer expects.
fn plausible(t: &RawTouch) -> bool {
    let slack = -0.05..=1.05;
    (0..=7).contains(&t.state)
        && slack.contains(&t.norm_x)
        && slack.contains(&t.norm_y)
        && t.size.is_finite()
        && (0.0..=50.0).contains(&t.size)
        && (0..=100).contains(&t.identifier)
}

/// Checks the first frames: values in range, timestamps not going back (per device).
#[derive(Debug, Default)]
pub struct SelfCheck {
    seen: u32,
    last_ts: Vec<(u64, f64)>,
    pub failed: bool,
}

impl SelfCheck {
    /// Feeds one frame. Returns false once the data is not to be trusted.
    pub fn feed(&mut self, device: u64, ts: f64, touches: &[RawTouch]) -> bool {
        if self.failed {
            return false;
        }
        if self.seen >= CHECKED_FRAMES || touches.is_empty() {
            return true;
        }
        self.seen += 1;
        let back = match self.last_ts.iter_mut().find(|(d, _)| *d == device) {
            Some((_, last)) => {
                let back = ts < *last;
                *last = ts;
                back
            }
            None => {
                self.last_ts.push((device, ts));
                false
            }
        };
        if back || !ts.is_finite() || touches.iter().any(|t| !plausible(t)) {
            self.failed = true;
        }
        !self.failed
    }
}

/// A trackpad is wide and tall; a Magic Mouse is narrow and the Touch Bar thin. Design assumption (S1).
pub fn is_trackpad_sized(w_mm: f32, h_mm: f32) -> bool {
    let (short, long) = if w_mm < h_mm {
        (w_mm, h_mm)
    } else {
        (h_mm, w_mm)
    };
    short >= 60.0 && long >= 90.0
}

// ------------------------------------------------------------------ loading

type ContactCallback = extern "C" fn(*mut c_void, *const RawTouch, c_int, f64, c_int) -> c_int;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}
const RTLD_NOW: c_int = 2;

/// CoreFoundation, for the array of devices. Stubbed off macOS so this file builds in the tests.
#[cfg(target_os = "macos")]
mod cf {
    use std::ffi::c_void;
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFArrayGetCount(a: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(a: *const c_void, i: isize) -> *const c_void;
        fn CFRelease(a: *const c_void);
    }
    pub unsafe fn count(a: *const c_void) -> isize {
        unsafe { CFArrayGetCount(a) }
    }
    pub unsafe fn at(a: *const c_void, i: isize) -> *const c_void {
        unsafe { CFArrayGetValueAtIndex(a, i) }
    }
    pub unsafe fn release(a: *const c_void) {
        unsafe { CFRelease(a) }
    }
}
#[cfg(not(target_os = "macos"))]
mod cf {
    use std::ffi::c_void;
    pub unsafe fn count(_: *const c_void) -> isize {
        0
    }
    pub unsafe fn at(_: *const c_void, _: isize) -> *const c_void {
        std::ptr::null()
    }
    pub unsafe fn release(_: *const c_void) {}
}

/// The functions of the framework that are used.
struct Api {
    create_list: unsafe extern "C" fn() -> *const c_void,
    register: unsafe extern "C" fn(*mut c_void, ContactCallback),
    unregister: unsafe extern "C" fn(*mut c_void, ContactCallback),
    start: unsafe extern "C" fn(*mut c_void, c_int),
    stop: unsafe extern "C" fn(*mut c_void),
    dimensions: unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int),
    /// Optional: says whether the device is the built-in trackpad. Read as one byte.
    is_built_in: Option<unsafe extern "C" fn(*mut c_void) -> u8>,
}

/// Loads the framework. `None` if the file or a required symbol is missing.
#[allow(clippy::missing_transmute_annotations)] // the field types name them
fn load() -> Option<Api> {
    let path = CString::new(FRAMEWORK).ok()?;
    // SAFETY: plain dlopen of a fixed system path.
    let lib = unsafe { dlopen(path.as_ptr(), RTLD_NOW) };
    if lib.is_null() {
        return None;
    }
    let sym = |name: &str| -> *mut c_void {
        match CString::new(name) {
            // SAFETY: `lib` is a live handle and `name` a valid C string.
            Ok(n) => unsafe { dlsym(lib, n.as_ptr()) },
            Err(_) => std::ptr::null_mut(),
        }
    };
    let (a, b, c, d, e, f) = (
        sym("MTDeviceCreateList"),
        sym("MTRegisterContactFrameCallback"),
        sym("MTUnregisterContactFrameCallback"),
        sym("MTDeviceStart"),
        sym("MTDeviceStop"),
        sym("MTDeviceGetSensorSurfaceDimensions"),
    );
    if [a, b, c, d, e, f].iter().any(|p| p.is_null()) {
        return None;
    }
    let built_in = sym("MTDeviceIsBuiltIn");
    // SAFETY: the signatures are those of the framework's published headers; if they differ on a
    // macOS release the self-check turns the source off.
    unsafe {
        Some(Api {
            create_list: std::mem::transmute(a),
            register: std::mem::transmute(b),
            unregister: std::mem::transmute(c),
            start: std::mem::transmute(d),
            stop: std::mem::transmute(e),
            dimensions: std::mem::transmute(f),
            is_built_in: if built_in.is_null() {
                None
            } else {
                Some(std::mem::transmute(built_in))
            },
        })
    }
}

// ------------------------------------------------------------------ the shared hub

/// What the contact-frame callback (a thread of the framework, no user data) can reach.
struct Hub {
    sink: Sink,
    check: Mutex<SelfCheck>,
    /// Surface size in mm of each started device.
    sizes: Mutex<Vec<(u64, (f32, f32))>>,
    /// Frames are processed only while this is true.
    enabled: AtomicBool,
    /// The self-check failed: the source stays off until the app restarts.
    broken: AtomicBool,
}

static HUB: OnceLock<Hub> = OnceLock::new();

extern "C" fn on_contacts(
    device: *mut c_void,
    touches: *const RawTouch,
    count: c_int,
    timestamp: f64,
    _frame: c_int,
) -> c_int {
    let _ = std::panic::catch_unwind(|| {
        handle_frame(device as usize as u64, touches, count, timestamp)
    });
    0
}

fn handle_frame(device: u64, touches: *const RawTouch, count: c_int, timestamp: f64) {
    let Some(hub) = HUB.get() else { return };
    if !hub.enabled.load(Ordering::Relaxed) {
        return;
    }
    let raw: &[RawTouch] = if touches.is_null() || count <= 0 {
        &[]
    } else {
        // SAFETY: the framework passes `count` records.
        unsafe { std::slice::from_raw_parts(touches, (count as usize).min(MAX_FINGERS)) }
    };
    if !lock(&hub.check).feed(device, timestamp, raw) {
        hub.enabled.store(false, Ordering::SeqCst);
        hub.broken.store(true, Ordering::SeqCst);
        return;
    }
    let mut fingers = [Finger {
        id: 0,
        x: 0.0,
        y: 0.0,
        size: 0.0,
    }; MAX_FINGERS];
    let mut n = 0;
    for t in raw.iter().filter(|t| touching(t.state)) {
        fingers[n] = Finger {
            id: t.identifier,
            x: t.norm_x,
            y: t.norm_y,
            size: t.size,
        };
        n += 1;
    }
    let size_mm = lock(&hub.sizes)
        .iter()
        .find(|(d, _)| *d == device)
        .map_or((0.0, 0.0), |(_, s)| *s);
    hub.sink.feed(device, size_mm, &fingers[..n]);
}

// ------------------------------------------------------------------ the thread

/// Devices that are being read.
struct Running {
    list: *const c_void,
    /// Every device the system listed (trackpads or not), to notice one that came or went.
    all: Vec<usize>,
    /// The ones that are read.
    devices: Vec<usize>,
    built_in: bool,
}

fn device_ids(list: *const c_void) -> Vec<usize> {
    // SAFETY: `list` is a live CFArray of devices.
    let n = unsafe { cf::count(list) };
    (0..n)
        .map(|i| unsafe { cf::at(list, i) } as usize)
        .collect()
}

fn start_devices(api: &Api, hub: &Hub) -> Option<Running> {
    // SAFETY: the function takes no arguments and returns a retained CFArray (or null).
    let list = unsafe { (api.create_list)() };
    if list.is_null() {
        return None;
    }
    let all = device_ids(list);
    let mut started = Vec::new();
    let mut built_in = false;
    lock(&hub.sizes).clear();
    for &d in &all {
        let dev = d as *mut c_void;
        let (mut w, mut h): (c_int, c_int) = (0, 0);
        // SAFETY: `dev` is in the live list; the out pointers are valid.
        unsafe { (api.dimensions)(dev, &mut w, &mut h) };
        // the framework reports hundredths of a millimeter (design assumption, S1)
        let size = (w as f32 / 100.0, h as f32 / 100.0);
        if !is_trackpad_sized(size.0, size.1) {
            diag::log!("touch", "device_skipped", w_mm = size.0, h_mm = size.1);
            continue;
        }
        lock(&hub.sizes).push((d as u64, size));
        // SAFETY: registering and starting a live device with our callback.
        unsafe {
            (api.register)(dev, on_contacts);
            (api.start)(dev, 0);
        }
        // SAFETY: optional call on a live device.
        if let Some(f) = api.is_built_in {
            built_in |= unsafe { f(dev) } != 0;
        }
        started.push(d);
    }
    diag::log!(
        "touch",
        "devices",
        seen = all.len(),
        started = started.len()
    );
    Some(Running {
        list,
        all,
        devices: started,
        built_in,
    })
}

fn stop_devices(api: &Api, run: Running) {
    for &d in &run.devices {
        let dev = d as *mut c_void;
        // SAFETY: the list that holds the devices is still alive (released below).
        unsafe {
            (api.unregister)(dev, on_contacts);
            (api.stop)(dev);
        }
    }
    // SAFETY: balances the create call.
    unsafe { cf::release(run.list) };
}

fn status_of(run: &Running) -> Status {
    if run.devices.is_empty() {
        Status::NoDevice
    } else {
        Status::Ready {
            devices: run.devices.len(),
            built_in: run.built_in,
        }
    }
}

/// Starts the thread. It idles while `touch_trigger` is off.
pub(super) fn spawn(state: Arc<Mutex<Gesture>>, notifier: Notifier) {
    let hub = HUB.get_or_init(|| Hub {
        sink: Sink::new(state, notifier),
        check: Mutex::new(SelfCheck::default()),
        sizes: Mutex::new(Vec::new()),
        enabled: AtomicBool::new(false),
        broken: AtomicBool::new(false),
    });
    let _ = std::thread::Builder::new()
        .name("cursorpie-touch".into())
        .spawn(move || run(hub));
}

fn reset_bridge(hub: &Hub) {
    hub.sink.reset();
}

fn run(hub: &'static Hub) {
    let mut api: Option<Option<Api>> = None; // loaded the first time the setting is on
    let mut running: Option<Running> = None;
    let (mut last_loop, mut last_scan) = (Instant::now(), Instant::now());
    let mut last_wall = SystemTime::now();
    loop {
        std::thread::sleep(TICK);
        // `Err` = the clock was set back: no sign of a sleep
        let wall = SystemTime::now().duration_since(last_wall).ok();
        let slept = loop_stalled(wall, last_loop.elapsed());
        last_loop = Instant::now();
        last_wall = SystemTime::now();
        let on = cfg().touch_trigger;

        if !on || slept || hub.broken.load(Ordering::SeqCst) {
            if let (Some(run), Some(Some(a))) = (running.take(), api.as_ref()) {
                hub.enabled.store(false, Ordering::SeqCst);
                stop_devices(a, run);
                reset_bridge(hub);
                diag::log!("touch", "source", state = "stopped", slept = slept);
            }
            if hub.broken.load(Ordering::SeqCst) {
                set_status(Status::Unavailable);
                continue;
            }
            if !on {
                set_status(Status::Off);
                continue;
            }
        }

        let loaded = api.get_or_insert_with(|| {
            let a = load();
            diag::log!("touch", "framework", loaded = a.is_some());
            a
        });
        let Some(a) = loaded.as_ref() else {
            set_status(Status::Unavailable);
            continue;
        };

        match &running {
            None => {
                hub.enabled.store(true, Ordering::SeqCst);
                match start_devices(a, hub) {
                    Some(run) => {
                        set_status(status_of(&run));
                        diag::log!("touch", "source", state = "running");
                        running = Some(run);
                    }
                    None => set_status(Status::NoDevice),
                }
                last_scan = Instant::now();
            }
            Some(run) => {
                if last_scan.elapsed() >= RESCAN {
                    last_scan = Instant::now();
                    // SAFETY: as in `start_devices`.
                    let list = unsafe { (a.create_list)() };
                    let changed = !list.is_null() && {
                        let mut now_ids = device_ids(list);
                        let mut before = run.all.clone();
                        now_ids.sort_unstable();
                        before.sort_unstable();
                        now_ids != before
                    };
                    if !list.is_null() {
                        // SAFETY: balances the create call.
                        unsafe { cf::release(list) };
                    }
                    if changed && let Some(old) = running.take() {
                        hub.enabled.store(false, Ordering::SeqCst);
                        stop_devices(a, old);
                        reset_bridge(hub);
                        hub.enabled.store(true, Ordering::SeqCst);
                        match start_devices(a, hub) {
                            Some(run) => {
                                set_status(status_of(&run));
                                running = Some(run);
                            }
                            None => set_status(Status::NoDevice),
                        }
                    }
                }
                hub.sink.poll();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(x: f32, y: f32) -> RawTouch {
        RawTouch {
            state: 4,
            norm_x: x,
            norm_y: y,
            size: 1.0,
            identifier: 1,
            ..RawTouch::default()
        }
    }

    #[test]
    fn the_record_has_the_published_layout() {
        assert_eq!(std::mem::size_of::<RawTouch>(), 96);
        assert_eq!(std::mem::offset_of!(RawTouch, timestamp), 8);
        assert_eq!(std::mem::offset_of!(RawTouch, identifier), 16);
        assert_eq!(std::mem::offset_of!(RawTouch, state), 20);
        assert_eq!(std::mem::offset_of!(RawTouch, mm), 68);
    }

    #[test]
    fn a_sleep_is_seen_by_the_wall_clock() {
        let long = SLEEP_GAP + Duration::from_secs(1);
        // the Mac slept: the process clock did not move, the wall clock did
        assert!(loop_stalled(Some(long), TICK));
        // the thread was held back: both clocks moved
        assert!(loop_stalled(Some(long), long));
        // the thread was held back and the wall clock was set back meanwhile
        assert!(loop_stalled(None, long));
        // an ordinary tick
        assert!(!loop_stalled(Some(TICK), TICK));
        assert!(
            !loop_stalled(None, TICK),
            "a clock set back alone is no sleep"
        );
        assert!(
            !loop_stalled(Some(SLEEP_GAP), SLEEP_GAP),
            "the gap itself is still fine"
        );
    }

    #[test]
    fn only_states_three_and_four_are_fingers() {
        for s in 0..=7 {
            assert_eq!(touching(s), s == 3 || s == 4, "{s}");
        }
    }

    #[test]
    fn sane_frames_pass_the_self_check() {
        let mut c = SelfCheck::default();
        for i in 0..100 {
            assert!(c.feed(1, i as f64 * 0.01, &[touch(0.4, 0.6)]));
        }
        assert!(!c.failed);
    }

    #[test]
    fn a_changed_layout_switches_the_source_off() {
        // positions far outside 0..1, as a shifted layout would read
        let mut c = SelfCheck::default();
        assert!(!c.feed(1, 0.0, &[touch(1234.5, -7.0)]));
        assert!(c.failed);
        assert!(!c.feed(1, 1.0, &[touch(0.5, 0.5)]), "and it stays off");
    }

    #[test]
    fn timestamps_that_go_back_fail_the_check() {
        let mut c = SelfCheck::default();
        assert!(c.feed(1, 5.0, &[touch(0.5, 0.5)]));
        assert!(
            c.feed(2, 1.0, &[touch(0.5, 0.5)]),
            "another device has its own clock"
        );
        assert!(!c.feed(1, 4.0, &[touch(0.5, 0.5)]));
    }

    #[test]
    fn empty_frames_and_late_frames_are_not_checked() {
        let mut c = SelfCheck::default();
        assert!(c.feed(1, 0.0, &[]));
        for i in 0..CHECKED_FRAMES {
            c.feed(1, i as f64, &[touch(0.5, 0.5)]);
        }
        assert!(
            c.feed(1, 0.0, &[touch(9.0, 9.0)]),
            "after the first frames nothing is judged"
        );
    }

    #[test]
    fn trackpads_pass_and_mice_and_touch_bars_do_not() {
        assert!(is_trackpad_sized(130.0, 90.0), "built-in");
        assert!(is_trackpad_sized(160.0, 115.0), "Magic Trackpad");
        assert!(
            is_trackpad_sized(73.0, 105.0),
            "small built-in, any orientation"
        );
        assert!(!is_trackpad_sized(52.0, 100.0), "Magic Mouse");
        assert!(!is_trackpad_sized(170.0, 20.0), "Touch Bar");
        assert!(!is_trackpad_sized(0.0, 0.0), "unknown size is skipped");
    }
}
