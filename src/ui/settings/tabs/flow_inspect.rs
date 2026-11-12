//! The inspector of the Flow canvas: the panel at the right of it.
//!
//! ```text
//! +------------------------------------------+---------------------------+
//! |  canvas                                   | [ Block | Variables ]     |
//! |                                           |  Set variable      step 2 |
//! |     ( Set total )----( Command )          |  Settings                 |
//! |                                           |   [ variable name ]       |
//! |                                           |   [ value         ]       |
//! |                                           |  Reads / Writes           |
//! +------------------------------------------+---------------------------+
//! ```
//!
//! Clicking a block (its header or anywhere in its body) opens the panel at the right of the
//! canvas, which keeps the rest of the width. The panel has two pages:
//!
//! - **Block**: the settings of the selected block, the same fields the block shows (they edit
//!   the same data), with room to read what is typed; the problems of the block in words; and
//!   what the block does with variables: *Reads* (each variable it uses, whether a block before
//!   it sets it, and which blocks it may get the value from) and *Writes* (each variable it sets
//!   and which blocks read that value afterwards). Every block named there is a button that
//!   selects it and brings it to the middle of the canvas.
//! - **Variables**: every variable of the flow, with how many blocks set and read it. A click
//!   *follows* one: on the canvas the blocks that set it get a green ring, the blocks that read
//!   it a blue one, and dashed lines show the way its value travels (see
//!   [`crate::config::flow_check::data_edges`]). A variable that is read where no path sets it is
//!   marked with `!`.
//!
//! The button of the bar under the canvas opens the panel when no block is selected (it then shows
//! the Variables page). A click on the empty canvas deselects the block and closes the panel.

use super::flow::{AMBER, Op, apply_ops, controls, kind_info, tint};
use crate::config::flow_check::{
    block_issues, block_reads, block_writes, call_may_set, check_flow, data_edges, is_set_before,
    variables,
};
use crate::config::*;
use crate::msg::Msg;
use crate::ui::fonts;
use crate::ui::settings::state::{Ed, Slot};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use eframe::egui::{
    self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, UiBuilder, vec2,
};
use std::sync::mpsc::Sender;

/// Width of the panel.
pub(super) const SIDE_W: f32 = 400.0;
/// The ring and the label of a block that sets the variable being followed.
pub(super) const SETS_C: Color32 = Color32::from_rgb(34, 160, 98);
/// The ring and the label of a block that reads it, and the dashed lines of its value.
pub(super) const READS_C: Color32 = Color32::from_rgb(58, 123, 224);

/// Whether the inspector is on screen: a block is selected, or the person opened it.
pub(super) fn visible(ed: &Ed, flow: &Flow) -> bool {
    ed.flow_insp || ed.flow_sel.is_some_and(|id| flow.node(id).is_some())
}

/// How a block is named in a list: its step number (the one in its header) and its kind.
fn block_label(flow: &Flow, id: u32) -> String {
    let Some(n) = flow.node(id) else {
        return "?".into();
    };
    let title = kind_info(&n.kind).1;
    match flow.reachable().iter().position(|r| r.id == id) {
        Some(p) if !matches!(n.kind, NodeKind::Start) => format!("{p} \u{b7} {title}"),
        _ => title.to_string(),
    }
}

/// What the variables every flow has are for.
fn builtin_note(name: &str) -> Option<&'static str> {
    match name {
        "ok" => Some("1 when the block did its work, 0 when it could not"),
        "index" => Some("the round of a loop or the position of an item, counted from 1"),
        "code" => Some("the exit code of the command (0 means it worked)"),
        _ => None,
    }
}

/// Selects a block and brings it to the middle of the canvas.
fn go(ed: &mut Ed, id: u32) {
    ed.flow_sel = Some(id);
    ed.flow_focus = Some(id);
}

/// A small button with a variable's name as it is written in a field, `{{name}}`.
fn name_chip(ui: &mut egui::Ui, name: &str, followed: bool, accent: Color32) -> egui::Response {
    let look = if followed {
        Look::Primary(accent)
    } else {
        Look::Soft
    };
    let r = button(ui, look, None, &format!("{{{{{name}}}}}"));
    let tip = builtin_note(name).map_or_else(
        || "Follow this variable on the canvas".to_string(),
        |n| format!("{n}. Click to follow it on the canvas"),
    );
    r.on_hover_text(tip)
}

/// A row of buttons, one per block, that go to the block.
fn block_links(ui: &mut egui::Ui, flow: &Flow, ed: &mut Ed, lead: &str, ids: &[u32]) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(lead).size(12.5).color(pal().muted));
        for &b in ids {
            if button(ui, Look::Plain, None, &block_label(flow, b))
                .on_hover_text("Go to this block")
                .clicked()
            {
                go(ed, b);
            }
        }
    });
}

fn wrapped(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap());
}

/// Draws the panel in `rect` and carries out what its fields asked for.
#[allow(clippy::too_many_arguments)]
pub(super) fn panel(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rect: Rect,
    flow: &mut Flow,
    ed: &mut Ed,
    accent: Color32,
    others: &[(String, String)],
    tx: &Sender<Msg>,
) {
    let issues = {
        let known: Vec<&str> = others
            .iter()
            .map(|(id, _)| id.as_str())
            .chain(std::iter::once(flow.id.as_str()))
            .collect();
        check_flow(flow, &known)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, PANEL_R, pal().card);
    painter.rect_stroke(
        rect,
        PANEL_R,
        Stroke::new(1.0, pal().border),
        StrokeKind::Inside,
    );

    let sel = ed.flow_sel.filter(|id| flow.node(*id).is_some());
    // a new selection brings the panel back to the block
    if ed.flow_insp_seen != sel {
        ed.flow_insp_seen = sel;
        if sel.is_some() {
            ed.flow_insp_vars = false;
        }
    }
    let show_vars = sel.is_none() || ed.flow_insp_vars;
    let mut ops: Vec<Op> = Vec::new();

    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(14.0)), |ui| {
        if sel.is_some() {
            let items = [(None, "Block"), (None, "Variables")];
            if let Some(i) = segmented(ui, &items, usize::from(show_vars)) {
                ed.flow_insp_vars = i == 1;
            }
            ui.add_space(GAP);
        }
        egui::ScrollArea::vertical()
            .id_salt("flow_inspector")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                match sel {
                    Some(id) if !show_vars => {
                        block_page(ui, flow, ed, accent, others, id, &issues, &mut ops);
                    }
                    _ => variables_page(ui, flow, ed, accent),
                }
            });
    });
    apply_ops(ops, ctx, flow, ed, tx);
}

/// The page of the selected block.
#[allow(clippy::too_many_arguments)]
fn block_page(
    ui: &mut egui::Ui,
    flow: &mut Flow,
    ed: &mut Ed,
    accent: Color32,
    others: &[(String, String)],
    id: u32,
    issues: &[crate::config::flow_check::Issue],
    ops: &mut Vec<Op>,
) {
    let Some(idx) = flow.nodes.iter().position(|n| n.id == id) else {
        return;
    };
    let (icon, title) = kind_info(&flow.nodes[idx].kind);
    let color = tint(&flow.nodes[idx].kind, accent);
    let is_start = matches!(flow.nodes[idx].kind, NodeKind::Start);
    let step = flow.reachable().iter().position(|r| r.id == id);

    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
        fonts::paint_icon(ui.painter(), r.center(), icon, 18.0, color, false);
        ui.label(fonts::title(title, 16.0, pal().text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let note = match step {
                _ if is_start => String::new(),
                Some(p) => format!("step {p}"),
                None => "not wired".to_string(),
            };
            ui.label(RichText::new(note).size(12.5).color(pal().muted));
        });
    });

    // what is wrong with the block
    for i in block_issues(issues, id) {
        let c = if i.severity == Severity::Error {
            pal().bad
        } else {
            AMBER
        };
        wrapped(ui, &i.message, c);
    }

    // the settings: the same fields the block has, edited here
    ui.add_space(4.0);
    section_title(ui, "Settings");
    if flow.nodes[idx].kind.is_marker() {
        let text = match flow.nodes[idx].kind {
            NodeKind::Start => "Where every run begins.",
            NodeKind::End => "Where a run ends.",
            _ => "Ends the whole run, also when another flow called this one.",
        };
        wrapped(ui, text, pal().muted);
    } else {
        let recording = ed
            .recording
            .as_ref()
            .is_some_and(|r| r.profile == ed.flow && r.slot == Slot::Node(id));
        ui.push_id(("flow_inspector_controls", id), |ui| {
            controls(
                ui,
                &mut flow.nodes[idx].kind,
                others,
                recording,
                accent,
                id,
                ops,
            );
        });
    }

    // what the block does with variables
    let flow: &Flow = flow;
    let kind = &flow.nodes[idx].kind;
    let reads = block_reads(kind);
    let writes = block_writes(kind);
    let edges_of = |name: &str| data_edges(flow, name);

    ui.add_space(8.0);
    section_title(ui, "Reads");
    if reads.is_empty() {
        wrapped(ui, "This block reads no variable.", pal().muted);
    }
    for name in &reads {
        let followed = ed.flow_var.as_deref() == Some(name.as_str());
        ui.horizontal_wrapped(|ui| {
            if name_chip(ui, name, followed, accent).clicked() {
                ed.flow_var = if followed { None } else { Some(name.clone()) };
            }
            match is_set_before(flow, id, name) {
                Some(true) => {
                    ui.label(
                        RichText::new("set before this block")
                            .size(12.5)
                            .color(pal().muted),
                    );
                }
                Some(false) => {
                    ui.label(
                        RichText::new("never set before this block")
                            .size(12.5)
                            .color(pal().bad),
                    );
                }
                None => {
                    ui.label(
                        RichText::new("block not wired")
                            .size(12.5)
                            .color(pal().muted),
                    );
                }
            }
        });
        let feeders: Vec<u32> = edges_of(name)
            .into_iter()
            .filter(|(_, r)| *r == id)
            .map(|(w, _)| w)
            .collect();
        if feeders.is_empty() {
            let setters: Vec<u32> = flow
                .nodes
                .iter()
                .filter(|n| block_writes(&n.kind).contains(name))
                .map(|n| n.id)
                .collect();
            if !setters.is_empty() {
                block_links(
                    ui,
                    flow,
                    ed,
                    "set only by blocks that do not lead here:",
                    &setters,
                );
            }
        } else {
            block_links(ui, flow, ed, "value may come from", &feeders);
        }
    }

    ui.add_space(8.0);
    section_title(ui, "Writes");
    if writes.is_empty() {
        wrapped(ui, "This block sets no variable.", pal().muted);
    }
    for name in &writes {
        let followed = ed.flow_var.as_deref() == Some(name.as_str());
        ui.horizontal_wrapped(|ui| {
            if name_chip(ui, name, followed, accent).clicked() {
                ed.flow_var = if followed { None } else { Some(name.clone()) };
            }
        });
        let readers: Vec<u32> = edges_of(name)
            .into_iter()
            .filter(|(w, _)| *w == id)
            .map(|(_, r)| r)
            .collect();
        if readers.is_empty() {
            wrapped(ui, "no block reads this value afterwards", pal().muted);
        } else {
            block_links(ui, flow, ed, "read afterwards by", &readers);
        }
    }
    if call_may_set(kind) {
        wrapped(
            ui,
            "Runs another flow with the same variables: it may set any of them.",
            pal().muted,
        );
    }
}

/// The page with every variable of the flow.
fn variables_page(ui: &mut egui::Ui, flow: &Flow, ed: &mut Ed, accent: Color32) {
    ui.label(fonts::title("Variables", 16.0, pal().text));
    wrapped(
        ui,
        "Click a variable to follow it. On the canvas the blocks that set it are ringed in green, \
         the blocks that read it in blue, and dashed lines show where its value travels.",
        pal().muted,
    );
    ui.add_space(4.0);
    let rows = variables(flow);
    if rows.is_empty() {
        wrapped(
            ui,
            "No variable yet. A Set variable, Ask or Run block makes one.",
            pal().muted,
        );
        return;
    }
    for row in rows {
        let followed = ed.flow_var.as_deref() == Some(row.name.as_str());
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_H), Sense::click());
        let fill = if followed {
            wash(accent, 0.16)
        } else if resp.hovered() {
            pal().hot
        } else {
            pal().input
        };
        let p = ui.painter();
        p.rect_filled(rect, CONTROL_R, fill);
        let mut x = rect.min.x + 12.0;
        if !row.unset.is_empty() {
            p.text(
                rect.left_center() + vec2(12.0, 0.0),
                Align2::LEFT_CENTER,
                "!",
                FontId::new(14.0, fonts::semibold()),
                pal().bad,
            );
            x += 14.0;
        }
        p.text(
            egui::pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            &row.name,
            FontId::new(13.5, fonts::semibold()),
            pal().text,
        );
        p.text(
            rect.right_center() - vec2(12.0, 0.0),
            Align2::RIGHT_CENTER,
            format!(
                "{} set \u{b7} {} read",
                row.writers.len(),
                row.readers.len()
            ),
            FontId::proportional(12.0),
            pal().muted,
        );
        if resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            ed.flow_var = if followed {
                None
            } else {
                Some(row.name.clone())
            };
        }
        if followed {
            if let Some(n) = builtin_note(&row.name) {
                wrapped(ui, &format!("{n}."), pal().muted);
            }
            if row.writers.is_empty() {
                wrapped(ui, "Nothing sets it.", pal().muted);
            } else {
                block_links(ui, flow, ed, "set by", &row.writers);
            }
            if row.readers.is_empty() {
                wrapped(ui, "Nothing reads it.", pal().muted);
            } else {
                block_links(ui, flow, ed, "read by", &row.readers);
            }
            if !row.unset.is_empty() {
                block_links(
                    ui,
                    flow,
                    ed,
                    "read before any block sets it, at",
                    &row.unset,
                );
            }
            ui.add_space(4.0);
        }
    }
}
