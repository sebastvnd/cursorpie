//! The choice of theme (Behavior, Look & feel): a row of cards that slides sideways, one card for
//! each theme, drawn with the theme's own colors and shape, so the choice is seen before it is made.
//!
//! - The row fills the width of the panel and is moved by dragging it, by the wheel or the
//!   trackpad, or by the two arrows; the chosen theme glides to the middle by itself.
//! - Clicking a card does not swap the colors in one frame: it starts the wave of
//!   [`theme_wave`], which grows from the card over the whole window.
//! - Under the row, one dot for each theme shows where the choice is, and the two buttons of the
//!   themes folder.

use super::theme::{Theme, registry};
use super::theme_wave;
use super::widgets::{Look, actions, button, icon_button, row_wide, tile};
use crate::ui::draw::smooth_approach;
use crate::ui::symbols;
use eframe::egui::{self, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Vec2};

/// Size of a card.
const CARD: Vec2 = Vec2::new(176.0, 128.0);
/// Space between two cards.
const SPACING: f32 = 14.0;
/// Room kept around the cards inside the row, for the hard shadow of a theme that has one.
const BLEED: f32 = 8.0;
/// Width of the fade at a side of the row where more cards wait.
const FADE: f32 = 36.0;
/// How fast the row glides to where it was sent (bigger is quicker).
const GLIDE: f32 = 11.0;

/// What the row remembers between frames.
#[derive(Clone, Copy, Default)]
struct Slide {
    /// Where the row is scrolled to, in points.
    offset: f32,
    /// Where it is gliding to, when it is.
    target: Option<f32>,
    /// The card that was the chosen one last frame: a new one makes the row glide to it.
    chosen: Option<usize>,
    /// Whether the row was placed for the first time.
    placed: bool,
}

/// Where to scroll for card `i` to be in the middle of a view `view` wide.
fn centered(i: usize, view: f32, count: usize) -> f32 {
    let step = CARD.x + SPACING;
    (i as f32 * step + CARD.x / 2.0 + BLEED - view / 2.0).clamp(0.0, max_offset(view, count))
}

/// Width of all the cards with the space between them.
fn cards_width(count: usize) -> f32 {
    count as f32 * (CARD.x + SPACING) - SPACING
}

/// The furthest the row can be scrolled.
fn max_offset(view: f32, count: usize) -> f32 {
    let content = cards_width(count) + 2.0 * BLEED;
    (content - view).max(0.0)
}

/// Draws what `theme` looks like in the appearance in use, into `rect`: a page, a sidebar, a panel
/// with its outline and its shadow, two rows (a pick and a switch) and the accent. Everything comes
/// from the theme's own tokens, and the drawing follows the size of `rect`.
fn preview(painter: &egui::Painter, rect: Rect, theme: &Theme, dark: bool) {
    let (p, s) = (theme.palette(dark), theme.shape);
    let accent = {
        let [r, g, b] = theme.accent;
        Color32::from_rgb(r, g, b)
    };
    painter.rect_filled(rect, 9, p.bg);
    let side = Rect::from_min_size(rect.min, Vec2::new(rect.width() * 0.2, rect.height()));
    painter.rect_filled(
        side,
        CornerRadius {
            nw: 9,
            sw: 9,
            ne: 0,
            se: 0,
        },
        p.sidebar,
    );
    // the entries of the sidebar: the first one is the chosen one
    for i in 0..3 {
        let y = side.min.y + 12.0 + i as f32 * 12.0;
        let w = side.width() - 14.0;
        let color = if i == 0 { accent } else { p.muted };
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(side.min.x + 7.0, y), Vec2::new(w, 5.0)),
            3,
            color.gamma_multiply(if i == 0 { 1.0 } else { 0.45 }),
        );
    }
    let card = Rect::from_min_max(
        Pos2::new(side.max.x + 9.0, rect.min.y + 10.0),
        rect.max - Vec2::new(10.0, 10.0),
    );
    let r = 6;
    if s.panel_drop.is_visible() {
        painter.rect_filled(card.translate(Vec2::new(3.0, 3.0)), r, p.line);
    }
    painter.rect_filled(card, r, p.card);
    let edge = if s.panel.is_visible() {
        Stroke::new(1.5, s.panel.tone.of(&p))
    } else {
        Stroke::new(1.0, p.border)
    };
    painter.rect_stroke(card, r, edge, egui::StrokeKind::Inside);
    // row 1: a dot and a line of text
    let y1 = card.min.y + card.height() * 0.30;
    painter.circle_filled(Pos2::new(card.min.x + 10.0, y1), 4.0, accent);
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(card.min.x + 20.0, y1 - 1.5),
            Vec2::new((card.width() - 32.0).max(4.0), 3.0),
        ),
        2,
        p.muted,
    );
    // row 2: a line of text and a switch that is on
    let y2 = card.min.y + card.height() * 0.68;
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(card.min.x + 10.0, y2 - 1.5),
            Vec2::new((card.width() * 0.4).max(4.0), 3.0),
        ),
        2,
        p.muted.gamma_multiply(0.7),
    );
    let track = Rect::from_center_size(Pos2::new(card.max.x - 20.0, y2), Vec2::new(22.0, 12.0));
    painter.rect_filled(track, 6, accent);
    painter.circle_filled(Pos2::new(track.max.x - 6.0, y2), 4.2, Color32::WHITE);
    painter.rect_stroke(
        rect,
        9,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );
}

/// A soft edge at a side of `rect`, in the color of the panel, to say that the row goes on.
fn fade(painter: &egui::Painter, rect: Rect, left: bool, color: Color32) {
    let (near, far) = if left {
        (rect.min.x, rect.min.x + FADE)
    } else {
        (rect.max.x, rect.max.x - FADE)
    };
    let mut mesh = egui::Mesh::default();
    let clear = Color32::TRANSPARENT;
    mesh.colored_vertex(Pos2::new(near, rect.min.y), color);
    mesh.colored_vertex(Pos2::new(far, rect.min.y), clear);
    mesh.colored_vertex(Pos2::new(far, rect.max.y), clear);
    mesh.colored_vertex(Pos2::new(near, rect.max.y), color);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// The theme row of Look & feel: the title with the two arrows, the sliding row of cards, the dots
/// and the buttons of the themes folder. Choosing a theme starts the wave; the theme itself is
/// written into `current` (and `accent` with it, unless the person chose an accent of their own)
/// when the wave covers the window, see [`theme_wave::drive`].
pub(in crate::ui::settings) fn theme_picker(
    ui: &mut egui::Ui,
    session: u64,
    current: &str,
    ui_accent: Color32,
) {
    let ctx = ui.ctx().clone();
    let themes = registry::all();
    let count = themes.len();
    let slide_id = ui.id().with("theme_slide");
    let mut slide: Slide = ctx.data(|d| d.get_temp(slide_id)).unwrap_or_default();

    let view = ui.available_width();
    let max = max_offset(view, count);
    let waiting = theme_wave::pending(&ctx);
    let shown = waiting.as_deref().unwrap_or(current);
    let chosen = themes.iter().position(|t| t.id == shown);

    // the title, and the two arrows that move the row a few cards at a time
    let mut jump: Option<f32> = None;
    row_wide(
        ui,
        "Theme",
        "Slide to look through them. Make your own in the themes folder.",
        88.0,
        |ui| {
            let from = slide.target.unwrap_or(slide.offset);
            let page = (view / (CARD.x + SPACING)).floor().max(1.0) * (CARD.x + SPACING);
            if icon_button(ui, symbols::CARET_RIGHT, "Next themes", from < max - 1.0).clicked() {
                jump = Some((from + page).clamp(0.0, max));
            }
            if icon_button(ui, symbols::CARET_LEFT, "Previous themes", from > 1.0).clicked() {
                jump = Some((from - page).clamp(0.0, max));
            }
        },
    );
    if jump.is_some() {
        slide.target = jump;
    }

    // the chosen card comes to the middle: at once the first time, gliding afterwards
    let fresh = !slide.placed;
    if let Some(i) = chosen {
        if !slide.placed {
            slide.offset = centered(i, view, count);
            slide.placed = true;
        } else if slide.chosen != Some(i) {
            slide.target = Some(centered(i, view, count));
        }
    }
    slide.chosen = chosen;
    // the glide moves the row by hand; when it is not gliding, the hand of the person has it
    let force = fresh || slide.target.is_some();
    if let Some(target) = slide.target {
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let mut at = slide.offset;
        if smooth_approach(&mut at, target, dt, GLIDE) && (target - at).abs() > 0.5 {
            slide.offset = at;
            ctx.request_repaint();
        } else {
            slide.offset = target;
            slide.target = None;
        }
    }

    // when every card fits, the row is centered; when not, it starts at the edge
    let lead = ((view - cards_width(count)) / 2.0).max(BLEED).floor();
    ui.add_space(2.0);
    let dark = crate::ui::draw::dark();
    let muted_text = super::theme::pal().text;
    let mut area = egui::ScrollArea::horizontal()
        .id_salt(slide_id)
        .auto_shrink([false, true])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    if force {
        area = area.horizontal_scroll_offset(slide.offset);
    }
    let mut picked: Option<(String, Pos2)> = None;
    let out = area.show(ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.horizontal(|ui| {
            ui.add_space(lead);
            // room above and below for the shadow of the selected card
            ui.vertical(|ui| {
                ui.add_space(BLEED / 2.0);
                ui.horizontal(|ui| {
                    for (i, theme) in themes.iter().enumerate() {
                        let selected = chosen == Some(i);
                        let (rect, resp) = tile(ui, CARD, selected, ui_accent);
                        let inner = rect.shrink(8.0);
                        let art = Rect::from_min_size(
                            inner.min,
                            Vec2::new(inner.width(), inner.height() - 26.0),
                        );
                        preview(ui.painter(), art, theme, dark);
                        let base = Pos2::new(inner.min.x + 2.0, art.max.y + 7.0);
                        ui.painter().text(
                            base,
                            egui::Align2::LEFT_TOP,
                            &theme.name,
                            FontId::proportional(13.5),
                            muted_text,
                        );
                        // a check that grows in the corner of the chosen card
                        let grow =
                            ui.ctx()
                                .animate_bool_with_time(resp.id.with("check"), selected, 0.22);
                        if grow > 0.01 {
                            let c = Pos2::new(inner.max.x - 9.0, base.y + 8.0);
                            ui.painter().circle_filled(c, 9.0 * grow, ui_accent);
                            ui.painter().text(
                                c,
                                egui::Align2::CENTER_CENTER,
                                symbols::CHECK,
                                FontId::new(11.0 * grow, crate::ui::fonts::icons()),
                                crate::ui::settings::theme::on_accent(ui_accent),
                            );
                        }
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
                            picked = Some((theme.id.clone(), rect.center()));
                        }
                        if i + 1 < count {
                            ui.add_space(SPACING);
                        }
                    }
                });
                ui.add_space(BLEED);
            });
            ui.add_space(BLEED);
        });
    });
    // what the row shows now (a drag moved it, or the glide did)
    slide.offset = out.state.offset.x;

    // a soft edge where more cards wait
    let card_color = super::theme::pal().card;
    let painter = ui.painter();
    if slide.offset > 1.0 {
        fade(painter, out.inner_rect, true, card_color);
    }
    if slide.offset < max - 1.0 {
        fade(painter, out.inner_rect, false, card_color);
    }

    if let Some((id, origin)) = picked {
        theme_wave::start(&ctx, session, &id, origin);
    }

    // one dot for each theme; the chosen one is a longer pill
    ui.add_space(10.0);
    let dot_h = 8.0;
    let widths: Vec<f32> = (0..count)
        .map(|i| {
            ctx.animate_value_with_time(
                slide_id.with(("dot", i)),
                if chosen == Some(i) { 22.0 } else { 8.0 },
                0.2,
            )
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 8.0 * count.saturating_sub(1) as f32;
    let (strip, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), dot_h), egui::Sense::hover());
    let mut x = strip.center().x - total / 2.0;
    for (i, w) in widths.iter().enumerate() {
        let color = if chosen == Some(i) {
            ui_accent
        } else {
            super::theme::pal().border
        };
        ui.painter().rect_filled(
            Rect::from_min_size(Pos2::new(x, strip.min.y), Vec2::new(*w, dot_h)),
            4,
            color,
        );
        x += w + 8.0;
    }
    ui.add_space(14.0);

    ctx.data_mut(|d| d.insert_temp(slide_id, slide));
}

/// The row of buttons under the cards, and the problems of the files.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_card_stays_at_the_start_and_the_last_at_the_end() {
        let view = 600.0;
        assert_eq!(centered(0, view, 6), 0.0);
        assert_eq!(centered(5, view, 6), max_offset(view, 6));
    }

    #[test]
    fn a_row_that_fits_does_not_scroll() {
        assert_eq!(max_offset(900.0, 4), 0.0);
        assert_eq!(centered(2, 900.0, 4), 0.0);
    }

    #[test]
    fn a_row_that_fits_has_the_same_room_on_both_sides() {
        let (view, count) = (780.0, 4);
        let lead = ((view - cards_width(count)) / 2.0).max(BLEED);
        let right = view - lead - cards_width(count);
        assert!((lead - right).abs() <= 1.0, "{lead} against {right}");
    }

    #[test]
    fn a_middle_card_is_centered_in_the_view() {
        let (view, count) = (500.0, 9);
        let i = 4;
        let at = centered(i, view, count);
        let middle = i as f32 * (CARD.x + SPACING) + CARD.x / 2.0 + BLEED - at;
        assert!((middle - view / 2.0).abs() < 1e-3);
    }
}
