//! A plain HTTPS `GET` for the update check, the same on every OS.
//!
//! CursorPie has no HTTP library on purpose: the app is small and makes no network connections of its
//! own, apart from the one look at the newest release (see [`update`](crate::update)). That request
//! is made by the `curl` program that macOS, Windows 10 (1803 and later) and nearly every Linux
//! distribution ship with, or by `wget` where `curl` is missing. The program is started directly and
//! never through a shell, so no part of the address can be read as a command.
//!
//! This module has no backend of its own, like [`web`](super::web): it is the same code on every OS,
//! which keeps the list of functions a backend must have (the table in [`platform`](super)) as it was.
//! The only OS-specific line is the Windows flag that keeps a console window from flashing.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Why a request gave no text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpError {
    /// Neither `curl` nor `wget` could be started.
    NoClient,
    /// The program ran but the request failed: no network, an error status, a timeout.
    Failed,
    /// The answer was bigger than allowed, or was not UTF-8 text.
    BadAnswer,
}

/// The programs that can make the request, in the order they are tried.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Client {
    Curl,
    Wget,
}

/// Every client, in the order they are tried.
const CLIENTS: [Client; 2] = [Client::Curl, Client::Wget];

/// What the request says about itself. GitHub's API refuses a request without one.
fn user_agent() -> String {
    format!("{}/{}", crate::about::APP_NAME, crate::about::VERSION)
}

impl Client {
    /// The name of the program, looked up in `PATH`.
    fn program(self) -> &'static str {
        match self {
            Client::Curl => "curl",
            Client::Wget => "wget",
        }
    }

    /// The arguments for a `GET` of `url` that gives up after `timeout_s` seconds. Only `https` is
    /// followed, also across redirects.
    fn args(self, url: &str, timeout_s: u64) -> Vec<String> {
        let accept = "Accept: application/vnd.github+json".to_string();
        match self {
            Client::Curl => vec![
                "--silent".into(),
                "--fail".into(),
                "--location".into(),
                "--proto".into(),
                "=https".into(),
                "--proto-redir".into(),
                "=https".into(),
                "--max-time".into(),
                timeout_s.to_string(),
                "--user-agent".into(),
                user_agent(),
                "--header".into(),
                accept,
                url.into(),
            ],
            Client::Wget => vec![
                "--quiet".into(),
                "--output-document=-".into(),
                format!("--timeout={timeout_s}"),
                "--tries=1".into(),
                format!("--user-agent={}", user_agent()),
                format!("--header={accept}"),
                url.into(),
            ],
        }
    }
}

impl Client {
    /// The arguments for a download of `url` into the file `dest`: the same rules as [`Client::args`],
    /// and `curl` also refuses a file of more than `max_bytes` when the server says how big it is.
    fn download_args(self, url: &str, dest: &Path, timeout_s: u64, max_bytes: u64) -> Vec<String> {
        let dest = dest.to_string_lossy().into_owned();
        match self {
            Client::Curl => vec![
                "--silent".into(),
                "--fail".into(),
                "--location".into(),
                "--proto".into(),
                "=https".into(),
                "--proto-redir".into(),
                "=https".into(),
                "--max-time".into(),
                timeout_s.to_string(),
                "--max-filesize".into(),
                max_bytes.to_string(),
                "--user-agent".into(),
                user_agent(),
                "--output".into(),
                dest,
                url.into(),
            ],
            Client::Wget => vec![
                "--quiet".into(),
                format!("--output-document={dest}"),
                format!("--timeout={timeout_s}"),
                "--tries=1".into(),
                format!("--user-agent={}", user_agent()),
                url.into(),
            ],
        }
    }
}

/// Fetches `url` (it must start with `https://`) and returns the body as text.
///
/// Gives up after `timeout_s` seconds and refuses an answer of more than `max_bytes`. It blocks, so
/// call it from a worker thread, never from the UI thread or the event tap. `wget` is only tried
/// when `curl` is not installed: a failed request is not repeated with another program.
///
/// # Errors
///
/// See [`HttpError`].
pub fn get_text(url: &str, timeout_s: u64, max_bytes: u64) -> Result<String, HttpError> {
    // also keeps an address from being read as an option by the program
    if !url.starts_with("https://") {
        return Err(HttpError::Failed);
    }
    for client in CLIENTS {
        let args = client.args(url, timeout_s);
        match run(client.program(), &args, timeout_s, max_bytes) {
            Err(HttpError::NoClient) => continue,
            other => return other,
        }
    }
    Err(HttpError::NoClient)
}

/// Downloads `url` (it must start with `https://`) into the file `dest`, which is replaced.
///
/// Gives up after `timeout_s` seconds and refuses a file of more than `max_bytes` (the partial file
/// is deleted then, and after any other failure). It blocks, so call it from a worker thread. As with
/// [`get_text`], `wget` is only tried when `curl` is not installed.
///
/// # Errors
///
/// See [`HttpError`].
pub fn download(url: &str, dest: &Path, timeout_s: u64, max_bytes: u64) -> Result<(), HttpError> {
    if !url.starts_with("https://") {
        return Err(HttpError::Failed);
    }
    for client in CLIENTS {
        let args = client.download_args(url, dest, timeout_s, max_bytes);
        let outcome = run_to_end(client.program(), &args, timeout_s);
        if outcome == Err(HttpError::NoClient) {
            continue;
        }
        let too_big = std::fs::metadata(dest).is_ok_and(|m| m.len() > max_bytes);
        if outcome.is_err() || too_big {
            let _ = std::fs::remove_file(dest);
        }
        return match outcome {
            Ok(()) if too_big => Err(HttpError::BadAnswer),
            other => other,
        };
    }
    Err(HttpError::NoClient)
}

/// Runs `program` until it ends, for at most `timeout_s` seconds (and a few more to start it).
fn run_to_end(program: &str, args: &[String], timeout_s: u64) -> Result<(), HttpError> {
    let mut child = quiet_command(program, args, Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                HttpError::NoClient
            } else {
                HttpError::Failed
            }
        })?;
    let deadline = Instant::now() + Duration::from_secs(timeout_s.saturating_add(5));
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err(HttpError::Failed),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HttpError::Failed);
            }
        }
    }
}

/// A command that runs without a console window and with no input and no error output; its standard
/// output goes where `stdout` says.
fn quiet_command(program: &str, args: &[String], stdout: Stdio) -> Command {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes on screen
    }
    cmd
}

/// Runs `program` with `args` and returns what it wrote, if it ended well within `timeout_s` seconds
/// (a little longer is allowed for starting it) and wrote at most `max_bytes`.
fn run(
    program: &str,
    args: &[String],
    timeout_s: u64,
    max_bytes: u64,
) -> Result<String, HttpError> {
    let mut cmd = quiet_command(program, args, Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            HttpError::NoClient
        } else {
            HttpError::Failed
        }
    })?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(HttpError::Failed);
    };
    // The body is read on a thread of its own so that a program that hangs (`wget` only times out
    // a silent connection, not a slow one) is stopped by the deadline below and never holds the
    // caller. One byte more than allowed is read, which is how "too big" is told from "just fits".
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut body = Vec::new();
        let read = stdout
            .take(max_bytes.saturating_add(1))
            .read_to_end(&mut body);
        let _ = tx.send(read.map(|_| body));
    });
    let answer = rx.recv_timeout(Duration::from_secs(timeout_s.saturating_add(2)));
    let body = match answer {
        Ok(Ok(body)) if body.len() as u64 <= max_bytes => body,
        Ok(Ok(_)) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HttpError::BadAnswer);
        }
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HttpError::Failed);
        }
    };
    let ended_well = child.wait().is_ok_and(|s| s.success());
    if !ended_well {
        return Err(HttpError::Failed);
    }
    String::from_utf8(body).map_err(|_| HttpError::BadAnswer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The argument that follows `flag`.
    fn after(args: &[String], flag: &str) -> String {
        let at = args.iter().position(|a| a == flag).expect(flag);
        args[at + 1].clone()
    }

    #[test]
    fn curl_only_follows_https_and_ends_with_the_address() {
        let a = Client::Curl.args("https://example.com/x", 10);
        assert_eq!(after(&a, "--proto"), "=https");
        assert_eq!(after(&a, "--proto-redir"), "=https");
        assert_eq!(after(&a, "--max-time"), "10");
        assert_eq!(a.last().map(String::as_str), Some("https://example.com/x"));
    }

    #[test]
    fn both_clients_say_who_is_asking() {
        let ua = user_agent();
        assert!(ua.starts_with("CursorPie/"), "{ua}");
        let curl = Client::Curl.args("https://example.com", 5);
        assert_eq!(after(&curl, "--user-agent"), ua);
        let wget = Client::Wget.args("https://example.com", 5);
        assert!(wget.contains(&format!("--user-agent={ua}")));
        assert!(wget.contains(&"--timeout=5".to_string()));
    }

    #[test]
    fn a_link_that_is_not_https_is_refused_before_anything_runs() {
        for url in ["http://x.org", "-o /tmp/x", "file:///etc/x", ""] {
            assert_eq!(get_text(url, 5, 1024), Err(HttpError::Failed), "{url}");
        }
    }

    #[test]
    fn a_download_names_its_file_and_keeps_to_https() {
        let dest = Path::new("/tmp/cursorpie-update/pkg.dmg");
        let a = Client::Curl.download_args("https://example.com/p", dest, 30, 1000);
        assert_eq!(after(&a, "--output"), dest.to_string_lossy());
        assert_eq!(after(&a, "--proto-redir"), "=https");
        assert_eq!(after(&a, "--max-filesize"), "1000");
        assert_eq!(a.last().map(String::as_str), Some("https://example.com/p"));
        let w = Client::Wget.download_args("https://example.com/p", dest, 30, 1000);
        assert!(w.contains(&format!("--output-document={}", dest.display())));
    }

    #[test]
    fn a_download_from_a_link_that_is_not_https_never_runs() {
        let dest = std::env::temp_dir().join("cursorpie-never-written.bin");
        assert_eq!(
            download("http://x.org/p", &dest, 5, 10),
            Err(HttpError::Failed)
        );
        assert!(!dest.exists());
    }

    #[test]
    fn a_program_that_is_not_installed_is_reported_as_such() {
        let r = run("cursorpie-no-such-program-0", &[], 5, 1024);
        assert_eq!(r, Err(HttpError::NoClient));
    }
}
