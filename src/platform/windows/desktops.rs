//! Virtual desktops: "Open app" for an app whose window is on another desktop.
//!
//! Windows has no public call that switches to a given desktop, and none that moves the window of
//! another process (`IVirtualDesktopManager::MoveWindowToDesktop` refuses it). What it does have,
//! and this module uses, is the documented `IVirtualDesktopManager`, which tells whether a window
//! is on the desktop that is shown and which desktop a window is on. Getting there is then what
//! the user does: Win+Ctrl+Right and Win+Ctrl+Left, pressed through `SendInput` like every other
//! shortcut slice, until the app's window is on the desktop that is shown. The search itself is
//! [`walk_to_target`], which is unit tested; this module connects it to Windows.
//!
//! Whatever goes wrong, [`reveal_app`] ends in [`open_app`], the behavior from before this
//! setting: no virtual desktops (Windows 7, a Server without the feature), COM unavailable, the
//! app not running, its window on this desktop already, or no way found.

use super::apps::{bring_to_front, pid_of, windows_of};
use super::shell::open_app;
use super::util::{app_id_for_open, desktop_signature, guid_to_u128};
use crate::config::DesktopMode;
use crate::keys::{MOD_CMD, MOD_CTRL, Shortcut, key};
use crate::platform::desktops::{Desktops, MAX_STEPS, walk_to_target};
use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, RPC_E_CHANGED_MODE, S_FALSE, S_OK};
use windows_sys::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Shell::VirtualDesktopManager;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetWindowLongW, GetWindowTextLengthW, IsWindowVisible,
    WS_EX_TOOLWINDOW,
};
use windows_sys::core::GUID;

/// How long to wait for the shell to finish one switch before taking it for "did not move": the
/// end of the row of desktops answers with nothing at all.
const SETTLE: Duration = Duration::from_millis(400);
/// How often the shown desktop is looked at while waiting.
const POLL: Duration = Duration::from_millis(25);

/// `IID_IVirtualDesktopManager`. `windows-sys` has the class id but no interface definitions.
const IID_VIRTUAL_DESKTOP_MANAGER: GUID = GUID::from_u128(0xa5cd92ff_29be_454c_8d04_d82879fb3f1b);

/// The methods of `IVirtualDesktopManager` in vtable order (`shobjidl_core.h`): `IUnknown`, then
/// the three of the interface.
// The layout needs every slot, but only the methods that are called are ever read.
#[allow(dead_code)]
#[repr(C)]
struct ManagerVtbl {
    query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    is_window_on_current_virtual_desktop:
        unsafe extern "system" fn(*mut c_void, HWND, *mut i32) -> i32,
    get_window_desktop_id: unsafe extern "system" fn(*mut c_void, HWND, *mut GUID) -> i32,
    move_window_to_desktop: unsafe extern "system" fn(*mut c_void, HWND, *const GUID) -> i32,
}

/// A COM object starts with a pointer to its vtable.
#[repr(C)]
struct ManagerObject {
    vtbl: *const ManagerVtbl,
}

/// An `IVirtualDesktopManager` together with the COM initialization of this thread that made it
/// possible. Dropping it releases both, so a call leaves nothing behind on the worker thread.
struct Manager {
    object: *mut ManagerObject,
    /// Whether this value has to balance a successful `CoInitializeEx`.
    uninitialize: bool,
}

impl Manager {
    /// `None` when the system has no virtual desktops or COM cannot be used on this thread.
    fn new() -> Option<Self> {
        // SAFETY: plain COM setup on this thread. Every path that returns `None` has undone the
        // initialization it did, and `Drop` undoes the rest.
        unsafe {
            let init = CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32);
            // S_OK and S_FALSE both count a use that has to be balanced. A thread that already
            // runs COM in the other mode can use it all the same, but is not ours to close.
            let uninitialize = init == S_OK || init == S_FALSE;
            if !uninitialize && init != RPC_E_CHANGED_MODE {
                return None;
            }
            let mut object: *mut c_void = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &VirtualDesktopManager,
                std::ptr::null_mut(),
                CLSCTX_ALL,
                &IID_VIRTUAL_DESKTOP_MANAGER,
                &mut object,
            );
            if hr != S_OK || object.is_null() {
                if uninitialize {
                    CoUninitialize();
                }
                return None;
            }
            Some(Manager {
                object: object.cast(),
                uninitialize,
            })
        }
    }

    /// Whether `hwnd` is on the desktop that is shown right now. `false` when Windows cannot say.
    fn on_current_desktop(&self, hwnd: HWND) -> bool {
        let mut yes = 0i32;
        // SAFETY: `object` is the live manager owned by `self`; `yes` is valid for one BOOL.
        let hr = unsafe {
            ((*(*self.object).vtbl).is_window_on_current_virtual_desktop)(
                self.object.cast(),
                hwnd,
                &mut yes,
            )
        };
        hr == S_OK && yes != 0
    }

    /// The id of the desktop `hwnd` is on, as a number. `None` when Windows cannot say.
    fn desktop_of(&self, hwnd: HWND) -> Option<u128> {
        let mut id = GUID::default();
        // SAFETY: `object` is the live manager owned by `self`; `id` is valid for one GUID.
        let hr = unsafe {
            ((*(*self.object).vtbl).get_window_desktop_id)(self.object.cast(), hwnd, &mut id)
        };
        (hr == S_OK).then(|| guid_to_u128(&id))
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        // SAFETY: `object` is a live manager that `self` owns; release comes before uninitialize.
        unsafe {
            ((*(*self.object).vtbl).release)(self.object.cast());
            if self.uninitialize {
                CoUninitialize();
            }
        }
    }
}

/// State of the `EnumWindows` pass of [`shown_desktop`].
struct Shown<'a> {
    manager: &'a Manager,
    own_pid: u32,
    /// The desktop ids of the windows that sit on the shown desktop.
    ids: Vec<u128>,
}

unsafe extern "system" fn collect_shown(hwnd: HWND, data: LPARAM) -> i32 {
    // SAFETY: `data` is the `Shown` passed by `shown_desktop`, alive for the whole enumeration.
    let shown = unsafe { &mut *(data as *mut Shown) };
    // the same kind of window that `windows_of` offers to the user: not helpers, tool windows
    // or CursorPie's own windows
    let ordinary = unsafe {
        IsWindowVisible(hwnd) != 0
            && GetWindowTextLengthW(hwnd) != 0
            && GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW == 0
    };
    if ordinary && pid_of(hwnd) != shown.own_pid && shown.manager.on_current_desktop(hwnd) {
        shown.ids.extend(shown.manager.desktop_of(hwnd));
    }
    1
}

/// Tells the desktop that is shown from the others: a number made of the desktop ids of the
/// ordinary windows on it (see [`desktop_signature`]). There is no call for "the current desktop",
/// and asking one window would be wrong for a window that is pinned to every desktop. `None` for a
/// desktop with no such window.
fn shown_desktop(manager: &Manager) -> Option<u64> {
    let mut shown = Shown {
        manager,
        // SAFETY: plain query.
        own_pid: unsafe { GetCurrentProcessId() },
        ids: Vec::new(),
    };
    // SAFETY: the callback only runs during this call and `shown` outlives it.
    unsafe { EnumWindows(Some(collect_shown), &mut shown as *mut Shown as LPARAM) };
    desktop_signature(shown.ids)
}

/// The row of virtual desktops as [`walk_to_target`] needs it: the windows of one app, looked up
/// before the first step.
struct Row<'a> {
    manager: &'a Manager,
    windows: &'a [HWND],
}

impl Row<'_> {
    /// Whether any window of the app is on the desktop that is shown.
    fn app_is_here(&self) -> bool {
        self.windows
            .iter()
            .any(|&w| self.manager.on_current_desktop(w))
    }
}

impl Desktops for Row<'_> {
    type Id = u64;

    fn target_here(&mut self) -> bool {
        self.app_is_here()
    }

    fn current(&mut self) -> Option<u64> {
        shown_desktop(self.manager)
    }

    fn step(&mut self, forward: bool) {
        let before = self.current();
        let arrow = if forward { key::RIGHT } else { key::LEFT };
        super::press_shortcut(Shortcut::new(arrow, MOD_CMD | MOD_CTRL));
        // Windows switches asynchronously: wait until the answer changes, or until it is clear that
        // nothing happens (the end of the row)
        let start = Instant::now();
        while start.elapsed() < SETTLE {
            std::thread::sleep(POLL);
            if self.app_is_here() || self.current() != before {
                break;
            }
        }
    }
}

/// The "Open app" action with desktop awareness: when the app is running and none of its windows is
/// on the shown desktop, go to the desktop that has one and bring it to the front; in every other
/// case open the app as before. `app` is a program name or a path, as typed in a slice.
///
/// Windows only offers [`SwitchToApp`](DesktopMode::SwitchToApp) (see
/// [`Os::desktop_modes`](crate::platform::Os::desktop_modes)), so `_mode` is not looked at: the
/// caller has already turned any other mode into it with [`DesktopMode::effective`].
pub fn reveal_app(app: &str, _mode: DesktopMode) {
    if switch_to_apps_desktop(app) {
        return;
    }
    // Not on another desktop. If the app already has a window, that window is what was asked for:
    // start the program again only when there is none. Starting it again is what a *new window*
    // does (Chrome, Explorer, terminals and editors answer a second start with one), so doing it
    // here made the yellow badge of "Open app" (go to the app that is open) open a new window.
    if !front_existing(app) {
        open_app(app);
    }
}

/// What [`reveal_app`] does when the app is not on another desktop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reveal {
    /// Put this window in front.
    Front(usize),
    /// The app has no window: start it.
    Start,
}

/// Which of `windows` (top-most first) to put in front. One on the desktop that is shown wins,
/// `on_here` says which those are; else the top-most one (Windows takes the user to its desktop).
fn pick_window(windows: &[bool]) -> Reveal {
    match windows.iter().position(|&here| here) {
        Some(i) => Reveal::Front(i),
        None if windows.is_empty() => Reveal::Start,
        None => Reveal::Front(0),
    }
}

/// Puts an existing window of `app` in front (restoring it if minimized). `false` when the app has
/// no window, so the caller starts it.
fn front_existing(app: &str) -> bool {
    let Some(id) = app_id_for_open(app) else {
        return false;
    };
    let windows = windows_of(&id);
    let manager = Manager::new();
    let here: Vec<bool> = windows
        .iter()
        .map(|&w| manager.as_ref().is_some_and(|m| m.on_current_desktop(w)))
        .collect();
    match pick_window(&here) {
        Reveal::Front(i) => {
            bring_to_front(&id, windows[i]);
            true
        }
        Reveal::Start => false,
    }
}

/// Goes to the desktop of the app `app` names and fronts its window. `false` when that was not
/// needed or not possible, which leaves the user on the desktop they were on.
fn switch_to_apps_desktop(app: &str) -> bool {
    let Some(id) = app_id_for_open(app) else {
        return false;
    };
    let windows = windows_of(&id);
    if windows.is_empty() {
        return false; // not running: opening it starts it on this desktop
    }
    let Some(manager) = Manager::new() else {
        return false;
    };
    let mut row = Row {
        manager: &manager,
        windows: &windows,
    };
    if row.app_is_here() || !walk_to_target(&mut row, MAX_STEPS) {
        return false;
    }
    // the top-most window of the app that is on this desktop, which is not always the top-most
    // overall: fronting that one could take the user to yet another desktop
    match windows.iter().find(|&&w| manager.on_current_desktop(w)) {
        Some(&hwnd) => {
            bring_to_front(&id, hwnd);
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vtable_has_the_slots_of_iunknown_and_the_three_methods_in_order() {
        // windows-sys has no interface definitions, so the layout is written by hand: a slot out
        // of place would call the wrong method
        let slot = std::mem::size_of::<usize>();
        assert_eq!(std::mem::size_of::<ManagerVtbl>(), 6 * slot);
        assert_eq!(std::mem::offset_of!(ManagerVtbl, release), 2 * slot);
        assert_eq!(
            std::mem::offset_of!(ManagerVtbl, is_window_on_current_virtual_desktop),
            3 * slot
        );
        assert_eq!(
            std::mem::offset_of!(ManagerVtbl, get_window_desktop_id),
            4 * slot
        );
        assert_eq!(
            std::mem::offset_of!(ManagerVtbl, move_window_to_desktop),
            5 * slot
        );
    }

    #[test]
    fn an_app_with_a_window_is_fronted_not_started_again() {
        assert_eq!(pick_window(&[]), Reveal::Start, "no window: start it");
        assert_eq!(pick_window(&[true]), Reveal::Front(0));
        // the top-most window is on another desktop, the second one is here: that one
        assert_eq!(pick_window(&[false, true, true]), Reveal::Front(1));
        // none here (no virtual desktops reported): the top-most one
        assert_eq!(pick_window(&[false, false]), Reveal::Front(0));
    }

    #[test]
    fn the_interface_id_is_the_documented_one() {
        // {A5CD92FF-29BE-454C-8D04-D82879FB3F1B}
        let g = IID_VIRTUAL_DESKTOP_MANAGER;
        assert_eq!((g.data1, g.data2, g.data3), (0xA5CD92FF, 0x29BE, 0x454C));
        assert_eq!(g.data4, [0x8D, 0x04, 0xD8, 0x28, 0x79, 0xFB, 0x3F, 0x1B]);
    }
}
