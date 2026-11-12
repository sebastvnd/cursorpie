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
//! system); the right button anywhere on the canvas (on the empty background, on a block or on
//! one of its fields) opens the same list at the pointer. It opens on the press, not on the
//! release, so a slow click or a trackpad that moves a little still opens it ([`canvas_menu`]).
//! *Tidy* lines the blocks up in columns, left to right, without any of them covering another.
//!
//! A wire that cannot work is **red**, one that probably is a mistake **amber**, each with a `!`
//! in the middle whose tooltip says why, and the block it leads to has the same mark on its
//! corner ([`crate::config::flow_check`]; the rules are in
//! [`guides::flow_checks`](crate::guides::flow_checks)). A block that could not do its work in
//! the last real run of the flow is outlined in red too, with the reason in the tooltip
//! (`Ed::flow_marks`, set by the report of the run). The strip under the canvas counts both.
//!
//! Every block has a dot on the right for each output (one; two for *If*, *Repeat* and *While*:
//! `then` / `else`, `body` / `done`; none for *End* and *Stop*) and, except *Start*, one on the left
//! (its input). Drag from an output dot to an input dot to wire two blocks; any number of wires can
//! go into one input, so two paths can join, and a wire can lead back to an earlier block to make a
//! loop. Click a wired output dot to unplug that wire, a wired input dot to unplug every wire into
//! it; drag the header of a block to move it. The canvas moves with a drag on its
//! background, the middle button or the scroll of a trackpad or wheel, and zooms with Alt + scroll
//! or a pinch ([`view_input`]); the text is laid out again at every zoom so it stays sharp
//! ([`crisp_text`]). The number in each header is the order a walk from *Start* finds
//! the blocks in; a block no wire leads to is marked *not wired* and never runs. The data model,
//! the rules that keep a flow valid and the share file are in [`crate::config::flow`], the
//! interpreter in [`crate::flowrun`]; this file only draws and edits.
//!
//! A slice runs a flow through its action: Menu > a slice > **Flow**.
//!
//! **New flow** does not add a flow at once: it opens the sheet of [`flow_new`](super::flow_new),
//! which asks whether to build the flow by hand or with an AI agent (`Do::NewMenu`; `Do::New` is
//! the empty flow itself).

use super::flow_inspect::{self, READS_C, SETS_C};
use super::flow_list as list_ui;
use super::flow_new::flow_new_sheet;
use crate::config::flow::{COORD_LIMIT, export_flow_to_downloads, fresh_id, unique_name};
use crate::config::flow_check::{
    block_issues, block_reads, block_severity, block_writes, data_edges, wire_severity,
};
use crate::config::*;
#[cfg(test)]
use crate::keys::{Shortcut, key};
use crate::msg::Msg;
use crate::platform;
use crate::tap::{RECORDING, SWALLOW_KEY};
use crate::ui::app::App;
use crate::ui::draw::rgb;
use crate::ui::fonts;
use crate::ui::settings::panel_designer;
use crate::ui::settings::picker::spawn_file_picker;
use crate::ui::settings::state::{Ed, FlowMenu, FlowNew, FlowView, KitStatus, PickFor, Rec, Slot};
use crate::ui::settings::tabs::profile_toolbar::{flow_app_sheet, open_picker};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{
    self, Align2, Color32, FontId, PointerButton, Pos2, Rect, RichText, Sense, Shape, Stroke,
    StrokeKind, UiBuilder, Vec2, emath::TSTransform, layers::ShapeIdx, pos2, vec2,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
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
pub(super) const OP_W: f32 = 96.0;
/// The color of a wire or a block with a warning: probably a mistake, but it can still work. An
/// error is `pal().bad` (red). Not part of the palette: the two have to read on both appearances.
pub(super) const AMBER: Color32 = Color32::from_rgb(201, 138, 28);
/// Height of the strip under the canvas that holds the hint line, so no block ever covers it.
const STATUS_H: f32 = 30.0;
/// Space between two columns of blocks, and between two blocks of a column, after *Tidy*.
const TIDY_COL_GAP: f32 = 90.0;
const TIDY_ROW_GAP: f32 = 28.0;
/// Where the first column of blocks starts after *Tidy*.
const TIDY_ORIGIN: f32 = 40.0;

/// What the toolbar (or the empty page) asked for this frame.
pub(super) enum Do {
    /// Add an empty flow
    New,
    /// Open the sheet that asks how the new flow is to be made (by hand, or with an AI agent)
    NewMenu,
    Duplicate,
    Delete,
    Import,
    Export,
    Pick(usize),
    ResetView,
    /// Line the blocks up in columns so none covers another
    Tidy,
    Add(NodeKind),
    /// Add a step at the end of the list of steps
    AddStep(NodeKind),
    /// Show the flow as a list of steps or on the canvas
    View(FlowView),
    /// Fill the whole display with the Flow tab, or go back to the window
    ToggleFull,
    /// Open or close the inspector of the canvas (the selected block, the variables)
    ToggleInspector,
}

/// What the canvas asked for this frame. They are applied after the blocks are drawn, so the flow
/// is not changed while it is being walked.
pub(super) enum Op {
    Connect(u32, Port, u32),
    Unplug(u32),
    UnplugOut(u32, Port),
    Remove(u32),
    Record(u32),
    PickApp(u32),
    /// Open the panel designer for this *Ask* block
    DesignPanel(u32),
    /// Add a block with no wires at this place of the canvas
    AddAt(NodeKind, Pos2),
    /// Add a block at this place and wire the output (block, port) that had nothing to it
    AddFrom(NodeKind, Pos2, (u32, Port)),
}

/// How far the canvas can be zoomed out and in. The text is laid out again at the size it is shown
/// at (see [`crisp_text`]), so it stays sharp over the whole range, like a vector drawing.
const ZOOM_MIN: f32 = 0.3;
const ZOOM_MAX: f32 = 4.0;

impl App {
    /// Draws the Flow tab into `ui`: the list of flows on the left and, beside it, the selected
    /// flow as a list of steps (or, on request, as a canvas of blocks and wires).
    pub(in crate::ui::settings) fn flow_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let App { edit, ed, tx, .. } = self;
        let tx_ref: &Sender<Msg> = tx;
        let accent = rgb(edit.accent);
        ed.flow = ed.flow.min(edit.flows.len().saturating_sub(1));

        if edit.flows.is_empty() {
            if let Some(todo) = empty_page(ui, accent) {
                run(todo, ctx, edit, ed, tx_ref);
            }
            flow_new_sheet(ctx, edit, ed, tx_ref, accent);
            return;
        }
        let full = ui.available_rect_before_wrap();
        let main = full;
        let mut todo = None;
        let fi = ed.flow;
        // the other flows a *Call* block can pick
        let others: Vec<(String, String)> = edit
            .flows
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != fi)
            .map(|(_, f)| (f.id.clone(), f.name.clone()))
            .collect();
        ui.scope_builder(UiBuilder::new().max_rect(main), |ui| {
            if let Some(t) = list_ui::header(ui, edit, ed, accent) {
                todo = Some(t);
            }
            ui.add_space(GAP);
            match ed.flow_view {
                FlowView::Steps => {
                    crate::tap::FLOW_CANVAS_HOVER.store(false, Ordering::SeqCst);
                    let avail = ui.available_rect_before_wrap();
                    list_ui::steps(ui, ctx, &mut edit.flows[fi], ed, accent, &others, tx_ref);
                    // the button to add a step floats over the bottom of the page (a flow that
                    // is wired freely is not a list: there is nothing to add to)
                    if crate::config::flow_list::outline(&edit.flows[fi]).is_some()
                        && let Some(t) = list_ui::steps_bar(
                            ctx,
                            avail,
                            &edit.flows[fi],
                            &mut ed.flow_search,
                            accent,
                            ed.flow_full.is_some(),
                        )
                    {
                        todo = Some(t);
                    }
                }
                FlowView::Canvas => {
                    // blocks added from the list have no place yet: line them up once
                    if ed.flow_untidy.remove(&edit.flows[fi].id) {
                        tidy(&mut edit.flows[fi]);
                        ed.flow_zoom = 1.0;
                        ed.flow_pan = Vec2::ZERO;
                    }
                    // the canvas takes all the room; the bar with the buttons floats over its
                    // bottom, above the strip with the hint line
                    let avail = ui.available_rect_before_wrap();
                    let whole = Rect::from_min_size(
                        avail.min,
                        vec2(avail.width(), avail.height().max(220.0)),
                    );
                    // a selected block (or the button of the bar) opens the inspector at the
                    // right: the canvas keeps the rest
                    let insp_on = flow_inspect::visible(ed, &edit.flows[fi]);
                    let insp_w = flow_inspect::SIDE_W.min(whole.width() * 0.5);
                    let canvas_rect = if insp_on {
                        Rect::from_min_max(whole.min, pos2(whole.max.x - insp_w - GAP, whole.max.y))
                    } else {
                        whole
                    };
                    ui.scope_builder(UiBuilder::new().max_rect(canvas_rect), |ui| {
                        canvas(ui, ctx, &mut edit.flows[fi], ed, accent, &others, tx_ref);
                    });
                    if insp_on {
                        let insp_rect =
                            Rect::from_min_max(pos2(whole.max.x - insp_w, whole.min.y), whole.max);
                        flow_inspect::panel(
                            ui,
                            ctx,
                            insp_rect,
                            &mut edit.flows[fi],
                            ed,
                            accent,
                            &others,
                            tx_ref,
                        );
                    }
                    // the bar stays where it was, in the middle of the whole width, whether or not
                    // the inspector is open (it never moves under the pointer)
                    let view =
                        Rect::from_min_max(whole.min, pos2(whole.max.x, whole.max.y - STATUS_H));
                    if let Some(t) = list_ui::canvas_bar(
                        ctx,
                        view,
                        &edit.flows[fi],
                        &mut ed.flow_search,
                        accent,
                        ed.flow_full.is_some(),
                        insp_on,
                    ) {
                        todo = Some(t);
                    }
                }
            }
        });
        // the list of flows slides over the page from the left edge; drawn last, so it is on top
        if let Some(t) = list_ui::sidebar(ctx, full, edit, ed, accent) {
            todo = Some(t);
        }
        if let Some(todo) = todo {
            run(todo, ctx, edit, ed, tx_ref);
        }
        flow_app_sheet(ctx, edit, ed, tx_ref, accent);
        flow_new_sheet(ctx, edit, ed, tx_ref, accent);
        panel_designer::panel_design_sheet(ctx, edit, ed, accent);
    }
}

/// A short message in the status bar (works from anywhere that has the channel).
pub(super) fn notice(tx: &Sender<Msg>, ctx: &egui::Context, text: impl Into<String>) {
    let _ = tx.send(Msg::Notice(text.into()));
    ctx.request_repaint();
}

/// Makes flow `i` the one on the canvas, with a fresh view.
pub(super) fn select_flow(ed: &mut Ed, i: usize) {
    ed.flow = i;
    ed.flow_sel = None;
    ed.flow_var = None;
    ed.flow_focus = None;
    ed.flow_wire = None;
    ed.flow_pan = Vec2::ZERO;
    ed.flow_zoom = 1.0;
    ed.stop_recording();
}

/// What there is to say when no flow exists yet.
pub(super) fn empty_page(ui: &mut egui::Ui, accent: Color32) -> Option<Do> {
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
                todo = Some(Do::NewMenu);
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

/// Every block the person can add, in groups, each with a line saying what it is for. The icon
/// and the name come from [`kind_info`]. The toolbar button and the right-click menu of the canvas
/// both list this.
#[cfg(test)]
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
                    "Make, change and read a list (array)",
                    NodeKind::List {
                        op: ListOp::Split,
                        list: var("items"),
                        a: String::new(),
                        b: String::new(),
                        out: String::new(),
                    },
                ),
                (
                    "Go through every item of a list",
                    NodeKind::ForEach {
                        list: var("items"),
                        item: var("item"),
                    },
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

/// A block picked from the list of the canvas: what, where on the canvas, and the output it is
/// wired from (`None`: a block that stands alone).
type Picked = (NodeKind, Pos2, Option<(u32, Port)>);

/// The list of blocks that the right button opens on the canvas, at the pointer.
///
/// It is not an `egui::Popup::context_menu` of the background, because that one needs a *click*
/// on the background itself: the button let go within a short time and a few points from where
/// it went down, and with nothing else under the pointer. So a slow press, a trackpad that moves
/// a little, a press on a block or on one of its fields did nothing, and a press while another
/// list was open only closed that list. Here the press itself opens the list, wherever on the
/// canvas it lands; a press outside the list, or Esc (see `window.rs`), closes it.
///
/// Returns the block that was picked and the place of the canvas where it goes.
fn canvas_menu(
    ctx: &egui::Context,
    ed: &mut Ed,
    view: Rect,
    from_global: TSTransform,
    can_add: bool,
) -> Option<Picked> {
    let (secondary, press, pos, ctrl_click) = ctx.input(|i| {
        (
            i.pointer.button_pressed(PointerButton::Secondary),
            i.pointer.any_pressed(),
            i.pointer.interact_pos(),
            // on a Mac, Control + click is the right button
            i.pointer.button_pressed(PointerButton::Primary)
                && i.modifiers.ctrl
                && cfg!(target_os = "macos"),
        )
    });

    // a press outside the list closes it (and goes on to whatever is under it)
    if press
        && let Some(p) = pos
        && let Some(m) = ed.flow_menu
        && !m.rect.contains(p)
    {
        ed.flow_menu = None;
        ctx.request_repaint();
    }

    // a right press on the canvas opens it there (again, if it was open somewhere else)
    if (secondary || ctrl_click)
        && let Some(p) = pos
        && can_add
        && view.contains(p)
        && !ed.sheet_open()
        && ed.recording.is_none()
        && !egui::Popup::is_any_open(ctx)
        && !ed.flow_menu.is_some_and(|m| m.rect.contains(p))
    {
        ed.flow_menu = Some(FlowMenu {
            at: p,
            scene: from_global * p,
            rect: Rect::NOTHING,
            from: None,
        });
        ed.flow_search.clear();
        ctx.request_repaint();
    }

    let m = ed.flow_menu?;
    if !can_add || ed.sheet_open() {
        ed.flow_menu = None;
        return None;
    }

    // keep the list inside the window: it is as wide as MENU_W and at most MENU_H tall, plus the
    // margins of its frame
    let mut at = m.at;
    let room = ed.card;
    let (w, h) = (list_ui::MENU_W + 16.0, list_ui::MENU_H + 20.0);
    if room.width() > 0.0 && room.height() > 0.0 {
        at.x = at.x.min(room.max.x - w - 8.0).max(room.min.x + 8.0);
        at.y = at.y.min(room.max.y - h - 8.0).max(room.min.y + 8.0);
    }
    let mut search = std::mem::take(&mut ed.flow_search);
    let out = egui::Area::new(egui::Id::new("flow_canvas_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .constrain(false)
        .show(ctx, |ui| {
            popup_frame()
                .inner_margin(egui::Margin::same(6))
                .show(ui, |ui| list_ui::add_menu(ui, &mut search, true))
                .inner
        });
    ed.flow_search = search;
    if let Some(open) = ed.flow_menu.as_mut() {
        open.rect = out.response.rect;
    }
    if let Some(kind) = out.inner {
        ed.flow_menu = None;
        ctx.request_repaint();
        return Some((kind, m.scene, m.from));
    }
    None
}

/// Carries out what the toolbar asked for.
pub(super) fn run(todo: Do, ctx: &egui::Context, edit: &mut Config, ed: &mut Ed, tx: &Sender<Msg>) {
    match todo {
        Do::NewMenu => {
            if edit.flows.len() >= MAX_FLOWS {
                notice(
                    tx,
                    ctx,
                    format!("There are already {MAX_FLOWS} flows. Delete one first."),
                );
                return;
            }
            ed.flow_new = Some(FlowNew::Choose);
            ed.flow_kit = KitStatus::Idle;
        }
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
        Do::ToggleFull => ed.flow_full_req = true,
        Do::ToggleInspector => {
            if ed.flow_insp || ed.flow_sel.is_some() {
                // closing: nothing selected and nothing followed
                ed.flow_insp = false;
                ed.flow_sel = None;
                ed.flow_var = None;
            } else {
                ed.flow_insp = true;
                ed.flow_insp_vars = true;
            }
        }
        Do::Tidy => {
            if let Some(flow) = edit.flows.get_mut(ed.flow) {
                tidy(flow);
            }
            ed.flow_menu = None;
            ed.flow_zoom = 1.0;
            ed.flow_pan = Vec2::ZERO;
        }
        Do::View(view) => {
            ed.flow_view = view;
            ed.flow_menu = None;
            ed.flow_wire = None;
            ed.stop_recording();
        }
        Do::AddStep(kind) => {
            if let Some(flow) = edit.flows.get_mut(ed.flow) {
                let at = crate::config::flow_list::Spot::END;
                if crate::config::flow_list::insert_block(flow, at, kind).is_some() {
                    ed.flow_untidy.insert(flow.id.clone());
                } else if flow.nodes.len() >= MAX_FLOW_NODES {
                    notice(
                        tx,
                        ctx,
                        format!("A flow can have {MAX_FLOW_NODES} blocks at most."),
                    );
                }
            }
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
pub(super) fn kind_info(kind: &NodeKind) -> (char, &'static str) {
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
        NodeKind::List { .. } => (symbols::STACK, "List"),
        NodeKind::ForEach { .. } => (symbols::ARROWS_CLOCKWISE, "For each item"),
        NodeKind::Call(_) => (symbols::FILE_TEXT, "Call flow"),
        NodeKind::Stop => (symbols::STOP, "Stop"),
    }
}

/// The color of the header of a block: one that runs a command or writes a file stands out in red.
pub(super) fn tint(kind: &NodeKind, accent: Color32) -> Color32 {
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
        NodeKind::List { op, .. } => {
            let (a, b, out) = op.hints();
            1 + usize::from(!a.is_empty())
                + usize::from(!b.is_empty())
                + usize::from(!out.is_empty())
        }
        NodeKind::ForEach { .. } => 2,
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

/// Lines the blocks of `flow` up, left to right: one column per step of the walk from *Start*
/// (a wire that leads back to an earlier block, as in a loop, does not move that block), *End* in
/// the last column of the walk, the blocks no wire reaches in a column of their own after it. In
/// a column the blocks sit one under another, in the order the walk found them, with a gap, so
/// none covers another and the same flow always looks the same.
pub(super) fn tidy(flow: &mut Flow) {
    let mut depth: BTreeMap<u32, usize> = BTreeMap::new();
    let mut order: Vec<u32> = Vec::new();
    let mut queue: std::collections::VecDeque<u32> = std::collections::VecDeque::new();
    if let Some(s) = flow.start() {
        depth.insert(s.id, 0);
        queue.push_back(s.id);
    }
    while let Some(id) = queue.pop_front() {
        order.push(id);
        let d = depth.get(&id).copied().unwrap_or(0);
        if let Some(n) = flow.node(id) {
            for t in n.wires().into_iter().flatten() {
                if flow.node(t).is_some() && !depth.contains_key(&t) {
                    depth.insert(t, d + 1);
                    queue.push_back(t);
                }
            }
        }
    }
    // everything leads to End: it goes after the deepest of the others
    let end_id = flow.end().map(|n| n.id);
    let body_last = depth
        .iter()
        .filter(|(id, _)| Some(**id) != end_id)
        .map(|(_, d)| *d)
        .max()
        .unwrap_or(0);
    if let Some(e) = end_id
        && depth.contains_key(&e)
    {
        depth.insert(e, body_last + 1);
    }
    // the blocks no wire reaches
    let last = depth.values().copied().max().unwrap_or(0);
    for n in &flow.nodes {
        if let std::collections::btree_map::Entry::Vacant(e) = depth.entry(n.id) {
            e.insert(last + 1);
            order.push(n.id);
        }
    }

    let count = depth.values().copied().max().map_or(0, |d| d + 1);
    let mut columns: Vec<Vec<u32>> = vec![Vec::new(); count];
    for id in &order {
        if let Some(d) = depth.get(id) {
            columns[*d].push(*id);
        }
    }
    let mut spot: BTreeMap<u32, (f32, f32)> = BTreeMap::new();
    let mut x = TIDY_ORIGIN;
    for col in &columns {
        let mut y = TIDY_ORIGIN;
        let mut wide: f32 = 0.0;
        for id in col {
            let Some(n) = flow.node(*id) else { continue };
            spot.insert(*id, (x, y));
            y += node_h(&n.kind) + TIDY_ROW_GAP;
            wide = wide.max(n.kind.width());
        }
        x += wide + TIDY_COL_GAP;
    }
    for n in &mut flow.nodes {
        if let Some((px, py)) = spot.get(&n.id) {
            n.x = *px;
            n.y = *py;
        }
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

/// The text of a one-line field as the field can draw it: a line break or a tab (a message
/// written with several lines, a flow that was imported) is a character no font has a picture
/// for, so it showed as an empty box. A pilcrow and an arrow stand for them instead.
fn show_breaks(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '\r')
        .map(|c| match c {
            '\n' => '\u{b6}',
            '\t' => '\u{2192}',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect()
}

/// The reverse of [`show_breaks`], for what the person edited.
fn hide_breaks(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{b6}' => '\n',
            '\u{2192}' => '\t',
            c => c,
        })
        .collect()
}

/// A text field that fills the row.
pub(super) fn field(ui: &mut egui::Ui, text: &mut String, hint: &str) {
    let width = ui.available_width();
    if text.chars().any(char::is_control) {
        // edit a copy that can be drawn; the text only changes when the person types
        let mut shown = show_breaks(text);
        if ui
            .add(text_field(&mut shown).desired_width(width).hint_text(hint))
            .changed()
        {
            *text = hide_breaks(&shown);
        }
    } else {
        ui.add(text_field(text).desired_width(width).hint_text(hint));
    }
}

/// A pill that picks one of `all`.
pub(super) fn pick<T: Copy + PartialEq>(
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
pub(super) fn cond_row(ui: &mut egui::Ui, c: &mut Cond) {
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
pub(super) fn operands_row<T: Copy + PartialEq>(
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
pub(super) fn controls(
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
                ui.add(
                    egui::Label::new(
                        RichText::new("then the next step")
                            .size(12.0)
                            .color(pal().muted),
                    )
                    .truncate(),
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
                ui.add(
                    egui::Label::new(
                        RichText::new("body, then done")
                            .size(12.0)
                            .color(pal().muted),
                    )
                    .truncate(),
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
            field(
                ui,
                message,
                if matches!(mode, AskMode::Panel) {
                    "panel (JSON, see docs/PANELS.md)"
                } else {
                    "question"
                },
            );
            if matches!(mode, AskMode::Panel)
                && button(
                    ui,
                    Look::Soft,
                    Some(symbols::PENCIL_SIMPLE),
                    "Design panel\u{2026}",
                )
                .on_hover_text("Build the panel with the mouse and see it as it will look")
                .clicked()
            {
                ops.push(Op::DesignPanel(id));
            }
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
        NodeKind::List {
            op,
            list,
            a,
            b,
            out,
        } => {
            ui.horizontal(|ui| {
                pick(ui, 136.0, &ListOp::ALL, ListOp::label, op);
                field(ui, list, "list variable");
            });
            let (ha, hb, ho) = op.hints();
            if !ha.is_empty() {
                field(ui, a, ha);
            }
            if !hb.is_empty() {
                field(ui, b, hb);
            }
            if !ho.is_empty() {
                field(ui, out, ho);
            }
        }
        NodeKind::ForEach { list, item } => {
            field(ui, list, "list variable");
            field(ui, item, "variable for each item");
        }
        NodeKind::Call(s) => {
            let labels: Vec<&str> = others.iter().map(|(_, n)| n.as_str()).collect();
            let at = others.iter().position(|(i, _)| i == s);
            let current = at
                .and_then(|a| labels.get(a).copied())
                .unwrap_or("choose a flow");
            let w = ui.available_width();
            if labels.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new("no other flow yet")
                            .size(12.0)
                            .color(pal().muted),
                    )
                    .truncate(),
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

/// Whether `shape` is, or holds, a text.
fn holds_text(shape: &Shape) -> bool {
    match shape {
        Shape::Text(_) => true,
        Shape::Vec(list) => list.iter().any(holds_text),
        _ => false,
    }
}

/// How many text sizes the canvas cuts per doubling of the zoom (see [`crisp_scale`]).
const CRISP_STEPS_PER_OCTAVE: f32 = 4.0;

/// Set once a text was laid out at a size other than its own, so the glyphs of those sizes are in
/// the font atlas. [`take_crisp_used`] reads and clears it when Settings closes.
static CRISP_USED: AtomicBool = AtomicBool::new(false);

/// Whether the canvas cut glyphs at zoomed sizes since the last call (and forgets it). egui never
/// evicts glyphs from its font atlas, so the caller rebuilds the atlas when this is true.
pub(crate) fn take_crisp_used() -> bool {
    CRISP_USED.swap(false, Ordering::Relaxed)
}

/// The size factor the text of a zoomed layer is cut at: `zoom` snapped to a ladder of
/// [`CRISP_STEPS_PER_OCTAVE`] steps per doubling (1.0, 1.19, 1.41, ...).
///
/// The factor only picks the resolution of the glyphs; the layer still scales them by the real
/// `zoom`, so the text keeps its exact size on screen and is at most about 9% off one texel to
/// one pixel. Using `zoom` itself cut a new set of glyphs for every value a pinch or a wheel
/// passed through, and the font atlas keeps every glyph it ever cut: memory grew by tens of MB per
/// zoom gesture. The ladder bounds it to a few sizes (16 over the whole range 0.3 to 4).
pub(in crate::ui::settings) fn crisp_scale(zoom: f32) -> f32 {
    if !zoom.is_finite() || zoom <= 0.0 {
        return 1.0;
    }
    let step = (zoom.log2() * CRISP_STEPS_PER_OCTAVE).round();
    (step / CRISP_STEPS_PER_OCTAVE).exp2()
}

/// Lays one text out again at `zoom` times its size, and shrinks the result by the same factor.
///
/// The layer of the blocks is scaled by `zoom` when the frame ends, so the text comes out at
/// its normal size on screen, but its glyphs were cut for `zoom` times that size and are mapped
/// one texel to one pixel: sharp, not a small picture stretched.
fn crisp_text_shape(painter: &egui::Painter, text: &mut egui::epaint::TextShape, zoom: f32) {
    // a turned text keeps what it has
    if text.angle != 0.0 {
        return;
    }
    let mut job = (*text.galley.job).clone();
    for section in &mut job.sections {
        section.leading_space *= zoom;
        section.format.font_id.size *= zoom;
        section.format.extra_letter_spacing *= zoom;
        if let Some(h) = section.format.line_height.as_mut() {
            *h *= zoom;
        }
        section.format.underline.width *= zoom;
        section.format.strikethrough.width *= zoom;
    }
    // an infinite width stays infinite
    job.wrap.max_width *= zoom;
    job.first_row_min_height *= zoom;
    text.galley = painter.layout_job(job);
    text.pos = pos2(text.pos.x * zoom, text.pos.y * zoom);
    text.underline.width *= zoom;
    text.transform(TSTransform::from_scaling(1.0 / zoom));
}

fn crisp_shape(painter: &egui::Painter, shape: &mut Shape, zoom: f32) {
    match shape {
        Shape::Text(text) => crisp_text_shape(painter, text, zoom),
        Shape::Vec(list) => {
            for inner in list {
                crisp_shape(painter, inner, zoom);
            }
        }
        _ => {}
    }
}

/// Keeps the text of the canvas sharp when it is zoomed.
///
/// The blocks and wires live in a layer that egui scales as a whole (`set_transform_layer`).
/// Lines, circles and rectangles are cut into triangles *after* that, so they are sharp at any
/// zoom, but a text is a set of pictures of glyphs cut for its own size: scaled with the layer
/// it turns blurry when zoomed in and rough when zoomed out. So, once the blocks are drawn and
/// before the frame is cut into triangles, every text of the layer is laid out again at `zoom`
/// times its size (see [`crisp_text_shape`]); the layer then scales it back to the size the
/// block has. This covers every widget in a block (fields, pills, buttons, icons) without
/// changing them. Nothing is laid out again at 100%, where the text already is what it should be.
pub(in crate::ui::settings) fn crisp_text(
    ctx: &egui::Context,
    painter: &egui::Painter,
    layer: egui::LayerId,
    zoom: f32,
) {
    // cut the glyphs at a size from a short ladder, not at every zoom the user passes through
    let zoom = crisp_scale(zoom);
    if (zoom - 1.0).abs() < 0.001 {
        return;
    }
    // Copy the shapes that hold text out of the layer. `graphics_mut` keeps the context locked,
    // and laying text out needs the fonts through that same context, so it cannot be done inside.
    let mut found: Vec<(usize, Shape)> = Vec::new();
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        for i in 0..list.next_idx().0 {
            list.mutate_shape(ShapeIdx(i), |clipped| {
                if holds_text(&clipped.shape) {
                    found.push((i, clipped.shape.clone()));
                }
            });
        }
    });
    if found.is_empty() {
        return;
    }
    CRISP_USED.store(true, Ordering::Relaxed);
    for (_, shape) in &mut found {
        crisp_shape(painter, shape, zoom);
    }
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        for (i, shape) in found {
            list.mutate_shape(ShapeIdx(i), |clipped| clipped.shape = shape);
        }
    });
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
    if !over || egui::Popup::is_any_open(ctx) || ed.flow_menu.is_some() {
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
    // what cannot work in this flow, found without running it, and what could not work in its
    // last run
    let issues = {
        let known: Vec<&str> = others
            .iter()
            .map(|(id, _)| id.as_str())
            .chain(std::iter::once(flow.id.as_str()))
            .collect();
        check_flow(flow, &known)
    };
    let marks: Vec<(u32, String)> = ed.flow_marks.get(&flow.id).cloned().unwrap_or_default();
    let avail = ui.available_rect_before_wrap();
    let rect = Rect::from_min_size(avail.min, vec2(avail.width(), avail.height().max(220.0)));
    // the blocks live above this; under it is a strip for the hint line, which no block covers
    let view = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - STATUS_H));
    let mut ops: Vec<Op> = Vec::new();

    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(rect));
        // the background takes the dragging; the blocks live in a layer above it and win
        let (_, bg) = ui.allocate_exact_size(view.size(), Sense::click_and_drag());
        // the count of problems at the right of the strip: its tooltip lists them
        let tally_area = Rect::from_min_max(pos2(rect.max.x - 260.0, view.max.y), rect.max);
        if !issues.is_empty() {
            let list: Vec<&str> = issues.iter().take(8).map(|i| i.message.as_str()).collect();
            let more = issues.len().saturating_sub(list.len());
            let mut text = list.join("\n\n");
            if more > 0 {
                text.push_str(&format!("\n\n(and {more} more: hover the red marks)"));
            }
            let _ = ui
                .interact(tally_area, egui::Id::new(("flow_tally", fi)), Sense::hover())
                .on_hover_text(text);
        }
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
        screen.line_segment(
            [
                pos2(rect.min.x + 1.0, view.max.y),
                pos2(rect.max.x - 1.0, view.max.y),
            ],
            Stroke::new(1.0, pal().border),
        );
        view_input(ctx, ed, view, &bg);
        // a click on the empty canvas lets go of the block and closes the inspector (and stops
        // following a variable, which is only drawn while the inspector is there)
        if bg.clicked() {
            ed.flow_sel = None;
            ed.flow_insp = false;
            ed.flow_var = None;
        }
        // a block the inspector asked to see goes to the middle of the view
        if let Some(id) = ed.flow_focus.take()
            && let Some(n) = flow.node(id)
        {
            let c = vec2(n.x + n.kind.width() / 2.0, n.y + node_h(&n.kind) / 2.0);
            ed.flow_pan = view.center() - rect.min - c * ed.flow_zoom;
        }
        let to_global = TSTransform::new(rect.min.to_vec2() + ed.flow_pan, ed.flow_zoom);
        let from_global = to_global.inverse();

        // Delete or Backspace removes the selected block, unless a field is being typed in, a
        // shortcut is being recorded or a list is open (Start and End stay: remove_node refuses)
        if let Some(id) = ed.flow_sel
            && !ctx.egui_wants_keyboard_input()
            && ed.recording.is_none()
            && !ed.sheet_open()
            && !egui::Popup::is_any_open(ctx)
            && ed.flow_menu.is_none()
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
            })
        {
            ops.push(Op::Remove(id));
        }
        // the right button anywhere on the canvas: the list of blocks to add, and the new block
        // goes where the pointer was
        if let Some((kind, at, from)) = canvas_menu(
            ctx,
            ed,
            view,
            from_global,
            flow.nodes.len() < MAX_FLOW_NODES,
        ) {
            match from {
                // from the "+" of an output: the block goes beside it and is wired to it
                Some(from) => ops.push(Op::AddFrom(kind, at, from)),
                None => ops.push(Op::AddAt(kind, at - vec2(20.0, HEAD_H / 2.0))),
            }
        }

        // a grid of dots that moves and zooms with the canvas (every second dot, and so on, when
        // zoomed out far enough that they would crowd)
        let mut step = 28.0 * ed.flow_zoom;
        while step < 14.0 {
            step *= 2.0;
        }
        let (ox, oy) = (ed.flow_pan.x.rem_euclid(step), ed.flow_pan.y.rem_euclid(step));
        let dot = pal().muted.gamma_multiply(0.55);
        let mut y = view.min.y + oy;
        while y < view.max.y {
            let mut x = view.min.x + ox;
            while x < view.max.x {
                screen.circle_filled(pos2(x, y), 1.5, dot);
                x += step;
            }
            y += step;
        }

        // the blocks and wires: a layer of their own that is zoomed and moved as a whole, so text,
        // fields and hit areas all follow
        let base_layer = ui.layer_id();
        let layer = egui::LayerId::new(ui.layer_id().order, ui.id().with(("flow_scene", fi)));
        ctx.set_sublayer(ui.layer_id(), layer);
        ctx.set_transform_layer(layer, to_global);
        let mut scene = ui.new_child(
            UiBuilder::new()
                .layer_id(layer)
                .max_rect(Rect::from_center_size(Pos2::ZERO, Vec2::splat(400_000.0))),
        );
        scene.set_clip_rect(from_global * view);
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
                    // a wire that cannot work is red, one that is probably a mistake amber, with
                    // a mark in the middle that says why
                    let sev = wire_severity(&issues, n.id, port);
                    let (color, width) = match sev {
                        Some(Severity::Error) => (pal().bad, 3.2),
                        Some(Severity::Warning) => (AMBER, 3.0),
                        None => (color, 2.5),
                    };
                    draw_wire(&painter, *a, *b, color, width);
                    if sev.is_some() {
                        let pts = wire_points(*a, *b);
                        if let Some(&mid) = pts.get(pts.len() / 2) {
                            painter.circle_filled(mid, 8.0, color);
                            painter.text(
                                mid,
                                Align2::CENTER_CENTER,
                                "!",
                                FontId::new(11.0, fonts::semibold()),
                                Color32::WHITE,
                            );
                            let why: Vec<&str> = issues
                                .iter()
                                .filter(|i| i.wire == Some((n.id, port)))
                                .map(|i| i.message.as_str())
                                .collect();
                            let _ = ui
                                .interact(
                                    Rect::from_center_size(mid, Vec2::splat(18.0)),
                                    egui::Id::new((
                                        "flow_wire_note",
                                        fi,
                                        n.id,
                                        matches!(port, Port::Alt),
                                    )),
                                    Sense::hover(),
                                )
                                .on_hover_text(why.join("\n\n"));
                        }
                    }
                }
            }
        }

        if flow.nodes.len() <= 2 {
            screen.text(
                view.center() + vec2(0.0, 30.0),
                Align2::CENTER_CENTER,
                "Right-click here, or press Add block, to add steps.\nThen drag from the dot on the right of a block\nto the dot on the left of the next one.",
                FontId::proportional(13.5),
                pal().muted,
            );
        }

        // the variable being followed: which blocks set it and read it, and where its value goes
        let trace = ed.flow_var.clone().map(|v| {
            let edges = data_edges(flow, &v);
            let sets: Vec<u32> = flow
                .nodes
                .iter()
                .filter(|n| block_writes(&n.kind).contains(&v))
                .map(|n| n.id)
                .collect();
            let gets: Vec<u32> = flow
                .nodes
                .iter()
                .filter(|n| block_reads(&n.kind).contains(&v))
                .map(|n| n.id)
                .collect();
            (v, edges, sets, gets)
        });
        if let Some((_, edges, ..)) = &trace {
            for &(w, r) in edges {
                let (Some(a), Some(b)) = (flow.node(w), flow.node(r)) else {
                    continue;
                };
                let p = origin + vec2(a.x + a.kind.width(), a.y + 6.0);
                let q = origin + vec2(b.x, b.y + 6.0);
                let pts = wire_points(p, q);
                for s in Shape::dashed_line(&pts, Stroke::new(2.5, READS_C), 7.0, 5.0) {
                    painter.add(s);
                }
                painter.circle_filled(q, 4.0, READS_C);
            }
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
            // a block that cannot work as it is, or did not in the last run, has a colored edge
            let worst = block_severity(&issues, id);
            let ran_badly = marks.iter().any(|(b, _)| *b == id);
            let flagged = if ran_badly || worst == Some(Severity::Error) {
                Some(pal().bad)
            } else if worst == Some(Severity::Warning) {
                Some(AMBER)
            } else {
                None
            };
            painter.rect_stroke(
                body_rect,
                CONTROL_R,
                if selected {
                    Stroke::new(2.0, accent)
                } else if let Some(c) = flagged {
                    Stroke::new(1.6, c)
                } else {
                    Stroke::new(1.0, pal().border)
                },
                StrokeKind::Inside,
            );
            // the blocks that set or read the variable being followed
            if let Some((v, _, sets, gets)) = &trace {
                let (w, r) = (sets.contains(&id), gets.contains(&id));
                if r {
                    painter.rect_stroke(
                        body_rect.expand(3.0),
                        CONTROL_R,
                        Stroke::new(2.5, READS_C),
                        StrokeKind::Middle,
                    );
                }
                if w {
                    painter.rect_stroke(
                        body_rect.expand(if r { 7.0 } else { 3.0 }),
                        CONTROL_R,
                        Stroke::new(2.5, SETS_C),
                        StrokeKind::Middle,
                    );
                }
                if w || r {
                    let text = match (w, r) {
                        (true, true) => format!("reads and sets {v}"),
                        (true, false) => format!("sets {v}"),
                        _ => format!("reads {v}"),
                    };
                    let g = painter.layout_no_wrap(
                        text,
                        FontId::new(11.5, fonts::semibold()),
                        Color32::WHITE,
                    );
                    let pill = Rect::from_min_size(
                        pos2(body_rect.min.x, body_rect.min.y - g.size().y - 12.0),
                        g.size() + vec2(12.0, 4.0),
                    );
                    painter.rect_filled(pill, 6.0, if w { SETS_C } else { READS_C });
                    painter.galley(pill.min + vec2(6.0, 2.0), g, Color32::WHITE);
                }
            }
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

            // the mark on the corner: what is wrong here, and what went wrong in the last run
            if let Some(c) = flagged {
                let mut notes: Vec<String> = block_issues(&issues, id)
                    .iter()
                    .map(|i| i.message.clone())
                    .collect();
                for (b, why) in &marks {
                    if *b == id {
                        notes.push(format!(
                            "In the last run this block could not do its work: {why}."
                        ));
                    }
                }
                let at = pos2(body_rect.min.x + 2.0, body_rect.min.y + 2.0);
                painter.circle_filled(at, 8.5, c);
                painter.text(
                    at,
                    Align2::CENTER_CENTER,
                    "!",
                    FontId::new(11.5, fonts::semibold()),
                    Color32::WHITE,
                );
                let _ = ui
                    .interact(
                        Rect::from_center_size(at, Vec2::splat(20.0)),
                        egui::Id::new(("flow_note", fi, id)),
                        Sense::hover(),
                    )
                    .on_hover_text(notes.join("\n\n"));
            }

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
                // an output with nothing plugged in: a "+" that adds a block already wired to it
                if !wired && wiring.is_none() && flow.nodes.len() < MAX_FLOW_NODES {
                    // on the same line as the dot, so the wire out of it is straight, and the
                    // block it adds (see `scene` below) lands right at the end of that line
                    let plus_c = out_c + vec2(34.0, 0.0);
                    let pr = ui.interact(
                        Rect::from_center_size(plus_c, Vec2::splat(24.0)),
                        egui::Id::new(("flow_plus", fi, id, k)),
                        Sense::click(),
                    );
                    let hot = pr.hovered();
                    painter.line_segment(
                        [out_c + vec2(PORT_R, 0.0), plus_c - vec2(9.0, 0.0)],
                        Stroke::new(1.5, accent.gamma_multiply(0.5)),
                    );
                    painter.circle_filled(plus_c, 9.0, if hot { accent } else { pal().card });
                    painter.circle_stroke(plus_c, 9.0, Stroke::new(1.5, accent));
                    fonts::paint_icon(
                        &painter,
                        plus_c,
                        symbols::PLUS,
                        12.0,
                        if hot { Color32::WHITE } else { accent },
                        true,
                    );
                    if pr
                        .on_hover_text("Add a block here, already wired to this output")
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        let at = ctx.input(|i| i.pointer.interact_pos()).unwrap_or(view.center());
                        ed.flow_search.clear();
                        ed.flow_menu = Some(FlowMenu {
                            at,
                            scene: out_c + vec2(56.0, -HEAD_H / 2.0),
                            rect: Rect::NOTHING,
                            from: Some((id, port)),
                        });
                    }
                }
                if !names[k].is_empty() {
                    painter.text(
                        // above the line out of the dot, clear of the "+" on it
                        out_c + vec2(10.0, -15.0),
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
                // a press anywhere in the body (a field too) selects the block
                // (not when something floats over it there: the bar under the canvas, a list)
                let covered = ctx
                    .input(|i| i.pointer.latest_pos())
                    .and_then(|p| ctx.layer_id_at(p))
                    .is_some_and(|l| l != layer && l != base_layer && l.order >= base_layer.order);
                if !covered
                    && ctx.input(|i| i.pointer.primary_pressed())
                    && pointer.is_some_and(|p| body_rect.contains(p))
                {
                    ed.flow_sel = Some(id);
                }
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
        // the hint line, in the strip under the blocks; the longest of these that fits the width
        // is the one shown, so it never runs out of the canvas
        let head = format!(
            "{steps} step{} \u{b7} {}%",
            if steps == 1 { "" } else { "s" },
            (ed.flow_zoom * 100.0).round()
        );
        let hints = [
            format!(
                "{head} \u{b7} right-click adds a block \u{b7} Alt + scroll or pinch zooms \u{b7} middle button or two fingers moves"
            ),
            format!("{head} \u{b7} right-click adds a block \u{b7} Alt + scroll zooms"),
            head,
        ];
        // the count of problems, at the right of the strip
        let errors = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count();
        let warnings = issues.len() - errors;
        let mut parts: Vec<String> = Vec::new();
        if errors > 0 {
            parts.push(format!("{errors} error{}", if errors == 1 { "" } else { "s" }));
        }
        if warnings > 0 {
            parts.push(format!(
                "{warnings} warning{}",
                if warnings == 1 { "" } else { "s" }
            ));
        }
        if !marks.is_empty() {
            parts.push(format!(
                "{} failed in the last run",
                marks.len()
            ));
        }
        let mut taken = 0.0;
        if !parts.is_empty() {
            let color = if errors > 0 || !marks.is_empty() {
                pal().bad
            } else {
                AMBER
            };
            let g = screen.layout_no_wrap(
                parts.join(" \u{b7} "),
                FontId::new(12.0, fonts::semibold()),
                color,
            );
            taken = g.size().x + 24.0;
            let at = pos2(
                rect.max.x - 14.0 - g.size().x,
                rect.max.y - STATUS_H / 2.0 - g.size().y / 2.0,
            );
            screen.galley(at, g, Color32::PLACEHOLDER);
        }
        let room = rect.width() - 28.0 - taken;
        let mut shown = None;
        for text in hints {
            let g = screen.layout_no_wrap(text, FontId::proportional(12.0), pal().muted);
            let fits = g.size().x <= room;
            shown = Some(g);
            if fits {
                break;
            }
        }
        if let Some(g) = shown {
            let at = pos2(rect.min.x + 14.0, rect.max.y - STATUS_H / 2.0 - g.size().y / 2.0);
            screen.galley(at, g, Color32::PLACEHOLDER);
        }

        // everything of the blocks is drawn: lay their text out again at the zoomed size
        crisp_text(ctx, &painter, layer, ed.flow_zoom);
    });

    apply_ops(ops, ctx, flow, ed, tx);
}

/// Carries out what the blocks (on the canvas or in the inspector) asked for.
pub(super) fn apply_ops(
    ops: Vec<Op>,
    ctx: &egui::Context,
    flow: &mut Flow,
    ed: &mut Ed,
    tx: &Sender<Msg>,
) {
    let fi = ed.flow;
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
            Op::AddFrom(kind, at, (from, port)) => {
                if flow.nodes.len() < MAX_FLOW_NODES
                    && flow.node(from).is_some_and(|n| n.wire(port).is_none())
                {
                    let id = flow.add_at(kind, at.x, at.y);
                    flow.connect(from, port, id);
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
            Op::PickApp(id) => pick_app(ctx, ed, tx, fi, id),
            Op::DesignPanel(id) => panel_designer::open(ed, fi, flow, id),
            Op::Record(id) => toggle_recording(ed, fi, id),
        }
    }
}

/// Opens the list of installed apps for the *Open app* block `id` of flow `fi`.
pub(super) fn pick_app(ctx: &egui::Context, ed: &mut Ed, tx: &Sender<Msg>, fi: usize, id: u32) {
    ed.flow_target = Some((fi, id));
    open_picker(ctx, ed, tx, PickFor::Flow);
}

/// Starts recording a shortcut for the *Shortcut* block `id` of flow `fi`, or stops when it is
/// already being recorded.
pub(super) fn toggle_recording(ed: &mut Ed, fi: usize, id: u32) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::{CARD_SIZE, WIN_MARGIN};
    use crate::ui::settings::Tab;

    #[test]
    fn the_crisp_scale_is_a_short_ladder() {
        // every zoom of a gesture lands on one of a few sizes, so the font atlas stays small
        let mut sizes: Vec<u32> = Vec::new();
        let mut z = ZOOM_MIN;
        while z <= ZOOM_MAX {
            let q = crisp_scale(z);
            assert!(
                (q / z).max(z / q) < 1.1,
                "{z} is cut at {q}: more than 10% off"
            );
            sizes.push(q.to_bits());
            z *= 1.003;
        }
        sizes.sort_unstable();
        sizes.dedup();
        assert!(
            sizes.len() <= 16,
            "{} sizes for the whole range",
            sizes.len()
        );
        assert_eq!(crisp_scale(1.0), 1.0, "100% is cut at its own size");
        assert_eq!(crisp_scale(0.97), 1.0);
        assert_eq!(crisp_scale(2.0), 2.0);
    }

    #[test]
    fn the_crisp_scale_survives_odd_input() {
        for z in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(crisp_scale(z), 1.0);
        }
    }

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
            NodeKind::List {
                op: ListOp::Get,
                list: "xs".into(),
                a: "1".into(),
                b: String::new(),
                out: "x".into(),
            },
            NodeKind::List {
                op: ListOp::Split,
                list: "xs".into(),
                a: "a,b".into(),
                b: ",".into(),
                out: String::new(),
            },
            NodeKind::ForEach {
                list: "xs".into(),
                item: "x".into(),
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
        let list = |op| NodeKind::List {
            op,
            list: String::new(),
            a: String::new(),
            b: String::new(),
            out: String::new(),
        };
        assert_eq!(
            rows(&list(ListOp::Sort)),
            1,
            "the pill and the list name share a row"
        );
        assert_eq!(rows(&list(ListOp::Get)), 3);
        assert_eq!(rows(&list(ListOp::Split)), 3);
        assert_eq!(rows(&list(ListOp::Push)), 2);
        assert_eq!(
            rows(&NodeKind::ForEach {
                list: String::new(),
                item: String::new()
            }),
            2
        );
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
    fn tidy_leaves_no_block_over_another_and_is_stable() {
        let mut flow = Flow::new("flow-1", "Demo");
        let mut last = None;
        for kind in every_kind()
            .into_iter()
            .filter(|k| !matches!(k, NodeKind::Start | NodeKind::End | NodeKind::Stop))
        {
            last = Some(flow.add_after(last, kind));
        }
        // every block on one spot, as a messy import can leave them
        for n in &mut flow.nodes {
            n.x = 0.0;
            n.y = 0.0;
        }
        tidy(&mut flow);
        let rect = |n: &FlowNode| {
            Rect::from_min_size(pos2(n.x, n.y), vec2(n.kind.width(), node_h(&n.kind)))
        };
        for (i, a) in flow.nodes.iter().enumerate() {
            assert!(a.x >= TIDY_ORIGIN && a.y >= TIDY_ORIGIN, "{a:?}");
            for b in flow.nodes.iter().skip(i + 1) {
                assert!(
                    !rect(a).intersects(rect(b)),
                    "{:?} covers {:?}",
                    a.kind,
                    b.kind
                );
            }
        }
        // the same flow always gets the same places
        let once = flow.clone();
        tidy(&mut flow);
        assert_eq!(flow, once);
        // Start is the first column, End the last of the walk
        let start = flow.start().expect("a flow has a start");
        let end = flow.end().expect("a flow has an end");
        assert_eq!(start.x, TIDY_ORIGIN);
        assert!(end.x > start.x);
    }

    #[test]
    fn a_line_break_in_a_field_is_shown_as_a_sign_and_comes_back_unchanged() {
        let text = "File:\n{{file}}\n\nGo?";
        let shown = show_breaks(text);
        assert!(!shown.chars().any(char::is_control), "{shown:?}");
        assert_eq!(hide_breaks(&shown), text);
        assert_eq!(show_breaks("plain"), "plain");
        assert_eq!(show_breaks("a\r\nb"), "a\u{b6}b");
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

    #[test]
    fn the_canvas_draws_at_every_zoom_without_changing_the_flow() {
        // laying the text out again at the zoomed size must neither hang (the shapes are copied
        // out of the layer first) nor touch the flow
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
        for zoom in [ZOOM_MIN, 0.5, 1.0, 2.0, ZOOM_MAX] {
            for _ in 0..2 {
                app.ed.flow_zoom = zoom;
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
        }
        assert_eq!(app.edit, before, "zooming never edits the flow");
    }
}
