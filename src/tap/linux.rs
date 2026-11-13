//! X11 event layer: passive grabs on the mouse buttons CursorPie uses, on their own thread.
//!
//! [`spawn`] connects to the X server and grabs the trigger button and the two side buttons with
//! XInput2 (asynchronous mode, any modifier). The extra buttons are grabbed only when a mode sends an
//! action for them. A grabbed press is delivered to CursorPie instead of
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
//!   `docs/developer/LINUX_MIGRATION.md`, S2).
//! * A side-button press the state machine wants to keep (no mode set) is sent again when the
//!   button is released, which is the same click one moment later.
//! * While CursorPie is paused the grabs are lifted, so every event reaches the apps untouched.
//! * Keys: only while Settings records a shortcut is the keyboard grabbed; it is let go as soon as
//!   the key was captured.
//! * Touchpad: the finger positions come from `/dev/input` (`touch::evdev`), because libinput hides
//!   them from X. What X must do for it is here: while a touchpad hold is running, the left and
//!   right buttons are grabbed too, so the click of the extra finger can be swallowed
//!   (`touch::extra`); a click that is not the extra one is sent again at its release, like a side
//!   button. With the trackpad hold on, the press of any key is heard (raw, no key is named) for the
//!   typing guard.
//! * The loop never blocks in `dispatch` and fails open: a panic passes the event on.

use super::{
    Ev, Gesture, Kind, PAUSED, RECORDING, RIGHT_BUTTON, SIDE_BACK, SIDE_FORWARD, SWALLOW_KEY,
    TAP_ACTIVE, Verdict, dispatch, modorder, touch,
};
use crate::config::{cfg, extra_buttons_in_use, lock};
use crate::geometry::Point;
use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT};
use crate::mouse;
use crate::msg::{Msg, Notifier};
use crate::platform::keymap;
use crate::platform::linux::backend::{post_mouse, x_button_for};
use crate::platform::linux::session::Session;
use crate::platform::linux::x11::{self, Conn};
use crate::platform::linux::xlib::*;
use std::collections::BTreeSet;
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

/// The macOS button number of an X button; `None` for buttons CursorPie ignores (the wheel, X
/// buttons 4 to 7, and everything past `mouse::MAX_BUTTON`). X button 10 and up are the extra buttons
/// 5 and up.
pub(super) fn mac_button(x_button: c_int) -> Option<i64> {
    match x_button {
        1 => Some(0),
        2 => Some(2),
        3 => Some(RIGHT_BUTTON),
        8 => Some(SIDE_BACK),
        9 => Some(SIDE_FORWARD),
        x if x >= 10 && mouse::is_extra(i64::from(x) - 5) => Some(i64::from(x) - 5),
        _ => None,
    }
}

/// The `MOD_*` bits of an `XKeyEvent::state`.
/// The `MOD_*` bit of an X keycode of a modifier key (either side), `None` for other keys. The X
/// keycode is the Linux key code plus 8 (`KEY_LEFTSHIFT` 42, `KEY_LEFTCTRL` 29, `KEY_LEFTALT` 56,
/// `KEY_LEFTMETA` 125, and the right ones).
fn modifier_bit(keycode: u32) -> Option<u8> {
    match keycode.checked_sub(8)? {
        42 | 54 => Some(MOD_SHIFT),
        29 | 97 => Some(MOD_CTRL),
        56 | 100 => Some(MOD_ALT),
        125 | 126 => Some(MOD_CMD),
        _ => None,
    }
}

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

/// The X buttons to grab for a trigger (macOS numbering): the trigger itself, both side buttons and
/// the extra buttons (`extras`) that a mode sends an action for.
pub(super) fn wanted_buttons(trigger: i64, extras: &BTreeSet<i64>) -> Vec<c_int> {
    let mut v = Vec::new();
    if let Some(x) = x_button_for(trigger) {
        v.push(x);
    }
    for side in [8, 9] {
        if !v.contains(&side) {
            v.push(side);
        }
    }
    for x in extras.iter().filter_map(|b| x_button_for(*b)) {
        if !v.contains(&x) {
            v.push(x);
        }
    }
    v
}

/// The X buttons of the left and the right button: grabbed while a touchpad hold is running.
const TOUCH_BUTTONS: [c_int; 2] = [1, 3];
/// The X buttons of the wheel (up, down, left, right): a touchpad's two-finger scroll arrives as
/// presses of these. They are grabbed, and their presses ignored, while a touchpad hold runs, so
/// the scrolling the fingers make does not reach the app under the pie.
const SCROLL_BUTTONS: [c_int; 4] = [4, 5, 6, 7];
/// A kept left or right press is forgotten after this long without its release (the button was let
/// go while no grab heard it); its click is then lost, never repeated late.
const PENDING_CLICK_MAX: Duration = Duration::from_secs(3);

/// The buttons to grab: `base` (see [`wanted_buttons`]), the left and the right button (the extra
/// click) while a touchpad hold needs them (`touch_active`), and the wheel (the scrolling of the
/// fingers) while a hold runs or the tail of its scroll is still open (`scroll_active`, see
/// [`touch::scroll`]).
pub(super) fn grabs_for(base: &[c_int], touch_active: bool, scroll_active: bool) -> Vec<c_int> {
    let mut v = base.to_vec();
    let wanted = [
        (touch_active, &TOUCH_BUTTONS[..]),
        (scroll_active, &SCROLL_BUTTONS[..]),
    ];
    for b in wanted
        .into_iter()
        .filter(|w| w.0)
        .flat_map(|w| w.1.iter().copied())
    {
        if !v.contains(&b) {
            v.push(b);
        }
    }
    v
}

/// Index of a left or right button (macOS numbering) in the table of kept presses.
fn basic_slot(button: i64) -> Option<usize> {
    match button {
        0 => Some(0),
        RIGHT_BUTTON => Some(1),
        _ => None,
    }
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
    touch::start(state.clone(), notifier.clone());
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
    let raw_keys = cfg().touch_trigger;
    conn.select_raw(raw_keys);
    let (trigger, extras) = {
        let c = cfg();
        (c.trigger_button, extra_buttons_in_use(&c))
    };
    let wanted = wanted_buttons(trigger, &extras);
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
        extras,
        lifted: false,
        pending_click: [false; mouse::SLOTS],
        pending_basic: [None; 2],
        touch_grabs: false,
        scroll_grabs: false,
        raw_keys,
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
    /// The extra buttons the grabs were made for.
    extras: BTreeSet<i64>,
    /// The grabs are lifted because CursorPie is paused.
    lifted: bool,
    /// A side or extra button press the state machine kept, by `mouse::slot`. Its click is sent
    /// again when the button is released.
    pending_click: [bool; mouse::SLOTS],
    /// The same for the left and the right button (grabbed while a touchpad hold runs), with the time
    /// of the press.
    pending_basic: [Option<Instant>; 2],
    /// The left and the right button are grabbed for a touchpad hold.
    touch_grabs: bool,
    /// The wheel is grabbed for a touchpad hold and the tail of its scroll.
    scroll_grabs: bool,
    /// The press of any key is selected (the trackpad hold is on).
    raw_keys: bool,
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
                        XI_RAW_BUTTON_PRESS => {
                            self.on_raw_press(&*(data as *const XIRawEvent));
                        }
                        XI_RAW_BUTTON_RELEASE => {
                            self.on_raw_release(&*(data as *const XIRawEvent));
                        }
                        XI_RAW_KEY_PRESS => touch::note_key(),
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
        // A grabbed wheel press: swallowed (there is nothing to route) and it keeps the tail of the
        // scroll alive. It comes before the filter and the log below, because scrolling arrives as
        // emulated presses (a touchpad makes hundreds a second) and the log asks the server where
        // the pointer is.
        if matches!(evtype, XI_BUTTON_PRESS | XI_BUTTON_RELEASE)
            && SCROLL_BUTTONS.contains(&e.detail)
        {
            touch::scroll::swallows();
            return;
        }
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
        // the trigger's own press and release are handled by its gesture
        if button != self.trigger
            && let Some(i) = basic_slot(button)
        {
            match kind {
                Kind::Down if matches!(verdict, Verdict::Keep) => {
                    self.pending_basic[i] = Some(Instant::now());
                }
                Kind::Up if self.pending_basic[i].take().is_some() => {
                    post_mouse(pos, Kind::Down, button);
                    post_mouse(pos, Kind::Up, button);
                }
                _ => {}
            }
            return;
        }
        let side = mouse::slot(button).filter(|_| button != self.trigger);
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

    /// A button went down, grabbed or not. Only a scan in Settings cares: the press is reported so
    /// the button is found even when no mode uses it (and so nothing grabbed it).
    fn on_raw_press(&mut self, e: &XIRawEvent) {
        if !mouse::scanning() {
            return;
        }
        if let Some(button) = mac_button(e.detail).filter(|b| mouse::is_extra(*b)) {
            self.notifier.send(Msg::ButtonScanned(button));
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
            // a modifier on its own: remembered for the order of a recorded shortcut
            if let Some(bit) = modifier_bit(k.keycode) {
                if down {
                    modorder::press(bit);
                } else {
                    modorder::release(bit);
                }
            }
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

        // a changed trigger button, or a mode that now uses another extra button, needs new grabs
        let (trigger, extras) = {
            let c = cfg();
            (c.trigger_button, extra_buttons_in_use(&c))
        };
        if !self.lifted && (trigger != self.trigger || extras != self.extras) {
            self.wanted = wanted_buttons(trigger, &extras);
            self.conn.set_grabs(&grabs_for(
                &self.wanted,
                self.touch_grabs,
                self.scroll_grabs,
            ));
            self.trigger = trigger;
            self.extras = extras;
        }

        // a touchpad hold takes the left and the right button for its extra click, and lets them go
        // when it is over (not while a click it kept or swallowed is still down: its release must
        // reach us)
        self.pending_basic
            .iter_mut()
            .filter(|p| p.is_some_and(|t| t.elapsed() > PENDING_CLICK_MAX))
            .for_each(|p| *p = None);
        let busy =
            self.pending_basic.iter().any(Option::is_some) || touch::extra::interested(Kind::Up);
        let want = touch::ACTIVE.load(Ordering::SeqCst) || (self.touch_grabs && busy);
        // the wheel is grabbed for as long as the pie swallows scrolling: the hold, and the tail
        // of the scroll its fingers began (checking it also closes a tail that ran out)
        let want_scroll = touch::scroll::wanted();
        if !self.lifted && (want != self.touch_grabs || want_scroll != self.scroll_grabs) {
            self.touch_grabs = want;
            self.scroll_grabs = want_scroll;
            self.conn.set_grabs(&grabs_for(
                &self.wanted,
                self.touch_grabs,
                self.scroll_grabs,
            ));
            diag::log!("touch", "x_grabs", left_right = want, wheel = want_scroll);
        }

        // the typing guard listens to keys only while the trackpad hold is on
        let keys = cfg().touch_trigger;
        if keys != self.raw_keys {
            self.raw_keys = keys;
            self.conn.select_raw(keys);
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
        assert_eq!(mac_button(10), Some(5), "the first extra button");
        assert_eq!(mac_button(20), Some(15), "the last one");
        assert_eq!(mac_button(21), None);
        assert_eq!(mac_button(7), None, "wheel right");
    }

    #[test]
    fn every_grabbed_button_maps_back_to_a_known_button() {
        for x in wanted_buttons(2, &BTreeSet::from([5, 6, 15])) {
            assert!(mac_button(x).is_some(), "X button {x}");
        }
    }

    #[test]
    fn the_trigger_and_both_side_buttons_are_grabbed() {
        let none = BTreeSet::new();
        assert_eq!(wanted_buttons(2, &none), [2, 8, 9]);
        assert_eq!(
            wanted_buttons(3, &none),
            [8, 9],
            "macOS button 3 is the back button, X button 8"
        );
        assert_eq!(
            wanted_buttons(5, &none),
            [10, 8, 9],
            "macOS button 5 is X button 10"
        );
    }

    #[test]
    fn the_extra_buttons_a_mode_uses_are_grabbed_too() {
        let extras = BTreeSet::from([5, 7]);
        assert_eq!(wanted_buttons(2, &extras), [2, 8, 9, 10, 12]);
        // the trigger is not grabbed twice
        assert_eq!(wanted_buttons(5, &extras), [10, 8, 9, 12]);
    }

    #[test]
    fn a_touchpad_hold_adds_the_left_and_right_button_to_the_grabs() {
        let base = wanted_buttons(2, &BTreeSet::new());
        assert_eq!(
            grabs_for(&base, false, false),
            base,
            "nothing changes without a hold"
        );
        assert_eq!(grabs_for(&base, true, true), [2, 8, 9, 1, 3, 4, 5, 6, 7]);
        // a trigger that already is one of them is not grabbed twice
        let left = wanted_buttons(0, &BTreeSet::new());
        assert_eq!(grabs_for(&left, true, true), [1, 8, 9, 3, 4, 5, 6, 7]);
        for x in grabs_for(&base, true, true) {
            // the wheel is grabbed to be swallowed: it has no button of its own
            assert_eq!(
                mac_button(x).is_some(),
                !SCROLL_BUTTONS.contains(&x),
                "X button {x}"
            );
        }
    }

    #[test]
    fn the_wheel_is_grabbed_on_its_own_for_the_tail_of_a_scroll() {
        let base = wanted_buttons(2, &BTreeSet::new());
        // the pie is closed, the glide of its scroll is not over: the wheel only, clicks pass
        assert_eq!(grabs_for(&base, false, true), [2, 8, 9, 4, 5, 6, 7]);
        // a hold with no scroll to swallow yet is not asked for the wheel by this function
        assert_eq!(grabs_for(&base, true, false), [2, 8, 9, 1, 3]);
    }

    #[test]
    fn only_the_left_and_the_right_button_have_a_kept_press_slot() {
        assert_eq!(basic_slot(0), Some(0));
        assert_eq!(basic_slot(RIGHT_BUTTON), Some(1));
        assert_eq!(basic_slot(2), None, "middle");
        assert_eq!(basic_slot(SIDE_BACK), None);
        // the left and the right X buttons are the ones that are grabbed
        assert_eq!(mac_button(TOUCH_BUTTONS[0]), Some(0));
        assert_eq!(mac_button(TOUCH_BUTTONS[1]), Some(RIGHT_BUTTON));
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
