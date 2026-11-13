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
//!   marked with `!`. The list has a search box (the name of a variable, or part of it) and shows for each variable
//!   how many blocks set it (green) and read it (blue), the same colors as the rings on the canvas.
//! - **All blocks**: every block of the flow in the order of a run, with a search box that looks at
//!   the step number, the name of the block, the text typed in its fields and the variables it uses.
//!   A click (or Enter, for the first match) moves the canvas to the block and stays on this page.
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

/// Goes to a block from the list of blocks: the canvas moves to it and the list stays where it is
/// (the block is marked in the list). The panel is told the selection is already known, so it does
/// not jump to the Block page as it does for a click on the canvas.
fn open_block(ed: &mut Ed, id: u32) {
    go(ed, id);
    ed.flow_insp_seen = Some(id);
}

/// The pages of the panel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Block,
    Vars,
    Blocks,
}

/// The words of a search, lower case.
fn tokens(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// Whether `haystack` (lower case) holds every word of the search.
fn has_all(haystack: &str, words: &[String]) -> bool {
    words.iter().all(|w| haystack.contains(w.as_str()))
}

/// The search box at the top of a list.
fn search_box(ui: &mut egui::Ui, text: &mut String, hint: &str) -> egui::Response {
    let w = ui.available_width();
    ui.add(text_field(text).desired_width(w).hint_text(hint))
}

/// Whether Enter was pressed in the search box.
fn entered(ui: &egui::Ui, r: &egui::Response) -> bool {
    r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
}

/// The text typed in the fields of a block (its command, its labels, its values...), in the order
/// the fields are saved in.
fn kind_text(kind: &NodeKind) -> Vec<String> {
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => {
                let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
                if !s.is_empty() {
                    out.push(s);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(m) => m.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Ok(v) = serde_json::to_value(kind) {
        walk(&v, &mut out);
    }
    out
}

/// Every block with its step (`None` for a block no run reaches): the blocks a run reaches first,
/// in the order of the run, then the others.
fn block_order(flow: &Flow) -> Vec<(u32, Option<usize>)> {
    let reach: Vec<u32> = flow.reachable().iter().map(|n| n.id).collect();
    let mut order: Vec<(u32, Option<usize>)> = reach
        .iter()
        .enumerate()
        .map(|(p, &id)| (id, Some(p)))
        .collect();
    for n in &flow.nodes {
        if !reach.contains(&n.id) {
            order.push((n.id, None));
        }
    }
    order
}

/// The blocks that match a search, in the order of [`block_order`].
fn block_matches(flow: &Flow, query: &str) -> Vec<(u32, Option<usize>)> {
    let words = tokens(query);
    block_order(flow)
        .into_iter()
        .filter(|(id, step)| {
            if words.is_empty() {
                return true;
            }
            let Some(n) = flow.node(*id) else {
                return false;
            };
            let mut hay = format!("{} {}", kind_info(&n.kind).1, id);
            if let Some(s) = step {
                hay.push_str(&format!(" step {s}"));
            }
            for t in kind_text(&n.kind)
                .into_iter()
                .chain(block_reads(&n.kind))
                .chain(block_writes(&n.kind))
            {
                hay.push(' ');
                hay.push_str(&t);
            }
            has_all(&hay.to_lowercase(), &words)
        })
        .collect()
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
            ed.flow_insp_blocks = false;
        }
    }
    let page = if ed.flow_insp_blocks {
        Page::Blocks
    } else if sel.is_none() || ed.flow_insp_vars {
        Page::Vars
    } else {
        Page::Block
    };
    let mut ops: Vec<Op> = Vec::new();

    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(14.0)), |ui| {
        let mut items: Vec<(Option<char>, &str)> = Vec::new();
        if sel.is_some() {
            items.push((None, "Block"));
        }
        items.push((None, "Variables"));
        items.push((None, "All blocks"));
        let current = match page {
            Page::Block => 0,
            Page::Vars => usize::from(sel.is_some()),
            Page::Blocks => items.len() - 1,
        };
        if let Some(i) = segmented(ui, &items, current) {
            let name = items[i].1;
            ed.flow_insp_blocks = name == "All blocks";
            ed.flow_insp_vars = name == "Variables";
        }
        ui.add_space(GAP);

        // the search boxes stay in place while the list under them scrolls
        match page {
            Page::Vars => {
                let r = search_box(ui, &mut ed.flow_var_search, "Search variables\u{2026}");
                if entered(ui, &r)
                    && let Some(name) = first_variable(flow, &ed.flow_var_search)
                {
                    ed.flow_var = Some(name);
                }
                ui.add_space(GAP / 2.0);
            }
            Page::Blocks => {
                let r = search_box(
                    ui,
                    &mut ed.flow_block_search,
                    "Search blocks: name, text, variable, step\u{2026}",
                );
                if entered(ui, &r)
                    && let Some((id, _)) = block_matches(flow, &ed.flow_block_search).first()
                {
                    open_block(ed, *id);
                }
                ui.add_space(GAP / 2.0);
            }
            Page::Block => {}
        }

        egui::ScrollArea::vertical()
            .id_salt("flow_inspector")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                match (page, sel) {
                    (Page::Block, Some(id)) => {
                        block_page(ui, flow, ed, accent, others, id, &issues, &mut ops);
                    }
                    (Page::Blocks, _) => blocks_page(ui, flow, ed, accent, sel),
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

/// The name of the first variable that matches a search.
fn first_variable(flow: &Flow, query: &str) -> Option<String> {
    let words = tokens(query);
    variables(flow)
        .into_iter()
        .find(|r| has_all(&r.name.to_lowercase(), &words))
        .map(|r| r.name)
}

/// A line that says who is in a group of blocks (the ones that set a variable, the ones that read
/// it...), with the blocks as buttons under it.
fn link_group(
    ui: &mut egui::Ui,
    flow: &Flow,
    ed: &mut Ed,
    title: &str,
    color: Color32,
    ids: &[u32],
    none: &str,
) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, color);
        ui.label(RichText::new(title).size(12.5).color(pal().muted));
    });
    if ids.is_empty() {
        wrapped(ui, none, pal().muted);
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for &b in ids {
            if button(ui, Look::Plain, None, &block_label(flow, b))
                .on_hover_text("Go to this block")
                .clicked()
            {
                // like the list of blocks: the canvas moves to the block and the panel stays on
                // the Variables page, with the variable still followed
                open_block(ed, b);
            }
        }
    });
}

/// The two small counters at the right of a variable: how many blocks set it and read it, in the
/// colors of the rings on the canvas. Returns where the free room of the row ends.
fn count_pills(p: &egui::Painter, rect: Rect, sets: usize, reads: usize) -> f32 {
    let pills = [
        (
            format!("set {sets}"),
            if sets == 0 { pal().muted } else { SETS_C },
        ),
        (
            format!("read {reads}"),
            if reads == 0 { pal().muted } else { READS_C },
        ),
    ];
    let mut right = rect.max.x - 10.0;
    for (text, color) in pills.iter().rev() {
        let g = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), *color);
        let w = g.size().x + 24.0;
        let pill =
            Rect::from_min_size(egui::pos2(right - w, rect.center().y - 11.0), vec2(w, 22.0));
        p.rect_filled(pill, PILL, wash(*color, 0.14));
        p.circle_filled(egui::pos2(pill.min.x + 10.0, pill.center().y), 3.0, *color);
        p.galley(
            egui::pos2(pill.min.x + 18.0, pill.center().y - g.size().y / 2.0),
            g,
            *color,
        );
        right = pill.min.x - 6.0;
    }
    right
}

/// The page with every variable of the flow.
fn variables_page(ui: &mut egui::Ui, flow: &Flow, ed: &mut Ed, accent: Color32) {
    let all = variables(flow);
    if all.is_empty() {
        wrapped(
            ui,
            "No variable yet. A Set variable, Ask or Run block makes one.",
            pal().muted,
        );
        return;
    }
    let words = tokens(&ed.flow_var_search);
    let total = all.len();
    let rows: Vec<_> = all
        .into_iter()
        .filter(|r| has_all(&r.name.to_lowercase(), &words))
        .collect();
    let hint = if words.is_empty() {
        "Click a variable to follow it: the blocks that set it get a green ring, the ones that \
         read it a blue ring, and dashed lines show where its value travels."
            .to_string()
    } else {
        format!("{} of {} variables", rows.len(), total)
    };
    wrapped(ui, &hint, pal().muted);
    if rows.is_empty() {
        wrapped(ui, "No variable has this name.", pal().muted);
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
        let room = count_pills(p, rect, row.writers.len(), row.readers.len());
        let p = p.with_clip_rect(Rect::from_min_max(rect.min, egui::pos2(room, rect.max.y)));
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
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    if let Some(n) = builtin_note(&row.name) {
                        wrapped(ui, &format!("{n}."), pal().muted);
                    }
                    link_group(
                        ui,
                        flow,
                        ed,
                        "Set by",
                        SETS_C,
                        &row.writers,
                        "Nothing sets it.",
                    );
                    link_group(
                        ui,
                        flow,
                        ed,
                        "Read by",
                        READS_C,
                        &row.readers,
                        "Nothing reads it.",
                    );
                    if !row.unset.is_empty() {
                        link_group(
                            ui,
                            flow,
                            ed,
                            "Read before any block sets it, at",
                            pal().bad,
                            &row.unset,
                            "",
                        );
                    }
                });
            });
            ui.add_space(4.0);
        }
    }
}

/// The page with every block of the flow.
fn blocks_page(ui: &mut egui::Ui, flow: &Flow, ed: &mut Ed, accent: Color32, sel: Option<u32>) {
    let total = flow.nodes.len();
    let found = block_matches(flow, &ed.flow_block_search);
    let hint = if ed.flow_block_search.trim().is_empty() {
        format!("{total} blocks. Click one to go to it.")
    } else {
        format!(
            "{} of {} blocks. Enter goes to the first.",
            found.len(),
            total
        )
    };
    wrapped(ui, &hint, pal().muted);
    if found.is_empty() {
        wrapped(ui, "No block matches.", pal().muted);
        return;
    }
    for (id, step) in found {
        block_row(ui, flow, ed, accent, id, step, sel == Some(id));
    }
}

/// One block in the list of blocks: its icon, its name, the text typed in it and its step.
fn block_row(
    ui: &mut egui::Ui,
    flow: &Flow,
    ed: &mut Ed,
    accent: Color32,
    id: u32,
    step: Option<usize>,
    selected: bool,
) {
    let Some(n) = flow.node(id) else {
        return;
    };
    let (icon, title) = kind_info(&n.kind);
    let color = tint(&n.kind, accent);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::click());
    let fill = if selected {
        wash(accent, 0.16)
    } else if resp.hovered() {
        pal().hot
    } else {
        pal().input
    };
    let p = ui.painter();
    p.rect_filled(rect, CONTROL_R, fill);
    fonts::paint_icon(
        p,
        egui::pos2(rect.min.x + 22.0, rect.center().y),
        icon,
        18.0,
        color,
        false,
    );
    let note = match step {
        _ if matches!(n.kind, NodeKind::Start) => String::new(),
        Some(s) => format!("step {s}"),
        None => "not wired".to_string(),
    };
    p.text(
        egui::pos2(rect.max.x - 12.0, rect.min.y + 17.0),
        Align2::RIGHT_CENTER,
        note,
        FontId::proportional(12.0),
        pal().muted,
    );
    p.with_clip_rect(Rect::from_min_max(
        rect.min,
        egui::pos2(rect.max.x - 80.0, rect.max.y),
    ))
    .text(
        egui::pos2(rect.min.x + 42.0, rect.min.y + 17.0),
        Align2::LEFT_CENTER,
        title,
        FontId::new(13.5, fonts::semibold()),
        pal().text,
    );
    let snippet: String = kind_text(&n.kind)
        .into_iter()
        .filter(|t| t.as_str() != title)
        .take(3)
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
        .chars()
        .take(80)
        .collect();
    if !snippet.is_empty() {
        p.with_clip_rect(rect.shrink2(vec2(10.0, 0.0))).text(
            egui::pos2(rect.min.x + 42.0, rect.min.y + 34.0),
            Align2::LEFT_CENTER,
            snippet,
            FontId::proportional(12.0),
            pal().muted,
        );
    }
    if resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        open_block(ed, id);
    }
}
