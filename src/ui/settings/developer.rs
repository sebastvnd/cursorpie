//! Developer mode in the About tab: a switch at the bottom, the sheet that offers the build with the
//! diagnostic log, and, once that build runs, a plain view of what its log found.
//!
//! The decisions that do not draw anything are in [`crate::devmode`]; the swap of the build is the
//! update's ([`App::start_build_swap`]). The layout, top to bottom:
//!
//! | Panel | Shown when | Holds |
//! |---|---|---|
//! | Developer tools | always, at the end of About | the switch and, with it on, the three steps of a bug report |
//! | Diagnostic build | developer mode on | which build runs, the way to the other build, whether the log is written (what to record is under *Advanced*) |
//! | What went wrong | developer mode on, log build | what the newest log says, in plain words (errors first, repeats counted), Refresh, Copy report, Open logs folder, Report an issue |
//! | Technical log | the same | the last lines of the log, by category, folded away until asked for |
//! | Terminal command | developer mode on | `cpd`, which reads the same logs in a terminal: installed and removed with one button ([`crate::cpdtool`]) |

use crate::about::{ISSUES_URL, LATEST_RELEASE_URL, VERSION};
use crate::devmode::{self, Level, RecentRow, Snapshot, Tally};
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

/// A length of time in words: `45 s`, `12 min`, `3 h 16 min`.
fn duration_words(seconds: f64) -> String {
    let s = seconds.max(0.0).round() as u64;
    match s {
        0..=59 => format!("{s} s"),
        60..=3599 => format!("{} min", s / 60),
        _ => format!("{} h {} min", s / 3600, (s % 3600) / 60),
    }
}

/// How often something happened: `once`, `3 times`.
fn times_words(count: u32) -> String {
    if count == 1 {
        "once".to_string()
    } else {
        format!("{count} times")
    }
}

/// A problem of the log in words a person can follow: a headline and one sentence that says what it
/// means and whether it matters. The log's own name and fields (`act.exec took 2187 ms
/// kind=open_app`) stay available, small, under it. Events this does not know keep their own name.
fn plain_problem(title: &str, detail: &str) -> (String, String) {
    let slow = title.split_once(" took ").and_then(|(name, rest)| {
        let ms: f64 = rest.trim_end_matches(" ms").trim().parse().ok()?;
        Some((name, ms))
    });
    if let Some((name, ms)) = slow {
        let secs = ms / 1000.0;
        return match name {
            "act.exec" => (
                "An action was slow to run".to_string(),
                format!(
                    "Running a slice's action took {secs:.1} s. Opening a heavy app does that; it is rarely a bug."
                ),
            ),
            _ => (
                "Something was slow".to_string(),
                format!("A step ({name}) took {secs:.1} s."),
            ),
        };
    }
    match title {
        "tap.press_blocked" => (
            "The pie button was pressed but the pie did not open".to_string(),
            if detail.contains("settings_open") {
                "Settings was open, and the pie stays closed while it is. This is normal.".to_string()
            } else {
                "Something kept the pie from opening. Note what you were doing and copy the report.".to_string()
            },
        ),
        "ui.pie_blocked" => (
            "The pie was kept from opening".to_string(),
            "The pie does not open while Settings is in front. This is normal.".to_string(),
        ),
        _ if title.ends_with(" failed") || title.ends_with(" refused") || title.ends_with(" denied") => (
            "Something CursorPie tried to do did not work".to_string(),
            "This one is worth reporting: copy the report and send it.".to_string(),
        ),
        _ if title.contains("blocked") || title.contains("unavailable") || title.contains("unreachable") => (
            "Something was not available".to_string(),
            "A warning is not always a bug. If something did not work for you, copy the report and send it.".to_string(),
        ),
        _ => (title.to_string(), String::new()),
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
            // The gaps are set here, one by one: with the spacing of the theme (GAP, 12 px) between
            // every two lines on top of the gaps added below, the points stood 33 px apart for text
            // that is 17 px tall and the sheet read as a scatter of lines, not as lists.
            let sp = ui.spacing_mut();
            sp.item_spacing.y = 0.0;
            // a row is as tall as its text, not as tall as a button (CONTROL_H)
            sp.interact_size.y = 0.0;
            ui.add(
                egui::Label::new(RichText::new(&content.lead).size(14.0).color(pal().text)).wrap(),
            );
            ui.add_space(6.0);
            for section in &content.sections {
                // the title has more room above (it starts a topic) than under it (it belongs to
                // the points below it)
                ui.add_space(12.0);
                ui.label(fonts::title(section.title, 13.0, pal().muted));
                ui.add_space(6.0);
                for point in &section.points {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.label(RichText::new("\u{2022}").size(14.0).color(pal().muted));
                        ui.add(
                            egui::Label::new(RichText::new(point).size(13.5).color(pal().text))
                                .wrap(),
                        );
                    });
                    ui.add_space(6.0);
                }
            }
            if let Some(notice) = &content.notice {
                ui.add_space(10.0);
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
        let shown = self.ed.dev_on;
        panel(ui, "Developer tools", |ui| {
            toggle_row(
                ui,
                &mut on,
                "Developer mode",
                "For reporting a problem. CursorPie keeps a private record of what it does, so you can see what went wrong and send it. The record stays on this computer unless you paste it somewhere yourself.",
                accent,
            );
            if shown {
                note(
                    ui,
                    "How to report a problem:\n1. Use CursorPie until the problem happens.\n2. Press Copy report under What went wrong.\n3. Press Report an issue and paste the report there.",
                );
            }
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
        }
        self.terminal_panel(ui, &ctx, accent);
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
        panel(ui, "Diagnostic build", |ui| {
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
                info_row(ui, "Recording", |ui| match (writing, &asked) {
                    (true, _) => status(ui, true, "Recording now"),
                    (false, Some(_)) => status(ui, true, "Starts at next launch"),
                    (false, None) => status(ui, false, "Off"),
                });
                let mut want = asked.is_some();
                let changed = toggle_row(
                    ui,
                    &mut want,
                    "Record what CursorPie does",
                    "Needed for the report below. A change applies the next time CursorPie starts. It records events and timings, never what you type.",
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
                    egui::CollapsingHeader::new(
                        RichText::new("Advanced: what to record")
                            .size(13.0)
                            .color(pal().muted),
                    )
                    .id_salt("dev_record_advanced")
                    .default_open(false)
                    .show(ui, |ui| {
                        row(
                            ui,
                            "What to record",
                            "all = everything (the default), tap,ui = only those parts, all,trace = also every pointer move, mem = memory use. Press Enter to save.",
                            |ui| {
                                let r = ui
                                    .add(text_field(&mut self.ed.dev_spec).desired_width(200.0));
                                apply = r.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            },
                        );
                    });
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
                    "Switch to the log build",
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
        panel(ui, "What went wrong", |ui| {
            match &snapshot {
                None => {
                    let why = error.as_deref().unwrap_or(
                        "Nothing recorded yet. Make sure Record what CursorPie does is on, restart CursorPie and use it: what went wrong shows up here.",
                    );
                    note(ui, why);
                }
                Some(s) => {
                    let clean = s.problems.is_empty();
                    row_wide(
                        ui,
                        &verdict(s),
                        &format!(
                            "Checked the record of {} ({} events, {}). A warning is not always a bug.",
                            if s.live {
                                "this session"
                            } else {
                                "an earlier session"
                            },
                            s.lines,
                            duration_words(s.seconds),
                        ),
                        120.0,
                        |ui| {
                            status(ui, clean, if clean { "All good" } else { "Take a look" });
                        },
                    );
                    for p in &s.problems {
                        let color = if p.error { pal().bad } else { warn_on_card() };
                        let (headline, meaning) = plain_problem(&p.title, &p.detail);
                        // the log's own words stay, small, for whoever reads the report
                        let raw = shorten(format!("{} {}", p.title, p.detail).trim(), DETAIL_CHARS);
                        let hint = if meaning.is_empty() {
                            raw
                        } else {
                            format!("{meaning} ({raw})")
                        };
                        row_wide(ui, &headline, &hint, 190.0, |ui| {
                            let kind = if p.error { "Error" } else { "Warning" };
                            badge(
                                ui,
                                &format!("{kind} \u{b7} {}", times_words(p.count)),
                                color,
                            );
                        });
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

    /// The last lines of the log, with what they add up to: numbers, a chart of the activity, the
    /// busiest parts, and the lines themselves, painted by how serious they are. Open by default: it
    /// is what a person who was asked for "the log" came here to read.
    fn recent_panel(&mut self, ui: &mut egui::Ui, accent: Color32) {
        let Some(s) = self.ed.dev_snapshot.clone() else {
            return;
        };
        let chosen = self.ed.dev_category.clone();
        let level = self.ed.dev_level;
        let big = self.ed.dev_log_big;
        let open = self.ed.dev_tech_open;
        let needle = self.ed.dev_search.trim().to_lowercase();
        let mut toggled = false;
        let mut picked: Option<Option<String>> = None;
        let visible: Vec<&RecentRow> = s
            .recent
            .iter()
            .filter(|r| chosen.as_deref().is_none_or(|c| r.cat == c))
            .filter(|r| level.is_none_or(|l| r.level >= l))
            .filter(|r| {
                needle.is_empty()
                    || format!("{} {} {}", r.cat, r.ev, r.detail)
                        .to_lowercase()
                        .contains(&needle)
            })
            .collect();
        panel(ui, "Technical log", |ui| {
            let folded = egui::CollapsingHeader::new(
                RichText::new(if open {
                    "Hide the log"
                } else {
                    "Show the log (for developers)"
                })
                .size(14.0)
                .color(pal().muted),
            )
            .id_salt("dev_technical_log")
            .open(Some(open))
            .show(ui, |ui| {
                note(
                    ui,
                    "One line per event, newest last: the time in seconds since start, ERR or WRN when something is wrong, the part of CursorPie (tap = mouse, ui = windows, win = window state, mem = memory, run = actions, smart = intelligent pie) and what happened. Red lines are errors, amber lines are warnings.",
                );
                term_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 8.0;
                    stat_tiles(ui, &s);
                    level_bar(ui, &s.levels);
                    term_heading(ui, "ACTIVITY OVER TIME");
                    timeline_chart(ui, &s);
                    term_heading(ui, "BUSIEST PARTS (click one to filter the lines)");
                    picked = category_bars(ui, &s, chosen.as_deref());
                });
                ui.add_space(8.0);
                actions(ui, |ui| {
                    let all = chosen.is_none();
                    if button(ui, if all { Look::Primary(accent) } else { Look::Soft }, None, "All parts")
                        .clicked()
                    {
                        self.ed.dev_category = None;
                    }
                    for (cat, n) in s.categories.iter().take(CHIPS) {
                        let on = chosen.as_deref() == Some(cat.as_str());
                        let look = if on { Look::Primary(accent) } else { Look::Soft };
                        if button(ui, look, None, &format!("{cat} {n}")).clicked() {
                            self.ed.dev_category = if on { None } else { Some(cat.clone()) };
                        }
                    }
                });
                actions(ui, |ui| {
                    for (label, want) in [
                        ("All levels", None),
                        ("Warnings + errors", Some(Level::Warning)),
                        ("Errors only", Some(Level::Error)),
                    ] {
                        let look = if level == want { Look::Primary(accent) } else { Look::Soft };
                        if button(ui, look, None, label).clicked() {
                            self.ed.dev_level = want;
                        }
                    }
                    ui.add(
                        text_field(&mut self.ed.dev_search)
                            .hint_text("Search the lines")
                            .desired_width(190.0),
                    );
                    if button(
                        ui,
                        Look::Soft,
                        None,
                        if big { "Smaller view" } else { "Bigger view" },
                    )
                    .clicked()
                    {
                        self.ed.dev_log_big = !big;
                    }
                    if button_if(
                        ui,
                        !visible.is_empty(),
                        Look::Soft,
                        Some(symbols::COPY),
                        "Copy these lines",
                    )
                    .clicked()
                    {
                        let text = visible
                            .iter()
                            .map(|r| format!("{:8.2} {:<5} {}", r.t, r.cat, r.text))
                            .collect::<Vec<_>>()
                            .join("\n");
                        ui.ctx().copy_text(text);
                        self.ed.dev_note = Some("The lines are on the clipboard.".into());
                    }
                });
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Showing {} of the last {} lines (the record has {}).",
                        visible.len(),
                        s.recent.len(),
                        s.lines
                    ))
                    .size(13.0)
                    .color(pal().muted),
                );
                ui.add_space(6.0);
                term_frame().inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let height = if big { LOG_TALL } else { LOG_COMPACT };
                    egui::ScrollArea::vertical()
                        .id_salt("dev_log_scroll")
                        .max_height(height)
                        .min_scrolled_height(height)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 1.0;
                            if visible.is_empty() {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("No line matches these filters.")
                                        .size(LOG_FONT)
                                        .color(TERM_DIM),
                                );
                            }
                            for r in &visible {
                                log_line(ui, r);
                            }
                        });
                });
            });
            toggled = folded.header_response.clicked();
        });
        if let Some(pick) = picked {
            self.ed.dev_category = pick;
        }
        if toggled {
            self.ed.dev_tech_open = !self.ed.dev_tech_open;
        }
    }

    /// The `cpd` command: what it is, and one button to install it and one to remove it.
    fn terminal_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, accent: Color32) {
        let installed = crate::cpdtool::installed();
        let available = crate::cpdtool::available_here();
        let busy = self.ed.cpd_busy;
        let mut install = false;
        let mut remove = false;
        panel(ui, "Terminal command (cpd)", |ui| {
            note(
                ui,
                "cpd is a small command that reads the same record in a terminal and names the same problems. You do not need it: everything above works without it. It is for people who like the terminal, and for reading a log somebody sent you.",
            );
            info_row(ui, "Status", |ui| match (&installed, busy) {
                (_, true) => status(ui, true, "Downloading\u{2026}"),
                (Some(_), false) => status(ui, true, "Installed"),
                (None, false) => status(ui, false, "Not installed"),
            });
            if let Some(path) = &installed {
                note(
                    ui,
                    &format!(
                        "Installed in {}. Paste the copied command into a terminal; add help to see what it can do (for example cpd help).",
                        path.display()
                    ),
                );
            } else if !available {
                note(ui, "There is no cpd for this system yet.");
            }
            if let Some(said) = &self.ed.cpd_note {
                note(ui, said);
            }
            actions(ui, |ui| {
                match &installed {
                    None => {
                        if button_if(
                            ui,
                            available && !busy,
                            Look::Primary(accent),
                            Some(symbols::DOWNLOAD_SIMPLE),
                            if busy {
                                "Downloading\u{2026}"
                            } else {
                                "Install cpd"
                            },
                        )
                        .clicked()
                        {
                            install = true;
                        }
                    }
                    Some(path) => {
                        if button(
                            ui,
                            Look::Primary(accent),
                            Some(symbols::COPY),
                            "Copy command",
                        )
                        .clicked()
                        {
                            ctx.copy_text(crate::cpdtool::run_command(path));
                            self.ed.cpd_note = Some(
                                "The command is on the clipboard: paste it into a terminal.".into(),
                            );
                        }
                        if button(ui, Look::Soft, Some(symbols::FOLDER_OPEN), "Show in folder")
                            .clicked()
                        {
                            platform::open_path(&crate::cpdtool::dir());
                        }
                        if button_if(ui, !busy, Look::Danger, Some(symbols::TRASH), "Remove cpd")
                            .clicked()
                        {
                            remove = true;
                        }
                    }
                }
                if button(
                    ui,
                    Look::Soft,
                    Some(symbols::TERMINAL_WINDOW),
                    "Other downloads",
                )
                .clicked()
                {
                    platform::open_url(LATEST_RELEASE_URL);
                }
            });
        });
        if install {
            self.ed.cpd_busy = true;
            self.ed.cpd_note = None;
            crate::cpdtool::spawn_install(crate::msg::Notifier {
                tx: self.tx.clone(),
                ctx: ctx.clone(),
            });
        }
        if remove {
            let result = crate::cpdtool::uninstall();
            self.cpd_done(false, result);
        }
    }

    /// `cpd` was installed or removed (or that failed): the answer goes into its panel.
    pub(crate) fn cpd_done(&mut self, installed: bool, result: Result<std::path::PathBuf, String>) {
        self.ed.cpd_busy = false;
        self.ed.cpd_note = Some(match (installed, result) {
            (true, Ok(_)) => "cpd is installed.".to_string(),
            (false, Ok(_)) => "cpd was removed.".to_string(),
            (true, Err(why)) => format!("cpd could not be installed. {why}"),
            (false, Err(why)) => why,
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
                    510.0
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

// ------------------------------------------------------------------------------ the log viewer
//
// The log is drawn on a dark surface whatever the theme: the colors below were chosen against it
// (every one is at least 4.5:1 on `TERM_BG`), so a line reads the same in every theme. Color is
// never the only signal: an error line starts with ERR, a warning with WRN.

/// Height of the list of lines, compact and tall.
const LOG_COMPACT: f32 = 380.0;
const LOG_TALL: f32 = 720.0;
/// Size of the text of the lines.
const LOG_FONT: f32 = 14.0;
/// How many parts get a bar in the chart of the busiest parts.
const BARS: usize = 8;

const TERM_BG: Color32 = Color32::from_rgb(0x15, 0x16, 0x1d);
const TERM_TILE: Color32 = Color32::from_rgb(0x24, 0x25, 0x30);
const TERM_TEXT: Color32 = Color32::from_rgb(0xe8, 0xe9, 0xf1);
const TERM_DIM: Color32 = Color32::from_rgb(0x9d, 0x9f, 0xb8);
const TERM_ERROR: Color32 = Color32::from_rgb(0xff, 0x6b, 0x6b);
const TERM_WARN: Color32 = Color32::from_rgb(0xff, 0xc8, 0x57);
const TERM_INFO: Color32 = Color32::from_rgb(0x6c, 0xb6, 0xff);
const TERM_OK: Color32 = Color32::from_rgb(0x6e, 0xe7, 0xa8);

/// The color of a level on the dark surface of the log.
fn level_color(level: Level) -> Color32 {
    match level {
        Level::Info => TERM_INFO,
        Level::Warning => TERM_WARN,
        Level::Error => TERM_ERROR,
    }
}

/// The amber of a warning on a panel of the theme: deep on a light panel, bright on a dark one
/// (the theme has a color for good and for bad, not for in between).
fn warn_on_card() -> Color32 {
    if pal().dark {
        TERM_WARN
    } else {
        Color32::from_rgb(0xb4, 0x53, 0x09)
    }
}

/// A fixed color for each part of CursorPie, so `tap` is the same blue in the chart and in the
/// lines. Parts this does not know get one of a few spare colors by their name.
fn category_color(cat: &str) -> Color32 {
    match cat {
        "tap" => Color32::from_rgb(0x5e, 0xc8, 0xff),
        "ui" => Color32::from_rgb(0xb6, 0x9c, 0xff),
        "win" => Color32::from_rgb(0x6e, 0xe7, 0xb7),
        "mem" => Color32::from_rgb(0xff, 0xa8, 0x6b),
        "run" => Color32::from_rgb(0xff, 0x8f, 0xd0),
        "smart" => Color32::from_rgb(0xf0, 0xe6, 0x8c),
        "act" => Color32::from_rgb(0x9b, 0xe1, 0x5d),
        other => {
            const SPARE: [Color32; 5] = [
                Color32::from_rgb(0x7d, 0xd3, 0xfc),
                Color32::from_rgb(0xd8, 0xb4, 0xfe),
                Color32::from_rgb(0xfd, 0xba, 0x74),
                Color32::from_rgb(0x86, 0xef, 0xac),
                Color32::from_rgb(0xf9, 0xa8, 0xd4),
            ];
            let h = other.bytes().fold(0usize, |a, b| {
                a.wrapping_mul(31).wrapping_add(usize::from(b))
            });
            SPARE[h % SPARE.len()]
        }
    }
}

/// The dark surface the log and its charts sit on.
fn term_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(TERM_BG)
        .corner_radius(12.0)
        .inner_margin(egui::Margin::same(14))
}

/// A small caps title over a chart.
fn term_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).size(12.0).strong().color(TERM_DIM));
}

/// One line of the log: time, level tag, part, event and fields, each in its own color; an error or
/// a warning also gets a tinted band behind it so it stands out when scrolling.
fn log_line(ui: &mut egui::Ui, r: &RecentRow) {
    let mono = egui::FontId::monospace(LOG_FONT);
    let fmt = |color: Color32| egui::text::TextFormat {
        font_id: mono.clone(),
        color,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.wrap.break_anywhere = true;
    job.append(&format!("{:>8.2}  ", r.t), 0.0, fmt(TERM_DIM));
    let tag = match r.level {
        Level::Error => "ERR ",
        Level::Warning => "WRN ",
        Level::Info => "    ",
    };
    job.append(tag, 0.0, fmt(level_color(r.level)));
    job.append(&format!("{:<5} ", r.cat), 0.0, fmt(category_color(&r.cat)));
    let event = if r.level == Level::Info {
        TERM_TEXT
    } else {
        level_color(r.level)
    };
    job.append(&r.ev, 0.0, fmt(event));
    if !r.detail.is_empty() {
        job.append(&format!("  {}", r.detail), 0.0, fmt(TERM_DIM));
    }
    let fill = match r.level {
        Level::Info => Color32::TRANSPARENT,
        other => wash(level_color(other), 0.14),
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(job).wrap());
        });
}

/// Four big numbers: how many events, errors and warnings, and how long the record is.
fn stat_tiles(ui: &mut egui::Ui, s: &Snapshot) {
    let count_color = |n: u32, bad: Color32| if n > 0 { bad } else { TERM_OK };
    let items = [
        ("EVENTS", s.lines.to_string(), TERM_TEXT),
        (
            "ERRORS",
            s.levels.errors.to_string(),
            count_color(s.levels.errors, TERM_ERROR),
        ),
        (
            "WARNINGS",
            s.levels.warnings.to_string(),
            count_color(s.levels.warnings, TERM_WARN),
        ),
        ("DURATION", duration_words(s.seconds), TERM_TEXT),
    ];
    ui.columns(items.len(), |cols| {
        for (col, (label, value, color)) in cols.iter_mut().zip(items) {
            egui::Frame::new()
                .fill(TERM_TILE)
                .corner_radius(10.0)
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(col, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(label).size(11.5).strong().color(TERM_DIM));
                    ui.label(RichText::new(value).size(26.0).strong().color(color));
                });
        }
    });
}

/// One bar split in errors, warnings and the rest, with a legend that names each color.
fn level_bar(ui: &mut egui::Ui, t: &Tally) {
    let total = t.total().max(1) as f32;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 12.0), egui::Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 6.0, TERM_TILE);
    let mut x = rect.min.x;
    for (n, color) in [
        (t.errors, TERM_ERROR),
        (t.warnings, TERM_WARN),
        (t.info, TERM_INFO),
    ] {
        if n == 0 {
            continue;
        }
        let w = (rect.width() * n as f32 / total)
            .max(4.0)
            .min(rect.max.x - x);
        let seg = egui::Rect::from_min_size(egui::pos2(x, rect.min.y), Vec2::new(w, rect.height()));
        painter.rect_filled(seg.shrink2(Vec2::new(0.5, 0.0)), 3.0, color);
        x += w;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (name, n, color) in [
            ("Errors", t.errors, TERM_ERROR),
            ("Warnings", t.warnings, TERM_WARN),
            ("Other events", t.info, TERM_INFO),
        ] {
            ui.label(RichText::new("\u{25CF}").size(13.0).color(color));
            ui.label(
                RichText::new(format!("{name} {n}"))
                    .size(13.0)
                    .color(TERM_TEXT),
            );
            ui.add_space(10.0);
        }
    });
}

/// Events per slice of time, as bars: errors at the foot, then warnings, then everything else.
/// Hovering a bar says what happened in that slice.
fn timeline_chart(ui: &mut egui::Ui, s: &Snapshot) {
    let n = s.timeline.len();
    if n == 0 {
        return;
    }
    let plot_h = 84.0;
    let axis_h = 18.0;
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), plot_h + axis_h),
        egui::Sense::hover(),
    );
    let painter = ui.painter().clone();
    let plot = egui::Rect::from_min_size(rect.min, Vec2::new(rect.width(), plot_h));
    let slot = plot.width() / n as f32;
    let max = s
        .timeline
        .iter()
        .map(Tally::total)
        .max()
        .unwrap_or(1)
        .max(1) as f32;
    let hovered = resp
        .hover_pos()
        .filter(|p| plot.contains(*p))
        .map(|p| (((p.x - plot.min.x) / slot) as usize).min(n - 1));
    let faint = wash(Color32::WHITE, 0.16);
    painter.hline(plot.x_range(), plot.max.y, egui::Stroke::new(1.0, faint));
    painter.hline(
        plot.x_range(),
        plot.min.y,
        egui::Stroke::new(1.0, wash(Color32::WHITE, 0.06)),
    );
    for (i, b) in s.timeline.iter().enumerate() {
        let x0 = plot.min.x + slot * i as f32;
        if hovered == Some(i) {
            let col = egui::Rect::from_min_max(
                egui::pos2(x0, plot.min.y),
                egui::pos2(x0 + slot, plot.max.y),
            );
            painter.rect_filled(col, 3.0, wash(Color32::WHITE, 0.08));
        }
        let mut y = plot.max.y - 1.0;
        for (count, color) in [
            (b.errors, TERM_ERROR),
            (b.warnings, TERM_WARN),
            (b.info, TERM_INFO),
        ] {
            if count == 0 {
                continue;
            }
            let h = ((plot_h - 8.0) * count as f32 / max).max(2.0);
            let bar = egui::Rect::from_min_max(
                egui::pos2(x0 + 1.0, y - h),
                egui::pos2(x0 + slot - 1.0_f32.min(slot / 3.0), y),
            );
            painter.rect_filled(bar, 1.5, color);
            y -= h;
        }
    }
    let axis_y = plot.max.y + axis_h / 2.0 + 1.0;
    let font = egui::FontId::proportional(12.0);
    painter.text(
        egui::pos2(rect.min.x, axis_y),
        egui::Align2::LEFT_CENTER,
        "start",
        font.clone(),
        TERM_DIM,
    );
    painter.text(
        egui::pos2(rect.max.x, axis_y),
        egui::Align2::RIGHT_CENTER,
        duration_words(s.seconds),
        font,
        TERM_DIM,
    );
    if let Some(i) = hovered {
        let b = s.timeline[i];
        let per = s.seconds / n as f64;
        resp.on_hover_text(format!(
            "{} to {}\n{} events, {} warnings, {} errors",
            duration_words(per * i as f64),
            duration_words(per * (i + 1) as f64),
            b.total(),
            b.warnings,
            b.errors,
        ));
    }
}

/// A bar for each of the busiest parts of CursorPie. Clicking one filters the lines to it (or
/// clears the filter when it was the chosen one): the answer is `Some(new filter)`.
fn category_bars(ui: &mut egui::Ui, s: &Snapshot, chosen: Option<&str>) -> Option<Option<String>> {
    let max = s.categories.first().map_or(1, |c| c.1).max(1) as f32;
    let all: usize = s.categories.iter().map(|c| c.1).sum::<usize>().max(1);
    let mut picked = None;
    ui.spacing_mut().item_spacing.y = 2.0;
    for (cat, n) in s.categories.iter().take(BARS) {
        let (rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), egui::Sense::click());
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let on = chosen == Some(cat.as_str());
        let painter = ui.painter().clone();
        if on || resp.hovered() {
            painter.rect_filled(
                rect,
                6.0,
                wash(Color32::WHITE, if on { 0.11 } else { 0.05 }),
            );
        }
        let color = category_color(cat);
        painter.text(
            egui::pos2(rect.min.x + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            cat,
            egui::FontId::monospace(LOG_FONT),
            color,
        );
        let bar = egui::Rect::from_min_max(
            egui::pos2(rect.min.x + 78.0, rect.center().y - 6.0),
            egui::pos2(rect.max.x - 112.0, rect.center().y + 6.0),
        );
        painter.rect_filled(bar, 6.0, TERM_TILE);
        let w = (bar.width() * *n as f32 / max).max(4.0).min(bar.width());
        painter.rect_filled(
            egui::Rect::from_min_size(bar.min, Vec2::new(w, bar.height())),
            6.0,
            color,
        );
        painter.text(
            egui::pos2(rect.max.x - 8.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            format!("{n}  ({}%)", n * 100 / all),
            egui::FontId::proportional(13.0),
            TERM_TEXT,
        );
        if resp.clicked() {
            picked = Some(if on { None } else { Some(cat.clone()) });
        }
    }
    picked
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
    fn times_and_counts_are_said_in_words() {
        assert_eq!(duration_words(0.4), "0 s");
        assert_eq!(duration_words(45.0), "45 s");
        assert_eq!(duration_words(725.0), "12 min");
        assert_eq!(duration_words(11823.0), "3 h 17 min");
        assert_eq!(times_words(1), "once");
        assert_eq!(times_words(3), "3 times");
    }

    #[test]
    fn the_problems_of_the_log_are_told_in_plain_words() {
        let (head, meaning) = plain_problem("act.exec took 2187 ms", "kind=open_app");
        assert_eq!(head, "An action was slow to run");
        assert!(meaning.contains("2.2 s"));
        let (_, normal) = plain_problem("tap.press_blocked", "reason=settings_open");
        assert!(normal.contains("normal"));
        let (_, other) = plain_problem("tap.press_blocked", "reason=other");
        assert!(!other.contains("normal"));
        let (_, normal) = plain_problem("ui.pie_blocked", "touch=false");
        assert!(normal.contains("normal"));
        let (head, meaning) = plain_problem("save failed", "");
        assert!(head.contains("did not work") && meaning.contains("report"));
        // what is not known keeps its own name
        assert_eq!(
            plain_problem("odd.thing", ""),
            ("odd.thing".to_string(), String::new())
        );
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

    fn row(t: f64, cat: &str, ev: &str, level: Level) -> RecentRow {
        RecentRow {
            t,
            cat: cat.into(),
            ev: ev.into(),
            detail: "reason=settings_open".into(),
            level,
            text: format!("{ev} reason=settings_open"),
        }
    }

    #[test]
    fn levels_and_parts_have_their_own_colors() {
        assert_ne!(level_color(Level::Error), level_color(Level::Warning));
        assert_ne!(level_color(Level::Warning), level_color(Level::Info));
        assert_eq!(category_color("tap"), category_color("tap"));
        assert_ne!(category_color("tap"), category_color("ui"));
        // a part nobody named still gets one color, always the same
        assert_eq!(category_color("odd"), category_color("odd"));
    }

    #[test]
    fn the_technical_log_draws_with_charts_in_every_state() {
        let mut snap = Snapshot {
            lines: 5,
            seconds: 30.0,
            recent: vec![
                row(1.0, "tap", "press", Level::Info),
                row(2.0, "tap", "press_blocked", Level::Warning),
                row(3.0, "act", "exec failed", Level::Error),
            ],
            categories: vec![("tap".into(), 4), ("act".into(), 1)],
            ..Default::default()
        };
        snap.levels = Tally {
            info: 3,
            warnings: 1,
            errors: 1,
        };
        snap.timeline = crate::devmode::bucketize(
            &[
                (1.0, Level::Info),
                (2.0, Level::Warning),
                (29.0, Level::Error),
            ],
            30.0,
            crate::devmode::BUCKETS,
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        app.ed.dev_snapshot = Some(snap);
        app.ed.dev_tech_open = true;
        for (big, level, search) in [
            (false, None, ""),
            (true, Some(Level::Warning), ""),
            (false, Some(Level::Error), "nomatch"),
        ] {
            app.ed.dev_log_big = big;
            app.ed.dev_level = level;
            app.ed.dev_search = search.into();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(900.0, 800.0),
                )),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| app.recent_panel(ui, Color32::GREEN))
                .drop_without_applying_deltas();
        }
        // an empty record draws too
        app.ed.dev_snapshot = Some(Snapshot::default());
        ctx.run_ui(egui::RawInput::default(), |ui| {
            app.recent_panel(ui, Color32::GREEN)
        })
        .drop_without_applying_deltas();
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
