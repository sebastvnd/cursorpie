//! Windows hook: `WH_MOUSE_LL` and `WH_KEYBOARD_LL` on their own thread with a message loop.
//!
//! [`spawn`] installs both hooks, publishes `TAP_ACTIVE` and pumps messages; when Windows refuses
//! the hooks it retries every 2 seconds. Native events are translated into the portable
//! `Ev` and routed by `dispatch`.
//!
//! Differences from the macOS tap, all handled here:
//!
//! * Windows has no "drag" event: a pointer move arrives as a plain move. Moves are only looked at
//!   while a gesture is in progress, and they are **never swallowed** (dropping them would freeze
//!   the cursor), whatever the state machine answers.
//! * Coordinates are physical pixels and are converted to logical points with the monitor scale.
//! * Windows silently removes a low-level hook whose callback takes too long (about 300 ms by
//!   default). The callback therefore only does atomic loads and short locks, like the macOS tap.
//! * Windows also removes the hooks without a word when the machine resumes, when the session
//!   is locked and unlocked, or after one callback was too slow, and the message loop then simply
//!   never gets another call. A **watchdog** (a thread timer of this loop) notices: the system still sees
//!   input, but no hook has run for a while. It then drops the hooks and installs them again at
//!   once (see `hooks_went_quiet`). A tick of the watchdog that comes far too late means the
//!   machine slept: the hooks are installed again then too, before the first press (see
//!   `woke_up`).
//! * A precision touchpad is read by its own thread (`touch::precision`, started by [`spawn`]). The
//!   hook only helps it in one way: while a touchpad pie is open, a left click (or the right click
//!   of a two-finger tap) may be the extra finger's click and is swallowed (`touch::extra`).

use super::{Ev, Gesture, Kind, Verdict, dispatch};
use crate::actions::SYNTH_TAG;
use crate::config::{cfg, lock};
use crate::geometry::Point;
use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT};
use crate::msg::Notifier;
use crate::platform::keymap;
use crate::platform::windows::{px_to_logical, scale_at_physical, set_last_scale};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, HC_ACTION, KBDLLHOOKSTRUCT, KillTimer, LLKHF_EXTENDED, MSG,
    MSLLHOOKSTRUCT, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP,
    WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_TIMER, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

/// What the hook callbacks need. Hooks have no user-data pointer, so it lives in a static.
struct Ctx {
    state: Arc<Mutex<Gesture>>,
    notifier: Notifier,
}

static CTX: OnceLock<Ctx> = OnceLock::new();

/// How often the watchdog looks at the hooks (a thread timer, in milliseconds).
const WATCHDOG_MS: u32 = 2_000;
/// Input newer than this must have reached the hooks; older input says nothing about them.
const RECENT_INPUT_MS: u32 = 4_000;
/// How much older than the last input the last hook call may be before the hooks count as gone.
/// A live hook runs within a few milliseconds of the input it is about.
const STALE_MS: u32 = 1_500;
/// The shortest time between two reinstalls the watchdog asks for. Input that Windows does not
/// pass to a hook (a window of a program run as administrator) looks like a dead hook too, and
/// must not make the hooks restart every few seconds.
const MIN_RESTART_GAP: Duration = Duration::from_secs(15);

/// A watchdog tick that comes this much later than the one before means the machine slept (or the
/// thread was starved): resuming is when Windows is known to drop the hooks, so they are installed
/// again at once, before anyone has pressed anything.
const RESUME_GAP: Duration = Duration::from_secs(10);

/// `GetTickCount` at the moment a hook callback last ran: the watchdog's proof of life. A relaxed
/// atomic store, so it costs the callbacks nothing worth measuring.
static HEARTBEAT: AtomicU32 = AtomicU32::new(0);

/// Records that a hook callback is running.
fn beat() {
    // SAFETY: plain query of the tick counter.
    HEARTBEAT.store(unsafe { GetTickCount() }, Ordering::Relaxed);
}

/// Milliseconds from `then` to `now` on the 32-bit tick counter, which wraps every 49 days. A
/// `then` that is a hair after `now` (the two were read at slightly different moments) is 0.
fn ago(now: u32, then: u32) -> u32 {
    (now.wrapping_sub(then) as i32).max(0) as u32
}

/// Whether the hooks have stopped seeing input: the system saw some `since_input` ms ago (recent),
/// yet no hook has run for much longer than that (`since_beat` ms). Pure, so it is unit tested.
pub(super) fn hooks_went_quiet(since_input: u32, since_beat: u32) -> bool {
    since_input <= RECENT_INPUT_MS && since_beat > since_input.saturating_add(STALE_MS)
}

/// Whether the time between two watchdog ticks (by the wall clock, which keeps counting while the
/// machine sleeps; `None` if the clock was set back) shows that the machine slept. Pure.
pub(super) fn woke_up(gap: Option<Duration>) -> bool {
    gap.is_some_and(|g| g > RESUME_GAP)
}

/// Asks the system when input last happened and compares it with the last hook call.
fn quiet_hooks() -> bool {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a live LASTINPUTINFO with its size set; the tick counter is a plain query.
    unsafe {
        if GetLastInputInfo(&mut info) == 0 {
            return false;
        }
        let now = GetTickCount();
        hooks_went_quiet(
            ago(now, info.dwTime),
            ago(now, HEARTBEAT.load(Ordering::Relaxed)),
        )
    }
}

/// Classifies a mouse message: `(kind, macOS button number)`. `mouse_data` is
/// `MSLLHOOKSTRUCT::mouseData` (the high word names the X button). `None` for left-button events
/// and everything else CursorPie does not use. A plain move is `(Kind::Drag, 0)`: the caller decides
/// which button it belongs to.
pub(super) fn classify_mouse(msg: u32, mouse_data: u32) -> Option<(Kind, i64)> {
    let x_button = || match mouse_data >> 16 {
        1 => Some(super::SIDE_BACK),
        2 => Some(super::SIDE_FORWARD),
        _ => None,
    };
    match msg {
        WM_MBUTTONDOWN => Some((Kind::Down, 2)),
        WM_MBUTTONUP => Some((Kind::Up, 2)),
        WM_RBUTTONDOWN => Some((Kind::Down, super::RIGHT_BUTTON)),
        WM_RBUTTONUP => Some((Kind::Up, super::RIGHT_BUTTON)),
        WM_XBUTTONDOWN => x_button().map(|b| (Kind::Down, b)),
        WM_XBUTTONUP => x_button().map(|b| (Kind::Up, b)),
        WM_MOUSEMOVE => Some((Kind::Drag, 0)),
        _ => None,
    }
}

/// Whether a mouse message is the wheel (vertical or horizontal): the scrolling of a touchpad too.
pub(super) fn is_wheel(msg: u32) -> bool {
    matches!(msg, WM_MOUSEWHEEL | WM_MOUSEHWHEEL)
}

/// The left button, which `classify_mouse` leaves out: `Some` for its press and release. It is only
/// looked at while a touchpad pie is open (the extra finger's click, see `touch::extra`).
pub(super) fn classify_left(msg: u32) -> Option<Kind> {
    match msg {
        WM_LBUTTONDOWN => Some(Kind::Down),
        WM_LBUTTONUP => Some(Kind::Up),
        _ => None,
    }
}

/// Whether a keyboard message is a key press (`Some(true)`), a release (`Some(false)`) or neither.
pub(super) fn key_is_down(msg: u32) -> Option<bool> {
    match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
        WM_KEYUP | WM_SYSKEYUP => Some(false),
        _ => None,
    }
}

/// Whether a modifier key is held right now.
fn held(vk: i32) -> bool {
    // SAFETY: plain query; the high bit means "down".
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

/// The `MOD_*` bitmask of the modifiers held right now.
fn current_mods() -> u8 {
    let mut m = 0;
    if held(0x10) {
        m |= MOD_SHIFT;
    }
    if held(0x11) {
        m |= MOD_CTRL;
    }
    if held(0x12) {
        m |= MOD_ALT;
    }
    if held(0x5B) || held(0x5C) {
        m |= MOD_CMD;
    }
    m
}

/// The `MOD_*` bit of a modifier's virtual-key code (either side), `None` for other keys.
fn modifier_bit(vk: u16) -> Option<u8> {
    match vk {
        0x10 | 0xA0 | 0xA1 => Some(MOD_SHIFT),
        0x11 | 0xA2 | 0xA3 => Some(MOD_CTRL),
        0x12 | 0xA4 | 0xA5 => Some(MOD_ALT),
        0x5B | 0x5C => Some(MOD_CMD),
        _ => None,
    }
}

/// Handles one mouse event. `true` = swallow.
fn on_mouse(ctx: &Ctx, msg: u32, info: &MSLLHOOKSTRUCT) -> bool {
    if info.dwExtraInfo == SYNTH_TAG as usize {
        return false; // our own replay
    }
    // the holding finger and the extra one are two fingers, which scroll: while a trackpad hold
    // runs that must not reach the app under the pie
    if is_wheel(msg) {
        return super::touch::swallows_scroll() || super::pie_clicks::swallows_wheel(&ctx.state);
    }
    let (kind, mut button) = match classify_mouse(msg, info.mouseData) {
        Some(c) => c,
        // a left click is none of our business unless a touchpad pie is open: one atomic read
        None => match classify_left(msg) {
            Some(kind)
                if super::touch::extra::interested(kind)
                    || super::pie_clicks::interested(&ctx.state, kind) =>
            {
                (kind, super::LEFT_BUTTON)
            }
            _ => return false,
        },
    };
    let is_move = kind == Kind::Drag;
    if is_move {
        // a move only matters while a gesture is running, and belongs to the trigger button
        if matches!(*lock(&ctx.state), Gesture::Idle) {
            return false;
        }
        button = cfg().trigger_button;
    }
    let scale = scale_at_physical(info.pt.x, info.pt.y);
    set_last_scale(scale);
    let pos = Point::new(
        px_to_logical(info.pt.x as f64, scale),
        px_to_logical(info.pt.y as f64, scale),
    );
    // a real drag needs no replay: moves are never swallowed here
    let verdict = dispatch(
        &ctx.state,
        &ctx.notifier,
        Ev::Mouse { kind, button, pos },
        &|| {},
    );
    !is_move && verdict == Verdict::Drop
}

/// Handles one key event. `true` = swallow.
fn on_key(ctx: &Ctx, msg: u32, info: &KBDLLHOOKSTRUCT) -> bool {
    if info.dwExtraInfo == SYNTH_TAG as usize {
        return false;
    }
    let Some(down) = key_is_down(msg) else {
        return false;
    };
    // a modifier is only remembered (the order of a recorded shortcut), never swallowed
    if let Some(bit) = modifier_bit(info.vkCode as u16) {
        if down {
            super::modorder::press(bit);
        } else {
            super::modorder::release(bit);
        }
        return false;
    }
    let extended = info.flags & LLKHF_EXTENDED != 0;
    let Some(code) = keymap::code_for_event(info.vkCode as u16, info.scanCode as u16, extended)
    else {
        return false; // modifiers and keys without a code pass through
    };
    let ev = Ev::Key {
        down,
        code: code as i32,
        mods: current_mods(),
    };
    dispatch(&ctx.state, &ctx.notifier, ev, &|| {}) == Verdict::Drop
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(ctx) = CTX.get()
    {
        beat();
        // SAFETY: for WH_MOUSE_LL with HC_ACTION, `lparam` points to a valid MSLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        // never panic out of a hook: fail open
        let swallow = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_mouse(ctx, wparam as u32, info)
        }))
        .unwrap_or(false);
        if swallow {
            return 1;
        }
    }
    // SAFETY: forwards the arguments unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

unsafe extern "system" fn key_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32
        && let Some(ctx) = CTX.get()
    {
        beat();
        // SAFETY: for WH_KEYBOARD_LL with HC_ACTION, `lparam` points to a valid KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let swallow = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            on_key(ctx, wparam as u32, info)
        }))
        .unwrap_or(false);
        if swallow {
            return 1;
        }
    }
    // SAFETY: forwards the arguments unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Starts the hooks on their own thread and keeps retrying until Windows accepts them.
pub fn spawn(notifier: Notifier) {
    let state = Arc::new(Mutex::new(Gesture::Idle));
    let _ = CTX.set(Ctx {
        state: state.clone(),
        notifier: notifier.clone(),
    });
    // the precision touchpad is a second source of the same gesture (idle until `touch_trigger` is on)
    super::touch::start(state, notifier.clone());
    std::thread::Builder::new()
        .name("cursorpie-tap".into())
        .spawn(move || {
            // a hook callback that waits behind a busy program for about a second is dropped
            crate::platform::windows::raise_current_thread();
            let mut warned = false;
            let mut last_restart: Option<Instant> = None;
            loop {
                // set only by the watchdog: the hooks went quiet, so they are installed again
                // without the pause that follows a refusal
                let mut restart = false;
                // SAFETY: both hooks are installed on this thread, which then pumps messages (a
                // low-level hook is only called while its thread does), and removed afterwards.
                unsafe {
                    let module = GetModuleHandleW(std::ptr::null());
                    let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module, 0);
                    let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), module, 0);
                    if mouse.is_null() || keys.is_null() {
                        for h in [mouse, keys] {
                            if !h.is_null() {
                                UnhookWindowsHookEx(h);
                            }
                        }
                        super::TAP_ACTIVE.store(false, Ordering::SeqCst);
                        if !warned {
                            warned = true;
                            diag::log!(
                                "tap",
                                "hook",
                                backend = "windows",
                                state = "refused",
                                os_error = std::io::Error::last_os_error().to_string()
                            );
                            eprintln!(
                                "CursorPie: Windows refused the input hooks ({}). Retrying every 2 s...",
                                std::io::Error::last_os_error()
                            );
                        }
                        notifier.wake();
                    } else {
                        // a fresh start for the watchdog: the hooks have not been silent yet
                        beat();
                        super::TAP_ACTIVE.store(true, Ordering::SeqCst);
                        diag::log!("tap", "hook", backend = "windows", state = "running");
                        notifier.wake();
                        // a thread timer (no window): its WM_TIMER comes out of GetMessageW below
                        let timer = SetTimer(std::ptr::null_mut(), 0, WATCHDOG_MS, None);
                        let mut last_tick = SystemTime::now();
                        let mut msg: MSG = std::mem::zeroed();
                        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                            if msg.message != WM_TIMER {
                                continue;
                            }
                            let gap = SystemTime::now().duration_since(last_tick).ok();
                            last_tick = SystemTime::now();
                            let quiet = last_restart.is_none_or(|t| t.elapsed() >= MIN_RESTART_GAP)
                                && quiet_hooks();
                            if woke_up(gap) || quiet {
                                restart = true;
                                break;
                            }
                        }
                        if timer != 0 {
                            KillTimer(std::ptr::null_mut(), timer);
                        }
                        super::TAP_ACTIVE.store(false, Ordering::SeqCst);
                        diag::log!(
                            "tap",
                            "hook",
                            backend = "windows",
                            state = if restart { "restarting" } else { "stopped" }
                        );
                        UnhookWindowsHookEx(mouse);
                        UnhookWindowsHookEx(keys);
                        if restart {
                            last_restart = Some(Instant::now());
                            // a press whose release the dropped hook never saw would leave the
                            // pie open and the gesture stuck
                            if let Some(ctx) = CTX.get() {
                                super::reset(&ctx.state, &ctx.notifier);
                            }
                        }
                    }
                }
                if !restart {
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        })
        .expect("failed to spawn event tap thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_messages_map_to_macos_button_numbers() {
        assert_eq!(classify_mouse(WM_MBUTTONDOWN, 0), Some((Kind::Down, 2)));
        assert_eq!(classify_mouse(WM_MBUTTONUP, 0), Some((Kind::Up, 2)));
        assert_eq!(classify_mouse(WM_RBUTTONDOWN, 0), Some((Kind::Down, 1)));
        assert_eq!(classify_mouse(WM_RBUTTONUP, 0), Some((Kind::Up, 1)));
        assert_eq!(classify_mouse(WM_MOUSEMOVE, 0), Some((Kind::Drag, 0)));
    }

    #[test]
    fn x_buttons_are_back_and_forward() {
        let xb1 = 1u32 << 16;
        let xb2 = 2u32 << 16;
        assert_eq!(classify_mouse(WM_XBUTTONDOWN, xb1), Some((Kind::Down, 3)));
        assert_eq!(classify_mouse(WM_XBUTTONUP, xb1), Some((Kind::Up, 3)));
        assert_eq!(classify_mouse(WM_XBUTTONDOWN, xb2), Some((Kind::Down, 4)));
        assert_eq!(classify_mouse(WM_XBUTTONUP, xb2), Some((Kind::Up, 4)));
        assert_eq!(
            classify_mouse(WM_XBUTTONDOWN, 3 << 16),
            None,
            "no third X button"
        );
        assert_eq!(
            classify_mouse(WM_XBUTTONDOWN, 0xFFFF),
            None,
            "the low word is not the button"
        );
    }

    #[test]
    fn both_wheels_are_recognized() {
        assert!(is_wheel(0x020A), "WM_MOUSEWHEEL");
        assert!(is_wheel(0x020E), "WM_MOUSEHWHEEL");
        assert!(!is_wheel(WM_MOUSEMOVE));
        assert!(!is_wheel(WM_LBUTTONDOWN));
    }

    #[test]
    fn the_left_button_and_the_wheel_are_ignored() {
        assert_eq!(classify_mouse(0x0201, 0), None, "WM_LBUTTONDOWN");
        assert_eq!(classify_mouse(0x0202, 0), None, "WM_LBUTTONUP");
        assert_eq!(classify_mouse(0x020A, 0), None, "WM_MOUSEWHEEL");
    }

    #[test]
    fn the_left_button_is_only_a_candidate_for_the_extra_click() {
        assert_eq!(classify_left(WM_LBUTTONDOWN), Some(Kind::Down));
        assert_eq!(classify_left(WM_LBUTTONUP), Some(Kind::Up));
        assert_eq!(classify_left(WM_MBUTTONDOWN), None);
        assert_eq!(
            classify_left(WM_RBUTTONDOWN),
            None,
            "the right button has its own path"
        );
        assert_eq!(classify_left(WM_MOUSEMOVE), None);
    }

    #[test]
    fn live_hooks_are_never_called_quiet() {
        // the user is moving the pointer: the hook ran a few ms after the input
        assert!(!hooks_went_quiet(10, 12));
        assert!(!hooks_went_quiet(0, 0));
        // typed once a moment ago and then nothing: both clocks agree
        assert!(!hooks_went_quiet(3_000, 3_020));
    }

    #[test]
    fn hooks_that_missed_recent_input_are_quiet() {
        // input 10 ms ago, but the last hook call was 5 s ago
        assert!(hooks_went_quiet(10, 5_000));
        assert!(hooks_went_quiet(2_000, 2_000 + STALE_MS + 1));
    }

    #[test]
    fn the_stale_margin_is_exclusive() {
        assert!(!hooks_went_quiet(2_000, 2_000 + STALE_MS));
    }

    #[test]
    fn old_input_says_nothing_about_the_hooks() {
        // nobody touched the machine for a minute: no hook call is not a sign of anything
        assert!(!hooks_went_quiet(60_000, 600_000));
        assert!(!hooks_went_quiet(RECENT_INPUT_MS + 1, 600_000));
        assert!(hooks_went_quiet(
            RECENT_INPUT_MS,
            RECENT_INPUT_MS + STALE_MS + 1
        ));
    }

    #[test]
    fn a_late_watchdog_tick_means_the_machine_slept() {
        let tick = Duration::from_millis(u64::from(WATCHDOG_MS));
        assert!(!woke_up(Some(tick)), "an ordinary tick");
        assert!(!woke_up(Some(tick * 3)), "a busy moment is not a sleep");
        assert!(woke_up(Some(RESUME_GAP + Duration::from_secs(1))));
        assert!(!woke_up(Some(RESUME_GAP)), "the gap itself is still fine");
        assert!(!woke_up(None), "a clock set back is no sleep");
    }

    #[test]
    fn tick_differences_survive_the_wrap_and_a_clock_hair() {
        assert_eq!(ago(100, 40), 60);
        assert_eq!(ago(5, u32::MAX - 4), 10, "the counter wrapped in between");
        assert_eq!(ago(100, 103), 0, "read a hair out of order");
    }

    #[test]
    fn key_messages() {
        assert_eq!(key_is_down(WM_KEYDOWN), Some(true));
        assert_eq!(key_is_down(WM_SYSKEYDOWN), Some(true));
        assert_eq!(key_is_down(WM_KEYUP), Some(false));
        assert_eq!(key_is_down(WM_SYSKEYUP), Some(false));
        assert_eq!(key_is_down(0x0102), None, "WM_CHAR");
    }
}
