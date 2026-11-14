//! Everything that lives on the UI thread.
//!
//! | Module | Contents |
//! |---|---|
//! | [`app`] | the eframe app: state, message handling, window management |
//! | [`pie`] | drawing the pie |
//! | [`settings`] | the Settings window (tabs, editors, icon picker) |
//! | [`hud`] | the notice of what CursorPie is doing: a pill at the edge of the screen |
//! | [`tray`] | the menu bar icon and menu |
//! | [`appicon`] | real app icons, reduced to white glyphs |
//! | [`icons`] | the bounded texture cache for icons, shared by the pie and Settings |
//! | [`sleep`] | the idle policy that frees caches and Settings state |
//! | [`memlog`] | memory marks for the diagnostic log (category `mem`): free while the log is off |
//! | [`trim`] | gives freed memory back to the OS soon after the pie was used, from a thread of its own |
//! | [`draw`] | drawing helpers shared by the pie and Settings: anti-aliased slices, icon loading, easing, the pie palette |
//! | [`fonts`] | Inter for text and Phosphor for icons, compiled into the binary |
//! | [`symbols`] | the names and glyphs of the icon set, and the `ico:` slice icon strings |
//! | [`screen`] | display lookup for window placement |
//!
//! The UI never runs actions or inspects raw input: the event tap does that and sends
//! [`Msg`](crate::msg::Msg) values here.

pub mod app;
pub mod appicon;
pub mod draw;
pub mod fonts;
pub mod hud;
pub mod iconfit;
pub mod icons;
pub mod memlog;
pub mod pie;
pub mod screen;
pub mod settings;
pub mod sleep;
pub mod symbols;
pub mod tray;
pub mod trim;
