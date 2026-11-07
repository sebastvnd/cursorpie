//! The Linux-only half of `hyprland.rs`: the control socket. One request is one connection, opened,
//! written, read to its end and dropped, with a timeout: Hyprland answers a connection
//! synchronously, so one that is left open would freeze it (see `hyprland.rs`).

use super::hyprland::{
    MAX_REPLY, build_snapshot, focus_command, parse_active, parse_clients, parse_cursor,
    parse_monitors, socket_paths,
};
use super::kwin;
use super::poller::Source;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
