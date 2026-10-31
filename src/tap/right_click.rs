//! Right-click while the pie is open: switches which pie the side buttons follow.

use super::{Gesture, Kind, Verdict};
use crate::config::{cfg, lock};
use crate::msg::{Msg, Notifier};
use crate::side;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Whether the last right-press was swallowed, so its release is too.
static RIGHT_SWALLOWED: AtomicBool = AtomicBool::new(false);

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
            let target = side::target();
            let switched = {
                let c = cfg();
                side::toggle(&c, target.as_deref())
            };
            diag::log!("tap", "right_switch", switched = switched.is_some());
            if switched.is_some() {
                notifier.send(Msg::Hover(hover)); // wakes the UI so the ring is redrawn
            }
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
