//! The settings of the intelligent pie and of the intelligent slice (see [`smart`](crate::smart)).
//!
//! Two things are serialized here:
//!
//! * [`SmartConfig`], the `smart` object of `config.json`: whether CursorPie learns at all, what it
//!   may learn from, how long Space is held to switch pies, what the suggestions are made of.
//! * [`SmartSlice`], the payload of an `ItemKind::Smart` slice: a submenu whose slices are not
//!   written by hand but suggested when the pie opens.
//!
//! Everything is **off by default**: CursorPie learns nothing until the person switches
//! [`SmartConfig::enabled`] on in Settings > Intelligent. The habits themselves are not in
//! `config.json`; they are in their own file (see [`smart::store`](crate::smart)).

use serde::{Deserialize, Serialize};

/// Where a suggestion comes from.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SmartSource {
    /// Apps that are open right now (the ones CursorPie has met before), the ones you use most
    /// first.
    OpenApps,
    /// The apps you use most: weighed by how long you used them, how lately, at this time of day,
    /// and by what you usually open after the app that is in front.
    FrequentApps,
    /// Web pages that are open in the browser right now. Needs the browser extension.
    OpenPages,
    /// The web pages (sites) you visit most. Needs the browser extension.
    FrequentPages,
    /// What you run from the pie most in the app in front: shortcuts, commands, links, sub-actions.
    Actions,
    /// Your flows, the ones you run most first.
    Flows,
}

impl SmartSource {
    /// Every source, in the order Settings lists them.
    pub const ALL: [SmartSource; 6] = [
        SmartSource::OpenApps,
        SmartSource::FrequentApps,
        SmartSource::OpenPages,
        SmartSource::FrequentPages,
        SmartSource::Actions,
        SmartSource::Flows,
    ];

    /// The name Settings and the grouped pie show.
    pub fn label(self) -> &'static str {
        match self {
            SmartSource::OpenApps => "Open apps",
            SmartSource::FrequentApps => "Frequent apps",
            SmartSource::OpenPages => "Open pages",
            SmartSource::FrequentPages => "Frequent pages",
            SmartSource::Actions => "Actions for this app",
            SmartSource::Flows => "Flows",
        }
    }

    /// The slice icon (`ico:<name>`, see [`ui::symbols`](crate::ui::symbols)) of the source.
    pub fn icon(self) -> &'static str {
        match self {
            SmartSource::OpenApps => "ico:app-window",
            SmartSource::FrequentApps => "ico:rocket-launch",
            SmartSource::OpenPages => "ico:browser",
            SmartSource::FrequentPages => "ico:globe",
            SmartSource::Actions => "ico:lightning",
            SmartSource::Flows => "ico:magic-wand",
        }
    }
}

/// How the intelligent pie lays out its suggestions.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Default)]
pub enum SmartLayout {
    /// One ring of the best suggestions of every source mixed together, the best first.
    Mixed,
    /// One slice per category, each an intelligent submenu: *Apps*, *Sites*, *Actions*, *Flows*.
    /// The default: a pie that mixes apps, sites and actions is hard to read at a glance.
    #[default]
    Grouped,
}

/// The payload of an intelligent slice: what its ring is made of when the pie opens.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct SmartSlice {
    /// What the ring draws from. Empty: every source switched on in [`SmartConfig::sources`].
    #[serde(default)]
    pub sources: Vec<SmartSource>,
    /// How many suggestions the ring has at most (1 to [`MAX_ITEMS`](super::MAX_ITEMS)).
    pub count: u8,
}

impl Default for SmartSlice {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            count: 6,
        }
    }
}

impl SmartSlice {
    /// An intelligent slice of one source.
    pub fn of(source: SmartSource) -> Self {
        Self {
            sources: vec![source],
            count: super::MAX_ITEMS as u8,
        }
    }
}

/// Shortest and longest hold of Space that switches pies, in milliseconds.
pub const SMART_HOLD_MS_MIN: u64 = 400;
pub const SMART_HOLD_MS_MAX: u64 = 5000;
/// Smallest and largest number of slices of the intelligent pie.
pub const SMART_SLICES_MIN: u8 = 3;
pub const SMART_SLICES_MAX: u8 = super::MAX_ITEMS as u8;
/// Shortest and longest time without input before CursorPie counts you as idle, in seconds.
pub const SMART_IDLE_S_MIN: u64 = 10;
pub const SMART_IDLE_S_MAX: u64 = 3600;
/// Shortest and longest time between two looks at what is open, in seconds.
pub const SMART_SAMPLE_S_MIN: u64 = 2;
pub const SMART_SAMPLE_S_MAX: u64 = 120;
/// Longest and shortest time habits are kept, in days (0 is not allowed: use *Forget everything*).
pub const SMART_KEEP_DAYS_MIN: u32 = 7;
pub const SMART_KEEP_DAYS_MAX: u32 = 730;
/// Most entries of each of the two ignore lists.
pub const SMART_IGNORE_MAX: usize = 64;

/// The `smart` object of `config.json`. Every field missing from a file takes its default, so a file
/// from before the intelligent pie existed loads with everything off.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct SmartConfig {
    /// The master switch. Off: nothing is observed, nothing is stored, Space held in a pie does
    /// not switch, an intelligent slice is empty. On by the person, in Settings > Intelligent.
    pub enabled: bool,
    /// How long Space is held in an open pie to switch to the intelligent pie, and to come back
    /// from it, in milliseconds. A quick press of Space still opens Settings.
    pub hold_ms: u64,
    /// Learn which apps are in front, for how long, and what follows what.
    pub learn_apps: bool,
    /// Learn which web pages (sites) you look at. Needs the browser extension.
    pub learn_pages: bool,
    /// Learn what you run from the pie, and in which app.
    pub learn_actions: bool,
    /// Do the heavy part of learning (fading old habits, working out names, saving) only while you
    /// are idle. Off: it is done every time the habits are saved, a minute or so apart.
    pub learn_when_idle: bool,
    /// How long without a key or a pointer movement counts as idle, in seconds.
    pub idle_s: u64,
    /// How often CursorPie looks at the app in front and the open pages, in seconds.
    pub sample_s: u64,
    /// What the intelligent pie and the intelligent slices (that name no source) draw from.
    pub sources: Vec<SmartSource>,
    /// How the intelligent pie lays them out.
    pub layout: SmartLayout,
    /// Whether the person picked the layout in Settings. Until they do the layout is
    /// [`SmartLayout::Grouped`] whatever the file says, so a file written before grouping was the
    /// default (it holds `Mixed` only because that was the default then) does not keep the old one.
    pub layout_chosen: bool,
    /// How many slices the intelligent pie has (when mixed).
    pub pie_slices: u8,
    /// Weigh the time of day: what you open in the morning is suggested more in the morning.
    pub time_of_day: bool,
    /// Keep the path of a page, not only its site (`github.com/rust-lang/rust`, not `github.com`).
    /// Off by default: a path can say much more than a site does.
    pub keep_page_paths: bool,
    /// Apps that are never learned and never suggested (their ids, as in a profile).
    pub ignore_apps: Vec<String>,
    /// Sites that are never learned and never suggested (`bank.example`; subdomains included).
    pub ignore_sites: Vec<String>,
    /// Habits older than this many days are forgotten, and the ones that are left fade (the
    /// half-life is a quarter of it).
    pub keep_days: u32,
    /// Ask the sites that are suggested for their own icon (the same lookup as *Icon from a
    /// website*), so a suggested site shows its logo and not a placeholder. The icons are files in
    /// the icons folder; *Forget everything* deletes them.
    pub site_icons: bool,
}

impl Default for SmartConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hold_ms: 900,
            learn_apps: true,
            learn_pages: true,
            learn_actions: true,
            learn_when_idle: true,
            idle_s: 60,
            sample_s: 10,
            sources: SmartSource::ALL.to_vec(),
            layout: SmartLayout::Grouped,
            layout_chosen: false,
            pie_slices: SMART_SLICES_MAX,
            time_of_day: true,
            keep_page_paths: false,
            ignore_apps: Vec::new(),
            ignore_sites: Vec::new(),
            keep_days: 120,
            site_icons: true,
        }
    }
}

impl SmartConfig {
    /// Whether `source` is switched on.
    pub fn has(&self, source: SmartSource) -> bool {
        self.sources.contains(&source)
    }

    /// Switches `source` on or off, keeping the order of [`SmartSource::ALL`].
    pub fn set(&mut self, source: SmartSource, on: bool) {
        self.sources.retain(|s| *s != source);
        if on {
            self.sources.push(source);
            self.sources.sort();
        }
    }

    /// The sources a slice draws from: its own, or every one that is switched on.
    pub fn sources_for(&self, slice: &SmartSlice) -> Vec<SmartSource> {
        if slice.sources.is_empty() {
            self.sources.clone()
        } else {
            slice.sources.clone()
        }
    }

    /// Whether anything is being learned: the switch is on and at least one kind of habit is
    /// allowed.
    pub fn learning(&self) -> bool {
        self.enabled && (self.learn_apps || self.learn_pages || self.learn_actions)
    }

    /// Whether the app `id` is on the ignore list.
    pub fn ignores_app(&self, id: &str) -> bool {
        self.ignore_apps
            .iter()
            .any(|a| crate::platform::OS.app_ids_equal(a, id))
    }

    /// Whether `host` (or a site it belongs to) is on the ignore list.
    pub fn ignores_site(&self, host: &str) -> bool {
        let host = host.trim().to_ascii_lowercase();
        self.ignore_sites.iter().any(|s| {
            let s = s.trim().to_ascii_lowercase();
            !s.is_empty() && (host == s || host.ends_with(&format!(".{s}")))
        })
    }

    /// Cuts every number into its range and tidies the lists. Called by `Config::sanitize`.
    pub fn sanitize(&mut self) {
        if !self.layout_chosen {
            self.layout = SmartLayout::Grouped;
        }
        self.hold_ms = self.hold_ms.clamp(SMART_HOLD_MS_MIN, SMART_HOLD_MS_MAX);
        self.idle_s = self.idle_s.clamp(SMART_IDLE_S_MIN, SMART_IDLE_S_MAX);
        self.sample_s = self.sample_s.clamp(SMART_SAMPLE_S_MIN, SMART_SAMPLE_S_MAX);
        self.pie_slices = self.pie_slices.clamp(SMART_SLICES_MIN, SMART_SLICES_MAX);
        self.keep_days = self
            .keep_days
            .clamp(SMART_KEEP_DAYS_MIN, SMART_KEEP_DAYS_MAX);
        self.sources.sort();
        self.sources.dedup();
        for list in [&mut self.ignore_apps, &mut self.ignore_sites] {
            for e in list.iter_mut() {
                *e = e.trim().chars().take(200).collect();
            }
            list.retain(|e| !e.is_empty());
            list.dedup();
            list.truncate(SMART_IGNORE_MAX);
        }
    }
}

impl SmartSlice {
    /// Cuts the slice into its limits.
    pub fn sanitize(&mut self) {
        self.sources.sort();
        self.sources.dedup();
        self.count = self.count.clamp(1, super::MAX_ITEMS as u8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everything_is_off_until_the_person_switches_it_on() {
        let c = SmartConfig::default();
        assert!(!c.enabled && !c.learning());
        assert!(!c.keep_page_paths, "a path says more than a site");
    }

    #[test]
    fn the_intelligent_pie_is_grouped_and_asks_for_site_icons_by_default() {
        let c = SmartConfig::default();
        assert_eq!(c.layout, SmartLayout::Grouped);
        assert!(c.site_icons);
    }

    #[test]
    fn an_old_file_that_holds_mixed_gets_the_grouped_pie_until_the_person_chooses() {
        let c = crate::config::parse_config(
            r#"{"version":2,"smart":{"enabled":true,"layout":"Mixed"}}"#,
        )
        .expect("parses");
        assert_eq!(c.smart.layout, SmartLayout::Grouped);
        let c = crate::config::parse_config(
            r#"{"version":2,"smart":{"enabled":true,"layout":"Mixed","layout_chosen":true}}"#,
        )
        .expect("parses");
        assert_eq!(c.smart.layout, SmartLayout::Mixed, "a choice is kept");
    }

    #[test]
    fn a_file_without_the_object_loads_with_the_defaults() {
        let c = crate::config::parse_config(r#"{"version": 2}"#).expect("parses");
        assert_eq!(c.smart, SmartConfig::default());
    }

    #[test]
    fn a_partial_object_keeps_the_rest_of_the_defaults() {
        let c =
            crate::config::parse_config(r#"{"version":2,"smart":{"enabled":true,"hold_ms":1500}}"#)
                .expect("parses");
        assert!(c.smart.enabled);
        assert_eq!(c.smart.hold_ms, 1500);
        assert_eq!(c.smart.idle_s, SmartConfig::default().idle_s);
    }

    #[test]
    fn numbers_are_cut_into_their_ranges() {
        let mut c = SmartConfig {
            hold_ms: 1,
            idle_s: 0,
            sample_s: 9999,
            pie_slices: 99,
            keep_days: 0,
            ..SmartConfig::default()
        };
        c.sanitize();
        assert_eq!(c.hold_ms, SMART_HOLD_MS_MIN);
        assert_eq!(c.idle_s, SMART_IDLE_S_MIN);
        assert_eq!(c.sample_s, SMART_SAMPLE_S_MAX);
        assert_eq!(c.pie_slices, SMART_SLICES_MAX);
        assert_eq!(c.keep_days, SMART_KEEP_DAYS_MIN);
    }

    #[test]
    fn ignore_lists_are_trimmed_and_bounded() {
        let mut c = SmartConfig {
            ignore_sites: vec!["  bank.example ".into(), "".into(), "   ".into()],
            ignore_apps: (0..200).map(|i| format!("app{i}")).collect(),
            ..SmartConfig::default()
        };
        c.sanitize();
        assert_eq!(c.ignore_sites, ["bank.example"]);
        assert_eq!(c.ignore_apps.len(), SMART_IGNORE_MAX);
    }

    #[test]
    fn a_site_on_the_list_covers_its_subdomains_only() {
        let c = SmartConfig {
            ignore_sites: vec!["bank.example".into()],
            ..SmartConfig::default()
        };
        assert!(c.ignores_site("bank.example"));
        assert!(c.ignores_site("www.Bank.example"));
        assert!(!c.ignores_site("notbank.example"));
        assert!(!c.ignores_site("example"));
    }

    #[test]
    fn sources_can_be_switched_one_by_one() {
        let mut c = SmartConfig::default();
        assert!(c.has(SmartSource::Flows));
        c.set(SmartSource::Flows, false);
        assert!(!c.has(SmartSource::Flows));
        c.set(SmartSource::Flows, true);
        c.set(SmartSource::Flows, true);
        assert_eq!(c.sources.len(), SmartSource::ALL.len(), "no duplicate");
    }

    #[test]
    fn a_slice_without_sources_uses_the_switched_on_ones() {
        let mut c = SmartConfig::default();
        c.set(SmartSource::OpenPages, false);
        let any = SmartSlice::default();
        assert!(!c.sources_for(&any).contains(&SmartSource::OpenPages));
        // a slice that names a source gets it even if the pie leaves it out
        let own = SmartSlice::of(SmartSource::OpenPages);
        assert_eq!(c.sources_for(&own), [SmartSource::OpenPages]);
    }
}
