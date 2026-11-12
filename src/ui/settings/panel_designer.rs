//! The **panel designer**: a sheet where the panel of an *Ask* block (mode *Panel*) is built with
//! the mouse instead of typed as JSON.
//!
//! ```text
//! +-----------+----------------------+-------------+
//! | Add  Templ|                      | Inspector   |
//! | Structure |   the real panel     |  (what is   |
//! | + Buttons |   (click to select)  |   selected) |
//! +-----------+----------------------+-------------+
//! ```
//!
//! * The middle is not a mock-up: it is [`panel_form::preview`], the code that draws the panel in a
//!   flow, so what is designed is what is shown. Its widgets work (type, switch, drag), and a click
//!   on one selects it; widgets that a `show_if` would hide are drawn anyway, so they can be
//!   selected.
//! * Every change is written back to the block at once, so Settings' undo and redo cover it. When
//!   the block changes behind the designer's back (undo, the JSON box) it reads the block again.
//! * The rules (what a new widget starts as, moving, renaming without breaking a `show_if`, the
//!   templates) are in [`crate::panel::design`] and have tests; this file only draws.
//!
//! A text that is not a panel at all (broken JSON) is left alone until the person picks a template.

use super::panel_form;
use super::state::Ed;
use super::theme::*;
use super::widgets::*;
use crate::config::Config;
use crate::config::flow::{AskMode, Flow, NodeKind};
use crate::panel::design::{self, Kind};
use crate::panel::layout::{self, card_size, header_height};
use crate::panel::spec::{
    DEFAULT_TIMEOUT_S, HEIGHT_MAX, HEIGHT_MIN, MAX_GROUP_BUTTONS, MAX_TIMEOUT_S, SPACER_DEFAULT,
    SPACER_MAX, SPACER_MIN, WIDTH_DEFAULT, WIDTH_MAX, WIDTH_MIN, default_buttons, format_hex,
    parse_hex,
};
use crate::panel::{
    Button, ButtonAlign, Form, Icon, Layout as ListLayout, Opt, PanelSpec, Place, ShowIf,
    TextStyle as Style, Tone, Widget,
};
use crate::ui::{fonts, symbols};
use eframe::egui::{self, Align, Align2, Color32, FontId, Rect, Sense, Stroke, UiBuilder};
use eframe::egui::{pos2, vec2};
use std::collections::BTreeSet;

/// Width of the column with the list of what the panel holds.
const LEFT_W: f32 = 216.0;
/// Width of the inspector.
const RIGHT_W: f32 = 300.0;
/// Room around the panel in the middle, above and below it and at its sides.
const PREVIEW_PAD: f32 = 28.0;
/// How far the panel in the middle can be zoomed out and in. The text is laid out again at the
/// zoomed size (see `crisp_text` of the Flow tab), so it stays sharp over the whole range.
const ZOOM_MIN: f32 = 0.5;
const ZOOM_MAX: f32 = 4.0;
/// What each press of the zoom buttons multiplies the zoom by.
const ZOOM_STEP: f32 = 1.15;
/// Biggest the sheet grows (on a big display, in full screen).
const SHEET_MAX: egui::Vec2 = egui::vec2(1500.0, 960.0);

/// What the inspector shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sel {
    Panel,
    Widget(usize),
    Button(usize),
}

/// How many frames a "show me this" request lives. The first ones are skipped on purpose: after an
/// edit the preview is made again a frame later, and where a widget is has to be read from that one.
const SHOW_TRIES: u8 = 5;
/// Frames of the request that are only waited out.
const SHOW_WAIT: u8 = 4;

/// The panel as shown in the middle.
struct Preview {
    form: Form,
    /// What `form` was made from, to tell when it has to be made again.
    shown: PanelSpec,
}

/// What is being designed. Lives in [`Ed::panel_design`] while the sheet is open.
pub struct PanelDesign {
    /// The flow and the block it belongs to.
    flow: usize,
    node: u32,
    /// The panel being made. Not checked: see [`crate::panel::design`].
    spec: PanelSpec,
    sel: Sel,
    /// What the block held when the designer last wrote or read it.
    written: String,
    /// Why the text of the block could not be read; nothing is written while it is set.
    unreadable: Option<String>,
    /// Why the panel could not be shown (a rule of [`PanelSpec::checked`]).
    problem: Option<String>,
    /// A thing the last action could not do ("a panel has at most 24 widgets").
    note: Option<String>,
    /// The preview has to be made again.
    dirty: bool,
    preview: Option<Preview>,
    /// Where each widget of the preview was drawn.
    rects: Vec<Rect>,
    /// The text of the options of the selected list, while it is being edited.
    options_buf: String,
    options_for: Option<usize>,
    /// How big the panel in the middle is drawn (1.0 = its real size).
    zoom: f32,
    /// How far the widgets inside the panel in the middle are scrolled. Kept here, not in the
    /// preview, because the preview is made again at every edit.
    body_scroll: panel_form::BodyScroll,
    /// What was picked in the list and has to be brought into view in the middle (frames left).
    show: Option<(Sel, u8)>,
}

impl PanelDesign {
    fn load(flow: usize, node: u32, message: &str) -> Self {
        let mut d = Self {
            flow,
            node,
            spec: PanelSpec::default(),
            sel: Sel::Panel,
            written: String::new(),
            unreadable: None,
            problem: None,
            note: None,
            dirty: true,
            preview: None,
            rects: Vec::new(),
            options_buf: String::new(),
            options_for: None,
            zoom: 1.0,
            body_scroll: panel_form::BodyScroll::default(),
            show: None,
        };
        d.reload(message);
        d
    }

    /// Reads `message` again (the block changed from outside).
    fn reload(&mut self, message: &str) {
        match design::from_flow_text(message) {
            Ok(spec) => {
                self.spec = spec;
                self.unreadable = None;
            }
            Err(e) => {
                self.spec = PanelSpec::default();
                self.unreadable = Some(e);
            }
        }
        self.written = message.to_string();
        self.options_for = None;
        self.dirty = true;
        self.sel = match self.sel {
            Sel::Widget(i) if i < self.spec.widgets.len() => Sel::Widget(i),
            Sel::Button(i) if i < self.spec.buttons.len() => Sel::Button(i),
            _ => Sel::Panel,
        };
    }

    /// Selects something and has the middle scroll to it (a pick in the list, a new widget).
    fn select(&mut self, sel: Sel) {
        self.select_in_place(sel);
        self.show = Some((sel, SHOW_TRIES));
    }

    /// Selects something that is already in view (a click on the panel itself): nothing scrolls.
    fn select_in_place(&mut self, sel: Sel) {
        if self.sel != sel {
            self.sel = sel;
            self.options_for = None;
        }
    }

    /// Makes the preview again when something changed.
    fn refresh(&mut self) {
        self.dirty = false;
        match self.spec.clone().checked() {
            Ok(shown) => {
                self.problem = None;
                if self.preview.as_ref().is_none_or(|p| p.shown != shown) {
                    let mut form = Form::new(shown.clone());
                    form.reveal_all = true;
                    self.preview = Some(Preview { form, shown });
                }
            }
            Err(e) => self.problem = Some(e.to_string()),
        }
    }
}

/// Opens the designer for the *Ask* block `id` of flow `fi` (nothing happens for another block).
pub(in crate::ui::settings) fn open(ed: &mut Ed, fi: usize, flow: &Flow, id: u32) {
    if let Some(node) = flow.node(id)
        && let NodeKind::Ask {
            mode: AskMode::Panel,
            message,
            ..
        } = &node.kind
    {
        ed.panel_design = Some(PanelDesign::load(fi, id, message));
    }
}

/// Draws the sheet while `Ed::panel_design` is set and writes what was designed into the block.
pub(in crate::ui::settings) fn panel_design_sheet(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    accent: Color32,
) {
    let Some(mut d) = ed.panel_design.take() else {
        return;
    };
    // the block it edits; when it is gone (undo took the block away) the designer closes
    let message = match edit
        .flows
        .get(d.flow)
        .and_then(|f| f.node(d.node))
        .map(|n| &n.kind)
    {
        Some(NodeKind::Ask {
            mode: AskMode::Panel,
            message,
            ..
        }) => message.clone(),
        _ => return,
    };
    if message != d.written {
        d.reload(&message);
    }
    if d.dirty {
        d.refresh();
    }
    // as big as the window lets it be: more room for the panel and its inspector in full screen
    let size = ed.card.size().min(SHEET_MAX);
    let (floating, changed) = sheet(ctx, "panel_design", ed.card, size, "Design panel", |ui| {
        draw(ui, &mut d, accent)
    });
    if changed && d.unreadable.is_none() {
        match design::to_flow_text(&d.spec) {
            Ok(text) => {
                d.note = None;
                if let Some(NodeKind::Ask { message, .. }) = edit
                    .flows
                    .get_mut(d.flow)
                    .and_then(|f| f.node_mut(d.node))
                    .map(|n| &mut n.kind)
                {
                    message.clone_from(&text);
                }
                d.written = text;
            }
            Err(e) => d.note = Some(e),
        }
        ctx.request_repaint();
    }
    if !floating.close {
        ed.panel_design = Some(d);
    }
}

fn draw(ui: &mut egui::Ui, d: &mut PanelDesign, accent: Color32) -> bool {
    let mut changed = false;
    let area = ui.available_rect_before_wrap();
    let left = Rect::from_min_size(area.min, vec2(LEFT_W, area.height()));
    let right = Rect::from_min_max(pos2(area.max.x - RIGHT_W, area.min.y), area.max);
    let mid = Rect::from_min_max(
        pos2(left.max.x + GAP, area.min.y),
        pos2(right.min.x - GAP, area.max.y),
    );
    ui.scope_builder(UiBuilder::new().max_rect(left), |ui| {
        changed |= left_column(ui, d, accent);
    });
    ui.scope_builder(UiBuilder::new().max_rect(mid), |ui| {
        changed |= preview_column(ui, d, mid, accent);
    });
    ui.scope_builder(UiBuilder::new().max_rect(right), |ui| {
        changed |= right_column(ui, d, accent);
    });
    if changed {
        d.dirty = true;
    }
    changed
}

// ------------------------------------------------------------------ small pieces

/// The icon of a kind of widget.
fn kind_icon(kind: Kind) -> char {
    match kind {
        Kind::Heading => symbols::TEXT_AA,
        Kind::Text => symbols::FILE_TEXT,
        Kind::Input => symbols::PENCIL_SIMPLE,
        Kind::Multiline => symbols::NOTE_PENCIL,
        Kind::Number => symbols::HASH,
        Kind::Toggle => symbols::CHECK_CIRCLE,
        Kind::Slider => symbols::SLIDERS_HORIZONTAL,
        Kind::Color => symbols::PAINT_BRUSH,
        Kind::Dropdown => symbols::CARET_DOWN,
        Kind::List => symbols::STACK,
        Kind::Chips => symbols::TAG,
        Kind::Multi => symbols::SELECTION_ALL,
        Kind::Progress => symbols::CLOCK,
        Kind::Separator => symbols::MINUS,
        Kind::Buttons => symbols::SQUARES_FOUR,
        Kind::Spacer => symbols::ARROWS_DOWN_UP,
    }
}

/// A small muted text above a control.
fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).size(12.5).color(pal().muted));
}

/// A muted sentence that wraps.
fn note_line(ui: &mut egui::Ui, text: &str, color: Color32) {
    ui.add(egui::Label::new(egui::RichText::new(text).size(12.5).color(color)).wrap());
}

/// A label and a box to type in. `rows` above 1 makes a box of several lines.
fn text_in(ui: &mut egui::Ui, label: &str, hint: &str, text: &mut String, rows: usize) -> bool {
    field_label(ui, label);
    let resp = if rows > 1 {
        ui.add(text_area(text, rows).desired_width(f32::INFINITY))
    } else {
        ui.add(
            text_field(text)
                .desired_width(f32::INFINITY)
                .hint_text(hint),
        )
    };
    resp.changed()
}

/// A number in a box that can be typed in or dragged.
fn number_in(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    speed: f64,
) -> bool {
    ui.add_sized(
        [ui.available_width(), CONTROL_H],
        egui::DragValue::new(value).range(range).speed(speed),
    )
    .changed()
}

/// A small menu under `trigger`: no title and no close button, it closes on a click outside, on a
/// choice (`ui.close()`) or on Esc.
fn menu_popup<R>(
    trigger: &egui::Response,
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
            add_contents(ui)
        })
        .map(|r| r.inner)
}

/// Two numbers side by side, each with its label above. Returns which of the two was changed.
fn number_pair(
    ui: &mut egui::Ui,
    label_a: &str,
    a: &mut f64,
    label_b: &str,
    b: &mut f64,
) -> (bool, bool) {
    ui.columns(2, |cols| {
        field_label(&mut cols[0], label_a);
        let ca = number_in(&mut cols[0], a, -1e12..=1e12, 0.1);
        field_label(&mut cols[1], label_b);
        let cb = number_in(&mut cols[1], b, -1e12..=1e12, 0.1);
        (ca, cb)
    })
}

/// One row of a list: an icon and a name, highlighted when `on`.
fn list_row(
    ui: &mut egui::Ui,
    icon: char,
    text: &str,
    on: bool,
    accent: Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), CONTROL_H),
        Sense::click_and_drag(),
    );
    if on {
        ui.painter()
            .rect_filled(rect, CONTROL_R, wash(accent, 0.16));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CONTROL_R, pal().idle);
    }
    fonts::paint_icon(
        ui.painter(),
        pos2(rect.min.x + 20.0, rect.center().y),
        icon,
        16.0,
        if on { pal().text } else { pal().muted },
        false,
    );
    let (g, _) = fit_galley(
        ui,
        text,
        FontId::proportional(14.0),
        pal().text,
        rect.width() - 46.0,
    );
    ui.painter().galley(
        pos2(rect.min.x + 38.0, rect.center().y - g.size().y / 2.0).round(),
        g,
        Color32::PLACEHOLDER,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The lists whose rows can be dragged: the widgets, the buttons at the bottom, and (one list for
/// each, counting from [`LIST_GRID`]) the buttons of a grid.
const LIST_WIDGETS: u64 = 1;
const LIST_BUTTONS: u64 = 2;
const LIST_GRID: u64 = 100;

/// What is being dragged from a list: the list and the place in it.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RowDrag {
    list: u64,
    index: usize,
}

/// What a row of a list that can be reordered reports.
struct RowOut {
    clicked: bool,
    /// Something of the same list was dropped on this row: where it came from, and whether it was
    /// let go on the lower half of the row.
    dropped: Option<(usize, bool)>,
}

/// A [`list_row`] that can be picked up and dropped on another row of the same `list`. While
/// something is over it, a line shows whether it would land above or below.
fn drag_row(
    ui: &mut egui::Ui,
    icon: char,
    text: &str,
    on: bool,
    accent: Color32,
    list: u64,
    index: usize,
) -> RowOut {
    let resp = list_row(ui, icon, text, on, accent);
    if resp.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), RowDrag { list, index });
    }
    if resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    let lower = ui
        .input(|i| i.pointer.latest_pos())
        .is_some_and(|p| p.y > resp.rect.center().y);
    if let Some(p) = resp.dnd_hover_payload::<RowDrag>()
        && p.list == list
    {
        let y = if lower {
            resp.rect.max.y + 1.0
        } else {
            resp.rect.min.y - 1.0
        };
        ui.painter()
            .hline(resp.rect.x_range(), y, Stroke::new(2.0, accent));
    }
    let dropped = resp
        .dnd_release_payload::<RowDrag>()
        .filter(|p| p.list == list)
        .map(|p| (p.index, lower));
    RowOut {
        clicked: resp.clicked(),
        dropped,
    }
}

/// Where a widget let go at height `y` of the panel lands: above the first widget whose middle is
/// below `y`, else below the last one. Hidden widgets (no rectangle) are skipped.
fn drop_slot(rects: &[Rect], y: f32) -> Option<(usize, bool)> {
    let mut last = None;
    for (i, r) in rects.iter().enumerate() {
        if !r.is_finite() {
            continue;
        }
        if y < r.center().y {
            return Some((i, false));
        }
        last = Some(i);
    }
    last.map(|i| (i, true))
}

// ------------------------------------------------------------------ left: add and structure

fn add_widget(d: &mut PanelDesign, kind: Kind) -> bool {
    let at = match d.sel {
        Sel::Widget(i) => i + 1,
        _ => d.spec.widgets.len(),
    };
    let w = design::new_widget(kind, &design::taken_ids(&d.spec));
    match design::insert_widget(&mut d.spec, at, w) {
        Some(i) => {
            d.note = None;
            d.select(Sel::Widget(i));
            true
        }
        None => {
            d.note = Some(format!(
                "A panel has at most {} widgets.",
                crate::panel::spec::MAX_WIDGETS
            ));
            false
        }
    }
}

fn left_column(ui: &mut egui::Ui, d: &mut PanelDesign, accent: Color32) -> bool {
    let mut changed = false;
    // two quiet buttons on top; what they offer opens under them, so the list of what the panel
    // holds has the whole column
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let add = button(ui, Look::Primary(accent), Some(symbols::PLUS), "Add");
        // two even columns of one-line rows, read left to right, no title: a plain menu
        let added = menu_popup(&add, 292.0, |ui| {
            let mut added = false;
            ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
            for pair in Kind::ALL.chunks(2) {
                ui.columns(2, |cols| {
                    for (col, kind) in cols.iter_mut().zip(pair) {
                        if list_row(col, kind_icon(*kind), kind.label(), false, accent)
                            .on_hover_text(kind.hint())
                            .clicked()
                        {
                            added |= add_widget(d, *kind);
                            col.close();
                        }
                    }
                });
            }
            added
        });
        changed |= added.unwrap_or(false);

        let from = button(ui, Look::Plain, Some(symbols::SQUARES_FOUR), "Templates");
        let picked = menu_popup(&from, 250.0, |ui| {
            let mut picked = false;
            ui.spacing_mut().item_spacing.y = 2.0;
            note_line(ui, "Replaces the panel (undo brings it back).", pal().muted);
            for (i, t) in design::TEMPLATES.iter().enumerate() {
                if list_row(ui, symbols::SQUARES_FOUR, t.name, false, accent)
                    .on_hover_text(t.hint)
                    .clicked()
                    && let Some(spec) = design::template_spec(i)
                {
                    d.spec = spec;
                    d.unreadable = None;
                    d.options_for = None;
                    d.sel = Sel::Panel;
                    d.body_scroll.offset = 0.0;
                    picked = true;
                    ui.close();
                }
            }
            picked
        });
        changed |= picked.unwrap_or(false);
    });
    ui.add_space(GAP);
    section_title(ui, "Structure");

    let mut pick = None;
    // a row dropped on another one: (is it a widget, from, over, below)
    let mut moved: Option<(bool, usize, usize, bool)> = None;
    egui::ScrollArea::vertical()
        .id_salt("panel_design_structure")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let title = if d.spec.title.is_empty() {
                "Panel"
            } else {
                d.spec.title.as_str()
            };
            if list_row(
                ui,
                symbols::SQUARES_FOUR,
                title,
                d.sel == Sel::Panel,
                accent,
            )
            .clicked()
            {
                pick = Some(Sel::Panel);
            }
            for (i, w) in d.spec.widgets.iter().enumerate() {
                let kind = design::kind_of(w);
                let mut text = design::summary(w);
                if text.is_empty() {
                    text = kind.label().to_string();
                }
                if w.show_if().is_some() {
                    text = format!("\u{21b3} {text}");
                }
                let out = drag_row(
                    ui,
                    kind_icon(kind),
                    &text,
                    d.sel == Sel::Widget(i),
                    accent,
                    LIST_WIDGETS,
                    i,
                );
                if out.clicked {
                    pick = Some(Sel::Widget(i));
                }
                if let Some((from, after)) = out.dropped {
                    moved = Some((true, from, i, after));
                }
            }
            ui.add_space(GAP);
            section_title(ui, "Buttons");
            if design::uses_default_buttons(&d.spec) {
                note_line(ui, "Cancel and OK, until you add your own.", pal().muted);
            }
            for (i, b) in d.spec.buttons.iter().enumerate() {
                let icon = match b.tone {
                    Tone::Primary => symbols::CHECK_CIRCLE,
                    Tone::Danger => symbols::TRASH,
                    Tone::Secondary => symbols::MINUS,
                };
                let text = if b.label.is_empty() { &b.id } else { &b.label };
                let out = drag_row(
                    ui,
                    icon,
                    text,
                    d.sel == Sel::Button(i),
                    accent,
                    LIST_BUTTONS,
                    i,
                );
                if out.clicked {
                    pick = Some(Sel::Button(i));
                }
                if let Some((from, after)) = out.dropped {
                    moved = Some((false, from, i, after));
                }
            }
            if let Some(b) = design::new_button(&d.spec)
                && button(ui, Look::Soft, Some(symbols::PLUS), "Add button").clicked()
            {
                d.spec.buttons.push(b);
                pick = Some(Sel::Button(d.spec.buttons.len() - 1));
                changed = true;
            }
        });
    if let Some((widgets, from, over, after)) = moved {
        if widgets {
            if let Some(to) = design::drop_widget(&mut d.spec, from, over, after) {
                pick = Some(Sel::Widget(to));
                changed = true;
            }
        } else if let Some(to) = design::drop_target(d.spec.buttons.len(), from, over, after)
            && design::move_item(&mut d.spec.buttons, from, to)
        {
            pick = Some(Sel::Button(to));
            changed = true;
        }
    }
    if let Some(sel) = pick {
        d.select(sel);
    }
    changed
}

// ------------------------------------------------------------------ middle: the real panel

fn preview_column(ui: &mut egui::Ui, d: &mut PanelDesign, rect: Rect, accent: Color32) -> bool {
    ui.painter().rect_filled(rect, PANEL_R, pal().bg);
    let mut next = None;
    // the selected widget was dragged by its grip to another place
    let mut changed = false;
    {
        let PanelDesign {
            preview,
            rects,
            sel,
            problem,
            spec,
            zoom,
            body_scroll,
            show,
            ..
        } = d;
        let Some(p) = preview.as_mut() else {
            let text = problem
                .as_deref()
                .unwrap_or("Add something from the left to see the panel.");
            ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(24.0)), |ui| {
                note_line(ui, text, pal().bad);
            });
            return false;
        };
        let (w, h) = card_size(&p.shown);

        // Alt + scroll or a pinch zooms, like on the canvas of a flow
        let ctx = ui.ctx().clone();
        let (pos, scroll, pinch, alt) = ctx.input(|i| {
            (
                i.pointer.latest_pos(),
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.modifiers.alt,
            )
        });
        if pos.is_some_and(|p| rect.contains(p)) && !egui::Popup::is_any_open(&ctx) {
            let mut factor = 1.0;
            if pinch != 1.0 {
                factor = pinch;
            } else if alt && scroll != egui::Vec2::ZERO {
                let delta = if scroll.y != 0.0 { scroll.y } else { scroll.x };
                factor = (delta * 0.0025).exp();
                // the zoom took the wheel: the page does not scroll as well
                ctx.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
            if factor != 1.0 {
                *zoom = (*zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
            }
        }

        let z = *zoom;
        let mut pressed = None;
        // where the panel is on the screen, and how its own points become screen points
        let mut to_screen = egui::emath::TSTransform::IDENTITY;
        // the panel is drawn whole: when it is bigger than the room (taller, or zoomed in) it
        // scrolls, instead of being cut off
        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect);
            egui::ScrollArea::both()
                .id_salt("panel_design_preview")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let size = vec2(w, h) * z;
                    let total = vec2(
                        rect.width().max(size.x + 2.0 * PREVIEW_PAD),
                        rect.height().max(size.y + 2.0 * PREVIEW_PAD),
                    );
                    let (slot, _) = ui.allocate_exact_size(total, Sense::hover());
                    let at = Rect::from_center_size(slot.center(), size);
                    to_screen = egui::emath::TSTransform::new(at.min.to_vec2(), z);
                    let from_screen = to_screen.inverse();
                    // the panel is a layer of its own that is zoomed as a whole, so text, fields
                    // and hit areas all follow
                    let layer =
                        egui::LayerId::new(ui.layer_id().order, ui.id().with("panel_design_scene"));
                    ui.ctx().set_sublayer(ui.layer_id(), layer);
                    ui.ctx().set_transform_layer(layer, to_screen);
                    let own = Rect::from_min_size(egui::Pos2::ZERO, vec2(w, h));
                    let mut scene = ui.new_child(UiBuilder::new().layer_id(layer).max_rect(own));
                    scene.set_clip_rect(from_screen * ui.clip_rect());
                    if let panel_form::Event::Press(id) = panel_form::preview(
                        &mut scene,
                        own,
                        &mut p.form,
                        accent,
                        rects,
                        body_scroll,
                    ) {
                        pressed = Some(id);
                    }
                    // the panel is drawn: lay its text out again at the zoomed size, so it is
                    // sharp like a vector drawing and not a picture that was made bigger
                    super::tabs::flow::crisp_text(&ctx, scene.painter(), layer, z);
                });
        });
        let card = Rect::from_min_size(egui::Pos2::ZERO, vec2(w, h));
        let on_screen = to_screen * card;

        // the wheel over the panel scrolls the widgets inside it: what the scroll area of the panel
        // did not take (it only takes it when it sees the pointer, and the panel is a zoomed layer)
        // is used here, so there is no need to drag the bar
        let (wheel, over) = ctx.input(|i| (i.smooth_scroll_delta.y, i.pointer.latest_pos()));
        if wheel != 0.0
            && body_scroll.max > 0.0
            && over.is_some_and(|p| rect.contains(p) && on_screen.contains(p))
            && !egui::Popup::is_any_open(&ctx)
        {
            let before = body_scroll.offset;
            body_scroll.offset = (before - wheel / z).clamp(0.0, body_scroll.max);
            let used = (before - body_scroll.offset) * z;
            if used != 0.0 {
                ctx.input_mut(|i| i.smooth_scroll_delta.y -= used);
                ctx.request_repaint();
            }
        }

        // a click selects what it landed on
        let inner = card.shrink(layout::PAD);
        let head = Rect::from_min_size(inner.min, vec2(inner.width(), header_height(&p.shown)));
        let foot_top = inner.max.y - layout::footer_height();

        // what was picked in the list is brought into view: the middle scrolls to it
        if let Some((target, left)) = *show {
            if left > SHOW_WAIT {
                *show = Some((target, left - 1));
            } else {
                let view = panel_form::body_rect(card, &p.shown);
                let margin = 10.0;
                let found = match target {
                    Sel::Panel => {
                        body_scroll.offset = 0.0;
                        true
                    }
                    // the buttons are not in the part that scrolls
                    Sel::Button(_) => true,
                    Sel::Widget(i) => match rects.get(i).copied().filter(|r| r.is_finite()) {
                        Some(r) => {
                            let mut offset = body_scroll.offset;
                            if r.height() > view.height() - 2.0 * margin
                                || r.min.y < view.min.y + margin
                            {
                                // above the view, or too tall to fit: its top goes to the top
                                offset += r.min.y - (view.min.y + margin);
                            } else if r.max.y > view.max.y - margin {
                                offset += r.max.y - (view.max.y - margin);
                            }
                            body_scroll.offset = offset.max(0.0);
                            true
                        }
                        None => false,
                    },
                };
                *show = if found || left == 0 {
                    None
                } else {
                    Some((target, left - 1))
                };
            }
            ctx.request_repaint();
        }
        let click = ui.input(|i| {
            if i.pointer.primary_clicked() {
                i.pointer.interact_pos()
            } else {
                None
            }
        });
        if let Some(pos) = click
            && rect.contains(pos)
            && on_screen.contains(pos)
        {
            let at = to_screen.inverse() * pos;
            if head.contains(at) {
                next = Some(Sel::Panel);
            } else if at.y < foot_top
                && let Some(i) = rects.iter().position(|r| r.contains(at))
            {
                next = Some(Sel::Widget(i));
            }
        }
        if let Some(id) = pressed {
            // a button of the bottom row, or one of a grid (which selects the grid)
            next = spec
                .buttons
                .iter()
                .position(|b| b.id == id)
                .map(Sel::Button)
                .or_else(|| {
                    spec.widgets
                        .iter()
                        .position(|w| {
                            matches!(w, Widget::Buttons { buttons, .. }
                                if buttons.iter().any(|b| b.id == id))
                        })
                        .map(Sel::Widget)
                })
                .or(Some(Sel::Panel));
        }
        // the outline of what is selected
        let painter = ui
            .painter()
            .with_clip_rect(on_screen.expand(8.0).intersect(rect));
        let outline = match *sel {
            Sel::Widget(i) => rects.get(i).copied().filter(|r| r.is_finite()),
            Sel::Panel => Some(head),
            Sel::Button(_) => None,
        };
        if let Some(r) = outline {
            painter.rect_stroke(
                (to_screen * r).expand(5.0),
                8.0,
                Stroke::new(2.0, accent),
                egui::StrokeKind::Outside,
            );
        }
        // a grip beside the selected widget: drag it up or down to move the widget
        if let Sel::Widget(si) = *sel
            && let Some(r) = rects.get(si).copied().filter(|r| r.is_finite())
        {
            let sr = to_screen * r;
            let grip = Rect::from_center_size(
                pos2(on_screen.min.x - 16.0, sr.center().y),
                vec2(20.0, 30.0),
            );
            let resp = ui.interact(grip, ui.id().with("panel_design_grip"), Sense::drag());
            let hot = resp.hovered() || resp.dragged();
            let clip = ui.painter().with_clip_rect(rect);
            clip.rect_filled(grip, 6.0, if hot { wash(accent, 0.25) } else { pal().idle });
            fonts::paint_icon(
                &clip,
                grip.center(),
                symbols::ARROWS_DOWN_UP,
                15.0,
                pal().text,
                false,
            );
            if hot {
                ctx.set_cursor_icon(if resp.dragged() {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::Grab
                });
            }
            if resp.dragged() || resp.drag_stopped() {
                let pointer = ctx.input(|i| i.pointer.latest_pos());
                let slot = pointer.and_then(|p| drop_slot(rects, (to_screen.inverse() * p).y));
                if resp.dragged()
                    && let Some((over, after)) = slot
                    && let Some(target) = rects.get(over).filter(|t| t.is_finite())
                {
                    let y = if after {
                        target.max.y + GAP / 2.0
                    } else {
                        target.min.y - GAP / 2.0
                    };
                    let line = [
                        to_screen * pos2(card.min.x + layout::PAD, y),
                        to_screen * pos2(card.max.x - layout::PAD, y),
                    ];
                    painter.line_segment(line, Stroke::new(3.0, accent));
                }
                if resp.drag_stopped()
                    && let Some((over, after)) = slot
                    && let Some(to) = design::drop_widget(spec, si, over, after)
                {
                    next = Some(Sel::Widget(to));
                    changed = true;
                }
            }
        }
        if let Some(text) = problem.as_deref() {
            ui.painter().with_clip_rect(rect).text(
                pos2(rect.center().x, rect.max.y - 12.0),
                Align2::CENTER_BOTTOM,
                text,
                FontId::proportional(12.5),
                pal().bad,
            );
        }
        zoom_bar(ui, rect, zoom);
    }
    if let Some(sel) = next {
        // a click on the panel itself: it is already in view
        d.select_in_place(sel);
    }
    changed
}

/// The small capsule at the bottom middle of the preview: zoom out, the zoom (a press goes back to
/// 100%), zoom in.
///
/// It is an area of its own, above everything: the panel in the middle is a layer of its own (so it
/// can be zoomed) and is drawn over whatever the page itself draws, so a capsule drawn in the page
/// went under the panel when this was tall. An area is also centered by the toolkit, whatever the
/// width of what is in it.
fn zoom_bar(ui: &mut egui::Ui, rect: Rect, zoom: &mut f32) {
    egui::Area::new(egui::Id::new("panel_design_zoom_bar"))
        .order(egui::Order::Tooltip)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(rect.center().x, rect.max.y - 14.0))
        .constrain(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(pal().card)
                .stroke(Stroke::new(1.0, pal().border))
                .corner_radius(PILL)
                .inner_margin(egui::Margin::same(4))
                .shadow(popup_shadow())
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if icon_button(ui, symbols::MINUS, "Zoom out", *zoom > ZOOM_MIN + 0.001)
                            .clicked()
                        {
                            *zoom = (*zoom / ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
                        }
                        let label = format!("{}%", (*zoom * 100.0).round());
                        if button(ui, Look::Plain, None, &label)
                            .on_hover_text("Back to 100%")
                            .clicked()
                        {
                            *zoom = 1.0;
                        }
                        if icon_button(ui, symbols::PLUS, "Zoom in", *zoom < ZOOM_MAX - 0.001)
                            .clicked()
                        {
                            *zoom = (*zoom * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX);
                        }
                    });
                });
        });
}

// ------------------------------------------------------------------ right: the inspector

/// What the buttons of the inspector's header ask for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Up,
    Down,
    Copy,
    Remove,
}

fn right_column(ui: &mut egui::Ui, d: &mut PanelDesign, accent: Color32) -> bool {
    let mut changed = false;
    let sel = d.sel;
    let (title, can_up, can_down, can_copy) = match sel {
        Sel::Panel => ("Panel".to_string(), false, false, false),
        Sel::Widget(i) => (
            d.spec
                .widgets
                .get(i)
                .map_or("Widget", |w| design::kind_of(w).label())
                .to_string(),
            i > 0,
            i + 1 < d.spec.widgets.len(),
            true,
        ),
        Sel::Button(i) => (
            "Button".to_string(),
            i > 0,
            i + 1 < d.spec.buttons.len(),
            false,
        ),
    };
    let mut act = None;
    ui.horizontal(|ui| {
        ui.label(fonts::title(&title, 15.0, pal().text));
        if sel != Sel::Panel {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if icon_button_look(ui, Look::Danger, symbols::TRASH, "Remove", true).clicked() {
                    act = Some(Act::Remove);
                }
                if can_copy
                    && icon_button_look(ui, Look::Plain, symbols::COPY, "Duplicate", true).clicked()
                {
                    act = Some(Act::Copy);
                }
                if icon_button_look(ui, Look::Plain, symbols::ARROW_DOWN, "Move down", can_down)
                    .clicked()
                {
                    act = Some(Act::Down);
                }
                if icon_button_look(ui, Look::Plain, symbols::ARROW_UP, "Move up", can_up).clicked()
                {
                    act = Some(Act::Up);
                }
            });
        }
    });
    if let Some(act) = act {
        changed |= run_act(d, sel, act);
    }
    if let Some(note) = d.note.clone() {
        note_line(ui, &note, pal().bad);
    }
    if let Some(e) = d.unreadable.clone() {
        note_line(
            ui,
            &format!(
                "The text of this block is not a panel ({e}). Pick one under Start from to replace it."
            ),
            pal().bad,
        );
    }
    ui.add_space(4.0);
    egui::ScrollArea::vertical()
        .id_salt("panel_design_inspector")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let sel = d.sel;
            changed |= match sel {
                Sel::Panel => panel_fields(ui, d, accent),
                Sel::Widget(i) => widget_fields(ui, d, i, accent),
                Sel::Button(i) => button_fields(ui, d, i, accent),
            };
        });
    changed
}

/// Carries out the move, copy or removal asked for by the header of the inspector.
fn run_act(d: &mut PanelDesign, sel: Sel, act: Act) -> bool {
    d.note = None;
    match (sel, act) {
        (Sel::Widget(i), Act::Up | Act::Down) => {
            let delta = if act == Act::Up { -1 } else { 1 };
            design::move_widget(&mut d.spec, i, delta).map(Sel::Widget)
        }
        (Sel::Widget(i), Act::Copy) => {
            let copy = design::duplicate_widget(&mut d.spec, i);
            if copy.is_none() {
                d.note = Some(format!(
                    "A panel has at most {} widgets.",
                    crate::panel::spec::MAX_WIDGETS
                ));
            }
            copy.map(Sel::Widget)
        }
        (Sel::Widget(i), Act::Remove) => {
            let next = design::remove_widget(&mut d.spec, i);
            Some(next.map_or(Sel::Panel, Sel::Widget))
        }
        (Sel::Button(i), Act::Up | Act::Down) => {
            let to = if act == Act::Up {
                i.checked_sub(1)
            } else {
                Some(i + 1)
            };
            to.filter(|to| *to < d.spec.buttons.len()).map(|to| {
                d.spec.buttons.swap(i, to);
                Sel::Button(to)
            })
        }
        (Sel::Button(i), Act::Remove) => {
            if i < d.spec.buttons.len() {
                d.spec.buttons.remove(i);
            }
            Some(if d.spec.buttons.is_empty() {
                Sel::Panel
            } else {
                Sel::Button(i.min(d.spec.buttons.len() - 1))
            })
        }
        _ => None,
    }
    .map(|to| d.select(to))
    .is_some()
}

fn panel_fields(ui: &mut egui::Ui, d: &mut PanelDesign, accent: Color32) -> bool {
    let mut c = false;
    let spec = &mut d.spec;
    c |= text_in(ui, "Title", "Shown at the top", &mut spec.title, 1);
    c |= text_in(
        ui,
        "Subtitle",
        "A line under the title",
        &mut spec.subtitle,
        1,
    );

    field_label(ui, "Icon");
    let icons = [
        (Icon::Flow, "Flow"),
        (Icon::App, "App"),
        (Icon::Link, "Link"),
        (Icon::File, "File"),
        (Icon::Shortcut, "Shortcut"),
        (Icon::Command, "Command"),
        (Icon::Browser, "Browser"),
    ];
    let names: Vec<&str> = icons.iter().map(|(_, n)| *n).collect();
    let at = icons.iter().position(|(i, _)| *i == spec.icon).unwrap_or(0);
    let width = ui.available_width();
    if let Some(i) = select(ui, width, names[at], &names, at) {
        spec.icon = icons[i].0;
        c = true;
    }

    field_label(ui, "Appears");
    let places = [Place::Cursor, Place::Center, Place::Top, Place::Bottom];
    let at = places.iter().position(|p| *p == spec.place).unwrap_or(0);
    let items = [
        (None, "Cursor"),
        (None, "Center"),
        (None, "Top"),
        (None, "Bottom"),
    ];
    if let Some(i) = segmented(ui, &items, at) {
        spec.place = places[i];
        c = true;
    }

    let mut w = spec.width.unwrap_or(WIDTH_DEFAULT);
    field_label(ui, &format!("Width: {} pt", w.round()));
    if slider(ui, &mut w, WIDTH_MIN..=WIDTH_MAX, accent).changed() {
        spec.width = Some(w.round());
        c = true;
    }

    // height: as tall as the content needs (the default), or a fixed one
    let fixed = spec.height.is_some();
    field_label(
        ui,
        &match spec.height {
            Some(h) => format!("Height: {} pt", h.round()),
            None => "Height: fits the content".to_string(),
        },
    );
    let items = [(None, "Auto"), (None, "Fixed")];
    if let Some(i) = segmented(ui, &items, usize::from(fixed)) {
        spec.height = if i == 1 {
            // start from the height the card has now, so switching does not make it jump
            Some(card_size(spec).1.clamp(HEIGHT_MIN, HEIGHT_MAX).round())
        } else {
            None
        };
        c = true;
    }
    if let Some(mut h) = spec.height
        && slider(ui, &mut h, HEIGHT_MIN..=HEIGHT_MAX, accent).changed()
    {
        spec.height = Some(h.round());
        c = true;
    }

    field_label(ui, "Gives up after (0 = never)");
    let mut secs = spec.timeout_s.unwrap_or(DEFAULT_TIMEOUT_S);
    if ui
        .add_sized(
            [ui.available_width(), CONTROL_H],
            egui::DragValue::new(&mut secs)
                .range(0..=MAX_TIMEOUT_S)
                .suffix(" s"),
        )
        .changed()
    {
        spec.timeout_s = Some(secs);
        c = true;
    }

    field_label(ui, "Buttons at the bottom");
    let aligns = [
        ButtonAlign::End,
        ButtonAlign::Start,
        ButtonAlign::Center,
        ButtonAlign::Stretch,
    ];
    let at = aligns
        .iter()
        .position(|a| *a == spec.buttons_align)
        .unwrap_or(0);
    let items = [
        (None, "Right"),
        (None, "Left"),
        (None, "Center"),
        (None, "Wide"),
    ];
    if let Some(k) = segmented(ui, &items, at) {
        spec.buttons_align = aligns[k];
        c = true;
    }
    ui.add_space(GAP);
    panel(ui, "", |ui| {
        c |= toggle_row(
            ui,
            &mut spec.keep_open,
            "Stay open after a button",
            "The panel does not go away and come back while the flow answers each press. Give it a name below.",
            accent,
        )
        .changed();
    });

    field_label(ui, "Color of the badge and the main button");
    let [r, g, b, _] = accent.to_array();
    let mut rgb = spec
        .accent
        .as_deref()
        .and_then(parse_hex)
        .unwrap_or([r, g, b]);
    let before = rgb;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), CONTROL_H),
        egui::Layout::right_to_left(Align::Center),
        |ui| accent_picker(ui, &mut rgb),
    );
    if rgb != before {
        spec.accent = Some(format_hex(rgb));
        c = true;
    }
    if spec.accent.is_some() && button(ui, Look::Plain, None, "Use the color of Settings").clicked()
    {
        spec.accent = None;
        c = true;
    }

    c |= text_in(
        ui,
        "Name (optional)",
        "so a flow or the extension can update it",
        &mut spec.id,
        1,
    );

    if design::uses_default_buttons(spec) {
        ui.add_space(GAP);
        note_line(ui, "The buttons are Cancel and OK.", pal().muted);
        if button(ui, Look::Soft, None, "Customize the buttons").clicked() {
            let menu = spec
                .widgets
                .iter()
                .any(|w| matches!(w, Widget::Choice { submit: true, .. }));
            spec.buttons = default_buttons(menu);
            d.sel = Sel::Button(0);
            c = true;
        }
    }
    c
}

fn button_fields(ui: &mut egui::Ui, d: &mut PanelDesign, i: usize, accent: Color32) -> bool {
    let mut c = false;
    let Some(b) = d.spec.buttons.get_mut(i) else {
        return false;
    };
    c |= one_button_fields(ui, b, accent);
    c
}

/// The icon of a button: picked from the icons of the app, or typed (a name, or `file:` and the
/// path of a picture).
fn icon_fields(ui: &mut egui::Ui, icon: &mut String) -> bool {
    let mut c = false;
    field_label(ui, "Icon");
    let name = icon
        .strip_prefix(symbols::PREFIX)
        .unwrap_or(icon.as_str())
        .to_string();
    let mut items: Vec<(Option<char>, &str)> = vec![(None, "No icon")];
    items.extend(symbols::ALL.iter().map(|(n, ch)| (Some(*ch), *n)));
    let at = symbols::ALL
        .iter()
        .position(|(n, _)| *n == name)
        .map_or(0, |p| p + 1);
    let width = ui.available_width();
    if let Some(k) = select_icons(ui, width, &items, at) {
        *icon = if k == 0 {
            String::new()
        } else {
            symbols::ALL[k - 1].0.to_string()
        };
        c = true;
    }
    c |= text_in(
        ui,
        "Icon name, or a picture",
        "play, or file:/path/to/picture.png",
        icon,
        1,
    );
    c
}

/// Everything a button has, at the bottom or in a grid.
fn one_button_fields(ui: &mut egui::Ui, b: &mut Button, accent: Color32) -> bool {
    let mut c = false;
    c |= text_in(
        ui,
        "Label",
        "What it says (empty: only the icon)",
        &mut b.label,
        1,
    );
    c |= text_in(
        ui,
        "Answer name",
        "The flow gets this when it is pressed",
        &mut b.id,
        1,
    );
    c |= icon_fields(ui, &mut b.icon);
    field_label(ui, "Look");
    let tones = [Tone::Secondary, Tone::Primary, Tone::Danger];
    let at = tones.iter().position(|t| *t == b.tone).unwrap_or(0);
    let items = [(None, "Normal"), (None, "Main"), (None, "Danger")];
    if let Some(i) = segmented(ui, &items, at) {
        b.tone = tones[i];
        c = true;
    }
    ui.add_space(GAP);
    panel(ui, "", |ui| {
        c |= toggle_row(
            ui,
            &mut b.cancel,
            "Cancels",
            "Also what Esc does: closes without an answer.",
            accent,
        )
        .changed();
        c |= toggle_row(
            ui,
            &mut b.keep_open,
            "Stays open",
            "The panel does not go away when this button is pressed.",
            accent,
        )
        .changed();
    });
    c
}

/// The buttons of a grid: a list to reorder by dragging, and the fields of the one picked.
fn grid_buttons_editor(
    ui: &mut egui::Ui,
    grid: usize,
    buttons: &mut Vec<Button>,
    taken: &BTreeSet<String>,
    accent: Color32,
) -> bool {
    let mut c = false;
    let key = egui::Id::new(("panel_design_grid_button", grid));
    let mut sel: usize = ui.data(|d| d.get_temp::<usize>(key)).unwrap_or(0);
    sel = sel.min(buttons.len().saturating_sub(1));
    let list = LIST_GRID + grid as u64;
    let mut dropped = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (i, b) in buttons.iter().enumerate() {
            let text = if b.label.is_empty() {
                b.id.clone()
            } else {
                b.label.clone()
            };
            let icon = symbols::resolve(&b.icon)
                .or_else(|| symbols::by_name(&b.icon))
                .unwrap_or(symbols::MINUS);
            let out = drag_row(ui, icon, &text, sel == i, accent, list, i);
            if out.clicked {
                sel = i;
            }
            if let Some((from, after)) = out.dropped {
                dropped = Some((from, i, after));
            }
        }
    });
    if let Some((from, over, after)) = dropped
        && let Some(to) = design::drop_target(buttons.len(), from, over, after)
        && design::move_item(buttons, from, to)
    {
        sel = to;
        c = true;
    }
    if buttons.len() < MAX_GROUP_BUTTONS
        && button(ui, Look::Soft, Some(symbols::PLUS), "Add button").clicked()
    {
        let mut used = taken.clone();
        used.extend(buttons.iter().map(|b| b.id.clone()));
        let id = design::unique_id("action", &used);
        buttons.push(Button {
            label: "Button".into(),
            id,
            ..Button::default()
        });
        sel = buttons.len() - 1;
        c = true;
    }
    let mut remove = false;
    if let Some(b) = buttons.get_mut(sel) {
        ui.add_space(GAP);
        c |= one_button_fields(ui, b, accent);
        remove = button(ui, Look::Danger, Some(symbols::TRASH), "Remove this button").clicked();
    }
    if remove {
        buttons.remove(sel);
        sel = sel.saturating_sub(1);
        c = true;
    }
    ui.data_mut(|d| d.insert_temp(key, sel));
    c
}

/// Fields of one widget, then what every widget has: its name and the condition that shows it.
fn widget_fields(ui: &mut egui::Ui, d: &mut PanelDesign, i: usize, accent: Color32) -> bool {
    let mut c = false;
    let candidates = design::conditions_for(&d.spec, i);
    let taken_buttons = design::taken_button_ids(&d.spec);
    {
        let PanelDesign {
            spec,
            options_buf,
            options_for,
            ..
        } = &mut *d;
        let Some(w) = spec.widgets.get_mut(i) else {
            return false;
        };
        c |= type_fields(ui, w, i, options_buf, options_for, &taken_buttons, accent);
    }
    c |= id_field(ui, &mut d.spec, i);
    c |= show_if_fields(ui, &mut d.spec, i, &candidates, accent);
    c
}

fn id_field(ui: &mut egui::Ui, spec: &mut PanelSpec, i: usize) -> bool {
    let Some(w) = spec.widgets.get(i) else {
        return false;
    };
    if matches!(w, Widget::Separator { .. } | Widget::Spacer { .. }) {
        return false;
    }
    let field = w.is_field();
    let mut id = w.id().unwrap_or_default().to_string();
    let (label, hint) = if field {
        ("Answer name", "letters, digits and _")
    } else {
        (
            "Name (optional)",
            "so a flow can change it while the panel is open",
        )
    };
    let changed = text_in(ui, label, hint, &mut id, 1);
    if changed {
        design::rename_widget(spec, i, &id);
    }
    if field && !id.is_empty() {
        note_line(ui, &format!("In the flow: {{{{{id}}}}}"), pal().muted);
    }
    changed
}

fn type_fields(
    ui: &mut egui::Ui,
    w: &mut Widget,
    i: usize,
    options_buf: &mut String,
    options_for: &mut Option<usize>,
    taken_buttons: &BTreeSet<String>,
    accent: Color32,
) -> bool {
    let mut c = false;
    match w {
        Widget::Text { text, style, .. } => {
            c |= text_in(ui, "Text", "", text, 4);
            field_label(ui, "Style");
            let styles = [Style::Body, Style::Heading, Style::Muted, Style::Code];
            let at = styles.iter().position(|s| s == style).unwrap_or(0);
            let items = [
                (None, "Text"),
                (None, "Heading"),
                (None, "Muted"),
                (None, "Code"),
            ];
            if let Some(k) = segmented(ui, &items, at) {
                *style = styles[k];
                c = true;
            }
        }
        Widget::Separator { .. } => note_line(ui, "A thin line between two groups.", pal().muted),
        Widget::Spacer { height, .. } => {
            field_label(ui, "Height (points)");
            let mut h = f64::from(height.unwrap_or(SPACER_DEFAULT));
            if number_in(
                ui,
                &mut h,
                f64::from(SPACER_MIN)..=f64::from(SPACER_MAX),
                1.0,
            ) {
                *height = Some(h as f32);
                c = true;
            }
        }
        Widget::Buttons {
            label,
            columns,
            buttons,
            ..
        } => {
            c |= text_in(ui, "Label (optional)", "", label, 1);
            field_label(ui, "Buttons per row");
            let items = [
                (None, "Auto"),
                (None, "1"),
                (None, "2"),
                (None, "3"),
                (None, "4"),
                (None, "5"),
                (None, "6"),
            ];
            if let Some(k) = segmented(ui, &items, usize::from(*columns).min(6)) {
                *columns = k as u8;
                c = true;
            }
            field_label(ui, "Buttons (drag a row to move it)");
            c |= grid_buttons_editor(ui, i, buttons, taken_buttons, accent);
            note_line(
                ui,
                "Pressing one answers with its answer name. Turn on Stay open in the panel so the window does not blink between presses.",
                pal().muted,
            );
        }
        Widget::Input {
            label,
            hint,
            value,
            secret,
            multiline,
            required,
            ..
        } => {
            c |= text_in(ui, "Label", "", label, 1);
            c |= text_in(ui, "Hint inside the box", "", hint, 1);
            c |= text_in(ui, "Starts with", "", value, 1);
            ui.add_space(GAP);
            panel(ui, "", |ui| {
                c |= toggle_row(
                    ui,
                    required,
                    "Required",
                    "OK stays refused while it is empty.",
                    accent,
                )
                .changed();
                c |= toggle_row(ui, secret, "Password", "Shown as dots.", accent).changed();
                c |= toggle_row(ui, multiline, "Several lines", "", accent).changed();
            });
        }
        Widget::Number {
            label,
            value,
            min,
            max,
            step,
            unit,
            ..
        } => {
            c |= text_in(ui, "Label", "", label, 1);
            c |= text_in(ui, "Unit", "px, min, ...", unit, 1);
            let mut lo = min.unwrap_or(0.0);
            let mut hi = max.unwrap_or(100.0);
            let (lo_c, hi_c) = number_pair(ui, "Smallest", &mut lo, "Largest", &mut hi);
            if lo_c {
                *min = Some(lo);
                c = true;
            }
            if hi_c {
                *max = Some(hi);
                c = true;
            }
            let mut v = value.unwrap_or(0.0);
            let mut s = step.unwrap_or(1.0);
            let (v_c, s_c) = number_pair(ui, "Starts at", &mut v, "Step (+ and -)", &mut s);
            if v_c {
                *value = Some(v);
                c = true;
            }
            if s_c {
                *step = Some(s.max(0.001));
                c = true;
            }
            if (min.is_some() || max.is_some())
                && button(ui, Look::Plain, None, "No limits").clicked()
            {
                *min = None;
                *max = None;
                c = true;
            }
        }
        Widget::Toggle { label, value, .. } => {
            c |= text_in(ui, "Label", "", label, 1);
            ui.add_space(GAP);
            panel(ui, "", |ui| {
                c |= toggle_row(ui, value, "On at the start", "", accent).changed();
            });
        }
        Widget::Slider {
            label,
            min,
            max,
            step,
            value,
            unit,
            ..
        } => {
            c |= text_in(ui, "Label", "", label, 1);
            c |= text_in(ui, "Unit", "%, s, ...", unit, 1);
            let (lo_c, hi_c) = number_pair(ui, "Smallest", min, "Largest", max);
            c |= lo_c || hi_c;
            let mut s = step.unwrap_or(1.0);
            let mut v = value.unwrap_or(*min);
            let (s_c, v_c) = number_pair(ui, "Step", &mut s, "Starts at", &mut v);
            if s_c {
                *step = Some(s.max(0.001));
                c = true;
            }
            if v_c {
                *value = Some(v);
                c = true;
            }
            if *min >= *max {
                note_line(ui, "The smallest has to be below the largest.", pal().bad);
            }
        }
        Widget::Color { label, value, .. } => {
            c |= text_in(ui, "Label", "", label, 1);
            field_label(ui, "Starts as");
            let mut rgb = parse_hex(value).unwrap_or([0x3b, 0x82, 0xf6]);
            let before = rgb;
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), CONTROL_H),
                egui::Layout::right_to_left(Align::Center),
                |ui| accent_picker(ui, &mut rgb),
            );
            if rgb != before || value.is_empty() {
                *value = format_hex(rgb);
                c = true;
            }
        }
        Widget::Choice {
            label,
            options,
            value,
            layout,
            submit,
            ..
        } => {
            c |= text_in(ui, "Label", "", label, 1);
            field_label(ui, "Shown as");
            let layouts = [ListLayout::List, ListLayout::Chips, ListLayout::Dropdown];
            let at = layouts.iter().position(|l| l == layout).unwrap_or(0);
            let items = [(None, "List"), (None, "Chips"), (None, "Dropdown")];
            if let Some(k) = segmented(ui, &items, at) {
                *layout = layouts[k];
                c = true;
            }
            c |= options_editor(ui, options_buf, options_for, i, options);
            field_label(ui, "Picked at the start");
            let mut names: Vec<&str> = vec!["(nothing)"];
            names.extend(options.iter().map(|o| {
                if o.label.is_empty() {
                    o.value.as_str()
                } else {
                    o.label.as_str()
                }
            }));
            let at = options
                .iter()
                .position(|o| o.value == *value)
                .map_or(0, |p| p + 1);
            let width = ui.available_width();
            if let Some(k) = select_capped(ui, width, names[at], &names, at, 220.0) {
                *value = if k == 0 {
                    String::new()
                } else {
                    options[k - 1].value.clone()
                };
                c = true;
            }
            ui.add_space(GAP);
            panel(ui, "", |ui| {
                c |= toggle_row(
                    ui,
                    submit,
                    "Answers when picked",
                    "A menu: no OK button is needed.",
                    accent,
                )
                .changed();
            });
        }
        Widget::Multi { label, options, .. } => {
            c |= text_in(ui, "Label", "", label, 1);
            c |= options_editor(ui, options_buf, options_for, i, options);
        }
        Widget::Progress { label, value, .. } => {
            c |= text_in(ui, "Label", "", label, 1);
            ui.add_space(GAP);
            let mut busy = *value < 0.0;
            panel(ui, "", |ui| {
                if toggle_row(
                    ui,
                    &mut busy,
                    "Busy",
                    "A moving bar without a number.",
                    accent,
                )
                .changed()
                {
                    *value = if busy { -1.0 } else { 0.0 };
                    c = true;
                }
            });
            if !busy {
                field_label(ui, "Done");
                if slider(ui, value, 0.0..=1.0, accent).changed() {
                    c = true;
                }
            }
            note_line(
                ui,
                "A flow or the extension moves it while the panel is open: give it a name below.",
                pal().muted,
            );
        }
    }
    c
}

/// The options of a list, a line each (`value | label | hint`).
fn options_editor(
    ui: &mut egui::Ui,
    buf: &mut String,
    for_widget: &mut Option<usize>,
    i: usize,
    options: &mut Vec<Opt>,
) -> bool {
    if *for_widget != Some(i) {
        *buf = design::options_to_text(options);
        *for_widget = Some(i);
    }
    field_label(ui, "Options (one per line: value | label | hint)");
    let resp = ui.add(
        text_area(buf, 4)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY),
    );
    if resp.changed() {
        *options = design::text_to_options(buf);
        return true;
    }
    false
}

/// "Show only when": which field the widget waits for, and what it has to hold.
fn show_if_fields(
    ui: &mut egui::Ui,
    spec: &mut PanelSpec,
    i: usize,
    candidates: &[String],
    accent: Color32,
) -> bool {
    ui.add_space(GAP);
    field_label(ui, "Show only when");
    let current = spec.widgets[i].show_if().cloned();
    let mut names: Vec<String> = vec!["Always".to_string()];
    names.extend(candidates.iter().cloned());
    if let Some(c) = &current
        && !names.contains(&c.id)
    {
        names.push(c.id.clone());
    }
    let at = current
        .as_ref()
        .and_then(|c| names.iter().position(|n| *n == c.id))
        .unwrap_or(0);
    let labels: Vec<&str> = names.iter().map(String::as_str).collect();
    let width = ui.available_width();
    let mut changed = false;
    if let Some(k) = select_capped(ui, width, labels[at], &labels, at, 220.0) {
        let condition = (k > 0).then(|| ShowIf {
            id: names[k].clone(),
            is: None,
            not: false,
        });
        spec.widgets[i].set_show_if(condition);
        changed = true;
    }
    let Some(mut cond) = spec.widgets[i].show_if().cloned() else {
        note_line(ui, "The widget is always there.", pal().muted);
        return changed;
    };
    // what the field it waits for is
    let target = spec
        .widgets
        .iter()
        .find(|w| w.id() == Some(cond.id.as_str()));
    let is_toggle = matches!(target, Some(Widget::Toggle { .. }));
    let target_options: Option<Vec<Opt>> = match target {
        Some(Widget::Choice { options, .. } | Widget::Multi { options, .. }) => {
            Some(options.clone())
        }
        _ => None,
    };
    let before = cond.clone();
    if !is_toggle {
        field_label(ui, "It has to");
        let items = [(None, "Have a value"), (None, "Equal")];
        if let Some(k) = segmented(ui, &items, usize::from(cond.is.is_some())) {
            cond.is = if k == 0 {
                None
            } else {
                Some(
                    target_options
                        .as_ref()
                        .and_then(|o| o.first())
                        .map(|o| o.value.clone())
                        .unwrap_or_default(),
                )
            };
        }
        if let Some(value) = cond.is.as_mut() {
            match &target_options {
                Some(options) if !options.is_empty() => {
                    let labels: Vec<&str> = options
                        .iter()
                        .map(|o| {
                            if o.label.is_empty() {
                                o.value.as_str()
                            } else {
                                o.label.as_str()
                            }
                        })
                        .collect();
                    let at = options
                        .iter()
                        .position(|o| o.value == *value)
                        .unwrap_or(usize::MAX);
                    let shown = labels
                        .get(at)
                        .copied()
                        .unwrap_or(value.as_str())
                        .to_string();
                    let width = ui.available_width();
                    if let Some(k) = select_capped(ui, width, &shown, &labels, at, 220.0) {
                        *value = options[k].value.clone();
                    }
                }
                _ => {
                    ui.add(
                        text_field(value)
                            .desired_width(f32::INFINITY)
                            .hint_text("the value"),
                    );
                }
            }
        }
    }
    ui.add_space(GAP);
    let (label, hint) = if is_toggle {
        ("Show when it is off", "Otherwise: when it is on.")
    } else {
        ("Show when this is not so", "Turns the test round.")
    };
    panel(ui, "", |ui| {
        toggle_row(ui, &mut cond.not, label, hint, accent);
    });
    if cond != before {
        spec.widgets[i].set_show_if(Some(cond));
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ctx: &egui::Context, d: &mut PanelDesign) -> bool {
        let mut changed = false;
        ctx.run_ui(Default::default(), |ui| {
            if d.dirty {
                d.refresh();
            }
            changed = draw(ui, d, Color32::from_rgb(60, 130, 246));
        })
        .drop_without_applying_deltas();
        changed
    }

    #[test]
    fn every_template_draws_with_everything_selected_and_shows_a_preview() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        for (i, t) in design::TEMPLATES.iter().enumerate() {
            let text = design::to_flow_text(&design::template_spec(i).unwrap()).unwrap();
            let mut d = PanelDesign::load(0, 1, &text);
            assert!(d.unreadable.is_none(), "{}", t.name);
            let mut selections = vec![Sel::Panel];
            selections.extend((0..d.spec.widgets.len()).map(Sel::Widget));
            selections.extend((0..d.spec.buttons.len()).map(Sel::Button));
            for sel in selections {
                d.select(sel);
                frame(&ctx, &mut d);
                frame(&ctx, &mut d);
                assert!(d.preview.is_some(), "{}: {:?}", t.name, d.problem);
                assert_eq!(d.rects.len(), d.spec.widgets.len(), "{}", t.name);
            }
        }
    }

    #[test]
    fn the_preview_draws_at_every_zoom_without_changing_the_design() {
        // laying the text out again at the zoomed size must neither hang nor touch the design
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let text = design::to_flow_text(&design::template_spec(0).unwrap()).unwrap();
        let mut d = PanelDesign::load(0, 1, &text);
        for zoom in [ZOOM_MIN, 1.0, 1.5, 2.5, ZOOM_MAX] {
            d.zoom = zoom;
            frame(&ctx, &mut d);
            frame(&ctx, &mut d);
            assert!(d.preview.is_some(), "zoom {zoom}: {:?}", d.problem);
        }
        assert_eq!(design::to_flow_text(&d.spec).unwrap(), text);
    }

    #[test]
    fn a_pick_in_the_list_scrolls_the_middle_to_it_but_a_click_on_the_panel_does_not() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let text = design::to_flow_text(&design::template_spec(0).unwrap()).unwrap();
        let mut d = PanelDesign::load(0, 1, &text);
        let last = d.spec.widgets.len() - 1;
        d.select_in_place(Sel::Widget(last));
        assert!(d.show.is_none());
        d.select(Sel::Widget(last));
        assert!(d.show.is_some());
        for _ in 0..=SHOW_TRIES {
            frame(&ctx, &mut d);
        }
        // the request is answered (or given up on) and never leaves the scroll out of range
        assert!(d.show.is_none());
        assert!(d.body_scroll.offset >= 0.0 && d.body_scroll.offset.is_finite());
        d.select(Sel::Panel);
        for _ in 0..=SHOW_TRIES {
            frame(&ctx, &mut d);
        }
        assert_eq!(d.body_scroll.offset, 0.0);
    }

    #[test]
    fn every_kind_can_be_added_and_selected_without_breaking_the_panel() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let mut d = PanelDesign::load(0, 1, "");
        for kind in Kind::ALL {
            assert!(add_widget(&mut d, kind));
            d.dirty = true;
            frame(&ctx, &mut d);
            frame(&ctx, &mut d);
            assert!(d.problem.is_none(), "{kind:?}: {:?}", d.problem);
        }
        assert_eq!(d.spec.widgets.len(), Kind::ALL.len());
        // the outcome is what a flow block keeps, and it reads back
        let text = design::to_flow_text(&d.spec).unwrap();
        assert_eq!(design::from_flow_text(&text).unwrap(), d.spec);
    }

    #[test]
    fn a_text_that_is_not_a_panel_is_kept_until_a_template_replaces_it() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let mut d = PanelDesign::load(0, 1, "{ broken");
        assert!(d.unreadable.is_some());
        frame(&ctx, &mut d);
        // a change from outside (undo) is read again
        d.reload(r#"{"title":"Back"}"#);
        assert!(d.unreadable.is_none());
        assert_eq!(d.spec.title, "Back");
    }

    #[test]
    fn the_buttons_of_the_header_move_copy_and_remove() {
        let mut d = PanelDesign::load(
            0,
            1,
            r#"{"widgets":[{"type":"toggle","id":"a"},{"type":"toggle","id":"b"}],"buttons":[{"id":"x"},{"id":"y"}]}"#,
        );
        assert!(run_act(&mut d, Sel::Widget(0), Act::Down));
        assert_eq!((d.sel, d.spec.widgets[1].id()), (Sel::Widget(1), Some("a")));
        assert!(run_act(&mut d, Sel::Widget(1), Act::Copy));
        assert_eq!(d.spec.widgets.len(), 3);
        assert!(run_act(&mut d, Sel::Widget(2), Act::Remove));
        assert!(!run_act(&mut d, Sel::Widget(0), Act::Up), "already first");
        assert!(run_act(&mut d, Sel::Button(0), Act::Down));
        assert_eq!(d.spec.buttons[1].id, "x");
        assert!(run_act(&mut d, Sel::Button(1), Act::Remove));
        assert!(run_act(&mut d, Sel::Button(0), Act::Remove));
        assert_eq!(d.sel, Sel::Panel, "nothing left to select");
    }
}
