//! The sheet that says **what went wrong in a run of a flow**: which block could not do its work
//! and why, in full sentences, instead of a flow that silently did less than it should.
//!
//! The words come from [`Outcome::report`](crate::flowrun::Outcome::report); the actions worker
//! sends them as `Msg::FlowRan` and the app keeps them in `App::run_report` until the sheet is
//! closed with Enter, Esc or the button. A problem that the flow dealt with itself (an *If / else*
//! on `ok` or `code`) is never in it. The same words are the tooltip of the red mark that the Flow
//! tab puts on the block (`Ed::flow_marks`), and the first of them is in the desktop notification.

use crate::ui::app::App;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, RichText, Vec2};

impl App {
    /// Draws the sheet while `run_report` is set.
    pub(super) fn run_report_dialog(&mut self, ctx: &egui::Context) {
        let Some(report) = self.run_report.clone() else {
            return;
        };
        let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut close = enter || esc;
        let height = (210.0 + 44.0 * report.lines.len() as f32).clamp(250.0, 480.0);
        let (floating, ()) = sheet(
            ctx,
            "run_report",
            self.ed.card,
            Vec2::new(620.0, height),
            "A flow had a problem",
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new(symbols::WARNING.to_string())
                            .font(egui::FontId::new(18.0, crate::ui::fonts::icons()))
                            .color(pal().bad),
                    );
                    ui.add(
                        egui::Label::new(crate::ui::fonts::title(&report.flow, 15.0, pal().text))
                            .truncate(),
                    );
                });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("{}. The flow ran, but:", report.title))
                        .size(13.5)
                        .color(pal().muted),
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(height - 170.0)
                    .show(ui, |ui| {
                        for line in &report.lines {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("\u{2022} {line}"))
                                        .size(13.5)
                                        .color(pal().text),
                                )
                                .wrap(),
                            );
                            ui.add_space(3.0);
                        }
                    });
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "The blocks are marked in red in the Flow tab. To deal with a failure \
                             yourself, put an If / else after the block that can fail and look at \
                             `ok` (1 when it worked) or `code` (the exit code of a command): a \
                             failure that the flow looks at is not reported.",
                        )
                        .size(12.5)
                        .color(pal().muted),
                    )
                    .wrap(),
                );
                ui.add_space(GAP);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if button(ui, Look::Soft, None, "OK (Enter)").clicked() {
                        close = true;
                    }
                });
            },
        );
        if floating.close || close {
            self.run_report = None;
        }
        ctx.request_repaint();
    }
}
