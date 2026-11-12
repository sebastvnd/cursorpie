//! The notice of what CursorPie is doing: a small pill on the screen that says "Open app  Safari",
//! "Shortcut  Cmd + C", "Run flow  Morning" while an action runs.
//!
//! The pill is drawn in the same transparent window as the pie and Settings (a third mode of it, see
//! [`guides::architecture`](crate::guides::architecture#one-window-for-everything)): the window is
//! made small, click-through and placed near the bottom edge of the display, and it hides again
//! when the notice is over. Nothing here runs actions or reads input; the worker of
//! [`actions`](crate::actions) describes what it is about to do ([`note_for`]) and sends it to the
//! UI as [`Msg::Hud`](crate::msg::Msg::Hud).
//!
//! | Part | Contents |
//! |---|---|
//! | [`HudNote`], [`HudKind`] | what is announced: a kind (which picks the icon) and one or two lines of text |
//! | [`note_for`] | turns an [`Action`] into a note (never shows a command: it may hold a secret) |
//! | [`paint_pill`] | draws the pill, in the colors of the current appearance and accent; Settings uses it for its preview too |
//! | [`Hud`] | the state of the notice on screen: its timing and animation |
//!
//! The pill follows the look of the pie: a card in the light or dark appearance, smoked graphite when
//! the pie is set to Glass, a round badge in the accent color with the icon of the kind of action.

use crate::config::{Action, Config, HUD_SIZE_MAX, HUD_SIZE_MIN, PieStyle, is_web_url};
use crate::platform::linux::roles::Role;
use crate::ui::draw::{ease_out_back_with, ease_out_cubic, on_accent, rgb, spring_out};
use crate::ui::fonts;
use crate::ui::settings::theme::{PILL, pal};
use crate::ui::symbols;
use eframe::egui::{self, Color32, FontId, Painter, Pos2, Rect, Vec2};
use std::time::{Duration, Instant};

/// Size of the window while it shows the notice at the normal size (points). The pill is centered in
/// it; the margin around the widest pill is where its shadow and the slide of the animation are
/// drawn. Use [`window_size`] for the size the person chose in Settings.
pub const WINDOW_SIZE: Vec2 = Vec2::new(560.0, 140.0);
/// Distance of the pill's center from the top or bottom edge of the display.
pub const EDGE_GAP: f32 = 112.0;
/// Distance of the pill's center below the pointer ([`HudPosition::Cursor`](crate::config::HudPosition)).
pub const CURSOR_GAP: f32 = 64.0;

/// The window for a notice drawn at `scale` (1.0 = normal, see [`scale_of`]).
pub fn window_size(scale: f32) -> Vec2 {
    WINDOW_SIZE * scale
}

/// The scale the person chose for the notice in Settings: 1.0 for 100%.
pub fn scale_of(cfg: &Config) -> f32 {
    cfg.hud_size.clamp(HUD_SIZE_MIN, HUD_SIZE_MAX) as f32 / 100.0
}

/// Height of the pill.
const PILL_H: f32 = 56.0;
/// Narrowest and widest the pill gets.
const PILL_MIN_W: f32 = 168.0;
const PILL_MAX_W: f32 = 480.0;
/// Diameter of the round badge with the icon.
const BADGE: f32 = 38.0;
/// Space between the edge of the pill and the badge, between badge and text, and after the text.
const PAD_L: f32 = 9.0;
const GAP: f32 = 13.0;
const PAD_R: f32 = 24.0;
/// Longest title and detail, in characters. A longer text is cut with an ellipsis.
const MAX_TITLE: usize = 26;
const MAX_DETAIL: usize = 38;

/// Seconds the notice takes to come in and to go away. It comes in as a round badge that rises from
/// below and bounces on a spring, then stretches into the pill (with a small overshoot) and slides
/// its text in; it goes away the other way round.
pub const IN_SECS: f32 = 0.72;
pub const OUT_SECS: f32 = 0.34;
/// Points the circle rises while it comes in (and sinks while it leaves).
const SLIDE: f32 = 48.0;
const SINK: f32 = 10.0;
/// How the time of coming in is shared: the circle rises during the first `RISE_END` of it, the
/// stretch starts at `GROW_START` (a little before the rise has ended, so the two run into each other).
const RISE_END: f32 = 0.55;
const GROW_START: f32 = 0.45;
/// Strength of the overshoot when the circle stretches into the pill (see `ease_out_back_with`).
const GROW_BACK: f32 = 1.2;
/// The pill may stretch a little past its full width while it bounces; the window has room for it.
const GROW_MAX: f32 = 1.08;
/// How far the text slides in from the left while it fades in (points).
const TEXT_SLIDE: f32 = 12.0;
/// How much the badge swells when the note changes (a damped bounce, see [`PillLook::pulse`]).
const PULSE_SWELL: f32 = 0.24;
/// Seconds the bounce after a change of the note takes.
const PULSE_SECS: f32 = 0.38;
/// How the time of going away is shared: the pill shrinks to a circle during the first `SHRINK_END`
/// of it, and the circle fades and sinks from `FADE_START` on.
const SHRINK_END: f32 = 0.6;
const FADE_START: f32 = 0.5;
/// How long a notice that has to wait for the pie to go away is remembered.
pub const PENDING_MAX: Duration = Duration::from_millis(1500);

/// What kind of thing is announced. It picks the icon of the badge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudKind {
    App,
    Link,
    File,
    Shortcut,
    Command,
    Browser,
    Flow,
}

impl HudKind {
    /// The glyph of the badge.
    pub fn glyph(self) -> char {
        match self {
            HudKind::App => symbols::APP_WINDOW,
            HudKind::Link => symbols::GLOBE,
            HudKind::File => symbols::FOLDER_OPEN,
            HudKind::Shortcut => symbols::KEYBOARD,
            HudKind::Command => symbols::TERMINAL_WINDOW,
            HudKind::Browser => symbols::BROWSER,
            HudKind::Flow => symbols::LIGHTNING,
        }
    }
}

/// One thing to announce: a short title ("Open app") and an optional line under it ("Safari").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HudNote {
    pub kind: HudKind,
    pub title: String,
    pub detail: String,
}

impl HudNote {
    pub fn new(kind: HudKind, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
        }
    }

    /// "Open app" with the app's name, for the blocks and steps that open an app.
    pub fn open_app(app: &str) -> Option<Self> {
        let app = app.trim();
        (!app.is_empty()).then(|| Self::new(HudKind::App, "Open app", app_display(app)))
    }

    /// "Open link" or "Open file" for a link or a path.
    pub fn open_target(target: &str) -> Option<Self> {
        let target = target.trim();
        if target.is_empty() {
            return None;
        }
        Some(if is_web_url(target) {
            Self::new(HudKind::Link, "Open link", host_of(target))
        } else if target.contains("://") || target.starts_with("mailto:") {
            Self::new(HudKind::Link, "Open link", scheme_of(target))
        } else {
            Self::new(HudKind::File, "Open file", file_name_of(target))
        })
    }

    /// The pill's two texts cut to what fits.
    fn clipped(&self) -> (String, String) {
        (clip(&self.title, MAX_TITLE), clip(&self.detail, MAX_DETAIL))
    }
}

/// What to announce for `action`, or `None` when it is not worth a notice: a pause, an empty
/// action, a sequence (its steps announce themselves one by one). `flow_name` looks up the name of
/// a flow by its id.
///
/// A shell command is announced as "Run command" and never with its text: it can hold a password
/// or a path the person does not want on a screen that may be shared.
pub fn note_for(action: &Action, flow_name: impl Fn(&str) -> Option<String>) -> Option<HudNote> {
    match action {
        Action::Nothing | Action::Wait(_) | Action::Sequence(_) => None,
        Action::Key(sc) => Some(HudNote::new(HudKind::Shortcut, "Shortcut", sc.label())),
        Action::OpenApp(app) => HudNote::open_app(app),
        Action::OpenUrl(target) => HudNote::open_target(target),
        Action::Command(cmd) => {
            (!cmd.trim().is_empty()).then(|| HudNote::new(HudKind::Command, "Run command", ""))
        }
        Action::Browser(spec) => {
            let name = spec.split_whitespace().next().unwrap_or("");
            (!name.is_empty()).then(|| HudNote::new(HudKind::Browser, "Browser", name))
        }
        // a flow that does not exist does nothing, so it says nothing
        Action::Flow(id) => flow_name(id).map(|name| HudNote::new(HudKind::Flow, "Run flow", name)),
    }
}

/// A readable name for what an "Open app" action names: `Safari` for `/Applications/Safari.app`,
/// `Chrome` for `chrome` or `com.google.Chrome`, `Browser` for the role `@browser`.
pub fn app_display(app: &str) -> String {
    let app = app.trim();
    if let Some(role) = Role::parse(app) {
        return match role {
            Role::Browser => "Browser",
            Role::Files => "Files",
            Role::Terminal => "Terminal",
        }
        .into();
    }
    let name = if app.contains('/') || app.contains('\\') {
        let file = app
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(app);
        strip_suffix_ci(strip_suffix_ci(file, ".app"), ".exe").to_string()
    } else {
        let id = strip_suffix_ci(app, ".desktop");
        // an id such as `com.google.Chrome` is named by its last part; a name with a space
        // ("Google Chrome") or a name that has no dot is kept as it is
        match id.rsplit('.').next() {
            Some(last) if id.contains('.') && !id.contains(' ') && !last.is_empty() => last,
            _ => id,
        }
        .to_string()
    };
    capitalize(&name)
}

fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> &'a str {
    match s.len().checked_sub(suffix.len()) {
        Some(at) if s.is_char_boundary(at) && s[at..].eq_ignore_ascii_case(suffix) => &s[..at],
        _ => s,
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `example.com` for `https://www.example.com/a/b?x=1`.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("");
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// `mailto` for `mailto:a@b.c` (the address stays off the screen).
fn scheme_of(target: &str) -> String {
    target
        .split([':', '/'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// The last part of a path, without the folders.
fn file_name_of(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// `text` cut to at most `max` characters, with an ellipsis if something was dropped.
fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// The state of the notice that is on screen.
#[derive(Debug)]
pub struct Hud {
    pub note: HudNote,
    /// How long it stays after it came in, or after the last change of the note.
    pub hold: Duration,
    /// When the first frame was drawn. `None` until the window is really on screen.
    pub shown_at: Option<Instant>,
    /// When it goes away; moved on by every change of the note.
    pub until: Option<Instant>,
    /// When the note last changed (a small pulse marks it).
    pub changed_at: Option<Instant>,
    /// How big it is drawn, 1.0 = normal. Fixed when the notice comes up (the window has that size).
    pub scale: f32,
}

impl Hud {
    pub fn new(note: HudNote, hold: Duration) -> Self {
        Self {
            note,
            hold,
            shown_at: None,
            until: None,
            changed_at: None,
            scale: 1.0,
        }
    }

    /// The same notice drawn `scale` times as big (see [`scale_of`]).
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Replaces the note while the pill is on screen and keeps it there for another `hold`.
    pub fn update(&mut self, note: HudNote, now: Instant) {
        if self.note != note {
            self.note = note;
            self.changed_at = Some(now);
        }
        if self.shown_at.is_some() {
            self.until = Some(now + self.hold);
        }
        // (a pill that was already on its way out comes back: `phase` derives everything from `until`)
    }

    /// The phase at `now`: how far the notice has come in (0 to 1), how far it has gone away
    /// (0 to 1), and whether it is over. Starts the clock on the first call. `hold` is the time the
    /// full pill stays, after it has come in and before it goes away.
    pub fn phase(&mut self, now: Instant) -> Phase {
        let shown = *self.shown_at.get_or_insert(now);
        let until = *self
            .until
            .get_or_insert(now + Duration::from_secs_f32(IN_SECS) + self.hold);
        let into = now.saturating_duration_since(shown).as_secs_f32();
        let late = now.saturating_duration_since(until).as_secs_f32();
        let coming = (into / IN_SECS).clamp(0.0, 1.0);
        let going = (late / OUT_SECS).clamp(0.0, 1.0);
        Phase {
            coming,
            going,
            over: going >= 1.0,
        }
    }

    /// 0 to 1: the little pulse after the note changed (1 = just changed).
    pub fn pulse(&self, now: Instant) -> f32 {
        self.changed_at.map_or(0.0, |t| {
            1.0 - (now.saturating_duration_since(t).as_secs_f32() / PULSE_SECS).clamp(0.0, 1.0)
        })
    }

    /// How long until the notice starts to go away (nothing else changes on its own before that).
    pub fn next_wake(&self, now: Instant) -> Option<Duration> {
        self.until.map(|u| u.saturating_duration_since(now))
    }
}

/// See [`Hud::phase`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Phase {
    pub coming: f32,
    pub going: f32,
    pub over: bool,
}

impl Phase {
    /// Whether an animation is running (the window needs a frame every tick).
    pub fn animating(&self) -> bool {
        self.coming < 1.0 || self.going > 0.0
    }

    /// Opacity of the pill, 0 to 1: it fades in quickly with the rise and out at the very end.
    pub fn alpha(&self) -> f32 {
        let come = ease_out_cubic((self.coming / 0.3).clamp(0.0, 1.0));
        let go = ease_out_cubic(((self.going - FADE_START) / (1.0 - FADE_START)).clamp(0.0, 1.0));
        (come * (1.0 - go)).clamp(0.0, 1.0)
    }

    /// Vertical offset of the pill in points: the circle comes up from below, overshoots its place
    /// and bounces on a spring until it rests; it sinks a little while it fades.
    pub fn offset(&self) -> f32 {
        let rise = spring_out((self.coming / RISE_END).clamp(0.0, 1.0));
        let sink = ease_out_cubic(((self.going - FADE_START) / (1.0 - FADE_START)).clamp(0.0, 1.0));
        (1.0 - rise) * SLIDE + sink * SINK
    }

    /// How far the circle has stretched into the pill, 0 (a circle) to 1 (the whole pill). While it
    /// comes in it overshoots 1 a little (up to `GROW_MAX`) and springs back, so the pill bounces.
    pub fn grow(&self) -> f32 {
        let out = ease_out_cubic((self.going / SHRINK_END).clamp(0.0, 1.0));
        let come = ease_out_back_with(
            ((self.coming - GROW_START) / (1.0 - GROW_START)).clamp(0.0, 1.0),
            GROW_BACK,
        );
        (come * (1.0 - out)).clamp(0.0, GROW_MAX)
    }

    /// Opacity of the text, 0 to 1: it shows once the pill is stretched enough to hold it.
    pub fn text(&self) -> f32 {
        ((self.grow() - 0.55) / 0.45).clamp(0.0, 1.0)
    }

    /// Scale of the badge, a pop: it starts small, swells past its size and settles at 1.
    pub fn pop(&self) -> f32 {
        let pop = spring_out(((self.coming - 0.05) / 0.55).clamp(0.0, 1.0));
        0.55 + 0.45 * pop
    }
}

/// The colors of the pill.
struct PillColors {
    fill: Color32,
    stroke: Color32,
    title: Color32,
    detail: Color32,
}

fn pill_colors(smoked: bool) -> PillColors {
    if smoked {
        PillColors {
            fill: Color32::from_rgba_unmultiplied(28, 31, 29, 232),
            stroke: Color32::from_white_alpha(40),
            title: Color32::from_rgb(244, 244, 239),
            detail: Color32::from_rgb(190, 196, 191),
        }
    } else {
        let p = pal();
        PillColors {
            fill: p.card,
            stroke: p.border,
            title: p.text,
            detail: p.muted,
        }
    }
}

/// Whether the pill is smoked graphite whatever the appearance: the pie does the same for Glass.
pub fn smoked(cfg: &Config) -> bool {
    cfg.pie_style == PieStyle::Glass
}

/// The size the pill has for `note`: as wide as its text, within `PILL_MIN_W` and `PILL_MAX_W`.
pub fn pill_size(painter: &Painter, note: &HudNote, scale: f32) -> Vec2 {
    let (title, detail) = note.clipped();
    let (tg, dg) = layouts(
        painter,
        &title,
        &detail,
        Color32::WHITE,
        Color32::WHITE,
        scale,
    );
    let text_w = tg.size().x.max(dg.map_or(0.0, |d| d.size().x));
    let w = ((PAD_L + BADGE + GAP + PAD_R) * scale + text_w)
        .clamp(PILL_MIN_W * scale, PILL_MAX_W * scale);
    Vec2::new(w.ceil(), PILL_H * scale)
}

type Galley = std::sync::Arc<egui::Galley>;

fn layouts(
    painter: &Painter,
    title: &str,
    detail: &str,
    title_color: Color32,
    detail_color: Color32,
    scale: f32,
) -> (Galley, Option<Galley>) {
    let t = painter.layout_no_wrap(
        title.to_string(),
        FontId::new(15.0 * scale, fonts::semibold()),
        title_color,
    );
    let d = (!detail.is_empty()).then(|| {
        painter.layout_no_wrap(
            detail.to_string(),
            FontId::proportional(12.5 * scale),
            detail_color,
        )
    });
    (t, d)
}

/// How the pill is drawn: the colors and the state of its animation.
#[derive(Clone, Copy, Debug)]
pub struct PillLook {
    /// The color of the badge.
    pub accent: Color32,
    /// Smoked graphite instead of the card of the appearance (see [`smoked`]).
    pub smoked: bool,
    /// Fades everything, 0 to 1.
    pub alpha: f32,
    /// Moves the pill down by that many points (the slide of the animation).
    pub lift: f32,
    /// Bounces the badge for a moment after the note changed, 0 to 1 (1 = just changed, 0 = at rest).
    pub pulse: f32,
    /// Scale of the badge, 1 at rest (see [`Phase::pop`]).
    pub pop: f32,
    /// How far the circle has stretched into the pill, 0 (a circle with the badge) to 1.
    pub grow: f32,
    /// Opacity of the text, 0 to 1.
    pub text: f32,
    /// How big everything is drawn, 1.0 = normal (see [`scale_of`]).
    pub scale: f32,
}

impl PillLook {
    /// The pill at rest, as Settings previews it.
    pub fn at_rest(cfg: &Config) -> Self {
        Self {
            accent: accent_of(cfg),
            smoked: smoked(cfg),
            alpha: 1.0,
            lift: 0.0,
            pulse: 0.0,
            pop: 1.0,
            grow: 1.0,
            text: 1.0,
            scale: scale_of(cfg),
        }
    }
}

/// Draws the pill with its center at `center`.
///
/// Returns the rectangle of the pill. Used by the notice window and by the preview in Settings.
pub fn paint_pill(painter: &Painter, center: Pos2, note: &HudNote, look: &PillLook) -> Rect {
    let PillLook {
        accent,
        smoked,
        alpha,
        lift,
        pulse,
        pop,
        grow,
        text,
        scale,
    } = *look;
    let scale = scale.clamp(0.25, 4.0);
    let alpha = alpha.clamp(0.0, 1.0);
    let c = pill_colors(smoked);
    let fade = |col: Color32| col.gamma_multiply(alpha);
    let ink = |col: Color32| col.gamma_multiply(alpha * text.clamp(0.0, 1.0));
    let (title, detail) = note.clipped();
    let (tg, dg) = layouts(painter, &title, &detail, ink(c.title), ink(c.detail), scale);
    // a circle (as wide as it is high) that stretches into the pill as `grow` goes from 0 to 1
    let full = pill_size(painter, note, scale);
    let size = Vec2::new(
        full.y + (full.x - full.y) * grow.clamp(0.0, GROW_MAX),
        full.y,
    );
    let rect = Rect::from_center_size(center + Vec2::new(0.0, lift * scale), size);

    // soft shadow, one blurred shape
    painter.add(
        egui::Shadow {
            offset: [0, (6.0 * scale).round() as i8],
            blur: (22.0 * scale).round().min(255.0) as u8,
            spread: 0,
            color: Color32::from_black_alpha((70.0 * alpha) as u8),
        }
        .as_shape(rect, PILL),
    );
    painter.rect_filled(rect, PILL, fade(c.fill));
    painter.rect_stroke(
        rect,
        PILL,
        egui::Stroke::new(1.0, fade(c.stroke)),
        egui::StrokeKind::Inside,
    );

    // the badge: the accent, with the icon of the kind in the color that reads on it
    let badge_c = Pos2::new(rect.min.x + (PAD_L + BADGE / 2.0) * scale, rect.center().y);
    // a change of the note makes the badge bounce: a swell that rings out (1 at first, then a dip)
    let t = 1.0 - pulse.clamp(0.0, 1.0);
    let ring = if pulse > 0.0 {
        (-4.5 * t).exp() * (std::f32::consts::TAU * 1.25 * t).cos()
    } else {
        0.0
    };
    let badge = BADGE * scale;
    let d = badge * pop.max(0.0) * (1.0 + PULSE_SWELL * ring);
    painter.circle_filled(badge_c, d / 2.0, fade(accent));
    fonts::paint_icon(
        painter,
        badge_c,
        note.kind.glyph(),
        20.0 * scale * (d / badge),
        fade(on_accent(accent)),
        true,
    );

    // the text: one line is centered, two lines are stacked; the pill is the window it shows through
    let painter = painter.with_clip_rect(rect.shrink(1.0));
    // it slides in from the left while it fades in
    let x = rect.min.x + (PAD_L + BADGE + GAP) * scale
        - (1.0 - text.clamp(0.0, 1.0)) * TEXT_SLIDE * scale;
    match dg {
        Some(dg) => {
            let total = tg.size().y + 1.0 + dg.size().y;
            let top = rect.center().y - total / 2.0;
            painter.galley(Pos2::new(x, top), tg.clone(), ink(c.title));
            painter.galley(Pos2::new(x, top + tg.size().y + 1.0), dg, ink(c.detail));
        }
        None => {
            let y = rect.center().y - tg.size().y / 2.0;
            painter.galley(Pos2::new(x, y), tg, ink(c.title));
        }
    }
    rect
}

/// The accent of the config as a color.
pub fn accent_of(cfg: &Config) -> Color32 {
    rgb(cfg.accent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{MOD_CMD, Shortcut};

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn an_app_is_named_the_way_a_person_would() {
        assert_eq!(app_display("/Applications/Safari.app/"), "Safari");
        assert_eq!(app_display("C:\\Program Files\\Foo\\bar.exe"), "Bar");
        assert_eq!(app_display("com.google.Chrome"), "Chrome");
        assert_eq!(app_display("org.gnome.Terminal.desktop"), "Terminal");
        assert_eq!(app_display("chrome"), "Chrome");
        assert_eq!(app_display("Google Chrome"), "Google Chrome");
        assert_eq!(app_display("@browser"), "Browser");
        assert_eq!(app_display("@Files"), "Files");
    }

    #[test]
    fn a_link_shows_its_host_and_a_file_its_name() {
        let link = HudNote::open_target("https://www.example.com/a/b?x=1#top").unwrap();
        assert_eq!(
            (link.kind, link.title.as_str()),
            (HudKind::Link, "Open link")
        );
        assert_eq!(link.detail, "example.com");
        let user = HudNote::open_target("https://me:secret@host.io/x").unwrap();
        assert_eq!(user.detail, "host.io", "never the password of a link");
        let mail = HudNote::open_target("mailto:a@b.c").unwrap();
        assert_eq!(mail.detail, "mailto", "the address stays off the screen");
        let file = HudNote::open_target("/home/u/Docs/report.pdf").unwrap();
        assert_eq!(
            (file.kind, file.detail.as_str()),
            (HudKind::File, "report.pdf")
        );
        assert!(HudNote::open_target("  ").is_none());
    }

    #[test]
    fn every_action_has_the_note_it_deserves() {
        let key = note_for(&Action::Key(Shortcut::new(6, MOD_CMD)), none).unwrap();
        assert_eq!(
            (key.kind, key.title.as_str()),
            (HudKind::Shortcut, "Shortcut")
        );
        assert!(!key.detail.is_empty());
        let app = note_for(&Action::OpenApp("Mail".into()), none).unwrap();
        assert_eq!(
            (app.title.as_str(), app.detail.as_str()),
            ("Open app", "Mail")
        );
        assert!(note_for(&Action::OpenApp("  ".into()), none).is_none());
        let br = note_for(&Action::Browser("media.next".into()), none).unwrap();
        assert_eq!(br.detail, "media.next");
        let flow = note_for(&Action::Flow("f1".into()), |id| {
            (id == "f1").then(|| "Morning".to_string())
        })
        .unwrap();
        assert_eq!(
            (flow.title.as_str(), flow.detail.as_str()),
            ("Run flow", "Morning")
        );
        assert!(note_for(&Action::Flow("gone".into()), none).is_none());
        for quiet in [
            Action::Nothing,
            Action::Wait(10),
            Action::Sequence(vec![Action::OpenApp("Mail".into())]),
        ] {
            assert!(note_for(&quiet, none).is_none(), "{quiet:?}");
        }
    }

    #[test]
    fn a_command_is_never_shown() {
        let n = note_for(&Action::Command("curl -H 'token: hunter2' x".into()), none).unwrap();
        assert_eq!(n.title, "Run command");
        assert!(n.detail.is_empty());
        assert!(note_for(&Action::Command(" ".into()), none).is_none());
    }

    #[test]
    fn long_texts_are_cut_with_an_ellipsis() {
        let n = HudNote::new(HudKind::App, "x".repeat(60), "y".repeat(60));
        let (t, d) = n.clipped();
        assert_eq!(t.chars().count(), MAX_TITLE);
        assert_eq!(d.chars().count(), MAX_DETAIL);
        assert!(t.ends_with('\u{2026}') && d.ends_with('\u{2026}'));
        assert_eq!(clip("short", 10), "short");
    }

    #[test]
    fn the_notice_comes_in_stays_and_goes_in_time() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut h = Hud::new(
            HudNote::new(HudKind::App, "Open app", "Mail"),
            Duration::from_millis(1000),
        );
        let p = h.phase(t0);
        assert_eq!((p.coming, p.going, p.over), (0.0, 0.0, false));
        assert_eq!(p.alpha(), 0.0, "invisible on the first frame");
        assert_eq!((p.grow(), p.text()), (0.0, 0.0), "a circle, no text");
        assert!(p.offset() > 40.0, "it starts below its place");
        // the circle has come up before it stretches: at a third of the way it is still a circle
        let rising = h.phase(ms(190));
        assert!(rising.offset() < p.offset() && rising.grow() < 0.2 && rising.text() == 0.0);
        // stretched: the text shows only once there is room for it
        let growing = h.phase(ms(400));
        assert!(growing.grow() > 0.3 && growing.grow() < 1.0 && growing.animating());
        let mid = h.phase(ms(1000));
        assert_eq!((mid.coming, mid.going), (1.0, 0.0));
        assert!(!mid.animating());
        assert_eq!((mid.grow(), mid.text()), (1.0, 1.0));
        assert!((mid.alpha() - 1.0).abs() < 1e-6 && mid.offset().abs() < 1e-3);
        // 1000 ms of hold after the 720 ms it takes to come in: it goes away at 1720 ms
        let leaving = h.phase(ms(1850));
        assert!(leaving.going > 0.0 && leaving.grow() < 1.0 && leaving.animating());
        assert!(!leaving.over);
        let gone = h.phase(ms(1720 + 345));
        assert!(gone.over && gone.alpha() == 0.0);
    }

    #[test]
    fn the_notice_bounces_when_it_comes_in() {
        let t0 = Instant::now();
        let mut h = Hud::new(
            HudNote::new(HudKind::App, "Open app", "Mail"),
            Duration::from_millis(1000),
        );
        h.phase(t0);
        let (mut min_offset, mut max_grow, mut max_pop) = (f32::MAX, 0.0f32, 0.0f32);
        for ms in 0..=(IN_SECS * 1000.0) as u64 {
            let p = h.phase(t0 + Duration::from_millis(ms));
            min_offset = min_offset.min(p.offset());
            max_grow = max_grow.max(p.grow());
            max_pop = max_pop.max(p.pop());
            assert!(p.grow() <= GROW_MAX && p.pop() >= 0.5, "{p:?}");
        }
        assert!(
            min_offset < -1.0,
            "the circle overshoots its place: {min_offset}"
        );
        assert!(
            max_grow > 1.01 && max_grow <= GROW_MAX,
            "the pill overshoots: {max_grow}"
        );
        assert!(max_pop > 1.02, "the badge pops past its size: {max_pop}");
        let rest = h.phase(t0 + Duration::from_millis(1000));
        assert_eq!((rest.grow(), rest.pop()), (1.0, 1.0), "and then it rests");
        assert!(rest.offset().abs() < 1e-3);
    }

    #[test]
    fn a_new_note_keeps_the_pill_a_little_longer() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut h = Hud::new(
            HudNote::new(HudKind::App, "Open app", "Mail"),
            Duration::from_millis(1000),
        );
        h.phase(t0);
        assert_eq!(h.pulse(ms(10)), 0.0, "no change yet");
        h.update(HudNote::new(HudKind::Link, "Open link", "a.io"), ms(800));
        assert!(!h.phase(ms(1200)).over, "extended to 1800 ms");
        assert!(h.pulse(ms(800)) > 0.99);
        assert_eq!(h.pulse(ms(1200)), 0.0);
        assert_eq!(h.phase(ms(1799)).going, 0.0, "still the whole pill");
        assert!(h.phase(ms(1800 + 340)).over);
        // the same note again only extends
        let before = h.note.clone();
        h.update(before, ms(1000));
        assert_eq!(h.next_wake(ms(1000)), Some(Duration::from_millis(1000)));
    }

    #[test]
    fn the_pill_fits_its_text_between_the_minimum_and_the_maximum() {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        let mut widths = Vec::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            for (t, d) in [
                ("Run", ""),
                ("Open app", "Safari"),
                ("Open link", &"w".repeat(60)[..]),
            ] {
                let n = HudNote::new(HudKind::App, t, d);
                widths.push(pill_size(&painter, &n, 1.0).x);
            }
        });
        out.textures_delta.clear(); // (the font atlas of this throw-away context)
        assert_eq!(widths[0], PILL_MIN_W);
        assert!(widths[1] >= PILL_MIN_W && widths[1] < widths[2]);
        assert!(widths[2] <= PILL_MAX_W);
        const { assert!(PILL_MAX_W + 40.0 <= WINDOW_SIZE.x, "room for the shadow") };
        const {
            assert!(
                PILL_H + 2.0 * 30.0 <= WINDOW_SIZE.y,
                "room for the shadow and the slide"
            )
        };
    }

    #[test]
    fn the_pill_grows_with_the_size_that_was_set() {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        let (mut normal, mut big, mut small) = (Vec2::ZERO, Vec2::ZERO, Vec2::ZERO);
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            let n = HudNote::new(HudKind::App, "Open app", "Safari");
            normal = pill_size(&painter, &n, 1.0);
            big = pill_size(&painter, &n, scale_of_percent(HUD_SIZE_MAX));
            small = pill_size(&painter, &n, scale_of_percent(HUD_SIZE_MIN));
            // even the longest text at the biggest size fits the window of that size
            let long = HudNote::new(HudKind::App, "x".repeat(60), "y".repeat(60));
            let s = scale_of_percent(HUD_SIZE_MAX);
            let w = window_size(s);
            let p = pill_size(&painter, &long, s);
            assert!(
                p.x * GROW_MAX.max(1.0) <= w.x && p.y + 2.0 * 30.0 * s <= w.y,
                "{p:?} {w:?}"
            );
        });
        out.textures_delta.clear();
        assert!(small.y < normal.y && normal.y < big.y);
        assert!(small.x < normal.x && normal.x < big.x);
        assert_eq!(normal.y, PILL_H);
        assert_eq!(window_size(1.0), WINDOW_SIZE);
    }

    fn scale_of_percent(p: u32) -> f32 {
        p as f32 / 100.0
    }

    #[test]
    fn every_kind_has_an_icon_in_the_font() {
        for k in [
            HudKind::App,
            HudKind::Link,
            HudKind::File,
            HudKind::Shortcut,
            HudKind::Command,
            HudKind::Browser,
            HudKind::Flow,
        ] {
            assert!(symbols::ALL.iter().any(|&(_, g)| g == k.glyph()), "{k:?}");
        }
    }
}
