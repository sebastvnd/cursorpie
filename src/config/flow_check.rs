//! Checks a [`Flow`] for wires and blocks that **cannot work as the person meant**.
//!
//! The data model only keeps a flow *valid* ([`Flow::clean`]: no wire to a block that does not
//! exist, none into *Start*, none from an output a block lacks). It never asked whether what the
//! wires say makes sense, so a wire that would never work looked exactly like one that does. This
//! module asks, without running anything, and answers with a list of [`Issue`]s. The Flow tab
//! draws a wire with an [`Severity::Error`] **red** and one with a [`Severity::Warning`] amber,
//! and marks the block it leads to; every rule is described in
//! [`guides::flow_checks`](crate::guides::flow_checks).
//!
//! ## What is checked
//!
//! *Errors* (red: it will not do what the wire promises):
//!
//! - **`undefined-variable`**: a block reads `{{name}}` (or compares the bare name `name`, or
//!   calculates with it) and no block on any path that leads to it sets that variable. A run
//!   starts with no variables, so the text would stay as it is.
//! - **`loop-never-returns`**: the body of a *Repeat* (2 or more rounds) or a *While* never leads
//!   back to it, so it runs once.
//! - **`endless-cycle`**: a wire closes a cycle in which no *If / else*, *Repeat* or *While* can
//!   leave it; only the limit of [`MAX_FLOW_STEPS`] blocks ends such a run.
//! - **`missing-flow`**: a *Call flow* block names a flow that does not exist.
//! - **`not-a-number`**, **`division-by-zero`**: a *Calculate* block with a value that is not a
//!   number or divides by zero.
//! - **`no-variable-name`**: a block that has to put its result in a variable has no valid name
//!   for it.
//!
//! *Warnings* (amber: probably a mistake, or a risk):
//!
//! - **`not-waited`** and **`shortcut-too-soon`**: a *Shell command* is not waited for before a
//!   block that needs its result; a shortcut comes right after opening an app or a link.
//! - **`same-target`**: both outputs of a decision lead to the same block.
//! - **`dead-wire`**: a wire out of a block that no run reaches.
//! - **`empty-block`**, **`self-call`**, **`empty-loop`**, **`bad-filter`**.
//!
//! Variables are followed along the wires (a block "sets" what it writes; a *Call flow* block may
//! set anything, so after one nothing is reported as undefined). The check is **conservative**: it
//! looks at each wire on its own, and a variable counts as set when *any* path to the block the
//! wire comes out of sets it. So a wire is red only when taking it can never have the variable;
//! a loop that sets a variable at its end and reads it at its start is fine.

use super::flow::{CalcOp, CmpOp, Flow, FlowNode, ListOp, NativeOp, NodeKind, Port, is_var_name};
use super::{MAX_CALL_DEPTH, MAX_FLOW_STEPS};
use crate::flowrun::{is_known_filter, parse_number};
use std::collections::{BTreeMap, BTreeSet};

/// The name that stands for "any variable": a *Call flow* block can set anything.
const ANYTHING: &str = "*";

/// Words that are a value of their own, not the name of a variable somebody forgot to set.
const WORDS: [&str; 6] = ["true", "false", "yes", "no", "on", "off"];

/// How bad a problem is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Probably a mistake, or a risk; the flow may still do what was meant.
    Warning,
    /// It will not do what was meant.
    Error,
}

/// One problem found in a flow.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub severity: Severity,
    /// The block the problem is at: the one a wire leads **to**, or the block itself.
    pub block: u32,
    /// The wire the problem belongs to, as the block and the output it comes out of; `None` for
    /// a problem of the block itself.
    pub wire: Option<(u32, Port)>,
    /// A sentence that says what is wrong, what will happen and what to do.
    pub message: String,
}

fn error(block: u32, wire: Option<(u32, Port)>, message: String) -> Issue {
    Issue {
        severity: Severity::Error,
        block,
        wire,
        message,
    }
}

fn warning(block: u32, wire: Option<(u32, Port)>, message: String) -> Issue {
    Issue {
        severity: Severity::Warning,
        block,
        wire,
        message,
    }
}

/// The worst problem of the wire that comes out of `from` at `port`, if it has any.
pub fn wire_severity(issues: &[Issue], from: u32, port: Port) -> Option<Severity> {
    issues
        .iter()
        .filter(|i| i.wire == Some((from, port)))
        .map(|i| i.severity)
        .max()
}

/// The worst problem that is at block `id` (its own, or of a wire that leads to it), if any.
pub fn block_severity(issues: &[Issue], id: u32) -> Option<Severity> {
    issues
        .iter()
        .filter(|i| i.block == id)
        .map(|i| i.severity)
        .max()
}

/// The problems that are at block `id`, the worst first.
pub fn block_issues(issues: &[Issue], id: u32) -> Vec<&Issue> {
    issues.iter().filter(|i| i.block == id).collect()
}

/// The title the editor gives a block, for the sentences (and for the report of a run).
pub fn label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Start => "Start",
        NodeKind::End => "End",
        NodeKind::Key(_) => "Shortcut",
        NodeKind::OpenApp(_) => "Open app",
        NodeKind::OpenUrl(_) => "Open URL / file",
        NodeKind::Command(_) => "Shell command",
        NodeKind::Wait(_) => "Wait",
        NodeKind::Set { .. } => "Set variable",
        NodeKind::Calc { .. } => "Calculate",
        NodeKind::Logic { .. } => "Logic gate",
        NodeKind::Branch(_) => "If / else",
        NodeKind::Repeat(_) => "Repeat",
        NodeKind::While(_) => "While",
        NodeKind::Ask { .. } => "Ask",
        NodeKind::Run { .. } => "Run and wait",
        NodeKind::Native { .. } => "System",
        NodeKind::List { .. } => "List",
        NodeKind::ForEach { .. } => "For each item",
        NodeKind::Call(_) => "Call flow",
        NodeKind::Stop => "Stop",
    }
}

/// How a block names a block in a sentence: `Block 5 (Run and wait)`.
fn at(n: &FlowNode) -> String {
    format!("Block {} ({})", n.id, label(&n.kind))
}

/// The start of a text, for a sentence.
fn brief(s: &str) -> String {
    const MAX: usize = 24;
    let s = s.trim();
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let cut: String = s.chars().take(MAX).collect();
        format!("{cut}\u{2026}")
    }
}

/// `{{name}}`, as it is written in a field.
fn tag(name: &str) -> String {
    ["{{", name, "}}"].concat()
}

// ------------------------------------------------------------------ variables

/// Adds `name` to `names` when it is a name a variable can have.
fn push_name(names: &mut Vec<String>, name: &str) {
    let t = name.trim();
    if is_var_name(t) {
        names.push(t.to_string());
    }
}

/// The variables a block may set when it runs (what [`crate::flowrun`] writes).
fn writes(kind: &NodeKind) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    match kind {
        NodeKind::Set { name, .. } => push_name(&mut names, name),
        NodeKind::Calc { out, .. } => {
            push_name(&mut names, out);
            push_name(&mut names, "ok");
        }
        NodeKind::Logic { out, .. } => push_name(&mut names, out),
        NodeKind::Repeat(_) | NodeKind::While(_) => push_name(&mut names, "index"),
        NodeKind::Ask {
            mode, message, out, ..
        } => {
            push_name(&mut names, out);
            push_name(&mut names, "ok");
            if *mode == super::flow::AskMode::Panel {
                // every field of the panel sets the variable named by its id
                for id in crate::panel::declared_vars(message) {
                    push_name(&mut names, &id);
                }
            }
        }
        NodeKind::Run { out, .. } => {
            push_name(&mut names, out);
            push_name(&mut names, "ok");
            push_name(&mut names, "code");
        }
        NodeKind::Native { op, a, b } => {
            push_name(&mut names, "ok");
            if op.a_is_variable() {
                push_name(&mut names, a);
            }
            if op.b_is_variable() {
                push_name(&mut names, b);
            }
        }
        NodeKind::List { op, list, out, .. } => {
            push_name(&mut names, "ok");
            if !matches!(
                op,
                ListOp::Get | ListOp::Length | ListOp::Join | ListOp::IndexOf | ListOp::Sum
            ) {
                push_name(&mut names, list);
            }
            if !op.hints().2.is_empty() {
                push_name(&mut names, out);
            }
        }
        NodeKind::ForEach { item, .. } => {
            push_name(&mut names, item);
            push_name(&mut names, "index");
            push_name(&mut names, "ok");
        }
        // the called flow shares the variables and may set any of them
        NodeKind::Call(_) => names.push(ANYTHING.to_string()),
        _ => {}
    }
    names
}

/// How a block reads a variable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum How {
    /// As `{{name}}` in a text.
    Text,
    /// As the bare name on the left of a condition.
    Compare,
    /// As the bare name of a value to calculate with.
    Number,
    /// As the name of the variable that holds a list.
    List,
}

/// Collects the names a text reads (`{{name}}`, `{{name|sh}}`) and the ones with a filter that
/// does not exist (those are left as written, like [`crate::flowrun::expand`] does).
fn scan_text(text: &str, reads: &mut Vec<(String, How)>, bad: &mut Vec<String>) {
    let mut rest = text;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            break;
        };
        let inner = &after[..close];
        let mut parts = inner.split('|');
        let name = parts.next().unwrap_or("").trim();
        let filters: Vec<&str> = parts.collect();
        if is_var_name(name) {
            if let Some(f) = filters.iter().find(|f| !is_known_filter(f)) {
                bad.push(f.trim().to_string());
            } else if filters
                .iter()
                .any(|f| f.trim_start().starts_with("default:"))
            {
                // a default stands in for a variable that was never set: nothing is read
            } else {
                reads.push((name.to_string(), How::Text));
            }
        }
        rest = &after[close + 2..];
    }
}

/// A field that is a value or the bare name of a variable (an operand).
fn scan_operand(s: &str, how: How, reads: &mut Vec<(String, How)>, bad: &mut Vec<String>) {
    scan_text(s, reads, bad);
    let t = s.trim();
    if !s.contains("{{") && is_var_name(t) && !WORDS.contains(&t.to_ascii_lowercase().as_str()) {
        reads.push((t.to_string(), how));
    }
}

/// What a block reads: the variables, and the texts with a filter that does not exist.
fn reads(kind: &NodeKind) -> (Vec<(String, How)>, Vec<String>) {
    let mut found: Vec<(String, How)> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    match kind {
        NodeKind::OpenApp(s) | NodeKind::OpenUrl(s) | NodeKind::Command(s) | NodeKind::Call(s) => {
            scan_text(s, &mut found, &mut bad);
        }
        NodeKind::Set { value, .. } => scan_text(value, &mut found, &mut bad),
        NodeKind::Ask {
            message, preset, ..
        } => {
            scan_text(message, &mut found, &mut bad);
            scan_text(preset, &mut found, &mut bad);
        }
        NodeKind::Run { command, .. } => scan_text(command, &mut found, &mut bad),
        NodeKind::Native { op, a, b } => {
            if !op.a_is_variable() {
                scan_text(a, &mut found, &mut bad);
            }
            if !op.b_is_variable() {
                scan_text(b, &mut found, &mut bad);
            }
        }
        NodeKind::List { op, list, a, b, .. } => {
            let (ha, hb, _) = op.hints();
            if *op != ListOp::Split && is_var_name(list.trim()) {
                found.push((list.trim().to_string(), How::List));
            }
            if !ha.is_empty() {
                scan_text(a, &mut found, &mut bad);
            }
            if !hb.is_empty() {
                scan_text(b, &mut found, &mut bad);
            }
        }
        NodeKind::ForEach { list, .. } => {
            if is_var_name(list.trim()) {
                found.push((list.trim().to_string(), How::List));
            }
        }
        NodeKind::Calc { a, b, .. } => {
            scan_operand(a, How::Number, &mut found, &mut bad);
            scan_operand(b, How::Number, &mut found, &mut bad);
        }
        NodeKind::Logic { a, b, .. } => {
            scan_text(a, &mut found, &mut bad);
            scan_text(b, &mut found, &mut bad);
        }
        NodeKind::Branch(c) | NodeKind::While(c) => {
            scan_operand(&c.a, How::Compare, &mut found, &mut bad);
            if !matches!(c.op, CmpOp::IsTrue | CmpOp::IsEmpty) {
                scan_text(&c.b, &mut found, &mut bad);
            }
        }
        _ => {}
    }
    (found, bad)
}

/// For every block a run can reach: the variables that may be set once it is done, by it or by
/// any block before it. Found by repeating until nothing grows.
fn defined_after(
    live: &[&FlowNode],
    preds: &BTreeMap<u32, Vec<(u32, Port)>>,
) -> BTreeMap<u32, BTreeSet<String>> {
    let mut after: BTreeMap<u32, BTreeSet<String>> =
        live.iter().map(|n| (n.id, BTreeSet::new())).collect();
    loop {
        let mut changed = false;
        for n in live {
            let mut set: BTreeSet<String> = BTreeSet::new();
            if let Some(ps) = preds.get(&n.id) {
                for (p, _) in ps {
                    if let Some(s) = after.get(p) {
                        set.extend(s.iter().cloned());
                    }
                }
            }
            set.extend(writes(&n.kind));
            if after.get(&n.id) != Some(&set) {
                after.insert(n.id, set);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    after
}

// ------------------------------------------------------- following a variable (for the editor)

/// One variable of a flow: the blocks that set it and the blocks that read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarRow {
    pub name: String,
    /// Blocks that set it, in the order of the flow's list of blocks.
    pub writers: Vec<u32>,
    /// Blocks that read it.
    pub readers: Vec<u32>,
    /// The readers a run can reach that no path sets the variable for (they read nothing).
    pub unset: Vec<u32>,
}

fn dedup(mut v: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    v.retain(|x| seen.insert(x.clone()));
    v
}

/// The variables a block reads, each once, in the order they are written in the block.
pub fn block_reads(kind: &NodeKind) -> Vec<String> {
    dedup(reads(kind).0.into_iter().map(|(n, _)| n).collect())
}

/// The variables a block sets, each once. A *Call flow* block may set any: see [`call_may_set`].
pub fn block_writes(kind: &NodeKind) -> Vec<String> {
    dedup(writes(kind).into_iter().filter(|n| n != ANYTHING).collect())
}

/// Whether the block runs another flow, which shares the variables and may set any of them.
pub fn call_may_set(kind: &NodeKind) -> bool {
    matches!(kind, NodeKind::Call(_))
}

/// For every block a run can reach: the variables that may be set when it **starts** (by any
/// block before it); `*` stands for "anything" after a *Call flow* block. Blocks no run reaches
/// have no entry.
fn defined_before(flow: &Flow) -> BTreeMap<u32, BTreeSet<String>> {
    let live = flow.reachable();
    let live_ids: BTreeSet<u32> = live.iter().map(|n| n.id).collect();
    let mut preds: BTreeMap<u32, Vec<(u32, Port)>> = BTreeMap::new();
    for n in &live {
        for &port in n.kind.outputs() {
            if let Some(t) = n.wire(port)
                && live_ids.contains(&t)
            {
                preds.entry(t).or_default().push((n.id, port));
            }
        }
    }
    let after = defined_after(&live, &preds);
    live.iter()
        .map(|n| {
            let mut set = BTreeSet::new();
            for (p, _) in preds.get(&n.id).map(Vec::as_slice).unwrap_or(&[]) {
                if let Some(a) = after.get(p) {
                    set.extend(a.iter().cloned());
                }
            }
            (n.id, set)
        })
        .collect()
}

/// Whether `name` may already be set when `block` starts. `None` when no run reaches the block.
pub fn is_set_before(flow: &Flow, block: u32, name: &str) -> Option<bool> {
    defined_before(flow)
        .get(&block)
        .map(|d| d.contains(ANYTHING) || d.contains(name))
}

/// Every variable of the flow with who sets it and who reads it, sorted by name.
pub fn variables(flow: &Flow) -> Vec<VarRow> {
    let before = defined_before(flow);
    let mut rows: BTreeMap<String, VarRow> = BTreeMap::new();
    for n in &flow.nodes {
        for name in block_writes(&n.kind) {
            rows.entry(name.clone())
                .or_insert_with(|| VarRow {
                    name,
                    writers: Vec::new(),
                    readers: Vec::new(),
                    unset: Vec::new(),
                })
                .writers
                .push(n.id);
        }
    }
    for n in &flow.nodes {
        for name in block_reads(&n.kind) {
            let r = rows.entry(name.clone()).or_insert_with(|| VarRow {
                name: name.clone(),
                writers: Vec::new(),
                readers: Vec::new(),
                unset: Vec::new(),
            });
            r.readers.push(n.id);
            if let Some(d) = before.get(&n.id)
                && !(d.contains(ANYTHING) || d.contains(&name))
            {
                r.unset.push(n.id);
            }
        }
    }
    rows.into_values().collect()
}

/// How a value of `name` travels: pairs `(setter, reader)` where the reader can get the value the
/// setter put there, because a wire path leads from one to the other without another block that
/// sets the same variable in between. A block that reads and sets it (`x = x + 1`) takes the
/// value first and then ends that path.
pub fn data_edges(flow: &Flow, name: &str) -> Vec<(u32, u32)> {
    let sets = |id: u32| {
        flow.node(id)
            .is_some_and(|n| block_writes(&n.kind).iter().any(|w| w == name))
    };
    let gets = |id: u32| {
        flow.node(id)
            .is_some_and(|n| block_reads(&n.kind).iter().any(|r| r == name))
    };
    let mut out: Vec<(u32, u32)> = Vec::new();
    for w in flow.nodes.iter().map(|n| n.id).filter(|id| sets(*id)) {
        let mut seen: BTreeSet<u32> = BTreeSet::new();
        let mut stack: Vec<u32> = flow
            .node(w)
            .map(|n| n.wires().into_iter().flatten().collect())
            .unwrap_or_default();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            if gets(id) {
                out.push((w, id));
            }
            if sets(id) {
                continue;
            }
            if let Some(n) = flow.node(id) {
                stack.extend(n.wires().into_iter().flatten());
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Every block a walk from `from` can get to, `from` included.
fn descendants(flow: &Flow, from: u32) -> BTreeSet<u32> {
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut stack = vec![from];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(n) = flow.node(id) {
            stack.extend(n.wires().into_iter().flatten());
        }
    }
    seen
}

/// What is wrong with a value that a *Calculate* block reads as a number, when that can be told
/// without running it.
fn number_problem(s: &str) -> Option<String> {
    if s.contains("{{") {
        return None;
    }
    let t = s.trim();
    if t.is_empty() {
        return Some("is empty".to_string());
    }
    if parse_number(t).is_some() || is_var_name(t) {
        return None;
    }
    Some(format!("`{}` is not a number", brief(t)))
}

/// What is wrong with a number typed in a field that only expands `{{variables}}` (a position, a
/// volume): a bare word is not a variable there. `None` when it is a number or has a `{{...}}`.
fn number_problem_text(s: &str) -> Option<String> {
    if s.contains("{{") {
        return None;
    }
    let t = s.trim();
    if t.is_empty() {
        return Some("is empty".to_string());
    }
    if parse_number(t).is_some() {
        return None;
    }
    Some(format!(
        "`{}` is not a number (a name does not work here: write {} to use a variable)",
        brief(t),
        tag("name")
    ))
}

// ------------------------------------------------------------------ the check

/// Checks `flow`. `known_flows` are the ids of the flows that exist (for *Call flow* blocks). The
/// list has the errors first, and no problem twice. Cheap enough to run on every frame of the
/// editor: a flow has at most [`MAX_FLOW_NODES`](super::MAX_FLOW_NODES) blocks.
pub fn check_flow(flow: &Flow, known_flows: &[&str]) -> Vec<Issue> {
    let mut found: Vec<Issue> = Vec::new();
    let live = flow.reachable();
    let live_ids: BTreeSet<u32> = live.iter().map(|n| n.id).collect();
    let by_id: BTreeMap<u32, &FlowNode> = flow.nodes.iter().map(|n| (n.id, n)).collect();

    // who leads to a block, and by which output
    let mut preds: BTreeMap<u32, Vec<(u32, Port)>> = BTreeMap::new();
    for n in &live {
        for &port in n.kind.outputs() {
            if let Some(t) = n.wire(port)
                && live_ids.contains(&t)
            {
                preds.entry(t).or_default().push((n.id, port));
            }
        }
    }
    let after = defined_after(&live, &preds);
    let desc: BTreeMap<u32, BTreeSet<u32>> = live
        .iter()
        .map(|n| (n.id, descendants(flow, n.id)))
        .collect();

    for n in &live {
        let me = at(n);

        // 1. what the block reads must have been set on the way here
        let (wanted, bad_filters) = reads(&n.kind);
        for f in &bad_filters {
            found.push(warning(
                n.id,
                None,
                format!(
                    "{me} uses the filter `{f}`, which does not exist (or cannot take that argument), \
                     so the whole {{{{...}}}} is left as it is written. See the list of filters in \
                     the guide."
                ),
            ));
        }
        for &(p, port) in preds.get(&n.id).map(Vec::as_slice).unwrap_or(&[]) {
            let defined = after.get(&p);
            for (name, how) in &wanted {
                let known = defined.is_some_and(|d| d.contains(ANYTHING) || d.contains(name));
                if known {
                    continue;
                }
                let message = match how {
                    How::Text => format!(
                        "{me} uses {}, but no block before it on this path sets `{name}`, so the \
                         text stays as it is. Add a Set variable (or an Ask, Run or System block \
                         that puts its result in `{name}`) before it.",
                        tag(name)
                    ),
                    How::Compare => format!(
                        "{me} looks at `{name}`, but no block before it on this path sets a \
                         variable with that name, so the word itself is compared and the answer \
                         never changes. Set `{name}` before this block.",
                    ),
                    How::Number => format!(
                        "{me} calculates with `{name}`, which is not a number and not a \
                         variable that was set before it, so the result stays empty and `ok` is 0."
                    ),
                    How::List => format!(
                        "{me} works on the list in `{name}`, but no block before it on this path \
                         sets that variable, so it is empty. Make the list first with a List block \
                         (Make from text), or a System block that returns one (List a folder)."
                    ),
                };
                found.push(error(n.id, Some((p, port)), message));
            }
        }

        // 2. what is wrong with the block itself
        match &n.kind {
            NodeKind::Set { name, .. } if !is_var_name(name.trim()) => {
                found.push(error(
                    n.id,
                    None,
                    format!(
                        "{me} has no valid variable name, so it sets nothing. A name uses \
                         letters, digits and _, and does not start with a digit."
                    ),
                ));
            }
            NodeKind::Logic { out, .. } if !is_var_name(out.trim()) => {
                found.push(error(
                    n.id,
                    None,
                    format!("{me} has no valid name for its result, so the result is lost."),
                ));
            }
            NodeKind::Calc { out, a, op, b } => {
                if !is_var_name(out.trim()) {
                    found.push(error(
                        n.id,
                        None,
                        format!("{me} has no valid name for its result, so the result is lost."),
                    ));
                }
                for (which, s) in [("first", a), ("second", b)] {
                    if let Some(why) = number_problem(s) {
                        found.push(error(
                            n.id,
                            None,
                            format!(
                                "{me}: its {which} value {why}, so the result stays empty and \
                                 `ok` is 0."
                            ),
                        ));
                    }
                }
                if matches!(op, CalcOp::Div | CalcOp::Rem) && parse_number(b) == Some(0.0) {
                    found.push(error(
                        n.id,
                        None,
                        format!("{me} divides by zero, so the result stays empty and `ok` is 0."),
                    ));
                }
            }
            NodeKind::Ask { out, .. } if !out.trim().is_empty() && !is_var_name(out.trim()) => {
                found.push(warning(
                    n.id,
                    None,
                    format!("{me} has no valid name for the answer, so the answer is lost."),
                ));
            }
            NodeKind::Run { command, out, .. } => {
                if command.trim().is_empty() {
                    found.push(warning(
                        n.id,
                        None,
                        format!("{me} is empty, so it does nothing."),
                    ));
                }
                if !out.trim().is_empty() && !is_var_name(out.trim()) {
                    found.push(warning(
                        n.id,
                        None,
                        format!("{me} has no valid name for what it prints, so that is lost."),
                    ));
                }
            }
            NodeKind::OpenApp(s) | NodeKind::OpenUrl(s) | NodeKind::Command(s)
                if s.trim().is_empty() =>
            {
                found.push(warning(
                    n.id,
                    None,
                    format!("{me} is empty, so it does nothing."),
                ));
            }
            NodeKind::Native { op, a, b } => {
                // the reply of the browser is the one result a block may leave without a place
                if op.b_is_variable() && *op != NativeOp::Browser && !is_var_name(b.trim()) {
                    found.push(error(
                        n.id,
                        None,
                        format!(
                            "{me} needs the name of a variable in its second field to put the \
                             result in; without one the result is lost."
                        ),
                    ));
                }
                if op.a_is_variable() && !is_var_name(a.trim()) {
                    found.push(error(
                        n.id,
                        None,
                        format!(
                            "{me} needs the name of a variable in its first field to put the \
                             result in; without one the result is lost."
                        ),
                    ));
                }
                if *op == NativeOp::Browser && !a.contains("{{") {
                    // a command that is written out is checked now; one with a variable in it can
                    // only be checked when the flow runs
                    if let Err(why) = crate::bridge::commands::parse(a.trim()) {
                        found.push(error(
                            n.id,
                            None,
                            format!("{me}: the browser cannot run `{}`: {why}.", brief(a.trim())),
                        ));
                    }
                }
                if matches!(op, NativeOp::OpenAppSmart | NativeOp::OpenUrlSmart)
                    && !b.contains("{{")
                    && crate::flowrun::OpenMode::parse(b).is_none()
                {
                    found.push(error(
                        n.id,
                        None,
                        format!(
                            "{me}: `{}` is not a way to open it, so nothing is opened. Write \
                             smart, existing, new or plain (or leave it empty for smart).",
                            brief(b.trim())
                        ),
                    ));
                }
                if *op == NativeOp::VolumeSet && !a.contains("{{") {
                    let t = a.trim();
                    if !parse_number(t).is_some_and(|v| (0.0..=100.0).contains(&v)) {
                        found.push(error(
                            n.id,
                            None,
                            format!(
                                "{me}: the volume `{}` is not a number from 0 to 100, so the \
                                 volume is not changed.",
                                brief(t)
                            ),
                        ));
                    }
                }
            }
            NodeKind::Call(id) => {
                let t = id.trim();
                if t.is_empty() {
                    found.push(error(
                        n.id,
                        None,
                        format!("{me} does not say which flow to call, so nothing is called."),
                    ));
                } else if !t.contains("{{") {
                    if t == flow.id {
                        found.push(warning(
                            n.id,
                            None,
                            format!(
                                "{me} calls the flow it is in. It stops {MAX_CALL_DEPTH} calls \
                                 deep, so it is rarely what you want."
                            ),
                        ));
                    } else if !known_flows.contains(&t) {
                        found.push(error(
                            n.id,
                            None,
                            format!(
                                "{me} calls a flow that does not exist (it was deleted, or the \
                                 file came from another computer), so nothing is called. Pick \
                                 another flow."
                            ),
                        ));
                    }
                }
            }
            NodeKind::List { op, list, a, .. } => {
                if !is_var_name(list.trim()) {
                    found.push(error(
                        n.id,
                        None,
                        format!(
                            "{me} has no valid name for the variable that holds the list, so it \
                             works on nothing."
                        ),
                    ));
                }
                let (ha, _, hout) = op.hints();
                if matches!(op, ListOp::Get | ListOp::SetItem | ListOp::Remove)
                    && let Some(why) = number_problem_text(a)
                {
                    found.push(error(
                        n.id,
                        None,
                        format!("{me}: its position {why}, so it does nothing and `ok` is 0."),
                    ));
                }
                let _ = ha;
                if !hout.is_empty()
                    && let NodeKind::List { out, .. } = &n.kind
                    && !is_var_name(out.trim())
                {
                    found.push(error(
                        n.id,
                        None,
                        format!("{me} has no valid name for its result, so the result is lost."),
                    ));
                }
            }
            NodeKind::ForEach { list, item } => {
                if !is_var_name(list.trim()) || !is_var_name(item.trim()) {
                    found.push(error(
                        n.id,
                        None,
                        format!(
                            "{me} needs the name of the variable that holds the list and the \
                             name of the one that gets each item, so as it is it goes through \
                             nothing."
                        ),
                    ));
                }
            }
            NodeKind::Repeat(0) => {
                found.push(warning(
                    n.id,
                    None,
                    format!("{me} repeats 0 times, so its body never runs."),
                ));
            }
            _ => {}
        }

        // 3. loops must come back, and a cycle must have a way out
        let loops = matches!(n.kind, NodeKind::Repeat(c) if c >= 2)
            || matches!(n.kind, NodeKind::While(_) | NodeKind::ForEach { .. });
        if loops {
            match n.next {
                None => found.push(warning(
                    n.id,
                    None,
                    format!(
                        "The body output of {me} has no wire, so the run goes on to `done` after \
                         the first round."
                    ),
                )),
                Some(body) => {
                    if let Some(d) = desc.get(&body)
                        && !d.contains(&n.id)
                    {
                        found.push(error(
                            n.id,
                            Some((n.id, Port::Next)),
                            format!(
                                "The body of {me} never leads back to it, so it runs only once. \
                                 Wire the last block of the body to the left dot of this block."
                            ),
                        ));
                    }
                }
            }
        }
        for &port in n.kind.outputs() {
            let Some(v) = n.wire(port) else { continue };
            // the wire lies on a cycle when its target can get back to this block
            if let Some(dv) = desc.get(&v)
                && dv.contains(&n.id)
            {
                let can_leave = dv.iter().any(|w| {
                    desc.get(w).is_some_and(|dw| dw.contains(&n.id))
                        && by_id.get(w).is_some_and(|m| {
                            matches!(
                                m.kind,
                                NodeKind::Branch(_)
                                    | NodeKind::Repeat(_)
                                    | NodeKind::While(_)
                                    | NodeKind::ForEach { .. }
                            )
                        })
                });
                if !can_leave {
                    found.push(error(
                        v,
                        Some((n.id, port)),
                        format!(
                            "This wire closes a loop that nothing can leave: no If / else, \
                             Repeat or While is on it, so it would run until the limit of \
                             {MAX_FLOW_STEPS} blocks stops it. Use a Repeat or a While."
                        ),
                    ));
                }
            }
        }

        // 4. wires that are suspicious
        if n.kind.outputs().len() == 2
            && n.next.is_some()
            && n.next == n.alt
            && let Some(t) = n.alt
        {
            found.push(warning(
                t,
                Some((n.id, Port::Alt)),
                format!(
                    "Both outputs of {me} lead to the same block, so the decision changes \
                     nothing."
                ),
            ));
        }
        if let Some(t) = n.next
            && let Some(m) = by_id.get(&t)
        {
            let needs_result = matches!(
                m.kind,
                NodeKind::Key(_)
                    | NodeKind::Native {
                        op: super::flow::NativeOp::ClipboardGet
                            | super::flow::NativeOp::ReadFile
                            | super::flow::NativeOp::FileExists,
                        ..
                    }
            );
            if matches!(n.kind, NodeKind::Command(_)) && needs_result {
                found.push(warning(
                    t,
                    Some((n.id, Port::Next)),
                    format!(
                        "{me} is not waited for: the next block starts 40 ms later, before the \
                         command is done. Use a Run and wait block, or add a Wait."
                    ),
                ));
            }
            if matches!(n.kind, NodeKind::OpenApp(_) | NodeKind::OpenUrl(_))
                && matches!(m.kind, NodeKind::Key(_))
            {
                found.push(warning(
                    t,
                    Some((n.id, Port::Next)),
                    format!(
                        "{me} opens something and the next block presses a shortcut at once. \
                         Add a Wait (300 to 800 ms) between them, or the shortcut goes to the app \
                         that was in front."
                    ),
                ));
            }
        }
    }

    // 5. wires out of blocks no run reaches are never used
    for n in &flow.nodes {
        if live_ids.contains(&n.id) || matches!(n.kind, NodeKind::Start) {
            continue;
        }
        for &port in n.kind.outputs() {
            if let Some(t) = n.wire(port) {
                found.push(warning(
                    t,
                    Some((n.id, port)),
                    format!(
                        "{} is not reached from Start, so this wire is never used.",
                        at(n)
                    ),
                ));
            }
        }
    }

    // the errors first, then by block; the same problem only once
    found.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| (a.block, a.wire, &a.message).cmp(&(b.block, b.wire, &b.message)))
    });
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::flow::{Cond, NativeOp};

    fn flow_with(kinds: Vec<NodeKind>) -> (Flow, Vec<u32>) {
        let mut f = Flow::new("f1", "Demo");
        let mut last = None;
        let mut ids = Vec::new();
        for k in kinds {
            let id = f.add_after(last, k);
            ids.push(id);
            last = Some(id);
        }
        (f, ids)
    }

    fn set(name: &str, value: &str) -> NodeKind {
        NodeKind::Set {
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn the_variables_of_a_flow_say_who_sets_them_and_who_reads_them() {
        let (f, ids) = flow_with(vec![
            set("a", "1"),
            set("b", "{{a}}"),
            NodeKind::Command("echo {{b}} {{c}}".into()),
        ]);
        let rows = variables(&f);
        let row = |n: &str| rows.iter().find(|r| r.name == n).unwrap().clone();
        assert_eq!(row("a").writers, vec![ids[0]]);
        assert_eq!(row("a").readers, vec![ids[1]]);
        assert_eq!(row("b").readers, vec![ids[2]]);
        // `c` is read and nobody sets it
        assert!(row("c").writers.is_empty());
        assert_eq!(row("c").unset, vec![ids[2]]);
        assert!(row("a").unset.is_empty());
        assert_eq!(is_set_before(&f, ids[2], "b"), Some(true));
        assert_eq!(is_set_before(&f, ids[2], "c"), Some(false));
        assert_eq!(is_set_before(&f, ids[0], "a"), Some(false));
        assert_eq!(block_reads(&f.node(ids[2]).unwrap().kind), vec!["b", "c"]);
        assert_eq!(block_writes(&f.node(ids[1]).unwrap().kind), vec!["b"]);
    }

    #[test]
    fn a_value_travels_to_the_readers_until_another_block_sets_the_variable_again() {
        let (f, ids) = flow_with(vec![
            set("x", "1"),
            NodeKind::Command("echo {{x}}".into()),
            set("x", "2"),
            NodeKind::Command("echo {{x}}".into()),
        ]);
        let e = data_edges(&f, "x");
        assert_eq!(e, vec![(ids[0], ids[1]), (ids[2], ids[3])]);
        // a block that reads and sets the variable takes the value and ends that path
        let (g, k) = flow_with(vec![
            set("n", "0"),
            set("n", "{{n}}1"),
            NodeKind::Command("echo {{n}}".into()),
        ]);
        assert_eq!(data_edges(&g, "n"), vec![(k[0], k[1]), (k[1], k[2])]);
        // a block no wire leads to still shows in the list, but is not set before anything
        let mut h = f.clone();
        let lone = h.add_at(NodeKind::Command("echo {{x}}".into()), 900.0, 900.0);
        assert_eq!(is_set_before(&h, lone, "x"), None);
    }

    #[test]
    fn a_new_flow_and_a_plain_chain_have_no_problem() {
        assert!(check_flow(&Flow::new("f1", "Demo"), &["f1"]).is_empty());
        let (f, _) = flow_with(vec![
            NodeKind::OpenApp("com.apple.Safari".into()),
            NodeKind::Wait(500),
            NodeKind::Command("echo hi".into()),
        ]);
        assert!(
            check_flow(&f, &["f1"]).is_empty(),
            "{:?}",
            check_flow(&f, &["f1"])
        );
    }

    #[test]
    fn a_variable_nobody_set_makes_the_wire_red() {
        let (f, ids) = flow_with(vec![set("a", "1"), NodeKind::Command("echo {{b}}".into())]);
        let issues = check_flow(&f, &["f1"]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].block, ids[1]);
        assert_eq!(issues[0].wire, Some((ids[0], Port::Next)));
        assert!(issues[0].message.contains("{{b}}"), "{}", issues[0].message);
        assert_eq!(
            wire_severity(&issues, ids[0], Port::Next),
            Some(Severity::Error)
        );
        assert_eq!(block_severity(&issues, ids[1]), Some(Severity::Error));
        assert_eq!(block_severity(&issues, ids[0]), None);

        // set before it, and the wire is fine
        let (f, _) = flow_with(vec![set("b", "1"), NodeKind::Command("echo {{b}}".into())]);
        assert!(check_flow(&f, &["f1"]).is_empty());
        // `{{b|sh}}` reads it too
        let (f, _) = flow_with(vec![NodeKind::Command("echo {{b|sh}}".into())]);
        assert_eq!(check_flow(&f, &["f1"]).len(), 1);
    }

    #[test]
    fn what_a_block_writes_counts_as_set() {
        let (f, _) = flow_with(vec![
            NodeKind::Run {
                command: "ls".into(),
                out: "files".into(),
                timeout_s: 5,
            },
            NodeKind::Command("echo {{files}} {{code}} {{ok}}".into()),
        ]);
        assert!(
            check_flow(&f, &["f1"]).is_empty(),
            "{:?}",
            check_flow(&f, &["f1"])
        );
        // a called flow may set anything
        let (f, _) = flow_with(vec![
            NodeKind::Call("other".into()),
            NodeKind::Command("echo {{whatever}}".into()),
        ]);
        let issues = check_flow(&f, &["f1", "other"]);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn a_variable_counts_as_set_when_any_path_to_the_wire_sets_it() {
        // If / else: `then` sets x on the way to the command, `else` goes straight there
        let mut f = Flow::new("f1", "Demo");
        let b = f.add_after(
            None,
            NodeKind::Branch(Cond {
                a: "1".into(),
                op: CmpOp::IsTrue,
                b: String::new(),
            }),
        );
        let s = f.add_at(set("x", "1"), 300.0, 300.0);
        let c = f.add_at(NodeKind::Command("echo {{x}}".into()), 600.0, 300.0);
        assert!(f.connect(b, Port::Next, s));
        assert!(f.connect(s, Port::Next, c));
        assert!(f.connect(b, Port::Alt, c));
        let issues = check_flow(&f, &["f1"]);
        // only the wire that skips the Set is red
        assert_eq!(wire_severity(&issues, s, Port::Next), None, "{issues:?}");
        assert_eq!(
            wire_severity(&issues, b, Port::Alt),
            Some(Severity::Error),
            "{issues:?}"
        );

        // a loop that sets a variable at its end and reads it at its start is fine
        let mut f = Flow::new("f1", "Demo");
        let r = f.add_after(None, NodeKind::Repeat(3));
        let u = f.add_at(NodeKind::Command("echo {{total}}".into()), 300.0, 300.0);
        let t = f.add_at(set("total", "1"), 600.0, 300.0);
        assert!(f.connect(r, Port::Next, u));
        assert!(f.connect(u, Port::Next, t));
        assert!(f.connect(t, Port::Next, r));
        let issues = check_flow(&f, &["f1"]);
        // `total` is set at the end of the body, which leads back to the loop: no wire is red
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn a_comparison_of_a_name_that_was_never_set_is_an_error() {
        let (mut f, ids) = flow_with(vec![NodeKind::Branch(Cond {
            a: "go".into(),
            op: CmpOp::IsTrue,
            b: String::new(),
        })]);
        // a new decision has both outputs on the block that follows, which is a warning of its
        // own (`same-target`); this test is about the errors
        let errors = |f: &Flow| {
            check_flow(f, &["f1"])
                .into_iter()
                .filter(|i| i.severity == Severity::Error)
                .collect::<Vec<_>>()
        };
        let issues = errors(&f);
        assert_eq!(issues.len(), 1, "{issues:?}");
        // a word that is a value of its own is not a forgotten variable
        if let Some(n) = f.node_mut(ids[0])
            && let NodeKind::Branch(c) = &mut n.kind
        {
            c.a = "yes".into();
        }
        let issues = errors(&f);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn a_loop_that_cannot_be_left_is_an_error_and_one_with_a_branch_is_not() {
        let (mut f, ids) = flow_with(vec![NodeKind::Wait(10), NodeKind::Wait(20)]);
        assert!(f.connect(ids[1], Port::Next, ids[0]));
        let issues = check_flow(&f, &["f1"]);
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Error && i.wire == Some((ids[1], Port::Next))),
            "{issues:?}"
        );

        // the same ring with an If / else in it can be left
        let (mut f, ids) = flow_with(vec![
            NodeKind::Wait(10),
            NodeKind::Branch(Cond {
                a: "1".into(),
                op: CmpOp::IsTrue,
                b: String::new(),
            }),
        ]);
        assert!(f.connect(ids[1], Port::Next, ids[0]));
        let issues = check_flow(&f, &["f1"]);
        assert!(
            issues.iter().all(|i| i.severity != Severity::Error),
            "{issues:?}"
        );
    }

    #[test]
    fn the_body_of_a_repeat_has_to_come_back() {
        let mut f = Flow::new("f1", "Demo");
        let r = f.add_after(None, NodeKind::Repeat(3));
        let c = f.add_at(NodeKind::Command("echo hi".into()), 300.0, 300.0);
        assert!(f.connect(r, Port::Next, c));
        let issues = check_flow(&f, &["f1"]);
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Error && i.wire == Some((r, Port::Next))),
            "{issues:?}"
        );
        // wired back, it is fine
        assert!(f.connect(c, Port::Next, r));
        let issues = check_flow(&f, &["f1"]);
        assert!(
            issues.iter().all(|i| i.severity != Severity::Error),
            "{issues:?}"
        );
        // once is not a loop
        if let Some(n) = f.node_mut(r) {
            n.kind = NodeKind::Repeat(1);
        }
        assert!(f.connect(c, Port::Next, f.end().map(|e| e.id).unwrap_or(0)));
        assert!(
            check_flow(&f, &["f1"])
                .iter()
                .all(|i| i.severity != Severity::Error)
        );
    }

    #[test]
    fn a_call_to_a_flow_that_does_not_exist_is_an_error() {
        let (f, ids) = flow_with(vec![NodeKind::Call("gone".into())]);
        let issues = check_flow(&f, &["f1"]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(issues[0].block, ids[0]);
        assert!(check_flow(&f, &["f1", "gone"]).is_empty());
        // itself: only a warning
        let (f, _) = flow_with(vec![NodeKind::Call("f1".into())]);
        let issues = check_flow(&f, &["f1"]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
    }

    #[test]
    fn calculate_needs_numbers_and_no_zero_divisor() {
        let calc = |a: &str, op: CalcOp, b: &str| NodeKind::Calc {
            out: "r".into(),
            a: a.into(),
            op,
            b: b.into(),
        };
        let (f, _) = flow_with(vec![calc("2", CalcOp::Add, "3")]);
        assert!(check_flow(&f, &["f1"]).is_empty());
        let (f, _) = flow_with(vec![calc("abc", CalcOp::Add, "3")]);
        assert_eq!(check_flow(&f, &["f1"]).len(), 1);
        let (f, _) = flow_with(vec![calc("2", CalcOp::Div, "0")]);
        assert_eq!(check_flow(&f, &["f1"]).len(), 1);
        let (f, _) = flow_with(vec![calc("2", CalcOp::Add, "")]);
        assert_eq!(check_flow(&f, &["f1"]).len(), 1);
        // the name of a variable that nobody set
        let (f, _) = flow_with(vec![calc("n", CalcOp::Add, "1")]);
        let issues = check_flow(&f, &["f1"]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].wire.is_some());
    }

    #[test]
    fn warnings_do_not_stop_a_wire_from_being_worth_a_look() {
        let (f, ids) = flow_with(vec![
            NodeKind::OpenApp("com.x".into()),
            NodeKind::Key(crate::keys::Shortcut::new(9, 0)),
        ]);
        let issues = check_flow(&f, &["f1"]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(issues[0].wire, Some((ids[0], Port::Next)));

        let (f, _) = flow_with(vec![
            NodeKind::Command("long".into()),
            NodeKind::Native {
                op: NativeOp::ReadFile,
                a: "/tmp/x".into(),
                b: "text".into(),
            },
        ]);
        let issues = check_flow(&f, &["f1"]);
        assert!(issues.iter().any(|i| i.severity == Severity::Warning));
        assert!(issues.iter().all(|i| i.severity != Severity::Error));
    }

    #[test]
    fn the_flows_of_the_documentation_have_no_red_wire() {
        // what the documentation tells an agent to write must not be marked as wrong
        for (name, text) in [
            (
                "open-app-and-new-note",
                include_str!("../../docs/examples/flows/open-app-and-new-note.cursorpie-flow.json"),
            ),
            (
                "copy-switch-paste",
                include_str!("../../docs/examples/flows/copy-switch-paste.cursorpie-flow.json"),
            ),
            (
                "link-wait-command",
                include_str!("../../docs/examples/flows/link-wait-command.cursorpie-flow.json"),
            ),
            (
                "ask-and-loop",
                include_str!("../../docs/examples/flows/ask-and-loop.cursorpie-flow.json"),
            ),
            (
                "copy-newest-file-and-paste",
                include_str!(
                    "../../docs/examples/flows/copy-newest-file-and-paste.cursorpie-flow.json"
                ),
            ),
            (
                "quick-reply-unread-chats",
                include_str!(
                    "../../docs/examples/flows/quick-reply-unread-chats.cursorpie-flow.json"
                ),
            ),
            (
                "send-then-wait-for-answer",
                include_str!(
                    "../../docs/examples/flows/send-then-wait-for-answer.cursorpie-flow.json"
                ),
            ),
            (
                "telegram-folder-triage",
                include_str!(
                    "../../docs/examples/flows/telegram-folder-triage.cursorpie-flow.json"
                ),
            ),
            (
                "telegram-saved-reply",
                include_str!("../../docs/examples/flows/telegram-saved-reply.cursorpie-flow.json"),
            ),
        ] {
            let flow = crate::config::flow::parse_flow(text).expect("a valid flow file");
            let issues = check_flow(&flow, &[flow.id.as_str()]);
            assert!(
                issues.iter().all(|i| i.severity != Severity::Error),
                "{name}: {issues:?}"
            );
        }
    }

    fn list(op: ListOp, list: &str, a: &str, out: &str) -> NodeKind {
        NodeKind::List {
            op,
            list: list.into(),
            a: a.into(),
            b: String::new(),
            out: out.into(),
        }
    }

    #[test]
    fn a_list_has_to_be_made_before_it_is_used() {
        let (f, ids) = flow_with(vec![list(ListOp::Get, "xs", "1", "x")]);
        let issues = check_flow(&f, &["f1"]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].severity, Severity::Error);
        assert_eq!(
            issues[0].wire,
            Some((f.start().map(|s| s.id).unwrap_or(0), Port::Next))
        );
        assert_eq!(issues[0].block, ids[0]);
        assert!(issues[0].message.contains("list"), "{}", issues[0].message);

        // Split makes it, Push and the rest keep it, and what Get returns is set afterwards
        let (f, _) = flow_with(vec![
            list(ListOp::Split, "xs", "a,b", ""),
            list(ListOp::Push, "xs", "c", ""),
            list(ListOp::Get, "xs", "1", "x"),
            NodeKind::Command("echo {{x}}".into()),
        ]);
        // (the Split block's own text field has a comma but its separator is empty: that is fine)
        let issues = check_flow(&f, &["f1"]);
        assert!(issues.is_empty(), "{issues:?}");

        // a list that a System block makes
        let (f, _) = flow_with(vec![
            NodeKind::Native {
                op: crate::config::flow::NativeOp::ListDir,
                a: "~/Downloads".into(),
                b: "files".into(),
            },
            NodeKind::ForEach {
                list: "files".into(),
                item: "file".into(),
            },
        ]);
        let issues = check_flow(&f, &["f1"]);
        assert!(
            issues.iter().all(|i| i.severity != Severity::Error),
            "{issues:?}"
        );
    }

    #[test]
    fn the_body_of_a_for_each_has_to_come_back_and_a_position_has_to_be_a_number() {
        let mut f = Flow::new("f1", "Demo");
        let s = f.add_after(None, list(ListOp::Split, "xs", "a,b", ""));
        let each = f.add_after(
            Some(s),
            NodeKind::ForEach {
                list: "xs".into(),
                item: "it".into(),
            },
        );
        let body = f.add_at(
            NodeKind::Command("echo {{it}} {{index}}".into()),
            300.0,
            300.0,
        );
        assert!(f.connect(each, Port::Next, body));
        let issues = check_flow(&f, &["f1"]);
        assert!(
            issues
                .iter()
                .any(|i| i.severity == Severity::Error && i.wire == Some((each, Port::Next))),
            "the body does not come back: {issues:?}"
        );
        assert!(f.connect(body, Port::Next, each));
        let issues = check_flow(&f, &["f1"]);
        assert!(
            issues.iter().all(|i| i.severity != Severity::Error),
            "{issues:?}"
        );

        // `i` is not a variable in a position: only {{i}} is
        let (f, _) = flow_with(vec![
            list(ListOp::Split, "xs", "a,b", ""),
            list(ListOp::Get, "xs", "i", "x"),
        ]);
        let issues = check_flow(&f, &["f1"]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(
            issues[0].message.contains("position"),
            "{}",
            issues[0].message
        );
        let (f, _) = flow_with(vec![
            set("i", "1"),
            list(ListOp::Split, "xs", "a,b", ""),
            list(ListOp::Get, "xs", "{{i}}", "x"),
        ]);
        assert!(check_flow(&f, &["f1"]).is_empty());
    }

    #[test]
    fn filters_that_do_not_exist_are_a_warning_and_a_default_stands_in_for_a_missing_variable() {
        let (f, _) = flow_with(vec![
            set("x", " a "),
            NodeKind::Command("echo {{x|trim|upper}}".into()),
        ]);
        assert!(check_flow(&f, &["f1"]).is_empty());
        let (f, _) = flow_with(vec![
            set("x", " a "),
            NodeKind::Command("echo {{x|shout}}".into()),
        ]);
        let issues = check_flow(&f, &["f1"]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].severity, Severity::Warning);
        assert!(issues[0].message.contains("shout"));
        // a variable nobody set, with a default: nothing is wrong
        let (f, _) = flow_with(vec![NodeKind::Command("echo {{who|default:you}}".into())]);
        assert!(check_flow(&f, &["f1"]).is_empty());
        // without it, it is
        let (f, _) = flow_with(vec![NodeKind::Command("echo {{who|trim}}".into())]);
        assert_eq!(check_flow(&f, &["f1"]).len(), 1);
    }

    #[test]
    fn a_volume_has_to_be_a_number_from_0_to_100() {
        let vol = |a: &str| NodeKind::Native {
            op: crate::config::flow::NativeOp::VolumeSet,
            a: a.into(),
            b: String::new(),
        };
        for ok in ["0", "50", "100", "{{v}}"] {
            let (f, _) = flow_with(vec![set("v", "10"), vol(ok)]);
            assert!(check_flow(&f, &["f1"]).is_empty(), "{ok}");
        }
        for bad in ["loud", "101", "-1", ""] {
            let (f, _) = flow_with(vec![vol(bad)]);
            assert_eq!(check_flow(&f, &["f1"]).len(), 1, "{bad}");
        }
    }

    #[test]
    fn a_browser_command_that_is_written_out_has_to_be_one_the_browser_knows() {
        let browser = |a: &str, b: &str| NodeKind::Native {
            op: crate::config::flow::NativeOp::Browser,
            a: a.into(),
            b: b.into(),
        };
        // the reply may be left without a place; a command with a variable is checked at run time
        for ok in [
            "media.next",
            "tab.find {{word}}",
            "{{cmd}}",
            "chat.draft_to Anna :: hi",
        ] {
            let (f, _) = flow_with(vec![
                set("word", "a"),
                set("cmd", "tab.url"),
                browser(ok, ""),
            ]);
            assert!(check_flow(&f, &["f1"]).is_empty(), "{ok}");
        }
        for bad in ["", "media.nxt", "tab.find", "chat.send_to Anna"] {
            let (f, _) = flow_with(vec![browser(bad, "")]);
            assert_eq!(check_flow(&f, &["f1"]).len(), 1, "{bad}");
        }
        // the variable that gets the reply counts as set
        let (f, _) = flow_with(vec![
            browser("tab.url", "address"),
            NodeKind::Command("echo {{address}}".into()),
        ]);
        assert!(check_flow(&f, &["f1"]).is_empty());
    }

    #[test]
    fn the_chat_commands_for_a_flow_are_known_to_the_check() {
        let browser = |a: &str, b: &str| NodeKind::Native {
            op: crate::config::flow::NativeOp::Browser,
            a: a.into(),
            b: b.into(),
        };
        // with or without the number of seconds or of messages, and with a variable in it
        for ok in [
            "chat.unread",
            "chat.unread @telegram",
            "chat.who",
            "chat.unread_names",
            "chat.last",
            "chat.read",
            "chat.read 3",
            "chat.wait",
            "chat.wait 90",
            "chat.wait {{secs}}",
        ] {
            let (f, _) = flow_with(vec![set("secs", "30"), browser(ok, "")]);
            assert!(check_flow(&f, &["f1"]).is_empty(), "{ok}");
        }
        // a name that is not one of them is found before the flow runs
        for bad in [
            "chat.waiting 5",
            "chat.unread_name",
            "chat.lastly",
            "chat.reed 3",
        ] {
            let (f, _) = flow_with(vec![browser(bad, "")]);
            assert_eq!(check_flow(&f, &["f1"]).len(), 1, "{bad}");
        }
        // what chat.wait brings back is a variable like any other: set by the block, read after it
        let (f, ids) = flow_with(vec![
            browser("chat.wait 90", "answer"),
            NodeKind::Command("echo {{answer}}".into()),
        ]);
        assert!(check_flow(&f, &["f1"]).is_empty());
        assert_eq!(is_set_before(&f, ids[1], "answer"), Some(true));
        // and one that reads it before the wait has put anything in it is not set yet
        let (f, ids) = flow_with(vec![
            NodeKind::Command("echo {{answer}}".into()),
            browser("chat.wait 90", "answer"),
        ]);
        assert_eq!(is_set_before(&f, ids[0], "answer"), Some(false));
    }

    #[test]
    fn the_errors_come_first_and_nothing_is_said_twice() {
        let (f, _) = flow_with(vec![
            NodeKind::OpenApp("com.x".into()),
            NodeKind::Key(crate::keys::Shortcut::new(9, 0)),
            NodeKind::Command("echo {{a}} {{a}}".into()),
        ]);
        let issues = check_flow(&f, &["f1"]);
        assert!(issues.len() >= 2, "{issues:?}");
        assert_eq!(issues[0].severity, Severity::Error);
        let mut seen = issues.clone();
        seen.dedup();
        assert_eq!(seen, issues);
    }
}
