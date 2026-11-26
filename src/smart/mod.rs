//! The intelligent pie and the intelligent slice: a pie, and a ring inside a pie, whose slices are
//! suggested from what CursorPie learned about the person instead of being written by hand.
//!
//! * **Intelligent pie.** Hold Space for [`SmartConfig::hold_ms`] in an open pie and the pie turns
//!   into one made of suggestions ([`SMART_PROFILE`]); hold Space again to come back to the pie
//!   that was open. A quick press of Space still opens Settings. See [`tap::space`](crate::tap).
//!   The intelligent pie is **kept**: once it is switched on it is the pie the next presses open
//!   too, however often the pie is closed, until Space is held again ([`kept`]).
//! * **Intelligent slice.** An [`ItemKind::Smart`] slice in any pie opens a ring of suggestions
//!   like a submenu does. The grouped intelligent pie is made of such slices, one per category
//!   (*Apps*, *Sites*, *Actions*, *Flows*, see [`suggest::GROUPS`]).
//!
//! # Modules
//!
//! | Module | Does |
//! |---|---|
//! | [`habits`] | the counters that are learned, how they fade, how they score (pure) |
//! | [`suggest`] | habits and the moment to slices (pure) |
//! | [`collect`] | the worker that looks, takes note, tidies and saves |
//! | [`siteicon`] | the icons of the suggested sites: fetched in the background, kept as files |
//! | this one | the glue: [`ring`] (the one place that turns a profile and a path into slices), the frozen [`Session`], switching pies |
//!
//! # One session per gesture
//!
//! A ring is looked up by the tap thread for every pointer move (which slice is hovered, is it a
//! submenu), by the UI thread for every frame, and again when the slice is released. If the
//! suggestions could change between those, the slice that was drawn under the pointer would not be
//! the one that runs. So the suggestions are made from a [`Session`]: a copy of the habits and of
//! the moment that is taken the first time a ring is asked for after a press, and kept until the
//! next press ([`begin`]). Same session, same input, same ring (the order is total: ties break on
//! name, then on key; see the test in [`suggest`]).
//!
//! # Lock order
//!
//! `config` (held by the caller of [`ring`]) → `SESSION` → `collect::RUNTIME`. [`ring`] takes the
//! config as an argument and never locks it itself. The worker copies the config before it locks
//! the habits.

pub mod collect;
pub mod habits;
pub mod siteicon;
pub mod suggest;

use crate::config::{Action, Config, Item, ItemKind, SmartConfig, SmartSlice};
use habits::Counts;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use suggest::{Context, FlowRef, Input};

/// The profile index of the intelligent pie. It is not a profile in the config: its top ring is
/// made by [`ring`]. It is never a valid index of `Config::profiles` (which is bounded by
/// [`MAX_PROFILES`](crate::config::MAX_PROFILES)).
pub const SMART_PROFILE: usize = usize::MAX;

/// Whether `profile` is the intelligent pie.
pub fn is_smart(profile: usize) -> bool {
    profile == SMART_PROFILE
}

/// Whether `id` is CursorPie itself, which is neither learned nor suggested.
pub(crate) fn is_self(id: &str) -> bool {
    id.to_ascii_lowercase().contains("cursorpie")
}

/// Whether `id` is a program that comes to the front without being an app the person uses: the
/// parts of the Windows shell (search, the Start menu, the input panel, the lock screen, the
/// secure desktop) and `ApplicationFrameHost`, the frame of every Store app. They are in front for a
/// moment whenever the Start menu or a search is used, so a count of "what is in front" ranks them
/// above the real apps, and they have no icon of their own to show: the intelligent pie then offered
/// the same nameless suggestions to everyone. Neither learned nor suggested; one that an earlier
/// version learned is dropped at the next tidy-up. The names are those of Windows programs: no app of
/// another system has one of them for an id.
pub(crate) fn is_noise(id: &str) -> bool {
    matches!(
        id.to_ascii_lowercase().as_str(),
        "applicationframehost.exe"
            | "searchhost.exe"
            | "searchapp.exe"
            | "searchui.exe"
            | "startmenuexperiencehost.exe"
            | "shellexperiencehost.exe"
            | "textinputhost.exe"
            | "lockapp.exe"
            | "logonui.exe"
            | "consent.exe"
            | "winlogon.exe"
            | "dwm.exe"
            | "ctfmon.exe"
            | "sihost.exe"
            | "fontdrvhost.exe"
            | "widgets.exe"
    )
}

/// Starts the worker (see [`collect::init`]). Called once at startup, before the tap.
pub fn init(notifier: crate::msg::Notifier) {
    collect::init(notifier);
}

// ------------------------------------------------------------------ session

/// What the suggestions of one gesture are made from. Immutable once taken.
pub struct Session {
    habits: habits::Habits,
    ctx: Context,
    cfg: SmartConfig,
    flows: Vec<FlowRef>,
    now: u64,
}

impl Session {
    fn input(&self) -> Input<'_> {
        Input {
            habits: &self.habits,
            ctx: &self.ctx,
            cfg: &self.cfg,
            flows: &self.flows,
            now: self.now,
        }
    }
}

static SESSION: Mutex<Option<Arc<Session>>> = Mutex::new(None);

fn session_slot() -> MutexGuard<'static, Option<Arc<Session>>> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner())
}

/// A trigger was pressed: the next ring asked for gets fresh suggestions, and the worker finds out
/// which apps run and which pages are open. Cheap and never blocks: the tap thread calls it.
pub fn begin(c: &Config) {
    *session_slot() = None;
    if c.smart.enabled {
        collect::send(collect::Cmd::Press);
    }
}

/// A session made now from `c` and the habits, not kept: [`session`] keeps the one of the gesture,
/// [`warm_icons`] only looks.
fn fresh_session(c: &Config) -> Arc<Session> {
    let (habits, mut ctx) = {
        let rt = collect::runtime();
        (rt.habits.clone(), rt.ctx.clone())
    };
    ctx.front = crate::side::target();
    // the icon files of the sites are looked for here, once, and not for every ring that is asked
    ctx.site_icons = if c.smart.site_icons {
        siteicon::lookup(&habits, &ctx.tabs, &c.smart)
    } else {
        HashMap::new()
    };
    Arc::new(Session {
        habits,
        ctx,
        cfg: c.smart.clone(),
        flows: c
            .flows
            .iter()
            .map(|f| FlowRef {
                id: f.id.clone(),
                name: f.name.clone(),
            })
            .collect(),
        now: collect::now_secs(),
    })
}

/// The session of the gesture that is running: taken now if none was, from `c`.
fn session(c: &Config) -> Arc<Session> {
    let mut slot = session_slot();
    if let Some(s) = slot.as_ref() {
        return s.clone();
    }
    let s = fresh_session(c);
    *slot = Some(s.clone());
    // The slices of the rings are not in any profile, so the press did not ask whether their apps
    // and pages are open. Ask now: the answer wakes the UI, which draws the two badges (new
    // window / the one that is open) on them like on any other slice. Never blocks.
    if c.open_app_choice {
        let keys = suggest::open_keys(&s.input(), c.browser_bridge);
        crate::running::warm_more(&keys);
    }
    s
}

/// The icon strings the intelligent rings of the pies `profiles` will draw, for the warm-up when
/// the trigger is pressed: `(every icon, the ones the first frame paints)`. The rings of the
/// intelligent slices at the top of those pies, and the intelligent pie itself when it is the one
/// that is [kept]. Made from a session of its own and stores nothing: the session of the
/// gesture must be taken when the pie opens, with what the worker found by then.
pub fn warm_icons(c: &Config, profiles: &[usize]) -> (Vec<String>, Vec<String>) {
    if !c.smart.enabled {
        return (Vec::new(), Vec::new());
    }
    let slices: Vec<&SmartSlice> = profiles
        .iter()
        .filter_map(|p| c.profiles.get(*p))
        .flat_map(|p| p.items.iter())
        .filter_map(|i| match &i.kind {
            ItemKind::Smart(s) => Some(s),
            _ => None,
        })
        .collect();
    let pie_kept = kept();
    if slices.is_empty() && !pie_kept {
        return (Vec::new(), Vec::new());
    }
    let s = fresh_session(c);
    let (mut all, mut first) = (Vec::new(), Vec::new());
    if pie_kept {
        for item in suggest::pie(&s.input()) {
            first.push(item.icon.clone());
            if let ItemKind::Smart(inner) = &item.kind {
                all.extend(
                    suggest::slice(inner, &s.input())
                        .into_iter()
                        .map(|i| i.icon),
                );
            }
            all.push(item.icon);
        }
    }
    for slice in slices {
        all.extend(
            suggest::slice(slice, &s.input())
                .into_iter()
                .map(|i| i.icon),
        );
    }
    (all, first)
}

/// The sites CursorPie knows, the ones open in the browser now first, then the ones that were in
/// view the longest. For the list of sites in Settings.
pub fn known_sites() -> Vec<String> {
    let rt = collect::runtime();
    siteicon::hosts(
        &rt.habits,
        &rt.ctx.tabs,
        &SmartConfig::default(),
        usize::MAX,
    )
}

// ------------------------------------------------------------------ rings

/// The slices an intelligent slice opens into.
pub fn expand(c: &Config, slice: &SmartSlice) -> Vec<Item> {
    if !c.smart.enabled {
        return Vec::new();
    }
    let s = session(c);
    suggest::slice(slice, &s.input())
}

/// The ring at `path` of `profile`: the one place that turns a profile and a path into slices, for
/// the tap (what is hovered, what runs), the UI (what is drawn) and the actions' bookkeeping.
///
/// Borrowed from the config when no intelligent slice is on the way (the common case costs
/// nothing), owned when it is. `None` when the profile or a step of the path does not exist, or a
/// step is not a ring.
pub fn ring<'a>(c: &'a Config, profile: usize, path: &[usize]) -> Option<Cow<'a, [Item]>> {
    let top: Cow<'a, [Item]> = if is_smart(profile) {
        let s = session(c);
        Cow::Owned(suggest::pie(&s.input()))
    } else {
        Cow::Borrowed(c.profiles.get(profile)?.items.as_slice())
    };
    walk(c, top, path)
}

fn walk<'a>(c: &Config, top: Cow<'a, [Item]>, path: &[usize]) -> Option<Cow<'a, [Item]>> {
    let mut cur = top;
    for &i in path {
        let next: Cow<'a, [Item]> = match cur {
            Cow::Borrowed(items) => match &items.get(i)?.kind {
                ItemKind::Submenu(ch) => Cow::Borrowed(ch.as_slice()),
                ItemKind::Smart(s) => Cow::Owned(expand(c, s)),
                _ => return None,
            },
            Cow::Owned(items) => match &items.get(i)?.kind {
                ItemKind::Submenu(ch) => Cow::Owned(ch.clone()),
                ItemKind::Smart(s) => Cow::Owned(expand(c, s)),
                _ => return None,
            },
        };
        cur = next;
    }
    Some(cur)
}

// ------------------------------------------------------------------ switching

/// Which pie the gesture went to the intelligent pie from: `(gesture id, profile)`.
static ORIGIN: Mutex<Option<(u64, usize)>> = Mutex::new(None);

/// Whether the intelligent pie is **kept**: switched on by a held Space and not yet switched off by
/// another. While it is, every pie that a press opens is the intelligent pie, so closing the pie
/// does not end it. Not saved: a restart starts with the normal pie.
static KEPT: AtomicBool = AtomicBool::new(false);

/// Whether the intelligent pie is kept (see `KEPT`).
pub fn kept() -> bool {
    KEPT.load(Ordering::SeqCst)
}

/// Keeps the intelligent pie, or lets it go.
pub fn set_kept(on: bool) {
    KEPT.store(on, Ordering::SeqCst);
}

/// The pie that a press of `gesture` opens when its own is `profile`: the intelligent pie while it
/// is kept (and switched on in the settings `c`), `profile` otherwise. The pie that was left is
/// remembered, so a held Space goes back to it.
pub fn opening(gesture: u64, profile: usize, c: &SmartConfig) -> usize {
    if !c.enabled {
        set_kept(false); // switched off in Settings: nothing is kept
        return profile;
    }
    if is_smart(profile) || !kept() {
        return profile;
    }
    *ORIGIN.lock().unwrap_or_else(|e| e.into_inner()) = Some((gesture, profile));
    SMART_PROFILE
}

/// The pie a held Space turns the pie of `gesture` into: the intelligent pie when `current` is
/// something else, and the pie it came from (or `fallback`) when `current` is the intelligent pie.
/// Going to the intelligent pie keeps it ([`kept`]); coming back lets it go.
pub fn switch_target(gesture: u64, current: usize, fallback: usize) -> usize {
    let mut origin = ORIGIN.lock().unwrap_or_else(|e| e.into_inner());
    if is_smart(current) {
        set_kept(false);
        match origin.take() {
            Some((id, p)) if id == gesture => p,
            _ => fallback,
        }
    } else {
        set_kept(true);
        *origin = Some((gesture, current));
        SMART_PROFILE
    }
}

/// Forgets the pie the intelligent pie was entered from (the gesture is over).
pub fn end_gesture() {
    *ORIGIN.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

// ------------------------------------------------------------------ learning from use

/// An action was run from the slice `name` / `icon` of a pie, with `app` as its target.
/// Never blocks; ignored while learning is off.
pub fn note_action(c: &Config, action: &Action, name: &str, icon: &str, app: Option<String>) {
    if !(c.smart.enabled && c.smart.learn_actions) {
        return;
    }
    collect::send(collect::Cmd::Action {
        action: Box::new(action.clone()),
        name: name.to_string(),
        icon: icon.to_string(),
        app,
    });
}

/// What Settings shows about what was learned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub counts: Counts,
    /// When the first thing was learned, seconds since the epoch (0: nothing yet).
    pub since: u64,
    /// When the habits were last tidied and saved (0: not yet).
    pub tidied: u64,
}

/// How much was learned. One short lock.
pub fn stats() -> Stats {
    let rt = collect::runtime();
    Stats {
        counts: rt.habits.counts(),
        since: rt.habits.since,
        tidied: rt.habits.tidied,
    }
}

/// Tidy and save now, and name the apps (the *Learn now* button). The answer is a notice.
pub fn learn_now() {
    collect::send(collect::Cmd::LearnNow);
}

/// Forget everything and delete the file (the *Forget everything* button). The answer is a notice.
pub fn forget_all() {
    collect::send(collect::Cmd::Forget);
    *session_slot() = None;
}

/// Adds an intelligent slice to the top ring of the pie `profile` of `c`, unless it has one or is
/// full. Returns whether it was added. Settings does this when the feature is switched on and for
/// the *Add to a pie* button, so a pie of an app can have its own suggestions too.
pub fn add_slice(c: &mut Config, profile: usize) -> bool {
    let Some(p) = c.profiles.get_mut(profile) else {
        return false;
    };
    if p.items.len() >= crate::config::MAX_ITEMS
        || p.items.iter().any(|i| matches!(i.kind, ItemKind::Smart(_)))
    {
        return false;
    }
    p.items.push(Item::smart(
        "Intelligent",
        "ico:sparkle",
        SmartSlice::default(),
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SmartSource;

    fn config_with(items: Vec<Item>) -> Config {
        let mut c = Config::default();
        c.profiles[0].items = items;
        c
    }

    #[test]
    fn the_parts_of_the_shell_are_noise_and_real_apps_are_not() {
        for id in [
            "ApplicationFrameHost.exe",
            "searchhost.exe",
            "StartMenuExperienceHost.exe",
            "textinputhost.exe",
        ] {
            assert!(is_noise(id), "{id}");
        }
        // File Explorer is an app the person uses
        for id in ["explorer.exe", "chrome.exe", "code.exe", "com.apple.Safari"] {
            assert!(!is_noise(id), "{id}");
        }
    }

    #[test]
    fn the_smart_profile_is_never_a_real_one() {
        assert!(is_smart(SMART_PROFILE) && !is_smart(0));
        let c = Config::default();
        assert!(c.profiles.get(SMART_PROFILE).is_none());
    }

    #[test]
    fn a_ring_without_smart_slices_is_borrowed_from_the_config() {
        let c = Config::default();
        let r = ring(&c, 0, &[]).expect("top ring");
        assert!(matches!(r, Cow::Borrowed(_)));
        assert_eq!(r.len(), c.profiles[0].items.len());
    }

    #[test]
    fn a_path_through_a_submenu_is_borrowed_too() {
        let sub = Item::submenu(
            "More",
            "ico:stack",
            vec![Item::run("A", "x", Action::Nothing)],
        );
        let c = config_with(vec![sub]);
        let r = ring(&c, 0, &[0]).expect("submenu ring");
        assert!(matches!(r, Cow::Borrowed(_)));
        assert_eq!(r[0].name, "A");
    }

    #[test]
    fn a_wrong_path_or_profile_is_none() {
        let c = Config::default();
        assert!(ring(&c, 99, &[]).is_none());
        assert!(ring(&c, 0, &[99]).is_none());
        // a slice that is not a ring cannot be walked into
        let first_run = c.profiles[0]
            .items
            .iter()
            .position(|i| matches!(i.kind, ItemKind::Run(_)));
        if let Some(i) = first_run {
            assert!(ring(&c, 0, &[i]).is_none());
        }
    }

    #[test]
    fn an_intelligent_slice_is_empty_while_the_feature_is_off() {
        let slice = Item::smart(
            "Suggestions",
            "ico:sparkle",
            SmartSlice::of(SmartSource::Flows),
        );
        let c = config_with(vec![slice]);
        assert!(!c.smart.enabled);
        let r = ring(&c, 0, &[0]).expect("a smart slice is a ring");
        assert!(r.is_empty());
    }

    #[test]
    fn the_smart_pie_says_it_is_off_before_it_is_switched_on() {
        let c = Config::default();
        let r = ring(&c, SMART_PROFILE, &[]).expect("the smart pie always has a ring");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "Intelligent is off");
    }

    #[test]
    fn a_held_space_goes_there_and_back() {
        let _s = crate::tap::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        end_gesture();
        let there = switch_target(7, 2, 0);
        assert_eq!(there, SMART_PROFILE);
        let back = switch_target(7, SMART_PROFILE, 0);
        assert_eq!(back, 2, "to the pie it came from");
        // an origin of another gesture is not trusted
        switch_target(8, 3, 0);
        assert_eq!(switch_target(9, SMART_PROFILE, 1), 1);
        end_gesture();
        set_kept(false);
    }

    #[test]
    fn the_intelligent_pie_stays_until_space_is_held_again() {
        let _s = crate::tap::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        end_gesture();
        let on = SmartConfig {
            enabled: true,
            ..SmartConfig::default()
        };
        set_kept(false);
        assert_eq!(opening(1, 2, &on), 2, "not kept: the pie asked for");
        switch_target(1, 2, 0); // held Space in the pie of gesture 1
        assert!(kept());
        end_gesture(); // the pie is closed
        assert!(kept(), "closing the pie does not end it");
        assert_eq!(opening(2, 2, &on), SMART_PROFILE, "the next press opens it");
        end_gesture();
        assert_eq!(opening(3, 0, &on), SMART_PROFILE, "and the one after that");
        // held again: back to the pie that press would have opened
        assert_eq!(switch_target(3, SMART_PROFILE, 9), 0);
        assert!(!kept());
        assert_eq!(opening(4, 2, &on), 2);
        end_gesture();
    }

    #[test]
    fn switching_the_feature_off_lets_the_kept_pie_go() {
        let _s = crate::tap::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        set_kept(true);
        assert_eq!(opening(1, 2, &SmartConfig::default()), 2);
        assert!(!kept());
    }

    #[test]
    fn the_slice_is_added_once_and_only_where_there_is_room() {
        let mut c = Config::default();
        assert!(add_slice(&mut c, 0));
        assert!(!add_slice(&mut c, 0), "one is enough");
        let n = c.profiles[0].items.len();
        assert!(matches!(
            c.profiles[0].items[n - 1].kind,
            ItemKind::Smart(_)
        ));
        assert!(!add_slice(&mut c, 99), "no such pie");
        c.profiles[1].items.clear();
        while c.profiles[1].items.len() < crate::config::MAX_ITEMS {
            c.profiles[1]
                .items
                .push(Item::run("x", "x", Action::Nothing));
        }
        assert!(!add_slice(&mut c, 1), "a full pie");
    }
}
