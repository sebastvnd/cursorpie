//! Right-click while the pie is open: switches which pie the side buttons follow.

use super::{Gesture, Kind, Verdict};
use crate::config::{cfg, lock};
use crate::msg::{Msg, Notifier};
use crate::side;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Whether the last right-press was swallowed, so its release is too.
static RIGHT_SWALLOWED: AtomicBool = AtomicBool::new(false);

/// Switches which pie (general / the app's own) the side buttons follow, for the app the open pie
/// is about, and wakes the UI (with the hovered slice) so the ring is redrawn in its new color.
/// Returns whether anything switched: false when the app has no pie of its own. Shared by the
/// right-click and by the extra click of a trackpad hold (`touch::extra`).
pub(super) fn switch_pie(notifier: &Notifier, hover: Option<usize>) -> bool {
    let target = side::target();
    let switched = {
        let c = cfg();
        side::toggle(&c, target.as_deref())
    };
    if switched.is_some() {
        notifier.send(Msg::Hover(hover)); // wakes the UI so the ring is redrawn
    }
    switched.is_some()
}

/// Right-click while the pie is open: switch which pie (general / the app's own) the side buttons
/// follow. The pie stays open and its ring changes color right away.
pub(super) fn handle_right(
    state: &Arc<Mutex<Gesture>>,
    notifier: &Notifier,
    kind: Kind,
) -> Verdict {
    match kind {
        Kind::Down => {
            let open = {
                let g = lock(state);
                match &*g {
                    Gesture::ActivePie { hover, .. } => Some(*hover),
                    _ => None,
                }
            };
            let Some(hover) = open else {
                return Verdict::Keep; // no pie open: a normal right-click
            };
            let enabled = cfg().right_click_switch;
            if !enabled {
                return Verdict::Keep;
            }
            RIGHT_SWALLOWED.store(true, Ordering::SeqCst);
            let switched = switch_pie(notifier, hover);
            diag::log!("tap", "right_switch", switched = switched);
            Verdict::Drop
        }
        Kind::Up => {
            if RIGHT_SWALLOWED.swap(false, Ordering::SeqCst) {
                Verdict::Drop
            } else {
                Verdict::Keep
            }
        }
        Kind::Drag => {
            if RIGHT_SWALLOWED.load(Ordering::SeqCst) {
                Verdict::Drop
            } else {
                Verdict::Keep
            }
        }
    }
}
