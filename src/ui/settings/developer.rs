//! Developer mode in the About tab: a switch at the bottom, the sheet that offers the build with the
//! diagnostic log, and, once that build runs, a plain view of what its log found.
//!
//! The decisions that do not draw anything are in [`crate::devmode`]; the swap of the build is the
//! update's ([`App::start_build_swap`]). The layout, top to bottom:
//!
//! | Panel | Shown when | Holds |
//! |---|---|---|
//! | Developer | always, at the end of About | the switch; with it on: which build runs, the way to the other build, whether the log is written and what it records |
//! | Problems | developer mode on, log build | what the newest log says went wrong (errors first, repeats counted), Refresh, Copy report, Open logs folder, Report an issue |
//! | Recent events | the same | the last lines of the log, by category |
//! | Terminal | the same | the `cpd` command, which reads the same logs and says the same things |

use crate::about::{ISSUES_URL, LATEST_RELEASE_URL, VERSION};
use crate::devmode::{self, Snapshot};
use crate::platform;
use crate::ui::app::App;
use crate::ui::fonts;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use crate::update::install::Build;
use eframe::egui::{self, Color32, RichText, Vec2};
use std::time::{Duration, Instant};

/// How often the panel reads the log again while it is on screen.
const REFRESH: Duration = Duration::from_secs(3);
/// How many characters of a problem's detail are shown (the whole text is in the copied report).
const DETAIL_CHARS: usize = 160;

/// Height kept under the text of a sheet for its row of buttons.
const BUTTON_ROW: f32 = 56.0;

/// A sheet of developer mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offer {
    /// "Install the log build": this copy has no log.
    LogBuild,
    /// "Back to the standard build".
    StandardBuild,
    /// Developer mode is being turned off in the log build: go back to the standard build too, or
    /// keep this one.
    TurnOff,
}

/// The categories offered as filter chips: the busiest few.
const CHIPS: usize = 6;

/// What a sheet asked for this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Pending,
    Install,
    Page,
    NotNow,
    /// Turn developer mode off and keep the log build.
    Keep,
}

/// `text` cut to at most `max` characters, with an ellipsis when it was longer.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}\u{2026}", cut.trim_end())
}

/// The one line that says what the log found: `No problems`, `2 errors, 1 warning`.
fn verdict(snapshot: &Snapshot) -> String {
    let errors = snapshot.problems.iter().filter(|p| p.error).count();
    let warnings = snapshot.problems.len() - errors;
    if snapshot.problems.is_empty() {
        return "No problems found".to_string();
    }
    let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
    match (errors, warnings) {
        (0, w) => plural(w, "warning"),
        (e, 0) => plural(e, "error"),
        (e, w) => format!("{}, {}", plural(e, "error"), plural(w, "warning")),
    }
}

/// A titled list of short points in a sheet.
#[derive(Clone, Debug, PartialEq)]
struct Section {
    title: &'static str,
    points: Vec<String>,
}

/// What a sheet says: one sentence, then the points by topic, then a notice when something stands in
/// the way. Short lines the eye can scan, not a paragraph.
#[derive(Clone, Debug, PartialEq)]
struct OfferContent {
    lead: String,
    sections: Vec<Section>,
    /// What stops or changes the install on this copy.
    notice: Option<String>,
}

fn points(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|l| (*l).to_string()).collect()
}

/// What the sheet says under its title.
fn offer_content(offer: Offer, can_install: bool, needs_password: bool) -> OfferContent {
    let (lead, sections) = match offer {
        Offer::LogBuild => (
            format!(
                "Developer mode needs the build of CursorPie {VERSION} that keeps a diagnostic log. \
                 The ordinary download has none of its code."
            ),
            vec![
                Section {
                    title: "What happens",
                    points: vec![
                        format!("Downloads CursorPie {VERSION} with the log from GitHub"),
                        "Checks it against the checksum of the release".to_string(),
                        "Replaces this copy and starts CursorPie again, with the log switched on"
                            .to_string(),
                    ],
                },
                Section {
                    title: "What stays",
                    points: points(&[
                        "Your settings are kept",
                        "You can come back to the ordinary build in the same place",
                    ]),
                },
                Section {
                    title: "About the log",
                    points: points(&[
                        "It stays on this computer: nothing is sent anywhere until you copy it into a bug report yourself",
                        "It records what CursorPie does (events, pointer positions, timings), not what you type",
                    ]),
                },
            ],
        ),
        Offer::StandardBuild => (
            format!("This replaces the log build with the ordinary CursorPie {VERSION}."),
            vec![
                Section {
                    title: "What happens",
                    points: points(&[
                        "Downloads the same version without the log and checks it",
                        "Replaces this copy and starts CursorPie again",
                    ]),
                },
                Section {
                    title: "What stays",
                    points: points(&[
                        "Your settings are kept",
                        "The logs already written are kept",
                    ]),
                },
            ],
        ),
        Offer::TurnOff => (
            "You are turning developer mode off, and this copy is the build with the log."
                .to_string(),
            vec![
                Section {
                    title: "Install the standard build (recommended)",
                    points: points(&[
                        "Replaces this copy with the ordinary CursorPie of the same version",
                        "The log is switched off and the developer tools are hidden",
                        "Your settings and the logs already written are kept",
                    ]),
                },
                Section {
                    title: "Keep the log build",
                    points: points(&[
                        "The developer tools are hidden and the log stops the next time CursorPie starts",
                        "This copy stays the build with the log until you install the standard one: turn developer mode on again to see the button",
                    ]),
                },
            ],
        ),
    };
    let notice = if !can_install {
        Some(
            "This copy was not installed in the usual place, so it cannot replace itself. \
             Download page opens the release page, where the log builds are named CursorPie-devlog-\u{2026}"
                .to_string(),
        )
    } else if needs_password {
        Some(
            "This copy is in a folder only an administrator can write to: your system asks for your \
             password when CursorPie restarts."
                .to_string(),
        )
    } else {
        None
    };
    OfferContent {
        lead,
        sections,
        notice,
    }
}

/// Draws [`OfferContent`] in a scroll area at most `max_height` high.
fn draw_offer_content(ui: &mut egui::Ui, content: &OfferContent, max_height: f32) {
    egui::ScrollArea::vertical()
        .id_salt("developer_offer_scroll")
        .max_height(max_height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(RichText::new(&content.lead).size(14.0).color(pal().text)).wrap(),
            );
            for section in &content.sections {
                ui.add_space(14.0);
                ui.label(fonts::title(section.title, 13.0, pal().muted));
                ui.add_space(4.0);
                for point in &section.points {
                    ui.horizontal_top(|ui| {
                        let sp = ui.spacing_mut();
                        sp.item_spacing = Vec2::new(8.0, 0.0);
                        // a row is as tall as its text, not as tall as a button (CONTROL_H)
                        sp.interact_size.y = 0.0;
                        ui.label(RichText::new("\u{2022}").size(14.0).color(pal().muted));
                        ui.add(
                            egui::Label::new(RichText::new(point).size(13.5).color(pal().text))
                                .wrap(),
                        );
                    });
                    ui.add_space(4.0);
                }
            }
            if let Some(notice) = &content.notice {
                ui.add_space(14.0);
                egui::Frame::new()
                    .fill(wash(pal().text, 0.06))
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.label(fonts::icon(symbols::WARNING_CIRCLE, 16.0, pal().muted));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(notice).size(13.5).color(pal().text),
                                )
                                .wrap(),
                            );
                        });
                    });
            }
            ui.add_space(4.0);
        });
}

impl App {
    /// Reads the log again (and says so in `dev_error` when it cannot).
    pub(in crate::ui::settings) fn dev_refresh(&mut self) {
        match devmode::snapshot() {
            Ok(found) => {
                self.ed.dev_snapshot = found;
                self.ed.dev_error = None;
            }
            Err(e) => {
                self.ed.dev_snapshot = None;
                self.ed.dev_error = Some(format!("The log could not be read: {e}"));
            }
        }
        self.ed.dev_read_at = Some(Instant::now());
    }

    /// The panels of developer mode, below the other panels of the About tab.
    pub(in crate::ui::settings) fn developer_panels(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let ctx = ui.ctx().clone();
        let mut on = self.ed.dev_on;
        panel(ui, "Developer", |ui| {
            toggle_row(
                ui,
                &mut on,
                "Developer mode",
                "Tools for finding a problem: the build with the diagnostic log, a list of what went wrong, and the cpd command.",
                accent,
            );
        });
        if on != self.ed.dev_on {
            self.set_developer_mode(on);
        }
        if !self.ed.dev_on {
            return;
        }
        self.build_panel(ui, accent);
        if devmode::LOG_BUILD {
            self.problems_panel(ui, &ctx, accent);
            self.recent_panel(ui, accent);
            terminal_panel(ui, &ctx);
        }
    }

    /// Developer mode was switched. Turning it on in a copy without the log offers the log build; in
    /// a copy with it, asks for the log from the next start (unless the person already chose).
    fn set_developer_mode(&mut self, on: bool) {
        // in the build with the log, turning it off asks first: leaving this build is the other half
        if !on && devmode::LOG_BUILD {
            self.ed.dev_offer = Some(Offer::TurnOff);
            return;
        }
        if let Err(e) = devmode::set_on(on) {
            self.say(format!("Could not change developer mode: {e}"));
            return;
        }
        diag::log!("ui", "developer_mode", on = on);
        self.ed.dev_on = on;
        if !on {
            self.ed.dev_offer = None;
            return;
        }
        if devmode::LOG_BUILD {
            if devmode::log_spec().is_none() {
                self.dev_set_spec(Some(devmode::DEFAULT_SPEC));
            }
            self.dev_refresh();
        } else {
            self.ed.dev_offer = Some(Offer::LogBuild);
        }
    }

    /// Developer mode off, and the log with it (it would go on being written with nobody looking).
    pub(crate) fn leave_developer_mode(&mut self) {
        if let Err(e) = devmode::set_on(false) {
            self.say(format!("Could not change developer mode: {e}"));
            return;
        }
        let _ = devmode::set_log_spec(None);
        diag::log!("ui", "developer_mode", on = false);
        self.ed.dev_on = false;
        self.ed.dev_offer = None;
        self.ed.dev_snapshot = None;
        self.ed.dev_note = None;
    }

    /// Writes the switch of the log (or removes it) and says when it takes effect.
    fn dev_set_spec(&mut self, spec: Option<&str>) {
        match devmode::set_log_spec(spec) {
            Ok(()) => {
                self.ed.dev_spec = devmode::log_spec().unwrap_or_default();
                let now = diag::is_on();
                self.ed.dev_note = Some(match (spec.is_some(), now) {
                    (true, true) => "Saved. A change of what is recorded applies the next time CursorPie starts.".into(),
                    (true, false) => "Saved. The log starts the next time CursorPie starts.".into(),
                    (false, true) => "Saved. The log stops the next time CursorPie starts.".into(),
                    (false, false) => "The log is off.".into(),
                });
            }
            Err(e) => self.say(format!("Could not change the log: {e}")),
        }
    }

    /// Which build runs, the way to the other one, and the switch of the log.
    fn build_panel(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let busy = matches!(
            self.ed.update,
            crate::update::Status::Checking
                | crate::update::Status::Downloading(_)
                | crate::update::Status::Ready(_)
        );
        panel(ui, "Build", |ui| {
            info_row(ui, "This copy", |ui| {
                if devmode::LOG_BUILD {
                    badge(ui, "Log build", pal().good);
                } else {
                    badge(ui, "Standard build", pal().muted);
                }
            });
            if devmode::LOG_BUILD {
                let writing = diag::is_on();
                let asked = devmode::log_spec();
                info_row(ui, "Log", |ui| match (writing, &asked) {
                    (true, _) => status(ui, true, "Writing"),
                    (false, Some(_)) => status(ui, true, "On at next start"),
                    (false, None) => status(ui, false, "Off"),
                });
                let mut want = asked.is_some();
                let changed = toggle_row(
                    ui,
                    &mut want,
                    "Write the log",
                    "Takes effect the next time CursorPie starts.",
                    accent,
                )
                .changed();
                if changed {
                    self.dev_set_spec(want.then_some(devmode::DEFAULT_SPEC));
                }
                if asked.is_some() {
                    if self.ed.dev_spec.is_empty() {
                        self.ed.dev_spec = asked.unwrap_or_default();
                    }
                    let mut apply = false;
                    row(
                        ui,
                        "What to record",
                        "all, a list such as tap,ui, all,trace for every pointer move, mem for memory",
                        |ui| {
                            let r = ui.add(text_field(&mut self.ed.dev_spec).desired_width(200.0));
                            apply = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        },
                    );
                    if apply {
                        let spec = self.ed.dev_spec.clone();
                        self.dev_set_spec(Some(&spec));
                    }
                }
            }
            if let Some(said) = &self.ed.dev_note {
                note(ui, said);
            }
            actions(ui, |ui| {
                if devmode::LOG_BUILD {
                    if button_if(
                        ui,
                        !busy,
                        Look::Soft,
                        Some(symbols::DOWNLOAD_SIMPLE),
                        "Back to the standard build",
                    )
                    .clicked()
                    {
                        self.ed.dev_offer = Some(Offer::StandardBuild);
                    }
                } else if button_if(
                    ui,
                    !busy,
                    Look::Primary(accent),
                    Some(symbols::DOWNLOAD_SIMPLE),
                    "Install log build",
                )
                .clicked()
                {
                    self.ed.dev_offer = Some(Offer::LogBuild);
                }
                if button(ui, Look::Soft, None, "Release page").clicked() {
                    platform::open_url(LATEST_RELEASE_URL);
                }
            });
        });
    }

    /// What the newest log says went wrong.
    fn problems_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        // read on first sight and every few seconds while the panel is on screen
        let due = self.ed.dev_read_at.is_none_or(|t| t.elapsed() >= REFRESH);
        if due {
            self.dev_refresh();
        }
        ctx.request_repaint_after(REFRESH);
        let snapshot = self.ed.dev_snapshot.clone();
        let error = self.ed.dev_error.clone();
        let mut refresh = false;
        let mut copy = None;
        panel(ui, "Problems", |ui| {
            match &snapshot {
                None => {
                    let why = error.as_deref().unwrap_or(
                        "No log yet. Turn the log on above, restart CursorPie and use it; what went wrong shows here.",
                    );
                    note(ui, why);
                }
                Some(s) => {
                    let clean = s.problems.is_empty();
                    row_wide(
                        ui,
                        &verdict(s),
                        &format!(
                            "{} \u{b7} {} lines \u{b7} {:.0} s{}",
                            s.file,
                            s.lines,
                            s.seconds,
                            if s.live { "" } else { " \u{b7} an earlier run" }
                        ),
                        120.0,
                        |ui| {
                            status(ui, clean, if clean { "OK" } else { "Look" });
                        },
                    );
                    for p in &s.problems {
                        let color = if p.error { pal().bad } else { pal().muted };
                        row_wide(
                            ui,
                            &p.title,
                            &shorten(&p.detail, DETAIL_CHARS),
                            150.0,
                            |ui| {
                                let when = format!("x{} \u{b7} {:.1} s", p.count, p.first_t);
                                badge(ui, &when, color);
                            },
                        );
                    }
                    copy = Some(s.text.clone());
                }
            }
            actions(ui, |ui| {
                refresh =
                    button(ui, Look::Soft, Some(symbols::ARROWS_CLOCKWISE), "Refresh").clicked();
                if button_if(
                    ui,
                    copy.is_some(),
                    Look::Primary(accent),
                    Some(symbols::COPY),
                    "Copy report",
                )
                .clicked()
                    && let Some(text) = copy.take()
                {
                    ui.ctx().copy_text(text);
                    self.ed.dev_note =
                        Some("The report is on the clipboard: paste it into the issue.".into());
                }
                if button(
                    ui,
                    Look::Soft,
                    Some(symbols::FOLDER_OPEN),
                    "Open logs folder",
                )
                .clicked()
                {
                    let dir = devmode::logs_dir();
                    let _ = std::fs::create_dir_all(&dir);
                    platform::open_path(&dir);
                }
                if button(ui, Look::Soft, Some(symbols::BUG), "Report an issue").clicked() {
                    platform::open_url(ISSUES_URL);
                }
            });
        });
        if refresh {
            self.dev_refresh();
        }
    }

    /// The last lines of the log, by category.
    fn recent_panel(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let Some(s) = self.ed.dev_snapshot.clone() else {
            return;
        };
        let chosen = self.ed.dev_category.clone();
        panel(ui, "Recent events", |ui| {
            actions(ui, |ui| {
                if button(
                    ui,
                    if chosen.is_none() {
                        Look::Primary(accent)
                    } else {
                        Look::Soft
                    },
                    None,
                    "All",
                )
                .clicked()
                {
                    self.ed.dev_category = None;
                }
                for (cat, n) in s.categories.iter().take(CHIPS) {
                    let on = chosen.as_deref() == Some(cat.as_str());
                    let look = if on {
                        Look::Primary(accent)
                    } else {
                        Look::Soft
                    };
                    if button(ui, look, None, &format!("{cat} {n}")).clicked() {
                        self.ed.dev_category = if on { None } else { Some(cat.clone()) };
                    }
                }
            });
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let shown = s
                        .recent
                        .iter()
                        .filter(|r| chosen.as_deref().is_none_or(|c| r.cat == c));
                    for r in shown {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("{:7.2}  {:<5} {}", r.t, r.cat, r.text))
                                    .size(12.5)
                                    .color(pal().muted),
                            )
                            .truncate(),
                        );
                    }
                });
        });
    }

    /// Draws the sheet while `Ed::dev_offer` is set and applies the answer.
    pub(in crate::ui::settings) fn developer_dialog(
        &mut self,
        ctx: &egui::Context,
        accent: Color32,
    ) {
        let Some(offer) = self.ed.dev_offer else {
            return;
        };
        let can_install = self.ed.update_can_install;
        let mut answer = Answer::Pending;
        let content = offer_content(offer, can_install, self.ed.update_needs_password);
        let (floating, ()) = sheet(
            ctx,
            "developer_offer",
            self.ed.card,
            Vec2::new(
                580.0,
                if matches!(offer, Offer::LogBuild | Offer::TurnOff) {
                    560.0
                } else {
                    400.0
                },
            ),
            "Developer mode",
            |ui| {
                let (glyph, title, yes) = match offer {
                    Offer::LogBuild => (
                        symbols::BUG,
                        "Install the build with the log?",
                        "Install log build",
                    ),
                    Offer::StandardBuild => (
                        symbols::DOWNLOAD_SIMPLE,
                        "Go back to the standard build?",
                        "Install standard build",
                    ),
                    Offer::TurnOff => (
                        symbols::BUG,
                        "Turn off developer mode",
                        "Install standard build",
                    ),
                };
                sheet_heading(ui, glyph, accent, title);
                // the text scrolls when the window is small; the buttons stay in sight under it
                draw_offer_content(ui, &content, (ui.available_height() - BUTTON_ROW).max(80.0));
                ui.add_space(GAP);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if can_install
                        && button(
                            ui,
                            Look::Primary(accent),
                            Some(symbols::DOWNLOAD_SIMPLE),
                            yes,
                        )
                        .clicked()
                    {
                        answer = Answer::Install;
                    }
                    if offer == Offer::TurnOff
                        && button(ui, Look::Soft, None, "Keep log build").clicked()
                    {
                        answer = Answer::Keep;
                    }
                    let no = if offer == Offer::TurnOff {
                        "Cancel"
                    } else {
                        "Not now"
                    };
                    if button(ui, Look::Soft, None, no).clicked() {
                        answer = Answer::NotNow;
                    }
                    let look = if can_install {
                        Look::Soft
                    } else {
                        Look::Primary(accent)
                    };
                    let label = if can_install {
                        "Release page"
                    } else {
                        "Download page"
                    };
                    if button(ui, look, None, label).clicked() {
                        answer = Answer::Page;
                    }
                });
            },
        );
        if floating.close && answer == Answer::Pending {
            answer = Answer::NotNow;
        }
        match answer {
            Answer::Pending => {}
            Answer::NotNow => self.ed.dev_offer = None,
            Answer::Page => {
                self.ed.dev_offer = None;
                platform::open_url(LATEST_RELEASE_URL);
            }
            Answer::Keep => self.leave_developer_mode(),
            Answer::Install => {
                let build = match offer {
                    Offer::LogBuild => Build::Log,
                    Offer::StandardBuild | Offer::TurnOff => Build::Standard,
                };
                self.start_build_swap(ctx, build, offer == Offer::TurnOff);
            }
        }
    }
}

/// The `cpd` command, which reads the logs in a terminal.
fn terminal_panel(ui: &mut egui::Ui, ctx: &egui::Context) {
    panel(ui, "Terminal", |ui| {
        note(
            ui,
            "The command cpd reads the same logs in a terminal and names the same problems. \
             cpd: what went wrong in the newest log. cpd tl: one row per press. cpd grep ui:show_pie: \
             the lines of an event. cpd f: follow a run as it is written. cpd on and cpd off switch the log. \
             cpd help lists the rest. It is attached to every release as cpd-<system>.",
        );
        actions(ui, |ui| {
            if button(ui, Look::Soft, Some(symbols::COPY), "Copy \"cpd\"").clicked() {
                ctx.copy_text("cpd".to_string());
            }
            if button(
                ui,
                Look::Soft,
                Some(symbols::TERMINAL_WINDOW),
                "Download cpd",
            )
            .clicked()
            {
                platform::open_url(LATEST_RELEASE_URL);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::devmode::ProblemRow;

    fn problem(error: bool) -> ProblemRow {
        ProblemRow {
            error,
            title: "t".into(),
            detail: String::new(),
            count: 1,
            first_t: 0.0,
        }
    }

    #[test]
    fn the_verdict_counts_in_words() {
        let mut s = Snapshot::default();
        assert_eq!(verdict(&s), "No problems found");
        s.problems = vec![problem(true)];
        assert_eq!(verdict(&s), "1 error");
        s.problems = vec![problem(false), problem(false)];
        assert_eq!(verdict(&s), "2 warnings");
        s.problems = vec![problem(true), problem(true), problem(false)];
        assert_eq!(verdict(&s), "2 errors, 1 warning");
    }

    #[test]
    fn a_long_detail_is_cut_with_an_ellipsis() {
        assert_eq!(shorten("short", 10), "short");
        let cut = shorten(&"x".repeat(300), 20);
        assert_eq!(cut.chars().count(), 21);
        assert!(cut.ends_with('\u{2026}'));
    }

    #[test]
    fn the_sheet_is_points_by_topic_not_a_paragraph() {
        let c = offer_content(Offer::LogBuild, true, false);
        let titles: Vec<_> = c.sections.iter().map(|s| s.title).collect();
        assert_eq!(titles, ["What happens", "What stays", "About the log"]);
        let all = c
            .sections
            .iter()
            .flat_map(|s| &s.points)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("checksum") && all.contains("settings are kept"));
        assert!(all.contains("stays on this computer"));
        assert!(
            c.sections
                .iter()
                .all(|s| s.points.iter().all(|p| p.len() < 130))
        );
        assert_eq!(c.notice, None);
    }

    #[test]
    fn a_copy_that_cannot_replace_itself_or_needs_a_password_is_told_so() {
        let cannot = offer_content(Offer::LogBuild, false, false).notice.unwrap();
        assert!(cannot.contains("devlog"));
        let password = offer_content(Offer::LogBuild, true, true).notice.unwrap();
        assert!(password.contains("password"));
        let back = offer_content(Offer::StandardBuild, true, false);
        assert!(
            back.lead.contains("without the log")
                || back.sections[0].points[0].contains("without the log")
        );
        assert!(
            back.sections[1]
                .points
                .iter()
                .any(|p| p.contains("logs already written"))
        );
    }

    #[test]
    fn the_sheet_draws_in_a_big_and_in_a_small_window() {
        for (w, h) in [(1000.0, 700.0), (560.0, 380.0)] {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut app = App::new(rx, tx, 200.0, None);
            let ctx = egui::Context::default();
            crate::ui::fonts::install(&ctx);
            for offer in [Offer::LogBuild, Offer::StandardBuild, Offer::TurnOff] {
                app.ed.dev_offer = Some(offer);
                app.ed.card = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
                let input = egui::RawInput {
                    screen_rect: Some(app.ed.card),
                    ..Default::default()
                };
                ctx.begin_pass(input);
                app.developer_dialog(&ctx, Color32::GREEN);
                ctx.end_pass().drop_without_applying_deltas();
                assert_eq!(
                    app.ed.dev_offer,
                    Some(offer),
                    "drawing alone answers nothing"
                );
            }
        }
    }

    #[test]
    fn turning_it_off_in_the_log_build_asks_before_anything_changes() {
        // a debug build is a log build; nothing is written, the sheet asks
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        app.ed.dev_on = true;
        app.set_developer_mode(false);
        assert_eq!(app.ed.dev_offer, Some(Offer::TurnOff));
        assert!(app.ed.dev_on, "still on until the person answers");
        let c = offer_content(Offer::TurnOff, true, false);
        assert!(c.sections[0].title.contains("recommended"));
        assert_eq!(c.sections.len(), 2);
    }

    #[test]
    fn nothing_is_drawn_without_an_offer() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        let ctx = egui::Context::default();
        app.developer_dialog(&ctx, Color32::GREEN);
        assert!(app.ed.dev_offer.is_none());
    }
}
