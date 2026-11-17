//! The "update available" sheet, offered when Settings opens and a newer release is known.
//!
//! It shows at most once a day (see [`update::plan`](crate::update::plan)): *Install update*
//! downloads, checks and installs the new version (see [`update::install`](crate::update::install)),
//! *Release page* opens the page in the browser, *Not now* (Esc, the close button, a click outside)
//! closes it and the About tab keeps showing the update. A copy that cannot update itself (not
//! installed in the usual place) gets *Download page* instead of *Install update*. It opens over the
//! Settings window like the other sheets, and the same on every OS.
//!
//! Two more sheets tell what happens after *Install update*, because CursorPie quits for the swap
//! and the person would otherwise see a window that just vanishes:
//!
//! * the **progress sheet** ([`App::update_progress_dialog`]) stays over Settings while the package
//!   is downloaded and checked, and says what comes next (CursorPie closes, the new version is put
//!   in place and starts by itself, which can take a few minutes);
//! * the **result sheet** ([`App::update_done_dialog`]) opens on the first start after the swap
//!   (see [`update::take_outcome`](crate::update::take_outcome)): "updated to X", or that the update
//!   did not finish and the old version is running.

use crate::about::{LATEST_RELEASE_URL, VERSION};
use crate::platform::{self, Os};
use crate::ui::app::App;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use crate::update::{Outcome, Status};
use eframe::egui::{self, Vec2};

/// What the sheet asked for this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// Nothing yet: keep showing the sheet.
    Pending,
    /// Download and install the update, and close the sheet.
    Install,
    /// Open the release page and close the sheet.
    Page,
    /// Close the sheet.
    NotNow,
}

/// What the sheet says under its title.
fn body_text(version: crate::update::Version, can_install: bool, needs_password: bool) -> String {
    let current = VERSION;
    if !can_install {
        return format!(
            "You are using {current}. This copy of CursorPie was not installed in the usual place, \
             so it cannot update itself: the page opens in your browser, where you pick the package \
             for your system."
        );
    }
    let mut text = format!(
        "You are using {current}. Install update downloads {version} from GitHub, checks it \
         against the checksum of the release, replaces this copy and starts CursorPie again. Your \
         settings are kept."
    );
    if platform::OS == Os::Mac {
        text.push_str(" macOS may ask you to allow Accessibility for CursorPie again.");
    }
    if needs_password {
        text.push_str(
            " This copy is in a folder only an administrator can write to, so your system asks \
             for your password when CursorPie restarts; if you cancel, the old version starts again.",
        );
    }
    text
}

/// What the progress sheet says while the package is downloaded and checked.
fn progress_text(needs_password: bool) -> String {
    let mut text = String::from(
        "CursorPie is downloading the update and checking it against the checksum of the release.\n\n\
         When that is done, CursorPie closes by itself, the new version is put in place and \
         CursorPie starts again. This can take a few minutes, and it is normal that nothing is on \
         screen for a moment. Please do not start CursorPie yourself or turn off the computer until \
         it is back.",
    );
    if platform::OS == Os::Windows {
        text.push_str(
            " A small \u{201c}Setup\u{201d} window shows the progress while it installs.",
        );
    }
    if needs_password {
        text.push_str(" Your system will ask for your password when CursorPie restarts.");
    }
    text
}

/// What the result sheet says: its title, the text under it and whether it is good news.
fn done_text(outcome: Outcome) -> (String, String, bool) {
    match outcome {
        Outcome::Updated { from, to } => (
            format!("CursorPie is now {to}"),
            format!(
                "The update from {from} is finished and CursorPie is running again. Your settings \
                 were kept."
            ),
            true,
        ),
        Outcome::Unfinished { from, to } => (
            format!("CursorPie {to} was not installed"),
            format!(
                "The update did not finish, so CursorPie {from} is running again and nothing was \
                 lost. You can press Install update in the About tab to try again, or download \
                 {to} from the release page."
            ),
            false,
        ),
    }
}

impl App {
    /// Draws the progress sheet while the update is being downloaded and checked. It has no answer:
    /// it ends by itself when the download does (the swap follows and CursorPie quits, or the About
    /// tab shows why it did not work), and Esc and clicks outside leave it open.
    pub(super) fn update_progress_dialog(&mut self, ctx: &egui::Context, accent: egui::Color32) {
        let Status::Downloading(version) = self.ed.update else {
            return;
        };
        let needs_password = self.ed.update_needs_password;
        let height = if needs_password { 330.0 } else { 300.0 };
        // the close button and a click outside mean nothing here: the download goes on
        let (_floating, ()) = sheet(
            ctx,
            "update_progress",
            self.ed.card,
            Vec2::new(560.0, height),
            "Updating CursorPie",
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add(egui::Spinner::new().size(18.0).color(accent));
                    ui.label(crate::ui::fonts::title(
                        format!("Downloading CursorPie {version}\u{2026}"),
                        15.0,
                        pal().text,
                    ));
                });
                ui.add_space(6.0);
                sheet_text(ui, &progress_text(needs_password));
            },
        );
    }

    /// Draws the result sheet while `Ed::update_done` is set; any way of closing it ends it.
    pub(super) fn update_done_dialog(&mut self, ctx: &egui::Context, accent: egui::Color32) {
        let Some(outcome) = self.ed.update_done else {
            return;
        };
        let (title, text, good) = done_text(outcome);
        let mut page = false;
        let mut close = false;
        let (floating, ()) = sheet(
            ctx,
            "update_done",
            self.ed.card,
            Vec2::new(520.0, 250.0),
            if good {
                "Update finished"
            } else {
                "Update not finished"
            },
            |ui| {
                let (glyph, color) = if good {
                    (symbols::CHECK_CIRCLE, pal().good)
                } else {
                    (symbols::WARNING_CIRCLE, pal().bad)
                };
                sheet_heading(ui, glyph, color, &title);
                sheet_text(ui, &text);
                ui.add_space(GAP);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if button(ui, Look::Primary(accent), None, "OK").clicked() {
                        close = true;
                    }
                    if !good && button(ui, Look::Soft, None, "Release page").clicked() {
                        page = true;
                    }
                });
            },
        );
        if page {
            diag::log!("ui", "update_download");
            platform::open_url(LATEST_RELEASE_URL);
        }
        if close || page || floating.close {
            self.ed.update_done = None;
        }
    }

    /// Draws the sheet while `Ed::update_offer` is set and applies the answer.
    pub(super) fn update_dialog(&mut self, ctx: &egui::Context, accent: egui::Color32) {
        let Some(version) = self.ed.update_offer else {
            return;
        };
        let can_install = self.ed.update_can_install;
        let mut answer = Answer::Pending;
        let (floating, ()) = sheet(
            ctx,
            "update_offer",
            self.ed.card,
            Vec2::new(
                560.0,
                if self.ed.update_needs_password {
                    340.0
                } else {
                    270.0
                },
            ),
            "Update available",
            |ui| {
                sheet_heading(
                    ui,
                    symbols::DOWNLOAD_SIMPLE,
                    accent,
                    &format!("CursorPie {version} is available"),
                );
                let text = body_text(version, can_install, self.ed.update_needs_password);
                sheet_text(ui, &text);
                ui.add_space(GAP);
                // the way forward is the rightmost button, the way back the one before it
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if can_install {
                        let install = button(
                            ui,
                            Look::Primary(accent),
                            Some(symbols::DOWNLOAD_SIMPLE),
                            "Install update",
                        );
                        if install.clicked() {
                            answer = Answer::Install;
                        }
                    }
                    if button(ui, Look::Soft, None, "Not now").clicked() {
                        answer = Answer::NotNow;
                    }
                    let (look, label) = if can_install {
                        (Look::Soft, "Release page")
                    } else {
                        (Look::Primary(accent), "Download page")
                    };
                    if button(ui, look, None, label).clicked() {
                        answer = Answer::Page;
                    }
                });
            },
        );
        // the close button and a click outside the sheet mean "not now"
        if floating.close && answer == Answer::Pending {
            answer = Answer::NotNow;
        }
        match answer {
            Answer::Pending => {}
            Answer::NotNow => self.ed.update_offer = None,
            Answer::Install => self.start_update_install(ctx),
            Answer::Page => {
                self.ed.update_offer = None;
                diag::log!("ui", "update_download");
                platform::open_url(LATEST_RELEASE_URL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_result_sheet_tells_good_news_from_bad() {
        let v = |t| crate::update::Version::parse(t).unwrap();
        let (title, _, good) = done_text(Outcome::Updated {
            from: v("0.8.19"),
            to: v("0.9.0"),
        });
        assert!(good && title.contains("0.9.0"));
        let (title, text, good) = done_text(Outcome::Unfinished {
            from: v("0.8.19"),
            to: v("0.9.0"),
        });
        assert!(!good && title.contains("0.9.0") && text.contains("0.8.19"));
    }

    #[test]
    fn the_progress_sheet_says_what_follows_the_download() {
        let text = progress_text(false);
        assert!(text.contains("closes by itself") && text.contains("starts again"));
        assert!(progress_text(true).contains("password"));
    }

    #[test]
    fn without_an_offer_nothing_is_drawn_and_nothing_changes() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        let ctx = egui::Context::default();
        app.update_dialog(&ctx, egui::Color32::GREEN);
        assert!(app.ed.update_offer.is_none());
    }
}
