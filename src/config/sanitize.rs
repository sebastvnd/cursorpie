//! Validation: forces a freshly loaded or edited [`Config`] back into the limits the app relies on.

use super::{
    CONFIG_VERSION, Config, HUD_MS_MAX, HUD_MS_MIN, HUD_SIZE_MAX, HUD_SIZE_MIN, Item, ItemKind,
    MAX_DEPTH, MAX_ITEMS, MAX_PROFILES, PIE_AT_LIMIT, item_at, presets, share::clean_apps,
};
use crate::platform::{Os, linux::roles};

/// The accent that versions before 0.7 started with. It is replaced by the new default.
const OLD_DEFAULT_ACCENT: [u8; 3] = [64, 156, 255];

/// Path of the first `Mode` slice in depth-first order, if there is one.
fn first_mode(items: &[Item]) -> Option<Vec<usize>> {
    for (i, it) in items.iter().enumerate() {
        match &it.kind {
            ItemKind::Mode { .. } => return Some(vec![i]),
            ItemKind::Submenu(c) => {
                if let Some(mut p) = first_mode(c) {
                    p.insert(0, i);
                    return Some(p);
                }
            }
            _ => {}
        }
    }
    None
}

/// Trims a ring to `MAX_ITEMS` slices and empties submenus nested deeper than `MAX_DEPTH`.
pub(super) fn clean_items(items: &mut Vec<Item>, depth: usize) {
    items.truncate(MAX_ITEMS);
    for it in items.iter_mut() {
        // where the pie of a submenu sits in the pie editor: a number that is not finite, or a
        // slice that is not a submenu, has none; the rest is kept on the canvas
        it.at = match (&it.kind, it.at) {
            (ItemKind::Submenu(_), Some([x, y])) if x.is_finite() && y.is_finite() => Some([
                x.clamp(-PIE_AT_LIMIT, PIE_AT_LIMIT),
                y.clamp(-PIE_AT_LIMIT, PIE_AT_LIMIT),
            ]),
            _ => None,
        };
        match &mut it.kind {
            ItemKind::Submenu(c) => {
                if depth + 1 >= MAX_DEPTH {
                    c.clear();
                } else {
                    clean_items(c, depth + 1);
                }
            }
            // back and forward have fields of their own; only real extra buttons are kept
            ItemKind::Mode { extra, .. } => extra.retain(|b, _| {
                crate::mouse::is_extra(*b)
                    && *b != crate::mouse::BACK
                    && *b != crate::mouse::FORWARD
            }),
            ItemKind::Smart(s) => s.sanitize(),
            ItemKind::Run(_) => {}
        }
    }
}

/// Turns the launcher commands of older Linux default configs (Files, Terminal, Browser) into roles
/// (see [`roles`]). A command always starts a new copy; an "Open app" slice raises the window that
/// exists.
fn upgrade_launchers(items: &mut [Item]) {
    for it in items {
        match &mut it.kind {
            ItemKind::Run(action) => roles::upgrade(action),
            ItemKind::Submenu(children) => upgrade_launchers(children),
            ItemKind::Mode { .. } | ItemKind::Smart(_) => {}
        }
    }
}

impl Config {
    /// Forces the config into the limits the rest of the program relies on (see
    /// [`guides::config_format`](crate::guides::config_format#validation) for the list).
    /// Called for every config that is loaded, imported or saved.
    pub fn sanitize(mut self) -> Self {
        self.version = CONFIG_VERSION;
        // Only the first profile without apps is kept (the general one). A second one could never
        // be chosen: profiles are picked by app, and the fallback is the first without apps.
        let mut have_general = false;
        for p in &mut self.profiles {
            p.apps = clean_apps(&p.apps);
        }
        self.profiles.retain(|p| {
            let extra_general = p.is_general() && have_general;
            have_general |= p.is_general();
            !extra_general
        });
        // An app has one profile only: the first profile that lists it keeps it. Taking it away
        // from the later ones changes nothing in use (the first one was always the one chosen);
        // a later profile left with no app was never reachable and goes.
        let mut claimed: Vec<String> = Vec::new();
        self.profiles.retain_mut(|p| {
            if p.is_general() {
                return true;
            }
            p.apps.retain(|a| {
                let free = !claimed
                    .iter()
                    .any(|c| crate::platform::OS.app_ids_equal(c, a));
                if free {
                    claimed.push(a.clone());
                }
                free
            });
            !p.apps.is_empty()
        });
        if self.profiles.is_empty() {
            self.profiles = Config::default().profiles;
        }
        self.profiles.truncate(MAX_PROFILES);
        self.double_click_ms = self.double_click_ms.clamp(150, 500);
        for p in &mut self.profiles {
            if p.items.is_empty() {
                p.items = vec![presets().remove(0)];
            }
            clean_items(&mut p.items, 0);
            // a config copied from another system may hold these commands: leave it alone there
            if matches!(Os::current(), Os::Linux) {
                upgrade_launchers(&mut p.items);
            }
            let ok = matches!(
                item_at(&p.items, &p.active).map(|i| &i.kind),
                Some(ItemKind::Mode { .. })
            );
            if !ok {
                p.active = first_mode(&p.items).unwrap_or_default();
            }
        }
        self.pie_hold_ms = self.pie_hold_ms.clamp(50, 2000);
        self.settings_hold_ms = self.settings_hold_ms.max(self.pie_hold_ms + 300);
        self.settings_space_presses = self.settings_space_presses.clamp(1, 2);
        self.smart.sanitize();
        self.pie_size = self.pie_size.clamp(140.0, 360.0);
        self.preview_ms = self.preview_ms.min(3000);
        self.hud_ms = self.hud_ms.clamp(HUD_MS_MIN, HUD_MS_MAX);
        self.hud_size = self.hud_size.clamp(HUD_SIZE_MIN, HUD_SIZE_MAX);
        if self.sleep_after_s != 0 {
            self.sleep_after_s = self.sleep_after_s.clamp(10, 3600);
        }
        {
            use crate::geometry::{
                TRAIL_FAN_RANGE, TRAIL_GLIDE_RANGE, TRAIL_NODE_RANGE, TRAIL_REACH_RANGE,
                TRAIL_SHRINK_RANGE,
            };
            self.trail_reach = self
                .trail_reach
                .clamp(TRAIL_REACH_RANGE.0, TRAIL_REACH_RANGE.1);
            self.trail_node = self
                .trail_node
                .clamp(TRAIL_NODE_RANGE.0, TRAIL_NODE_RANGE.1);
            self.trail_shrink = self
                .trail_shrink
                .clamp(TRAIL_SHRINK_RANGE.0, TRAIL_SHRINK_RANGE.1);
            self.trail_fan = self.trail_fan.clamp(TRAIL_FAN_RANGE.0, TRAIL_FAN_RANGE.1);
            self.trail_glide = self
                .trail_glide
                .clamp(TRAIL_GLIDE_RANGE.0, TRAIL_GLIDE_RANGE.1);
        }
        self.glass_clear = self.glass_clear.min(100);
        self.glass_frost = self.glass_frost.min(100);
        self.glass_shine = self.glass_shine.min(100);
        {
            use crate::geometry::{PIE_SIDES_RANGE, PIE_TURN_MAX};
            self.pie_sides = self.pie_sides.clamp(PIE_SIDES_RANGE.0, PIE_SIDES_RANGE.1);
            self.pie_round = self.pie_round.min(100);
            self.pie_turn = self.pie_turn.min(PIE_TURN_MAX);
        }
        // an id that cannot be a theme file name (a hand-edited config) is the default theme; one
        // that is just not there is kept, so the theme is back when its file is
        if !crate::ui::settings::theme::registry::is_valid_id(&self.ui_theme) {
            self.ui_theme = super::DEFAULT_UI_THEME.to_owned();
        }
        // the blue of earlier versions was the old default; it is the one that hurt the eyes
        if self.accent == OLD_DEFAULT_ACCENT {
            self.accent = Config::default().accent;
        }
        if !matches!(self.trigger_button, 2 | 5 | 6 | 7) {
            self.trigger_button = 2;
        }
        self.touch_still_mm = if self.touch_still_mm.is_finite() {
            self.touch_still_mm.clamp(0.5, 6.0)
        } else {
            Config::default().touch_still_mm
        };
        self.touch_typing_guard_ms = self.touch_typing_guard_ms.min(2000);
        // a combination of the keyboard trigger needs a modifier (a bare key would take typing
        // away) and may not be Esc (Esc closes a pie): anything else is no combination at all
        let usable = |s: &crate::keys::Shortcut| {
            s.mods & crate::keys::MOD_MASK != 0 && s.key != crate::keys::key::ESCAPE
        };
        self.key_trigger = self.key_trigger.filter(usable);
        self.key_trigger_app = self.key_trigger_app.filter(usable);
        // the same combination twice is one: the general pie keeps it
        if self.key_trigger_app.is_some_and(|a| {
            self.key_trigger
                .is_some_and(|g| (g.key, g.mods) == (a.key, a.mods))
        }) {
            self.key_trigger_app = None;
        }
        super::flow::clean_flows(&mut self.flows);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TouchClickAction;

    fn parse(json: &str) -> Config {
        crate::config::parse_config(json).expect("valid config")
    }

    /// What a file without any of the newer fields loads as: the defaults, except that a file
    /// saved before `show_labels` existed keeps the names (the default is icons only).
    fn defaults_of_an_old_file(version: u32) -> Config {
        Config {
            version,
            show_labels: true,
            ..Config::default()
        }
    }

    #[test]
    fn the_look_settings_default_and_are_clamped() {
        let old = parse(r#"{"version":2}"#);
        assert_eq!(old.appearance, crate::config::Appearance::System);
        assert_eq!(old.pie_style, crate::config::PieStyle::GenZ);
        let wild = parse(
            r#"{"version":2,"appearance":"Dark","pie_style":"Glass","glass_clear":200,"glass_frost":101,"glass_shine":255}"#,
        );
        assert_eq!(wild.appearance, crate::config::Appearance::Dark);
        assert_eq!(wild.pie_style, crate::config::PieStyle::Glass);
        assert_eq!(
            (wild.glass_clear, wild.glass_frost, wild.glass_shine),
            (100, 100, 100)
        );
    }

    #[test]
    fn settings_by_hold_and_by_space_default_and_are_clamped() {
        let old = parse(r#"{"version":2}"#);
        assert!(
            !old.settings_by_hold,
            "the hold is off unless it is switched on"
        );
        assert!(
            old.settings_by_space,
            "Space is the default way into Settings"
        );
        assert_eq!(old.settings_space_presses, 1);
        // a file that switched the hold on keeps it, and Space can still be turned off
        let kept = parse(r#"{"version":2,"settings_by_hold":true,"settings_by_space":false}"#);
        assert!(kept.settings_by_hold && !kept.settings_by_space);
        let wild = parse(
            r#"{"version":2,"settings_by_hold":false,"settings_by_space":true,"settings_space_presses":9}"#,
        );
        assert!(!wild.settings_by_hold && wild.settings_by_space);
        assert_eq!(wild.settings_space_presses, 2);
        let zero = parse(r#"{"version":2,"settings_space_presses":0}"#);
        assert_eq!(zero.settings_space_presses, 1);
    }

    #[test]
    fn the_pie_is_a_circle_by_default_and_the_shape_is_clamped() {
        let old = parse(r#"{"version":2}"#);
        assert!(!old.pie_custom_shape);
        assert!(old.outline().is_circle());
        let wild = parse(
            r#"{"version":2,"pie_custom_shape":true,"pie_sides":1,"pie_round":250,"pie_turn":9000}"#,
        );
        assert!(!wild.outline().is_circle());
        assert_eq!(
            (wild.pie_sides, wild.pie_round, wild.pie_turn),
            (3, 100, 359)
        );
    }

    #[test]
    fn the_trackpad_settings_are_off_by_default_and_old_files_load_unchanged() {
        let old = parse(r#"{"version":2}"#);
        assert!(!old.touch_trigger, "the trackpad trigger is opt-in");
        assert_eq!(old.touch_still_mm, 2.0);
        assert_eq!(old.touch_click_action, TouchClickAction::SwitchPie);
        assert_eq!(old.touch_typing_guard_ms, 400);
        assert!(
            old.touch_lock,
            "the lock gesture is on unless it is switched off"
        );
        assert!(old.haptics && old.haptic_hover);
        assert_eq!(old, defaults_of_an_old_file(old.version));
        // the two-finger mode is gone: the old field is ignored and never comes back
        let two = parse(r#"{"version":2,"touch_fingers":2}"#);
        assert_eq!(two, defaults_of_an_old_file(two.version));
        assert!(
            !serde_json::to_string(&two)
                .unwrap()
                .contains("touch_fingers")
        );
    }

    #[test]
    fn the_trackpad_settings_are_clamped() {
        let wild = parse(
            r#"{"version":2,"touch_trigger":true,"touch_still_mm":99.0,"touch_typing_guard_ms":99999,"touch_click_action":"PickHovered"}"#,
        );
        assert!(wild.touch_trigger);
        assert_eq!(wild.touch_still_mm, 6.0);
        assert_eq!(wild.touch_typing_guard_ms, 2000);
        assert_eq!(wild.touch_click_action, TouchClickAction::PickHovered);
        let tiny = parse(r#"{"version":2,"touch_still_mm":0.0}"#);
        assert_eq!(tiny.touch_still_mm, 0.5);
        let nan = Config {
            touch_still_mm: f32::NAN,
            ..Config::default()
        };
        assert_eq!(nan.sanitize().touch_still_mm, 2.0);
    }

    #[test]
    fn the_old_default_blue_becomes_the_new_default_accent() {
        let old = parse(r#"{"version":2,"accent":[64,156,255]}"#);
        assert_eq!(old.accent, Config::default().accent);
        let chosen = parse(r#"{"version":2,"accent":[255,45,85]}"#);
        assert_eq!(
            chosen.accent,
            [255, 45, 85],
            "a color the user picked stays"
        );
    }

    #[test]
    fn timings_and_sizes_are_clamped() {
        let c = parse(
            r#"{"version":2,"pie_hold_ms":1,"settings_hold_ms":1,"double_click_ms":9999,"pie_size":10}"#,
        );
        assert_eq!(c.pie_hold_ms, 50);
        assert_eq!(
            c.settings_hold_ms,
            c.pie_hold_ms + 300,
            "at least 300 ms after the pie"
        );
        assert_eq!(c.double_click_ms, 500);
        assert_eq!(c.pie_size, 140.0);
        let big = parse(r#"{"version":2,"pie_hold_ms":99999,"pie_size":9999}"#);
        assert_eq!(big.pie_hold_ms, 2000);
        assert_eq!(big.pie_size, 360.0);
    }

    #[test]
    fn the_place_of_a_pie_on_the_canvas_is_kept_cut_down_and_never_written_when_unset() {
        let c = parse(
            r#"{"version":2,"profiles":[{"name":"G","apps":[],"active":[0],"items":[
                {"name":"M","icon":"M","kind":{"Mode":{"back":"Nothing","forward":"Nothing"}},"at":[5.0,5.0]},
                {"name":"S","icon":"S","kind":{"Submenu":[]},"at":[120.5,-40.0]},
                {"name":"T","icon":"T","kind":{"Submenu":[]},"at":[99999.0,-99999.0]},
                {"name":"U","icon":"U","kind":{"Submenu":[]}}
            ]}]}"#,
        );
        let items = &c.profiles[0].items;
        assert_eq!(items[0].at, None, "only a submenu has a pie to place");
        assert_eq!(items[1].at, Some([120.5, -40.0]));
        assert_eq!(items[2].at, Some([PIE_AT_LIMIT, -PIE_AT_LIMIT]));
        assert_eq!(items[3].at, None);
        let nan = Item {
            at: Some([f32::NAN, 1.0]),
            ..Item::submenu("n", "ico:folder", Vec::new())
        };
        let mut cfg = Config::default();
        cfg.profiles[0].items.push(nan);
        let cfg = cfg.sanitize();
        assert_eq!(cfg.profiles[0].items.last().and_then(|i| i.at), None);
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(
            !json.contains("\"at\""),
            "a pie that was never moved writes nothing"
        );
    }

    #[test]
    fn extra_buttons_of_a_mode_are_kept_and_cleaned() {
        let c = parse(
            r#"{"version":2,"profiles":[{"name":"G","apps":[],"active":[0],"items":[
                {"name":"M","icon":"M","kind":{"Mode":{"back":"Nothing","forward":"Nothing",
                 "extra":{"5":{"OpenUrl":"a"},"3":{"OpenUrl":"b"},"2":{"OpenUrl":"c"},"99":"Nothing","7":"Nothing"}}}}
            ]}]}"#,
        );
        let ItemKind::Mode { extra, .. } = &c.profiles[0].items[0].kind else {
            panic!("not a mode");
        };
        let keys: Vec<i64> = extra.keys().copied().collect();
        assert_eq!(
            keys,
            [5, 7],
            "back, middle and out-of-range buttons are dropped"
        );
        // only buttons with an action are taken from the apps
        let used: Vec<i64> = crate::config::extra_buttons_in_use(&c)
            .into_iter()
            .collect();
        assert_eq!(used, [5]);
        // and a file written by an older version has none
        let old = parse(
            r#"{"version":2,"profiles":[{"name":"G","apps":[],"active":[0],"items":[
                {"name":"M","icon":"M","kind":{"Mode":{"back":"Nothing","forward":"Nothing"}}}
            ]}]}"#,
        );
        assert!(crate::config::extra_buttons_in_use(&old).is_empty());
        let json = serde_json::to_string(&old).unwrap();
        assert!(
            !json.contains("extra"),
            "nothing is written for a mode without extras"
        );
    }

    #[test]
    fn an_app_keeps_one_profile_the_first_one_that_lists_it() {
        let item = r#"{"name":"s","icon":"x","kind":{"Run":"Nothing"}}"#;
        let c = parse(&format!(
            r#"{{"version":2,"profiles":[
                {{"name":"G","apps":[],"items":[{item}],"active":[]}},
                {{"name":"A","apps":["one.app","two.app"],"items":[{item}],"active":[]}},
                {{"name":"B","apps":["two.app","three.app"],"items":[{item}],"active":[]}},
                {{"name":"C","apps":["one.app"],"items":[{item}],"active":[]}}]}}"#
        ));
        let names: Vec<&str> = c.profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["G", "A", "B"],
            "C had nothing left and was never reachable"
        );
        assert_eq!(c.profiles[1].apps.len(), 2);
        assert_eq!(c.profiles[2].apps.len(), 1, "B keeps only its own app");
        assert_eq!(c.profile_owning("two.app", None), Some(1));
        assert_eq!(c.profile_owning("two.app", Some(1)), None);
    }

    #[test]
    fn only_known_trigger_buttons_survive() {
        for ok in [2, 5, 6, 7] {
            let c = parse(&format!(r#"{{"version":2,"trigger_button":{ok}}}"#));
            assert_eq!(c.trigger_button, ok);
        }
        for bad in [0, 1, 3, 4, 8, -1] {
            let c = parse(&format!(r#"{{"version":2,"trigger_button":{bad}}}"#));
            assert_eq!(c.trigger_button, 2, "{bad} falls back to the middle button");
        }
    }

    #[test]
    fn rings_and_profiles_are_limited() {
        let item = |n: usize| {
            (0..n)
                .map(|i| format!(r#"{{"name":"{i}","icon":"x","kind":{{"Run":"Nothing"}}}}"#))
                .collect::<Vec<_>>()
                .join(",")
        };
        let c = parse(&format!(
            r#"{{"version":2,"profiles":[{{"name":"G","apps":[],"items":[{}],"active":[]}}]}}"#,
            item(12)
        ));
        assert_eq!(c.profiles[0].items.len(), MAX_ITEMS);
        let many: Vec<String> = (0..30)
            .map(|i| {
                format!(
                    r#"{{"name":"p{i}","apps":["a{i}"],"items":[{}],"active":[]}}"#,
                    item(1)
                )
            })
            .collect();
        let c = parse(&format!(
            r#"{{"version":2,"profiles":[{}]}}"#,
            many.join(",")
        ));
        assert_eq!(c.profiles.len(), MAX_PROFILES);
    }

    #[test]
    fn an_empty_profile_gets_a_starter_slice_and_an_active_mode() {
        let c =
            parse(r#"{"version":2,"profiles":[{"name":"G","apps":[],"items":[],"active":[7]}]}"#);
        assert!(!c.profiles[0].items.is_empty());
        assert_eq!(c.profiles[0].active, vec![0], "the starter slice is a mode");
        let none = parse(r#"{"version":2,"profiles":[]}"#);
        assert_eq!(none.profiles.len(), Config::default().profiles.len());
    }

    #[test]
    fn a_bad_active_path_is_repaired() {
        let json = r#"{"version":2,"profiles":[{"name":"G","apps":[],
            "items":[{"name":"r","icon":"x","kind":{"Run":"Nothing"}},
                     {"name":"m","icon":"x","kind":{"Mode":{"back":"Nothing","forward":"Nothing"}}}],
            "active":[0]}]}"#;
        let c = parse(json);
        assert_eq!(c.profiles[0].active, vec![1], "points at the first mode");
    }

    #[test]
    fn deep_submenus_are_emptied() {
        fn nest(depth: usize) -> String {
            if depth == 0 {
                return r#"{"name":"leaf","icon":"x","kind":{"Run":"Nothing"}}"#.into();
            }
            format!(
                r#"{{"name":"s","icon":"x","kind":{{"Submenu":[{}]}}}}"#,
                nest(depth - 1)
            )
        }
        let json = format!(
            r#"{{"version":2,"profiles":[{{"name":"G","apps":[],"items":[{}],"active":[]}}]}}"#,
            nest(8)
        );
        let c = parse(&json);
        let mut depth = 1;
        let mut items = &c.profiles[0].items;
        while let Some(ItemKind::Submenu(ch)) = items.first().map(|i| &i.kind) {
            if ch.is_empty() {
                break;
            }
            depth += 1;
            items = ch;
        }
        assert!(depth <= MAX_DEPTH, "depth {depth}");
    }

    #[test]
    fn old_launcher_commands_become_roles_in_every_ring() {
        use crate::config::Action;
        let cmd = |c: &str| Action::Command(c.into());
        let mut items = vec![
            Item::run("Files", "ico:folder-simple", cmd(roles::FILES_COMMAND)),
            Item::submenu(
                "Launch",
                "ico:rocket-launch",
                vec![
                    Item::run("Browser", "ico:globe", cmd(roles::BROWSER_COMMAND)),
                    Item::run("Mine", "ico:crop", cmd("ls")),
                ],
            ),
        ];
        upgrade_launchers(&mut items);
        let kind = |i: &Item| match &i.kind {
            ItemKind::Run(a) => a.clone(),
            other => panic!("not a slice: {other:?}"),
        };
        assert_eq!(kind(&items[0]), Action::OpenApp("@files".into()));
        let ItemKind::Submenu(inner) = &items[1].kind else {
            panic!("a submenu");
        };
        assert_eq!(kind(&inner[0]), Action::OpenApp("@browser".into()));
        assert_eq!(kind(&inner[1]), cmd("ls"), "a command of the user stays");
    }

    // ------------------------------------------------------------ the keyboard trigger

    #[test]
    fn the_keyboard_trigger_is_off_by_default_and_an_old_file_keeps_it_off() {
        let c = parse(r#"{"version":2}"#);
        assert_eq!(c.key_trigger, None);
        assert_eq!(c.key_trigger_app, None);
        assert_eq!(c.key_trigger_mode, crate::config::KeyPieMode::Auto);
    }

    #[test]
    fn a_combination_survives_saving_and_loading() {
        use crate::keys::{MOD_CTRL, MOD_SHIFT, Shortcut, key};
        let c = Config {
            key_trigger: Some(Shortcut::new(key::SPACE, MOD_CTRL)),
            key_trigger_app: Some(Shortcut::new(key::SPACE, MOD_CTRL | MOD_SHIFT)),
            key_trigger_mode: crate::config::KeyPieMode::Toggle,
            ..Default::default()
        };
        let json = serde_json::to_string(&c).unwrap();
        let back = parse(&json);
        assert_eq!(back.key_trigger, c.key_trigger);
        assert_eq!(back.key_trigger_app, c.key_trigger_app);
        assert_eq!(back.key_trigger_mode, crate::config::KeyPieMode::Toggle);
    }

    #[test]
    fn a_combination_without_a_modifier_or_with_esc_is_dropped() {
        use crate::keys::{MOD_CTRL, Shortcut, key};
        let c = Config {
            key_trigger: Some(Shortcut::new(key::SPACE, 0)),
            key_trigger_app: Some(Shortcut::new(key::ESCAPE, MOD_CTRL)),
            ..Default::default()
        };
        let c = c.sanitize();
        assert_eq!(c.key_trigger, None, "a bare key would take typing away");
        assert_eq!(c.key_trigger_app, None, "Esc closes a pie");
    }

    #[test]
    fn the_same_combination_twice_is_one_and_the_general_pie_keeps_it() {
        use crate::keys::{MOD_CTRL, Shortcut, key};
        let sc = Shortcut::new(key::SPACE, MOD_CTRL);
        let c = Config {
            key_trigger: Some(sc),
            key_trigger_app: Some(sc),
            ..Default::default()
        };
        let c = c.sanitize();
        assert_eq!(c.key_trigger, Some(sc));
        assert_eq!(c.key_trigger_app, None);
    }

    #[test]
    fn recording_a_combination_that_the_other_has_moves_it() {
        use crate::keys::{MOD_CTRL, MOD_SHIFT, Shortcut, key};
        let a = Shortcut::new(key::SPACE, MOD_CTRL);
        let b = Shortcut::new(key::SPACE, MOD_CTRL | MOD_SHIFT);
        let mut c = Config::default();
        c.set_key_trigger(false, a);
        c.set_key_trigger(true, b);
        assert_eq!((c.key_trigger, c.key_trigger_app), (Some(a), Some(b)));
        c.set_key_trigger(true, a); // the one of the app takes it from the general pie
        assert_eq!((c.key_trigger, c.key_trigger_app), (None, Some(a)));
        c.set_key_trigger(false, Shortcut::new(key::SPACE, 0)); // not usable: nothing changes
        assert_eq!((c.key_trigger, c.key_trigger_app), (None, Some(a)));
    }
}
