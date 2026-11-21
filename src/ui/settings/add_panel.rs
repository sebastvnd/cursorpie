//! The "Add a slice" panel: ready-made slices to append to the current ring.

use super::state::{AddCatalog, Ed};
use crate::config::flow::Flow;
use crate::config::*;
use crate::ui::draw::*;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use eframe::egui::{self, Color32, FontId, Vec2};

/// Draws the sheet; picking a tile appends that slice to `ring` and selects it.
pub(super) fn add_panel(
    ctx: &egui::Context,
    ed: &mut Ed,
    ring: &mut Vec<Item>,
    depth: usize,
    accent: Color32,
    flows: &[Flow],
) {
    let mut picked: Option<Item> = None;
    // built once when the sheet opens, not on every frame (see `AddCatalog`)
    let can_nest = depth + 1 < MAX_DEPTH;
    if ed
        .add_catalog
        .as_ref()
        .is_none_or(|c| c.can_nest != can_nest)
    {
        ed.add_catalog = Some(AddCatalog::new(can_nest, flows));
    }
    let Some(catalog) = ed.add_catalog.as_ref() else {
        return;
    };
    let (floating, ()) = sheet(
        ctx,
        "add_slice",
        ed.card,
        Vec2::new(600.0, 620.0),
        "Add a slice",
        |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let inner_w = ui.available_width();
                    let (min_w, th, gap) = (104.0_f32, 84.0_f32, GAP);
                    let cols = (((inner_w + gap) / (min_w + gap)).floor().max(1.0)) as usize;
                    // the tiles share the whole width between them, so the last column ends at the
                    // edge of the sheet and no blank strip is left on the right
                    let tw = ((inner_w - gap * (cols - 1) as f32) / cols as f32).floor();
                    for (title, list) in &catalog.groups {
                        section_title(ui, title);
                        for row in list.chunks(cols) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for it in row {
                                    let (r, resp) = tile(ui, Vec2::new(tw, th), false, accent);
                                    draw_icon(
                                        ui.painter(),
                                        None,
                                        &it.icon,
                                        r.center() - Vec2::new(0.0, 12.0),
                                        28.0,
                                        pie_colors().ink,
                                    );
                                    // one line, cut with an ellipsis when a name is longer than the tile
                                    let mut job = egui::text::LayoutJob::simple(
                                        it.name.clone(),
                                        FontId::proportional(12.5),
                                        pal().text,
                                        tw - 12.0,
                                    );
                                    job.wrap.max_rows = 1;
                                    job.wrap.break_anywhere = true;
                                    let galley = ui.painter().layout_job(job);
                                    let at =
                                        r.center() + Vec2::new(0.0, 24.0) - galley.size() / 2.0;
                                    ui.painter().galley(at.round(), galley, pal().text);
                                    if resp.clicked() {
                                        picked = Some(it.clone());
                                    }
                                }
                            });
                        }
                        ui.add_space(GAP);
                    }
                });
        },
    );
    if floating.close {
        ed.adding = false;
        ed.add_catalog = None;
    }
    if let Some(it) = picked {
        ring.push(it);
        ed.sel = Some(ring.len() - 1);
        ed.adding = false;
        ed.add_catalog = None;
    }
}
