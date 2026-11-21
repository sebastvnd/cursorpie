//! The themes that ship with CursorPie: **gen z** (the look of the website, and the default),
//! **sage** (the look Settings had before themes existed) and **bubblegum** (Gen Z in candy pink).
//! All of them are only values: see [`super::tokens`] for what each one means.
//!
//! To add a theme of your own to the app, copy one of the functions below, change the values, and
//! list it in [`all`]. To make one without touching the code, write a file: see `spec.rs`.

use super::Palette;
use super::tokens::{DropShadow, Edge, Shape, Swatches, Theme, Tone};
use crate::ui::draw::PieColors;
use eframe::egui::Color32;

/// The id of the theme a fresh install starts with, and the one any unknown id falls back to.
pub const DEFAULT_ID: &str = crate::config::DEFAULT_UI_THEME;
/// The id of the Gen Z theme.
pub const GENZ_ID: &str = "genz";
/// The id of the Sage theme: the base of a theme file that does not say what it `extends`.
pub const SAGE_ID: &str = "sage";
/// The id of the Bubblegum theme.
pub const BUBBLEGUM_ID: &str = "bubblegum";

const fn c(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// The built-in themes, in the order they are offered: the default first.
pub fn all() -> Vec<Theme> {
    vec![genz(), sage(), bubblegum()]
}

/// The theme a fresh install starts with, and the one an unknown id is shown as.
pub fn default_theme() -> Theme {
    find(DEFAULT_ID).unwrap_or_else(genz)
}

/// The built-in theme with this id.
pub fn find(id: &str) -> Option<Theme> {
    all().into_iter().find(|t| t.id == id)
}

// ======================================================================================== sage

/// The light appearance of sage: paper-white panels on a cream page, with sage-green neutrals.
/// These are the colors of the website's first look (`assets/site.css` before the Gen Z one), so the
/// app and the site looked like one product.
pub const SAGE_LIGHT: Palette = Palette {
    dark: false,
    bg: c(245, 243, 236),
    sidebar: c(238, 238, 229),
    card: c(255, 254, 250),
    idle: c(240, 240, 231),
    hot: c(229, 236, 226),
    input: c(244, 244, 236),
    border: c(224, 225, 215),
    divider: c(234, 235, 226),
    line: c(41, 54, 47),
    text: c(41, 54, 47),
    muted: c(88, 100, 92),
    good: c(22, 101, 58),
    bad: c(176, 36, 40),
    scrim: Color32::from_rgba_premultiplied(11, 15, 13, 70),
    track: c(211, 216, 204),
    keycap: c(255, 254, 250),
};

/// The dark appearance of sage: graphite panels on a near-black page, the way Apple's dark mode
/// steps its surfaces, with a faint sage tint in every neutral (a hair more green than blue and
/// red). It is deliberately not the full green of the website's dark mode: the sage accent has to
/// stand out from the surfaces, and the logo has to be seen on the sidebar.
pub const SAGE_DARK: Palette = Palette {
    dark: true,
    bg: c(27, 29, 28),
    sidebar: c(34, 37, 35),
    card: c(42, 46, 44),
    idle: c(55, 60, 57),
    hot: c(66, 72, 68),
    input: c(32, 35, 33),
    border: c(64, 70, 66),
    divider: c(55, 60, 57),
    line: c(244, 244, 239),
    text: c(244, 244, 239),
    muted: c(184, 190, 185),
    good: c(96, 208, 132),
    bad: c(255, 160, 150),
    scrim: Color32::from_rgba_premultiplied(0, 0, 0, 120),
    track: c(86, 93, 88),
    keycap: c(84, 91, 86),
};

/// Modern flat: soft fills, hairlines, large rounded corners, soft shadows only under what floats.
pub const SAGE_SHAPE: Shape = Shape {
    control_r: 12,
    panel_r: 18,
    window_r: 24,
    control: Edge::NONE,
    chip: Edge::new(1.0, Tone::Border),
    panel: Edge::new(1.0, Tone::Divider),
    popup: Edge::new(1.0, Tone::Border),
    window: Edge::new(1.0, Tone::Border),
    side: Edge::new(1.0, Tone::Border),
    window_accent_mix: 0.18,
    control_drop: DropShadow::NONE,
    panel_drop: DropShadow::NONE,
    popup_drop: DropShadow::soft(8, 28, 38),
    window_drop: DropShadow::soft(10, 30, 64),
    nav_wash: 0.14,
    nav_icon_accent: true,
};

/// Frosted paper white, or smoked graphite with a sage tint.
pub const SAGE_PIE: [PieColors; 2] = [
    PieColors {
        slice: [255, 254, 250],
        ghost: [238, 238, 229],
        ink: c(41, 54, 47),
        core: c(255, 254, 250),
    },
    PieColors {
        slice: [47, 51, 49],
        ghost: [37, 41, 39],
        ink: c(244, 244, 239),
        core: c(31, 34, 32),
    },
];

/// The default accent of sage: the sage green of the website, deep enough to carry white text.
pub const SAGE_ACCENT: [u8; 3] = [90, 120, 94];

/// The colors sage suggests. All of them are sage-tinted neutrals, like the rest of the palette, so
/// any of them sits well with the default accent.
pub static SAGE_SWATCHES: Swatches = Swatches {
    pages: [
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
    ],
    panels: [
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
    ],
    sidebars: [
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
    ],
    texts: [
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
    ],
    // the first is the default of a new install; the others are the system colors of Apple's light
    // appearance: saturated enough to read as a color, not so much that a whole selected row hurts
    accents: &[
        ("Sage", SAGE_ACCENT),
        ("Indigo", [88, 86, 214]),
        ("Blue", [0, 122, 255]),
        ("Purple", [175, 82, 222]),
        ("Pink", [255, 45, 85]),
        ("Orange", [255, 149, 0]),
        ("Green", [52, 199, 89]),
        ("Graphite", [99, 99, 102]),
    ],
};

/// The sage theme.
pub fn sage() -> Theme {
    Theme {
        id: SAGE_ID.to_owned(),
        name: "Sage".to_owned(),
        light: SAGE_LIGHT,
        dark: SAGE_DARK,
        shape: SAGE_SHAPE,
        accent: SAGE_ACCENT,
        pie: SAGE_PIE,
        swatches: &SAGE_SWATCHES,
        builtin: true,
    }
}

// ======================================================================================== gen z

/// The light appearance of Gen Z: the website's tokens (`assets/site.css`: `--bg`, `--bg-2`,
/// `--card`, `--ink`, `--muted`). A warm cream page, white panels, ink as the line and the text.
pub const GENZ_LIGHT: Palette = Palette {
    dark: false,
    bg: c(255, 248, 236),
    sidebar: c(255, 241, 214),
    card: c(255, 255, 255),
    idle: c(255, 244, 222),
    hot: c(255, 233, 190),
    input: c(255, 248, 236),
    border: c(221, 208, 182),
    divider: c(240, 228, 206),
    line: c(20, 17, 15),
    text: c(20, 17, 15),
    muted: c(93, 86, 76),
    good: c(22, 101, 58),
    bad: c(184, 24, 66),
    scrim: Color32::from_rgba_premultiplied(10, 9, 8, 128),
    track: c(233, 221, 196),
    keycap: c(255, 255, 255),
};

/// The dark appearance of Gen Z: the website's dark tokens, a violet-tinted near black with cream
/// ink, where the outlines and the hard shadows turn cream.
pub const GENZ_DARK: Palette = Palette {
    dark: true,
    bg: c(16, 15, 20),
    sidebar: c(23, 22, 29),
    card: c(28, 27, 36),
    idle: c(40, 38, 50),
    hot: c(54, 51, 68),
    input: c(22, 21, 28),
    border: c(66, 63, 80),
    divider: c(50, 48, 62),
    line: c(246, 242, 232),
    text: c(246, 242, 232),
    muted: c(180, 174, 162),
    good: c(96, 208, 132),
    bad: c(255, 138, 160),
    scrim: Color32::from_rgba_premultiplied(0, 0, 0, 130),
    track: c(78, 75, 94),
    keycap: c(62, 59, 78),
};

/// Neo-brutalist, like the website: pill controls, 2-point ink outlines on everything, and hard
/// shadows (no blur) that fall down and to the right. A pressed button loses its shadow.
pub const GENZ_SHAPE: Shape = Shape {
    control_r: 18,
    panel_r: 22,
    window_r: 30,
    control: Edge::new(2.0, Tone::Line),
    chip: Edge::new(2.0, Tone::Line),
    panel: Edge::new(2.0, Tone::Line),
    popup: Edge::new(2.0, Tone::Line),
    window: Edge::new(2.5, Tone::Line),
    side: Edge::new(2.0, Tone::Line),
    window_accent_mix: 0.0,
    control_drop: DropShadow::hard(3, 235),
    panel_drop: DropShadow::hard(4, 235),
    popup_drop: DropShadow::hard(4, 235),
    window_drop: DropShadow::hard(6, 235),
    nav_wash: 0.55,
    nav_icon_accent: false,
};

/// White slices with ink, or violet-tinted ones in the dark.
pub const GENZ_PIE: [PieColors; 2] = [
    PieColors {
        slice: [255, 255, 255],
        ghost: [255, 244, 222],
        ink: c(20, 17, 15),
        core: c(255, 255, 255),
    },
    PieColors {
        slice: [40, 38, 50],
        ghost: [30, 29, 38],
        ink: c(246, 242, 232),
        core: c(23, 22, 29),
    },
];

/// The lime of the website (`--lime`): the accent of Gen Z. It carries ink, not white.
pub const GENZ_ACCENT: [u8; 3] = [200, 255, 46];

/// The colors Gen Z suggests: the candy colors of the website, as pale pages and bold accents.
pub static GENZ_SWATCHES: Swatches = Swatches {
    pages: [
        &[
            ("Butter", [255, 248, 236]),
            ("Lemon", [255, 247, 204]),
            ("Peach", [255, 233, 216]),
            ("Lilac", [241, 235, 255]),
            ("Sky", [228, 245, 255]),
            ("Mint", [225, 250, 236]),
        ],
        &[
            ("Midnight", [16, 15, 20]),
            ("Void", [10, 9, 13]),
            ("Grape", [24, 16, 34]),
            ("Navy", [14, 18, 32]),
            ("Cocoa", [24, 18, 16]),
        ],
    ],
    panels: [
        &[
            ("White", [255, 255, 255]),
            ("Cream", [255, 250, 240]),
            ("Lemon", [255, 252, 226]),
            ("Lilac", [248, 244, 255]),
            ("Sky", [241, 250, 255]),
        ],
        &[
            ("Slate", [28, 27, 36]),
            ("Plum", [36, 24, 48]),
            ("Ink", [22, 21, 28]),
            ("Navy", [22, 28, 46]),
            ("Cocoa", [38, 30, 26]),
        ],
    ],
    sidebars: [
        &[
            ("Butter", [255, 241, 214]),
            ("Lemon", [255, 240, 176]),
            ("Peach", [255, 222, 200]),
            ("Lilac", [228, 220, 255]),
            ("Mint", [214, 244, 226]),
        ],
        &[
            ("Dusk", [23, 22, 29]),
            ("Grape", [30, 20, 42]),
            ("Night", [14, 13, 18]),
            ("Navy", [18, 23, 40]),
            ("Cocoa", [30, 24, 22]),
        ],
    ],
    texts: [
        &[
            ("Ink", [20, 17, 15]),
            ("Plum", [46, 20, 60]),
            ("Navy", [16, 24, 60]),
            ("Cocoa", [60, 36, 24]),
            ("Forest", [14, 40, 28]),
        ],
        &[
            ("Cream", [246, 242, 232]),
            ("Lilac", [232, 224, 255]),
            ("White", [255, 255, 255]),
            ("Mint", [210, 244, 224]),
            ("Peach", [255, 226, 208]),
        ],
    ],
    accents: &[
        ("Lime", GENZ_ACCENT),
        ("Pink", [255, 94, 168]),
        ("Violet", [141, 107, 255]),
        ("Sky", [90, 208, 255]),
        ("Orange", [255, 154, 61]),
        ("Green", [47, 210, 122]),
    ],
};

/// The Gen Z theme.
pub fn genz() -> Theme {
    Theme {
        id: GENZ_ID.to_owned(),
        name: "Gen Z".to_owned(),
        light: GENZ_LIGHT,
        dark: GENZ_DARK,
        shape: GENZ_SHAPE,
        accent: GENZ_ACCENT,
        pie: GENZ_PIE,
        swatches: &GENZ_SWATCHES,
        builtin: true,
    }
}

// ======================================================================================== bubblegum

/// The light appearance of Bubblegum: a cotton-candy pink page and sidebar, a milky panel and plum
/// ink instead of the near black of Gen Z. The strong line is plum, so outlines and hard shadows
/// stay as bold as on the website without the page turning cream.
pub const BUBBLEGUM_LIGHT: Palette = Palette {
    dark: false,
    bg: c(255, 227, 241),
    sidebar: c(255, 208, 231),
    card: c(255, 250, 253),
    idle: c(255, 236, 246),
    hot: c(255, 214, 236),
    input: c(255, 240, 248),
    border: c(232, 184, 208),
    divider: c(246, 214, 230),
    line: c(42, 16, 48),
    text: c(42, 16, 48),
    muted: c(108, 70, 104),
    good: c(18, 92, 52),
    bad: c(170, 20, 60),
    scrim: Color32::from_rgba_premultiplied(21, 6, 24, 128),
    track: c(245, 200, 225),
    keycap: c(255, 250, 253),
};

/// The dark appearance of Bubblegum: a berry-tinted near black with blush ink, where the outlines and
/// the hard shadows turn blush.
pub const BUBBLEGUM_DARK: Palette = Palette {
    dark: true,
    bg: c(26, 15, 28),
    sidebar: c(34, 20, 38),
    card: c(44, 27, 50),
    idle: c(60, 38, 68),
    hot: c(80, 50, 90),
    input: c(30, 18, 34),
    border: c(88, 58, 98),
    divider: c(66, 42, 74),
    line: c(255, 236, 246),
    text: c(255, 236, 246),
    muted: c(206, 174, 200),
    good: c(96, 208, 132),
    bad: c(255, 138, 160),
    scrim: Color32::from_rgba_premultiplied(0, 0, 0, 130),
    track: c(100, 70, 112),
    keycap: c(84, 56, 94),
};

/// Gen Z with rounder shapes: the same outlines and hard shadows, bubblier corners.
pub const BUBBLEGUM_SHAPE: Shape = Shape {
    control_r: 18,
    panel_r: 26,
    window_r: 34,
    control: Edge::new(2.0, Tone::Line),
    chip: Edge::new(2.0, Tone::Line),
    panel: Edge::new(2.0, Tone::Line),
    popup: Edge::new(2.0, Tone::Line),
    window: Edge::new(2.5, Tone::Line),
    side: Edge::new(2.0, Tone::Line),
    window_accent_mix: 0.0,
    control_drop: DropShadow::hard(3, 235),
    panel_drop: DropShadow::hard(4, 235),
    popup_drop: DropShadow::hard(4, 235),
    window_drop: DropShadow::hard(6, 235),
    nav_wash: 0.55,
    nav_icon_accent: false,
};

/// Milky slices with plum ink, or berry ones in the dark.
pub const BUBBLEGUM_PIE: [PieColors; 2] = [
    PieColors {
        slice: [255, 250, 253],
        ghost: [255, 232, 244],
        ink: c(42, 16, 48),
        core: c(255, 250, 253),
    },
    PieColors {
        slice: [60, 38, 68],
        ghost: [44, 27, 50],
        ink: c(255, 236, 246),
        core: c(34, 20, 38),
    },
];

/// The pink of Bubblegum. It carries ink, not white.
pub const BUBBLEGUM_ACCENT: [u8; 3] = [255, 94, 168];

/// The colors Bubblegum suggests: sweets, as pale pages and bold accents.
pub static BUBBLEGUM_SWATCHES: Swatches = Swatches {
    pages: [
        &[
            ("Bubblegum", [255, 227, 241]),
            ("Cotton", [255, 240, 248]),
            ("Lilac", [241, 228, 255]),
            ("Peach", [255, 234, 224]),
            ("Mint", [222, 248, 238]),
        ],
        &[
            ("Plum", [26, 15, 28]),
            ("Grape", [24, 16, 34]),
            ("Void", [14, 9, 16]),
            ("Berry", [34, 14, 28]),
            ("Navy", [16, 16, 32]),
        ],
    ],
    panels: [
        &[
            ("Milk", [255, 250, 253]),
            ("White", [255, 255, 255]),
            ("Blush", [255, 242, 248]),
            ("Lilac", [250, 244, 255]),
            ("Mint", [243, 252, 248]),
        ],
        &[
            ("Plum", [44, 27, 50]),
            ("Ink", [30, 18, 34]),
            ("Berry", [52, 24, 44]),
            ("Grape", [40, 26, 58]),
            ("Navy", [26, 28, 50]),
        ],
    ],
    sidebars: [
        &[
            ("Candy", [255, 208, 231]),
            ("Blush", [255, 224, 238]),
            ("Lilac", [232, 218, 255]),
            ("Peach", [255, 222, 206]),
            ("Mint", [208, 242, 228]),
        ],
        &[
            ("Dusk", [34, 20, 38]),
            ("Grape", [30, 20, 42]),
            ("Night", [18, 10, 20]),
            ("Berry", [44, 18, 36]),
            ("Navy", [20, 22, 40]),
        ],
    ],
    texts: [
        &[
            ("Plum", [42, 16, 48]),
            ("Ink", [20, 17, 15]),
            ("Navy", [22, 24, 70]),
            ("Berry", [84, 14, 56]),
            ("Forest", [14, 40, 28]),
        ],
        &[
            ("Blush", [255, 236, 246]),
            ("Lilac", [232, 224, 255]),
            ("White", [255, 255, 255]),
            ("Mint", [210, 244, 224]),
            ("Peach", [255, 226, 208]),
        ],
    ],
    accents: &[
        ("Bubblegum", BUBBLEGUM_ACCENT),
        ("Lime", GENZ_ACCENT),
        ("Violet", [141, 107, 255]),
        ("Sky", [90, 208, 255]),
        ("Orange", [255, 154, 61]),
        ("Green", [47, 210, 122]),
    ],
};

/// The Bubblegum theme.
pub fn bubblegum() -> Theme {
    Theme {
        id: BUBBLEGUM_ID.to_owned(),
        name: "Bubblegum".to_owned(),
        light: BUBBLEGUM_LIGHT,
        dark: BUBBLEGUM_DARK,
        shape: BUBBLEGUM_SHAPE,
        accent: BUBBLEGUM_ACCENT,
        pie: BUBBLEGUM_PIE,
        swatches: &BUBBLEGUM_SWATCHES,
        builtin: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ids_are_unique_and_the_default_is_first() {
        let all = all();
        assert_eq!(all[0].id, DEFAULT_ID);
        assert_eq!(DEFAULT_ID, GENZ_ID, "a fresh install is Gen Z");
        assert_eq!(default_theme().id, GENZ_ID);
        let ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, [GENZ_ID, SAGE_ID, BUBBLEGUM_ID]);
        for (i, t) in all.iter().enumerate() {
            assert!(t.builtin);
            assert!(!all[..i].iter().any(|o| o.id == t.id), "{} twice", t.id);
        }
        assert_eq!(find(GENZ_ID).unwrap().name, "Gen Z");
        assert!(find("no such theme").is_none());
    }

    /// Sage is the look the app had before themes existed: its tokens are the old constants
    /// (`CONTROL_R_PT` 12, `PANEL_R_PT` 18, `CARD_RADIUS_PT` 24, a 1-point hairline on panels, a
    /// soft shadow under the window), so that picking it changes nothing.
    #[test]
    fn sage_is_the_look_the_app_always_had() {
        let s = sage().shape;
        assert_eq!((s.control_r, s.panel_r, s.window_r), (12, 18, 24));
        assert!(!s.control.is_visible() && !s.control_drop.is_visible());
        assert!(!s.panel_drop.is_visible());
        assert_eq!(s.panel, Edge::new(1.0, Tone::Divider));
        assert_eq!(s.window, Edge::new(1.0, Tone::Border));
        assert_eq!(s.window_drop, DropShadow::soft(10, 30, 64));
        assert_eq!(s.popup_drop, DropShadow::soft(8, 28, 38));
        assert_eq!(sage().accent, [90, 120, 94]);
    }

    #[test]
    fn gen_z_has_the_look_of_the_website() {
        let t = genz();
        // --bg, --card, --ink and --muted of assets/site.css
        assert_eq!(t.light.bg, c(255, 248, 236));
        assert_eq!(t.light.card, c(255, 255, 255));
        assert_eq!(t.light.text, c(20, 17, 15));
        assert_eq!(t.light.muted, c(93, 86, 76));
        // and the dark ones
        assert_eq!(t.dark.bg, c(16, 15, 20));
        assert_eq!(t.dark.text, c(246, 242, 232));
        // outlines and hard shadows on everything, as on the website
        assert!(t.shape.control.is_visible() && t.shape.panel.is_visible());
        assert!(t.shape.control_drop.is_visible() && t.shape.panel_drop.is_visible());
        assert_eq!(t.shape.panel_drop.blur, 0, "hard, not soft");
        assert_eq!(t.accent, [200, 255, 46], "--lime");
    }

    #[test]
    fn bubblegum_is_gen_z_in_pink() {
        let t = bubblegum();
        assert_eq!(t.name, "Bubblegum");
        assert_eq!(t.accent, BUBBLEGUM_ACCENT);
        assert_eq!(
            t.accent, t.swatches.accents[0].1,
            "the first swatch is the default"
        );
        // outlines and hard shadows like the website, on a pink page
        assert!(t.shape.control.is_visible() && t.shape.panel.is_visible());
        assert_eq!(t.shape.panel_drop.blur, 0, "hard, not soft");
        assert_ne!(t.light.bg, genz().light.bg);
        // the same Edge and shadow tokens as Gen Z, rounder corners
        assert!(t.shape.panel_r > genz().shape.panel_r);
        assert_eq!(find(BUBBLEGUM_ID).unwrap().id, BUBBLEGUM_ID);
    }

    #[test]
    fn the_line_of_gen_z_is_its_text_so_it_is_always_seen() {
        for p in [GENZ_LIGHT, GENZ_DARK, BUBBLEGUM_LIGHT, BUBBLEGUM_DARK] {
            assert_eq!(p.line, p.text);
        }
    }
}
