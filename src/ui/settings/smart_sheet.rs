//! The sheet that answers **Learn now** and **Forget everything** (Behavior > Intelligent).
//!
//! Both buttons used to answer with one line in the status bar. That line is small, sits at the very
//! bottom (under the floating page switch of the Behavior tab) and goes away after six seconds, and
//! the notice pill that CursorPie uses elsewhere is never shown while Settings is open: so a click
//! seemed to do nothing. Now a sheet opens at the click, says that CursorPie is working (*Learn now*
//! reads the list of installed apps, which takes a moment) and then tells what came of it. It stays
//! until it is closed with Enter, Esc, the button or a click outside.
//!
//! The state is [`SmartSheet`](super::SmartSheet) in `Ed::smart_sheet`; the worker's answer is
//! `Msg::SmartDone`, applied by [`App::smart_done`].

use crate::ui::app::App;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Color32, RichText, Vec2};

impl App {
    /// The worker is done: fills the sheet that is open for it. A sheet that was closed meanwhile
    /// (or an answer that does not belong to the open one) goes to the status bar instead, so the
    /// answer is never lost.
    pub(crate) fn smart_done(&mut self, forget: bool, lines: Vec<String>) {
        match self.ed.smart_sheet.as_mut() {
            Some(sheet) if sheet.forget == forget => {
                sheet.busy = false;
                sheet.lines = lines;
            }
            _ => self.say(lines.join(" ")),
        }
    }

    /// Draws the sheet while `Ed::smart_sheet` is set.
    pub(super) fn smart_sheet_dialog(&mut self, ctx: &egui::Context, accent: Color32) {
        let Some(state) = self.ed.smart_sheet.clone() else {
            return;
        };
        let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut close = enter || esc;
        let (title, glyph) = if state.forget {
            ("Forget everything", symbols::TRASH)
        } else {
            ("Learn now", symbols::SPARKLE)
        };
        let height = 280.0;
        let (floating, ()) = sheet(
            ctx,
            "smart_sheet",
            self.ed.card,
            Vec2::new(540.0, height),
            title,
            |ui| {
                if state.busy {
                    sheet_heading(ui, glyph, accent, "Working\u{2026}");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.add(egui::Spinner::new().size(18.0).color(accent));
                        ui.add(
                            egui::Label::new(
                                RichText::new(if state.forget {
                                    "Deleting what CursorPie learned."
                                } else {
                                    "Looking at your apps and saving what CursorPie learned. This takes a few seconds."
                                })
                                .size(13.5)
                                .color(pal().muted),
                            )
                            .wrap(),
                        );
                    });
                } else {
                    sheet_heading(
                        ui,
                        glyph,
                        accent,
                        if state.forget { "Forgotten" } else { "Done" },
                    );
                    sheet_bullets(ui, &state.lines, height - 150.0);
                }
                ui.add_space(GAP);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let label = if state.busy { "Close" } else { "OK (Enter)" };
                    let look = if state.busy {
                        Look::Soft
                    } else {
                        Look::Primary(accent)
                    };
                    if button(ui, look, None, label).clicked() {
                        close = true;
                    }
                });
            },
        );
        if floating.close || close {
            self.ed.smart_sheet = None;
        }
        // the spinner moves, and the answer arrives from another thread
        if state.busy {
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::settings::SmartSheet;

    fn app() -> App {
        let (tx, rx) = std::sync::mpsc::channel();
        App::new(rx, tx, 200.0, None)
    }

    #[test]
    fn the_intelligent_page_is_not_the_last_one_of_the_switch() {
        use crate::ui::settings::BehaviorPage;
        let all = BehaviorPage::ALL;
        assert_eq!(all[all.len() - 1], BehaviorPage::System);
        assert_eq!(all[all.len() - 2], BehaviorPage::Smart);
    }

    #[test]
    fn the_answer_fills_the_sheet_that_waits_for_it() {
        let mut app = app();
        app.ed.smart_sheet = Some(SmartSheet::working(false));
        app.smart_done(
            false,
            vec!["Learned so far: 1 apps, 0 web pages, 0 actions.".into()],
        );
        let sheet = app.ed.smart_sheet.as_ref().expect("the sheet stays open");
        assert!(!sheet.busy);
        assert_eq!(sheet.lines.len(), 1);
        assert!(app.toast.is_none(), "the sheet says it, not the status bar");
    }

    #[test]
    fn an_answer_with_no_sheet_goes_to_the_status_bar() {
        let mut app = app();
        app.smart_done(true, vec!["one".into(), "two".into()]);
        assert!(app.ed.smart_sheet.is_none());
        assert_eq!(app.toast.as_ref().map(|(t, _)| t.as_str()), Some("one two"));
    }

    #[test]
    fn the_sheet_draws_while_busy_and_when_done() {
        let mut app = app();
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        for state in [
            SmartSheet::working(false),
            SmartSheet {
                forget: true,
                busy: false,
                lines: vec!["x".into()],
            },
        ] {
            app.ed.smart_sheet = Some(state.clone());
            app.ed.card = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(900.0, 600.0));
            let input = egui::RawInput {
                screen_rect: Some(app.ed.card),
                ..Default::default()
            };
            ctx.begin_pass(input);
            app.smart_sheet_dialog(&ctx, Color32::GREEN);
            ctx.end_pass().drop_without_applying_deltas();
            assert_eq!(
                app.ed.smart_sheet,
                Some(state),
                "drawing alone answers nothing"
            );
        }
    }
}
