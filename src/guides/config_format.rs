//! # Config file format
//!
//! Settings are stored as pretty-printed JSON in
//! `~/Library/Application Support/CursorPie/config.json`
//! ([`config::config_path`](crate::config::config_path)). The Rust types in
//! [`config`](crate::config) *are* the schema: they derive `serde::{Serialize, Deserialize}`.
//! This guide describes what is not obvious from the types.
//!
//! ## Shape
//!
//! ```text
//! Config
//! ├── profiles[]            (1 to MAX_PROFILES = 16)
//! │   ├── name, apps[] (bundle ids; empty = the general/fallback profile)
//! │   ├── active            path of the active side-button mode, e.g. [3, 1]
//! │   └── items[]           (at most MAX_ITEMS = 8 per ring)
//! │       ├── name, icon    icon = text/emoji, or an image path ("~/" allowed)
//! │       └── kind          Mode { back, forward, extra? } | Run(Action) | Submenu(items[])
//! └── scalar settings       timings, size, colors, gesture and side-button options
//! ```
//!
//! `Mode.extra` maps the number of an extra mouse button (5 to 15; 3 and 4 are `back` and `forward`) to an
//! `Action`; it is left out of the file when empty, and buttons without an entry pass through to the app.
//!
//! `Action` is `Nothing | Key(Shortcut) | OpenApp | OpenUrl | Command | Sequence(Vec<Action>) | Wait(u32) | Flow(String)`.
//! `Flow` names a flow of the top-level `flows` list by its `id` (see [`config::flow`](crate::config::flow)).
//! `Shortcut` is `{ key, mods }`, where `key` is a macOS *virtual key code* (a physical key
//! position) and `mods` a bitmask: Shift = 1, Ctrl = 2, Opt = 4 (Alt), Cmd = 8 (the Windows key
//! on Windows) (see [`keys`](crate::keys)). The codes are the same on every OS: Windows translates
//! them to the key in the same physical position, so a profile shared from a Mac imports on Windows.
//! `Profile::apps` holds bundle ids on macOS and lower-case executable names on Windows. Enums use serde's default external tagging, e.g.
//! `{"Run": {"OpenApp": "Notes"}}`.
//!
//! A step-by-step reference for writing a flow as JSON (written for AI agents, with a JSON Schema and tested
//! examples) is `docs/FLOWS_FOR_AI.md`.
//!
//! The full annotated example and the table of fields are in the repository's `docs/CONFIGURATION.md`.
//!
//! ## Versioning and compatibility
//!
//! * [`CONFIG_VERSION`](crate::config::CONFIG_VERSION) is written to every file. A file **without**
//!   a `version` key (the old v1 format) or with invalid JSON is rejected by
//!   [`parse_config`](crate::config::parse_config).
//! * `Config` is `#[serde(default)]`: a field missing from an older file takes its default. **New
//!   settings must therefore always have a sensible `Default`**, and adding one does *not* require
//!   a version bump. Bump the version only for changes an old reader would misinterpret
//!   (renamed or re-typed fields), and write a migration in `parse_config`.
//! * Unknown fields are ignored.
//!
//! ## Validation
//!
//! Every config that enters the program (loaded, imported, or edited in Settings) goes through
//! [`Config::sanitize`](crate::config::Config::sanitize), so the rest of the code can rely on:
//!
//! | Rule | Value |
//! |---|---|
//! | profiles | at least 1, at most 16 (defaults are restored if empty) |
//! | general profile | at most one profile without apps (the first); later ones are dropped, since they could never be chosen. `apps` entries are trimmed, blanks and duplicates removed |
//! | slices per ring | at most 8; every profile has at least one slice |
//! | menu depth | at most [`MAX_DEPTH`](crate::config::MAX_DEPTH) = 5 (deeper submenus are emptied) |
//! | `active` | points at a `Mode` slice, otherwise the first `Mode` found (or empty) |
//! | `pie_hold_ms` | 50 to 2000 |
//! | `settings_hold_ms` | at least `pie_hold_ms + 300` |
//! | `double_click_ms` | 150 to 500 |
//! | `pie_size` | 140 to 360 |
//! | `preview_ms` | at most 3000 |
//! | `sleep_after_s` | 0 (never sleep) or 10 to 3600 |
//! | `trigger_button` | 2, 5, 6 or 7 (otherwise 2) |
//! | `flows` | at most [`MAX_FLOWS`](crate::config::MAX_FLOWS); unique non-empty ids; per flow see [`Flow::clean`](crate::config::Flow::clean): at most [`MAX_FLOW_NODES`](crate::config::MAX_FLOW_NODES) blocks, one `Start` and one `End`, waits of at most [`MAX_WAIT_MS`](crate::config::MAX_WAIT_MS), no loops |
//!
//! If you add a setting with a range, clamp it there and add a test.
//!
//! ## Profile files (sharing)
//!
//! One profile can be exported on its own ([`config::share`](crate::config::share)). The file is
//! `{ "cursorpie_profile": 1, "profile": { .. }, "icons": { "<icon string>": { "file", "data" } } }`:
//! the [`Profile`](crate::config::Profile) as in `config.json`, plus every image icon it uses as
//! base64. Importing parses it with [`parse_bundle`](crate::config::share::parse_bundle), writes the icons into
//! the icons folder ([`install_icons`](crate::config::share::install_icons): the extension comes from
//! the image bytes, names are reduced to safe characters, sizes are capped, nothing is written
//! outside the folder) and adds the result with [`add_profile`](crate::config::share::add_profile),
//! which requires an app, a free slot and a unique name. Bump `cursorpie_profile` only for changes an old
//! reader would misread. The file also carries `"flows": [..]`, the flows the profile's slices use
//! (left out when there are none); [`merge_flows`](crate::config::flow::merge_flows) adds them to the receiver's
//! config and points the slices at them. A single flow is shared as `{ "cursorpie_flow": 1, "flow": { .. } }`.
//!
//! ## Loading, saving and the broken-file path
//!
//! * [`load_config`](crate::config::load_config) runs once, lazily, the first time
//!   [`cfg()`](crate::config::cfg) is called. If the file is unreadable as a config it is copied
//!   to `config.broken.json` next to the original and the defaults are used. The next save
//!   overwrites `config.json`.
//! * Saves happen on *Save* in Settings (UI thread) and when the active mode changes
//!   ([`set_active`](crate::config::set_active), which saves on a worker thread because it is
//!   called from inside the event-tap callback). Both write through a temporary file that is
//!   renamed over `config.json`, under one lock, so a crash cannot leave half a file and two
//!   writers cannot interleave.
//! * *Save* uses [`save_config_with_backup`](crate::config::save_config_with_backup): the text of the
//!   file it replaces becomes `config.backup.1.json`, older backups move up to
//!   `config.backup.5.json`, and an identical or blank version is not stored (see
//!   [`config::backup`](crate::config::backup)). `set_active` makes no backup, since it saves on
//!   every mode pick. [`backups`](crate::config::backups) lists them and
//!   [`load_backup`](crate::config::load_backup) parses one through the normal validation.
//! * Import/Export use [`parse_config`](crate::config::parse_config) and
//!   [`export_config`](crate::config::export_config). After an import,
//!   [`has_commands`](crate::config::has_commands) decides whether to warn that the file runs
//!   shell commands.
//! * A change made by hand to `config.json` is picked up at the next start.
