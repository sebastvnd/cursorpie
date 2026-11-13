//! Draws a panel ([`Form`]) in the look of Settings: a rounded card with a header, the widgets and
//! the buttons. This file only paints and reports *what the person did* as an [`Event`]; what an
//! answer is, what an empty required box does and when the window closes are decided elsewhere
//! (`panel::form` and `ui::app::panel`), so a different drawing (a themed one, a compact one) can
//! replace this file without touching any rule.
//!
//! The card has a fixed height worked out by [`crate::panel::layout`] before the first frame; the
//! header and the buttons stay in place, and the widgets between them scroll when there are more
//! than the display holds. Every height used here comes from the same constants.

use super::theme::{CARD_RADIUS, CONTROL_R, pal, wash, window_outline, window_shadow};
use super::widgets::{
    Look, accent_picker, button_frame, fit_galley, icon_button_look, select_capped, slider,
    text_field, text_font,
};
use crate::panel::form::snap_number;
use crate::panel::layout::{
    BAR_H, BTN_GAP, CHIP_H, CONTROL_H, GAP, LIST_ROWS, MULTI_ROWS, PAD, ROW_H, ROW_HINT_H,
    SEPARATOR_H, footer_height, group_columns, group_rows, has_search, header_height, list_height,
};
use crate::panel::spec::{
    DEFAULT_COLOR, FILE_ICON_PREFIX, SPACER_DEFAULT, format_hex, number_bounds, parse_hex,
};
use crate::panel::{
    Button, ButtonAlign, Field, Form, Icon, Layout, Opt, PanelSpec, TextStyle, Tone, Widget,
};
use crate::ui::fonts;
use crate::ui::hud::HudKind;
use crate::ui::symbols;
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, UiBuilder,
    pos2, vec2,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What the person did this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    None,
    /// A button was pressed (or Enter, which presses the main one).
    Press(String),
    /// An option of a menu (a list that answers when picked) was chosen: the widget and the value.
    Pick(usize, String),
    /// Esc, or the window was closed.
    Cancel,
}

/// The badge glyph of an icon name of a panel.
fn glyph(icon: Icon) -> char {
    let kind = match icon {
        Icon::App => HudKind::App,
        Icon::Link => HudKind::Link,
        Icon::File => HudKind::File,
        Icon::Shortcut => HudKind::Shortcut,
        Icon::Command => HudKind::Command,
        Icon::Browser => HudKind::Browser,
        Icon::Flow => HudKind::Flow,
    };
    kind.glyph()
}

/// Draws the whole panel inside `card` (the window minus its shadow margin). `focus_first` asks for
/// the keyboard to go to the first box (or search box) once, and is cleared when it did.
pub fn draw(
    ui: &mut egui::Ui,
    card: Rect,
    form: &mut Form,
    accent: Color32,
    focus_first: &mut bool,
) -> Event {
    paint(ui, card, form, accent, focus_first, None, None, true)
}

/// Draws the panel for the panel designer: the same drawing as [`draw`], so what is designed is
/// what a flow shows, but it leaves Esc and Enter to the designer and writes into `rects` where
/// each widget was drawn (one entry per widget of the spec, [`Rect::NOTHING`] for one that is
/// hidden), so a click on a widget can select it. Whatever the person pressed is returned as an
/// [`Event`]; the designer only uses it to select the button.
pub fn preview(
    ui: &mut egui::Ui,
    card: Rect,
    form: &mut Form,
    accent: Color32,
    rects: &mut Vec<Rect>,
    scroll: &mut BodyScroll,
) -> Event {
    rects.clear();
    let mut no_focus = false;
    paint(
        ui,
        card,
        form,
        accent,
        &mut no_focus,
        Some(rects),
        Some(scroll),
        false,
    )
}

/// How far the widgets of a previewed panel are scrolled. The designer owns it (it outlives the
/// preview, which is made again at every edit) so it can scroll the panel with the wheel and bring
/// a widget into view when it is picked in the list of the structure.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyScroll {
    /// Points scrolled down, in the panel's own size (not zoomed).
    pub offset: f32,
    /// The most it can be scrolled, as of the last frame.
    pub max: f32,
}

/// Where the widgets of a panel are drawn inside `card`: between the header and the buttons.
pub fn body_rect(card: Rect, spec: &PanelSpec) -> Rect {
    let inner = card.shrink(PAD);
    let head_bottom = inner.min.y + header_height(spec);
    let foot_top = inner.max.y - footer_height();
    let top = head_bottom
        + if spec.widgets.is_empty() {
            0.0
        } else {
            GAP + 4.0
        };
    Rect::from_min_max(
        pos2(inner.min.x, top),
        pos2(inner.max.x, (foot_top - GAP).max(top)),
    )
}

#[allow(clippy::too_many_arguments)]
fn paint(
    ui: &mut egui::Ui,
    card: Rect,
    form: &mut Form,
    accent: Color32,
    focus_first: &mut bool,
    rects: Option<&mut Vec<Rect>>,
    scroll: Option<&mut BodyScroll>,
    keys: bool,
) -> Event {
    let ctx = ui.ctx().clone();
    {
        let v = ui.visuals_mut();
        v.extreme_bg_color = pal().input;
        v.override_text_color = Some(pal().text);
        v.selection.bg_fill = wash(accent, 0.28);
        v.selection.stroke = Stroke::new(1.5, accent);
    }
    let painter = ui.painter().clone();
    painter.add(window_shadow().as_shape(card, CARD_RADIUS));
    painter.rect_filled(card, CARD_RADIUS, pal().card);
    window_outline(&painter, card, accent);

    let mut event = Event::None;
    let inner = card.shrink(PAD);
    let head = Rect::from_min_size(inner.min, vec2(inner.width(), header_height(&form.spec)));
    header(ui, head, &form.spec, accent);

    let foot = Rect::from_min_max(pos2(inner.min.x, inner.max.y - footer_height()), inner.max);
    let body = body_rect(card, &form.spec);

    let mut multiline_focus = false;
    if !form.spec.widgets.is_empty() {
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            let mut area = egui::ScrollArea::vertical()
                .id_salt("panel_body")
                .auto_shrink([false, false]);
            // the designer says where the body is scrolled to (the wheel, a pick in the list)
            if let Some(s) = scroll.as_deref() {
                area = area.vertical_scroll_offset(s.offset);
            }
            let out = area.show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = GAP;
                body_widgets(
                    ui,
                    form,
                    accent,
                    focus_first,
                    &mut event,
                    &mut multiline_focus,
                    rects,
                );
            });
            // ... and reads back where it is now (the scroll bar can have moved it)
            if let Some(s) = scroll {
                s.offset = out.state.offset.y;
                s.max = (out.content_size.y - out.inner_rect.height()).max(0.0);
            }
        });
    }

    footer(ui, foot, form, accent, &mut event);

    // keys: Esc cancels; Enter presses the main button (in a box with several lines only with
    // Ctrl/Cmd), or, in a menu, picks the first option that matches the search
    if keys
        && event == Event::None
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        event = Event::Cancel;
    }
    if keys && event == Event::None {
        let (enter, command) =
            ctx.input(|i| (i.key_pressed(egui::Key::Enter), i.modifiers.command));
        if enter && (!multiline_focus || command) {
            if let Some(b) = form.default_button() {
                event = Event::Press(b.id.clone());
            } else if let Some((i, value)) = first_match_of_menu(form) {
                event = Event::Pick(i, value);
            }
        }
    }
    event
}

/// The first option that matches the search of the first menu: what Enter picks.
fn first_match_of_menu(form: &Form) -> Option<(usize, String)> {
    for (i, w) in form.spec.widgets.iter().enumerate() {
        if let Widget::Choice {
            submit: true,
            options,
            ..
        } = w
            && let Some(&first) = form.matching(i).first()
        {
            return Some((i, options[first].value.clone()));
        }
    }
    None
}

// ------------------------------------------------------------------ header and footer

fn header(ui: &mut egui::Ui, rect: Rect, spec: &PanelSpec, accent: Color32) {
    let p = ui.painter();
    let badge_c = pos2(rect.min.x + 18.0, rect.min.y + 20.0);
    p.circle_filled(badge_c, 18.0, wash(accent, 0.16));
    fonts::paint_icon(p, badge_c, glyph(spec.icon), 18.0, accent, true);
    let text_x = rect.min.x + 46.0;
    let max_w = (rect.max.x - text_x).max(40.0);
    let title = if spec.title.is_empty() {
        "CursorPie"
    } else {
        spec.title.as_str()
    };
    let (g, _) = fit_galley(ui, title, text_font(16.0, true), pal().text, max_w);
    let title_y = if spec.subtitle.is_empty() {
        rect.min.y + 20.0 - g.size().y / 2.0
    } else {
        rect.min.y + 2.0
    };
    ui.painter()
        .galley(pos2(text_x, title_y).round(), g, pal().text);
    if !spec.subtitle.is_empty() {
        let (g, _) = fit_galley(
            ui,
            &spec.subtitle,
            text_font(13.0, false),
            pal().muted,
            max_w,
        );
        ui.painter()
            .galley(pos2(text_x, rect.min.y + 24.0).round(), g, pal().muted);
    }
}

fn footer(ui: &mut egui::Ui, rect: Rect, form: &Form, accent: Color32, event: &mut Event) {
    let row = Rect::from_min_size(
        rect.min,
        vec2(rect.width(), crate::ui::settings::theme::CONTROL_H),
    );
    let buttons = &form.spec.buttons;
    let pics: Vec<Pic> = buttons
        .iter()
        .map(|b| button_pic(ui.ctx(), &b.icon))
        .collect();
    let count = buttons.len();
    let gaps = BTN_GAP * count.saturating_sub(1) as f32;
    let mut widths: Vec<f32> = match form.spec.buttons_align {
        // every button as wide as the others, together as wide as the row
        ButtonAlign::Stretch if count > 0 => {
            vec![((row.width() - gaps) / count as f32).max(24.0); count]
        }
        // each one as wide as it needs
        _ => buttons
            .iter()
            .zip(&pics)
            .map(|(b, pic)| natural_width(ui, pic, &b.label, b.tone == Tone::Primary))
            .collect(),
    };
    let total = widths.iter().sum::<f32>() + gaps;
    if total > row.width() && total > 0.0 {
        // too many words for the row: all of them shrink alike
        let scale = ((row.width() - gaps) / (total - gaps)).clamp(0.2, 1.0);
        for w in &mut widths {
            *w *= scale;
        }
    }
    let total = widths.iter().sum::<f32>() + gaps;
    let mut x = match form.spec.buttons_align {
        ButtonAlign::End => row.max.x - total,
        ButtonAlign::Center => row.center().x - total / 2.0,
        ButtonAlign::Start | ButtonAlign::Stretch => row.min.x,
    }
    .max(row.min.x);
    for ((b, pic), w) in buttons.iter().zip(&pics).zip(&widths) {
        let r = Rect::from_min_size(pos2(x.round(), row.min.y), vec2(w.round(), row.height()));
        x += w + BTN_GAP;
        let id = ui.id().with(("footer_button", &b.id));
        let resp = action_button(ui, r, id, look_of(b.tone, accent), pic, &b.label, &b.id);
        if resp.clicked() && *event == Event::None {
            *event = Event::Press(b.id.clone());
        }
    }
    if let Some(why) = &form.notice {
        let at = pos2(rect.min.x, row.max.y + 3.0);
        ui.painter()
            .text(at, Align2::LEFT_TOP, why, text_font(12.5, false), pal().bad);
    }
}

fn look_of(tone: Tone, accent: Color32) -> Look {
    match tone {
        Tone::Primary => Look::Primary(accent),
        Tone::Danger => Look::Danger,
        Tone::Secondary => Look::Soft,
    }
}

// ------------------------------------------------------------------ buttons with pictures

/// Size of the picture in front of the label of a button, and the space after it.
const PIC: f32 = 18.0;
const PIC_GAP: f32 = 7.0;
/// How long a picture file that could not be read is left alone before it is tried again.
const PIC_RETRY: Duration = Duration::from_secs(5);
/// Biggest picture file that is read (bytes), and the most pictures kept.
const PIC_FILE_MAX: u64 = 4 * 1024 * 1024;
const PIC_KEEP: usize = 64;

/// The picture of a button.
enum Pic {
    None,
    /// A glyph of the icon font.
    Glyph(char),
    /// A picture file, drawn in its own colors.
    File(egui::TextureHandle),
}

thread_local! {
    /// The picture files read so far (and when a failed one was tried), by path. Only the window
    /// thread draws, so no lock is needed.
    static FILE_PICS: RefCell<HashMap<String, (Instant, Option<egui::TextureHandle>)>> =
        RefCell::new(HashMap::new());
}

/// Forgets the picture files read so far (their textures go with them). Called when Settings
/// closes and when the UI sleeps, on the window thread, because the cache is thread local.
pub(crate) fn release_pics() {
    FILE_PICS.with(|cache| *cache.borrow_mut() = HashMap::new());
}

/// The picture that the `icon` text of a button names: an icon of the app (`play`, `ico:play`) or
/// a picture file (`file:/path.png`). Nothing when it names neither.
fn button_pic(ctx: &egui::Context, icon: &str) -> Pic {
    let icon = icon.trim();
    if icon.is_empty() {
        return Pic::None;
    }
    if let Some(path) = icon.strip_prefix(FILE_ICON_PREFIX) {
        return file_pic(ctx, path.trim()).map_or(Pic::None, Pic::File);
    }
    symbols::resolve(icon)
        .or_else(|| symbols::by_name(icon))
        .map_or(Pic::None, Pic::Glyph)
}

fn file_pic(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    FILE_PICS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some((tried, tex)) = cache.get(path)
            && (tex.is_some() || tried.elapsed() < PIC_RETRY)
        {
            return tex.clone();
        }
        if cache.len() >= PIC_KEEP {
            cache.clear();
        }
        let tex = load_pic(ctx, path);
        cache.insert(path.to_string(), (Instant::now(), tex.clone()));
        tex
    })
}

fn load_pic(ctx: &egui::Context, path: &str) -> Option<egui::TextureHandle> {
    let file = match path.strip_prefix("~/") {
        Some(rest) => {
            let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
            std::path::Path::new(&home).join(rest)
        }
        None => std::path::PathBuf::from(path),
    };
    // read on the window thread: a huge file would freeze the panel
    if std::fs::metadata(&file).ok()?.len() > PIC_FILE_MAX {
        return None;
    }
    let img = ::image::open(&file).ok()?.thumbnail(64, 64).to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(
        format!("panel-icon:{}", file.display()),
        color,
        egui::TextureOptions::LINEAR,
    ))
}

/// How wide a button is when it takes what it needs: its picture, its label and some air.
fn natural_width(ui: &egui::Ui, pic: &Pic, label: &str, bold: bool) -> f32 {
    let text_w = if label.is_empty() {
        0.0
    } else {
        fit_galley(
            ui,
            label,
            text_font(14.0, bold),
            Color32::PLACEHOLDER,
            10_000.0,
        )
        .0
        .size()
        .x
    };
    let pic_w = if matches!(pic, Pic::None) { 0.0 } else { PIC };
    let gap = if text_w > 0.0 && pic_w > 0.0 {
        PIC_GAP
    } else {
        0.0
    };
    let pad = if label.is_empty() { 0.0 } else { 32.0 };
    (text_w + pic_w + gap + pad).ceil().max(CONTROL_H)
}

/// A button of the panel in `rect`, in the look of Settings, with its picture and its label (either
/// may be missing). `tip` is shown when the pointer rests on it, for a button that has no label.
fn action_button(
    ui: &mut egui::Ui,
    rect: Rect,
    id: egui::Id,
    look: Look,
    pic: &Pic,
    label: &str,
    tip: &str,
) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    let fg = button_frame(ui, rect, &resp, look);
    let bold = matches!(look, Look::Primary(_));
    let pic_w = if matches!(pic, Pic::None) { 0.0 } else { PIC };
    let room = (rect.width() - pic_w - 16.0 - if pic_w > 0.0 { PIC_GAP } else { 0.0 }).max(8.0);
    let text = (!label.is_empty())
        .then(|| fit_galley(ui, label, text_font(14.0, bold), Color32::PLACEHOLDER, room).0);
    let text_w = text.as_ref().map_or(0.0, |g| g.size().x);
    let gap = if text.is_some() && pic_w > 0.0 {
        PIC_GAP
    } else {
        0.0
    };
    let mut x = rect.center().x - (pic_w + gap + text_w) / 2.0;
    let painter = ui.painter();
    match pic {
        Pic::None => {}
        Pic::Glyph(c) => {
            let at = pos2(x + PIC / 2.0, rect.center().y);
            fonts::paint_icon(painter, at, *c, PIC, fg, false);
            x += PIC + gap;
        }
        Pic::File(tex) => {
            let r = Rect::from_center_size(pos2(x + PIC / 2.0, rect.center().y), vec2(PIC, PIC));
            let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(tex.id(), r, whole, Color32::WHITE);
            x += PIC + gap;
        }
    }
    if let Some(g) = text {
        painter.galley(pos2(x, rect.center().y - g.size().y / 2.0).round(), g, fg);
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if label.is_empty() && !tip.is_empty() {
        resp.on_hover_text(tip)
    } else {
        resp
    }
}

/// A grid of buttons ([`Widget::Buttons`]): every button as wide as the others, in rows.
fn button_grid(
    ui: &mut egui::Ui,
    c: &Ctx<'_>,
    label: &str,
    columns: u8,
    buttons: &[Button],
    event: &mut Event,
) {
    field_label(ui, label);
    let cols = group_columns(columns, buttons.len());
    let rows = group_rows(columns, buttons.len());
    let width = ui.available_width();
    let height = rows as f32 * CONTROL_H + rows.saturating_sub(1) as f32 * BTN_GAP;
    let (area, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    if buttons.is_empty() {
        ui.painter().text(
            area.left_center(),
            Align2::LEFT_CENTER,
            "No buttons yet",
            text_font(13.0, false),
            pal().muted,
        );
        return;
    }
    let cell_w = ((width - BTN_GAP * cols.saturating_sub(1) as f32) / cols as f32).max(24.0);
    for (i, b) in buttons.iter().enumerate() {
        let (row, col) = (i / cols, i % cols);
        let at = pos2(
            area.min.x + col as f32 * (cell_w + BTN_GAP),
            area.min.y + row as f32 * (CONTROL_H + BTN_GAP),
        );
        let r = Rect::from_min_size(at, vec2(cell_w, CONTROL_H)).intersect(area);
        let pic = button_pic(ui.ctx(), &b.icon);
        let id = ui.id().with(("grid_button", c.index, &b.id));
        let resp = action_button(ui, r, id, look_of(b.tone, c.accent), &pic, &b.label, &b.id);
        if resp.clicked() && *event == Event::None {
            *event = Event::Press(b.id.clone());
        }
    }
}

// ------------------------------------------------------------------ the widgets

fn body_widgets(
    ui: &mut egui::Ui,
    form: &mut Form,
    accent: Color32,
    focus_first: &mut bool,
    event: &mut Event,
    multiline_focus: &mut bool,
    mut rects: Option<&mut Vec<Rect>>,
) {
    for i in 0..form.spec.widgets.len() {
        // a widget that waits for another one (`show_if`) takes no room until it is shown
        if !form.is_visible(i) {
            if let Some(r) = rects.as_deref_mut() {
                r.push(Rect::NOTHING);
            }
            continue;
        }
        let visible = form.matching(i);
        let w = form.spec.widgets[i].clone();
        let Form {
            fields, filters, ..
        } = &mut *form;
        let ctx = Ctx {
            index: i,
            accent,
            visible: &visible,
        };
        // each widget is a group of its own, so the label sits close to its control
        let group = ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            widget(
                ui,
                &ctx,
                &w,
                &mut fields[i],
                &mut filters[i],
                focus_first,
                event,
                multiline_focus,
            );
        });
        if let Some(r) = rects.as_deref_mut() {
            r.push(group.response.rect);
        }
    }
}

/// What a widget needs to know about its place.
struct Ctx<'a> {
    index: usize,
    accent: Color32,
    /// For a list: the options that match its search box.
    visible: &'a [usize],
}

#[allow(clippy::too_many_arguments)]
fn widget(
    ui: &mut egui::Ui,
    c: &Ctx<'_>,
    w: &Widget,
    field: &mut Field,
    filter: &mut String,
    focus_first: &mut bool,
    event: &mut Event,
    multiline_focus: &mut bool,
) {
    match (w, field) {
        (Widget::Separator { .. }, _) => separator(ui),
        (Widget::Text { text, style, .. }, _) => text_block(ui, text, *style),
        (
            Widget::Input {
                label,
                hint,
                secret,
                multiline,
                max_chars,
                ..
            },
            Field::Text(value),
        ) => input(
            ui,
            label,
            hint,
            *secret,
            *multiline,
            *max_chars,
            value,
            focus_first,
            multiline_focus,
        ),
        (Widget::Toggle { label, .. }, Field::Bool(on)) => toggle(ui, label, on, c.accent),
        (
            Widget::Number {
                label,
                min,
                max,
                step,
                unit,
                ..
            },
            Field::Number(n),
        ) => number_widget(ui, label, n, *min, *max, *step, unit),
        (Widget::Color { label, .. }, Field::Text(value)) => color_widget(ui, label, value),
        (
            Widget::Slider {
                label,
                min,
                max,
                step,
                unit,
                ..
            },
            Field::Number(n),
        ) => slider_widget(ui, label, n, *min, *max, *step, unit, c.accent),
        (
            Widget::Choice {
                label,
                options,
                layout,
                search,
                submit,
                ..
            },
            Field::One(current),
        ) => choice(
            ui,
            c,
            label,
            options,
            *layout,
            has_search(*search, options.len()) && *layout != Layout::Dropdown,
            *submit,
            current,
            filter,
            focus_first,
            event,
        ),
        (Widget::Multi { label, options, .. }, Field::Many(current)) => {
            multi(ui, c, label, options, current, filter)
        }
        (Widget::Progress { label, value, .. }, _) => progress(ui, label, *value, c.accent),
        (
            Widget::Buttons {
                label,
                columns,
                buttons,
                ..
            },
            _,
        ) => button_grid(ui, c, label, *columns, buttons, event),
        (Widget::Spacer { height, .. }, _) => {
            let h = height.unwrap_or(SPACER_DEFAULT);
            ui.allocate_exact_size(vec2(1.0, h), Sense::hover());
        }
        _ => {}
    }
}

fn field_label(ui: &mut egui::Ui, label: &str) {
    if !label.is_empty() {
        ui.label(
            RichText::new(label)
                .font(text_font(13.0, true))
                .color(pal().muted),
        );
    }
}

fn separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), SEPARATOR_H), Sense::hover());
    ui.painter().line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(1.0, pal().divider),
    );
}

fn text_block(ui: &mut egui::Ui, text: &str, style: TextStyle) {
    let rich = match style {
        TextStyle::Body => RichText::new(text)
            .font(text_font(14.0, false))
            .color(pal().text),
        TextStyle::Heading => RichText::new(text)
            .font(text_font(17.0, true))
            .color(pal().text),
        TextStyle::Muted => RichText::new(text)
            .font(text_font(13.0, false))
            .color(pal().muted),
        TextStyle::Code => RichText::new(text).monospace().size(13.0).color(pal().text),
    };
    ui.label(rich);
}

#[allow(clippy::too_many_arguments)]
fn input(
    ui: &mut egui::Ui,
    label: &str,
    hint: &str,
    secret: bool,
    multiline: bool,
    max_chars: Option<u32>,
    value: &mut String,
    focus_first: &mut bool,
    multiline_focus: &mut bool,
) {
    field_label(ui, label);
    let resp = if multiline {
        let mut edit = egui::TextEdit::multiline(value)
            .desired_rows(4)
            .desired_width(f32::INFINITY)
            .margin(egui::Margin::symmetric(12, 8))
            .hint_text(hint);
        if let Some(n) = max_chars {
            edit = edit.char_limit(n as usize);
        }
        ui.add(edit)
    } else {
        let mut edit = text_field(value)
            .desired_width(f32::INFINITY)
            .hint_text(hint)
            .password(secret);
        if let Some(n) = max_chars {
            edit = edit.char_limit(n as usize);
        }
        ui.add(edit)
    };
    if multiline && resp.has_focus() {
        *multiline_focus = true;
    }
    if *focus_first {
        resp.request_focus();
        *focus_first = false;
    }
}

fn toggle(ui: &mut egui::Ui, label: &str, on: &mut bool, accent: Color32) {
    let (rect, resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), crate::ui::settings::theme::CONTROL_H),
        Sense::click(),
    );
    if resp.clicked() {
        *on = !*on;
    }
    let p = ui.painter();
    let (g, _) = fit_galley(
        ui,
        label,
        text_font(14.0, false),
        pal().text,
        rect.width() - 60.0,
    );
    p.galley(
        pos2(rect.min.x, rect.center().y - g.size().y / 2.0).round(),
        g,
        pal().text,
    );
    let sw = Rect::from_center_size(pos2(rect.max.x - 22.0, rect.center().y), vec2(40.0, 22.0));
    let fill = if *on { accent } else { pal().track };
    p.rect_filled(sw, CornerRadius::same(11), fill);
    let knob_x = if *on {
        sw.max.x - 11.0
    } else {
        sw.min.x + 11.0
    };
    p.circle_filled(pos2(knob_x, sw.center().y), 8.5, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

#[allow(clippy::too_many_arguments)]
fn slider_widget(
    ui: &mut egui::Ui,
    label: &str,
    n: &mut f64,
    min: f64,
    max: f64,
    step: Option<f64>,
    unit: &str,
    accent: Color32,
) {
    ui.horizontal(|ui| {
        if !label.is_empty() {
            ui.label(
                RichText::new(label)
                    .font(text_font(13.0, true))
                    .color(pal().muted),
            );
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            let shown = crate::panel::form::snap(*n, min, max, step);
            ui.label(
                RichText::new(format!("{}{unit}", crate::panel::form::number_text(shown)))
                    .font(text_font(13.0, true))
                    .color(pal().text),
            );
        });
    });
    if slider(ui, n, min..=max, accent).changed() {
        *n = crate::panel::form::snap(*n, min, max, step);
    }
    ui.add_space(8.0);
}

/// A number: `-` and `+` buttons around a box that can be typed in or dragged.
fn number_widget(
    ui: &mut egui::Ui,
    label: &str,
    n: &mut f64,
    min: Option<f64>,
    max: Option<f64>,
    step: Option<f64>,
    unit: &str,
) {
    field_label(ui, label);
    let (lo, hi) = number_bounds(min, max);
    let grid = step.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if icon_button_look(ui, Look::Soft, symbols::MINUS, "Less", *n > lo).clicked() {
            *n = snap_number(*n - grid, min, max, step);
        }
        let box_w = (ui.available_width() - CONTROL_H - 8.0).max(60.0);
        let mut drag = egui::DragValue::new(n)
            .range(lo..=hi)
            .speed(grid * 0.1)
            .custom_formatter(|v, _| crate::panel::form::number_text(v))
            .custom_parser(|text| text.trim().parse::<f64>().ok());
        if !unit.is_empty() {
            drag = drag.suffix(format!(" {unit}"));
        }
        if ui.add_sized([box_w, CONTROL_H], drag).changed() {
            *n = snap_number(*n, min, max, step);
        }
        if icon_button_look(ui, Look::Soft, symbols::PLUS, "More", *n < hi).clicked() {
            *n = snap_number(*n + grid, min, max, step);
        }
    });
}

/// A color: the accent swatches of Settings, and the color picker of the system for any other.
fn color_widget(ui: &mut egui::Ui, label: &str, value: &mut String) {
    field_label(ui, label);
    let mut rgb = parse_hex(value)
        .or_else(|| parse_hex(DEFAULT_COLOR))
        .unwrap_or([0x3b, 0x82, 0xf6]);
    let before = rgb;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), CONTROL_H),
        egui::Layout::right_to_left(Align::Center),
        |ui| accent_picker(ui, &mut rgb),
    );
    if rgb != before {
        *value = format_hex(rgb);
    }
}

fn progress(ui: &mut egui::Ui, label: &str, value: f64, accent: Color32) {
    field_label(ui, label);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), BAR_H), Sense::hover());
    let p = ui.painter();
    let r = CornerRadius::same(4);
    p.rect_filled(rect, r, pal().track);
    if value >= 0.0 {
        let w = rect.width() * value.clamp(0.0, 1.0) as f32;
        if w > 0.5 {
            p.rect_filled(Rect::from_min_size(rect.min, vec2(w, BAR_H)), r, accent);
        }
    } else {
        // busy: a short bar that runs back and forth
        let t = ui.input(|i| i.time) as f32;
        let phase = (t * 0.9).fract();
        let along = if phase < 0.5 {
            phase * 2.0
        } else {
            2.0 - phase * 2.0
        };
        let seg = rect.width() * 0.3;
        let x = rect.min.x + (rect.width() - seg) * along;
        p.rect_filled(
            Rect::from_min_size(pos2(x, rect.min.y), vec2(seg, BAR_H)),
            r,
            accent,
        );
        ui.ctx().request_repaint();
    }
}

fn search_box(ui: &mut egui::Ui, filter: &mut String, focus_first: &mut bool) {
    let edit = text_field(filter)
        .desired_width(f32::INFINITY)
        .hint_text("Search");
    let resp = ui.add(edit);
    if *focus_first {
        resp.request_focus();
        *focus_first = false;
    }
    ui.add_space(2.0);
}

/// One row of a list: the label (and the hint under it), and a mark when it is on.
fn option_row(
    ui: &mut egui::Ui,
    o: &Opt,
    on: bool,
    check: bool,
    accent: Color32,
) -> egui::Response {
    let h = if o.hint.is_empty() { ROW_H } else { ROW_HINT_H };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, CONTROL_R, wash(accent, 0.14));
    } else if resp.hovered() {
        p.rect_filled(rect, CONTROL_R, pal().idle);
    }
    let mark_w = 30.0;
    let max_w = (rect.width() - 14.0 - mark_w).max(20.0);
    let color = if on {
        crate::ui::settings::theme::accent_text(accent)
    } else {
        pal().text
    };
    let (g, _) = fit_galley(ui, &o.label, text_font(14.0, on), color, max_w);
    let y = if o.hint.is_empty() {
        rect.center().y - g.size().y / 2.0
    } else {
        rect.min.y + 7.0
    };
    ui.painter()
        .galley(pos2(rect.min.x + 12.0, y).round(), g, color);
    if !o.hint.is_empty() {
        let (g, _) = fit_galley(ui, &o.hint, text_font(12.5, false), pal().muted, max_w);
        ui.painter().galley(
            pos2(rect.min.x + 12.0, rect.min.y + 27.0).round(),
            g,
            pal().muted,
        );
    }
    let mark_c = pos2(rect.max.x - 16.0, rect.center().y);
    if check {
        let b = Rect::from_center_size(mark_c, vec2(18.0, 18.0));
        if on {
            ui.painter().rect_filled(b, CornerRadius::same(5), accent);
            fonts::paint_icon(
                ui.painter(),
                mark_c,
                symbols::CHECK,
                12.0,
                crate::ui::draw::on_accent(accent),
                true,
            );
        } else {
            ui.painter().rect_stroke(
                b,
                CornerRadius::same(5),
                Stroke::new(1.5, pal().border),
                egui::StrokeKind::Inside,
            );
        }
    } else if on {
        fonts::paint_icon(ui.painter(), mark_c, symbols::CHECK, 14.0, accent, true);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

#[allow(clippy::too_many_arguments)]
fn choice(
    ui: &mut egui::Ui,
    c: &Ctx<'_>,
    label: &str,
    options: &[Opt],
    layout: Layout,
    searchable: bool,
    submit: bool,
    current: &mut String,
    filter: &mut String,
    focus_first: &mut bool,
    event: &mut Event,
) {
    field_label(ui, label);
    if searchable {
        search_box(ui, filter, focus_first);
    }
    let mut picked: Option<String> = None;
    match layout {
        Layout::Dropdown => {
            let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
            let selected = options
                .iter()
                .position(|o| o.value == *current)
                .unwrap_or(usize::MAX);
            let shown = labels.get(selected).copied().unwrap_or("Choose\u{2026}");
            // the list opens inside the little window: never taller than half of it
            let room = (ui.ctx().content_rect().height() * 0.5).clamp(120.0, 340.0);
            let width = ui.available_width();
            if let Some(i) = select_capped(ui, width, shown, &labels, selected, room) {
                picked = Some(options[i].value.clone());
            }
        }
        Layout::Chips => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for &i in c.visible {
                    let o = &options[i];
                    let on = *current == o.value;
                    if chip(ui, &o.label, on, c.accent).clicked() {
                        picked = Some(o.value.clone());
                    }
                }
            });
        }
        Layout::List => {
            let shown: Vec<Opt> = c.visible.iter().map(|&i| options[i].clone()).collect();
            let height = list_height(options, LIST_ROWS).max(ROW_H);
            egui::ScrollArea::vertical()
                .id_salt(("panel_list", c.index))
                .max_height(height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for o in &shown {
                        let on = *current == o.value;
                        if option_row(ui, o, on, false, c.accent).clicked() {
                            picked = Some(o.value.clone());
                        }
                    }
                    if shown.is_empty() {
                        ui.label(
                            RichText::new("Nothing matches")
                                .font(text_font(13.0, false))
                                .color(pal().muted),
                        );
                    }
                });
        }
    }
    if let Some(value) = picked {
        if submit {
            if *event == Event::None {
                *event = Event::Pick(c.index, value);
            }
        } else {
            *current = value;
        }
    }
}

fn chip(ui: &mut egui::Ui, text: &str, on: bool, accent: Color32) -> egui::Response {
    let color = if on {
        crate::ui::draw::on_accent(accent)
    } else {
        pal().text
    };
    let g = ui
        .painter()
        .layout_no_wrap(text.to_string(), text_font(13.5, on), color);
    let size = vec2(g.size().x + 28.0, CHIP_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if on {
        accent
    } else if resp.hovered() {
        pal().hot
    } else {
        pal().idle
    };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((CHIP_H / 2.0) as u8), fill);
    p.galley(
        pos2(
            rect.center().x - g.size().x / 2.0,
            rect.center().y - g.size().y / 2.0,
        )
        .round(),
        g,
        color,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn multi(
    ui: &mut egui::Ui,
    c: &Ctx<'_>,
    label: &str,
    options: &[Opt],
    current: &mut Vec<String>,
    filter: &mut String,
) {
    field_label(ui, label);
    if options.len() > crate::panel::layout::SEARCH_FROM {
        let mut none = false;
        search_box(ui, filter, &mut none);
    }
    let shown: Vec<Opt> = c.visible.iter().map(|&i| options[i].clone()).collect();
    let height = list_height(options, MULTI_ROWS).max(ROW_H);
    egui::ScrollArea::vertical()
        .id_salt(("panel_multi", c.index))
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for o in &shown {
                let on = current.contains(&o.value);
                if option_row(ui, o, on, true, c.accent).clicked() {
                    if on {
                        current.retain(|v| *v != o.value);
                    } else {
                        current.push(o.value.clone());
                    }
                }
            }
        });
}

/// The font used by a panel's code text, exposed so a themed drawing can match it.
pub fn code_font() -> FontId {
    FontId::monospace(13.0)
}
