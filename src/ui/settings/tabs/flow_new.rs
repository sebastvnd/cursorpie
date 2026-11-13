//! The sheet that opens when **New flow** is pressed in the Flow tab.
//!
//! It first asks how the flow is to be made:
//!
//! * **Build it myself** adds an empty flow (what the button did before);
//! * **Start from a template** lists complete flows that control the browser (a remote with a panel,
//!   a pause that asks first, a tab finder; see
//!   [`config::flow_templates`](crate::config::flow_templates)). One press adds it and opens it, so
//!   the person sees how a panel, a *Branch* and a *Browser command* fit together;
//! * **Build it with AI** shows a box that looks like the message box of a chat, but sends
//!   nothing. The person says what the flow should do and presses **Copy kit**: CursorPie writes a
//!   zip with the guides an agent needs and what was typed (see
//!   [`config::flow_kit`](crate::config::flow_kit)) and puts it on the clipboard as a file. The
//!   person pastes it into whatever AI agent they use, adds a prompt such as "make this", and
//!   brings the file it returns back with **Import result**.
//!
//! Nothing here talks to an AI service: the app stays offline, and the person picks the agent. The
//! zip is made on a worker thread (a clipboard program can take a second) and the outcome comes
//! back as `Msg::FlowKit`, kept in `Ed::flow_kit`.

use super::flow::{Do, notice, run, select_flow};
use crate::config::flow_kit::{self, MAX_REQUEST_CHARS};
use crate::config::flow_templates::{self, TEMPLATES};
use crate::config::{Config, MAX_FLOWS};
use crate::msg::Msg;
use crate::platform::{self, Os};
use crate::ui::fonts;
use crate::ui::settings::state::{Ed, FlowNew, KitStatus};
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, Align2, Color32, FontId, RichText, Vec2};
use std::sync::mpsc::Sender;

/// Fewest characters of a request before the kit can be copied: an empty box would give the agent
/// nothing to build.
const MIN_REQUEST_CHARS: usize = 3;

/// The name of a system as the agent should read it: shortcuts and shell commands differ.
fn system_name(os: Os) -> &'static str {
    match os {
        Os::Mac => "macOS (Cmd is the command key, shell commands run in /bin/sh)",
        Os::Windows => "Windows (shell commands run in cmd.exe)",
        Os::Linux => "Linux (shell commands run in /bin/sh)",
    }
}

/// Whether the request holds enough to build a flow from.
fn can_copy(request: &str, status: &KitStatus) -> bool {
    request.trim().chars().count() >= MIN_REQUEST_CHARS && *status != KitStatus::Working
}

/// Draws the sheet while `Ed::flow_new` is set and carries out what it asks for.
pub(in crate::ui::settings) fn flow_new_sheet(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: Color32,
) {
    match ed.flow_new {
        None => {}
        Some(FlowNew::Choose) => choose_page(ctx, edit, ed, tx, accent),
        Some(FlowNew::Ai) => ai_page(ctx, edit, ed, tx, accent),
        Some(FlowNew::Templates) => templates_page(ctx, edit, ed, tx, accent),
    }
}

/// One of the two big choices: an icon, a title and a line under it.
fn choice_tile(
    ui: &mut egui::Ui,
    width: f32,
    icon: char,
    title: &str,
    hint: &str,
    accent: Color32,
) -> egui::Response {
    let (rect, resp) = tile(ui, Vec2::new(width, 150.0), false, accent);
    let pad = 16.0;
    let painter = ui.painter();
    fonts::paint_icon(
        painter,
        rect.min + Vec2::new(pad + 11.0, pad + 12.0),
        icon,
        24.0,
        accent_text(accent),
        false,
    );
    painter.text(
        rect.min + Vec2::new(pad, pad + 38.0),
        Align2::LEFT_TOP,
        title,
        FontId::new(15.0, fonts::semibold()),
        pal().text,
    );
    let text = painter.layout(
        hint.to_owned(),
        FontId::proportional(13.0),
        pal().muted,
        width - 2.0 * pad,
    );
    painter.galley(rect.min + Vec2::new(pad, pad + 64.0), text, pal().muted);
    resp
}

/// "How do you want to make it?": by hand, from a template, or with an AI agent.
fn choose_page(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: Color32,
) {
    let mut by_hand = false;
    let mut with_ai = false;
    let mut from_template = false;
    let (floating, ()) = sheet(
        ctx,
        "flow_new",
        ed.card,
        Vec2::new(760.0, 300.0),
        "New flow",
        |ui| {
            let width = (ui.available_width() - 2.0 * GAP) / 3.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                by_hand = choice_tile(
                    ui,
                    width,
                    symbols::PENCIL_SIMPLE,
                    "Build it myself",
                    "Start from an empty flow and add the blocks yourself.",
                    accent,
                )
                .clicked();
                from_template = choice_tile(
                    ui,
                    width,
                    symbols::BROWSER,
                    "Start from a template",
                    "A working flow for the browser (a remote, a pause, a tab finder) to change.",
                    accent,
                )
                .clicked();
                with_ai = choice_tile(
                    ui,
                    width,
                    symbols::SPARKLE,
                    "Build it with AI",
                    "Say what you want, then hand a ready-made kit to an AI agent.",
                    accent,
                )
                .clicked();
            });
        },
    );
    if by_hand {
        ed.flow_new = None;
        run(Do::New, ctx, edit, ed, tx);
    } else if from_template {
        ed.flow_new = Some(FlowNew::Templates);
    } else if with_ai {
        ed.flow_new = Some(FlowNew::Ai);
        ed.flow_kit = KitStatus::Idle;
    } else if floating.close {
        ed.flow_new = None;
    }
}

/// "Start from a template": one row for each, with the button that adds it.
fn templates_page(
    ctx: &egui::Context,
    edit: &mut Config,
    ed: &mut Ed,
    tx: &Sender<Msg>,
    accent: Color32,
) {
    let mut chosen = None;
    let mut back = false;
    let (floating, ()) = sheet(
        ctx,
        "flow_new",
        ed.card,
        Vec2::new(600.0, 420.0),
        "Start from a template",
        |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Each one is a complete flow. It is added as a new flow that you can open, \
                         run and change. The commands for the browser need the extension: \
                         Settings > Behavior > Browser extension.",
                    )
                    .size(13.0)
                    .color(pal().muted),
                )
                .wrap(),
            );
            ui.add_space(GAP);
            for (i, t) in TEMPLATES.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.vertical(|ui| {
                        ui.set_width((ui.available_width() - 90.0).max(120.0));
                        ui.label(RichText::new(t.title).size(14.5).strong());
                        ui.add(
                            egui::Label::new(RichText::new(t.about).size(12.5).color(pal().muted))
                                .wrap(),
                        );
                    });
                    if button(ui, Look::Primary(accent), Some(symbols::PLUS), "Add")
                        .on_hover_text("Add it as a new flow and open it")
                        .clicked()
                    {
                        chosen = Some(i);
                    }
                });
                ui.add_space(10.0);
            }
            back = button(ui, Look::Soft, None, "Back").clicked();
        },
    );
    if let Some(i) = chosen {
        if edit.flows.len() >= MAX_FLOWS {
            notice(
                tx,
                ctx,
                format!("There are already {MAX_FLOWS} flows. Delete one first."),
            );
            return;
        }
        match flow_templates::build(&TEMPLATES[i], &edit.flows) {
            Ok(flow) => {
                edit.flows.push(flow);
                select_flow(ed, edit.flows.len() - 1);
                ed.flow_new = None;
            }
            Err(why) => notice(tx, ctx, format!("The template could not be added: {why}")),
        }
    } else if back {
        ed.flow_new = Some(FlowNew::Choose);
    } else if floating.close {
        ed.flow_new = None;
    }
}

/// Makes the kit on a worker thread and reports the outcome as `Msg::FlowKit`.
fn start_kit(ctx: &egui::Context, ed: &mut Ed, tx: &Sender<Msg>) {
    ed.flow_kit = KitStatus::Working;
    let request = ed.flow_request.clone();
    let (tx, ctx) = (tx.clone(), ctx.clone());
    std::thread::spawn(move || {
        let status = make_kit(&request);
        let _ = tx.send(Msg::FlowKit(status));
        ctx.request_repaint();
    });
}

/// Writes the zip and puts it on the clipboard as a file. Blocks: call it off the UI thread.
fn make_kit(request: &str) -> KitStatus {
    match flow_kit::write_kit(&flow_kit::kit_dir(), request, system_name(platform::OS)) {
        Err(e) => KitStatus::Failed(format!("Could not make the kit: {e}")),
        Ok(path) if platform::native::clipboard_set_file(&path) => KitStatus::Copied(path),
        Ok(path) => KitStatus::NotCopied(path),
    }
}

/// Puts the zip's path on the clipboard as text, for an agent that runs in a terminal. On a worker
/// thread, like the kit itself.
fn copy_path(ctx: &egui::Context, tx: &Sender<Msg>, path: &std::path::Path) {
    let text = path.display().to_string();
    let (tx, ctx) = (tx.clone(), ctx.clone());
    std::thread::spawn(move || {
        let said = if platform::native::clipboard_set(&text) {
            "Path copied. Give it to an agent that can read files.".to_string()
        } else {
            format!("Nothing could use the clipboard. The kit is at {text}")
        };
        let _ = tx.send(Msg::Notice(said));
        ctx.request_repaint();
    });
}

/// The status line under the box: what happened to the kit.
fn status_line(ui: &mut egui::Ui, status: &KitStatus) {
    let (icon, color) = match status {
        KitStatus::Idle => return,
        KitStatus::Working => (symbols::CLOCK_COUNTER_CLOCKWISE, pal().muted),
        KitStatus::Copied(_) => (symbols::CHECK_CIRCLE, pal().good),
        KitStatus::NotCopied(_) => (symbols::WARNING, pal().bad),
        KitStatus::Failed(_) => (symbols::WARNING_CIRCLE, pal().bad),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            RichText::new(icon.to_string())
                .font(FontId::new(16.0, fonts::icons()))
                .color(color),
        );
        ui.add(egui::Label::new(RichText::new(status.notice()).size(13.0).color(color)).wrap());
    });
}

/// "Build it with AI": the box that looks like a chat, and the buttons around it.
fn ai_page(ctx: &egui::Context, edit: &mut Config, ed: &mut Ed, tx: &Sender<Msg>, accent: Color32) {
    let mut copy = false;
    let mut back = false;
    let mut reveal = false;
    let mut path_copy = false;
    let mut import = false;
    let ready = can_copy(&ed.flow_request, &ed.flow_kit);
    let known = ed.flow_kit.path().map(std::path::Path::to_path_buf);
    let (floating, ()) = sheet(
        ctx,
        "flow_new",
        ed.card,
        Vec2::new(600.0, 500.0),
        "Build a flow with AI",
        |ui| {
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "1. Say what the flow should do.  2. Copy the kit and paste it into your \
                         AI agent, then add a prompt such as \"make this\".  3. Import the file it \
                         gives you. Nothing is sent from here.",
                    )
                    .size(13.0)
                    .color(pal().muted),
                )
                .wrap(),
            );
            ui.add_space(GAP);
            // the box of a chat, with its button right under it: it sends nothing, it copies the kit
            ui.add(
                text_area(&mut ed.flow_request, 7)
                    .desired_width(f32::INFINITY)
                    .char_limit(MAX_REQUEST_CHARS)
                    .hint_text(
                        "Describe the flow you want, for example: open my notes app, wait a \
                         second, then paste what I copied.",
                    ),
            );
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                copy = button_if(
                    ui,
                    ready,
                    Look::Primary(accent),
                    Some(symbols::COPY),
                    "Copy kit",
                )
                .on_hover_text("Put the kit on the clipboard as a zip file")
                .clicked();
            });
            ui.add_space(GAP);
            status_line(ui, &ed.flow_kit);
            ui.add_space(GAP);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(8.0);
                back = button(ui, Look::Soft, None, "Back").clicked();
                reveal = button_if(
                    ui,
                    known.is_some(),
                    Look::Soft,
                    Some(symbols::FOLDER_OPEN),
                    "Show in folder",
                )
                .clicked();
                path_copy = button_if(
                    ui,
                    known.is_some(),
                    Look::Soft,
                    Some(symbols::CLIPBOARD_TEXT),
                    "Copy path",
                )
                .clicked();
                import = button(
                    ui,
                    Look::Soft,
                    Some(symbols::DOWNLOAD_SIMPLE),
                    "Import result\u{2026}",
                )
                .clicked();
            });
        },
    );
    if copy {
        start_kit(ctx, ed, tx);
    }
    if let Some(path) = known {
        if reveal {
            platform::reveal_in_file_manager(&path);
        }
        if path_copy {
            copy_path(ctx, tx, &path);
        }
    }
    if import {
        ed.flow_new = None;
        run(Do::Import, ctx, edit, ed, tx);
    } else if back {
        ed.flow_new = Some(FlowNew::Choose);
    } else if floating.close {
        ed.flow_new = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::app::App;

    #[test]
    fn the_kit_needs_a_few_words_and_is_not_made_twice_at_once() {
        assert!(!can_copy("", &KitStatus::Idle));
        assert!(!can_copy("  ab  ", &KitStatus::Idle));
        assert!(can_copy("open Notes", &KitStatus::Idle));
        assert!(!can_copy("open Notes", &KitStatus::Working));
        assert!(can_copy("open Notes", &KitStatus::Failed("x".into())));
    }

    #[test]
    fn the_agent_is_told_the_system_it_writes_for() {
        assert!(system_name(Os::Mac).starts_with("macOS"));
        assert!(system_name(Os::Windows).starts_with("Windows"));
        assert!(system_name(Os::Linux).starts_with("Linux"));
    }

    /// Draws the Flow tab for `frames` frames on a headless context.
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

    fn app() -> App {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        app.settings_open = true;
        app.edit = Config::default();
        app.opened = Some(app.edit.clone());
        app.ed.tab = crate::ui::settings::Tab::Flow;
        app
    }

    #[test]
    fn both_pages_draw_without_changing_the_config() {
        let mut a = app();
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        for page in [FlowNew::Choose, FlowNew::Ai, FlowNew::Templates] {
            a.ed.flow_new = Some(page);
            draw(&mut a, &ctx, 3);
            assert_eq!(a.ed.flow_new, Some(page), "the sheet waits for an answer");
            assert!(!a.has_unsaved_changes());
        }
    }

    #[test]
    fn a_template_adds_a_flow_and_opens_it() {
        let mut a = app();
        let before = a.edit.flows.len();
        let flow = flow_templates::build(&TEMPLATES[0], &a.edit.flows).unwrap();
        a.edit.flows.push(flow);
        select_flow(&mut a.ed, a.edit.flows.len() - 1);
        assert_eq!(a.edit.flows.len(), before + 1);
        assert_eq!(a.ed.flow, before);
    }

    #[test]
    fn the_sheet_draws_over_the_page_with_no_flow() {
        let mut a = app();
        a.edit.flows.clear();
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        a.ed.flow_new = Some(FlowNew::Choose);
        draw(&mut a, &ctx, 3);
        assert_eq!(a.ed.flow_new, Some(FlowNew::Choose));
        assert!(a.edit.flows.is_empty(), "choosing nothing adds nothing");
    }
}
