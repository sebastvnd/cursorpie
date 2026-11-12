//! What a gesture means: which slice is hovered, which profile and app it is about, and how a
//! released slice is applied.

use crate::actions;
use crate::config::{
    self, Action, AppGesture, Item, ItemKind, MAX_DEPTH, TargetApp, cfg, items_at,
};
use crate::geometry::Point;
use crate::geometry::{pie_inner, pie_outer, slice_at};
use crate::platform;
use crate::running::{self, Choice};
use crate::side;

/// Result of testing the cursor against the pie.
pub(super) struct Hit {
    pub hover: Option<usize>,
    /// cursor crossed the rim over a submenu slice
    pub enter: bool,
    /// The cursor is over an "Open app" slice whose app already runs: which of its two badges it is
    /// on (see [`running`]). `None` over every other slice, and whenever the setting is off.
    pub choice: Option<Choice>,
}

/// `p` as a distance from `center` in the points the pie is drawn in, when one point of the pointer
/// is `scale` of them (see [`platform::window_scale`]). `p` itself when the scale is 1, which is
/// every system but Linux in a Wayland session whose scale differs from XWayland's.
fn in_pie_points(center: Point, p: Point, scale: f64) -> Point {
    if (scale - 1.0).abs() < 1e-9 {
        return p;
    }
    Point::new(
        center.x + (p.x - center.x) * scale,
        center.y + (p.y - center.y) * scale,
    )
}

/// Which slice is under `p`, and whether `p` crossed the outer rim over a submenu slice.
pub(super) fn eval(profile: usize, path: &[usize], center: Point, p: Point) -> Hit {
    let p = in_pie_points(center, p, platform::window_scale(center.x, center.y));
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
            choice: None,
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
    // the cache is asked with the config locked: lock order is config, then running
    let choice = hover
        .and_then(|h| items.get(h))
        .and_then(|i| match &i.kind {
            ItemKind::Run(a) => a.open_choice_key(c.browser_bridge),
            _ => None,
        })
        .and_then(|app| {
            running::choose(
                c.open_app_choice,
                Some(app.as_str()),
                center,
                p,
                hover.unwrap_or(0),
                items.len(),
            )
        });
    Hit {
        hover,
        enter,
        choice,
    }
}

/// Applies the slice the user released over. `choice` is what the pointer was on when the slice had
/// two badges (see [`Hit::choice`]).
pub(super) fn select(profile: usize, path: &[usize], index: usize, choice: Option<Choice>) {
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
        Some(ItemKind::Mode { .. }) => pick_mode(profile, path, index),
        Some(ItemKind::Run(action)) => run_action(action, side::target(), choice),
        _ => {}
    }
}

/// Makes the *Mode* slice at `path` + `index` the active mode of `profile`, and, if the settings
/// say so, makes the side buttons follow the pie it was picked in. The caller has checked that the
/// slice is a mode.
pub(super) fn pick_mode(profile: usize, path: &[usize], index: usize) {
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

/// Whether the slice at `path` + `index` of `profile` is a *Mode* slice.
pub(super) fn is_mode(profile: usize, path: &[usize], index: usize) -> bool {
    let c = cfg();
    c.profiles
        .get(profile)
        .and_then(|pr| items_at(&pr.items, path))
        .and_then(|items| items.get(index))
        .is_some_and(|i| matches!(i.kind, ItemKind::Mode { .. }))
}

/// Runs an action. If the settings say so, the target app is brought to the front first, because
/// shortcuts always go to the app in front.
pub(super) fn run_action(action: Action, target: Option<String>, choice: Option<Choice>) {
    let focus = focus_for(target);
    actions::run_in_with(focus, action, choice);
}

/// [`run_action`] for the back and forward buttons: the notice on the screen stays quiet, they are
/// used all day.
pub(super) fn run_action_quiet(action: Action, target: Option<String>) {
    let focus = focus_for(target);
    actions::run_in_quiet(focus, action);
}

/// The app to bring to the front before an action that sends keys, if the settings say so.
fn focus_for(target: Option<String>) -> Option<String> {
    let focus = {
        let c = cfg();
        c.target_app == TargetApp::UnderCursor && c.focus_target
    };
    if focus { target } else { None }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scale_of_one_leaves_the_pointer_alone() {
        let (c, p) = (Point::new(100.0, 100.0), Point::new(130.3, 90.7));
        assert_eq!(in_pie_points(c, p, 1.0), p, "to the last bit");
    }

    #[test]
    fn a_distance_is_counted_in_the_points_the_pie_is_drawn_in() {
        // X shows the screen at 1.5 times the points of the compositor: 20 compositor points from
        // the center are 30 points of the pie
        let (c, p) = (Point::new(100.0, 100.0), Point::new(120.0, 80.0));
        assert_eq!(in_pie_points(c, p, 1.5), Point::new(130.0, 70.0));
        assert_eq!(
            in_pie_points(c, c, 1.5),
            c,
            "the center is the center at any scale"
        );
    }
}
