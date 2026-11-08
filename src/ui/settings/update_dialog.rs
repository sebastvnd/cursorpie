//! The "update available" sheet, offered when Settings opens and a newer release is known.
//!
//! It shows at most once a day (see [`update::plan`](crate::update::plan)): *Install update*
//! downloads, checks and installs the new version (see [`update::install`](crate::update::install)),
//! *Release page* opens the page in the browser, *Not now* (Esc, the close button, a click outside)
//! closes it and the About tab keeps showing the update. A copy that cannot update itself (not
//! installed in the usual place) gets *Download page* instead of *Install update*. It opens over the
//! Settings window like the other sheets, and the same on every OS.

use crate::about::{LATEST_RELEASE_URL, VERSION};
use crate::platform::{self, Os};
use crate::ui::app::App;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, RichText, Vec2};

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
fn body_text(version: crate::update::Version, can_install: bool) -> String {
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
    text
}

impl App {
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
            Vec2::new(560.0, 270.0),
            "Update available",
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new(symbols::DOWNLOAD_SIMPLE.to_string())
                            .font(egui::FontId::new(18.0, crate::ui::fonts::icons()))
                            .color(accent),
                    );
                    ui.label(crate::ui::fonts::title(
                        format!("CursorPie {version} is available"),
                        15.0,
                        pal().text,
                    ));
                });
                ui.add_space(6.0);
                let text = body_text(version, can_install);
                ui.add(egui::Label::new(RichText::new(text).size(13.5).color(pal().muted)).wrap());
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
    fn without_an_offer_nothing_is_drawn_and_nothing_changes() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        let ctx = egui::Context::default();
        app.update_dialog(&ctx, egui::Color32::GREEN);
        assert!(app.ed.update_offer.is_none());
    }
}
