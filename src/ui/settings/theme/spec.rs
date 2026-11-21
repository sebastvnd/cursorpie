//! A theme written as a file: the format people use to make a theme of their own.
//!
//! A file is a *layer over another theme*, not a whole theme: it names the theme it `extends`
//! (sage when it says nothing) and lists only what it changes. Colors are written `"#rrggbb"`
//! (`"#rgb"` and `"#rrggbbaa"` work too). Anything left out is the base theme's.
//!
//! ```json
//! {
//!   "name": "Mint Soda",
//!   "extends": "genz",
//!   "accent": "#2fd27a",
//!   "light": { "bg": "#e1faec", "sidebar": "#d0f2e0", "text": "#0e2a1c" },
//!   "dark":  { "bg": "#0f1a14" },
//!   "shape": { "control_radius": 10, "panel_radius": 14 }
//! }
//! ```
//!
//! The four main colors of an appearance (`bg`, `sidebar`, `card`, `text`) are the same four the user
//! can pick in Settings, and the rest of the palette (controls, hairlines, the secondary text...)
//! follows them, so a theme can be as short as the example above. Every other field of the
//! palette can still be written to set it exactly. Whatever is written, the text is moved just far
//! enough from the surfaces to read on all of them (see [`super::ensure_readable`]).
//!
//! The full list of fields is `docs/user/themes.md`.

use super::tokens::{DropShadow, Edge, Shape, Theme};
use super::{Palette, ensure_readable, with_colors};
use crate::config::ModeColors;
use crate::ui::draw::PieColors;
use eframe::egui::Color32;
use serde::Deserialize;
use serde::de::{Deserializer, Error};

/// A color as written in a theme file: `#rgb`, `#rrggbb`, `#rrggbbaa`, or an `[r, g, b]` array.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hex(pub [u8; 4]);

impl Hex {
    /// The color of a `#rgb`, `#rrggbb` or `#rrggbbaa` text (the `#` is optional).
    pub fn parse(text: &str) -> Option<Hex> {
        let t = text.trim().trim_start_matches('#');
        if !t.is_ascii() {
            return None;
        }
        let digit = |i: usize| u8::from_str_radix(&t[i..=i], 16).ok().map(|d| d * 17);
        let pair = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).ok();
        match t.len() {
            3 => Some(Hex([digit(0)?, digit(1)?, digit(2)?, 255])),
            6 => Some(Hex([pair(0)?, pair(2)?, pair(4)?, 255])),
            8 => Some(Hex([pair(0)?, pair(2)?, pair(4)?, pair(6)?])),
            _ => None,
        }
    }

    fn from_json(v: &serde_json::Value) -> Option<Hex> {
        match v {
            serde_json::Value::String(s) => Self::parse(s),
            serde_json::Value::Array(a) if a.len() == 3 || a.len() == 4 => {
                let n = |i: usize| {
                    a.get(i)
                        .map_or(Some(255), |x| u8::try_from(x.as_u64()?).ok())
                };
                Some(Hex([n(0)?, n(1)?, n(2)?, n(3)?]))
            }
            _ => None,
        }
    }

    /// `[r, g, b]`, the form the config keeps colors in.
    pub fn rgb(self) -> [u8; 3] {
        let [r, g, b, _] = self.0;
        [r, g, b]
    }

    /// As an opaque or translucent egui color.
    pub fn color(self) -> Color32 {
        let [r, g, b, a] = self.0;
        Color32::from_rgba_unmultiplied(r, g, b, a)
    }
}

impl<'de> Deserialize<'de> for Hex {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        Hex::from_json(&v)
            .ok_or_else(|| D::Error::custom(format!("{v} is not a color (write \"#rrggbb\")")))
    }
}

/// The colors of one appearance a file may set. All of them are optional.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PaletteSpec {
    // the four main colors, which the rest follows
    pub bg: Option<Hex>,
    pub sidebar: Option<Hex>,
    pub card: Option<Hex>,
    pub text: Option<Hex>,
    // everything else, to set it exactly
    pub idle: Option<Hex>,
    pub hot: Option<Hex>,
    pub input: Option<Hex>,
    pub border: Option<Hex>,
    pub divider: Option<Hex>,
    pub line: Option<Hex>,
    pub muted: Option<Hex>,
    pub good: Option<Hex>,
    pub bad: Option<Hex>,
    pub scrim: Option<Hex>,
    pub track: Option<Hex>,
    pub keycap: Option<Hex>,
}

impl PaletteSpec {
    /// Whether one of the four main colors is set, so what follows them has to be worked out again.
    fn sets_main(&self) -> bool {
        [self.bg, self.sidebar, self.card, self.text]
            .iter()
            .any(Option::is_some)
    }

    /// `base`, with this spec laid over it.
    fn apply(&self, base: &Theme, dark: bool) -> Palette {
        let main = ModeColors {
            page: self.bg.map(Hex::rgb),
            panel: self.card.map(Hex::rgb),
            sidebar: self.sidebar.map(Hex::rgb),
            text: self.text.map(Hex::rgb),
        };
        // the user's own colors go through the same step: the rest of the palette follows
        let mut p = with_colors(base, dark, &main);
        let set = |slot: &mut Color32, v: Option<Hex>| {
            if let Some(h) = v {
                *slot = h.color();
            }
        };
        set(&mut p.idle, self.idle);
        set(&mut p.hot, self.hot);
        set(&mut p.input, self.input);
        set(&mut p.border, self.border);
        set(&mut p.divider, self.divider);
        set(&mut p.line, self.line);
        set(&mut p.muted, self.muted);
        set(&mut p.good, self.good);
        set(&mut p.bad, self.bad);
        set(&mut p.scrim, self.scrim);
        set(&mut p.track, self.track);
        set(&mut p.keycap, self.keycap);
        ensure_readable(&mut p);
        p
    }
}

/// The geometry a file may set. All of it is optional.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ShapeSpec {
    pub control_radius: Option<u8>,
    pub panel_radius: Option<u8>,
    pub window_radius: Option<u8>,
    pub control_edge: Option<Edge>,
    pub chip_edge: Option<Edge>,
    pub panel_edge: Option<Edge>,
    pub popup_edge: Option<Edge>,
    pub window_edge: Option<Edge>,
    pub side_edge: Option<Edge>,
    pub window_accent_mix: Option<f32>,
    pub control_shadow: Option<DropShadow>,
    pub panel_shadow: Option<DropShadow>,
    pub popup_shadow: Option<DropShadow>,
    pub window_shadow: Option<DropShadow>,
    pub nav_wash: Option<f32>,
    pub nav_icon_accent: Option<bool>,
}

/// The widest radius that still makes sense: a control is 36 points high.
const MAX_RADIUS: u8 = 36;
/// The widest outline.
const MAX_EDGE: f32 = 6.0;

impl ShapeSpec {
    /// `base`, with this spec laid over it, held within what the layout can take: radii and
    /// outlines are capped, and the shadow of the window stays within the margin it is painted in.
    fn apply(&self, base: Shape) -> Shape {
        let mut s = base;
        let edge = |slot: &mut Edge, v: Option<Edge>| {
            if let Some(e) = v {
                *slot = Edge {
                    width: if e.width.is_finite() {
                        e.width.clamp(0.0, MAX_EDGE)
                    } else {
                        0.0
                    },
                    ..e
                };
            }
        };
        s.control_r = self.control_radius.unwrap_or(s.control_r).min(MAX_RADIUS);
        s.panel_r = self.panel_radius.unwrap_or(s.panel_r).min(MAX_RADIUS);
        s.window_r = self.window_radius.unwrap_or(s.window_r).min(MAX_RADIUS);
        edge(&mut s.control, self.control_edge);
        edge(&mut s.chip, self.chip_edge);
        edge(&mut s.panel, self.panel_edge);
        edge(&mut s.popup, self.popup_edge);
        edge(&mut s.window, self.window_edge);
        edge(&mut s.side, self.side_edge);
        let unit = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                0.0
            }
        };
        s.window_accent_mix = self.window_accent_mix.map_or(s.window_accent_mix, unit);
        s.nav_wash = self.nav_wash.map_or(s.nav_wash, unit);
        s.nav_icon_accent = self.nav_icon_accent.unwrap_or(s.nav_icon_accent);
        s.control_drop = self.control_shadow.unwrap_or(s.control_drop);
        s.panel_drop = self.panel_shadow.unwrap_or(s.panel_drop);
        s.popup_drop = self.popup_shadow.unwrap_or(s.popup_drop);
        s.window_drop = self.window_shadow.unwrap_or(s.window_drop);
        // the window is transparent, and its shadow is painted in a margin of `WIN_MARGIN`
        let room = crate::ui::app::WIN_MARGIN;
        if s.window_drop.reach() > room {
            s.window_drop = DropShadow::NONE;
        }
        s
    }
}

/// The colors of the Solid pie in one appearance a file may set.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PieModeSpec {
    pub slice: Option<Hex>,
    pub ghost: Option<Hex>,
    pub ink: Option<Hex>,
    pub core: Option<Hex>,
}

/// The colors of the Solid pie a file may set, per appearance.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PieSpec {
    pub light: PieModeSpec,
    pub dark: PieModeSpec,
}

impl PieModeSpec {
    /// `base`, with this spec laid over it.
    fn apply(&self, base: PieColors) -> PieColors {
        PieColors {
            slice: self.slice.map_or(base.slice, Hex::rgb),
            ghost: self.ghost.map_or(base.ghost, Hex::rgb),
            ink: self.ink.map_or(base.ink, Hex::color),
            core: self.core.map_or(base.core, Hex::color),
        }
    }
}

/// The pie colors that follow a palette: slices like the panels, the "+" like the sidebar.
fn pie_of(p: &Palette) -> PieColors {
    let rgb = |c: Color32| [c.r(), c.g(), c.b()];
    PieColors {
        slice: rgb(p.card),
        ghost: rgb(p.sidebar),
        ink: p.text,
        core: p.card,
    }
}

/// A theme file.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ThemeFile {
    /// What the person reads. The name of the file when left out.
    pub name: Option<String>,
    /// The theme this one is a layer over: `sage`, `genz` or another file. Sage when left out.
    pub extends: Option<String>,
    /// The accent a fresh choice of this theme starts with.
    pub accent: Option<Hex>,
    pub light: PaletteSpec,
    pub dark: PaletteSpec,
    pub shape: ShapeSpec,
    pub pie: PieSpec,
}

impl ThemeFile {
    /// Reads a file's text. The error says what is wrong and where.
    pub fn parse(text: &str) -> Result<ThemeFile, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    /// The theme this file makes on top of `base`.
    pub fn resolve(&self, id: &str, base: &Theme) -> Theme {
        let name = self
            .name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .unwrap_or(id);
        let (light, dark) = (self.light.apply(base, false), self.dark.apply(base, true));
        // a pie follows the palette it sits in once a main color of it is chosen
        let pie_mode = |set: &PaletteSpec, p: &Palette, base_pie: PieColors, spec: &PieModeSpec| {
            spec.apply(if set.sets_main() { pie_of(p) } else { base_pie })
        };
        Theme {
            id: id.to_owned(),
            name: name.chars().take(40).collect(),
            light,
            dark,
            shape: self.shape.apply(base.shape),
            accent: self.accent.map_or(base.accent, Hex::rgb),
            pie: [
                pie_mode(&self.light, &light, base.pie[0], &self.pie.light),
                pie_mode(&self.dark, &dark, base.pie[1], &self.pie.dark),
            ],
            swatches: base.swatches,
            builtin: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw::contrast;
    use crate::ui::settings::theme::builtin;

    fn file(json: &str) -> ThemeFile {
        ThemeFile::parse(json).unwrap()
    }

    #[test]
    fn colors_are_written_in_hex_or_as_an_array() {
        assert_eq!(Hex::parse("#ff5ea8"), Some(Hex([255, 94, 168, 255])));
        assert_eq!(Hex::parse("FF5EA8"), Some(Hex([255, 94, 168, 255])));
        assert_eq!(Hex::parse("#f80"), Some(Hex([255, 136, 0, 255])));
        assert_eq!(Hex::parse("#00000080"), Some(Hex([0, 0, 0, 128])));
        for bad in [
            "",
            "#",
            "#12",
            "#12345",
            "#gg0000",
            "red",
            "#ff5ea8ff00",
            "é12345",
        ] {
            assert_eq!(Hex::parse(bad), None, "{bad:?}");
        }
        let f = file(r##"{"accent":[1,2,3],"light":{"bg":"#102030"}}"##);
        assert_eq!(f.accent.unwrap().rgb(), [1, 2, 3]);
        assert_eq!(f.light.bg.unwrap().rgb(), [16, 32, 48]);
    }

    #[test]
    fn a_wrong_color_is_reported_with_what_was_written() {
        let e = ThemeFile::parse(r#"{"accent":"banana"}"#).unwrap_err();
        assert!(e.contains("banana") && e.contains("color"), "{e}");
        assert!(ThemeFile::parse(r#"{"accent":[1,2,300]}"#).is_err());
        assert!(ThemeFile::parse("not json").is_err());
    }

    #[test]
    fn an_empty_file_is_its_base_theme() {
        let base = builtin::genz();
        let t = file("{}").resolve("mine", &base);
        assert_eq!((t.id.as_str(), t.name.as_str()), ("mine", "mine"));
        assert_eq!(
            (t.light, t.dark, t.shape),
            (base.light, base.dark, base.shape)
        );
        assert_eq!((t.accent, t.pie), (base.accent, base.pie));
        assert!(!t.builtin);
    }

    #[test]
    fn unknown_fields_are_ignored_so_a_newer_file_still_loads() {
        let t = file(r##"{"future":1,"light":{"sparkle":"#fff"}}"##).resolve("x", &builtin::sage());
        assert_eq!(t.light, builtin::sage().light);
    }

    #[test]
    fn the_main_colors_make_the_rest_of_the_palette_follow() {
        let base = builtin::genz();
        let t = file(r##"{"light":{"bg":"#ffe3f1"}}"##).resolve("pink", &base);
        assert_eq!(t.light.bg.to_array()[..3], [255, 227, 241]);
        assert_ne!(t.light.card, base.light.card, "the panel follows the page");
        assert_ne!(t.light.border, base.light.border, "and so do the hairlines");
        assert_eq!(t.dark, base.dark, "the other appearance is left alone");
    }

    #[test]
    fn a_field_that_is_written_is_used_as_it_is() {
        let t = file(r##"{"light":{"bg":"#ffe3f1","hot":"#123456","line":"#ff0000"}}"##)
            .resolve("x", &builtin::genz());
        assert_eq!(t.light.hot, Color32::from_rgb(0x12, 0x34, 0x56));
        assert_eq!(t.light.line, Color32::from_rgb(255, 0, 0));
    }

    #[test]
    fn the_text_stays_readable_whatever_the_file_says() {
        // white text on a white page, and a muted color that is the page
        let t = file(
            r##"{"light":{"bg":"#ffffff","card":"#ffffff","sidebar":"#ffffff",
                "text":"#ffffff","muted":"#ffffff","idle":"#fafafa","hot":"#f0f0f0"}}"##,
        )
        .resolve("x", &builtin::sage());
        let p = t.light;
        for s in [p.bg, p.sidebar, p.card, p.idle, p.hot, p.input] {
            assert!(contrast(p.text, s) >= 7.0 - 0.01, "{s:?}");
            assert!(contrast(p.muted, s) >= 4.5 - 0.01, "{s:?}");
        }
    }

    #[test]
    fn a_pie_follows_the_palette_it_sits_in_unless_it_is_set() {
        let base = builtin::sage();
        let t = file(r##"{"light":{"card":"#e0f0ff"}}"##).resolve("x", &base);
        assert_eq!(t.pie[0].slice, [224, 240, 255]);
        assert_eq!(t.pie[1], base.pie[1], "the dark pie did not change");
        let t = file(r##"{"light":{"card":"#e0f0ff"},"pie":{"light":{"slice":"#010203"}}}"##)
            .resolve("x", &base);
        assert_eq!(t.pie[0].slice, [1, 2, 3]);
    }

    #[test]
    fn the_shape_is_held_within_what_the_layout_can_take() {
        let t = file(
            r#"{"shape":{"control_radius":200,"panel_edge":{"width":99},
                "window_shadow":{"x":0,"y":60,"blur":60,"alpha":255},
                "nav_wash":7}}"#,
        )
        .resolve("x", &builtin::sage());
        assert_eq!(t.shape.control_r, MAX_RADIUS);
        assert_eq!(t.shape.panel.width, MAX_EDGE);
        assert!(!t.shape.window_drop.is_visible(), "it would be cut off");
        assert_eq!(t.shape.nav_wash, 1.0);
    }

    #[test]
    fn the_name_and_the_accent_can_be_set_and_the_swatches_come_from_the_base() {
        let base = builtin::genz();
        let t = file(r##"{"name":"  Bubblegum ","accent":"#ff5ea8"}"##).resolve("bg", &base);
        assert_eq!((t.name.as_str(), t.accent), ("Bubblegum", [255, 94, 168]));
        assert!(std::ptr::eq(t.swatches, base.swatches));
        let long = format!(r#"{{"name":"{}"}}"#, "x".repeat(100));
        assert_eq!(file(&long).resolve("a", &base).name.chars().count(), 40);
    }

    #[test]
    fn an_edge_and_a_shadow_can_be_written_in_part() {
        let t = file(
            r#"{"shape":{"control_edge":{"width":3},"panel_shadow":{"x":2,"y":2,"alpha":200,"ink":true}}}"#,
        )
        .resolve("x", &builtin::sage());
        assert!(t.shape.control.is_visible());
        assert_eq!(t.shape.control.tone, super::super::Tone::Line);
        assert_eq!(t.shape.panel_drop.blur, 0);
        assert!(t.shape.panel_drop.is_visible());
    }
}
