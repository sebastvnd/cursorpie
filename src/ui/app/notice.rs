//! The notice of what CursorPie is doing: the window shown as a small pill (see [`crate::ui::hud`]).
//!
//! The notice is a third mode of the one window, next to the pie and Settings. It is click-through
//! and always on top like the pie, small, and placed near the bottom edge of the display the pointer
//! is on. The pie and Settings take the window over at once ([`App::drop_hud`]); a notice that
//! arrives while the pie is still fading waits for the window ([`App::step_hud`]).

use super::App;
use crate::config::{HudPosition, cfg};
use crate::platform;
use crate::ui::hud::{self, CURSOR_GAP, EDGE_GAP, Hud, HudNote, PENDING_MAX, window_size};
use crate::ui::screen::screen_rect_at;
use eframe::egui::{self, Pos2, Rect};
use std::time::{Duration, Instant};

/// Where the center of the pill goes on a display `area`, with the pointer at `at`: the center of
/// the bottom or top edge (`EDGE_GAP` in), or just below the pointer. Kept inside the display, so the
/// whole window is on it. The gaps and the window grow with `scale` (1.0 = normal size).
pub(super) fn pill_center(position: HudPosition, area: Rect, at: Pos2, scale: f32) -> Pos2 {
    let raw = match position {
        HudPosition::Bottom => Pos2::new(area.center().x, area.max.y - EDGE_GAP * scale),
        HudPosition::Top => Pos2::new(area.center().x, area.min.y + EDGE_GAP * scale),
        HudPosition::Cursor => Pos2::new(at.x, at.y + CURSOR_GAP * scale),
    };
    let half = window_size(scale) / 2.0;
    // a display smaller than the window keeps its center
    let clamp = |v: f32, lo: f32, hi: f32| {
        if lo <= hi {
            v.clamp(lo, hi)
        } else {
            (lo + hi) / 2.0
        }
    };
    Pos2::new(
        clamp(raw.x, area.min.x + half.x, area.max.x - half.x),
        clamp(raw.y, area.min.y + half.y, area.max.y - half.y),
    )
}

impl App {
    /// A notice arrived from the actions worker.
    pub(super) fn hud_note(&mut self, ctx: &egui::Context, note: HudNote) {
        // a panel that waits for the window (see `yield_hud`) gets it without a new pill in between
        if !cfg().hud || self.settings_open || self.panel.is_some() || !self.panel_queue.is_empty()
        {
            return;
        }
        if let Some(h) = self.hud.as_mut() {
            if h.is_leaving() {
                return; // on its way out, for a panel
            }
            // already on screen: the pill changes its text and stays a little longer
            h.update(note, Instant::now());
            ctx.request_repaint();
            return;
        }
        if self.pie_open || self.visible {
            // the pie is still on screen (or fading): wait until the window is free
            self.hud_pending = Some((note, Instant::now()));
            ctx.request_repaint();
            return;
        }
        self.begin_hud(ctx, note);
    }

    /// The run that the notices announced is over: the pill that stays for it fills its bar and
    /// goes away after its usual time. A notice that still waits for the window is shown as an
    /// ordinary one, for its usual time.
    pub(super) fn hud_end(&mut self, ctx: &egui::Context) {
        if let Some((note, _)) = self.hud_pending.as_mut() {
            note.live = false;
        }
        if let Some(h) = self.hud.as_mut() {
            h.release(Instant::now());
            ctx.request_repaint();
        }
    }

    /// Runs every frame: shows a notice that was waiting for the window.
    pub(super) fn step_hud(&mut self, ctx: &egui::Context) {
        let Some((note, since)) = self.hud_pending.take() else {
            return;
        };
        if self.settings_open
            || since.elapsed() > PENDING_MAX
            || !cfg().hud
            || !self.panel_queue.is_empty()
        {
            return; // too late to be of any use (or a panel is next)
        }
        if self.pie_open || self.visible {
            self.hud_pending = Some((note, since));
            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }
        self.begin_hud(ctx, note);
    }

    /// Turns the (hidden) window into the notice and shows it.
    fn begin_hud(&mut self, ctx: &egui::Context, note: HudNote) {
        let (position, hold, scale) = {
            let c = cfg();
            (
                c.hud_position,
                Duration::from_millis(c.hud_ms),
                hud::scale_of(&c),
            )
        };
        let win = window_size(scale);
        let at = self.cursor_or_default();
        let area = screen_rect_at(at)
            .unwrap_or_else(|| Rect::from_center_size(at, egui::vec2(1600.0, 1000.0)));
        let center = pill_center(position, area, at, scale);
        let (wx, wy) = platform::window_point(f64::from(center.x), f64::from(center.y));
        let top_left = (Pos2::new(wx as f32, wy as f32) - win / 2.0).round();
        diag::log!(
            "ui",
            "hud_show",
            left = top_left.x,
            top = top_left.y,
            ppp = ctx.pixels_per_point()
        );
        crate::debug::expect_window(top_left.x, top_left.y);

        // it must not take the keyboard from the app the action is for
        platform::set_window_passive(true);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(win));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
        self.hud = Some(Hud::new(note, hold).with_scale(scale));
        self.visible = true;
        self.show_window(ctx, win);
        ctx.request_repaint();
        if platform::window_manager_may_recenter() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
            self.want_pos = Some((top_left, Instant::now()));
            ctx.request_repaint();
        }
    }

    /// A panel wants the window the notice is on. The notice does not vanish at once (that looked
    /// like something went wrong): it finishes coming in and goes away with its usual animation, and
    /// the panel starts when the window is free again (see [`App::step_panel`]). Returns whether a
    /// notice is still on screen, in which case the panel has to wait for it.
    pub(super) fn yield_hud(&mut self, ctx: &egui::Context) -> bool {
        let Some(h) = self.hud.as_mut() else {
            return false;
        };
        if !h.is_leaving() {
            h.dismiss(Instant::now());
            self.hud_pending = None;
            ctx.request_repaint();
        }
        // (a pill that is leaving draws its own frames; nothing to ask for here)
        true
    }

    /// The pie or Settings wants the window: the notice (and one that was waiting) goes away at once.
    pub(super) fn drop_hud(&mut self, ctx: &egui::Context) {
        self.hud_pending = None;
        if self.hud.take().is_some() {
            platform::set_window_passive(false);
            self.visible = false;
            self.show_wait = None;
            self.want_pos = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    /// The notice is over: the window is hidden again, or, when a panel waits for it, goes straight
    /// to the panel.
    ///
    /// The pill has faded out completely by now, so the window is empty. Hiding it and mapping it
    /// again for the panel left the screen bare for a moment (the window has to be mapped, resized
    /// and drawn into first), and the keyboard focus was given away and asked for again. Handing
    /// the mapped window over keeps the focus where it is and the panel's own fade-in does the rest.
    fn end_hud(&mut self, ctx: &egui::Context) {
        self.hud = None;
        self.want_pos = None;
        if self.panel.is_none()
            && !self.pie_open
            && !self.settings_open
            && let Some(req) = self.panel_queue.pop_front()
        {
            self.begin_panel(ctx, req);
            return;
        }
        platform::set_window_passive(false);
        self.visible = false;
        self.hide_root(ctx);
        if !self.panel_queue.is_empty() {
            // a panel that could not start now (the pie is open): it starts when the window is free
            ctx.request_repaint();
        }
        self.sleep_step(ctx);
    }

    /// Draws the notice and advances its animation.
    pub fn hud_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        let Some(h) = self.hud.as_mut() else {
            return;
        };
        let phase = h.phase(now);
        if phase.over {
            self.end_hud(&ctx);
            return;
        }
        // a note that came too soon after the last one takes its turn now
        h.flush_queued(now);
        let pulse = h.pulse(now);
        let scale = h.scale;
        let note = h.note.clone();
        let (prev, swap) = h.previous(now);
        let prev = prev.cloned();
        let elapsed = h.elapsed(now);
        let sweep = h.sweep(now);
        let bar = h.bar_fraction(now);

        // the notice follows the look of the pie, and the accent the person chose
        let (accent, smoked) = {
            let c = cfg();
            (hud::accent_of(&c), hud::smoked(&c))
        };
        let painter = ui.painter();
        // the pill glides to the size its note asks for instead of jumping to it
        let size = h.morph_size(hud::pill_size(painter, &note, scale), now);

        let wake = h.next_wake(now);
        let moving = h.progress_animates(now) || h.gliding(now);
        if phase.animating() || pulse > 0.0 || moving {
            ctx.request_repaint();
        } else if let Some(wait) = wake {
            // nothing moves until it is time to leave (or until the note changes)
            ctx.request_repaint_after(wait);
        }

        let win = hud::window_size(scale);
        let center = Pos2::new(win.x / 2.0, win.y / 2.0);
        let look = hud::PillLook {
            accent,
            smoked,
            alpha: phase.alpha(),
            lift: phase.offset(),
            pulse,
            pop: phase.pop(),
            grow: phase.grow(),
            text: phase.text(),
            scale,
            elapsed,
            size: Some(size),
            swap,
            sweep,
            bar,
        };
        hud::paint_pill_swap(painter, center, &note, prev.as_ref(), &look);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(1920.0, 1080.0))
    }

    #[test]
    fn the_pill_sits_above_the_bottom_edge_or_below_the_top_one() {
        let c = pill_center(HudPosition::Bottom, area(), Pos2::new(10.0, 10.0), 1.0);
        assert_eq!(c, Pos2::new(960.0, 1080.0 - EDGE_GAP));
        let c = pill_center(HudPosition::Top, area(), Pos2::new(10.0, 10.0), 1.0);
        assert_eq!(c, Pos2::new(960.0, EDGE_GAP));
    }

    #[test]
    fn near_the_cursor_it_stays_on_the_display() {
        let c = pill_center(HudPosition::Cursor, area(), Pos2::new(500.0, 400.0), 1.0);
        assert_eq!(c, Pos2::new(500.0, 400.0 + CURSOR_GAP));
        // in a corner the whole window still fits on the display
        let c = pill_center(HudPosition::Cursor, area(), Pos2::new(5.0, 1075.0), 1.0);
        let win = Rect::from_center_size(c, hud::WINDOW_SIZE);
        assert!(area().contains_rect(win), "{win:?}");
    }

    #[test]
    fn a_bigger_notice_keeps_its_distance_and_stays_on_the_display() {
        let c = pill_center(HudPosition::Bottom, area(), Pos2::new(10.0, 10.0), 1.5);
        assert_eq!(c, Pos2::new(960.0, 1080.0 - EDGE_GAP * 1.5));
        for position in [HudPosition::Bottom, HudPosition::Top, HudPosition::Cursor] {
            for at in [Pos2::new(5.0, 5.0), Pos2::new(1915.0, 1075.0)] {
                let c = pill_center(position, area(), at, 1.6);
                let win = Rect::from_center_size(c, hud::window_size(1.6));
                assert!(area().contains_rect(win), "{position:?} {win:?}");
            }
        }
    }

    #[test]
    fn a_display_smaller_than_the_window_does_not_panic() {
        let tiny = Rect::from_min_size(Pos2::new(100.0, 100.0), egui::vec2(300.0, 100.0));
        let c = pill_center(HudPosition::Bottom, tiny, Pos2::new(150.0, 150.0), 1.0);
        assert_eq!(c, tiny.center());
    }

    #[test]
    fn a_second_display_keeps_its_own_origin() {
        let right = Rect::from_min_size(Pos2::new(1920.0, 0.0), egui::vec2(2560.0, 1440.0));
        let c = pill_center(HudPosition::Bottom, right, Pos2::new(2000.0, 100.0), 1.0);
        assert_eq!(c.x, 1920.0 + 1280.0);
    }
}
