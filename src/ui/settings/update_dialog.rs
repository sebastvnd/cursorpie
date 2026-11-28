//! The "update available" sheet, offered when Settings opens and a newer release is known.
//!
//! It shows at most once a day (see [`update::plan`](crate::update::plan)): *Install update*
//! downloads, checks and installs the new version (see [`update::install`](crate::update::install)),
//! *Release page* opens the page in the browser, *Not now* (Esc, the close button, a click outside)
//! closes it and the About tab keeps showing the update. A copy that cannot update itself (not
//! installed in the usual place) gets *Download page* instead of *Install update*. It opens over the
//! Settings window like the other sheets, and the same on every OS.
//!
//! Two more sheets tell what happens after *Install update*, because CursorPie quits for the swap
//! and the person would otherwise see a window that just vanishes:
//!
//! * the **progress sheet** ([`App::update_progress_dialog`]) stays over Settings while the package
//!   is downloaded and checked, and says what comes next (CursorPie closes, the new version is put
//!   in place and starts by itself, which can take a few minutes);
//! * the **result sheet** ([`App::update_done_dialog`]) opens on the first start after the swap
//!   (see [`update::take_outcome`](crate::update::take_outcome)): "updated to X", or that the update
//!   did not finish and the old version is running.

use crate::about::{LATEST_RELEASE_URL, VERSION};
use crate::platform::{self, Os};
use crate::ui::app::App;
use crate::ui::mascot::Pose;
use crate::ui::settings::theme::*;
use crate::ui::settings::widgets::*;
use crate::ui::symbols;
use crate::update::install::{self, Progress, Step};
use crate::update::{Outcome, Status};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

/// What the sheet asked for this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// Nothing yet: keep showing the sheet.
    Pending,
    /// Download and install the update, and close the sheet.
    Install,
    /// Open the release page and close the sheet.
    Page,
    /// Close the sheet.
    NotNow,
}

/// What the sheet says under its title.
fn body_text(
    version: crate::update::Version,
    can_install: bool,
    needs_password: bool,
    blocker: Option<&install::Blocker>,
) -> String {
    let current = VERSION;
    if !can_install {
        // say why: "not installed in the usual place" alone leaves the person with a button and no
        // idea what to change
        let why = blocker.map_or_else(
            || "This copy of CursorPie was not installed in the usual place, so it cannot update itself.".to_string(),
            install::Blocker::explain,
        );
        return format!(
            "You are using {current}. {why}\n\nThe page opens in your browser, where you pick the \
             package for your system."
        );
    }
    let mut text = format!(
        "You are using {current}. Install update downloads {version} from GitHub, checks it \
         against the checksum of the release, replaces this copy and starts CursorPie again. Your \
         settings are kept."
    );
    if platform::OS == Os::Mac {
        text.push_str(" macOS may ask you to allow Accessibility for CursorPie again.");
    }
    if needs_password {
        text.push_str(
            " This copy is in a folder only an administrator can write to, so your system asks \
             for your password when CursorPie restarts; if you cancel, the old version starts again.",
        );
    }
    text
}

/// What the progress sheet says while the package is downloaded and checked.
fn progress_text(needs_password: bool) -> String {
    let mut text = String::from(
        "CursorPie is downloading the update and checking it against the checksum of the release.\n\n\
         When that is done, CursorPie closes by itself, the new version is put in place and \
         CursorPie starts again. This can take a few minutes, and it is normal that nothing is on \
         screen for a moment. Please do not start CursorPie yourself or turn off the computer until \
         it is back.",
    );
    if platform::OS == Os::Windows {
        text.push_str(
            " A small \u{201c}Setup\u{201d} window shows the progress while it installs.",
        );
    }
    if needs_password {
        text.push_str(" Your system will ask for your password when CursorPie restarts.");
    }
    text
}

/// What the progress sheet says over the bar, and the numbers at its right.
fn progress_label(p: Progress) -> (&'static str, String) {
    match p.step {
        Step::Looking => ("Looking for the package\u{2026}", String::new()),
        Step::Downloading => {
            let numbers = match p.fraction() {
                Some(f) => format!("{:.0} %", (f * 100.0).floor()),
                None => String::new(),
            };
            ("Downloading\u{2026}", numbers)
        }
        Step::Checking => ("Checking the download\u{2026}", "100 %".to_string()),
        Step::Preparing => ("Getting it ready\u{2026}", "100 %".to_string()),
    }
}

/// "12.4 of 31.0 MB" under the bar while the size is known.
fn progress_sizes(p: Progress) -> String {
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    if p.step == Step::Downloading && p.total > 0 {
        format!("{:.1} of {:.1} MB", mb(p.done), mb(p.total))
    } else {
        String::new()
    }
}

/// An animated progress bar. With `Some(fraction)` the fill glides to it and a soft shine runs
/// along the fill; with `None` a short piece runs back and forth on the track.
fn progress_bar(ui: &mut egui::Ui, ctx: &egui::Context, fraction: Option<f32>, accent: Color32) {
    const H: f32 = 12.0;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), H), egui::Sense::hover());
    let t = ctx.input(|i| i.time) as f32;
    let painter = ui.painter();
    let r = H / 2.0;
    painter.rect_filled(rect, r, pal().idle);
    let track = painter.with_clip_rect(rect);
    match fraction {
        Some(target) => {
            let shown = ctx.animate_value_with_time(ui.id().with("bar"), target, 0.3);
            let fill = Rect::from_min_size(
                rect.min,
                Vec2::new((rect.width() * shown).max(H.min(rect.width())), H),
            );
            if shown > 0.0 {
                track.rect_filled(fill, r, accent);
                // a soft band of light that runs along the fill again and again
                let band = 70.0_f32;
                let x = fill.min.x - band + (t * 0.55 % 1.0) * (fill.width() + band);
                let glint = Rect::from_min_max(
                    Pos2::new(x.max(fill.min.x), fill.min.y),
                    Pos2::new((x + band).min(fill.max.x), fill.max.y),
                );
                if glint.width() > 0.0 {
                    track
                        .with_clip_rect(fill)
                        .rect_filled(glint, r, Color32::from_white_alpha(70));
                }
            }
        }
        None => {
            let piece = rect.width() * 0.3;
            // 0 -> 1 -> 0: back and forth, slowing down at the ends
            let k = 0.5 - 0.5 * (t * 1.6).cos();
            let x = rect.min.x + (rect.width() - piece) * k;
            track.rect_filled(
                Rect::from_min_size(Pos2::new(x, rect.min.y), Vec2::new(piece, H)),
                r,
                accent,
            );
        }
    }
}

/// What the result sheet says: its title, the text under it and whether it is good news.
fn done_text(outcome: Outcome) -> (String, String, bool) {
    match outcome {
        Outcome::Updated { from, to } => (
            format!("CursorPie is now {to}"),
            format!(
                "The update from {from} is finished and CursorPie is running again. Your settings \
                 were kept."
            ),
            true,
        ),
        Outcome::Unfinished { from, to } => (
            format!("CursorPie {to} was not installed"),
            format!(
                "The update did not finish, so CursorPie {from} is running again and nothing was \
                 lost. The usual causes are a folder that cannot be written to, a security program \
                 that blocked the swap, or CursorPie being started by hand while it ran; the About \
                 tab says when it knows which. You can press Install update there to try again, or \
                 download {to} from the release page."
            ),
            false,
        ),
    }
}

impl App {
    /// Draws the progress sheet while the update is being downloaded and checked. It has no answer:
    /// it ends by itself when the download does (the swap follows and CursorPie quits, or the About
    /// tab shows why it did not work), and Esc and clicks outside leave it open.
    pub(super) fn update_progress_dialog(&mut self, ctx: &egui::Context, accent: egui::Color32) {
        let Status::Downloading(version) = self.ed.update else {
            return;
        };
        let needs_password = self.ed.update_needs_password;
        // 14 more than with a glyph: the mascot is taller
        let height = if needs_password { 414.0 } else { 384.0 };
        let progress = install::progress();
        // the close button and a click outside mean nothing here: the download goes on
        let (_floating, ()) = sheet(
            ctx,
            "update_progress",
            self.ed.card,
            Vec2::new(560.0, height),
            "Updating CursorPie",
            |ui| {
                sheet_heading_pose(
                    ui,
                    Pose::Downloading,
                    &format!("Downloading CursorPie {version}"),
                );
                let (what, percent) = progress_label(progress);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(what).size(13.5).color(pal().text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(crate::ui::fonts::title(percent, 14.0, pal().text));
                    });
                });
                ui.add_space(6.0);
                progress_bar(ui, ctx, progress.fraction(), accent);
                ui.add_space(4.0);
                muted(ui, &progress_sizes(progress));
                ui.add_space(GAP);
                sheet_text(ui, &progress_text(needs_password));
            },
        );
        // the bar moves with the file: look again soon
        ctx.request_repaint_after(std::time::Duration::from_millis(50));
    }

    /// Draws the result sheet while `Ed::update_done` is set; any way of closing it ends it.
    pub(super) fn update_done_dialog(&mut self, ctx: &egui::Context, accent: egui::Color32) {
        let Some(outcome) = self.ed.update_done else {
            return;
        };
        let (title, text, good) = done_text(outcome);
        let mut page = false;
        let mut close = false;
        let (floating, ()) = sheet(
            ctx,
            "update_done",
            self.ed.card,
            Vec2::new(520.0, 264.0), // 14 more than with a glyph: the mascot is taller
            if good {
                "Update finished"
            } else {
                "Update not finished"
            },
            |ui| {
                let pose = if good { Pose::Success } else { Pose::Sad };
                sheet_heading_pose(ui, pose, &title);
                sheet_text(ui, &text);
                ui.add_space(GAP);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if button(ui, Look::Primary(accent), None, "OK").clicked() {
                        close = true;
                    }
                    if !good && button(ui, Look::Soft, None, "Release page").clicked() {
                        page = true;
                    }
                });
            },
        );
        if page {
            diag::log!("ui", "update_download");
            platform::open_url(LATEST_RELEASE_URL);
        }
        if close || page || floating.close {
            self.ed.update_done = None;
        }
    }

    /// Draws the sheet while `Ed::update_offer` is set and applies the answer.
    pub(super) fn update_dialog(&mut self, ctx: &egui::Context, accent: egui::Color32) {
        let Some(version) = self.ed.update_offer else {
            return;
        };
        let can_install = self.ed.update_can_install;
        let mut answer = Answer::Pending;
        let (floating, ()) = sheet(
            ctx,
            "update_offer",
            self.ed.card,
            Vec2::new(
                560.0,
                // 14 more than with a glyph: the mascot is taller
                if self.ed.update_needs_password || self.ed.update_blocker.is_some() {
                    354.0
                } else {
                    284.0
                },
            ),
            "Update available",
            |ui| {
                sheet_heading_pose(
                    ui,
                    Pose::Notification,
                    &format!("CursorPie {version} is available"),
                );
                let text = body_text(
                    version,
                    can_install,
                    self.ed.update_needs_password,
                    self.ed.update_blocker.as_ref(),
                );
                sheet_text(ui, &text);
                ui.add_space(GAP);
                // the way forward is the rightmost button, the way back the one before it
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if can_install {
                        let install = button(
                            ui,
                            Look::Primary(accent),
                            Some(symbols::DOWNLOAD_SIMPLE),
                            "Install update",
                        );
                        if install.clicked() {
                            answer = Answer::Install;
                        }
                    }
                    if button(ui, Look::Soft, None, "Not now").clicked() {
                        answer = Answer::NotNow;
                    }
                    let (look, label) = if can_install {
                        (Look::Soft, "Release page")
                    } else {
                        (Look::Primary(accent), "Download page")
                    };
                    if button(ui, look, None, label).clicked() {
                        answer = Answer::Page;
                    }
                });
            },
        );
        // the close button and a click outside the sheet mean "not now"
        if floating.close && answer == Answer::Pending {
            answer = Answer::NotNow;
        }
        match answer {
            Answer::Pending => {}
            Answer::NotNow => self.ed.update_offer = None,
            Answer::Install => self.start_update_install(ctx),
            Answer::Page => {
                self.ed.update_offer = None;
                diag::log!("ui", "update_download");
                platform::open_url(LATEST_RELEASE_URL);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_result_sheet_tells_good_news_from_bad() {
        let v = |t| crate::update::Version::parse(t).unwrap();
        let (title, _, good) = done_text(Outcome::Updated {
            from: v("0.8.19"),
            to: v("0.9.0"),
        });
        assert!(good && title.contains("0.9.0"));
        let (title, text, good) = done_text(Outcome::Unfinished {
            from: v("0.8.19"),
            to: v("0.9.0"),
        });
        assert!(!good && title.contains("0.9.0") && text.contains("0.8.19"));
    }

    #[test]
    fn the_offer_says_why_a_copy_cannot_update_itself() {
        let v = crate::update::Version::parse("9.9.9").unwrap();
        let generic = body_text(v, false, false, None);
        assert!(generic.contains("not installed in the usual place"));
        let why = install::Blocker::Translocated;
        let text = body_text(v, false, false, Some(&why));
        assert!(text.contains("Applications"), "{text}");
        assert!(
            !text.contains("usual place"),
            "the reason replaces the guess"
        );
        // a copy that can update itself says nothing about a blocker
        let ok = body_text(v, true, false, Some(&why));
        assert!(!ok.contains("Applications folder"));
    }

    #[test]
    fn the_progress_sheet_says_what_follows_the_download() {
        let text = progress_text(false);
        assert!(text.contains("closes by itself") && text.contains("starts again"));
        assert!(progress_text(true).contains("password"));
    }

    #[test]
    fn the_progress_sheet_names_the_step_and_the_size() {
        let at = |step, done, total| Progress { step, done, total };
        let (what, pct) = progress_label(at(Step::Downloading, 50, 200));
        assert!(what.starts_with("Downloading") && pct == "25 %");
        assert_eq!(progress_label(at(Step::Looking, 0, 0)).1, "");
        assert_eq!(progress_label(at(Step::Checking, 9, 9)).1, "100 %");
        let mib = 1024 * 1024;
        assert_eq!(
            progress_sizes(at(Step::Downloading, 5 * mib, 20 * mib)),
            "5.0 of 20.0 MB"
        );
        assert_eq!(progress_sizes(at(Step::Checking, 20 * mib, 20 * mib)), "");
    }

    #[test]
    fn without_an_offer_nothing_is_drawn_and_nothing_changes() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut app = App::new(rx, tx, 200.0, None);
        let ctx = egui::Context::default();
        app.update_dialog(&ctx, egui::Color32::GREEN);
        assert!(app.ed.update_offer.is_none());
    }
}
