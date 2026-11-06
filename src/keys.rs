//! Shortcut model: virtual key code + modifier bitmask, with display labels.

use serde::{Deserialize, Serialize};

/// Modifier bit: Shift.
pub const MOD_SHIFT: u8 = 1;
/// Modifier bit: Control.
pub const MOD_CTRL: u8 = 2;
/// Modifier bit: Option (Alt).
pub const MOD_ALT: u8 = 4;
/// Modifier bit: Command.
pub const MOD_CMD: u8 = 8;

/// The modifiers in the order a label lists them when the press order is not known (the order of
/// the macOS menus): Ctrl, Opt, Shift, Cmd.
pub const MOD_ORDER: [u8; 4] = [MOD_CTRL, MOD_ALT, MOD_SHIFT, MOD_CMD];

/// Every `MOD_*` bit together.
const MOD_ALL: u8 = MOD_SHIFT | MOD_CTRL | MOD_ALT | MOD_CMD;

/// Two bits of [`Shortcut::order`] name one modifier: its index in this table.
const MOD_BY_ID: [u8; 4] = [MOD_SHIFT, MOD_CTRL, MOD_ALT, MOD_CMD];

/// Virtual key codes that mean the same thing on every layout (no letters / digits / punctuation:
/// those move around between layouts, use `Shortcut::ch` for them).
pub mod key {
    /// Space bar.
    pub const SPACE: u16 = 49;
    /// Return.
    pub const RETURN: u16 = 36;
    /// Backspace (the key macOS calls Delete).
    pub const DELETE: u16 = 51;
    /// Page Up.
    pub const PAGE_UP: u16 = 116;
    /// Page Down.
    pub const PAGE_DOWN: u16 = 121;
    /// Tab.
    pub const TAB: u16 = 48;
    /// Arrow left.
    pub const LEFT: u16 = 123;
    /// Arrow right.
    pub const RIGHT: u16 = 124;
    /// Arrow up.
    pub const UP: u16 = 126;
    /// F13 (a safe default for a new shortcut slice).
    pub const F13: u16 = 105;
}

/// One shortcut = any key code + modifier combination (recorded from the keyboard).
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub struct Shortcut {
    pub key: u16,
    pub mods: u8,
    /// The order the modifiers were pressed in while recording, two bits per modifier (the first
    /// pressed in the lowest bits; see [`Shortcut::with_order`]). `0` means the usual order
    /// ([`MOD_ORDER`]), which is also what every shortcut saved before this field existed has. It
    /// is left out of `config.json` while it is `0`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub order: u8,
}

fn is_zero(v: &u8) -> bool {
    *v == 0
}

impl Shortcut {
    /// A shortcut from a virtual key code and a `MOD_*` bitmask.
    pub const fn new(key: u16, mods: u8) -> Self {
        Self {
            key,
            mods,
            order: 0,
        }
    }

    /// This shortcut with its modifiers in the order `pressed` lists them (`MOD_*` bits, first
    /// pressed first). Bits that are not in `mods` are ignored and modifiers `pressed` leaves out
    /// follow in the usual order. The usual order is stored as `0`, so a shortcut that was pressed
    /// in that order equals one made with [`Shortcut::new`].
    pub fn with_order(mut self, pressed: &[u8]) -> Self {
        let mods = self.mods & MOD_ALL;
        let mut list: Vec<u8> = Vec::new();
        for &bit in pressed {
            if mods & bit != 0 && bit.count_ones() == 1 && !list.contains(&bit) {
                list.push(bit);
            }
        }
        for bit in MOD_ORDER {
            if mods & bit != 0 && !list.contains(&bit) {
                list.push(bit);
            }
        }
        self.order = if list == self.usual_mods() {
            0
        } else {
            list.iter().enumerate().fold(0u8, |acc, (i, bit)| {
                let id = MOD_BY_ID.iter().position(|b| b == bit).unwrap_or(0) as u8;
                acc | (id << (2 * i))
            })
        };
        self
    }

    /// The modifiers of this shortcut in the usual order ([`MOD_ORDER`]).
    fn usual_mods(&self) -> Vec<u8> {
        MOD_ORDER
            .iter()
            .copied()
            .filter(|bit| self.mods & bit != 0)
            .collect()
    }

    /// The modifiers of this shortcut (`MOD_*` bits) in the order they were pressed. The usual
    /// order when none was recorded, or when the recorded one does not match `mods`.
    pub fn ordered_mods(&self) -> Vec<u8> {
        let usual = self.usual_mods();
        if self.order == 0 {
            return usual;
        }
        let list: Vec<u8> = (0..usual.len())
            .map(|i| MOD_BY_ID[((self.order >> (2 * i)) & 3) as usize])
            .collect();
        let seen = list.iter().fold(0u8, |acc, bit| acc | bit);
        let known = usual.iter().fold(0u8, |acc, bit| acc | bit);
        if seen == known && seen.count_ones() as usize == list.len() {
            list
        } else {
            usual
        }
    }

    /// The key that *types* `c` on the current layout (AZERTY, QWERTZ, Dvorak, ...), plus any
    /// modifier that layout needs to type it. Use for built-in presets such as Cmd+Z.
    /// Falls back to the US/ANSI position if the layout is not available yet.
    pub fn ch(c: char, mods: u8) -> Self {
        let c = c.to_ascii_lowercase();
        if let Some((code, need)) = crate::platform::layout::find_key(&c.to_string()) {
            return Self::new(code, mods | need);
        }
        Self::new(ansi_code(c).unwrap_or(0), mods)
    }

    /// Display text, the modifiers in the order they were pressed. Shift + a symbol key shows the resulting symbol ("+" instead of "Shift + =").
    pub fn label(&self) -> String {
        self.label_for(crate::platform::OS)
    }

    /// [`label`](Self::label) with the modifier names of `os` ("Opt" and "Cmd" on macOS, "Alt" and
    /// "Win" on Windows).
    pub fn label_for(&self, os: crate::platform::Os) -> String {
        let names = os.mod_names();
        let shift = self.mods & MOD_SHIFT != 0;
        let sym = if shift {
            shifted_symbol(self.key)
        } else {
            None
        };

        let mut parts: Vec<String> = Vec::new();
        for bit in self.ordered_mods() {
            match bit {
                MOD_CTRL => parts.push(names.ctrl.into()),
                MOD_ALT => parts.push(names.alt.into()),
                MOD_SHIFT if sym.is_none() => parts.push(names.shift.into()),
                MOD_CMD => parts.push(names.cmd.into()),
                _ => {}
            }
        }
        parts.push(match sym {
            Some(s) => s,
            None => key_name(self.key),
        });
        parts.join(" + ")
    }
}

/// Symbol a key types while Shift is held on the *current* layout, when that is a symbol/digit
/// rather than just the capital letter ("+" instead of "Shift + =").
fn shifted_symbol(code: u16) -> Option<String> {
    if special_name(code).is_some() {
        return None;
    }
    let shifted = crate::platform::layout::char_for(code, true)?;
    if shifted.chars().any(|c| c.is_alphabetic()) {
        return None;
    }
    if crate::platform::layout::char_for(code, false).as_deref() == Some(shifted.as_str()) {
        return None;
    }
    Some(shifted)
}

/// Keys that are named the same on every layout (not produced by UCKeyTranslate).
pub fn special_name(code: u16) -> Option<&'static str> {
    Some(match code {
        36 => "Return",
        48 => "Tab",
        49 => "Space",
        51 => "Backspace",
        53 => "Esc",
        64 => "F17",
        65 => "Num .",
        67 => "Num *",
        69 => "Num +",
        71 => "Num Clear",
        72 => "Volume Up",
        73 => "Volume Down",
        74 => "Mute",
        75 => "Num /",
        76 => "Num Enter",
        78 => "Num -",
        79 => "F18",
        80 => "F19",
        81 => "Num =",
        82 => "Num 0",
        83 => "Num 1",
        84 => "Num 2",
        85 => "Num 3",
        86 => "Num 4",
        87 => "Num 5",
        88 => "Num 6",
        89 => "Num 7",
        90 => "F20",
        91 => "Num 8",
        92 => "Num 9",
        96 => "F5",
        97 => "F6",
        98 => "F7",
        99 => "F3",
        100 => "F8",
        101 => "F9",
        102 => "Eisu",
        103 => "F11",
        104 => "Kana",
        105 => "F13",
        106 => "F16",
        107 => "F14",
        109 => "F10",
        111 => "F12",
        113 => "F15",
        114 => "Help",
        115 => "Home",
        116 => "Page Up",
        117 => "Delete",
        118 => "F4",
        119 => "End",
        120 => "F2",
        121 => "Page Down",
        122 => "F1",
        123 => "Left",
        124 => "Right",
        125 => "Down",
        126 => "Up",
        _ => return None,
    })
}

/// US/ANSI letters, digits and punctuation. Only a fallback for when the layout snapshot is not
/// available (e.g. queried from another thread before `layout::init()` ran).
const ANSI: &[(u16, &str)] = &[
    (0, "A"),
    (1, "S"),
    (2, "D"),
    (3, "F"),
    (4, "H"),
    (5, "G"),
    (6, "Z"),
    (7, "X"),
    (8, "C"),
    (9, "V"),
    (11, "B"),
    (12, "Q"),
    (13, "W"),
    (14, "E"),
    (15, "R"),
    (16, "Y"),
    (17, "T"),
    (18, "1"),
    (19, "2"),
    (20, "3"),
    (21, "4"),
    (22, "6"),
    (23, "5"),
    (24, "="),
    (25, "9"),
    (26, "7"),
    (27, "-"),
    (28, "8"),
    (29, "0"),
    (30, "]"),
    (31, "O"),
    (32, "U"),
    (33, "["),
    (34, "I"),
    (35, "P"),
    (37, "L"),
    (38, "J"),
    (39, "'"),
    (40, "K"),
    (41, ";"),
    (42, "\\"),
    (43, ","),
    (44, "/"),
    (45, "N"),
    (46, "M"),
    (47, "."),
    (50, "`"),
];

/// Key code of a letter, digit or punctuation mark on a US/ANSI keyboard.
fn ansi_code(c: char) -> Option<u16> {
    let s = c.to_string();
    ANSI.iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(&s))
        .map(|(k, _)| *k)
}

/// Display name of a key on the current layout. Letters are shown uppercase ("Z", not "z").
pub fn key_name(code: u16) -> String {
    if let Some(s) = special_name(code) {
        return s.to_string();
    }
    if let Some(s) = crate::platform::layout::char_for(code, false) {
        // keep "ß"-style characters as typed instead of expanding them to "SS"
        let upper = s.to_uppercase();
        return if upper.chars().count() == s.chars().count() {
            upper
        } else {
            s
        };
    }
    if let Some((_, n)) = ANSI.iter().find(|(k, _)| *k == code) {
        return n.to_string();
    }
    format!("Key {code}")
}

#[cfg(test)]
mod tests {
    //! Characterization tests: they pin what the macOS build produced before Windows support was
    //! added. Test threads never see the keyboard-layout snapshot (it is only built on the main
    //! thread), so labels and `Shortcut::ch` use the US/ANSI fallback table here.
    use super::*;

    #[test]
    fn modifier_bits_are_stable() {
        // these values are stored in config.json files
        assert_eq!((MOD_SHIFT, MOD_CTRL, MOD_ALT, MOD_CMD), (1, 2, 4, 8));
    }

    #[test]
    fn key_constants_are_macos_virtual_key_codes() {
        assert_eq!(key::SPACE, 49);
        assert_eq!(key::RETURN, 36);
        assert_eq!(key::DELETE, 51);
        assert_eq!(key::PAGE_UP, 116);
        assert_eq!(key::PAGE_DOWN, 121);
        assert_eq!(key::UP, 126);
        assert_eq!((key::TAB, key::LEFT, key::RIGHT), (48, 123, 124));
        assert_eq!(key::F13, 105);
    }

    #[test]
    fn ch_resolves_through_the_ansi_fallback() {
        assert_eq!(Shortcut::ch('z', MOD_CMD), Shortcut::new(6, MOD_CMD));
        assert_eq!(Shortcut::ch('Z', 0), Shortcut::new(6, 0), "case is ignored");
        assert_eq!(Shortcut::ch('[', 0), Shortcut::new(33, 0));
        assert_eq!(Shortcut::ch(']', MOD_CMD), Shortcut::new(30, MOD_CMD));
        assert_eq!(Shortcut::ch('-', 0).key, 27);
        assert_eq!(Shortcut::ch('=', 0).key, 24);
        assert_eq!(Shortcut::ch('4', MOD_CMD | MOD_SHIFT).key, 21);
        assert_eq!(Shortcut::ch('q', 0).key, 12);
        assert_eq!(
            Shortcut::ch('\u{20ac}', 0).key,
            0,
            "unknown text falls back to key 0"
        );
    }

    #[test]
    fn labels_list_modifiers_in_a_fixed_order() {
        use crate::platform::Os;
        // the macOS names, pinned: the labels of the other OS are tested below
        assert_eq!(Shortcut::new(6, MOD_CMD).label_for(Os::Mac), "Cmd + Z");
        assert_eq!(
            Shortcut::new(6, MOD_CMD | MOD_SHIFT).label_for(Os::Mac),
            "Shift + Cmd + Z"
        );
        assert_eq!(
            Shortcut::new(6, MOD_CTRL | MOD_ALT | MOD_SHIFT | MOD_CMD).label_for(Os::Mac),
            "Ctrl + Opt + Shift + Cmd + Z"
        );
        assert_eq!(
            Shortcut::new(key::SPACE, MOD_CMD).label_for(Os::Mac),
            "Cmd + Space"
        );
        assert_eq!(Shortcut::new(key::F13, 0).label_for(Os::Mac), "F13");
        assert_eq!(
            Shortcut::new(key::UP, MOD_CTRL).label_for(Os::Mac),
            "Ctrl + Up"
        );
    }

    #[test]
    fn labels_follow_the_order_the_modifiers_were_pressed_in() {
        use crate::platform::Os;
        let all = MOD_CTRL | MOD_ALT | MOD_SHIFT | MOD_CMD;
        let sc = Shortcut::new(13, all).with_order(&[MOD_CMD, MOD_SHIFT, MOD_ALT, MOD_CTRL]);
        assert_eq!(sc.label_for(Os::Mac), "Cmd + Shift + Opt + Ctrl + W");
        // Cmd, Shift, Opt then W: what the user pressed is what the label says
        let sc = Shortcut::new(13, MOD_CMD | MOD_SHIFT | MOD_ALT)
            .with_order(&[MOD_CMD, MOD_SHIFT, MOD_ALT]);
        assert_eq!(sc.label_for(Os::Mac), "Cmd + Shift + Opt + W");
        assert_eq!(sc.label_for(Os::Windows), "Win + Shift + Alt + W");
        assert_eq!(
            sc.ordered_mods(),
            vec![MOD_CMD, MOD_SHIFT, MOD_ALT],
            "playback uses the same order"
        );
    }

    #[test]
    fn the_usual_order_is_stored_as_zero() {
        let sc = Shortcut::new(13, MOD_CTRL | MOD_CMD).with_order(&[MOD_CTRL, MOD_CMD]);
        assert_eq!(sc, Shortcut::new(13, MOD_CTRL | MOD_CMD));
        // modifiers the list leaves out, and bits that are not held, do not matter
        let sc = Shortcut::new(13, MOD_CMD | MOD_ALT).with_order(&[MOD_SHIFT, MOD_CMD]);
        assert_eq!(sc.ordered_mods(), vec![MOD_CMD, MOD_ALT]);
    }

    #[test]
    fn a_stale_order_falls_back_to_the_usual_one() {
        let mut sc = Shortcut::new(13, MOD_CMD | MOD_SHIFT).with_order(&[MOD_CMD, MOD_SHIFT]);
        assert_eq!(sc.ordered_mods(), vec![MOD_CMD, MOD_SHIFT]);
        sc.mods = MOD_CMD | MOD_ALT; // edited by hand in config.json
        assert_eq!(sc.ordered_mods(), vec![MOD_ALT, MOD_CMD]);
    }

    #[test]
    fn order_is_only_written_when_it_is_not_the_usual_one() {
        let sc = Shortcut::new(13, MOD_CMD | MOD_SHIFT).with_order(&[MOD_CMD, MOD_SHIFT]);
        let json = serde_json::to_string(&sc).unwrap();
        assert_eq!(json, r#"{"key":13,"mods":9,"order":3}"#);
        let back: Shortcut = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sc);
        // a config written before `order` existed still loads
        let old: Shortcut = serde_json::from_str(r#"{"key":13,"mods":9}"#).unwrap();
        assert_eq!(old, Shortcut::new(13, MOD_CMD | MOD_SHIFT));
    }

    #[test]
    fn labels_use_the_modifier_names_of_the_os() {
        use crate::platform::Os;
        let sc = Shortcut::new(6, MOD_CTRL | MOD_ALT | MOD_SHIFT | MOD_CMD);
        assert_eq!(sc.label_for(Os::Mac), "Ctrl + Opt + Shift + Cmd + Z");
        assert_eq!(sc.label_for(Os::Windows), "Ctrl + Alt + Shift + Win + Z");
        assert_eq!(
            Shortcut::new(key::SPACE, MOD_CMD).label_for(Os::Windows),
            "Win + Space"
        );
    }

    #[test]
    fn key_names() {
        assert_eq!(key_name(0), "A");
        assert_eq!(key_name(42), "\\");
        assert_eq!(key_name(50), "`");
        assert_eq!(key_name(36), "Return");
        assert_eq!(key_name(117), "Delete");
        assert_eq!(key_name(51), "Backspace");
        // far beyond every layout table, so no OS and no keyboard layout names it
        assert_eq!(key_name(1000), "Key 1000", "no name for an unmapped code");
    }

    #[test]
    fn special_names_cover_the_keys_that_ignore_the_layout() {
        for (code, name) in [
            (36, "Return"),
            (48, "Tab"),
            (49, "Space"),
            (53, "Esc"),
            (115, "Home"),
            (119, "End"),
            (123, "Left"),
            (124, "Right"),
            (125, "Down"),
            (126, "Up"),
            (122, "F1"),
            (111, "F12"),
        ] {
            assert_eq!(special_name(code), Some(name), "code {code}");
        }
        assert_eq!(special_name(0), None, "letters depend on the layout");
        assert_eq!(special_name(127), None);
    }

    #[test]
    fn ansi_table_has_no_duplicate_codes_or_names() {
        let mut codes: Vec<_> = ANSI.iter().map(|(c, _)| *c).collect();
        let mut names: Vec<_> = ANSI.iter().map(|(_, n)| n.to_string()).collect();
        let (nc, nn) = (codes.len(), names.len());
        codes.sort();
        codes.dedup();
        names.sort();
        names.dedup();
        assert_eq!((codes.len(), names.len()), (nc, nn));
        assert_eq!(nc, 47, "26 letters, 10 digits and 11 punctuation keys");
    }

    #[test]
    fn shortcut_json_shape_is_stable() {
        let json = serde_json::to_string(&Shortcut::new(6, MOD_CMD)).unwrap();
        assert_eq!(json, r#"{"key":6,"mods":8}"#);
        let back: Shortcut = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Shortcut::new(6, MOD_CMD));
    }
}
