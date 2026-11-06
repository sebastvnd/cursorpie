//! The eframe application: owns the UI state and reacts to messages from the other threads.
//!
//! | Submodule | Contents |
//! |---|---|
//! | `messages` | `App::handle_msg`: applies each [`Msg`] |
//! | `windows` | showing the pie, opening/closing Settings, quitting, menu bar actions |
//! | `lifecycle` | idle sleep, freeing the Settings copy, warming up icons before the pie opens |
//! | `vtrackpad` | the virtual trackpad developer tool's windows, and parking the main window while it is open |
//!
//! The same native window shows the pie and Settings (see
//! [`guides::architecture`](crate::guides::architecture#one-window-for-everything)). `App::logic`
//! runs every frame even while the window is hidden and must not draw; `App::ui` draws either the
//! pie ([`pie_ui`](crate::ui::pie)) or Settings.

mod lifecycle;
mod messages;
mod vtrackpad;
mod windows;

use crate::config::{Config, MAX_ITEMS, cfg};
use crate::history::History;
use crate::msg::Msg;
use crate::platform;
use crate::ui::settings::{Ed, Welcome};
use crate::ui::sleep::Sleeper;
use crate::ui::tray::TrayHandles;
use crate::ui::vtrackpad::draw::Shared as VtShared;
use eframe::egui::{self, Pos2, Vec2};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::{Arc, Mutex, mpsc};
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
pub const WELCOME_SIZE: [f32; 2] = [820.0, 580.0];
/// Smallest the welcome card gets on a small display.
pub const WELCOME_MIN: [f32; 2] = [660.0, 500.0];

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

/// All UI-thread state. Fields are grouped: the message channel, the pie, Settings.
pub struct App {
    pub rx: mpsc::Receiver<Msg>,
    pub tx: mpsc::Sender<Msg>,

    // pie
    pub hover: Option<usize>,
    pub profile: usize,
    pub path: Vec<usize>,
    pub pie_open: bool,
    pub visible: bool,
    pub fresh: bool,
    pub anim: f32,
    pub hover_anim: [f32; MAX_ITEMS],
    pub active_anim: [f32; MAX_ITEMS],
    pub win: f32,
    pub pie_pos: Pos2,
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
    /// Some(true) = closing and saving, Some(false) = closing without saving
    pub closing: Option<bool>,
    pub card_size: Vec2,
    pub toast: Option<(String, Instant)>,
    /// Some while the welcome window is open (it uses the Settings window, so `settings_open` is
    /// true too). See [`crate::ui::settings::Welcome`].
    pub welcome: Option<Welcome>,

    /// Idle clock that decides when caches and the Settings copy are freed
    pub sleeper: Sleeper,

    pub tray: Option<TrayHandles>,

    /// The virtual trackpad: what its window shares with the app (see `vtrackpad`).
    pub vt: Arc<Mutex<VtShared>>,
    /// The tool is open (`cargo tools trackpad` started the app with it, closing it turns it off).
    pub vt_open: bool,
    /// The trackpad was wanted when `vt_logic` last looked.
    pub vt_shown: bool,
    /// The main window is parked (1 x 1, click-through) instead of hidden, to keep the trackpad's
    /// window alive.
    pub vt_parked: bool,
    /// The display the trackpad is in the middle of, chosen when it opened.
    pub vt_display: Option<egui::Rect>,
    /// Where the virtual pointer began (its window is declared there; it moves by command).
    pub vt_pointer_start: Option<(f64, f64)>,
    /// The width the trackpad's window was last declared with.
    pub vt_width: f32,
    /// The level the trackpad's window was last given.
    pub vt_level: egui::WindowLevel,
}

impl App {
    /// Creates the app. `win` is the pie window size, `tray` the menu bar handles (if the icon could be created).
    pub fn new(
        rx: mpsc::Receiver<Msg>,
        tx: mpsc::Sender<Msg>,
        win: f32,
        tray: Option<TrayHandles>,
    ) -> Self {
        let edit = cfg().clone();
        let opts = crate::ui::vtrackpad::options();
        let width = opts.map_or(crate::ui::vtrackpad::DEFAULT_WIDTH, |o| o.width);
        let vt = VtShared::new(width, opts.is_none_or(|o| o.sound));
        Self {
            rx,
            tx,
            hover: None,
            profile: 0,
            path: Vec::new(),
            pie_open: false,
            visible: false,
            fresh: false,
            anim: 0.0,
            hover_anim: [0.0; MAX_ITEMS],
            active_anim: [0.0; MAX_ITEMS],
            win,
            pie_pos: egui::pos2(200.0, 200.0),
            opened_at: Instant::now(),
            want_pos: None,
            last_tick: Instant::now(),
            preview_until: None,
            settings_open: false,
            quitting: false,
            confirm_quit: false,
            confirm_discard: false,
            opened: None,
            history: History::new(&edit, HISTORY_LIMIT),
            edit,
            ed: Ed::new(),
            settings_anim: 0.0,
            settings_wait: None,
            closing: None,
            card_size: egui::vec2(CARD_SIZE[0], CARD_SIZE[1]),
            toast: None,
            welcome: None,
            sleeper: Sleeper::new(Instant::now()),
            tray,
            vt: Arc::new(Mutex::new(vt)),
            vt_open: opts.is_some(),
            vt_shown: false,
            vt_parked: false,
            vt_display: None,
            vt_pointer_start: None,
            vt_width: width,
            vt_level: egui::WindowLevel::AlwaysOnTop,
        }
    }

    /// Shows a short message (toast) at the bottom of Settings; it disappears after a few seconds.
    pub fn say(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }
}

impl eframe::App for App {
    // (documented by the trait)
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0] // the window is always transparent; the settings card paints itself
    }

    /// Runs even while the window is hidden (as long as something calls request_repaint).
    /// Must not draw any UI.
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        // winit rewrites the window styles on every show / hide / click-through change; strip the
        // frame again before anything is painted (cheap, and a no-op on macOS)
        if let Some(handle) = native_handle(frame) {
            platform::clear_window_frame(handle);
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

        self.vt_logic(ctx);
        self.sleep_step(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // light or dark for this frame; Settings previews the choice before it is saved
        let appearance = if self.settings_open {
            self.edit.appearance
        } else {
            cfg().appearance
        };
        crate::ui::settings::theme::apply_appearance(ui.ctx(), appearance);
        if self.settings_open && self.welcome.is_some() {
            self.welcome_ui(ui);
        } else if self.settings_open {
            self.settings_ui(ui);
        } else {
            self.pie_ui(ui);
        }
        // the virtual trackpad's window is declared by the frames of this one
        self.vt_ui(&ui.ctx().clone());
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
        let s = welcome_size_for(egui::vec2(1024.0, 600.0));
        assert!(s.x <= 1024.0 - 2.0 * WIN_MARGIN && s.y <= 600.0 - 2.0 * WIN_MARGIN);
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
