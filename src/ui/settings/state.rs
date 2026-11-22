//! Editor-only state: which tab is open, which profile is selected, which
//! shortcut slot is being recorded, and the icon texture cache.
//!
//! The texture cache lives here because both the pie and Settings reach it through
//! `App::ed`; it is an [`IconCache`] (bounded, kept across Settings sessions).

use crate::config::backup::Backup;
use crate::config::flow::Flow;
use crate::config::{Action, Config, Item, blank_items, items_at, presets, quick_actions};
use crate::mouse::Scan;
use crate::tap::RECORDING;
use crate::ui::icons::{IconCache, IconState};
use crate::update::{Outcome, Status, Version, install};
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
    /// Learning and the intelligent pie
    Smart,
    /// Startup and memory (always the last page: it is the far end of the switch)
    System,
}

impl BehaviorPage {
    /// Every page, in the order of the switch.
    pub const ALL: [BehaviorPage; 5] = [
        BehaviorPage::Input,
        BehaviorPage::Look,
        BehaviorPage::Apps,
        BehaviorPage::Smart,
        BehaviorPage::System,
    ];

    /// The name on the switch.
    pub fn label(self) -> &'static str {
        match self {
            BehaviorPage::Input => "Input",
            BehaviorPage::Look => "Look",
            BehaviorPage::Apps => "Apps",
            BehaviorPage::System => "System",
            BehaviorPage::Smart => "Intelligent",
        }
    }

    /// The icon on the switch.
    pub fn icon(self) -> char {
        match self {
            BehaviorPage::Input => crate::ui::symbols::MOUSE_SIMPLE,
            BehaviorPage::Look => crate::ui::symbols::PAINT_BRUSH,
            BehaviorPage::Apps => crate::ui::symbols::APP_WINDOW,
            BehaviorPage::System => crate::ui::symbols::CPU,
            BehaviorPage::Smart => crate::ui::symbols::SPARKLE,
        }
    }

    /// One line under the switch that says what is on the page.
    pub fn hint(self) -> &'static str {
        match self {
            BehaviorPage::Input => "The trigger, the side buttons and the trackpad.",
            BehaviorPage::Look => "How the pie looks, its middle and the notice on screen.",
            BehaviorPage::Apps => "Which pie each app gets, how apps are found, desktops.",
            BehaviorPage::System => "Startup and memory.",
            BehaviorPage::Smart => "The intelligent pie: what it learns and suggests.",
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

/// How the Menu tab shows the pie being edited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuView {
    /// One pie at a time with the slice editor beside it: the way the tab has always looked.
    #[default]
    Classic,
    /// The pie editor: the whole tree of rings on a canvas (every submenu is a pie of its own,
    /// wired to its slice), like the canvas of the Flow tab. See `tabs::pie_editor`.
    Editor,
}

/// How much of the width the imported pie takes when the import tray opens: a third, so your own pie
/// (the one being edited) keeps the larger part.
pub const IMPORT_SPLIT: f32 = 0.3;

/// How far a canvas of pies is moved and zoomed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PieView {
    /// How far the canvas is scrolled, from the top left corner of its area.
    pub pan: egui::Vec2,
    /// How far it is zoomed (1.0 = actual size).
    pub zoom: f32,
    /// What the view was last fitted to (the profile of the canvas); it is fitted again when that
    /// changes, or when this is `None`.
    pub fitted_for: Option<usize>,
}

impl Default for PieView {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 1.0,
            fitted_for: None,
        }
    }
}

/// A slice that is being dragged in the pie editor or the import tray.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PieDrag {
    /// The slice comes from the imported pie (it is copied) and not from yours (it is moved).
    pub from_import: bool,
    /// The ring it is in.
    pub ring: Vec<usize>,
    /// Its place in that ring.
    pub idx: usize,
}

/// The list of slices to add that the **+** of the pie editor, or the right button on its canvas,
/// opens over the canvas (not a sheet: the list floats, like the one of the Flow canvas).
#[derive(Clone, Debug)]
pub struct PieMenu {
    /// Where it opens: the place of the right click, or `None` over the bar, centered on it.
    pub at: Option<egui::Pos2>,
    /// The ring a picked slice is added to.
    pub ring: Vec<usize>,
    /// Where the list was drawn last frame (a press outside it closes it).
    pub rect: egui::Rect,
    /// The group of the list that is narrowed to (an index into `AddCatalog::groups`), if any.
    pub group: Option<usize>,
    /// What the person typed in the search box.
    pub search: String,
}

/// The **import tray**: a pie file that was opened but not added to anything. The window shows it
/// next to your pie, and slices are dragged from one into the other (see `tabs::pie_editor`). Nothing
/// is written to the icons folder or added to the flows until a slice is taken, and then only what
/// that slice needs.
pub struct PieImport {
    /// The name of the file, for the title of its half.
    pub label: String,
    /// What the file holds. The profile in it is never changed: it is the source.
    pub bundle: Box<crate::config::share::Bundle>,
    /// The slice that is selected in the imported pie: its ring and its place.
    pub sel: Option<(Vec<usize>, usize)>,
    /// The slices picked together (select mode, Shift or Ctrl/Cmd + click, select all), as paths: the
    /// ring and the place, like [`pie_edit::slices_at`](crate::config::pie_edit::slices_at) wants
    /// them. A picked submenu goes with everything in it. When it is not empty, *Add selected*
    /// takes these and not `sel`.
    pub picked: std::collections::BTreeSet<Vec<usize>>,
    /// Select mode: a plain click picks or unpicks a slice and nothing is dragged, so a click can
    /// never move a slice by accident.
    pub select_mode: bool,
    /// Where the imported pie is on its half of the window.
    pub view: PieView,
    /// The images of the file as textures, by icon string; `None` for an icon that is not an image
    /// of the file (text, a symbol or an app), which the usual cache draws.
    pub textures: std::collections::HashMap<String, Option<egui::TextureHandle>>,
    /// Whether anything in the file runs shell commands, writes files or sends messages.
    pub runs_commands: bool,
}

impl PieImport {
    /// A pie file that waits in the tray: nothing selected, nothing picked.
    pub fn new(bundle: crate::config::share::Bundle, label: String, runs_commands: bool) -> Self {
        Self {
            label,
            bundle: Box::new(bundle),
            sel: None,
            picked: std::collections::BTreeSet::new(),
            select_mode: false,
            view: PieView::default(),
            textures: std::collections::HashMap::new(),
            runs_commands,
        }
    }
}

/// The sheet that opens over the Flow tab when **New flow** is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowNew {
    /// The choice: build the flow by hand, or with the help of an AI agent.
    Choose,
    /// The AI page: describe the flow, then copy the kit for the agent (see
    /// [`config::flow_kit`](crate::config::flow_kit)).
    Ai,
    /// The templates: complete flows for the browser extension to start from (see
    /// [`config::flow_templates`](crate::config::flow_templates)).
    Templates,
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
    /// The groups of the panel; `can_nest` = the ring is not yet at the maximum depth, `flows` are
    /// the flows of the config (each one is offered as a slice that runs it).
    pub fn new(can_nest: bool, flows: &[Flow]) -> Self {
        Self {
            can_nest,
            groups: vec![
                ("Quick actions", quick_actions()),
                ("Side buttons", presets()),
                ("Flows", flow_items(flows)),
                ("Custom", blank_items(can_nest)),
            ],
        }
    }
}

/// The slices of the "Flows" group: one that runs the flow the person picks in the editor (it starts
/// without one), then one per flow of the config, named like it.
fn flow_items(flows: &[Flow]) -> Vec<Item> {
    let mut v = vec![Item::run("Flow", "ico:play", Action::Flow(String::new()))];
    v.extend(flows.iter().map(|f| {
        let name = f.name.trim();
        Item::run(
            if name.is_empty() { "Flow" } else { name },
            "ico:play",
            Action::Flow(f.id.clone()),
        )
    }));
    v
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
    /// The list of sites CursorPie knows, searchable, for the sites the intelligent pie never
    /// learns (Behavior > Intelligent; drawn by that page).
    Site,
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
    /// One more app the intelligent pie never learns (Behavior > Intelligent).
    Ignore,
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
    /// Behavior > Intelligent: what is typed in the list of sites to ignore (the sheet "Choose a
    /// site")
    pub smart_new_site: String,
    /// Behavior > Intelligent: the pie the *Add* button of the intelligent slice adds it to
    pub smart_add_to: usize,
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
    /// The sheet "Icon from a website" (see `web_icon`)
    pub web_icon: super::web_icon::WebIcon,
    /// Counts the times Settings was opened. It salts the ids of everything drawn in Settings, so
    /// each opening starts with every scroll area at the top (egui keeps the scroll offset of an id
    /// for ever, even while the window is closed) instead of only the tab and the profile going
    /// back to their first value.
    pub session: u64,
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
    /// Menu tab: classic pie and slice editor, or the pie editor (a canvas)
    pub menu_view: MenuView,
    /// Menu tab, pie editor: where the canvas of your pie is
    pub pie_view: PieView,
    /// Menu tab, pie editor: the slice editor stays open at the right without a selected slice
    /// (the button of the bar)
    pub pie_insp: bool,
    /// Menu tab, pie editor and import tray: the slice being dragged
    pub pie_drag: Option<PieDrag>,
    /// Menu tab, pie editor: the pies (rings, by the path of the submenu that opens them) that are
    /// selected. The top pie can be selected but not moved: the others are placed around it.
    pub pie_picked: std::collections::BTreeSet<Vec<usize>>,
    /// Menu tab, pie editor: where the selection box started, in the window, while one is dragged
    pub pie_marquee: Option<egui::Pos2>,
    /// Menu tab, pie editor: what was selected when the box started with Shift or Ctrl/Cmd held
    pub pie_marquee_base: std::collections::BTreeSet<Vec<usize>>,
    /// Menu tab, pie editor: the middle of a pie is being dragged, so the selected pies move
    pub pie_moving: bool,
    /// Menu tab, pie editor: the list of slices to add, while it is open
    pub pie_menu: Option<PieMenu>,
    /// Menu tab, import tray: the imported pie that waits to be dragged into yours, if any. While
    /// there is one, the tab is split in two halves.
    pub pie_import: Option<PieImport>,
    /// Menu tab, import tray: how much of the width the imported half takes (0.25 to 0.75)
    pub pie_split: f32,
    /// Flow tab: the selected block
    pub flow_sel: Option<u32>,
    /// Flow tab: the blocks selected together (a drag over the empty canvas, Shift or Ctrl/Cmd +
    /// click, Ctrl/Cmd + A). Only used for two blocks or more, and then `flow_sel` is `None`: one
    /// selected block is `flow_sel` alone, as before (the inspector shows that one).
    pub flow_multi: std::collections::BTreeSet<u32>,
    /// Flow tab: where the selection box started, on the canvas, while one is being dragged
    pub flow_marquee: Option<egui::Pos2>,
    /// Flow tab: what was already selected when the selection box started with Shift or Ctrl/Cmd
    /// held (the box adds to it)
    pub flow_marquee_base: std::collections::BTreeSet<u32>,
    /// Flow tab: the header of a selected group is being dragged, so the whole group moves
    pub flow_group: bool,
    /// Flow tab: the inspector (at the right of the canvas) stays open without a selected block
    /// (the button of the canvas bar)
    pub flow_insp: bool,
    /// Flow tab: the inspector shows the variables of the flow instead of the selected block
    pub flow_insp_vars: bool,
    /// Flow tab: the inspector shows the searchable list of every block of the flow
    pub flow_insp_blocks: bool,
    /// Flow tab: the words typed in the search box of the inspector's variable list
    pub flow_var_search: String,
    /// Flow tab: the words typed in the search box of the inspector's list of blocks
    pub flow_block_search: String,
    /// Flow tab: the block that was selected when the inspector last looked, so a new selection
    /// brings it back to the block
    pub flow_insp_seen: Option<u32>,
    /// Flow tab: the variable being followed: the blocks that set it and read it are marked on
    /// the canvas and the way its value travels is drawn
    pub flow_var: Option<String>,
    /// Flow tab: a block the canvas has to bring to the middle of its view on the next frame
    pub flow_focus: Option<u32>,
    /// Flow tab: the middle of the visible canvas, in the coordinates of the blocks (a block added
    /// with nothing selected is put here, loose)
    pub flow_center: egui::Pos2,
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
    /// Whether the swap asks for the administrator's password (Linux, a folder only root writes to)
    pub update_needs_password: bool,
    /// Windows: whether CursorPie runs as administrator or is installed to, read when Settings
    /// opens; the About tab's *Administrator mode* panel is drawn from it (nothing elsewhere)
    pub admin_mode: crate::platform::AdminMode,
    /// The downloaded and checked update that waits for the restart
    pub update_staged: Option<install::Staged>,
    /// How the update that the last run handed to the swap ended, while the sheet that tells it is
    /// open (see `update_dialog`)
    pub update_done: Option<Outcome>,
    /// A dialog that lives in `App` rather than here is open over the window (the import, export
    /// and flow-run sheets, the quit and discard questions). `App::settings_ui` sets it at the start
    /// of every frame, so the canvases under the dialog leave the wheel, the trackpad and the keys
    /// to it (see [`Ed::sheet_open`]).
    pub dialog_open: bool,
    /// Developer mode is on (the `developer` file, read when Settings opens)
    pub dev_on: bool,
    /// The sheet of developer mode that is open (install the log build, or go back)
    pub dev_offer: Option<super::developer::Offer>,
    /// What the newest log says, for the panels of developer mode
    pub dev_snapshot: Option<crate::devmode::Snapshot>,
    /// Why the log could not be read
    pub dev_error: Option<String>,
    /// When the log was read last
    pub dev_read_at: Option<std::time::Instant>,
    /// The category the list of recent events is cut to
    pub dev_category: Option<String>,
    /// The least serious level the list of recent events shows (`None`: all of them)
    pub dev_level: Option<crate::devmode::Level>,
    /// Only the recent events that contain this text
    pub dev_search: String,
    /// The list of recent events is drawn tall (the whole window) instead of compact
    pub dev_log_big: bool,
    /// The switch of the log being edited (what to record)
    pub dev_spec: String,
    /// What the last button of the Developer panels did
    pub dev_note: Option<String>,
    /// The standard build that is being installed also turns developer mode off (set when it was
    /// asked for from "Turn off developer mode", done once the swap has started)
    pub dev_leaving: bool,
    /// Developer mode: the download of the `cpd` command is running
    pub cpd_busy: bool,
    /// What the last install or removal of `cpd` came to, shown in its panel
    pub cpd_note: Option<String>,
    /// Developer mode: the technical list of recent log lines is unfolded
    pub dev_tech_open: bool,
    /// The sheet that answers *Learn now* and *Forget everything* (Behavior > Intelligent): it
    /// opens at the click, says that CursorPie is working, and tells what came of it
    pub smart_sheet: Option<SmartSheet>,
}

/// What the sheet of *Learn now* / *Forget everything* shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SmartSheet {
    /// `true`: Forget everything; `false`: Learn now
    pub forget: bool,
    /// Still working (the lines are not there yet)
    pub busy: bool,
    /// What the work found, one point per line
    pub lines: Vec<String>,
}

impl SmartSheet {
    /// The sheet as it opens, while the worker is still busy.
    pub fn working(forget: bool) -> Self {
        Self {
            forget,
            busy: true,
            lines: Vec::new(),
        }
    }
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
            smart_new_site: String::new(),
            smart_add_to: 0,
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
            session: 0,
            add_catalog: None,
            picking_icon: false,
            web_icon: super::web_icon::WebIcon::new(),
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
            flow_center: egui::Pos2::new(240.0, 160.0),
            menu_view: MenuView::default(),
            pie_view: PieView::default(),
            pie_insp: false,
            pie_drag: None,
            pie_picked: std::collections::BTreeSet::new(),
            pie_marquee: None,
            pie_marquee_base: std::collections::BTreeSet::new(),
            pie_moving: false,
            pie_menu: None,
            pie_import: None,
            pie_split: IMPORT_SPLIT,
            flow_sel: None,
            flow_multi: std::collections::BTreeSet::new(),
            flow_marquee: None,
            flow_marquee_base: std::collections::BTreeSet::new(),
            flow_group: false,
            flow_insp: false,
            flow_insp_vars: false,
            flow_insp_blocks: false,
            flow_var_search: String::new(),
            flow_block_search: String::new(),
            flow_insp_seen: None,
            flow_var: None,
            flow_focus: None,
            flow_wire: None,
            flow_menu: None,
            flow_marks: std::collections::BTreeMap::new(),
            update: Status::default(),
            update_offer: None,
            update_can_install: false,
            update_needs_password: false,
            admin_mode: crate::platform::AdminMode::default(),
            update_staged: None,
            update_done: None,
            dialog_open: false,
            dev_on: false,
            dev_offer: None,
            dev_snapshot: None,
            dev_error: None,
            dev_read_at: None,
            dev_category: None,
            dev_level: None,
            dev_search: String::new(),
            dev_log_big: false,
            dev_spec: String::new(),
            dev_note: None,
            dev_leaving: false,
            cpd_busy: false,
            cpd_note: None,
            dev_tech_open: true,
            smart_sheet: None,
        }
    }

    /// Reads the icons folder again (the "Mine" page): the images to show and how many were left out.
    pub fn reload_user_icons(&mut self) {
        let found = super::icon_catalog::scan_user_icons();
        self.user_icons = found.icons;
        self.icons_not_listed = found.not_listed;
    }

    /// Whether a sheet (the "Add a slice" panel, the icon picker, a profile dialog or the update
    /// offer) or a dialog of the app (see [`Ed::dialog_open`]) is open over the window. What is
    /// under it must not react to the wheel, the trackpad or shortcuts.
    pub fn sheet_open(&self) -> bool {
        self.dialog_open || self.own_sheet_open()
    }

    /// The sheets that `Ed` itself holds and [`Ed::close_sheet`] can close.
    fn own_sheet_open(&self) -> bool {
        self.adding
            || self.picking_icon
            || self.web_icon.open
            || self.profile_sheet.is_some()
            || self.flow_new.is_some()
            || self.panel_design.is_some()
            || self.update_offer.is_some()
            || self.update_done.is_some()
            || self.dev_offer.is_some()
            || self.smart_sheet.is_some()
            || matches!(self.update, Status::Downloading(_))
    }

    /// Closes the open sheet. Returns whether there was one.
    pub fn close_sheet(&mut self) -> bool {
        let was = self.own_sheet_open();
        self.profile_sheet = None;
        self.action_target = None;
        self.flow_target = None;
        self.adding = false;
        self.add_catalog = None;
        self.picking_icon = false;
        self.web_icon.close();
        self.flow_new = None;
        self.panel_design = None;
        self.update_offer = None;
        self.update_done = None;
        self.dev_offer = None;
        self.smart_sheet = None;
        was
    }

    /// Lets go of what the Flow tab holds for the flows of the working copy, which is dropped when
    /// Settings closes or the UI sleeps (see `App::release_editor`): the selection, the wire being
    /// dragged, the variable being followed and the search text. Kept on purpose: the marks of the
    /// last runs and `flow_untidy` (both belong to the saved flows, not to this session), and the
    /// kit request, which the person may come back to.
    pub fn release_flow_state(&mut self) {
        self.flow_sel = None;
        self.flow_multi.clear();
        self.flow_marquee = None;
        self.flow_group = false;
        self.flow_wire = None;
        self.flow_focus = None;
        self.flow_var = None;
        self.flow_insp = false;
        self.flow_insp_vars = false;
        self.flow_insp_blocks = false;
        self.flow_var_search = String::new();
        self.flow_block_search = String::new();
        self.flow_insp_seen = None;
        self.flow_mid = false;
        self.flow_search = String::new();
        self.flow_side = false;
        self.flow_side_leave = None;
        self.pie_picked.clear();
        self.pie_marquee = None;
        self.pie_marquee_base.clear();
        self.pie_moving = false;
        self.close_pie_menu();
    }

    /// Every selected block of the canvas: the one, or the group.
    pub fn selected_ids(&self) -> std::collections::BTreeSet<u32> {
        let mut all = self.flow_multi.clone();
        all.extend(self.flow_sel);
        all
    }

    /// Whether block `id` is selected, alone or in the group.
    pub fn is_selected(&self, id: u32) -> bool {
        self.flow_sel == Some(id) || self.flow_multi.contains(&id)
    }

    /// Selects exactly these blocks: none, one (the one the inspector shows) or a group.
    pub fn set_selection(&mut self, ids: std::collections::BTreeSet<u32>) {
        self.flow_multi.clear();
        self.flow_sel = None;
        match ids.len() {
            0 => {}
            1 => self.flow_sel = ids.into_iter().next(),
            _ => self.flow_multi = ids,
        }
    }

    /// Closes the list of blocks of the Flow canvas, or the list of slices of the pie editor.
    /// Returns whether one was open.
    pub fn close_flow_menu(&mut self) -> bool {
        let flow = self.flow_menu.take().is_some();
        let pie = self.close_pie_menu();
        flow || pie
    }

    /// Closes the list of slices of the pie editor. Returns whether it was open.
    pub fn close_pie_menu(&mut self) -> bool {
        let was = self.pie_menu.take().is_some();
        if was {
            // the tiles are built again the next time one is opened (see `AddCatalog`)
            self.add_catalog = None;
        }
        was
    }

    /// Whether the Menu tab is showing something that can fill the whole display: the pie editor
    /// or the import tray (see `App::toggle_flow_full`).
    pub fn pie_canvas_open(&self) -> bool {
        self.tab == Tab::Menu && (self.menu_view == MenuView::Editor || self.pie_import.is_some())
    }

    /// Opens the import tray on a pie file that was read (replacing the one that was open).
    pub fn open_import(
        &mut self,
        bundle: crate::config::share::Bundle,
        label: String,
        runs_commands: bool,
    ) {
        self.pie_import = Some(PieImport::new(bundle, label, runs_commands));
        self.pie_drag = None;
        self.pie_split = IMPORT_SPLIT;
        self.tab = Tab::Menu;
        self.adding = false;
        self.add_catalog = None;
        self.picking_icon = false;
    }

    /// Closes the import tray. Returns whether there was one.
    pub fn close_import(&mut self) -> bool {
        self.pie_drag = None;
        self.pie_import.take().is_some()
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
        if let Some(f) = edit.flows.get(self.flow) {
            self.flow_multi.retain(|id| f.node(*id).is_some());
        } else {
            self.flow_multi.clear();
        }
        self.flow_marquee = None;
        self.flow_group = false;
        self.pie_drag = None;
        self.pie_marquee = None;
        self.pie_moving = false;
        self.close_pie_menu();
        let top = edit.profiles.get(self.profile).map(|p| p.items.as_slice());
        self.pie_picked
            .retain(|path| top.is_some_and(|items| items_at(items, path).is_some()));
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
        // nesting adds two tiles: the Submenu and the Intelligent one (a submenu the program fills)
        assert_eq!(
            tiles(&AddCatalog::new(true, &[])),
            tiles(&AddCatalog::new(false, &[])) + 2
        );
    }

    #[test]
    fn the_add_catalog_offers_every_flow_as_a_slice_that_runs_it() {
        let flows = [Flow::new("f1", "Morning"), Flow::new("f2", "  ")];
        let cat = AddCatalog::new(true, &flows);
        let (_, items) = cat
            .groups
            .iter()
            .find(|(title, _)| *title == "Flows")
            .expect("a Flows group");
        // the blank one to choose a flow for, then one per flow
        assert_eq!(items.len(), 3);
        let ids: Vec<&Action> = items
            .iter()
            .filter_map(|it| match &it.kind {
                crate::config::ItemKind::Run(a) => Some(a),
                _ => None,
            })
            .collect();
        assert_eq!(ids[0], &Action::Flow(String::new()));
        assert_eq!(ids[1], &Action::Flow("f1".into()));
        assert_eq!(items[1].name, "Morning");
        assert_eq!(
            items[2].name, "Flow",
            "a flow without a name still has a label"
        );
        // with no flows the group still holds the blank slice
        let none = AddCatalog::new(true, &[]);
        assert!(
            none.groups
                .iter()
                .any(|(t, l)| *t == "Flows" && l.len() == 1)
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
    fn a_dialog_of_the_app_blocks_what_is_under_it_but_is_not_closed_here() {
        let mut ed = Ed::new();
        ed.dialog_open = true;
        assert!(ed.sheet_open());
        assert!(!ed.close_sheet(), "the app closes its own dialogs");
        ed.dialog_open = false;
        assert!(!ed.sheet_open());
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
        ed.add_catalog = Some(AddCatalog::new(true, &[]));
        ed.reset_nav();
        assert!(!ed.adding && ed.add_catalog.is_none());
    }
}
