//! A plain HTTPS `GET` for the update check, the same on every OS.
//!
//! CursorPie has no HTTP library on purpose: the app is small and makes no network connections of its
//! own, apart from the one look at the newest release (see [`update`](crate::update)) and the website
//! icon the user asks for in the icon picker (see [`webicon`](crate::ui::webicon)). Those requests
//! are made by the `curl` program that macOS, Windows 10 (1803 and later) and nearly every Linux
//! distribution ship with, or by `wget` where `curl` is missing. The program is started directly and
//! never through a shell, so no part of the address can be read as a command.
//!
//! A request that `curl` fails is tried again with `wget` when that is installed, and the reason a request
//! failed (the exit code and what it means) is kept for the person (see [`last_detail`]).
//!
//! This module has no backend of its own, like [`web`](super::web): it is the same code on every OS,
//! which keeps the list of functions a backend must have (the table in [`platform`](super)) as it was.
//! The only OS-specific line is the Windows flag that keeps a console window from flashing.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, mpsc};
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

/// What the last failed request came to, in words a person can pass on (`curl exit 22: ...`). Kept
/// for the update dialogs, which would otherwise only be able to say "could not be reached" for a
/// dozen different causes. Best effort: another request on another thread can overwrite it.
static LAST_DETAIL: Mutex<String> = Mutex::new(String::new());

/// The exit code of the last client that failed (`i32::MIN` = none, or it ended without a code).
static LAST_CODE: AtomicI32 = AtomicI32::new(i32::MIN);

/// Forgets the detail of an earlier request; every public request starts with it.
fn clear_detail() {
    LAST_DETAIL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
    LAST_CODE.store(i32::MIN, Ordering::Relaxed);
}

/// Adds one sentence to the detail of the request that is running.
fn add_detail(text: &str) {
    let mut detail = LAST_DETAIL.lock().unwrap_or_else(|e| e.into_inner());
    if !detail.is_empty() {
        detail.push_str("; ");
    }
    detail.push_str(text);
}

/// Why the last request failed, as far as is known (empty when it did not fail). See
/// `LAST_DETAIL`.
pub fn last_detail() -> String {
    LAST_DETAIL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// What an exit code of `curl` or `wget` means, in a few words. The codes are the documented ones
/// (`man curl`, `man wget`); any other code is only named.
pub fn exit_meaning(program: &str, code: Option<i32>) -> String {
    let Some(code) = code else {
        return format!("{program} was ended without an exit code");
    };
    let meaning = match (program, code) {
        ("curl", 5 | 6) => "the server name could not be looked up (no network, DNS or proxy)",
        ("curl", 7) => "could not connect to the server (no network, a firewall or a proxy)",
        ("curl", 18 | 56) => "the connection broke during the transfer",
        ("curl", 22) => {
            "the server answered with an error status (403: GitHub's limit for this network was reached; 404: the file is not published yet)"
        }
        ("curl", 23) => "the file could not be written (disk full or no permission)",
        ("curl", 28) => "the request timed out",
        ("curl", 35 | 58 | 59 | 77 | 83) => "the secure (TLS) connection could not be made",
        ("curl", 60) => {
            "the server certificate could not be verified (check the clock and any proxy)"
        }
        ("curl", 63) => "the file is bigger than allowed",
        ("wget", 3) => "the file could not be written (disk full or no permission)",
        ("wget", 4) => "network failure (no connection, DNS or timeout)",
        ("wget", 5) => "the server certificate could not be verified",
        ("wget", 8) => "the server answered with an error status",
        _ => "",
    };
    if meaning.is_empty() {
        format!("{program} exit {code}")
    } else {
        format!("{program} exit {code}: {meaning}")
    }
}

/// Remembers how `program` ended (see [`LAST_DETAIL`]).
fn note_exit(program: &str, code: Option<i32>) {
    LAST_CODE.store(code.unwrap_or(i32::MIN), Ordering::Relaxed);
    add_detail(&exit_meaning(program, code));
}

/// Whether `curl` stopped at the TLS handshake on Windows. There it is most often the check of the
/// certificate's revocation list (`CRYPT_E_NO_REVOCATION_CHECK`) that a firewall or an antivirus
/// blocks, and one more try without that single check connects; the connection is encrypted and
/// the certificate is checked in the same way.
fn tls_handshake_failed() -> bool {
    cfg!(windows) && LAST_CODE.load(Ordering::Relaxed) == 35
}

/// Runs `attempt` with each client in turn, until one gives an answer. A client that is not
/// installed is skipped; a client that ran and failed is followed by the next one (a `curl` that
/// cannot make the secure connection on this machine is often followed by a `wget` that can), and
/// on Windows a `curl` that stopped at the TLS handshake gets one more try with `relaxed` set. A
/// wrong answer (too big, not text) ends it: another program would get the same.
fn with_clients<T>(
    mut attempt: impl FnMut(Client, bool) -> Result<T, HttpError>,
) -> Result<T, HttpError> {
    clear_detail();
    let mut last = HttpError::NoClient;
    for client in CLIENTS {
        let mut result = attempt(client, false);
        if matches!(result, Err(HttpError::Failed))
            && client == Client::Curl
            && tls_handshake_failed()
        {
            add_detail("trying again without the revocation check");
            result = attempt(client, true);
        }
        match result {
            Ok(value) => return Ok(value),
            Err(HttpError::NoClient) => {}
            Err(HttpError::BadAnswer) => return Err(HttpError::BadAnswer),
            Err(HttpError::Failed) => last = HttpError::Failed,
        }
    }
    if last == HttpError::NoClient {
        add_detail("neither curl nor wget was found");
    }
    Err(last)
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

    /// What every request has in common, whatever it fetches: quiet, `https` only (also across
    /// redirects), a time limit of `timeout_s` seconds, no retry, and the program's own user agent.
    fn base_args(self, timeout_s: u64) -> Vec<String> {
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
            ],
            Client::Wget => vec![
                "--quiet".into(),
                format!("--timeout={timeout_s}"),
                "--tries=1".into(),
                format!("--user-agent={}", user_agent()),
            ],
        }
    }

    /// The arguments for a `GET` of `url` that gives up after `timeout_s` seconds. Only `https` is
    /// followed, also across redirects.
    fn args(self, url: &str, timeout_s: u64) -> Vec<String> {
        self.args_accepting(url, timeout_s, "Accept: application/vnd.github+json")
    }

    /// [`Client::args`] with the `Accept` header (the whole line, `Accept: ...`) of the caller.
    fn args_accepting(self, url: &str, timeout_s: u64, accept: &str) -> Vec<String> {
        let mut args = self.base_args(timeout_s);
        match self {
            Client::Curl => args.extend(["--header".into(), accept.into()]),
            Client::Wget => {
                args.extend(["--output-document=-".into(), format!("--header={accept}")])
            }
        }
        args.push(url.into());
        args
    }

    /// The arguments for a download of `url` into the file `dest`: the same rules as [`Client::args`],
    /// and `curl` also refuses a file of more than `max_bytes` when the server says how big it is.
    fn download_args(self, url: &str, dest: &Path, timeout_s: u64, max_bytes: u64) -> Vec<String> {
        let dest = dest.to_string_lossy().into_owned();
        let mut args = self.base_args(timeout_s);
        match self {
            Client::Curl => args.extend([
                "--max-filesize".into(),
                max_bytes.to_string(),
                // a slow link is not a failed one: only a transfer that stays under 1 KB/s for a
                // minute is given up (the overall time limit is the one of the caller)
                "--connect-timeout".into(),
                "20".into(),
                "--speed-limit".into(),
                "1024".into(),
                "--speed-time".into(),
                "60".into(),
                "--output".into(),
                dest,
            ]),
            Client::Wget => args.push(format!("--output-document={dest}")),
        }
        args.push(url.into());
        args
    }
}

/// Fetches `url` (it must start with `https://`) and returns the body as text.
///
/// Gives up after `timeout_s` seconds and refuses an answer of more than `max_bytes`. It blocks, so
/// call it from a worker thread, never from the UI thread or the event tap. `curl` is tried first;
/// when it is missing or fails, `wget` is tried (see 'with_clients`). Why a request failed is in
/// [`last_detail`].
///
/// # Errors
///
/// See [`HttpError`].
pub fn get_text(url: &str, timeout_s: u64, max_bytes: u64) -> Result<String, HttpError> {
    // also keeps an address from being read as an option by the program
    if !url.starts_with("https://") {
        clear_detail();
        add_detail("the address is not an https address");
        return Err(HttpError::Failed);
    }
    with_clients(|client, relaxed| {
        let mut args = client.args(url, timeout_s);
        if relaxed {
            args.insert(0, "--ssl-no-revoke".into());
        }
        run(client.program(), &args, timeout_s, max_bytes)
    })
}

/// Fetches the web page at `url` (it must start with `https://`) and returns its HTML as text. The
/// same as [`get_text`], but it asks for `text/html` (the update check asks for GitHub's JSON), and
/// it reads a page whole. The website icon lookup (see [`webicon`](crate::ui::webicon)) uses
/// [`get_head`] instead, because the page it needs is only its start.
///
/// # Errors
///
/// See [`HttpError`]. A page that is not UTF-8 is [`HttpError::BadAnswer`].
pub fn get_page(url: &str, timeout_s: u64, max_bytes: u64) -> Result<String, HttpError> {
    if !url.starts_with("https://") {
        return Err(HttpError::Failed);
    }
    for client in CLIENTS {
        let args = client.args_accepting(url, timeout_s, "Accept: text/html,*/*;q=0.8");
        match run(client.program(), &args, timeout_s, max_bytes) {
            Err(HttpError::NoClient) => continue,
            other => return other,
        }
    }
    Err(HttpError::NoClient)
}

/// Fetches the web page at `url` (it must start with `https://`) and returns **the first
/// `max_bytes` bytes** of it. Unlike [`get_page`] a page that is bigger is not an error: the request
/// is stopped once enough was read. The `<link>` tags that name the icons of a site are in the head
/// of the page, but a big home page (GitHub's is well over 500 KB) made [`get_page`] give up, and
/// the lookup then never saw them.
///
/// # Errors
///
/// [`HttpError::NoClient`] without `curl` and `wget`; [`HttpError::Failed`] when the request failed
/// or the answer was empty.
pub fn get_head(url: &str, timeout_s: u64, max_bytes: u64) -> Result<Vec<u8>, HttpError> {
    if !url.starts_with("https://") {
        return Err(HttpError::Failed);
    }
    for client in CLIENTS {
        let args = client.args_accepting(url, timeout_s, "Accept: text/html,*/*;q=0.8");
        match run_head(client.program(), &args, timeout_s, max_bytes) {
            Err(HttpError::NoClient) => continue,
            other => return other,
        }
    }
    Err(HttpError::NoClient)
}

/// Runs `program` and returns the first `max_bytes` bytes it writes, then stops it.
fn run_head(
    program: &str,
    args: &[String],
    timeout_s: u64,
    max_bytes: u64,
) -> Result<Vec<u8>, HttpError> {
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
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut body = Vec::new();
        let read = stdout.take(max_bytes).read_to_end(&mut body);
        let _ = tx.send(read.map(|_| body));
    });
    let answer = rx.recv_timeout(Duration::from_secs(timeout_s.saturating_add(2)));
    // enough was read (or the time is up): the rest of the page is not wanted
    let _ = child.kill();
    let _ = child.wait();
    match answer {
        Ok(Ok(body)) if !body.is_empty() => Ok(body),
        _ => Err(HttpError::Failed),
    }
}

/// Downloads `url` (it must start with `https://`) into the file `dest`, which is replaced.
///
/// Gives up after `timeout_s` seconds and refuses a file of more than `max_bytes` (the partial file
/// is deleted then, and after any other failure). It blocks, so call it from a worker thread. As
/// with [`get_text`], `wget` is tried when `curl` is missing or fails. Why a download failed is in
/// [`last_detail`].
///
/// # Errors
///
/// See [`HttpError`].
pub fn download(url: &str, dest: &Path, timeout_s: u64, max_bytes: u64) -> Result<(), HttpError> {
    if !url.starts_with("https://") {
        clear_detail();
        add_detail("the address is not an https address");
        return Err(HttpError::Failed);
    }
    with_clients(|client, relaxed| {
        let mut args = client.download_args(url, dest, timeout_s, max_bytes);
        if relaxed {
            args.insert(0, "--ssl-no-revoke".into());
        }
        // what an earlier try left behind is not the start of this one
        let _ = std::fs::remove_file(dest);
        let outcome = run_to_end(client.program(), &args, timeout_s);
        let too_big = std::fs::metadata(dest).is_ok_and(|m| m.len() > max_bytes);
        if outcome.is_err() || too_big {
            let _ = std::fs::remove_file(dest);
        }
        match outcome {
            Ok(()) if too_big => Err(HttpError::BadAnswer),
            other => other,
        }
    })
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
            Ok(Some(status)) => {
                note_exit(program, status.code());
                return Err(HttpError::Failed);
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                add_detail(&format!("{program} timed out after {timeout_s} s"));
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
    super::hide_console(&mut cmd); // no console flashes on screen (Windows)
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
            add_detail(&format!("{program} timed out after {timeout_s} s"));
            return Err(HttpError::Failed);
        }
    };
    let status = child.wait();
    if !status.as_ref().is_ok_and(|s| s.success()) {
        note_exit(program, status.ok().and_then(|s| s.code()));
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
    fn an_exit_code_is_told_in_words() {
        assert!(exit_meaning("curl", Some(22)).contains("403"));
        assert!(exit_meaning("curl", Some(6)).contains("looked up"));
        assert!(exit_meaning("curl", Some(60)).contains("certificate"));
        assert!(exit_meaning("wget", Some(4)).contains("network"));
        // a code that is not known is still named, so it can be searched for
        assert_eq!(exit_meaning("curl", Some(99)), "curl exit 99");
        assert!(exit_meaning("curl", None).contains("without an exit code"));
    }

    #[test]
    fn a_program_that_is_not_installed_is_reported_as_such() {
        let r = run("cursorpie-no-such-program-0", &[], 5, 1024);
        assert_eq!(r, Err(HttpError::NoClient));
    }
}
