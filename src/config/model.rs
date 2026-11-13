//! The config data model: everything that is serialized to `config.json`.
//!
//! Enum variants are serialized by name (serde's default external tagging), so renaming one is a
//! breaking change of the file format.

use super::item_at;
use crate::keys::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Something CursorPie can do.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub enum Action {
    /// Do nothing (for side buttons: let the click pass through to the app).
    #[default]
    Nothing,
    Key(Shortcut),
    /// App name ("Safari") or path ("/Applications/Safari.app"), passed to `open -a` on macOS. On
    /// Windows a program name ("chrome") or the path of an `.exe`.
    OpenApp(String),
    /// URL or file path, passed to `open` (macOS) or opened with its default handler (Windows).
    OpenUrl(String),
    /// Shell command, run with `/bin/sh -c` (macOS) or `cmd /C` (Windows).
    Command(String),
    /// Do something in the browser, through the browser extension: a line such as `media.next`,
    /// `tab.close` or `chat.send_to Anna :: on my way` (the command, then its text). See
    /// [`bridge::commands`](crate::bridge::commands) for the commands and
    /// `docs/BROWSER_EXTENSION.md`. Needs [`Config::browser_bridge`](super::Config::browser_bridge)
    /// and the extension; without them it tells the person so and does nothing.
    Browser(String),
    Sequence(Vec<Action>),
    /// Pause this many milliseconds (at most [`MAX_WAIT_MS`](super::MAX_WAIT_MS)). Made by the
    /// *Wait* block of a flow; useful inside a `Sequence` too.
    Wait(u32),
    /// Run the flow with this id (see [`Config::flows`] and the [`flow`](super::flow) module).
    /// Nothing happens when no flow has that id.
    Flow(String),
}

impl Action {
    /// The app this action opens, if it opens one: the text of an [`Action::OpenApp`], or of the
    /// first one of a [`Action::Sequence`]. `None` for everything else and for a blank app name.
    ///
    /// This is the app an "Open app" slice offers the choice "new window or the one that is open"
    /// for (see [`running`](crate::running)).
    pub fn open_target(&self) -> Option<&str> {
        match self {
            Action::OpenApp(app) => Some(app.trim()).filter(|a| !a.is_empty()),
            Action::Sequence(steps) => steps.iter().find_map(Action::open_target),
            _ => None,
        }
    }

    /// What this action opens that a slice can offer the choice for: the first "Open app" step
    /// with a name, or the first "Open URL" step with a web link, in the order they run (a
    /// [`Action::Sequence`] is looked through). `None` for everything else, a blank app name, and a
    /// link that is not a web link (a file, `mailto:`...).
    ///
    /// [`Action::open_target`] is the same for apps only.
    ///
    /// Use [`Action::open_choice_key`] to know what the pie asks about: a web link only counts
    /// when the browser extension is on.
    pub fn open_choice_target(&self) -> Option<OpenTarget<'_>> {
        match self {
            Action::OpenApp(app) => Some(app.trim())
                .filter(|a| !a.is_empty())
                .map(OpenTarget::App),
            Action::OpenUrl(url) => Some(url.trim())
                .filter(|u| is_web_url(u))
                .map(OpenTarget::Url),
            Action::Sequence(steps) => steps.iter().find_map(Action::open_choice_target),
            _ => None,
        }
    }

    /// The key of what a press asks about for this action (see [`OpenTarget::key`]): an app, or a
    /// web link when `bridge` is true. A web link needs the browser extension (the setting
    /// [`Config::browser_bridge`]): without it CursorPie cannot know which pages are open, so the
    /// slice just opens the link, with no choice.
    pub fn open_choice_key(&self, bridge: bool) -> Option<String> {
        match self.open_choice_target()? {
            OpenTarget::Url(_) if !bridge => None,
            target => Some(target.key()),
        }
    }
}

/// What an "Open" slice can offer the choice "new window or the one that is open" for: an app, or a
/// web page (an `http://` or `https://` link).
///
/// The choice is made by [`Action::open_choice_target`]; [`OpenTarget::key`] is the one string the
/// cache of [`running`](crate::running) knows it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenTarget<'a> {
    /// The text of an [`Action::OpenApp`], trimmed and not blank.
    App(&'a str),
    /// The text of an [`Action::OpenUrl`], trimmed, when it is a web link (see [`is_web_url`]).
    Url(&'a str),
}

/// What a web link's key starts with, so it can never be mistaken for an app (see
/// [`OpenTarget::key`]).
pub const URL_KEY_PREFIX: &str = "url:";

impl OpenTarget<'_> {
    /// The key the running-cache knows this target by: the app as typed, or [`URL_KEY_PREFIX`] and
    /// the link.
    pub fn key(&self) -> String {
        match self {
            OpenTarget::App(app) => (*app).to_string(),
            OpenTarget::Url(url) => format!("{URL_KEY_PREFIX}{url}"),
        }
    }
}

/// Whether `text` is a web link: `http://` or `https://` (any case). Only those get the choice
/// between a new window and the page that is open. A file path, a `mailto:` link or any other scheme
/// is opened as it always was.
pub fn is_web_url(text: &str) -> bool {
    let t = text.trim();
    ["http://", "https://"]
        .iter()
        .any(|p| t.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p)))
}

/// What a slice does.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum ItemKind {
    /// Selecting it makes the side buttons (back/forward) send these actions, and every other extra
    /// button of the mouse (button number 5 and up) the action listed for it in `extra`. A button
    /// that is not listed passes through to the app. Files from before extra buttons existed have
    /// no `extra` and load with none.
    Mode {
        back: Action,
        forward: Action,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        extra: BTreeMap<i64, Action>,
    },
    /// Runs the action when you release the button over the slice.
    Run(Action),
    /// Drag outward across the rim to open it.
    Submenu(Vec<Item>),
}

/// One slice of a ring.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Item {
    pub name: String,
    /// Text / emoji, or an image path (.png .jpg .jpeg .webp), e.g. "~/Pictures/undo.png"
    pub icon: String,
    pub kind: ItemKind,
}

/// How the pie of an app that has its own profile is opened.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum AppGesture {
    /// Classic: hold opens the pie of the app in front (if it has one).
    Merged,
    /// New: hold opens the general pie, double-click opens the app's own pie.
    #[default]
    DoubleClick,
}

/// What the extra click does while a pie opened by a trackpad hold is open (macOS trackpad, see
/// `docs/developer/TRACKPAD_HAPTICS_PLAN.md`).
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum TouchClickAction {
    /// The same as the right-click: the side buttons now follow the other pie; the pie stays open.
    #[default]
    SwitchPie,
    /// The hovered *Mode* slice becomes the active mode of its profile, without closing the pie.
    /// Over any other slice the click does nothing.
    PickHovered,
    /// The click is swallowed and does nothing.
    Nothing,
}

/// How the app's icon looks in the center of an app's own pie.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum AppIconStyle {
    /// One color: a silhouette of the icon in the ink of the pie, like the other icons of the pie.
    #[default]
    Glyph,
    /// The app's own icon, in its own colors.
    Color,
}

/// A soft shadow under the app's icon in the center of the pie, so the icon stands out from a plain
/// white disc.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum IconShadow {
    /// A soft shadow while the center is a light, solid disc (the light appearance without glass),
    /// none where the icon already stands out.
    #[default]
    Auto,
    /// Always a soft shadow.
    Soft,
    /// Always a stronger shadow.
    Strong,
    /// Never.
    Off,
}

/// Which pie the side buttons follow in an app that has a pie of its own, until you choose another.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum SideTarget {
    /// The app's own pie.
    #[default]
    App,
    /// The general pie, even inside an app that has its own.
    General,
}

/// How CursorPie decides which app a gesture is about.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum TargetApp {
    /// The app in front (the one with keyboard focus).
    Focused,
    /// The app whose window is under the pointer, even if it is not in front.
    #[default]
    UnderCursor,
}

/// What an "Open app" slice does when the app's window is on another virtual desktop (Windows) or
/// Space (macOS).
///
/// The OSes offer different choices, see [`Os::desktop_modes`](crate::platform::Os::desktop_modes).
/// Every value is kept on every OS so a `config.json` copied between them stays whole;
/// [`DesktopMode::effective`] is what an OS makes of a value it does not offer.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum DesktopMode {
    /// Go to the desktop the app's window is on, like clicking its Dock icon or taskbar button.
    #[default]
    SwitchToApp,
    /// Pull the app's window onto the current desktop and show it here. macOS only, and
    /// experimental: it needs private system calls, which fail silently on some macOS versions.
    /// When it fails CursorPie switches to the app's desktop instead.
    BringToCurrent,
    /// Never change desktops: open the app the way it was opened before this setting existed.
    DoNothing,
}

impl DesktopMode {
    /// Whether "Open app" should look for the app's window on the other desktops.
    pub fn follows_app(self) -> bool {
        self != DesktopMode::DoNothing
    }

    /// The mode that is carried out on `os`: the mode itself when `os` offers it, otherwise
    /// [`SwitchToApp`](DesktopMode::SwitchToApp), the closest thing it can do.
    pub fn effective(self, os: crate::platform::Os) -> DesktopMode {
        if os.desktop_modes().contains(&self) {
            self
        } else {
            DesktopMode::SwitchToApp
        }
    }
}

impl ItemKind {
    /// What this slice does for the side or extra button `button` (mouse button numbers: 3 back, 4
    /// forward, 5 and up extra). `None` unless it is a mode; `Nothing` for a button it does not
    /// handle.
    pub fn button_action(&self, button: i64) -> Option<&Action> {
        static NOTHING: Action = Action::Nothing;
        match self {
            ItemKind::Mode {
                back,
                forward,
                extra,
            } => Some(match button {
                crate::mouse::BACK => back,
                crate::mouse::FORWARD => forward,
                b => extra.get(&b).unwrap_or(&NOTHING),
            }),
            _ => None,
        }
    }
}

impl Item {
    /// Creates a side-button mode slice that sends the `back` / `forward` shortcuts.
    pub fn mode(name: &str, icon: &str, back: Shortcut, forward: Shortcut) -> Self {
        Self {
            name: name.into(),
            icon: icon.into(),
            kind: ItemKind::Mode {
                back: Action::Key(back),
                forward: Action::Key(forward),
                extra: BTreeMap::new(),
            },
        }
    }
    /// Creates a slice that runs `action` when the button is released over it.
    pub fn run(name: &str, icon: &str, action: Action) -> Self {
        Self {
            name: name.into(),
            icon: icon.into(),
            kind: ItemKind::Run(action),
        }
    }
    /// Creates a slice that opens a nested ring.
    pub fn submenu(name: &str, icon: &str, items: Vec<Item>) -> Self {
        Self {
            name: name.into(),
            icon: icon.into(),
            kind: ItemKind::Submenu(items),
        }
    }
}

/// Whether Settings and the pie are drawn light or dark.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum Appearance {
    /// Follow the system (light when the system does not say).
    #[default]
    System,
    Light,
    Dark,
}

/// Shortest and longest time the notice of an action stays on screen, in milliseconds.
pub const HUD_MS_MIN: u64 = 500;
pub const HUD_MS_MAX: u64 = 6000;
/// Smallest and largest size of the notice, in percent of the normal size.
pub const HUD_SIZE_MIN: u32 = 70;
pub const HUD_SIZE_MAX: u32 = 160;

/// Where the notice of the actions CursorPie carries out (see [`crate::ui::hud`]) appears.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum HudPosition {
    /// Bottom center of the display the pointer is on, above the dock or taskbar.
    #[default]
    Bottom,
    /// Top center of the display the pointer is on.
    Top,
    /// Just below the pointer.
    Cursor,
}

/// How the slices of the pie are drawn.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum PieStyle {
    /// Opaque slices.
    #[default]
    Solid,
    /// Translucent, frosted slices with a bright rim, like frosted glass.
    Glass,
}

/// A pie: its slices, and the apps it applies to.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    /// App ids: bundle identifiers on macOS ("com.apple.Safari"), lower-case executable names on
    /// Windows ("chrome.exe"). Empty = fallback profile.
    pub apps: Vec<String>,
    pub items: Vec<Item>,
    /// Path (indices) of the item whose side-button actions are currently active.
    pub active: Vec<usize>,
}

impl Profile {
    /// The general (fallback) profile: the only one that lists no apps. There is exactly one;
    /// every other profile lists at least one app (see [`Config::sanitize`]).
    pub fn is_general(&self) -> bool {
        self.apps.is_empty()
    }

    /// A new profile for the app `bundle` with one starter slice. `None` if the name or the app
    /// is blank, because a profile that is not the general one must belong to an app.
    pub fn for_app(name: &str, bundle: &str) -> Option<Self> {
        let (name, bundle) = (name.trim(), bundle.trim());
        if name.is_empty() || bundle.is_empty() {
            return None;
        }
        Some(Self {
            name: name.into(),
            apps: vec![bundle.into()],
            items: vec![super::presets().remove(0)],
            active: vec![0],
        })
    }

    /// The slice whose side-button actions are currently active, if `active` still points at one.
    pub fn active_item(&self) -> Option<&Item> {
        item_at(&self.items, &self.active)
    }
}

/// The value of `show_labels` for a file that does not have the field: before the setting existed
/// the pie always showed the names.
fn names_on() -> bool {
    true
}

/// The whole configuration. `#[serde(default)]` keeps older files loadable: every field
/// missing from a file takes its value from [`Config::default`].
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub profiles: Vec<Profile>,
    pub pie_hold_ms: u64,
    pub settings_hold_ms: u64,
    pub pie_size: f32,
    pub accent: [u8; 3],
    /// Mouse button that opens the pie: 2 = middle, 5/6/7 = extra buttons.
    pub trigger_button: i64,
    /// Dragging right after pressing opens the pie instantly (disables middle-drag pass-through).
    pub flick: bool,
    pub sound: bool,
    /// Show a small notice on screen (a pill with an icon and a line of text) for each action that
    /// runs: an app that opens, a link, a shortcut, a flow. See [`crate::ui::hud`].
    pub hud: bool,
    /// Where that notice appears.
    pub hud_position: HudPosition,
    /// How long the notice stays, in milliseconds.
    pub hud_ms: u64,
    /// How big that notice is, in percent of the normal size (see `HUD_SIZE_MIN` and `HUD_SIZE_MAX`).
    pub hud_size: u32,
    pub launch_at_login: bool,
    pub app_gesture: AppGesture,
    pub double_click_ms: u64,
    /// Which app picks the pie (and gets the shortcuts): the one in front or the one under the pointer.
    pub target_app: TargetApp,
    /// With `UnderCursor`: bring that app to the front before sending it a shortcut.
    pub focus_target: bool,
    /// What an "Open app" slice does when the app's window is on another virtual desktop or Space.
    pub desktop_mode: DesktopMode,
    /// An "Open app" slice of an app that already runs offers two choices while it is hovered: a
    /// green + (a new window) and a yellow square (the app that is open). Off: the slice always
    /// does what it did before this setting existed (see [`running`](crate::running)).
    pub open_app_choice: bool,
    /// Use the CursorPie browser extension (it ships inside the program) to know which web pages
    /// are open, go to one, and open a link in a new window, for the choice of an "Open URL" slice
    /// (see [`bridge`](crate::bridge)). Off by default: the person installs the extension and
    /// switches this on.
    pub browser_bridge: bool,
    /// A flow may show a *panel*: a small window with fields, lists and buttons (an *Ask* block in
    /// Panel mode, see [`panel`](crate::panel)). On by default: the person wrote the flow.
    pub panels: bool,
    /// A run that takes a while (a flow, a pause, a sequence with a pause in it) opens a small
    /// progress panel by itself, with a bar, the step that is running and a Stop button (see
    /// [`runpanel`](crate::runpanel)). On by default; needs [`Config::panels`].
    pub progress_panel: bool,
    /// The browser extension may show panels too (a tab switcher, a question). Off by default,
    /// because it puts content from outside CursorPie on the screen; needs
    /// [`Config::browser_bridge`].
    pub extension_panels: bool,
    /// Look for a newer release once a day, when Settings opens, and offer it (see
    /// [`update`](crate::update)). It is the only network connection CursorPie makes by itself; the package is only downloaded when the person presses *Install update*. On by default,
    /// also for a file saved before this setting existed; *Check now* in the About tab works either way.
    pub check_updates: bool,
    /// Which pie the side buttons follow in an app that has its own pie, until you choose another.
    pub side_default: SideTarget,
    /// Choosing a side-button mode in a pie makes the side buttons follow that pie.
    pub side_adopt: bool,
    /// Forget that choice when another app becomes the target.
    pub side_forget: bool,
    /// Right-click while the pie is open to switch which pie the side buttons follow.
    pub right_click_switch: bool,
    /// Ring color tells which pie the side buttons follow: accent = this one, `inactive_color` = the other.
    pub side_indicator: bool,
    pub inactive_color: [u8; 3],
    /// How long a pie preview stays after switching from the menu bar (0 = no preview).
    pub preview_ms: u64,
    /// The center of an app's own pie shows that app's icon (as a white silhouette) instead of the
    /// icon of the active side-button mode.
    pub center_app_icon: bool,
    /// With `center_app_icon`: the app's icon as a one-color silhouette or in its own colors.
    pub app_icon_style: AppIconStyle,
    /// With `center_app_icon`: when the icon gets a shadow (see [`IconShadow`]).
    pub app_icon_shadow: IconShadow,
    /// Each slice shows its name under its icon. Off (the default): the pie shows icons only. A
    /// file saved before this setting existed keeps the names.
    #[serde(default = "names_on")]
    pub show_labels: bool,
    /// Seconds without a pie or Settings on screen before the UI frees its caches and Settings
    /// state (0 = never). See [`ui::sleep`](crate::ui::sleep).
    pub sleep_after_s: u64,
    /// Light, dark or follow the system.
    pub appearance: Appearance,
    /// Whether the slices are opaque or frosted glass.
    pub pie_style: PieStyle,
    /// Glass only: how see-through the slices are, in percent (100 = clear, 0 = opaque).
    pub glass_clear: u8,
    /// Glass only: how frosted the glass is, in percent: the milky veil, the soft inner glow and
    /// the sheen. The window cannot blur what is behind it, so this is drawn.
    pub glass_frost: u8,
    /// Glass only: how bright the rim and the sheen are, in percent.
    pub glass_shine: u8,
    /// Trackpad (macOS) or precision touchpad (Windows): touching and holding opens the pie. Off:
    /// the touch source never starts (on macOS the private touch framework is never loaded, on
    /// Windows nothing is registered with Raw Input).
    pub touch_trigger: bool,
    /// Trackpad: how little the holding finger may move, in millimeters on the surface, before the
    /// hold stops counting.
    pub touch_still_mm: f32,
    /// Trackpad: what the extra click does while the pie is open.
    pub touch_click_action: TouchClickAction,
    /// Trackpad: no hold is armed this soon after a key press, in ms (0 = off).
    pub touch_typing_guard_ms: u64,
    /// Haptic ticks on a trackpad (macOS), when the pie opens, the slice changes and the mode switches.
    pub haptics: bool,
    /// With `haptics`: also a tick when the hovered slice changes.
    pub haptic_hover: bool,
    /// The flows the *Flow* tab edits: chains of steps that a slice runs with [`Action::Flow`].
    /// Files from before flows existed have none.
    pub flows: Vec<super::flow::Flow>,
}

impl Config {
    /// Index of the general (fallback) profile: the first one with no apps, or 0.
    pub fn global_profile(&self) -> usize {
        self.profiles
            .iter()
            .position(|p| p.apps.is_empty())
            .unwrap_or(0)
    }

    /// Index of the profile that lists `bundle`, if any.
    pub fn app_profile_for(&self, bundle: Option<&str>) -> Option<usize> {
        let b = bundle?;
        self.profiles.iter().position(|p| {
            p.apps
                .iter()
                .any(|a| crate::platform::OS.app_ids_equal(a, b))
        })
    }

    /// Which profile applies for the given frontmost app.
    pub fn profile_for(&self, bundle: Option<&str>) -> usize {
        self.app_profile_for(bundle)
            .unwrap_or_else(|| self.global_profile())
    }
}
