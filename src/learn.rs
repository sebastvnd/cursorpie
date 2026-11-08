//! A small memory of what happened when a new window was asked of an app, so the pie learns which
//! apps cannot open another window (see [`windowing`](crate::windowing)).
//!
//! Each app has two numbers, how often a new window **worked** (the app had more windows a moment
//! after) and how often it **failed**. They are the evidence of a Beta(1, 1) estimate of "a new
//! window works": `p = (worked + 1) / (worked + failed + 2)`. Four rules make it forgiving:
//!
//! * **It takes two failures to condemn an app** (`1 / (2 + 2) = 0.25`) **and two successes to vouch
//!   for one** (`3 / 4`): one slow start, one click on the wrong badge or one popup that looked like a
//!   window is not a verdict, in either direction.
//! * **Evidence fades** (half-life [`HALF_LIFE_DAYS`]): an app that learned to open windows in an
//!   update, or was misjudged, is judged again from fresh attempts.
//! * **A success counts against old failures** (they are halved): the latest word wins.
//! * **Two failures in a row condemn an app, whatever came before** ([`CONDEMN_STREAK`]). The three
//!   rules above alone leave a mixed record (a few slips that looked like successes between the
//!   failures) in a middle zone where nothing is decided, and then the pie keeps offering a badge
//!   that has just failed twice. The streak is what the app did *last*; it holds for as long as the
//!   evidence has not faded ([`HALF_LIFE_DAYS`]), then the counts decide again.
//!
//! In between (`None`) the built-in list and the app's own declaration decide, so a new install
//! behaves as before and only gets smarter with use. The memory is a small JSON file next to the
//! configuration; nothing in it leaves the computer, and it holds app names and counts only.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Days after which the weight of an observation has halved.
pub const HALF_LIFE_DAYS: f64 = 60.0;
/// Most evidence kept for one app, so an old habit can still be unlearned.
const CAP: f64 = 20.0;
/// An app whose chance of opening a window is below this keeps one window.
const SINGLE_BELOW: f64 = 0.26;
/// An app whose chance is at least this is known to take new windows.
const MULTI_FROM: f64 = 0.7;
/// Most apps remembered; the least recently seen go first.
const MAX_APPS: usize = 500;
/// This many failures in a row (the latest word) make an app a one-window app, however many
/// successes came before them.
pub const CONDEMN_STREAK: i16 = 2;

/// What is known about one app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// New windows that appeared (decayed).
    pub worked: f64,
    /// New windows that did not (decayed).
    pub failed: f64,
    /// When the numbers were last updated, in seconds since the epoch.
    pub at: u64,
    /// The latest outcomes of the same kind in a row: positive for new windows that appeared,
    /// negative for the ones that did not (`-2`: the last two failed). Missing in files written
    /// before it existed, which read as `0`.
    #[serde(default)]
    pub streak: i16,
}

impl Record {
    /// Days since the numbers were last updated, as of `now`.
    fn age_days(&self, now: u64) -> f64 {
        now.saturating_sub(self.at) as f64 / 86_400.0
    }

    /// The numbers as they are `now`, faded by the time since `at`.
    fn faded(&self, now: u64) -> (f64, f64) {
        let f = 0.5f64.powf(self.age_days(now) / HALF_LIFE_DAYS);
        (self.worked * f, self.failed * f)
    }

    /// Whether the latest word is a run of failures that is still fresh. Once the evidence has
    /// faded by half the run says no more and the counts decide.
    fn condemned_by_streak(&self, now: u64) -> bool {
        self.streak <= -CONDEMN_STREAK && self.age_days(now) < HALF_LIFE_DAYS
    }

    /// The estimated chance that a new window opens.
    pub fn p_works(&self, now: u64) -> f64 {
        let (w, f) = self.faded(now);
        (w + 1.0) / (w + f + 2.0)
    }
}

/// The version of the way new windows are counted that wrote the numbers. 2: the windows are
/// counted right before the request, with the app in front (see [`windowing::Asked`](crate::windowing::Asked)); before
/// that, a window that bringing the app forward showed by itself counted as a new one.
const GENERATION: u8 = 2;

/// What the pie has learned, by app key (see [`windowing::key`](crate::windowing::key)).
#[derive(Default, Serialize, Deserialize)]
pub struct Model {
    /// Which way of counting wrote `apps`; `0` in files that do not say.
    #[serde(default)]
    generation: u8,
    apps: HashMap<String, Record>,
}

impl Model {
    /// Drops what an older way of counting got wrong: its successes (the windows it saw may have
    /// been shown by bringing the app forward, not made by the request), kept as doubt. Its
    /// failures stand, since a window that did not come is never the fault of the counting.
    /// Done once, when the file is read.
    fn migrate(&mut self) {
        if self.generation >= GENERATION {
            return;
        }
        for r in self.apps.values_mut() {
            r.worked = 0.0;
            r.streak = r.streak.min(0);
        }
        self.generation = GENERATION;
    }

    /// Records that a new window of `key` did (`worked`) or did not appear.
    pub fn observe(&mut self, key: &str, worked: bool, now: u64) {
        if key.is_empty() {
            return;
        }
        let r = self.apps.entry(key.to_string()).or_default();
        let (mut w, mut f) = r.faded(now);
        let streak = match (worked, r.streak) {
            (true, s) if s > 0 => s.saturating_add(1),
            (true, _) => 1,
            (false, s) if s < 0 => s.saturating_sub(1),
            (false, _) => -1,
        };
        if worked {
            w += 1.0;
            f *= 0.5; // the latest word wins
        } else {
            f += 1.0;
        }
        *r = Record {
            worked: w.min(CAP),
            failed: f.min(CAP),
            at: now,
            streak,
        };
        if self.apps.len() > MAX_APPS {
            // forget the app seen longest ago
            if let Some(old) = self
                .apps
                .iter()
                .min_by_key(|(_, r)| r.at)
                .map(|(k, _)| k.clone())
            {
                self.apps.remove(&old);
            }
        }
    }

    /// `Some(true)`: it keeps one window. `Some(false)`: it takes new windows. `None`: not enough
    /// is known, so the caller's other sources decide.
    pub fn verdict(&self, key: &str, now: u64) -> Option<bool> {
        let record = self.apps.get(key)?;
        if record.condemned_by_streak(now) {
            return Some(true);
        }
        let p = record.p_works(now);
        if p < SINGLE_BELOW {
            Some(true)
        } else if p >= MULTI_FROM {
            Some(false)
        } else {
            None
        }
    }

    /// The streak of `key` (see [`Record::streak`]), `0` when nothing is known. For the log.
    fn streak_of(&self, key: &str) -> i16 {
        self.apps.get(key).map_or(0, |r| r.streak)
    }
}

// ------------------------------------------------------------------ the memory on disk

static MODEL: Mutex<Option<Model>> = Mutex::new(None);

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn path() -> std::path::PathBuf {
    crate::config::app_support_dir().join("window-habits.json")
}

/// Runs `f` on the model, reading the file on first use. A missing or broken file is an empty
/// memory: it is only an optimization and must never cost the user anything.
fn with_model<R>(f: impl FnOnce(&mut Model) -> R) -> R {
    let mut g = MODEL.lock().unwrap_or_else(|e| e.into_inner());
    let m = g.get_or_insert_with(|| {
        let mut m: Model = std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        m.migrate();
        m
    });
    f(m)
}

/// What is known about `key`, see [`Model::verdict`].
pub fn verdict(key: &str) -> Option<bool> {
    if key.is_empty() {
        return None;
    }
    with_model(|m| m.verdict(key, now()))
}

/// Records what happened to a new window of `key` and saves the memory (a few hundred bytes, written
/// on the worker that watched, never on the event tap).
pub fn observe(key: &str, worked: bool) {
    let (json, streak) = with_model(|m| {
        m.observe(key, worked, now());
        (serde_json::to_string(m).ok(), m.streak_of(key))
    });
    diag::log!("run", "learned", worked = worked, streak = streak);
    if let Some(json) = json {
        let p = path();
        let tmp = p.with_extension("json.tmp");
        // written aside and moved over, so a crash cannot leave half a file
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
    // the answer of the last press was worked out before this; the next press must ask again, or
    // the badge that has just failed is still on show until the answer is old enough
    crate::running::invalidate();
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    #[test]
    fn nothing_is_known_about_an_app_that_was_never_tried() {
        assert_eq!(Model::default().verdict("spotify", 0), None);
    }

    #[test]
    fn one_failure_is_not_a_verdict_and_two_are() {
        let mut m = Model::default();
        m.observe("calc", false, 0);
        assert_eq!(m.verdict("calc", 0), None);
        m.observe("calc", false, 10);
        assert_eq!(m.verdict("calc", 10), Some(true), "keeps one window");
    }

    #[test]
    fn it_takes_two_successes_to_vouch_for_an_app() {
        let mut m = Model::default();
        m.observe("code", true, 0);
        assert_eq!(m.verdict("code", 0), None, "one popup could have fooled it");
        m.observe("code", true, 1);
        assert_eq!(m.verdict("code", 1), Some(false), "takes new windows");
    }

    #[test]
    fn the_latest_word_wins_over_old_failures() {
        let mut m = Model::default();
        m.observe("app", false, 0);
        m.observe("app", false, 1);
        assert_eq!(m.verdict("app", 1), Some(true));
        m.observe("app", true, 2);
        assert_eq!(m.verdict("app", 2), None, "doubt again, the list decides");
        m.observe("app", true, 3);
        assert_eq!(m.verdict("app", 3), None, "still not sure");
        m.observe("app", true, 4);
        assert_eq!(m.verdict("app", 4), Some(false), "three successes win");
    }

    #[test]
    fn old_evidence_fades_and_the_app_is_judged_again() {
        let mut m = Model::default();
        m.observe("app", false, 0);
        m.observe("app", false, 1);
        assert_eq!(m.verdict("app", 1), Some(true));
        let later = 1 + (HALF_LIFE_DAYS as u64) * 3 * DAY;
        assert_eq!(m.verdict("app", later), None, "three half-lives later");
        // and fresh successes outweigh what is left of the failures
        m.observe("app", true, later);
        m.observe("app", true, later + 1);
        assert_eq!(m.verdict("app", later + 1), Some(false));
    }

    #[test]
    fn evidence_is_capped_so_a_habit_can_be_unlearned() {
        let mut m = Model::default();
        for t in 0..100 {
            m.observe("app", false, t);
        }
        assert!(m.apps["app"].failed <= CAP);
        for t in 100..110 {
            m.observe("app", true, t);
        }
        assert_eq!(m.verdict("app", 110), Some(false));
    }

    #[test]
    fn the_memory_is_bounded_and_forgets_the_oldest_app() {
        let mut m = Model::default();
        for i in 0..=MAX_APPS {
            m.observe(&format!("app{i}"), true, i as u64);
        }
        assert_eq!(m.apps.len(), MAX_APPS);
        assert!(!m.apps.contains_key("app0"), "the oldest went");
        assert!(m.apps.contains_key(&format!("app{MAX_APPS}")));
    }

    #[test]
    fn two_failures_in_a_row_condemn_whatever_came_before() {
        // the record of a real app: slips that looked like windows between the failures left it at
        // worked 2.0 / failed 2.5, a chance of 0.46 that is neither "single" nor "multi"
        let mut m = Model::default();
        let outcomes = [false, false, true, true, false, false];
        for (t, worked) in outcomes.into_iter().enumerate() {
            m.observe("zed", worked, t as u64);
        }
        let p = m.apps["zed"].p_works(5);
        assert!(
            (SINGLE_BELOW..MULTI_FROM).contains(&p),
            "the counts alone do not decide: {p}"
        );
        assert_eq!(m.verdict("zed", 5), Some(true), "the last two failed");
    }

    #[test]
    fn a_success_ends_the_run_of_failures() {
        let mut m = Model::default();
        m.observe("app", false, 0);
        m.observe("app", false, 1);
        assert_eq!(m.verdict("app", 1), Some(true));
        m.observe("app", true, 2);
        assert_eq!(m.apps["app"].streak, 1);
        assert_eq!(m.verdict("app", 2), None, "doubt again");
    }

    #[test]
    fn a_run_of_failures_holds_only_while_the_evidence_is_fresh() {
        let mut m = Model::default();
        m.observe("app", true, 0);
        m.observe("app", true, 1);
        m.observe("app", false, 2);
        m.observe("app", false, 3);
        assert_eq!(m.verdict("app", 3), Some(true));
        let day = |d: f64| 3 + (d * DAY as f64) as u64;
        assert_eq!(m.verdict("app", day(HALF_LIFE_DAYS - 1.0)), Some(true));
        assert_eq!(
            m.verdict("app", day(HALF_LIFE_DAYS + 1.0)),
            None,
            "the app is judged again from new attempts"
        );
    }

    #[test]
    fn a_memory_written_before_streaks_still_loads() {
        // the file of an app with a mixed record, as an older version wrote it
        let json = r#"{"apps":{"zed":{"worked":2.0,"failed":2.5,"at":1000}}}"#;
        let mut m: Model = serde_json::from_str(json).unwrap();
        assert_eq!(m.apps["zed"].streak, 0);
        assert_eq!(m.verdict("zed", 1000), None, "nothing new yet to go by");
        m.observe("zed", false, 1001);
        assert_eq!(m.verdict("zed", 1001), None, "one failure is not a run");
        m.observe("zed", false, 1002);
        assert_eq!(m.verdict("zed", 1002), Some(true));
    }

    #[test]
    fn successes_of_the_old_way_of_counting_are_dropped_once() {
        // a launcher that never opened a window, and an app whose failures were right
        let json = r#"{"apps":{
            "launcher":{"worked":4.0,"failed":0.0,"at":1000,"streak":4},
            "zed":{"worked":2.0,"failed":2.5,"at":1000}}}"#;
        let mut m: Model = serde_json::from_str(json).unwrap();
        assert_eq!(
            m.verdict("launcher", 1000),
            Some(false),
            "what the old count said"
        );
        m.migrate();
        assert_eq!(m.verdict("launcher", 1000), None, "back to doubt");
        assert_eq!(m.verdict("zed", 1000), Some(true), "its failures stand");
        // a success after that is kept: the file is not migrated again
        m.observe("launcher", true, 1001);
        m.migrate();
        assert!((m.apps["launcher"].worked - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_memory_round_trips_through_json() {
        let mut m = Model::default();
        m.observe("spotify", false, 5);
        m.observe("spotify", false, 6);
        let back: Model = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back.verdict("spotify", 6), Some(true));
    }
}
