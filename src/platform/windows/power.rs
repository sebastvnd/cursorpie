//! Staying quick to respond while hidden, and giving freed memory back without a cold start.
//!
//! CursorPie sits hidden for hours, and its whole job is to answer the first press after that.
//! Two things of Windows' own make that first press slow, and this module keeps both away:
//!
//! * **Power throttling** (EcoQoS, "Efficiency mode"). Windows 11 moves a process that shows no
//!   window to the slow cores and stops it from raising the timer resolution, which is exactly what
//!   a hidden tray program looks like. [`stay_responsive`] opts the process out of both.
//! * **Trimming the working set.** `SetProcessWorkingSetSize(-1, -1)` throws *every* page of the
//!   process out of RAM, including the code of the hook callbacks, the toolkit and the OpenGL
//!   driver. The next press has to fault all of it back in (from the pagefile if the system was
//!   busy), and a low-level hook that answers too slowly is dropped by Windows.
//!   [`release_unused_memory`] therefore compacts the heap instead, which gives back what was freed and touches nothing
//!   that is in use. When the whole machine is short of memory, Windows trims on its own.

use std::ffi::c_void;
use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapCompact};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, SetProcessInformation, SetThreadPriority,
};

/// `PROCESS_INFORMATION_CLASS::ProcessPowerThrottling`.
const PROCESS_POWER_THROTTLING: i32 = 4;
/// `PROCESS_POWER_THROTTLING_CURRENT_VERSION`.
const THROTTLING_VERSION: u32 = 1;
/// `PROCESS_POWER_THROTTLING_EXECUTION_SPEED`: the EcoQoS switch.
const THROTTLE_EXECUTION_SPEED: u32 = 0x1;
/// `PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION`: a hidden process may not raise the timer rate.
const THROTTLE_IGNORE_TIMER_RESOLUTION: u32 = 0x4;
/// `THREAD_PRIORITY_ABOVE_NORMAL`.
const ABOVE_NORMAL: i32 = 1;

/// `PROCESS_POWER_THROTTLING_STATE`.
#[repr(C)]
struct PowerThrottlingState {
    version: u32,
    control_mask: u32,
    state_mask: u32,
}

/// The state that switches both kinds of throttling off: the bits of `control_mask` are the ones
/// this process decides about, and a clear bit in `state_mask` means "never throttle".
fn unthrottled() -> PowerThrottlingState {
    PowerThrottlingState {
        version: THROTTLING_VERSION,
        control_mask: THROTTLE_EXECUTION_SPEED | THROTTLE_IGNORE_TIMER_RESOLUTION,
        state_mask: 0,
    }
}

/// Tells Windows not to throttle this process (see the module docs). Windows before 10 1709 does
/// not know the setting and refuses it; nothing is lost then, so the answer is only logged.
pub fn stay_responsive() {
    let state = unthrottled();
    // SAFETY: the pseudo handle of the current process is always valid, and `state` is a live
    // `PROCESS_POWER_THROTTLING_STATE` of the size that is passed.
    let ok = unsafe {
        SetProcessInformation(
            GetCurrentProcess(),
            PROCESS_POWER_THROTTLING,
            std::ptr::from_ref(&state).cast::<c_void>(),
            std::mem::size_of::<PowerThrottlingState>() as u32,
        )
    };
    diag::log!("sys", "power_throttling", off = ok != 0);
}

/// Runs the calling thread a little above normal priority, so a hook callback is not left waiting
/// behind a busy program for longer than Windows allows (about a second; after that the hook is
/// dropped). Used by the thread of the input hooks.
pub fn raise_current_thread() {
    // SAFETY: the pseudo handle of the current thread is always valid.
    unsafe {
        SetThreadPriority(GetCurrentThread(), ABOVE_NORMAL);
    }
}

/// Compacts the process heap, which returns freed blocks to the system. Memory that is in use stays
/// where it is, so the next press finds its code and data in RAM. Reports 0 because Windows does
/// not say how much was released.
pub fn release_unused_memory() -> usize {
    // SAFETY: the process heap handle is always valid, and flags of 0 mean a plain compaction.
    unsafe {
        HeapCompact(GetProcessHeap(), 0);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_kinds_of_throttling_are_decided_and_both_are_off() {
        let s = unthrottled();
        assert_eq!(s.version, 1);
        assert_eq!(s.control_mask, 0x5, "execution speed and timer resolution");
        assert_eq!(s.state_mask, 0, "a clear state bit = never throttled");
    }

    #[test]
    fn the_state_has_the_size_windows_expects() {
        assert_eq!(std::mem::size_of::<PowerThrottlingState>(), 12);
    }
}
