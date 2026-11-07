//! Recording a shortcut in a Wayland session.
//!
//! On X11, macOS and Windows the event tap hears the key and sends it to Settings
//! (`tap::keyboard`). A Wayland program cannot hear another program's keys, and CursorPie's
//! Wayland layer never opens a keyboard (the udev rule gives it a mouse and `/dev/uinput`, nothing
//! more: a rule for keyboards would let every program of the user read what is typed). It does not
//! need to: while a shortcut is being recorded the Settings window is the one with the focus, so
//! the window's own key events carry everything, including the modifier keys on their own (egui
//! reports `ControlLeft`, `SuperLeft` and the others as keys), which also gives the order the
//! modifiers were pressed in.
//!
//! [`Recorder`] is pure (keys in, a decision out) so it is tested on every OS; [`capture`] feeds it
//! from the window's input and takes the key events away from the widgets while recording.

use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT, Shortcut};
use crate::msg::Msg;
use eframe::egui::{self, Key};

/// Whether this session records shortcuts this way: a Wayland session (the other layers have a tap
/// that does it).
pub(super) fn applies() -> bool {
    crate::platform::linux::wayland::active()
}

/// The `MOD_*` bit of a modifier key (either side).
fn modifier_of(key: Key) -> Option<u8> {
    match key {
        Key::ControlLeft | Key::ControlRight => Some(MOD_CTRL),
        Key::ShiftLeft | Key::ShiftRight => Some(MOD_SHIFT),
        Key::AltLeft | Key::AltRight => Some(MOD_ALT),
        Key::SuperLeft | Key::SuperRight => Some(MOD_CMD),
        _ => None,
    }
}

/// The macOS key code of the key at this position (the codes `config.json` stores), for the keys a
/// shortcut can use. `None` for the rest (the key is ignored, the recording goes on).
pub(super) fn mac_code(key: Key) -> Option<u16> {
    use Key::*;
    Some(match key {
        A => 0,
        S => 1,
        D => 2,
        F => 3,
        H => 4,
        G => 5,
        Z => 6,
        X => 7,
        C => 8,
        V => 9,
        IntlBackslash => 10,
        B => 11,
        Q => 12,
        W => 13,
        E => 14,
        R => 15,
        Y => 16,
        T => 17,
        Num1 => 18,
        Num2 => 19,
        Num3 => 20,
        Num4 => 21,
        Num6 => 22,
        Num5 => 23,
        Equals => 24,
        Num9 => 25,
        Num7 => 26,
        Minus => 27,
        Num8 => 28,
        Num0 => 29,
        CloseBracket => 30,
        O => 31,
        U => 32,
        OpenBracket => 33,
        I => 34,
        P => 35,
        Enter => 36,
        L => 37,
        J => 38,
        Quote => 39,
        K => 40,
        Semicolon => 41,
        Backslash => 42,
        Comma => 43,
        Slash => 44,
        N => 45,
        M => 46,
        Period => 47,
        Tab => 48,
        Space => 49,
        Backtick => 50,
        Backspace => 51,
        Escape => 53,
        F17 => 64,
        F18 => 79,
        F19 => 80,
        F20 => 90,
        F5 => 96,
        F6 => 97,
        F7 => 98,
        F3 => 99,
        F8 => 100,
        F9 => 101,
        F11 => 103,
        F13 => 105,
        F16 => 106,
        F14 => 107,
        F10 => 109,
        F12 => 111,
        F15 => 113,
        Home => 115,
        PageUp => 116,
        Delete => 117,
        F4 => 118,
        End => 119,
        F2 => 120,
        PageDown => 121,
        F1 => 122,
        ArrowLeft => 123,
        ArrowRight => 124,
        ArrowDown => 125,
        ArrowUp => 126,
        _ => return None,
    })
}

/// What a key event decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// Plain Esc: stop recording.
    Cancel,
    /// A key (with the modifiers that were down): the shortcut.
    Captured(Shortcut),
}

/// Follows the modifier keys and turns the first other key into a shortcut.
#[derive(Default)]
pub(super) struct Recorder {
    /// The modifiers held now, the one pressed first at the front.
    order: Vec<u8>,
}

impl Recorder {
    /// Forgets the modifiers (a recording starts, or stops).
    pub(super) fn reset(&mut self) {
        self.order.clear();
    }

    /// One key event. `physical` is the key by its position (it is used for the key, so Shift+/ is
    /// the slash key and not a question mark), `held` the Ctrl, Alt and Shift the window says are
    /// down now: it is the truth, so a modifier that was pressed before the recording began, or
    /// released while the window had no focus, is put right.
    pub(super) fn feed(
        &mut self,
        key: Key,
        physical: Option<Key>,
        pressed: bool,
        repeat: bool,
        held: u8,
    ) -> Option<Outcome> {
        if let Some(bit) = modifier_of(key) {
            if pressed {
                if !self.order.contains(&bit) {
                    self.order.push(bit);
                }
            } else {
                self.order.retain(|b| *b != bit);
            }
            return None;
        }
        // Ctrl, Alt and Shift are known exactly; Super is only known from its own key
        let known = MOD_CTRL | MOD_ALT | MOD_SHIFT;
        self.order.retain(|b| known & b == 0 || held & b != 0);
        for bit in [MOD_CTRL, MOD_ALT, MOD_SHIFT] {
            if held & bit != 0 && !self.order.contains(&bit) {
                self.order.push(bit);
            }
        }
        if !pressed || repeat {
            return None;
        }
        let code = mac_code(physical.unwrap_or(key)).or_else(|| mac_code(key))?;
        let mods = self.order.iter().fold(0u8, |m, b| m | b);
        if code == 53 && mods == 0 {
            return Some(Outcome::Cancel);
        }
        Some(Outcome::Captured(
            Shortcut::new(code, mods).with_order(&self.order),
        ))
    }
}

/// Reads the window's key events while a shortcut is being recorded and takes them away from the
/// widgets (so a recorded Backspace deletes nothing). The message to apply, if a key decided it.
pub(super) fn capture(ctx: &egui::Context, rec: &mut Recorder) -> Option<Msg> {
    let mut found = None;
    ctx.input_mut(|i| {
        let m = i.modifiers;
        let held = [(m.ctrl, MOD_CTRL), (m.alt, MOD_ALT), (m.shift, MOD_SHIFT)]
            .iter()
            .filter(|(down, _)| *down)
            .fold(0u8, |acc, (_, bit)| acc | bit);
        i.events.retain(|e| match e {
            egui::Event::Key {
                key,
                physical_key,
                pressed,
                repeat,
                ..
            } => {
                if found.is_none() {
                    found = rec.feed(*key, *physical_key, *pressed, *repeat, held);
                }
                false
            }
            egui::Event::Text(_) => false,
            _ => true,
        });
    });
    found.map(|o| match o {
        Outcome::Cancel => Msg::RecordCancelled,
        Outcome::Captured(sc) => Msg::KeyCaptured(sc),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(r: &mut Recorder, k: Key, pressed: bool, held: u8) -> Option<Outcome> {
        r.feed(k, Some(k), pressed, false, held)
    }

    #[test]
    fn the_first_key_with_its_modifiers_is_the_shortcut_in_the_order_they_went_down() {
        let mut r = Recorder::default();
        assert_eq!(key(&mut r, Key::SuperLeft, true, 0), None);
        assert_eq!(key(&mut r, Key::ShiftLeft, true, MOD_SHIFT), None);
        let got = key(&mut r, Key::Z, true, MOD_SHIFT).unwrap();
        let Outcome::Captured(sc) = got else {
            panic!("{got:?}")
        };
        assert_eq!(sc.key, 6, "Z");
        assert_eq!(sc.mods, MOD_CMD | MOD_SHIFT);
        assert_eq!(sc.ordered_mods(), [MOD_CMD, MOD_SHIFT], "Super first");
    }

    #[test]
    fn a_modifier_alone_is_not_a_shortcut_and_a_release_takes_it_back() {
        let mut r = Recorder::default();
        assert_eq!(key(&mut r, Key::ControlLeft, true, MOD_CTRL), None);
        assert_eq!(key(&mut r, Key::ControlLeft, false, 0), None);
        let Some(Outcome::Captured(sc)) = key(&mut r, Key::A, true, 0) else {
            panic!()
        };
        assert_eq!(sc.mods, 0);
    }

    #[test]
    fn plain_escape_cancels_but_escape_with_a_modifier_is_a_shortcut() {
        let mut r = Recorder::default();
        assert_eq!(key(&mut r, Key::Escape, true, 0), Some(Outcome::Cancel));
        let mut r = Recorder::default();
        assert!(matches!(
            key(&mut r, Key::Escape, true, MOD_CTRL),
            Some(Outcome::Captured(sc)) if sc.key == 53 && sc.mods == MOD_CTRL
        ));
    }

    #[test]
    fn releases_and_repeats_record_nothing() {
        let mut r = Recorder::default();
        assert_eq!(key(&mut r, Key::A, false, 0), None);
        assert_eq!(r.feed(Key::A, Some(Key::A), true, true, 0), None);
    }

    #[test]
    fn the_windows_modifier_state_corrects_a_modifier_pressed_before_recording() {
        let mut r = Recorder::default();
        // Ctrl was already down when the recording began: no press was seen
        let Some(Outcome::Captured(sc)) = key(&mut r, Key::S, true, MOD_CTRL) else {
            panic!()
        };
        assert_eq!(sc.mods, MOD_CTRL);
        // and one that was let go while the window had no focus is dropped
        let mut r = Recorder::default();
        key(&mut r, Key::AltLeft, true, MOD_ALT);
        let Some(Outcome::Captured(sc)) = key(&mut r, Key::S, true, 0) else {
            panic!()
        };
        assert_eq!(sc.mods, 0);
    }

    #[test]
    fn the_key_is_taken_by_position_so_shift_slash_is_the_slash_key() {
        let mut r = Recorder::default();
        let got = r.feed(Key::Questionmark, Some(Key::Slash), true, false, MOD_SHIFT);
        assert!(matches!(got, Some(Outcome::Captured(sc)) if sc.key == 44));
        // with no physical key the logical one is used if it has a code
        assert!(matches!(
            Recorder::default().feed(Key::Q, None, true, false, 0),
            Some(Outcome::Captured(sc)) if sc.key == 12
        ));
        // a key a shortcut cannot use is ignored, the recording goes on
        assert_eq!(
            Recorder::default().feed(Key::Copy, None, true, false, 0),
            None
        );
    }

    #[test]
    fn every_code_is_the_key_the_platform_tables_say_it_is() {
        use crate::platform::keymap;
        // letters and digits against the Linux table the shortcuts are played with
        let linux = |k: Key, evdev: u16| {
            assert_eq!(
                keymap::linux_code_for(mac_code(k).unwrap()),
                Some(evdev),
                "{k:?}"
            )
        };
        // linux/input-event-codes.h
        linux(Key::A, 30);
        linux(Key::Z, 44);
        linux(Key::Q, 16);
        linux(Key::M, 50);
        linux(Key::Num1, 2);
        linux(Key::Num0, 11);
        linux(Key::Enter, 28);
        linux(Key::Space, 57);
        linux(Key::Tab, 15);
        linux(Key::Escape, 1);
        linux(Key::Backspace, 14);
        linux(Key::Minus, 12);
        linux(Key::Equals, 13);
        linux(Key::OpenBracket, 26);
        linux(Key::CloseBracket, 27);
        linux(Key::Semicolon, 39);
        linux(Key::Quote, 40);
        linux(Key::Backtick, 41);
        linux(Key::Backslash, 43);
        linux(Key::Comma, 51);
        linux(Key::Period, 52);
        linux(Key::Slash, 53);
        linux(Key::F1, 59);
        linux(Key::F10, 68);
        linux(Key::F11, 87);
        linux(Key::F12, 88);
        linux(Key::Home, 102);
        linux(Key::ArrowUp, 103);
        linux(Key::PageUp, 104);
        linux(Key::ArrowLeft, 105);
        linux(Key::ArrowRight, 106);
        linux(Key::End, 107);
        linux(Key::ArrowDown, 108);
        linux(Key::PageDown, 109);
        linux(Key::Delete, 111);
    }

    #[test]
    fn no_two_keys_share_a_code() {
        let keys = [
            Key::A,
            Key::B,
            Key::C,
            Key::D,
            Key::E,
            Key::F,
            Key::G,
            Key::H,
            Key::I,
            Key::J,
            Key::K,
            Key::L,
            Key::M,
            Key::N,
            Key::O,
            Key::P,
            Key::Q,
            Key::R,
            Key::S,
            Key::T,
            Key::U,
            Key::V,
            Key::W,
            Key::X,
            Key::Y,
            Key::Z,
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
            Key::F1,
            Key::F2,
            Key::F3,
            Key::F4,
            Key::F5,
            Key::F6,
            Key::F7,
            Key::F8,
            Key::F9,
            Key::F10,
            Key::F11,
            Key::F12,
            Key::F13,
            Key::F14,
            Key::F15,
            Key::F16,
            Key::F17,
            Key::F18,
            Key::F19,
            Key::F20,
        ];
        let mut codes: Vec<u16> = keys.iter().map(|k| mac_code(*k).unwrap()).collect();
        codes.sort_unstable();
        let n = codes.len();
        codes.dedup();
        assert_eq!(codes.len(), n);
    }
}
