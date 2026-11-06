//! The Linux touch source: the fingers of a touchpad, read from the kernel's evdev nodes.
//!
//! **Why evdev and not the X server.** With the usual `xf86-input-libinput` driver a touchpad is a
//! pointer to X: it moves the pointer and clicks, and no XInput2 event carries how many fingers touch
//! it or where (libinput turns the fingers into pointer motion, scrolling and gestures before the
//! server sees them). The kernel's `/dev/input/event*` node of the touchpad does carry them
//! (the multitouch protocol B). Reading it needs permission: the nodes belong to the `input` group,
//! so the user must be in it (`sudo usermod -aG input $USER`, then log in again). Without it the
//! source says so in Settings and the mouse is unaffected. Reading a node does not take it away
//! from the X server: the pointer keeps working exactly as before.
//!
//! The pure part is [`Parser`], which turns the kernel's events into the fingers of a frame and is
//! tested everywhere. The rest finds touchpads in `/proc/bus/input/devices` (readable by all, see
//! `platform::linux::inputdev`), opens them, and reads each on its own thread.
//!
//! A touchpad only sends events when something changes, and a finger that rests still sends almost
//! none, but the recognizer measures a hold by the time of the frames. So each reader repeats the
//! last frame every [`REPEAT_MS`] while a finger is down.
//!
//! Design assumptions, not yet verified on a laptop in this change: the units of
//! `ABS_MT_POSITION_*` (millimeters come from the axis resolution, units per mm, as the kernel
//! documents), and the palm rule (the kernel's `MT_TOOL_PALM`; many touchpads never report it).

pub mod debug;

use super::recognizer::{Finger, PALM_SIZE};
use super::{Sink, Status, set_status};
use crate::config::{cfg, lock};
use crate::msg::Notifier;
use crate::platform::linux::evdev_io::{self, POLLIN, PollFd, open_node, poll_fds, raw_fd};
use crate::platform::linux::inputdev;
use crate::tap::gesture::Gesture;
use std::ffi::c_int;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

const EV_SYN: u16 = 0;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const SYN_DROPPED: u16 = 3;
const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TOOL_TYPE: u16 = 0x37;
const ABS_MT_TRACKING_ID: u16 = 0x39;
/// `MT_TOOL_PALM`
const MT_TOOL_PALM: i32 = 2;
/// Most contacts followed at once.
const MAX_SLOTS: usize = 16;
/// Size of one `struct input_event`: a `timeval` (two longs), the type, the code and the value.
const EVENT_SIZE: usize = if cfg!(target_pointer_width = "64") {
    24
} else {
    16
};

/// While a finger is down the last frame is repeated this often (ms), so the recognizer's clock runs.
const REPEAT_MS: c_int = 20;
/// How often the config switch is looked at, and stale gestures.
const TICK: Duration = Duration::from_millis(250);
/// How often the touchpads are listed again (one plugged in later is found).
const RESCAN: Duration = Duration::from_secs(5);

/// One axis of the device: `struct input_absinfo` without the current value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub min: i32,
    pub max: i32,
    /// Units per millimeter; 0 when the device does not say.
    pub resolution: i32,
}

impl Axis {
    /// `v` as 0 (min) to 1 (max).
    pub fn norm(&self, v: i32) -> f32 {
        if self.max <= self.min {
            return 0.5;
        }
        ((v - self.min) as f32 / (self.max - self.min) as f32).clamp(0.0, 1.0)
    }

    /// The length of the axis in millimeters; 0 when the resolution is not known.
    pub fn mm(&self) -> f32 {
        if self.resolution > 0 {
            (self.max - self.min) as f32 / self.resolution as f32
        } else {
            0.0
        }
    }
}

/// The two axes that place a finger.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub x: Axis,
    pub y: Axis,
}

impl Geometry {
    /// The surface in millimeters; `(0, 0)` when unknown (the recognizer then assumes a size).
    pub fn size_mm(&self) -> (f32, f32) {
        (self.x.mm(), self.y.mm())
    }
}

#[derive(Clone, Copy, Debug)]
struct Slot {
    /// The kernel's tracking id; negative = no finger in this slot.
    id: i32,
    x: i32,
    y: i32,
    palm: bool,
}

const EMPTY: Slot = Slot {
    id: -1,
    x: 0,
    y: 0,
    palm: false,
};

/// Follows the multitouch protocol B: slots hold the fingers, `SYN_REPORT` ends a frame.
#[derive(Debug)]
pub struct Parser {
    slots: [Slot; MAX_SLOTS],
    /// The slot the next events are about; `None` for a slot beyond [`MAX_SLOTS`] (ignored).
    cur: Option<usize>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub fn new() -> Self {
        Self {
            slots: [EMPTY; MAX_SLOTS],
            cur: Some(0),
        }
    }

    /// Feeds one kernel event. Returns true at the end of a frame (`SYN_REPORT`).
    pub fn feed(&mut self, ty: u16, code: u16, value: i32) -> bool {
        match (ty, code) {
            (EV_SYN, SYN_REPORT) => return true,
            (EV_SYN, SYN_DROPPED) => {
                // events were lost: what is known is stale; the next changes rebuild it
                self.slots = [EMPTY; MAX_SLOTS];
                self.cur = Some(0);
            }
            (EV_ABS, ABS_MT_SLOT) => {
                self.cur = usize::try_from(value).ok().filter(|&i| i < MAX_SLOTS);
            }
            (EV_ABS, c) => {
                let Some(slot) = self.cur.map(|i| &mut self.slots[i]) else {
                    return false;
                };
                match c {
                    ABS_MT_TRACKING_ID => {
                        *slot = if value < 0 {
                            EMPTY
                        } else {
                            Slot { id: value, ..EMPTY }
                        }
                    }
                    ABS_MT_POSITION_X => slot.x = value,
                    ABS_MT_POSITION_Y => slot.y = value,
                    ABS_MT_TOOL_TYPE => slot.palm = value == MT_TOOL_PALM,
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }

    /// Whether a finger is down.
    pub fn any_down(&self) -> bool {
        self.slots.iter().any(|s| s.id >= 0)
    }

    /// The fingers of the frame, for the recognizer.
    pub fn fingers(&self, geo: &Geometry, out: &mut Vec<Finger>) {
        out.clear();
        out.extend(self.slots.iter().filter(|s| s.id >= 0).map(|s| Finger {
            id: s.id,
            x: geo.x.norm(s.x),
            y: geo.y.norm(s.y),
            // the kernel says "palm" or nothing; a fingertip is 1 on the scale the recognizer uses
            size: if s.palm { PALM_SIZE + 1.0 } else { 1.0 },
        }));
    }
}

/// The `(type, code, value)` of every complete event in `buf`; a partial one at the end is ignored.
pub fn decode(buf: &[u8]) -> impl Iterator<Item = (u16, u16, i32)> + '_ {
    // whole events only: a partial one at the end is left out by `as_chunks`
    let (events, _partial) = buf.as_chunks::<EVENT_SIZE>();
    events.iter().map(|e| {
        let at = EVENT_SIZE - 8;
        (
            u16::from_ne_bytes([e[at], e[at + 1]]),
            u16::from_ne_bytes([e[at + 2], e[at + 3]]),
            i32::from_ne_bytes([e[at + 4], e[at + 5], e[at + 6], e[at + 7]]),
        )
    })
}

// ------------------------------------------------------------------ the kernel

/// The range of an absolute axis of an open evdev node (the OS side: `platform::linux::evdev_io`).
fn read_axis(fd: c_int, abs: u16) -> Option<Axis> {
    let r = evdev_io::read_abs(fd, abs)?;
    Some(Axis {
        min: r.min,
        max: r.max,
        resolution: r.resolution,
    })
}

// ------------------------------------------------------------------ the threads

struct Hub {
    sink: Sink,
    /// Readers of an older generation stop (the source was switched off).
    generation: AtomicU64,
    /// The devices that are being read.
    active: Mutex<Vec<u64>>,
}

static HUB: OnceLock<Hub> = OnceLock::new();

/// A stable number for a device node, for the recognizer.
fn device_id(node: &str) -> u64 {
    node.bytes()
        .fold(5381u64, |h, b| h.wrapping_mul(33) ^ u64::from(b))
        | 1
}

/// Reads one touchpad until it goes away or the generation changes.
fn read_device(hub: &'static Hub, file: std::fs::File, id: u64, generation: u64) {
    use std::io::Read;
    let fd = raw_fd(&file);
    let (Some(x), Some(y)) = (
        read_axis(fd, ABS_MT_POSITION_X),
        read_axis(fd, ABS_MT_POSITION_Y),
    ) else {
        diag::log!("touch", "device_skipped", reason = "no_axes");
        return;
    };
    let geo = Geometry { x, y };
    diag::log!(
        "touch",
        "device_open",
        w_mm = geo.size_mm().0,
        h_mm = geo.size_mm().1
    );
    let mut parser = Parser::new();
    let mut fingers = Vec::with_capacity(MAX_SLOTS);
    let mut buf = [0u8; EVENT_SIZE * 64];
    let mut reader = &file;
    'run: while hub.generation.load(Ordering::SeqCst) == generation {
        let mut p = PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        let ready = poll_fds(std::slice::from_mut(&mut p), REPEAT_MS);
        if ready > 0 {
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break 'run,
                    Ok(n) => {
                        let mut frame_ended = false;
                        for (ty, code, value) in decode(&buf[..n]) {
                            if parser.feed(ty, code, value) {
                                frame_ended = true;
                            }
                        }
                        if frame_ended {
                            parser.fingers(&geo, &mut fingers);
                            hub.sink.feed(id, geo.size_mm(), &fingers);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break 'run, // the device is gone
                }
            }
            if p.revents & !POLLIN != 0 {
                break; // hang up or error: unplugged
            }
        } else if ready == 0 && parser.any_down() {
            // nothing changed: the fingers rest; time still passes for the recognizer
            parser.fingers(&geo, &mut fingers);
            hub.sink.feed(id, geo.size_mm(), &fingers);
        }
    }
    hub.sink.device_lost(id);
    lock(&hub.active).retain(|&d| d != id);
    diag::log!("touch", "device_closed");
}

/// Starts the thread. It idles while `touch_trigger` is off: nothing is opened then.
pub(super) fn spawn(state: Arc<Mutex<Gesture>>, notifier: Notifier) {
    let hub = HUB.get_or_init(|| Hub {
        sink: Sink::new(state, notifier),
        generation: AtomicU64::new(0),
        active: Mutex::new(Vec::new()),
    });
    let _ = std::thread::Builder::new()
        .name("cursorpie-touchpad".into())
        .spawn(move || run(hub));
}

fn run(hub: &'static Hub) {
    let mut on_before = false;
    let mut last_scan: Option<Instant> = None;
    loop {
        std::thread::sleep(TICK);
        let on = cfg().touch_trigger;
        if !on {
            if on_before {
                hub.generation.fetch_add(1, Ordering::SeqCst);
                hub.sink.reset();
                lock(&hub.active).clear();
                diag::log!("touch", "source", state = "stopped");
            }
            on_before = false;
            last_scan = None;
            set_status(Status::Off);
            continue;
        }
        if !on_before {
            diag::log!("touch", "source", state = "running");
        }
        on_before = true;
        if last_scan.is_none_or(|t| t.elapsed() >= RESCAN) {
            last_scan = Some(Instant::now());
            scan(hub);
        }
        hub.sink.poll();
    }
}

/// Lists the touchpads, opens the new ones and tells Settings what was found.
fn scan(hub: &'static Hub) {
    let Ok(text) = std::fs::read_to_string("/proc/bus/input/devices") else {
        set_status(Status::Unavailable);
        return;
    };
    let pads = inputdev::parse_touchpads(&text);
    let (mut denied, generation) = (0, hub.generation.load(Ordering::SeqCst));
    for pad in &pads {
        let id = device_id(&pad.node);
        if lock(&hub.active).contains(&id) {
            continue;
        }
        let path = format!("/dev/input/{}", pad.node);
        match open_node(&path) {
            Ok(file) => {
                lock(&hub.active).push(id);
                let started = std::thread::Builder::new()
                    .name("cursorpie-evdev".into())
                    .spawn(move || read_device(hub, file, id, generation));
                if started.is_err() {
                    lock(&hub.active).retain(|&d| d != id);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => denied += 1,
            Err(_) => {}
        }
    }
    let reading = lock(&hub.active).len();
    diag::log!(
        "touch",
        "devices",
        seen = pads.len(),
        started = reading,
        denied = denied
    );
    set_status(if reading > 0 {
        Status::Ready {
            devices: reading,
            built_in: false,
        }
    } else if denied > 0 {
        Status::NoPermission { devices: denied }
    } else {
        Status::NoDevice
    });
}

#[cfg(test)]
mod tests {
    use super::super::recognizer::{Frame, Params, Recognizer, TouchEvent};
    use super::*;

    const ABS: u16 = EV_ABS;

    fn geo() -> Geometry {
        Geometry {
            x: Axis {
                min: 0,
                max: 1200,
                resolution: 12,
            },
            y: Axis {
                min: 0,
                max: 800,
                resolution: 10,
            },
        }
    }

    /// Feeds `events` and returns the fingers at the end of the frame.
    fn frame(p: &mut Parser, events: &[(u16, u16, i32)]) -> Vec<Finger> {
        let mut ended = false;
        for &(t, c, v) in events {
            ended |= p.feed(t, c, v);
        }
        assert!(ended, "the frame must end with SYN_REPORT");
        let mut out = Vec::new();
        p.fingers(&geo(), &mut out);
        out
    }

    const SYN: (u16, u16, i32) = (EV_SYN, SYN_REPORT, 0);

    #[test]
    fn the_surface_is_measured_in_millimeters_from_the_resolution() {
        assert_eq!(geo().size_mm(), (100.0, 80.0));
        let unknown = Axis {
            min: 0,
            max: 100,
            resolution: 0,
        };
        assert_eq!(unknown.mm(), 0.0);
        assert_eq!(
            Geometry {
                x: unknown,
                y: unknown
            }
            .size_mm(),
            (0.0, 0.0)
        );
    }

    #[test]
    fn positions_are_normalized_and_clamped() {
        let a = geo().x;
        assert_eq!((a.norm(0), a.norm(600), a.norm(1200)), (0.0, 0.5, 1.0));
        assert_eq!((a.norm(-50), a.norm(9999)), (0.0, 1.0));
        let flat = Axis {
            min: 5,
            max: 5,
            resolution: 1,
        };
        assert_eq!(flat.norm(5), 0.5);
    }

    #[test]
    fn a_finger_lands_moves_and_lifts() {
        let mut p = Parser::new();
        let f = frame(
            &mut p,
            &[
                (ABS, ABS_MT_SLOT, 0),
                (ABS, ABS_MT_TRACKING_ID, 41),
                (ABS, ABS_MT_POSITION_X, 600),
                (ABS, ABS_MT_POSITION_Y, 400),
                SYN,
            ],
        );
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].id, f[0].x, f[0].y, f[0].size), (41, 0.5, 0.5, 1.0));
        assert!(p.any_down());
        // only x changes: the slot keeps its y and its id
        let f = frame(&mut p, &[(ABS, ABS_MT_POSITION_X, 1200), SYN]);
        assert_eq!((f[0].id, f[0].x, f[0].y), (41, 1.0, 0.5));
        let f = frame(&mut p, &[(ABS, ABS_MT_TRACKING_ID, -1), SYN]);
        assert!(f.is_empty() && !p.any_down());
    }

    #[test]
    fn two_fingers_use_two_slots() {
        let mut p = Parser::new();
        let f = frame(
            &mut p,
            &[
                (ABS, ABS_MT_SLOT, 0),
                (ABS, ABS_MT_TRACKING_ID, 7),
                (ABS, ABS_MT_POSITION_X, 100),
                (ABS, ABS_MT_POSITION_Y, 100),
                (ABS, ABS_MT_SLOT, 1),
                (ABS, ABS_MT_TRACKING_ID, 8),
                (ABS, ABS_MT_POSITION_X, 900),
                (ABS, ABS_MT_POSITION_Y, 700),
                SYN,
            ],
        );
        assert_eq!(f.iter().map(|f| f.id).collect::<Vec<_>>(), [7, 8]);
        // the first lifts, the second stays
        let f = frame(
            &mut p,
            &[(ABS, ABS_MT_SLOT, 0), (ABS, ABS_MT_TRACKING_ID, -1), SYN],
        );
        assert_eq!(f.iter().map(|f| f.id).collect::<Vec<_>>(), [8]);
    }

    #[test]
    fn a_palm_is_reported_as_a_palm_sized_contact() {
        let mut p = Parser::new();
        let f = frame(
            &mut p,
            &[
                (ABS, ABS_MT_TRACKING_ID, 3),
                (ABS, ABS_MT_POSITION_X, 600),
                (ABS, ABS_MT_POSITION_Y, 400),
                (ABS, ABS_MT_TOOL_TYPE, MT_TOOL_PALM),
                SYN,
            ],
        );
        assert!(f[0].size > PALM_SIZE);
    }

    #[test]
    fn a_slot_beyond_the_table_is_ignored_and_lost_events_reset() {
        let mut p = Parser::new();
        let f = frame(
            &mut p,
            &[
                (ABS, ABS_MT_SLOT, 99),
                (ABS, ABS_MT_TRACKING_ID, 5),
                (ABS, ABS_MT_POSITION_X, 1),
                SYN,
            ],
        );
        assert!(f.is_empty());
        frame(
            &mut p,
            &[(ABS, ABS_MT_SLOT, 0), (ABS, ABS_MT_TRACKING_ID, 6), SYN],
        );
        assert!(p.any_down());
        assert!(!p.feed(EV_SYN, SYN_DROPPED, 0));
        assert!(!p.any_down(), "after SYN_DROPPED nothing is trusted");
        assert!(!p.feed(EV_ABS, ABS_MT_SLOT, -1));
        assert!(
            !p.feed(EV_ABS, ABS_MT_POSITION_X, 3),
            "a negative slot is ignored too"
        );
    }

    #[test]
    fn events_are_decoded_from_the_kernels_bytes() {
        let mut raw = Vec::new();
        for (ty, code, value) in [(EV_ABS, ABS_MT_POSITION_X, -5i32), (EV_SYN, SYN_REPORT, 0)] {
            raw.extend_from_slice(&[0u8; EVENT_SIZE - 8]); // the time
            raw.extend_from_slice(&ty.to_ne_bytes());
            raw.extend_from_slice(&code.to_ne_bytes());
            raw.extend_from_slice(&value.to_ne_bytes());
        }
        raw.extend_from_slice(&[1, 2, 3]); // a partial event is left alone
        let got: Vec<_> = decode(&raw).collect();
        assert_eq!(
            got,
            vec![(EV_ABS, ABS_MT_POSITION_X, -5), (EV_SYN, SYN_REPORT, 0)]
        );
    }

    #[test]
    fn a_resting_finger_that_sends_nothing_still_arms_the_hold() {
        // one frame when the finger lands, then the reader repeats it every 20 ms
        let mut p = Parser::new();
        let f = frame(
            &mut p,
            &[
                (ABS, ABS_MT_TRACKING_ID, 1),
                (ABS, ABS_MT_POSITION_X, 600),
                (ABS, ABS_MT_POSITION_Y, 400),
                SYN,
            ],
        );
        let mut rec = Recognizer::new(Params {
            typing_guard: Duration::ZERO,
            ..Params::default()
        });
        let mut out = Vec::new();
        for i in 0..40u64 {
            rec.frame(
                &Frame {
                    device: 1,
                    time: Duration::from_millis(i * 20),
                    size_mm: geo().size_mm(),
                    fingers: &f,
                },
                &mut out,
            );
        }
        assert!(matches!(out[0], TouchEvent::HoldArmed { .. }), "{out:?}");
        assert_eq!(out[1], TouchEvent::HoldOpen);
        // the lift is the last frame of the touch
        let f = frame(&mut p, &[(ABS, ABS_MT_TRACKING_ID, -1), SYN]);
        rec.frame(
            &Frame {
                device: 1,
                time: Duration::from_millis(900),
                size_mm: geo().size_mm(),
                fingers: &f,
            },
            &mut out,
        );
        assert_eq!(out.last(), Some(&TouchEvent::Release));
    }

    #[test]
    fn device_numbers_differ_and_are_never_zero() {
        assert_ne!(device_id("event9"), device_id("event10"));
        assert_ne!(device_id("event0"), 0);
    }
}
