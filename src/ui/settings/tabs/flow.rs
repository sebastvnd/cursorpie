//! Flow tab: a node editor for chains of steps, like the graphs of Unity or Blender.
//!
//! ```text
//! [ Flow  v ] [ name ]  [+] [copy] [trash] [export] [import]
//! Add a step:  [Shortcut] [App] [URL / file] [Command] [Wait]                [reset view]
//! +--------------------------------------------------------------------------------------+
//! |  ( Start )o--------o( Shortcut        1 x)o--------o( Wait         2 x)o-----o( End )  |
//! |                     [ Cmd + C           ]            [ 500 ms           ]               |
//! +--------------------------------------------------------------------------------------+
//! ```
//!
//! Every block has one dot on the right (its output, except *End*) and, except *Start*, one on the
//! left (its input). Drag from an output dot to an input dot to wire two blocks; click a wired input dot to
//! unplug it; drag the header of a block to move it. The canvas moves with a drag on its
//! background, the middle button or the scroll of a trackpad or wheel, and zooms with Alt + scroll
//! or a pinch ([`view_input`]). The flow runs
//! the blocks in the order the wires lead from *Start* (the number in each header), and ignores
//! blocks that are not wired in. The data model, the rules that keep a flow valid and the share
//! file are in [`crate::config::flow`]; this file only draws and edits.
//!
//! A slice runs a flow through its action: Menu > a slice > **Flow**.

use crate::config::flow::{COORD_LIMIT, export_flow_to_downloads, fresh_id, unique_name};
use crate::config::*;
use crate::keys::{Shortcut, key};
use crate::msg::Msg;
use crate::platform;
use crate::tap::{RECORDING, SWALLOW_KEY};
use crate::ui::app::App;
use crate::ui::draw::rgb;
use crate::ui::fonts;
use crate::ui::settings::picker::spawn_file_picker;
use crate::ui::settings::state::{Ed, PickFor, Rec, Slot};
use crate::ui::settings::tabs::profile_toolbar::{flow_app_sheet, open_picker};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{
    self, Align2, Color32, FontId, PointerButton, Pos2, Rect, RichText, Sense, Shape, Stroke,
    StrokeKind, UiBuilder, Vec2, emath::TSTransform, pos2, vec2,
};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;

/// Height of the header of a block (title, step number, remove button; the dots sit in it).
const HEAD_H: f32 = 36.0;
/// Height of an ordinary block: the header and one control.
const NODE_H: f32 = 92.0;
/// Radius of the dots.
const PORT_R: f32 = 6.0;
/// Side of the square that reacts to the pointer around a dot.
const PORT_HIT: f32 = 22.0;
/// How close to an input dot a wire has to be dropped, in points.
const DROP_R: f32 = 18.0;

/// What the toolbar (or the empty page) asked for this frame.
enum Do {
    New,
    Duplicate,
    Delete,
    Import,
    Export,
    Pick(usize),
    ResetView,
    Add(NodeKind),
}

/// What the canvas asked for this frame. They are applied after the blocks are drawn, so the flow
/// is not changed while it is being walked.
enum Op {
    Connect(u32, u32),
    Unplug(u32),
    Remove(u32),
    Record(u32),
    PickApp(u32),
}

/// How far the canvas can be zoomed out and in. The text is drawn once at its normal size and
/// then scaled, so it is only sharp up to a little over 100%: further in it turns blurry.
const ZOOM_MIN: f32 = 0.3;
const ZOOM_MAX: f32 = 1.25;

impl App {
    /// Draws the Flow tab into `ui`: the toolbar and, under it, the canvas of the selected flow.
    pub(in crate::ui::settings) fn flow_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let App { edit, ed, tx, .. } = self;
        let tx_ref: &Sender<Msg> = tx;
        let accent = rgb(edit.accent);
        ed.flow = ed.flow.min(edit.flows.len().saturating_sub(1));

        let todo = if edit.flows.is_empty() {
            empty_page(ui, accent)
        } else {
            toolbar(ui, edit, ed)
        };
        if let Some(todo) = todo {
            run(todo, ctx, edit, ed, tx_ref);
        }
        if edit.flows.is_empty() {
            return;
        }
        ui.add_space(GAP);
        let fi = ed.flow;
        canvas(ui, ctx, &mut edit.flows[fi], ed, fi, accent, tx_ref);
        flow_app_sheet(ctx, edit, ed, tx_ref, accent);
    }
}

/// A short message in the status bar (works from anywhere that has the channel).
fn notice(tx: &Sender<Msg>, ctx: &egui::Context, text: impl Into<String>) {
    let _ = tx.send(Msg::Notice(text.into()));
    ctx.request_repaint();
}

/// Makes flow `i` the one on the canvas, with a fresh view.
fn select_flow(ed: &mut Ed, i: usize) {
    ed.flow = i;
    ed.flow_sel = None;
    ed.flow_wire = None;
    ed.flow_pan = Vec2::ZERO;
    ed.flow_zoom = 1.0;
    ed.stop_recording();
}

/// What there is to say when no flow exists yet.
fn empty_page(ui: &mut egui::Ui, accent: Color32) -> Option<Do> {
    let mut todo = None;
    panel(ui, "Flows", |ui| {
        note(
            ui,
            "A flow is a chain of steps (shortcuts, apps, links, commands, waits) that a slice \
             runs one after the other. Build it here with blocks and wires, then pick it as the \
             action of a slice in the Menu tab. Flows can be exported and imported, so you can \
             share them.",
        );
        actions(ui, |ui| {
            if button(ui, Look::Primary(accent), Some(symbols::PLUS), "New flow").clicked() {
                todo = Some(Do::New);
            }
            if button(
                ui,
                Look::Soft,
                Some(symbols::DOWNLOAD_SIMPLE),
                "Import\u{2026}",
            )
            .clicked()
            {
                todo = Some(Do::Import);
            }
        });
    });
    todo
}

/// The two rows above the canvas: the flow (pick, name, new, copy, delete, share) and the blocks
/// that can be added.
fn toolbar(ui: &mut egui::Ui, edit: &mut Config, ed: &Ed) -> Option<Do> {
    let mut todo = None;
    let fi = ed.flow;
    let names: Vec<String> = edit.flows.iter().map(|f| f.name.clone()).collect();
    let can_add = edit.flows[fi].nodes.len() < MAX_FLOW_NODES;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        if let Some(i) = select(ui, 200.0, labels[fi], &labels, fi)
            && i != fi
        {
            todo = Some(Do::Pick(i));
        }
        ui.add(
            text_field(&mut edit.flows[fi].name)
                .desired_width(190.0)
                .hint_text("Name"),
        );
        if icon_button_look(ui, Look::Soft, symbols::PLUS, "New flow", true).clicked() {
            todo = Some(Do::New);
        }
        if icon_button_look(ui, Look::Soft, symbols::COPY, "Duplicate this flow", true).clicked() {
            todo = Some(Do::Duplicate);
        }
        if icon_button_look(ui, Look::Danger, symbols::TRASH, "Delete this flow", true).clicked() {
            todo = Some(Do::Delete);
        }
        if icon_button_look(
            ui,
            Look::Soft,
            symbols::UPLOAD_SIMPLE,
            "Export this flow\u{2026}",
            true,
        )
        .clicked()
        {
            todo = Some(Do::Export);
        }
        if icon_button_look(
            ui,
            Look::Soft,
            symbols::DOWNLOAD_SIMPLE,
            "Import a flow\u{2026}",
            true,
        )
        .clicked()
        {
            todo = Some(Do::Import);
        }
    });
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        muted(ui, "Add a step:");
        let kinds: [(char, &str, NodeKind); 5] = [
            (
                symbols::KEYBOARD,
                "Shortcut",
                NodeKind::Key(Shortcut::new(key::F13, 0)),
            ),
            (symbols::APP_WINDOW, "App", NodeKind::OpenApp(String::new())),
            (
                symbols::LINK,
                "URL / file",
                NodeKind::OpenUrl(String::new()),
            ),
            (
                symbols::TERMINAL_WINDOW,
                "Command",
                NodeKind::Command(String::new()),
            ),
            (symbols::CLOCK, "Wait", NodeKind::Wait(500)),
        ];
        for (icon, label, kind) in kinds {
            if button_if(ui, can_add, Look::Soft, Some(icon), label).clicked() {
                todo = Some(Do::Add(kind));
            }
        }
        if icon_button_look(
            ui,
            Look::Plain,
            symbols::ARROWS_IN,
            "Bring the blocks back into view",
            true,
        )
        .clicked()
        {
            todo = Some(Do::ResetView);
        }
    });
    todo
}

/// Carries out what the toolbar asked for.
fn run(todo: Do, ctx: &egui::Context, edit: &mut Config, ed: &mut Ed, tx: &Sender<Msg>) {
    match todo {
        Do::New => {
            if edit.flows.len() >= MAX_FLOWS {
                notice(
                    tx,
                    ctx,
                    format!("There are already {MAX_FLOWS} flows. Delete one first."),
                );
                return;
            }
            let id = fresh_id(&edit.flows);
            let name = unique_name(&edit.flows, "New flow");
            edit.flows.push(Flow::new(&id, &name));
            select_flow(ed, edit.flows.len() - 1);
        }
        Do::Duplicate => {
            if edit.flows.len() >= MAX_FLOWS {
                notice(
                    tx,
                    ctx,
                    format!("There are already {MAX_FLOWS} flows. Delete one first."),
                );
                return;
            }
            let Some(mut copy) = edit.flows.get(ed.flow).cloned() else {
                return;
            };
            copy.id = fresh_id(&edit.flows);
            copy.name = unique_name(&edit.flows, &format!("{} copy", copy.name));
            edit.flows.push(copy);
            select_flow(ed, edit.flows.len() - 1);
        }
        Do::Delete => {
            if ed.flow < edit.flows.len() {
                edit.flows.remove(ed.flow);
                let i = ed.flow.min(edit.flows.len().saturating_sub(1));
                select_flow(ed, i);
                notice(
                    tx,
                    ctx,
                    "Flow deleted. A slice that used it does nothing until you pick another. Undo brings it back.",
                );
            }
        }
        Do::Import => spawn_file_picker(
            tx.clone(),
            ctx.clone(),
            platform::FileKind::Json,
            "Choose a CursorPie flow",
            |file| Some(Msg::FlowImportPicked(file)),
        ),
        Do::Export => {
            if let Some(flow) = edit.flows.get(ed.flow) {
                match export_flow_to_downloads(flow) {
                    Ok(path) => {
                        platform::reveal_in_file_manager(&path);
                        notice(tx, ctx, format!("Exported to {}", path.display()));
                    }
                    Err(e) => notice(tx, ctx, format!("Export failed: {e}")),
                }
            }
        }
        Do::Pick(i) => {
            if i < edit.flows.len() {
                select_flow(ed, i);
            }
        }
        Do::ResetView => {
            let (mx, my) = edit
                .flows
                .get(ed.flow)
                .map(|f| {
                    f.nodes
                        .iter()
                        .fold((f32::MAX, f32::MAX), |(a, b), n| (a.min(n.x), b.min(n.y)))
                })
                .filter(|(a, b)| a.is_finite() && b.is_finite() && *a < f32::MAX && *b < f32::MAX)
                .unwrap_or((40.0, 40.0));
            ed.flow_zoom = 1.0;
            ed.flow_pan = vec2(40.0 - mx, 40.0 - my);
        }
        Do::Add(kind) => {
            let sel = ed.flow_sel;
            if let Some(flow) = edit.flows.get_mut(ed.flow)
                && flow.nodes.len() < MAX_FLOW_NODES
            {
                let id = flow.add_after(sel, kind);
                ed.flow_sel = Some(id);
            }
        }
    }
}

// ------------------------------------------------------------------ drawing

/// The icon and the title of a block.
fn kind_info(kind: &NodeKind) -> (char, &'static str) {
    match kind {
        NodeKind::Start => (symbols::PLAY, "Start"),
        NodeKind::End => (symbols::FLAG, "End"),
        NodeKind::Key(_) => (symbols::KEYBOARD, "Shortcut"),
        NodeKind::OpenApp(_) => (symbols::APP_WINDOW, "Open app"),
        NodeKind::OpenUrl(_) => (symbols::LINK, "Open URL / file"),
        NodeKind::Command(_) => (symbols::TERMINAL_WINDOW, "Shell command"),
        NodeKind::Wait(_) => (symbols::CLOCK, "Wait"),
    }
}

/// The color of the header of a block: a shell command stands out in red.
fn tint(kind: &NodeKind, accent: Color32) -> Color32 {
    match kind {
        NodeKind::Command(_) => pal().bad,
        NodeKind::Wait(_) => pal().muted,
        _ => accent,
    }
}

/// How tall a block is.
fn node_h(kind: &NodeKind) -> f32 {
    if matches!(kind, NodeKind::Start | NodeKind::End) {
        HEAD_H
    } else {
        NODE_H
    }
}

/// Points along a smooth wire from the output dot `a` to the input dot `b`.
fn wire_points(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let dx = ((b.x - a.x).abs() * 0.5).max(48.0);
    let (c1, c2) = (a + vec2(dx, 0.0), b - vec2(dx, 0.0));
    const N: usize = 24;
    (0..=N)
        .map(|i| {
            let t = i as f32 / N as f32;
            let u = 1.0 - t;
            let (w0, w1, w2, w3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            pos2(
                w0 * a.x + w1 * c1.x + w2 * c2.x + w3 * b.x,
                w0 * a.y + w1 * c1.y + w2 * c2.y + w3 * b.y,
            )
        })
        .collect()
}

/// Draws a wire.
fn draw_wire(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32, width: f32) {
    painter.add(Shape::line(wire_points(a, b), Stroke::new(width, color)));
}

/// Moves and zooms the canvas the way a node editor does:
/// * drag the empty canvas, or hold the middle button anywhere on it, to move;
/// * a mouse wheel or two fingers on a trackpad move it too;
/// * Alt (Option) + scroll, a pinch, or Ctrl/Cmd + scroll zoom around the pointer.
fn view_input(ctx: &egui::Context, ed: &mut Ed, rect: Rect, bg: &egui::Response) {
    if ed.sheet_open() {
        ed.flow_mid = false;
        crate::tap::FLOW_CANVAS_HOVER.store(false, Ordering::SeqCst);
        return;
    }
    let (pos, pressed, down, motion, scroll, pinch, alt) = ctx.input(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.button_pressed(PointerButton::Middle),
            i.pointer.middle_down(),
            i.pointer.delta(),
            i.smooth_scroll_delta,
            i.zoom_delta(),
            i.modifiers.alt,
        )
    });
    let over = pos.is_some_and(|p| rect.contains(p));
    // The trigger button is the middle one by default and the tap swallows it while Settings is
    // open; over this canvas it is let through, so holding it can move the canvas.
    crate::tap::FLOW_CANVAS_HOVER.store(over, Ordering::SeqCst);

    if bg.dragged_by(PointerButton::Primary) {
        ed.flow_pan += bg.drag_delta();
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if pressed && over {
        ed.flow_mid = true;
    }
    if !down {
        ed.flow_mid = false;
    }
    if ed.flow_mid {
        ed.flow_pan += motion;
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        ctx.request_repaint();
    }

    if !over {
        return;
    }
    let mut factor = 1.0;
    if pinch != 1.0 {
        factor = pinch;
    } else if alt && scroll != Vec2::ZERO {
        let d = if scroll.y != 0.0 { scroll.y } else { scroll.x };
        factor = (d * 0.0025).exp();
    } else if scroll != Vec2::ZERO {
        ed.flow_pan += scroll;
    }
    if factor != 1.0 {
        let old = ed.flow_zoom;
        let new = (old * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        // keep the point under the pointer where it is
        let at = pos.unwrap_or_else(|| rect.center()) - rect.min;
        let scene_at = (at - ed.flow_pan) / old;
        ed.flow_pan = at - scene_at * new;
        ed.flow_zoom = new;
    }
}

/// Draws the canvas of `flow`: the grid, the wires, the blocks with their controls, and handles
/// what the pointer does there.
fn canvas(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    flow: &mut Flow,
    ed: &mut Ed,
    fi: usize,
    accent: Color32,
    tx: &Sender<Msg>,
) {
    let avail = ui.available_rect_before_wrap();
    let rect = Rect::from_min_size(avail.min, vec2(avail.width(), avail.height().max(220.0)));
    let mut ops: Vec<Op> = Vec::new();

    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(rect));
        // the background takes the dragging; the blocks live in a layer above it and win
        let (_, bg) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
        // `screen` paints in the window's own coordinates (background, grid, texts that do not
        // zoom); `painter` below paints in the coordinates of the blocks
        let screen = ui.painter_at(rect);
        screen.rect_filled(rect, PANEL_R, pal().input);
        screen.rect_stroke(
            rect,
            PANEL_R,
            Stroke::new(1.0, pal().border),
            StrokeKind::Inside,
        );
        view_input(ctx, ed, rect, &bg);
        if bg.clicked() {
            ed.flow_sel = None;
        }
        let to_global = TSTransform::new(rect.min.to_vec2() + ed.flow_pan, ed.flow_zoom);
        let from_global = to_global.inverse();

        // a grid of dots that moves and zooms with the canvas (every second dot, and so on, when
        // zoomed out far enough that they would crowd)
        let mut step = 28.0 * ed.flow_zoom;
        while step < 14.0 {
            step *= 2.0;
        }
        let (ox, oy) = (ed.flow_pan.x.rem_euclid(step), ed.flow_pan.y.rem_euclid(step));
        let dot = pal().muted.gamma_multiply(0.55);
        let mut y = rect.min.y + oy;
        while y < rect.max.y {
            let mut x = rect.min.x + ox;
            while x < rect.max.x {
                screen.circle_filled(pos2(x, y), 1.5, dot);
                x += step;
            }
            y += step;
        }

        // the blocks and wires: a layer of their own that is zoomed and moved as a whole, so text,
        // fields and hit areas all follow
        let layer = egui::LayerId::new(ui.layer_id().order, ui.id().with(("flow_scene", fi)));
        ctx.set_sublayer(ui.layer_id(), layer);
        ctx.set_transform_layer(layer, to_global);
        let mut scene = ui.new_child(
            UiBuilder::new()
                .layer_id(layer)
                .max_rect(Rect::from_center_size(Pos2::ZERO, Vec2::splat(400_000.0))),
        );
        scene.set_clip_rect(from_global * rect);
        let ui = &mut scene;
        let painter = ui.painter().clone();
        let origin = Pos2::ZERO;

        let chain_ids: Vec<u32> = flow.chain().iter().map(|n| n.id).collect();
        let pointer = ctx
            .input(|i| i.pointer.latest_pos())
            .map(|p| from_global * p);

        // where every dot is, before anything moves
        let mut outs: BTreeMap<u32, Pos2> = BTreeMap::new();
        let mut ins: Vec<(u32, Pos2)> = Vec::new();
        for n in &flow.nodes {
            let top = origin + vec2(n.x, n.y);
            outs.insert(n.id, pos2(top.x + n.kind.width(), top.y + HEAD_H / 2.0));
            if !matches!(n.kind, NodeKind::Start) {
                ins.push((n.id, pos2(top.x, top.y + HEAD_H / 2.0)));
            }
        }

        // the wires, under the blocks
        for n in &flow.nodes {
            if let Some(t) = n.next
                && let Some(a) = outs.get(&n.id)
                && let Some((_, b)) = ins.iter().find(|(id, _)| *id == t)
            {
                let live = chain_ids.contains(&n.id);
                let color = if live {
                    accent
                } else {
                    pal().muted.gamma_multiply(0.7)
                };
                draw_wire(&painter, *a, *b, color, 2.5);
            }
        }

        if flow.nodes.len() <= 2 {
            screen.text(
                rect.center() + vec2(0.0, 30.0),
                Align2::CENTER_CENTER,
                "Add steps with the buttons above.\nThen drag from the dot on the right of a block\nto the dot on the left of the next one.",
                FontId::proportional(13.5),
                pal().muted,
            );
        }

        // the blocks; the selected one last, so it is on top
        let sel = ed.flow_sel;
        let wiring = ed.flow_wire;
        let mut order: Vec<usize> = (0..flow.nodes.len()).collect();
        order.sort_by_key(|&i| Some(flow.nodes[i].id) == sel);
        for i in order {
            let id = flow.nodes[i].id;
            let is_start = matches!(flow.nodes[i].kind, NodeKind::Start);
            let is_end = matches!(flow.nodes[i].kind, NodeKind::End);
            // Start and End are only a header
            let short = is_start || is_end;
            let size = vec2(flow.nodes[i].kind.width(), node_h(&flow.nodes[i].kind));
            let mut top = origin + vec2(flow.nodes[i].x, flow.nodes[i].y);

            // the header moves the block
            let head = Rect::from_min_size(top, vec2(size.x, HEAD_H));
            let hr = ui.interact(head, egui::Id::new(("flow_head", fi, id)), Sense::click_and_drag());
            if hr.clicked() || hr.drag_started() {
                ed.flow_sel = Some(id);
            }
            if hr.dragged_by(PointerButton::Primary) {
                let d = hr.drag_delta();
                let n = &mut flow.nodes[i];
                n.x = (n.x + d.x).clamp(-COORD_LIMIT, COORD_LIMIT);
                n.y = (n.y + d.y).clamp(-COORD_LIMIT, COORD_LIMIT);
                top += d;
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            }

            let body_rect = Rect::from_min_size(top, size);
            let head = Rect::from_min_size(top, vec2(size.x, HEAD_H));
            let selected = ed.flow_sel == Some(id);
            let (icon, title) = kind_info(&flow.nodes[i].kind);
            let color = tint(&flow.nodes[i].kind, accent);

            painter.rect_filled(body_rect, CONTROL_R, pal().card);
            let r = CONTROL_R_PT;
            painter.rect_filled(
                head,
                egui::CornerRadius {
                    nw: r,
                    ne: r,
                    sw: if short { r } else { 0 },
                    se: if short { r } else { 0 },
                },
                wash(color, 0.16),
            );
            painter.rect_stroke(
                body_rect,
                CONTROL_R,
                if selected {
                    Stroke::new(2.0, accent)
                } else {
                    Stroke::new(1.0, pal().border)
                },
                StrokeKind::Inside,
            );
            fonts::paint_icon(
                &painter,
                pos2(head.min.x + 24.0, head.center().y),
                icon,
                15.0,
                if matches!(flow.nodes[i].kind, NodeKind::Wait(_)) {
                    pal().text
                } else {
                    color
                },
                false,
            );
            painter.text(
                pos2(head.min.x + 42.0, head.center().y),
                Align2::LEFT_CENTER,
                title,
                FontId::new(13.5, fonts::semibold()),
                pal().text,
            );

            if !is_start {
                // the step number, or a hint that the block is not wired in
                match chain_ids.iter().position(|c| *c == id) {
                    Some(p) => {
                        if !is_end {
                            painter.text(
                                pos2(head.max.x - 44.0, head.center().y),
                                Align2::CENTER_CENTER,
                                p.to_string(),
                                FontId::new(12.0, fonts::semibold()),
                                pal().muted,
                            );
                        }
                    }
                    None => {
                        painter.text(
                            pos2(head.max.x - 34.0, head.center().y),
                            Align2::RIGHT_CENTER,
                            "not wired",
                            FontId::proportional(11.5),
                            pal().muted,
                        );
                    }
                }
                // remove (End stays)
                let xr = Rect::from_center_size(
                    pos2(head.max.x - 16.0, head.center().y),
                    Vec2::splat(22.0),
                );
                let xresp = ui.interact(xr, egui::Id::new(("flow_del", fi, id)), Sense::click());
                fonts::paint_icon(
                    &painter,
                    xr.center(),
                    symbols::X,
                    12.0,
                    if xresp.hovered() { pal().bad } else { pal().muted },
                    false,
                );
                if !is_end && xresp.on_hover_text("Remove this step").clicked() {
                    ops.push(Op::Remove(id));
                }
            }

            // output dot: drag from it to wire (End has none)
            if !is_end {
                let out_c = pos2(top.x + size.x, top.y + HEAD_H / 2.0);
                let out_r = ui.interact(
                    Rect::from_center_size(out_c, Vec2::splat(PORT_HIT)),
                    egui::Id::new(("flow_out", fi, id)),
                    Sense::click_and_drag(),
                );
                if out_r.drag_started_by(PointerButton::Primary) {
                    ed.flow_wire = Some(id);
                    ed.flow_sel = Some(id);
                }
                if out_r.hovered() {
                    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                let wired_out = flow.nodes[i].next.is_some() || wiring == Some(id);
                painter.circle_filled(out_c, PORT_R, if wired_out { accent } else { pal().card });
                painter.circle_stroke(out_c, PORT_R, Stroke::new(2.0, accent));
            }

            // input dot: click a wired one to unplug it
            if !is_start {
                let in_c = pos2(top.x, top.y + HEAD_H / 2.0);
                let in_r = ui.interact(
                    Rect::from_center_size(in_c, Vec2::splat(PORT_HIT)),
                    egui::Id::new(("flow_in", fi, id)),
                    Sense::click(),
                );
                let wired_in = flow.nodes.iter().any(|n| n.next == Some(id));
                let hot = wiring.is_some()
                    && wiring != Some(id)
                    && pointer.is_some_and(|p| (p - in_c).length() <= DROP_R);
                painter.circle_filled(
                    in_c,
                    PORT_R + if hot { 2.0 } else { 0.0 },
                    if wired_in || hot { accent } else { pal().card },
                );
                painter.circle_stroke(in_c, PORT_R, Stroke::new(2.0, accent));
                if wired_in {
                    if in_r.hovered() {
                        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if in_r.on_hover_text("Click to unplug").clicked() {
                        ops.push(Op::Unplug(id));
                    }
                }
            }

            // the control of the block
            if !short {
                let area = Rect::from_min_max(
                    pos2(body_rect.min.x + 12.0, head.max.y + 10.0),
                    pos2(body_rect.max.x - 12.0, body_rect.max.y - 10.0),
                );
                let recording = ed
                    .recording
                    .as_ref()
                    .is_some_and(|r| r.profile == fi && r.slot == Slot::Node(id));
                ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                    match &mut flow.nodes[i].kind {
                        NodeKind::Key(sc) => {
                            if shortcut_button(ui, sc, recording, accent).clicked() {
                                ops.push(Op::Record(id));
                            }
                        }
                        NodeKind::OpenApp(s) => {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let w = (ui.available_width() - 84.0).max(60.0);
                                ui.add(
                                    text_field(s)
                                        .desired_width(w)
                                        .hint_text(platform::OS.open_app_hint()),
                                );
                                // the list of installed apps, the same one as in a slice's action
                                if button(ui, Look::Soft, Some(symbols::APP_WINDOW), "Pick\u{2026}")
                                    .on_hover_text("Choose from the installed apps")
                                    .clicked()
                                {
                                    ops.push(Op::PickApp(id));
                                }
                            });
                        }
                        NodeKind::OpenUrl(s) => {
                            ui.add(
                                text_field(s)
                                    .desired_width(ui.available_width())
                                    .hint_text("https://\u{2026}  or a file path"),
                            );
                        }
                        NodeKind::Command(s) => {
                            ui.add(
                                text_field(s)
                                    .desired_width(ui.available_width())
                                    .hint_text("shell command"),
                            );
                        }
                        NodeKind::Wait(ms) => {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::DragValue::new(ms)
                                        .range(0..=MAX_WAIT_MS)
                                        .speed(10.0)
                                        .suffix(" ms"),
                                );
                                ui.label(
                                    RichText::new("then the next step")
                                        .size(12.0)
                                        .color(pal().muted),
                                );
                            });
                        }
                        NodeKind::Start | NodeKind::End => {}
                    }
                });
            }
        }

        // a wire being dragged, and where it ends
        if let Some(from) = ed.flow_wire {
            if let (Some(p), Some(a)) = (pointer, outs.get(&from)) {
                draw_wire(&painter, *a, p, accent.gamma_multiply(0.75), 2.0);
            }
            if ctx.input(|i| i.pointer.any_down()) {
                ctx.request_repaint();
            } else {
                if let Some(p) = pointer
                    && let Some((to, _)) = ins.iter().find(|(_, c)| (*c - p).length() <= DROP_R)
                {
                    ops.push(Op::Connect(from, *to));
                }
                ed.flow_wire = None;
            }
        }

        let steps = flow.steps().len();
        screen.text(
            rect.left_bottom() + vec2(14.0, -14.0),
            Align2::LEFT_CENTER,
            format!(
                "{steps} step{} \u{b7} {}% \u{b7} Alt + scroll or pinch zooms \u{b7} middle button or two fingers moves \u{b7} use the flow in Menu > a slice > Flow",
                if steps == 1 { "" } else { "s" },
                (ed.flow_zoom * 100.0).round()
            ),
            FontId::proportional(12.0),
            pal().muted,
        );
    });

    for op in ops {
        match op {
            Op::Connect(from, to) => {
                flow.connect(from, to);
            }
            Op::Unplug(to) => flow.unplug_into(to),
            Op::Remove(id) => {
                if flow.remove_node(id) {
                    if ed.flow_sel == Some(id) {
                        ed.flow_sel = None;
                    }
                    ed.stop_recording();
                }
            }
            Op::PickApp(id) => {
                ed.flow_target = Some((fi, id));
                open_picker(ctx, ed, tx, PickFor::Flow);
            }
            Op::Record(id) => {
                let on = ed
                    .recording
                    .as_ref()
                    .is_some_and(|r| r.profile == fi && r.slot == Slot::Node(id));
                if on {
                    ed.stop_recording();
                } else {
                    SWALLOW_KEY.store(-1, Ordering::SeqCst);
                    RECORDING.store(true, Ordering::SeqCst);
                    ed.recording = Some(Rec {
                        profile: fi,
                        path: Vec::new(),
                        slot: Slot::Node(id),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::{CARD_SIZE, WIN_MARGIN};
    use crate::ui::settings::Tab;

    #[test]
    fn a_wire_runs_from_one_dot_to_the_other() {
        let (a, b) = (pos2(10.0, 20.0), pos2(300.0, 90.0));
        let pts = wire_points(a, b);
        assert_eq!(pts.first(), Some(&a));
        let last = pts.last().unwrap();
        assert!((last.x - b.x).abs() < 1e-3 && (last.y - b.y).abs() < 1e-3);
        // a wire that goes backwards still leaves to the right and arrives from the left
        let back = wire_points(pos2(300.0, 20.0), pos2(10.0, 20.0));
        assert!(back[1].x > 300.0 - 1.0);
    }

    #[test]
    fn only_start_is_short_and_every_kind_has_a_title() {
        assert_eq!(node_h(&NodeKind::Start), HEAD_H);
        assert_eq!(node_h(&NodeKind::End), HEAD_H);
        assert_eq!(node_h(&NodeKind::Wait(1)), NODE_H);
        for k in [
            NodeKind::Start,
            NodeKind::End,
            NodeKind::Key(Shortcut::new(key::F13, 0)),
            NodeKind::OpenApp(String::new()),
            NodeKind::OpenUrl(String::new()),
            NodeKind::Command(String::new()),
            NodeKind::Wait(0),
        ] {
            assert!(!kind_info(&k).1.is_empty());
        }
    }

    #[test]
    fn the_canvas_draws_a_flow_with_every_kind_of_block_without_changing_it() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        app.settings_open = true;
        let mut flow = Flow::new("flow-1", "Demo");
        let mut last = None;
        for kind in [
            NodeKind::Key(Shortcut::new(key::F13, 0)),
            NodeKind::OpenApp("Safari".into()),
            NodeKind::OpenUrl("https://example.com".into()),
            NodeKind::Command("true".into()),
            NodeKind::Wait(250),
        ] {
            last = Some(flow.add_after(last, kind));
        }
        app.edit = Config {
            flows: vec![flow],
            ..Config::default()
        };
        let before = app.edit.clone();
        app.ed.tab = Tab::Flow;
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    vec2(
                        CARD_SIZE[0] + 2.0 * WIN_MARGIN,
                        CARD_SIZE[1] + 2.0 * WIN_MARGIN,
                    ),
                )),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| app.settings_ui(ui))
                .drop_without_applying_deltas();
        }
        assert_eq!(app.edit, before, "drawing alone never edits the flow");
    }
}
