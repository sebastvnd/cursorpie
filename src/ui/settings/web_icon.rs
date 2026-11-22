//! The sheet **Icon from a website**: type an address, watch the icon being fetched, then tidy it
//! so it matches the other icons of the pie, and use it for the selected slice.
//!
//! The fetching runs on a worker thread (`ui::webicon::fetch`). While it runs the sheet shows a
//! loading animation with what is being done ("Reading github.com", "Downloading the icon (2 of
//! 5)"), and a Cancel button. When the picture arrives, the sheet shows it twice, as the site
//! serves it and as it will look on the pie, and a few controls (remove the plain background, trim
//! the border, one colour, a plate, the margin) redraw the second one at once. The look is
//! remembered (`webicon::save_style`), so the next site starts with the same one.

use super::widgets::*;
use crate::ui::fonts;
use crate::ui::settings::theme::*;
use crate::ui::symbols;
use crate::ui::webicon::{self, Fetched, Plate, Stage, Style, Tint};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Stroke, Vec2};
use image::RgbaImage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Where the sheet is in its work.
enum Phase {
    /// Nothing fetched yet.
    Idle,
    /// The worker is busy (since when).
    Loading(Instant),
    /// A picture is here and can be tidied.
    Ready,
    /// Something went wrong (the sentence for the user).
    Failed(String),
}

/// What the sheet wants done after a frame.
pub(super) enum Step {
    Stay,
    Close,
    /// Use this icon string for the slice.
    Use {
        file: String,
        note: Option<String>,
    },
}

/// State of the sheet. Lives in [`Ed`](super::state::Ed).
pub struct WebIcon {
    /// Whether the sheet is showing.
    pub open: bool,
    url: String,
    phase: Phase,
    style: Style,
    raw: Option<RgbaImage>,
    source: String,
    host: String,
    orig_tex: Option<egui::TextureHandle>,
    /// The tidied picture, the style it was made with, and its texture.
    shown: Option<(Style, egui::TextureHandle, RgbaImage)>,
    stage: Stage,
    cancel: Arc<AtomicBool>,
    inbox: Arc<Mutex<Option<Result<Fetched, String>>>>,
    focus: bool,
}

fn color_image(img: &RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    )
}

impl Default for WebIcon {
    fn default() -> Self {
        Self::new()
    }
}

impl WebIcon {
    /// A closed sheet.
    pub fn new() -> Self {
        Self {
            open: false,
            url: String::new(),
            phase: Phase::Idle,
            style: Style::default(),
            raw: None,
            source: String::new(),
            host: String::new(),
            orig_tex: None,
            shown: None,
            stage: Arc::new(Mutex::new(String::new())),
            cancel: Arc::new(AtomicBool::new(false)),
            inbox: Arc::new(Mutex::new(None)),
            focus: false,
        }
    }

    /// Opens the sheet. With a usable address (the link of the slice) the fetching starts at once.
    pub(super) fn open_for(&mut self, address: &str, ctx: &egui::Context) {
        self.cancel_job();
        self.open = true;
        self.style = webicon::load_style();
        self.raw = None;
        self.shown = None;
        self.orig_tex = None;
        self.phase = Phase::Idle;
        if webicon::parse_site(address).is_ok() {
            self.url = address.trim().to_string();
            self.start(ctx);
        } else {
            self.url.clear();
            self.focus = true;
        }
    }

    /// Closes the sheet and stops the fetching.
    pub fn close(&mut self) {
        self.cancel_job();
        self.open = false;
        self.raw = None;
        self.shown = None;
        self.orig_tex = None;
        self.phase = Phase::Idle;
    }

    /// Tells a running worker to give up and makes sure it can no longer deliver anything.
    fn cancel_job(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.inbox = Arc::new(Mutex::new(None));
    }

    fn start(&mut self, ctx: &egui::Context) {
        if let Err(why) = webicon::parse_site(&self.url) {
            self.phase = Phase::Failed(why);
            return;
        }
        self.cancel_job();
        self.phase = Phase::Loading(Instant::now());
        if let Ok(mut s) = self.stage.lock() {
            *s = "Starting".to_string();
        }
        let url = self.url.clone();
        let stage = self.stage.clone();
        let cancel = self.cancel.clone();
        let inbox = self.inbox.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = webicon::fetch(&url, &stage, &cancel);
            if !cancel.load(Ordering::Relaxed)
                && let Ok(mut slot) = inbox.lock()
            {
                *slot = Some(result);
            }
            ctx.request_repaint();
        });
    }

    /// The Cancel button: stops the search and goes back to what was there before.
    fn stop(&mut self) {
        self.cancel_job();
        self.phase = if self.raw.is_some() {
            Phase::Ready
        } else {
            Phase::Idle
        };
    }

    /// Takes the answer of the worker, if it came.
    fn poll(&mut self, ctx: &egui::Context) {
        if !matches!(self.phase, Phase::Loading(_)) {
            return;
        }
        let got = self.inbox.lock().ok().and_then(|mut slot| slot.take());
        match got {
            None => ctx.request_repaint(),
            Some(Ok(found)) => {
                self.style.knock_out = webicon::has_neutral_background(&found.image);
                self.orig_tex = Some(ctx.load_texture(
                    "web_icon_original",
                    color_image(&found.image),
                    egui::TextureOptions::LINEAR,
                ));
                self.source = found.source;
                self.host = found.host;
                self.raw = Some(found.image);
                self.shown = None;
                self.phase = Phase::Ready;
            }
            Some(Err(why)) => self.phase = Phase::Failed(why),
        }
    }

    /// Redraws the tidied picture when the style changed.
    fn refresh(&mut self, ctx: &egui::Context) {
        let Some(raw) = &self.raw else {
            return;
        };
        if self.shown.as_ref().is_some_and(|s| s.0 == self.style) {
            return;
        }
        let out = webicon::harmonize(raw, &self.style);
        let tex = ctx.load_texture(
            "web_icon_result",
            color_image(&out),
            egui::TextureOptions::LINEAR,
        );
        self.shown = Some((self.style, tex, out));
    }
}

/// Draws the sheet and says what to do next.
pub(super) fn web_icon_sheet(
    w: &mut WebIcon,
    ctx: &egui::Context,
    bounds: Rect,
    accent: Color32,
) -> Step {
    w.poll(ctx);
    w.refresh(ctx);
    let mut step = Step::Stay;
    let (floating, ()) = sheet(
        ctx,
        "web_icon",
        bounds,
        Vec2::new(560.0, 680.0),
        "Icon from a website",
        |ui| {
            step = body(ui, ctx, w, accent);
        },
    );
    if floating.close {
        step = Step::Close;
    }
    step
}

fn body(ui: &mut egui::Ui, ctx: &egui::Context, w: &mut WebIcon, accent: Color32) -> Step {
    let mut step = Step::Stay;
    sheet_text(
        ui,
        "Type a website address. CursorPie fetches the icon of the site, and you can tidy it so \
         it matches your other icons.",
    );
    ui.add_space(8.0);
    address_row(ui, ctx, w, accent);
    ui.add_space(8.0);

    let loading = matches!(w.phase, Phase::Loading(_));
    let failed = match &w.phase {
        Phase::Failed(m) => Some(m.clone()),
        _ => None,
    };
    if loading {
        loading_view(ui, ctx, w, accent);
    } else if let Some(message) = failed {
        sheet_heading(ui, symbols::WARNING, pal().bad, "No icon this time");
        sheet_text(ui, &message);
    } else if w.raw.is_some() {
        step = ready_view(ui, w, accent);
    } else {
        muted(ui, "Example: github.com");
    }
    step
}

/// The address field and the Fetch / Cancel button.
///
/// The button keeps its own width and sits at the right edge, the field takes what is left. It
/// used to reserve a fixed 112 points for the button, but "Fetch again" is wider than that, so the
/// row overflowed and the whole sheet grew wider the moment a picture arrived.
fn address_row(ui: &mut egui::Ui, ctx: &egui::Context, w: &mut WebIcon, accent: Color32) {
    let loading = matches!(w.phase, Phase::Loading(_));
    let again = w.raw.is_some();
    let width = ui.available_width();
    let mut fetch = false;
    ui.allocate_ui_with_layout(
        Vec2::new(width, CONTROL_H),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if loading {
                if button(ui, Look::Soft, Some(symbols::X), "Cancel").clicked() {
                    w.stop();
                }
            } else {
                let label = if again { "Fetch again" } else { "Fetch" };
                fetch = button(ui, Look::Primary(accent), Some(symbols::GLOBE), label).clicked();
            }
            let field_w = ui.available_width().max(120.0);
            let resp = ui.add_enabled(
                !loading,
                text_field(&mut w.url)
                    .desired_width(field_w)
                    .hint_text("example.com"),
            );
            if w.focus {
                resp.request_focus();
                w.focus = false;
            }
            fetch |= resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        },
    );
    if fetch && !loading {
        w.start(ctx);
    }
}

/// The loading animation: a tile that breathes, a comet circling it, and what is being done.
fn loading_view(ui: &mut egui::Ui, ctx: &egui::Context, w: &WebIcon, accent: Color32) {
    let t = ctx.input(|i| i.time) as f32;
    let secs = match w.phase {
        Phase::Loading(since) => since.elapsed().as_secs(),
        _ => 0,
    };
    // the animation takes all the room that is left in the sheet and sits in the middle of it
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), ui.available_height().max(190.0)),
        egui::Sense::hover(),
    );
    // the ring, the tile and the two lines of text are about 190 high together: centre that block
    let c = rect.center() - Vec2::new(0.0, 28.0);
    let tile = Rect::from_center_size(c, Vec2::splat(96.0));
    let pulse = 0.5 + 0.5 * (t * 3.0).sin();
    let painter = ui.painter();
    painter.rect_filled(tile, 22.0, pal().idle);
    painter.rect_stroke(
        tile,
        22.0,
        Stroke::new(1.5, accent.gamma_multiply(0.25 + 0.45 * pulse)),
        egui::StrokeKind::Inside,
    );
    painter.text(
        c,
        Align2::CENTER_CENTER,
        symbols::GLOBE.to_string(),
        FontId::new(34.0, fonts::icons()),
        pal().muted.gamma_multiply(0.6 + 0.4 * pulse),
    );
    // a comet: the head runs round the tile, the tail fades behind it
    let (radius, steps, head) = (66.0_f32, 28_u32, t * 4.0);
    let at = |a: f32| c + Vec2::new(a.cos(), a.sin()) * radius;
    for k in 0..steps {
        let a0 = head - k as f32 * 0.09;
        let fade = 1.0 - k as f32 / steps as f32;
        painter.line_segment(
            [at(a0), at(a0 - 0.09)],
            Stroke::new(3.0 * fade + 0.5, accent.gamma_multiply(fade)),
        );
    }
    let stage = w.stage.lock().map(|s| s.clone()).unwrap_or_default();
    let dots = ".".repeat((t * 2.5) as usize % 4);
    painter.text(
        Pos2::new(c.x, c.y + 94.0),
        Align2::CENTER_CENTER,
        format!("{stage}{dots:<3}"),
        FontId::proportional(14.0),
        pal().text,
    );
    let slow = if secs >= 8 {
        "This site is slow to answer. You can cancel."
    } else {
        "Looking for the best picture the site has."
    };
    painter.text(
        Pos2::new(c.x, c.y + 116.0),
        Align2::CENTER_CENTER,
        slow,
        FontId::proportional(12.5),
        pal().muted,
    );
    ctx.request_repaint();
}

/// Height of the preview card.
const PREVIEW_H: f32 = 156.0;
/// Side of the two big tiles of the preview card.
const PREVIEW_TILE: f32 = 104.0;
/// Side of the small tiles that show the icon about as large as it is on the pie.
const PREVIEW_CHIP: f32 = 40.0;
/// The icon is drawn this far from the edge of its tile.
const PREVIEW_INSET: f32 = 10.0;

/// The part of `outer` that a picture of `w` x `h` points fills when it is fitted in, centered.
fn fit(outer: Rect, w: f32, h: f32) -> Rect {
    if w <= 0.0 || h <= 0.0 {
        return outer;
    }
    let k = (outer.width() / w).min(outer.height() / h);
    Rect::from_center_size(outer.center(), Vec2::new(w * k, h * k))
}

/// Paints `tex` fitted into `rect`.
fn paint_tex(painter: &egui::Painter, rect: Rect, tex: &egui::TextureHandle) {
    let [w, h] = tex.size();
    let whole = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
    painter.image(
        tex.id(),
        fit(rect, w as f32, h as f32),
        whole,
        Color32::WHITE,
    );
}

/// Covers the four corners of `rect` outside a rounded corner of `radius` with `color`, so
/// something square painted inside `rect` seems to have rounded corners.
fn round_off(painter: &egui::Painter, rect: Rect, radius: f32, color: Color32) {
    use std::f32::consts::{FRAC_PI_2, PI};
    const STEPS: usize = 10;
    let r = radius;
    let corners = [
        (
            rect.left_top(),
            Pos2::new(rect.min.x + r, rect.min.y + r),
            PI,
        ),
        (
            rect.right_top(),
            Pos2::new(rect.max.x - r, rect.min.y + r),
            PI * 1.5,
        ),
        (
            rect.right_bottom(),
            Pos2::new(rect.max.x - r, rect.max.y - r),
            0.0,
        ),
        (
            rect.left_bottom(),
            Pos2::new(rect.min.x + r, rect.max.y - r),
            FRAC_PI_2,
        ),
    ];
    let mut mesh = egui::Mesh::default();
    for (corner, centre, from) in corners {
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(corner, color);
        for k in 0..=STEPS {
            let a = from + FRAC_PI_2 * k as f32 / STEPS as f32;
            mesh.colored_vertex(centre + Vec2::new(a.cos(), a.sin()) * r, color);
        }
        for k in 0..STEPS as u32 {
            mesh.add_triangle(base, base + 1 + k, base + 2 + k);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// A rounded square with a checkerboard (so that a transparent area can be told from a plain
/// one) and the picture `tex` on it. `around` is the color behind the square.
fn preview_tile(
    painter: &egui::Painter,
    rect: Rect,
    checks: (Color32, Color32),
    around: Color32,
    tex: Option<&egui::TextureHandle>,
) {
    const RADIUS: f32 = 14.0;
    const CELL: f32 = 8.0;
    painter.rect_filled(rect, 0.0, checks.0);
    let inside = painter.with_clip_rect(rect);
    let (cols, rows) = (
        (rect.width() / CELL).ceil() as i32,
        (rect.height() / CELL).ceil() as i32,
    );
    for j in 0..rows {
        for i in 0..cols {
            if (i + j) % 2 == 1 {
                let at = rect.min + Vec2::new(i as f32, j as f32) * CELL;
                inside.rect_filled(Rect::from_min_size(at, Vec2::splat(CELL)), 0.0, checks.1);
            }
        }
    }
    round_off(painter, rect, RADIUS, around);
    if let Some(t) = tex {
        paint_tex(painter, rect.shrink(PREVIEW_INSET), t);
    }
    painter.rect_stroke(
        rect,
        RADIUS,
        Stroke::new(1.0, pal().border),
        egui::StrokeKind::Inside,
    );
}

/// A line of text cut with an ellipsis at `width` points.
fn one_line(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, width.max(20.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    painter.layout_job(job)
}

/// The preview card: the icon as the site serves it, an arrow, the icon as it will be on the pie,
/// and next to them the name of the site, the size, and the icon at about its real size on a
/// light and a dark slice. Everything is painted inside one rectangle of a fixed size, so nothing
/// in it can push the sheet wider.
fn preview_card(ui: &mut egui::Ui, w: &WebIcon) {
    let (card, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), PREVIEW_H),
        egui::Sense::hover(),
    );
    let p = pal();
    let painter = ui.painter().clone();
    painter.rect_filled(card, 16.0, p.idle);

    const PAD: f32 = 16.0;
    const ARROW_ROOM: f32 = 44.0;
    let from = Rect::from_min_size(card.min + Vec2::splat(PAD), Vec2::splat(PREVIEW_TILE));
    let to = Rect::from_min_size(
        Pos2::new(from.max.x + ARROW_ROOM, from.min.y),
        Vec2::splat(PREVIEW_TILE),
    );
    let light = (
        Color32::from_rgb(250, 250, 252),
        Color32::from_rgb(228, 228, 233),
    );
    let dark = (Color32::from_rgb(30, 30, 34), Color32::from_rgb(42, 42, 48));
    preview_tile(&painter, from, light, p.idle, w.orig_tex.as_ref());
    let result = w.shown.as_ref().map(|s| &s.1);
    preview_tile(&painter, to, dark, p.idle, result);

    let arrow_at = Pos2::new((from.max.x + to.min.x) / 2.0, from.center().y);
    fonts::paint_icon(
        &painter,
        arrow_at,
        symbols::ARROW_RIGHT,
        18.0,
        p.muted,
        false,
    );
    for (tile, name) in [(from, "From the site"), (to, "On the pie")] {
        painter.text(
            Pos2::new(tile.center().x, tile.max.y + 12.0),
            Align2::CENTER_CENTER,
            name,
            FontId::proportional(12.5),
            p.muted,
        );
    }

    // the facts, at the right
    let x = to.max.x + 22.0;
    let room = card.max.x - PAD - x;
    let (rw, rh) = w.raw.as_ref().map_or((0, 0), |r| (r.width(), r.height()));
    let host = one_line(&painter, &w.host, text_font(15.0, true), p.text, room);
    let dims = one_line(
        &painter,
        &format!("{rw} \u{d7} {rh} px"),
        FontId::proportional(12.5),
        p.muted,
        room,
    );
    let mut y = from.min.y + 2.0;
    let host_h = host.size().y;
    painter.galley(Pos2::new(x, y), host, p.text);
    y += host_h + 2.0;
    painter.galley(Pos2::new(x, y), dims, p.muted);

    painter.text(
        Pos2::new(x, from.max.y - PREVIEW_CHIP - 8.0),
        Align2::LEFT_BOTTOM,
        "Actual size",
        FontId::proportional(12.5),
        p.muted,
    );
    let chips_y = from.max.y - PREVIEW_CHIP;
    for (k, checks) in [light, dark].into_iter().enumerate() {
        let chip = Rect::from_min_size(
            Pos2::new(x + k as f32 * (PREVIEW_CHIP + 10.0), chips_y),
            Vec2::splat(PREVIEW_CHIP),
        );
        painter.rect_filled(chip, 11.0, checks.0);
        if let Some(t) = result {
            paint_tex(&painter, chip.shrink(6.0), t);
        }
        painter.rect_stroke(
            chip,
            11.0,
            Stroke::new(1.0, p.border),
            egui::StrokeKind::Inside,
        );
    }
}

/// The preview, the controls and the buttons.
fn ready_view(ui: &mut egui::Ui, w: &mut WebIcon, accent: Color32) -> Step {
    let mut step = Step::Stay;
    let flat = w.style.plate == Plate::None
        && w.style.tint != Tint::Original
        && w.shown.as_ref().is_some_and(|s| webicon::is_opaque(&s.2));

    preview_card(ui, w);
    if w.raw
        .as_ref()
        .is_some_and(|r| r.width().max(r.height()) < 64)
    {
        ui.add_space(6.0);
        sheet_text(
            ui,
            "The site only offers a small icon, so it looks soft when enlarged.",
        );
    }
    ui.add_space(6.0);

    let mut st = w.style;
    let room = (ui.available_height() - 60.0).max(120.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(room)
        .show(ui, |ui| {
            toggle_row(
                ui,
                &mut st.knock_out,
                "Remove plain background",
                "Makes the plain square behind the logo (white or black) transparent, so only the logo is left.",
                accent,
            );
            toggle_row(
                ui,
                &mut st.trim,
                "Trim the empty border",
                "Cuts the margin the site left, so every logo fills the same space.",
                accent,
            );
            toggle_row(
                ui,
                &mut st.round_square,
                "Round a square icon",
                "A coloured tile with no transparent area gets rounded corners, and no plate.",
                accent,
            );
            row_wide(
                ui,
                "Colour",
                "One colour for all of them looks the most even.",
                250.0,
                |ui| {
                    let items: [(Option<char>, &str); 3] =
                        [(None, "Original"), (None, "White"), (None, "Black")];
                    let all = [Tint::Original, Tint::White, Tint::Black];
                    let at = all.iter().position(|t| *t == st.tint).unwrap_or(0);
                    if let Some(i) = segmented(ui, &items, at) {
                        st.tint = all[i];
                    }
                },
            );
            row_wide(
                ui,
                "Plate",
                "A square behind the logo, the same for every icon.",
                250.0,
                |ui| {
                    let items: [(Option<char>, &str); 3] =
                        [(None, "None"), (None, "Light"), (None, "Dark")];
                    let all = [Plate::None, Plate::Light, Plate::Dark];
                    let at = all.iter().position(|p| *p == st.plate).unwrap_or(0);
                    if let Some(i) = segmented(ui, &items, at) {
                        st.plate = all[i];
                    }
                },
            );
            slider_row(ui, "Margin", &mut st.padding, 0..=30, "%", accent);
            if st.plate != Plate::None || st.round_square {
                slider_row(ui, "Roundness", &mut st.radius, 0..=50, "%", accent);
            }
            if flat {
                ui.add_space(6.0);
                sheet_text(
                    ui,
                    "This picture has no transparent area, so one colour makes a flat square. \
                     Turn on Remove plain background.",
                );
            }
        });
    w.style = st;

    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if button(
            ui,
            Look::Primary(accent),
            Some(symbols::CHECK),
            "Use as icon",
        )
        .clicked()
            && let Some((_, _, img)) = &w.shown
        {
            match webicon::save_png(img, &crate::config::icons_dir(), &w.host) {
                Ok(path) => {
                    webicon::save_style(&w.style);
                    step = Step::Use {
                        file: crate::config::tilde(&path),
                        note: Some(format!("Icon of {} saved in your icons (Mine).", w.host)),
                    };
                }
                Err(why) => w.phase = Phase::Failed(why),
            }
        }
        if button(ui, Look::Soft, None, "Cancel").clicked() {
            step = Step::Close;
        }
        if button(ui, Look::Soft, Some(symbols::ARROW_CLOCKWISE), "Reset look")
            .on_hover_text("Back to the standard look")
            .clicked()
        {
            let keep = w.style.knock_out;
            w.style = Style {
                knock_out: keep,
                ..Style::default()
            };
        }
    });
    step
}
