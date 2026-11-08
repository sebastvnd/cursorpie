//! The settings card: a sidebar of sections (Menu, Flow, Behavior, General, About) and the page of the selected one.
//!
//! | Module | Contents |
//! |---|---|
//! | `state` | editor-only state ([`Ed`], [`Tab`], [`Slot`]) |
//! | `window` | the window, animation, top bar, sidebar, body and footer (`App::settings_ui`, `regions`) |
//! | `tabs/*` | one file per section (the [`Tab`] enum) (`menu` also has `profile_toolbar`; `flow` is the node editor) |
//! | `demo` | small looping animations that show what a setting does (Behavior tab) |
//! | `pie_designer` | the interactive pie used to pick and arrange slices |
//! | `inspector` | editor for one slice, `add_panel` for the "Add a slice" panel |
//! | `action_editor` | editor for one [`Action`](crate::config::Action) |
//! | `icon_picker`, `icon_catalog` | icon chooser and its catalog |
//! | `picker` | native file choosers run off the UI thread |
//! | `welcome` | the first-launch welcome window (`App::welcome_ui`), drawn in the same window and style |
//! | `quit_dialog` | the "unsaved changes" sheet shown when you quit with edits that were not saved (`App::request_quit`) |
//! | `update_dialog` | the "update available" sheet, offered once a day when Settings opens (see [`update`](crate::update)) |
//! | `wayland_keys` | recording a shortcut from the window's own key events in a Wayland session |
//! | `undo` | undo / redo shortcuts and backup restore, on top of [`History`](crate::history::History) |
//! | `theme`, `widgets` | palette, geometry, egui style and small reusable widgets |
//!
//! Settings edits a *copy* of the config (`App::edit`); pressing Save sanitizes and applies it.
//! Every change to that copy can be undone and redone (`undo`), and the copy is dropped again when
//! Settings closes.
//!
//! The look is modern flat: a light, roomy window with large rounded cards, pill-shaped chips, one
//! icon set, the Inter font and one accent color. Anything that opens on top of a page is a sheet or a
//! popover (see `widgets`). See the `theme` module for the rules and `docs/UI_DESIGN.md` for the
//! reasoning.

mod action_editor;
mod add_panel;
mod brand;
mod demo;
mod icon_catalog;
mod icon_picker;
mod inspector;
mod picker;
mod pie_designer;
mod quit_dialog;
mod state;
mod tabs;
pub mod theme;
mod undo;
mod update_dialog;
mod wayland_keys;
mod welcome;
mod widgets;
mod window;

pub use picker::choosing;
pub use state::{AppScan, Ed, Slot, Tab};
pub use welcome::Welcome;
