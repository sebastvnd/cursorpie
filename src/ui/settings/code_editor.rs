//! A small code editor for the text fields of a flow that hold a command (the *Shell command* and
//! *Run a command* blocks), a JSON text (the panel of an *Ask* block) or a value with
//! `{{variables}}` and `{{= expressions}}` (the *Set* block).
//!
//! What it adds to a plain text box:
//!
//! * colors for the words of a shell command (keywords, well-known commands, flags, strings,
//!   `$variables`, comments) or of JSON, and a tinted `{{variable}}` of the flow wherever it stands;
//! * a list of suggestions under the cursor: the commands of the OS, shell keywords with snippets
//!   (`if`, `for`, `while`...), `$ENV` names, the flags of the common commands, the variables of the
//!   flow in an argument (as `{{name|sh}}`, quoted for the shell) and, inside `{{`, the variables,
//!   the filters (`{{name|sh}}`) and the functions of `{{= }}`. Words are matched by their start
//!   first, then by a word inside them, then by their letters in order. It opens while typing (from
//!   the second letter on) and with Ctrl + Space (Cmd + Space too, though macOS keeps that one for
//!   Spotlight); Tab accepts, Enter accepts only after Up / Down chose an entry (otherwise Enter is
//!   always a new line), Esc closes it (and only it: the window's own Esc waits, see
//!   [`escape_is_taken`]);
//! * numbered lines once the text has more than one, and a red underline for a filter or a
//!   function that does not exist;
//! * Tab and Shift + Tab indent and outdent (a whole selection too), Enter keeps the indent (and
//!   opens a block after `then`, `do`, `{`...) and Ctrl/Cmd + / comments lines out. Brackets and
//!   quotes are typed as they are: nothing is added behind the person's back.
//!
//! The text logic (the tokenizer, the suggestions, the edits) works on plain strings and
//! **character** indexes, the way egui counts the cursor, and has no egui in it, so it is tested
//! without a window. [`code_edit`] is the one widget; [`set_variables`] tells it the variables of
//! the flow being edited.

use crate::config::Flow;
use crate::config::flow_check::{block_reads, block_writes};
use crate::platform::Os;
use crate::ui::settings::theme::pal;
use eframe::egui::{
    self, Align2, Color32, FontId, Modifiers, Sense, Stroke,
    text::{CCursor, CCursorRange},
    vec2,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// What the text is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Shell,
    Json,
    /// A text of `{{variables}}` and `{{= expressions}}` and nothing else (the value of *Set*).
    Value,
}

// ------------------------------------------------------------------------------------ tokens

/// The kind of a piece of text, for its color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Plain,
    Keyword,
    Command,
    Str,
    Comment,
    Var,
    Template,
    Op,
    Number,
    Flag,
    Key,
    /// A filter or a function that does not exist.
    Bad,
}

/// A piece of the text, in **bytes**. The pieces never overlap and are in order; what no piece
/// covers is plain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub tok: Tok,
}

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "select", "time", "return", "exit", "break", "continue", "export", "local",
    "unset", "set",
];
/// Keywords that start a command after them (`then echo hi`).
const OPENS_COMMAND: &[&str] = &["if", "then", "do", "else", "elif", "while", "until", "time"];
/// Keywords that are keywords wherever they stand (`for x in a b`).
const MID_KEYWORDS: &[&str] = &["in", "then", "do", "done", "fi", "esac", "else"];

/// Commands that are colored and suggested on macOS and Linux: `(name, what it does)`.
const UNIX_COMMANDS: &[(&str, &str)] = &[
    ("echo", "print text"),
    ("printf", "print formatted text"),
    ("cd", "change folder"),
    ("pwd", "current folder"),
    ("ls", "list files"),
    ("cat", "print a file"),
    ("grep", "search text"),
    ("sed", "edit a stream"),
    ("awk", "process columns"),
    ("find", "find files"),
    ("xargs", "build commands from input"),
    ("sort", "sort lines"),
    ("uniq", "drop repeated lines"),
    ("head", "first lines"),
    ("tail", "last lines"),
    ("wc", "count lines and words"),
    ("cut", "cut columns"),
    ("tr", "translate characters"),
    ("tee", "copy a stream to a file"),
    ("mkdir", "make a folder"),
    ("rm", "remove files"),
    ("cp", "copy files"),
    ("mv", "move or rename"),
    ("touch", "create a file"),
    ("chmod", "change permissions"),
    ("curl", "request a URL"),
    ("wget", "download a URL"),
    ("git", "version control"),
    ("open", "open a file or app (macOS)"),
    ("xdg-open", "open a file or URL (Linux)"),
    ("pbcopy", "copy to the clipboard (macOS)"),
    ("pbpaste", "paste from the clipboard (macOS)"),
    ("xclip", "clipboard (X11)"),
    ("wl-copy", "copy to the clipboard (Wayland)"),
    ("osascript", "run AppleScript (macOS)"),
    ("notify-send", "show a notification (Linux)"),
    ("say", "speak text (macOS)"),
    ("sleep", "wait some seconds"),
    ("kill", "end a process"),
    ("pkill", "end processes by name"),
    ("date", "date and time"),
    ("env", "environment"),
    ("source", "run a script here"),
    ("eval", "run text as a command"),
    ("exec", "replace the shell"),
    ("test", "check a condition"),
    ("read", "read a line"),
    ("true", "do nothing, succeed"),
    ("false", "do nothing, fail"),
    ("python3", "run Python"),
    ("node", "run Node.js"),
    ("jq", "work with JSON"),
    ("zip", "make an archive"),
    ("unzip", "extract an archive"),
    ("tar", "archive files"),
    ("ln", "make a link"),
    ("rmdir", "remove an empty folder"),
    ("chown", "change owner"),
    ("pgrep", "find processes by name"),
    ("ps", "list processes"),
    ("ssh", "remote shell"),
    ("scp", "copy over ssh"),
    ("rsync", "sync folders"),
    ("npm", "Node packages"),
    ("playerctl", "control media players (Linux)"),
    ("brightnessctl", "screen brightness (Linux)"),
    ("pactl", "PulseAudio control (Linux)"),
    ("wpctl", "PipeWire control (Linux)"),
    ("amixer", "ALSA mixer (Linux)"),
    ("hyprctl", "control Hyprland (Linux)"),
    ("swaymsg", "control Sway (Linux)"),
    ("grim", "take a screenshot (Wayland)"),
    ("slurp", "select a region (Wayland)"),
    ("wl-paste", "paste from the clipboard (Wayland)"),
];

/// The flags of a few commands: `(command, [(flag, what it does)])`. Offered for the word that
/// starts with `-` in an argument of that command (not on Windows).
const FLAGS: &[(&str, &[(&str, &str)])] = &[
    (
        "ls",
        &[
            ("-l", "long format"),
            ("-a", "include hidden"),
            ("-h", "human sizes"),
            ("-t", "newest first"),
        ],
    ),
    (
        "grep",
        &[
            ("-i", "ignore case"),
            ("-r", "recursive"),
            ("-n", "line numbers"),
            ("-v", "invert match"),
            ("-E", "extended regex"),
            ("-q", "quiet, only the exit code"),
            ("-l", "only file names"),
        ],
    ),
    (
        "curl",
        &[
            ("-s", "silent"),
            ("-S", "show errors"),
            ("-L", "follow redirects"),
            ("-o", "write to a file"),
            ("-X", "request method"),
            ("-H", "add a header"),
            ("-d", "send data"),
            ("-f", "fail on HTTP errors"),
            ("--max-time", "give up after seconds"),
        ],
    ),
    (
        "rm",
        &[
            ("-r", "recursive"),
            ("-f", "never ask"),
            ("-i", "ask before each"),
        ],
    ),
    (
        "cp",
        &[
            ("-r", "recursive"),
            ("-a", "keep everything"),
            ("-v", "say what is done"),
        ],
    ),
    (
        "mv",
        &[
            ("-f", "overwrite"),
            ("-n", "do not overwrite"),
            ("-v", "say what is done"),
        ],
    ),
    ("mkdir", &[("-p", "also the parents")]),
    (
        "tar",
        &[
            ("-czf", "make a .tar.gz"),
            ("-xzf", "extract a .tar.gz"),
            ("-tf", "list"),
            ("-C", "change to a folder"),
        ],
    ),
    (
        "sort",
        &[
            ("-n", "numbers"),
            ("-r", "reverse"),
            ("-u", "unique"),
            ("-k", "by column"),
        ],
    ),
    ("head", &[("-n", "number of lines")]),
    ("tail", &[("-n", "number of lines"), ("-f", "follow")]),
    (
        "find",
        &[
            ("-name", "match a name"),
            ("-type", "f or d"),
            ("-mtime", "modified days ago"),
            ("-maxdepth", "how deep"),
        ],
    ),
    (
        "sed",
        &[
            ("-i", "edit in place"),
            ("-n", "no auto print"),
            ("-E", "extended regex"),
        ],
    ),
    (
        "notify-send",
        &[
            ("-u", "urgency"),
            ("-t", "milliseconds shown"),
            ("-i", "icon"),
        ],
    ),
    ("jq", &[("-r", "raw output"), ("-c", "compact")]),
    ("kill", &[("-9", "force"), ("-15", "ask to end")]),
];

/// Commands that are colored and suggested on Windows (PowerShell): `(name, what it does)`.
const WINDOWS_COMMANDS: &[(&str, &str)] = &[
    ("Write-Output", "print text"),
    ("Write-Host", "print to the console"),
    ("Get-Content", "read a file"),
    ("Set-Content", "write a file"),
    ("Add-Content", "append to a file"),
    ("Get-ChildItem", "list files"),
    ("Set-Location", "change folder"),
    ("Copy-Item", "copy files"),
    ("Move-Item", "move or rename"),
    ("Remove-Item", "remove files"),
    ("New-Item", "create a file or folder"),
    ("Test-Path", "does a path exist"),
    ("Start-Process", "start a program"),
    ("Start-Sleep", "wait"),
    ("Stop-Process", "end a process"),
    ("Get-Process", "list processes"),
    ("Get-Date", "date and time"),
    ("Get-Clipboard", "read the clipboard"),
    ("Set-Clipboard", "write the clipboard"),
    ("Invoke-WebRequest", "request a URL"),
    ("Invoke-RestMethod", "request a JSON API"),
    ("ConvertFrom-Json", "read JSON"),
    ("ConvertTo-Json", "write JSON"),
    ("Select-Object", "pick properties"),
    ("Where-Object", "filter"),
    ("ForEach-Object", "do for each"),
    ("explorer", "open Explorer"),
    ("git", "version control"),
];

/// A name that goes after `$` on macOS and Linux.
const UNIX_ENV: &[&str] = &[
    "HOME",
    "USER",
    "PATH",
    "PWD",
    "SHELL",
    "TMPDIR",
    "LANG",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XDG_SESSION_TYPE",
    "XDG_CONFIG_HOME",
    "XDG_RUNTIME_DIR",
    "HOSTNAME",
    "RANDOM",
    "?",
];
/// A name that goes after `$` on Windows.
const WINDOWS_ENV: &[&str] = &[
    "env:USERPROFILE",
    "env:USERNAME",
    "env:TEMP",
    "env:APPDATA",
    "env:LOCALAPPDATA",
    "env:COMPUTERNAME",
    "env:PATH",
];

fn is_known_command(word: &str) -> bool {
    UNIX_COMMANDS.iter().any(|(n, _)| *n == word)
        || WINDOWS_COMMANDS
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case(word))
}

fn push(out: &mut Vec<Span>, start: usize, end: usize, tok: Tok) {
    if end > start {
        out.push(Span { start, end, tok });
    }
}

/// Where the `{{...}}` that starts at `i` ends (after the `}}`), or at the end of its line.
fn template_end(b: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    while j < b.len() {
        if b[j] == b'}' && b.get(j + 1) == Some(&b'}') {
            return j + 2;
        }
        if b[j] == b'\n' {
            return j;
        }
        j += 1;
    }
    b.len()
}

/// The pieces of the `{{...}}` that is `b[start..end]`: all of it is a template, except a filter
/// (`{{x|nope}}`) or a function (`{{= nope(1)}}`) that does not exist, which is [`Tok::Bad`].
fn template_spans(b: &[u8], start: usize, end: usize, out: &mut Vec<Span>) {
    let Ok(src) = std::str::from_utf8(&b[start..end]) else {
        push(out, start, end, Tok::Template);
        return;
    };
    let inner_end = if src.ends_with("}}") && src.len() >= 4 {
        src.len() - 2
    } else {
        src.len()
    };
    let inner = &src[2.min(inner_end)..inner_end];
    let mut bad: Vec<(usize, usize)> = Vec::new();
    let lead = inner.len() - inner.trim_start().len();
    if inner.trim_start().starts_with('=') {
        // a name followed by `(` is a call
        let bytes = inner.as_bytes();
        let mut i = lead + 1;
        while i < bytes.len() {
            if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                let from = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'(')
                    && !crate::flowexpr::FUNCTIONS
                        .iter()
                        .any(|f| f.0 == &inner[from..i])
                {
                    bad.push((from, i));
                }
            } else if bytes[i] == b'"' || bytes[i] == b'\'' {
                // a string: its words are not calls
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                i += 1;
            } else {
                i += 1;
            }
        }
    } else {
        let mut at = 0;
        for (n, seg) in inner.split('|').enumerate() {
            if n > 0 {
                let spec = seg.trim();
                if !spec.is_empty() && !crate::flowrun::is_known_filter(spec) {
                    let from = at + (seg.len() - seg.trim_start().len());
                    let name_len = seg.trim_start().find(':').unwrap_or(seg.trim_start().len());
                    let name = seg.trim_start()[..name_len].trim_end();
                    bad.push((from, from + name.len()));
                }
            }
            at += seg.len() + 1;
        }
    }
    let mut pos = start;
    for (from, to) in bad {
        let (f, t) = (start + 2 + from, start + 2 + to);
        if f > pos {
            push(out, pos, f, Tok::Template);
        }
        push(out, f, t, Tok::Bad);
        pos = t;
    }
    push(out, pos, end, Tok::Template);
}

/// Where the `$name`, `${...}`, `$(` or `$?` that starts at `i` ends.
fn var_end(b: &[u8], i: usize) -> usize {
    let j = i + 1;
    match b.get(j) {
        Some(b'{') => {
            let mut k = j + 1;
            while k < b.len() && b[k] != b'}' && b[k] != b'\n' {
                k += 1;
            }
            if k < b.len() && b[k] == b'}' {
                k + 1
            } else {
                k
            }
        }
        Some(b'(') => j + 1,
        Some(c) if c.is_ascii_alphabetic() || *c == b'_' => {
            let mut k = j;
            while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                k += 1;
            }
            k
        }
        Some(c) if c.is_ascii_digit() || b"?#@*!$-".contains(c) => j + 1,
        _ => j,
    }
}

/// A string that opens at `start` with `quote`: its pieces, with the `{{templates}}` (and, in
/// double quotes, the `$variables`) cut out of it. Returns where it ends.
fn quoted(b: &[u8], start: usize, quote: u8, shell: bool, out: &mut Vec<Span>) -> usize {
    let n = b.len();
    let mut seg = start;
    let mut i = start + 1;
    while i < n {
        let c = b[i];
        if c == b'\\' && quote == b'"' {
            i += 1;
            if i < n {
                i += 1;
                while i < n && (b[i] & 0xC0) == 0x80 {
                    i += 1;
                }
            }
            continue;
        }
        if c == quote {
            i += 1;
            push(out, seg, i, Tok::Str);
            return i;
        }
        if c == b'{' && b.get(i + 1) == Some(&b'{') {
            push(out, seg, i, Tok::Str);
            let end = template_end(b, i);
            template_spans(b, i, end, out);
            i = end;
            seg = end;
            continue;
        }
        if shell && quote == b'"' && c == b'$' {
            let end = var_end(b, i);
            if end > i + 1 {
                push(out, seg, i, Tok::Str);
                push(out, i, end, Tok::Var);
                i = end;
                seg = end;
                continue;
            }
        }
        i += 1;
    }
    push(out, seg, n, Tok::Str);
    n
}

fn shell_spans(src: &str, out: &mut Vec<Span>) {
    let b = src.as_bytes();
    let n = b.len();
    let mut i = 0;
    // whether the next word is the name of a command
    let mut cmd = true;
    while i < n {
        let c = b[i];
        match c {
            b'\n' => {
                cmd = true;
                i += 1;
            }
            b' ' | b'\t' | b'\r' => i += 1,
            b'{' if b.get(i + 1) == Some(&b'{') => {
                let end = template_end(b, i);
                template_spans(b, i, end, out);
                i = end;
                cmd = false;
            }
            b'#' if i == 0 || matches!(b[i - 1], b' ' | b'\t' | b'\n' | b';') => {
                let end = b[i..].iter().position(|&x| x == b'\n').map_or(n, |p| i + p);
                push(out, i, end, Tok::Comment);
                i = end;
            }
            b'\'' | b'"' => {
                i = quoted(b, i, c, true, out);
                cmd = false;
            }
            b'$' => {
                let end = var_end(b, i);
                push(out, i, end, Tok::Var);
                cmd = b.get(i + 1) == Some(&b'(');
                i = end;
            }
            b'|' | b'&' | b';' | b'(' | b')' | b'<' | b'>' | b'!' | b'{' | b'}' => {
                let double = matches!(c, b'|' | b'&' | b'<' | b'>') && b.get(i + 1) == Some(&c);
                let end = i + if double { 2 } else { 1 };
                push(out, i, end, Tok::Op);
                cmd = matches!(c, b'|' | b'&' | b';' | b'(' | b'{' | b'!');
                i = end;
            }
            _ => {
                let mut end = i;
                while end < n
                    && !matches!(
                        b[end],
                        b' ' | b'\t'
                            | b'\r'
                            | b'\n'
                            | b'|'
                            | b'&'
                            | b';'
                            | b'('
                            | b')'
                            | b'<'
                            | b'>'
                            | b'"'
                            | b'\''
                            | b'$'
                    )
                    && !(b[end] == b'{' && b.get(end + 1) == Some(&b'{'))
                {
                    end += 1;
                }
                let w = &src[i..end];
                let tok = if KEYWORDS.contains(&w) && (cmd || MID_KEYWORDS.contains(&w)) {
                    cmd = OPENS_COMMAND.contains(&w);
                    Tok::Keyword
                } else if cmd && is_assignment(w) {
                    Tok::Plain
                } else if cmd {
                    cmd = false;
                    if is_known_command(w) {
                        Tok::Command
                    } else {
                        Tok::Plain
                    }
                } else if w.len() > 1 && w.starts_with('-') {
                    Tok::Flag
                } else if w.bytes().all(|x| x.is_ascii_digit() || x == b'.') {
                    Tok::Number
                } else {
                    Tok::Plain
                };
                push(out, i, end, tok);
                i = end;
            }
        }
    }
}

/// `NAME=value`: a word that sets a variable and leaves the command to come.
fn is_assignment(w: &str) -> bool {
    w.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty()
            && !name.starts_with(|c: char| c.is_ascii_digit())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn json_spans(src: &str, out: &mut Vec<Span>) {
    let b = src.as_bytes();
    let n = b.len();
    let mut i = 0;
    while i < n {
        let c = b[i];
        match c {
            b'"' => {
                let before = out.len();
                let end = quoted(b, i, b'"', false, out);
                let mut k = end;
                while k < n && matches!(b[k], b' ' | b'\t' | b'\r' | b'\n') {
                    k += 1;
                }
                if b.get(k) == Some(&b':') {
                    for s in &mut out[before..] {
                        if s.tok == Tok::Str {
                            s.tok = Tok::Key;
                        }
                    }
                }
                i = end;
            }
            b'{' if b.get(i + 1) == Some(&b'{') => {
                let end = template_end(b, i);
                template_spans(b, i, end, out);
                i = end;
            }
            b'{' | b'}' | b'[' | b']' | b',' | b':' => {
                push(out, i, i + 1, Tok::Op);
                i += 1;
            }
            b'-' | b'0'..=b'9' => {
                let mut end = i + 1;
                while end < n && matches!(b[end], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
                    end += 1;
                }
                push(out, i, end, Tok::Number);
                i = end;
            }
            c if c.is_ascii_alphabetic() => {
                let mut end = i;
                while end < n && b[end].is_ascii_alphanumeric() {
                    end += 1;
                }
                if matches!(&src[i..end], "true" | "false" | "null") {
                    push(out, i, end, Tok::Keyword);
                }
                i = end;
            }
            _ => i += 1,
        }
    }
}

/// A text that is plain except for its `{{templates}}`.
fn value_spans(src: &str, out: &mut Vec<Span>) {
    let b = src.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'{' && b[i + 1] == b'{' {
            let end = template_end(b, i);
            template_spans(b, i, end, out);
            i = end;
        } else {
            i += 1;
        }
    }
}

/// The colored pieces of `src`.
pub fn tokenize(src: &str, lang: Lang) -> Vec<Span> {
    let mut out = Vec::new();
    match lang {
        Lang::Shell => shell_spans(src, &mut out),
        Lang::Json => json_spans(src, &mut out),
        Lang::Value => value_spans(src, &mut out),
    }
    out
}

// -------------------------------------------------------------------------------- text helpers

fn byte_at(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map_or(s.len(), |(b, _)| b)
}

fn replace_chars(s: &mut String, from: usize, to: usize, with: &str) {
    let (a, b) = (byte_at(s, from), byte_at(s, to));
    s.replace_range(a..b, with);
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn line_start(chars: &[char], ci: usize) -> usize {
    chars[..ci.min(chars.len())]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |p| p + 1)
}

fn line_end(chars: &[char], ci: usize) -> usize {
    let ci = ci.min(chars.len());
    chars[ci..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |p| ci + p)
}

fn indent_of(chars: &[char], from: usize, to: usize) -> String {
    chars[from..to.max(from)]
        .iter()
        .take_while(|c| **c == ' ' || **c == '\t')
        .collect()
}

/// The first and the last character of the lines a selection touches (the last line is left out
/// when the selection ends at its very start).
fn line_block(chars: &[char], lo: usize, hi: usize) -> (usize, usize) {
    let first = line_start(chars, lo);
    let hi_eff = if hi > lo && chars.get(hi - 1) == Some(&'\n') {
        hi - 1
    } else {
        hi
    };
    (first, line_end(chars, hi_eff))
}

// ------------------------------------------------------------------------------- suggestions

/// What a suggestion is, for its color and for how it is put in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Command,
    Keyword,
    Snippet,
    Variable,
    Filter,
    Env,
    /// A function of `{{= }}`; it goes in with its `(`.
    Function,
    /// A flag of a command (`-i`).
    Flag,
    /// A variable as an argument, already wrapped: `{{name|sh}}`.
    Arg,
}

impl Kind {
    /// Whether it goes inside a `{{ }}` (which then gets its closing braces).
    fn in_template(self) -> bool {
        matches!(self, Kind::Variable | Kind::Filter)
    }
}

/// A line of the list of suggestions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    /// What replaces the word; [`CARET`] marks where the cursor goes.
    pub insert: String,
    pub detail: String,
    pub kind: Kind,
}

/// A suggestion list for the word that ends at the cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggest {
    /// The character index the word starts at.
    pub start: usize,
    pub items: Vec<Item>,
}

/// Marks the cursor in the text of a snippet.
pub const CARET: char = '\u{1}';
const MAX_ITEMS: usize = 8;

fn item(label: &str, insert: &str, detail: &str, kind: Kind) -> Item {
    Item {
        label: label.to_string(),
        insert: insert.to_string(),
        detail: detail.to_string(),
        kind,
    }
}

fn starts(name: &str, prefix: &str) -> bool {
    name.to_lowercase().starts_with(&prefix.to_lowercase())
}

/// How well `label` fits what was typed: 0 it starts with it, 1 a word in it does, 2 its letters
/// come in that order (only from two letters on); `None` it does not fit.
fn fit(label: &str, prefix: &str) -> Option<u8> {
    if prefix.is_empty() || starts(label, prefix) {
        return Some(0);
    }
    let (l, p) = (label.to_lowercase(), prefix.to_lowercase());
    if l.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&p))
    {
        return Some(1);
    }
    if p.chars().count() < 2 {
        return None;
    }
    let mut it = l.chars();
    p.chars().all(|pc| it.any(|lc| lc == pc)).then_some(2)
}

/// The built-in variables of a run, which are not set by any block.
const RUN_VARS: &[(&str, &str)] = &[
    ("ok", "1 if the last block worked"),
    ("code", "the exit code of the last command"),
    ("index", "the round of a loop, from 1"),
];

/// The variables of the flow and the built-in ones, with what they are.
fn all_vars(vars: &[String]) -> Vec<(String, String)> {
    let mut names: Vec<(String, String)> = vars
        .iter()
        .map(|v| (v.clone(), "variable of this flow".to_string()))
        .collect();
    for (n, d) in RUN_VARS {
        if !names.iter().any(|(x, _)| x == n) {
            names.push((n.to_string(), d.to_string()));
        }
    }
    names
}

/// What a filter of `{{name|filter}}` does, in a few words.
fn filter_detail(name: &str) -> &'static str {
    match name {
        "sh" => "quote for the shell",
        "trim" => "cut the spaces around",
        "upper" => "CAPITALS",
        "lower" => "small letters",
        "title" => "Each Word Capitalized",
        "len" => "number of characters",
        "reverse" => "backwards",
        "int" => "cut the decimals",
        "round" => "round to a whole number",
        "abs" => "without the sign",
        "num" => "write a plain number",
        "bool" => "1 or 0",
        "json" => "as a JSON string",
        "url" => "percent-encode",
        "basename" => "file name of a path",
        "dirname" => "folder of a path",
        "ext" => "extension of a path",
        "count" => "items of a list",
        "first" => "first item",
        "last" => "last item",
        "item" => "item at a position, e.g. item:2",
        "default" => "text when empty, e.g. default:none",
        "replace" => "replace a by b, e.g. replace:a:b",
        "join" => "join a list, e.g. join:, ",
        _ => "",
    }
}

fn snippets(os: Os) -> Vec<Item> {
    let c = CARET;
    if os == Os::Windows {
        vec![
            item(
                "if",
                &format!("if ({c}) {{\n  \n}}"),
                "if block",
                Kind::Snippet,
            ),
            item(
                "foreach",
                &format!("foreach ($x in {c}) {{\n  \n}}"),
                "loop over a list",
                Kind::Snippet,
            ),
        ]
    } else {
        vec![
            item(
                "if",
                &format!("if [ {c} ]; then\n  \nfi"),
                "if ... then ... fi",
                Kind::Snippet,
            ),
            item(
                "for",
                &format!("for f in {c}; do\n  \ndone"),
                "loop over a list",
                Kind::Snippet,
            ),
            item(
                "while",
                &format!("while {c}; do\n  \ndone"),
                "loop while true",
                Kind::Snippet,
            ),
            item(
                "case",
                &format!("case \"{c}\" in\n  *) ;;\nesac"),
                "pick by pattern",
                Kind::Snippet,
            ),
        ]
    }
}

/// The `{{` the cursor is inside of, if any: the character index of its first brace.
fn open_template(chars: &[char], cursor: usize) -> Option<usize> {
    let ls = line_start(chars, cursor);
    let mut i = cursor.checked_sub(2)?;
    loop {
        if i < ls {
            return None;
        }
        // a `}}` between the braces and the cursor: that template is closed
        if chars[i] == '}' && chars.get(i + 1) == Some(&'}') && i + 2 <= cursor {
            return None;
        }
        if chars[i] == '{' && chars.get(i + 1) == Some(&'{') {
            return Some(i);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

/// The suggestions for the word that ends at `cursor` (a character index), or `None`. `force`
/// (Ctrl/Cmd + Space) allows a word with nothing typed yet.
pub fn suggest(
    text: &str,
    cursor: usize,
    lang: Lang,
    vars: &[String],
    os: Os,
    force: bool,
) -> Option<Suggest> {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let found = match open_template(&chars, cursor) {
        Some(open) => suggest_in_template(&chars, open, cursor, vars),
        None if lang == Lang::Shell => suggest_shell(&chars, cursor, vars, os, force),
        None => None,
    }?;
    let prefix: String = chars[found.start..cursor].iter().collect();
    if found.items.is_empty() || (found.items.len() == 1 && found.items[0].insert == prefix) {
        return None;
    }
    Some(found)
}

fn suggest_in_template(
    chars: &[char],
    open: usize,
    cursor: usize,
    vars: &[String],
) -> Option<Suggest> {
    let inner: String = chars[open + 2..cursor].iter().collect();
    let base = open + 2;
    if let Some(p) = inner.rfind('|') {
        let seg = &inner[p + 1..];
        // an argument is being typed (`default:..`): nothing to suggest
        if seg.contains(':') {
            return None;
        }
        let lead = seg.len() - seg.trim_start().len();
        let prefix = seg.trim_start();
        let start = base + inner[..p].chars().count() + 1 + lead;
        let mut items = Vec::new();
        for name in crate::flowrun::PLAIN_FILTERS {
            if name != "join" && starts(name, prefix) {
                items.push(item(name, name, filter_detail(name), Kind::Filter));
            }
        }
        for name in crate::flowrun::ARG_FILTERS {
            if starts(name, prefix) {
                let with_colon = format!("{name}:");
                items.push(item(name, &with_colon, filter_detail(name), Kind::Filter));
            }
        }
        return Some(Suggest { start, items });
    }
    let trimmed = inner.trim_start();
    let lead = inner.chars().count() - trimmed.chars().count();
    let is_expr = trimmed.starts_with('=');
    let (prefix, start) = if let Some(expr) = trimmed.strip_prefix('=') {
        // `{{= a + 1}}`: the name that is being typed at the end
        let word: String = expr
            .chars()
            .rev()
            .take_while(|c| is_word(*c))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let n = word.chars().count();
        (word, cursor - n)
    } else {
        if trimmed.chars().any(char::is_whitespace) {
            return None;
        }
        (trimmed.to_string(), base + lead)
    };
    let mut ranked: Vec<(u8, Item)> = Vec::new();
    for (n, d) in all_vars(vars) {
        if let Some(r) = fit(&n, &prefix) {
            ranked.push((r, item(&n, &n, &d, Kind::Variable)));
        }
    }
    if is_expr {
        for (name, min, max, what) in crate::flowexpr::FUNCTIONS {
            // a name alone is a variable; a function needs a letter typed
            if let (false, Some(r)) = (prefix.is_empty(), fit(name, &prefix)) {
                let args = if min == max {
                    format!("{min}")
                } else {
                    format!("{min}-{max}")
                };
                let ins = format!("{name}({CARET})");
                ranked.push((
                    r,
                    item(
                        name,
                        &ins,
                        &format!("{what}  ({args} args)"),
                        Kind::Function,
                    ),
                ));
            }
        }
    }
    ranked.sort_by_key(|(r, i)| {
        (
            *r,
            i.label.to_lowercase() != prefix.to_lowercase(),
            i.label.to_lowercase(),
        )
    });
    let items: Vec<Item> = ranked.into_iter().map(|(_, i)| i).take(MAX_ITEMS).collect();
    Some(Suggest { start, items })
}

fn suggest_shell(
    chars: &[char],
    cursor: usize,
    vars: &[String],
    os: Os,
    force: bool,
) -> Option<Suggest> {
    // the cursor must be at the end of a word, not in the middle of one
    if chars.get(cursor).is_some_and(|c| is_word(*c)) {
        return None;
    }
    let ls = line_start(chars, cursor);
    let mut j = cursor;
    while j > ls && (is_word(chars[j - 1]) || matches!(chars[j - 1], '-' | '.' | ':' | '/' | '~')) {
        j -= 1;
    }
    let prefix: String = chars[j..cursor].iter().collect();
    // `$HOME`
    if j > ls && chars[j - 1] == '$' {
        let names: &[&str] = if os == Os::Windows {
            WINDOWS_ENV
        } else {
            UNIX_ENV
        };
        let items: Vec<Item> = names
            .iter()
            .filter(|n| starts(n, &prefix))
            .map(|n| item(&format!("${n}"), &format!("${n}"), "environment", Kind::Env))
            .collect();
        return Some(Suggest {
            start: j - 1,
            items,
        });
    }
    let before: String = chars[ls..j].iter().collect();
    let t = before.trim_end();
    let at_command = t.is_empty()
        || t.ends_with(['|', '&', ';', '(', '{', '!'])
        || OPENS_COMMAND
            .iter()
            .any(|k| t == *k || t.ends_with(&format!(" {k}")));
    if !at_command {
        return suggest_argument(chars, ls, j, &prefix, vars, os, force);
    }
    // one letter is not worth a list that pops up under the person's hands
    let too_short = prefix.chars().count() < 2 && !force;
    if too_short || prefix.starts_with(['-', '.', '/', '~']) {
        return None;
    }
    let mut items: Vec<Item> = snippets(os)
        .into_iter()
        .filter(|s| starts(&s.label, &prefix))
        .collect();
    let mut plain: Vec<(u8, Item)> = Vec::new();
    for k in KEYWORDS {
        if let Some(r) = fit(k, &prefix) {
            plain.push((r, item(k, k, "shell keyword", Kind::Keyword)));
        }
    }
    let table: &[(&str, &str)] = if os == Os::Windows {
        WINDOWS_COMMANDS
    } else {
        UNIX_COMMANDS
    };
    for (name, what) in table {
        if let Some(r) = fit(name, &prefix) {
            plain.push((r, item(name, name, what, Kind::Command)));
        }
    }
    let low = prefix.to_lowercase();
    plain.sort_by_key(|(r, i)| (*r, i.label.to_lowercase() != low, i.label.to_lowercase()));
    items.extend(plain.into_iter().map(|(_, i)| i));
    items.truncate(if force { MAX_ITEMS + 4 } else { MAX_ITEMS });
    Some(Suggest { start: j, items })
}

/// A word after the command: the flags of the command when it starts with `-`, and (with
/// Ctrl/Cmd + Space) the variables of the flow, put in quoted for the shell (`{{name|sh}}`).
#[allow(clippy::too_many_arguments)]
fn suggest_argument(
    chars: &[char],
    ls: usize,
    j: usize,
    prefix: &str,
    vars: &[String],
    os: Os,
    force: bool,
) -> Option<Suggest> {
    if prefix.starts_with('-') {
        if os == Os::Windows {
            return None;
        }
        // the command of this part of the line: the first word after the last `| & ;`
        let line: String = chars[ls..j].iter().collect();
        let part = line.rsplit(['|', '&', ';', '(']).next().unwrap_or("");
        let cmd = part.split_whitespace().next().unwrap_or("");
        let cmd = cmd.rsplit('/').next().unwrap_or(cmd);
        let (_, flags) = FLAGS.iter().find(|(c, _)| *c == cmd)?;
        let items = flags
            .iter()
            .filter(|(f, _)| starts(f, prefix))
            .map(|(f, what)| item(f, f, what, Kind::Flag))
            .collect();
        return Some(Suggest { start: j, items });
    }
    if !force || prefix.starts_with(['.', '/', '~']) {
        return None;
    }
    let mut ranked: Vec<(u8, Item)> = Vec::new();
    for (n, d) in all_vars(vars) {
        if let Some(r) = fit(&n, prefix) {
            let ins = format!("{{{{{n}|sh}}}}");
            ranked.push((
                r,
                item(&n, &ins, &format!("{d}  \u{2192}  {ins}"), Kind::Arg),
            ));
        }
    }
    ranked.sort_by_key(|(r, i)| (*r, i.label.to_lowercase()));
    let items = ranked
        .into_iter()
        .map(|(_, i)| i)
        .take(MAX_ITEMS + 4)
        .collect();
    Some(Suggest { start: j, items })
}

/// Puts `item` in place of the word `start..cursor`. Returns where the cursor goes.
pub fn accept(text: &mut String, start: usize, cursor: usize, item: &Item) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let indent = indent_of(&chars, line_start(&chars, start), start);
    let mut ins = item.insert.replace('\n', &format!("\n{indent}"));
    let caret = ins.find(CARET).map(|p| ins[..p].chars().count());
    ins = ins.replace(CARET, "");
    let at = start + caret.unwrap_or_else(|| ins.chars().count());
    if item.kind.in_template() {
        let rest: String = chars[cursor.min(chars.len())..]
            .iter()
            .take_while(|c| **c != '\n')
            .collect();
        if !rest.trim_start().starts_with("}}") {
            ins.push_str("}}");
        }
    }
    replace_chars(text, start, cursor, &ins);
    at
}

// --------------------------------------------------------------------------------------- edits

/// Enter: a new line with the indent of this one, one level deeper after `then`, `do`, `else`,
/// `{`, `(` or `[` (and the closing bracket on a line of its own when it is right there).
/// Returns where the cursor goes.
pub fn enter(text: &mut String, cursor: usize) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let ls = line_start(&chars, cursor);
    let indent = indent_of(&chars, ls, cursor);
    let before: String = chars[ls..cursor].iter().collect();
    let t = before.trim_end();
    let word_open = ["then", "do", "else"].iter().any(|k| {
        t.strip_suffix(*k)
            .is_some_and(|r| r.is_empty() || r.ends_with([' ', '\t', ';']))
    });
    let sym = t.chars().last().filter(|c| matches!(c, '{' | '(' | '['));
    let closer = sym.map(|c| match c {
        '{' => '}',
        '(' => ')',
        _ => ']',
    });
    let (ins, back) = if closer.is_some() && chars.get(cursor) == closer.as_ref() {
        (
            format!("\n{indent}  \n{indent}"),
            1 + indent.chars().count(),
        )
    } else if sym.is_some() || word_open {
        (format!("\n{indent}  "), 0)
    } else {
        (format!("\n{indent}"), 0)
    };
    let new_cursor = cursor + ins.chars().count() - back;
    replace_chars(text, cursor, cursor, &ins);
    new_cursor
}

/// Rewrites the lines a selection touches. `f` gets a line and says what it becomes, the column
/// of the change and how many characters it adds (or removes, when negative). Returns the
/// selection, moved along.
fn transform_lines(
    text: &mut String,
    lo: usize,
    hi: usize,
    f: &dyn Fn(&str) -> (String, usize, isize),
) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (first, last_end) = line_block(&chars, lo, hi);
    let block: String = chars[first..last_end].iter().collect();
    let mut out = Vec::new();
    let (mut nlo, mut nhi) = (lo as isize, hi as isize);
    let mut pos = first;
    for line in block.split('\n') {
        let (new, col, delta) = f(line);
        let at = pos + col;
        for (p, np) in [(lo, &mut nlo), (hi, &mut nhi)] {
            if delta >= 0 {
                if p >= at {
                    *np += delta;
                }
            } else if p > at {
                *np += delta.max(-((p - at) as isize));
            }
        }
        pos += line.chars().count() + 1;
        out.push(new);
    }
    replace_chars(text, first, last_end, &out.join("\n"));
    let len = text.chars().count() as isize;
    (nlo.clamp(0, len) as usize, nhi.clamp(0, len) as usize)
}

/// Tab (`outdent` false) and Shift + Tab. With nothing selected, Tab adds spaces up to the next
/// stop of two. Returns the selection.
pub fn indent(text: &mut String, lo: usize, hi: usize, outdent: bool) -> (usize, usize) {
    if lo == hi && !outdent {
        let chars: Vec<char> = text.chars().collect();
        let col = lo - line_start(&chars, lo);
        let n = 2 - col % 2;
        replace_chars(text, lo, lo, &" ".repeat(n));
        return (lo + n, lo + n);
    }
    if outdent {
        transform_lines(text, lo, hi, &|line| {
            let r = if line.starts_with('\t') {
                1
            } else {
                line.chars().take(2).take_while(|c| *c == ' ').count()
            };
            (line.chars().skip(r).collect(), 0, -(r as isize))
        })
    } else {
        transform_lines(text, lo, hi, &|line| {
            if line.is_empty() {
                (String::new(), 0, 0)
            } else {
                (format!("  {line}"), 0, 2)
            }
        })
    }
}

/// Ctrl/Cmd + /: puts `# ` in front of the lines, or takes it away when they all have it.
pub fn toggle_comment(text: &mut String, lo: usize, hi: usize) -> (usize, usize) {
    let chars: Vec<char> = text.chars().collect();
    let (first, last_end) = line_block(&chars, lo, hi);
    let block: String = chars[first..last_end].iter().collect();
    let all = block
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .all(|l| l.trim_start().starts_with('#'));
    let has_text = block.split('\n').any(|l| !l.trim().is_empty());
    if all && has_text {
        transform_lines(text, lo, hi, &|line| {
            let col = line.chars().take_while(|c| c.is_whitespace()).count();
            let rest: String = line.chars().skip(col).collect();
            if let Some(r) = rest.strip_prefix("# ") {
                (format!("{}{r}", " ".repeat(col)), col, -2)
            } else if let Some(r) = rest.strip_prefix('#') {
                (format!("{}{r}", " ".repeat(col)), col, -1)
            } else {
                (line.to_string(), col, 0)
            }
        })
    } else {
        transform_lines(text, lo, hi, &|line| {
            if line.trim().is_empty() {
                return (line.to_string(), 0, 0);
            }
            let col = line.chars().take_while(|c| c.is_whitespace()).count();
            let (a, b): (String, String) = (
                line.chars().take(col).collect(),
                line.chars().skip(col).collect(),
            );
            (format!("{a}# {b}"), col, 2)
        })
    }
}

/// The names of the variables a flow uses, for the suggestions inside `{{ }}`.
pub fn variable_names(flow: &Flow) -> Vec<String> {
    let mut set = BTreeSet::new();
    for n in &flow.nodes {
        set.extend(block_writes(&n.kind));
        set.extend(block_reads(&n.kind));
    }
    set.into_iter().filter(|n| n != "*").collect()
}

// ------------------------------------------------------------------------------------ the widget

const VARS_KEY: &str = "code_editor_vars";
const FONT: f32 = 13.0;
const ROW_H: f32 = 24.0;

/// Tells the editors the variables of the flow on show (once per frame, by the Flow tab).
pub(in crate::ui::settings) fn set_variables(ctx: &egui::Context, names: Vec<String>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(VARS_KEY), names));
}

fn color_of(tok: Tok, dark: bool) -> Color32 {
    let (d, l) = match tok {
        Tok::Plain => return pal().text,
        Tok::Comment => return pal().muted,
        Tok::Keyword => ((199, 146, 234), (130, 80, 223)),
        Tok::Command => ((130, 170, 255), (9, 105, 218)),
        Tok::Str => ((195, 232, 141), (26, 127, 55)),
        Tok::Var => ((247, 140, 108), (188, 76, 0)),
        Tok::Template => ((255, 203, 107), (154, 103, 0)),
        Tok::Op => ((137, 221, 255), (10, 126, 164)),
        Tok::Number => ((242, 163, 101), (149, 56, 0)),
        Tok::Flag => ((156, 220, 254), (5, 80, 174)),
        Tok::Key => ((130, 170, 255), (5, 80, 174)),
        Tok::Bad => ((255, 121, 121), (207, 34, 46)),
    };
    let (r, g, b) = if dark { d } else { l };
    Color32::from_rgb(r, g, b)
}

fn kind_tok(kind: Kind) -> Tok {
    match kind {
        Kind::Command => Tok::Command,
        Kind::Keyword | Kind::Snippet => Tok::Keyword,
        Kind::Variable | Kind::Arg => Tok::Template,
        Kind::Filter | Kind::Flag | Kind::Function => Tok::Flag,
        Kind::Env => Tok::Var,
    }
}

/// The text as a laid-out job, colored by [`tokenize`].
fn highlight(text: &str, lang: Lang, wrap: f32) -> egui::text::LayoutJob {
    let dark = pal().dark;
    let font = FontId::monospace(FONT);
    let mut job = egui::text::LayoutJob::default();
    let mut add = |s: &str, tok: Tok| {
        let color = color_of(tok, dark);
        let mut f = egui::TextFormat::simple(font.clone(), color);
        if tok == Tok::Template {
            f.background = color.gamma_multiply(0.16);
        }
        if tok == Tok::Bad {
            f.underline = egui::Stroke::new(1.5, color);
        }
        job.append(s, 0.0, f);
    };
    let mut pos = 0;
    for s in tokenize(text, lang) {
        if s.start > pos {
            add(&text[pos..s.start], Tok::Plain);
        }
        add(&text[s.start..s.end], s.tok);
        pos = s.end;
    }
    if pos < text.len() || text.is_empty() {
        add(&text[pos..], Tok::Plain);
    }
    job.wrap.max_width = wrap;
    job
}

/// What the editor keeps between two frames.
#[derive(Clone, Default)]
struct Local {
    sel: usize,
    snap: Option<Snap>,
    /// `(cursor, length)` of the text when Esc closed the list: it stays closed until either changes.
    dismissed: Option<(usize, usize)>,
    /// The word a list asked for with Ctrl/Cmd + Space is for.
    forced: Option<usize>,
    force_now: bool,
    /// Up / Down moved the highlight: only then does Enter take the entry (it is a new line
    /// otherwise, so typing and pressing Enter never swaps a word for another).
    chosen: bool,
}

/// The list as it was drawn last frame, which the keys of this frame act on.
#[derive(Clone)]
struct Snap {
    start: usize,
    cursor: usize,
    items: Vec<Item>,
}

fn put_cursor(
    state: &mut egui::text_edit::TextEditState,
    ctx: &egui::Context,
    id: egui::Id,
    lo: usize,
    hi: usize,
) {
    state
        .cursor
        .set_char_range(Some(CCursorRange::two(CCursor::new(lo), CCursor::new(hi))));
    state.clone().store(ctx, id);
}

/// Everything the editor does to the text before the box sees the keys of this frame.
fn before_show(
    ctx: &egui::Context,
    id: egui::Id,
    text: &mut String,
    lang: Lang,
    local: &mut Local,
) {
    let Some(mut state) = egui::TextEdit::load_state(ctx, id) else {
        return;
    };
    let Some(range) = state.cursor.char_range() else {
        return;
    };
    let (prim, sec): (usize, usize) = (range.primary.index.into(), range.secondary.index.into());
    let (lo, hi) = (prim.min(sec), prim.max(sec));
    let len = text.chars().count();
    let consume = |m: Modifiers, k: egui::Key| ctx.input_mut(|i| i.consume_key(m, k));

    // the list is open: its keys come first
    let open = lo == hi
        && local.dismissed != Some((lo, len))
        && local
            .snap
            .as_ref()
            .is_some_and(|s| s.cursor == lo && !s.items.is_empty());
    if open {
        let n = local.snap.as_ref().map_or(1, |s| s.items.len());
        if consume(Modifiers::NONE, egui::Key::ArrowDown) {
            local.sel = (local.sel + 1) % n;
            local.chosen = true;
            return;
        }
        if consume(Modifiers::NONE, egui::Key::ArrowUp) {
            local.sel = (local.sel + n - 1) % n;
            local.chosen = true;
            return;
        }
        if consume(Modifiers::NONE, egui::Key::Escape) {
            local.dismissed = Some((lo, len));
            return;
        }
        // Tab always takes the highlighted entry; Enter only after the arrows chose one
        if consume(Modifiers::NONE, egui::Key::Tab)
            || (local.chosen && consume(Modifiers::NONE, egui::Key::Enter))
        {
            if let Some(s) = local.snap.take() {
                let it = s.items[local.sel.min(s.items.len() - 1)].clone();
                let c = accept(text, s.start, lo, &it);
                put_cursor(&mut state, ctx, id, c, c);
            }
            return;
        }
    }

    if consume(Modifiers::SHIFT, egui::Key::Tab) {
        let (a, b) = indent(text, lo, hi, true);
        put_cursor(&mut state, ctx, id, a, b);
    } else if consume(Modifiers::NONE, egui::Key::Tab) {
        let (a, b) = indent(text, lo, hi, false);
        put_cursor(&mut state, ctx, id, a, b);
    } else if consume(Modifiers::NONE, egui::Key::Enter) {
        replace_chars(text, lo, hi, "");
        let c = enter(text, lo);
        put_cursor(&mut state, ctx, id, c, c);
    } else if lang == Lang::Shell && consume(Modifiers::COMMAND, egui::Key::Slash) {
        let (a, b) = toggle_comment(text, lo, hi);
        put_cursor(&mut state, ctx, id, a, b);
    } else if consume(Modifiers::COMMAND, egui::Key::Space)
        || consume(Modifiers::CTRL, egui::Key::Space)
    {
        local.force_now = true;
    }
}

/// Where the Settings window (the card) keeps the rectangle it is drawn in, for the windows that
/// open over it.
pub(in crate::ui::settings) const CARD_KEY: &str = "settings_card_rect";

/// Set while the list of suggestions is on screen: the pass it was drawn in.
const OPEN_KEY: &str = "code_editor_list_open";

/// Called every frame a code window is on screen: Esc is then for it (it closes it), not for the
/// Settings window.
pub(in crate::ui::settings) fn hold_escape(ctx: &egui::Context) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN_KEY), pass));
}

/// Whether the list of suggestions is open, so that Esc is for it: the window must leave Esc alone.
pub(in crate::ui::settings) fn escape_is_taken(ctx: &egui::Context) -> bool {
    let at: Option<u64> = ctx.data(|d| d.get_temp(egui::Id::new(OPEN_KEY)));
    at.is_some_and(|n| n + 1 >= ctx.cumulative_pass_nr())
}

/// The line numbers in the left margin of `out`, one per line of the text (not per wrapped row).
fn paint_numbers(ui: &egui::Ui, out: &egui::text_edit::TextEditOutput) {
    let p = pal();
    let font = FontId::monospace(FONT - 1.5);
    let x = out.response.rect.left() + 26.0;
    let mut n = 1;
    let mut start = true;
    for row in &out.galley.rows {
        if start {
            ui.painter().text(
                egui::pos2(x, out.galley_pos.y + row.pos.y + row.height() / 2.0),
                Align2::RIGHT_CENTER,
                n.to_string(),
                font.clone(),
                p.muted,
            );
            n += 1;
        }
        start = row.ends_with_newline;
    }
}

/// A code editor in place of a text box: see the module's documentation. `rows` is how many lines
/// it shows before it scrolls (up to a few more as the text grows).
pub(in crate::ui::settings) fn code_edit(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    lang: Lang,
    hint: &str,
    rows: usize,
) -> egui::Response {
    ui.vertical(|ui| code_edit_inner(ui, salt, text, lang, hint, rows, false))
        .inner
}

/// The same editor, but filling all the room the parent gives it (a window the person can resize):
/// the box takes the height there is, and the list of suggestions floats under the cursor in place
/// of pushing the rest down.
pub(in crate::ui::settings) fn code_edit_fill(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    lang: Lang,
    hint: &str,
) -> egui::Response {
    ui.vertical(|ui| code_edit_inner(ui, salt, text, lang, hint, 3, true))
        .inner
}

fn code_edit_inner(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    text: &mut String,
    lang: Lang,
    hint: &str,
    rows: usize,
    fill: bool,
) -> egui::Response {
    let ctx = ui.ctx().clone();
    let id = ui.make_persistent_id(salt);
    let key = id.with("code_local");
    let mut local: Local = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    let vars: Vec<String> = ctx
        .data(|d| d.get_temp(egui::Id::new(VARS_KEY)))
        .unwrap_or_default();

    if ctx.memory(|m| m.has_focus(id)) {
        before_show(&ctx, id, text, lang, &mut local);
    }

    let mut layouter =
        |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| -> Arc<egui::Galley> {
            ui.painter().layout_job(highlight(buf.as_str(), lang, wrap))
        };
    let line_h = FONT * 1.4;
    let numbered = fill || text.contains('\n');
    let (max_h, rows) = if fill {
        // what is left, less the status line under the box and the room between the two (with
        // some to spare: a box a little too tall makes a window that grows by itself)
        let h = (ui.available_height() - ui.spacing().item_spacing.y - 24.0).max(60.0);
        (h, 1)
    } else {
        ((rows.max(3) + 9) as f32 * line_h + 16.0, rows)
    };
    let out = egui::ScrollArea::vertical()
        .id_salt(id.with("scroll"))
        .max_height(max_h)
        .auto_shrink([false, !fill])
        .show(ui, |ui| {
            let o = egui::TextEdit::multiline(text)
                .id(id)
                .font(FontId::monospace(FONT))
                .desired_rows(rows)
                .min_size(vec2(0.0, if fill { max_h } else { 0.0 }))
                .desired_width(f32::INFINITY)
                .lock_focus(true)
                .margin(egui::Margin {
                    left: if numbered { 36 } else { 10 },
                    right: 10,
                    top: 8,
                    bottom: 8,
                })
                .hint_text(hint)
                .layouter(&mut layouter)
                .show(ui);
            if numbered {
                paint_numbers(ui, &o);
            }
            o
        })
        .inner;
    let response: egui::Response = egui::Response::clone(&out.response);
    let has_focus = response.has_focus();
    let cursor: Option<(usize, usize)> = out
        .cursor_range
        .map(|r| (r.primary.index.into(), r.secondary.index.into()));

    // the list for what is under the cursor now
    let mut found = None;
    if has_focus
        && let Some((p, s)) = cursor
        && p == s
    {
        let len = text.chars().count();
        if local.dismissed != Some((p, len)) {
            found = suggest(text, p, lang, &vars, crate::platform::OS, false);
            if found.is_none() && (local.force_now || local.forced.is_some()) {
                found = suggest(text, p, lang, &vars, crate::platform::OS, true);
                local.forced = match &found {
                    Some(f) if local.force_now || local.forced == Some(f.start) => Some(f.start),
                    _ => None,
                };
            }
        }
    }
    local.force_now = false;
    let same = match (&local.snap, &found) {
        (Some(a), Some(b)) => a.start == b.start && a.items == b.items,
        _ => false,
    };
    if !same {
        local.sel = 0;
        local.chosen = false;
    }
    local.snap = found.zip(cursor).map(|(f, (p, _))| Snap {
        start: f.start,
        cursor: p,
        items: f.items,
    });

    let mut picked = None;
    if let Some(snap) = &local.snap {
        picked = if fill {
            // under the line of the cursor, over whatever is there
            let at = out
                .galley
                .pos_from_cursor(egui::text::CCursor::new(snap.cursor));
            let pos = out.galley_pos + at.left_bottom().to_vec2() + vec2(0.0, 4.0);
            // the editor can be in a zoomed layer (the canvas of a flow); the list is in the screen's
            let pos = ctx
                .layer_transform_to_global(ui.layer_id())
                .map_or(pos, |t| t * pos);
            let w = ui.available_width().clamp(220.0, 360.0);
            egui::Area::new(id.with("list"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .constrain(true)
                .show(&ctx, |ui| {
                    ui.set_width(w);
                    suggestion_list(ui, snap, local.sel)
                })
                .inner
        } else {
            suggestion_list(ui, snap, local.sel)
        };
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN_KEY), pass));
    }
    if let Some(i) = picked
        && let Some(snap) = local.snap.take()
    {
        let c = accept(text, snap.start, snap.cursor, &snap.items[i]);
        let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
        put_cursor(&mut state, &ctx, id, c, c);
        ctx.memory_mut(|m| m.request_focus(id));
        ctx.request_repaint();
    }

    if has_focus && let Some((p, _)) = cursor {
        let list = local.snap.as_ref().map(|_| local.chosen);
        status_line(ui, text, p, lang, list);
    }
    ctx.data_mut(|d| d.insert_temp(key, local));
    response
}

/// The list of suggestions under the box. Returns the one that was pressed.
fn suggestion_list(ui: &mut egui::Ui, snap: &Snap, sel: usize) -> Option<usize> {
    let p = pal();
    let mut picked = None;
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(4))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            for (i, it) in snap.items.iter().enumerate() {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
                if i == sel {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(5), p.hot);
                } else if resp.hovered() {
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(5), p.idle);
                }
                ui.painter().text(
                    rect.left_center() + vec2(8.0, 0.0),
                    Align2::LEFT_CENTER,
                    &it.label,
                    FontId::monospace(FONT),
                    color_of(kind_tok(it.kind), p.dark),
                );
                ui.painter().text(
                    rect.right_center() - vec2(8.0, 0.0),
                    Align2::RIGHT_CENTER,
                    &it.detail,
                    FontId::proportional(11.5),
                    p.muted,
                );
                if resp.hovered() && pressed {
                    picked = Some(i);
                }
            }
        });
    picked
}

/// What the keys do now, in plain words, under the box while it has the focus: the place of the
/// cursor and, depending on whether the list of suggestions is open (`Some(true)` when the arrows
/// chose an entry), what Tab, Enter and Esc do.
fn status_line(ui: &mut egui::Ui, text: &str, cursor: usize, lang: Lang, list: Option<bool>) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(status_text(text, cursor, lang, list, crate::platform::OS))
                .size(11.0)
                .color(pal().muted),
        )
        .truncate(),
    );
}

/// The words of [`status_line`].
fn status_text(text: &str, cursor: usize, lang: Lang, list: Option<bool>, os: Os) -> String {
    const DOT: &str = "  \u{b7}  ";
    let before: String = text.chars().take(cursor).collect();
    let line = before.matches('\n').count() + 1;
    let col = before.chars().rev().take_while(|c| *c != '\n').count() + 1;
    let keys = match list {
        Some(true) => format!("Enter or Tab: use the suggestion{DOT}Esc: hide the list"),
        Some(false) => format!("Tab: use the suggestion{DOT}Up/Down: choose{DOT}Esc: hide it"),
        None => {
            let m = if os == Os::Mac { "Cmd" } else { "Ctrl" };
            let mut keys = format!("Enter: new line{DOT}Tab: indent");
            if lang == Lang::Shell {
                keys.push_str(&format!("{DOT}{m}+/: comment"));
            }
            keys.push_str(&format!("{DOT}Esc: close"));
            keys
        }
    };
    format!("Line {line}, column {col}   {keys}")
}

// ----------------------------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<(&str, Tok)> {
        tokenize(src, Lang::Shell)
            .into_iter()
            .map(|s| (&src[s.start..s.end], s.tok))
            .collect()
    }

    fn vars() -> Vec<String> {
        vec!["name".into(), "count".into(), "path".into()]
    }

    fn sug(text: &str, force: bool) -> Option<Suggest> {
        suggest(
            text,
            text.chars().count(),
            Lang::Shell,
            &vars(),
            Os::Linux,
            force,
        )
    }

    #[test]
    fn spans_are_ordered_and_on_char_boundaries() {
        let src = "echo \"héllo {{name|sh}} $HOME\" # ünï\nif [ -f a ]; then ls -la; fi 'x";
        let mut end = 0;
        for s in tokenize(src, Lang::Shell) {
            assert!(s.start >= end && s.end > s.start);
            assert!(src.is_char_boundary(s.start) && src.is_char_boundary(s.end));
            end = s.end;
        }
        assert!(end <= src.len());
    }

    #[test]
    fn shell_words_get_their_kind() {
        let t = toks("echo -n hi | grep 'a' # note");
        assert!(t.contains(&("echo", Tok::Command)));
        assert!(t.contains(&("-n", Tok::Flag)));
        assert!(t.contains(&("|", Tok::Op)));
        assert!(t.contains(&("grep", Tok::Command)));
        assert!(t.contains(&("'a'", Tok::Str)));
        assert!(t.contains(&("# note", Tok::Comment)));
        // a name after `echo` is an argument, not a command
        assert!(!toks("echo ls").contains(&("ls", Tok::Command)));
    }

    #[test]
    fn templates_and_variables_stand_out_even_in_strings() {
        let t = toks("echo \"a {{x|sh}} $HOME b\"");
        assert!(t.contains(&("{{x|sh}}", Tok::Template)));
        assert!(t.contains(&("$HOME", Tok::Var)));
        assert!(toks("{{unclosed").contains(&("{{unclosed", Tok::Template)));
    }

    #[test]
    fn a_hash_inside_a_word_is_not_a_comment() {
        assert!(!toks("echo a#b").iter().any(|(_, t)| *t == Tok::Comment));
    }

    #[test]
    fn json_keys_differ_from_values() {
        let src = r#"{"a": "b", "n": 1, "t": true}"#;
        let t: Vec<_> = tokenize(src, Lang::Json)
            .into_iter()
            .map(|s| (&src[s.start..s.end], s.tok))
            .collect();
        assert!(t.contains(&("\"a\"", Tok::Key)));
        assert!(t.contains(&("\"b\"", Tok::Str)));
        assert!(t.contains(&("1", Tok::Number)));
        assert!(t.contains(&("true", Tok::Keyword)));
    }

    #[test]
    fn commands_are_suggested_at_the_start_of_a_command() {
        let s = sug("ec", false).unwrap();
        assert_eq!(s.start, 0);
        assert_eq!(s.items[0].label, "echo");
        // after a pipe too, but not for an argument
        assert!(sug("ls | gr", false).is_some());
        assert!(sug("echo gr", false).is_none());
        assert!(sug("", false).is_none());
        assert!(sug("", true).is_some());
        // a single letter does not interrupt the typing; Ctrl + Space still lists it
        assert!(sug("e", false).is_none());
        assert!(sug("e", true).is_some());
    }

    #[test]
    fn the_hint_under_the_box_says_what_the_keys_do() {
        let t = status_text("echo hi\nls", 10, Lang::Shell, None, Os::Mac);
        assert!(t.starts_with("Line 2, column 3"), "{t}");
        assert!(
            t.contains("Enter: new line") && t.contains("Cmd+/: comment"),
            "{t}"
        );
        // no word the person has to decode, and not the shortcut that macOS keeps for Spotlight
        assert!(!t.contains("Space") && !t.contains("Ln "), "{t}");
        assert!(!status_text("{}", 1, Lang::Json, None, Os::Linux).contains("comment"));
        let open = status_text("ec", 2, Lang::Shell, Some(false), Os::Linux);
        assert!(
            open.contains("Tab: use the suggestion") && !open.contains("Enter"),
            "{open}"
        );
        let chosen = status_text("ec", 2, Lang::Shell, Some(true), Os::Linux);
        assert!(chosen.contains("Enter or Tab"), "{chosen}");
    }

    #[test]
    fn variables_are_suggested_inside_braces() {
        let s = sug("echo {{na", false).unwrap();
        assert_eq!(s.start, 7);
        assert_eq!(s.items[0].label, "name");
        let all = sug("echo {{", false).unwrap();
        // the three of the flow and the built-in ones of a run
        assert_eq!(all.items.len(), 3 + RUN_VARS.len());
        assert!(all.items.iter().any(|i| i.label == "code"));
        // closed again: back to the shell
        assert!(sug("echo {{name}} gr", false).is_none());
    }

    #[test]
    fn filters_are_suggested_after_a_pipe_in_braces() {
        let s = sug("echo {{name|s", false).unwrap();
        assert_eq!(s.items[0].label, "sh");
        let all = sug("echo {{name|", false).unwrap();
        assert!(all.items.iter().any(|i| i.insert == "default:"));
        assert!(sug("echo {{name|default:x", false).is_none());
    }

    #[test]
    fn accepting_a_variable_closes_the_braces_once() {
        let mut t = String::from("echo {{na");
        let it = item("name", "name", "", Kind::Variable);
        let c = accept(&mut t, 7, 9, &it);
        assert_eq!((t.as_str(), c), ("echo {{name}}", 11));
        // the braces were already there (auto-closed)
        let mut t = String::from("echo {{na}}");
        let c = accept(&mut t, 7, 9, &it);
        assert_eq!((t.as_str(), c), ("echo {{name}}", 11));
    }

    #[test]
    fn a_snippet_keeps_the_indent_and_places_the_cursor() {
        let mut t = String::from("  if");
        let s = sug("  if", false).unwrap();
        let it = &s.items[0];
        assert_eq!(it.kind, Kind::Snippet);
        let c = accept(&mut t, s.start, 4, it);
        assert_eq!(t, "  if [  ]; then\n    \n  fi");
        assert_eq!(c, 7);
    }

    #[test]
    fn enter_keeps_the_indent_and_opens_blocks() {
        let mut t = String::from("  echo hi");
        assert_eq!(enter(&mut t, 9), 12);
        assert_eq!(t, "  echo hi\n  ");
        let mut t = String::from("if x; then");
        let c = enter(&mut t, 10);
        assert_eq!((t.as_str(), c), ("if x; then\n  ", 13));
        let mut t = String::from("f() {}");
        let c = enter(&mut t, 5);
        assert_eq!((t.as_str(), c), ("f() {\n  \n}", 8));
    }

    #[test]
    fn tab_indents_and_shift_tab_outdents_lines() {
        let mut t = String::from("a\nb");
        assert_eq!(indent(&mut t, 0, 3, false), (2, 7));
        assert_eq!(t, "  a\n  b");
        assert_eq!(indent(&mut t, 2, 7, true), (0, 3));
        assert_eq!(t, "a\nb");
        // nothing selected: spaces up to the next stop of two
        let mut t = String::from("ab");
        assert_eq!(indent(&mut t, 1, 1, false), (2, 2));
        assert_eq!(t, "a b");
        // a selection that ends at the start of a line leaves that line alone
        let mut t = String::from("a\nb");
        indent(&mut t, 0, 2, false);
        assert_eq!(t, "  a\nb");
    }

    #[test]
    fn comments_toggle_on_and_off() {
        let mut t = String::from("ls\n  pwd");
        let n = t.chars().count();
        let (a, b) = toggle_comment(&mut t, 0, n);
        assert_eq!(t, "# ls\n  # pwd");
        assert!(a <= b && b <= t.chars().count());
        let n = t.chars().count();
        toggle_comment(&mut t, 0, n);
        assert_eq!(t, "ls\n  pwd");
    }

    #[test]
    fn multibyte_text_is_edited_by_characters() {
        let mut t = String::from("é\nü");
        let (a, b) = indent(&mut t, 0, 3, false);
        assert_eq!((a, b, t.as_str()), (2, 7, "  é\n  ü"));
        assert!(tokenize("echo \"é\\é\" ü", Lang::Shell).iter().all(|s| {
            "echo \"é\\é\" ü".is_char_boundary(s.start) && "echo \"é\\é\" ü".is_char_boundary(s.end)
        }));
    }

    fn texts(src: &str, lang: Lang, tok: Tok) -> Vec<&str> {
        tokenize(src, lang)
            .into_iter()
            .filter(|s| s.tok == tok)
            .map(|s| &src[s.start..s.end])
            .collect()
    }

    #[test]
    fn unknown_filters_and_functions_are_marked() {
        assert_eq!(texts("{{x|nope}}", Lang::Value, Tok::Bad), ["nope"]);
        assert!(texts("{{x|sh}} {{y|default:a}}", Lang::Value, Tok::Bad).is_empty());
        assert_eq!(
            texts("{{= nope(1) + round(2)}}", Lang::Value, Tok::Bad),
            ["nope"]
        );
        assert!(texts("{{= round(2)}} and (x)", Lang::Value, Tok::Bad).is_empty());
        // the shell gets the same
        assert_eq!(texts("echo {{x|zzz}}", Lang::Shell, Tok::Bad), ["zzz"]);
        // all of it is still covered, in order
        let src = "a {{x|nope|sh}} b";
        let mut end = 0;
        for s in tokenize(src, Lang::Value) {
            assert!(s.start >= end && s.end > s.start);
            end = s.end;
        }
    }

    #[test]
    fn expressions_offer_functions_and_run_variables() {
        let s = sug("echo {{= ro", false).unwrap();
        assert_eq!(s.items[0].label, "round");
        assert_eq!(s.items[0].kind, Kind::Function);
        let co = sug("echo {{= co", false).unwrap();
        assert!(co.items.iter().any(|i| i.label == "count"));
        assert!(co.items.iter().any(|i| i.label == "code"));
        let mut t = String::from("echo {{= ro");
        let n = t.chars().count();
        let c = accept(&mut t, s.start, n, &s.items[0].clone());
        assert_eq!(t, "echo {{= round()");
        assert_eq!(c, t.chars().count() - 1);
    }

    #[test]
    fn flags_and_wrapped_variables_in_arguments() {
        let s = sug("grep -", false).unwrap();
        assert!(
            s.items
                .iter()
                .any(|i| i.label == "-i" && i.kind == Kind::Flag)
        );
        assert!(sug("echo -", false).is_none());
        assert!(sug("ls | grep -", false).is_some());
        // a variable in an argument: only when asked for, and quoted for the shell
        assert!(sug("echo na", false).is_none());
        let s = sug("echo na", true).unwrap();
        assert_eq!(s.items[0].insert, "{{name|sh}}");
        let mut t = String::from("echo na");
        accept(&mut t, s.start, 7, &s.items[0]);
        assert_eq!(t, "echo {{name|sh}}");
    }

    #[test]
    fn fuzzy_matching_ranks_prefixes_first() {
        assert_eq!(fit("grep", "gr"), Some(0));
        assert_eq!(fit("notify-send", "send"), Some(1));
        assert_eq!(fit("curl", "crl"), Some(2));
        assert_eq!(fit("curl", "c"), Some(0));
        assert_eq!(fit("curl", "z"), None);
        let s = sug("ntf", false).unwrap();
        assert!(s.items.iter().any(|i| i.label == "notify-send"));
    }

    #[test]
    fn the_window_escape_waits_for_an_open_list() {
        let ctx = egui::Context::default();
        assert!(!escape_is_taken(&ctx));
        let pass = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN_KEY), pass));
        assert!(escape_is_taken(&ctx));
    }

    /// One frame of a resizable window with the filling editor, in a screen of `w` x `h`.
    fn frame_fill(
        ctx: &egui::Context,
        text: &mut String,
        events: Vec<egui::Event>,
        w: f32,
        h: f32,
    ) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w, h),
            )),
            events,
            ..Default::default()
        };
        let out = ctx.run_ui(input, |ui| {
            egui::Window::new("t")
                .id(egui::Id::new("win_t"))
                .default_size(egui::vec2(420.0, 200.0))
                .resizable(true)
                .show(ui.ctx(), |ui| {
                    let r = code_edit_fill(ui, "t", text, Lang::Shell, "");
                    r.request_focus();
                });
        });
        std::mem::forget(out.textures_delta);
    }

    #[test]
    fn the_filling_editor_runs_in_a_window_and_survives_resizing() {
        let ctx = egui::Context::default();
        let mut text = String::new();
        frame_fill(&ctx, &mut text, vec![], 900.0, 600.0);
        frame_fill(
            &ctx,
            &mut text,
            vec![egui::Event::Text("gr".into())],
            900.0,
            600.0,
        );
        assert_eq!(text, "gr");
        // the list is open: the window must leave Esc alone
        frame_fill(&ctx, &mut text, vec![], 900.0, 600.0);
        assert!(escape_is_taken(&ctx));
        // the size it settles at does not move, however many frames go by
        for _ in 0..5 {
            frame_fill(&ctx, &mut text, vec![], 900.0, 600.0);
        }
        let r1 = ctx.memory(|m| m.area_rect(egui::Id::new("win_t"))).unwrap();
        for _ in 0..30 {
            frame_fill(&ctx, &mut text, vec![], 900.0, 600.0);
        }
        let r2 = ctx.memory(|m| m.area_rect(egui::Id::new("win_t"))).unwrap();
        assert_eq!(r1, r2, "the window keeps growing");
        assert!(r2.height() < 300.0 && r2.width() < 500.0, "{r2:?}");
        // Enter with the list open is a new line: it does not swap `gr` for `grep`
        let key = |k: egui::Key| egui::Event::Key {
            key: k,
            physical_key: Some(k),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(text, "gr");
        frame_fill(&ctx, &mut text, vec![key(egui::Key::Enter)], 900.0, 600.0);
        assert_eq!(text, "gr\n");
        // a tiny screen and a long text do not panic
        let mut long = "ls\n".repeat(200);
        frame_fill(&ctx, &mut long, vec![], 200.0, 120.0);
    }
}
