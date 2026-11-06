//! Hand-written bindings to the few Xlib, XInput2, XTest and XRandR functions CursorPie needs.
//!
//! The project keeps its dependencies few (like `windows-sys` on Windows), and these four C
//! libraries are present on every X11 or XWayland system. Struct layouts follow the C headers
//! (`X11/Xlib.h`, `X11/extensions/XInput2.h`, `Xrandr.h`); the XInput2 and key event layouts were
//! checked against a real X server by decoding live events. Everything here is `unsafe` to call.
#![allow(non_camel_case_types, non_snake_case, dead_code)]

use std::os::raw::{c_char, c_double, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};

pub type Display = c_void;
pub type Window = c_ulong;
pub type Atom = c_ulong;
pub type Time = c_ulong;
pub type Cursor = c_ulong;
pub type Bool = c_int;

pub const FALSE: Bool = 0;
pub const TRUE: Bool = 1;
pub const CURRENT_TIME: Time = 0;
pub const NONE: c_ulong = 0;

// predefined atoms
pub const XA_ATOM: Atom = 4;
pub const XA_CARDINAL: Atom = 6;
pub const XA_STRING: Atom = 31;
pub const XA_WINDOW: Atom = 33;
pub const XA_WM_CLASS: Atom = 67;

// event types
pub const KEY_PRESS: c_int = 2;
pub const KEY_RELEASE: c_int = 3;
pub const GENERIC_EVENT: c_int = 35;
pub const CLIENT_MESSAGE: c_int = 33;

// grab modes and results
pub const GRAB_MODE_ASYNC: c_int = 1;
pub const GRAB_SUCCESS: c_int = 0;

// event masks for XSendEvent
pub const SUBSTRUCTURE_NOTIFY_MASK: c_long = 1 << 19;
pub const SUBSTRUCTURE_REDIRECT_MASK: c_long = 1 << 20;

// XInput2
pub const XI_ALL_DEVICES: c_int = 0;
pub const XI_ALL_MASTER_DEVICES: c_int = 1;
pub const XI_BUTTON_PRESS: c_int = 4;
pub const XI_BUTTON_RELEASE: c_int = 5;
pub const XI_MOTION: c_int = 6;
pub const XI_RAW_KEY_PRESS: c_int = 13;
pub const XI_RAW_BUTTON_PRESS: c_int = 15;
pub const XI_RAW_BUTTON_RELEASE: c_int = 16;
/// Highest event type of XInput 2.2 plus one byte of slack: the mask is `(XI_LASTEVENT >> 3) + 1`
/// bytes long.
pub const XI_MASK_LEN: usize = 4;
/// `XIAnyModifier` is `1 << 31`, which C reads as a negative `int`.
pub const XI_ANY_MODIFIER: c_int = i32::MIN;
/// The event came from scroll or touch emulation, not from a real button.
pub const XI_POINTER_EMULATED: c_int = 1 << 16;

// core modifier masks in `XKeyEvent::state`
pub const SHIFT_MASK: c_uint = 1 << 0;
pub const CONTROL_MASK: c_uint = 1 << 2;
pub const MOD1_MASK: c_uint = 1 << 3; // Alt
pub const MOD4_MASK: c_uint = 1 << 6; // Super

pub const POLLIN: i16 = 1;

#[repr(C)]
pub struct XErrorEvent {
    pub type_: c_int,
    pub display: *mut Display,
    pub resourceid: c_ulong,
    pub serial: c_ulong,
    pub error_code: c_uchar,
    pub request_code: c_uchar,
    pub minor_code: c_uchar,
}

#[repr(C)]
pub struct XGenericEventCookie {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub extension: c_int,
    pub evtype: c_int,
    pub cookie: c_uint,
    pub data: *mut c_void,
}

#[repr(C)]
pub struct XKeyEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub window: Window,
    pub root: Window,
    pub subwindow: Window,
    pub time: Time,
    pub x: c_int,
    pub y: c_int,
    pub x_root: c_int,
    pub y_root: c_int,
    pub state: c_uint,
    pub keycode: c_uint,
    pub same_screen: Bool,
}

#[repr(C)]
pub struct XClientMessageEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub window: Window,
    pub message_type: Atom,
    pub format: c_int,
    pub data: [c_long; 5],
}

/// `XEvent` is a union padded to 24 longs.
#[repr(C)]
pub union XEvent {
    pub type_: c_int,
    pub xcookie: std::mem::ManuallyDrop<XGenericEventCookie>,
    pub xkey: std::mem::ManuallyDrop<XKeyEvent>,
    pub xclient: std::mem::ManuallyDrop<XClientMessageEvent>,
    pub pad: [c_long; 24],
}

impl XEvent {
    /// An all-zero event, ready to be filled by `XNextEvent`.
    pub fn zeroed() -> XEvent {
        XEvent { pad: [0; 24] }
    }
}

#[repr(C)]
pub struct XIEventMask {
    pub deviceid: c_int,
    pub mask_len: c_int,
    pub mask: *mut c_uchar,
}

#[repr(C)]
pub struct XIGrabModifiers {
    pub modifiers: c_int,
    pub status: c_int,
}

#[repr(C)]
pub struct XIButtonState {
    pub mask_len: c_int,
    pub mask: *mut c_uchar,
}

#[repr(C)]
pub struct XIValuatorState {
    pub mask_len: c_int,
    pub mask: *mut c_uchar,
    pub values: *mut c_double,
}

#[repr(C)]
pub struct XIModifierState {
    pub base: c_int,
    pub latched: c_int,
    pub locked: c_int,
    pub effective: c_int,
}

/// `XIDeviceEvent`: button press, release and motion (also used for the group state, which has the
/// same shape as the modifier state).
#[repr(C)]
pub struct XIDeviceEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub extension: c_int,
    pub evtype: c_int,
    pub time: Time,
    pub deviceid: c_int,
    pub sourceid: c_int,
    pub detail: c_int,
    pub root: Window,
    pub event: Window,
    pub child: Window,
    pub root_x: c_double,
    pub root_y: c_double,
    pub event_x: c_double,
    pub event_y: c_double,
    pub flags: c_int,
    pub buttons: XIButtonState,
    pub valuators: XIValuatorState,
    pub mods: XIModifierState,
    pub group: XIModifierState,
}

#[repr(C)]
pub struct XIRawEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub extension: c_int,
    pub evtype: c_int,
    pub time: Time,
    pub deviceid: c_int,
    pub sourceid: c_int,
    pub detail: c_int,
    pub flags: c_int,
    pub valuators: XIValuatorState,
    pub raw_values: *mut c_double,
}

#[repr(C)]
pub struct XRRMonitorInfo {
    pub name: Atom,
    pub primary: Bool,
    pub automatic: Bool,
    pub noutput: c_int,
    pub x: c_int,
    pub y: c_int,
    pub width: c_int,
    pub height: c_int,
    pub mwidth: c_int,
    pub mheight: c_int,
    pub outputs: *mut c_ulong,
}

#[repr(C)]
pub struct PollFd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}

pub type XErrorHandler = Option<unsafe extern "C" fn(*mut Display, *mut XErrorEvent) -> c_int>;

#[link(name = "X11")]
unsafe extern "C" {
    pub fn XInitThreads() -> c_int;
    pub fn XOpenDisplay(name: *const c_char) -> *mut Display;
    pub fn XCloseDisplay(dpy: *mut Display) -> c_int;
    pub fn XDefaultRootWindow(dpy: *mut Display) -> Window;
    pub fn XConnectionNumber(dpy: *mut Display) -> c_int;
    pub fn XInternAtom(dpy: *mut Display, name: *const c_char, only_if_exists: Bool) -> Atom;
    pub fn XGetAtomName(dpy: *mut Display, atom: Atom) -> *mut c_char;
    pub fn XFree(data: *mut c_void) -> c_int;
    pub fn XFlush(dpy: *mut Display) -> c_int;
    pub fn XSync(dpy: *mut Display, discard: Bool) -> c_int;
    pub fn XPending(dpy: *mut Display) -> c_int;
    pub fn XNextEvent(dpy: *mut Display, event: *mut XEvent) -> c_int;
    pub fn XQueryExtension(
        dpy: *mut Display,
        name: *const c_char,
        major_opcode: *mut c_int,
        first_event: *mut c_int,
        first_error: *mut c_int,
    ) -> Bool;
    pub fn XGetEventData(dpy: *mut Display, cookie: *mut XGenericEventCookie) -> Bool;
    pub fn XFreeEventData(dpy: *mut Display, cookie: *mut XGenericEventCookie);
    pub fn XSetErrorHandler(handler: XErrorHandler) -> XErrorHandler;
    pub fn XQueryPointer(
        dpy: *mut Display,
        w: Window,
        root_return: *mut Window,
        child_return: *mut Window,
        root_x: *mut c_int,
        root_y: *mut c_int,
        win_x: *mut c_int,
        win_y: *mut c_int,
        mask_return: *mut c_uint,
    ) -> Bool;
    pub fn XGetWindowProperty(
        dpy: *mut Display,
        w: Window,
        property: Atom,
        long_offset: c_long,
        long_length: c_long,
        delete: Bool,
        req_type: Atom,
        actual_type: *mut Atom,
        actual_format: *mut c_int,
        nitems: *mut c_ulong,
        bytes_after: *mut c_ulong,
        prop: *mut *mut c_uchar,
    ) -> c_int;
    pub fn XTranslateCoordinates(
        dpy: *mut Display,
        src: Window,
        dest: Window,
        src_x: c_int,
        src_y: c_int,
        dest_x: *mut c_int,
        dest_y: *mut c_int,
        child: *mut Window,
    ) -> Bool;
    pub fn XGetGeometry(
        dpy: *mut Display,
        d: Window,
        root: *mut Window,
        x: *mut c_int,
        y: *mut c_int,
        width: *mut c_uint,
        height: *mut c_uint,
        border: *mut c_uint,
        depth: *mut c_uint,
    ) -> c_int;
    pub fn XSendEvent(
        dpy: *mut Display,
        w: Window,
        propagate: Bool,
        event_mask: c_long,
        event: *mut XEvent,
    ) -> c_int;
    pub fn XGrabKeyboard(
        dpy: *mut Display,
        grab_window: Window,
        owner_events: Bool,
        pointer_mode: c_int,
        keyboard_mode: c_int,
        time: Time,
    ) -> c_int;
    pub fn XUngrabKeyboard(dpy: *mut Display, time: Time) -> c_int;
}

#[link(name = "Xi")]
unsafe extern "C" {
    pub fn XIQueryVersion(dpy: *mut Display, major: *mut c_int, minor: *mut c_int) -> c_int;
    pub fn XISelectEvents(
        dpy: *mut Display,
        win: Window,
        masks: *mut XIEventMask,
        num_masks: c_int,
    ) -> c_int;
    pub fn XIGrabButton(
        dpy: *mut Display,
        deviceid: c_int,
        button: c_int,
        grab_window: Window,
        cursor: Cursor,
        grab_mode: c_int,
        paired_device_mode: c_int,
        owner_events: Bool,
        mask: *mut XIEventMask,
        num_modifiers: c_int,
        modifiers: *mut XIGrabModifiers,
    ) -> c_int;
    pub fn XIUngrabButton(
        dpy: *mut Display,
        deviceid: c_int,
        button: c_int,
        grab_window: Window,
        num_modifiers: c_int,
        modifiers: *mut XIGrabModifiers,
    ) -> c_int;
    pub fn XIUngrabDevice(dpy: *mut Display, deviceid: c_int, time: Time) -> c_int;
}

#[link(name = "Xtst")]
unsafe extern "C" {
    pub fn XTestFakeButtonEvent(
        dpy: *mut Display,
        button: c_uint,
        is_press: Bool,
        delay: c_ulong,
    ) -> c_int;
    pub fn XTestFakeKeyEvent(
        dpy: *mut Display,
        keycode: c_uint,
        is_press: Bool,
        delay: c_ulong,
    ) -> c_int;
}

#[link(name = "Xrandr")]
unsafe extern "C" {
    pub fn XRRGetMonitors(
        dpy: *mut Display,
        window: Window,
        get_active: Bool,
        nmonitors: *mut c_int,
    ) -> *mut XRRMonitorInfo;
    pub fn XRRFreeMonitors(monitors: *mut XRRMonitorInfo);
}

// libc, which std already links
unsafe extern "C" {
    pub fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    pub fn flock(fd: c_int, operation: c_int) -> c_int;
    pub fn getuid() -> c_uint;
}

#[cfg(target_env = "gnu")]
unsafe extern "C" {
    pub fn malloc_trim(pad: usize) -> c_int;
}

/// `flock` operation: exclusive, do not block.
pub const LOCK_EX_NB: c_int = 2 | 4;
