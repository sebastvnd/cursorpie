//! The icons of the sites the intelligent pie suggests.
//!
//! A site is learned by its address, and an address tells the eye nothing: seven sites in a row
//! would all wear the same globe. So every site that can be suggested gets the icon the site itself
//! offers, found by the same lookup as *Icon from a website* ([`webicon`]), made to look like the
//! other icons (the style the person chose there is reused) and kept as a file,
//! `icons/site-<host>.png`, next to the icons the person picked.
//!
//! # Where it runs
//!
//! * [`icon_for`] is a `stat` and a string: it answers *the file, if it exists*. A suggestion never
//!   waits for a download; until the icon is there the slice shows the globe.
//! * [`want`] starts the downloads, on a thread of their own and a few sites at a time, so neither
//!   the worker of [`collect`](super::collect) (the next press must not wait for a slow site) nor
//!   the UI is held. It is asked after every press, so the icons arrive within a few presses.
//!   A site that gave nothing is not asked again for [`RETRY_AFTER`].
//! * [`forget_all`] deletes the files (*Forget everything*).
//!
//! What goes out: the request for the home page and for the icon files of that one site, which the
//! person visits anyway, and, only when the site offers no usable icon, the host name to Google's
//! favicon service (see [`webicon`]). It is off with [`SmartConfig::site_icons`].

use super::habits::{Habits, page_of};
use crate::config::{SmartConfig, app_support_dir, tilde};
use crate::ui::webicon;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Every file of this module starts with this, so *Forget everything* knows which ones are ours.
pub const PREFIX: &str = "site-";
/// How long a site that gave no icon is left alone.
pub const RETRY_AFTER: Duration = Duration::from_secs(6 * 3600);
/// How many sites one run of the downloader asks.
const BATCH: usize = 6;

/// The name of the file that holds the icon of `host`: `github.com` is `site-github-com.png`.
pub fn file_name(host: &str) -> String {
    let safe: String = host
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{PREFIX}{safe}.png")
}

/// Where the icon of `host` is (or will be) kept.
pub fn file_for(host: &str) -> PathBuf {
    app_support_dir().join("icons").join(file_name(host))
}

/// The icon string (`~/...png`, see [`crate::config::image_path`]) of `host`, when its file is
/// there.
pub fn icon_for(host: &str) -> Option<String> {
    let path = file_for(host);
    path.is_file().then(|| tilde(&path))
}

/// The sites worth having an icon for, the open tabs first, then the sites that were in view the
/// longest, at most `limit` of them: bare host names without `www.`, none of them on the ignore
/// list. Deterministic, so the same habits always ask for the same sites.
pub fn hosts(
    habits: &Habits,
    tabs: &[(String, bool)],
    c: &SmartConfig,
    limit: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let add = |host: &str, out: &mut Vec<String>| {
        let host = host.trim().to_ascii_lowercase();
        if !host.is_empty() && !c.ignores_site(&host) && !out.contains(&host) {
            out.push(host);
        }
    };
    for (url, _) in tabs {
        if let Some((_, _, site)) = page_of(url, false) {
            add(&site, &mut out);
        }
    }
    let mut pages: Vec<(&str, f32)> = habits
        .pages
        .values()
        .map(|p| (p.label.as_str(), p.secs))
        .collect();
    pages.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (label, _) in pages {
        add(label, &mut out);
    }
    out.truncate(limit);
    out
}

/// `host` and the icon string of each of the `hosts` that already has a file.
pub fn lookup(
    habits: &Habits,
    tabs: &[(String, bool)],
    c: &SmartConfig,
) -> HashMap<String, String> {
    hosts(habits, tabs, c, 64)
        .into_iter()
        .filter_map(|h| icon_for(&h).map(|icon| (h, icon)))
        .collect()
}

static BUSY: AtomicBool = AtomicBool::new(false);
static TRIED: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();

fn tried() -> std::sync::MutexGuard<'static, HashMap<String, Instant>> {
    TRIED
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// Asks for the icons of `hosts` that have none yet. Returns at once: the downloads run on a thread
/// of their own, `BATCH` sites at a time, and only one such thread runs; what it leaves is asked
/// again at the next press.
pub fn want(hosts: &[String]) {
    let mut todo: Vec<String> = Vec::new();
    {
        let mut seen = tried();
        for h in hosts {
            let h = h.trim().to_ascii_lowercase();
            if h.is_empty() || todo.contains(&h) || file_for(&h).is_file() {
                continue;
            }
            if seen.get(&h).is_some_and(|at| at.elapsed() < RETRY_AFTER) {
                continue;
            }
            todo.push(h);
            if todo.len() == BATCH {
                break;
            }
        }
        if todo.is_empty() || BUSY.swap(true, Ordering::SeqCst) {
            return;
        }
        let now = Instant::now();
        for h in &todo {
            seen.insert(h.clone(), now);
        }
    }
    let spawned = std::thread::Builder::new()
        .name("cursorpie-siteicons".into())
        .spawn(move || {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fetch_all(&todo)));
            BUSY.store(false, Ordering::SeqCst);
        });
    if spawned.is_err() {
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Downloads, restyles and saves the icon of each of `hosts`. Slow: only [`want`]'s thread calls it.
fn fetch_all(hosts: &[String]) {
    let cancel = AtomicBool::new(false);
    let stage = webicon::Stage::default();
    let style = webicon::load_style();
    let mut saved = 0usize;
    for host in hosts {
        let Ok(found) = webicon::fetch(host, &stage, &cancel) else {
            continue;
        };
        let mut style = style;
        style.knock_out = webicon::has_neutral_background(&found.image);
        let img = webicon::harmonize(&found.image, &style);
        let path = file_for(host);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // written aside and moved over, so the pie never reads half a picture
        let tmp = path.with_extension("png.part");
        if img.save_with_format(&tmp, image::ImageFormat::Png).is_ok()
            && std::fs::rename(&tmp, &path).is_ok()
        {
            saved += 1;
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    diag::log!("smart", "site_icons", asked = hosts.len(), saved = saved);
}

/// Deletes the icons of the sites and forgets which ones were asked (*Forget everything*).
pub fn forget_all() {
    tried().clear();
    let Ok(dir) = std::fs::read_dir(app_support_dir().join("icons")) else {
        return;
    };
    for entry in dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(PREFIX) && name.ends_with(".png") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_700_000_000;

    #[test]
    fn a_host_makes_a_safe_file_name() {
        assert_eq!(file_name("github.com"), "site-github-com.png");
        assert_eq!(file_name(" Docs.RS "), "site-docs-rs.png");
        assert_eq!(file_name("../x"), "site----x.png");
        assert!(!file_name("a/b\\c").contains(['/', '\\']));
    }

    #[test]
    fn sites_are_asked_open_tabs_first_then_the_longest_in_view() {
        let mut h = Habits::default();
        for (site, secs) in [
            ("a.example", 10.0),
            ("b.example", 900.0),
            ("c.example", 50.0),
        ] {
            h.see_page(site, &format!("https://{site}"), site, secs, true, T0, 30.0);
        }
        let c = SmartConfig::default();
        let tabs = vec![("https://www.open.example/x".to_string(), false)];
        let got = hosts(&h, &tabs, &c, 3);
        assert_eq!(got, ["open.example", "b.example", "c.example"]);
    }

    #[test]
    fn an_ignored_site_is_never_asked() {
        let mut h = Habits::default();
        for (site, secs) in [("bank.example", 99.0), ("news.example", 5.0)] {
            h.see_page(site, &format!("https://{site}"), site, secs, true, T0, 30.0);
        }
        let c = SmartConfig {
            ignore_sites: vec!["bank.example".into()],
            ..SmartConfig::default()
        };
        assert_eq!(hosts(&h, &[], &c, 10), ["news.example"]);
    }
}
