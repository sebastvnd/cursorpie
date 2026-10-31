//! Back/forward side buttons: send the active mode's actions, otherwise pass through.

use super::resolve::{remember_target, run_action, target_bundle_at};
use super::{Gesture, Kind, SIDE_BACK, Verdict};
use crate::config::{Action, ItemKind, cfg, item_at, lock};
use crate::geometry::Point;
use crate::side;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Whether the last side-button press (back, forward) was swallowed, so its release is too.
static SIDE_SWALLOWED: [AtomicBool; 2] = [AtomicBool::new(false), AtomicBool::new(false)];

/// Back/forward side buttons: send the active (or hovered) mode's actions, otherwise pass through.
/// Which pie they follow is decided by `side` (see there); `p` is where the pointer is.
pub(super) fn handle_side(
    state: &Arc<Mutex<Gesture>>,
    kind: Kind,
    button: i64,
    p: Point,
) -> Verdict {
    let idx = if button == SIDE_BACK { 0 } else { 1 };
    match kind {
        Kind::Down => {
            let (pie_profile, hovered) = {
                let g = lock(state);
                match &*g {
                    Gesture::ActivePie {
                        profile,
                        path,
                        hover,
                        ..
                    } => {
                        let h = hover.map(|h| {
                            let mut p = path.clone();
                            p.push(h);
                            p
                        });
                        (Some(*profile), h)
                    }
                    _ => (None, None),
                }
            };
            let in_pie = pie_profile.is_some();

            // Outside the pie, find out which app this press is about. Inside the pie it is the app
            // the pie was opened for.
            let target = if in_pie {
                side::target()
            } else {
                let bundle = target_bundle_at(p);
                remember_target(bundle.clone());
                bundle
            };

            let action = {
                let c = cfg();
                let pi = pie_profile.unwrap_or_else(|| side::owner(&c, target.as_deref()));
                let item = c.profiles.get(pi).and_then(|pr| match &hovered {
                    Some(p) => item_at(&pr.items, p),
                    None => pr.active_item(),
                });
                match item.map(|i| &i.kind) {
                    Some(ItemKind::Mode { back, forward }) => {
                        if idx == 0 {
                            back.clone()
                        } else {
                            forward.clone()
                        }
                    }
                    _ => Action::Nothing,
                }
            };

            let nothing = matches!(action, Action::Nothing);
            diag::log!(
                "tap",
                "side_press",
                button = button,
                in_pie = in_pie,
                mapped = !nothing,
                passed_through = nothing && !in_pie
            );
            if nothing && !in_pie {
                SIDE_SWALLOWED[idx].store(false, Ordering::SeqCst);
                return Verdict::Keep; // native back/forward keeps working
            }
            SIDE_SWALLOWED[idx].store(true, Ordering::SeqCst);
            run_action(action, target);
            Verdict::Drop
        }
        Kind::Up => {
            if SIDE_SWALLOWED[idx].swap(false, Ordering::SeqCst) {
                Verdict::Drop
            } else {
                Verdict::Keep
            }
        }
        Kind::Drag => {
            if SIDE_SWALLOWED[idx].load(Ordering::SeqCst) {
                Verdict::Drop
            } else {
                Verdict::Keep
            }
        }
    }
}
