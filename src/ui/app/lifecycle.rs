//! Sleeping and waking: what the UI frees when it has been idle, and how icons are warmed up
//! before the pie opens. The policy itself is [`Sleeper`](crate::ui::sleep::Sleeper); see
//! [`guides::architecture`](crate::guides::architecture#idle-sleep-and-caches) for the whole story.

use super::App;
use crate::config::cfg;
use crate::platform;
use crate::side;
use crate::ui::icons::{icons_to_keep_asleep, icons_to_warm};
use crate::ui::sleep::{Tick, sleep_after};
use eframe::egui;
use std::time::Instant;

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
        let after = sleep_after(cfg().sleep_after_s);
        if self.sleeper.tick(now, self.is_busy(), after) == Tick::Sleep {
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
        let keep = icons_to_keep_asleep(&cfg());
        self.ed.icons.keep_only(&keep);
        self.release_editor();
        self.release_font_atlas(ctx);
        self.toast = None;
        platform::release_unused_memory();
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
        let target = side::target();
        let icons = {
            let c = cfg();
            let mut v = icons_to_warm(&c, general, target.as_deref());
            if let Some(a) = app {
                v.extend(icons_to_warm(&c, a, target.as_deref()));
            }
            v
        };
        // how long loading the icons took: it runs while the hold timer does, so it should end
        // before the pie opens
        let _t = diag::timed!("ui", "warm", count = icons.len());
        for icon in &icons {
            let _ = self.ed.icons.get(ctx, icon);
        }
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
}
