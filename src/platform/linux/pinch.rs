//! The pinch of a trackpad, read from the X server.
//!
//! Windows hands a two-finger pinch to the app as the wheel with Ctrl held, and macOS as a magnify
//! event; egui turns both into `zoom_delta`. On Linux the window is always an X11 window (also in a
//! Wayland session, where it runs through XWayland, see `event_loop_builder_hook` in `backend.rs`) and winit's X11
//! backend does not know the pinch gestures of XInput 2.4, so a pinch never reached the Flow canvas.
//!
//! A small thread of its own opens a second connection to the X server, asks for the pinch
//! gestures on the root window (nobody below it takes them: no window of the app selects them, so
//! they travel up to the root) and multiplies the steps of the running pinch into one factor.
//! [`take_pinch`] hands that factor to the UI once per frame. Without XInput 2.4 (an old X server or
//! XWayland, or no touchpad) nothing is read and the factor stays 1.0.

use super::xlib::*;
use eframe::egui;
use std::ffi::CString;
use std::os::raw::c_int;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Product of the zoom steps not handed out yet.
static PENDING: Mutex<f64> = Mutex::new(1.0);
/// Wakes the UI when a step arrives (egui sleeps until something asks for a frame).
static REPAINT: OnceLock<egui::Context> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);

/// The zoom factor of the pinch since the last call, 1.0 when there was none. The first call
/// starts the listener, so a session that never opens a zoomable page pays nothing.
pub fn take_pinch(ctx: &egui::Context) -> f32 {
    let _ = REPAINT.set(ctx.clone());
    if !STARTED.swap(true, Ordering::SeqCst) {
        let spawned = std::thread::Builder::new()
            .name("cursorpie-pinch".into())
            .spawn(listen);
        if spawned.is_err() {
            return 1.0;
        }
    }
    let mut p = crate::config::lock(&PENDING);
    let f = *p;
    *p = 1.0;
    f as f32
}

/// The ratio of one step of a pinch to the one before it, `None` for a value that cannot be a scale
/// (the layout of the event would then be wrong, and a wild zoom is worse than none).
fn step(prev: f64, now: f64) -> Option<f64> {
    if !(0.01..=100.0).contains(&now) || !(0.01..=100.0).contains(&prev) {
        return None;
    }
    let r = now / prev;
    // one step of a real pinch is a few percent; never take more than a factor of two at once
    Some(r.clamp(0.5, 2.0))
}

fn listen() {
    // SAFETY: a display of this thread's own, used only here.
    unsafe {
        let dpy = XOpenDisplay(ptr::null());
        if dpy.is_null() {
            return;
        }
        let root = XDefaultRootWindow(dpy);
        let (mut opcode, mut first_event, mut first_error) = (0, 0, 0);
        let Ok(name) = CString::new("XInputExtension") else {
            XCloseDisplay(dpy);
            return;
        };
        if XQueryExtension(
            dpy,
            name.as_ptr(),
            &mut opcode,
            &mut first_event,
            &mut first_error,
        ) == FALSE
        {
            XCloseDisplay(dpy);
            return;
        }
        // gestures came with XInput 2.4; the server answers with the version it speaks
        let (mut major, mut minor) = (2, 4);
        if XIQueryVersion(dpy, &mut major, &mut minor) != 0 || (major, minor) < (2, 4) {
            diag::log!("x11", "pinch_unavailable", major = major, minor = minor);
            XCloseDisplay(dpy);
            return;
        }
        let mut bits = [0u8; XI_MASK_LEN];
        for ev in [
            XI_GESTURE_PINCH_BEGIN,
            XI_GESTURE_PINCH_UPDATE,
            XI_GESTURE_PINCH_END,
        ] {
            bits[(ev >> 3) as usize] |= 1 << (ev & 7);
        }
        let mut mask = XIEventMask {
            deviceid: XI_ALL_MASTER_DEVICES,
            mask_len: XI_MASK_LEN as c_int,
            mask: bits.as_mut_ptr(),
        };
        XISelectEvents(dpy, root, &mut mask, 1);
        XFlush(dpy);

        let mut last = 1.0f64;
        loop {
            let mut ev = XEvent::zeroed();
            // blocks until the server sends something; this connection is only read here
            XNextEvent(dpy, &mut ev);
            if ev.type_ != GENERIC_EVENT {
                continue;
            }
            let cookie = &mut ev as *mut XEvent as *mut XGenericEventCookie;
            if (*cookie).extension != opcode || XGetEventData(dpy, cookie) == FALSE {
                continue;
            }
            let evtype = (*cookie).evtype;
            let data = (*cookie).data as *const XIGesturePinchEvent;
            let mut wake = false;
            match evtype {
                XI_GESTURE_PINCH_BEGIN => {
                    last = 1.0;
                    *crate::config::lock(&PENDING) = 1.0;
                }
                XI_GESTURE_PINCH_UPDATE => {
                    let now = (*data).scale;
                    if let Some(r) = step(last, now) {
                        last = now;
                        *crate::config::lock(&PENDING) *= r;
                        wake = true;
                    }
                }
                _ => {}
            }
            XFreeEventData(dpy, cookie);
            if wake && let Some(ctx) = REPAINT.get() {
                ctx.request_repaint();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_the_ratio_of_two_scales() {
        assert!((step(1.0, 1.1).unwrap() - 1.1).abs() < 1e-9);
        assert!((step(1.1, 1.0).unwrap() - 1.0 / 1.1).abs() < 1e-9);
    }

    #[test]
    fn a_value_that_cannot_be_a_scale_is_ignored() {
        assert!(step(1.0, 0.0).is_none());
        assert!(step(1.0, f64::NAN).is_none());
        assert!(step(1.0, 1e9).is_none());
        assert!(step(0.0, 1.0).is_none());
    }

    #[test]
    fn a_jump_is_limited() {
        assert_eq!(step(1.0, 50.0), Some(2.0));
        assert_eq!(step(1.0, 0.02), Some(0.5));
    }
}
