//! Whether an app can open one more window, the same answer on every OS.
//!
//! The two badges of an "Open app" slice (see [`running`](crate::running)) offer a *new window* of
//! an app that already runs. For an app that has only one window (a music player, a chat client, a
//! password manager, a calculator) the green badge can only fail: Windows and Linux start the app
//! again and it shows the window it has, macOS presses Cmd+N into a window that has no such
//! command. Offering the choice there is noise, so the pie leaves the badges out and the slice
//! simply brings the app forward, as it always did.
//!
//! The decision is made by [`single_window`], which combines three sources:
//!
//! 1. **What happened before** ([`learn`](crate::learn)): every time a new window is asked of an app
//!    that already has windows, [`watch_new_window`] counts its windows for a few seconds, and the
//!    result (a window appeared, or not) is remembered. Enough failures make an app a one-window
//!    app, a success makes it a multi-window one, whatever the two sources below say.
//! 2. **What the app says about itself**, if the OS has a place for it: the `SingleMainWindow` key
//!    of a Linux `.desktop` file (see [`platform::declares_single_window`]). Windows and macOS have
//!    no such declaration that can be read without launching the app, so they say nothing.
//! 3. **A built-in list of well-known one-window apps**, matched by name, so the same app is
//!    recognized by its program (`Spotify.exe`), its bundle (`com.spotify.client`), its desktop id
//!    (`com.spotify.Client.desktop`) or the plain name typed in the slice (`Spotify`).
//!
//! An app that is in neither source is assumed to take a new window, which is what the badges did
//! before; a wrong "yes" costs a click on the other badge, a wrong "no" would hide a feature, so the
//! list holds only apps that are known to keep one window, or that the badge cannot ask for another
//! one (Zed: Cmd+N makes a file).

use crate::platform;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// How long to wait for a window to appear after a new one was asked for: a cold start of a large
/// app is slow, and a verdict of "it did not open" must not be a mistake of timing.
const WATCH: Duration = Duration::from_secs(5);
/// How often the windows are counted while waiting.
const WATCH_EVERY: Duration = Duration::from_millis(200);

/// Apps that keep one main window, as the words of their name. An entry matches when its words
/// appear next to each other, in that order, in the words of the app's name (see [`words`]).
const SINGLE_WINDOW: &[&[&str]] = &[
    // chat and calls
    &["spotify"],
    &["discord"],
    &["slack"],
    &["teams"],
    &["teams2"],
    &["telegram"],
    &["whatsapp"],
    &["signal"],
    &["viber"],
    &["zoom"],
    &["anydesk"],
    &["teamviewer"],
    // media and downloads
    &["itunes"],
    &["apple", "music"],
    &["qbittorrent"],
    &["transmission"],
    &["steam"],
    // secrets and sync
    &["1password"],
    &["bitwarden"],
    &["keepass"],
    &["keepassxc"],
    &["dropbox"],
    &["onedrive"],
    &["docker", "desktop"],
    // utilities of the system
    &["calculator"],
    &["calc", "exe"],
    &["system", "settings"],
    &["system", "preferences"],
    &["activity", "monitor"],
    &["task", "manager"],
    &["taskmgr"],
    &["pavucontrol"],
    // editors whose "new" shortcut makes a file, not a window (Cmd+N in Zed), so the green badge
    // can only fail; they open windows with another shortcut that the badge does not press
    &["zed"],
];

/// The words of an app's name, lower case: the last part of a path, without `.exe`, `.app`,
/// `.desktop` or `.lnk`, cut at everything that is not a letter or a digit
/// (`C:\Apps\Spotify\Spotify.exe` -> `spotify`; `com.spotify.Client.desktop` -> `com`, `spotify`,
/// `client`; `/Applications/System Settings.app` -> `system`, `settings`).
fn words(app: &str) -> Vec<String> {
    let name = app.trim().rsplit(['/', '\\']).next().unwrap_or("");
    let lower = name.to_lowercase();
    let stem = ["exe", "app", "desktop", "lnk"]
        .iter()
        .find_map(|ext| lower.strip_suffix(&format!(".{ext}")))
        .unwrap_or(&lower);
    stem.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `app` is one the built-in list knows to keep a single window.
pub fn listed_single_window(app: &str) -> bool {
    let w = words(app);
    if w.is_empty() {
        return false;
    }
    SINGLE_WINDOW.iter().any(|entry| {
        w.windows(entry.len())
            .any(|run| run.iter().eq(entry.iter()))
    })
}

/// The name an app is remembered by: its words, so `Spotify.exe`, `Spotify.app` and `Spotify` are
/// one app. Empty for a name with nothing to go by.
pub fn key(app: &str) -> String {
    words(app).join(" ")
}

/// Whether `app` (as typed in a slice) cannot open another window: what was learned from earlier
/// attempts if that is clear, otherwise what the app says or the built-in list does. Meant for the
/// worker thread: the platform may read files or ask the system.
pub fn single_window(app: &str) -> bool {
    match crate::learn::verdict(&key(app)) {
        Some(single) => single,
        None => listed_single_window(app) || platform::declares_single_window(app),
    }
}

/// What [`platform::open_app_new_window`] did, for what is learned from it ([`watch_new_window`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// The request went out. The number is how many windows the app had at that moment, counted
    /// by the platform when it can do that between bringing the app forward and asking (macOS: with
    /// the app in front, right before the key), so that what bringing it forward alone shows is
    /// not taken for the new window. `None`: the count taken before everything stands.
    Sent(Option<usize>),
    /// It did not go out (macOS: the app never came to the front, so the key would have reached
    /// another app; or it did not run). Whatever the windows do then is not the answer to the
    /// request, so nothing is learned.
    NotSent,
}

/// The windows of an app told apart one from another, as a platform that can do it reports them
/// (`platform::window_ids`): a number for each window, the same for as long as the window lives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Windows {
    /// Every top-level window the app has, shown or not (Windows: also the invisible ones; X11 and
    /// KWin: every window of the window manager's list, dialogs included). A window that was
    /// hidden and is shown again keeps its number, so being on this list is what tells it from a
    /// new one.
    pub known: Vec<u64>,
    /// The ones that count as a window of the app: the windows `platform::window_count` counts.
    pub main: Vec<u64>,
}

/// What an app had before a new window was asked of it. Where the platform can tell windows
/// apart, a window counts as new only when its number was not on the list before: a window that
/// was hidden (closed to the tray or the menu bar) and that starting the app again shows is not a
/// new window, though a count taken while it was hidden says it is. Elsewhere the count decides.
#[derive(Clone, Debug)]
pub struct Baseline {
    /// The windows that counted.
    count: usize,
    /// The numbers of every window the app had; `None` when the platform cannot say.
    known: Option<HashSet<u64>>,
}

impl Baseline {
    /// Looks at `app` now. `None` when the OS cannot count its windows, or the app does not run.
    pub fn take(app: &str) -> Option<Baseline> {
        if let Some(w) = platform::window_ids(app) {
            return Some(Baseline {
                count: w.main.len(),
                known: Some(w.known.into_iter().chain(w.main).collect()),
            });
        }
        platform::window_count(app).map(|count| Baseline { count, known: None })
    }

    /// Takes `settled`, the count the platform made right before it asked (see [`Asked::Sent`]),
    /// for the count; windows told apart by number need no such count.
    pub fn settled(mut self, settled: Option<usize>) -> Baseline {
        if self.known.is_none()
            && let Some(n) = settled
        {
            self.count = n;
        }
        self
    }

    /// The windows `app` has now, in the terms of [`judge`]: more than before exactly when a window
    /// that was not known appeared.
    fn sample(&self, app: &str) -> Option<usize> {
        let Some(known) = &self.known else {
            return platform::window_count(app);
        };
        let now = platform::window_ids(app)?;
        Some(windows_now(self.count, known, &now.main))
    }
}

/// The windows an app has, for [`judge`], given that it had `count` windows and knew `known`:
/// `count` plus the windows of `main` that were not known, and `0` when it has none left.
fn windows_now(count: usize, known: &HashSet<u64>, main: &[u64]) -> usize {
    if main.is_empty() {
        return 0;
    }
    count + main.iter().filter(|id| !known.contains(id)).count()
}

/// How many samples in a row must show more windows than before: a splash screen, a notice or a
/// dialog that comes and goes within a sample (about [`WATCH_EVERY`]) is not a new window.
const STABLE_SAMPLES: u32 = 2;

/// What a new window of an app did, from the number of windows it had before (`before`) and what
/// `sample` reports while `wait` allows: `Some(true)` once it has had more for `STABLE_SAMPLES`
/// looks in a row (a window that vanishes again starts the count over), `Some(false)` when the time
/// is up and it never did, `None` when nothing can be concluded (the count is unknown, or the app is
/// gone: it was quit, or crashed).
pub fn judge(
    before: usize,
    mut sample: impl FnMut() -> Option<usize>,
    mut wait: impl FnMut() -> bool,
) -> Option<bool> {
    let mut more = 0;
    loop {
        match sample() {
            Some(n) if n > before => {
                more += 1;
                if more >= STABLE_SAMPLES {
                    return Some(true);
                }
            }
            Some(n) if n > 0 => more = 0,
            _ => return None,
        }
        if !wait() {
            return Some(false);
        }
    }
}

/// An "Open app" slice asked `app` for a new window while it had the windows of `before` (`None` =
/// the OS cannot count them, or the app was not running: nothing to learn). Watches on a thread of
/// its own whether one appeared and remembers the answer ([`crate::learn`]).
pub fn watch_new_window(app: &str, before: Option<Baseline>) {
    let key = key(app);
    let Some(before) = before.filter(|b| b.count > 0) else {
        return;
    };
    if key.is_empty() {
        return;
    }
    let app = app.trim().to_string();
    let _ = std::thread::Builder::new()
        .name("cursorpie-learn".into())
        .spawn(move || {
            let end = Instant::now() + WATCH;
            // the most windows seen while waiting, so a log shows what was counted (a window that
            // came and went is told from one that never came)
            let peak = std::cell::Cell::new(before.count);
            let verdict = judge(
                before.count,
                || {
                    let n = before.sample(&app);
                    if let Some(n) = n {
                        peak.set(peak.get().max(n));
                    }
                    n
                },
                || {
                    std::thread::sleep(WATCH_EVERY);
                    Instant::now() < end
                },
            );
            diag::log!(
                "run",
                "watched",
                before = before.count,
                by_number = before.known.is_some(),
                peak = peak.get(),
                verdict = ?verdict
            );
            if let Some(worked) = verdict {
                crate::learn::observe(&key, worked);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_cut_into_words_whatever_its_form() {
        assert_eq!(words("C:\\Apps\\Spotify\\Spotify.exe"), ["spotify"]);
        assert_eq!(
            words("com.spotify.Client.desktop"),
            ["com", "spotify", "client"]
        );
        assert_eq!(
            words("/Applications/System Settings.app"),
            ["system", "settings"]
        );
        assert!(words("  ").is_empty());
    }

    #[test]
    fn a_new_window_is_judged_by_the_windows_counted_afterwards() {
        let counts = |v: Vec<Option<usize>>| {
            let mut it = v.into_iter();
            move || it.next().unwrap_or(Some(1))
        };
        // appears on the third look
        let mut left = 10;
        let wait = move || {
            left -= 1;
            left > 0
        };
        assert_eq!(
            judge(1, counts(vec![Some(1), Some(1), Some(2), Some(2)]), wait),
            Some(true)
        );
        // a popup that comes and goes is not a window
        let mut left = 4;
        let wait = move || {
            left -= 1;
            left > 0
        };
        assert_eq!(
            judge(1, counts(vec![Some(2), Some(1), Some(2), Some(1)]), wait),
            Some(false),
            "never stayed for two looks in a row"
        );
        // never appears: the time runs out
        let mut left = 3;
        let wait = move || {
            left -= 1;
            left > 0
        };
        assert_eq!(judge(1, counts(vec![]), wait), Some(false));
        // the count is unknown or the app is gone: nothing is learned
        assert_eq!(judge(1, counts(vec![None]), || true), None);
        assert_eq!(judge(1, counts(vec![Some(1), Some(0)]), || true), None);
    }

    #[test]
    fn a_window_that_was_hidden_and_is_shown_again_is_not_a_new_one() {
        // the app had one visible window (7) and one closed to the tray (9)
        let known: HashSet<u64> = [7, 9].into();
        let mut wait_left = 6;
        let mut wait = move || {
            wait_left -= 1;
            wait_left > 0
        };
        // starting it again shows window 9: two windows, none of them new
        assert_eq!(windows_now(1, &known, &[7, 9]), 1);
        assert_eq!(
            judge(1, || Some(windows_now(1, &known, &[7, 9])), &mut wait),
            Some(false)
        );
        // a window nobody knew is new, however many are left
        assert_eq!(windows_now(1, &known, &[7, 12]), 2);
        assert_eq!(
            windows_now(1, &known, &[12]),
            2,
            "the old one closed, a new one came"
        );
        assert_eq!(
            judge(1, || Some(windows_now(1, &known, &[7, 12])), || true),
            Some(true)
        );
        // nothing left: the app is gone, nothing is learned
        assert_eq!(windows_now(1, &known, &[]), 0);
        assert_eq!(
            judge(1, || Some(windows_now(1, &known, &[])), || true),
            None
        );
    }

    #[test]
    fn the_key_of_an_app_is_the_same_whatever_its_form() {
        assert_eq!(key("Spotify.exe"), key("/Applications/Spotify.app"));
        assert_eq!(key("Spotify"), "spotify");
        assert_eq!(key("  "), "");
    }

    #[test]
    fn the_same_app_is_recognized_on_every_os() {
        for name in [
            "Spotify",
            "Spotify.exe",
            "C:\\Users\\a\\AppData\\Roaming\\Spotify\\Spotify.exe",
            "/Applications/Spotify.app",
            "com.spotify.client",
            "com.spotify.Client.desktop",
            "spotify",
        ] {
            assert!(listed_single_window(name), "{name}");
        }
        assert!(listed_single_window("zoom.us"));
        assert!(listed_single_window("/System/Applications/Calculator.app"));
        assert!(listed_single_window("org.gnome.Calculator"));
        assert!(listed_single_window("System Settings"));
        assert!(listed_single_window("com.apple.Music"));
        assert!(listed_single_window("Taskmgr.exe"));
        for zed in [
            "Zed",
            "Zed.app",
            "/Applications/Zed.app",
            "dev.zed.Zed",
            "zed.exe",
        ] {
            assert!(listed_single_window(zed), "{zed}");
        }
    }

    #[test]
    fn apps_that_take_windows_and_unknown_apps_keep_the_badges() {
        for name in [
            "firefox",
            "Safari",
            "chrome.exe",
            "Code",
            "org.gnome.Nautilus",
            "Notepad.exe",
            "gnome-terminal",
            "SpotifyHelper", // one word that merely starts with a listed name
            "Music Player",  // `music` alone is not listed: only Apple's
            "Organized",     // `zed` is a word of its own, not the end of another
            "",
        ] {
            assert!(!listed_single_window(name), "{name}");
        }
    }
}
