//! The welcome window: a three-page introduction shown the first time CursorPie starts (and again
//! from the tray item "Welcome guide...").
//!
//! It is drawn in the same native window and with the same card, shadow, colors and widgets as
//! Settings, so it looks like part of the app and needs no second window. While it is open
//! `App::settings_open` is true (so the pie stays hidden) and `App::welcome` holds its state.
//!
//! ```text
//! +--------------------------------------------------------+
//! | logo CursorPie                                    Skip |  top bar
//! +--------------------------------------------------------+
//! |                                                        |
//! |   page 1  How it works    (hold - flick - release)     |
//! |   page 2  Get set up      (macOS: Accessibility,       |
//! |                            Windows: the system tray)   |
//! |   page 3  All set         (launch at login, tips)      |
//! |                                                        |
//! +--------------------------------------------------------+
//! | o o o                                  Back    Next    |  footer
//! +--------------------------------------------------------+
//! ```
//!
//! The texts that differ between macOS and Windows are small pure functions of [`Os`], so a test
//! can check both without running on both.

use super::brand;
use super::theme::*;
use super::widgets::*;
use crate::about::*;
use crate::config::cfg;
use crate::platform::{self, Os};
use crate::tap::TAP_ACTIVE;
use crate::ui::app::App;
use crate::ui::draw::*;
use crate::ui::fonts;
use crate::ui::symbols;
use eframe::egui::{self, Color32, Stroke, Vec2};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Number of pages of the welcome window.
pub const PAGES: usize = 3;

/// Height of the top bar.
const TOP_H: f32 = 68.0;
/// Height of the footer (page dots and buttons).
const FOOT_H: f32 = 76.0;
/// Space between the bands and the page, and the page's padding.
const PAD: f32 = GAP * 3.0;

/// State of the open welcome window.
pub struct Welcome {
    /// Index of the page on screen, `0..PAGES`.
    pub page: usize,
    /// The "Launch at login" box of the last page.
    pub launch_at_login: bool,
    /// Set when the window was left with "Get started" or "Open Settings" (not with Skip or Esc):
    /// only then is the launch-at-login box applied.
    pub apply: bool,
    /// Open Settings as soon as the window has closed.
    pub open_settings_after: bool,
}

impl Welcome {
    /// A new welcome window on its first page; `launch_at_login` is the current system setting.
    pub fn new(launch_at_login: bool) -> Self {
        Self {
            page: 0,
            launch_at_login,
            apply: false,
            open_settings_after: false,
        }
    }
}

/// What a button of the window asked for this frame.
enum Step {
    None,
    Next,
    Back,
    Skip,
    Finish { open_settings: bool },
}

/// Heading of the second page.
pub fn setup_title(os: Os) -> &'static str {
    match os {
        Os::Mac => "Allow CursorPie to watch the mouse",
        Os::Windows => "CursorPie lives in the system tray",
        Os::Linux => "CursorPie works in your X11 session",
    }
}

/// The sentence under the heading of the second page: what the page is about, in one line.
pub fn setup_lead(os: Os) -> &'static str {
    match os {
        Os::Mac => "Do this once, in System Settings:",
        Os::Windows => {
            "CursorPie has no window of its own: look for its pie icon next to the clock."
        }
        Os::Linux => "X11 asks for no permission.",
    }
}

/// What to do, one short step each, under [`setup_lead`]. On macOS they are numbered.
pub fn setup_steps(os: Os) -> &'static [&'static str] {
    match os {
        Os::Mac => &[
            "Open Privacy & Security > Accessibility.",
            "Switch CursorPie on. Not listed? Press + and choose it in Applications.",
            "If macOS asks for Input Monitoring, allow that too.",
        ],
        Os::Windows => &[
            "Click the ^ arrow next to the clock if you do not see the icon.",
            "Right-click the icon for Settings, Pause and Quit.",
            "Windows asks for no permission. Programs that run as administrator only react if \
             CursorPie runs as administrator too.",
        ],
        Os::Linux => &[
            "CursorPie has no tray icon on Linux yet: hold the trigger in the center of the pie \
             to open Settings.",
            "Quit from the bottom of Settings.",
            "In a Wayland session it only sees X11 and XWayland windows, so log in with an X11 \
             session for full support.",
        ],
    }
}

/// A line of small print for the right column of the second page, shown while the permission is
/// still missing. Empty where nothing needs saying.
pub fn setup_note(os: Os) -> &'static str {
    match os {
        Os::Mac => {
            "macOS only lets an app watch the mouse after you allow it. This page notices the \
             change by itself; there is nothing to restart."
        }
        Os::Windows | Os::Linux => "",
    }
}

/// The three steps of the gesture, as `(label, text)`. `hold_ms` is the configured hold time.
pub fn gesture_steps(hold_ms: u64) -> [(&'static str, String); 3] {
    [
        (
            "Hold",
            format!(
                "Press and hold the middle mouse button for {}.",
                seconds(hold_ms)
            ),
        ),
        (
            "Flick",
            "The pie opens under the pointer. Move toward the slice you want.".into(),
        ),
        (
            "Release",
            "Let go over a slice and CursorPie runs it: a shortcut, an app, a link.".into(),
        ),
    ]
}

/// `ms` as a short time: "150 ms" below one second, "1.2 s" from one second on.
fn seconds(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.1} s", ms as f64 / 1000.0)
    }
}

/// Short tips of the last page.
pub fn tips(os: Os, settings_hold_ms: u64) -> Vec<String> {
    let last = if os.has_tray() {
        format!(
            "Pause, Settings and Quit are in the {} icon. \"Welcome guide...\" there shows this window again.",
            os.tray_name()
        )
    } else {
        "Pause and Quit are at the bottom of Settings. There is no tray icon on this system yet."
            .to_string()
    };
    vec![
        format!(
            "Hold the trigger in the center of the pie for {} to open Settings.",
            seconds(settings_hold_ms)
        ),
        "Double-click the trigger over an app that has its own profile to open that app's pie."
            .into(),
        last,
    ]
}

impl App {
    /// Draws the welcome window: animation, top bar, current page, footer.
    pub fn welcome_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();

        // the window is still becoming the card: an empty frame, the animation waits
        if self.window_settling(&ctx) {
            return;
        }

        // open / close animation (the same as Settings)
        let dt = ctx.input(|i| i.stable_dt).min(0.05);
        let target = if self.closing.is_some() { 0.0 } else { 1.0 };
        if smooth_approach(&mut self.settings_anim, target, dt, 14.0) {
            ctx.request_repaint();
        }
        if self.closing.is_some() && self.settings_anim <= 0.02 {
            self.finish_welcome(&ctx);
            return;
        }
        // keep the permission status fresh while the window is open
        ctx.request_repaint_after(Duration::from_millis(1000));

        if self.closing.is_none() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.closing = Some(false);
        }
        let Some(page) = self.welcome.as_ref().map(|w| w.page) else {
            return;
        };

        let t = self.settings_anim.clamp(0.0, 1.0);
        let slide = (1.0 - t) * (1.0 - t) * 26.0;
        let accent = rgb(cfg().accent);
        style_settings(ui, accent);
        ui.set_opacity(t);

        let full = ui.max_rect();
        let card_rect = snap_rect(
            egui::Rect::from_center_size(full.center(), self.card_size)
                .translate(Vec2::new(0.0, slide)),
            ctx.pixels_per_point(),
        );
        let top = egui::Rect::from_min_size(card_rect.min, Vec2::new(card_rect.width(), TOP_H));
        let foot = egui::Rect::from_min_max(
            egui::pos2(card_rect.min.x, card_rect.max.y - FOOT_H),
            card_rect.max,
        );
        let body = egui::Rect::from_min_max(
            egui::pos2(card_rect.min.x + PAD, top.max.y + GAP),
            egui::pos2(card_rect.max.x - PAD, foot.min.y - GAP / 2.0),
        );

        let painter = ui.painter().clone();
        painter.add(window_shadow().as_shape(card_rect, CARD_RADIUS));
        painter.rect_filled(card_rect, CARD_RADIUS, pal().bg);
        window_outline(&painter, card_rect, accent);

        // dragging the empty part of the top bar moves the window
        let drag = ui.interact(
            top,
            egui::Id::new("welcome_drag"),
            egui::Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        let mut step = Step::None;

        // ---------------------------------------------------------------- top bar
        draw_wordmark(&painter, top.min.x + PAD, top.center().y, 30.0);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(top.shrink2(Vec2::new(PAD, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                if page + 1 < PAGES
                    && button(ui, Look::Plain, None, "Skip")
                        .on_hover_text("Close this guide. You can reopen it from the tray menu.")
                        .clicked()
                {
                    step = Step::Skip;
                }
            },
        );

        // ---------------------------------------------------------------- page
        // each page fades in and rises a few points when it appears
        let enter = page_enter(&ctx, page);
        let rise = (1.0 - enter) * 16.0;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(body.translate(Vec2::new(0.0, rise))),
            |ui| {
                ui.set_opacity(t * enter);
                ui.set_clip_rect(ui.clip_rect().intersect(body));
                match page {
                    0 => intro_page(ui, accent),
                    1 => setup_page(ui, accent, platform::OS),
                    _ => {
                        if let Some(w) = self.welcome.as_mut()
                            && ready_page(ui, w, accent, platform::OS)
                        {
                            step = Step::Finish {
                                open_settings: true,
                            };
                        }
                    }
                }
            },
        );

        // ---------------------------------------------------------------- footer
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(foot.shrink2(Vec2::new(PAD, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                // page dots: the current one stretches into a pill
                ui.spacing_mut().item_spacing.x = 6.0;
                for i in 0..PAGES {
                    let wide = ctx.animate_value_with_time(
                        egui::Id::new(("welcome_dot", i)),
                        if i == page { 22.0 } else { 8.0 },
                        0.22,
                    );
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(wide, 8.0), egui::Sense::hover());
                    let color = if i == page { accent } else { pal().border };
                    ui.painter().rect_filled(rect, PILL, color);
                }
            },
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(foot.shrink2(Vec2::new(PAD, 0.0)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if page + 1 < PAGES {
                    if button(
                        ui,
                        Look::Primary(accent),
                        Some(symbols::CARET_RIGHT),
                        "Next",
                    )
                    .clicked()
                    {
                        step = Step::Next;
                    }
                } else if button(
                    ui,
                    Look::Primary(accent),
                    Some(symbols::CHECK),
                    "Get started",
                )
                .clicked()
                {
                    step = Step::Finish {
                        open_settings: false,
                    };
                }
                if page > 0 && button(ui, Look::Soft, None, "Back").clicked() {
                    step = Step::Back;
                }
            },
        );

        // Enter goes forward, like the primary button
        if matches!(step, Step::None)
            && self.closing.is_none()
            && ctx.input(|i| i.key_pressed(egui::Key::Enter))
        {
            step = if page + 1 < PAGES {
                Step::Next
            } else {
                Step::Finish {
                    open_settings: false,
                }
            };
        }

        if let Some(w) = self.welcome.as_mut() {
            match step {
                Step::None => {}
                Step::Next => w.page = (w.page + 1).min(PAGES - 1),
                Step::Back => w.page = w.page.saturating_sub(1),
                Step::Skip => self.closing = Some(false),
                Step::Finish { open_settings } => {
                    w.apply = true;
                    w.open_settings_after = open_settings;
                    self.closing = Some(false);
                }
            }
        }
    }
}

/// Eased 0..1 progress of the entrance of `page`, restarted every time the page changes.
fn page_enter(ctx: &egui::Context, page: usize) -> f32 {
    const DURATION: f64 = 0.45;
    let now = ctx.input(|i| i.time);
    let id = egui::Id::new("welcome_page_enter");
    let since = ctx.data_mut(|d| {
        let (shown, start) = d.get_temp_mut_or_insert_with(id, || (page, now));
        if *shown != page {
            *shown = page;
            *start = now;
        }
        now - *start
    });
    let p = (since / DURATION).clamp(0.0, 1.0) as f32;
    if p < 1.0 {
        ctx.request_repaint();
    }
    1.0 - (1.0 - p).powi(3) // ease out
}

/// A small spaced-out label above a heading, in the accent color: "STEP 2 OF 3".
fn eyebrow(ui: &mut egui::Ui, text: &str, accent: Color32) {
    ui.label(fonts::title(text.to_uppercase(), 11.5, accent_text(accent)));
    ui.add_space(4.0);
}

/// A big heading with a line of secondary text under it.
fn heading(ui: &mut egui::Ui, title: &str) {
    ui.label(fonts::title(title, 28.0, pal().text));
    ui.add_space(GAP);
}

/// Seconds since the program started drawing, for the animations of the first page.
fn clock(ctx: &egui::Context) -> f32 {
    ctx.input(|i| i.time) as f32
}

/// The banner of the first page: pulsing rings around the mark, the name in the brand font (the
/// lettering of the logo) and the version.
fn hero(ui: &mut egui::Ui, accent: Color32, height: f32) {
    let ctx = ui.ctx().clone();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    let time = clock(&ctx);
    ctx.request_repaint_after(Duration::from_millis(33));

    // the band: a tint of the accent, with a faint copy of the mark bleeding off the right edge
    painter.rect_filled(rect, PANEL_R, wash(accent, 0.10));
    painter.rect_stroke(
        rect,
        PANEL_R,
        Stroke::new(1.0, wash(accent, 0.22)),
        egui::StrokeKind::Inside,
    );
    let ghost = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 56.0, rect.center().y + 8.0),
        Vec2::splat(height * 1.9),
    );
    brand::paint(&painter, brand::Art::Mark, ghost, wash(accent, 0.09));

    // rings that leave the mark and fade, three at a time
    let c = egui::pos2(rect.min.x + 30.0 + 62.0, rect.center().y);
    let r0 = (height * 0.5 - 8.0).min(54.0);
    for k in 0..3 {
        let phase = (time * 0.45 + k as f32 / 3.0).fract();
        let r = r0 + phase * 20.0;
        let a = (1.0 - phase).powi(2) * 0.5;
        painter.circle_stroke(c, r, Stroke::new(1.5, wash(accent, a)));
    }
    painter.circle_filled(c, r0 - 2.0, pal().card);
    painter.circle_stroke(c, r0 - 2.0, Stroke::new(1.0, wash(accent, 0.35)));
    draw_logo(&painter, c, r0 * 0.58);

    // the words; on a short banner (a small screen) they shrink together
    let k = (height / 150.0).clamp(0.72, 1.0);
    let x = c.x + r0 + 34.0;
    painter.text(
        egui::pos2(x, rect.center().y - 44.0 * k),
        egui::Align2::LEFT_CENTER,
        "WELCOME TO",
        egui::FontId::new(11.5, fonts::semibold()),
        accent_text(accent),
    );
    painter.text(
        egui::pos2(x - 2.0, rect.center().y - 10.0 * k),
        egui::Align2::LEFT_CENTER,
        APP_NAME,
        egui::FontId::new(52.0 * k, fonts::brand()),
        pal().text,
    );
    painter.text(
        egui::pos2(x, rect.center().y + 28.0 * k),
        egui::Align2::LEFT_CENTER,
        TAGLINE,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
        pal().muted,
    );
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            egui::pos2(x, rect.center().y + 42.0 * k),
            Vec2::new(260.0, 28.0),
        )),
        |ui| badge(ui, &format!("Version {VERSION}"), accent_text(accent)),
    );
}

/// Page 1: what CursorPie is and the three-step gesture.
fn intro_page(ui: &mut egui::Ui, accent: Color32) {
    let ctx = ui.ctx().clone();
    let avail = ui.available_height();
    // the title row and the gaps around it take about 42 points; the cards need up to 190
    let hero_h = (avail - 50.0 - 190.0).clamp(104.0, 176.0);
    hero(ui, accent, hero_h);

    ui.add_space(GAP + 2.0);
    ui.horizontal(|ui| {
        // a row is at least as high as a button by default, which left a gap under this one
        ui.spacing_mut().interact_size.y = 18.0;
        ui.label(fonts::title("Three moves open anything", 15.0, pal().text));
        ui.add_space(4.0);
        muted(ui, "Try them once you finish this guide.");
    });
    ui.add_space(GAP - 4.0);
    // what is left for the cards, less the frame around their content
    let card_h = (ui.available_height() - 6.0).clamp(126.0, 176.0);

    let steps = gesture_steps(cfg().pie_hold_ms);
    let icons = [
        symbols::CURSOR_CLICK,
        symbols::ARROWS_OUT,
        symbols::CHECK_CIRCLE,
    ];
    // the three steps light up one after the other, as if the gesture were being made
    let active = ((clock(&ctx) / 1.4) as usize) % 3;
    ui.spacing_mut().item_spacing.x = 22.0;
    let painter = ui.painter().clone();
    let mut arrows = Vec::new();
    ui.columns(3, |cols| {
        for (i, (col, (icon, (label, text)))) in cols
            .iter_mut()
            .zip(icons.into_iter().zip(steps))
            .enumerate()
        {
            // columns lay out justified, which would stretch the words of the text
            let left = egui::Layout::top_down(egui::Align::Min);
            let on = ctx.animate_bool_with_time(
                egui::Id::new(("welcome_step_on", i)),
                i == active,
                0.35,
            );
            // The card is a rectangle of a fixed height, drawn by hand, and its content is laid out
            // inside it and clipped to it. A card that grew with its text would be taller when the
            // text takes three lines (cut off at the bottom of the page) than when it takes two.
            let (r, _) = col.allocate_exact_size(
                Vec2::new(col.available_width(), card_h),
                egui::Sense::hover(),
            );
            col.painter().rect_filled(r, PANEL_R, pal().card);
            col.painter().rect_stroke(
                r,
                PANEL_R,
                Stroke::new(1.0, pal().divider),
                egui::StrokeKind::Inside,
            );
            let inner = r.shrink2(Vec2::new(f32::from(PANEL_PAD), 14.0));
            let content =
                col.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(left), |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(r));
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::splat(42.0), egui::Sense::hover());
                        let p = ui.painter();
                        p.circle_filled(
                            rect.center(),
                            21.0,
                            wash(accent, 0.12).lerp_to_gamma(accent, on),
                        );
                        fonts::paint_icon(
                            p,
                            rect.center(),
                            icon,
                            22.0,
                            accent.lerp_to_gamma(on_accent(accent), on),
                            true,
                        );
                        ui.add_space(4.0);
                        ui.label(fonts::title(
                            format!("0{}", i + 1),
                            26.0,
                            wash(accent, 0.28),
                        ));
                    });
                    ui.add_space(6.0);
                    ui.label(fonts::title(label, 17.0, pal().text));
                    muted(ui, &text);
                });
            // how far the text reaches below the card's padding: the test of this page reads it
            let over = (content.response.rect.max.y - inner.max.y).max(0.0);
            ctx.data_mut(|d| {
                let id = egui::Id::new("welcome_card_overflow");
                let worst: f32 = d.get_temp(id).unwrap_or(0.0);
                d.insert_temp(id, worst.max(over));
            });
            painter.rect_stroke(
                r,
                PANEL_R,
                Stroke::new(1.5, wash(accent, 0.55 * on)),
                egui::StrokeKind::Inside,
            );
            if i < 2 {
                arrows.push(egui::pos2(r.max.x + 11.0, r.center().y));
            }
        }
    });
    for at in arrows {
        fonts::paint_icon(
            ui.painter(),
            at,
            symbols::CARET_RIGHT,
            16.0,
            pal().muted,
            false,
        );
    }
}

/// The free area of `ui` cut in two side by side: the left one `left_frac` of the width, a gap of
/// [`PAD`] between them. Both start at the top of the free area.
fn two_columns(ui: &egui::Ui, left_frac: f32) -> (egui::Rect, egui::Rect) {
    let area = ui.available_rect_before_wrap();
    let left_w = ((area.width() - PAD) * left_frac).floor();
    let left = egui::Rect::from_min_size(area.min, Vec2::new(left_w, area.height()));
    let right =
        egui::Rect::from_min_max(egui::pos2(area.min.x + left_w + PAD, area.min.y), area.max);
    (left, right)
}

/// A short list, one item per row: a round badge (the number of the step, or a dot) and the text
/// wrapped beside it. Each item is laid out as a whole, so a line never runs under the badge and
/// the rows are spaced evenly.
fn step_list(ui: &mut egui::Ui, accent: Color32, items: &[&str], numbered: bool) {
    const BADGE: f32 = 22.0;
    const TEXT_X: f32 = BADGE + 12.0;
    let width = ui.available_width();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            ui.add_space(8.0);
        }
        let text = ui.painter().layout(
            (*item).to_owned(),
            egui::FontId::proportional(14.0),
            pal().text,
            (width - TEXT_X).max(80.0),
        );
        let h = text.size().y.max(BADGE);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, h), egui::Sense::hover());
        let painter = ui.painter();
        let c = egui::pos2(rect.min.x + BADGE / 2.0, rect.min.y + BADGE / 2.0);
        painter.circle_filled(c, BADGE / 2.0, wash(accent, 0.14));
        if numbered {
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                (i + 1).to_string(),
                egui::FontId::new(12.0, fonts::semibold()),
                accent_text(accent),
            );
        } else {
            painter.circle_filled(c, 3.0, accent_text(accent));
        }
        // the first line is as high as the badge, so the text starts a little above its top
        let top = rect.min.y + ((BADGE - 17.0) / 2.0).max(0.0);
        painter.galley(
            egui::pos2(rect.min.x + TEXT_X, top).round(),
            text,
            Color32::PLACEHOLDER,
        );
    }
}

/// Page 2: the permission (macOS) or the tray (Windows), with the live status.
fn setup_page(ui: &mut egui::Ui, accent: Color32, os: Os) {
    eyebrow(ui, &format!("Step 2 of {PAGES}"), accent);
    heading(ui, setup_title(os));

    let running = TAP_ACTIVE.load(Ordering::SeqCst);
    let ready = if os.needs_permission() {
        platform::is_trusted_cached() && running
    } else {
        running
    };
    // the explanation on the left, the live status on the right: nothing is left empty or cut off
    let (left, right) = two_columns(ui, 0.5);
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        ui.add_space(4.0);
        muted(ui, setup_lead(os));
        ui.add_space(GAP - 2.0);
        step_list(ui, accent, setup_steps(os), os.needs_permission());
        ui.add_space(GAP);
        if os.needs_permission() && !ready {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                if button(
                    ui,
                    Look::Soft,
                    Some(symbols::ARROW_SQUARE_OUT),
                    "Accessibility settings",
                )
                .clicked()
                {
                    platform::open_accessibility_settings();
                }
                if button(
                    ui,
                    Look::Soft,
                    Some(symbols::ARROW_SQUARE_OUT),
                    "Input Monitoring settings",
                )
                .clicked()
                {
                    platform::open_input_monitoring_settings();
                }
            });
        }
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        panel(ui, "Status", |ui| {
            // the control of each row is as wide as its pill: the label keeps the rest and is
            // never broken in two ("Accessibilit" / "y")
            if os.needs_permission() {
                let granted = platform::is_trusted_cached();
                let text = if granted {
                    "Granted"
                } else {
                    "Not granted yet"
                };
                let w = status_width(ui, text) + 2.0;
                row_wide(ui, "Accessibility", "", w, |ui| status(ui, granted, text));
            }
            let name = if os.needs_permission() {
                "Event tap"
            } else {
                "Mouse hook"
            };
            let text = if running { "Running" } else { "Not running" };
            // the pill says the state; the way out goes in small print under the name
            let hint = if running { "" } else { "Retrying every 2 s" };
            let w = status_width(ui, text) + 2.0;
            row_wide(ui, name, hint, w, |ui| status(ui, running, text));
        });
        if ready {
            ui.horizontal(|ui| {
                status(ui, true, "Ready");
                muted(ui, "CursorPie can see your mouse.");
            });
        } else if !setup_note(os).is_empty() {
            ui.add(egui::Label::new(
                egui::RichText::new(setup_note(os))
                    .size(13.0)
                    .color(pal().muted),
            ));
        }
    });
}

/// Page 3: tips and the launch-at-login box. Returns true when "Open Settings" was clicked.
fn ready_page(ui: &mut egui::Ui, w: &mut Welcome, accent: Color32, os: Os) -> bool {
    eyebrow(ui, &format!("Step 3 of {PAGES}"), accent);
    ui.horizontal(|ui| {
        ui.label(fonts::title("You're all set", 28.0, pal().text));
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 15.0, accent);
        fonts::paint_icon(
            ui.painter(),
            rect.center(),
            symbols::CHECK,
            17.0,
            on_accent(accent),
            false,
        );
    });
    ui.add_space(GAP);
    let hold = cfg().settings_hold_ms;
    let mut open_settings = false;
    // the tips on the left, the startup choice and the way into Settings on the right
    let (left, right) = two_columns(ui, 0.56);
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        panel(ui, "Good to know", |ui| {
            for tip in tips(os, hold) {
                note(ui, &tip);
            }
        });
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        panel(ui, "Startup", |ui| {
            toggle_row(
                ui,
                &mut w.launch_at_login,
                "Launch CursorPie at login",
                os.welcome_startup_hint(),
                accent,
            );
        });
        open_settings = button(
            ui,
            Look::Soft,
            Some(symbols::SLIDERS_HORIZONTAL),
            "Open Settings now",
        )
        .clicked();
    });
    open_settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_systems_have_their_own_setup_page() {
        assert_ne!(setup_title(Os::Mac), setup_title(Os::Windows));
        assert!(
            setup_steps(Os::Mac)
                .iter()
                .any(|s| s.contains("Accessibility"))
        );
        assert!(setup_title(Os::Windows).contains("system tray"));
        assert!(
            setup_steps(Os::Windows)
                .iter()
                .any(|s| s.contains("administrator"))
        );
        assert!(
            !setup_steps(Os::Windows)
                .iter()
                .any(|s| s.contains("Accessibility"))
        );
        assert!(!setup_lead(Os::Windows).contains("Accessibility"));
    }

    #[test]
    fn every_setup_step_is_one_short_sentence_or_two() {
        // a wall of text is what the steps replaced: keep each one easy to take in at a glance
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            assert!(setup_lead(os).len() < 90, "{os:?}");
            assert!(setup_note(os).len() < 160, "{os:?}");
            for step in setup_steps(os) {
                assert!(step.len() < 160, "{os:?}: {step}");
            }
        }
    }

    #[test]
    fn the_gesture_uses_the_configured_hold_time() {
        let steps = gesture_steps(150);
        assert!(steps[0].1.contains("150 ms"), "{}", steps[0].1);
        assert!(gesture_steps(1200)[0].1.contains("1.2 s"));
        assert_eq!(steps.len(), 3);
    }

    #[test]
    fn tips_name_the_tray_of_the_system() {
        assert!(tips(Os::Mac, 1200).iter().any(|t| t.contains("menu bar")));
        assert!(
            tips(Os::Windows, 1200)
                .iter()
                .any(|t| t.contains("system tray"))
        );
        assert!(tips(Os::Mac, 1200)[0].contains("1.2 s"));
    }

    #[test]
    fn linux_has_its_own_setup_page_and_no_tray_tip() {
        assert!(setup_title(Os::Linux).contains("X11"));
        let steps = setup_steps(Os::Linux);
        assert!(
            steps.iter().any(|s| s.contains("no tray icon")),
            "{steps:?}"
        );
        assert!(!steps.iter().any(|s| s.contains("Accessibility")));
        assert!(!setup_lead(Os::Linux).contains("Accessibility"));
        let t = tips(Os::Linux, 1200);
        assert_eq!(t.len(), 3);
        assert!(t.iter().all(|x| !x.contains("icon.")), "{t:?}");
        assert!(t[2].contains("bottom of Settings"));
    }

    /// Draws every page for a few frames in a headless egui context: a smoke test for layout
    /// panics (zero widths, bad column counts) that no other test would reach.
    #[test]
    fn every_page_draws_without_panicking() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let accent = egui::Color32::from_rgb(64, 156, 255);
        let mut w = Welcome::new(false);
        for page in 0..PAGES {
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(760.0, 520.0),
                    )),
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| match page {
                    0 => intro_page(ui, accent),
                    1 => setup_page(ui, accent, platform::OS),
                    _ => {
                        ready_page(ui, &mut w, accent, platform::OS);
                    }
                });
                output.textures_delta.clear(); // nothing uploads the textures here
            }
        }
    }

    /// The body of the card, as `welcome_ui` cuts it, for a card of `card` points.
    fn body_size(card: [f32; 2]) -> Vec2 {
        Vec2::new(
            card[0] - 2.0 * PAD,
            card[1] - TOP_H - GAP - FOOT_H + GAP / 2.0,
        )
    }

    /// Every OS variant must fit both card sizes, regardless of the host running the test.
    #[test]
    fn every_page_fits_the_body_of_the_card() {
        use crate::ui::app::{WELCOME_MIN, WELCOME_SIZE};
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let accent = egui::Color32::from_rgb(90, 120, 94);
        let mut w = Welcome::new(false);

        for os in [Os::Mac, Os::Windows, Os::Linux] {
            for card in [WELCOME_SIZE, WELCOME_MIN] {
                let body = body_size(card);
                for page in 0..PAGES {
                    let mut used = 0.0;
                    ctx.data_mut(|d| {
                        d.insert_temp(egui::Id::new("welcome_card_overflow"), 0.0_f32)
                    });
                    for _ in 0..3 {
                        let input = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, body)),
                            ..Default::default()
                        };
                        let mut output = ctx.run_ui(input, |ui| {
                            match page {
                                0 => intro_page(ui, accent),
                                1 => setup_page(ui, accent, os),
                                _ => {
                                    ready_page(ui, &mut w, accent, os);
                                }
                            }
                            used = ui.min_rect().height();
                        });
                        output.textures_delta.clear();
                    }
                    let over: f32 = ctx
                        .data(|d| d.get_temp(egui::Id::new("welcome_card_overflow")))
                        .unwrap_or(0.0);
                    assert!(
                        over <= 0.5,
                        "page {page} on {os:?}: step card text reaches {over} points out of its card ({card:?})"
                    );
                    assert!(
                        used <= body.y + 0.5,
                        "page {page} on {os:?} needs {used} points but the body of a {card:?} card has {}",
                        body.y
                    );
                }
            }
        }
    }

    #[test]
    fn a_new_welcome_starts_on_the_first_page() {
        let w = Welcome::new(true);
        assert_eq!((w.page, w.launch_at_login, w.apply), (0, true, false));
    }
}
