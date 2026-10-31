//! Executes [`Action`]s on a single worker thread so they always run in the order they were triggered.
//!
//! [`run_in`] only queues a job, so it is safe to call from the event-tap callback. The worker (started
//! by [`init`]) sends keys through the platform backend (`CGEvent` on macOS, `SendInput` on Windows)
//! and launches apps, URLs and shell commands through it as well. Events posted here carry [`SYNTH_TAG`] so the event tap ignores them.

use crate::config::Action;
use crate::geometry::Point;
use crate::platform;
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
                execute(&job.action);
            }
        })
        .expect("failed to spawn action worker");
}

/// Queue an action. Safe to call from the event tap callback (never blocks). If `focus` (a bundle id)
/// is given and the action sends keys, that app is brought to the front first, because keys always
/// go to the app in front.
pub fn run_in(focus: Option<String>, action: Action) {
    if matches!(action, Action::Nothing) {
        return;
    }
    if let Some(tx) = TX.get() {
        let _ = tx.send(Job {
            focus,
            action,
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
    }
}

/// Whether running `action` posts keystrokes (directly or inside a sequence).
fn sends_keys(action: &Action) -> bool {
    match action {
        Action::Key(_) => true,
        Action::Sequence(steps) => steps.iter().any(sends_keys),
        _ => false,
    }
}

/// Runs one action on the worker thread. A sequence waits 40 ms between steps.
fn execute(action: &Action) {
    match action {
        Action::Nothing => {}
        Action::Key(sc) => platform::press_shortcut(*sc),
        Action::OpenApp(app) if !app.trim().is_empty() => {
            // read first: the config lock must not be held while the app is looked for
            let mode = crate::config::cfg().desktop_mode.effective(platform::OS);
            if mode.follows_app() {
                platform::reveal_app(app.trim(), mode);
            } else {
                platform::open_app(app.trim());
            }
        }
        Action::OpenUrl(target) if !target.trim().is_empty() => {
            platform::open_url(target.trim());
        }
        Action::Command(cmd) if !cmd.trim().is_empty() => {
            platform::run_shell(cmd);
        }
        Action::Sequence(steps) => {
            for s in steps {
                execute(s);
                std::thread::sleep(Duration::from_millis(40));
            }
        }
        _ => {}
    }
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
    }

    #[test]
    fn the_synthetic_event_tag_is_stable() {
        // events carrying this value are ignored by the event tap; changing it is harmless but
        // must be done in one place only
        assert_eq!(SYNTH_TAG, 0x504F_5055);
    }
}
