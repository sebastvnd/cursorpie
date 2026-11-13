//! Executes [`Action`]s on a single worker thread so they always run in the order they were triggered.
//!
//! [`run_in`] only queues a job, so it is safe to call from the event-tap callback. The worker (started
//! by [`init`]) sends keys through the platform backend (`CGEvent` on macOS, `SendInput` on Windows)
//! and launches apps, URLs and shell commands through it as well. Events posted here carry [`SYNTH_TAG`] so the event tap ignores them.
//! A flow is run by [`flowrun`], which asks `SystemHost` (the real [`Host`]) for everything it does.
//! When blocks of a flow could not do their work and the flow did not deal with it, `report_run`
//! tells the person: a desktop notification (at most one every few seconds) and a message for the
//! UI ([`Msg::FlowRan`]) that marks the blocks and, while Settings is open, shows the details.
//!
//! A run that takes a while (a flow, a pause, a sequence with a pause in it: see `takes_time`) is
//! followed on the screen from its start to its end: its notices are marked as part of a run that
//! is going on and carry how far it has come (the block of a flow, the round of a loop, the step of
//! a sequence, the countdown of a wait), so the notice does not disappear while the run is still
//! busy; [`Msg::HudEnd`] tells the UI when it is over.
//!
//! The same runs can also be followed in a small panel with a bar and a **Stop** button
//! ([`runpanel`]): it opens by itself when a run lasts a couple of seconds, and the
//! *Update panel* block of a flow can open one on purpose. Stop ends the run: the interpreter asks
//! [`Host::cancelled`] at every block, and a pause or a command that is waited for is cut short.

use crate::config::{Action, Flow, NodeKind, is_web_url};
use crate::flowrun::{self, ExecResult, Host, OpenMode};
use crate::geometry::Point;
use crate::keys::Shortcut;
use crate::msg::{Msg, Notifier};
use crate::platform;
use crate::running::{self, Choice};
use crate::runpanel;
use crate::tap::Kind;
use crate::ui::hud::{self, HudKind, HudNote, HudProgress};
use crate::windowing::{Asked, Baseline};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Marks events we posted ourselves so the event tap ignores them.
pub const SYNTH_TAG: i64 = 0x504F_5055;

/// One queued unit of work.
struct Job {
    /// Bundle id of an app to bring to the front first (only done when the action sends keys).
    focus: Option<String>,
    action: Action,
    /// What was picked on an "Open app" slice whose app already runs: a new window, or the app
    /// that is open. `None` for every other job, which does what it always did.
    choice: Option<Choice>,
    /// Whether the notice on the screen announces it. The side buttons do not: they fire all day.
    announce: bool,
    /// When the job was queued, so the diagnostic log can say how long it waited for the worker.
    queued: Instant,
}

/// Sender side of the worker queue; set once by [`init`].
static TX: OnceLock<mpsc::Sender<Job>> = OnceLock::new();

/// How the worker reaches the UI, for the report of a flow run; set once by [`set_notifier`].
static NOTIFIER: OnceLock<Notifier> = OnceLock::new();

/// When the last desktop notification of a failed flow was shown (Unix seconds), so a flow that
/// fails every time it is triggered does not bury the screen in notifications.
static LAST_NOTE: AtomicU64 = AtomicU64::new(0);

/// Fewest seconds between two desktop notifications about failed flows.
const NOTE_EVERY_SECS: u64 = 5;

/// Whether the job the worker is running now is announced on the screen (see [`announce`]). Only the
/// worker thread writes it, and it runs one job at a time.
static ANNOUNCING: AtomicBool = AtomicBool::new(true);

/// Whether the job the worker is running now takes a while (see `takes_time`): its notices stay
/// on the screen until the job is over. Only the worker thread writes it.
static LIVE: AtomicBool = AtomicBool::new(false);

/// How far that job has come: the step of a sequence, the block of a flow (or the round of its
/// loop) that runs, and how many there are (`0` when that is not known). Only the worker thread
/// writes them.
static STEP_DONE: AtomicU32 = AtomicU32::new(0);
static STEP_TOTAL: AtomicU32 = AtomicU32::new(0);

/// How often a pause looks at the Stop button of the progress panel, in milliseconds.
const WAIT_SLICE_MS: u64 = 50;

/// A pause shorter than this (in milliseconds) is not announced: it is over before it can be read.
const WAIT_NOTE_MS: u64 = 300;

/// Tells the person what is about to happen: a small notice on the screen (see [`hud`]). Does nothing
/// for the jobs that are silent (the side buttons, which fire all the time) and when the notice is off.
fn announce(note: Option<HudNote>) {
    let Some(mut note) = note else {
        return;
    };
    if !ANNOUNCING.load(Ordering::Relaxed) {
        return;
    }
    // a run that takes a while keeps its notice until it is over, and says how far it has come
    if LIVE.load(Ordering::Relaxed) {
        note.live = true;
        if note.progress == HudProgress::None {
            note.progress = HudProgress::Steps {
                done: STEP_DONE.load(Ordering::Relaxed),
                total: STEP_TOTAL.load(Ordering::Relaxed),
            };
        }
        // the progress panel says the same, whether or not the small notice is on
        runpanel::note(&note);
    }
    if !crate::config::cfg().hud {
        return;
    }
    if let Some(n) = NOTIFIER.get() {
        n.send(Msg::Hud(note));
    }
}

/// The name of the flow `id`, for the notice.
fn flow_name(id: &str) -> Option<String> {
    flow_by_id(id).map(|f| f.name)
}

/// Gives the worker a way to reach the UI. Call once, when the UI exists.
pub fn set_notifier(n: Notifier) {
    let _ = NOTIFIER.set(n);
}

/// Starts the `cursorpie-actions` worker thread. Call once, before the first [`run_in`].
pub fn init() {
    let (tx, rx) = mpsc::channel::<Job>();
    TX.set(tx).ok();
    std::thread::Builder::new()
        .name("cursorpie-actions".into())
        .spawn(move || {
            for job in rx {
                // one line per action: its kind (never its content), how long it waited in the
                // queue (`wait_ms`) and how long it ran (`ms`, written when `_t` is dropped)
                let _t = diag::timed!(
                    "act",
                    "exec",
                    kind = kind_name(&job.action),
                    wait_ms = job.queued.elapsed().as_secs_f64() * 1000.0
                );
                if let Some(app) = job.focus.as_deref()
                    && sends_keys(&job.action)
                {
                    platform::activate_app(app);
                }
                ANNOUNCING.store(job.announce, Ordering::Relaxed);
                execute(&job.action, job.choice);
            }
        })
        .expect("failed to spawn action worker");
}

/// Queue an action. Safe to call from the event tap callback (never blocks). If `focus` (a bundle id)
/// is given and the action sends keys, that app is brought to the front first, because keys always
/// go to the app in front.
pub fn run_in(focus: Option<String>, action: Action) {
    run_in_with(focus, action, None);
}

/// Like [`run_in`], for a slice that offered the choice between a new window and the app that is
/// open (see [`running`]): `choice` says which one was picked. `None` is the plain behavior.
pub fn run_in_with(focus: Option<String>, action: Action, choice: Option<Choice>) {
    queue(focus, action, choice, true);
}

/// Like [`run_in`], but the notice on the screen stays quiet: for what is repeated all day, such as
/// the back and forward buttons.
pub fn run_in_quiet(focus: Option<String>, action: Action) {
    queue(focus, action, None, false);
}

fn queue(focus: Option<String>, action: Action, choice: Option<Choice>, announce: bool) {
    if matches!(action, Action::Nothing) {
        return;
    }
    if let Some(tx) = TX.get() {
        let _ = tx.send(Job {
            focus,
            action,
            choice,
            announce,
            queued: Instant::now(),
        });
    }
}

/// Sends the line of a "Browser" action to the browser. When it cannot be sent, the person is told
/// why (a slice that does nothing and says nothing cannot be understood); what the browser then
/// does is reported by the extension, see [`bridge::run_command`](crate::bridge::run_command).
fn browser_command(spec: &str) {
    let sent = crate::bridge::run_command(spec);
    if let Some(problem) = sent.problem() {
        diag::log!("act", "browser_command", sent = false);
        platform::native::notify("CursorPie: browser", &problem);
    }
}

/// The kind of an action as one word, for the diagnostic log. Never its content: an action holds
/// a command, a URL or a path, which belong to the person (docs/DEBUGGING.md, "Add a log line").
fn kind_name(action: &Action) -> &'static str {
    match action {
        Action::Nothing => "nothing",
        Action::Key(_) => "key",
        Action::OpenApp(_) => "open_app",
        Action::OpenUrl(_) => "open_url",
        Action::Command(_) => "command",
        Action::Browser(_) => "browser",
        Action::Sequence(_) => "sequence",
        Action::Wait(_) => "wait",
        Action::Flow(_) => "flow",
    }
}

/// Whether running `action` posts keystrokes (directly or inside a sequence).
fn sends_keys(action: &Action) -> bool {
    match action {
        Action::Key(_) => true,
        Action::Sequence(steps) => steps.iter().any(sends_keys),
        Action::Flow(id) => flow_by_id(id).is_some_and(|f| f.sends_keys()),
        _ => false,
    }
}

/// The flow with this id, copied out so the config lock is not held while it runs. `None` when
/// there is no such flow.
fn flow_by_id(id: &str) -> Option<Flow> {
    let c = crate::config::cfg();
    let flow = c.flows.iter().find(|f| f.id == id).cloned();
    drop(c);
    flow
}

/// Runs one action on the worker thread. A sequence (and a flow) waits 40 ms between steps; a
/// `Wait` adds its own pause on top of that.
///
/// `choice` applies to the first "Open app" step, or "Open URL" step with a web link, that runs,
/// whether it is the action itself or a step of a sequence: [`Choice::NewWindow`] opens a new window
/// of the app (or the link in a new browser window), [`Choice::Existing`] goes to the page that is
/// open (see [`open_web`]), and `None` does what the step always did. The other steps are not
/// affected.
fn execute(action: &Action, choice: Option<Choice>) {
    let mut choice = choice;
    let long = takes_time(action);
    if long {
        STEP_DONE.store(0, Ordering::Relaxed);
        STEP_TOTAL.store(0, Ordering::Relaxed);
        LIVE.store(true, Ordering::Relaxed);
        runpanel::begin(&run_name(action), panel_may_open(action));
    }
    execute_step(action, &mut choice);
    if long {
        LIVE.store(false, Ordering::Relaxed);
        STEP_DONE.store(0, Ordering::Relaxed);
        STEP_TOTAL.store(0, Ordering::Relaxed);
        let stopped = runpanel::stopped();
        runpanel::end();
        // the notice that stayed for the run fills its bar and goes away
        if let Some(n) = NOTIFIER.get() {
            n.send(Msg::HudEnd);
        }
        if stopped {
            announce(Some(HudNote::new(HudKind::Flow, "Stopped", "")));
        }
    }
}

/// What the progress panel of a run of `action` is titled: the name of the flow.
fn run_name(action: &Action) -> String {
    match action {
        Action::Flow(id) => flow_by_id(id).map_or_else(|| "Running".to_string(), |f| f.name),
        _ => "Running".to_string(),
    }
}

/// Whether the progress panel may open by itself for a run of `action`: the person has it on, and
/// the notices of the job are not silenced (the side buttons). A window that takes the keyboard
/// would take the keys that a run sends to the app in front, so off Windows (where the window can
/// be shown without being activated) a run that sends keys keeps the small notice instead.
fn panel_may_open(action: &Action) -> bool {
    // The config lock is let go before `presses_keys` runs: it looks the flows up with
    // `flow_by_id`, which takes the same lock, and a `Mutex` is not re-entrant. Holding it here
    // made the worker wait for itself for ever the first time a flow ran (and with the worker
    // stuck, every later action and every screen that reads the config froze too).
    let wanted = {
        let c = crate::config::cfg();
        c.progress_panel && c.panels
    };
    wanted
        && ANNOUNCING.load(Ordering::Relaxed)
        && (platform::OS == platform::Os::Windows || !presses_keys(action, 0))
}

/// Whether a run of `action` presses shortcuts or types text, in it or in a flow it calls.
fn presses_keys(action: &Action, depth: u8) -> bool {
    match action {
        Action::Key(_) => true,
        Action::Sequence(steps) => steps.iter().any(|s| presses_keys(s, depth)),
        Action::Flow(id) => flow_by_id(id).is_some_and(|f| flow_presses_keys(&f, depth)),
        _ => false,
    }
}

/// [`presses_keys`] for a flow: a block that does, or a call to a flow that does (a call whose id
/// is only known when the flow runs counts as one that does not).
fn flow_presses_keys(flow: &Flow, depth: u8) -> bool {
    flow.nodes.iter().any(|n| match &n.kind {
        NodeKind::Call(id) => {
            !id.contains("{{")
                && depth < 4
                && flow_by_id(id.trim()).is_some_and(|f| flow_presses_keys(&f, depth + 1))
        }
        kind => kind.sends_keys(),
    })
}

/// Pauses for `ms` milliseconds, or until the person presses Stop in the progress panel.
fn sleep_unless_stopped(ms: u64) {
    let end = Instant::now() + Duration::from_millis(ms);
    loop {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() || runpanel::stopped() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(WAIT_SLICE_MS)));
    }
}

/// Whether running `action` takes long enough to be followed on the screen: a flow, a pause, or a
/// sequence that has one of them among its steps.
fn takes_time(action: &Action) -> bool {
    match action {
        Action::Flow(_) => true,
        Action::Wait(ms) => u64::from(*ms) >= WAIT_NOTE_MS,
        Action::Sequence(steps) => steps.iter().any(takes_time),
        _ => false,
    }
}

/// [`execute`], with the choice that is still waiting for an "Open app" step to be used on.
fn execute_step(action: &Action, choice: &mut Option<Choice>) {
    // a step that does not do anything yet (an empty app name) has no notice, see `note_for`
    announce(hud::note_for(action, flow_name));
    match action {
        Action::Nothing => {}
        Action::Key(sc) => platform::press_shortcut(*sc),
        Action::OpenApp(app) if !app.trim().is_empty() => {
            open_app(app.trim(), choice.take());
        }
        Action::OpenUrl(target) if !target.trim().is_empty() => {
            let target = target.trim();
            // only a web link takes the choice: a file or a `mailto:` link leaves it for a later step
            let picked = if is_web_url(target) {
                choice.take()
            } else {
                None
            };
            open_web(target, picked);
        }
        Action::Command(cmd) if !cmd.trim().is_empty() => {
            platform::run_shell(cmd);
        }
        Action::Browser(spec) if !spec.trim().is_empty() => browser_command(spec),
        Action::Sequence(steps) => {
            let total = u32::try_from(steps.len()).unwrap_or(u32::MAX);
            for (i, s) in steps.iter().enumerate() {
                // Stop in the progress panel ends the sequence before the next step
                if runpanel::stopped() {
                    break;
                }
                // the notice of this step says which step of how many it is
                let done = u32::try_from(i + 1).unwrap_or(u32::MAX);
                STEP_DONE.store(done, Ordering::Relaxed);
                STEP_TOTAL.store(total, Ordering::Relaxed);
                runpanel::steps(done, total, false);
                execute_step(s, choice);
                std::thread::sleep(Duration::from_millis(40));
            }
        }
        Action::Wait(ms) => {
            let ms = (*ms).min(crate::config::MAX_WAIT_MS);
            if u64::from(ms) >= WAIT_NOTE_MS {
                announce(Some(HudNote::waiting(ms)));
            }
            sleep_unless_stopped(u64::from(ms));
        }
        Action::Flow(id) => {
            if let Some(flow) = flow_by_id(id) {
                let mut host = SystemHost {
                    choice: choice.take(),
                    announced: ANNOUNCING.load(Ordering::Relaxed),
                };
                let outcome = flowrun::run(&mut host, &flow, platform::OS);
                // a *Hide notices* block only lasts as long as the run
                ANNOUNCING.store(host.announced, Ordering::Relaxed);
                // a panel that stayed on screen between two questions (`keep_open`) is not
                // waiting for this flow any more
                crate::panel::release(crate::panel::Source::Flow);
                report_run(&flow, &outcome);
            }
        }
        _ => {}
    }
}

/// Tells the person how a run of `flow` went when blocks could not do their work and the flow did
/// not deal with it (see [`flowrun::Outcome::problems`]). The UI is told after every run, so the
/// marks of an earlier failure go away when the flow works again.
fn report_run(flow: &Flow, outcome: &flowrun::Outcome) {
    let report = outcome.report(&flow.id, &flow.name);
    if let Some(r) = &report {
        // only how many, never what: the log must not hold the content of a flow
        diag::log!("act", "flow_problems", count = r.lines.len());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if now.saturating_sub(LAST_NOTE.load(Ordering::Relaxed)) >= NOTE_EVERY_SECS {
            LAST_NOTE.store(now, Ordering::Relaxed);
            let title = format!("CursorPie \u{b7} flow \"{}\"", r.flow);
            let text: String = format!("{}. {}", r.title, r.summary)
                .chars()
                .take(240)
                .collect();
            let _ = platform::native::notify(&title, &text);
        }
    }
    if let Some(n) = NOTIFIER.get() {
        n.send(Msg::FlowRan {
            id: flow.id.clone(),
            report,
        });
    }
}

/// Opens `app` the way `choice` says (see [`execute`]).
fn open_app(app: &str, choice: Option<Choice>) {
    if choice == Some(Choice::NewWindow) {
        // counted before and watched after: whether a window appeared teaches the pie which apps
        // cannot open another one (see `windowing`)
        let before = Baseline::take(app);
        match platform::open_app_new_window(app) {
            // the count of the platform, taken right before the request, wins where windows are
            // only counted: bringing an app forward can show windows by itself (one that was
            // hidden, one made on activation)
            Asked::Sent(settled) => {
                crate::windowing::watch_new_window(app, before.map(|b| b.settled(settled)));
            }
            Asked::NotSent => diag::log!("run", "ask_not_sent"),
        }
    } else {
        // read first: the config lock must not be held while the app is looked for
        let mode = crate::config::cfg().desktop_mode.effective(platform::OS);
        if mode.follows_app() {
            platform::reveal_app(app, mode);
        } else {
            platform::open_app(app);
        }
    }
    // from now on it runs, even if the next press is sooner than the system notices
    running::note_launched(app);
}

/// Opens `target` (a link or a file) the way `choice` says: [`Choice::NewWindow`] in a new browser
/// window, [`Choice::Existing`] by going to the tab that has the page (it is opened as usual when
/// that cannot be done), `None` as it always was. Only a link of a page that the browser extension
/// reported as open gets a choice (see [`crate::bridge`]).
fn open_web(target: &str, choice: Option<Choice>) {
    match choice {
        Some(Choice::NewWindow) => platform::web::open_new_window(target),
        Some(Choice::Existing) => {
            // read first: the config lock must not be held while the browser is looked for
            let mode = crate::config::cfg().desktop_mode.effective(platform::OS);
            platform::web::reveal(target, mode);
        }
        None => platform::open_url(target),
    }
}

/// The real [`Host`] of a flow run: every request goes to [`platform`] or [`platform::native`].
struct SystemHost {
    /// What was picked on an "Open app" slice whose app already runs; the first block of the flow
    /// that opens an app (or a web link) uses it, like the first step of a sequence does.
    choice: Option<Choice>,
    /// Whether the notices of this job were on when the run began (the side buttons are quiet), so
    /// a *Hide notices* block set to `off` goes back to that and not to always on.
    announced: bool,
}

impl Host for SystemHost {
    fn key(&mut self, sc: Shortcut) {
        announce(hud::note_for(&Action::Key(sc), flow_name));
        platform::press_shortcut(sc);
    }

    fn open_app(&mut self, app: &str) {
        announce(HudNote::open_app(app));
        open_app(app, self.choice.take());
    }

    fn open_url(&mut self, target: &str) {
        announce(HudNote::open_target(target));
        // only a web link takes the choice: a file or a `mailto:` link leaves it for a later block
        let picked = if is_web_url(target) {
            self.choice.take()
        } else {
            None
        };
        open_web(target, picked);
    }

    fn command(&mut self, command: &str) {
        announce(hud::note_for(&Action::Command(command.into()), flow_name));
        platform::run_shell(command);
    }

    fn open_app_as(&mut self, app: &str, mode: OpenMode) -> bool {
        match mode {
            OpenMode::New => {
                announce(HudNote::open_app(app));
                open_app(app, Some(Choice::NewWindow));
                true
            }
            OpenMode::Existing => {
                // read first: one look at the system, on this worker thread
                let running = platform::running_among(&[app.to_string()])
                    .first()
                    .copied()
                    .unwrap_or(false);
                if running {
                    announce(HudNote::open_app(app));
                    open_app(app, Some(Choice::Existing));
                }
                running
            }
            // opening an app that runs goes to it, so smart is what the plain block does
            OpenMode::Smart | OpenMode::Plain => {
                self.open_app(app);
                true
            }
        }
    }

    fn open_url_as(&mut self, target: &str, mode: OpenMode) -> bool {
        // a file or a `mailto:` link has no tabs: it is opened as usual
        if !is_web_url(target) {
            self.open_url(target);
            return true;
        }
        let tab_open = || {
            platform::web::urls_open(&[target.to_string()])
                .first()
                .copied()
                .unwrap_or(false)
        };
        match mode {
            OpenMode::Plain => {
                announce(HudNote::open_target(target));
                open_web(target, None);
                true
            }
            OpenMode::New => {
                announce(HudNote::open_target(target));
                open_web(target, Some(Choice::NewWindow));
                true
            }
            OpenMode::Smart => {
                let existing = tab_open().then_some(Choice::Existing);
                announce(HudNote::open_target(target));
                open_web(target, existing);
                true
            }
            OpenMode::Existing => {
                let open = tab_open();
                if open {
                    announce(HudNote::open_target(target));
                    open_web(target, Some(Choice::Existing));
                }
                open
            }
        }
    }

    fn set_quiet(&mut self, quiet: bool) {
        ANNOUNCING.store(self.announced && !quiet, Ordering::Relaxed);
        runpanel::set_quiet(quiet);
    }

    fn progress(&mut self, p: flowrun::Progress) {
        // what the notices of the next blocks carry (see `announce`): the round of a loop with a
        // known number of rounds, else the block of a flow that knows its length, else only the
        // number of the block
        let (done, total) = match p.round {
            Some((round, rounds)) => (round, rounds),
            None if p.total > 0 => (p.step.min(p.total), p.total),
            None => (p.step, 0),
        };
        STEP_DONE.store(done, Ordering::Relaxed);
        STEP_TOTAL.store(total, Ordering::Relaxed);
        runpanel::steps(done, total, p.round.is_some());
    }

    fn wait(&mut self, ms: u64) {
        if ms >= WAIT_NOTE_MS {
            announce(Some(HudNote::waiting(
                u32::try_from(ms).unwrap_or(u32::MAX),
            )));
        }
        // a pause that the person can end with Stop
        sleep_unless_stopped(ms);
    }

    fn cancelled(&mut self) -> bool {
        runpanel::stopped()
    }

    fn panel_command(&mut self, command: &flowrun::PanelCommand) -> Result<(), String> {
        runpanel::command(command)
    }

    fn sleep(&mut self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    fn exec(
        &mut self,
        command: &str,
        env: &[(String, String)],
        timeout: Duration,
    ) -> Option<ExecResult> {
        // a command can take as long as its timeout: say that it is waited for, and when it ended
        announce(Some(HudNote::busy("Running command")));
        let result =
            platform::native::exec_until(command, env, timeout, &runpanel::stopped).map(|o| {
                ExecResult {
                    code: o.code,
                    stdout: o.stdout,
                }
            });
        let title = if result.is_some() {
            "Command finished"
        } else {
            "Command not started"
        };
        announce(Some(HudNote::new(HudKind::Command, title, "")));
        result
    }

    fn ask_text(&mut self, message: &str, preset: &str) -> Option<String> {
        platform::native::ask_text(message, preset)
    }

    fn ask_yes_no(&mut self, message: &str) -> Option<bool> {
        platform::native::ask_yes_no(message)
    }

    fn ask_panel(&mut self, spec: &crate::panel::PanelSpec) -> Option<crate::panel::PanelResult> {
        // the window is the UI thread's: this worker waits here for the answer
        Some(crate::panel::ask(spec.clone(), crate::panel::Source::Flow))
    }

    fn clipboard(&mut self) -> Option<String> {
        platform::native::clipboard_get()
    }

    fn set_clipboard(&mut self, text: &str) -> bool {
        platform::native::clipboard_set(text)
    }

    fn notify(&mut self, title: &str, text: &str) -> bool {
        platform::native::notify(title, text)
    }

    fn read_file(&mut self, path: &str) -> Option<String> {
        platform::native::read_file(path)
    }

    fn write_file(&mut self, path: &str, text: &str, append: bool) -> bool {
        platform::native::write_file(path, text, append)
    }

    fn file_exists(&mut self, path: &str) -> bool {
        platform::native::file_exists(path)
    }

    fn env_var(&mut self, name: &str) -> Option<String> {
        if name.is_empty() {
            return None;
        }
        std::env::var(name).ok()
    }

    fn front_app(&mut self) -> Option<String> {
        platform::frontmost_bundle_id()
    }

    fn activate_app(&mut self, app: &str) {
        platform::activate_app(app);
    }

    fn app_running(&mut self, app: &str) -> bool {
        // one look at the system, on this worker thread (the pie is not waiting for it)
        platform::running_among(&[app.to_string()])
            .first()
            .copied()
            .unwrap_or(false)
    }

    fn cursor_pos(&mut self) -> Option<(f64, f64)> {
        platform::cursor_pos()
    }

    fn random_below(&mut self, below: u64) -> u64 {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        // two calls in the same nanosecond still differ
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        mix(nanos ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15)) % below.max(1)
    }

    fn unix_time(&mut self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    fn reveal(&mut self, path: &str) {
        platform::reveal_in_file_manager(std::path::Path::new(path));
    }

    fn flow(&mut self, id: &str) -> Option<Flow> {
        flow_by_id(id)
    }

    fn list_dir(&mut self, path: &str) -> Option<Vec<String>> {
        platform::native::list_dir(path)
    }

    fn make_dir(&mut self, path: &str) -> bool {
        platform::native::make_dir(path)
    }

    fn copy_file(&mut self, from: &str, to: &str) -> bool {
        platform::native::copy_file(from, to)
    }

    fn move_file(&mut self, from: &str, to: &str) -> bool {
        platform::native::move_file(from, to)
    }

    fn delete_file(&mut self, path: &str) -> bool {
        platform::native::delete_file(path)
    }

    fn file_info(&mut self, path: &str) -> Option<(u64, u64)> {
        platform::native::file_info(path)
    }

    fn system_info(&mut self, key: &str) -> Option<String> {
        platform::native::system_info(key)
    }

    fn say(&mut self, text: &str) -> bool {
        platform::native::say(text)
    }

    fn screenshot(&mut self, path: &str) -> bool {
        platform::native::screenshot(path)
    }

    fn volume(&mut self) -> Option<u8> {
        platform::native::volume()
    }

    fn set_volume(&mut self, percent: u8) -> bool {
        platform::native::set_volume(percent)
    }

    fn browser(&mut self, line: &str) -> flowrun::BrowserReply {
        use crate::bridge::Waited;
        let (ok, message) = match crate::bridge::run_command_wait(line) {
            Waited::Reply(reply) => (reply.ok, reply.message),
            Waited::Problem(why) | Waited::Lost(why) => (false, why),
        };
        flowrun::BrowserReply { ok, message }
    }
}

/// Spreads the bits of `x` (the finalizer of SplitMix64), so numbers that differ little give
/// numbers that look unrelated. Not for secrets: a flow only rolls dice with it.
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Re-posts a swallowed mouse press/release (used when a quick click must reach the app after all).
pub fn post_mouse(pos: Point, kind: Kind, button: i64) {
    platform::post_mouse(pos, kind, button);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{MOD_CMD, Shortcut};

    #[test]
    fn only_key_actions_need_the_target_in_front() {
        assert!(sends_keys(&Action::Key(Shortcut::new(6, MOD_CMD))));
        assert!(sends_keys(&Action::Sequence(vec![
            Action::OpenApp("Safari".into()),
            Action::Key(Shortcut::new(6, 0)),
        ])));
        assert!(!sends_keys(&Action::Nothing));
        assert!(!sends_keys(&Action::OpenApp("Safari".into())));
        assert!(!sends_keys(&Action::Command("true".into())));
        assert!(!sends_keys(&Action::Sequence(vec![Action::OpenUrl(
            "https://example.com".into()
        )])));
    }

    #[test]
    fn only_runs_that_take_a_while_are_followed_on_the_screen() {
        assert!(takes_time(&Action::Flow("f".into())));
        assert!(takes_time(&Action::Wait(1000)));
        assert!(!takes_time(&Action::Wait(100)));
        assert!(takes_time(&Action::Sequence(vec![
            Action::OpenApp("Safari".into()),
            Action::Wait(2000),
        ])));
        assert!(!takes_time(&Action::Sequence(vec![
            Action::OpenApp("Safari".into()),
            Action::Key(Shortcut::new(6, 0)),
        ])));
        assert!(!takes_time(&Action::Command("true".into())));
        assert!(!takes_time(&Action::Nothing));
    }

    #[test]
    fn the_log_names_the_kind_of_an_action_and_never_its_content() {
        assert_eq!(kind_name(&Action::Nothing), "nothing");
        assert_eq!(kind_name(&Action::Key(Shortcut::new(6, 0))), "key");
        assert_eq!(kind_name(&Action::OpenApp("Safari".into())), "open_app");
        assert_eq!(
            kind_name(&Action::OpenUrl("https://example.com".into())),
            "open_url"
        );
        assert_eq!(
            kind_name(&Action::Command("rm -rf secret".into())),
            "command"
        );
        assert_eq!(
            kind_name(&Action::Sequence(vec![Action::Nothing])),
            "sequence"
        );
        assert_eq!(kind_name(&Action::Wait(10)), "wait");
        assert_eq!(kind_name(&Action::Flow("secret-name".into())), "flow");
    }

    #[test]
    fn the_app_of_a_slice_is_the_first_open_app_step() {
        // the app of the slice is the first "Open app" of the action
        let seq = Action::Sequence(vec![
            Action::Key(Shortcut::new(6, 0)),
            Action::OpenApp(" Safari ".into()),
            Action::OpenApp("Notes".into()),
        ]);
        assert_eq!(seq.open_target(), Some("Safari"));
        assert_eq!(Action::OpenApp("Mail".into()).open_target(), Some("Mail"));
        assert_eq!(Action::OpenApp("  ".into()).open_target(), None);
        assert_eq!(Action::Command("true".into()).open_target(), None);
        assert_eq!(Action::Nothing.open_target(), None);
    }

    #[test]
    fn the_choice_target_is_the_first_app_or_web_link_that_runs() {
        use crate::config::OpenTarget;
        let url = |u: &str| Action::OpenUrl(u.into());
        assert_eq!(
            url(" https://example.com ").open_choice_target(),
            Some(OpenTarget::Url("https://example.com"))
        );
        assert_eq!(
            Action::OpenApp(" Mail ".into()).open_choice_target(),
            Some(OpenTarget::App("Mail"))
        );
        // not web links: opened as they always were
        assert_eq!(url("/home/u/a.pdf").open_choice_target(), None);
        assert_eq!(url("mailto:a@b.c").open_choice_target(), None);
        assert_eq!(url("  ").open_choice_target(), None);
        // the first one that runs wins, whichever kind it is
        let seq = Action::Sequence(vec![
            Action::Key(Shortcut::new(6, 0)),
            url("mailto:a@b.c"),
            url("HTTPS://example.com"),
            Action::OpenApp("Notes".into()),
        ]);
        assert_eq!(
            seq.open_choice_target(),
            Some(OpenTarget::Url("HTTPS://example.com"))
        );
        assert_eq!(
            OpenTarget::Url("https://x.y").key(),
            "url:https://x.y",
            "a link is told from an app by its key"
        );
        assert_eq!(OpenTarget::App("Notes").key(), "Notes");
    }

    #[test]
    fn mixing_spreads_neighbouring_numbers_and_the_dice_stay_in_range() {
        assert_ne!(mix(1), mix(2));
        assert_ne!(mix(0) % 100, mix(1) % 100);
        let mut host = SystemHost {
            choice: None,
            announced: true,
        };
        for _ in 0..50 {
            assert!(host.random_below(6) < 6);
        }
        assert_eq!(host.random_below(1), 0);
        assert_eq!(host.random_below(0), 0, "never divides by zero");
    }

    #[test]
    fn the_host_answers_for_files_and_the_environment() {
        let mut host = SystemHost {
            choice: None,
            announced: true,
        };
        assert_eq!(host.env_var(""), None);
        assert_eq!(host.env_var("CURSORPIE_SURELY_NOT_SET_0"), None);
        assert!(!host.file_exists(""));
        assert!(host.unix_time() > 1_600_000_000);
    }

    #[test]
    fn the_synthetic_event_tag_is_stable() {
        // events carrying this value are ignored by the event tap; changing it is harmless but
        // must be done in one place only
        assert_eq!(SYNTH_TAG, 0x504F_5055);
    }
}
