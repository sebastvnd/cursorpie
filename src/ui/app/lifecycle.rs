//! Sleeping and waking: what the UI frees when it has been idle, and how icons are warmed up
//! before the pie opens. The policy itself is [`Sleeper`](crate::ui::sleep::Sleeper); see
//! [`guides::architecture`](crate::guides::architecture#idle-sleep-and-caches) for the whole story.

use super::App;
use crate::config::cfg;
use crate::platform;
use crate::side;
use crate::ui::icons::icons_to_warm;
use crate::ui::sleep::{Tick, sleep_after};
use eframe::egui;
use std::time::Instant;

impl App {
    /// Whether something is on screen: a pie, a preview or Settings.
    fn is_busy(&self) -> bool {
        self.settings_open || self.visible || self.pie_open || self.preview_until.is_some()
    }

    /// Advances the idle clock and, when it runs out, puts the UI to sleep. Also asks egui for one
    /// frame at the moment that will happen, because a hidden window draws nothing on its own.
    /// Cheap enough to call on every frame.
    pub(super) fn sleep_step(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        let after = sleep_after(cfg().sleep_after_s);
        if self.sleeper.tick(now, self.is_busy(), after) == Tick::Sleep {
            self.enter_sleep();
        }
        if let Some(wait) = self.sleeper.next_check(now, after) {
            ctx.request_repaint_after(wait);
        }
    }

    /// Frees what is only a cache or only needed while Settings is open, then hands the pages back
    /// to macOS. Everything is rebuilt on demand, so nothing is lost: icons are decoded again by
    /// [`App::warm`] when the trigger is pressed, and Settings copies the config when it opens.
    fn enter_sleep(&mut self) {
        // how long freeing the caches and handing the pages back to the OS took
        let _t = diag::timed!("ui", "sleep");
        self.ed.icons.clear();
        self.release_editor();
        self.toast = None;
        platform::release_unused_memory();
    }

    /// Drops the Settings working copy and everything derived from it. Called when Settings
    /// closes and when the UI sleeps; `open_settings` makes a fresh copy of the live config.
    pub(super) fn release_editor(&mut self) {
        self.edit.profiles = Vec::new();
        self.history.reset(&self.edit);
        self.ed.reset_nav();
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
