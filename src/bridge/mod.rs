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
//! * [`commands`]: what a "Browser" action can ask the browser to do (`media.next`, `tab.close`,
//!   `chat.next_unread`...), see `docs/BROWSER_EXTENSION.md`.
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
//! own on Windows). The extension sends the address of `http(s)` tabs, and how many tabs make
//! sound and how many chats wait (numbers only), and the answer to a command; private windows are
//! not included unless the person allowed the extension there. The app takes all of it only as
//! data. What the app asks of the browser is limited to [`commands::CATALOG`]: a line that names
//! anything else is never sent.

pub mod commands;
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
use std::path::{Path, PathBuf};
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
    /// The version of the extension (it is the version of the app that exported it).
    version: String,
    /// The commands the extension knows. Empty: an extension from before commands existed.
    commands: Vec<String>,
    state: Activity,
}

/// What the extension last said about sound and chats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Activity {
    audible: u32,
    media_tabs: u32,
    /// Open chat sites (`whatsapp`, `telegram`) and their unread number when known.
    chats: Vec<(String, Option<u32>)>,
}

struct State {
    running: bool,
    /// Changes at every start and stop, so a server thread of an earlier run ends.
    generation: u64,
    next: u64,
    /// The `id` of the next command sent to a browser.
    next_command: u64,
    clients: BTreeMap<u64, Client>,
    /// What registering the host did at the last start.
    report: install::Report,
    /// Commands somebody waits for the answer of (a *System / Browser command* block of a flow),
    /// by the `id` they were sent with. A command nobody waits for is not in here: its answer is
    /// shown on the screen instead (see [`on_line`]).
    waiting: BTreeMap<u64, Waiter>,
    /// How many times each browser (`chrome`, `firefox`...) was asked to reload its extension in this
    /// run, so an extension that keeps coming back old (loaded from another folder) is not asked for
    /// ever (see [`reload_wanted`]).
    reloads: BTreeMap<String, u8>,
}

/// Who waits for the answer of a command, and through which connection it has to come.
struct Waiter {
    /// The connection the command was sent to: no other one may answer it.
    client: u64,
    tx: mpsc::Sender<Reply>,
}

/// What a browser said about a command it ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reply {
    /// Whether it worked.
    pub ok: bool,
    /// The words of the browser: what went wrong, or the answer to a question (`tab.url`,
    /// `media.state`...). Empty for a command that has nothing to say.
    pub message: String,
}

impl State {
    const fn new() -> Self {
        Self {
            running: false,
            generation: 0,
            next: 1,
            next_command: 1,
            clients: BTreeMap::new(),
            report: install::Report {
                registered: Vec::new(),
                errors: Vec::new(),
            },
            waiting: BTreeMap::new(),
            reloads: BTreeMap::new(),
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
    // dropping the senders ends every wait: nobody is going to answer now
    s.waiting.clear();
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
                version: String::new(),
                commands: Vec::new(),
                state: Activity::default(),
            },
        );
        // Ask the extension for hello + tabs right away: when the app starts after the browser the
        // extension has nothing to prompt it, and the list would stay empty until a tab is clicked.
        if let Some(client) = s.clients.get(&id) {
            send(client, &ToBrowser::Sync);
        }
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
    forget_client(&mut lock(), id);
}

/// Removes a connection that ended. A command somebody waits for on it will not be answered, so
/// the wait ends at once (the sender is dropped) instead of at its time limit.
fn forget_client(s: &mut State, id: u64) {
    s.clients.remove(&id);
    s.waiting.retain(|_, w| w.client != id);
}

/// Applies one message of the extension to the client `id`.
fn on_line(id: u64, line: &str) {
    let Ok(msg) = serde_json::from_str::<FromBrowser>(line.trim()) else {
        return;
    };
    // panels: answered on a thread of their own, because the person takes their time
    let msg = match msg {
        FromBrowser::Panel { req, spec } => {
            panel_request(id, req, spec);
            return;
        }
        FromBrowser::PanelUpdate { panel, patch } => {
            if let Some(browser) = browser_of(id) {
                crate::panel::patch(&panel, patch, crate::panel::Source::Extension(browser));
            }
            return;
        }
        FromBrowser::PanelClose { panel } => {
            if let Some(browser) = browser_of(id) {
                crate::panel::close(&panel, crate::panel::Source::Extension(browser));
            }
            return;
        }
        other => other,
    };
    if let FromBrowser::Outcome {
        id: command,
        ok,
        message,
        notify,
    } = msg
    {
        diag::log!("bridge", "result", seq = command, ok = ok);
        let (known, waiter) = {
            let mut s = lock();
            let known = s.clients.contains_key(&id);
            // only the connection a command was sent to can answer it
            let mine = s.waiting.get(&command).is_some_and(|w| w.client == id);
            let waiter = if known && mine {
                s.waiting.remove(&command)
            } else {
                None
            };
            (known, waiter)
        };
        if let Some(w) = waiter {
            // a flow waits for this: it gets the words, and the screen stays quiet
            let _ = w.tx.send(Reply {
                ok,
                message: message.trim().to_string(),
            });
        } else if known {
            show_result(ok, &message, notify);
        }
        return;
    }
    let hello = matches!(msg, FromBrowser::Hello { .. });
    {
        let mut s = lock();
        let Some(client) = s.clients.get_mut(&id) else {
            return;
        };
        apply_message(client, msg);
    }
    if hello {
        ask_to_reload_in(id, &extension::default_dir());
    }
}

/// Most times one browser is asked to reload its extension in one run of the app.
const MAX_RELOAD_ASKS: u8 = 3;

/// Whether an extension that has just said hello should be asked to reload itself: it is not the
/// version of this build, the folder it was most likely loaded from now holds this build's files
/// (`files`, see [`extension::exported_version`]), so the reload makes it current, and it was not
/// asked too often already (an extension loaded from another folder stays old however often it
/// reloads). One that did not say its version is too old to understand the request.
fn reload_wanted(extension: &str, build: &str, files: Option<&str>, asked: u8) -> bool {
    !extension.is_empty() && extension != build && files == Some(build) && asked < MAX_RELOAD_ASKS
}

/// After an update the app writes the new extension files into the folder it exported to (see
/// [`crate::refresh`]), but a browser keeps running the old code until the extension is reloaded.
/// So when an extension says hello with another version than this build and `dir` holds the files of
/// this build, it is asked to reload itself: no trip to the extensions page.
fn ask_to_reload_in(id: u64, dir: &Path) {
    let build = env!("CARGO_PKG_VERSION");
    let (browser, version) = {
        let s = lock();
        match s.clients.get(&id) {
            Some(c) => (c.browser.clone(), c.version.clone()),
            None => return,
        }
    };
    // read the folder with the lock released
    let files = extension::exported_version(dir, &browser);
    let mut s = lock();
    let asked = s.reloads.get(&browser).copied().unwrap_or(0);
    if !reload_wanted(&version, build, files.as_deref(), asked) {
        return;
    }
    let sent = match s.clients.get(&id) {
        Some(client) => send(
            client,
            &ToBrowser::Reload {
                version: build.to_string(),
            },
        ),
        None => false,
    };
    if sent {
        s.reloads.insert(browser, asked + 1);
        diag::log!("bridge", "reload", asked = u64::from(asked) + 1);
    }
}

/// The browser of the connection `id` (`firefox`, `chrome`...), while it is connected.
fn browser_of(id: u64) -> Option<String> {
    lock().clients.get(&id).map(|c| c.browser.clone())
}

/// The longest an extension's panel waits for an answer, in seconds. A flow may wait longer; an
/// extension is not asked to hold the window for ever.
const EXTENSION_PANEL_MAX_S: u32 = 300;

/// The extension asks for a panel: refused at once when panels from the extension are off or the
/// description is not valid, otherwise shown, and the answer goes back to the same connection.
fn panel_request(client: u64, req: u64, spec: serde_json::Value) {
    use crate::panel::{PanelResult, PanelSpec, Source};
    let Some(browser) = browser_of(client) else {
        return;
    };
    let source = Source::Extension(browser);
    let reply = move |result: PanelResult| {
        let msg = ToBrowser::PanelResult {
            req,
            result: result.to_json(),
        };
        if let Some(c) = lock().clients.get(&client) {
            let _ = send(c, &msg);
        }
    };
    if let Err(why) = crate::panel::broker::allowed(&source) {
        reply(PanelResult::unavailable("", why));
        return;
    }
    let mut spec = match PanelSpec::from_value(spec) {
        Ok(s) => s,
        Err(e) => {
            reply(PanelResult::unavailable("", e.to_string()));
            return;
        }
    };
    let wait = match spec.timeout_s {
        Some(s) if s > 0 => s.min(EXTENSION_PANEL_MAX_S),
        _ => EXTENSION_PANEL_MAX_S,
    };
    spec.timeout_s = Some(wait);
    diag::log!("bridge", "panel", req = req, widgets = spec.widgets.len());
    crate::panel::ask_in_background(spec, source, reply);
}

/// Shows the person what a command said, when the extension asked for it (it follows the
/// person's option "Notifications" of the extension).
fn show_result(ok: bool, message: &str, notify: bool) {
    let text = message.trim();
    if !notify || text.is_empty() {
        return;
    }
    let text: String = text.chars().take(300).collect();
    let title = if ok {
        "CursorPie"
    } else {
        "CursorPie: browser"
    };
    // starting the program that shows it can take a moment: not on the thread that reads the browser
    let _ = std::thread::Builder::new()
        .name("cursorpie-bridge-notify".into())
        .spawn(move || {
            crate::platform::native::notify(title, &text);
        });
}

fn apply_message(client: &mut Client, msg: FromBrowser) {
    client.seen = Instant::now();
    match msg {
        FromBrowser::Hello {
            browser,
            version,
            commands,
        } => {
            client.browser = browser;
            client.version = version;
            client.commands = commands;
        }
        FromBrowser::State {
            audible,
            media_tabs,
            chats,
        } => {
            client.state = Activity {
                audible,
                media_tabs,
                chats: chats.into_iter().map(|c| (c.site, c.unread)).collect(),
            };
        }
        // answered in `on_line`, which needs more than the client
        FromBrowser::Outcome { .. }
        | FromBrowser::Panel { .. }
        | FromBrowser::PanelUpdate { .. }
        | FromBrowser::PanelClose { .. } => {}
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

/// What happened to a line of a "Browser" action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dispatch {
    /// Sent to this browser (`chrome`...). Whether it worked comes back later, as a notification.
    Sent(String),
    /// The line is not a browser command.
    Invalid(String),
    /// The bridge is switched off.
    Off,
    /// The bridge is on but no browser is connected.
    NoBrowser,
    /// A browser is connected but its extension is too old to know the command.
    Outdated(String),
}

impl Dispatch {
    /// What to tell the person when the command could not be sent; `None` when it was sent.
    pub fn problem(&self) -> Option<String> {
        match self {
            Self::Sent(_) => None,
            Self::Invalid(why) => Some(why.clone()),
            Self::Off => Some(
                "The browser extension is off: turn on \"Browser extension\" in Settings > Behavior"
                    .into(),
            ),
            Self::NoBrowser => Some(
                "No browser is connected: open the browser that has the CursorPie extension".into(),
            ),
            Self::Outdated(b) => Some(format!(
                "The CursorPie extension in {b} is too old for this: export it again from Settings and reload it"
            )),
        }
    }
}

/// How well a connected browser suits a command: the one that makes sound for a media command, the
/// one that has a chat site open for a chat command. Equal scores go to the browser heard from last.
fn suits(client: &Client, group: commands::Group) -> u8 {
    use commands::Group;
    match group {
        Group::Media if client.state.audible > 0 => 3,
        Group::Media if client.state.media_tabs > 0 => 2,
        Group::Chat if !client.state.chats.is_empty() => 3,
        _ => 1,
    }
}

/// A command that went to a browser.
struct Sent {
    /// `chrome`, `firefox`...
    browser: String,
    /// The `id` it was sent with.
    id: u64,
    /// How many seconds to wait for the browser to say how it went ([`commands::wait_secs_for`]).
    wait: u64,
}

/// Sends the line of a "Browser" action (`media.next`, `chat.send_to Anna :: hi`) to the browser
/// that suits it, and says what happened to it. Whether it worked comes back later, as a
/// notification; to wait for it use [`run_command_wait`].
pub fn run_command(spec: &str) -> Dispatch {
    match dispatch(spec, None) {
        Ok(sent) => Dispatch::Sent(sent.browser),
        Err(why) => why,
    }
}

/// The work of [`run_command`]. With a `waiter`, the answer of the browser goes to it (and not to
/// the screen).
fn dispatch(spec: &str, waiter: Option<mpsc::Sender<Reply>>) -> Result<Sent, Dispatch> {
    let (cmd, arg) = match commands::parse(spec) {
        Ok(parsed) => parsed,
        Err(e) => return Err(Dispatch::Invalid(e.to_string())),
    };
    let mut s = lock();
    if !s.running {
        return Err(Dispatch::Off);
    }
    if s.clients.is_empty() {
        return Err(Dispatch::NoBrowser);
    }
    let best = s
        .clients
        .iter()
        .filter(|(_, c)| c.commands.iter().any(|k| k == cmd.id))
        .max_by_key(|(_, c)| (suits(c, cmd.group), c.seen))
        .map(|(&k, _)| k);
    let Some(key) = best else {
        let newest = s.clients.values().max_by_key(|c| c.seen);
        let name = newest
            .map_or("the browser", |c| browser_label(&c.browser))
            .to_string();
        return Err(Dispatch::Outdated(name));
    };
    let wait = commands::wait_secs_for(cmd, &arg);
    let command = s.next_command;
    s.next_command += 1;
    let sent = send(
        &s.clients[&key],
        &ToBrowser::Command {
            id: command,
            cmd: cmd.id.to_string(),
            arg,
        },
    );
    diag::log!("bridge", "command", name = cmd.id, sent = sent);
    if !sent {
        return Err(Dispatch::NoBrowser);
    }
    // the lock is still held, so the answer cannot come before the waiter is in place
    if let Some(tx) = waiter {
        s.waiting.insert(command, Waiter { client: key, tx });
    }
    Ok(Sent {
        browser: s.clients[&key].browser.clone(),
        id: command,
        wait,
    })
}

/// How a command that was waited for ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Waited {
    /// The browser ran it and said how it went.
    Reply(Reply),
    /// It could not be sent: the extension is off, no browser is connected, the line is not a
    /// command... The words are for the person (see [`Dispatch::problem`]).
    Problem(String),
    /// It was sent but no answer came in time, or the browser went away.
    Lost(String),
}

/// Sends a command like [`run_command`] and **waits** for the browser to say how it went: what a
/// flow needs, to put the answer in a variable and decide what to do next. It waits at most
/// [`commands::wait_secs_for`] the command (a panel is given the time a person needs, and
/// `chat.wait` the time it was told to wait). Nothing is shown on the screen for the answer: the
/// flow reports a failure itself.
pub fn run_command_wait(spec: &str) -> Waited {
    let (tx, rx) = mpsc::channel();
    let sent = match dispatch(spec, Some(tx)) {
        Ok(sent) => sent,
        Err(why) => return Waited::Problem(why.problem().unwrap_or_default()),
    };
    let wait = Duration::from_secs(sent.wait);
    match rx.recv_timeout(wait) {
        Ok(reply) => Waited::Reply(reply),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            lock().waiting.remove(&sent.id);
            Waited::Lost(format!(
                "{} did not answer within {} seconds",
                browser_label(&sent.browser),
                wait.as_secs()
            ))
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Waited::Lost("the browser closed the connection before it answered".into())
        }
    }
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
    /// Tabs that make sound, over all browsers.
    pub playing: u32,
    /// Open chat sites and their unread number when known, over all browsers.
    pub chats: Vec<(String, Option<u32>)>,
    /// Browsers whose extension is not the version of this app: its files were not exported again
    /// (and reloaded) after an update, so new commands do not work there.
    pub outdated: Vec<String>,
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
        playing: s.clients.values().map(|c| c.state.audible).sum(),
        chats: {
            let mut all: Vec<(String, Option<u32>)> = Vec::new();
            for (site, n) in s.clients.values().flat_map(|c| c.state.chats.iter()) {
                match all.iter_mut().find(|(k, _)| k == site) {
                    Some(slot) => slot.1 = slot.1.max(*n),
                    None => all.push((site.clone(), *n)),
                }
            }
            all
        },
        outdated: s
            .clients
            .values()
            .filter(|c| c.version != env!("CARGO_PKG_VERSION"))
            .map(|c| browser_label(&c.browser).to_string())
            .collect(),
    }
}

/// How the chat sites are written for a person.
pub fn chat_label(site: &str) -> &str {
    match site {
        "whatsapp" => "WhatsApp",
        "telegram" => "Telegram",
        other => other,
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
            version: String::new(),
            commands: Vec::new(),
            state: Activity::default(),
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
                version: "1.2.3".into(),
                commands: vec!["media.next".into()],
            },
        );
        assert_eq!(c.browser, "brave");
        assert_eq!((c.version.as_str(), c.commands.len()), ("1.2.3", 1));
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

    fn with_commands(mut c: Client, cmds: &[&str]) -> Client {
        c.commands = cmds.iter().map(|k| (*k).to_string()).collect();
        c
    }

    // `run_command` works on the global state, so the tests that use it run one after the other
    static GLOBAL: Mutex<()> = Mutex::new(());

    fn install_for_test(clients: Vec<Client>) -> MutexGuard<'static, ()> {
        let guard = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = lock();
        *s = state_with(clients);
        drop(s);
        guard
    }

    fn sink() -> (Option<mpsc::Sender<String>>, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel();
        (Some(tx), rx)
    }

    #[test]
    fn an_old_extension_is_asked_to_reload_only_when_its_folder_is_current() {
        let build = env!("CARGO_PKG_VERSION");
        assert!(reload_wanted("0.0.1", build, Some(build), 0));
        assert!(
            !reload_wanted(build, build, Some(build), 0),
            "already current"
        );
        assert!(
            !reload_wanted("0.0.1", build, Some("0.0.1"), 0),
            "files not refreshed"
        );
        assert!(!reload_wanted("0.0.1", build, None, 0), "nothing exported");
        assert!(
            !reload_wanted("", build, Some(build), 0),
            "too old to know the request"
        );
        assert!(
            !reload_wanted("0.0.1", build, Some(build), MAX_RELOAD_ASKS),
            "asked enough"
        );
    }

    #[test]
    fn the_reload_goes_to_the_connection_that_said_hello_and_stops_after_a_few_tries() {
        let dir = std::env::temp_dir().join(format!("cursorpie-reload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        extension::export(&dir).unwrap();
        let (out, rx) = sink();
        let mut old = client("chrome", &[], Instant::now());
        old.out = out;
        old.version = "0.0.1".into();
        let _guard = install_for_test(vec![old]);

        ask_to_reload_in(1, &dir);
        let line = rx.try_recv().expect("the extension is asked");
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "reload");
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));

        // it comes back old (loaded from another folder): only a few more tries
        for _ in 0..5 {
            ask_to_reload_in(1, &dir);
        }
        assert_eq!(rx.try_iter().count(), usize::from(MAX_RELOAD_ASKS) - 1);

        // a folder that was never exported is never a reason to ask
        let none =
            std::env::temp_dir().join(format!("cursorpie-reload-none-{}", std::process::id()));
        let (out, rx) = sink();
        let mut other = client("firefox", &[], Instant::now());
        other.out = out;
        other.version = "0.0.1".into();
        drop(_guard);
        let _guard = install_for_test(vec![other]);
        ask_to_reload_in(1, &none);
        assert!(rx.try_recv().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_media_command_goes_to_the_browser_that_makes_sound() {
        let t0 = Instant::now();
        let (tx_quiet, rx_quiet) = sink();
        let (tx_loud, rx_loud) = sink();
        let mut quiet = with_commands(
            client("firefox", &[], t0 + Duration::from_secs(9)),
            &["media.next"],
        );
        quiet.out = tx_quiet;
        let mut loud = with_commands(client("chrome", &[], t0), &["media.next"]);
        loud.out = tx_loud;
        loud.state.audible = 1;
        let _g = install_for_test(vec![quiet, loud]);
        assert_eq!(run_command("media.next"), Dispatch::Sent("chrome".into()));
        let line = rx_loud.try_recv().expect("sent to the loud browser");
        assert!(line.contains(r#""cmd":"media.next""#) && line.contains(r#""type":"command""#));
        assert!(
            rx_quiet.try_recv().is_err(),
            "the browser heard from last is not the one that plays"
        );
    }

    #[test]
    fn a_chat_command_goes_to_the_browser_that_has_a_chat_site_and_ids_count_up() {
        let t0 = Instant::now();
        let (tx_a, rx_a) = sink();
        let (tx_b, rx_b) = sink();
        let mut a = with_commands(
            client("chrome", &[], t0 + Duration::from_secs(5)),
            &["chat.next_unread"],
        );
        a.out = tx_a;
        let mut b = with_commands(client("firefox", &[], t0), &["chat.next_unread"]);
        b.out = tx_b;
        b.state.chats = vec![("whatsapp".into(), Some(2))];
        let _g = install_for_test(vec![a, b]);
        assert!(run_command("chat.next_unread").problem().is_none());
        assert!(run_command("chat.next_unread").problem().is_none());
        let first = rx_b.try_recv().unwrap();
        let second = rx_b.try_recv().unwrap();
        assert!(
            first.contains(r#""id":1"#) && second.contains(r#""id":2"#),
            "{first} {second}"
        );
        assert!(rx_a.try_recv().is_err());
    }

    #[test]
    fn other_commands_go_to_the_browser_heard_from_last() {
        let t0 = Instant::now();
        let (tx_old, rx_old) = sink();
        let (tx_new, rx_new) = sink();
        let mut old = with_commands(client("firefox", &[], t0), &["tab.close"]);
        old.out = tx_old;
        let mut new = with_commands(
            client("chrome", &[], t0 + Duration::from_secs(3)),
            &["tab.close"],
        );
        new.out = tx_new;
        let _g = install_for_test(vec![old, new]);
        assert_eq!(run_command("tab.close"), Dispatch::Sent("chrome".into()));
        assert!(rx_new.try_recv().is_ok() && rx_old.try_recv().is_err());
    }

    #[test]
    fn nothing_is_sent_for_a_bad_line_a_stopped_bridge_or_a_missing_browser() {
        let (tx, rx) = sink();
        let mut c = with_commands(client("chrome", &[], Instant::now()), &["tab.close"]);
        c.out = tx;
        let _g = install_for_test(vec![c]);
        assert!(matches!(run_command("rm -rf /"), Dispatch::Invalid(_)));
        assert!(matches!(run_command(""), Dispatch::Invalid(_)));
        assert!(matches!(
            run_command("chat.send_to nobody"),
            Dispatch::Invalid(_)
        ));
        assert!(rx.try_recv().is_err());
        lock().clients.clear();
        assert_eq!(run_command("tab.close"), Dispatch::NoBrowser);
        lock().running = false;
        assert_eq!(run_command("tab.close"), Dispatch::Off);
        assert!(Dispatch::Off.problem().unwrap().contains("Behavior"));
        assert!(Dispatch::Sent("chrome".into()).problem().is_none());
    }

    #[test]
    fn an_extension_that_does_not_know_the_command_is_called_outdated() {
        let (tx, rx) = sink();
        // the extension of 0.8.9 said hello without a list of commands
        let mut old = client("chrome", &[], Instant::now());
        old.out = tx;
        let _g = install_for_test(vec![old]);
        let d = run_command("media.next");
        assert_eq!(d, Dispatch::Outdated("Chrome".into()));
        assert!(d.problem().unwrap().contains("export it again"));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn a_command_that_is_waited_for_gets_the_answer_and_not_the_screen() {
        let (tx, rx) = sink();
        let mut c = with_commands(client("chrome", &[], Instant::now()), &["tab.url"]);
        c.out = tx;
        let _g = install_for_test(vec![c]);
        let (wtx, wrx) = mpsc::channel();
        let Ok(sent) = dispatch("tab.url", Some(wtx)) else {
            panic!("the command was not sent");
        };
        assert_eq!((sent.id, sent.browser.as_str()), (1, "chrome"));
        assert!(rx.try_recv().is_ok(), "the browser got the command");
        assert_eq!(lock().waiting.len(), 1);
        on_line(
            1,
            r#"{"type":"result","id":1,"ok":true,"message":" https://a.b/ ","notify":true}"#,
        );
        let reply = wrx.try_recv().expect("the waiter was answered");
        assert_eq!(
            reply,
            Reply {
                ok: true,
                message: "https://a.b/".into()
            }
        );
        assert!(lock().waiting.is_empty(), "an answer is taken once");
    }

    #[test]
    fn only_the_browser_a_command_went_to_can_answer_it() {
        let (tx, _rx) = sink();
        let mut first = with_commands(client("chrome", &[], Instant::now()), &["tab.url"]);
        first.out = tx;
        let second = client("firefox", &[], Instant::now());
        let _g = install_for_test(vec![first, second]);
        let (wtx, wrx) = mpsc::channel();
        assert!(dispatch("tab.url", Some(wtx)).is_ok());
        // the other connection says it ran command 1: that is not its command
        on_line(2, r#"{"type":"result","id":1,"ok":true,"message":"x"}"#);
        assert!(wrx.try_recv().is_err());
        assert_eq!(lock().waiting.len(), 1, "still waiting for the right one");
        on_line(
            1,
            r#"{"type":"result","id":1,"ok":false,"message":"No tab is open"}"#,
        );
        let reply = wrx.try_recv().expect("answered by its own browser");
        assert!(!reply.ok);
        assert_eq!(reply.message, "No tab is open");
    }

    #[test]
    fn a_wait_ends_when_the_browser_goes_away() {
        let (tx, _rx) = sink();
        let mut c = with_commands(client("chrome", &[], Instant::now()), &["media.state"]);
        c.out = tx;
        let _g = install_for_test(vec![c]);
        let (wtx, wrx) = mpsc::channel();
        assert!(dispatch("media.state", Some(wtx)).is_ok());
        forget_client(&mut lock(), 1);
        assert!(matches!(
            wrx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert!(lock().clients.is_empty() && lock().waiting.is_empty());
    }

    #[test]
    fn a_line_that_cannot_be_sent_is_a_problem_for_the_flow_not_a_wait() {
        let (tx, rx) = sink();
        let mut c = with_commands(client("chrome", &[], Instant::now()), &["tab.close"]);
        c.out = tx;
        let _g = install_for_test(vec![c]);
        assert!(matches!(run_command_wait("rm -rf /"), Waited::Problem(_)));
        assert!(matches!(run_command_wait("chat.open"), Waited::Problem(_)));
        // an extension that does not know the command: told so at once, nothing is waited for
        assert!(matches!(run_command_wait("media.next"), Waited::Problem(_)));
        assert!(rx.try_recv().is_err());
        assert!(lock().waiting.is_empty());
        lock().running = false;
        let Waited::Problem(why) = run_command_wait("tab.close") else {
            panic!("a stopped bridge is a problem");
        };
        assert!(why.contains("Behavior"), "{why}");
    }

    #[test]
    fn the_status_adds_up_sound_and_chats_and_names_a_stale_extension() {
        let mut a = client("chrome", &["https://a.b/"], Instant::now());
        a.version = env!("CARGO_PKG_VERSION").into();
        a.state = Activity {
            audible: 2,
            media_tabs: 3,
            chats: vec![("whatsapp".into(), Some(3))],
        };
        let mut b = client("firefox", &[], Instant::now());
        b.version = "0.0.1".into();
        b.state = Activity {
            audible: 1,
            media_tabs: 0,
            chats: vec![("whatsapp".into(), Some(5)), ("telegram".into(), None)],
        };
        let _g = install_for_test(vec![a, b]);
        let st = status();
        assert_eq!(st.playing, 3);
        assert_eq!(
            st.chats,
            vec![
                ("whatsapp".to_string(), Some(5)),
                ("telegram".to_string(), None)
            ]
        );
        assert_eq!(st.outdated, vec!["Firefox".to_string()]);
        assert_eq!(chat_label("whatsapp"), "WhatsApp");
        lock().running = false;
    }

    #[test]
    fn a_state_message_replaces_the_previous_one() {
        let mut c = client("chrome", &[], Instant::now());
        let state = |audible, n| FromBrowser::State {
            audible,
            media_tabs: 1,
            chats: vec![proto::WireChat {
                site: "telegram".into(),
                unread: Some(n),
            }],
        };
        apply_message(&mut c, state(1, 4));
        apply_message(&mut c, state(0, 0));
        assert_eq!(c.state.audible, 0);
        assert_eq!(c.state.chats, vec![("telegram".to_string(), Some(0))]);
    }

    #[test]
    fn tokens_differ_and_are_long_enough() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }
}
