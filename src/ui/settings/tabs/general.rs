//! General tab: permissions status, the mouse, and config import/export/reset.

use crate::config::backup::age_label;
use crate::config::*;
use crate::msg::Msg;
use crate::platform;
use crate::tap::{PAUSED, TAP_ACTIVE};
use crate::ui::app::App;
use crate::ui::draw::rgb;
use crate::ui::settings::picker::spawn_file_picker;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

impl App {
    /// Draws the General tab: permission status, config location, import / export / reset.
    pub(in crate::ui::settings) fn general_tab(&mut self, ui: &mut egui::Ui) {
        let accent = rgb(self.edit.accent);
        let title = if platform::OS.needs_permission() {
            "Permissions"
        } else {
            "Input"
        };
        panel(ui, title, |ui| {
            let needs = platform::OS.needs_permission();
            let running = TAP_ACTIVE.load(Ordering::SeqCst);
            if needs {
                let trusted = platform::is_trusted_cached();
                info_row(ui, "Accessibility", |ui| {
                    status(ui, trusted, if trusted { "Granted" } else { "Missing" });
                });
            }
            info_row(ui, if needs { "Event tap" } else { "Mouse hook" }, |ui| {
                status(
                    ui,
                    running,
                    if running {
                        "Running"
                    } else {
                        "Not running (retrying every 2 s)"
                    },
                );
            });
            // without a tray icon (Linux, for now) pausing and quitting live here
            if !platform::OS.has_tray() {
                let mut paused = PAUSED.load(Ordering::SeqCst);
                if toggle_row(ui, &mut paused, "Pause CursorPie", "", accent).changed() {
                    PAUSED.store(paused, Ordering::SeqCst);
                }
                actions(ui, |ui| {
                    if button(ui, Look::Danger, Some(symbols::POWER), "Quit CursorPie").clicked() {
                        let ctx = ui.ctx().clone();
                        self.request_quit(&ctx);
                    }
                });
            }
            if needs {
                actions(ui, |ui| {
                    if button(
                        ui,
                        Look::Soft,
                        Some(symbols::ARROW_SQUARE_OUT),
                        "Accessibility settings",
                    )
                    .clicked()
                    {
                        platform::open_accessibility_settings();
                    }
                    if button(
                        ui,
                        Look::Soft,
                        Some(symbols::ARROW_SQUARE_OUT),
                        "Input Monitoring settings",
                    )
                    .clicked()
                    {
                        platform::open_input_monitoring_settings();
                    }
                });
                note(
                    ui,
                    "If it still says 'Not running' after granting access, quit and reopen CursorPie.",
                );
            } else if platform::OS == platform::Os::Linux {
                // X11 needs no permission; a Wayland session needs the udev rule (see
                // `platform::linux::wayland::report`), and says exactly what is missing
                let wayland = crate::platform::linux::session::Session::current().server
                    == crate::platform::linux::session::DisplayServer::Wayland;
                if wayland {
                    note(ui, &crate::platform::linux::wayland::cached_report().text);
                } else {
                    note(ui, "No permission is needed on X11.");
                }
            } else {
                note(
                    ui,
                    "No permission is needed. Windows ignores input sent to programs running as administrator unless CursorPie runs as administrator too.",
                );
            }
        });
        self.mouse_panel(ui);
        panel(ui, "Configuration", |ui| {
            let path = config_path();
            note(ui, &path.to_string_lossy());
            actions(ui, |ui| {
                if button(
                    ui,
                    Look::Soft,
                    Some(symbols::FOLDER_OPEN),
                    &format!("Reveal in {}", platform::OS.file_manager()),
                )
                .clicked()
                {
                    platform::reveal_in_file_manager(&path);
                }
                if button(
                    ui,
                    Look::Soft,
                    Some(symbols::UPLOAD_SIMPLE),
                    "Export\u{2026}",
                )
                .clicked()
                {
                    match export_config(&self.edit) {
                        Ok(p) => self.say(format!("Exported to {}", p.display())),
                        Err(e) => self.say(format!("Export failed: {e}")),
                    }
                }
                if button(
                    ui,
                    Look::Soft,
                    Some(symbols::DOWNLOAD_SIMPLE),
                    "Import\u{2026}",
                )
                .clicked()
                {
                    spawn_file_picker(
                        self.tx.clone(),
                        ui.ctx().clone(),
                        platform::FileKind::Json,
                        "Choose a CursorPie config",
                        |f| Some(Msg::ImportPicked(f)),
                    );
                }
                if button(
                    ui,
                    Look::Danger,
                    Some(symbols::ARROW_COUNTER_CLOCKWISE),
                    "Reset to defaults",
                )
                .clicked()
                {
                    let login = self.edit.launch_at_login;
                    self.edit = Config::default();
                    self.edit.launch_at_login = login;
                    self.ed.profile = 0;
                    self.ed.stop_recording();
                    self.say("Defaults restored. Press Save to apply.");
                }
            });
            note(
                ui,
                "Export saves your whole setup to ~/Downloads so you can share it. Only import configs you trust: actions can run shell commands.",
            );
        });
        panel(ui, "Backups", |ui| {
            note(
                ui,
                "Every Save keeps the version it replaces, so a setting deleted by mistake is not gone for good. Restoring loads a backup into the editor; nothing changes until you press Save, and Undo brings back what you had.",
            );
            if self.ed.backups.is_empty() {
                note(
                    ui,
                    "No backups yet. The first one is made by your next Save.",
                );
            }
            let now = SystemTime::now();
            let mut restore = None;
            if !self.ed.backups.is_empty() {
                actions(ui, |ui| {
                    for b in &self.ed.backups {
                        let age = b
                            .modified
                            .and_then(|t| now.duration_since(t).ok())
                            .map_or_else(|| "earlier".to_string(), age_label);
                        if button(ui, Look::Soft, Some(symbols::CLOCK_COUNTER_CLOCKWISE), &age)
                            .clicked()
                        {
                            restore = Some(b.n);
                        }
                    }
                });
            }
            if let Some(n) = restore {
                self.restore_backup(n);
            }
        });
    }
}
