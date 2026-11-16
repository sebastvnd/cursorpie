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
//! * Several widgets can be selected together (Ctrl/Cmd + click, Shift + click for a range, Ctrl/Cmd + A) and
//!   then removed, duplicated, moved, cut, copied and pasted as a group. A copy goes to the system clipboard
//!   (see [`super::clip`]), so it can be pasted into another panel too.
//! * Every change is written back to the block at once, so Settings' undo and redo cover it. When
//!   the block changes behind the designer's back (undo, the JSON box) it reads the block again.
//! * The rules (what a new widget starts as, moving, renaming without breaking a `show_if`, the
//!   templates) are in [`crate::panel::design`] and have tests; this file only draws.
//!
//! A text that is not a panel at all (broken JSON) is left alone until the person picks a template.

use super::clip::{self, Clip};
use super::panel_form;
use super::state::Ed;
use super::theme::*;
use super::widgets::*;
use crate::config::Config;
use crate::config::flow::{AskMode, Flow, NodeKind};
use crate::panel::design::{self, Kind, PasteWidgets};
use crate::panel::layout::{self, card_size, header_height};
use crate::panel::spec::{
    BOX_H_MAX, BOX_H_MIN, BOX_W_MIN, BUTTON_H_MAX, BUTTON_RADIUS_MAX, BUTTON_W_MAX, ButtonSide,
    DEFAULT_TIMEOUT_S, FONT_SIZE_MAX, FONT_SIZE_MIN, HEIGHT_MAX, HEIGHT_MIN, MAX_GROUP_BUTTONS,
    MAX_TIMEOUT_S, NUDGE_MAX, PADDING_MAX, SPACER_DEFAULT, SPACER_MAX, SPACER_MIN, WIDTH_DEFAULT,
    WIDTH_MAX, WIDTH_MIN, default_buttons, format_hex, parse_hex,
};
use crate::panel::{
    Button, ButtonAlign, Form, Icon, Layout as ListLayout, Opt, Padding, PanelSpec, Place, ShowIf,
    TextAlign, TextFmt, TextStyle as Style, TextVAlign, Tone, Widget,
};
use crate::ui::{fonts, symbols};
use eframe::egui::{self, Align, Align2, Color32, FontId, Rect, Sense, Stroke, UiBuilder};
use eframe::egui::{pos2, vec2};
use std::collections::BTreeSet;

/// Width of the column with the list of what the panel holds.
const LEFT_W: f32 = 216.0;
/// Width of the inspector. Wide enough for the color swatches and the four letter toggles to sit
/// on one line, so the cards do not need to wrap or scroll sideways.
const RIGHT_W: f32 = 324.0;
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
    /// A button of a grid that was picked on its own (in the middle, or in the list of the grid):
    /// the widget and the button. Delete and the trash of the bottom bar then remove that button
    /// and not the whole grid. Any other pick of the list or of the middle lets go of it.
    inner: Option<(usize, usize)>,
    /// The widgets selected together (Ctrl/Cmd or Shift + click, Ctrl/Cmd + A). Empty, or two or
    /// more: one widget is `sel` alone, as before. While a group exists `sel` is [`Sel::Panel`],
    /// and the inspector shows the group.
    group: BTreeSet<usize>,
    /// The widget a Shift + click counts from (the last one picked).
    anchor: Option<usize>,
    /// The page one step in that an inspector shows, and for what: for the panel the look of the
    /// title (0), of the subtitle (1) or of the labels (2), or the space inside the card (3); for
    /// an input box the look of the text inside it ([`INPUT_LOOK`]). `None`: the first page.
    /// Picking something else closes it.
    look: Option<(Sel, u8)>,
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
            inner: None,
            group: BTreeSet::new(),
            anchor: None,
            look: None,
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
        self.inner = None;
        self.dirty = true;
        self.sel = match self.sel {
            Sel::Widget(i) if i < self.spec.widgets.len() => Sel::Widget(i),
            Sel::Button(i) if i < self.spec.buttons.len() => Sel::Button(i),
            _ => Sel::Panel,
        };
        // a group that lost widgets (undo, the JSON box) shrinks to what is still there
        let len = self.spec.widgets.len();
        self.group.retain(|i| *i < len);
        if self.group.len() < 2 {
            if let Some(only) = self.group.iter().next().copied() {
                self.sel = Sel::Widget(only);
            }
            self.group.clear();
        }
    }

    /// Whether widget `i` is selected, alone or in the group.
    fn widget_on(&self, i: usize) -> bool {
        self.sel == Sel::Widget(i) || self.group.contains(&i)
    }

    /// The widgets that are selected, in the order they have in the panel.
    fn selected_widgets(&self) -> Vec<usize> {
        if !self.group.is_empty() {
            return self.group.iter().copied().collect();
        }
        match self.sel {
            Sel::Widget(i) => vec![i],
            _ => Vec::new(),
        }
    }

    /// Selects exactly these widgets: none, one (the one the inspector shows) or a group.
    fn set_widgets(&mut self, set: BTreeSet<usize>) {
        let len = self.spec.widgets.len();
        let set: BTreeSet<usize> = set.into_iter().filter(|i| *i < len).collect();
        match set.len() {
            0 => {
                self.group.clear();
                self.select_in_place(Sel::Panel);
            }
            1 => {
                let only = set.into_iter().next().unwrap_or(0);
                self.select_in_place(Sel::Widget(only));
            }
            _ => {
                self.group = set;
                self.sel = Sel::Panel;
                self.options_for = None;
                self.inner = None;
            }
        }
    }

    /// A click on widget `i` (in the list or on the panel) with the keys that were held:
    /// Ctrl/Cmd adds it to what is selected or takes it out, Shift selects everything between the
    /// last one picked and it, and without a key it is the one selected. `scroll`: the middle
    /// brings it into view (a pick in the list).
    fn click_widget(&mut self, i: usize, shift: bool, command: bool, scroll: bool) {
        if !shift && !command {
            self.anchor = Some(i);
            if scroll {
                self.select(Sel::Widget(i));
            } else {
                self.select_in_place(Sel::Widget(i));
            }
            return;
        }
        let current: BTreeSet<usize> = self.selected_widgets().into_iter().collect();
        let last = self.spec.widgets.len().saturating_sub(1);
        let anchor = self
            .anchor
            .or_else(|| current.iter().next().copied())
            .unwrap_or(i)
            .min(last);
        let set = pick_set(&current, anchor, i, shift, command);
        if command {
            self.anchor = Some(i);
        } else if self.anchor.is_none() {
            self.anchor = Some(anchor);
        }
        self.set_widgets(set);
    }

    /// Selects something and has the middle scroll to it (a pick in the list, a new widget).
    fn select(&mut self, sel: Sel) {
        self.select_in_place(sel);
        self.inner = None;
        self.show = Some((sel, SHOW_TRIES));
    }

    /// Selects something that is already in view (a click on the panel itself): nothing scrolls.
    fn select_in_place(&mut self, sel: Sel) {
        self.group.clear();
        if self.sel != sel {
            self.sel = sel;
            self.options_for = None;
            self.inner = None;
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
                    // a button the person tried in the preview keeps its look while something
                    // else is edited, unless the edit is the look it starts in
                    if let Some(old) = &self.preview {
                        let ids: Vec<String> = shown
                            .all_buttons()
                            .filter(|b| {
                                b.toggle
                                    && old
                                        .shown
                                        .find_button(&b.id)
                                        .is_some_and(|o| o.toggle && o.on == b.on)
                            })
                            .map(|b| b.id.clone())
                            .collect();
                        for id in ids {
                            if old.form.is_on(&id) {
                                form.on.insert(id);
                            } else {
                                form.on.remove(&id);
                            }
                        }
                    }
                    self.preview = Some(Preview { form, shown });
                }
            }
            Err(e) => self.problem = Some(e.to_string()),
        }
    }
}

/// What is selected after a click on widget `i` with Ctrl/Cmd (it joins `current`, or leaves it
/// when it was in) or with Shift (everything from `anchor` to `i`), and without a key (just `i`).
fn pick_set(
    current: &BTreeSet<usize>,
    anchor: usize,
    i: usize,
    shift: bool,
    command: bool,
) -> BTreeSet<usize> {
    if command {
        let mut set = current.clone();
        if !set.remove(&i) {
            set.insert(i);
        }
        set
    } else if shift {
        (anchor.min(i)..=anchor.max(i)).collect()
    } else {
        BTreeSet::from([i])
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
    // Delete or Backspace removes what is selected, like on the canvas of a flow, unless a field is
    // being typed in or a list is open
    let ctx = ui.ctx().clone();
    if !ctx.egui_wants_keyboard_input()
        && !egui::Popup::is_any_open(&ctx)
        && ctx.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        })
    {
        changed |= delete_selected(d);
    }
    // Ctrl/Cmd + A selects every widget; Ctrl/Cmd + C copies the selected ones, X cuts them and V
    // pastes after them (through the system clipboard, so into another panel too)
    if !ctx.egui_wants_keyboard_input() && !egui::Popup::is_any_open(&ctx) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A)) {
            d.set_widgets((0..d.spec.widgets.len()).collect());
        }
        if let Some(clipped) = clip::take(&ctx) {
            changed |= run_clip(&ctx, d, clipped);
        }
    }
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
    // a button picked in the list of a grid (the inspector says so through the context)
    let pick_key = egui::Id::new(INNER_PICK);
    if let Some((w, j)) = ctx.data(|dd| dd.get_temp::<(usize, usize)>(pick_key)) {
        ctx.data_mut(|dd| dd.insert_temp(pick_key, (usize::MAX, 0usize)));
        if d.sel == Sel::Widget(w) {
            d.inner = Some((w, j));
        }
    }
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
    menu_popup_at(trigger, width, egui::RectAlign::BOTTOM_START, add_contents)
}

/// [`menu_popup`] where the menu opens at `align` of the trigger (over it, for a trigger at the
/// bottom of the sheet).
fn menu_popup_at<R>(
    trigger: &egui::Response,
    width: f32,
    align: egui::RectAlign,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    egui::Popup::from_toggle_button_response(trigger)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(align)
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
    /// It was let go over the trash of the bottom bar.
    trashed: bool,
}

/// Where the pick of a button of a grid is passed from the inspector to the designer.
const INNER_PICK: &str = "panel_design_inner_pick";

/// Where the trash of the bottom bar was drawn last (to tell, when something is let go, whether it
/// landed there), and whether a row or a widget is being dragged now (the trash lights up).
fn trash_key() -> egui::Id {
    egui::Id::new("panel_design_trash")
}
fn drag_key() -> egui::Id {
    egui::Id::new("panel_design_dragging")
}

/// Whether the pointer is over the trash of the bottom bar.
fn pointer_on_trash(ctx: &egui::Context) -> bool {
    let at = ctx.input(|i| i.pointer.latest_pos());
    let zone = ctx.data(|d| d.get_temp::<Rect>(trash_key()));
    matches!((at, zone), (Some(p), Some(r)) if r.contains(p))
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
        ui.ctx().data_mut(|d| d.insert_temp(drag_key(), true));
    }
    let trashed = resp.drag_stopped() && pointer_on_trash(ui.ctx());
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
        .filter(|_| !trashed)
        .map(|p| (p.index, lower));
    RowOut {
        clicked: resp.clicked(),
        dropped,
        trashed,
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

/// The kinds of widget to add: two even columns of one-line rows, read left to right. Returns the
/// one picked (and closes the menu it is in).
fn kind_menu(ui: &mut egui::Ui, accent: Color32) -> Option<Kind> {
    let mut picked = None;
    ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
    for pair in Kind::ALL.chunks(2) {
        ui.columns(2, |cols| {
            for (col, kind) in cols.iter_mut().zip(pair) {
                if list_row(col, kind_icon(*kind), kind.label(), false, accent)
                    .on_hover_text(kind.hint())
                    .clicked()
                {
                    picked = Some(*kind);
                    col.close();
                }
            }
        });
    }
    picked
}

/// Removes what is selected: the widgets of a group, the button of a grid that was picked on its
/// own, else the widget or the button at the bottom (the panel itself stays).
fn delete_selected(d: &mut PanelDesign) -> bool {
    if !d.group.is_empty() {
        let at: Vec<usize> = d.group.iter().copied().collect();
        d.note = None;
        let next = design::remove_widgets(&mut d.spec, &at);
        d.select(next.map_or(Sel::Panel, Sel::Widget));
        return true;
    }
    if let Some((w, j)) = d.inner
        && remove_inner(d, w, j)
    {
        return true;
    }
    match d.sel {
        Sel::Panel => false,
        sel => run_act(d, sel, Act::Remove),
    }
}

/// Removes button `j` of the grid `w`.
fn remove_inner(d: &mut PanelDesign, w: usize, j: usize) -> bool {
    d.inner = None;
    let Some(Widget::Buttons { buttons, .. }) = d.spec.widgets.get_mut(w) else {
        return false;
    };
    if j >= buttons.len() {
        return false;
    }
    buttons.remove(j);
    d.note = None;
    true
}

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
        let added = menu_popup(&add, 292.0, |ui| kind_menu(ui, accent));
        if let Some(Some(kind)) = added {
            changed |= add_widget(d, kind);
        }

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
                    d.inner = None;
                    d.group.clear();
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
    // a row clicked with Ctrl/Cmd or Shift held: (the widget, shift, command)
    let mut group_pick: Option<(usize, bool, bool)> = None;
    // a row let go over the trash of the bottom bar
    let mut trashed: Option<Sel> = None;
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
                    d.widget_on(i),
                    accent,
                    LIST_WIDGETS,
                    i,
                );
                if out.clicked {
                    let (shift, command) =
                        ui.input(|inp| (inp.modifiers.shift, inp.modifiers.command));
                    if shift || command {
                        group_pick = Some((i, shift, command));
                    } else {
                        pick = Some(Sel::Widget(i));
                    }
                }
                if let Some((from, after)) = out.dropped {
                    moved = Some((true, from, i, after));
                }
                if out.trashed {
                    trashed = Some(Sel::Widget(i));
                }
            }
            ui.add_space(GAP);
            section_title(ui, "Buttons");
            if design::uses_default_buttons(&d.spec) {
                note_line(ui, "Cancel and OK, until you add your own.", pal().muted);
            } else if d.spec.buttons.is_empty() {
                note_line(
                    ui,
                    "None: the X at the top right closes the panel.",
                    pal().muted,
                );
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
                if out.trashed {
                    trashed = Some(Sel::Button(i));
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
    if let Some(sel) = trashed {
        // a row of the group let go over the trash takes the whole group with it
        changed |= if matches!(sel, Sel::Widget(i) if d.group.contains(&i)) {
            delete_selected(d)
        } else {
            run_act(d, sel, Act::Remove)
        };
        pick = None;
        group_pick = None;
    }
    if let Some((i, shift, command)) = group_pick {
        d.click_widget(i, shift, command, true);
    }
    if let Some(sel) = pick {
        if let Sel::Widget(i) = sel {
            d.anchor = Some(i);
        }
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
    // what the bottom bar asked for, a widget let go over its trash, a button of a grid picked
    let bar_act: BarAct;
    let mut trash_widget: Option<usize> = None;
    let mut inner_pick: Option<(usize, usize)> = None;
    // a widget of the panel clicked with Ctrl/Cmd or Shift held: (the widget, shift, command)
    let mut group_click: Option<(usize, bool, bool)> = None;
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
            inner: grid_pick,
            group,
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
        // the pinch of a trackpad on Linux (egui does not get it there)
        let pinch = pinch * crate::platform::take_pinch(&ctx);
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
        let inner = panel_form::inner_rect(card, &p.shown);
        let head = Rect::from_min_size(inner.min, vec2(inner.width(), header_height(&p.shown)));
        let foot_top = inner.max.y - layout::footer_height(&p.shown);

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
                let (shift, command) = ui.input(|inp| (inp.modifiers.shift, inp.modifiers.command));
                if shift || command {
                    group_click = Some((i, shift, command));
                } else {
                    next = Some(Sel::Widget(i));
                }
            }
        }
        // a button that changes when pressed does it here too, so what the person designed can be
        // tried (the press also selects it, like any other button)
        if let Some(id) = &pressed
            && p.form
                .spec
                .find_button(id)
                .is_some_and(|b| b.toggle && !b.cancel)
            && !p.form.on.remove(id)
        {
            p.form.on.insert(id.clone());
        }
        if let Some(id) = pressed {
            // a button of a grid is picked on its own too (Delete removes just that one)
            inner_pick = spec.widgets.iter().enumerate().find_map(|(wi, w)| match w {
                Widget::Buttons { buttons, .. } => {
                    buttons.iter().position(|b| b.id == id).map(|j| (wi, j))
                }
                _ => None,
            });
            if let Some((wi, j)) = inner_pick {
                // the list of the grid in the inspector shows it as the one picked
                ctx.data_mut(|dd| {
                    dd.insert_temp(egui::Id::new(("panel_design_grid_button", wi)), j)
                });
            }
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
        // every widget of a group has its outline
        for i in group.iter() {
            if let Some(r) = rects.get(*i).copied().filter(|r| r.is_finite()) {
                painter.rect_stroke(
                    (to_screen * r).expand(5.0),
                    8.0,
                    Stroke::new(2.0, accent),
                    egui::StrokeKind::Outside,
                );
            }
        }
        let outline = match *sel {
            Sel::Widget(i) => rects.get(i).copied().filter(|r| r.is_finite()),
            Sel::Panel if !group.is_empty() => None,
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
        // a grip beside the selected widget: drag it up or down to move the widget, or onto the
        // trash of the bottom bar to remove it
        let mut grip_dragging = false;
        if let Sel::Widget(si) = *sel
            && let Some(r) = rects.get(si).copied().filter(|r| r.is_finite())
        {
            let sr = to_screen * r;
            // always inside the preview, so it can be reached however far the panel is zoomed or
            // scrolled (a widget that is out of view has its grip at the edge)
            let gx = (on_screen.min.x - 16.0).max(rect.min.x + 12.0);
            let gy = sr.center().y.max(rect.min.y + 24.0).min(rect.max.y - 72.0);
            let grip = Rect::from_center_size(pos2(gx, gy), vec2(20.0, 30.0));
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
            grip_dragging = resp.dragged();
            if resp.dragged() || resp.drag_stopped() {
                let pointer = ctx.input(|i| i.pointer.latest_pos());
                let on_trash = pointer_on_trash(&ctx);
                let slot = pointer
                    .and_then(|p| drop_slot(rects, (to_screen.inverse() * p).y))
                    .filter(|_| !on_trash);
                if resp.drag_stopped() && on_trash {
                    trash_widget = Some(si);
                }
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
                        to_screen * pos2(inner.min.x, y),
                        to_screen * pos2(inner.max.x, y),
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
        // what a click on the trash of the bottom bar removes (None: nothing is selected)
        let remove = match (*grid_pick, *sel) {
            (Some(_), _) => Some("Remove the selected button (Delete)"),
            (None, Sel::Panel) if !group.is_empty() => {
                Some("Remove the selected elements (Delete)")
            }
            (None, Sel::Panel) => None,
            (None, Sel::Widget(_)) => Some("Remove the selected element (Delete)"),
            (None, Sel::Button(_)) => Some("Remove the selected button (Delete)"),
        };
        let dragging = grip_dragging
            || ctx
                .data(|dd| dd.get_temp::<bool>(drag_key()))
                .unwrap_or(false);
        ctx.data_mut(|dd| dd.insert_temp(drag_key(), false));
        bar_act = zoom_bar(ui, rect, zoom, problem.as_deref(), dragging, remove, accent);
    }
    if let Some(sel) = next {
        // a click on the panel itself: it is already in view
        if let Sel::Widget(i) = sel {
            d.anchor = Some(i);
        }
        d.select_in_place(sel);
    }
    if let Some((i, shift, command)) = group_click {
        d.click_widget(i, shift, command, false);
    }
    if let Some(pick) = inner_pick {
        d.inner = Some(pick);
    }
    match bar_act {
        BarAct::Add(kind) => changed |= add_widget(d, kind),
        BarAct::Delete => changed |= delete_selected(d),
        BarAct::None => {}
    }
    if let Some(si) = trash_widget {
        changed |= run_act(d, Sel::Widget(si), Act::Remove);
    }
    changed
}

/// What the bottom bar asks for.
enum BarAct {
    None,
    /// A widget of this kind was picked in its "Add element" menu.
    Add(Kind),
    /// The trash was clicked: remove what is selected.
    Delete,
}

/// The small capsule at the bottom middle of the preview: add an element, a trash (a click removes
/// what is selected; a row of the list or the widget being dragged by its grip is removed when
/// let go over it), zoom out, the zoom (a press goes back to 100%), zoom in. When the panel is not valid, the reason is shown right above the capsule.
///
/// It is an area of its own, above everything: the panel in the middle is a layer of its own (so it
/// can be zoomed) and is drawn over whatever the page itself draws, so a capsule drawn in the page
/// went under the panel when this was tall. An area is also centered by the toolkit, whatever the
/// width of what is in it.
///
/// The reason is an area of its own too, stacked over the capsule and wrapped to the width of the
/// preview: drawn in the page at the bottom it was covered by the capsule, and a long reason was cut
/// at the edges instead of wrapping.
fn zoom_bar(
    ui: &mut egui::Ui,
    rect: Rect,
    zoom: &mut f32,
    problem: Option<&str>,
    dragging: bool,
    remove: Option<&str>,
    accent: Color32,
) -> BarAct {
    /// The room the capsule takes from the bottom of the preview (its height and the gap below it).
    const BAR_ROOM: f32 = 20.0 + 40.0;
    if let Some(text) = problem {
        egui::Area::new(egui::Id::new("panel_design_problem"))
            .order(egui::Order::Tooltip)
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(pos2(rect.center().x, rect.max.y - BAR_ROOM - 6.0))
            .constrain(false)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                ui.set_max_width((rect.width() - 32.0).max(120.0));
                egui::Frame::new()
                    .fill(pal().bg)
                    .corner_radius(PILL)
                    .inner_margin(egui::Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(text).size(12.5).color(pal().bad))
                                .wrap(),
                        );
                    });
            });
    }
    let mut act = BarAct::None;
    egui::Area::new(egui::Id::new("panel_design_zoom_bar"))
        .order(egui::Order::Tooltip)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(rect.center().x, rect.max.y - 14.0))
        .constrain(false)
        .show(ui.ctx(), |ui| {
            let out = egui::Frame::new()
                .fill(pal().card)
                .stroke(Stroke::new(1.0, pal().border))
                .corner_radius(PILL)
                .inner_margin(egui::Margin::same(4))
                .shadow(popup_shadow())
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // add an element: a round "+" (its menu opens over the bar, below)
                        let add = round_button(
                            ui,
                            Look::Primary(accent),
                            symbols::PLUS,
                            "Add element",
                            true,
                            CONTROL_H,
                        );
                        // the trash: a click removes what is selected, and a row or a widget
                        // dragged here is removed when let go
                        let (cell, resp) =
                            ui.allocate_exact_size(vec2(CONTROL_H, CONTROL_H), Sense::click());
                        ui.ctx()
                            .data_mut(|d| d.insert_temp(trash_key(), cell.expand(6.0)));
                        let over = dragging && pointer_on_trash(ui.ctx());
                        let fill = if over {
                            wash(pal().bad, 0.32)
                        } else if dragging {
                            wash(pal().bad, 0.16)
                        } else if resp.hovered() && remove.is_some() {
                            wash(pal().bad, 0.12)
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect_filled(cell, PILL, fill);
                        if dragging {
                            ui.painter().rect_stroke(
                                cell,
                                PILL,
                                Stroke::new(1.5, pal().bad),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let ink = if dragging || remove.is_some() {
                            pal().bad
                        } else {
                            pal().muted
                        };
                        fonts::paint_icon(
                            ui.painter(),
                            cell.center(),
                            symbols::TRASH,
                            17.0,
                            ink,
                            false,
                        );
                        let tip = remove.unwrap_or("Drag an element here to remove it");
                        if resp.on_hover_text(tip).clicked() && remove.is_some() {
                            act = BarAct::Delete;
                        }
                        let (bar, _) = ui.allocate_exact_size(vec2(1.0, 20.0), Sense::hover());
                        ui.painter().rect_filled(bar, 0.0, pal().border);
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
                        add
                    })
                    .inner
                });
            // the menu is anchored to the whole capsule, not to the "+" in it, so it opens
            // centered over the bar (egui places a popup by `interact_rect` too)
            let mut anchor = out.inner;
            anchor.rect = out.response.rect;
            anchor.interact_rect = out.response.rect;
            let picked = menu_popup_at(&anchor, 292.0, egui::RectAlign::TOP, |ui| {
                kind_menu(ui, accent)
            });
            if let Some(Some(kind)) = picked {
                act = BarAct::Add(kind);
            }
        });
    act
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
    if d.look.is_some_and(|(sel, _)| sel != d.sel) || !d.group.is_empty() {
        d.look = None;
    }
    if !d.group.is_empty() {
        return group_column(ui, d, accent);
    }
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

/// Carries out a cut, copy or paste of the keyboard on the selected widgets. Returns whether the
/// panel changed.
fn run_clip(ctx: &egui::Context, d: &mut PanelDesign, clipped: Clip) -> bool {
    let cut = matches!(clipped, Clip::Cut);
    match clipped {
        Clip::Copy | Clip::Cut => {
            let at = d.selected_widgets();
            let Some(text) = design::copy_widgets(&d.spec, &at) else {
                return false;
            };
            ctx.copy_text(text);
            if cut {
                // the widgets go, not the one button of a grid that may be picked
                d.inner = None;
                return delete_selected(d);
            }
            false
        }
        Clip::Paste(text) => {
            let at = d
                .selected_widgets()
                .last()
                .map_or(d.spec.widgets.len(), |i| *i + 1);
            d.note = None;
            match design::paste_widgets(&mut d.spec, at, &text) {
                PasteWidgets::Done(now) => {
                    if let Some(first) = now.first().copied() {
                        d.show = Some((Sel::Widget(first), SHOW_TRIES));
                    }
                    d.set_widgets(now.into_iter().collect());
                    true
                }
                PasteWidgets::Full => {
                    d.note = Some(format!(
                        "A panel has at most {} widgets.",
                        crate::panel::spec::MAX_WIDGETS
                    ));
                    false
                }
                PasteWidgets::NotWidgets => false,
            }
        }
    }
}

/// Carries out the move, duplicate or removal that the header of the inspector asked for on a
/// group of widgets.
fn run_group(d: &mut PanelDesign, act: Act) -> bool {
    d.note = None;
    let at: Vec<usize> = d.group.iter().copied().collect();
    match act {
        Act::Up | Act::Down => {
            let delta = if act == Act::Up { -1 } else { 1 };
            match design::move_widgets(&mut d.spec, &at, delta) {
                Some(now) => {
                    d.group = now.into_iter().collect();
                    true
                }
                None => false,
            }
        }
        Act::Copy => match design::duplicate_widgets(&mut d.spec, &at) {
            Some(now) => {
                if let Some(first) = now.first().copied() {
                    d.show = Some((Sel::Widget(first), SHOW_TRIES));
                }
                d.set_widgets(now.into_iter().collect());
                true
            }
            None => {
                d.note = Some(format!(
                    "A panel has at most {} widgets.",
                    crate::panel::spec::MAX_WIDGETS
                ));
                false
            }
        },
        Act::Remove => delete_selected(d),
    }
}

/// The inspector while several widgets are selected: what they are, and what can be done with all of
/// them at once.
fn group_column(ui: &mut egui::Ui, d: &mut PanelDesign, accent: Color32) -> bool {
    let at: Vec<usize> = d.group.iter().copied().collect();
    let len = d.spec.widgets.len();
    let can_up = at.iter().any(|i| *i > 0 && !d.group.contains(&(*i - 1)));
    let can_down = at
        .iter()
        .any(|i| *i + 1 < len && !d.group.contains(&(*i + 1)));
    let mut act = None;
    ui.horizontal(|ui| {
        ui.label(fonts::title(
            format!("{} elements", at.len()),
            15.0,
            pal().text,
        ));
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if icon_button_look(ui, Look::Danger, symbols::TRASH, "Remove", true).clicked() {
                act = Some(Act::Remove);
            }
            if icon_button_look(ui, Look::Plain, symbols::COPY, "Duplicate", true).clicked() {
                act = Some(Act::Copy);
            }
            if icon_button_look(ui, Look::Plain, symbols::ARROW_DOWN, "Move down", can_down)
                .clicked()
            {
                act = Some(Act::Down);
            }
            if icon_button_look(ui, Look::Plain, symbols::ARROW_UP, "Move up", can_up).clicked() {
                act = Some(Act::Up);
            }
        });
    });
    let changed = act.is_some_and(|act| run_group(d, act));
    if let Some(note) = d.note.clone() {
        note_line(ui, &note, pal().bad);
    }
    note_line(
        ui,
        "Ctrl/Cmd + click adds or removes one, Shift + click picks a range, Ctrl/Cmd + A picks all. \
         Ctrl/Cmd + C copies, X cuts and V pastes (into another panel too). Click one below to work on it alone.",
        pal().muted,
    );
    ui.add_space(4.0);
    let mut alone = None;
    egui::ScrollArea::vertical()
        .id_salt("panel_design_group")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for i in at {
                let Some(w) = d.spec.widgets.get(i) else {
                    continue;
                };
                let kind = design::kind_of(w);
                let mut text = design::summary(w);
                if text.is_empty() {
                    text = kind.label().to_string();
                }
                if list_row(ui, kind_icon(kind), &text, true, accent).clicked() {
                    alone = Some(i);
                }
            }
        });
    if let Some(i) = alone {
        d.anchor = Some(i);
        d.select(Sel::Widget(i));
    }
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
            // taking away the last one leaves the panel without buttons (the X still closes it)
            // instead of bringing Cancel and OK back
            if d.spec.buttons.is_empty() {
                d.spec.no_buttons = true;
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

/// The inspector of the panel itself, in four small cards: what the header says, how the card looks,
/// how big it is and how it behaves. What is touched only now and then (how the three texts are
/// written and the space inside the card) is one step in, behind a row that sums it up, so the
/// first screen stays short and every line on it is one thing to decide.
fn panel_fields(ui: &mut egui::Ui, d: &mut PanelDesign, accent: Color32) -> bool {
    if let Some((Sel::Panel, which)) = d.look {
        return look_page(ui, d, which, accent);
    }
    let mut c = false;
    let mut open = None;
    let spec = &mut d.spec;

    panel(ui, "Header", |ui| {
        c |= field(ui, "Title", |ui| {
            ui.add(
                text_field(&mut spec.title)
                    .desired_width(f32::INFINITY)
                    .hint_text("Shown at the top"),
            )
            .changed()
        });
        c |= field(ui, "Subtitle", |ui| {
            ui.add(
                text_field(&mut spec.subtitle)
                    .desired_width(f32::INFINITY)
                    .hint_text("A line under the title"),
            )
            .changed()
        });
        c |= row_wide(ui, "Icon", "", CHOICE_W, |ui| {
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
            match select(ui, width, names[at], &names, at) {
                Some(i) => {
                    spec.icon = icons[i].0;
                    true
                }
                None => false,
            }
        });
    });

    panel(ui, "Look", |ui| {
        let [r, g, b, _] = accent.to_array();
        let usual = [r, g, b];
        let mut swatches: Vec<(&str, [u8; 3])> = vec![("Color of Settings", usual)];
        swatches.extend(ACCENTS.iter().filter(|(_, rgb)| *rgb != usual).copied());
        let mut slot = spec.accent.as_deref().and_then(parse_hex);
        if field(ui, "Badge and main button", |ui| {
            color_dots(ui, &mut slot, &swatches)
        }) {
            spec.accent = slot.map(format_hex);
            c = true;
        }
        // the looks of the texts: one row each, with what is set now on the right
        let rows = [("Title", 0u8), ("Subtitle", 1), ("Field labels", 2)];
        for (name, which) in rows {
            let (fmt, range) = look_of(spec, which);
            if nav_row(ui, name, &fmt_summary(fmt, &range)) {
                open = Some(which);
            }
        }
        let now = layout::insets(spec);
        let sides = [now.top, now.right, now.bottom, now.left];
        let summary = if sides.iter().all(|x| *x == sides[0]) {
            format!("{} pt", sides[0].round())
        } else {
            "Custom".to_string()
        };
        if nav_row(ui, "Inner space", &summary) {
            open = Some(3);
        }
    });

    panel(ui, "Size and place", |ui| {
        c |= row_wide(ui, "Appears", "", CHOICE_W, |ui| {
            let places = [Place::Cursor, Place::Center, Place::Top, Place::Bottom];
            let names = ["Cursor", "Center", "Top", "Bottom"];
            let at = places.iter().position(|p| *p == spec.place).unwrap_or(0);
            let width = ui.available_width();
            match select(ui, width, names[at], &names, at) {
                Some(i) => {
                    spec.place = places[i];
                    true
                }
                None => false,
            }
        });
        let mut w = spec.width.unwrap_or(WIDTH_DEFAULT);
        if slider_row(ui, "Width", &mut w, WIDTH_MIN..=WIDTH_MAX, " pt", accent).changed() {
            spec.width = Some(w.round());
            c = true;
        }
        // height: as tall as the content needs (the default), or a fixed one
        let fixed = spec.height.is_some();
        c |= row_wide(ui, "Height", "", CHOICE_W, |ui| {
            let items = [(None, "Auto"), (None, "Fixed")];
            match segmented(ui, &items, usize::from(fixed)) {
                Some(i) => {
                    spec.height = if i == 1 {
                        // start from the height the card has now, so switching does not make it jump
                        Some(card_size(spec).1.clamp(HEIGHT_MIN, HEIGHT_MAX).round())
                    } else {
                        None
                    };
                    true
                }
                None => false,
            }
        });
        if let Some(mut h) = spec.height
            && slider_row(
                ui,
                "Fixed height",
                &mut h,
                HEIGHT_MIN..=HEIGHT_MAX,
                " pt",
                accent,
            )
            .changed()
        {
            spec.height = Some(h.round());
            c = true;
        }
    });

    panel(ui, "Behavior", |ui| {
        let mut show_close = !spec.hide_close;
        if toggle_row(
            ui,
            &mut show_close,
            "Close button",
            "A small X at the top right closes the panel.",
            accent,
        )
        .changed()
        {
            spec.hide_close = !show_close;
            c = true;
        }
        c |= toggle_row(
            ui,
            &mut spec.keep_open,
            "Stay open",
            "Stays after a button press. Give it a name.",
            accent,
        )
        .changed();
        c |= toggle_row(
            ui,
            &mut spec.restore,
            "Remember values",
            "A flow shows every field with the variable of its answer name (a slider stays where it was left).",
            accent,
        )
        .changed();
        c |= row_wide(ui, "Gives up after", "0 means never", 110.0, |ui| {
            let mut secs = spec.timeout_s.unwrap_or(DEFAULT_TIMEOUT_S);
            let resp = ui.add_sized(
                [ui.available_width(), CONTROL_H],
                egui::DragValue::new(&mut secs)
                    .range(0..=MAX_TIMEOUT_S)
                    .suffix(" s"),
            );
            if resp.changed() {
                spec.timeout_s = Some(secs);
            }
            resp.changed()
        });
        c |= row_wide(ui, "Buttons", "", CHOICE_W, |ui| {
            let aligns = [
                ButtonAlign::End,
                ButtonAlign::Start,
                ButtonAlign::Center,
                ButtonAlign::Stretch,
            ];
            let names = ["Right", "Left", "Center", "Wide"];
            let at = aligns
                .iter()
                .position(|a| *a == spec.buttons_align)
                .unwrap_or(0);
            let width = ui.available_width();
            match select(ui, width, names[at], &names, at) {
                Some(k) => {
                    spec.buttons_align = aligns[k];
                    true
                }
                None => false,
            }
        });
        c |= field(ui, "Name (optional)", |ui| {
            ui.add(
                text_field(&mut spec.id)
                    .desired_width(f32::INFINITY)
                    .hint_text("so a flow or the extension can update it"),
            )
            .changed()
        });
    });

    if design::uses_default_buttons(spec) {
        note_line(
            ui,
            "The buttons are Cancel and OK. The X at the top right closes the panel too.",
            pal().muted,
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if button(ui, Look::Soft, None, "Customize the buttons").clicked() {
                let menu = spec
                    .widgets
                    .iter()
                    .any(|w| matches!(w, Widget::Choice { submit: true, .. }));
                spec.buttons = default_buttons(menu);
                d.sel = Sel::Button(0);
                c = true;
            }
            if button(ui, Look::Plain, None, "No buttons").clicked() {
                spec.no_buttons = true;
                c = true;
            }
        });
    } else if spec.buttons.is_empty() {
        note_line(
            ui,
            "No buttons: the X at the top right closes the panel. Add your own on the left.",
            pal().muted,
        );
        ui.add_space(4.0);
        if button(ui, Look::Soft, None, "Use Cancel and OK").clicked() {
            spec.no_buttons = false;
            c = true;
        }
    }
    if let Some(which) = open {
        d.look = Some((Sel::Panel, which));
    }
    c
}

/// How wide the control of a row is when it is a list or a choice of two.
const CHOICE_W: f32 = 140.0;

/// The page one step in: the look of one text (0 title, 1 subtitle, 2 labels) or the space inside
/// the card (3), with a way back.
fn look_page(ui: &mut egui::Ui, d: &mut PanelDesign, which: u8, accent: Color32) -> bool {
    let mut c = false;
    let name = match which {
        0 => "Title",
        1 => "Subtitle",
        2 => "Field labels",
        _ => "Inner space",
    };
    ui.horizontal(|ui| {
        if icon_button_look(ui, Look::Plain, symbols::ARROW_LEFT, "Back", true).clicked() {
            d.look = None;
        }
        ui.label(fonts::title(name, 14.0, pal().text));
    });
    ui.add_space(4.0);
    if which < 3 {
        let (fmt, range) = look_of(&mut d.spec, which);
        c |= text_format_fields(ui, fmt, &range, accent);
    } else {
        c |= padding_fields(ui, &mut d.spec);
    }
    c
}

/// The format of one of the three texts of the panel, with what it can change.
fn look_of(spec: &mut PanelSpec, which: u8) -> (&mut TextFmt, FmtRange) {
    match which {
        0 => (
            &mut spec.title_fmt,
            FmtRange {
                usual: layout::TITLE_SIZE,
                least: 12.0,
                most: layout::TITLE_SIZE_MAX,
                bold: true,
                extra: FmtExtra::Header,
            },
        ),
        1 => (
            &mut spec.subtitle_fmt,
            FmtRange {
                usual: layout::SUBTITLE_SIZE,
                least: f32::from(FONT_SIZE_MIN),
                most: layout::SUBTITLE_SIZE_MAX,
                bold: false,
                extra: FmtExtra::Header,
            },
        ),
        _ => (
            &mut spec.label_fmt,
            FmtRange {
                usual: layout::LABEL_SIZE,
                least: f32::from(FONT_SIZE_MIN),
                most: layout::LABEL_SIZE_MAX,
                bold: true,
                extra: FmtExtra::None,
            },
        ),
    }
}

/// A look in a few words for the row that opens it: "Bold, 22 pt", "Regular, italic, 13 pt".
fn fmt_summary(fmt: &TextFmt, range: &FmtRange) -> String {
    let mut parts = vec![if fmt.bold.unwrap_or(range.bold) {
        "Bold".to_string()
    } else {
        "Regular".to_string()
    }];
    if fmt.italic {
        parts.push("italic".into());
    }
    if fmt.underline {
        parts.push("underline".into());
    }
    if fmt.strike {
        parts.push("strike".into());
    }
    parts.push(format!(
        "{} pt",
        fmt.size.map_or(range.usual, f32::from).round()
    ));
    parts.join(", ")
}

/// Whether something was already added to `ui`, so the next field starts with a hairline.
fn has_content(ui: &egui::Ui) -> bool {
    ui.cursor().min.y > ui.min_rect().min.y + 0.5
}

/// A pale line across a card, between two fields.
fn rule(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, pal().divider);
}

/// A field of a card whose control is as wide as the card: the name small and muted above it, a
/// hairline from the field before. Returns what `add_control` returns.
fn field<R>(ui: &mut egui::Ui, label: &str, add_control: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if has_content(ui) {
        rule(ui);
    }
    ui.add_space(10.0);
    ui.label(egui::RichText::new(label).size(13.0).color(pal().muted));
    ui.add_space(6.0);
    let out = add_control(ui);
    ui.add_space(12.0);
    out
}

/// A row of a card that opens a page one step in: the name on the left, what is set now and a
/// caret on the right. The whole row is the click target. Returns whether it was clicked.
fn nav_row(ui: &mut egui::Ui, label: &str, summary: &str) -> bool {
    if has_content(ui) {
        rule(ui);
    }
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_H + 18.0), Sense::click());
    let hot = resp.hovered();
    let painter = ui.painter();
    if hot {
        painter.rect_filled(rect.shrink2(vec2(-6.0, 5.0)), CONTROL_R, pal().idle);
    }
    painter.text(
        pos2(rect.min.x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.5),
        pal().text,
    );
    fonts::paint_icon(
        painter,
        pos2(rect.max.x - 8.0, rect.center().y),
        symbols::CARET_RIGHT,
        16.0,
        pal().muted,
        false,
    );
    painter.text(
        pos2(rect.max.x - 24.0, rect.center().y),
        Align2::RIGHT_CENTER,
        summary,
        FontId::proportional(13.0),
        pal().muted,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Toggles of equal width on one line, each lit with the accent while on. Returns the one clicked.
fn chips(ui: &mut egui::Ui, items: &[(&str, bool)], accent: Color32) -> Option<usize> {
    let gap = 6.0;
    let n = items.len().max(1) as f32;
    let width = ((ui.available_width() - gap * (n - 1.0)) / n).floor();
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, (label, on)) in items.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(width, CONTROL_H - 4.0), Sense::click());
            let fill = if *on {
                wash(accent, 0.16)
            } else if resp.hovered() {
                pal().hot
            } else {
                pal().idle
            };
            ui.painter().rect_filled(rect, CONTROL_R, fill);
            let fg = if *on { accent_text(accent) } else { pal().text };
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                *label,
                text_font(12.5, *on),
                fg,
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                picked = Some(i);
            }
        }
    });
    picked
}

/// Round color swatches that wrap to the width they are given, and after them the system color
/// picker. `swatches` are `(name, rgb)`; the first is the color that is used while nothing is
/// chosen: it is the lit one while `slot` is `None`, and picking it empties `slot` again.
fn color_dots(ui: &mut egui::Ui, slot: &mut Option<[u8; 3]>, swatches: &[(&str, [u8; 3])]) -> bool {
    let mut changed = false;
    let default = swatches.first().map_or([0; 3], |s| s.1);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (i, (name, rgb)) in swatches.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
            let c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            let on = if i == 0 {
                slot.is_none()
            } else {
                *slot == Some(*rgb)
            };
            let painter = ui.painter();
            painter.circle_filled(rect.center(), 10.0, c);
            // the pale colors are as light as the card: a hairline keeps them visible
            painter.circle_stroke(rect.center(), 10.0, Stroke::new(1.0, pal().border));
            if on {
                painter.circle_stroke(rect.center(), 13.0, Stroke::new(2.0, accent_text(c)));
                fonts::paint_icon(
                    painter,
                    rect.center(),
                    symbols::CHECK,
                    13.0,
                    on_accent(c),
                    false,
                );
            }
            let tip = if i == 0 {
                format!("{name} (the usual)")
            } else {
                (*name).to_string()
            };
            if resp
                .on_hover_text(tip)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *slot = (i != 0).then_some(*rgb);
                changed = true;
            }
        }
        let mut rgb = slot.unwrap_or(default);
        if ui
            .color_edit_button_srgb(&mut rgb)
            .on_hover_text("Any color")
            .changed()
        {
            *slot = Some(rgb);
            changed = true;
        }
    });
    changed
}

/// A label and a number of points for one edge of the padding.
fn pad_box(ui: &mut egui::Ui, label: &str, v: &mut f64) -> bool {
    field_label(ui, label);
    ui.add_sized(
        [ui.available_width(), CONTROL_H],
        egui::DragValue::new(v)
            .range(0.0..=f64::from(PADDING_MAX))
            .speed(0.5)
            .max_decimals(0)
            .suffix(" pt"),
    )
    .changed()
}

/// Stores one side of the padding: the usual space is not written.
fn set_pad(slot: &mut Option<f32>, v: f64) {
    let v = v as f32;
    *slot = ((v - layout::PAD).abs() >= 0.5).then_some(v.round());
}

/// The space between the edge of the card and its content: one number for every side, two (left
/// and right, top and bottom) or one for each side, so that a single side can be changed too.
fn padding_fields(ui: &mut egui::Ui, spec: &mut PanelSpec) -> bool {
    let now = layout::insets(spec);
    let before = [now.top, now.right, now.bottom, now.left].map(f64::from);
    let mut v = before;
    // the least the sides need to be shown as they are: 0 one number, 1 two, 2 four
    let needed: u8 = if v.iter().all(|x| *x == v[0]) {
        0
    } else if v[0] == v[2] && v[1] == v[3] {
        1
    } else {
        2
    };
    let key = egui::Id::new("panel_design_padding_mode");
    let mut mode = ui.data(|d| d.get_temp::<u8>(key)).unwrap_or(0).max(needed);
    panel(ui, "", |ui| {
        field(ui, "Which sides", |ui| {
            let items = [(None, "All"), (None, "H and V"), (None, "Each side")];
            if let Some(k) = segmented(ui, &items, usize::from(mode)) {
                mode = k as u8;
                // a mode with fewer numbers makes the sides alike again
                if mode == 0 {
                    v = [v[0]; 4];
                } else if mode == 1 {
                    v[2] = v[0];
                    v[3] = v[1];
                }
            }
        });
        match mode {
            0 => {
                if pad_box(ui, "All sides", &mut v[0]) {
                    v = [v[0]; 4];
                }
            }
            1 => {
                let (h, t) = ui.columns(2, |cols| {
                    let h = pad_box(&mut cols[0], "Left and right", &mut v[1]);
                    let t = pad_box(&mut cols[1], "Top and bottom", &mut v[0]);
                    (h, t)
                });
                if h {
                    v[3] = v[1];
                }
                if t {
                    v[2] = v[0];
                }
            }
            _ => {
                ui.columns(2, |cols| {
                    pad_box(&mut cols[0], "Top", &mut v[0]);
                    pad_box(&mut cols[1], "Right", &mut v[1]);
                });
                ui.columns(2, |cols| {
                    pad_box(&mut cols[0], "Bottom", &mut v[2]);
                    pad_box(&mut cols[1], "Left", &mut v[3]);
                });
            }
        }
        ui.add_space(8.0);
    });
    ui.data_mut(|d| d.insert_temp(key, mode));
    let mut c = false;
    if v != before {
        let p = &mut spec.padding;
        set_pad(&mut p.top, v[0]);
        set_pad(&mut p.right, v[1]);
        set_pad(&mut p.bottom, v[2]);
        set_pad(&mut p.left, v[3]);
        c = true;
    }
    if spec.padding != Padding::default()
        && button(
            ui,
            Look::Plain,
            Some(symbols::ARROW_COUNTER_CLOCKWISE),
            "Usual space",
        )
        .clicked()
    {
        spec.padding = Padding::default();
        c = true;
    }
    c
}

fn button_fields(ui: &mut egui::Ui, d: &mut PanelDesign, i: usize, accent: Color32) -> bool {
    let mut c = false;
    let Some(b) = d.spec.buttons.get_mut(i) else {
        return false;
    };
    c |= one_button_fields(ui, b, true, accent);
    c
}

/// The icon of a button: picked from the icons of the app, or typed (a name, or `file:` and the
/// path of a picture).
fn icon_fields(ui: &mut egui::Ui, icon: &mut String) -> bool {
    icon_fields_titled(ui, "Icon", "No icon", icon)
}

/// [`icon_fields`] for the icon a toggle shows after a press: empty keeps the first one.
fn icon_fields_after(ui: &mut egui::Ui, icon: &mut String) -> bool {
    icon_fields_titled(ui, "Icon after a press", "Same icon", icon)
}

/// A box to type in, as a field of a card: the name above it and a hairline from the field before.
fn text_field_in(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    text: &mut String,
    rows: usize,
) -> bool {
    field(ui, label, |ui| {
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
    })
}

/// A choice among a few, as a [`field`] of a card. Returns the index that was clicked.
fn choice_field(
    ui: &mut egui::Ui,
    label: &str,
    items: &[(Option<char>, &str)],
    at: usize,
) -> Option<usize> {
    field(ui, label, |ui| segmented(ui, items, at))
}

/// Two numbers side by side with their names above, as a field of a card. Returns which of the
/// two was changed.
fn pair_field(
    ui: &mut egui::Ui,
    label_a: &str,
    a: &mut f64,
    label_b: &str,
    b: &mut f64,
) -> (bool, bool) {
    if has_content(ui) {
        rule(ui);
    }
    ui.add_space(6.0);
    let out = number_pair(ui, label_a, a, label_b, b);
    ui.add_space(10.0);
    out
}

/// A number of points that says "auto" at 0, as a [`DragValue`](egui::DragValue).
fn auto_drag(v: &mut f64, most: f64) -> egui::DragValue<'_> {
    egui::DragValue::new(v)
        .range(0.0..=most)
        .speed(1.0)
        .custom_formatter(|n, _| {
            if n < 1.0 {
                "auto".to_owned()
            } else {
                format!("{n:.0} pt")
            }
        })
        .custom_parser(|t| {
            let t = t.trim().trim_end_matches("pt").trim();
            if t.eq_ignore_ascii_case("auto") {
                Some(0.0)
            } else {
                t.parse::<f64>().ok()
            }
        })
}

/// A row of a card with an "auto" number on the right and what auto means under the name.
fn auto_row(ui: &mut egui::Ui, label: &str, hint: &str, v: &mut f64, most: f64) -> bool {
    row_wide(ui, label, hint, 110.0, |ui| {
        let width = ui.available_width();
        ui.add_sized([width, CONTROL_H], auto_drag(v, most))
            .changed()
    })
}

fn icon_fields_titled(
    ui: &mut egui::Ui,
    title: &str,
    none: &'static str,
    icon: &mut String,
) -> bool {
    let mut c = false;
    c |= field(ui, title, |ui| {
        let name = icon
            .strip_prefix(symbols::PREFIX)
            .unwrap_or(icon.as_str())
            .to_string();
        let mut items: Vec<(Option<char>, &str)> = vec![(None, none)];
        items.extend(symbols::ALL.iter().map(|(n, ch)| (Some(*ch), *n)));
        let at = symbols::ALL
            .iter()
            .position(|(n, _)| *n == name)
            .map_or(0, |p| p + 1);
        let width = ui.available_width();
        match select_icons(ui, width, &items, at) {
            Some(k) => {
                *icon = if k == 0 {
                    String::new()
                } else {
                    symbols::ALL[k - 1].0.to_string()
                };
                true
            }
            None => false,
        }
    });
    c |= text_field_in(
        ui,
        "Icon name, or a picture",
        "play, or file:/path/to/picture.png",
        icon,
        1,
    );
    c
}

/// Everything a button has, at the bottom or in a grid, in cards: what it says and looks like,
/// what it does, what it shows after a press, and its size.
fn one_button_fields(ui: &mut egui::Ui, b: &mut Button, in_footer: bool, accent: Color32) -> bool {
    let mut c = false;
    panel(ui, "Button", |ui| {
        c |= text_field_in(
            ui,
            "Label",
            "What it says (empty: only the icon)",
            &mut b.label,
            1,
        );
        c |= text_field_in(
            ui,
            "Answer name",
            "The flow gets this when it is pressed",
            &mut b.id,
            1,
        );
        c |= icon_fields(ui, &mut b.icon);
        let tones = [Tone::Secondary, Tone::Primary, Tone::Danger];
        let at = tones.iter().position(|t| *t == b.tone).unwrap_or(0);
        let items = [(None, "Normal"), (None, "Main"), (None, "Danger")];
        if let Some(i) = choice_field(ui, "Look", &items, at) {
            b.tone = tones[i];
            c = true;
        }
    });
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
        if !b.cancel {
            c |= toggle_row(
                ui,
                &mut b.toggle,
                "Changes when pressed",
                "Shows another icon, text or color after a press, and goes back at the next one.",
                accent,
            )
            .changed();
        }
    });
    if b.toggle && !b.cancel {
        panel(ui, "After a press", |ui| {
            c |= text_field_in(
                ui,
                "Text after a press",
                "Empty: the same text",
                &mut b.on_label,
                1,
            );
            c |= icon_fields_after(ui, &mut b.on_icon);
            let tones = [
                None,
                Some(Tone::Secondary),
                Some(Tone::Primary),
                Some(Tone::Danger),
            ];
            let at = tones.iter().position(|t| *t == b.on_tone).unwrap_or(0);
            let items = [
                (None, "Same"),
                (None, "Normal"),
                (None, "Main"),
                (None, "Danger"),
            ];
            if let Some(i) = choice_field(ui, "Look after a press", &items, at) {
                b.on_tone = tones[i];
                c = true;
            }
            c |= toggle_row(
                ui,
                &mut b.on,
                "Starts changed",
                if b.bind.is_empty() {
                    "The panel opens with the button already showing how it looks after a press."
                } else {
                    "How it opens while the variable below has no value yet."
                },
                accent,
            )
            .changed();
        });
        panel(ui, "Kept by the flow", |ui| {
            // a variable name: letters, digits and _, so a wrong key is never typed in
            if text_field_in(
                ui,
                "Remember in variable",
                "Empty: only while the panel is open",
                &mut b.bind,
                1,
            ) {
                b.bind = crate::config::flow::clean_var_name(&b.bind);
                c = true;
            }
            note_line(
                ui,
                "The button opens with the look this variable says (1 = after a press, 0 = \
                 before) and every press writes it back. A name that starts with keep_ is also \
                 saved between runs.",
                pal().muted,
            );
        });
    }
    c |= size_and_shape(ui, b, in_footer);
    c
}

/// Where a button sits (at the bottom), how wide and how tall it is, and how round its corners are.
fn size_and_shape(ui: &mut egui::Ui, b: &mut Button, in_footer: bool) -> bool {
    let mut c = false;
    panel(ui, "Size and shape", |ui| {
        if in_footer {
            let sides = [
                None,
                Some(ButtonSide::Start),
                Some(ButtonSide::Center),
                Some(ButtonSide::End),
            ];
            let at = sides.iter().position(|x| *x == b.side).unwrap_or(0);
            let items = [
                (None, "Panel"),
                (None, "Left"),
                (None, "Center"),
                (None, "Right"),
            ];
            if let Some(i) = choice_field(ui, "Place in the row", &items, at) {
                b.side = sides[i];
                c = true;
            }
        }
        let mut w = f64::from(b.width);
        if auto_row(
            ui,
            "Width",
            "As the text needs",
            &mut w,
            f64::from(BUTTON_W_MAX),
        ) {
            b.width = w as u16;
            c = true;
        }
        let mut h = f64::from(b.height);
        if auto_row(ui, "Height", "The usual", &mut h, f64::from(BUTTON_H_MAX)) {
            b.height = h as u16;
            c = true;
        }
        c |= row_wide(ui, "Corners", "", CHOICE_W, |ui| {
            let radii = [None, Some(0), Some(8), Some(14), Some(BUTTON_RADIUS_MAX)];
            let at = radii.iter().position(|r| *r == b.radius);
            let mut names = vec!["Usual", "Square", "Soft", "Round", "Pill"];
            // a roundness that is none of the five has a name of its own, and its number below
            if at.is_none() {
                names.push("Custom");
            }
            let shown = at.unwrap_or(names.len() - 1);
            let width = ui.available_width();
            match select(ui, width, names[shown], &names, shown) {
                Some(i) if i < radii.len() => {
                    b.radius = radii[i];
                    true
                }
                _ => false,
            }
        });
        if let Some(r) = b.radius {
            let mut r = f64::from(r);
            let changed = row_wide(ui, "Roundness", "", 110.0, |ui| {
                let width = ui.available_width();
                ui.add_sized(
                    [width, CONTROL_H],
                    egui::DragValue::new(&mut r)
                        .range(0.0..=f64::from(BUTTON_RADIUS_MAX))
                        .speed(1.0)
                        .max_decimals(0)
                        .suffix(" pt"),
                )
                .changed()
            });
            if changed {
                b.radius = Some(r as u8);
                c = true;
            }
        }
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
    // a row let go over the trash of the bottom bar
    let mut trashed = None;
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
                // picked on its own: Delete removes this button, not the grid
                ui.ctx()
                    .data_mut(|d| d.insert_temp(egui::Id::new(INNER_PICK), (grid, i)));
            }
            if let Some((from, after)) = out.dropped {
                dropped = Some((from, i, after));
            }
            if out.trashed {
                trashed = Some(i);
            }
        }
    });
    if let Some(i) = trashed
        && i < buttons.len()
    {
        buttons.remove(i);
        sel = sel.min(buttons.len().saturating_sub(1));
        c = true;
    }
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
        c |= one_button_fields(ui, b, false, accent);
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
    if let Some((Sel::Widget(w), INPUT_LOOK)) = d.look
        && w == i
    {
        return input_look_page(ui, d, i, accent);
    }
    let mut c = false;
    let mut open_look = false;
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
        c |= type_fields(
            ui,
            w,
            i,
            options_buf,
            options_for,
            &taken_buttons,
            accent,
            &mut open_look,
        );
    }
    if open_look {
        d.look = Some((Sel::Widget(i), INPUT_LOOK));
    }
    c |= id_field(ui, &mut d.spec, i);
    c |= show_if_fields(ui, &mut d.spec, i, &candidates, accent);
    c
}

/// The page one step in of an input box: how the text inside it looks.
const INPUT_LOOK: u8 = 4;

/// What the text inside an input box can change.
fn box_range() -> FmtRange {
    FmtRange {
        usual: layout::BOX_TEXT_SIZE,
        least: f32::from(FONT_SIZE_MIN),
        most: f32::from(FONT_SIZE_MAX),
        bold: false,
        extra: FmtExtra::Box,
    }
}

/// The look of the text inside the input box `i` (the label above it follows the panel's label
/// look), with a way back.
fn input_look_page(ui: &mut egui::Ui, d: &mut PanelDesign, i: usize, accent: Color32) -> bool {
    let mut c = false;
    ui.horizontal(|ui| {
        if icon_button_look(ui, Look::Plain, symbols::ARROW_LEFT, "Back", true).clicked() {
            d.look = None;
        }
        ui.label(fonts::title("Text inside the box", 14.0, pal().text));
    });
    ui.add_space(4.0);
    if let Some(Widget::Input {
        value_fmt, secret, ..
    }) = d.spec.widgets.get_mut(i)
    {
        c |= text_format_fields(ui, value_fmt, &box_range(), accent);
        if *secret {
            note_line(
                ui,
                "A password shows dots: slant and lines do not apply to them.",
                pal().muted,
            );
        }
    }
    c
}

/// What the format of one text can change, and where it starts from.
struct FmtRange {
    /// The size when none is asked for, and the smallest and the biggest that can be asked for.
    usual: f32,
    least: f32,
    most: f32,
    /// Whether the text is bold when nothing is asked for (a heading, a title, a label).
    bold: bool,
    /// What else can be set: where the text sits in its box, and a nudge from where it was put.
    extra: FmtExtra,
}

/// The controls beyond the common ones that a place for text offers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FmtExtra {
    /// Nothing more (a paragraph, the labels).
    None,
    /// A nudge to either side and up or down, and a button that centers on the whole card (the
    /// title and the subtitle share the header with the badge, so their "center" is a little off).
    Header,
    /// A nudge, and where the text sits in the height of its box (the text inside an input box).
    Box,
}

/// The controls of a [`TextFmt`]: where the text sits, bold, italic, underline, strike-through,
/// size and color, in one card, and a second card for the fine position when the place has one.
/// What is left alone stays out of the panel's JSON.
fn text_format_fields(
    ui: &mut egui::Ui,
    fmt: &mut TextFmt,
    range: &FmtRange,
    accent: Color32,
) -> bool {
    let mut c = false;
    panel(ui, "", |ui| {
        c |= field(ui, "Position", |ui| {
            let aligns = [TextAlign::Left, TextAlign::Center, TextAlign::Right];
            let at = aligns.iter().position(|a| *a == fmt.align).unwrap_or(0);
            let items = [(None, "Left"), (None, "Center"), (None, "Right")];
            match segmented(ui, &items, at) {
                Some(k) => {
                    fmt.align = aligns[k];
                    true
                }
                None => false,
            }
        });

        c |= field(ui, "Letters", |ui| {
            let bold = fmt.bold.unwrap_or(range.bold);
            let marks = [
                ("Bold", bold),
                ("Italic", fmt.italic),
                ("Underline", fmt.underline),
                ("Strike", fmt.strike),
            ];
            match chips(ui, &marks, accent) {
                Some(k) => {
                    let on = marks[k].1;
                    match k {
                        // only what differs from the usual weight is written down
                        0 => fmt.bold = (!on != range.bold).then_some(!on),
                        1 => fmt.italic = !on,
                        2 => fmt.underline = !on,
                        _ => fmt.strike = !on,
                    }
                    true
                }
                None => false,
            }
        });

        // the size: its name and number on one line, a way back to the usual one, the slider under
        if has_content(ui) {
            rule(ui);
        }
        ui.add_space(10.0);
        let mut size = fmt
            .size
            .map_or(range.usual, f32::from)
            .clamp(range.least, range.most);
        let set = fmt.size.is_some();
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Size").size(13.0).color(pal().muted));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if set
                    && ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new("Reset")
                                    .size(13.0)
                                    .color(accent_text(accent)),
                            )
                            .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                {
                    fmt.size = None;
                    c = true;
                }
                let shown = if set {
                    format!("{} pt", size.round())
                } else {
                    format!("{} pt, usual", range.usual.round())
                };
                ui.label(egui::RichText::new(shown).size(13.0).color(pal().text));
            });
        });
        ui.add_space(6.0);
        if slider(ui, &mut size, range.least..=range.most, accent).changed() {
            fmt.size = Some(size.round().clamp(range.least, range.most) as u8);
            c = true;
        }
        ui.add_space(4.0);

        c |= field(ui, "Color", |ui| {
            let [cr, cg, cb, _] = pal().text.to_array();
            let mut swatches: Vec<(&str, [u8; 3])> = vec![("Usual", [cr, cg, cb])];
            swatches.extend(ACCENTS.iter().copied());
            let mut slot = fmt.rgb();
            let changed = color_dots(ui, &mut slot, &swatches);
            if changed {
                fmt.color = slot.map(format_hex);
            }
            changed
        });
    });

    if range.extra != FmtExtra::None {
        c |= text_place_fields(ui, fmt, range.extra);
    }

    if !fmt.is_plain()
        && button(
            ui,
            Look::Plain,
            Some(symbols::ARROW_COUNTER_CLOCKWISE),
            "Reset style",
        )
        .clicked()
    {
        *fmt = TextFmt::default();
        c = true;
    }
    c
}

/// How far the header's text sits from the left edge of the text area compared with the middle of
/// the whole card (the round badge and its gap are in front of the text): `dx` of this much puts
/// a centered title or subtitle in the exact middle of the card.
const HEADER_CENTER_DX: i16 = 0;

/// Where the text sits: in the height of its box and a nudge in points, for the places that
/// are not laid out by their alignment alone.
fn text_place_fields(ui: &mut egui::Ui, fmt: &mut TextFmt, extra: FmtExtra) -> bool {
    let mut c = false;
    panel(ui, "Fine position", |ui| {
        if extra == FmtExtra::Box {
            c |= field(ui, "Up and down in the box", |ui| {
                let mut r = false;
                let aligns = [TextVAlign::Top, TextVAlign::Middle, TextVAlign::Bottom];
                // missing: what the box does by itself, which the first look of the panel shows
                let at = fmt.valign.and_then(|v| aligns.iter().position(|a| *a == v));
                let items = [(None, "Top"), (None, "Middle"), (None, "Bottom")];
                if let Some(k) = segmented(ui, &items, at.unwrap_or(usize::MAX)) {
                    fmt.valign = Some(aligns[k]);
                    r = true;
                }
                if fmt.valign.is_some()
                    && button(ui, Look::Plain, None, "Use the usual position").clicked()
                {
                    fmt.valign = None;
                    r = true;
                }
                r
            });
        }

        c |= field(ui, "Nudge", |ui| {
            let mut r = false;
            let mut dx = f64::from(fmt.dx);
            let mut dy = f64::from(fmt.dy);
            let lim = f64::from(NUDGE_MAX);
            let (dx_c, dy_c) = ui.columns(2, |cols| {
                let wa = cols[0].available_width();
                let a = cols[0]
                    .add_sized(
                        [wa, CONTROL_H],
                        egui::DragValue::new(&mut dx)
                            .range(-lim..=lim)
                            .speed(0.5)
                            .max_decimals(0)
                            .prefix("X  ")
                            .suffix(" pt"),
                    )
                    .changed();
                let wb = cols[1].available_width();
                let b = cols[1]
                    .add_sized(
                        [wb, CONTROL_H],
                        egui::DragValue::new(&mut dy)
                            .range(-lim..=lim)
                            .speed(0.5)
                            .max_decimals(0)
                            .prefix("Y  ")
                            .suffix(" pt"),
                    )
                    .changed();
                (a, b)
            });
            if dx_c {
                fmt.dx = (dx.round() as i16).clamp(-NUDGE_MAX, NUDGE_MAX);
                r = true;
            }
            if dy_c {
                fmt.dy = (dy.round() as i16).clamp(-NUDGE_MAX, NUDGE_MAX);
                r = true;
            }
            ui.add_space(4.0);
            note_line(
                ui,
                "X moves the text right, Y moves it down. A number below zero goes the other way.",
                pal().muted,
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if extra == FmtExtra::Header
                    && button(ui, Look::Soft, None, "Center on the card").clicked()
                {
                    fmt.align = TextAlign::Center;
                    fmt.dx = HEADER_CENTER_DX;
                    r = true;
                }
                if (fmt.dx != 0 || fmt.dy != 0) && button(ui, Look::Plain, None, "Reset").clicked()
                {
                    fmt.dx = 0;
                    fmt.dy = 0;
                    r = true;
                }
            });
            r
        });
    });
    c
}

/// Width and height of an input box, and the space between its edge and its text.
fn box_size_fields(
    ui: &mut egui::Ui,
    multiline: bool,
    width: &mut Option<f32>,
    height: &mut Option<f32>,
    pad: &mut Padding,
) -> bool {
    let mut c = false;
    panel(ui, "Size of the box", |ui| {
        let mut w = f64::from(width.unwrap_or(0.0));
        let mut h = f64::from(height.unwrap_or(0.0));
        // from 0 ("auto") the first step jumps to the least that makes sense
        if auto_row(
            ui,
            "Width",
            "As wide as the card",
            &mut w,
            f64::from(WIDTH_MAX),
        ) {
            *width = (w >= 1.0).then(|| (w as f32).clamp(BOX_W_MIN, WIDTH_MAX).round());
            c = true;
        }
        if auto_row(ui, "Height", "The usual", &mut h, f64::from(BOX_H_MAX)) {
            *height = (h >= 1.0).then(|| (h as f32).clamp(BOX_H_MIN, BOX_H_MAX).round());
            c = true;
        }

        // the usual space: the sides, and above and below (a line or several lines)
        let base_y = if multiline {
            layout::BOX_PAD_Y_MANY
        } else {
            layout::BOX_PAD_Y_ONE
        };
        let shown = |own: Option<f32>, usual: f32| f64::from(own.unwrap_or(usual));
        let mut v = [
            shown(pad.top, base_y),
            shown(pad.right, layout::BOX_PAD_X),
            shown(pad.bottom, base_y),
            shown(pad.left, layout::BOX_PAD_X),
        ];
        let before = v;
        field(ui, "Space between the edge and the text", |ui| {
            ui.columns(2, |cols| {
                pad_box(&mut cols[0], "Top", &mut v[0]);
                pad_box(&mut cols[1], "Right", &mut v[1]);
            });
            ui.columns(2, |cols| {
                pad_box(&mut cols[0], "Bottom", &mut v[2]);
                pad_box(&mut cols[1], "Left", &mut v[3]);
            });
        });
        if v != before {
            // only what differs from the usual space is written down
            let keep = |x: f64, usual: f32| {
                ((x as f32 - usual).abs() >= 0.5).then_some((x as f32).round())
            };
            pad.top = keep(v[0], base_y);
            pad.right = keep(v[1], layout::BOX_PAD_X);
            pad.bottom = keep(v[2], base_y);
            pad.left = keep(v[3], layout::BOX_PAD_X);
            c = true;
        }
        if (*pad != Padding::default() || width.is_some() || height.is_some())
            && button(
                ui,
                Look::Plain,
                Some(symbols::ARROW_COUNTER_CLOCKWISE),
                "Usual size and space",
            )
            .clicked()
        {
            *pad = Padding::default();
            *width = None;
            *height = None;
            c = true;
        }
        ui.add_space(6.0);
    });
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
    let (title, label, hint) = if field {
        ("Answer", "Answer name", "letters, digits and _")
    } else {
        (
            "Name",
            "Name (optional)",
            "so a flow can change it while the panel is open",
        )
    };
    panel(ui, title, |ui| {
        let changed = text_field_in(ui, label, hint, &mut id, 1);
        if changed {
            design::rename_widget(spec, i, &id);
        }
        if field && !id.is_empty() {
            note_line(ui, &format!("In the flow: {{{{{id}}}}}"), pal().muted);
            ui.add_space(10.0);
        }
        changed
    })
}

#[allow(clippy::too_many_arguments)]
fn type_fields(
    ui: &mut egui::Ui,
    w: &mut Widget,
    i: usize,
    options_buf: &mut String,
    options_for: &mut Option<usize>,
    taken_buttons: &BTreeSet<String>,
    accent: Color32,
    open_look: &mut bool,
) -> bool {
    let mut c = false;
    match w {
        Widget::Text {
            text, style, fmt, ..
        } => {
            panel(ui, "Text", |ui| {
                c |= text_field_in(ui, "Text", "", text, 4);
                let styles = [Style::Body, Style::Heading, Style::Muted, Style::Code];
                let at = styles.iter().position(|s| s == style).unwrap_or(0);
                let items = [
                    (None, "Text"),
                    (None, "Heading"),
                    (None, "Muted"),
                    (None, "Code"),
                ];
                if let Some(k) = choice_field(ui, "Style", &items, at) {
                    *style = styles[k];
                    c = true;
                }
            });
            let range = FmtRange {
                usual: layout::style_size(*style),
                least: f32::from(FONT_SIZE_MIN),
                most: f32::from(FONT_SIZE_MAX),
                bold: *style == Style::Heading,
                extra: FmtExtra::None,
            };
            c |= text_format_fields(ui, fmt, &range, accent);
        }
        Widget::Separator { .. } => note_line(ui, "A thin line between two groups.", pal().muted),
        Widget::Spacer { height, .. } => {
            panel(ui, "", |ui| {
                let mut h = f64::from(height.unwrap_or(SPACER_DEFAULT));
                let changed = row_wide(ui, "Height", "", 110.0, |ui| {
                    let width = ui.available_width();
                    ui.add_sized(
                        [width, CONTROL_H],
                        egui::DragValue::new(&mut h)
                            .range(f64::from(SPACER_MIN)..=f64::from(SPACER_MAX))
                            .speed(1.0)
                            .max_decimals(0)
                            .suffix(" pt"),
                    )
                    .changed()
                });
                if changed {
                    *height = Some(h as f32);
                    c = true;
                }
            });
        }
        Widget::Buttons {
            label,
            columns,
            buttons,
            align,
            ..
        } => {
            panel(ui, "Grid", |ui| {
                c |= text_field_in(ui, "Label (optional)", "", label, 1);
                let items = [
                    (None, "Auto"),
                    (None, "1"),
                    (None, "2"),
                    (None, "3"),
                    (None, "4"),
                    (None, "5"),
                    (None, "6"),
                ];
                if let Some(k) =
                    choice_field(ui, "Buttons per row", &items, usize::from(*columns).min(6))
                {
                    *columns = k as u8;
                    c = true;
                }
                let sides = [ButtonSide::Start, ButtonSide::Center, ButtonSide::End];
                let items = [(None, "Left"), (None, "Middle"), (None, "Right")];
                let at = sides
                    .iter()
                    .position(|x| *x == align.unwrap_or(ButtonSide::Center))
                    .unwrap_or(1);
                if let Some(k) = choice_field(ui, "A narrow button sits", &items, at) {
                    *align = (sides[k] != ButtonSide::Center).then_some(sides[k]);
                    c = true;
                }
            });
            section_title(ui, "Buttons (drag a row to move it)");
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
            value_fmt,
            width,
            height,
            text_padding,
            ..
        } => {
            panel(ui, "Input", |ui| {
                c |= text_field_in(ui, "Label", "", label, 1);
                c |= text_field_in(ui, "Hint inside the box", "", hint, 1);
                c |= text_field_in(ui, "Starts with", "", value, 1);
            });
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
            c |= box_size_fields(ui, *multiline, width, height, text_padding);
            // how the text inside the box looks (the label above it follows the panel's label look)
            panel(ui, "", |ui| {
                if nav_row(
                    ui,
                    "Text inside the box",
                    &fmt_summary(value_fmt, &box_range()),
                ) {
                    *open_look = true;
                }
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
            panel(ui, "Number", |ui| {
                c |= text_field_in(ui, "Label", "", label, 1);
                c |= text_field_in(ui, "Unit", "px, min, ...", unit, 1);
                let mut lo = min.unwrap_or(0.0);
                let mut hi = max.unwrap_or(100.0);
                let (lo_c, hi_c) = pair_field(ui, "Smallest", &mut lo, "Largest", &mut hi);
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
                let (v_c, s_c) = pair_field(ui, "Starts at", &mut v, "Step (+ and -)", &mut s);
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
                ui.add_space(6.0);
            });
        }
        Widget::Toggle { label, value, .. } => {
            panel(ui, "Switch", |ui| {
                c |= text_field_in(ui, "Label", "", label, 1);
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
            panel(ui, "Slider", |ui| {
                c |= text_field_in(ui, "Label", "", label, 1);
                c |= text_field_in(ui, "Unit", "%, s, ...", unit, 1);
                let (lo_c, hi_c) = pair_field(ui, "Smallest", min, "Largest", max);
                c |= lo_c || hi_c;
                let mut s = step.unwrap_or(1.0);
                let mut v = value.unwrap_or(*min);
                let (s_c, v_c) = pair_field(ui, "Step", &mut s, "Starts at", &mut v);
                if s_c {
                    *step = Some(s.max(0.001));
                    c = true;
                }
                if v_c {
                    *value = Some(v);
                    c = true;
                }
            });
            if *min >= *max {
                note_line(ui, "The smallest has to be below the largest.", pal().bad);
            }
        }
        Widget::Color { label, value, .. } => {
            panel(ui, "Color", |ui| {
                c |= text_field_in(ui, "Label", "", label, 1);
                let usual = [0x3b, 0x82, 0xf6];
                let mut swatches: Vec<(&str, [u8; 3])> = vec![("Blue", usual)];
                swatches.extend(ACCENTS.iter().filter(|(_, rgb)| *rgb != usual).copied());
                let mut slot = parse_hex(value).filter(|rgb| *rgb != usual);
                let changed = field(ui, "Starts as", |ui| color_dots(ui, &mut slot, &swatches));
                if changed || value.is_empty() {
                    *value = format_hex(slot.unwrap_or(usual));
                    c = true;
                }
            });
        }
        Widget::Choice {
            label,
            options,
            value,
            layout,
            submit,
            ..
        } => {
            panel(ui, "Choice", |ui| {
                c |= text_field_in(ui, "Label", "", label, 1);
                let layouts = [ListLayout::List, ListLayout::Chips, ListLayout::Dropdown];
                let at = layouts.iter().position(|l| l == layout).unwrap_or(0);
                let items = [(None, "List"), (None, "Chips"), (None, "Dropdown")];
                if let Some(k) = choice_field(ui, "Shown as", &items, at) {
                    *layout = layouts[k];
                    c = true;
                }
            });
            panel(ui, "Options", |ui| {
                c |= options_editor(ui, options_buf, options_for, i, options);
                let picked = field(ui, "Picked at the start", |ui| {
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
                    select_capped(ui, width, names[at], &names, at, 220.0)
                });
                if let Some(k) = picked {
                    *value = if k == 0 {
                        String::new()
                    } else {
                        options[k - 1].value.clone()
                    };
                    c = true;
                }
            });
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
            panel(ui, "Multiple choice", |ui| {
                c |= text_field_in(ui, "Label", "", label, 1);
            });
            panel(ui, "Options", |ui| {
                c |= options_editor(ui, options_buf, options_for, i, options);
            });
        }
        Widget::Progress { label, value, .. } => {
            panel(ui, "Progress", |ui| {
                c |= text_field_in(ui, "Label", "", label, 1);
                let mut busy = *value < 0.0;
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
                if !busy {
                    c |= field(ui, "Done", |ui| {
                        slider(ui, value, 0.0..=1.0, accent).changed()
                    });
                }
            });
            note_line(
                ui,
                "A flow or the extension moves it while the panel is open: give it a name below.",
                pal().muted,
            );
        }
    }
    c
}

/// The options of a list, a line each (`value | label | hint`), as a field of a card.
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
    let changed = field(ui, "Options (one per line: value | label | hint)", |ui| {
        ui.add(
            text_area(buf, 4)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY),
        )
        .changed()
    });
    if changed {
        *options = design::text_to_options(buf);
    }
    changed
}

/// "Show only when": which field the widget waits for, and what it has to hold.
fn show_if_fields(
    ui: &mut egui::Ui,
    spec: &mut PanelSpec,
    i: usize,
    candidates: &[String],
    accent: Color32,
) -> bool {
    panel(ui, "Visibility", |ui| {
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
        let mut changed = false;
        let picked = field(ui, "Show only when", |ui| {
            let width = ui.available_width();
            select_capped(ui, width, labels[at], &labels, at, 220.0)
        });
        if let Some(k) = picked {
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
            ui.add_space(10.0);
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
            let items = [(None, "Have a value"), (None, "Equal")];
            if let Some(k) = choice_field(ui, "It has to", &items, usize::from(cond.is.is_some())) {
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
                field(ui, "Equal to", |ui| match &target_options {
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
                });
            }
        }
        let (label, hint) = if is_toggle {
            ("Show when it is off", "Otherwise: when it is on.")
        } else {
            ("Show when this is not so", "Turns the test round.")
        };
        toggle_row(ui, &mut cond.not, label, hint, accent);
        if cond != before {
            spec.widgets[i].set_show_if(Some(cond));
            changed = true;
        }
        changed
    })
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

    #[test]
    fn delete_removes_the_selected_widget_button_or_the_button_of_a_grid() {
        let mut d = PanelDesign::load(
            0,
            1,
            r#"{"widgets":[{"type":"toggle","id":"a"},{"type":"toggle","id":"b"}],"buttons":[{"id":"x"}]}"#,
        );
        assert!(!delete_selected(&mut d), "the panel itself stays");
        d.select(Sel::Widget(0));
        assert!(delete_selected(&mut d));
        assert_eq!(d.spec.widgets.len(), 1);
        d.select(Sel::Button(0));
        assert!(delete_selected(&mut d));
        assert!(d.spec.buttons.is_empty());
        assert!(
            d.spec.no_buttons,
            "the last button gone means none, not Cancel and OK again"
        );

        assert!(add_widget(&mut d, Kind::Buttons));
        let grid = d.spec.widgets.len() - 1;
        let before = match &d.spec.widgets[grid] {
            Widget::Buttons { buttons, .. } => buttons.len(),
            _ => unreachable!(),
        };
        assert!(before >= 2, "a new grid has some buttons");
        // picked on its own: only that button goes
        d.inner = Some((grid, 0));
        assert!(delete_selected(&mut d));
        match &d.spec.widgets[grid] {
            Widget::Buttons { buttons, .. } => assert_eq!(buttons.len(), before - 1),
            _ => unreachable!(),
        }
        assert_eq!(d.inner, None);
        // not picked on its own: the whole grid goes
        assert!(delete_selected(&mut d));
        assert_eq!(d.spec.widgets.len(), grid);
    }

    const FOUR: &str = r#"{"widgets":[{"type":"toggle","id":"a"},{"type":"toggle","id":"b"},{"type":"toggle","id":"c"},{"type":"toggle","id":"d"}]}"#;

    #[test]
    fn ctrl_and_shift_pick_several_widgets_and_a_plain_click_picks_one() {
        let mut d = PanelDesign::load(0, 1, FOUR);
        d.click_widget(0, false, false, false);
        assert_eq!(d.sel, Sel::Widget(0));
        d.click_widget(2, false, true, false);
        assert_eq!(d.selected_widgets(), vec![0, 2]);
        assert_eq!(d.sel, Sel::Panel, "a group is not a single pick");
        assert!(d.widget_on(0) && d.widget_on(2) && !d.widget_on(1));
        // Shift counts from the last one picked
        d.click_widget(3, true, false, false);
        assert_eq!(d.selected_widgets(), vec![2, 3]);
        // Ctrl on one of the group takes it out; one left is a plain pick again
        d.click_widget(3, false, true, false);
        assert_eq!((d.sel, d.group.len()), (Sel::Widget(2), 0));
        d.click_widget(1, false, false, false);
        assert_eq!((d.sel, d.group.len()), (Sel::Widget(1), 0));
    }

    #[test]
    fn a_group_is_removed_moved_and_duplicated_together() {
        let mut d = PanelDesign::load(0, 1, FOUR);
        d.set_widgets(BTreeSet::from([1, 2]));
        assert!(run_group(&mut d, Act::Up));
        assert_eq!(d.group, BTreeSet::from([0, 1]));
        assert_eq!(d.spec.widgets[2].id(), Some("a"));
        assert!(!run_group(&mut d, Act::Up), "already first");
        assert!(run_group(&mut d, Act::Copy));
        assert_eq!(d.spec.widgets.len(), 6);
        assert_eq!(d.group, BTreeSet::from([2, 3]), "the copies are selected");
        assert!(run_group(&mut d, Act::Remove));
        assert_eq!(d.spec.widgets.len(), 4);
        assert!(d.group.is_empty());
        d.set_widgets((0..d.spec.widgets.len()).collect());
        assert!(delete_selected(&mut d));
        assert!(d.spec.widgets.is_empty());
        assert_eq!(d.sel, Sel::Panel);
    }

    #[test]
    fn copy_cut_and_paste_work_on_the_selected_widgets() {
        let ctx = egui::Context::default();
        let mut d = PanelDesign::load(0, 1, FOUR);
        d.set_widgets(BTreeSet::from([0, 1]));
        assert!(
            !run_clip(&ctx, &mut d, Clip::Copy),
            "a copy changes nothing"
        );
        let text = design::copy_widgets(&d.spec, &d.selected_widgets()).unwrap();
        assert!(run_clip(&ctx, &mut d, Clip::Paste(text.clone())));
        assert_eq!(d.spec.widgets.len(), 6);
        assert_eq!(
            d.group,
            BTreeSet::from([2, 3]),
            "pasted after the last one picked"
        );
        assert!(!run_clip(&ctx, &mut d, Clip::Paste("hello".into())));
        assert_eq!(d.spec.widgets.len(), 6);
        assert!(run_clip(&ctx, &mut d, Clip::Cut));
        assert_eq!(d.spec.widgets.len(), 4);
        // nothing selected: a paste goes to the end
        d.select(Sel::Panel);
        assert!(run_clip(&ctx, &mut d, Clip::Paste(text)));
        assert_eq!(d.group, BTreeSet::from([4, 5]));
    }

    #[test]
    fn a_group_that_lost_widgets_shrinks_when_the_block_is_read_again() {
        let mut d = PanelDesign::load(0, 1, FOUR);
        d.set_widgets(BTreeSet::from([1, 3]));
        d.reload(r#"{"widgets":[{"type":"toggle","id":"a"},{"type":"toggle","id":"b"}]}"#);
        assert_eq!((d.sel, d.group.len()), (Sel::Widget(1), 0));
    }

    #[test]
    fn a_look_is_summed_up_in_a_few_words() {
        let range = FmtRange {
            usual: 22.0,
            least: 12.0,
            most: 28.0,
            bold: true,
            extra: FmtExtra::None,
        };
        let mut fmt = TextFmt::default();
        assert_eq!(fmt_summary(&fmt, &range), "Bold, 22 pt");
        fmt.bold = Some(false);
        fmt.italic = true;
        fmt.size = Some(18);
        assert_eq!(fmt_summary(&fmt, &range), "Regular, italic, 18 pt");
    }

    #[test]
    fn every_page_of_the_panel_inspector_draws_and_closes_when_something_else_is_picked() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let mut d = PanelDesign::load(0, 1, FOUR);
        for page in 0..4u8 {
            d.sel = Sel::Panel;
            d.look = Some((Sel::Panel, page));
            frame(&ctx, &mut d);
            frame(&ctx, &mut d);
            assert_eq!(
                d.look,
                Some((Sel::Panel, page)),
                "drawing a page does not leave it"
            );
        }
        d.sel = Sel::Widget(0);
        frame(&ctx, &mut d);
        assert_eq!(d.look, None, "picking a widget closes the page");
        // the first page of the panel draws too
        d.sel = Sel::Panel;
        frame(&ctx, &mut d);
        assert_eq!(d.look, None);
    }

    #[test]
    fn a_group_draws_in_the_list_the_panel_and_the_inspector() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let mut d = PanelDesign::load(0, 1, FOUR);
        d.set_widgets((0..4).collect());
        frame(&ctx, &mut d);
        frame(&ctx, &mut d);
        assert!(d.preview.is_some());
        assert_eq!(d.group.len(), 4, "drawing does not change the selection");
    }

    #[test]
    fn a_pick_in_the_list_lets_go_of_the_button_of_a_grid() {
        let mut d = PanelDesign::load(0, 1, r#"{"widgets":[{"type":"toggle","id":"a"}]}"#);
        d.inner = Some((0, 0));
        d.select(Sel::Widget(0));
        assert_eq!(d.inner, None);
    }
}
