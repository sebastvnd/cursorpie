//! The browser bridge: knowing which web pages are open, and going to one.
//!
//! A browser does not tell other programs which tabs it has. This module gets them from a small
//! **extension that ships inside CursorPie** ([`extension`]): the extension reports the open
//! `http(s)` tabs and can go to one, or open a link in a new window. It is optional
//! ([`Config::browser_bridge`](crate::config::Config::browser_bridge), off by default); without it
//! an "Open URL" slice just opens its link, with no choice: CursorPie does not guess which pages
//! are open.
//!
//! ```text
//!  browser ──(extension)──native messaging──▶ host process ──loopback TCP──▶ this module
//!   tabs, windows          stdin/stdout frames   = cursorpie.exe              (in the running app)
//!                                                  started by the browser
//! ```
//!
//! * [`proto`]: the messages and the framing of both links.
//! * [`extension`]: the extension files, embedded at build time, and their export.
//! * [`install`]: registers the host with the installed browsers (a file, or a registry key).
//! * [`host`]: the process the browser starts: a relay with no window.
//! * this file: the server inside the app, the list of connected browsers and their tabs.
//!
//! # Security
//!
//! Only the browser can start the host (it names the extension that may), and the host only talks
//! to the app on `127.0.0.1`, after sending the secret of `bridge.json` (a random token, written
//! when the bridge starts, readable by the user only on Linux and macOS; the folder is the user's
//! own on Windows). The extension sends the address of `http(s)` tabs and nothing else; private
//! windows are not included unless the person allowed the extension there. The app accepts
//! commands from the extension only as data: a tab list.

pub mod extension;
pub mod host;
pub mod install;
pub mod proto;

use crate::config::is_web_url;
use crate::platform::web::{normalize, site_and_path};
use proto::{Auth, BridgeFile, FromBrowser, ToBrowser};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, mpsc};
use std::time::{Duration, Instant};

/// The longest line taken from a host: a list of tabs, with room to spare.
const MAX_LINE: u64 = proto::MAX_FROM_BROWSER as u64 + 1024;

/// One open tab.
struct Tab {
    id: i64,
    window_id: i64,
    /// [`normalize`]d address, what is compared first.
    key: String,
    /// Host (without `www.`) and path (see [`site_and_path`]), what is compared when the exact
    /// address differs by scheme, `www.`, query or `#fragment`.
    site: Option<(String, String)>,
    active: bool,
    /// The address as the browser has it.
    url: String,
}

/// A browser that is connected.
struct Client {
    /// `firefox`, `chrome`, `edge`, `brave`, `vivaldi`, `opera` (what the extension said).
    browser: String,
    tabs: Vec<Tab>,
    /// Lines for the host process of this connection.
    out: Option<mpsc::Sender<String>>,
    sock: Option<TcpStream>,
    /// When it last said something: the most recent browser is the one used for a new window.
    seen: Instant,
}

struct State {
    running: bool,
    /// Changes at every start and stop, so a server thread of an earlier run ends.
    generation: u64,
    next: u64,
    clients: BTreeMap<u64, Client>,
    /// What registering the host did at the last start.
    report: install::Report,
}

impl State {
    const fn new() -> Self {
        Self {
            running: false,
            generation: 0,
            next: 1,
            clients: BTreeMap::new(),
            report: install::Report {
                registered: Vec::new(),
                errors: Vec::new(),
            },
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Where the running app says how to reach it: `<support folder>/bridge.json`.
pub fn bridge_file_path() -> PathBuf {
    crate::config::app_support_dir().join("bridge.json")
}

/// Brings the bridge in line with the setting: starts it (server, `bridge.json`, host registered
/// with the browsers) or stops it (and removes what was written). Does nothing when it already is
/// as asked. Cheap to call after every change of the config.
pub fn apply(on: bool) {
    if on {
        start();
    } else {
        stop();
    }
}

fn start() {
    let generation = {
        let mut s = lock();
        if s.running {
            return;
        }
        s.running = true;
        s.generation += 1;
        s.generation
    };
    let listener = TcpListener::bind(("127.0.0.1", 0)).and_then(|l| {
        let port = l.local_addr()?.port();
        let token = new_token();
        write_bridge_file(port, &token)?;
        Ok((l, token))
    });
    match listener {
        Ok((listener, token)) => {
            let report = install::install_current();
            diag::log!(
                "bridge",
                "start",
                registered = report.registered.len(),
                errors = report.errors.len()
            );
            lock().report = report;
            let _ = std::thread::Builder::new()
                .name("cursorpie-bridge".into())
                .spawn(move || serve(listener, &token, generation));
        }
        Err(e) => {
            eprintln!("CursorPie: the browser bridge could not start: {e}");
            lock().running = false;
        }
    }
}

fn stop() {
    {
        let mut s = lock();
        if !s.running {
            return;
        }
        s.running = false;
        s.generation += 1;
        drop_clients(&mut s);
        s.report = install::Report::default();
    }
    let _ = std::fs::remove_file(bridge_file_path());
    install::uninstall();
}

fn drop_clients(s: &mut State) {
    for c in std::mem::take(&mut s.clients).into_values() {
        if let Some(sock) = c.sock {
            let _ = sock.shutdown(Shutdown::Both);
        }
    }
}

/// A secret for this run. The standard library's hash keys are seeded from the OS, which is all
/// that is needed for a value that only travels over the loopback and a private file.
fn new_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    (0..4u64)
        .map(|i| {
            let mut h = RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

fn write_bridge_file(port: u16, token: &str) -> std::io::Result<()> {
    let text = serde_json::to_string(&BridgeFile {
        port,
        token: token.to_string(),
    })?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    #[allow(unused_mut)] // only changed on Unix
    let mut file = opts.open(bridge_file_path())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // a file that was there already keeps its old mode otherwise
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(text.as_bytes())
}

fn still_current(generation: u64) -> bool {
    let s = lock();
    s.running && s.generation == generation
}

/// Accepts host processes until the bridge is stopped.
fn serve(listener: TcpListener, token: &str, generation: u64) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    while still_current(generation) {
        match listener.accept() {
            Ok((stream, _)) => {
                let token = token.to_string();
                let _ = std::thread::Builder::new()
                    .name("cursorpie-bridge-client".into())
                    .spawn(move || handle(stream, &token, generation));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(_) => std::thread::sleep(Duration::from_secs(1)),
        }
    }
}

/// One host process: checks its secret, then reads what the browser says until it goes away.
fn handle(stream: TcpStream, token: &str, generation: u64) {
    // a stream accepted from a non-blocking listener may inherit that mode
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let (Ok(read_half), Ok(mut write_half), Ok(sock)) =
        (stream.try_clone(), stream.try_clone(), stream.try_clone())
    else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader.by_ref().take(4096).read_line(&mut line).is_err() {
        return;
    }
    let authorized = serde_json::from_str::<Auth>(line.trim()).is_ok_and(|a| a.auth == token);
    if !authorized || write_half.write_all(b"{\"ok\":true}\n").is_err() {
        return; // the stream is closed when it goes out of scope
    }
    let _ = stream.set_read_timeout(None);

    let (tx, rx) = mpsc::channel::<String>();
    let _ = std::thread::Builder::new()
        .name("cursorpie-bridge-writer".into())
        .spawn(move || {
            for msg in rx {
                let sent = write_half
                    .write_all(msg.as_bytes())
                    .and_then(|()| write_half.write_all(b"\n"))
                    .and_then(|()| write_half.flush());
                if sent.is_err() {
                    break;
                }
            }
        });
    let id = {
        let mut s = lock();
        if !s.running || s.generation != generation {
            return;
        }
        let id = s.next;
        s.next += 1;
        s.clients.insert(
            id,
            Client {
                browser: "browser".into(),
                tabs: Vec::new(),
                out: Some(tx),
                sock: Some(sock),
                seen: Instant::now(),
            },
        );
        id
    };
    loop {
        line.clear();
        let read = reader.by_ref().take(MAX_LINE).read_line(&mut line);
        // a line without its end is a message cut short: end the link
        if !matches!(read, Ok(n) if n > 0) || !line.ends_with('\n') {
            break;
        }
        on_line(id, &line);
    }
    lock().clients.remove(&id);
}

/// Applies one message of the extension to the client `id`.
fn on_line(id: u64, line: &str) {
    let Ok(msg) = serde_json::from_str::<FromBrowser>(line.trim()) else {
        return;
    };
    let mut s = lock();
    let Some(client) = s.clients.get_mut(&id) else {
        return;
    };
    apply_message(client, msg);
}

fn apply_message(client: &mut Client, msg: FromBrowser) {
    client.seen = Instant::now();
    match msg {
        FromBrowser::Hello { browser, .. } => client.browser = browser,
        FromBrowser::Tabs { tabs } => {
            client.tabs = tabs
                .into_iter()
                .filter(|t| is_web_url(&t.url))
                .map(|t| Tab {
                    key: normalize(&t.url),
                    site: site_and_path(&t.url),
                    active: t.active,
                    id: t.id,
                    window_id: t.window_id,
                    url: t.url,
                })
                .collect();
        }
    }
}

fn send(client: &Client, msg: &ToBrowser) -> bool {
    match (serde_json::to_string(msg), &client.out) {
        (Ok(text), Some(out)) => out.send(text).is_ok(),
        _ => false,
    }
}

/// How well a tab is "the page of" a link: 3 the same address, 2 the same site and path (the
/// scheme, `www.`, query or `#fragment` differ: a page you reached by a redirect, or with a
/// tracking parameter), 1 the link is a whole site (`https://youtube.com`) and the tab is on that
/// site, 0 not the page.
fn match_level(link: &str, link_site: Option<&(String, String)>, tab: &Tab) -> u8 {
    if tab.key == normalize(link) {
        return 3;
    }
    let (Some((lh, lp)), Some((th, tp))) = (link_site, tab.site.as_ref()) else {
        return 0;
    };
    if lh != th {
        0
    } else if lp == tp {
        2
    } else if lp.is_empty() {
        1
    } else {
        0
    }
}

/// The tab that is the page of `url`, with the browser that has it: the best match (see
/// [`match_level`]), then the tab that is showing, then the browser heard from last.
fn find_tab<'a>(s: &'a State, url: &str) -> Option<(&'a Client, &'a Tab)> {
    let link_site = site_and_path(url);
    s.clients
        .values()
        .flat_map(|c| c.tabs.iter().map(move |t| (c, t)))
        .map(|(c, t)| (match_level(url, link_site.as_ref(), t), c, t))
        .filter(|(level, _, _)| *level > 0)
        .max_by_key(|(level, c, t)| (*level, t.active, c.seen))
        .map(|(_, c, t)| (c, t))
}

/// For each of `urls`: whether some connected browser has it open. `None` when the bridge is off
/// or no browser is connected, which tells the caller to use its own way of knowing.
pub fn urls_open(urls: &[String]) -> Option<Vec<bool>> {
    let s = lock();
    if !s.running || s.clients.is_empty() {
        return None;
    }
    Some(urls.iter().map(|u| find_tab(&s, u).is_some()).collect())
}

/// Asks the browser that has `url` open to go to that tab and focus its window. Returns the name
/// of that browser (`firefox`, `chrome`...), or `None` when no connected browser has it.
pub fn activate(url: &str) -> Option<String> {
    let s = lock();
    if !s.running {
        return None;
    }
    let (client, tab) = find_tab(&s, url)?;
    let ok = send(
        client,
        &ToBrowser::Activate {
            tab_id: tab.id,
            window_id: tab.window_id,
            url: tab.url.clone(),
        },
    );
    ok.then(|| client.browser.clone())
}

/// Asks the browser heard from last to open `url` in a new window. `false` when there is none.
pub fn open_new_window(url: &str) -> bool {
    let s = lock();
    if !s.running {
        return false;
    }
    let Some(client) = s.clients.values().max_by_key(|c| c.seen) else {
        return false;
    };
    send(
        client,
        &ToBrowser::Open {
            url: url.to_string(),
            new_window: true,
        },
    )
}

/// What the Settings window shows about the bridge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub running: bool,
    /// Browsers the host is registered with.
    pub registered: Vec<&'static str>,
    /// What went wrong when registering.
    pub errors: Vec<String>,
    /// Connected browsers and how many tabs each has open.
    pub browsers: Vec<(String, usize)>,
}

/// The state of the bridge, cheap enough to ask at every frame.
pub fn status() -> Status {
    let s = lock();
    Status {
        running: s.running,
        registered: s.report.registered.clone(),
        errors: s.report.errors.clone(),
        browsers: s
            .clients
            .values()
            .map(|c| (browser_label(&c.browser).to_string(), c.tabs.len()))
            .collect(),
    }
}

/// How a browser's name from the extension is written for a person.
pub fn browser_label(raw: &str) -> &str {
    match raw {
        "firefox" => "Firefox",
        "chrome" => "Chrome",
        "edge" => "Edge",
        "brave" => "Brave",
        "vivaldi" => "Vivaldi",
        "opera" => "Opera",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(browser: &str, urls: &[&str], seen: Instant) -> Client {
        let mut c = Client {
            browser: browser.into(),
            tabs: Vec::new(),
            out: None,
            sock: None,
            seen,
        };
        let tabs = urls
            .iter()
            .enumerate()
            .map(|(i, u)| proto::WireTab {
                id: i as i64 + 1,
                window_id: 10,
                url: (*u).into(),
                active: false,
            })
            .collect();
        apply_message(&mut c, FromBrowser::Tabs { tabs });
        c
    }

    fn state_with(clients: Vec<Client>) -> State {
        let mut s = State::new();
        s.running = true;
        for (i, c) in clients.into_iter().enumerate() {
            s.clients.insert(i as u64 + 1, c);
        }
        s
    }

    #[test]
    fn only_web_tabs_are_kept_and_compared_by_their_normal_form() {
        let c = client(
            "firefox",
            &[
                "https://Example.com/a/",
                "about:blank",
                "file:///x",
                "http://b.c",
            ],
            Instant::now(),
        );
        assert_eq!(c.tabs.len(), 2);
        let s = state_with(vec![c]);
        assert!(find_tab(&s, "https://example.com/a#top").is_some());
        assert!(find_tab(&s, "https://example.com/b").is_none());
    }

    #[test]
    fn the_browser_heard_from_last_wins_when_two_have_the_page() {
        let t0 = Instant::now();
        let old = client("firefox", &["https://a.b/"], t0);
        let new = client("chrome", &["https://a.b/"], t0 + Duration::from_secs(5));
        let s = state_with(vec![new, old]);
        let (c, t) = find_tab(&s, "https://a.b").unwrap();
        assert_eq!(c.browser, "chrome");
        assert_eq!(t.url, "https://a.b/");
    }

    #[test]
    fn a_page_reached_by_a_redirect_or_with_a_query_is_still_the_page() {
        let s = state_with(vec![client(
            "chrome",
            &[
                "https://www.youtube.com/watch?v=abc",
                "https://github.com/other/repo",
                "http://news.example/story/",
            ],
            Instant::now(),
        )]);
        // the link was typed without www, and the tab has a query
        assert!(find_tab(&s, "https://youtube.com/watch").is_some());
        // a whole site matches any page of it
        assert!(find_tab(&s, "https://youtube.com").is_some());
        assert!(find_tab(&s, "https://github.com").is_some());
        // another page of the site is not the page, when the link names one
        assert!(find_tab(&s, "https://github.com/foo/bar").is_none());
        // http and https are the same page
        assert!(find_tab(&s, "https://news.example/story").is_some());
        assert!(find_tab(&s, "https://other.example").is_none());
    }

    #[test]
    fn the_same_address_beats_a_page_of_the_same_site_and_the_showing_tab_wins_a_tie() {
        let mut c = client(
            "firefox",
            &["https://a.b/x?q=1", "https://a.b/x", "https://a.b/y"],
            Instant::now(),
        );
        c.tabs[2].active = true;
        let s = state_with(vec![c]);
        // exact address (tab 2) over the same page with a query (tab 1)
        assert_eq!(find_tab(&s, "https://a.b/x").unwrap().1.id, 2);
        // a bare site: every tab matches, the one that is showing wins
        assert_eq!(find_tab(&s, "https://a.b").unwrap().1.id, 3);
    }

    #[test]
    fn a_hello_names_the_browser() {
        let mut c = client("browser", &[], Instant::now());
        apply_message(
            &mut c,
            FromBrowser::Hello {
                browser: "brave".into(),
                version: String::new(),
            },
        );
        assert_eq!(c.browser, "brave");
        assert_eq!(browser_label("brave"), "Brave");
        assert_eq!(browser_label("zen"), "zen");
    }

    #[test]
    fn a_command_goes_to_the_writer_of_the_connection() {
        let (tx, rx) = mpsc::channel();
        let mut c = client("firefox", &["https://a.b/"], Instant::now());
        c.out = Some(tx);
        let s = state_with(vec![c]);
        let (c, t) = find_tab(&s, "https://a.b").unwrap();
        assert!(send(
            c,
            &ToBrowser::Activate {
                tab_id: t.id,
                window_id: t.window_id,
                url: t.url.clone()
            }
        ));
        let text = rx.recv().unwrap();
        assert!(text.contains(r#""type":"activate""#) && text.contains(r#""tabId":1"#));
    }

    #[test]
    fn tokens_differ_and_are_long_enough() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
