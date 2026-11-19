//! The choice of theme (Behavior, Look & feel): one tile for each theme, drawn with the theme's own
//! colors and shape, so the choice is seen before it is made.

use super::theme::{Theme, registry};
use super::widgets::{Look, actions, button, tile};
use crate::ui::symbols;
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Vec2};

/// Size of a tile.
const TILE: Vec2 = Vec2::new(118.0, 82.0);

/// Draws what `theme` looks like in the appearance in use, into `rect`: a page, a sidebar, a panel
/// with its outline and its shadow, and the accent. Everything comes from the theme's own tokens.
fn preview(painter: &egui::Painter, rect: Rect, theme: &Theme, dark: bool) {
    let (p, s) = (theme.palette(dark), theme.shape);
    let page = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 42.0));
    painter.rect_filled(page, 7, p.bg);
    let side = Rect::from_min_size(page.min, Vec2::new(22.0, page.height()));
    painter.rect_filled(
        side,
        CornerRadius {
            nw: 7,
            sw: 7,
            ne: 0,
            se: 0,
        },
        p.sidebar,
    );
    let card = Rect::from_min_max(
        page.min + Vec2::new(29.0, 8.0),
        page.max - Vec2::new(10.0, 10.0),
    );
    let r = 5;
    if s.panel_drop.is_visible() {
        painter.rect_filled(card.translate(Vec2::new(2.5, 2.5)), r, p.line);
    }
    painter.rect_filled(card, r, p.card);
    let edge = if s.panel.is_visible() {
        Stroke::new(1.5, s.panel.tone.of(&p))
    } else {
        Stroke::new(1.0, p.border)
    };
    painter.rect_stroke(card, r, edge, egui::StrokeKind::Inside);
    let [ar, ag, ab] = theme.accent;
    painter.circle_filled(
        Pos2::new(card.min.x + 9.0, card.center().y),
        3.5,
        Color32::from_rgb(ar, ag, ab),
    );
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(card.min.x + 17.0, card.center().y - 1.5),
            Vec2::new((card.width() - 26.0).max(4.0), 3.0),
        ),
        2,
        p.muted,
    );
    painter.rect_stroke(
        page,
        7,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
}

/// The tiles of every theme, then the buttons that open the themes folder and read it again, and one
/// line for each file that could not be used. Choosing a theme also moves the accent to the one of
/// the theme, unless the person chose an accent of their own.
pub(in crate::ui::settings) fn theme_picker(
    ui: &mut egui::Ui,
    current: &mut String,
    accent: &mut [u8; 3],
    ui_accent: Color32,
) {
    let dark = crate::ui::draw::dark();
    let muted_text = super::theme::pal().text;
    // the row of controls is right-aligned, which would also reverse the tiles
    let tiles = egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true);
    ui.with_layout(tiles, |ui| {
        for theme in registry::all() {
            let selected = theme.id == *current;
            let (rect, resp) = tile(ui, TILE, selected, ui_accent);
            let inner = rect.shrink(8.0);
            preview(ui.painter(), inner, &theme, dark);
            ui.painter().text(
                Pos2::new(inner.min.x + 1.0, inner.min.y + 52.0),
                egui::Align2::LEFT_TOP,
                &theme.name,
                FontId::proportional(13.0),
                muted_text,
            );
            let resp = resp.on_hover_text(format!("{} theme", theme.name));
            resp.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::RadioButton,
                    true,
                    selected,
                    &theme.name,
                )
            });
            if resp.clicked() && !selected {
                // an accent nobody changed follows the theme; a chosen one stays
                if *accent == registry::find(current).accent {
                    *accent = theme.accent;
                }
                *current = theme.id.clone();
            }
        }
    });
}

/// The row of buttons under the tiles, and the problems of the files.
pub(in crate::ui::settings) fn theme_files(ui: &mut egui::Ui) {
    actions(ui, |ui| {
        if button(
            ui,
            Look::Soft,
            Some(symbols::FOLDER_OPEN),
            "Open themes folder",
        )
        .on_hover_text("Put a .json file here to add a theme")
        .clicked()
        {
            let dir = crate::config::themes_dir();
            let _ = std::fs::create_dir_all(&dir);
            crate::platform::open_path(&dir);
        }
        if button(
            ui,
            Look::Soft,
            Some(symbols::ARROW_CLOCKWISE),
            "Reload themes",
        )
        .on_hover_text("Read the folder again, after editing a file")
        .clicked()
        {
            registry::reload();
        }
    });
    for problem in registry::problems().iter().take(3) {
        super::widgets::note(ui, problem);
    }
}
