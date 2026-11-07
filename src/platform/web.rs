//! Web pages for the "Open URL" slice's choice (see [`running`](crate::running)): which pages are
//! open, a new browser window, and going to the page that is open.
//!
//! A browser does not tell other programs which pages it has open, and guessing (for example from
//! "CursorPie opened that link earlier") opens the same page twice as often as it helps. So the
//! choice for a web link exists **only with the browser extension** that ships with CursorPie
//! ([`bridge`](crate::bridge), switched on in Settings > Behavior > Browser extension): the page is
//! open when a tab of a connected browser has it, the choice goes to that very tab, and a new window
//! is a real new window on every OS. Without the extension a web link is simply opened, as it
//! always was, and its slice shows no badges.
//!
//! This module has no backend of its own: it is built from the functions every backend already
//! provides (`open_url`, `running_among`...), plus two Linux-only helpers. It is the same code on
//! every OS, which keeps the list of functions a backend must have (the table in
//! [`platform`](crate::platform)) as it was.

use crate::config::DesktopMode;

/// A link as it is compared: trimmed, scheme and host in lower case, the `#fragment` and a trailing
/// `/` left out, so `HTTPS://Example.com/a/#top` and `https://example.com/a` are the same page.
pub fn normalize(url: &str) -> String {
    let t = url.trim();
    let t = t.split('#').next().unwrap_or(t);
    let Some((scheme, rest)) = t.split_once("://") else {
        return t.to_string();
    };
    let (host, path) = match rest.find(['/', '?']) {
        Some(i) => rest.split_at(i),
        None => (rest, ""),
    };
    let path = if path == "/" { "" } else { path };
    let path = path.strip_suffix('/').unwrap_or(path);
    format!(
        "{}://{}{}",
        scheme.to_ascii_lowercase(),
        host.to_ascii_lowercase(),
        path
    )
}

/// The site and the page of a link, without what changes between two visits to "the same page":
/// the host in lower case without a leading `www.`, and the path without a trailing `/`. The scheme
/// (`http` or `https`), the query (`?...`) and the `#fragment` are left out. `None` when `url` has
/// no `scheme://host`.
pub fn site_and_path(url: &str) -> Option<(String, String)> {
    let t = url.trim();
    let t = t.split('#').next().unwrap_or(t);
    let (_, rest) = t.split_once("://")?;
    let end = rest.find(['/', '?']).unwrap_or(rest.len());
    let (host, after) = rest.split_at(end);
    // user:password@host:port -> host:port
    let host = host.rsplit('@').next().unwrap_or(host).to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host).to_string();
    if host.is_empty() {
        return None;
    }
    let path = after.split('?').next().unwrap_or("");
    let path = path.strip_suffix('/').unwrap_or(path);
    Some((host, path.to_string()))
}

/// For each of `urls` (web links as typed in "Open URL" slices), whether a tab of a connected
/// browser has it open. All `false` when the browser extension is off or no browser is connected.
pub fn urls_open(urls: &[String]) -> Vec<bool> {
    crate::bridge::urls_open(urls).unwrap_or_else(|| vec![false; urls.len()])
}

/// Opens `url` in a new browser window: through the extension when a browser is connected,
/// otherwise as well as the OS can (Linux: the default browser with `--new-window` when it is a
/// known one; elsewhere the link is opened as usual).
pub fn open_new_window(url: &str) {
    if crate::bridge::open_new_window(url) {
        return;
    }
    #[cfg(target_os = "linux")]
    super::linux::backend::open_url_new_window(url);
    #[cfg(not(target_os = "linux"))]
    super::open_url(url);
}

/// Goes to the tab that has the page `url`; opens the link as usual when no connected browser has
/// it (the tab was closed since). `mode` is the desktop mode of the settings (kept for the
/// signature of the other "reveal" functions).
pub fn reveal(url: &str, mode: DesktopMode) {
    let _ = mode;
    // the extension selects the tab and focuses its window...
    if let Some(browser) = crate::bridge::activate(url) {
        // ...and where a program cannot take the focus for a window (a Wayland session), the
        // window manager is asked to raise the browser too
        #[cfg(target_os = "linux")]
        for id in linux_app_ids(&browser) {
            if super::linux::backend::raise_if_running(id) {
                break;
            }
        }
        let _ = browser;
        return;
    }
    super::open_url(url);
}

/// The desktop ids a browser (as the extension names it) can have on Linux, in the order to try.
/// The window manager knows the browser by its desktop id; the extension only says which browser it
/// is.
pub fn linux_app_ids(browser: &str) -> &'static [&'static str] {
    match browser {
        "firefox" => &["firefox", "org.mozilla.firefox", "firefox-esr", "librewolf"],
        "chrome" => &[
            "google-chrome",
            "google-chrome-stable",
            "com.google.Chrome",
            "chromium",
            "chromium-browser",
            "org.chromium.Chromium",
        ],
        "edge" => &["microsoft-edge", "microsoft-edge-stable"],
        "brave" => &["brave-browser", "brave-browser-stable", "com.brave.Browser"],
        "vivaldi" => &["vivaldi-stable", "vivaldi"],
        "opera" => &["opera"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_page_is_written_in_one_way() {
        let same = [
            "https://example.com",
            "https://example.com/",
            " HTTPS://Example.COM/ ",
            "https://example.com/#top",
        ];
        for s in same {
            assert_eq!(normalize(s), "https://example.com", "{s}");
        }
        assert_eq!(normalize("https://a.b/c/"), "https://a.b/c");
        assert_eq!(normalize("https://a.b/C?q=1"), "https://a.b/C?q=1");
        assert_ne!(normalize("https://a.b/c"), normalize("https://a.b/d"));
        assert_ne!(normalize("https://a.b/?q=1"), normalize("https://a.b/?q=2"));
        assert_eq!(normalize("/home/u/a.txt"), "/home/u/a.txt");
    }

    #[test]
    fn the_site_and_page_ignore_scheme_www_query_and_fragment() {
        let want = Some(("youtube.com".to_string(), "/watch".to_string()));
        for s in [
            "https://www.youtube.com/watch?v=abc#t=5",
            "http://YouTube.com/watch/",
            " https://user:pw@www.youtube.com/watch ",
        ] {
            assert_eq!(site_and_path(s), want, "{s}");
        }
        assert_eq!(
            site_and_path("https://example.com"),
            Some(("example.com".into(), String::new()))
        );
        assert_eq!(
            site_and_path("https://example.com/?a=1"),
            Some(("example.com".into(), String::new()))
        );
        assert_eq!(
            site_and_path("https://localhost:3000/app"),
            Some(("localhost:3000".into(), "/app".into()))
        );
        assert_eq!(site_and_path("/home/u/a.txt"), None);
        assert_eq!(site_and_path("https:///x"), None);
    }

    #[test]
    fn every_browser_of_the_extension_has_desktop_ids() {
        for b in ["firefox", "chrome", "edge", "brave", "vivaldi", "opera"] {
            assert!(!linux_app_ids(b).is_empty(), "{b}");
        }
        assert!(linux_app_ids("netscape").is_empty());
    }

    #[test]
    fn nothing_is_open_without_the_extension() {
        // the bridge is not running in a test: no guessing
        assert_eq!(
            urls_open(&["https://a.b".to_string(), "https://c.d".to_string()]),
            [false, false]
        );
        assert_eq!(urls_open(&[]), Vec::<bool>::new());
    }
}
