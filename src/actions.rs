//! Executes [`Action`]s on a single worker thread so they always run in the order they were triggered.
//!
//! [`run_in`] only queues a job, so it is safe to call from the event-tap callback. The worker (started
//! by [`init`]) sends keys through the platform backend (`CGEvent` on macOS, `SendInput` on Windows)
//! and launches apps, URLs and shell commands through it as well. Events posted here carry [`SYNTH_TAG`] so the event tap ignores them.
//! A flow is run by [`flowrun`], which asks `SystemHost` (the real [`Host`]) for everything it does.

use crate::config::{Action, Flow, is_web_url};
use crate::flowrun::{self, ExecResult, Host};
use crate::geometry::Point;
use crate::keys::Shortcut;
use crate::platform;
use crate::running::{self, Choice};
use crate::tap::Kind;
use crate::windowing::{Asked, Baseline};
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// When the job was queued, so the diagnostic log can say how long it waited for the worker.
    queued: Instant,
}

/// Sender side of the worker queue; set once by [`init`].
static TX: OnceLock<mpsc::Sender<Job>> = OnceLock::new();

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
    if matches!(action, Action::Nothing) {
        return;
    }
    if let Some(tx) = TX.get() {
        let _ = tx.send(Job {
            focus,
            action,
            choice,
            queued: Instant::now(),
        });
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
    execute_step(action, &mut choice);
}

/// [`execute`], with the choice that is still waiting for an "Open app" step to be used on.
fn execute_step(action: &Action, choice: &mut Option<Choice>) {
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
        Action::Sequence(steps) => {
            for s in steps {
                execute_step(s, choice);
                std::thread::sleep(Duration::from_millis(40));
            }
        }
        Action::Wait(ms) => {
            let ms = (*ms).min(crate::config::MAX_WAIT_MS);
            std::thread::sleep(Duration::from_millis(u64::from(ms)));
        }
        Action::Flow(id) => {
            if let Some(flow) = flow_by_id(id) {
                let mut host = SystemHost {
                    choice: choice.take(),
                };
                flowrun::run(&mut host, &flow, platform::OS);
            }
        }
        _ => {}
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
}

impl Host for SystemHost {
    fn key(&mut self, sc: Shortcut) {
        platform::press_shortcut(sc);
    }

    fn open_app(&mut self, app: &str) {
        open_app(app, self.choice.take());
    }

    fn open_url(&mut self, target: &str) {
        // only a web link takes the choice: a file or a `mailto:` link leaves it for a later block
        let picked = if is_web_url(target) {
            self.choice.take()
        } else {
            None
        };
        open_web(target, picked);
    }

    fn command(&mut self, command: &str) {
        platform::run_shell(command);
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
        platform::native::exec(command, env, timeout).map(|o| ExecResult {
            code: o.code,
            stdout: o.stdout,
        })
    }

    fn ask_text(&mut self, message: &str, preset: &str) -> Option<String> {
        platform::native::ask_text(message, preset)
    }

    fn ask_yes_no(&mut self, message: &str) -> Option<bool> {
        platform::native::ask_yes_no(message)
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
        let mut host = SystemHost { choice: None };
        for _ in 0..50 {
            assert!(host.random_below(6) < 6);
        }
        assert_eq!(host.random_below(1), 0);
        assert_eq!(host.random_below(0), 0, "never divides by zero");
    }

    #[test]
    fn the_host_answers_for_files_and_the_environment() {
        let mut host = SystemHost { choice: None };
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
