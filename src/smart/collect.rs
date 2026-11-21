//! Gathering what the pie learns from: a small worker thread that looks at the app in front and the
//! open web pages every few seconds, takes note of the actions run from the pie, and, while the
//! person is idle, tidies the habits and saves them.
//!
//! # Threads and locks
//!
//! Everything slow happens on the `cursorpie-smart` thread: asking the system for the app in front
//! and the running apps, listing the installed apps to name what was learned, reading and writing
//! the file. The tap thread and the UI thread only send a [`Cmd`] (a push into a channel, never a
//! wait) or take a snapshot of the habits (one short lock). The lock of the habits is never held
//! while the system is asked anything, and never while the config is locked, so it cannot be part of
//! a deadlock: the config is read (and copied) before it, never inside it.
//!
//! # When it learns
//!
//! * **Looking** is cheap (one call for the app in front, one for the pointer, a copy of the tab
//!   list) and happens every [`SmartConfig::sample_s`] seconds, only while
//!   [`SmartConfig::enabled`] is on and at least one kind of learning is allowed.
//! * **Tidying** (fading, forgetting, naming the apps by listing the installed ones, saving) is the
//!   heavy part. With [`SmartConfig::learn_when_idle`] it runs only after the pointer has been
//!   still for [`SmartConfig::idle_s`] seconds, and at most every [`IDLE_EVERY`]; otherwise a
//!   minute after something changed. *Learn now* in Settings runs it at once.
//! * **Saving** is done at the end of the tidying, and at most every [`SAVE_EVERY`] for what is
//!   learned in between, so a crash costs a few minutes of habits at the most.
//!
//! Nothing here sends anything anywhere. The habits are one JSON file next to `config.json`.

use super::habits::{Habits, Kind, MAX_GAP_PERIODS, half_life, page_of};
use super::suggest::Context;
use crate::config::{Action, SmartConfig, app_support_dir, cfg};
use crate::msg::{Msg, Notifier};
use crate::platform;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The least time between two tidyings while the person is idle.
pub const IDLE_EVERY: Duration = Duration::from_secs(10 * 60);
/// The least time between two tidyings when not waiting for idleness.
pub const BUSY_EVERY: Duration = Duration::from_secs(60);
/// The most time learned things stay only in memory.
pub const SAVE_EVERY: Duration = Duration::from_secs(5 * 60);
/// How long the names of the installed apps are trusted before the list is read again.
const NAMES_FOR: Duration = Duration::from_secs(24 * 3600);
/// How many of the strongest apps are asked about when the pie opens ("is it running?").
const RUNNING_ASKED: usize = 40;
/// How many sites are checked for an icon at each press (the downloader takes a few at a time).
const SITE_ICONS_ASKED: usize = 16;

/// Browsers are told by their id: a page counts as "in view" only while one of them is in front.
const BROWSERS: [&str; 16] = [
    "chrome",
    "chromium",
    "firefox",
    "safari",
    "edge",
    "msedge",
    "brave",
    "vivaldi",
    "opera",
    "arc",
    "zen",
    "librewolf",
    "waterfox",
    "orion",
    "duckduckgo",
    "thorium",
];

/// Whether the app `id` is a web browser, by its name.
pub fn looks_like_browser(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    BROWSERS.iter().any(|b| id.contains(b))
}

/// Seconds since the epoch.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ------------------------------------------------------------------ one look (pure)

/// What one look at the system found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observation {
    /// The app in front.
    pub front: Option<String>,
    /// The open web tabs, `(address, in view)`.
    pub tabs: Vec<(String, bool)>,
}

/// What the looks before this one found, so a look can tell a switch from staying.
#[derive(Debug, Default)]
pub struct Sampler {
    last_at: Option<u64>,
    /// The app that was in front at the last look, when it was one worth learning.
    last_front: Option<String>,
    /// The page that was in view at the last look.
    last_page: Option<String>,
}

impl Sampler {
    /// Takes note of one look at `now`. Returns whether anything was learned.
    ///
    /// The time since the last look is credited to what is in front now (a look every few seconds
    /// is a good estimate of a long stay, and misses what lasts less than that). A gap longer than
    /// [`MAX_GAP_PERIODS`] periods (the computer slept, the lid was shut) is not credited, and
    /// forgets the app and the page before it, so no switch is invented across it.
    pub fn step(&mut self, h: &mut Habits, obs: &Observation, c: &SmartConfig, now: u64) -> bool {
        let hl = half_life(c.keep_days);
        let gap = self.last_at.map(|t| now.saturating_sub(t));
        self.last_at = Some(now);
        let dt = match gap {
            Some(g) if g <= MAX_GAP_PERIODS * c.sample_s => g as f32,
            Some(_) => {
                self.last_front = None;
                self.last_page = None;
                0.0
            }
            None => 0.0,
        };
        let mut learned = false;

        let front = obs
            .front
            .as_deref()
            .filter(|id| !id.is_empty() && !super::is_self(id));
        // an app on the ignore list is not learned, and is not remembered as the app before the
        // next one either: no trace of it, not even in "what comes after"
        let learnable = front.filter(|id| !c.ignores_app(id));
        if c.learn_apps {
            match learnable {
                Some(id) => {
                    let switched = self.last_front.as_deref() != Some(id);
                    if switched || dt > 0.0 {
                        let from = if switched {
                            self.last_front.clone()
                        } else {
                            None
                        };
                        h.see_app(id, dt, switched, from.as_deref(), now, hl);
                        learned = true;
                    }
                    self.last_front = Some(id.to_string());
                }
                None if front.is_some() => self.last_front = None, // ignored
                None => {}
            }
        }

        if c.learn_pages {
            let in_view = learnable
                .filter(|id| looks_like_browser(id))
                .and_then(|_| obs.tabs.iter().find(|(_, active)| *active))
                .and_then(|(url, _)| page_of(url, c.keep_page_paths))
                .filter(|(_, _, label)| !c.ignores_site(label));
            match in_view {
                Some((key, url, label)) => {
                    let came = self.last_page.as_deref() != Some(key.as_str());
                    if came || dt > 0.0 {
                        h.see_page(&key, &url, &label, dt, came, now, hl);
                        learned = true;
                    }
                    self.last_page = Some(key);
                }
                None => self.last_page = None,
            }
        }
        learned
    }
}

/// Whether the habits are to be tidied now. Pure, so the schedule is tested.
///
/// With `learn_when_idle` the person must have been idle for `idle_s` and the last tidying at least
/// [`IDLE_EVERY`] ago; without it, [`BUSY_EVERY`] is enough. Either way there must be something new
/// (`dirty`), or a name still missing (`unnamed`) that the idle time can be used to find.
pub fn tidy_due(
    learn_when_idle: bool,
    idle_for: Duration,
    idle_s: u64,
    since_last: Duration,
    dirty: bool,
    unnamed: bool,
) -> bool {
    if !(dirty || unnamed) {
        return false;
    }
    if learn_when_idle {
        idle_for >= Duration::from_secs(idle_s) && since_last >= IDLE_EVERY
    } else {
        since_last >= BUSY_EVERY
    }
}

// ------------------------------------------------------------------ the file

/// Where the habits are kept.
pub fn file_path() -> PathBuf {
    app_support_dir().join("smart-habits.json")
}

/// Reads the habits. A missing or broken file is no habits: it is only a memory and must never cost
/// the person anything.
pub fn load_from(path: &Path) -> Habits {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// Writes the habits aside and moves them over, so a crash cannot leave half a file. Only the
/// person can read it: on Unix the file is `0600`.
pub fn save_to(path: &Path, h: &Habits) -> std::io::Result<()> {
    let json = serde_json::to_string(h).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path)
}

// ------------------------------------------------------------------ the live state

/// What the worker keeps between looks.
pub(super) struct Runtime {
    pub habits: Habits,
    pub ctx: Context,
    dirty: bool,
    last_tidy: Instant,
    last_save: Instant,
    names_at: Option<Instant>,
    sampler: Sampler,
    pointer: Option<(f64, f64)>,
    last_obs: Observation,
    active_at: Instant,
}

impl Runtime {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            habits: load_from(&file_path()),
            ctx: Context::default(),
            dirty: false,
            last_tidy: now,
            last_save: now,
            names_at: None,
            sampler: Sampler::default(),
            pointer: None,
            last_obs: Observation::default(),
            active_at: now,
        }
    }
}

static RUNTIME: OnceLock<Mutex<Runtime>> = OnceLock::new();

pub(super) fn runtime() -> MutexGuard<'static, Runtime> {
    RUNTIME
        .get_or_init(|| Mutex::new(Runtime::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// What the worker is asked to do.
pub enum Cmd {
    /// The trigger was pressed: find out which apps run and which pages are open, so the pie that
    /// may open has them.
    Press,
    /// A slice was released: an action was run from the pie.
    Action {
        action: Box<Action>,
        name: String,
        icon: String,
        app: Option<String>,
    },
    /// Tidy now, whatever the schedule says, and name the apps (Settings > Learn now).
    LearnNow,
    /// Forget everything and delete the file (Settings > Forget everything).
    Forget,
}

static TX: OnceLock<Sender<Cmd>> = OnceLock::new();

/// Sends a command to the worker. Never blocks; a command is lost when the worker is not running
/// (before [`init`], or if the system would not start it).
pub fn send(cmd: Cmd) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(cmd);
    }
}

/// Starts the worker. Harmless to call twice.
pub fn init(notifier: Notifier) {
    let (tx, rx) = mpsc::channel::<Cmd>();
    if TX.set(tx).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("cursorpie-smart".into())
        .spawn(move || worker(&rx, &notifier));
    if spawned.is_err() {
        diag::log!("smart", "spawn_failed");
    }
}

fn worker(rx: &mpsc::Receiver<Cmd>, notifier: &Notifier) {
    let mut last_look = Instant::now();
    loop {
        let c = cfg().smart.clone();
        let period = Duration::from_secs(if c.learning() { c.sample_s } else { 5 });
        let wait = period.saturating_sub(last_look.elapsed());
        match rx.recv_timeout(wait) {
            Ok(cmd) => {
                let c = cfg().smart.clone();
                // a panic in one job must not end the worker every later press depends on
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_cmd(cmd, &c, notifier);
                }));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if last_look.elapsed() >= period {
            last_look = Instant::now();
            let c = cfg().smart.clone();
            if c.learning() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| look(&c)));
            }
        }
    }
}

fn run_cmd(cmd: Cmd, c: &SmartConfig, notifier: &Notifier) {
    match cmd {
        Cmd::Press => {
            if c.enabled {
                refresh_context(c);
            }
        }
        Cmd::Action {
            action,
            name,
            icon,
            app,
        } => {
            if !(c.enabled && c.learn_actions) || app.as_deref().is_some_and(|a| c.ignores_app(a)) {
                return;
            }
            let mut rt = runtime();
            rt.habits.note_action(
                &action,
                &name,
                &icon,
                app.as_deref(),
                now_secs(),
                half_life(c.keep_days),
            );
            rt.dirty = true;
        }
        Cmd::LearnNow => {
            if c.enabled {
                look(c);
            }
            let names = installed_names();
            let counts = {
                let mut rt = runtime();
                tidy(&mut rt, c, Some(names));
                rt.habits.counts()
            };
            notifier.send(Msg::Notice(format!(
                "Learned so far: {} apps, {} pages, {} actions.",
                counts.apps, counts.pages, counts.actions
            )));
        }
        Cmd::Forget => {
            {
                let mut rt = runtime();
                rt.habits = Habits::default();
                rt.sampler = Sampler::default();
                rt.dirty = false;
            }
            let _ = std::fs::remove_file(file_path());
            super::siteicon::forget_all();
            notifier.send(Msg::Notice(
                "CursorPie forgot everything it learned.".into(),
            ));
        }
    }
}

/// The apps CursorPie knows by id: `(name, what an "Open app" action stores)`. Slow (it can run
/// programs); only the worker calls it.
fn installed_names() -> HashMap<String, (String, String)> {
    platform::installed::installed_apps()
        .into_iter()
        .map(|a| {
            let launch = platform::installed::action_text(&a);
            (a.id, (a.name, launch))
        })
        .collect()
}

/// Which apps run and which pages are open, for the pie that may open now.
fn refresh_context(c: &SmartConfig) {
    // the strongest apps, asked about in one go (a process list and a window list for all of them)
    let ids = {
        let rt = runtime();
        let now = now_secs();
        let hl = half_life(c.keep_days);
        let mut v: Vec<(&String, f32)> = rt
            .habits
            .apps
            .keys()
            .filter(|id| !c.ignores_app(id))
            .map(|id| (id, rt.habits.app_score(id, now, hl, false)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        v.into_iter()
            .take(RUNNING_ASKED)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
    };
    // the system is asked with no lock held
    let flags = if ids.is_empty() {
        Vec::new()
    } else {
        platform::running_among(&ids)
    };
    let tabs = crate::bridge::open_tabs();
    let mut rt = runtime();
    rt.ctx.running = ids
        .into_iter()
        .zip(flags)
        .filter_map(|(id, on)| on.then_some(id))
        .collect();
    rt.ctx.tabs = tabs;
    // the sites that may be suggested get their icon in the background (never waited for here)
    let hosts = if c.site_icons {
        super::siteicon::hosts(&rt.habits, &rt.ctx.tabs, c, SITE_ICONS_ASKED)
    } else {
        Vec::new()
    };
    drop(rt);
    super::siteicon::want(&hosts);
}

/// One look at the system: the app in front, the open pages, the pointer. Then the tidying, if it
/// is due.
fn look(c: &SmartConfig) {
    let front = platform::frontmost_bundle_id();
    let tabs = crate::bridge::open_tabs();
    let pointer = platform::cursor_pos();
    let obs = Observation { front, tabs };
    let now = now_secs();
    let mut rt = runtime();
    // a pointer that moved, or an app or page that changed, is somebody there
    if pointer != rt.pointer || obs != rt.last_obs {
        rt.active_at = Instant::now();
    }
    rt.pointer = pointer;
    let mut sampler = std::mem::take(&mut rt.sampler);
    let learned = sampler.step(&mut rt.habits, &obs, c, now);
    rt.sampler = sampler;
    rt.ctx.tabs.clone_from(&obs.tabs);
    rt.last_obs = obs;
    if learned {
        rt.dirty = true;
    }
    let unnamed = rt.names_at.is_none_or(|t| t.elapsed() > NAMES_FOR)
        && rt.habits.apps.values().any(|a| a.label.is_empty());
    let idle_for = rt.active_at.elapsed();
    let due = tidy_due(
        c.learn_when_idle,
        idle_for,
        c.idle_s,
        rt.last_tidy.elapsed(),
        rt.dirty,
        unnamed,
    );
    if due {
        // listing the installed apps can take seconds: not with the habits locked
        drop(rt);
        let names = unnamed.then(installed_names);
        let mut rt = runtime();
        tidy(&mut rt, c, names);
    } else if rt.dirty && rt.last_save.elapsed() >= SAVE_EVERY {
        save(&mut rt);
    }
}

/// Fades and prunes the habits, names the apps (when `names` is given), and saves.
fn tidy(rt: &mut Runtime, c: &SmartConfig, names: Option<HashMap<String, (String, String)>>) {
    if let Some(names) = names {
        rt.habits.name_apps(&names);
        rt.names_at = Some(Instant::now());
    }
    let keep = |kind: Kind, key: &str| match kind {
        Kind::App => !c.ignores_app(key),
        Kind::Page => !c.ignores_site(key.split('/').next().unwrap_or(key)),
        Kind::Action => true,
    };
    rt.habits.digest(now_secs(), c.keep_days, &keep);
    rt.last_tidy = Instant::now();
    save(rt);
}

fn save(rt: &mut Runtime) {
    match save_to(&file_path(), &rt.habits) {
        Ok(()) => {
            rt.dirty = false;
            rt.last_save = Instant::now();
            diag::log!("smart", "saved", apps = rt.habits.apps.len());
        }
        Err(_) => diag::log!("smart", "save_failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_700_000_000;

    fn cfg_on() -> SmartConfig {
        SmartConfig {
            enabled: true,
            sample_s: 10,
            ..SmartConfig::default()
        }
    }

    fn obs(front: &str) -> Observation {
        Observation {
            front: Some(front.into()),
            tabs: Vec::new(),
        }
    }

    #[test]
    fn staying_in_an_app_adds_its_time() {
        let (mut h, mut s, c) = (Habits::default(), Sampler::default(), cfg_on());
        for i in 0..7 {
            s.step(&mut h, &obs("code"), &c, T0 + i * 10);
        }
        let secs = h.apps["code"].secs;
        assert!((secs - 60.0).abs() < 1.0, "six gaps of ten seconds: {secs}");
        assert_eq!(
            h.apps["code"].visits.round() as u32,
            1,
            "one visit, not seven"
        );
    }

    #[test]
    fn a_switch_is_remembered_as_what_came_after() {
        let (mut h, mut s, c) = (Habits::default(), Sampler::default(), cfg_on());
        for round in 0..4u64 {
            s.step(&mut h, &obs("figma"), &c, T0 + round * 40);
            s.step(&mut h, &obs("slack"), &c, T0 + round * 40 + 20);
        }
        assert!(h.after["figma"].0.contains_key("slack"));
        assert!(h.after["slack"].0.contains_key("figma"));
    }

    #[test]
    fn a_long_gap_is_not_credited_and_invents_no_switch() {
        let (mut h, mut s, c) = (Habits::default(), Sampler::default(), cfg_on());
        s.step(&mut h, &obs("a"), &c, T0);
        s.step(&mut h, &obs("a"), &c, T0 + 10);
        // the computer slept for an hour and woke up in another app
        s.step(&mut h, &obs("b"), &c, T0 + 3600);
        assert!((h.apps["a"].secs - 10.0).abs() < 1.0);
        assert!(h.apps["b"].secs < 1.0, "the hour of sleep is not b's");
        assert!(h.after.is_empty(), "and a did not 'lead to' b");
    }

    #[test]
    fn an_ignored_app_leaves_no_trace_not_even_as_what_came_before() {
        let (mut h, mut s) = (Habits::default(), Sampler::default());
        let mut c = cfg_on();
        c.ignore_apps = vec!["bank".into()];
        for round in 0..4u64 {
            s.step(&mut h, &obs("bank"), &c, T0 + round * 30);
            s.step(&mut h, &obs("notes"), &c, T0 + round * 30 + 15);
        }
        assert!(!h.apps.contains_key("bank"));
        assert!(h.after.is_empty(), "{:?}", h.after);
        assert!(h.apps.contains_key("notes"));
    }

    #[test]
    fn cursorpie_itself_is_not_learned() {
        let (mut h, mut s, c) = (Habits::default(), Sampler::default(), cfg_on());
        s.step(&mut h, &obs("io.github.cursorpie"), &c, T0);
        s.step(&mut h, &obs("io.github.cursorpie"), &c, T0 + 10);
        assert!(h.apps.is_empty());
    }

    #[test]
    fn apps_are_not_learned_when_that_is_off() {
        let (mut h, mut s) = (Habits::default(), Sampler::default());
        let c = SmartConfig {
            learn_apps: false,
            ..cfg_on()
        };
        s.step(&mut h, &obs("code"), &c, T0);
        s.step(&mut h, &obs("code"), &c, T0 + 10);
        assert!(h.apps.is_empty());
    }

    fn browsing(url: &str) -> Observation {
        Observation {
            front: Some("org.mozilla.firefox".into()),
            tabs: vec![(url.into(), true), ("https://other.example".into(), false)],
        }
    }

    #[test]
    fn the_page_in_view_is_learned_as_its_site() {
        let (mut h, mut s, c) = (Habits::default(), Sampler::default(), cfg_on());
        for i in 0..4 {
            s.step(
                &mut h,
                &browsing("https://github.com/a/b?x=1"),
                &c,
                T0 + i * 10,
            );
        }
        assert!(h.pages.contains_key("github.com"));
        assert!(
            !h.pages.contains_key("other.example"),
            "only the one in view"
        );
        assert!(h.pages["github.com"].secs > 25.0);
        assert!(
            h.pages["github.com"].url == "https://github.com",
            "no path, no query"
        );
    }

    #[test]
    fn a_page_is_not_in_view_while_another_app_is_in_front() {
        let (mut h, mut s, c) = (Habits::default(), Sampler::default(), cfg_on());
        let o = Observation {
            front: Some("figma".into()),
            tabs: vec![("https://github.com".into(), true)],
        };
        for i in 0..4 {
            s.step(&mut h, &o, &c, T0 + i * 10);
        }
        assert!(h.pages.is_empty());
    }

    #[test]
    fn an_ignored_site_is_not_learned() {
        let (mut h, mut s) = (Habits::default(), Sampler::default());
        let mut c = cfg_on();
        c.ignore_sites = vec!["bank.example".into()];
        for i in 0..4 {
            s.step(
                &mut h,
                &browsing("https://secure.bank.example/acct/1"),
                &c,
                T0 + i * 10,
            );
        }
        assert!(h.pages.is_empty());
    }

    #[test]
    fn paths_are_kept_only_when_allowed() {
        let (mut h, mut s) = (Habits::default(), Sampler::default());
        let c = SmartConfig {
            keep_page_paths: true,
            ..cfg_on()
        };
        for i in 0..3 {
            s.step(
                &mut h,
                &browsing("https://github.com/rust-lang/rust/pulls"),
                &c,
                T0 + i * 10,
            );
        }
        assert!(h.pages.contains_key("github.com/rust-lang/rust"));
    }

    #[test]
    fn browsers_are_told_by_name() {
        for b in [
            "org.mozilla.firefox",
            "com.google.Chrome",
            "msedge.exe",
            "com.apple.Safari",
            "brave-browser.desktop",
        ] {
            assert!(looks_like_browser(b), "{b}");
        }
        for n in ["com.microsoft.VSCode", "slack", "org.gnome.Nautilus"] {
            assert!(!looks_like_browser(n), "{n}");
        }
    }

    #[test]
    fn tidying_waits_for_idleness_when_asked() {
        let s = Duration::from_secs;
        // idle long enough, last tidy long ago, something new: yes
        assert!(tidy_due(true, s(120), 60, s(3600), true, false));
        // the person is at the computer: no
        assert!(!tidy_due(true, s(5), 60, s(3600), true, false));
        // tidied a minute ago: no
        assert!(!tidy_due(true, s(120), 60, s(60), true, false));
        // nothing new and nothing to name: never
        assert!(!tidy_due(true, s(999), 60, s(9999), false, false));
        // a missing name is a reason on its own
        assert!(tidy_due(true, s(120), 60, s(3600), false, true));
    }

    #[test]
    fn without_waiting_for_idleness_it_is_a_minute_after_a_change() {
        let s = Duration::from_secs;
        assert!(tidy_due(false, s(0), 60, s(61), true, false));
        assert!(!tidy_due(false, s(0), 60, s(30), true, false));
    }

    #[test]
    fn the_file_round_trips_and_a_broken_one_is_no_habits() {
        let dir = std::env::temp_dir().join(format!("cursorpie-smart-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("smart-habits.json");
        let mut h = Habits::default();
        h.see_app("code", 60.0, false, None, T0, 30.0);
        save_to(&path, &h).unwrap();
        assert_eq!(load_from(&path), h);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "only the person reads it");
        }
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_from(&path).is_empty());
        assert!(load_from(&dir.join("missing.json")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
