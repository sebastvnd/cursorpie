//! Window management: showing the pie, opening/closing Settings, and the tray menu actions.

use super::{App, WIN_MARGIN, card_size_for, welcome_size_for};
use crate::config::{self, MAX_ITEMS, cfg};
use crate::platform;
use crate::side;
use crate::tap::PAUSED;
use crate::ui::screen::{PieHold, pie_hold, screen_rect_at};
use crate::ui::settings::{Tab, Welcome};
use eframe::egui::{self, Pos2, Vec2};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

/// Longest the card waits for the window to take its size (see [`App::window_settling`]).
const SETTLE_MAX: Duration = Duration::from_millis(300);

/// Longest a window on Linux stays unmapped while it waits for its size and a clean first frame
/// (see [`App::show_window`]).
const SHOW_WAIT_MAX: Duration = Duration::from_millis(100);

/// Frames drawn empty at the final size before the window is mapped.
const SHOW_WAIT_FRAMES: u8 = 2;

/// A window that was asked to appear on Linux and is still being prepared (see `App::show_window`).
pub struct ShowWait {
    /// The window size it was asked for.
    want: Vec2,
    since: Instant,
    /// Frames seen so far with the window at that size.
    frames: u8,
    /// Where the pie's top-left corner has to be once the window is mapped (see `App::show_pie`).
    pub pos: Option<Pos2>,
}

/// Transparent frames drawn after the window was mapped (see [`App::settle_frames`]).
const SETTLE_FRAMES: u8 = 2;

/// How long the window is repainted on a timer after a file chooser closed.
const REPAINT_BURST: Duration = Duration::from_millis(800);

/// Whether a window of `size` (None = the system did not say) is the size `want`, give or take
/// the rounding to whole pixels. An unknown size counts as arrived: there is nothing to wait for.
fn has_size(size: Option<Vec2>, want: Vec2) -> bool {
    size.is_none_or(|s| (s.x - want.x).abs() <= 2.0 && (s.y - want.y).abs() <= 2.0)
}

/// Whether the shared window lets the pointer through once the pie is hidden ([`HidePie`](crate::msg::Msg::HidePie)).
///
/// The pie, the welcome card and Settings are one window. The pie is click-through; Settings takes
/// the pointer. The tap can send `HidePie` while Settings is open (the hook watchdog resetting the
/// gesture, a press that began before Settings opened and ended after it, a trackpad hold that was
/// cancelled), and making the window click-through then leaves a Settings card that shows but
/// ignores every click (Close and Save included), while the tray menu still works.
pub(super) fn passthrough_after_hide(settings_open: bool) -> bool {
    !settings_open
}

impl App {
    /// Applies the edited config and writes it to disk (with a backup), without closing Settings:
    /// the config is sanitized and stored, and the launch-at-login setting is applied.
    pub fn persist_edit(&mut self) {
        let new = self.edit.clone().sanitize();
        let login = new.launch_at_login;
        *cfg() = new.clone();
        side::forget_all(); // a changed default should apply at once
        config::save_config_with_backup(&new);
        crate::bridge::apply(new.browser_bridge); // starts or stops the browser extension link
        if let Err(e) = platform::set_launch_at_login(login) {
            eprintln!("CursorPie: could not update launch-at-login: {e}");
        }
        if let Some(t) = &self.tray {
            t.set_login_checked(login);
        }
    }

    /// Closes Settings and turns the window back into the hidden, click-through pie window.
    /// With `save`, the edited config is sanitized, stored, saved to disk and the launch-at-login setting is applied.
    pub fn close_settings(&mut self, ctx: &egui::Context, save: bool) {
        // how long closing took (saving is most of it) and whether the edits were kept
        let _t = diag::timed!("ui", "close_settings", save = save);
        crate::ui::memlog::mark("close_settings");
        self.ed.stop_recording();
        self.ed.scan = None;
        self.confirm_quit = false;
        self.confirm_discard = false;
        self.leave_flow_full(ctx);
        if save {
            self.persist_edit();
        }
        self.ed.icons.revalidate(); // re-read only the images that changed on disk
        self.release_editor(); // the next open copies the config again
        // The icon picker and the app pickers decoded hundreds of textures that only Settings
        // showed. Free them now, not after the idle time: the pie's own icons are kept and the
        // other profiles' are decoded again by `App::warm` when the trigger is pressed.
        self.ed
            .icons
            .keep_only(&crate::ui::icons::icons_to_keep_asleep(&cfg()));
        // Settings cuts glyphs at many sizes and in several fonts (text, icons, emoji fallbacks),
        // and egui never gives one back: rebuild the atlas on every close, not only after zoomed
        // text. The few glyphs of the pie are cut again when it is drawn.
        let _ = crate::ui::settings::take_crisp_used();
        crate::ui::fonts::reset_atlas(ctx);
        self.win = cfg().pie_size;
        self.set_settings_open(false);
        self.welcome = None;
        self.closing = None;
        self.settings_anim = 0.0;
        self.settings_wait = None;
        self.pie_open = false;
        self.anim = 0.0;
        self.visible = false;

        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        // The window is not hidden yet: a hidden window runs no egui pass and paints nothing, so
        // the textures and the font atlas freed above would stay allocated until the next time
        // the pie shows (the RAM of Settings stayed for as long as nobody pressed the trigger).
        // `finish_close` resizes and hides it after a few empty frames.
        self.begin_close_flush(ctx);
        self.sleep_step(ctx); // the idle clock starts now
    }

    /// Puts the window away: hides it.
    pub(crate) fn hide_root(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    /// Makes the (hidden) window appear. On Linux not at once: the window is mapped only after it
    /// has its final size and one transparent frame has been drawn into it.
    ///
    /// Why: a window that is mapped right after it was resized (or that shows what it held when it
    /// was hidden) is on screen for a moment with a buffer nobody has drawn into at the new size, and
    /// a transparent window then flashes **black**: the pie as a black square, Settings as a black
    /// rectangle, before the first real frame. The size request is carried out by the X server a
    /// moment after the frame that sent it, and the drawing surface is resized after that. So the
    /// size and position are sent now, [`App::step_show_wait`] draws nothing for a few frames (the
    /// buffer is cleared to transparent) and only then sends `Visible(true)`.
    pub(super) fn show_window(&mut self, ctx: &egui::Context, want: Vec2) {
        if platform::OS == platform::Os::Linux {
            self.show_wait = Some(ShowWait {
                want,
                since: Instant::now(),
                frames: 0,
                pos: None,
            });
            ctx.request_repaint();
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
    }

    /// Runs every frame: maps the window once [`App::show_window`]'s conditions are met.
    pub(super) fn step_show_wait(&mut self, ctx: &egui::Context) {
        let Some(wait) = self.show_wait.as_mut() else {
            return;
        };
        if !self.visible {
            // closed again before it was ever shown (a quick release): nothing to map
            self.show_wait = None;
            return;
        }
        let size = ctx.input(|i| i.viewport().inner_rect).map(|r| r.size());
        let arrived = has_size(size, wait.want);
        if arrived {
            wait.frames = wait.frames.saturating_add(1);
        }
        let waited = wait.since.elapsed();
        if (arrived && wait.frames >= SHOW_WAIT_FRAMES) || waited >= SHOW_WAIT_MAX {
            diag::log!(
                "ui",
                "show_wait_done",
                waited_ms = waited,
                arrived = arrived
            );
            let pos = wait.pos;
            self.show_wait = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.settle_frames = SETTLE_FRAMES;
            // KWin ignores the position a window had while it was hidden (a program-specified
            // position is not the user's: ICCCM 4.1.2.3) and places it itself when it is mapped.
            // A move of a window that is already mapped is honored, so ask again now, and keep
            // watching from this moment (not from the press, the map may come 100 ms later).
            if let Some(top_left) = pos
                && platform::window_manager_may_recenter()
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
                self.want_pos = Some((top_left, Instant::now()));
            }
            if self.settings_open {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(4));
        }
    }

    /// Brings the Settings (or welcome) window in front of everything: out of the minimized state,
    /// from behind other windows and from another desktop, workspace or Space. `why` names the
    /// reason for the diagnostic log.
    ///
    /// The toolkit's request is enough on macOS and Windows. On X11 the window manager refuses it
    /// while another program has the focus, so the platform asks again in its own way (see
    /// [`platform::raise_own_window`]).
    pub fn raise_settings(&self, ctx: &egui::Context, why: &'static str) {
        diag::log!(
            "ui",
            "raise_settings",
            why = why,
            visible = self.visible,
            welcome = self.welcome.is_some()
        );
        // Only a window that is minimized (or whose state is not known) is restored: the command
        // sent to a window that is already on screen makes the system re-order it for nothing.
        if ctx.input(|i| i.viewport().minimized) != Some(false) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        }
        request_focus(ctx);
        platform::raise_own_window();
    }

    /// Whether the window is still on its way to the size the card was given. While it is, the
    /// caller draws nothing and returns, and the open animation has not started.
    ///
    /// The pie is a small window that is already on screen when a held trigger opens Settings
    /// (and a hidden one when the menu bar opens it). The commands that resize and move it are
    /// carried out by the system a moment after the frame that sent them, so the first frames of
    /// the card would be drawn into a window that still has the old size: the card cut off, or
    /// stretched when the window grows, and then a jump. Worst on the first open, when the
    /// frames are slowest (nothing is cached yet and the larger drawing surface is created).
    /// Drawing nothing keeps the transparent window empty until it has arrived.
    ///
    /// Only macOS needs this (the window there is resized while it is visible); elsewhere it is
    /// always false. A window that never reaches the size (the system clamped it) is let go of
    /// after `SETTLE_MAX`.
    pub fn window_settling(&mut self, ctx: &egui::Context) -> bool {
        let Some((want, since)) = self.settings_wait else {
            return false;
        };
        let size = ctx.input(|i| i.viewport().inner_rect).map(|r| r.size());
        if platform::OS != platform::Os::Mac
            || has_size(size, want)
            || since.elapsed() >= SETTLE_MAX
        {
            diag::log!(
                "ui",
                "window_settled",
                waited_ms = since.elapsed(),
                arrived = has_size(size, want)
            );
            self.settings_wait = None;
            ctx.request_repaint(); // draw the card now
            return false;
        }
        ctx.request_repaint_after(Duration::from_millis(8));
        true
    }

    /// Turns the window into Settings, centered on the display that contains `at`.
    pub(super) fn open_settings(&mut self, ctx: &egui::Context, at: Pos2, about: bool) {
        diag::log!(
            "ui",
            "open_settings",
            about = about,
            already_open = self.settings_open
        );
        if self.settings_open {
            self.raise_settings(ctx, "open_settings");
            if about {
                self.ed.tab = Tab::About;
            }
            return;
        }
        crate::ui::memlog::mark("open_settings");
        let was_visible = self.visible;
        self.drop_hud(ctx);
        self.drop_panel(ctx, true);
        self.pie_open = false;
        self.set_settings_open(true);
        self.visible = true;
        self.closing = None;
        self.settings_anim = 0.0;
        self.ed.stop_recording();
        self.ed.scan = None;
        self.ed.icons.revalidate();
        self.ed.profile = 0;
        self.ed.reset_nav();
        // a fresh start: first tab (or About), and every page scrolled to the top
        self.ed.session = self.ed.session.wrapping_add(1);
        self.ed.tab = if about { Tab::About } else { Tab::Menu };
        self.edit = cfg().clone();
        self.edit.launch_at_login = platform::launch_at_login_enabled();
        self.history.reset(&self.edit); // undo starts from what Settings opened with
        self.opened = Some(self.edit.clone()); // what "unsaved" is measured against

        let size = self.place_card_window(ctx, at, card_size_for, was_visible);
        self.card_size = size;
        self.update_on_settings_open(ctx);
    }

    /// Puts the Flow tab on the whole display, or back into the window it was in.
    pub(crate) fn toggle_flow_full(&mut self, ctx: &egui::Context) {
        if self.ed.flow_full.is_some() {
            self.leave_flow_full(ctx);
            return;
        }
        let outer_rect = ctx.input(|i| i.viewport().outer_rect);
        self.ed.flow_full = Some(crate::ui::settings::FlowFull {
            card: self.card_size,
            outer: outer_rect.map(|r| r.min),
        });
        // The window is made as big as the display it is on, the way it is made the size of the
        // card (a native full screen does not work for a transparent window without a frame on
        // every system, and on macOS it leaves the window as it was).
        let area = outer_rect.and_then(|r| screen_rect_at(r.center()));
        if let Some(area) = area {
            // what the system draws over the display (the menu bar and the notch of a Mac, the
            // taskbar of Windows) is left out, or the top (or bottom) of the page is hidden
            let (top, bottom) = match platform::OS {
                platform::Os::Mac => (38.0, 0.0),
                platform::Os::Windows => (0.0, 48.0),
                platform::Os::Linux => (0.0, 0.0),
            };
            let area = egui::Rect::from_min_max(
                area.min + egui::vec2(0.0, top),
                area.max - egui::vec2(0.0, bottom),
            );
            let c = area.center();
            let (cx, cy) = platform::window_point(f64::from(c.x), f64::from(c.y));
            let top_left = egui::pos2(cx as f32, cy as f32) - area.size() / 2.0;
            // nothing is drawn until the window has the new size (see `App::window_settling`),
            // so there is no frame of the old card stretched over the display
            self.settings_wait = Some((area.size(), Instant::now()));
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(area.size()));
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
        }
        ctx.request_repaint();
    }

    /// Leaves the full screen of the Flow tab: the window goes back to the size and place it had.
    /// Returns whether it was in full screen.
    pub(crate) fn leave_flow_full(&mut self, ctx: &egui::Context) -> bool {
        let Some(before) = self.ed.flow_full.take() else {
            return false;
        };
        let win = before.card + Vec2::splat(2.0 * WIN_MARGIN);
        self.settings_wait = Some((win, Instant::now()));
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
        if let Some(at) = before.outer {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(at));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(win));
        ctx.request_repaint();
        true
    }

    /// Turns the window into the welcome window, centered on the display that contains `at`.
    /// Does nothing to a window that is already showing Settings or the welcome window except
    /// bring it to the front.
    pub(super) fn open_welcome(&mut self, ctx: &egui::Context, at: Pos2) {
        if self.settings_open {
            self.raise_settings(ctx, "open_welcome");
            return;
        }
        let was_visible = self.visible;
        self.drop_hud(ctx);
        self.drop_panel(ctx, true);
        self.pie_open = false;
        self.set_settings_open(true);
        self.visible = true;
        self.closing = None;
        self.settings_anim = 0.0;
        self.ed.stop_recording();
        self.ed.scan = None;
        self.welcome = Some(Welcome::new(platform::launch_at_login_enabled()));
        self.card_size = self.place_card_window(ctx, at, welcome_size_for, was_visible);
    }

    /// Shows the window as a normal, centered card on the display that contains `at`, `fit`
    /// deciding the card size from the size of that display. Returns the card size.
    fn place_card_window(
        &mut self,
        ctx: &egui::Context,
        at: Pos2,
        fit: fn(Vec2) -> Vec2,
        was_visible: bool,
    ) -> Vec2 {
        // centered on the display that contains the cursor
        let area = screen_rect_at(at)
            .unwrap_or_else(|| egui::Rect::from_center_size(at, egui::vec2(1600.0, 1000.0)));
        let card = fit(area.size());
        let win_sz = card + Vec2::splat(2.0 * WIN_MARGIN);
        let center = area.center();
        let (cx, cy) = platform::window_point(f64::from(center.x), f64::from(center.y));
        let top_left = egui::pos2(cx as f32, cy as f32) - win_sz / 2.0;
        diag::log!(
            "ui",
            "place_card",
            at_x = at.x,
            at_y = at.y,
            left = top_left.x,
            top = top_left.y,
            w = win_sz.x,
            h = win_sz.y
        );
        crate::debug::expect_window(top_left.x, top_left.y);
        self.settings_wait = Some((win_sz, Instant::now()));

        if was_visible && platform::OS == platform::Os::Linux {
            // the pie window is on screen (a held trigger opened Settings): resizing it while it is
            // mapped shows a black frame, so it goes away and comes back at the new size
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(win_sz));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::Normal,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
        self.show_window(ctx, win_sz);
        request_focus(ctx);
        // on X11 the toolkit drops that request until the window is mapped, and a window manager
        // may map it without the focus: ask again once it is listed
        platform::raise_own_window();
        card
    }

    /// Ends the welcome window once its close animation has finished: remembers that it was
    /// shown, applies the "Launch at login" box if the user went through to the end, and either
    /// hides the window or turns it into Settings (the "Open Settings" button).
    pub fn finish_welcome(&mut self, ctx: &egui::Context) {
        crate::config::mark_welcomed();
        // a trackpad found at the first start switched its hold on; this is where it is written
        crate::tap::save_first_start_trackpad();
        let (apply, login, open_settings) = match &self.welcome {
            Some(w) => (w.apply, w.launch_at_login, w.open_settings_after),
            None => (false, false, false),
        };
        if apply && login != platform::launch_at_login_enabled() {
            match platform::set_launch_at_login(login) {
                Ok(()) => {
                    let snapshot = {
                        let mut c = cfg();
                        c.launch_at_login = login;
                        c.clone()
                    };
                    config::save_config(&snapshot);
                    if let Some(t) = &self.tray {
                        t.set_login_checked(login);
                    }
                }
                Err(e) => eprintln!("CursorPie: could not update launch-at-login: {e}"),
            }
        }
        if open_settings {
            // straight into Settings, without hiding the window in between
            self.welcome = None;
            self.set_settings_open(false);
            self.closing = None;
            let at = self.cursor_or_default();
            self.open_settings(ctx, at, false);
        } else {
            self.close_settings(ctx, false);
        }
    }

    /// Mirrors `settings_open` into [`tap::SETTINGS_OPEN`](crate::tap::SETTINGS_OPEN), which the
    /// tap thread reads to keep the pie off. Every change of `settings_open` goes through here.
    pub(super) fn set_settings_open(&mut self, open: bool) {
        self.settings_open = open;
        crate::tap::SETTINGS_OPEN.store(open, std::sync::atomic::Ordering::SeqCst);
    }

    /// The trigger was pressed while Settings is open, so the pie did not open. Settings may be
    /// behind other windows, minimized or on another desktop: without a sign the press looks like it
    /// did nothing. Brings the window forward and says why.
    ///
    /// A window that already has the focus is not raised again, and a trackpad hold that lands on
    /// it says nothing at all. On a trackpad every pause of the finger before a click is a "hold"
    /// to the recognizer, and the raise is not harmless: on Windows the toolkit takes the
    /// foreground by injecting an Alt key press, which turns the next click into a menu
    /// activation. The person then had to click twice, or hold for a long time, to get anything
    /// done in Settings.
    pub(super) fn pie_blocked(&mut self, ctx: &egui::Context, _x: f64, _y: f64, touch: bool) {
        let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
        let reaction = blocked_reaction(touch, focused);
        diag::log!(
            "ui",
            "pie_blocked",
            touch = touch,
            focused = focused,
            raise = reaction == Blocked::Raise
        );
        if reaction == Blocked::Quiet {
            return;
        }
        // (not while a file chooser is open: it would end up under Settings)
        if reaction == Blocked::Raise && !crate::ui::settings::choosing() {
            self.raise_settings(ctx, "pie_press");
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
        self.say("The pie is off while Settings is open  \u{b7}  Save or Cancel to use it again");
    }

    /// Opens the pie window for `profile` at (x, y). Submenus call this again to re-center.
    pub(super) fn show_pie(
        &mut self,
        ctx: &egui::Context,
        x: f64,
        y: f64,
        profile: usize,
        path: Vec<usize>,
    ) {
        if self.settings_open {
            // defense in depth: the tap does not ask for a pie while Settings is open
            diag::log!(
                "ui",
                "show_pie_ignored",
                reason = "settings_open",
                x = x,
                y = y
            );
            self.pie_blocked(ctx, x, y, false);
            return;
        }
        crate::ui::memlog::mark("show_pie");
        // the pie takes the window over from a notice or a panel that is still on screen
        self.drop_hud(ctx);
        self.drop_panel(ctx, false);
        self.win = cfg().pie_size;
        self.profile = profile;
        self.path = path;
        self.hover = None;
        self.hover_anim = [0.0; MAX_ITEMS];
        self.badge_in = [0.0; MAX_ITEMS];
        self.badge_pick = [[0.0; 2]; MAX_ITEMS];
        self.badge_step = [[0.0; 2]; MAX_ITEMS];
        self.pie_open = true;
        self.fresh = true;
        self.pie_frames = 0;
        self.anim = 0.0; // submenus "bloom" again, re-centered on the cursor
        self.opened_at = Instant::now();
        self.pie_pos = egui::pos2(x as f32, y as f32);

        let w = crate::geometry::window_size(self.win);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([w, w].into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        // A mouse hold has a button down, so the apps under the pie hear no pointer motion. A
        // trackpad hold has none: the finger moves the real pointer, and a click-through pie lets
        // every move reach the page below (a video player shows its controls, links highlight).
        // So the pie of a trackpad hold takes the pointer; the pie follows the cursor, so the
        // pointer stays inside it. Clicks are swallowed by the taps as before.
        let takes_pointer = crate::tap::touch_pie_active() || mouse_pie_takes_pointer();
        diag::log!("ui", "pie_input", takes_pointer = takes_pointer);
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(!takes_pointer));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        // whole points: a window on a half-point offset is resampled by the compositor on
        // standard-density screens, which softens everything drawn in it
        let (wx, wy) = platform::window_point(x, y);
        let top_left = egui::pos2((wx as f32 - w / 2.0).round(), (wy as f32 - w / 2.0).round());
        diag::log!(
            "ui",
            "show_pie",
            x = x,
            y = y,
            win = w,
            left = top_left.x,
            top = top_left.y,
            ppp = ctx.pixels_per_point(),
            visible_before = self.visible,
            mon = screen_rect_at(self.pie_pos).map_or_else(String::new, |r| {
                diag::rect_str(
                    f64::from(r.min.x),
                    f64::from(r.min.y),
                    f64::from(r.width()),
                    f64::from(r.height()),
                )
            })
        );
        crate::debug::expect_window(top_left.x, top_left.y);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
        if !self.visible {
            self.visible = true;
            self.show_window(ctx, Vec2::splat(w));
            if let Some(wait) = self.show_wait.as_mut() {
                wait.pos = Some(top_left); // asked again once the window is mapped
            }
        }
        // A hidden window gets its next frame only if somebody asks for it. Without this the pie
        // could be "open" for as long as nothing else woke the window (the first press after a long
        // idle time), and the hold timer then opened Settings instead.
        ctx.request_repaint();
        if platform::window_manager_may_recenter() && self.show_wait.is_none() {
            // Once mapped, a move is a request from the client and the window manager honors
            // it; the position given while the window was hidden is not always kept. (A window
            // that is still waiting to be mapped is moved by `step_show_wait` when it is.)
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
            self.want_pos = Some((top_left, Instant::now()));
            ctx.request_repaint();
        }
    }

    /// Starts repainting on a timer for [`REPAINT_BURST`] (see `Msg::Refocus`).
    pub(super) fn start_repaint_burst(&mut self, ctx: &egui::Context) {
        self.repaint_until = Some(Instant::now() + REPAINT_BURST);
        ctx.request_repaint();
    }

    /// Runs every frame: keeps the window repainting while a burst is on.
    pub(super) fn step_repaint_burst(&mut self, ctx: &egui::Context) {
        let Some(until) = self.repaint_until else {
            return;
        };
        if Instant::now() >= until {
            self.repaint_until = None;
        } else {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }

    /// For a short time after the pie was shown, moves the window back to where it was asked to
    /// be if the window manager put it somewhere else (see [`App::want_pos`]).
    pub(super) fn enforce_pie_pos(&mut self, ctx: &egui::Context) {
        const HOLD: Duration = Duration::from_millis(300);
        let Some((target, since)) = self.want_pos else {
            return;
        };
        if self.settings_open || (!self.pie_open && self.hud.is_none() && self.panel.is_none()) {
            self.want_pos = None;
            return;
        }
        let outer_min = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min);
        match pie_hold(target, outer_min, since.elapsed(), HOLD) {
            PieHold::Release => {
                self.want_pos = None;
                return;
            }
            PieHold::Move => {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(target));
            }
            PieHold::Keep => {
                // The position read back may be the one winit stored while the window was hidden,
                // not the one the window manager gave it: for the first moments say it again.
                if since.elapsed() < Duration::from_millis(250) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(target));
                }
            }
        }
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    /// Menu bar item: switch which pie the side buttons follow in the app in front.
    fn swap_side(&mut self, ctx: &egui::Context) {
        let bundle = platform::frontmost_bundle_id();
        let (switched, general, ms) = {
            let c = cfg();
            (
                side::toggle(&c, bundle.as_deref()),
                c.global_profile(),
                c.preview_ms,
            )
        };
        // an app without a pie of its own has nothing to switch: just show the one pie there is
        let profile = switched.unwrap_or(general);
        if ms == 0 {
            return;
        }
        let at = self.cursor_or_default();
        let center = screen_rect_at(at).map_or(at, |r| r.center());
        self.show_pie(ctx, center.x as f64, center.y as f64, profile, Vec::new());
        // a preview is not a hold: nobody is using the pie, so it never takes the pointer
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        self.preview_until = Some(Instant::now() + Duration::from_millis(ms));
        // the ring colors are computed for the app the last gesture was about
        side::set_target(bundle, false);
    }

    /// Current cursor position, or a fixed fallback if the OS cannot tell.
    pub(super) fn cursor_or_default(&self) -> Pos2 {
        let raw = platform::cursor_pos();
        diag::log!(
            "ui",
            "cursor_or_default",
            ok = raw.is_some(),
            x = raw.map_or(400.0, |p| p.0),
            y = raw.map_or(300.0, |p| p.1)
        );
        raw.map(|(x, y)| egui::pos2(x as f32, y as f32))
            .unwrap_or(egui::pos2(400.0, 300.0))
    }

    /// Quits the app. The close request may never be processed while the window is hidden, so a
    /// short-lived thread exits the process as a fallback.
    pub fn quit(&mut self, ctx: &egui::Context) {
        self.quitting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(1500));
            std::process::exit(0);
        });
    }

    /// Handles a menu bar item click by its id (see `ui::tray`).
    pub(super) fn handle_tray(&mut self, ctx: &egui::Context, id: &str) {
        match id {
            "settings" => {
                let at = self.cursor_or_default();
                self.open_settings(ctx, at, false);
            }
            "about" => {
                let at = self.cursor_or_default();
                self.open_settings(ctx, at, true);
            }
            "welcome" => {
                let at = self.cursor_or_default();
                self.open_welcome(ctx, at);
            }
            "swap_side" => self.swap_side(ctx),
            "pause" => {
                if let Some(t) = &self.tray {
                    PAUSED.store(t.pause_checked(), Ordering::SeqCst);
                }
            }
            "login" => {
                if let Some(t) = &self.tray
                    && let Err(e) = platform::set_launch_at_login(t.login_checked())
                {
                    eprintln!("CursorPie: could not update launch-at-login: {e}");
                }
            }
            "quit" => self.request_quit(ctx),
            _ => {}
        }
    }
}

/// Asks the toolkit to give the window the keyboard focus.
///
/// Not on Windows: there winit takes the foreground by injecting an Alt key press, and the next
/// click is then eaten as the end of a menu activation (the first click, and so the first hold of
/// the pie, did nothing after Settings had been opened). `platform::raise_own_window` brings the
/// window forward there without any injected key, so the request is left to it.
pub(super) fn request_focus(ctx: &egui::Context) {
    if platform::OS != platform::Os::Windows {
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
}

/// Whether the pie of a mouse hold also takes the pointer instead of letting it through.
///
/// In a Wayland session the swallowed trigger does not hold the pointer for the pie, as an X11
/// grab or a pressed button does elsewhere: the window below the pie stays under the pointer. It
/// sees the pointer move, and, worse, it keeps the say over the cursor image: a terminal, an editor,
/// a browser or a video player that hid the cursor (after typing, or after a moment without
/// movement) keeps it hidden while the pie is open, so the person sees a pie and no cursor, or one
/// that is cut off by the pie, and does not know where the pointer is. The pie window under the
/// pointer shows an ordinary arrow and the app below hears nothing (`WAYLAND_SUPPORT.md`, "Cursor
/// keeps moving during the hold"). The window lets the pointer through again when the pie hides
/// ([`Msg::HidePie`](crate::msg::Msg::HidePie)).
fn mouse_pie_takes_pointer() -> bool {
    crate::platform::linux::wayland::active()
}

/// What a blocked pie asks of the Settings window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Blocked {
    /// Nothing: the person is using Settings with a trackpad.
    Quiet,
    /// Only the message: Settings is in front already.
    Say,
    /// Bring Settings forward and say why.
    Raise,
}

/// Decides how to answer a blocked pie from where it came from and whether Settings has the focus.
fn blocked_reaction(touch: bool, focused: bool) -> Blocked {
    match (touch, focused) {
        (true, true) => Blocked::Quiet,
        (false, true) => Blocked::Say,
        (_, false) => Blocked::Raise,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hiding_the_pie_never_makes_an_open_settings_window_click_through() {
        assert!(super::passthrough_after_hide(false));
        assert!(!super::passthrough_after_hide(true));
    }

    use super::*;

    #[test]
    fn a_trackpad_pause_does_not_raise_a_settings_window_that_has_the_focus() {
        assert_eq!(blocked_reaction(true, true), Blocked::Quiet);
        assert_eq!(blocked_reaction(false, true), Blocked::Say);
        // behind another window, both triggers still bring Settings back
        assert_eq!(blocked_reaction(true, false), Blocked::Raise);
        assert_eq!(blocked_reaction(false, false), Blocked::Raise);
    }

    #[test]
    fn a_window_has_arrived_when_it_has_the_size_asked_for() {
        let want = Vec2::new(1260.0, 820.0);
        assert!(has_size(Some(Vec2::new(1260.0, 820.0)), want));
        assert!(
            has_size(Some(Vec2::new(1259.0, 821.0)), want),
            "pixel rounding"
        );
        assert!(
            !has_size(Some(Vec2::new(400.0, 400.0)), want),
            "still the pie"
        );
        assert!(!has_size(Some(Vec2::new(1260.0, 400.0)), want));
        assert!(has_size(None, want), "nothing to compare: do not wait");
    }
}
