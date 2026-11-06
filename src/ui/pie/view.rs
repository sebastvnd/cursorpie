//! The data one pie frame needs, copied out of the config.

use crate::config::{AppIconStyle, Config, IconShadow, ItemKind, item_at, items_at};
use crate::side;
use crate::ui::appicon;

/// Snapshot of what to draw, so the config lock is not held while painting.
pub(super) struct PieView {
    /// (icon, name, is_submenu)
    pub(super) items: Vec<(String, String, bool)>,
    /// Per slice of `items`: the app it opens, when it is an "Open app" slice and the choice
    /// between a new window and the open app is on (see [`running`](crate::running)). All `None`
    /// when the setting is off.
    pub(super) open_apps: Vec<Option<String>>,
    /// index (in this ring) of the active side-button mode
    pub(super) active: Option<usize>,
    pub(super) center_icon: String,
    /// draw each slice's name under its icon
    pub(super) labels: bool,
    pub(super) accent: [u8; 3],
    pub(super) settle_ms: f32,
    pub(super) is_app: bool,
    /// does the side-button indicator apply, and does the side button follow this pie?
    pub(super) side: side::View,
    pub(super) inactive: [u8; 3],
    /// icon string (`app:<bundle id>`, or `appcolor:` for its own colors) of the app this pie
    /// belongs to, for the center disc
    pub(super) app_icon: Option<String>,
    /// whether that icon is drawn in its own colors (instead of one color, in the ink of the pie)
    pub(super) app_icon_color: bool,
    /// when the app's icon gets a shadow
    pub(super) app_icon_shadow: IconShadow,
}

impl PieView {
    /// Copies what is needed to draw ring `path` of `profile`. Call it with the config locked, then release the lock.
    pub(super) fn new(c: &Config, profile: usize, path: &[usize]) -> Self {
        let prof = c.profiles.get(profile);
        let ring = prof.and_then(|p| items_at(&p.items, path)).unwrap_or(&[]);
        let items = ring
            .iter()
            .map(|i| {
                (
                    i.icon.clone(),
                    i.name.clone(),
                    matches!(i.kind, ItemKind::Submenu(_)),
                )
            })
            .collect();
        let open_apps = ring
            .iter()
            .map(|i| match &i.kind {
                ItemKind::Run(a) if c.open_app_choice => a.open_target().map(str::to_string),
                _ => None,
            })
            .collect();
        let active = prof.and_then(|p| {
            (p.active.len() == path.len() + 1 && p.active[..path.len()] == *path)
                .then(|| p.active[path.len()])
        });
        let center_icon = if path.is_empty() {
            prof.and_then(|p| p.active_item()).map(|i| i.icon.clone())
        } else {
            prof.and_then(|p| item_at(&p.items, path))
                .map(|i| i.icon.clone())
        }
        .unwrap_or_default();
        // the center of an app's own pie shows that app's icon (top level only)
        let target = side::target();
        let app_icon = (path.is_empty() && c.center_app_icon)
            .then(|| {
                prof.and_then(|p| appicon::bundle_for(&p.apps, target.as_deref()))
                    .map(|b| appicon::icon_string(c.app_icon_style, b))
            })
            .flatten();
        Self {
            items,
            open_apps,
            active,
            center_icon,
            labels: c.show_labels,
            accent: c.accent,
            settle_ms: c.settings_hold_ms.saturating_sub(c.pie_hold_ms) as f32,
            is_app: prof.is_some_and(|p| !p.apps.is_empty()),
            side: side::view_of(c, profile),
            inactive: c.inactive_color,
            app_icon,
            app_icon_color: c.app_icon_style == AppIconStyle::Color,
            app_icon_shadow: c.app_icon_shadow,
        }
    }
}
