//! Window management: showing the pie, opening/closing Settings, and the tray menu actions.

use super::{App, WIN_MARGIN, card_size_for, welcome_size_for};
use crate::config::{self, MAX_ITEMS, cfg};
use crate::platform;
use crate::side;
use crate::tap::PAUSED;
use crate::ui::screen::screen_rect_at;
use crate::ui::settings::{Tab, Welcome};
use eframe::egui::{self, Pos2, Vec2};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

impl App {
    /// Closes Settings and turns the window back into the hidden, click-through pie window.
    /// With `save`, the edited config is sanitized, stored, saved to disk and the launch-at-login setting is applied.
    pub fn close_settings(&mut self, ctx: &egui::Context, save: bool) {
        self.ed.stop_recording();
        if save {
            let new = self.edit.clone().sanitize();
            let login = new.launch_at_login;
            *cfg() = new.clone();
            side::forget_all(); // a changed default should apply at once
            config::save_config_with_backup(&new);
            if let Err(e) = platform::set_launch_at_login(login) {
                eprintln!("CursorPie: could not update launch-at-login: {e}");
            }
            if let Some(t) = &self.tray {
                t.set_login_checked(login);
            }
        }
        self.ed.icons.revalidate(); // re-read only the images that changed on disk
        self.release_editor(); // the next open copies the config again
        self.win = cfg().pie_size;
        self.settings_open = false;
        self.welcome = None;
        self.closing = None;
        self.settings_anim = 0.0;
        self.pie_open = false;
        self.anim = 0.0;
        self.visible = false;

        let w = self.win;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([w, w].into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        self.sleep_step(ctx); // the idle clock starts now
    }

    /// Turns the window into Settings, centered on the display that contains `at`.
    pub(super) fn open_settings(&mut self, ctx: &egui::Context, at: Pos2, about: bool) {
        if self.settings_open {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            if about {
                self.ed.tab = Tab::About;
            }
            return;
        }
        self.pie_open = false;
        self.settings_open = true;
        self.visible = true;
        self.closing = None;
        self.settings_anim = 0.0;
        self.ed.stop_recording();
        self.ed.icons.revalidate();
        self.ed.profile = 0;
        self.ed.reset_nav();
        self.ed.tab = if about { Tab::About } else { Tab::Menu };
        self.edit = cfg().clone();
        self.edit.launch_at_login = platform::launch_at_login_enabled();
        self.history.reset(&self.edit); // undo starts from what Settings opened with

        let size = self.place_card_window(ctx, at, card_size_for);
        self.card_size = size;
    }

    /// Turns the window into the welcome window, centered on the display that contains `at`.
    /// Does nothing to a window that is already showing Settings or the welcome window except
    /// bring it to the front.
    pub(super) fn open_welcome(&mut self, ctx: &egui::Context, at: Pos2) {
        if self.settings_open {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            return;
        }
        self.pie_open = false;
        self.settings_open = true;
        self.visible = true;
        self.closing = None;
        self.settings_anim = 0.0;
        self.ed.stop_recording();
        self.welcome = Some(Welcome::new(platform::launch_at_login_enabled()));
        self.card_size = self.place_card_window(ctx, at, welcome_size_for);
    }

    /// Shows the window as a normal, centered card on the display that contains `at`, `fit`
    /// deciding the card size from the size of that display. Returns the card size.
    fn place_card_window(&mut self, ctx: &egui::Context, at: Pos2, fit: fn(Vec2) -> Vec2) -> Vec2 {
        // centered on the display that contains the cursor
        let area = screen_rect_at(at)
            .unwrap_or_else(|| egui::Rect::from_center_size(at, egui::vec2(1600.0, 1000.0)));
        let card = fit(area.size());
        let win_sz = card + Vec2::splat(2.0 * WIN_MARGIN);
        let top_left = area.center() - win_sz / 2.0;
        crate::diag_log!(
            "ui",
            "place_card",
            at_x = at.x,
            at_y = at.y,
            left = top_left.x,
            top = top_left.y,
            w = win_sz.x,
            h = win_sz.y
        );
        crate::diag::expect_window(top_left.x, top_left.y);

        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(win_sz));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::Normal,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        card
    }

    /// Ends the welcome window once its close animation has finished: remembers that it was
    /// shown, applies the "Launch at login" box if the user went through to the end, and either
    /// hides the window or turns it into Settings (the "Open Settings" button).
    pub fn finish_welcome(&mut self, ctx: &egui::Context) {
        crate::config::mark_welcomed();
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
            self.settings_open = false;
            self.closing = None;
            let at = self.cursor_or_default();
            self.open_settings(ctx, at, false);
        } else {
            self.close_settings(ctx, false);
        }
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
            crate::diag_log!(
                "ui",
                "show_pie_ignored",
                reason = "settings_open",
                x = x,
                y = y
            );
            return;
        }
        self.win = cfg().pie_size;
        self.profile = profile;
        self.path = path;
        self.hover = None;
        self.hover_anim = [0.0; MAX_ITEMS];
        self.pie_open = true;
        self.fresh = true;
        self.anim = 0.0; // submenus "bloom" again, re-centered on the cursor
        self.opened_at = Instant::now();
        self.pie_pos = egui::pos2(x as f32, y as f32);

        let w = self.win;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([w, w].into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        // whole points: a window on a half-point offset is resampled by the compositor on
        // standard-density screens, which softens everything drawn in it
        let top_left = egui::pos2((x as f32 - w / 2.0).round(), (y as f32 - w / 2.0).round());
        crate::diag_log!(
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
                crate::diag::rect_str(
                    f64::from(r.min.x),
                    f64::from(r.min.y),
                    f64::from(r.width()),
                    f64::from(r.height()),
                )
            })
        );
        crate::diag::expect_window(top_left.x, top_left.y);
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
        if !self.visible {
            self.visible = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
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
        self.preview_until = Some(Instant::now() + Duration::from_millis(ms));
        // the ring colors are computed for the app the last gesture was about
        side::set_target(bundle, false);
    }

    /// Current cursor position, or a fixed fallback if the OS cannot tell.
    fn cursor_or_default(&self) -> Pos2 {
        let raw = platform::cursor_pos();
        crate::diag_log!(
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
            "quit" => self.quit(ctx),
            _ => {}
        }
    }
}
