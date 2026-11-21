//! From habits and the moment to slices: what the intelligent pie and the intelligent slices show.
//!
//! Pure functions of an [`Input`], so the ranking is unit tested without a window, a thread or a
//! file. Nothing here reads the system; [`collect`](super::collect) gathers the [`Context`].
//!
//! # How a ring is made
//!
//! 1. Every source in the list makes its **candidates**, best first ([`candidates`]). A candidate
//!    is a ready slice with a score that only has to be comparable inside its source.
//! 2. Each list is scaled so its best candidate is `1.0` and every next one is worth `0.8` of the
//!    one before ([`RANK_DECAY`]), times the weight of the source. This is what mixes the sources:
//!    the second app is worth about as much as the best page, the third app less than the second
//!    page, so one source never fills the ring unless it is the only one asked for.
//! 3. The mixed list is sorted, a slice that two sources both offered (the same flow as a flow and
//!    as an action) is kept once, and the first `count` are the ring.
//!
//! The app **in front** and the page that is **in view** are never suggested: you are there.

use super::habits::{Habits, action_key, half_life, page_of, readable_id};
use crate::config::{
    Action, AppIconStyle, Item, ItemKind, MAX_ITEMS, SmartConfig, SmartLayout, SmartSlice,
    SmartSource,
};
use crate::ui::appicon;
use std::collections::{HashMap, HashSet};

/// What the next slice of a source is worth compared to the one before it.
pub const RANK_DECAY: f32 = 0.8;
/// How much a certain "this app usually comes next" adds to an app's score: about what doubling the
/// time spent in it does, so the app you always go to from here can lead the ring.
pub const NEXT_WEIGHT: f32 = 4.0;
/// Longest name of a suggested slice, in characters.
pub const NAME_CHARS: usize = 18;
/// Most candidates one source makes.
const MAX_CANDIDATES: usize = 24;

/// What is going on right now, as far as CursorPie could tell when the pie was asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    /// The app the pie is about (the one in front, or under the pointer), if known.
    pub front: Option<String>,
    /// The apps CursorPie knows that are running now.
    pub running: HashSet<String>,
    /// The web tabs that are open: `(address, is the one in view)`.
    pub tabs: Vec<(String, bool)>,
    /// The icon string of each site that has an icon file, by host name (see
    /// [`siteicon`](super::siteicon)). Looked up once per gesture, so a suggestion never touches
    /// the disk.
    pub site_icons: HashMap<String, String>,
}

/// A flow, as far as a suggestion needs to know it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowRef {
    pub id: String,
    pub name: String,
}

/// Everything a suggestion is made from. All borrowed: making one never changes anything.
pub struct Input<'a> {
    pub habits: &'a Habits,
    pub ctx: &'a Context,
    pub cfg: &'a SmartConfig,
    pub flows: &'a [FlowRef],
    /// Seconds since the epoch.
    pub now: u64,
}

/// A suggested slice and what it is worth.
#[derive(Clone, Debug, PartialEq)]
pub struct Cand {
    /// What makes two candidates the same slice.
    pub key: String,
    pub item: Item,
    pub source: SmartSource,
    pub score: f32,
}

/// How much a source counts when several are mixed.
fn weight(source: SmartSource) -> f32 {
    match source {
        SmartSource::OpenApps | SmartSource::Actions => 1.0,
        SmartSource::OpenPages => 0.95,
        SmartSource::FrequentApps => 0.9,
        SmartSource::FrequentPages => 0.85,
        SmartSource::Flows => 0.8,
    }
}

/// `name` cut to [`NAME_CHARS`] characters, with an ellipsis when it was longer.
pub fn short(name: &str) -> String {
    let name = name.trim();
    if name.chars().count() <= NAME_CHARS {
        return name.to_string();
    }
    let mut s: String = name.chars().take(NAME_CHARS - 1).collect();
    s.push('\u{2026}');
    s
}

/// The text an "Open app" action stores for the app `id`: what was learned, otherwise the id (the
/// name on macOS, where `open -a` takes names and not bundle ids).
fn launch_text(id: &str, launch: &str, label: &str) -> String {
    if !launch.trim().is_empty() {
        return launch.trim().to_string();
    }
    if matches!(crate::platform::OS, crate::platform::Os::Mac) {
        return if label.is_empty() {
            readable_id(id)
        } else {
            label.to_string()
        };
    }
    id.to_string()
}

fn app_item(id: &str, input: &Input) -> Item {
    let (label, launch) = input
        .habits
        .apps
        .get(id)
        .map_or(("", ""), |a| (a.label.as_str(), a.launch.as_str()));
    let name = if label.is_empty() {
        readable_id(id)
    } else {
        label.to_string()
    };
    Item::run(
        &short(&name),
        &appicon::icon_string(AppIconStyle::Color, id),
        Action::OpenApp(launch_text(id, launch, label)),
    )
}

/// The icon of the site `host`: its own icon when it was fetched, otherwise `fallback`.
fn site_icon(host: &str, input: &Input, fallback: &str) -> String {
    if input.cfg.site_icons
        && let Some(icon) = input.ctx.site_icons.get(host)
    {
        return icon.clone();
    }
    fallback.to_string()
}

fn hl(input: &Input) -> f64 {
    half_life(input.cfg.keep_days)
}

fn allowed_app(id: &str, input: &Input) -> bool {
    !super::is_self(id) && !input.cfg.ignores_app(id) && input.ctx.front.as_deref() != Some(id)
}

/// The key of the page that is in view, if the browser told.
fn page_in_view(input: &Input) -> Option<String> {
    input
        .ctx
        .tabs
        .iter()
        .find(|(_, active)| *active)
        .and_then(|(url, _)| page_of(url, input.cfg.keep_page_paths))
        .map(|(key, _, _)| key)
}

fn sort_desc(list: &mut [Cand]) {
    list.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.item.name.cmp(&b.item.name))
            .then_with(|| a.key.cmp(&b.key))
    });
}

/// The candidates of one source, best first, at most `MAX_CANDIDATES`. Empty when the source has
/// nothing to offer (never learned anything, or the browser is not connected).
pub fn candidates(source: SmartSource, input: &Input) -> Vec<Cand> {
    let (now, hl, tod) = (input.now, hl(input), input.cfg.time_of_day);
    let front = input.ctx.front.as_deref();
    let mut out: Vec<Cand> = Vec::new();
    match source {
        SmartSource::FrequentApps | SmartSource::OpenApps => {
            let open_only = source == SmartSource::OpenApps;
            for id in input.habits.apps.keys() {
                if !allowed_app(id, input) {
                    continue;
                }
                let running = input.ctx.running.contains(id);
                if open_only && !running {
                    continue;
                }
                let mut score = input.habits.app_score(id, now, hl, tod);
                if let Some(f) = front {
                    score += NEXT_WEIGHT * input.habits.next_chance(f, id, now, hl);
                }
                if open_only {
                    score += 0.5; // open now: worth more than a habit alone
                }
                if score <= 0.05 {
                    continue;
                }
                out.push(Cand {
                    key: format!("app:{id}"),
                    item: app_item(id, input),
                    source,
                    score,
                });
            }
        }
        SmartSource::OpenPages => {
            let mut seen = HashSet::new();
            for (url, active) in &input.ctx.tabs {
                if *active {
                    continue;
                }
                let Some((key, _, label)) = page_of(url, input.cfg.keep_page_paths) else {
                    continue;
                };
                if input.cfg.ignores_site(label.as_str()) || !seen.insert(key.clone()) {
                    continue;
                }
                let score = input.habits.page_score(&key, now, hl, tod) + 0.3;
                out.push(Cand {
                    key: format!("page:{key}"),
                    // the address of the tab itself, so the slice goes to that tab
                    item: Item::run(
                        &short(&label),
                        &site_icon(&label, input, "ico:browser"),
                        Action::OpenUrl(url.trim().to_string()),
                    ),
                    source,
                    score,
                });
            }
        }
        SmartSource::FrequentPages => {
            let in_view = page_in_view(input);
            for (key, p) in &input.habits.pages {
                if in_view.as_deref() == Some(key.as_str()) || input.cfg.ignores_site(&p.label) {
                    continue;
                }
                let score = input.habits.page_score(key, now, hl, tod);
                if score <= 0.05 {
                    continue;
                }
                out.push(Cand {
                    key: format!("page:{key}"),
                    item: Item::run(
                        &short(&p.label),
                        &site_icon(&p.label, input, "ico:globe"),
                        Action::OpenUrl(p.url.clone()),
                    ),
                    source,
                    score,
                });
            }
        }
        SmartSource::Actions => {
            for (key, a) in &input.habits.actions {
                let score = input.habits.action_score(key, front, now, hl, tod);
                if score <= 0.0 {
                    continue;
                }
                let icon = if a.icon.trim().is_empty() {
                    "ico:lightning"
                } else {
                    a.icon.as_str()
                };
                out.push(Cand {
                    key: format!("act:{key}"),
                    item: Item::run(&short(&a.name), icon, a.action.clone()),
                    source,
                    score,
                });
            }
        }
        SmartSource::Flows => {
            for f in input.flows {
                let action = Action::Flow(f.id.clone());
                let Some(key) = action_key(&action) else {
                    continue;
                };
                // a flow never run is still on offer, behind the ones that were
                let score = input.habits.action_score(&key, front, now, hl, tod) + 0.01;
                // the icon of the slice the flow was run from, when there was one
                let icon = input
                    .habits
                    .actions
                    .get(&key)
                    .map(|a| a.icon.trim())
                    .filter(|i| !i.is_empty())
                    .unwrap_or(SmartSource::Flows.icon());
                out.push(Cand {
                    key: format!("act:{key}"),
                    item: Item::run(&short(&f.name), icon, action),
                    source,
                    score,
                });
            }
        }
    }
    sort_desc(&mut out);
    out.truncate(MAX_CANDIDATES);
    out
}

/// How many suggestions of each source are asked about ("is it open?") when the pie opens.
const OPEN_KEYS_PER_SOURCE: usize = 12;

/// The keys ([`Action::open_choice_key`]) of the slices the rings of this gesture can show that
/// open an app or a web page, each once: what the running cache has to be asked about so the two
/// badges (a new window, or the one that is open) appear on them like on the slices of an ordinary
/// pie. `bridge` is the browser extension's setting: without it a web link has no key.
pub fn open_keys(input: &Input, bridge: bool) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for source in SmartSource::ALL {
        for cand in candidates(source, input)
            .into_iter()
            .take(OPEN_KEYS_PER_SOURCE)
        {
            if let ItemKind::Run(action) = &cand.item.kind
                && let Some(key) = action.open_choice_key(bridge)
                && !keys.contains(&key)
            {
                keys.push(key);
            }
        }
    }
    keys
}

/// The ring made of `sources`, `count` slices at most, mixed as the module docs say. Empty when no
/// source has anything.
pub fn ring(sources: &[SmartSource], count: usize, input: &Input) -> Vec<Item> {
    let count = count.clamp(1, MAX_ITEMS);
    let mut mixed: Vec<(f32, Cand)> = Vec::new();
    for &source in sources {
        let list = candidates(source, input);
        let Some(top) = list.first().map(|c| c.score).filter(|s| *s > 0.0) else {
            continue;
        };
        for (rank, c) in list.into_iter().enumerate() {
            let adjusted = c.score / top * RANK_DECAY.powi(rank as i32) * weight(source);
            mixed.push((adjusted, c));
        }
    }
    mixed.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.item.name.cmp(&b.1.item.name))
            .then_with(|| a.1.key.cmp(&b.1.key))
    });
    let mut seen = HashSet::new();
    let mut items: Vec<Item> = mixed
        .into_iter()
        .filter(|(_, c)| seen.insert(c.key.clone()))
        .take(count)
        .map(|(_, c)| c.item)
        .collect();
    distinct_icons(&mut items);
    items
}

/// The two letters (or a letter and a number) that stand for `name` when its icon is shared: the
/// initials of its first two words (`Browser remote` is `BR`), else the first letters of its first
/// word (`Daily` is `DA`, then `DI`, `DL`...), else the first letter and a number. The first one
/// that is not in `used`.
fn initials(name: &str, used: &HashSet<String>) -> String {
    let words: Vec<Vec<char>> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().collect())
        .collect();
    let up = |c: char| c.to_uppercase().next().unwrap_or(c);
    let first = words.first().map_or('?', |w| up(w[0]));
    let mut tries: Vec<String> = Vec::new();
    if let [a, b, ..] = words.as_slice() {
        tries.push(format!("{}{}", up(a[0]), up(b[0])));
    }
    if let Some(w) = words.first() {
        if w.len() == 1 {
            tries.push(first.to_string());
        }
        tries.extend(w.iter().skip(1).map(|c| format!("{first}{}", up(*c))));
    }
    tries.extend((2..100).map(|n| format!("{first}{n}")));
    tries
        .into_iter()
        .find(|t| !used.contains(t))
        .unwrap_or_else(|| first.to_string())
}

/// Makes sure no two slices of a ring wear the same picture. Five flows that all have the wand, or
/// four sites that have no icon yet and all have the globe, cannot be told apart at a glance, and
/// the person is left reading names. Every slice whose icon is shared gets the letters of its name
/// instead (see `initials`, at most two characters, so they are drawn large); the ones with a
/// picture of their own are not touched. Deterministic: the same ring always gets the same letters.
pub fn distinct_icons(items: &mut [Item]) {
    let mut count: HashMap<String, usize> = HashMap::new();
    for it in items.iter() {
        *count.entry(it.icon.clone()).or_default() += 1;
    }
    let mut used: HashSet<String> = items
        .iter()
        .filter(|i| count[&i.icon] == 1)
        .map(|i| i.icon.clone())
        .collect();
    for it in items.iter_mut() {
        if count[&it.icon] < 2 {
            continue;
        }
        let mark = initials(&it.name, &used);
        used.insert(mark.clone());
        it.icon = mark;
    }
}

/// The ring of an intelligent slice.
pub fn slice(slice: &SmartSlice, input: &Input) -> Vec<Item> {
    if !input.cfg.enabled {
        return Vec::new();
    }
    ring(
        &input.cfg.sources_for(slice),
        usize::from(slice.count),
        input,
    )
}

/// A category of the grouped intelligent pie: a slice with a name and an icon that opens a ring of
/// the suggestions of the sources it gathers.
pub struct Group {
    pub name: &'static str,
    pub icon: &'static str,
    pub sources: &'static [SmartSource],
}

/// The categories of the grouped pie, in the order they are drawn. A person thinks "an app" or "a
/// site", not "a frequent page", so open and frequent are one category.
pub const GROUPS: [Group; 4] = [
    Group {
        name: "Apps",
        icon: "ico:app-window",
        sources: &[SmartSource::OpenApps, SmartSource::FrequentApps],
    },
    Group {
        name: "Sites",
        icon: "ico:globe",
        sources: &[SmartSource::OpenPages, SmartSource::FrequentPages],
    },
    Group {
        name: "Actions",
        icon: "ico:lightning",
        sources: &[SmartSource::Actions],
    },
    Group {
        name: "Flows",
        icon: "ico:magic-wand",
        sources: &[SmartSource::Flows],
    },
];

/// The slices of the grouped pie: one intelligent submenu per category that has something to
/// suggest (and a source switched on in Settings).
pub fn groups(input: &Input) -> Vec<Item> {
    GROUPS
        .iter()
        .filter_map(|g| {
            let sources: Vec<SmartSource> = g
                .sources
                .iter()
                .copied()
                .filter(|s| input.cfg.has(*s))
                .collect();
            if sources.iter().all(|s| candidates(*s, input).is_empty()) {
                return None;
            }
            Some(Item::smart(
                g.name,
                g.icon,
                SmartSlice {
                    sources,
                    count: MAX_ITEMS as u8,
                },
            ))
        })
        .collect()
}

/// The one slice the intelligent pie shows when it has nothing to suggest: a pie with no slice
/// cannot be drawn, and the person should be told why it is empty.
fn placeholder(input: &Input) -> Item {
    let text = if !input.cfg.enabled {
        "Intelligent is off"
    } else if input.habits.is_empty() {
        "Still learning"
    } else {
        "Nothing to suggest"
    };
    Item::run(text, "ico:sparkle", Action::Nothing)
}

/// The top ring of the intelligent pie.
pub fn pie(input: &Input) -> Vec<Item> {
    if !input.cfg.enabled {
        return vec![placeholder(input)];
    }
    let items = match input.cfg.layout {
        SmartLayout::Mixed => ring(&input.cfg.sources, usize::from(input.cfg.pie_slices), input),
        SmartLayout::Grouped => groups(input),
    };
    if items.is_empty() {
        vec![placeholder(input)]
    } else {
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ItemKind;
    use crate::keys::{Shortcut, key};

    const T0: u64 = 1_700_000_000;
    const HL: f64 = 30.0;

    fn cfg() -> SmartConfig {
        SmartConfig {
            enabled: true,
            time_of_day: false,
            ..SmartConfig::default()
        }
    }

    fn habits() -> Habits {
        let mut h = Habits::default();
        for (id, mins) in [("code", 300u32), ("mail", 60), ("music", 20), ("notes", 5)] {
            for i in 0..mins {
                h.see_app(id, 60.0, false, None, T0 + u64::from(i) * 60, HL);
            }
        }
        h
    }

    fn input<'a>(
        h: &'a Habits,
        ctx: &'a Context,
        cfg: &'a SmartConfig,
        flows: &'a [FlowRef],
    ) -> Input<'a> {
        Input {
            habits: h,
            ctx,
            cfg,
            flows,
            now: T0 + 400 * 60,
        }
    }

    fn names(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn frequent_apps_come_most_used_first_without_the_one_in_front() {
        let (h, c) = (habits(), cfg());
        let ctx = Context {
            front: Some("code".into()),
            ..Context::default()
        };
        let list = candidates(SmartSource::FrequentApps, &input(&h, &ctx, &c, &[]));
        let ids: Vec<&str> = list.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(ids, ["app:mail", "app:music", "app:notes"]);
    }

    #[test]
    fn open_apps_are_only_the_ones_that_run() {
        let (h, c) = (habits(), cfg());
        let ctx = Context {
            running: ["music".to_string(), "notes".to_string()].into(),
            ..Context::default()
        };
        let list = candidates(SmartSource::OpenApps, &input(&h, &ctx, &c, &[]));
        let ids: Vec<&str> = list.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(ids, ["app:music", "app:notes"]);
    }

    #[test]
    fn what_usually_comes_next_is_lifted() {
        let mut h = habits();
        // after notes you nearly always go to music
        for i in 0..8 {
            h.see_app("music", 5.0, true, Some("notes"), T0 + 30_000 + i, HL);
        }
        let c = cfg();
        let ctx = Context {
            front: Some("notes".into()),
            ..Context::default()
        };
        let list = candidates(SmartSource::FrequentApps, &input(&h, &ctx, &c, &[]));
        assert_eq!(
            list[0].key, "app:music",
            "ahead of mail, which has more time"
        );
    }

    #[test]
    fn an_ignored_app_and_cursorpie_itself_are_never_suggested() {
        let mut h = habits();
        h.see_app("io.github.cursorpie", 9999.0, false, None, T0, HL);
        let mut c = cfg();
        c.ignore_apps = vec!["mail".into()];
        let ctx = Context::default();
        let list = candidates(SmartSource::FrequentApps, &input(&h, &ctx, &c, &[]));
        let ids: Vec<&str> = list.iter().map(|c| c.key.as_str()).collect();
        assert!(!ids.contains(&"app:mail"));
        assert!(!ids.iter().any(|k| k.contains("cursorpie")));
        assert!(ids.contains(&"app:code"));
    }

    #[test]
    fn an_app_slice_opens_the_app_with_its_icon() {
        let h = habits();
        let c = cfg();
        let ctx = Context::default();
        let list = candidates(SmartSource::FrequentApps, &input(&h, &ctx, &c, &[]));
        let top = &list[0].item;
        assert_eq!(top.icon, "appcolor:code");
        match &top.kind {
            ItemKind::Run(Action::OpenApp(app)) => assert!(!app.is_empty()),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn open_pages_skip_the_page_in_view_and_repeats_of_a_site() {
        let h = habits();
        let c = cfg();
        let ctx = Context {
            tabs: vec![
                ("https://docs.rs/serde".into(), true),
                ("https://github.com/a/b".into(), false),
                ("https://github.com/c/d".into(), false),
                ("file:///tmp/x.html".into(), false),
            ],
            ..Context::default()
        };
        let list = candidates(SmartSource::OpenPages, &input(&h, &ctx, &c, &[]));
        assert_eq!(list.len(), 1, "one tab per site, never the one in view");
        assert_eq!(list[0].item.name, "github.com");
        assert!(
            matches!(&list[0].item.kind, ItemKind::Run(Action::OpenUrl(u)) if u == "https://github.com/a/b")
        );
    }

    #[test]
    fn frequent_pages_leave_out_the_one_in_view_and_the_ignored() {
        let mut h = habits();
        for (site, secs) in [
            ("github.com", 900.0),
            ("news.example", 600.0),
            ("bank.example", 5000.0),
        ] {
            h.see_page(site, &format!("https://{site}"), site, secs, true, T0, HL);
        }
        let mut c = cfg();
        c.ignore_sites = vec!["bank.example".into()];
        let ctx = Context {
            tabs: vec![("https://github.com/x/y".into(), true)],
            ..Context::default()
        };
        let list = candidates(SmartSource::FrequentPages, &input(&h, &ctx, &c, &[]));
        let n: Vec<&str> = list.iter().map(|c| c.item.name.as_str()).collect();
        assert_eq!(n, ["news.example"]);
    }

    #[test]
    fn actions_are_suggested_for_the_app_they_were_used_in() {
        let mut h = habits();
        let undo = Action::Key(Shortcut::new(key::F13, 0));
        let redo = Action::Key(Shortcut::new(key::F13, 1));
        for i in 0..5 {
            h.note_action(
                &undo,
                "Undo",
                "ico:arrow-u-up-left",
                Some("figma"),
                T0 + i,
                HL,
            );
        }
        for i in 0..2 {
            h.note_action(
                &redo,
                "Redo",
                "ico:arrow-u-up-right",
                Some("safari"),
                T0 + i,
                HL,
            );
        }
        let c = cfg();
        let ctx = Context {
            front: Some("figma".into()),
            ..Context::default()
        };
        let list = candidates(SmartSource::Actions, &input(&h, &ctx, &c, &[]));
        let n: Vec<&str> = list.iter().map(|c| c.item.name.as_str()).collect();
        assert_eq!(n, ["Undo"], "Redo was a habit of Safari, and only twice");
    }

    #[test]
    fn flows_never_run_are_still_offered_after_the_ones_that_were() {
        let mut h = habits();
        let flows = vec![
            FlowRef {
                id: "a".into(),
                name: "Rare".into(),
            },
            FlowRef {
                id: "b".into(),
                name: "Daily".into(),
            },
        ];
        for i in 0..6 {
            h.note_action(&Action::Flow("b".into()), "Daily", "x", None, T0 + i, HL);
        }
        let c = cfg();
        let ctx = Context::default();
        let list = candidates(SmartSource::Flows, &input(&h, &ctx, &c, &flows));
        let n: Vec<&str> = list.iter().map(|c| c.item.name.as_str()).collect();
        assert_eq!(n, ["Daily", "Rare"]);
    }

    #[test]
    fn a_flow_offered_twice_is_one_slice() {
        let mut h = habits();
        for i in 0..6 {
            h.note_action(&Action::Flow("b".into()), "Daily", "x", None, T0 + i, HL);
        }
        let flows = vec![FlowRef {
            id: "b".into(),
            name: "Daily".into(),
        }];
        let c = cfg();
        let ctx = Context::default();
        let items = ring(
            &[SmartSource::Actions, SmartSource::Flows],
            8,
            &input(&h, &ctx, &c, &flows),
        );
        assert_eq!(items.iter().filter(|i| i.name == "Daily").count(), 1);
    }

    #[test]
    fn a_mixed_ring_takes_from_every_source_not_only_the_first() {
        let mut h = habits();
        h.see_page(
            "github.com",
            "https://github.com",
            "github.com",
            3000.0,
            true,
            T0,
            HL,
        );
        let undo = Action::Key(Shortcut::new(key::F13, 0));
        for i in 0..8 {
            h.note_action(&undo, "Undo", "x", None, T0 + i, HL);
        }
        let c = cfg();
        let ctx = Context::default();
        let items = ring(
            &[
                SmartSource::FrequentApps,
                SmartSource::FrequentPages,
                SmartSource::Actions,
            ],
            4,
            &input(&h, &ctx, &c, &[]),
        );
        let n = names(&items);
        assert!(n.contains(&"github.com"), "{n:?}");
        assert!(n.contains(&"Undo"), "{n:?}");
        assert!(items.len() == 4);
    }

    #[test]
    fn a_ring_is_never_longer_than_asked_or_than_a_pie_can_hold() {
        let h = habits();
        let c = cfg();
        let ctx = Context::default();
        let i = input(&h, &ctx, &c, &[]);
        assert_eq!(ring(&[SmartSource::FrequentApps], 2, &i).len(), 2);
        assert!(ring(&[SmartSource::FrequentApps], 99, &i).len() <= MAX_ITEMS);
        assert!(ring(&[SmartSource::Flows], 5, &i).is_empty());
    }

    #[test]
    fn long_names_are_cut() {
        assert_eq!(short("Short"), "Short");
        let long = short("A very long name that does not fit");
        assert_eq!(long.chars().count(), NAME_CHARS);
        assert!(long.ends_with('\u{2026}'));
    }

    #[test]
    fn the_pie_says_why_it_is_empty() {
        let h = Habits::default();
        let ctx = Context::default();
        let off = SmartConfig::default();
        let p = pie(&input(&h, &ctx, &off, &[]));
        assert_eq!(names(&p), ["Intelligent is off"]);
        let on = cfg();
        let p = pie(&input(&h, &ctx, &on, &[]));
        assert_eq!(names(&p), ["Still learning"]);
        let learned = habits();
        let p = pie(&input(
            &learned,
            &ctx,
            &SmartConfig {
                sources: vec![SmartSource::OpenPages],
                ..cfg()
            },
            &[],
        ));
        assert_eq!(names(&p), ["Nothing to suggest"]);
    }

    #[test]
    fn the_grouped_pie_has_a_smart_slice_for_each_category_that_has_something() {
        let h = habits();
        let c = SmartConfig {
            layout: SmartLayout::Grouped,
            ..cfg()
        };
        let ctx = Context::default();
        let p = pie(&input(&h, &ctx, &c, &[]));
        // only apps are learned: no pages, no actions, no flows
        assert_eq!(names(&p), ["Apps"]);
        assert!(matches!(
            &p[0].kind,
            ItemKind::Smart(s) if s.sources == [SmartSource::OpenApps, SmartSource::FrequentApps]
        ));
    }

    #[test]
    fn the_grouped_pie_is_apps_then_sites_then_actions_then_flows() {
        let mut h = habits();
        h.see_page(
            "github.com",
            "https://github.com",
            "github.com",
            3000.0,
            true,
            T0,
            HL,
        );
        h.note_action(&Action::Flow("b".into()), "Daily", "x", None, T0, HL);
        let flows = vec![FlowRef {
            id: "b".into(),
            name: "Daily".into(),
        }];
        let undo = Action::Key(Shortcut::new(key::F13, 0));
        for i in 0..8 {
            h.note_action(&undo, "Undo", "x", None, T0 + i, HL);
        }
        let c = cfg();
        let ctx = Context::default();
        let p = pie(&input(&h, &ctx, &c, &flows));
        assert_eq!(names(&p), ["Apps", "Sites", "Actions", "Flows"]);
    }

    #[test]
    fn a_category_whose_sources_are_all_switched_off_is_left_out() {
        let h = habits();
        let c = SmartConfig {
            sources: vec![SmartSource::Flows],
            ..cfg()
        };
        let ctx = Context::default();
        let flows = vec![FlowRef {
            id: "a".into(),
            name: "Rare".into(),
        }];
        let p = pie(&input(&h, &ctx, &c, &flows));
        assert_eq!(names(&p), ["Flows"], "no Apps slice: the source is off");
    }

    #[test]
    fn what_can_be_open_is_asked_about_with_the_keys_of_an_ordinary_slice() {
        let mut h = habits();
        h.see_page(
            "github.com",
            "https://github.com",
            "github.com",
            3000.0,
            true,
            T0,
            HL,
        );
        let c = cfg();
        let ctx = Context {
            tabs: vec![("https://docs.rs/serde".into(), false)],
            ..Context::default()
        };
        let with = open_keys(&input(&h, &ctx, &c, &[]), true);
        assert!(
            with.contains(&"url:https://docs.rs/serde".to_string()),
            "{with:?}"
        );
        assert!(
            with.contains(&"url:https://github.com".to_string()),
            "{with:?}"
        );
        assert!(
            with.iter().any(|k| !k.starts_with("url:")),
            "apps too: {with:?}"
        );
        let mut seen = std::collections::HashSet::new();
        assert!(with.iter().all(|k| seen.insert(k)), "each once");
        let without = open_keys(&input(&h, &ctx, &c, &[]), false);
        assert!(
            without.iter().all(|k| !k.starts_with("url:")),
            "no extension, no links"
        );
    }

    fn run_item(name: &str, icon: &str) -> Item {
        Item::run(name, icon, Action::Nothing)
    }

    #[test]
    fn slices_that_share_an_icon_get_the_letters_of_their_names() {
        let mut items = vec![
            run_item("Browser remote", "ico:magic-wand"),
            run_item("Bookmarks", "ico:magic-wand"),
            run_item("Daily", "ico:magic-wand"),
            run_item("Mail", "appcolor:mail"),
        ];
        distinct_icons(&mut items);
        let icons: Vec<&str> = items.iter().map(|i| i.icon.as_str()).collect();
        assert_eq!(icons, ["BR", "BO", "DA", "appcolor:mail"]);
    }

    #[test]
    fn two_names_with_the_same_letters_still_differ() {
        let mut items = vec![
            run_item("Daily", "ico:globe"),
            run_item("Dairy", "ico:globe"),
            run_item("Dawn", "ico:globe"),
        ];
        distinct_icons(&mut items);
        let mut icons: Vec<&str> = items.iter().map(|i| i.icon.as_str()).collect();
        assert_eq!(icons, ["DA", "DI", "DW"]);
        icons.sort();
        icons.dedup();
        assert_eq!(icons.len(), 3);
    }

    #[test]
    fn a_picture_of_its_own_is_never_replaced() {
        let mut items = vec![
            run_item("One", "ico:play"),
            run_item("Two", "~/icons/two.png"),
        ];
        distinct_icons(&mut items);
        assert_eq!(items[0].icon, "ico:play");
        assert_eq!(items[1].icon, "~/icons/two.png");
    }

    #[test]
    fn a_name_without_letters_still_gets_a_mark_of_its_own() {
        let mut items = vec![run_item("???", "ico:globe"), run_item("!!!", "ico:globe")];
        distinct_icons(&mut items);
        assert_ne!(items[0].icon, items[1].icon);
    }

    #[test]
    fn flows_that_share_an_icon_are_told_apart_in_the_ring() {
        let mut h = habits();
        let flows: Vec<FlowRef> = ["Browser remote", "Daily", "Backup", "Deploy"]
            .iter()
            .enumerate()
            .map(|(i, n)| FlowRef {
                id: format!("f{i}"),
                name: (*n).into(),
            })
            .collect();
        for (i, f) in flows.iter().enumerate() {
            // every one was run from a slice with the same icon
            h.note_action(
                &Action::Flow(f.id.clone()),
                &f.name,
                "ico:play",
                None,
                T0 + i as u64,
                HL,
            );
        }
        let c = cfg();
        let ctx = Context::default();
        let items = ring(&[SmartSource::Flows], 8, &input(&h, &ctx, &c, &flows));
        let mut icons: Vec<String> = items.iter().map(|i| i.icon.clone()).collect();
        assert_eq!(icons.len(), 4);
        icons.sort();
        icons.dedup();
        assert_eq!(icons.len(), 4, "no two alike: {items:?}");
    }

    #[test]
    fn a_site_wears_its_own_icon_once_it_has_one() {
        let mut h = habits();
        h.see_page(
            "github.com",
            "https://github.com",
            "github.com",
            3000.0,
            true,
            T0,
            HL,
        );
        let mut ctx = Context::default();
        let c = cfg();
        let before = candidates(SmartSource::FrequentPages, &input(&h, &ctx, &c, &[]));
        assert_eq!(before[0].item.icon, "ico:globe", "nothing fetched yet");
        ctx.site_icons
            .insert("github.com".into(), "~/icons/site-github-com.png".into());
        let after = candidates(SmartSource::FrequentPages, &input(&h, &ctx, &c, &[]));
        assert_eq!(after[0].item.icon, "~/icons/site-github-com.png");
        let off = SmartConfig {
            site_icons: false,
            ..cfg()
        };
        let none = candidates(SmartSource::FrequentPages, &input(&h, &ctx, &off, &[]));
        assert_eq!(
            none[0].item.icon, "ico:globe",
            "the switch wins over a file"
        );
    }

    #[test]
    fn an_intelligent_slice_is_empty_while_the_feature_is_off() {
        let h = habits();
        let c = SmartConfig::default();
        let ctx = Context::default();
        assert!(slice(&SmartSlice::default(), &input(&h, &ctx, &c, &[])).is_empty());
    }

    #[test]
    fn an_intelligent_slice_uses_its_own_sources_and_count() {
        let mut h = habits();
        h.see_page(
            "github.com",
            "https://github.com",
            "github.com",
            3000.0,
            true,
            T0,
            HL,
        );
        let c = cfg();
        let ctx = Context::default();
        let only_pages = SmartSlice::of(SmartSource::FrequentPages);
        let items = slice(&only_pages, &input(&h, &ctx, &c, &[]));
        assert_eq!(names(&items), ["github.com"]);
        let two_apps = SmartSlice {
            sources: vec![SmartSource::FrequentApps],
            count: 2,
        };
        assert_eq!(slice(&two_apps, &input(&h, &ctx, &c, &[])).len(), 2);
    }

    #[test]
    fn the_same_input_always_gives_the_same_ring() {
        let h = habits();
        let c = cfg();
        let ctx = Context::default();
        let i = input(&h, &ctx, &c, &[]);
        let a = pie(&i);
        for _ in 0..20 {
            assert_eq!(
                pie(&i),
                a,
                "slice order must not change between hover and release"
            );
        }
    }
}
