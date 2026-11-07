//! A virtual input device made through `/dev/uinput`: what the Wayland layer writes the events it
//! keeps to (a clone of the mouse it grabbed), and what it sends shortcuts through (a virtual
//! keyboard). See `docs/WAYLAND_SUPPORT.md`, "Architecture".
//!
//! The file is built on every OS so its pure part is tested everywhere: the event codec, the
//! mouse button numbering, the choice of what a clone copies from the mouse ([`CloneSpec`], from
//! the capability bitmaps the kernel reports), the bytes of `UI_DEV_SETUP`, and the `ioctl`
//! numbers. Only [`UinputDevice`] touches the kernel, and where there is no Unix it always fails.

use super::evdev_io::{self, IOC_NONE, IOC_WRITE, InputId, ioc, ioctl_buf, ioctl_val};
use std::collections::BTreeSet;
use std::ffi::{c_int, c_ulong, c_void};
use std::io::{self, Write};

pub(crate) const EV_SYN: u16 = 0;
pub(crate) const EV_KEY: u16 = 1;
pub(crate) const EV_REL: u16 = 2;
pub(crate) const EV_ABS: u16 = 3;
pub(crate) const EV_MSC: u16 = 4;
pub(crate) const SYN_REPORT: u16 = 0;
pub(crate) const SYN_DROPPED: u16 = 3;
pub(crate) const REL_X: u16 = 0;
pub(crate) const REL_Y: u16 = 1;

pub(crate) const BTN_LEFT: u16 = 0x110;
pub(crate) const BTN_MIDDLE: u16 = 0x112;
/// The first extra button: `BTN_SIDE`, CursorPie's button 3 (back).
pub(crate) const BTN_SIDE: u16 = 0x113;
/// The last code of the mouse group of `input-event-codes.h`.
pub(crate) const BTN_LAST: u16 = 0x11f;

/// How many bytes of each capability bitmap are asked for: `EV_MAX` is 0x1f, `KEY_MAX` 0x2ff,
/// `REL_MAX` 0x0f and `MSC_MAX` 0x07.
pub(crate) const EV_BYTES: usize = 4;
pub(crate) const KEY_BYTES: usize = 0x300 / 8;
pub(crate) const REL_BYTES: usize = 2;
pub(crate) const MSC_BYTES: usize = 1;

// ------------------------------------------------------------------------------------- buttons

/// The CursorPie (macOS) number of a mouse button code: 0 left, 1 right, 2 middle, 3 back, 4 forward,
/// 5 and up the extra buttons, in the order the kernel numbers them (`BTN_SIDE`, `BTN_EXTRA`,
/// `BTN_FORWARD`, `BTN_BACK`, `BTN_TASK`...). The same numbers `inputdev` gives a mouse's buttons.
/// `None` for a code that is not a mouse button, or one past `mouse::MAX_BUTTON`.
pub(crate) fn button_for_code(code: u16) -> Option<i64> {
    match code {
        BTN_LEFT..=BTN_MIDDLE => Some(i64::from(code - BTN_LEFT)),
        BTN_SIDE..=BTN_LAST => Some(crate::mouse::FIRST_EXTRA + i64::from(code - BTN_SIDE)),
        _ => None,
    }
}

/// The button code of a CursorPie button number: the inverse of [`button_for_code`].
pub(crate) fn code_for_button(button: i64) -> Option<u16> {
    match button {
        0..=2 => Some(BTN_LEFT + button as u16),
        b if crate::mouse::is_extra(b) => Some(BTN_SIDE + (b - crate::mouse::FIRST_EXTRA) as u16),
        _ => None,
    }
}

// --------------------------------------------------------------------------------------- events

/// The time stamp of a `struct input_event`: a `timeval`, two longs. Written as zeros (the kernel
/// stamps events itself) and skipped when reading. A Linux `long` is as wide as a pointer; Rust's
/// `c_long` is not (it is 32 bits on 64-bit Windows), so the size comes from `usize`.
const STAMP: usize = 2 * std::mem::size_of::<usize>();
/// Size of one `struct input_event`.
pub(crate) const EVENT_SIZE: usize = STAMP + 8;

/// One event: the type, the code and the value of a `struct input_event`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InputEvent {
    pub ty: u16,
    pub code: u16,
    pub value: i32,
}

impl InputEvent {
    /// The end of a frame: events between two of these happened at the same moment.
    pub(crate) const SYN: InputEvent = InputEvent::new(EV_SYN, SYN_REPORT, 0);

    pub(crate) const fn new(ty: u16, code: u16, value: i32) -> InputEvent {
        InputEvent { ty, code, value }
    }

    /// A key or button press (`true`) or release (`false`).
    pub(crate) const fn key(code: u16, down: bool) -> InputEvent {
        InputEvent::new(EV_KEY, code, down as i32)
    }

    /// Appends the bytes of the event as the kernel's input and uinput interfaces take them.
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&[0u8; STAMP]);
        out.extend_from_slice(&self.ty.to_le_bytes());
        out.extend_from_slice(&self.code.to_le_bytes());
        out.extend_from_slice(&self.value.to_le_bytes());
    }

    /// The event in the first [`EVENT_SIZE`] bytes of `chunk`; `None` if there are fewer.
    pub(crate) fn decode(chunk: &[u8]) -> Option<InputEvent> {
        let body = chunk.get(STAMP..EVENT_SIZE)?;
        Some(InputEvent {
            ty: u16::from_le_bytes([body[0], body[1]]),
            code: u16::from_le_bytes([body[2], body[3]]),
            value: i32::from_le_bytes([body[4], body[5], body[6], body[7]]),
        })
    }
}

/// Every whole event in `bytes` (a trailing partial one is ignored).
pub(crate) fn decode_all(bytes: &[u8]) -> Vec<InputEvent> {
    bytes
        .as_chunks::<EVENT_SIZE>()
        .0
        .iter()
        .filter_map(|chunk| InputEvent::decode(chunk.as_slice()))
        .collect()
}

/// The numbers of the bits that are set in a kernel capability bitmap (bit `n` is bit `n % 8` of
/// byte `n / 8`, see `evdev_io::event_bits`), lowest first.
pub(crate) fn bits_set(map: &[u8]) -> Vec<u16> {
    map.iter()
        .enumerate()
        .flat_map(|(i, byte)| {
            (0..8u16)
                .filter(move |b| byte & (1 << b) != 0)
                .map(move |b| (i as u16) * 8 + b)
        })
        .collect()
}

// -------------------------------------------------------------------------------- what to clone

/// `UINPUT_MAX_NAME_SIZE`
const NAME_SIZE: usize = 80;
/// `sizeof(struct uinput_setup)`: a `struct input_id`, the name, and `ff_effects_max`.
const SETUP_SIZE: usize = 8 + NAME_SIZE + 4;

/// What a virtual device is made with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CloneSpec {
    pub name: String,
    pub id: InputId,
    pub keys: Vec<u16>,
    pub rels: Vec<u16>,
    pub msc: Vec<u16>,
}

impl CloneSpec {
    /// The spec of a clone of a mouse, from the capability bitmaps the kernel reports for it (types,
    /// keys, relative axes, misc). It has the mouse's name and ids, so settings a desktop keeps for
    /// that device (pointer speed, acceleration, natural scroll) can match the clone; and every
    /// button and every relative axis the mouse has, the hi-res and horizontal wheels included, or
    /// scrolling would double or stop.
    ///
    /// `Err` says why the device cannot be cloned faithfully, and is then left alone: it has no
    /// buttons or no relative axes (not a mouse), or it has absolute axes, which a clone made here
    /// would not carry.
    pub(crate) fn of_mouse(
        name: &str,
        id: InputId,
        types: &[u8],
        keys: &[u8],
        rels: &[u8],
        msc: &[u8],
    ) -> Result<CloneSpec, &'static str> {
        let has = |ev: u16| bits_set(types).contains(&ev);
        if !has(EV_KEY) || !has(EV_REL) {
            return Err("has no buttons or no relative axes");
        }
        if has(EV_ABS) {
            return Err("has absolute axes, which a clone would not carry");
        }
        Ok(CloneSpec {
            name: name.to_string(),
            id,
            keys: bits_set(keys),
            rels: bits_set(rels),
            msc: if has(EV_MSC) {
                bits_set(msc)
            } else {
                Vec::new()
            },
        })
    }

    /// The virtual keyboard shortcuts are sent through: every key code from `KEY_ESC` (1) to
    /// `KEY_MICMUTE` (248), the range the key table of `platform::keymap` draws from.
    pub(crate) fn keyboard() -> CloneSpec {
        CloneSpec {
            name: "CursorPie virtual keyboard".to_string(),
            // BUS_VIRTUAL
            id: InputId {
                bus: 0x06,
                vendor: 0,
                product: 0,
                version: 1,
            },
            keys: (1..=248).collect(),
            rels: Vec::new(),
            msc: Vec::new(),
        }
    }

    /// A small virtual mouse: relative X and Y and the three main buttons (libinput only treats a
    /// device as a pointer when it has a button). The Wayland touchpad layer moves the pointer
    /// through it while it holds the touchpad's node (see `tap::touch::evdev::PadPointer`).
    pub(crate) fn pointer() -> CloneSpec {
        CloneSpec {
            name: "CursorPie virtual pointer".to_string(),
            // BUS_VIRTUAL
            id: InputId {
                bus: 0x06,
                vendor: 0,
                product: 0,
                version: 1,
            },
            keys: vec![BTN_LEFT, BTN_LEFT + 1, BTN_MIDDLE],
            rels: vec![REL_X, REL_Y],
            msc: Vec::new(),
        }
    }

    /// The `struct uinput_setup` of `UI_DEV_SETUP`: the ids, the name (cut to fit, with its NUL) and
    /// `ff_effects_max` 0.
    pub(crate) fn setup_bytes(&self) -> [u8; SETUP_SIZE] {
        let mut out = [0u8; SETUP_SIZE];
        out[0..2].copy_from_slice(&self.id.bus.to_le_bytes());
        out[2..4].copy_from_slice(&self.id.vendor.to_le_bytes());
        out[4..6].copy_from_slice(&self.id.product.to_le_bytes());
        out[6..8].copy_from_slice(&self.id.version.to_le_bytes());
        // cut on a character boundary, leaving room for the NUL
        let mut end = self.name.len().min(NAME_SIZE - 1);
        while !self.name.is_char_boundary(end) {
            end -= 1;
        }
        out[8..8 + end].copy_from_slice(&self.name.as_bytes()[..end]);
        out
    }
}

// ----------------------------------------------------------------------------------------- ioctl

/// `UI_DEV_CREATE`: `_IO('U', 1)`.
fn ui_dev_create() -> Option<c_ulong> {
    ioc(IOC_NONE, b'U', 1, 0)
}

/// `UI_DEV_DESTROY`: `_IO('U', 2)`.
fn ui_dev_destroy() -> Option<c_ulong> {
    ioc(IOC_NONE, b'U', 2, 0)
}

/// `UI_DEV_SETUP`: `_IOW('U', 3, struct uinput_setup)`.
fn ui_dev_setup() -> Option<c_ulong> {
    ioc(IOC_WRITE, b'U', 3, SETUP_SIZE)
}

/// `UI_SET_EVBIT` (100), `UI_SET_KEYBIT` (101), `UI_SET_RELBIT` (102), `UI_SET_MSCBIT` (104):
/// `_IOW('U', nr, int)`.
fn ui_set_bit(nr: u8) -> Option<c_ulong> {
    ioc(IOC_WRITE, b'U', nr, std::mem::size_of::<c_int>())
}

const UI_SET_EVBIT: u8 = 100;
const UI_SET_KEYBIT: u8 = 101;
const UI_SET_RELBIT: u8 = 102;
const UI_SET_MSCBIT: u8 = 104;

// ---------------------------------------------------------------------------------------- device

/// A virtual device. Dropping it removes it from the system.
pub(crate) struct UinputDevice {
    file: std::fs::File,
    /// Keys and buttons that were written as pressed and not yet as released.
    down: BTreeSet<u16>,
}

#[cfg(unix)]
fn open_uinput() -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let open = |path: &str| {
        std::fs::OpenOptions::new()
            .write(true)
            // `O_NONBLOCK`: a write never waits for a stalled reader of the clone
            .custom_flags(0o4000)
            .open(path)
    };
    // `/dev/input/uinput` is where some older systems have it
    open("/dev/uinput").or_else(|e| open("/dev/input/uinput").map_err(|_| e))
}

#[cfg(not(unix))]
fn open_uinput() -> io::Result<std::fs::File> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

impl UinputDevice {
    /// Creates the device. The caller should wait a moment before it grabs the mouse the clone
    /// stands for: the desktop opens a new device a little after it appears, and what is written
    /// before that is lost.
    pub(crate) fn create(spec: &CloneSpec) -> io::Result<UinputDevice> {
        let file = open_uinput()?;
        let fd = evdev_io::raw_fd(&file);
        let fail = |what: &str| io::Error::other(format!("uinput: {what} failed"));
        let set = |nr: u8, value: u16| {
            ui_set_bit(nr).is_some_and(|req| ioctl_val(fd, req, c_ulong::from(value)))
        };
        let mut types = vec![EV_KEY];
        if !spec.rels.is_empty() {
            types.push(EV_REL);
        }
        if !spec.msc.is_empty() {
            types.push(EV_MSC);
        }
        for ty in types {
            if !set(UI_SET_EVBIT, ty) {
                return Err(fail("UI_SET_EVBIT"));
            }
        }
        for (list, nr, what) in [
            (&spec.keys, UI_SET_KEYBIT, "UI_SET_KEYBIT"),
            (&spec.rels, UI_SET_RELBIT, "UI_SET_RELBIT"),
            (&spec.msc, UI_SET_MSCBIT, "UI_SET_MSCBIT"),
        ] {
            for code in list {
                if !set(nr, *code) {
                    return Err(fail(what));
                }
            }
        }
        let mut setup = spec.setup_bytes();
        let ok =
            ui_dev_setup().is_some_and(|req| ioctl_buf(fd, req, setup.as_mut_ptr() as *mut c_void));
        if !ok {
            return Err(fail("UI_DEV_SETUP"));
        }
        let created =
            ui_dev_create().is_some_and(|req| ioctl_buf(fd, req, std::ptr::null_mut::<c_void>()));
        if !created {
            return Err(fail("UI_DEV_CREATE"));
        }
        Ok(UinputDevice {
            file,
            down: BTreeSet::new(),
        })
    }

    /// Writes `events`, which should end with [`InputEvent::SYN`] (a frame).
    pub(crate) fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        let mut bytes = Vec::with_capacity(events.len() * EVENT_SIZE);
        for e in events {
            e.encode(&mut bytes);
            if e.ty == EV_KEY {
                match e.value {
                    0 => {
                        self.down.remove(&e.code);
                    }
                    1 => {
                        self.down.insert(e.code);
                    }
                    _ => {}
                }
            }
        }
        self.file.write_all(&bytes)
    }

    /// Releases every key and button that is down, so none stays stuck in the system.
    pub(crate) fn release_all(&mut self) -> io::Result<()> {
        if self.down.is_empty() {
            return Ok(());
        }
        let mut frame: Vec<InputEvent> = self
            .down
            .iter()
            .map(|code| InputEvent::key(*code, false))
            .collect();
        frame.push(InputEvent::SYN);
        self.emit(&frame)
    }

    /// Whether anything is down.
    #[cfg(test)]
    pub(crate) fn any_down(&self) -> bool {
        !self.down.is_empty()
    }
}

impl Drop for UinputDevice {
    fn drop(&mut self) {
        let _ = self.release_all();
        if let Some(req) = ui_dev_destroy() {
            ioctl_buf(
                evdev_io::raw_fd(&self.file),
                req,
                std::ptr::null_mut::<c_void>(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> InputId {
        InputId {
            bus: 3,
            vendor: 0x046d,
            product: 0xc08b,
            version: 0x0111,
        }
    }

    /// The bitmap bytes of the given set bits.
    fn map(bytes: usize, bits: &[u16]) -> Vec<u8> {
        let mut m = vec![0u8; bytes];
        for b in bits {
            m[usize::from(*b) / 8] |= 1 << (b % 8);
        }
        m
    }

    #[test]
    fn button_codes_use_the_numbers_of_the_other_platforms() {
        assert_eq!(button_for_code(0x110), Some(0), "left");
        assert_eq!(button_for_code(0x111), Some(1), "right");
        assert_eq!(button_for_code(0x112), Some(2), "middle");
        assert_eq!(button_for_code(0x113), Some(3), "BTN_SIDE is back");
        assert_eq!(button_for_code(0x114), Some(4), "BTN_EXTRA is forward");
        assert_eq!(
            button_for_code(0x115),
            Some(5),
            "BTN_FORWARD is the first extra"
        );
        assert_eq!(button_for_code(0x11f), Some(15), "the last one");
        assert_eq!(button_for_code(0x120), None);
        assert_eq!(button_for_code(30), None, "KEY_A");
    }

    #[test]
    fn the_two_button_tables_are_inverse() {
        for b in 0..=crate::mouse::MAX_BUTTON {
            let code = code_for_button(b).unwrap();
            assert_eq!(button_for_code(code), Some(b), "button {b}");
        }
        assert_eq!(code_for_button(16), None);
        assert_eq!(code_for_button(-1), None);
    }

    #[test]
    fn they_agree_with_the_x_button_numbering_of_the_x11_layer() {
        // X button 8 is BTN_SIDE, 9 BTN_EXTRA, 10 BTN_FORWARD...: the CursorPie number is the X
        // number minus 5 from the second side button on (see `backend::x_button_for`)
        for code in BTN_SIDE..=BTN_LAST {
            let x = i64::from(code - BTN_SIDE) + 8;
            assert_eq!(button_for_code(code), Some(x - 5));
        }
    }

    #[test]
    fn an_event_survives_the_codec() {
        let events = [
            InputEvent::new(EV_REL, REL_X, -3),
            InputEvent::key(0x112, true),
            InputEvent::SYN,
        ];
        let mut bytes = Vec::new();
        for e in &events {
            e.encode(&mut bytes);
        }
        assert_eq!(bytes.len(), 3 * EVENT_SIZE);
        assert_eq!(decode_all(&bytes), events);
        // a trailing partial event is left out
        assert_eq!(decode_all(&bytes[..2 * EVENT_SIZE + 5]), events[..2]);
        assert_eq!(InputEvent::decode(&bytes[..EVENT_SIZE - 1]), None);
    }

    #[test]
    fn the_size_of_an_event_is_the_kernels() {
        assert_eq!(
            EVENT_SIZE,
            if cfg!(target_pointer_width = "64") {
                24
            } else {
                16
            }
        );
    }

    #[test]
    fn set_bits_are_listed_lowest_first() {
        assert_eq!(bits_set(&[0b0000_0101, 0b1000_0000]), [0, 2, 15]);
        assert!(bits_set(&[]).is_empty());
        assert_eq!(
            bits_set(&map(KEY_BYTES, &[0x110, 0x112, 0x2ff])),
            [0x110, 0x112, 0x2ff]
        );
    }

    #[test]
    fn a_clone_copies_every_button_and_axis_of_the_mouse() {
        // a mouse with the hi-res and horizontal wheels: REL_X, REL_Y, REL_HWHEEL (6), REL_WHEEL (8),
        // REL_WHEEL_HI_RES (11), REL_HWHEEL_HI_RES (12)
        let types = map(EV_BYTES, &[EV_SYN, EV_KEY, EV_REL, EV_MSC]);
        let keys = map(KEY_BYTES, &[0x110, 0x111, 0x112, 0x113, 0x114]);
        let rels = map(REL_BYTES, &[0, 1, 6, 8, 11, 12]);
        let msc = map(MSC_BYTES, &[4]);
        let spec = CloneSpec::of_mouse("Logitech G502", id(), &types, &keys, &rels, &msc).unwrap();
        assert_eq!(spec.name, "Logitech G502");
        assert_eq!(spec.id, id());
        assert_eq!(spec.keys, [0x110, 0x111, 0x112, 0x113, 0x114]);
        assert_eq!(spec.rels, [0, 1, 6, 8, 11, 12]);
        assert_eq!(spec.msc, [4]);
    }

    #[test]
    fn misc_events_are_copied_only_if_the_mouse_has_them() {
        let types = map(EV_BYTES, &[EV_SYN, EV_KEY, EV_REL]);
        let keys = map(KEY_BYTES, &[0x110]);
        let rels = map(REL_BYTES, &[0, 1]);
        // the msc bitmap says something, the type bitmap says the device has no such events
        let spec =
            CloneSpec::of_mouse("m", id(), &types, &keys, &rels, &map(MSC_BYTES, &[4])).unwrap();
        assert!(spec.msc.is_empty());
    }

    #[test]
    fn a_device_that_cannot_be_cloned_faithfully_is_refused() {
        let keys = map(KEY_BYTES, &[0x110]);
        let rels = map(REL_BYTES, &[0, 1]);
        let tablet = map(EV_BYTES, &[EV_SYN, EV_KEY, EV_REL, EV_ABS]);
        let err = CloneSpec::of_mouse("t", id(), &tablet, &keys, &rels, &[0]).unwrap_err();
        assert!(err.contains("absolute"), "{err}");
        let keyboard = map(EV_BYTES, &[EV_SYN, EV_KEY]);
        assert!(CloneSpec::of_mouse("k", id(), &keyboard, &keys, &[0, 0], &[0]).is_err());
    }

    #[test]
    fn the_setup_bytes_follow_struct_uinput_setup() {
        let spec = CloneSpec {
            name: "Mouse".into(),
            id: id(),
            keys: vec![],
            rels: vec![],
            msc: vec![],
        };
        let b = spec.setup_bytes();
        assert_eq!(b.len(), 92);
        assert_eq!(&b[0..2], &3u16.to_le_bytes(), "bustype");
        assert_eq!(&b[2..4], &0x046du16.to_le_bytes(), "vendor");
        assert_eq!(&b[4..6], &0xc08bu16.to_le_bytes(), "product");
        assert_eq!(&b[6..8], &0x0111u16.to_le_bytes(), "version");
        assert_eq!(&b[8..13], b"Mouse");
        assert_eq!(b[13], 0, "NUL terminated");
        assert_eq!(&b[88..92], &[0, 0, 0, 0], "ff_effects_max");
    }

    #[test]
    fn a_long_name_is_cut_on_a_character_boundary_with_room_for_the_nul() {
        let spec = CloneSpec {
            name: "é".repeat(60), // 120 bytes
            id: id(),
            keys: vec![],
            rels: vec![],
            msc: vec![],
        };
        let b = spec.setup_bytes();
        // 79 bytes would split a two-byte character: 78 are kept
        assert_eq!(b[8 + 78], 0);
        assert_eq!(&b[8..8 + 78], "é".repeat(39).as_bytes());
        assert_eq!(b[8 + 79], 0, "the last byte of the name field stays NUL");
    }

    #[test]
    fn the_virtual_keyboard_has_every_key_the_table_can_name() {
        let k = CloneSpec::keyboard();
        assert!(k.rels.is_empty());
        for code in [29u16, 42, 56, 125, 30, 57] {
            assert!(k.keys.contains(&code), "key {code}");
        }
        assert!(
            k.name.to_ascii_lowercase().contains("virtual"),
            "listed as a virtual device"
        );
    }

    #[test]
    fn the_uinput_requests_are_the_kernels() {
        if !cfg!(target_arch = "x86_64") {
            return;
        }
        assert_eq!(ui_dev_create(), Some(0x5501));
        assert_eq!(ui_dev_destroy(), Some(0x5502));
        assert_eq!(ui_dev_setup(), Some(0x405c_5503));
        assert_eq!(ui_set_bit(UI_SET_EVBIT), Some(0x4004_5564));
        assert_eq!(ui_set_bit(UI_SET_KEYBIT), Some(0x4004_5565));
        assert_eq!(ui_set_bit(UI_SET_RELBIT), Some(0x4004_5566));
        assert_eq!(ui_set_bit(UI_SET_MSCBIT), Some(0x4004_5568));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn a_device_can_be_made_where_uinput_is_writable() {
        // skipped where the device is missing or not writable (most CI machines)
        let Ok(mut dev) = UinputDevice::create(&CloneSpec::keyboard()) else {
            return;
        };
        dev.emit(&[InputEvent::key(30, true), InputEvent::SYN])
            .unwrap();
        assert!(dev.any_down());
        dev.release_all().unwrap();
        assert!(!dev.any_down());
    }
}
