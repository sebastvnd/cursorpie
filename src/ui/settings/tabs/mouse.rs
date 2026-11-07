//! The "Mouse" panel of the General tab: which mouse is in use, how many extra buttons it has, a scan
//! to find them by pressing them, and the mice CursorPie knows.
//!
//! The mice are device state (`mice.json`), not part of the config: what happens here is saved at
//! once, and Save, Undo and backups do not touch it.

use crate::mouse::{self, MouseProfile, Recognition};
use crate::ui::app::App;
use crate::ui::draw::rgb;
use crate::ui::settings::demo;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use eframe::egui::{self, RichText};
use std::time::Duration;

/// The names of the buttons, "Back button, Extra button 6", or a dash when there are none.
fn names_of(buttons: &[i64]) -> String {
    if buttons.is_empty() {
        return "none".into();
    }
    buttons
        .iter()
        .map(|b| mouse::button_label(*b))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The end of the status line of "Detect again": what the look at the trackpads found, or nothing
/// where the OS has no trackpad support.
fn trackpad_clause(found: Option<usize>) -> String {
    match found {
        None => String::new(),
        Some(0) => " No trackpad found.".to_string(),
        Some(1) => " 1 trackpad found.".to_string(),
        Some(n) => format!(" {n} trackpads found."),
    }
}

/// How long ago, for the list of mice.
fn ago(seconds: u64) -> String {
    match seconds {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", seconds / 60),
        3600..=86_399 => format!("{} h ago", seconds / 3600),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

/// What the person asked for in this frame; applied after the panel is drawn.
#[derive(Default)]
struct Wanted {
    detect: bool,
    start_scan: bool,
    done: bool,
    cancel: bool,
    forget: Option<String>,
}

impl App {
    /// Draws the Mouse panel.
    pub(in crate::ui::settings) fn mouse_panel(&mut self, ui: &mut egui::Ui) {
        // a scan that nobody finishes ends by itself, so no button stays swallowed
        if self.ed.scan.as_ref().is_some_and(mouse::Scan::idle) {
            self.ed.scan = None;
            self.say("Scan stopped: no button was pressed for a while.");
        }
        let book = mouse::snapshot();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut want = Wanted::default();
        let scanning = self.ed.scan.is_some();
        let pressed = self.ed.scan.as_ref().map(mouse::Scan::pressed);
        let accent = rgb(self.edit.accent);

        panel(ui, "Mouse", |ui| {
            demo::scan_demo(ui, accent);
            match book.active_profile() {
                Some(p) => {
                    info_row(ui, "In use", |ui| {
                        ui.label(RichText::new(&p.name).color(pal().text));
                    });
                    info_row(ui, "Extra buttons", |ui| {
                        if p.is_scanned() {
                            status(ui, true, "scanned");
                        } else {
                            ui.label(RichText::new("reported by the system").color(pal().muted));
                        }
                        ui.label(RichText::new(p.buttons().len().to_string()).color(pal().text));
                    });
                    note(ui, &names_of(&p.buttons()));
                }
                None => note(
                    ui,
                    "No mouse found yet. Scan it to tell CursorPie which buttons it has.",
                ),
            }

            if let Some(list) = &pressed {
                note(
                    ui,
                    &format!(
                        "Press every extra button of your mouse once. Heard so far: {}. The buttons do nothing else while the scan runs.",
                        names_of(list)
                    ),
                );
                actions(ui, |ui| {
                    if button_if(
                        ui,
                        !list.is_empty(),
                        Look::Primary(accent),
                        Some(symbols::CHECK),
                        "Done",
                    )
                    .clicked()
                    {
                        want.done = true;
                    }
                    if button(ui, Look::Soft, None, "Cancel").clicked() {
                        want.cancel = true;
                    }
                });
            } else {
                actions(ui, |ui| {
                    if button(
                        ui,
                        Look::Soft,
                        Some(symbols::MOUSE_SIMPLE),
                        "Scan this mouse",
                    )
                    .clicked()
                    {
                        want.start_scan = true;
                    }
                    if button(
                        ui,
                        Look::Soft,
                        Some(symbols::ARROW_CLOCKWISE),
                        "Detect again",
                    )
                    .clicked()
                    {
                        want.detect = true;
                    }
                });
                note(
                    ui,
                    "Detect again asks the system which mice are connected. Scan finds the buttons by pressing them: use it when the list is wrong or empty.",
                );
            }

            if book.mice.len() > 1 || (book.mice.len() == 1 && book.active_profile().is_none()) {
                for p in &book.mice {
                    mouse_row(ui, p, p.id == book.active, now, &mut want);
                }
            }
            note(
                ui,
                "Saved at once, not with Save. Slices set to 'Side buttons' can then use every button listed here.",
            );
        });

        if scanning {
            // the presses arrive as messages; look again soon so the list and the timeout stay current
            ui.ctx().request_repaint_after(Duration::from_millis(400));
        }
        if want.start_scan {
            self.ed.scan = Some(mouse::Scan::start());
        }
        if want.cancel {
            self.ed.scan = None;
        }
        if want.done
            && let Some(list) = pressed
        {
            self.ed.scan = None;
            let n = mouse::finish_scan(&list);
            self.say(format!("Saved: this mouse has {n} extra buttons."));
        }
        if let Some(id) = want.forget {
            mouse::forget(&id);
            self.say("Mouse forgotten. It is recognized as new when it is connected again.");
        }
        if want.detect {
            let out = mouse::refresh();
            // the same look also counts the trackpads (it starts nothing; the Trackpad panel
            // shows the result and a scan there can switch the hold on)
            let pads = crate::platform::OS
                .has_trackpad()
                .then(crate::tap::touch_scan);
            let mouse_part = match out.recognition {
                Recognition::Absent => {
                    "No mouse was listed by the system. Use Scan to find the buttons.".to_string()
                }
                _ => format!("{}: {} extra buttons.", out.name, out.extra),
            };
            self.say(format!("{mouse_part}{}", trackpad_clause(pads)));
        }
    }
}

/// One line of the list of known mice.
fn mouse_row(ui: &mut egui::Ui, p: &MouseProfile, in_use: bool, now: u64, want: &mut Wanted) {
    let detail = format!(
        "{} extra \u{b7} {}",
        p.buttons().len(),
        ago(now.saturating_sub(p.last_seen))
    );
    let name = if in_use {
        format!("{} (in use)", p.name)
    } else {
        p.name.clone()
    };
    row(ui, &name, &detail, |ui| {
        if button(ui, Look::Plain, Some(symbols::TRASH), "Forget").clicked() {
            want.forget = Some(p.id.clone());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_are_listed_by_name() {
        assert_eq!(names_of(&[]), "none");
        assert_eq!(
            names_of(&[3, 4, 5]),
            "Back button, Forward button, Extra button 6"
        );
    }

    #[test]
    fn detecting_mice_also_reports_the_trackpads() {
        assert_eq!(trackpad_clause(None), "");
        assert_eq!(trackpad_clause(Some(0)), " No trackpad found.");
        assert_eq!(trackpad_clause(Some(1)), " 1 trackpad found.");
        assert_eq!(trackpad_clause(Some(2)), " 2 trackpads found.");
    }

    #[test]
    fn ages_are_short() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(120), "2 min ago");
        assert_eq!(ago(7200), "2 h ago");
        assert_eq!(ago(3 * 86_400), "3 days ago");
    }
}
