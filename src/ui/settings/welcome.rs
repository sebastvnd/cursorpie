//! The welcome window: a six-page introduction shown the first time CursorPie starts (and again
//! from the tray item "Welcome guide...").
//!
//! It is drawn in the same native window and with the same card, shadow, colors and widgets as
//! Settings, so it looks like part of the app and needs no second window. Its own pieces (the
//! banner, the tiles, the persona cards, the small pies, the step badges, the page dots) are
//! *stickers* in the style of the website (`assets/site.css`): a 2-point outline in the line color
//! of the theme, a hard shadow that falls right and down, and the candy colors of the site, so they
//! read the same in the light and the dark appearance and in every theme. While it is open
//! `App::settings_open` is true (so the pie stays hidden) and `App::welcome` holds its state.
//!
//! ```text
//! +--------------------------------------------------------+
//! | logo CursorPie                                    Skip |  top bar
//! +--------------------------------------------------------+
//! |                                                        |
//! |   page 0  Welcome       (banner and three promises)    |
//! |   page 1  How it works  (hold - flick - release, live) |
//! |   page 2  About you     (what do you do most?)         |
//! |   page 3  Your pie      (the pie that the pick made)   |
//! |   page 4  Setup         (macOS: Accessibility,         |
//! |                          Windows: the system tray)     |
//! |   page 5  All set       (launch at login, tips)        |
//! |                                                        |
//! +--------------------------------------------------------+
//! | o o o o o o  page name                 Back    Next    |  footer
//! +--------------------------------------------------------+
//! ```
//!
//! The pick of page 2 is kept in [`Welcome::picks`]; it is written into the config only when the
//! guide ends with "Get started" or "Open Settings" (see `App::finish_welcome` and
//! [`persona::apply`]), so Skip and Esc leave the pie as it was.
//!
//! The texts that differ between macOS, Windows and Linux are small pure functions of [`Os`], so a
//! test can check all of them without running on all.

use super::brand;
use super::theme::*;
use super::widgets::*;
use crate::about::*;
use crate::config::cfg;
use crate::config::persona::{self, Persona};
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
pub const PAGES: usize = 6;

/// The name of every page, shown beside the page dots.
const PAGE_NAMES: [&str; PAGES] = [
    "Welcome",
    "How it works",
    "About you",
    "Your pie",
    "Setup",
    "All set",
];

/// Room the body leaves at its right (and the clip leaves below it) for the hard shadow of a
/// sticker on the edge, so no shadow is cut off.
const BODY_ROOM: f32 = HARD + 4.0;

/// Height of the top bar.
const TOP_H: f32 = 72.0;
/// Height of the footer (page dots and buttons).
const FOOT_H: f32 = 84.0;
/// Space between the bands and the page, and the page's padding.
const PAD: f32 = GAP * 3.0;

/// How far the hard shadow of a sticker falls, right and down, in points (`--hard-sm` of the site).
const HARD: f32 = 4.0;

/// The candy colors of the website (`--pink`, `--sky`, `--violet`, `--orange`, `--green`): the
/// stickers, the step badges and the gesture cards take them in turn. Ink reads on every one.
const CANDY: [[u8; 3]; 5] = [
    [255, 94, 168],
    [90, 208, 255],
    [141, 107, 255],
    [255, 154, 61],
    [47, 210, 122],
];

/// The `i`-th candy color (it wraps around).
fn candy(i: usize) -> Color32 {
    let [r, g, b] = CANDY[i % CANDY.len()];
    Color32::from_rgb(r, g, b)
}

/// A sticker: `fill` inside a 2-point outline in the line color, over a hard shadow of [`HARD`]
/// points. The shadow is outside `rect`, so a caller that clips leaves [`HARD`] points free.
fn sticker(painter: &egui::Painter, rect: egui::Rect, radius: egui::CornerRadius, fill: Color32) {
    let line = pal().line;
    painter.rect_filled(rect.translate(Vec2::splat(HARD)), radius, line);
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(2.0, line),
        egui::StrokeKind::Inside,
    );
}

/// A round sticker: the same outline and hard shadow around a disc.
fn sticker_disc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    fill: Color32,
    drop: f32,
) {
    let line = pal().line;
    painter.circle_filled(center + Vec2::splat(drop), radius, line);
    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(center, radius, Stroke::new(2.0, line));
}

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
    /// What the person said they do most (page 2), in the order they picked it. Empty: nothing was
    /// picked and the pie stays as it is.
    pub picks: Vec<Persona>,
}

impl Welcome {
    /// A new welcome window on its first page; `launch_at_login` is the current system setting.
    pub fn new(launch_at_login: bool) -> Self {
        Self {
            page: 0,
            launch_at_login,
            apply: false,
            open_settings_after: false,
            picks: Vec::new(),
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
        Os::Mac => "Allow CursorPie to see your mouse",
        Os::Windows => "CursorPie lives in the system tray",
        Os::Linux => "CursorPie works in X11 and Wayland sessions",
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
            "Switch CursorPie on, or press + and add it from Applications.",
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
            "A Wayland session works too (Plasma 6.8 has no other): it needs the udev rule in \
             packaging/linux/.",
        ],
    }
}

/// The sentence under the heading of the second page in a Wayland session, where (unlike X11) one
/// permission is needed.
pub fn wayland_setup_lead() -> &'static str {
    "A Wayland session needs one permission, once:"
}

/// What to do in a Wayland session, one short step each. The first two are the install of the
/// udev rule (`wayland::UDEV_RULE_FILE`); they are what the status on this page waits for.
pub fn wayland_setup_steps() -> &'static [&'static str] {
    &[
        "Copy 70-cursorpie.rules (in the package, or in packaging/linux/) to /etc/udev/rules.d/.",
        "Run `sudo udevadm control --reload && sudo udevadm trigger`, then log out and in.",
        "No tray icon on Linux yet: hold the trigger in the center of the pie to open Settings.",
    ]
}

/// Small print under the status in a Wayland session while the permission is missing.
pub fn wayland_setup_note() -> &'static str {
    "Until then CursorPie does not touch your mouse. This page notices the change by itself."
}

/// Whether this is a Linux build running in a Wayland session (the second page then shows the
/// permission step and its live status).
fn wayland_session() -> bool {
    platform::OS == Os::Linux
        && crate::platform::linux::session::Session::current().server
            == crate::platform::linux::session::DisplayServer::Wayland
}

/// A line of small print for the right column of the second page, shown while the permission is
/// still missing. Empty where nothing needs saying.
pub fn setup_note(os: Os) -> &'static str {
    match os {
        Os::Mac => {
            "macOS only lets an app watch the mouse after you allow it. This page notices the change \
             by itself."
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
                "Press and hold the middle mouse button, or touch and hold the trackpad, for {}.",
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
            egui::pos2(card_rect.max.x - PAD - BODY_ROOM, foot.min.y - GAP / 2.0),
        );

        let painter = ui.painter().clone();
        painter.add(window_shadow().as_shape(card_rect, card_radius()));
        painter.rect_filled(card_rect, card_radius(), pal().bg);
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
                    && button(ui, Look::Soft, None, "Skip")
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
                // the clip leaves room right and below for the hard shadow of the stickers
                let clip = egui::Rect::from_min_max(body.min, body.max + Vec2::splat(BODY_ROOM));
                ui.set_clip_rect(ui.clip_rect().intersect(clip));
                match page {
                    0 => intro_page(ui, accent),
                    1 => how_page(ui, accent),
                    2 => {
                        if let Some(w) = self.welcome.as_mut() {
                            persona_page(ui, w, accent);
                        }
                    }
                    3 => {
                        if let Some(w) = self.welcome.as_ref() {
                            pie_page(ui, w, accent, platform::OS);
                        }
                    }
                    4 => setup_page(ui, accent, platform::OS),
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
                // page dots, outlined like the buttons: the current one stretches into a pill
                // filled with the accent
                ui.spacing_mut().item_spacing.x = 8.0;
                for i in 0..PAGES {
                    let wide = ctx.animate_value_with_time(
                        egui::Id::new(("welcome_dot", i)),
                        if i == page { 30.0 } else { 12.0 },
                        0.22,
                    );
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(wide, 12.0), egui::Sense::hover());
                    let color = if i == page { accent } else { pal().card };
                    ui.painter().rect_filled(rect, PILL, color);
                    ui.painter().rect_stroke(
                        rect,
                        PILL,
                        Stroke::new(2.0, pal().line),
                        egui::StrokeKind::Inside,
                    );
                }
                ui.add_space(10.0);
                muted(ui, PAGE_NAMES[page]);
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

/// A small sticker above a heading, in the accent color: "STEP 2 OF 5". It is as high as the plain
/// label it replaced (the pill and its 2-point shadow, then a point of air), so no page grows.
fn eyebrow(ui: &mut egui::Ui, text: &str, accent: Color32) {
    let ink = on_accent(accent);
    let galley = ui.painter().layout_no_wrap(
        text.to_uppercase(),
        egui::FontId::new(10.5, fonts::semibold()),
        ink,
    );
    let pill = Vec2::new(galley.size().x + 22.0, 16.0);
    let (rect, _) = ui.allocate_exact_size(pill + Vec2::splat(2.0), egui::Sense::hover());
    let body = egui::Rect::from_min_size(rect.min, pill);
    let line = pal().line;
    let painter = ui.painter();
    painter.rect_filled(body.translate(Vec2::splat(2.0)), PILL, line);
    painter.rect_filled(body, PILL, accent);
    painter.rect_stroke(body, PILL, Stroke::new(1.5, line), egui::StrokeKind::Inside);
    painter.galley(
        egui::pos2(body.min.x + 11.0, body.center().y - galley.size().y / 2.0).round(),
        galley,
        Color32::PLACEHOLDER,
    );
    ui.add_space(1.0);
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

/// The banner of the first page, a sticker in the accent color: pulsing rings around the mark, the
/// name in the brand font (the lettering of the logo), the version, and candy stickers that bob
/// beside the name when the window is wide enough for them.
fn hero(ui: &mut egui::Ui, accent: Color32, height: f32) {
    let ctx = ui.ctx().clone();
    let (full, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter().with_clip_rect(full);
    let time = clock(&ctx);
    ctx.request_repaint_after(Duration::from_millis(33));

    // the banner leaves room on its right and below for its hard shadow
    let rect = egui::Rect::from_min_max(full.min, full.max - Vec2::splat(HARD));
    let ink = on_accent(accent);
    sticker(&painter, rect, panel_r(), accent);
    // a faint copy of the mark bleeds off the right edge, inside the banner
    let banner = painter.with_clip_rect(rect.shrink(1.0));
    let ghost = egui::Rect::from_center_size(
        egui::pos2(rect.max.x - 56.0, rect.center().y + 8.0),
        Vec2::splat(rect.height() * 1.9),
    );
    brand::paint(&banner, brand::Art::Mark, ghost, wash(ink, 0.08));

    // rings that leave the mark and fade, three at a time
    let c = egui::pos2(rect.min.x + 40.0 + 62.0, rect.center().y);
    let r0 = (rect.height() * 0.5 - 12.0).min(54.0);
    for k in 0..3 {
        let phase = (time * 0.45 + k as f32 / 3.0).fract();
        let r = r0 + phase * 20.0;
        let a = (1.0 - phase).powi(2) * 0.6;
        banner.circle_stroke(c, r, Stroke::new(2.0, wash(ink, a)));
    }
    sticker_disc(&banner, c, r0 - 2.0, pal().card, 3.0);
    draw_logo(&banner, c, r0 * 0.58);

    // the words are one block, stacked with real gaps and centered on the banner; on a short
    // banner (a small screen) the type and the gaps shrink together
    let k = (rect.height() / 190.0).clamp(0.72, 1.0);
    // the pulsing rings reach r0 + 20, so the words start clear of them
    let x = c.x + r0 + 20.0 + 36.0;
    let line = pal().line;
    let pink = candy(0);
    let kicker = banner.layout_no_wrap(
        "WELCOME TO".to_owned(),
        egui::FontId::new(10.5, fonts::semibold()),
        on_accent(pink),
    );
    let name = banner.layout_no_wrap(
        APP_NAME.to_owned(),
        egui::FontId::new(52.0 * k, fonts::brand()),
        ink,
    );
    let tagline = banner.layout_no_wrap(
        TAGLINE.to_owned(),
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ink,
    );
    let version = format!("Version {VERSION}");
    let version_g = banner.layout_no_wrap(version, egui::FontId::new(12.0, fonts::semibold()), ink);
    let pill_h = 20.0;
    let badge_h = version_g.size().y + 10.0;
    let gaps = [10.0 * k, 6.0 * k, 16.0 * k];
    let block_h = pill_h + gaps[0] + name.size().y + gaps[1] + tagline.size().y + gaps[2] + badge_h;
    let mut y = (rect.center().y - block_h / 2.0).round();

    // "WELCOME TO" on a pink sticker
    let pill =
        egui::Rect::from_min_size(egui::pos2(x, y), Vec2::new(kicker.size().x + 22.0, pill_h));
    banner.rect_filled(pill.translate(Vec2::splat(2.0)), PILL, line);
    banner.rect_filled(pill, PILL, pink);
    banner.rect_stroke(pill, PILL, Stroke::new(1.5, line), egui::StrokeKind::Inside);
    banner.galley(
        egui::pos2(pill.min.x + 11.0, pill.center().y - kicker.size().y / 2.0).round(),
        kicker,
        Color32::PLACEHOLDER,
    );
    y += pill_h + gaps[0];

    // the name (the brand font sits a little to the right of its box, so pull it back)
    let name_h = name.size().y;
    banner.galley(egui::pos2(x - 2.0, y).round(), name, Color32::PLACEHOLDER);
    y += name_h + gaps[1];

    let tagline_h = tagline.size().y;
    banner.galley(egui::pos2(x, y).round(), tagline, Color32::PLACEHOLDER);
    y += tagline_h + gaps[2];

    // the version, on a faint pill of its own
    let badge = egui::Rect::from_min_size(
        egui::pos2(x, y.round()),
        Vec2::new(version_g.size().x + 24.0, badge_h),
    );
    banner.rect_filled(badge, PILL, wash(ink, 0.12));
    banner.galley(
        egui::pos2(
            badge.min.x + 12.0,
            badge.center().y - version_g.size().y / 2.0,
        )
        .round(),
        version_g,
        Color32::PLACEHOLDER,
    );

    // stickers that bob beside the name, only where there is room for them
    if rect.width() >= 700.0 {
        let bob = |phase: f32| (time * 1.6 + phase).sin() * 3.0;
        sticker_disc(
            &banner,
            egui::pos2(rect.max.x - 132.0, rect.min.y + 34.0 + bob(0.0)),
            13.0,
            candy(0),
            2.0,
        );
        sticker_disc(
            &banner,
            egui::pos2(rect.max.x - 88.0, rect.max.y - 36.0 + bob(2.1)),
            9.0,
            candy(2),
            2.0,
        );
        let square = egui::Rect::from_center_size(
            egui::pos2(rect.max.x - 50.0, rect.min.y + 52.0 + bob(4.2)),
            Vec2::splat(22.0),
        );
        sticker(&banner, square, egui::CornerRadius::same(7), candy(1));
    }
}

/// The "Step n of m" text of the sticker above a heading. The first page is the welcome and is
/// not a step.
fn step_label(n: usize) -> String {
    format!("Step {n} of {}", PAGES - 1)
}

/// Paints `text` wrapped to `width`, its top-left corner at `at`; returns the height it takes.
fn paint_text(
    painter: &egui::Painter,
    at: egui::Pos2,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
) -> f32 {
    let galley = painter.layout(text.to_owned(), font, color, width.max(40.0));
    let height = galley.size().y;
    painter.galley(at.round(), galley, Color32::PLACEHOLDER);
    height
}

/// Remembers how far content reached below its card: the test of the pages reads it.
fn note_overflow(ctx: &egui::Context, over: f32) {
    ctx.data_mut(|d| {
        let id = egui::Id::new("welcome_card_overflow");
        let worst: f32 = d.get_temp(id).unwrap_or(0.0);
        d.insert_temp(id, worst.max(over));
    });
}

/// Ease-out curve of `x` in `0..=1`.
fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// The icon of a persona card.
fn persona_icon(persona: Persona) -> char {
    match persona {
        Persona::Everyday => symbols::SPARKLE,
        Persona::Student => symbols::BOOK_OPEN,
        Persona::Developer => symbols::CODE,
        Persona::Designer => symbols::PAINT_BRUSH,
        Persona::Writer => symbols::NOTE_PENCIL,
        Persona::Creator => symbols::VIDEO_CAMERA,
    }
}

/// Adds `persona` to the picks, or takes it out when it is already there.
fn toggle_persona(picks: &mut Vec<Persona>, persona: Persona) {
    if let Some(at) = picks.iter().position(|p| *p == persona) {
        picks.remove(at);
    } else {
        picks.push(persona);
    }
}

/// A small pie made of round stickers: a disc with one bubble per slice around it. `open` (0..1)
/// is how far it has grown out of the pointer, `hot` the slice under the pointer. It is a sketch
/// of the real pie, drawn the same way on every page that shows one.
fn bubble_pie(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    icons: &[char],
    open: f32,
    hot: Option<usize>,
    accent: Color32,
) {
    let open = open.clamp(0.0, 1.0);
    if open <= 0.02 {
        return;
    }
    let n = icons.len().max(1);
    sticker_disc(painter, center, radius * open, pal().card, 3.0);
    let ring = radius * 0.64;
    let chord = 2.0 * ring * (std::f32::consts::PI / n as f32).sin();
    let bubble = (radius * 0.24).min(chord * 0.5 * 0.84);
    for (i, glyph) in icons.iter().enumerate() {
        let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
        let on = hot == Some(i);
        let at = center + Vec2::angled(angle) * ring * open;
        let r = bubble * open * if on { 1.16 } else { 1.0 };
        let fill = if on {
            accent
        } else {
            pal().card.lerp_to_gamma(candy(i), 0.38)
        };
        sticker_disc(painter, at, r, fill, 2.0);
        if open > 0.5 {
            fonts::paint_icon(painter, at, *glyph, r * 1.1, on_accent(fill), true);
        }
    }
    painter.circle_filled(center, (radius * 0.06).max(2.5) * open, pal().line);
}

/// Page 0: what CursorPie is, in a banner and three short promises.
fn intro_page(ui: &mut egui::Ui, accent: Color32) {
    const TILE_H: f32 = 124.0;
    const BETWEEN: f32 = 40.0;
    let area = ui.available_rect_before_wrap();
    let hero_h = (area.height() - TILE_H - BETWEEN - HARD).clamp(120.0, 250.0);
    let used = hero_h + BETWEEN + TILE_H;
    // what the page does not need is air above the banner (a third of it) and below the tiles
    let lead = ((area.height() - used) / 3.0).max(0.0).floor();
    let hero_rect = egui::Rect::from_min_size(
        egui::pos2(area.min.x, area.min.y + lead),
        Vec2::new(area.width(), hero_h),
    );
    let tiles_rect = egui::Rect::from_min_size(
        egui::pos2(area.min.x, hero_rect.max.y + BETWEEN),
        Vec2::new(area.width(), TILE_H),
    );
    // each piece is laid out in a rectangle of its own: the cursor of the page is not trusted
    ui.scope_builder(egui::UiBuilder::new().max_rect(hero_rect), |ui| {
        hero(ui, accent, hero_h);
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(tiles_rect), |ui| {
        feature_tiles(ui, accent, TILE_H);
    });
}

/// Three tiles under the banner: a round icon and a few words each. The tiles are as far apart as
/// the hard shadow and some air need, and their text is clipped to them (see `note_overflow`).
fn feature_tiles(ui: &mut egui::Ui, accent: Color32, height: f32) {
    const GAP_X: f32 = 28.0;
    let ctx = ui.ctx().clone();
    let tiles = [
        (
            symbols::CURSOR_CLICK,
            "Instant",
            "Opens under your pointer, in any app.",
        ),
        (symbols::SPARKLE, "Personal", "Shaped around how you work."),
        (
            symbols::SQUARES_FOUR,
            "Per app",
            "Each app can have its own pie.",
        ),
    ];
    let tones = [accent, candy(0), candy(1)];
    let (area, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let width = ((area.width() - 2.0 * GAP_X) / 3.0).floor();
    let painter = ui.painter().clone();
    for (i, (icon, title, text)) in tiles.into_iter().enumerate() {
        let full = egui::Rect::from_min_size(
            egui::pos2(area.min.x + i as f32 * (width + GAP_X), area.min.y),
            Vec2::new(width, height),
        );
        let r = egui::Rect::from_min_max(full.min, full.max - Vec2::splat(HARD));
        sticker(&painter, r, panel_r(), pal().card);
        let tone = tones[i];
        let c = egui::pos2(r.min.x + 22.0 + 21.0, r.center().y);
        sticker_disc(&painter, c, 21.0, tone, 2.0);
        fonts::paint_icon(&painter, c, icon, 24.0, on_accent(tone), true);
        let x = c.x + 21.0 + 18.0;
        let w = r.max.x - x - 20.0;
        // measure first, then center the title and its text as one block beside the icon
        let tg = painter.layout(
            title.to_owned(),
            egui::FontId::new(15.0, fonts::semibold()),
            pal().text,
            w.max(40.0),
        );
        let bg = painter.layout(
            text.to_owned(),
            egui::FontId::proportional(12.5),
            pal().muted,
            w.max(40.0),
        );
        let block = tg.size().y + 6.0 + bg.size().y;
        let top = (r.center().y - block / 2.0).round();
        let bottom = top + block;
        painter.galley(egui::pos2(x, top).round(), tg.clone(), Color32::PLACEHOLDER);
        painter.galley(
            egui::pos2(x, top + tg.size().y + 6.0).round(),
            bg,
            Color32::PLACEHOLDER,
        );
        note_overflow(&ctx, (bottom - (r.max.y - 12.0)).max(0.0));
    }
}

/// Page 1: the gesture, as three rows that light up one after the other next to a small pie that
/// does what they say.
fn how_page(ui: &mut egui::Ui, accent: Color32) {
    const CYCLE: f32 = 5.1;
    let ctx = ui.ctx().clone();
    eyebrow(ui, &step_label(1), accent);
    heading(ui, "Three moves open anything");
    ui.add_space(GAP / 2.0);
    ctx.request_repaint_after(Duration::from_millis(33));
    let phase = (clock(&ctx) % CYCLE) / CYCLE;
    let stage = ((phase * 3.0) as usize).min(2);
    let local = (phase * 3.0).fract();

    let steps = gesture_steps(cfg().pie_hold_ms);
    let icons = [
        symbols::CURSOR_CLICK,
        symbols::ARROWS_OUT,
        symbols::CHECK_CIRCLE,
    ];
    let tones = [accent, candy(0), candy(1)];
    let (left, right) = two_columns(ui, 0.46);

    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        let width = ui.available_width();
        // the three rows share the height of the stage next to them; the room between two rows is
        // the gap plus the spacing egui puts on both sides of it, which the share has to count or
        // the column ends a few points below the stage (and out of the card on the smallest one)
        let row_gap = 6.0;
        let between = row_gap + 2.0 * ui.spacing().item_spacing.y;
        let share = ((right.height().clamp(200.0, 330.0) - 2.0 * between) / 3.0).max(58.0);
        for (i, (label, text)) in steps.iter().enumerate() {
            if i > 0 {
                ui.add_space(row_gap);
            }
            let on =
                ctx.animate_bool_with_time(egui::Id::new(("welcome_how_on", i)), i == stage, 0.3);
            let galley = ui.painter().layout(
                text.clone(),
                egui::FontId::proportional(13.0),
                pal().muted,
                (width - 70.0).max(80.0),
            );
            let h = share.max(10.0 + 22.0 + galley.size().y + 12.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, h), egui::Sense::hover());
            let p = ui.painter().clone();
            p.rect_filled(rect, panel_r(), wash(tones[i], 0.16 * on));
            p.rect_stroke(
                rect,
                panel_r(),
                Stroke::new(1.5, wash(pal().line, 0.55 * on)),
                egui::StrokeKind::Inside,
            );
            let block = 22.0 + galley.size().y;
            let top = rect.min.y + ((rect.height() - block) / 2.0).max(8.0);
            let c = egui::pos2(rect.min.x + 12.0 + 19.0, top + 19.0);
            sticker_disc(&p, c, 19.0, tones[i], 2.0);
            fonts::paint_icon(&p, c, icons[i], 22.0, on_accent(tones[i]), true);
            let x = rect.min.x + 12.0 + 38.0 + 12.0;
            paint_text(
                &p,
                egui::pos2(x, top),
                label,
                egui::FontId::new(16.0, fonts::semibold()),
                pal().text,
                width - 70.0,
            );
            paint_text(
                &p,
                egui::pos2(x, top + 22.0),
                text,
                egui::FontId::proportional(13.0),
                pal().muted,
                width - 70.0,
            );
        }
    });

    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        let h = right.height().clamp(200.0, 330.0);
        let (full, _) = ui.allocate_exact_size(Vec2::new(right.width(), h), egui::Sense::hover());
        let p = ui.painter().clone();
        let r = egui::Rect::from_min_max(full.min, full.max - Vec2::splat(HARD));
        sticker(&p, r, panel_r(), pal().card);
        let inner = p.with_clip_rect(r.shrink(2.0));

        let radius = ((r.height() - 64.0) / 2.0).min(r.width() * 0.42).max(40.0);
        let c = egui::pos2(r.center().x, r.min.y + 18.0 + radius);
        let demo = [
            symbols::ARROW_U_UP_LEFT,
            symbols::COPY,
            symbols::GLOBE,
            symbols::CROP,
            symbols::FOLDER_SIMPLE,
            symbols::ROCKET_LAUNCH,
        ];
        let target = 1usize;
        let angle = -std::f32::consts::FRAC_PI_2
            + target as f32 * std::f32::consts::TAU / demo.len() as f32;
        let target_at = c + Vec2::angled(angle) * radius * 0.64;
        let (open, hot, cursor) = match stage {
            0 => (0.0, None, c),
            1 => (
                ease_out(local * 1.6),
                (local > 0.65).then_some(target),
                c.lerp(target_at, ease_out(local)),
            ),
            _ => (1.0, Some(target), target_at),
        };
        if stage == 0 {
            // the hold: a ring fills around the pointer
            inner.circle_stroke(c, radius * 0.64, Stroke::new(1.5, wash(pal().line, 0.12)));
            inner.circle_stroke(c, 18.0, Stroke::new(1.5, wash(pal().line, 0.25)));
            let n = 40usize;
            let upto = (n as f32 * local) as usize;
            let points: Vec<egui::Pos2> = (0..=upto)
                .map(|k| {
                    let a =
                        -std::f32::consts::FRAC_PI_2 + k as f32 / n as f32 * std::f32::consts::TAU;
                    c + Vec2::angled(a) * 18.0
                })
                .collect();
            if points.len() > 1 {
                inner.add(egui::Shape::line(points, Stroke::new(3.5, accent)));
            }
        }
        bubble_pie(&inner, c, radius, &demo, open, hot, accent);
        if stage == 2 {
            // the release: a ripple leaves the slice that ran
            inner.circle_stroke(
                target_at,
                radius * 0.24 * (1.2 + local),
                Stroke::new(2.0, wash(accent, 1.0 - local)),
            );
        }
        inner.circle_filled(cursor, 6.0, pal().line);
        inner.circle_filled(cursor, 3.5, Color32::WHITE);
        let caption = [
            "Hold the button",
            "Flick toward a slice",
            "Release to run it",
        ][stage];
        inner.text(
            egui::pos2(r.center().x, r.max.y - 22.0),
            egui::Align2::CENTER_CENTER,
            caption,
            egui::FontId::new(15.0, fonts::semibold()),
            pal().text,
        );
    });
}

/// Page 2: "What do you do most?", six cards to pick from (one or several). The pick shapes the
/// pie of the next page and is written to the config when the guide ends with "Get started".
fn persona_page(ui: &mut egui::Ui, w: &mut Welcome, accent: Color32) {
    const GAP_X: f32 = 24.0;
    const GAP_Y: f32 = 22.0;
    let ctx = ui.ctx().clone();
    eyebrow(ui, &step_label(2), accent);
    heading(ui, "What do you do most?");
    muted(
        ui,
        "We shape your first pie around it. Pick one or several.",
    );
    ui.add_space(GAP + 8.0);

    // below the cards: their shadow, the gap, the hint row and the spacing between them
    let below = HARD + 14.0 + 18.0 + 2.0 * ui.spacing().item_spacing.y + 8.0;
    let card_h = ((ui.available_height() - GAP_Y - below) / 2.0).clamp(88.0, 150.0);
    let (area, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), card_h * 2.0 + GAP_Y),
        egui::Sense::hover(),
    );
    let width = ((area.width() - 2.0 * GAP_X) / 3.0).floor();
    let painter = ui.painter().clone();
    for (i, persona) in Persona::ALL.into_iter().enumerate() {
        let (row, col) = (i / 3, i % 3);
        let full = egui::Rect::from_min_size(
            egui::pos2(
                area.min.x + col as f32 * (width + GAP_X),
                area.min.y + row as f32 * (card_h + GAP_Y),
            ),
            Vec2::new(width, card_h),
        );
        let resp = ui
            .interact(
                full,
                egui::Id::new(("welcome_persona", i)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let picked = w.picks.contains(&persona);
        let on = ctx.animate_bool_with_time(egui::Id::new(("welcome_persona_on", i)), picked, 0.2);
        let hover = ctx.animate_bool_with_time(
            egui::Id::new(("welcome_persona_hover", i)),
            resp.hovered(),
            0.15,
        );
        let r = egui::Rect::from_min_max(full.min, full.max - Vec2::splat(HARD))
            .translate(Vec2::new(0.0, -2.0 * hover));
        let tone = candy(i);
        sticker(
            &painter,
            r,
            panel_r(),
            pal().card.lerp_to_gamma(tone, 0.34 * on),
        );
        let blurb = painter.layout(
            persona.blurb().to_owned(),
            egui::FontId::proportional(13.0),
            pal().muted,
            r.width() - 36.0,
        );
        let block = 40.0 + 6.0 + blurb.size().y;
        let top = r.min.y + ((r.height() - block) / 2.0).max(4.0);
        let c = egui::pos2(r.min.x + 18.0 + 20.0, top + 20.0);
        sticker_disc(&painter, c, 20.0, tone, 2.0);
        fonts::paint_icon(
            &painter,
            c,
            persona_icon(persona),
            22.0,
            on_accent(tone),
            true,
        );
        painter.text(
            egui::pos2(c.x + 20.0 + 12.0, c.y),
            egui::Align2::LEFT_CENTER,
            persona.title(),
            egui::FontId::new(15.0, fonts::semibold()),
            pal().text,
        );
        let check = egui::pos2(r.max.x - 24.0, c.y);
        sticker_disc(
            &painter,
            check,
            10.0,
            if picked { accent } else { pal().card },
            1.5,
        );
        if on > 0.5 {
            fonts::paint_icon(
                &painter,
                check,
                symbols::CHECK,
                12.0,
                on_accent(accent),
                true,
            );
        }
        let text_top = top + 40.0 + 6.0;
        let bh = blurb.size().y;
        painter.galley(
            egui::pos2(r.min.x + 18.0, text_top).round(),
            blurb,
            Color32::PLACEHOLDER,
        );
        note_overflow(&ctx, (text_top + bh - (r.max.y - 8.0)).max(0.0));
        if resp.clicked() {
            toggle_persona(&mut w.picks, persona);
        }
    }

    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().interact_size.y = 18.0;
        muted(ui, "Not sure? Skip ahead and you get the standard pie.");
        if !w.picks.is_empty() {
            ui.add_space(8.0);
            badge(ui, &format!("{} selected", w.picks.len()), pal().text);
        }
    });
}

/// Page 3: the pie the pick made, sketched on the left and listed on the right while each slice
/// takes its turn to light up.
fn pie_page(ui: &mut egui::Ui, w: &Welcome, accent: Color32, os: Os) {
    let ctx = ui.ctx().clone();
    eyebrow(ui, &step_label(3), accent);
    heading(
        ui,
        if w.picks.is_empty() {
            "The standard pie"
        } else {
            "Your pie is ready"
        },
    );
    ui.add_space(GAP / 2.0);
    let items = persona::pie_for(os, &w.picks);
    let n = items.len().max(1);
    ctx.request_repaint_after(Duration::from_millis(33));
    let hot = ((clock(&ctx) / 1.3) as usize) % n;
    let h = (ui.available_height() - 22.0 - GAP).clamp(150.0, 300.0);
    let (left, right) = two_columns(ui, 0.5);

    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        let (full, _) = ui.allocate_exact_size(Vec2::new(left.width(), h), egui::Sense::hover());
        let p = ui.painter().clone();
        let r = egui::Rect::from_min_max(full.min, full.max - Vec2::splat(HARD));
        sticker(&p, r, panel_r(), pal().card);
        let inner = p.with_clip_rect(r.shrink(2.0));
        let radius = ((r.height() - 56.0) / 2.0).min(r.width() * 0.40).max(40.0);
        let c = egui::pos2(r.center().x, r.min.y + 16.0 + radius);
        let icons: Vec<char> = items
            .iter()
            .map(|i| symbols::slice_icon(&i.icon).unwrap_or(symbols::CHART_PIE_SLICE))
            .collect();
        bubble_pie(&inner, c, radius, &icons, 1.0, Some(hot), accent);
        inner.text(
            egui::pos2(r.center().x, r.max.y - 20.0),
            egui::Align2::CENTER_CENTER,
            items.get(hot).map_or("", |i| i.name.as_str()),
            egui::FontId::new(15.0, fonts::semibold()),
            pal().text,
        );
    });

    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        let (full, _) = ui.allocate_exact_size(Vec2::new(right.width(), h), egui::Sense::hover());
        let p = ui.painter().clone();
        let r = egui::Rect::from_min_max(full.min, full.max - Vec2::splat(HARD));
        sticker(&p, r, panel_r(), pal().card);
        let inner = p.with_clip_rect(r.shrink(2.0));
        let row_h = ((r.height() - 28.0) / n as f32).clamp(20.0, 34.0);
        let first = r.min.y + ((r.height() - row_h * n as f32) / 2.0).max(14.0);
        for (i, item) in items.iter().enumerate() {
            let on = i == hot;
            let row = egui::Rect::from_min_size(
                egui::pos2(r.min.x + 12.0, first + i as f32 * row_h),
                Vec2::new(r.width() - 24.0, row_h),
            );
            if on {
                inner.rect_filled(row, PILL, wash(accent, 0.22));
            }
            let dot = egui::pos2(row.min.x + 14.0, row.center().y);
            sticker_disc(&inner, dot, 7.0, candy(i), 1.0);
            let font = if on {
                egui::FontId::new(14.0, fonts::semibold())
            } else {
                egui::FontId::proportional(14.0)
            };
            inner.text(
                egui::pos2(row.min.x + 30.0, row.center().y),
                egui::Align2::LEFT_CENTER,
                &item.name,
                font,
                pal().text,
            );
            if matches!(item.kind, crate::config::ItemKind::Submenu(_)) {
                inner.text(
                    egui::pos2(row.max.x - 12.0, row.center().y),
                    egui::Align2::RIGHT_CENTER,
                    "submenu",
                    egui::FontId::proportional(12.0),
                    pal().muted,
                );
            }
        }
    });

    ui.add_space(GAP);
    let extra: Vec<String> = persona::app_profiles(os, &w.picks)
        .into_iter()
        .map(|p| p.name)
        .collect();
    if extra.is_empty() {
        muted(ui, "You can change every slice later in Settings.");
    } else {
        muted(
            ui,
            &format!(
                "Also added pies for: {}. Change anything in Settings.",
                extra.join(", ")
            ),
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
            ui.add_space(12.0);
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
        // a candy sticker with an outline (the accent for a plain dot), the number in ink
        let tone = if numbered { candy(i) } else { accent };
        painter.circle_filled(c, BADGE / 2.0 - 1.0, tone);
        painter.circle_stroke(c, BADGE / 2.0 - 1.0, Stroke::new(1.5, pal().line));
        if numbered {
            painter.text(
                c,
                egui::Align2::CENTER_CENTER,
                (i + 1).to_string(),
                egui::FontId::new(12.0, fonts::semibold()),
                on_accent(tone),
            );
        } else {
            painter.circle_filled(c, 3.0, on_accent(tone));
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

/// Page 4: the permission (macOS) or the tray (Windows), with the live status.
fn setup_page(ui: &mut egui::Ui, accent: Color32, os: Os) {
    eyebrow(ui, &step_label(4), accent);
    heading(ui, setup_title(os));

    let running = TAP_ACTIVE.load(Ordering::SeqCst);
    // Linux in a Wayland session: the permission is the udev rule, and the compositor's adapter is a second
    // line of the status (`wayland::report` decides, the same text as in Settings > General)
    let wayland = os == Os::Linux && wayland_session();
    let wl = if wayland {
        Some(crate::platform::linux::wayland::cached_report())
    } else {
        None
    };
    let ready = if os.needs_permission() {
        platform::is_trusted_cached() && running
    } else if let Some(r) = &wl {
        r.ready && running
    } else {
        running
    };
    // the explanation on the left, the live status on the right: nothing is left empty or cut off
    let (left, right) = two_columns(ui, 0.5);
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        ui.add_space(9.0);
        if wayland {
            muted(ui, wayland_setup_lead());
            ui.add_space(GAP - 2.0);
            step_list(ui, accent, wayland_setup_steps(), true);
        } else {
            muted(ui, setup_lead(os));
            ui.add_space(GAP - 2.0);
            step_list(ui, accent, setup_steps(os), os.needs_permission());
        }
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
            if let Some(r) = &wl {
                let text = if r.ready { "Allowed" } else { "Missing" };
                let hint = if r.ready {
                    ""
                } else {
                    "See the steps on the left"
                };
                let w = status_width(ui, text) + 2.0;
                row_wide(ui, "Mouse access", hint, w, |ui| status(ui, r.ready, text));
            }
            let name = if os.needs_permission() {
                "Event tap"
            } else if wayland {
                "Mouse capture"
            } else {
                "Mouse hook"
            };
            let text = if running { "Running" } else { "Not running" };
            // the pill says the state; the way out goes in small print under the name
            let hint = if running { "" } else { "Retrying every 2 s" };
            let w = status_width(ui, text) + 2.0;
            row_wide(ui, name, hint, w, |ui| status(ui, running, text));
            if let Some(r) = &wl
                && r.ready
            {
                // the pie still works without the adapter, but not per-app profiles: say so, do not fail
                let text = if r.per_app { "Running" } else { "Not running" };
                let hint = if r.per_app {
                    ""
                } else {
                    "Per-app profiles are off"
                };
                let w = status_width(ui, text) + 2.0;
                let label = crate::platform::linux::wayland::adapter_label();
                row_wide(ui, label, hint, w, |ui| status(ui, r.per_app, text));
            }
        });
        if ready {
            ui.horizontal(|ui| {
                status(ui, true, "Ready");
                muted(ui, "CursorPie can see your mouse.");
            });
        } else if wayland || !setup_note(os).is_empty() {
            let note = if wayland {
                wayland_setup_note()
            } else {
                setup_note(os)
            };
            ui.add(egui::Label::new(
                egui::RichText::new(note).size(13.0).color(pal().muted),
            ));
        }
    });
}

/// Page 5: tips and the launch-at-login box. Returns true when "Open Settings" was clicked.
fn ready_page(ui: &mut egui::Ui, w: &mut Welcome, accent: Color32, os: Os) -> bool {
    eyebrow(ui, &step_label(5), accent);
    ui.horizontal(|ui| {
        ui.label(fonts::title("You're all set", 28.0, pal().text));
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::hover());
        sticker_disc(ui.painter(), rect.center(), 14.0, accent, 2.0);
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
    let (hold, by_hold, by_space, presses) = {
        let c = cfg();
        (
            c.settings_hold_ms,
            c.settings_by_hold,
            c.settings_by_space,
            c.settings_space_presses,
        )
    };
    let mut open_settings = false;
    // the tips on the left, the startup choice and the way into Settings on the right
    let (left, right) = two_columns(ui, 0.56);
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        panel(ui, "Good to know", |ui| {
            let mut list = tips(os, hold);
            // the first tip is the way into Settings: it follows what is switched on
            match (by_hold, by_space) {
                (true, _) => {}
                (false, true) => {
                    let times = if presses >= 2 { "twice" } else { "once" };
                    list[0] =
                        format!("Press Space {times} while the pie is open to open Settings.");
                }
                (false, false) => {
                    list.remove(0);
                }
            }
            for tip in list {
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
        ui.add_space(GAP + 4.0);
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

    #[test]
    fn the_wayland_setup_page_names_the_udev_rule_and_is_short() {
        let steps = wayland_setup_steps();
        assert_eq!(steps.len(), 3);
        // the same file name as the report in Settings > General and the package
        assert!(
            steps[0].contains(crate::platform::linux::wayland::UDEV_RULE_FILE),
            "{steps:?}"
        );
        assert!(steps[1].contains("udevadm"), "{steps:?}");
        assert!(steps[2].contains("tray icon"), "{steps:?}");
        assert!(wayland_setup_lead().len() < 90);
        assert!(wayland_setup_note().len() < 160);
        for step in steps {
            assert!(step.len() < 160, "{step}");
        }
        // the old advice cannot be followed on Plasma 6.8: no page may still send people to X11
        for os_step in setup_steps(Os::Linux) {
            assert!(!os_step.contains("log in with an X11"), "{os_step}");
        }
    }

    /// Draws page `page` into `ui`, for a welcome in the state `w`.
    fn draw_page(ui: &mut egui::Ui, page: usize, w: &mut Welcome, accent: egui::Color32, os: Os) {
        match page {
            0 => intro_page(ui, accent),
            1 => how_page(ui, accent),
            2 => persona_page(ui, w, accent),
            3 => pie_page(ui, w, accent, os),
            4 => setup_page(ui, accent, os),
            _ => {
                ready_page(ui, w, accent, os);
            }
        }
    }

    /// Draws every page for a few frames in a headless egui context: a smoke test for layout
    /// panics (zero widths, bad column counts) that no other test would reach.
    #[test]
    fn every_page_draws_without_panicking() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let accent = egui::Color32::from_rgb(64, 156, 255);
        let mut w = Welcome::new(false);
        for picks in [Vec::new(), Persona::ALL.to_vec()] {
            w.picks = picks;
            for page in 0..PAGES {
                for _ in 0..3 {
                    let input = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(760.0, 520.0),
                        )),
                        ..Default::default()
                    };
                    let mut output = ctx.run_ui(input, |ui| {
                        draw_page(ui, page, &mut w, accent, platform::OS)
                    });
                    output.textures_delta.clear(); // nothing uploads the textures here
                }
            }
        }
    }

    /// The body of the card, as `welcome_ui` cuts it, for a card of `card` points.
    fn body_size(card: [f32; 2]) -> Vec2 {
        Vec2::new(
            card[0] - 2.0 * PAD - BODY_ROOM,
            card[1] - TOP_H - GAP - FOOT_H + GAP / 2.0,
        )
    }

    /// Every OS variant must fit both card sizes, with nothing picked and with everything picked,
    /// regardless of the host running the test.
    #[test]
    fn every_page_fits_the_body_of_the_card() {
        use crate::ui::app::{WELCOME_MIN, WELCOME_SIZE};
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let accent = egui::Color32::from_rgb(90, 120, 94);
        let mut w = Welcome::new(false);

        for os in [Os::Mac, Os::Windows, Os::Linux] {
            for picks in [Vec::new(), Persona::ALL.to_vec()] {
                w.picks = picks;
                for card in [WELCOME_SIZE, WELCOME_MIN] {
                    let body = body_size(card);
                    for page in 0..PAGES {
                        let mut used = 0.0;
                        ctx.data_mut(|d| {
                            d.insert_temp(egui::Id::new("welcome_card_overflow"), 0.0_f32)
                        });
                        for _ in 0..3 {
                            let input = egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    body,
                                )),
                                ..Default::default()
                            };
                            let mut output = ctx.run_ui(input, |ui| {
                                draw_page(ui, page, &mut w, accent, os);
                                used = ui.min_rect().height();
                            });
                            output.textures_delta.clear();
                        }
                        let over: f32 = ctx
                            .data(|d| d.get_temp(egui::Id::new("welcome_card_overflow")))
                            .unwrap_or(0.0);
                        assert!(
                            over <= 0.5,
                            "page {page} on {os:?}: card text reaches {over} points out of its card ({card:?})"
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
    }

    #[test]
    fn ink_reads_on_every_candy_color_of_the_stickers() {
        for i in 0..CANDY.len() {
            let c = candy(i);
            assert!(contrast(on_accent(c), c) >= 4.0, "candy {i}");
        }
        assert_eq!(candy(CANDY.len()), candy(0), "the colors wrap around");
        // the hard shadow of a sticker has to stay inside the gap between two cards of the page
        const { assert!(HARD < 22.0 / 2.0) };
    }

    #[test]
    fn a_new_welcome_starts_on_the_first_page_with_nothing_picked() {
        let w = Welcome::new(true);
        assert_eq!((w.page, w.launch_at_login, w.apply), (0, true, false));
        assert!(w.picks.is_empty());
    }

    #[test]
    fn picking_a_card_twice_takes_it_back() {
        let mut picks = Vec::new();
        toggle_persona(&mut picks, Persona::Designer);
        toggle_persona(&mut picks, Persona::Writer);
        assert_eq!(picks, [Persona::Designer, Persona::Writer]);
        toggle_persona(&mut picks, Persona::Designer);
        assert_eq!(picks, [Persona::Writer]);
    }

    #[test]
    fn every_page_has_a_name_and_the_steps_count_the_pages_after_the_first() {
        assert_eq!(PAGE_NAMES.len(), PAGES);
        assert_eq!(
            step_label(PAGES - 1),
            format!("Step {} of {}", PAGES - 1, PAGES - 1)
        );
    }
}
