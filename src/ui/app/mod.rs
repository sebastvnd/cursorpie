//! The eframe application: owns the UI state and reacts to messages from the other threads.
//!
//! | Submodule | Contents |
//! |---|---|
//! | `messages` | `App::handle_msg`: applies each [`Msg`] |
//! | `windows` | showing the pie, opening/closing Settings, quitting, menu bar actions |
//! | `notice` | the notice of what CursorPie is doing (`ui::hud`): showing the window as a small pill and drawing it |
//! | `standby` | keeping the window mapped for the next pie (Windows) and showing a pie again that never reached the screen |
//! | `lifecycle` | idle sleep, freeing the Settings copy, warming up icons before the pie opens |
//! | `owner` | who owns the window (pie, pill, lock, panel, Settings) and how it is handed over |
//! | `updates` | the daily look for a newer release: what Settings does when it opens, and with the answer |
//!
//! The same native window shows the pie and Settings (see
//! [`guides::architecture`](crate::guides::architecture#one-window-for-everything)). `App::logic`
//! runs every frame even while the window is hidden and must not draw; `App::ui` draws either the
//! pie ([`pie_ui`](crate::ui::pie)) or Settings.

mod lifecycle;
mod lock;
mod messages;
mod notice;
mod owner;
mod panel;
mod standby;
mod updates;
mod windows;
pub(crate) use notice::pill_center;
pub use windows::{GeomGate, ShowWait};

use crate::config::{Config, MAX_ITEMS, cfg};
use crate::history::History;
use crate::msg::Msg;
use crate::platform;
use crate::ui::settings::{Ed, Welcome};
use crate::ui::sleep::Sleeper;
use crate::ui::tray::TrayHandles;
use eframe::egui::{self, Pos2, Vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::mpsc;
use std::time::Instant;

/// Settings card size. The window is larger by WIN_MARGIN on each side for the drop shadow.
/// The width leaves room for the sidebar and a Menu editor of the same width it had before.
pub const CARD_SIZE: [f32; 2] = [1180.0, 740.0];
/// Space around the card for its drop shadow (window size = card size + 2 x margin).
pub const WIN_MARGIN: f32 = 40.0;
/// Narrowest the card gets on a small display: the Menu tab still has room for the pie and an editor.
pub const CARD_MIN_W: f32 = 980.0;
/// Shortest the card gets on a small display.
pub const CARD_MIN_H: f32 = 520.0;

/// The card size for a display `area` points large: [`CARD_SIZE`], shrunk to fit with room for the
/// shadow and the taskbar or menu bar, but never below [`CARD_MIN_W`] x [`CARD_MIN_H`].
pub fn card_size_for(area: Vec2) -> Vec2 {
    let max_w = (area.x - 2.0 * WIN_MARGIN - 40.0).max(CARD_MIN_W);
    let max_h = (area.y - 2.0 * WIN_MARGIN - 60.0).max(CARD_MIN_H);
    egui::vec2(CARD_SIZE[0].min(max_w), CARD_SIZE[1].min(max_h))
}

/// Welcome window card size (the window adds [`WIN_MARGIN`] on each side for the shadow).
pub const WELCOME_SIZE: [f32; 2] = [900.0, 640.0];
/// Smallest the welcome card gets on a small display.
pub const WELCOME_MIN: [f32; 2] = [720.0, 540.0];

/// The welcome card size for a display `area` points large: [`WELCOME_SIZE`], shrunk to fit with
/// room for the shadow and the taskbar or menu bar, but never below [`WELCOME_MIN`].
pub fn welcome_size_for(area: Vec2) -> Vec2 {
    let max_w = (area.x - 2.0 * WIN_MARGIN - 40.0).max(WELCOME_MIN[0]);
    let max_h = (area.y - 2.0 * WIN_MARGIN - 60.0).max(WELCOME_MIN[1]);
    egui::vec2(WELCOME_SIZE[0].min(max_w), WELCOME_SIZE[1].min(max_h))
}

/// Most undo steps Settings keeps. A step is a copy of the edited config (a few KiB).
pub const HISTORY_LIMIT: usize = 50;

/// The `HWND` of the native window, as the integer [`platform::clear_window_frame`] takes.
/// `None` on every platform that does not have one (macOS), or while the window does not exist yet.
pub fn native_handle(window: &impl HasWindowHandle) -> Option<isize> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

/// The X window id of the native window, for the platform's X11 requests
/// (`platform::set_window_veil`).
/// `None` when the window is not an X window (Windows, macOS, or a native Wayland window).
pub fn x11_window(window: &impl HasWindowHandle) -> Option<std::os::raw::c_ulong> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window),
        RawWindowHandle::Xcb(h) => Some(std::os::raw::c_ulong::from(h.window.get())),
        _ => None,
    }
}

/// All UI-thread state. Fields are grouped: the message channel, the pie, Settings.
pub struct App {
    pub rx: mpsc::Receiver<Msg>,
    pub tx: mpsc::Sender<Msg>,

    // pie
    pub hover: Option<usize>,
    pub profile: usize,
    pub path: Vec<usize>,
    /// Trail ("web") mode: where the rings that were left to open the current submenu were
    /// centered (screen points), outermost first. One per level of `path`, or empty.
    pub trail: Vec<Pos2>,
    /// Where the node of each ring in `trail` is drawn now (screen points). It glides from where
    /// the ring was to its place in the web, so a level change is not a jump.
    pub trail_disp: Vec<Pos2>,
    pub pie_open: bool,
    pub visible: bool,
    pub fresh: bool,
    /// Frames of the current pie drawn so far, up to 2. At 2 the tap is told the pie is really on
    /// screen (`tap::pie_painted`), which is what lets a held trigger turn into Settings.
    pub pie_frames: u8,
    pub anim: f32,
    pub hover_anim: [f32; MAX_ITEMS],
    /// How far the two badges of an "Open app" slice have come out (0 to 1, one clock per slice).
    pub badge_in: [f32; MAX_ITEMS],
    /// Per slice and badge (new window, app that is open): how much it is the one the pointer is on.
    pub badge_pick: [[f32; 2]; MAX_ITEMS],
    /// The same for the one the pointer is not on, which steps back.
    pub badge_step: [[f32; 2]; MAX_ITEMS],
    pub active_anim: [f32; MAX_ITEMS],
    pub win: f32,
    pub pie_pos: Pos2,
    /// The top-left corner the pie window was last asked to be at (window points), so the ring
    /// can be drawn on the pointer when a window manager put the window somewhere else, as it
    /// does near the edge of the screen (see `ui::screen::pie_center_shift`).
    pub pie_origin: Pos2,
    /// Where the pointer was (pointer space, like `pie_pos`) when the pie window was last put
    /// somewhere: the center of the window. A submenu does not move the window while its ring
    /// still fits in it; the ring is drawn `pie_off` away from the center instead.
    pub pie_anchor: Pos2,
    /// How far the center of the ring on screen is from the center of the pie window (window
    /// points). Zero while the window is centered on the ring.
    pub pie_off: Vec2,
    /// The pie window is as big as the display (*Show slice names* is on, see `App::show_pie`);
    /// `hide_root` makes it small again.
    pub pie_full: bool,
    pub opened_at: Instant,
    /// Where the pie window was last asked to be, and since when. `logic` keeps re-sending that
    /// position for a moment, because some window managers (KWin on X11) place a window that is
    /// mapped again at the center of the screen and ignore the position set while it was hidden.
    /// Only set where [`platform::window_manager_may_recenter`] is true.
    pub want_pos: Option<(Pos2, Instant)>,
    pub last_tick: Instant,
    /// Some while a preview (menu bar "Switch side-button pie") is on screen: when it ends
    pub preview_until: Option<Instant>,

    // settings
    pub settings_open: bool,
    pub quitting: bool,
    /// The quit confirmation sheet is showing: Settings has unsaved changes and the user asked to
    /// quit. See [`crate::ui::settings`] (`quit_dialog`).
    pub confirm_quit: bool,
    /// The "discard unsaved changes?" sheet is showing (Cancel or Esc with edits not saved).
    pub confirm_discard: bool,
    /// What the last import of a file found: shown in a sheet until it is closed.
    pub import_report: Option<ImportSheet>,
    /// An export that could not be written: shown in a sheet until it is closed (see
    /// `ui::settings::export_error`).
    pub export_error: Option<ExportError>,
    /// What a run of a flow that did not go well found: shown in a sheet until it is closed (see
    /// `ui::settings::run_report`). Only set while Settings is open; the desktop notification
    /// of the run is sent by [`actions`](crate::actions).
    pub run_report: Option<crate::flowrun::Report>,
    pub edit: Config,
    /// What `edit` was when Settings opened, to tell whether there is anything unsaved. Dropped
    /// with the rest of the editor (`App::release_editor`).
    pub opened: Option<Config>,
    /// Undo / redo for `edit`, see [`crate::history`]
    pub history: History<Config>,
    pub ed: Ed,
    pub settings_anim: f32,
    /// Set when the window is asked to become the Settings (or welcome) card: the window size it
    /// was asked for and since when. Until the window really has that size nothing is drawn, see
    /// [`App::window_settling`].
    pub settings_wait: Option<(Vec2, Instant)>,
    /// Linux: the window was asked to appear and is kept unmapped until it has its size and an empty
    /// frame was drawn, see `App::show_window`. While set, nothing is drawn.
    pub show_wait: Option<ShowWait>,
    /// macOS: the window is on its way to the size and place of its new owner and nothing is drawn
    /// until it has them (see `App::arm_geom_gate`).
    pub geom_gate: Option<GeomGate>,
    /// Linux: transparent frames still to draw *after* the window was mapped. Under XWayland the
    /// surface only exists once the window is mapped, so the frames drawn before that never reach
    /// the compositor; the first buffer it sees must be empty as well. While above 0 nothing is drawn.
    pub settle_frames: u8,
    /// Linux: the X window id of the native window, once it is known (see [`x11_window`]).
    pub x_window: Option<std::os::raw::c_ulong>,
    /// Wayland: the window was mapped invisible (`platform::set_window_veil`, at every map) and is
    /// made visible when its first empty frame is on screen: its X id and since when. See `App::step_veil`.
    pub veil: Option<(std::os::raw::c_ulong, Instant)>,
    /// Until when the window is repainted on a timer after a file chooser closed (see `Msg::Refocus`).
    pub repaint_until: Option<Instant>,
    /// Some(true) = closing and saving, Some(false) = closing without saving
    pub closing: Option<bool>,
    pub card_size: Vec2,
    pub toast: Option<(String, Instant)>,
    /// Some while the window shows the notice of an action (see [`crate::ui::hud`]).
    pub hud: Option<crate::ui::hud::Hud>,
    /// The pill with the name and the icon of the slice under the pointer, drawn in the window of the
    /// pie next to the ring (see [`crate::ui::pie::pill`]). Some from the first slice the pointer
    /// reaches until the pill has gone away; `None` while the pie is closed.
    pub slice_pill: Option<crate::ui::pie::pill::SlicePill>,
    /// A notice that arrived while the pie was still on screen: shown as soon as the window is free.
    pub hud_pending: Option<(crate::ui::hud::HudNote, Instant)>,
    /// A pie that was asked for while a notice was on screen: the notice finishes its animation
    /// first and the pie opens when the window is free (see `App::defer_pie_for_hud`).
    pub pie_pending: Option<(f64, f64, usize, Vec<usize>)>,
    /// Some while the window shows the lock animation: the pie that turns into a padlock, or the
    /// padlock that opens into the pie (see [`crate::ui::pie::lock`]).
    pub lock_anim: Option<crate::ui::pie::lock::LockAnim>,
    /// The lock animation that runs is drawn over the pie that was open (its own slices and icons
    /// collapse into the padlock) and not as a plain stand-in; see `App::lock_anim_start`.
    pub lock_over_pie: bool,
    /// Since when the pie that opened out of an unlock carries the opening padlock in its middle
    /// (`Some` for [`crate::ui::pie::lock::REVEAL_SECS`]); see [`crate::ui::pie::lock::reveal_scene`].
    pub lock_reveal: Option<Instant>,
    /// Some while the window shows a panel (see [`crate::panel`]).
    pub panel: Option<panel::ActivePanel>,
    /// Panels that were asked for while the window was busy: shown one by one when it is free.
    pub panel_queue: std::collections::VecDeque<crate::panel::Request>,
    /// Some while the welcome window is open (it uses the Settings window, so `settings_open` is
    /// true too). See [`crate::ui::settings::Welcome`].
    pub welcome: Option<Welcome>,

    /// Idle clock that decides when caches and the Settings copy are freed
    pub sleeper: Sleeper,

    /// Set while the window, already click-through and empty, is kept on screen for a few more
    /// frames after Settings closed: frames left, and since when. A hidden window runs no egui
    /// pass and paints nothing, so what Settings freed (textures, the old font atlas) is only given
    /// back by a pass that really paints (see `App::begin_close_flush`).
    pub close_flush: Option<(u8, Instant)>,

    /// The window is mapped, empty and parked off screen, waiting for a pie (see `standby`).
    pub standby: Option<standby::Standby>,
    /// The startup prime was done (or skipped): see `App::prime_window`.
    pub primed: bool,
    /// A pie was asked for and has not been drawn yet (see `App::step_pie_watch`).
    pub pie_watch: Option<standby::PieWatch>,

    /// Who owned the window at the last pass of `logic`, to log every change (see `owner`).
    last_owner: owner::Owner,

    pub tray: Option<TrayHandles>,
}

impl App {
    /// Creates the app. `win` is the pie window size, `tray` the menu bar handles (if the icon could be created).
    pub fn new(
        rx: mpsc::Receiver<Msg>,
        tx: mpsc::Sender<Msg>,
        win: f32,
        tray: Option<TrayHandles>,
    ) -> Self {
        // the memory log: the first mark, and the sampler that writes one every 30 s
        crate::ui::memlog::start_sampler();
        crate::ui::memlog::mark("app_new");
        let edit = cfg().clone();
        Self {
            rx,
            tx,
            hover: None,
            profile: 0,
            path: Vec::new(),
            trail: Vec::new(),
            trail_disp: Vec::new(),
            pie_open: false,
            visible: false,
            fresh: false,
            pie_frames: 2,
            anim: 0.0,
            hover_anim: [0.0; MAX_ITEMS],
            badge_in: [0.0; MAX_ITEMS],
            badge_pick: [[0.0; 2]; MAX_ITEMS],
            badge_step: [[0.0; 2]; MAX_ITEMS],
            active_anim: [0.0; MAX_ITEMS],
            win,
            pie_pos: egui::pos2(200.0, 200.0),
            pie_origin: egui::pos2(0.0, 0.0),
            pie_anchor: egui::pos2(0.0, 0.0),
            pie_off: Vec2::ZERO,
            pie_full: false,
            opened_at: Instant::now(),
            want_pos: None,
            last_tick: Instant::now(),
            preview_until: None,
            settings_open: false,
            quitting: false,
            confirm_quit: false,
            confirm_discard: false,
            import_report: None,
            export_error: None,
            run_report: None,
            opened: None,
            history: History::new(&edit, HISTORY_LIMIT),
            edit,
            ed: Ed::new(),
            settings_anim: 0.0,
            settings_wait: None,
            show_wait: None,
            geom_gate: None,
            settle_frames: 0,
            x_window: None,
            veil: None,
            repaint_until: None,
            closing: None,
            card_size: egui::vec2(CARD_SIZE[0], CARD_SIZE[1]),
            toast: None,
            hud: None,
            slice_pill: None,
            hud_pending: None,
            pie_pending: None,
            lock_anim: None,
            lock_over_pie: false,
            lock_reveal: None,
            panel: None,
            panel_queue: std::collections::VecDeque::new(),
            welcome: None,
            sleeper: Sleeper::new(Instant::now()),
            close_flush: None,
            standby: None,
            primed: false,
            pie_watch: None,
            last_owner: owner::Owner::Idle,
            tray,
        }
    }

    /// Shows a short message (toast) at the bottom of Settings; it disappears after a few seconds.
    pub fn say(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// An export failed: says so in the status bar and opens the sheet that stays until it is
    /// closed. Nothing may fail silently, whatever the status bar shows at that moment.
    pub fn export_failed(&mut self, what: &str, reason: String) {
        self.say(format!("Export failed: {reason}"));
        self.export_error = Some(ExportError {
            what: what.to_string(),
            reason,
        });
    }
}

/// The words of the sheet that follows an export that failed (see `ui::settings::export_error`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportError {
    /// What was being exported ("Flow", "Profile", "Your setup", "Browser extension files").
    pub what: String,
    /// Why it failed, in plain words.
    pub reason: String,
}

/// The words of the sheet that follows an import (see `ui::settings::import_report`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportSheet {
    /// The name of the file that was imported.
    pub file: String,
    /// Whether the file was imported (with notes) or refused.
    pub imported: bool,
    /// One line per finding: what was repaired, what will not work, or why it was refused.
    pub lines: Vec<String>,
}

impl eframe::App for App {
    // (documented by the trait)
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // the window is always transparent; the settings card paints itself
    }

    /// Caps the side of the font atlas (see [`crate::ui::fonts::ATLAS_SIDE_MAX`]). egui makes the
    /// atlas as wide as the biggest texture the GPU takes, 16384 texels on Metal, and only starts it
    /// over when it is 80% full: a zoom over text at many sizes then filled hundreds of MB, on the
    /// CPU and again on the GPU, that the allocator did not give back after Settings closed. The
    /// limit is only ever lowered, never raised above what the GPU reported.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let reported = raw_input
            .max_texture_side
            .unwrap_or_else(|| ctx.input(|i| i.max_texture_side));
        raw_input.max_texture_side = Some(reported.min(crate::ui::fonts::ATLAS_SIDE_MAX));
    }

    /// Runs even while the window is hidden (as long as something calls request_repaint).
    /// Must not draw any UI.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // winit rewrites the window styles on every show / hide / click-through change; strip the
        // frame again before anything is painted (cheap, and a no-op on macOS)
        if let Some(handle) = native_handle(frame) {
            platform::clear_window_frame(handle);
        }
        // macOS: the pie of an app in full screen (a Space of its own) needs the window to join
        // every Space; the toolkit rewrites the level each time it shows the window, so this runs
        // every frame (no-op elsewhere, and nothing is sent when the window is already right)
        platform::keep_over_full_screen(!self.settings_open);
        if self.x_window.is_none() {
            self.x_window = x11_window(frame);
        }
        // debug log only: where the window really is, compared with where it was asked to be
        if crate::debug::observing() {
            let (outer, ppp) = ctx.input(|i| {
                let v = i.viewport();
                (
                    v.outer_rect
                        .map(|r| [r.min.x, r.min.y, r.width(), r.height()]),
                    v.native_pixels_per_point,
                )
            });
            crate::debug::observe_window(outer, ppp);
        }
        self.enforce_pie_pos(ctx);
        // evicts icons nobody used lately (never one the last frame used)
        self.ed.icons.new_frame();
        while let Ok(msg) = self.rx.try_recv() {
            self.handle_msg(ctx, msg);
        }
        self.step_show_wait(ctx);
        self.step_geom_gate(ctx);
        self.step_anim_watchdog(ctx);
        self.step_veil(ctx);
        self.watch_close_flush(ctx);
        self.step_repaint_burst(ctx);
        self.step_hud(ctx);
        self.step_panel(ctx);
        self.panel_close_requested(ctx);
        self.step_pie_watch(ctx);
        self.prime_window(ctx);
        self.step_standby(ctx);
        self.note_owner();

        // end of a preview (see Msg::Preview)
        if let Some(until) = self.preview_until {
            let now = Instant::now();
            if now >= until {
                self.preview_until = None;
                self.hover = None;
                self.pie_open = false;
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(until - now);
            }
        }

        // Cmd+W / the close button on the settings window = cancel (with animation), not quit
        if self.settings_open
            && !self.quitting
            && self.closing.is_none()
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_cancel();
        }

        self.sleep_step(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Settings cuts glyphs at many sizes while a canvas is zoomed: start the atlas again when it
        // has grown past what the interface needs (a no-op nearly always: two numbers are read)
        if self.settings_open {
            crate::ui::fonts::guard_atlas(ui.ctx());
        }
        // light or dark for this frame; Settings previews the choice before it is saved
        let (appearance, ui_theme, theme) = if self.settings_open {
            (
                self.edit.appearance,
                self.edit.ui_theme.clone(),
                self.edit.theme,
            )
        } else {
            let c = cfg();
            (c.appearance, c.ui_theme.clone(), c.theme)
        };
        crate::ui::settings::theme::apply_appearance(ui.ctx(), appearance, &ui_theme, &theme);
        if self.show_wait.is_some() || self.geom_gate.is_some() {
            // waiting to be mapped, or for the size and place of the new owner: draw nothing, so
            // the buffer shown first is fully transparent
        } else if self.settle_frames > 0 {
            // just mapped: the compositor has no buffer of ours yet, keep it empty a little longer
            self.settle_frames -= 1;
            ui.ctx().request_repaint();
        } else if self.close_flush.is_some() {
            // Settings just closed: draw nothing, but let this pass paint so the textures and the
            // font atlas it freed are really released before the window is hidden
            self.step_close_flush(&ui.ctx().clone());
        } else if self.settings_open && self.welcome.is_some() {
            self.welcome_ui(ui);
        } else if self.settings_open {
            // the ids below are salted with the opening: scroll offsets do not outlive the window
            let session = self.ed.session;
            ui.push_id(("settings_session", session), |ui| self.settings_ui(ui));
        } else if self.panel.is_some() && !self.pie_open {
            self.panel_ui(ui);
        } else if self.lock_anim.is_some() && !self.pie_open {
            self.lock_ui(ui);
        } else if self.hud.is_some() && !self.pie_open {
            self.hud_ui(ui);
        } else if self.standby_active() {
            // mapped and parked, waiting for a pie: draw nothing
            self.standby_ui(&ui.ctx().clone());
        } else {
            self.pie_ui(ui);
        }
        // the pie may just have hidden the window: start the idle clock
        self.sleep_step(&ui.ctx().clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_display_gets_the_full_card() {
        assert_eq!(
            card_size_for(egui::vec2(2560.0, 1440.0)),
            egui::vec2(CARD_SIZE[0], CARD_SIZE[1])
        );
    }

    #[test]
    fn the_welcome_card_fits_small_displays_but_not_below_its_minimum() {
        assert_eq!(
            welcome_size_for(egui::vec2(2560.0, 1440.0)),
            egui::vec2(WELCOME_SIZE[0], WELCOME_SIZE[1])
        );
        let s = welcome_size_for(egui::vec2(1280.0, 720.0));
        assert!(s.x <= 1280.0 - 2.0 * WIN_MARGIN && s.y <= 720.0 - 2.0 * WIN_MARGIN);
        assert_eq!(
            welcome_size_for(egui::vec2(300.0, 200.0)),
            egui::vec2(WELCOME_MIN[0], WELCOME_MIN[1])
        );
    }

    #[test]
    fn a_small_display_shrinks_the_card_but_not_below_the_minimum() {
        let s = card_size_for(egui::vec2(1280.0, 720.0));
        assert!(s.x <= 1280.0 - 2.0 * WIN_MARGIN, "fits the width: {}", s.x);
        assert!(s.y <= 720.0 - 2.0 * WIN_MARGIN, "fits the height: {}", s.y);
        let tiny = card_size_for(egui::vec2(600.0, 300.0));
        assert_eq!(tiny, egui::vec2(CARD_MIN_W, CARD_MIN_H));
    }
}
