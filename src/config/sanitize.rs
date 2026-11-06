//! Validation: forces a freshly loaded or edited [`Config`] back into the limits the app relies on.

use super::{
    CONFIG_VERSION, Config, Item, ItemKind, MAX_DEPTH, MAX_ITEMS, MAX_PROFILES, item_at, presets,
    share::clean_apps,
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
fn clean_items(items: &mut Vec<Item>, depth: usize) {
    items.truncate(MAX_ITEMS);
    for it in items.iter_mut() {
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
            ItemKind::Mode { .. } => {}
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
        self.pie_size = self.pie_size.clamp(140.0, 360.0);
        self.preview_ms = self.preview_ms.min(3000);
        if self.sleep_after_s != 0 {
            self.sleep_after_s = self.sleep_after_s.clamp(10, 3600);
        }
        self.glass_clear = self.glass_clear.min(100);
        self.glass_frost = self.glass_frost.min(100);
        self.glass_shine = self.glass_shine.min(100);
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

    #[test]
    fn the_look_settings_default_and_are_clamped() {
        let old = parse(r#"{"version":2}"#);
        assert_eq!(old.appearance, crate::config::Appearance::System);
        assert_eq!(old.pie_style, crate::config::PieStyle::Solid);
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
    fn the_trackpad_settings_are_off_by_default_and_old_files_load_unchanged() {
        let old = parse(r#"{"version":2}"#);
        assert!(!old.touch_trigger, "the trackpad trigger is opt-in");
        assert_eq!(old.touch_still_mm, 2.0);
        assert_eq!(old.touch_click_action, TouchClickAction::SwitchPie);
        assert_eq!(old.touch_typing_guard_ms, 400);
        assert!(old.haptics && old.haptic_hover);
        assert_eq!(
            old,
            Config {
                version: old.version,
                ..Config::default()
            }
        );
        // the two-finger mode is gone: the old field is ignored and never comes back
        let two = parse(r#"{"version":2,"touch_fingers":2}"#);
        assert_eq!(
            two,
            Config {
                version: two.version,
                ..Config::default()
            }
        );
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
        assert_eq!(c.settings_hold_ms, 450, "at least 300 ms after the pie");
        assert_eq!(c.double_click_ms, 500);
        assert_eq!(c.pie_size, 140.0);
        let big = parse(r#"{"version":2,"pie_hold_ms":99999,"pie_size":9999}"#);
        assert_eq!(big.pie_hold_ms, 2000);
        assert_eq!(big.pie_size, 360.0);
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
}
