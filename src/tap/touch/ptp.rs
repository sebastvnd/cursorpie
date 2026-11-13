//! The pure half of the Windows touch source: what the reports of a Precision Touchpad mean.
//!
//! A Windows Precision Touchpad (PTP) is a HID device on the *Digitizer / Touch Pad* usage page
//! (`0x0D` / `0x05`). Raw Input hands a background program its reports; this module decides what
//! they say, with **no Windows type in it**, so every line here runs in the unit tests of every OS.
//! The Win32 calls (the window that receives `WM_INPUT`, the `HidP_*` parser) are in
//! [`precision`](super::precision).
//!
//! ```text
//! WM_INPUT packet --parse_raw_hid--> device + reports
//! preparsed data  --layout_from_caps--> Spec { report id, one link collection per finger, size }
//! report          --(HidP_*)--> count + RawContact per finger slot
//! Assembler       --> one complete frame (hybrid mode: several reports are one frame)
//! finger()        --> recognizer::Finger  (0..1 position, palms marked large)
//! ```
//!
//! What a PTP report holds (Microsoft's *Precision Touchpad* requirements, as far as they matter):
//!
//! - one **link collection per finger slot** (usually five), each with *X*, *Y*, *Contact ID*, the
//!   one-bit *Tip Switch* and *Confidence*, and in the top-level collection the *Contact Count*;
//! - **hybrid mode**: when more fingers touch than a report has slots, the first report carries the
//!   total in *Contact Count* and the next ones carry `0`, until all contacts were sent;
//! - a lifted finger is reported once more with *Tip Switch* off;
//! - **palm rejection is done by the device**: *Confidence* off means "not an intentional finger".
//!   The width and height of a contact are optional in the standard and their units are not
//!   guaranteed, so they are **not** used here.
//!
//! Everything about the real layouts of devices is a **design assumption until spikes W1 to W4 of
//! `docs/developer/TRACKPAD_HAPTICS_PLAN.md` are answered on a Windows laptop**.

use super::recognizer::{Finger, PALM_SIZE};

/// Raw Input: the packet of a HID device.
pub const RIM_TYPEHID: u32 = 2;
/// `WM_INPUT`: a Raw Input packet arrived.
pub const WM_INPUT: u32 = 0x00FF;
/// `WM_INPUT_DEVICE_CHANGE`: a Raw Input device was added or removed.
pub const WM_INPUT_DEVICE_CHANGE: u32 = 0x00FE;

/// Usage page *Generic Desktop* (X and Y).
pub const PAGE_GENERIC: u16 = 0x01;
/// Usage page *Digitizer*.
pub const PAGE_DIGITIZER: u16 = 0x0D;
/// Usage *Touch Pad* of the Digitizer page: the top-level collection of a precision touchpad.
pub const USAGE_TOUCH_PAD: u16 = 0x05;
/// Usage *X* of the Generic Desktop page.
pub const USAGE_X: u16 = 0x30;
/// Usage *Y* of the Generic Desktop page.
pub const USAGE_Y: u16 = 0x31;
/// Usage *Tip Switch*: the finger touches.
pub const USAGE_TIP: u16 = 0x42;
/// Usage *Confidence*: the contact is an intentional finger, not a palm.
pub const USAGE_CONFIDENCE: u16 = 0x47;
/// Usage *Contact ID*: stays the same while the finger stays down.
pub const USAGE_CONTACT_ID: u16 = 0x51;
/// Usage *Contact Count*: how many contacts the frame has in all.
pub const USAGE_CONTACT_COUNT: u16 = 0x54;

/// Most finger slots read from one report.
pub const MAX_SLOTS: usize = 10;

/// One value of the report descriptor (what `HidP_GetValueCaps` says), reduced to what is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cap {
    pub page: u16,
    pub usage: u16,
    /// The link collection (the finger slot) the value belongs to; `0` is the top level.
    pub link: u16,
    pub report_id: u8,
    pub logical_min: i32,
    pub logical_max: i32,
    pub physical_min: i32,
    pub physical_max: i32,
    /// The HID unit word (`0x11` is centimeters, `0x13` inches).
    pub units: u32,
    /// The HID unit exponent, a four-bit two's complement number in the low nibble.
    pub units_exp: u32,
}

/// The range of the coordinates of a device, in report units, and its real size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub x: (i32, i32),
    pub y: (i32, i32),
    /// Width and height in millimeters, or `(0.0, 0.0)` when the device does not say (the
    /// recognizer then assumes a typical trackpad).
    pub size_mm: (f32, f32),
}

/// Where the values of a device are: derived once per device from its descriptor.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    /// The report that carries the contacts (a precision touchpad numbers its reports).
    pub report_id: u8,
    /// The link collection of each finger slot, in report order.
    pub slots: Vec<u16>,
    /// The link collection that holds *Contact Count*.
    pub count_link: u16,
    pub surface: Surface,
}

/// The extent of an axis in millimeters, from its physical range, unit and exponent.
/// `None` when the device does not say it in centimeters or inches, or says something absurd.
pub fn extent_mm(physical_min: i32, physical_max: i32, units: u32, units_exp: u32) -> Option<f32> {
    let span = f64::from(physical_max) - f64::from(physical_min);
    if span <= 0.0 {
        return None;
    }
    let nibble = (units_exp & 0xF) as i32;
    let exp = if nibble >= 8 { nibble - 16 } else { nibble };
    let per_unit_mm = match units & 0xFF {
        0x11 => 10.0, // SI linear, length: centimeters
        0x13 => 25.4, // English linear, length: inches
        _ => return None,
    };
    let mm = span * 10f64.powi(exp) * per_unit_mm;
    (10.0..=400.0).contains(&mm).then_some(mm as f32)
}

/// Finds where the values are. `None` when the descriptor is not that of a precision touchpad
/// (no X and Y, no contact count, no numbered report, an empty range): the device is then ignored.
pub fn layout_from_caps(caps: &[Cap]) -> Option<Spec> {
    let find = |page: u16, usage: u16| {
        caps.iter()
            .filter(move |c| c.page == page && c.usage == usage)
    };
    let x0 = *find(PAGE_GENERIC, USAGE_X).next()?;
    let report_id = x0.report_id;
    if report_id == 0 {
        return None;
    }
    let y0 = *find(PAGE_GENERIC, USAGE_Y).find(|c| c.report_id == report_id)?;
    let count = *find(PAGE_DIGITIZER, USAGE_CONTACT_COUNT).find(|c| c.report_id == report_id)?;
    if x0.logical_max <= x0.logical_min || y0.logical_max <= y0.logical_min {
        return None;
    }
    let mut slots: Vec<u16> = Vec::new();
    for c in find(PAGE_GENERIC, USAGE_X).filter(|c| c.report_id == report_id) {
        if !slots.contains(&c.link) {
            slots.push(c.link);
        }
    }
    slots.truncate(MAX_SLOTS);
    let w = extent_mm(x0.physical_min, x0.physical_max, x0.units, x0.units_exp);
    let h = extent_mm(y0.physical_min, y0.physical_max, y0.units, y0.units_exp);
    let size_mm = match (w, h) {
        (Some(w), Some(h)) => (w, h),
        _ => (0.0, 0.0),
    };
    Some(Spec {
        report_id,
        slots,
        count_link: count.link,
        surface: Surface {
            x: (x0.logical_min, x0.logical_max),
            y: (y0.logical_min, y0.logical_max),
            size_mm,
        },
    })
}

/// One finger slot of one report, in report units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawContact {
    pub id: i32,
    /// *Tip Switch*: the finger touches.
    pub tip: bool,
    /// *Confidence*: an intentional finger (off: a palm).
    pub confidence: bool,
    pub x: i32,
    pub y: i32,
}

/// What the recognizer is told about a contact: a finger that is lifted is nothing; a contact the
/// device does not trust is as large as a palm, so the recognizer leaves it out.
pub fn finger(c: &RawContact, s: &Surface) -> Option<Finger> {
    if !c.tip {
        return None;
    }
    let span = |(lo, hi): (i32, i32)| (i64::from(hi) - i64::from(lo)) as f32;
    let (sx, sy) = (span(s.x), span(s.y));
    if sx <= 0.0 || sy <= 0.0 {
        return None;
    }
    let norm =
        |v: i32, lo: i32, len: f32| ((i64::from(v) - i64::from(lo)) as f32 / len).clamp(0.0, 1.0);
    Some(Finger {
        id: c.id,
        x: norm(c.x, s.x.0, sx),
        y: norm(c.y, s.y.0, sy),
        size: if c.confidence { 1.0 } else { PALM_SIZE * 2.0 },
    })
}

/// Puts the reports of one frame together. In hybrid mode a frame can be longer than one report:
/// the first report says how many contacts there are in all, the next ones say `0` and carry the
/// rest. One assembler per device.
#[derive(Debug, Default)]
pub struct Assembler {
    expected: usize,
    pending: Vec<RawContact>,
}

impl Assembler {
    /// How many finger slots of the report that is being read are worth reading, given the
    /// *Contact Count* it carries and how many slots a report has.
    pub fn wanted(&self, count: usize, slots: usize) -> usize {
        let left = if count > 0 {
            count
        } else {
            self.expected.saturating_sub(self.pending.len())
        };
        left.min(slots)
    }

    /// Adds one report (its count and the contacts [`wanted`](Self::wanted) asked for). Returns
    /// `true` when a frame is complete; it is then in `out`. A frame with no contacts at all (every
    /// finger lifted and reported) is complete at once and `out` is empty.
    pub fn push(
        &mut self,
        count: usize,
        contacts: &[RawContact],
        out: &mut Vec<RawContact>,
    ) -> bool {
        if count > 0 {
            // a new frame starts; one that never finished is dropped
            self.pending.clear();
            self.expected = count;
        }
        let room = self.expected.saturating_sub(self.pending.len());
        self.pending.extend(contacts.iter().take(room).copied());
        if self.pending.len() >= self.expected {
            out.clear();
            out.append(&mut self.pending);
            self.expected = 0;
            true
        } else {
            false
        }
    }
}

/// The HID packet of a `WM_INPUT` message (`RAWINPUT` with `dwType` = `RIM_TYPEHID`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawHid<'a> {
    /// The handle of the device (the key of everything kept per device).
    pub device: u64,
    /// Bytes of one report.
    pub size: usize,
    /// The reports one after the other (`size` bytes each).
    pub data: &'a [u8],
}

impl<'a> RawHid<'a> {
    /// The reports of the packet, each starting with its report id.
    pub fn reports(&self) -> std::slice::ChunksExact<'a, u8> {
        self.data.chunks_exact(self.size)
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let bytes: [u8; 4] = b.get(at..at + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    let bytes: [u8; 8] = b.get(at..at + 8)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

/// Reads a `RAWINPUT` packet: a `RAWINPUTHEADER` of `header_len` bytes (`dwType`, `dwSize`, the
/// device handle and `wParam`, so 16 bytes on a 32-bit build and 24 on a 64-bit one), then the
/// `RAWHID` (`dwSizeHid`, `dwCount`, the bytes). `None` for anything else, or a packet that is cut off.
pub fn parse_raw_hid(buf: &[u8], header_len: usize) -> Option<RawHid<'_>> {
    if u32_at(buf, 0)? != RIM_TYPEHID {
        return None;
    }
    let pointer = header_len.checked_sub(8)? / 2;
    let device = match pointer {
        4 => u64::from(u32_at(buf, 8)?),
        8 => u64_at(buf, 8)?,
        _ => return None,
    };
    let size = u32_at(buf, header_len)? as usize;
    let count = u32_at(buf, header_len + 4)? as usize;
    let total = size.checked_mul(count)?;
    if size == 0 || count == 0 {
        return None;
    }
    let data = buf.get(header_len + 8..)?.get(..total)?;
    Some(RawHid { device, size, data })
}

/// Whether the 32 bytes `GetRawInputDeviceInfo` wrote (`RID_DEVICE_INFO`, read as eight `u32`: size,
/// type, then for a HID device vendor, product, version and the usage page and usage packed in one
/// word) describe a precision touchpad.
pub fn is_touchpad_info(info: &[u32; 8]) -> bool {
    info[1] == RIM_TYPEHID
        && (info[5] & 0xFFFF) == u32::from(PAGE_DIGITIZER)
        && (info[5] >> 16) == u32::from(USAGE_TOUCH_PAD)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(page: u16, usage: u16, link: u16) -> Cap {
        Cap {
            page,
            usage,
            link,
            report_id: 4,
            logical_min: 0,
            logical_max: 0,
            physical_min: 0,
            physical_max: 0,
            units: 0,
            units_exp: 0,
        }
    }

    /// The descriptor of a typical laptop: five slots, X and Y in 0.01 cm, a contact count at the top.
    fn typical() -> Vec<Cap> {
        let mut caps = Vec::new();
        for link in 1..=5 {
            caps.push(Cap {
                logical_max: 3000,
                physical_max: 1050,
                units: 0x11,
                units_exp: 0xE, // -2
                ..cap(PAGE_GENERIC, USAGE_X, link)
            });
            caps.push(Cap {
                logical_max: 2000,
                physical_max: 700,
                units: 0x11,
                units_exp: 0xE,
                ..cap(PAGE_GENERIC, USAGE_Y, link)
            });
            caps.push(Cap {
                logical_max: 15,
                ..cap(PAGE_DIGITIZER, USAGE_CONTACT_ID, link)
            });
        }
        caps.push(Cap {
            logical_max: 5,
            ..cap(PAGE_DIGITIZER, USAGE_CONTACT_COUNT, 0)
        });
        caps
    }

    fn contact(id: i32, x: i32) -> RawContact {
        RawContact {
            id,
            tip: true,
            confidence: true,
            x,
            y: 1000,
        }
    }

    #[test]
    fn the_size_comes_from_the_physical_range_and_the_unit() {
        let near = |got: Option<f32>, want: f32| got.is_some_and(|g| (g - want).abs() < 0.05);
        assert!(near(extent_mm(0, 1050, 0x11, 0xE), 105.0), "0.01 cm");
        assert!(near(extent_mm(0, 105, 0x11, 0xF), 105.0), "0.1 cm");
        assert!(near(extent_mm(0, 4, 0x13, 0), 101.6), "inches");
        assert!(
            near(extent_mm(100, 1150, 0x11, 0xE), 105.0),
            "the range does not start at 0"
        );
        assert_eq!(extent_mm(0, 1050, 0, 0), None, "no unit");
        assert_eq!(extent_mm(10, 10, 0x11, 0xE), None, "no extent");
        assert_eq!(
            extent_mm(0, 100_000, 0x11, 0xE),
            None,
            "a trackpad 10 m wide is not believed"
        );
    }

    #[test]
    fn a_typical_descriptor_gives_five_slots_and_the_size() {
        let spec = layout_from_caps(&typical()).expect("a precision touchpad");
        assert_eq!(spec.report_id, 4);
        assert_eq!(spec.slots, vec![1, 2, 3, 4, 5]);
        assert_eq!(spec.count_link, 0);
        assert_eq!(spec.surface.x, (0, 3000));
        assert_eq!(spec.surface.y, (0, 2000));
        assert!((spec.surface.size_mm.0 - 105.0).abs() < 0.05);
        assert!((spec.surface.size_mm.1 - 70.0).abs() < 0.05);
    }

    #[test]
    fn a_descriptor_that_is_not_a_precision_touchpad_is_ignored() {
        let mut no_count = typical();
        no_count.retain(|c| c.usage != USAGE_CONTACT_COUNT);
        assert_eq!(layout_from_caps(&no_count), None, "no contact count");

        let mut no_y = typical();
        no_y.retain(|c| c.usage != USAGE_Y);
        assert_eq!(layout_from_caps(&no_y), None, "no Y");

        let mut unnumbered = typical();
        for c in &mut unnumbered {
            c.report_id = 0;
        }
        assert_eq!(layout_from_caps(&unnumbered), None, "reports without an id");

        let mut flat = typical();
        for c in flat.iter_mut().filter(|c| c.usage == USAGE_X) {
            c.logical_max = 0;
        }
        assert_eq!(layout_from_caps(&flat), None, "an empty X range");
        assert_eq!(layout_from_caps(&[]), None);
    }

    #[test]
    fn a_device_that_does_not_say_its_size_still_works() {
        let mut caps = typical();
        for c in &mut caps {
            c.units = 0;
        }
        let spec = layout_from_caps(&caps).expect("still a touchpad");
        assert_eq!(spec.surface.size_mm, (0.0, 0.0));
    }

    #[test]
    fn only_the_slots_of_the_contact_report_count() {
        let mut caps = typical();
        // another report of the same device that also has an X (for example a pen)
        caps.push(Cap {
            report_id: 9,
            logical_max: 100,
            ..cap(PAGE_GENERIC, USAGE_X, 7)
        });
        let spec = layout_from_caps(&caps).expect("touchpad");
        assert_eq!(spec.slots, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_contact_becomes_a_position_between_zero_and_one() {
        let s = layout_from_caps(&typical()).unwrap().surface;
        let f = finger(&contact(3, 1500), &s).expect("a finger");
        assert_eq!(f.id, 3);
        assert!((f.x - 0.5).abs() < 1e-6);
        assert!((f.y - 0.5).abs() < 1e-6);
        assert!(f.size <= PALM_SIZE, "a fingertip is not a palm");
        // out of range values are held inside the surface
        let edge = finger(&contact(1, 99_999), &s).unwrap();
        assert_eq!(edge.x, 1.0);
    }

    #[test]
    fn a_lifted_finger_is_nothing_and_a_palm_is_large() {
        let s = layout_from_caps(&typical()).unwrap().surface;
        let up = RawContact {
            tip: false,
            ..contact(1, 100)
        };
        assert_eq!(finger(&up, &s), None);
        let palm = RawContact {
            confidence: false,
            ..contact(1, 100)
        };
        assert!(finger(&palm, &s).unwrap().size > PALM_SIZE);
        let broken = Surface { x: (5, 5), ..s };
        assert_eq!(finger(&contact(1, 5), &broken), None);
    }

    #[test]
    fn one_report_is_one_frame() {
        let mut a = Assembler::default();
        let mut out = Vec::new();
        assert_eq!(a.wanted(2, 5), 2, "only two slots are worth reading");
        assert!(a.push(2, &[contact(1, 10), contact(2, 20)], &mut out));
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].id, 2);
    }

    #[test]
    fn hybrid_mode_puts_two_reports_together() {
        let mut a = Assembler::default();
        let mut out = Vec::new();
        // seven fingers, five slots per report
        assert_eq!(a.wanted(7, 5), 5);
        let first: Vec<_> = (1..=5).map(|i| contact(i, i * 100)).collect();
        assert!(!a.push(7, &first, &mut out), "two more are coming");
        assert_eq!(a.wanted(0, 5), 2, "the second report has two");
        let second = [contact(6, 600), contact(7, 700)];
        assert!(a.push(0, &second, &mut out));
        assert_eq!(
            out.iter().map(|c| c.id).collect::<Vec<_>>(),
            (1..=7).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_frame_with_no_contacts_is_an_empty_frame() {
        let mut a = Assembler::default();
        let mut out = vec![contact(9, 9)];
        assert_eq!(a.wanted(0, 5), 0);
        assert!(a.push(0, &[], &mut out));
        assert!(out.is_empty(), "every finger is up");
    }

    #[test]
    fn a_frame_that_never_finished_is_dropped_by_the_next_one() {
        let mut a = Assembler::default();
        let mut out = Vec::new();
        let five: Vec<_> = (1..=5).map(|i| contact(i, 0)).collect();
        assert!(!a.push(6, &five, &mut out));
        // the continuation was lost: a new frame starts with one finger
        assert!(a.push(1, &[contact(42, 0)], &mut out));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 42);
    }

    /// A `RAWINPUT` packet as Windows lays it out: `RAWINPUTHEADER`, then `RAWHID`.
    fn packet(pointer: usize, device: u64, size: u32, count: u32, data: &[u8]) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(RIM_TYPEHID.to_le_bytes());
        let header = 8 + 2 * pointer;
        b.extend(((header + 8 + data.len()) as u32).to_le_bytes());
        match pointer {
            4 => b.extend((device as u32).to_le_bytes()),
            _ => b.extend(device.to_le_bytes()),
        }
        b.extend(vec![0u8; pointer]); // wParam
        b.extend(size.to_le_bytes());
        b.extend(count.to_le_bytes());
        b.extend(data);
        b
    }

    #[test]
    fn a_hid_packet_is_read_on_32_and_64_bit_layouts() {
        for (pointer, header) in [(8usize, 24usize), (4, 16)] {
            let p = packet(pointer, 0x1234, 3, 2, &[1, 2, 3, 4, 5, 6]);
            let hid = parse_raw_hid(&p, header).expect("a HID packet");
            assert_eq!(hid.device, 0x1234);
            assert_eq!(hid.size, 3);
            let reports: Vec<_> = hid.reports().collect();
            assert_eq!(reports, vec![&[1u8, 2, 3][..], &[4, 5, 6][..]]);
        }
    }

    #[test]
    fn a_packet_that_is_not_hid_or_is_cut_off_is_nothing() {
        let mut mouse = packet(8, 1, 3, 1, &[1, 2, 3]);
        mouse[0] = 0; // RIM_TYPEMOUSE
        assert_eq!(parse_raw_hid(&mouse, 24), None);
        let cut = packet(8, 1, 3, 2, &[1, 2, 3]); // says two reports, has one
        assert_eq!(parse_raw_hid(&cut, 24), None);
        assert_eq!(parse_raw_hid(&[], 24), None);
        assert_eq!(
            parse_raw_hid(&packet(8, 1, 0, 1, &[]), 24),
            None,
            "an empty report"
        );
        assert_eq!(
            parse_raw_hid(&packet(8, 1, 3, 1, &[1, 2, 3]), 20),
            None,
            "a header of an odd size"
        );
    }

    #[test]
    fn the_device_info_says_whether_it_is_a_touchpad() {
        let usage = |page: u32, usage: u32| {
            [
                32,
                RIM_TYPEHID,
                0x046D,
                0xC52B,
                1,
                page | (usage << 16),
                0,
                0,
            ]
        };
        assert!(is_touchpad_info(&usage(0x0D, 0x05)));
        assert!(!is_touchpad_info(&usage(0x0D, 0x04)), "a touch screen");
        assert!(!is_touchpad_info(&usage(0x01, 0x06)), "a keyboard");
        let mut mouse = usage(0x0D, 0x05);
        mouse[1] = 0;
        assert!(!is_touchpad_info(&mouse), "not a HID device");
    }
}
