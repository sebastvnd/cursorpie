//! macOS backend: Core Graphics, AppKit, TIS, LaunchAgent and the tools that ship with macOS.
//!
//! The submodules only group the functions; everything is re-exported by [`platform`](crate::platform).
//!
//! | Submodule | Contents |
//! |---|---|
//! | `permissions` | Accessibility check and links to the System Settings panes |
//! | `apps` | frontmost app, app under the pointer, app icons, activation policy, cursor position, displays |
//! | `input` | posting keys and mouse buttons (`CGEvent`) |
//! | `memory` | returning freed memory to macOS after the UI goes to sleep |
//! | `activity` | opting out of App Nap, so the first press after a long idle time is not slow |
//! | `login_item` | "Launch at login" LaunchAgent |
//! | `spaces` | "Open app" that follows the app's window to its Space, or fetches the window |
//! | `shell` | helpers that run macOS tools: `open`, `osascript`, `mdls`, `afplay` |
//! | `haptics` | one haptic tick on a trackpad (`NSHapticFeedbackManager`) |
//! | [`layout`] | keyboard-layout snapshot (TIS + `UCKeyTranslate`) |

pub mod layout;

mod activity;
mod apps;
mod haptics;
mod input;
mod login_item;
mod memory;
mod permissions;
mod shell;
mod spaces;

pub use activity::*;
pub use apps::*;
pub use haptics::*;
pub use input::*;
pub use login_item::*;
pub use memory::*;
pub use permissions::*;
pub use shell::*;
pub use spaces::*;
