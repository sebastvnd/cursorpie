//! The sheet that follows an import of a flow or profile file: it says **why a file was refused**
//! or **what had to be repaired / will probably not work** in one that was imported, in full
//! sentences, instead of one line in the status bar that disappears after a few seconds.
//!
//! The words come from `config::flow::parse_flow_report` (and `config::share::parse_bundle`); the
//! app keeps them in `App::import_report` until the sheet is closed with Enter, Esc or the button.

use crate::ui::app::App;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, RichText, Vec2};

impl App {
    /// Draws the import sheet while `import_report` is set.
    pub(super) fn import_report_dialog(&mut self, ctx: &egui::Context) {
        let Some(report) = self.import_report.clone() else {
            return;
        };
        let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut close = enter || esc;
        let height = (150.0 + 34.0 * report.lines.len() as f32).clamp(190.0, 420.0);
        let (floating, ()) = sheet(
            ctx,
            "import_report",
            self.ed.card,
            Vec2::new(600.0, height),
            if report.imported {
                "Imported, with notes"
            } else {
                "Could not import"
            },
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.label(
                        RichText::new(symbols::WARNING.to_string())
                            .font(egui::FontId::new(18.0, crate::ui::fonts::icons()))
                            .color(pal().bad),
                    );
                    ui.label(crate::ui::fonts::title(&report.file, 15.0, pal().text));
                });
                ui.add_space(6.0);
                let intro = if report.imported {
                    "The file was imported, but you should know this before you Save:"
                } else {
                    "The file was not imported, because:"
                };
                ui.label(RichText::new(intro).size(13.5).color(pal().muted));
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(height - 130.0)
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
                ui.add_space(GAP);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if button(ui, Look::Soft, None, "OK (Enter)").clicked() {
                        close = true;
                    }
                });
            },
        );
        if floating.close || close {
            self.import_report = None;
        }
        ctx.request_repaint();
    }
}
