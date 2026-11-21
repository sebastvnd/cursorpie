//! What CursorPie has learned about the person: apps, web pages and pie actions, as small counters
//! that fade with time. Pure data and arithmetic, no I/O and no threads, so it is unit tested.
//!
//! # What is kept
//!
//! | Habit | Key | Numbers |
//! |---|---|---|
//! | [`AppHabit`] | the app id, as in a profile | seconds in front, times switched to, seconds per hour of the day |
//! | [`PageHabit`] | the site (`github.com`), or the site and the first two path parts when the person allowed paths | seconds looked at, times seen, seconds per hour |
//! | [`ActionHabit`] | the action as JSON | times run, times run per app, per hour |
//! | `after` | app, then the app that came next | how often |
//!
//! Page titles, page contents, keystrokes and window titles are **never** read. A page is its address
//! and nothing else, a site by default.
//!
//! # How evidence fades
//!
//! Every number is stored *as of* the time in its `at` field and faded when it is read or written:
//! `value * 0.5^(age / half_life)`. The half-life is a quarter of
//! [`SmartConfig::keep_days`](crate::config::SmartConfig::keep_days), so a habit that stops is half
//! forgotten after a month with the default of 120 days, and removed when it is older than
//! `keep_days` or too weak to matter ([`MIN_WEIGHT`]).
//!
//! # Time of day
//!
//! The hour is counted from the system clock in UTC (`seconds since the epoch / 3600 % 24`). That is
//! not the person's hour, but it does not have to be: the same shift is applied when learning and
//! when asking, so "what I open at this time" works without knowing the time zone. A change of
//! zone or of daylight saving time is learned away by the fading.
//!
//! # Bounds
//!
//! At most [`MAX_APPS`] apps, [`MAX_PAGES`] pages and [`MAX_ACTIONS`] actions are kept, and
//! [`MAX_AFTER`] successors per app; when a table is full the weakest entry makes room.

use crate::config::Action;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Most apps kept.
pub const MAX_APPS: usize = 300;
/// Most pages (sites) kept.
pub const MAX_PAGES: usize = 300;
/// Most actions kept.
pub const MAX_ACTIONS: usize = 200;
/// Most "what came next" entries kept for one app.
pub const MAX_AFTER: usize = 12;
/// Most apps an action remembers having been run in.
const MAX_ACTION_APPS: usize = 12;
/// Longest key of a habit, in bytes. A longer action or address is not learned.
pub const MAX_KEY: usize = 400;
/// An entry whose faded weight is below this is forgotten.
pub const MIN_WEIGHT: f32 = 0.05;
/// A gap between two looks longer than this many times the sampling period is not counted as time
/// spent (the computer slept, or the app was stopped).
pub const MAX_GAP_PERIODS: u64 = 3;
/// The version of the file layout.
const GENERATION: u8 = 1;

const DAY: f64 = 86_400.0;

/// The hour bucket (0 to 23) of a time in seconds since the epoch. See the module docs.
pub fn hour_of(now: u64) -> usize {
    ((now / 3600) % 24) as usize
}

/// `value` as it is `now` when it was `at`, with the given half-life in days.
fn fade(value: f32, at: u64, now: u64, half_life_days: f64) -> f32 {
    if value <= 0.0 {
        return 0.0;
    }
    let age = now.saturating_sub(at) as f64 / DAY;
    (f64::from(value) * 0.5f64.powf(age / half_life_days.max(0.1))) as f32
}

fn fade_hours(h: &[f32; 24], at: u64, now: u64, hl: f64) -> [f32; 24] {
    let mut out = [0.0; 24];
    for (o, v) in out.iter_mut().zip(h) {
        *o = fade(*v, at, now, hl);
    }
    out
}

/// How typical `hour` is for what `hours` counts: 1.0 is "as often as any other time", above it is
/// more typical (up to 3.0), below it is less. A window of three hours is used, so a habit of
/// 9:00 also counts at 8:30 and at 9:59. `1.0` while there is too little to tell.
pub fn typical_at(hours: &[f32; 24], hour: usize) -> f32 {
    let total: f32 = hours.iter().sum();
    if total < 120.0 {
        // less than two minutes of evidence (or two uses): nothing to go by
        return 1.0;
    }
    let h = hour % 24;
    let near = hours[(h + 23) % 24] + hours[h] + hours[(h + 1) % 24];
    let share = near / total;
    (share / (3.0 / 24.0)).clamp(0.0, 3.0)
}

/// What is known about one app.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AppHabit {
    /// The name to show. Empty until it is known (see [`Habits::name_apps`]); then the id is made
    /// readable.
    #[serde(default)]
    pub label: String,
    /// What an "Open app" action stores for it. Empty: the id.
    #[serde(default)]
    pub launch: String,
    /// Seconds the app was in front (faded).
    pub secs: f32,
    /// Times it was switched to (faded).
    pub visits: f32,
    /// Seconds in front per hour of the day (faded).
    pub hours: [f32; 24],
    /// When the numbers were last updated, seconds since the epoch.
    pub at: u64,
}

/// What is known about one web page (site).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageHabit {
    /// The address that is opened for it: `https://github.com`, or with a path when allowed.
    pub url: String,
    /// The name to show: the site, without `www.`.
    pub label: String,
    /// Seconds the page was the one in view (faded).
    pub secs: f32,
    /// Times it came into view (faded).
    pub visits: f32,
    /// Seconds in view per hour of the day (faded).
    pub hours: [f32; 24],
    /// When the numbers were last updated, seconds since the epoch.
    pub at: u64,
}

/// What is known about one action that was run from the pie.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActionHabit {
    /// The name of the slice it was run from.
    pub name: String,
    /// The icon of that slice.
    pub icon: String,
    /// The action itself.
    pub action: Action,
    /// Times it was run (faded).
    pub uses: f32,
    /// Times it was run in each app (faded); the key is the app id, empty for "no app known".
    pub by_app: HashMap<String, f32>,
    /// Times it was run per hour of the day (faded).
    pub hours: [f32; 24],
    /// When the numbers were last updated, seconds since the epoch.
    pub at: u64,
}

/// Everything that was learned. Serialized as a whole to `smart-habits.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Habits {
    /// The version of the layout that wrote the file.
    #[serde(default)]
    generation: u8,
    /// When the first thing was learned, seconds since the epoch (0: nothing yet).
    #[serde(default)]
    pub since: u64,
    /// When the numbers were last tidied ([`Habits::digest`]).
    #[serde(default)]
    pub tidied: u64,
    pub apps: HashMap<String, AppHabit>,
    pub pages: HashMap<String, PageHabit>,
    pub actions: HashMap<String, ActionHabit>,
    /// For an app, how often each other app came to the front right after it (faded, with the time
    /// of the last update in the second field).
    #[serde(default)]
    pub after: HashMap<String, (HashMap<String, f32>, u64)>,
}

/// How many entries each table holds, for Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub apps: usize,
    pub pages: usize,
    pub actions: usize,
}

impl Habits {
    /// Whether nothing has been learned.
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty() && self.pages.is_empty() && self.actions.is_empty()
    }

    /// How much is known.
    pub fn counts(&self) -> Counts {
        Counts {
            apps: self.apps.len(),
            pages: self.pages.len(),
            actions: self.actions.len(),
        }
    }

    fn touch_since(&mut self, now: u64) {
        if self.since == 0 {
            self.since = now;
        }
    }

    // ------------------------------------------------------------ learning

    /// The app `id` was in front for `secs` seconds ending `now`. `came` is true when it just came
    /// to the front (counts a visit); `from` is the app that was in front before it, when known
    /// (counts a transition).
    pub fn see_app(
        &mut self,
        id: &str,
        secs: f32,
        came: bool,
        from: Option<&str>,
        now: u64,
        hl: f64,
    ) {
        if id.is_empty() || id.len() > MAX_KEY {
            return;
        }
        self.touch_since(now);
        let hour = hour_of(now);
        let a = self.apps.entry(id.to_string()).or_default();
        a.secs = fade(a.secs, a.at, now, hl) + secs;
        a.visits = fade(a.visits, a.at, now, hl);
        a.hours = fade_hours(&a.hours, a.at, now, hl);
        a.hours[hour] += secs;
        if came {
            a.visits += 1.0;
        }
        a.at = now;
        if let Some(prev) = from.filter(|p| !p.is_empty() && *p != id) {
            let (succ, at) = self
                .after
                .entry(prev.to_string())
                .or_insert_with(|| (HashMap::new(), now));
            for v in succ.values_mut() {
                *v = fade(*v, *at, now, hl);
            }
            *at = now;
            *succ.entry(id.to_string()).or_insert(0.0) += 1.0;
            trim_map(succ, MAX_AFTER);
        }
        if self.apps.len() > MAX_APPS {
            evict_weakest(&mut self.apps, |a| a.secs);
        }
    }

    /// The page with address `url` (key `key`, shown as `label`) was in view for `secs` seconds
    /// ending `now`. `came` is true when it just came into view.
    #[allow(clippy::too_many_arguments)]
    pub fn see_page(
        &mut self,
        key: &str,
        url: &str,
        label: &str,
        secs: f32,
        came: bool,
        now: u64,
        hl: f64,
    ) {
        if key.is_empty() || key.len() > MAX_KEY || url.len() > MAX_KEY {
            return;
        }
        self.touch_since(now);
        let hour = hour_of(now);
        let p = self.pages.entry(key.to_string()).or_default();
        if p.url.is_empty() {
            p.url = url.to_string();
            p.label = label.to_string();
        }
        p.secs = fade(p.secs, p.at, now, hl) + secs;
        p.visits = fade(p.visits, p.at, now, hl) + if came { 1.0 } else { 0.0 };
        p.hours = fade_hours(&p.hours, p.at, now, hl);
        p.hours[hour] += secs;
        p.at = now;
        if self.pages.len() > MAX_PAGES {
            evict_weakest(&mut self.pages, |p| p.secs + 30.0 * p.visits);
        }
    }

    /// `action`, run from the slice `name` / `icon` while `app` was the target, `now`.
    pub fn note_action(
        &mut self,
        action: &Action,
        name: &str,
        icon: &str,
        app: Option<&str>,
        now: u64,
        hl: f64,
    ) {
        let Some(key) = action_key(action) else {
            return;
        };
        self.touch_since(now);
        let hour = hour_of(now);
        let a = self.actions.entry(key).or_insert_with(|| ActionHabit {
            name: name.to_string(),
            icon: icon.to_string(),
            action: action.clone(),
            uses: 0.0,
            by_app: HashMap::new(),
            hours: [0.0; 24],
            at: now,
        });
        // the slice may have been renamed or given another icon since
        if !name.trim().is_empty() {
            a.name = name.to_string();
            a.icon = icon.to_string();
        }
        a.uses = fade(a.uses, a.at, now, hl) + 1.0;
        for v in a.by_app.values_mut() {
            *v = fade(*v, a.at, now, hl);
        }
        a.hours = fade_hours(&a.hours, a.at, now, hl);
        a.hours[hour] += 1.0;
        a.at = now;
        *a.by_app.entry(app.unwrap_or("").to_string()).or_insert(0.0) += 1.0;
        trim_map(&mut a.by_app, MAX_ACTION_APPS);
        if self.actions.len() > MAX_ACTIONS {
            evict_weakest(&mut self.actions, |a| a.uses);
        }
    }

    /// Fills the names and launch texts of apps that have none, from `known`: for an id the name and
    /// what an "Open app" action stores.
    pub fn name_apps(&mut self, known: &HashMap<String, (String, String)>) {
        for (id, a) in &mut self.apps {
            if let Some((name, launch)) = known.get(id) {
                if !name.is_empty() {
                    a.label = name.clone();
                }
                if !launch.is_empty() {
                    a.launch = launch.clone();
                }
            }
        }
    }

    // ------------------------------------------------------------ tidying

    /// The heavy part of learning: fades every number to `now`, forgets what is older than
    /// `keep_days` or too weak, and drops what the person asked to ignore. Done while the person is
    /// idle (or when forced), see [`collect`](super::collect).
    pub fn digest(&mut self, now: u64, keep_days: u32, keep: &dyn Fn(Kind, &str) -> bool) {
        self.generation = GENERATION;
        let hl = half_life(keep_days);
        let limit = u64::from(keep_days) * 86_400;
        self.apps.retain(|k, a| {
            let old = now.saturating_sub(a.at) > limit;
            a.secs = fade(a.secs, a.at, now, hl);
            a.visits = fade(a.visits, a.at, now, hl);
            a.hours = fade_hours(&a.hours, a.at, now, hl);
            a.at = now;
            !old && keep(Kind::App, k) && a.secs + 30.0 * a.visits >= MIN_WEIGHT
        });
        self.pages.retain(|k, p| {
            let old = now.saturating_sub(p.at) > limit;
            p.secs = fade(p.secs, p.at, now, hl);
            p.visits = fade(p.visits, p.at, now, hl);
            p.hours = fade_hours(&p.hours, p.at, now, hl);
            p.at = now;
            !old && keep(Kind::Page, k) && p.secs + 30.0 * p.visits >= MIN_WEIGHT
        });
        self.actions.retain(|k, a| {
            let old = now.saturating_sub(a.at) > limit;
            a.uses = fade(a.uses, a.at, now, hl);
            for v in a.by_app.values_mut() {
                *v = fade(*v, a.at, now, hl);
            }
            a.by_app
                .retain(|app, v| *v >= MIN_WEIGHT && (app.is_empty() || keep(Kind::App, app)));
            a.hours = fade_hours(&a.hours, a.at, now, hl);
            a.at = now;
            !old && keep(Kind::Action, k) && a.uses >= MIN_WEIGHT
        });
        // an app that is gone takes its transitions with it
        let apps: Vec<String> = self.apps.keys().cloned().collect();
        self.after.retain(|k, (succ, at)| {
            for v in succ.values_mut() {
                *v = fade(*v, *at, now, hl);
            }
            *at = now;
            succ.retain(|n, v| *v >= MIN_WEIGHT && apps.contains(n));
            apps.contains(k) && !succ.is_empty()
        });
        self.tidied = now;
    }

    // ------------------------------------------------------------ reading

    /// How much the app `id` matters now: a number that grows with the time spent and the visits,
    /// and is higher at the hours it is usually used at. `0.0` for an unknown app.
    pub fn app_score(&self, id: &str, now: u64, hl: f64, time_of_day: bool) -> f32 {
        let Some(a) = self.apps.get(id) else {
            return 0.0;
        };
        let secs = fade(a.secs, a.at, now, hl);
        let visits = fade(a.visits, a.at, now, hl);
        let hours = fade_hours(&a.hours, a.at, now, hl);
        let base = (1.0 + secs / 60.0 + 2.0 * visits).ln();
        let tod = if time_of_day {
            0.6 + 0.4 * typical_at(&hours, hour_of(now))
        } else {
            1.0
        };
        base * tod
    }

    /// The chance, from 0 to 1, that `next` is the app that comes to the front after `front`.
    pub fn next_chance(&self, front: &str, next: &str, now: u64, hl: f64) -> f32 {
        let Some((succ, at)) = self.after.get(front) else {
            return 0.0;
        };
        let total: f32 = succ.values().map(|v| fade(*v, *at, now, hl)).sum();
        if total < 2.0 {
            return 0.0; // too few to be a habit
        }
        let n = succ.get(next).map_or(0.0, |v| fade(*v, *at, now, hl));
        (n / total).clamp(0.0, 1.0)
    }

    /// How much the page `key` matters now, like [`Habits::app_score`].
    pub fn page_score(&self, key: &str, now: u64, hl: f64, time_of_day: bool) -> f32 {
        let Some(p) = self.pages.get(key) else {
            return 0.0;
        };
        let secs = fade(p.secs, p.at, now, hl);
        let visits = fade(p.visits, p.at, now, hl);
        let hours = fade_hours(&p.hours, p.at, now, hl);
        let base = (1.0 + secs / 60.0 + 2.0 * visits).ln();
        let tod = if time_of_day {
            0.6 + 0.4 * typical_at(&hours, hour_of(now))
        } else {
            1.0
        };
        base * tod
    }

    /// How much the action at `key` matters now for the app `app` (`None`: no app known). Uses in
    /// that app count fully, uses elsewhere a quarter. `0.0` when it was never used there and was
    /// used too rarely elsewhere to be a habit of its own.
    pub fn action_score(
        &self,
        key: &str,
        app: Option<&str>,
        now: u64,
        hl: f64,
        time_of_day: bool,
    ) -> f32 {
        let Some(a) = self.actions.get(key) else {
            return 0.0;
        };
        let uses = fade(a.uses, a.at, now, hl);
        let here = a
            .by_app
            .get(app.unwrap_or(""))
            .map_or(0.0, |v| fade(*v, a.at, now, hl));
        if here < 0.5 && uses < 4.0 {
            return 0.0;
        }
        let weighted = here + 0.25 * (uses - here).max(0.0);
        let hours = fade_hours(&a.hours, a.at, now, hl);
        let tod = if time_of_day {
            0.6 + 0.4 * typical_at(&hours, hour_of(now))
        } else {
            1.0
        };
        (1.0 + weighted).ln() * tod
    }
}

/// What a habit is about, for the question "may this be kept?".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    App,
    Page,
    Action,
}

/// The half-life of the evidence, in days, for `keep_days`.
pub fn half_life(keep_days: u32) -> f64 {
    (f64::from(keep_days) / 4.0).max(1.0)
}

/// The key an action is remembered by: its JSON. `None` for an action that is not worth learning (does
/// nothing, a wait, a sequence that is too long to be a key).
pub fn action_key(action: &Action) -> Option<String> {
    match action {
        Action::Nothing | Action::Wait(_) => return None,
        Action::OpenApp(s) | Action::OpenUrl(s) | Action::Command(s) | Action::Browser(s)
            if s.trim().is_empty() =>
        {
            return None;
        }
        _ => {}
    }
    let key = serde_json::to_string(action).ok()?;
    (key.len() <= MAX_KEY).then_some(key)
}

/// Keeps the `n` largest values of `map`.
fn trim_map(map: &mut HashMap<String, f32>, n: usize) {
    while map.len() > n {
        let weakest = map
            .iter()
            .min_by(|a, b| a.1.total_cmp(b.1))
            .map(|(k, _)| k.clone());
        match weakest {
            Some(k) => {
                map.remove(&k);
            }
            None => break,
        }
    }
}

/// Removes the entry of `map` with the smallest `weight`, the one just added excepted by its size.
fn evict_weakest<V>(map: &mut HashMap<String, V>, weight: impl Fn(&V) -> f32) {
    let weakest = map
        .iter()
        .min_by(|a, b| weight(a.1).total_cmp(&weight(b.1)))
        .map(|(k, _)| k.clone());
    if let Some(k) = weakest {
        map.remove(&k);
    }
}

// ------------------------------------------------------------------ addresses

/// The parts of a web address CursorPie keeps: the key it is learned under, the address that is
/// opened, and the name shown. `None` for anything that is not an `http(s)` address with a host.
///
/// Only the site is kept (`https://github.com`) unless `paths` is true, in which case the first two
/// parts of the path are too (`https://github.com/rust-lang/rust`). The query, the `#fragment`, a
/// user name or a password in the address are always dropped.
pub fn page_of(url: &str, paths: bool) -> Option<(String, String, String)> {
    let url = url.trim();
    let (scheme, rest) = match strip_prefix_ci(url, "https://") {
        Some(r) => ("https", r),
        None => ("http", strip_prefix_ci(url, "http://")?),
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    let host = host_port
        .split(':')
        .next()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') && host != "localhost" {
        return None;
    }
    let site = host.strip_prefix("www.").unwrap_or(&host).to_string();
    let mut keep = String::new();
    if paths && tail.starts_with('/') {
        let path = tail.split(['?', '#']).next().unwrap_or("");
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).take(2).collect();
        for p in parts {
            keep.push('/');
            keep.push_str(p);
        }
    }
    let key = format!("{site}{keep}");
    let open = format!("{scheme}://{host}{keep}");
    Some((key, open, site))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    s.get(..prefix.len())
        .filter(|h| h.eq_ignore_ascii_case(prefix))
        .map(|_| &s[prefix.len()..])
}

/// A readable name for an app id that has no name yet: the last meaningful part, in words.
/// `com.apple.Safari` is `Safari`, `code.exe` is `Code`, `org.gnome.Nautilus.desktop` is `Nautilus`.
pub fn readable_id(id: &str) -> String {
    let id = id.trim();
    let id = id
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(id)
        .trim_end_matches(".desktop")
        .trim_end_matches(".exe")
        .trim_end_matches(".app");
    let last = id.rsplit('.').next().unwrap_or(id);
    let mut out = String::new();
    let mut prev_lower = false;
    for c in last.chars() {
        if c == '-' || c == '_' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase();
        out.push(c);
    }
    let mut chars = out.chars();
    match chars.next() {
        Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Shortcut, key};

    const HL: f64 = 30.0;
    const DAYS: u64 = 86_400;
    const T0: u64 = 1_700_000_000;

    fn learn_app(h: &mut Habits, id: &str, minutes: u32, now: u64) {
        for i in 0..minutes {
            h.see_app(id, 60.0, false, None, now + u64::from(i) * 60, HL);
        }
    }

    #[test]
    fn an_app_used_more_ranks_higher() {
        let mut h = Habits::default();
        learn_app(&mut h, "code", 120, T0);
        learn_app(&mut h, "mail", 10, T0);
        let now = T0 + 7200;
        assert!(h.app_score("code", now, HL, false) > h.app_score("mail", now, HL, false));
        assert_eq!(h.app_score("never", now, HL, false), 0.0);
    }

    #[test]
    fn evidence_fades_with_the_half_life() {
        let mut h = Habits::default();
        h.see_app("old", 3600.0, false, None, T0, HL);
        let a = h.app_score("old", T0, HL, false);
        let b = h.app_score("old", T0 + 30 * DAYS, HL, false);
        let c = h.app_score("old", T0 + 300 * DAYS, HL, false);
        assert!(b < a && c < b, "{a} {b} {c}");
        assert!(c < 0.2, "ten half-lives later it is almost nothing: {c}");
    }

    #[test]
    fn a_recent_habit_beats_an_older_bigger_one_eventually() {
        let mut h = Habits::default();
        h.see_app("then", 36_000.0, false, None, T0, HL);
        let later = T0 + 120 * DAYS;
        h.see_app("now", 3_600.0, false, None, later, HL);
        assert!(h.app_score("now", later, HL, false) > h.app_score("then", later, HL, false));
    }

    #[test]
    fn the_hour_of_the_day_raises_what_is_usual_then() {
        let mut h = Habits::default();
        // an app used only at one hour of the day, for many days
        let hour0 = T0 - (T0 / 3600 % 24) * 3600; // 00:00 of that day, in the same buckets
        for d in 0..10 {
            h.see_app(
                "calendar",
                1800.0,
                false,
                None,
                hour0 + d * DAYS + 9 * 3600,
                HL,
            );
        }
        let nine = hour0 + 10 * DAYS + 9 * 3600;
        let three = hour0 + 10 * DAYS + 3 * 3600;
        let at_nine = h.app_score("calendar", nine, HL, true);
        let at_three = h.app_score("calendar", three, HL, true);
        assert!(at_nine > at_three * 1.5, "{at_nine} vs {at_three}");
        // with the time of day off both hours are the same
        let a = h.app_score("calendar", nine, HL, false);
        let b = h.app_score("calendar", three, HL, false);
        assert!((a - b).abs() < 0.05, "{a} vs {b}");
    }

    #[test]
    fn too_little_evidence_does_not_pretend_to_know_the_hour() {
        let mut hours = [0.0; 24];
        hours[9] = 30.0;
        assert_eq!(typical_at(&hours, 3), 1.0);
        assert_eq!(typical_at(&hours, 9), 1.0);
    }

    #[test]
    fn what_comes_after_an_app_is_learned() {
        let mut h = Habits::default();
        for i in 0..6 {
            h.see_app("figma", 60.0, true, Some("slack"), T0 + i * 100, HL);
            h.see_app("slack", 60.0, true, Some("figma"), T0 + i * 100 + 50, HL);
        }
        h.see_app("mail", 60.0, true, Some("figma"), T0 + 700, HL);
        let now = T0 + 800;
        let p = h.next_chance("figma", "slack", now, HL);
        assert!(p > 0.8, "slack follows figma almost always: {p}");
        assert!(h.next_chance("figma", "mail", now, HL) < p);
        assert_eq!(h.next_chance("unknown", "slack", now, HL), 0.0);
    }

    #[test]
    fn a_single_transition_is_not_a_habit() {
        let mut h = Habits::default();
        h.see_app("b", 10.0, true, Some("a"), T0, HL);
        assert_eq!(h.next_chance("a", "b", T0, HL), 0.0);
    }

    #[test]
    fn staying_in_the_same_app_is_not_a_transition() {
        let mut h = Habits::default();
        h.see_app("a", 10.0, true, Some("a"), T0, HL);
        assert!(h.after.is_empty());
    }

    fn key_action() -> Action {
        Action::Key(Shortcut::new(key::F13, 0))
    }

    #[test]
    fn an_action_is_suggested_where_it_was_used() {
        let mut h = Habits::default();
        for i in 0..6 {
            h.note_action(&key_action(), "Undo", "ico:undo", Some("figma"), T0 + i, HL);
        }
        let k = action_key(&key_action()).unwrap();
        let now = T0 + 10;
        let here = h.action_score(&k, Some("figma"), now, HL, false);
        let there = h.action_score(&k, Some("safari"), now, HL, false);
        assert!(here > 0.0);
        assert!(there > 0.0, "six uses elsewhere are a habit of their own");
        assert!(here > there, "but it counts more where it was used");
    }

    #[test]
    fn a_rare_action_is_not_suggested_in_another_app() {
        let mut h = Habits::default();
        h.note_action(&key_action(), "Undo", "x", Some("figma"), T0, HL);
        let k = action_key(&key_action()).unwrap();
        assert_eq!(h.action_score(&k, Some("safari"), T0, HL, false), 0.0);
        assert!(h.action_score(&k, Some("figma"), T0, HL, false) > 0.0);
    }

    #[test]
    fn actions_that_do_nothing_are_not_learned() {
        let mut h = Habits::default();
        for a in [
            Action::Nothing,
            Action::Wait(100),
            Action::OpenApp("  ".into()),
            Action::Command(String::new()),
        ] {
            h.note_action(&a, "x", "x", None, T0, HL);
        }
        assert!(h.actions.is_empty());
    }

    #[test]
    fn a_renamed_slice_keeps_its_habit_under_the_new_name() {
        let mut h = Habits::default();
        h.note_action(&key_action(), "Old", "a", None, T0, HL);
        h.note_action(&key_action(), "New", "b", None, T0 + 1, HL);
        assert_eq!(h.actions.len(), 1);
        let a = h.actions.values().next().unwrap();
        assert_eq!((a.name.as_str(), a.icon.as_str()), ("New", "b"));
        assert!(a.uses > 1.9);
    }

    #[test]
    fn tables_are_bounded_and_the_weakest_goes() {
        let mut h = Habits::default();
        h.see_app("keeper", 100_000.0, false, None, T0, HL);
        for i in 0..=MAX_APPS + 20 {
            h.see_app(&format!("app{i}"), 1.0, false, None, T0 + i as u64, HL);
        }
        assert_eq!(h.apps.len(), MAX_APPS);
        assert!(h.apps.contains_key("keeper"), "the strong one stays");
    }

    #[test]
    fn what_comes_after_is_bounded_per_app() {
        let mut h = Habits::default();
        for i in 0..40 {
            h.see_app(&format!("n{i}"), 1.0, true, Some("hub"), T0 + i, HL);
        }
        assert!(h.after["hub"].0.len() <= MAX_AFTER);
    }

    #[test]
    fn digest_forgets_the_old_and_the_ignored() {
        let mut h = Habits::default();
        h.see_app("old", 600.0, false, None, T0, HL);
        h.see_page(
            "old.example",
            "https://old.example",
            "old.example",
            600.0,
            true,
            T0,
            HL,
        );
        let later = T0 + 400 * DAYS;
        h.see_app("fresh", 600.0, false, None, later, HL);
        h.see_app("secret", 600.0, false, None, later, HL);
        h.see_page(
            "bank.example",
            "https://bank.example",
            "bank.example",
            600.0,
            true,
            later,
            HL,
        );
        h.digest(later, 120, &|kind, key| {
            !(kind == Kind::App && key == "secret")
                && !(kind == Kind::Page && key == "bank.example")
        });
        assert!(h.apps.contains_key("fresh"));
        assert!(!h.apps.contains_key("old"), "faded below the minimum");
        assert!(!h.apps.contains_key("secret"), "ignored");
        assert!(!h.pages.contains_key("old.example"), "older than keep_days");
        assert!(!h.pages.contains_key("bank.example"), "ignored");
        assert_eq!(h.tidied, later);
    }

    #[test]
    fn digest_drops_transitions_of_forgotten_apps() {
        let mut h = Habits::default();
        for i in 0..4 {
            h.see_app("b", 60.0, true, Some("a"), T0 + i, HL);
        }
        h.see_app("a", 60.0, false, None, T0, HL);
        h.digest(T0 + 10, 120, &|_, k| k != "b");
        assert!(h.after.is_empty());
    }

    #[test]
    fn names_are_filled_in_only_where_known() {
        let mut h = Habits::default();
        h.see_app("com.apple.Safari", 10.0, false, None, T0, HL);
        h.see_app("odd", 10.0, false, None, T0, HL);
        let mut known = HashMap::new();
        known.insert(
            "com.apple.Safari".to_string(),
            ("Safari".to_string(), "Safari".to_string()),
        );
        h.name_apps(&known);
        assert_eq!(h.apps["com.apple.Safari"].label, "Safari");
        assert_eq!(h.apps["odd"].label, "");
    }

    #[test]
    fn habits_round_trip_through_json() {
        let mut h = Habits::default();
        learn_app(&mut h, "code", 5, T0);
        h.see_page(
            "github.com",
            "https://github.com",
            "github.com",
            30.0,
            true,
            T0,
            HL,
        );
        h.note_action(&key_action(), "Undo", "i", Some("code"), T0, HL);
        h.see_app("slack", 5.0, true, Some("code"), T0 + 5, HL);
        let back: Habits = serde_json::from_str(&serde_json::to_string(&h).unwrap()).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn a_file_from_nowhere_loads_as_nothing() {
        let h: Habits = serde_json::from_str(r#"{"apps":{},"pages":{},"actions":{}}"#).unwrap();
        assert!(h.is_empty());
    }

    // ---------------------------------------------------------- addresses

    #[test]
    fn a_page_is_its_site_by_default() {
        let (key, open, label) = page_of(
            "https://www.GitHub.com/rust-lang/rust?tab=readme#top",
            false,
        )
        .unwrap();
        assert_eq!(key, "github.com");
        assert_eq!(open, "https://www.github.com");
        assert_eq!(label, "github.com");
    }

    #[test]
    fn a_path_is_kept_only_when_allowed_and_only_two_parts() {
        let (key, open, _) =
            page_of("https://github.com/rust-lang/rust/issues/1?x=1", true).unwrap();
        assert_eq!(key, "github.com/rust-lang/rust");
        assert_eq!(open, "https://github.com/rust-lang/rust");
    }

    #[test]
    fn credentials_ports_and_queries_never_reach_the_key() {
        let (key, open, _) =
            page_of("http://user:secret@example.com:8080/a?token=abc#f", false).unwrap();
        assert_eq!(key, "example.com");
        assert_eq!(open, "http://example.com");
        assert!(!open.contains("secret") && !open.contains("token"));
    }

    #[test]
    fn only_web_addresses_with_a_host_are_pages() {
        for bad in [
            "file:///etc/passwd",
            "chrome://settings",
            "about:blank",
            "mailto:a@b.c",
            "https://",
            "https://intranet/x",
            "javascript:alert(1)",
            "",
        ] {
            assert!(page_of(bad, false).is_none(), "{bad}");
        }
        assert!(page_of("http://localhost:3000/app", false).is_some());
        assert!(page_of("HTTPS://Example.COM", false).is_some());
    }

    #[test]
    fn app_ids_become_readable_names() {
        assert_eq!(readable_id("com.apple.Safari"), "Safari");
        assert_eq!(readable_id("code.exe"), "Code");
        assert_eq!(readable_id("org.gnome.Nautilus.desktop"), "Nautilus");
        assert_eq!(readable_id("visual-studio-code"), "Visual studio code");
        assert_eq!(readable_id("com.figma.DesktopApp"), "Desktop App");
        assert_eq!(readable_id(""), "");
    }

    #[test]
    fn a_long_action_is_not_a_key() {
        let a = Action::Command("x".repeat(MAX_KEY + 10));
        assert!(action_key(&a).is_none());
    }
}
