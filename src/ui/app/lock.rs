//! The lock animation of the trackpad gesture (see [`crate::ui::pie::lock`]): the window that shows
//! the pie shows a pie that turns into a padlock, or a padlock that opens into the pie, and when it
//! is over the pill says what happened ([`HudNote::locked`], [`HudNote::unlocked`]).
//!
//! The animation takes the window over exactly as a pie does ([`App::show_pie`] places it), but it is
//! click-through and never takes the pointer: the fingers are still on the trackpad.

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
        // The window is placed, sized and shown as for a pie (and takes over from a notice) ...
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

    /// Draws the animation (while no pie is open) and ends it: the window is hidden and the pill
    /// that follows is queued for it.
    pub fn lock_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(anim) = self.lock_anim else {
            return;
        };
        let t = anim.started.elapsed().as_secs_f32();
        if t >= LOCK_SECS {
            self.lock_anim = None;
            self.visible = false;
            self.forget_trail();
            self.hide_root(&ctx);
            if anim.started.elapsed() < Duration::from_secs_f32(LOCK_SECS) + STALE_AFTER {
                self.hud_pending = Some((note_for(anim.locked), Instant::now()));
            }
            ctx.request_repaint();
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
