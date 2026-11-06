//! Editor-only state: which tab is open, which profile is selected, which
//! shortcut slot is being recorded, and the icon texture cache.
//!
//! The texture cache lives here because both the pie and Settings reach it through
//! `App::ed`; it is an [`IconCache`] (bounded, kept across Settings sessions).

use crate::config::backup::Backup;
use crate::config::{Config, Item, blank_items, items_at, presets, quick_actions};
use crate::mouse::Scan;
use crate::tap::RECORDING;
use crate::ui::icons::{IconCache, IconState};
use eframe::egui;
use std::sync::atomic::Ordering;

/// The tabs of the Settings window.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Menu,
    Flow,
    Behavior,
    General,
    About,
}

/// Which action field of an item is being recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Back,
    Forward,
    Run,
    /// An extra button of the mouse (button number 5 and up) of a side-button mode.
    Extra(i64),
    /// The shortcut block `.0` of the flow being edited in the Flow tab (the flow is
    /// `Rec::profile`, the path is empty).
    Node(u32),
}

/// The action field currently recording a shortcut.
pub struct Rec {
    pub profile: usize,
    pub path: Vec<usize>,
    pub slot: Slot,
}

/// The ready-made slices of the "Add a slice" panel, grouped under a title.
///
/// Built when the panel opens and dropped when it closes (see `Ed::add_catalog`): the panel is
/// redrawn every frame, and rebuilding these lists with all their strings that often is wasted work.
pub struct AddCatalog {
    /// Whether the groups were built for a ring that may hold a submenu.
    pub can_nest: bool,
    pub groups: Vec<(&'static str, Vec<Item>)>,
}

impl AddCatalog {
    /// The groups of the panel; `can_nest` = the ring is not yet at the maximum depth.
    pub fn new(can_nest: bool) -> Self {
        Self {
            can_nest,
            groups: vec![
                ("Quick actions", quick_actions()),
                ("Side buttons", presets()),
                ("Custom", blank_items(can_nest)),
            ],
        }
    }
}

/// The dialogs of the profile toolbar (Menu tab). Each one is a centered sheet over the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSheet {
    /// Name and app of a new profile.
    New,
    /// The name of the current profile.
    Rename,
    /// The apps the current profile applies to.
    Apps,
    /// The list of installed apps, searchable; it comes from one of the two dialogs above and goes
    /// back to it.
    Pick(PickFor),
}

/// Which dialog the app list was opened from, and so where its answer goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickFor {
    /// The app of a new profile.
    New,
    /// One more app for the current profile.
    Add,
    /// The app of an "Open app" action of a slice (see [`ActionTarget`]); it goes back to the editor.
    Action,
    /// The app of an "App" block of a flow (see [`Ed::flow_target`]); it goes back to the Flow tab.
    Flow,
}

/// The action field of a slice that the app list is answering (set by the **Pick...** button of the
/// slice editor, used while the list is open with [`PickFor::Action`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionTarget {
    pub profile: usize,
    pub path: Vec<usize>,
    pub slot: Slot,
}

/// Where the list of installed apps stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppScan {
    /// Not asked for yet.
    #[default]
    Idle,
    /// A background thread is reading it (the list shown, if any, is the previous one).
    Reading,
    /// Read.
    Done,
}

/// Editor-only state (kept apart from the edited config so both can be borrowed at once).
pub struct Ed {
    pub tab: Tab,
    pub profile: usize,
    pub recording: Option<Rec>,
    pub icons: IconCache,
    pub new_app: String,
    /// the "New profile" form: name and the app the profile belongs to
    pub new_profile_name: String,
    pub new_profile_app: String,
    /// the profile dialog that is open over the window, if any
    pub profile_sheet: Option<ProfileSheet>,
    /// the installed apps the app list shows, and what is being typed in its search box
    pub installed: Vec<crate::platform::installed::InstalledApp>,
    pub installed_scan: AppScan,
    pub app_query: String,
    /// The action field the app list is choosing an app for
    /// while the picker is open for an action.
    pub action_target: Option<ActionTarget>,
    pub nav: Vec<usize>,
    pub sel: Option<usize>,
    pub adding: bool,
    /// the slice of the ring that is being dragged in the pie (to move it, or to the trash)
    pub drag: Option<usize>,
    /// where the trash button of the slice editor was drawn last frame, the drop target of a drag
    pub trash_rect: Option<egui::Rect>,
    /// the tiles of the "Add a slice" panel; `Some` only while the panel is showing
    pub add_catalog: Option<AddCatalog>,
    /// the icon picker is open (it temporarily replaces the inspector)
    pub picking_icon: bool,
    pub icon_query: String,
    /// 0 = all, 1..=N = category, N+1 = the user's own icons
    pub icon_cat: usize,
    pub user_icons: Vec<String>,
    /// images in the icons folder that `user_icons` leaves out (too big, or over the limit)
    pub icons_not_listed: usize,
    /// backups of `config.json` found on disk (read when the General tab is opened)
    pub backups: Vec<Backup>,
    /// A scan of the mouse is running (General tab); dropping it ends the scan
    pub scan: Option<Scan>,
    /// The window's rectangle of the last frame: sheets are centered over it and dim it
    pub card: egui::Rect,
    /// Flow tab: the flow being edited (index into `Config::flows`)
    pub flow: usize,
    /// Flow tab: how far the canvas is scrolled
    pub flow_pan: egui::Vec2,
    /// Flow tab: how far the canvas is zoomed (1.0 = actual size)
    pub flow_zoom: f32,
    /// Flow tab: the middle button is held on the canvas, so moving the mouse scrolls it
    pub flow_mid: bool,
    /// Flow tab: the "App" block (flow index, block id) the app list is choosing an app for
    pub flow_target: Option<(usize, u32)>,
    /// Flow tab: the selected block
    pub flow_sel: Option<u32>,
    /// Flow tab: the block whose output a wire is being dragged from
    pub flow_wire: Option<u32>,
}

impl Default for Ed {
    fn default() -> Self {
        Self::new()
    }
}

impl Ed {
    /// Fresh editor state: Menu tab, first profile, nothing selected.
    pub fn new() -> Self {
        Self {
            tab: Tab::Menu,
            profile: 0,
            recording: None,
            icons: IconCache::new(),
            new_app: String::new(),
            new_profile_name: String::new(),
            new_profile_app: String::new(),
            profile_sheet: None,
            installed: Vec::new(),
            installed_scan: AppScan::Idle,
            app_query: String::new(),
            action_target: None,
            nav: Vec::new(),
            sel: None,
            adding: false,
            drag: None,
            trash_rect: None,
            add_catalog: None,
            picking_icon: false,
            icon_query: String::new(),
            icon_cat: 0,
            user_icons: Vec::new(),
            icons_not_listed: 0,
            backups: Vec::new(),
            scan: None,
            card: egui::Rect::NOTHING,
            flow: 0,
            flow_pan: egui::Vec2::ZERO,
            flow_zoom: 1.0,
            flow_mid: false,
            flow_target: None,
            flow_sel: None,
            flow_wire: None,
        }
    }

    /// Reads the icons folder again (the "Mine" page): the images to show and how many were left out.
    pub fn reload_user_icons(&mut self) {
        let found = super::icon_catalog::scan_user_icons();
        self.user_icons = found.icons;
        self.icons_not_listed = found.not_listed;
    }

    /// Whether a sheet (the "Add a slice" panel, the icon picker or a profile dialog) is open over
    /// the window.
    pub fn sheet_open(&self) -> bool {
        self.adding || self.picking_icon || self.profile_sheet.is_some()
    }

    /// Closes the open sheet. Returns whether there was one.
    pub fn close_sheet(&mut self) -> bool {
        let was = self.sheet_open();
        self.profile_sheet = None;
        self.action_target = None;
        self.flow_target = None;
        self.adding = false;
        self.add_catalog = None;
        self.picking_icon = false;
        was
    }

    /// Stops recording a shortcut (also disarms the event tap).
    pub fn stop_recording(&mut self) {
        RECORDING.store(false, Ordering::SeqCst);
        self.recording = None;
    }

    /// Like [`Ed::icon_for`] for the icon of an app, but never waits for the system: while the
    /// icon is being read on a worker thread the answer is [`IconState::Pending`].
    pub fn app_icon_async(&mut self, ctx: &egui::Context, icon: &str) -> IconState {
        self.icons.get_app_async(ctx, icon)
    }

    /// Cached texture for an image icon. None for text icons or images that fail to load.
    pub fn icon_for(&mut self, ctx: &egui::Context, icon: &str) -> Option<egui::TextureHandle> {
        // The icon of an app comes from the system (on Windows: a scan of the running processes and
        // a read of the executable), which can be slow or even stall for an odd file. It is read on
        // a worker thread, so a click on "Full color" never freezes the window; the icon appears
        // as soon as it is ready (`get_app_async` repaints then). Warming up (`Msg::Warm`) fills
        // the cache ahead of the pie, so the pie itself finds the texture already there.
        if crate::ui::appicon::is_app_icon(icon) {
            return match self.icons.get_app_async(ctx, icon) {
                IconState::Ready(tex) => Some(tex),
                IconState::Pending | IconState::Missing => None,
            };
        }
        self.icons.get(ctx, icon)
    }

    /// Makes the selection valid for `edit` again after the config was replaced from outside the
    /// editor (undo, redo, restoring a backup): clamps the profile, leaves a ring that no longer
    /// exists and drops a stale slice selection.
    pub fn fit_to(&mut self, edit: &Config) {
        self.stop_recording();
        self.profile = self.profile.min(edit.profiles.len().saturating_sub(1));
        self.flow = self.flow.min(edit.flows.len().saturating_sub(1));
        self.flow_wire = None;
        if let Some(f) = edit.flows.get(self.flow)
            && self.flow_sel.is_some_and(|id| f.node(id).is_none())
        {
            self.flow_sel = None;
        }
        let ring_len = edit
            .profiles
            .get(self.profile)
            .and_then(|p| items_at(&p.items, &self.nav))
            .map(<[_]>::len);
        match ring_len {
            None => self.reset_nav(),
            Some(n) => {
                if self.sel.is_some_and(|i| i >= n) {
                    self.sel = None;
                }
            }
        }
    }

    /// Back to the top ring with nothing selected.
    pub fn reset_nav(&mut self) {
        self.nav.clear();
        self.sel = None;
        self.adding = false;
        self.add_catalog = None;
        self.picking_icon = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiles(c: &AddCatalog) -> usize {
        c.groups.iter().map(|(_, list)| list.len()).sum()
    }

    #[test]
    fn the_add_catalog_offers_a_submenu_only_when_nesting_is_allowed() {
        assert_eq!(
            tiles(&AddCatalog::new(true)),
            tiles(&AddCatalog::new(false)) + 1
        );
    }

    #[test]
    fn closing_a_sheet_reports_whether_there_was_one() {
        let mut ed = Ed::new();
        assert!(!ed.sheet_open() && !ed.close_sheet());
        ed.picking_icon = true;
        assert!(ed.sheet_open());
        assert!(ed.close_sheet() && !ed.sheet_open());
    }

    #[test]
    fn leaving_the_ring_drops_the_add_catalog() {
        let mut ed = Ed::new();
        ed.adding = true;
        ed.add_catalog = Some(AddCatalog::new(true));
        ed.reset_nav();
        assert!(!ed.adding && ed.add_catalog.is_none());
    }
}
