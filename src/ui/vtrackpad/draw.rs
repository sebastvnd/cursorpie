//! The look and the input of the virtual trackpad's window: aluminum frame, glass surface, a
//! finger that follows the mouse, rings and a glow for every cue (see the module docs).

use super::{
    Cue, Look, Mover, Pointer, RIPPLE_LIFE, SURFACE_MM, Step, Tracker, WIDTHS, fade, move_pointer,
};
use crate::config::cfg;
use crate::platform::{self, HapticKind};
use crate::tap;
use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2,
};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Transparent margin around the trackpad: room for its shadow.
pub const PAD: f32 = 20.0;
/// The aluminum frame around the glass, at the top and at the sides.
pub const BEZEL: f32 = 16.0;
/// The frame below the glass: room for the size slider and the click switch.
pub const BEZEL_BOTTOM: f32 = 38.0;

/// Everything the window and the app share. The window runs as a deferred viewport, so the app
/// reaches it through one mutex.
#[derive(Debug)]
pub struct Shared {
    pub tracker: Tracker,
    pub look: Look,
    /// Touches are refused (Settings is open: the pie is off).
    pub blocked: bool,
    /// The pie of a gesture from here is open (for the hint text).
    pub pie_open: bool,
    /// Width of the surface, in points: what the window is.
    pub width: f32,
    /// The width the slider shows (it only becomes `width` when let go: the window resizes then).
    pub width_edit: f32,
    /// A click on the close button or a close request from the system: the app closes it.
    pub close_requested: bool,
    pub sound: bool,
    /// How far the finger moved, for the pointer.
    pub mover: Mover,
    /// When the window last drew, for the speed of the finger.
    pub last_frame: Option<Instant>,
    /// Where the virtual pointer is (the same as [`platform::pointer_pos`] says while it is open).
    pub pointer: Option<(f64, f64)>,
    /// All the displays together: where the virtual pointer may go.
    pub bounds: Option<Rect>,
    /// The mouse button went down on the glass and is still down (the click).
    pub clicking: bool,
}

impl Shared {
    pub fn new(width: f32, sound: bool) -> Self {
        Self {
            tracker: Tracker::default(),
            look: Look::default(),
            blocked: false,
            pie_open: false,
            width,
            width_edit: width,
            close_requested: false,
            sound,
            mover: Mover::default(),
            last_frame: None,
            pointer: None,
            bounds: None,
            clicking: false,
        }
    }

    /// A cue, from the app (a tick of the gesture) or from the window (the click).
    pub fn cue(&mut self, cue: Cue) {
        let at = self.tracker.finger().map(|f| (f.x, f.y));
        self.look.cue(cue, at, Instant::now());
        if self.sound && cue.sounds() {
            platform::tick();
        }
    }

    /// Puts the virtual pointer at `p` (or takes it away): the system is told, and the pointer's
    /// window follows.
    pub fn set_pointer(&mut self, ctx: &egui::Context, p: Option<(f64, f64)>) {
        self.pointer = p;
        platform::set_virtual_pointer(p);
        if let Some((x, y)) = p {
            ctx.send_viewport_cmd_to(
                super::pointer::id(),
                egui::ViewportCommand::OuterPosition(super::pointer::window_pos(x, y)),
            );
            ctx.request_repaint_of(super::pointer::id());
        }
    }
}

/// The size of the window for a surface `width` wide.
pub fn window_size(width: f32) -> Vec2 {
    Vec2::new(
        width + 2.0 * (PAD + BEZEL),
        width * super::ASPECT + 2.0 * PAD + BEZEL + BEZEL_BOTTOM,
    )
}

/// Where the parts are inside a window whose content is `rect`: the body and the glass.
pub fn layout(rect: Rect) -> (Rect, Rect) {
    let body = rect.shrink(PAD);
    let w = body.width() - 2.0 * BEZEL;
    let glass = Rect::from_min_size(
        body.min + Vec2::splat(BEZEL),
        Vec2::new(w, w * super::ASPECT),
    );
    (body, glass)
}

/// Draws the trackpad and handles its input. Runs once per frame of the window.
pub fn show(ui: &mut Ui, shared: &Arc<Mutex<Shared>>) {
    let ctx = ui.ctx().clone();
    let now = Instant::now();
    let (body, glass) = layout(ui.max_rect());
    let close = Rect::from_center_size(
        Pos2::new(body.min.x + BEZEL + 8.0, body.min.y + BEZEL * 0.5),
        Vec2::splat(14.0),
    );
    let close_hit = ui.interact(close, Id::new("vtrackpad-close"), Sense::click());

    // the pointer over the window is the finger (none when it is somewhere else)
    let hover = ctx.input(|i| i.pointer.hover_pos());
    let pointer = ctx.input(|i| Pointer {
        pos: i.pointer.hover_pos(),
        pressed: i.pointer.primary_pressed(),
        down: i.pointer.primary_down(),
    });

    let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
    let dt = sh
        .last_frame
        .map_or(0.016, |t| now.saturating_duration_since(t).as_secs_f32());
    sh.last_frame = Some(now);
    let blocked = sh.blocked;

    // the mouse is a finger on the glass
    match sh.tracker.step(pointer, glass, blocked) {
        Step::Idle => {}
        Step::Down(f) => {
            tap::virtual_feed(SURFACE_MM, &[f]);
        }
        Step::Hold(f) => {
            tap::virtual_feed(SURFACE_MM, &[f]);
            tap::virtual_poll();
        }
        Step::Up => {
            tap::virtual_feed(SURFACE_MM, &[]);
        }
    }
    let finger = sh.tracker.finger().map(|f| (f.x, f.y));

    // the finger moves the virtual pointer, relatively and with acceleration
    let moved = sh.mover.step(finger, dt.clamp(0.001, 0.05));
    if moved != (0.0, 0.0)
        && let (Some(p), Some(bounds)) = (sh.pointer, sh.bounds)
    {
        let to = move_pointer(p, moved, bounds);
        if to != p {
            sh.set_pointer(&ctx, Some(to));
        }
    }

    // the click of the mouse is the click of the trackpad: it sinks, it ticks
    if pointer.pressed && finger.is_some() {
        sh.clicking = true;
        sh.cue(Cue::Down);
        if cfg().haptics {
            platform::haptic(HapticKind::Generic);
        }
    }
    if sh.clicking && (!pointer.down || finger.is_none()) {
        sh.clicking = false;
        sh.cue(Cue::Up);
        if cfg().haptics {
            platform::haptic(HapticKind::Alignment);
        }
    }
    if close_hit.clicked() || ctx.input(|i| i.viewport().close_requested()) {
        sh.close_requested = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
    }

    let down = finger.is_some();
    let clicking = sh.clicking;
    let animating = sh.look.step(clicking, dt.min(0.05), now);

    let frame = Frame {
        body,
        glass,
        close,
        close_hot: close_hit.hovered(),
        finger,
        blocked,
        pie_open: sh.pie_open,
        glow: sh.look.glow(now),
        press: sh.look.press,
        accent: accent(),
    };
    let ripples = sh.look.ripples.clone();
    drop(sh);
    paint(ui.painter(), &frame, &ripples, now);

    // the controls come after the painting, on top of the frame
    let mut sh = shared.lock().unwrap_or_else(|e| e.into_inner());
    // the controls in the lower frame: the size of the trackpad and its click
    let bar = Rect::from_min_max(
        Pos2::new(glass.min.x, glass.max.y + 7.0),
        Pos2::new(glass.max.x, body.max.y - 7.0),
    );
    let mut bar_ui = ui.new_child(UiBuilder::new().max_rect(bar));
    bar_ui.visuals_mut().override_text_color = Some(Color32::from_rgb(86, 90, 104));
    let mut width = sh.width_edit;
    let mut sound = sh.sound;
    bar_ui.horizontal_centered(|ui| {
        ui.spacing_mut().slider_width = (bar.width() - 90.0).max(60.0);
        let r = ui.add(egui::Slider::new(&mut width, WIDTHS.0..=WIDTHS.1).show_value(false));
        ui.add_space(6.0);
        ui.checkbox(&mut sound, "Click");
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            sh.width = width;
        }
    });
    sh.width_edit = width;
    sh.sound = sound;
    drop(sh);

    if hover.is_some_and(|p| glass.contains(p)) {
        ctx.set_cursor_icon(CursorIcon::Default);
    }
    if down {
        // a resting finger sends no event: the hold time still has to be counted
        ctx.request_repaint_after(Duration::from_millis(8));
    } else if animating {
        ctx.request_repaint();
    }
}

fn accent() -> Color32 {
    let a = cfg().accent;
    Color32::from_rgb(a[0], a[1], a[2])
}

/// What [`paint`] draws.
pub struct Frame {
    pub body: Rect,
    pub glass: Rect,
    pub close: Rect,
    pub close_hot: bool,
    /// The finger, 0..1 on the glass.
    pub finger: Option<(f32, f32)>,
    pub blocked: bool,
    pub pie_open: bool,
    pub glow: f32,
    pub press: f32,
    pub accent: Color32,
}

fn at(glass: Rect, p: (f32, f32)) -> Pos2 {
    Pos2::new(
        glass.min.x + p.0 * glass.width(),
        glass.min.y + p.1 * glass.height(),
    )
}

fn alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Draws one frame of the trackpad.
pub fn paint(p: &egui::Painter, f: &Frame, ripples: &[super::Ripple], now: Instant) {
    let radius = f.body.width() * 0.055;
    // the shadow: a few rounded rects growing and fading, which is a blur for our purposes
    for i in 0..7 {
        let g = i as f32 * 2.2;
        p.rect_filled(
            f.body.expand(g).translate(Vec2::new(0.0, 5.0 + g * 0.3)),
            radius + g,
            Color32::from_black_alpha(9),
        );
    }
    // aluminum
    p.rect_filled(f.body, radius, Color32::from_rgb(222, 224, 230));
    p.rect_stroke(
        f.body,
        radius,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 220)),
        StrokeKind::Inside,
    );
    p.rect_stroke(
        f.body.expand(0.5),
        radius,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(90, 94, 108, 90)),
        StrokeKind::Outside,
    );
    // the glow of a cue runs around the frame
    if f.glow > 0.01 {
        p.rect_stroke(
            f.body.expand(1.5),
            radius,
            Stroke::new(2.5, alpha(f.accent, f.glow * 0.8)),
            StrokeKind::Outside,
        );
        p.rect_stroke(
            f.body.expand(4.0),
            radius + 2.5,
            Stroke::new(4.0, alpha(f.accent, f.glow * 0.25)),
            StrokeKind::Outside,
        );
    }

    // the glass, which sinks a little when it is pressed
    let sink = f.press * 2.0;
    let glass = f.glass.shrink(sink * 0.5);
    let gr = radius * 0.62;
    let lit = Color32::from_rgb(249, 250, 252);
    let pressed = Color32::from_rgb(231, 233, 239);
    p.rect_filled(glass, gr, lerp(lit, pressed, f.press));
    p.rect_stroke(
        glass,
        gr,
        Stroke::new(
            1.0 + f.press,
            Color32::from_rgba_unmultiplied(70, 74, 90, 45 + (f.press * 40.0) as u8),
        ),
        StrokeKind::Inside,
    );
    // a soft light from the top edge
    let sheen = Rect::from_min_size(
        glass.min + Vec2::splat(1.5),
        Vec2::new(glass.width() - 3.0, glass.height() * 0.35),
    );
    p.rect_filled(
        sheen,
        gr * 0.8,
        Color32::from_white_alpha(((1.0 - f.press) * 38.0) as u8),
    );

    // the close button, in the corner of the frame
    let dot = f.close.center();
    p.circle_filled(dot, 4.6, Color32::from_rgb(255, 95, 86));
    p.circle_stroke(dot, 4.6, Stroke::new(0.8, Color32::from_black_alpha(50)));
    if f.close_hot {
        let k = Stroke::new(1.2, Color32::from_black_alpha(150));
        p.line_segment([dot + Vec2::new(-1.8, -1.8), dot + Vec2::new(1.8, 1.8)], k);
        p.line_segment([dot + Vec2::new(-1.8, 1.8), dot + Vec2::new(1.8, -1.8)], k);
    }

    let tip = glass.width() * 0.05;
    // the rings of the cues
    for r in ripples {
        let age = now.saturating_duration_since(r.born);
        let t = (age.as_secs_f32() / RIPPLE_LIFE.as_secs_f32()).clamp(0.0, 1.0);
        let center = at(f.glass, r.at);
        let reach = glass.width() * (0.08 + 0.14 * r.strength);
        let a = fade(age, RIPPLE_LIFE) * (0.35 + 0.5 * r.strength);
        p.circle_stroke(
            center,
            tip + reach * (1.0 - (1.0 - t) * (1.0 - t)),
            Stroke::new(1.5 + 1.5 * r.strength * (1.0 - t), alpha(f.accent, a)),
        );
    }
    // the finger
    if let Some(fg) = f.finger {
        let c = at(f.glass, fg);
        let r = tip * (1.0 + 0.12 * f.press);
        p.circle_filled(
            c + Vec2::new(0.0, 2.0),
            r * 1.15,
            Color32::from_black_alpha(22),
        );
        p.circle_filled(c, r, alpha(f.accent, 0.30));
        p.circle_stroke(c, r, Stroke::new(1.8, alpha(f.accent, 0.9)));
        p.circle_filled(c, r * 0.28, alpha(f.accent, 0.95));
    }

    // a hint while nothing happens, and why nothing happens when it cannot
    let size = (f.glass.width() * 0.034).clamp(10.0, 14.0);
    let font = FontId::proportional(size);
    if f.blocked {
        p.rect_filled(
            glass,
            gr,
            Color32::from_rgba_unmultiplied(242, 243, 246, 225),
        );
        p.text(
            glass.center(),
            Align2::CENTER_CENTER,
            "The pie is off while Settings is open",
            font,
            Color32::from_rgb(92, 96, 110),
        );
    } else if !f.pie_open {
        let hint = if f.finger.is_none() {
            "Move your mouse in here: it is the finger"
        } else {
            "Hold still to open the pie"
        };
        p.text(
            Pos2::new(glass.center().x, glass.max.y - size * 1.6),
            Align2::CENTER_CENTER,
            hint,
            font,
            Color32::from_rgb(150, 154, 166),
        );
    }
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_the_surface_plus_its_frame_and_shadow_room() {
        let s = window_size(360.0);
        let (body, glass) = layout(Rect::from_min_size(Pos2::ZERO, s));
        assert!((glass.width() - 360.0).abs() < 1e-3);
        // the lower frame has room for the controls
        assert!((body.max.y - glass.max.y - BEZEL_BOTTOM).abs() < 1e-3);
        assert!((glass.height() / glass.width() - super::super::ASPECT).abs() < 1e-3);
        assert!(body.contains_rect(glass));
    }

    #[test]
    fn every_state_paints_without_panicking() {
        let ctx = egui::Context::default();
        let size = window_size(300.0);
        let (body, glass) = layout(Rect::from_min_size(Pos2::ZERO, size));
        let now = Instant::now();
        let mut look = Look::default();
        look.cue(Cue::Down, Some((0.5, 0.5)), now);
        for (finger, blocked) in [
            (None, false),
            (Some((0.3, 0.6)), false),
            (Some((0.5, 0.5)), true),
        ] {
            let frame = Frame {
                body,
                glass,
                close: Rect::from_center_size(body.min + Vec2::new(20.0, 8.0), Vec2::splat(14.0)),
                close_hot: blocked,
                finger,
                blocked,
                pie_open: false,
                glow: 0.7,
                press: 0.5,
                accent: Color32::from_rgb(64, 156, 255),
            };
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                paint(ui.painter(), &frame, &look.ripples, now);
            });
            out.textures_delta.clear();
        }
    }

    #[test]
    fn a_cue_from_the_app_lights_the_trackpad_at_the_finger() {
        let mut sh = Shared::new(300.0, false);
        sh.cue(Cue::Tick(HapticKind::LevelChange));
        assert_eq!(sh.look.ripples.len(), 1);
        assert_eq!(sh.look.ripples[0].at, (0.5, 0.5));
        assert!(sh.look.pulse.is_some());
    }
}
