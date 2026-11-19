//! Small looping animations that show what a setting does, so the page does not have to explain
//! it in words. Each one sits at the top of the panel it belongs to and reads the settings being
//! edited, so moving a slider or picking another choice changes the animation at once.
//!
//! | Demo | Setting | Shows |
//! |---|---|---|
//! | [`hold_demo`] | hold times, "open the pie when I drag" | the button going down, the pie opening, the "hold for settings" arc |
//! | [`gesture_demo`] | [`AppGesture`] | which pie opens on a hold and on a double-click |
//! | [`target_demo`] | [`TargetApp`], "bring that app to the front" | which window the pie works on |
//! | [`open_choice_demo`] | "Choose between a new window and the open app" | the two badges coming out of a slice, one of them picked, and what it does to the app |
//! | [`desktop_demo`] | [`DesktopMode`] | what "Open app" does to an app on another desktop |
//! | [`look_demo`] | pie size, names, app icon, accent, material | the pie as it will look, the pointer passing over its slices |
//! | [`trail_demo`] | the submenu trail (web mode): distance, node size, spacing, thread, glide speed | a pie opening a submenu, the pie it left shrinking to a node, the nodes lining up on an arc, the pointer returning to one |
//! | [`app_icon_demo`] | [`AppIconStyle`], "Show the app's icon in the center" | the app's icon flying into the middle of the pie, staying in color or turning into a one-color silhouette |
//! | [`side_demo`] | side buttons: which pie they follow, ring color, right-click | the side button firing the pie it follows, and right-click switching it |
//! | [`memory_demo`] | "Free memory while idle" | the idle timer running out, the memory dropped, the trigger loading it again |
//! | [`scan_demo`] | the mouse scan | each extra button pressed once and heard, then Done |
//!
//! Every demo is split in two. A pure function turns the seconds of the loop and the settings into
//! a *frame* (numbers between 0 and 1: how far the pie is open, where the pointer is), and a
//! drawing function paints that frame. The frames are what the tests check; the painting only has
//! to not panic. Nothing is drawn off screen and a demo asks for the next frame only while it is
//! visible, so a closed Settings window costs nothing.

use super::theme::*;
use crate::config::{AppGesture, AppIconStyle, DesktopMode, SideTarget, TargetApp};
use crate::geometry::{Outline, TrailNode, TrailShape, trail_layout};
use crate::ui::draw::{
    Bend, ease_out_back, ease_out_cubic, mix, on_accent, pie_colors, rgb, ring_slice,
};
use crate::ui::fonts;
use crate::ui::pie::{Material, genz, glass};
use crate::ui::symbols;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2,
};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Duration;

/// Height of the stage every demo is drawn on.
const STAGE_H: f32 = 190.0;
/// Time between two frames of a demo that is on screen (30 per second).
const FRAME: Duration = Duration::from_millis(33);
/// Where the center of a pie is, measured from the top of the stage.
const PIE_Y: f32 = 88.0;
/// Radius of the small pies.
const PIE_R: f32 = 50.0;
// The pie, the name under it and the caption must all fit on the stage.
const _: () = assert!(PIE_Y + PIE_R + 20.0 < STAGE_H - 24.0);
const _: () = assert!(PIE_Y - PIE_R > 0.0);
/// Seconds the pie takes to open and to close.
const OPEN_SECS: f32 = 0.2;
const CLOSE_SECS: f32 = 0.3;

/// How far `t` is between `from` and `from + secs`, from 0 to 1.
fn ramp(t: f32, from: f32, secs: f32) -> f32 {
    ((t - from) / secs.max(1e-3)).clamp(0.0, 1.0)
}

/// `word` with a capital first letter.
fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

// ------------------------------------------------------------------ the hold

/// The seconds the mouse rests before the button goes down.
const HOLD_REST: f32 = 0.5;
/// The seconds the finished arc stays before the button is let go.
const HOLD_LINGER: f32 = 0.6;
/// The seconds the pie takes to fade away after the button is let go.
const HOLD_FADE: f32 = 0.4;
/// The seconds a drag takes, and the moment in it where the pie opens.
const FLICK_SECS: f32 = 0.25;
const FLICK_OPENS: f32 = 0.15;

/// One moment of the hold demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct HoldFrame {
    /// The trigger button is down.
    pub button: bool,
    /// How far the pointer was dragged (0 to 1); only with "open the pie when I drag".
    pub drag: f32,
    /// How far the pie is open.
    pub pie: f32,
    /// How far the "hold for settings" arc has run.
    pub settings: f32,
    /// How long the button has been down, in milliseconds.
    pub held_ms: f32,
}

/// The time the "hold for settings" timeline spans, in milliseconds. Settings never opens before
/// the pie, so a setting below the pie's own is stretched a little.
pub(super) fn hold_span_ms(pie_ms: u64, settings_ms: u64) -> f32 {
    settings_ms.max(pie_ms + 100) as f32
}

/// The hold demo at `t` seconds.
pub(super) fn hold_frame(t: f32, pie_ms: u64, settings_ms: u64, flick: bool) -> HoldFrame {
    let pie_s = pie_ms as f32 / 1000.0;
    let span_s = hold_span_ms(pie_ms, settings_ms) / 1000.0;
    let hold_s = span_s + HOLD_LINGER;
    let t = t.rem_euclid(HOLD_REST + hold_s + HOLD_FADE);
    if t < HOLD_REST {
        return HoldFrame::default();
    }
    let u = t - HOLD_REST;
    let opens = if flick { FLICK_OPENS } else { pie_s };
    let drag = if flick { ramp(u, 0.0, FLICK_SECS) } else { 0.0 };
    let settings = ramp(u, opens, span_s - pie_s);
    if u >= hold_s {
        // let go: the pie closes
        return HoldFrame {
            button: false,
            drag,
            pie: 1.0 - ramp(u, hold_s, HOLD_FADE),
            settings,
            held_ms: hold_s * 1000.0,
        };
    }
    HoldFrame {
        button: true,
        drag,
        pie: ramp(u, opens, OPEN_SECS),
        settings,
        held_ms: u * 1000.0,
    }
}

/// Hold the trigger: the pie opens after `pie_ms`, Settings after `settings_ms`. With `flick` the
/// pie opens as soon as the pointer is dragged.
pub(in crate::ui::settings) fn hold_demo(
    ui: &mut egui::Ui,
    accent: Color32,
    pie_ms: u64,
    settings_ms: u64,
    flick: bool,
) {
    let (p, rect, t) = stage(ui);
    let f = hold_frame(t, pie_ms, settings_ms, flick);
    mouse(
        &p,
        Pos2::new(rect.min.x + 56.0, rect.min.y + PIE_Y),
        f.button,
        accent,
    );
    let c = Pos2::new(rect.min.x + rect.width() * 0.42, rect.min.y + PIE_Y);
    mini_pie(&p, c, PIE_R, f.pie, accent, false, f.settings);
    cursor(&p, c + Vec2::new(f.drag * PIE_R * 0.8, 0.0));

    // the timeline: how long the button has been down, with the two moments that matter
    let span = hold_span_ms(pie_ms, settings_ms);
    let (x0, x1) = (rect.min.x + rect.width() * 0.62, rect.max.x - 40.0);
    let y = rect.min.y + PIE_Y + 8.0;
    let bar = Rect::from_min_max(Pos2::new(x0, y - 4.0), Pos2::new(x1, y + 4.0));
    p.rect_filled(bar, 4.0, pal().track);
    let done = (f.held_ms / span).clamp(0.0, 1.0);
    if done > 0.0 {
        let filled = Rect::from_min_max(bar.min, Pos2::new(x0 + (x1 - x0) * done, bar.max.y));
        p.rect_filled(filled, 4.0, accent);
    }
    let tick = x0 + (x1 - x0) * (pie_ms as f32 / span);
    p.line_segment(
        [Pos2::new(tick, y - 10.0), Pos2::new(tick, y + 10.0)],
        Stroke::new(1.5, pal().text),
    );
    let label = |x: f32, text: &str, on: bool| {
        let color = if on { accent } else { pal().muted };
        p.text(
            Pos2::new(x, y - 14.0),
            Align2::CENTER_BOTTOM,
            text,
            FontId::proportional(11.0),
            color,
        );
    };
    label(tick, "Pie", f.pie > 0.5);
    label(x1, "Settings", f.held_ms >= span);
    p.text(
        Pos2::new(x0, y + 14.0),
        Align2::LEFT_TOP,
        format!("{:.1} s", f.held_ms / 1000.0),
        FontId::proportional(11.0),
        pal().muted,
    );
    ui.add_space(GAP);
}

// --------------------------------------------------------------- the gesture

/// The seconds a gesture loop lasts: a hold in an app that has its own pie, and the two scenes
/// (hold, then double-click) of the new gesture.
const MERGED_LOOP: f32 = 3.0;
const DOUBLE_LOOP: f32 = 6.6;
/// Where the second scene of the double-click loop starts, and when its first click lands.
const SCENE_TWO: f32 = 3.0;
const FIRST_CLICK: f32 = 3.3;

/// One moment of the gesture demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct GestureFrame {
    /// The trigger button is down.
    pub button: bool,
    /// Presses so far in this scene that belong to a double-click (0 to 2).
    pub taps: u8,
    /// How far the pie is open.
    pub pie: f32,
    /// The pie that opens is the app's own, not the general one.
    pub app_pie: bool,
}

/// A press at `down`, held until `up`: the pie opens half a second into it.
fn press(t: f32, down: f32, up: f32, app_pie: bool) -> GestureFrame {
    let open = down + 0.5;
    GestureFrame {
        button: t >= down && t < up,
        taps: 0,
        pie: if t < up {
            ramp(t, open, OPEN_SECS)
        } else {
            1.0 - ramp(t, up, CLOSE_SECS)
        },
        app_pie,
    }
}

/// The gesture demo at `t` seconds.
pub(super) fn gesture_frame(t: f32, mode: AppGesture, double_click_ms: u64) -> GestureFrame {
    match mode {
        AppGesture::Merged => press(t.rem_euclid(MERGED_LOOP), 0.4, 2.3, true),
        AppGesture::DoubleClick => {
            let t = t.rem_euclid(DOUBLE_LOOP);
            if t < SCENE_TWO {
                return press(t, 0.4, 2.3, false);
            }
            // click, then press again within the double-click time and hold
            let gap = double_click_ms as f32 / 1000.0;
            let down = FIRST_CLICK + 0.1 + gap * 0.5;
            let mut f = press(t, down, down + 1.5, true);
            f.button |= (FIRST_CLICK..FIRST_CLICK + 0.1).contains(&t);
            f.taps = if t >= down {
                2
            } else if t >= FIRST_CLICK {
                1
            } else {
                0
            };
            f
        }
    }
}

/// What the gesture demo says under the pie.
fn gesture_caption(mode: AppGesture, f: &GestureFrame) -> &'static str {
    match (mode, f.app_pie) {
        (AppGesture::Merged, _) => "Hold: the app's own pie",
        (AppGesture::DoubleClick, false) => "Hold: the general pie",
        (AppGesture::DoubleClick, true) => "Double-click, then hold: the app's own pie",
    }
}

/// Which pie opens, for the chosen gesture.
pub(in crate::ui::settings) fn gesture_demo(
    ui: &mut egui::Ui,
    accent: Color32,
    mode: AppGesture,
    double_click_ms: u64,
) {
    let (p, rect, t) = stage(ui);
    let f = gesture_frame(t, mode, double_click_ms);
    let mouse_at = Pos2::new(rect.min.x + rect.width() * 0.3, rect.min.y + PIE_Y);
    mouse(&p, mouse_at, f.button, accent);
    for i in 0..f.taps {
        let dot = mouse_at + Vec2::new(34.0 + 14.0 * i as f32, -30.0);
        p.circle_filled(dot, 3.5, accent);
    }
    let c = Pos2::new(rect.min.x + rect.width() * 0.62, rect.min.y + PIE_Y);
    mini_pie(&p, c, PIE_R, f.pie, accent, f.app_pie, 0.0);
    cursor(&p, c);
    caption(&p, rect, gesture_caption(mode, &f));
    ui.add_space(GAP);
}

// ---------------------------------------------------------------- the target

/// The seconds the target loop lasts.
const TARGET_LOOP: f32 = 5.0;

/// One moment of the target demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct TargetFrame {
    /// Where the pointer is: 0 over the window in front, 1 over the other window.
    pub cursor: f32,
    /// How far the pie is open.
    pub pie: f32,
    /// The pie works on the window under the pointer (otherwise on the one in front).
    pub target_right: bool,
    /// How far the window under the pointer has been brought to the front.
    pub front_right: f32,
}

/// The target demo at `t` seconds. `bring` is "bring that app to the front first".
pub(super) fn target_frame(t: f32, mode: TargetApp, bring: bool) -> TargetFrame {
    let t = t.rem_euclid(TARGET_LOOP);
    let under = mode == TargetApp::UnderCursor;
    TargetFrame {
        cursor: ease_out_cubic(ramp(t, 0.4, 0.8)),
        pie: if t < 3.2 {
            ramp(t, 1.5, OPEN_SECS)
        } else {
            1.0 - ramp(t, 3.2, 0.25)
        },
        target_right: under,
        front_right: if under && bring {
            ramp(t, 3.4, 0.3) - ramp(t, 4.6, 0.3)
        } else {
            0.0
        },
    }
}

/// Two windows: the app in front (left) and an app the pointer moves over (right).
pub(in crate::ui::settings) fn target_demo(
    ui: &mut egui::Ui,
    accent: Color32,
    mode: TargetApp,
    bring: bool,
) {
    let (p, rect, t) = stage(ui);
    let f = target_frame(t, mode, bring);
    let top = rect.min.y + 14.0;
    let h = PIE_Y + 40.0;
    let w = rect.width() * 0.38;
    let left = Rect::from_min_size(
        Pos2::new(rect.min.x + rect.width() * 0.07, top),
        Vec2::new(w, h),
    );
    let right = Rect::from_min_size(
        Pos2::new(rect.max.x - rect.width() * 0.07 - w, top),
        Vec2::new(w, h),
    );
    mini_window(&p, left, "Notes", 1.0 - f.front_right, accent);
    mini_window(&p, right, "Browser", f.front_right, accent);
    let target = if f.target_right { right } else { left };
    p.rect_stroke(
        target.expand(3.0),
        11.0,
        Stroke::new(2.0, accent.gamma_multiply(f.pie)),
        StrokeKind::Outside,
    );
    let from = left.center();
    let to = right.center();
    let at = from + (to - from) * f.cursor;
    mini_pie(&p, at, PIE_R * 0.8, f.pie, accent, false, 0.0);
    cursor(&p, at);
    if f.pie > 0.5 {
        let name = if f.target_right { "Browser" } else { "Notes" };
        caption(&p, rect, &format!("The pie works on {name}"));
    }
    ui.add_space(GAP);
}

// ------------------------------------------------------------ the open-app choice

/// The seconds one half of the choice loop lasts: the first half picks the new window, the second
/// half picks the app that is open.
const CHOICE_HALF: f32 = 4.5;
/// Radius of the pie in the choice demo, and the size and gaps of its badges.
const CHOICE_R: f32 = PIE_R * 0.9;
const CHOICE_BADGE: f32 = 18.0;
const CHOICE_BADGE_GAP: f32 = 8.0;
const CHOICE_BADGES_APART: f32 = 3.5;
/// Middle of the slice the demo hovers (of four, starting at the east, turning clockwise).
const CHOICE_MID: f32 = TAU / 8.0;
/// The badge of the app that is open (the same gold as the real pie).
const CHOICE_GOLD: Color32 = Color32::from_rgb(212, 190, 118);
const CHOICE_GOLD_INK: Color32 = Color32::from_rgb(58, 50, 24);

/// One moment of the open-app choice demo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ChoiceFrame {
    /// How far the pie is open.
    pub pie: f32,
    /// How much the slice is hovered.
    pub hover: f32,
    /// How far the two badges have come out.
    pub badges: f32,
    /// How much the pointer is on the badge that is picked.
    pub pick: f32,
    /// Where the pointer is, from the center of the pie.
    pub cursor: Vec2,
    /// Which badge is picked in this half: `true` = new window.
    pub new_window: bool,
    /// How much of the result is shown: a new window, or the app that is open brought forward.
    pub result: f32,
}

/// Where the middle of a badge is, from the center of the pie.
fn choice_badge_at(new_window: bool) -> Vec2 {
    let dir = Vec2::angled(CHOICE_MID);
    let tangent = Vec2::new(-dir.y, dir.x);
    let half = CHOICE_BADGE / 2.0;
    let along = (half + CHOICE_BADGES_APART / 2.0) * if new_window { -1.0 } else { 1.0 };
    dir * (CHOICE_R + CHOICE_BADGE_GAP + half) + tangent * along
}

/// The open-app choice demo at `t` seconds. `enabled` is the setting: when it is off nothing is
/// offered and the slice just goes to the app.
pub(super) fn choice_frame(t: f32, enabled: bool) -> ChoiceFrame {
    let t = t.rem_euclid(CHOICE_HALF * 2.0);
    let new_window = enabled && t < CHOICE_HALF;
    let u = t.rem_euclid(CHOICE_HALF);
    let dir = Vec2::angled(CHOICE_MID);
    let tangent = Vec2::new(-dir.y, dir.x);
    let on_slice = dir * CHOICE_R * 0.75;
    // the pointer rests on the half of the slice nearer the badge it picks
    let side = if new_window { -1.0 } else { 1.0 };
    let on_half = on_slice + tangent * side * CHOICE_R * 0.3;
    let to_slice = ease_out_cubic(ramp(u, 0.5, 0.5));
    let to_half = ease_out_cubic(ramp(u, 1.9, 0.5));
    let cursor = if enabled {
        let a = on_slice * to_slice;
        a + (on_half - on_slice) * to_half
    } else {
        on_slice * to_slice
    };
    let closing = ramp(u, 3.2, 0.25);
    ChoiceFrame {
        pie: ramp(u, 0.2, OPEN_SECS) - closing,
        hover: ramp(u, 0.9, 0.2) * (1.0 - closing),
        badges: if enabled {
            ramp(u, 1.0, 0.45) * (1.0 - closing)
        } else {
            0.0
        },
        pick: if enabled {
            ramp(u, 2.2, 0.2) * (1.0 - closing)
        } else {
            0.0
        },
        cursor,
        new_window,
        result: ramp(u, 3.3, 0.3) - ramp(u, 4.2, 0.25),
    }
}

/// What the demo says at this moment.
fn choice_caption(enabled: bool, f: &ChoiceFrame) -> &'static str {
    if !enabled {
        return if f.result > 0.5 {
            "Off: it goes to the app, as before"
        } else {
            "Off: the slice always opens the app"
        };
    }
    if f.result > 0.5 {
        return if f.new_window {
            "A new window of the app opened"
        } else {
            "It went to the app that is open"
        };
    }
    if f.pick > 0.5 {
        return if f.new_window {
            "Release on the green + for a new window"
        } else {
            "Release on the yellow square for the open app"
        };
    }
    if f.badges > 0.5 {
        return "The app runs: two choices come out";
    }
    "Hover an Open app slice"
}

/// One round badge of the demo: sage with a + for a new window, gold with a square for the app that
/// is open. `show` fades and grows it in, `pick` lifts it, `step` pushes it back.
fn choice_badge(
    p: &egui::Painter,
    at: Pos2,
    new_window: bool,
    show: f32,
    pick: f32,
    step: f32,
    accent: Color32,
) {
    if show <= 0.01 {
        return;
    }
    let radius = CHOICE_BADGE / 2.0
        * (0.35 + 0.65 * ease_out_back(show))
        * (1.0 + 0.18 * pick - 0.08 * step);
    let alpha = ease_out_cubic(show) * (1.0 - 0.3 * step);
    let tint = if new_window { accent } else { CHOICE_GOLD };
    p.circle_filled(
        at + Vec2::new(0.0, 1.5),
        radius + 1.0,
        Color32::from_black_alpha((40.0 * alpha) as u8),
    );
    p.circle_filled(at, radius, tint.gamma_multiply(alpha));
    p.circle_stroke(
        at,
        radius,
        Stroke::new(1.0, Color32::from_black_alpha((34.0 * alpha) as u8)),
    );
    if pick > 0.02 {
        p.circle_stroke(
            at,
            radius + 2.0 * pick,
            Stroke::new(1.75, pal().text.gamma_multiply(alpha * pick)),
        );
    }
    let glyph = if new_window {
        on_accent(accent)
    } else {
        CHOICE_GOLD_INK
    }
    .gamma_multiply(alpha);
    if new_window {
        let arm = radius * 0.5;
        let stroke = Stroke::new((radius * 0.24).max(1.8), glyph);
        p.line_segment([at - Vec2::new(arm, 0.0), at + Vec2::new(arm, 0.0)], stroke);
        p.line_segment([at - Vec2::new(0.0, arm), at + Vec2::new(0.0, arm)], stroke);
    } else {
        let rect = Rect::from_center_size(at, Vec2::splat(radius * 0.9));
        p.rect_filled(rect, (radius * 0.14).max(1.0), glyph);
    }
}

/// A pie with one slice hovered and the app it opens already running (the window on the right). The
/// two badges come out of the slice; the pointer picks one and the window answers.
pub(in crate::ui::settings) fn open_choice_demo(ui: &mut egui::Ui, accent: Color32, enabled: bool) {
    let (p, rect, t) = stage(ui);
    let f = choice_frame(t, enabled);
    let c = Pos2::new(rect.min.x + rect.width() * 0.32, rect.min.y + PIE_Y);
    // the app that is already open, and the new window that may join it
    let w = rect.width() * 0.26;
    let win = Rect::from_min_size(
        Pos2::new(rect.max.x - rect.width() * 0.12 - w, rect.min.y + 22.0),
        Vec2::new(w, 96.0),
    );
    let went_to = if f.new_window { 0.0 } else { f.result };
    mini_window(&p, win, "Notes", 0.35 + 0.65 * went_to, accent);
    if went_to > 0.01 {
        p.rect_stroke(
            win.expand(3.0),
            11.0,
            Stroke::new(2.0, accent.gamma_multiply(went_to)),
            StrokeKind::Outside,
        );
    }
    if f.new_window && f.result > 0.01 {
        let r = ease_out_back(f.result.clamp(0.0, 1.0)).max(0.0);
        let second = Rect::from_center_size(
            win.center() + Vec2::new(14.0, 12.0),
            win.size() * (0.6 + 0.4 * r),
        );
        mini_window(&p, second, "Notes 2", 1.0, accent);
        p.rect_stroke(
            second.expand(3.0),
            11.0,
            Stroke::new(2.0, accent.gamma_multiply(f.result.clamp(0.0, 1.0))),
            StrokeKind::Outside,
        );
    }
    mini_pie(&p, c, CHOICE_R, f.pie, accent, false, 0.0);
    if f.pie > 0.01 && f.hover > 0.01 {
        let inner = CHOICE_R * 0.4;
        let outer = inner + (CHOICE_R - inner) * ease_out_cubic(f.pie);
        let quarter = TAU / 4.0;
        p.add(ring_slice(
            &p,
            c,
            inner,
            outer,
            0.05,
            quarter - 0.05,
            accent.gamma_multiply(f.hover * f.pie),
            Bend::NONE,
        ));
    }
    for new_window in [true, false] {
        let mine = new_window == f.new_window;
        choice_badge(
            &p,
            c + choice_badge_at(new_window),
            new_window,
            f.badges,
            if mine { f.pick } else { 0.0 },
            if mine { 0.0 } else { f.pick },
            accent,
        );
    }
    if f.pie > 0.01 {
        cursor(&p, c + f.cursor);
    }
    caption(&p, rect, choice_caption(enabled, &f));
    ui.add_space(GAP);
}

// --------------------------------------------------------------- the desktop

/// The seconds the desktop loop lasts.
const DESKTOP_LOOP: f32 = 4.4;

/// One moment of the desktop demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct DesktopFrame {
    /// The "Open app" slice is being pressed (a pulse from 0 to 1 and back).
    pub click: f32,
    /// What you are looking at: 0 the first desktop, 1 the second.
    pub view: f32,
    /// How far the app's window has come from the second desktop to the first.
    pub moved: f32,
    /// How visible a second window of the app is on the first desktop.
    pub copy: f32,
}

/// The desktop demo at `t` seconds: the app's window is on the second desktop, you are on the first.
pub(super) fn desktop_frame(t: f32, mode: DesktopMode) -> DesktopFrame {
    let t = t.rem_euclid(DESKTOP_LOOP);
    let click = ramp(t, 0.6, 0.15) - ramp(t, 0.75, 0.25);
    // the effect of the click, undone at the end of the loop so it can start again
    let went = ease_out_cubic(ramp(t, 0.9, 0.6)) * (1.0 - ramp(t, 3.8, 0.5));
    let mut f = DesktopFrame {
        click,
        ..DesktopFrame::default()
    };
    match mode {
        DesktopMode::SwitchToApp => f.view = went,
        DesktopMode::BringToCurrent => f.moved = went,
        DesktopMode::DoNothing => f.copy = went,
    }
    f
}

/// What the desktop demo says about the chosen mode. `noun` is what the system calls a desktop.
fn desktop_caption(mode: DesktopMode, noun: &str) -> String {
    match mode {
        DesktopMode::SwitchToApp => format!("Goes to the {noun} where the app is"),
        DesktopMode::BringToCurrent => "Brings the app's window to this one".to_string(),
        DesktopMode::DoNothing => "Opens the app here, as before".to_string(),
    }
}

/// Two desktops and an "Open app" slice, for the chosen mode.
pub(in crate::ui::settings) fn desktop_demo(
    ui: &mut egui::Ui,
    accent: Color32,
    mode: DesktopMode,
    noun: &str,
) {
    let (p, rect, t) = stage(ui);
    let f = desktop_frame(t, mode);
    let pad = 28.0;
    let w = (rect.width() - pad * 3.0) / 2.0;
    let box_at = |i: f32| {
        Rect::from_min_size(
            Pos2::new(rect.min.x + pad + (w + pad) * i, rect.min.y + 30.0),
            Vec2::new(w, 84.0),
        )
    };
    let (one, two) = (box_at(0.0), box_at(1.0));
    for (b, n) in [(one, 1), (two, 2)] {
        p.rect_filled(b, 8.0, pal().card);
        p.rect_stroke(b, 8.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
        p.text(
            b.center_top() - Vec2::new(0.0, 4.0),
            Align2::CENTER_BOTTOM,
            format!("{} {n}", capitalized(noun)),
            FontId::proportional(11.0),
            pal().muted,
        );
    }
    let size = Vec2::new(64.0, 44.0);
    let moved = two.center() + (one.center() - two.center()) * f.moved;
    mini_window(&p, Rect::from_center_size(moved, size), "App", 1.0, accent);
    if f.copy > 0.01 {
        // the same app, opened a second time on this desktop: it grows into place
        let grown = size * (0.5 + 0.5 * f.copy);
        mini_window(
            &p,
            Rect::from_center_size(one.center(), grown),
            "App",
            1.0,
            accent,
        );
    }
    // what you are looking at
    let view = Rect::from_min_max(
        one.min + (two.min - one.min) * f.view,
        one.max + (two.max - one.max) * f.view,
    );
    p.rect_stroke(
        view.expand(4.0),
        11.0,
        Stroke::new(2.0, accent),
        StrokeKind::Outside,
    );
    caption_at(&p, rect.min.y + 134.0, rect, &desktop_caption(mode, noun));
    // the slice that is pressed
    let pill = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.max.y - 20.0),
        Vec2::new(100.0, 24.0),
    );
    p.rect_filled(pill, 11.0, pal().idle.lerp_to_gamma(accent, f.click));
    p.text(
        pill.center(),
        Align2::CENTER_CENTER,
        "Open app",
        FontId::proportional(11.0),
        pal().text,
    );
    ui.add_space(GAP);
}

// ------------------------------------------------------------------ the look

/// Seconds the pointer stays on one slice in the look demo.
const LOOK_STEP: f32 = 1.1;

/// One moment of the look demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct LookFrame {
    /// The slice the pointer is on (0 to 3).
    pub slice: usize,
    /// How far that slice has blended to the accent.
    pub hover: f32,
}

/// The look demo at `t` seconds: the pointer visits the four slices one after another.
pub(super) fn look_frame(t: f32) -> LookFrame {
    let t = t.rem_euclid(LOOK_STEP * 4.0);
    let slice = ((t / LOOK_STEP) as usize).min(3);
    let u = t - slice as f32 * LOOK_STEP;
    LookFrame {
        slice,
        hover: ramp(u, 0.0, 0.15) - ramp(u, LOOK_STEP - 0.2, 0.2),
    }
}

/// Radius of the pie in the look demo for a pie of `size` pixels in the real thing (140 to 360).
fn look_radius(size: f32) -> f32 {
    32.0 + 40.0 * ((size - 140.0) / 220.0).clamp(0.0, 1.0)
}

/// What the look demo says about the pie: its size and whether it has names.
fn look_caption(size: f32, labels: bool) -> String {
    let what = if labels {
        "icons and names"
    } else {
        "icons only"
    };
    format!("{size:.0} px, {what}")
}

/// What a pie of the look demos is like: the accent on the slice under the pointer, its names, the
/// app's icon in the middle (in its colors or as a silhouette), the material and the outline.
#[derive(Clone, Copy)]
pub(in crate::ui::settings) struct LookPie {
    pub accent: Color32,
    pub labels: bool,
    pub center_icon: bool,
    pub app_icon: AppIconStyle,
    pub material: Material,
    pub outline: Outline,
}

/// Paints a pie of radius `r` centered on `c` for a moment of a demo: `open` (0 to 1) is how far it
/// has come out, `f` where the pointer is. It is drawn the way the real pie draws itself.
fn look_pie(p: &egui::Painter, c: Pos2, r: f32, (open, f): (f32, LookFrame), look: &LookPie) {
    let fade = ease_out_cubic(open);
    let pop = ease_out_back(open);
    let r = r * (0.80 + 0.20 * pop);
    let inner = r * 0.4 * (0.75 + 0.25 * pop) / (0.80 + 0.20 * pop);
    let bend = Bend::new(look.outline, inner, r);
    let colors = look.material.colors();
    let accent = look.accent;
    let quarter = TAU / 4.0;
    let slices = [
        (symbols::COPY, "Copy"),
        (symbols::CLIPBOARD_TEXT, "Paste"),
        (symbols::ARROW_U_UP_LEFT, "Undo"),
        (symbols::DOWNLOAD_SIMPLE, "Save"),
    ];
    for (i, (icon, name)) in slices.into_iter().enumerate() {
        let h = if i == f.slice { f.hover } else { 0.0 };
        let mid = -FRAC_PI_2 + quarter * i as f32;
        let (a0, a1) = (mid - quarter / 2.0 + 0.03, mid + quarter / 2.0 - 0.03);
        let base = mix(colors.slice, [accent.r(), accent.g(), accent.b()], h * 0.95);
        match look.material {
            Material::Glass(g) => {
                glass::slice(p, c, bend, (inner, r), (a0, a1), base, h, fade, g, false);
            }
            Material::GenZ => genz::slice(p, c, bend, (inner, r), (a0, a1), base, h, fade, false),
            Material::Solid => {
                let fill = rgb(base).gamma_multiply(fade);
                p.add(ring_slice(p, c, inner, r, a0, a1, fill, bend));
            }
        }
        let ink = colors.ink.lerp_to_gamma(on_accent(accent), h * 0.95);
        let at = c + Vec2::angled(mid) * bend.radius(mid, (inner + r) / 2.0);
        let (dy, icon_size) = if look.labels {
            (-r * 0.12, r * 0.36)
        } else {
            (0.0, r * 0.44)
        };
        fonts::paint_icon(
            p,
            at + Vec2::new(0.0, dy),
            icon,
            icon_size,
            ink.gamma_multiply(fade),
            false,
        );
        if look.labels {
            p.text(
                at + Vec2::new(0.0, r * 0.2),
                Align2::CENTER_CENTER,
                name,
                FontId::proportional((r * 0.17).max(9.0)),
                ink.gamma_multiply(0.8 * fade),
            );
        }
    }
    if look.material == Material::Solid {
        p.add(bend.rim(c, r, Stroke::new(1.0, pal().border.gamma_multiply(fade))));
    }
    let core = inner * 0.86;
    match look.material {
        Material::Glass(g) => glass::core(p, c, core, fade, Some(colors.core), g),
        Material::GenZ => genz::core(p, c, core, fade, colors.core),
        Material::Solid => {
            p.circle_filled(c, core, colors.core.gamma_multiply(fade));
        }
    }
    if look.center_icon {
        // the app's icon, here a made-up one: in its colors, or a silhouette in the ink
        let blend = if look.app_icon == AppIconStyle::Color {
            0.0
        } else {
            1.0
        };
        let ink = colors.ink.gamma_multiply(0.85);
        demo_app_icon(p, c, core * 1.3, blend, ink, fade);
    }
}

/// The pie as the Look & feel settings make it: size, names, the app's icon in the middle, the
/// accent on the slice under the pointer, the material (solid, glass or Gen Z) and the outline.
pub(in crate::ui::settings) fn look_demo(ui: &mut egui::Ui, look: LookPie, size: f32) {
    let (p, rect, t) = stage(ui);
    let c = Pos2::new(rect.center().x, rect.min.y + PIE_Y);
    look_pie(&p, c, look_radius(size), (1.0, look_frame(t)), &look);
    caption(&p, rect, &look_caption(size, look.labels));
    ui.add_space(GAP);
}

// ------------------------------------------------------------ the pies side by side

/// Seconds a pie of [`tiles_demo`] waits, open, before the pointer comes to its first slice.
const TILE_LEAD: f32 = 0.45;
/// Seconds one round of [`tiles_demo`] takes: the pie comes out, the pointer visits the four
/// slices, the pie goes away.
const TILE_LOOP: f32 = TILE_LEAD + LOOK_STEP * 4.0 + 0.5;
/// Seconds between one pie of [`tiles_demo`] and the next, so they are not all on the same slice.
const TILE_SHIFT: f32 = LOOK_STEP;

/// The pie of [`tiles_demo`] at `t` seconds: how far it is open and where the pointer is. The
/// pointer is only on it while it is open.
pub(super) fn tile_frame(t: f32) -> (f32, LookFrame) {
    let t = t.rem_euclid(TILE_LOOP);
    let u = t - TILE_LEAD;
    let open = ramp(t, 0.0, OPEN_SECS) - ramp(t, TILE_LOOP - CLOSE_SECS, CLOSE_SECS);
    let pointer = if (0.0..LOOK_STEP * 4.0).contains(&u) {
        look_frame(u)
    } else {
        LookFrame::default()
    };
    (open, pointer)
}

/// One pie of [`tiles_demo`].
pub(in crate::ui::settings) struct Tile<'a> {
    /// Under the pie.
    pub name: &'a str,
    /// Shown while the pointer is on the pie.
    pub hint: &'a str,
    pub material: Material,
    /// The one the settings have (it gets the accent around its name).
    pub chosen: bool,
}

/// The colors the glass sits on: a few blobs of color and a sharp diagonal, so what comes through
/// shows (and what does not).
fn backdrop(p: &egui::Painter, rect: Rect) {
    p.rect_filled(rect, control_r(), rgb([64, 120, 214]));
    for (dx, dy, r, c) in [
        (0.12, 0.2, 52.0, [255, 149, 0]),
        (0.35, 0.9, 44.0, [255, 45, 85]),
        (0.62, 0.15, 56.0, [52, 199, 89]),
        (0.88, 0.8, 50.0, [175, 82, 222]),
    ] {
        let at = Pos2::new(
            rect.min.x + rect.width() * dx,
            rect.min.y + rect.height() * dy,
        );
        p.circle_filled(at, r, rgb(c));
    }
    p.line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(3.0, Color32::WHITE),
    );
}

/// The id of tile `i` of the [`tiles_demo`] called `salt`. It must be unique to that call: two
/// calls on one page (the material and the shape of the pie) that built it from the id of their
/// `Ui` shared the id of their first tile, and the one drawn last took the clicks of the other,
/// so the first material (Solid) could not be chosen.
fn tile_id(salt: &str, i: usize) -> egui::Id {
    egui::Id::new(("tiles_demo", salt, i))
}

/// Pies of different materials side by side, each coming out and being passed over by the pointer
/// the way the real one does, over a colorful backdrop. A click on one returns its index, so the
/// demo is also how a material is chosen. `salt` tells this demo from any other on the page.
pub(in crate::ui::settings) fn tiles_demo(
    ui: &mut egui::Ui,
    salt: &str,
    accent: Color32,
    outline: Outline,
    tiles: &[Tile],
) -> Option<usize> {
    let (p, rect, t) = stage(ui);
    backdrop(&p, rect);
    let width = rect.width() / tiles.len().max(1) as f32;
    let r = (width / 2.0 - 14.0).clamp(30.0, 50.0);
    let mut clicked = None;
    for (i, tile) in tiles.iter().enumerate() {
        let column = Rect::from_min_size(
            rect.min + Vec2::new(width * i as f32, 0.0),
            Vec2::new(width, rect.height()),
        );
        let c = Pos2::new(column.center().x, rect.min.y + PIE_Y - 8.0);
        let look = LookPie {
            accent,
            labels: false,
            center_icon: false,
            app_icon: AppIconStyle::Glyph,
            material: tile.material,
            outline,
        };
        look_pie(&p, c, r, tile_frame(t + TILE_SHIFT * i as f32), &look);
        // the name, on a pill that reads on any color
        let (fill, ink) = if tile.chosen {
            (accent, on_accent(accent))
        } else {
            (pal().input, pal().text)
        };
        let name = p.layout_no_wrap(tile.name.to_owned(), FontId::proportional(12.5), ink);
        let pill = Rect::from_center_size(
            Pos2::new(column.center().x, rect.max.y - 30.0),
            name.size() + Vec2::new(22.0, 10.0),
        );
        p.rect_filled(pill, pill.height() / 2.0, fill);
        p.galley(pill.center() - name.size() / 2.0, name, ink);
        let response = ui
            .interact(column, tile_id(salt, i), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tile.hint);
        if response.clicked() {
            clicked = Some(i);
        }
    }
    ui.add_space(GAP);
    clicked
}

// --------------------------------------------------------------- the submenu trail

/// Seconds the trail loop lasts.
const TRAIL_LOOP: f32 = 8.0;
/// Outer radius of the ring in the trail demo, in points.
const TRAIL_R: f32 = 28.0;
/// Seconds the ring takes to bloom (the real one blooms in 0.22).
const TRAIL_BLOOM: f32 = 0.22;
/// Seconds a node takes to glide to its place at the usual speed.
const TRAIL_GLIDE_SECS: f32 = 0.4;
/// When the pointer leaves the first ring, the second one, and starts for a node of the trail.
const TRAIL_T_OUT_1: f32 = 1.0;
const TRAIL_T_OUT_2: f32 = 2.7;
const TRAIL_T_BACK: f32 = 4.2;
/// When the levels change: the first submenu opens, the second one, and the way back.
const TRAIL_AT_1: f32 = 1.5;
const TRAIL_AT_2: f32 = 3.2;
const TRAIL_AT_3: f32 = 4.7;
/// When the demo starts to fade away.
const TRAIL_T_FADE: f32 = 7.2;
/// Seconds the pointer takes to cross from one place to the next.
const TRAIL_MOVE: f32 = 0.5;

/// One node of the trail in the demo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TrailDemoNode {
    /// Where it is now, from the center of the ring on screen.
    pub at: Vec2,
    /// Its radius now (it grows as it arrives).
    pub r: f32,
    /// 0 for the ring just left, 1 for the one before it, ...
    pub slot: usize,
}

/// One moment of the trail demo.
#[derive(Clone, Debug, PartialEq, Default)]
pub(super) struct TrailFrame {
    /// How visible all of it is (it fades away at the end of the loop).
    pub fade: f32,
    /// How far the ring on screen has bloomed (every level blooms).
    pub bloom: f32,
    /// The pointer, from the center of the ring on screen.
    pub pointer: Vec2,
    /// The nodes, the nearest first.
    pub nodes: Vec<TrailDemoNode>,
    /// Which part of the story it is (0 to 3, see [`trail_caption`]).
    pub step: usize,
    /// The pointer is on the node it is going back to.
    pub on_node: bool,
}

/// Where the nodes of the rings at `centers` go, seen from the ring on screen at `cur`: the real
/// layout of the pie, for a ring of the size of the demo's.
fn trail_nodes(centers: &[Vec2], cur: Vec2, shape: TrailShape) -> Vec<TrailNode> {
    let offsets: Vec<(f32, f32)> = centers.iter().map(|c| (c.x - cur.x, c.y - cur.y)).collect();
    // a pie of this window has an outer radius of TRAIL_R
    trail_layout(&offsets, 2.0 * (TRAIL_R + 10.0), shape)
}

/// Where the node of ring `level` is in `nodes`.
fn node_pos(nodes: &[TrailNode], level: usize) -> Option<Vec2> {
    nodes
        .iter()
        .find(|n| n.level == level)
        .map(|n| Vec2::new(n.x, n.y))
}

/// The nodes `secs` after they were asked to go to `targets`: each starts at the place `from` gives
/// for its level and glides to its target, and grows as it arrives, as in the real pie.
fn glide_nodes(
    targets: &[TrailNode],
    from: &dyn Fn(usize) -> Vec2,
    secs: f32,
    duration: f32,
) -> Vec<TrailDemoNode> {
    let k = ease_out_cubic(ramp(secs, 0.0, duration));
    targets
        .iter()
        .enumerate()
        .map(|(slot, n)| {
            let to = Vec2::new(n.x, n.y);
            let start = from(n.level);
            let at = start + (to - start) * k;
            let left = ((at - to).length() / (1.2 * TRAIL_R)).clamp(0.0, 1.0);
            TrailDemoNode {
                at,
                r: n.r * (0.55 + 0.45 * (1.0 - left)),
                slot,
            }
        })
        .collect()
}

/// The trail demo at `t` seconds, for a trail of `shape` whose nodes glide at `glide` times the
/// usual speed: a pie opens, the pointer crosses a slice with a dot and a submenu opens where the
/// pointer is, while the pie it left shrinks to a node; again, and the nodes line up on an arc;
/// then the pointer goes onto a node and that pie is back, and the loop fades out.
///
/// The positions are the real layout (`geometry::trail_layout`), so the distance, the size, the
/// shrinking and the spacing of the settings change the demo exactly as they change the pie.
pub(super) fn trail_frame(t: f32, shape: TrailShape, glide: f32) -> TrailFrame {
    let t = t.rem_euclid(TRAIL_LOOP);
    let duration = TRAIL_GLIDE_SECS / glide.clamp(0.1, 4.0);
    // the centers of the rings, in one frame of reference: the pointer crosses the rim of each
    // ring a little beyond it, and the next ring is centered there
    let g0 = Vec2::ZERO;
    let g1 = g0 + Vec2::new(1.1 * TRAIL_R, 0.0);
    let g2 = g1 + Vec2::new(0.0, 1.1 * TRAIL_R);
    let n1 = trail_nodes(&[g0], g1, shape);
    let n2 = trail_nodes(&[g0, g1], g2, shape);
    // going back: the ring of the nearest node is centered where the pointer is, on that node, and
    // the rings behind it move with it
    let g3 = g2 + node_pos(&n2, 1).unwrap_or(Vec2::ZERO);
    let g0b = g0 + (g3 - g1);
    let n3 = trail_nodes(&[g0b], g3, shape);
    let go = |secs: f32, from: f32| ease_out_cubic(ramp(secs, from, TRAIL_MOVE));
    let bloom = |secs: f32| ease_out_cubic(ramp(secs, 0.0, TRAIL_BLOOM));
    let fade = 1.0 - ramp(t, TRAIL_T_FADE, 0.5);
    let pos = |nodes: &[TrailNode], level: usize| node_pos(nodes, level).unwrap_or(Vec2::ZERO);

    let mut f = if t < TRAIL_AT_1 {
        TrailFrame {
            bloom: bloom(t),
            pointer: (g1 - g0) * go(t, TRAIL_T_OUT_1),
            step: 0,
            ..TrailFrame::default()
        }
    } else if t < TRAIL_AT_2 {
        let se = t - TRAIL_AT_1;
        TrailFrame {
            bloom: bloom(se),
            pointer: (g2 - g1) * go(t, TRAIL_T_OUT_2),
            // the ring that was left starts where it was
            nodes: glide_nodes(&n1, &|_| g0 - g1, se, duration),
            step: 1,
            ..TrailFrame::default()
        }
    } else if t < TRAIL_AT_3 {
        let se = t - TRAIL_AT_2;
        let moved = go(t, TRAIL_T_BACK);
        TrailFrame {
            bloom: bloom(se),
            pointer: (g3 - g2) * moved,
            // the nodes of the level before keep their places in the world and glide to the new ones
            nodes: glide_nodes(
                &n2,
                &|level| {
                    if level == 1 {
                        g1 - g2
                    } else {
                        pos(&n1, level) + (g1 - g2)
                    }
                },
                se,
                duration,
            ),
            step: 2,
            on_node: moved > 0.95,
            ..TrailFrame::default()
        }
    } else {
        let se = t - TRAIL_AT_3;
        TrailFrame {
            bloom: bloom(se),
            nodes: glide_nodes(&n3, &|level| pos(&n2, level) + (g2 - g3), se, duration),
            step: 3,
            ..TrailFrame::default()
        }
    };
    f.fade = fade;
    f
}

/// What the trail demo says about each part of its story.
fn trail_caption(step: usize) -> &'static str {
    match step {
        0 => "The pie opens; the pointer crosses a slice with a dot",
        1 => "The submenu opens where the pointer is; the pie you left shrinks to a node",
        2 => "Every level you left stays as a node on an arc, nearest first",
        _ => "Move the pointer onto a node: that pie is back",
    }
}

/// A small arrowhead at `at`, pointing along `dir` (a unit vector), as on the threads of the pie.
fn trail_chevron(p: &egui::Painter, at: Pos2, dir: Vec2, stroke: Stroke) {
    let side = Vec2::new(-dir.y, dir.x);
    let tip = at + dir * 3.6;
    let back = at - dir * 2.4;
    p.add(egui::Shape::line(
        vec![back + side * 4.2, tip, back - side * 4.2],
        stroke,
    ));
}

/// The submenu trail as the settings make it: how far the nodes sit (`shape`), whether the thread
/// is drawn, how fast the nodes glide. `enabled` is the web mode itself: while it is off the demo
/// is dimmed, because the settings do nothing.
pub(in crate::ui::settings) fn trail_demo(
    ui: &mut egui::Ui,
    accent: Color32,
    shape: TrailShape,
    thread: bool,
    glide: f32,
    enabled: bool,
) {
    let (p, rect, t) = stage(ui);
    let f = trail_frame(t, shape, glide);
    let dim = if enabled { 1.0 } else { 0.4 };
    let fade = f.fade * dim;
    let colors = pie_colors();
    let c = Pos2::new(rect.center().x, rect.min.y + PIE_Y - 4.0);

    if thread && fade > 0.01 {
        let line = |from: Pos2, to: Pos2, w: f32, a: f32| {
            p.line_segment(
                [from, to],
                Stroke::new(w + 2.0, colors.core.gamma_multiply(0.55 * fade)),
            );
            let stroke = Stroke::new(w, colors.ink.gamma_multiply(a * fade));
            p.line_segment([from, to], stroke);
            let d = to - from;
            if d.length() > 24.0 {
                trail_chevron(&p, from + d * 0.5, d.normalized(), stroke);
            }
        };
        if let Some(first) = f.nodes.first() {
            line(c + first.at, c, 3.0, 0.9);
        }
        for w in f.nodes.windows(2) {
            let age = (1.0 - 0.13 * w[1].slot as f32).max(0.45);
            line(c + w[1].at, c + w[0].at, 2.0, 0.65 * age);
        }
    }
    // the nodes, the oldest first, so the nearest is in front
    for n in f.nodes.iter().rev() {
        let age = (1.0 - 0.13 * n.slot as f32).max(0.45);
        let at = c + n.at;
        p.circle_filled(
            at,
            n.r,
            colors.core.gamma_multiply((0.6 + 0.37 * age) * fade),
        );
        let edge = if n.slot == 0 {
            accent.gamma_multiply(fade)
        } else {
            colors.ink.gamma_multiply(0.6 * age * fade)
        };
        let lit = if f.on_node && n.slot == 0 { 3.0 } else { 2.0 };
        p.circle_stroke(at, n.r, Stroke::new(lit, edge));
        p.circle_filled(at, n.r * 0.3, colors.ink.gamma_multiply(0.6 * age * fade));
    }
    mini_pie(&p, c, TRAIL_R, f.bloom * fade, accent, false, 0.0);
    if fade > 0.05 {
        cursor(&p, c + f.pointer);
    }
    let text = if enabled {
        trail_caption(f.step)
    } else {
        "Off: a submenu replaces the pie"
    };
    caption(&p, rect, text);
    ui.add_space(GAP);
}

// ------------------------------------------------------------ the app's icon

/// Seconds the app-icon loop lasts.
const ICON_LOOP: f32 = 4.4;

/// One moment of the app-icon demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct AppIconFrame {
    /// How far the pie is open.
    pub pie: f32,
    /// Where the copy of the app's icon is: 0 at the app, 1 in the middle of the pie.
    pub fly: f32,
    /// How visible that copy is (it appears when it sets off and goes with the pie).
    pub icon: f32,
    /// 0: the icon keeps its colors; 1: it is a one-color silhouette.
    pub blend: f32,
}

/// The app-icon demo at `t` seconds: the pie opens, the app's icon flies into its middle and, if
/// the style is a silhouette, loses its colors on the way in.
pub(super) fn app_icon_frame(t: f32, style: AppIconStyle) -> AppIconFrame {
    let t = t.rem_euclid(ICON_LOOP);
    let leaves = 3.9;
    AppIconFrame {
        pie: ramp(t, 0.2, 0.3) - ramp(t, leaves, 0.4),
        fly: ease_out_cubic(ramp(t, 0.9, 0.8)),
        icon: ramp(t, 0.9, 0.1) - ramp(t, leaves, 0.4),
        blend: match style {
            AppIconStyle::Glyph => ease_out_cubic(ramp(t, 1.7, 0.5)),
            AppIconStyle::Color => 0.0,
        },
    }
}

/// What the app-icon demo says about the icon.
fn app_icon_caption(style: AppIconStyle, enabled: bool) -> &'static str {
    match (enabled, style) {
        (false, _) => "Off: the center shows the icon of the active mode",
        (true, AppIconStyle::Glyph) => "One color: a silhouette in the ink of the pie",
        (true, AppIconStyle::Color) => "Full color: the app's own icon",
    }
}

/// A made-up app icon of `size` px centered at `c`: a blue tile with a red square and a yellow
/// dot. `blend` 0 keeps those colors; 1 drops the tile and draws the shapes in `ink`, which is
/// what the pie makes of a real icon. `alpha` fades all of it.
fn demo_app_icon(p: &egui::Painter, c: Pos2, size: f32, blend: f32, ink: Color32, alpha: f32) {
    let own = |color: Color32| color.lerp_to_gamma(ink, blend).gamma_multiply(alpha);
    let tile = Rect::from_center_size(c, Vec2::splat(size));
    p.rect_filled(
        tile,
        size * 0.23,
        Color32::from_rgb(52, 120, 246).gamma_multiply((1.0 - blend) * alpha),
    );
    let square = Rect::from_center_size(
        c + Vec2::new(-size * 0.08, size * 0.08),
        Vec2::splat(size * 0.34),
    );
    p.rect_filled(square, size * 0.06, own(Color32::from_rgb(255, 69, 58)));
    p.circle_filled(
        c + Vec2::new(size * 0.17, -size * 0.17),
        size * 0.12,
        own(Color32::from_rgb(255, 204, 0)),
    );
}

/// The icon of an app in the middle of its pie: the real icon sits at the left, a copy flies into
/// the pie and either stays in its colors or turns into a silhouette, as the setting says.
/// `enabled` is the "Show the app's icon in the center" switch: off, no icon flies.
pub(in crate::ui::settings) fn app_icon_demo(
    ui: &mut egui::Ui,
    accent: Color32,
    style: AppIconStyle,
    enabled: bool,
) {
    let (p, rect, t) = stage(ui);
    let f = app_icon_frame(t, style);
    let colors = pie_colors();
    let reach = (rect.width() * 0.2).min(90.0);
    let home = Pos2::new(rect.center().x - reach - 20.0, rect.min.y + PIE_Y);
    let c = Pos2::new(rect.center().x + reach, rect.min.y + PIE_Y);
    // the app: its real icon stays where it is
    demo_app_icon(&p, home, 36.0, 0.0, colors.ink, 1.0);
    p.text(
        home + Vec2::new(0.0, 34.0),
        Align2::CENTER_CENTER,
        "App",
        FontId::proportional(11.0),
        pal().muted,
    );
    mini_pie(&p, c, PIE_R, f.pie, accent, false, 0.0);
    if enabled && f.icon > 0.0 {
        let core = PIE_R * 0.4 * 0.86;
        let size = 36.0 + (core * 1.3 - 36.0) * f.fly;
        let mut at = home + (c - home) * f.fly;
        at.y -= (f.fly * std::f32::consts::PI).sin() * 14.0;
        let ink = colors.ink.gamma_multiply(0.85);
        demo_app_icon(&p, at, size, f.blend, ink, f.icon);
    }
    caption(&p, rect, app_icon_caption(style, enabled));
    ui.add_space(GAP);
}

// ------------------------------------------------------------ the side buttons

/// The seconds the side-button loop lasts.
const SIDE_LOOP: f32 = 6.0;

/// One moment of the side-button demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct SideFrame {
    /// The side button is down.
    pub side_button: bool,
    /// The right button is down.
    pub right_button: bool,
    /// Which pie the side buttons follow: 0 the app's pie, 1 the general pie. It moves between the
    /// two while a right-click switches it.
    pub follow: f32,
    /// A press of the side button has just run a slice of the pie it follows: 0 for none, then
    /// 1 at the end of the pulse.
    pub fire: f32,
}

/// The side-button demo at `t` seconds. `default` is the pie the buttons follow to begin with and
/// `switch` is "right-click switches the pie".
pub(super) fn side_frame(t: f32, default: SideTarget, switch: bool) -> SideFrame {
    let t = t.rem_euclid(SIDE_LOOP);
    let base = if default == SideTarget::General {
        1.0
    } else {
        0.0
    };
    // a right-click moves the buttons to the other pie; it is undone at the end of the loop
    let flip = if switch {
        ease_out_cubic(ramp(t, 2.8, 0.4)) * (1.0 - ramp(t, 5.5, 0.4))
    } else {
        0.0
    };
    let pulse = |from: f32| {
        if (from..from + 0.7).contains(&t) {
            ramp(t, from, 0.7)
        } else {
            0.0
        }
    };
    SideFrame {
        side_button: (0.6..0.9).contains(&t) || (switch && (3.8..4.1).contains(&t)),
        right_button: switch && (2.6..2.9).contains(&t),
        follow: base + (1.0 - 2.0 * base) * flip,
        fire: if switch {
            pulse(0.6).max(pulse(3.8))
        } else {
            pulse(0.6)
        },
    }
}

/// What the side-button demo says under the pies.
fn side_caption(f: &SideFrame) -> &'static str {
    if f.right_button {
        "Right-click: the buttons move to the other pie"
    } else if f.follow > 0.5 {
        "The side buttons follow the general pie"
    } else {
        "The side buttons follow the app's pie"
    }
}

/// A mouse with its side button (on the left edge) and its right button, lit while they are down.
fn side_mouse(p: &egui::Painter, center: Pos2, side: bool, right: bool, accent: Color32) {
    mouse(p, center, false, accent);
    let body = Rect::from_center_size(center, MOUSE_SIZE);
    let tab = Rect::from_center_size(
        Pos2::new(body.min.x - 1.0, center.y + 6.0),
        Vec2::new(7.0, 16.0),
    );
    p.rect_filled(tab, 3.0, if side { accent } else { pal().track });
    if right {
        let half = Rect::from_min_max(
            Pos2::new(center.x + 1.5, body.min.y + 2.5),
            Pos2::new(body.max.x - 2.5, body.min.y + 22.0),
        );
        p.rect_filled(half, 8.0, accent.gamma_multiply(0.7));
    }
}

/// The two pies of an app that has its own, and the side buttons following one of them. With
/// `indicator` the ring of the pie they follow is the accent and the other one is `inactive`.
pub(in crate::ui::settings) fn side_demo(
    ui: &mut egui::Ui,
    accent: Color32,
    inactive: Color32,
    default: SideTarget,
    switch: bool,
    indicator: bool,
) {
    let (p, rect, t) = stage(ui);
    let f = side_frame(t, default, switch);
    let y = rect.min.y + PIE_Y;
    side_mouse(
        &p,
        Pos2::new(rect.min.x + rect.width() * 0.14, y),
        f.side_button,
        f.right_button,
        accent,
    );
    let pies = [
        (rect.min.x + rect.width() * 0.45, true, 1.0 - f.follow),
        (rect.min.x + rect.width() * 0.8, false, f.follow),
    ];
    for (x, app, follows) in pies {
        let c = Pos2::new(x, y);
        mini_pie(&p, c, PIE_R, 1.0, accent, app, 0.0);
        if indicator {
            let ring = inactive.lerp_to_gamma(accent, follows);
            p.circle_stroke(c, PIE_R, Stroke::new(4.0, ring));
        }
        if f.fire > 0.0 && follows > 0.5 {
            // a press ran a slice: a ring spreads from the pie and fades
            p.circle_stroke(
                c,
                PIE_R + 4.0 + 12.0 * f.fire,
                Stroke::new(2.0, accent.gamma_multiply(1.0 - f.fire)),
            );
        }
        p.text(
            c + Vec2::new(0.0, PIE_R + 14.0),
            Align2::CENTER_CENTER,
            if app { "App's pie" } else { "General pie" },
            FontId::proportional(11.0),
            pal().muted,
        );
    }
    caption(&p, rect, side_caption(&f));
    ui.add_space(GAP);
}

// ------------------------------------------------------------------- the memory

/// The seconds the memory loop lasts, the moment the memory is freed and the moment the trigger
/// is pressed again.
const MEMORY_LOOP: f32 = 6.4;
const MEMORY_FREED: f32 = 3.2;
const MEMORY_PRESS: f32 = 4.6;

/// One moment of the memory demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct MemoryFrame {
    /// How far the idle timer has run (0 to 1; 1 is the time set in the slider).
    pub idle: f32,
    /// How much of the icons and the editor's copy is held in memory (0 to 1).
    pub used: f32,
    /// The trigger is down.
    pub pressed: bool,
    /// The trigger has been pressed and the memory is being filled again.
    pub reload: bool,
}

/// The memory demo at `t` seconds. With `enabled` off nothing is ever freed.
pub(super) fn memory_frame(t: f32, enabled: bool) -> MemoryFrame {
    if !enabled {
        return MemoryFrame {
            used: 1.0,
            ..MemoryFrame::default()
        };
    }
    let t = t.rem_euclid(MEMORY_LOOP);
    if t < MEMORY_PRESS {
        MemoryFrame {
            idle: ramp(t, 0.8, MEMORY_FREED - 0.8),
            used: 1.0 - 0.85 * ease_out_cubic(ramp(t, MEMORY_FREED, 0.4)),
            pressed: false,
            reload: false,
        }
    } else {
        MemoryFrame {
            idle: 0.0,
            used: 0.15 + 0.85 * ease_out_cubic(ramp(t, MEMORY_PRESS + 0.2, 0.5)),
            pressed: t < MEMORY_PRESS + 0.3,
            reload: true,
        }
    }
}

/// What the memory demo says about the moment it shows.
fn memory_caption(enabled: bool, f: &MemoryFrame) -> &'static str {
    if !enabled {
        "Kept in memory all the time"
    } else if f.reload {
        "Loaded again when you press the trigger"
    } else if f.used < 0.5 {
        "Freed: the icons and the editor's copy are dropped"
    } else if f.idle > 0.0 {
        "Nothing is open: counting the idle time"
    } else {
        "In use"
    }
}

/// A bar of what is held in memory, a bar of the idle time, and the trigger that loads it again.
pub(in crate::ui::settings) fn memory_demo(
    ui: &mut egui::Ui,
    accent: Color32,
    enabled: bool,
    secs: u64,
) {
    let (p, rect, t) = stage(ui);
    let f = memory_frame(t, enabled);
    let (x0, x1) = (
        rect.min.x + rect.width() * 0.14,
        rect.max.x - rect.width() * 0.14,
    );
    let small = FontId::proportional(11.0);

    // what is held in memory
    let y = rect.min.y + 50.0;
    p.text(
        Pos2::new(x0, y - 18.0),
        Align2::LEFT_CENTER,
        "Icons and the editor's copy",
        small.clone(),
        pal().muted,
    );
    let bar = Rect::from_min_max(Pos2::new(x0, y - 6.0), Pos2::new(x1, y + 6.0));
    p.rect_filled(bar, 6.0, pal().track);
    let held = Rect::from_min_max(bar.min, Pos2::new(x0 + (x1 - x0) * f.used, bar.max.y));
    p.rect_filled(held, 6.0, accent);

    // the time without a pie or Settings
    let y = rect.min.y + 100.0;
    p.text(
        Pos2::new(x0, y - 18.0),
        Align2::LEFT_CENTER,
        "Time without a pie or Settings",
        small.clone(),
        pal().muted,
    );
    p.text(
        Pos2::new(x1, y - 18.0),
        Align2::RIGHT_CENTER,
        if enabled {
            format!("{secs} s")
        } else {
            "never freed".to_string()
        },
        small,
        pal().muted,
    );
    let bar = Rect::from_min_max(Pos2::new(x0, y - 3.0), Pos2::new(x1, y + 3.0));
    p.rect_filled(bar, 3.0, pal().track);
    if f.idle > 0.0 {
        let run = Rect::from_min_max(bar.min, Pos2::new(x0 + (x1 - x0) * f.idle, bar.max.y));
        p.rect_filled(run, 3.0, pal().muted);
    }

    // the trigger that loads it again
    let pill = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.min.y + 138.0),
        Vec2::new(120.0, 24.0),
    );
    let press = if f.pressed { 1.0 } else { 0.0 };
    p.rect_filled(pill, 12.0, pal().idle.lerp_to_gamma(accent, press));
    p.text(
        pill.center(),
        Align2::CENTER_CENTER,
        "Press the trigger",
        FontId::proportional(11.0),
        pal().text,
    );
    caption(&p, rect, memory_caption(enabled, &f));
    ui.add_space(GAP);
}

// --------------------------------------------------------------------- the scan

/// The seconds the scan loop lasts.
const SCAN_LOOP: f32 = 5.0;
/// The extra buttons the demo mouse has (back and forward) and when each is pressed.
const SCAN_BUTTONS: [i64; 2] = [3, 4];
const SCAN_PRESSES: [f32; 2] = [0.8, 1.9];
/// When "Done" is pressed.
const SCAN_DONE: f32 = 3.4;

/// One moment of the scan demo.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(super) struct ScanFrame {
    /// The button that is down, as an index into `SCAN_BUTTONS`.
    pub lit: Option<usize>,
    /// How many buttons the scan has heard so far.
    pub heard: usize,
    /// "Done" is pressed (a pulse from 0 to 1 and back).
    pub done: f32,
    /// The scan has been finished and saved.
    pub saved: bool,
    /// How visible the list is: it fades at the end of the loop, so the next one starts empty.
    pub shown: f32,
}

/// The scan demo at `t` seconds.
pub(super) fn scan_frame(t: f32) -> ScanFrame {
    let t = t.rem_euclid(SCAN_LOOP);
    ScanFrame {
        lit: SCAN_PRESSES
            .iter()
            .position(|&at| (at..at + 0.4).contains(&t)),
        heard: SCAN_PRESSES.iter().filter(|&&at| t >= at).count(),
        done: ramp(t, SCAN_DONE, 0.15) - ramp(t, SCAN_DONE + 0.15, 0.25),
        saved: t >= SCAN_DONE,
        shown: 1.0 - ramp(t, SCAN_LOOP - 0.4, 0.4),
    }
}

/// What the scan demo says about the moment it shows.
fn scan_caption(f: &ScanFrame) -> String {
    if f.saved {
        format!("Saved: this mouse has {} extra buttons", f.heard)
    } else {
        "Press every extra button once".to_string()
    }
}

/// A mouse with two buttons on its left edge (forward above back, as on a real mouse), the one
/// that is down lit. They sit against the body, a little over its outline.
fn scan_mouse(p: &egui::Painter, center: Pos2, lit: Option<usize>, accent: Color32) {
    mouse(p, center, false, accent);
    let body = Rect::from_center_size(center, MOUSE_SIZE);
    // index 0 is the back button, which is the lower one
    for (i, dy) in [(0, 14.0), (1, -4.0)] {
        let tab = Rect::from_center_size(
            Pos2::new(body.min.x - 1.0, center.y + dy),
            Vec2::new(7.0, 15.0),
        );
        let on = lit == Some(i);
        p.rect_filled(tab, 3.0, if on { accent } else { pal().track });
        p.rect_stroke(tab, 3.0, Stroke::new(1.0, pal().border), StrokeKind::Inside);
    }
}

/// The scan of the mouse's buttons: each extra button is pressed once, heard and listed, then Done
/// saves them.
pub(in crate::ui::settings) fn scan_demo(ui: &mut egui::Ui, accent: Color32) {
    let (p, rect, t) = stage(ui);
    let f = scan_frame(t);
    scan_mouse(
        &p,
        Pos2::new(rect.min.x + rect.width() * 0.3, rect.min.y + PIE_Y),
        f.lit,
        accent,
    );
    // what has been heard
    let x = rect.min.x + rect.width() * 0.46;
    for (i, &button) in SCAN_BUTTONS.iter().enumerate() {
        let chip = Rect::from_min_size(
            Pos2::new(x, rect.min.y + 42.0 + 34.0 * i as f32),
            Vec2::new(180.0, 26.0),
        );
        if i < f.heard {
            p.rect_filled(chip, 13.0, pal().idle.gamma_multiply(f.shown));
            p.circle_filled(
                Pos2::new(chip.min.x + 13.0, chip.center().y),
                4.0,
                accent.gamma_multiply(f.shown),
            );
            p.text(
                Pos2::new(chip.min.x + 26.0, chip.center().y),
                Align2::LEFT_CENTER,
                crate::mouse::button_label(button),
                FontId::proportional(12.0),
                pal().text.gamma_multiply(f.shown),
            );
        } else {
            p.rect_stroke(
                chip,
                13.0,
                Stroke::new(1.0, pal().border.gamma_multiply(f.shown)),
                StrokeKind::Inside,
            );
        }
    }
    let pill = Rect::from_center_size(
        Pos2::new(x + 40.0, rect.min.y + 126.0),
        Vec2::new(80.0, 24.0),
    );
    p.rect_filled(pill, 12.0, pal().idle.lerp_to_gamma(accent, f.done));
    p.text(
        pill.center(),
        Align2::CENTER_CENTER,
        "Done",
        FontId::proportional(11.0),
        pal().text,
    );
    caption(&p, rect, &scan_caption(&f));
    ui.add_space(GAP);
}

// ------------------------------------------------------------------ painting

/// What every demo starts with: some room, its painter and the seconds of the loop. The next frame
/// is asked for only while the room is on screen.
fn stage(ui: &mut egui::Ui) -> (egui::Painter, Rect, f32) {
    ui.add_space(GAP);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), STAGE_H),
        egui::Sense::hover(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, control_r(), pal().input);
    if ui.is_rect_visible(rect) {
        ui.ctx().request_repaint_after(FRAME);
    }
    let t = ui.ctx().input(|i| i.time % 3600.0) as f32;
    (painter, rect, t)
}

/// A line of text at the bottom of the stage.
fn caption(p: &egui::Painter, rect: Rect, text: &str) {
    caption_at(p, rect.max.y - 18.0, rect, text);
}

/// A line of text centered on the stage, at height `y`.
fn caption_at(p: &egui::Painter, y: f32, rect: Rect, text: &str) {
    p.text(
        Pos2::new(rect.center().x, y),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(12.0),
        pal().muted,
    );
}

/// The size of the mouse the demos draw.
const MOUSE_SIZE: Vec2 = Vec2::new(34.0, 54.0);

/// A mouse seen from above, its middle button lit while it is down.
fn mouse(p: &egui::Painter, center: Pos2, button: bool, accent: Color32) {
    let body = Rect::from_center_size(center, MOUSE_SIZE);
    p.rect_filled(body, 17.0, pal().idle);
    p.rect_stroke(
        body,
        17.0,
        Stroke::new(1.5, pal().border),
        StrokeKind::Inside,
    );
    let wheel =
        Rect::from_center_size(Pos2::new(center.x, body.min.y + 16.0), Vec2::new(7.0, 14.0));
    p.rect_filled(wheel, 3.0, if button { accent } else { pal().track });
}

/// A pointer, its tip at `at`.
fn cursor(p: &egui::Painter, at: Pos2) {
    let points = vec![at, at + Vec2::new(0.0, 15.0), at + Vec2::new(10.5, 10.5)];
    p.add(egui::Shape::convex_polygon(
        points,
        pal().text,
        Stroke::new(1.0, pal().card),
    ));
}

/// A small window with a title bar whose dot shows whether it has the keyboard focus.
fn mini_window(p: &egui::Painter, rect: Rect, title: &str, focus: f32, accent: Color32) {
    p.rect_filled(rect, 8.0, pal().card);
    p.rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, pal().border),
        StrokeKind::Inside,
    );
    let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 16.0));
    let top = CornerRadius {
        nw: 8,
        ne: 8,
        sw: 0,
        se: 0,
    };
    p.rect_filled(bar, top, pal().idle);
    p.circle_filled(
        Pos2::new(bar.min.x + 10.0, bar.center().y),
        3.0,
        pal().track.lerp_to_gamma(accent, focus.clamp(0.0, 1.0)),
    );
    p.text(
        Pos2::new(bar.min.x + 20.0, bar.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(10.0),
        pal().muted,
    );
    // lines of "content", as many as the window has room for
    for i in 0..8 {
        let y = bar.max.y + 9.0 + 9.0 * i as f32;
        if y + 8.0 < rect.max.y {
            let line = Rect::from_min_size(
                Pos2::new(rect.min.x + 8.0, y),
                Vec2::new(rect.width() * (0.8 - 0.2 * (i % 3) as f32) - 16.0, 4.0),
            );
            p.rect_filled(line, 2.0, pal().divider);
        }
    }
}

/// A pie of four slices, `open` from 0 (nothing) to 1. `app` marks the pie of an app (an accent
/// ring and a badge in the middle); `progress` is the "hold for settings" arc around the center.
fn mini_pie(
    p: &egui::Painter,
    c: Pos2,
    r: f32,
    open: f32,
    accent: Color32,
    app: bool,
    progress: f32,
) {
    if open <= 0.01 {
        return;
    }
    let inner = r * 0.4;
    let outer = inner + (r - inner) * ease_out_cubic(open);
    let colors = pie_colors();
    let quarter = TAU / 4.0;
    for i in 0..4 {
        let a0 = i as f32 * quarter + 0.05;
        let a1 = a0 + quarter - 0.1;
        p.add(ring_slice(
            p,
            c,
            inner,
            outer,
            a0,
            a1,
            rgb(colors.slice).gamma_multiply(open),
            Bend::NONE,
        ));
        let dot = c + Vec2::angled((a0 + a1) / 2.0) * ((inner + outer) / 2.0);
        p.circle_filled(dot, r * 0.07, colors.ink.gamma_multiply(open * 0.8));
    }
    let (ring, width) = if app {
        (accent, 2.0)
    } else {
        (pal().border, 1.0)
    };
    p.circle_stroke(c, outer, Stroke::new(width, ring.gamma_multiply(open)));
    p.circle_filled(c, inner * 0.86, colors.core.gamma_multiply(open));
    if app {
        let badge = Rect::from_center_size(c, Vec2::splat(inner * 0.9));
        p.rect_filled(badge, 4.0, accent.gamma_multiply(open));
    }
    if progress > 0.01 {
        let a0 = -FRAC_PI_2;
        let a1 = a0 + TAU * progress.min(0.999);
        let r0 = inner * 0.86;
        p.add(ring_slice(
            p,
            c,
            r0,
            r0 + 3.0,
            a0,
            a1,
            accent.gamma_multiply(open),
            Bend::NONE,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PieStyle;

    #[test]
    fn the_trail_demo_opens_a_submenu_leaves_a_node_and_comes_back() {
        let shape = TrailShape::default();
        let at = |t: f32| trail_frame(t, shape, 1.0);
        let start = at(0.0);
        assert!(start.nodes.is_empty() && start.bloom == 0.0 && start.step == 0);
        assert!(at(0.9).nodes.is_empty(), "the first pie has no trail");
        // one level in: one node, then two, then the way back leaves one again
        assert_eq!(at(TRAIL_AT_1 + 1.0).nodes.len(), 1);
        assert_eq!(at(TRAIL_AT_2 + 1.0).nodes.len(), 2);
        assert_eq!(at(TRAIL_AT_3 + 1.0).nodes.len(), 1);
        assert_eq!(
            [0.5, 2.0, 4.0, 6.0].map(|t| at(t).step),
            [0, 1, 2, 3],
            "the parts of the story, in order"
        );
        // every level blooms again, at the moment it opens
        for t in [TRAIL_AT_1, TRAIL_AT_2, TRAIL_AT_3] {
            assert!(
                at(t + 0.01).bloom < 0.3,
                "a new ring starts to bloom at {t}"
            );
            assert!(at(t + TRAIL_BLOOM + 0.05).bloom > 0.99);
        }
        // it ends faded out, so the loop restarts from nothing
        let end = at(TRAIL_LOOP - 0.01);
        assert!(end.fade < 0.01, "{}", end.fade);
    }

    #[test]
    fn the_trail_demo_follows_the_shape_of_the_settings() {
        let near = TrailShape::from_percent(150, 20, 78, 38);
        let far = TrailShape::from_percent(240, 40, 78, 38);
        let t = TRAIL_AT_2 + 1.0; // settled (the glide is over), two nodes, before the way back opens
        let (a, b) = (trail_frame(t, near, 1.0), trail_frame(t, far, 1.0));
        assert!(b.nodes[0].at.length() > a.nodes[0].at.length(), "farther");
        assert!(b.nodes[0].r > a.nodes[0].r, "bigger");
        // settled nodes are at the real distance: reach outer radii from the center
        assert!((a.nodes[0].at.length() - near.reach * TRAIL_R).abs() < 0.5);
        // and each older one is smaller
        assert!(a.nodes[1].r < a.nodes[0].r);
    }

    #[test]
    fn the_nodes_of_the_demo_glide_and_a_faster_glide_arrives_sooner() {
        let shape = TrailShape::default();
        let t = TRAIL_AT_1 + 0.1;
        let slow = trail_frame(t, shape, 0.4);
        let fast = trail_frame(t, shape, 3.0);
        let reach = shape.reach * TRAIL_R;
        let (s, f) = (slow.nodes[0].at.length(), fast.nodes[0].at.length());
        assert!(
            (f - reach).abs() < (s - reach).abs(),
            "the fast one is nearer its place: {f} {s}"
        );
        let settled = trail_frame(TRAIL_AT_1 + 1.0, shape, 0.4);
        assert!((settled.nodes[0].at.length() - reach).abs() < 0.5);
    }

    #[test]
    fn the_trail_demo_loops_without_a_jump_and_every_frame_is_finite() {
        let shape = TrailShape::default();
        for i in 0..=800 {
            let t = i as f32 * 0.01;
            let f = trail_frame(t, shape, 1.0);
            assert!(f.pointer.x.is_finite() && f.pointer.y.is_finite());
            assert!(f.nodes.iter().all(|n| n.at.x.is_finite() && n.r > 0.0));
        }
        let (a, b) = (
            trail_frame(2.0, shape, 1.0),
            trail_frame(2.0 + TRAIL_LOOP, shape, 1.0),
        );
        assert_eq!(a, b);
        // the pointer is where the next ring opens when the level changes (no jump)
        let before = trail_frame(TRAIL_AT_1 - 0.001, shape, 1.0).pointer;
        assert!((before.x - 1.1 * TRAIL_R).abs() < 0.5 && before.y.abs() < 0.5);
        let after = trail_frame(TRAIL_AT_1 + 0.001, shape, 1.0).pointer;
        assert!(after.length() < 0.5, "centered on the new ring: {after:?}");
    }

    #[test]
    fn the_trail_captions_tell_the_four_parts_apart() {
        let all: Vec<&str> = (0..4).map(trail_caption).collect();
        for (i, a) in all.iter().enumerate() {
            assert!(!a.is_empty());
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn the_icon_flies_into_the_pie_and_a_silhouette_loses_its_colors() {
        let glyph = app_icon_frame(2.5, AppIconStyle::Glyph);
        assert!(glyph.pie > 0.99 && glyph.fly > 0.99 && glyph.icon > 0.99);
        assert!(glyph.blend > 0.99, "a silhouette ends up one color");
        let color = app_icon_frame(2.5, AppIconStyle::Color);
        assert_eq!(color.blend, 0.0, "full color keeps its colors");
        assert_eq!((color.pie, color.fly), (glyph.pie, glyph.fly));
        let early = app_icon_frame(0.5, AppIconStyle::Glyph);
        assert_eq!(
            (early.icon, early.blend),
            (0.0, 0.0),
            "nothing has set off yet"
        );
        let mid = app_icon_frame(1.3, AppIconStyle::Glyph);
        assert!(mid.fly > 0.0 && mid.fly < 1.0, "on its way");
        assert_eq!(mid.blend, 0.0, "still in its colors until it arrives");
    }

    #[test]
    fn the_app_icon_loop_ends_with_the_pie_closed_and_repeats() {
        for style in [AppIconStyle::Glyph, AppIconStyle::Color] {
            let end = app_icon_frame(ICON_LOOP - 0.01, style);
            assert!(end.pie.abs() < 1e-6 && end.icon.abs() < 1e-6, "{style:?}");
            for t in [0.3, 1.2, 2.2, 3.5] {
                let a = app_icon_frame(t, style);
                let b = app_icon_frame(t + ICON_LOOP, style);
                assert!((a.pie - b.pie).abs() < 1e-3 && (a.fly - b.fly).abs() < 1e-3);
            }
        }
    }

    #[test]
    fn the_app_icon_caption_names_the_style() {
        assert!(app_icon_caption(AppIconStyle::Glyph, true).contains("silhouette"));
        assert!(app_icon_caption(AppIconStyle::Color, true).contains("own icon"));
        assert!(app_icon_caption(AppIconStyle::Color, false).starts_with("Off"));
    }

    #[test]
    fn the_hold_waits_then_opens_the_pie_then_runs_the_arc() {
        let at = |t: f32| hold_frame(t, 500, 1200, false);
        assert_eq!(at(0.2), HoldFrame::default(), "nothing before the press");
        let early = at(HOLD_REST + 0.3);
        assert!(early.button && early.pie == 0.0, "held, pie not open yet");
        let open = at(HOLD_REST + 0.5 + OPEN_SECS + 0.01);
        assert_eq!(open.pie, 1.0);
        assert!(open.settings < 1.0);
        let done = at(HOLD_REST + 1.2 + 0.05);
        assert_eq!(done.settings, 1.0);
        let gone = at(HOLD_REST + 1.2 + HOLD_LINGER + HOLD_FADE + 0.01);
        assert_eq!(gone.pie, 0.0, "let go: the pie has closed");
        assert!(!gone.button);
    }

    #[test]
    fn a_longer_hold_time_opens_the_pie_later() {
        let slow = hold_frame(HOLD_REST + 0.6, 1000, 2000, false);
        let fast = hold_frame(HOLD_REST + 0.6, 300, 2000, false);
        assert_eq!(slow.pie, 0.0);
        assert_eq!(fast.pie, 1.0);
    }

    #[test]
    fn dragging_opens_the_pie_at_once() {
        let t = HOLD_REST + FLICK_OPENS + OPEN_SECS + 0.01;
        assert_eq!(hold_frame(t, 1500, 3000, true).pie, 1.0);
        assert_eq!(hold_frame(t, 1500, 3000, false).pie, 0.0);
        assert_eq!(hold_frame(t, 1500, 3000, true).drag, 1.0);
        assert_eq!(hold_frame(t, 1500, 3000, false).drag, 0.0);
    }

    #[test]
    fn the_arc_only_grows_while_the_button_is_down() {
        let mut last = 0.0;
        for step in 0..200 {
            let f = hold_frame(HOLD_REST + step as f32 * 0.01, 500, 1200, false);
            if f.button {
                assert!(f.settings >= last, "the arc went back at step {step}");
                last = f.settings;
            }
        }
        assert!(last > 0.9);
    }

    #[test]
    fn a_settings_time_below_the_pie_time_is_stretched_not_divided_by_zero() {
        assert_eq!(hold_span_ms(1500, 500), 1600.0);
        for t in [0.0, 1.0, 2.0, 3.0] {
            let f = hold_frame(t, 1500, 500, false);
            assert!(f.pie.is_finite() && f.settings.is_finite() && f.held_ms.is_finite());
        }
    }

    #[test]
    fn the_hold_loop_repeats() {
        let total = HOLD_REST + 1.2 + HOLD_LINGER + HOLD_FADE;
        for t in [0.3, 1.0, 1.7] {
            let (a, b) = (
                hold_frame(t, 500, 1200, false),
                hold_frame(t + total, 500, 1200, false),
            );
            assert!((a.pie - b.pie).abs() < 1e-3 && a.button == b.button);
        }
    }

    #[test]
    fn a_hold_opens_the_general_pie_and_a_double_click_the_apps() {
        let hold = gesture_frame(1.5, AppGesture::DoubleClick, 260);
        assert!(hold.pie > 0.9 && !hold.app_pie && hold.taps == 0);
        let double = gesture_frame(4.5, AppGesture::DoubleClick, 260);
        assert!(double.pie > 0.9 && double.app_pie && double.taps == 2);
        let first = gesture_frame(FIRST_CLICK + 0.05, AppGesture::DoubleClick, 260);
        assert!(first.button && first.taps == 1 && first.pie == 0.0);
        let classic = gesture_frame(1.5, AppGesture::Merged, 260);
        assert!(classic.pie > 0.9 && classic.app_pie && classic.taps == 0);
    }

    #[test]
    fn every_scene_ends_with_the_pie_closed() {
        for t in [2.9, 6.5] {
            assert_eq!(
                gesture_frame(t, AppGesture::DoubleClick, 500).pie,
                0.0,
                "{t}"
            );
        }
        assert_eq!(gesture_frame(2.9, AppGesture::Merged, 260).pie, 0.0);
    }

    #[test]
    fn the_captions_say_which_pie_opens() {
        let f = |app_pie| GestureFrame {
            app_pie,
            ..GestureFrame::default()
        };
        assert!(gesture_caption(AppGesture::DoubleClick, &f(false)).contains("general"));
        assert!(gesture_caption(AppGesture::DoubleClick, &f(true)).contains("Double-click"));
        assert!(gesture_caption(AppGesture::Merged, &f(true)).contains("own pie"));
    }

    #[test]
    fn the_pie_works_on_the_window_the_setting_names() {
        let under = target_frame(2.0, TargetApp::UnderCursor, true);
        let front = target_frame(2.0, TargetApp::Focused, true);
        assert!(under.target_right && !front.target_right);
        assert_eq!(
            under.cursor, 1.0,
            "the pointer has crossed to the other window"
        );
        assert_eq!(under.pie, 1.0);
        assert_eq!(target_frame(0.0, TargetApp::Focused, false).cursor, 0.0);
    }

    #[test]
    fn only_the_window_under_the_pointer_comes_to_the_front_and_only_when_asked() {
        let at = 4.0;
        assert_eq!(
            target_frame(at, TargetApp::UnderCursor, true).front_right,
            1.0
        );
        assert_eq!(
            target_frame(at, TargetApp::UnderCursor, false).front_right,
            0.0
        );
        assert_eq!(target_frame(at, TargetApp::Focused, true).front_right, 0.0);
        // and it is back where it was before the loop starts again
        assert_eq!(
            target_frame(4.95, TargetApp::UnderCursor, true).front_right,
            0.0
        );
    }

    #[test]
    fn each_desktop_mode_changes_one_thing() {
        let t = 2.0;
        let switch = desktop_frame(t, DesktopMode::SwitchToApp);
        assert!(switch.view > 0.9 && switch.moved == 0.0 && switch.copy == 0.0);
        let bring = desktop_frame(t, DesktopMode::BringToCurrent);
        assert!(bring.moved > 0.9 && bring.view == 0.0 && bring.copy == 0.0);
        let nothing = desktop_frame(t, DesktopMode::DoNothing);
        assert!(nothing.copy > 0.9 && nothing.view == 0.0 && nothing.moved == 0.0);
    }

    #[test]
    fn the_desktop_loop_starts_and_ends_at_rest() {
        for mode in [
            DesktopMode::SwitchToApp,
            DesktopMode::BringToCurrent,
            DesktopMode::DoNothing,
        ] {
            let end = desktop_frame(DESKTOP_LOOP - 0.01, mode);
            assert_eq!(end, DesktopFrame::default(), "{mode:?} must end at rest");
            assert_eq!(desktop_frame(0.0, mode), DesktopFrame::default());
        }
        let pulse = desktop_frame(0.75, DesktopMode::DoNothing).click;
        assert!(pulse > 0.99, "the slice is pressed at 0.75 s");
        for step in 0..440 {
            let click = desktop_frame(step as f32 * 0.01, DesktopMode::SwitchToApp).click;
            assert!((0.0..=1.0).contains(&click), "click {click} at step {step}");
        }
    }

    #[test]
    fn the_desktop_caption_names_what_the_system_calls_a_desktop() {
        assert!(desktop_caption(DesktopMode::SwitchToApp, "Space").contains("Space"));
        assert!(desktop_caption(DesktopMode::BringToCurrent, "Space").contains("window"));
        assert!(desktop_caption(DesktopMode::DoNothing, "Space").contains("here"));
        assert_eq!(capitalized("workspace"), "Workspace");
        assert_eq!(capitalized("virtual desktop"), "Virtual desktop");
        assert_eq!(capitalized(""), "");
    }

    #[test]
    fn the_pointer_visits_every_slice_and_the_loop_ends_at_rest() {
        let mut seen = [false; 4];
        for step in 0..440 {
            let f = look_frame(step as f32 * 0.01);
            assert!(
                (0.0..=1.0).contains(&f.hover),
                "hover {} at {step}",
                f.hover
            );
            if f.hover > 0.99 {
                seen[f.slice] = true;
            }
        }
        assert_eq!(seen, [true; 4]);
        assert_eq!(look_frame(0.0).hover, 0.0);
        assert!(look_frame(LOOK_STEP * 4.0 - 0.001).hover < 0.01);
    }

    #[test]
    fn a_tile_pie_comes_out_is_passed_over_by_the_pointer_and_goes_away() {
        let (open, f) = tile_frame(0.0);
        assert_eq!((open, f.hover), (0.0, 0.0), "a round starts closed");
        let mut seen = [false; 4];
        for step in 0..(TILE_LOOP * 100.0) as usize {
            let (open, f) = tile_frame(step as f32 * 0.01);
            assert!((0.0..=1.0).contains(&open), "open {open} at {step}");
            assert!(
                open >= 0.99 || f.hover == 0.0,
                "the pointer is on a closed pie"
            );
            if f.hover > 0.99 {
                seen[f.slice] = true;
            }
        }
        assert_eq!(seen, [true; 4]);
        assert_eq!(
            tile_frame(TILE_LOOP - 0.001).0.round(),
            0.0,
            "and ends closed"
        );
    }

    #[test]
    fn a_bigger_pie_setting_draws_a_bigger_demo_pie_within_the_stage() {
        assert!(look_radius(360.0) > look_radius(200.0));
        assert!(look_radius(200.0) > look_radius(140.0));
        assert_eq!(look_radius(10.0), look_radius(140.0), "below the range");
        assert_eq!(look_radius(900.0), look_radius(360.0), "above the range");
        // the biggest pie and its names still fit above the caption
        assert!(PIE_Y + look_radius(360.0) < STAGE_H - 24.0);
        assert!(PIE_Y - look_radius(360.0) > 0.0);
    }

    #[test]
    fn the_look_caption_says_the_size_and_whether_there_are_names() {
        assert_eq!(look_caption(220.0, true), "220 px, icons and names");
        assert_eq!(look_caption(220.4, false), "220 px, icons only");
    }

    #[test]
    fn the_side_buttons_fire_the_pie_they_follow() {
        let app = side_frame(0.9, SideTarget::App, false);
        assert!(app.follow == 0.0 && app.fire > 0.0, "{app:?}");
        let general = side_frame(0.9, SideTarget::General, false);
        assert!(general.follow == 1.0 && general.fire > 0.0, "{general:?}");
        assert!(side_frame(0.7, SideTarget::App, false).side_button);
        assert!(!side_frame(0.3, SideTarget::App, false).side_button);
    }

    #[test]
    fn a_right_click_moves_the_buttons_to_the_other_pie_only_when_it_is_on() {
        let after = 3.5;
        assert_eq!(side_frame(after, SideTarget::App, true).follow, 1.0);
        assert_eq!(side_frame(after, SideTarget::General, true).follow, 0.0);
        assert_eq!(side_frame(after, SideTarget::App, false).follow, 0.0);
        assert!(side_frame(2.7, SideTarget::App, true).right_button);
        assert!(!side_frame(2.7, SideTarget::App, false).right_button);
        // the second press runs the other pie, and only when the switch is on
        assert!(side_frame(4.0, SideTarget::App, true).fire > 0.0);
        assert_eq!(side_frame(4.0, SideTarget::App, false).fire, 0.0);
    }

    #[test]
    fn the_side_loop_starts_and_ends_at_rest() {
        for default in [SideTarget::App, SideTarget::General] {
            for switch in [false, true] {
                let base = if default == SideTarget::General {
                    1.0
                } else {
                    0.0
                };
                for t in [0.0, SIDE_LOOP - 0.01] {
                    let f = side_frame(t, default, switch);
                    assert!(
                        (f.follow - base).abs() < 0.01,
                        "{default:?} {switch} {t}: {f:?}"
                    );
                    assert!(!f.side_button && !f.right_button && f.fire == 0.0);
                }
                for step in 0..600 {
                    let f = side_frame(step as f32 * 0.01, default, switch);
                    assert!((0.0..=1.0).contains(&f.follow), "{f:?}");
                    assert!((0.0..=1.0).contains(&f.fire), "{f:?}");
                }
            }
        }
        assert!(side_caption(&side_frame(3.5, SideTarget::App, true)).contains("general"));
        assert!(side_caption(&side_frame(0.0, SideTarget::App, true)).contains("app's"));
        assert!(side_caption(&side_frame(2.7, SideTarget::App, true)).contains("Right-click"));
    }

    #[test]
    fn the_idle_memory_is_freed_after_the_timer_and_loaded_again_by_the_trigger() {
        let at = |t: f32| memory_frame(t, true);
        assert_eq!(
            (at(0.0).used, at(0.0).idle),
            (1.0, 0.0),
            "in use at the start"
        );
        let counting = at(2.0);
        assert!(counting.idle > 0.0 && counting.idle < 1.0 && counting.used == 1.0);
        let freed = at(MEMORY_FREED + 0.5);
        assert!(freed.idle == 1.0 && freed.used < 0.2, "{freed:?}");
        let pressed = at(MEMORY_PRESS + 0.1);
        assert!(pressed.pressed && pressed.reload && pressed.idle == 0.0);
        assert_eq!(
            at(MEMORY_LOOP - 0.01).used.round(),
            1.0,
            "full again at the end"
        );
        assert!(!at(MEMORY_LOOP - 0.01).pressed);
    }

    #[test]
    fn nothing_is_freed_while_the_setting_is_off() {
        for step in 0..640 {
            let f = memory_frame(step as f32 * 0.01, false);
            assert_eq!(
                f,
                MemoryFrame {
                    used: 1.0,
                    ..MemoryFrame::default()
                }
            );
        }
        assert!(memory_caption(false, &memory_frame(4.0, false)).contains("all the time"));
        assert!(memory_caption(true, &memory_frame(4.0, true)).contains("Freed"));
        assert!(memory_caption(true, &memory_frame(5.0, true)).contains("trigger"));
        assert_eq!(memory_caption(true, &memory_frame(0.0, true)), "In use");
    }

    #[test]
    fn the_scan_hears_each_button_once_in_order_and_then_saves() {
        assert_eq!(
            scan_frame(0.2),
            ScanFrame {
                shown: 1.0,
                ..ScanFrame::default()
            }
        );
        let first = scan_frame(SCAN_PRESSES[0] + 0.1);
        assert!(first.lit == Some(0) && first.heard == 1 && !first.saved);
        let second = scan_frame(SCAN_PRESSES[1] + 0.1);
        assert!(second.lit == Some(1) && second.heard == 2);
        let between = scan_frame(SCAN_PRESSES[0] + 0.6);
        assert!(between.lit.is_none() && between.heard == 1, "{between:?}");
        let done = scan_frame(SCAN_DONE + 0.15);
        assert!(done.saved && done.done > 0.99 && done.heard == 2);
        assert_eq!(scan_caption(&done), "Saved: this mouse has 2 extra buttons");
        assert!(scan_caption(&first).contains("Press"));
    }

    #[test]
    fn the_scan_list_fades_out_before_the_loop_starts_again() {
        assert_eq!(scan_frame(SCAN_LOOP - 0.001).heard, 2);
        assert!(
            scan_frame(SCAN_LOOP - 0.001).shown < 0.01,
            "faded before it empties"
        );
        assert_eq!(scan_frame(SCAN_LOOP + 0.2).heard, 0);
        for step in 0..600 {
            let f = scan_frame(step as f32 * 0.01);
            assert!((0.0..=1.0).contains(&f.done), "{f:?}");
            assert!((0.0..=1.0).contains(&f.shown), "{f:?}");
        }
    }

    #[test]
    fn every_demo_draws_without_panicking() {
        let ctx = egui::Context::default();
        crate::ui::fonts::install(&ctx);
        let accent = Color32::from_rgb(88, 86, 214);
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 1600.0))),
                ..Default::default()
            };
            // nothing renders the output, so the texture uploads are dropped on purpose
            ctx.run_ui(input, |ui| {
                hold_demo(ui, accent, 500, 1200, false);
                hold_demo(ui, accent, 1500, 500, true);
                gesture_demo(ui, accent, AppGesture::DoubleClick, 260);
                gesture_demo(ui, accent, AppGesture::Merged, 260);
                target_demo(ui, accent, TargetApp::UnderCursor, true);
                target_demo(ui, accent, TargetApp::Focused, false);
                for mode in [
                    DesktopMode::SwitchToApp,
                    DesktopMode::BringToCurrent,
                    DesktopMode::DoNothing,
                ] {
                    desktop_demo(ui, accent, mode, "workspace");
                }
                let cfg = crate::config::Config::default();
                let pie = |material, outline| LookPie {
                    accent,
                    labels: true,
                    center_icon: true,
                    app_icon: AppIconStyle::Glyph,
                    material,
                    outline,
                };
                let square = Outline::from_settings(true, 4, 45, 0);
                let triangle = Outline::from_settings(true, 3, 0, 90);
                look_demo(ui, pie(Material::Solid, Outline::CIRCLE), 140.0);
                look_demo(
                    ui,
                    pie(Material::styled(PieStyle::Glass, &cfg), square),
                    360.0,
                );
                look_demo(ui, pie(Material::GenZ, triangle), 220.0);
                let tiles = [PieStyle::Solid, PieStyle::Glass, PieStyle::GenZ].map(|style| Tile {
                    name: "style",
                    hint: "",
                    material: Material::styled(style, &cfg),
                    chosen: style == PieStyle::Glass,
                });
                tiles_demo(ui, "test", accent, square, &tiles);
                let other = Color32::from_rgb(255, 59, 48);
                side_demo(ui, accent, other, SideTarget::App, true, true);
                side_demo(ui, accent, other, SideTarget::General, false, false);
                memory_demo(ui, accent, true, 60);
                memory_demo(ui, accent, false, 0);
                app_icon_demo(ui, accent, AppIconStyle::Glyph, true);
                app_icon_demo(ui, accent, AppIconStyle::Color, true);
                app_icon_demo(ui, accent, AppIconStyle::Color, false);
                scan_demo(ui, accent);
            })
            .drop_without_applying_deltas();
        }
    }

    #[test]
    fn the_tiles_of_two_demos_never_share_an_id() {
        let mut ids = std::collections::HashSet::new();
        for salt in ["material", "shape"] {
            for i in 0..3 {
                assert!(ids.insert(tile_id(salt, i)), "{salt} {i}");
            }
        }
        assert_eq!(
            ids.len(),
            6,
            "the first tile of each demo has an id of its own"
        );
    }

    #[test]
    fn the_choice_demo_brings_out_the_badges_picks_one_and_shows_the_result() {
        let at = |t: f32, on: bool| choice_frame(t, on);
        assert_eq!(at(0.0, true).pie, 0.0, "nothing before the pie opens");
        assert!(at(1.0, true).pie > 0.99 && at(1.0, true).badges < 0.1);
        let out = at(2.0, true);
        assert!(
            out.badges > 0.99 && out.hover > 0.99,
            "badges are out while hovered"
        );
        let picked = at(2.6, true);
        assert!(picked.pick > 0.99);
        assert!(picked.new_window, "the first half picks the new window");
        assert!(at(CHOICE_HALF + 2.6, true).pick > 0.99);
        assert!(
            !at(CHOICE_HALF + 2.6, true).new_window,
            "the second half picks the open app"
        );
        assert!(at(3.8, true).result > 0.99, "then the window answers");
        assert!(at(CHOICE_HALF - 0.1, true).result < 0.2 && at(CHOICE_HALF - 0.1, true).pie < 0.01);
    }

    #[test]
    fn the_choice_demo_offers_nothing_when_the_setting_is_off() {
        for t in [0.5, 1.5, 2.5, 3.5, 6.0, 8.0] {
            let f = choice_frame(t, false);
            assert_eq!((f.badges, f.pick), (0.0, 0.0), "t={t}");
            assert!(!f.new_window);
        }
        assert!(choice_caption(false, &choice_frame(2.0, false)).starts_with("Off"));
    }

    #[test]
    fn the_choice_demo_badges_sit_outside_the_pie_and_apart() {
        let (a, b) = (choice_badge_at(true), choice_badge_at(false));
        for v in [a, b] {
            assert!(
                v.length() > CHOICE_R + CHOICE_BADGE / 2.0,
                "outside the rim"
            );
        }
        let gap = (a - b).length() - CHOICE_BADGE;
        assert!(
            gap > 0.0 && gap < CHOICE_BADGE,
            "close but not touching: {gap}"
        );
    }

    #[test]
    fn the_choice_demo_loop_repeats() {
        for t in [0.7, 2.1, 3.5, 6.2] {
            let (a, b) = (
                choice_frame(t, true),
                choice_frame(t + CHOICE_HALF * 2.0, true),
            );
            assert_eq!(a.new_window, b.new_window);
            for (x, y) in [
                (a.pie, b.pie),
                (a.badges, b.badges),
                (a.pick, b.pick),
                (a.result, b.result),
            ] {
                assert!((x - y).abs() < 1e-3, "t={t}: {x} vs {y}");
            }
        }
    }
}
