//! The kernel half of the touchpad reader and of the Wayland mouse layer: `ioctl`, `poll` and
//! opening an evdev node (and, for the Wayland layer, grabbing one and describing it).
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

// ------------------------------------------------------------------------------ Wayland layer
//
// What `tap::wayland` and `uinput` need on top of the above: grab a mouse (`EVIOCGRAB`), describe it
// (name, ids, capability bitmaps) so a clone can be made, and the generic `ioctl` calls the clone
// itself needs. All of them go through the one `ioctl` declared above, with a pointer-sized last
// argument, so the non-Unix stub keeps compiling.

/// `_IOC_NONE`, `_IOC_WRITE`, `_IOC_READ` of the layout x86, ARM and RISC-V use.
pub(crate) const IOC_NONE: c_ulong = 0;
pub(crate) const IOC_WRITE: c_ulong = 1;
pub(crate) const IOC_READ: c_ulong = 2;

/// Whether the `_IOC` layout used below is the one of this architecture (see [`eviocgabs`]).
pub(crate) const fn ioc_layout_known() -> bool {
    cfg!(any(
        target_arch = "x86",
        target_arch = "x86_64",
        target_arch = "arm",
        target_arch = "aarch64",
        target_arch = "riscv64"
    ))
}

/// `_IOC(dir, ty, nr, size)` of the layout above. `None` on an architecture that lays the direction
/// bits out differently (MIPS, PowerPC, SPARC): there the Wayland layer is not used.
pub(crate) fn ioc(dir: c_ulong, ty: u8, nr: u8, size: usize) -> Option<c_ulong> {
    ioc_layout_known().then(|| {
        (dir << 30) | ((size as c_ulong) << 16) | (c_ulong::from(ty) << 8) | c_ulong::from(nr)
    })
}

/// `struct input_id`: the ids the kernel (and a clone) report for a device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub(crate) struct InputId {
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
    pub version: u16,
}

/// `EVIOCGRAB`: `_IOW('E', 0x90, int)`.
fn eviocgrab() -> Option<c_ulong> {
    ioc(IOC_WRITE, b'E', 0x90, std::mem::size_of::<c_int>())
}

/// `EVIOCGID`: `_IOR('E', 0x02, struct input_id)`.
fn eviocgid() -> Option<c_ulong> {
    ioc(IOC_READ, b'E', 0x02, std::mem::size_of::<InputId>())
}

/// `EVIOCGNAME(len)`: `_IOC(_IOC_READ, 'E', 0x06, len)`.
fn eviocgname(len: usize) -> Option<c_ulong> {
    ioc(IOC_READ, b'E', 0x06, len)
}

/// `EVIOCGBIT(ev, len)`: `_IOC(_IOC_READ, 'E', 0x20 + ev, len)`. `ev` 0 asks for the event types the
/// device has; an `EV_*` type asks for the codes of that type.
fn eviocgbit(ev: u8, len: usize) -> Option<c_ulong> {
    ioc(IOC_READ, b'E', 0x20u8.checked_add(ev)?, len)
}

/// Calls `ioctl(fd, req, arg)` where `arg` is a pointer to a buffer the request fills or reads.
pub(crate) fn ioctl_buf(fd: c_int, req: c_ulong, arg: *mut c_void) -> bool {
    // SAFETY: the callers pass a buffer that is as large as the request says (the size is part of
    // `req`), or null for a request that takes none; a bad `fd` only makes the call fail.
    unsafe { ioctl(fd, req, arg) >= 0 }
}

/// Calls `ioctl(fd, req, value)` where the argument is a plain number (`EVIOCGRAB`, `UI_SET_*BIT`).
pub(crate) fn ioctl_val(fd: c_int, req: c_ulong, value: c_ulong) -> bool {
    ioctl_buf(fd, req, value as usize as *mut c_void)
}

/// Grabs (`true`) or lets go of (`false`) the evdev node `fd`: while it is grabbed, no other reader
/// (the compositor) gets its events. The kernel also lets go when the descriptor is closed, which
/// is what keeps a crashed CursorPie from leaving a dead mouse behind.
pub(crate) fn grab(fd: c_int, on: bool) -> bool {
    eviocgrab().is_some_and(|req| ioctl_val(fd, req, c_ulong::from(on)))
}

/// The ids of an open evdev node.
pub(crate) fn device_id(fd: c_int) -> Option<InputId> {
    let mut id = InputId::default();
    let req = eviocgid()?;
    ioctl_buf(fd, req, &mut id as *mut InputId as *mut c_void).then_some(id)
}

/// The name of an open evdev node.
pub(crate) fn device_name(fd: c_int) -> Option<String> {
    let mut buf = [0u8; 256];
    let req = eviocgname(buf.len())?;
    if !ioctl_buf(fd, req, buf.as_mut_ptr() as *mut c_void) {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// The capability bitmap of an open evdev node for the event type `ev` (`0` = the types themselves),
/// `bytes` long. Bit `n` is bit `n % 8` of byte `n / 8` (the kernel's bitmaps of longs, read on a
/// little-endian machine: every architecture `ioc` accepts is one).
pub(crate) fn event_bits(fd: c_int, ev: u8, bytes: usize) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; bytes];
    let req = eviocgbit(ev, bytes)?;
    ioctl_buf(fd, req, buf.as_mut_ptr() as *mut c_void).then_some(buf)
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

    #[test]
    fn the_wayland_requests_are_the_kernels() {
        if !cfg!(target_arch = "x86_64") {
            return;
        }
        // as printed by the kernel headers on x86-64
        assert_eq!(eviocgrab(), Some(0x4004_4590), "EVIOCGRAB");
        assert_eq!(eviocgid(), Some(0x8008_4502), "EVIOCGID");
        assert_eq!(eviocgname(256), Some(0x8100_4506), "EVIOCGNAME(256)");
        assert_eq!(eviocgbit(0, 4), Some(0x8004_4520), "EVIOCGBIT(0, 4)");
        assert_eq!(eviocgbit(1, 96), Some(0x8060_4521), "EVIOCGBIT(EV_KEY, 96)");
    }

    #[test]
    fn an_event_type_that_does_not_fit_the_request_number_has_no_request() {
        assert_eq!(eviocgbit(0xff, 4), None);
    }
}
