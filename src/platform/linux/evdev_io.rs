//! The kernel half of the touchpad reader: `ioctl`, `poll` and opening an evdev node.
//!
//! The reader of the fingers (`tap::touch::evdev`) keeps only the pure parser. Everything that
//! needs the operating system lives here, so that the reader has no `cfg` of its own. This file is
//! built on every OS so the reader's tests run everywhere; where there is no Unix, `ioctl` and
//! `poll` always fail, as if the device were gone, and nothing but Linux ever calls them.

use std::ffi::{c_int, c_ulong, c_void};

#[cfg(unix)]
unsafe extern "C" {
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
}

/// # Safety
/// Never dereferences its arguments.
#[cfg(not(unix))]
unsafe fn ioctl(_fd: c_int, _request: c_ulong, _arg: *mut c_void) -> c_int {
    -1
}

/// # Safety
/// Never dereferences its arguments.
#[cfg(not(unix))]
unsafe fn poll(_fds: *mut PollFd, _nfds: c_ulong, _timeout: c_int) -> c_int {
    -1
}

/// `struct pollfd`
#[repr(C)]
pub(crate) struct PollFd {
    pub fd: c_int,
    pub events: i16,
    pub revents: i16,
}

pub(crate) const POLLIN: i16 = 1;
#[cfg_attr(not(unix), allow(dead_code))]
const O_NONBLOCK: i32 = 0o4000;

/// Waits up to `timeout_ms` for one of `fds`; the number of ready ones, `0` on timeout, negative
/// on error.
pub(crate) fn poll_fds(fds: &mut [PollFd], timeout_ms: c_int) -> c_int {
    // SAFETY: `fds` is a valid array of `fds.len()` pollfds.
    unsafe { poll(fds.as_mut_ptr(), fds.len() as c_ulong, timeout_ms) }
}

/// The raw file descriptor of an opened node (`-1` where there are none).
#[cfg(unix)]
pub(crate) fn raw_fd(file: &std::fs::File) -> c_int {
    use std::os::fd::AsRawFd;
    file.as_raw_fd()
}

#[cfg(not(unix))]
pub(crate) fn raw_fd(_file: &std::fs::File) -> c_int {
    -1
}

/// Opens an evdev node for reading, without blocking.
#[cfg(unix)]
pub(crate) fn open_node(path: &str) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
pub(crate) fn open_node(path: &str) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).open(path)
}

/// `struct input_absinfo`
#[repr(C)]
#[derive(Default)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

/// The range of an absolute axis, as the kernel reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AbsRange {
    pub min: i32,
    pub max: i32,
    /// Units per millimeter (`0` when unknown).
    pub resolution: i32,
}

/// `EVIOCGABS(abs)`: `_IOR('E', 0x40 + abs, struct input_absinfo)`. The numbers of this layout are
/// those of x86, ARM and RISC-V; other architectures lay the direction bits out differently and
/// are not supported (the touchpad is then not used).
fn eviocgabs(abs: u16) -> Option<c_ulong> {
    if cfg!(any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv64"
    )) {
        let size = std::mem::size_of::<AbsInfo>() as c_ulong;
        Some((2 << 30) | (size << 16) | (0x45 << 8) | (0x40 + c_ulong::from(abs)))
    } else {
        None
    }
}

/// The range of the axis `abs` (an `ABS_*` code) of the open evdev node `fd`.
pub(crate) fn read_abs(fd: c_int, abs: u16) -> Option<AbsRange> {
    let req = eviocgabs(abs)?;
    let mut info = AbsInfo::default();
    // SAFETY: `info` is the structure the request fills; `fd` is an open evdev node.
    let ok = unsafe { ioctl(fd, req, &mut info as *mut AbsInfo as *mut c_void) } == 0;
    ok.then_some(AbsRange {
        min: info.minimum,
        max: info.maximum,
        resolution: info.resolution,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ioctl_request_for_an_axis_is_the_kernels() {
        if cfg!(target_arch = "x86_64") {
            // EVIOCGABS(ABS_MT_POSITION_X = 0x35) as printed by the kernel headers on x86-64
            assert_eq!(eviocgabs(0x35), Some(0x8018_4575));
        }
    }
}
