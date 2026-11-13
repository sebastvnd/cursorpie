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
//! ## Failures
//!
//! A block that cannot do its work never stops the run: it sets `ok` to 0 and the flow goes on, so a
//! flow can decide what to do (an *If / else* on `ok` or `code`). Nothing was ever *said* about it,
//! though, so a flow that did not do its job looked as if it had. Now every such block also leaves
//! a [`Failure`] in the [`Outcome`]: which block, what kind of thing went wrong ([`FailKind`]) and a
//! sentence with the reason. A failure that a later *If / else* or *While* looked at (it read `ok`
//! or `code`) counts as **handled**: the flow dealt with it, and nothing is reported.
//! [`Outcome::problems`] is what is left; [`actions`](crate::actions) turns it into the desktop
//! notification and the sheet of the Settings window. The rules are in
//! [`guides::flow_checks`](crate::guides::flow_checks).
//!
//! ## Why a [`Host`]
//!
//! The interpreter never touches the system: it asks a [`Host`] to press a key, ask a question or
//! read a file. [`actions`](crate::actions) has the real one, built from [`platform`](crate::platform);
//! the tests below have a recording one. That keeps every decision (loops, gates, limits) testable
//! on any OS.

use crate::config::flow::is_var_name;
use crate::config::{
    AskMode, CalcOp, CmpOp, Cond, DEFAULT_PROGRESS_PANEL, Flow, FlowNode, ListOp, LogicOp,
    MAX_CALL_DEPTH, MAX_FLOW_RUN_SECS, MAX_FLOW_STEPS, MAX_REPEAT, MAX_RUN_SECS, MAX_VALUE_CHARS,
    MAX_VARS, MAX_WAIT_MS, NativeOp, NodeKind, PanelOp,
};
use crate::keys::Shortcut;
use crate::platform::Os;
use std::collections::{BTreeMap, BTreeSet};
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
/// How many distinct failures a run keeps; a loop that fails every round says it once, with a count.
pub const MAX_FAILURES: usize = 100;

/// What the browser answered to the command of a *Browser command* block.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrowserReply {
    /// Whether it worked.
    pub ok: bool,
    /// Its words: the answer to a question (`tab.url`, `media.state`), or what went wrong.
    pub message: String,
}

/// What a finished *Run* block's command left behind.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecResult {
    /// The exit code; `None` when the program was ended by a signal or by the timeout.
    pub code: Option<i32>,
    /// What it printed (the start of it, when it printed a lot).
    pub stdout: String,
}

/// How far a run has come, told to the host at the start of every block (see [`Host::progress`]).
///
/// A run is a walk over wires, so how long it still takes is not known: a loop can go round a
/// thousand times and a *If / else* skips half the blocks. What is known is told as it is:
/// `total` only for a flow that goes straight through, `round` only while a loop with a known
/// number of rounds is running.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// Blocks carried out so far in this run, called flows included, with the one that starts now.
    pub step: u32,
    /// How many blocks the flow has when no loop or call can make it longer (*Start*, *End* and
    /// *Stop* not counted); `0` when that is not known. An *If / else* may skip some of them, so a
    /// run can end below it.
    pub total: u32,
    /// The round of the loop the run is in, and how many rounds it makes (*Repeat* and *For each*);
    /// `None` outside a loop and in a *While*, which does not know.
    pub round: Option<(u32, u32)>,
}

/// What an *Update panel* block wants the bar of the panel to show.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanelValue {
    /// Leave the bar as it is.
    Keep,
    /// How full it is, 0 to 1.
    Fraction(f64),
    /// A bar that moves, without a number.
    Busy,
}

/// An *Update panel* block, with its texts filled in, as it is handed to the [`Host`].
#[derive(Clone, Debug, PartialEq)]
pub struct PanelCommand {
    pub op: PanelOp,
    /// The id of the panel; never empty.
    pub panel: String,
    /// `None` leaves the title as it is.
    pub title: Option<String>,
    /// `None` leaves the label as it is.
    pub label: Option<String>,
    pub value: PanelValue,
    /// [`PanelOp::Open`] only: the panel has a Stop button.
    pub stop: bool,
}

/// What the *value* field of an *Update panel* block says: empty keeps the bar, `busy` makes it
/// move, `40%` is 0.4 and a plain number is a fraction from 0 to 1 (more is cut to 1, less to 0).
/// `None` when it is none of these.
pub fn parse_panel_value(text: &str) -> Option<PanelValue> {
    let t = text.trim();
    if t.is_empty() {
        return Some(PanelValue::Keep);
    }
    if t.eq_ignore_ascii_case("busy") {
        return Some(PanelValue::Busy);
    }
    let (number, scale) = match t.strip_suffix('%') {
        Some(n) => (n.trim(), 100.0),
        None => (t, 1.0),
    };
    let n: f64 = number.replace(',', ".").parse().ok()?;
    n.is_finite()
        .then(|| PanelValue::Fraction((n / scale).clamp(0.0, 1.0)))
}

/// How an *Open app (smart)* or *Open link (smart)* block opens what it is told to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpenMode {
    /// Go to it when it is already open, start it when it is not.
    #[default]
    Smart,
    /// Only go to it when it is already open; nothing is started (the block's `ok` says which).
    Existing,
    /// A new window of the app, or the link in a new browser window.
    New,
    /// As the plain *Open app* and *Open URL* blocks do.
    Plain,
}

impl OpenMode {
    /// Reads the word of the second field of the block; empty means [`OpenMode::Smart`]. `None`
    /// for a word that is none of them.
    pub fn parse(word: &str) -> Option<OpenMode> {
        match word.trim().to_lowercase().as_str() {
            "" | "smart" | "auto" => Some(OpenMode::Smart),
            "existing" | "open" | "focus" | "reuse" => Some(OpenMode::Existing),
            "new" | "new window" | "new_window" => Some(OpenMode::New),
            "plain" | "launch" | "normal" => Some(OpenMode::Plain),
            _ => None,
        }
    }
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

    // What follows has a default (it cannot do it), so a host only says what it can.

    /// Opens an app the way `mode` says; whether it was opened or brought forward. The default
    /// does what *Open app* does, whatever the mode.
    fn open_app_as(&mut self, app: &str, _mode: OpenMode) -> bool {
        self.open_app(app);
        true
    }
    /// Opens a link or file the way `mode` says; whether it was opened or brought forward.
    fn open_url_as(&mut self, target: &str, _mode: OpenMode) -> bool {
        self.open_url(target);
        true
    }
    /// The rest of the run is quiet (or not): no notices of what it does. The default has none.
    fn set_quiet(&mut self, _quiet: bool) {}
    /// A block is about to be carried out: how far the run has come. A host that shows the person
    /// what a run is doing (the notice on the screen) keeps it; the default does nothing.
    fn progress(&mut self, _progress: Progress) {}
    /// Pauses the run for a *Wait* block (and only for that: the short pauses between blocks use
    /// [`sleep`](Host::sleep)). The default is a plain pause; a host may also show how long it
    /// takes.
    fn wait(&mut self, ms: u64) {
        self.sleep(ms);
    }
    /// Whether the person asked to stop this run (the Stop button of the progress panel). Asked at
    /// the start of every block; `true` ends the run with [`Ending::Cancelled`]. The default never
    /// stops.
    fn cancelled(&mut self) -> bool {
        false
    }
    /// Carries out an *Update panel* block: opens, changes or closes a progress panel, without
    /// waiting for it. `Err` says why that was not possible. The default cannot show panels.
    fn panel_command(&mut self, _command: &PanelCommand) -> Result<(), String> {
        Err("this host cannot show panels".to_string())
    }

    /// Shows a panel ([`crate::panel`]) and waits for the answer. `None` when this host cannot show
    /// panels; the answer says itself when the panel was cancelled, timed out or could not be shown.
    fn ask_panel(&mut self, _spec: &crate::panel::PanelSpec) -> Option<crate::panel::PanelResult> {
        None
    }

    /// The names of what is in a folder, sorted, without the path.
    fn list_dir(&mut self, _path: &str) -> Option<Vec<String>> {
        None
    }
    /// Makes a folder and the ones above it that are missing.
    fn make_dir(&mut self, _path: &str) -> bool {
        false
    }
    /// Copies a file, replacing what is at the target.
    fn copy_file(&mut self, _from: &str, _to: &str) -> bool {
        false
    }
    /// Moves or renames a file or folder.
    fn move_file(&mut self, _from: &str, _to: &str) -> bool {
        false
    }
    /// Deletes a file (never a folder).
    fn delete_file(&mut self, _path: &str) -> bool {
        false
    }
    /// The size in bytes and the time of the last change (Unix seconds) of a file or folder.
    fn file_info(&mut self, _path: &str) -> Option<(u64, u64)> {
        None
    }
    /// A fact about this computer: `os`, `arch`, `user`, `host`, `home` or `temp`.
    fn system_info(&mut self, _key: &str) -> Option<String> {
        None
    }
    /// Speaks a text aloud.
    fn say(&mut self, _text: &str) -> bool {
        false
    }
    /// Saves a picture of the screen as a PNG file.
    fn screenshot(&mut self, _path: &str) -> bool {
        false
    }
    /// The volume of the speakers, 0 to 100.
    fn volume(&mut self) -> Option<u8> {
        None
    }
    /// Sets the volume of the speakers, 0 to 100.
    fn set_volume(&mut self, _percent: u8) -> bool {
        false
    }
    /// Sends the command `line` (`media.next`, `tab.find docs`) to the browser extension and waits
    /// for it to say how it went. The default cannot reach a browser.
    fn browser(&mut self, _line: &str) -> BrowserReply {
        BrowserReply {
            ok: false,
            message: "this host cannot reach a browser".into(),
        }
    }
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
    /// The person pressed Stop in the progress panel ([`Host::cancelled`]). Not a failure: nothing
    /// is reported about it.
    Cancelled,
}

/// What kind of thing went wrong in a block, for the words of the report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailKind {
    /// The command of a *Run* block ended with an error code, was stopped, or did not start.
    Command,
    /// A file could not be read or written.
    File,
    /// The clipboard, or typing through it.
    Clipboard,
    /// A request to the system that was not answered (notification, front app, pointer, variable).
    System,
    /// A *Calculate* block got something it cannot calculate.
    Calculation,
    /// A *Call flow* block could not call its flow.
    Call,
    /// A *List* block or a loop over a list could not do what it was asked.
    List,
    /// A limit stopped the run.
    Limit,
}

impl FailKind {
    /// A short title for the kind, for a notification and the head of a line of the report.
    pub const fn title(self) -> &'static str {
        match self {
            FailKind::Command => "A command failed",
            FailKind::File => "A file could not be used",
            FailKind::Clipboard => "The clipboard did not work",
            FailKind::System => "The system did not answer",
            FailKind::Calculation => "A calculation failed",
            FailKind::Call => "A called flow could not run",
            FailKind::List => "A list could not be used",
            FailKind::Limit => "The run was stopped",
        }
    }
}

/// One block that could not do its work in a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The name of the flow the block is in (a called flow has its own).
    pub flow: String,
    /// The id of the block.
    pub block: u32,
    /// The title the editor gives the block.
    pub block_name: &'static str,
    pub kind: FailKind,
    /// A sentence: what went wrong, and why when that is known.
    pub detail: String,
    /// How many times in a row it happened (a loop repeats it).
    pub times: u32,
    /// A later *If / else* or *While* read `ok` or `code`: the flow dealt with it.
    pub handled: bool,
}

/// What a run leaves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub ending: Ending,
    pub vars: Vars,
    /// Every block that could not do its work, in the order it happened (the first
    /// [`MAX_FAILURES`] distinct ones).
    pub failures: Vec<Failure>,
}

impl Outcome {
    /// The failures the flow did not deal with: what the person should hear about.
    pub fn problems(&self) -> Vec<&Failure> {
        self.failures.iter().filter(|f| !f.handled).collect()
    }

    /// What to tell the person about this run of the flow `flow_id` / `flow_name`; `None` when
    /// nothing is left to say.
    pub fn report(&self, flow_id: &str, flow_name: &str) -> Option<Report> {
        let problems = self.problems();
        let first = problems.first()?;
        let sentence = |f: &Failure| {
            let place = if f.flow == flow_name {
                format!("Block {} ({})", f.block, f.block_name)
            } else {
                format!(
                    "Block {} ({}) of the flow \"{}\"",
                    f.block, f.block_name, f.flow
                )
            };
            let times = if f.times > 1 {
                format!(" ({} times)", f.times)
            } else {
                String::new()
            };
            format!("{place}: {}{times}.", f.detail)
        };
        let lines: Vec<String> = problems.iter().map(|f| sentence(f)).collect();
        let same_kind = problems.iter().all(|f| f.kind == first.kind);
        let title = if same_kind && problems.len() == 1 {
            first.kind.title().to_string()
        } else if same_kind {
            format!("{} ({} blocks)", first.kind.title(), problems.len())
        } else {
            format!("{} problems", problems.len())
        };
        let mut summary = lines[0].clone();
        if lines.len() > 1 {
            summary.push_str(&format!(" (and {} more)", lines.len() - 1));
        }
        let marks = problems
            .iter()
            .filter(|f| f.flow == flow_name)
            .map(|f| (f.block, f.detail.clone()))
            .collect();
        Some(Report {
            flow_id: flow_id.to_string(),
            flow: flow_name.to_string(),
            title,
            summary,
            lines,
            marks,
        })
    }
}

/// What the person is told about a run that did not go well (see [`Outcome::report`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The id of the flow that was run.
    pub flow_id: String,
    /// Its name.
    pub flow: String,
    /// A short title: the kind of the problem when there is one, else how many.
    pub title: String,
    /// One line for a notification: the first problem, and how many more there are.
    pub summary: String,
    /// One full sentence per problem, in the order they happened.
    pub lines: Vec<String>,
    /// The blocks of the flow itself that had a problem, with the reason: the editor marks them.
    pub marks: Vec<(u32, String)>,
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
        failures: Vec::new(),
        last_failure: None,
        cur: (0, ""),
        flow_name: String::new(),
        quiet: false,
        total: straight_total(flow),
        round: None,
    };
    let ending = match runner.run_flow(flow, 0) {
        Ok(()) => Ending::Finished,
        Err(e) => e,
    };
    Outcome {
        ending,
        vars: runner.vars,
        failures: runner.failures,
    }
}

/// How many blocks a run of `flow` carries out when it goes straight through (see
/// [`Progress::total`]): `0` when a loop or a call can make it longer.
fn straight_total(flow: &Flow) -> u32 {
    let mut blocks = 0u32;
    for node in &flow.nodes {
        match &node.kind {
            NodeKind::Start | NodeKind::End | NodeKind::Stop => {}
            NodeKind::Repeat(_)
            | NodeKind::While(_)
            | NodeKind::ForEach { .. }
            | NodeKind::Call(_) => return 0,
            _ => blocks += 1,
        }
    }
    blocks
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

/// The value for what stands between `{{` and `}}`: a defined variable (or any name, when a
/// `default:` filter can stand in for it) with its filters applied from left to right. `None`
/// (the text stays as written) for a variable that is not defined, a filter that does not exist,
/// or one that cannot work on the value (`count` on something that is not a list).
fn fill(inner: &str, vars: &Vars, os: Os) -> Option<String> {
    let mut parts = inner.split('|');
    let name = parts.next()?.trim();
    let filters: Vec<&str> = parts.collect();
    let mut value = match vars.get(name) {
        Some(v) => v.to_string(),
        None if is_var_name(name)
            && filters
                .iter()
                .any(|f| f.trim_start().starts_with("default:")) =>
        {
            String::new()
        }
        None => return None,
    };
    for f in filters {
        value = apply_filter(f, &value, os)?;
    }
    Some(value)
}

/// The filters that take no argument, as written after a `|`.
pub const PLAIN_FILTERS: [&str; 21] = [
    "sh", "trim", "upper", "lower", "title", "len", "reverse", "int", "round", "abs", "num",
    "bool", "json", "url", "basename", "dirname", "ext", "count", "first", "last", "join",
];
/// The filters that need an argument after a `:`.
pub const ARG_FILTERS: [&str; 4] = ["item", "default", "replace", "join"];

/// Whether `spec` (what stands between two `|`, like `trim` or `join:, `) is a filter that exists.
pub fn is_known_filter(spec: &str) -> bool {
    let (name, arg) = split_filter(spec);
    match arg {
        None => PLAIN_FILTERS.contains(&name),
        Some(_) => ARG_FILTERS.contains(&name),
    }
}

/// A filter as its name and its argument: `join:, ` is `("join", Some(", "))`. The name is
/// trimmed, the argument is not (a separator can be a space).
fn split_filter(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once(':') {
        Some((n, a)) => (n.trim(), Some(a)),
        None => (spec.trim(), None),
    }
}

/// `\n` and `\t` in a text typed in a one-line field, as the line break and the tab they stand for.
pub fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\t", "\t")
}

/// The result of the filter `spec` on `value`. See `fill` for when there is none.
///
/// | Filter | Result |
/// |---|---|
/// | `sh` | quoted as one word for the shell |
/// | `trim` `upper` `lower` `title` `reverse` | the text without the spaces around it, in capitals, in small letters, with each word capitalized, backwards |
/// | `len` | how many characters |
/// | `int` `round` `abs` `num` | the number cut, rounded, without its sign, written the plain way (empty when the text is not a number) |
/// | `bool` | `1` or `0`, as a condition would read it |
/// | `json` | the text as a JSON string, quotes included |
/// | `url` | percent-encoded for a URL |
/// | `basename` `dirname` `ext` | the file name, the folder, the extension (no dot) of a path |
/// | `count` `first` `last` | how many items, the first, the last (of a list) |
/// | `join:, ` | the items of a list with this text between them |
/// | `item:2` | the item at that position (from 1; -1 is the last) |
/// | `default:text` | the text when the value is empty |
/// | `replace:a:b` | every `a` replaced by `b` |
pub fn apply_filter(spec: &str, value: &str, os: Os) -> Option<String> {
    let (name, arg) = split_filter(spec);
    Some(match (name, arg) {
        ("sh", None) => quote_for_shell(os, value),
        ("trim", None) => value.trim().to_string(),
        ("upper", None) => value.to_uppercase(),
        ("lower", None) => value.to_lowercase(),
        ("title", None) => title_case(value),
        ("reverse", None) => value.chars().rev().collect(),
        ("len", None) => value.chars().count().to_string(),
        ("int", None) => format_number(parse_number(value)?.trunc()),
        ("round", None) => format_number(parse_number(value)?.round()),
        ("abs", None) => format_number(parse_number(value)?.abs()),
        ("num", None) => format_number(parse_number(value)?),
        ("bool", None) => flag(is_truthy(value)).to_string(),
        ("json", None) => serde_json::to_string(value).ok()?,
        ("url", None) => percent_encode(value),
        ("basename", None) => base_name(value),
        ("dirname", None) => dir_name(value),
        ("ext", None) => extension(value),
        ("count", None) => parse_list(value)?.len().to_string(),
        ("first", None) => parse_list(value)?.first().cloned().unwrap_or_default(),
        ("last", None) => parse_list(value)?.last().cloned().unwrap_or_default(),
        ("join", None) => parse_list(value)?.join(", "),
        ("join", Some(sep)) => parse_list(value)?.join(&unescape(sep)),
        ("item", Some(n)) => {
            let items = parse_list(value)?;
            let i = position(n, items.len())?;
            items.get(i).cloned().unwrap_or_default()
        }
        ("default", Some(d)) => {
            if value.trim().is_empty() {
                unescape(d)
            } else {
                value.to_string()
            }
        }
        ("replace", Some(args)) => {
            let (from, to) = args.split_once(':').unwrap_or((args, ""));
            if from.is_empty() {
                value.to_string()
            } else {
                value.replace(&unescape(from), &unescape(to))
            }
        }
        _ => return None,
    })
}

/// Each word of `s` with a capital first letter and the rest in small letters.
fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut start = true;
    for c in s.chars() {
        if c.is_whitespace() {
            start = true;
            out.push(c);
        } else if start {
            out.extend(c.to_uppercase());
            start = false;
        } else {
            out.extend(c.to_lowercase());
        }
    }
    out
}

/// `s` for the part of a URL after the `?`: everything but letters, digits and `-._~` as `%XX`.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The last part of a path, whichever slash it uses.
fn base_name(path: &str) -> String {
    let t = path.trim_end_matches(['/', '\\']);
    t.rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

/// A path without its last part (empty when it has no folder).
fn dir_name(path: &str) -> String {
    let t = path.trim_end_matches(['/', '\\']);
    match t.rfind(['/', '\\']) {
        Some(0) => "/".to_string(),
        Some(i) => t[..i].to_string(),
        None => String::new(),
    }
}

/// The extension of the last part of a path, without the dot (empty when it has none).
fn extension(path: &str) -> String {
    let name = base_name(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_string(),
        _ => String::new(),
    }
}

// ------------------------------------------------------------------ lists

/// The items of a text that holds a list: a JSON array (`["a","b"]`). Numbers and other values in
/// it become their text. An empty text is the empty list; `None` when the text is anything else.
pub fn parse_list(s: &str) -> Option<Vec<String>> {
    let t = s.trim();
    if t.is_empty() {
        return Some(Vec::new());
    }
    match serde_json::from_str::<serde_json::Value>(t).ok()? {
        serde_json::Value::Array(items) => Some(
            items
                .into_iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null => String::new(),
                    other => other.to_string(),
                })
                .collect(),
        ),
        _ => None,
    }
}

/// A list as the text a variable holds: a JSON array of texts.
pub fn format_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_string())
}

/// Like [`format_list`], but with items taken off the end until it fits in a variable (a list cut
/// in the middle of an item would not be a list any more). The bool says whether it was cut.
fn format_list_capped(items: &[String]) -> (String, bool) {
    let mut keep = items.len();
    loop {
        let text = format_list(&items[..keep]);
        if text.chars().count() <= MAX_VALUE_CHARS {
            return (text, keep < items.len());
        }
        keep -= 1;
    }
}

/// The index (from 0) that the position `s` points at in a list of `len` items: positions count
/// from 1, a negative one from the end. `None` when it is not a number or out of range.
fn position(s: &str, len: usize) -> Option<usize> {
    let n = parse_number(s)?.trunc();
    let len = len as f64;
    if n >= 1.0 && n <= len {
        Some(n as usize - 1)
    } else if n < 0.0 && -n <= len {
        Some((len + n) as usize)
    } else {
        None
    }
}

/// The pieces of `text` between the `sep`s. An empty separator, or a line break, cuts at every line
/// (a final line break does not make an empty piece).
fn split_text(text: &str, sep: &str) -> Vec<String> {
    let sep = unescape(sep);
    if sep.is_empty() || sep == "\n" {
        text.lines().map(str::to_string).collect()
    } else {
        text.split(sep.as_str()).map(str::to_string).collect()
    }
}

/// Sorts the items: as numbers when every one of them is a number, as text otherwise.
fn sort_items(items: &mut [String]) {
    let numbers: Option<Vec<f64>> = items.iter().map(|x| parse_number(x)).collect();
    if numbers.is_some() {
        items.sort_by(|a, b| {
            let (x, y) = (
                parse_number(a).unwrap_or(0.0),
                parse_number(b).unwrap_or(0.0),
            );
            x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
        });
    } else {
        items.sort();
    }
}

/// The start of a text, for a sentence: at most `max` characters, then an ellipsis.
fn brief(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}\u{2026}")
    }
}

/// What kind of failure a *System* block that did not work is, and the words for it. `a` and `b`
/// are its fields with the variables filled in.
fn native_failure(op: NativeOp, a: &str, b: &str) -> (FailKind, String) {
    match op {
        NativeOp::ClipboardSet => (
            FailKind::Clipboard,
            "the text could not be put on the clipboard (another app may be holding it)".into(),
        ),
        NativeOp::ClipboardGet => (
            FailKind::Clipboard,
            "the clipboard could not be read: it is empty or holds something that is not text"
                .into(),
        ),
        NativeOp::TypeText => (
            FailKind::Clipboard,
            "the text could not be put on the clipboard, so nothing was typed".into(),
        ),
        NativeOp::Notify => (
            FailKind::System,
            "the desktop notification could not be shown (notifications may be switched off for \
             CursorPie in the system settings)"
                .into(),
        ),
        NativeOp::ReadFile => (
            FailKind::File,
            format!(
                "the file `{}` could not be read: it does not exist, is not text, is bigger than \
                 256 KB, or CursorPie may not open it",
                brief(a, 60)
            ),
        ),
        NativeOp::WriteFile | NativeOp::AppendFile => (
            FailKind::File,
            format!(
                "the file `{}` could not be written: its folder may not exist, or CursorPie may \
                 not write there",
                brief(a, 60)
            ),
        ),
        NativeOp::Env => (
            FailKind::System,
            format!(
                "the environment variable `{}` is not set, so `{}` is empty",
                brief(a, 40),
                brief(b, 30)
            ),
        ),
        NativeOp::FrontApp => (
            FailKind::System,
            "the app in front could not be found out".into(),
        ),
        NativeOp::CursorPos => (
            FailKind::System,
            "the position of the pointer could not be read".into(),
        ),
        NativeOp::ListDir => (
            FailKind::File,
            format!(
                "the folder `{}` could not be listed: it does not exist, is not a folder, \
                 CursorPie may not open it, or it holds more names than a variable can keep",
                brief(a, 60)
            ),
        ),
        NativeOp::MakeDir => (
            FailKind::File,
            format!(
                "the folder `{}` could not be made: a file is in the way, or CursorPie may not \
                 write there",
                brief(a, 60)
            ),
        ),
        NativeOp::CopyFile => (
            FailKind::File,
            format!(
                "the file `{}` could not be copied to `{}`: it does not exist, is a folder, or \
                 CursorPie may not write there",
                brief(a, 40),
                brief(b, 40)
            ),
        ),
        NativeOp::MoveFile => (
            FailKind::File,
            format!(
                "`{}` could not be moved to `{}`: it does not exist, or CursorPie may not write \
                 there",
                brief(a, 40),
                brief(b, 40)
            ),
        ),
        NativeOp::DeleteFile => (
            FailKind::File,
            format!(
                "the file `{}` could not be deleted: it does not exist, it is a folder (folders \
                 are never deleted), or CursorPie may not change it",
                brief(a, 60)
            ),
        ),
        NativeOp::FileSize | NativeOp::FileTime => (
            FailKind::File,
            format!(
                "nothing is known about `{}`: it does not exist",
                brief(a, 60)
            ),
        ),
        NativeOp::SystemInfo => (
            FailKind::System,
            format!(
                "`{}` is not something this computer can tell: use os, arch, user, host, home \
                 or temp",
                brief(a, 30)
            ),
        ),
        NativeOp::Say => (
            FailKind::System,
            "the text could not be spoken: the text is empty, or the system has no speech program \
             (say on a Mac, spd-say or espeak on Linux, PowerShell on Windows)"
                .into(),
        ),
        NativeOp::Screenshot => (
            FailKind::System,
            format!(
                "the screen could not be saved to `{}`: the path is empty, CursorPie may not \
                 record the screen (allow it in the system settings), or the system has no \
                 screenshot program",
                brief(a, 50)
            ),
        ),
        NativeOp::Browser => (
            FailKind::System,
            format!(
                "the browser did not run `{}`: the extension is not connected or the command is \
                 not one it knows",
                brief(a, 50)
            ),
        ),
        NativeOp::OpenAppSmart | NativeOp::OpenUrlSmart => {
            let what = if op == NativeOp::OpenAppSmart {
                "the app"
            } else {
                "the link"
            };
            match OpenMode::parse(b) {
                None => (
                    FailKind::System,
                    format!(
                        "`{}` is not a way to open {what}: use smart, existing, new or plain",
                        brief(b, 30)
                    ),
                ),
                Some(OpenMode::Existing) => (
                    FailKind::System,
                    format!(
                        "{what} `{}` is not open, and `existing` does not open anything: use \
                         smart to open it when it is not",
                        brief(a, 50)
                    ),
                ),
                Some(_) => (
                    FailKind::System,
                    format!(
                        "{what} `{}` was not opened: the name is empty",
                        brief(a, 50)
                    ),
                ),
            }
        }
        NativeOp::VolumeGet | NativeOp::VolumeSet => (
            FailKind::System,
            "the volume could not be read or set: the number is not from 0 to 100, or the system \
             has no program for it (osascript on a Mac, wpctl, pactl or amixer on Linux)"
                .into(),
        ),
        _ => (
            FailKind::System,
            "the system did not do what it was asked".into(),
        ),
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
    /// What could not be done so far.
    failures: Vec<Failure>,
    /// The failure that `ok` still says (the last block that can fail failed): the next *If /
    /// else* that reads `ok` or `code` handles it.
    last_failure: Option<usize>,
    /// The block being carried out: its id and its title.
    cur: (u32, &'static str),
    /// The name of the flow being run.
    flow_name: String,
    /// A *Hide notices* block switched the notices off for the rest of the run.
    quiet: bool,
    /// What [`straight_total`] said about the flow the run began with.
    total: u32,
    /// The round of the loop the run is in, when it has a known number of rounds.
    round: Option<(u32, u32)>,
}

impl<H: Host> Runner<'_, H> {
    /// Runs one flow to its end. `Err` stops the whole run (a *Stop* block or a limit), also in
    /// the flows that called this one.
    fn run_flow(&mut self, flow: &Flow, depth: u8) -> Result<(), Ending> {
        let outer = std::mem::replace(&mut self.flow_name, flow.name.clone());
        let result = self.walk(flow, depth);
        self.flow_name = outer;
        result
    }

    /// The walk of [`run_flow`](Self::run_flow).
    fn walk(&mut self, flow: &Flow, depth: u8) -> Result<(), Ending> {
        // how many rounds each loop block has made in this run of this flow
        let mut rounds: BTreeMap<u32, u32> = BTreeMap::new();
        let mut cur = flow.start().and_then(|s| s.next);
        while let Some(id) = cur {
            self.tick()?;
            let Some(node) = flow.node(id) else {
                break;
            };
            self.cur = (node.id, crate::config::flow_check::label(&node.kind));
            self.host.progress(Progress {
                step: self.steps,
                total: self.total,
                round: self.round,
            });
            cur = self.step(node, depth, &mut rounds)?;
        }
        Ok(())
    }

    /// Counts one more block against the limits.
    fn tick(&mut self) -> Result<(), Ending> {
        if self.host.cancelled() {
            return Err(Ending::Cancelled);
        }
        self.steps += 1;
        if self.steps > MAX_FLOW_STEPS {
            self.fail(
                FailKind::Limit,
                format!(
                    "the run was stopped after {MAX_FLOW_STEPS} blocks. A loop that never ends is \
                     the usual cause: check the wire that goes back and the condition of the \
                     While. This was the last block that ran"
                ),
            );
            return Err(Ending::OutOfSteps);
        }
        if Instant::now() >= self.deadline {
            self.fail(
                FailKind::Limit,
                format!(
                    "the run was stopped after {MAX_FLOW_RUN_SECS} seconds. A command that never \
                     ends or a loop that waits is the usual cause. This was the last block that ran"
                ),
            );
            return Err(Ending::OutOfTime);
        }
        Ok(())
    }

    /// Writes down that the block being carried out could not do its work.
    fn fail(&mut self, kind: FailKind, detail: String) {
        // the same thing again, in a row (a loop): count it
        if let Some(last) = self.failures.last_mut()
            && last.block == self.cur.0
            && last.kind == kind
            && last.detail == detail
            && last.flow == self.flow_name
        {
            last.times = last.times.saturating_add(1);
            last.handled = false;
            self.last_failure = Some(self.failures.len() - 1);
            return;
        }
        if self.failures.len() >= MAX_FAILURES {
            self.last_failure = None;
            return;
        }
        self.failures.push(Failure {
            flow: self.flow_name.clone(),
            block: self.cur.0,
            block_name: self.cur.1,
            kind,
            detail,
            times: 1,
            handled: false,
        });
        self.last_failure = Some(self.failures.len() - 1);
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

    /// Whether a condition holds. A condition that reads `ok` or `code` has looked at what the
    /// last block that can fail did: that failure is handled.
    fn holds(&mut self, c: &Cond) -> bool {
        let reads_status = [&c.a, &c.b].iter().any(|s| {
            let t = s.trim();
            t == "ok" || t == "code" || s.contains("{{ok") || s.contains("{{code")
        });
        if reads_status
            && let Some(i) = self.last_failure.take()
            && let Some(f) = self.failures.get_mut(i)
        {
            f.handled = true;
        }
        compare(c.op, &self.operand(&c.a), &self.operand(&c.b))
    }

    /// Sets a variable; a bad name is ignored.
    fn put(&mut self, name: &str, value: impl Into<String>) {
        let _ = self.vars.set(name.trim(), value);
    }

    /// Records whether the last block that can fail worked.
    fn ok(&mut self, ok: bool) {
        if ok {
            self.last_failure = None;
        }
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
                self.host.wait(u64::from((*ms).min(MAX_WAIT_MS)));
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
                        let why = if parse_number(&x).is_none() {
                            format!("`{}` is not a number", brief(&x, 24))
                        } else if parse_number(&y).is_none() {
                            format!("`{}` is not a number", brief(&y, 24))
                        } else {
                            "it divides by zero, or the result is too large".to_string()
                        };
                        self.fail(
                            FailKind::Calculation,
                            format!("{why}, so the result is empty"),
                        );
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
                    return Ok(self.enter_loop(node, rounds, round, (*n).min(MAX_REPEAT)));
                }
                rounds.remove(&node.id);
                self.round = None;
                return Ok(node.alt);
            }
            NodeKind::While(c) => {
                let round = rounds.get(&node.id).copied().unwrap_or(0);
                if round < MAX_REPEAT && self.holds(c) {
                    return Ok(self.enter_loop(node, rounds, round, 0));
                }
                rounds.remove(&node.id);
                self.round = None;
                return Ok(node.alt);
            }
            NodeKind::ForEach { list, item } => {
                let round = rounds.get(&node.id).copied().unwrap_or(0);
                let current = self.vars.get(list.trim()).unwrap_or("").to_string();
                let items = match parse_list(&current) {
                    Some(items) => items,
                    None => {
                        if round == 0 {
                            self.ok(false);
                            self.fail(
                                FailKind::List,
                                format!(
                                    "the variable `{}` does not hold a list (it holds `{}`), so \
                                     the loop did not run",
                                    brief(list, 32),
                                    brief(&current, 24)
                                ),
                            );
                        }
                        Vec::new()
                    }
                };
                let count = u32::try_from(items.len())
                    .unwrap_or(u32::MAX)
                    .min(MAX_REPEAT);
                if round < MAX_REPEAT
                    && let Some(x) = items.get(round as usize)
                {
                    self.put(item, x.clone());
                    return Ok(self.enter_loop(node, rounds, round, count));
                }
                rounds.remove(&node.id);
                self.round = None;
                return Ok(node.alt);
            }
            NodeKind::List {
                op,
                list,
                a,
                b,
                out,
            } => {
                self.list_block(*op, list, a, b, out);
                self.gap();
            }
            NodeKind::Ask {
                mode,
                message,
                preset,
                out,
            } => self.ask(*mode, message, preset, out),
            NodeKind::PanelUpdate {
                op,
                panel,
                title,
                label,
                value,
                stop,
            } => self.update_panel(*op, panel, title, label, value, *stop),
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
                if depth >= MAX_CALL_DEPTH {
                    self.fail(
                        FailKind::Call,
                        format!(
                            "the calls are {MAX_CALL_DEPTH} flows deep, which is the limit: a flow \
                             probably calls itself, so this call was skipped"
                        ),
                    );
                } else if let Some(sub) = self.host.flow(id.trim()) {
                    self.run_flow(&sub, depth + 1)?;
                } else {
                    self.fail(
                        FailKind::Call,
                        format!(
                            "there is no flow `{}`, so nothing was called (it may have been \
                             deleted)",
                            brief(id.trim(), 40)
                        ),
                    );
                }
            }
        }
        Ok(node.next)
    }

    /// Starts round `round + 1` of a loop block: counts it, sets `index` and leads into the body.
    /// `total` is how many rounds the loop makes, `0` when that is not known (see
    /// [`Progress::round`]).
    fn enter_loop(
        &mut self,
        node: &FlowNode,
        rounds: &mut BTreeMap<u32, u32>,
        round: u32,
        total: u32,
    ) -> Option<u32> {
        rounds.insert(node.id, round + 1);
        self.put("index", (round + 1).to_string());
        self.round = (total > 0).then_some((round + 1, total));
        node.next
    }

    /// A *List* block: `op` on the list in the variable `list`, with the operands `a` and `b`
    /// (variables filled in), and the variable `out` for a result.
    fn list_block(&mut self, op: ListOp, list: &str, a: &str, b: &str, out: &str) {
        let name = list.trim().to_string();
        let a = self.text(a);
        let b = self.text(b);
        let current = self.vars.get(&name).unwrap_or("").to_string();
        // making a list needs none to start with; every other operation needs one
        let mut items = if op == ListOp::Split {
            Vec::new()
        } else {
            match parse_list(&current) {
                Some(items) => items,
                None => {
                    self.ok(false);
                    self.fail(
                        FailKind::List,
                        format!(
                            "the variable `{}` does not hold a list (it holds `{}`), so nothing \
                             was done. Make the list first with Make from text",
                            brief(&name, 32),
                            brief(&current, 24)
                        ),
                    );
                    return;
                }
            }
        };
        let count = items.len();
        let mut result: Option<String> = None;
        let mut changed = false;
        let mut why: Option<String> = None; // set when the operation could not be done
        let no_item = |a: &str| {
            format!(
                "there is no item at position `{}` (the list has {count} item{})",
                brief(a, 16),
                if count == 1 { "" } else { "s" }
            )
        };
        match op {
            ListOp::Split => {
                items = split_text(&a, &b);
                changed = true;
            }
            ListOp::Push => {
                items.push(a.clone());
                changed = true;
            }
            ListOp::Pop => match items.pop() {
                Some(last) => {
                    result = Some(last);
                    changed = true;
                }
                None => why = Some("the list is empty, so there is nothing to take".to_string()),
            },
            ListOp::Get => match position(&a, count) {
                Some(i) => result = Some(items[i].clone()),
                None => why = Some(no_item(&a)),
            },
            ListOp::SetItem => match position(&a, count) {
                Some(i) => {
                    items[i] = b.clone();
                    changed = true;
                }
                None => why = Some(no_item(&a)),
            },
            ListOp::Remove => match position(&a, count) {
                Some(i) => {
                    items.remove(i);
                    changed = true;
                }
                None => why = Some(no_item(&a)),
            },
            ListOp::Length => result = Some(count.to_string()),
            ListOp::Join => result = Some(items.join(&unescape(&a))),
            ListOp::IndexOf => {
                let at = items.iter().position(|x| *x == a).map_or(0, |i| i + 1);
                result = Some(at.to_string());
            }
            ListOp::Sort => {
                sort_items(&mut items);
                changed = true;
            }
            ListOp::Reverse => {
                items.reverse();
                changed = true;
            }
            ListOp::Unique => {
                let mut seen: BTreeSet<String> = BTreeSet::new();
                items.retain(|x| seen.insert(x.clone()));
                changed = true;
            }
            ListOp::Shuffle => {
                for i in (1..items.len()).rev() {
                    let j = self.host.random_below(i as u64 + 1) as usize;
                    items.swap(i, j.min(i));
                }
                changed = true;
            }
            ListOp::Sum => {
                let numbers: Option<Vec<f64>> = items.iter().map(|x| parse_number(x)).collect();
                match numbers {
                    Some(n) => result = Some(format_number(n.iter().sum())),
                    None => why = Some("an item is not a number, so there is no sum".to_string()),
                }
            }
            ListOp::Clear => {
                items.clear();
                changed = true;
            }
        }
        if let Some(why) = why {
            self.put(out, "");
            self.ok(false);
            self.fail(FailKind::List, why);
            return;
        }
        if changed {
            let (text, cut) = format_list_capped(&items);
            if cut {
                self.ok(false);
                self.fail(
                    FailKind::List,
                    format!(
                        "the list is too long to keep in a variable (at most {MAX_VALUE_CHARS} \
                         characters), so it was left as it was"
                    ),
                );
                return;
            }
            self.put(&name, text);
        }
        if let Some(r) = result {
            self.put(out, r);
        }
        self.ok(true);
    }

    /// An *Ask* block. The time the question stays open does not count against the run.
    fn ask(&mut self, mode: AskMode, message: &str, preset: &str, out: &str) {
        if mode == AskMode::Panel {
            // the JSON is read first and the variables are filled in afterwards, text by text: a
            // value with a quote in it can never add a widget or a button
            self.ask_panel(message, out);
            return;
        }
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
            AskMode::Panel => unreachable!("handled above"),
        };
        self.deadline += asked.elapsed();
        self.put(out, answer);
        self.ok(ok);
    }

    /// An *Ask* block in panel mode: shows the panel that `spec_json` describes. When a button that
    /// confirms is pressed, every field's value goes in the variable named by its `id`, `out` gets
    /// the id of that button and `ok` is 1; a panel that was cancelled (a cancel button, Esc) or
    /// timed out gives an empty `out` and `ok` 0, which is an answer, not a failure. A description
    /// that is not valid, a host that cannot show panels or panels that are switched off are
    /// failures.
    fn ask_panel(&mut self, spec_json: &str, out: &str) {
        let spec = match crate::panel::PanelSpec::parse(spec_json) {
            Ok(s) => s,
            Err(e) => {
                self.put(out, "");
                self.ok(false);
                self.fail(FailKind::System, e.to_string());
                return;
            }
        };
        let mut spec = spec;
        spec.map_text(&mut |t: &str| self.text(t));
        let spec = match spec.checked() {
            Ok(s) => s,
            Err(e) => {
                self.put(out, "");
                self.ok(false);
                self.fail(FailKind::System, e.to_string());
                return;
            }
        };
        let asked = Instant::now();
        let result = self.host.ask_panel(&spec);
        self.deadline += asked.elapsed();
        match result {
            Some(r) if r.submitted() => {
                for (name, value) in r.flow_pairs() {
                    self.put(&name, value);
                }
                self.put(out, r.button.clone());
                self.ok(true);
            }
            Some(r) => {
                self.put(out, "");
                self.ok(false);
                if r.outcome == crate::panel::Outcome::Unavailable {
                    self.fail(
                        FailKind::System,
                        format!("the panel could not be shown: {}", r.reason),
                    );
                }
            }
            None => {
                self.put(out, "");
                self.ok(false);
                self.fail(FailKind::System, "this host cannot show panels".to_string());
            }
        }
    }

    /// An *Update panel* block: opens, changes or closes the progress panel. A value that is not a
    /// number, a percentage or `busy` leaves the bar as it was and is a failure (`ok` 0); nothing
    /// waits for the panel and nothing is paused after it, so a loop can update it every round.
    fn update_panel(
        &mut self,
        op: PanelOp,
        panel: &str,
        title: &str,
        label: &str,
        value: &str,
        stop: bool,
    ) {
        let id = self.text(panel);
        let id = id.trim();
        let id = if id.is_empty() {
            DEFAULT_PROGRESS_PANEL.to_string()
        } else {
            id.to_string()
        };
        let (title, label, value_text) = (self.text(title), self.text(label), self.text(value));
        let mut good = true;
        let value = match parse_panel_value(&value_text) {
            Some(v) => v,
            None => {
                good = false;
                self.fail(
                    FailKind::System,
                    format!(
                        "the value `{}` is not a number from 0 to 1, a percentage like `40%` or \
                         `busy`, so the bar stayed as it was",
                        brief(value_text.trim(), 30)
                    ),
                );
                PanelValue::Keep
            }
        };
        let command = PanelCommand {
            op,
            panel: id,
            title: (!title.trim().is_empty()).then(|| title.trim().to_string()),
            label: (!label.trim().is_empty()).then(|| label.trim().to_string()),
            value: if op.has_fields() {
                value
            } else {
                PanelValue::Keep
            },
            stop: op == PanelOp::Open && stop,
        };
        match self.host.panel_command(&command) {
            Ok(()) => self.ok(good),
            Err(why) => {
                self.ok(false);
                self.fail(
                    FailKind::System,
                    format!("the panel could not be shown: {why}"),
                );
            }
        }
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
                if code != 0 {
                    let mut detail = match r.code {
                        Some(c) => format!("the command ended with code {c}"),
                        None => format!(
                            "the command was stopped: it did not finish within {} seconds, or \
                             the system ended it",
                            timeout.as_secs()
                        ),
                    };
                    let printed = r.stdout.lines().next().unwrap_or("").trim();
                    if !printed.is_empty() {
                        detail.push_str(&format!(" and printed `{}`", brief(printed, 80)));
                    }
                    self.fail(FailKind::Command, detail);
                }
            }
            None => {
                self.put(out, "");
                self.put("code", "-1");
                self.ok(false);
                self.fail(
                    FailKind::Command,
                    "the command could not be started (the shell was not found, or the system \
                     refused to run it)"
                        .to_string(),
                );
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
        // what the browser said, when it did not work (a *Browser command* block)
        let mut browser_said = String::new();
        let ok = match op {
            NativeOp::OpenAppSmart => {
                let opened = match OpenMode::parse(&tb) {
                    Some(mode) if !ta.trim().is_empty() => self.host.open_app_as(ta.trim(), mode),
                    _ => false,
                };
                self.gap();
                opened
            }
            NativeOp::OpenUrlSmart => {
                let opened = match OpenMode::parse(&tb) {
                    Some(mode) if !ta.trim().is_empty() => self.host.open_url_as(ta.trim(), mode),
                    _ => false,
                };
                self.gap();
                opened
            }
            NativeOp::Quiet => {
                let on = !matches!(
                    ta.trim().to_lowercase().as_str(),
                    "off" | "0" | "no" | "false"
                );
                self.quiet = on;
                self.host.set_quiet(on);
                true
            }
            NativeOp::ClipboardSet => self.host.set_clipboard(&ta),
            NativeOp::ClipboardGet => {
                let got = self.host.clipboard();
                let ok = got.is_some();
                self.put(&tb, got.unwrap_or_default());
                ok
            }
            NativeOp::TypeText => self.type_text(&ta),
            // a quiet run shows no notification: the block still worked
            NativeOp::Notify => self.quiet || self.host.notify(&ta, &tb),
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
            NativeOp::ListDir => match self.host.list_dir(ta.trim()) {
                Some(names) => {
                    let (text, cut) = format_list_capped(&names);
                    self.put(&tb, text);
                    !cut
                }
                None => {
                    self.put(&tb, "[]");
                    false
                }
            },
            NativeOp::MakeDir => !ta.trim().is_empty() && self.host.make_dir(ta.trim()),
            NativeOp::CopyFile => {
                !ta.trim().is_empty()
                    && !tb.trim().is_empty()
                    && self.host.copy_file(ta.trim(), tb.trim())
            }
            NativeOp::MoveFile => {
                !ta.trim().is_empty()
                    && !tb.trim().is_empty()
                    && self.host.move_file(ta.trim(), tb.trim())
            }
            NativeOp::DeleteFile => !ta.trim().is_empty() && self.host.delete_file(ta.trim()),
            NativeOp::FileSize | NativeOp::FileTime => {
                let got = self.host.file_info(ta.trim());
                let ok = got.is_some();
                let value = got.map_or(String::new(), |(size, time)| {
                    if op == NativeOp::FileSize {
                        size.to_string()
                    } else {
                        time.to_string()
                    }
                });
                self.put(&tb, value);
                ok
            }
            NativeOp::SystemInfo => {
                let got = self.host.system_info(&ta.trim().to_ascii_lowercase());
                let ok = got.is_some();
                self.put(&tb, got.unwrap_or_default());
                ok
            }
            NativeOp::Say => !ta.trim().is_empty() && self.host.say(&ta),
            NativeOp::Screenshot => !ta.trim().is_empty() && self.host.screenshot(ta.trim()),
            NativeOp::VolumeGet => {
                let got = self.host.volume();
                let ok = got.is_some();
                self.put(&tb, got.map_or(String::new(), |v| v.to_string()));
                ok
            }
            NativeOp::VolumeSet => match parse_number(&ta) {
                Some(n) if (0.0..=100.0).contains(&n) => self.host.set_volume(n.round() as u8),
                _ => false,
            },
            NativeOp::Browser => {
                let line = ta.trim();
                // a panel waits for a person and `chat.wait` for a message from somebody else: that
                // time does not count against the run
                let waits = crate::bridge::commands::waits_for_others(line);
                let asked = Instant::now();
                let reply = if line.is_empty() {
                    BrowserReply {
                        ok: false,
                        message: "the block has no command".into(),
                    }
                } else {
                    self.host.browser(line)
                };
                if waits {
                    self.deadline += asked.elapsed();
                }
                // the words go in the variable also when it did not work: they say why
                self.put(&tb, reply.message.clone());
                if !reply.ok {
                    browser_said = reply.message;
                }
                reply.ok
            }
        };
        self.ok(ok);
        if !ok {
            let (kind, mut detail) = native_failure(op, &ta, &tb);
            if op == NativeOp::Browser && !browser_said.is_empty() {
                detail = format!(
                    "the browser did not run `{}`: {}",
                    brief(ta.trim(), 50),
                    brief(&browser_said, 200)
                );
            }
            self.fail(kind, detail);
        }
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
        dirs: BTreeMap<String, Vec<String>>,
        info: BTreeMap<String, String>,
        vol: Option<u8>,
        panels: Vec<crate::panel::PanelResult>,
        browser: Vec<BrowserReply>,
        progress: Vec<Progress>,
        waits: Vec<u64>,
        panel_cmds: Vec<PanelCommand>,
        /// What the host says to a panel command; `None` is that it worked.
        panel_error: Option<String>,
        /// `cancelled` says yes from the call with this number on (0 = never).
        cancel_from: usize,
        cancel_calls: usize,
    }

    impl Host for Mock {
        fn cancelled(&mut self) -> bool {
            self.cancel_calls += 1;
            self.cancel_from > 0 && self.cancel_calls >= self.cancel_from
        }
        fn panel_command(&mut self, command: &PanelCommand) -> Result<(), String> {
            self.panel_cmds.push(command.clone());
            match &self.panel_error {
                Some(why) => Err(why.clone()),
                None => Ok(()),
            }
        }
        fn progress(&mut self, progress: Progress) {
            self.progress.push(progress);
        }
        fn wait(&mut self, ms: u64) {
            self.waits.push(ms);
            self.sleep(ms);
        }

        fn ask_panel(
            &mut self,
            spec: &crate::panel::PanelSpec,
        ) -> Option<crate::panel::PanelResult> {
            self.log.push(format!("panel {}", spec.title));
            if self.panels.is_empty() {
                None
            } else {
                Some(self.panels.remove(0))
            }
        }

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
        fn list_dir(&mut self, path: &str) -> Option<Vec<String>> {
            self.dirs.get(path).cloned()
        }
        fn make_dir(&mut self, path: &str) -> bool {
            self.log.push(format!("mkdir {path}"));
            true
        }
        fn copy_file(&mut self, from: &str, to: &str) -> bool {
            let Some(text) = self.files.get(from).cloned() else {
                return false;
            };
            self.files.insert(to.to_string(), text);
            true
        }
        fn move_file(&mut self, from: &str, to: &str) -> bool {
            let Some(text) = self.files.remove(from) else {
                return false;
            };
            self.files.insert(to.to_string(), text);
            true
        }
        fn delete_file(&mut self, path: &str) -> bool {
            self.files.remove(path).is_some()
        }
        fn file_info(&mut self, path: &str) -> Option<(u64, u64)> {
            self.files
                .get(path)
                .map(|t| (t.len() as u64, 1_700_000_000))
        }
        fn system_info(&mut self, key: &str) -> Option<String> {
            self.info.get(key).cloned()
        }
        fn say(&mut self, text: &str) -> bool {
            self.log.push(format!("say {text}"));
            true
        }
        fn screenshot(&mut self, path: &str) -> bool {
            self.log.push(format!("shot {path}"));
            true
        }
        fn volume(&mut self) -> Option<u8> {
            self.vol
        }
        fn set_volume(&mut self, percent: u8) -> bool {
            self.vol = Some(percent);
            true
        }
        fn browser(&mut self, line: &str) -> BrowserReply {
            self.log.push(format!("browser {line}"));
            if self.browser.is_empty() {
                BrowserReply::default()
            } else {
                self.browser.remove(0)
            }
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
    fn the_host_is_told_how_far_a_straight_run_has_come() {
        let f = line(vec![
            NodeKind::Key(Shortcut::new(8, 2)),
            NodeKind::Wait(500),
            NodeKind::Key(Shortcut::new(9, 2)),
        ]);
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        // a straight flow knows its length: three blocks (Start and End are not counted)
        assert!(h.progress.len() >= 3);
        assert!(h.progress.iter().all(|p| p.total == 3 && p.round.is_none()));
        let steps: Vec<u32> = h.progress.iter().map(|p| p.step).collect();
        assert_eq!(steps[..3], [1, 2, 3], "one report per block, counting up");
        // the Wait block goes through `wait`, so a host can show it
        assert_eq!(h.waits, [500]);
    }

    #[test]
    fn a_loop_reports_its_round_and_a_flow_with_a_loop_has_no_total() {
        let mut f = Flow::new("f", "F");
        let r = f.add_after(None, NodeKind::Repeat(3));
        let k = f.add_after(Some(r), NodeKind::Key(Shortcut::new(8, 2)));
        assert!(f.connect(k, Port::Next, r));
        let after = loose(&mut f, NodeKind::OpenApp("done".into()));
        f.node_mut(after).unwrap().next = Some(2);
        assert!(f.connect(r, Port::Alt, after));
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert!(
            h.progress.iter().all(|p| p.total == 0),
            "the length is not known"
        );
        let rounds: Vec<(u32, u32)> = h.progress.iter().filter_map(|p| p.round).collect();
        assert_eq!(rounds.first(), Some(&(1, 3)));
        assert_eq!(rounds.iter().max(), Some(&(3, 3)));
        // after the loop the run is no longer in a round
        assert_eq!(h.progress.last().map(|p| p.round), Some(None));
    }

    // ---- the progress panel

    fn panel_block(op: PanelOp, title: &str, label: &str, value: &str, stop: bool) -> NodeKind {
        NodeKind::PanelUpdate {
            op,
            panel: String::new(),
            title: title.into(),
            label: label.into(),
            value: value.into(),
            stop,
        }
    }

    #[test]
    fn a_panel_value_is_a_fraction_a_percentage_or_busy() {
        assert_eq!(parse_panel_value(""), Some(PanelValue::Keep));
        assert_eq!(parse_panel_value("  "), Some(PanelValue::Keep));
        assert_eq!(parse_panel_value("busy"), Some(PanelValue::Busy));
        assert_eq!(parse_panel_value("BUSY"), Some(PanelValue::Busy));
        assert_eq!(parse_panel_value("0.25"), Some(PanelValue::Fraction(0.25)));
        assert_eq!(parse_panel_value("0,5"), Some(PanelValue::Fraction(0.5)));
        assert_eq!(parse_panel_value("40%"), Some(PanelValue::Fraction(0.4)));
        assert_eq!(parse_panel_value("40 %"), Some(PanelValue::Fraction(0.4)));
        assert_eq!(
            parse_panel_value("250%"),
            Some(PanelValue::Fraction(1.0)),
            "cut to full"
        );
        assert_eq!(
            parse_panel_value("7"),
            Some(PanelValue::Fraction(1.0)),
            "a plain number is a fraction"
        );
        assert_eq!(parse_panel_value("-3"), Some(PanelValue::Fraction(0.0)));
        assert_eq!(parse_panel_value("soon"), None);
        assert_eq!(parse_panel_value("NaN"), None);
        assert_eq!(parse_panel_value("inf%"), None);
    }

    #[test]
    fn an_update_panel_block_opens_changes_and_closes_a_panel_without_waiting() {
        let mut f = Flow::new("f", "F");
        f.add_after(
            None,
            panel_block(PanelOp::Open, "Copying", "Starting", "0", true),
        );
        f.add_after(
            None,
            NodeKind::Set {
                name: "n".into(),
                value: "3".into(),
            },
        );
        f.add_after(
            None,
            panel_block(PanelOp::Update, "", "File {{n}}", "40%", true),
        );
        f.add_after(None, panel_block(PanelOp::Update, "", "", "busy", false));
        f.add_after(
            None,
            panel_block(PanelOp::Close, "ignored", "ignored", "0.9", true),
        );
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert!(out.problems().is_empty());
        let c = &h.panel_cmds;
        assert_eq!(c.len(), 4);
        // the id is the default one, empty fields are left alone, the stop button is for Open only
        assert_eq!(c[0].panel, DEFAULT_PROGRESS_PANEL);
        assert_eq!(
            (
                c[0].op,
                c[0].title.as_deref(),
                c[0].label.as_deref(),
                c[0].value,
                c[0].stop
            ),
            (
                PanelOp::Open,
                Some("Copying"),
                Some("Starting"),
                PanelValue::Fraction(0.0),
                true
            )
        );
        assert_eq!(
            (c[1].title.clone(), c[1].label.as_deref()),
            (None, Some("File 3"))
        );
        assert_eq!((c[1].value, c[1].stop), (PanelValue::Fraction(0.4), false));
        assert_eq!(c[2].value, PanelValue::Busy);
        assert_eq!((c[3].op, c[3].value), (PanelOp::Close, PanelValue::Keep));
        // no pause was added for any of them: only the Set has none either
        assert_eq!(h.slept, 0);
        assert_eq!(out.vars.get("ok"), Some("1"));
    }

    #[test]
    fn a_panel_that_cannot_be_shown_is_a_failure_and_ok_is_zero_but_the_flow_goes_on() {
        let f = line(vec![
            panel_block(PanelOp::Open, "T", "", "", false),
            NodeKind::Set {
                name: "after".into(),
                value: "yes".into(),
            },
        ]);
        let mut h = Mock {
            panel_error: Some("Panels are switched off".into()),
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(out.vars.get("after"), Some("yes"), "the flow went on");
        assert_eq!(out.vars.get("ok"), Some("0"));
        let problems = out.problems();
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].detail.contains("Panels are switched off"),
            "{problems:?}"
        );
    }

    #[test]
    fn a_bad_value_leaves_the_bar_as_it_was() {
        let f = line(vec![panel_block(PanelOp::Update, "", "x", "soon", false)]);
        let mut h = Mock::default();
        let out = go(&f, &mut h);
        assert_eq!(h.panel_cmds[0].value, PanelValue::Keep, "the bar stays");
        assert_eq!(
            h.panel_cmds[0].label.as_deref(),
            Some("x"),
            "the rest still goes through"
        );
        assert_eq!(out.vars.get("ok"), Some("0"));
        let problems = out.problems();
        assert!(
            problems.iter().any(|p| p.detail.contains("`soon`")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_named_panel_keeps_its_name_and_the_name_may_hold_a_variable() {
        let mut f = Flow::new("f", "F");
        f.add_after(
            None,
            NodeKind::Set {
                name: "who".into(),
                value: "job2".into(),
            },
        );
        f.add_after(
            None,
            NodeKind::PanelUpdate {
                op: PanelOp::Open,
                panel: "{{who}}".into(),
                title: String::new(),
                label: String::new(),
                value: String::new(),
                stop: false,
            },
        );
        let mut h = Mock::default();
        go(&f, &mut h);
        assert_eq!(h.panel_cmds[0].panel, "job2");
        assert_eq!(h.panel_cmds[0].title, None);
    }

    #[test]
    fn stop_ends_the_run_at_the_next_block_and_is_not_a_problem() {
        let f = line(vec![
            NodeKind::Set {
                name: "a".into(),
                value: "1".into(),
            },
            NodeKind::Set {
                name: "b".into(),
                value: "2".into(),
            },
            NodeKind::Set {
                name: "c".into(),
                value: "3".into(),
            },
            NodeKind::Set {
                name: "d".into(),
                value: "4".into(),
            },
        ]);
        // asked at the start of every block after Start: a, b, then "stop" at the block of c
        let mut h = Mock {
            cancel_from: 3,
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Cancelled);
        assert_eq!(out.vars.get("b"), Some("2"));
        assert_eq!(out.vars.get("c"), None, "c never ran");
        assert!(out.problems().is_empty(), "stopping is not a failure");
        assert!(out.report("f", "F").is_none());
    }

    #[test]
    fn stop_ends_a_loop_that_would_have_gone_on() {
        let mut f = Flow::new("f", "F");
        let r = f.add_after(None, NodeKind::Repeat(1000));
        let k = f.add_after(
            Some(r),
            NodeKind::Set {
                name: "x".into(),
                value: "{{index}}".into(),
            },
        );
        assert!(f.connect(k, Port::Next, r));
        let mut h = Mock {
            cancel_from: 30,
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(out.ending, Ending::Cancelled);
        let rounds: u32 = out.vars.get("index").and_then(|v| v.parse().ok()).unwrap();
        assert!(
            (10..40).contains(&rounds),
            "stopped after {rounds} rounds, not 1000"
        );
    }

    #[test]
    fn a_host_that_never_cancels_changes_nothing() {
        let f = line(vec![NodeKind::Set {
            name: "a".into(),
            value: "1".into(),
        }]);
        let mut h = Mock::default();
        assert_eq!(go(&f, &mut h).ending, Ending::Finished);
        assert!(h.cancel_calls >= 2, "asked at every block");
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

    fn panel_flow(json: &str) -> Flow {
        line(vec![
            NodeKind::Set {
                name: "who".into(),
                value: "Ann".into(),
            },
            NodeKind::Ask {
                mode: AskMode::Panel,
                message: json.into(),
                preset: String::new(),
                out: "btn".into(),
            },
        ])
    }

    fn submitted(
        button: &str,
        values: &[(&str, crate::panel::Value)],
    ) -> crate::panel::PanelResult {
        crate::panel::PanelResult {
            panel: String::new(),
            outcome: crate::panel::Outcome::Submitted,
            button: button.into(),
            values: values
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
            reason: String::new(),
        }
    }

    #[test]
    fn a_panel_sets_a_variable_per_field_and_the_button_that_was_pressed() {
        use crate::panel::Value;
        let f = panel_flow(
            r#"{"title":"Hi {{who}}","widgets":[{"type":"input","id":"name"},{"type":"toggle","id":"loud"},
                {"type":"slider","id":"vol","min":0,"max":1},{"type":"multi","id":"tags","options":["a","b"]}]}"#,
        );
        let mut h = Mock {
            panels: vec![submitted(
                "ok",
                &[
                    ("name", Value::Text("Bo".into())),
                    ("loud", Value::Bool(true)),
                    ("vol", Value::Number(0.5)),
                    ("tags", Value::List(vec!["a".into(), "b".into()])),
                ],
            )],
            ..Mock::default()
        };
        let out = go(&f, &mut h);
        assert_eq!(h.log, ["panel Hi Ann"], "variables are filled in, per text");
        assert_eq!(out.vars.get("btn"), Some("ok"));
        assert_eq!(out.vars.get("name"), Some("Bo"));
        assert_eq!(out.vars.get("loud"), Some("1"));
        assert_eq!(out.vars.get("vol"), Some("0.5"));
        assert_eq!(out.vars.get("tags"), Some(r#"["a","b"]"#));
        assert_eq!(out.vars.get("ok"), Some("1"));
        assert!(out.problems().is_empty());
    }

    #[test]
    fn a_cancelled_or_timed_out_panel_is_an_answer_not_a_failure() {
        let f = panel_flow(r#"{"title":"Q"}"#);
        for result in [
            crate::panel::PanelResult::cancelled("", ""),
            crate::panel::PanelResult::timeout(""),
        ] {
            let mut h = Mock {
                panels: vec![result],
                ..Mock::default()
            };
            let out = go(&f, &mut h);
            assert_eq!(out.vars.get("btn"), Some(""));
            assert_eq!(out.vars.get("ok"), Some("0"));
            assert!(out.problems().is_empty());
        }
    }

    #[test]
    fn a_panel_that_cannot_be_shown_or_read_is_a_failure_that_says_why() {
        // the host has no panels
        let mut h = Mock::default();
        let out = go(&panel_flow(r#"{"title":"Q"}"#), &mut h);
        assert_eq!(out.vars.get("ok"), Some("0"));
        assert_eq!(out.problems().len(), 1);
        // switched off or busy: the answer says it was unavailable
        let mut h = Mock {
            panels: vec![crate::panel::PanelResult::unavailable(
                "",
                "Panels are switched off",
            )],
            ..Mock::default()
        };
        let out = go(&panel_flow(r#"{"title":"Q"}"#), &mut h);
        let p = out.problems();
        assert_eq!(p.len(), 1);
        assert!(p[0].detail.contains("switched off"), "{}", p[0].detail);
        // not a panel: the host is never asked
        let mut h = Mock::default();
        let out = go(&panel_flow("{ not json"), &mut h);
        assert!(h.log.is_empty());
        assert_eq!(out.problems().len(), 1);
    }

    #[test]
    fn a_value_with_quotes_in_a_variable_cannot_change_the_shape_of_the_panel() {
        let f = line(vec![
            NodeKind::Set {
                name: "who".into(),
                value: r#"x","buttons":[{"id":"evil"}],"y":""#.into(),
            },
            NodeKind::Ask {
                mode: AskMode::Panel,
                message: r#"{"title":"Hi {{who}}"}"#.into(),
                preset: String::new(),
                out: "btn".into(),
            },
        ]);
        let mut h = Mock {
            panels: vec![crate::panel::PanelResult::cancelled("", "")],
            ..Mock::default()
        };
        go(&f, &mut h);
        assert_eq!(h.log.len(), 1, "the panel was read and shown, as one panel");
        assert!(h.log[0].contains("evil"), "only inside its title");
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
    fn a_block_that_cannot_do_its_work_is_written_down_unless_the_flow_looked_at_ok() {
        let run_block = || NodeKind::Run {
            command: "false".into(),
            out: "o".into(),
            timeout_s: 5,
        };
        let failing = || Mock {
            exec: Some(ExecResult {
                code: Some(2),
                stdout: "boom\nmore\n".into(),
            }),
            ..Mock::default()
        };
        // nobody looks at `ok`: it is a problem
        let f = line(vec![run_block()]);
        let out = go(&f, &mut failing());
        let problems = out.problems();
        assert_eq!(problems.len(), 1, "{:?}", out.failures);
        assert_eq!(problems[0].kind, FailKind::Command);
        assert_eq!(problems[0].block_name, "Run and wait");
        assert_eq!(problems[0].flow, "F");
        assert!(
            problems[0].detail.contains("code 2"),
            "{}",
            problems[0].detail
        );
        assert!(
            problems[0].detail.contains("boom"),
            "{}",
            problems[0].detail
        );
        assert!(
            !problems[0].detail.contains("more"),
            "{}",
            problems[0].detail
        );

        // an If / else on `ok` right after it: the flow dealt with it
        let f = line(vec![
            run_block(),
            NodeKind::Branch(cond("ok", CmpOp::IsTrue, "")),
        ]);
        let out = go(&f, &mut failing());
        assert_eq!(out.failures.len(), 1);
        assert!(out.failures[0].handled);
        assert!(out.problems().is_empty());

        // the same with `code`, also through a {{variable}}
        let f = line(vec![
            run_block(),
            NodeKind::Branch(cond("{{code}}", CmpOp::Ne, "0")),
        ]);
        assert!(go(&f, &mut failing()).problems().is_empty());

        // a command that cannot be started at all
        let f = line(vec![run_block()]);
        let out = go(&f, &mut Mock::default());
        assert_eq!(out.problems().len(), 1);
        assert!(out.problems()[0].detail.contains("could not be started"));

        // a command that works is not a problem
        let mut ok = Mock {
            exec: Some(ExecResult {
                code: Some(0),
                stdout: String::new(),
            }),
            ..Mock::default()
        };
        assert!(go(&line(vec![run_block()]), &mut ok).failures.is_empty());
    }

    #[test]
    fn a_failure_is_not_handled_by_a_decision_that_looks_at_something_else() {
        let f = line(vec![
            NodeKind::Native {
                op: NativeOp::ReadFile,
                a: "/no/such/file".into(),
                b: "text".into(),
            },
            set("x", "1"),
            NodeKind::Branch(cond("x", CmpOp::IsTrue, "")),
        ]);
        let out = go(&f, &mut Mock::default());
        let problems = out.problems();
        assert_eq!(problems.len(), 1, "{:?}", out.failures);
        assert_eq!(problems[0].kind, FailKind::File);
        assert!(
            problems[0].detail.contains("/no/such/file"),
            "{}",
            problems[0].detail
        );
    }

    #[test]
    fn a_missing_called_flow_a_bad_calculation_and_a_limit_are_written_down() {
        let f = line(vec![NodeKind::Call("nope".into())]);
        let out = go(&f, &mut Mock::default());
        assert_eq!(out.problems().len(), 1);
        assert_eq!(out.problems()[0].kind, FailKind::Call);
        assert!(out.problems()[0].detail.contains("nope"));

        let f = line(vec![NodeKind::Calc {
            out: "r".into(),
            a: "abc".into(),
            op: CalcOp::Add,
            b: "1".into(),
        }]);
        let out = go(&f, &mut Mock::default());
        assert_eq!(out.problems().len(), 1);
        assert_eq!(out.problems()[0].kind, FailKind::Calculation);
        assert!(out.problems()[0].detail.contains("abc"));

        // a ring of blocks with no way out is stopped by the limit, and that is said
        let mut g = Flow::new("g", "G");
        let a = g.add_after(None, NodeKind::Wait(0));
        let b = g.add_after(Some(a), NodeKind::Wait(0));
        assert!(g.connect(b, Port::Next, a));
        let out = go(&g, &mut Mock::default());
        assert_eq!(out.ending, Ending::OutOfSteps);
        assert!(out.problems().iter().any(|p| p.kind == FailKind::Limit));
    }

    #[test]
    fn a_loop_that_fails_every_round_says_so_once_with_a_count() {
        let mut f = Flow::new("f", "F");
        let r = f.add_after(None, NodeKind::Repeat(3));
        let c = f.add_after(
            Some(r),
            NodeKind::Calc {
                out: "n".into(),
                a: "x".into(),
                op: CalcOp::Add,
                b: "1".into(),
            },
        );
        assert!(f.connect(c, Port::Next, r));
        let out = go(&f, &mut Mock::default());
        assert_eq!(out.failures.len(), 1, "{:?}", out.failures);
        assert_eq!(out.failures[0].times, 3);
    }

    #[test]
    fn the_report_says_where_what_and_how_often_and_is_empty_when_all_is_well() {
        let ok_run = go(&line(vec![set("x", "1")]), &mut Mock::default());
        assert!(ok_run.report("f", "F").is_none());

        let mut f = Flow::new("f", "F");
        let r = f.add_after(None, NodeKind::Repeat(2));
        let c = f.add_after(
            Some(r),
            NodeKind::Calc {
                out: "n".into(),
                a: "x".into(),
                op: CalcOp::Add,
                b: "1".into(),
            },
        );
        assert!(f.connect(c, Port::Next, r));
        let out = go(&f, &mut Mock::default());
        let report = out.report("f", "F").expect("a calculation failed");
        assert_eq!(report.flow_id, "f");
        assert_eq!(report.flow, "F");
        assert_eq!(report.lines.len(), 1);
        assert!(report.lines[0].starts_with(&format!("Block {c} (Calculate): ")));
        assert!(report.lines[0].contains("(2 times)"), "{}", report.lines[0]);
        assert_eq!(report.title, "A calculation failed");
        assert_eq!(report.marks.len(), 1);
        assert_eq!(report.marks[0].0, c);

        // two different problems: a count in the title, and "and 1 more" in the summary
        let g = line(vec![
            NodeKind::Call("nope".into()),
            NodeKind::Native {
                op: NativeOp::ReadFile,
                a: "/x".into(),
                b: "t".into(),
            },
        ]);
        let report = go(&g, &mut Mock::default()).report("g", "F").unwrap();
        assert_eq!(report.title, "2 problems");
        assert!(
            report.summary.ends_with("(and 1 more)"),
            "{}",
            report.summary
        );
    }

    fn vars_of(pairs: &[(&str, &str)]) -> Vars {
        let mut v = Vars::default();
        for (k, val) in pairs {
            let _ = v.set(k, *val);
        }
        v
    }

    #[test]
    fn filters_can_be_chained_and_do_what_they_say() {
        let v = vars_of(&[
            ("x", "  hello wORLD "),
            ("n", "-3.7"),
            ("p", "/Users/me/My Files/report.final.PDF"),
            ("xs", r#"["b","a","c"]"#),
            ("empty", ""),
        ]);
        let e = |t: &str| expand(t, &v, OS);
        assert_eq!(e("{{x|trim|upper}}"), "HELLO WORLD");
        assert_eq!(e("{{x|trim|title}}"), "Hello World");
        assert_eq!(e("{{x|trim|len}}"), "11");
        assert_eq!(e("{{x|trim|lower|reverse}}"), "dlrow olleh");
        assert_eq!(
            e("{{n|int}} {{n|round}} {{n|abs}} {{n|num}}"),
            "-3 -4 3.7 -3.7"
        );
        assert_eq!(
            e("{{p|basename}}|{{p|dirname}}|{{p|ext}}"),
            "report.final.PDF|/Users/me/My Files|PDF"
        );
        assert_eq!(
            e("{{p|url}}"),
            "%2FUsers%2Fme%2FMy%20Files%2Freport.final.PDF"
        );
        assert_eq!(e("{{x|trim|json}}"), "\"hello wORLD\"");
        assert_eq!(e("{{x|trim|sh}}"), "'hello wORLD'");
        assert_eq!(e("{{x|trim|replace:o:0}}"), "hell0 wORLD");
        assert_eq!(
            e("{{xs|count}} {{xs|first}} {{xs|last}} {{xs|join}}"),
            "3 b c b, a, c"
        );
        assert_eq!(e("{{xs|join:\\n}}"), "b\na\nc");
        assert_eq!(e("{{xs|item:2}} {{xs|item:-1}}"), "a c");
        assert_eq!(
            e("{{empty|default:none}} {{x|trim|default:none}}"),
            "none hello wORLD"
        );
        // a variable that was never set can be given a default; without one it stays as written
        assert_eq!(e("{{nope|default:fallback}}"), "fallback");
        assert_eq!(e("{{nope}}"), "{{nope}}");
        assert_eq!(e("{{nope|trim}}"), "{{nope|trim}}");
        // a filter that does not exist, or cannot work on the value, leaves the text as written
        assert_eq!(e("{{x|shout}}"), "{{x|shout}}");
        assert_eq!(e("{{x|count}}"), "{{x|count}}");
        assert_eq!(e("{{x|int}}"), "{{x|int}}");
        assert_eq!(e("{{xs|item:9}}"), "{{xs|item:9}}");
        assert!(is_known_filter("trim") && is_known_filter("join:, ") && is_known_filter("item:1"));
        assert!(!is_known_filter("nope") && !is_known_filter("item") && !is_known_filter("trim:x"));
    }

    #[test]
    fn a_list_is_a_json_array_in_a_variable_and_the_list_block_works_on_it() {
        let list = |op: ListOp, a: &str, b: &str, out: &str| NodeKind::List {
            op,
            list: "xs".into(),
            a: a.into(),
            b: b.into(),
            out: out.into(),
        };
        let f = line(vec![
            list(ListOp::Split, "pear\napple\ncherry\n", "", ""),
            list(ListOp::Sort, "", "", ""),
            list(ListOp::Get, "1", "", "first"),
            list(ListOp::Get, "-1", "", "last"),
            list(ListOp::Push, "apple", "", ""),
            list(ListOp::Length, "", "", "n"),
            list(ListOp::Unique, "", "", ""),
            list(ListOp::Length, "", "", "m"),
            list(ListOp::IndexOf, "cherry", "", "at"),
            list(ListOp::Join, " | ", "", "joined"),
            list(ListOp::SetItem, "1", "fig", ""),
            list(ListOp::Remove, "-1", "", ""),
            list(ListOp::Pop, "", "", "popped"),
            list(ListOp::Reverse, "", "", ""),
        ]);
        let out = go(&f, &mut Mock::default());
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        let get = |k: &str| out.vars.get(k).unwrap_or("<unset>").to_string();
        assert_eq!(get("first"), "apple");
        assert_eq!(get("last"), "pear");
        assert_eq!(get("n"), "4");
        assert_eq!(get("m"), "3");
        assert_eq!(get("at"), "2");
        assert_eq!(get("joined"), "apple | cherry | pear");
        // fig, cherry, pear -> remove the last (pear) -> pop (cherry) -> fig
        assert_eq!(get("popped"), "cherry");
        assert_eq!(get("xs"), r#"["fig"]"#);
    }

    #[test]
    fn numbers_sort_as_numbers_and_sum() {
        let list = |op: ListOp, a: &str, out: &str| NodeKind::List {
            op,
            list: "xs".into(),
            a: a.into(),
            b: String::new(),
            out: out.into(),
        };
        let f = line(vec![
            set("xs", r#"["10","9","100","2.5"]"#),
            list(ListOp::Sort, "", ""),
            list(ListOp::Sum, "", "total"),
            list(ListOp::Shuffle, "", ""),
            list(ListOp::Sort, "", ""),
        ]);
        let out = go(&f, &mut Mock::default());
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        assert_eq!(out.vars.get("xs"), Some(r#"["2.5","9","10","100"]"#));
        assert_eq!(out.vars.get("total"), Some("121.5"));
    }

    #[test]
    fn a_list_block_that_cannot_work_says_why() {
        let list = |op: ListOp, a: &str| NodeKind::List {
            op,
            list: "xs".into(),
            a: a.into(),
            b: String::new(),
            out: "r".into(),
        };
        // a position that is not there
        let f = line(vec![set("xs", r#"["a","b"]"#), list(ListOp::Get, "5")]);
        let out = go(&f, &mut Mock::default());
        let problems = out.problems();
        assert_eq!(problems.len(), 1, "{:?}", out.failures);
        assert_eq!(problems[0].kind, FailKind::List);
        assert!(
            problems[0].detail.contains("position `5`"),
            "{}",
            problems[0].detail
        );
        assert!(
            problems[0].detail.contains("2 items"),
            "{}",
            problems[0].detail
        );
        // an empty list has nothing to take
        let f = line(vec![list(ListOp::Pop, "")]);
        assert_eq!(go(&f, &mut Mock::default()).problems().len(), 1);
        // a variable that is not a list
        let f = line(vec![set("xs", "hello"), list(ListOp::Length, "")]);
        let out = go(&f, &mut Mock::default());
        assert_eq!(out.problems().len(), 1);
        assert!(out.problems()[0].detail.contains("does not hold a list"));
        assert_eq!(out.vars.get("xs"), Some("hello"));
        // an item that is not a number has no sum
        let f = line(vec![set("xs", r#"["1","x"]"#), list(ListOp::Sum, "")]);
        assert_eq!(go(&f, &mut Mock::default()).problems().len(), 1);
    }

    #[test]
    fn for_each_goes_through_every_item_with_its_position() {
        let mut f = Flow::new("f", "F");
        let a = f.add_after(None, set("xs", r#"["a","b","c"]"#));
        let b = f.add_after(Some(a), set("acc", ""));
        let each = f.add_after(
            Some(b),
            NodeKind::ForEach {
                list: "xs".into(),
                item: "it".into(),
            },
        );
        let body = f.add_after(Some(each), set("acc", "{{acc}}{{index}}{{it}} "));
        assert!(f.connect(body, Port::Next, each));
        let out = go(&f, &mut Mock::default());
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(out.vars.get("acc"), Some("1a 2b 3c "));
        assert!(out.failures.is_empty(), "{:?}", out.failures);

        // an empty list: the body never runs, the loop goes on at once
        let mut g = Flow::new("g", "G");
        let a = g.add_after(None, set("acc", "none"));
        let each = g.add_after(
            Some(a),
            NodeKind::ForEach {
                list: "nothing".into(),
                item: "it".into(),
            },
        );
        let body = g.add_after(Some(each), set("acc", "ran"));
        assert!(g.connect(body, Port::Next, each));
        assert_eq!(go(&g, &mut Mock::default()).vars.get("acc"), Some("none"));

        // a variable that holds something else than a list is said
        let mut h = Flow::new("h", "H");
        let a = h.add_after(None, set("xs", "text"));
        let each = h.add_after(
            Some(a),
            NodeKind::ForEach {
                list: "xs".into(),
                item: "it".into(),
            },
        );
        let _ = each;
        let out = go(&h, &mut Mock::default());
        assert_eq!(out.problems().len(), 1, "{:?}", out.failures);
        assert_eq!(out.problems()[0].kind, FailKind::List);
    }

    #[test]
    fn the_new_system_blocks_use_the_host_and_say_when_it_cannot() {
        let nat = |op: NativeOp, a: &str, b: &str| NodeKind::Native {
            op,
            a: a.into(),
            b: b.into(),
        };
        let mut host = Mock::default();
        host.dirs
            .insert("/docs".into(), vec!["a.txt".into(), "b.txt".into()]);
        host.files.insert("/docs/a.txt".into(), "12345".into());
        host.info.insert("user".into(), "ana".into());
        host.vol = Some(40);
        let f = line(vec![
            nat(NativeOp::ListDir, "/docs", "names"),
            nat(NativeOp::FileSize, "/docs/a.txt", "size"),
            nat(NativeOp::FileTime, "/docs/a.txt", "time"),
            nat(NativeOp::CopyFile, "/docs/a.txt", "/docs/c.txt"),
            nat(NativeOp::MoveFile, "/docs/c.txt", "/docs/d.txt"),
            nat(NativeOp::DeleteFile, "/docs/a.txt", ""),
            nat(NativeOp::MakeDir, "/docs/new", ""),
            nat(NativeOp::SystemInfo, "USER", "who"),
            nat(NativeOp::Say, "hello {{who}}", ""),
            nat(NativeOp::Screenshot, "/tmp/s.png", ""),
            nat(NativeOp::VolumeGet, "", "vol"),
            nat(NativeOp::VolumeSet, "75", ""),
        ]);
        let out = go(&f, &mut host);
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        assert_eq!(out.vars.get("names"), Some(r#"["a.txt","b.txt"]"#));
        assert_eq!(out.vars.get("size"), Some("5"));
        assert_eq!(out.vars.get("time"), Some("1700000000"));
        assert_eq!(out.vars.get("who"), Some("ana"));
        assert_eq!(out.vars.get("vol"), Some("40"));
        assert_eq!(host.vol, Some(75));
        assert!(host.files.contains_key("/docs/d.txt"));
        assert!(!host.files.contains_key("/docs/a.txt"));
        assert!(
            host.log.contains(&"say hello ana".to_string()),
            "{:?}",
            host.log
        );
        assert!(host.log.contains(&"shot /tmp/s.png".to_string()));
        assert!(host.log.contains(&"mkdir /docs/new".to_string()));

        // what cannot be done is said, with the reason
        let f = line(vec![
            nat(NativeOp::ListDir, "/nope", "names"),
            nat(NativeOp::VolumeSet, "loud", ""),
            nat(NativeOp::SystemInfo, "colour", "c"),
        ]);
        let out = go(&f, &mut Mock::default());
        let problems = out.problems();
        assert_eq!(problems.len(), 3, "{:?}", out.failures);
        assert_eq!(problems[0].kind, FailKind::File);
        assert!(problems[0].detail.contains("/nope"));
        assert_eq!(problems[1].kind, FailKind::System);
        assert_eq!(out.vars.get("names"), Some("[]"));
    }

    #[test]
    fn a_browser_block_sends_the_command_and_keeps_the_answer() {
        let browser = |line: &str, var: &str| NodeKind::Native {
            op: NativeOp::Browser,
            a: line.into(),
            b: var.into(),
        };
        let mut host = Mock::default();
        host.browser.push(BrowserReply {
            ok: true,
            message: "https://example.org/".into(),
        });
        host.browser.push(BrowserReply {
            ok: true,
            message: String::new(),
        });
        let f = line(vec![
            set("word", "docs"),
            browser("tab.url", "address"),
            browser("tab.find {{word}}", ""),
        ]);
        let out = go(&f, &mut host);
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        assert_eq!(out.vars.get("address"), Some("https://example.org/"));
        assert_eq!(out.vars.get("ok"), Some("1"));
        assert_eq!(host.log, ["browser tab.url", "browser tab.find docs"]);
    }

    #[test]
    fn a_browser_block_that_failed_says_what_the_browser_said() {
        let browser = |line: &str, var: &str| NodeKind::Native {
            op: NativeOp::Browser,
            a: line.into(),
            b: var.into(),
        };
        let mut host = Mock::default();
        host.browser.push(BrowserReply {
            ok: false,
            message: "No player is open".into(),
        });
        let f = line(vec![browser("media.next", "why")]);
        let out = go(&f, &mut host);
        assert_eq!(out.vars.get("ok"), Some("0"));
        assert_eq!(out.vars.get("why"), Some("No player is open"));
        let problems = out.problems();
        assert_eq!(problems.len(), 1, "{:?}", out.failures);
        assert_eq!(problems[0].kind, FailKind::System);
        assert!(problems[0].detail.contains("No player is open"));
        assert!(problems[0].detail.contains("media.next"));
    }

    #[test]
    fn a_browser_block_without_a_command_does_not_call_the_host() {
        let f = line(vec![NodeKind::Native {
            op: NativeOp::Browser,
            a: "  ".into(),
            b: String::new(),
        }]);
        let mut host = Mock::default();
        let out = go(&f, &mut host);
        assert!(host.log.is_empty());
        assert_eq!(out.vars.get("ok"), Some("0"));
        assert_eq!(out.problems().len(), 1, "{:?}", out.failures);
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

    // ---- the chat examples of docs/FLOWS_FOR_AI.md, run against a host that answers from a script

    const QUICK_REPLY_ASK: &str = "Type your reply, or press Cancel to skip this chat.";
    const QUICK_REPLY_PRESET: &str = "Ok, thanks! I will answer properly soon.";

    fn chat_reply(ok: bool, message: &str) -> BrowserReply {
        BrowserReply {
            ok,
            message: message.into(),
        }
    }

    fn quick_reply_flow() -> Flow {
        crate::config::flow::parse_flow(include_str!(
            "../docs/examples/flows/quick-reply-unread-chats.cursorpie-flow.json"
        ))
        .expect("a valid flow file")
    }

    fn send_wait_flow() -> Flow {
        crate::config::flow::parse_flow(include_str!(
            "../docs/examples/flows/send-then-wait-for-answer.cursorpie-flow.json"
        ))
        .expect("a valid flow file")
    }

    fn quick_reply_question(chat: &str, last: &str) -> String {
        format!("ask {chat} wrote:\n\n{last}\n\n{QUICK_REPLY_ASK} / {QUICK_REPLY_PRESET}")
    }

    #[test]
    fn the_quick_reply_example_asks_about_each_unread_chat_and_sends_only_what_was_confirmed() {
        let mut h = Mock {
            texts: vec![Some("Siap!".into()), None],
            yes: vec![Some(true)],
            browser: vec![
                chat_reply(true, "Anna\nBudi"),
                chat_reply(true, ""),
                chat_reply(true, "Jadi jam 5?"),
                chat_reply(true, ""),
                chat_reply(true, ""),
                chat_reply(true, "ping"),
            ],
            ..Mock::default()
        };
        let out = go(&quick_reply_flow(), &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(
            h.log,
            [
                "browser chat.unread_names".to_string(),
                "browser chat.open Anna".to_string(),
                "browser chat.last".to_string(),
                quick_reply_question("Anna", "Jadi jam 5?"),
                "yesno Send this to Anna?\n\nSiap!".to_string(),
                "browser chat.send_to Anna :: Siap!".to_string(),
                "browser chat.open Budi".to_string(),
                "browser chat.last".to_string(),
                // cancelled: nothing is sent to Budi and the run ends
                quick_reply_question("Budi", "ping"),
            ]
        );
        assert!(out.problems().is_empty(), "{:?}", out.failures);
    }

    #[test]
    fn the_quick_reply_example_does_not_send_when_the_person_says_no_or_types_nothing() {
        let mut h = Mock {
            texts: vec![Some("Siap!".into()), Some(String::new())],
            yes: vec![Some(false)],
            browser: vec![
                chat_reply(true, "Anna\nBudi"),
                chat_reply(true, ""),
                chat_reply(true, "halo"),
                chat_reply(true, ""),
                chat_reply(true, "hai"),
            ],
            ..Mock::default()
        };
        let out = go(&quick_reply_flow(), &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert!(
            !h.log.iter().any(|l| l.contains("chat.send")),
            "nothing is sent without a yes: {:?}",
            h.log
        );
        assert!(out.problems().is_empty(), "{:?}", out.failures);
    }

    #[test]
    fn the_quick_reply_example_says_why_when_the_chats_cannot_be_read_and_when_none_wait() {
        let why = "Reading messages is off: turn on 'Allow reading messages' in the CursorPie Bridge options";
        let mut h = Mock {
            browser: vec![chat_reply(false, why)],
            ..Mock::default()
        };
        let out = go(&quick_reply_flow(), &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(
            h.log,
            [
                "browser chat.unread_names".to_string(),
                format!("notify Cannot read the chats|{why}"),
            ]
        );
        assert!(
            out.problems().is_empty(),
            "the flow looked at ok: {:?}",
            out.failures
        );

        let mut h = Mock {
            browser: vec![chat_reply(true, "")],
            ..Mock::default()
        };
        go(&quick_reply_flow(), &mut h);
        assert_eq!(
            h.log,
            [
                "browser chat.unread_names",
                "notify No unread chats|Nothing waits in WhatsApp Web or Telegram Web",
            ]
        );
    }

    #[test]
    fn the_quick_reply_example_skips_a_chat_it_cannot_open_and_goes_on_to_the_next() {
        let mut h = Mock {
            texts: vec![None],
            browser: vec![
                chat_reply(true, "Anna\nBudi"),
                chat_reply(false, "No chat called Anna"),
                chat_reply(true, ""),
                chat_reply(true, "ping"),
            ],
            ..Mock::default()
        };
        let out = go(&quick_reply_flow(), &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(
            h.log,
            [
                "browser chat.unread_names".to_string(),
                "browser chat.open Anna".to_string(),
                "notify Skipped Anna|No chat called Anna".to_string(),
                "browser chat.open Budi".to_string(),
                "browser chat.last".to_string(),
                quick_reply_question("Budi", "ping"),
            ]
        );
        assert!(out.problems().is_empty(), "{:?}", out.failures);
    }

    #[test]
    fn the_quick_reply_example_tells_when_a_message_was_not_sent() {
        let mut h = Mock {
            texts: vec![Some("Siap!".into())],
            yes: vec![Some(true)],
            browser: vec![
                chat_reply(true, "Anna"),
                chat_reply(true, ""),
                chat_reply(true, "halo"),
                chat_reply(false, "The open chat is not Anna"),
            ],
            ..Mock::default()
        };
        let out = go(&quick_reply_flow(), &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(
            h.log.last().map(String::as_str),
            Some("notify Not sent to Anna|The open chat is not Anna")
        );
        assert!(out.problems().is_empty(), "{:?}", out.failures);
    }

    #[test]
    fn the_send_and_wait_example_sends_then_waits_and_tells_the_answer() {
        let mut h = Mock {
            texts: vec![Some("Anna".into()), Some("Kamu di mana?".into())],
            yes: vec![Some(true)],
            browser: vec![chat_reply(true, ""), chat_reply(true, "Di rumah")],
            ..Mock::default()
        };
        let out = go(&send_wait_flow(), &mut h);
        assert_eq!(out.ending, Ending::Finished);
        assert_eq!(
            h.log,
            [
                "ask Who do you want to write to? Type the name as it shows in your chat list. / "
                    .to_string(),
                "ask Message to Anna: / ".to_string(),
                "yesno Send this to Anna and then wait for the answer?\n\nKamu di mana?"
                    .to_string(),
                "browser chat.send_to Anna :: Kamu di mana?".to_string(),
                "browser chat.wait 120".to_string(),
                "notify Anna answered|Di rumah".to_string(),
            ]
        );
        assert!(out.problems().is_empty(), "{:?}", out.failures);
    }

    #[test]
    fn the_send_and_wait_example_says_so_when_nobody_answers_in_time() {
        let mut h = Mock {
            texts: vec![Some("Anna".into()), Some("halo".into())],
            yes: vec![Some(true)],
            browser: vec![chat_reply(true, ""), chat_reply(true, "")],
            ..Mock::default()
        };
        let out = go(&send_wait_flow(), &mut h);
        assert_eq!(
            h.log.last().map(String::as_str),
            Some("notify No answer yet|Anna did not answer in 120 seconds")
        );
        assert!(out.problems().is_empty(), "{:?}", out.failures);
    }

    #[test]
    fn the_send_and_wait_example_does_not_wait_for_a_message_that_was_not_sent() {
        let mut h = Mock {
            texts: vec![Some("Anna".into()), Some("halo".into())],
            yes: vec![Some(true)],
            browser: vec![chat_reply(false, "The open chat is not Anna")],
            ..Mock::default()
        };
        let out = go(&send_wait_flow(), &mut h);
        assert!(
            !h.log.iter().any(|l| l.starts_with("browser chat.wait")),
            "{:?}",
            h.log
        );
        assert_eq!(
            h.log.last().map(String::as_str),
            Some("notify Not sent to Anna|The open chat is not Anna")
        );
        assert!(out.problems().is_empty(), "{:?}", out.failures);

        // and nothing is sent when the person does not confirm
        let mut h = Mock {
            texts: vec![Some("Anna".into()), Some("halo".into())],
            yes: vec![Some(false)],
            ..Mock::default()
        };
        go(&send_wait_flow(), &mut h);
        assert!(
            !h.log.iter().any(|l| l.starts_with("browser ")),
            "{:?}",
            h.log
        );
    }
}
