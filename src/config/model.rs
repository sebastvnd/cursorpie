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
    Sequence(Vec<Action>),
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
/// `docs/TRACKPAD_HAPTICS_PLAN.md`).
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
    pub launch_at_login: bool,
    pub app_gesture: AppGesture,
    pub double_click_ms: u64,
    /// Which app picks the pie (and gets the shortcuts): the one in front or the one under the pointer.
    pub target_app: TargetApp,
    /// With `UnderCursor`: bring that app to the front before sending it a shortcut.
    pub focus_target: bool,
    /// What an "Open app" slice does when the app's window is on another virtual desktop or Space.
    pub desktop_mode: DesktopMode,
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
