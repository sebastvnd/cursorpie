//! Running apps and the pointer: frontmost app, app under the pointer, app icons, activation policy.

use crate::geometry::Point;
use crate::platform::{NativeIcon, ScreenRect, spawn_reaped};
use core_graphics::event::CGEvent;
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;
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

/// The icon of the app with this bundle id, as the TIFF macOS provides (see [`app_icon_tiff`]).
pub fn app_icon(bundle: &str) -> Option<NativeIcon> {
    app_icon_tiff(bundle).map(NativeIcon::Tiff)
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

/// macOS keeps the position a hidden window was given.
pub fn window_manager_may_recenter() -> bool {
    false
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
