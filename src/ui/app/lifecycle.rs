//! Sleeping and waking: what the UI frees when it has been idle, and how icons are warmed up
//! before the pie opens. The policy itself is [`Sleeper`](crate::ui::sleep::Sleeper); see
//! [`guides::architecture`](crate::guides::architecture#idle-sleep-and-caches) for the whole story.

use super::App;
use crate::config::cfg;
use crate::platform;
use crate::side;
use crate::ui::appicon;
use crate::ui::icons::{icons_on_first_frame, icons_to_keep_asleep, icons_to_warm};
use crate::ui::sleep::{Tick, sleep_after};
use eframe::egui;
use std::time::{Duration, Instant};

/// Empty frames drawn after Settings closed, before the window is hidden. The first one applies
/// the new font atlas (the old one is dropped) and sends the free of every dropped texture to the
/// renderer; the others make sure it has been carried out before the heap is trimmed.
const CLOSE_FLUSH_FRAMES: u8 = 3;

/// Longest the window waits for those frames (a window the system does not draw must not stay
/// on screen).
const CLOSE_FLUSH_MAX: Duration = Duration::from_millis(300);

impl App {
    /// Whether something is on screen: a pie, a preview or Settings.
    fn is_busy(&self) -> bool {
        self.settings_open
            || self.visible
            || self.pie_open
            || self.preview_until.is_some()
            || self.hud.is_some()
            || self.panel.is_some()
            || !self.panel_queue.is_empty()
    }

    /// Advances the idle clock and, when it runs out, puts the UI to sleep. Also asks egui for one
    /// frame at the moment that will happen, because a hidden window draws nothing on its own.
    /// Cheap enough to call on every frame.
    pub(super) fn sleep_step(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let busy = self.is_busy();
        // the thread that gives freed memory back soon after a press never works while the pie or
        // Settings is on screen, and starts its wait when they go away (see `ui::trim`)
        crate::ui::trim::set_busy(busy);
        let after = sleep_after(cfg().sleep_after_s);
        if self.sleeper.tick(now, busy, after) == Tick::Sleep {
            self.enter_sleep(ctx);
        }
        if let Some(wait) = self.sleeper.next_check(now, after) {
            ctx.request_repaint_after(wait);
        }
    }

    /// Frees what is only a cache or only needed while Settings is open, then hands the freed
    /// pages back to the OS. Everything is rebuilt on demand, so nothing is lost: Settings copies
    /// the config when it opens, and the icons of other profiles are decoded again by
    /// [`App::warm`] when the trigger is pressed.
    ///
    /// The icons of the general pie are **kept**. They are a few dozen KiB, and dropping them made
    /// the first pie after a long idle time decode them while the hold timer ran (a multi-size TIFF
    /// per app icon on macOS), so the pie opened late.
    fn enter_sleep(&mut self, ctx: &egui::Context) {
        // how long freeing the caches and handing the pages back to the OS took
        let _t = diag::timed!("ui", "sleep");
        crate::ui::memlog::mark("sleep_begin");
        let keep = icons_to_keep_asleep(&cfg());
        self.ed.icons.keep_only(&keep);
        self.release_editor();
        self.release_font_atlas(ctx);
        self.toast = None;
        platform::release_unused_memory();
        crate::ui::memlog::mark("sleep_end");
        self.ed.icons.log_stats("sleep_end");
    }

    /// Starts the flush that ends [`App::close_settings`]: the window stays on screen, click-through
    /// and empty, for [`CLOSE_FLUSH_FRAMES`] frames (about 50 ms, nothing is drawn so nobody sees it).
    ///
    /// eframe runs **no egui pass and paints nothing while the window is hidden**, only `logic`.
    /// What a pass has to do for memory is therefore never done for a hidden window: freed
    /// textures stay allocated until the renderer is told, the font atlas that `reset_atlas`
    /// replaced stays until the next pass starts, and `malloc_trim` run before that finds nothing
    /// to give back. So after Settings (400 MB) the process kept its size for as long as nobody
    /// pressed the trigger, and a press that never opened the pie, which shows the window for a
    /// frame, made it fall.
    pub(super) fn begin_close_flush(&mut self, ctx: &egui::Context) {
        self.close_flush = Some((CLOSE_FLUSH_FRAMES, Instant::now()));
        ctx.request_repaint();
    }

    /// Runs in a frame that draws (see `App::ui`): counts one empty frame and, after the last one,
    /// finishes closing.
    pub(super) fn step_close_flush(&mut self, ctx: &egui::Context) {
        let Some((frames, since)) = self.close_flush else {
            return;
        };
        let frames = frames.saturating_sub(1);
        if frames == 0 {
            self.finish_close(ctx);
        } else {
            self.close_flush = Some((frames, since));
            ctx.request_repaint();
        }
    }

    /// Runs every frame, drawn or not: gives up the flush when something else wants the window
    /// (Settings or a pie opened again in the meantime), and ends it if the frames do not come.
    pub(super) fn watch_close_flush(&mut self, ctx: &egui::Context) {
        let Some((_, since)) = self.close_flush else {
            return;
        };
        if self.is_busy() {
            self.close_flush = None; // whoever took the window sizes and shows it themselves
        } else if since.elapsed() >= CLOSE_FLUSH_MAX {
            self.finish_close(ctx);
        } else {
            ctx.request_repaint_after(CLOSE_FLUSH_MAX.saturating_sub(since.elapsed()));
        }
    }

    /// The end of [`App::close_settings`]: the window goes back to the size of the pie, is hidden,
    /// and the memory that Settings freed is handed back to the OS (now that the renderer has let
    /// go of it, which is why this does not happen in `close_settings` itself).
    fn finish_close(&mut self, ctx: &egui::Context) {
        let _t = diag::timed!("ui", "finish_close");
        self.close_flush = None;
        let w = crate::geometry::window_size(self.win);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([w, w].into()));
        self.hide_root(ctx);
        platform::release_unused_memory();
        crate::ui::memlog::mark("settings_closed");
        self.ed.icons.log_stats("settings_closed");
        self.sleep_step(ctx); // the idle clock starts now
    }

    /// Rebuilds the font atlas if the Flow canvas or the panel designer cut glyphs at zoomed
    /// sizes since the last time. egui never evicts a glyph, so without this every size a zoom
    /// passed through stays allocated (CPU and GPU) after Settings is closed. Does nothing, and
    /// costs nothing, when no zoomed text was drawn, so the pie's own glyphs are not touched.
    pub(super) fn release_font_atlas(&mut self, ctx: &egui::Context) {
        if crate::ui::settings::take_crisp_used() {
            crate::ui::fonts::reset_atlas(ctx);
        }
    }

    /// Drops the Settings working copy and everything derived from it. Called when Settings
    /// closes and when the UI sleeps; `open_settings` makes a fresh copy of the live config.
    pub(super) fn release_editor(&mut self) {
        self.edit.profiles = Vec::new();
        // the flows are as big as the profiles, and `history.reset` below would keep two more
        // copies of them (its base and the last seen value) for as long as the app runs
        self.edit.flows = Vec::new();
        self.opened = None;
        self.run_report = None;
        self.history.reset(&self.edit);
        self.ed.reset_nav();
        // sheets over the page (the new flow sheet, the panel designer with its own copy of a
        // panel, the update offer) and the Flow tab's per-session state that points into `edit`
        self.ed.close_sheet();
        self.ed.release_flow_state();
        self.ed.user_icons = Vec::new();
        self.ed.icons_not_listed = 0;
        self.ed.backups = Vec::new();
        // the list of installed apps (name, id and search text of every app on the machine) is
        // only the app picker's; the next picker reads it again
        self.ed.installed = Vec::new();
        self.ed.installed_scan = crate::ui::settings::AppScan::Idle;
        self.ed.app_query = String::new();
        // the pictures of panel buttons are a cache of the window thread: they are read again
        crate::ui::settings::panel_form::release_pics();
        // marks of flows that were deleted would stay for as long as the app runs
        {
            let c = cfg();
            self.ed
                .flow_marks
                .retain(|id, _| c.flows.iter().any(|f| f.id == *id));
        }
        self.ed.icon_query = String::new();
        self.ed.new_app = String::new();
        self.ed.new_profile_name = String::new();
        self.ed.new_profile_app = String::new();
    }

    /// Loads the icons the pie of `general` (and the app's own pie, if any) will draw first, so the
    /// frame that opens the pie only paints. Runs when the trigger is pressed, which is
    /// `pie_hold_ms` before the pie appears. Already cached icons cost only a lookup.
    pub(super) fn warm(&mut self, ctx: &egui::Context, general: usize, app: Option<usize>) {
        if self.settings_open {
            return;
        }
        // a press is activity: without this, the first press after a long idle time would load
        // the icons and then have them dropped again by the sleep that was already due
        self.sleeper.poke(Instant::now());
        crate::ui::memlog::mark("warm_begin");
        let target = side::target();
        let (icons, first) = {
            let c = cfg();
            let mut v = icons_to_warm(&c, general, target.as_deref());
            let mut f = icons_on_first_frame(&c, general, target.as_deref());
            if let Some(a) = app {
                v.extend(icons_to_warm(&c, a, target.as_deref()));
                f.extend(icons_on_first_frame(&c, a, target.as_deref()));
            }
            (v, f)
        };
        // how long loading the icons took: it runs while the hold timer does, so it should end
        // before the pie opens
        let _t = diag::timed!("ui", "warm", count = icons.len());
        for icon in &icons {
            if appicon::is_app_icon(icon) && !first.contains(icon) {
                // An app icon of a submenu: nothing paints it until the pointer stays on a
                // slice, so it is read on the worker thread, one at a time, instead of here.
                // Reading a dozen of them in the hold timer was the biggest peak of a press.
                let _ = self.ed.icons.get_app_async(ctx, icon);
            } else {
                let _ = self.ed.icons.get(ctx, icon);
            }
        }
        self.ed.icons.log_stats("warm_end");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Flow};

    #[test]
    fn releasing_the_editor_drops_the_flows_and_what_points_into_them() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        app.edit = Config {
            flows: vec![Flow::new("flow-1", "Demo")],
            ..Config::default()
        };
        app.ed.flow_sel = Some(3);
        app.ed.flow_var = Some("x".into());
        app.ed.flow_search = "wait".into();
        app.ed.flow_new = Some(crate::ui::settings::FlowNew::Ai);
        app.release_editor();
        assert!(
            app.edit.flows.is_empty(),
            "the flows of the working copy are dropped"
        );
        assert!(app.ed.flow_sel.is_none() && app.ed.flow_var.is_none());
        assert!(app.ed.flow_search.is_empty());
        assert!(
            app.ed.flow_new.is_none(),
            "a sheet over the page does not outlive Settings"
        );
    }

    #[test]
    fn closing_settings_flushes_a_few_empty_frames_before_the_window_hides() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        let ctx = egui::Context::default();
        app.begin_close_flush(&ctx);
        for _ in 1..CLOSE_FLUSH_FRAMES {
            app.step_close_flush(&ctx);
            assert!(app.close_flush.is_some(), "the window waits for its frames");
        }
        app.step_close_flush(&ctx);
        assert!(
            app.close_flush.is_none(),
            "after the last frame it is let go"
        );
    }

    #[test]
    fn a_window_someone_else_needs_is_not_hidden_by_the_flush() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        let ctx = egui::Context::default();
        app.begin_close_flush(&ctx);
        app.settings_open = true; // Settings was opened again before the frames were drawn
        app.watch_close_flush(&ctx);
        assert!(app.close_flush.is_none());
    }

    #[test]
    fn a_flush_whose_frames_never_come_ends_on_time() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        let ctx = egui::Context::default();
        app.close_flush = Some((CLOSE_FLUSH_FRAMES, Instant::now() - CLOSE_FLUSH_MAX));
        app.watch_close_flush(&ctx);
        assert!(app.close_flush.is_none());
    }

    #[test]
    fn releasing_the_editor_drops_the_list_of_installed_apps() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        app.ed.installed = vec![crate::platform::installed::InstalledApp::default(); 3];
        app.ed.installed_scan = crate::ui::settings::AppScan::Done;
        app.ed.app_query = "code".into();
        app.release_editor();
        assert!(app.ed.installed.is_empty() && app.ed.app_query.is_empty());
        assert_eq!(app.ed.installed_scan, crate::ui::settings::AppScan::Idle);
    }
}
