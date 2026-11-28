//! The Linux-only half of `hyprland.rs`: the control socket. One request is one connection, opened,
//! written, read to its end and dropped, with a timeout: Hyprland answers a connection
//! synchronously, so one that is left open would freeze it (see `hyprland.rs`).

use super::hyprland::{
    MAX_REPLY, build_snapshot, float_command, focus_command, look_commands, move_command,
    needs_move, parse_active, parse_clients, parse_cursor, parse_monitors, parse_own_window,
    socket_paths, top_left_for_center,
};
use super::kwin;
use super::poller::Source;
use crate::geometry::Point;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// How long one request may take. Hyprland answers in well under a millisecond; this only bounds a
/// compositor that is stuck.
const TIMEOUT: Duration = Duration::from_millis(400);

/// The control socket of this session, if there is one.
fn find_socket() -> Option<PathBuf> {
    socket_paths(&|k| std::env::var(k).ok())
        .into_iter()
        .find(|p| p.exists())
}

/// Writes `command` to a new connection and reads the reply to its end. The connection is dropped
/// (closed) before this returns, whatever happens.
fn request(socket: &Path, command: &str) -> io::Result<String> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(command.as_bytes())?;
    let mut out = Vec::new();
    (&stream).take(MAX_REPLY as u64 + 1).read_to_end(&mut out)?;
    String::from_utf8(out).map_err(|_| io::ErrorKind::InvalidData.into())
}

/// The adapter that `poller.rs` runs on its own thread: it reads Hyprland through its socket.
pub(super) struct Hypr;

impl Source for Hypr {
    fn sync(&mut self) -> bool {
        let Some(socket) = find_socket() else {
            return false;
        };
        let ask = |cmd: &str| request(&socket, cmd).ok();
        let clients = ask("j/clients").and_then(|t| parse_clients(&t));
        let monitors = ask("j/monitors").and_then(|t| parse_monitors(&t));
        let (Some(clients), Some(monitors)) = (clients, monitors) else {
            return false;
        };
        // these two may fail on their own: no window has the focus, or the pointer is unknown
        let active = ask("j/activewindow").and_then(|t| parse_active(&t));
        let pointer = ask("j/cursorpos").and_then(|t| parse_cursor(&t));
        kwin::publish(build_snapshot(clients, &monitors, pointer, active));
        true
    }

    fn pointer(&mut self) {
        let Some(socket) = find_socket() else {
            return;
        };
        let reply = request(&socket, "j/cursorpos").ok();
        if let Some(p) = reply.and_then(|t| parse_cursor(&t)) {
            kwin::set_pointer(p);
        }
    }

    fn command(&mut self, command: &str) {
        if let Some(id) = command.strip_prefix("activate:")
            && let Some(line) = focus_command(id)
            && let Some(socket) = find_socket()
        {
            let _ = request(&socket, &line);
        }
    }

    fn polls_pointer(&self) -> bool {
        true
    }
}

// ------------------------------------------------------------------- CursorPie's own window

/// Which placement is the current one: a newer request ends the thread of the older one.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Whether a placement thread is still working (see [`placing`]).
static PLACING: AtomicBool = AtomicBool::new(false);

/// How long one placement looks for the window and keeps it where it belongs.
const PLACE_MAX: Duration = Duration::from_millis(1200);
/// How often the window is looked at while it is placed.
const PLACE_STEP: Duration = Duration::from_millis(15);
/// How many looks in a row that find nothing to do end the placement.
const PLACE_STABLE: u32 = 8;

/// Whether the window of CursorPie is still being placed. The app keeps the window invisible
/// while it is, so the first frames never show the compositor's border, shadow and blur, or the
/// window in the wrong place.
pub(crate) fn placing() -> bool {
    PLACING.load(Ordering::Acquire)
}

/// Asks Hyprland, from a thread of its own, to put the center of CursorPie's window on `center`
/// (layout points, as the pointer is read) and to take its border, shadow, blur and rounded
/// corners off.
///
/// Why Hyprland has to be asked: the window is an XWayland window, and Hyprland treats it as any
/// other window. It draws a border, a shadow and a blur behind it, and a transparent window
/// (the pie) turns into an outlined box. It also places the window itself, from the position the
/// X client asked for in X's own points, which are not the layout's points when the screen is
/// scaled; the pie was then not under the cursor. `setprop` and `movewindowpixel` act on this one
/// window only, so nothing is needed in the person's `hyprland.conf`.
///
/// A window that is mapped again is a new window for Hyprland (a new address, nothing set), so
/// the thread looks at the window every few milliseconds, not once: whenever the address is a new
/// one the look is taken off again, and whenever the corner is off it is moved. It ends after a
/// few quiet looks, or after [`PLACE_MAX`].
pub(crate) fn place_own_window(center: Point) {
    let Some(socket) = find_socket() else {
        return;
    };
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    PLACING.store(true, Ordering::Release);
    let spawned = std::thread::Builder::new()
        .name("hyprland-place".into())
        .spawn(move || {
            place_loop(&socket, center, generation);
            // only the newest placement may say that nothing is being placed any more
            if GENERATION.load(Ordering::Acquire) == generation {
                PLACING.store(false, Ordering::Release);
            }
        });
    if spawned.is_err() {
        PLACING.store(false, Ordering::Release);
    }
}

fn place_loop(socket: &Path, center: Point, generation: u64) {
    let pid = std::process::id();
    let started = Instant::now();
    let mut done_for: Option<String> = None;
    let mut quiet = 0u32;
    while started.elapsed() < PLACE_MAX && GENERATION.load(Ordering::Acquire) == generation {
        let own = request(socket, "j/clients")
            .ok()
            .and_then(|t| parse_own_window(&t, pid));
        if let Some(w) = own {
            let mut acted = false;
            if done_for.as_deref() != Some(w.id.as_str()) {
                if !w.floating
                    && let Some(cmd) = float_command(&w.id)
                {
                    let _ = request(socket, &cmd);
                }
                for cmd in look_commands(&w.id) {
                    let _ = request(socket, &cmd);
                }
                done_for = Some(w.id.clone());
                acted = true;
            }
            let want = top_left_for_center(center, w.rect.w, w.rect.h);
            if needs_move(&w.rect, want)
                && let Some(cmd) = move_command(&w.id, want.0, want.1)
            {
                let _ = request(socket, &cmd);
                acted = true;
            }
            quiet = if acted { 0 } else { quiet + 1 };
            if quiet >= PLACE_STABLE {
                return;
            }
        }
        std::thread::sleep(PLACE_STEP);
    }
}
