//! The lock animation (see [`crate::ui::pie::lock`]): the pie that was open collapses, slices and
//! icons included, into a padlock, and when it is over the pill says what happened
//! ([`HudNote::locked`], [`HudNote::unlocked`]).
//!
//! An unlock that opens a pie at once (the trigger is still held) is not played here: the pie
//! blooms and the padlock opens in its middle ([`App::lock_reveal`], drawn by `pie_ui`). What is
//! left here is the plain stand-in for a window that has no pie to start from.
//!
//! The animation is click-through and never takes the pointer: the fingers are still on the trackpad.

use super::App;
use crate::geometry::{pie_inner, pie_outer};
use crate::ui::hud::HudNote;
use crate::ui::pie::PIE_SHIFT_SLACK;
use crate::ui::pie::lock::{LOCK_SECS, LockAnim, Look, paint, scene};
use eframe::egui;
use std::time::{Duration, Instant};

/// A pill this much later than the end of the animation is no longer news: the window was taken by
/// something else in between (Settings), and it is not shown.
const STALE_AFTER: Duration = Duration::from_millis(500);

fn note_for(locked: bool) -> HudNote {
    if locked {
        HudNote::locked()
    } else {
        HudNote::unlocked()
    }
}

impl App {
    /// CursorPie was locked or unlocked at (x, y): plays the animation there.
    pub(super) fn lock_anim_start(&mut self, ctx: &egui::Context, x: f64, y: f64, locked: bool) {
        if self.settings_open || self.panel.is_some() || !self.panel_queue.is_empty() {
            // the window is busy with something that must not be taken from it: the pill alone
            self.hud_note(ctx, note_for(locked));
            return;
        }
        self.preview_until = None;
        // Locking a pie that is on screen: that very pie collapses into the padlock. Its window
        // stays as it is (position, size, slices, icons), so nothing is swapped for a plain ring
        // half way and the change is one continuous movement. `HidePie` has already been handled
        // (the pie is no longer "open" and lets the pointer through); its bloom stays at full size
        // until the animation is over (see `pie_ui`).
        if locked && self.visible && self.anim > 0.15 && self.hud.is_none() {
            self.pie_open = false;
            self.hover = None;
            self.lock_over_pie = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
            self.lock_anim = Some(LockAnim::new(true));
            diag::log!("ui", "lock_anim_over_pie", x = x, y = y);
            ctx.request_repaint();
            return;
        }
        self.lock_over_pie = false;
        // Otherwise (an unlock that opened no pie, or a window that shows nothing): the plain
        // stand-in. The window is placed, sized and shown as for a pie (and takes over from a notice) ...
        self.show_pie(ctx, x, y, self.profile, Vec::new());
        // ... but nothing of the pie is open, and the window lets everything through.
        self.pie_open = false;
        self.anim = 0.0;
        self.hover = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        self.lock_anim = Some(LockAnim::new(locked));
        diag::log!("ui", "lock_anim", locked = locked, x = x, y = y);
        ctx.request_repaint();
    }

    /// The animation is over: the window is hidden and the pill that follows is queued for it.
    pub(crate) fn lock_finish(&mut self, ctx: &egui::Context, anim: LockAnim) {
        self.lock_anim = None;
        self.lock_over_pie = false;
        self.lock_reveal = None;
        self.anim = 0.0;
        self.forget_trail();
        // the pill of the slice names went away with the pie that collapsed: a stale one would be
        // drawn, or found over and skipped for a frame, by the next pie
        self.slice_pill = None;
        if anim.started.elapsed() < Duration::from_secs_f32(LOCK_SECS) + STALE_AFTER {
            self.hud_pending = Some((note_for(anim.locked), Instant::now()));
        }
        // macOS: the pill that says what happened takes the mapped window (the animation has faded
        // to nothing, so the window is empty); it is not hidden and mapped again for it
        if self.hand_to_pending_hud(ctx) {
            ctx.request_repaint();
            return;
        }
        self.visible = false;
        self.hide_root(ctx);
        ctx.request_repaint();
    }

    /// Draws the animation (while no pie is open) and ends it.
    pub fn lock_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(anim) = self.lock_anim else {
            return;
        };
        if self.lock_over_pie {
            // the pie that was open is the picture (see `lock_anim_start`)
            self.pie_ui(ui);
            return;
        }
        let t = anim.started.elapsed().as_secs_f32();
        if t >= LOCK_SECS {
            self.lock_finish(&ctx, anim);
            return;
        }
        let sc = scene(t, anim.locked);
        let look = Look::current();
        let (win, origin, off) = (self.win, self.pie_origin, self.pie_off);
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
                let painter = ui.painter_at(rect);
                // the center of the ring is where the pie was drawn (see `pie_ui`)
                let outer_min = ctx.input(|i| i.viewport().outer_rect).map(|r| r.min);
                let c = rect.center()
                    + crate::ui::screen::pie_center_shift(
                        origin,
                        outer_min,
                        rect.width(),
                        PIE_SHIFT_SLACK,
                    )
                    + off;
                paint(&painter, c, pie_outer(win), pie_inner(win), sc, &look);
            });
        ctx.request_repaint();
    }
}
