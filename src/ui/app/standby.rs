//! Getting the pie to the screen on time, and getting it there at all.
//!
//! # What went wrong
//!
//! The window is hidden between two pies. The toolkit (eframe 0.36) treats a hidden window in a
//! special way: it runs `App::logic` for it and **nothing else** (no `App::ui`, no painting), and it
//! lets that happen at most once every 100 ms, however often the tap thread asks for a frame. Two
//! things followed from that:
//!
//! 1. **Late pie.** `ShowPie` waited up to 100 ms (and more, on top of the hold time) before the
//!    window was even shown. A flick that ended before that ran its slice's action on the tap thread
//!    while the pie had not been drawn once: the function worked, the picture never came.
//! 2. **Cold first frame.** The first frame that really paints (shaders, the font atlas, the surface)
//!    was the first pie after the program started, which after a login on a busy machine can take
//!    longer than the whole gesture. Opening Settings and closing it paid that cost, which is why
//!    the pie "came back" after it.
//!
//! And if the window was shown but nothing ever painted (the system ignored or lost the show), the
//! pie stayed "open" in the state of the app with nothing on screen, and nobody noticed.
//!
//! # What this module does
//!
//! * **Stand-by (Windows).** On the press of the trigger (`Msg::Warm`) the window is mapped at once:
//!   empty, click-through, parked far off screen. A mapped window is not throttled, so the `ShowPie`
//!   that follows is carried out in the next frame. If no pie comes (a plain click) the window is
//!   hidden again after the hold time plus a little. At startup the same stand-by is used once as a
//!   *prime*: the first real frame (shaders, atlas, surface) is drawn there, unseen.
//! * **Watch (every system).** A pie that was asked for and has not painted two frames a moment later
//!   is shown again (hidden, then shown), a few times, and the attempt is logged. This is what
//!   opening and closing Settings did by hand.

use super::App;
use crate::platform;
use eframe::egui;
use std::time::{Duration, Instant};

/// How long past `pie_hold_ms` a stand-by window waits for the pie before it is hidden again.
pub(super) const STANDBY_EXTRA: Duration = Duration::from_millis(700);
/// Painted frames a prime must have drawn before it ends.
const PRIME_FRAMES: u8 = 3;
/// A prime lasts at least this long (the surface needs time to be mapped) ...
const PRIME_MIN: Duration = Duration::from_millis(250);
/// ... and never longer than this, even if no frame was painted.
const PRIME_MAX: Duration = Duration::from_millis(2000);
/// Where the stand-by window waits: so far off screen that nothing of it can be seen, and not so
/// far that the position overflows a 16-bit coordinate at a high scale factor.
const PARK: f32 = -8000.0;

/// How long after the pie was asked for the first check is made (the first pie of a mapped-late
/// window on Linux takes about 300 ms to appear) ...
const WATCH_FIRST: Duration = Duration::from_millis(700);
/// ... and the time between two later attempts.
const WATCH_STEP: Duration = Duration::from_millis(500);
/// Times the pie is shown again before the watch gives up.
const WATCH_TRIES: u8 = 3;

/// The window is mapped, empty and parked, waiting for a pie.
pub struct Standby {
    since: Instant,
    until: Instant,
    painted: u8,
    prime: bool,
}

/// A pie was asked for and has not been drawn yet.
pub struct PieWatch {
    pub since: Instant,
    pub tries: u8,
}

impl PieWatch {
    pub fn new() -> Self {
        Self {
            since: Instant::now(),
            tries: 0,
        }
    }
}

/// What the watch does next.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum WatchStep {
    /// Look again after this long.
    Wait(Duration),
    /// Show the pie again.
    Retry,
    /// Stop: it was shown again [`WATCH_TRIES`] times and still nothing was drawn.
    GiveUp,
}

/// Pure decision of the watch: `waited` since the pie was asked for (or shown again), `tries` made
/// so far, `mapping` whether the window is still on its way to the screen (that is bounded by its
/// own timers, so it is waited for).
pub(super) fn watch_step(waited: Duration, tries: u8, mapping: bool) -> WatchStep {
    let due = if tries == 0 { WATCH_FIRST } else { WATCH_STEP };
    if mapping {
        return WatchStep::Wait(Duration::from_millis(16));
    }
    if waited < due {
        return WatchStep::Wait(due - waited);
    }
    if tries >= WATCH_TRIES {
        WatchStep::GiveUp
    } else {
        WatchStep::Retry
    }
}

/// Whether a stand-by is over. A prime ends once it has drawn enough frames (and lasted a minimum),
/// or at its maximum; a stand-by for a press ends at its deadline.
fn standby_over(s: &Standby, now: Instant) -> bool {
    if s.prime {
        let age = now.saturating_duration_since(s.since);
        (s.painted >= PRIME_FRAMES && age >= PRIME_MIN) || age >= PRIME_MAX
    } else {
        now >= s.until
    }
}

/// Only Windows keeps the window mapped between pies: its `show` is a plain `Visible(true)`. On
/// Linux a mapped transparent window has to be veiled and settled (`show_window`), and on macOS the
/// window joins Spaces, so both keep hiding the window and rely on the watch.
fn standby_allowed() -> bool {
    platform::OS == platform::Os::Windows
}

impl App {
    /// Whether something else owns the window right now (so a stand-by must not touch it).
    fn window_taken(&self) -> bool {
        self.settings_open
            || self.visible
            || self.pie_open
            || self.hud.is_some()
            || self.panel.is_some()
            || self.lock_anim.is_some()
            || self.preview_until.is_some()
            || self.close_flush.is_some()
            || self.show_wait.is_some()
            || self.quitting
    }

    /// Maps the empty window, parked off screen, so the next `ShowPie` is not delayed by the
    /// toolkit's handling of hidden windows. `hold` is how long to wait for a pie before hiding it
    /// again. Does nothing where stand-by is not used or when the window is in use.
    pub(super) fn enter_standby(&mut self, ctx: &egui::Context, hold: Duration, prime: bool) {
        if !standby_allowed() {
            return;
        }
        let now = Instant::now();
        if let Some(s) = self.standby.as_mut() {
            s.until = s.until.max(now + hold);
            return;
        }
        if self.window_taken() {
            return;
        }
        let w = crate::geometry::window_size(self.win);
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([w, w].into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::MousePassthrough(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
            egui::WindowLevel::AlwaysOnTop,
        ));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(PARK, PARK)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        platform::set_window_passive(true);
        self.standby = Some(Standby {
            since: now,
            until: now + hold,
            painted: 0,
            prime,
        });
        diag::log!("ui", "standby_begin", prime = prime);
        ctx.request_repaint();
    }

    /// Once at startup (Windows): draws the first real frames while nobody waits for them.
    pub(super) fn prime_window(&mut self, ctx: &egui::Context) {
        if self.primed {
            return;
        }
        self.primed = true;
        self.enter_standby(ctx, Duration::ZERO, true);
    }

    /// Runs every pass of `App::logic`, after the owners of the window had their turn.
    pub(super) fn step_standby(&mut self, ctx: &egui::Context) {
        let Some(s) = self.standby.as_ref() else {
            return;
        };
        if self.settings_open
            || self.visible
            || self.pie_open
            || self.hud.is_some()
            || self.panel.is_some()
            || self.lock_anim.is_some()
        {
            // somebody took the window: they show it and they hide it
            self.standby = None;
            return;
        }
        let now = Instant::now();
        if standby_over(s, now) {
            self.standby = None;
            diag::log!("ui", "standby_end");
            self.hide_root(ctx);
        } else if s.prime {
            ctx.request_repaint_after(Duration::from_millis(16));
        } else {
            ctx.request_repaint_after(s.until.saturating_duration_since(now));
        }
    }

    /// `App::ui` while a stand-by window is mapped: draws nothing, but counts the frame, so a prime
    /// knows its real frames have been painted.
    pub(super) fn standby_ui(&mut self, ctx: &egui::Context) {
        if let Some(s) = self.standby.as_mut() {
            s.painted = s.painted.saturating_add(1);
            if s.prime && s.painted < PRIME_FRAMES {
                ctx.request_repaint();
            }
        }
    }

    /// Whether the stand-by window is what `App::ui` should draw (nothing).
    pub(super) fn standby_active(&self) -> bool {
        self.standby.is_some() && !self.pie_open && !self.visible
    }

    /// Runs every pass of `App::logic`: a pie that was asked for and is not on screen after a
    /// moment is shown again.
    pub(super) fn step_pie_watch(&mut self, ctx: &egui::Context) {
        let Some(w) = self.pie_watch.as_ref() else {
            return;
        };
        if !self.pie_open
            || self.settings_open
            || self.preview_until.is_some()
            || self.pie_frames >= 2
        {
            self.pie_watch = None;
            return;
        }
        let tries = w.tries;
        let mapping = self.show_wait.is_some() || self.veil.is_some();
        match watch_step(w.since.elapsed(), tries, mapping) {
            WatchStep::Wait(d) => ctx.request_repaint_after(d),
            WatchStep::GiveUp => {
                diag::log!("ui", "pie_revive_gave_up", tries = tries);
                self.pie_watch = None;
            }
            WatchStep::Retry => {
                diag::log!(
                    "ui",
                    "pie_revive",
                    try_nr = tries + 1,
                    visible = self.visible,
                    frames = self.pie_frames
                );
                let (x, y) = (f64::from(self.pie_pos.x), f64::from(self.pie_pos.y));
                let (profile, path) = (self.profile, self.path.clone());
                // Hidden first: a window the toolkit already counts as shown cannot be shown by
                // one more `Visible(true)`, and the state of the app may have drifted from it.
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.visible = false;
                self.show_wait = None;
                self.want_pos = None;
                self.settle_frames = 0;
                self.show_pie(ctx, x, y, profile, path);
                self.pie_watch = Some(PieWatch {
                    since: Instant::now(),
                    tries: tries + 1,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_waits_then_retries_then_gives_up() {
        let ms = Duration::from_millis;
        assert_eq!(watch_step(ms(100), 0, false), WatchStep::Wait(ms(600)));
        assert_eq!(watch_step(ms(700), 0, false), WatchStep::Retry);
        assert_eq!(watch_step(ms(200), 1, false), WatchStep::Wait(ms(300)));
        assert_eq!(watch_step(ms(500), 2, false), WatchStep::Retry);
        assert_eq!(watch_step(ms(500), WATCH_TRIES, false), WatchStep::GiveUp);
    }

    #[test]
    fn watch_does_not_retry_while_the_window_is_being_mapped() {
        let long = Duration::from_secs(5);
        assert!(matches!(watch_step(long, 0, true), WatchStep::Wait(_)));
    }

    #[test]
    fn standby_for_a_press_ends_at_its_deadline() {
        let now = Instant::now();
        let s = Standby {
            since: now,
            until: now + Duration::from_millis(800),
            painted: 0,
            prime: false,
        };
        assert!(!standby_over(&s, now + Duration::from_millis(799)));
        assert!(standby_over(&s, now + Duration::from_millis(800)));
    }

    #[test]
    fn a_prime_waits_for_its_frames_but_not_for_ever() {
        let now = Instant::now();
        let mut s = Standby {
            since: now,
            until: now,
            painted: 0,
            prime: true,
        };
        assert!(!standby_over(&s, now + Duration::from_millis(500)));
        s.painted = PRIME_FRAMES;
        assert!(!standby_over(&s, now + Duration::from_millis(100)));
        assert!(standby_over(&s, now + PRIME_MIN));
        s.painted = 0;
        assert!(standby_over(&s, now + PRIME_MAX));
    }
}
