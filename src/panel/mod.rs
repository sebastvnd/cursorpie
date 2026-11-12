//! **Panels**: small interactive windows that CursorPie shows on request, and the one way a flow, the
//! browser extension or any future extension asks the person something with more than a yes or a no.
//!
//! The notice of what CursorPie is doing ([`ui::hud`](crate::ui::hud)) is a pill that only
//! announces. A panel is its interactive sibling: a title, widgets (text, boxes, switches, sliders,
//! lists, a progress bar) and buttons, drawn in the look of Settings, answered with the values the
//! person entered. It is built from three layers, so each can be used (and replaced) on its own:
//!
//! | Layer | Module | What it is |
//! |---|---|---|
//! | description | [`spec`] | [`PanelSpec`]: plain data (JSON) with limits, safe to take from anywhere |
//! | state | [`form`] | [`Form`]: what is typed and picked, how it becomes a [`PanelResult`], [`Patch`]es |
//! | window | `ui::settings::panel_form`, `ui::app::panel` | draws a form in the one window; no logic of its own |
//! | designer | [`design`], `ui::settings::panel_designer` | builds a [`PanelSpec`] with the mouse instead of by typing JSON |
//! | transport | [`broker`] | [`ask`]: any thread asks, the UI answers; who may ask |
//!
//! Callers today: the *Ask* block of a flow in *Panel* mode ([`flowrun`](crate::flowrun)) and the
//! browser extension (`panel` messages of the [`bridge`](crate::bridge)). To add another, build a
//! [`PanelSpec`] and call [`ask`] from a thread that may wait. `docs/PANELS.md` has the format, the
//! examples and the rules.

pub mod broker;
pub mod design;
pub mod form;
pub mod layout;
pub mod spec;

pub use broker::{Request, Source, ask, ask_in_background, close, patch, release, set_notifier};
pub use form::{Field, Form, Outcome, PanelResult, Patch, Value};
pub use spec::{
    Button, ButtonAlign, Icon, Layout, Opt, PanelSpec, Place, ShowIf, SpecError, TextStyle, Tone,
    Widget, declared_vars,
};
