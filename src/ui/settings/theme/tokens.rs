//! The tokens a theme is made of, and the [`Theme`] that holds them.
//!
//! A theme is data, not code. Every widget of Settings asks [`pal`](super::pal) for a color and
//! [`shape`](super::shape) for a radius, an outline or a shadow, and draws itself the same way
//! whatever the theme is. So a new look is a new set of values (see `builtin.rs`, or a JSON file,
//! see `spec.rs`), never another copy of the drawing code.
//!
//! The values then go through the same steps whatever their source (see `registry::resolve`):
//!
//! 1. a theme (built in, or a file that `extends` one),
//! 2. the colors the user picked on top of it (`Config::theme`, see [`super::with_colors`]),
//! 3. a last check that the text stays readable on every surface.
//!
//! What reaches the widgets is the result of step 3.

use super::Palette;
use crate::ui::draw::PieColors;
use eframe::egui::{self, Color32, CornerRadius, Stroke};
use serde::Deserialize;

/// Which color of the [`Palette`] an outline or a shadow takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// The pale hairline between the rows of a panel.
    Divider,
    /// The hairline around a control or a panel.
    Border,
    /// The strong line of the theme: black ink on a light page, cream on a dark one.
    #[default]
    Line,
}

impl Tone {
    /// The color of this tone in `p`.
    pub fn of(self, p: &Palette) -> Color32 {
        match self {
            Tone::Divider => p.divider,
            Tone::Border => p.border,
            Tone::Line => p.line,
        }
    }
}

/// An outline: how wide (0 = none) and in which tone.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct Edge {
    pub width: f32,
    pub tone: Tone,
}

/// What a theme file gets for the fields it leaves out of an outline: no width, in the line tone.
impl Default for Edge {
    fn default() -> Self {
        Self::NONE
    }
}

impl Edge {
    /// No outline at all.
    pub const NONE: Edge = Edge {
        width: 0.0,
        tone: Tone::Line,
    };

    /// An outline of `width` points in `tone`.
    pub const fn new(width: f32, tone: Tone) -> Self {
        Self { width, tone }
    }

    /// Whether anything is drawn.
    pub fn is_visible(self) -> bool {
        self.width > 0.0
    }

    /// The egui stroke of this outline in `p`.
    pub fn stroke(self, p: &Palette) -> Stroke {
        if self.is_visible() {
            Stroke::new(self.width, self.tone.of(p))
        } else {
            Stroke::NONE
        }
    }
}

/// A shadow under a shape. `blur` 0 makes it hard: an exact copy of the shape, moved by `x` and `y`
/// (the look of the website). `alpha` 0 means no shadow.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct DropShadow {
    pub x: i8,
    pub y: i8,
    pub blur: u8,
    /// Opacity, 0 to 255.
    pub alpha: u8,
    /// Cast in the line color of the theme (hard shadows) instead of black (soft ones).
    pub ink: bool,
}

/// What a theme file gets for the fields it leaves out of a shadow: a hard, opaque one, 3 points down
/// and to the right (write `"alpha": 0` for none).
impl Default for DropShadow {
    fn default() -> Self {
        Self::hard(3, 255)
    }
}

impl DropShadow {
    /// No shadow.
    pub const NONE: DropShadow = DropShadow {
        x: 0,
        y: 0,
        blur: 0,
        alpha: 0,
        ink: false,
    };

    /// A soft black shadow, as the sage theme has under what floats.
    pub const fn soft(y: i8, blur: u8, alpha: u8) -> Self {
        Self {
            x: 0,
            y,
            blur,
            alpha,
            ink: false,
        }
    }

    /// A hard shadow in the line color, moved `by` points to the right and down.
    pub const fn hard(by: i8, alpha: u8) -> Self {
        Self {
            x: by,
            y: by,
            blur: 0,
            alpha,
            ink: true,
        }
    }

    /// Whether anything is drawn.
    pub fn is_visible(self) -> bool {
        self.alpha > 0
    }

    /// How far the shadow reaches from the shape, in points: for a window, it must fit its margin.
    pub fn reach(self) -> f32 {
        f32::from(self.x.unsigned_abs().max(self.y.unsigned_abs())) + f32::from(self.blur)
    }

    /// The egui shadow in the palette `p`.
    pub fn shadow(self, p: &Palette) -> egui::Shadow {
        if !self.is_visible() {
            return egui::Shadow::NONE;
        }
        let color = if self.ink {
            let [r, g, b, _] = p.line.to_array();
            Color32::from_rgba_unmultiplied(r, g, b, self.alpha)
        } else {
            Color32::from_black_alpha(self.alpha)
        };
        egui::Shadow {
            offset: [self.x, self.y],
            blur: self.blur,
            spread: 0,
            color,
        }
    }
}

/// The geometry of a theme: radii, outlines and shadows. Sizes (the height of a control, the gaps)
/// are the same in every theme on purpose, so that a theme cannot break a layout.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// Corner radius of buttons, fields and tiles, in points. 18 (half of the control height)
    /// makes them pills.
    pub control_r: u8,
    /// Corner radius of panels and popups.
    pub panel_r: u8,
    /// Corner radius of the window.
    pub window_r: u8,
    /// Outline of buttons, fields, tiles and switches (none: they rest on a soft fill).
    pub control: Edge,
    /// Outline of a raised piece inside a control: the chosen segment, a key cap.
    pub chip: Edge,
    /// Outline of a panel.
    pub panel: Edge,
    /// Outline of what floats: popups, menus and sheets.
    pub popup: Edge,
    /// Outline of the window.
    pub window: Edge,
    /// The line between the sidebar and the page.
    pub side: Edge,
    /// How far the outline of the window is pulled toward the accent color (0 to 1).
    pub window_accent_mix: f32,
    /// Shadow of a button at rest (it goes when the button is pressed).
    pub control_drop: DropShadow,
    /// Shadow of a panel.
    pub panel_drop: DropShadow,
    /// Shadow of what floats. Soft by default.
    pub popup_drop: DropShadow,
    /// Shadow of the window. It is painted in the margin around the window, so [`DropShadow::reach`]
    /// must stay within `WIN_MARGIN`.
    pub window_drop: DropShadow,
    /// Opacity of the wash behind the chosen entry of the sidebar (0 to 1).
    pub nav_wash: f32,
    /// Whether the icon of the chosen entry is drawn in the accent (it is in the text color when the
    /// wash is strong enough that the accent would sink into it).
    pub nav_icon_accent: bool,
}

impl Shape {
    /// The corner radius of a control.
    pub fn control_radius(&self) -> CornerRadius {
        CornerRadius::same(self.control_r)
    }
}

/// The colors a theme suggests when the user picks one of their own (Behavior, Look & feel), as
/// `(name, rgb)`: per appearance (light, dark) for the page, the panels, the sidebar and the text,
/// and one list of accents. The first color of each list is the color the theme comes with.
#[derive(Debug, PartialEq)]
pub struct Swatches {
    pub pages: [&'static [(&'static str, [u8; 3])]; 2],
    pub panels: [&'static [(&'static str, [u8; 3])]; 2],
    pub sidebars: [&'static [(&'static str, [u8; 3])]; 2],
    pub texts: [&'static [(&'static str, [u8; 3])]; 2],
    pub accents: &'static [(&'static str, [u8; 3])],
}

/// A theme, resolved: everything the widgets need to draw one look.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    /// What `Config::ui_theme` stores: `sage`, `genz`, or the name of a file in the themes folder.
    pub id: String,
    /// What the person reads.
    pub name: String,
    pub light: Palette,
    pub dark: Palette,
    pub shape: Shape,
    /// The accent a fresh choice of this theme starts with.
    pub accent: [u8; 3],
    /// The colors of a Solid pie, in the light and the dark appearance.
    pub pie: [PieColors; 2],
    pub swatches: &'static Swatches,
    /// Whether it ships with CursorPie (and so cannot be overwritten by a file).
    pub builtin: bool,
}

impl Theme {
    /// The palette of an appearance, before the colors of the user are laid over it.
    pub fn palette(&self, dark: bool) -> Palette {
        if dark { self.dark } else { self.light }
    }

    /// The pie colors of an appearance.
    pub fn pie_colors(&self, dark: bool) -> PieColors {
        self.pie[usize::from(dark)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::settings::theme::builtin;

    #[test]
    fn no_edge_and_no_shadow_draw_nothing() {
        let p = builtin::sage().light;
        assert_eq!(Edge::NONE.stroke(&p), Stroke::NONE);
        assert!(!Edge::NONE.is_visible());
        assert_eq!(DropShadow::NONE.shadow(&p), egui::Shadow::NONE);
        assert!(!DropShadow::NONE.is_visible());
    }

    #[test]
    fn a_tone_picks_its_color_of_the_palette() {
        let p = builtin::genz().light;
        assert_eq!(Edge::new(2.0, Tone::Line).stroke(&p).color, p.line);
        assert_eq!(Edge::new(1.0, Tone::Border).stroke(&p).color, p.border);
        assert_eq!(Edge::new(1.0, Tone::Divider).stroke(&p).color, p.divider);
    }

    #[test]
    fn a_hard_shadow_is_cast_in_the_line_color_and_a_soft_one_in_black() {
        let p = builtin::genz().light;
        let hard = DropShadow::hard(4, 255).shadow(&p);
        assert_eq!((hard.offset, hard.blur), ([4, 4], 0));
        assert_eq!(hard.color, p.line);
        let soft = DropShadow::soft(8, 28, 38).shadow(&p);
        assert_eq!((soft.offset, soft.blur), ([0, 8], 28));
        assert_eq!(soft.color, Color32::from_black_alpha(38));
    }

    #[test]
    fn the_reach_of_a_shadow_is_its_offset_and_its_blur() {
        assert_eq!(DropShadow::soft(10, 30, 64).reach(), 40.0);
        assert_eq!(DropShadow::hard(6, 255).reach(), 6.0);
        assert_eq!(DropShadow::NONE.reach(), 0.0);
    }
}
