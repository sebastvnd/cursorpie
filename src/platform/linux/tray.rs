//! No tray icon on Linux yet (see `docs/developer/LINUX_MIGRATION.md`): `build_tray` reports none.

use crate::msg::Notifier;

/// There is no tray icon yet; the handles never exist.
pub struct TrayHandles;

impl TrayHandles {
    /// Never ticked.
    pub fn pause_checked(&self) -> bool {
        false
    }
    /// Never ticked.
    pub fn login_checked(&self) -> bool {
        false
    }
    /// Ignored.
    pub fn set_login_checked(&self, _on: bool) {}
}

/// No tray here.
pub fn build_tray(_n: &Notifier) -> Option<TrayHandles> {
    None
}
