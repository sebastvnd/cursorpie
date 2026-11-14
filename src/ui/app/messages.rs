//! Applies one [`Msg`] (sent by the event tap, the tray menu or a file-picker thread) to the UI state.

use super::App;
use crate::config::flow::{add_flow, import_flow_report, merge_flows};
use crate::config::share::{add_profile, import_file_with_flows};
use crate::config::{
    Action, Flow, Item, ItemKind, MAX_CONFIG_FILE_BYTES, NodeKind, cfg, has_commands, icons_dir,
    item_at_mut, parse_config, profile_has_commands, read_text_capped, tilde,
};
use crate::msg::Msg;
use crate::platform;
use crate::ui::appicon;
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
            Msg::PieBlocked { x, y, touch } => self.pie_blocked(ctx, x, y, touch),
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
            Msg::Hud(note) => self.hud_note(ctx, note),
            Msg::HudEnd => self.hud_end(ctx),
            Msg::Panel(req) => self.panel_request(ctx, req),
            Msg::PanelPatch {
                panel,
                patch,
                source,
            } => self.panel_patch(ctx, &panel, &patch, &source),
            Msg::PanelClose { panel, source } => self.panel_close(ctx, &panel, &source),
            Msg::Haptic(kind) => {
                platform::haptic(kind);
            }
            Msg::NewProfileAppPicked(bundle) => self.ed.new_profile_app = bundle,
            Msg::InstalledApps(apps) => {
                // a read that ends after Settings was closed would put the list back for good
                if self.settings_open {
                    self.ed.installed = apps;
                    self.ed.installed_scan = crate::ui::settings::AppScan::Done;
                }
            }
            Msg::ProfileImportPicked(file) => self.import_profile(&file),
            Msg::FlowImportPicked(file) => self.import_flow(&file),
            Msg::Notice(text) => self.say(text),
            Msg::FlowKit(status) => {
                self.say(status.notice());
                self.ed.flow_kit = status;
            }
            Msg::FlowRan { id, report } => self.flow_ran(id, report),
            Msg::UpdateChecked { manual, result } => self.update_checked(manual, result),
            Msg::UpdateStaged(result) => self.update_staged(ctx, result),
            Msg::MouseChanged { new, name, extra } => {
                let which = if new { "New mouse" } else { "Mouse back again" };
                self.say(format!("{which}: {name} ({extra} extra buttons)"));
            }
            Msg::ButtonScanned(button) => {
                if let Some(scan) = self.ed.scan.as_mut() {
                    scan.press(button);
                }
            }
            Msg::Warm { general, app } => self.warm(ctx, general, app),
            Msg::Refocus => {
                // The chooser is gone and the system hands the focus back by itself, often to the
                // window that was below Settings. Raise only if Settings really does not have it:
                // a raise on top of a window that has the focus makes it flicker or lose its place.
                // (The picker sends this twice, so a handover that comes late is caught too.)
                let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
                if self.settings_open && !focused {
                    self.raise_settings(ctx, "chooser_closed");
                }
                // While the chooser was open the compositor stopped asking Settings for frames
                // (it was covered or unfocused), so what is on screen can be a stale buffer. Draw
                // on a timer for a moment so the first real frames replace it.
                if self.settings_open {
                    self.start_repaint_burst(ctx);
                }
            }
            Msg::HidePie => {
                diag::log!(
                    "ui",
                    "hide_pie",
                    was_open = self.pie_open,
                    was_visible = self.visible
                );
                crate::ui::memlog::mark("hide_pie");
                self.hover = None;
                self.pie_open = false;
                self.preview_until = None;
                // the pie that took the pointer for a trackpad hold lets it through again while it
                // fades; Settings shares this window and must keep its clicks (see
                // `passthrough_after_hide`)
                ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(
                    super::windows::passthrough_after_hide(self.settings_open),
                ));
            }
            Msg::KeyCaptured(sc) => {
                // a shortcut block of the Flow tab, or a field of a slice of the Menu tab
                let flow_block = self.ed.recording.as_ref().and_then(|r| match r.slot {
                    Slot::Node(id) => Some((r.profile, id)),
                    _ => None,
                });
                if let Some((flow, id)) = flow_block {
                    self.ed.recording = None;
                    if let Some(f) = self.edit.flows.get_mut(flow)
                        && let Some(n) = f.node_mut(id)
                        && let NodeKind::Key(k) = &mut n.kind
                    {
                        *k = sc;
                    }
                } else if let Some(rec) = self.ed.recording.take()
                    && let Some(p) = self.edit.profiles.get_mut(rec.profile)
                    && let Some(item) = item_at_mut(&mut p.items, &rec.path)
                {
                    match (&mut item.kind, rec.slot) {
                        (ItemKind::Mode { back, .. }, Slot::Back) => *back = Action::Key(sc),
                        (ItemKind::Mode { forward, .. }, Slot::Forward) => {
                            *forward = Action::Key(sc)
                        }
                        (ItemKind::Mode { extra, .. }, Slot::Extra(b)) => {
                            extra.insert(b, Action::Key(sc));
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
                // a copy may have been made in the icons folder: "Mine" lists it at once
                self.ed.reload_user_icons();
                if let Some(note) = note {
                    self.say(note);
                }
            }
            Msg::IconDeleted { file, error } => {
                self.ed.icons.revalidate();
                self.ed.reload_user_icons();
                let name = file_label(&file);
                match error {
                    Some(why) => self.say(why),
                    None => {
                        let used: usize = self
                            .edit
                            .profiles
                            .iter()
                            .map(|p| count_icon(&p.items, &file))
                            .sum();
                        self.say(match used {
                            0 => format!("Deleted {name}."),
                            1 => format!("Deleted {name}. One slice used it and now shows ?."),
                            n => format!("Deleted {name}. {n} slices used it and now show ?."),
                        });
                    }
                }
            }
            Msg::IconRefused { file, lines } => {
                if let Some(first) = lines.first() {
                    self.say(first.clone());
                }
                self.import_report = Some(super::ImportSheet {
                    file: file_label(&file),
                    imported: false,
                    lines,
                });
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
                        "Imported. This config can run shell commands, write files or send messages - review them before you Save."
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
            Msg::FlowAppPicked { flow, node, app } => {
                if let Some(n) = self
                    .edit
                    .flows
                    .get_mut(flow)
                    .and_then(|f| f.nodes.iter_mut().find(|n| n.id == node))
                    && matches!(n.kind, NodeKind::OpenApp(_))
                {
                    n.kind = NodeKind::OpenApp(app);
                }
            }
            Msg::ActionAppPicked {
                profile,
                path,
                slot,
                app,
            } => {
                // the icon of the picked app, for a slice that already shows an app's icon or
                // still has the icon a new "Open app" slice starts with
                let app_id = appicon::app_id_for_action(&app, &self.ed.installed);
                if let Some(p) = self.edit.profiles.get_mut(profile)
                    && let Some(item) = item_at_mut(&mut p.items, &path)
                {
                    match (&mut item.kind, slot) {
                        (ItemKind::Mode { extra, .. }, Slot::Extra(b)) => {
                            extra.insert(b, Action::OpenApp(app));
                        }
                        (ItemKind::Run(a), Slot::Run) => {
                            *a = Action::OpenApp(app);
                            let tone = match appicon::parse_slice(&item.icon) {
                                Some((tone, _)) => Some(tone),
                                None if appicon::is_placeholder_icon(&item.icon) => {
                                    Some(appicon::SliceTone::Color)
                                }
                                None => None,
                            };
                            if let (Some(tone), Some(id)) = (tone, app_id) {
                                item.icon = appicon::slice_icon_string(tone, &id);
                            }
                        }
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
        // the flows the profile uses come with it; if the profile cannot be added after all, the
        // flows are taken out again
        let flows_before = self.edit.flows.clone();
        let added =
            import_file_with_flows(file, &icons_dir(), &tilde).and_then(|(mut p, flows)| {
                let runs_commands =
                    profile_has_commands(&p) || flows.iter().any(Flow::has_commands);
                merge_flows(&mut self.edit.flows, flows, &mut p.items)?;
                add_profile(&mut self.edit, p).map(|i| (i, runs_commands))
            });
        if added.is_err() {
            self.edit.flows = flows_before;
        }
        match added {
            Ok((index, runs_commands)) => {
                self.ed.profile = index;
                self.ed.reset_nav();
                self.ed.stop_recording();
                self.ed.icons.revalidate();
                self.say(if runs_commands {
                    "Profile imported. It can run shell commands, write files or send messages - review them before you Save."
                } else {
                    "Profile imported. Review it, then press Save to apply."
                });
            }
            Err(e) => self.import_refused(file, e),
        }
    }

    /// Shows the sheet that says why a file was refused.
    fn import_refused(&mut self, file: &str, why: String) {
        self.say(why.clone());
        self.import_report = Some(super::ImportSheet {
            file: file_label(file),
            imported: false,
            lines: vec![why],
        });
    }

    /// Adds the flow in `file` to the Settings copy as a new flow. A refused file, or one that had
    /// to be repaired or looks wrong, opens the import sheet that says why.
    fn import_flow(&mut self, file: &str) {
        if !self.settings_open {
            return; // the chooser answered after Settings was closed
        }
        let report = match import_flow_report(file) {
            Ok(r) => r,
            Err(e) => return self.import_refused(file, e),
        };
        let notes = report.notes;
        let runs_commands = report.flow.has_commands();
        match add_flow(&mut self.edit.flows, report.flow) {
            Ok(index) => {
                self.ed.flow = index;
                self.ed.flow_sel = None;
                self.ed.flow_wire = None;
                self.ed.flow_pan = egui::Vec2::ZERO;
                self.ed.stop_recording();
                self.say(if runs_commands {
                    "Flow imported. It can run shell commands, write files or send messages - review them before you Save."
                } else {
                    "Flow imported. Review it, then press Save to apply."
                });
                if !notes.is_empty() {
                    self.import_report = Some(super::ImportSheet {
                        file: file_label(file),
                        imported: true,
                        lines: notes,
                    });
                }
            }
            Err(e) => self.import_refused(file, e),
        }
    }
}

/// The last part of a path, for the sheet.
fn file_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

impl App {
    /// A flow was run: the blocks that could not do their work are marked in the Flow tab (until
    /// it runs well again), and while Settings is open the details come up in a sheet.
    fn flow_ran(&mut self, id: String, report: Option<crate::flowrun::Report>) {
        match report {
            Some(r) => {
                self.ed.flow_marks.insert(id, r.marks.clone());
                if self.settings_open {
                    self.say(format!("Flow \"{}\": {}", r.flow, r.title));
                    self.run_report = Some(r);
                }
            }
            None => {
                self.ed.flow_marks.remove(&id);
            }
        }
    }
}

/// How many slices, submenus included, have `icon` as their icon.
fn count_icon(items: &[Item], icon: &str) -> usize {
    items
        .iter()
        .map(|it| {
            usize::from(it.icon == icon)
                + match &it.kind {
                    ItemKind::Submenu(children) => count_icon(children, icon),
                    _ => 0,
                }
        })
        .sum()
}
