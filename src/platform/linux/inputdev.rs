//! The mice the kernel lists in `/proc/bus/input/devices` (pure: the file is read by
//! `backend::mouse_devices`, so the parser is tested on every OS).
//!
//! One device is a block of lines separated from the next by an empty line:
//!
//! ```text
//! I: Bus=0003 Vendor=046d Product=c08b Version=0111
//! N: Name="Logitech G502 HERO Gaming Mouse"
//! H: Handlers=mouse1 event7
//! B: EV=17
//! B: KEY=ffff0000 0 0 0 0
//! B: REL=143
//! ```
//!
//! `B: KEY` lists the buttons and keys the device can send as a bitmap of hexadecimal words, the
//! highest word first. A mouse has `BTN_LEFT` and moves with `REL_X` and `REL_Y`; a touchpad or a
//! tablet moves with absolute axes and is left out. The extra buttons are `BTN_SIDE` (0x113) and the
//! codes after it. The X server numbers them 8, 9, 10... in that order (xf86-input-libinput), and
//! CursorPie's numbers are those minus 5 (3 = back, 4 = forward, 5 and up extra), see `mouse`.

use crate::mouse::{self, Device};

/// `BTN_LEFT`: every mouse has it.
const BTN_LEFT: usize = 0x110;
/// `BTN_SIDE`: the first extra button, X button 8, CursorPie's button 3 (back).
const BTN_SIDE: usize = 0x113;
/// The last code of the mouse group of `input-event-codes.h`.
const BTN_LAST: usize = 0x11f;
/// `REL_X` and `REL_Y`.
const REL_X: usize = 0;
const REL_Y: usize = 1;

/// Words of a device name that mark a program's pointer, not a mouse in a hand: they would be met as
/// a "new mouse" every time the program starts.
const VIRTUAL: [&str; 3] = ["virtual", "ydotool", "uinput"];

/// Whether bit `bit` is set in a kernel bitmap such as `ffff0000 0 0`: words of hexadecimal digits,
/// the highest word first. A word has as many bits as the kernel's `long` (64 on a 64-bit kernel).
fn bit_set(map: &str, bit: usize) -> bool {
    let words: Vec<&str> = map.split_whitespace().collect();
    let width = if words.iter().any(|w| w.len() > 8) {
        64
    } else {
        usize::BITS as usize
    };
    let Some(word) = words
        .len()
        .checked_sub(bit / width + 1)
        .and_then(|at| words.get(at))
    else {
        return false;
    };
    let Ok(value) = u64::from_str_radix(word, 16) else {
        return false;
    };
    ((value >> (bit % width)) & 1) == 1
}

/// The value of `key=` in a line such as `Bus=0003 Vendor=046d Product=c08b`.
fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let at = line.find(key)? + key.len();
    line[at..].split_whitespace().next()
}

/// The device of one block, if it is a mouse.
fn mouse_of(block: &str) -> Option<Device> {
    let (mut ids, mut name, mut handlers) = (None, None, "");
    let (mut key, mut rel) = ("", "");
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("I: ") {
            ids = Some(rest);
        } else if let Some(rest) = line.strip_prefix("N: Name=") {
            name = Some(rest.trim().trim_matches('"'));
        } else if let Some(rest) = line.strip_prefix("H: Handlers=") {
            handlers = rest;
        } else if let Some(rest) = line.strip_prefix("B: KEY=") {
            key = rest;
        } else if let Some(rest) = line.strip_prefix("B: REL=") {
            rel = rest;
        }
    }
    let name = name?;
    let is_mouse = handlers.split_whitespace().any(|h| h.starts_with("mouse"))
        && bit_set(key, BTN_LEFT)
        && bit_set(rel, REL_X)
        && bit_set(rel, REL_Y);
    let lower = name.to_ascii_lowercase();
    if !is_mouse || VIRTUAL.iter().any(|v| lower.contains(v)) {
        return None;
    }
    let hw_id = hardware_id(ids);
    let extra = (BTN_SIDE..=BTN_LAST)
        .filter(|code| bit_set(key, *code))
        .map(|code| mouse::BACK + (code - BTN_SIDE) as i64)
        .collect();
    Some(Device {
        name: name.to_string(),
        hw_id,
        extra,
    })
}

/// `046d:c08b` from the `I:` line of a device; empty when the line is missing or the ids are zero.
fn hardware_id(ids: Option<&str>) -> String {
    let Some(line) = ids else {
        return String::new();
    };
    let (Some(vendor), Some(product)) = (field(line, "Vendor="), field(line, "Product=")) else {
        return String::new();
    };
    if is_zero(vendor) && is_zero(product) {
        return String::new();
    }
    format!("{vendor}:{product}")
}

/// Whether a hexadecimal id is all zeros (devices that have no real ids, such as virtual ones).
fn is_zero(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c == '0')
}

/// The mice in the text of `/proc/bus/input/devices`.
pub fn parse_devices(text: &str) -> Vec<Device> {
    text.split("\n\n").filter_map(mouse_of).collect()
}

/// A touchpad the kernel lists: its name and its `eventN` node under `/dev/input`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touchpad {
    pub name: String,
    /// `event9`, from the `Handlers=` line.
    pub node: String,
}

/// `INPUT_PROP_DIRECT`: a touchscreen, not a touchpad.
const PROP_DIRECT: usize = 1;
/// `BTN_TOOL_FINGER`: a touchpad reports how many fingers touch it.
const BTN_TOOL_FINGER: usize = 0x145;
/// `ABS_MT_POSITION_X` and `ABS_MT_POSITION_Y`: the position of each finger.
const ABS_MT_POSITION_X: usize = 0x35;
const ABS_MT_POSITION_Y: usize = 0x36;

/// What the kernel's description of one input device says about it, for [`touchpad_of`] and for
/// the debug report (`cursorpie --touchpad-debug`).
struct Block<'a> {
    name: &'a str,
    node: &'a str,
    /// `Ok` for a touchpad; otherwise the first thing that rules it out.
    verdict: Result<(), &'static str>,
    /// The device has some of what a touchpad has (the finger button, finger positions, or the
    /// word in its name): worth listing when it was not taken.
    looks_related: bool,
}

fn judge(block: &str) -> Option<Block<'_>> {
    let (mut name, mut handlers) = (None, "");
    let (mut key, mut abs, mut prop) = ("", "", None);
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("N: Name=") {
            name = Some(rest.trim().trim_matches('"'));
        } else if let Some(rest) = line.strip_prefix("H: Handlers=") {
            handlers = rest;
        } else if let Some(rest) = line.strip_prefix("B: KEY=") {
            key = rest;
        } else if let Some(rest) = line.strip_prefix("B: ABS=") {
            abs = rest;
        } else if let Some(rest) = line.strip_prefix("B: PROP=") {
            prop = Some(rest);
        }
    }
    let name = name?;
    let node = handlers
        .split_whitespace()
        .find(|h| h.starts_with("event"))?;
    let lower = name.to_ascii_lowercase();
    let finger = bit_set(key, BTN_TOOL_FINGER);
    let position = bit_set(abs, ABS_MT_POSITION_X) && bit_set(abs, ABS_MT_POSITION_Y);
    let verdict = if !finger {
        Err("does not report a finger count (no BTN_TOOL_FINGER)")
    } else if !position {
        Err("does not report finger positions (no ABS_MT_POSITION_X/Y)")
    } else if prop.is_some_and(|p| bit_set(p, PROP_DIRECT)) {
        Err("is a touchscreen (INPUT_PROP_DIRECT)")
    } else if VIRTUAL.iter().any(|v| lower.contains(v)) {
        Err("is a virtual device made by a program")
    } else {
        Ok(())
    };
    Some(Block {
        name,
        node,
        verdict,
        looks_related: finger
            || position
            || lower.contains("touchpad")
            || lower.contains("trackpad"),
    })
}

/// The touchpad of one block, if it is one: it reports the position of several fingers
/// (multitouch) and the number of fingers (`BTN_TOOL_FINGER`), and it is not a touchscreen.
fn touchpad_of(block: &str) -> Option<Touchpad> {
    let b = judge(block)?;
    b.verdict.is_ok().then(|| Touchpad {
        name: b.name.to_string(),
        node: b.node.to_string(),
    })
}

/// One device that looks like it could be a touchpad, and what was decided about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub node: String,
    /// `Ok` = taken as a touchpad; `Err` says why not.
    pub verdict: Result<(), &'static str>,
}

/// Every device of `/proc/bus/input/devices` that has anything to do with touch, with the verdict:
/// what `cursorpie --touchpad-debug` prints, so a touchpad that is not found says why.
pub fn explain_touchpads(text: &str) -> Vec<Candidate> {
    text.split("\n\n")
        .filter_map(judge)
        .filter(|b| b.looks_related)
        .map(|b| Candidate {
            name: b.name.to_string(),
            node: b.node.to_string(),
            verdict: b.verdict,
        })
        .collect()
}

/// The touchpads in the text of `/proc/bus/input/devices`. Readable by everyone, unlike the
/// `/dev/input/event*` nodes, so Settings can say "there is a touchpad, but no permission".
pub fn parse_touchpads(text: &str) -> Vec<Touchpad> {
    text.split("\n\n").filter_map(touchpad_of).collect()
}

/// A mouse the Wayland layer can grab: the `eventN` node under `/dev/input` and what the kernel
/// lists about it. Unlike [`parse_devices`] this leaves out every device a program made (the
/// kernel puts those under `/devices/virtual/`): the clone of a mouse that CursorPie made itself
/// must never be grabbed, or its own output would be read back in an endless loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MouseNode {
    pub name: String,
    /// `event7`, from the `Handlers=` line.
    pub node: String,
    /// `046d:c08b`, empty when the kernel gives none.
    pub hw_id: String,
}

/// `BTN_MIDDLE`: the trigger button, which a mouse for CursorPie must have.
const BTN_MIDDLE: usize = 0x112;

/// Whether a bitmap line such as `ABS=3` names any bit at all.
fn any_bit(map: &str) -> bool {
    map.split_whitespace()
        .any(|w| u64::from_str_radix(w, 16).is_ok_and(|v| v != 0))
}

fn mouse_node_of(block: &str) -> Option<MouseNode> {
    let (mut ids, mut name, mut handlers, mut sysfs) = (None, None, "", "");
    let (mut key, mut rel, mut abs) = ("", "", "");
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("I: ") {
            ids = Some(rest);
        } else if let Some(rest) = line.strip_prefix("N: Name=") {
            name = Some(rest.trim().trim_matches('"'));
        } else if let Some(rest) = line.strip_prefix("S: Sysfs=") {
            sysfs = rest.trim();
        } else if let Some(rest) = line.strip_prefix("H: Handlers=") {
            handlers = rest;
        } else if let Some(rest) = line.strip_prefix("B: KEY=") {
            key = rest;
        } else if let Some(rest) = line.strip_prefix("B: REL=") {
            rel = rest;
        } else if let Some(rest) = line.strip_prefix("B: ABS=") {
            abs = rest;
        }
    }
    let name = name?;
    let node = handlers
        .split_whitespace()
        .find(|h| h.starts_with("event"))?;
    let is_mouse = handlers.split_whitespace().any(|h| h.starts_with("mouse"))
        && bit_set(key, BTN_LEFT)
        && bit_set(key, BTN_MIDDLE)
        && bit_set(rel, REL_X)
        && bit_set(rel, REL_Y);
    let lower = name.to_ascii_lowercase();
    if !is_mouse
        || any_bit(abs)
        || sysfs.contains("/virtual/")
        || VIRTUAL.iter().any(|v| lower.contains(v))
    {
        return None;
    }
    Some(MouseNode {
        name: name.to_string(),
        node: node.to_string(),
        hw_id: hardware_id(ids),
    })
}

/// The mice the Wayland layer may grab, in the text of `/proc/bus/input/devices`: relative X and Y,
/// a left and a middle button, no absolute axes, and not made by a program.
pub fn parse_mouse_nodes(text: &str) -> Vec<MouseNode> {
    text.split("\n\n").filter_map(mouse_node_of).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gaming mouse with seven extra buttons (BTN_SIDE to BTN_TASK and two more).
    const GAMING: &str = "\
I: Bus=0003 Vendor=046d Product=c08b Version=0111
N: Name=\"Logitech G502 HERO Gaming Mouse\"
P: Phys=usb-0000:00:14.0-2/input0
S: Sysfs=/devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.0/input/input12
U: Uniq=
H: Handlers=mouse1 event7
B: PROP=0
B: EV=17
B: KEY=ffff0000 0 0 0 0
B: REL=943
B: MSC=10
";

    /// A plain office mouse: left, right, middle, back and forward.
    const OFFICE: &str = "\
I: Bus=0003 Vendor=413c Product=301a Version=0111
N: Name=\"Dell MS116 USB Optical Mouse\"
H: Handlers=mouse0 event3
B: EV=17
B: KEY=1f0000 0 0 0 0
B: REL=903
";

    const KEYBOARD: &str = "\
I: Bus=0003 Vendor=046d Product=c31c Version=0110
N: Name=\"Logitech USB Keyboard\"
H: Handlers=sysrq kbd leds event4
B: EV=120013
B: KEY=1000000000007 ff9f207ac14057ff febeffdfffefffff fffffffffffffffe
B: MSC=10
B: LED=7
";

    const TOUCHPAD: &str = "\
I: Bus=0018 Vendor=04f3 Product=3140 Version=0100
N: Name=\"ELAN0412:00 04F3:3140 Touchpad\"
H: Handlers=mouse2 event9
B: EV=b
B: KEY=e520 10000 0 0 0 0
B: ABS=260800000000003
";

    fn blocks(parts: &[&str]) -> String {
        parts.join("\n")
    }

    #[test]
    fn bitmaps_read_the_highest_word_first() {
        // 0x1f0000: bits 16 to 20 of the last word
        assert!(bit_set("1f0000", 16));
        assert!(bit_set("1f0000", 20));
        assert!(!bit_set("1f0000", 21));
        // a bit of a higher word: bit 272 is bit 16 of the fifth word from the right (64-bit words)
        let map = "1f0000 0 0 0 0";
        if usize::BITS == 64 {
            assert!(bit_set(map, 0x110));
            assert!(bit_set(map, 0x114));
            assert!(!bit_set(map, 0x115));
        }
        // a map that is too short, or empty, has no such bit
        assert!(!bit_set("1f0000", 0x110));
        assert!(!bit_set("", 0));
        assert!(!bit_set("zz", 0));
    }

    #[test]
    fn a_mouse_is_found_with_its_ids_and_extra_buttons() {
        if usize::BITS != 64 {
            return; // the sample maps are those of a 64-bit kernel
        }
        let found = parse_devices(&blocks(&[GAMING, OFFICE]));
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "Logitech G502 HERO Gaming Mouse");
        assert_eq!(found[0].hw_id, "046d:c08b");
        // ffff0000 is bits 16 to 31 of the last word: BTN_LEFT 0x110 to 0x11f
        assert_eq!(
            found[0].extra,
            [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
        );
        assert_eq!(found[1].hw_id, "413c:301a");
        assert_eq!(
            found[1].extra,
            [3, 4],
            "BTN_SIDE and BTN_EXTRA: back, forward"
        );
    }

    #[test]
    fn keyboards_and_touchpads_are_not_mice() {
        if usize::BITS != 64 {
            return;
        }
        assert!(parse_devices(&blocks(&[KEYBOARD, TOUCHPAD])).is_empty());
        // the mouse between them is still found
        let found = parse_devices(&blocks(&[KEYBOARD, OFFICE, TOUCHPAD]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Dell MS116 USB Optical Mouse");
    }

    #[test]
    fn a_program_pretending_to_be_a_mouse_is_left_out() {
        if usize::BITS != 64 {
            return;
        }
        let fake = OFFICE.replace("Dell MS116 USB Optical Mouse", "ydotoold virtual device");
        assert!(parse_devices(&fake).is_empty());
    }

    #[test]
    fn a_device_without_real_ids_is_known_by_its_name() {
        if usize::BITS != 64 {
            return;
        }
        let none = OFFICE.replace("Vendor=413c Product=301a", "Vendor=0000 Product=0000");
        let found = parse_devices(&none);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].hw_id, "");
        assert_eq!(found[0].id(), "name:dell ms116 usb optical mouse");
    }

    #[test]
    fn garbage_gives_no_mice() {
        assert!(parse_devices("").is_empty());
        assert!(parse_devices("not a device list\n\nat all").is_empty());
        assert!(parse_devices("N: Name=\"x\"\nH: Handlers=mouse0\n").is_empty());
    }

    #[test]
    fn a_touchpad_is_found_by_its_fingers_and_a_mouse_is_not() {
        let text = blocks(&[GAMING, TOUCHPAD, KEYBOARD, OFFICE]);
        assert_eq!(
            parse_touchpads(&text),
            vec![Touchpad {
                name: "ELAN0412:00 04F3:3140 Touchpad".into(),
                node: "event9".into()
            }]
        );
        assert!(parse_touchpads(&blocks(&[GAMING, OFFICE, KEYBOARD])).is_empty());
    }

    #[test]
    fn a_touchscreen_is_not_a_touchpad() {
        let screen = "\
I: Bus=0018 Vendor=04f3 Product=2a12 Version=0100
N: Name=\"ELAN Touchscreen\"
H: Handlers=mouse3 event11
B: PROP=2
B: EV=b
B: KEY=e520 10000 0 0 0 0
B: ABS=260800000000003
";
        assert!(parse_touchpads(screen).is_empty());
        // a touchscreen without the finger button is no touchpad either
        let no_finger = screen.replace("e520", "400");
        assert!(parse_touchpads(&no_finger).is_empty(), "no BTN_TOOL_FINGER");
    }

    #[test]
    fn the_debug_list_says_why_a_device_was_not_taken() {
        let list = explain_touchpads(&blocks(&[GAMING, OFFICE, KEYBOARD]));
        assert!(
            list.is_empty(),
            "a mouse and a keyboard are not touch related"
        );
        let pad = explain_touchpads(TOUCHPAD);
        assert_eq!(pad.len(), 1);
        assert_eq!(pad[0].verdict, Ok(()));
        assert_eq!(pad[0].node, parse_touchpads(TOUCHPAD)[0].node);
        // still listed (it has positions), but not taken: no finger button
        let no_finger = explain_touchpads(&TOUCHPAD.replace("e520", "400"));
        assert_eq!(no_finger.len(), 1);
        assert!(
            no_finger[0]
                .verdict
                .unwrap_err()
                .contains("BTN_TOOL_FINGER")
        );
        let fake = TOUCHPAD.replace(
            "ELAN0412:00 04F3:3140 Touchpad",
            "ydotoold virtual touchpad",
        );
        let fake = explain_touchpads(&fake);
        assert_eq!(fake.len(), 1);
        assert!(fake[0].verdict.unwrap_err().contains("virtual"));
    }

    #[test]
    fn a_program_s_virtual_touchpad_is_left_out() {
        let fake = TOUCHPAD.replace("ELAN0412:00 04F3:3140 Touchpad", "ydotoold virtual device");
        assert!(parse_touchpads(&fake).is_empty());
    }

    #[test]
    fn a_mouse_node_is_found_and_the_clone_of_it_is_not() {
        if usize::BITS != 64 {
            return; // the sample maps are those of a 64-bit kernel
        }
        // what the kernel lists after CursorPie made a clone of the G502: the same name and ids, but
        // under /devices/virtual/
        let clone = "\
I: Bus=0003 Vendor=046d Product=c08b Version=0111
N: Name=\"Logitech G502 HERO Gaming Mouse\"
S: Sysfs=/devices/virtual/input/input31
H: Handlers=mouse4 event12
B: EV=17
B: KEY=ffff0000 0 0 0 0
B: REL=943
";
        let found = parse_mouse_nodes(&blocks(&[GAMING, clone, OFFICE, KEYBOARD, TOUCHPAD]));
        let nodes: Vec<&str> = found.iter().map(|m| m.node.as_str()).collect();
        assert_eq!(
            nodes,
            ["event7", "event3"],
            "the clone (event12) is left out"
        );
        assert_eq!(found[0].name, "Logitech G502 HERO Gaming Mouse");
        assert_eq!(found[0].hw_id, "046d:c08b");
    }

    #[test]
    fn a_pointer_that_has_absolute_axes_or_no_middle_button_is_not_grabbed() {
        if usize::BITS != 64 {
            return;
        }
        let absolute = "\
I: Bus=0003 Vendor=056a Product=0302 Version=0100
N: Name=\"Wacom Pen\"
H: Handlers=mouse5 event8
B: EV=1f
B: KEY=1f0000 0 0 0 0
B: REL=903
B: ABS=3
";
        // left and right only (0x30000 = BTN_LEFT, BTN_RIGHT)
        let two_buttons = "\
I: Bus=0003 Vendor=1234 Product=5678 Version=0100
N: Name=\"Two Button Mouse\"
H: Handlers=mouse6 event9
B: EV=17
B: KEY=30000 0 0 0 0
B: REL=3
";
        assert!(parse_mouse_nodes(&blocks(&[absolute, two_buttons])).is_empty());
    }

    #[test]
    fn a_pointer_of_another_program_is_not_grabbed() {
        if usize::BITS != 64 {
            return;
        }
        let ydotool = "\
I: Bus=0003 Vendor=0000 Product=0000 Version=0000
N: Name=\"ydotoold virtual device\"
S: Sysfs=/devices/virtual/input/input40
H: Handlers=mouse7 event14
B: EV=17
B: KEY=ffff0000 0 0 0 0
B: REL=903
";
        assert!(parse_mouse_nodes(ydotool).is_empty());
    }
}
