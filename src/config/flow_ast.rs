//! A flow as a **program tree**: the same flow, written as nested statements instead of blocks and
//! wires.
//!
//! A [`Flow`] is a graph. That is the right shape to draw, and an awkward one to
//! write by hand or to hand to a program that writes flows: to say "if this, do that, else the
//! other, then go on" in a graph one has to join three wires and invent block ids. A **program
//! tree** says it the way a program does:
//!
//! ```text
//! { "cursorpie_flow_ast": 1,
//!   "name": "Odd numbers",
//!   "body": [
//!     { "Let": { "name": "n", "expr": "0" } },
//!     { "While": { "cond": "n < 4", "body": [
//!         { "Let": { "name": "n", "expr": "n + 1" } },
//!         { "If": { "cond": "n % 2 == 0", "then": ["Continue"] } },
//!         { "Do": { "Key": { "key": 8, "mods": 2 } } }
//!     ] } }
//!   ] }
//! ```
//!
//! - [`compile`] turns a tree into a flow (the blocks, their wires and a layout of the blocks on
//!   the canvas). Importing a flow file that has `cursorpie_flow_ast` at the top does this, so the
//!   *Import* button of the Flow tab reads both kinds of file ([`parse_ast_report`]).
//! - [`flow_to_ast`] turns a flow back into a tree, when its wires are **structured**: every
//!   `Branch` closes where its two sides meet, every loop is entered at its head, and the only
//!   wire that leaves a loop is the one of the loop block itself. It never guesses: the tree is
//!   compiled again and must give back the very same graph (see [`shape`]), or the export is
//!   refused with a sentence. *Shift + Export* in the Flow tab writes this file
//!   ([`export_flow_ast_to_downloads`]).
//!
//! ## The statements ([`Stmt`])
//!
//! | Statement | Becomes |
//! |---|---|
//! | `{"Do": <block>}` | one block: any kind of [`NodeKind`] that is not `Start`, `End`, `Branch`, `Repeat`, `While` or `ForEach` (`"Stop"` ends the whole run) |
//! | `{"Let": {"name", "expr"}}` | a *Set* block whose value is the expression `{{= expr }}` (see [`flowexpr`](crate::flowexpr)) |
//! | `{"If": {"cond", "then", "else"}}` | a *Branch* block; both sides lead on to what follows the `If` |
//! | `{"While": {"cond", "body"}}` | a *While* block, the end of its body wired back to it |
//! | `{"Repeat": {"times", "body"}}` | a *Repeat* block (the variable `index` counts from 1) |
//! | `{"ForEach": {"list", "item", "body"}}` | a *For each* block |
//! | `"Continue"` | a wire back to the loop block: the next round |
//! | `"Break"` | a wire to the block that follows the loop: leaves it |
//!
//! A condition (`cond`) is either a **text** that is an expression (`"n < 3 && ok"`), or an object
//! like the one of a *Branch* block (`{"a": "n", "op": "Lt", "b": "3"}`).
//!
//! `Break` is a wire from where it stands to what follows the innermost loop. The runner forgets the
//! rounds a loop has made when a run leaves it by any wire but the loop's own (see
//! [`flowrun`](crate::flowrun)), so a loop that was broken out of starts again at round 1 the next
//! time it is entered.

use super::flow::{ImportReport, export_flow, parse_flow_report};
use super::share::safe_stem;
use super::{CmpOp, Cond, Flow, FlowNode, MAX_FIELD_CHARS, MAX_FLOW_NODES, MAX_REPEAT, NodeKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

/// The value of `cursorpie_flow_ast` in the files this version writes and reads.
pub const AST_FORMAT: u32 = 1;

/// How many statements may nest in one another.
pub const MAX_AST_DEPTH: usize = 24;

/// The blocks per row of the layout.
const PER_ROW: usize = 3;

/// Why a flow cannot be written as a tree.
const NOT_STRUCTURED: &str = "This flow cannot be written as a program tree: its wires do not \
     form nested If / While / Repeat / For each statements (for example a wire that jumps into a \
     loop or out of one, or paths that join in different places). Export it as an ordinary flow \
     file instead.";

/// What a statement asks. Either an expression (`"n < 3"`) or the condition of a *Branch* block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AstCond {
    /// An expression that is true when its value is not empty, `0`, `false`, `no` or `off`.
    Expr(String),
    /// `a op b`, as in a *Branch* block.
    Cmp(Cond),
}

impl AstCond {
    /// The condition of the block that carries it out.
    fn to_cond(&self) -> Result<Cond, String> {
        match self {
            AstCond::Cmp(c) => Ok(c.clone()),
            AstCond::Expr(src) => {
                let src = src.trim();
                if src.contains("}}") {
                    return Err(format!("the condition `{}` holds `}}}}`", short(src)));
                }
                crate::flowexpr::parse(src)
                    .map_err(|why| format!("the condition `{}` is wrong: {why}", short(src)))?;
                Ok(Cond {
                    a: format!("{{{{= {src} }}}}"),
                    op: CmpOp::IsTrue,
                    b: String::new(),
                })
            }
        }
    }

    /// The way to write the condition of a block: as an expression when it is exactly what an
    /// expression condition compiles to (so it compiles back to the same block).
    fn of(c: &Cond) -> AstCond {
        if c.op == CmpOp::IsTrue
            && c.b.is_empty()
            && let Some(rest) = c.a.strip_prefix("{{= ")
            && let Some(inner) = rest.strip_suffix(" }}")
            && !inner.is_empty()
            && inner.trim() == inner
            && !inner.contains("}}")
        {
            return AstCond::Expr(inner.to_string());
        }
        AstCond::Cmp(c.clone())
    }
}

/// One statement of a program tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Stmt {
    /// One block. The kinds that are structure (`Start`, `End`, `Branch`, `Repeat`, `While`,
    /// `ForEach`) are written with the statements below.
    Do(NodeKind),
    /// `name = expr`: a *Set* block with the value `{{= expr }}`.
    Let { name: String, expr: String },
    /// Runs `then` when the condition holds and `else` when it does not, then goes on.
    If {
        cond: AstCond,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        then: Vec<Stmt>,
        #[serde(default, rename = "else", skip_serializing_if = "Vec::is_empty")]
        otherwise: Vec<Stmt>,
    },
    /// Runs `body` as long as the condition holds.
    While { cond: AstCond, body: Vec<Stmt> },
    /// Runs `body` this many times; the variable `index` counts the rounds from 1.
    Repeat { times: u32, body: Vec<Stmt> },
    /// Runs `body` for every item of the list in the variable `list`, with the item in the
    /// variable `item` and its position in `index`.
    ForEach {
        list: String,
        item: String,
        body: Vec<Stmt>,
    },
    /// Goes to the next round of the innermost loop. Nothing may follow it in the same list.
    Continue,
    /// Leaves the innermost loop and goes on with what follows it. Nothing may follow it in the
    /// same list.
    Break,
}

/// A flow written as a program tree: what `cursorpie_flow_ast` files hold.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FlowAst {
    /// The format of the file: [`AST_FORMAT`].
    pub cursorpie_flow_ast: u32,
    #[serde(default)]
    pub name: String,
    pub body: Vec<Stmt>,
}

/// A text cut to something a sentence can hold.
fn short(s: &str) -> String {
    let mut out: String = s.chars().take(40).collect();
    if s.chars().count() > 40 {
        out.push('\u{2026}');
    }
    out
}

// -------------------------------------------------------------------------------- tree -> graph

/// Builds the blocks of a flow from the end backwards, so every statement knows where it goes on
/// to (`cont`) when it is made.
struct Builder {
    nodes: Vec<FlowNode>,
}

impl Builder {
    /// Adds a block and returns its id. The ids are 1, 2, 3... in the order of the blocks, which
    /// is what [`Builder::set_next`] relies on.
    fn push(&mut self, kind: NodeKind, next: Option<u32>, alt: Option<u32>) -> Result<u32, String> {
        if self.nodes.len() >= MAX_FLOW_NODES {
            return Err(format!(
                "the program makes more than {MAX_FLOW_NODES} blocks, which is the most a flow can have"
            ));
        }
        let id = self.nodes.len() as u32 + 1;
        self.nodes.push(FlowNode {
            id,
            x: 0.0,
            y: 0.0,
            kind,
            next,
            alt,
        });
        Ok(id)
    }

    fn set_next(&mut self, id: u32, next: Option<u32>) {
        if let Some(n) = self.nodes.get_mut((id as usize).saturating_sub(1)) {
            n.next = next;
        }
    }

    /// The statements of one list, in order; returns the block where the list begins (`cont`
    /// itself for an empty list).
    fn block(
        &mut self,
        stmts: &[Stmt],
        cont: u32,
        lp: Option<(u32, u32)>,
        depth: usize,
    ) -> Result<u32, String> {
        if depth > MAX_AST_DEPTH {
            return Err(format!(
                "the program nests more than {MAX_AST_DEPTH} levels deep"
            ));
        }
        for (i, s) in stmts.iter().enumerate() {
            let ends = matches!(s, Stmt::Continue | Stmt::Break | Stmt::Do(NodeKind::Stop));
            if ends && i + 1 < stmts.len() {
                return Err(
                    "nothing can follow `Continue`, `Break` or `Stop` in the same list: it would never run"
                        .to_string(),
                );
            }
        }
        let mut entry = cont;
        for s in stmts.iter().rev() {
            entry = self.stmt(s, entry, lp, depth)?;
        }
        Ok(entry)
    }

    /// One statement that goes on to `cont`; returns the block where it begins. `lp` is the block
    /// of the innermost loop and the block that follows it (where `Continue` and `Break` go).
    fn stmt(
        &mut self,
        s: &Stmt,
        cont: u32,
        lp: Option<(u32, u32)>,
        depth: usize,
    ) -> Result<u32, String> {
        match s {
            Stmt::Do(kind) => {
                let structure = match kind {
                    NodeKind::Start | NodeKind::End => {
                        Some("Start and End are not statements: the flow has them already")
                    }
                    NodeKind::Branch(_) => Some("a Branch is written as an `If`"),
                    NodeKind::While(_) => Some("a While block is written as a `While` statement"),
                    NodeKind::Repeat(_) => {
                        Some("a Repeat block is written as a `Repeat` statement")
                    }
                    NodeKind::ForEach { .. } => {
                        Some("a For each block is written as a `ForEach` statement")
                    }
                    _ => None,
                };
                if let Some(why) = structure {
                    return Err(format!("`Do` cannot hold that: {why}"));
                }
                if matches!(kind, NodeKind::Stop) {
                    self.push(NodeKind::Stop, None, None)
                } else {
                    self.push(kind.clone(), Some(cont), None)
                }
            }
            Stmt::Let { name, expr } => {
                let name = name.trim();
                if !super::flow::is_var_name(name) {
                    return Err(format!(
                        "`{}` cannot be the name of a variable: letters, digits and _, not starting with a digit",
                        short(name)
                    ));
                }
                let src = expr.trim();
                if src.contains("}}") {
                    return Err(format!("the expression `{}` holds `}}}}`", short(src)));
                }
                crate::flowexpr::parse(src)
                    .map_err(|why| format!("the expression of `{name}` is wrong: {why}"))?;
                if src.chars().count() > MAX_FIELD_CHARS {
                    return Err(format!("the expression of `{name}` is too long"));
                }
                self.push(
                    NodeKind::Set {
                        name: name.to_string(),
                        value: format!("{{{{= {src} }}}}"),
                    },
                    Some(cont),
                    None,
                )
            }
            Stmt::If {
                cond,
                then,
                otherwise,
            } => {
                let c = cond.to_cond()?;
                let t = self.block(then, cont, lp, depth + 1)?;
                let e = self.block(otherwise, cont, lp, depth + 1)?;
                self.push(NodeKind::Branch(c), Some(t), Some(e))
            }
            Stmt::While { cond, body } => {
                let c = cond.to_cond()?;
                self.looped(NodeKind::While(c), body, cont, depth)
            }
            Stmt::Repeat { times, body } => {
                if *times > MAX_REPEAT {
                    return Err(format!("`Repeat` makes at most {MAX_REPEAT} rounds"));
                }
                self.looped(NodeKind::Repeat(*times), body, cont, depth)
            }
            Stmt::ForEach { list, item, body } => self.looped(
                NodeKind::ForEach {
                    list: list.clone(),
                    item: item.clone(),
                },
                body,
                cont,
                depth,
            ),
            Stmt::Continue => lp
                .map(|(head, _)| head)
                .ok_or_else(|| "`Continue` can only stand inside a loop".to_string()),
            Stmt::Break => lp
                .map(|(_, exit)| exit)
                .ok_or_else(|| "`Break` can only stand inside a loop".to_string()),
        }
    }

    /// A loop block with `body`: its `next` leads into the body, the body leads back to it, and
    /// its `alt` goes on to `cont`.
    fn looped(
        &mut self,
        kind: NodeKind,
        body: &[Stmt],
        cont: u32,
        depth: usize,
    ) -> Result<u32, String> {
        if body.is_empty() {
            return Err("a loop needs at least one statement in its body".to_string());
        }
        let id = self.push(kind, None, Some(cont))?;
        let entry = self.block(body, id, Some((id, cont)), depth + 1)?;
        if entry == id {
            return Err("a loop whose body is only `Continue` never ends".to_string());
        }
        self.set_next(id, Some(entry));
        Ok(id)
    }
}

/// Puts the blocks on the canvas: in the order a run meets them, [`PER_ROW`] to a row. It decides
/// nothing about what runs; the *Tidy* button of the Flow tab lines them up again.
fn layout(nodes: &mut [FlowNode]) {
    let mut order: Vec<u32> = Vec::new();
    let mut seen: BTreeSet<u32> = BTreeSet::new();
    let mut stack: Vec<u32> = vec![1];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        order.push(id);
        if let Some(n) = nodes.get((id as usize).saturating_sub(1)) {
            // the alt goes on the stack first, so the next is met first
            if let Some(a) = n.alt {
                stack.push(a);
            }
            if let Some(nx) = n.next {
                stack.push(nx);
            }
        }
    }
    // a block no wire reaches (the End of a flow that always stops) goes last
    for id in 1..=nodes.len() as u32 {
        if !seen.contains(&id) {
            order.push(id);
        }
    }
    for (k, id) in order.iter().enumerate() {
        if let Some(n) = nodes.get_mut((*id as usize).saturating_sub(1)) {
            n.x = 40.0 + (k % PER_ROW) as f32 * (super::flow::NODE_W + 50.0);
            n.y = 60.0 + (k / PER_ROW) as f32 * 130.0;
        }
    }
}

/// Makes the flow a program tree describes: *Start*, the blocks of the statements, *End*. The
/// flow has no id yet (importing gives it one) and no repairs have been made: see
/// [`parse_ast_report`] for the whole import.
pub fn compile(ast: &FlowAst) -> Result<Flow, String> {
    if ast.cursorpie_flow_ast == 0 {
        return Err("That file says format 0, which never existed: it is damaged.".to_string());
    }
    if ast.cursorpie_flow_ast > AST_FORMAT {
        return Err(format!(
            "That program was made by a newer CursorPie (file format {}; this version reads up to \
             {AST_FORMAT}). Update the app first.",
            ast.cursorpie_flow_ast
        ));
    }
    let mut b = Builder { nodes: Vec::new() };
    let start = b.push(NodeKind::Start, None, None)?;
    let end = b.push(NodeKind::End, None, None)?;
    let entry = b.block(&ast.body, end, None, 0)?;
    b.set_next(start, Some(entry));
    let mut nodes = b.nodes;
    layout(&mut nodes);
    let name = if ast.name.trim().is_empty() {
        "Flow from a program".to_string()
    } else {
        ast.name.clone()
    };
    Ok(Flow {
        id: "ast".to_string(),
        name,
        nodes,
    })
}

/// Reads a program-tree file that is already parsed as JSON, makes the flow and runs it through
/// the same repairs and checks as any imported flow file ([`parse_flow_report`]).
pub fn parse_ast_report(v: serde_json::Value) -> Result<ImportReport, String> {
    let ast: FlowAst = serde_json::from_value(v)
        .map_err(|e| format!("That program file is damaged: {}.", short(&e.to_string())))?;
    let flow = compile(&ast)?;
    let text = export_flow(&flow)?;
    parse_flow_report(&text)
}

// -------------------------------------------------------------------------------- graph -> tree

/// The graph of a flow as text, independent of ids and positions: the blocks that a run can
/// reach, numbered in the order a depth-first walk from *Start* meets them, each with its kind
/// and where its two wires go. Two flows with the same shape run the same way.
pub fn shape(flow: &Flow) -> Vec<String> {
    let mut order: Vec<u32> = Vec::new();
    let mut index: BTreeMap<u32, usize> = BTreeMap::new();
    let mut stack: Vec<u32> = flow.start().map(|n| n.id).into_iter().collect();
    while let Some(id) = stack.pop() {
        if index.contains_key(&id) {
            continue;
        }
        let Some(n) = flow.node(id) else {
            continue;
        };
        index.insert(id, order.len());
        order.push(id);
        if let Some(a) = n.alt {
            stack.push(a);
        }
        if let Some(nx) = n.next {
            stack.push(nx);
        }
    }
    let at = |wire: Option<u32>| match wire.and_then(|i| index.get(&i)) {
        Some(k) => k.to_string(),
        None => "-".to_string(),
    };
    order
        .iter()
        .filter_map(|id| flow.node(*id))
        .map(|n| format!("{:?}|{}|{}", n.kind, at(n.next), at(n.alt)))
        .collect()
}

/// Reads the structure back out of the wires.
struct Walker<'a> {
    flow: &'a Flow,
    /// How many steps are left: a graph that is not what it seems cannot make this run forever.
    budget: usize,
}

impl Walker<'_> {
    /// The blocks reachable from `from` (the order a depth-first walk meets them), not going on
    /// past the head of the loop `lp` stands in or the block that follows it.
    fn reach(&self, from: Option<u32>, lp: Option<(u32, u32)>) -> Vec<u32> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut stack: Vec<u32> = from.into_iter().collect();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) || lp.is_some_and(|(head, exit)| id == head || id == exit) {
                continue;
            }
            out.push(id);
            if let Some(n) = self.flow.node(id) {
                if let Some(a) = n.alt {
                    stack.push(a);
                }
                if let Some(nx) = n.next {
                    stack.push(nx);
                }
            }
        }
        out
    }

    /// The statements from the block `from` until the run arrives at `cont` (where the list goes
    /// on). `lp` is the block of the innermost loop and the block that follows it: a wire to the
    /// first is a `Continue`, a wire to the second a `Break`.
    fn walk(
        &mut self,
        from: Option<u32>,
        cont: u32,
        lp: Option<(u32, u32)>,
        depth: usize,
    ) -> Result<Vec<Stmt>, String> {
        if depth > MAX_AST_DEPTH {
            return Err(NOT_STRUCTURED.to_string());
        }
        let flow = self.flow;
        let mut out = Vec::new();
        let mut cur = from;
        loop {
            if self.budget == 0 {
                return Err(NOT_STRUCTURED.to_string());
            }
            self.budget -= 1;
            let Some(id) = cur else {
                return Err(
                    "A wire is missing: the run would end there. Wire every block \
                     before writing the flow as a tree."
                        .to_string(),
                );
            };
            if id == cont {
                return Ok(out);
            }
            if let Some((head, exit)) = lp {
                if id == head {
                    out.push(Stmt::Continue);
                    return Ok(out);
                }
                if id == exit {
                    out.push(Stmt::Break);
                    return Ok(out);
                }
            }
            let Some(node) = flow.node(id) else {
                return Err(NOT_STRUCTURED.to_string());
            };
            match &node.kind {
                NodeKind::Start | NodeKind::End => return Err(NOT_STRUCTURED.to_string()),
                NodeKind::Stop => {
                    out.push(Stmt::Do(NodeKind::Stop));
                    return Ok(out);
                }
                NodeKind::Branch(c) => {
                    let (t, e) = (node.next, node.alt);
                    if t.is_none() || e.is_none() {
                        return Err(NOT_STRUCTURED.to_string());
                    }
                    let on_then = self.reach(t, lp);
                    let on_else: BTreeSet<u32> = self.reach(e, lp).into_iter().collect();
                    let join = on_then.iter().copied().find(|i| on_else.contains(i));
                    let cond = AstCond::of(c);
                    if let Some(j) = join {
                        let then = self.walk(t, j, lp, depth + 1)?;
                        let otherwise = self.walk(e, j, lp, depth + 1)?;
                        out.push(Stmt::If {
                            cond,
                            then,
                            otherwise,
                        });
                        cur = Some(j);
                    } else {
                        // the two sides never meet: one of them ends the run, and the other one
                        // goes on to where this list ends
                        let then = self.walk(t, cont, lp, depth + 1)?;
                        let otherwise = self.walk(e, cont, lp, depth + 1)?;
                        out.push(Stmt::If {
                            cond,
                            then,
                            otherwise,
                        });
                        return Ok(out);
                    }
                }
                NodeKind::Repeat(_) | NodeKind::While(_) | NodeKind::ForEach { .. } => {
                    let (Some(first), Some(exit)) = (node.next, node.alt) else {
                        return Err(NOT_STRUCTURED.to_string());
                    };
                    let body = self.walk(Some(first), id, Some((id, exit)), depth + 1)?;
                    out.push(match &node.kind {
                        NodeKind::Repeat(times) => Stmt::Repeat {
                            times: *times,
                            body,
                        },
                        NodeKind::While(c) => Stmt::While {
                            cond: AstCond::of(c),
                            body,
                        },
                        NodeKind::ForEach { list, item } => Stmt::ForEach {
                            list: list.clone(),
                            item: item.clone(),
                            body,
                        },
                        _ => return Err(NOT_STRUCTURED.to_string()),
                    });
                    cur = node.alt;
                }
                other => {
                    out.push(Stmt::Do(other.clone()));
                    cur = node.next;
                }
            }
        }
    }
}

/// The program tree of a flow, or why it cannot be written as one. The tree is compiled again and
/// must give the same [`shape`] as the flow: what the walk could not tell for sure is refused here
/// rather than exported wrong. Blocks that no wire reaches from *Start* are not in the tree.
pub fn flow_to_ast(flow: &Flow) -> Result<FlowAst, String> {
    let Some(start) = flow.start() else {
        return Err("The flow has no Start block.".to_string());
    };
    let Some(end) = flow.end() else {
        return Err("The flow has no End block.".to_string());
    };
    let mut walker = Walker {
        flow,
        budget: flow.nodes.len() * 4 + 16,
    };
    let body = walker.walk(start.next, end.id, None, 0)?;
    let ast = FlowAst {
        cursorpie_flow_ast: AST_FORMAT,
        name: flow.name.clone(),
        body,
    };
    let again = compile(&ast).map_err(|why| format!("{NOT_STRUCTURED} ({why})"))?;
    if shape(&again) != shape(flow) {
        return Err(NOT_STRUCTURED.to_string());
    }
    Ok(ast)
}

/// The text of the program-tree file of `flow`.
pub fn export_flow_ast(flow: &Flow) -> Result<String, String> {
    let ast = flow_to_ast(flow)?;
    serde_json::to_string_pretty(&ast).map_err(|e| e.to_string())
}

/// Writes the program-tree file to `<downloads>/<name>.cursorpie-flow-ast.json` and returns its
/// path.
pub fn export_flow_ast_to_downloads(flow: &Flow) -> Result<PathBuf, String> {
    let text = export_flow_ast(flow)?;
    let mut p = super::downloads_dir();
    fs::create_dir_all(&p).map_err(|e| super::describe_io_error(&e))?;
    p.push(format!(
        "{}.cursorpie-flow-ast.json",
        safe_stem(&flow.name, "flow")
    ));
    super::backup::write_atomic(&p, &text).map_err(|e| super::describe_io_error(&e))?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::flow::{Port, parse_flow};
    use crate::keys::Shortcut;

    fn tree(json: &str) -> FlowAst {
        serde_json::from_str(json).unwrap_or_else(|e| panic!("{e}: {json}"))
    }

    fn key(n: u16) -> Stmt {
        Stmt::Do(NodeKind::Key(Shortcut::new(n, 2)))
    }

    fn wait(ms: u32) -> Stmt {
        Stmt::Do(NodeKind::Wait(ms))
    }

    fn program(body: Vec<Stmt>) -> FlowAst {
        FlowAst {
            cursorpie_flow_ast: AST_FORMAT,
            name: "t".into(),
            body,
        }
    }

    /// The import repaired nothing: no block dropped, no wire removed, no block cut off.
    fn no_repairs(report: &ImportReport) {
        for note in &report.notes {
            assert!(
                !(note.contains("dropped")
                    || note.contains("wire(s)")
                    || note.contains("cannot be reached")
                    || note.contains("no Start")
                    || note.contains("no End")),
                "{note}"
            );
        }
    }

    fn kinds(f: &Flow) -> Vec<&NodeKind> {
        f.chain().iter().map(|n| &n.kind).collect()
    }

    #[test]
    fn a_list_of_statements_is_a_chain_from_start_to_end() {
        let f = compile(&program(vec![wait(1), wait(2), wait(3)])).unwrap();
        assert_eq!(f.nodes.len(), 5);
        assert_eq!(
            kinds(&f),
            [
                &NodeKind::Start,
                &NodeKind::Wait(1),
                &NodeKind::Wait(2),
                &NodeKind::Wait(3),
                &NodeKind::End
            ]
        );
    }

    #[test]
    fn an_empty_program_wires_start_to_end() {
        let f = compile(&program(vec![])).unwrap();
        assert_eq!(f.start().unwrap().next, f.end().map(|n| n.id));
    }

    #[test]
    fn an_if_closes_where_its_two_sides_meet() {
        let c = AstCond::Expr("a > 1".into());
        let f = compile(&program(vec![
            Stmt::If {
                cond: c,
                then: vec![wait(1)],
                otherwise: vec![wait(2), wait(3)],
            },
            wait(4),
        ]))
        .unwrap();
        let branch = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Branch(_)))
            .unwrap();
        let find = |ms: u32| {
            f.nodes
                .iter()
                .find(|n| n.kind == NodeKind::Wait(ms))
                .unwrap()
        };
        assert_eq!(branch.next, Some(find(1).id));
        assert_eq!(branch.alt, Some(find(2).id));
        // both sides lead to the statement after the If
        assert_eq!(find(1).next, Some(find(4).id));
        assert_eq!(find(3).next, Some(find(4).id));
        // the condition is an expression block
        let NodeKind::Branch(cond) = &branch.kind else {
            panic!("a branch");
        };
        assert_eq!(cond.a, "{{= a > 1 }}");
        assert_eq!(cond.op, CmpOp::IsTrue);
    }

    #[test]
    fn an_if_without_an_else_skips_to_what_follows() {
        let f = compile(&program(vec![
            Stmt::If {
                cond: AstCond::Expr("x".into()),
                then: vec![wait(1)],
                otherwise: vec![],
            },
            wait(2),
        ]))
        .unwrap();
        let branch = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Branch(_)))
            .unwrap();
        let after = f
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Wait(2))
            .unwrap();
        assert_eq!(branch.alt, Some(after.id));
    }

    #[test]
    fn a_loop_is_wired_back_to_itself_and_goes_on_through_its_alt() {
        let f = compile(&program(vec![
            Stmt::Repeat {
                times: 3,
                body: vec![wait(1), wait(2)],
            },
            wait(3),
        ]))
        .unwrap();
        let rep = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Repeat(3)))
            .unwrap();
        let find = |ms: u32| {
            f.nodes
                .iter()
                .find(|n| n.kind == NodeKind::Wait(ms))
                .unwrap()
        };
        assert_eq!(rep.next, Some(find(1).id), "the body");
        assert_eq!(find(2).next, Some(rep.id), "the end of the body leads back");
        assert_eq!(rep.alt, Some(find(3).id), "done");
        assert_eq!(rep.wire(Port::Next), rep.next);
    }

    #[test]
    fn continue_is_a_wire_back_to_the_innermost_loop() {
        let f = compile(&program(vec![Stmt::While {
            cond: AstCond::Expr("1".into()),
            body: vec![
                Stmt::If {
                    cond: AstCond::Expr("a".into()),
                    then: vec![Stmt::Continue],
                    otherwise: vec![],
                },
                wait(1),
            ],
        }]))
        .unwrap();
        let w = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::While(_)))
            .unwrap();
        let branch = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Branch(_)))
            .unwrap();
        assert_eq!(branch.next, Some(w.id), "Continue goes to the loop block");
    }

    #[test]
    fn break_is_a_wire_to_what_follows_the_loop() {
        let f = compile(&program(vec![
            Stmt::Repeat {
                times: 3,
                body: vec![
                    Stmt::If {
                        cond: AstCond::Expr("a".into()),
                        then: vec![Stmt::Break],
                        otherwise: vec![],
                    },
                    wait(1),
                ],
            },
            wait(2),
        ]))
        .unwrap();
        let rep = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Repeat(3)))
            .unwrap();
        let branch = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Branch(_)))
            .unwrap();
        let after = f
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Wait(2))
            .unwrap();
        assert_eq!(
            branch.next,
            Some(after.id),
            "Break goes to what follows the loop"
        );
        assert_eq!(rep.alt, Some(after.id));
    }

    #[test]
    fn break_leaves_only_the_innermost_loop() {
        let f = compile(&program(vec![
            Stmt::While {
                cond: AstCond::Expr("1".into()),
                body: vec![
                    Stmt::While {
                        cond: AstCond::Expr("1".into()),
                        body: vec![Stmt::If {
                            cond: AstCond::Expr("a".into()),
                            then: vec![Stmt::Break],
                            otherwise: vec![wait(1)],
                        }],
                    },
                    wait(2),
                ],
            },
            wait(3),
        ]))
        .unwrap();
        let branch = f
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::Branch(_)))
            .unwrap();
        let two = f
            .nodes
            .iter()
            .find(|n| n.kind == NodeKind::Wait(2))
            .unwrap();
        assert_eq!(branch.next, Some(two.id), "to what follows the inner loop");
    }

    #[test]
    fn stop_has_no_wire_out() {
        let f = compile(&program(vec![Stmt::Do(NodeKind::Stop)])).unwrap();
        let stop = f.nodes.iter().find(|n| n.kind == NodeKind::Stop).unwrap();
        assert_eq!((stop.next, stop.alt), (None, None));
    }

    #[test]
    fn mistakes_are_refused_with_a_sentence() {
        let bad = |body: Vec<Stmt>| compile(&program(body)).unwrap_err();
        assert!(bad(vec![Stmt::Continue]).contains("inside a loop"));
        assert!(bad(vec![Stmt::Break]).contains("inside a loop"));
        assert!(
            bad(vec![Stmt::While {
                cond: AstCond::Expr("1".into()),
                body: vec![Stmt::Break, wait(1)]
            }])
            .contains("nothing can follow"),
            "a block after Break never runs"
        );
        assert!(
            bad(vec![Stmt::Do(NodeKind::Stop), wait(1)]).contains("nothing can follow"),
            "a block after Stop never runs"
        );
        assert!(
            bad(vec![Stmt::While {
                cond: AstCond::Expr("1".into()),
                body: vec![]
            }])
            .contains("at least one statement")
        );
        assert!(
            bad(vec![Stmt::While {
                cond: AstCond::Expr("1".into()),
                body: vec![Stmt::Continue]
            }])
            .contains("never ends")
        );
        assert!(bad(vec![Stmt::Do(NodeKind::Start)]).contains("not statements"));
        assert!(bad(vec![Stmt::Do(NodeKind::Repeat(2))]).contains("`Repeat` statement"));
        assert!(
            bad(vec![Stmt::If {
                cond: AstCond::Expr("1 +".into()),
                then: vec![],
                otherwise: vec![]
            }])
            .contains("is wrong")
        );
        assert!(
            bad(vec![Stmt::Let {
                name: "1x".into(),
                expr: "1".into()
            }])
            .contains("cannot be the name")
        );
        assert!(
            bad(vec![Stmt::Let {
                name: "x".into(),
                expr: "1 +".into()
            }])
            .contains("is wrong")
        );
        assert!(
            bad(vec![Stmt::Repeat {
                times: MAX_REPEAT + 1,
                body: vec![wait(1)]
            }])
            .contains("at most")
        );
        let mut ast = program(vec![]);
        ast.cursorpie_flow_ast = AST_FORMAT + 1;
        assert!(compile(&ast).unwrap_err().contains("newer CursorPie"));
        ast.cursorpie_flow_ast = 0;
        assert!(compile(&ast).unwrap_err().contains("format 0"));
    }

    #[test]
    fn a_program_is_bounded_in_blocks_and_in_depth() {
        let many = program((0..MAX_FLOW_NODES as u32).map(wait).collect());
        assert!(compile(&many).unwrap_err().contains("more than"));
        let fits = program((0..(MAX_FLOW_NODES - 2) as u32).map(wait).collect());
        assert_eq!(compile(&fits).unwrap().nodes.len(), MAX_FLOW_NODES);
        let mut deep = vec![wait(1)];
        for _ in 0..MAX_AST_DEPTH + 2 {
            deep = vec![Stmt::If {
                cond: AstCond::Expr("1".into()),
                then: deep,
                otherwise: vec![],
            }];
        }
        assert!(compile(&program(deep)).unwrap_err().contains("levels deep"));
    }

    #[test]
    fn the_json_of_a_tree_reads_and_writes_the_same() {
        let json = r#"{
          "cursorpie_flow_ast": 1,
          "name": "Odd numbers",
          "body": [
            { "Let": { "name": "n", "expr": "0" } },
            { "While": { "cond": "n < 4", "body": [
                { "Let": { "name": "n", "expr": "n + 1" } },
                { "If": { "cond": "n % 2 == 0", "then": ["Continue"] } },
                { "If": { "cond": "n == 3", "then": ["Break"] } },
                { "Do": { "Key": { "key": 8, "mods": 2 } } }
            ] } },
            { "If": { "cond": { "a": "n", "op": "Ge", "b": "4" },
                      "then": [ { "Do": { "OpenApp": "done" } } ],
                      "else": [ { "Do": "Stop" } ] } }
          ]
        }"#;
        let ast = tree(json);
        assert_eq!(ast.name, "Odd numbers");
        let back = serde_json::to_string(&ast).unwrap();
        assert_eq!(tree(&back), ast);
    }

    #[test]
    fn importing_a_tree_file_gives_a_clean_flow() {
        let ast = program(vec![
            Stmt::Let {
                name: "n".into(),
                expr: "1 + 1".into(),
            },
            Stmt::Repeat {
                times: 2,
                body: vec![key(8)],
            },
        ]);
        let text = serde_json::to_string(&ast).unwrap();
        let report = parse_flow_report(&text).unwrap();
        assert_eq!(report.flow.name, "t");
        no_repairs(&report);
        // the plain reader takes it too
        let f = parse_flow(&text).unwrap();
        assert_eq!(f.nodes.len(), 5);
        assert_eq!(f.format(), 2, "a Set and a loop need the second format");
    }

    #[test]
    fn a_damaged_tree_file_says_so() {
        let e = parse_flow_report(r#"{ "cursorpie_flow_ast": 1, "body": 3 }"#).unwrap_err();
        assert!(e.contains("damaged"), "{e}");
        let e = parse_flow_report(r#"{ "cursorpie_flow_ast": 1, "body": [ { "Nope": 1 } ] }"#)
            .unwrap_err();
        assert!(e.contains("damaged"), "{e}");
    }

    /// A tree, compiled, written back as a tree and compiled again: the same graph.
    fn round_trip(ast: &FlowAst) -> FlowAst {
        let f = compile(ast).unwrap();
        let back = flow_to_ast(&f).unwrap_or_else(|e| panic!("{e}\n{ast:?}"));
        assert_eq!(shape(&compile(&back).unwrap()), shape(&f));
        back
    }

    #[test]
    fn a_structured_flow_is_written_back_as_the_tree_it_came_from() {
        let ast = program(vec![
            Stmt::Let {
                name: "n".into(),
                expr: "0".into(),
            },
            Stmt::While {
                cond: AstCond::Expr("n < 4".into()),
                body: vec![
                    Stmt::Let {
                        name: "n".into(),
                        expr: "n + 1".into(),
                    },
                    Stmt::If {
                        cond: AstCond::Expr("n % 2 == 0".into()),
                        then: vec![Stmt::Continue],
                        otherwise: vec![],
                    },
                    key(8),
                ],
            },
            Stmt::If {
                cond: AstCond::Cmp(Cond {
                    a: "n".into(),
                    op: CmpOp::Ge,
                    b: "4".into(),
                }),
                then: vec![Stmt::Do(NodeKind::OpenApp("done".into()))],
                otherwise: vec![Stmt::Do(NodeKind::Stop)],
            },
        ]);
        let back = round_trip(&ast);
        // a Let is a Set block with an expression: it comes back as the block it is
        let again = compile(&back).unwrap();
        assert_eq!(shape(&again), shape(&compile(&ast).unwrap()));
        // and the condition that is an expression comes back as an expression
        let Stmt::While { cond, .. } = &back.body[1] else {
            panic!("a While: {back:?}");
        };
        assert_eq!(cond, &AstCond::Expr("n < 4".into()));
    }

    #[test]
    fn nested_structures_and_every_loop_kind_round_trip() {
        let inner = Stmt::ForEach {
            list: "items".into(),
            item: "it".into(),
            body: vec![
                Stmt::If {
                    cond: AstCond::Expr("it == \"x\"".into()),
                    then: vec![wait(1), Stmt::Continue],
                    otherwise: vec![wait(2)],
                },
                wait(3),
            ],
        };
        let ast = program(vec![
            wait(0),
            Stmt::Repeat {
                times: 2,
                body: vec![
                    inner,
                    Stmt::Repeat {
                        times: 3,
                        body: vec![wait(4)],
                    },
                ],
            },
            Stmt::If {
                cond: AstCond::Expr("a".into()),
                then: vec![Stmt::If {
                    cond: AstCond::Expr("b".into()),
                    then: vec![wait(5)],
                    otherwise: vec![wait(6)],
                }],
                otherwise: vec![wait(7)],
            },
            wait(8),
        ]);
        round_trip(&ast);
    }

    #[test]
    fn break_and_continue_round_trip_in_nested_loops() {
        let ast = program(vec![
            Stmt::Repeat {
                times: 2,
                body: vec![
                    Stmt::While {
                        cond: AstCond::Expr("a".into()),
                        body: vec![
                            Stmt::If {
                                cond: AstCond::Expr("b".into()),
                                then: vec![wait(1), Stmt::Break],
                                otherwise: vec![Stmt::If {
                                    cond: AstCond::Expr("c".into()),
                                    then: vec![Stmt::Continue],
                                    otherwise: vec![wait(2)],
                                }],
                            },
                            wait(3),
                        ],
                    },
                    Stmt::If {
                        cond: AstCond::Expr("d".into()),
                        then: vec![Stmt::Break],
                        otherwise: vec![],
                    },
                    wait(4),
                ],
            },
            wait(5),
        ]);
        let back = round_trip(&ast);
        // the Break of the inner loop is still in the tree
        let text = serde_json::to_string(&back).unwrap();
        assert!(text.contains("\"Break\""), "{text}");
    }

    #[test]
    fn a_side_that_ends_the_run_is_kept() {
        // `if a { stop }` and then more: the other side goes on to the rest
        let ast = program(vec![
            Stmt::If {
                cond: AstCond::Expr("a".into()),
                then: vec![Stmt::Do(NodeKind::Stop)],
                otherwise: vec![],
            },
            wait(1),
            wait(2),
        ]);
        round_trip(&ast);
    }

    #[test]
    fn a_flow_that_is_not_structured_is_refused_and_never_exported_wrong() {
        // a wire that jumps from one side of an If into the middle of the other side
        let mut f = Flow::new("f", "F");
        let b = f.add_after(
            None,
            NodeKind::Branch(Cond {
                a: "x".into(),
                op: CmpOp::IsTrue,
                b: String::new(),
            }),
        );
        let w1 = f.add_after(Some(b), NodeKind::Wait(1));
        let w2 = f.add_after(Some(w1), NodeKind::Wait(2));
        assert!(f.connect(b, Port::Next, w1));
        assert!(f.connect(b, Port::Alt, w2));
        f.node_mut(w2).unwrap().next = Some(w1);
        let e = flow_to_ast(&f).unwrap_err();
        assert!(e.contains("cannot be written as a program tree"), "{e}");
    }

    #[test]
    fn a_flow_with_an_open_wire_is_refused() {
        let mut f = Flow::new("f", "F");
        let w = f.add_after(None, NodeKind::Wait(1));
        f.node_mut(w).unwrap().next = None;
        let e = flow_to_ast(&f).unwrap_err();
        assert!(e.contains("wire is missing"), "{e}");
    }

    #[test]
    fn the_flows_of_the_blocks_editor_that_are_straight_lines_export() {
        let mut f = Flow::new("f", "My flow");
        f.add_after(None, NodeKind::Wait(10));
        f.add_after(None, NodeKind::OpenUrl("https://example.com".into()));
        let text = export_flow_ast(&f).unwrap();
        let ast = tree(&text);
        assert_eq!(ast.name, "My flow");
        assert_eq!(ast.body.len(), 2);
        // and the file is read back by the ordinary importer
        let g = parse_flow(&text).unwrap();
        assert_eq!(shape(&g), shape(&f));
    }

    #[test]
    fn the_example_file_is_a_valid_program() {
        let text =
            include_str!("../../docs/examples/flows-ast/odd-numbers.cursorpie-flow-ast.json");
        let report = parse_flow_report(text).unwrap();
        no_repairs(&report);
        // and it writes back as a tree
        let back = flow_to_ast(&report.flow).unwrap();
        assert_eq!(shape(&compile(&back).unwrap()), shape(&report.flow));
    }
}
