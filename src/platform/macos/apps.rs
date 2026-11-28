//! Running apps and the pointer: frontmost app, app under the pointer, app icons, activation policy.

use crate::geometry::Point;
use crate::platform::{NativeIcon, ScreenRect, spawn_reaped};
use core_graphics::event::CGEvent;
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
use objc2::runtime::{AnyObject, Sel};
use objc2::sel;
use std::ffi::c_void;
use std::process::Command;
use std::time::Duration;

/// Bundle identifier of the frontmost application (the key used by per-app profiles).
/// NSWorkspace can be read from any thread; the pool drains temporaries when called off the main thread.
pub fn frontmost_bundle_id() -> Option<String> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSWorkspace;

    autoreleasepool(|_| {
        let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        app.bundleIdentifier().map(|s| s.to_string())
    })
}

/// Bundle identifier of the app that owns the topmost window at `p` (global coordinates, the same
/// space as the cursor), whether or not that app is in front. Only regular app windows count: not
/// the Dock, the menu bar or the desktop, and CursorPie's own windows are skipped. None when
/// nothing suitable is under the point or the app has no bundle identifier.
pub fn bundle_id_at(p: Point) -> Option<String> {
    let p = CGPoint::new(p.x, p.y);
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::geometry::CGRect;
    use core_graphics::window::{
        copy_window_info, kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowOwnerPID,
    };

    // ordered front to back
    let windows = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    )?;
    let (k_layer, k_alpha, k_pid, k_bounds) = unsafe {
        (
            CFString::wrap_under_get_rule(kCGWindowLayer),
            CFString::wrap_under_get_rule(kCGWindowAlpha),
            CFString::wrap_under_get_rule(kCGWindowOwnerPID),
            CFString::wrap_under_get_rule(kCGWindowBounds),
        )
    };
    let me = std::process::id() as i64;

    for w in windows.iter() {
        let d: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(*w as CFDictionaryRef) };
        let number = |k: &CFString| d.find(k).and_then(|v| v.downcast::<CFNumber>());

        // layer 0 = normal windows; up to 19 also covers floating palettes and panels
        let Some(layer) = number(&k_layer).and_then(|n| n.to_i64()) else {
            continue;
        };
        if !(0..=19).contains(&layer) {
            continue;
        }
        // fully transparent overlays do not count as "the window under the pointer"
        if number(&k_alpha)
            .and_then(|n| n.to_f64())
            .is_some_and(|a| a < 0.01)
        {
            continue;
        }
        let Some(pid) = number(&k_pid).and_then(|n| n.to_i64()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Some(bounds) = d
            .find(&k_bounds)
            .and_then(|v| v.downcast::<CFDictionary>())
            .and_then(|b| CGRect::from_dict_representation(&b))
        else {
            continue;
        };
        if bounds.contains(&p) {
            return bundle_id_of_pid(pid as i32);
        }
    }
    None
}

/// Bundle identifier of the running app with this process id.
fn bundle_id_of_pid(pid: i32) -> Option<String> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSRunningApplication;

    autoreleasepool(|_| {
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
        app.bundleIdentifier().map(|s| s.to_string())
    })
}

/// The icon of an app as TIFF data holding every size macOS has for it (one page per size), found by
/// bundle identifier. This is the real icon, the one the Dock and Finder show. None if no such app is
/// installed. Meant for the main thread; the result is decoded by `appicon`.
pub fn app_icon_tiff(bundle: &str) -> Option<Vec<u8>> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::NSString;

    autoreleasepool(|_| {
        let ws = NSWorkspace::sharedWorkspace();
        let url = ws.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle))?;
        let path = url.path()?;
        let icon = ws.iconForFile(&path);
        let data = icon.TIFFRepresentation()?;
        Some(data.to_vec())
    })
}

/// Edge (px) of the bitmap an app icon is drawn into by [`app_icon_rgba`]. The pie reduces it to
/// 128 px, so this leaves room for a clean reduction and nothing more.
const ICON_PX: usize = 256;

/// A rectangle as Core Graphics takes it (`CGRect`: `CGFloat` is a `f64` on every Mac we run on).
#[repr(C)]
#[derive(Clone, Copy)]
struct CgRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

unsafe extern "C" {
    /// The trampoline every message send goes through. It is cast to the real signature at the
    /// one call that needs it (a typed `msg_send!` would need Cargo features of `objc2-app-kit`
    /// for `CGImage`, and checks the return type against `^{CGImage=}`).
    fn objc_msgSend();
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: *mut c_void,
        bitmap_info: u32,
    ) -> *mut c_void;
    fn CGContextSetInterpolationQuality(ctx: *mut c_void, quality: i32);
    fn CGContextDrawImage(ctx: *mut c_void, rect: CgRect, image: *const c_void);
    fn CGContextRelease(ctx: *mut c_void);
}

/// `kCGImageAlphaPremultipliedLast`: R, G, B, A with the color already multiplied by the alpha.
const PREMULTIPLIED_LAST: u32 = 1;
/// `kCGInterpolationHigh`.
const INTERPOLATION_HIGH: i32 = 3;

/// The icon of an app drawn at **one size**, as straight RGBA (`ICON_PX` x `ICON_PX`). None if
/// the app is not installed or the system gives no usable picture.
///
/// This is what the pie uses instead of [`app_icon_tiff`]. `iconForFile` hands out an image with
/// every size macOS has for the app, and `TIFFRepresentation` writes all of them out: about 70 MiB
/// for one icon (the diagnostic log of a press showed `source_bytes` of 73949448 for each of six
/// icons), which AppKit and Icon Services keep in caches that the allocator cannot give back.
/// Asking the image for a single picture at the size we need costs a few hundred KiB.
pub fn app_icon_rgba(bundle: &str) -> Option<NativeIcon> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::{NSImage, NSWorkspace};
    use objc2_foundation::NSString;

    autoreleasepool(|_| {
        let ws = NSWorkspace::sharedWorkspace();
        let url = ws.URLForApplicationWithBundleIdentifier(&NSString::from_str(bundle))?;
        let path = url.path()?;
        let icon = ws.iconForFile(&path);
        let object = &*icon as *const NSImage as *const AnyObject;
        // SAFETY: `object` is a live NSImage for the whole closure (`icon` is not dropped before
        // it ends), and the picture it hands out is used before the pool is drained.
        let data = unsafe { draw_icon(object) }?;
        Some(NativeIcon::Rgba {
            width: ICON_PX as u32,
            height: ICON_PX as u32,
            data,
        })
    })
}

/// Asks `icon` (an `NSImage`) for the picture that suits an `ICON_PX` square and draws it into a
/// bitmap of that size. Straight RGBA, or None when the image gives no picture or it is empty.
///
/// # Safety
/// `icon` must point to a live `NSImage`.
unsafe fn draw_icon(icon: *const AnyObject) -> Option<Vec<u8>> {
    type ProposedRect = unsafe extern "C" fn(
        *const AnyObject,
        Sel,
        *mut CgRect,
        *const c_void,
        *const c_void,
    ) -> *const c_void;
    // SAFETY: `objc_msgSend` takes the receiver, the selector and the arguments of the method,
    // and `-CGImageForProposedRect:context:hints:` is declared exactly like `ProposedRect`.
    let send = unsafe { std::mem::transmute::<unsafe extern "C" fn(), ProposedRect>(objc_msgSend) };
    let side = ICON_PX as f64;
    let mut proposed = CgRect {
        x: 0.0,
        y: 0.0,
        w: side,
        h: side,
    };
    // A +0 reference, valid until the autorelease pool of the caller is drained: it is never
    // retained or released here.
    // SAFETY: see above; a null context and no hints are allowed.
    let image = unsafe {
        send(
            icon,
            sel!(CGImageForProposedRect:context:hints:),
            &mut proposed,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if image.is_null() {
        return None;
    }

    let mut buf = vec![0u8; ICON_PX * ICON_PX * 4];
    // SAFETY: the bitmap context draws into `buf`, which is `ICON_PX * 4` bytes wide and
    // `ICON_PX` tall, and `buf` outlives it: the context is released before `buf` is used again.
    unsafe {
        let space = CGColorSpaceCreateDeviceRGB();
        if space.is_null() {
            return None;
        }
        let ctx = CGBitmapContextCreate(
            buf.as_mut_ptr().cast(),
            ICON_PX,
            ICON_PX,
            8,
            ICON_PX * 4,
            space,
            PREMULTIPLIED_LAST,
        );
        CGColorSpaceRelease(space);
        if ctx.is_null() {
            return None;
        }
        CGContextSetInterpolationQuality(ctx, INTERPOLATION_HIGH);
        let whole = CgRect {
            x: 0.0,
            y: 0.0,
            w: side,
            h: side,
        };
        CGContextDrawImage(ctx, whole, image);
        CGContextRelease(ctx);
    }
    if buf.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
        return None; // nothing was drawn: let the caller use the TIFF
    }
    unpremultiply(&mut buf);
    Some(buf)
}

/// Turns premultiplied RGBA (what a bitmap context writes) into straight RGBA, in place.
fn unpremultiply(rgba: &mut [u8]) {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = u32::from(px[3]);
        if a == 0 || a == 255 {
            continue;
        }
        for c in &mut px[..3] {
            *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
        }
    }
}

/// The icon of the app with this bundle id: one picture at the size the pie needs
/// ([`app_icon_rgba`]) and, only when the system gives none, the multi-size TIFF
/// ([`app_icon_tiff`]).
pub fn app_icon(bundle: &str) -> Option<NativeIcon> {
    app_icon_rgba(bundle).or_else(|| app_icon_tiff(bundle).map(NativeIcon::Tiff))
}

/// Hides the Dock icon / app switcher entry (same effect as LSUIElement, also when run via `cargo run`).
pub fn set_accessory_policy() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }
}

/// macOS windows have no DWM-style frame to remove: the window is already transparent and borderless.
pub fn clear_window_frame(_handle: isize) {}

/// The window of the notice on the screen (`ui::hud`) must not take the keyboard from the app the
/// action is for. Nothing to ask here: the app has the accessory policy, so a window that is shown does not make it the active app.
pub fn set_window_passive(_on: bool) {}

// `NSWindowCollectionBehavior` bits (AppKit's `NSWindow.h`).
const CB_CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const CB_MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
const CB_MANAGED: usize = 1 << 2;
const CB_TRANSIENT: usize = 1 << 3;
const CB_STATIONARY: usize = 1 << 4;
const CB_PARTICIPATES_IN_CYCLE: usize = 1 << 5;
const CB_IGNORES_CYCLE: usize = 1 << 6;
const CB_FULL_SCREEN_PRIMARY: usize = 1 << 7;
const CB_FULL_SCREEN_AUXILIARY: usize = 1 << 8;
const CB_FULL_SCREEN_NONE: usize = 1 << 9;
/// macOS 13+: `.primary` / `.auxiliary` / `.canJoinAllApplications` are one group (Stage Manager and
/// full screen); a window has at most one of them. A bit AppKit does not know (macOS 12 and older)
/// is ignored.
const CB_PRIMARY: usize = 1 << 16;
const CB_AUXILIARY: usize = 1 << 17;
const CB_CAN_JOIN_ALL_APPLICATIONS: usize = 1 << 18;

/// `NSFloatingWindowLevel`: what the toolkit gives an always-on-top window, and what Settings has.
const LEVEL_FLOATING: isize = 3;
/// `NSScreenSaverWindowLevel` (`kCGScreenSaverWindowLevel`): the level of a window that is drawn
/// over an app in full screen. The status level (25) used before is *below* what the Space of a
/// full-screen app draws on current macOS, so a window there was ordered in and never seen.
const LEVEL_OVER_FULL_SCREEN: isize = 1000;

/// Lets the window of the pie (and of the notices and panels) appear over an app in full screen.
///
/// A full-screen app lives in a Space of its own, and a window of **another app** is drawn there
/// only when three things hold at once:
///
/// 1. its *collection behavior* says so: `CanJoinAllSpaces` (it follows the Space the person is
///    in), `FullScreenAuxiliary` (it may be drawn beside a full-screen window) and, since macOS 13,
///    `CanJoinAllApplications` (it may be drawn in the Space of an app that is not its own; without
///    it the window of one app never reaches the full-screen Space of another, whatever the other
///    bits say). The version 0.9.0 set the first two only, so the pie was shown and nothing was
///    seen over a full-screen browser, video or game;
/// 2. its level is above what a full-screen Space draws: the screen saver level
///    (`LEVEL_OVER_FULL_SCREEN`). The toolkit's always-on-top is the floating level, and the
///    status level is not enough either;
/// 3. the app is an accessory app (no Dock icon: `LSUIElement`, see [`set_accessory_policy`]), so
///    that showing the window does not activate the app and take the person out of the Space.
///
/// The level is owned by this function alone (the caller does not send the toolkit's
/// `WindowLevel` on macOS, which rewrote it to floating each time the pie was shown, so the window
/// was ordered in at the wrong level).
///
/// `over` is false while Settings (or the welcome window) is open: it is a normal card that
/// belongs to one Space and stays below the system's own panels (a file chooser must be able to
/// cover it), so it gets the bits and the level back it had. Only the toolkit's own window is
/// touched, never the status item's window or any other window the app owns. Cheap enough to run
/// every frame (a few messages, and nothing is sent when the window is already right).
pub fn keep_over_full_screen(over: bool) {
    use objc2::rc::{Retained, autoreleasepool};
    use objc2::{MainThreadMarker, msg_send};
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    autoreleasepool(|_| {
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: `-[NSApplication windows]` takes nothing and returns an `NSArray` of windows;
        // `-count` returns an `NSUInteger` and `-objectAtIndex:` takes one and returns the window.
        // The calls are made on the main thread, as AppKit wants for windows.
        let windows: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, windows] };
        let Some(windows) = windows else {
            return;
        };
        let count: usize = unsafe { msg_send![&*windows, count] };
        for i in 0..count {
            let win: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*windows, objectAtIndex: i] };
            let Some(win) = win else {
                continue;
            };
            // The toolkit's window only. AppKit gives an object that is observed with KVO a
            // subclass of its own, `NSKVONotifying_WinitWindow`, and the window of this app is
            // observed, so the class is not `WinitWindow` itself: the test is on the name it
            // contains. (It used to be `starts_with("Winit")`, which never matched, so none of the
            // settings below reached the pie window; and a test that took every class that is not
            // `NS...` reached the window of the tray icon instead.) The status item
            // (`NSStatusBarWindow`), the menus and the tray's window are left alone.
            let class_name = win.class().name().to_string_lossy().into_owned();
            note_window_class(&class_name);
            if !is_toolkit_window_class(&class_name) {
                continue;
            }
            let behavior: usize = unsafe { msg_send![&*win, collectionBehavior] };
            let want_behavior = if over {
                over_full_screen_behavior(behavior)
            } else {
                normal_behavior(behavior)
            };
            if behavior != want_behavior {
                let _: () = unsafe { msg_send![&*win, setCollectionBehavior: want_behavior] };
            }
            let level: isize = unsafe { msg_send![&*win, level] };
            if let Some(want_level) = level_for(over, level) {
                let _: () = unsafe { msg_send![&*win, setLevel: want_level] };
            }
            log_window_state(&win, app_active_policy(&app), over);
        }
    });
}

/// Logs the class of every window the app owns, once per distinct class (debug / log builds).
fn note_window_class(name: &str) {
    use std::sync::Mutex;
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let Ok(mut seen) = SEEN.lock() else {
        return;
    };
    if !seen.iter().any(|n| n == name) {
        seen.push(name.to_string());
        diag::log!("ui", "mac_window_class", class = name);
    }
}

/// `NSApplication.activationPolicy` (0 regular, 1 accessory, 2 prohibited).
fn app_active_policy(app: &objc2_app_kit::NSApplication) -> (isize, bool) {
    use objc2::msg_send;
    // SAFETY: both are plain getters of `NSApplication` (`NSInteger` and `BOOL`), main thread.
    let policy: isize = unsafe { msg_send![app, activationPolicy] };
    let active: bool = unsafe { msg_send![app, isActive] };
    (policy, active)
}

/// Writes to the diagnostic log (debug / log builds only, see `docs/DEBUGGING.md`) what AppKit
/// says about the window each time it changes: the collection behavior and the level it really
/// has, whether it is on screen, **on the Space the person is in**, not occluded, and whether the
/// app is active. A pie that is "open but not seen over a full-screen app" shows here which of
/// those is not what it should be (`on_space=false` / `occlusion` without bit 2 = the window is
/// not drawn in that Space). One line per change, so a frame loop does not flood the log.
fn log_window_state(win: &objc2::runtime::AnyObject, app: (isize, bool), over: bool) {
    use objc2::msg_send;
    use std::sync::Mutex;
    static LAST: Mutex<String> = Mutex::new(String::new());

    // SAFETY: plain getters of `NSWindow` (`BOOL`, `NSUInteger`, `NSInteger`), main thread.
    let visible: bool = unsafe { msg_send![win, isVisible] };
    let on_space: bool = unsafe { msg_send![win, isOnActiveSpace] };
    let key: bool = unsafe { msg_send![win, isKeyWindow] };
    let occlusion: usize = unsafe { msg_send![win, occlusionState] };
    let behavior: usize = unsafe { msg_send![win, collectionBehavior] };
    let level: isize = unsafe { msg_send![win, level] };
    let line = format!(
        "over={over} visible={visible} on_space={on_space} key={key} occlusion={occlusion} \
         behavior={behavior:#x} level={level} policy={} app_active={}",
        app.0, app.1
    );
    let Ok(mut last) = LAST.lock() else {
        return;
    };
    if *last != line {
        diag::log!("ui", "mac_window_state", state = line.as_str());
        *last = line;
    }
}

/// Whether `name` is the class of the toolkit's window: `WinitWindow`, or the `NSKVONotifying_`
/// subclass AppKit makes of it while it is observed.
fn is_toolkit_window_class(name: &str) -> bool {
    name.strip_prefix("NSKVONotifying_").unwrap_or(name) == "WinitWindow"
}

/// The level the window has to be given, if it is not the right one already.
///
/// Over a full-screen app (`over`) it is always [`LEVEL_OVER_FULL_SCREEN`], whatever the toolkit
/// left (floating, or normal after Settings). With Settings open it only gives back a level this
/// module raised: Settings keeps whatever level it has otherwise (the toolkit's normal one).
fn level_for(over: bool, current: isize) -> Option<isize> {
    if over {
        (current != LEVEL_OVER_FULL_SCREEN).then_some(LEVEL_OVER_FULL_SCREEN)
    } else {
        (current == LEVEL_OVER_FULL_SCREEN).then_some(LEVEL_FLOATING)
    }
}

/// The collection behavior that shows a window over a full-screen app: in every Space, in the
/// Space of every app, beside a full-screen window, out of Cmd+` and untouched by Mission
/// Control, and keeping the bits that do not conflict with those.
/// (`CanJoinAllSpaces` excludes `MoveToActiveSpace`; `Stationary` excludes `Managed` and
/// `Transient`; `FullScreenAuxiliary` excludes `FullScreenPrimary` and `FullScreenNone`;
/// `CanJoinAllApplications` excludes `Primary` and `Auxiliary`. AppKit raises an exception for a
/// window that has two bits that exclude each other.)
fn over_full_screen_behavior(current: usize) -> usize {
    let keep = current
        & !(CB_MOVE_TO_ACTIVE_SPACE
            | CB_MANAGED
            | CB_TRANSIENT
            | CB_PARTICIPATES_IN_CYCLE
            | CB_FULL_SCREEN_PRIMARY
            | CB_FULL_SCREEN_NONE
            | CB_PRIMARY
            | CB_AUXILIARY);
    keep | OVER_FULL_SCREEN_BITS
}

/// The bits [`over_full_screen_behavior`] adds, and [`normal_behavior`] takes away again.
const OVER_FULL_SCREEN_BITS: usize = CB_CAN_JOIN_ALL_SPACES
    | CB_CAN_JOIN_ALL_APPLICATIONS
    | CB_FULL_SCREEN_AUXILIARY
    | CB_STATIONARY
    | CB_IGNORES_CYCLE;

/// What a window that is not shown over a full-screen app has: the bits of
/// [`over_full_screen_behavior`] are taken away again.
fn normal_behavior(current: usize) -> usize {
    current & !OVER_FULL_SCREEN_BITS
}

#[cfg(test)]
mod full_screen_tests {
    use super::*;

    #[test]
    fn the_pie_joins_every_space_and_every_app_and_sits_beside_a_full_screen_window() {
        let b = over_full_screen_behavior(0);
        assert_ne!(b & CB_CAN_JOIN_ALL_SPACES, 0);
        assert_ne!(
            b & CB_CAN_JOIN_ALL_APPLICATIONS,
            0,
            "the Space of another app"
        );
        assert_ne!(b & CB_FULL_SCREEN_AUXILIARY, 0);
        assert_ne!(b & CB_STATIONARY, 0);
        assert_ne!(b & CB_IGNORES_CYCLE, 0);
    }

    #[test]
    fn bits_that_conflict_are_removed_so_appkit_does_not_raise() {
        let b = over_full_screen_behavior(
            CB_MOVE_TO_ACTIVE_SPACE
                | CB_MANAGED
                | CB_TRANSIENT
                | CB_FULL_SCREEN_PRIMARY
                | CB_FULL_SCREEN_NONE
                | CB_PARTICIPATES_IN_CYCLE
                | CB_PRIMARY
                | CB_AUXILIARY,
        );
        for bit in [
            CB_MOVE_TO_ACTIVE_SPACE,
            CB_MANAGED,
            CB_TRANSIENT,
            CB_FULL_SCREEN_PRIMARY,
            CB_FULL_SCREEN_NONE,
            CB_PARTICIPATES_IN_CYCLE,
            CB_PRIMARY,
            CB_AUXILIARY,
        ] {
            assert_eq!(b & bit, 0);
        }
    }

    #[test]
    fn the_toolkit_window_is_found_by_its_class_even_when_it_is_observed() {
        assert!(is_toolkit_window_class("WinitWindow"));
        assert!(is_toolkit_window_class("NSKVONotifying_WinitWindow"));
        assert!(!is_toolkit_window_class("TUINSWindow"));
        assert!(!is_toolkit_window_class("NSKVONotifying_TUINSWindow"));
        assert!(!is_toolkit_window_class("NSStatusBarWindow"));
    }

    #[test]
    fn the_behavior_is_stable_once_applied() {
        // run every frame: the second pass must send nothing
        let once = over_full_screen_behavior(CB_MANAGED | CB_PARTICIPATES_IN_CYCLE);
        assert_eq!(over_full_screen_behavior(once), once);
    }

    #[test]
    fn settings_gives_the_over_full_screen_bits_back() {
        let b = normal_behavior(over_full_screen_behavior(0));
        assert_eq!(b & OVER_FULL_SCREEN_BITS, 0);
    }

    #[test]
    fn the_pie_is_raised_over_whatever_level_the_toolkit_left() {
        assert_eq!(
            level_for(true, LEVEL_FLOATING),
            Some(LEVEL_OVER_FULL_SCREEN)
        );
        assert_eq!(level_for(true, 0), Some(LEVEL_OVER_FULL_SCREEN));
        assert_eq!(level_for(true, 25), Some(LEVEL_OVER_FULL_SCREEN));
        assert_eq!(level_for(true, LEVEL_OVER_FULL_SCREEN), None);
    }

    #[test]
    fn settings_only_gives_back_the_level_this_module_raised() {
        assert_eq!(
            level_for(false, LEVEL_OVER_FULL_SCREEN),
            Some(LEVEL_FLOATING)
        );
        assert_eq!(
            level_for(false, 0),
            None,
            "the toolkit's normal level stays"
        );
        assert_eq!(level_for(false, LEVEL_FLOATING), None);
    }
}

/// macOS keeps the position a hidden window was given.
pub fn window_manager_may_recenter() -> bool {
    false
}

/// Only Hyprland places the window through the compositor.
pub fn place_window_center(_top_left: (f64, f64), _size: (f64, f64)) -> bool {
    false
}
/// Nothing is being placed.
pub fn window_placement_pending() -> bool {
    false
}
/// A window is placed in the same points the pointer is in.
pub fn window_point(x: f64, y: f64) -> (f64, f64) {
    (x, y)
}

/// The pie is drawn in the same points the pointer moves in.
pub fn window_scale(_x: f64, _y: f64) -> f64 {
    1.0
}

/// Activates CursorPie with all its windows. The toolkit's focus request is not enough on its own:
/// it asks to take the focus from the app in front, and a recent macOS refuses that when the app in
/// front does not give it up (after a file chooser is cancelled the app below Settings is the one in
/// front, and Settings stayed behind its window). Activating through `NSRunningApplication` goes
/// through the system's own activation, which also switches to the Space the window is on.
pub fn raise_own_window() {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    let me = NSRunningApplication::currentApplication();
    let _ = me.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
}

/// Current cursor position in global coordinates.
pub fn cursor_pos() -> Option<(f64, f64)> {
    let src = CGEventSource::new(CGEventSourceStateID::HIDSystemState).ok()?;
    let p = CGEvent::new(src).ok()?.location();
    Some((p.x, p.y))
}

/// The system answers at once: the same as [`cursor_pos`].
pub fn cursor_pos_fresh() -> Option<(f64, f64)> {
    cursor_pos()
}

/// The system needs no asking for the pointer.
pub fn pointer_hot(_on: bool) {}

/// Only a Wayland session shows a black square when a transparent window is first mapped.
pub fn set_window_veil(_window: std::os::raw::c_ulong, _on: bool) -> bool {
    false
}

/// Activates an app and waits (briefly) until macOS really has it in front: `open` returns
/// before the switch is done, and keys sent too early would reach the previous app.
pub fn activate_app(bundle: &str) {
    if frontmost_bundle_id().as_deref() == Some(bundle) {
        return;
    }
    spawn_reaped(Command::new("open").arg("-b").arg(bundle));
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(15));
        if frontmost_bundle_id().as_deref() == Some(bundle) {
            std::thread::sleep(Duration::from_millis(40)); // let its window become the key window
            return;
        }
    }
}

/// Bounds of every active display in global coordinates (the space of the cursor, in points).
pub fn display_rects() -> Vec<ScreenRect> {
    use core_graphics::display::CGDisplay;

    let Ok(ids) = CGDisplay::active_displays() else {
        return Vec::new();
    };
    ids.into_iter()
        .map(|id| {
            let b = CGDisplay::new(id).bounds();
            ScreenRect {
                x: b.origin.x,
                y: b.origin.y,
                w: b.size.width,
                h: b.size.height,
            }
        })
        .collect()
}

/// winit's own choice of window system stays: there is only one on this OS.
pub fn event_loop_builder_hook() -> Option<eframe::EventLoopBuilderHook> {
    None
}

#[cfg(test)]
mod tests {
    use super::unpremultiply;

    #[test]
    fn unpremultiply_restores_the_color_and_leaves_opaque_and_clear_pixels() {
        // half transparent white (255 * 128 / 255 = 128), opaque red, fully clear
        let mut px = [128, 128, 128, 128, 255, 0, 0, 255, 0, 0, 0, 0];
        unpremultiply(&mut px);
        assert_eq!(&px[..4], &[255, 255, 255, 128]);
        assert_eq!(&px[4..8], &[255, 0, 0, 255]);
        assert_eq!(&px[8..], &[0, 0, 0, 0]);
    }
}
