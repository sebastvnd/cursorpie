//! The configuration: data model, defaults, validation and persistence.
//!
//! ```text
//! Config -> profiles[] -> items[] -> (Mode | Run | Submenu(items[]))
//! ```
//!
//! A profile with an empty `apps` list is the fallback for every app that has no profile of its own.
//!
//! | Submodule | Contents |
//! |---|---|
//! | `model` | the serialized types: [`Config`], [`Profile`], [`Item`], [`ItemKind`], [`Action`] and the option enums |
//! | `tree` | walking a menu by *path* (a list of indices), see [`items_at`] |
//! | `defaults` | built-in slices and `Config::default()` |
//! | `sanitize` | [`Config::sanitize`]: clamps everything into the supported limits |
//! | `store` | `config.json` on disk, import/export and the live global copy ([`cfg()`]) |
//! | [`flow`] | flows: chains of steps drawn as blocks and wires, and their own share file |
//! | [`share`] | exporting and importing one profile as a file, icons and flows included |
//! | [`backup`] | rolling backups of `config.json`, written by every Save in Settings |
//! | `first_run` | whether the welcome window is due (a marker file next to `config.json`) |
//!
//! All public items are re-exported here, so use `crate::config::X`. The file format is described in
//! [`guides::config_format`](crate::guides::config_format).

pub mod backup;
mod defaults;
mod first_run;
pub mod flow;
mod model;
mod sanitize;
pub mod share;
mod store;
mod tree;

pub use defaults::{blank_items, presets, quick_actions};
pub use first_run::{FirstRun, mark_welcomed, welcome_due};
pub use flow::{AskMode, CalcOp, CmpOp, Cond, Flow, FlowNode, LogicOp, NativeOp, NodeKind, Port};
pub use model::*;
pub use share::image_path;
pub use store::*;
pub use tree::*;

/// Format version written to every config file; files without it are rejected.
pub const CONFIG_VERSION: u32 = 2;
/// Max slices per ring.
pub const MAX_ITEMS: usize = 8;
/// Max menu levels (top level counts as 1).
pub const MAX_DEPTH: usize = 5;
/// Max number of profiles.
pub const MAX_PROFILES: usize = 16;
/// Max number of flows (see [`flow`]).
pub const MAX_FLOWS: usize = 32;
/// Max blocks in one flow, *Start* included.
pub const MAX_FLOW_NODES: usize = 128;
/// Most times a *Repeat* or *While* block sends the flow into its body in one run.
pub const MAX_REPEAT: u32 = 1_000;
/// Most blocks one run of a flow carries out, loops and called flows included. A flow that never
/// ends (a *While* whose condition stays true) is stopped here.
pub const MAX_FLOW_STEPS: u32 = 10_000;
/// Longest one run of a flow lasts, in seconds. Time spent waiting for an answer in an *Ask*
/// block does not count.
pub const MAX_FLOW_RUN_SECS: u64 = 600;
/// Deepest a flow can call another flow (a *Call* block) before the call is skipped.
pub const MAX_CALL_DEPTH: u8 = 8;
/// Longest a *Run* block waits for its command, in seconds.
pub const MAX_RUN_SECS: u32 = 120;
/// Longest text a field of a block holds, in characters.
pub const MAX_FIELD_CHARS: usize = 4_000;
/// Longest name of a variable of a flow, in characters.
pub const MAX_VAR_NAME_CHARS: usize = 32;
/// Most variables one run of a flow keeps.
pub const MAX_VARS: usize = 128;
/// Longest value of a variable, in characters. A longer one is cut.
pub const MAX_VALUE_CHARS: usize = 16_384;
/// Longest a *Wait* step lasts, in milliseconds. The actions worker does one thing at a time, so
/// a long wait would hold up every other action.
pub const MAX_WAIT_MS: u32 = 10_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_without_the_new_fields_still_loads() {
        // what a v0.2.0 file looks like: none of the side-button / app-detection settings
        let c = parse_config(r#"{"version": 2, "pie_hold_ms": 400}"#).expect("old config parses");
        assert_eq!(c.pie_hold_ms, 400);
        assert_eq!(c.target_app, TargetApp::UnderCursor);
        assert_eq!(c.side_default, SideTarget::App);
        assert!(c.focus_target && c.side_adopt && c.right_click_switch && c.side_indicator);
        assert!(c.center_app_icon);
        assert_eq!(
            c.app_icon_style,
            AppIconStyle::Glyph,
            "an old file keeps the silhouette"
        );
        assert_eq!(
            c.app_icon_shadow,
            IconShadow::Auto,
            "an old file gets the automatic shadow"
        );
        assert!(c.show_labels, "an old file keeps the names");
        assert!(!c.side_forget);
        assert_eq!(c.desktop_mode, DesktopMode::SwitchToApp);
    }

    #[test]
    fn icons_only_is_the_default_and_an_old_file_keeps_the_names() {
        assert!(!Config::default().show_labels);
        let old = parse_config(r#"{"version": 2}"#).expect("parses");
        assert!(
            old.show_labels,
            "a file without the setting keeps the names"
        );
        let c = parse_config(r#"{"version": 2, "show_labels": false}"#).expect("parses");
        assert!(!c.show_labels);
        let json = serde_json::to_string(&c).expect("serializes");
        assert!(json.contains(r#""show_labels":false"#));
        let on = parse_config(r#"{"version": 2, "show_labels": true}"#).expect("parses");
        assert!(on.show_labels);
    }

    #[test]
    fn the_browser_bridge_is_off_by_default_and_old_files_still_load() {
        assert!(!Config::default().browser_bridge);
        let old = parse_config(r#"{"version": 2}"#).expect("parses");
        assert!(!old.browser_bridge);
        let on = parse_config(r#"{"version": 2, "browser_bridge": true}"#).expect("parses");
        assert!(on.browser_bridge);
        let json = serde_json::to_string(&on).unwrap();
        assert!(json.contains(r#""browser_bridge":true"#));
    }

    #[test]
    fn the_update_check_is_on_by_default_and_old_files_get_it() {
        assert!(Config::default().check_updates);
        let old = parse_config(r#"{"version": 2}"#).expect("parses");
        assert!(old.check_updates);
        let off = parse_config(r#"{"version": 2, "check_updates": false}"#).expect("parses");
        assert!(!off.check_updates);
        let json = serde_json::to_string(&off).expect("serializes");
        assert!(json.contains(r#""check_updates":false"#));
    }

    #[test]
    fn the_open_app_choice_is_on_by_default_and_can_be_switched_off() {
        assert!(Config::default().open_app_choice);
        // a file from before the setting existed gets the new behavior
        let old = parse_config(r#"{"version": 2}"#).expect("parses");
        assert!(old.open_app_choice);
        let off = parse_config(r#"{"version": 2, "open_app_choice": false}"#).expect("parses");
        assert!(!off.open_app_choice);
        let json = serde_json::to_string(&off).expect("serializes");
        assert!(json.contains(r#""open_app_choice":false"#));
    }

    #[test]
    fn the_desktop_mode_is_stored_by_variant_name() {
        let c = parse_config(r#"{"version": 2, "desktop_mode": "DoNothing"}"#).unwrap();
        assert_eq!(c.desktop_mode, DesktopMode::DoNothing);
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""desktop_mode":"DoNothing""#));
        assert_eq!(
            serde_json::to_string(&DesktopMode::SwitchToApp).unwrap(),
            "\"SwitchToApp\""
        );
    }

    #[test]
    fn only_doing_nothing_stays_on_the_current_desktop() {
        assert_eq!(DesktopMode::default(), DesktopMode::SwitchToApp);
        assert!(DesktopMode::SwitchToApp.follows_app());
        assert!(DesktopMode::BringToCurrent.follows_app());
        assert!(!DesktopMode::DoNothing.follows_app());
        assert_eq!(Config::default().desktop_mode, DesktopMode::SwitchToApp);
    }

    #[test]
    fn a_mode_an_os_does_not_offer_becomes_switching() {
        use crate::platform::Os;
        for mode in [DesktopMode::SwitchToApp, DesktopMode::DoNothing] {
            assert_eq!(mode.effective(Os::Mac), mode);
            assert_eq!(mode.effective(Os::Windows), mode);
        }
        assert_eq!(
            DesktopMode::BringToCurrent.effective(Os::Mac),
            DesktopMode::BringToCurrent
        );
        assert_eq!(
            DesktopMode::BringToCurrent.effective(Os::Windows),
            DesktopMode::SwitchToApp
        );
        assert_eq!(
            DesktopMode::BringToCurrent.effective(Os::Linux),
            DesktopMode::SwitchToApp
        );
    }

    #[test]
    fn new_fields_round_trip_and_preview_is_clamped() {
        let c = parse_config(
            r#"{"version": 2, "target_app": "Focused", "side_default": "General", "preview_ms": 999999}"#,
        )
        .unwrap();
        assert_eq!(c.target_app, TargetApp::Focused);
        assert_eq!(c.side_default, SideTarget::General);
        assert_eq!(c.preview_ms, 3000);
    }

    #[test]
    fn only_one_general_profile_survives_and_apps_are_cleaned() {
        let item = r#"[{"name":"A","icon":"A","kind":{"Run":"Nothing"}}]"#;
        let json = format!(
            r#"{{"version": 2, "profiles": [
                {{"name":"General","apps":[],"items":{item},"active":[]}},
                {{"name":"Dead","apps":["  "],"items":{item},"active":[]}},
                {{"name":"Safari","apps":[" com.apple.Safari ","com.apple.Safari"],"items":{item},"active":[]}}
            ]}}"#
        );
        let c = parse_config(&json).unwrap();
        let names: Vec<_> = c.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["General", "Safari"],
            "the app-less duplicate is dropped"
        );
        // app ids are normalized per OS (lower case on Windows)
        let safari = crate::platform::OS.normalize_app_id("com.apple.Safari");
        assert_eq!(c.profiles[1].apps, [safari]);
        assert_eq!(c.profiles.iter().filter(|p| p.is_general()).count(), 1);
    }

    #[test]
    fn a_new_profile_needs_a_name_and_an_app() {
        assert!(Profile::for_app("Photo", "").is_none());
        assert!(Profile::for_app("  ", "com.x.App").is_none());
        let p = Profile::for_app(" Photo ", " com.x.App ").unwrap();
        assert_eq!(
            (p.name.as_str(), p.apps.len(), p.is_general()),
            ("Photo", 1, false)
        );
        assert!(!p.items.is_empty());
        assert!(!profile_has_commands(&p));
    }

    #[test]
    fn sleep_setting_defaults_and_is_clamped() {
        let old = parse_config(r#"{"version": 2}"#).unwrap();
        assert_eq!(old.sleep_after_s, Config::default().sleep_after_s);
        assert!(old.sleep_after_s > 0, "sleeping is on by default");
        let never = parse_config(r#"{"version": 2, "sleep_after_s": 0}"#).unwrap();
        assert_eq!(never.sleep_after_s, 0, "0 = never sleep, kept as it is");
        let low = parse_config(r#"{"version": 2, "sleep_after_s": 1}"#).unwrap();
        assert_eq!(low.sleep_after_s, 10);
        let high = parse_config(r#"{"version": 2, "sleep_after_s": 999999}"#).unwrap();
        assert_eq!(high.sleep_after_s, 3600);
    }
}
