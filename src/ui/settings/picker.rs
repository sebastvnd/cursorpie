//! Native file choosers run off the UI thread, one at a time.
//!
//! The chooser is a window of the system, not a child of Settings, so Settings stays clickable
//! beside it. While one is open Settings is dimmed and ignores input ([`waiting_scrim`]), a second
//! chooser cannot be started, and a press of the trigger does not pull Settings over the chooser.

use super::theme::{CARD_RADIUS, pal};
use crate::msg::Msg;
use crate::platform::{self, FileKind};
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

/// Set from the click that starts a chooser until the chooser is gone.
static CHOOSING: AtomicBool = AtomicBool::new(false);

/// Counts the times the user gave up waiting: a chooser that was given up on must not touch the
/// state of the next one when it finally ends.
static ROUND: AtomicU64 = AtomicU64::new(0);

/// Whether a native file chooser is open (or about to be).
pub fn choosing() -> bool {
    CHOOSING.load(Ordering::SeqCst)
}

/// Takes the right to open a chooser. `None` while another one is open. The right is given back
/// when the returned guard is dropped, also if the worker thread panics.
fn claim() -> Option<Release> {
    (!CHOOSING.swap(true, Ordering::SeqCst)).then(|| Release(ROUND.load(Ordering::SeqCst)))
}

/// Gives the chooser back when dropped, unless the user gave up on it before.
struct Release(u64);

impl Release {
    /// The user pressed "Stop waiting" while this chooser was open.
    fn abandoned(&self) -> bool {
        ROUND.load(Ordering::SeqCst) != self.0
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        if !self.abandoned() {
            CHOOSING.store(false, Ordering::SeqCst);
        }
    }
}

/// Gives up waiting for the chooser that is open (one that hangs, or hides behind other windows):
/// Settings is usable again and the chooser's answer, if it ever comes, is dropped.
pub(super) fn stop_waiting() {
    diag::log!("ui", "chooser_abandoned");
    ROUND.fetch_add(1, Ordering::SeqCst);
    CHOOSING.store(false, Ordering::SeqCst);
}

/// Opens the native file chooser on a worker thread (it blocks until the user answers), turns the
/// chosen path into a message with `make`, and finally asks the UI to take focus back. Does
/// nothing while another chooser is open.
///
/// `kind` says what the chooser accepts; `make` may return `None` to drop
/// the choice (for example when the file turns out to be unusable).
pub(super) fn spawn_file_picker(
    tx: Sender<Msg>,
    ctx: egui::Context,
    kind: FileKind,
    prompt: &'static str,
    make: impl FnOnce(String) -> Option<Msg> + Send + 'static,
) {
    let Some(release) = claim() else {
        diag::log!("ui", "chooser_busy");
        return;
    };
    diag::log!("ui", "chooser_open");
    let started = std::time::Instant::now();
    ctx.request_repaint(); // the dimming shows at once
    std::thread::spawn(move || {
        let picked = platform::choose_file(kind, prompt);
        if release.abandoned() {
            return; // the user stopped waiting: the answer comes too late
        }
        if let Some(file) = picked
            && let Some(msg) = make(file)
        {
            let _ = tx.send(msg);
        }
        std::thread::sleep(Duration::from_millis(120)); // give the OS time to hand focus back
        drop(release);
        diag::log!("ui", "chooser_closed", waited_ms = started.elapsed());
        let _ = tx.send(Msg::Refocus);
        ctx.request_repaint();
        // After Cancel the system may hand the focus to the window below Settings a moment *after*
        // the first request (macOS does so when `osascript` quits). `Msg::Refocus` does nothing
        // when Settings has the focus already, so a second one is free when it is not needed.
        std::thread::sleep(Duration::from_millis(250));
        let _ = tx.send(Msg::Refocus);
        ctx.request_repaint();
    });
}

/// Side of the round "stop waiting" button.
const STOP_SIZE: f32 = 32.0;

/// A red circle with a white X: gives up waiting for the chooser. It has no text, so it carries a
/// tooltip and an accessible label.
fn stop_button(ui: &mut egui::Ui, rect: egui::Rect) -> egui::Response {
    const LABEL: &str = "Stop waiting";
    let resp = ui
        .interact(rect, egui::Id::new("chooser_stop"), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(LABEL);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, LABEL));
    let red = pal().bad;
    let fill = if resp.is_pointer_button_down_on() {
        red.lerp_to_gamma(egui::Color32::BLACK, 0.25)
    } else if resp.hovered() {
        red.lerp_to_gamma(egui::Color32::BLACK, 0.12)
    } else {
        red
    };
    let painter = ui.painter();
    let center = rect.center();
    painter.circle_filled(center, rect.width() / 2.0, fill);
    let arm = rect.width() * 0.19;
    let stroke = egui::Stroke::new(2.4, egui::Color32::WHITE);
    painter.line_segment(
        [
            center + egui::vec2(-arm, -arm),
            center + egui::vec2(arm, arm),
        ],
        stroke,
    );
    painter.line_segment(
        [
            center + egui::vec2(-arm, arm),
            center + egui::vec2(arm, -arm),
        ],
        stroke,
    );
    resp
}

/// While a chooser is open: dims the window over `bounds` and takes every click, so nothing
/// underneath can be changed or started a second time.
pub(super) fn waiting_scrim(ctx: &egui::Context, bounds: egui::Rect) {
    egui::Area::new(egui::Id::new("chooser_scrim"))
        .order(egui::Order::Foreground)
        .fixed_pos(bounds.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.allocate_rect(bounds, egui::Sense::click_and_drag());
            ui.painter().rect_filled(bounds, CARD_RADIUS, pal().scrim);
            // a pill under the words: the dimmed window shows through the scrim, and its text would
            // run into the words
            let galley = ui.painter().layout_no_wrap(
                "Waiting for the file chooser\u{2026}".to_string(),
                egui::FontId::proportional(15.0),
                pal().text,
            );
            let pill = egui::Rect::from_center_size(
                bounds.center(),
                galley.size() + egui::vec2(40.0, 24.0),
            );
            ui.painter()
                .rect_filled(pill, pill.height() / 2.0, pal().card);
            ui.painter()
                .galley(pill.center() - galley.size() / 2.0, galley, pal().text);
            // the way out when the chooser never answers or cannot be found
            let below = egui::Rect::from_center_size(
                pill.center_bottom() + egui::vec2(0.0, 36.0),
                egui::vec2(STOP_SIZE, STOP_SIZE),
            );
            let clicked = stop_button(ui, below).clicked();
            if clicked {
                stop_waiting();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_chooser_at_a_time_and_it_is_given_back() {
        let first = claim();
        assert!(first.is_some());
        assert!(choosing());
        assert!(
            claim().is_none(),
            "a second click must not open a second chooser"
        );
        drop(first);
        assert!(!choosing());
        let again = claim();
        assert!(again.is_some(), "free again once the first one is gone");
        drop(again);
    }

    #[test]
    fn giving_up_frees_settings_and_the_old_chooser_cannot_undo_it() {
        let old = claim().expect("free");
        stop_waiting();
        assert!(!choosing(), "usable again at once");
        assert!(old.abandoned());
        let new = claim().expect("a new chooser may start");
        drop(old); // the abandoned one ends late
        assert!(choosing(), "and must not free the new one");
        drop(new);
        assert!(!choosing());
    }
}
