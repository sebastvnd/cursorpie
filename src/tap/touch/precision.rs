//! The Windows touch source: the contact reports of a Precision Touchpad, through Raw Input.
//!
//! **Everything here is a design assumption until spikes W1 to W5 of
//! `docs/TRACKPAD_HAPTICS_PLAN.md` are answered on a Windows laptop.** Unlike macOS, Windows has a
//! public interface for this: a program registers for the HID *Digitizer / Touch Pad* usage page
//! with `RegisterRawInputDevices` and gets one `WM_INPUT` per report, even when it is in the
//! background (`RIDEV_INPUTSINK`). Nothing private is loaded, and nothing is registered unless the
//! user turned `touch_trigger` on.
//!
//! | Part | What it does |
//! |---|---|
//! | the window | a hidden top-level window on its own thread (`cursorpie-touch`) that only receives `WM_INPUT`, `WM_INPUT_DEVICE_CHANGE` and a timer |
//! | [`Device`] | the descriptor of one touchpad (`HidP_GetValueCaps`), read once; reads the values of a report with `HidP_GetUsageValue` and `HidP_GetUsages` |
//! | [`ptp`] | everything that is a decision (layout, hybrid reports, palms), tested on every OS |
//! | [`Bridge`](super::Bridge) | the same translation into the gesture state machine that macOS uses |
//!
//! Rules it follows, like the hook of `tap::windows`: nothing blocks, nothing panics out of the
//! window procedure, and the mouse is never affected. A device that is not a precision touchpad
//! (an older touchpad that Windows treats as a plain mouse sends no touch reports at all) simply
//! never appears, and Settings says "No precision touchpad found".
//!
//! Reports are read only for the number of contacts a frame has, so a resting hand with five
//! fingers on the pad costs five slots of reading, not ten.

use super::ptp::{self, Assembler, Cap, RawContact, Spec};
use super::recognizer::{Finger, Frame};
use super::{Bridge, Env, Status, now, set_status};
use crate::config::{cfg, lock};
use crate::geometry::Point;
use crate::msg::Notifier;
use crate::platform::windows::util::wide;
use crate::tap::gesture::Gesture;
use std::collections::HashMap;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    HIDP_CAPS, HIDP_VALUE_CAPS, HidP_GetCaps, HidP_GetUsageValue, HidP_GetUsages,
    HidP_GetValueCaps, HidP_Input,
};
use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, HRAWINPUT, RAWINPUTDEVICE,
    RAWINPUTDEVICELIST, RAWINPUTHEADER, RID_DEVICE_INFO, RegisterRawInputDevices,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, RegisterClassW, SetTimer,
    TranslateMessage, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};

/// `GetRawInputData`: the whole packet (`RID_INPUT`).
const RID_INPUT: u32 = 0x1000_0003;
/// `GetRawInputDeviceInfo`: the parsed report descriptor (`RIDI_PREPARSEDDATA`).
const RIDI_PREPARSEDDATA: u32 = 0x2000_0005;
/// `GetRawInputDeviceInfo`: the `RID_DEVICE_INFO` (`RIDI_DEVICEINFO`).
const RIDI_DEVICEINFO: u32 = 0x2000_000B;
/// Receive the input also in the background (`RIDEV_INPUTSINK`).
const RIDEV_INPUTSINK: u32 = 0x0000_0100;
/// Get `WM_INPUT_DEVICE_CHANGE` when a device comes or goes (`RIDEV_DEVNOTIFY`).
const RIDEV_DEVNOTIFY: u32 = 0x0000_2000;
/// Stop receiving (`RIDEV_REMOVE`).
const RIDEV_REMOVE: u32 = 0x0000_0001;
/// `wParam` of `WM_INPUT_DEVICE_CHANGE` when a device was removed (`GIDC_REMOVAL`).
const GIDC_REMOVAL: usize = 2;
/// `HIDP_STATUS_SUCCESS`.
const HIDP_OK: i32 = 0x0011_0000;

/// `RID_DEVICE_INFO` is read as eight `u32` by [`ptp::is_touchpad_info`].
const _: () = assert!(std::mem::size_of::<RID_DEVICE_INFO>() == 32);

/// How often the config switch and stale gestures are looked at.
const TICK_MS: u32 = 250;
/// How often the list of touchpads is counted again (one connected later is found without a restart).
const RESCAN: Duration = Duration::from_secs(5);
/// A tick that came this late means the PC slept: the registration is renewed.
const SLEEP_GAP: Duration = Duration::from_secs(3);
/// Most fingers passed to the recognizer from one frame.
const MAX_FINGERS: usize = 10;
/// A packet or a descriptor larger than this is not believed.
const MAX_BYTES: u32 = 1 << 20;
/// Id of the timer of the window.
const TIMER_ID: usize = 1;

// ------------------------------------------------------------------ one touchpad

/// A precision touchpad that sends reports.
struct Device {
    spec: Spec,
    /// The parsed descriptor `pp` points into (`u64`s keep it aligned).
    _data: Vec<u64>,
    /// `PHIDP_PREPARSED_DATA`.
    pp: isize,
    asm: Assembler,
}

impl Device {
    /// Reads the descriptor of a device. `None` if it is not a precision touchpad or Windows does not
    /// say (the device is then ignored for the session).
    fn load(device: u64) -> Option<Device> {
        let handle = device as usize as HANDLE;
        let mut size: u32 = 0;
        // SAFETY: with a null buffer the call only reports the size it needs.
        unsafe { GetRawInputDeviceInfoW(handle, RIDI_PREPARSEDDATA, null_mut(), &mut size) };
        if size == 0 || size > MAX_BYTES {
            return None;
        }
        let mut data = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: `data` has room for `size` bytes.
        let got = unsafe {
            GetRawInputDeviceInfoW(
                handle,
                RIDI_PREPARSEDDATA,
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if got == u32::MAX {
            return None;
        }
        let pp = data.as_ptr() as isize;
        // SAFETY: plain data, all zeroes is a valid value.
        let mut caps: HIDP_CAPS = unsafe { std::mem::zeroed() };
        // SAFETY: `pp` is the preparsed data Windows just gave us, `caps` is valid for writes.
        if unsafe { HidP_GetCaps(pp, &mut caps) } != HIDP_OK {
            return None;
        }
        if caps.UsagePage != ptp::PAGE_DIGITIZER || caps.Usage != ptp::USAGE_TOUCH_PAD {
            return None;
        }
        let mut len = caps.NumberInputValueCaps;
        let mut values: Vec<HIDP_VALUE_CAPS> = Vec::with_capacity(len as usize);
        // SAFETY: plain data, all zeroes is a valid value.
        values.resize_with(len as usize, || unsafe { std::mem::zeroed() });
        // SAFETY: `values` has room for `len` entries; `len` is updated to what was written.
        if unsafe { HidP_GetValueCaps(HidP_Input, values.as_mut_ptr(), &mut len, pp) } != HIDP_OK {
            return None;
        }
        values.truncate(len as usize);
        let caps: Vec<Cap> = values
            .iter()
            .filter(|c| !c.IsRange) // BOOLEAN is a `bool` in windows-sys
            .map(|c| Cap {
                page: c.UsagePage,
                // SAFETY: the value is not a range, so the union holds the single usage.
                usage: unsafe { c.Anonymous.NotRange.Usage },
                link: c.LinkCollection,
                report_id: c.ReportID,
                logical_min: c.LogicalMin,
                logical_max: c.LogicalMax,
                physical_min: c.PhysicalMin,
                physical_max: c.PhysicalMax,
                units: c.Units,
                units_exp: c.UnitsExp,
            })
            .collect();
        let Some(spec) = ptp::layout_from_caps(&caps) else {
            diag::log!(
                "touch",
                "device_skipped",
                why = "no_layout",
                values = caps.len()
            );
            return None;
        };
        diag::log!(
            "touch",
            "layout",
            slots = spec.slots.len(),
            w_mm = spec.surface.size_mm.0,
            h_mm = spec.surface.size_mm.1
        );
        Some(Device {
            spec,
            _data: data,
            pp,
            asm: Assembler::default(),
        })
    }

    /// One value of a report; `None` if Windows cannot read it.
    fn value(&self, link: u16, page: u16, usage: u16, report: &mut [u8]) -> Option<u32> {
        let mut v: u32 = 0;
        // SAFETY: `self.pp` points into `self._data`, which is alive; `report` is a report of this
        // device and its length is passed.
        let status = unsafe {
            HidP_GetUsageValue(
                HidP_Input,
                page,
                link,
                usage,
                &mut v,
                self.pp,
                report.as_mut_ptr() as _,
                report.len() as u32,
            )
        };
        (status == HIDP_OK).then_some(v)
    }

    /// The two one-bit flags of a finger slot: `(tip switch, confidence)`.
    fn flags(&self, link: u16, report: &mut [u8]) -> (bool, bool) {
        let mut usages = [0u16; 16];
        let mut len = usages.len() as u32;
        // SAFETY: as in `value`; `usages` has room for `len` entries.
        let status = unsafe {
            HidP_GetUsages(
                HidP_Input,
                ptp::PAGE_DIGITIZER,
                link,
                usages.as_mut_ptr(),
                &mut len,
                self.pp,
                report.as_mut_ptr() as _,
                report.len() as u32,
            )
        };
        if status != HIDP_OK {
            return (false, false);
        }
        let set = &usages[..(len as usize).min(usages.len())];
        (
            set.contains(&ptp::USAGE_TIP),
            set.contains(&ptp::USAGE_CONFIDENCE),
        )
    }

    /// The finger slot `slot` of a report.
    fn contact(&self, slot: usize, report: &mut [u8]) -> RawContact {
        let link = self.spec.slots[slot];
        let (tip, confidence) = self.flags(link, report);
        let x = self.value(link, ptp::PAGE_GENERIC, ptp::USAGE_X, report);
        let y = self.value(link, ptp::PAGE_GENERIC, ptp::USAGE_Y, report);
        let id = self
            .value(link, ptp::PAGE_DIGITIZER, ptp::USAGE_CONTACT_ID, report)
            .map_or(slot as i32, |v| v as i32);
        match (x, y) {
            (Some(x), Some(y)) => RawContact {
                id,
                tip,
                confidence,
                x: x as i32,
                y: y as i32,
            },
            // a slot Windows cannot read is a finger that is not there
            _ => RawContact::default(),
        }
    }

    /// Reads one report. `true` when it completed a frame, which is then in `out`.
    fn report(&mut self, report: &[u8], out: &mut Vec<RawContact>) -> bool {
        if report.first() != Some(&self.spec.report_id) {
            return false; // another report of the device (buttons, feature data)
        }
        let mut rep = report.to_vec();
        let Some(count) = self.value(
            self.spec.count_link,
            ptp::PAGE_DIGITIZER,
            ptp::USAGE_CONTACT_COUNT,
            &mut rep,
        ) else {
            return false;
        };
        let count = count as usize;
        let want = self.asm.wanted(count, self.spec.slots.len());
        let mut contacts = [RawContact::default(); ptp::MAX_SLOTS];
        for (i, slot) in contacts.iter_mut().enumerate().take(want) {
            *slot = self.contact(i, &mut rep);
        }
        self.asm.push(count, &contacts[..want], out)
    }
}

// ------------------------------------------------------------------ the shared hub

/// What the window procedure (no user data) can reach.
struct Hub {
    bridge: Mutex<Bridge>,
    state: Arc<Mutex<Gesture>>,
    notifier: Notifier,
    /// Per device handle; `None` is a device that was looked at and is not usable.
    devices: Mutex<HashMap<u64, Option<Device>>>,
    run: Mutex<Run>,
    /// Reports are processed only while this is true.
    enabled: AtomicBool,
}

/// What the timer remembers between ticks.
struct Run {
    registered: bool,
    last_tick: Instant,
    last_scan: Option<Instant>,
    /// The number of touchpads at the last count (to log only a change).
    touchpads: Option<usize>,
}

static HUB: OnceLock<Hub> = OnceLock::new();
/// A device was added or removed: count the touchpads again at the next tick.
static RESCAN_NOW: AtomicBool = AtomicBool::new(false);

fn cursor() -> Option<Point> {
    crate::platform::pointer_pos().map(|(x, y)| Point::new(x, y))
}

fn reset_bridge(hub: &Hub) {
    let cur = cursor;
    let env = Env {
        state: &hub.state,
        notifier: &hub.notifier,
        cursor: &cur,
    };
    lock(&hub.bridge).reset(&env);
}

/// One `WM_INPUT`: reads the packet and feeds the frames it completes to the bridge.
fn on_input(raw: HRAWINPUT) {
    let Some(hub) = HUB.get() else { return };
    if !hub.enabled.load(Ordering::Relaxed) {
        return;
    }
    let header = std::mem::size_of::<RAWINPUTHEADER>();
    let mut size: u32 = 0;
    // SAFETY: with a null buffer the call only reports the size of the packet.
    let asked = unsafe { GetRawInputData(raw, RID_INPUT, null_mut(), &mut size, header as u32) };
    if asked != 0 || size == 0 || size > MAX_BYTES {
        return;
    }
    let mut buf = vec![0u64; (size as usize).div_ceil(8)];
    // SAFETY: `buf` has room for `size` bytes.
    let got = unsafe {
        GetRawInputData(
            raw,
            RID_INPUT,
            buf.as_mut_ptr().cast(),
            &mut size,
            header as u32,
        )
    };
    if got == u32::MAX || got as usize > buf.len() * 8 {
        return;
    }
    // SAFETY: `got` bytes of `buf` were written by the call above.
    let bytes = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), got as usize) };
    let Some(hid) = ptp::parse_raw_hid(bytes, header) else {
        return;
    };

    let mut devices = lock(&hub.devices);
    let slot = devices
        .entry(hid.device)
        .or_insert_with(|| Device::load(hid.device));
    let Some(dev) = slot.as_mut() else { return };
    let cur = cursor;
    let env = Env {
        state: &hub.state,
        notifier: &hub.notifier,
        cursor: &cur,
    };
    let mut frame: Vec<RawContact> = Vec::with_capacity(ptp::MAX_SLOTS);
    for report in hid.reports() {
        if !dev.report(report, &mut frame) {
            continue;
        }
        let mut fingers = [Finger {
            id: 0,
            x: 0.0,
            y: 0.0,
            size: 0.0,
        }; MAX_FINGERS];
        let mut n = 0;
        for c in &frame {
            if n < MAX_FINGERS
                && let Some(f) = ptp::finger(c, &dev.spec.surface)
            {
                fingers[n] = f;
                n += 1;
            }
        }
        lock(&hub.bridge).on_frame(
            &env,
            &Frame {
                device: hid.device,
                time: now(),
                size_mm: dev.spec.surface.size_mm,
                fingers: &fingers[..n],
            },
        );
    }
}

/// A device was removed: its gesture ends.
fn on_device_removed(handle: u64) {
    let Some(hub) = HUB.get() else { return };
    lock(&hub.devices).remove(&handle);
    let cur = cursor;
    let env = Env {
        state: &hub.state,
        notifier: &hub.notifier,
        cursor: &cur,
    };
    lock(&hub.bridge).device_lost(&env, handle);
}

// ------------------------------------------------------------------ the list of touchpads

fn is_touchpad(handle: HANDLE) -> bool {
    let mut info = [0u32; 8];
    info[0] = 32; // cbSize
    let mut size: u32 = 32;
    // SAFETY: `info` is 32 bytes, the size of `RID_DEVICE_INFO`, and `cbSize` is set.
    let got = unsafe {
        GetRawInputDeviceInfoW(handle, RIDI_DEVICEINFO, info.as_mut_ptr().cast(), &mut size)
    };
    got != u32::MAX && ptp::is_touchpad_info(&info)
}

/// How many precision touchpads Windows lists.
fn count_touchpads() -> usize {
    let item = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
    let mut n: u32 = 0;
    // SAFETY: with a null list the call only reports how many devices there are.
    if unsafe { GetRawInputDeviceList(null_mut(), &mut n, item) } == u32::MAX || n == 0 {
        return 0;
    }
    let mut list: Vec<RAWINPUTDEVICELIST> = Vec::with_capacity(n as usize);
    // SAFETY: plain data, all zeroes is a valid value.
    list.resize_with(n as usize, || unsafe { std::mem::zeroed() });
    // SAFETY: `list` has room for `n` entries; `n` is updated to what was written.
    let got = unsafe { GetRawInputDeviceList(list.as_mut_ptr(), &mut n, item) };
    if got == u32::MAX {
        return 0;
    }
    list.iter()
        .take(got as usize)
        .filter(|d| d.dwType == ptp::RIM_TYPEHID && is_touchpad(d.hDevice))
        .count()
}

// ------------------------------------------------------------------ the window

/// Starts or stops the delivery of touchpad reports to `hwnd`.
fn register(hwnd: HWND, on: bool) -> bool {
    let device = RAWINPUTDEVICE {
        usUsagePage: ptp::PAGE_DIGITIZER,
        usUsage: ptp::USAGE_TOUCH_PAD,
        dwFlags: if on {
            RIDEV_INPUTSINK | RIDEV_DEVNOTIFY
        } else {
            RIDEV_REMOVE
        },
        hwndTarget: if on { hwnd } else { null_mut() },
    };
    // SAFETY: one valid entry, and its size.
    unsafe {
        RegisterRawInputDevices(&device, 1, std::mem::size_of::<RAWINPUTDEVICE>() as u32) != 0
    }
}

/// The timer: the setting is looked at, the touchpads are counted, a gesture whose device went
/// quiet is cancelled.
fn tick(hwnd: HWND) {
    let Some(hub) = HUB.get() else { return };
    let on = cfg().touch_trigger;
    let mut run = lock(&hub.run);
    let slept = run.last_tick.elapsed() > SLEEP_GAP;
    run.last_tick = Instant::now();

    if run.registered && (!on || slept) {
        hub.enabled.store(false, Ordering::SeqCst);
        register(hwnd, false);
        run.registered = false;
        lock(&hub.devices).clear();
        reset_bridge(hub);
        diag::log!("touch", "source", state = "stopped", slept = slept);
    }
    if !on {
        set_status(Status::Off);
        return;
    }
    if !run.registered {
        if !register(hwnd, true) {
            set_status(Status::Unavailable);
            diag::log!("touch", "source", state = "refused");
            return;
        }
        hub.enabled.store(true, Ordering::SeqCst);
        run.registered = true;
        run.last_scan = None;
        diag::log!("touch", "source", state = "running");
    }
    if RESCAN_NOW.swap(false, Ordering::SeqCst)
        || run.last_scan.is_none_or(|t| t.elapsed() >= RESCAN)
    {
        run.last_scan = Some(Instant::now());
        let n = count_touchpads();
        if run.touchpads != Some(n) {
            run.touchpads = Some(n);
            diag::log!("touch", "devices", seen = n, started = n);
        }
        set_status(if n == 0 {
            Status::NoDevice
        } else {
            Status::Ready {
                devices: n,
                built_in: false,
            }
        });
    }
    drop(run);

    let cur = cursor;
    let env = Env {
        state: &hub.state,
        notifier: &hub.notifier,
        cursor: &cur,
    };
    lock(&hub.bridge).poll(&env);
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // never panic out of a window procedure: fail quiet
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match msg {
        ptp::WM_INPUT => on_input(lparam as HRAWINPUT),
        ptp::WM_INPUT_DEVICE_CHANGE => {
            RESCAN_NOW.store(true, Ordering::SeqCst);
            if wparam == GIDC_REMOVAL {
                on_device_removed(lparam as usize as u64);
            }
        }
        WM_TIMER => tick(hwnd),
        _ => {}
    }));
    // SAFETY: forwards the arguments unchanged (the docs ask for this after `WM_INPUT`, for cleanup).
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// A hidden window that only receives the messages above.
fn create_window() -> HWND {
    let class = wide("CursorPieTouchSink");
    // SAFETY: plain Win32 calls; `class` outlives them and the window procedure is `'static`.
    unsafe {
        let module = GetModuleHandleW(null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(window_proc);
        wc.hInstance = module;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);
        CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            class.as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            module,
            null(),
        )
    }
}

/// Starts the thread of the source. It idles until the setting `touch_trigger` is on: only then is
/// anything registered with Windows.
pub(super) fn spawn(state: Arc<Mutex<Gesture>>, notifier: Notifier) {
    HUB.get_or_init(|| Hub {
        bridge: Mutex::new(Bridge::new()),
        state,
        notifier,
        devices: Mutex::new(HashMap::new()),
        run: Mutex::new(Run {
            registered: false,
            last_tick: Instant::now(),
            last_scan: None,
            touchpads: None,
        }),
        enabled: AtomicBool::new(false),
    });
    let _ = std::thread::Builder::new()
        .name("cursorpie-touch".into())
        .spawn(|| {
            let hwnd = create_window();
            if hwnd.is_null() {
                set_status(Status::Unavailable);
                diag::log!(
                    "touch",
                    "source",
                    state = "no_window",
                    os_error = std::io::Error::last_os_error().to_string()
                );
                return;
            }
            // SAFETY: `hwnd` is the window of this thread, which then pumps its messages.
            unsafe {
                SetTimer(hwnd, TIMER_ID, TICK_MS, None);
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        });
}
