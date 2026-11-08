//! Flows: graphs of blocks joined by wires (the *Flow* tab of Settings).
//!
//! A [`Flow`] is a small program drawn as a graph. Every block ([`FlowNode`]) has one input and
//! one or two outputs: most blocks have the output `next`; the blocks that decide ([`NodeKind::Branch`],
//! [`NodeKind::Repeat`], [`NodeKind::While`]) have a second one, `alt` (see [`Port`]). Any number of
//! wires can lead into a block, so two paths can join again, and a wire may lead back to an earlier
//! block: that is how a loop is made. A run starts at the one [`NodeKind::Start`] block, does what
//! each block says and follows the wire it leads to; it ends at [`NodeKind::End`], at
//! [`NodeKind::Stop`], at an output without a wire, or when it hits one of the limits of
//! [`flowrun`](crate::flowrun). Blocks that no path from *Start* reaches are ignored.
//!
//! A run has **variables**: named texts that the blocks read and write (`{{name}}` in a text field
//! is replaced by the value of the variable). The blocks that compute, ask or look at the system
//! put their result in a variable, and the blocks that decide read them. The interpreter lives in
//! [`flowrun`](crate::flowrun); this module is the data model, the rules that keep a flow valid
//! and the share file.
//!
//! A slice uses a flow through [`Action::Flow`], which names the flow by its `id`. The flows live in
//! [`Config::flows`](super::Config::flows), so they are saved, backed up, exported and undone with the rest of the config.
//!
//! ## Sharing
//!
//! A flow is shared as one JSON file, `<name>.cursorpie-flow.json`:
//!
//! ```text
//! { "cursorpie_flow": 2, "flow": { "id": "...", "name": "...", "nodes": [...] } }
//! ```
//!
//! The number is 1 for a flow that only uses the blocks and wires of the first version, so those
//! files still open in an older CursorPie, and 2 when a block or a second output needs this one.
//!
//! A profile file ([`share`](super::share)) also carries the flows its slices use, so a shared
//! profile keeps working. Importing never trusts a file: the flow goes through [`Flow::clean`]
//! (limits, one *Start* and one *End*), it always gets a **new** id and a unique name, and the
//! caller warns when it can run shell commands or write files ([`Flow::has_commands`]).

use super::share::safe_stem;
use super::{
    Action, Item, ItemKind, MAX_FIELD_CHARS, MAX_FLOW_NODES, MAX_FLOWS, MAX_REPEAT, MAX_RUN_SECS,
    MAX_VAR_NAME_CHARS, MAX_WAIT_MS,
};
use crate::keys::Shortcut;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::PathBuf;

/// Value of `cursorpie_flow` in files that need this version of CursorPie (see [`Flow::format`]).
pub const FLOW_FORMAT: u32 = 2;
/// Value of `cursorpie_flow` in files that an older CursorPie can open too.
pub const LEGACY_FLOW_FORMAT: u32 = 1;
/// Larger flow files are refused without being read.
pub const MAX_FLOW_FILE_BYTES: u64 = 1024 * 1024;
/// Blocks are kept inside this distance of the origin, so a hostile file cannot place one at
/// infinity.
pub const COORD_LIMIT: f32 = 20_000.0;
/// Width of an ordinary block in the editor, in points.
pub const NODE_W: f32 = 280.0;
/// Width of the *Start*, *End* and *Stop* blocks.
pub const START_W: f32 = 150.0;
/// Longest name of a flow, in characters.
pub const MAX_NAME_CHARS: usize = 60;
/// How long a *Run* block waits for its command when the file does not say.
const DEFAULT_RUN_SECS: u32 = 10;

fn default_run_secs() -> u32 {
    DEFAULT_RUN_SECS
}

/// Which output of a block a wire leaves from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Port {
    /// The ordinary output: the next block, the *then* branch of a [`NodeKind::Branch`], the body
    /// of a loop.
    Next,
    /// The second output of a block that decides: the *else* branch, or what follows a loop.
    Alt,
}

/// How a [`Cond`] compares its two operands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    /// Equal (as numbers when both are numbers, as text otherwise).
    #[default]
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// The left text contains the right one.
    Contains,
    StartsWith,
    EndsWith,
    /// The left operand is true (not empty, not `0`, `false`, `no` or `off`). The right one is unused.
    IsTrue,
    /// The left operand is empty or only spaces. The right one is unused.
    IsEmpty,
}

impl CmpOp {
    /// Every comparison, in the order the editor lists them.
    pub const ALL: [CmpOp; 11] = [
        CmpOp::Eq,
        CmpOp::Ne,
        CmpOp::Lt,
        CmpOp::Le,
        CmpOp::Gt,
        CmpOp::Ge,
        CmpOp::Contains,
        CmpOp::StartsWith,
        CmpOp::EndsWith,
        CmpOp::IsTrue,
        CmpOp::IsEmpty,
    ];

    /// What the editor shows for it.
    pub const fn label(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::Ne => "!=",
            CmpOp::Lt => "<",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Ge => ">=",
            CmpOp::Contains => "has",
            CmpOp::StartsWith => "begins",
            CmpOp::EndsWith => "ends",
            CmpOp::IsTrue => "is true",
            CmpOp::IsEmpty => "is empty",
        }
    }

    /// Whether the comparison looks at the left operand only.
    pub const fn is_unary(self) -> bool {
        matches!(self, CmpOp::IsTrue | CmpOp::IsEmpty)
    }
}

/// What a *Branch* or *While* block asks: `a op b`. An operand is a value or the name of a
/// variable (see [`flowrun`](crate::flowrun)).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cond {
    #[serde(default)]
    pub a: String,
    #[serde(default)]
    pub op: CmpOp,
    #[serde(default)]
    pub b: String,
}

/// The arithmetic of a *Calc* block.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalcOp {
    #[default]
    Add,
    Sub,
    Mul,
    Div,
    /// The remainder of a division.
    Rem,
    Min,
    Max,
}

impl CalcOp {
    /// Every operation, in the order the editor lists them.
    pub const ALL: [CalcOp; 7] = [
        CalcOp::Add,
        CalcOp::Sub,
        CalcOp::Mul,
        CalcOp::Div,
        CalcOp::Rem,
        CalcOp::Min,
        CalcOp::Max,
    ];

    /// What the editor shows for it.
    pub const fn label(self) -> &'static str {
        match self {
            CalcOp::Add => "+",
            CalcOp::Sub => "-",
            CalcOp::Mul => "*",
            CalcOp::Div => "/",
            CalcOp::Rem => "%",
            CalcOp::Min => "min",
            CalcOp::Max => "max",
        }
    }
}

/// The logic gate of a *Logic* block. The operands are read as true or false.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicOp {
    #[default]
    And,
    Or,
    Xor,
    Nand,
    Nor,
    /// Inverts the left operand. The right one is unused.
    Not,
}

impl LogicOp {
    /// Every gate, in the order the editor lists them.
    pub const ALL: [LogicOp; 6] = [
        LogicOp::And,
        LogicOp::Or,
        LogicOp::Xor,
        LogicOp::Nand,
        LogicOp::Nor,
        LogicOp::Not,
    ];

    /// What the editor shows for it.
    pub const fn label(self) -> &'static str {
        match self {
            LogicOp::And => "AND",
            LogicOp::Or => "OR",
            LogicOp::Xor => "XOR",
            LogicOp::Nand => "NAND",
            LogicOp::Nor => "NOR",
            LogicOp::Not => "NOT",
        }
    }

    /// Whether the gate looks at the left operand only.
    pub const fn is_unary(self) -> bool {
        matches!(self, LogicOp::Not)
    }
}

/// What an *Ask* block shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AskMode {
    /// A box to type a text in.
    #[default]
    Text,
    /// A question with Yes and No.
    YesNo,
}

impl AskMode {
    /// Every mode, in the order the editor lists them.
    pub const ALL: [AskMode; 2] = [AskMode::Text, AskMode::YesNo];

    /// What the editor shows for it.
    pub const fn label(self) -> &'static str {
        match self {
            AskMode::Text => "Text",
            AskMode::YesNo => "Yes / No",
        }
    }
}

/// What a *Native* block does with the operating system. Each one takes up to two texts, `a` and
/// `b`; [`NativeOp::hints`] says what each is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum NativeOp {
    /// Puts `a` on the clipboard.
    #[default]
    ClipboardSet,
    /// Reads the clipboard into the variable `b`.
    ClipboardGet,
    /// Types `a` into the app in front (through the clipboard and the paste shortcut; the clipboard is put back).
    TypeText,
    /// Shows a desktop notification: title `a`, text `b`.
    Notify,
    /// Reads the file `a` (text, at most 256 KB) into the variable `b`.
    ReadFile,
    /// Writes `b` to the file `a`, replacing what was there.
    WriteFile,
    /// Adds `b` at the end of the file `a`.
    AppendFile,
    /// Puts 1 in the variable `b` when the file or folder `a` exists, 0 otherwise.
    FileExists,
    /// Reads the environment variable `a` into the variable `b`.
    Env,
    /// Puts the id of the app in front in the variable `b`.
    FrontApp,
    /// Brings the app with the id `a` to the front.
    ActivateApp,
    /// Puts 1 in the variable `b` when the app `a` has a window, 0 otherwise.
    AppRunning,
    /// Puts the position of the pointer in the variables `a` (x) and `b` (y).
    CursorPos,
    /// Puts a whole number from 0 up to, not including, `a` in the variable `b`.
    Random,
    /// Puts the seconds since 1 January 1970 (UTC) in the variable `b`.
    UnixTime,
    /// Shows the file or folder `a` in the file manager.
    Reveal,
}

impl NativeOp {
    /// Every operation, in the order the editor lists them.
    pub const ALL: [NativeOp; 16] = [
        NativeOp::ClipboardSet,
        NativeOp::ClipboardGet,
        NativeOp::TypeText,
        NativeOp::Notify,
        NativeOp::ReadFile,
        NativeOp::WriteFile,
        NativeOp::AppendFile,
        NativeOp::FileExists,
        NativeOp::Env,
        NativeOp::FrontApp,
        NativeOp::ActivateApp,
        NativeOp::AppRunning,
        NativeOp::CursorPos,
        NativeOp::Random,
        NativeOp::UnixTime,
        NativeOp::Reveal,
    ];

    /// What the editor shows for it.
    pub const fn label(self) -> &'static str {
        match self {
            NativeOp::ClipboardSet => "Copy text to clipboard",
            NativeOp::ClipboardGet => "Read clipboard",
            NativeOp::TypeText => "Type text",
            NativeOp::Notify => "Notification",
            NativeOp::ReadFile => "Read file",
            NativeOp::WriteFile => "Write file",
            NativeOp::AppendFile => "Append to file",
            NativeOp::FileExists => "File exists?",
            NativeOp::Env => "Environment variable",
            NativeOp::FrontApp => "App in front",
            NativeOp::ActivateApp => "Bring app to front",
            NativeOp::AppRunning => "App has a window?",
            NativeOp::CursorPos => "Pointer position",
            NativeOp::Random => "Random number",
            NativeOp::UnixTime => "Time (Unix seconds)",
            NativeOp::Reveal => "Show in file manager",
        }
    }

    /// What the two text fields are for, as the hints the editor shows. An empty hint means the
    /// operation does not use that field and the editor leaves it out.
    pub const fn hints(self) -> (&'static str, &'static str) {
        match self {
            NativeOp::ClipboardSet => ("text to copy", ""),
            NativeOp::ClipboardGet => ("", "variable"),
            NativeOp::TypeText => ("text to type", ""),
            NativeOp::Notify => ("title", "text"),
            NativeOp::ReadFile => ("file path", "variable"),
            NativeOp::WriteFile | NativeOp::AppendFile => ("file path", "text"),
            NativeOp::FileExists => ("file or folder path", "variable"),
            NativeOp::Env => ("name, e.g. HOME", "variable"),
            NativeOp::FrontApp | NativeOp::UnixTime => ("", "variable"),
            NativeOp::ActivateApp => ("app id", ""),
            NativeOp::AppRunning => ("app", "variable"),
            NativeOp::CursorPos => ("x variable", "y variable"),
            NativeOp::Random => ("below (e.g. 100)", "variable"),
            NativeOp::Reveal => ("file or folder path", ""),
        }
    }

    /// Whether field `a` names a variable that gets a result.
    pub const fn a_is_variable(self) -> bool {
        matches!(self, NativeOp::CursorPos)
    }

    /// Whether field `b` names a variable that gets a result.
    pub const fn b_is_variable(self) -> bool {
        matches!(
            self,
            NativeOp::ClipboardGet
                | NativeOp::ReadFile
                | NativeOp::FileExists
                | NativeOp::Env
                | NativeOp::FrontApp
                | NativeOp::AppRunning
                | NativeOp::CursorPos
                | NativeOp::Random
                | NativeOp::UnixTime
        )
    }
}

/// What a block does.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum NodeKind {
    /// Where a run begins. There is exactly one.
    Start,
    /// Press a shortcut.
    Key(Shortcut),
    /// Open (or raise) an app.
    OpenApp(String),
    /// Open a URL or a file.
    OpenUrl(String),
    /// Run a shell command and go on without waiting for it.
    Command(String),
    /// Wait this many milliseconds before the next block.
    Wait(u32),
    /// Where the chain ends. There is exactly one; it has an input and no output.
    End,
    /// Puts `value` (with its `{{variables}}` filled in) in the variable `name`.
    Set {
        #[serde(default)]
        name: String,
        #[serde(default)]
        value: String,
    },
    /// `out = a op b`, as numbers. A text that is not a number, or a division by zero, leaves
    /// `out` empty and `ok` at 0.
    Calc {
        #[serde(default)]
        out: String,
        #[serde(default)]
        a: String,
        #[serde(default)]
        op: CalcOp,
        #[serde(default)]
        b: String,
    },
    /// `out = a gate b`, as 1 or 0.
    Logic {
        #[serde(default)]
        out: String,
        #[serde(default)]
        a: String,
        #[serde(default)]
        op: LogicOp,
        #[serde(default)]
        b: String,
    },
    /// Follows `next` when the condition holds and `alt` when it does not.
    Branch(Cond),
    /// Goes through `next` (the body) this many times, then follows `alt`. The variable `index`
    /// counts the rounds from 1.
    Repeat(u32),
    /// Goes through `next` (the body) as long as the condition holds, then follows `alt`.
    While(Cond),
    /// Asks the person, in a window of the system, and puts the answer in `out`. `ok` says whether
    /// there was one.
    Ask {
        #[serde(default)]
        mode: AskMode,
        #[serde(default)]
        message: String,
        /// What the box starts with ([`AskMode::Text`] only).
        #[serde(default)]
        preset: String,
        #[serde(default)]
        out: String,
    },
    /// Runs a shell command, **waits** for it (at most `timeout_s` seconds), puts what it printed
    /// in `out` and its exit code in the variable `code`.
    Run {
        #[serde(default)]
        command: String,
        #[serde(default)]
        out: String,
        #[serde(default = "default_run_secs")]
        timeout_s: u32,
    },
    /// Something the operating system does or tells.
    Native {
        #[serde(default)]
        op: NativeOp,
        #[serde(default)]
        a: String,
        #[serde(default)]
        b: String,
    },
    /// Runs the flow with this id, with the same variables, then goes on.
    Call(String),
    /// Ends the whole run, also when this flow was called by another one.
    Stop,
}

impl NodeKind {
    /// The action this block performs; `None` for the blocks that are not a plain action (*Start*,
    /// *End* and everything that computes, decides or asks).
    pub fn to_action(&self) -> Option<Action> {
        match self {
            NodeKind::Key(sc) => Some(Action::Key(*sc)),
            NodeKind::OpenApp(s) => Some(Action::OpenApp(s.clone())),
            NodeKind::OpenUrl(s) => Some(Action::OpenUrl(s.clone())),
            NodeKind::Command(s) => Some(Action::Command(s.clone())),
            NodeKind::Wait(ms) => Some(Action::Wait(*ms)),
            _ => None,
        }
    }

    /// Whether this is one of the blocks that are only a header: *Start*, *End* and *Stop*.
    pub fn is_marker(&self) -> bool {
        matches!(self, NodeKind::Start | NodeKind::End | NodeKind::Stop)
    }

    /// Width of the block in the editor.
    pub fn width(&self) -> f32 {
        if self.is_marker() { START_W } else { NODE_W }
    }

    /// The outputs the block has, in the order the editor draws them.
    pub fn outputs(&self) -> &'static [Port] {
        match self {
            NodeKind::End | NodeKind::Stop => &[],
            NodeKind::Branch(_) | NodeKind::Repeat(_) | NodeKind::While(_) => {
                &[Port::Next, Port::Alt]
            }
            _ => &[Port::Next],
        }
    }

    /// The names of the two outputs for the editor; empty for an output the block does not have
    /// or one that needs no name.
    pub fn output_names(&self) -> [&'static str; 2] {
        match self {
            NodeKind::Branch(_) => ["then", "else"],
            NodeKind::Repeat(_) | NodeKind::While(_) => ["body", "done"],
            _ => ["", ""],
        }
    }

    /// Whether the block is one of the first version (a file with only these opens in an older CursorPie).
    pub fn is_legacy(&self) -> bool {
        matches!(
            self,
            NodeKind::Start
                | NodeKind::Key(_)
                | NodeKind::OpenApp(_)
                | NodeKind::OpenUrl(_)
                | NodeKind::Command(_)
                | NodeKind::Wait(_)
                | NodeKind::End
        )
    }

    /// Whether the block runs a shell command or writes a file: what an import warns about.
    pub fn is_sensitive(&self) -> bool {
        match self {
            NodeKind::Command(_) | NodeKind::Run { .. } => true,
            NodeKind::Native { op, .. } => {
                matches!(op, NativeOp::WriteFile | NativeOp::AppendFile)
            }
            _ => false,
        }
    }

    /// Whether the block sends keystrokes, so the app of the slice has to be in front first.
    pub fn sends_keys(&self) -> bool {
        match self {
            NodeKind::Key(_) => true,
            NodeKind::Native { op, .. } => matches!(op, NativeOp::TypeText),
            _ => false,
        }
    }

    /// Forces the fields of the block into their limits: texts of at most [`MAX_FIELD_CHARS`],
    /// variable names that [`is_var_name`] accepts, a wait of at most [`MAX_WAIT_MS`], a loop of at
    /// most [`MAX_REPEAT`] rounds and a command timeout of 1 to [`MAX_RUN_SECS`] seconds.
    fn clean(&mut self) {
        fn text(s: &mut String) {
            if s.chars().count() > MAX_FIELD_CHARS {
                *s = s.chars().take(MAX_FIELD_CHARS).collect();
            }
        }
        fn var(s: &mut String) {
            *s = clean_var_name(s);
        }
        match self {
            NodeKind::Start | NodeKind::End | NodeKind::Stop | NodeKind::Key(_) => {}
            NodeKind::OpenApp(s)
            | NodeKind::OpenUrl(s)
            | NodeKind::Command(s)
            | NodeKind::Call(s) => text(s),
            NodeKind::Wait(ms) => *ms = (*ms).min(MAX_WAIT_MS),
            NodeKind::Repeat(n) => *n = (*n).min(MAX_REPEAT),
            NodeKind::Set { name, value } => {
                var(name);
                text(value);
            }
            NodeKind::Calc { out, a, b, .. } | NodeKind::Logic { out, a, b, .. } => {
                var(out);
                text(a);
                text(b);
            }
            NodeKind::Branch(c) | NodeKind::While(c) => {
                text(&mut c.a);
                text(&mut c.b);
            }
            NodeKind::Ask {
                message,
                preset,
                out,
                ..
            } => {
                text(message);
                text(preset);
                var(out);
            }
            NodeKind::Run {
                command,
                out,
                timeout_s,
            } => {
                text(command);
                var(out);
                *timeout_s = (*timeout_s).clamp(1, MAX_RUN_SECS);
            }
            NodeKind::Native { op, a, b } => {
                if op.a_is_variable() {
                    var(a);
                } else {
                    text(a);
                }
                if op.b_is_variable() {
                    var(b);
                } else {
                    text(b);
                }
            }
        }
    }
}

/// Whether `s` can be the name of a variable: 1 to [`MAX_VAR_NAME_CHARS`] letters, digits and
/// underscores (ASCII), not starting with a digit.
pub fn is_var_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars().count() <= MAX_VAR_NAME_CHARS
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !s.starts_with(|c: char| c.is_ascii_digit())
}

/// The variable name closest to `s`: what [`is_var_name`] does not allow is dropped. May be empty.
pub fn clean_var_name(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .skip_while(char::is_ascii_digit)
        .take(MAX_VAR_NAME_CHARS)
        .collect()
}

/// One block of a flow.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct FlowNode {
    /// Unique inside its flow.
    pub id: u32,
    /// Where the block sits in the editor (top left corner, in points from the origin).
    pub x: f32,
    pub y: f32,
    pub kind: NodeKind,
    /// The block the wire from this block's output goes to ([`Port::Next`]).
    #[serde(default)]
    pub next: Option<u32>,
    /// The block the wire from the second output goes to ([`Port::Alt`]); only the blocks that
    /// decide have one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<u32>,
}

impl FlowNode {
    /// The block the wire from `port` leads to.
    pub fn wire(&self, port: Port) -> Option<u32> {
        match port {
            Port::Next => self.next,
            Port::Alt => self.alt,
        }
    }

    /// Mutable version of [`FlowNode::wire`].
    fn wire_mut(&mut self, port: Port) -> &mut Option<u32> {
        match port {
            Port::Next => &mut self.next,
            Port::Alt => &mut self.alt,
        }
    }

    /// Both wires that leave the block (`None` for an output without one).
    pub fn wires(&self) -> [Option<u32>; 2] {
        [self.next, self.alt]
    }
}

/// A named graph of blocks.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Flow {
    /// What [`Action::Flow`] refers to. Unique in the config; never shown.
    pub id: String,
    pub name: String,
    pub nodes: Vec<FlowNode>,
}

impl Flow {
    /// A new flow with its *Start* block wired straight to its *End* block.
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            nodes: vec![
                FlowNode {
                    id: 1,
                    x: 40.0,
                    y: 60.0,
                    kind: NodeKind::Start,
                    next: Some(2),
                    alt: None,
                },
                FlowNode {
                    id: 2,
                    x: 40.0 + START_W + 50.0,
                    y: 60.0,
                    kind: NodeKind::End,
                    next: None,
                    alt: None,
                },
            ],
        }
    }

    /// The *End* block.
    pub fn end(&self) -> Option<&FlowNode> {
        self.nodes.iter().find(|n| matches!(n.kind, NodeKind::End))
    }

    /// The last block of the chain that is not *End*: where a new block goes by default.
    fn last_before_end(&self) -> Option<u32> {
        self.chain()
            .iter()
            .rev()
            .find(|n| !matches!(n.kind, NodeKind::End))
            .map(|n| n.id)
    }

    /// The *Start* block.
    pub fn start(&self) -> Option<&FlowNode> {
        self.nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Start))
    }

    /// The block with this id.
    pub fn node(&self, id: u32) -> Option<&FlowNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Mutable version of [`Flow::node`].
    pub fn node_mut(&mut self, id: u32) -> Option<&mut FlowNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// An id no block uses yet.
    pub fn next_node_id(&self) -> u32 {
        self.nodes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    /// The blocks reached from *Start* by following only the `next` wires, in order. For a flow
    /// without decisions and loops this is everything that runs; [`Flow::reachable`] is the general
    /// version.
    pub fn chain(&self) -> Vec<&FlowNode> {
        let mut out: Vec<&FlowNode> = Vec::new();
        let mut seen = BTreeSet::new();
        let mut cur = self.start();
        while let Some(n) = cur {
            if !seen.insert(n.id) {
                break; // a loop: the chain ends where it comes back
            }
            out.push(n);
            cur = n.next.and_then(|id| self.node(id));
        }
        out
    }

    /// Every block a run can get to: *Start*, and whatever its wires, of both outputs, lead to,
    /// each once, in the order a breadth-first walk finds them. A block that is not here never runs.
    pub fn reachable(&self) -> Vec<&FlowNode> {
        let mut out: Vec<&FlowNode> = Vec::new();
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<&FlowNode> = VecDeque::new();
        if let Some(s) = self.start() {
            seen.insert(s.id);
            queue.push_back(s);
        }
        while let Some(n) = queue.pop_front() {
            out.push(n);
            for t in n.wires().into_iter().flatten() {
                if let Some(m) = self.node(t)
                    && seen.insert(m.id)
                {
                    queue.push_back(m);
                }
            }
        }
        out
    }

    /// The id of the last block of the chain.
    pub fn tail(&self) -> Option<u32> {
        self.chain().last().map(|n| n.id)
    }

    /// The plain actions along the `next` wires, in order. Blocks that compute, decide or ask are
    /// not actions and are left out, so this is the whole flow only when it has none of them.
    pub fn steps(&self) -> Vec<Action> {
        self.chain()
            .iter()
            .filter_map(|n| n.kind.to_action())
            .collect()
    }

    /// Whether a run can send keystrokes, so the app of the slice must be brought to the front first.
    pub fn sends_keys(&self) -> bool {
        self.reachable().iter().any(|n| n.kind.sends_keys())
    }

    /// Whether running the flow can run a shell command or write a file. Only the blocks a run
    /// can reach count: an unplugged one never runs. (Importing warns about all of them anyway,
    /// see [`Flow::has_commands`].)
    pub fn runs_commands(&self) -> bool {
        self.reachable().iter().any(|n| n.kind.is_sensitive())
    }

    /// Whether any block, wired or not, runs a shell command or writes a file: what a warning
    /// after an import goes by, because wiring it up later is one drag.
    pub fn has_commands(&self) -> bool {
        self.nodes.iter().any(|n| n.kind.is_sensitive())
    }

    /// The `cursorpie_flow` number the share file of this flow carries: the first version's when an
    /// older CursorPie can still read it, [`FLOW_FORMAT`] when a block or an output needs this one.
    pub fn format(&self) -> u32 {
        if self
            .nodes
            .iter()
            .any(|n| n.alt.is_some() || !n.kind.is_legacy())
        {
            FLOW_FORMAT
        } else {
            LEGACY_FLOW_FORMAT
        }
    }

    /// Adds a block after `after` (or just before *End* when `after` is `None`, gone, or *End*
    /// itself) and wires it in: a block that already had a successor keeps it, behind the new one.
    /// A block that decides (*Branch*, *Repeat*, *While*) gets that successor on the output that
    /// continues the flow (both of a *Branch*; the `alt` of a loop, so its body starts empty).
    /// When the new block goes right before *End*, *End* moves to the right of it.
    /// Returns the id of the new block.
    pub fn add_after(&mut self, after: Option<u32>, kind: NodeKind) -> u32 {
        let end_id = self.end().map(|n| n.id);
        let after_id = after
            .filter(|a| self.node(*a).is_some() && Some(*a) != end_id)
            .or_else(|| self.last_before_end());
        let (ax, ay, aw, anext) = match after_id.and_then(|a| self.node(a)) {
            Some(n) => (n.x, n.y, n.kind.width(), n.next),
            None => (40.0, 60.0, START_W, None),
        };
        let before_end = anext.is_some() && anext == end_id;
        let id = self.next_node_id();
        let (x, y) = if anext.is_some() && !before_end {
            (ax + 24.0, ay + 120.0)
        } else if ax + aw + 50.0 + NODE_W > 900.0 {
            (40.0, ay + 130.0) // the row is full: continue on the next one
        } else {
            (ax + aw + 50.0, ay)
        };
        let (own_next, own_alt) = match &kind {
            NodeKind::Repeat(_) | NodeKind::While(_) => (None, anext),
            NodeKind::Branch(_) => (anext, anext),
            NodeKind::Stop => (None, None),
            _ => (anext, None),
        };
        self.nodes.push(FlowNode {
            id,
            x,
            y,
            kind,
            next: own_next,
            alt: own_alt,
        });
        if let Some(a) = after_id
            && let Some(n) = self.node_mut(a)
        {
            n.next = Some(id);
        }
        if before_end && let Some(e) = end_id {
            let (ex, ey) = if x + NODE_W + 50.0 + START_W > 900.0 {
                (40.0, y + 130.0)
            } else {
                (x + NODE_W + 50.0, y)
            };
            if let Some(n) = self.node_mut(e) {
                n.x = ex;
                n.y = ey;
            }
        }
        id
    }

    /// Adds a block with no wires, with its top left corner at (`x`, `y`) (kept inside
    /// [`COORD_LIMIT`]): what the right-click menu of the editor does, so the person decides where
    /// it goes. Returns the id of the new block.
    pub fn add_at(&mut self, kind: NodeKind, x: f32, y: f32) -> u32 {
        let id = self.next_node_id();
        let fit = |v: f32| {
            if v.is_finite() {
                v.clamp(-COORD_LIMIT, COORD_LIMIT)
            } else {
                0.0
            }
        };
        self.nodes.push(FlowNode {
            id,
            x: fit(x),
            y: fit(y),
            kind,
            next: None,
            alt: None,
        });
        id
    }

    /// Removes a block (never *Start* or *End*) and joins its neighbours: every wire that led into
    /// it now leads where its own `next` did, so deleting a step in the middle of a chain leaves
    /// no gap (a wire that would lead back to its own block is dropped). Returns whether a block
    /// was removed.
    pub fn remove_node(&mut self, id: u32) -> bool {
        let Some(node) = self.node(id) else {
            return false;
        };
        if matches!(node.kind, NodeKind::Start | NodeKind::End) {
            return false;
        }
        let after = node.next;
        for n in &mut self.nodes {
            let me = n.id;
            for port in [Port::Next, Port::Alt] {
                if n.wire(port) == Some(id) {
                    *n.wire_mut(port) = after.filter(|a| *a != me && *a != id);
                }
            }
        }
        self.nodes.retain(|n| n.id != id);
        true
    }

    /// Draws a wire from the output `port` of `from` to the input of `to`. An output gives one
    /// wire, so the one that left it before is replaced; an input takes any number of them, which
    /// is how two paths join. A wire may lead back to an earlier block: that is a loop. Refused
    /// (`false`) for a block to itself, for *Start* as a target, and for an output the block does not have.
    pub fn connect(&mut self, from: u32, port: Port, to: u32) -> bool {
        if from == to {
            return false;
        }
        let (Some(a), Some(b)) = (self.node(from), self.node(to)) else {
            return false;
        };
        if matches!(b.kind, NodeKind::Start) || !a.kind.outputs().contains(&port) {
            return false;
        }
        if let Some(n) = self.node_mut(from) {
            *n.wire_mut(port) = Some(to);
        }
        true
    }

    /// Removes every wire that goes into `to`.
    pub fn unplug_into(&mut self, to: u32) {
        for n in &mut self.nodes {
            for port in [Port::Next, Port::Alt] {
                if n.wire(port) == Some(to) {
                    *n.wire_mut(port) = None;
                }
            }
        }
    }

    /// Removes the wire that leaves the output `port` of `from`.
    pub fn unplug_out(&mut self, from: u32, port: Port) {
        if let Some(n) = self.node_mut(from) {
            *n.wire_mut(port) = None;
        }
    }

    /// Forces the flow into the limits the rest of the program relies on: a name, at most
    /// [`MAX_FLOW_NODES`] blocks with unique ids and reasonable places, exactly one *Start* and one
    /// *End* (a flow from before *End* existed gets one after its last block), the fields of each
    /// block within their limits ([`NodeKind`]), and wires that lead to real blocks, never into
    /// *Start* and never out of an output a block does not have. Loops and several wires into one
    /// block are allowed: a run is limited while it runs, see [`flowrun`](crate::flowrun). Called
    /// for every config that is loaded, imported or saved.
    pub fn clean(&mut self) {
        let name: String = self.name.trim().chars().take(MAX_NAME_CHARS).collect();
        self.name = if name.is_empty() { "Flow".into() } else { name };

        let mut ids = BTreeSet::new();
        self.nodes.retain(|n| ids.insert(n.id));
        let mut have_start = false;
        self.nodes.retain(|n| {
            if matches!(n.kind, NodeKind::Start) {
                let keep = !have_start;
                have_start = true;
                keep
            } else {
                true
            }
        });
        let mut have_end = false;
        self.nodes.retain(|n| {
            if matches!(n.kind, NodeKind::End) {
                let keep = !have_end;
                have_end = true;
                keep
            } else {
                true
            }
        });
        self.nodes.truncate(MAX_FLOW_NODES);
        if !self.nodes.iter().any(|n| matches!(n.kind, NodeKind::Start)) {
            if self.nodes.len() >= MAX_FLOW_NODES {
                self.nodes.pop();
            }
            let id = self.next_node_id();
            self.nodes.insert(
                0,
                FlowNode {
                    id,
                    x: 40.0,
                    y: 60.0,
                    kind: NodeKind::Start,
                    next: None,
                    alt: None,
                },
            );
        }
        // a flow from before *End* existed: one more block (made room for if the flow is full)
        let mut new_end = None;
        if !self.nodes.iter().any(|n| matches!(n.kind, NodeKind::End)) {
            if self.nodes.len() >= MAX_FLOW_NODES
                && let Some(i) = self
                    .nodes
                    .iter()
                    .rposition(|n| !matches!(n.kind, NodeKind::Start | NodeKind::End))
            {
                self.nodes.remove(i);
            }
            let id = self.next_node_id();
            self.nodes.push(FlowNode {
                id,
                x: 0.0,
                y: 0.0,
                kind: NodeKind::End,
                next: None,
                alt: None,
            });
            new_end = Some(id);
        }
        for n in &mut self.nodes {
            n.x = if n.x.is_finite() {
                n.x.clamp(-COORD_LIMIT, COORD_LIMIT)
            } else {
                0.0
            };
            n.y = if n.y.is_finite() {
                n.y.clamp(-COORD_LIMIT, COORD_LIMIT)
            } else {
                0.0
            };
            n.kind.clean();
        }

        // wires: to a block that exists, not to itself, not into Start, only out of an output the
        // block has (nothing comes after End or Stop)
        let live: BTreeSet<u32> = self.nodes.iter().map(|n| n.id).collect();
        let start_id = self.start().map(|n| n.id);
        for n in &mut self.nodes {
            let me = n.id;
            let outputs = n.kind.outputs();
            for port in [Port::Next, Port::Alt] {
                let keep = outputs.contains(&port)
                    && n.wire(port)
                        .is_some_and(|t| t != me && live.contains(&t) && Some(t) != start_id);
                if !keep {
                    *n.wire_mut(port) = None;
                }
            }
        }
        // the End that was just added goes after the last block of the chain and is wired to it
        if let Some(end_id) = new_end {
            let tail = self
                .chain()
                .last()
                .map(|n| (n.id, n.x, n.y, n.kind.width()));
            if let Some((tid, tx, ty, tw)) = tail {
                let (x, y) = if tx + tw + 50.0 + START_W > 900.0 {
                    (40.0, ty + 130.0)
                } else {
                    (tx + tw + 50.0, ty)
                };
                if let Some(e) = self.node_mut(end_id) {
                    e.x = x;
                    e.y = y;
                }
                if let Some(t) = self.node_mut(tid)
                    && t.kind.outputs().contains(&Port::Next)
                {
                    t.next = Some(end_id);
                }
            }
        }
    }
}

// ------------------------------------------------------------------ the list of flows

/// Keeps at most [`MAX_FLOWS`] flows, drops the ones without an id and the repeats of an id, and
/// [`clean`](Flow::clean)s each one.
pub fn clean_flows(flows: &mut Vec<Flow>) {
    flows.truncate(MAX_FLOWS);
    let mut seen = BTreeSet::new();
    flows.retain(|f| !f.id.trim().is_empty() && seen.insert(f.id.clone()));
    for f in flows.iter_mut() {
        f.clean();
    }
}

/// An id that no flow of `flows` has.
pub fn fresh_id(flows: &[Flow]) -> String {
    let mut n = flows.len() + 1;
    loop {
        let id = format!("flow-{n}");
        if !flows.iter().any(|f| f.id == id) {
            return id;
        }
        n += 1;
    }
}

/// `base`, or `base (2)`, `base (3)`... when a flow already has that name.
pub fn unique_name(flows: &[Flow], base: &str) -> String {
    let base = base.trim();
    let base = if base.is_empty() { "Flow" } else { base };
    let mut name = base.to_string();
    let mut n = 2;
    while flows.iter().any(|f| f.name == name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    name
}

/// Adds an imported flow as a **new** flow (new id, unique name) and returns its index.
pub fn add_flow(flows: &mut Vec<Flow>, mut flow: Flow) -> Result<usize, String> {
    if flows.len() >= MAX_FLOWS {
        return Err(format!(
            "There are already {MAX_FLOWS} flows. Delete one first."
        ));
    }
    flow.clean();
    flow.id = fresh_id(flows);
    flow.name = unique_name(flows, &flow.name);
    flows.push(flow);
    Ok(flows.len() - 1)
}

// ------------------------------------------------------------------ flows used by slices

/// Calls `f` for every action of `items`, submenus included.
pub fn each_action(items: &[Item], f: &mut dyn FnMut(&Action)) {
    for it in items {
        match &it.kind {
            ItemKind::Run(a) => f(a),
            ItemKind::Mode {
                back,
                forward,
                extra,
            } => {
                f(back);
                f(forward);
                extra.values().for_each(&mut *f);
            }
            ItemKind::Submenu(children) => each_action(children, f),
        }
    }
}

/// Mutable version of [`each_action`].
pub fn each_action_mut(items: &mut [Item], f: &mut dyn FnMut(&mut Action)) {
    for it in items {
        match &mut it.kind {
            ItemKind::Run(a) => f(a),
            ItemKind::Mode {
                back,
                forward,
                extra,
            } => {
                f(back);
                f(forward);
                extra.values_mut().for_each(&mut *f);
            }
            ItemKind::Submenu(children) => each_action_mut(children, f),
        }
    }
}

/// The ids of the flows the slices in `items` use.
pub fn flow_ids_used(items: &[Item]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    each_action(items, &mut |a| {
        if let Action::Flow(id) = a {
            out.insert(id.clone());
        }
    });
    out
}

/// Brings the flows of an imported profile into `flows` and points the slices of `items` at them.
///
/// A flow that is already there, identical, is reused. Any other gets a place of its own (its own
/// id when that is free, a new one otherwise), so nothing of the receiver is overwritten. On an
/// error nothing is added (the caller keeps a copy of `flows` to restore).
pub fn merge_flows(
    flows: &mut Vec<Flow>,
    incoming: Vec<Flow>,
    items: &mut [Item],
) -> Result<(), String> {
    let mut remap: BTreeMap<String, String> = BTreeMap::new();
    for mut flow in incoming.into_iter().take(MAX_FLOWS) {
        flow.clean();
        let old = flow.id.clone();
        if let Some(same) = flows.iter().find(|f| f.id == old && **f == flow) {
            remap.insert(old, same.id.clone());
            continue;
        }
        if flows.len() >= MAX_FLOWS {
            return Err(format!(
                "There are already {MAX_FLOWS} flows. Delete one first."
            ));
        }
        if old.trim().is_empty() || flows.iter().any(|f| f.id == old) {
            flow.id = fresh_id(flows);
        }
        flow.name = unique_name(flows, &flow.name);
        remap.insert(old, flow.id.clone());
        flows.push(flow);
    }
    each_action_mut(items, &mut |a| {
        if let Action::Flow(id) = a
            && let Some(new) = remap.get(id.as_str())
        {
            *id = new.clone();
        }
    });
    Ok(())
}

// ------------------------------------------------------------------ the flow file

/// The file format. See the module docs.
#[derive(Serialize, Deserialize, Debug)]
struct FlowFile {
    cursorpie_flow: u32,
    flow: Flow,
}

/// The text of the flow file for `flow`.
pub fn export_flow(flow: &Flow) -> Result<String, String> {
    let file = FlowFile {
        cursorpie_flow: flow.format(),
        flow: flow.clone(),
    };
    serde_json::to_string_pretty(&file).map_err(|e| e.to_string())
}

/// A file name for the export of a flow: letters, digits, `-` and `_` only.
pub fn export_flow_file_name(flow_name: &str) -> String {
    format!("{}.cursorpie-flow.json", safe_stem(flow_name, "flow"))
}

/// Writes the flow file to `<downloads>/<name>.cursorpie-flow.json` and returns its path.
pub fn export_flow_to_downloads(flow: &Flow) -> Result<PathBuf, String> {
    let text = export_flow(flow)?;
    let mut p = super::downloads_dir();
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    p.push(export_flow_file_name(&flow.name));
    fs::write(&p, text).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Parses a flow file and cleans the flow. The error is a sentence for the status bar.
pub fn parse_flow(text: &str) -> Result<Flow, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "That file is not a CursorPie flow.".to_string())?;
    match v.get("cursorpie_flow").and_then(|x| x.as_u64()) {
        None => return Err("That file is not a CursorPie flow.".into()),
        Some(n) if n > u64::from(FLOW_FORMAT) => {
            return Err("That flow was made by a newer CursorPie. Update the app first.".into());
        }
        Some(_) => {}
    }
    let file: FlowFile =
        serde_json::from_value(v).map_err(|_| "That flow file is damaged.".to_string())?;
    let mut flow = file.flow;
    flow.clean();
    Ok(flow)
}

/// Reads a flow file. The error is a sentence for the status bar.
pub fn import_flow_file(path: &str) -> Result<Flow, String> {
    let meta = fs::metadata(path).map_err(|_| "Could not read that file.".to_string())?;
    if meta.len() > MAX_FLOW_FILE_BYTES {
        return Err("That file is too large to be a flow.".into());
    }
    let text = fs::read_to_string(path).map_err(|_| "Could not read that file.".to_string())?;
    parse_flow(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{MOD_CMD, Shortcut};

    fn key() -> NodeKind {
        NodeKind::Key(Shortcut::new(6, MOD_CMD))
    }

    /// Start -> key -> wait -> url
    fn sample() -> Flow {
        let mut f = Flow::new("flow-1", "Sample");
        let a = f.add_after(None, key());
        let b = f.add_after(Some(a), NodeKind::Wait(200));
        f.add_after(Some(b), NodeKind::OpenUrl("https://example.com".into()));
        f
    }

    // ---- the examples of docs/FLOWS_FOR_AI.md: what an agent is told to write must load as written

    const EX_OPEN_APP: &str =
        include_str!("../../docs/examples/flows/open-app-and-new-note.cursorpie-flow.json");
    const EX_COPY_PASTE: &str =
        include_str!("../../docs/examples/flows/copy-switch-paste.cursorpie-flow.json");
    const EX_COMMAND: &str =
        include_str!("../../docs/examples/flows/link-wait-command.cursorpie-flow.json");
    const EX_CONFIG: &str = include_str!("../../docs/examples/flows/config-with-flow.json");
    const EX_LOOP: &str =
        include_str!("../../docs/examples/flows/ask-and-loop.cursorpie-flow.json");

    /// A flow loads "as written" when cleaning it changes nothing: every block is on the chain, in
    /// the order of the file's ids, and the chain runs from Start to End.
    fn assert_untouched_by_clean(text: &str) -> Flow {
        let raw: serde_json::Value = serde_json::from_str(text).unwrap();
        let written: FlowFile = serde_json::from_value(raw).unwrap();
        let flow = parse_flow(text).expect("the example is a valid flow file");
        assert_eq!(
            flow, written.flow,
            "clean() repaired something in the example"
        );
        let chain = flow.chain();
        assert_eq!(chain.len(), flow.nodes.len(), "a block is not on the chain");
        assert!(matches!(chain.first().unwrap().kind, NodeKind::Start));
        assert!(matches!(chain.last().unwrap().kind, NodeKind::End));
        flow
    }

    #[test]
    fn the_documented_examples_load_untouched_and_run_in_order() {
        let f = assert_untouched_by_clean(EX_OPEN_APP);
        assert_eq!(
            f.steps(),
            vec![
                Action::OpenApp("Notes".into()),
                Action::Wait(600),
                Action::Key(Shortcut::new(45, MOD_CMD)),
            ]
        );
        let f = assert_untouched_by_clean(EX_COPY_PASTE);
        assert_eq!(
            f.steps(),
            vec![
                Action::Key(Shortcut::new(8, MOD_CMD)),
                Action::Wait(150),
                Action::Key(Shortcut::new(48, MOD_CMD)),
                Action::Wait(250),
                Action::Key(Shortcut::new(9, MOD_CMD)),
            ]
        );
        let f = assert_untouched_by_clean(EX_COMMAND);
        assert!(f.has_commands() && f.runs_commands());
        assert_eq!(f.steps().len(), 3);
    }

    #[test]
    fn the_documented_example_with_decisions_and_a_loop_loads_untouched_and_is_all_reachable() {
        let raw: serde_json::Value = serde_json::from_str(EX_LOOP).unwrap();
        let written: FlowFile = serde_json::from_value(raw).unwrap();
        let flow = parse_flow(EX_LOOP).expect("the example is a valid flow file");
        assert_eq!(
            flow, written.flow,
            "clean() repaired something in the example"
        );
        assert_eq!(flow.reachable().len(), flow.nodes.len());
        assert_eq!(flow.format(), FLOW_FORMAT);
        assert!(!flow.has_commands());
        // the loop: 7 leads back to 5
        assert_eq!(flow.node(7).unwrap().next, Some(5));
        assert_eq!(flow.node(3).unwrap().alt, Some(9));
    }

    #[test]
    fn the_documented_config_example_has_a_slice_that_reaches_its_flow() {
        let c = crate::config::parse_config(EX_CONFIG).expect("the example config loads");
        assert_eq!(c.flows.len(), 1);
        let ids = flow_ids_used(&c.profiles[0].items);
        assert!(ids.contains("morning") && c.flows.iter().any(|f| f.id == "morning"));
        assert_eq!(c.flows[0].steps().len(), 3);
    }

    #[test]
    fn the_documented_failure_modes_are_what_the_page_says() {
        // a missing Start leaves nothing to run
        let no_start = r#"{"cursorpie_flow":1,"flow":{"id":"a","name":"A","nodes":[
            {"id":1,"x":0,"y":0,"kind":{"Wait":10},"next":2},
            {"id":2,"x":0,"y":0,"kind":"End","next":null}]}}"#;
        assert!(parse_flow(no_start).unwrap().steps().is_empty());
        // an unknown kind refuses the file
        let unknown = r#"{"cursorpie_flow":1,"flow":{"id":"a","name":"A","nodes":[
            {"id":1,"x":0,"y":0,"kind":"Start","next":null},
            {"id":2,"x":0,"y":0,"kind":{"TypeText":"hi"},"next":null}]}}"#;
        assert!(parse_flow(unknown).is_err());
        // a wait above the cap is cut, a negative one is a wrong shape
        let long = r#"{"cursorpie_flow":1,"flow":{"id":"a","name":"A","nodes":[
            {"id":1,"x":0,"y":0,"kind":"Start","next":2},
            {"id":2,"x":0,"y":0,"kind":{"Wait":99999},"next":3},
            {"id":3,"x":0,"y":0,"kind":"End","next":null}]}}"#;
        assert_eq!(
            parse_flow(long).unwrap().steps(),
            vec![Action::Wait(MAX_WAIT_MS)]
        );
        assert!(parse_flow(&long.replace("99999", "-5")).is_err());
        // two wires into one block are both kept (two paths can join)
        let two = r#"{"cursorpie_flow":1,"flow":{"id":"a","name":"A","nodes":[
            {"id":1,"x":0,"y":0,"kind":"Start","next":3},
            {"id":2,"x":0,"y":0,"kind":{"Wait":1},"next":3},
            {"id":3,"x":0,"y":0,"kind":"End","next":null}]}}"#;
        let f = parse_flow(two).unwrap();
        assert_eq!(f.node(1).unwrap().next, Some(3));
        assert_eq!(f.node(2).unwrap().next, Some(3));
        // a file made by a newer version is refused with its own sentence
        assert!(
            parse_flow(&long.replace("\"cursorpie_flow\":1", "\"cursorpie_flow\":3"))
                .unwrap_err()
                .contains("newer")
        );
    }

    #[test]
    fn a_new_flow_is_start_wired_to_end_and_does_nothing() {
        let f = Flow::new("a", "A");
        assert_eq!(f.nodes.len(), 2);
        assert!(f.steps().is_empty());
        assert_eq!(f.tail(), Some(2));
        assert_eq!(f.start().and_then(|n| n.next), Some(2));
        assert!(f.end().is_some_and(|n| n.next.is_none()));
    }

    #[test]
    fn new_blocks_go_before_end_and_end_cannot_be_removed_or_started_from() {
        let mut f = Flow::new("a", "A");
        let a = f.add_after(Some(2), key()); // "after End" means before it
        assert_eq!(f.chain().last().map(|n| n.id), Some(2));
        assert_eq!(f.steps().len(), 1);
        assert!(
            f.node(2).unwrap().x > f.node(a).unwrap().x,
            "End moved to the right"
        );
        assert!(!f.remove_node(2));
        assert!(!f.connect(2, Port::Next, a));
    }

    #[test]
    fn a_flow_from_before_end_got_one_after_its_last_block() {
        let mut f = Flow::new("a", "A");
        f.nodes.retain(|n| !matches!(n.kind, NodeKind::End));
        f.nodes[0].next = None;
        f.nodes.push(FlowNode {
            id: 3,
            x: 300.0,
            y: 60.0,
            kind: key(),
            next: None,
            alt: None,
        });
        f.nodes[0].next = Some(3);
        f.clean();
        let end = f.end().expect("clean adds an End");
        assert!(end.x > 300.0);
        assert_eq!(f.chain().last().map(|n| n.id), Some(end.id));
        assert_eq!(f.steps().len(), 1);
    }

    #[test]
    fn adding_blocks_builds_the_chain_in_order() {
        let f = sample();
        assert_eq!(
            f.steps(),
            vec![
                Action::Key(Shortcut::new(6, MOD_CMD)),
                Action::Wait(200),
                Action::OpenUrl("https://example.com".into()),
            ]
        );
    }

    #[test]
    fn a_block_added_after_a_middle_one_goes_between() {
        let mut f = sample();
        let first = f.nodes[2].id;
        f.add_after(Some(first), NodeKind::Command("true".into()));
        let kinds: Vec<_> = f
            .steps()
            .iter()
            .map(|a| matches!(a, Action::Command(_)))
            .collect();
        assert_eq!(kinds, [false, true, false, false]);
    }

    #[test]
    fn removing_a_block_joins_its_neighbours_and_start_stays() {
        let mut f = sample();
        let wait = f.nodes[3].id;
        assert!(f.remove_node(wait));
        assert_eq!(f.steps().len(), 2);
        assert!(!f.remove_node(1), "Start cannot be removed");
        assert!(!f.remove_node(999));
    }

    #[test]
    fn unplugged_blocks_do_not_run() {
        let mut f = sample();
        let wait = f.nodes[3].id;
        f.unplug_into(wait); // cuts the chain after the first step
        assert_eq!(f.steps().len(), 1);
        assert_eq!(f.nodes.len(), 5, "the blocks are still there");
    }

    #[test]
    fn connecting_replaces_the_wire_of_an_output_and_refuses_what_makes_no_sense() {
        let mut f = sample();
        let (k, w, u) = (f.nodes[2].id, f.nodes[3].id, f.nodes[4].id);
        assert!(!f.connect(k, Port::Next, k), "a block to itself");
        assert!(!f.connect(u, Port::Next, 1), "nothing goes into Start");
        assert!(!f.connect(k, Port::Alt, w), "a plain block has one output");
        assert!(!f.connect(2, Port::Next, k), "End has no output");
        // Start -> url directly: the wire out of Start is replaced
        assert!(f.connect(1, Port::Next, u));
        assert_eq!(
            f.steps(),
            vec![Action::OpenUrl("https://example.com".into())]
        );
        assert!(f.node(w).is_some(), "the unplugged blocks are still there");
    }

    #[test]
    fn two_paths_can_join_and_a_wire_can_lead_back() {
        let mut f = Flow::new("a", "A");
        let b = f.add_after(
            None,
            NodeKind::Branch(Cond {
                a: "x".into(),
                op: CmpOp::IsTrue,
                b: String::new(),
            }),
        );
        let t = f.add_after(Some(b), NodeKind::Wait(1));
        // the else output goes to End too: two wires into one block
        assert!(f.connect(b, Port::Alt, 2));
        assert!(f.connect(t, Port::Next, 2));
        f.clean();
        assert_eq!(f.node(b).unwrap().alt, Some(2));
        assert_eq!(f.node(t).unwrap().next, Some(2));
        // a loop: the body leads back to the block that decides
        assert!(f.connect(t, Port::Next, b));
        f.clean();
        assert_eq!(f.node(t).unwrap().next, Some(b), "clean keeps a loop");
        assert!(
            f.chain().len() <= f.nodes.len(),
            "the chain stops at a loop"
        );
        assert_eq!(f.reachable().len(), f.nodes.len());
    }

    #[test]
    fn a_block_placed_by_hand_has_no_wires_and_stays_in_range() {
        let mut f = Flow::new("a", "A");
        let id = f.add_at(NodeKind::Wait(1), 120.0, 80.0);
        let n = f.node(id).unwrap();
        assert_eq!((n.x, n.y, n.next, n.alt), (120.0, 80.0, None, None));
        assert!(
            !f.reachable().iter().any(|m| m.id == id),
            "nothing leads to it yet"
        );
        let far = f.add_at(NodeKind::Stop, f32::INFINITY, -1e9);
        let n = f.node(far).unwrap();
        assert_eq!((n.x, n.y), (0.0, -COORD_LIMIT));
        assert_ne!(id, far);
    }

    #[test]
    fn a_block_that_decides_starts_with_its_outputs_where_the_flow_continues() {
        let mut f = Flow::new("a", "A");
        let r = f.add_after(None, NodeKind::Repeat(3));
        let n = f.node(r).unwrap();
        assert_eq!(n.next, None, "the body starts empty");
        assert_eq!(n.alt, Some(2), "and what follows the loop is End");
        let b = f.add_after(Some(1), NodeKind::Branch(Cond::default()));
        let n = f.node(b).unwrap();
        assert_eq!((n.next, n.alt), (Some(r), Some(r)));
        let s = f.add_after(Some(b), NodeKind::Stop);
        assert_eq!(f.node(s).unwrap().next, None, "Stop has no output");
    }

    #[test]
    fn removing_a_block_joins_the_wires_of_both_outputs_and_drops_a_self_wire() {
        let mut f = Flow::new("a", "A");
        let r = f.add_after(None, NodeKind::Repeat(2));
        let body = f.add_after(Some(r), NodeKind::Wait(1));
        // body -> r makes a loop; the branch alt of r leads to End
        assert!(f.connect(body, Port::Next, r));
        // removing the body: r.next would lead to what the body led to, which is r itself
        assert!(f.remove_node(body));
        assert_eq!(f.node(r).unwrap().next, None);
        assert_eq!(f.node(r).unwrap().alt, Some(2));
    }

    #[test]
    fn unplugging_an_input_cuts_every_wire_into_it_and_an_output_only_its_own() {
        let mut f = Flow::new("a", "A");
        let b = f.add_after(None, NodeKind::Branch(Cond::default()));
        assert_eq!(f.node(b).unwrap().wires(), [Some(2), Some(2)]);
        f.unplug_out(b, Port::Alt);
        assert_eq!(f.node(b).unwrap().wires(), [Some(2), None]);
        f.unplug_into(2);
        assert_eq!(f.node(b).unwrap().wires(), [None, None]);
    }

    #[test]
    fn clean_repairs_a_hostile_flow() {
        let node = |id, next, kind| FlowNode {
            id,
            x: f32::NAN,
            y: 1e9,
            kind,
            next,
            alt: None,
        };
        let mut f = Flow {
            id: "x".into(),
            name: "   ".into(),
            nodes: vec![
                node(5, Some(6), NodeKind::Wait(u32::MAX)),
                node(6, Some(5), NodeKind::Command("echo".into())), // a loop with 5
                node(6, None, NodeKind::Start),                     // a repeated id
                node(7, Some(99), NodeKind::Start),                 // a second Start, dangling wire
            ],
        };
        f.clean();
        assert_eq!(f.name, "Flow");
        assert_eq!(
            f.nodes
                .iter()
                .filter(|n| matches!(n.kind, NodeKind::Start))
                .count(),
            1
        );
        assert!(
            f.nodes
                .iter()
                .all(|n| n.x.is_finite() && n.y <= COORD_LIMIT)
        );
        assert!(f.nodes.iter().all(|n| match n.kind {
            NodeKind::Wait(ms) => ms <= MAX_WAIT_MS,
            _ => true,
        }));
        // whatever is left terminates
        assert!(f.chain().len() <= f.nodes.len());
        let ids: BTreeSet<_> = f.nodes.iter().map(|n| n.id).collect();
        assert_eq!(ids.len(), f.nodes.len(), "ids are unique");
    }

    #[test]
    fn a_flow_without_start_gets_one_and_the_block_count_is_capped() {
        let mut f = Flow::new("a", "A");
        f.nodes.clear();
        for i in 0..(MAX_FLOW_NODES as u32 + 20) {
            f.nodes.push(FlowNode {
                id: i + 1,
                x: 0.0,
                y: 0.0,
                kind: NodeKind::Wait(1),
                next: None,
                alt: None,
            });
        }
        f.clean();
        assert!(f.nodes.len() <= MAX_FLOW_NODES);
        assert!(f.start().is_some());
    }

    #[test]
    fn the_list_keeps_unique_ids_and_a_limit() {
        let mut v = vec![
            Flow::new("a", "A"),
            Flow::new("a", "B"),
            Flow::new(" ", "C"),
        ];
        clean_flows(&mut v);
        assert_eq!(v.len(), 1);
        let mut many: Vec<Flow> = (0..MAX_FLOWS + 5)
            .map(|i| Flow::new(&format!("f{i}"), "F"))
            .collect();
        clean_flows(&mut many);
        assert_eq!(many.len(), MAX_FLOWS);
    }

    #[test]
    fn names_and_ids_stay_unique() {
        let v = vec![Flow::new("flow-2", "Copy")];
        assert_eq!(fresh_id(&v), "flow-3");
        assert_eq!(unique_name(&v, "Copy"), "Copy (2)");
        assert_eq!(unique_name(&v, "  "), "Flow");
    }

    #[test]
    fn export_then_import_round_trips_and_gets_a_new_id() {
        let f = sample();
        let text = export_flow(&f).unwrap();
        let back = parse_flow(&text).unwrap();
        assert_eq!(back, f);
        let mut mine = vec![f.clone()];
        let i = add_flow(&mut mine, back).unwrap();
        assert_eq!(i, 1);
        assert_ne!(mine[1].id, mine[0].id);
        assert_eq!(mine[1].name, "Sample (2)");
        assert_eq!(mine[1].steps(), mine[0].steps());
    }

    #[test]
    fn parse_rejects_other_files_and_newer_formats() {
        assert!(parse_flow("nope").is_err());
        assert!(parse_flow(r#"{"cursorpie_profile": 1}"#).is_err());
        assert!(
            parse_flow(r#"{"cursorpie_flow": 99}"#)
                .unwrap_err()
                .contains("newer")
        );
        assert!(
            parse_flow(r#"{"cursorpie_flow": 1, "flow": 3}"#)
                .unwrap_err()
                .contains("damaged")
        );
    }

    #[test]
    fn commands_are_found_even_when_unplugged() {
        let mut f = sample();
        assert!(!f.has_commands() && !f.runs_commands());
        f.nodes.push(FlowNode {
            id: 50,
            x: 0.0,
            y: 0.0,
            kind: NodeKind::Command("rm -rf x".into()),
            next: None,
            alt: None,
        });
        assert!(f.has_commands());
        assert!(!f.runs_commands(), "it is not wired in");
    }

    #[test]
    fn export_file_names_are_safe() {
        assert_eq!(
            export_flow_file_name("../..").as_str(),
            "flow.cursorpie-flow.json"
        );
        assert_eq!(
            export_flow_file_name("Copy / Paste"),
            "Copy---Paste.cursorpie-flow.json"
        );
    }

    #[test]
    fn a_profile_brings_its_flows_and_is_pointed_at_them() {
        let mut mine = vec![Flow::new("flow-1", "Mine")];
        let theirs = Flow::new("flow-1", "Theirs"); // same id, different flow
        let mut items = vec![
            Item::run("A", "A", Action::Flow("flow-1".into())),
            Item::submenu(
                "S",
                "S",
                vec![Item::run("B", "B", Action::Flow("flow-1".into()))],
            ),
        ];
        assert_eq!(flow_ids_used(&items).len(), 1);
        merge_flows(&mut mine, vec![theirs], &mut items).unwrap();
        assert_eq!(mine.len(), 2);
        let new_id = mine[1].id.clone();
        assert_ne!(new_id, "flow-1");
        assert_eq!(
            mine[0].name, "Mine",
            "nothing of the receiver is overwritten"
        );
        let used = flow_ids_used(&items);
        assert_eq!(used.len(), 1);
        assert!(used.contains(&new_id));
    }

    #[test]
    fn an_identical_flow_is_reused() {
        let f = sample();
        let mut mine = vec![f.clone()];
        let mut items = vec![Item::run("A", "A", Action::Flow(f.id.clone()))];
        merge_flows(&mut mine, vec![f.clone()], &mut items).unwrap();
        assert_eq!(mine.len(), 1);
    }

    #[test]
    fn the_new_blocks_round_trip_and_older_flows_keep_their_format_number() {
        let mut f = sample();
        assert_eq!(f.format(), LEGACY_FLOW_FORMAT, "only the first blocks");
        let a = f.add_after(
            None,
            NodeKind::Set {
                name: "n".into(),
                value: "{{n}}1".into(),
            },
        );
        let b = f.add_after(
            Some(a),
            NodeKind::Native {
                op: NativeOp::Notify,
                a: "t".into(),
                b: "x".into(),
            },
        );
        f.add_after(
            Some(b),
            NodeKind::Run {
                command: "echo hi".into(),
                out: "o".into(),
                timeout_s: 5,
            },
        );
        assert_eq!(f.format(), FLOW_FORMAT);
        let text = export_flow(&f).unwrap();
        assert!(text.contains("\"cursorpie_flow\": 2"));
        assert_eq!(parse_flow(&text).unwrap(), f);
    }

    #[test]
    fn a_block_written_without_its_optional_fields_gets_their_defaults() {
        let text = r#"{"cursorpie_flow":2,"flow":{"id":"a","name":"A","nodes":[
            {"id":1,"x":0,"y":0,"kind":"Start","next":2},
            {"id":2,"x":0,"y":0,"kind":{"Run":{"command":"true"}},"next":3},
            {"id":3,"x":0,"y":0,"kind":{"Native":{"op":"Notify"}},"next":4},
            {"id":4,"x":0,"y":0,"kind":{"Branch":{"a":"x"}},"next":5,"alt":5},
            {"id":5,"x":0,"y":0,"kind":"End"}]}}"#;
        let f = parse_flow(text).unwrap();
        assert_eq!(
            f.node(2).unwrap().kind,
            NodeKind::Run {
                command: "true".into(),
                out: String::new(),
                timeout_s: 10
            }
        );
        assert_eq!(f.node(4).unwrap().alt, Some(5));
        assert_eq!(f.reachable().len(), 5);
    }

    #[test]
    fn clean_limits_the_fields_of_the_new_blocks() {
        let mut f = Flow::new("a", "A");
        let long = "x".repeat(MAX_FIELD_CHARS + 50);
        f.add_after(None, NodeKind::Repeat(u32::MAX));
        f.add_after(
            None,
            NodeKind::Run {
                command: long.clone(),
                out: "9bad name!".into(),
                timeout_s: 0,
            },
        );
        f.add_after(
            None,
            NodeKind::Native {
                op: NativeOp::Random,
                a: long,
                b: "n m".into(),
            },
        );
        f.clean();
        for n in &f.nodes {
            match &n.kind {
                NodeKind::Repeat(c) => assert_eq!(*c, MAX_REPEAT),
                NodeKind::Run {
                    command,
                    out,
                    timeout_s,
                } => {
                    assert_eq!(command.chars().count(), MAX_FIELD_CHARS);
                    assert_eq!(out, "badname");
                    assert_eq!(*timeout_s, 1);
                }
                NodeKind::Native { a, b, .. } => {
                    assert_eq!(a.chars().count(), MAX_FIELD_CHARS);
                    assert_eq!(b, "nm");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn wires_out_of_an_output_a_block_does_not_have_are_removed() {
        let mut f = Flow::new("a", "A");
        let w = f.add_after(None, NodeKind::Wait(1));
        f.node_mut(w).unwrap().alt = Some(2);
        f.node_mut(2).unwrap().next = Some(w);
        f.clean();
        assert_eq!(f.node(w).unwrap().alt, None);
        assert_eq!(f.node(2).unwrap().next, None);
    }

    #[test]
    fn variable_names_are_letters_digits_and_underscores_not_starting_with_a_digit() {
        assert!(is_var_name("a"));
        assert!(is_var_name("_x1"));
        assert!(!is_var_name(""));
        assert!(!is_var_name("1a"));
        assert!(!is_var_name("a b"));
        assert!(!is_var_name(&"a".repeat(MAX_VAR_NAME_CHARS + 1)));
        assert_eq!(clean_var_name("  9 my-var! "), "myvar");
        assert!(clean_var_name("é").is_empty());
    }

    #[test]
    fn only_blocks_that_run_commands_or_write_files_are_sensitive_and_unreachable_ones_do_not_run()
    {
        let mut f = sample();
        assert!(!f.has_commands());
        let w = f.add_after(
            None,
            NodeKind::Native {
                op: NativeOp::WriteFile,
                a: "/tmp/x".into(),
                b: "t".into(),
            },
        );
        assert!(f.has_commands() && f.runs_commands());
        f.unplug_into(w);
        assert!(f.has_commands(), "found even when nothing leads to it");
        assert!(!f.runs_commands());
        let read = NodeKind::Native {
            op: NativeOp::ReadFile,
            a: String::new(),
            b: String::new(),
        };
        assert!(!read.is_sensitive());
        assert!(
            NodeKind::Run {
                command: String::new(),
                out: String::new(),
                timeout_s: 1
            }
            .is_sensitive()
        );
    }

    #[test]
    fn only_key_and_type_text_blocks_need_the_app_in_front() {
        let mut f = Flow::new("a", "A");
        assert!(!f.sends_keys());
        f.add_after(None, NodeKind::Wait(1));
        assert!(!f.sends_keys());
        f.add_after(
            None,
            NodeKind::Native {
                op: NativeOp::TypeText,
                a: "x".into(),
                b: String::new(),
            },
        );
        assert!(f.sends_keys());
    }

    #[test]
    fn every_native_operation_has_a_label_and_a_hint_for_the_field_it_uses() {
        for op in NativeOp::ALL {
            assert!(!op.label().is_empty());
            let (a, b) = op.hints();
            assert!(!a.is_empty() || !b.is_empty(), "{op:?} uses no field");
            assert!(!op.a_is_variable() || !a.is_empty());
            assert!(!op.b_is_variable() || !b.is_empty());
        }
        assert_eq!(CmpOp::ALL.len(), 11);
        assert!(CmpOp::ALL.iter().all(|c| !c.label().is_empty()));
        assert!(CalcOp::ALL.iter().all(|c| !c.label().is_empty()));
        assert!(LogicOp::ALL.iter().all(|c| !c.label().is_empty()));
        assert!(AskMode::ALL.iter().all(|c| !c.label().is_empty()));
    }
}
