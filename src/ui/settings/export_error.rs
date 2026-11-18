//! The sheet that follows an **export that could not be written**: a full disk, a folder that
//! refuses the file, a read-only disk. The status bar line of such a failure goes away after a few
//! seconds (and the status bar can show another problem instead), so the person was left with
//! nothing happening; this sheet stays until it is closed with Enter, Esc or the button.
//!
//! The words come from `config::describe_io_error`; the app keeps them in `App::export_error`.

use crate::ui::app::App;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Vec2};

impl App {
    /// Draws the export error sheet while `export_error` is set.
    pub(super) fn export_error_dialog(&mut self, ctx: &egui::Context) {
        let Some(error) = self.export_error.clone() else {
            return;
        };
        let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut close = enter || esc;
        let lines = [error.reason.clone()];
        let height = 260.0;
        let (floating, ()) = sheet(
            ctx,
            "export_error",
            self.ed.card,
            Vec2::new(600.0, height),
            "Could not export",
            |ui| {
                sheet_heading(ui, symbols::WARNING, pal().bad, &error.what);
                sheet_text(ui, "Nothing was exported, because:");
                ui.add_space(4.0);
                sheet_bullets(ui, &lines, height - 130.0);
                ui.add_space(GAP);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if button(ui, Look::Soft, None, "OK (Enter)").clicked() {
                        close = true;
                    }
                });
            },
        );
        if floating.close || close {
            self.export_error = None;
        }
        ctx.request_repaint();
    }
}
