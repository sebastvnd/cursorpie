//! Look of the settings window: palette, geometry and the egui style applied to it.
//!
//! The look is modern flat, in the spirit of the Apple system apps, in a light and a dark palette
//! (read through [`pal`]). The colors are those of the website (`assets/site.css`): sage-green
//! neutrals, a cream page and paper-white panels, and a sage accent by default:
//!
//! - a light window with large rounded corners ([`card_radius`]); a sidebar that names the
//!   sections, and paper panels (`card`) on a cream page (`bg`), separated by space and
//!   one pale hairline, not by frames and not by shadows;
//! - generous room: one spacing unit ([`GAP`]), a page padding of its own, and one control height
//!   ([`CONTROL_H`]) for every button, field and list row;
//! - round shapes all over: controls use [`control_r`], panels [`panel_r`], badges and chips are
//!   pills, switches and sliders are circles on a rail;
//! - Inter for text and one icon set for every picture (see [`fonts`](crate::ui::fonts) and
//!   [`symbols`](crate::ui::symbols)); there are no capitals-in-monospace labels and no letters
//!   standing in for icons;
//! - one accent color, chosen by the user, for whatever is selected or primary, so it always
//!   means "this". Everything else is neutral.
//!
//! That is the **sage** theme, the look Settings always had. There is a second one, **gen z** (the
//! look of the website today: thick ink outlines and hard shadows), and people can make their own.
//!
//! # Themes
//!
//! A theme is data (see [`Theme`]): two palettes, a [`Shape`] (radii, outlines and shadows) and a few
//! suggestions. The widgets never know which theme is on: they take a color from [`pal`] and a radius,
//! an outline or a shadow from [`shape`] (through [`control_r`], `paint_control`, `card`...), so
//! a theme costs no drawing code. What the widgets read is what is left after three layers:
//!
//! 1. the theme (`builtin.rs`, or a file of the themes folder that extends one, `spec.rs`),
//! 2. the colors the user picked on top of it (`Config::theme`, see [`with_colors`]),
//! 3. a check that the text stays readable on every surface ([`ensure_readable`]).
//!
//! Change the values of a theme to re-theme. Other files must not hard-code a color or a radius.

mod builtin;
pub mod registry;
pub mod spec;
mod tokens;

pub use builtin::{SAGE_PIE, all as builtin_themes};
pub use tokens::{DropShadow, Edge, Shape, Swatches, Theme, Tone};

use crate::config::{Appearance, ModeColors, ThemeColors};
use crate::ui::draw::{PieColors, contrast, luminance};
use eframe::egui::{self, Color32, CornerRadius, Painter, Rect, Stroke, Vec2};
use std::sync::RwLock;

pub use crate::ui::draw::on_accent;

/// The colors of one appearance. Read them through [`pal`], never copy a value.
///
/// `contrast` pairs are checked by the tests at the bottom of this file: primary text keeps 7:1
/// on every surface, secondary text 4.5:1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Whether this is the dark appearance.
    pub dark: bool,
    /// The page: a very light grey (or near black), so panels lift off it without a frame.
    pub bg: Color32,
    /// The sidebar: a step away from the page.
    pub sidebar: Color32,
    /// Panels and popups.
    pub card: Color32,
    /// Controls at rest (buttons, tiles, chips).
    pub idle: Color32,
    /// Controls under the pointer.
    pub hot: Color32,
    /// Text fields: a hair away from the panel they sit in.
    pub input: Color32,
    /// Hairlines and borders.
    pub border: Color32,
    /// Hairlines inside a panel, between its rows: paler than `border`.
    pub divider: Color32,
    /// The strong line of the theme, for the outlines and hard shadows that have one (see
    /// [`Tone::Line`]): ink on a light page, cream on a dark one.
    pub line: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text.
    pub muted: Color32,
    /// "OK" status.
    pub good: Color32,
    /// Warning / error status.
    pub bad: Color32,
    /// The dimmed layer behind a sheet.
    pub scrim: Color32,
    /// The rail of a slider and a switch that is off.
    pub track: Color32,
    /// The face of a key in a shortcut.
    pub keycap: Color32,
}

/// What [`pal`] and [`shape`] hand out: the palettes and the geometry of the theme in use, with the
/// colors of the user in the palettes, and what they were made from, so a frame that changes nothing
/// costs nothing.
struct Live {
    light: Palette,
    dark: Palette,
    shape: Shape,
    pie: [PieColors; 2],
    swatches: &'static Swatches,
    made_from: Key,
}

/// What a [`Live`] was made from.
#[derive(PartialEq)]
struct Key {
    theme: String,
    colors: ThemeColors,
    /// The generation of the list of themes (see [`registry::generation`]).
    generation: u64,
}

static LIVE: RwLock<Live> = RwLock::new(Live {
    light: builtin::SAGE_LIGHT,
    dark: builtin::SAGE_DARK,
    shape: builtin::SAGE_SHAPE,
    pie: builtin::SAGE_PIE,
    swatches: &builtin::SAGE_SWATCHES,
    made_from: Key {
        theme: String::new(),
        colors: ThemeColors {
            light: ModeColors {
                page: None,
                panel: None,
                sidebar: None,
                text: None,
            },
            dark: ModeColors {
                page: None,
                panel: None,
                sidebar: None,
                text: None,
            },
        },
        generation: u64::MAX,
    },
});

/// The palette in use. Settings and the pie call this every frame, so the appearance can change
/// while a window is open. It is a copy: the colors of the user are in it (see [`apply_appearance`]).
pub fn pal() -> Palette {
    let live = LIVE.read().unwrap_or_else(|e| e.into_inner());
    if crate::ui::draw::dark() {
        live.dark
    } else {
        live.light
    }
}

/// The geometry of the theme in use: radii, outlines and shadows. A copy, like [`pal`].
pub fn shape() -> Shape {
    LIVE.read().unwrap_or_else(|e| e.into_inner()).shape
}

/// The colors of a Solid pie in an appearance, in the theme in use.
pub fn pie_colors_of(dark: bool) -> PieColors {
    LIVE.read().unwrap_or_else(|e| e.into_inner()).pie[usize::from(dark)]
}

/// The colors the theme in use suggests for the page, the panels, the sidebar, the text and the accent.
pub fn swatches() -> &'static Swatches {
    LIVE.read().unwrap_or_else(|e| e.into_inner()).swatches
}

/// Whether `appearance` is dark right now: `Dark`, or `System` while the system is dark.
pub fn resolve_dark(appearance: Appearance, system: Option<egui::Theme>) -> bool {
    match appearance {
        Appearance::Dark => true,
        Appearance::Light => false,
        Appearance::System => system == Some(egui::Theme::Dark),
    }
}

/// The palette of an appearance of `theme` with the colors of the user in it. What depends on a
/// color follows it: the cards, the controls, the hairlines and the rail are a step away from the
/// page and the panel, the secondary text is the text a step toward the page. The text is then
/// moved just far enough from the surfaces to read on all of them (7:1, and 4.5:1 for the secondary
/// text), whatever was chosen. A theme file goes through here too, for its main colors.
pub fn with_colors(theme: &Theme, dark: bool, t: &ModeColors) -> Palette {
    let base = theme.palette(dark);
    if t.is_default() {
        return base;
    }
    let rgb = |[r, g, b]: [u8; 3]| Color32::from_rgb(r, g, b);
    let mut p = base;
    // away from a surface: toward black on a light page, toward white on a dark one (the chosen
    // page decides, not the appearance: a light appearance may have a dark page, and the reverse)
    let page_is_dark = luminance(t.page.map_or(base.bg, rgb)) < 0.18;
    let shade = if page_is_dark {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    if let Some(c) = t.page {
        p.bg = rgb(c);
        p.card =
            p.bg.lerp_to_gamma(Color32::WHITE, if page_is_dark { 0.07 } else { 0.6 });
        p.sidebar =
            p.bg.lerp_to_gamma(shade, if page_is_dark { 0.04 } else { 0.035 });
    }
    if let Some(c) = t.panel {
        p.card = rgb(c);
    }
    if let Some(c) = t.sidebar {
        p.sidebar = rgb(c);
    }
    if t.page.is_some() || t.panel.is_some() || t.sidebar.is_some() {
        p.idle = p.card.lerp_to_gamma(shade, 0.045);
        p.hot = p.card.lerp_to_gamma(shade, 0.1);
        p.input = p.bg.lerp_to_gamma(shade, 0.015);
        p.border = p.card.lerp_to_gamma(shade, 0.12);
        p.divider = p.card.lerp_to_gamma(shade, 0.07);
        p.track = p.card.lerp_to_gamma(shade, 0.18);
        p.keycap = if page_is_dark {
            p.card.lerp_to_gamma(Color32::WHITE, 0.15)
        } else {
            p.card
        };
    }
    if let Some(c) = t.text {
        p.text = rgb(c);
        p.muted = p.text.lerp_to_gamma(p.bg, 0.3);
    } else if t.page.is_some() || t.panel.is_some() || t.sidebar.is_some() {
        // the text of the appearance may not carry on a page of another lightness
        let other = theme.palette(page_is_dark);
        p.text = other.text;
        p.muted = other.muted;
    }
    // the strong line follows the text where the text or the lightness of the page changed, so that
    // it is not ink on a page that turned dark; otherwise the theme keeps its own line
    if t.text.is_some() || page_is_dark != base.dark {
        p.line = p.text;
    }
    ensure_readable(&mut p);
    p
}

/// Moves the text and the secondary text of `p` just far enough from the surfaces to read on all of
/// them (7:1 and 4.5:1). A color that already does is left as it is.
pub fn ensure_readable(p: &mut Palette) {
    let surfaces = [p.bg, p.sidebar, p.card, p.idle, p.hot, p.input];
    // black or white, whichever reads better on the worst of the surfaces (on a mid-tone page the
    // lightness of the interface does not say which)
    let worst = |c: Color32| {
        surfaces
            .iter()
            .map(|s| contrast(c, *s))
            .fold(f32::MAX, f32::min)
    };
    let extreme = if worst(Color32::WHITE) >= worst(Color32::BLACK) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    p.text = legible_on(p.text, extreme, &surfaces, 7.0);
    p.muted = legible_on(p.muted, p.text, &surfaces, 4.5);
}

/// Picks the palette for this frame from the configured appearance, the theme (`ui_theme`, an id
/// of [`registry`]) and the colors of the user.
pub fn apply_appearance(
    ctx: &egui::Context,
    appearance: Appearance,
    ui_theme: &str,
    colors: &ThemeColors,
) {
    crate::ui::draw::set_dark(resolve_dark(appearance, ctx.system_theme()));
    registry::ensure_loaded();
    let generation = registry::generation();
    let fresh = LIVE
        .read()
        .map(|l| {
            let k = &l.made_from;
            k.theme != ui_theme || k.colors != *colors || k.generation != generation
        })
        .unwrap_or(true);
    if fresh {
        let theme = registry::find(ui_theme);
        let mut live = LIVE.write().unwrap_or_else(|e| e.into_inner());
        // the theme decides the base, the colors of the user are laid over it
        live.light = with_colors(&theme, false, &colors.light);
        live.dark = with_colors(&theme, true, &colors.dark);
        live.shape = theme.shape;
        live.pie = theme.pie;
        live.swatches = theme.swatches;
        live.made_from = Key {
            theme: ui_theme.to_owned(),
            colors: *colors,
            generation,
        };
    }
}

/// `color`, moved toward `target` (the text color) just far enough that it shows up with at least
/// `min`:1 contrast on every one of `surfaces`. A color that already does is returned as it is.
/// It keeps a mark drawn in the user's accent, such as the logo, visible on a dark sidebar, where
/// a sage deep enough to carry white text would sink into the panel.
pub fn legible_on(color: Color32, target: Color32, surfaces: &[Color32], min: f32) -> Color32 {
    for step in 0..=20 {
        let c = color.lerp_to_gamma(target, step as f32 / 20.0);
        if surfaces.iter().all(|s| contrast(c, *s) >= min) {
            return c;
        }
    }
    target
}

/// The accent as a color for text and thin marks (links, the version badge): lightened or darkened
/// just enough to read at 4.5:1 on the page, the sidebar, the panels and the controls. The accent
/// itself is chosen to carry white text on a fill, which a dark panel swallows as text.
pub fn accent_text(accent: Color32) -> Color32 {
    let p = pal();
    legible_on(accent, p.text, &[p.bg, p.sidebar, p.card, p.idle], 4.5)
}

// Geometry
// The radii, the outlines and the shadows belong to the theme (see [`Shape`]); the sizes below are the
// same in every theme, so that no theme can break a layout.
/// Corner radius of buttons, fields and tiles, in points.
pub fn control_r_pt() -> u8 {
    shape().control_r
}
/// Corner radius of buttons, fields and tiles.
pub fn control_r() -> CornerRadius {
    CornerRadius::same(control_r_pt())
}
/// Corner radius of panels, in points.
pub fn panel_r_pt() -> u8 {
    shape().panel_r
}
/// Corner radius of panels and popups.
pub fn panel_r() -> CornerRadius {
    CornerRadius::same(panel_r_pt())
}
/// Corner radius that turns any control into a pill. egui clamps it to half the height.
pub const PILL: CornerRadius = CornerRadius::same(255);
/// Corner radius of the window itself, in points.
pub fn card_radius_pt() -> u8 {
    shape().window_r
}
/// Corner radius of the window.
pub fn card_radius() -> CornerRadius {
    CornerRadius::same(card_radius_pt())
}
/// Space between controls, in points.
pub const GAP: f32 = 12.0;
/// Space between two panels, in points.
pub const SECTION_GAP: f32 = 24.0;
/// Height of every button, chip, input and list row, in points.
pub const CONTROL_H: f32 = 36.0;
/// Space between the edge of a panel and its content, in points.
pub const PANEL_PAD: i8 = 18;

/// The outline of buttons, fields and tiles in the theme in use (none in sage).
pub fn control_stroke() -> Stroke {
    shape().control.stroke(&pal())
}

/// The outline of a raised piece inside a control: the chosen segment, a key cap.
pub fn chip_stroke() -> Stroke {
    shape().chip.stroke(&pal())
}

/// The outline of a round knob: the outline of a control, or, where controls have none, a faint
/// edge that lifts a white knob off its rail.
pub fn knob_stroke() -> Stroke {
    let s = control_stroke();
    if s.width > 0.0 {
        s
    } else {
        Stroke::new(1.0, wash(Color32::BLACK, 0.08))
    }
}

/// Paints the body of a control the way the theme draws one: its shadow (while `lifted`: a pressed
/// button has none), the `fill`, and the outline. Every control that rests on a fill goes through
/// here, so that a theme that outlines its controls outlines all of them.
pub(in crate::ui::settings) fn paint_control(
    painter: &Painter,
    rect: Rect,
    radius: CornerRadius,
    fill: Color32,
    lifted: bool,
) {
    let (s, p) = (shape(), pal());
    if lifted && s.control_drop.is_visible() {
        painter.add(s.control_drop.shadow(&p).as_shape(rect, radius));
    }
    painter.rect_filled(rect, radius, fill);
    let stroke = s.control.stroke(&p);
    if stroke.width > 0.0 {
        painter.rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
    }
}

/// A panel: white, rounded, no border to speak of in sage; outlined with a hard shadow in gen z.
/// Rows inside it add their own vertical room, so the frame only pads the sides.
pub(in crate::ui::settings) fn card() -> egui::Frame {
    let (s, p) = (shape(), pal());
    egui::Frame::new()
        .fill(p.card)
        .corner_radius(panel_r())
        .inner_margin(egui::Margin::symmetric(PANEL_PAD, 6))
        .stroke(s.panel.stroke(&p))
        .shadow(s.panel_drop.shadow(&p))
}

/// The frame of a popover or a sheet: a panel that floats, with the shadow of the theme.
pub(in crate::ui::settings) fn popup_frame() -> egui::Frame {
    let (s, p) = (shape(), pal());
    egui::Frame::new()
        .fill(p.card)
        .corner_radius(panel_r())
        .inner_margin(egui::Margin::same(PANEL_PAD))
        .stroke(s.popup.stroke(&p))
        .shadow(popup_shadow())
}

/// The shadow of anything that floats above the window.
pub(in crate::ui::settings) fn popup_shadow() -> egui::Shadow {
    shape().popup_drop.shadow(&pal())
}

/// The color of the window outline: the color of the theme for it, pulled a little toward `accent`
/// (as far as the theme says), so the window reads as focused without a loud frame.
pub fn window_outline_color(accent: Color32) -> Color32 {
    let s = shape();
    s.window
        .tone
        .of(&pal())
        .lerp_to_gamma(accent, s.window_accent_mix)
}

/// Draws the outline of the window just inside `rect`. It is an ordinary rounded rectangle
/// stroke, which egui anti-aliases; a hand-made mesh would have stair-stepped edges and could not
/// follow rounded corners.
pub(in crate::ui::settings) fn window_outline(
    painter: &egui::Painter,
    rect: Rect,
    accent: Color32,
) {
    let width = shape().window.width;
    if width > 0.0 {
        painter.rect_stroke(
            rect,
            card_radius(),
            Stroke::new(width, window_outline_color(accent)),
            egui::StrokeKind::Inside,
        );
    }
}

/// How far a panel with a hard shadow reaches past the edge of the area it is laid out in: its
/// shadow and its outline (0 for a theme without a panel shadow, sage). Areas that clip their content
/// let this much through, or leave this much room.
pub(in crate::ui::settings) fn panel_bleed() -> f32 {
    let s = shape();
    if s.panel_drop.is_visible() {
        (s.panel_drop.reach() + s.panel.width).min(f32::from(PANEL_PAD))
    } else {
        0.0
    }
}

/// The line between the sidebar and the page.
pub(in crate::ui::settings) fn side_stroke() -> Stroke {
    shape().side.stroke(&pal())
}

/// The shadow under the window. The window is transparent and larger than the card by
/// `WIN_MARGIN` on every side, which is where the shadow is painted, so offset plus blur must stay
/// inside that margin (a theme file whose shadow would not is held back, see `spec.rs`).
pub(in crate::ui::settings) fn window_shadow() -> egui::Shadow {
    shape().window_drop.shadow(&pal())
}

/// A color with `alpha` (0 to 1) of its own strength, for the tinted background of a selected row.
pub fn wash(color: Color32, alpha: f32) -> Color32 {
    let [r, g, b, _] = color.to_array();
    Color32::from_rgba_unmultiplied(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// Applies the Settings style (always light, whatever the system uses) with `accent` as highlight.
pub(in crate::ui::settings) fn style_settings(ui: &mut egui::Ui, accent: Color32) {
    // force the light theme so the window never follows the system's dark mode
    *ui.visuals_mut() = egui::Visuals::light();
    let v = ui.visuals_mut();
    v.panel_fill = pal().bg;
    v.window_fill = pal().card;
    v.window_stroke = shape().popup.stroke(&pal());
    v.extreme_bg_color = pal().input;
    v.faint_bg_color = pal().idle;
    v.override_text_color = None;
    v.selection.bg_fill = wash(accent, 0.28);
    v.selection.stroke = Stroke::new(1.5, accent);
    v.hyperlink_color = accent_text(accent);
    // popups (combo boxes, menus) float above the window: they keep the shadow of the theme
    v.window_corner_radius = panel_r();
    v.menu_corner_radius = control_r();
    v.window_shadow = popup_shadow();
    v.popup_shadow = popup_shadow();

    let w = &mut v.widgets;
    // no hover growth: a growing box looks like a layout shift
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = control_r();
        state.expansion = 0.0;
    }
    // separators and panel outlines; labels and button text use the primary text color
    w.noninteractive.bg_stroke = Stroke::new(1.0, pal().divider);
    w.noninteractive.fg_stroke = Stroke::new(1.0, pal().text);
    w.noninteractive.bg_fill = pal().card;
    w.noninteractive.weak_bg_fill = pal().card;
    // at rest: a soft grey fill (with the outline of the theme, if it has one); under the pointer: a
    // step darker; pressed: the accent
    let outline = control_stroke();
    w.inactive.fg_stroke = Stroke::new(1.0, pal().text);
    w.inactive.bg_fill = pal().idle;
    w.inactive.weak_bg_fill = pal().idle;
    w.inactive.bg_stroke = outline;
    w.hovered.fg_stroke = Stroke::new(1.0, pal().text);
    w.hovered.bg_fill = pal().hot;
    w.hovered.weak_bg_fill = pal().hot;
    w.hovered.bg_stroke = outline;
    w.active.fg_stroke = Stroke::new(1.0, on_accent(accent));
    w.active.bg_fill = accent;
    w.active.weak_bg_fill = accent;
    w.active.bg_stroke = outline;
    w.open.fg_stroke = Stroke::new(1.0, pal().text);
    w.open.bg_fill = pal().hot;
    w.open.weak_bg_fill = pal().hot;
    w.open.bg_stroke = outline;

    let sp = ui.spacing_mut();
    sp.item_spacing = Vec2::splat(GAP);
    sp.button_padding = Vec2::new(14.0, 7.0);
    sp.menu_margin = egui::Margin::same(8);
    sp.combo_width = 160.0;
    // one height for buttons, text fields and combo boxes
    sp.interact_size.y = CONTROL_H;
    sp.scroll = egui::style::ScrollStyle::floating();

    let text = &mut ui.style_mut().text_styles;
    let size = |s: f32, family: egui::FontFamily| egui::FontId::new(s, family);
    text.insert(
        egui::TextStyle::Body,
        size(14.0, egui::FontFamily::Proportional),
    );
    text.insert(
        egui::TextStyle::Button,
        size(14.0, egui::FontFamily::Proportional),
    );
    text.insert(
        egui::TextStyle::Small,
        size(12.0, egui::FontFamily::Proportional),
    );
    text.insert(
        egui::TextStyle::Heading,
        size(22.0, crate::ui::fonts::semibold()),
    );
    text.insert(
        egui::TextStyle::Monospace,
        size(13.0, egui::FontFamily::Monospace),
    );

    // Popups, menus and tooltips are drawn by the context, not by this `ui`: give them the same
    // style, or they come out in egui's own dark one.
    let style = ui.style().clone();
    for theme in [egui::Theme::Light, egui::Theme::Dark] {
        ui.ctx().set_style_of(theme, style.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw::INK;

    /// The theme and the appearance of every built-in palette, by name.
    fn palettes() -> Vec<(String, Palette)> {
        builtin::all()
            .into_iter()
            .flat_map(|t| {
                [(false, "light"), (true, "dark")]
                    .map(|(dark, look)| (format!("{} {look}", t.id), t.palette(dark)))
            })
            .collect()
    }

    /// The surfaces text sits on, by name.
    fn surfaces(p: &Palette) -> [(&'static str, Color32); 6] {
        [
            ("bg", p.bg),
            ("sidebar", p.sidebar),
            ("card", p.card),
            ("idle", p.idle),
            ("hot", p.hot),
            ("input", p.input),
        ]
    }

    #[test]
    fn no_chosen_color_leaves_the_appearance_as_it_is() {
        for theme in builtin::all() {
            for dark in [false, true] {
                let base = theme.palette(dark);
                let p = with_colors(&theme, dark, &ModeColors::default());
                assert_eq!(p, base, "{}", theme.id);
            }
        }
    }

    #[test]
    fn a_chosen_page_color_carries_readable_text_whatever_it_is() {
        let pages = [
            [0, 0, 0],
            [255, 255, 255],
            [255, 255, 0],
            [128, 128, 128],
            [10, 10, 80],
            [200, 30, 30],
        ];
        for theme in builtin::all() {
            for dark in [false, true] {
                for page in pages {
                    for text in [None, Some([120, 120, 120]), Some(page)] {
                        let t = ModeColors {
                            page: Some(page),
                            text,
                            ..ModeColors::default()
                        };
                        let p = with_colors(&theme, dark, &t);
                        assert_eq!(p.dark, dark, "the appearance decides it, not the page");
                        let surfaces = [p.bg, p.sidebar, p.card, p.idle, p.hot, p.input];
                        // a mid tone cannot carry 7:1 in either black or white: the best there is,
                        // for the worst surface
                        let worst = |c: Color32| {
                            surfaces
                                .iter()
                                .map(|s| contrast(c, *s))
                                .fold(f32::MAX, f32::min)
                        };
                        let best = worst(Color32::BLACK).max(worst(Color32::WHITE));
                        for s in surfaces {
                            assert!(
                                contrast(p.text, s) >= 7.0f32.min(best) - 0.01,
                                "{} {page:?} {text:?}",
                                theme.id
                            );
                            assert!(
                                contrast(p.muted, s) >= 4.5f32.min(best) - 0.01,
                                "{} {page:?} {text:?}",
                                theme.id
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_light_and_the_dark_colors_are_kept_apart() {
        let sage = builtin::sage();
        let mut theme = ThemeColors::default();
        theme.dark.page = Some([20, 30, 25]);
        assert!(theme.light.is_default());
        assert_eq!(*theme.mode(false), ModeColors::default());
        theme.mode_mut(false).text = Some([1, 2, 3]);
        assert_eq!(theme.dark.text, None);
        assert_eq!(theme.light.text, Some([1, 2, 3]));
        assert_eq!(with_colors(&sage, false, &theme.light).bg, sage.light.bg);
        assert_eq!(
            with_colors(&sage, true, &theme.dark).bg,
            Color32::from_rgb(20, 30, 25)
        );
    }

    #[test]
    fn the_colors_of_the_user_go_over_the_theme_that_is_on() {
        let page = ModeColors {
            page: Some([230, 240, 255]),
            ..ModeColors::default()
        };
        let (sage, genz) = (builtin::sage(), builtin::genz());
        let (a, b) = (
            with_colors(&sage, false, &page),
            with_colors(&genz, false, &page),
        );
        assert_eq!(a.bg, b.bg, "the page is the one the user chose, in both");
        // what is left of the theme shows through: the shape of the controls does not depend on
        // the colors, and the text of the theme is kept where it reads
        assert_eq!(a.text, sage.light.text);
        assert_eq!(b.text, genz.light.text);
    }

    #[test]
    fn the_strong_line_follows_the_text_it_is_drawn_with() {
        let genz = builtin::genz();
        // ink on a page that turned dark would vanish: the line turns light with the text
        let dark_page = ModeColors {
            page: Some([10, 10, 20]),
            ..ModeColors::default()
        };
        let p = with_colors(&genz, false, &dark_page);
        assert_eq!(p.line, p.text);
        assert!(contrast(p.line, p.bg) >= 7.0);
        // a page of the same lightness keeps the line of the theme
        let pale = ModeColors {
            page: Some([250, 240, 250]),
            ..ModeColors::default()
        };
        assert_eq!(with_colors(&genz, false, &pale).line, genz.light.line);
    }

    #[test]
    fn every_suggested_color_keeps_the_text_readable() {
        for theme in builtin::all() {
            for dark in [false, true] {
                let (base, at) = (theme.palette(dark), usize::from(dark));
                let sw = theme.swatches;
                let mut tried = Vec::new();
                for (_, c) in sw.pages[at] {
                    tried.push(ModeColors {
                        page: Some(*c),
                        ..ModeColors::default()
                    });
                }
                for (_, c) in sw.panels[at] {
                    tried.push(ModeColors {
                        panel: Some(*c),
                        ..ModeColors::default()
                    });
                }
                for (_, c) in sw.sidebars[at] {
                    tried.push(ModeColors {
                        sidebar: Some(*c),
                        ..ModeColors::default()
                    });
                }
                for (_, c) in sw.texts[at] {
                    tried.push(ModeColors {
                        text: Some(*c),
                        ..ModeColors::default()
                    });
                }
                for t in tried {
                    let p = with_colors(&theme, dark, &t);
                    assert_eq!(p.dark, base.dark);
                    for s in [p.bg, p.sidebar, p.card, p.idle, p.hot, p.input] {
                        assert!(contrast(p.text, s) >= 7.0 - 0.01, "{} {t:?}", theme.id);
                        assert!(contrast(p.muted, s) >= 4.5 - 0.01, "{} {t:?}", theme.id);
                    }
                }
            }
        }
    }

    #[test]
    fn the_first_suggestion_of_each_is_the_color_the_appearance_comes_with() {
        for theme in builtin::all() {
            for dark in [false, true] {
                let (p, at) = (theme.palette(dark), usize::from(dark));
                let rgb = |c: Color32| [c.r(), c.g(), c.b()];
                let sw = theme.swatches;
                assert_eq!(sw.pages[at][0].1, rgb(p.bg), "{}", theme.id);
                assert_eq!(sw.panels[at][0].1, rgb(p.card), "{}", theme.id);
                assert_eq!(sw.sidebars[at][0].1, rgb(p.sidebar), "{}", theme.id);
                assert_eq!(sw.texts[at][0].1, rgb(p.text), "{}", theme.id);
            }
            assert_eq!(theme.swatches.accents[0].1, theme.accent, "{}", theme.id);
        }
    }

    #[test]
    fn only_the_colors_that_were_chosen_are_saved() {
        let none = serde_json::to_string(&crate::config::Config::default()).unwrap();
        assert!(!none.contains("theme"));
        let mut c = crate::config::Config::default();
        c.theme.dark.page = Some([1, 2, 3]);
        let text = serde_json::to_string(&c).unwrap();
        assert!(
            text.contains(r#""theme":{"dark":{"page":[1,2,3]}}"#),
            "{text}"
        );
        let back: crate::config::Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.theme, c.theme);
    }

    #[test]
    fn the_choice_of_theme_is_saved_only_when_it_is_not_the_default() {
        let mut c = crate::config::Config::default();
        assert_eq!(c.ui_theme, builtin::DEFAULT_ID);
        c.ui_theme = builtin::GENZ_ID.into();
        let text = serde_json::to_string(&c).unwrap();
        assert!(text.contains(r#""ui_theme":"genz""#), "{text}");
        let back: crate::config::Config = serde_json::from_str(&text).unwrap();
        assert_eq!(back.ui_theme, "genz");
        // a file from before themes existed is sage
        let old: crate::config::Config = serde_json::from_str("{}").unwrap();
        assert_eq!(old.ui_theme, builtin::DEFAULT_ID);
    }

    #[test]
    fn contrast_matches_the_wcag_extremes() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.05);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-4);
        let light = builtin::SAGE_LIGHT;
        assert_eq!(
            contrast(light.bg, light.text),
            contrast(light.text, light.bg),
            "order does not matter"
        );
    }

    #[test]
    fn primary_text_is_readable_on_every_surface() {
        for (look, p) in palettes() {
            for (name, bg) in surfaces(&p) {
                let c = contrast(p.text, bg);
                assert!(c >= 7.0, "text on {look} {name}: {c:.1}:1");
            }
        }
    }

    #[test]
    fn secondary_text_is_readable_on_every_surface() {
        for (look, p) in palettes() {
            for (name, bg) in surfaces(&p) {
                let c = contrast(p.muted, bg);
                assert!(c >= 4.5, "muted on {look} {name}: {c:.1}:1");
            }
        }
    }

    #[test]
    fn status_colors_are_readable_on_the_panels() {
        for (look, p) in palettes() {
            for (label, fg) in [("good", p.good), ("bad", p.bad)] {
                for (name, bg) in surfaces(&p) {
                    let c = contrast(fg, bg);
                    assert!(c >= 4.5, "{label} on {look} {name}: {c:.1}:1");
                }
            }
        }
    }

    #[test]
    fn controls_step_away_from_the_panel_and_lines_are_visible() {
        for (look, p) in palettes() {
            let l = luminance;
            // idle, then hot, move further from the card in the same direction
            let (card, idle, hot) = (l(p.card), l(p.idle), l(p.hot));
            assert!(
                (card - idle).abs() > 0.005 && (idle - hot).abs() > 0.005,
                "{look}: controls are told apart"
            );
            assert!(
                (card - idle) * (idle - hot) > 0.0,
                "{look}: idle and hot go the same way"
            );
            assert!(
                contrast(p.border, p.bg) > 1.1,
                "{look}: a hairline must be visible on the page"
            );
            assert!(
                contrast(p.divider, p.card) > 1.05,
                "{look}: a divider must be visible in a panel"
            );
        }
    }

    #[test]
    fn the_line_of_a_theme_shows_on_every_surface() {
        // it is what outlines panels and controls, and what the hard shadows are cast in
        for (look, p) in palettes() {
            for (name, bg) in surfaces(&p) {
                assert!(contrast(p.line, bg) >= 3.0, "line on {look} {name}");
            }
        }
    }

    #[test]
    fn the_accent_is_lifted_only_where_it_would_sink() {
        let (light, dark) = (builtin::SAGE_LIGHT, builtin::SAGE_DARK);
        let sage = Color32::from_rgb(90, 120, 94);
        // on the light surfaces the sage already shows: it is left alone
        let on_light = [light.sidebar, light.card];
        assert_eq!(legible_on(sage, light.text, &on_light, 4.0), sage);
        // on the dark ones it is lightened until it does
        let on_dark = [dark.sidebar, dark.card];
        assert!(contrast(sage, dark.card) < 4.0, "this is what the logo had");
        let lifted = legible_on(sage, dark.text, &on_dark, 4.0);
        assert_ne!(lifted, sage);
        for s in on_dark {
            assert!(contrast(lifted, s) >= 4.0, "{lifted:?} on {s:?}");
        }
        assert!(lifted.g() > lifted.r(), "still green, not grey");
        // an impossible demand ends at the target instead of looping
        assert_eq!(legible_on(sage, dark.text, &on_dark, 30.0), dark.text);
    }

    #[test]
    fn every_accent_can_be_read_as_text_in_both_appearances() {
        for theme in builtin::all() {
            for dark in [false, true] {
                let p = theme.palette(dark);
                let surfaces = [p.bg, p.sidebar, p.card, p.idle];
                for (name, rgb) in theme.swatches.accents {
                    let accent = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                    let text = legible_on(accent, p.text, &surfaces, 4.5);
                    for s in surfaces {
                        let c = contrast(text, s);
                        assert!(
                            c >= 4.5,
                            "{name} as text on {} dark={dark} {s:?}: {c:.1}:1",
                            theme.id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_appearance_picks_the_palette() {
        use egui::Theme;
        assert!(resolve_dark(Appearance::Dark, None));
        assert!(!resolve_dark(Appearance::Light, Some(Theme::Dark)));
        assert!(resolve_dark(Appearance::System, Some(Theme::Dark)));
        assert!(!resolve_dark(Appearance::System, Some(Theme::Light)));
        assert!(!resolve_dark(Appearance::System, None), "light by default");
        for t in builtin::all() {
            assert!(t.dark.dark && !t.light.dark, "{}", t.id);
        }
    }

    #[test]
    fn on_accent_is_readable_on_any_accent() {
        // the accent is user-chosen, so check the whole spread: saturated, light, dark, grey
        let mut accents = vec![
            Color32::from_rgb(110, 150, 255),
            Color32::from_rgb(255, 220, 60),
            Color32::from_rgb(20, 20, 20),
            Color32::from_rgb(240, 240, 240),
            Color32::from_rgb(128, 128, 128),
            Color32::from_rgb(230, 60, 60),
            Color32::from_rgb(60, 200, 120),
        ];
        for theme in builtin::all() {
            accents.extend(
                theme
                    .swatches
                    .accents
                    .iter()
                    .map(|(_, c)| Color32::from_rgb(c[0], c[1], c[2])),
            );
        }
        for a in accents {
            let c = contrast(a, on_accent(a));
            assert!(c >= 3.0, "text on {a:?}: {c:.1}:1");
        }
        assert_eq!(on_accent(Color32::from_rgb(255, 255, 0)), INK);
        assert_eq!(on_accent(Color32::from_rgb(10, 10, 80)), Color32::WHITE);
    }

    #[test]
    fn the_default_accent_of_sage_carries_white_text_and_that_of_gen_z_carries_ink() {
        let [r, g, b] = builtin::SAGE_ACCENT;
        let accent = Color32::from_rgb(r, g, b);
        assert_eq!(on_accent(accent), Color32::WHITE);
        assert!(contrast(accent, Color32::WHITE) >= 4.5);
        assert!(
            contrast(accent, builtin::SAGE_LIGHT.card) >= 3.0,
            "the accent is visible as a line or a mark on a panel"
        );
        let [r, g, b] = builtin::GENZ_ACCENT;
        let lime = Color32::from_rgb(r, g, b);
        assert_eq!(on_accent(lime), INK, "lime is too light for white text");
        assert!(contrast(lime, INK) >= 7.0);
    }

    #[test]
    fn the_window_outline_is_between_the_hairline_and_the_accent() {
        // the live theme, sage unless a frame picked another
        let (s, p) = (shape(), pal());
        let accent = Color32::from_rgb(0, 0, 0);
        let c = window_outline_color(accent);
        let line = s.window.tone.of(&p);
        if s.window_accent_mix > 0.0 && line.r() > 0 {
            assert!(c.r() < line.r(), "moved toward the accent");
        }
        assert_eq!(
            window_outline_color(line),
            line,
            "an accent that is the line changes nothing"
        );
        // sage: the hairline pulled a little toward the accent
        let sage = builtin::SAGE_SHAPE;
        assert_eq!(sage.window, Edge::new(1.0, Tone::Border));
        assert!(sage.window_accent_mix > 0.0 && sage.window_accent_mix < 0.5);
        // gen z: ink, whatever the accent
        assert_eq!(builtin::GENZ_SHAPE.window_accent_mix, 0.0);
    }

    #[test]
    fn wash_keeps_the_color_and_scales_the_opacity() {
        let w = wash(Color32::from_rgb(10, 20, 30), 0.5);
        assert_eq!(w.a(), 128);
        assert_eq!(wash(Color32::RED, 2.0).a(), 255, "clamped");
        assert_eq!(wash(Color32::RED, -1.0).a(), 0, "clamped");
    }

    #[test]
    fn the_scale_of_radii_goes_from_controls_to_the_window() {
        for t in builtin::all() {
            let s = t.shape;
            assert!(
                s.control_r < s.panel_r,
                "{}: panels are rounder than controls",
                t.id
            );
            assert!(
                s.panel_r < s.window_r,
                "{}: the window is the roundest",
                t.id
            );
            assert_eq!(s.control_radius().nw, s.control_r);
            // a control is 36 points high: more than 18 cannot show
            assert!(f32::from(s.control_r) <= CONTROL_H / 2.0, "{}", t.id);
        }
    }

    #[test]
    fn the_accent_swatches_are_all_different() {
        for t in builtin::all() {
            let accents = t.swatches.accents;
            for (i, (_, a)) in accents.iter().enumerate() {
                assert!(
                    !accents[..i].iter().any(|(_, b)| a == b),
                    "{}: {a:?} is listed twice",
                    t.id
                );
            }
        }
    }

    #[test]
    fn the_window_shadow_fits_the_margin() {
        use crate::ui::app::WIN_MARGIN;
        for t in builtin::all() {
            let reach = t.shape.window_drop.reach();
            assert!(reach <= WIN_MARGIN, "{}: shadow reaches {reach}", t.id);
            // and the outline is inside the card, so it needs no room
        }
    }

    #[test]
    fn a_theme_that_outlines_its_controls_says_so_through_one_stroke() {
        let (sage, genz) = (builtin::sage(), builtin::genz());
        assert_eq!(sage.shape.control.stroke(&sage.light), Stroke::NONE);
        let s = genz.shape.control.stroke(&genz.light);
        assert_eq!((s.width, s.color), (2.0, genz.light.line));
        // in the dark the same theme is outlined in cream
        let d = genz.shape.control.stroke(&genz.dark);
        assert_eq!(d.color, genz.dark.line);
        assert_ne!(d.color, s.color);
    }

    #[test]
    fn the_solid_pie_of_every_theme_reads_on_its_slices() {
        for t in builtin::all() {
            for dark in [false, true] {
                let c = t.pie_colors(dark);
                let slice = Color32::from_rgb(c.slice[0], c.slice[1], c.slice[2]);
                assert!(
                    contrast(c.ink, slice) >= 7.0,
                    "{} dark={dark}: the ink on the slices",
                    t.id
                );
            }
        }
    }
}
