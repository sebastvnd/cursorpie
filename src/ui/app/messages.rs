//! Applies one [`Msg`] (sent by the event tap, the tray menu or a file-picker thread) to the UI state.

use super::App;
use crate::config::share::{add_profile, import_file};
use crate::config::{
    Action, ItemKind, MAX_CONFIG_FILE_BYTES, cfg, has_commands, icons_dir, item_at_mut,
    parse_config, profile_has_commands, read_text_capped, tilde,
};
use crate::msg::Msg;
use crate::platform;
use crate::ui::settings::Slot;
use eframe::egui;
use std::time::{Duration, Instant};

impl App {
    /// Applies one message. Anything that touches the config edits the Settings *copy* (`self.edit`).
    pub(super) fn handle_msg(&mut self, ctx: &egui::Context, msg: Msg) {
        match msg {
            Msg::ShowPie {
                x,
                y,
                profile,
                path,
            } => {
                // a real gesture takes over from any preview
                self.preview_until = None;
                diag::log!(
                    "ui",
                    "msg_show_pie",
                    x = x,
                    y = y,
                    profile = profile,
                    depth = path.len()
                );
                self.show_pie(ctx, x, y, profile, path);
            }
            Msg::OpenSettings { x, y, about } => {
                self.open_settings(ctx, egui::pos2(x as f32, y as f32), about);
            }
            Msg::OpenWelcome { x, y } => self.open_welcome(ctx, egui::pos2(x as f32, y as f32)),
            Msg::Hover(h) => {
                if h.is_some()
                    && h != self.hover
                    && self.last_tick.elapsed() > Duration::from_millis(60)
                    && cfg().sound
                {
                    platform::tick();
                    self.last_tick = Instant::now();
                }
                self.hover = h;
            }
            Msg::NewProfileAppPicked(bundle) => self.ed.new_profile_app = bundle,
            Msg::ProfileImportPicked(file) => self.import_profile(&file),
            Msg::Notice(text) => self.say(text),
            Msg::Warm { general, app } => self.warm(ctx, general, app),
            Msg::Refocus => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
            Msg::HidePie => {
                diag::log!(
                    "ui",
                    "hide_pie",
                    was_open = self.pie_open,
                    was_visible = self.visible
                );
                self.hover = None;
                self.pie_open = false;
                self.preview_until = None;
            }
            Msg::KeyCaptured(sc) => {
                if let Some(rec) = self.ed.recording.take()
                    && let Some(p) = self.edit.profiles.get_mut(rec.profile)
                    && let Some(item) = item_at_mut(&mut p.items, &rec.path)
                {
                    match (&mut item.kind, rec.slot) {
                        (ItemKind::Mode { back, .. }, Slot::Back) => *back = Action::Key(sc),
                        (ItemKind::Mode { forward, .. }, Slot::Forward) => {
                            *forward = Action::Key(sc)
                        }
                        (ItemKind::Run(a), Slot::Run) => *a = Action::Key(sc),
                        _ => {}
                    }
                }
            }
            Msg::RecordCancelled => self.ed.recording = None,
            Msg::IconPicked {
                profile,
                path,
                file,
                note,
            } => {
                if let Some(p) = self.edit.profiles.get_mut(profile)
                    && let Some(item) = item_at_mut(&mut p.items, &path)
                {
                    item.icon = file;
                }
                if let Some(note) = note {
                    self.say(note);
                }
            }
            Msg::AppPicked { profile, bundle } => {
                if let Some(p) = self.edit.profiles.get_mut(profile)
                    && !p.apps.contains(&bundle)
                {
                    p.apps.push(bundle);
                }
            }
            // streamed with a size cap: a wrong file is never read whole into memory
            Msg::ImportPicked(file) => match read_text_capped(&file, MAX_CONFIG_FILE_BYTES)
                .and_then(|t| parse_config(&t))
            {
                Some(cfg_in) => {
                    let warn = has_commands(&cfg_in);
                    diag::log!(
                        "ui",
                        "import_config",
                        ok = true,
                        profiles = cfg_in.profiles.len(),
                        has_commands = warn
                    );
                    self.edit = cfg_in;
                    self.ed.profile = 0;
                    self.ed.reset_nav();
                    self.ed.stop_recording();
                    self.ed.icons.revalidate();
                    self.say(if warn {
                        "Imported. This config runs shell commands - review them before you Save."
                    } else {
                        "Imported. Review it, then press Save to apply."
                    });
                }
                None => {
                    diag::log!("ui", "import_config", ok = false);
                    self.say("That file is not a valid CursorPie config.")
                }
            },
            Msg::Tray(id) => self.handle_tray(ctx, &id),
            Msg::ActionAppPicked {
                profile,
                path,
                slot,
                app,
            } => {
                if let Some(p) = self.edit.profiles.get_mut(profile)
                    && let Some(item) = item_at_mut(&mut p.items, &path)
                {
                    match (&mut item.kind, slot) {
                        (ItemKind::Run(a), Slot::Run) => *a = Action::OpenApp(app),
                        (ItemKind::Mode { back, .. }, Slot::Back) => *back = Action::OpenApp(app),
                        (ItemKind::Mode { forward, .. }, Slot::Forward) => {
                            *forward = Action::OpenApp(app)
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

impl App {
    /// Adds the profile in `file` (with its icons) to the Settings copy as a new profile.
    fn import_profile(&mut self, file: &str) {
        if !self.settings_open {
            return; // the chooser answered after Settings was closed
        }
        let added = import_file(file, &icons_dir(), &tilde).and_then(|p| {
            let runs_commands = profile_has_commands(&p);
            add_profile(&mut self.edit, p).map(|i| (i, runs_commands))
        });
        match added {
            Ok((index, runs_commands)) => {
                self.ed.profile = index;
                self.ed.reset_nav();
                self.ed.stop_recording();
                self.ed.icons.revalidate();
                self.say(if runs_commands {
                    "Profile imported. It runs shell commands - review them before you Save."
                } else {
                    "Profile imported. Review it, then press Save to apply."
                });
            }
            Err(e) => self.say(e),
        }
    }
}
