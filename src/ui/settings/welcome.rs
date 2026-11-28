//! The welcome window: a five-page introduction shown the first time CursorPie starts (and again
//! from the tray item "Welcome guide...").
//!
//! It is drawn in the same native window and with the same card, shadow, colors and widgets as
//! Settings, so it looks like part of the app and needs no second window. It is deliberately quiet:
//! one idea per page, centered, with few words. Its own pieces (the persona cards, the page dots,
//! the check) are *stickers* in the style of the website (`assets/site.css`): a 2-point outline in
//! the line color of the theme, a hard shadow that falls right and down, and the candy colors of
//! the site. While it is open `App::settings_open` is true (so the pie stays hidden) and
//! `App::welcome` holds its state.
//!
//! ```text
//! +--------------------------------------------------------+
//! |                                                   Skip |  top bar
//! +--------------------------------------------------------+
//! |                                                        |
//! |   page 0  Welcome      (app icon, name, "Get started") |
//! |   page 1  Permissions  (macOS: Accessibility,          |
//! |                         Windows: the system tray, ...) |
//! |   page 2  About you    (what do you do most?)          |
//! |   page 3  Shortcut     (the key combination; required) |
//! |   page 4  Done         (launch at login, intelligent pie) |
//! |                                                        |
//! +--------------------------------------------------------+
//! | Back                   o o o o                   Next  |  footer
//! +--------------------------------------------------------+
//! ```
//!
//! The pick of page 2 is kept in [`Welcome::picks`]; it is written into the config only when the
//! guide ends with "Done" (see `App::finish_welcome` and [`crate::config::persona::apply`]), so Skip and Esc leave
//! the pie as it was.
//!
//! Page 3 asks for the key combination that opens the pie ([`crate::tap`]'s keyboard trigger) and
//! **cannot be passed without one** (except where the system does not allow the keyboard trigger, a
//! Wayland session: see [`key_required`]). It listens as soon as it is shown and writes what is
//! pressed as it is pressed: the modifiers held show at once as keycaps, and the key that ends the
//! combination completes it. The combination is kept in [`Welcome::key`] and written into the config
//! only with "Done", like the picks.
//!
//! Below the combination sits an optional second one that opens the pie of the app in front
//! ([`Welcome::key_app`]). Each combination is a roomy box with no buttons: click it and press the
//! keys; the box that takes them gets an accent edge. The intelligent pie box on the last
//! page starts from `smart.enabled`, so it shows what Settings already has.
//!
//! The texts that differ between macOS, Windows and Linux are small pure functions of [`Os`], so a
//! test can check all of them without running on all.

use super::brand;
use super::theme::*;
use super::widgets::*;
use crate::about::*;
use crate::config::cfg;
use crate::config::persona::Persona;
use crate::platform::{self, Os};
use crate::tap::TAP_ACTIVE;
use crate::ui::app::App;
use crate::ui::draw::*;
use crate::ui::fonts;
use crate::ui::mascot::{self, Pose, Style};
use crate::ui::settings::state::Slot;
use crate::ui::symbols;
use eframe::egui::{self, Color32, Stroke, Vec2};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Number of pages of the welcome window.
pub const PAGES: usize = 5;

/// The page that asks for the key combination.
pub const KEY_PAGE: usize = 3;

/// Room the body leaves at its right (and the clip leaves below it) for the hard shadow of a
/// sticker on the edge, so no shadow is cut off.
const BODY_ROOM: f32 = HARD + 4.0;

/// How big the mascot behind the first page is, as a multiple of the page's shorter side.
const INTRO_MASCOT_SCALE: f32 = 1.22;
/// Where its center is, from the middle of the page, as a fraction of the page's width and height
/// (right and down). Together with [`INTRO_MASCOT_SCALE`] these are the numbers to change to move it.
const INTRO_MASCOT_SHIFT: [f32; 2] = [0.15, 0.31];

/// The mascot of the last page needs this much free space at the side of the toggles to show.
const READY_MASCOT_MIN_MARGIN: f32 = 150.0;
/// Its largest side.
const READY_MASCOT_MAX: f32 = 140.0;

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
    /// The "Intelligent pie" box of the last page. Off by default: learning what the person uses
    /// is theirs to switch on, and it is applied only when the guide ends with "Done".
    pub intelligent: bool,
    /// Set when the window was left with "Get started" or "Open Settings" (not with Skip or Esc):
    /// only then is the launch-at-login box applied.
    pub apply: bool,
    /// Open Settings as soon as the window has closed.
    pub open_settings_after: bool,
    /// What the person said they do most (page 2), in the order they picked it. Empty: nothing was
    /// picked and the pie stays as it is.
    pub picks: Vec<Persona>,
    /// The key combination that opens the pie (page 3). Starts as the one the config already has,
    /// so opening the guide again from the tray does not ask for what is set. Written into the
    /// config only when the guide ends with "Done".
    pub key: Option<crate::keys::Shortcut>,
    /// The optional second combination of page 3: it opens the pie of the app in front (see
    /// [`crate::config::Config::key_trigger_app`]). Starts as the config has it; written with
    /// "Done", like `key`.
    pub key_app: Option<crate::keys::Shortcut>,
    /// The last key pressed on page 3 had no modifier: the page says what it needs.
    pub key_hint: bool,
    /// The last combination pressed for the pie of the app is the one that opens the general pie.
    pub key_dup: bool,
    /// Page 3 has started listening once (so Esc, which cancels a recording, is not undone by
    /// listening again at once).
    pub key_armed: bool,
}

impl Welcome {
    /// A new welcome window on its first page; `launch_at_login` is the current system setting.
    pub fn new(launch_at_login: bool) -> Self {
        // one lock, read once and let go: `cfg()` is a mutex guard, and a guard taken inside the
        // struct below would live to the end of the statement, so a second `cfg()` there would wait
        // for the first one forever (the window froze)
        let (intelligent, key, key_app) = {
            let c = cfg();
            (c.smart.enabled, c.key_trigger, c.key_trigger_app)
        };
        Self {
            page: 0,
            launch_at_login,
            // what Settings says: a person who already switched the intelligent pie on finds the
            // box checked, and a fresh install finds it off
            intelligent,
            apply: false,
            open_settings_after: false,
            picks: Vec::new(),
            key,
            key_app,
            key_hint: false,
            key_dup: false,
            key_armed: false,
        }
    }

    /// A combination was pressed on page 3. Returns whether it was kept: one without a modifier
    /// (a bare key would take typing away) or Esc is not, and the page says what it needs.
    pub fn set_key(&mut self, sc: crate::keys::Shortcut) -> bool {
        self.set_slot_key(false, sc)
    }

    /// Like [`Welcome::set_key`] for the combination of the pie of the app (`key_app`). One that
    /// the general pie already uses is not kept (the general one is required, so it wins).
    pub fn set_app_key(&mut self, sc: crate::keys::Shortcut) -> bool {
        self.set_slot_key(true, sc)
    }

    fn set_slot_key(&mut self, app: bool, sc: crate::keys::Shortcut) -> bool {
        let usable = sc.mods & crate::keys::MOD_MASK != 0 && sc.key != crate::keys::key::ESCAPE;
        self.key_hint = !usable;
        self.key_dup = false;
        if !usable {
            return false;
        }
        let same = |o: Option<crate::keys::Shortcut>| {
            o.is_some_and(|o| (o.key, o.mods) == (sc.key, sc.mods))
        };
        if app {
            if same(self.key) {
                self.key_dup = true;
                return false;
            }
            self.key_app = Some(sc);
        } else {
            if same(self.key_app) {
                self.key_app = None;
            }
            self.key = Some(sc);
        }
        true
    }

    /// Whether the guide may leave `page` forward: the key page needs a combination.
    pub fn can_continue(&self, page: usize) -> bool {
        page != KEY_PAGE || self.key.is_some() || !key_required()
    }
}

/// Whether the key page cannot be passed without a combination: it can everywhere the keyboard
/// trigger works. A Wayland session does not allow a program to read keys while another app has the
/// focus, so there the page only says so.
pub fn key_required() -> bool {
    !crate::platform::linux::wayland::active()
}

/// The names of the modifiers in `mods` in the usual order, in the words of `os`.
pub fn mod_names_in(os: Os, mods: u8) -> Vec<&'static str> {
    use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT};
    let n = os.mod_names();
    [
        (MOD_CTRL, n.ctrl),
        (MOD_ALT, n.alt),
        (MOD_SHIFT, n.shift),
        (MOD_CMD, n.cmd),
    ]
    .into_iter()
    .filter(|(bit, _)| mods & bit != 0)
    .map(|(_, name)| name)
    .collect()
}

/// What the key page shows: the keys of the combination, one keycap each, or the modifiers held so
/// far followed by a gap for the key still to come.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyView {
    /// Nothing is held and nothing is set: "Press your keys".
    Waiting,
    /// Modifiers are held, the key is still to come.
    Holding(Vec<&'static str>),
    /// A combination is set.
    Set(Vec<String>),
}

/// What the key page shows now: `live` are the modifiers held at this moment (only looked at while
/// `listening`), `key` the combination that is set.
pub fn key_view(os: Os, key: Option<&crate::keys::Shortcut>, listening: bool, live: u8) -> KeyView {
    if listening && live != 0 {
        return KeyView::Holding(mod_names_in(os, live));
    }
    match key {
        Some(sc) => KeyView::Set(
            sc.label_for(os)
                .split(" + ")
                .filter(|k| !k.is_empty())
                .map(str::to_owned)
                .collect(),
        ),
        None => KeyView::Waiting,
    }
}

/// What the key page asked for this frame.
pub enum KeyAction {
    None,
    /// Start recording the combination of this slot (or stop, when it is being recorded).
    Record(Slot),
    /// Take the combination of the pie of the app away.
    ClearApp,
}

/// What a button of the window asked for this frame.
enum Step {
    None,
    Next,
    Back,
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

/// Width of the page dots: the current one is a pill of [`DOT_WIDE`], the others discs of [`DOT`],
/// [`DOT_GAP`] apart. It is constant, so the dots stay centered while they animate.
const DOT: f32 = 10.0;
const DOT_WIDE: f32 = 26.0;
const DOT_GAP: f32 = 8.0;

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

        let Some(page) = self.welcome.as_ref().map(|w| w.page) else {
            return;
        };

        // the key page listens while it is on screen, once (see `Welcome::key_armed`), and stops
        // listening when the person leaves it
        let key_slot = Slot::KeyPie;
        let rec_slot: Option<Slot> = self
            .ed
            .recording
            .as_ref()
            .map(|r| r.slot)
            .filter(|s| matches!(s, Slot::KeyPie | Slot::KeyPieApp));
        let listening = rec_slot.is_some();
        if page == KEY_PAGE && key_required() {
            if let Some(w) = self.welcome.as_mut()
                && !w.key_armed
                && w.key.is_none()
                && !listening
                && self.closing.is_none()
            {
                w.key_armed = true;
                self.ed.toggle_key_trigger_rec(key_slot);
            }
        } else if listening {
            self.ed.stop_recording();
        }
        if !listening {
            self.ed.key_rec.reset();
        }
        // Windows and Wayland: the window's own keys carry the combination (see `wayland_keys`),
        // and they are taken away from the widgets, so Enter and Esc do not also act as buttons
        if listening
            && super::wayland_keys::applies()
            && let Some(msg) = super::wayland_keys::capture(&ctx, &mut self.ed.key_rec)
        {
            crate::tap::RECORDING.store(false, Ordering::SeqCst);
            self.ed.key_rec.reset();
            let _ = self.tx.send(msg);
            ctx.request_repaint();
        }

        // Esc closes the guide, unless it is cancelling a recording of the key page
        if self.closing.is_none() && !listening && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.closing = Some(false);
        }

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
        // the first page has the logo in its middle, so the bar carries no second one there
        if page > 0 {
            draw_wordmark(&painter, top.min.x + PAD, top.center().y, 28.0);
        }

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
                    0 => {
                        if intro_page(ui) {
                            step = Step::Next;
                        }
                    }
                    1 => setup_page(ui, accent, platform::OS),
                    2 => {
                        if let Some(w) = self.welcome.as_mut() {
                            persona_page(ui, w, accent);
                        }
                    }
                    KEY_PAGE => {
                        if let Some(w) = self.welcome.as_mut() {
                            let m = ctx.input(|i| i.modifiers);
                            let mut live = self.ed.key_rec.held_bits();
                            for (down, bit) in [
                                (m.ctrl, crate::keys::MOD_CTRL),
                                (m.alt, crate::keys::MOD_ALT),
                                (m.shift, crate::keys::MOD_SHIFT),
                                (m.mac_cmd, crate::keys::MOD_CMD),
                            ] {
                                if down {
                                    live |= bit;
                                }
                            }
                            match key_page(ui, w, accent, platform::OS, rec_slot, live) {
                                KeyAction::None => {}
                                KeyAction::Record(slot) => {
                                    w.key_hint = false;
                                    w.key_dup = false;
                                    self.ed.toggle_key_trigger_rec(slot);
                                }
                                KeyAction::ClearApp => {
                                    self.ed.stop_recording();
                                    w.key_hint = false;
                                    w.key_dup = false;
                                    w.key_app = None;
                                }
                            }
                        }
                    }
                    _ => {
                        if let Some(w) = self.welcome.as_mut() {
                            ready_page(ui, w, accent, platform::OS);
                        }
                    }
                }
            },
        );

        // ---------------------------------------------------------------- footer
        // page dots, outlined like the buttons: the current one stretches into a pill filled with
        // the accent. They sit in the middle of the footer; Back and Next hold its two sides.
        let dots_w = DOT_WIDE + (PAGES as f32 - 1.0) * (DOT + DOT_GAP);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_center_size(
                    foot.center(),
                    Vec2::new(dots_w + 2.0, DOT + 4.0),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = DOT_GAP;
                for i in 0..PAGES {
                    let wide = ctx.animate_value_with_time(
                        egui::Id::new(("welcome_dot", i)),
                        if i == page { DOT_WIDE } else { DOT },
                        0.22,
                    );
                    // the others give up what the current one has grown by
                    let (rect, _) =
                        ui.allocate_exact_size(Vec2::new(wide, DOT), egui::Sense::hover());
                    let color = if i == page { accent } else { pal().card };
                    ui.painter().rect_filled(rect, PILL, color);
                    ui.painter().rect_stroke(
                        rect,
                        PILL,
                        Stroke::new(2.0, pal().line),
                        egui::StrokeKind::Inside,
                    );
                }
            },
        );
        // the first page has its own "Get started" in the middle of the page
        if page > 0 {
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(foot.shrink2(Vec2::new(PAD, 0.0)))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    if button(ui, Look::Soft, None, "Back").clicked() {
                        step = Step::Back;
                    }
                },
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(foot.shrink2(Vec2::new(PAD, 0.0)))
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    if page + 1 < PAGES {
                        // the key page cannot be passed without a combination
                        let ok = self.welcome.as_ref().is_none_or(|w| w.can_continue(page));
                        ui.add_enabled_ui(ok, |ui| {
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
                        });
                    } else if button(ui, Look::Primary(accent), Some(symbols::CHECK), "Done")
                        .clicked()
                    {
                        step = Step::Finish {
                            open_settings: false,
                        };
                    }
                },
            );
        }

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
                Step::Next if w.can_continue(w.page) => w.page = (w.page + 1).min(PAGES - 1),
                Step::Next => {}
                Step::Back => w.page = w.page.saturating_sub(1),
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

/// A big heading with a line of secondary text under it.
fn heading(ui: &mut egui::Ui, title: &str) {
    ui.label(fonts::title(title, 28.0, pal().text));
    ui.add_space(GAP);
}

/// Remembers how far content reached below its card: the test of the pages reads it.
fn note_overflow(ctx: &egui::Context, over: f32) {
    ctx.data_mut(|d| {
        let id = egui::Id::new("welcome_card_overflow");
        let worst: f32 = d.get_temp(id).unwrap_or(0.0);
        d.insert_temp(id, worst.max(over));
    });
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

/// Page 0: the app icon in the middle of the page (the rounded square of the real application),
/// its name, one line about it and "Get started". Behind them, the mascot waves as a faint
/// watermark. Nothing else. Returns true when "Get started" was clicked.
fn intro_page(ui: &mut egui::Ui) -> bool {
    const ICON: f32 = 108.0; // side of the app icon
    const CTA_H: f32 = 44.0;
    let ctx = ui.ctx().clone();
    let accent = rgb(cfg().accent);
    let area = ui.available_rect_before_wrap();
    let painter = ui.painter().with_clip_rect(area.expand(BODY_ROOM));

    // Decorative mascot background: the welcome pose, bigger than the page, a little right of and
    // below the middle, fading out toward the edges of the page. Everything is a fraction of the
    // page (not a number of points), so it keeps its place in the small card too.
    let side = (area.width().min(area.height()) * INTRO_MASCOT_SCALE).max(1.0);
    let center = area.center()
        + Vec2::new(
            area.width() * INTRO_MASCOT_SHIFT[0],
            area.height() * INTRO_MASCOT_SHIFT[1],
        );
    mascot::paint_faded(
        &painter,
        Pose::Welcome,
        egui::Rect::from_center_size(center, Vec2::splat(side)),
        area.shrink(0.2),
        0.18,
        Style::Watermark,
    );

    let name = painter.layout_no_wrap(
        APP_NAME.to_owned(),
        egui::FontId::new(44.0, fonts::brand()),
        pal().text,
    );
    let tagline = painter.layout(
        TAGLINE.to_owned(),
        egui::FontId::proportional(14.0),
        pal().muted,
        (area.width() - 2.0 * PAD).clamp(160.0, 420.0),
    );
    // the block: icon, name, line, button, with real gaps, centered in the page
    let gaps = [26.0, 8.0, 30.0];
    let block = ICON + gaps[0] + name.size().y + gaps[1] + tagline.size().y + gaps[2] + CTA_H;
    let mut y = (area.center().y - block / 2.0).round().max(area.min.y);
    let cx = area.center().x;

    // the icon, on a soft shadow
    let icon = egui::Rect::from_center_size(egui::pos2(cx, y + ICON / 2.0), Vec2::splat(ICON));
    let radius = egui::CornerRadius::same((ICON * 0.225) as u8);
    painter.rect_filled(
        icon.translate(Vec2::new(0.0, 6.0)).expand(1.0),
        radius,
        Color32::from_black_alpha(46),
    );
    if !brand::paint_app_icon(&painter, icon) {
        // the picture is part of the program, but if it cannot be read a rounded square in the
        // accent with the mark stands in
        painter.rect_filled(icon, radius, accent);
        brand::paint(
            &painter,
            brand::Art::Mark,
            icon.shrink(ICON * 0.2),
            on_accent(accent),
        );
    }
    y += ICON + gaps[0];

    // the name (the brand font sits a little to the right of its box, so pull it back)
    let name_h = name.size().y;
    painter.galley(
        egui::pos2(cx - name.size().x / 2.0 - 2.0, y).round(),
        name,
        Color32::PLACEHOLDER,
    );
    y += name_h + gaps[1];

    let tagline_h = tagline.size().y;
    painter.galley(
        egui::pos2(cx - tagline.size().x / 2.0, y).round(),
        tagline,
        Color32::PLACEHOLDER,
    );
    y += tagline_h + gaps[2];

    let mut start = false;
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                egui::pos2(area.min.x, y.round()),
                Vec2::new(area.width(), CTA_H),
            ))
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            start = button(
                ui,
                Look::Primary(accent),
                Some(symbols::CARET_RIGHT),
                "Get started",
            )
            .clicked();
        },
    );
    note_overflow(&ctx, (y + CTA_H - area.max.y).max(0.0));
    start
}

/// Page 2: "What do you do most?", six cards to pick from (one or several). The pick shapes the
/// pie of the next page and is written to the config when the guide ends with "Get started".
fn persona_page(ui: &mut egui::Ui, w: &mut Welcome, accent: Color32) {
    const GAP_X: f32 = 24.0;
    const GAP_Y: f32 = 22.0;
    let ctx = ui.ctx().clone();
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

/// Height and font size of a keycap of the key page.
const CAP_H: f32 = 36.0;
const CAP_FONT: f32 = 16.0;

/// A keycap of the key page: a small sticker with the name of the key in it. `ghost` draws the gap
/// where the key that is still to come will be. Returns the width used.
fn keycap(painter: &egui::Painter, at: egui::Pos2, text: &str, tone: Color32, ghost: bool) -> f32 {
    const H: f32 = CAP_H;
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::new(CAP_FONT, fonts::semibold()),
        pal().text,
    );
    let w = (galley.size().x + 28.0).max(H);
    let rect = egui::Rect::from_min_size(at, Vec2::new(w, H));
    let radius = egui::CornerRadius::same(12);
    if ghost {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(2.0, pal().muted.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::new(CAP_FONT, fonts::semibold()),
            pal().muted,
        );
    } else {
        sticker(painter, rect, radius, tone);
        painter.galley(
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            )
            .round(),
            galley,
            Color32::PLACEHOLDER,
        );
    }
    w
}

/// Page 3: the key combination that opens the pie, and an optional second one for the pie of the
/// app in front. Two roomy boxes, centered, and no buttons: click a box and press the keys. The box
/// that will take the keys gets an accent edge (a soft one while the pointer is over it), the
/// modifiers held show at once as keycaps, and the key that ends the combination completes it.
/// A small cross on the second box removes it. The first one is required: "Next" waits for it.
fn key_page(
    ui: &mut egui::Ui,
    w: &mut Welcome,
    accent: Color32,
    os: Os,
    rec: Option<Slot>,
    live: u8,
) -> KeyAction {
    const CARD_GAP: f32 = 16.0;
    const INSET: f32 = 20.0;
    let ctx = ui.ctx().clone();
    let required = key_required();
    let mut action = KeyAction::None;
    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
        heading(ui, "Open the pie with your keys");
        muted(
            ui,
            if required {
                "Press the keys you want, for example Ctrl + Space."
            } else {
                "Not available in this session."
            },
        );
        ui.add_space(8.0);
        if !required {
            muted(
                ui,
                "The mouse trigger and the trackpad still work. You can continue.",
            );
            return;
        }
        ui.spacing_mut().item_spacing.y = CARD_GAP;
        let width = ui.available_width().min(540.0);
        // as roomy as the page allows: what is left, less the gap, the hint and the shadows
        let card_h =
            ((ui.available_height() - 2.0 * CARD_GAP - 26.0) / 2.0 - HARD).clamp(104.0, 140.0);
        let rows = [
            (
                Slot::KeyPie,
                "Open the pie",
                "Your general pie. Required.",
                w.key,
                candy(4),
            ),
            (
                Slot::KeyPieApp,
                "Open the pie of the app",
                "The app in front. Optional.",
                w.key_app,
                candy(1),
            ),
        ];
        for (i, (slot, title, hint, key, tone)) in rows.into_iter().enumerate() {
            let painter = ui.painter().clone();
            let (full, resp) =
                ui.allocate_exact_size(Vec2::new(width, card_h + HARD), egui::Sense::click());
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            let rect = egui::Rect::from_min_size(full.min, Vec2::new(width - HARD, card_h));
            let recording = rec == Some(slot);
            let on =
                ctx.animate_bool_with_time(egui::Id::new(("welcome_key_on", i)), recording, 0.15);
            let hot = ctx.animate_bool_with_time(
                egui::Id::new(("welcome_key_hot", i)),
                resp.hovered() && !recording,
                0.12,
            );
            sticker(
                &painter,
                rect,
                panel_r(),
                pal().card.lerp_to_gamma(accent, 0.12 * on),
            );
            // the edge says where the keys will go: the accent while recording, softer on hover
            let edge = on.max(0.6 * hot);
            if edge > 0.01 {
                painter.rect_stroke(
                    rect,
                    panel_r(),
                    Stroke::new(3.0, pal().line.lerp_to_gamma(accent, edge)),
                    egui::StrokeKind::Inside,
                );
            }

            painter.text(
                rect.min + Vec2::new(INSET, 16.0),
                egui::Align2::LEFT_TOP,
                title,
                egui::FontId::new(15.0, fonts::semibold()),
                pal().text,
            );
            painter.text(
                rect.min + Vec2::new(INSET, 38.0),
                egui::Align2::LEFT_TOP,
                hint,
                egui::FontId::proportional(12.5),
                pal().muted,
            );

            // a small cross removes the second combination
            let mut cleared = false;
            if slot == Slot::KeyPieApp && key.is_some() && !recording {
                let xr = egui::Rect::from_center_size(
                    egui::pos2(rect.max.x - INSET - 6.0, rect.min.y + 26.0),
                    Vec2::splat(28.0),
                );
                let xresp = ui
                    .interact(xr, egui::Id::new("welcome_key_clear"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Remove");
                let c = if xresp.hovered() {
                    pal().text
                } else {
                    pal().muted
                };
                fonts::paint_icon(&painter, xr.center(), symbols::X_CIRCLE, 18.0, c, false);
                if xresp.clicked() {
                    cleared = true;
                    action = KeyAction::ClearApp;
                }
            }
            if resp.clicked() && !cleared {
                action = KeyAction::Record(slot);
            }

            // the keys, centered in the lower part of the box
            let caps_rect = egui::Rect::from_min_max(
                egui::pos2(rect.min.x + INSET, rect.min.y + 58.0),
                egui::pos2(rect.max.x - INSET, rect.max.y - 10.0),
            );
            let view = if recording && live == 0 {
                KeyView::Waiting
            } else {
                key_view(
                    os,
                    key.as_ref(),
                    recording,
                    if recording { live } else { 0 },
                )
            };
            match &view {
                KeyView::Waiting => {
                    let text = if recording {
                        "Press your keys"
                    } else if slot == Slot::KeyPieApp {
                        "Click to add one"
                    } else {
                        "Click to set"
                    };
                    painter.text(
                        caps_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        text,
                        egui::FontId::new(15.0, fonts::semibold()),
                        pal().muted,
                    );
                    if recording {
                        // a quiet pulse says the box is listening
                        let t = ctx.input(|i| i.time) as f32;
                        let a = 0.5 + 0.5 * (t * 3.0).sin();
                        painter.circle_filled(
                            egui::pos2(caps_rect.center().x + 62.0, caps_rect.center().y),
                            3.5,
                            accent.gamma_multiply(0.35 + 0.65 * a),
                        );
                        ctx.request_repaint();
                    }
                }
                KeyView::Holding(mods) => {
                    let names: Vec<(&str, bool)> = mods
                        .iter()
                        .map(|m| (*m, false))
                        .chain(std::iter::once(("key", true)))
                        .collect();
                    paint_caps(&painter, caps_rect, &names, candy(3));
                    ctx.request_repaint();
                }
                KeyView::Set(keys) => {
                    let names: Vec<(&str, bool)> =
                        keys.iter().map(|k| (k.as_str(), false)).collect();
                    paint_caps(&painter, caps_rect, &names, tone);
                }
            }
        }

        let msg = if rec.is_some() {
            "Press the keys you want. Esc cancels."
        } else if w.key_dup {
            "That one already opens your general pie."
        } else if w.key_hint {
            "Add Ctrl, Alt, Shift or Cmd: a key alone blocks typing."
        } else if w.key.is_none() {
            "Click the first box to continue."
        } else if w.key_app.is_some() {
            "Click a box to change it. The cross removes the second one."
        } else {
            "Click a box, then press your keys."
        };
        muted(ui, msg);
    });
    action
}

/// Draws `caps` in a row, centered in `rect`, with a small gap between them.
fn paint_caps(painter: &egui::Painter, rect: egui::Rect, caps: &[(&str, bool)], tone: Color32) {
    const H: f32 = CAP_H;
    const GAP_CAP: f32 = 8.0;
    // measure first, so the row is centered
    let widths: Vec<f32> = caps
        .iter()
        .map(|(t, _)| {
            let g = painter.layout_no_wrap(
                (*t).to_owned(),
                egui::FontId::new(CAP_FONT, fonts::semibold()),
                pal().text,
            );
            (g.size().x + 28.0).max(H)
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + GAP_CAP * (caps.len().saturating_sub(1)) as f32;
    let mut x = rect.center().x - total / 2.0;
    let y = rect.center().y - H / 2.0;
    for ((text, ghost), w) in caps.iter().zip(widths) {
        keycap(painter, egui::pos2(x, y), text, tone, *ghost);
        x += w + GAP_CAP;
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

/// Page 3: a check, "You're all set", one line on how to use the pie and the launch-at-login box.
fn ready_page(ui: &mut egui::Ui, w: &mut Welcome, accent: Color32, os: Os) {
    const RADIUS: f32 = 18.0;
    const TOGGLE_W: f32 = 460.0;
    let area = ui.available_rect_before_wrap();
    let painter = ui.painter().clone();
    let cx = area.center().x;

    let title = painter.layout_no_wrap(
        "You're all set".to_owned(),
        egui::FontId::new(30.0, fonts::semibold()),
        pal().text,
    );
    let lead = painter.layout(
        "Hold the middle mouse button, or touch and hold the trackpad, to open your pie."
            .to_owned(),
        egui::FontId::proportional(14.0),
        pal().muted,
        (area.width() - 2.0 * PAD).clamp(160.0, 440.0),
    );
    // the block: check, title, line, toggle (about 64 points high), centered in the page
    let gaps = [14.0, 6.0, 22.0];
    let toggle_h = 64.0 * 2.0;
    let block =
        2.0 * RADIUS + gaps[0] + title.size().y + gaps[1] + lead.size().y + gaps[2] + toggle_h;
    let mut y = (area.center().y - block / 2.0).round().max(area.min.y);

    let c = egui::pos2(cx, y + RADIUS);
    sticker_disc(&painter, c, RADIUS - 2.0, accent, 3.0);
    fonts::paint_icon(&painter, c, symbols::CHECK, 22.0, on_accent(accent), false);
    y += 2.0 * RADIUS + gaps[0];

    let title_h = title.size().y;
    painter.galley(
        egui::pos2(cx - title.size().x / 2.0, y).round(),
        title,
        Color32::PLACEHOLDER,
    );
    y += title_h + gaps[1];
    let lead_h = lead.size().y;
    painter.galley(
        egui::pos2(cx - lead.size().x / 2.0, y).round(),
        lead,
        Color32::PLACEHOLDER,
    );
    y += lead_h + gaps[2];

    let width = TOGGLE_W.min(area.width());
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            egui::pos2(cx - width / 2.0, y.round()),
            Vec2::new(width, toggle_h + 8.0),
        )),
        |ui| {
            panel(ui, "", |ui| {
                toggle_row(
                    ui,
                    &mut w.launch_at_login,
                    "Launch CursorPie at login",
                    os.welcome_startup_hint(),
                    accent,
                );
                toggle_row(
                    ui,
                    &mut w.intelligent,
                    "Turn on the intelligent pie",
                    "Suggests the apps and sites you use. Stays on this computer.",
                    accent,
                );
            });
        },
    );

    // the mascot celebrates in the free margin at the right of the toggles: only when that margin is
    // wide enough for it to breathe, so a small card (or a long translation) never gets it on top
    // of the text
    let margin = (area.width() - width) / 2.0;
    if margin >= READY_MASCOT_MIN_MARGIN {
        let side = (margin - 2.0 * GAP).min(READY_MASCOT_MAX);
        let bottom = (y + toggle_h + 8.0).min(area.max.y);
        let rect = egui::Rect::from_min_size(
            egui::pos2(area.max.x - margin + GAP, bottom - side),
            Vec2::splat(side),
        );
        mascot::paint(&painter, Pose::Celebrate, rect, Style::Full);
    }
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
    fn the_guide_has_five_pages_and_the_dots_fit_the_footer() {
        assert_eq!(PAGES, 5);
        // the dots are centered in a box as wide as they are when one of them is the wide one
        assert!(DOT_WIDE + (PAGES as f32 - 1.0) * (DOT + DOT_GAP) < 200.0);
    }

    #[test]
    fn linux_has_its_own_setup_page() {
        assert!(setup_title(Os::Linux).contains("X11"));
        let steps = setup_steps(Os::Linux);
        assert!(
            steps.iter().any(|s| s.contains("no tray icon")),
            "{steps:?}"
        );
        assert!(!steps.iter().any(|s| s.contains("Accessibility")));
        assert!(!setup_lead(Os::Linux).contains("Accessibility"));
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
            0 => {
                intro_page(ui);
            }
            1 => setup_page(ui, accent, os),
            2 => persona_page(ui, w, accent),
            KEY_PAGE => {
                key_page(ui, w, accent, os, None, 0);
            }
            _ => ready_page(ui, w, accent, os),
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
        let enabled = cfg().smart.enabled;
        assert_eq!(
            w.intelligent, enabled,
            "the box shows what Settings already has"
        );
        assert!(w.picks.is_empty());
    }

    #[test]
    fn the_key_page_cannot_be_passed_without_a_combination() {
        use crate::keys::{MOD_CTRL, Shortcut, key};
        let mut w = Welcome::new(false);
        w.key = None;
        assert!(w.can_continue(0) && w.can_continue(1) && w.can_continue(2));
        assert_eq!(
            w.can_continue(KEY_PAGE),
            !key_required(),
            "required, except where the keyboard trigger cannot work"
        );
        assert!(w.set_key(Shortcut::new(key::SPACE, MOD_CTRL)));
        assert!(w.can_continue(KEY_PAGE));
        assert!(!w.key_hint);
    }

    #[test]
    fn a_key_without_a_modifier_or_esc_is_not_kept_and_the_page_says_so() {
        use crate::keys::{MOD_CTRL, Shortcut, key};
        let mut w = Welcome::new(false);
        w.key = None;
        assert!(!w.set_key(Shortcut::new(key::SPACE, 0)));
        assert!(w.key.is_none() && w.key_hint);
        assert!(!w.set_key(Shortcut::new(key::ESCAPE, MOD_CTRL)));
        assert!(w.key.is_none());
        assert!(w.set_key(Shortcut::new(key::SPACE, MOD_CTRL)));
        assert!(!w.key_hint, "a good one clears the hint");
    }

    #[test]
    fn the_modifiers_are_named_as_they_are_held() {
        use crate::keys::{MOD_ALT, MOD_CMD, MOD_CTRL, MOD_SHIFT};
        assert_eq!(mod_names_in(Os::Mac, MOD_CTRL | MOD_ALT), ["Ctrl", "Opt"]);
        assert_eq!(
            mod_names_in(Os::Windows, MOD_CMD | MOD_SHIFT),
            ["Shift", "Win"]
        );
        assert!(mod_names_in(Os::Linux, 0).is_empty());
    }

    #[test]
    fn the_key_page_shows_what_is_pressed_as_it_is_pressed() {
        use crate::keys::{MOD_CTRL, MOD_SHIFT, Shortcut, key};
        let set = Shortcut::new(key::SPACE, MOD_CTRL);
        // nothing yet
        assert_eq!(key_view(Os::Windows, None, true, 0), KeyView::Waiting);
        // modifiers held: they show at once, the key is still to come
        assert_eq!(
            key_view(Os::Windows, None, true, MOD_CTRL | MOD_SHIFT),
            KeyView::Holding(vec!["Ctrl", "Shift"])
        );
        // the combination: one keycap per key
        assert_eq!(
            key_view(Os::Windows, Some(&set), true, 0),
            KeyView::Set(vec!["Ctrl".into(), "Space".into()])
        );
        // pressing again while one is set shows the new keys being held
        assert_eq!(
            key_view(Os::Windows, Some(&set), true, MOD_SHIFT),
            KeyView::Holding(vec!["Shift"])
        );
        // not listening: held modifiers are none of its business
        assert_eq!(
            key_view(Os::Windows, Some(&set), false, MOD_SHIFT),
            KeyView::Set(vec!["Ctrl".into(), "Space".into()])
        );
    }

    #[test]
    fn the_app_combination_is_optional_and_never_the_same_as_the_general_one() {
        use crate::keys::{MOD_CTRL, MOD_SHIFT, Shortcut, key};
        let mut w = Welcome::new(false);
        w.key = None;
        w.key_app = None;
        assert!(w.can_continue(KEY_PAGE) == !key_required());
        assert!(w.set_key(Shortcut::new(key::SPACE, MOD_CTRL)));
        // optional: the page can be passed without it
        assert!(w.key_app.is_none() && w.can_continue(KEY_PAGE));
        assert!(w.set_app_key(Shortcut::new(key::SPACE, MOD_CTRL | MOD_SHIFT)));
        assert!(w.key_app.is_some());
        // the general one already has this: not kept, and the page says so
        assert!(!w.set_app_key(Shortcut::new(key::SPACE, MOD_CTRL)));
        assert!(w.key_dup);
        // pressing the app's combination for the general pie takes it over
        assert!(w.set_key(Shortcut::new(key::SPACE, MOD_CTRL | MOD_SHIFT)));
        assert!(w.key_app.is_none());
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
}
