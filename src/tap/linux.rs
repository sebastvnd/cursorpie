//! X11 event layer: passive grabs on the mouse buttons CursorPie uses, on their own thread.
//!
//! [`spawn`] connects to the X server and grabs the trigger button and the two side buttons with
//! XInput2 (asynchronous mode, any modifier). A grabbed press is delivered to CursorPie instead of
//! the app under the pointer, and while it is held the pointer motion is too: that is the
//! "swallow". Native events are translated into the portable `Ev` and routed by `dispatch`.
//!
//! How it differs from the macOS and Windows layers, all handled here:
//!
//! * A swallowed press can only be passed on by **sending it again** (XTest). The grabs are lifted
//!   around that (`Conn::inject_button`) or CursorPie would catch its own event. A **quick click**
//!   (press and release both swallowed, then sent again) is reliable. Handing a **drag** back to the
//!   app while the button is still down is best effort: the server may drop a press of a button it
//!   believes is already down, so the app can miss the start of the drag (see
//!   `docs/LINUX_MIGRATION.md`, S2).
//! * A side-button press the state machine wants to keep (no mode set) is sent again when the
//!   button is released, which is the same click one moment later.
//! * While CursorPie is paused the grabs are lifted, so every event reaches the apps untouched.
//! * Keys: only while Settings records a shortcut is the keyboard grabbed; it is let go as soon as
//!   the key was captured.
//! * The loop never blocks in `dispatch` and fails open: a panic passes the event on.

use super::{
    Ev, Gesture, Kind, PAUSED, RECORDING, RIGHT_BUTTON, SIDE_BACK, SIDE_FORWARD, SWALLOW_KEY,
    TAP_ACTIVE, Verdict, dispatch,
};
use crate::config::{cfg, lock};
use crate::geometry::Point;
use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT};
use crate::msg::Notifier;
use crate::platform::keymap;
use crate::platform::linux::backend::{post_mouse, x_button_for};
use crate::platform::linux::session::Session;
use crate::platform::linux::x11::{self, Conn};
use crate::platform::linux::xlib::*;
use std::os::raw::c_int;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the loop waits for X traffic before it looks at the pause, recording and settings
/// flags again.
const IDLE_WAIT_MS: c_int = 30;
/// How long the keyboard stays grabbed after recording ended, waiting for the key's release.
const KEY_RELEASE_GRACE: Duration = Duration::from_millis(1500);

/// The macOS button number of an X button; `None` for buttons CursorPie ignores.
pub(super) fn mac_button(x_button: c_int) -> Option<i64> {
    match x_button {
        1 => Some(0),
        2 => Some(2),
        3 => Some(RIGHT_BUTTON),
        8 => Some(SIDE_BACK),
        9 => Some(SIDE_FORWARD),
        _ => None,
    }
}

/// The `MOD_*` bits of an `XKeyEvent::state`.
pub(super) fn mods_from_state(state: u32) -> u8 {
    let mut m = 0;
    if state & SHIFT_MASK != 0 {
        m |= MOD_SHIFT;
    }
    if state & CONTROL_MASK != 0 {
        m |= MOD_CTRL;
    }
    if state & MOD1_MASK != 0 {
        m |= MOD_ALT;
    }
    if state & MOD4_MASK != 0 {
        m |= MOD_CMD;
    }
    m
}

/// The X buttons to grab for a trigger (macOS numbering): the trigger itself and both side buttons.
pub(super) fn wanted_buttons(trigger: i64) -> Vec<c_int> {
    let mut v = Vec::new();
    if let Some(x) = x_button_for(trigger) {
        v.push(x);
    }
    for side in [8, 9] {
        if !v.contains(&side) {
            v.push(side);
        }
    }
    v
}

/// Starts the tap on its own thread (retrying while the X server cannot be reached).
pub fn spawn(notifier: Notifier) {
    let session = Session::current();
    if let Some(problem) = session.problem() {
        eprintln!("CursorPie: {problem}");
    }
    diag::log!(
        "tap",
        "session",
        server = format!("{:?}", session.server),
        desktops = session.desktops.join(","),
        has_x_display = session.has_x_display,
        problem = session.problem().unwrap_or("")
    );
    x11::warm_index();
    let state = Arc::new(Mutex::new(Gesture::Idle));
    let started = std::thread::Builder::new()
        .name("cursorpie-x11-tap".into())
        .spawn(move || run(state, notifier));
    if let Err(e) = started {
        eprintln!("CursorPie: could not start the mouse tap: {e}");
    }
}

fn run(state: Arc<Mutex<Gesture>>, notifier: Notifier) {
    let mut reported = false;
    loop {
        match x11::conn() {
            Some(conn) => {
                if !serve(conn, &state, &notifier) && !reported {
                    reported = true;
                    diag::log!("tap", "hook", backend = "x11", state = "grab_failed");
                    eprintln!(
                        "CursorPie: could not grab the mouse button (another program may hold it); retrying every 2 s"
                    );
                }
            }
            None => {
                if !reported {
                    reported = true;
                    diag::log!("tap", "hook", backend = "x11", state = "no_x_server");
                    eprintln!("CursorPie: cannot connect to the X server; retrying every 2 s");
                }
            }
        }
        TAP_ACTIVE.store(false, Ordering::SeqCst);
        std::thread::sleep(Duration::from_secs(2));
    }
}

/// Installs the grabs and handles events. Returns only if the trigger button could not be grabbed.
fn serve(conn: &'static Conn, state: &Arc<Mutex<Gesture>>, notifier: &Notifier) -> bool {
    conn.select_raw_release();
    let trigger = cfg().trigger_button;
    let wanted = wanted_buttons(trigger);
    let all_grabbed = conn.set_grabs(&wanted);
    diag::log!(
        "tap",
        "grabs",
        trigger = trigger,
        wanted = format!("{wanted:?}"),
        grabbed = format!("{:?}", conn.grabbed()),
        all_grabbed = all_grabbed
    );
    let trigger_x = x_button_for(trigger);
    if trigger_x.is_none_or(|x| !conn.grabbed().contains(&x)) {
        conn.release_grabs();
        return false;
    }
    TAP_ACTIVE.store(true, Ordering::SeqCst);
    diag::log!("tap", "hook", backend = "x11", state = "running");
    let mut tap = Tap {
        conn,
        state: state.clone(),
        notifier: notifier.clone(),
        wanted,
        trigger,
        lifted: false,
        pending_click: [false; 2],
        keyboard: false,
        keyboard_deadline: None,
    };
    // SAFETY: the display is valid for the whole process.
    let fd = unsafe { XConnectionNumber(conn.dpy) };
    loop {
        tap.drain();
        tap.housekeeping();
        let mut p = PollFd {
            fd,
            events: POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd.
        unsafe {
            poll(&mut p, 1, IDLE_WAIT_MS);
        }
    }
}

struct Tap {
    conn: &'static Conn,
    state: Arc<Mutex<Gesture>>,
    notifier: Notifier,
    /// The buttons that are grabbed (or would be, after a pause).
    wanted: Vec<c_int>,
    /// The trigger button the grabs were made for (macOS numbering).
    trigger: i64,
    /// The grabs are lifted because CursorPie is paused.
    lifted: bool,
    /// A side-button press the state machine kept: `[back, forward]`. Its click is sent again when
    /// the button is released.
    pending_click: [bool; 2],
    /// The keyboard is grabbed to record a shortcut.
    keyboard: bool,
    /// When to give up waiting for the recorded key's release.
    keyboard_deadline: Option<Instant>,
}

impl Tap {
    /// Handles every event that is waiting.
    fn drain(&mut self) {
        loop {
            // SAFETY: valid display; `XNextEvent` only runs when an event is waiting, so it does
            // not block.
            let pending = unsafe { XPending(self.conn.dpy) };
            if pending <= 0 {
                break;
            }
            let mut ev = XEvent::zeroed();
            // SAFETY: `ev` is a valid, zeroed event.
            unsafe {
                XNextEvent(self.conn.dpy, &mut ev);
            }
            self.handle(&mut ev);
        }
    }

    fn handle(&mut self, ev: &mut XEvent) {
        // SAFETY: every member of the union starts with the event type.
        let ty = unsafe { ev.type_ };
        let raw = ev as *mut XEvent;
        match ty {
            GENERIC_EVENT => {
                let cookie = raw as *mut XGenericEventCookie;
                // SAFETY: a generic event is an `XGenericEventCookie`; its data is valid between
                // `XGetEventData` and `XFreeEventData`.
                unsafe {
                    if (*cookie).extension != self.conn.xi_opcode
                        || XGetEventData(self.conn.dpy, cookie) == FALSE
                    {
                        return;
                    }
                    let evtype = (*cookie).evtype;
                    let data = (*cookie).data;
                    match evtype {
                        XI_BUTTON_PRESS | XI_BUTTON_RELEASE | XI_MOTION => {
                            self.on_device(evtype, &*(data as *const XIDeviceEvent));
                        }
                        XI_RAW_BUTTON_RELEASE => {
                            self.on_raw_release(&*(data as *const XIRawEvent));
                        }
                        _ => {}
                    }
                    XFreeEventData(self.conn.dpy, cookie);
                }
            }
            KEY_PRESS | KEY_RELEASE => {
                // SAFETY: a key event is an `XKeyEvent`.
                let key = unsafe { &*(raw as *const XKeyEvent) };
                self.on_key(ty == KEY_PRESS, key);
            }
            _ => {}
        }
    }

    /// Runs the state machine; a panic passes the event on.
    fn route(&self, ev: Ev) -> Verdict {
        catch_unwind(AssertUnwindSafe(|| {
            dispatch(&self.state, &self.notifier, ev, &|| {})
        }))
        .unwrap_or(Verdict::Keep)
    }

    fn on_device(&mut self, evtype: c_int, e: &XIDeviceEvent) {
        self.conn.set_master(e.deviceid);
        if e.flags & XI_POINTER_EMULATED != 0 {
            return;
        }
        let pos = Point::new(e.root_x / self.conn.scale, e.root_y / self.conn.scale);
        if evtype == XI_MOTION {
            diag::trace!("tap", "drag", x = pos.x, y = pos.y);
        } else {
            // the position the event carries, next to what the server says right now
            diag::log!(
                "tap",
                if evtype == XI_BUTTON_PRESS {
                    "press"
                } else {
                    "release"
                },
                detail = e.detail,
                root_x = e.root_x,
                root_y = e.root_y,
                scale = self.conn.scale,
                x = pos.x,
                y = pos.y,
                query_px = self
                    .conn
                    .pointer_px()
                    .map_or_else(String::new, |(qx, qy)| format!("{qx},{qy}"))
            );
        }
        let (kind, button) = match evtype {
            XI_BUTTON_PRESS => (Kind::Down, mac_button(e.detail)),
            XI_BUTTON_RELEASE => (Kind::Up, mac_button(e.detail)),
            // moves only arrive while a button is held; they belong to the trigger gesture
            _ => (Kind::Drag, Some(self.trigger)),
        };
        let Some(button) = button else { return };
        let verdict = self.route(Ev::Mouse { kind, button, pos });
        if kind != Kind::Drag {
            diag::log!(
                "tap",
                "verdict",
                kind = format!("{kind:?}"),
                button = button,
                verdict = format!("{verdict:?}")
            );
        }
        let side = match button {
            SIDE_BACK => Some(0),
            SIDE_FORWARD => Some(1),
            _ => None,
        };
        let Some(i) = side else { return };
        match kind {
            Kind::Down if matches!(verdict, Verdict::Keep) => self.pending_click[i] = true,
            Kind::Up if self.pending_click[i] => {
                self.pending_click[i] = false;
                post_mouse(pos, Kind::Down, button);
                post_mouse(pos, Kind::Up, button);
            }
            _ => {}
        }
    }

    /// The physical release of a button while no grab holds it: the end of a drag that was handed
    /// back to the app.
    fn on_raw_release(&mut self, e: &XIRawEvent) {
        if x_button_for(self.trigger) != Some(e.detail) {
            return;
        }
        if !matches!(*lock(&self.state), Gesture::Passthrough) {
            return;
        }
        let (x, y) = self.conn.pointer().unwrap_or((0.0, 0.0));
        self.route(Ev::Mouse {
            kind: Kind::Up,
            button: self.trigger,
            pos: Point::new(x, y),
        });
    }

    fn on_key(&mut self, down: bool, k: &XKeyEvent) {
        let Some(code) = keymap::code_for_x_keycode(k.keycode) else {
            return; // a modifier on its own, or a key without a code
        };
        self.route(Ev::Key {
            down,
            code: code as i32,
            mods: mods_from_state(k.state),
        });
    }

    /// Follows the pause switch, the trigger setting and the recording flag.
    fn housekeeping(&mut self) {
        // pause: lift the grabs so every click reaches the apps
        let paused = PAUSED.load(Ordering::SeqCst);
        if paused && !self.lifted {
            self.conn.release_grabs();
            self.lifted = true;
        } else if !paused && self.lifted {
            self.conn.restore_grabs();
            self.lifted = false;
        }

        // a changed trigger button needs new grabs
        let trigger = cfg().trigger_button;
        if !self.lifted && trigger != self.trigger {
            self.wanted = wanted_buttons(trigger);
            self.conn.set_grabs(&self.wanted);
            self.trigger = trigger;
        }

        // recording a shortcut: take the keyboard, and give it back once the key is released
        let recording = RECORDING.load(Ordering::SeqCst);
        if recording {
            self.keyboard_deadline = None;
            if !self.keyboard && self.conn.grab_keyboard() {
                self.keyboard = true;
            }
        } else if self.keyboard {
            let deadline = *self
                .keyboard_deadline
                .get_or_insert_with(|| Instant::now() + KEY_RELEASE_GRACE);
            let released = SWALLOW_KEY.load(Ordering::SeqCst) < 0;
            if released || Instant::now() >= deadline {
                self.conn.ungrab_keyboard();
                SWALLOW_KEY.store(-1, Ordering::SeqCst);
                self.keyboard = false;
                self.keyboard_deadline = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_buttons_map_to_the_numbers_the_state_machine_uses() {
        assert_eq!(mac_button(1), Some(0));
        assert_eq!(mac_button(2), Some(2));
        assert_eq!(mac_button(3), Some(RIGHT_BUTTON));
        assert_eq!(mac_button(8), Some(SIDE_BACK));
        assert_eq!(mac_button(9), Some(SIDE_FORWARD));
        assert_eq!(mac_button(4), None, "wheel up");
        assert_eq!(mac_button(5), None, "wheel down");
        assert_eq!(mac_button(10), None);
    }

    #[test]
    fn every_grabbed_button_maps_back_to_a_known_button() {
        for x in wanted_buttons(2) {
            assert!(mac_button(x).is_some(), "X button {x}");
        }
    }

    #[test]
    fn the_trigger_and_both_side_buttons_are_grabbed() {
        assert_eq!(wanted_buttons(2), [2, 8, 9]);
        assert_eq!(
            wanted_buttons(3),
            [8, 9],
            "macOS button 3 is the back button, X button 8"
        );
        assert_eq!(
            wanted_buttons(5),
            [8, 9],
            "no X button known for an extra button"
        );
    }

    #[test]
    fn modifier_state_becomes_mod_bits() {
        assert_eq!(mods_from_state(0), 0);
        assert_eq!(mods_from_state(CONTROL_MASK), MOD_CTRL);
        assert_eq!(
            mods_from_state(SHIFT_MASK | MOD1_MASK | MOD4_MASK),
            MOD_SHIFT | MOD_ALT | MOD_CMD
        );
        // Caps Lock (bit 1) and Num Lock (Mod2, bit 4) are not modifiers of a shortcut
        assert_eq!(mods_from_state((1 << 1) | (1 << 4)), 0);
    }
}
