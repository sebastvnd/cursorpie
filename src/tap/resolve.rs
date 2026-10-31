//! What a gesture means: which slice is hovered, which profile and app it is about, and how a
//! released slice is applied.

use crate::actions;
use crate::config::{
    self, Action, AppGesture, Item, ItemKind, MAX_DEPTH, TargetApp, cfg, items_at,
};
use crate::geometry::Point;
use crate::geometry::{pie_inner, pie_outer, slice_at};
use crate::platform;
use crate::side;

/// Result of testing the cursor against the pie.
pub(super) struct Hit {
    pub hover: Option<usize>,
    /// cursor crossed the rim over a submenu slice
    pub enter: bool,
}

/// Which slice is under `p`, and whether `p` crossed the outer rim over a submenu slice.
pub(super) fn eval(profile: usize, path: &[usize], center: Point, p: Point) -> Hit {
    let c = cfg();
    let win = c.pie_size;
    let Some(items) = c
        .profiles
        .get(profile)
        .and_then(|pr| items_at(&pr.items, path))
    else {
        return Hit {
            hover: None,
            enter: false,
        };
    };
    let hover = slice_at(center, p, items.len(), pie_inner(win));
    let dist = ((p.x - center.x) as f32).hypot((p.y - center.y) as f32);
    let enter = match hover.and_then(|h| items.get(h)) {
        Some(Item {
            kind: ItemKind::Submenu(ch),
            ..
        }) => !ch.is_empty() && dist > pie_outer(win) && path.len() + 1 < MAX_DEPTH,
        _ => false,
    };
    Hit { hover, enter }
}

/// Applies the slice the user released over.
pub(super) fn select(profile: usize, path: &[usize], index: usize) {
    let kind = {
        let c = cfg();
        c.profiles
            .get(profile)
            .and_then(|pr| items_at(&pr.items, path))
            .and_then(|items| items.get(index))
            .map(|i| i.kind.clone())
    };
    diag::log!(
        "tap",
        "select",
        profile = profile,
        depth = path.len(),
        index = index,
        kind = match &kind {
            Some(ItemKind::Mode { .. }) => "mode",
            Some(ItemKind::Run(_)) => "run",
            Some(_) => "other",
            None => "missing",
        }
    );
    match kind {
        Some(ItemKind::Mode { .. }) => {
            let mut full = path.to_vec();
            full.push(index);
            config::set_active(profile, full);
            // the side buttons now follow the pie this mode was picked in
            let target = side::target();
            let c = cfg();
            if c.side_adopt {
                side::adopt(&c, target.as_deref(), profile);
            }
        }
        Some(ItemKind::Run(action)) => run_action(action, side::target()),
        _ => {}
    }
}

/// Runs an action. If the settings say so, the target app is brought to the front first, because
/// shortcuts always go to the app in front.
pub(super) fn run_action(action: Action, target: Option<String>) {
    let focus = {
        let c = cfg();
        c.target_app == TargetApp::UnderCursor && c.focus_target
    };
    actions::run_in(if focus { target } else { None }, action);
}

/// The app a gesture at `p` is about: the app in front, or the app whose window is under `p`
/// (falling back to the app in front when that cannot be told).
pub(super) fn target_bundle_at(p: Point) -> Option<String> {
    let mode = cfg().target_app;
    match mode {
        TargetApp::Focused => platform::frontmost_bundle_id(),
        TargetApp::UnderCursor => platform::bundle_id_at(p).or_else(platform::frontmost_bundle_id),
    }
}

/// Remembers the target app for this gesture (the UI and `select` read it back from `side`).
pub(super) fn remember_target(bundle: Option<String>) {
    let forget = cfg().side_forget;
    side::set_target(bundle, forget);
}

/// (profile for HOLD, the app's own profile for DOUBLE-CLICK)
pub(super) fn resolve_profiles(p: Point) -> (usize, Option<usize>) {
    let bundle = target_bundle_at(p);
    remember_target(bundle.clone());
    let (merged, general, own) = {
        let c = cfg();
        match c.app_gesture {
            AppGesture::Merged => (true, c.profile_for(bundle.as_deref()), None),
            AppGesture::DoubleClick => (
                false,
                c.global_profile(),
                c.app_profile_for(bundle.as_deref()),
            ),
        }
    };
    // which profile a press resolves to is the first thing to check when "the wrong pie opened";
    // the app itself is never logged, only whether one was found
    diag::log!(
        "tap",
        "resolve",
        target_found = bundle.is_some(),
        merged = merged,
        profile = general,
        app_profile = own
    );
    (general, own)
}
