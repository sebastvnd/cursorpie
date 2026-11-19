//! The pie editor and the import tray: the Menu tab on a canvas, like the canvas of the Flow tab.
//!
//! ```text
//!  pie editor                                           import tray (split screen)
//! +-----------------------------------+--------+      +--------------------+--+--------------------+
//! |  Default                          |        |      | IMPORTED  photo    |  | YOUR PIE  Default  |
//! |     ( pie )--------( Editors )    | slice  |      |   ( pie )---( sub )|  |  ( pie )---( sub ) |
//! |        \                          | editor |      |      drag a slice -+--+-> drop it on a slice |
//! |         \--------( Social )       |        |      |                    |  |                    |
//! |        [ + ] [ import ] [ fit ] [ inspector ] [ bin ] [ full screen ]  |  |                    |
//! +-----------------------------------+--------+      +--------------------+--+--------------------+
//! ```
//!
//! **The editor** (Menu tab, the second switch position) draws the whole tree of rings: the top ring
//! is a pie, and every submenu is a pie of its own to its right, wired to the slice that opens it.
//! It works the way the Flow canvas does:
//!
//! * the canvas moves with the middle button (the trigger button), Space + drag, or the scroll of a
//!   wheel or a trackpad, and zooms with Alt + scroll, Ctrl/Cmd + scroll or a pinch
//!   ([`pane_input`]). The left button never moves it: it selects and moves what is on it;
//! * click a slice to select it: the slice editor opens at the right (the inspector of the Flow
//!   tab). Click the empty canvas to let go of it and close the editor. A click on the middle of a
//!   pie selects that pie and makes it the ring a new slice is added to; Shift or Ctrl/Cmd + click
//!   adds a pie to the selection or takes it out;
//! * a drag on the empty canvas draws a **selection box** (Shift or Ctrl/Cmd adds to what is selected,
//!   as in the Flow tab) and Ctrl/Cmd + A selects every pie;
//! * drag the **middle of a pie** to move it, and the other selected pies with it. Where a pie was
//!   put is saved in [`Item::at`] of the slice that opens it, so it is still there next time; the
//!   top pie stays put (the others are placed from it) and the broom of the bar puts every pie back
//!   where the editor places it;
//! * Ctrl/Cmd + C copies the selected slice, or the selected pies with everything in them, Ctrl/Cmd + X
//!   cuts them and Ctrl/Cmd + V pastes them at the end of the pie that is lit, the way the Flow canvas
//!   copies blocks: through the system clipboard as text ([`pie_edit::copy_slices`]), so a copy can
//!   go to another pie or profile;
//! * the **+** of the floating bar unfolds the three groups of slices in the bar and opens a
//!   list over it, and the right button opens the same list where it is pressed: not a sheet, a
//!   floating list like the one of the Flow canvas ([`slice_menu`]);
//! * drag a slice onto another slice to move it before that one, onto the middle of another pie to
//!   put it last in that ring (also into a submenu's pie), onto the bin of the bar to delete it;
//!   Delete or Backspace deletes the selected slice;
//! * the **full screen** button of the bar (or F11) fills the display, as in the Flow tab.
//!
//! **The import tray** opens when a profile file is imported. Nothing is added to your pies: the file
//! is shown on the left half, your pie on the right half, and what you need is dragged across (a
//! double click, or the arrow of the bar, copies the selected slice at once). Only the images and
//! flows that a copied slice uses are brought along, and a slice that runs a shell command is marked
//! before it is copied. The divider between the halves moves, and the tray can use the whole
//! display too.
//!
//! Dragging one slice at a time is not the only way in. **Select mode** (or Shift / Ctrl/Cmd + click)
//! picks several slices of the imported pie and *Add selected* brings them together; the middle of a
//! pie picks its whole ring and Ctrl/Cmd + A the top ring. The bar can also bring the file in with one
//! press: *Add every slice* (end of the lit ring), *As a submenu* (one slice that holds the file) or
//! *Use as this pie* (the file's slices replace the ones of the pie you edit; Undo brings them back),
//! see [`bring_items`] and [`replace_pie_with_file`]. *Add all as a pie* keeps the old behavior of
//! importing: the whole file becomes a new pie. A profile file dropped on the window, or copied as
//! text and pasted with Ctrl/Cmd + V, opens in the tray too ([`pasted_profile`]).
//!
//! The rules for what may be moved or dropped where (8 slices a ring, 5 rings deep, a submenu never
//! inside itself) are in [`pie_edit`] and unit tested there.

use super::flow::{AMBER, notice};
use super::flow_list::{
    BAR_GAP, BAR_PAD, MENU_H, MENU_W, Tone, bar_button, bar_divider, full_button,
};
use crate::config::flow::{flow_ids_used, merge_flows};
use crate::config::pie_edit::{self, Landed, PasteSlices, Place};
use crate::config::share::{self, Bundle};
use crate::config::*;
use crate::geometry::Point;
use crate::geometry::{pie_inner, pie_outer, slice_at, slice_start};
use crate::msg::Msg;
use crate::platform;
use crate::ui::app::App;
use crate::ui::draw::*;
use crate::ui::fonts;
use crate::ui::settings::clip::{self, Clip};
use crate::ui::settings::inspector::{Env, item_inspector};
use crate::ui::settings::picker::spawn_file_picker;
use crate::ui::settings::state::{
    AddCatalog, Ed, IMPORT_SPLIT, MenuView, PieDrag, PieImport, PieMenu, PieView,
};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::{choice_sheet_open, icon_switch, select_item, text_field};
use crate::ui::symbols;
use eframe::egui::{
    self, Align2, Color32, FontId, PointerButton, Pos2, Rect, Response, RichText, Sense, Shape,
    Stroke, StrokeKind, TextureHandle, Ui, UiBuilder, Vec2, pos2, vec2,
};
use std::collections::{BTreeSet, HashMap};
use std::f32::consts::TAU;
use std::sync::mpsc::Sender;

/// Diameter of a pie on the canvas, in the units of the canvas (the pie of the classic tab is
/// 200 to 300 points).
const PIE_D: f32 = 250.0;
/// Distance between the middles of two columns of pies (the depth of a ring) and of two rows.
const COL_W: f32 = 380.0;
const ROW_H: f32 = 330.0;
/// How far the canvas can be zoomed out and in.
const ZOOM_MIN: f32 = 0.25;
const ZOOM_MAX: f32 = 3.0;
/// Height of the strip under a canvas that holds the hint line, so no pie covers it.
const STATUS_H: f32 = 30.0;
/// Width of the slice editor at the right of the pie editor.
const INSP_W: f32 = 400.0;
/// Space between the edge of the slice editor and what is in it, as in the Flow tab.
const INSP_PAD: f32 = 14.0;
/// Width of the divider between the two halves of the import tray.
const DIVIDER_W: f32 = 16.0;
/// The round buttons of the bar, the **+** included: the same size as in the Flow tab.
const BAR_CELL: f32 = CONTROL_H - 6.0;
/// How long the groups of slices take to unfold in the bar, in seconds.
const UNFOLD_SECS: f32 = 0.24;
/// How long the circle under the chosen group takes to travel to the next one, in seconds (the same
/// as in the Flow tab).
const THUMB_SECS: f64 = 0.28;
/// The groups of the list of slices (the order of `AddCatalog::groups`): the icon in the bar and
/// what it says.
const GROUPS: [(char, &str); 3] = [
    (symbols::LIGHTNING, "Quick actions"),
    (symbols::MOUSE_SIMPLE, "Side buttons"),
    (symbols::SQUARES_FOUR, "Custom"),
];

// ------------------------------------------------------------------------------ the switch

/// The switch between the classic Menu tab and the pie editor, two icons on one round track like the
/// switch of the Flow tab. Returns the view that was picked, if it is not the current one.
pub(in crate::ui::settings) fn view_switch(
    ui: &mut Ui,
    current: MenuView,
    accent: Color32,
) -> Option<MenuView> {
    let views = [
        (
            MenuView::Classic,
            symbols::CHART_PIE_SLICE,
            "Classic: one pie at a time, with its slice editor",
        ),
        (
            MenuView::Editor,
            symbols::SELECTION_ALL,
            "Pie editor: every pie and submenu on a canvas",
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

// ------------------------------------------------------------------------------ the layout

/// One ring of the tree and where its pie goes.
struct RingBox {
    /// The ring: the path of the submenu that opens it (empty for the top ring).
    path: Vec<usize>,
    depth: usize,
    /// The ring (an index into the list of boxes) and the slice that this ring is the submenu of.
    parent: Option<(usize, usize)>,
    /// The row, in the units of the canvas.
    y: f32,
    /// How far the person moved this pie from its place ([`Item::at`]); zero for the top pie and for
    /// a pie that was never moved. The place of a pie that holds submenus is the middle of theirs
    /// *before* they were moved, so moving one pie never drags the others along.
    off: Vec2,
}

/// Where the person put the pie of the submenu `it`, as a distance from its place.
fn item_offset(it: &Item) -> Vec2 {
    it.at.map_or(Vec2::ZERO, |[x, y]| vec2(x, y))
}

/// Places one pie per ring: a column for each depth, the pies of a column one under the other, and a
/// pie next to the middle of the pies of its own submenus.
fn layout(items: &[Item]) -> Vec<RingBox> {
    fn place(
        items: &[Item],
        path: &mut Vec<usize>,
        parent: Option<(usize, usize)>,
        off: Vec2,
        next_row: &mut f32,
        out: &mut Vec<RingBox>,
    ) -> f32 {
        let me = out.len();
        out.push(RingBox {
            path: path.clone(),
            depth: path.len(),
            parent,
            y: 0.0,
            off,
        });
        let mut ys = Vec::new();
        for (i, it) in items.iter().enumerate() {
            if let ItemKind::Submenu(kids) = &it.kind {
                path.push(i);
                ys.push(place(
                    kids,
                    path,
                    Some((me, i)),
                    item_offset(it),
                    next_row,
                    out,
                ));
                path.pop();
            }
        }
        let y = match (ys.first(), ys.last()) {
            (Some(a), Some(b)) => (a + b) / 2.0,
            _ => {
                let y = *next_row;
                *next_row += ROW_H;
                y
            }
        };
        out[me].y = y;
        y
    }
    let mut out = Vec::new();
    let mut next_row = 0.0;
    place(
        items,
        &mut Vec::new(),
        None,
        Vec2::ZERO,
        &mut next_row,
        &mut out,
    );
    out
}

/// The middle of the pie of `b`, in the units of the canvas.
fn scene_center(b: &RingBox) -> Vec2 {
    vec2(
        PIE_D / 2.0 + 40.0 + b.depth as f32 * COL_W,
        PIE_D / 2.0 + 60.0 + b.y,
    ) + b.off
}

/// Pans and zooms `view` so every pie of `boxes` is in view, in an area of `size`.
fn fit_view(view: &mut PieView, boxes: &[RingBox], size: Vec2) {
    let half = PIE_D / 2.0 + 36.0;
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for b in boxes {
        let c = scene_center(b);
        lo = lo.min(c - Vec2::splat(half));
        hi = hi.max(c + Vec2::splat(half));
    }
    if lo.x > hi.x {
        view.zoom = 1.0;
        view.pan = Vec2::ZERO;
        return;
    }
    let (w, h) = ((hi.x - lo.x).max(1.0), (hi.y - lo.y).max(1.0));
    let zoom = ((size.x - 40.0) / w)
        .min((size.y - 90.0) / h)
        .clamp(ZOOM_MIN, 1.0);
    view.zoom = if zoom.is_finite() { zoom } else { 1.0 };
    let mid = (lo + hi) / 2.0;
    view.pan = vec2(size.x / 2.0, (size.y - 40.0) / 2.0) - mid * view.zoom;
}

// ------------------------------------------------------------------------------ one canvas

/// Which side a canvas is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Your pie: slices are moved inside it and dropped into it.
    Mine,
    /// The imported pie: slices are only taken from it.
    Source,
}

/// Where the images of a canvas come from.
enum Icons<'a> {
    /// The usual cache.
    Mine,
    /// The file that is being imported (its images are not on disk yet).
    Staged {
        bundle: &'a Bundle,
        textures: &'a mut HashMap<String, Option<TextureHandle>>,
    },
}

/// What a canvas needs for one frame.
struct Pane<'a> {
    id: &'static str,
    /// The whole half, the hint strip included.
    rect: Rect,
    items: &'a [Item],
    title: String,
    badge: &'static str,
    role: Role,
    view: &'a mut PieView,
    icons: Icons<'a>,
    /// The ring a new slice goes to (yours only).
    active: Vec<usize>,
    sel: Option<(Vec<usize>, usize)>,
    /// The slices picked together in the imported pie (paths); empty on your own pie.
    picked: BTreeSet<Vec<usize>>,
    /// The imported pie is in select mode: a click picks a slice and nothing is dragged.
    select_mode: bool,
    /// What the view is fitted to: when it changes the view is fitted again.
    fit_key: usize,
    show_labels: bool,
    accent: Color32,
    /// The pinch that [`platform::take_pinch`] gave this frame.
    pinch: f32,
    hint: &'static str,
}

/// Where a pie was drawn this frame, in the coordinates of the window.
struct Geom {
    path: Vec<usize>,
    c: Pos2,
    outer: f32,
    inner: f32,
    len: usize,
    parent: Option<(usize, usize)>,
}

/// What the pointer is over.
enum Hit {
    None,
    /// The middle of the pie (its index in the list of [`Geom`]), or the whole pie when it is empty.
    Center(usize),
    /// A slice: the pie and the place in it.
    Slice(usize, usize),
}

fn hit_at(geoms: &[Geom], p: Pos2) -> Hit {
    // the pie that is drawn last is on top, so it is asked first (two pies can overlap now)
    for (g, geom) in geoms.iter().enumerate().rev() {
        let d = (p - geom.c).length();
        if d > geom.outer + 6.0 {
            continue;
        }
        if d < geom.inner || geom.len == 0 {
            return Hit::Center(g);
        }
        return match slice_at(
            Point::new(f64::from(geom.c.x), f64::from(geom.c.y)),
            Point::new(f64::from(p.x), f64::from(p.y)),
            geom.len,
            geom.inner,
        ) {
            Some(i) => Hit::Slice(g, i),
            None => Hit::Center(g),
        };
    }
    Hit::None
}

/// What a click on a canvas meant.
enum Click {
    Slice(Vec<usize>, usize),
    Ring(Vec<usize>),
    Empty,
}

/// What a canvas reports back for one frame.
#[derive(Default)]
struct PaneOut {
    geoms: Vec<Geom>,
    click: Option<Click>,
    /// A slice was double clicked.
    double: Option<(Vec<usize>, usize)>,
    /// The right button: the ring to add a slice to, and where it was pressed.
    secondary: Option<(Vec<usize>, Pos2)>,
    /// The middle of a pie was pressed to drag it: its ring, and whether Shift or Ctrl/Cmd was held.
    pie_press: Option<(Vec<usize>, bool)>,
    /// How far the selected pies were dragged this frame, in the units of the canvas.
    pie_delta: Vec2,
    /// The drag of the pies ended (or was cut short) this frame.
    pie_dropped: bool,
    /// A selection box was let go: the rings of the pies it touched.
    marquee: Option<Vec<Vec<usize>>>,
    /// A slice that was dragged here was let go at this place.
    dropped: Option<Pos2>,
    /// The pointer is over the canvas.
    over: bool,
}

/// The size of a text at zoom `z`, in half points (the text is cut into the font atlas at each
/// size, so a steady zoom does not fill it).
fn text_size(base: f32, z: f32) -> f32 {
    ((base * z * 2.0).round() / 2.0).max(6.0)
}

fn rgb3(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// The texture of an icon, from the cache or (for the imported pie) from the bytes of the file.
fn texture(
    ctx: &egui::Context,
    ed: &mut Ed,
    icons: &mut Icons,
    icon: &str,
) -> Option<TextureHandle> {
    match icons {
        Icons::Mine => ed.icon_for(ctx, icon),
        Icons::Staged { bundle, textures } => {
            if let Some(entry) = textures.get(icon) {
                return match entry {
                    Some(t) => Some(t.clone()),
                    None => ed.icon_for(ctx, icon),
                };
            }
            let tex = bundle
                .icon_bytes(icon)
                .and_then(|bytes| load_icon_bytes(ctx, &bytes, "imported-icon"));
            textures.insert(icon.to_string(), tex.clone());
            match tex {
                Some(t) => Some(t),
                None => ed.icon_for(ctx, icon),
            }
        }
    }
}

/// What a slice does, in a word, for its tooltip.
fn kind_label(it: &Item) -> &'static str {
    match it.kind {
        ItemKind::Mode { .. } => "Side buttons",
        ItemKind::Run(_) => "Action",
        ItemKind::Submenu(_) => "Submenu",
    }
}

/// Whether a drag with the left button moves the canvas (Space is held and no field is being typed
/// in). Without Space the left button belongs to what is on the canvas: it selects and moves.
fn pans_with_drag(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_down(egui::Key::Space)) && !ctx.egui_wants_keyboard_input()
}

/// Moves and zooms the canvas the way the canvas of the Flow tab does: hold the middle button (the
/// trigger button) or Space + drag to move; a wheel or two fingers move it too; Alt (Option) +
/// scroll, Ctrl/Cmd + scroll or a pinch zoom around the pointer. A plain drag with the left button
/// does **not** move the canvas: it draws a selection box or carries a slice or a pie. Returns
/// whether the pointer is over the canvas. `wheel_taken` is set while a list is open over the canvas:
/// the wheel belongs to it.
fn pane_input(
    ctx: &egui::Context,
    view: &mut PieView,
    canvas: Rect,
    resp: &Response,
    pinch_extra: f32,
    blocked: bool,
    wheel_taken: bool,
) -> bool {
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
    let over = pos.is_some_and(|p| canvas.contains(p));
    let mid_key = resp.id.with("mid");
    let mut mid = ctx.data(|d| d.get_temp::<bool>(mid_key)).unwrap_or(false);
    if blocked {
        ctx.data_mut(|d| d.insert_temp(mid_key, false));
        return false;
    }
    // Space + drag moves it (a drag of the empty canvas draws a selection box instead)
    if resp.dragged_by(PointerButton::Primary) && pans_with_drag(ctx) {
        view.pan += resp.drag_delta();
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    if pressed && over {
        mid = true;
    }
    if !down {
        mid = false;
    }
    if mid {
        view.pan += motion;
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        ctx.request_repaint();
    }
    ctx.data_mut(|d| d.insert_temp(mid_key, mid));

    // a list that is open takes the wheel
    if !over || wheel_taken || egui::Popup::is_any_open(ctx) {
        return over;
    }
    let pinch = pinch * pinch_extra;
    let mut factor = 1.0;
    if pinch != 1.0 {
        factor = pinch;
    } else if alt && scroll != Vec2::ZERO {
        let d = if scroll.y != 0.0 { scroll.y } else { scroll.x };
        factor = (d * 0.0025).exp();
    } else if scroll != Vec2::ZERO {
        view.pan += scroll;
    }
    if factor != 1.0 {
        let old = view.zoom;
        let new = (old * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        // keep the point under the pointer where it is
        let at = pos.unwrap_or_else(|| canvas.center()) - canvas.min;
        let scene_at = (at - view.pan) / old;
        view.pan = at - scene_at * new;
        view.zoom = new;
    }
    over
}

/// A cubic curve from `a` to `b` that leaves `a` to the right and arrives at `b` from the left.
fn wire_points(a: Pos2, b: Pos2, reach: f32) -> Vec<Pos2> {
    let (c1, c2) = (a + vec2(reach, 0.0), b - vec2(reach, 0.0));
    (0..=28)
        .map(|k| {
            let t = k as f32 / 28.0;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            pos2(
                w[0] * a.x + w[1] * c1.x + w[2] * c2.x + w[3] * b.x,
                w[0] * a.y + w[1] * c1.y + w[2] * c2.y + w[3] * b.y,
            )
        })
        .collect()
}

/// Whether the circle of radius `r` around `c` touches `rect`.
fn circle_touches_rect(c: Pos2, r: f32, rect: Rect) -> bool {
    let nearest = pos2(
        c.x.clamp(rect.min.x, rect.max.x),
        c.y.clamp(rect.min.y, rect.max.y),
    );
    (nearest - c).length() <= r
}

/// The rings whose pie the selection box `r` touches.
fn pies_touched(geoms: &[Geom], r: Rect) -> Vec<Vec<usize>> {
    geoms
        .iter()
        .filter(|g| circle_touches_rect(g.c, g.outer, r))
        .map(|g| g.path.clone())
        .collect()
}

/// The pies a selection box leaves selected: the ones it touches, and, when `base` (what was selected
/// before the box, empty unless Shift or Ctrl/Cmd was held) has some, the touched ones that were in
/// it are taken out and the others added, as the box of the Flow canvas does.
fn pick_pies(base: &BTreeSet<Vec<usize>>, touched: &[Vec<usize>]) -> BTreeSet<Vec<usize>> {
    let mut chosen = base.clone();
    for path in touched {
        if !chosen.remove(path) {
            chosen.insert(path.clone());
        }
    }
    chosen
}

/// Draws one canvas of pies and handles what the pointer does there.
fn pane(ui: &mut Ui, ctx: &egui::Context, ed: &mut Ed, p: Pane) -> PaneOut {
    let Pane {
        id,
        rect,
        items,
        title,
        badge,
        role,
        view,
        mut icons,
        active,
        sel,
        picked: multi_picked,
        select_mode,
        fit_key,
        show_labels,
        accent,
        pinch,
        hint,
    } = p;
    let canvas = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - STATUS_H));
    let boxes = layout(items);
    if view.fitted_for != Some(fit_key) {
        fit_view(view, &boxes, canvas.size());
        view.fitted_for = Some(fit_key);
    }
    let is_source = role == Role::Source;
    let (outer0, inner0) = (pie_outer(PIE_D), pie_inner(PIE_D));
    let geoms_for = |view: &PieView| -> Vec<Geom> {
        boxes
            .iter()
            .map(|b| Geom {
                path: b.path.clone(),
                c: canvas.min.to_vec2().to_pos2() + view.pan + scene_center(b) * view.zoom,
                outer: outer0 * view.zoom,
                inner: inner0 * view.zoom,
                len: items_at(items, &b.path).map_or(0, <[Item]>::len),
                parent: b.parent,
            })
            .collect()
    };

    let resp = ui.interact(
        canvas,
        egui::Id::new((id, ed.session)),
        Sense::click_and_drag(),
    );
    let mut out = PaneOut::default();
    let blocked = ed.sheet_open() || choice_sheet_open(ctx);

    // The left button never moves the canvas (the middle button, Space + drag and the scroll do).
    // A drag that starts on a slice carries it, one that starts on the middle of a pie moves the
    // selected pies, and one that starts on the empty canvas draws a selection box.
    let mine = role == Role::Mine;
    if resp.drag_started_by(PointerButton::Primary)
        && !blocked
        && ed.pie_drag.is_none()
        && !pans_with_drag(ctx)
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        let geoms = geoms_for(view);
        let additive = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        match hit_at(&geoms, origin) {
            // in select mode a press on the imported pie only picks: nothing is carried
            Hit::Slice(g, i) if !(is_source && select_mode) => {
                let ring = geoms[g].path.clone();
                ed.pie_drag = Some(PieDrag {
                    from_import: is_source,
                    ring: ring.clone(),
                    idx: i,
                });
                out.click = Some(Click::Slice(ring, i));
            }
            // the top pie stays where it is: the others are placed from it
            Hit::Center(g) if mine && !geoms[g].path.is_empty() => {
                out.pie_press = Some((geoms[g].path.clone(), additive));
            }
            Hit::None if mine => {
                ed.pie_marquee = Some(origin);
                ed.pie_marquee_base = if additive {
                    ed.pie_picked.clone()
                } else {
                    BTreeSet::new()
                };
            }
            _ => {}
        }
    }
    let drag_here = ed
        .pie_drag
        .clone()
        .filter(|d| d.from_import == is_source && items_at(items, &d.ring).is_some());
    out.over = pane_input(
        ctx,
        view,
        canvas,
        &resp,
        pinch,
        blocked,
        ed.pie_menu.is_some(),
    );
    // the selected pies follow the pointer while the middle of one is held
    if mine && ed.pie_moving {
        if resp.dragged_by(PointerButton::Primary) {
            out.pie_delta = resp.drag_delta() / view.zoom;
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        } else {
            out.pie_dropped = true;
        }
    }
    let z = view.zoom;
    let geoms = geoms_for(view);
    let pointer = ui.input(|i| i.pointer.latest_pos());
    let hover = if blocked {
        Hit::None
    } else {
        resp.hover_pos().map_or(Hit::None, |p| hit_at(&geoms, p))
    };
    let dragging = drag_here.filter(|_| resp.dragged());
    // while a slice is carried, the slice of your pie it would land on is lit
    let carrying = ed.pie_drag.is_some() && role == Role::Mine;
    let landing = if carrying && !blocked {
        pointer
            .filter(|p| canvas.contains(*p))
            .map(|p| hit_at(&geoms, p))
    } else {
        None
    };

    // the selection box, the pies it touches, and so the pies that are shown as selected
    let marquee_box = if mine {
        ed.pie_marquee
            .zip(pointer)
            .map(|(a, b)| Rect::from_two_pos(a, b))
    } else {
        None
    };
    let touched = marquee_box.map_or_else(Vec::new, |r| pies_touched(&geoms, r));
    let shown_picked: BTreeSet<Vec<usize>> = if marquee_box.is_some() {
        pick_pies(&ed.pie_marquee_base, &touched)
    } else if mine {
        ed.pie_picked.clone()
    } else {
        BTreeSet::new()
    };
    if mine && ed.pie_marquee.is_some() {
        if resp.drag_stopped() {
            out.marquee = Some(touched);
            ed.pie_marquee = None;
        } else if !resp.dragged() {
            ed.pie_marquee = None;
        }
    }

    // ---- the page of the canvas
    let screen = ui.painter_at(rect);
    screen.rect_filled(rect, panel_r(), pal().input);
    if z >= 0.45 {
        // a grid of dots that moves with the canvas
        let step = 28.0 * z;
        let dot = pal().border.gamma_multiply(0.55);
        let (ox, oy) = (
            canvas.min.x + view.pan.x.rem_euclid(step),
            canvas.min.y + view.pan.y.rem_euclid(step),
        );
        let clip = canvas.shrink(2.0);
        let mut y = oy;
        while y < clip.max.y {
            let mut x = ox;
            while x < clip.max.x {
                if clip.contains(pos2(x, y)) {
                    screen.circle_filled(pos2(x, y), 1.0, dot);
                }
                x += step;
            }
            y += step;
        }
    }
    let painter = ui.painter_at(canvas.shrink(1.0));

    // ---- the wires, under the pies: from the slice of a submenu to the pie of its ring
    for geom in &geoms {
        let Some((pg, slice)) = geom.parent else {
            continue;
        };
        let parent = &geoms[pg];
        let sl = TAU / parent.len.max(1) as f32;
        let am = slice_start(slice, parent.len.max(1)) + sl / 2.0;
        let a = parent.c + vec2(am.cos(), am.sin()) * parent.outer;
        let b = geom.c - vec2(geom.outer + 8.0 * z, 0.0);
        let hot = sel
            .as_ref()
            .is_some_and(|(r, i)| *r == parent.path && *i == slice);
        let color = if hot {
            accent
        } else {
            pal().muted.gamma_multiply(0.55)
        };
        painter.add(Shape::line(
            wire_points(a, b, 70.0 * z),
            Stroke::new((if hot { 2.6 } else { 1.8 }) * z.max(0.6), color),
        ));
        painter.circle_filled(a, 4.0 * z.max(0.6), color);
        painter.circle_filled(b, 4.0 * z.max(0.6), color);
    }

    // ---- the pies
    let accent_rgb = rgb3(accent);
    for geom in &geoms {
        // the ring is off the visible part of the canvas: nothing to draw
        if !canvas.expand(geom.outer + 40.0).contains(geom.c) {
            continue;
        }
        let ring = items_at(items, &geom.path).unwrap_or(&[]);
        let total = ring.len();
        let is_active = role == Role::Mine && active == geom.path;
        let picked = shown_picked.contains(&geom.path);
        // a soft shadow under the ring: one blurred shape, so it leaves no step at its edge
        if z >= 0.45 {
            painter.add(
                egui::Shadow {
                    offset: [0, 6],
                    blur: 22,
                    spread: 0,
                    color: Color32::from_black_alpha(26),
                }
                .as_shape(
                    Rect::from_center_size(geom.c, Vec2::splat(geom.outer * 2.0)),
                    PILL,
                ),
            );
        }
        // a selected pie sits on a tint of the accent with a thin ring (a tint, not a heavy border)
        if picked {
            painter.circle_filled(geom.c, geom.outer + 12.0 * z, wash(accent, 0.10));
            painter.circle_stroke(
                geom.c,
                geom.outer + 12.0 * z,
                Stroke::new(1.5 * z.max(0.6), accent.gamma_multiply(0.75)),
            );
        }
        let name = if geom.path.is_empty() {
            title.clone()
        } else {
            item_at(items, &geom.path)
                .map(|i| i.name.clone())
                .unwrap_or_default()
        };
        // the name of the ring over its pie
        painter.text(
            geom.c - vec2(0.0, geom.outer + 10.0 * z),
            Align2::CENTER_BOTTOM,
            name,
            FontId::proportional(text_size(14.0, z)),
            if is_active || picked {
                accent
            } else {
                pal().text
            },
        );
        if is_active && !picked {
            painter.circle_stroke(
                geom.c,
                geom.outer + 7.0 * z,
                Stroke::new(2.0 * z.max(0.6), accent.gamma_multiply(0.55)),
            );
        }
        if total == 0 {
            painter.circle_stroke(
                geom.c,
                geom.outer,
                Stroke::new(1.5 * z.max(0.6), pal().border),
            );
        }
        let sl = TAU / total.max(1) as f32;
        for (i, it) in ring.iter().enumerate() {
            let selected = sel
                .as_ref()
                .is_some_and(|(r, s)| *r == geom.path && *s == i)
                || (is_source && !multi_picked.is_empty() && {
                    let mut at = geom.path.clone();
                    at.push(i);
                    multi_picked.contains(&at)
                });
            let lifted = dragging
                .as_ref()
                .is_some_and(|d| d.ring == geom.path && d.idx == i);
            let g_index = geoms
                .iter()
                .position(|o| o.path == geom.path)
                .unwrap_or_default();
            let hovered =
                dragging.is_none() && matches!(hover, Hit::Slice(g, s) if g == g_index && s == i);
            let lands = matches!(landing, Some(Hit::Slice(g, s)) if g == g_index && s == i);
            let t = if lands {
                0.45
            } else if selected {
                1.0
            } else if hovered {
                0.16
            } else {
                0.0
            };
            let ink = if selected {
                on_accent(accent)
            } else {
                pie_colors().ink
            };
            let ink = if lifted {
                ink.gamma_multiply(0.35)
            } else {
                ink
            };
            let a0 = slice_start(i, total);
            painter.add(ring_slice(
                &painter,
                geom.c,
                geom.inner,
                geom.outer,
                a0 + 0.03,
                a0 + sl - 0.03,
                rgb(mix(rgb3(pal().idle), accent_rgb, t)),
                Bend::NONE,
            ));
            let am = a0 + sl / 2.0;
            let tp = geom.c + vec2(am.cos(), am.sin()) * ((geom.inner + geom.outer) / 2.0);
            let labels = show_labels && z >= 0.5;
            let (icon_dy, caret_dy) = if labels {
                (-8.0 * z, 33.0 * z)
            } else {
                (0.0, 24.0 * z)
            };
            let tex = texture(ctx, ed, &mut icons, &it.icon);
            draw_icon(
                &painter,
                tex.as_ref(),
                &it.icon,
                tp + vec2(0.0, icon_dy),
                30.0 * z,
                ink,
            );
            if labels {
                painter.text(
                    tp + vec2(0.0, 18.0 * z),
                    Align2::CENTER_CENTER,
                    &it.name,
                    FontId::proportional(text_size(11.5, z)),
                    ink.gamma_multiply(0.85),
                );
            }
            if matches!(it.kind, ItemKind::Submenu(_)) {
                fonts::paint_icon(
                    &painter,
                    tp + vec2(0.0, caret_dy),
                    symbols::CARET_RIGHT,
                    13.0 * z,
                    if selected { ink } else { accent },
                    false,
                );
            }
            // in the imported pie, a slice that can run a command is marked before it is copied
            if is_source && item_runs_commands(it) {
                fonts::paint_icon(
                    &painter,
                    tp + vec2(
                        0.30 * (geom.outer - geom.inner),
                        -0.45 * (geom.outer - geom.inner),
                    ),
                    symbols::TERMINAL_WINDOW,
                    14.0 * z,
                    AMBER,
                    false,
                );
            }
        }
        // the middle: the mark, a "+" while the ring is empty
        let core = geom.inner * 0.86;
        let center_hot = matches!(hover, Hit::Center(g) if geoms[g].path == geom.path)
            || matches!(&landing, Some(Hit::Center(g)) if geoms[*g].path == geom.path);
        painter.circle_filled(
            geom.c,
            core,
            if center_hot && role == Role::Mine && carrying {
                accent.gamma_multiply(0.35)
            } else if center_hot {
                pal().hot
            } else {
                pal().bg
            },
        );
        fonts::paint_icon(
            &painter,
            geom.c,
            if total == 0 {
                symbols::PLUS
            } else {
                symbols::CHART_PIE_SLICE
            },
            22.0 * z,
            pal().muted,
            false,
        );
        if total == 0 && z >= 0.5 {
            painter.text(
                geom.c + vec2(0.0, geom.outer * 0.62),
                Align2::CENTER_CENTER,
                if role == Role::Mine {
                    "Empty: drop a slice here"
                } else {
                    "Empty"
                },
                FontId::proportional(text_size(11.5, z)),
                pal().muted,
            );
        }
    }

    // ---- the selection box
    if let Some(r) = marquee_box {
        let r = r.intersect(canvas.shrink(1.0));
        painter.rect_filled(r, control_r(), wash(accent, 0.10));
        painter.rect_stroke(r, control_r(), Stroke::new(1.0, accent), StrokeKind::Inside);
    }

    // ---- the frame of the canvas, its badge and the hint strip
    screen.rect_stroke(
        rect,
        panel_r(),
        Stroke::new(1.0, pal().border),
        StrokeKind::Inside,
    );
    screen.line_segment(
        [
            pos2(rect.min.x + 1.0, canvas.max.y),
            pos2(rect.max.x - 1.0, canvas.max.y),
        ],
        Stroke::new(1.0, pal().border),
    );
    let chip_color = if is_source { accent } else { pal().muted };
    screen.text(
        pos2(rect.min.x + 16.0, rect.min.y + 18.0),
        Align2::LEFT_CENTER,
        badge,
        FontId::proportional(11.5),
        chip_color,
    );
    screen.text(
        pos2(rect.min.x + 16.0, canvas.max.y + STATUS_H / 2.0),
        Align2::LEFT_CENTER,
        hint,
        FontId::proportional(12.0),
        pal().muted,
    );

    // ---- the slice that is carried follows the pointer
    if let (Some(d), Some(p)) = (&dragging, pointer)
        && let Some(it) = items_at(items, &d.ring).and_then(|r| r.get(d.idx))
    {
        let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new(("pie_slice_drag", id)));
        let float = ctx.layer_painter(layer);
        float.circle_filled(p, 26.0, accent.gamma_multiply(0.92));
        let tex = texture(ctx, ed, &mut icons, &it.icon);
        draw_icon(&float, tex.as_ref(), &it.icon, p, 28.0, on_accent(accent));
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        ctx.request_repaint();
    } else if ed.pie_moving {
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if let Hit::Center(g) = &hover
        && mine
        && !geoms[*g].path.is_empty()
    {
        // the middle of a pie that can be moved
        ctx.set_cursor_icon(egui::CursorIcon::Grab);
    } else if !matches!(hover, Hit::None) {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    // ---- let go: the owner tells where
    if resp.drag_stopped()
        && let Some(d) = ed.pie_drag.clone()
        && d.from_import == is_source
    {
        out.dropped = pointer;
    }
    // a drag that ended without telling us (focus lost)
    if !resp.dragged()
        && !resp.drag_stopped()
        && ed
            .pie_drag
            .as_ref()
            .is_some_and(|d| d.from_import == is_source)
    {
        ed.pie_drag = None;
    }

    // ---- clicks
    if !blocked {
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            out.click = Some(match hit_at(&geoms, p) {
                Hit::Slice(g, i) => Click::Slice(geoms[g].path.clone(), i),
                Hit::Center(g) => Click::Ring(geoms[g].path.clone()),
                Hit::None => Click::Empty,
            });
        }
        if resp.double_clicked()
            && let Some(p) = resp.interact_pointer_pos()
            && let Hit::Slice(g, i) = hit_at(&geoms, p)
        {
            out.double = Some((geoms[g].path.clone(), i));
        }
        if resp.secondary_clicked()
            && let Some(at) = resp.interact_pointer_pos()
        {
            let ring = match hit_at(&geoms, at) {
                Hit::Slice(g, _) | Hit::Center(g) => geoms[g].path.clone(),
                Hit::None => active.clone(),
            };
            out.secondary = Some((ring, at));
        }
    }
    // the name and kind of the slice under the pointer
    if let Hit::Slice(g, i) = &hover
        && let Some(it) = items_at(items, &geoms[*g].path).and_then(|r| r.get(*i))
    {
        let mut tip = format!("{} \u{b7} {}", it.name, kind_label(it));
        if is_source && item_runs_commands(it) {
            tip.push_str("\nCan run a shell command, write files or send messages.");
        }
        let _ = resp.clone().on_hover_text(tip);
    } else if let Hit::Center(g) = &hover
        && mine
        && !resp.dragged()
        && ed.pie_drag.is_none()
    {
        let tip = if geoms[*g].path.is_empty() {
            "The main pie. The others are placed around it."
        } else {
            "Drag to move this pie \u{b7} click to add slices to it"
        };
        let _ = resp.clone().on_hover_text(tip);
    }
    out.geoms = geoms;
    out
}

// ------------------------------------------------------------------------------ the bar

/// What the floating bar asked for.
enum BarDo {
    Add,
    Import,
    Fit,
    Tidy,
    Inspector,
    Delete,
    Full,
    CopySelected,
    /// Select mode of the imported pie on or off.
    SelectMode,
    /// Pick every slice of the imported pie, or none when all are picked.
    SelectAll,
    /// Add every slice of the file to the end of the lit ring.
    AddRing,
    /// Add the whole file to the lit ring as one submenu.
    AddGroup,
    /// Make the file the slices of this pie.
    ReplacePie,
    AddAll,
    Done,
}

/// What the bar needs to know to draw itself.
#[derive(Clone, Copy)]
struct BarState {
    /// The import tray is open (the bar is another one then).
    tray: bool,
    /// The window is on the whole display.
    full: bool,
    /// The slice editor is open.
    inspector: bool,
    can_add: bool,
    can_delete: bool,
    can_copy: bool,
    /// Import tray: select mode is on.
    select_mode: bool,
    /// Import tray: how many slices are picked together.
    picked: usize,
    /// Some pie was moved from its place, so there is something to tidy.
    can_tidy: bool,
    /// The list of slices is open over the bar: the groups unfold in the capsule.
    menu_over_bar: bool,
    accent: Color32,
    /// Where the pointer is while a slice of your pie is carried.
    carrying: Option<Pos2>,
}

/// What the bar answered.
struct BarOut {
    todo: Option<BarDo>,
    /// Where the bin is (the place a carried slice is dropped to delete it).
    bin: Rect,
    /// The whole capsule, which the list of slices opens over.
    capsule: Rect,
}

/// The round **+** of the bar, the same size as the **+** of the Flow bar ([`BAR_CELL`]), filled with
/// the accent. It only gets a little darker under the pointer. When no slice can be added to the ring
/// (it is full) it is grey.
fn add_fab(ui: &mut Ui, enabled: bool, accent: Color32, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(BAR_CELL), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    let hot = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hot"), enabled && resp.hovered(), 0.12);
    let press = if enabled && resp.is_pointer_button_down_on() {
        0.08
    } else {
        0.0
    };
    let (fill, ink) = if enabled {
        (
            accent.lerp_to_gamma(Color32::BLACK, 0.12 * hot + press),
            on_accent(accent),
        )
    } else {
        (pal().idle, pal().muted)
    };
    ui.painter().rect_filled(rect, PILL, fill);
    fonts::paint_icon(ui.painter(), rect.center(), symbols::PLUS, 17.0, ink, false);
    let resp = resp.on_hover_text(tip);
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// The groups of slices in the capsule, right after the **+**: one round cell per group on a row that
/// unfolds from the **+** while the list is open and folds back when it closes (the same way as
/// the categories of the Flow canvas). The chosen group has a circle in the accent color under it
/// that travels to the next one, stretching on the way, exactly as the circle of the categories in
/// the Flow bar does (and the switch of the Behavior tab, see [`icon_switch`]): the cells take
/// turns instead of the fill jumping from one to the other. The chosen group narrows the list to
/// it; a second click on it shows everything again.
fn group_strip(ui: &mut Ui, open: bool, sel: &mut Option<usize>, accent: Color32) {
    let ctx = ui.ctx().clone();
    let key = egui::Id::new("pie_group_strip");
    let t = ctx.animate_bool_with_time_and_easing(
        key.with("unfold"),
        open,
        UNFOLD_SECS,
        egui::emath::easing::cubic_out,
    );
    let n = GROUPS.len();
    let full = n as f32 * BAR_CELL + (n - 1) as f32 * BAR_GAP;
    let (rect, _) = ui.allocate_exact_size(vec2(full * t, BAR_CELL), Sense::hover());
    if t <= 0.0 {
        return;
    }
    let cell_at = |i: usize| {
        Rect::from_min_size(
            pos2(rect.min.x + i as f32 * (BAR_CELL + BAR_GAP), rect.min.y),
            Vec2::splat(BAR_CELL),
        )
    };
    // the cells answer only once the row is (nearly) out
    let mut hot = [0.0f32; GROUPS.len()];
    if t > 0.6 {
        for (i, (_, tip)) in GROUPS.iter().enumerate() {
            let id = key.with(("cell", i));
            let resp = ui.interact(cell_at(i).intersect(rect), id, Sense::click());
            resp.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::RadioButton,
                    true,
                    *sel == Some(i),
                    *tip,
                )
            });
            hot[i] = ctx.animate_bool_with_time(id.with("hot"), resp.hovered(), 0.12);
            if resp
                .on_hover_text(*tip)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *sel = if *sel == Some(i) { None } else { Some(i) };
            }
        }
    }
    let chosen = *sel;
    // the move of the circle: from which cell, to which, since when (a circle that comes in from
    // nothing appears where it is chosen instead of travelling there)
    let now = ui.input(|i| i.time);
    let (move_key, last_key) = (key.with("thumb_move"), key.with("thumb_last"));
    let last = ctx
        .data(|d| d.get_temp::<Option<usize>>(last_key))
        .flatten();
    let (mut from, mut to, mut since) = ctx
        .data(|d| d.get_temp::<(usize, usize, f64)>(move_key))
        .unwrap_or((0, 0, -1.0e9));
    if let Some(c) = chosen {
        if last.is_none() {
            (from, to, since) = (c, c, -1.0e9);
        } else if c != to {
            (from, to, since) = (to, c, now);
        }
    }
    ctx.data_mut(|d| {
        d.insert_temp(move_key, (from, to, since));
        d.insert_temp(last_key, chosen);
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
    let shown = ctx.animate_bool_with_time(key.with("thumb_shown"), chosen.is_some(), 0.15);
    let painter = ui.painter().with_clip_rect(rect.expand2(vec2(0.0, 4.0)));
    for (i, h) in hot.iter().enumerate() {
        // the wash under the pointer, on a cell that is not chosen
        if *h > 0.0 && chosen != Some(i) {
            painter.rect_filled(cell_at(i), PILL, pal().hot.gamma_multiply(*h * t));
        }
    }
    if shown > 0.0 {
        painter.rect_filled(thumb, PILL, accent.gamma_multiply(shown * t));
    }
    for (i, (icon, _)) in GROUPS.iter().enumerate() {
        // how much of the ink of the chosen cell this one has: it passes from one to the other
        // with the circle
        let near = if chosen.is_none() {
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
        fonts::paint_icon(&painter, cell_at(i).center(), *icon, 17.0, ink, false);
    }
}

/// The capsule at the bottom of the canvas. Returns what was pressed, where the bin is (the place a
/// carried slice is dropped to delete it) and where the capsule is. `group` is the group of the list
/// of slices that is narrowed to (changed by the cells that unfold after the **+**).
fn bar(ctx: &egui::Context, view: Rect, st: &BarState, group: &mut Option<usize>) -> BarOut {
    let BarState {
        tray,
        full,
        inspector,
        can_add,
        can_delete,
        can_copy,
        select_mode,
        picked,
        can_tidy,
        menu_over_bar,
        accent,
        carrying,
    } = *st;

    let mut todo = None;
    let mut bin = Rect::NOTHING;
    let mut capsule = Rect::NOTHING;
    egui::Area::new(egui::Id::new("pie_editor_bar"))
        .order(egui::Order::Foreground)
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(pos2(view.center().x, view.max.y - 16.0))
        .constrain(false)
        .show(ctx, |ui| {
            let framed = egui::Frame::new()
                .fill(pal().card)
                .stroke(Stroke::new(1.0, pal().border))
                .corner_radius(PILL)
                .inner_margin(egui::Margin::same(BAR_PAD))
                .shadow(popup_shadow())
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = BAR_GAP;
                        if tray {
                            let copy_tip = if picked > 0 {
                                format!("Add the {picked} picked slices to your pie")
                            } else {
                                "Add the selected slice to your pie (or double click it)".to_string()
                            };
                            let copy = ui.add_enabled_ui(can_copy, |ui| {
                                bar_button(ui, symbols::ARROW_RIGHT, &copy_tip, Tone::Accent(accent))
                            });
                            if copy.inner.clicked() && can_copy {
                                todo = Some(BarDo::CopySelected);
                            }
                            if bar_button(
                                ui,
                                symbols::CURSOR_CLICK,
                                if select_mode {
                                    "Select mode is on: click slices to pick them, nothing is dragged. Click to turn it off"
                                } else {
                                    "Select mode: click slices to pick several, then add them together (Shift or Ctrl/Cmd + click does the same)"
                                },
                                if select_mode {
                                    Tone::Accent(accent)
                                } else {
                                    Tone::Plain
                                },
                            )
                            .clicked()
                            {
                                todo = Some(BarDo::SelectMode);
                            }
                            if bar_button(
                                ui,
                                symbols::CHECK_CIRCLE,
                                "Pick every slice of the imported pie, or none (Ctrl/Cmd + A)",
                                Tone::Plain,
                            )
                            .clicked()
                            {
                                todo = Some(BarDo::SelectAll);
                            }
                            bar_divider(ui);
                            if bar_button(
                                ui,
                                symbols::STACK,
                                "Add every slice of the file to the end of the lit ring of your pie",
                                Tone::Plain,
                            )
                            .clicked()
                            {
                                todo = Some(BarDo::AddRing);
                            }
                            if bar_button(
                                ui,
                                symbols::FOLDER_SIMPLE,
                                "Add the whole file as one submenu of the lit ring (always fits when the ring has a free place)",
                                Tone::Plain,
                            )
                            .clicked()
                            {
                                todo = Some(BarDo::AddGroup);
                            }
                            if bar_button(
                                ui,
                                symbols::SWAP,
                                "Use the file as this pie: its slices replace the ones of your pie (Undo brings them back)",
                                Tone::Plain,
                            )
                            .clicked()
                            {
                                todo = Some(BarDo::ReplacePie);
                            }
                            if bar_button(
                                ui,
                                symbols::COPY,
                                "Add all of it as a new pie (needs the apps it lists)",
                                Tone::Plain,
                            )
                            .clicked()
                            {
                                todo = Some(BarDo::AddAll);
                            }
                        } else {
                            if add_fab(ui, can_add, accent, "Add a slice to the pie that is lit")
                                .clicked()
                                && can_add
                            {
                                todo = Some(BarDo::Add);
                            }
                            group_strip(ui, menu_over_bar, group, accent);
                            bar_divider(ui);
                            if bar_button(
                                ui,
                                symbols::DOWNLOAD_SIMPLE,
                                "Import a pie: open it beside yours and drag slices across",
                                Tone::Plain,
                            )
                            .clicked()
                            {
                                todo = Some(BarDo::Import);
                            }
                            let tidy = ui.add_enabled_ui(can_tidy, |ui| {
                                bar_button(
                                    ui,
                                    symbols::SQUARES_FOUR,
                                    "Tidy up: put every pie back where the editor places it",
                                    Tone::Plain,
                                )
                            });
                            if tidy.inner.clicked() && can_tidy {
                                todo = Some(BarDo::Tidy);
                            }
                        }
                        if bar_button(
                            ui,
                            symbols::SCAN,
                            "Bring every pie back into view",
                            Tone::Plain,
                        )
                        .clicked()
                        {
                            todo = Some(BarDo::Fit);
                        }
                        if !tray
                            && bar_button(
                                ui,
                                symbols::SIDEBAR_SIMPLE,
                                if inspector {
                                    "Close the slice editor"
                                } else {
                                    "Slice editor"
                                },
                                if inspector {
                                    Tone::Accent(accent)
                                } else {
                                    Tone::Plain
                                },
                            )
                            .clicked()
                        {
                            todo = Some(BarDo::Inspector);
                        }
                        let del = ui.add_enabled_ui(can_delete, |ui| {
                            bar_button(
                                ui,
                                symbols::TRASH,
                                "Delete the selected slice (or drop a slice here)",
                                Tone::Danger,
                            )
                        });
                        bin = del.inner.rect;
                        if del.inner.clicked() && can_delete {
                            todo = Some(BarDo::Delete);
                        }
                        if carrying.is_some_and(|p| bin.expand(8.0).contains(p)) {
                            ui.painter().circle_filled(
                                bin.center(),
                                bin.width() * 0.7,
                                Color32::from_rgba_unmultiplied(214, 69, 69, 70),
                            );
                        }
                        if full_button(ui, full).clicked() {
                            todo = Some(BarDo::Full);
                        }
                        if tray {
                            bar_divider(ui);
                            if bar_button(
                                ui,
                                symbols::CHECK,
                                "Done: close the imported pie (Esc)",
                                Tone::Accent(accent),
                            )
                            .clicked()
                            {
                                todo = Some(BarDo::Done);
                            }
                        }
                    });
                });
            capsule = framed.response.rect;
        });
    BarOut { todo, bin, capsule }
}

// ------------------------------------------------------------------------------ the list of slices

/// The list of slices that the **+** or the right button opens: a search box and the groups under
/// it, each slice one row. Returns the slice that was picked (Enter picks the first one shown).
fn menu_list(
    ui: &mut Ui,
    catalog: &AddCatalog,
    search: &mut String,
    only: Option<usize>,
) -> Option<Item> {
    let mut picked: Option<Item> = None;
    ui.set_width(MENU_W);
    ui.spacing_mut().item_spacing.y = 2.0;
    let field = ui.add(
        text_field(search)
            .desired_width(ui.available_width())
            .hint_text("Search: screenshot, link, submenu\u{2026}"),
    );
    if !field.has_focus() && search.is_empty() {
        field.request_focus();
    }
    let q = search.trim().to_lowercase();
    let mut first: Option<Item> = None;
    // always MENU_H tall, however few slices a group or a search leaves, so the list does not jump
    // in height as the person switches group
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .min_scrolled_height(MENU_H)
        .max_height(MENU_H)
        .show(ui, |ui| {
            for (gi, (group, list)) in catalog.groups.iter().enumerate() {
                if only.is_some_and(|o| o != gi) {
                    continue;
                }
                let shown: Vec<&Item> = list
                    .iter()
                    .filter(|it| {
                        q.is_empty()
                            || it.name.to_lowercase().contains(&q)
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
                for it in shown {
                    let row = select_item(ui, symbols::resolve(&it.icon), &it.name, false);
                    if first.is_none() {
                        first = Some(it.clone());
                    }
                    if row.clicked() {
                        picked = Some(it.clone());
                    }
                }
            }
            if first.is_none() {
                ui.add_space(8.0);
                ui.label(RichText::new("Nothing like that. Try another word.").color(pal().muted));
            }
        });
    if picked.is_none() && field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        picked = first;
    }
    picked
}

/// Draws the list of slices while `ed.pie_menu` is open and returns the ring and the slice that
/// were picked. It floats over the canvas, over the capsule of the bar when the **+** opened it (the
/// same place as the list of the Flow canvas) or where the right button was pressed, and is no
/// sheet: nothing is dimmed, and a press anywhere else, or Esc, closes it. `capsule` is where the bar
/// was drawn (a press on it does not close the list: the **+** and the groups are in it).
fn slice_menu(ctx: &egui::Context, ed: &mut Ed, capsule: Rect) -> Option<(Vec<usize>, Item)> {
    let mut m = ed.pie_menu.take()?;
    // a press outside the list and the bar closes it (and goes on to whatever is under it)
    let (press, pos) = ctx.input(|i| (i.pointer.any_pressed(), i.pointer.interact_pos()));
    if press
        && let Some(p) = pos
        && !m.rect.contains(p)
        && !capsule.contains(p)
    {
        ed.add_catalog = None;
        ctx.request_repaint();
        return None;
    }
    // the tiles are built once when the list opens, not on every frame (see `AddCatalog`)
    let can_nest = m.ring.len() + 1 < MAX_DEPTH;
    let catalog = match ed.add_catalog.take() {
        Some(c) if c.can_nest == can_nest => c,
        _ => AddCatalog::new(can_nest),
    };
    // keep the list inside the window: as wide as MENU_W and at most MENU_H tall, plus the search
    // box and the margins of its frame
    let (at, pivot) = match m.at {
        Some(mut at) => {
            let room = ed.card;
            let (w, h) = (MENU_W + 16.0, MENU_H + CONTROL_H + 28.0);
            if room.width() > 0.0 && room.height() > 0.0 {
                at.x = at.x.min(room.max.x - w - 8.0).max(room.min.x + 8.0);
                at.y = at.y.min(room.max.y - h - 8.0).max(room.min.y + 8.0);
            }
            (at, Align2::LEFT_TOP)
        }
        None => (
            pos2(capsule.center().x, capsule.min.y - 10.0),
            Align2::CENTER_BOTTOM,
        ),
    };
    let mut picked: Option<Item> = None;
    let group = if m.at.is_none() { m.group } else { None };
    let shown = egui::Area::new(egui::Id::new("pie_slice_menu"))
        .order(egui::Order::Foreground)
        .pivot(pivot)
        .fixed_pos(at)
        .constrain(false)
        .show(ctx, |ui| {
            popup_frame()
                .inner_margin(egui::Margin::same(8))
                .show(ui, |ui| {
                    picked = menu_list(ui, &catalog, &mut m.search, group);
                });
        });
    m.rect = shown.response.rect;
    if let Some(item) = picked {
        ed.add_catalog = None;
        ctx.request_repaint();
        return Some((m.ring, item));
    }
    ed.add_catalog = Some(catalog);
    ed.pie_menu = Some(m);
    None
}

// ------------------------------------------------------------------------------ the changes

/// Deletes the selected slice of the lit ring. Returns whether one went.
fn delete_selected(edit: &mut Config, ed: &mut Ed) -> bool {
    let nav = ed.nav.clone();
    let Some(prof) = edit.profiles.get_mut(ed.profile) else {
        return false;
    };
    if let Some(ring) = ring_mut(&mut prof.items, &nav)
        && let Some(i) = ed.sel
        && i < ring.len()
        && (ring.len() > 1 || !nav.is_empty())
    {
        ring.remove(i);
        ed.sel = None;
        ed.stop_recording();
        return true;
    }
    false
}

/// Selects the slice `i` of `ring` in your pie.
fn select_mine(ed: &mut Ed, ring: Vec<usize>, i: usize) {
    if ed.nav != ring || ed.sel != Some(i) {
        ed.picking_icon = false;
    }
    ed.nav = ring;
    ed.sel = Some(i);
    ed.adding = false;
}

/// Copies the slice `from` (a ring and a place of the imported pie) into the ring `to` of your pie at
/// `place`: its images are written to the icons folder and the flows it runs are brought along, and
/// nothing else of the file. Returns where it landed and whether it can run commands. Nothing is
/// changed on an error.
fn copy_from_import(
    edit: &mut Config,
    profile: usize,
    imp: &PieImport,
    from: &(Vec<usize>, usize),
    to: &[usize],
    place: Place,
) -> Result<(Landed, bool), String> {
    let source = items_at(&imp.bundle.profile.items, &from.0)
        .and_then(|r| r.get(from.1))
        .ok_or("That slice is not in the file any more.")?;
    let items = &mut edit
        .profiles
        .get_mut(profile)
        .ok_or("There is no pie to copy into.")?
        .items;
    // the rules first, on a trial copy, so no file is written for a slice that does not fit
    pie_edit::insert_item(&mut items.clone(), to, place, source.clone())?;
    let mut copy = imp.bundle.install_item(source, &icons_dir(), &tilde);
    let used = flow_ids_used(std::slice::from_ref(&copy));
    let incoming: Vec<Flow> = imp
        .bundle
        .flows()
        .iter()
        .filter(|f| used.contains(&f.id))
        .cloned()
        .collect();
    let runs = item_runs_commands(&copy) || incoming.iter().any(Flow::has_commands);
    let flows_before = edit.flows.clone();
    if let Err(e) = merge_flows(&mut edit.flows, incoming, std::slice::from_mut(&mut copy)) {
        edit.flows = flows_before;
        return Err(e);
    }
    let items = &mut edit.profiles[profile].items;
    match pie_edit::insert_item(items, to, place, copy) {
        Ok(landed) => Ok((landed, runs)),
        Err(e) => {
            edit.flows = flows_before;
            Err(e)
        }
    }
}

/// How much of the imported file is brought into your pie at once.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bulk {
    /// The slices that are picked (or the selected one), to the end of the lit ring.
    Picked,
    /// Every slice of the file, to the end of the lit ring.
    Ring,
    /// The whole file as one submenu in the lit ring.
    Group,
    /// The file becomes the slices of this pie.
    Replace,
}

/// The slices of the imported pie that *Add selected* takes: the ones that are picked, or else the
/// selected one. Each comes with everything in it, and none twice.
fn chosen_sources(imp: &PieImport) -> Vec<Item> {
    let paths: BTreeSet<Vec<usize>> = if imp.picked.is_empty() {
        imp.sel
            .iter()
            .map(|(ring, i)| {
                let mut p = ring.clone();
                p.push(*i);
                p
            })
            .collect()
    } else {
        imp.picked.clone()
    };
    pie_edit::slices_at(&imp.bundle.profile.items, &paths)
}

/// The name of the submenu that holds a whole imported pie: the name of the pie in the file.
fn group_name(imp: &PieImport) -> String {
    let n = imp.bundle.profile.name.trim();
    if n.is_empty() {
        "Imported".to_string()
    } else {
        n.to_string()
    }
}

/// Copies `sources` (slices of the imported pie, with everything in them) to the end of the ring
/// `to` of your pie, all together: when one does not fit, nothing is added and no image is written.
/// Their images are written to the icons folder and the flows they run are brought along, as
/// [`copy_from_import`] does for one slice. Returns the place of the first slice, how many were
/// added and whether one of them can run commands.
fn bring_items(
    edit: &mut Config,
    profile: usize,
    imp: &PieImport,
    sources: Vec<Item>,
    to: &[usize],
) -> Result<(usize, usize, bool), String> {
    let items = &edit
        .profiles
        .get(profile)
        .ok_or("There is no pie to add to.")?
        .items;
    // the rules first, on a trial copy
    pie_edit::insert_many(&mut items.clone(), to, sources.clone())?;
    let mut copies: Vec<Item> = sources
        .iter()
        .map(|s| imp.bundle.install_item(s, &icons_dir(), &tilde))
        .collect();
    let used = flow_ids_used(&copies);
    let incoming: Vec<Flow> = imp
        .bundle
        .flows()
        .iter()
        .filter(|f| used.contains(&f.id))
        .cloned()
        .collect();
    let runs = copies.iter().any(item_runs_commands) || incoming.iter().any(Flow::has_commands);
    let flows_before = edit.flows.clone();
    if let Err(e) = merge_flows(&mut edit.flows, incoming, &mut copies) {
        edit.flows = flows_before;
        return Err(e);
    }
    match pie_edit::insert_many(&mut edit.profiles[profile].items, to, copies) {
        Ok((first, count)) => Ok((first, count, runs)),
        Err(e) => {
            edit.flows = flows_before;
            Err(e)
        }
    }
}

/// Makes the slices of the imported file the slices of the pie `profile` (its name and its apps
/// stay). The images and flows of the file come along. Returns whether anything can run commands.
/// Nothing is changed on an error; Undo brings the old slices back.
fn replace_pie_with_file(
    edit: &mut Config,
    profile: usize,
    imp: &PieImport,
) -> Result<bool, String> {
    let mut bundle = (*imp.bundle).clone();
    let flows = bundle.take_flows();
    let flows_before = edit.flows.clone();
    let mut p = share::install_icons(bundle, &icons_dir(), &tilde);
    let runs = profile_has_commands(&p) || flows.iter().any(Flow::has_commands);
    let merged = merge_flows(&mut edit.flows, flows, &mut p.items);
    let result = merged.and_then(|()| {
        let prof = edit
            .profiles
            .get_mut(profile)
            .ok_or_else(|| "There is no pie to replace.".to_string())?;
        pie_edit::replace_items(&mut prof.items, p.items)?;
        prof.active = pie_edit::first_mode_path(&prof.items);
        Ok(())
    });
    match result {
        Ok(()) => Ok(runs),
        Err(e) => {
            edit.flows = flows_before;
            Err(e)
        }
    }
}

/// The notice after slices were brought in.
fn brought_notice(count: usize, runs: bool) -> String {
    let what = if count == 1 {
        "1 slice".to_string()
    } else {
        format!("{count} slices")
    };
    if runs {
        format!(
            "{what} added. Some can run a shell command, write files or send messages - review them before you Save."
        )
    } else {
        format!("{what} added to your pie. Press Save to apply.")
    }
}

/// A profile file that was copied as text (what the clipboard holds), if that is what `text` is.
/// Anything else, a big text included, is not read.
fn pasted_profile(text: &str) -> Option<Bundle> {
    let t = text.trim_start();
    if !t.starts_with('{') || t.len() as u64 > share::MAX_FILE_BYTES {
        return None;
    }
    if !t.contains("cursorpie_profile") {
        return None;
    }
    share::parse_bundle(t).ok()
}

// ------------------------------------------------------------------------------ the page

impl App {
    /// The pie editor / import tray, in the room under the profile toolbar of the Menu tab.
    pub(in crate::ui::settings) fn pie_canvas_page(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        area: Rect,
    ) {
        let App { edit, ed, tx, .. } = self;
        let tx_ref: &Sender<Msg> = tx;
        let accent = rgb(edit.accent);
        let pi = ed.profile.min(edit.profiles.len().saturating_sub(1));
        if edit.profiles.is_empty() {
            return;
        }
        // a path that an undo or a delete left behind
        if items_at(&edit.profiles[pi].items, &ed.nav).is_none() {
            ed.reset_nav();
        }
        if let Some(ring) = items_at(&edit.profiles[pi].items, &ed.nav)
            && ed.sel.is_some_and(|i| i >= ring.len())
        {
            ed.sel = None;
        }
        let sel_before = ed.sel;
        // pies that an undo or a move of a slice left behind
        {
            let top = &edit.profiles[pi].items;
            ed.pie_picked.retain(|path| items_at(top, path).is_some());
        }
        let tray = ed.pie_import.is_some();
        let whole = Rect::from_min_size(area.min, vec2(area.width(), area.height().max(260.0)));
        let insp_on = !tray && (ed.pie_insp || ed.sel.is_some());
        let insp_w = INSP_W.min(whole.width() * 0.45);
        // a profile file dropped on the window opens in the import tray, no file chooser needed
        // (with the slice editor open a dropped file is an icon, see `inspector::icon_drop`)
        if !insp_on {
            let dropped: Option<std::path::PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .map(|f| f.path().to_path_buf())
                    .find(|p| {
                        p.extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
                    })
            });
            if let Some(path) = dropped {
                let _ = tx_ref.send(Msg::ProfileImportPicked(
                    path.to_string_lossy().into_owned(),
                ));
            }
        }

        // ---- where the canvases go
        let (src_rect, div_rect, mine_rect) = if tray {
            let free = whole.width() - DIVIDER_W;
            let left = (free * ed.pie_split.clamp(0.25, 0.75)).round();
            let src = Rect::from_min_size(whole.min, vec2(left, whole.height()));
            let div = Rect::from_min_size(
                pos2(src.max.x, whole.min.y),
                vec2(DIVIDER_W, whole.height()),
            );
            let mine = Rect::from_min_max(pos2(div.max.x, whole.min.y), whole.max);
            (Some(src), Some(div), mine)
        } else if insp_on {
            (
                None,
                None,
                Rect::from_min_max(whole.min, pos2(whole.max.x - insp_w - GAP, whole.max.y)),
            )
        } else {
            (None, None, whole)
        };
        // the divider moves the split
        if let (Some(div), Some(_)) = (div_rect, src_rect) {
            let r = ui.interact(div, egui::Id::new("pie_split"), Sense::drag());
            if r.dragged() {
                let free = whole.width() - DIVIDER_W;
                let left = (src_rect.map_or(0.0, |s| s.width()) + r.drag_delta().x) / free;
                ed.pie_split = left.clamp(0.25, 0.75);
            }
            if r.hovered() || r.dragged() {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            let grip = Rect::from_center_size(div.center(), vec2(4.0, 40.0));
            ui.painter().rect_filled(
                grip,
                PILL,
                if r.hovered() || r.dragged() {
                    accent
                } else {
                    pal().border
                },
            );
        }

        let pinch = platform::take_pinch(ctx);
        let show_labels = edit.show_labels;
        let profile_name = edit.profiles[pi].name.clone();
        let nav = ed.nav.clone();
        let mine_sel = ed.sel.map(|i| (nav.clone(), i));
        let mut over = false;
        let mut imp = ed.pie_import.take();

        // ---- the imported pie (left half)
        let mut src_out = None;
        if let (Some(imp), Some(r)) = (imp.as_mut(), src_rect) {
            let PieImport {
                label,
                bundle,
                sel,
                picked,
                select_mode,
                view,
                textures,
                ..
            } = imp;
            let title = if bundle.profile.name.trim().is_empty() {
                label.clone()
            } else {
                bundle.profile.name.clone()
            };
            src_out = Some(pane(
                ui,
                ctx,
                ed,
                Pane {
                    id: "pie_import_canvas",
                    rect: r,
                    items: &bundle.profile.items,
                    title,
                    badge: "IMPORTED \u{b7} NOT ADDED YET",
                    role: Role::Source,
                    view,
                    icons: Icons::Staged { bundle, textures },
                    active: Vec::new(),
                    sel: sel.clone(),
                    picked: picked.clone(),
                    select_mode: *select_mode,
                    fit_key: 0,
                    show_labels,
                    accent,
                    pinch,
                    hint: "Drag a slice across, double click it, or use select mode to pick several. \u{26a0} marks a command.",
                },
            ));
        }

        // ---- your pie
        let mut view = std::mem::take(&mut ed.pie_view);
        let mine_out = {
            let items = &edit.profiles[pi].items;
            pane(
                ui,
                ctx,
                ed,
                Pane {
                    id: "pie_editor_canvas",
                    rect: mine_rect,
                    items,
                    title: profile_name,
                    badge: if tray { "YOUR PIE" } else { "PIE EDITOR" },
                    role: Role::Mine,
                    view: &mut view,
                    icons: Icons::Mine,
                    active: nav.clone(),
                    sel: mine_sel,
                    picked: BTreeSet::new(),
                    select_mode: false,
                    fit_key: pi + 1,
                    show_labels,
                    accent,
                    pinch,
                    hint: if tray {
                        "Drop on a slice to put it before that one, on a pie's middle to add it last."
                    } else {
                        "Click to edit \u{b7} drag a pie's middle to move it \u{b7} drag the background to select \u{b7} middle button pans \u{b7} Ctrl/Cmd + C, X, V"
                    },
                },
            )
        };
        over |= mine_out.over;
        over |= src_out.as_ref().is_some_and(|o| o.over);
        crate::tap::FLOW_CANVAS_HOVER.store(over, std::sync::atomic::Ordering::SeqCst);

        // ---- the bar
        let view_area = Rect::from_min_max(whole.min, pos2(whole.max.x, whole.max.y - STATUS_H));
        let carrying = ed
            .pie_drag
            .as_ref()
            .filter(|d| !d.from_import)
            .and_then(|_| ctx.input(|i| i.pointer.latest_pos()));
        let can_delete = ed.sel.is_some_and(|i| {
            items_at(&edit.profiles[pi].items, &nav)
                .is_some_and(|r| i < r.len() && (r.len() > 1 || !nav.is_empty()))
        });
        let can_add = items_at(&edit.profiles[pi].items, &nav).is_some_and(|r| r.len() < MAX_ITEMS);
        let can_copy = imp
            .as_ref()
            .is_some_and(|i| i.sel.is_some() || !i.picked.is_empty());
        let (select_mode, picked_n) = imp
            .as_ref()
            .map_or((false, 0), |i| (i.select_mode, i.picked.len()));
        let mut group = ed.pie_menu.as_ref().and_then(|m| m.group);
        let bar_out = bar(
            ctx,
            view_area,
            &BarState {
                tray,
                full: ed.flow_full.is_some(),
                inspector: insp_on,
                can_add,
                can_delete,
                can_copy,
                select_mode,
                picked: picked_n,
                can_tidy: pie_edit::has_layout(&edit.profiles[pi].items),
                menu_over_bar: ed.pie_menu.as_ref().is_some_and(|m| m.at.is_none()),
                accent,
                carrying,
            },
            &mut group,
        );
        if let Some(m) = ed.pie_menu.as_mut() {
            m.group = group;
        }
        let (bar_do, bin, capsule) = (bar_out.todo, bar_out.bin, bar_out.capsule);
        ed.trash_rect = (can_delete).then_some(bin);

        // ---- what the pointer did on your canvas
        let additive = ctx.input(|i| i.modifiers.shift || i.modifiers.command);
        match mine_out.click {
            Some(Click::Slice(ring, i)) => {
                ed.pie_picked.clear();
                select_mine(ed, ring, i);
            }
            Some(Click::Ring(ring)) => {
                if additive {
                    if !ed.pie_picked.remove(&ring) {
                        ed.pie_picked.insert(ring.clone());
                    }
                } else {
                    ed.pie_picked.clear();
                    ed.pie_picked.insert(ring.clone());
                }
                ed.nav = ring;
                ed.sel = None;
                ed.adding = false;
            }
            Some(Click::Empty) => {
                ed.sel = None;
                ed.pie_insp = false;
                ed.pie_picked.clear();
            }
            None => {}
        }
        // the middle of a pie was pressed: it and the pies selected with it are carried
        if let Some((path, add)) = mine_out.pie_press.clone() {
            if !ed.pie_picked.contains(&path) {
                if !add {
                    ed.pie_picked.clear();
                }
                ed.pie_picked.insert(path.clone());
            }
            ed.nav = path;
            ed.sel = None;
            ed.adding = false;
            ed.pie_moving = true;
        }
        if ed.pie_moving && mine_out.pie_delta != Vec2::ZERO {
            pie_edit::nudge_pies(
                &mut edit.profiles[pi].items,
                ed.pie_picked.iter(),
                mine_out.pie_delta.x,
                mine_out.pie_delta.y,
            );
            ctx.request_repaint();
        }
        if mine_out.pie_dropped {
            ed.pie_moving = false;
            pie_edit::settle_pies(&mut edit.profiles[pi].items, ed.pie_picked.iter());
        }
        // a selection box was let go
        if let Some(touched) = mine_out.marquee.as_ref() {
            ed.pie_picked = pick_pies(&ed.pie_marquee_base, touched);
            ed.pie_marquee_base.clear();
            if let Some(last) = touched.last() {
                ed.nav = last.clone();
                ed.sel = None;
            }
        }
        // the right button opens the list of slices where it was pressed
        if let Some((ring, at)) = mine_out.secondary.clone()
            && !tray
        {
            ed.nav = ring.clone();
            ed.sel = None;
            ed.pie_menu = Some(PieMenu {
                at: Some(at),
                ring,
                rect: Rect::NOTHING,
                group: None,
                search: String::new(),
            });
        }
        // what the pointer did on the imported canvas
        // (select mode, or Shift / Ctrl/Cmd, picks several slices; a plain click selects one)
        if let (Some(imp), Some(o)) = (imp.as_mut(), src_out.as_ref()) {
            let multi = imp.select_mode || additive;
            match &o.click {
                Some(Click::Slice(ring, i)) => {
                    if multi {
                        let mut path = ring.clone();
                        path.push(*i);
                        if !imp.picked.remove(&path) {
                            imp.picked.insert(path);
                        }
                        imp.sel = None;
                    } else {
                        imp.picked.clear();
                        imp.sel = Some((ring.clone(), *i));
                    }
                }
                // the middle of a pie picks (or lets go of) every slice of that ring
                Some(Click::Ring(ring)) if multi => {
                    let all = pie_edit::ring_paths(&imp.bundle.profile.items, ring);
                    if !all.is_empty() {
                        if all.iter().all(|p| imp.picked.contains(p)) {
                            for p in &all {
                                imp.picked.remove(p);
                            }
                        } else {
                            imp.picked.extend(all);
                        }
                    }
                    imp.sel = None;
                }
                Some(Click::Ring(_)) | Some(Click::Empty) if !multi => {
                    imp.sel = None;
                    imp.picked.clear();
                }
                Some(Click::Ring(_)) | Some(Click::Empty) | None => {}
            }
        }

        // ---- a slice was let go
        let drop_at = mine_out
            .dropped
            .map(|p| (p, false))
            .or_else(|| src_out.as_ref().and_then(|o| o.dropped).map(|p| (p, true)));
        if let Some((at, from_import)) = drop_at
            && let Some(drag) = ed.pie_drag.take()
        {
            let on_bin = !from_import && bin.expand(8.0).contains(at);
            let on_mine = mine_rect.contains(at)
                && !Rect::from_min_max(
                    pos2(mine_rect.min.x, mine_rect.max.y - STATUS_H),
                    mine_rect.max,
                )
                .contains(at);
            // where on your pie: before a slice, last in a ring, or (from the imported pie only)
            // last in the lit ring when the canvas is empty there
            let target: Option<(Vec<usize>, Place)> = if on_mine {
                match hit_at(&mine_out.geoms, at) {
                    Hit::Slice(g, i) => Some((mine_out.geoms[g].path.clone(), Place::At(i))),
                    Hit::Center(g) => Some((mine_out.geoms[g].path.clone(), Place::End)),
                    Hit::None if from_import => Some((nav.clone(), Place::End)),
                    Hit::None => None,
                }
            } else {
                None
            };
            if on_bin {
                ed.nav = drag.ring.clone();
                ed.sel = Some(drag.idx);
                if delete_selected(edit, ed) {
                    notice(tx_ref, ctx, "Slice deleted. Undo brings it back.");
                }
            } else if let Some((to, place)) = target {
                if from_import {
                    if let Some(imp) = imp.as_ref() {
                        match copy_from_import(
                            edit,
                            pi,
                            imp,
                            &(drag.ring.clone(), drag.idx),
                            &to,
                            place,
                        ) {
                            Ok(((ring, i), runs)) => {
                                select_mine(ed, ring, i);
                                ed.icons.revalidate();
                                notice(
                                    tx_ref,
                                    ctx,
                                    if runs {
                                        "Slice copied. It can run a shell command, write files or send messages - review it before you Save."
                                    } else {
                                        "Slice copied into your pie. Press Save to apply."
                                    },
                                );
                            }
                            Err(e) => notice(tx_ref, ctx, e),
                        }
                    }
                } else if !(drag.ring == to && place == Place::At(drag.idx)) {
                    match pie_edit::move_item(
                        &mut edit.profiles[pi].items,
                        &drag.ring,
                        drag.idx,
                        &to,
                        place,
                    ) {
                        Ok((ring, i)) => {
                            select_mine(ed, ring, i);
                            ed.stop_recording();
                        }
                        Err(e) => notice(tx_ref, ctx, e),
                    }
                }
            }
        }

        // ---- double click on the imported pie copies the slice
        let mut copy_now: Option<(Vec<usize>, usize)> = None;
        if let Some(o) = src_out.as_ref()
            && let Some(d) = &o.double
        {
            copy_now = Some(d.clone());
        }
        let mut close_tray = false;
        let mut add_all = false;
        let mut bulk: Option<Bulk> = None;
        // any button of the bar but the + puts the list of slices away
        if bar_do.is_some() && !matches!(bar_do, Some(BarDo::Add)) {
            ed.close_pie_menu();
        }
        match bar_do {
            Some(BarDo::Add) => {
                if !ed.close_pie_menu() {
                    ed.pie_menu = Some(PieMenu {
                        at: None,
                        ring: nav.clone(),
                        rect: Rect::NOTHING,
                        group: None,
                        search: String::new(),
                    });
                }
            }
            Some(BarDo::Tidy) => {
                let n = pie_edit::tidy_pies(&mut edit.profiles[pi].items);
                view.fitted_for = None;
                if n > 0 {
                    notice(tx_ref, ctx, "Every pie is back where the editor places it.");
                }
            }
            Some(BarDo::Import) => spawn_file_picker(
                tx_ref.clone(),
                ctx.clone(),
                platform::FileKind::Json,
                "Choose a CursorPie profile",
                |file| Some(Msg::ProfileImportPicked(file)),
            ),
            Some(BarDo::Fit) => {
                view.fitted_for = None;
                if let Some(imp) = imp.as_mut() {
                    imp.view.fitted_for = None;
                }
            }
            Some(BarDo::Inspector) => {
                if ed.pie_insp || ed.sel.is_some() {
                    ed.pie_insp = false;
                    ed.sel = None;
                } else {
                    ed.pie_insp = true;
                }
            }
            Some(BarDo::Delete) => {
                if delete_selected(edit, ed) {
                    notice(tx_ref, ctx, "Slice deleted. Undo brings it back.");
                }
            }
            Some(BarDo::Full) => ed.flow_full_req = true,
            Some(BarDo::CopySelected) => bulk = Some(Bulk::Picked),
            Some(BarDo::SelectMode) => {
                if let Some(imp) = imp.as_mut() {
                    imp.select_mode = !imp.select_mode;
                }
            }
            Some(BarDo::SelectAll) => {
                if let Some(imp) = imp.as_mut() {
                    let all = pie_edit::top_paths(&imp.bundle.profile.items);
                    if imp.picked == all {
                        imp.picked.clear();
                    } else {
                        imp.picked = all;
                    }
                    imp.sel = None;
                }
            }
            Some(BarDo::AddRing) => bulk = Some(Bulk::Ring),
            Some(BarDo::AddGroup) => bulk = Some(Bulk::Group),
            Some(BarDo::ReplacePie) => bulk = Some(Bulk::Replace),
            Some(BarDo::AddAll) => add_all = true,
            Some(BarDo::Done) => close_tray = true,
            None => {}
        }
        if let Some(from) = copy_now
            && let Some(imp) = imp.as_ref()
        {
            match copy_from_import(edit, pi, imp, &from, &nav, Place::End) {
                Ok(((ring, i), runs)) => {
                    select_mine(ed, ring, i);
                    ed.icons.revalidate();
                    notice(
                        tx_ref,
                        ctx,
                        if runs {
                            "Slice copied. It can run a shell command, write files or send messages - review it before you Save."
                        } else {
                            "Slice copied into your pie. Press Save to apply."
                        },
                    );
                }
                Err(e) => notice(tx_ref, ctx, e),
            }
        }
        if let Some(kind) = bulk
            && let Some(imp) = imp.as_ref()
        {
            let result: Result<(usize, usize, bool), String> = match kind {
                Bulk::Picked => {
                    let sources = chosen_sources(imp);
                    if sources.is_empty() {
                        Err("Pick a slice of the imported pie first.".to_string())
                    } else {
                        bring_items(edit, pi, imp, sources, &nav)
                    }
                }
                Bulk::Ring => bring_items(edit, pi, imp, imp.bundle.profile.items.clone(), &nav),
                Bulk::Group => {
                    let group = pie_edit::group_item(
                        &group_name(imp),
                        "ico:folder-simple",
                        imp.bundle.profile.items.clone(),
                        nav.len(),
                    );
                    bring_items(edit, pi, imp, vec![group], &nav)
                }
                Bulk::Replace => replace_pie_with_file(edit, pi, imp).map(|runs| (0, 0, runs)),
            };
            match result {
                Ok((_, _, runs)) if kind == Bulk::Replace => {
                    ed.reset_nav();
                    ed.sel = None;
                    ed.pie_picked.clear();
                    ed.stop_recording();
                    ed.icons.revalidate();
                    view.fitted_for = None;
                    close_tray = true;
                    notice(
                        tx_ref,
                        ctx,
                        if runs {
                            "Pie replaced. It can run shell commands, write files or send messages - review them before you Save. Undo brings your old pie back."
                        } else {
                            "Pie replaced. Press Save to apply, or Undo to bring your old pie back."
                        },
                    );
                }
                Ok((first, count, runs)) => {
                    select_mine(ed, nav.clone(), first + count - 1);
                    ed.pie_picked.clear();
                    ed.stop_recording();
                    ed.icons.revalidate();
                    view.fitted_for = None;
                    notice(tx_ref, ctx, brought_notice(count, runs));
                }
                Err(e) => notice(tx_ref, ctx, e),
            }
        }
        if add_all && let Some(imp) = imp.as_ref() {
            match add_file_as_pie(edit, imp) {
                Ok((index, runs)) => {
                    ed.profile = index;
                    ed.reset_nav();
                    ed.stop_recording();
                    ed.icons.revalidate();
                    ed.pie_view.fitted_for = None;
                    close_tray = true;
                    notice(
                        tx_ref,
                        ctx,
                        if runs {
                            "Pie added. It can run shell commands, write files or send messages - review them before you Save."
                        } else {
                            "Pie added. Review it, then press Save to apply."
                        },
                    );
                }
                Err(e) => notice(tx_ref, ctx, e),
            }
        }
        // Delete or Backspace removes the selected slice of your pie, unless a field is being typed in
        let keys_free = !ctx.egui_wants_keyboard_input()
            && ed.recording.is_none()
            && !ed.sheet_open()
            && !choice_sheet_open(ctx)
            && ed.pie_menu.is_none()
            && !egui::Popup::is_any_open(ctx);
        // Ctrl/Cmd + A selects every pie
        if keys_free
            && !tray
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A))
        {
            ed.pie_picked = layout(&edit.profiles[pi].items)
                .into_iter()
                .map(|b| b.path)
                .collect();
        }
        // Ctrl/Cmd + A in the import tray picks every slice of the imported pie
        if keys_free
            && tray
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A))
            && let Some(imp) = imp.as_mut()
        {
            imp.picked = pie_edit::top_paths(&imp.bundle.profile.items);
            imp.sel = None;
        }
        // Ctrl/Cmd + C copies the selected slice (or the selected pies), X cuts and V pastes at the end
        // of the lit pie. They go through the system clipboard as text, like the blocks of a flow
        if keys_free
            && !tray
            && let Some(clipped) = clip::take(ctx)
        {
            let cut = matches!(clipped, Clip::Cut);
            let lit = ed.nav.clone();
            match clipped {
                Clip::Copy | Clip::Cut => {
                    let items = &edit.profiles[pi].items;
                    let (slices, paths): (Vec<Item>, BTreeSet<Vec<usize>>) = if let Some(i) = ed.sel
                    {
                        let mut path = lit.clone();
                        path.push(i);
                        (
                            item_at(items, &path).cloned().into_iter().collect(),
                            BTreeSet::from([path]),
                        )
                    } else {
                        (
                            pie_edit::slices_at(items, &ed.pie_picked),
                            ed.pie_picked.clone(),
                        )
                    };
                    if let Some(text) = pie_edit::copy_slices(&slices) {
                        ctx.copy_text(text);
                        if cut && pie_edit::remove_slices(&mut edit.profiles[pi].items, &paths) > 0
                        {
                            ed.sel = None;
                            ed.pie_picked.clear();
                            ed.stop_recording();
                            notice(
                                tx_ref,
                                ctx,
                                "Cut. Paste it with Ctrl/Cmd + V; Undo brings it back.",
                            );
                        }
                    }
                }
                Clip::Paste(text) => {
                    match pie_edit::paste_slices(&mut edit.profiles[pi].items, &lit, &text) {
                        PasteSlices::Done { first, count, runs } => {
                            select_mine(ed, lit.clone(), first + count - 1);
                            ed.pie_picked.clear();
                            ed.icons.revalidate();
                            if runs {
                                notice(
                                    tx_ref,
                                    ctx,
                                    "Pasted. It can run a shell command, write files or send messages - review it before you Save.",
                                );
                            }
                        }
                        PasteSlices::Full => notice(
                            tx_ref,
                            ctx,
                            format!("A ring holds {MAX_ITEMS} slices at most."),
                        ),
                        PasteSlices::TooDeep => notice(
                            tx_ref,
                            ctx,
                            format!(
                                "Submenus nest {MAX_DEPTH} rings deep at most, and that copy would go deeper."
                            ),
                        ),
                        // not slices: a whole profile file copied as text opens in the import tray
                        PasteSlices::NotSlices => {
                            if let Some(bundle) = pasted_profile(&text) {
                                let runs = profile_has_commands(&bundle.profile)
                                    || bundle.flows().iter().any(Flow::has_commands);
                                imp = Some(PieImport::new(
                                    bundle,
                                    "Pasted from the clipboard".to_string(),
                                    runs,
                                ));
                                ed.pie_drag = None;
                                ed.pie_split = IMPORT_SPLIT;
                                ed.sel = None;
                                ed.adding = false;
                                ed.picking_icon = false;
                                ed.close_pie_menu();
                                notice(
                                    tx_ref,
                                    ctx,
                                    "Pie pasted beside yours. Pick the slices you need, or use the buttons to add it all.",
                                );
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            }
        }
        if keys_free
            && ed.sel.is_some()
            && ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
            })
            && delete_selected(edit, ed)
        {
            notice(tx_ref, ctx, "Slice deleted. Undo brings it back.");
        }

        ed.pie_view = view;
        ed.pie_import = if close_tray { None } else { imp };
        if close_tray {
            ed.pie_drag = None;
        }

        // ---- the slice editor, at the right of the pie editor (the inspector of the Flow tab)
        let nav = ed.nav.clone(); // may have just changed
        if ed.sel != sel_before {
            ed.picking_icon = false;
        }
        if insp_on {
            let right = Rect::from_min_max(pos2(whole.max.x - insp_w, whole.min.y), whole.max);
            let flow_list: Vec<(String, String)> = edit
                .flows
                .iter()
                .map(|f| (f.id.clone(), f.name.clone()))
                .collect();
            let mouse = crate::mouse::active_buttons();
            // the panel is drawn the way the inspector of the Flow tab is: as tall as the canvas, on
            // paper, with its content a little inside it (see `flow_inspect::panel`)
            let painter = ui.painter_at(right);
            painter.rect_filled(right, panel_r(), pal().card);
            painter.rect_stroke(
                right,
                panel_r(),
                Stroke::new(1.0, pal().border),
                StrokeKind::Inside,
            );
            let inner = right.shrink(INSP_PAD);
            let env = Env {
                ctx,
                tx: tx_ref,
                accent,
                profile: pi,
                width: inner.width(),
                flat: true,
                height: inner.height(),
                trigger: edit.trigger_button,
                mouse: &mouse,
                bounds: ed.card,
                flows: &flow_list,
            };
            ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(right));
                ui.spacing_mut().scroll = egui::style::ScrollStyle::floating();
                egui::ScrollArea::vertical()
                    .id_salt("pie_editor_inspector")
                    .max_height(inner.height())
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(inner.width());
                        if let Some(prof) = edit.profiles.get_mut(pi)
                            && let Some(ring) = ring_mut(&mut prof.items, &nav)
                        {
                            item_inspector(ui, ed, &env, ring, &nav);
                        }
                    });
            });
        }
        // the list of slices floats over the canvas (no sheet); a slice picked goes to the end of
        // the ring it was opened for
        if let Some((ring_path, item)) = slice_menu(ctx, ed, capsule) {
            let idx = edit
                .profiles
                .get_mut(pi)
                .and_then(|prof| ring_mut(&mut prof.items, &ring_path))
                .filter(|ring| ring.len() < MAX_ITEMS)
                .map(|ring| {
                    ring.push(item);
                    ring.len() - 1
                });
            match idx {
                Some(i) => select_mine(ed, ring_path, i),
                None => notice(
                    tx_ref,
                    ctx,
                    format!("A ring holds {MAX_ITEMS} slices at most."),
                ),
            }
        }
    }
}

/// Adds everything in the imported file as a new pie, the way importing worked before the tray: its
/// images and flows come along, and the pie needs an app (there is one general pie only). Returns the
/// index of the new pie and whether it can run commands. Nothing is changed on an error.
fn add_file_as_pie(edit: &mut Config, imp: &PieImport) -> Result<(usize, bool), String> {
    let mut bundle = (*imp.bundle).clone();
    let flows = bundle.take_flows();
    let flows_before = edit.flows.clone();
    let mut p = share::install_icons(bundle, &icons_dir(), &tilde);
    let runs = profile_has_commands(&p) || flows.iter().any(Flow::has_commands);
    let result = merge_flows(&mut edit.flows, flows, &mut p.items)
        .and_then(|()| share::add_profile(edit, p));
    match result {
        Ok(index) => Ok((index, runs)),
        Err(e) => {
            edit.flows = flows_before;
            Err(format!(
                "{e} Use \"Add every slice\", \"As a submenu\" or \"Use as this pie\" instead, or pick slices and add them."
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(n: &str) -> Item {
        Item::run(n, "ico:app-window", Action::Nothing)
    }

    fn sub(n: &str, kids: Vec<Item>) -> Item {
        Item::submenu(n, "ico:folder", kids)
    }

    #[test]
    fn every_ring_gets_a_pie_and_a_submenu_sits_next_to_its_slice() {
        let items = vec![
            leaf("a"),
            sub("s", vec![leaf("x"), sub("t", vec![leaf("y")])]),
            sub("u", vec![]),
        ];
        let boxes = layout(&items);
        // the top ring, s, s/t and u
        assert_eq!(boxes.len(), 4);
        assert!(boxes[0].path.is_empty() && boxes[0].parent.is_none());
        assert_eq!(boxes[1].path, vec![1]);
        assert_eq!(boxes[1].parent, Some((0, 1)));
        assert_eq!(boxes[2].path, vec![1, 1]);
        assert_eq!(boxes[2].parent, Some((1, 1)));
        assert_eq!(boxes[3].path, vec![2]);
        assert_eq!(boxes[3].parent, Some((0, 2)));
        // one column for each depth
        assert_eq!(
            boxes.iter().map(|b| b.depth).collect::<Vec<_>>(),
            vec![0, 1, 2, 1]
        );
        // two pies of one column never share a row
        assert!(boxes[3].y > boxes[1].y);
        // the top pie is between its two submenus
        assert!((boxes[0].y - (boxes[1].y + boxes[3].y) / 2.0).abs() < 0.01);
    }

    #[test]
    fn a_pie_without_submenus_is_one_pie() {
        let boxes = layout(&[leaf("a"), leaf("b")]);
        assert_eq!(boxes.len(), 1);
        assert_eq!(layout(&[]).len(), 1);
    }

    #[test]
    fn the_view_fits_every_pie() {
        let items = vec![sub("s", vec![leaf("x")]), sub("u", vec![leaf("y")])];
        let boxes = layout(&items);
        let mut view = PieView::default();
        let size = vec2(700.0, 500.0);
        fit_view(&mut view, &boxes, size);
        assert!((ZOOM_MIN..=1.0).contains(&view.zoom));
        for b in &boxes {
            let c = scene_center(b) * view.zoom + view.pan;
            let r = PIE_D / 2.0 * view.zoom;
            assert!(c.x - r >= -0.5 && c.x + r <= size.x + 0.5, "{c:?}");
            assert!(c.y - r >= -0.5 && c.y + r <= size.y + 0.5, "{c:?}");
        }
    }

    #[test]
    fn a_click_finds_the_slice_and_the_middle_of_the_pie_under_it() {
        let g = Geom {
            path: vec![],
            c: pos2(100.0, 100.0),
            outer: 80.0,
            inner: 30.0,
            len: 4,
            parent: None,
        };
        let geoms = [g];
        assert!(matches!(hit_at(&geoms, pos2(100.0, 100.0)), Hit::Center(0)));
        assert!(matches!(hit_at(&geoms, pos2(300.0, 300.0)), Hit::None));
        // slice 0 is at the left (see `geometry::slice_start`)
        assert!(matches!(
            hit_at(&geoms, pos2(40.0, 100.0)),
            Hit::Slice(0, 0)
        ));
        let empty = [Geom {
            len: 0,
            path: vec![],
            c: pos2(0.0, 0.0),
            outer: 80.0,
            inner: 30.0,
            parent: None,
        }];
        assert!(matches!(hit_at(&empty, pos2(60.0, 0.0)), Hit::Center(0)));
    }

    #[test]
    fn the_wire_runs_from_one_end_to_the_other() {
        let pts = wire_points(pos2(0.0, 0.0), pos2(100.0, 50.0), 40.0);
        assert_eq!(pts.first().copied(), Some(pos2(0.0, 0.0)));
        let last = pts.last().copied().unwrap();
        assert!((last - pos2(100.0, 50.0)).length() < 0.01);
    }

    #[test]
    fn text_sizes_come_in_half_points() {
        assert_eq!(text_size(11.5, 1.0), 11.5);
        assert_eq!(text_size(11.5, 0.3), 6.0);
        assert_eq!(
            text_size(14.0, 1.26) * 2.0,
            (text_size(14.0, 1.26) * 2.0).round()
        );
    }

    #[test]
    fn a_moved_pie_moves_alone_and_the_parent_keeps_its_place() {
        let mut items = vec![sub("s", vec![leaf("x")]), sub("u", vec![leaf("y")])];
        let before = layout(&items);
        items[1].at = Some([30.0, -20.0]);
        let after = layout(&items);
        assert_eq!(after.len(), before.len());
        // the top pie did not move, and neither did the other submenu
        assert_eq!(scene_center(&after[0]), scene_center(&before[0]));
        assert_eq!(scene_center(&after[1]), scene_center(&before[1]));
        // the moved one is where it was put
        assert_eq!(
            scene_center(&after[2]) - scene_center(&before[2]),
            vec2(30.0, -20.0)
        );
    }

    #[test]
    fn the_pie_on_top_is_the_one_a_click_finds() {
        let mk = |c: Pos2| Geom {
            path: vec![],
            c,
            outer: 80.0,
            inner: 30.0,
            len: 4,
            parent: None,
        };
        // two pies that overlap: the later one is drawn over the first
        let geoms = [mk(pos2(100.0, 100.0)), mk(pos2(190.0, 100.0))];
        assert!(matches!(hit_at(&geoms, pos2(190.0, 100.0)), Hit::Center(1)));
        assert!(matches!(hit_at(&geoms, pos2(100.0, 100.0)), Hit::Center(0)));
        // in the part both cover, the later pie wins
        assert!(matches!(
            hit_at(&geoms, pos2(150.0, 100.0)),
            Hit::Slice(1, _)
        ));
    }

    #[test]
    fn a_selection_box_touches_the_pies_it_overlaps() {
        let r = Rect::from_min_max(pos2(0.0, 0.0), pos2(50.0, 50.0));
        // the middle inside, a rim that reaches the box, and a pie that is too far
        assert!(circle_touches_rect(pos2(25.0, 25.0), 5.0, r));
        assert!(circle_touches_rect(pos2(100.0, 25.0), 60.0, r));
        assert!(!circle_touches_rect(pos2(200.0, 25.0), 60.0, r));
        // a corner is measured as a distance, not as a square
        assert!(!circle_touches_rect(pos2(80.0, 80.0), 40.0, r));
    }

    #[test]
    fn a_box_with_shift_toggles_and_a_plain_box_replaces() {
        let a = vec![0];
        let b = vec![1];
        let c = vec![2];
        let none = BTreeSet::new();
        assert_eq!(
            pick_pies(&none, &[a.clone(), b.clone()]),
            BTreeSet::from([a.clone(), b.clone()])
        );
        assert!(pick_pies(&none, &[]).is_empty());
        let had = BTreeSet::from([a.clone(), b.clone()]);
        assert_eq!(
            pick_pies(&had, &[b.clone(), c.clone()]),
            BTreeSet::from([a, c])
        );
    }
}
