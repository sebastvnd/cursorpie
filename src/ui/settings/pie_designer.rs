//! The interactive pie in Settings: drawing, hit-testing and clicks.
use super::state::Ed;
use crate::config::*;
use crate::geometry::Point;
use crate::geometry::*;
use crate::ui::draw::*;
use crate::ui::fonts;
use crate::ui::settings::theme::pal;
use crate::ui::symbols;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Sense, Vec2};
use std::f32::consts::TAU;

/// What the user did with the designer this frame.
pub(super) enum Designer {
    None,
    Enter(usize),
    Up,
    /// The slice was dragged onto another one: move it to that place.
    Move(usize, usize),
    /// The slice was dragged onto the trash.
    Delete(usize),
}
/// `[r, g, b]` of a palette color.
fn rgb3(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// What the pointer is over.
enum Hit {
    Out,
    Center,
    Slice(usize),
}

/// Draws the pie of `items` at `size` px; click a slice to select it, drag it onto another slice to
/// move it or onto the trash button of the editor to delete it. `show_labels` off draws icons only,
/// like the pie itself. `outline` is the shape of the pie as the settings give it (circle or rounded
/// polygon), so the pie here is the one the person will see when it opens.
#[allow(clippy::too_many_arguments)]
pub(super) fn pie_designer(
    ui: &mut egui::Ui,
    ed: &mut Ed,
    ctx: &egui::Context,
    items: &[Item],
    accent: [u8; 3],
    size: f32,
    show_labels: bool,
    outline: Outline,
) -> Designer {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let c = rect.center();
    let (outer, inner) = (pie_outer(size), pie_inner(size));
    // the ring is made round and bent to the outline, exactly as the real pie does it
    let bend = Bend::new(outline, inner, outer);

    let n = items.len();
    let total = n;

    let hit = |p: Pos2| -> Hit {
        let v = p - c;
        let d = v.length();
        // the rim is where the outline is in that direction, not a circle
        let rim = outer * outline.radius(v.y.atan2(v.x));
        if d > rim + 6.0 {
            Hit::Out
        } else if d < inner {
            Hit::Center
        } else {
            let i = slice_at(
                Point::new(c.x as f64, c.y as f64),
                Point::new(p.x as f64, p.y as f64),
                total,
                inner,
            );
            i.map(Hit::Slice).unwrap_or(Hit::Center)
        }
    };
    let hov = resp.hover_pos().map(hit).unwrap_or(Hit::Out);
    let pointer = ui.input(|i| i.pointer.latest_pos());

    // a drag starts on the slice where the button went down
    if resp.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        && let Hit::Slice(i) = hit(origin)
        && i < n
    {
        ed.drag = Some(i);
        ed.sel = Some(i);
        ed.adding = false;
    }
    let dragging = ed.drag.filter(|&d| d < n && resp.dragged());
    // the slice the dragged one would take the place of
    let drag_to = dragging.zip(pointer).and_then(|(from, p)| match hit(p) {
        Hit::Slice(j) if j < n && j != from => Some(j),
        _ => None,
    });
    if dragging.is_some() {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if !matches!(hov, Hit::Out) {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // ---- draw ----
    let sl = TAU / total.max(1) as f32;
    let accent_c = rgb(accent);
    for i in 0..total {
        let item = items.get(i);
        let selected = ed.sel == Some(i);
        let lifted = dragging == Some(i);
        let hovered = matches!(hov, Hit::Slice(h) if h == i) && dragging.is_none();
        let base = rgb3(pal().idle);
        let t = if drag_to == Some(i) {
            0.45 // where the dragged slice would land
        } else if selected {
            1.0
        } else if hovered {
            0.16
        } else {
            0.0
        };
        // text on the slice: dark, or whatever reads on the accent while it is selected
        let ink = if selected {
            on_accent(accent_c)
        } else {
            pie_colors().ink
        };
        // the dragged slice stays behind as a faint place holder
        let ink = if lifted {
            ink.gamma_multiply(0.35)
        } else {
            ink
        };
        let a0 = slice_start(i, total);
        painter.add(ring_slice(
            &painter,
            c,
            inner,
            outer,
            a0 + 0.03,
            a0 + sl - 0.03,
            rgb(mix(base, accent, t)),
            bend,
        ));

        let am = a0 + sl / 2.0;
        let tp = c + Vec2::new(am.cos(), am.sin()) * bend.radius(am, (inner + outer) / 2.0);
        if let Some(it) = item {
            let tex = ed.icon_for(ctx, &it.icon);
            // icons only: the icon sits in the middle of the slice
            let (icon_dy, caret_dy) = if show_labels {
                (-8.0, 33.0)
            } else {
                (0.0, 24.0)
            };
            draw_icon(
                &painter,
                tex.as_ref(),
                &it.icon,
                tp + Vec2::new(0.0, icon_dy),
                30.0,
                ink,
            );
            if show_labels {
                painter.text(
                    tp + Vec2::new(0.0, 18.0),
                    Align2::CENTER_CENTER,
                    &it.name,
                    FontId::proportional(11.5),
                    ink.gamma_multiply(0.85),
                );
            }
            if matches!(it.kind, ItemKind::Submenu(_)) {
                fonts::paint_icon(
                    &painter,
                    tp + Vec2::new(0.0, caret_dy),
                    symbols::CARET_DOWN,
                    13.0,
                    if selected { ink } else { accent_c },
                    false,
                );
            }
        }
    }
    // center: "Back" while inside a submenu
    let core = inner * 0.86;
    let center_hot = matches!(hov, Hit::Center) && !ed.nav.is_empty();
    painter.circle_filled(c, core, if center_hot { pal().hot } else { pal().bg });
    if ed.nav.is_empty() {
        fonts::paint_icon(
            &painter,
            c,
            symbols::CHART_PIE_SLICE,
            26.0,
            pal().muted,
            false,
        );
    } else {
        fonts::paint_icon(
            &painter,
            c - Vec2::new(0.0, 9.0),
            symbols::CARET_LEFT,
            18.0,
            pal().text,
            false,
        );
        painter.text(
            c + Vec2::new(0.0, 10.0),
            Align2::CENTER_CENTER,
            "Back",
            FontId::proportional(12.5),
            pal().text,
        );
    }

    // the dragged slice follows the pointer
    if let (Some(from), Some(p)) = (dragging, pointer) {
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("slice_drag"));
        let float = ctx.layer_painter(layer);
        float.circle_filled(p, 26.0, accent_c.gamma_multiply(0.92));
        let it = &items[from];
        let tex = ed.icon_for(ctx, &it.icon);
        draw_icon(&float, tex.as_ref(), &it.icon, p, 28.0, on_accent(accent_c));
    }

    // ---- released: move it, delete it, or let it go back ----
    if resp.drag_stopped() {
        let from = ed.drag.take();
        if let (Some(from), Some(p)) = (from, pointer)
            && from < n
        {
            if ed.trash_rect.is_some_and(|r| r.expand(8.0).contains(p)) {
                return Designer::Delete(from);
            }
            if let Hit::Slice(j) = hit(p)
                && j < n
                && j != from
            {
                return Designer::Move(from, j);
            }
        }
    }

    if !resp.dragged() {
        ed.drag = None; // a drag that ended without telling us (focus lost)
    }

    // ---- clicks ----
    if resp.clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        match hit(p) {
            Hit::Center if !ed.nav.is_empty() => return Designer::Up,
            Hit::Slice(i) if i < n => {
                let is_sub = matches!(items[i].kind, ItemKind::Submenu(_));
                if ed.sel == Some(i) && is_sub {
                    return Designer::Enter(i); // click again = step in
                }
                ed.sel = Some(i);
                ed.adding = false;
            }
            _ => {}
        }
    }
    Designer::None
}
