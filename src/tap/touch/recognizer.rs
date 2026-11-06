//! The touch recognizer: decides, from frames of finger positions, whether the person is holding.
//!
//! It is **pure**: no operating-system type, no thread, no timer and no clock of its own. It is fed
//! frames and a time value, so it behaves the same on every OS and is tested with recorded frames
//! (see the tests below). The native layer (`touch::multitouch` on macOS) turns the frames of the
//! system into [`Frame`]s; [`touch::Bridge`](super::Bridge) turns the [`TouchEvent`]s into the
//! events the gesture state machine already understands.
//!
//! ```text
//! Frame { device, time, fingers: [Finger { id, x, y, size }] }     x, y in 0..1 of the surface
//!         |
//!         v
//! Recognizer ---- HoldArmed { elapsed, double }  the fingers are down and still: start counting
//!             |-- HoldOpen                       still for the hold time: the pie is open
//!             |-- Move                           the holding fingers moved (the pie is open)
//!             |-- ExtraClick                     one more finger tapped while the pie is open
//!             |-- Release                        a holding finger lifted
//!             `-- Cancel(Why)                    too many fingers, the hold was broken, the device went away
//! ```
//!
//! What it decides (every number is a setting in [`Params`] or a named constant):
//!
//! - **Exactly one finger** must touch. A different count never arms; it blocks the touch until every
//!   finger has lifted. Two fingers are not a hold: the system scrolls with them.
//! - **Still means still on the surface, in millimeters**, from the size of the device, not in
//!   screen pixels (pointer acceleration makes a few pixels a poor measure).
//! - **Palms and edges are ignored**: a contact far larger than a fingertip, and a thin border of the
//!   surface, are not counted.
//! - **Typing guard**: nothing arms soon after a key press, so resting a hand while typing is safe.
//! - **Nothing is reported for an ordinary touch.** A finger that slides, scrolls or taps quickly
//!   never produces an event: [`ARM_DELAY`] has to pass first, so the work behind `HoldArmed` is
//!   not done for every touch of the trackpad. The one thing remembered is a quick tap, so the
//!   next hold that starts within the double-click window is flagged `double` (a trackpad's
//!   "double-click": tap, then touch and hold).
//! - **Several devices**: each is tracked on its own; the first one to arm owns the gesture until it
//!   ends.

use std::time::Duration;

/// How long the fingers must be down and still before `HoldArmed` is reported. Until then the touch
/// might be the start of a scroll or a tap, and nothing is done for it. (The hold time itself counts
/// from the touch-down, so this delay is not added to it.)
pub const ARM_DELAY: Duration = Duration::from_millis(100);
/// A touch shorter than this, that did not move, is a tap.
pub const TAP_MAX: Duration = Duration::from_millis(200);
/// No frame for this long while a gesture is running: the device is gone or stopped sending.
pub const STALE_AFTER: Duration = Duration::from_millis(500);
/// A contact larger than this (in the units of the framework's `size`) is a palm. **Design
/// assumption**: the scale of `size` is checked in spike S1.
pub const PALM_SIZE: f32 = 2.5;
/// Contacts closer than this fraction of the surface to its border are ignored.
pub const EDGE: f32 = 0.03;
/// The holding fingers must move this much (mm) for a `Move` to be reported.
pub const MOVE_EPS_MM: f32 = 0.15;
/// Surface size used when the device does not say (mm). A typical built-in trackpad.
pub const DEFAULT_SURFACE_MM: (f32, f32) = (130.0, 90.0);

/// One finger on the surface, as the native layer reports it. Only fingers that touch are passed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finger {
    /// Stays the same while the finger stays down.
    pub id: i32,
    /// 0 (left) to 1 (right) of the surface.
    pub x: f32,
    /// 0 to 1 of the surface.
    pub y: f32,
    /// How large the contact is; a palm is much larger than a fingertip.
    pub size: f32,
}

/// One frame of one device.
#[derive(Clone, Copy, Debug)]
pub struct Frame<'a> {
    /// Identifies the device (the native layer uses its address).
    pub device: u64,
    /// Time of the frame, on the same clock as [`Recognizer::note_key`].
    pub time: Duration,
    /// Size of the surface in millimeters; `(0.0, 0.0)` when unknown.
    pub size_mm: (f32, f32),
    pub fingers: &'a [Finger],
}

/// The settings the recognizer needs (from [`Config`](crate::config::Config)).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// How far (mm) the holding fingers may move before the hold counts.
    pub still_mm: f32,
    /// The hold time (`pie_hold_ms`), counted from the touch-down.
    pub hold: Duration,
    /// No arming within this time of a key press (zero = off).
    pub typing_guard: Duration,
    /// The double-click window (`double_click_ms`): a hold that starts this soon after a tap is a
    /// `double`.
    pub double_click: Duration,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            still_mm: 2.0,
            hold: Duration::from_millis(500),
            typing_guard: Duration::from_millis(400),
            double_click: Duration::from_millis(260),
        }
    }
}

/// Why a gesture was cancelled (for the diagnostic log: decisions, never positions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// More fingers than the gesture allows.
    TooManyFingers,
    /// The holding finger moved before the hold time ran out: it was moving the pointer, not holding.
    Slid,
    /// The device sent no frame for [`STALE_AFTER`].
    Stale,
    /// The device was unplugged, or the source was stopped.
    DeviceLost,
}

impl Why {
    /// The name used in the diagnostic log.
    pub fn name(self) -> &'static str {
        match self {
            Why::TooManyFingers => "too_many_fingers",
            Why::Slid => "slid",
            Why::Stale => "stale",
            Why::DeviceLost => "device_lost",
        }
    }
}

/// What the recognizer reports.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TouchEvent {
    /// The fingers are down and still: a hold may be starting. `elapsed` is how long they have been
    /// down; `double` is true when this touch started within the double-click window of a tap.
    HoldArmed { elapsed: Duration, double: bool },
    /// Still for the hold time.
    HoldOpen,
    /// The holding fingers moved (only after `HoldOpen`).
    Move,
    /// One more finger tapped while the pie was open.
    ExtraClick,
    /// A holding finger lifted: the hold ends.
    Release,
    /// The hold ends without a release.
    Cancel(Why),
}

type Mm = (f32, f32);

#[derive(Clone, Copy, Debug)]
struct Contact {
    id: i32,
    mm: Mm,
}

#[derive(Clone, Copy, Debug)]
struct Holder {
    id: i32,
    start: Mm,
}

#[derive(Clone, Copy, Debug)]
struct Extra {
    id: i32,
    since: Duration,
    start: Mm,
    moved: bool,
}

#[derive(Clone, Debug)]
struct Hold {
    t0: Duration,
    holders: Vec<Holder>,
    armed: bool,
    opened: bool,
    double: bool,
    /// Where the holding finger was at the last `Move` (or when the pie opened).
    last: Mm,
    extra: Option<Extra>,
}

#[derive(Clone, Debug)]
enum Phase {
    Idle,
    /// This touch can never become a hold (wrong count, typing, it slid). Wait until all lift.
    Blocked,
    Holding(Hold),
}

#[derive(Debug)]
struct Track {
    device: u64,
    phase: Phase,
    seen: Duration,
}

/// The recognizer. One instance watches every device.
#[derive(Debug, Default)]
pub struct Recognizer {
    params: Params,
    tracks: Vec<Track>,
    owner: Option<u64>,
    last_key: Option<Duration>,
    last_tap_end: Option<Duration>,
    count: usize,
    scratch: Vec<Contact>,
}

fn dist(a: Mm, b: Mm) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn centroid(points: impl Iterator<Item = Mm>) -> Mm {
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for p in points {
        sx += p.0;
        sy += p.1;
        n += 1.0;
    }
    if n == 0.0 {
        (0.0, 0.0)
    } else {
        (sx / n, sy / n)
    }
}

impl Recognizer {
    pub fn new(params: Params) -> Self {
        Self {
            params,
            ..Self::default()
        }
    }

    pub fn params(&self) -> Params {
        self.params
    }

    /// Takes new settings. Applied to touches that start afterwards, and to the checks of one that
    /// is running.
    pub fn set_params(&mut self, params: Params) {
        self.params = params;
    }

    /// A key was pressed at `time` (same clock as the frames): the typing guard starts.
    pub fn note_key(&mut self, time: Duration) {
        self.last_key = Some(time);
    }

    /// Fingers on the surface of the device that owns the gesture (or the last one that sent a
    /// frame), after palms and edges were left out. The extra click is recognized by this count.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Whether a gesture that was armed is running (nothing is running for a plain touch).
    #[cfg(test)]
    pub fn active(&self) -> bool {
        self.owner.is_some()
    }

    /// True when no device has fingers on it that matter and no gesture is running, so the settings
    /// may be changed without surprising a touch in progress.
    pub fn idle(&self) -> bool {
        self.owner.is_none() && self.tracks.iter().all(|t| matches!(t.phase, Phase::Idle))
    }

    fn typing_blocked(&self, time: Duration) -> bool {
        let guard = self.params.typing_guard;
        !guard.is_zero() && self.last_key.is_some_and(|k| time >= k && time - k < guard)
    }

    /// Feeds one frame. The events it causes are appended to `out`.
    pub fn frame(&mut self, f: &Frame<'_>, out: &mut Vec<TouchEvent>) {
        let (w, h) = if f.size_mm.0 > 1.0 && f.size_mm.1 > 1.0 {
            f.size_mm
        } else {
            DEFAULT_SURFACE_MM
        };
        let mut contacts = std::mem::take(&mut self.scratch);
        contacts.clear();
        contacts.extend(
            f.fingers
                .iter()
                .filter(|c| {
                    c.size <= PALM_SIZE
                        && c.x >= EDGE
                        && c.x <= 1.0 - EDGE
                        && c.y >= EDGE
                        && c.y <= 1.0 - EDGE
                })
                .map(|c| Contact {
                    id: c.id,
                    mm: (c.x * w, c.y * h),
                }),
        );

        let at = match self.tracks.iter().position(|t| t.device == f.device) {
            Some(i) => i,
            None => {
                self.tracks.push(Track {
                    device: f.device,
                    phase: Phase::Idle,
                    seen: f.time,
                });
                self.tracks.len() - 1
            }
        };
        self.tracks[at].seen = f.time;
        if self.owner.is_none() || self.owner == Some(f.device) {
            self.count = contacts.len();
        }

        let phase = std::mem::replace(&mut self.tracks[at].phase, Phase::Idle);
        let next = self.step(f.device, f.time, phase, &contacts, out);
        self.tracks[at].phase = next;
        self.scratch = contacts;
        self.drop_quiet_tracks();
    }

    /// Call now and then (a few times a second) with the current time: a device that stopped
    /// sending frames in the middle of a gesture cancels it.
    pub fn poll(&mut self, now: Duration, out: &mut Vec<TouchEvent>) {
        for i in 0..self.tracks.len() {
            let t = &self.tracks[i];
            if now < t.seen || now - t.seen < STALE_AFTER {
                continue;
            }
            let device = t.device;
            let phase = std::mem::replace(&mut self.tracks[i].phase, Phase::Idle);
            if let Phase::Holding(h) = phase
                && h.armed
            {
                out.push(TouchEvent::Cancel(Why::Stale));
                self.finish(device);
            }
            if self.owner == Some(device) {
                self.owner = None;
            }
        }
        self.drop_quiet_tracks();
    }

    /// A device went away (or the source stops): its gesture, if it owned one, is cancelled.
    pub fn device_lost(&mut self, device: u64, out: &mut Vec<TouchEvent>) {
        if let Some(i) = self.tracks.iter().position(|t| t.device == device) {
            if let Phase::Holding(h) = &self.tracks[i].phase
                && h.armed
            {
                out.push(TouchEvent::Cancel(Why::DeviceLost));
            }
            self.tracks.remove(i);
        }
        if self.owner == Some(device) {
            self.owner = None;
        }
        if self.tracks.is_empty() {
            self.count = 0;
        }
    }

    /// Forgets everything (the source is switched off).
    pub fn reset(&mut self, out: &mut Vec<TouchEvent>) {
        let devices: Vec<u64> = self.tracks.iter().map(|t| t.device).collect();
        for d in devices {
            self.device_lost(d, out);
        }
        self.owner = None;
        self.count = 0;
        self.last_tap_end = None;
    }

    fn finish(&mut self, device: u64) {
        if self.owner == Some(device) {
            self.owner = None;
        }
    }

    /// Tracks with nothing going on are not kept, so a device that is never touched again costs
    /// nothing.
    fn drop_quiet_tracks(&mut self) {
        self.tracks
            .retain(|t| !matches!(t.phase, Phase::Idle) || self.owner == Some(t.device));
    }

    fn step(
        &mut self,
        device: u64,
        time: Duration,
        phase: Phase,
        now: &[Contact],
        out: &mut Vec<TouchEvent>,
    ) -> Phase {
        match phase {
            Phase::Idle => {
                if now.is_empty() {
                    Phase::Idle
                } else if now.len() > 1 || self.typing_blocked(time) {
                    Phase::Blocked
                } else {
                    self.begin(time, now)
                }
            }
            Phase::Blocked => {
                if now.is_empty() {
                    Phase::Idle
                } else {
                    Phase::Blocked
                }
            }
            Phase::Holding(h) => self.holding(device, time, h, now, out),
        }
    }

    /// Exactly one finger just landed.
    fn begin(&mut self, time: Duration, now: &[Contact]) -> Phase {
        let double = self
            .last_tap_end
            .is_some_and(|t| time >= t && time - t < self.params.double_click);
        Phase::Holding(Hold {
            t0: time,
            holders: now
                .iter()
                .map(|c| Holder {
                    id: c.id,
                    start: c.mm,
                })
                .collect(),
            armed: false,
            opened: false,
            double,
            last: centroid(now.iter().map(|c| c.mm)),
            extra: None,
        })
    }

    fn holding(
        &mut self,
        device: u64,
        time: Duration,
        mut h: Hold,
        now: &[Contact],
        out: &mut Vec<TouchEvent>,
    ) -> Phase {
        let still = self.params.still_mm;
        let find = |id: i32| now.iter().find(|c| c.id == id);

        // a holding finger lifted (or became a palm)
        if h.holders.iter().any(|o| find(o.id).is_none()) {
            let quick = time - h.t0 <= TAP_MAX;
            if h.armed {
                out.push(TouchEvent::Release);
                self.finish(device);
            }
            if !h.opened && quick && h.extra.is_none() {
                self.last_tap_end = Some(time);
            }
            return if now.is_empty() {
                Phase::Idle
            } else {
                Phase::Blocked
            };
        }

        let extras: Vec<&Contact> = now
            .iter()
            .filter(|c| !h.holders.iter().any(|o| o.id == c.id))
            .collect();

        if !h.opened {
            // before the pie opens, any other finger means the person is doing something else
            if !extras.is_empty() {
                if h.armed {
                    out.push(TouchEvent::Cancel(Why::TooManyFingers));
                    self.finish(device);
                }
                return Phase::Blocked;
            }
            let slid = h
                .holders
                .iter()
                .any(|o| dist(o.start, find(o.id).map_or(o.start, |c| c.mm)) > still);
            if slid {
                if h.armed {
                    out.push(TouchEvent::Cancel(Why::Slid));
                    self.finish(device);
                }
                return Phase::Blocked;
            }
            let held = time - h.t0;
            if !h.armed {
                let arm = ARM_DELAY.min(self.params.hold);
                if held < arm {
                    return Phase::Holding(h);
                }
                let owned_elsewhere = self.owner.is_some_and(|o| o != device);
                if owned_elsewhere || self.typing_blocked(time) {
                    return Phase::Blocked;
                }
                self.owner = Some(device);
                h.armed = true;
                out.push(TouchEvent::HoldArmed {
                    elapsed: held,
                    double: h.double,
                });
            }
            if held >= self.params.hold {
                h.opened = true;
                h.last = centroid(now.iter().map(|c| c.mm));
                out.push(TouchEvent::HoldOpen);
            }
            return Phase::Holding(h);
        }

        // the pie is open
        match extras.as_slice() {
            [] => {
                if let Some(e) = h.extra.take()
                    && time - e.since <= TAP_MAX
                    && !e.moved
                {
                    out.push(TouchEvent::ExtraClick);
                }
            }
            [one] => match &mut h.extra {
                Some(e) if e.id == one.id => {
                    if dist(e.start, one.mm) > still {
                        e.moved = true;
                    }
                }
                _ => {
                    h.extra = Some(Extra {
                        id: one.id,
                        since: time,
                        start: one.mm,
                        moved: false,
                    });
                }
            },
            _ => {
                out.push(TouchEvent::Cancel(Why::TooManyFingers));
                self.finish(device);
                return Phase::Blocked;
            }
        }
        let c = centroid(h.holders.iter().filter_map(|o| find(o.id).map(|c| c.mm)));
        if dist(c, h.last) >= MOVE_EPS_MM {
            h.last = c;
            out.push(TouchEvent::Move);
        }
        Phase::Holding(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    /// Feeds frames every 10 ms and collects what comes out, with the time of each event.
    struct Sim {
        r: Recognizer,
        t: u64,
        out: Vec<(u64, TouchEvent)>,
        device: u64,
    }

    fn f(id: i32, x: f32, y: f32) -> Finger {
        Finger {
            id,
            x,
            y,
            size: 1.0,
        }
    }

    impl Sim {
        fn new() -> Self {
            Self {
                r: Recognizer::new(Params {
                    typing_guard: Duration::ZERO,
                    ..Params::default()
                }),
                t: 1000,
                out: Vec::new(),
                device: 1,
            }
        }

        /// `ms` milliseconds with these fingers on the surface.
        fn hold(&mut self, ms: u64, fingers: &[Finger]) -> &mut Self {
            for _ in 0..ms / 10 {
                self.t += 10;
                let mut ev = Vec::new();
                self.r.frame(
                    &Frame {
                        device: self.device,
                        time: MS(self.t),
                        size_mm: (100.0, 100.0),
                        fingers,
                    },
                    &mut ev,
                );
                self.out.extend(ev.into_iter().map(|e| (self.t, e)));
            }
            self
        }

        fn kinds(&self) -> Vec<TouchEvent> {
            self.out.iter().map(|&(_, e)| e).collect()
        }

        fn clear(&mut self) {
            self.out.clear();
        }
    }

    fn armed(e: &TouchEvent) -> bool {
        matches!(e, TouchEvent::HoldArmed { .. })
    }

    #[test]
    fn a_still_finger_arms_after_the_delay_and_opens_after_the_hold_time() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]);
        let k = s.kinds();
        assert!(armed(&k[0]), "{k:?}");
        assert_eq!(k[1], TouchEvent::HoldOpen);
        assert_eq!(k.len(), 2);
        // armed ARM_DELAY after the touch-down, open after the hold time
        assert_eq!(s.out[0].0, 1000 + 10 + 100);
        assert!(s.out[1].0 >= 1000 + 10 + 500 && s.out[1].0 <= 1000 + 10 + 520);
        match k[0] {
            TouchEvent::HoldArmed { elapsed, double } => {
                assert!(elapsed >= ARM_DELAY && !double);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn lifting_after_the_pie_opened_is_a_release() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]).hold(20, &[]);
        assert_eq!(s.kinds().last(), Some(&TouchEvent::Release));
        assert!(s.r.idle());
    }

    #[test]
    fn lifting_before_the_pie_opens_is_a_release_too() {
        let mut s = Sim::new();
        s.hold(300, &[f(1, 0.5, 0.5)]).hold(20, &[]);
        let k = s.kinds();
        assert!(
            armed(&k[0]) && k[1] == TouchEvent::Release && k.len() == 2,
            "{k:?}"
        );
    }

    #[test]
    fn a_slide_before_the_hold_time_breaks_the_hold() {
        let mut s = Sim::new();
        // 2 mm of travel is allowed (100 mm surface: 0.02); 5 mm is a slide
        s.hold(200, &[f(1, 0.5, 0.5)])
            .hold(100, &[f(1, 0.51, 0.5)])
            .hold(100, &[f(1, 0.56, 0.5)])
            .hold(500, &[f(1, 0.56, 0.5)])
            .hold(20, &[]);
        let k = s.kinds();
        assert!(armed(&k[0]));
        assert_eq!(k[1], TouchEvent::Cancel(Why::Slid));
        assert_eq!(
            k.len(),
            2,
            "no more events for the rest of this touch: {k:?}"
        );
    }

    #[test]
    fn a_slide_before_arming_reports_nothing() {
        let mut s = Sim::new();
        s.hold(50, &[f(1, 0.5, 0.5)])
            .hold(500, &[f(1, 0.7, 0.5)])
            .hold(20, &[]);
        assert!(s.kinds().is_empty(), "{:?}", s.kinds());
    }

    #[test]
    fn a_still_finger_is_judged_in_millimeters_not_in_surface_fractions() {
        // the same fraction of travel is twice the distance on a surface twice as large
        let mut small = Recognizer::new(Params {
            typing_guard: Duration::ZERO,
            ..Params::default()
        });
        let mut big = Recognizer::new(Params {
            typing_guard: Duration::ZERO,
            ..Params::default()
        });
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for (r, size, out) in [(&mut small, 50.0, &mut a), (&mut big, 200.0, &mut b)] {
            for (t, x) in [(0u64, 0.5f32), (110, 0.5), (200, 0.52)] {
                r.frame(
                    &Frame {
                        device: 1,
                        time: MS(t),
                        size_mm: (size, size),
                        fingers: &[f(1, x, 0.5)],
                    },
                    out,
                );
            }
        }
        // 0.02 of 50 mm = 1 mm: still. 0.02 of 200 mm = 4 mm: a slide
        assert!(
            a.iter().all(|e| !matches!(e, TouchEvent::Cancel(_))),
            "{a:?}"
        );
        assert!(b.contains(&TouchEvent::Cancel(Why::Slid)), "{b:?}");
    }

    #[test]
    fn two_fingers_scrolling_never_arm_a_hold() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.4, 0.5), f(2, 0.6, 0.5)]).hold(20, &[]);
        assert!(s.kinds().is_empty());
    }

    #[test]
    fn a_second_finger_that_lands_late_cancels_a_one_finger_hold_that_started() {
        let mut s = Sim::new();
        s.hold(200, &[f(1, 0.5, 0.5)])
            .hold(100, &[f(1, 0.5, 0.5), f(2, 0.7, 0.5)]);
        let k = s.kinds();
        assert!(armed(&k[0]));
        assert_eq!(k[1], TouchEvent::Cancel(Why::TooManyFingers));
        // and the rest of the touch is blocked
        s.clear();
        s.hold(1000, &[f(2, 0.7, 0.5)]);
        assert!(s.kinds().is_empty());
    }

    #[test]
    fn a_second_finger_that_lands_at_once_never_arms_anything_for_one_finger() {
        let mut s = Sim::new();
        s.hold(30, &[f(1, 0.5, 0.5)])
            .hold(600, &[f(1, 0.5, 0.5), f(2, 0.7, 0.5)]);
        assert!(s.kinds().is_empty(), "{:?}", s.kinds());
    }

    #[test]
    fn a_palm_is_not_a_finger_and_does_not_count() {
        let mut s = Sim::new();
        let palm = Finger {
            id: 9,
            x: 0.5,
            y: 0.8,
            size: PALM_SIZE + 1.0,
        };
        s.hold(600, &[f(1, 0.5, 0.4), palm]);
        let k = s.kinds();
        assert!(armed(&k[0]) && k[1] == TouchEvent::HoldOpen, "{k:?}");
        // a palm alone arms nothing
        let mut s = Sim::new();
        s.hold(700, &[palm]);
        assert!(s.kinds().is_empty());
    }

    #[test]
    fn the_border_of_the_surface_is_ignored() {
        let mut s = Sim::new();
        s.hold(700, &[f(1, 0.005, 0.5)]);
        assert!(s.kinds().is_empty());
    }

    #[test]
    fn a_key_press_just_before_blocks_the_hold() {
        let mut s = Sim::new();
        s.r.set_params(Params {
            typing_guard: MS(400),
            ..s.r.params()
        });
        s.r.note_key(MS(1000));
        s.hold(700, &[f(1, 0.5, 0.5)]);
        assert!(s.kinds().is_empty(), "{:?}", s.kinds());
        // after the guard it works again
        s.hold(20, &[]);
        s.hold(700, &[f(1, 0.5, 0.5)]);
        assert!(armed(&s.kinds()[0]));
    }

    #[test]
    fn a_key_pressed_while_the_finger_is_down_but_before_arming_blocks_too() {
        let mut s = Sim::new();
        s.r.set_params(Params {
            typing_guard: MS(400),
            ..s.r.params()
        });
        s.hold(50, &[f(1, 0.5, 0.5)]);
        s.r.note_key(MS(s.t));
        s.hold(700, &[f(1, 0.5, 0.5)]);
        assert!(s.kinds().is_empty());
    }

    #[test]
    fn the_pie_follows_the_finger_and_only_when_it_moved() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]);
        s.clear();
        s.hold(100, &[f(1, 0.5, 0.5)]);
        assert!(s.kinds().is_empty(), "a still finger reports nothing");
        s.hold(10, &[f(1, 0.55, 0.5)]).hold(10, &[f(1, 0.60, 0.5)]);
        assert_eq!(s.kinds(), vec![TouchEvent::Move, TouchEvent::Move]);
    }

    #[test]
    fn a_quick_tap_of_the_extra_finger_is_an_extra_click() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]);
        s.clear();
        s.hold(80, &[f(1, 0.5, 0.5), f(2, 0.7, 0.6)])
            .hold(20, &[f(1, 0.5, 0.5)]);
        assert_eq!(s.kinds(), vec![TouchEvent::ExtraClick]);
        assert_eq!(s.r.count(), 1);
    }

    #[test]
    fn a_long_press_of_the_extra_finger_is_left_to_the_click_path() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]);
        s.clear();
        s.hold(400, &[f(1, 0.5, 0.5), f(2, 0.7, 0.6)]);
        assert_eq!(s.r.count(), 2, "the click path counts the fingers");
        s.hold(20, &[f(1, 0.5, 0.5)]);
        assert!(s.kinds().is_empty(), "no tap: {:?}", s.kinds());
    }

    #[test]
    fn an_extra_finger_that_slides_is_not_a_tap() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]);
        s.clear();
        s.hold(30, &[f(1, 0.5, 0.5), f(2, 0.7, 0.6)])
            .hold(30, &[f(1, 0.5, 0.5), f(2, 0.8, 0.6)])
            .hold(20, &[f(1, 0.5, 0.5)]);
        assert!(s.kinds().is_empty(), "{:?}", s.kinds());
    }

    #[test]
    fn two_extra_fingers_cancel_the_open_pie() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]);
        s.clear();
        s.hold(20, &[f(1, 0.5, 0.5), f(2, 0.7, 0.5), f(3, 0.8, 0.5)]);
        assert_eq!(s.kinds(), vec![TouchEvent::Cancel(Why::TooManyFingers)]);
    }

    #[test]
    fn lifting_the_holding_finger_while_the_extra_one_is_down_releases() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]);
        s.clear();
        s.hold(100, &[f(1, 0.5, 0.5), f(2, 0.7, 0.5)])
            .hold(20, &[f(2, 0.7, 0.5)]);
        assert_eq!(s.kinds(), vec![TouchEvent::Release]);
    }

    #[test]
    fn a_tap_then_a_hold_inside_the_double_click_window_is_flagged_double() {
        let mut s = Sim::new();
        s.hold(60, &[f(1, 0.5, 0.5)]).hold(10, &[]);
        assert!(s.kinds().is_empty(), "a tap alone says nothing");
        s.hold(100, &[]); // 110 ms gap < 260 ms
        s.hold(300, &[f(1, 0.5, 0.5)]);
        match s.kinds()[0] {
            TouchEvent::HoldArmed { double, .. } => assert!(double),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn a_hold_after_the_window_is_not_double() {
        let mut s = Sim::new();
        s.hold(60, &[f(1, 0.5, 0.5)]).hold(10, &[]);
        s.hold(400, &[]); // longer than the 260 ms window
        s.hold(300, &[f(1, 0.5, 0.5)]);
        match s.kinds()[0] {
            TouchEvent::HoldArmed { double, .. } => assert!(!double),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn a_long_touch_is_not_a_tap() {
        let mut s = Sim::new();
        s.hold(350, &[f(1, 0.5, 0.5)]).hold(10, &[]);
        s.clear();
        s.hold(50, &[]);
        s.hold(300, &[f(1, 0.5, 0.5)]);
        match s.kinds()[0] {
            TouchEvent::HoldArmed { double, .. } => assert!(!double),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn two_devices_the_first_to_arm_owns_the_gesture() {
        let mut r = Recognizer::new(Params {
            typing_guard: Duration::ZERO,
            ..Params::default()
        });
        let mut out = Vec::new();
        let touch = |device, t| Frame {
            device,
            time: MS(t),
            size_mm: (100.0, 100.0),
            fingers: &[Finger {
                id: 1,
                x: 0.5,
                y: 0.5,
                size: 1.0,
            }],
        };
        r.frame(&touch(1, 0), &mut out);
        r.frame(&touch(2, 5), &mut out);
        r.frame(&touch(1, 110), &mut out);
        r.frame(&touch(2, 115), &mut out);
        let armed_count = out.iter().filter(|e| armed(e)).count();
        assert_eq!(armed_count, 1, "{out:?}");
        assert!(r.active());
        // the device that lost does not release anything when it lifts
        out.clear();
        r.frame(
            &Frame {
                device: 2,
                time: MS(130),
                size_mm: (100.0, 100.0),
                fingers: &[],
            },
            &mut out,
        );
        assert!(out.is_empty(), "{out:?}");
        // the owner's lift does
        r.frame(
            &Frame {
                device: 1,
                time: MS(140),
                size_mm: (100.0, 100.0),
                fingers: &[],
            },
            &mut out,
        );
        assert_eq!(out, vec![TouchEvent::Release]);
        assert!(r.idle());
    }

    #[test]
    fn a_device_that_disappears_cancels_its_gesture() {
        let mut s = Sim::new();
        s.hold(300, &[f(1, 0.5, 0.5)]);
        s.clear();
        let mut ev = Vec::new();
        s.r.device_lost(1, &mut ev);
        assert_eq!(ev, vec![TouchEvent::Cancel(Why::DeviceLost)]);
        assert!(s.r.idle() && !s.r.active());
    }

    #[test]
    fn a_device_that_stops_sending_cancels_after_a_while() {
        let mut s = Sim::new();
        s.hold(300, &[f(1, 0.5, 0.5)]);
        let mut ev = Vec::new();
        s.r.poll(MS(s.t + 100), &mut ev);
        assert!(ev.is_empty());
        s.r.poll(MS(s.t) + STALE_AFTER + MS(1), &mut ev);
        assert_eq!(ev, vec![TouchEvent::Cancel(Why::Stale)]);
        assert!(!s.r.active());
    }

    #[test]
    fn reset_cancels_a_running_gesture_and_forgets_the_taps() {
        let mut s = Sim::new();
        s.hold(600, &[f(1, 0.5, 0.5)]);
        let mut ev = Vec::new();
        s.r.reset(&mut ev);
        assert_eq!(ev, vec![TouchEvent::Cancel(Why::DeviceLost)]);
        assert!(s.r.idle() && s.r.count() == 0);
    }

    #[test]
    fn a_slow_frame_rate_still_arms_and_opens_in_one_frame() {
        let mut r = Recognizer::new(Params {
            typing_guard: Duration::ZERO,
            ..Params::default()
        });
        let mut out = Vec::new();
        for t in [0u64, 700] {
            r.frame(
                &Frame {
                    device: 1,
                    time: MS(t),
                    size_mm: (100.0, 100.0),
                    fingers: &[f(1, 0.5, 0.5)],
                },
                &mut out,
            );
        }
        assert!(armed(&out[0]) && out[1] == TouchEvent::HoldOpen, "{out:?}");
    }

    #[test]
    fn an_unknown_surface_size_uses_the_default() {
        let mut r = Recognizer::new(Params {
            typing_guard: Duration::ZERO,
            ..Params::default()
        });
        let mut out = Vec::new();
        for (t, x) in [(0u64, 0.5f32), (110, 0.5), (200, 0.51)] {
            r.frame(
                &Frame {
                    device: 1,
                    time: MS(t),
                    size_mm: (0.0, 0.0),
                    fingers: &[f(1, x, 0.5)],
                },
                &mut out,
            );
        }
        // 0.01 of 130 mm = 1.3 mm: still
        assert!(
            out.iter().all(|e| !matches!(e, TouchEvent::Cancel(_))),
            "{out:?}"
        );
    }
}
