//! Small widgets reused by the editors: buttons, segmented controls, icon tabs, tiles, switches, sliders, the rows of a
//! settings panel, sidebar entries, badges, and the floating sheets and popovers.
//!
//! All of them take their colors and radii from [`theme`](super::theme), their icons from
//! [`symbols`], and are drawn here with the painter, so a control looks the
//! same wherever it is used and there is exactly one place to change how it looks.
//!
//! The vocabulary, from small to large:
//!
//! | Widget | Look |
//! |---|---|
//! | [`button`], [`icon_button`] | rounded, [`CONTROL_H`] high; [`Look`] picks primary, soft, plain or danger |
//! | [`round_button`] | a round disc with one icon, for an action that sits right next to what it acts on |
//! | [`segmented`] | a soft track with the chosen option raised on it, for two to four options |
//! | [`icon_tabs`] | icons sharing one line, a short accent bar under the current one, for a handful of views |
//! | [`select`], [`select_icons`] | a drop-down list in a popup card |
//! | [`menu_popup`] | a menu of [`menu_item`]s under a button, for actions rather than choices |
//! | a switch, [`slider`] | a round knob on a rail |
//! | [`row`], [`toggle_row`], [`choice_row`], [`slider_row`] | the rows of a [`panel`]: a label, an optional hint and one control |
//! | [`panel`] | a titled white card holding rows; every tab is a stack of these |
//! | [`sheet`], [`popover`] | what floats: a modal sheet over the window, and a small popup under a button |

use super::brand;
use super::theme::*;
use crate::keys::Shortcut;
use crate::ui::draw::ring_slice;
use crate::ui::fonts;
use crate::ui::symbols;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, Vec2};
use std::ops::RangeInclusive;

/// Size of the icon inside a button.
const BUTTON_ICON: f32 = 17.0;
/// Space between the icon and the label of a button.
const ICON_GAP: f32 = 8.0;
/// Vertical room above and below the content of a row.
const ROW_PAD: f32 = 11.0;

// ------------------------------------------------------------------ painting helpers

/// The color of `c` made `amount` (0 to 1) darker, for the pressed state of a filled control.
fn darken(c: Color32, amount: f32) -> Color32 {
    c.lerp_to_gamma(Color32::BLACK, amount)
}

/// Lays `text` out on one line, for measuring and painting by hand.
fn galley(ui: &egui::Ui, text: &str, font: FontId, color: Color32) -> std::sync::Arc<egui::Galley> {
    ui.painter().layout_no_wrap(text.to_owned(), font, color)
}

/// Lays `text` out on one line that is at most `max_w` wide. A longer text is cut and ends in an
/// ellipsis, so it never paints over the caret, the check or the edge of what holds it. The bool
/// says whether the text had to be cut (the caller can then show the whole of it as a tooltip).
pub(in crate::ui::settings) fn fit_galley(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    max_w: f32,
) -> (std::sync::Arc<egui::Galley>, bool) {
    let max_w = max_w.max(0.0);
    let whole = galley(ui, text, font.clone(), color);
    if whole.size().x <= max_w {
        return (whole, false);
    }
    // the most characters that still fit together with the ellipsis (the width only grows with
    // the length, so a binary search finds it in a few steps)
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        let head: String = chars[..mid].iter().collect();
        let probe = format!("{}\u{2026}", head.trim_end());
        if galley(ui, &probe, font.clone(), color).size().x <= max_w {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let head: String = chars[..lo].iter().collect();
    let cut = format!("{}\u{2026}", head.trim_end());
    (galley(ui, &cut, font, color), true)
}

/// The text font at `size`, in the regular or the title weight.
pub(in crate::ui::settings) fn text_font(size: f32, bold: bool) -> FontId {
    if bold {
        FontId::new(size, fonts::semibold())
    } else {
        FontId::proportional(size)
    }
}

/// The ring that shows keyboard focus on a control.
fn focus_ring(ui: &egui::Ui, rect: Rect, radius: egui::CornerRadius, accent: Color32) {
    ui.painter().rect_stroke(
        rect.expand(2.0),
        radius,
        Stroke::new(2.0, wash(accent, 0.55)),
        egui::StrokeKind::Outside,
    );
}

// ------------------------------------------------------------------ buttons

/// How a [`button`] looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui::settings) enum Look {
    /// Filled with the accent: the one thing to do on this screen.
    Primary(Color32),
    /// A soft grey fill: the other things to do.
    Soft,
    /// Nothing until the pointer is over it: toolbars.
    Plain,
    /// Red text on a pale red wash while hovered: destructive actions.
    Danger,
}

impl Look {
    /// `(fill at rest, fill under the pointer, text color)`.
    fn colors(self) -> (Color32, Color32, Color32) {
        self.colors_in(pal())
    }

    /// [`Look::colors`] in the palette `p`.
    fn colors_in(self, p: &Palette) -> (Color32, Color32, Color32) {
        match self {
            Look::Primary(accent) => (accent, darken(accent, 0.12), on_accent(accent)),
            Look::Soft => (p.idle, p.hot, p.text),
            Look::Plain => (Color32::TRANSPARENT, p.idle, p.text),
            Look::Danger => (Color32::TRANSPARENT, wash(p.bad, 0.10), p.bad),
        }
    }
}

/// Paints the frame of a button-like control and returns the color its content should use.
pub(in crate::ui::settings) fn button_frame(
    ui: &egui::Ui,
    rect: Rect,
    resp: &egui::Response,
    look: Look,
) -> Color32 {
    let enabled = ui.is_enabled();
    let (rest, hot, fg) = look.colors();
    let fill = if !enabled {
        if matches!(look, Look::Primary(_)) {
            pal().idle
        } else {
            Color32::TRANSPARENT
        }
    } else if resp.is_pointer_button_down_on() {
        darken(hot, 0.06)
    } else if resp.hovered() {
        hot
    } else {
        rest
    };
    ui.painter().rect_filled(rect, CONTROL_R, fill);
    if resp.has_focus()
        && let Look::Primary(accent) = look
    {
        focus_ring(ui, rect, CONTROL_R, accent);
    }
    if enabled {
        fg
    } else {
        pal().muted.gamma_multiply(0.7)
    }
}

/// A rounded button with an optional icon in front of its label.
///
/// It follows the enabled state of the `ui` (wrap it in `add_enabled_ui`, or use
/// [`button_if`]), and keeps reporting hovers when disabled so it can still carry a tooltip.
pub(in crate::ui::settings) fn button(
    ui: &mut egui::Ui,
    look: Look,
    icon: Option<char>,
    label: &str,
) -> egui::Response {
    let bold = matches!(look, Look::Primary(_));
    let probe = Color32::PLACEHOLDER;
    let text = (!label.is_empty()).then(|| galley(ui, label, text_font(14.0, bold), probe));
    let glyph = icon.map(|c| {
        galley(
            ui,
            &c.to_string(),
            FontId::new(BUTTON_ICON, fonts::icons()),
            probe,
        )
    });
    let text_w = text.as_ref().map_or(0.0, |g| g.size().x);
    let icon_w = glyph.as_ref().map_or(0.0, |g| g.size().x);
    let inner = text_w
        + icon_w
        + if text.is_some() && glyph.is_some() {
            ICON_GAP
        } else {
            0.0
        };
    // an icon on its own is a square
    let pad = if text.is_some() { 16.0 } else { 0.0 };
    let width = (inner + 2.0 * pad).ceil().max(CONTROL_H);
    let sense = if ui.is_enabled() {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CONTROL_H), sense);
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let fg = button_frame(ui, rect, &resp, look);
    let painter = ui.painter();
    let mut x = rect.center().x - inner / 2.0;
    if let Some(g) = glyph {
        let at = Pos2::new(x, rect.center().y - g.size().y / 2.0);
        painter.galley(at.round(), g, fg);
        x += icon_w + ICON_GAP;
    }
    if let Some(g) = text {
        let at = Pos2::new(x, rect.center().y - g.size().y / 2.0);
        painter.galley(at.round(), g, fg);
    }
    if ui.is_enabled() {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// [`button`], disabled unless `enabled`.
pub(in crate::ui::settings) fn button_if(
    ui: &mut egui::Ui,
    enabled: bool,
    look: Look,
    icon: Option<char>,
    label: &str,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| button(ui, look, icon, label))
        .inner
}

/// A line of text that acts as a link: the accent color and an arrow after it, underlined and with
/// the arrow nudged while the pointer is over it. It has no box, so it reads as part of the text
/// around it (a footer, a sentence) and not as one more button.
pub(in crate::ui::settings) fn link(
    ui: &mut egui::Ui,
    label: &str,
    accent: Color32,
) -> egui::Response {
    const SIZE: f32 = 12.0;
    let text = galley(ui, label, text_font(SIZE, true), Color32::PLACEHOLDER);
    let arrow = galley(
        ui,
        &symbols::ARROW_RIGHT.to_string(),
        FontId::new(SIZE, fonts::icons()),
        Color32::PLACEHOLDER,
    );
    let size = Vec2::new(
        text.size().x + ICON_GAP / 2.0 + arrow.size().x + 2.0,
        text.size().y.max(arrow.size().y),
    );
    let accent = accent_text(accent);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, label));
    let hot = resp.hovered() || resp.has_focus();
    let (text_w, arrow_h) = (text.size().x, arrow.size().y);
    let painter = ui.painter();
    let top = rect.center().y - text.size().y / 2.0;
    painter.galley(Pos2::new(rect.min.x, top).round(), text, accent);
    let nudge = if hot { 2.0 } else { 0.0 };
    let arrow_at = Pos2::new(
        rect.min.x + text_w + ICON_GAP / 2.0 + nudge,
        rect.center().y - arrow_h / 2.0,
    );
    painter.galley(arrow_at.round(), arrow, accent);
    if hot {
        let y = rect.center().y + SIZE / 2.0 + 1.0;
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.min.x + text_w, y)],
            Stroke::new(1.0, accent),
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A square button with one icon and a tooltip. Plain until the pointer is over it.
pub(in crate::ui::settings) fn icon_button(
    ui: &mut egui::Ui,
    icon: char,
    tip: &str,
    enabled: bool,
) -> egui::Response {
    icon_button_look(ui, Look::Plain, icon, tip, enabled)
}

/// [`icon_button`] in another [`Look`].
pub(in crate::ui::settings) fn icon_button_look(
    ui: &mut egui::Ui,
    look: Look,
    icon: char,
    tip: &str,
    enabled: bool,
) -> egui::Response {
    let resp = button_if(ui, enabled, look, Some(icon), "");
    if enabled {
        resp.on_hover_text(tip)
    } else {
        resp.on_disabled_hover_text(tip)
    }
}

/// A round button, `size` across, with one icon and a tooltip: a soft grey disc that darkens under
/// the pointer. [`Look::Danger`] keeps its red glyph and turns the disc pale red while hovered. Meant
/// for the few actions that belong right next to what they act on (add or delete a slice under the pie).
pub(in crate::ui::settings) fn round_button(
    ui: &mut egui::Ui,
    look: Look,
    icon: char,
    tip: &str,
    enabled: bool,
    size: f32,
) -> egui::Response {
    let resp = ui
        .add_enabled_ui(enabled, |ui| {
            let sense = if ui.is_enabled() {
                egui::Sense::click()
            } else {
                egui::Sense::hover()
            };
            let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), sense);
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tip)
            });
            let (rest, hot, fg) = look.colors();
            // the disc is always there, so the button is easy to find; only the glyph says what it does
            let rest = if rest == Color32::TRANSPARENT {
                pal().idle
            } else {
                rest
            };
            let on = ui.is_enabled();
            let fill = if !on {
                pal().idle.gamma_multiply(0.6)
            } else if resp.is_pointer_button_down_on() {
                darken(hot, 0.06)
            } else if resp.hovered() {
                hot
            } else {
                rest
            };
            let fg = if on {
                fg
            } else {
                pal().muted.gamma_multiply(0.7)
            };
            let painter = ui.painter();
            painter.circle_filled(rect.center(), size / 2.0, fill);
            fonts::paint_icon(painter, rect.center(), icon, BUTTON_ICON + 1.0, fg, false);
            if on {
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
            } else {
                resp
            }
        })
        .inner;
    if enabled {
        resp.on_hover_text(tip)
    } else {
        resp.on_disabled_hover_text(tip)
    }
}

/// How long the circle of an [`icon_switch`] takes to travel, in seconds.
const SWITCH_SECS: f64 = 0.28;

/// A switch between two or more pages, with icons only: one round track, one round cell for each
/// page, and a circle in the accent color under the chosen one. When another is picked the circle
/// travels to it, stretching on the way (the edge it moves towards leaves first, the other follows)
/// while the icons change color as it passes. `items` are `(icon, tooltip)`; `cell` is the width of
/// one round cell (the track is 6 points higher); `track_fill` is the color of the track (the color
/// of a floating bar that is itself the track).
///
/// Returns the index that was clicked, if it is not the chosen one.
pub(in crate::ui::settings) fn icon_switch(
    ui: &mut egui::Ui,
    items: &[(char, String)],
    selected: usize,
    accent: Color32,
    track_fill: Color32,
    cell: f32,
) -> Option<usize> {
    let n = items.len();
    let size = Vec2::new(
        n as f32 * cell + n.saturating_sub(1) as f32 * 2.0 + 6.0,
        cell + 6.0,
    );
    let (track, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let cell_at = |i: usize| {
        Rect::from_min_size(
            Pos2::new(
                track.min.x + 3.0 + i as f32 * (cell + 2.0),
                track.min.y + 3.0,
            ),
            Vec2::splat(cell),
        )
    };
    let mut picked = None;
    let mut hot = vec![false; n];
    for (i, (_, tip)) in items.iter().enumerate() {
        let resp = ui.interact(
            cell_at(i),
            ui.id().with(("icon_switch", i)),
            egui::Sense::click(),
        );
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, i == selected, tip)
        });
        hot[i] = resp.hovered();
        let resp = resp
            .on_hover_text(tip)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if resp.clicked() && i != selected {
            picked = Some(i);
        }
    }
    // the move that is going on: from which cell, to which, since when
    let now = ui.input(|i| i.time);
    let key = ui.id().with("icon_switch_move");
    let (from, to, since) = ui
        .data(|d| d.get_temp::<(usize, usize, f64)>(key))
        .unwrap_or((selected, selected, -1.0e9));
    let (from, to, since) = if to != selected {
        (to, selected, now)
    } else {
        (from, to, since)
    };
    ui.data_mut(|d| d.insert_temp(key, (from, to, since)));
    let u = (((now - since) / SWITCH_SECS) as f32).clamp(0.0, 1.0);
    if u < 1.0 {
        ui.ctx().request_repaint();
    }
    let lead = egui::emath::easing::cubic_out(u);
    let lag = egui::emath::easing::cubic_in(u);
    let (a, b) = (cell_at(from), cell_at(to));
    let right = b.min.x >= a.min.x;
    let left_edge = a.min.x + (b.min.x - a.min.x) * if right { lag } else { lead };
    let right_edge = a.max.x + (b.max.x - a.max.x) * if right { lead } else { lag };
    let thumb = Rect::from_min_max(
        Pos2::new(left_edge, a.min.y),
        Pos2::new(right_edge, a.max.y),
    );
    let painter = ui.painter();
    painter.rect_filled(track, PILL, track_fill);
    for (i, h) in hot.iter().enumerate() {
        // the wash under the pointer, on a cell that is not chosen
        if *h && i != selected {
            painter.circle_filled(cell_at(i).center(), cell / 2.0, pal().hot);
        }
    }
    painter.rect_filled(thumb, PILL, accent);
    for (i, (icon, _)) in items.iter().enumerate() {
        let near = if from == to {
            f32::from(i == to)
        } else if i == to {
            lead
        } else if i == from {
            1.0 - lead
        } else {
            0.0
        };
        let ink = pal().muted.lerp_to_gamma(on_accent(accent), near);
        fonts::paint_icon(painter, cell_at(i).center(), *icon, cell * 0.56, ink, false);
    }
    picked
}

/// Room added around the content of one option of a [`segmented`] control, per side.
const SEG_PAD: f32 = 12.0;

/// A control to pick one of two to four options: a soft track with the chosen one raised on it
/// (white on the light appearance, a lighter graphite on the dark one). Calmer than a row of
/// separate pills, and it never wraps, so nothing jumps when the window is resized: if the icons
/// do not fit in the width left, they are dropped and only the names stay.
///
/// Returns the index that was clicked, if it is not the selected one.
pub(in crate::ui::settings) fn segmented(
    ui: &mut egui::Ui,
    items: &[(Option<char>, &str)],
    selected: usize,
) -> Option<usize> {
    const TRACK_PAD: f32 = 4.0;
    // measured in the bold weight, so choosing another option never changes any width
    let widths = |with_icons: bool| -> Vec<f32> {
        items
            .iter()
            .map(|(icon, label)| {
                let text = galley(ui, label, text_font(13.5, true), pal().text)
                    .size()
                    .x;
                let glyph = icon.filter(|_| with_icons).map_or(0.0, |c| {
                    galley(
                        ui,
                        &c.to_string(),
                        FontId::new(15.0, fonts::icons()),
                        pal().text,
                    )
                    .size()
                    .x + 6.0
                });
                (text + glyph + 2.0 * SEG_PAD).ceil()
            })
            .collect()
    };
    let mut with_icons = true;
    let mut w = widths(true);
    if w.iter().sum::<f32>() + 2.0 * TRACK_PAD > ui.available_width() {
        with_icons = false;
        w = widths(false);
    }
    let total = w.iter().sum::<f32>() + 2.0 * TRACK_PAD;
    let (track, _) = ui.allocate_exact_size(Vec2::new(total, CONTROL_H), egui::Sense::hover());
    ui.painter().rect_filled(track, PILL, pal().idle);
    let raised = if pal().dark { pal().hot } else { pal().card };

    let mut picked = None;
    let mut x = track.min.x + TRACK_PAD;
    for (i, ((icon, label), width)) in items.iter().zip(&w).enumerate() {
        let seg = Rect::from_min_size(
            Pos2::new(x, track.min.y + TRACK_PAD),
            Vec2::new(*width, CONTROL_H - 2.0 * TRACK_PAD),
        );
        x += width;
        let resp = ui
            .interact(
                seg,
                ui.id().with(("segment", i, *label)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, i == selected, *label)
        });
        let on = i == selected;
        if on {
            ui.painter().rect_filled(seg, PILL, raised);
            ui.painter().rect_stroke(
                seg,
                PILL,
                Stroke::new(1.0, pal().border),
                egui::StrokeKind::Inside,
            );
        }
        if resp.has_focus() {
            focus_ring(ui, seg, PILL, pal().text);
        }
        let fg = if on || resp.hovered() {
            pal().text
        } else {
            pal().muted
        };
        let text = galley(ui, label, text_font(13.5, on), fg);
        let glyph = icon
            .filter(|_| with_icons)
            .map(|c| galley(ui, &c.to_string(), FontId::new(15.0, fonts::icons()), fg));
        let content = text.size().x + glyph.as_ref().map_or(0.0, |g| g.size().x + 6.0);
        let mut cx = seg.center().x - content / 2.0;
        if let Some(g) = glyph {
            let gw = g.size().x;
            ui.painter().galley(
                Pos2::new(cx, seg.center().y - g.size().y / 2.0).round(),
                g,
                Color32::PLACEHOLDER,
            );
            cx += gw + 6.0;
        }
        ui.painter().galley(
            Pos2::new(cx, seg.center().y - text.size().y / 2.0).round(),
            text,
            Color32::PLACEHOLDER,
        );
        if resp.clicked() && !on {
            picked = Some(i);
        }
    }
    picked
}

/// A row of views to switch between, as icons only: they share `width` equally, the current one
/// in the strong color with a short accent bar under it, the others muted. Nothing is cut off
/// however many there are, and each icon names its view in a tooltip.
///
/// Returns the index that was clicked, if it is not the selected one.
pub(in crate::ui::settings) fn icon_tabs(
    ui: &mut egui::Ui,
    items: &[(char, &str)],
    selected: usize,
    accent: Color32,
    width: f32,
) -> Option<usize> {
    const H: f32 = 38.0;
    let mut picked = None;
    let each = (width / items.len().max(1) as f32).floor();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, (icon, label)) in items.iter().enumerate() {
            let on = i == selected;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(each, H), egui::Sense::click());
            let resp = resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(*label);
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, *label)
            });
            if resp.hovered() && !on {
                ui.painter()
                    .rect_filled(rect.shrink2(Vec2::new(3.0, 3.0)), CONTROL_R, pal().idle);
            }
            if resp.has_focus() {
                focus_ring(ui, rect.shrink(3.0), CONTROL_R, accent);
            }
            let fg = if on || resp.hovered() {
                pal().text
            } else {
                pal().muted
            };
            fonts::paint_icon(
                ui.painter(),
                Pos2::new(rect.center().x, rect.center().y - 2.0),
                *icon,
                19.0,
                fg,
                false,
            );
            if on {
                let bar = Rect::from_center_size(
                    Pos2::new(rect.center().x, rect.max.y - 2.0),
                    Vec2::new(18.0, 3.0),
                );
                ui.painter().rect_filled(bar, PILL, accent);
            }
            if resp.clicked() && !on {
                picked = Some(i);
            }
        }
    });
    picked
}

/// A clickable square. The caller draws its content into the returned `rect`.
///
/// The selected tile gets a 2 px accent border and a faint wash of the accent; a hovered one
/// gets the hover fill.
pub(in crate::ui::settings) fn tile(
    ui: &mut egui::Ui,
    size: Vec2,
    selected: bool,
    accent: Color32,
) -> (Rect, egui::Response) {
    tile_sense(ui, size, selected, accent, egui::Sense::click())
}

/// [`tile`] that also answers to `sense` (a tile that can be dragged: `Sense::click_and_drag()`).
pub(in crate::ui::settings) fn tile_sense(
    ui: &mut egui::Ui,
    size: Vec2,
    selected: bool,
    accent: Color32,
    sense: egui::Sense,
) -> (Rect, egui::Response) {
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let radius = egui::CornerRadius::same(14);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, radius, wash(accent, 0.10));
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, accent),
            egui::StrokeKind::Inside,
        );
    } else {
        painter.rect_filled(
            rect,
            radius,
            if resp.hovered() {
                pal().hot
            } else {
                pal().idle
            },
        );
    }
    (rect, resp)
}

// ------------------------------------------------------------------ switch and slider

/// Size of the track of a switch.
const SWITCH: Vec2 = Vec2::new(46.0, 28.0);

/// Paints a switch into `rect`, `t` going from 0 (off) to 1 (on).
fn paint_switch(painter: &egui::Painter, rect: Rect, t: f32, accent: Color32) {
    painter.rect_filled(rect, PILL, pal().track.lerp_to_gamma(accent, t));
    let r = rect.height() / 2.0 - 3.0;
    let x = egui::lerp((rect.min.x + 3.0 + r)..=(rect.max.x - 3.0 - r), t);
    let c = Pos2::new(x, rect.center().y);
    // a soft edge under the knob stands in for a shadow
    painter.circle_filled(
        c + Vec2::new(0.0, 1.0),
        r + 1.0,
        Color32::from_black_alpha(26),
    );
    painter.circle_filled(c, r, Color32::WHITE);
}

/// Where along a rail of `0..=1` the value `v` of `range` sits.
fn slider_fraction(v: f64, range: (f64, f64)) -> f32 {
    let span = range.1 - range.0;
    if span <= 0.0 {
        0.0
    } else {
        (((v - range.0) / span) as f32).clamp(0.0, 1.0)
    }
}

/// The "nice" numbers steps and anchors are taken from: 1, 2, 5, 10, 20, 50, 100, ... (and the
/// tenths below one, for a slider of small fractional numbers). With `quarters`, 25, 250, ... too,
/// which suit marks (0, 25, 50, 75, 100) but not steps (2.5 is no whole number).
fn nice_numbers(integral: bool, quarters: bool) -> impl Iterator<Item = f64> {
    let tenths: &[f64] = if integral { &[] } else { &[0.1, 0.2, 0.5] };
    tenths.iter().copied().chain((0..9).flat_map(move |e| {
        let k = 10f64.powi(e);
        [
            Some(k),
            Some(2.0 * k),
            quarters.then_some(2.5 * k),
            Some(5.0 * k),
        ]
        .into_iter()
        .flatten()
    }))
}

/// The smallest nice number that is at least `at_least`.
fn nice_at_least(at_least: f64, integral: bool, quarters: bool) -> f64 {
    nice_numbers(integral, quarters)
        .find(|n| *n >= at_least)
        .unwrap_or(at_least)
}

/// How a slider is anchored: the `step` its value moves in, and the `majors`, the values that get a
/// long mark and a number. The ends of the range are always majors.
#[derive(Clone, Debug, PartialEq)]
struct Anchors {
    step: f64,
    majors: Vec<f64>,
}

/// Anchors for a slider over `bounds`: about 20 to 40 steps in all, and a major mark about every
/// fifth of the range, at round numbers (a slider from 150 to 500 gets 200, 300 and 400 besides its
/// ends), never so close to an end that two numbers would touch.
fn slider_anchors(bounds: (f64, f64), integral: bool) -> Anchors {
    let (lo, hi) = bounds;
    let span = hi - lo;
    if span <= 0.0 {
        return Anchors {
            step: 1.0,
            majors: vec![lo],
        };
    }
    let step = nice_at_least(span / 40.0, integral, false);
    let major = nice_at_least(span / 5.0, integral, true).max(step);
    let mut majors = vec![lo];
    let mut m = (lo / major).floor() * major + major;
    while m < hi - span * 0.07 {
        if m > lo + span * 0.07 {
            majors.push(m);
        }
        m += major;
    }
    majors.push(hi);
    Anchors { step, majors }
}

/// The value the pointer at `fraction` of the rail means: the nearest of the grid of `step`, the
/// start and the end of the range. `free` (Shift held) skips the grid.
fn slider_snap(fraction: f32, bounds: (f64, f64), step: f64, integral: bool, free: bool) -> f64 {
    let raw = bounds.0 + (bounds.1 - bounds.0) * f64::from(fraction.clamp(0.0, 1.0));
    if free {
        return if integral { raw.round() } else { raw };
    }
    let grid = ((raw / step).round() * step).clamp(bounds.0, bounds.1);
    [grid, bounds.0, bounds.1]
        .into_iter()
        .min_by(|a, b| (raw - a).abs().total_cmp(&(raw - b).abs()))
        .map_or(grid, |v| if integral { v.round() } else { v })
}

/// A rail with a round knob, as wide as the space left, with marks under it (see
/// [`slider_anchors`]). Dragging or clicking sets `value`, snapped to the marks (hold Shift to move
/// freely); the arrow keys move it by one step while the slider has the focus.
pub(in crate::ui::settings) fn slider<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    value: &mut Num,
    range: RangeInclusive<Num>,
    accent: Color32,
) -> egui::Response {
    const RAIL_H: f32 = 6.0;
    const KNOB: f32 = 11.0;
    let bounds = (range.start().to_f64(), range.end().to_f64());
    let anchors = slider_anchors(bounds, Num::INTEGRAL);
    // room for the rail with its knob, the marks under it and their numbers
    let size = Vec2::new(ui.available_width().max(60.0), 46.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let rail = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.top() + 15.0),
        Vec2::new((rect.width() - 2.0 * KNOB).max(1.0), RAIL_H),
    );

    if let Some(p) = resp.interact_pointer_pos()
        && (resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on())
    {
        let f = ((p.x - rail.min.x) / rail.width().max(1.0)).clamp(0.0, 1.0);
        // Shift moves freely; otherwise the value snaps to the marks, which is what makes a round
        // number easy to land on
        let free = ui.input(|i| i.modifiers.shift);
        let new = slider_snap(f, bounds, anchors.step, Num::INTEGRAL, free);
        if new != value.to_f64() {
            *value = Num::from_f64(new);
            resp.mark_changed();
        }
    }
    if resp.has_focus() {
        let step = anchors.step;
        let dir = ui.input(|i| {
            i64::from(i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp))
                - i64::from(
                    i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown),
                )
        });
        if dir != 0 {
            let new = (value.to_f64() + step * dir as f64).clamp(bounds.0, bounds.1);
            *value = Num::from_f64(new);
            resp.mark_changed();
        }
    }
    resp.widget_info(|| egui::WidgetInfo::slider(true, value.to_f64(), ""));

    let f = slider_fraction(value.to_f64(), bounds);
    let x = rail.min.x + rail.width() * f;
    let painter = ui.painter();
    // the marks: a small one at every step, a long one with its number at the majors
    let at = |v: f64| rail.min.x + rail.width() * slider_fraction(v, bounds);
    let steps = ((bounds.1 - bounds.0) / anchors.step).floor() as usize;
    if (1..=60).contains(&steps) {
        let mut v = (bounds.0 / anchors.step).ceil() * anchors.step;
        while v <= bounds.1 + 1e-9 {
            painter.line_segment(
                [
                    Pos2::new(at(v), rail.max.y + 5.0),
                    Pos2::new(at(v), rail.max.y + 8.0),
                ],
                Stroke::new(1.0, pal().border),
            );
            v += anchors.step;
        }
    }
    for m in &anchors.majors {
        let on = (value.to_f64() - m).abs() < 1e-6;
        painter.line_segment(
            [
                Pos2::new(at(*m), rail.max.y + 5.0),
                Pos2::new(at(*m), rail.max.y + 12.0),
            ],
            Stroke::new(1.5, if on { accent } else { pal().muted }),
        );
        let text = if Num::INTEGRAL || m.fract() == 0.0 {
            format!("{}", *m as i64)
        } else {
            format!("{m:.1}")
        };
        painter.text(
            Pos2::new(at(*m), rail.max.y + 14.0),
            egui::Align2::CENTER_TOP,
            text,
            FontId::proportional(10.5),
            if on { accent_text(accent) } else { pal().muted },
        );
    }
    painter.rect_filled(rail, PILL, pal().track);
    let filled = Rect::from_min_max(rail.min, Pos2::new(x, rail.max.y));
    painter.rect_filled(filled, PILL, accent);
    let c = Pos2::new(x, rail.center().y);
    let grow = if resp.hovered() || resp.dragged() {
        1.0
    } else {
        0.0
    };
    painter.circle_filled(
        c + Vec2::new(0.0, 1.5),
        KNOB + grow + 1.0,
        Color32::from_black_alpha(30),
    );
    painter.circle_filled(c, KNOB + grow, Color32::WHITE);
    painter.circle_stroke(c, KNOB + grow, Stroke::new(1.0, wash(Color32::BLACK, 0.08)));
    if resp.has_focus() {
        painter.circle_stroke(c, KNOB + grow + 2.0, Stroke::new(2.0, wash(accent, 0.55)));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ------------------------------------------------------------------ sidebar and headings

/// Height of one sidebar entry.
pub(in crate::ui::settings) const NAV_H: f32 = 44.0;

/// The wash behind the chosen entry of the sidebar, which travels from the old entry to the new
/// one when another is picked: the edge it moves towards leaves first and the other follows, so
/// it stretches on the way and settles, in [`SWITCH_SECS`]. It is drawn first, so the entries are
/// painted on top of it. The entries are `count` rows of [`NAV_H`], `gap` apart, from `top_left`
/// down, `width` wide.
///
/// Returns for each entry how much it is the chosen one (0 to 1): the entries use it to blend their
/// icon and title between the quiet color and the accent while the wash passes.
pub(in crate::ui::settings) fn nav_highlight(
    ui: &mut egui::Ui,
    top_left: Pos2,
    width: f32,
    gap: f32,
    selected: usize,
    count: usize,
    accent: Color32,
) -> Vec<f32> {
    let row = |i: usize| {
        Rect::from_min_size(
            Pos2::new(top_left.x, top_left.y + i as f32 * (NAV_H + gap)),
            Vec2::new(width, NAV_H),
        )
    };
    let now = ui.input(|i| i.time);
    let key = ui.id().with("nav_highlight_move");
    let (from, to, since) = ui
        .data(|d| d.get_temp::<(usize, usize, f64)>(key))
        .unwrap_or((selected, selected, -1.0e9));
    let (from, to, since) = if to != selected {
        (to, selected, now)
    } else {
        (from, to, since)
    };
    ui.data_mut(|d| d.insert_temp(key, (from, to, since)));
    let u = (((now - since) / SWITCH_SECS) as f32).clamp(0.0, 1.0);
    if u < 1.0 {
        ui.ctx().request_repaint();
    }
    let lead = egui::emath::easing::cubic_out(u);
    let lag = egui::emath::easing::cubic_in(u);
    let (a, b) = (row(from), row(to));
    let down = b.min.y >= a.min.y;
    let top = a.min.y + (b.min.y - a.min.y) * if down { lag } else { lead };
    let bottom = a.max.y + (b.max.y - a.max.y) * if down { lead } else { lag };
    let wash_rect = Rect::from_min_max(Pos2::new(a.min.x, top), Pos2::new(a.max.x, bottom));
    ui.painter()
        .rect_filled(wash_rect, CONTROL_R, wash(accent, 0.14));
    (0..count)
        .map(|i| {
            if from == to {
                f32::from(i == to)
            } else if i == to {
                lead
            } else if i == from {
                1.0 - lead
            } else {
                0.0
            }
        })
        .collect()
}

/// One entry of the sidebar: an icon and a title, as wide as the sidebar. Folded (`compact`), only
/// the icon is drawn and the title is the tooltip.
///
/// The chosen entry has the filled version of its icon in the accent color and the title weight;
/// the others are quiet and light up under the pointer. The wash behind the chosen one is not
/// drawn here but by [`nav_highlight`], which also says how much this entry is the chosen one
/// (`near`, 0 to 1) so the colors change as the wash passes.
pub(in crate::ui::settings) fn nav_item(
    ui: &mut egui::Ui,
    icon: char,
    title: &str,
    near: f32,
    accent: Color32,
    compact: bool,
) -> egui::Response {
    let on = near > 0.5;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), NAV_H), egui::Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, on, title)
    });
    let painter = ui.painter();
    if near < 0.05 && resp.hovered() {
        painter.rect_filled(rect, CONTROL_R, wash(pal().text, 0.07));
    }
    let ink = pal().muted.lerp_to_gamma(accent, near);
    let icon_x = if compact {
        rect.center().x
    } else {
        rect.min.x + 14.0 + 11.0
    };
    let icon_c = Pos2::new(icon_x, rect.center().y);
    fonts::paint_icon(painter, icon_c, icon, 22.0, ink, on);
    if compact {
        return resp
            .on_hover_text(title)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let text = galley(
        ui,
        title,
        text_font(14.5, on),
        pal().muted.lerp_to_gamma(pal().text, near),
    );
    let at = Pos2::new(
        rect.min.x + 14.0 + 22.0 + 12.0,
        rect.center().y - text.size().y / 2.0,
    );
    ui.painter().galley(at.round(), text, Color32::PLACEHOLDER);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A small outlined status label such as the version or "OK": a pill with a faint wash of its
/// own color behind the text.
pub(in crate::ui::settings) fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let g = galley(ui, text, text_font(12.0, true), color);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(g.size().x + 20.0, g.size().y + 8.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, PILL, wash(color, 0.12));
    ui.painter().galley(
        Pos2::new(rect.min.x + 10.0, rect.center().y - g.size().y / 2.0).round(),
        g,
        Color32::PLACEHOLDER,
    );
}

/// A status as a dot and a word: "Granted", "Running", "Not running".
pub(in crate::ui::settings) fn status(ui: &mut egui::Ui, ok: bool, text: &str) {
    let color = if ok { pal().good } else { pal().bad };
    let g = galley(ui, text, text_font(13.0, true), color);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(g.size().x + 32.0, g.size().y + 10.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, PILL, wash(color, 0.12));
    ui.painter()
        .circle_filled(Pos2::new(rect.min.x + 14.0, rect.center().y), 4.0, color);
    ui.painter().galley(
        Pos2::new(rect.min.x + 24.0, rect.center().y - g.size().y / 2.0).round(),
        g,
        Color32::PLACEHOLDER,
    );
}

/// The width a [`status`] pill needs for `text`: what a row gives its control so the pill is not
/// squeezed (and the label beside it is not, either).
pub(in crate::ui::settings) fn status_width(ui: &egui::Ui, text: &str) -> f32 {
    galley(ui, text, text_font(13.0, true), Color32::PLACEHOLDER)
        .size()
        .x
        + 32.0
}

/// A line of secondary text.
pub(in crate::ui::settings) fn muted(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(pal().muted));
}

/// The title above a panel: small, in the title weight, in the secondary color.
pub(in crate::ui::settings) fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        ui.label(fonts::title(text, 13.0, pal().muted));
    });
    ui.add_space(6.0);
}

// ------------------------------------------------------------------ panels and rows

/// Whether something was already added to `ui`, so the next row starts with a divider.
fn started(ui: &egui::Ui) -> bool {
    ui.cursor().min.y > ui.min_rect().min.y + 0.5
}

/// A pale line across the panel, between two rows.
fn divider(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, pal().divider);
}

/// A titled panel that fills the width it is given, followed by one [`SECTION_GAP`].
/// Every tab is a stack of these, so the spacing is the same everywhere. Its content is made of
/// the rows below, which bring their own vertical room.
pub(in crate::ui::settings) fn panel<R>(
    ui: &mut egui::Ui,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    if !title.is_empty() {
        section_title(ui, title);
    }
    let out = card()
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add_contents(ui)
        })
        .inner;
    ui.add_space(SECTION_GAP);
    out
}

/// The geometry of a row once its text is laid out.
struct RowLayout {
    /// The whole row, padding included.
    rect: Rect,
    /// Where the control goes: [`CONTROL_H`] high, vertically centered.
    control: Rect,
}

/// Lays out and paints the text of a row (`label` and an optional `hint`) and returns where the
/// control goes. `control_w` is the width kept free at the right.
fn row_text(ui: &mut egui::Ui, label: &str, hint: &str, control_w: f32) -> RowLayout {
    if started(ui) {
        divider(ui);
    }
    let width = ui.available_width();
    let text_w = (width - control_w - GAP).max(80.0);
    let painter = ui.painter().clone();
    let name = painter.layout(
        label.to_owned(),
        FontId::proportional(14.5),
        pal().text,
        text_w,
    );
    let note = (!hint.is_empty()).then(|| {
        painter.layout(
            hint.to_owned(),
            FontId::proportional(12.5),
            pal().muted,
            text_w,
        )
    });
    let text_h = name.size().y + note.as_ref().map_or(0.0, |n| n.size().y + 2.0);
    let inner_h = text_h.max(CONTROL_H);
    let top = ui.cursor().min;
    let rect = Rect::from_min_size(top, Vec2::new(width, inner_h + 2.0 * ROW_PAD));
    let text_top = rect.min.y + ROW_PAD + (inner_h - text_h) / 2.0;
    painter.galley(
        Pos2::new(rect.min.x, text_top).round(),
        name.clone(),
        Color32::PLACEHOLDER,
    );
    if let Some(n) = note {
        painter.galley(
            Pos2::new(rect.min.x, text_top + name.size().y + 2.0).round(),
            n,
            Color32::PLACEHOLDER,
        );
    }
    let control = Rect::from_min_size(
        Pos2::new(
            rect.max.x - control_w,
            rect.min.y + ROW_PAD + (inner_h - CONTROL_H) / 2.0,
        ),
        Vec2::new(control_w, CONTROL_H),
    );
    RowLayout { rect, control }
}

/// Moves the cursor of `ui` to the bottom of `row`, whatever the control inside used.
fn finish_row(ui: &mut egui::Ui, row: &RowLayout) {
    let rest = row.rect.max.y - ui.cursor().min.y;
    if rest > 0.0 {
        ui.add_space(rest);
    }
}

/// A row of a [`panel`]: `label` (and a smaller `hint` under it) on the left, a control of up to
/// `control_w` points on the right, laid out right to left.
pub(in crate::ui::settings) fn row_wide<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    control_w: f32,
    add_control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let row = row_text(ui, label, hint, control_w);
    let out = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(row.control)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                add_control(ui)
            },
        )
        .inner;
    finish_row(ui, &row);
    out
}

/// [`row_wide`] with the usual control width.
pub(in crate::ui::settings) fn row<R>(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    add_control: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    row_wide(ui, label, hint, 220.0, add_control)
}

/// A row that is a switch: the whole row is the click target.
pub(in crate::ui::settings) fn toggle_row(
    ui: &mut egui::Ui,
    value: &mut bool,
    label: &str,
    hint: &str,
    accent: Color32,
) -> egui::Response {
    let row = row_text(ui, label, hint, SWITCH.x);
    let mut resp = ui.interact(
        row.rect,
        ui.id().with(("toggle", label)),
        egui::Sense::click(),
    );
    if resp.clicked() {
        *value = !*value;
        resp.mark_changed();
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label)
    });
    let t = ui.ctx().animate_bool_responsive(resp.id, *value);
    let track = Rect::from_center_size(
        row.control.right_center() - Vec2::new(SWITCH.x / 2.0, 0.0),
        SWITCH,
    );
    if resp.has_focus() {
        focus_ring(ui, track, PILL, accent);
    }
    let dim = if ui.is_enabled() { 1.0 } else { 0.45 };
    let mut painter = ui.painter().clone();
    painter.set_opacity(dim);
    paint_switch(&painter, track, t, accent);
    finish_row(ui, &row);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A row that picks `value` for `cur`: a check mark at the right shows the chosen one.
pub(in crate::ui::settings) fn choice_row<T: PartialEq>(
    ui: &mut egui::Ui,
    cur: &mut T,
    value: T,
    label: &str,
    hint: &str,
    accent: Color32,
) -> egui::Response {
    let on = *cur == value;
    let row = row_text(ui, label, hint, 24.0);
    let mut resp = ui.interact(
        row.rect,
        ui.id().with(("choice", label)),
        egui::Sense::click(),
    );
    if resp.clicked() && !on {
        *cur = value;
        resp.mark_changed();
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, ui.is_enabled(), on, label)
    });
    if on {
        fonts::paint_icon(
            ui.painter(),
            row.control.center(),
            symbols::CHECK,
            20.0,
            accent,
            false,
        );
    }
    finish_row(ui, &row);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A row with a slider under its label and the value at the right of the label.
pub(in crate::ui::settings) fn slider_row<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Num,
    range: RangeInclusive<Num>,
    suffix: &str,
    accent: Color32,
) -> egui::Response {
    if started(ui) {
        divider(ui);
    }
    ui.add_space(ROW_PAD);
    let shown = if Num::INTEGRAL {
        format!("{}{suffix}", value.to_f64() as i64)
    } else {
        format!("{:.0}{suffix}", value.to_f64())
    };
    let name = galley(ui, label, FontId::proportional(14.5), pal().text);
    let number = galley(ui, &shown, text_font(13.5, true), pal().muted);
    let (line, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), name.size().y.max(number.size().y)),
        egui::Sense::hover(),
    );
    ui.painter()
        .galley(line.left_top(), name, Color32::PLACEHOLDER);
    ui.painter().galley(
        Pos2::new(line.right() - number.size().x, line.top()).round(),
        number,
        Color32::PLACEHOLDER,
    );
    ui.add_space(4.0);
    let resp = slider(ui, value, range, accent);
    ui.add_space(ROW_PAD - 4.0);
    resp
}

/// A row of secondary text inside a panel, with the vertical room of a row.
pub(in crate::ui::settings) fn note(ui: &mut egui::Ui, text: &str) {
    if started(ui) {
        divider(ui);
    }
    ui.add_space(ROW_PAD);
    ui.add(egui::Label::new(RichText::new(text).size(13.0).color(pal().muted)).wrap());
    ui.add_space(ROW_PAD);
}

/// A row of buttons inside a panel, wrapping onto more lines when it must.
pub(in crate::ui::settings) fn actions<R>(
    ui: &mut egui::Ui,
    add_buttons: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    if started(ui) {
        divider(ui);
    }
    ui.add_space(ROW_PAD);
    let out = ui
        .horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(8.0);
            add_buttons(ui)
        })
        .inner;
    ui.add_space(ROW_PAD);
    out
}

/// A row that shows a fact: a label and, at the right, whatever `add_value` draws.
pub(in crate::ui::settings) fn info_row<R>(
    ui: &mut egui::Ui,
    label: &str,
    add_value: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    row(ui, label, "", add_value)
}

/// A single-line text field as high as every other control, with room inside.
pub(in crate::ui::settings) fn text_field(text: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(text)
        .min_size(Vec2::new(0.0, CONTROL_H))
        .vertical_align(egui::Align::Center)
        .margin(egui::Margin::symmetric(12, 4))
}

/// A text field of several lines (`rows` high before it scrolls), with the margins of
/// [`text_field`]. For a sentence or more that the person writes in their own words.
pub(in crate::ui::settings) fn text_area(text: &mut String, rows: usize) -> egui::TextEdit<'_> {
    egui::TextEdit::multiline(text)
        .desired_rows(rows)
        .margin(egui::Margin::symmetric(12, 8))
}

// ------------------------------------------------------------------ floating things

/// What a [`sheet`] or a [`popover`] asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui::settings) struct Floating {
    /// The close button was pressed, or the dimmed area around the sheet was clicked.
    pub close: bool,
}

/// A title row for a sheet or a popover: the title at the left, a close button at the right.
/// Returns whether the close button was pressed.
fn floating_header(ui: &mut egui::Ui, title: &str) -> bool {
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(fonts::title(title, 17.0, pal().text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            close = icon_button(ui, symbols::X, "Close", true).clicked();
        });
    });
    ui.add_space(GAP);
    close
}

/// A modal sheet centered over the window: the window dims, and the sheet floats above it. All
/// the "add something" dialogs of Settings (a slice, an icon) use this, so they look and behave
/// the same: a title, a close button, Esc to close, a click outside to close.
///
/// `bounds` is the window's rectangle (the dimmed area, which follows the window's rounded
/// corners), `size` the sheet's size, shrunk to fit `bounds`.
pub(in crate::ui::settings) fn sheet<R>(
    ctx: &egui::Context,
    id: &str,
    bounds: Rect,
    size: Vec2,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> (Floating, R) {
    let mut out = Floating::default();
    let size = Vec2::new(
        size.x.min(bounds.width() - 2.0 * GAP),
        size.y.min(bounds.height() - 4.0 * GAP),
    );
    let inner = egui::Area::new(egui::Id::new(("sheet", id)))
        .order(egui::Order::Foreground)
        .fixed_pos(bounds.min)
        .constrain(false)
        .show(ctx, |ui| {
            let scrim = ui.allocate_rect(bounds, egui::Sense::click());
            ui.painter().rect_filled(bounds, CARD_RADIUS, pal().scrim);
            let rect = Rect::from_center_size(bounds.center(), size);
            let mut inner = None;
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                popup_frame().show(ui, |ui| {
                    let inner_size = size - Vec2::splat(2.0 * f32::from(PANEL_PAD));
                    ui.set_min_size(inner_size);
                    ui.set_max_size(inner_size);
                    out.close |= floating_header(ui, title);
                    inner = Some(add_contents(ui));
                });
            });
            if scrim.clicked()
                && let Some(p) = scrim.interact_pointer_pos()
                && !rect.contains(p)
            {
                out.close = true;
            }
            inner
        })
        .inner;
    // the caller reads the closure's result; it is always Some after the closure ran
    (out, inner.expect("the sheet closure ran"))
}

/// A button that opens a small popup under it. Every popup of Settings that is not a dropdown
/// list is one of these: a title, the content, a close button; it stays open while you click and
/// type inside and closes on a click outside, on its button again, on Esc, or when `add_contents`
/// calls `ui.close()`.
///
/// Nothing uses it at the moment (the profile actions unfold in a row instead); it stays for the
/// next small form that belongs under its button.
#[allow(dead_code)]
pub(in crate::ui::settings) fn popover<R>(
    trigger: &egui::Response,
    title: &str,
    width: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    egui::Popup::from_toggle_button_response(trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_START)
        .gap(6.0)
        .width(width)
        .frame(popup_frame())
        .show(|ui| {
            ui.set_width(width);
            if floating_header(ui, title) {
                ui.close();
            }
            add_contents(ui)
        })
        .map(|r| r.inner)
}

/// A drop-down list: a soft pill with the current choice and a caret, and under it a card with
/// one line per choice (a check marks the current one). Returns the index the user picked.
///
/// It replaces `egui::ComboBox`, whose list is a plain menu that does not follow the look of the
/// rest of Settings.
pub(in crate::ui::settings) fn select(
    ui: &mut egui::Ui,
    width: f32,
    current: &str,
    labels: &[&str],
    selected: usize,
) -> Option<usize> {
    let items: Vec<(Option<char>, &str)> = labels.iter().map(|&l| (None, l)).collect();
    select_with(
        ui, width, width, None, current, &items, selected, LIST_MAX_H,
    )
}

/// [`select`] whose list is at most `max_height` high: for a drop-down inside a small window (a
/// panel of a flow), where the usual list would run past the edge and be cut off.
pub(in crate::ui::settings) fn select_capped(
    ui: &mut egui::Ui,
    width: f32,
    current: &str,
    labels: &[&str],
    selected: usize,
    max_height: f32,
) -> Option<usize> {
    let items: Vec<(Option<char>, &str)> = labels.iter().map(|&l| (None, l)).collect();
    select_with(
        ui, width, width, None, current, &items, selected, max_height,
    )
}

/// [`select`] whose list is wider than the pill: for a small pill (an operator in a row of
/// fields) whose choices have longer names than the pill can show.
pub(in crate::ui::settings) fn select_wide(
    ui: &mut egui::Ui,
    width: f32,
    list_width: f32,
    current: &str,
    labels: &[&str],
    selected: usize,
) -> Option<usize> {
    let items: Vec<(Option<char>, &str)> = labels.iter().map(|&l| (None, l)).collect();
    select_with(
        ui,
        width,
        list_width.max(width),
        None,
        current,
        &items,
        selected,
        LIST_MAX_H,
    )
}

/// [`select`] whose choices have an icon each: the icon of the current one shows on the pill, and
/// every line of the list starts with its own.
pub(in crate::ui::settings) fn select_icons(
    ui: &mut egui::Ui,
    width: f32,
    items: &[(Option<char>, &str)],
    selected: usize,
) -> Option<usize> {
    let (icon, label) = items.get(selected).copied().unwrap_or((None, ""));
    select_with(ui, width, width, icon, label, items, selected, LIST_MAX_H)
}

/// How high the list of a [`select`] gets before it scrolls.
const LIST_MAX_H: f32 = 340.0;

#[allow(clippy::too_many_arguments)]
fn select_with(
    ui: &mut egui::Ui,
    width: f32,
    list_width: f32,
    current_icon: Option<char>,
    current: &str,
    items: &[(Option<char>, &str)],
    selected: usize,
    max_height: f32,
) -> Option<usize> {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, CONTROL_H), egui::Sense::click());
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let fill = if open || resp.hovered() {
        pal().hot
    } else {
        pal().idle
    };
    ui.painter().rect_filled(rect, CONTROL_R, fill);
    let mut text_x = rect.min.x + 14.0;
    if let Some(icon) = current_icon {
        fonts::paint_icon(
            ui.painter(),
            Pos2::new(rect.min.x + 25.0, rect.center().y),
            icon,
            17.0,
            pal().muted,
            false,
        );
        text_x = rect.min.x + 44.0;
    }
    // the name stops before the caret instead of running under it
    let (g, cut) = fit_galley(
        ui,
        current,
        FontId::proportional(14.0),
        pal().text,
        rect.max.x - 36.0 - text_x,
    );
    ui.painter().galley(
        Pos2::new(text_x, rect.center().y - g.size().y / 2.0).round(),
        g,
        Color32::PLACEHOLDER,
    );
    fonts::paint_icon(
        ui.painter(),
        Pos2::new(rect.max.x - 18.0, rect.center().y),
        if open {
            symbols::CARET_UP
        } else {
            symbols::CARET_DOWN
        },
        14.0,
        pal().muted,
        false,
    );
    let mut resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if cut && !open {
        resp = resp.on_hover_text(current);
    }
    let mut picked = None;
    egui::Popup::from_toggle_button_response(&resp)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .align(egui::RectAlign::BOTTOM_START)
        .gap(6.0)
        .width(list_width)
        .frame(popup_frame().inner_margin(egui::Margin::same(6)))
        .show(|ui| {
            ui.set_width(list_width - 12.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            // a long list (the operations of a System block) scrolls instead of running out of
            // the window
            egui::ScrollArea::vertical()
                .max_height(max_height)
                .show(ui, |ui| {
                    for (i, (icon, label)) in items.iter().enumerate() {
                        if select_item(ui, *icon, label, i == selected).clicked() {
                            picked = Some(i);
                        }
                    }
                });
        });
    picked
}

/// One line of a [`select`] list.
pub(in crate::ui::settings) fn select_item(
    ui: &mut egui::Ui,
    icon: Option<char>,
    label: &str,
    on: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), CONTROL_H),
        egui::Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_filled(rect, CONTROL_R, pal().idle);
    }
    let mut text_x = rect.min.x + 12.0;
    if let Some(icon) = icon {
        fonts::paint_icon(
            ui.painter(),
            Pos2::new(rect.min.x + 22.0, rect.center().y),
            icon,
            17.0,
            if on { pal().text } else { pal().muted },
            false,
        );
        text_x = rect.min.x + 42.0;
    }
    // room for the check at the right: a long name is cut before it, never drawn over it
    let right_pad = if on { 40.0 } else { 14.0 };
    let (g, cut) = fit_galley(
        ui,
        label,
        text_font(14.0, on),
        pal().text,
        rect.max.x - right_pad - text_x,
    );
    ui.painter().galley(
        Pos2::new(text_x, rect.center().y - g.size().y / 2.0).round(),
        g,
        Color32::PLACEHOLDER,
    );
    if on {
        fonts::paint_icon(
            ui.painter(),
            Pos2::new(rect.max.x - 20.0, rect.center().y),
            symbols::CHECK,
            16.0,
            pal().text,
            false,
        );
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if cut { resp.on_hover_text(label) } else { resp }
}

/// A menu of actions under a button: no title, no close button, just [`menu_item`] lines on a
/// card. It closes on any click inside (the item was chosen), a click outside, or Esc. For a choice
/// that stays on screen use [`select`]; for a form use [`popover`].
pub(in crate::ui::settings) fn menu_popup<R>(
    trigger: &egui::Response,
    width: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    egui::Popup::from_toggle_button_response(trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(width)
        .frame(popup_frame().inner_margin(egui::Margin::same(6)))
        .show(|ui| {
            ui.set_width(width - 12.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            add_contents(ui)
        })
        .map(|r| r.inner)
}

/// One entry of a menu-like popover: an icon and a label, the whole line clickable.
pub(in crate::ui::settings) fn menu_item(
    ui: &mut egui::Ui,
    icon: char,
    label: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), CONTROL_H),
        egui::Sense::click(),
    );
    if resp.hovered() {
        ui.painter().rect_filled(rect, CONTROL_R, pal().idle);
    }
    fonts::paint_icon(
        ui.painter(),
        Pos2::new(rect.min.x + 20.0, rect.center().y),
        icon,
        18.0,
        pal().muted,
        false,
    );
    let g = galley(ui, label, FontId::proportional(14.0), pal().text);
    ui.painter().galley(
        Pos2::new(rect.min.x + 40.0, rect.center().y - g.size().y / 2.0).round(),
        g,
        Color32::PLACEHOLDER,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ------------------------------------------------------------------ brand and shortcuts

/// The CursorPie mark (a ring of four arcs around a diamond), `radius` points from its center to the
/// outer edge of the ring, in the text color of the appearance. The real artwork is used (see
/// [`brand`]); if it cannot be loaded the same shape is drawn with `ring_slice`.
pub(in crate::ui::settings) fn draw_logo(painter: &egui::Painter, center: Pos2, radius: f32) {
    let color = pal().text;
    let rect = Rect::from_center_size(center, Vec2::splat(radius * 2.0));
    if !brand::paint(painter, brand::Art::Mark, rect, color) {
        draw_logo_shape(painter, center, radius, color);
    }
}

/// The CursorPie logo with its name, as one picture: `height` points high, its left edge at `left`
/// and its middle at `center_y`, in the text color of the appearance. If the artwork cannot be
/// loaded, the mark and the name in the interface font stand in.
pub(in crate::ui::settings) fn draw_wordmark(
    painter: &egui::Painter,
    left: f32,
    center_y: f32,
    height: f32,
) {
    let color = pal().text;
    // the picture is 1260 x 307 px
    let drawn = Rect::from_min_size(
        Pos2::new(left, center_y - height / 2.0),
        Vec2::new(height * 1260.0 / 307.0, height),
    );
    if brand::paint(painter, brand::Art::Wordmark, drawn, color) {
        return;
    }
    let r = height / 2.0;
    draw_logo_shape(painter, Pos2::new(left + r, center_y), r, color);
    painter.text(
        Pos2::new(left + height + 6.0, center_y),
        egui::Align2::LEFT_CENTER,
        "CursorPie",
        FontId::new(19.0, fonts::brand()),
        color,
    );
}

/// The mark drawn by hand, for when the artwork is not there.
fn draw_logo_shape(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
    let gap = 0.30; // radians cut out of the ring at each diagonal
    for i in 0..4 {
        let mid = FRAC_PI_2 * i as f32 - FRAC_PI_2;
        let (a0, a1) = (mid - FRAC_PI_4 + gap / 2.0, mid + FRAC_PI_4 - gap / 2.0);
        painter.add(ring_slice(
            painter,
            center,
            radius * 0.74,
            radius,
            a0,
            a1,
            color,
        ));
    }
    let d = radius * 0.24;
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + Vec2::new(0.0, -d),
            center + Vec2::new(d, 0.0),
            center + Vec2::new(0.0, d),
            center + Vec2::new(-d, 0.0),
        ],
        color,
        Stroke::NONE,
    ));
}

/// The keys of a shortcut label, one per keycap: `"Shift + Cmd + Z"` gives three.
fn keycaps(label: &str) -> Vec<&str> {
    label.split(" + ").filter(|k| !k.is_empty()).collect()
}

/// Button showing a shortcut as keycaps (or the "press any key" prompt while recording).
pub(in crate::ui::settings) fn shortcut_button(
    ui: &mut egui::Ui,
    sc: &Shortcut,
    recording: bool,
    accent: Color32,
) -> egui::Response {
    let label = sc.label();
    let caps: Vec<_> = keycaps(&label)
        .into_iter()
        .map(|k| galley(ui, k, text_font(13.0, true), pal().text))
        .collect();
    let caps_w: f32 = caps.iter().map(|g| g.size().x + 16.0 + 4.0).sum::<f32>() - 4.0;
    let prompt = galley(
        ui,
        "Press the keys…  (Esc cancels)",
        text_font(13.5, true),
        on_accent(accent),
    );
    let width = if recording {
        prompt.size().x
    } else {
        caps_w.max(0.0)
    } + 28.0;
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(width.max(150.0), CONTROL_H), egui::Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let fill = match (recording, resp.hovered()) {
        (true, _) => accent,
        (false, true) => pal().hot,
        (false, false) => pal().idle,
    };
    ui.painter().rect_filled(rect, CONTROL_R, fill);
    if recording {
        let at = Pos2::new(
            rect.center().x - prompt.size().x / 2.0,
            rect.center().y - prompt.size().y / 2.0,
        );
        ui.painter()
            .galley(at.round(), prompt, Color32::PLACEHOLDER);
    } else {
        let mut x = rect.center().x - caps_w / 2.0;
        for g in caps {
            let w = g.size().x + 16.0;
            let cap = Rect::from_min_size(Pos2::new(x, rect.center().y - 12.0), Vec2::new(w, 24.0));
            ui.painter()
                .rect_filled(cap, egui::CornerRadius::same(7), pal().keycap);
            ui.painter().rect_stroke(
                cap,
                egui::CornerRadius::same(7),
                Stroke::new(1.0, pal().border),
                egui::StrokeKind::Inside,
            );
            ui.painter().galley(
                Pos2::new(cap.min.x + 8.0, cap.center().y - g.size().y / 2.0).round(),
                g,
                Color32::PLACEHOLDER,
            );
            x += w + 4.0;
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The accent swatches and, after them, the system color picker for any other color.
pub(in crate::ui::settings) fn accent_picker(ui: &mut egui::Ui, color: &mut [u8; 3]) {
    ui.spacing_mut().item_spacing.x = 8.0;
    // added from the right edge, so the picker comes first and the swatches fill in toward the left
    ui.color_edit_button_srgb(color).on_hover_text("Any color");
    for (name, rgb) in ACCENTS.iter().rev() {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), egui::Sense::click());
        let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        let on = *color == *rgb;
        ui.painter().circle_filled(rect.center(), 10.0, c);
        if on {
            ui.painter()
                .circle_stroke(rect.center(), 13.0, Stroke::new(2.0, c));
            fonts::paint_icon(
                ui.painter(),
                rect.center(),
                symbols::CHECK,
                13.0,
                on_accent(c),
                false,
            );
        }
        if resp
            .on_hover_text(*name)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            *color = *rgb;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::draw::contrast;

    #[test]
    fn a_shortcut_label_splits_into_keycaps() {
        assert_eq!(keycaps("Shift + Cmd + Z"), ["Shift", "Cmd", "Z"]);
        assert_eq!(keycaps("F13"), ["F13"]);
        assert!(keycaps("").is_empty());
        // a plus key is a key too: it is the label's last part, not a separator
        assert_eq!(keycaps("Cmd + +"), ["Cmd", "+"]);
    }

    #[test]
    fn the_slider_maps_values_to_the_rail_and_back() {
        let range = (150.0, 1500.0);
        assert_eq!(slider_fraction(150.0, range), 0.0);
        assert_eq!(slider_fraction(1500.0, range), 1.0);
        assert_eq!(slider_fraction(9999.0, range), 1.0, "clamped");
        assert_eq!(slider_fraction(5.0, (3.0, 3.0)), 0.0, "an empty range");
        for v in [150.0, 500.0, 1200.0, 1500.0] {
            let back = slider_snap(slider_fraction(v, range), range, 50.0, true, true);
            assert_eq!(back, v, "{v} survives the round trip");
        }
        assert_eq!(
            slider_snap(0.5, (0.0, 3.0), 1.0, true, true),
            2.0,
            "rounds to a whole number"
        );
        assert_eq!(slider_snap(0.5, (0.0, 3.0), 1.0, false, true), 1.5);
        assert_eq!(
            slider_snap(7.0, (0.0, 10.0), 1.0, true, true),
            10.0,
            "clamped"
        );
    }

    #[test]
    fn every_slider_of_settings_gets_round_anchors() {
        // (range, step, majors) of the sliders Settings has
        let cases: [((f64, f64), f64, Vec<f64>); 5] = [
            ((0.0, 100.0), 5.0, vec![0.0, 20.0, 40.0, 60.0, 80.0, 100.0]),
            (
                (150.0, 500.0),
                10.0,
                vec![150.0, 200.0, 300.0, 400.0, 500.0],
            ),
            ((150.0, 1500.0), 50.0, vec![150.0, 500.0, 1000.0, 1500.0]),
            (
                (500.0, 4000.0),
                100.0,
                vec![500.0, 1000.0, 2000.0, 3000.0, 4000.0],
            ),
            ((10.0, 600.0), 20.0, vec![10.0, 200.0, 400.0, 600.0]),
        ];
        for (range, step, majors) in cases {
            let a = slider_anchors(range, true);
            assert_eq!((a.step, a.majors), (step, majors), "{range:?}");
        }
        let pie = slider_anchors((140.0, 360.0), false);
        assert_eq!(pie.step, 10.0);
        assert_eq!(pie.majors.first(), Some(&140.0));
        assert_eq!(pie.majors.last(), Some(&360.0));
        assert!(pie.majors.windows(2).all(|w| w[1] - w[0] >= 220.0 * 0.07));
        assert_eq!(
            slider_anchors((3.0, 3.0), true).majors,
            vec![3.0],
            "an empty range"
        );
    }

    #[test]
    fn the_slider_snaps_to_the_grid_and_the_ends_unless_shift_is_held() {
        let r = (150.0, 500.0);
        let at = |v: f64| slider_fraction(v, r);
        assert_eq!(slider_snap(at(263.0), r, 10.0, true, false), 260.0);
        assert_eq!(slider_snap(at(268.0), r, 10.0, true, false), 270.0);
        assert_eq!(
            slider_snap(at(263.0), r, 10.0, true, true),
            263.0,
            "Shift is free"
        );
        assert_eq!(
            slider_snap(0.0, r, 10.0, true, false),
            150.0,
            "the start is reachable"
        );
        assert_eq!(
            slider_snap(1.0, r, 10.0, true, false),
            500.0,
            "so is the end"
        );
        // a range that does not start on the grid: the start beats the nearest grid point
        assert_eq!(
            slider_snap(
                slider_fraction(14.0, (10.0, 600.0)),
                (10.0, 600.0),
                20.0,
                true,
                false
            ),
            10.0
        );
    }

    #[test]
    fn a_pressed_filled_button_is_darker_than_at_rest() {
        let accent = Color32::from_rgb(88, 86, 214);
        let (rest, hot, _) = Look::Primary(accent).colors();
        assert_eq!(rest, accent);
        assert!(crate::ui::draw::luminance(hot) < crate::ui::draw::luminance(rest));
    }

    #[test]
    fn every_look_has_readable_text() {
        let accent = Color32::from_rgb(88, 86, 214);
        for p in [LIGHT, DARK] {
            for look in [Look::Primary(accent), Look::Soft, Look::Plain, Look::Danger] {
                let (rest, hot, fg) = look.colors_in(&p);
                for bg in [rest, hot] {
                    // translucent fills sit on a panel: compare against what the eye sees
                    let under = if bg.a() == 255 {
                        bg
                    } else {
                        let f = f32::from(bg.a()) / 255.0;
                        let mix = |a: u8, b: u8| {
                            (f32::from(a) * f + f32::from(b) * (1.0 - f)).round() as u8
                        };
                        Color32::from_rgb(
                            mix(bg.r(), p.card.r()),
                            mix(bg.g(), p.card.g()),
                            mix(bg.b(), p.card.b()),
                        )
                    };
                    assert!(
                        contrast(fg, under) >= 4.5,
                        "{look:?} (dark: {}): {fg:?} on {under:?} is {:.1}:1",
                        p.dark,
                        contrast(fg, under)
                    );
                }
            }
        }
    }

    #[test]
    fn a_link_is_a_line_of_text_and_not_a_button() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0))),
            ..Default::default()
        };
        let mut size = Vec2::ZERO;
        // nothing renders the output, so the texture uploads are dropped on purpose
        ctx.run_ui(input, |ui| {
            size = link(ui, "Open System Settings", Color32::RED).rect.size();
        })
        .drop_without_applying_deltas();
        assert!(
            size.x > 100.0 && size.x < 260.0,
            "as wide as its words: {size:?}"
        );
        assert!(
            size.y < CONTROL_H - 8.0,
            "no taller than a line of text: {size:?}"
        );
    }
}
