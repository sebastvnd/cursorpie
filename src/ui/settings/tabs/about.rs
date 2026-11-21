//! About tab: logo and version, project links, author, privacy note, credits.
//!
//! A hero panel on top (logo, name, version, link buttons), then one column of panels.

use crate::about::*;
use crate::platform::{self, ReinstallError};
use crate::ui::app::App;
use crate::ui::fonts;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use crate::update::Status;
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

/// Opens `url` (or a `mailto:` address) with the system's default handler.
///
/// This goes through [`platform::open_url`] (`xdg-open` on Linux, `ShellExecute` on Windows, `open` on
/// macOS), not through egui's `Context::open_url`: CursorPie builds eframe with its default features
/// off on Linux and Windows (see `Cargo.toml`), which drops eframe's `web_browser` feature, and
/// without it egui's `open_url` does nothing at all. macOS keeps the default features, which is why
/// the links worked only there. Only the link target is logged, and these targets are the project's
/// own constants (see [`crate::about`]).
fn open_link(url: &str) {
    diag::log!("ui", "open_link");
    platform::open_url(url);
}

/// One "label  link" row.
fn link_row(ui: &mut egui::Ui, label: &str, text: &str, url: &str) {
    info_row(ui, label, |ui| {
        if ui
            .link(text)
            .on_hover_text(url)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            open_link(url);
        }
    });
}

/// Diameter of the round link buttons in the hero card.
const LINK_SIZE: f32 = 32.0;

/// A round icon button that opens `url` in the browser. The tooltip names it and shows the address.
fn link_button(ui: &mut egui::Ui, icon: char, text: &str, url: &str) {
    if round_button(
        ui,
        Look::Soft,
        icon,
        &format!("{text}\n{url}"),
        true,
        LINK_SIZE,
    )
    .clicked()
    {
        open_link(url);
    }
}

impl App {
    /// Draws the About tab: the hero card, the updates panel, then the panels made from the
    /// constants in [`crate::about`]. `accent` colors the version badge and the primary button.
    pub(in crate::ui::settings) fn about_tab(&mut self, ui: &mut egui::Ui, accent: Color32) {
        hero_card(ui, accent);
        self.updates_panel(ui, accent);
        self.admin_panel(ui, accent);
        details_panels(ui);
        self.developer_panels(ui, accent);
        credits_panel(ui);
    }

    /// The "Updates" panel: what the last look for a newer release found, the switch for the daily
    /// look, and the buttons to look now and to download. The same on every OS.
    fn updates_panel(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let ctx = ui.ctx().clone();
        let state = self.ed.update;
        let can_install = self.ed.update_can_install;
        panel(ui, "Updates", |ui| {
            let hint = match state {
                Status::Available(_) => format!("You are using {VERSION}."),
                Status::Failed(why) => why.explain().to_string(),
                Status::Downloading(_) => "Downloading the package and checking it.".to_string(),
                Status::Ready(_) => "CursorPie restarts to finish the update.".to_string(),
                Status::InstallFailed(_, why) => why.explain().to_string(),
                _ => String::new(),
            };
            row_wide(ui, "Status", &hint, 300.0, |ui| match state {
                Status::Idle => muted(ui, "Not checked yet"),
                Status::Checking => muted(ui, "Checking\u{2026}"),
                Status::UpToDate => status(ui, true, "Up to date"),
                Status::Available(found) => {
                    status(ui, true, &format!("Version {found} is available"))
                }
                Status::Failed(_) => status(ui, false, "Could not check"),
                Status::Downloading(found) => muted(ui, &format!("Downloading {found}\u{2026}")),
                Status::Ready(found) => status(ui, true, &format!("Version {found} is ready")),
                Status::InstallFailed(..) => status(ui, false, "Could not install"),
            });
            toggle_row(
                ui,
                &mut self.edit.check_updates,
                "Check for updates automatically",
                "Once a day, when Settings opens. Off: only Check now looks.",
                accent,
            );
            actions(ui, |ui| {
                let idle = !matches!(
                    state,
                    Status::Checking | Status::Downloading(_) | Status::Ready(_)
                );
                let check = button_if(
                    ui,
                    idle,
                    Look::Soft,
                    Some(symbols::ARROWS_CLOCKWISE),
                    "Check now",
                );
                if check.clicked() {
                    self.start_update_check(&ctx, true);
                }
                match state {
                    Status::Available(_) | Status::InstallFailed(..) if can_install => {
                        let retry = matches!(state, Status::InstallFailed(..));
                        let label = if retry { "Try again" } else { "Install update" };
                        let install = button(
                            ui,
                            Look::Primary(accent),
                            Some(symbols::DOWNLOAD_SIMPLE),
                            label,
                        );
                        if install.clicked() {
                            self.start_update_install(&ctx);
                        }
                        if button(ui, Look::Soft, None, "Release page").clicked() {
                            open_link(LATEST_RELEASE_URL);
                        }
                    }
                    Status::Available(_) | Status::InstallFailed(..) => {
                        let page = button(
                            ui,
                            Look::Primary(accent),
                            Some(symbols::DOWNLOAD_SIMPLE),
                            "Download page",
                        );
                        if page.clicked() {
                            open_link(LATEST_RELEASE_URL);
                        }
                    }
                    Status::Ready(_) => {
                        let restart = button(
                            ui,
                            Look::Primary(accent),
                            Some(symbols::ARROWS_CLOCKWISE),
                            "Restart to update",
                        );
                        if restart.clicked() {
                            self.apply_staged_update(&ctx);
                        }
                    }
                    _ => {}
                }
            });
            let how = if can_install && self.ed.update_needs_password {
                "Install update downloads the release from GitHub and checks its checksum. This copy is in a folder only an administrator can write to, so when CursorPie restarts to update, your system asks for your password; if you cancel, the old version starts again. Your settings are kept."
            } else if can_install {
                "Install update downloads the release from GitHub, checks its checksum, replaces this copy and restarts CursorPie. Your settings are kept."
            } else if self.ed.admin_mode.installed {
                "CursorPie is installed as administrator, which Install update does not touch. Download the new version and start it: CursorPie offers to update the installed one."
            } else {
                "This copy was not installed in the usual place, so it cannot update itself: Download page opens the release page in your browser."
            };
            note(ui, how);
            if crate::platform::OS == crate::platform::Os::Mac {
                note(
                    ui,
                    "macOS may ask you to allow Accessibility for CursorPie again after an update.",
                );
            }
        });
    }
}

impl App {
    /// The "Administrator mode" panel (Windows only): Windows keeps the input of a normal program
    /// from windows that run as administrator, so the pie does nothing there. The button installs
    /// CursorPie to run as administrator at every login, with one question for permission now.
    fn admin_panel(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let mode = self.ed.admin_mode;
        if !mode.available {
            return;
        }
        let ctx = ui.ctx().clone();
        panel(ui, "Administrator mode", |ui| {
            let (ok, text, hint) = match (mode.installed, mode.elevated) {
                (true, true) => (
                    true,
                    "On",
                    "Installed in Program Files, started with administrator rights at login.",
                ),
                (true, false) => (
                    false,
                    "Not running",
                    "Installed as administrator, but this copy runs without those rights.",
                ),
                (false, true) => (
                    true,
                    "This session",
                    "Running as administrator now, but not after a restart.",
                ),
                (false, false) => (
                    false,
                    "Off",
                    "Windows ignores CursorPie in programs that run as administrator.",
                ),
            };
            row_wide(ui, "Status", hint, 300.0, |ui| status(ui, ok, text));
            actions(ui, |ui| {
                // the main thing to do while it is off, a repair or an update once it is on
                let look = if mode.installed && mode.elevated {
                    Look::Soft
                } else {
                    Look::Primary(accent)
                };
                let label = "Reinstall as administrator";
                let reinstall = button(ui, look, Some(symbols::SHIELD_CHECK), label);
                if reinstall.clicked() {
                    self.reinstall_as_administrator(&ctx);
                }
            });
            note(
                ui,
                "Task Manager, a terminal opened with Run as administrator and installers that asked for permission ignore a program that does not run as administrator, so the pie does nothing in their windows. This moves CursorPie to Program Files and starts it with administrator rights each time you sign in, without a question every time. Windows asks for your permission once, now; CursorPie restarts and your settings are kept. Your account must be an administrator.",
            );
            note(
                ui,
                "While it runs as administrator, everything a slice starts, a Shell command block included, runs with full rights. To go back, uninstall CursorPie in Settings > Apps and start it again.",
            );
        });
    }

    /// The *Reinstall as administrator* button: starts the elevated step and, once Windows let it
    /// start, quits so that the step can replace the files. Changes that Settings still holds are
    /// not lost silently: the button asks to save or cancel them first.
    fn reinstall_as_administrator(&mut self, ctx: &egui::Context) {
        if self.has_unsaved_changes() {
            self.say("Save or cancel your changes, then press Reinstall as administrator.");
            return;
        }
        diag::log!("ui", "admin_reinstall");
        match platform::reinstall_as_administrator() {
            Ok(()) => self.quit(ctx),
            Err(ReinstallError::Cancelled) => {
                self.say("Cancelled: CursorPie was not reinstalled.");
            }
            Err(ReinstallError::Failed | ReinstallError::Unsupported) => {
                self.say("Could not ask Windows for administrator rights.");
            }
        }
    }
}

/// The hero card: logo, name, version and the four round link buttons.
fn hero_card(ui: &mut egui::Ui, accent: Color32) {
    // ------------------------------------------------------------ hero
    card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        let (corner, room) = (ui.cursor().min, ui.available_width());
        ui.add_space(GAP);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(88.0), egui::Sense::hover());
            draw_logo(ui.painter(), rect.center(), 42.0);
            ui.add_space(GAP);
            ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(fonts::brand_name(APP_NAME, 34.0, pal().text));
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    badge(ui, &format!("Version {VERSION}"), accent_text(accent));
                    badge(ui, LICENSE_NAME, pal().muted);
                });
                ui.add_space(2.0);
                muted(ui, TAGLINE);
                // room under the tagline, the card's bottom edge must not crowd it
                ui.add_space(GAP);
            });
        });
        // the links sit in the top right corner of the card as a row of round icons
        let row_w = 4.0 * LINK_SIZE + 3.0 * 8.0;
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_size(
                    Pos2::new(corner.x + room - row_w - GAP, corner.y + GAP),
                    Vec2::new(row_w, LINK_SIZE),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                link_button(ui, symbols::GLOBE, "Website", WEBSITE_URL);
                link_button(ui, symbols::GITHUB_LOGO, "GitHub repository", REPO_URL);
                link_button(ui, symbols::BUG, "Report an issue", ISSUES_URL);
                link_button(ui, symbols::DOWNLOAD_SIMPLE, "Downloads", RELEASES_URL);
            },
        );
        ui.add_space(GAP);
    });
    ui.add_space(SECTION_GAP);
}

/// The panels below the updates: author, project and privacy.
fn details_panels(ui: &mut egui::Ui) {
    panel(ui, "Created by", |ui| {
        info_row(ui, AUTHOR_NAME, |_| {});
        link_row(
            ui,
            "GitHub",
            &format!("@{AUTHOR_GITHUB_USER}"),
            AUTHOR_GITHUB_URL,
        );
        link_row(ui, "Email", AUTHOR_EMAIL, &format!("mailto:{AUTHOR_EMAIL}"));
    });
    panel(ui, "Project", |ui| {
        link_row(ui, "Website", "sebastvnd.github.io/cursorpie", WEBSITE_URL);
        link_row(ui, "Inspired by", INSPIRED_BY.0, INSPIRED_BY.1);
        info_row(ui, "License", |ui| muted(ui, LICENSE_NAME));
        info_row(ui, "Copyright", |ui| muted(ui, COPYRIGHT));
    });
    panel(ui, "Privacy", |ui| {
        note(ui, PRIVACY);
    });
}

/// The "Built with" credits. Drawn last, below the Developer panels, so it is always the bottom of the tab.
fn credits_panel(ui: &mut egui::Ui) {
    panel(ui, "Built with", |ui| {
        for (name, url, license) in CREDITS {
            row(ui, name, license, |ui| {
                if icon_button(ui, symbols::ARROW_SQUARE_OUT, url, true).clicked() {
                    open_link(url);
                }
            });
        }
    });
}
