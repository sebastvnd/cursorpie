//! Flows: chains of steps drawn as blocks joined by wires (the *Flow* tab of Settings).
//!
//! A [`Flow`] is a small graph. Every block ([`FlowNode`]) has at most one wire going out
//! (`next`) and at most one coming in, so the graph is a chain that starts at the one
//! [`NodeKind::Start`] block and ends at the one [`NodeKind::End`] block. Running a flow follows the
//! wires from *Start* and does each block in order ([`Flow::steps`]); blocks that are not wired
//! to the chain are ignored.
//!
//! A slice uses a flow through [`Action::Flow`], which names the flow by its `id`. The flows live in
//! [`Config::flows`](super::Config::flows), so they are saved, backed up, exported and undone with the rest of the config.
//!
//! ## Sharing
//!
//! A flow is shared as one JSON file, `<name>.cursorpie-flow.json`:
//!
//! ```text
//! { "cursorpie_flow": 1, "flow": { "id": "...", "name": "...", "nodes": [...] } }
//! ```
//!
//! A profile file ([`share`](super::share)) also carries the flows its slices use, so a shared
//! profile keeps working. Importing never trusts a file: the flow goes through [`Flow::clean`]
//! (limits, one *Start* and one *End*, no loops), it always gets a **new** id and a unique name, and the caller
//! warns when it holds shell commands ([`Flow::has_commands`]).

use super::share::safe_stem;
use super::{Action, Item, ItemKind, MAX_FLOW_NODES, MAX_FLOWS, MAX_WAIT_MS};
use crate::keys::Shortcut;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

/// Value of `cursorpie_flow` in files written by this version.
pub const FLOW_FORMAT: u32 = 1;
/// Larger flow files are refused without being read.
pub const MAX_FLOW_FILE_BYTES: u64 = 1024 * 1024;
/// Blocks are kept inside this distance of the origin, so a hostile file cannot place one at
/// infinity.
pub const COORD_LIMIT: f32 = 20_000.0;
/// Width of an ordinary block in the editor, in points.
pub const NODE_W: f32 = 250.0;
/// Width of the *Start* block.
pub const START_W: f32 = 150.0;
/// Longest name of a flow, in characters.
pub const MAX_NAME_CHARS: usize = 60;

/// What a block does.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum NodeKind {
    /// Where the chain begins. There is exactly one.
    Start,
    /// Press a shortcut.
    Key(Shortcut),
    /// Open (or raise) an app.
    OpenApp(String),
    /// Open a URL or a file.
    OpenUrl(String),
    /// Run a shell command.
    Command(String),
    /// Wait this many milliseconds before the next block.
    Wait(u32),
    /// Where the chain ends. There is exactly one; it has an input and no output.
    End,
}

impl NodeKind {
    /// The action this block performs; `None` for *Start* and *End*.
    pub fn to_action(&self) -> Option<Action> {
        match self {
            NodeKind::Start | NodeKind::End => None,
            NodeKind::Key(sc) => Some(Action::Key(*sc)),
            NodeKind::OpenApp(s) => Some(Action::OpenApp(s.clone())),
            NodeKind::OpenUrl(s) => Some(Action::OpenUrl(s.clone())),
            NodeKind::Command(s) => Some(Action::Command(s.clone())),
            NodeKind::Wait(ms) => Some(Action::Wait(*ms)),
        }
    }

    /// Width of the block in the editor.
    pub fn width(&self) -> f32 {
        if matches!(self, NodeKind::Start | NodeKind::End) {
            START_W
        } else {
            NODE_W
        }
    }
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
    /// The block the wire from this block's output goes to.
    #[serde(default)]
    pub next: Option<u32>,
}

/// A named chain of blocks.
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
                },
                FlowNode {
                    id: 2,
                    x: 40.0 + START_W + 50.0,
                    y: 60.0,
                    kind: NodeKind::End,
                    next: None,
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

    /// The blocks that run, in order: *Start*, then whatever its wire leads to, and so on.
    pub fn chain(&self) -> Vec<&FlowNode> {
        let mut out: Vec<&FlowNode> = Vec::new();
        let mut cur = self.start();
        while let Some(n) = cur {
            if out.len() > self.nodes.len() {
                break; // a loop (clean() removes them, this keeps a hand-made one harmless)
            }
            out.push(n);
            cur = n.next.and_then(|id| self.node(id));
        }
        out
    }

    /// The id of the last block of the chain.
    pub fn tail(&self) -> Option<u32> {
        self.chain().last().map(|n| n.id)
    }

    /// The actions the flow performs, in order.
    pub fn steps(&self) -> Vec<Action> {
        self.chain()
            .iter()
            .filter_map(|n| n.kind.to_action())
            .collect()
    }

    /// Whether running the flow runs a shell command. Only the blocks that are wired count: an
    /// unplugged one never runs. (Importing warns about all of them anyway, see
    /// [`Flow::has_commands`].)
    pub fn runs_commands(&self) -> bool {
        self.chain()
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Command(_)))
    }

    /// Whether any block, wired or not, holds a shell command: what a warning after an import
    /// goes by, because wiring it up later is one drag.
    pub fn has_commands(&self) -> bool {
        self.nodes
            .iter()
            .any(|n| matches!(n.kind, NodeKind::Command(_)))
    }

    /// Adds a block after `after` (or just before *End* when `after` is `None`, gone, or *End*
    /// itself) and wires it in: a block that already had a successor keeps it, behind the new one.
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
        self.nodes.push(FlowNode {
            id,
            x,
            y,
            kind,
            next: anext,
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

    /// Removes a block (never *Start* or *End*) and joins its neighbours, so deleting a step in the
    /// middle of a chain leaves no gap. Returns whether a block was removed.
    pub fn remove_node(&mut self, id: u32) -> bool {
        let Some(node) = self.node(id) else {
            return false;
        };
        if matches!(node.kind, NodeKind::Start | NodeKind::End) {
            return false;
        }
        let after = node.next;
        for n in &mut self.nodes {
            if n.next == Some(id) {
                n.next = after;
            }
        }
        self.nodes.retain(|n| n.id != id);
        true
    }

    /// Draws a wire from the output of `from` to the input of `to`. The input takes one wire, so
    /// the one that was there is removed, and so is the one that left `from` before. Refused
    /// (`false`) for a block to itself, for *Start* as a target, for *End* as a source, and when
    /// it would make a loop.
    pub fn connect(&mut self, from: u32, to: u32) -> bool {
        if from == to {
            return false;
        }
        let (Some(a), Some(b)) = (self.node(from), self.node(to)) else {
            return false;
        };
        if matches!(b.kind, NodeKind::Start) || matches!(a.kind, NodeKind::End) {
            return false;
        }
        // a loop: following the wires from `to` would come back to `from`
        let mut cur = Some(to);
        let mut guard = 0;
        while let Some(c) = cur {
            if c == from {
                return false;
            }
            guard += 1;
            if guard > self.nodes.len() {
                return false;
            }
            cur = self.node(c).and_then(|n| n.next);
        }
        self.unplug_into(to);
        if let Some(n) = self.node_mut(from) {
            n.next = Some(to);
        }
        true
    }

    /// Removes the wire that goes into `to`.
    pub fn unplug_into(&mut self, to: u32) {
        for n in &mut self.nodes {
            if n.next == Some(to) {
                n.next = None;
            }
        }
    }

    /// Forces the flow into the limits the rest of the program relies on: a name, at most
    /// [`MAX_FLOW_NODES`] blocks with unique ids and reasonable places, exactly one *Start* and one
    /// *End* (a flow from before *End* existed gets one after its last block), waits
    /// of at most [`MAX_WAIT_MS`], wires that lead to real blocks, one wire into each block, and
    /// no loops. Called for every config that is loaded, imported or saved.
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
            if let NodeKind::Wait(ms) = &mut n.kind {
                *ms = (*ms).min(MAX_WAIT_MS);
            }
        }

        // wires: to a block that exists, not to itself, not into Start, one into each block
        let live: BTreeSet<u32> = self.nodes.iter().map(|n| n.id).collect();
        let start_id = self.start().map(|n| n.id);
        let mut targeted = BTreeSet::new();
        for n in &mut self.nodes {
            if matches!(n.kind, NodeKind::End) {
                n.next = None; // nothing comes after End
            }
            if let Some(t) = n.next {
                let ok =
                    t != n.id && live.contains(&t) && Some(t) != start_id && targeted.insert(t);
                if !ok {
                    n.next = None;
                }
            }
        }
        // loops: a block whose wires lead back to itself loses its own wire
        let count = self.nodes.len();
        for i in 0..count {
            let me = self.nodes[i].id;
            let mut cur = self.nodes[i].next;
            let mut steps = 0;
            while let Some(c) = cur {
                if c == me {
                    self.nodes[i].next = None;
                    break;
                }
                steps += 1;
                if steps > count {
                    break;
                }
                cur = self.node(c).and_then(|n| n.next);
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
                if let Some(t) = self.node_mut(tid) {
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
        cursorpie_flow: FLOW_FORMAT,
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
        // two wires into one block: only the first keeps its wire
        let two = r#"{"cursorpie_flow":1,"flow":{"id":"a","name":"A","nodes":[
            {"id":1,"x":0,"y":0,"kind":"Start","next":3},
            {"id":2,"x":0,"y":0,"kind":{"Wait":1},"next":3},
            {"id":3,"x":0,"y":0,"kind":"End","next":null}]}}"#;
        let f = parse_flow(two).unwrap();
        assert_eq!(f.node(1).unwrap().next, Some(3));
        assert_eq!(f.node(2).unwrap().next, None);
        // a file made by a newer version is refused with its own sentence
        assert!(
            parse_flow(&long.replace("\"cursorpie_flow\":1", "\"cursorpie_flow\":2"))
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
        assert!(!f.connect(2, a));
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
    fn connecting_replaces_wires_and_refuses_loops() {
        let mut f = sample();
        let (k, w, u) = (f.nodes[2].id, f.nodes[3].id, f.nodes[4].id);
        assert!(!f.connect(k, k), "a block to itself");
        assert!(!f.connect(u, 1), "nothing goes into Start");
        assert!(!f.connect(u, k), "a loop");
        // Start -> url directly: the old wires into url and out of Start are replaced
        assert!(f.connect(1, u));
        assert_eq!(
            f.steps(),
            vec![Action::OpenUrl("https://example.com".into())]
        );
        assert!(f.node(w).is_some(), "the unplugged blocks are still there");
    }

    #[test]
    fn clean_repairs_a_hostile_flow() {
        let node = |id, next, kind| FlowNode {
            id,
            x: f32::NAN,
            y: 1e9,
            kind,
            next,
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
}
