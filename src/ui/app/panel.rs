//! A panel (see [`crate::panel`]) on screen: the window as a small interactive card.
//!
//! It is a fourth mode of the one window, next to the pie, Settings and the notice. Unlike the
//! notice it takes clicks and the keyboard, so it is not click-through and not passive. It follows
//! the same rules about sharing the window: the pie and Settings take it over at once (the panel that
//! was open is answered as cancelled), and a panel that is asked for while the pie is still on
//! screen waits in a short queue ([`App::step_panel`]), and is given up after
//! [`QUEUE_WAIT`].
//!
//! A **passive** panel ([`PanelSpec::passive`](crate::panel::PanelSpec::passive): the progress panel
//! of a run, see [`runpanel`](crate::runpanel)) is shown without taking the keyboard, so the keys that
//! the run sends still reach the app they are for, and it gives the window up to any other panel
//! that is asked for.
//!
//! The rules about answers live in [`Form`]; the drawing lives in
//! [`panel_form`]. This file is the window and the clock.

use super::App;
use super::windows::request_focus;
use crate::config::cfg;
use crate::panel::broker::QUEUE_WAIT;
use crate::panel::layout::card_size;
use crate::panel::{Form, PanelResult, PanelSpec, Patch, Place, Request, Source};
use crate::platform;
use crate::ui::draw::{ease_out_cubic, rgb};
use crate::ui::screen::screen_rect_at;
use crate::ui::settings::panel_form::{self, Event};
use eframe::egui::{self, Pos2, Rect, Vec2, pos2, vec2};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::WIN_MARGIN;

/// Panels that may wait for the window behind the one on screen.
const QUEUE_MAX: usize = 3;
/// Space kept between a card and the edge of the display.
const EDGE: f32 = 12.0;
/// Space between the pointer and the card that appears next to it.
const POINTER_GAP: f32 = 14.0;
/// Frames in which the window asks for the keyboard focus after it appeared.
const FOCUS_FRAMES: u8 = 6;
/// How long a panel that stays open after a button (`keep_open`) waits for the flow to ask again
/// before it goes away. The flow ending lets it go at once (`panel::release`); this is only the
/// net for a flow that does something slow, or that never asks again.
const LINGER: Duration = Duration::from_secs(6);
/// How long a panel takes to come in: it fades in while it rises into place. Short on purpose, so
/// the panel is there at once for the eye but does not jump at the person.
const OPEN_TIME: Duration = Duration::from_millis(170);
/// How far below its place the card starts, in points.
const OPEN_RISE: f32 = 14.0;

/// How far a panel that appeared `elapsed` ago has come in: 0 (not there yet) to 1 (in place), on a
/// curve that starts fast and settles softly.
pub(super) fn open_progress(elapsed: Duration) -> f32 {
    ease_out_cubic(elapsed.as_secs_f32() / OPEN_TIME.as_secs_f32())
}

/// The panel that is on screen.
pub struct ActivePanel {
    pub form: Form,
    /// Where the answer goes.
    pub reply: mpsc::Sender<PanelResult>,
    /// Who asked: only that source may update or close it.
    pub source: Source,
    /// When it gives up (none: waits for ever).
    pub deadline: Option<Instant>,
    /// The first box takes the keyboard once.
    pub focus_first: bool,
    /// The size of the card (the window is larger by `WIN_MARGIN` on every side).
    pub card: Vec2,
    /// Frames drawn so far, to repeat the focus request while the window is being mapped.
    pub frames: u8,
    /// Set when a button answered and the panel stayed on screen (`keep_open`): since when it has
    /// been waiting for the flow to ask again. Its answer is gone, so it ignores presses.
    pub linger: Option<Instant>,
    /// When the card was first drawn on a window that is on screen, for the opening animation.
    /// A panel that takes the window over from one that waited (`keep_open`) keeps its value, so
    /// nothing blinks and nothing plays again.
    pub shown: Option<Instant>,
}

/// The center of a card of size `card` on a display `area`, for a panel that wants to be at `place`
/// with the pointer at `at`: next to the pointer, in the middle, or at the top or bottom edge. Kept
/// inside the display. A display smaller than the card keeps its center.
pub(super) fn panel_center(place: Place, area: Rect, at: Pos2, card: Vec2) -> Pos2 {
    let raw = match place {
        Place::Cursor => at + vec2(POINTER_GAP, POINTER_GAP) + card / 2.0,
        Place::Center => area.center(),
        Place::Top => pos2(area.center().x, area.min.y + 64.0 + card.y / 2.0),
        Place::Bottom => pos2(area.center().x, area.max.y - 64.0 - card.y / 2.0),
    };
    let clamp = |v: f32, lo: f32, hi: f32| {
        if lo <= hi {
            v.clamp(lo, hi)
        } else {
            (lo + hi) / 2.0
        }
    };
    let half = card / 2.0;
    pos2(
        clamp(
            raw.x,
            area.min.x + half.x + EDGE,
            area.max.x - half.x - EDGE,
        ),
        clamp(
            raw.y,
            area.min.y + half.y + EDGE,
            area.max.y - half.y - EDGE,
        ),
    )
}

/// The card size for `spec` on a display `area`: the size it needs, never taller than the display
/// leaves room for (the body then scrolls).
pub(super) fn fit_card(spec: &PanelSpec, area: Vec2) -> Vec2 {
    let (w, h) = card_size(spec);
    let max_w = (area.x - 2.0 * WIN_MARGIN - 2.0 * EDGE).max(280.0);
    let max_h = (area.y - 2.0 * WIN_MARGIN - 60.0).max(240.0);
    vec2(w.min(max_w), h.min(max_h))
}

impl App {
    /// A panel was asked for by a flow or the browser extension.
    pub(super) fn panel_request(&mut self, ctx: &egui::Context, req: Request) {
        let id = req.spec.id.clone();
        if self.settings_open {
            let _ = req
                .reply
                .send(PanelResult::unavailable(&id, "Settings is open"));
            return;
        }
        // the panel on screen stayed open after a button (`keep_open`) and now the flow asks
        // again: the same window takes the new description over, so nothing blinks
        let lingering = self
            .panel
            .as_ref()
            .is_some_and(|p| p.linger.is_some() && p.source == req.source);
        if lingering {
            let same = self.panel.as_ref().is_some_and(|p| p.form.panel_id() == id);
            if same && !self.pie_open {
                self.reuse_panel(ctx, req);
            } else {
                // another panel: the one that waited for a follow-up is over
                self.end_panel(
                    ctx,
                    PanelResult::cancelled(&id, "replaced by a newer panel"),
                );
                self.panel_queue.push_front(req);
                ctx.request_repaint();
            }
            return;
        }
        // a panel that only shows progress does not hold the window against another panel: that
        // one (a question of the flow, say) would wait for it for as long as the run lasts
        let yields = self.panel.as_ref().is_some_and(|p| {
            p.form.spec.passive && p.source == req.source && p.form.panel_id() != id
        });
        if yields && !self.pie_open {
            let shown = self
                .panel
                .as_ref()
                .map(|p| p.form.panel_id().to_string())
                .unwrap_or_default();
            self.end_panel(
                ctx,
                PanelResult::cancelled(&shown, "another panel needs the window"),
            );
            self.panel_queue.push_front(req);
            ctx.request_repaint();
            return;
        }
        // a panel with the id of the one on screen replaces it
        let replaces = !id.is_empty()
            && self
                .panel
                .as_ref()
                .is_some_and(|p| p.form.panel_id() == id && p.source == req.source);
        if replaces {
            self.end_panel(
                ctx,
                PanelResult::cancelled(&id, "replaced by a newer panel"),
            );
            self.panel_queue.push_front(req);
            ctx.request_repaint();
            return;
        }
        // one that is waiting with the same id is replaced too
        if !id.is_empty()
            && let Some(at) = self
                .panel_queue
                .iter()
                .position(|q| q.spec.id == id && q.source == req.source)
            && let Some(old) = self.panel_queue.remove(at)
        {
            let _ = old
                .reply
                .send(PanelResult::cancelled(&id, "replaced by a newer panel"));
        }
        let free = !self.pie_open && (!self.visible || self.hud.is_some()) && self.panel.is_none();
        if free {
            if self.yield_hud(ctx) {
                // the notice is still on screen: it goes away with its animation first
                self.panel_queue.push_back(req);
                return;
            }
            self.begin_panel(ctx, req);
            return;
        }
        if self.panel_queue.len() >= QUEUE_MAX {
            let _ = req
                .reply
                .send(PanelResult::unavailable(&id, "Too many panels are waiting"));
            return;
        }
        self.panel_queue.push_back(req);
        ctx.request_repaint();
    }

    /// Changes the panel on screen (progress, new options), when `source` asked for it.
    pub(super) fn panel_patch(
        &mut self,
        ctx: &egui::Context,
        panel: &str,
        patch: &Patch,
        source: &Source,
    ) {
        if let Some(p) = self.panel.as_mut()
            && p.form.panel_id() == panel
            && p.source == *source
            && p.form.apply(patch)
        {
            ctx.request_repaint();
        }
    }

    /// Closes the panel named `panel` as cancelled, when `source` asked for it.
    pub(super) fn panel_close(&mut self, ctx: &egui::Context, panel: &str, source: &Source) {
        // no name: the flow is over, let go of a panel that stayed open waiting for it
        if panel.is_empty() {
            let waiting = self
                .panel
                .as_ref()
                .is_some_and(|p| p.linger.is_some() && p.source == *source);
            if waiting {
                self.end_panel(ctx, PanelResult::cancelled("", "the flow ended"));
            }
            return;
        }
        if self
            .panel
            .as_ref()
            .is_some_and(|p| p.form.panel_id() == panel && p.source == *source)
        {
            self.end_panel(ctx, PanelResult::cancelled(panel, "closed by the caller"));
            return;
        }
        if let Some(at) = self
            .panel_queue
            .iter()
            .position(|q| q.spec.id == panel && q.source == *source)
            && let Some(old) = self.panel_queue.remove(at)
        {
            let _ = old
                .reply
                .send(PanelResult::cancelled(panel, "closed by the caller"));
        }
    }

    /// Runs every frame: the clock of the panel on screen, panels that waited too long, and the
    /// start of the next one when the window is free.
    pub(super) fn step_panel(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(p) = &self.panel
            && let Some(deadline) = p.deadline
        {
            if now >= deadline {
                let id = p.form.panel_id().to_string();
                self.end_panel(ctx, PanelResult::timeout(&id));
            } else {
                ctx.request_repaint_after(deadline - now);
            }
        }
        let linger_left = self
            .panel
            .as_ref()
            .and_then(|p| p.linger)
            .map(|since| LINGER.saturating_sub(since.elapsed()));
        match linger_left {
            Some(left) if left.is_zero() => {
                self.end_panel(ctx, PanelResult::cancelled("", "nobody asked again"));
            }
            Some(left) => ctx.request_repaint_after(left),
            None => {}
        }
        self.panel_queue.retain(|q| {
            if q.created.elapsed() > QUEUE_WAIT {
                let _ = q.reply.send(PanelResult::unavailable(
                    &q.spec.id,
                    "The window was busy for too long",
                ));
                false
            } else {
                true
            }
        });
        if self.panel_queue.is_empty() {
            return;
        }
        let free = self.panel.is_none()
            && !self.pie_open
            && !self.settings_open
            && (!self.visible || self.hud.is_some());
        if free && self.yield_hud(ctx) {
            // the notice is on its way out and hands the window over when it is gone (`end_hud`)
            return;
        }
        if free && let Some(req) = self.panel_queue.pop_front() {
            self.begin_panel(ctx, req);
        } else {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    /// The pie or Settings wants the window: the panel on screen is answered as cancelled and goes
    /// away at once. With `all`, the ones that were waiting are answered too (Settings is going to
    /// stay open); otherwise they wait for the pie to close.
    pub(super) fn drop_panel(&mut self, ctx: &egui::Context, all: bool) {
        if all {
            for q in self.panel_queue.drain(..) {
                let _ = q.reply.send(PanelResult::cancelled(
                    &q.spec.id,
                    "another window took over",
                ));
            }
        }
        if let Some(p) = self.panel.take() {
            if p.form.spec.passive {
                platform::set_window_passive(false);
            }
            let id = p.form.panel_id().to_string();
            let _ = p
                .reply
                .send(PanelResult::cancelled(&id, "another window took over"));
            self.visible = false;
            self.show_wait = None;
            self.want_pos = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    /// Turns the (hidden) window into the panel and shows it.
    pub(super) fn begin_panel(&mut self, ctx: &egui::Context, req: Request) {
        let Request {
            spec,
            reply,
            source,
            ..
        } = req;
        let at = self.cursor_or_default();
        let area =
            screen_rect_at(at).unwrap_or_else(|| Rect::from_center_size(at, vec2(1600.0, 1000.0)));
        let card = fit_card(&spec, area.size());
        let win = card + Vec2::splat(2.0 * WIN_MARGIN);
        let center = panel_center(spec.place, area, at, card);
        let (wx, wy) = platform::window_point(f64::from(center.x), f64::from(center.y));
        let top_left = (Pos2::new(wx as f32, wy as f32) - win / 2.0).round();
        diag::log!(
            "ui",
            "panel_show",
            left = top_left.x,
            top = top_left.y,
            w = win.x,
            h = win.y
        );
        crate::debug::expect_window(top_left.x, top_left.y);

        let deadline = match spec.timeout_s {
            Some(0) => None,
            Some(s) => Some(Instant::now() + Duration::from_secs(u64::from(s))),
            None => None,
        };
        let focus_first = spec.widgets.iter().any(|w| {
            matches!(
                w,
                crate::panel::Widget::Input { .. }
                    | crate::panel::Widget::Choice {
                        search: Some(true),
                        ..
                    }
            )
        }) || spec.widgets.iter().any(|w| {
            matches!(w, crate::panel::Widget::Choice { options, search: None, .. }
                if options.len() > crate::panel::layout::SEARCH_FROM)
        });

        // it takes the keyboard (unlike the notice it is for typing), except a panel that only
        // shows progress
        platform::set_window_passive(spec.passive);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(win));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
        self.panel = Some(ActivePanel {
            form: Form::new(spec),
            reply,
            source,
            deadline,
            focus_first,
            card,
            frames: 0,
            linger: None,
            shown: None,
        });
        self.visible = true;
        self.show_window(ctx, win);
        ctx.request_repaint();
        if platform::window_manager_may_recenter() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(top_left));
            self.want_pos = Some((top_left, Instant::now()));
            ctx.request_repaint();
        }
    }

    /// The panel on screen was waiting for the flow to ask again (`keep_open`) and `req` is that
    /// next panel: the window stays where it is (the person may have moved it), only its size
    /// follows the new description, and nothing is hidden or shown.
    fn reuse_panel(&mut self, ctx: &egui::Context, req: Request) {
        let Request {
            spec,
            reply,
            source,
            ..
        } = req;
        let at = self.cursor_or_default();
        let area =
            screen_rect_at(at).unwrap_or_else(|| Rect::from_center_size(at, vec2(1600.0, 1000.0)));
        let card = fit_card(&spec, area.size());
        let deadline = match spec.timeout_s {
            Some(0) | None => None,
            Some(s) => Some(Instant::now() + Duration::from_secs(u64::from(s))),
        };
        let Some(p) = self.panel.as_mut() else {
            return;
        };
        if p.card != card {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                card + Vec2::splat(2.0 * WIN_MARGIN),
            ));
        }
        p.form = Form::new(spec);
        p.reply = reply;
        p.source = source;
        p.deadline = deadline;
        p.card = card;
        p.focus_first = false;
        p.linger = None;
        ctx.request_repaint();
    }

    /// A button answered a panel that wants to stay (`keep_open`): the answer goes to the flow, the
    /// panel stays on screen and waits for the flow to ask again (see [`LINGER`]).
    fn linger_panel(&mut self, ctx: &egui::Context, result: PanelResult) {
        if let Some(p) = self.panel.as_mut() {
            let _ = p.reply.send(result);
            p.linger = Some(Instant::now());
            p.deadline = None;
            p.form.notice = None;
        }
        ctx.request_repaint_after(LINGER);
    }

    /// The panel is over: it is answered, and the window is hidden again after one empty frame.
    fn end_panel(&mut self, ctx: &egui::Context, result: PanelResult) {
        if let Some(p) = self.panel.take() {
            if p.form.spec.passive {
                platform::set_window_passive(false);
            }
            let _ = p.reply.send(result);
        }
        // The window is not hidden here: it still holds the last frame of the panel, and a hidden
        // window keeps what it showed last. The next time it is mapped (the pie, a notice) that old
        // panel was on screen for a moment before the first new frame. Left `visible`, the next
        // frame draws nothing (see `pie_ui`) and only then hides the window, so what it keeps is empty.
        self.show_wait = None;
        self.want_pos = None;
        self.sleep_step(ctx);
        // that frame, and a panel that was waiting starts after it
        ctx.request_repaint();
    }

    /// Draws the panel and handles what the person did.
    pub fn panel_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let result = {
            let Some(p) = self.panel.as_mut() else {
                return;
            };
            // the window is being mapped: ask for the keyboard a few times, it may not be there yet
            if p.frames < FOCUS_FRAMES {
                p.frames += 1;
                // a panel that only shows progress leaves the keyboard where it is
                // (and stop once it has it: asking again and again while the window manager is
                // already giving it, or while the notice window is still being handed over, made the
                // focus jump back and forth)
                let focused = ctx.input(|i| i.viewport().focused).unwrap_or(false);
                if !p.form.spec.passive && !focused {
                    request_focus(&ctx);
                    if p.frames == 3 {
                        platform::raise_own_window();
                    }
                }
                ctx.request_repaint();
            }
            let accent = rgb(p.form.spec.accent_rgb().unwrap_or_else(|| cfg().accent));
            // the panel comes in: it fades in while it rises a little into its place
            let came = p.shown.get_or_insert_with(Instant::now).elapsed();
            let open = open_progress(came);
            if open < 1.0 {
                ui.multiply_opacity(open);
                ctx.request_repaint();
            }
            let rise = (1.0 - open) * OPEN_RISE;
            let card = Rect::from_min_size(pos2(WIN_MARGIN, WIN_MARGIN + rise), p.card);

            // the header is a handle to move the window by
            let handle = Rect::from_min_size(card.min, vec2(card.width(), 64.0));
            let drag = ui.interact(
                handle,
                egui::Id::new("panel_drag"),
                egui::Sense::click_and_drag(),
            );
            if drag.drag_started() {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            let event = panel_form::draw(ui, card, &mut p.form, accent, &mut p.focus_first);
            // already answered and waiting for the flow: only Esc or a button that cancels still count
            let event = match event {
                Event::Press(ref b)
                    if p.linger.is_some()
                        && !p.form.spec.find_button(b).is_some_and(|x| x.cancel) =>
                {
                    Event::None
                }
                Event::Pick(..) if p.linger.is_some() => Event::None,
                other => other,
            };
            match event {
                Event::None => None,
                Event::Cancel => {
                    let cancel = p.form.cancel_button().map(|b| b.id.clone());
                    let id = p.form.panel_id().to_string();
                    Some(match cancel {
                        Some(b) => p
                            .form
                            .submit(&b)
                            .unwrap_or_else(|_| PanelResult::cancelled(&id, "")),
                        None => PanelResult::cancelled(&id, ""),
                    })
                }
                Event::Press(button) => p.form.submit(&button).ok(),
                Event::Pick(widget, value) => p.form.pick(widget, &value),
            }
        };
        if let Some(r) = result {
            let keep = r.submitted()
                && self
                    .panel
                    .as_ref()
                    .is_some_and(|p| p.form.keeps_open(&r.button));
            if keep {
                self.linger_panel(&ctx, r);
            } else {
                self.end_panel(&ctx, r);
            }
        }
    }

    /// Cmd+W or the close button of the window: the panel is cancelled.
    pub(super) fn panel_close_requested(&mut self, ctx: &egui::Context) {
        if self.panel.is_some() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            let id = self
                .panel
                .as_ref()
                .map(|p| p.form.panel_id().to_string())
                .unwrap_or_default();
            self.end_panel(ctx, PanelResult::cancelled(&id, "the window was closed"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(1600.0, 1000.0))
    }

    #[test]
    fn a_panel_comes_in_quickly_and_ends_in_place() {
        assert_eq!(open_progress(Duration::ZERO), 0.0);
        let mut last = 0.0;
        for ms in (0..=OPEN_TIME.as_millis() as u64).step_by(10) {
            let v = open_progress(Duration::from_millis(ms));
            assert!(v >= last, "it never goes back");
            last = v;
        }
        assert_eq!(open_progress(OPEN_TIME), 1.0);
        assert_eq!(open_progress(Duration::from_secs(5)), 1.0);
        assert!(OPEN_TIME <= Duration::from_millis(250), "short, not a show");
    }

    #[test]
    fn a_panel_next_to_the_pointer_is_below_and_right_of_it() {
        let card = vec2(400.0, 300.0);
        let c = panel_center(Place::Cursor, area(), pos2(500.0, 400.0), card);
        let tl = c - card / 2.0;
        assert_eq!(tl, pos2(500.0 + POINTER_GAP, 400.0 + POINTER_GAP));
    }

    #[test]
    fn a_panel_never_leaves_the_display() {
        let card = vec2(400.0, 300.0);
        for at in [pos2(0.0, 0.0), pos2(1600.0, 1000.0), pos2(1590.0, 5.0)] {
            for place in [Place::Cursor, Place::Center, Place::Top, Place::Bottom] {
                let c = panel_center(place, area(), at, card);
                let r = Rect::from_center_size(c, card);
                assert!(area().contains_rect(r), "{place:?} at {at:?} gives {r:?}");
            }
        }
    }

    #[test]
    fn a_display_smaller_than_the_card_keeps_its_center() {
        let small = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0));
        let c = panel_center(Place::Cursor, small, pos2(10.0, 10.0), vec2(400.0, 300.0));
        assert_eq!(c, small.center());
    }

    #[test]
    fn the_card_is_cut_to_what_the_display_leaves_room_for() {
        let spec = PanelSpec::parse(r#"{"title":"x"}"#).unwrap();
        let tall = fit_card(&spec, vec2(1600.0, 1000.0));
        assert!(tall.y < 1000.0 - 2.0 * WIN_MARGIN);
        let many: Vec<String> = (0..24)
            .map(|i| format!(r#"{{"type":"input","id":"f{i}","label":"F"}}"#))
            .collect();
        let big = PanelSpec::parse(&format!(r#"{{"widgets":[{}]}}"#, many.join(","))).unwrap();
        let cut = fit_card(&big, vec2(1600.0, 700.0));
        assert!(
            cut.y <= 700.0 - 2.0 * WIN_MARGIN,
            "scrolls instead of overflowing"
        );
    }
}
