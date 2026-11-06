//! Everything that lives on the UI thread.
//!
//! | Module | Contents |
//! |---|---|
//! | [`app`] | the eframe app: state, message handling, window management |
//! | [`pie`] | drawing the pie |
//! | [`settings`] | the Settings window (tabs, editors, icon picker) |
//! | [`tray`] | the menu bar icon and menu |
//! | [`appicon`] | real app icons, reduced to white glyphs |
//! | [`icons`] | the bounded texture cache for icons, shared by the pie and Settings |
//! | [`sleep`] | the idle policy that frees caches and Settings state |
//! | [`draw`] | drawing helpers shared by the pie and Settings: anti-aliased slices, icon loading, easing, the pie palette |
//! | [`fonts`] | Inter for text and Phosphor for icons, compiled into the binary |
//! | [`symbols`] | the names and glyphs of the icon set, and the `ico:` slice icon strings |
//! | [`screen`] | display lookup for window placement |
//! | `vtrackpad` (feature `vtrackpad`) | the virtual trackpad: a developer tool: a floating trackpad whose finger is the mouse and whose pointer is virtual |
//!
//! The UI never runs actions or inspects raw input: the event tap does that and sends
//! [`Msg`](crate::msg::Msg) values here.

pub mod app;
pub mod appicon;
pub mod draw;
pub mod fonts;
pub mod iconfit;
pub mod icons;
pub mod pie;
pub mod screen;
pub mod settings;
pub mod sleep;
pub mod symbols;
pub mod tray;
#[cfg(feature = "vtrackpad")]
pub mod vtrackpad;
