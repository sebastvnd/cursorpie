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

/// macOS keeps the position a hidden window was given.
pub fn window_manager_may_recenter() -> bool {
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
