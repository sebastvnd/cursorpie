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
use crate::update::{Status, Version, install};
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

/// The Flow tab taking the whole display (see `App::toggle_flow_full`): what the window looked like
/// before, so leaving full screen puts it back.
#[derive(Clone, Copy, Debug)]
pub struct FlowFull {
    /// The size of the card before.
    pub card: egui::Vec2,
    /// Where the window was before (its outer top left corner), when the system said.
    pub outer: Option<egui::Pos2>,
}

/// The pages of the Behavior tab: its panels, grouped by what they are about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BehaviorPage {
    /// What you press: the trigger, the side buttons, the trackpad
    #[default]
    Input,
    /// How the pie looks and the notice on the screen
    Look,
    /// Which pie each app gets and how apps are found
    Apps,
    /// Startup and memory
    System,
}

impl BehaviorPage {
    /// Every page, in the order of the switch.
    pub const ALL: [BehaviorPage; 4] = [
        BehaviorPage::Input,
        BehaviorPage::Look,
        BehaviorPage::Apps,
        BehaviorPage::System,
    ];

    /// The name on the switch.
    pub fn label(self) -> &'static str {
        match self {
            BehaviorPage::Input => "Input",
            BehaviorPage::Look => "Look",
            BehaviorPage::Apps => "Apps",
            BehaviorPage::System => "System",
        }
    }

    /// The icon on the switch.
    pub fn icon(self) -> char {
        match self {
            BehaviorPage::Input => crate::ui::symbols::MOUSE_SIMPLE,
            BehaviorPage::Look => crate::ui::symbols::PAINT_BRUSH,
            BehaviorPage::Apps => crate::ui::symbols::APP_WINDOW,
            BehaviorPage::System => crate::ui::symbols::CPU,
        }
    }

    /// One line under the switch that says what is on the page.
    pub fn hint(self) -> &'static str {
        match self {
            BehaviorPage::Input => "The trigger, the side buttons and the trackpad.",
            BehaviorPage::Look => "How the pie looks, its middle and the notice on screen.",
            BehaviorPage::Apps => "Which pie each app gets, how apps are found, desktops.",
            BehaviorPage::System => "Startup and memory.",
        }
    }
}

/// The list of blocks that the right button opens on the canvas of the Flow tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowMenu {
    /// Where the pointer was, in the window: the list opens here.
    pub at: egui::Pos2,
    /// The same place on the canvas: the block that is picked goes there.
    pub scene: egui::Pos2,
    /// The room the list took last frame, to tell a click inside it from one outside.
    pub rect: egui::Rect,
    /// An output with nothing plugged in that the block is to be wired from (the "+" at the end of
    /// it); `None` for a block added with the right button.
    pub from: Option<(u32, crate::config::Port)>,
}

/// How the Flow tab shows the flow being edited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlowView {
    /// A list of steps, one under the other: the way most people think of a sequence.
    #[default]
    Steps,
    /// The canvas of blocks and wires, which can draw any graph.
    Canvas,
}

/// The sheet that opens over the Flow tab when **New flow** is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowNew {
    /// The choice: build the flow by hand, or with the help of an AI agent.
    Choose,
    /// The AI page: describe the flow, then copy the kit for the agent (see
    /// [`config::flow_kit`](crate::config::flow_kit)).
    Ai,
}

/// Where the kit for an AI agent stands (see [`FlowNew::Ai`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum KitStatus {
    /// Nothing asked for yet.
    #[default]
    Idle,
    /// A worker thread is making the zip and putting it on the clipboard.
    Working,
    /// The zip is at the path and on the clipboard.
    Copied(std::path::PathBuf),
    /// The zip is at the path, but no program could put it on the clipboard.
    NotCopied(std::path::PathBuf),
    /// The zip could not be made; the reason.
    Failed(String),
}

impl KitStatus {
    /// Where the zip is, once it was written.
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            KitStatus::Copied(p) | KitStatus::NotCopied(p) => Some(p),
            KitStatus::Idle | KitStatus::Working | KitStatus::Failed(_) => None,
        }
    }

    /// The sentence for the status bar and the sheet; empty for [`KitStatus::Idle`].
    pub fn notice(&self) -> String {
        match self {
            KitStatus::Idle => String::new(),
            KitStatus::Working => "Preparing the kit\u{2026}".into(),
            KitStatus::Copied(_) => {
                "Kit copied. Paste it into your AI agent and add a prompt such as \
                 \"make this\"."
                    .into()
            }
            KitStatus::NotCopied(_) => {
                "The kit is ready, but nothing could put it on the clipboard. Use Show in \
                 folder or Copy path."
                    .into()
            }
            KitStatus::Failed(why) => why.clone(),
        }
    }
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
    /// Behavior tab: the page that is shown
    pub behavior_page: BehaviorPage,
    pub profile: usize,
    pub recording: Option<Rec>,
    /// Follows the modifier keys of a shortcut being recorded in a Wayland session.
    pub(super) key_rec: super::wayland_keys::Recorder,
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
    /// Flow tab: the list of steps or the canvas
    pub flow_view: FlowView,
    /// Flow tab: the "New flow" sheet that is open over the page, if any
    pub flow_new: Option<FlowNew>,
    /// Flow tab: the panel designer that is open over the page, if any
    pub panel_design: Option<super::panel_designer::PanelDesign>,
    /// Flow tab: set while the Flow tab fills the whole display (the sidebar is hidden then)
    pub flow_full: Option<FlowFull>,
    /// Flow tab: the full screen button was pressed; `settings_ui` carries it out
    pub flow_full_req: bool,
    /// Flow tab: what the person typed to say which flow they want (at most
    /// `config::flow_kit::MAX_REQUEST_CHARS` characters); it goes into the kit for the AI agent
    pub flow_request: String,
    /// Flow tab: where the kit for the AI agent stands
    pub flow_kit: KitStatus,
    /// Flow tab: whether the list of flows is out (it slides over the page when the pointer goes
    /// to the left edge)
    pub flow_side: bool,
    /// Flow tab: when (egui clock, seconds) the pointer left the list of flows that is out; it
    /// goes back a moment after that, not at once
    pub flow_side_leave: Option<f64>,
    /// Flow tab: the room the list of flows took last frame
    pub flow_side_rect: egui::Rect,
    /// Flow tab: the words typed in the search box of the "add a step" list
    pub flow_search: String,
    /// Flow tab: flows (by id) whose steps were edited in the list since their canvas was last
    /// lined up. Blocks added from the list have no place on the canvas yet; the canvas gives
    /// them one the first time it is opened.
    pub flow_untidy: std::collections::BTreeSet<String>,
    /// Flow tab: the selected block
    pub flow_sel: Option<u32>,
    /// Flow tab: the inspector (at the right of the canvas) stays open without a selected block
    /// (the button of the canvas bar)
    pub flow_insp: bool,
    /// Flow tab: the inspector shows the variables of the flow instead of the selected block
    pub flow_insp_vars: bool,
    /// Flow tab: the block that was selected when the inspector last looked, so a new selection
    /// brings it back to the block
    pub flow_insp_seen: Option<u32>,
    /// Flow tab: the variable being followed: the blocks that set it and read it are marked on
    /// the canvas and the way its value travels is drawn
    pub flow_var: Option<String>,
    /// Flow tab: a block the canvas has to bring to the middle of its view on the next frame
    pub flow_focus: Option<u32>,
    /// Flow tab: the block and output a wire is being dragged from
    pub flow_wire: Option<(u32, crate::config::Port)>,
    /// Flow tab: the list of blocks the right button opened on the canvas, while it is open
    pub flow_menu: Option<FlowMenu>,
    /// Flow tab: the blocks that could not do their work in the last run of each flow (by flow
    /// id), with the reason; the editor marks them until the flow runs well again
    pub flow_marks: std::collections::BTreeMap<String, Vec<(u32, String)>>,
    /// What the About tab says about updates. Kept across Settings sessions: a check that is still
    /// running when Settings closes reports into it, and the next opening starts from what the last
    /// check found (see `App::update_on_settings_open`).
    pub update: Status,
    /// The newer release the "update available" sheet is offering, while that sheet is open
    pub update_offer: Option<Version>,
    /// Whether this copy is updated in place (see `update::install::installable_here`), found when
    /// Settings opens: decides between *Install update* and *Download page*
    pub update_can_install: bool,
    /// The downloaded and checked update that waits for the restart
    pub update_staged: Option<install::Staged>,
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
            behavior_page: BehaviorPage::default(),
            profile: 0,
            recording: None,
            key_rec: super::wayland_keys::Recorder::default(),
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
            flow_view: FlowView::default(),
            flow_new: None,
            panel_design: None,
            flow_full: None,
            flow_full_req: false,
            flow_request: String::new(),
            flow_kit: KitStatus::Idle,
            flow_search: String::new(),
            flow_side: false,
            flow_side_leave: None,
            flow_side_rect: egui::Rect::NOTHING,
            flow_untidy: std::collections::BTreeSet::new(),
            flow_sel: None,
            flow_insp: false,
            flow_insp_vars: false,
            flow_insp_seen: None,
            flow_var: None,
            flow_focus: None,
            flow_wire: None,
            flow_menu: None,
            flow_marks: std::collections::BTreeMap::new(),
            update: Status::default(),
            update_offer: None,
            update_can_install: false,
            update_staged: None,
        }
    }

    /// Reads the icons folder again (the "Mine" page): the images to show and how many were left out.
    pub fn reload_user_icons(&mut self) {
        let found = super::icon_catalog::scan_user_icons();
        self.user_icons = found.icons;
        self.icons_not_listed = found.not_listed;
    }

    /// Whether a sheet (the "Add a slice" panel, the icon picker, a profile dialog or the update
    /// offer) is open over the window.
    pub fn sheet_open(&self) -> bool {
        self.adding
            || self.picking_icon
            || self.profile_sheet.is_some()
            || self.flow_new.is_some()
            || self.panel_design.is_some()
            || self.update_offer.is_some()
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
        self.flow_new = None;
        self.panel_design = None;
        self.update_offer = None;
        was
    }

    /// Lets go of what the Flow tab holds for the flows of the working copy, which is dropped when
    /// Settings closes or the UI sleeps (see `App::release_editor`): the selection, the wire being
    /// dragged, the variable being followed and the search text. Kept on purpose: the marks of the
    /// last runs and `flow_untidy` (both belong to the saved flows, not to this session), and the
    /// kit request, which the person may come back to.
    pub fn release_flow_state(&mut self) {
        self.flow_sel = None;
        self.flow_wire = None;
        self.flow_focus = None;
        self.flow_var = None;
        self.flow_insp = false;
        self.flow_insp_vars = false;
        self.flow_insp_seen = None;
        self.flow_mid = false;
        self.flow_search = String::new();
        self.flow_side = false;
        self.flow_side_leave = None;
    }

    /// Closes the list of blocks of the canvas. Returns whether it was open.
    pub fn close_flow_menu(&mut self) -> bool {
        self.flow_menu.take().is_some()
    }

    /// Stops recording a shortcut (also disarms the event tap).
    pub fn stop_recording(&mut self) {
        RECORDING.store(false, Ordering::SeqCst);
        self.recording = None;
        self.key_rec.reset();
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
        self.flow_menu = None;
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
    fn the_update_offer_is_a_sheet_that_esc_closes() {
        let mut ed = Ed::new();
        assert_eq!(ed.update, Status::Idle);
        ed.update_offer = Version::parse("0.9.0");
        assert!(ed.sheet_open());
        assert!(ed.close_sheet() && !ed.sheet_open());
        assert!(ed.update_offer.is_none());
    }

    #[test]
    fn the_new_flow_sheet_is_a_sheet_that_esc_closes() {
        let mut ed = Ed::new();
        ed.flow_new = Some(FlowNew::Ai);
        assert!(ed.sheet_open());
        assert!(ed.close_sheet() && !ed.sheet_open());
        assert!(ed.flow_new.is_none());
    }

    #[test]
    fn the_kit_status_knows_where_the_zip_is_only_once_it_was_written() {
        let path = std::path::PathBuf::from("/tmp/k.zip");
        assert_eq!(KitStatus::Idle.path(), None);
        assert_eq!(KitStatus::Working.path(), None);
        assert_eq!(KitStatus::Failed("no".into()).path(), None);
        assert_eq!(KitStatus::Copied(path.clone()).path(), Some(path.as_path()));
        assert_eq!(
            KitStatus::NotCopied(path.clone()).path(),
            Some(path.as_path())
        );
        assert!(KitStatus::Idle.notice().is_empty());
        assert!(KitStatus::Copied(path).notice().contains("Paste"));
        assert_eq!(KitStatus::Failed("no".into()).notice(), "no");
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
