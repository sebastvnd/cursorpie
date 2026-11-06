//! Menu tab: profile toolbar on top, then two tiles side by side without an outer scroll area.
//!   left  = breadcrumb + the interactive pie
//!   right = slice editor (scrolls on its own if needed)
//!
//! Under the pie sit two round buttons: add a slice, and delete the selected one (also where a dragged
//! slice is dropped to delete it).
//!
//! "Add a slice" and the icon picker are sheets that float over the window, see
//! [`sheet`](crate::ui::settings::widgets::sheet).

use super::profile_toolbar::profile_toolbar;
use crate::config::*;
use crate::msg::Msg;
use crate::ui::app::App;
use crate::ui::draw::*;
use crate::ui::fonts;
use crate::ui::settings::add_panel::add_panel;
use crate::ui::settings::inspector::{Env, item_inspector};
use crate::ui::settings::pie_designer::{Designer, pie_designer};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::{Look, round_button};
use crate::ui::symbols;
use eframe::egui::{self, Color32, Rect, RichText, Sense, UiBuilder, pos2, vec2};
use std::sync::mpsc::Sender;

/// Diameter of the round add and delete buttons under the pie.
const ROUND: f32 = 40.0;

impl App {
    /// Draws the Menu tab into `ui`, `content_w` pixels wide.
    pub(in crate::ui::settings) fn menu_tab(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        content_w: f32,
    ) {
        let App { edit, ed, tx, .. } = self;
        ed.profile = ed.profile.min(edit.profiles.len().saturating_sub(1));
        let accent = rgb(edit.accent);
        let accent_rgb = edit.accent;
        let show_labels = edit.show_labels;
        let tx_ref: &Sender<Msg> = tx;
        let trigger = edit.trigger_button;
        let mouse = crate::mouse::active_buttons();

        profile_toolbar(ui, ctx, edit, ed, tx_ref, accent);
        ui.add_space(GAP + 4.0);

        // ------------------------------------------------------------ two tiles
        // widths are computed explicitly (not from available_width) so nothing grows with its content
        let avail = ui.available_rect_before_wrap();
        let left_w = 320.0;
        let gap = GAP;
        // the pie fills the tile's width (when there is enough height) and is centered in the tile
        let pad = GAP + 4.0;
        let pie_size = (left_w - 2.0 * pad)
            .min(avail.height() - 110.0 - ROUND - GAP / 2.0)
            .clamp(200.0, 300.0);
        let left = Rect::from_min_size(avail.min, vec2(left_w, avail.height()));
        let right = Rect::from_min_size(
            pos2(avail.min.x + left_w + gap, avail.min.y),
            vec2((content_w - left_w - gap).max(200.0), avail.height()),
        );

        let Some(prof) = edit.profiles.get_mut(ed.profile) else {
            return;
        };
        if items_at(&prof.items, &ed.nav).is_none() {
            ed.reset_nav();
        } // stale path after an item was deleted
        let nav = ed.nav.clone();
        let sel_before = ed.sel;

        // left: a bordered tile with the breadcrumb + pie inside
        ui.painter().rect_filled(left, PANEL_R, pal().card);
        let (res, add_slice, delete_slice) = ui
            .scope_builder(UiBuilder::new().max_rect(left.shrink(pad)), |ui| {
                ui.horizontal_wrapped(|ui| {
                    let mut go: Option<usize> = None;
                    ui.spacing_mut().item_spacing.x = 4.0;
                    if ui
                        .link(fonts::title(&prof.name, 14.0, pal().text))
                        .clicked()
                    {
                        go = Some(0);
                    }
                    for d in 0..nav.len() {
                        ui.label(fonts::icon(symbols::CARET_RIGHT, 13.0, pal().muted));
                        let name = item_at(&prof.items, &nav[..=d])
                            .map(|i| i.name.as_str())
                            .unwrap_or("?");
                        if ui.link(name).clicked() {
                            go = Some(d + 1);
                        }
                    }
                    if let Some(k) = go {
                        ed.nav.truncate(k);
                        ed.sel = None;
                        ed.adding = false;
                    }
                });
                let ring = items_at(&prof.items, &nav).unwrap_or(&[]);
                // center the pie + hint in what is left under the breadcrumb
                let spare = (ui.available_height() - pie_size - 28.0 - ROUND - GAP / 2.0).max(0.0);
                ui.add_space(spare / 2.0);
                let res = ui
                    .vertical_centered(|ui| {
                        pie_designer(ui, ed, ctx, ring, accent_rgb, pie_size, show_labels)
                    })
                    .inner;
                ui.add_space(GAP / 2.0);

                // add and delete, one round button each, right under the pie
                let n = ring.len();
                let can_add = n < MAX_ITEMS;
                let can_delete = ed.sel.is_some_and(|i| i < n) && (n > 1 || !nav.is_empty());
                let (row, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROUND), Sense::hover());
                let step = (ROUND + GAP) / 2.0;
                let slot = |dx: f32| {
                    Rect::from_center_size(
                        pos2(row.center().x + dx, row.center().y),
                        vec2(ROUND, ROUND),
                    )
                };
                let (add_at, del_at) = (slot(-step), slot(step));
                let add = ui
                    .scope_builder(UiBuilder::new().max_rect(add_at), |ui| {
                        round_button(ui, Look::Soft, symbols::PLUS, "Add a slice", can_add, ROUND)
                    })
                    .inner
                    .clicked();
                let del = ui
                    .scope_builder(UiBuilder::new().max_rect(del_at), |ui| {
                        round_button(
                            ui,
                            Look::Danger,
                            symbols::TRASH,
                            "Delete slice",
                            can_delete,
                            ROUND,
                        )
                    })
                    .inner
                    .clicked();
                // a slice dragged over the delete button turns it red; dropping it there deletes it
                ed.trash_rect = can_delete.then_some(del_at);
                let over = ui
                    .input(|i| i.pointer.latest_pos())
                    .is_some_and(|p| del_at.expand(8.0).contains(p));
                if can_delete && ed.drag.is_some() && over {
                    ui.painter().circle_filled(
                        del_at.center(),
                        ROUND * 0.75,
                        Color32::from_rgba_unmultiplied(214, 69, 69, 70),
                    );
                }

                ui.add_space(GAP / 2.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Click to edit. Drag to move or delete.")
                            .size(12.5)
                            .color(pal().muted),
                    )
                });
                (res, add, del)
            })
            .inner;
        // the delete button acts on the selected slice, like a drop on it does
        let res = match (delete_slice, ed.sel) {
            (true, Some(i)) => Designer::Delete(i),
            _ => res,
        };
        if add_slice {
            ed.adding = true;
            ed.sel = None;
        }
        match res {
            Designer::Enter(i) => {
                ed.nav.push(i);
                ed.sel = None;
            }
            Designer::Up => {
                ed.sel = ed.nav.pop();
            } // select the submenu we came from
            Designer::Move(from, to) => {
                if let Some(ring) = ring_mut(&mut prof.items, &nav)
                    && from < ring.len()
                    && to < ring.len()
                {
                    let item = ring.remove(from);
                    ring.insert(to, item);
                    ed.sel = Some(to);
                    ed.stop_recording();
                }
            }
            Designer::Delete(i) => {
                if let Some(ring) = ring_mut(&mut prof.items, &nav)
                    && i < ring.len()
                    && (ring.len() > 1 || !nav.is_empty())
                {
                    ring.remove(i);
                    ed.sel = None;
                    ed.stop_recording();
                }
            }
            Designer::None => {}
        }
        if ed.sel != sel_before {
            ed.picking_icon = false;
        }

        // right: editor
        let nav = ed.nav.clone(); // may have just changed
        let flow_list: Vec<(String, String)> = edit
            .flows
            .iter()
            .map(|f| (f.id.clone(), f.name.clone()))
            .collect();
        let env = Env {
            ctx,
            tx: tx_ref,
            accent,
            profile: ed.profile,
            width: right.width(),
            trigger,
            mouse: &mouse,
            bounds: ed.card,
            flows: &flow_list,
        };
        ui.scope_builder(UiBuilder::new().max_rect(right), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(right)); // clip anything that overflows
            // the editor's width is computed from `right`, so its scrollbar must not take any of it
            ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
            egui::ScrollArea::vertical()
                .id_salt("inspector_scroll")
                .max_height(right.height())
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(right.width());
                    if let Some(ring) = ring_mut(&mut prof.items, &nav) {
                        if ed.adding {
                            add_panel(ctx, ed, ring, nav.len(), accent);
                        } else {
                            ed.add_catalog = None; // the closed sheet keeps nothing
                        }
                        item_inspector(ui, ed, &env, ring, &nav);
                    }
                });
        });
    }
}
