//! Look of the settings window: palette, geometry and the egui style applied to it.
//!
//! The look is modern flat, in the spirit of the Apple system apps, in a light and a dark palette
//! (read through [`pal`]). The colors are those of the website (`assets/site.css`): sage-green
//! neutrals, a cream page and paper-white panels, and a sage accent by default:
//!
//! - a light window with large rounded corners ([`CARD_RADIUS`]); a sidebar that names the
//!   sections, and paper panels (`card`) on a cream page (`bg`), separated by space and
//!   one pale hairline, not by frames and not by shadows;
//! - generous room: one spacing unit ([`GAP`]), a page padding of its own, and one control height
//!   ([`CONTROL_H`]) for every button, field and list row;
//! - round shapes all over: controls use [`CONTROL_R`], panels [`PANEL_R`], badges and chips are
//!   pills, switches and sliders are circles on a rail;
//! - Inter for text and one icon set for every picture (see [`fonts`](crate::ui::fonts) and
//!   [`symbols`](crate::ui::symbols)); there are no capitals-in-monospace labels and no letters
//!   standing in for icons;
//! - one accent color, chosen by the user, for whatever is selected or primary, so it always
//!   means "this". Everything else is neutral.
//!
//! Change the constants here to re-theme. Other files must not hard-code a color or a radius.

use crate::config::{Appearance, ModeColors, ThemeColors};
use crate::ui::draw::{contrast, luminance};
use eframe::egui::{self, Color32, CornerRadius, Rect, Stroke, Vec2};
use std::sync::RwLock;

pub use crate::ui::draw::on_accent;

/// The colors of one appearance. Read them through [`pal`], never copy a value.
///
/// `contrast` pairs are checked by the tests at the bottom of this file: primary text keeps 7:1
/// on every surface, secondary text 4.5:1.
#[derive(Clone, Copy, Debug)]
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

/// The light appearance: paper-white panels on a cream page, with sage-green neutrals. These are the
/// colors of the website (`assets/site.css`: `--page`, `--paper`, `--ink`...), so the app and the
/// site look like one product.
pub const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(245, 243, 236),
    sidebar: Color32::from_rgb(238, 238, 229),
    card: Color32::from_rgb(255, 254, 250),
    idle: Color32::from_rgb(240, 240, 231),
    hot: Color32::from_rgb(229, 236, 226),
    input: Color32::from_rgb(244, 244, 236),
    border: Color32::from_rgb(224, 225, 215),
    divider: Color32::from_rgb(234, 235, 226),
    text: Color32::from_rgb(41, 54, 47),
    muted: Color32::from_rgb(88, 100, 92),
    good: Color32::from_rgb(22, 101, 58),
    bad: Color32::from_rgb(176, 36, 40),
    scrim: Color32::from_rgba_premultiplied(11, 15, 13, 70),
    track: Color32::from_rgb(211, 216, 204),
    keycap: Color32::from_rgb(255, 254, 250),
};

/// The dark appearance: graphite panels on a near-black page, the way Apple's dark mode steps its
/// surfaces, with a faint sage tint in every neutral (a hair more green than blue and red). It is
/// deliberately not the full green of the website's dark mode: the sage accent has to stand out
/// from the surfaces, and the logo has to be seen on the sidebar.
pub const DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(27, 29, 28),
    sidebar: Color32::from_rgb(34, 37, 35),
    card: Color32::from_rgb(42, 46, 44),
    idle: Color32::from_rgb(55, 60, 57),
    hot: Color32::from_rgb(66, 72, 68),
    input: Color32::from_rgb(32, 35, 33),
    border: Color32::from_rgb(64, 70, 66),
    divider: Color32::from_rgb(55, 60, 57),
    text: Color32::from_rgb(244, 244, 239),
    muted: Color32::from_rgb(184, 190, 185),
    good: Color32::from_rgb(96, 208, 132),
    bad: Color32::from_rgb(255, 160, 150),
    scrim: Color32::from_rgba_premultiplied(0, 0, 0, 120),
    track: Color32::from_rgb(86, 93, 88),
    keycap: Color32::from_rgb(84, 91, 86),
};

/// What [`pal`] hands out: the light and the dark palette with the user's colors in them, and the
/// colors they were made from, so a frame that changes nothing costs nothing.
struct Live {
    light: Palette,
    dark: Palette,
    made_from: ThemeColors,
}

static LIVE: RwLock<Live> = RwLock::new(Live {
    light: LIGHT,
    dark: DARK,
    made_from: ThemeColors {
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

/// The palette of an appearance as it is without any color of the user.
pub fn base_palette(dark: bool) -> Palette {
    if dark { DARK } else { LIGHT }
}

/// Whether `appearance` is dark right now: `Dark`, or `System` while the system is dark.
pub fn resolve_dark(appearance: Appearance, system: Option<egui::Theme>) -> bool {
    match appearance {
        Appearance::Dark => true,
        Appearance::Light => false,
        Appearance::System => system == Some(egui::Theme::Dark),
    }
}

/// `base` with the colors of the user in it. What depends on a color follows it: the cards, the
/// controls, the hairlines and the rail are a step away from the page and the panel, the secondary
/// text is the text a step toward the page. The text is then moved just far enough from the
/// surfaces to read on all of them (7:1, and 4.5:1 for the secondary text), whatever was chosen.
pub fn with_colors(base: Palette, t: &ModeColors) -> Palette {
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
        p.text = if page_is_dark { DARK.text } else { LIGHT.text };
        p.muted = if page_is_dark {
            DARK.muted
        } else {
            LIGHT.muted
        };
    }
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
    p
}

/// Picks the palette for this frame from the configured appearance, the system theme and the
/// colors of the user.
pub fn apply_appearance(ctx: &egui::Context, appearance: Appearance, theme: &ThemeColors) {
    crate::ui::draw::set_dark(resolve_dark(appearance, ctx.system_theme()));
    let fresh = LIVE.read().map(|l| l.made_from != *theme).unwrap_or(true);
    if fresh {
        let mut live = LIVE.write().unwrap_or_else(|e| e.into_inner());
        // the appearance decides the base, the colors of the user are laid over it
        live.light = with_colors(LIGHT, &theme.light);
        live.dark = with_colors(DARK, &theme.dark);
        live.made_from = *theme;
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

/// The colors suggested for the page, as `(name, rgb)`, for the light and the dark appearance. The
/// first of each is the color the appearance comes with. All of them are sage-tinted neutrals, like
/// the rest of the palette, so any of them sits well with the default accent.
pub const PAGES: [&[(&str, [u8; 3])]; 2] = [
    &[
        ("Cream", [245, 243, 236]),
        ("Sage mist", [232, 239, 230]),
        ("Sage", [221, 231, 218]),
        ("Oat", [238, 235, 224]),
        ("Fog", [234, 237, 236]),
    ],
    &[
        ("Graphite", [27, 29, 28]),
        ("Forest", [22, 28, 24]),
        ("Night", [17, 21, 19]),
        ("Moss", [31, 36, 32]),
        ("Slate", [28, 31, 34]),
    ],
];

/// The colors suggested for the panels (see [`PAGES`]).
pub const PANELS: [&[(&str, [u8; 3])]; 2] = [
    &[
        ("Paper", [255, 254, 250]),
        ("White", [255, 255, 255]),
        ("Mint", [247, 250, 245]),
        ("Linen", [250, 247, 240]),
        ("Mist", [241, 246, 240]),
    ],
    &[
        ("Graphite", [42, 46, 44]),
        ("Forest", [34, 41, 37]),
        ("Moss", [47, 54, 49]),
        ("Deep", [30, 35, 32]),
        ("Slate", [40, 44, 48]),
    ],
];

/// The colors suggested for the sidebar (see [`PAGES`]).
pub const SIDEBARS: [&[(&str, [u8; 3])]; 2] = [
    &[
        ("Stone", [238, 238, 229]),
        ("Sage mist", [228, 236, 225]),
        ("Sage", [214, 226, 211]),
        ("Oat", [243, 240, 231]),
        ("Fog", [229, 233, 232]),
    ],
    &[
        ("Graphite", [34, 37, 35]),
        ("Forest", [27, 33, 30]),
        ("Night", [20, 25, 22]),
        ("Moss", [40, 46, 42]),
        ("Slate", [33, 36, 40]),
    ],
];

/// The colors suggested for the text (see [`PAGES`]).
pub const TEXTS: [&[(&str, [u8; 3])]; 2] = [
    &[
        ("Ink", [41, 54, 47]),
        ("Deep green", [28, 42, 34]),
        ("Charcoal", [38, 40, 39]),
        ("Slate", [45, 52, 62]),
        ("Umber", [62, 52, 44]),
    ],
    &[
        ("Paper", [244, 244, 239]),
        ("Sage mist", [226, 234, 224]),
        ("White", [255, 255, 255]),
        ("Mint", [208, 224, 212]),
        ("Cream", [240, 232, 214]),
    ],
];

/// The accents offered as swatches, as `(name, rgb)`. The first is the default of a new install: the
/// sage green of the website, deep enough to carry white text. The others are the system colors of
/// Apple's light appearance: saturated enough to read as a color, not so much that a whole
/// selected row hurts.
pub const ACCENTS: [(&str, [u8; 3]); 8] = [
    ("Sage", [90, 120, 94]),
    ("Indigo", [88, 86, 214]),
    ("Blue", [0, 122, 255]),
    ("Purple", [175, 82, 222]),
    ("Pink", [255, 45, 85]),
    ("Orange", [255, 149, 0]),
    ("Green", [52, 199, 89]),
    ("Graphite", [99, 99, 102]),
];

// Geometry
/// Corner radius of buttons, fields and tiles, in points.
pub const CONTROL_R_PT: u8 = 12;
/// Corner radius of buttons, fields and tiles.
pub const CONTROL_R: CornerRadius = CornerRadius::same(CONTROL_R_PT);
/// Corner radius of panels, in points.
pub const PANEL_R_PT: u8 = 18;
/// Corner radius of panels and popups.
pub const PANEL_R: CornerRadius = CornerRadius::same(PANEL_R_PT);
/// Corner radius that turns any control into a pill. egui clamps it to half the height.
pub const PILL: CornerRadius = CornerRadius::same(255);
/// Corner radius of the window itself, in points.
pub const CARD_RADIUS_PT: u8 = 24;
/// Corner radius of the window.
pub const CARD_RADIUS: CornerRadius = CornerRadius::same(CARD_RADIUS_PT);
/// Space between controls, in points.
pub const GAP: f32 = 12.0;
/// Space between two panels, in points.
pub const SECTION_GAP: f32 = 24.0;
/// Height of every button, chip, input and list row, in points.
pub const CONTROL_H: f32 = 36.0;
/// Space between the edge of a panel and its content, in points.
pub const PANEL_PAD: i8 = 18;

/// A panel: white, rounded, no border to speak of. Rows inside it add their own vertical room, so
/// the frame only pads the sides.
pub(in crate::ui::settings) fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(pal().card)
        .corner_radius(PANEL_R)
        .inner_margin(egui::Margin::symmetric(PANEL_PAD, 6))
        .stroke(Stroke::new(1.0, pal().divider))
}

/// The frame of a popover or a sheet: a panel that floats, with a soft shadow.
pub(in crate::ui::settings) fn popup_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(pal().card)
        .corner_radius(PANEL_R)
        .inner_margin(egui::Margin::same(PANEL_PAD))
        .stroke(Stroke::new(1.0, pal().border))
        .shadow(popup_shadow())
}

/// The shadow of anything that floats above the window.
pub(in crate::ui::settings) fn popup_shadow() -> egui::Shadow {
    egui::Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(38),
    }
}

/// The color of the window outline: the hairline color pulled a little toward `accent`, so the
/// window reads as focused without a loud frame.
pub fn window_outline_color(accent: Color32) -> Color32 {
    pal().border.lerp_to_gamma(accent, 0.18)
}

/// Draws the outline of the window just inside `rect`. It is an ordinary rounded rectangle
/// stroke, which egui anti-aliases; a hand-made mesh would have stair-stepped edges and could not
/// follow rounded corners.
pub(in crate::ui::settings) fn window_outline(
    painter: &egui::Painter,
    rect: Rect,
    accent: Color32,
) {
    painter.rect_stroke(
        rect,
        CARD_RADIUS,
        Stroke::new(1.0, window_outline_color(accent)),
        egui::StrokeKind::Inside,
    );
}

/// The shadow under the window. The window is transparent and larger than the card by
/// `WIN_MARGIN` on every side, which is where the shadow is painted, so offset plus blur must stay
/// inside that margin.
pub(in crate::ui::settings) fn window_shadow() -> egui::Shadow {
    egui::Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(64),
    }
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
    v.window_stroke = Stroke::new(1.0, pal().border);
    v.extreme_bg_color = pal().input;
    v.faint_bg_color = pal().idle;
    v.override_text_color = None;
    v.selection.bg_fill = wash(accent, 0.28);
    v.selection.stroke = Stroke::new(1.5, accent);
    v.hyperlink_color = accent_text(accent);
    // popups (combo boxes, menus) float above the window, so they keep a soft shadow
    v.window_corner_radius = PANEL_R;
    v.menu_corner_radius = CONTROL_R;
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
        state.corner_radius = CONTROL_R;
        state.expansion = 0.0;
    }
    // separators and panel outlines; labels and button text use the primary text color
    w.noninteractive.bg_stroke = Stroke::new(1.0, pal().divider);
    w.noninteractive.fg_stroke = Stroke::new(1.0, pal().text);
    w.noninteractive.bg_fill = pal().card;
    w.noninteractive.weak_bg_fill = pal().card;
    // at rest: a soft grey fill without an outline; under the pointer: a step darker;
    // pressed: the accent
    w.inactive.fg_stroke = Stroke::new(1.0, pal().text);
    w.inactive.bg_fill = pal().idle;
    w.inactive.weak_bg_fill = pal().idle;
    w.inactive.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.0, pal().text);
    w.hovered.bg_fill = pal().hot;
    w.hovered.weak_bg_fill = pal().hot;
    w.hovered.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.0, on_accent(accent));
    w.active.bg_fill = accent;
    w.active.weak_bg_fill = accent;
    w.active.bg_stroke = Stroke::NONE;
    w.open.fg_stroke = Stroke::new(1.0, pal().text);
    w.open.bg_fill = pal().hot;
    w.open.weak_bg_fill = pal().hot;
    w.open.bg_stroke = Stroke::NONE;

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
    use crate::ui::draw::{INK, contrast, luminance};

    /// Both appearances, by name.
    const PALETTES: [(&str, Palette); 2] = [("light", LIGHT), ("dark", DARK)];

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
        for base in [LIGHT, DARK] {
            let p = with_colors(base, &ModeColors::default());
            assert_eq!((p.bg, p.card, p.text), (base.bg, base.card, base.text));
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
        for base in [LIGHT, DARK] {
            for page in pages {
                for text in [None, Some([120, 120, 120]), Some(page)] {
                    let t = ModeColors {
                        page: Some(page),
                        text,
                        ..ModeColors::default()
                    };
                    let p = with_colors(base, &t);
                    assert_eq!(p.dark, base.dark, "the appearance decides it, not the page");
                    let surfaces = [p.bg, p.sidebar, p.card, p.idle, p.hot, p.input];
                    // a mid tone cannot carry 7:1 in either black or white: the best there is, for
                    // the worst surface
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
                            "{page:?} {text:?}"
                        );
                        assert!(
                            contrast(p.muted, s) >= 4.5f32.min(best) - 0.01,
                            "{page:?} {text:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_light_and_the_dark_colors_are_kept_apart() {
        let mut theme = ThemeColors::default();
        theme.dark.page = Some([20, 30, 25]);
        assert!(theme.light.is_default());
        assert_eq!(*theme.mode(false), ModeColors::default());
        theme.mode_mut(false).text = Some([1, 2, 3]);
        assert_eq!(theme.dark.text, None);
        assert_eq!(theme.light.text, Some([1, 2, 3]));
        assert_eq!(with_colors(LIGHT, &theme.light).bg, LIGHT.bg);
        assert_eq!(
            with_colors(DARK, &theme.dark).bg,
            Color32::from_rgb(20, 30, 25)
        );
    }

    #[test]
    fn every_suggested_color_keeps_the_text_readable() {
        for dark in [false, true] {
            let (base, at) = (base_palette(dark), usize::from(dark));
            let mut tried = Vec::new();
            for (_, c) in PAGES[at] {
                tried.push(ModeColors {
                    page: Some(*c),
                    ..ModeColors::default()
                });
            }
            for (_, c) in PANELS[at] {
                tried.push(ModeColors {
                    panel: Some(*c),
                    ..ModeColors::default()
                });
            }
            for (_, c) in SIDEBARS[at] {
                tried.push(ModeColors {
                    sidebar: Some(*c),
                    ..ModeColors::default()
                });
            }
            for (_, c) in TEXTS[at] {
                tried.push(ModeColors {
                    text: Some(*c),
                    ..ModeColors::default()
                });
            }
            for t in tried {
                let p = with_colors(base, &t);
                for s in [p.bg, p.sidebar, p.card, p.idle, p.hot, p.input] {
                    assert!(contrast(p.text, s) >= 7.0 - 0.01, "{t:?}");
                    assert!(contrast(p.muted, s) >= 4.5 - 0.01, "{t:?}");
                }
            }
        }
    }

    #[test]
    fn the_first_suggestion_of_each_is_the_color_the_appearance_comes_with() {
        for dark in [false, true] {
            let (p, at) = (base_palette(dark), usize::from(dark));
            let rgb = |c: Color32| [c.r(), c.g(), c.b()];
            assert_eq!(PAGES[at][0].1, rgb(p.bg));
            assert_eq!(PANELS[at][0].1, rgb(p.card));
            assert_eq!(SIDEBARS[at][0].1, rgb(p.sidebar));
            assert_eq!(TEXTS[at][0].1, rgb(p.text));
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
    fn contrast_matches_the_wcag_extremes() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.05);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-4);
        assert_eq!(
            contrast(LIGHT.bg, LIGHT.text),
            contrast(LIGHT.text, LIGHT.bg),
            "order does not matter"
        );
    }

    #[test]
    fn primary_text_is_readable_on_every_surface() {
        for (look, p) in PALETTES {
            for (name, bg) in surfaces(&p) {
                let c = contrast(p.text, bg);
                assert!(c >= 7.0, "text on {look} {name}: {c:.1}:1");
            }
        }
    }

    #[test]
    fn secondary_text_is_readable_on_every_surface() {
        for (look, p) in PALETTES {
            for (name, bg) in surfaces(&p) {
                let c = contrast(p.muted, bg);
                assert!(c >= 4.5, "muted on {look} {name}: {c:.1}:1");
            }
        }
    }

    #[test]
    fn status_colors_are_readable_on_the_panels() {
        for (look, p) in PALETTES {
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
        for (look, p) in PALETTES {
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
    fn the_accent_is_lifted_only_where_it_would_sink() {
        let sage = Color32::from_rgb(90, 120, 94);
        // on the light surfaces the sage already shows: it is left alone
        let light = [LIGHT.sidebar, LIGHT.card];
        assert_eq!(legible_on(sage, LIGHT.text, &light, 4.0), sage);
        // on the dark ones it is lightened until it does
        let dark = [DARK.sidebar, DARK.card];
        assert!(contrast(sage, DARK.card) < 4.0, "this is what the logo had");
        let lifted = legible_on(sage, DARK.text, &dark, 4.0);
        assert_ne!(lifted, sage);
        for s in dark {
            assert!(contrast(lifted, s) >= 4.0, "{lifted:?} on {s:?}");
        }
        assert!(lifted.g() > lifted.r(), "still green, not grey");
        // an impossible demand ends at the target instead of looping
        assert_eq!(legible_on(sage, DARK.text, &dark, 30.0), DARK.text);
    }

    #[test]
    fn every_accent_can_be_read_as_text_in_both_appearances() {
        for (look, p) in PALETTES {
            let surfaces = [p.bg, p.sidebar, p.card, p.idle];
            for (name, rgb) in ACCENTS {
                let accent = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                let text = legible_on(accent, p.text, &surfaces, 4.5);
                for s in surfaces {
                    let c = contrast(text, s);
                    assert!(c >= 4.5, "{name} as text on {look} {s:?}: {c:.1}:1");
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
        const {
            assert!(DARK.dark && !LIGHT.dark);
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
        accents.extend(
            ACCENTS
                .iter()
                .map(|(_, c)| Color32::from_rgb(c[0], c[1], c[2])),
        );
        for a in accents {
            let c = contrast(a, on_accent(a));
            assert!(c >= 3.0, "text on {a:?}: {c:.1}:1");
        }
        assert_eq!(on_accent(Color32::from_rgb(255, 255, 0)), INK);
        assert_eq!(on_accent(Color32::from_rgb(10, 10, 80)), Color32::WHITE);
    }

    #[test]
    fn the_default_accent_carries_white_text() {
        let [r, g, b] = ACCENTS[0].1;
        let accent = Color32::from_rgb(r, g, b);
        assert_eq!(on_accent(accent), Color32::WHITE);
        assert!(contrast(accent, Color32::WHITE) >= 4.5);
        assert!(
            contrast(accent, pal().card) >= 3.0,
            "the accent is visible as a line or a mark on a panel"
        );
    }

    #[test]
    fn the_window_outline_is_between_the_hairline_and_the_accent() {
        let accent = Color32::from_rgb(0, 0, 0);
        let c = window_outline_color(accent);
        assert!(
            c.r() < pal().border.r() && c.r() > accent.r(),
            "moved toward the accent"
        );
        assert_eq!(
            window_outline_color(pal().border),
            pal().border,
            "no accent, no change"
        );
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
        // both sides are constants, so the compiler can check this instead of a test run
        const {
            assert!(
                CONTROL_R_PT < PANEL_R_PT,
                "panels are rounder than controls"
            );
            assert!(PANEL_R_PT < CARD_RADIUS_PT, "the window is the roundest");
        }
        assert_eq!(CARD_RADIUS.nw, CARD_RADIUS_PT);
        assert_eq!(PANEL_R.se, PANEL_R_PT);
    }

    #[test]
    fn the_accent_swatches_are_all_different() {
        for (i, (_, a)) in ACCENTS.iter().enumerate() {
            assert!(
                !ACCENTS[..i].iter().any(|(_, b)| a == b),
                "{a:?} is listed twice"
            );
        }
    }

    #[test]
    fn the_window_shadow_fits_the_margin() {
        use crate::ui::app::WIN_MARGIN;
        let s = window_shadow();
        let reach = s.offset[1].unsigned_abs() as f32 + s.blur as f32 + s.spread as f32;
        assert!(reach <= WIN_MARGIN, "shadow reaches {reach}");
    }
}
