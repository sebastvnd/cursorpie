//! The daily look for a newer release, and the offer that follows.
//!
//! When Settings opens, CursorPie looks at most once a day for a newer stable release and, if there
//! is one, offers it once that day (a sheet over Settings) and always shows it in the About tab. The
//! same on macOS, Windows and Linux. Nothing is downloaded until the person presses *Install update*;
//! then [`install`] downloads the package for their system from the release's own address, checks it
//! against the checksum GitHub publishes, swaps it in and starts the new version (see `docs/user/updates.md`).
//! Where that is not possible (a copy that is not an ordinary install, a folder that cannot be
//! written) the person is sent to the release page instead. No address from the network is ever
//! opened or downloaded: the page is the constant [`LATEST_RELEASE_URL`](crate::about::LATEST_RELEASE_URL) and the package's address is
//! made from the release's tag.
//!
//! | Part | Contents |
//! |---|---|
//! | [`Version`] | `x.y.z`, parsed and compared |
//! | [`plan`] | what a Settings that just opened should do, decided from the clock and [`State`] (pure) |
//! | [`State`] | what is remembered between runs: the day of the last check, what it found, the day of the last offer |
//! | [`fetch_latest`], [`spawn_check`] | the request itself (see [`platform::http`](crate::platform::http)), on a worker thread |
//! | [`install`] | the one-click install: package choice, download, checksum, swap (see its table) |
//! | [`sha256`] | the checksum |
//! | [`Status`] | what the About tab shows |
//!
//! The state is a small file of its own, `update_state.json`, next to `config.json`. It is not part
//! of the config on purpose: it changes without the person touching Settings, so it must not make a
//! backup, an export or an "unsaved changes" question out of a check. Whether the check runs at all
//! is the setting [`Config::check_updates`](crate::config::Config::check_updates).

pub mod install;
pub mod sha256;

use crate::about::{LATEST_RELEASE_API_URL, VERSION};
use crate::config::backup::write_atomic;
use crate::config::{app_support_dir, lock, read_text_capped};
use crate::msg::{Msg, Notifier};
use crate::platform::http::{self, HttpError};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Name of the state file inside the app support directory.
pub const STATE_FILE: &str = "update_state.json";

/// Longest the request takes before it is given up, in seconds.
pub const CHECK_TIMEOUT_S: u64 = 10;

/// Largest answer that is read. The release description of a real release is a few KiB.
pub const MAX_ANSWER_BYTES: u64 = 256 * 1024;

/// Largest state file that is read. A real one is under 100 bytes.
const MAX_STATE_BYTES: u64 = 4 * 1024;

/// Seconds in a day, for [`day_number`].
const DAY_S: u64 = 86_400;

// ---------------------------------------------------------------------------- versions

/// A release number, `MAJOR.MINOR.PATCH`, ordered the way releases are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Reads `0.8.6` or `v0.8.6`. `None` for anything else: a pre-release (`0.9.0-beta`), a tag that
    /// is not a version, a number with a sign or with more or fewer than three parts. CursorPie
    /// never makes pre-releases (see `CONTRIBUTING.md`), so none is ever offered.
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim();
        let t = t.strip_prefix('v').unwrap_or(t);
        let mut parts = t.split('.');
        let version = Self {
            major: number(parts.next()?)?,
            minor: number(parts.next()?)?,
            patch: number(parts.next()?)?,
        };
        parts.next().is_none().then_some(version)
    }

    /// The version of this build, from `Cargo.toml`.
    pub fn current() -> Option<Self> {
        Self::parse(VERSION)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A whole number written with digits only (`u64::from_str` also takes a leading `+`).
fn number(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// The version of the release that GitHub's answer describes. `None` for a draft, a pre-release, a
/// tag that is not a version, or text that is not JSON.
pub fn parse_latest(json: &str) -> Option<Version> {
    install::parse_release(json).map(|release| release.version)
}

// ---------------------------------------------------------------------------- the clock

/// The day `now` falls in, counted in days since 1970-01-01 (UTC). A day is "the same day" for the
/// once-a-day rules when the numbers are equal. A clock before 1970 counts as day 0.
pub fn day_number(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() / DAY_S)
}

// ---------------------------------------------------------------------------- what is remembered

/// What is remembered between runs (the file [`STATE_FILE`]). Missing or unreadable means the
/// defaults, which mean "never checked, never offered".
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// The [`day_number`] of the last check that got an answer (0 = never).
    pub checked_day: u64,
    /// The newest version that check found, as `x.y.z` (empty = none known).
    pub latest: String,
    /// The [`day_number`] of the day the offer was last shown (0 = never).
    pub offered_day: u64,
}

impl State {
    /// The newest version a check has found, if any.
    pub fn latest_version(&self) -> Option<Version> {
        Version::parse(&self.latest)
    }
}

/// Serializes writes of the state file: the check's worker thread and the UI thread both change it.
static STATE_IO: Mutex<()> = Mutex::new(());

/// Reads the state file of `dir`. Anything wrong with it gives the defaults: a state that cannot be
/// read only costs one more check.
fn load_state_from(dir: &Path) -> State {
    read_text_capped(dir.join(STATE_FILE), MAX_STATE_BYTES)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Reads, changes and writes back the state file of `dir`, with the writes of other threads kept
/// apart. Errors are ignored: a failed write must never crash the app, and the worst case is one more
/// check or one more offer.
fn modify_state_in(dir: &Path, change: impl FnOnce(&mut State)) {
    let _io = lock(&STATE_IO);
    let mut state = load_state_from(dir);
    change(&mut state);
    if let Ok(data) = serde_json::to_string_pretty(&state) {
        let _ = write_atomic(&dir.join(STATE_FILE), &data);
    }
}

/// The remembered state. One tiny read, so it is fine to call when Settings opens.
pub fn load_state() -> State {
    load_state_from(&app_support_dir())
}

/// Remembers that the offer was shown on `today`. One tiny write, at most once a day.
pub fn mark_offered(today: u64) {
    modify_state_in(&app_support_dir(), |s| s.offered_day = today);
}

// ---------------------------------------------------------------------------- what to do

/// What a Settings window that just opened does about updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Look for a newer release now (in the background).
    pub check: bool,
    /// Offer a newer release that an earlier check already found.
    pub offer: bool,
}

/// Decides what a Settings that just opened does, from the setting (`enabled`), the day (`today`),
/// what is remembered and the version of this build.
///
/// * At most one check a day, and only one that got an answer counts: with no network the next
///   opening tries again.
/// * At most one offer a day, and only for a version newer than this build. A check that is still
///   to come offers its result itself (see `App::update_checked`), so a first run offers nothing here.
pub fn plan(enabled: bool, today: u64, state: &State, current: Version) -> Plan {
    if !enabled {
        return Plan::default();
    }
    let newer_known = state
        .latest_version()
        .is_some_and(|latest| latest > current);
    Plan {
        check: state.checked_day != today,
        offer: newer_known && state.offered_day != today,
    }
}

/// Whether a check that just found `found` should be offered now: it is newer than this build and
/// nothing was offered on `today` yet.
pub fn offer_due(found: Version, current: Version, state: &State, today: u64) -> bool {
    found > current && state.offered_day != today
}

// ---------------------------------------------------------------------------- the request

/// Why a check gave no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Neither `curl` nor `wget` is installed.
    NoClient,
    /// The release page could not be reached (no network, a timeout, an error status).
    Unreachable,
    /// GitHub answered with something that is not a release this program can read.
    BadAnswer,
}

impl Failure {
    /// One sentence for the About tab.
    pub fn explain(self) -> &'static str {
        match self {
            Failure::NoClient => {
                "CursorPie needs curl (or wget) to look for updates, and neither was found."
            }
            Failure::Unreachable => {
                "The release page could not be reached. Check the connection and try again."
            }
            Failure::BadAnswer => "GitHub answered, but not with a release CursorPie can read.",
        }
    }
}

impl From<HttpError> for Failure {
    fn from(e: HttpError) -> Self {
        match e {
            HttpError::NoClient => Failure::NoClient,
            HttpError::Failed => Failure::Unreachable,
            HttpError::BadAnswer => Failure::BadAnswer,
        }
    }
}

/// Asks GitHub for the newest stable release and returns its version. Blocks for up to
/// [`CHECK_TIMEOUT_S`] seconds: call it from a worker thread.
///
/// # Errors
///
/// See [`Failure`].
pub fn fetch_latest() -> Result<Version, Failure> {
    let text = http::get_text(LATEST_RELEASE_API_URL, CHECK_TIMEOUT_S, MAX_ANSWER_BYTES)?;
    parse_latest(&text).ok_or(Failure::BadAnswer)
}

/// Looks for a newer release on a worker thread. An answer is written to the state file first (so
/// the day counts as checked) and then sent to the UI as [`Msg::UpdateChecked`]; `manual` says that
/// the person pressed *Check now*, which is answered in the About tab and not with an offer.
pub fn spawn_check(notifier: Notifier, manual: bool) {
    std::thread::spawn(move || {
        let result = fetch_latest();
        if let Ok(found) = result {
            let today = day_number(SystemTime::now());
            modify_state_in(&app_support_dir(), |s| {
                s.checked_day = today;
                s.latest = found.to_string();
            });
        }
        let ok = result.is_ok();
        diag::log!("ui", "update_checked", manual = manual, ok = ok);
        notifier.send(Msg::UpdateChecked { manual, result });
    });
}

// ---------------------------------------------------------------------------- what is shown

/// What the About tab says about updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Nothing is known yet in this run.
    #[default]
    Idle,
    /// A check is running.
    Checking,
    /// The newest release is not newer than this build.
    UpToDate,
    /// A newer release exists.
    Available(Version),
    /// The last check got no answer.
    Failed(Failure),
    /// The package of this version is being downloaded and checked.
    Downloading(Version),
    /// The package of this version is downloaded, checked and waits to be swapped in.
    Ready(Version),
    /// The package of this version could not be installed (the release page is still the way to update).
    InstallFailed(Version, install::InstallError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn v(text: &str) -> Version {
        Version::parse(text).expect("a version")
    }

    fn state(checked_day: u64, latest: &str, offered_day: u64) -> State {
        State {
            checked_day,
            latest: latest.into(),
            offered_day,
        }
    }

    fn plan_of(check: bool, offer: bool) -> Plan {
        Plan { check, offer }
    }

    /// A fresh, empty directory unique to this test.
    fn temp_dir(name: &str) -> std::path::PathBuf {
        let id = std::process::id();
        let d = std::env::temp_dir().join(format!("cursorpie-update-{id}-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn versions_are_read_with_or_without_the_v() {
        let a = v("0.8.6");
        assert_eq!((a.major, a.minor, a.patch), (0, 8, 6));
        let b = v("v10.0.12");
        assert_eq!((b.major, b.minor, b.patch), (10, 0, 12));
        assert_eq!(v(" v1.2.3 \n"), v("1.2.3"));
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
    }

    #[test]
    fn anything_that_is_not_a_plain_version_is_refused() {
        let refused = |text: &str| assert_eq!(Version::parse(text), None, "{text:?}");
        for text in ["", "v", "1", "1.2", "1.2.3.4", "1.2.3-beta"] {
            refused(text);
        }
        for text in ["1.2.3+build", "v1.x.3", "+1.2.3", "-1.2.3", "1..3"] {
            refused(text);
        }
        refused("latest");
        refused("1.2.3 beta");
        refused("99999999999999999999.0.0"); // too big for a u64
    }

    #[test]
    fn versions_are_ordered_by_number_not_by_text() {
        assert!(v("0.9.0") > v("0.8.6"));
        assert!(v("0.10.0") > v("0.9.9"), "10 is more than 9");
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.8.7") > v("0.8.6"));
        assert!(v("0.8.6") == v("v0.8.6"));
    }

    #[test]
    fn this_build_has_a_version_the_check_can_compare() {
        assert!(Version::current().is_some(), "{VERSION}");
    }

    #[test]
    fn the_answer_of_a_release_is_read_by_its_tag() {
        let full = r#"{"tag_name": "v0.9.0", "draft": false, "prerelease": false}"#;
        assert_eq!(parse_latest(full), Some(v("0.9.0")));
        assert_eq!(parse_latest(r#"{"tag_name": "0.9.1"}"#), Some(v("0.9.1")));
    }

    #[test]
    fn drafts_prereleases_and_nonsense_are_never_offered() {
        let none = |json: &str| assert_eq!(parse_latest(json), None, "{json}");
        none(r#"{"tag_name": "v0.9.0", "draft": true}"#);
        none(r#"{"tag_name": "v0.9.0", "prerelease": true}"#);
        none(r#"{"tag_name": "v0.9.0-rc1"}"#);
        none(r#"{"tag_name": 5}"#);
        none(r#"{"name": "v0.9.0"}"#);
        none("<html>rate limited</html>");
        none("");
    }

    #[test]
    fn a_day_is_counted_in_whole_days_since_1970() {
        let at = |secs: u64| UNIX_EPOCH + Duration::from_secs(secs);
        assert_eq!(day_number(at(0)), 0);
        assert_eq!(day_number(at(DAY_S - 1)), 0);
        assert_eq!(day_number(at(DAY_S)), 1);
        assert_eq!(day_number(at(3 * DAY_S + 5)), 3);
        let before = UNIX_EPOCH - Duration::from_secs(10);
        assert_eq!(day_number(before), 0);
    }

    #[test]
    fn with_the_setting_off_nothing_happens() {
        let s = state(0, "9.9.9", 0);
        assert_eq!(plan(false, 100, &s, v("0.8.6")), Plan::default());
    }

    #[test]
    fn a_first_run_checks_and_offers_nothing_yet() {
        let p = plan(true, 100, &State::default(), v("0.8.6"));
        assert_eq!(p, plan_of(true, false));
    }

    #[test]
    fn one_check_a_day() {
        let s = state(100, "0.8.6", 0);
        assert!(
            !plan(true, 100, &s, v("0.8.6")).check,
            "already looked today"
        );
        assert!(plan(true, 101, &s, v("0.8.6")).check, "a new day");
    }

    #[test]
    fn a_newer_release_that_is_known_is_offered_once_a_day() {
        let current = v("0.8.6");
        let s = state(100, "0.9.0", 0);
        assert_eq!(plan(true, 100, &s, current), plan_of(false, true));
        let offered = state(100, "0.9.0", 100);
        assert_eq!(plan(true, 100, &offered, current), Plan::default());
        // the next day it is offered again, and a check runs too
        assert_eq!(plan(true, 101, &offered, current), plan_of(true, true));
    }

    #[test]
    fn a_release_that_is_not_newer_is_never_offered() {
        for latest in ["0.8.6", "0.8.5", "0.1.0", ""] {
            let s = state(1, latest, 0);
            assert!(!plan(true, 100, &s, v("0.8.6")).offer, "{latest:?}");
        }
        // the person updated: what was found earlier is old news
        let s = state(100, "0.9.0", 0);
        assert!(!plan(true, 100, &s, v("0.9.0")).offer);
    }

    #[test]
    fn a_fresh_answer_is_offered_unless_something_was_offered_today() {
        let (new, old) = (v("0.9.0"), v("0.8.6"));
        assert!(offer_due(new, old, &State::default(), 100));
        assert!(!offer_due(old, old, &State::default(), 100), "not newer");
        assert!(!offer_due(new, old, &state(100, "", 100), 100));
        assert!(offer_due(new, old, &state(100, "", 99), 100));
    }

    #[test]
    fn the_state_survives_a_round_trip_and_a_change_keeps_the_rest() {
        let d = temp_dir("state");
        assert_eq!(load_state_from(&d), State::default(), "no file yet");
        modify_state_in(&d, |s| {
            s.checked_day = 7;
            s.latest = "0.9.0".into();
        });
        modify_state_in(&d, |s| s.offered_day = 8);
        assert_eq!(load_state_from(&d), state(7, "0.9.0", 8));
        assert_eq!(load_state_from(&d).latest_version(), Some(v("0.9.0")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_damaged_state_file_means_a_fresh_start() {
        let d = temp_dir("damaged");
        let file = d.join(STATE_FILE);
        for text in ["", "not json", "[1, 2]", r#"{"checked_day": "no"}"#] {
            std::fs::write(&file, text).unwrap();
            assert_eq!(load_state_from(&d), State::default(), "{text:?}");
        }
        // a file from a newer version with more fields still loads
        std::fs::write(&file, r#"{"checked_day": 3, "later": true}"#).unwrap();
        assert_eq!(load_state_from(&d).checked_day, 3);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn every_failure_has_its_own_sentence() {
        let all = [Failure::NoClient, Failure::Unreachable, Failure::BadAnswer];
        for (i, a) in all.iter().enumerate() {
            assert!(!a.explain().is_empty());
            for b in &all[i + 1..] {
                assert_ne!(a.explain(), b.explain());
            }
        }
        assert_eq!(Failure::from(HttpError::NoClient), Failure::NoClient);
        assert_eq!(Failure::from(HttpError::Failed), Failure::Unreachable);
        assert_eq!(Failure::from(HttpError::BadAnswer), Failure::BadAnswer);
    }
}
