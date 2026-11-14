//! Messages sent *to* the UI thread, from the event tap, the menu bar and short-lived worker threads.
//!
//! Senders use a [`Notifier`], which also wakes the UI so it handles the message even while its window
//! is hidden. The UI drains them in `App::logic` and applies each one in `ui::app::messages`.

use crate::keys::Shortcut;
use crate::ui::settings::Slot;
use eframe::egui;
use std::sync::mpsc;

/// Something the UI thread has to react to.
pub enum Msg {
    /// Open (or re-center, for submenus) the pie for `profile` at menu level `path`.
    ShowPie {
        x: f64,
        y: f64,
        profile: usize,
        path: Vec<usize>,
    },
    /// The trigger was pressed at (x, y) while Settings is open, so the pie stays off. The UI says
    /// so and brings Settings forward (a press that seems to do nothing is confusing). `touch` is
    /// true for a trackpad hold: the UI then stays quiet while Settings already has the focus,
    /// because a finger that pauses on the trackpad to aim at a button is not a call for the pie.
    PieBlocked { x: f64, y: f64, touch: bool },
    /// Open the Settings window centered on the screen containing (x, y); `about` selects the About tab.
    OpenSettings { x: f64, y: f64, about: bool },
    /// Open the welcome window centered on the screen containing (x, y) (first launch, or the
    /// tray item "Welcome guide...").
    OpenWelcome { x: f64, y: f64 },
    /// The hovered slice of the open pie changed.
    Hover(Option<usize>),
    /// Close the pie (also used to reset the UI after the tap was disabled or paused).
    HidePie,
    /// A shortcut was recorded while Settings was in recording mode.
    KeyCaptured(Shortcut),
    /// Recording was cancelled with a plain Esc.
    RecordCancelled,
    /// An image was chosen as the icon of slice `path` in `profile`.
    IconPicked {
        profile: usize,
        path: Vec<usize>,
        file: String,
        /// What was done to the image (shrunk, ...), for the status bar
        note: Option<String>,
    },
    /// An image of the icons folder ("Mine") was deleted from the icon picker (`error` says why not
    /// when it could not be). The UI tells which slices were still using it.
    IconDeleted { file: String, error: Option<String> },
    /// One or more images could not be used as an icon (chosen with the chooser or dropped on the
    /// window). `file` is a path or a short label, `lines` say why, one sentence per file; the UI
    /// shows them in a pop-up sheet, not only in the status bar.
    IconRefused { file: String, lines: Vec<String> },
    /// An app was chosen for the "apps" list of `profile`; `bundle` is its bundle identifier.
    AppPicked { profile: usize, bundle: String },
    /// A config file to import was chosen (its path).
    ImportPicked(String),
    /// Menu bar item clicked (the menu item id).
    Tray(String),
    /// Take keyboard focus back after a native file chooser closed.
    Refocus,
    /// The trigger was pressed: the pie of `general` (and the app's own `app` profile) may open
    /// soon. The UI loads their icons now, while the hold timer runs, instead of on the frame that
    /// opens the pie. Also wakes the UI from sleep (see [`ui::sleep`](crate::ui::sleep)).
    Warm { general: usize, app: Option<usize> },
    /// An app was chosen in the "New profile" form; `0` is its bundle identifier.
    NewProfileAppPicked(String),
    /// The installed apps were read (see `platform::installed`), for the app list of the profile
    /// dialogs.
    InstalledApps(Vec<crate::platform::installed::InstalledApp>),
    /// A profile file to import was chosen (its path).
    ProfileImportPicked(String),
    /// A flow file to import was chosen (its path).
    FlowImportPicked(String),
    /// Show a short message in the Settings status bar (sent by helpers that cannot reach `App`).
    Notice(String),
    /// A flow was run (by the actions worker). `report` is `Some` when blocks of it could not do
    /// their work and the flow did not deal with that: the Settings window shows the details, and
    /// the Flow tab marks those blocks. `None` clears the marks of an earlier run of the flow.
    FlowRan {
        id: String,
        report: Option<crate::flowrun::Report>,
    },
    /// A mouse was connected that is not the one that was in use: `new` if CursorPie had never met
    /// it, otherwise one it knows. `extra` is the number of extra buttons it has.
    MouseChanged {
        new: bool,
        name: String,
        extra: usize,
    },
    /// An extra button (its button number) was pressed while Settings scans the mouse.
    ButtonScanned(i64),
    /// Play one haptic tick on the trackpad (a trackpad hold only; see
    /// [`tap::touch::haptic`](crate::tap)). Handled on the UI thread: AppKit is not called from the tap.
    Haptic(crate::platform::HapticKind),
    /// An app was chosen for an "Open app" action of the slice at `path` (`slot` = which action).
    ActionAppPicked {
        profile: usize,
        path: Vec<usize>,
        slot: Slot,
        app: String,
    },
    /// An app was chosen for the "App" block `node` of the flow at index `flow`.
    FlowAppPicked { flow: usize, node: u32, app: String },
    /// The look for a newer release ended (see [`update`](crate::update)). `manual` is true when the
    /// person pressed *Check now* in the About tab: the answer is shown there and nothing is offered.
    UpdateChecked {
        manual: bool,
        result: Result<crate::update::Version, crate::update::Failure>,
    },
    /// An action is about to run (sent by the actions worker): the UI shows what it is in a small
    /// notice on the screen (see [`ui::hud`](crate::ui::hud)). Ignored while Settings is open or
    /// when the notice is switched off.
    Hud(crate::ui::hud::HudNote),
    /// The run that the notices were announcing is over (sent by the actions worker, see
    /// [`HudNote::live`](crate::ui::hud::HudNote::live)): the pill fills its bar and goes away
    /// after its usual time. Ignored when no such pill is on the screen.
    HudEnd,
    /// The kit for an AI agent was made, and put on the clipboard if a program could do that (see
    /// [`config::flow_kit`](crate::config::flow_kit)). The "New flow" sheet shows the outcome.
    FlowKit(crate::ui::settings::KitStatus),
    /// A panel (a small window with fields and buttons, see [`panel`](crate::panel)) is asked for by
    /// a flow or the browser extension. The UI shows it when the window is free and sends the
    /// answer down `reply`.
    Panel(crate::panel::Request),
    /// Changes the open panel named `panel` (progress, new options...), if `source` asked for it.
    PanelPatch {
        panel: String,
        patch: crate::panel::Patch,
        source: crate::panel::Source,
    },
    /// Closes the open or waiting panel named `panel` as cancelled, if `source` asked for it.
    PanelClose {
        panel: String,
        source: crate::panel::Source,
    },
    /// The package of a newer release was downloaded, checked and made ready (or could not be): see
    /// [`update::install`](crate::update::install).
    UpdateStaged(Result<crate::update::install::Staged, crate::update::install::InstallError>),
}

/// A channel sender plus the egui context needed to wake the UI. Cheap to clone.
#[derive(Clone)]
pub struct Notifier {
    pub tx: mpsc::Sender<Msg>,
    pub ctx: egui::Context,
}

impl Notifier {
    /// Queues `m` and requests a repaint so the UI picks it up promptly. Never blocks.
    ///
    /// The repaint is asked of the *root* window, which is where `App::logic` reads the messages.
    /// A plain `request_repaint` means "the window that was drawn last", which is not necessarily
    /// the root window.
    pub fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        self.wake();
    }

    /// Asks the root window for a frame (see [`send`](Self::send)): the tap does this to make the
    /// UI notice a change of state that has no message.
    pub fn wake(&self) {
        self.ctx.request_repaint_of(egui::ViewportId::ROOT);
    }
}
