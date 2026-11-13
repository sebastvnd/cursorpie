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
//! | [`HudProgress`] | how far a run has come: a countdown, the blocks of a flow, or a moving bar |
//! | [`note_for`] | turns an [`Action`] into a note (never shows a command: it may hold a secret) |
//! | [`paint_pill`] | draws the pill, in the colors of the current appearance and accent; Settings uses it for its preview too |
//! | [`Hud`] | the state of the notice on screen: its timing and animation |
//!
//! A run that takes a while (a flow, a sequence with a wait in it) keeps its notice on the screen
//! until it is over: the notes carry a [`HudNote::live`] flag, the pill stays (see [`Hud::phase`])
//! and the worker ends it with [`Msg::HudEnd`](crate::msg::Msg::HudEnd). A pill that has a progress
//! shows a thin bar along its bottom edge and a small label (`3/8`, `4.2 s`) at its right end.
//!
//! Changes of the note are never a jump. The pill glides to its new width and height, the old text and
//! icon fade out while the new ones fade in, the bar glides to its new fill, a note that comes right
//! after another waits its turn (only the last one is shown), and a pill that was already on its way
//! out is called back smoothly. [`Hud`] keeps the little bit of state this needs; the drawing stays a
//! function of that state.
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
/// Height of the pill when it shows a progress bar under its text.
const PILL_H_BAR: f32 = 72.0;
/// Thickness of the progress bar and the distance of its middle from the bottom edge of the pill.
const BAR_H: f32 = 4.0;
const BAR_FROM_BOTTOM: f32 = 14.0;
/// Seconds one sweep takes of the bar that does not know how far the run has come.
const SWEEP_SECS: f32 = 1.4;
/// A notice that belongs to a run that is going on goes away by itself when nothing was heard of
/// the run for this long (the worker may have died before it could end the notice).
pub const LIVE_STALL: Duration = Duration::from_secs(150);
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
const PULSE_SWELL: f32 = 0.12;
/// A change of the note only starts a new bounce when the last one is mostly over (see
/// [`Hud::pulse`]), so quick changes do not make the badge shake.
const PULSE_RETRIGGER: f32 = 0.5;
/// Seconds the bounce after a change of the note takes.
const PULSE_SECS: f32 = 0.38;
/// How the time of going away is shared: the pill shrinks to a circle during the first `SHRINK_END`
/// of it, and the circle fades and sinks from `FADE_START` on.
const SHRINK_END: f32 = 0.6;
const FADE_START: f32 = 0.5;
/// Seconds the pill takes to glide to a new width and height.
const MORPH_SECS: f32 = 0.24;
/// Seconds the bar takes to glide to a new fill.
const BAR_SECS: f32 = 0.30;
/// Seconds the old text and icon take to give way to the new ones.
const SWAP_SECS: f32 = 0.24;
/// Points the text moves while it is replaced: the old one up, the new one up into its place.
const SWAP_SLIDE: f32 = 5.0;
/// Seconds a pill that was on its way out takes to come back when a note arrives.
const RECALL_SECS: f32 = 0.22;
/// A note stays at least this long before the next one replaces it. Notes that come faster (the
/// blocks of a quick flow) wait, and only the last of them is shown: a note that is on screen for a
/// few milliseconds is not a note, it is a flicker.
pub const MIN_DWELL: Duration = Duration::from_millis(260);
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
    Wait,
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
            HudKind::Wait => symbols::CLOCK,
        }
    }
}

/// How far what is announced has come. The pill draws it as a thin bar along its bottom edge and a
/// small label at its right end; the time of a countdown is kept by the pill itself, so a long wait
/// costs one message and not one per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HudProgress {
    /// Nothing to show: a short pill without a bar.
    #[default]
    None,
    /// A pause of `ms` milliseconds that began when the note arrived: a bar that fills and the time
    /// that is left.
    Wait { ms: u32 },
    /// Something that takes as long as it takes (a command that is waited for): a bar that moves
    /// back and forth and the seconds it has taken.
    Busy,
    /// Block or round `done` of `total`; with a `total` of `0` the number of blocks is not known (a
    /// flow with a loop in it): a moving bar and `step 4`.
    Steps { done: u32, total: u32 },
    /// The run is over: a full bar, while the pill waits to go away.
    Done,
}

impl HudProgress {
    /// How full the bar is, 0 to 1, `elapsed` seconds after the note arrived. `None` when it is not
    /// known (the bar moves instead) and for [`HudProgress::None`].
    pub fn fraction(self, elapsed: f32) -> Option<f32> {
        match self {
            HudProgress::None | HudProgress::Busy => None,
            HudProgress::Wait { ms } => Some((elapsed * 1000.0 / ms.max(1) as f32).clamp(0.0, 1.0)),
            HudProgress::Steps { done, total } => {
                (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0))
            }
            HudProgress::Done => Some(1.0),
        }
    }

    /// The small text at the right end of the pill.
    pub fn label(self, elapsed: f32) -> String {
        match self {
            HudProgress::None | HudProgress::Done => String::new(),
            HudProgress::Wait { ms } => {
                format!("{:.1} s", (ms as f32 / 1000.0 - elapsed).max(0.0))
            }
            HudProgress::Busy => format!("{} s", elapsed.max(0.0) as u32),
            // not started: no number yet
            HudProgress::Steps { done: 0, .. } => String::new(),
            HudProgress::Steps { done, total } if total > 0 => format!("{done}/{total}"),
            HudProgress::Steps { done, .. } => format!("step {done}"),
        }
    }

    /// The widest text [`label`](Self::label) can be, so that the pill keeps its width while the
    /// numbers change.
    fn reserve(self) -> String {
        match self {
            HudProgress::None | HudProgress::Done => String::new(),
            HudProgress::Wait { .. } => "88.8 s".into(),
            HudProgress::Busy => "888 s".into(),
            HudProgress::Steps { total, .. } if total > 0 => format!("{total}/{total}"),
            HudProgress::Steps { .. } => "step 8888".into(),
        }
    }

    /// Whether the bar moves by itself `elapsed` seconds after the note arrived (the window then
    /// needs a frame every tick).
    pub fn animates(self, elapsed: f32) -> bool {
        match self {
            HudProgress::Wait { ms } => elapsed * 1000.0 < ms as f32,
            HudProgress::Busy => true,
            HudProgress::Steps { total: 0, .. } => true,
            HudProgress::None | HudProgress::Steps { .. } | HudProgress::Done => false,
        }
    }
}

/// One thing to announce: a short title ("Open app") and an optional line under it ("Safari").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HudNote {
    pub kind: HudKind,
    pub title: String,
    pub detail: String,
    /// How far it has come; a pill with a progress also shows a bar.
    pub progress: HudProgress,
    /// The note belongs to a run that is still going on (a flow, a sequence with a wait): the pill
    /// stays on the screen until [`Msg::HudEnd`](crate::msg::Msg::HudEnd) says the run is over.
    pub live: bool,
}

impl HudNote {
    pub fn new(kind: HudKind, title: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
            progress: HudProgress::None,
            live: false,
        }
    }

    /// The same note with a progress.
    pub fn with_progress(mut self, progress: HudProgress) -> Self {
        self.progress = progress;
        self
    }

    /// The same note, marked as part of a run that is still going on (or not).
    pub fn running(mut self, live: bool) -> Self {
        self.live = live;
        self
    }

    /// "Waiting" with a countdown of `ms` milliseconds, for a pause of a flow or a sequence.
    pub fn waiting(ms: u32) -> Self {
        Self::new(HudKind::Wait, "Waiting", "").with_progress(HudProgress::Wait { ms })
    }

    /// A command that is waited for: a moving bar (the command itself is never shown, see
    /// [`note_for`]).
    pub fn busy(title: impl Into<String>) -> Self {
        Self::new(HudKind::Command, title, "").with_progress(HudProgress::Busy)
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

/// The size of the pill on its way from one width and height to another (see [`Hud::morph_size`]).
#[derive(Clone, Copy, Debug)]
struct SizeGlide {
    from: Vec2,
    to: Vec2,
    since: Instant,
}

impl SizeGlide {
    /// The size `now`: eased from `from` to `to` over [`MORPH_SECS`].
    fn at(&self, now: Instant) -> Vec2 {
        let t = now.saturating_duration_since(self.since).as_secs_f32() / MORPH_SECS;
        self.from + (self.to - self.from) * ease_out_cubic(t)
    }

    fn running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since).as_secs_f32() < MORPH_SECS
    }
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
    /// The run it announces is still going on: the pill stays until [`release`](Self::release).
    pub live: bool,
    /// When the last note arrived: a progress counts its seconds from there.
    pub progress_since: Instant,
    /// When something was last heard of the run (see [`LIVE_STALL`]).
    pub last_note: Instant,
    /// The note that was on screen before the last change: its text and icon fade out while the
    /// new ones fade in (see [`previous`](Self::previous)).
    prev: Option<HudNote>,
    /// When the shown note last changed, which is when the fade between the two starts.
    swapped_at: Instant,
    /// A note that came too soon after the last change and waits its turn, with the time it arrived.
    queued: Option<(HudNote, Instant)>,
    /// Where the bar stood when the note last changed, so that it glides from there to the fill the
    /// new note asks for. `None` when the bar had no known fill (it sweeps instead).
    bar_from: Option<f32>,
    bar_since: Instant,
    /// The glide of the pill's width and height; `None` until the first frame.
    size: Option<SizeGlide>,
    /// A pill that was on its way out and was called back by a new note: how far it had gone
    /// (0 to 1) and when. It comes back over [`RECALL_SECS`] instead of jumping.
    recall: Option<(f32, Instant)>,
    /// When this notice was made: the sweep of a bar that does not know how far the run has come
    /// counts from here, so it keeps moving from one note to the next.
    born: Instant,
}

impl Hud {
    pub fn new(note: HudNote, hold: Duration) -> Self {
        let now = Instant::now();
        Self {
            live: note.live,
            note,
            hold,
            shown_at: None,
            until: None,
            changed_at: None,
            scale: 1.0,
            progress_since: now,
            last_note: now,
            prev: None,
            swapped_at: now,
            queued: None,
            bar_from: None,
            bar_since: now,
            size: None,
            recall: None,
            born: now,
        }
    }

    /// The same notice drawn `scale` times as big (see [`scale_of`]).
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Replaces the note while the pill is on screen and keeps it there for another `hold`.
    ///
    /// A note that comes less than [`MIN_DWELL`] after the last change waits (a newer one takes its
    /// place), so the blocks of a quick flow do not flash by; [`flush_queued`](Self::flush_queued)
    /// shows it when its turn comes.
    pub fn update(&mut self, note: HudNote, now: Instant) {
        if self.shown_at.is_some() {
            // a pill that was already on its way out comes back smoothly (how far it had gone is
            // worked out before `live` and `last_note` change: they decide whether it was leaving)
            self.recall_from(now);
            self.until = Some(now + self.hold);
        }
        self.live = note.live;
        self.last_note = now;
        if note == self.note {
            // nothing new to say: what waited is stale, and a countdown starts over with every
            // note, also when the note is the same as before
            self.queued = None;
            self.restart_progress(now);
            return;
        }
        if let Some((queued, at)) = self.queued.as_mut()
            && *queued == note
        {
            *at = now;
            return;
        }
        let settled = now.saturating_duration_since(self.swapped_at) >= MIN_DWELL;
        if self.shown_at.is_none() || settled {
            self.queued = None;
            self.apply(note, now, now);
        } else {
            self.queued = Some((note, now));
        }
    }

    /// Shows the note that waited for its turn once the last change has been on screen long enough.
    /// Runs every frame.
    pub fn flush_queued(&mut self, now: Instant) {
        if self.queued.is_none() || now.saturating_duration_since(self.swapped_at) < MIN_DWELL {
            return;
        }
        if let Some((note, arrived)) = self.queued.take() {
            self.apply(note, arrived, now);
        }
    }

    /// Makes `note` the one that is shown. `arrived` is when it came (a countdown counts from
    /// there, also when the note had to wait), `now` is the moment of the change.
    fn apply(&mut self, note: HudNote, arrived: Instant, now: Instant) {
        // where the bar is now, so that it glides to where the new note puts it
        let bar_from = self.bar_fraction(now);
        if self.shown_at.is_some() {
            self.prev = Some(std::mem::replace(&mut self.note, note));
            self.swapped_at = now;
        } else {
            self.note = note;
        }
        if self.pulse(now) < PULSE_RETRIGGER {
            self.changed_at = Some(now);
        }
        self.progress_since = arrived;
        self.bar_from = bar_from;
        self.bar_since = now;
    }

    /// The countdown and the moving bar start over; the bar glides from where it is.
    fn restart_progress(&mut self, now: Instant) {
        let bar_from = self.bar_fraction(now);
        self.progress_since = now;
        self.bar_from = bar_from;
        self.bar_since = now;
    }

    /// Remembers how far the pill had gone away at `now`, before a new note calls it back.
    fn recall_from(&mut self, now: Instant) {
        let going = self.going_at(now);
        self.recall = (going > 0.001).then_some((going, now));
    }

    /// How far the notice has gone away at `now`, 0 to 1 (see [`Phase::going`]).
    fn going_at(&self, now: Instant) -> f32 {
        let Some(until) = self.until else {
            return 0.0;
        };
        let mut late = now.saturating_duration_since(until).as_secs_f32();
        if self.live && !self.stalled(now) {
            // the run goes on: the pill does not leave
            late = 0.0;
        }
        let mut going = (late / OUT_SECS).clamp(0.0, 1.0);
        if let Some((from, at)) = self.recall {
            // called back: it eases back from where it was, it does not jump to the full pill
            let back = now.saturating_duration_since(at).as_secs_f32() / RECALL_SECS;
            going = going.max(from * (1.0 - ease_out_cubic(back)));
        }
        going
    }

    /// The phase at `now`: how far the notice has come in (0 to 1), how far it has gone away
    /// (0 to 1), and whether it is over. Starts the clock on the first call. `hold` is the time the
    /// full pill stays, after it has come in and before it goes away.
    pub fn phase(&mut self, now: Instant) -> Phase {
        let shown = *self.shown_at.get_or_insert(now);
        if self.until.is_none() {
            self.until = Some(now + Duration::from_secs_f32(IN_SECS) + self.hold);
        }
        let into = now.saturating_duration_since(shown).as_secs_f32();
        let coming = (into / IN_SECS).clamp(0.0, 1.0);
        let going = self.going_at(now);
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
    /// While the run goes on it is the time until nothing heard of it counts as a stall. A note that
    /// waits for its turn shortens it to the time until that turn.
    pub fn next_wake(&self, now: Instant) -> Option<Duration> {
        let base = if self.live {
            let quiet = now.saturating_duration_since(self.last_note);
            Some(LIVE_STALL.saturating_sub(quiet))
        } else {
            self.until.map(|u| u.saturating_duration_since(now))
        };
        if self.queued.is_some() {
            let left = MIN_DWELL.saturating_sub(now.saturating_duration_since(self.swapped_at));
            return Some(base.map_or(left, |b| b.min(left)));
        }
        base
    }

    /// Whether nothing was heard of the run for [`LIVE_STALL`]: the pill then goes away as if the
    /// run had ended.
    fn stalled(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_note) > LIVE_STALL
    }

    /// Seconds since the last note arrived: what the countdown and the label are drawn from.
    pub fn elapsed(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.progress_since)
            .as_secs_f32()
    }

    /// Seconds since the notice was made: what the sweep of a bar without a known fill is drawn
    /// from. It does not start over with a new note, so the bar does not jump back to the left.
    pub fn sweep(&self, now: Instant) -> f32 {
        now.saturating_duration_since(self.born).as_secs_f32()
    }

    /// How full the bar is at `now`, 0 to 1, or `None` when the fill is not known. When the note
    /// (or its progress) changed a moment ago the bar glides from where it stood to the new fill
    /// instead of jumping.
    pub fn bar_fraction(&self, now: Instant) -> Option<f32> {
        let raw = self.note.progress.fraction(self.elapsed(now))?;
        let Some(from) = self.bar_from else {
            return Some(raw);
        };
        let t = now.saturating_duration_since(self.bar_since).as_secs_f32() / BAR_SECS;
        Some(from + (raw - from) * ease_out_cubic(t))
    }

    /// The note that was shown before the current one and how far the change has come (0 = just
    /// changed, 1 = done): the old text and icon fade out while the new ones fade in. `None` for
    /// the note once the change is over.
    pub fn previous(&self, now: Instant) -> (Option<&HudNote>, f32) {
        let t = now.saturating_duration_since(self.swapped_at).as_secs_f32() / SWAP_SECS;
        if t >= 1.0 {
            return (None, 1.0);
        }
        (
            self.prev.as_ref(),
            if self.prev.is_some() { t } else { 1.0 },
        )
    }

    /// The size the pill is drawn at in `now`, given the size `target` its note asks for: it
    /// glides from the size it has to the new one over `MORPH_SECS`. While the run goes on the
    /// pill never gets narrower, or it would breathe with every block.
    pub fn morph_size(&mut self, target: Vec2, now: Instant) -> Vec2 {
        let Some(glide) = self.size.as_mut() else {
            self.size = Some(SizeGlide {
                from: target,
                to: target,
                since: now,
            });
            return target;
        };
        let target = if self.live {
            Vec2::new(target.x.max(glide.to.x), target.y)
        } else {
            target
        };
        if (glide.to.x - target.x).abs() > 0.5 || (glide.to.y - target.y).abs() > 0.5 {
            *glide = SizeGlide {
                from: glide.at(now),
                to: target,
                since: now,
            };
        }
        glide.at(now)
    }

    /// Whether the bar of the pill moves by itself at `now`.
    pub fn progress_animates(&self, now: Instant) -> bool {
        self.note.progress.animates(self.elapsed(now))
    }

    /// Whether something glides at `now` (the size, the bar, the old text giving way, a note that
    /// waits for its turn): the window then needs a frame every tick.
    pub fn gliding(&self, now: Instant) -> bool {
        let since = |t: Instant| now.saturating_duration_since(t).as_secs_f32();
        self.queued.is_some()
            || self.size.is_some_and(|g| g.running(now))
            || (self.bar_from.is_some() && since(self.bar_since) < BAR_SECS)
            || (self.prev.is_some() && since(self.swapped_at) < SWAP_SECS)
    }

    /// The run is over: the bar fills up and the pill goes away after its usual time.
    pub fn release(&mut self, now: Instant) {
        if !self.live {
            return;
        }
        if self.shown_at.is_some() {
            // (before `live` changes: a pill that was held for its run was not on its way out)
            self.recall_from(now);
            self.until = Some(now + self.hold);
        }
        // a note that still waits for its turn is the last thing the run said: show it first
        if let Some((note, arrived)) = self.queued.take() {
            self.apply(note, arrived, now);
        }
        self.live = false;
        self.note.live = false;
        let bar_from = self.bar_fraction(now);
        self.note.progress = match self.note.progress {
            HudProgress::None => HudProgress::None,
            HudProgress::Steps { total, .. } if total > 0 => {
                HudProgress::Steps { done: total, total }
            }
            _ => HudProgress::Done,
        };
        // the bar glides to full from where it stands
        self.bar_from = bar_from;
        self.bar_since = now;
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
    // a progress has a label at the right end, and a bar under the text
    let reserve = note.progress.reserve();
    let label_w = if reserve.is_empty() {
        0.0
    } else {
        let g = painter.layout_no_wrap(reserve, FontId::proportional(12.5 * scale), Color32::WHITE);
        g.size().x + GAP * scale
    };
    let w = ((PAD_L + BADGE + GAP + PAD_R) * scale + text_w + label_w)
        .clamp(PILL_MIN_W * scale, PILL_MAX_W * scale);
    let h = if note.progress == HudProgress::None {
        PILL_H
    } else {
        PILL_H_BAR
    };
    Vec2::new(w.ceil(), h * scale)
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
    /// Seconds since the note arrived: the countdown and the label of a progress.
    pub elapsed: f32,
    /// The size to draw the pill at instead of the one its note asks for (the glide of
    /// [`Hud::morph_size`]). `None` draws the size of the note.
    pub size: Option<Vec2>,
    /// How far the old text and icon have given way to the new ones, 0 (just changed) to 1 (done,
    /// also the value at rest). Only has an effect when [`paint_pill_swap`] gets the old note.
    pub swap: f32,
    /// Seconds the sweep of a bar without a known fill has been going (see [`Hud::sweep`]).
    pub sweep: f32,
    /// The fill of the bar after it was smoothed (see [`Hud::bar_fraction`]); `None` draws the
    /// fill the progress itself gives.
    pub bar: Option<f32>,
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
            elapsed: 0.0,
            size: None,
            swap: 1.0,
            sweep: 0.0,
            bar: None,
        }
    }
}

/// Draws the pill with its center at `center`.
///
/// Returns the rectangle of the pill. Used by the notice window and by the preview in Settings.
pub fn paint_pill(painter: &Painter, center: Pos2, note: &HudNote, look: &PillLook) -> Rect {
    paint_pill_swap(painter, center, note, None, look)
}

/// [`paint_pill`] while the note is being replaced: `prev` is the note that was shown before, and
/// `look.swap` says how far the change has come. The old text (and the old icon, when the kind of
/// the note changed) fades out and moves up while the new ones fade in, instead of one replacing the
/// other in a single frame.
pub fn paint_pill_swap(
    painter: &Painter,
    center: Pos2,
    note: &HudNote,
    prev: Option<&HudNote>,
    look: &PillLook,
) -> Rect {
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
        elapsed,
        size,
        swap,
        sweep,
        bar,
    } = *look;
    let scale = scale.clamp(0.25, 4.0);
    let alpha = alpha.clamp(0.0, 1.0);
    let c = pill_colors(smoked);
    let fade = |col: Color32| col.gamma_multiply(alpha);
    let ink = |col: Color32| col.gamma_multiply(alpha * text.clamp(0.0, 1.0));
    // the change of the note: the old one gives way during the first part, the new one comes in
    // during the last part, so that the two are not both at half strength for long
    let swap = swap.clamp(0.0, 1.0);
    let prev = prev.filter(|_| swap < 1.0);
    let old_k = (1.0 - swap / 0.6).clamp(0.0, 1.0);
    let new_k = ease_out_cubic(((swap - 0.3) / 0.7).clamp(0.0, 1.0));
    // a circle (as wide as it is high) that stretches into the pill as `grow` goes from 0 to 1
    let full = size.unwrap_or_else(|| pill_size(painter, note, scale));
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
    let glyph_size = 20.0 * scale * (d / badge);
    let on = fade(on_accent(accent));
    match prev.filter(|p| p.kind != note.kind) {
        Some(p) => {
            // another kind of icon: the old one fades out, the new one fades in, in the same place
            fonts::paint_icon(
                painter,
                badge_c,
                p.kind.glyph(),
                glyph_size,
                on.gamma_multiply(old_k),
                true,
            );
            fonts::paint_icon(
                painter,
                badge_c,
                note.kind.glyph(),
                glyph_size,
                on.gamma_multiply(new_k),
                true,
            );
        }
        None => fonts::paint_icon(painter, badge_c, note.kind.glyph(), glyph_size, on, true),
    }

    // the text: one line is centered, two lines are stacked; the pill is the window it shows through
    let painter = painter.with_clip_rect(rect.shrink(1.0));
    // it slides in from the left while it fades in
    let x = rect.min.x + (PAD_L + BADGE + GAP) * scale
        - (1.0 - text.clamp(0.0, 1.0)) * TEXT_SLIDE * scale;
    // `k` is how much of the text shows, `dy` how far it is moved down (a note that gives way moves
    // up, a note that comes in rises into its place)
    let paint_text = |n: &HudNote, k: f32, dy: f32| {
        if k <= 0.0 {
            return;
        }
        let tint = |col: Color32| ink(col).gamma_multiply(k);
        let (title, detail) = n.clipped();
        let (tg, dg) = layouts(
            &painter,
            &title,
            &detail,
            tint(c.title),
            tint(c.detail),
            scale,
        );
        match dg {
            Some(dg) => {
                let total = tg.size().y + 1.0 + dg.size().y;
                let top = rect.center().y - total / 2.0 + dy;
                let below = top + tg.size().y + 1.0;
                painter.galley(Pos2::new(x, top), tg, tint(c.title));
                painter.galley(Pos2::new(x, below), dg, tint(c.detail));
            }
            None => {
                let y = rect.center().y - tg.size().y / 2.0 + dy;
                painter.galley(Pos2::new(x, y), tg, tint(c.title));
            }
        }
    };
    match prev {
        Some(p) => {
            paint_text(p, old_k, -swap * SWAP_SLIDE * scale);
            paint_text(note, new_k, (1.0 - swap) * SWAP_SLIDE * scale);
        }
        None => paint_text(note, 1.0, 0.0),
    }

    // the progress: its label at the right end and a thin bar along the bottom, under the text
    if note.progress != HudProgress::None {
        let right = rect.max.x - PAD_R * scale;
        let left = rect.min.x + (PAD_L + BADGE + GAP) * scale;
        let label = note.progress.label(elapsed);
        if !label.is_empty() {
            let lg =
                painter.layout_no_wrap(label, FontId::proportional(12.5 * scale), ink(c.detail));
            let y = rect.center().y - lg.size().y / 2.0;
            painter.galley(Pos2::new(right - lg.size().x, y), lg, ink(c.detail));
        }
        // while the pill grows taller to make room for the bar, the bar fades in with it
        let room = ((rect.height() / scale - PILL_H) / (PILL_H_BAR - PILL_H)).clamp(0.0, 1.0);
        let bar_ink = |col: Color32| ink(col).gamma_multiply(room);
        let bar_h = BAR_H * scale;
        let mid = rect.max.y - BAR_FROM_BOTTOM * scale;
        let track = Rect::from_min_max(
            Pos2::new(left, mid - bar_h / 2.0),
            Pos2::new(right.max(left), mid + bar_h / 2.0),
        );
        let w = track.width();
        painter.rect_filled(track, PILL, bar_ink(c.detail).gamma_multiply(0.3));
        // known: filled from the left (as smoothed by the notice, if it says so); not known: a
        // short piece that sweeps along the track
        let known = note
            .progress
            .fraction(elapsed)
            .map(|raw| bar.unwrap_or(raw));
        let (from, to) = match known {
            Some(f) => (0.0, w * f.clamp(0.0, 1.0)),
            None => {
                let seg = w * 0.35;
                let head = (sweep / SWEEP_SECS).fract() * (w + seg);
                ((head - seg).max(0.0), head.min(w))
            }
        };
        if to - from > 0.5 {
            let fill = Rect::from_min_max(
                Pos2::new(track.min.x + from, track.min.y),
                Pos2::new(track.min.x + to, track.max.y),
            );
            painter.rect_filled(fill, PILL, bar_ink(accent));
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
    fn a_progress_says_how_far_it_has_come() {
        let wait = HudProgress::Wait { ms: 4000 };
        assert_eq!(wait.fraction(1.0), Some(0.25));
        assert_eq!(wait.label(1.0), "3.0 s");
        assert_eq!(wait.label(9.0), "0.0 s", "never below zero");
        assert_eq!(wait.fraction(9.0), Some(1.0));
        assert!(wait.animates(1.0) && !wait.animates(4.0));

        let steps = HudProgress::Steps { done: 3, total: 8 };
        assert_eq!(steps.fraction(0.0), Some(0.375));
        assert_eq!(steps.label(0.0), "3/8");
        assert!(!steps.animates(5.0), "a bar that is known stands still");

        let unknown = HudProgress::Steps { done: 4, total: 0 };
        assert_eq!(unknown.fraction(0.0), None);
        assert_eq!(unknown.label(0.0), "step 4");
        assert!(unknown.animates(0.0));

        assert!(
            HudProgress::Steps { done: 0, total: 8 }
                .label(0.0)
                .is_empty()
        );
        assert_eq!(HudProgress::Busy.label(12.4), "12 s");
        assert_eq!(HudProgress::Done.fraction(0.0), Some(1.0));
        assert!(HudProgress::None.label(1.0).is_empty());
        assert!(!HudProgress::None.animates(1.0));
    }

    #[test]
    fn the_longest_label_is_reserved_so_the_pill_keeps_its_width() {
        let steps = HudProgress::Steps {
            done: 1,
            total: 120,
        };
        assert!(steps.reserve().len() >= steps.label(0.0).len());
        let done = HudProgress::Steps {
            done: 120,
            total: 120,
        };
        assert!(steps.reserve().len() >= done.label(0.0).len());
        assert!(HudProgress::Wait { ms: 10_000 }.reserve().len() >= "10.0 s".len());
    }

    #[test]
    fn a_pill_of_a_run_stays_until_the_run_is_over() {
        let hold = Duration::from_millis(1500);
        let note = HudNote::waiting(5000).running(true);
        let mut hud = Hud::new(note, hold);
        let t0 = Instant::now();
        assert!(!hud.phase(t0).over);
        // long after the hold would have run out it is still there, and nothing has gone away
        let later = t0 + Duration::from_secs(60);
        let phase = hud.phase(later);
        assert!(!phase.over && phase.going == 0.0);
        // the run ends: the bar is full and the pill leaves after its usual time
        hud.release(later);
        assert_eq!(hud.note.progress, HudProgress::Done);
        assert!(!hud.phase(later).over);
        assert!(hud.phase(later + hold + Duration::from_secs(1)).over);
    }

    #[test]
    fn a_finished_flow_shows_a_full_bar() {
        let note = HudNote::new(HudKind::Flow, "Run flow", "Morning")
            .with_progress(HudProgress::Steps { done: 2, total: 5 })
            .running(true);
        let mut hud = Hud::new(note, Duration::from_millis(1500));
        hud.release(Instant::now());
        assert_eq!(hud.note.progress, HudProgress::Steps { done: 5, total: 5 });
        assert!(!hud.live && !hud.note.live);
        // a second release changes nothing
        hud.release(Instant::now());
        assert_eq!(hud.note.progress, HudProgress::Steps { done: 5, total: 5 });
    }

    #[test]
    fn a_pill_whose_run_was_never_ended_goes_away_by_itself() {
        let mut hud = Hud::new(
            HudNote::busy("Running command").running(true),
            Duration::from_millis(1500),
        );
        let t0 = Instant::now();
        assert!(!hud.phase(t0).over);
        assert!(hud.phase(t0 + LIVE_STALL + Duration::from_secs(5)).over);
        assert!(hud.next_wake(t0).is_some_and(|w| w <= LIVE_STALL));
    }

    #[test]
    fn a_new_note_starts_the_countdown_again() {
        let mut hud = Hud::new(HudNote::waiting(2000), Duration::from_millis(1500));
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(3);
        assert!(hud.elapsed(t1) >= 3.0);
        hud.update(HudNote::waiting(2000), t1);
        assert!(
            hud.elapsed(t1) < 0.01,
            "the same wait again counts from its own start"
        );
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
    fn a_pill_that_is_called_back_does_not_jump() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut h = Hud::new(
            HudNote::new(HudKind::App, "Open app", "Mail"),
            Duration::from_millis(1000),
        );
        h.phase(t0);
        // it comes in during 720 ms, stays 1000 ms and has gone halfway out at 1890 ms
        let before = h.phase(ms(1890));
        assert!(before.going > 0.4 && before.going < 0.6, "{before:?}");
        h.update(HudNote::new(HudKind::Link, "Open link", "a.io"), ms(1890));
        let after = h.phase(ms(1890));
        assert!(
            (after.going - before.going).abs() < 0.01,
            "no jump: {} then {}",
            before.going,
            after.going
        );
        // it eases back instead of snapping, and then it is the whole pill again
        let later = h.phase(ms(1890 + 100));
        assert!(later.going > 0.0 && later.going < after.going, "{later:?}");
        assert_eq!(h.phase(ms(1890 + 300)).going, 0.0);
        assert!(!h.phase(ms(1890 + 300)).over);
    }

    #[test]
    fn notes_that_come_too_fast_wait_and_only_the_last_is_shown() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut h = Hud::new(
            HudNote::new(HudKind::App, "Open app", "A"),
            Duration::from_millis(1000),
        );
        h.phase(t0);
        h.update(HudNote::new(HudKind::App, "Open app", "B"), ms(50));
        h.update(HudNote::new(HudKind::App, "Open app", "C"), ms(90));
        assert_eq!(h.note.detail, "A", "the first note had no time to be read");
        assert!(h.next_wake(ms(100)).is_some_and(|w| w <= MIN_DWELL));
        h.flush_queued(ms(100));
        assert_eq!(h.note.detail, "A", "still its turn for A");
        h.flush_queued(ms(500));
        assert_eq!(h.note.detail, "C", "B was skipped, C is shown");
        // the old note is kept for the fade, and then let go
        let (prev, swap) = h.previous(ms(500));
        assert_eq!(prev.map(|n| n.detail.as_str()), Some("A"));
        assert!(swap < 0.1);
        let (prev, swap) = h.previous(ms(500 + 400));
        assert!(prev.is_none() && swap == 1.0);
        // a note that waits is not lost when the run ends
        h.update(
            HudNote::new(HudKind::App, "Open app", "D").running(true),
            ms(600),
        );
        h.release(ms(620));
        assert_eq!(h.note.detail, "D");
    }

    #[test]
    fn a_countdown_that_waited_counts_from_its_arrival() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut h = Hud::new(
            HudNote::new(HudKind::App, "Open app", "A"),
            Duration::from_millis(1000),
        );
        h.phase(t0);
        h.update(HudNote::waiting(2000), ms(50));
        h.flush_queued(ms(450));
        assert_eq!(h.note.progress, HudProgress::Wait { ms: 2000 });
        let left = h.note.progress.label(h.elapsed(ms(450)));
        assert_eq!(
            left, "1.6 s",
            "it counts from the arrival of the note, not from its turn"
        );
    }

    #[test]
    fn the_bar_glides_to_a_new_step() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let step = |done, detail: &str| {
            HudNote::new(HudKind::Flow, "Run flow", detail)
                .with_progress(HudProgress::Steps { done, total: 4 })
                .running(true)
        };
        let mut h = Hud::new(step(1, "a"), Duration::from_millis(1000));
        h.phase(t0);
        assert_eq!(h.bar_fraction(ms(10)), Some(0.25));
        h.update(step(2, "b"), ms(1000));
        assert_eq!(
            h.bar_fraction(ms(1000)),
            Some(0.25),
            "it starts where it was"
        );
        let mid = h.bar_fraction(ms(1150)).unwrap();
        assert!(mid > 0.25 && mid < 0.5, "{mid}");
        assert_eq!(h.bar_fraction(ms(1500)), Some(0.5));
        assert!(h.gliding(ms(1100)) && !h.gliding(ms(1000 + 1000)));
        // the end of the run: the bar glides to full too
        h.release(ms(3000));
        assert_eq!(h.bar_fraction(ms(3000)), Some(0.5));
        assert_eq!(h.bar_fraction(ms(3500)), Some(1.0));
    }

    #[test]
    fn the_sweep_does_not_start_over_with_every_note() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let sweeping = |detail: &str| {
            HudNote::new(HudKind::Flow, "Run flow", detail)
                .with_progress(HudProgress::Steps { done: 1, total: 0 })
                .running(true)
        };
        let mut h = Hud::new(sweeping("a"), Duration::from_millis(1000));
        h.phase(t0);
        h.update(sweeping("b"), ms(1500));
        assert!(h.sweep(ms(2000)) > 1.9, "the bar keeps moving along");
        assert!(
            h.elapsed(ms(2000)) < 0.6,
            "but the label counts from the new note"
        );
    }

    #[test]
    fn the_pill_glides_to_a_new_size_and_a_run_never_narrows_it() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut h = Hud::new(
            HudNote::new(HudKind::App, "Open app", "Mail"),
            Duration::from_millis(1000),
        );
        assert_eq!(
            h.morph_size(Vec2::new(200.0, 56.0), t0),
            Vec2::new(200.0, 56.0)
        );
        // the new size is reached with a glide, not in one frame
        assert_eq!(
            h.morph_size(Vec2::new(300.0, 72.0), t0),
            Vec2::new(200.0, 56.0)
        );
        let mid = h.morph_size(Vec2::new(300.0, 72.0), ms(100));
        assert!(
            mid.x > 200.0 && mid.x < 300.0 && mid.y > 56.0 && mid.y < 72.0,
            "{mid:?}"
        );
        assert_eq!(
            h.morph_size(Vec2::new(300.0, 72.0), ms(1000)),
            Vec2::new(300.0, 72.0)
        );
        // a pill that is not a run's gets narrower again ...
        let narrow = h.morph_size(Vec2::new(220.0, 72.0), ms(1100));
        assert!(narrow.x <= 300.0);
        assert_eq!(h.morph_size(Vec2::new(220.0, 72.0), ms(2000)).x, 220.0);
        // ... but a run's pill keeps the widest width it had
        let mut run = Hud::new(
            HudNote::waiting(5000).running(true),
            Duration::from_millis(1000),
        );
        run.morph_size(Vec2::new(300.0, 72.0), t0);
        assert_eq!(run.morph_size(Vec2::new(220.0, 72.0), ms(2000)).x, 300.0);
        run.morph_size(Vec2::new(340.0, 72.0), ms(4000));
        assert_eq!(run.morph_size(Vec2::new(340.0, 72.0), ms(5000)).x, 340.0);
    }

    #[test]
    fn a_change_of_kind_fades_between_the_icons() {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            let old = HudNote::new(HudKind::App, "Open app", "Mail");
            let new = HudNote::new(HudKind::Link, "Open link", "a.io");
            let look = PillLook {
                accent: Color32::from_rgb(80, 120, 200),
                smoked: false,
                alpha: 1.0,
                lift: 0.0,
                pulse: 0.0,
                pop: 1.0,
                grow: 1.0,
                text: 1.0,
                scale: 1.0,
                elapsed: 0.0,
                size: None,
                swap: 0.5,
                sweep: 0.0,
                bar: None,
            };
            // halfway through a change: nothing panics and the pill has the size of the new note
            let rect = paint_pill_swap(&painter, Pos2::new(200.0, 70.0), &new, Some(&old), &look);
            let want = pill_size(&painter, &new, 1.0);
            assert!((rect.width() - want.x).abs() < 0.01 && (rect.height() - want.y).abs() < 0.01);
            // a size from the glide is what is drawn
            let glided = PillLook {
                size: Some(Vec2::new(250.0, 60.0)),
                ..look
            };
            let rect = paint_pill_swap(&painter, Pos2::new(200.0, 70.0), &new, None, &glided);
            assert!((rect.width() - 250.0).abs() < 0.01 && (rect.height() - 60.0).abs() < 0.01);
        });
        out.textures_delta.clear();
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
