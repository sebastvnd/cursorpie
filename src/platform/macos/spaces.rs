//! Apps on other Spaces: "Open app" for an app whose window is not on the Space that is shown.
//!
//! What the public API gives: `CGWindowListCopyWindowInfo` lists the windows of every Space and
//! says which are on screen, so CursorPie can tell that the app has a window, and that it is
//! somewhere else. What it does not give is a way to go there: the Accessibility API only lists the
//! windows of the current Space, and nothing switches to a given Space. Two things are done:
//!
//! * **Switch to the app's Space.** Four ways, each tried only when the one before did not bring a
//!   window of the app onto the screen:
//!   1. `open -a`, which is what a Dock click does: with the system setting *"When switching to an
//!      application, switch to a Space with open windows for the application"* (on by default)
//!      macOS goes there by itself.
//!   2. The Accessibility action `AXRaise` on the app's main window. The app element can name a
//!      window of another Space even though the window list of the Accessibility API cannot, and
//!      raising a window shows the Space it is on.
//!   3. What the user would do: Control+Right or Control+Left, one Space at a time, until a window
//!      of the app is on screen ([`walk_to_target`]). The
//!      window server tells the order of the Spaces and the Spaces of the app's windows, so the
//!      nearest one is gone to directly, to the left or right and however many Spaces away; without
//!      that the walk feels its way, right first and then left. That needs the shortcuts to be on
//!      (System Settings > Keyboard > Keyboard Shortcuts > Mission Control).
//!   4. The same walk with the keys sent through System Events (`osascript`), for a macOS that
//!      ignores keys posted by other programs. It asks for the Automation permission once.
//!
//!   **More than one screen.** With separate Spaces per display (the macOS default) every screen
//!   has its own row of Spaces and Control+Arrow acts on the screen under the pointer. So the
//!   screen that holds the app's window is found first ([`locate_target`]), the walk uses the row
//!   and the shown Space of that screen only, and the pointer is moved onto that screen while the
//!   keys are sent and put back afterwards: the switch animation happens on the screen of the
//!   window, not on the screen that was clicked.
//!
//!   **An app that is on the other screen.** `open -a` cannot help when the app's window is on a
//!   Space that is shown, but on another display than the one that was clicked: the app is already
//!   "here" for the window server, and activating it only focuses a window the person is not looking
//!   at. So with more than one display the app's front window on the display under the pointer is
//!   raised and given the focus, and when it has none there the window is moved onto that display first
//!   ([`show_on_pointer_display`]), through Accessibility (public API, no private calls). When that does not work (a
//!   full-screen window, no Accessibility permission) the app is opened as before.
//!
//!   Once the Space is shown the window is raised itself ([`focus_app`]): after the switch animation
//!   it is raised through Accessibility, the app is made frontmost, and the front window is checked.
//!
//!   `CURSORPIE_DEBUG=1` in the environment prints what each step saw to stderr.
//! * **Bring the window here** (experimental). Only private SkyLight calls can move a window of
//!   another app to a Space, the ones Hammerspoon and yabai use since macOS 14.5. They are looked up
//!   with `dlsym`, so a macOS that lacks them just reports "not available", and a macOS that has them
//!   but ignores them is caught by asking where the window is afterwards. Either way the answer is
//!   `false` and the caller switches to the app's Space instead.
//!
//! Everything is best effort and ends in `open -a`, the behavior from before this setting.

use super::input::press_shortcut;
use super::shell::open_app;
use crate::config::DesktopMode;
use crate::keys::{MOD_CMD, Shortcut};
use crate::platform::desktops::{
    Desktops, DisplayRow, MAX_STEPS, OnScreen, Presence, WindowInfo, app_name_of_path,
    pick_display, presence_of, same_app_path, signature, steps_between, walk_to_target,
    window_for_screen,
};
use crate::platform::{ScreenRect, spawn_reaped};
use crate::windowing::{Asked, Windows};
use core_foundation::array::{CFArray, CFArrayRef};
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowIsOnscreen,
    kCGWindowLayer, kCGWindowListExcludeDesktopElements, kCGWindowListOptionAll,
    kCGWindowListOptionOnScreenOnly, kCGWindowNumber, kCGWindowOwnerPID,
};
use std::ffi::{c_char, c_int, c_void};
use std::hash::{Hash, Hasher};
use std::process::Command;
use std::time::{Duration, Instant};

/// How long `open -a` gets to bring a window of the app onto the screen by itself before the
/// Spaces are walked: the switch animation takes about half a second.
const OPEN_WAIT: Duration = Duration::from_millis(1200);
/// How long to wait for one Control+Arrow to take effect before taking it for "did not move": the
/// last Space answers with nothing at all.
const SETTLE: Duration = Duration::from_millis(1000);
/// How long the switch animation of a Space is given to finish before the window is raised: during
/// it the new Space is already reported as shown, but the app does not take focus yet.
const ARRIVE: Duration = Duration::from_millis(700);
/// How long a window gets to come to the front after it was raised.
const FRONT_WAIT: Duration = Duration::from_millis(500);
/// How long the animation of a Space switch is given before the next arrow key is pressed.
const STEP_PAUSE: Duration = Duration::from_millis(350);
/// How long the window server gets to notice that the pointer is on another display before the
/// arrow key is pressed: Control+Arrow acts on the display under the pointer.
const POINTER_SETTLE: Duration = Duration::from_millis(150);
/// How often the screen is looked at while waiting.
const POLL: Duration = Duration::from_millis(50);
/// How long to wait between two counts of an app's windows while they settle after the app was
/// brought forward (see [`settled_window_count`]), and how many more looks are made at most.
const COUNT_SETTLE: Duration = Duration::from_millis(40);
const COUNT_SETTLE_LOOKS: usize = 4;
/// How long a window that was moved to another display gets to show up there.
const MOVE_WAIT: Duration = Duration::from_millis(600);
/// Space kept free at the top of a display for the menu bar when a window is moved onto it.
const MENU_BAR: f64 = 40.0;
/// How far (summed over x and y) the position a window reports may be from the one it was given.
const SAME_PLACE: f64 = 6.0;

/// Smallest a window is to count as a main window of an app (see [`window_count`]): the alerts,
/// sheets and small dialogs an app shows are smaller than this, and macOS does not say which
/// windows are dialogs without the Accessibility API.
const MAIN_MIN_W: f64 = 200.0;
const MAIN_MIN_H: f64 = 140.0;

/// Prints what the Space code saw when `CURSORPIE_DEBUG` is set, to find out why an app was not
/// reached. `message` is only built then.
fn trace(message: impl FnOnce() -> String) {
    if std::env::var_os("CURSORPIE_DEBUG").is_some() {
        eprintln!("CursorPie[spaces]: {}", message());
    }
}

/// The running app `app` names (bundle id, localized name or a `.app` path) that is not hidden:
/// its process id. `None` when it does not run, or is hidden, in which case `open -a` starts or
/// unhides it, which is all there is to do.
///
/// A path is compared as the system reports it (no trailing slash, any letter case), and by the
/// name in it, so `/Applications/Helium.app/` finds Helium even when the system knows the app under
/// another path (a symbolic link, a copy of the folder).
fn visible_running_pid(app: &str) -> Option<i32> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSWorkspace;

    let target = app.trim();
    let by_path = target.contains('/');
    let name = if by_path {
        app_name_of_path(target)
    } else {
        target
    };
    autoreleasepool(|_| {
        // NSRunningApplication documents its properties as thread-safe.
        for a in NSWorkspace::sharedWorkspace().runningApplications().iter() {
            let named = a
                .localizedName()
                .is_some_and(|s| !name.is_empty() && s.to_string().eq_ignore_ascii_case(name));
            let hit = if by_path {
                a.bundleURL()
                    .and_then(|u| u.path())
                    .is_some_and(|p| same_app_path(&p.to_string(), target))
                    || named
            } else {
                a.bundleIdentifier()
                    .is_some_and(|s| s.to_string().eq_ignore_ascii_case(target))
                    || named
            };
            if hit {
                let pid = a.processIdentifier();
                if a.isHidden() {
                    trace(|| format!("{app:?} runs as pid {pid} but is hidden"));
                    return None;
                }
                return Some(pid);
            }
        }
        trace(|| {
            format!(
                "no running app matches {app:?} (by {})",
                if by_path { "path" } else { "name or bundle id" }
            )
        });
        None
    })
}

/// Whether the running app `a` is the one `target` names: a bundle id or a localized name, or, when
/// `target` is a path (`by_path`), the `.app` it was started from or the name in it. `name` is the
/// name to compare with the localized one. The same test as [`visible_running_pid`] makes.
fn is_app(
    a: &objc2_app_kit::NSRunningApplication,
    target: &str,
    by_path: bool,
    name: &str,
) -> bool {
    let named = a
        .localizedName()
        .is_some_and(|s| !name.is_empty() && s.to_string().eq_ignore_ascii_case(name));
    if by_path {
        a.bundleURL()
            .and_then(|u| u.path())
            .is_some_and(|p| same_app_path(&p.to_string(), target))
            || named
    } else {
        a.bundleIdentifier()
            .is_some_and(|s| s.to_string().eq_ignore_ascii_case(target))
            || named
    }
}

/// For each of `apps` (a bundle id, an app name or a `.app` path, as typed in "Open app" slices),
/// whether the app runs. A hidden app, and an app with no window, run. One list of the running apps
/// serves every question. Meant for a worker thread.
pub fn running_among(apps: &[String]) -> Vec<bool> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSWorkspace;

    autoreleasepool(|_| {
        // NSRunningApplication documents its properties as thread-safe.
        let running = NSWorkspace::sharedWorkspace().runningApplications();
        apps.iter()
            .map(|app| {
                let target = app.trim();
                if target.is_empty() {
                    return false;
                }
                let by_path = target.contains('/');
                let name = if by_path {
                    app_name_of_path(target)
                } else {
                    target
                };
                running.iter().any(|a| is_app(&a, target, by_path, name))
            })
            .collect()
    })
}

/// The process id of the app `app` names, hidden or not. `None` when it does not run.
fn running_pid(app: &str) -> Option<i32> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSWorkspace;

    let target = app.trim();
    let by_path = target.contains('/');
    let name = if by_path {
        app_name_of_path(target)
    } else {
        target
    };
    autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .runningApplications()
            .iter()
            .find(|a| is_app(a, target, by_path, name))
            .map(|a| a.processIdentifier())
    })
}

/// How many main windows the app `app` names has right now, on any Space, minimized or not: the
/// normal windows (layer 0) that are not transparent and not small (the invisible helper windows
/// many apps keep, and their alerts and small dialogs; this is a size guess, not a type: macOS only
/// says what a window is through the Accessibility API). It is used by comparing two counts, so a constant extra window does not matter. `None`
/// when the app does not run or the window list cannot be read. Meant for a worker thread.
pub fn window_count(app: &str) -> Option<usize> {
    let pid = i64::from(running_pid(app)?);
    // every window of every Space, front to back
    let windows = copy_window_info(
        kCGWindowListOptionAll | kCGWindowListExcludeDesktopElements,
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
    let mut count = 0;
    for w in windows.iter() {
        let d: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(*w as CFDictionaryRef) };
        let number = |k: &CFString| d.find(k).and_then(|v| v.downcast::<CFNumber>());
        if number(&k_pid).and_then(|n| n.to_i64()) != Some(pid) {
            continue;
        }
        if number(&k_layer).and_then(|n| n.to_i64()) != Some(0) {
            continue;
        }
        if number(&k_alpha)
            .and_then(|n| n.to_f64())
            .is_some_and(|a| a < 0.01)
        {
            continue;
        }
        let big = d
            .find(&k_bounds)
            .and_then(|v| v.downcast::<CFDictionary>())
            .and_then(|b| CGRect::from_dict_representation(&b))
            .is_some_and(|r| r.size.width >= MAIN_MIN_W && r.size.height >= MAIN_MIN_H);
        if big {
            count += 1;
        }
    }
    Some(count)
}

/// The windows of an app told apart: not on macOS, where the new window is told by the count taken
/// right before the key (see [`open_app_new_window`]).
pub fn window_ids(_app: &str) -> Option<Windows> {
    None
}

/// Process id of the app in front, whether or not it has a window.
fn frontmost_pid() -> Option<i32> {
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSWorkspace;

    autoreleasepool(|_| {
        NSWorkspace::sharedWorkspace()
            .frontmostApplication()
            .map(|a| a.processIdentifier())
    })
}

/// Whether the app declares that it has a single main window. macOS has no such declaration that can be read without asking the app; `windowing` has the list.
pub fn declares_single_window(_app: &str) -> bool {
    false
}

/// Opens a new window of an app that already runs: the app is brought forward (`open -a`, which also
/// goes to its Space), then Cmd+N is pressed, the shortcut for "new window" (or "new document") in
/// Finder, Safari, Chrome, Terminal, Notes and most other apps. There is no call that asks an app
/// for a window, so an app that uses another shortcut, or none, only comes forward.
///
/// An app that does not run (the answer that offered the choice was out of date) is just opened.
///
/// Returns what `windowing` learns from: the windows the app had **right before the key**, with the
/// app in front and its windows settled. Bringing an app forward can change its windows by itself
/// (a window that was hidden or closed to the menu bar comes back, one is made on activation), and
/// counted before that it looks like the new window the key was meant to make. [`Asked::NotSent`]
/// when no key was sent: the app does not run, or never came to the front.
pub fn open_app_new_window(app: &str) -> Asked {
    let Some(pid) = running_pid(app) else {
        trace(|| format!("new window of {app:?}: not running, plain open -a"));
        open_app(app);
        return Asked::NotSent;
    };
    open_app(app);
    // `open` returns before the switch is done, and the key would reach the app that was in front
    let start = Instant::now();
    while frontmost_pid() != Some(pid) && start.elapsed() < OPEN_WAIT {
        std::thread::sleep(Duration::from_millis(15));
    }
    let front = frontmost_pid() == Some(pid);
    trace(|| format!("new window of {app:?}: pid {pid}, frontmost {front}"));
    if !front {
        return Asked::NotSent;
    }
    std::thread::sleep(Duration::from_millis(60)); // let its window become the key window
    let settled = settled_window_count(app);
    press_shortcut(Shortcut::ch('n', MOD_CMD));
    Asked::Sent(settled)
}

/// How many windows the app has once what bringing it forward shows has happened: counted again
/// every [`COUNT_SETTLE`] until two looks in a row agree (at most [`COUNT_SETTLE_LOOKS`] more).
fn settled_window_count(app: &str) -> Option<usize> {
    let mut count = window_count(app)?;
    for _ in 0..COUNT_SETTLE_LOOKS {
        std::thread::sleep(COUNT_SETTLE);
        match window_count(app) {
            Some(n) if n == count => break,
            Some(n) => count = n,
            None => return None,
        }
    }
    Some(count)
}

/// The windows of every app, front to back, as [`WindowInfo`]. With `on_screen_only` just the ones
/// on the Spaces that are shown, otherwise those of all Spaces (and minimized ones).
fn window_list(on_screen_only: bool) -> Vec<WindowInfo> {
    let scope = if on_screen_only {
        kCGWindowListOptionOnScreenOnly
    } else {
        kCGWindowListOptionAll
    };
    let Some(windows) =
        copy_window_info(scope | kCGWindowListExcludeDesktopElements, kCGNullWindowID)
    else {
        return Vec::new();
    };
    let (k_number, k_layer, k_alpha, k_pid, k_bounds, k_on_screen) = unsafe {
        (
            CFString::wrap_under_get_rule(kCGWindowNumber),
            CFString::wrap_under_get_rule(kCGWindowLayer),
            CFString::wrap_under_get_rule(kCGWindowAlpha),
            CFString::wrap_under_get_rule(kCGWindowOwnerPID),
            CFString::wrap_under_get_rule(kCGWindowBounds),
            CFString::wrap_under_get_rule(kCGWindowIsOnscreen),
        )
    };
    let mut out = Vec::new();
    for w in windows.iter() {
        let d: CFDictionary<CFString, CFType> =
            unsafe { CFDictionary::wrap_under_get_rule(*w as CFDictionaryRef) };
        let number = |k: &CFString| d.find(k).and_then(|v| v.downcast::<CFNumber>());
        let (Some(num), Some(layer), Some(pid)) = (
            number(&k_number).and_then(|n| n.to_i64()),
            number(&k_layer).and_then(|n| n.to_i64()),
            number(&k_pid).and_then(|n| n.to_i64()),
        ) else {
            continue;
        };
        let Some(bounds) = d
            .find(&k_bounds)
            .and_then(|v| v.downcast::<CFDictionary>())
            .and_then(|b| CGRect::from_dict_representation(&b))
        else {
            continue;
        };
        out.push(WindowInfo {
            number: num as u32,
            pid: pid as i32,
            layer,
            alpha: number(&k_alpha).and_then(|n| n.to_f64()).unwrap_or(1.0),
            width: bounds.size.width,
            height: bounds.size.height,
            // the key is only present for windows that are on screen
            on_screen: on_screen_only
                || d.find(&k_on_screen)
                    .and_then(|v| v.downcast::<CFBoolean>())
                    .is_some_and(bool::from),
        });
    }
    out
}

/// The display a walk is about: the one that holds a window of the app. With separate Spaces per
/// display every screen has its own row of Spaces, so the walk is made on that row, and
/// Control+Arrow is sent while the pointer is on that screen (the key acts on the display under
/// the pointer), never on the screen that was clicked when the window is on another.
struct Target {
    /// The display, as `SLSCopyManagedDisplaySpaces` names it.
    display: String,
    /// The Spaces of that display, left to right.
    row: Vec<u64>,
    /// The Spaces of that display that have a window of the app.
    spaces: Vec<u64>,
    /// Where the pointer has to be for the keys to act on that display: `None` when it is there
    /// already, or when there is one display only.
    pointer: Option<CGPoint>,
}

/// The row of Spaces as [`walk_to_target`] needs it, looking for the windows of the app `pid`.
struct Row {
    pid: i32,
    /// The private calls, when this macOS has them: they tell the order of the Spaces, so the way
    /// there is known, and tell an empty Space from another.
    sky: Option<SkyLight>,
    /// Send the arrow keys through System Events instead of posting them directly.
    via_script: bool,
    /// The display of the window, when the window server can tell.
    target: Option<Target>,
    /// Where the pointer was before it was moved to the display of the window, to put it back.
    saved_pointer: Option<CGPoint>,
    /// Whether the pointer was moved to the display of the window already.
    aimed: bool,
}

impl Row {
    /// `windows` are the ordinary windows of the app, on any Space, front to back: the Spaces they
    /// are on are the ones to go to.
    fn new(pid: i32, windows: &[u32], via_script: bool) -> Self {
        let sky = SkyLight::load();
        let target = sky.as_ref().and_then(|sky| locate_target(sky, windows));
        Row {
            pid,
            sky,
            via_script,
            target,
            saved_pointer: None,
            aimed: false,
        }
    }

    /// Whether a window of the app is on a Space that is shown (on any display).
    fn app_is_here(&self) -> bool {
        presence_of(&window_list(true), self.pid) == Presence::Here
    }

    /// The Space the display of the window shows right now. `None` without a known display.
    fn target_space(&self) -> Option<u64> {
        let (sky, target) = (self.sky.as_ref()?, self.target.as_ref()?);
        sky.displays()
            .into_iter()
            .find(|d| d.display == target.display)
            .map(|d| d.current)
            .filter(|space| *space != 0)
    }

    /// Puts the pointer on the display of the window, once: the Control+Arrow shortcuts switch
    /// the Spaces of the display under the pointer.
    fn aim_pointer(&mut self) {
        let Some(point) = self.target.as_ref().and_then(|t| t.pointer) else {
            return;
        };
        if self.aimed {
            return;
        }
        self.aimed = true;
        self.saved_pointer = super::cursor_pos().map(|(x, y)| CGPoint::new(x, y));
        trace(|| format!("pointer to the display of the window: {point:?}"));
        warp_pointer(point);
        std::thread::sleep(POINTER_SETTLE);
    }
}

impl Drop for Row {
    /// The pointer goes back to where the user had it.
    fn drop(&mut self) {
        if let Some(point) = self.saved_pointer.take() {
            warp_pointer(point);
        }
    }
}

/// The display that has the windows to go to, from the Spaces they are on. See [`Target`].
fn locate_target(sky: &SkyLight, windows: &[u32]) -> Option<Target> {
    if windows.is_empty() {
        return None;
    }
    let rows = sky.displays();
    let under_pointer = pointer_display(&rows).map(|r| r.display.clone());
    let window_spaces: Vec<Vec<u64>> = windows.iter().map(|w| sky.spaces_of(*w)).collect();
    let (at, spaces) = pick_display(&rows, &window_spaces, under_pointer.as_deref())?;
    let chosen = &rows[at];
    let pointer = if rows.len() > 1 && under_pointer.as_deref() != Some(chosen.display.as_str()) {
        display_bounds(&chosen.display).map(|b| {
            CGPoint::new(
                b.origin.x + b.size.width / 2.0,
                b.origin.y + b.size.height / 2.0,
            )
        })
    } else {
        None
    };
    trace(|| {
        format!(
            "{} display(s); pointer on {under_pointer:?}; windows on display {:?} (Spaces {spaces:?} of {:?}, shows {})",
            rows.len(),
            chosen.display,
            chosen.spaces,
            chosen.current
        )
    });
    Some(Target {
        display: chosen.display.clone(),
        row: chosen.spaces.clone(),
        spaces,
        pointer,
    })
}

/// The display under the pointer, among the displays of the window server.
fn pointer_display(rows: &[DisplayRow]) -> Option<&DisplayRow> {
    let (x, y) = super::cursor_pos()?;
    let point = CGPoint::new(x, y);
    rows.iter()
        .find(|r| display_bounds(&r.display).is_some_and(|b| b.contains(&point)))
}

/// Moves the pointer without generating mouse events.
fn warp_pointer(point: CGPoint) {
    // SAFETY: a plain call with a point by value.
    unsafe {
        CGWarpMouseCursorPosition(point);
    }
}

/// The bounds (global coordinates, in points) of the display the window server names `identifier`:
/// the UUID of the display, or `Main` when the Spaces are shared by all displays.
fn display_bounds(identifier: &str) -> Option<CGRect> {
    use core_graphics::display::CGDisplay;
    if identifier.eq_ignore_ascii_case("Main") {
        return Some(CGDisplay::main().bounds());
    }
    CGDisplay::active_displays()
        .ok()?
        .into_iter()
        .find(|id| display_uuid(*id).is_some_and(|u| u.eq_ignore_ascii_case(identifier)))
        .map(|id| CGDisplay::new(id).bounds())
}

/// The UUID of a display as text, the name `SLSCopyManagedDisplaySpaces` gives it.
fn display_uuid(id: u32) -> Option<String> {
    // SAFETY: the CFUUID that is returned is wrapped at once, so it is released; the string made
    // from it is wrapped the same way.
    unsafe {
        let uuid = CGDisplayCreateUUIDFromDisplayID(id);
        if uuid.is_null() {
            return None;
        }
        let uuid = CFType::wrap_under_create_rule(uuid);
        let text = CFUUIDCreateString(std::ptr::null(), uuid.as_CFTypeRef());
        (!text.is_null()).then(|| CFString::wrap_under_create_rule(text).to_string())
    }
}

impl Desktops for Row {
    type Id = u64;

    /// A window of the app is on the Space that the display of the window shows. Without a known
    /// display: on any Space that is shown.
    fn target_here(&mut self) -> bool {
        match (self.target_space(), self.target.as_ref()) {
            (Some(space), Some(target)) => target.spaces.contains(&space),
            _ => self.app_is_here(),
        }
    }

    /// The Space that is shown on the display of the window, so that what happens on the other
    /// displays does not count as a move. Without a known display: the Space that is shown, its id
    /// from the window server, together with the windows on it, so that a change of either counts
    /// as a move, and an empty Space can still be told. CursorPie's own windows are left out of the
    /// windows: the pie may still be fading out.
    fn current(&mut self) -> Option<u64> {
        if let Some(space) = self.target_space() {
            return Some(space);
        }
        let me = std::process::id() as i32;
        let windows = signature(
            window_list(true)
                .iter()
                .filter(|w| w.pid != me && w.is_ordinary())
                .map(|w| w.number)
                .collect(),
        );
        let space = self.sky.as_ref().map_or(0, SkyLight::shown_space);
        if space == 0 {
            return windows;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (space, windows).hash(&mut hasher);
        Some(hasher.finish())
    }

    /// Steps to the nearest Space with a window of the app, on the row of the display of the
    /// window, from the order the window server reports the Spaces in.
    fn route(&mut self) -> Option<isize> {
        let here = self.target_space()?;
        let target = self.target.as_ref()?;
        let steps = steps_between(&target.row, &here, &target.spaces);
        trace(|| {
            format!(
                "display {:?}: Spaces in order {:?}, shown {here}, windows of the app on {:?}: {steps:?} steps",
                target.display, target.row, target.spaces
            )
        });
        steps
    }

    fn step(&mut self, forward: bool) {
        // the keys act on the display under the pointer: it has to be the display of the window
        self.aim_pointer();
        let before = self.current();
        if self.via_script {
            press_space_arrow_with_script(forward);
        } else {
            super::press_space_arrow(forward);
        }
        // macOS animates the switch: wait until the answer changes, or until it is clear that
        // nothing happens (the last Space). The app being on the screen is not the end of it: during
        // the animation the windows of two Spaces are shown.
        let start = Instant::now();
        let mut moved = false;
        while start.elapsed() < SETTLE {
            std::thread::sleep(POLL);
            if self.current() != before {
                moved = true;
                break;
            }
        }
        if moved {
            // a key pressed during the animation can be lost, so the next step waits for it
            std::thread::sleep(STEP_PAUSE);
        }
        let after = self.current();
        let here = self.target_here();
        let via_script = self.via_script;
        trace(|| {
            format!(
                "step {} (script: {via_script}): Space {} after {} ms, app here: {here}",
                if forward { "right" } else { "left" },
                if after == before {
                    "did not change"
                } else {
                    "changed"
                },
                start.elapsed().as_millis(),
            )
        });
    }
}

/// Control+Left or Control+Right sent through System Events, which some macOS versions honor when
/// they ignore the same keys posted with `CGEvent`. Key codes 123 and 124 are the arrows. The call
/// is not waited for: the caller looks at the screen.
fn press_space_arrow_with_script(forward: bool) {
    let code = if forward { 124 } else { 123 };
    spawn_reaped(Command::new("osascript").arg("-e").arg(format!(
        "tell application \"System Events\" to key code {code} using control down"
    )));
}

/// Waits (at most `OPEN_WAIT`) until a window of the app `pid` is on a Space that is shown.
fn wait_until_here(pid: i32) -> bool {
    let row = Row::new(pid, &[], false);
    let start = Instant::now();
    while start.elapsed() < OPEN_WAIT {
        if row.app_is_here() {
            return true;
        }
        std::thread::sleep(POLL);
    }
    row.app_is_here()
}

/// Whether the frontmost ordinary window (CursorPie's own left out) belongs to the app `pid`.
fn is_frontmost(pid: i32) -> bool {
    let me = std::process::id() as i32;
    window_list(true)
        .iter()
        .find(|w| w.pid != me && w.is_ordinary())
        .is_some_and(|w| w.pid == pid)
}

/// Waits (at most `FRONT_WAIT`) until the app `pid` has the frontmost window.
fn wait_until_frontmost(pid: i32) -> bool {
    let start = Instant::now();
    while start.elapsed() < FRONT_WAIT {
        if is_frontmost(pid) {
            return true;
        }
        std::thread::sleep(POLL);
    }
    is_frontmost(pid)
}

/// Brings the window of the app `pid` to the front once its Space is shown. `open -a` alone does
/// not do it: it activates the app, but another app's window can stay in front. So the window is
/// raised itself (it is visible to Accessibility now that its Space is the current one), the app is
/// made frontmost, and the result is checked, up to three times. `wait` lets a Space animation
/// finish first.
fn focus_app(pid: i32, app: &str, wait: bool) -> bool {
    if wait {
        std::thread::sleep(ARRIVE);
    }
    for attempt in 1..=3 {
        let raised = raise_main_window(pid);
        activate(pid);
        open_app(app);
        let front = wait_until_frontmost(pid);
        trace(|| format!("focus attempt {attempt}: raised {raised}, frontmost {front}"));
        if front {
            return true;
        }
    }
    false
}

/// The "Open app" action with Space awareness (see [`DesktopMode`]): `app` is a bundle id, an app
/// name or a `.app` path, as typed in a slice. Apps that do not run, are hidden, have no window or
/// have one on the current Space are opened exactly as before.
pub fn reveal_app(app: &str, mode: DesktopMode) {
    trace(|| format!("open {app:?}, mode {mode:?}"));
    let Some(pid) = visible_running_pid(app) else {
        trace(|| "not running or hidden: plain open -a".into());
        open_app(app);
        return;
    };
    let all = window_list(false);
    let presence = presence_of(&all, pid);
    trace(|| {
        let own: Vec<_> = all.iter().filter(|w| w.pid == pid).collect();
        format!(
            "pid {pid}: {} windows in the list of all Spaces, {} of the app, {} ordinary, {} ordinary on screen; {presence:?}",
            all.len(),
            own.len(),
            own.iter().filter(|w| w.is_ordinary()).count(),
            own.iter()
                .filter(|w| w.is_ordinary() && w.on_screen)
                .count(),
        )
    });
    let windows: Vec<u32> = all
        .iter()
        .filter(|w| w.pid == pid && w.is_ordinary())
        .map(|w| w.number)
        .collect();
    if presence == Presence::Here && show_on_pointer_display(pid) {
        return;
    }
    let Presence::Elsewhere(window) = presence else {
        open_app(app);
        return;
    };
    if mode == DesktopMode::BringToCurrent {
        if bring_window_here(window) {
            focus_app(pid, app, false);
            return;
        }
        eprintln!(
            "CursorPie: could not move a window of {app} to this Space (private system calls are not \
             available or refused); switching to its Space instead"
        );
    }
    // 1. `open -a`: with the system setting on, this is all it takes
    open_app(app);
    if wait_until_here(pid) {
        trace(|| "open -a brought a window here".into());
        if !wait_until_frontmost(pid) {
            focus_app(pid, app, true);
        }
        return;
    }
    // 2. raise the main window through Accessibility
    if raise_main_window(pid) && wait_until_here(pid) {
        trace(|| "AXRaise brought a window here".into());
        focus_app(pid, app, true);
        return;
    }
    // 3. and 4. walk the Spaces with the arrow keys, posted and then through System Events
    for via_script in [false, true] {
        trace(|| format!("walking the Spaces (script: {via_script})"));
        let reached = {
            let mut row = Row::new(pid, &windows, via_script);
            walk_to_target(&mut row, MAX_STEPS)
        }; // the row is gone here: the pointer is back where the user had it
        if reached {
            focus_app(pid, app, true);
            return;
        }
    }
    eprintln!(
        "CursorPie: could not reach the Space of {app}; start CursorPie with CURSORPIE_DEBUG=1 to see why"
    );
}

/// The display under the pointer, in the space of the pointer. `None` with one display only (there
/// is no other screen to fetch from), or when the pointer is on none.
fn pointer_screen() -> Option<ScreenRect> {
    use core_graphics::display::CGDisplay;

    let (x, y) = super::cursor_pos()?;
    let point = CGPoint::new(x, y);
    let displays = CGDisplay::active_displays().ok()?;
    if displays.len() < 2 {
        return None;
    }
    displays
        .into_iter()
        .map(|id| CGDisplay::new(id).bounds())
        .find(|b| b.contains(&point))
        .map(|b| ScreenRect {
            x: b.origin.x,
            y: b.origin.y,
            w: b.size.width,
            h: b.size.height,
        })
}

/// Where a window of `size` goes on `screen`: centered, and never above the menu bar. A window
/// larger than the display is made to fit.
fn place_on(screen: ScreenRect, width: f64, height: f64) -> (CGPoint, CGSize) {
    let w = width.min(screen.w).max(1.0);
    let h = height.min(screen.h - MENU_BAR).max(1.0);
    let x = screen.x + (screen.w - w) / 2.0;
    let y = screen.y + MENU_BAR.max((screen.h - h) / 2.0);
    (CGPoint::new(x, y), CGSize::new(w, h))
}

/// Shows a window of the app `pid` on the display under the pointer, when there is more than one
/// display: its front window on that display is raised and gets the focus, and when it has none
/// there, its front window is moved onto that display first. `true` when that was done. `false`
/// leaves everything as it was: one display only, no real window (an app with only its desktop,
/// like Finder without a window), a window that cannot be moved (full screen) or no Accessibility
/// permission. The caller then opens the app as before.
///
/// `open -a` is not enough on more than one display: it focuses whichever window of the app was
/// active last, which can be the one on the display the person is not looking at, even when the app
/// has a window on the clicked display too.
///
/// The windows come from Accessibility, and only the standard ones count. The window list of the
/// window server also holds small helper windows of an app (Finder has them) that Accessibility
/// does not know and that cannot be moved, and the windows that draw the desktop.
fn show_on_pointer_display(pid: i32) -> bool {
    let Some(screen) = pointer_screen() else {
        return false;
    };
    // SAFETY: every element that is created or copied is wrapped at once, so it is released on
    // every path; names are CFStrings that outlive the calls; AXValue pointers are to live locals
    // of the type the AXValue type asks for.
    unsafe {
        let app_ref = AXUIElementCreateApplication(pid);
        if app_ref.is_null() {
            return false;
        }
        let app = CFType::wrap_under_create_rule(app_ref);
        let Some(list) = copy_attribute(&app, "AXWindows").and_then(|v| v.downcast::<CFArray>())
        else {
            trace(|| format!("AXWindows of pid {pid}: none (no permission?)"));
            return false;
        };
        trace(|| format!("AXWindows of pid {pid}: {} windows", list.len()));
        let mut frames: Vec<ScreenRect> = Vec::new();
        let mut elements: Vec<CFType> = Vec::new();
        for item in list.iter() {
            let window = CFType::wrap_under_get_rule(*item);
            let subrole = copy_attribute(&window, "AXSubrole")
                .and_then(|v| v.downcast::<CFString>())
                .map(|s| s.to_string());
            let flag = |name: &str| {
                copy_attribute(&window, name)
                    .and_then(|v| v.downcast::<CFBoolean>())
                    .is_some_and(bool::from)
            };
            let (at, by) = (ax_point(&window, "AXPosition"), ax_size(&window, "AXSize"));
            trace(|| format!("  AX window {subrole:?} at {at:?} size {by:?}"));
            let (Some(at), Some(by)) = (at, by) else {
                continue;
            };
            if subrole.as_deref() != Some("AXStandardWindow")
                || flag("AXMinimized")
                || flag("AXFullScreen")
                || by.width < 50.0
                || by.height < 50.0
            {
                continue;
            }
            frames.push(ScreenRect {
                x: at.x,
                y: at.y,
                w: by.width,
                h: by.height,
            });
            elements.push(window);
        }
        let Some(pick) = window_for_screen(&frames, screen) else {
            trace(|| "no real window".into());
            return false;
        };
        let (front, moves) = match pick {
            OnScreen::Raise(at) => (at, false),
            OnScreen::Move(at) => (at, true),
        };
        let (frame, target) = (frames[front], elements.swap_remove(front));
        let mut expected = None;
        if moves {
            let (origin, size) = place_on(screen, frame.w, frame.h);
            // the size first, only when it has to shrink, so that the window fits where it lands
            if size.width < frame.w || size.height < frame.h {
                ax_set_value(
                    &target,
                    "AXSize",
                    2,
                    &size as *const CGSize as *const c_void,
                );
            }
            let error = ax_set_value(
                &target,
                "AXPosition",
                1,
                &origin as *const CGPoint as *const c_void,
            );
            trace(|| format!("AXPosition set to {origin:?}: AX error {error}"));
            if error != 0 {
                return false;
            }
            expected = Some(origin);
        } else {
            trace(|| format!("a window is on this display already: raising {frame:?}"));
        }
        // the app first (that raises all of its windows), then the one that was meant, last, so
        // that it is the one in front and the one that has the keyboard
        activate(pid);
        let frontmost = CFString::from_static_string("AXFrontmost");
        AXUIElementSetAttributeValue(
            app.as_CFTypeRef(),
            frontmost.as_concrete_TypeRef(),
            CFBoolean::true_value().as_CFTypeRef(),
        );
        let raise = CFString::from_static_string("AXRaise");
        AXUIElementPerformAction(target.as_CFTypeRef(), raise.as_concrete_TypeRef());
        for attribute in ["AXMain", "AXFocused"] {
            let name = CFString::from_static_string(attribute);
            AXUIElementSetAttributeValue(
                target.as_CFTypeRef(),
                name.as_concrete_TypeRef(),
                CFBoolean::true_value().as_CFTypeRef(),
            );
        }
        // a move is judged by where the window says it is now, not by the answer of the set call
        if let Some(origin) = expected {
            let start = Instant::now();
            loop {
                let at = ax_point(&target, "AXPosition");
                if at.is_some_and(|p| (p.x - origin.x).abs() + (p.y - origin.y).abs() <= SAME_PLACE)
                {
                    break;
                }
                if start.elapsed() >= MOVE_WAIT {
                    trace(|| format!("the window did not move (now at {at:?})"));
                    return false;
                }
                std::thread::sleep(POLL);
            }
        }
        trace(|| "a window of the app is shown on this display".into());
        true
    }
}

/// Sets an Accessibility attribute to a point (`kind` 1) or a size (`kind` 2) and gives the AX
/// error, 0 when it worked.
///
/// # Safety
/// `element` must be a live accessibility element and `value` must point to a live `CGPoint` for
/// kind 1, or to a live `CGSize` for kind 2.
unsafe fn ax_set_value(
    element: &CFType,
    attribute: &str,
    kind: c_int,
    value: *const c_void,
) -> c_int {
    // SAFETY: guaranteed by the caller; the AXValue that is created is wrapped at once.
    unsafe {
        let made = AXValueCreate(kind, value);
        if made.is_null() {
            return -1;
        }
        let made = CFType::wrap_under_create_rule(made);
        let name = CFString::new(attribute);
        AXUIElementSetAttributeValue(
            element.as_CFTypeRef(),
            name.as_concrete_TypeRef(),
            made.as_CFTypeRef(),
        )
    }
}

/// The size an Accessibility attribute such as `AXSize` holds.
///
/// # Safety
/// `element` must be a live accessibility element.
unsafe fn ax_size(element: &CFType, attribute: &str) -> Option<CGSize> {
    // SAFETY: guaranteed by the caller; `size` is a live CGSize, which is what kind 2 writes.
    unsafe {
        let value = copy_attribute(element, attribute)?;
        let mut size = CGSize::new(0.0, 0.0);
        let ok = AXValueGetValue(
            value.as_CFTypeRef(),
            2,
            &mut size as *mut CGSize as *mut c_void,
        );
        (ok != 0).then_some(size)
    }
}

/// The point an Accessibility attribute such as `AXPosition` holds.
///
/// # Safety
/// `element` must be a live accessibility element.
unsafe fn ax_point(element: &CFType, attribute: &str) -> Option<CGPoint> {
    // SAFETY: guaranteed by the caller; `point` is a live CGPoint, which is what kind 1 writes.
    unsafe {
        let value = copy_attribute(element, attribute)?;
        let mut point = CGPoint::new(0.0, 0.0);
        let ok = AXValueGetValue(
            value.as_CFTypeRef(),
            1,
            &mut point as *mut CGPoint as *mut c_void,
        );
        (ok != 0).then_some(point)
    }
}

// ----------------------------------------------------------------------------- Accessibility

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateApplication(pid: c_int) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> c_int;
    fn AXUIElementPerformAction(element: CFTypeRef, action: CFStringRef) -> c_int;
    fn AXUIElementSetAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> c_int;
    fn AXValueCreate(kind: c_int, value: *const c_void) -> CFTypeRef;
    fn AXValueGetValue(value: CFTypeRef, kind: c_int, out: *mut c_void) -> u8;
    /// The CFUUID of a display (ColorSync, part of ApplicationServices); a new object.
    fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> CFTypeRef;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFUUIDCreateString(allocator: *const c_void, uuid: CFTypeRef) -> CFStringRef;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> c_int;
}

/// Raises the main (else the focused) window of the app `pid` with the Accessibility action
/// `AXRaise`, which shows the Space the window is on. `true` when the action was accepted. Needs
/// the Accessibility permission, which CursorPie has to have anyway.
fn raise_main_window(pid: i32) -> bool {
    // SAFETY: every element that is created or copied is wrapped at once, so it is released on every
    // path; the attribute and action names are CFStrings that outlive the calls.
    unsafe {
        let app_ref = AXUIElementCreateApplication(pid);
        if app_ref.is_null() {
            return false;
        }
        let app = CFType::wrap_under_create_rule(app_ref);
        // make the app frontmost, so that the raised window gets the keyboard focus too
        let frontmost = CFString::from_static_string("AXFrontmost");
        let error = AXUIElementSetAttributeValue(
            app.as_CFTypeRef(),
            frontmost.as_concrete_TypeRef(),
            CFBoolean::true_value().as_CFTypeRef(),
        );
        trace(|| format!("AXFrontmost of pid {pid}: AX error {error}"));
        // the main and the focused window, else the windows of the app on the Space that is shown,
        // front to back: an app that is not active has no main or focused window
        let mut candidates: Vec<(&str, CFType)> = Vec::new();
        for attribute in ["AXMainWindow", "AXFocusedWindow"] {
            let value = copy_attribute(&app, attribute);
            trace(|| format!("{attribute} of pid {pid}: found {}", value.is_some()));
            candidates.extend(value.map(|w| (attribute, w)));
        }
        if let Some(list) = copy_attribute(&app, "AXWindows").and_then(|v| v.downcast::<CFArray>())
        {
            trace(|| format!("AXWindows of pid {pid}: {} windows", list.len()));
            for item in list.iter() {
                candidates.push(("AXWindows item", CFType::wrap_under_get_rule(*item)));
            }
        }
        let action = CFString::from_static_string("AXRaise");
        for (what, window) in candidates {
            let error =
                AXUIElementPerformAction(window.as_CFTypeRef(), action.as_concrete_TypeRef());
            trace(|| format!("AXRaise of the {what}: AX error {error}"));
            if error == 0 {
                for attribute in ["AXMain", "AXFocused"] {
                    let name = CFString::from_static_string(attribute);
                    AXUIElementSetAttributeValue(
                        window.as_CFTypeRef(),
                        name.as_concrete_TypeRef(),
                        CFBoolean::true_value().as_CFTypeRef(),
                    );
                }
                return true;
            }
        }
        false
    }
}

/// The value of the Accessibility attribute `attribute` of `element`, `None` when it has none.
///
/// # Safety
/// `element` must be a live accessibility element.
unsafe fn copy_attribute(element: &CFType, attribute: &str) -> Option<CFType> {
    let name = CFString::new(attribute);
    let mut value: CFTypeRef = std::ptr::null();
    // SAFETY: the caller guarantees the element; the copied value is wrapped, so it is released.
    unsafe {
        let error = AXUIElementCopyAttributeValue(
            element.as_CFTypeRef(),
            name.as_concrete_TypeRef(),
            &mut value,
        );
        (error == 0 && !value.is_null()).then(|| CFType::wrap_under_create_rule(value))
    }
}

/// Asks the system to activate the app `pid` with all its windows.
fn activate(pid: i32) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
        let sent = app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        trace(|| format!("activate pid {pid}: {sent}"));
    }
}

// ----------------------------------------------------------------------------- SkyLight

// libSystem, which every process links, has both
unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

/// `RTLD_LAZY`.
const RTLD_LAZY: c_int = 1;
/// The private framework that holds the window server client calls.
const SKYLIGHT: &std::ffi::CStr = c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight";
/// A made-up workspace id ("yabe"): while a Space has it, `SLSSetWindowListWorkspace` puts windows
/// on that Space. This is how windows are moved since macOS 14.5.
const COMPAT_ID: c_int = 0x7961_6265;
/// Selector of `SLSCopySpacesForWindows`: the Spaces of every kind.
const ALL_SPACES: c_int = 0x7;

type MainConnection = unsafe extern "C" fn() -> c_int;
type ActiveSpace = unsafe extern "C" fn(c_int) -> u64;
type SpacesForWindows = unsafe extern "C" fn(c_int, c_int, CFArrayRef) -> CFArrayRef;
type ManagedDisplaySpaces = unsafe extern "C" fn(c_int) -> CFArrayRef;
type SetCompatId = unsafe extern "C" fn(c_int, u64, c_int) -> c_int;
type SetWindowListWorkspace = unsafe extern "C" fn(c_int, *const u32, c_int, c_int) -> c_int;

/// The private calls, looked up when needed. A field is a function the running macOS has.
struct SkyLight {
    main_connection: MainConnection,
    active_space: ActiveSpace,
    spaces_for_windows: SpacesForWindows,
    managed_display_spaces: ManagedDisplaySpaces,
    set_compat_id: SetCompatId,
    set_window_list_workspace: SetWindowListWorkspace,
}

impl SkyLight {
    /// `None` when SkyLight or any of the calls is missing.
    fn load() -> Option<Self> {
        // SAFETY: `dlopen` and `dlsym` are called with zero terminated names. A symbol is only
        // turned into a function pointer when it is not null, and with the signature that the
        // call has in `yabai` and `Hammerspoon`, which use it the same way.
        unsafe {
            let handle = dlopen(SKYLIGHT.as_ptr(), RTLD_LAZY);
            if handle.is_null() {
                return None;
            }
            let sym = |name: &std::ffi::CStr| {
                let p = dlsym(handle, name.as_ptr());
                (!p.is_null()).then_some(p)
            };
            Some(SkyLight {
                main_connection: std::mem::transmute::<*mut c_void, MainConnection>(sym(
                    c"SLSMainConnectionID",
                )?),
                active_space: std::mem::transmute::<*mut c_void, ActiveSpace>(sym(
                    c"SLSGetActiveSpace",
                )?),
                spaces_for_windows: std::mem::transmute::<*mut c_void, SpacesForWindows>(sym(
                    c"SLSCopySpacesForWindows",
                )?),
                managed_display_spaces: std::mem::transmute::<*mut c_void, ManagedDisplaySpaces>(
                    sym(c"SLSCopyManagedDisplaySpaces")?,
                ),
                set_compat_id: std::mem::transmute::<*mut c_void, SetCompatId>(sym(
                    c"SLSSpaceSetCompatID",
                )?),
                set_window_list_workspace: std::mem::transmute::<*mut c_void, SetWindowListWorkspace>(
                    sym(c"SLSSetWindowListWorkspace")?,
                ),
            })
        }
    }

    /// The id of the Space that is shown, 0 when the call has nothing to say.
    fn shown_space(&self) -> u64 {
        // SAFETY: both are plain calls with the connection of this process.
        unsafe { (self.active_space)((self.main_connection)()) }
    }

    /// The ids of the Spaces the window `number` is on. Empty for a minimized window, or when the
    /// call has nothing to say.
    fn spaces_of(&self, number: u32) -> Vec<u64> {
        let ids = CFArray::from_CFTypes(&[CFNumber::from(i64::from(number))]);
        // SAFETY: `ids` is a live CFArray of CFNumber; the result, when not null, is a new array
        // that is released by the wrapper.
        unsafe {
            let spaces = (self.spaces_for_windows)(
                (self.main_connection)(),
                ALL_SPACES,
                ids.as_concrete_TypeRef(),
            );
            if spaces.is_null() {
                return Vec::new();
            }
            CFArray::<CFNumber>::wrap_under_create_rule(spaces)
                .iter()
                .filter_map(|n| n.to_i64())
                .map(|n| n as u64)
                .collect()
        }
    }

    /// Every display with the Spaces it has, from left to right, and the one it shows. With
    /// separate Spaces per display each screen has its own row (Control+Arrow goes through the row
    /// of the display under the pointer); with shared Spaces there is one entry.
    fn displays(&self) -> Vec<DisplayRow> {
        let key_display = CFString::from_static_string("Display Identifier");
        let key_current = CFString::from_static_string("Current Space");
        let key_spaces = CFString::from_static_string("Spaces");
        let key_id = CFString::from_static_string("ManagedSpaceID");
        let id_of = |space: &CFDictionary<CFString, CFType>| {
            space
                .find(&key_id)
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|n| n.to_i64())
                .map(|n| n as u64)
        };
        // SAFETY: the result, when not null, is a new array that is released by the wrapper; its
        // items are dictionaries, which are only read through the typed wrappers.
        let displays = unsafe {
            let list = (self.managed_display_spaces)((self.main_connection)());
            if list.is_null() {
                return Vec::new();
            }
            CFArray::<*const c_void>::wrap_under_create_rule(list)
        };
        let mut out = Vec::new();
        for display in displays.iter() {
            let display: CFDictionary<CFString, CFType> =
                unsafe { CFDictionary::wrap_under_get_rule(*display as CFDictionaryRef) };
            let name = display
                .find(&key_display)
                .and_then(|v| v.downcast::<CFString>())
                .map(|s| s.to_string());
            let Some(spaces) = display
                .find(&key_spaces)
                .and_then(|v| v.downcast::<CFArray>())
            else {
                continue;
            };
            let ids: Vec<u64> = spaces
                .iter()
                .filter_map(|space| {
                    let space: CFDictionary<CFString, CFType> =
                        unsafe { CFDictionary::wrap_under_get_rule(*space as CFDictionaryRef) };
                    id_of(&space)
                })
                .collect();
            let current = display
                .find(&key_current)
                .and_then(|v| v.downcast::<CFDictionary>())
                .and_then(|d| {
                    let d: CFDictionary<CFString, CFType> =
                        unsafe { CFDictionary::wrap_under_get_rule(d.as_concrete_TypeRef()) };
                    id_of(&d)
                })
                .unwrap_or(0);
            if let Some(display) = name {
                out.push(DisplayRow {
                    display,
                    current,
                    spaces: ids,
                });
            }
        }
        out
    }

    /// The Space shown on the display under the pointer: the one the user is looking at when they
    /// click. With one display, or when the display cannot be told, the Space that is shown.
    fn space_under_pointer(&self) -> u64 {
        let rows = self.displays();
        if rows.len() > 1
            && let Some(row) = pointer_display(&rows)
            && row.current != 0
        {
            return row.current;
        }
        self.shown_space()
    }
}

/// Moves the window `number` onto the Space shown on the display under the pointer. `true` only when the window server then
/// reports the window on that Space: the calls themselves return success on macOS versions where
/// they do nothing.
fn bring_window_here(number: u32) -> bool {
    let Some(sl) = SkyLight::load() else {
        return false;
    };
    // SAFETY: the function pointers come from `SkyLight::load`; `number` outlives the call that
    // reads it through the pointer.
    unsafe {
        let connection = (sl.main_connection)();
        let here = sl.space_under_pointer();
        if here == 0 {
            return false;
        }
        (sl.set_compat_id)(connection, here, COMPAT_ID);
        (sl.set_window_list_workspace)(connection, &number, 1, COMPAT_ID);
        (sl.set_compat_id)(connection, here, 0);
        sl.spaces_of(number).contains(&here)
    }
}
