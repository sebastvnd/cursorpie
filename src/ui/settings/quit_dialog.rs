//! The "unsaved changes" sheet shown when you quit CursorPie while Settings holds edits that were
//! not saved yet.
//!
//! Quitting from Settings (the sidebar button, the Quit button of General) or from the tray menu
//! goes through [`App::request_quit`]. With nothing to lose it quits at once; otherwise it sets
//! `App::confirm_quit` and this sheet asks once more, with three ways out: keep editing, save and
//! quit, or quit and lose the edits. The question of "is there something to lose" is
//! [`App::has_unsaved_changes`], a plain comparison of the working copy with the config Settings
//! opened with.

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
    /// Close the sheet and go back to editing.
    KeepEditing,
    /// Save, then quit.
    SaveAndQuit,
    /// Quit and drop the edits.
    QuitWithoutSaving,
}

impl App {
    /// Whether leaving now would lose edits: Settings is open (not the welcome window, not
    /// already closing) and its working copy differs from what it opened with.
    ///
    /// A comparison of two configs, so it is cheap enough to call when a button is pressed, but it
    /// is not meant to run on every frame.
    pub fn has_unsaved_changes(&self) -> bool {
        self.asks_before_quitting()
            && self
                .opened
                .as_ref()
                .is_some_and(|opened| *opened != self.edit)
    }

    /// Whether Quit asks first: while the Settings window is showing. With Settings closed (the
    /// pie window is hidden) there is no window to ask in, and the welcome window has no sheet.
    fn asks_before_quitting(&self) -> bool {
        self.settings_open && self.welcome.is_none() && self.closing.is_none()
    }

    /// The way to quit from Settings and the tray. While Settings is open it opens the
    /// confirmation sheet (bringing the window to the front first, since the tray menu may have
    /// been used while another app was in front); otherwise it quits at once.
    pub fn request_quit(&mut self, ctx: &egui::Context) {
        // a Save that is still animating away has to finish before the process goes
        if self.closing == Some(true) {
            self.close_settings(ctx, true);
        }
        if self.asks_before_quitting() {
            diag::log!(
                "ui",
                "quit_needs_confirmation",
                unsaved = self.has_unsaved_changes()
            );
            self.ed.stop_recording();
            self.ed.close_sheet();
            self.confirm_discard = false;
            self.confirm_quit = true;
            self.raise_settings(ctx, "quit");
            ctx.request_repaint();
        } else {
            self.quit(ctx);
        }
    }

    /// Cancel, Esc and the window's close button. With nothing unsaved the window closes at once;
    /// otherwise it opens the sheet that asks before the edits are thrown away.
    pub fn request_cancel(&mut self) {
        if self.confirm_discard || self.confirm_quit {
            return;
        }
        if self.has_unsaved_changes() {
            self.ed.stop_recording();
            self.ed.close_sheet();
            self.confirm_discard = true;
        } else {
            self.closing = Some(false);
        }
    }

    /// Draws the "discard changes?" sheet while `confirm_discard` is set. Enter accepts (closes
    /// without saving), Esc or a click outside keeps editing.
    pub(super) fn discard_dialog(&mut self, ctx: &egui::Context) {
        if !self.confirm_discard {
            return;
        }
        let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let mut discard = enter;
        let mut keep = false;
        let (floating, ()) = sheet(
            ctx,
            "confirm_discard",
            self.ed.card,
            Vec2::new(520.0, 200.0),
            "Discard changes?",
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new(symbols::WARNING.to_string())
                            .font(egui::FontId::new(18.0, crate::ui::fonts::icons()))
                            .color(pal().bad),
                    );
                    ui.label(crate::ui::fonts::title(
                        "You have unsaved changes",
                        15.0,
                        pal().text,
                    ));
                });
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "If you cancel now, the changes you made in Settings are lost. \
                             Press Enter to discard them, or Esc to keep editing.",
                        )
                        .size(13.5)
                        .color(pal().muted),
                    )
                    .wrap(),
                );
                ui.add_space(GAP);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if button(ui, Look::Danger, None, "Discard (Enter)").clicked() {
                        discard = true;
                    }
                    if button(ui, Look::Soft, None, "Keep editing (Esc)").clicked() {
                        keep = true;
                    }
                });
            },
        );
        if floating.close && !discard {
            keep = true;
        }
        if discard {
            self.confirm_discard = false;
            self.closing = Some(false);
        } else if keep {
            self.confirm_discard = false;
        }
        ctx.request_repaint();
    }

    /// Draws the sheet while `confirm_quit` is set and applies the answer. It is a plain "are you
    /// sure" when nothing is unsaved, and it names the unsaved changes (with a way to save them)
    /// when there are some.
    pub(super) fn quit_dialog(&mut self, ctx: &egui::Context, accent: egui::Color32) {
        if !self.confirm_quit {
            return;
        }
        let unsaved = self.has_unsaved_changes();
        let mut answer = Answer::Pending;
        let size = Vec2::new(560.0, if unsaved { 214.0 } else { 172.0 });
        let (floating, ()) = sheet(
            ctx,
            "confirm_quit",
            self.ed.card,
            size,
            "Quit CursorPie?",
            |ui| {
                if unsaved {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(
                            RichText::new(symbols::WARNING.to_string())
                                .font(egui::FontId::new(18.0, crate::ui::fonts::icons()))
                                .color(pal().bad),
                        );
                        ui.label(crate::ui::fonts::title(
                            "You have unsaved changes",
                            15.0,
                            pal().text,
                        ));
                    });
                    ui.add_space(6.0);
                }
                let text = if unsaved {
                    "Some changes in Settings were not saved yet. If you quit now they are lost. \
                 Do you really want to quit CursorPie?"
                } else {
                    "CursorPie will stop, and the pie will not open until you start it again. \
                 Do you really want to quit?"
                };
                ui.add(egui::Label::new(RichText::new(text).size(13.5).color(pal().muted)).wrap());
                ui.add_space(GAP);
                // the buttons sit at the right: the way back first, the way out last
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    // added from the right edge: the way out is the rightmost, the way back the leftmost
                    if unsaved {
                        if button(ui, Look::Danger, None, "Quit without saving").clicked() {
                            answer = Answer::QuitWithoutSaving;
                        }
                        if button(ui, Look::Soft, None, "Save and quit").clicked() {
                            answer = Answer::SaveAndQuit;
                        }
                        if button(ui, Look::Primary(accent), None, "Keep editing").clicked() {
                            answer = Answer::KeepEditing;
                        }
                    } else {
                        if button(ui, Look::Danger, Some(symbols::POWER), "Quit CursorPie")
                            .clicked()
                        {
                            answer = Answer::QuitWithoutSaving;
                        }
                        if button(ui, Look::Primary(accent), None, "Cancel").clicked() {
                            answer = Answer::KeepEditing;
                        }
                    }
                });
            },
        );
        // the close button and a click outside the sheet mean "not yet"
        if floating.close && answer == Answer::Pending {
            answer = Answer::KeepEditing;
        }
        match answer {
            Answer::Pending => {}
            Answer::KeepEditing => self.confirm_quit = false,
            Answer::SaveAndQuit => {
                self.close_settings(ctx, true);
                self.quit(ctx);
            }
            Answer::QuitWithoutSaving => {
                self.confirm_quit = false;
                self.quit(ctx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::ui::settings::Tab;

    fn app() -> App {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        app.settings_open = true;
        app.edit = Config::default();
        app.opened = Some(app.edit.clone());
        app
    }

    #[test]
    fn a_freshly_opened_settings_has_nothing_to_lose() {
        assert!(!app().has_unsaved_changes());
    }

    #[test]
    fn an_edit_counts_until_it_is_undone() {
        let mut a = app();
        a.edit.accent = [1, 2, 3];
        assert!(a.has_unsaved_changes());
        a.edit = a.opened.clone().expect("the copy Settings opened with");
        assert!(!a.has_unsaved_changes());
    }

    #[test]
    fn nothing_is_lost_when_settings_is_not_the_thing_being_edited() {
        let mut a = app();
        a.edit.accent = [1, 2, 3];
        a.closing = Some(false); // Cancel was pressed: the edits are already given up
        assert!(!a.has_unsaved_changes());
        a.closing = None;
        a.settings_open = false; // the pie window: no working copy at all
        assert!(!a.has_unsaved_changes());
        a.settings_open = true;
        a.opened = None; // no baseline, nothing to compare with
        assert!(!a.has_unsaved_changes());
    }

    #[test]
    fn the_welcome_window_never_asks() {
        let mut a = app();
        a.edit.accent = [1, 2, 3];
        a.welcome = Some(crate::ui::settings::Welcome::new(false));
        assert!(!a.has_unsaved_changes());
    }

    #[test]
    fn quit_asks_whenever_settings_is_showing() {
        let mut a = app();
        assert!(a.asks_before_quitting(), "even with nothing unsaved");
        a.welcome = Some(crate::ui::settings::Welcome::new(false));
        assert!(!a.asks_before_quitting());
        a.welcome = None;
        a.settings_open = false;
        assert!(!a.asks_before_quitting(), "no window to ask in");
    }

    #[test]
    fn quitting_with_nothing_unsaved_still_asks() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.request_quit(&ctx);
        assert!(a.confirm_quit && !a.quitting);
        crate::ui::fonts::install(&ctx);
        draw(&mut a, &ctx, 3);
        assert!(a.confirm_quit, "the plain question draws and waits");
    }

    #[test]
    fn quitting_with_unsaved_changes_asks_first() {
        let mut a = app();
        let ctx = egui::Context::default();
        a.edit.accent = [1, 2, 3];
        a.request_quit(&ctx);
        assert!(a.confirm_quit, "the sheet is shown");
        assert!(!a.quitting, "the app keeps running until it is answered");
    }

    /// Draws Settings for `frames` frames on a headless context.
    fn draw(app: &mut App, ctx: &egui::Context, frames: usize) {
        use crate::ui::app::{CARD_SIZE, WIN_MARGIN};
        for _ in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(
                        CARD_SIZE[0] + 2.0 * WIN_MARGIN,
                        CARD_SIZE[1] + 2.0 * WIN_MARGIN,
                    ),
                )),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| app.settings_ui(ui))
                .drop_without_applying_deltas();
        }
    }

    #[test]
    fn drawing_alone_never_makes_a_change() {
        // every section draws with the config Settings opened with: if one of them normalized
        // something while drawing, Quit would warn about edits the user never made
        let mut a = app();
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        for tab in [
            Tab::Menu,
            Tab::Flow,
            Tab::Behavior,
            Tab::General,
            Tab::About,
        ] {
            a.ed.tab = tab;
            draw(&mut a, &ctx, 3);
            assert!(!a.has_unsaved_changes());
        }
    }

    #[test]
    fn the_sheet_draws_and_stays_until_answered() {
        let mut a = app();
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        a.edit.accent = [1, 2, 3];
        a.request_quit(&ctx);
        draw(&mut a, &ctx, 3);
        assert!(a.confirm_quit && !a.quitting);
        assert!(
            a.has_unsaved_changes(),
            "asking does not save or drop anything"
        );
    }
}
