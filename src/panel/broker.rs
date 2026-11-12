//! How a panel is asked for from any thread, and how its answer gets back.
//!
//! The window belongs to the UI thread. A caller (the worker that runs a flow, the thread that reads
//! the browser extension) builds a [`PanelSpec`], hands it over with [`ask`] and waits on a channel;
//! the UI shows the panel, and sends the [`PanelResult`] down that channel when the person answers,
//! when the time is up, or when something else needs the window.
//!
//! This module is also where the rules about **who may ask** live: a flow may show panels when
//! [`Config::panels`](crate::config::Config::panels) is on (it is, by default: the person wrote the
//! flow); the browser extension only when the person switched on
//! [`Config::extension_panels`](crate::config::Config::extension_panels) too, because a panel is
//! content that something outside CursorPie puts on the screen.

use super::form::{PanelResult, Patch};
use super::spec::{DEFAULT_TIMEOUT_S, PanelSpec};
use crate::msg::{Msg, Notifier};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Panels that may be waiting for an answer at the same time (shown or queued), over all callers.
pub const MAX_WAITING: usize = 4;
/// How long a panel that has not been shown yet waits for the window before it is given up.
pub const QUEUE_WAIT: Duration = Duration::from_secs(15);
/// Extra time the caller waits beyond the panel's own timeout, so the UI always answers first.
const GRACE: Duration = Duration::from_secs(20);

/// Who asked for a panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A flow (an *Ask* block in panel mode).
    Flow,
    /// The browser extension of this browser (`firefox`, `chrome`...).
    Extension(String),
}

/// A panel on its way to the window.
pub struct Request {
    pub spec: PanelSpec,
    /// Where the answer goes.
    pub reply: mpsc::Sender<PanelResult>,
    pub source: Source,
    /// When it was asked for, to give up on one that waited too long for the window.
    pub created: Instant,
}

static NOTIFIER: OnceLock<Notifier> = OnceLock::new();
static WAITING: AtomicUsize = AtomicUsize::new(0);

/// Gives the callers a way to reach the UI. Call once, when the UI exists.
pub fn set_notifier(n: Notifier) {
    let _ = NOTIFIER.set(n);
}

/// Whether `source` may show a panel now; the reason when it may not.
pub fn allowed(source: &Source) -> Result<(), String> {
    let c = crate::config::cfg();
    match source {
        Source::Flow if c.panels => Ok(()),
        Source::Flow => Err("Panels are switched off: turn them on in Settings > Behavior".into()),
        Source::Extension(_) if !c.browser_bridge => {
            Err("The browser extension is switched off in Settings > Behavior".into())
        }
        Source::Extension(_) if !c.extension_panels => Err(
            "Panels from the browser extension are switched off: turn them on in Settings > Behavior"
                .into(),
        ),
        Source::Extension(_) => Ok(()),
    }
}

/// One of the [`MAX_WAITING`] places, given back when it is dropped.
struct Place;

impl Place {
    fn take() -> Option<Place> {
        if WAITING.fetch_add(1, Ordering::SeqCst) >= MAX_WAITING {
            WAITING.fetch_sub(1, Ordering::SeqCst);
            None
        } else {
            Some(Place)
        }
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        WAITING.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Shows `spec` and waits for the answer. **Blocks**: never call it from the UI thread, nor from a
/// thread that has to keep reading (use [`ask_in_background`]).
pub fn ask(spec: PanelSpec, source: Source) -> PanelResult {
    let mut spec = spec;
    if matches!(source, Source::Extension(_)) {
        // content from outside may not make CursorPie read picture files of this computer
        spec.strip_file_icons();
    }
    let id = spec.id.clone();
    if let Err(why) = allowed(&source) {
        return PanelResult::unavailable(&id, why);
    }
    let Some(notifier) = NOTIFIER.get() else {
        return PanelResult::unavailable(&id, "CursorPie is not ready yet");
    };
    let Some(_place) = Place::take() else {
        return PanelResult::unavailable(&id, "Too many panels are waiting");
    };
    let wait = spec.timeout_s.unwrap_or(DEFAULT_TIMEOUT_S);
    let (tx, rx) = mpsc::channel();
    diag::log!(
        "panel",
        "ask",
        id = id.clone(),
        widgets = spec.widgets.len()
    );
    notifier.send(Msg::Panel(Request {
        spec,
        reply: tx,
        source,
        created: Instant::now(),
    }));
    let answer = if wait == 0 {
        // no time limit: the answer, or the UI going away
        rx.recv().ok()
    } else {
        rx.recv_timeout(Duration::from_secs(u64::from(wait)) + GRACE)
            .ok()
    };
    answer.unwrap_or_else(|| PanelResult::unavailable(&id, "The window did not answer"))
}

/// [`ask`] on a thread of its own; `done` gets the answer there.
pub fn ask_in_background(
    spec: PanelSpec,
    source: Source,
    done: impl FnOnce(PanelResult) + Send + 'static,
) {
    let _ = std::thread::Builder::new()
        .name("cursorpie-panel-wait".into())
        .spawn(move || done(ask(spec, source)));
}

/// Changes the panel `panel` that is open (see [`Patch`]). A panel that `source` did not ask for is
/// left alone.
pub fn patch(panel: &str, patch: Patch, source: Source) {
    if let Some(n) = NOTIFIER.get() {
        n.send(Msg::PanelPatch {
            panel: panel.to_string(),
            patch,
            source,
        });
    }
}

/// Lets go of a panel of `source` that stayed on screen after a button answered it (`keep_open`,
/// see [`PanelSpec::keep_open`]) and was not taken over by a newer panel: the flow that was
/// asking is over. Does nothing to a panel that is waiting for an answer.
pub fn release(source: Source) {
    if let Some(n) = NOTIFIER.get() {
        n.send(Msg::PanelClose {
            panel: String::new(),
            source,
        });
    }
}

/// Closes the panel `panel` as cancelled. A panel that `source` did not ask for is left alone.
pub fn close(panel: &str, source: Source) {
    if let Some(n) = NOTIFIER.get() {
        n.send(Msg::PanelClose {
            panel: panel.to_string(),
            source,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_so_many_panels_wait_at_once_and_a_place_comes_back_when_dropped() {
        let held: Vec<Place> = (0..MAX_WAITING).filter_map(|_| Place::take()).collect();
        assert_eq!(held.len(), MAX_WAITING);
        assert!(Place::take().is_none(), "one too many");
        drop(held);
        assert!(Place::take().is_some(), "free again");
    }
}
