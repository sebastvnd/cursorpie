//! The **progress panel** of a run that takes a while, and the *Update panel* block of a flow.
//!
//! The notice ([`ui::hud`](crate::ui::hud)) is a pill that says what a run is doing. This is its
//! bigger sibling: a small panel ([`crate::panel`]) with a bar, the name of the step that is running
//! (`Waiting`, `Running command`, `Open app: Safari`), how far it has come (`3.2 s`, `Round 2/10`,
//! `Step 3/8`) and a **Stop** button that ends the run. It is built from the same pieces as every
//! other panel and needs nothing new in the window except one flag, [`PanelSpec::passive`], which
//! asks the window not to take the keyboard from the app the run is for.
//!
//! There are two ways to get one, and they share everything in this file:
//!
//! * **Automatic.** A run that takes a while (see `actions::takes_time`) calls [`begin`] and [`end`].
//!   A thread of its own (`ticker`) wakes five times a second; once the run has lasted
//!   [`OPEN_AFTER`] it opens the panel, then keeps it in step with what the run announces
//!   ([`note`], [`steps`]) and closes it a moment after the run is over. A run that is quick never
//!   shows it. It stands back when the flow shows panels of its own, when the person turned it off
//!   (*Behavior > Panels of flows*), and, off Windows, when the run sends keystrokes (a window that
//!   takes the focus would take those keys).
//! * **By the flow.** The *Update panel* block ([`command`]) opens a progress panel with a title, a
//!   label and a bar, changes them while the flow works and closes it. The flow decides what it
//!   says; the panel goes away by itself when the run ends.
//!
//! **Stop** sets a flag ([`stopped`]) that the interpreter asks about at the start of every block
//! ([`Host::cancelled`](crate::flowrun::Host::cancelled)) and that cuts a *Wait* or a *Run and wait*
//! short. It is cleared by the next [`begin`], so it can never reach into a later run.
//!
//! Nothing here talks to the window directly: a panel is asked for and changed through
//! [`panel::ask_in_background`], [`panel::patch`] and [`panel::close`], which is why the rules about
//! who may show panels and what happens when the pie or another panel needs the window are the
//! ones of every panel. The text of a command is never shown (the notes it works from already
//! leave it out).

use crate::config::PanelOp;
use crate::flowrun::{PanelCommand, PanelValue};
use crate::panel::{
    self, Button, Icon, PanelResult, PanelSpec, Patch, Place, Source, TextStyle, Tone, Widget,
};
use crate::ui::hud::{HudNote, HudProgress};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The id of the panel that opens by itself.
pub const AUTO_ID: &str = "cursorpie.run";

/// How long a run has to last before its panel opens. A flow that is over in a moment never shows
/// one, and the notice that came first is enough for it.
pub const OPEN_AFTER: Duration = Duration::from_millis(2000);

/// How often the panel is brought up to date. A countdown moves in tenths of a second, so this
/// is as often as it is worth sending a message to the window.
const TICK: Duration = Duration::from_millis(200);

/// How long a panel stays with a full bar after the run it followed is over.
const LINGER: Duration = Duration::from_millis(700);

/// The ids of the parts of the panel, for the patches.
const STEP: &str = "step";
const BAR: &str = "bar";
const STOP_BUTTON: &str = "stop";
const HIDE_BUTTON: &str = "hide";

/// The person pressed Stop in the run that is going on.
static STOP: AtomicBool = AtomicBool::new(false);

/// Counts the runs: a thread or an answer that belongs to an earlier one finds that out and leaves
/// everything alone.
static GENERATION: AtomicU64 = AtomicU64::new(0);

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// What the notice said last, with the moment it said it (a countdown counts from there).
#[derive(Clone, Debug)]
struct Step {
    title: String,
    detail: String,
    progress: HudProgress,
    since: Instant,
}

/// What the panel of the run shows now, to send a patch only when something changed.
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    text: String,
    label: String,
    /// The value in thousandths (`-1000` for the moving bar), so two nearly equal ones compare equal.
    value: i32,
}

/// The run that is going on.
struct State {
    generation: u64,
    started: Instant,
    /// What the panel says in its title: the flow, or what is being run.
    name: String,
    /// The run may open a panel by itself.
    auto: bool,
    /// A *Hide notices* block is in force: the run is quiet.
    quiet: bool,
    step: Option<Step>,
    /// The block (or round) that runs, how many there are (`0`: not known) and whether it is a
    /// round of a loop.
    done: u32,
    total: u32,
    round: bool,
    /// The panel was asked for and has not been given up.
    auto_open: bool,
    /// It was closed by the person or taken over by something else: it does not come back.
    auto_gone: bool,
    shown: Option<Shown>,
    /// Panels that *Update panel* blocks opened.
    own: Vec<String>,
    /// When the run ended.
    ended: Option<Instant>,
    /// The panels were taken off the screen.
    closed: bool,
}

fn lock() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the person asked to stop the run that is going on.
pub fn stopped() -> bool {
    STOP.load(Ordering::Relaxed)
}

/// A run that takes a while starts. `name` is what the panel is titled, `auto` whether it may open
/// by itself (see the module text). Starts the thread that keeps the panel up to date.
pub fn begin(name: &str, auto: bool) {
    STOP.store(false, Ordering::Relaxed);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    {
        let mut guard = lock();
        if let Some(old) = guard.take() {
            // the run before it did not get to take its panels away
            old.close_panels();
        }
        *guard = Some(State {
            generation,
            started: Instant::now(),
            name: name.trim().to_string(),
            auto,
            quiet: false,
            step: None,
            done: 0,
            total: 0,
            round: false,
            auto_open: false,
            auto_gone: false,
            shown: None,
            own: Vec::new(),
            ended: None,
            closed: false,
        });
    }
    let _ = std::thread::Builder::new()
        .name("cursorpie-run-panel".into())
        .spawn(move || ticker(generation));
}

/// The run is over: the bar fills and the panels go away after a moment.
pub fn end() {
    let mut guard = lock();
    if let Some(st) = guard.as_mut()
        && st.ended.is_none()
    {
        st.ended = Some(Instant::now());
        st.finish_view();
    }
}

/// The notice of a block of the run (see `actions::announce`): what the panel says it is doing.
pub fn note(note: &HudNote) {
    if let Some(st) = lock().as_mut() {
        st.step = Some(Step {
            title: note.title.clone(),
            detail: note.detail.clone(),
            progress: note.progress,
            since: Instant::now(),
        });
    }
}

/// How far the run has come (see [`flowrun::Progress`](crate::flowrun::Progress)): `round` says that
/// `done` and `total` count the rounds of a loop and not the blocks of a flow.
pub fn steps(done: u32, total: u32, round: bool) {
    if let Some(st) = lock().as_mut() {
        st.done = done;
        st.total = total;
        st.round = round;
    }
}

/// A *Hide notices* block switched the notices of the rest of the run off (or on again): a quiet run
/// shows no panel of its own.
pub fn set_quiet(quiet: bool) {
    if let Some(st) = lock().as_mut() {
        st.quiet = quiet;
    }
}

/// An *Update panel* block: opens, changes or closes a panel of the flow. Nothing waits for it.
pub fn command(cmd: &PanelCommand) -> Result<(), String> {
    let source = Source::Flow;
    match cmd.op {
        PanelOp::Open => {
            panel::broker::allowed(&source)?;
            let mut guard = lock();
            let st = guard
                .as_mut()
                .ok_or_else(|| "no run is going on".to_string())?;
            let title = cmd.title.clone().unwrap_or_else(|| st.name.clone());
            let label = cmd.label.clone().unwrap_or_default();
            let value = match cmd.value {
                PanelValue::Fraction(f) => f,
                PanelValue::Busy => -1.0,
                PanelValue::Keep => 0.0,
            };
            let spec = progress_spec(&cmd.panel, &title, "", &label, value, cmd.stop)
                .checked()
                .map_err(|e| e.to_string())?;
            // the flow shows its own: the one that opens by itself stands back
            st.auto_gone = true;
            if !st.own.contains(&cmd.panel) {
                st.own.push(cmd.panel.clone());
            }
            let generation = st.generation;
            drop(guard);
            panel::ask_in_background(spec, source, move |answer| answered(generation, &answer));
            Ok(())
        }
        PanelOp::Update => {
            if let Some(title) = &cmd.title {
                panel::patch(
                    &cmd.panel,
                    Patch {
                        text: Some(title.clone()),
                        ..Patch::default()
                    },
                    source.clone(),
                );
            }
            let value = match cmd.value {
                PanelValue::Fraction(f) => Some(serde_json::json!(f)),
                PanelValue::Busy => Some(serde_json::json!(-1.0)),
                PanelValue::Keep => None,
            };
            if cmd.label.is_some() || value.is_some() {
                panel::patch(
                    &cmd.panel,
                    Patch {
                        widget: BAR.into(),
                        label: cmd.label.clone(),
                        value,
                        ..Patch::default()
                    },
                    source,
                );
            }
            Ok(())
        }
        PanelOp::Close => {
            if let Some(st) = lock().as_mut() {
                st.own.retain(|id| *id != cmd.panel);
            }
            panel::close(&cmd.panel, source);
            Ok(())
        }
    }
}

/// A panel of the run was answered: Stop stops the run; anything else (Hide, another window that
/// took over) only means that the panel is gone.
fn answered(generation: u64, answer: &PanelResult) {
    let mut guard = lock();
    let Some(st) = guard.as_mut() else {
        return;
    };
    if st.generation != generation || st.ended.is_some() {
        return;
    }
    if answer.submitted() && answer.button == STOP_BUTTON {
        diag::log!("run", "stop_pressed");
        STOP.store(true, Ordering::Relaxed);
    } else if answer.panel == AUTO_ID {
        st.auto_open = false;
        st.auto_gone = true;
    }
}

/// The description of a progress panel: a line that names the step, a bar with a label, and the
/// buttons. It does not take the keyboard (see [`PanelSpec::passive`]).
fn progress_spec(
    id: &str,
    title: &str,
    step: &str,
    label: &str,
    value: f64,
    stop: bool,
) -> PanelSpec {
    let mut widgets = Vec::new();
    // the line of the step takes room only when there is something to say
    if !step.is_empty() {
        widgets.push(Widget::Text {
            id: STEP.into(),
            text: step.to_string(),
            style: TextStyle::Body,
            show_if: None,
        });
    }
    widgets.push(Widget::Progress {
        id: BAR.into(),
        label: label.to_string(),
        value,
        show_if: None,
    });
    let mut buttons = vec![Button {
        id: HIDE_BUTTON.into(),
        label: "Hide".into(),
        cancel: true,
        ..Button::default()
    }];
    if stop {
        buttons.push(Button {
            id: STOP_BUTTON.into(),
            label: "Stop".into(),
            tone: Tone::Danger,
            ..Button::default()
        });
    }
    PanelSpec {
        id: id.to_string(),
        title: title.to_string(),
        icon: Icon::Flow,
        place: Place::Bottom,
        width: Some(340.0),
        // it goes away when the run does
        timeout_s: Some(0),
        widgets,
        buttons,
        passive: true,
        ..PanelSpec::default()
    }
}

impl State {
    /// What the panel of the run says `now`.
    fn view(&self, now: Instant) -> Shown {
        let Some(step) = &self.step else {
            return Shown {
                text: "Working".into(),
                label: String::new(),
                value: -1000,
            };
        };
        let text = if step.detail.is_empty() {
            step.title.clone()
        } else {
            format!("{}: {}", step.title, step.detail)
        };
        let elapsed = now.saturating_duration_since(step.since).as_secs_f32();
        // the block or round the run is in now, not the one the note was made in
        let progress = match step.progress {
            HudProgress::Steps { .. } => HudProgress::Steps {
                done: self.done,
                total: self.total,
            },
            other => other,
        };
        let label = match progress {
            HudProgress::Steps { done, total } if done > 0 => {
                let what = if self.round { "Round" } else { "Step" };
                if total > 0 {
                    format!("{what} {done}/{total}")
                } else {
                    format!("{what} {done}")
                }
            }
            other => other.label(elapsed),
        };
        let value = progress
            .fraction(elapsed)
            .map_or(-1000, |f| (f * 1000.0).round() as i32);
        Shown { text, label, value }
    }

    /// What the panel says when the run is over.
    fn finish_view(&mut self) {
        let text = if stopped() { "Stopped" } else { "Done" };
        self.step = Some(Step {
            title: text.into(),
            detail: String::new(),
            progress: HudProgress::Done,
            since: Instant::now(),
        });
        self.round = false;
    }

    /// Whether the panel that opens by itself should be on screen now.
    fn wants_auto(&self, now: Instant) -> bool {
        self.auto
            && !self.auto_gone
            && !self.quiet
            && now.saturating_duration_since(self.started) >= OPEN_AFTER
    }

    /// Sends what changed since the last time to the panel of the run (and opens it first when it
    /// is time). Called by the thread of the run, with the lock held.
    fn refresh(&mut self, now: Instant) {
        if self.ended.is_some() && !self.auto_open {
            // a run that was over before its panel was due never shows one
            return;
        }
        if self.quiet && self.auto_open {
            // a Hide notices block: the panel goes away, and does not come back
            panel::close(AUTO_ID, Source::Flow);
            self.auto_open = false;
            self.auto_gone = true;
            return;
        }
        let view = self.view(now);
        if !self.auto_open {
            if !self.wants_auto(now) || !panel::broker::allowed(&Source::Flow).is_ok() {
                return;
            }
            let spec = progress_spec(
                AUTO_ID,
                &self.name,
                &view.text,
                &view.label,
                f64::from(view.value) / 1000.0,
                true,
            );
            let Ok(spec) = spec.checked() else {
                self.auto_gone = true;
                return;
            };
            diag::log!("run", "panel_open");
            let generation = self.generation;
            panel::ask_in_background(spec, Source::Flow, move |answer| {
                answered(generation, &answer);
            });
            self.auto_open = true;
            self.shown = Some(view);
            return;
        }
        if self.shown.as_ref() == Some(&view) {
            return;
        }
        let old = self.shown.replace(view.clone());
        if old.as_ref().map(|o| &o.text) != Some(&view.text) {
            panel::patch(
                AUTO_ID,
                Patch {
                    widget: STEP.into(),
                    text: Some(view.text.clone()),
                    ..Patch::default()
                },
                Source::Flow,
            );
        }
        panel::patch(
            AUTO_ID,
            Patch {
                widget: BAR.into(),
                label: Some(view.label.clone()),
                value: Some(serde_json::json!(f64::from(view.value) / 1000.0)),
                ..Patch::default()
            },
            Source::Flow,
        );
    }

    /// Takes every panel of the run off the screen.
    fn close_panels(&self) {
        if self.auto_open {
            panel::close(AUTO_ID, Source::Flow);
        }
        for id in &self.own {
            panel::close(id, Source::Flow);
        }
    }
}

/// Keeps the panel of one run in step with it, and takes it away after it. Ends when the run does,
/// or when another run has begun.
fn ticker(generation: u64) {
    loop {
        std::thread::sleep(TICK);
        let mut guard = lock();
        let Some(st) = guard.as_mut() else {
            return;
        };
        if st.generation != generation {
            return;
        }
        let now = Instant::now();
        if let Some(ended) = st.ended {
            if st.closed {
                return;
            }
            // the bar shows full for a moment, then everything goes away
            st.refresh(now);
            if now.saturating_duration_since(ended) >= LINGER {
                st.close_panels();
                st.closed = true;
                return;
            }
            continue;
        }
        st.refresh(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        State {
            generation: 1,
            started: Instant::now(),
            name: "Morning".into(),
            auto: true,
            quiet: false,
            step: None,
            done: 0,
            total: 0,
            round: false,
            auto_open: false,
            auto_gone: false,
            shown: None,
            own: Vec::new(),
            ended: None,
            closed: false,
        }
    }

    fn step(title: &str, detail: &str, progress: HudProgress, since: Instant) -> Step {
        Step {
            title: title.into(),
            detail: detail.into(),
            progress,
            since,
        }
    }

    #[test]
    fn a_wait_shows_its_countdown_and_a_bar_that_fills() {
        let t0 = Instant::now();
        let mut st = state();
        st.step = Some(step("Waiting", "", HudProgress::Wait { ms: 4000 }, t0));
        let v = st.view(t0 + Duration::from_secs(1));
        assert_eq!(v.text, "Waiting");
        assert_eq!(v.label, "3.0 s");
        assert_eq!(v.value, 250);
    }

    #[test]
    fn a_command_shows_a_moving_bar_and_the_seconds_it_has_taken() {
        let t0 = Instant::now();
        let mut st = state();
        st.step = Some(step("Running command", "", HudProgress::Busy, t0));
        let v = st.view(t0 + Duration::from_secs(12));
        assert_eq!(v.text, "Running command");
        assert_eq!(v.label, "12 s");
        assert_eq!(v.value, -1000, "no number: the bar moves");
    }

    #[test]
    fn a_flow_names_its_block_and_a_loop_its_round() {
        let t0 = Instant::now();
        let mut st = state();
        st.step = Some(step(
            "Open app",
            "Safari",
            HudProgress::Steps { done: 1, total: 8 },
            t0,
        ));
        st.done = 3;
        st.total = 8;
        let v = st.view(t0);
        assert_eq!(v.text, "Open app: Safari");
        assert_eq!(
            v.label, "Step 3/8",
            "the block that runs now, not the one of the note"
        );
        assert_eq!(v.value, 375);
        st.round = true;
        st.done = 2;
        st.total = 10;
        let v = st.view(t0);
        assert_eq!(v.label, "Round 2/10");
        assert_eq!(v.value, 200);
        // a flow with a While or a call does not know how many there will be
        st.round = false;
        st.done = 4;
        st.total = 0;
        let v = st.view(t0);
        assert_eq!(v.label, "Step 4");
        assert_eq!(v.value, -1000);
    }

    #[test]
    fn nothing_is_said_before_the_first_notice_and_a_finished_run_shows_a_full_bar() {
        let mut st = state();
        let v = st.view(Instant::now());
        assert_eq!((v.text.as_str(), v.value), ("Working", -1000));
        st.finish_view();
        let v = st.view(Instant::now());
        assert_eq!(v.text, "Done");
        assert_eq!(v.value, 1000);
    }

    #[test]
    fn the_panel_opens_only_after_a_while_and_only_when_it_may() {
        let mut st = state();
        let t0 = st.started;
        assert!(!st.wants_auto(t0 + Duration::from_millis(500)), "too early");
        assert!(st.wants_auto(t0 + OPEN_AFTER));
        st.quiet = true;
        assert!(!st.wants_auto(t0 + OPEN_AFTER), "a quiet run shows nothing");
        st.quiet = false;
        st.auto_gone = true;
        assert!(
            !st.wants_auto(t0 + OPEN_AFTER),
            "a panel that was closed stays closed"
        );
        st.auto_gone = false;
        st.auto = false;
        assert!(
            !st.wants_auto(t0 + OPEN_AFTER),
            "the run did not ask for one"
        );
    }

    #[test]
    fn the_spec_has_a_bar_a_hide_button_and_a_stop_button_when_asked_and_never_takes_the_focus() {
        let s = progress_spec("p", "Morning", "Waiting", "1 s", 0.5, true)
            .checked()
            .expect("a valid panel");
        assert!(s.passive);
        assert_eq!(s.timeout_s, Some(0));
        assert_eq!(s.widgets.len(), 2);
        assert!(s.find_button(STOP_BUTTON).is_some_and(|b| !b.cancel));
        assert!(s.find_button(HIDE_BUTTON).is_some_and(|b| b.cancel));
        let quiet = progress_spec("p", "Morning", "", "", 0.0, false)
            .checked()
            .expect("a valid panel");
        assert_eq!(quiet.widgets.len(), 1, "no line for the step");
        assert!(quiet.find_button(STOP_BUTTON).is_none());
    }

    #[test]
    fn only_the_stop_button_of_the_run_that_is_going_on_stops_it() {
        // the state is shared by the whole program: the tests that touch it take turns
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        begin("Morning", false);
        let generation = lock().as_ref().map(|s| s.generation).unwrap();
        assert!(!stopped());
        // Hide, or another window that took over, is not a stop
        answered(
            generation,
            &PanelResult::cancelled(AUTO_ID, "another window took over"),
        );
        assert!(!stopped());
        // a Stop of an earlier run
        let mut late = panel_submitted(STOP_BUTTON);
        answered(generation - 1, &late);
        assert!(!stopped());
        // the Stop of this one
        answered(generation, &late);
        assert!(stopped());
        // the next run starts without it
        begin("Evening", false);
        assert!(!stopped());
        // and a Stop that arrives after the run ended counts for nothing
        let generation = lock().as_ref().map(|s| s.generation).unwrap();
        end();
        late.button = STOP_BUTTON.into();
        answered(generation, &late);
        assert!(!stopped());
    }

    fn panel_submitted(button: &str) -> PanelResult {
        let mut form = panel::Form::new(
            progress_spec("p", "T", "", "", 0.0, true)
                .checked()
                .expect("valid"),
        );
        form.submit(button).expect("a button that confirms")
    }

    #[test]
    fn a_block_that_asks_for_a_panel_while_no_run_is_going_on_says_so() {
        let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        *lock() = None;
        let cmd = PanelCommand {
            op: PanelOp::Open,
            panel: crate::config::DEFAULT_PROGRESS_PANEL.into(),
            title: None,
            label: None,
            value: PanelValue::Keep,
            stop: true,
        };
        // either panels are off in the test configuration or there is no run: never a panic
        assert!(command(&cmd).is_err());
    }

    static TURN: Mutex<()> = Mutex::new(());
}
