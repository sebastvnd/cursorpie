//! Updates on the UI thread: what Settings does about a newer release when it opens, and what it
//! does with the answer of a check. The decisions are plain functions of [`update`];
//! this only applies them to the UI state. The sheet that makes the offer is drawn by
//! `ui::settings::update_dialog`, the status by the About tab.

use super::App;
use crate::msg::Notifier;
use crate::update::{self, Failure, Status, Version, install};
use eframe::egui;
use std::time::SystemTime;

impl App {
    /// Runs when Settings has just opened (not the welcome window): shows what the last check found
    /// in the About tab, offers a newer release once a day, and looks again in the background when
    /// the last answer is not from today. Does nothing about checking or offering while the setting
    /// is off; *Check now* in the About tab still works then.
    pub(super) fn update_on_settings_open(&mut self, ctx: &egui::Context) {
        let Some(current) = Version::current() else {
            return;
        };
        let today = update::day_number(SystemTime::now());
        let state = update::load_state();
        self.ed.update_can_install = install::installable_here().is_some();
        // a check or an install that is still running keeps its say: its answer comes by message
        if !Self::update_busy(self.ed.update) {
            self.ed.update = match state.latest_version() {
                Some(latest) if latest > current => Status::Available(latest),
                Some(_) => Status::UpToDate,
                None => Status::Idle,
            };
        }
        let plan = update::plan(self.edit.check_updates, today, &state, current);
        if plan.offer
            && let Some(latest) = state.latest_version()
        {
            self.offer_update(latest, today);
        }
        if plan.check {
            self.start_update_check(ctx, false);
        }
    }

    /// Looks for a newer release on a worker thread; the answer arrives as `Msg::UpdateChecked`.
    /// `manual` is true for the *Check now* button. Does nothing while a check is already running.
    pub(crate) fn start_update_check(&mut self, ctx: &egui::Context, manual: bool) {
        if Self::update_busy(self.ed.update) {
            return;
        }
        self.ed.update = Status::Checking;
        let notifier = Notifier {
            tx: self.tx.clone(),
            ctx: ctx.clone(),
        };
        update::spawn_check(notifier, manual);
    }

    /// Applies the answer of a check to the About tab. A check that was asked for says so in the
    /// status bar and offers nothing. A check that ran by itself offers a newer release at most once
    /// a day, and only while the Settings window is showing: otherwise the next opening offers it.
    pub(super) fn update_checked(&mut self, manual: bool, result: Result<Version, Failure>) {
        // an install that is under way (or waits for the restart) is not disturbed by a look
        if matches!(self.ed.update, Status::Downloading(_) | Status::Ready(_)) {
            return;
        }
        let current = Version::current();
        self.ed.update = match result {
            Ok(found) if current.is_some_and(|c| found > c) => Status::Available(found),
            Ok(_) => Status::UpToDate,
            Err(why) => Status::Failed(why),
        };
        if manual {
            let text = match self.ed.update {
                Status::Available(found) => format!("CursorPie {found} is available."),
                Status::Failed(_) => "Could not check for updates.".to_string(),
                _ => "CursorPie is up to date.".to_string(),
            };
            self.say(text);
            return;
        }
        let Status::Available(found) = self.ed.update else {
            return;
        };
        let showing = self.settings_open && self.welcome.is_none() && self.closing.is_none();
        if !showing || self.ed.update_offer.is_some() {
            return;
        }
        let Some(current) = current else {
            return;
        };
        let today = update::day_number(SystemTime::now());
        if update::offer_due(found, current, &update::load_state(), today) {
            self.offer_update(found, today);
        }
    }

    /// Whether something is running or waiting that a new check must not replace: a check, the
    /// download of an update, or a downloaded update that waits for the restart.
    fn update_busy(status: Status) -> bool {
        matches!(
            status,
            Status::Checking | Status::Downloading(_) | Status::Ready(_)
        )
    }

    /// Downloads, checks and prepares the newer release on a worker thread (the *Install update*
    /// button); the answer arrives as `Msg::UpdateStaged`. Closes the offer sheet.
    pub(crate) fn start_update_install(&mut self, ctx: &egui::Context) {
        let (Status::Available(version) | Status::InstallFailed(version, _)) = self.ed.update
        else {
            return;
        };
        diag::log!("ui", "update_install");
        self.ed.update_offer = None;
        self.ed.update = Status::Downloading(version);
        self.say(format!("Downloading CursorPie {version}..."));
        let notifier = Notifier {
            tx: self.tx.clone(),
            ctx: ctx.clone(),
        };
        install::spawn_prepare(notifier);
    }

    /// Applies the answer of the download: the update is ready (and is swapped in at once unless
    /// Settings holds changes that were not saved), or it could not be installed.
    pub(super) fn update_staged(
        &mut self,
        ctx: &egui::Context,
        result: Result<install::Staged, install::InstallError>,
    ) {
        match (result, self.ed.update) {
            (Ok(staged), Status::Downloading(_)) => {
                self.ed.update = Status::Ready(staged.version);
                self.ed.update_staged = Some(staged);
                self.apply_staged_update(ctx);
            }
            (Err(why), Status::Downloading(version)) => {
                self.ed.update = Status::InstallFailed(version, why);
                self.say("Could not install the update.");
            }
            _ => {}
        }
    }

    /// Swaps the downloaded update in and quits; the swap starts the new version. With changes in
    /// Settings that were not saved nothing happens but a message: the *Restart to update* button
    /// calls this again once they are saved or cancelled.
    pub(crate) fn apply_staged_update(&mut self, ctx: &egui::Context) {
        let Some(staged) = self.ed.update_staged.clone() else {
            return;
        };
        if self.has_unsaved_changes() {
            self.say("Update ready. Save or cancel your changes, then press Restart to update.");
            return;
        }
        match install::apply(&staged) {
            Ok(()) => {
                diag::log!("ui", "update_apply");
                self.quit(ctx);
            }
            Err(why) => {
                self.ed.update_staged = None;
                self.ed.update = Status::InstallFailed(staged.version, why);
                self.say("Could not start the update.");
            }
        }
    }

    /// Opens the "update available" sheet for `latest` and remembers that today's offer was made.
    fn offer_update(&mut self, latest: Version, today: u64) {
        diag::log!("ui", "update_offered");
        self.ed.update_offer = Some(latest);
        update::mark_offered(today);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let (tx, rx) = std::sync::mpsc::channel();
        App::new(rx, tx, 200.0, None)
    }

    fn v(text: &str) -> Version {
        Version::parse(text).expect("a version")
    }

    #[test]
    fn a_check_that_was_asked_for_answers_in_the_about_tab_and_offers_nothing() {
        let mut app = app();
        app.settings_open = true;
        app.update_checked(true, Ok(v("999.0.0")));
        assert_eq!(app.ed.update, Status::Available(v("999.0.0")));
        assert!(app.ed.update_offer.is_none());
        assert!(app.toast.is_some(), "the status bar says what was found");
    }

    #[test]
    fn a_release_that_is_not_newer_means_up_to_date() {
        let mut app = app();
        app.update_checked(true, Ok(v("0.0.1")));
        assert_eq!(app.ed.update, Status::UpToDate);
        app.update_checked(true, Ok(Version::current().expect("a version")));
        assert_eq!(app.ed.update, Status::UpToDate, "the same version");
    }

    #[test]
    fn a_failed_check_is_kept_with_its_reason() {
        let mut app = app();
        app.update_checked(true, Err(Failure::Unreachable));
        assert_eq!(app.ed.update, Status::Failed(Failure::Unreachable));
        app.update_checked(false, Err(Failure::NoClient));
        assert_eq!(app.ed.update, Status::Failed(Failure::NoClient));
        assert!(app.ed.update_offer.is_none(), "a failure offers nothing");
    }

    #[test]
    fn a_look_does_not_disturb_an_install_that_is_under_way() {
        let mut app = app();
        let v = v("999.0.0");
        app.ed.update = Status::Downloading(v);
        app.update_checked(true, Err(Failure::Unreachable));
        assert_eq!(app.ed.update, Status::Downloading(v));
        app.ed.update = Status::Ready(v);
        app.update_checked(false, Ok(v));
        assert_eq!(app.ed.update, Status::Ready(v));
    }

    #[test]
    fn a_failed_install_keeps_the_version_so_the_page_can_still_be_offered() {
        let mut app = app();
        let v = v("999.0.0");
        app.ed.update = Status::Downloading(v);
        app.update_staged(
            &egui::Context::default(),
            Err(install::InstallError::Corrupt),
        );
        assert_eq!(
            app.ed.update,
            Status::InstallFailed(v, install::InstallError::Corrupt)
        );
        // an answer that nobody waits for changes nothing
        app.ed.update = Status::UpToDate;
        app.update_staged(
            &egui::Context::default(),
            Err(install::InstallError::Corrupt),
        );
        assert_eq!(app.ed.update, Status::UpToDate);
    }

    #[test]
    fn an_update_that_is_ready_waits_while_settings_has_unsaved_changes() {
        let mut app = app();
        let v = v("999.0.0");
        let staged = install::Staged {
            version: v,
            os: crate::platform::OS,
            package: "/nonexistent/pkg".into(),
            staged: "/nonexistent/new".into(),
            target: "/nonexistent/old".into(),
        };
        app.ed.update = Status::Ready(v);
        app.ed.update_staged = Some(staged);
        app.settings_open = true;
        app.opened = Some(app.edit.clone());
        app.edit.check_updates = !app.edit.check_updates;
        assert!(app.has_unsaved_changes());
        app.apply_staged_update(&egui::Context::default());
        assert!(
            !app.quitting,
            "nothing was swapped in, the app keeps running"
        );
        assert_eq!(app.ed.update, Status::Ready(v));
        assert!(app.toast.is_some());
    }

    #[test]
    fn an_automatic_answer_with_settings_closed_waits_for_the_next_opening() {
        let mut app = app();
        assert!(!app.settings_open);
        app.update_checked(false, Ok(v("999.0.0")));
        assert_eq!(app.ed.update, Status::Available(v("999.0.0")));
        assert!(app.ed.update_offer.is_none());
    }
}
