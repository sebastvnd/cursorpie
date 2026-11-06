//! Executes [`Action`]s on a single worker thread so they always run in the order they were triggered.
//!
//! [`run_in`] only queues a job, so it is safe to call from the event-tap callback. The worker (started
//! by [`init`]) sends keys through the platform backend (`CGEvent` on macOS, `SendInput` on Windows)
//! and launches apps, URLs and shell commands through it as well. Events posted here carry [`SYNTH_TAG`] so the event tap ignores them.

use crate::config::Action;
use crate::geometry::Point;
use crate::platform;
use crate::running::{self, Choice};
use crate::tap::Kind;
use std::sync::{OnceLock, mpsc};
use std::time::{Duration, Instant};

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
        Action::Flow(id) => flow_steps(id).iter().any(sends_keys),
        _ => false,
    }
}

/// The steps of the flow with this id, copied out so the config lock is not held while they run.
/// Empty when there is no such flow.
fn flow_steps(id: &str) -> Vec<Action> {
    let c = crate::config::cfg();
    let steps: Vec<Action> = c
        .flows
        .iter()
        .find(|f| f.id == id)
        .map(|f| f.steps())
        .unwrap_or_default();
    drop(c);
    steps
}

/// Runs one action on the worker thread. A sequence (and a flow) waits 40 ms between steps; a
/// `Wait` adds its own pause on top of that.
///
/// `choice` applies to the first "Open app" step that runs, whether it is the action itself or a step
/// of a sequence: [`Choice::NewWindow`] opens a new window of the app, anything else does what an
/// "Open app" always did. The other steps are not affected.
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
            platform::open_url(target.trim());
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
            for s in flow_steps(id) {
                execute_step(&s, choice);
                std::thread::sleep(Duration::from_millis(40));
            }
        }
        _ => {}
    }
}

/// Opens `app` the way `choice` says (see [`execute`]).
fn open_app(app: &str, choice: Option<Choice>) {
    if choice == Some(Choice::NewWindow) {
        platform::open_app_new_window(app);
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
    fn the_synthetic_event_tag_is_stable() {
        // events carrying this value are ignored by the event tap; changing it is harmless but
        // must be done in one place only
        assert_eq!(SYNTH_TAG, 0x504F_5055);
    }
}
