//! The fonts of the interface: Inter for text, Phosphor for icons, Isometra for the name.
//!
//! Four families are registered on startup ([`install`]):
//!
//! | Family | Font | Use |
//! |---|---|---|
//! | `Proportional` | Inter Regular, then egui's own fonts | all text; the fallbacks keep emoji and other scripts that Inter does not hold |
//! | [`semibold`] | Inter SemiBold | titles and the names of things |
//! | [`brand`] | Isometra Regular | the name "CursorPie", the lettering of the logo |
//! | [`icons`] / [`icons_fill`] | Phosphor Regular / Fill | every icon, see [`symbols`](super::symbols) |
//!
//! The Inter and Phosphor files in `assets/fonts/` are cut down to what the interface needs (see
//! `scripts/subset-fonts.py`), about 380 KB for all five (Isometra is kept whole), and compiled into the binary: the app makes no
//! network connection and reads no font from the system, so it looks the same everywhere.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

static INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
static INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");
static ICONS_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Icons-Regular.ttf");
static ICONS_FILL: &[u8] = include_bytes!("../../assets/fonts/Icons-Fill.ttf");
static ISOMETRA: &[u8] = include_bytes!("../../assets/fonts/Isometra-Regular.ttf");

/// Name of the family for titles.
const SEMIBOLD: &str = "semibold";
/// Name of the family for the brand name.
const BRAND: &str = "isometra";
/// Name of the family for outline icons.
const ICONS: &str = "icons";
/// Name of the family for filled icons (the selected entry of the sidebar).
const ICONS_FILL_NAME: &str = "icons-fill";

/// Inter SemiBold: titles and the names of things.
pub fn semibold() -> FontFamily {
    FontFamily::Name(SEMIBOLD.into())
}

/// Isometra Regular: the lettering of the logo, used wherever the name "CursorPie" is a heading.
pub fn brand() -> FontFamily {
    FontFamily::Name(BRAND.into())
}

/// The name of the app in the brand font. Isometra has one weight and wide, geometric capitals, so
/// it is set a little smaller than a title of the same row would be.
pub fn brand_name(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size, brand()))
        .color(color)
}

/// Phosphor Regular: outline icons.
pub fn icons() -> FontFamily {
    FontFamily::Name(ICONS.into())
}

/// Phosphor Fill: the same icons, filled.
pub fn icons_fill() -> FontFamily {
    FontFamily::Name(ICONS_FILL_NAME.into())
}

/// Text in the title weight.
pub fn title(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .font(FontId::new(size, semibold()))
        .color(color)
}

/// An icon as text, for a label or a button that has no room for the painter.
pub fn icon(glyph: char, size: f32, color: Color32) -> RichText {
    RichText::new(glyph.to_string())
        .font(FontId::new(size, icons()))
        .color(color)
}

/// Draws an icon centered at `center`, in the outline or the filled weight.
///
/// The icon fonts are laid out so that the em square is the line box, so centering the text
/// centers the picture: there is no per-icon offset to maintain.
pub fn paint_icon(
    painter: &egui::Painter,
    center: egui::Pos2,
    glyph: char,
    size: f32,
    color: Color32,
    filled: bool,
) {
    let family = if filled { icons_fill() } else { icons() };
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        glyph,
        FontId::new(size, family),
        color,
    );
}

/// The font definitions: egui's defaults with ours first.
///
/// Kept apart from [`install`] so a test can build them without a window.
pub fn definitions() -> FontDefinitions {
    definitions_for(false)
}

/// [`definitions`], with `odd` telling which of two otherwise identical sets to build. The odd
/// one holds one more font file that no family uses. egui compares a new set of definitions with
/// the current one and ignores an equal one, so this is how [`reset_atlas`] makes it really
/// rebuild the atlas: it asks for the set that is not the current one.
fn definitions_for(odd: bool) -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    if odd {
        defs.font_data.insert(
            "Atlas-Epoch".to_owned(),
            Arc::new(FontData::from_static(ICONS_REGULAR)),
        );
    }
    for (name, bytes) in [
        ("Inter", INTER_REGULAR),
        ("Inter-SemiBold", INTER_SEMIBOLD),
        ("Icons-Regular", ICONS_REGULAR),
        ("Icons-Fill", ICONS_FILL),
        ("Isometra", ISOMETRA),
    ] {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }

    // Inter first; egui's fonts stay behind it as the fallback for what Inter does not have
    // (emoji used as slice icons, scripts it was not cut for).
    let fallback = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut text = vec!["Inter".to_owned()];
    text.extend(fallback.iter().cloned());
    let mut bold = vec!["Inter-SemiBold".to_owned()];
    bold.extend(text.iter().cloned());
    defs.families.insert(FontFamily::Proportional, text.clone());
    defs.families.insert(semibold(), bold);
    // Isometra first, Inter behind it for any character the name font lacks.
    let mut name = vec!["Isometra".to_owned()];
    name.extend(text.iter().cloned());
    defs.families.insert(brand(), name);
    defs.families
        .insert(icons(), vec!["Icons-Regular".to_owned()]);
    defs.families
        .insert(icons_fill(), vec!["Icons-Fill".to_owned()]);
    defs
}

/// Which of the two sets of definitions the context was given last (see [`definitions_for`]).
static ATLAS_ODD: AtomicBool = AtomicBool::new(false);

/// Registers the fonts. Call once, when the egui context is created.
pub fn install(ctx: &egui::Context) {
    ATLAS_ODD.store(false, Ordering::Relaxed);
    ctx.set_fonts(definitions());
}

/// Throws away every glyph cut so far and starts the font atlas again from nothing.
///
/// egui cuts each glyph once per size and never gives one back, so a size used once (the zoomed
/// text of the Flow canvas or the panel designer) would stay in memory, on the CPU and on the
/// GPU, until the program ends. Call it when what used those sizes is gone: the glyphs the pie
/// needs are cut again the next time it is drawn (a few small ones, so nothing is felt). The new
/// atlas is built by the next frame, which this asks for.
pub fn reset_atlas(ctx: &egui::Context) {
    let odd = !ATLAS_ODD.fetch_xor(true, Ordering::Relaxed);
    ctx.set_fonts(definitions_for(odd));
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::symbols;

    #[test]
    fn every_family_has_a_font() {
        let defs = definitions();
        for family in [
            FontFamily::Proportional,
            FontFamily::Monospace,
            semibold(),
            brand(),
            icons(),
            icons_fill(),
        ] {
            let list = defs.families.get(&family).expect("family is defined");
            assert!(!list.is_empty());
            for name in list {
                assert!(defs.font_data.contains_key(name), "{name} has no data");
            }
        }
        assert_eq!(
            defs.families[&FontFamily::Proportional][0],
            "Inter",
            "Inter leads the text family"
        );
        assert_eq!(
            defs.families[&brand()][0],
            "Isometra",
            "Isometra leads the brand family"
        );
    }

    /// Lays out a frame with the fonts installed, so a font that ab_glyph cannot parse fails here
    /// instead of on a user's screen, and every icon of the set must have a glyph in both weights.
    #[test]
    fn every_icon_has_a_glyph_in_both_weights() {
        let ctx = egui::Context::default();
        install(&ctx);
        let missing = std::cell::RefCell::new(Vec::new());
        let input = egui::RawInput::default();
        // nothing renders the output, so the texture uploads are dropped on purpose
        ctx.run_ui(input, |ui| {
            ui.ctx().fonts_mut(|fonts| {
                for family in [icons(), icons_fill()] {
                    let id = FontId::new(16.0, family);
                    for (name, glyph) in symbols::ALL {
                        if !fonts.has_glyph(&id, *glyph) {
                            missing.borrow_mut().push(*name);
                        }
                    }
                }
                let text = FontId::new(14.0, FontFamily::Proportional);
                assert!(fonts.has_glyph(&text, 'A'), "Inter has Latin");
                assert!(fonts.has_glyph(&text, '\u{2026}'), "Inter has the ellipsis");
                assert!(fonts.has_glyph(&text, '\u{203a}'), "Inter has the chevron");
                let name = FontId::new(30.0, brand());
                for c in "CursorPie".chars() {
                    assert!(fonts.has_glyph(&name, c), "Isometra has {c:?}");
                }
            });
        })
        .drop_without_applying_deltas();
        assert!(
            missing.borrow().is_empty(),
            "no glyph for {:?}",
            missing.borrow()
        );
    }
}
