//! The virtual trackpad, a developer tool: a trackpad that floats in the middle of the screen, for
//! trying the trackpad gestures with a plain mouse. Run it with `cargo tools trackpad`.
//!
//! The real pointer is the *finger*: while it is over the glass it touches the surface, leaving
//! the glass lifts it. A *virtual pointer*, drawn on the screen outside the trackpad, is what the
//! finger moves, relatively and with acceleration like a real trackpad does (a slide of the finger
//! of so many millimeters moves it so far, see [`pointer_delta`]). The pie follows the virtual
//! pointer: it opens where it is, aims with it, and the app it acts on is the one under it (see
//! [`platform::pointer_pos`](crate::platform::pointer_pos)).
//!
//! It is a *source of frames* for the same recognizer a real trackpad feeds (see
//! [`tap::virtual_feed`](crate::tap::virtual_feed)), so everything a trackpad does here is
//! decided by the code that decides it for a real one: a hold opens the pie, moving the finger aims
//! it, lifting the finger runs the slice, the extra click is a click of the mouse (and a click of
//! the mouse with no pie open is a click at the virtual pointer, see below). This module is
//! only the finger, the pointer and what it feels like:
//!
//! | Part | Contents |
//! |---|---|
//! | `mod` (this file) | the pure model: finger, pointer motion, geometry, the cues and their fading (all tested) |
//! | [`draw`] | the trackpad as it looks, and the window's input |
//! | [`pointer`] | the window of the virtual pointer |
//!
//! # Feel
//!
//! # Clicks
//!
//! The mouse buttons are the *click* of the trackpad: the left button is a primary click and the
//! right button a secondary click, and both are made **at the virtual pointer** (see [`Clicker`]
//! and [`platform::click_at`](crate::platform::click_at)), not under the real pointer, which is
//! only the finger on the glass. A click that belongs to the pie (the pie opened while the button
//! was down, or the pie was already open) is never sent to the app below.
//!
//! A real trackpad answers a click with a haptic tick that a mouse cannot give. The virtual one
//! answers with what it can: the surface sinks a little, a ring spreads from the finger, the
//! frame glows, and a short click sounds. The ticks of the gesture itself (the pie opened, the
//! slice changed, the mode switched) are shown the same way, and are also sent to the system's
//! haptic engine, which a finger on a real trackpad does feel.

pub mod draw;
pub mod pointer;

use crate::platform::HapticKind;
use crate::tap::TouchFinger;
use eframe::egui::{Pos2, Rect, Vec2};
use std::time::{Duration, Instant};

/// How long the window may lose sight of the pointer, with a finger down, before the finger counts
/// as lifted. A click is sent to the virtual pointer by moving the system's pointer there and
/// back, and for a moment the window sees no pointer at all: that is not a lifted finger.
pub const LOST_GRACE: Duration = Duration::from_millis(150);

/// Height of the surface over its width: a 160 x 100 mm trackpad.
pub const ASPECT: f32 = 0.625;
/// What the surface stands for, in millimeters: the recognizer measures a still finger in mm.
pub const SURFACE_MM: (f32, f32) = (160.0, 100.0);
/// Fingers closer than this to the edge (0..1) are held back from it: the recognizer treats an edge
/// contact as a palm, and a mouse that slides past the edge is not meant as one.
pub const EDGE_KEEP: f32 = 0.04;
/// The size of a fingertip contact, as a trackpad reports it (a palm is above 2.5).
pub const FINGER_SIZE: f32 = 1.0;
/// How long a ring takes to spread and fade.
pub const RIPPLE_LIFE: Duration = Duration::from_millis(650);
/// How long the glow of a cue takes to fade.
pub const PULSE_LIFE: Duration = Duration::from_millis(280);

/// The id of the finger the mouse is.
const FINGER_ID: i32 = 1;

/// Where a pointer position lands on the surface: 0..1 on both axes, held off the edge.
pub fn norm(pos: Pos2, surface: Rect) -> (f32, f32) {
    let w = surface.width().max(1.0);
    let h = surface.height().max(1.0);
    (
        ((pos.x - surface.min.x) / w).clamp(EDGE_KEEP, 1.0 - EDGE_KEEP),
        ((pos.y - surface.min.y) / h).clamp(EDGE_KEEP, 1.0 - EDGE_KEEP),
    )
}

/// The finger the recognizer is given for a position on the surface.
pub fn finger_at(pos: Pos2, surface: Rect) -> TouchFinger {
    let (x, y) = norm(pos, surface);
    TouchFinger {
        id: FINGER_ID,
        x,
        y,
        size: FINGER_SIZE,
    }
}

/// What the window sees of the pointer in one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pointer {
    /// Where the pointer is over the window, in window points (`None` when it is not over it).
    pub pos: Option<Pos2>,
    /// The primary button went down in this frame.
    pub pressed: bool,
    /// The primary button is down.
    pub down: bool,
}

/// What the tracker asks for after a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// Nothing to tell the recognizer (no finger, none lifted).
    Idle,
    /// A finger came onto the surface, at this place (0..1).
    Down(TouchFinger),
    /// The finger is on the surface (and moved, or keeps still: the recognizer wants frames either way).
    Hold(TouchFinger),
    /// The finger left the surface.
    Up,
}

/// Turns the pointer into a finger: over the glass it is a finger touching, off it the finger is
/// lifted. Nothing touches while the trackpad is `blocked`. A button held on the glass keeps the
/// finger on it (pressed against the edge) wherever the mouse goes, as a finger that clicks and
/// drags does not leave the surface. A press is not needed to touch: the click of a real trackpad
/// is the *click* of the mouse (see [`Clicker`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tracker {
    finger: Option<TouchFinger>,
    /// Since when the window has had no pointer at all while a finger was down.
    lost: Option<Instant>,
}

impl Tracker {
    /// The finger now, if one is touching.
    pub fn finger(&self) -> Option<TouchFinger> {
        self.finger
    }

    /// One frame. `surface` is where the glass is, `blocked` says touches are refused.
    pub fn step(&mut self, p: Pointer, surface: Rect, blocked: bool) -> Step {
        self.step_at(p, surface, blocked, Instant::now())
    }

    /// [`step`](Self::step) at a given time.
    pub fn step_at(&mut self, p: Pointer, surface: Rect, blocked: bool, now: Instant) -> Step {
        let on = match p.pos {
            _ if blocked => None,
            Some(pos) if surface.contains(pos) || (p.down && self.finger.is_some()) => {
                self.lost = None;
                Some(finger_at(pos, surface))
            }
            // no pointer over the window: a button still down keeps the finger where it was (some
            // systems stop reporting the pointer once it is outside the window), and without one
            // the finger stays for a moment, so a click's trip to the virtual pointer is no lift
            None if self.finger.is_some() => {
                if p.down {
                    self.lost = None;
                    self.finger
                } else {
                    let since = *self.lost.get_or_insert(now);
                    if now.saturating_duration_since(since) < LOST_GRACE {
                        self.finger
                    } else {
                        None
                    }
                }
            }
            _ => None,
        };
        match (self.finger, on) {
            (None, None) => Step::Idle,
            (None, Some(f)) => {
                self.finger = Some(f);
                Step::Down(f)
            }
            (Some(_), Some(f)) => {
                self.finger = Some(f);
                Step::Hold(f)
            }
            (Some(_), None) => {
                self.finger = None;
                self.lost = None;
                Step::Up
            }
        }
    }
}

/// Points of screen that one millimeter of finger travel moves the pointer, when the finger is slow.
pub const POINTS_PER_MM: f32 = 3.5;
/// The speed (mm/s) at which the acceleration has added about two thirds of its most.
const ACCEL_SPEED: f32 = 120.0;
/// The most the acceleration adds (a fast flick moves the pointer this much + 1 times as far).
const ACCEL_MAX: f32 = 3.0;
/// The time (s) over which the speed of the finger is averaged. The speed is what the gain is made
/// of, so a frame that comes a millisecond after the last one cannot make the pointer jump.
const SPEED_TAU: f32 = 0.04;
/// A gap between two frames longer than this (s) is not motion: the finger is taken as still.
const MAX_GAP: f32 = 0.1;

/// The gain of the pointer for a finger that moves at `speed` mm/s: [`POINTS_PER_MM`] when slow,
/// growing smoothly to `POINTS_PER_MM * (1 + ACCEL_MAX)`. A curve without corners, so a slide that
/// speeds up never feels a step.
pub fn gain(speed: f32) -> f32 {
    POINTS_PER_MM * (1.0 + ACCEL_MAX * (1.0 - (-speed.max(0.0) / ACCEL_SPEED).exp()))
}

/// The averaged speed (mm/s) after a frame in which the finger travelled `dist` mm in `dt` seconds.
/// The weight of a frame is its duration, so a burst of short frames counts as much as one long
/// frame of the same motion.
pub fn blend_speed(prev: f32, dist: f32, dt: f32) -> f32 {
    let dt = dt.max(1e-4);
    let k = 1.0 - (-dt / SPEED_TAU).exp();
    prev + (dist / dt - prev) * k
}

/// How far the virtual pointer moves, in screen points, for a finger that moved `(dx, dy)` of the
/// surface (0..1 on both axes) in `dt` seconds, with `prev_speed` the averaged speed so far (mm/s):
/// the travel in millimeters, times a gain that grows with the speed, as the pointer of a real
/// trackpad does. A slow slide is precise, a flick crosses the screen. Returns the move and the new
/// averaged speed.
pub fn pointer_delta(d: (f32, f32), dt: f32, prev_speed: f32) -> ((f32, f32), f32) {
    let mm = (d.0 * SURFACE_MM.0, d.1 * SURFACE_MM.1);
    let speed = blend_speed(prev_speed, mm.0.hypot(mm.1), dt);
    let g = gain(speed);
    ((mm.0 * g, mm.1 * g), speed)
}

/// Remembers where the finger was and how fast it goes, to know how far it moved.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mover {
    last: Option<(f32, f32)>,
    speed: f32,
}

impl Mover {
    /// The finger is at `now` (`None`: it is lifted) after `dt` seconds: how far the pointer
    /// moves. A finger that just landed does not move it, so lifting and putting it down again
    /// elsewhere never makes the pointer jump; neither does a long gap between two frames.
    pub fn step(&mut self, now: Option<(f32, f32)>, dt: f32) -> (f32, f32) {
        let moved = match (self.last, now) {
            (Some(a), Some(b)) if dt <= MAX_GAP => {
                let (delta, speed) = pointer_delta((b.0 - a.0, b.1 - a.1), dt, self.speed);
                self.speed = speed;
                delta
            }
            _ => {
                self.speed = 0.0;
                (0.0, 0.0)
            }
        };
        self.last = now;
        moved
    }
}

/// Moves `p` by `d` and keeps it inside `bounds` (the displays together).
pub fn move_pointer(p: (f64, f64), d: (f32, f32), bounds: Rect) -> (f64, f64) {
    (
        (p.0 + d.0 as f64).clamp(
            bounds.min.x as f64,
            (bounds.max.x - 1.0).max(bounds.min.x) as f64,
        ),
        (p.1 + d.1 as f64).clamp(
            bounds.min.y as f64,
            (bounds.max.y - 1.0).max(bounds.min.y) as f64,
        ),
    )
}

/// A mouse button the trackpad can click with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// The primary click.
    Left,
    /// The secondary click.
    Right,
}

impl Button {
    /// The button number the rest of CursorPie uses (macOS numbering: 0 left, 1 right).
    pub fn code(self) -> i64 {
        match self {
            Button::Left => 0,
            Button::Right => 1,
        }
    }
}

/// What the window saw of the buttons in one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub left_pressed: bool,
    pub left_released: bool,
    pub right_pressed: bool,
    pub right_released: bool,
}

/// A click the trackpad asks the system to make at the virtual pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Click {
    pub button: Button,
    /// 1 for a click, 2 for the second of a double click, 3 for the third.
    pub count: u32,
}

/// The button that went down on the glass and has not come up yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Held {
    button: Button,
    /// The press belongs to something else now (the pie opened under it): it is no click.
    spoiled: bool,
}

/// Turns the buttons of the mouse into clicks of the trackpad. A click is made when the button
/// comes **up**, at the virtual pointer, and only if the press began on the glass with the finger
/// down, no pie open and no Settings window, and no pie opened while the button was held (that
/// press was the pie's). A second click of the same button within the double-click time counts as
/// the second of a double click.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clicker {
    held: Option<Held>,
    last: Option<(Button, Instant, u32)>,
}

impl Clicker {
    /// One frame. `finger` says a finger touches the glass, `pie_open` that a pie is open and
    /// `blocked` that touches are refused. Returns the click to make, if the frame completed one.
    pub fn step(
        &mut self,
        b: Buttons,
        finger: bool,
        pie_open: bool,
        blocked: bool,
        now: Instant,
        double: Duration,
    ) -> Option<Click> {
        if blocked || !finger {
            self.held = None;
        }
        if pie_open && let Some(h) = &mut self.held {
            h.spoiled = true;
        }
        let mut out = None;
        let frame = [
            (Button::Left, b.left_pressed, b.left_released),
            (Button::Right, b.right_pressed, b.right_released),
        ];
        for (button, pressed, released) in frame {
            if pressed && self.held.is_none() && finger && !pie_open && !blocked {
                self.held = Some(Held {
                    button,
                    spoiled: false,
                });
            }
            if released
                && let Some(h) = self.held
                && h.button == button
            {
                self.held = None;
                if h.spoiled {
                    continue;
                }
                let count = match self.last {
                    Some((last, at, n))
                        if last == button && now.saturating_duration_since(at) <= double =>
                    {
                        (n + 1).min(3)
                    }
                    _ => 1,
                };
                self.last = Some((button, now, count));
                out = Some(Click { button, count });
            }
        }
        out
    }
}

/// Where the trackpad's window goes: exactly in the middle of the display.
pub fn centered(display: Rect, size: Vec2) -> Pos2 {
    display.center() - size * 0.5
}

/// Where the virtual pointer starts: in the middle of the display above the trackpad, in plain view.
pub fn pointer_start(display: Rect, window: Rect) -> (f64, f64) {
    let y = display.min.y + (window.min.y - display.min.y).max(0.0) * 0.5;
    (display.center().x as f64, y as f64)
}

/// The smallest and the largest width of the trackpad, in points.
pub const WIDTHS: (f32, f32) = (240.0, 640.0);
/// The width it opens with.
pub const DEFAULT_WIDTH: f32 = 360.0;

/// How the tool was asked to open (`cargo tools trackpad` sets the environment).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub width: f32,
    pub sound: bool,
}

/// Reads the options from an environment: `CURSORPIE_VTRACKPAD` turns the tool on (anything but
/// empty, `0`, `false`), `CURSORPIE_VTRACKPAD_WIDTH` is the width in points (clamped to
/// [`WIDTHS`]), `CURSORPIE_VTRACKPAD_SOUND=0` silences the click.
pub fn options_from(get: impl Fn(&str) -> Option<String>) -> Option<Options> {
    let on = get("CURSORPIE_VTRACKPAD")
        .is_some_and(|v| !matches!(v.trim(), "" | "0" | "false" | "off" | "no"));
    if !on {
        return None;
    }
    let width = get("CURSORPIE_VTRACKPAD_WIDTH")
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|w| w.is_finite())
        .map_or(DEFAULT_WIDTH, |w| w.clamp(WIDTHS.0, WIDTHS.1));
    let sound = get("CURSORPIE_VTRACKPAD_SOUND").is_none_or(|v| v.trim() != "0");
    Some(Options { width, sound })
}

/// The options of this process.
pub fn options() -> Option<Options> {
    options_from(|k| std::env::var(k).ok())
}

/// What the trackpad shows and sounds as a cue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cue {
    /// The finger went down: the click.
    Down,
    /// The finger was lifted: a softer click.
    Up,
    /// A tick of the gesture (what the haptic engine is asked for).
    Tick(HapticKind),
}

impl Cue {
    /// How strong the cue is, 0..1: the height of the glow and the weight of the ring.
    pub fn strength(self) -> f32 {
        match self {
            Cue::Down => 0.9,
            Cue::Up => 0.45,
            Cue::Tick(HapticKind::LevelChange) => 1.0,
            Cue::Tick(HapticKind::Alignment) => 0.35,
            Cue::Tick(HapticKind::Generic) => 0.7,
        }
    }

    /// Whether the cue is also a click on the speaker: the click of the surface, and the firm
    /// confirmation. The soft ticks of hovering would be a machine gun.
    pub fn sounds(self) -> bool {
        matches!(
            self,
            Cue::Down | Cue::Up | Cue::Tick(HapticKind::LevelChange)
        )
    }
}

/// A ring that spreads from where a cue happened.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ripple {
    /// Where, 0..1 on the surface.
    pub at: (f32, f32),
    pub born: Instant,
    pub strength: f32,
}

/// What the trackpad remembers for drawing: the cues in flight and how far the surface is pressed.
#[derive(Clone, Debug, Default)]
pub struct Look {
    pub ripples: Vec<Ripple>,
    /// The glow of the latest cue: when, and how strong.
    pub pulse: Option<(Instant, f32)>,
    /// 0 (resting) to 1 (pressed all the way): follows the finger with a little inertia.
    pub press: f32,
}

impl Look {
    /// A cue happened at `at` (0..1; the middle when no finger is down).
    pub fn cue(&mut self, cue: Cue, at: Option<(f32, f32)>, now: Instant) {
        let s = cue.strength();
        self.pulse = Some((now, s));
        self.ripples.push(Ripple {
            at: at.unwrap_or((0.5, 0.5)),
            born: now,
            strength: s,
        });
        // a gesture that ticks fast must not pile rings up
        if self.ripples.len() > 8 {
            self.ripples.remove(0);
        }
    }

    /// Moves the animation on by `dt` seconds and forgets what has faded. True while something moves.
    pub fn step(&mut self, down: bool, dt: f32, now: Instant) -> bool {
        let target = if down { 1.0 } else { 0.0 };
        // an exponential approach: fast down, slower back up, like a spring-loaded surface
        let rate = if down { 40.0 } else { 14.0 };
        let k = 1.0 - (-rate * dt.max(0.0)).exp();
        self.press += (target - self.press) * k;
        if (self.press - target).abs() < 0.002 {
            self.press = target;
        }
        self.ripples
            .retain(|r| now.saturating_duration_since(r.born) < RIPPLE_LIFE);
        if let Some((t, _)) = self.pulse
            && now.saturating_duration_since(t) >= PULSE_LIFE
        {
            self.pulse = None;
        }
        self.press != target || !self.ripples.is_empty() || self.pulse.is_some()
    }

    /// The glow, 0..1, easing out.
    pub fn glow(&self, now: Instant) -> f32 {
        self.pulse.map_or(0.0, |(t, s)| {
            s * fade(now.saturating_duration_since(t), PULSE_LIFE)
        })
    }
}

/// 1 at birth, 0 at the end of `life`, easing out.
pub fn fade(age: Duration, life: Duration) -> f32 {
    let t = (age.as_secs_f32() / life.as_secs_f32()).clamp(0.0, 1.0);
    (1.0 - t) * (1.0 - t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn surface() -> Rect {
        Rect::from_min_size(pos2(20.0, 20.0), eframe::egui::vec2(400.0, 250.0))
    }

    fn p(x: f32, y: f32, pressed: bool, down: bool) -> Pointer {
        Pointer {
            pos: Some(pos2(x, y)),
            pressed,
            down,
        }
    }

    #[test]
    fn the_surface_stands_for_the_same_shape_as_its_millimeters() {
        assert!((SURFACE_MM.1 / SURFACE_MM.0 - ASPECT).abs() < 1e-6);
    }

    #[test]
    fn positions_map_to_the_unit_square_and_stay_off_the_edge() {
        let s = surface();
        assert_eq!(norm(s.center(), s), (0.5, 0.5));
        assert_eq!(norm(pos2(-500.0, 9000.0), s), (EDGE_KEEP, 1.0 - EDGE_KEEP));
        let f = finger_at(pos2(120.0, 20.0 + 125.0), s);
        assert!((f.x - 0.25).abs() < 1e-6 && (f.y - 0.5).abs() < 1e-6);
        assert!(f.size < crate::tap::TOUCH_PALM_SIZE);
    }

    #[test]
    fn the_pointer_over_the_glass_is_a_finger_and_leaving_lifts_it() {
        let (s, mut t) = (surface(), Tracker::default());
        let off = Pointer::default();
        assert_eq!(t.step(off, s, false), Step::Idle);
        // no press needed: being over the glass is touching it
        let down = t.step(p(200.0, 100.0, false, false), s, false);
        assert!(matches!(down, Step::Down(_)));
        let Step::Hold(f) = t.step(p(300.0, 150.0, false, false), s, false) else {
            panic!("expected a finger on the surface");
        };
        assert!(f.x > 0.6);
        // on the frame, or out of the window, is off the glass
        assert_eq!(t.step(p(5.0, 5.0, false, false), s, false), Step::Up);
        assert!(t.finger().is_none());
        assert_eq!(t.step(off, s, false), Step::Idle);
    }

    #[test]
    fn nothing_touches_while_blocked_and_blocking_lifts_the_finger() {
        let (s, mut t) = (surface(), Tracker::default());
        assert_eq!(t.step(p(200.0, 100.0, true, true), s, true), Step::Idle);
        assert!(t.finger().is_none());
        t.step(p(200.0, 100.0, false, false), s, false);
        assert!(t.finger().is_some());
        assert_eq!(t.step(p(200.0, 100.0, false, false), s, true), Step::Up);
    }

    /// The pointer's travel per millimeter of finger, over the last frames of a steady slide.
    fn travel_per_mm(mm_per_frame: f32, dt: f32) -> f32 {
        let mut m = Mover::default();
        let mut x = 0.2;
        m.step(Some((x, 0.5)), dt);
        let (mut sum, mut n) = (0.0, 0.0);
        for i in 0..10 {
            x += mm_per_frame / SURFACE_MM.0;
            let d = m.step(Some((x, 0.5)), dt);
            assert!(d.1 == 0.0);
            if i >= 5 {
                sum += d.0;
                n += 1.0;
            }
        }
        sum / (n * mm_per_frame)
    }

    #[test]
    fn the_pointer_moves_with_the_finger_and_faster_when_it_is_flicked() {
        // 16 ms frames: half a millimeter a frame is a slow slide, eight a flick
        let slow = travel_per_mm(0.5, 0.016);
        let flick = travel_per_mm(8.0, 0.016);
        // a slow slide is about POINTS_PER_MM per millimeter (a little more: it already accelerates)
        assert!(
            (POINTS_PER_MM..POINTS_PER_MM * 2.0).contains(&slow),
            "{slow}"
        );
        // a flick goes much further per millimeter: that is the acceleration, and it is bounded
        assert!(flick > slow * 1.8, "{flick} {slow}");
        assert!(flick <= POINTS_PER_MM * (1.0 + ACCEL_MAX) + 1e-3);
        // the same slide down is as far in millimeters, whatever the shape of the surface
        let mut m = Mover::default();
        m.step(Some((0.5, 0.2)), 0.016);
        let down = m.step(Some((0.5, 0.2 + 0.5 / SURFACE_MM.1)), 0.016);
        let mut m = Mover::default();
        m.step(Some((0.2, 0.5)), 0.016);
        let side = m.step(Some((0.2 + 0.5 / SURFACE_MM.0, 0.5)), 0.016);
        assert!((down.1 - side.0).abs() < 0.01, "{down:?} {side:?}");
        // no movement, no motion
        let mut m = Mover::default();
        m.step(Some((0.5, 0.5)), 0.016);
        assert_eq!(m.step(Some((0.5, 0.5)), 0.016), (0.0, 0.0));
    }

    #[test]
    fn the_gain_has_no_steps_and_no_ceiling_overshoot() {
        let mut last = gain(0.0);
        assert!((last - POINTS_PER_MM).abs() < 1e-6);
        for v in 1..2000 {
            let g = gain(v as f32);
            assert!(g >= last, "the gain never drops as the speed grows");
            assert!(g - last < 0.1, "no step at {v} mm/s");
            assert!(g <= POINTS_PER_MM * (1.0 + ACCEL_MAX));
            last = g;
        }
    }

    #[test]
    fn frames_that_come_close_together_do_not_make_the_pointer_jump() {
        // the same steady slide of 200 mm/s for a tenth of a second, seen in frames of 16 ms
        // and in bursts of 1 ms: the pointer ends up about as far in both
        let total = |dt: f32| {
            let mut m = Mover::default();
            let mut x = 0.1;
            m.step(Some((x, 0.5)), dt);
            let mut sum = 0.0;
            for _ in 0..(0.3 / dt) as usize {
                x += 200.0 * dt / SURFACE_MM.0;
                sum += m.step(Some((x, 0.5)), dt).0;
            }
            sum
        };
        let (steady, burst) = (total(0.016), total(0.001));
        assert!((steady - burst).abs() / steady < 0.12, "{steady} {burst}");
        // one frame 1 ms after another with a real step in it is bounded, not a spike
        let mut m = Mover::default();
        m.step(Some((0.5, 0.5)), 0.001);
        let d = m.step(Some((0.5 + 2.0 / SURFACE_MM.0, 0.5)), 0.001);
        assert!(d.0 < 0.7 * 2.0 * POINTS_PER_MM * (1.0 + ACCEL_MAX), "{d:?}");
    }

    #[test]
    fn a_finger_that_lands_again_elsewhere_does_not_make_the_pointer_jump() {
        let mut m = Mover::default();
        assert_eq!(m.step(Some((0.2, 0.2)), 0.016), (0.0, 0.0)); // landed
        let moved = m.step(Some((0.3, 0.2)), 0.016);
        assert!(moved.0 > 0.0);
        assert_eq!(m.step(None, 0.016), (0.0, 0.0)); // lifted
        assert_eq!(m.step(Some((0.9, 0.9)), 0.016), (0.0, 0.0)); // landed far away
        // a long pause between two frames is not motion either
        assert_eq!(m.step(Some((0.1, 0.1)), 0.5), (0.0, 0.0));
    }

    #[test]
    fn the_pointer_stays_on_the_screen() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), eframe::egui::vec2(1000.0, 600.0));
        assert_eq!(
            move_pointer((10.0, 10.0), (-500.0, -500.0), screen),
            (0.0, 0.0)
        );
        assert_eq!(
            move_pointer((990.0, 590.0), (500.0, 500.0), screen),
            (999.0, 599.0)
        );
        assert_eq!(
            move_pointer((100.0, 100.0), (5.0, -3.0), screen),
            (105.0, 97.0)
        );
    }

    #[test]
    fn the_trackpad_is_in_the_middle_and_the_pointer_starts_above_it() {
        let screen = Rect::from_min_size(pos2(100.0, 50.0), eframe::egui::vec2(1600.0, 900.0));
        let size = eframe::egui::vec2(440.0, 300.0);
        let at = centered(screen, size);
        assert_eq!(Rect::from_min_size(at, size).center(), screen.center());
        let start = pointer_start(screen, Rect::from_min_size(at, size));
        assert_eq!(start.0, screen.center().x as f64);
        assert!(start.1 < at.y as f64 && start.1 > screen.min.y as f64);
    }

    #[test]
    fn a_button_held_on_the_glass_keeps_the_finger_on_it_wherever_the_mouse_goes() {
        let (s, mut t) = (surface(), Tracker::default());
        let now = Instant::now();
        assert!(matches!(
            t.step_at(p(200.0, 100.0, true, true), s, false, now),
            Step::Down(_)
        ));
        // dragged far outside the glass with the button still down: the finger is at the edge
        let Step::Hold(f) = t.step_at(p(9000.0, 100.0, false, true), s, false, now) else {
            panic!("the finger must stay while the button is down");
        };
        assert_eq!(f.x, 1.0 - EDGE_KEEP);
        // the button is let go outside the glass: the finger lifts
        assert_eq!(
            t.step_at(p(9000.0, 100.0, false, false), s, false, now),
            Step::Up
        );
    }

    #[test]
    fn losing_the_pointer_for_a_moment_does_not_lift_the_finger() {
        let (s, mut t) = (surface(), Tracker::default());
        let t0 = Instant::now();
        t.step_at(p(200.0, 100.0, false, false), s, false, t0);
        let none = Pointer::default();
        let soon = t0 + LOST_GRACE / 2;
        assert!(matches!(t.step_at(none, s, false, soon), Step::Hold(_)));
        assert!(t.finger().is_some());
        // it comes back: the grace starts over the next time it is lost
        t.step_at(p(210.0, 100.0, false, false), s, false, soon);
        let later = soon + LOST_GRACE - Duration::from_millis(1);
        assert!(matches!(t.step_at(none, s, false, later), Step::Hold(_)));
        // a button still down keeps the finger, however long the pointer is not reported
        let held = Pointer {
            pos: None,
            pressed: false,
            down: true,
        };
        assert!(matches!(
            t.step_at(held, s, false, later + LOST_GRACE * 4),
            Step::Hold(_)
        ));
        // lost for good: the finger lifts after the grace
        let later = later + LOST_GRACE * 4;
        t.step_at(p(210.0, 100.0, false, false), s, false, later);
        let gone_from = later + Duration::from_millis(1);
        t.step_at(none, s, false, gone_from);
        let gone = gone_from + LOST_GRACE;
        assert_eq!(t.step_at(none, s, false, gone), Step::Up);
        // blocking lifts at once, grace or not
        let mut t = Tracker::default();
        t.step_at(p(200.0, 100.0, false, false), s, false, t0);
        assert_eq!(t.step_at(none, s, true, t0), Step::Up);
    }

    const DBL: Duration = Duration::from_millis(400);

    fn left(pressed: bool, released: bool) -> Buttons {
        Buttons {
            left_pressed: pressed,
            left_released: released,
            ..Buttons::default()
        }
    }

    fn right(pressed: bool, released: bool) -> Buttons {
        Buttons {
            right_pressed: pressed,
            right_released: released,
            ..Buttons::default()
        }
    }

    #[test]
    fn a_click_is_made_when_the_button_comes_up() {
        let (mut c, t0) = (Clicker::default(), Instant::now());
        assert_eq!(c.step(left(true, false), true, false, false, t0, DBL), None);
        let up = c.step(left(false, true), true, false, false, t0, DBL);
        assert_eq!(
            up,
            Some(Click {
                button: Button::Left,
                count: 1
            })
        );
        // a press and a release in one frame is a click too
        let fast = c.step(
            left(true, true),
            true,
            false,
            false,
            t0 + Duration::from_secs(5),
            DBL,
        );
        assert_eq!(fast.map(|k| k.count), Some(1));
        // the right button is the secondary click
        let r = c.step(right(true, true), true, false, false, t0, DBL);
        assert_eq!(r.map(|k| k.button), Some(Button::Right));
        assert_eq!(Button::Left.code(), 0);
        assert_eq!(Button::Right.code(), 1);
    }

    #[test]
    fn two_quick_clicks_are_a_double_click_and_a_slow_one_is_not() {
        let (mut c, t0) = (Clicker::default(), Instant::now());
        let mut click = |at: Duration, b: Buttons| c.step(b, true, false, false, t0 + at, DBL);
        let ms = Duration::from_millis;
        assert_eq!(click(ms(0), left(true, true)).map(|k| k.count), Some(1));
        assert_eq!(click(ms(200), left(true, true)).map(|k| k.count), Some(2));
        assert_eq!(click(ms(350), left(true, true)).map(|k| k.count), Some(3));
        assert_eq!(click(ms(360), left(true, true)).map(|k| k.count), Some(3));
        // too late, or the other button: a new count
        assert_eq!(click(ms(2000), left(true, true)).map(|k| k.count), Some(1));
        assert_eq!(click(ms(2100), right(true, true)).map(|k| k.count), Some(1));
    }

    #[test]
    fn a_press_that_belongs_to_the_pie_is_no_click() {
        let (mut c, t0) = (Clicker::default(), Instant::now());
        // a press while the pie is open
        assert_eq!(c.step(left(true, true), true, true, false, t0, DBL), None);
        // the pie opens while the button is held: its release is not a click
        c.step(left(true, false), true, false, false, t0, DBL);
        c.step(Buttons::default(), true, true, false, t0, DBL);
        let up = c.step(left(false, true), true, false, false, t0, DBL);
        assert_eq!(up, None);
        // and the next click is a click again
        let next = c.step(left(true, true), true, false, false, t0, DBL);
        assert!(next.is_some());
    }

    #[test]
    fn nothing_clicks_without_a_finger_or_while_blocked() {
        let (mut c, t0) = (Clicker::default(), Instant::now());
        assert_eq!(c.step(left(true, true), false, false, false, t0, DBL), None);
        assert_eq!(c.step(left(true, true), true, false, true, t0, DBL), None);
        // a press, then the finger is lost: no click at the release
        c.step(left(true, false), true, false, false, t0, DBL);
        c.step(Buttons::default(), false, false, false, t0, DBL);
        assert_eq!(c.step(left(false, true), true, false, false, t0, DBL), None);
    }

    #[test]
    fn the_options_come_from_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(options_from(env(&[])), None);
        assert_eq!(options_from(env(&[("CURSORPIE_VTRACKPAD", "0")])), None);
        assert_eq!(
            options_from(env(&[("CURSORPIE_VTRACKPAD", "1")])),
            Some(Options {
                width: DEFAULT_WIDTH,
                sound: true
            })
        );
        let o = options_from(env(&[
            ("CURSORPIE_VTRACKPAD", "1"),
            ("CURSORPIE_VTRACKPAD_WIDTH", "9000"),
            ("CURSORPIE_VTRACKPAD_SOUND", "0"),
        ]))
        .unwrap();
        assert_eq!((o.width, o.sound), (WIDTHS.1, false));
        let nan = options_from(env(&[
            ("CURSORPIE_VTRACKPAD", "1"),
            ("CURSORPIE_VTRACKPAD_WIDTH", "NaN"),
        ]))
        .unwrap();
        assert_eq!(nan.width, DEFAULT_WIDTH);
    }

    #[test]
    fn cues_light_the_glow_and_fade_away() {
        let mut look = Look::default();
        let t0 = Instant::now();
        look.cue(Cue::Down, Some((0.3, 0.4)), t0);
        assert!(look.glow(t0) > 0.85);
        assert!(look.glow(t0 + PULSE_LIFE / 2) < look.glow(t0));
        assert_eq!(look.glow(t0 + PULSE_LIFE), 0.0);
        assert!(look.step(false, 0.016, t0));
        assert!(!look.step(false, 0.016, t0 + RIPPLE_LIFE + Duration::from_millis(1)));
        assert!(look.ripples.is_empty() && look.pulse.is_none());
    }

    #[test]
    fn the_surface_sinks_with_the_finger_and_comes_back() {
        let mut look = Look::default();
        let now = Instant::now();
        for _ in 0..30 {
            look.step(true, 0.016, now);
        }
        assert!(look.press > 0.99);
        for _ in 0..60 {
            look.step(false, 0.016, now);
        }
        assert_eq!(look.press, 0.0);
    }

    #[test]
    fn rings_do_not_pile_up_and_only_firm_cues_click() {
        let mut look = Look::default();
        let now = Instant::now();
        for _ in 0..50 {
            look.cue(Cue::Tick(HapticKind::Alignment), None, now);
        }
        assert_eq!(look.ripples.len(), 8);
        assert!(Cue::Down.sounds() && Cue::Up.sounds());
        assert!(Cue::Tick(HapticKind::LevelChange).sounds());
        assert!(!Cue::Tick(HapticKind::Alignment).sounds());
        assert!(
            Cue::Tick(HapticKind::LevelChange).strength()
                > Cue::Tick(HapticKind::Alignment).strength()
        );
    }
}
