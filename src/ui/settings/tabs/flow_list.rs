//! The Flow tab as most people know such a thing from Shortcuts, Automator or Zapier: the flows in a
//! list on the left, and the flow itself as **steps one under the other**, each a card with
//! labelled fields, a round "+" between two steps that opens one searchable list of everything
//! that can be added, and up / down / remove on each card.
//!
//! What is shown is the flow of [`config::flow_list`](crate::config::flow_list): the same blocks
//! and wires as on the canvas, read as a list. The canvas is still one click away (Steps / Canvas
//! at the top) for a flow that is wired freely, and it stays the place to see the wires.

use super::flow::{
    AMBER, Do, browser_by_variable, cond_row, field, kind_info, notice, operands_row, pick, tint,
    toggle_recording,
};
use crate::config::flow_check::{Severity, block_issues, check_flow};
use crate::config::flow_list::{self as outline, Arm, Item, Outline, Spot};
use crate::config::*;
use crate::keys::{Shortcut, key};
use crate::msg::Msg;
use crate::ui::draw::on_accent;
use crate::ui::fonts;
use crate::ui::settings::action_editor::{bridge_line, browser_fields};
use crate::ui::settings::state::{Ed, FlowView, Slot};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{
    self, Color32, FontId, Rect, RichText, Sense, Stroke, UiBuilder, Vec2, pos2, vec2,
};
use std::sync::mpsc::Sender;

/// What a new *Show a panel* block starts with: a question with a box, a switch and two buttons.
const PANEL_TEMPLATE: &str = r#"{
  "title": "Question",
  "widgets": [
    { "type": "input", "id": "answer", "label": "Your answer", "required": true },
    { "type": "toggle", "id": "again", "label": "Ask again later" }
  ],
  "buttons": [
    { "id": "cancel", "label": "Cancel", "cancel": true },
    { "id": "ok", "label": "OK", "tone": "primary" }
  ]
}"#;

/// Shows the scroll bar of the list of flows only when there are more flows than room.
const BAR_VIS: egui::scroll_area::ScrollBarVisibility =
    egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded;

/// Width of the list of flows on the left.
pub(super) const SIDEBAR_W: f32 = 200.0;
/// How long the list takes to come out and to go back, in seconds.
const SIDE_ANIM: f32 = 0.30;
/// How long the list stays out after the pointer has left it, in seconds: a pointer that slips
/// off the edge for a moment does not make it flicker shut.
const SIDE_GRACE: f64 = 0.30;
/// How wide and how tall (at most) the list of things to add is.
pub(super) const MENU_W: f32 = 300.0;
pub(super) const MENU_H: f32 = 400.0;
/// The widest a card of a step gets: wider than this, the line of a field is hard to follow.
const CARD_MAX_W: f32 = 700.0;
/// Width of the column with the names of the fields of a card.
const LABEL_W: f32 = 96.0;
/// How far the steps of a part (*then*, *else*, the body of a loop) are moved in.
const INDENT: f32 = 26.0;
/// Height of the row with the round "+" between two steps.
const GAP_H: f32 = 30.0;
/// Size of a button in the capsule of the header: with the margin of the capsule the whole is as
/// high as every other control.
/// Every button of a capsule is a circle of the same size as the cells of the view switch, and
/// the buttons of one capsule are [`BAR_GAP`] apart, so the header and the floating bars line up.
const BAR_BTN: Vec2 = Vec2::splat(CONTROL_H - 6.0);
/// The gap between two buttons of a capsule (the same as between the cells of the view switch).
const BAR_GAP: f32 = 2.0;
/// The padding between the edge of a capsule and its buttons.
const BAR_PAD: i8 = 3;

// ------------------------------------------------------------------ the list of flows

/// The flows, one under the other. It is not part of the page: it slides over it from the left
/// edge when the pointer goes there and slides back a moment after the pointer leaves, so the
/// steps and the canvas keep the whole width. A thin mark at the edge says it is there.
///
/// The list scrolls, so it holds as many flows as there will ever be. A name that is too long is
/// cut with an ellipsis (the whole name is in the tooltip). Making, importing, duplicating,
/// exporting and deleting a flow are in the header of the page, not here.
pub(super) fn sidebar(
    ctx: &egui::Context,
    body: Rect,
    edit: &Config,
    ed: &mut Ed,
    accent: Color32,
) -> Option<Do> {
    let mut todo = None;
    let now = ctx.input(|i| i.time);
    // the strip at the left edge that brings the list out
    let trigger = Rect::from_min_max(body.min, pos2(body.min.x + 18.0, body.max.y));
    // while it is out it stays out as long as the pointer is in the column it covers, with room to
    // spare on every side, from the top of the window to the bottom: going to the top or the
    // bottom of the list must not close it
    let column = Rect::from_min_max(
        pos2(body.min.x, f32::MIN / 4.0),
        pos2(body.min.x + SIDEBAR_W + 28.0, f32::MAX / 4.0),
    );
    let (pointer, down) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.any_down()));
    let want = if ed.flow_side {
        if pointer.is_some_and(|p| column.contains(p)) {
            ed.flow_side_leave = None;
            true
        } else {
            // out of the column: it goes after a short grace time, not at once
            let since = *ed.flow_side_leave.get_or_insert(now);
            let stay = now - since < SIDE_GRACE;
            if stay {
                ctx.request_repaint_after(std::time::Duration::from_millis(40));
            }
            stay
        }
    } else {
        ed.flow_side_leave = None;
        // a drag that happens to end up at the edge (a block, the canvas) does not open it
        pointer.is_some_and(|p| trigger.contains(p))
            && !down
            && !ed.sheet_open()
            && ed.recording.is_none()
    };
    ed.flow_side = want;
    // one clock for everything that moves, eased once: the list, its fade and the mark at the edge
    // all follow it, so they start and stop together
    let lin = ctx.animate_value_with_time(
        egui::Id::new("flow_side"),
        if ed.flow_side { 1.0 } else { 0.0 },
        SIDE_ANIM,
    );
    let t = egui::emath::easing::cubic_out(lin.clamp(0.0, 1.0));
    if t < 1.0 {
        // the mark at the edge: a small pill that fades as the list comes out
        let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("flow_side_mark"));
        let c = pos2(body.min.x + 5.0, body.center().y);
        ctx.layer_painter(layer).rect_filled(
            Rect::from_center_size(c, vec2(4.0, 48.0)),
            PILL,
            wash(pal().muted, 0.45 * (1.0 - t)),
        );
    }
    if lin <= 0.001 {
        ed.flow_side_rect = Rect::NOTHING;
        return None;
    }
    // the list is as tall as the page and slides in from the left edge of the page (what is
    // outside the page is cut away, so it never paints over the menu of the window), fading in
    // as it comes
    let first_frame = ed.flow_side_rect == Rect::NOTHING;
    let rest_x = body.min.x + 6.0;
    let x = rest_x - (1.0 - t) * (SIDEBAR_W + 18.0);
    let panel = Rect::from_min_max(
        pos2(x, body.min.y + 6.0),
        pos2(x + SIDEBAR_W, body.max.y - 6.0),
    );
    egui::Area::new(egui::Id::new("flow_side"))
        .order(egui::Order::Foreground)
        .fixed_pos(panel.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(body);
            ui.set_opacity(t);
            ui.allocate_rect(panel, Sense::hover());
            ui.painter().add(popup_shadow().as_shape(panel, PANEL_R));
            ui.painter().rect_filled(panel, PANEL_R, pal().card);
            ui.painter().rect_stroke(
                panel,
                PANEL_R,
                Stroke::new(1.0, pal().border),
                egui::StrokeKind::Inside,
            );
            let inner = panel.shrink(10.0);
            // the title and how many flows there are
            let head = Rect::from_min_size(inner.min, vec2(inner.width(), 30.0));
            let p = ui.painter();
            p.text(
                pos2(head.min.x + 6.0, head.center().y),
                egui::Align2::LEFT_CENTER,
                "Flows",
                FontId::new(14.5, fonts::semibold()),
                pal().text,
            );
            let count = p.layout_no_wrap(
                edit.flows.len().to_string(),
                FontId::proportional(11.5),
                pal().muted,
            );
            let pill = Rect::from_center_size(
                pos2(
                    head.max.x - (count.size().x + 14.0) / 2.0 - 2.0,
                    head.center().y,
                ),
                vec2(count.size().x + 14.0, 20.0),
            );
            p.rect_filled(pill, PILL, pal().idle);
            p.galley(
                pill.center() - count.size() / 2.0,
                count,
                Color32::PLACEHOLDER,
            );
            let line_y = head.max.y + 5.0;
            p.line_segment(
                [pos2(inner.min.x, line_y), pos2(inner.max.x, line_y)],
                Stroke::new(1.0, pal().divider),
            );
            // the flows: this part scrolls
            let list = Rect::from_min_max(pos2(inner.min.x, line_y + 8.0), inner.max);
            ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("flow_side_list")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(BAR_VIS)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        for (i, f) in edit.flows.iter().enumerate() {
                            let name = if f.name.trim().is_empty() {
                                "Untitled"
                            } else {
                                f.name.as_str()
                            };
                            let on = i == ed.flow;
                            let resp = flow_row(ui, name, on, accent);
                            if on && first_frame {
                                // opening the list with many flows shows the one that is open
                                resp.scroll_to_me(Some(egui::Align::Center));
                            }
                            if resp.clicked() && !on {
                                todo = Some(Do::Pick(i));
                            }
                        }
                        ui.add_space(4.0);
                    });
            });
        });
    ed.flow_side_rect = panel;
    todo
}

/// One flow in the list: a lightning bolt, the name cut with an ellipsis when it does not fit,
/// and for the open flow a soft wash of the accent with a small bar at the left. The wash of the
/// pointer fades in and out instead of switching.
fn flow_row(ui: &mut egui::Ui, name: &str, on: bool, accent: Color32) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_H), Sense::click());
    let hot = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hot"), resp.hovered(), 0.12);
    let p = ui.painter();
    if on {
        p.rect_filled(rect, CONTROL_R, wash(accent, 0.14));
        p.rect_filled(
            Rect::from_center_size(pos2(rect.min.x + 4.0, rect.center().y), vec2(3.0, 16.0)),
            PILL,
            accent,
        );
    } else if hot > 0.0 {
        p.rect_filled(rect, CONTROL_R, pal().idle.gamma_multiply(hot));
    }
    fonts::paint_icon(
        p,
        pos2(rect.min.x + 23.0, rect.center().y),
        symbols::LIGHTNING,
        16.0,
        if on { accent } else { pal().muted },
        false,
    );
    let text_x = rect.min.x + 42.0;
    let (g, cut) = fit_galley(
        ui,
        name,
        text_font(14.0, on),
        pal().text,
        rect.max.x - 12.0 - text_x,
    );
    ui.painter().galley(
        pos2(text_x, rect.center().y - g.size().y / 2.0).round(),
        g,
        Color32::PLACEHOLDER,
    );
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if cut { resp.on_hover_text(name) } else { resp }
}

// ------------------------------------------------------------------ the header

/// How a button of the capsule in the header looks.
#[derive(Clone, Copy)]
enum Tone {
    /// Calm: the glyph is grey until the pointer is over it.
    Plain,
    /// Filled with the accent: the main thing to do.
    Accent(Color32),
    /// Red: destructive.
    Danger,
}

/// One round button of the capsule: a glyph, a tooltip, a wash that fades in under the pointer.
fn bar_button(ui: &mut egui::Ui, icon: char, tip: &str, tone: Tone) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(BAR_BTN, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
    let h = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hot"), resp.hovered(), 0.12);
    let press = if resp.is_pointer_button_down_on() {
        0.08
    } else {
        0.0
    };
    let (fill, ink) = match tone {
        Tone::Plain => (
            pal().hot.gamma_multiply(h),
            pal().muted.lerp_to_gamma(pal().text, h),
        ),
        Tone::Accent(a) => (
            a.lerp_to_gamma(Color32::BLACK, 0.12 * h + press),
            on_accent(a),
        ),
        Tone::Danger => (wash(pal().bad, 0.14 * h + press), pal().bad),
    };
    ui.painter().rect_filled(rect, PILL, fill);
    fonts::paint_icon(ui.painter(), rect.center(), icon, 17.0, ink, false);
    resp.on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The button of the capsule that fills the whole display with the Flow tab (and, while it does,
/// goes back to the window).
fn full_button(ui: &mut egui::Ui, full: bool) -> egui::Response {
    if full {
        bar_button(
            ui,
            symbols::ARROWS_IN,
            "Leave full screen (Esc)",
            Tone::Plain,
        )
    } else {
        bar_button(ui, symbols::ARROWS_OUT, "Full screen (F11)", Tone::Plain)
    }
}

/// The thin line between two groups of buttons of the capsule.
fn bar_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(BAR_GAP * 2.0 + 1.0, 16.0), Sense::hover());
    ui.painter().line_segment(
        [rect.center_top(), rect.center_bottom()],
        Stroke::new(1.0, pal().border),
    );
}

/// The switch between the list of steps and the canvas: two icons on one round track with a
/// circle that travels to the chosen one (see [`icon_switch`]).
fn view_switch(ui: &mut egui::Ui, current: FlowView, accent: Color32) -> Option<FlowView> {
    let views = [
        (
            FlowView::Steps,
            symbols::STACK,
            "Steps: one step under the other",
        ),
        (
            FlowView::Canvas,
            symbols::SELECTION_ALL,
            "Canvas: blocks and wires, for flows that need free wiring",
        ),
    ];
    let items: Vec<(char, String)> = views
        .iter()
        .map(|(_, icon, tip)| (*icon, (*tip).to_string()))
        .collect();
    let at = views
        .iter()
        .position(|(v, _, _)| *v == current)
        .unwrap_or(0);
    icon_switch(ui, &items, at, accent, pal().idle, CONTROL_H - 6.0).map(|i| views[i].0)
}

/// The name of the flow, the switch between the list of steps and the canvas, and one capsule
/// with everything that can be done with flows: new and import, duplicate and export, delete.
pub(super) fn header(ui: &mut egui::Ui, edit: &mut Config, ed: &Ed, accent: Color32) -> Option<Do> {
    let mut todo = None;
    let fi = ed.flow;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            egui::Frame::new()
                .fill(pal().idle)
                .corner_radius(PILL)
                .inner_margin(egui::Margin::same(BAR_PAD))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = BAR_GAP;
                        // this row runs right to left like the one around it: the last button
                        // added is the one at the left
                        if bar_button(ui, symbols::TRASH, "Delete this flow", Tone::Danger)
                            .clicked()
                        {
                            todo = Some(Do::Delete);
                        }
                        bar_divider(ui);
                        if bar_button(
                            ui,
                            symbols::UPLOAD_SIMPLE,
                            "Export this flow\u{2026}",
                            Tone::Plain,
                        )
                        .clicked()
                        {
                            todo = Some(Do::Export);
                        }
                        if bar_button(ui, symbols::COPY, "Duplicate this flow", Tone::Plain)
                            .clicked()
                        {
                            todo = Some(Do::Duplicate);
                        }
                        bar_divider(ui);
                        if bar_button(
                            ui,
                            symbols::DOWNLOAD_SIMPLE,
                            "Import a flow\u{2026}",
                            Tone::Plain,
                        )
                        .clicked()
                        {
                            todo = Some(Do::Import);
                        }
                        if bar_button(ui, symbols::PLUS, "New flow", Tone::Accent(accent)).clicked()
                        {
                            todo = Some(Do::NewMenu);
                        }
                    });
                });
            ui.add_space(8.0);
            if let Some(view) = view_switch(ui, ed.flow_view, accent) {
                todo = Some(Do::View(view));
            }
            ui.add_space(8.0);
            ui.add(
                text_field(&mut edit.flows[fi].name)
                    .desired_width(ui.available_width())
                    .hint_text("Name of the flow"),
            );
        });
    });
    todo
}

/// The round "+" of the floating bars: the main thing to do, filled with the accent.
fn add_button(ui: &mut egui::Ui, enabled: bool, accent: Color32, tip: &str) -> egui::Response {
    round_button(
        ui,
        Look::Primary(accent),
        symbols::PLUS,
        tip,
        enabled,
        BAR_BTN.x,
    )
}

/// The "+" of a floating bar: the accent in every state (it only gets a little darker under the
/// pointer, and not at all while the list it opens is out), and with a fade of its own.
fn plus_button(ui: &mut egui::Ui, enabled: bool, accent: Color32, tip: &str) -> egui::Response {
    if enabled {
        bar_button(ui, symbols::PLUS, tip, Tone::Accent(accent))
    } else {
        add_button(ui, false, accent, tip)
    }
}

/// A bar that floats at the bottom of `area`, in the middle, as a capsule with a soft shadow.
/// `add` draws what is in it and returns the response of the round "+" (the list of things to add
/// opens over the whole capsule, in its middle, when that is clicked). Returns what was picked.
fn floating_bar(
    ctx: &egui::Context,
    id: &'static str,
    area: Rect,
    search: &mut String,
    can_add: bool,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> Option<NodeKind> {
    let mut picked = None;
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(area.center().x, area.max.y - 16.0))
        .constrain(false)
        .show(ctx, |ui| {
            let out = egui::Frame::new()
                .fill(pal().card)
                .stroke(Stroke::new(1.0, pal().border))
                .corner_radius(PILL)
                .inner_margin(egui::Margin::same(BAR_PAD))
                .shadow(popup_shadow())
                .show(ui, |ui| ui.horizontal(add).inner);
            // the list is anchored to the capsule, not to the "+" in it, so it opens centered
            // (egui places a popup by `interact_rect`, so that one has to be the capsule's too, or
            // the list lines up with the "+" and sits off to the left)
            let mut anchor = out.inner;
            anchor.rect = out.response.rect;
            anchor.interact_rect = out.response.rect;
            if can_add {
                picked = quick_popup(&anchor, search, id);
            }
        });
    picked
}

/// The bar of the canvas: a round "+" to add a block, and three buttons to line the blocks up, to
/// bring them back into view and to fill the whole display with the Flow tab. It floats over the bottom of the canvas (`view`, the part where the
/// blocks are), in the middle, so the canvas keeps all the room above it.
pub(super) fn canvas_bar(
    ctx: &egui::Context,
    view: Rect,
    flow: &Flow,
    search: &mut String,
    accent: Color32,
    full: bool,
    inspector: bool,
) -> Option<Do> {
    let mut todo = None;
    let can_add = flow.nodes.len() < MAX_FLOW_NODES;
    let picked = floating_bar(ctx, "flow_canvas_bar", view, search, can_add, |ui| {
        ui.spacing_mut().item_spacing.x = BAR_GAP;
        let plus = plus_button(ui, can_add, accent, "Add a block");
        category_strip(ui, "flow_canvas_bar", accent);
        bar_divider(ui);
        if bar_button(
            ui,
            symbols::SQUARES_FOUR,
            "Tidy up: line the blocks up in columns so none covers another",
            Tone::Plain,
        )
        .clicked()
        {
            todo = Some(Do::Tidy);
        }
        if bar_button(
            ui,
            symbols::SCAN,
            "Bring the blocks back into view",
            Tone::Plain,
        )
        .clicked()
        {
            todo = Some(Do::ResetView);
        }
        if bar_button(
            ui,
            symbols::SIDEBAR_SIMPLE,
            if inspector {
                "Close the inspector"
            } else {
                "Inspector: edit the selected block and follow its variables"
            },
            if inspector {
                Tone::Accent(accent)
            } else {
                Tone::Plain
            },
        )
        .clicked()
        {
            todo = Some(Do::ToggleInspector);
        }
        if full_button(ui, full).clicked() {
            todo = Some(Do::ToggleFull);
        }
        plus
    });
    picked.map(Do::Add).or(todo)
}

/// The bar of the list of steps: a round "+" that adds a step at the end. It floats over the
/// bottom of the page (`area`), in the middle.
pub(super) fn steps_bar(
    ctx: &egui::Context,
    area: Rect,
    flow: &Flow,
    search: &mut String,
    accent: Color32,
    full: bool,
) -> Option<Do> {
    let can_add = flow.nodes.len() < MAX_FLOW_NODES;
    let mut todo = None;
    let picked = floating_bar(ctx, "flow_steps_bar", area, search, can_add, |ui| {
        ui.spacing_mut().item_spacing.x = BAR_GAP;
        let plus = plus_button(ui, can_add, accent, "Add a step");
        category_strip(ui, "flow_steps_bar", accent);
        bar_divider(ui);
        if full_button(ui, full).clicked() {
            todo = Some(Do::ToggleFull);
        }
        plus
    });
    picked.map(Do::AddStep).or(todo)
}

// ------------------------------------------------------------------ the categories in the bar

/// Id of the list of things to add that the "+" of the bar `bar` opens.
fn menu_id(bar: &str) -> egui::Id {
    egui::Id::new(bar).with("menu")
}

/// Where the category picked for the bar `bar` is kept while its list is open.
fn group_key(bar: &str) -> egui::Id {
    egui::Id::new(bar).with("group")
}

/// The category (an index into [`quick_catalog`]) the open list of `bar` is narrowed to, if any.
fn picked_group(ctx: &egui::Context, bar: &str) -> Option<usize> {
    ctx.data(|d| d.get_temp::<Option<usize>>(group_key(bar)))
        .flatten()
}

/// The icon that stands for a group of [`quick_catalog`].
fn group_icon(group: &str) -> char {
    match group {
        "Do something" => symbols::LIGHTNING,
        "Decide and repeat" => symbols::ARROWS_CLOCKWISE,
        "Ask and run" => symbols::BELL,
        "Browser" => symbols::BROWSER,
        "Variables" => symbols::BRACKETS_CURLY,
        "Read from the system" => symbols::CPU,
        "Files and folders" => symbols::FOLDER_SIMPLE,
        "Lists" => symbols::STACK,
        _ => symbols::SQUARES_FOUR,
    }
}

/// How long the categories take to unfold and to fold, and how long the circle under the chosen one
/// takes to travel, in seconds.
const UNFOLD_SECS: f32 = 0.24;
const THUMB_SECS: f64 = 0.28;

/// The categories in the capsule, right after the "+": one round cell per category on a row that
/// unfolds from the "+" while the list is open and folds back when it closes (the capsule grows and
/// shrinks with it). The chosen category has a circle in the accent color under it that travels to
/// the next one, stretching on the way, as the circle of the switch of the Behavior tab does (see
/// [`icon_switch`]). A click narrows the list to that category, a second click on the same one shows
/// everything again.
fn category_strip(ui: &mut egui::Ui, bar: &'static str, accent: Color32) {
    let ctx = ui.ctx().clone();
    let open = egui::Popup::is_id_open(&ctx, menu_id(bar));
    let t = ctx.animate_bool_with_time_and_easing(
        egui::Id::new(bar).with("unfold"),
        open,
        UNFOLD_SECS,
        egui::emath::easing::cubic_out,
    );
    let groups: Vec<&'static str> = quick_catalog().iter().map(|(g, _)| *g).collect();
    let n = groups.len();
    let cell = BAR_BTN.x;
    let full = n as f32 * cell + n.saturating_sub(1) as f32 * BAR_GAP;
    let (rect, _) = ui.allocate_exact_size(vec2(full * t, cell), Sense::hover());
    if t <= 0.0 {
        return;
    }
    let sel = picked_group(&ctx, bar);
    let cell_at = |i: usize| {
        Rect::from_min_size(
            pos2(rect.min.x + i as f32 * (cell + BAR_GAP), rect.min.y),
            Vec2::splat(cell),
        )
    };
    // the cells answer only once the row is (nearly) out
    let mut hot = vec![0.0f32; n];
    if t > 0.6 {
        for (i, group) in groups.iter().enumerate() {
            let hit = cell_at(i).intersect(rect);
            let id = egui::Id::new(bar).with(("group_cell", i));
            let resp = ui.interact(hit, id, Sense::click());
            resp.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::RadioButton,
                    true,
                    sel == Some(i),
                    *group,
                )
            });
            hot[i] = ctx.animate_bool_with_time(id.with("hot"), resp.hovered(), 0.12);
            if resp
                .on_hover_text(*group)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                let next = if sel == Some(i) { None } else { Some(i) };
                ctx.data_mut(|d| d.insert_temp(group_key(bar), next));
            }
        }
    }
    // the move of the circle: from which cell, to which, since when (a circle that comes in from
    // nothing appears where it is chosen instead of travelling there)
    let now = ui.input(|i| i.time);
    let (move_key, last_key) = (
        egui::Id::new(bar).with("thumb_move"),
        egui::Id::new(bar).with("thumb_last"),
    );
    let last = ctx
        .data(|d| d.get_temp::<Option<usize>>(last_key))
        .flatten();
    let (mut from, mut to, mut since) = ctx
        .data(|d| d.get_temp::<(usize, usize, f64)>(move_key))
        .unwrap_or((0, 0, -1.0e9));
    if let Some(s) = sel {
        if last.is_none() {
            (from, to, since) = (s, s, -1.0e9);
        } else if s != to {
            (from, to, since) = (to, s, now);
        }
    }
    ctx.data_mut(|d| {
        d.insert_temp(move_key, (from, to, since));
        d.insert_temp(last_key, sel);
    });
    let u = (((now - since) / THUMB_SECS) as f32).clamp(0.0, 1.0);
    if u < 1.0 {
        ctx.request_repaint();
    }
    let lead = egui::emath::easing::cubic_out(u);
    let lag = egui::emath::easing::cubic_in(u);
    let (a, b) = (cell_at(from), cell_at(to));
    let right = b.min.x >= a.min.x;
    let left_edge = a.min.x + (b.min.x - a.min.x) * if right { lag } else { lead };
    let right_edge = a.max.x + (b.max.x - a.max.x) * if right { lead } else { lag };
    let thumb = Rect::from_min_max(pos2(left_edge, a.min.y), pos2(right_edge, a.max.y));
    let shown =
        ctx.animate_bool_with_time(egui::Id::new(bar).with("thumb_shown"), sel.is_some(), 0.15);
    let painter = ui.painter().with_clip_rect(rect.expand2(vec2(0.0, 4.0)));
    for (i, h) in hot.iter().enumerate() {
        // the wash under the pointer, on a cell that is not chosen
        if *h > 0.0 && sel != Some(i) {
            painter.rect_filled(cell_at(i), PILL, pal().hot.gamma_multiply(*h * t));
        }
    }
    if shown > 0.0 {
        painter.rect_filled(thumb, PILL, accent.gamma_multiply(shown * t));
    }
    for (i, group) in groups.iter().enumerate() {
        let near = if sel.is_none() {
            0.0
        } else if from == to {
            f32::from(i == to)
        } else if i == to {
            lead
        } else if i == from {
            1.0 - lead
        } else {
            0.0
        } * shown;
        let ink = pal()
            .muted
            .lerp_to_gamma(pal().text, hot[i])
            .lerp_to_gamma(on_accent(accent), near)
            .gamma_multiply(t);
        fonts::paint_icon(
            &painter,
            cell_at(i).center(),
            group_icon(group),
            17.0,
            ink,
            false,
        );
    }
}

// ------------------------------------------------------------------ what can be added

/// One thing the "+" list offers.
pub(super) struct Entry {
    pub title: String,
    pub hint: &'static str,
    pub icon: char,
    pub kind: NodeKind,
}

fn entry(title: &str, hint: &'static str, kind: NodeKind) -> Entry {
    let (icon, _) = kind_info(&kind);
    Entry {
        title: title.to_string(),
        hint,
        icon,
        kind,
    }
}

/// A system action as an entry of its own: no list of operations to pick from afterwards.
fn native(op: NativeOp, hint: &'static str) -> Entry {
    let var = |is: bool| {
        if is {
            "result".to_string()
        } else {
            String::new()
        }
    };
    // a browser block starts with a command that works, so it is never empty
    let a = if op == NativeOp::Browser {
        "media.toggle".to_string()
    } else {
        var(op.a_is_variable())
    };
    let kind = NodeKind::Native {
        op,
        a,
        b: var(op.b_is_variable()),
    };
    entry(op.label(), hint, kind)
}

/// A browser command as an entry of its own; `reply` is the variable that gets the answer (empty:
/// none).
fn browser_entry(title: &'static str, hint: &'static str, command: &str, reply: &str) -> Entry {
    entry(
        title,
        hint,
        NodeKind::Native {
            op: NativeOp::Browser,
            a: command.to_string(),
            b: reply.to_string(),
        },
    )
}

/// A list operation as an entry of its own.
fn list_op(op: ListOp, hint: &'static str) -> Entry {
    let (_, _, ho) = op.hints();
    let kind = NodeKind::List {
        op,
        list: "list".to_string(),
        a: String::new(),
        b: String::new(),
        out: if ho.is_empty() {
            String::new()
        } else {
            "result".to_string()
        },
    };
    entry(op.label(), hint, kind)
}

/// Everything a step can be, in groups, each operation as one entry: the person picks *what it
/// does* in one go instead of a kind of block and then an operation from a drop-down.
pub(super) fn quick_catalog() -> Vec<(&'static str, Vec<Entry>)> {
    use NativeOp as N;
    let s = |t: &str| t.to_string();
    vec![
        (
            "Do something",
            vec![
                entry(
                    "Press a keyboard shortcut",
                    "Sends keys, e.g. Ctrl + C",
                    NodeKind::Key(Shortcut::new(key::F13, 0)),
                ),
                entry(
                    "Open or raise an app",
                    "Starts an app or brings it to the front",
                    NodeKind::OpenApp(String::new()),
                ),
                entry(
                    "Open a web page or a file",
                    "A link or a path",
                    NodeKind::OpenUrl(String::new()),
                ),
                entry(
                    "Run a shell command",
                    "Starts it and goes on without waiting",
                    NodeKind::Command(String::new()),
                ),
                entry("Wait", "Pause before the next step", NodeKind::Wait(500)),
                native(N::Notify, "A message on the screen"),
                native(N::ClipboardSet, "Puts a text on the clipboard"),
                native(N::TypeText, "Types a text as if on the keyboard"),
                native(N::Say, "Reads a text aloud"),
                native(N::ActivateApp, "Raises an app by its id"),
                native(N::OpenAppSmart, "Opens an app: go to it, or a new window"),
                native(
                    N::OpenUrlSmart,
                    "Opens a link: go to its tab, or a new window",
                ),
                native(N::Quiet, "Hides the notices of this run"),
                native(N::VolumeSet, "Sets the system volume"),
                native(N::Browser, "Controls the browser: play, tabs, chats"),
                native(N::Screenshot, "Saves the screen as a PNG"),
            ],
        ),
        (
            "Decide and repeat",
            vec![
                entry(
                    "If / else",
                    "Do one thing or another, depending on a check",
                    NodeKind::Branch(Cond::default()),
                ),
                entry(
                    "Repeat a number of times",
                    "Runs the steps inside again and again",
                    NodeKind::Repeat(3),
                ),
                entry(
                    "Repeat while",
                    "Keeps going as long as a check is true",
                    NodeKind::While(Cond::default()),
                ),
                entry(
                    "For each item of a list",
                    "Runs the steps inside once per item",
                    NodeKind::ForEach {
                        list: s("list"),
                        item: s("item"),
                    },
                ),
                entry(
                    "Run another flow",
                    "Starts a flow you made, then comes back",
                    NodeKind::Call(String::new()),
                ),
                entry("Stop", "Ends the whole run here", NodeKind::Stop),
            ],
        ),
        (
            "Browser",
            vec![
                browser_entry(
                    "Play / pause",
                    "Plays or pauses the player of the browser",
                    "media.toggle",
                    "",
                ),
                browser_entry(
                    "Next track",
                    "On YouTube, Spotify, SoundCloud...",
                    "media.next",
                    "",
                ),
                browser_entry(
                    "Previous track",
                    "Or back to the start of the track",
                    "media.prev",
                    "",
                ),
                browser_entry(
                    "Next tab",
                    "The tab to the right of this one",
                    "tab.next",
                    "",
                ),
                browser_entry(
                    "Is the music playing?",
                    "Puts playing, paused or none in a variable",
                    "media.state",
                    "state",
                ),
                browser_entry(
                    "Address of the tab",
                    "Puts the address of the tab in a variable",
                    "tab.url",
                    "address",
                ),
                native(N::Browser, "Any other command, chosen from the list"),
            ],
        ),
        (
            "Ask and run",
            vec![
                entry(
                    "Ask for a text",
                    "Shows a box to type in",
                    NodeKind::Ask {
                        mode: AskMode::Text,
                        message: String::new(),
                        preset: String::new(),
                        out: s("answer"),
                    },
                ),
                entry(
                    "Ask yes or no",
                    "Shows a question with two buttons",
                    NodeKind::Ask {
                        mode: AskMode::YesNo,
                        message: String::new(),
                        preset: String::new(),
                        out: s("answer"),
                    },
                ),
                entry(
                    "Show a panel",
                    "A small window with fields, lists and buttons",
                    NodeKind::Ask {
                        mode: AskMode::Panel,
                        message: PANEL_TEMPLATE.to_string(),
                        preset: String::new(),
                        out: s("button"),
                    },
                ),
                entry(
                    "Open a progress panel",
                    "A small window with a bar and a Stop button; the flow goes on",
                    NodeKind::PanelUpdate {
                        op: PanelOp::Open,
                        panel: String::new(),
                        title: String::new(),
                        label: s("Working"),
                        value: s("0"),
                        stop: true,
                    },
                ),
                entry(
                    "Update the progress panel",
                    "Changes its label and how far the bar is",
                    NodeKind::PanelUpdate {
                        op: PanelOp::Update,
                        panel: String::new(),
                        title: String::new(),
                        label: String::new(),
                        value: s("50%"),
                        stop: false,
                    },
                ),
                entry(
                    "Close the progress panel",
                    "Takes it off the screen",
                    NodeKind::PanelUpdate {
                        op: PanelOp::Close,
                        panel: String::new(),
                        title: String::new(),
                        label: String::new(),
                        value: String::new(),
                        stop: false,
                    },
                ),
                entry(
                    "Run a command and wait",
                    "Keeps what it prints in a variable",
                    NodeKind::Run {
                        command: String::new(),
                        out: s("output"),
                        timeout_s: 10,
                    },
                ),
            ],
        ),
        (
            "Variables",
            vec![
                entry(
                    "Set a variable",
                    "Puts a text in a variable",
                    NodeKind::Set {
                        name: s("x"),
                        value: s("1"),
                    },
                ),
                entry(
                    "Calculate",
                    "Add, subtract, multiply, divide...",
                    NodeKind::Calc {
                        out: s("x"),
                        a: s("x"),
                        op: CalcOp::Add,
                        b: s("1"),
                    },
                ),
                entry(
                    "Logic gate",
                    "And, or, not on true / false values",
                    NodeKind::Logic {
                        out: s("x"),
                        a: s("1"),
                        op: LogicOp::default(),
                        b: s("1"),
                    },
                ),
            ],
        ),
        (
            "Read from the system",
            vec![
                native(N::ClipboardGet, "The text on the clipboard"),
                native(N::FrontApp, "Which app is in front"),
                native(N::AppRunning, "Whether an app has a window"),
                native(N::CursorPos, "Where the pointer is"),
                native(N::VolumeGet, "The system volume"),
                native(N::SystemInfo, "System, user, host, folders"),
                native(N::Env, "An environment variable"),
                native(N::Random, "A random number"),
                native(N::UnixTime, "The time, in seconds"),
            ],
        ),
        (
            "Files and folders",
            vec![
                native(N::ReadFile, "Reads a text file"),
                native(N::WriteFile, "Writes a text file"),
                native(N::AppendFile, "Adds to the end of a file"),
                native(N::FileExists, "Whether a file or folder exists"),
                native(N::Reveal, "Shows it in the file manager"),
                native(N::ListDir, "The files of a folder, as a list"),
                native(N::MakeDir, "Makes a folder"),
                native(N::CopyFile, "Copies a file"),
                native(N::MoveFile, "Moves or renames"),
                native(N::DeleteFile, "Deletes a file"),
                native(N::FileSize, "Size in bytes"),
                native(N::FileTime, "When it last changed"),
            ],
        ),
        (
            "Lists",
            ListOp::ALL.iter().map(|op| list_op(*op, "")).collect(),
        ),
    ]
}

/// The "+" list: a search box and the groups under it. Returns what was picked.
pub(super) fn add_menu(ui: &mut egui::Ui, search: &mut String, can_stop: bool) -> Option<NodeKind> {
    add_menu_in(ui, search, can_stop, None)
}

/// The "+" list, narrowed to the group with this index of [`quick_catalog`] when `only` is set.
fn add_menu_in(
    ui: &mut egui::Ui,
    search: &mut String,
    can_stop: bool,
    only: Option<usize>,
) -> Option<NodeKind> {
    let mut picked = None;
    ui.set_width(MENU_W);
    ui.spacing_mut().item_spacing.y = 2.0;
    let r = ui.add(
        text_field(search)
            .desired_width(ui.available_width())
            .hint_text("Search: copy, notify, loop\u{2026}"),
    );
    if !r.has_focus() && search.is_empty() {
        r.request_focus();
    }
    let q = search.trim().to_lowercase();
    let mut first = None;
    egui::ScrollArea::vertical()
        .max_height(MENU_H)
        .show(ui, |ui| {
            for (gi, (group, entries)) in quick_catalog().into_iter().enumerate() {
                if only.is_some_and(|o| o != gi) {
                    continue;
                }
                let shown: Vec<&Entry> = entries
                    .iter()
                    .filter(|e| can_stop || !matches!(e.kind, NodeKind::Stop))
                    .filter(|e| {
                        q.is_empty()
                            || e.title.to_lowercase().contains(&q)
                            || e.hint.to_lowercase().contains(&q)
                            || group.to_lowercase().contains(&q)
                    })
                    .collect();
                if shown.is_empty() {
                    continue;
                }
                ui.add_space(6.0);
                ui.label(
                    RichText::new(group.to_uppercase())
                        .size(11.0)
                        .color(pal().muted),
                );
                for e in shown {
                    let resp = select_item(ui, Some(e.icon), &e.title, false);
                    let resp = if e.hint.is_empty() {
                        resp
                    } else {
                        resp.on_hover_text(e.hint)
                    };
                    if first.is_none() {
                        first = Some(e.kind.clone());
                    }
                    if resp.clicked() {
                        picked = Some(e.kind.clone());
                    }
                }
            }
            if first.is_none() {
                ui.add_space(8.0);
                ui.label(RichText::new("Nothing like that. Try another word.").color(pal().muted));
            }
        });
    if picked.is_none() && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        picked = first;
    }
    picked
}

/// The "+" list as a popup over `trigger`, centered on it (the floating bars are at the bottom of
/// the page; a click on `trigger` opens and closes it). While it is open the capsule shows a button
/// per category (see [`category_strip`]), so a click on one of those must not close the list: the
/// popup ignores clicks and this closes it itself for a click outside both the list and the capsule.
fn quick_popup(
    trigger: &egui::Response,
    search: &mut String,
    bar: &'static str,
) -> Option<NodeKind> {
    let ctx = trigger.ctx.clone();
    if trigger.clicked() {
        search.clear();
        ctx.data_mut(|d| d.insert_temp(group_key(bar), None::<usize>));
        // the buttons of the categories come into the capsule on the next frame
        ctx.request_repaint();
    }
    let only = picked_group(&ctx, bar);
    let mut picked = None;
    let shown = egui::Popup::from_toggle_button_response(trigger)
        .id(menu_id(bar))
        .close_behavior(egui::PopupCloseBehavior::IgnoreClicks)
        .align(egui::RectAlign::TOP)
        .gap(10.0)
        .frame(popup_frame().inner_margin(egui::Margin::same(8)))
        .show(|ui| picked = add_menu_in(ui, search, true, only));
    if let Some(shown) = shown
        && ctx.input(|i| i.pointer.primary_clicked())
    {
        let at = ctx.input(|i| i.pointer.interact_pos());
        let inside =
            at.is_some_and(|p| shown.response.rect.contains(p) || trigger.rect.contains(p));
        if !inside {
            egui::Popup::close_id(&ctx, menu_id(bar));
            ctx.request_repaint();
        }
    }
    if picked.is_some() {
        egui::Popup::close_all(&ctx);
        ctx.request_repaint();
    }
    picked
}

// ------------------------------------------------------------------ the steps

/// What the list asked for this frame. Applied after drawing, so the flow is not changed while it
/// is being walked.
enum Op {
    Insert(Spot, NodeKind),
    Remove(u32),
    Move(u32, bool),
    Record(u32),
    PickApp(u32),
    /// Open the panel designer for this *Ask* block
    DesignPanel(u32),
    DropLoose,
}

struct Cx<'a> {
    flow: &'a mut Flow,
    ed: &'a mut Ed,
    accent: Color32,
    fi: usize,
    others: &'a [(String, String)],
    issues: Vec<crate::config::Issue>,
    marks: Vec<(u32, String)>,
    ops: Vec<Op>,
    n: usize,
    width: f32,
}

/// The flow as a list of steps.
pub(super) fn steps(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    flow: &mut Flow,
    ed: &mut Ed,
    accent: Color32,
    others: &[(String, String)],
    tx: &Sender<Msg>,
) {
    let fi = ed.flow;
    let issues = {
        let known: Vec<&str> = others
            .iter()
            .map(|(id, _)| id.as_str())
            .chain(std::iter::once(flow.id.as_str()))
            .collect();
        check_flow(flow, &known)
    };
    let marks = ed.flow_marks.get(&flow.id).cloned().unwrap_or_default();
    let list = outline::outline(flow);
    let area = ui.available_rect_before_wrap();
    let mut cx = Cx {
        flow,
        ed,
        accent,
        fi,
        others,
        issues,
        marks,
        ops: Vec::new(),
        n: 0,
        width: area.width().min(CARD_MAX_W),
    };
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let left = ((area.width() - cx.width) / 2.0).max(0.0);
            let page = Rect::from_min_size(
                pos2(area.min.x + left, ui.cursor().min.y),
                vec2(cx.width, area.height()),
            );
            ui.scope_builder(UiBuilder::new().max_rect(page), |ui| {
                ui.set_width(cx.width);
                match &list {
                    Some(o) => draw_outline(ui, &mut cx, o),
                    None => free_wiring(ui, &mut cx),
                }
            });
        });

    let Cx { flow, ed, ops, .. } = cx;
    for op in ops {
        match op {
            Op::Insert(spot, kind) => {
                if outline::insert_block(flow, spot, kind).is_some() {
                    ed.flow_untidy.insert(flow.id.clone());
                } else if flow.nodes.len() >= MAX_FLOW_NODES {
                    let _ = tx.send(Msg::Notice(format!(
                        "A flow can have {MAX_FLOW_NODES} blocks at most."
                    )));
                    ctx.request_repaint();
                }
            }
            Op::Remove(id) => {
                if outline::remove_block(flow, id) {
                    ed.flow_untidy.insert(flow.id.clone());
                    ed.stop_recording();
                    notice(tx, ctx, "Step removed. Undo brings it back.");
                }
            }
            Op::Move(id, up) => {
                if outline::move_block(flow, id, up) {
                    ed.flow_untidy.insert(flow.id.clone());
                }
            }
            Op::Record(id) => toggle_recording(ed, fi, id),
            Op::PickApp(id) => super::flow::pick_app(ctx, ed, tx, fi, id),
            Op::DesignPanel(id) => crate::ui::settings::panel_designer::open(ed, fi, flow, id),
            Op::DropLoose => {
                if let Some(o) = outline::outline(flow) {
                    for id in o.loose {
                        flow.remove_node(id);
                    }
                }
            }
        }
    }
}

/// What to say when the wires are not a list.
fn free_wiring(ui: &mut egui::Ui, cx: &mut Cx) {
    card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(fonts::title("This flow is wired freely", 15.0, pal().text));
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                RichText::new(
                    "Some wires jump back or sideways, or an output is not plugged in, so the flow \
                     is not one step after the other and cannot be shown as a list without \
                     changing what it does. Nothing is wrong with it: it still runs. Open the \
                     canvas to see and edit its wires.",
                )
                .size(13.0)
                .color(pal().muted),
            )
            .wrap(),
        );
        ui.add_space(10.0);
        if button(
            ui,
            Look::Primary(cx.accent),
            Some(symbols::SELECTION_ALL),
            "Open the canvas",
        )
        .clicked()
        {
            cx.ed.flow_view = FlowView::Canvas;
        }
    });
}

/// The whole list: *Start*, the steps, *End*, and the blocks nothing leads to.
fn draw_outline(ui: &mut egui::Ui, cx: &mut Cx, o: &Outline) {
    ui.spacing_mut().item_spacing.y = 0.0;
    marker(ui, symbols::PLAY, "When the slice is used", cx.accent);
    let ends = outline::list_ends(cx.flow, &o.items);
    draw_list(ui, cx, &o.items, None, ends);
    if !ends {
        marker(ui, symbols::FLAG, "Done", pal().muted);
    }
    if o.items.is_empty() {
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new("Nothing yet: press + to add the first step.")
                    .size(13.0)
                    .color(pal().muted),
            )
            .wrap(),
        );
    }
    if !o.loose.is_empty() {
        ui.add_space(24.0);
        loose(ui, cx, &o.loose);
    }
    // room under the last step for the floating "+"
    ui.add_space(84.0);
}

/// *Start* or *End*: a small pill that frames the list.
fn marker(ui: &mut egui::Ui, icon: char, text: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let p = ui.painter();
    let c = pos2(rect.min.x + 14.0, rect.center().y);
    p.circle_filled(c, 13.0, wash(color, 0.14));
    fonts::paint_icon(p, c, icon, 15.0, color, true);
    p.text(
        pos2(rect.min.x + 36.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.0),
        pal().muted,
    );
}

/// The blocks that no wire leads to.
fn loose(ui: &mut egui::Ui, cx: &mut Cx, ids: &[u32]) {
    card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(fonts::title("Not connected", 14.0, AMBER));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if button(ui, Look::Danger, Some(symbols::TRASH), "Remove them").clicked() {
                    cx.ops.push(Op::DropLoose);
                }
            });
        });
        ui.add(
            egui::Label::new(
                RichText::new(
                    "No wire leads to these blocks, so they never run. Remove them here, or open \
                     the canvas to wire them in.",
                )
                .size(13.0)
                .color(pal().muted),
            )
            .wrap(),
        );
        ui.add_space(6.0);
        for id in ids {
            if let Some(n) = cx.flow.node(*id) {
                let (icon, title) = title_of(&n.kind);
                ui.horizontal(|ui| {
                    ui.label(fonts::icon(icon, 16.0, pal().muted));
                    ui.label(RichText::new(title).color(pal().muted));
                });
            }
        }
    });
}

/// A list of steps with a "+" before the first, between every two and after the last.
fn draw_list(
    ui: &mut egui::Ui,
    cx: &mut Cx,
    items: &[Item],
    parent: Option<(u32, Arm)>,
    ends: bool,
) {
    // a list that ends in a Stop takes no step after it
    plus(ui, cx, Spot { parent, index: 0 }, !ends && items.is_empty());
    for (i, it) in items.iter().enumerate() {
        let last = i + 1 == items.len();
        draw_item(ui, cx, it, parent, i, items.len());
        let can = outline::can_follow(cx.flow, it);
        if can {
            plus(
                ui,
                cx,
                Spot {
                    parent,
                    index: i + 1,
                },
                last && !ends,
            );
        }
    }
}

/// The round "+" between two steps, on a short line that joins them, and the list it opens.
fn plus(ui: &mut egui::Ui, cx: &mut Cx, spot: Spot, at_end: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), GAP_H), Sense::hover());
    let c = pos2(rect.min.x + 14.0, rect.center().y);
    let line = Stroke::new(2.0, wash(pal().text, 0.12));
    ui.painter()
        .line_segment([pos2(c.x, rect.min.y), pos2(c.x, rect.max.y)], line);
    let hit = Rect::from_center_size(c, Vec2::splat(24.0));
    let id = ui.id().with(("plus", spot.parent, spot.index));
    let resp = ui.interact(hit, id, Sense::click());
    let on = resp.hovered() || egui::Popup::is_id_open(ui.ctx(), id.with("popup"));
    let fill = if on { cx.accent } else { pal().card };
    let ink = if on { Color32::WHITE } else { pal().muted };
    ui.painter().circle_filled(c, 11.0, fill);
    ui.painter().circle_stroke(
        c,
        11.0,
        Stroke::new(1.0, if on { cx.accent } else { pal().border }),
    );
    fonts::paint_icon(ui.painter(), c, symbols::PLUS, 14.0, ink, true);
    let resp = resp
        .on_hover_text("Add a step here")
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if at_end {
        // the last "+" says what it is for
        ui.painter().text(
            pos2(c.x + 22.0, c.y),
            egui::Align2::LEFT_CENTER,
            "Add a step",
            FontId::proportional(13.0),
            pal().muted,
        );
    }
    if resp.clicked() {
        cx.ed.flow_search.clear();
    }
    let can_stop = at_end || spot.index != usize::MAX;
    let full = cx.flow.nodes.len() >= MAX_FLOW_NODES;
    if !full {
        let search = &mut cx.ed.flow_search;
        let mut picked = None;
        egui::Popup::from_toggle_button_response(&resp)
            .id(id.with("popup"))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .align(egui::RectAlign::RIGHT_START)
            .gap(8.0)
            .frame(popup_frame().inner_margin(egui::Margin::same(8)))
            .show(|ui| picked = add_menu(ui, search, can_stop));
        if let Some(kind) = picked {
            egui::Popup::close_id(ui.ctx(), id.with("popup"));
            cx.ops.push(Op::Insert(spot, kind));
        }
    }
}

/// The icon and the name a step has in the list: what it does, not which kind of block it is.
fn title_of(kind: &NodeKind) -> (char, String) {
    let (icon, title) = kind_info(kind);
    match kind {
        NodeKind::Native { op, .. } => (icon, op.label().to_string()),
        NodeKind::List { op, .. } => (icon, format!("List: {}", op.label().to_lowercase())),
        NodeKind::Ask { mode, .. } => (
            icon,
            match mode {
                AskMode::Text => "Ask for a text".to_string(),
                AskMode::YesNo => "Ask yes or no".to_string(),
                AskMode::Panel => "Show a panel".to_string(),
            },
        ),
        NodeKind::Key(_) => (icon, "Press a shortcut".to_string()),
        NodeKind::OpenApp(_) => (icon, "Open an app".to_string()),
        NodeKind::OpenUrl(_) => (icon, "Open a page or file".to_string()),
        NodeKind::Command(_) => (icon, "Run a shell command".to_string()),
        NodeKind::PanelUpdate { op, .. } => (
            icon,
            match op {
                PanelOp::Open => "Open a progress panel".to_string(),
                PanelOp::Update => "Update the progress panel".to_string(),
                PanelOp::Close => "Close the progress panel".to_string(),
            },
        ),
        NodeKind::Run { .. } => (icon, "Run a command and wait".to_string()),
        NodeKind::Branch(_) => (icon, "If".to_string()),
        NodeKind::Repeat(_) => (icon, "Repeat".to_string()),
        NodeKind::While(_) => (icon, "Repeat while".to_string()),
        NodeKind::ForEach { .. } => (icon, "For each item".to_string()),
        NodeKind::Call(_) => (icon, "Run another flow".to_string()),
        NodeKind::Set { .. } => (icon, "Set a variable".to_string()),
        _ => (icon, title.to_string()),
    }
}

/// One step: its card and, for an *If* or a loop, the steps inside it.
fn draw_item(
    ui: &mut egui::Ui,
    cx: &mut Cx,
    it: &Item,
    parent: Option<(u32, Arm)>,
    index: usize,
    count: usize,
) {
    let id = it.id();
    cx.n += 1;
    let number = cx.n;
    let first = index == 0 && parent.is_none();
    let last = index + 1 == count && parent.is_none();
    card_of(ui, cx, id, number, first, last);
    let inner = |ui: &mut egui::Ui, cx: &mut Cx, label: &str, arm: Arm, items: &[Item]| {
        // a thin line down the left shows that these steps belong to the card above
        let top = ui.cursor().min;
        ui.horizontal(|ui| {
            ui.add_space(INDENT);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    RichText::new(label.to_uppercase())
                        .size(11.0)
                        .color(pal().muted),
                );
                let ends = outline::list_ends(cx.flow, items);
                draw_list(ui, cx, items, Some((id, arm)), ends);
            });
        });
        let bottom = ui.cursor().min.y;
        let x = top.x + 14.0;
        ui.painter().line_segment(
            [pos2(x, top.y), pos2(x, bottom)],
            Stroke::new(2.0, wash(cx.accent, 0.35)),
        );
    };
    match it {
        Item::Step(_) => {}
        Item::Branch {
            then, otherwise, ..
        } => {
            inner(ui, cx, "If true", Arm::Then, then);
            inner(ui, cx, "Otherwise", Arm::Else, otherwise);
        }
        Item::Loop { body, .. } => inner(ui, cx, "Do this each time", Arm::Body, body),
    }
}

/// The card of one step: number, icon, name, move / remove, the fields and what is wrong.
fn card_of(ui: &mut egui::Ui, cx: &mut Cx, id: u32, number: usize, first: bool, last: bool) {
    let Some(pos) = cx.flow.nodes.iter().position(|n| n.id == id) else {
        return;
    };
    let kind_now = cx.flow.nodes[pos].kind.clone();
    let (icon, title) = title_of(&kind_now);
    let color = tint(&kind_now, cx.accent);
    let sev = block_issues(&cx.issues, id).first().map(|i| i.severity);
    let border = match sev {
        Some(Severity::Error) => pal().bad,
        Some(Severity::Warning) => AMBER,
        None => pal().border,
    };
    let recording = cx
        .ed
        .recording
        .as_ref()
        .is_some_and(|r| r.profile == cx.fi && r.slot == Slot::Node(id));
    let messages: Vec<(Severity, String)> = block_issues(&cx.issues, id)
        .iter()
        .map(|i| (i.severity, i.message.clone()))
        .collect();
    let failed: Vec<String> = cx
        .marks
        .iter()
        .filter(|(b, _)| *b == id)
        .map(|(_, m)| m.clone())
        .collect();
    let accent = cx.accent;
    let others = cx.others;
    egui::Frame::new()
        .fill(pal().card)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CONTROL_R)
        .inner_margin(egui::Margin::symmetric(14, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::hover());
                ui.painter()
                    .circle_filled(r.center(), 13.0, wash(color, 0.16));
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    number.to_string(),
                    FontId::proportional(12.5),
                    color,
                );
                ui.label(fonts::icon(icon, 18.0, color));
                ui.add(egui::Label::new(fonts::title(&title, 15.0, pal().text)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if icon_button_look(ui, Look::Danger, symbols::X, "Remove this step", true)
                        .clicked()
                    {
                        cx.ops.push(Op::Remove(id));
                    }
                    if icon_button_look(ui, Look::Plain, symbols::ARROW_DOWN, "Move down", !last)
                        .clicked()
                    {
                        cx.ops.push(Op::Move(id, false));
                    }
                    if icon_button_look(ui, Look::Plain, symbols::ARROW_UP, "Move up", !first)
                        .clicked()
                    {
                        cx.ops.push(Op::Move(id, true));
                    }
                });
            });
            let mut ops = Vec::new();
            fields(
                ui,
                &mut cx.flow.nodes[pos].kind,
                others,
                recording,
                accent,
                id,
                &mut ops,
            );
            cx.ops.extend(ops);
            for (sev, msg) in &messages {
                note_line(ui, *sev == Severity::Error, msg);
            }
            for msg in &failed {
                note_line(ui, true, &format!("Failed in the last run: {msg}"));
            }
        });
}

/// A line under a card that says what is wrong, with a sign.
fn note_line(ui: &mut egui::Ui, error: bool, text: &str) {
    let color = if error { pal().bad } else { AMBER };
    ui.horizontal_top(|ui| {
        ui.label(fonts::icon(symbols::WARNING, 14.0, color));
        ui.add(egui::Label::new(RichText::new(text).size(12.5).color(color)).wrap());
    });
}

// ------------------------------------------------------------------ the fields of a step

/// A row: the name of the field on the left, the field on the right.
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (r, _) = ui.allocate_exact_size(vec2(LABEL_W, CONTROL_H), Sense::hover());
        ui.painter().text(
            pos2(r.min.x, r.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            pal().muted,
        );
        add(ui);
    });
}

/// A text row. A field that starts with a capital in `hint` ("Result") is a name; the rest is
/// shown as the hint inside the field.
fn text_row(ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str) {
    row(ui, label, |ui| field(ui, text, hint));
}

/// The first letter in capitals.
fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

/// What a field of a system action is called: the hint, but a variable that gets a result says
/// "Save in".
fn field_label(hint: &str, is_var: bool) -> String {
    if is_var {
        "Save in".to_string()
    } else {
        capital(
            hint.split(',')
                .next()
                .unwrap_or(hint)
                .split(" (")
                .next()
                .unwrap_or(hint),
        )
    }
}

/// The fields of a step, each with its name. The kind of an action (which system action, which
/// list operation) was picked when the step was added, so there is no list of kinds here.
fn fields(
    ui: &mut egui::Ui,
    kind: &mut NodeKind,
    others: &[(String, String)],
    recording: bool,
    accent: Color32,
    id: u32,
    ops: &mut Vec<Op>,
) {
    match kind {
        NodeKind::Key(sc) => row(ui, "Keys", |ui| {
            if shortcut_button(ui, sc, recording, accent).clicked() {
                ops.push(Op::Record(id));
            }
        }),
        NodeKind::OpenApp(s) => row(ui, "App", |ui| {
            let w = (ui.available_width() - 90.0).max(60.0);
            ui.add(
                text_field(s)
                    .desired_width(w)
                    .hint_text(crate::platform::OS.open_app_hint()),
            );
            if button(ui, Look::Soft, Some(symbols::APP_WINDOW), "Pick\u{2026}")
                .on_hover_text("Choose from the installed apps")
                .clicked()
            {
                ops.push(Op::PickApp(id));
            }
        }),
        NodeKind::OpenUrl(s) => text_row(ui, "Address", s, "https://\u{2026}  or a file path"),
        NodeKind::Command(s) => text_row(ui, "Command", s, "shell command (not waited for)"),
        NodeKind::Wait(ms) => row(ui, "Wait for", |ui| {
            ui.add(
                egui::DragValue::new(ms)
                    .range(0..=MAX_WAIT_MS)
                    .speed(10.0)
                    .suffix(" ms"),
            );
        }),
        NodeKind::Set { name, value } => {
            text_row(ui, "Variable", name, "name, e.g. count");
            text_row(ui, "Value", value, "text, e.g. {{name}}");
        }
        NodeKind::Calc { out, a, op, b } => {
            text_row(ui, "Save in", out, "variable");
            row(ui, "Calculate", |ui| {
                operands_row(ui, a, &CalcOp::ALL, CalcOp::label, op, false, b);
            });
        }
        NodeKind::Logic { out, a, op, b } => {
            text_row(ui, "Save in", out, "variable (1 or 0)");
            let unary = op.is_unary();
            row(ui, "Check", |ui| {
                operands_row(ui, a, &LogicOp::ALL, LogicOp::label, op, unary, b);
            });
        }
        NodeKind::Branch(c) => row(ui, "If", |ui| cond_row(ui, c)),
        NodeKind::While(c) => row(ui, "While", |ui| cond_row(ui, c)),
        NodeKind::Repeat(n) => row(ui, "Repeat", |ui| {
            ui.add(
                egui::DragValue::new(n)
                    .range(0..=MAX_REPEAT)
                    .speed(0.2)
                    .suffix(" times"),
            );
        }),
        NodeKind::ForEach { list, item } => {
            text_row(ui, "List", list, "list variable");
            text_row(ui, "Each item in", item, "variable");
        }
        NodeKind::Ask {
            mode,
            message,
            preset,
            out,
        } => {
            if matches!(mode, AskMode::Panel) {
                row(ui, "Panel", |ui| {
                    if button(
                        ui,
                        Look::Primary(accent),
                        Some(symbols::PENCIL_SIMPLE),
                        "Design\u{2026}",
                    )
                    .on_hover_text("Build the panel with the mouse and see it as it will look")
                    .clicked()
                    {
                        ops.push(Op::DesignPanel(id));
                    }
                });
                row(ui, "Panel (JSON)", |ui| {
                    ui.add(
                        egui::TextEdit::multiline(message)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(8)
                            .desired_width(f32::INFINITY)
                            .hint_text("see docs/PANELS.md"),
                    );
                });
                hint_line(
                    ui,
                    "Each field sets the variable named by its id. The button pressed goes below.",
                );
                text_row(ui, "Save button in", out, "variable");
            } else {
                text_row(ui, "Question", message, "what to ask");
                if matches!(mode, AskMode::Text) {
                    text_row(ui, "Starts with", preset, "optional");
                }
                text_row(ui, "Save answer in", out, "variable");
            }
        }
        NodeKind::PanelUpdate {
            op,
            panel,
            title,
            label,
            value,
            stop,
        } => {
            row(ui, "Action", |ui| {
                let w = ui.available_width();
                pick(ui, w, &PanelOp::ALL, PanelOp::label, op);
            });
            text_row(ui, "Panel id", panel, DEFAULT_PROGRESS_PANEL);
            if op.has_fields() {
                let open = *op == PanelOp::Open;
                text_row(
                    ui,
                    "Title",
                    title,
                    if open {
                        "the name of the flow"
                    } else {
                        "empty keeps it"
                    },
                );
                text_row(ui, "Label", label, "above the bar, e.g. File {{index}}");
                text_row(ui, "Bar", value, "0 to 1, 40%, busy, or empty to keep");
                if open {
                    row(ui, "Stop button", |ui| {
                        ui.checkbox(stop, "Pressing it ends the run");
                    });
                }
                hint_line(
                    ui,
                    "Nothing waits for the panel. It goes away when the run ends, or at a Close block.",
                );
            }
        }
        NodeKind::Run {
            command,
            out,
            timeout_s,
        } => {
            text_row(ui, "Command", command, "shell command (waited for)");
            text_row(ui, "Save output in", out, "variable");
            row(ui, "Give up after", |ui| {
                ui.add(
                    egui::DragValue::new(timeout_s)
                        .range(1..=MAX_RUN_SECS)
                        .suffix(" s"),
                );
            });
        }
        NodeKind::Native { op, a, b } if *op == NativeOp::Browser => {
            row(ui, "Action", |ui| {
                let w = ui.available_width();
                pick(ui, w, &NativeOp::ALL, NativeOp::label, op);
            });
            if browser_by_variable(a) {
                text_row(ui, "Command", a, "command, e.g. media.next");
            } else {
                row(ui, "Command", |ui| browser_fields(ui, a));
            }
            text_row(ui, "Save reply in", b, "variable (optional)");
            let (line, good) = bridge_line(
                crate::config::cfg().browser_bridge,
                &crate::bridge::status(),
            );
            ui.label(
                RichText::new(line)
                    .size(12.5)
                    .color(if good { pal().muted } else { AMBER }),
            );
            hint_line(
                ui,
                "The block waits for the browser. `ok` says how it went; the reply is also what a question answers (tab.url, media.state).",
            );
        }
        NodeKind::Native { op, a, b } => {
            let (ha, hb) = op.hints();
            if !ha.is_empty() {
                text_row(ui, &field_label(ha, op.a_is_variable()), a, ha);
            }
            if !hb.is_empty() {
                text_row(ui, &field_label(hb, op.b_is_variable()), b, hb);
            }
            if ha.is_empty() && hb.is_empty() {
                hint_line(ui, "Nothing to fill in.");
            }
        }
        NodeKind::List {
            op,
            list,
            a,
            b,
            out,
        } => {
            text_row(ui, "List", list, "list variable");
            let (ha, hb, ho) = op.hints();
            if !ha.is_empty() {
                text_row(ui, &field_label(ha, false), a, ha);
            }
            if !hb.is_empty() {
                text_row(ui, &field_label(hb, false), b, hb);
            }
            if !ho.is_empty() {
                text_row(ui, "Save in", out, ho);
            }
        }
        NodeKind::Call(s) => row(ui, "Flow", |ui| {
            let labels: Vec<&str> = others.iter().map(|(_, n)| n.as_str()).collect();
            let at = others.iter().position(|(i, _)| i == s);
            if labels.is_empty() {
                ui.label(
                    RichText::new("No other flow yet: make one in the list on the left.")
                        .size(12.5)
                        .color(pal().muted),
                );
            } else {
                let current = at
                    .and_then(|a| labels.get(a).copied())
                    .unwrap_or("Choose a flow");
                let w = ui.available_width();
                if let Some(i) = select(ui, w, current, &labels, at.unwrap_or(0))
                    && let Some((fid, _)) = others.get(i)
                {
                    *s = fid.clone();
                }
            }
        }),
        NodeKind::Stop => hint_line(ui, "The run ends here. Nothing after it can happen."),
        NodeKind::Start | NodeKind::End => {}
    }
}

fn hint_line(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.5).color(pal().muted));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_add_list_has_every_operation_once_and_every_kind_of_step() {
        let all: Vec<Entry> = quick_catalog().into_iter().flat_map(|(_, e)| e).collect();
        for op in NativeOp::ALL {
            let n = all
                .iter()
                .filter(|e| matches!(&e.kind, NodeKind::Native { op: o, .. } if *o == op))
                .count();
            if op == NativeOp::Browser {
                // the common commands are entries of their own, and the one that picks any
                assert!(n >= 2, "the Browser group has several entries");
                assert!(
                    all.iter().any(|e| matches!(&e.kind,
                        NodeKind::Native { op: NativeOp::Browser, b, .. } if b == "state")),
                    "a question for a flow"
                );
            } else {
                assert_eq!(n, 1, "{op:?}");
            }
        }
        for op in ListOp::ALL {
            let n = all
                .iter()
                .filter(|e| matches!(&e.kind, NodeKind::List { op: o, .. } if *o == op))
                .count();
            assert_eq!(n, 1, "{op:?}");
        }
        let has = |f: fn(&NodeKind) -> bool| all.iter().any(|e| f(&e.kind));
        assert!(has(|k| matches!(k, NodeKind::Key(_))));
        assert!(has(|k| matches!(k, NodeKind::OpenApp(_))));
        assert!(has(|k| matches!(k, NodeKind::OpenUrl(_))));
        assert!(has(|k| matches!(k, NodeKind::Command(_))));
        assert!(has(|k| matches!(k, NodeKind::Wait(_))));
        assert!(has(|k| matches!(k, NodeKind::Set { .. })));
        assert!(has(|k| matches!(k, NodeKind::Calc { .. })));
        assert!(has(|k| matches!(k, NodeKind::Logic { .. })));
        assert!(has(|k| matches!(k, NodeKind::Branch(_))));
        assert!(has(|k| matches!(k, NodeKind::Repeat(_))));
        assert!(has(|k| matches!(k, NodeKind::While(_))));
        assert!(has(|k| matches!(k, NodeKind::ForEach { .. })));
        assert!(has(|k| matches!(
            k,
            NodeKind::Ask {
                mode: AskMode::Text,
                ..
            }
        )));
        assert!(has(|k| matches!(
            k,
            NodeKind::Ask {
                mode: AskMode::YesNo,
                ..
            }
        )));
        assert!(has(|k| matches!(k, NodeKind::Run { .. })));
        assert!(has(|k| matches!(k, NodeKind::Call(_))));
        assert!(has(|k| matches!(k, NodeKind::Stop)));
        assert!(!has(|k| matches!(k, NodeKind::Start | NodeKind::End)));
    }

    #[test]
    fn every_entry_can_be_added_to_a_list_and_the_flow_stays_clean() {
        for e in quick_catalog().into_iter().flat_map(|(_, e)| e) {
            let mut f = Flow::new("f", "F");
            let first = outline::insert_block(&mut f, Spot::END, e.kind.clone());
            assert!(first.is_some(), "{}", e.title);
            let mut clean = f.clone();
            clean.clean();
            assert_eq!(clean, f, "{}", e.title);
        }
    }

    #[test]
    fn a_field_is_named_after_what_it_holds() {
        assert_eq!(field_label("variable", true), "Save in");
        assert_eq!(field_label("file path", false), "File path");
        assert_eq!(field_label("name, e.g. HOME", false), "Name");
        assert_eq!(field_label("variable (0: not there)", true), "Save in");
        assert_eq!(
            field_label("position (1 = first, -1 = last)", false),
            "Position"
        );
    }

    fn draw(app: &mut crate::ui::app::App, frames: usize) {
        use crate::ui::app::{CARD_SIZE, WIN_MARGIN};
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        for _ in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    pos2(0.0, 0.0),
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

    fn app_with(flow: Flow, view: FlowView) -> crate::ui::app::App {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = crate::ui::app::App::new(rx, tx, 200.0, None);
        app.settings_open = true;
        app.edit = Config {
            flows: vec![flow],
            ..Config::default()
        };
        app.ed.tab = crate::ui::settings::Tab::Flow;
        app.ed.flow_view = view;
        app
    }

    /// A flow with every kind of step, an If and loops with steps inside.
    fn big_flow() -> Flow {
        let mut f = Flow::new("flow-1", "Demo");
        for e in quick_catalog().into_iter().flat_map(|(_, e)| e) {
            if matches!(e.kind, NodeKind::Stop) {
                continue;
            }
            let id = outline::insert_block(&mut f, Spot::END, e.kind.clone()).unwrap();
            if matches!(e.kind, NodeKind::Branch(_)) {
                outline::insert_block(
                    &mut f,
                    Spot {
                        parent: Some((id, Arm::Then)),
                        index: 0,
                    },
                    NodeKind::Wait(5),
                );
                outline::insert_block(
                    &mut f,
                    Spot {
                        parent: Some((id, Arm::Else)),
                        index: 0,
                    },
                    NodeKind::Stop,
                );
            }
            if matches!(e.kind, NodeKind::Repeat(_)) {
                outline::insert_block(
                    &mut f,
                    Spot {
                        parent: Some((id, Arm::Body)),
                        index: 0,
                    },
                    NodeKind::Wait(5),
                );
            }
        }
        f
    }

    #[test]
    fn the_list_draws_every_kind_of_step_and_the_canvas_still_draws_it_without_changing_it() {
        for view in [FlowView::Steps, FlowView::Canvas] {
            let flow = big_flow();
            assert!(outline::outline(&flow).is_some());
            let mut app = app_with(flow, view);
            let before = app.edit.clone();
            draw(&mut app, 3);
            // the canvas lines up blocks that were added from the list: only places change
            let mut after = app.edit.clone();
            for (a, b) in after.flows[0].nodes.iter_mut().zip(&before.flows[0].nodes) {
                a.x = b.x;
                a.y = b.y;
            }
            assert_eq!(after, before, "{view:?}");
        }
    }

    #[test]
    fn a_freely_wired_flow_shows_the_notice_instead_of_a_list() {
        let mut f = Flow::new("flow-1", "Demo");
        let a = outline::insert_block(&mut f, Spot::END, NodeKind::Wait(1)).unwrap();
        let b = outline::insert_block(&mut f, Spot::END, NodeKind::Wait(2)).unwrap();
        f.connect(b, Port::Next, a);
        assert!(outline::outline(&f).is_none());
        let mut app = app_with(f, FlowView::Steps);
        let before = app.edit.clone();
        draw(&mut app, 2);
        assert_eq!(app.edit, before);
    }

    #[test]
    fn an_empty_flow_draws() {
        let mut app = app_with(Flow::new("flow-1", "Demo"), FlowView::Steps);
        draw(&mut app, 2);
    }
}
