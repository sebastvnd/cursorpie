//! Giving freed memory back to macOS.

use std::ffi::c_void;

unsafe extern "C" {
    /// The port of this process (`mach_task_self()` is a macro that reads it).
    static mach_task_self_: u32;
    /// `kern_return_t task_info(task_t, task_flavor_t, task_info_t, mach_msg_type_number_t *)`.
    fn task_info(task: u32, flavor: u32, out: *mut i32, count: *mut u32) -> i32;
    /// `size_t malloc_zone_pressure_relief(malloc_zone_t *zone, size_t goal)` from libmalloc.
    fn malloc_zone_pressure_relief(zone: *mut c_void, goal: usize) -> usize;
}

/// Asks the allocator to return free pages to the system and reports how many bytes it released.
///
/// Dropping a texture or a decoded image frees it for the *process*, but the allocator usually
/// keeps the pages, so the app still shows up as using the memory in Activity Monitor. Call this
/// after freeing something big (the UI does so when it goes to sleep, see
/// [`ui::sleep`](crate::ui::sleep)). It never touches memory that is in use.
pub fn release_unused_memory() -> usize {
    // SAFETY: a null zone means "every zone" and a goal of 0 means "as much as possible". No
    // pointer is read or written.
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) }
}

/// `TASK_VM_INFO`: the flavor of `task_info` that has the footprint.
const TASK_VM_INFO: u32 = 22;
/// Byte offsets in `task_vm_info_data_t` of the three numbers read (all `u64`).
const OFFSET_RESIDENT: usize = 16;
const OFFSET_RESIDENT_PEAK: usize = 24;
const OFFSET_PHYS_FOOTPRINT: usize = 144;

/// The memory this process holds, as macOS counts it: `phys_footprint` is what Activity Monitor
/// shows as "Memory". Read for the diagnostic log only. None if the kernel does not answer or
/// answers with a shorter record than expected (an older system).
pub fn memory_use() -> Option<crate::platform::MemoryUse> {
    // 64 words = 512 bytes, more than `task_vm_info_data_t`; the kernel fills what it has and
    // says how many 4-byte units that is
    let mut words = [0u64; 64];
    let mut count = (words.len() * 2) as u32;
    // SAFETY: `words` is `count` 4-byte units long and writable, and the task port is this process.
    let rc = unsafe {
        task_info(
            mach_task_self_,
            TASK_VM_INFO,
            words.as_mut_ptr().cast(),
            &mut count,
        )
    };
    if rc != 0 || (count as usize) * 4 < OFFSET_PHYS_FOOTPRINT + 8 {
        return None;
    }
    Some(crate::platform::MemoryUse {
        resident: words[OFFSET_RESIDENT / 8],
        peak: words[OFFSET_RESIDENT_PEAK / 8],
        footprint: words[OFFSET_PHYS_FOOTPRINT / 8],
    })
}
