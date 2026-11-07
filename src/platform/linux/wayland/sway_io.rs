//! The Linux-only half of `sway.rs`: Sway's IPC socket. One request is one connection, with a
//! timeout, dropped when the reply has been read.

use super::kwin;
use super::poller::Source;
use super::sway::{
    GET_TREE, HEADER_LEN, RUN_COMMAND, encode, focus_command, parse_header, parse_tree, socket_path,
};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long one request may take; it only bounds a compositor that is stuck.
const TIMEOUT: Duration = Duration::from_millis(500);

/// The socket of this session.
fn find_socket() -> Option<PathBuf> {
    socket_path(&|k| std::env::var(k).ok()).filter(|p| p.exists())
}

/// Sends one message on a new connection and returns the payload of its reply. The connection is
/// dropped before this returns.
fn request(socket: &Path, kind: u32, payload: &[u8]) -> io::Result<Vec<u8>> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(&encode(kind, payload))?;
    let mut head = [0u8; HEADER_LEN];
    stream.read_exact(&mut head)?;
    let Some((len, _)) = parse_header(&head) else {
        return Err(io::ErrorKind::InvalidData.into());
    };
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body)?;
    Ok(body)
}

/// The adapter that `poller.rs` runs on its own thread: it reads Sway through its socket.
pub(super) struct Sway;

impl Source for Sway {
    fn sync(&mut self) -> bool {
        let Some(socket) = find_socket() else {
            return false;
        };
        let Ok(body) = request(&socket, GET_TREE, b"") else {
            return false;
        };
        let Some(snap) = std::str::from_utf8(&body).ok().and_then(parse_tree) else {
            return false;
        };
        kwin::publish(snap);
        true
    }

    fn pointer(&mut self) {}

    fn command(&mut self, command: &str) {
        if let Some(id) = command.strip_prefix("activate:")
            && let Some(line) = focus_command(id)
            && let Some(socket) = find_socket()
        {
            let _ = request(&socket, RUN_COMMAND, line.as_bytes());
        }
    }

    fn polls_pointer(&self) -> bool {
        false
    }
}
