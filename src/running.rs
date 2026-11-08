//! "Open app" and "Open web page" slices of something that is already open: a new window, or the
//! one that is open.
//!
//! While the pie is open and the pointer is over an *Open app* slice whose app is already running,
//! two round badges appear just **outside the rim, in front of that slice**, side by side: a green
//! circle with a **+** (open a *new window*) and a yellow circle with a small square in it (go to
//! the app that is *already open*, which is what the slice always did). The slice keeps its icon and
//! name. The green badge is on the side where the slice starts (counter-clockwise), the yellow one
//! on the side where it ends; the pointer chooses by which half of the slice it is in, so moving
//! toward a badge picks it. An app that does not run is simply opened, with no badges, and neither
//! is an app that cannot open another window (a music player, a chat client, a calculator): there
//! the green badge could only fail, so the slice just brings the app forward. Which apps those are
//! is decided the same way on every OS by [`windowing`](crate::windowing).
//! The setting is [`Config::open_app_choice`](crate::config::Config::open_app_choice)
//! (Settings > Behavior).
//!
//! # Web pages
//!
//! An "Open URL" slice whose text is an `http://` or `https://` link works the same way, **but only
//! with the browser extension** (Settings > Behavior > Browser extension, see
//! [`bridge`](crate::bridge)); a file, a `mailto:` link or anything else is opened as it always was.
//! A browser does not tell other programs which pages it has open, and guessing opens the same page
//! twice, so without the extension a web link is simply opened, with no badges. With it, "this page
//! is open" means a tab of a connected browser has it. The green badge opens the link in a new
//! browser window; the yellow one goes to that tab. In the cache a link is told from an app by its
//! key (`url:<link>`, see [`OpenTarget::key`](crate::config::OpenTarget::key)); the keys of links are
//! only asked about while the extension is on ([`open_apps_of`]).
//!
//! # Not asking the system on every hover
//!
//! Finding out whether an app runs is a question for the OS (a list of processes, a list of
//! windows), and it must never be asked on the event-tap thread or once per hover. So:
//!
//! 1. **Only when it can matter.** [`warm`] runs at the press of the trigger. It collects the apps of
//!    the "Open app" slices of the pies that may open ([`open_apps_of`]).
//!    No such slice, or the setting off: nothing else happens, the press costs one walk of a few
//!    slices.
//! 2. **Once per press, off the tap thread.** The apps are sent to the `cursorpie-running` worker,
//!    which asks the platform *once* for all of them ([`platform::running_among`]: one process list,
//!    one window list, however many slices there are) and stores the answers in a small cache.
//!    The press is not delayed: the answer is ready while the hold timer (`pie_hold_ms`) runs.
//! 3. **Hovering is a hash lookup.** [`is_running`] reads the cache (`try_lock`, never waits). If the
//!    answer is not there yet, it says `None` and the slice behaves as it always did.
//! 4. **Not again and again.** A fresh answer for the same apps (younger than [`TTL`]) is reused, so a
//!    burst of presses asks the OS once. An app CursorPie started itself is remembered as running
//!    ([`note_launched`]).
//!
//! A wrong answer is harmless by design: "running" for an app that just quit offers two ways to open
//! it, "not running" for one that just started opens it the old way.
//!
//! # Lock order
//!
//! The cache has a lock of its own. It is taken after the config lock (`eval` asks while it holds
//! the config) and never while waiting for anything: the worker holds it only to store an answer.

use crate::config::{URL_KEY_PREFIX, cfg, open_apps_of};
use crate::geometry::{Point, pie_outer, slice_start, window_size};
use crate::msg::Notifier;
use crate::platform;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock, TryLockError, mpsc};
use std::time::{Duration, Instant};

/// How long an answer of the platform is reused by the next press.
pub const TTL: Duration = Duration::from_secs(2);

/// What the person picked on an "Open app" slice whose app already runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// The yellow square: the app that is open (what the slice did before the choice existed).
    Existing,
    /// The green circle with a +: a new window of the app.
    NewWindow,
}

impl Choice {
    fn to_u8(c: Option<Choice>) -> u8 {
        match c {
            None => 0,
            Some(Choice::Existing) => 1,
            Some(Choice::NewWindow) => 2,
        }
    }

    fn from_u8(v: u8) -> Option<Choice> {
        match v {
            1 => Some(Choice::Existing),
            2 => Some(Choice::NewWindow),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ geometry (pure)

/// How far the badges are kept from the rim, in points. The hovered slice grows 5 points past the
/// rim, so this leaves a clear gap beyond it.
const BADGE_GAP: f32 = 14.0;
/// Space between the two badges, in points.
const BADGES_APART: f32 = 7.0;

/// Width (and height) of a badge, in points, for a pie of size `win`.
pub fn badge_size(win: f32) -> f32 {
    (win * 0.11).clamp(18.0, 28.0)
}

/// Where the badge of `choice` is, relative to the center of the pie, for slice `index` of `total`
/// in a pie of size `win`: just outside the rim, in front of the middle of the slice, the two badges
/// side by side with a small gap (green on the side where the slice starts, yellow on the side
/// where it ends). The pie window has [`BADGE_ROOM`](crate::geometry::BADGE_ROOM) around the ring
/// for them; the position is still kept inside the window as a precaution.
pub fn badge_offset(win: f32, index: usize, total: usize, choice: Choice) -> (f32, f32) {
    let mid = slice_start(index, total) + std::f32::consts::TAU / total.max(1) as f32 / 2.0;
    let half = badge_size(win) / 2.0;
    let radius = pie_outer(win) + BADGE_GAP + half;
    // along the rim: toward where the slice starts (-) or ends (+)
    let along = (half + BADGES_APART / 2.0)
        * match choice {
            Choice::NewWindow => -1.0,
            Choice::Existing => 1.0,
        };
    let (dir, tangent) = ((mid.cos(), mid.sin()), (-mid.sin(), mid.cos()));
    let room = window_size(win) / 2.0 - half - 1.0;
    (
        (dir.0 * radius + tangent.0 * along).clamp(-room, room),
        (dir.1 * radius + tangent.1 * along).clamp(-room, room),
    )
}

/// Which badge a pointer at `p` is on, for slice `index` of `total` around `center`: the half of the
/// slice that is nearer to the corner of that badge. The distance from the center does not matter.
pub fn choice_in_slice(center: Point, p: Point, index: usize, total: usize) -> Choice {
    let angle = ((p.y - center.y) as f32).atan2((p.x - center.x) as f32);
    let mid = slice_start(index, total) + std::f32::consts::TAU / total.max(1) as f32 / 2.0;
    // signed angle from the middle of the slice to the pointer, in (-PI, PI]
    let d = (angle - mid + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    if d < 0.0 {
        Choice::NewWindow
    } else {
        Choice::Existing
    }
}

/// What a pointer at `p` chooses over slice `index` of `total`, which opens `app`: `None` when the
/// setting is off, the slice opens no app, the app does not run (as far as is known) or it cannot
/// open another window ([`offers_choice`]).
pub fn choose(
    enabled: bool,
    app: Option<&str>,
    center: Point,
    p: Point,
    index: usize,
    total: usize,
) -> Option<Choice> {
    if !enabled {
        return None;
    }
    let app = app?;
    offers_choice(app).then(|| choice_in_slice(center, p, index, total))
}

// ------------------------------------------------------------------ the choice on show

/// The choice the tap last told the UI about (see [`shown`]).
static SHOWN: AtomicU8 = AtomicU8::new(0);

/// The choice the pointer is on right now, as the tap thread last saw it. The tap sets it when it
/// changes and wakes the UI (with `Msg::Hover`), which reads it while it draws the badges. `None`
/// while the pointer is on no choice.
pub fn shown() -> Option<Choice> {
    Choice::from_u8(SHOWN.load(Ordering::Relaxed))
}

/// Records the choice on show. Only the tap thread (and the UI, when the pie closes) calls this.
pub fn set_shown(c: Option<Choice>) {
    SHOWN.store(Choice::to_u8(c), Ordering::Relaxed);
}

// ------------------------------------------------------------------ the cache (pure)

/// What the platform said about the apps of the last press.
#[derive(Default)]
struct Cache {
    /// Counts the requests, so an answer to an old one is thrown away.
    generation: u64,
    /// Whether `running` holds the answer to the latest request.
    ready: bool,
    /// When the latest answer arrived.
    at: Option<Instant>,
    /// App (as typed in the slice, trimmed) to whether it runs.
    running: HashMap<String, bool>,
    /// Apps of the latest request that cannot open another window (see [`crate::windowing`]).
    single: HashSet<String>,
}

impl Cache {
    /// A press that needs `apps`. Returns the generation of the request to make, or `None` when
    /// nothing has to be asked: no apps, or a fresh answer that covers all of them.
    fn begin(&mut self, apps: &[String], now: Instant) -> Option<u64> {
        if apps.is_empty() {
            return None;
        }
        let fresh = self.ready
            && self.at.is_some_and(|t| now.duration_since(t) < TTL)
            && apps.iter().all(|a| self.running.contains_key(a));
        if fresh {
            return None;
        }
        // from here until the answer arrives nothing is known: an old answer must not be used
        self.ready = false;
        self.generation += 1;
        Some(self.generation)
    }

    /// Makes the latest answer count as old, so the next press asks again (see [`invalidate`]).
    fn expire(&mut self) {
        self.at = None;
    }

    /// The platform answered request `generation` about `apps` (`flags[i]` is for `apps[i]`).
    /// False, and nothing stored, when a newer request has been made since.
    fn finish(&mut self, generation: u64, apps: &[String], flags: &[bool], now: Instant) -> bool {
        if generation != self.generation {
            return false;
        }
        self.running = apps
            .iter()
            .enumerate()
            .map(|(i, a)| (a.clone(), flags.get(i).copied().unwrap_or(false)))
            .collect();
        self.ready = true;
        self.at = Some(now);
        true
    }

    /// Whether `app` runs, if the latest request has been answered and covered it.
    fn get(&self, app: &str) -> Option<bool> {
        if !self.ready {
            return None;
        }
        self.running.get(app.trim()).copied()
    }

    /// Whether the two badges are offered for `app`: it runs and it can open another window.
    fn offers_choice(&self, app: &str) -> bool {
        self.get(app) == Some(true) && !self.single.contains(app.trim())
    }

    /// Remembers that `app` has been started by CursorPie.
    fn mark_running(&mut self, app: &str) {
        if self.ready {
            self.running.insert(app.trim().to_string(), true);
        }
    }
}

// ------------------------------------------------------------------ the worker and its entry points

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// One question for the worker.
struct Request {
    generation: u64,
    /// Keys of the apps and web links asked about (see [`OpenTarget::key`](crate::config::OpenTarget::key)).
    apps: Vec<String>,
}

/// Keys that kept the position they had in the list they came from.
type Indexed = Vec<(usize, String)>;

/// Splits `keys` into the apps and the web links they stand for (`url:` removed), each with the
/// position it had in `keys`, so the answers can be put back in order.
fn split_keys(keys: &[String]) -> (Indexed, Indexed) {
    let (mut apps, mut urls) = (Vec::new(), Vec::new());
    for (i, key) in keys.iter().enumerate() {
        match key.strip_prefix(URL_KEY_PREFIX) {
            Some(url) => urls.push((i, url.to_string())),
            None => apps.push((i, key.clone())),
        }
    }
    (apps, urls)
}

/// The apps among `keys` that run and cannot open another window (web links are left out: a
/// browser can). May read a desktop entry, so like [`ask_platform`] it belongs on the worker.
fn single_window_among(keys: &[String], flags: &[bool]) -> HashSet<String> {
    keys.iter()
        .zip(flags)
        .filter(|(key, running)| **running && !key.starts_with(URL_KEY_PREFIX))
        .filter(|(key, _)| crate::windowing::single_window(key))
        .map(|(key, _)| key.trim().to_string())
        .collect()
}

/// Asks the platform about each of `keys`: whether the app runs, or whether the web page is open.
/// `flags[i]` is for `keys[i]`. One look at the system serves all the apps, and the web pages are
/// answered from what CursorPie opened (see [`platform::web`]).
fn ask_platform(keys: &[String]) -> Vec<bool> {
    let (apps, urls) = split_keys(keys);
    let mut flags = vec![false; keys.len()];
    if !apps.is_empty() {
        let names: Vec<String> = apps.iter().map(|(_, a)| a.clone()).collect();
        for ((i, _), running) in apps.iter().zip(platform::running_among(&names)) {
            flags[*i] = running;
        }
    }
    if !urls.is_empty() {
        let links: Vec<String> = urls.iter().map(|(_, u)| u.clone()).collect();
        for ((i, _), open) in urls.iter().zip(platform::web::urls_open(&links)) {
            flags[*i] = open;
        }
    }
    flags
}

/// Sender side of the worker's queue; set once by [`init`].
static TX: OnceLock<mpsc::Sender<Request>> = OnceLock::new();

/// Runs `f` on the cache, creating it on first use. Recovers from a poisoned lock: a panic elsewhere
/// must never take the question away from the tap.
fn with_cache<R>(f: impl FnOnce(&mut Cache) -> R) -> R {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Cache::default))
}

/// Like [`with_cache`], but gives up (`None`) instead of waiting for the lock. For the event tap.
fn try_with_cache<R>(f: impl FnOnce(&mut Cache) -> R) -> Option<R> {
    let mut guard = match CACHE.try_lock() {
        Ok(g) => g,
        Err(TryLockError::Poisoned(e)) => e.into_inner(),
        Err(TryLockError::WouldBlock) => return None,
    };
    Some(f(guard.get_or_insert_with(Cache::default)))
}

/// Starts the `cursorpie-running` worker thread. `notifier` wakes the UI when an answer arrives, so a
/// pie that is already open draws the badges without waiting for the next move. Call once.
pub fn init(notifier: Notifier) {
    let (tx, rx) = mpsc::channel::<Request>();
    if TX.set(tx).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("cursorpie-running".into())
        .spawn(move || {
            while let Ok(mut req) = rx.recv() {
                // only the newest question is worth asking: the person has pressed again
                while let Ok(newer) = rx.try_recv() {
                    req = newer;
                }
                let t = diag::timed!("run", "scan", apps = req.apps.len());
                let flags = ask_platform(&req.apps);
                drop(t);
                // asked before the cache is locked: it may read files
                let single = single_window_among(&req.apps, &flags);
                let stored = with_cache(|c| {
                    let stored = c.finish(req.generation, &req.apps, &flags, Instant::now());
                    if stored {
                        c.single = single;
                    }
                    stored
                });
                diag::log!(
                    "run",
                    "answer",
                    stored = stored,
                    running = flags.iter().filter(|r| **r).count()
                );
                if stored {
                    notifier.wake();
                }
            }
        });
    if spawned.is_err() {
        diag::log!("run", "spawn_failed");
    }
}

/// A press of the trigger: the pies `profiles` may open. Makes sure the answer to "which of their
/// "Open app" apps run?" is on its way (or already fresh). Never blocks, so the tap may call it.
pub fn warm(profiles: &[usize]) {
    let apps = {
        let c = cfg();
        if !c.open_app_choice {
            return;
        }
        open_apps_of(&c, profiles)
    };
    let Some(generation) = with_cache(|c| c.begin(&apps, Instant::now())) else {
        return;
    };
    if let Some(tx) = TX.get() {
        let _ = tx.send(Request { generation, apps });
    }
}

/// Whether `app` (as typed in a slice) runs, according to the answer of the latest press. `None`
/// when that is not known (not answered yet, not asked about, or the cache is busy): the caller then
/// treats the app as not running. Never waits, so the tap may call it.
pub fn is_running(app: &str) -> Option<bool> {
    try_with_cache(|c| c.get(app)).flatten()
}

/// Whether the pointer on the slice of `app` is offered the two badges: the app runs (as far as is
/// known) **and** can open another window. An app that runs but keeps a single window gets no badges
/// and is brought forward as it always was. `false` when the answer is not known yet. Never waits.
pub fn offers_choice(app: &str) -> bool {
    try_with_cache(|c| c.offers_choice(app)).unwrap_or(false)
}

/// Forgets how old the latest answer is, so the next press asks again instead of reusing it for
/// [`TTL`]. Called when something the answer was worked out from has changed: the pie has just
/// learned that an app cannot open another window (see [`crate::learn`]), and the badge that
/// failed must not be offered again by a press that comes within [`TTL`]. The answer being asked
/// for right now, if any, is not touched.
pub fn invalidate() {
    with_cache(Cache::expire);
}

/// CursorPie has just started `app` (or brought it forward), or opened a web link (`app` is then
/// its key, `url:<link>`): it runs, or is open, from now on, whatever the answer of the press said.
pub fn note_launched(app: &str) {
    with_cache(|c| c.mark_running(app));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apps(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn an_app_that_runs_but_keeps_one_window_gets_no_badges() {
        let now = Instant::now();
        let list = apps(&["Safari", "Spotify"]);
        let mut c = Cache::default();
        let g = c.begin(&list, now).unwrap();
        assert!(c.finish(g, &list, &[true, true], now));
        c.single = HashSet::from(["Spotify".to_string()]);
        assert!(c.offers_choice("Safari"), "runs and takes windows");
        assert!(c.offers_choice(" Safari "), "as typed, trimmed");
        assert!(!c.offers_choice("Spotify"), "runs, but one window");
        assert_eq!(c.get("Spotify"), Some(true), "it still counts as running");
        assert!(!c.offers_choice("Notes"), "not asked about");
    }

    #[test]
    fn only_running_apps_are_asked_whether_they_keep_one_window() {
        let keys = apps(&["Spotify", "Discord", "url:https://example.com", "Safari"]);
        let single = single_window_among(&keys, &[true, false, true, true]);
        assert_eq!(single, HashSet::from(["Spotify".to_string()]));
    }

    #[test]
    fn the_pointer_picks_the_badge_on_its_side_of_the_slice() {
        let center = Point::new(0.0, 0.0);
        for total in [2usize, 3, 4, 5, 8] {
            for index in 0..total {
                let sl = std::f32::consts::TAU / total as f32;
                let start = slice_start(index, total);
                let at = |a: f32| Point::new((a.cos() * 50.0) as f64, (a.sin() * 50.0) as f64);
                assert_eq!(
                    choice_in_slice(center, at(start + sl * 0.2), index, total),
                    Choice::NewWindow,
                    "{total} slices, slice {index}: near the start"
                );
                assert_eq!(
                    choice_in_slice(center, at(start + sl * 0.8), index, total),
                    Choice::Existing,
                    "{total} slices, slice {index}: near the end"
                );
                // the distance from the center does not matter
                let far = Point::new(at(start + sl * 0.2).x * 9.0, at(start + sl * 0.2).y * 9.0);
                assert_eq!(
                    choice_in_slice(center, far, index, total),
                    Choice::NewWindow
                );
            }
        }
    }

    #[test]
    fn the_badges_sit_outside_the_rim_side_by_side_inside_the_window() {
        for win in [140.0f32, 200.0, 320.0, 360.0] {
            let size = badge_size(win);
            for total in [2usize, 3, 4, 5, 6, 7, 8] {
                for index in 0..total {
                    let g = badge_offset(win, index, total, Choice::NewWindow);
                    let y = badge_offset(win, index, total, Choice::Existing);
                    for (x, yy) in [g, y] {
                        // the whole badge is inside the window, and clear of the ring
                        let half_window = window_size(win) / 2.0;
                        assert!(
                            x.abs() + size / 2.0 <= half_window + 0.01,
                            "x outside the window"
                        );
                        assert!(yy.abs() + size / 2.0 <= half_window + 0.01, "y outside");
                        assert!(
                            x.hypot(yy) >= pie_outer(win) + size / 2.0,
                            "win {win}, {total} slices: on the ring"
                        );
                    }
                    // close together, but not on top of each other
                    let gap = (g.0 - y.0).hypot(g.1 - y.1);
                    assert!(gap > size, "win {win}, {total} slices: overlap ({gap})");
                    assert!(gap < size * 1.5, "win {win}, {total} slices: apart ({gap})");
                }
            }
        }
    }

    #[test]
    fn the_green_badge_is_on_the_side_where_the_slice_starts() {
        // the pointer on the green badge picks the green badge, on the yellow one the yellow one
        let (win, total) = (200.0, 6);
        for index in 0..total {
            let center = Point::new(0.0, 0.0);
            for choice in [Choice::NewWindow, Choice::Existing] {
                let (x, y) = badge_offset(win, index, total, choice);
                let p = Point::new(f64::from(x), f64::from(y));
                assert_eq!(
                    choice_in_slice(center, p, index, total),
                    choice,
                    "slice {index}"
                );
            }
        }
    }

    #[test]
    fn a_choice_needs_the_setting_an_app_and_a_running_app() {
        let (c, p) = (Point::new(0.0, 0.0), Point::new(40.0, 10.0));
        // no cache answer at all: never a choice
        assert_eq!(
            choose(true, Some("some app nobody asked about"), c, p, 0, 4),
            None
        );
        assert_eq!(choose(true, None, c, p, 0, 4), None);
        assert_eq!(choose(false, Some("whatever"), c, p, 0, 4), None);
    }

    #[test]
    fn the_shown_choice_round_trips() {
        for c in [None, Some(Choice::Existing), Some(Choice::NewWindow)] {
            assert_eq!(Choice::from_u8(Choice::to_u8(c)), c);
        }
        assert_eq!(Choice::from_u8(200), None);
    }

    #[test]
    fn nothing_is_asked_when_there_is_nothing_to_look_up() {
        let mut c = Cache::default();
        assert_eq!(c.begin(&[], Instant::now()), None);
        assert_eq!(c.generation, 0);
    }

    #[test]
    fn nothing_is_known_until_the_answer_arrives() {
        let mut c = Cache::default();
        let now = Instant::now();
        let a = apps(&["Safari", "Notes"]);
        let g = c.begin(&a, now).expect("a first press asks");
        assert_eq!(c.get("Safari"), None, "not answered yet");
        assert!(c.finish(g, &a, &[true, false], now));
        assert_eq!(c.get("Safari"), Some(true));
        assert_eq!(c.get(" Notes "), Some(false), "the name is trimmed");
        assert_eq!(c.get("Mail"), None, "never asked about");
    }

    #[test]
    fn an_expired_answer_is_asked_for_again_at_once() {
        let mut c = Cache::default();
        let a = apps(&["Zed"]);
        let t0 = Instant::now();
        let g = c.begin(&a, t0).unwrap();
        assert!(c.finish(g, &a, &[true], t0));
        assert_eq!(c.begin(&a, t0), None, "fresh, so reused");
        c.expire();
        assert!(
            c.begin(&a, t0).is_some(),
            "what it was worked out from changed"
        );
    }

    #[test]
    fn a_fresh_answer_is_reused_and_an_old_one_is_not() {
        let mut c = Cache::default();
        let t0 = Instant::now();
        let a = apps(&["Safari"]);
        let g = c.begin(&a, t0).unwrap();
        c.finish(g, &a, &[true], t0);
        // the next press, soon after: nothing to ask
        assert_eq!(c.begin(&a, t0 + TTL / 2), None);
        assert_eq!(c.get("Safari"), Some(true), "still readable");
        // later: ask again, and the old answer is not used while the new one is on its way
        assert!(c.begin(&a, t0 + TTL * 2).is_some());
        assert_eq!(c.get("Safari"), None);
    }

    #[test]
    fn a_press_for_other_apps_asks_again_even_when_the_answer_is_fresh() {
        let mut c = Cache::default();
        let t0 = Instant::now();
        let a = apps(&["Safari"]);
        let g = c.begin(&a, t0).unwrap();
        c.finish(g, &a, &[true], t0);
        let b = apps(&["Safari", "Notes"]);
        assert!(c.begin(&b, t0).is_some(), "Notes is not covered");
    }

    #[test]
    fn the_answer_to_an_old_request_is_thrown_away() {
        let mut c = Cache::default();
        let t0 = Instant::now();
        let a = apps(&["Safari"]);
        let first = c.begin(&a, t0).unwrap();
        let second = c.begin(&a, t0).unwrap();
        assert_ne!(first, second, "pressing again asks again while unanswered");
        assert!(!c.finish(first, &a, &[true], t0));
        assert_eq!(c.get("Safari"), None);
        assert!(c.finish(second, &a, &[false], t0));
        assert_eq!(c.get("Safari"), Some(false));
    }

    #[test]
    fn an_app_that_cursorpie_started_runs_from_then_on() {
        let mut c = Cache::default();
        let t0 = Instant::now();
        let a = apps(&["Safari"]);
        // before any answer there is nothing to correct
        c.mark_running("Safari");
        assert_eq!(c.get("Safari"), None);
        let g = c.begin(&a, t0).unwrap();
        c.finish(g, &a, &[false], t0);
        c.mark_running(" Safari ");
        assert_eq!(c.get("Safari"), Some(true));
    }

    #[test]
    fn keys_are_split_into_apps_and_links_keeping_their_places() {
        let keys = apps(&[
            "Safari",
            "url:https://example.com",
            "Notes",
            "url:http://a.b/c?d=e",
        ]);
        let (a, u) = split_keys(&keys);
        assert_eq!(a, [(0, "Safari".to_string()), (2, "Notes".to_string())]);
        assert_eq!(
            u,
            [
                (1, "https://example.com".to_string()),
                (3, "http://a.b/c?d=e".to_string())
            ]
        );
        assert_eq!(split_keys(&[]), (Vec::new(), Vec::new()));
    }

    #[test]
    fn a_link_that_cursorpie_opened_is_open_from_then_on() {
        let mut c = Cache::default();
        let t0 = Instant::now();
        let a = apps(&["url:https://example.com"]);
        let g = c.begin(&a, t0).unwrap();
        c.finish(g, &a, &[false], t0);
        assert_eq!(c.get("url:https://example.com"), Some(false));
        c.mark_running("url:https://example.com");
        assert_eq!(c.get("url:https://example.com"), Some(true));
    }

    #[test]
    fn a_short_answer_counts_the_missing_apps_as_not_running() {
        let mut c = Cache::default();
        let a = apps(&["A", "B"]);
        let g = c.begin(&a, Instant::now()).unwrap();
        c.finish(g, &a, &[true], Instant::now());
        assert_eq!(c.get("A"), Some(true));
        assert_eq!(c.get("B"), Some(false));
    }
}
