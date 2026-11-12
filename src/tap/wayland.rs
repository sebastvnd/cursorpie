//! Wayland event layer: the mouse is grabbed through evdev and everything the state machine does
//! not keep is written to a clone of it. See `docs/WAYLAND_SUPPORT.md`.
//!
//! ```text
//! physical mouse --EVIOCGRAB--> this thread --Ev--> tap::dispatch (unchanged)
//!                                    |                     |
//!                  Verdict::Keep: write to the clone       Verdict::Drop: write nothing
//! ```
//!
//! **The state machine is not touched** (decision D5): the same `Ev` values go in that the X11
//! layer sends, and the same `Verdict`s are obeyed. What differs from the X11 layer is only how a
//! verdict is carried out:
//!
//! * `Keep` writes the original event to the clone; `Drop` writes nothing. The X11 layer has to send
//!   a swallowed press *again*; here a press that is kept is simply never taken away.
//! * A click the state machine re-posts itself (`actions::post_mouse`: a quick click, a drag handed
//!   back, a replayed first click) goes to the clone of the mouse that is in use
//!   (`wayland::inject_button`), so the application sees one device press and release.
//! * Motion is always forwarded (the cursor must move) and also becomes `Kind::Drag` while the trigger
//!   is held. Its position is the compositor's answer when there is one (`kwin::pointer`), and the
//!   raw motion added up in between: decision D6 allows that between two answers only.
//!
//! **The engine** ([`Engine`]) is pure: it takes the events a mouse produced and returns the events
//! to write, asking an [`Env`] for everything outside (routing, the pointer). Recorded streams drive
//! it in the tests (this file is built on Linux only; the pure parts it relies on, `uinput` and
//! `wayland::kwin`, are tested on every OS).
//!
//! **Fail open** (risk R1). The kernel lets go of a grab when the descriptor is closed, so a crash or a
//! `kill -9` never leaves a dead mouse. A hang is covered by a watchdog thread: if the loop has not
//! shown a sign of life for [`STALL`], it releases every grab itself and the loop takes them back
//! when it wakes. A panic in the engine forwards the events untouched.
//!
//! **Also here.** Pausing while the screen is locked or the session is in the background (logind,
//! `wayland::logind`: the grabs are lifted like the pause switch does), and the touchpad's extra
//! click and scroll (the touch source grabs the pad while a hold runs, `touch/evdev.rs`). Keyboards
//! are never opened: a shortcut is recorded from the key events of the Settings window itself
//! (`ui/settings/wayland_keys.rs`).

use super::{Ev, Gesture, Kind, PAUSED, TAP_ACTIVE, Verdict, dispatch, touch};
use crate::config::{cfg, extra_buttons_in_use, lock};
use crate::geometry::Point;
use crate::mouse;
use crate::msg::Notifier;
use crate::platform::linux::evdev_io::{self, POLLIN, PollFd};
use crate::platform::linux::inputdev::{self, MouseNode};
use crate::platform::linux::session::Session;
use crate::platform::linux::uinput::{
    CloneSpec, EV_BYTES, EV_KEY, EV_MSC, EV_REL, EV_SYN, EVENT_SIZE, InputEvent, KEY_BYTES,
    MSC_BYTES, REL_BYTES, REL_X, REL_Y, SYN_DROPPED, SYN_REPORT, UinputDevice, button_for_code,
    decode_all,
};
use crate::platform::linux::wayland::{self, SharedSink, kwin};
use crate::platform::linux::x11;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::c_int;
use std::fs::File;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long the loop waits for events before it looks at the pause flag and the clock again.
const IDLE_WAIT_MS: c_int = 30;
/// How often the list of mice is read again (a mouse plugged in later is found).
const RESCAN: Duration = Duration::from_secs(1);
/// How often the settings (trigger, extra buttons) are read again.
const SETTINGS_EVERY: Duration = Duration::from_millis(250);
/// How long a new clone is left alone before its mouse is grabbed: the desktop opens a new device a
/// little after it appears, and what is written before that is lost.
const CLONE_SETTLE: Duration = Duration::from_millis(400);
/// How long a mouse that could not be taken is left alone.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// How long the loop may be silent before the watchdog lets go of the mice.
const STALL: Duration = Duration::from_millis(250);
/// Most reads of one mouse in one turn of the loop, so one chatty device cannot starve the rest.
const MAX_READS: usize = 64;

// ------------------------------------------------------------------------------------- the engine

/// What the engine asks of the program around it.
pub(super) trait Env {
    /// The state machine's answer for one event (a panic in it is a `Keep`).
    fn route(&self, ev: Ev) -> Verdict;
    /// The latest pointer position the compositor reported and its number (it grows with every
    /// answer).
    fn compositor_pointer(&self) -> Option<(u64, Point)>;
    /// The pointer for a press that happens now. With `wait` the compositor is asked and its
    /// answer awaited for a few milliseconds (the press of the trigger, which places the pie);
    /// without it this is the latest answer, as [`Env::compositor_pointer`] gives it.
    fn pointer_at_press(&self, wait: bool) -> Option<(u64, Point)>;
    /// A position to start from when the compositor has not answered yet.
    fn fallback_pointer(&self) -> Option<Point>;
    /// Asks for the pointer at full rate while a hold runs (`true`), or at the idle rate.
    fn arm(&self, on: bool);
    /// Settings is scanning the mouse for its extra buttons.
    fn scanning(&self) -> bool;
}

/// One more than the last button number (`mouse::MAX_BUTTON`).
const SLOTS: usize = 16;

fn slot(button: i64) -> Option<usize> {
    usize::try_from(button).ok().filter(|b| *b < SLOTS)
}

/// The part of the layer that is decided by the events alone.
pub(super) struct Engine {
    trigger: i64,
    extras: BTreeSet<i64>,
    /// The press of this button was let through, so its release must go too.
    forwarded: [bool; SLOTS],
    /// This button is physically down (only buttons the state machine looks at).
    held: [bool; SLOTS],
    /// Where the pointer is, as far as is known.
    pos: Option<Point>,
    /// The number of the last compositor answer used.
    seen: u64,
    /// The events of the frame that is being read.
    frame: Vec<InputEvent>,
    /// The kernel dropped events: the frame in progress and the next one are not trusted.
    dropped: bool,
}

impl Engine {
    pub(super) fn new(trigger: i64, extras: BTreeSet<i64>) -> Engine {
        Engine {
            trigger,
            extras,
            forwarded: [false; SLOTS],
            held: [false; SLOTS],
            pos: None,
            seen: 0,
            frame: Vec::new(),
            dropped: false,
        }
    }

    /// The buttons the state machine looks at changed (Settings).
    pub(super) fn set_buttons(&mut self, trigger: i64, extras: BTreeSet<i64>) {
        self.trigger = trigger;
        self.extras = extras;
    }

    /// Forgets everything about a gesture in progress (after a fault).
    pub(super) fn reset(&mut self) {
        self.forwarded = [false; SLOTS];
        self.held = [false; SLOTS];
        self.frame.clear();
        self.dropped = false;
    }

    /// Whether any routed button is physically down.
    #[cfg(test)]
    fn any_held(&self) -> bool {
        self.held.iter().any(|h| *h)
    }

    /// Takes the events a mouse produced (any number of frames, a partial last one is kept for the
    /// next call) and returns the events to write to the clone: whole frames, each ending in
    /// `SYN_REPORT`.
    pub(super) fn feed(&mut self, env: &dyn Env, events: &[InputEvent]) -> Vec<InputEvent> {
        let mut out = Vec::with_capacity(events.len());
        for e in events {
            match (e.ty, e.code) {
                (EV_SYN, SYN_REPORT) => self.end_frame(env, &mut out),
                (EV_SYN, SYN_DROPPED) => {
                    self.frame.clear();
                    self.dropped = true;
                }
                _ => self.frame.push(*e),
            }
        }
        out
    }

    fn end_frame(&mut self, env: &dyn Env, out: &mut Vec<InputEvent>) {
        let frame = std::mem::take(&mut self.frame);
        if std::mem::take(&mut self.dropped) {
            // events were lost: what the state machine believes about the buttons may be wrong
            self.resync(env);
            return;
        }
        if frame.is_empty() {
            return;
        }
        let mut kept = Vec::with_capacity(frame.len() + 1);
        let (mut dx, mut dy) = (0i64, 0i64);
        let mut swallowed = false;
        for e in &frame {
            match (e.ty, e.code) {
                (EV_REL, REL_X) => {
                    dx += i64::from(e.value);
                    kept.push(*e);
                }
                (EV_REL, REL_Y) => {
                    dy += i64::from(e.value);
                    kept.push(*e);
                }
                (EV_KEY, _) => {
                    if self.key(env, *e) {
                        kept.push(*e);
                    } else {
                        swallowed = true;
                    }
                }
                _ => kept.push(*e),
            }
        }
        if dx != 0 || dy != 0 {
            self.track(env, dx, dy);
            if let (Some(i), Some(pos)) = (slot(self.trigger), self.pos)
                && self.held[i]
            {
                env.route(Ev::Mouse {
                    kind: Kind::Drag,
                    button: self.trigger,
                    pos,
                });
            }
        }
        if swallowed {
            // the scan code that came with a swallowed button is of no use alone
            kept.retain(|e| e.ty != EV_MSC);
        }
        if !kept.is_empty() {
            kept.push(InputEvent::SYN);
            out.extend(kept);
        }
    }

    /// Where the pointer is at the press of a routed button.
    ///
    /// What the engine adds up is only the motion of *this* mouse since the last answer: after the
    /// pointer was moved by a touchpad or another mouse, or after fast motion that the compositor's
    /// throttled reports did not cover, it is somewhere else, and so is the pie opened at it. So
    /// the press takes the compositor's answer whenever there is one the engine has not used yet,
    /// and for the trigger (which places the pie) it asks for a fresh one first. The pointer of the
    /// X server is the last resort: in a Wayland session it only knows the pointer while it is
    /// over an X window.
    fn anchor(&mut self, env: &dyn Env, wait: bool) {
        if let Some((seq, p)) = env.pointer_at_press(wait)
            && seq != self.seen
        {
            self.seen = seq;
            self.pos = Some(p);
        }
        if self.pos.is_none() {
            self.pos = env.fallback_pointer();
        }
    }

    /// Follows the pointer: the compositor's latest answer when it is a new one, otherwise the
    /// motion of the frame added to what is known.
    fn track(&mut self, env: &dyn Env, dx: i64, dy: i64) {
        if let Some((seq, p)) = env.compositor_pointer()
            && seq != self.seen
        {
            self.seen = seq;
            self.pos = Some(p);
        } else if let Some(p) = self.pos.as_mut() {
            p.x += dx as f64;
            p.y += dy as f64;
        }
    }

    /// Whether the state machine looks at `button`: the trigger, the two side buttons, the extra buttons
    /// a mode uses (the X11 layer grabs the same ones), and every extra button while Settings scans.
    fn is_routed(&self, env: &dyn Env, button: i64) -> bool {
        button == self.trigger
            || button == mouse::BACK
            || button == mouse::FORWARD
            || self.extras.contains(&button)
            || (mouse::is_extra(button) && env.scanning())
    }

    /// One button or key event of a frame. Returns whether to write it to the clone.
    fn key(&mut self, env: &dyn Env, e: InputEvent) -> bool {
        let Some(button) = button_for_code(e.code) else {
            return true; // a key on a mouse node: not ours
        };
        let Some(i) = slot(button).filter(|_| self.is_routed(env, button)) else {
            return true;
        };
        match e.value {
            1 => {
                let is_trigger = button == self.trigger;
                // first: the compositor then answers while the position is being decided
                if is_trigger {
                    env.arm(true);
                }
                self.anchor(env, is_trigger);
                let pos = self.pos.unwrap_or_default();
                let keep = env.route(Ev::Mouse {
                    kind: Kind::Down,
                    button,
                    pos,
                }) == Verdict::Keep;
                self.held[i] = true;
                self.forwarded[i] = keep;
                keep
            }
            0 => {
                let pos = self.pos.unwrap_or_default();
                if button == self.trigger {
                    env.arm(false);
                }
                let verdict = env.route(Ev::Mouse {
                    kind: Kind::Up,
                    button,
                    pos,
                });
                // the trigger's release also goes when the state machine handed the press back to the
                // app (`Passthrough`): the press was written by `post_mouse`, the release is ours
                let keep =
                    self.forwarded[i] || (button == self.trigger && verdict == Verdict::Keep);
                self.held[i] = false;
                self.forwarded[i] = false;
                keep
            }
            // a repeat: follows the press
            _ => self.forwarded[i],
        }
    }

    /// After lost events: lets the state machine see every button that was down go up.
    fn resync(&mut self, env: &dyn Env) {
        for i in 0..SLOTS {
            if self.held[i] {
                let pos = self.pos.unwrap_or_default();
                env.route(Ev::Mouse {
                    kind: Kind::Up,
                    button: i as i64,
                    pos,
                });
            }
        }
        self.held = [false; SLOTS];
        self.forwarded = [false; SLOTS];
    }
}

// -------------------------------------------------------------------------------------- the loop

/// The state machine, as the engine's [`Env`].
struct PipeEnv {
    sink: SharedSink,
    state: Arc<Mutex<Gesture>>,
    notifier: Notifier,
}

impl Env for PipeEnv {
    fn route(&self, ev: Ev) -> Verdict {
        if matches!(ev, Ev::Mouse { kind, .. } if kind != Kind::Drag) {
            // set before the state machine runs: a click it re-posts must go to this mouse
            wayland::set_current_sink(&self.sink);
        }
        catch_unwind(AssertUnwindSafe(|| {
            dispatch(&self.state, &self.notifier, ev, &|| {})
        }))
        .unwrap_or(Verdict::Keep)
    }

    fn compositor_pointer(&self) -> Option<(u64, Point)> {
        kwin::pointer()
    }

    fn pointer_at_press(&self, wait: bool) -> Option<(u64, Point)> {
        let started = Instant::now();
        let answer = if wait {
            kwin::sample_pointer()
        } else {
            kwin::pointer()
        };
        diag::log!(
            "tap",
            "press_pointer",
            wait = wait,
            answered = answer.is_some(),
            took_ms = started.elapsed()
        );
        answer
    }

    fn fallback_pointer(&self) -> Option<Point> {
        let (x, y) = x11::conn()?.pointer()?;
        Some(Point::new(x, y))
    }

    fn arm(&self, on: bool) {
        kwin::arm(on);
    }

    fn scanning(&self) -> bool {
        mouse::scanning()
    }
}

/// One mouse: its node, its clone, and the engine between them.
struct Pipe {
    node: String,
    name: String,
    file: File,
    fd: c_int,
    sink: SharedSink,
    engine: Engine,
    env: PipeEnv,
    /// When to grab it (after the clone has settled); `None` once it is grabbed.
    grab_at: Option<Instant>,
    grabbed: bool,
}

/// The descriptors that are grabbed right now: what the watchdog lets go of.
static GRABBED: Mutex<Vec<c_int>> = Mutex::new(Vec::new());
/// The loop's last sign of life, in milliseconds on [`clock`].
static BEAT: AtomicU64 = AtomicU64::new(0);
/// The watchdog let go of the mice: the loop must take them back.
static TRIPPED: AtomicBool = AtomicBool::new(false);

fn clock() -> &'static Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    clock().elapsed().as_millis() as u64
}

fn beat() {
    BEAT.store(now_ms(), Ordering::Relaxed);
}

/// Lets go of every grab if the loop has been silent for [`STALL`].
fn watchdog() {
    loop {
        std::thread::sleep(Duration::from_millis(50));
        let fds = lock(&GRABBED);
        let silent = now_ms().saturating_sub(BEAT.load(Ordering::Relaxed));
        if !fds.is_empty() && silent > STALL.as_millis() as u64 {
            for fd in fds.iter() {
                evdev_io::grab(*fd, false);
            }
            TRIPPED.store(true, Ordering::SeqCst);
            diag::log!("tap", "watchdog_released", silent_ms = silent);
        }
    }
}

impl Pipe {
    fn grab(&mut self) -> bool {
        if self.grabbed {
            return true;
        }
        if !evdev_io::grab(self.fd, true) {
            return false;
        }
        self.grabbed = true;
        self.grab_at = None;
        lock(&GRABBED).push(self.fd);
        true
    }

    /// Writes the releases the clone still owes, then lets go of the mouse.
    fn ungrab(&mut self) {
        self.release_sink();
        if self.grabbed {
            lock(&GRABBED).retain(|fd| *fd != self.fd);
            evdev_io::grab(self.fd, false);
            self.grabbed = false;
        }
        self.engine.reset();
    }

    fn release_sink(&self) {
        let mut s = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        let _ = s.release_all();
    }

    /// The watchdog let go of the mouse behind our back.
    fn lost(&mut self) {
        self.grabbed = false;
        self.release_sink();
        self.engine.reset();
        self.grab_at = Some(Instant::now());
    }

    /// Runs the engine on what was read and writes the result. `Err` if the clone cannot be
    /// written (the pipe is then dropped, which lets go of the mouse).
    fn handle(&mut self, events: &[InputEvent]) -> Result<(), ()> {
        let out = match catch_unwind(AssertUnwindSafe(|| self.engine.feed(&self.env, events))) {
            Ok(out) => out,
            Err(_) => {
                // a fault in the engine: the events go through untouched
                self.engine.reset();
                let mut raw = events.to_vec();
                if raw.last().is_none_or(|e| *e != InputEvent::SYN) {
                    raw.push(InputEvent::SYN);
                }
                raw
            }
        };
        if out.is_empty() {
            return Ok(());
        }
        let mut s = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        s.emit(&out).map_err(|_| ())
    }

    /// Reads what is waiting. `Err` if the mouse is gone or the clone broke.
    fn read(&mut self) -> Result<(), ()> {
        let mut buf = [0u8; EVENT_SIZE * 64];
        for _ in 0..MAX_READS {
            match (&self.file).read(&mut buf) {
                Ok(0) => return Err(()),
                Ok(n) => {
                    let events = decode_all(&buf[..n]);
                    self.handle(&events)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(()),
            }
        }
        Ok(())
    }
}

impl Drop for Pipe {
    fn drop(&mut self) {
        self.ungrab();
        wayland::forget_sink(&self.sink);
        diag::log!(
            "tap",
            "mouse_released",
            node = self.node.as_str(),
            name = self.name.as_str()
        );
    }
}

struct Hub {
    state: Arc<Mutex<Gesture>>,
    notifier: Notifier,
    pipes: Vec<Pipe>,
    cooldown: HashMap<String, Instant>,
    said: HashSet<String>,
    lifted: bool,
    last_scan: Option<Instant>,
    last_settings: Option<Instant>,
}

impl Hub {
    fn say_once(&mut self, text: String) {
        if self.said.insert(text.clone()) {
            eprintln!("CursorPie: {text}");
        }
    }

    /// Makes a pipe for a mouse: opens it, describes it, makes its clone. The mouse is grabbed
    /// later, once the clone has settled.
    fn open(&self, n: &MouseNode) -> Result<Pipe, String> {
        let path = format!("/dev/input/{}", n.node);
        let file = evdev_io::open_node(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => wayland::current_report().text,
            _ => format!("could not open {path}: {e}"),
        })?;
        let fd = evdev_io::raw_fd(&file);
        let name = evdev_io::device_name(fd).unwrap_or_else(|| n.name.clone());
        let id = evdev_io::device_id(fd).ok_or("could not read the ids of the mouse")?;
        let bits = |ev: u8, bytes: usize| {
            evdev_io::event_bits(fd, ev, bytes).ok_or("could not read what the mouse can do")
        };
        let types = bits(0, EV_BYTES)?;
        let keys = bits(EV_KEY as u8, KEY_BYTES)?;
        let rels = bits(EV_REL as u8, REL_BYTES)?;
        let msc = bits(EV_MSC as u8, MSC_BYTES).unwrap_or_default();
        let spec = CloneSpec::of_mouse(&name, id, &types, &keys, &rels, &msc)
            .map_err(|why| format!("{name} {why}: left alone"))?;
        let clone = UinputDevice::create(&spec).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound => {
                wayland::current_report().text
            }
            _ => format!("could not make a virtual mouse: {e}"),
        })?;
        let sink: SharedSink = Arc::new(Mutex::new(clone));
        let (trigger, extras) = {
            let c = cfg();
            (c.trigger_button, extra_buttons_in_use(&c))
        };
        diag::log!(
            "tap",
            "mouse_found",
            node = n.node.as_str(),
            name = name.as_str()
        );
        Ok(Pipe {
            node: n.node.clone(),
            name,
            file,
            fd,
            engine: Engine::new(trigger, extras),
            env: PipeEnv {
                sink: sink.clone(),
                state: self.state.clone(),
                notifier: self.notifier.clone(),
            },
            sink,
            grab_at: Some(Instant::now() + CLONE_SETTLE),
            grabbed: false,
        })
    }

    /// Looks at the mice the kernel lists: new ones get a pipe, gone ones lose theirs.
    fn rescan(&mut self) {
        let Ok(text) = std::fs::read_to_string("/proc/bus/input/devices") else {
            return;
        };
        let nodes = inputdev::parse_mouse_nodes(&text);
        self.pipes
            .retain(|p| nodes.iter().any(|n| n.node == p.node));
        let now = Instant::now();
        self.cooldown.retain(|_, until| *until > now);
        for n in &nodes {
            if self.pipes.iter().any(|p| p.node == n.node) || self.cooldown.contains_key(&n.node) {
                continue;
            }
            match self.open(n) {
                Ok(pipe) => self.pipes.push(pipe),
                Err(why) => {
                    self.cooldown.insert(n.node.clone(), now + RETRY_AFTER);
                    self.say_once(why);
                }
            }
        }
    }

    /// The watchdog let go of the mice: they are taken back.
    fn recover(&mut self) {
        if TRIPPED.swap(false, Ordering::SeqCst) {
            lock(&GRABBED).clear();
            for p in &mut self.pipes {
                p.lost();
            }
        }
    }

    fn housekeeping(&mut self) {
        let now = Instant::now();
        // the pause switch: every mouse goes back to the desktop untouched
        // (and the lock screen, or a session in the background: `wayland::logind`)
        let paused = PAUSED.load(Ordering::SeqCst) || wayland::logind::is_away();
        if paused != self.lifted {
            for p in &mut self.pipes {
                if paused {
                    p.ungrab();
                } else {
                    p.grab_at = Some(now);
                }
            }
            self.lifted = paused;
        }
        if self
            .last_scan
            .is_none_or(|t| now.duration_since(t) >= RESCAN)
        {
            self.last_scan = Some(now);
            self.rescan();
        }
        if !self.lifted {
            // the mice whose clone has settled are taken; one that cannot be taken is dropped
            let mut failed = Vec::new();
            for p in &mut self.pipes {
                if !p.grabbed && p.grab_at.is_some_and(|t| t <= now) && !p.grab() {
                    failed.push(p.node.clone());
                }
            }
            for node in failed {
                self.pipes.retain(|p| p.node != node);
                self.cooldown.insert(node.clone(), now + RETRY_AFTER);
                self.say_once(format!(
                    "could not grab {node} (another program may hold it); trying again later"
                ));
            }
        }
        if self
            .last_settings
            .is_none_or(|t| now.duration_since(t) >= SETTINGS_EVERY)
        {
            self.last_settings = Some(now);
            let (trigger, extras) = {
                let c = cfg();
                (c.trigger_button, extra_buttons_in_use(&c))
            };
            for p in &mut self.pipes {
                p.engine.set_buttons(trigger, extras.clone());
            }
            TAP_ACTIVE.store(self.pipes.iter().any(|p| p.grabbed), Ordering::SeqCst);
        }
    }

    /// Waits for events on the grabbed mice and handles them.
    fn pump(&mut self) {
        let live: Vec<usize> = (0..self.pipes.len())
            .filter(|i| self.pipes[*i].grabbed)
            .collect();
        if live.is_empty() {
            std::thread::sleep(Duration::from_millis(IDLE_WAIT_MS as u64));
            return;
        }
        let mut fds: Vec<PollFd> = live
            .iter()
            .map(|i| PollFd {
                fd: self.pipes[*i].fd,
                events: POLLIN,
                revents: 0,
            })
            .collect();
        if evdev_io::poll_fds(&mut fds, IDLE_WAIT_MS) <= 0 {
            return;
        }
        let mut gone = Vec::new();
        for (k, i) in live.iter().enumerate() {
            if fds[k].revents != 0 && self.pipes[*i].read().is_err() {
                gone.push(self.pipes[*i].node.clone());
            }
        }
        for node in gone {
            // unplugged, or its clone broke: let go now, the next scan finds it again if it is back
            self.pipes.retain(|p| p.node != node);
        }
    }
}

/// Starts the layer on its own thread.
pub fn spawn(notifier: Notifier) {
    let session = Session::current();
    let report = wayland::current_report();
    diag::log!(
        "tap",
        "session",
        server = format!("{:?}", session.server),
        compositor = session.compositor().name(),
        ready = report.ready,
        per_app = report.per_app
    );
    if !report.ready {
        eprintln!("CursorPie: {}", report.text);
    }
    x11::warm_index();
    let state = Arc::new(Mutex::new(Gesture::Idle));
    touch::start(state.clone(), notifier.clone());
    beat();
    let started = std::thread::Builder::new()
        .name("cursorpie-wayland-tap".into())
        .spawn(move || {
            wayland::prepare();
            kwin::start();
            wayland::logind::start();
            let _ = std::thread::Builder::new()
                .name("cursorpie-watchdog".into())
                .spawn(watchdog);
            let mut hub = Hub {
                state,
                notifier,
                pipes: Vec::new(),
                cooldown: HashMap::new(),
                said: HashSet::new(),
                lifted: false,
                last_scan: None,
                last_settings: None,
            };
            diag::log!("tap", "hook", backend = "wayland", state = "running");
            loop {
                beat();
                hub.recover();
                hub.housekeeping();
                hub.pump();
            }
        });
    if let Err(e) = started {
        eprintln!("CursorPie: could not start the mouse tap: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::linux::uinput::{BTN_LEFT, BTN_MIDDLE, BTN_SIDE};
    use std::cell::{Cell, RefCell};

    const MIDDLE: u16 = BTN_MIDDLE;
    const REL_WHEEL: u16 = 8;
    const REL_WHEEL_HI_RES: u16 = 11;

    /// The state machine, replaced by a rule, with a record of what it was asked.
    struct Fake {
        log: RefCell<Vec<Ev>>,
        armed: RefCell<Vec<bool>>,
        rule: Box<dyn Fn(&Ev) -> Verdict>,
        pointer: Cell<Option<(u64, Point)>>,
        /// What the compositor answers when it is asked at a press (the latest one if `None`).
        fresh: Cell<Option<(u64, Point)>>,
        /// For each press that asked for the pointer: whether it waited for a fresh answer.
        asked: RefCell<Vec<bool>>,
        scanning: Cell<bool>,
    }

    impl Fake {
        fn new(rule: impl Fn(&Ev) -> Verdict + 'static) -> Fake {
            Fake {
                log: RefCell::new(Vec::new()),
                armed: RefCell::new(Vec::new()),
                rule: Box::new(rule),
                pointer: Cell::new(None),
                fresh: Cell::new(None),
                asked: RefCell::new(Vec::new()),
                scanning: Cell::new(false),
            }
        }

        /// The trigger is swallowed like the state machine does while it decides.
        fn swallowing() -> Fake {
            Fake::new(|_| Verdict::Drop)
        }

        fn events(&self) -> Vec<Ev> {
            self.log.borrow().clone()
        }
    }

    impl Env for Fake {
        fn route(&self, ev: Ev) -> Verdict {
            self.log.borrow_mut().push(ev);
            (self.rule)(&ev)
        }
        fn compositor_pointer(&self) -> Option<(u64, Point)> {
            self.pointer.get()
        }
        fn pointer_at_press(&self, wait: bool) -> Option<(u64, Point)> {
            self.asked.borrow_mut().push(wait);
            if wait {
                self.fresh.get().or(self.pointer.get())
            } else {
                self.pointer.get()
            }
        }
        fn fallback_pointer(&self) -> Option<Point> {
            Some(Point::new(100.0, 200.0))
        }
        fn arm(&self, on: bool) {
            self.armed.borrow_mut().push(on);
        }
        fn scanning(&self) -> bool {
            self.scanning.get()
        }
    }

    fn engine() -> Engine {
        Engine::new(2, BTreeSet::new())
    }

    fn rel(code: u16, v: i32) -> InputEvent {
        InputEvent::new(EV_REL, code, v)
    }
    fn btn(code: u16, down: bool) -> InputEvent {
        InputEvent::key(code, down)
    }
    fn scan() -> InputEvent {
        InputEvent::new(EV_MSC, 4, 0x90002)
    }
    const SYN: InputEvent = InputEvent::SYN;

    fn kind_of(ev: &Ev) -> Kind {
        match ev {
            Ev::Mouse { kind, .. } => *kind,
            Ev::Key { .. } => panic!("a key"),
        }
    }

    #[test]
    fn motion_and_every_wheel_code_pass_through_untouched() {
        let fake = Fake::swallowing();
        let mut e = engine();
        let input = [
            rel(REL_X, 5),
            rel(REL_Y, -3),
            rel(REL_WHEEL, 1),
            rel(REL_WHEEL_HI_RES, 120),
            rel(6, 1),
            rel(12, 120),
            SYN,
        ];
        assert_eq!(
            e.feed(&fake, &input),
            input,
            "event by event, so nothing doubles or stops"
        );
        assert!(
            fake.events().is_empty(),
            "the state machine hears nothing: no button is held"
        );
    }

    #[test]
    fn the_left_and_right_button_are_never_routed_and_always_pass() {
        let fake = Fake::swallowing();
        let mut e = engine();
        let input = [btn(BTN_LEFT, true), SYN, btn(BTN_LEFT, false), SYN];
        assert_eq!(e.feed(&fake, &input), input);
        assert!(fake.events().is_empty());
    }

    #[test]
    fn a_swallowed_trigger_is_not_written_and_takes_its_scan_code_with_it() {
        let fake = Fake::swallowing();
        let mut e = engine();
        let out = e.feed(&fake, &[scan(), btn(MIDDLE, true), SYN]);
        assert!(out.is_empty(), "{out:?}");
        assert_eq!(
            fake.events(),
            [Ev::Mouse {
                kind: Kind::Down,
                button: 2,
                pos: Point::new(100.0, 200.0)
            }]
        );
        assert_eq!(
            *fake.armed.borrow(),
            [true],
            "a hold asks for the pointer at full rate"
        );
        assert!(e.any_held());
    }

    #[test]
    fn a_quick_click_is_press_and_release_to_the_state_machine_and_nothing_to_the_app() {
        // the state machine re-posts the click itself (post_mouse), so nothing is forwarded here
        let fake = Fake::swallowing();
        let mut e = engine();
        let mut out = e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        out.extend(e.feed(&fake, &[btn(MIDDLE, false), SYN]));
        assert!(out.is_empty());
        let kinds: Vec<Kind> = fake.events().iter().map(kind_of).collect();
        assert_eq!(kinds, [Kind::Down, Kind::Up]);
        assert_eq!(*fake.armed.borrow(), [true, false]);
        assert!(!e.any_held());
    }

    #[test]
    fn a_drag_handed_back_forwards_its_release() {
        // the press was swallowed and re-posted by the state machine; in `Passthrough` the release is
        // answered `Keep`, and it is the only thing forwarded
        let fake = Fake::new(|ev| match ev {
            Ev::Mouse { kind: Kind::Up, .. } => Verdict::Keep,
            _ => Verdict::Drop,
        });
        let mut e = engine();
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        e.feed(&fake, &[rel(REL_X, 10), SYN]);
        let out = e.feed(&fake, &[btn(MIDDLE, false), SYN]);
        assert_eq!(out, [btn(MIDDLE, false), SYN]);
    }

    #[test]
    fn motion_while_the_trigger_is_held_is_a_drag_at_the_added_up_position() {
        let fake = Fake::swallowing();
        let mut e = engine();
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        let out = e.feed(&fake, &[rel(REL_X, 4), rel(REL_Y, -2), SYN]);
        assert_eq!(
            out,
            [rel(REL_X, 4), rel(REL_Y, -2), SYN],
            "the cursor still moves"
        );
        assert_eq!(
            fake.events()[1],
            Ev::Mouse {
                kind: Kind::Drag,
                button: 2,
                pos: Point::new(104.0, 198.0)
            }
        );
    }

    #[test]
    fn no_drag_is_sent_when_the_trigger_is_not_held() {
        let fake = Fake::swallowing();
        let mut e = engine();
        e.feed(&fake, &[rel(REL_X, 4), SYN]);
        assert!(fake.events().is_empty());
    }

    #[test]
    fn the_compositors_answer_replaces_the_added_up_position_once() {
        let fake = Fake::swallowing();
        let mut e = engine();
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        fake.pointer.set(Some((7, Point::new(500.0, 600.0))));
        e.feed(&fake, &[rel(REL_X, 1), SYN]);
        // the same answer again is not used twice: motion adds up on top of it
        e.feed(&fake, &[rel(REL_X, 3), SYN]);
        let drags: Vec<Point> = fake
            .events()
            .iter()
            .filter_map(|ev| match ev {
                Ev::Mouse {
                    kind: Kind::Drag,
                    pos,
                    ..
                } => Some(*pos),
                _ => None,
            })
            .collect();
        assert_eq!(drags, [Point::new(500.0, 600.0), Point::new(503.0, 600.0)]);
    }

    fn first_down(fake: &Fake) -> Point {
        match fake.events().first() {
            Some(Ev::Mouse {
                kind: Kind::Down,
                pos,
                ..
            }) => *pos,
            other => panic!("no press was routed: {other:?}"),
        }
    }

    #[test]
    fn the_trigger_press_takes_the_fresh_answer_of_the_compositor() {
        // the engine had added up a position from its own mouse; a touchpad moved the pointer since
        let fake = Fake::swallowing();
        let mut e = engine();
        fake.pointer.set(Some((3, Point::new(10.0, 10.0))));
        e.feed(&fake, &[rel(REL_X, 1), SYN]);
        fake.fresh.set(Some((4, Point::new(900.0, 700.0))));
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        assert_eq!(first_down(&fake), Point::new(900.0, 700.0));
        assert_eq!(*fake.asked.borrow(), [true], "the trigger asks and waits");
    }

    #[test]
    fn the_first_press_starts_from_the_compositor_and_not_from_the_x_server() {
        // no motion was read yet, so the engine knows nothing; the X server (XWayland) only knows
        // the pointer while it is over an X window
        let fake = Fake::swallowing();
        let mut e = engine();
        fake.pointer.set(Some((1, Point::new(640.0, 360.0))));
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        assert_eq!(first_down(&fake), Point::new(640.0, 360.0));
    }

    #[test]
    fn the_x_server_is_the_last_resort() {
        let fake = Fake::swallowing();
        let mut e = engine();
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        assert_eq!(first_down(&fake), Point::new(100.0, 200.0));
    }

    #[test]
    fn a_late_answer_keeps_the_added_up_position() {
        // the compositor did not answer in time: the answer it gave before was already used
        let fake = Fake::swallowing();
        let mut e = engine();
        fake.pointer.set(Some((5, Point::new(500.0, 600.0))));
        e.feed(&fake, &[rel(REL_X, 1), SYN]);
        e.feed(&fake, &[rel(REL_X, 4), SYN]);
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        assert_eq!(first_down(&fake), Point::new(504.0, 600.0));
    }

    #[test]
    fn only_the_trigger_waits_for_the_compositor() {
        let fake = Fake::new(|_| Verdict::Keep);
        let mut e = engine();
        e.feed(&fake, &[btn(BTN_SIDE, true), SYN]);
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        assert_eq!(
            *fake.asked.borrow(),
            [false, true],
            "a side button takes what is known, the trigger asks"
        );
    }

    #[test]
    fn the_state_machine_gets_the_events_the_x11_layer_sends_for_the_same_gesture() {
        // press, move, release of the middle button: Down, Drag, Up at the same positions
        let fake = Fake::swallowing();
        let mut e = engine();
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        e.feed(&fake, &[rel(REL_X, 8), SYN]);
        e.feed(&fake, &[btn(MIDDLE, false), SYN]);
        let p = |x| Point::new(x, 200.0);
        assert_eq!(
            fake.events(),
            [
                Ev::Mouse {
                    kind: Kind::Down,
                    button: 2,
                    pos: p(100.0)
                },
                Ev::Mouse {
                    kind: Kind::Drag,
                    button: 2,
                    pos: p(108.0)
                },
                Ev::Mouse {
                    kind: Kind::Up,
                    button: 2,
                    pos: p(108.0)
                },
            ]
        );
    }

    #[test]
    fn a_kept_side_press_goes_through_and_so_does_its_release_whatever_is_said() {
        let fake = Fake::new(|ev| match ev {
            Ev::Mouse {
                kind: Kind::Down, ..
            } => Verdict::Keep,
            _ => Verdict::Drop, // a release the state machine would swallow
        });
        let mut e = engine();
        let down = e.feed(&fake, &[btn(BTN_SIDE, true), SYN]);
        let up = e.feed(&fake, &[btn(BTN_SIDE, false), SYN]);
        assert_eq!(down, [btn(BTN_SIDE, true), SYN]);
        assert_eq!(
            up,
            [btn(BTN_SIDE, false), SYN],
            "never a button left down in the app"
        );
    }

    #[test]
    fn a_swallowed_side_press_has_no_release_either() {
        let fake = Fake::new(|ev| match ev {
            Ev::Mouse { kind: Kind::Up, .. } => Verdict::Keep, // even if it says Keep
            _ => Verdict::Drop,
        });
        let mut e = engine();
        e.feed(&fake, &[btn(BTN_SIDE, true), SYN]);
        let up = e.feed(&fake, &[btn(BTN_SIDE, false), SYN]);
        assert!(up.is_empty(), "an orphan release is not sent");
    }

    #[test]
    fn extra_buttons_are_routed_only_when_a_mode_uses_them_or_settings_scans() {
        let fake = Fake::swallowing();
        let mut e = engine();
        let extra5 = BTN_SIDE + 2; // BTN_FORWARD, button 5
        assert_eq!(
            e.feed(&fake, &[btn(extra5, true), SYN]),
            [btn(extra5, true), SYN]
        );
        assert!(fake.events().is_empty());
        e.set_buttons(2, BTreeSet::from([5]));
        assert!(e.feed(&fake, &[btn(extra5, true), SYN]).is_empty());
        assert_eq!(fake.events().len(), 1);
        e.set_buttons(2, BTreeSet::new());
        fake.scanning.set(true);
        e.feed(&fake, &[btn(extra5 + 1, true), SYN]);
        assert_eq!(fake.events().len(), 2, "a scan hears every extra button");
    }

    #[test]
    fn a_trigger_other_than_the_middle_button_leaves_the_middle_button_alone() {
        let fake = Fake::swallowing();
        let mut e = Engine::new(5, BTreeSet::new());
        let input = [btn(MIDDLE, true), SYN];
        assert_eq!(e.feed(&fake, &input), input);
    }

    #[test]
    fn lost_events_let_the_state_machine_see_the_held_button_go_up() {
        let fake = Fake::swallowing();
        let mut e = engine();
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        // the kernel dropped events: the rest of that frame and its end are not trusted
        let out = e.feed(
            &fake,
            &[
                rel(REL_X, 9),
                InputEvent::new(EV_SYN, SYN_DROPPED, 0),
                rel(REL_X, 9),
                SYN,
            ],
        );
        assert!(out.is_empty(), "the frame is lost");
        assert!(!e.any_held());
        assert_eq!(kind_of(fake.events().last().unwrap()), Kind::Up);
        // and the next frame is read normally
        assert_eq!(e.feed(&fake, &[rel(REL_X, 1), SYN]), [rel(REL_X, 1), SYN]);
    }

    #[test]
    fn events_wait_for_the_end_of_their_frame() {
        let fake = Fake::swallowing();
        let mut e = engine();
        assert!(e.feed(&fake, &[rel(REL_X, 1)]).is_empty());
        assert_eq!(
            e.feed(&fake, &[rel(REL_Y, 2), SYN]),
            [rel(REL_X, 1), rel(REL_Y, 2), SYN]
        );
    }

    #[test]
    fn a_fault_resets_the_engine_without_losing_the_events() {
        let fake = Fake::swallowing();
        let mut e = engine();
        e.feed(&fake, &[btn(MIDDLE, true), SYN]);
        e.reset();
        assert!(!e.any_held());
    }
}
