//! macOS hook: a `CGEventTap` on its own thread with a CoreFoundation run loop.
//!
//! [`spawn`] keeps retrying every 2 seconds until the Accessibility permission is granted. Native
//! events are translated by [`translate`] and routed by [`dispatch`].

use super::{Ev, Gesture, Kind, Verdict, dispatch, reset};
use crate::actions::SYNTH_TAG;
use crate::geometry::Point;
use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT};
use crate::msg::Notifier;
use core_foundation::base::TCFType;
use core_foundation::mach_port::CFMachPortRef;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField,
};
use core_graphics::geometry::CGPoint;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The tap's mach port, kept so the callback can re-enable the tap after macOS disables it.
static TAP_PORT: AtomicUsize = AtomicUsize::new(0);

// Not exposed by the `core-graphics` crate.
unsafe extern "C" {
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
}

/// Converts event flags from a `CGEvent` into a `MOD_*` bitmask.
pub fn mods_from_flags(f: CGEventFlags) -> u8 {
    let mut m = 0;
    if f.contains(CGEventFlags::CGEventFlagShift) {
        m |= MOD_SHIFT;
    }
    if f.contains(CGEventFlags::CGEventFlagControl) {
        m |= MOD_CTRL;
    }
    if f.contains(CGEventFlags::CGEventFlagAlternate) {
        m |= MOD_ALT;
    }
    if f.contains(CGEventFlags::CGEventFlagCommand) {
        m |= MOD_CMD;
    }
    m
}

fn point(p: CGPoint) -> Point {
    Point::new(p.x, p.y)
}

// ------------------------------------------------------------------- startup

/// Starts the tap on its own thread and keeps retrying until the permission is granted.
pub fn spawn(notifier: Notifier) {
    std::thread::Builder::new()
        .name("cursorpie-tap".into())
        .spawn(move || {
            let state = Arc::new(Mutex::new(Gesture::Idle));
            let mut warned = false;
            loop {
                let (st, n) = (state.clone(), notifier.clone());
                let tap = CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    vec![
                        CGEventType::OtherMouseDown,
                        CGEventType::OtherMouseUp,
                        CGEventType::OtherMouseDragged,
                        CGEventType::RightMouseDown,
                        CGEventType::RightMouseUp,
                        CGEventType::RightMouseDragged,
                        CGEventType::KeyDown,
                        CGEventType::KeyUp,
                    ],
                    move |_proxy, etype, event| guarded(&st, &n, etype, event),
                );

                match tap {
                    Ok(tap) => {
                        let Ok(source) = tap.mach_port().create_runloop_source(0) else {
                            std::thread::sleep(Duration::from_secs(2));
                            continue;
                        };
                        TAP_PORT.store(tap.mach_port().as_concrete_TypeRef() as usize, Ordering::SeqCst);
                        CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
                        tap.enable();
                        super::TAP_ACTIVE.store(true, Ordering::SeqCst);
                        diag::log!("tap", "hook", backend = "macos", state = "running");
                        notifier.ctx.request_repaint();
                        CFRunLoop::run_current();
                        super::TAP_ACTIVE.store(false, Ordering::SeqCst);
                        diag::log!("tap", "hook", backend = "macos", state = "stopped");
                    }
                    Err(_) => {
                        super::TAP_ACTIVE.store(false, Ordering::SeqCst);
                        if !warned {
                            warned = true;
                            diag::log!("tap", "hook", backend = "macos", state = "blocked");
                            eprintln!(
                                "CursorPie: macOS blocked the event tap. Grant 'Accessibility' (and 'Input Monitoring') \
                                 to this app in System Settings -> Privacy & Security. Retrying every 2 s..."
                            );
                        }
                        notifier.ctx.request_repaint();
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        })
        .expect("failed to spawn event tap thread");
}

/// Runs the handler and turns a panic into "let the event through".
fn guarded(
    state: &Arc<Mutex<Gesture>>,
    n: &Notifier,
    t: CGEventType,
    e: &CGEvent,
) -> CallbackResult {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handle(state, n, t, e)))
        .unwrap_or(CallbackResult::Keep)
}

/// Converts a native event into the portable one. `None` for event types the tap does not use.
fn translate(etype: CGEventType, event: &CGEvent) -> Option<Ev> {
    let mouse = |kind: Kind, button: i64| Ev::Mouse {
        kind,
        button,
        pos: point(event.location()),
    };
    let other = |kind: Kind| {
        mouse(
            kind,
            event.get_integer_value_field(EventField::MOUSE_EVENT_BUTTON_NUMBER),
        )
    };
    Some(match etype {
        CGEventType::KeyDown | CGEventType::KeyUp => Ev::Key {
            down: matches!(etype, CGEventType::KeyDown),
            code: event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as i32,
            mods: mods_from_flags(event.get_flags()),
        },
        CGEventType::RightMouseDown => mouse(Kind::Down, super::RIGHT_BUTTON),
        CGEventType::RightMouseUp => mouse(Kind::Up, super::RIGHT_BUTTON),
        CGEventType::RightMouseDragged => mouse(Kind::Drag, super::RIGHT_BUTTON),
        CGEventType::OtherMouseDown => other(Kind::Down),
        CGEventType::OtherMouseUp => other(Kind::Up),
        CGEventType::OtherMouseDragged => other(Kind::Drag),
        _ => return None,
    })
}

/// The tap callback: decides what happens to one event (swallow = `Drop`, pass on = `Keep`).
fn handle(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    etype: CGEventType,
    event: &CGEvent,
) -> CallbackResult {
    if matches!(
        etype,
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput
    ) {
        let port = TAP_PORT.load(Ordering::SeqCst);
        if port != 0 {
            unsafe { CGEventTapEnable(port as CFMachPortRef, true) };
        }
        reset(state, notifier);
        return CallbackResult::Keep;
    }
    if event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA) == SYNTH_TAG {
        return CallbackResult::Keep;
    }
    let Some(ev) = translate(etype, event) else {
        return CallbackResult::Keep;
    };
    // a press that turns out to be a real drag is handed back to the app, drag event included
    let replay_drag = || {
        let drag = event.clone();
        drag.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SYNTH_TAG);
        drag.post(CGEventTapLocation::HID);
    };
    match dispatch(state, notifier, ev, &replay_drag) {
        Verdict::Keep => CallbackResult::Keep,
        Verdict::Drop => CallbackResult::Drop,
    }
}
