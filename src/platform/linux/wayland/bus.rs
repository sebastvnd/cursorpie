//! The D-Bus half of the KDE and GNOME adapters (the `zbus` crate; decision OD-W1).
//!
//! One thread does everything, so nothing here ever blocks the tap thread:
//!
//! * it owns the name [`kwin::SERVICE`] and serves two methods the script calls (`Report`,
//!   `Pointer`), which only *receive* data (risk R5, see `kwin::parse_snapshot`);
//! * it writes the script to `$XDG_RUNTIME_DIR` and loads it into KWin (`org.kde.KWin`,
//!   `/Scripting`), and looks again every two seconds that it is still loaded, so it comes back
//!   after KWin restarts (SW7);
//! * when asked ([`request_sync`], a channel, so the caller never waits) it invokes the global
//!   shortcut the script registered, which makes the script report.
//!
//! On GNOME the same service is served, but nothing is loaded: the extension (see `gnome.rs`) is
//! installed by the user. The thread then only checks that the extension owns its bus name and asks
//! it to report by calling its `Sync` method.
//!
//! **Not run.** The calls to `org.kde.KWin` and `org.kde.kglobalaccel` follow the interfaces as
//! read in the KWin sources and the KDE documentation; they are the thing the spikes SW4 and SW7
//! must confirm on a Plasma 6.8 machine, and the place to look first if the adapter stays off.

use super::adapter::Adapter;
use super::{gnome, kwin};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use zbus::blocking::Connection;
use zbus::blocking::connection::Builder;
use zbus::interface;

/// The object the script calls.
struct Service;

#[interface(name = "org.cursorpie.Wayland")]
impl Service {
    /// The window list and the pointer; the reply is the commands waiting for the script.
    fn report(&self, json: &str) -> String {
        kwin::on_report(json)
    }

    /// The pointer, `"x,y"`.
    fn pointer(&self, xy: &str) {
        kwin::on_pointer(xy);
    }
}

static SYNC: OnceLock<Mutex<Sender<()>>> = OnceLock::new();

/// Asks the script to report. Never waits.
pub(crate) fn request_sync() {
    if let Some(tx) = SYNC.get() {
        let _ = tx.lock().unwrap_or_else(|e| e.into_inner()).send(());
    }
}

/// Starts the thread (once). `adapter` is `Kwin` or `Gnome`.
pub(crate) fn start(adapter: Adapter) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = channel();
    let _ = SYNC.set(Mutex::new(tx));
    let spawned = std::thread::Builder::new()
        .name("cursorpie-kwin".into())
        .spawn(move || {
            let conn = loop {
                match connect() {
                    Ok(c) => break c,
                    Err(e) => {
                        diag::log!("wayland", "bus_failed", error = format!("{e}"));
                        std::thread::sleep(Duration::from_secs(5));
                    }
                }
            };
            loop {
                match rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(()) => {
                        while rx.try_recv().is_ok() {} // one invoke serves every request waiting
                        invoke_sync(&conn, adapter);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if adapter == Adapter::Gnome {
                            check_bridge(&conn);
                        } else {
                            ensure_loaded(&conn);
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
        });
    if spawned.is_err() {
        eprintln!("CursorPie: could not start the KWin adapter thread");
    }
}

fn connect() -> zbus::Result<Connection> {
    Builder::session()?
        .name(kwin::SERVICE)?
        .serve_at(kwin::PATH, Service)?
        .build()
}

/// Where the script file is written: the runtime directory of the user, so nothing is left behind
/// after a log out.
fn script_path() -> std::path::PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|d| d.starts_with('/'))
        .unwrap_or_else(|| "/tmp".to_string());
    std::path::Path::new(&dir).join("cursorpie-adapter.js")
}

fn kwin_call<B>(
    conn: &Connection,
    path: &str,
    iface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<zbus::Message>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    conn.call_method(Some("org.kde.KWin"), path, Some(iface), method, body)
}

/// Loads the script if KWin does not have it (first time, or after KWin restarted).
fn ensure_loaded(conn: &Connection) {
    let loaded = kwin_call(
        conn,
        "/Scripting",
        "org.kde.kwin.Scripting",
        "isScriptLoaded",
        &(kwin::PLUGIN,),
    )
    .ok()
    .and_then(|m| m.body().deserialize::<bool>().ok())
    .unwrap_or(false);
    if loaded {
        kwin::set_loaded(true);
        return;
    }
    kwin::set_loaded(false);
    let path = script_path();
    if std::fs::write(&path, kwin::script_source()).is_err() {
        return;
    }
    let file = path.to_string_lossy().into_owned();
    let id = kwin_call(
        conn,
        "/Scripting",
        "org.kde.kwin.Scripting",
        "loadScript",
        &(file.as_str(), kwin::PLUGIN),
    )
    .ok()
    .and_then(|m| m.body().deserialize::<i32>().ok());
    let Some(id) = id.filter(|id| *id >= 0) else {
        diag::log!("wayland", "script_load_failed");
        return;
    };
    // a loaded script is started through its own object; older KWin starts all of them at once
    let ran = kwin_call(
        conn,
        &format!("/Scripting/Script{id}"),
        "org.kde.kwin.Script",
        "run",
        &(),
    )
    .is_ok()
        || kwin_call(conn, "/Scripting", "org.kde.kwin.Scripting", "start", &()).is_ok();
    diag::log!("wayland", "script_loaded", id = id, started = ran);
    kwin::set_loaded(ran);
}

/// GNOME: whether the extension is running (it owns its name), and, while nothing has been
/// reported yet, asks it to report.
fn check_bridge(conn: &Connection) {
    let owned = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(gnome::BRIDGE_NAME,),
        )
        .ok()
        .and_then(|m| m.body().deserialize::<bool>().ok())
        .unwrap_or(false);
    kwin::set_loaded(owned);
    if owned && kwin::snapshot().is_none() {
        invoke_sync(conn, Adapter::Gnome);
    }
}

/// Makes the adapter report: invokes the script's shortcut (KDE), or calls the extension (GNOME).
fn invoke_sync(conn: &Connection, adapter: Adapter) {
    let r = if adapter == Adapter::Gnome {
        conn.call_method(
            Some(gnome::BRIDGE_NAME),
            gnome::BRIDGE_PATH,
            Some(gnome::BRIDGE_IFACE),
            gnome::BRIDGE_SYNC,
            &(),
        )
    } else {
        conn.call_method(
            Some("org.kde.kglobalaccel"),
            "/component/kwin",
            Some("org.kde.kglobalaccel.Component"),
            "invokeShortcut",
            &(kwin::SHORTCUT,),
        )
    };
    if r.is_err() {
        diag::log!("wayland", "sync_failed");
    }
}
