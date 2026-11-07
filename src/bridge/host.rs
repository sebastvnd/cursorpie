//! The native messaging host: the short-lived process a browser starts for the extension.
//!
//! The browser starts the **CursorPie executable** itself (the host manifest points at it) and
//! passes the caller as the first argument: `chrome-extension://<id>/` in Chromium browsers, the
//! path of the host manifest in Firefox. [`wants_host`] recognizes both, and [`run`] then only
//! relays: frames from the browser's `stdin` go to the running app as lines, lines from the app go
//! to the browser's `stdout` as frames. It shows no window, claims no single-instance lock and
//! never writes anything else to `stdout` (the browser would read it as a message).
//!
//! When the app is not running there is nothing to relay to: the host ends at once, the browser
//! sees the port close, and the extension tries again a minute later.

use super::proto::{self, Auth, BridgeFile, HOST_NAME};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

/// Whether the arguments (without the program name) are those of a browser starting the host.
/// `--native-host` does the same by hand, for trying it out in a terminal.
pub fn wants_host(args: &[String]) -> bool {
    let manifest = format!("{HOST_NAME}.json");
    args.iter().any(|a| {
        a == "--native-host" || a.starts_with("chrome-extension://") || a.ends_with(&manifest)
    })
}

/// Connects to the running app and proves who it is. `None` when the app is not running.
fn connect() -> Option<TcpStream> {
    let text = std::fs::read_to_string(super::bridge_file_path()).ok()?;
    let file: BridgeFile = serde_json::from_str(&text).ok()?;
    let addr = SocketAddr::from(([127, 0, 0, 1], file.port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    let mut line = serde_json::to_string(&Auth { auth: file.token }).ok()?;
    line.push('\n');
    stream.write_all(line.as_bytes()).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut reply = String::new();
    BufReader::new(stream.try_clone().ok()?)
        .read_line(&mut reply)
        .ok()?;
    if !reply.contains("true") {
        return None;
    }
    stream.set_read_timeout(None).ok()?;
    Some(stream)
}

/// Runs the host until the browser or the app closes its side. The exit code of the process.
pub fn run() -> i32 {
    let Some(link) = connect() else {
        return 1;
    };
    let Ok(mut to_app) = link.try_clone() else {
        return 1;
    };
    // browser -> app. The browser closing its pipe ends the host: it has nothing left to serve.
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        while let Ok(Some(frame)) = proto::read_frame(&mut stdin) {
            // one message per line: read it as JSON and write it back compact
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&frame) else {
                continue;
            };
            let mut line = value.to_string();
            line.push('\n');
            if to_app.write_all(line.as_bytes()).is_err() {
                break;
            }
        }
        std::process::exit(0);
    });
    // app -> browser
    let mut out = std::io::stdout().lock();
    for line in BufReader::new(link).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if proto::write_frame(&mut out, line.as_bytes()).is_err() {
            break;
        }
    }
    0
}

/// The command-line switches of the bridge, for packaging and for a person to run by hand. `None`
/// when `args` holds none of them (the program then starts as usual); otherwise the exit code.
///
/// * `--export-extension [DIR]` writes the extension (default: the app's support folder);
/// * `--install-browser-bridge` registers the host with the installed browsers;
/// * `--uninstall-browser-bridge` removes that again.
pub fn command_line(args: &[String]) -> Option<i32> {
    let first = args.first()?.as_str();
    match first {
        "--export-extension" => {
            let dir = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(super::extension::default_dir);
            Some(match super::extension::export(&dir) {
                Ok(()) => {
                    println!("CursorPie browser extension written to {}", dir.display());
                    0
                }
                Err(e) => {
                    eprintln!(
                        "CursorPie: could not write the extension to {}: {e}",
                        dir.display()
                    );
                    1
                }
            })
        }
        "--install-browser-bridge" => {
            let report = super::install::install_current();
            println!("registered for: {}", report.registered.join(", "));
            for e in &report.errors {
                eprintln!("CursorPie: {e}");
            }
            Some(i32::from(!report.errors.is_empty()))
        }
        "--uninstall-browser-bridge" => {
            super::install::uninstall();
            Some(0)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_browser_starting_the_host_is_recognized() {
        // Chromium: the origin of the caller (Windows adds the parent window)
        assert!(wants_host(&args(&["chrome-extension://abcdefgh/"])));
        assert!(wants_host(&args(&[
            "chrome-extension://abcdefgh/",
            "--parent-window=1234"
        ])));
        // Firefox: the path of the host manifest, then the id of the add-on
        assert!(wants_host(&args(&[
            "/home/u/.mozilla/native-messaging-hosts/io.github.cursorpie.bridge.json",
            "browser-bridge@cursorpie"
        ])));
        assert!(wants_host(&args(&[
            r"C:\Users\u\AppData\Roaming\CursorPie\native-messaging\firefox\io.github.cursorpie.bridge.json",
            "browser-bridge@cursorpie"
        ])));
        assert!(wants_host(&args(&["--native-host"])));
    }

    #[test]
    fn the_normal_start_is_not_taken_for_a_host() {
        assert!(!wants_host(&[]));
        assert!(!wants_host(&args(&["--autostart"])));
        assert!(!wants_host(&args(&["--uninstall"])));
        assert_eq!(command_line(&[]), None);
        assert_eq!(command_line(&args(&["--autostart"])), None);
    }
}
