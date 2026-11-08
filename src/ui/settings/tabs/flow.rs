//! Flow tab: a node editor for chains of steps, like the graphs of Unity or Blender.
//!
//! ```text
//! [ Flow  v ] [ name ]  [+] [copy] [trash] [export] [import]
//!                                                       [+ Add block v] [reset view]
//! +--------------------------------------------------------------------------------------+
//! |  ( Start )o--------o( Shortcut        1 x)o--------o( Wait         2 x)o-----o( End )  |
//! |                     [ Cmd + C           ]            [ 500 ms           ]               |
//! +--------------------------------------------------------------------------------------+
//! ```
//!
//! Click a block and press Delete or Backspace to remove it (not while typing in a field).
//!
//! *Add block* opens a list of every kind of block in groups (Steps, Variables and logic, Ask and
//! system); the right button on the empty canvas opens the same list at the pointer.
//!
//! Every block has a dot on the right for each output (one; two for *If*, *Repeat* and *While*:
//! `then` / `else`, `body` / `done`; none for *End* and *Stop*) and, except *Start*, one on the left
//! (its input). Drag from an output dot to an input dot to wire two blocks; any number of wires can
//! go into one input, so two paths can join, and a wire can lead back to an earlier block to make a
//! loop. Click a wired output dot to unplug that wire, a wired input dot to unplug every wire into
//! it; drag the header of a block to move it. The canvas moves with a drag on its
//! background, the middle button or the scroll of a trackpad or wheel, and zooms with Alt + scroll
//! or a pinch ([`view_input`]). The number in each header is the order a walk from *Start* finds
//! the blocks in; a block no wire leads to is marked *not wired* and never runs. The data model,
//! the rules that keep a flow valid and the share file are in [`crate::config::flow`], the
//! interpreter in [`crate::flowrun`]; this file only draws and edits.
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
/// How much taller a block gets for each control row after the first (a control is 32 high, the gap 8).
const ROW_STEP: f32 = 40.0;
/// Width of the pill that picks an operation in a row of three controls.
const OP_W: f32 = 96.0;

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
    Connect(u32, Port, u32),
    Unplug(u32),
    UnplugOut(u32, Port),
    Remove(u32),
    Record(u32),
    PickApp(u32),
    /// Add a block with no wires at this place of the canvas
    AddAt(NodeKind, Pos2),
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
            toolbar(ui, edit, ed, accent)
        };
        if let Some(todo) = todo {
            run(todo, ctx, edit, ed, tx_ref);
        }
        if edit.flows.is_empty() {
            return;
        }
        ui.add_space(GAP);
        let fi = ed.flow;
        // the other flows a *Call* block can pick
        let others: Vec<(String, String)> = edit
            .flows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != fi)
            .map(|(_, f)| (f.id.clone(), f.name.clone()))
            .collect();
        canvas(ui, ctx, &mut edit.flows[fi], ed, accent, &others, tx_ref);
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
            "A flow is a small program made of blocks and wires: shortcuts, apps, links, \
             commands and waits, and also variables, conditions, loops, questions to the person \
             and things the system can do (clipboard, notifications, files). Build it here, then \
             pick it as the action of a slice in the Menu tab. Flows can be exported and \
             imported, so you can share them.",
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
fn toolbar(ui: &mut egui::Ui, edit: &mut Config, ed: &Ed, accent: Color32) -> Option<Do> {
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
        ui.add_space(8.0);
        // one button for every kind of block, in a list with headings (the canvas has the same
        // list under the right button)
        let add = button_if(
            ui,
            can_add,
            Look::Primary(accent),
            Some(symbols::PLUS),
            "Add block",
        );
        if can_add && let Some(kind) = add_popup(&add) {
            todo = Some(Do::Add(kind));
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

/// Every block the person can add, in groups, each with a line saying what it is for. The icon
/// and the name come from [`kind_info`]. The toolbar button and the right-click menu of the canvas
/// both list this.
fn catalog() -> Vec<(&'static str, Vec<(&'static str, NodeKind)>)> {
    let var = |name: &str| name.to_string();
    vec![
        (
            "Steps",
            vec![
                (
                    "Press a keyboard shortcut",
                    NodeKind::Key(Shortcut::new(key::F13, 0)),
                ),
                ("Open or raise an app", NodeKind::OpenApp(String::new())),
                (
                    "Open a web page or a file",
                    NodeKind::OpenUrl(String::new()),
                ),
                (
                    "Start a shell command, do not wait for it",
                    NodeKind::Command(String::new()),
                ),
                ("Pause before the next step", NodeKind::Wait(500)),
            ],
        ),
        (
            "Variables and logic",
            vec![
                (
                    "Put a text in a variable",
                    NodeKind::Set {
                        name: var("x"),
                        value: var("1"),
                    },
                ),
                (
                    "Add, subtract, multiply, divide...",
                    NodeKind::Calc {
                        out: var("x"),
                        a: var("x"),
                        op: CalcOp::Add,
                        b: var("1"),
                    },
                ),
                (
                    "AND, OR, XOR, NAND, NOR, NOT",
                    NodeKind::Logic {
                        out: var("x"),
                        a: String::new(),
                        op: LogicOp::And,
                        b: String::new(),
                    },
                ),
                (
                    "Two ways out: then / else",
                    NodeKind::Branch(Cond {
                        a: var("x"),
                        op: CmpOp::Eq,
                        b: var("1"),
                    }),
                ),
                ("Run a part a number of times", NodeKind::Repeat(3)),
                (
                    "Run a part while a condition holds",
                    NodeKind::While(Cond {
                        a: var("x"),
                        op: CmpOp::Lt,
                        b: var("3"),
                    }),
                ),
                (
                    "Run another flow, then go on",
                    NodeKind::Call(String::new()),
                ),
                ("End the whole run here", NodeKind::Stop),
            ],
        ),
        (
            "Ask and system",
            vec![
                (
                    "Ask the person for a text or yes / no",
                    NodeKind::Ask {
                        mode: AskMode::Text,
                        message: var("Value?"),
                        preset: String::new(),
                        out: var("answer"),
                    },
                ),
                (
                    "Run a command and wait for its result",
                    NodeKind::Run {
                        command: String::new(),
                        out: var("output"),
                        timeout_s: 10,
                    },
                ),
                (
                    "Clipboard, notification, files, apps...",
                    NodeKind::Native {
                        op: NativeOp::ClipboardGet,
                        a: String::new(),
                        b: var("clip"),
                    },
                ),
            ],
        ),
    ]
}

/// How wide and how tall (at most) the list of blocks is.
const MENU_W: f32 = 270.0;
const MENU_H: f32 = 400.0;

/// The list of blocks to add, with a heading per group. Returns the one that was clicked.
fn add_menu_items(ui: &mut egui::Ui) -> Option<NodeKind> {
    let mut picked = None;
    ui.set_width(MENU_W);
    ui.spacing_mut().item_spacing.y = 2.0;
    egui::ScrollArea::vertical()
        .max_height(MENU_H)
        .show(ui, |ui| {
            for (group, items) in catalog() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(group.to_uppercase())
                        .size(11.0)
                        .color(pal().muted),
                );
                for (hint, kind) in items {
                    let (icon, title) = kind_info(&kind);
                    if select_item(ui, Some(icon), title, false)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        picked = Some(kind);
                    }
                }
            }
        });
    picked
}

/// The list of blocks as a popup under `trigger` (a click on it opens and closes it).
fn add_popup(trigger: &egui::Response) -> Option<NodeKind> {
    let mut picked = None;
    egui::Popup::from_toggle_button_response(trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .frame(popup_frame().inner_margin(egui::Margin::same(6)))
        .show(|ui| picked = add_menu_items(ui));
    picked
}

/// The list of blocks as the menu of the right button on `area`, at the pointer.
fn add_context_menu(area: &egui::Response) -> Option<NodeKind> {
    let mut picked = None;
    egui::Popup::context_menu(area)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .frame(popup_frame().inner_margin(egui::Margin::same(6)))
        .show(|ui| picked = add_menu_items(ui));
    picked
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
                // nothing can follow a block without an output: the new one goes before End instead
                let sel =
                    sel.filter(|id| flow.node(*id).is_some_and(|n| !n.kind.outputs().is_empty()));
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
        NodeKind::Set { .. } => (symbols::CLIPBOARD_TEXT, "Set variable"),
        NodeKind::Calc { .. } => (symbols::GEAR_SIX, "Calculate"),
        NodeKind::Logic { .. } => (symbols::CODE, "Logic gate"),
        NodeKind::Branch(_) => (symbols::QUESTION, "If / else"),
        NodeKind::Repeat(_) => (symbols::ARROWS_CLOCKWISE, "Repeat"),
        NodeKind::While(_) => (symbols::ARROWS_CLOCKWISE, "While"),
        NodeKind::Ask { .. } => (symbols::BELL, "Ask"),
        NodeKind::Run { .. } => (symbols::TERMINAL_WINDOW, "Run and wait"),
        NodeKind::Native { .. } => (symbols::CPU, "System"),
        NodeKind::Call(_) => (symbols::FILE_TEXT, "Call flow"),
        NodeKind::Stop => (symbols::STOP, "Stop"),
    }
}

/// The color of the header of a block: one that runs a command or writes a file stands out in red.
fn tint(kind: &NodeKind, accent: Color32) -> Color32 {
    if kind.is_sensitive() {
        pal().bad
    } else if matches!(kind, NodeKind::Wait(_)) {
        pal().muted
    } else {
        accent
    }
}

/// How many rows of controls a block has.
fn rows(kind: &NodeKind) -> usize {
    match kind {
        NodeKind::Start | NodeKind::End | NodeKind::Stop => 0,
        NodeKind::Set { .. } | NodeKind::Calc { .. } | NodeKind::Logic { .. } => 2,
        NodeKind::Run { .. } => 2,
        NodeKind::Ask { mode, .. } => {
            if matches!(mode, AskMode::Text) {
                3
            } else {
                2
            }
        }
        NodeKind::Native { op, .. } => {
            let (a, b) = op.hints();
            1 + usize::from(!a.is_empty()) + usize::from(!b.is_empty())
        }
        _ => 1,
    }
}

/// How tall a block is.
fn node_h(kind: &NodeKind) -> f32 {
    match rows(kind) {
        0 => HEAD_H,
        n => NODE_H + (n - 1) as f32 * ROW_STEP,
    }
}

/// Where the dot of an output sits for a block whose top left corner is `top`: the first on the
/// header, the second near the bottom edge.
fn out_pos(top: Pos2, size: Vec2, port: Port) -> Pos2 {
    let y = match port {
        Port::Next => top.y + HEAD_H / 2.0,
        Port::Alt => top.y + size.y - 24.0,
    };
    pos2(top.x + size.x, y)
}

/// A text field that fills the row.
fn field(ui: &mut egui::Ui, text: &mut String, hint: &str) {
    ui.add(
        text_field(text)
            .desired_width(ui.available_width())
            .hint_text(hint),
    );
}

/// A pill that picks one of `all`.
fn pick<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    width: f32,
    all: &[T],
    label: fn(T) -> &'static str,
    current: &mut T,
) {
    let labels: Vec<&str> = all.iter().map(|t| label(*t)).collect();
    let at = all.iter().position(|t| t == current).unwrap_or(0);
    // the list is as wide as its longest name needs, even when the pill is small
    let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let list = longest as f32 * 8.0 + 64.0;
    let shown = labels.get(at).copied().unwrap_or("");
    if let Some(i) = select_wide(ui, width, list, shown, &labels, at)
        && let Some(v) = all.get(i)
    {
        *current = *v;
    }
}

/// `a op b` in one row, as the blocks that decide have it.
fn cond_row(ui: &mut egui::Ui, c: &mut Cond) {
    ui.horizontal(|ui| {
        let w = (ui.available_width() - OP_W - 12.0) / 2.0;
        let unary = c.op.is_unary();
        ui.add(
            text_field(&mut c.a)
                .desired_width(if unary { w * 2.0 + 6.0 } else { w })
                .hint_text("value"),
        );
        pick(ui, OP_W, &CmpOp::ALL, CmpOp::label, &mut c.op);
        if !c.op.is_unary() {
            ui.add(text_field(&mut c.b).desired_width(w).hint_text("value"));
        }
    });
}

/// `a op b` in one row, as the blocks that calculate have it.
fn operands_row<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    a: &mut String,
    all: &[T],
    label: fn(T) -> &'static str,
    op: &mut T,
    unary: bool,
    b: &mut String,
) {
    ui.horizontal(|ui| {
        let w = (ui.available_width() - OP_W - 12.0) / 2.0;
        ui.add(
            text_field(a)
                .desired_width(if unary { w * 2.0 + 6.0 } else { w })
                .hint_text("value"),
        );
        pick(ui, OP_W, all, label, op);
        if !unary {
            ui.add(text_field(b).desired_width(w).hint_text("value"));
        }
    });
}

/// The controls of one block. What a control needs from the canvas (recording a shortcut, the app
/// list) is asked for through `ops`.
fn controls(
    ui: &mut egui::Ui,
    kind: &mut NodeKind,
    others: &[(String, String)],
    recording: bool,
    accent: Color32,
    id: u32,
    ops: &mut Vec<Op>,
) {
    ui.spacing_mut().item_spacing = vec2(6.0, 8.0);
    match kind {
        NodeKind::Key(sc) => {
            if shortcut_button(ui, sc, recording, accent).clicked() {
                ops.push(Op::Record(id));
            }
        }
        NodeKind::OpenApp(s) => {
            ui.horizontal(|ui| {
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
        NodeKind::OpenUrl(s) => field(ui, s, "https://\u{2026}  or a file path"),
        NodeKind::Command(s) => field(ui, s, "shell command (not waited for)"),
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
        NodeKind::Set { name, value } => {
            field(ui, name, "variable name");
            field(ui, value, "value, e.g. {{name}}");
        }
        NodeKind::Calc { out, a, op, b } => {
            field(ui, out, "result variable");
            operands_row(ui, a, &CalcOp::ALL, CalcOp::label, op, false, b);
        }
        NodeKind::Logic { out, a, op, b } => {
            field(ui, out, "result variable (1 or 0)");
            let unary = op.is_unary();
            operands_row(ui, a, &LogicOp::ALL, LogicOp::label, op, unary, b);
        }
        NodeKind::Branch(c) | NodeKind::While(c) => cond_row(ui, c),
        NodeKind::Repeat(n) => {
            ui.horizontal(|ui| {
                ui.add(
                    egui::DragValue::new(n)
                        .range(0..=MAX_REPEAT)
                        .speed(0.2)
                        .suffix(" times"),
                );
                ui.label(
                    RichText::new("body, then done")
                        .size(12.0)
                        .color(pal().muted),
                );
            });
        }
        NodeKind::Ask {
            mode,
            message,
            preset,
            out,
        } => {
            ui.horizontal(|ui| {
                pick(ui, 112.0, &AskMode::ALL, AskMode::label, mode);
                field(ui, out, "answer variable");
            });
            field(ui, message, "question");
            if matches!(mode, AskMode::Text) {
                field(ui, preset, "starts with");
            }
        }
        NodeKind::Run {
            command,
            out,
            timeout_s,
        } => {
            field(ui, command, "shell command (waited for)");
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 82.0).max(60.0);
                ui.add(
                    text_field(out)
                        .desired_width(w)
                        .hint_text("output variable"),
                );
                ui.add(
                    egui::DragValue::new(timeout_s)
                        .range(1..=MAX_RUN_SECS)
                        .suffix(" s max"),
                );
            });
        }
        NodeKind::Native { op, a, b } => {
            let w = ui.available_width();
            pick(ui, w, &NativeOp::ALL, NativeOp::label, op);
            let (ha, hb) = op.hints();
            if !ha.is_empty() {
                field(ui, a, ha);
            }
            if !hb.is_empty() {
                field(ui, b, hb);
            }
        }
        NodeKind::Call(s) => {
            let labels: Vec<&str> = others.iter().map(|(_, n)| n.as_str()).collect();
            let at = others.iter().position(|(i, _)| i == s);
            let current = at
                .and_then(|a| labels.get(a).copied())
                .unwrap_or("choose a flow");
            let w = ui.available_width();
            if labels.is_empty() {
                ui.label(
                    RichText::new("no other flow yet")
                        .size(12.0)
                        .color(pal().muted),
                );
            } else if let Some(i) = select(ui, w, current, &labels, at.unwrap_or(0))
                && let Some((fid, _)) = others.get(i)
            {
                *s = fid.clone();
            }
        }
        NodeKind::Start | NodeKind::End | NodeKind::Stop => {}
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

    // A list that is open (the blocks to add, a pick list) takes the wheel and the trackpad: its
    // own scrolling reads them later in the frame, so the canvas has to leave them alone here,
    // or it moves under the list.
    if !over || egui::Popup::is_any_open(ctx) {
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
    accent: Color32,
    others: &[(String, String)],
    tx: &Sender<Msg>,
) {
    let fi = ed.flow;
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

        // the right button on the background: the list of blocks to add, and the new block goes
        // where the pointer was
        let menu_at = egui::Id::new(("flow_menu_at", fi));
        if bg.secondary_clicked()
            && let Some(p) = ctx.input(|i| i.pointer.interact_pos())
        {
            ctx.data_mut(|d| d.insert_temp(menu_at, from_global * p));
        }
        // Delete or Backspace removes the selected block, unless a field is being typed in, a
        // shortcut is being recorded or a list is open (Start and End stay: remove_node refuses)
        if let Some(id) = ed.flow_sel
            && !ctx.egui_wants_keyboard_input()
            && ed.recording.is_none()
            && !ed.sheet_open()
            && !egui::Popup::is_any_open(ctx)
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
            })
        {
            ops.push(Op::Remove(id));
        }
        if flow.nodes.len() < MAX_FLOW_NODES
            && let Some(kind) = add_context_menu(&bg)
        {
            let at = ctx
                .data(|d| d.get_temp::<Pos2>(menu_at))
                .unwrap_or(Pos2::ZERO);
            ops.push(Op::AddAt(kind, at - vec2(20.0, HEAD_H / 2.0)));
        }

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

        // the blocks a run can get to; the others are marked and their wires are dim
        let reach_ids: Vec<u32> = flow.reachable().iter().map(|n| n.id).collect();
        let pointer = ctx
            .input(|i| i.pointer.latest_pos())
            .map(|p| from_global * p);

        // where every dot is, before anything moves
        let mut outs: BTreeMap<(u32, Port), Pos2> = BTreeMap::new();
        let mut ins: Vec<(u32, Pos2)> = Vec::new();
        for n in &flow.nodes {
            let top = origin + vec2(n.x, n.y);
            let size = vec2(n.kind.width(), node_h(&n.kind));
            for &port in n.kind.outputs() {
                outs.insert((n.id, port), out_pos(top, size, port));
            }
            if !matches!(n.kind, NodeKind::Start) {
                ins.push((n.id, pos2(top.x, top.y + HEAD_H / 2.0)));
            }
        }

        // the wires, under the blocks
        for n in &flow.nodes {
            for &port in n.kind.outputs() {
                if let Some(t) = n.wire(port)
                    && let Some(a) = outs.get(&(n.id, port))
                    && let Some((_, b)) = ins.iter().find(|(id, _)| *id == t)
                {
                    let live = reach_ids.contains(&n.id);
                    let color = match (live, port) {
                        (false, _) => pal().muted.gamma_multiply(0.7),
                        (true, Port::Next) => accent,
                        (true, Port::Alt) => accent.gamma_multiply(0.6),
                    };
                    draw_wire(&painter, *a, *b, color, 2.5);
                }
            }
        }

        if flow.nodes.len() <= 2 {
            screen.text(
                rect.center() + vec2(0.0, 30.0),
                Align2::CENTER_CENTER,
                "Right-click here, or press Add block, to add steps.\nThen drag from the dot on the right of a block\nto the dot on the left of the next one.",
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
            // Start, End and Stop are only a header
            let short = flow.nodes[i].kind.is_marker();
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
                match reach_ids.iter().position(|c| *c == id) {
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

            // output dots: drag from one to wire; click a wired one to unplug it (End and Stop
            // have none, *If*, *Repeat* and *While* have two)
            let names = flow.nodes[i].kind.output_names();
            for (k, &port) in flow.nodes[i].kind.outputs().iter().enumerate() {
                let out_c = out_pos(top, size, port);
                let out_r = ui.interact(
                    Rect::from_center_size(out_c, Vec2::splat(PORT_HIT)),
                    egui::Id::new(("flow_out", fi, id, k)),
                    Sense::click_and_drag(),
                );
                if out_r.drag_started_by(PointerButton::Primary) {
                    ed.flow_wire = Some((id, port));
                    ed.flow_sel = Some(id);
                }
                let wired = flow.nodes[i].wire(port).is_some();
                if out_r.hovered() {
                    ctx.set_cursor_icon(if wired {
                        egui::CursorIcon::PointingHand
                    } else {
                        egui::CursorIcon::Crosshair
                    });
                }
                if wired && out_r.on_hover_text("Click to unplug this wire").clicked() {
                    ops.push(Op::UnplugOut(id, port));
                }
                let wired_out = wired || wiring == Some((id, port));
                painter.circle_filled(out_c, PORT_R, if wired_out { accent } else { pal().card });
                painter.circle_stroke(out_c, PORT_R, Stroke::new(2.0, accent));
                if !names[k].is_empty() {
                    painter.text(
                        out_c + vec2(10.0, -9.0),
                        Align2::LEFT_CENTER,
                        names[k],
                        FontId::proportional(11.0),
                        pal().muted,
                    );
                }
            }

            // input dot: click a wired one to unplug it
            if !is_start {
                let in_c = pos2(top.x, top.y + HEAD_H / 2.0);
                let in_r = ui.interact(
                    Rect::from_center_size(in_c, Vec2::splat(PORT_HIT)),
                    egui::Id::new(("flow_in", fi, id)),
                    Sense::click(),
                );
                let wired_in = flow.nodes.iter().any(|n| n.wires().contains(&Some(id)));
                let hot = wiring.is_some_and(|(w, _)| w != id)
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
                    if in_r.on_hover_text("Click to unplug every wire into it").clicked() {
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
                let kind = &mut flow.nodes[i].kind;
                ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                    controls(ui, kind, others, recording, accent, id, &mut ops);
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
                    ops.push(Op::Connect(from.0, from.1, *to));
                }
                ed.flow_wire = None;
            }
        }

        let steps = flow
            .reachable()
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::Start | NodeKind::End))
            .count();
        screen.text(
            rect.left_bottom() + vec2(14.0, -14.0),
            Align2::LEFT_CENTER,
            format!(
                "{steps} step{} \u{b7} {}% \u{b7} right-click adds a block \u{b7} Alt + scroll or pinch zooms \u{b7} middle button or two fingers moves",
                if steps == 1 { "" } else { "s" },
                (ed.flow_zoom * 100.0).round()
            ),
            FontId::proportional(12.0),
            pal().muted,
        );
    });

    for op in ops {
        match op {
            Op::Connect(from, port, to) => {
                flow.connect(from, port, to);
            }
            Op::AddAt(kind, at) => {
                if flow.nodes.len() < MAX_FLOW_NODES {
                    let id = flow.add_at(kind, at.x, at.y);
                    ed.flow_sel = Some(id);
                }
            }
            Op::Unplug(to) => flow.unplug_into(to),
            Op::UnplugOut(from, port) => flow.unplug_out(from, port),
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

    /// One block of every kind.
    fn every_kind() -> Vec<NodeKind> {
        vec![
            NodeKind::Start,
            NodeKind::End,
            NodeKind::Key(Shortcut::new(key::F13, 0)),
            NodeKind::OpenApp(String::new()),
            NodeKind::OpenUrl(String::new()),
            NodeKind::Command(String::new()),
            NodeKind::Wait(0),
            NodeKind::Set {
                name: "x".into(),
                value: "1".into(),
            },
            NodeKind::Calc {
                out: "x".into(),
                a: "x".into(),
                op: CalcOp::Add,
                b: "1".into(),
            },
            NodeKind::Logic {
                out: "x".into(),
                a: "1".into(),
                op: LogicOp::Not,
                b: String::new(),
            },
            NodeKind::Branch(Cond::default()),
            NodeKind::Repeat(2),
            NodeKind::While(Cond::default()),
            NodeKind::Ask {
                mode: AskMode::Text,
                message: "?".into(),
                preset: String::new(),
                out: "a".into(),
            },
            NodeKind::Ask {
                mode: AskMode::YesNo,
                message: "?".into(),
                preset: String::new(),
                out: "a".into(),
            },
            NodeKind::Run {
                command: "true".into(),
                out: "o".into(),
                timeout_s: 5,
            },
            NodeKind::Native {
                op: NativeOp::Notify,
                a: "t".into(),
                b: "x".into(),
            },
            NodeKind::Native {
                op: NativeOp::FrontApp,
                a: String::new(),
                b: "app".into(),
            },
            NodeKind::Call(String::new()),
            NodeKind::Stop,
        ]
    }

    #[test]
    fn the_add_menu_lists_every_block_a_person_can_add_once() {
        let mut titles = Vec::new();
        for (group, items) in catalog() {
            assert!(!group.is_empty() && !items.is_empty());
            for (hint, kind) in items {
                assert!(!hint.is_empty());
                assert!(!kind.is_marker() || matches!(kind, NodeKind::Stop));
                titles.push(kind_info(&kind).1);
            }
        }
        let mut unique = titles.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), titles.len(), "a block listed twice");
        // Start and End come with the flow; everything else is in the menu
        let others = every_kind()
            .iter()
            .filter(|k| !matches!(k, NodeKind::Start | NodeKind::End))
            .map(|k| kind_info(k).1)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(
            others.iter().all(|t| titles.contains(t)),
            "{others:?} vs {titles:?}"
        );
    }

    #[test]
    fn only_the_markers_are_short_and_every_kind_has_a_title() {
        assert_eq!(node_h(&NodeKind::Start), HEAD_H);
        assert_eq!(node_h(&NodeKind::End), HEAD_H);
        assert_eq!(node_h(&NodeKind::Stop), HEAD_H);
        assert_eq!(node_h(&NodeKind::Wait(1)), NODE_H);
        for k in every_kind() {
            assert!(!kind_info(&k).1.is_empty());
            assert_eq!(node_h(&k) == HEAD_H, k.is_marker(), "{k:?}");
        }
    }

    #[test]
    fn a_block_is_as_tall_as_its_rows_and_the_second_dot_is_below_the_first() {
        let ask = |mode| NodeKind::Ask {
            mode,
            message: String::new(),
            preset: String::new(),
            out: String::new(),
        };
        assert_eq!(rows(&ask(AskMode::Text)), 3);
        assert_eq!(rows(&ask(AskMode::YesNo)), 2);
        assert_eq!(node_h(&ask(AskMode::Text)), NODE_H + 2.0 * ROW_STEP);
        let native = |op| NodeKind::Native {
            op,
            a: String::new(),
            b: String::new(),
        };
        assert_eq!(
            rows(&native(NativeOp::FrontApp)),
            2,
            "the pill and one field"
        );
        assert_eq!(rows(&native(NativeOp::Notify)), 3);
        let top = pos2(10.0, 20.0);
        for k in every_kind() {
            let size = vec2(k.width(), node_h(&k));
            if let [first, second] = k.outputs() {
                let (a, b) = (out_pos(top, size, *first), out_pos(top, size, *second));
                assert!(b.y > a.y + 2.0 * PORT_R, "{k:?}: the dots must not overlap");
                assert!(
                    b.y < top.y + size.y,
                    "{k:?}: the second dot is inside the block"
                );
            }
        }
    }

    #[test]
    fn the_canvas_draws_a_flow_with_every_kind_of_block_without_changing_it() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        app.settings_open = true;
        let mut flow = Flow::new("flow-1", "Demo");
        let mut last = None;
        for kind in every_kind()
            .into_iter()
            .filter(|k| !matches!(k, NodeKind::Start | NodeKind::End | NodeKind::Stop))
        {
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
