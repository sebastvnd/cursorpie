//! What the blocks of a flow ask of the desktop that no backend function covers: the clipboard,
//! notifications, questions in a window, and a shell command that is waited for.
//!
//! Like [`http`](super::http) and [`web`](super::web) this module has no backend of its own: it is
//! the same code on every OS, built from the programs each system ships (`pbcopy`, PowerShell,
//! `wl-copy`, `xclip`, `osascript`, `zenity`, `notify-send`...), so the list of functions a backend
//! must have stays as it was. The programs are started directly and never through a shell, and a
//! text that comes from a flow (a message, a clipboard text) is handed over as an argument, on
//! standard input or in an environment variable, never pasted into a command line, so no part of
//! it can be read as a command. A program that is not installed makes the call answer `None` or
//! `false`; a flow sees that as `ok = 0`.
//!
//! The decisions (which program, which arguments) are plain functions that return an
//! [`Invocation`], so they are unit tested on every OS.

use super::Os;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Most bytes read from what a program prints.
const MAX_CAPTURE_BYTES: u64 = 256 * 1024;
/// Most bytes of a file that a flow reads.
pub const MAX_READ_BYTES: u64 = 256 * 1024;
/// How long a clipboard or notification program may take.
const QUICK: Duration = Duration::from_secs(5);
/// How long a question may stay open before it counts as cancelled.
const ASK_TIMEOUT: Duration = Duration::from_secs(600);
/// The name windows and notifications carry.
const TITLE: &str = "CursorPie";

/// One program to start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    program: &'static str,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    stdin: Option<String>,
}

impl Invocation {
    fn new(program: &'static str, args: &[&str]) -> Self {
        Self {
            program,
            args: args.iter().map(|a| (*a).to_string()).collect(),
            envs: Vec::new(),
            stdin: None,
        }
    }

    fn env(mut self, key: &str, value: &str) -> Self {
        self.envs.push((key.to_string(), value.to_string()));
        self
    }

    fn stdin(mut self, text: &str) -> Self {
        self.stdin = Some(text.to_string());
        self
    }
}

/// What a program left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// The exit code; `None` when a signal or the timeout ended it.
    pub code: Option<i32>,
    /// What it printed.
    pub stdout: String,
}

// ------------------------------------------------------------------ the programs

/// PowerShell with a script: no profile, no prompt, no window.
fn powershell(script: &str) -> Invocation {
    Invocation::new(
        "powershell.exe",
        &[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ],
    )
}

/// The programs that read the text clipboard, in the order they are tried.
fn clipboard_get_programs(os: Os) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new("pbpaste", &[])],
        Os::Windows => vec![powershell(
            "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-Clipboard -Raw",
        )],
        Os::Linux => vec![
            Invocation::new("wl-paste", &["--no-newline"]),
            Invocation::new("xclip", &["-selection", "clipboard", "-out"]),
            Invocation::new("xsel", &["--clipboard", "--output"]),
        ],
    }
}

/// The programs that put `text` on the clipboard, in the order they are tried.
fn clipboard_set_programs(os: Os, text: &str) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new("pbcopy", &[]).stdin(text)],
        Os::Windows => {
            vec![powershell("Set-Clipboard -Value $env:CURSORPIE_TEXT").env("CURSORPIE_TEXT", text)]
        }
        Os::Linux => vec![
            Invocation::new("wl-copy", &[]).stdin(text),
            Invocation::new("xclip", &["-selection", "clipboard", "-in"]).stdin(text),
            Invocation::new("xsel", &["--clipboard", "--input"]).stdin(text),
        ],
    }
}

/// The programs that show a notification, in the order they are tried.
fn notify_programs(os: Os, title: &str, text: &str) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new(
            "osascript",
            &[
                "-e",
                "on run argv",
                "-e",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "-e",
                "end run",
                title,
                text,
            ],
        )],
        Os::Windows => vec![
            powershell(
                "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
                 $n=New-Object System.Windows.Forms.NotifyIcon; \
                 $n.Icon=[System.Drawing.SystemIcons]::Information; $n.Visible=$true; \
                 $n.ShowBalloonTip(5000,$env:CURSORPIE_TITLE,$env:CURSORPIE_TEXT,[System.Windows.Forms.ToolTipIcon]::Info); \
                 Start-Sleep -Seconds 6; $n.Dispose()",
            )
            .env("CURSORPIE_TITLE", title)
            .env("CURSORPIE_TEXT", text),
        ],
        Os::Linux => vec![Invocation::new(
            "notify-send",
            &["--app-name=CursorPie", "--", title, text],
        )],
    }
}

/// The programs that ask for a text, in the order they are tried.
fn ask_text_programs(os: Os, message: &str, preset: &str) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new(
            "osascript",
            &[
                "-e",
                "on run argv",
                "-e",
                "text returned of (display dialog (item 1 of argv) default answer (item 2 of argv) with title \"CursorPie\")",
                "-e",
                "end run",
                message,
                preset,
            ],
        )],
        Os::Windows => vec![
            powershell(
                "Add-Type -AssemblyName Microsoft.VisualBasic; [Console]::OutputEncoding=[Text.Encoding]::UTF8; \
                 [Microsoft.VisualBasic.Interaction]::InputBox($env:CURSORPIE_MESSAGE,'CursorPie',$env:CURSORPIE_PRESET)",
            )
            .env("CURSORPIE_MESSAGE", message)
            .env("CURSORPIE_PRESET", preset),
        ],
        Os::Linux => vec![
            Invocation::new(
                "zenity",
                &[
                    "--entry",
                    &format!("--title={TITLE}"),
                    &format!("--text={message}"),
                    &format!("--entry-text={preset}"),
                ],
            ),
            Invocation::new(
                "kdialog",
                &["--title", TITLE, "--inputbox", message, preset],
            ),
        ],
    }
}

/// The programs that ask a yes or no question, in the order they are tried.
fn ask_yes_no_programs(os: Os, message: &str) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new(
            "osascript",
            &[
                "-e",
                "on run argv",
                "-e",
                "button returned of (display dialog (item 1 of argv) with title \"CursorPie\" buttons {\"No\", \"Yes\"} default button \"Yes\")",
                "-e",
                "end run",
                message,
            ],
        )],
        Os::Windows => vec![
            powershell(
                "Add-Type -AssemblyName System.Windows.Forms; [Console]::OutputEncoding=[Text.Encoding]::UTF8; \
                 [System.Windows.Forms.MessageBox]::Show($env:CURSORPIE_MESSAGE,'CursorPie','YesNo','Question')",
            )
            .env("CURSORPIE_MESSAGE", message),
        ],
        Os::Linux => vec![
            Invocation::new(
                "zenity",
                &[
                    "--question",
                    &format!("--title={TITLE}"),
                    &format!("--text={message}"),
                ],
            ),
            Invocation::new("kdialog", &["--title", TITLE, "--yesno", message]),
        ],
    }
}

/// The answer of a text question from what the program printed: one line less its line break.
/// `None` for no answer at all (Windows has no way to tell an empty answer from Cancel).
fn text_answer(os: Os, out: &Output) -> Option<String> {
    let text = out.stdout.strip_suffix('\n').unwrap_or(&out.stdout);
    let text = text.strip_suffix('\r').unwrap_or(text);
    match os {
        Os::Windows if text.is_empty() => None,
        _ if out.code != Some(0) => None,
        _ => Some(text.to_string()),
    }
}

/// The answer of a yes or no question from how the program ended.
fn yes_no_answer(os: Os, out: &Output) -> Option<bool> {
    match os {
        Os::Mac | Os::Windows => match out.stdout.trim() {
            "Yes" if out.code == Some(0) => Some(true),
            "No" if out.code == Some(0) => Some(false),
            _ => None,
        },
        Os::Linux => match out.code {
            Some(0) => Some(true),
            Some(1) => Some(false),
            _ => None,
        },
    }
}

// ------------------------------------------------------------------ starting programs

/// Starts `inv`, hands it its input, and waits for it for at most `timeout`. `None` when the
/// program is not installed or cannot be started.
fn run(inv: &Invocation, timeout: Duration, capture: bool) -> Option<Output> {
    let mut cmd = Command::new(inv.program);
    cmd.args(&inv.args);
    finish(cmd, inv, timeout, capture)
}

/// The part of [`run`] that is the same for a program and a shell line: the environment, the
/// pipes, the hidden window, the wait.
fn finish(mut cmd: Command, inv: &Invocation, timeout: Duration, capture: bool) -> Option<Output> {
    cmd.envs(inv.envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(if inv.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashes on screen
    }
    let mut child = cmd.spawn().ok()?;
    if let Some(text) = &inv.stdin
        && let Some(mut pipe) = child.stdin.take()
    {
        let _ = pipe.write_all(text.as_bytes());
        // dropped here: the program sees the end of its input
    }
    // read on a thread of its own, so a program that never closes its output cannot hold us
    let reader = child.stdout.take().map(|out| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = out.take(MAX_CAPTURE_BYTES).read_to_end(&mut bytes);
            let _ = tx.send(bytes);
        });
        rx
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break None,
        }
    };
    let stdout = reader
        .and_then(|rx| rx.recv_timeout(Duration::from_millis(500)).ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    Some(Output {
        code: status.and_then(|s| s.code()),
        stdout,
    })
}

/// Tries each program until one can be started; `None` when none can.
fn first_that_starts(programs: &[Invocation], timeout: Duration, capture: bool) -> Option<Output> {
    programs.iter().find_map(|p| run(p, timeout, capture))
}

/// Starts `inv` without waiting for it (a notification that stays on screen for a while).
fn start_and_forget(inv: &Invocation) -> bool {
    let mut cmd = Command::new(inv.program);
    cmd.args(&inv.args)
        .envs(inv.envs.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

// ------------------------------------------------------------------ the functions blocks use

/// Runs a shell command line and waits for it (`/bin/sh -c`, or `cmd.exe /S /C` without a window),
/// with `env` added to the environment. Gives up after `timeout` and ends the program (`code` is then
/// `None`). `None` when the shell could not be started.
pub fn exec(command: &str, env: &[(String, String)], timeout: Duration) -> Option<Output> {
    let inv = Invocation {
        program: shell_program(Os::current()),
        args: Vec::new(),
        envs: env.to_vec(),
        stdin: None,
    };
    let mut cmd = Command::new(inv.program);
    shell_line(&mut cmd, command);
    finish(cmd, &inv, timeout, true)
}

/// The program that runs shell lines on `os`.
const fn shell_program(os: Os) -> &'static str {
    match os {
        Os::Windows => "cmd.exe",
        Os::Mac | Os::Linux => "/bin/sh",
    }
}

/// Gives `cmd` the arguments that run `line` as typed. On Windows the line is passed as it is
/// (`/S` makes cmd strip only the outer pair of quotes, as the *Command* block does).
#[cfg(windows)]
fn shell_line(cmd: &mut Command, line: &str) {
    use std::os::windows::process::CommandExt;
    cmd.raw_arg(format!("/S /C \"{line}\""));
}

/// Gives `cmd` the arguments that run `line` as typed.
#[cfg(not(windows))]
fn shell_line(cmd: &mut Command, line: &str) {
    cmd.arg("-c").arg(line);
}

/// The text on the clipboard.
pub fn clipboard_get() -> Option<String> {
    let programs = clipboard_get_programs(Os::current());
    let out = programs
        .iter()
        .filter_map(|p| run(p, QUICK, true))
        .find(|o| o.code == Some(0))?;
    Some(out.stdout)
}

/// Puts `text` on the clipboard.
pub fn clipboard_set(text: &str) -> bool {
    let programs = clipboard_set_programs(Os::current(), text);
    programs
        .iter()
        .filter_map(|p| run(p, QUICK, false))
        .any(|o| o.code == Some(0))
}

/// Shows a notification; whether a program could be started to show it.
pub fn notify(title: &str, text: &str) -> bool {
    notify_programs(Os::current(), title, text)
        .iter()
        .any(start_and_forget)
}

/// Asks for a text in a window and waits for the answer. `None` when it was cancelled or no program
/// could show it.
pub fn ask_text(message: &str, preset: &str) -> Option<String> {
    let os = Os::current();
    let out = first_that_starts(&ask_text_programs(os, message, preset), ASK_TIMEOUT, true)?;
    text_answer(os, &out)
}

/// Asks a yes or no question and waits for the answer. `None` when no program could show it.
pub fn ask_yes_no(message: &str) -> Option<bool> {
    let os = Os::current();
    let out = first_that_starts(&ask_yes_no_programs(os, message), ASK_TIMEOUT, true)?;
    yes_no_answer(os, &out)
}

/// `path` with a leading `~/` turned into the home folder.
fn expand_home(path: &str) -> std::path::PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => crate::config::home().join(rest),
        None => std::path::PathBuf::from(path),
    }
}

/// The text of a file, if it is UTF-8 text of at most [`MAX_READ_BYTES`].
pub fn read_file(path: &str) -> Option<String> {
    if path.trim().is_empty() {
        return None;
    }
    crate::config::read_text_capped(expand_home(path.trim()), MAX_READ_BYTES)
}

/// Writes `text` to a file, or adds it at the end with `append`. The file is created when it is
/// missing; its folder is not.
pub fn write_file(path: &str, text: &str, append: bool) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true);
    if append {
        options.append(true);
    } else {
        options.write(true).truncate(true);
    }
    options
        .open(expand_home(path.trim()))
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .is_ok()
}

/// Whether a file or folder exists.
pub fn file_exists(path: &str) -> bool {
    !path.trim().is_empty() && expand_home(path.trim()).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: Option<i32>, stdout: &str) -> Output {
        Output {
            code,
            stdout: stdout.into(),
        }
    }

    #[test]
    fn a_text_from_a_flow_never_becomes_part_of_a_command_line() {
        let evil = "x'; rm -rf ~ #";
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let all = [
                clipboard_set_programs(os, evil),
                notify_programs(os, evil, evil),
                ask_text_programs(os, evil, evil),
                ask_yes_no_programs(os, evil),
            ];
            for inv in all.iter().flatten() {
                // the program is a fixed name; the text is an argument, input or environment
                assert!(!inv.program.contains(evil));
                assert!(
                    inv.args.iter().all(|a| a == evil
                        || a.starts_with("--")
                        || !a.contains(evil)
                        || a.ends_with(evil)),
                    "{inv:?}"
                );
            }
        }
    }

    #[test]
    fn on_windows_the_text_travels_in_the_environment_not_in_the_script() {
        let set = &clipboard_set_programs(Os::Windows, "SECRET")[0];
        assert!(set.args.iter().all(|a| !a.contains("SECRET")));
        assert_eq!(
            set.envs,
            vec![("CURSORPIE_TEXT".to_string(), "SECRET".to_string())]
        );
        let ask = &ask_text_programs(Os::Windows, "m", "p")[0];
        assert_eq!(ask.envs.len(), 2);
    }

    #[test]
    fn the_clipboard_is_set_through_standard_input_where_there_is_a_program_for_it() {
        let mac = &clipboard_set_programs(Os::Mac, "hi")[0];
        assert_eq!((mac.program, mac.stdin.as_deref()), ("pbcopy", Some("hi")));
        let linux = clipboard_set_programs(Os::Linux, "hi");
        assert_eq!(
            linux.iter().map(|i| i.program).collect::<Vec<_>>(),
            ["wl-copy", "xclip", "xsel"],
            "Wayland first, then the X11 tools"
        );
        assert!(linux.iter().all(|i| i.stdin.as_deref() == Some("hi")));
        assert_eq!(clipboard_get_programs(Os::Mac)[0].program, "pbpaste");
        assert_eq!(clipboard_get_programs(Os::Linux).len(), 3);
    }

    #[test]
    fn a_title_that_looks_like_an_option_stays_a_title() {
        let n = &notify_programs(Os::Linux, "--help", "-x")[0];
        let at = n.args.iter().position(|a| a == "--").unwrap();
        assert_eq!(n.args[at + 1], "--help");
    }

    #[test]
    fn answers_are_read_from_what_the_programs_print_and_how_they_end() {
        assert_eq!(
            text_answer(Os::Mac, &out(Some(0), "Ann\n")),
            Some("Ann".to_string())
        );
        assert_eq!(
            text_answer(Os::Mac, &out(Some(0), "\n")),
            Some(String::new())
        );
        assert_eq!(text_answer(Os::Linux, &out(Some(1), "")), None, "cancelled");
        assert_eq!(text_answer(Os::Linux, &out(None, "x")), None, "timed out");
        assert_eq!(text_answer(Os::Windows, &out(Some(0), "\r\n")), None);
        assert_eq!(
            text_answer(Os::Windows, &out(Some(0), "ok\r\n")),
            Some("ok".to_string())
        );
        assert_eq!(yes_no_answer(Os::Mac, &out(Some(0), "Yes\n")), Some(true));
        assert_eq!(
            yes_no_answer(Os::Windows, &out(Some(0), "No\r\n")),
            Some(false)
        );
        assert_eq!(yes_no_answer(Os::Mac, &out(Some(1), "")), None);
        assert_eq!(yes_no_answer(Os::Linux, &out(Some(0), "")), Some(true));
        assert_eq!(yes_no_answer(Os::Linux, &out(Some(1), "")), Some(false));
        assert_eq!(yes_no_answer(Os::Linux, &out(Some(5), "")), None);
    }

    #[test]
    fn a_program_that_is_not_installed_is_reported_as_such() {
        let inv = Invocation::new("cursorpie-no-such-program-0", &[]);
        assert_eq!(run(&inv, Duration::from_secs(1), true), None);
        assert_eq!(
            first_that_starts(&[inv], Duration::from_secs(1), true),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_shell_command_is_waited_for_and_its_output_and_exit_code_are_kept() {
        let env = [("CURSORPIE_X".to_string(), "from env".to_string())];
        let r = exec(
            "echo \"$CURSORPIE_X\"; exit 3",
            &env,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(r.code, Some(3));
        assert_eq!(r.stdout, "from env\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_command_that_takes_too_long_is_ended() {
        let t = Instant::now();
        let r = exec("sleep 5", &[], Duration::from_millis(200)).unwrap();
        assert_eq!(r.code, None);
        assert!(t.elapsed() < Duration::from_secs(4));
    }

    #[cfg(unix)]
    #[test]
    fn what_a_command_prints_is_cut_at_the_limit() {
        let r = exec(
            "head -c 400000 /dev/zero | tr '\\0' a",
            &[],
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(r.stdout.len() as u64 <= MAX_CAPTURE_BYTES);
    }

    #[test]
    fn files_are_written_appended_read_and_found() {
        let dir = std::env::temp_dir().join(format!("cursorpie-native-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.txt");
        let p = p.to_str().unwrap();
        assert!(!file_exists(p));
        assert!(write_file(p, "one", false));
        assert!(write_file(p, "+two", true));
        assert_eq!(read_file(p).as_deref(), Some("one+two"));
        assert!(write_file(p, "new", false));
        assert_eq!(read_file(p).as_deref(), Some("new"));
        assert!(file_exists(p));
        assert!(!write_file("", "x", false));
        assert!(!write_file(
            dir.join("no/such/dir/f").to_str().unwrap(),
            "x",
            false
        ));
        assert_eq!(read_file(""), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_may_start_with_the_home_folder() {
        assert_eq!(expand_home("/etc/x"), std::path::PathBuf::from("/etc/x"));
        assert_eq!(expand_home("~/a/b"), crate::config::home().join("a/b"));
        assert_eq!(expand_home("~x"), std::path::PathBuf::from("~x"));
    }

    #[test]
    fn the_shell_of_each_os_is_the_one_the_command_block_uses() {
        assert_eq!(shell_program(Os::Windows), "cmd.exe");
        assert_eq!(shell_program(Os::Mac), "/bin/sh");
        assert_eq!(shell_program(Os::Linux), "/bin/sh");
    }
}
