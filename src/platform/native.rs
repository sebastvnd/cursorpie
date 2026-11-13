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
/// How long a text may take to be spoken, or a screen to be saved.
const SPEAK_TIMEOUT: Duration = Duration::from_secs(120);
/// Most names a folder listing keeps.
const MAX_LIST_NAMES: usize = 5000;

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

/// A `file://` address for the absolute `path`: everything but letters, digits and `-_.~/` is
/// written as `%XX` of its UTF-8 bytes, which is how file managers and browsers read a copied file.
fn file_uri(path: &str) -> String {
    let mut uri = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'/') {
            uri.push(char::from(b));
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

/// The programs that put the *file* at `path` on the clipboard (not its name as text), so that
/// pasting in a chat window or a file manager attaches or copies it, in the order they are tried.
/// The path goes in as an argument, an environment variable or an address, never into a script.
fn clipboard_set_file_programs(os: Os, path: &str) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new(
            "osascript",
            &[
                "-e",
                "on run argv",
                "-e",
                "set the clipboard to (POSIX file (item 1 of argv))",
                "-e",
                "end run",
                path,
            ],
        )],
        Os::Windows => vec![
            powershell("Set-Clipboard -LiteralPath $env:CURSORPIE_FILE")
                .env("CURSORPIE_FILE", path),
        ],
        Os::Linux => {
            let list = format!("{}\r\n", file_uri(path));
            vec![
                Invocation::new("wl-copy", &["--type", "text/uri-list"]).stdin(&list),
                Invocation::new(
                    "xclip",
                    &["-selection", "clipboard", "-t", "text/uri-list", "-in"],
                )
                .stdin(&list),
            ]
        }
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

/// The programs that speak `text`, in the order they are tried. The text is standard input or an
/// environment variable (after `--` on Linux), never part of a command.
fn say_programs(os: Os, text: &str) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new("say", &[]).stdin(text)],
        Os::Windows => vec![
            powershell(
                "Add-Type -AssemblyName System.Speech; \
                 $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; $s.Speak($env:CURSORPIE_TEXT)",
            )
            .env("CURSORPIE_TEXT", text),
        ],
        Os::Linux => vec![
            Invocation::new("spd-say", &["--wait", "--", text]),
            Invocation::new("espeak-ng", &["--stdin"]).stdin(text),
            Invocation::new("espeak", &["--stdin"]).stdin(text),
        ],
    }
}

/// The programs that save a picture of the screen as the PNG file `path` (which does not start
/// with `-`), in the order they are tried.
fn screenshot_programs(os: Os, path: &str) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new("screencapture", &["-x", path])],
        Os::Windows => vec![
            powershell(
                "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
                 $b=[System.Windows.Forms.SystemInformation]::VirtualScreen; \
                 $bmp=New-Object System.Drawing.Bitmap $b.Width,$b.Height; \
                 $g=[System.Drawing.Graphics]::FromImage($bmp); \
                 $g.CopyFromScreen($b.Location,[System.Drawing.Point]::Empty,$b.Size); \
                 $bmp.Save($env:CURSORPIE_PATH,[System.Drawing.Imaging.ImageFormat]::Png)",
            )
            .env("CURSORPIE_PATH", path),
        ],
        Os::Linux => vec![
            Invocation::new("grim", &[path]),
            Invocation::new("gnome-screenshot", &["-f", path]),
            Invocation::new("spectacle", &["-b", "-n", "-o", path]),
            Invocation::new("scrot", &["-o", path]),
            Invocation::new("import", &["-window", "root", path]),
        ],
    }
}

/// The programs that print the volume of the speakers, in the order they are tried. Windows has
/// none: it has no program that does this without installing one.
fn volume_get_programs(os: Os) -> Vec<Invocation> {
    match os {
        Os::Mac => vec![Invocation::new(
            "osascript",
            &["-e", "output volume of (get volume settings)"],
        )],
        Os::Windows => Vec::new(),
        Os::Linux => vec![
            Invocation::new("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"]),
            Invocation::new("pactl", &["get-sink-volume", "@DEFAULT_SINK@"]),
            Invocation::new("amixer", &["get", "Master"]),
        ],
    }
}

/// The programs that set the volume of the speakers to `percent` (0 to 100), in the order they
/// are tried.
fn volume_set_programs(os: Os, percent: u8) -> Vec<Invocation> {
    let percent = percent.min(100);
    match os {
        Os::Mac => vec![Invocation::new(
            "osascript",
            &[
                "-e",
                "on run argv",
                "-e",
                "set volume output volume ((item 1 of argv) as integer)",
                "-e",
                "end run",
                &percent.to_string(),
            ],
        )],
        Os::Windows => Vec::new(),
        Os::Linux => vec![
            Invocation::new(
                "wpctl",
                &[
                    "set-volume",
                    "@DEFAULT_AUDIO_SINK@",
                    &format!("{:.2}", f64::from(percent) / 100.0),
                ],
            ),
            Invocation::new(
                "pactl",
                &["set-sink-volume", "@DEFAULT_SINK@", &format!("{percent}%")],
            ),
            Invocation::new("amixer", &["set", "Master", &format!("{percent}%")]),
        ],
    }
}

/// The volume (0 to 100) in what a volume program printed: a plain number (`osascript`), the
/// first `NN%` (`pactl`, `amixer`) or `Volume: 0.NN` (`wpctl`).
fn parse_volume(text: &str) -> Option<u8> {
    let t = text.trim();
    if let Ok(n) = t.parse::<f64>() {
        return n.is_finite().then(|| n.round().clamp(0.0, 100.0) as u8);
    }
    if let Some(at) = t.find('%') {
        let digits: String = t[..at]
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if let Ok(n) = digits.parse::<u32>() {
            return Some(n.min(100) as u8);
        }
    }
    let after = t.split("Volume:").nth(1)?;
    let n: f64 = after.split_whitespace().next()?.parse().ok()?;
    n.is_finite()
        .then(|| (n * 100.0).round().clamp(0.0, 100.0) as u8)
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
fn finish(cmd: Command, inv: &Invocation, timeout: Duration, capture: bool) -> Option<Output> {
    finish_until(cmd, inv, timeout, capture, &|| false)
}

/// [`finish`] that also ends the program when `stop` says so.
fn finish_until(
    mut cmd: Command,
    inv: &Invocation,
    timeout: Duration,
    capture: bool,
    stop: &dyn Fn() -> bool,
) -> Option<Output> {
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
            Ok(None) if started.elapsed() >= timeout || stop() => {
                kill_tree(&mut child);
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

/// Ends `child` and, on Windows, the programs it started. `cmd.exe /C` runs the person's command
/// as a grandchild: killing only the shell leaves that program holding the end of the output pipe,
/// so the thread that reads it (see [`finish`]) would wait for ever, with its buffer, for every
/// command that timed out.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
    }
    let _ = child.kill();
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
    exec_until(command, env, timeout, &|| false)
}

/// [`exec`] that also gives up, and ends the program (`code` is then `None`), as soon as `stop`
/// says so. It is asked about every 10 ms while the program runs.
pub fn exec_until(
    command: &str,
    env: &[(String, String)],
    timeout: Duration,
    stop: &dyn Fn() -> bool,
) -> Option<Output> {
    let inv = Invocation {
        program: shell_program(Os::current()),
        args: Vec::new(),
        envs: env.to_vec(),
        stdin: None,
    };
    let mut cmd = Command::new(inv.program);
    shell_line(&mut cmd, command);
    finish_until(cmd, &inv, timeout, true, stop)
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

/// Puts the file at `path` on the clipboard as a file, so it can be pasted into a chat window or a
/// file manager. `false` when no program could do it (on Linux that needs `wl-copy` or `xclip`) or
/// the path is not text.
pub fn clipboard_set_file(path: &std::path::Path) -> bool {
    let Some(path) = path.to_str() else {
        return false;
    };
    let programs = clipboard_set_file_programs(Os::current(), path);
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

/// Speaks `text` aloud and waits until it is said; whether a program did.
pub fn say(text: &str) -> bool {
    if text.trim().is_empty() {
        return false;
    }
    say_programs(Os::current(), text)
        .iter()
        .filter_map(|p| run(p, SPEAK_TIMEOUT, false))
        .any(|o| o.code == Some(0))
}

/// Saves a picture of the screen as the PNG file `path`; whether a program did.
pub fn screenshot(path: &str) -> bool {
    let path = path.trim();
    // a path that looks like an option must not reach a program as one
    if path.is_empty() || path.starts_with('-') || !path.to_ascii_lowercase().ends_with(".png") {
        return false;
    }
    let target = expand_home(path);
    let target = target.to_string_lossy();
    screenshot_programs(Os::current(), &target)
        .iter()
        .filter_map(|p| run(p, SPEAK_TIMEOUT, false))
        .any(|o| o.code == Some(0))
}

/// The volume of the speakers, 0 to 100; `None` when no program can tell.
pub fn volume() -> Option<u8> {
    volume_get_programs(Os::current())
        .iter()
        .filter_map(|p| run(p, QUICK, true))
        .filter(|o| o.code == Some(0))
        .find_map(|o| parse_volume(&o.stdout))
}

/// Sets the volume of the speakers; whether a program did.
pub fn set_volume(percent: u8) -> bool {
    volume_set_programs(Os::current(), percent)
        .iter()
        .filter_map(|p| run(p, QUICK, false))
        .any(|o| o.code == Some(0))
}

/// The names (not the paths) of what is in the folder `path`, sorted; `None` when it cannot be
/// read. At most `MAX_LIST_NAMES` names.
pub fn list_dir(path: &str) -> Option<Vec<String>> {
    if path.trim().is_empty() {
        return None;
    }
    let mut names: Vec<String> = std::fs::read_dir(expand_home(path.trim()))
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names.truncate(MAX_LIST_NAMES);
    Some(names)
}

/// Makes the folder `path` and the ones above it that are missing.
pub fn make_dir(path: &str) -> bool {
    !path.trim().is_empty() && std::fs::create_dir_all(expand_home(path.trim())).is_ok()
}

/// Copies the file `from` to `to`, replacing what is there. A folder is not copied.
pub fn copy_file(from: &str, to: &str) -> bool {
    if from.trim().is_empty() || to.trim().is_empty() {
        return false;
    }
    let (from, to) = (expand_home(from.trim()), expand_home(to.trim()));
    from.is_file() && std::fs::copy(from, to).is_ok()
}

/// Moves or renames `from` to `to`. Across disks a file is copied and then removed.
pub fn move_file(from: &str, to: &str) -> bool {
    if from.trim().is_empty() || to.trim().is_empty() {
        return false;
    }
    let (from, to) = (expand_home(from.trim()), expand_home(to.trim()));
    if std::fs::rename(&from, &to).is_ok() {
        return true;
    }
    from.is_file() && std::fs::copy(&from, &to).is_ok() && std::fs::remove_file(&from).is_ok()
}

/// Deletes the file `path`. A folder is never deleted, and neither is a link to one.
pub fn delete_file(path: &str) -> bool {
    if path.trim().is_empty() {
        return false;
    }
    let path = expand_home(path.trim());
    match std::fs::symlink_metadata(&path) {
        Ok(meta) if !meta.file_type().is_dir() => std::fs::remove_file(&path).is_ok(),
        _ => false,
    }
}

/// The size in bytes and the time of the last change (Unix seconds) of `path`.
pub fn file_info(path: &str) -> Option<(u64, u64)> {
    if path.trim().is_empty() {
        return None;
    }
    let meta = std::fs::metadata(expand_home(path.trim())).ok()?;
    let time = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    Some((meta.len(), time))
}

/// A fact about this computer: `os`, `arch`, `user`, `host`, `home` or `temp`.
pub fn system_info(key: &str) -> Option<String> {
    let env = |names: &[&str]| {
        names
            .iter()
            .find_map(|n| std::env::var(n).ok().filter(|v| !v.trim().is_empty()))
    };
    match key {
        "os" => Some(std::env::consts::OS.to_string()),
        "arch" => Some(std::env::consts::ARCH.to_string()),
        "user" => env(&["USER", "USERNAME", "LOGNAME"]),
        "host" => env(&["HOSTNAME", "COMPUTERNAME"]).or_else(|| {
            run(&Invocation::new("hostname", &[]), QUICK, true)
                .filter(|o| o.code == Some(0))
                .map(|o| o.stdout.trim().to_string())
                .filter(|h| !h.is_empty())
        }),
        "home" => Some(crate::config::home().to_string_lossy().into_owned()),
        "temp" => Some(std::env::temp_dir().to_string_lossy().into_owned()),
        _ => None,
    }
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

    #[test]
    fn a_file_address_encodes_what_a_path_may_hold() {
        assert_eq!(file_uri("/home/a/x.zip"), "file:///home/a/x.zip");
        assert_eq!(
            file_uri("/Users/Ana Maria/Library/App Support/x'y.zip"),
            "file:///Users/Ana%20Maria/Library/App%20Support/x%27y.zip"
        );
        assert_eq!(file_uri("/caf\u{e9}"), "file:///caf%C3%A9");
    }

    #[test]
    fn the_file_clipboard_programs_never_put_the_path_in_a_script() {
        let evil = "/tmp/x'; rm -rf ~ #.zip";
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let all = clipboard_set_file_programs(os, evil);
            assert!(!all.is_empty(), "{os:?}");
            for inv in &all {
                assert!(!inv.program.contains(evil));
                // a script is fixed text: the path is the last argument, an environment value or
                // the encoded address on standard input
                assert!(
                    inv.args.iter().all(|a| !a.contains(evil) || a == evil),
                    "{inv:?}"
                );
                if let Some(input) = &inv.stdin {
                    assert!(!input.contains("rm -rf"), "{inv:?}");
                }
            }
        }
    }

    #[test]
    fn the_file_goes_on_the_clipboard_as_a_file_not_as_text() {
        let mac = clipboard_set_file_programs(Os::Mac, "/tmp/k.zip");
        assert!(mac[0].args.iter().any(|a| a.contains("POSIX file")));
        let win = clipboard_set_file_programs(Os::Windows, "C:\\k.zip");
        assert!(
            win[0]
                .args
                .iter()
                .any(|a| a.contains("Set-Clipboard -LiteralPath"))
        );
        let linux = clipboard_set_file_programs(Os::Linux, "/tmp/k.zip");
        assert!(
            linux
                .iter()
                .all(|i| i.args.contains(&"text/uri-list".to_string()))
        );
        assert_eq!(linux[0].stdin.as_deref(), Some("file:///tmp/k.zip\r\n"));
    }

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
    fn speaking_and_screenshots_never_put_a_text_or_a_path_in_a_command() {
        let evil = "x'; rm -rf ~ #";
        for os in [Os::Mac, Os::Windows, Os::Linux] {
            let all = [say_programs(os, evil), screenshot_programs(os, evil)];
            for inv in all.iter().flatten() {
                assert!(!inv.program.contains(evil));
                assert!(
                    inv.args
                        .iter()
                        .all(|a| a == evil || a.starts_with('-') || !a.contains(evil)),
                    "{inv:?}"
                );
            }
        }
        // Windows: in the environment, not in the script
        let spoken = &say_programs(Os::Windows, "SECRET")[0];
        assert!(spoken.args.iter().all(|a| !a.contains("SECRET")));
        assert_eq!(
            spoken.envs,
            vec![("CURSORPIE_TEXT".to_string(), "SECRET".to_string())]
        );
        let shot = &screenshot_programs(Os::Windows, "C:/s.png")[0];
        assert_eq!(
            shot.envs,
            vec![("CURSORPIE_PATH".to_string(), "C:/s.png".to_string())]
        );
        // a path that looks like an option, or is not a PNG, is refused before anything starts
        assert!(!screenshot("-o"));
        assert!(!screenshot("/tmp/x.jpg"));
        assert!(!screenshot("  "));
        assert!(!say("   "));
        // the speech goes in through standard input on a Mac
        assert_eq!(say_programs(Os::Mac, "hi")[0].stdin.as_deref(), Some("hi"));
    }

    #[test]
    fn the_volume_is_read_from_what_each_program_prints() {
        assert_eq!(parse_volume("50\n"), Some(50));
        assert_eq!(parse_volume("Volume: 0.35\n"), Some(35));
        assert_eq!(parse_volume("Volume: 0.35 [MUTED]"), Some(35));
        assert_eq!(
            parse_volume(
                "Volume: front-left: 32768 /  50% / -18.06 dB,   front-right: 32768 /  50% / -18.06 dB"
            ),
            Some(50)
        );
        assert_eq!(
            parse_volume("  Front Left: Playback 40 [62%] [on]"),
            Some(62)
        );
        assert_eq!(parse_volume("nothing"), None);
        assert_eq!(parse_volume("250"), Some(100));
        assert!(volume_get_programs(Os::Windows).is_empty());
        let set = &volume_set_programs(Os::Linux, 75);
        assert_eq!(set[0].args.last().map(String::as_str), Some("0.75"));
        assert_eq!(set[1].args.last().map(String::as_str), Some("75%"));
        assert_eq!(
            volume_set_programs(Os::Mac, 200)[0]
                .args
                .last()
                .map(String::as_str),
            Some("100")
        );
    }

    #[test]
    fn files_and_folders_are_handled_with_plain_std_and_never_a_folder_deleted() {
        // a folder of its own: the tests run side by side and one of them deletes its folder
        let dir =
            std::env::temp_dir().join(format!("cursorpie-native-dirs-{}", std::process::id()));
        let d = dir.to_string_lossy().into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(make_dir(&format!("{d}/a/b")));
        assert!(write_file(&format!("{d}/a/one.txt"), "hello", false));
        assert_eq!(file_info(&format!("{d}/a/one.txt")).map(|i| i.0), Some(5));
        assert_eq!(
            list_dir(&format!("{d}/a")),
            Some(vec!["b".to_string(), "one.txt".to_string()])
        );
        assert!(copy_file(
            &format!("{d}/a/one.txt"),
            &format!("{d}/a/two.txt")
        ));
        assert!(move_file(
            &format!("{d}/a/two.txt"),
            &format!("{d}/a/b/three.txt")
        ));
        assert_eq!(
            read_file(&format!("{d}/a/b/three.txt")).as_deref(),
            Some("hello")
        );
        // a folder is not copied, and is never deleted
        assert!(!copy_file(&format!("{d}/a/b"), &format!("{d}/c")));
        assert!(!delete_file(&format!("{d}/a/b")));
        assert!(delete_file(&format!("{d}/a/b/three.txt")));
        assert!(!delete_file(&format!("{d}/a/b/three.txt")), "already gone");
        assert!(list_dir(&format!("{d}/missing")).is_none());
        assert!(file_info(&format!("{d}/missing")).is_none());
        assert!(!make_dir("  "));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_facts_about_this_computer_have_a_fixed_list() {
        assert_eq!(system_info("os").as_deref(), Some(std::env::consts::OS));
        assert_eq!(system_info("arch").as_deref(), Some(std::env::consts::ARCH));
        assert!(system_info("temp").is_some());
        assert!(system_info("home").is_some());
        assert!(system_info("colour").is_none());
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
