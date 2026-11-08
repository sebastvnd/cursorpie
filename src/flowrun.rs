//! Runs a [`Flow`]: the interpreter behind [`Action::Flow`](crate::config::Action::Flow).
//!
//! A run starts at the *Start* block and follows the wires: a plain block does its work and leads
//! to the block its `next` wire names, a *Branch* leads to `next` or `alt` by its condition, and a
//! *Repeat* or *While* leads into its body (`next`) round after round and to `alt` when it is done.
//! The run ends at *End*, at *Stop*, at an output without a wire, or when it hits a limit
//! ([`MAX_FLOW_STEPS`] blocks, [`MAX_FLOW_RUN_SECS`] seconds); nothing a flow contains can keep the
//! worker thread busy for ever.
//!
//! ## Variables
//!
//! A run has variables: names ([`is_var_name`]) with a text as value. A block writes one (*Set*,
//! *Calc*, *Logic*, *Ask*, *Run*, *Native*) and any text field reads them: `{{name}}` is replaced
//! by the value, `{{name|sh}}` by the value quoted for the shell of the OS ([`quote_for_shell`]).
//! Only a defined name is replaced, so a command that has `{{x}}` in it for another reason keeps it.
//! The operands of *Calc*, *Logic*, *Branch* and *While* are a value or the bare name of a variable.
//! A *Run* block also hands every variable to its command in the environment (`CURSORPIE_NAME`),
//! which is the safe way to pass text a person typed. Some variables are set by the blocks
//! themselves: `ok` (1 when the last block that can fail worked), `code` (the exit code of the last
//! *Run*) and `index` (the round of the innermost loop, from 1).
//!
//! ## Why a [`Host`]
//!
//! The interpreter never touches the system: it asks a [`Host`] to press a key, ask a question or
//! read a file. [`actions`](crate::actions) has the real one, built from [`platform`](crate::platform);
//! the tests below have a recording one. That keeps every decision (loops, gates, limits) testable
//! on any OS.

use crate::config::flow::is_var_name;
use crate::config::{
    AskMode, CalcOp, CmpOp, Cond, Flow, FlowNode, LogicOp, MAX_CALL_DEPTH, MAX_FLOW_RUN_SECS,
    MAX_FLOW_STEPS, MAX_REPEAT, MAX_RUN_SECS, MAX_VALUE_CHARS, MAX_VARS, MAX_WAIT_MS, NativeOp,
    NodeKind,
};
use crate::keys::Shortcut;
use crate::platform::Os;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The pause between two blocks that do something, in milliseconds: the target app needs a moment
/// to see one key before the next one arrives.
const GAP_MS: u64 = 40;
/// Time the app gets to take the pasted text of a *Type text* block, and to see the shortcut.
const PASTE_SETTLE_MS: u64 = 60;
/// Time the paste shortcut gets before the clipboard is put back.
const PASTE_DONE_MS: u64 = 200;
/// The macOS virtual key code of V, the key of the paste shortcut.
const KEY_V: u16 = 9;
/// How many variables a *Run* block hands to its command.
const MAX_ENV_VARS: usize = 64;

/// What a finished *Run* block's command left behind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecResult {
    /// The exit code; `None` when the program was ended by a signal or by the timeout.
    pub code: Option<i32>,
    /// What it printed (the start of it, when it printed a lot).
    pub stdout: String,
}

/// What the interpreter needs from the system. Every method is allowed to fail quietly: the
/// interpreter turns a `None` or `false` into `ok = 0` and goes on.
pub trait Host {
    /// Presses a shortcut.
    fn key(&mut self, sc: Shortcut);
    /// Opens or raises an app (name or path, already trimmed and never empty).
    fn open_app(&mut self, app: &str);
    /// Opens a URL or a file (already trimmed and never empty).
    fn open_url(&mut self, target: &str);
    /// Starts a shell command and does not wait for it.
    fn command(&mut self, command: &str);
    /// Pauses the run.
    fn sleep(&mut self, ms: u64);
    /// Runs a shell command with the extra environment `env`, waits for it for at most `timeout`,
    /// and returns what it left. `None` when it could not be started.
    fn exec(
        &mut self,
        command: &str,
        env: &[(String, String)],
        timeout: Duration,
    ) -> Option<ExecResult>;
    /// Asks for a text in a window of the system. `None` when it was cancelled or could not be shown.
    fn ask_text(&mut self, message: &str, preset: &str) -> Option<String>;
    /// Asks a yes or no question. `None` when it could not be shown.
    fn ask_yes_no(&mut self, message: &str) -> Option<bool>;
    /// The text on the clipboard.
    fn clipboard(&mut self) -> Option<String>;
    /// Puts a text on the clipboard; whether it worked.
    fn set_clipboard(&mut self, text: &str) -> bool;
    /// Shows a desktop notification; whether it could be shown.
    fn notify(&mut self, title: &str, text: &str) -> bool;
    /// The text of a file (`None` when it cannot be read as text or is too big).
    fn read_file(&mut self, path: &str) -> Option<String>;
    /// Writes (or, with `append`, adds to) a file; whether it worked.
    fn write_file(&mut self, path: &str, text: &str, append: bool) -> bool;
    /// Whether a file or folder exists.
    fn file_exists(&mut self, path: &str) -> bool;
    /// An environment variable of CursorPie.
    fn env_var(&mut self, name: &str) -> Option<String>;
    /// The id of the app in front.
    fn front_app(&mut self) -> Option<String>;
    /// Brings an app to the front.
    fn activate_app(&mut self, app: &str);
    /// Whether an app has a window.
    fn app_running(&mut self, app: &str) -> bool;
    /// The position of the pointer.
    fn cursor_pos(&mut self) -> Option<(f64, f64)>;
    /// A whole number from 0 up to, not including, `below` (which is at least 1).
    fn random_below(&mut self, below: u64) -> u64;
    /// Seconds since 1 January 1970 (UTC).
    fn unix_time(&mut self) -> u64;
    /// Shows a file or folder in the file manager.
    fn reveal(&mut self, path: &str);
    /// The flow with this id, copied out of the config.
    fn flow(&mut self, id: &str) -> Option<Flow>;
}

/// The variables of a run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vars(BTreeMap<String, String>);

impl Vars {
    /// The value of a variable.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    /// Sets a variable. A value longer than [`MAX_VALUE_CHARS`] is cut. Returns `false` (and
    /// changes nothing) for a name [`is_var_name`] refuses, or when there are already
    /// [`MAX_VARS`] variables and this one is new.
    pub fn set(&mut self, name: &str, value: impl Into<String>) -> bool {
        if !is_var_name(name) || (self.0.len() >= MAX_VARS && !self.0.contains_key(name)) {
            return false;
        }
        let mut value: String = value.into();
        if value.chars().count() > MAX_VALUE_CHARS {
            value = value.chars().take(MAX_VALUE_CHARS).collect();
        }
        self.0.insert(name.to_string(), value);
        true
    }

    /// Every variable, by name.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ending {
    /// At *End*, or at an output without a wire.
    Finished,
    /// At a *Stop* block.
    Stopped,
    /// After [`MAX_FLOW_STEPS`] blocks.
    OutOfSteps,
    /// After [`MAX_FLOW_RUN_SECS`] seconds.
    OutOfTime,
}

/// What a run leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ending: Ending,
    pub vars: Vars,
}

/// Runs `flow` on `host` for the OS `os` (it decides the paste shortcut and the shell quoting).
/// Blocks the calling thread until the run ends: call it from the actions worker.
pub fn run<H: Host>(host: &mut H, flow: &Flow, os: Os) -> Outcome {
    let mut runner = Runner {
        host,
        os,
        vars: Vars::default(),
        steps: 0,
        deadline: Instant::now() + Duration::from_secs(MAX_FLOW_RUN_SECS),
    };
    let ending = match runner.run_flow(flow, 0) {
        Ok(()) => Ending::Finished,
        Err(e) => e,
    };
    Outcome {
        ending,
        vars: runner.vars,
    }
}

// ------------------------------------------------------------------ pure helpers

/// Whether a text counts as true: anything but empty, `0`, `false`, `no` and `off` (any case).
pub fn is_truthy(s: &str) -> bool {
    let t = s.trim().to_ascii_lowercase();
    !matches!(t.as_str(), "" | "0" | "false" | "no" | "off")
}

/// The number a text spells, if it spells a finite one.
pub fn parse_number(s: &str) -> Option<f64> {
    let n: f64 = s.trim().parse().ok()?;
    n.is_finite().then_some(n)
}

/// A number as text: a whole number without a decimal point.
pub fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// `1` or `0`, the text of a true or false result.
fn flag(b: bool) -> &'static str {
    if b { "1" } else { "0" }
}

/// Whether `a op b` holds. Two numbers are compared as numbers (`1` equals `1.0`), anything else
/// as text, letter by letter.
pub fn compare(op: CmpOp, a: &str, b: &str) -> bool {
    use std::cmp::Ordering;
    let (a, b) = (a.trim(), b.trim());
    let order = || match (parse_number(a), parse_number(b)) {
        (Some(x), Some(y)) => x.partial_cmp(&y),
        _ => Some(a.cmp(b)),
    };
    match op {
        CmpOp::Eq => order() == Some(Ordering::Equal),
        CmpOp::Ne => order() != Some(Ordering::Equal),
        CmpOp::Lt => order() == Some(Ordering::Less),
        CmpOp::Le => matches!(order(), Some(Ordering::Less | Ordering::Equal)),
        CmpOp::Gt => order() == Some(Ordering::Greater),
        CmpOp::Ge => matches!(order(), Some(Ordering::Greater | Ordering::Equal)),
        CmpOp::Contains => a.contains(b),
        CmpOp::StartsWith => a.starts_with(b),
        CmpOp::EndsWith => a.ends_with(b),
        CmpOp::IsTrue => is_truthy(a),
        CmpOp::IsEmpty => a.is_empty(),
    }
}

/// `a op b` as numbers; `None` when a text is not a number, on a division by zero, or when the
/// result is not finite.
pub fn calc(op: CalcOp, a: &str, b: &str) -> Option<f64> {
    let (x, y) = (parse_number(a)?, parse_number(b)?);
    let r = match op {
        CalcOp::Add => x + y,
        CalcOp::Sub => x - y,
        CalcOp::Mul => x * y,
        CalcOp::Div => {
            if y == 0.0 {
                return None;
            }
            x / y
        }
        CalcOp::Rem => {
            if y == 0.0 {
                return None;
            }
            x % y
        }
        CalcOp::Min => x.min(y),
        CalcOp::Max => x.max(y),
    };
    r.is_finite().then_some(r)
}

/// The gate `op` on two texts read as true or false.
pub fn logic(op: LogicOp, a: &str, b: &str) -> bool {
    let (x, y) = (is_truthy(a), is_truthy(b));
    match op {
        LogicOp::And => x && y,
        LogicOp::Or => x || y,
        LogicOp::Xor => x != y,
        LogicOp::Nand => !(x && y),
        LogicOp::Nor => !(x || y),
        LogicOp::Not => !x,
    }
}

/// `s` as one word for the shell of `os`. Unix: single quotes, a quote inside is closed, escaped
/// and opened again. Windows (`cmd.exe`): double quotes, and the characters that cannot be quoted
/// safely there (`"`, `%` and line breaks) are dropped.
pub fn quote_for_shell(os: Os, s: &str) -> String {
    match os {
        Os::Windows => {
            let kept: String = s
                .chars()
                .filter(|c| !matches!(c, '"' | '%' | '\n' | '\r'))
                .collect();
            format!("\"{kept}\"")
        }
        Os::Mac | Os::Linux => format!("'{}'", s.replace('\'', "'\\''")),
    }
}

/// `text` with every `{{name}}` of a defined variable replaced by its value, and every
/// `{{name|sh}}` by the value quoted for the shell of `os`. Anything else stays as written.
pub fn expand(text: &str, vars: &Vars, os: Os) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = &after[..close];
        match fill(inner, vars, os) {
            Some(v) => out.push_str(&v),
            None => {
                out.push_str("{{");
                out.push_str(inner);
                out.push_str("}}");
            }
        }
        rest = &after[close + 2..];
    }
    out.push_str(rest);
    out
}

/// The value for what stands between `{{` and `}}`, if it is a defined variable and a known filter.
fn fill(inner: &str, vars: &Vars, os: Os) -> Option<String> {
    let (name, filter) = match inner.split_once('|') {
        Some((n, f)) => (n.trim(), Some(f.trim())),
        None => (inner.trim(), None),
    };
    let value = vars.get(name)?;
    match filter {
        None => Some(value.to_string()),
        Some("sh") => Some(quote_for_shell(os, value)),
        Some(_) => None,
    }
}

// ------------------------------------------------------------------ the interpreter

/// The state of one run.
struct Runner<'a, H: Host> {
    host: &'a mut H,
    os: Os,
    vars: Vars,
    /// Blocks carried out so far, called flows included.
    steps: u32,
    /// When the run is stopped. Moved later by the time a question stays open.
    deadline: Instant,
}

impl<H: Host> Runner<'_, H> {
    /// Runs one flow to its end. `Err` stops the whole run (a *Stop* block or a limit), also in
    /// the flows that called this one.
    fn run_flow(&mut self, flow: &Flow, depth: u8) -> Result<(), Ending> {
        // how many rounds each loop block has made in this run of this flow
        let mut rounds: BTreeMap<u32, u32> = BTreeMap::new();
        let mut cur = flow.start().and_then(|s| s.next);
        while let Some(id) = cur {
            self.tick()?;
            let Some(node) = flow.node(id) else {
                break;
            };
            cur = self.step(node, depth, &mut rounds)?;
        }
        Ok(())
    }

    /// Counts one more block against the limits.
    fn tick(&mut self) -> Result<(), Ending> {
        self.steps += 1;
        if self.steps > MAX_FLOW_STEPS {
            return Err(Ending::OutOfSteps);
        }
        if Instant::now() >= self.deadline {
            return Err(Ending::OutOfTime);
        }
        Ok(())
    }

    /// The text of a field with its variables filled in.
    fn text(&self, s: &str) -> String {
        expand(s, &self.vars, self.os)
    }

    /// An operand: the value of the variable it names, or else the text itself (filled in).
    fn operand(&self, s: &str) -> String {
        let t = s.trim();
        if is_var_name(t)
            && let Some(v) = self.vars.get(t)
        {
            return v.to_string();
        }
        self.text(t)
    }

    /// Whether a condition holds.
    fn holds(&self, c: &Cond) -> bool {
        compare(c.op, &self.operand(&c.a), &self.operand(&c.b))
    }

    /// Sets a variable; a bad name is ignored.
    fn put(&mut self, name: &str, value: impl Into<String>) {
        let _ = self.vars.set(name.trim(), value);
    }

    /// Records whether the last block that can fail worked.
    fn ok(&mut self, ok: bool) {
        self.put("ok", flag(ok));
    }

    /// The pause after a block that did something.
    fn gap(&mut self) {
        self.host.sleep(GAP_MS);
    }

    /// Carries out one block and returns the block to go to next.
    fn step(
        &mut self,
        node: &FlowNode,
        depth: u8,
        rounds: &mut BTreeMap<u32, u32>,
    ) -> Result<Option<u32>, Ending> {
        match &node.kind {
            NodeKind::Start => {}
            NodeKind::End => return Ok(None),
            NodeKind::Stop => return Err(Ending::Stopped),
            NodeKind::Key(sc) => {
                self.host.key(*sc);
                self.gap();
            }
            NodeKind::OpenApp(s) => {
                let app = self.text(s);
                if !app.trim().is_empty() {
                    self.host.open_app(app.trim());
                }
                self.gap();
            }
            NodeKind::OpenUrl(s) => {
                let target = self.text(s);
                if !target.trim().is_empty() {
                    self.host.open_url(target.trim());
                }
                self.gap();
            }
            NodeKind::Command(s) => {
                let command = self.text(s);
                if !command.trim().is_empty() {
                    self.host.command(&command);
                }
                self.gap();
            }
            NodeKind::Wait(ms) => {
                self.host.sleep(u64::from((*ms).min(MAX_WAIT_MS)));
                self.gap();
            }
            NodeKind::Set { name, value } => {
                let v = self.text(value);
                self.put(name, v);
            }
            NodeKind::Calc { out, a, op, b } => {
                let (x, y) = (self.operand(a), self.operand(b));
                match calc(*op, &x, &y) {
                    Some(r) => {
                        self.put(out, format_number(r));
                        self.ok(true);
                    }
                    None => {
                        self.put(out, "");
                        self.ok(false);
                    }
                }
            }
            NodeKind::Logic { out, a, op, b } => {
                let (x, y) = (self.operand(a), self.operand(b));
                self.put(out, flag(logic(*op, &x, &y)));
            }
            NodeKind::Branch(c) => {
                return Ok(if self.holds(c) { node.next } else { node.alt });
            }
            NodeKind::Repeat(n) => {
                let round = rounds.get(&node.id).copied().unwrap_or(0);
                if round < (*n).min(MAX_REPEAT) {
                    return Ok(self.enter_loop(node, rounds, round));
                }
                rounds.remove(&node.id);
                return Ok(node.alt);
            }
            NodeKind::While(c) => {
                let round = rounds.get(&node.id).copied().unwrap_or(0);
                if round < MAX_REPEAT && self.holds(c) {
                    return Ok(self.enter_loop(node, rounds, round));
                }
                rounds.remove(&node.id);
                return Ok(node.alt);
            }
            NodeKind::Ask {
                mode,
                message,
                preset,
                out,
            } => self.ask(*mode, message, preset, out),
            NodeKind::Run {
                command,
                out,
                timeout_s,
            } => self.run_command(command, out, *timeout_s),
            NodeKind::Native { op, a, b } => {
                self.native(*op, a, b);
                self.gap();
            }
            NodeKind::Call(id) => {
                let id = self.text(id);
                if depth < MAX_CALL_DEPTH
                    && let Some(sub) = self.host.flow(id.trim())
                {
                    self.run_flow(&sub, depth + 1)?;
                }
            }
        }
        Ok(node.next)
    }

    /// Starts round `round + 1` of a loop block: counts it, sets `index` and leads into the body.
    fn enter_loop(
        &mut self,
        node: &FlowNode,
        rounds: &mut BTreeMap<u32, u32>,
        round: u32,
    ) -> Option<u32> {
        rounds.insert(node.id, round + 1);
        self.put("index", (round + 1).to_string());
        node.next
    }

    /// An *Ask* block. The time the question stays open does not count against the run.
    fn ask(&mut self, mode: AskMode, message: &str, preset: &str, out: &str) {
        let message = self.text(message);
        let preset = self.text(preset);
        let asked = Instant::now();
        let (answer, ok) = match mode {
            AskMode::Text => match self.host.ask_text(&message, &preset) {
                Some(t) => (t, true),
                None => (String::new(), false),
            },
            AskMode::YesNo => match self.host.ask_yes_no(&message) {
                Some(yes) => (flag(yes).to_string(), true),
                None => ("0".to_string(), false),
            },
        };
        self.deadline += asked.elapsed();
        self.put(out, answer);
        self.ok(ok);
    }

    /// A *Run* block: runs the command, waits, and keeps what it printed and its exit code.
    fn run_command(&mut self, command: &str, out: &str, timeout_s: u32) {
        let command = self.text(command);
        if command.trim().is_empty() {
            return;
        }
        let env: Vec<(String, String)> = self
            .vars
            .iter()
            .take(MAX_ENV_VARS)
            .map(|(k, v)| {
                (
                    format!("CURSORPIE_{}", k.to_ascii_uppercase()),
                    v.to_string(),
                )
            })
            .collect();
        let timeout = Duration::from_secs(u64::from(timeout_s.clamp(1, MAX_RUN_SECS)));
        match self.host.exec(&command, &env, timeout) {
            Some(r) => {
                let code = r.code.unwrap_or(-1);
                self.put(out, r.stdout.trim_end_matches(['\r', '\n']));
                self.put("code", code.to_string());
                self.ok(code == 0);
            }
            None => {
                self.put(out, "");
                self.put("code", "-1");
                self.ok(false);
            }
        }
        self.gap();
    }

    /// A *Native* block: what the operating system does or tells.
    fn native(&mut self, op: NativeOp, a: &str, b: &str) {
        // a field that names a variable is used as the name; the others have their variables filled in
        let ta = if op.a_is_variable() {
            a.trim().to_string()
        } else {
            self.text(a)
        };
        let tb = if op.b_is_variable() {
            b.trim().to_string()
        } else {
            self.text(b)
        };
        let ok = match op {
            NativeOp::ClipboardSet => self.host.set_clipboard(&ta),
            NativeOp::ClipboardGet => {
                let got = self.host.clipboard();
                let ok = got.is_some();
                self.put(&tb, got.unwrap_or_default());
                ok
            }
            NativeOp::TypeText => self.type_text(&ta),
            NativeOp::Notify => self.host.notify(&ta, &tb),
            NativeOp::ReadFile => {
                let got = self.host.read_file(&ta);
                let ok = got.is_some();
                self.put(&tb, got.unwrap_or_default());
                ok
            }
            NativeOp::WriteFile => self.host.write_file(&ta, &tb, false),
            NativeOp::AppendFile => self.host.write_file(&ta, &tb, true),
            NativeOp::FileExists => {
                let exists = self.host.file_exists(&ta);
                self.put(&tb, flag(exists));
                true
            }
            NativeOp::Env => {
                let got = self.host.env_var(ta.trim());
                let ok = got.is_some();
                self.put(&tb, got.unwrap_or_default());
                ok
            }
            NativeOp::FrontApp => {
                let got = self.host.front_app();
                let ok = got.is_some();
                self.put(&tb, got.unwrap_or_default());
                ok
            }
            NativeOp::ActivateApp => {
                if !ta.trim().is_empty() {
                    self.host.activate_app(ta.trim());
                }
                true
            }
            NativeOp::AppRunning => {
                let running = self.host.app_running(ta.trim());
                self.put(&tb, flag(running));
                true
            }
            NativeOp::CursorPos => match self.host.cursor_pos() {
                Some((x, y)) => {
                    self.put(&ta, format_number(x.round()));
                    self.put(&tb, format_number(y.round()));
                    true
                }
                None => {
                    self.put(&ta, "");
                    self.put(&tb, "");
                    false
                }
            },
            NativeOp::Random => {
                let below = parse_number(&ta).map_or(100, |n| n.clamp(1.0, 1e9) as u64);
                let n = self.host.random_below(below.max(1));
                self.put(&tb, n.to_string());
                true
            }
            NativeOp::UnixTime => {
                let t = self.host.unix_time();
                self.put(&tb, t.to_string());
                true
            }
            NativeOp::Reveal => {
                if !ta.trim().is_empty() {
                    self.host.reveal(ta.trim());
                }
                true
            }
        };
        self.ok(ok);
    }

    /// Types a text into the app in front: it goes on the clipboard, the paste shortcut is
    /// pressed, and what the clipboard held is put back. (Sending each character as a key would
    /// depend on the keyboard layout; pasting does not.)
    fn type_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return true;
        }
        let saved = self.host.clipboard();
        let set = self.host.set_clipboard(text);
        if set {
            self.host.sleep(PASTE_SETTLE_MS);
            self.host.key(Shortcut::new(KEY_V, self.os.primary_mod()));
            self.host.sleep(PASTE_DONE_MS);
        }
        if let Some(old) = saved {
            let _ = self.host.set_clipboard(&old);
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{MAX_FIELD_CHARS, Port};

    /// A host that does nothing but write down what it was asked, and answers from a script.
    #[derive(Default)]
    struct Mock {
        log: Vec<String>,
        clip: Option<String>,
        texts: Vec<Option<String>>,
        yes: Vec<Option<bool>>,
        exec: Option<ExecResult>,
        files: BTreeMap<String, String>,
        flows: Vec<Flow>,
        env: BTreeMap<String, String>,
        front: Option<String>,
        slept: u64,
    }

    impl Host for Mock {
        fn key(&mut self, sc: Shortcut) {
            self.log.push(format!("key {} {}", sc.key, sc.mods));
        }
        fn open_app(&mut self, app: &str) {
            self.log.push(format!("app {app}"));
        }
        fn open_url(&mut self, target: &str) {
            self.log.push(format!("url {target}"));
        }
        fn command(&mut self, command: &str) {
            self.log.push(format!("cmd {command}"));
        }
        fn sleep(&mut self, ms: u64) {
            self.slept += ms;
        }
        fn exec(
            &mut self,
            command: &str,
            env: &[(String, String)],
            timeout: Duration,
        ) -> Option<ExecResult> {
            let vars: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
            self.log.push(format!(
                "exec {command} [{}] {}s",
                vars.join(","),
                timeout.as_secs()
            ));
            self.exec.clone()
        }
        fn ask_text(&mut self, message: &str, preset: &str) -> Option<String> {
            self.log.push(format!("ask {message} / {preset}"));
            if self.texts.is_empty() {
                None
            } else {
                self.texts.remove(0)
            }
        }
        fn ask_yes_no(&mut self, message: &str) -> Option<bool> {
            self.log.push(format!("yesno {message}"));
            if self.yes.is_empty() {
                None
            } else {
                self.yes.remove(0)
            }
        }
        fn clipboard(&mut self) -> Option<String> {
            self.clip.clone()
        }
        fn set_clipboard(&mut self, text: &str) -> bool {
            self.log.push(format!("clip {text}"));
            self.clip = Some(text.to_string());
            true
        }
        fn notify(&mut self, title: &str, text: &str) -> bool {
            self.log.push(format!("notify {title}|{text}"));
            true
        }
        fn read_file(&mut self, path: &str) -> Option<String> {
            self.files.get(path).cloned()
        }
        fn write_file(&mut self, path: &str, text: &str, append: bool) -> bool {
            let e = self.files.entry(path.to_string()).or_default();
            if !append {
                e.clear();
            }
            e.push_str(text);
            true
        }
        fn file_exists(&mut self, path: &str) -> bool {
            self.files.contains_key(path)
        }
        fn env_var(&mut self, name: &str) -> Option<String> {
            self.env.get(name).cloned()
        }
        fn front_app(&mut self) -> Option<String> {
            self.front.clone()
        }
        fn activate_app(&mut self, app: &str) {
            self.log.push(format!("activate {app}"));
        }
        fn app_running(&mut self, app: &str) -> bool {
            app == "running.app"
        }
        fn cursor_pos(&mut self) -> Option<(f64, f64)> {
            Some((10.4, 20.6))
        }
        fn random_below(&mut self, below: u64) -> u64 {
            below - 1
        }
        fn unix_time(&mut self) -> u64 {
            1_700_000_000
        }
        fn reveal(&mut self, path: &str) {
            self.log.push(format!("reveal {path}"));
        }
        fn flow(&mut self, id: &str) -> Option<Flow> {
            self.flows.iter().find(|f| f.id == id).cloned()
        }
    }

    const OS: Os = Os::Linux;

    fn set(name: &str, value: &str) -> NodeKind {
        NodeKind::Set {
            name: name.into(),
            value: value.into(),
        }
    }

    fn cond(a: &str, op: CmpOp, b: &str) -> Cond {
        Cond {
            a: a.into(),
            op,
            b: b.into(),
        }
    }

    /// Start -> kinds in a row -> End.
    fn line(kinds: Vec<NodeKind>) -> Flow {
        let mut f = Flow::new("f", "F");
        for k in kinds {
            f.add_after(None, k);
        }
        f
    }

    /// A block that is not in the chain yet, with no wires.
    fn loose(f: &mut Flow, kind: NodeKind) -> u32 {
        let id = f.next_node_id();
        f.nodes.push(FlowNode {
            id,
            x: 0.0,
            y: 0.0,
            kind,
            next: None,
            alt: None,
        });
        id
    }

    fn go(f: &Flow, host: &mut Mock) -> Outcome {
        run(host, f, OS)
    }

    // ---- the pure helpers

    #[test]
    fn variables_are_filled_in_only_when_they_are_defined() {
        let mut v = Vars::default();
        assert!(v.set("who", "Ann"));
        assert_eq!(expand("hi {{who}}!", &v, OS), "hi Ann!");
        assert_eq!(expand("hi {{ who }}!", &v, OS), "hi Ann!");
        assert_eq!(expand("{{nobody}} {{who}}", &v, OS), "{{nobody}} Ann");
        assert_eq!(expand("awk '{{print}}'", &v, OS), "awk '{{print}}'");
        assert_eq!(expand("open {{who", &v, OS), "open {{who");
        assert_eq!(expand("{{{who}}}", &v, OS), "{{{who}}}");
        assert_eq!(expand("{{who|nonsense}}", &v, OS), "{{who|nonsense}}");
        assert_eq!(expand("no braces", &v, OS), "no braces");
    }

    #[test]
    fn a_value_for_the_shell_is_quoted_so_it_stays_one_word() {
        let mut v = Vars::default();
        v.set("x", "it's; rm -rf ~");
        assert_eq!(
            expand("echo {{x|sh}}", &v, Os::Linux),
            "echo 'it'\\''s; rm -rf ~'"
        );
        assert_eq!(quote_for_shell(Os::Mac, "a b"), "'a b'");
        assert_eq!(quote_for_shell(Os::Windows, "a\"b%c\nd"), "\"abcd\"");
    }

    #[test]
    fn a_variable_keeps_a_bounded_value_and_the_number_of_variables_is_bounded() {
        let mut v = Vars::default();
        assert!(!v.set("1bad", "x"));
        assert!(!v.set("", "x"));
        assert!(v.set("big", "y".repeat(MAX_VALUE_CHARS + 10)));
        assert_eq!(v.get("big").unwrap().chars().count(), MAX_VALUE_CHARS);
        for i in 0..MAX_VARS {
            v.set(&format!("v{i}"), "1");
        }
        assert!(!v.set("one_too_many", "1"));
        assert!(v.set("big", "short"), "an existing one can still change");
    }

    #[test]
    fn truth_numbers_and_comparisons_follow_simple_rules() {
        for t in ["", " ", "0", "false", "No", "OFF"] {
            assert!(!is_truthy(t), "{t:?}");
        }
        for t in ["1", "yes", "x", "00", "true"] {
            assert!(is_truthy(t), "{t:?}");
        }
        assert_eq!(parse_number(" 2.5 "), Some(2.5));
        assert_eq!(parse_number("nan"), None);
        assert_eq!(parse_number("inf"), None);
        assert_eq!(parse_number("a"), None);
        assert_eq!(format_number(3.0), "3");
        assert_eq!(format_number(-0.0), "0");
        assert_eq!(format_number(2.5), "2.5");
        assert!(compare(CmpOp::Eq, "1", "1.0"), "numbers are numbers");
        assert!(compare(CmpOp::Lt, "9", "10"), "9 < 10 as numbers");
        assert!(compare(CmpOp::Gt, "b", "a"), "text by letters");
        assert!(compare(CmpOp::Ne, "a", "b"));
        assert!(compare(CmpOp::Le, "2", "2") && compare(CmpOp::Ge, "2", "2"));
        assert!(compare(CmpOp::Contains, "hello", "ell"));
        assert!(compare(CmpOp::StartsWith, "hello", "he"));
        assert!(compare(CmpOp::EndsWith, "hello", "lo"));
        assert!(compare(CmpOp::IsTrue, "yes", ""));
        assert!(compare(CmpOp::IsEmpty, "  ", ""));
        assert!(!compare(CmpOp::IsEmpty, "a", ""));
    }

    #[test]
    fn arithmetic_refuses_what_is_not_a_number_and_division_by_zero() {
        assert_eq!(calc(CalcOp::Add, "1", "2"), Some(3.0));
        assert_eq!(calc(CalcOp::Sub, "1", "2"), Some(-1.0));
        assert_eq!(calc(CalcOp::Mul, "3", "4"), Some(12.0));
        assert_eq!(calc(CalcOp::Div, "9", "2"), Some(4.5));
        assert_eq!(calc(CalcOp::Rem, "9", "4"), Some(1.0));
        assert_eq!(calc(CalcOp::Min, "9", "4"), Some(4.0));
        assert_eq!(calc(CalcOp::Max, "9", "4"), Some(9.0));
        assert_eq!(calc(CalcOp::Div, "1", "0"), None);
        assert_eq!(calc(CalcOp::Rem, "1", "0"), None);
        assert_eq!(calc(CalcOp::Add, "a", "1"), None);
        assert_eq!(calc(CalcOp::Mul, "1e308", "10"), None, "not finite");
    }

    #[test]
    fn the_gates_follow_their_truth_tables() {
        let cases = [
            (LogicOp::And, [false, false, false, true]),
            (LogicOp::Or, [false, true, true, true]),
            (LogicOp::Xor, [false, true, true, false]),
            (LogicOp::Nand, [true, true, true, false]),
            (LogicOp::Nor, [true, false, false, false]),
        ];
        for (op, expected) in cases {
            let got: Vec<bool> = [("0", "0"), ("0", "1"), ("1", "0"), ("1", "1")]
                .iter()
                .map(|(a, b)| logic(op, a, b))
                .collect();
            assert_eq!(got, expected, "{op:?}");
        }
        assert!(logic(LogicOp::Not, "0", "1"));
        assert!(!logic(LogicOp::Not, "yes", "0"));
    }

    // ---- running flows

    #[test]
    fn a_plain_flow_does_its_steps_in_order_with_a_pause_after_each() {
        let f = line(vec![
            NodeKind::OpenApp(" Notes ".into()),
            NodeKind::Wait(500),
            NodeKind::Key(Shortcut::new(45, 8)),
            NodeKind::OpenUrl("https://example.com".into()),
            NodeKind::Command("true".into()),
            NodeKind::OpenApp("   ".into()),
        ]);
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(
            h.log,
            [
                "app Notes",
                "key 45 8",
                "url https://example.com",
                "cmd true"
            ]
        );
        assert_eq!(h.slept, 6 * GAP_MS + 500);
    }

    #[test]
    fn set_fills_in_variables_and_later_blocks_see_them() {
        let f = line(vec![
            set("q", "rust lang"),
            set("url", "https://x.org/?q={{q}}"),
            NodeKind::OpenUrl("{{url}}".into()),
        ]);
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(h.log, ["url https://x.org/?q=rust lang"]);
        assert_eq!(out.vars.get("url"), Some("https://x.org/?q=rust lang"));
    }

    #[test]
    fn a_branch_follows_then_or_else() {
        for (value, expected) in [("5", "big"), ("1", "small")] {
            let mut f = line(vec![
                set("n", value),
                NodeKind::Branch(cond("n", CmpOp::Gt, "3")),
                NodeKind::OpenApp("big".into()),
            ]);
            let b = f
                .nodes
                .iter()
                .find(|n| matches!(n.kind, NodeKind::Branch(_)))
                .unwrap()
                .id;
            let small = loose(&mut f, NodeKind::OpenApp("small".into()));
            f.node_mut(small).unwrap().next = Some(2);
            assert!(f.connect(b, Port::Alt, small));
            let mut h = Mock::default();
            go(&f, &mut h);
            assert_eq!(h.log, [format!("app {expected}")]);
        }
    }

    #[test]
    fn an_output_without_a_wire_ends_the_run() {
        let mut f = line(vec![
            NodeKind::Branch(cond("", CmpOp::IsTrue, "")),
            NodeKind::OpenApp("never".into()),
        ]);
        let b = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Branch(_)))
            .unwrap()
            .id;
        f.unplug_out(b, Port::Alt);
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert!(h.log.is_empty());
    }

    #[test]
    fn repeat_runs_its_body_that_many_times_and_counts_the_rounds() {
        // Repeat(3): body = Key, then back to the Repeat; done = Open app
        let mut f = Flow::new("f", "F");
        let r = f.add_after(None, NodeKind::Repeat(3));
        let k = f.add_after(Some(r), NodeKind::Key(Shortcut::new(8, 2)));
        let log = f.add_after(
            Some(k),
            NodeKind::Native {
                op: NativeOp::AppendFile,
                a: "log".into(),
                b: "{{index}},".into(),
            },
        );
        assert!(f.connect(log, Port::Next, r));
        let after = loose(&mut f, NodeKind::OpenApp("done".into()));
        f.node_mut(after).unwrap().next = Some(2);
        assert!(f.connect(r, Port::Alt, after));
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(h.log, ["key 8 2", "key 8 2", "key 8 2", "app done"]);
        assert_eq!(h.files.get("log").map(String::as_str), Some("1,2,3,"));
    }

    #[test]
    fn a_nested_loop_starts_again_each_time_it_is_entered() {
        // outer x2 { inner x2 { Key } }
        let mut f = Flow::new("f", "F");
        let outer = f.add_after(None, NodeKind::Repeat(2));
        let inner = f.add_after(Some(outer), NodeKind::Repeat(2));
        let k = f.add_after(Some(inner), NodeKind::Key(Shortcut::new(1, 0)));
        assert!(f.connect(k, Port::Next, inner));
        assert!(f.connect(inner, Port::Alt, outer));
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(h.log.len(), 4, "2 x 2 rounds");
    }

    #[test]
    fn while_loops_until_its_condition_fails() {
        // i = 0; while i < 4 { i = i + 2 }
        let mut f = line(vec![set("i", "0")]);
        let w = f.add_after(None, NodeKind::While(cond("i", CmpOp::Lt, "4")));
        let c = f.add_after(
            Some(w),
            NodeKind::Calc {
                out: "i".into(),
                a: "i".into(),
                op: CalcOp::Add,
                b: "2".into(),
            },
        );
        assert!(f.connect(c, Port::Next, w));
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(out.vars.get("i"), Some("4"));
        assert_eq!(out.vars.get("index"), Some("2"));
    }

    #[test]
    fn a_loop_that_never_ends_is_stopped_by_the_limits() {
        let mut f = Flow::new("f", "F");
        let w = f.add_after(None, NodeKind::While(cond("1", CmpOp::IsTrue, "")));
        let k = f.add_after(Some(w), NodeKind::Wait(1));
        assert!(f.connect(k, Port::Next, w));
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(
            out.ending,
            Ending::Finished,
            "the loop gives up after MAX_REPEAT rounds"
        );
        assert_eq!(out.vars.get("index"), Some(MAX_REPEAT.to_string().as_str()));
        // two blocks pointing at each other with no loop block in between: only the step limit helps
        let mut g = Flow::new("g", "G");
        let a = g.add_after(None, NodeKind::Wait(0));
        let b = g.add_after(Some(a), NodeKind::Wait(0));
        assert!(g.connect(b, Port::Next, a));
        let mut h = Mock::default();
        let out = go(&g, &mut h);
        assert_eq!(out.ending, Ending::OutOfSteps);
    }

    #[test]
    fn calc_and_logic_put_their_result_in_a_variable() {
        let f = line(vec![
            NodeKind::Calc {
                out: "n".into(),
                a: "6".into(),
                op: CalcOp::Div,
                b: "0".into(),
            },
            NodeKind::Set {
                name: "failed".into(),
                value: "{{ok}}".into(),
            },
            NodeKind::Logic {
                out: "g".into(),
                a: "1".into(),
                op: LogicOp::Nand,
                b: "failed".into(),
            },
            NodeKind::Logic {
                out: "h".into(),
                a: "g".into(),
                op: LogicOp::Not,
                b: String::new(),
            },
        ]);
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.vars.get("n"), Some(""));
        assert_eq!(out.vars.get("failed"), Some("0"));
        assert_eq!(out.vars.get("g"), Some("1"));
        assert_eq!(out.vars.get("h"), Some("0"));
    }

    #[test]
    fn ask_puts_the_answer_in_a_variable_and_says_whether_there_was_one() {
        let f = line(vec![
            NodeKind::Ask {
                mode: AskMode::Text,
                message: "Name?".into(),
                preset: "Bob".into(),
                out: "name".into(),
            },
            NodeKind::Set {
                name: "answered".into(),
                value: "{{ok}}".into(),
            },
            NodeKind::Ask {
                mode: AskMode::Text,
                message: "Again, {{name}}?".into(),
                preset: String::new(),
                out: "second".into(),
            },
            NodeKind::Ask {
                mode: AskMode::YesNo,
                message: "Sure?".into(),
                preset: String::new(),
                out: "sure".into(),
            },
        ]);
        let mut h = Mock {
            texts: vec![Some("Ann".into()), None],
            yes: vec![Some(true)],
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(
            h.log,
            ["ask Name? / Bob", "ask Again, Ann? / ", "yesno Sure?"]
        );
        assert_eq!(out.vars.get("name"), Some("Ann"));
        assert_eq!(out.vars.get("answered"), Some("1"));
        assert_eq!(out.vars.get("second"), Some(""), "cancelled");
        assert_eq!(out.vars.get("sure"), Some("1"));
        assert_eq!(
            out.vars.get("ok"),
            Some("1"),
            "the last question was answered"
        );
    }

    #[test]
    fn run_waits_for_the_command_and_keeps_its_output_and_exit_code() {
        let f = line(vec![
            set("who", "Ann Lee"),
            NodeKind::Run {
                command: "greet {{who|sh}}".into(),
                out: "o".into(),
                timeout_s: 7,
            },
        ]);
        let mut h = Mock {
            exec: Some(ExecResult {
                code: Some(3),
                stdout: "hello\r\n\n".into(),
            }),
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(h.log, ["exec greet 'Ann Lee' [CURSORPIE_WHO=Ann Lee] 7s"]);
        assert_eq!(out.vars.get("o"), Some("hello"));
        assert_eq!(out.vars.get("code"), Some("3"));
        assert_eq!(out.vars.get("ok"), Some("0"));
        // a command that cannot be started
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.vars.get("code"), Some("-1"));
        assert_eq!(out.vars.get("o"), Some(""));
    }

    #[test]
    fn a_called_flow_shares_the_variables_and_stop_ends_everything() {
        let mut sub = line(vec![set("x", "from sub"), NodeKind::Stop]);
        sub.id = "sub".into();
        let f = line(vec![
            NodeKind::Call("sub".into()),
            NodeKind::OpenApp("never".into()),
        ]);
        let mut h = Mock {
            flows: vec![sub],
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Stopped);
        assert_eq!(out.vars.get("x"), Some("from sub"));
        assert!(h.log.is_empty());
    }

    #[test]
    fn a_flow_that_calls_itself_stops_at_the_depth_limit() {
        let mut f = line(vec![
            NodeKind::Call("f".into()),
            NodeKind::Native {
                op: NativeOp::AppendFile,
                a: "depth".into(),
                b: "x".into(),
            },
        ]);
        f.id = "f".into();
        let mut h = Mock {
            flows: vec![f.clone()],
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(
            h.files.get("depth").map(String::len),
            Some(usize::from(MAX_CALL_DEPTH) + 1),
            "once per level, the top one included"
        );
        // a call to a flow that does not exist is skipped
        let g = line(vec![
            NodeKind::Call("nope".into()),
            NodeKind::OpenApp("a".into()),
        ]);
        let mut h = Mock::default();
        go(&g, &mut h);
        assert_eq!(h.log, ["app a"]);
    }

    #[test]
    fn native_blocks_read_and_write_through_the_host() {
        let native = |op, a: &str, b: &str| NodeKind::Native {
            op,
            a: a.into(),
            b: b.into(),
        };
        let f = line(vec![
            native(NativeOp::WriteFile, "/t", "hi"),
            native(NativeOp::FileExists, "/t", "e1"),
            native(NativeOp::FileExists, "/nope", "e2"),
            native(NativeOp::ReadFile, "/t", "content"),
            native(NativeOp::ReadFile, "/nope", "missing"),
            native(NativeOp::Env, "HOME", "home"),
            native(NativeOp::FrontApp, "", "front"),
            native(NativeOp::AppRunning, "running.app", "up"),
            native(NativeOp::CursorPos, "px", "py"),
            native(NativeOp::Random, "10", "r"),
            native(NativeOp::UnixTime, "", "t"),
            native(NativeOp::ClipboardSet, "{{content}}!", ""),
            native(NativeOp::ClipboardGet, "", "clip"),
            native(NativeOp::Notify, "T", "{{content}}"),
            native(NativeOp::ActivateApp, "com.x", ""),
            native(NativeOp::Reveal, "/t", ""),
        ]);
        let mut h = Mock {
            front: Some("com.front".into()),
            env: BTreeMap::from([("HOME".to_string(), "/home/u".to_string())]),
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        let v = |n: &str| out.vars.get(n).map(str::to_string);
        assert_eq!(v("e1").as_deref(), Some("1"));
        assert_eq!(v("e2").as_deref(), Some("0"));
        assert_eq!(v("content").as_deref(), Some("hi"));
        assert_eq!(v("missing").as_deref(), Some(""));
        assert_eq!(v("home").as_deref(), Some("/home/u"));
        assert_eq!(v("front").as_deref(), Some("com.front"));
        assert_eq!(v("up").as_deref(), Some("1"));
        assert_eq!(
            (v("px").as_deref(), v("py").as_deref()),
            (Some("10"), Some("21"))
        );
        assert_eq!(v("r").as_deref(), Some("9"));
        assert_eq!(v("t").as_deref(), Some("1700000000"));
        assert_eq!(v("clip").as_deref(), Some("hi!"));
        assert_eq!(
            h.log,
            ["clip hi!", "notify T|hi", "activate com.x", "reveal /t"]
        );
    }

    #[test]
    fn type_text_pastes_and_puts_the_clipboard_back() {
        let f = line(vec![NodeKind::Native {
            op: NativeOp::TypeText,
            a: "hello".into(),
            b: String::new(),
        }]);
        let mut h = Mock {
            clip: Some("mine".into()),
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        // Linux: the paste shortcut is Ctrl+V
        assert_eq!(h.log, ["clip hello", "key 9 2", "clip mine"]);
        assert_eq!(h.clip.as_deref(), Some("mine"));
        assert_eq!(out.vars.get("ok"), Some("1"));
        let mut h = Mock::default();
        let f = line(vec![NodeKind::Native {
            op: NativeOp::TypeText,
            a: "x".into(),
            b: String::new(),
        }]);
        run(&mut h, &f, Os::Mac);
        assert_eq!(h.log, ["clip x", "key 9 8"], "Cmd+V on a Mac");
    }

    #[test]
    fn a_block_cut_to_its_limit_still_runs() {
        let f = line(vec![NodeKind::Set {
            name: "x".into(),
            value: "a".repeat(MAX_FIELD_CHARS),
        }]);
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.vars.get("x").map(str::len), Some(MAX_FIELD_CHARS));
    }

    #[test]
    fn the_documented_example_asks_decides_and_loops() {
        let f = crate::config::flow::parse_flow(include_str!(
            "../docs/examples/flows/ask-and-loop.cursorpie-flow.json"
        ))
        .unwrap();
        let mut h = Mock {
            texts: vec![Some("2".into())],
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(
            h.log,
            [
                "ask How many times? / 3",
                "notify Hello|Round 1 of 2",
                "notify Hello|Round 2 of 2",
                "notify Done|",
            ]
        );
        // cancelled: the branch leads straight to End
        let mut h = Mock::default();
        go(&f, &mut h);
        assert_eq!(h.log, ["ask How many times? / 3"]);
    }

    #[test]
    fn a_flow_without_start_wiring_does_nothing() {
        let mut f = Flow::new("f", "F");
        f.add_after(None, NodeKind::OpenApp("x".into()));
        f.unplug_out(1, Port::Next);
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert!(h.log.is_empty());
    }
}
