//! A flow as a **list of steps**: the view the Flow tab opens with.
//!
//! The model of a flow is a graph (blocks and wires, see [`flow`](super::flow)). Most flows are
//! not a tangle but a *structure*: steps one after another, an *If / else* with a *then* and an
//! *else* part that meet again, and loops whose body leads back to the loop block. Such a flow can
//! be shown, and edited, as a nested list, the way Shortcuts, Automator or Zapier show theirs:
//!
//! ```text
//! 1  Shortcut  Cmd + C
//! 2  If  clip is true
//!      then   3  Notification
//!      else   4  Wait
//! 5  Repeat 3 times
//!      body   6  Shortcut  Down
//! 7  Open URL
//! ```
//!
//! This module turns the graph into that tree ([`outline`]), edits the tree and writes it back
//! into the wires ([`insert_block`], [`remove_block`], [`move_block`]). It adds no data: the file
//! on disk, the share file and the interpreter are untouched. A flow that is *not* a structure (a
//! wire that jumps into the middle of a loop, an output left unplugged, two paths that join in an
//! odd place) gives `None`, and the editor then sends the person to the canvas, which can draw
//! any graph.
//!
//! Where a block sits on the canvas is not part of the structure. The blocks this module adds get
//! the place `(0, 0)`; the editor lines the canvas up (`tidy`) when the person opens it.

use super::flow::{Flow, FlowNode, NodeKind, Port};
use std::collections::{BTreeSet, VecDeque};

/// Which part of a block that holds other steps a list belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arm {
    /// The *then* part of an *If / else*.
    Then,
    /// The *else* part of an *If / else*.
    Else,
    /// The body of a loop (*Repeat*, *While*, *For each*).
    Body,
}

/// One entry of a list of steps.
#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// A block with one way out; *Stop* too, which has none and so ends the list it is in.
    Step(u32),
    /// An *If / else*; after both parts the list goes on with the entry that follows.
    Branch {
        id: u32,
        then: Vec<Item>,
        otherwise: Vec<Item>,
    },
    /// A loop: its body runs round after round, then the list goes on.
    Loop { id: u32, body: Vec<Item> },
}

impl Item {
    /// The block this entry is.
    pub fn id(&self) -> u32 {
        match self {
            Item::Step(id) | Item::Branch { id, .. } | Item::Loop { id, .. } => *id,
        }
    }

    /// The lists inside this entry, with the part each one is.
    pub fn arms(&self) -> Vec<(Arm, &Vec<Item>)> {
        match self {
            Item::Step(_) => Vec::new(),
            Item::Branch {
                then, otherwise, ..
            } => vec![(Arm::Then, then), (Arm::Else, otherwise)],
            Item::Loop { body, .. } => vec![(Arm::Body, body)],
        }
    }

    fn arms_mut(&mut self) -> Vec<(Arm, &mut Vec<Item>)> {
        match self {
            Item::Step(_) => Vec::new(),
            Item::Branch {
                then, otherwise, ..
            } => vec![(Arm::Then, then), (Arm::Else, otherwise)],
            Item::Loop { body, .. } => vec![(Arm::Body, body)],
        }
    }

    /// Every block id in this entry, itself first.
    fn ids(&self, out: &mut Vec<u32>) {
        out.push(self.id());
        for (_, arm) in self.arms() {
            for it in arm {
                it.ids(out);
            }
        }
    }

    /// Whether the entry is, or holds, the block `id`.
    pub fn contains(&self, id: u32) -> bool {
        self.id() == id
            || self
                .arms()
                .iter()
                .any(|(_, a)| a.iter().any(|i| i.contains(id)))
    }

    /// Whether nothing can come after this entry: *Stop*, or an *If / else* whose two parts both end.
    fn ends(&self, flow_stop: &dyn Fn(u32) -> bool) -> bool {
        match self {
            Item::Step(id) => flow_stop(*id),
            Item::Branch {
                then, otherwise, ..
            } => seq_ends(then, flow_stop) && seq_ends(otherwise, flow_stop),
            Item::Loop { .. } => false,
        }
    }
}

fn seq_ends(seq: &[Item], flow_stop: &dyn Fn(u32) -> bool) -> bool {
    seq.last().is_some_and(|i| i.ends(flow_stop))
}

/// A flow as a list of steps.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    /// The steps between *Start* and *End*.
    pub items: Vec<Item>,
    /// Blocks no wire leads to: they never run. In the order of the flow.
    pub loose: Vec<u32>,
}

/// A place in the list where a step can go: in the list of `parent` (`None`: the main list), at
/// position `index`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub parent: Option<(u32, Arm)>,
    pub index: usize,
}

impl Spot {
    /// The end of the main list.
    pub const END: Spot = Spot {
        parent: None,
        index: usize::MAX,
    };
}

impl Outline {
    /// The list at `parent`.
    pub fn list(&self, parent: Option<(u32, Arm)>) -> Option<&Vec<Item>> {
        fn find(seq: &[Item], id: u32, arm: Arm) -> Option<&Vec<Item>> {
            for it in seq {
                if it.id() == id {
                    return it
                        .arms()
                        .into_iter()
                        .find(|(a, _)| *a == arm)
                        .map(|(_, v)| v);
                }
                for (_, a) in it.arms() {
                    if let Some(v) = find(a, id, arm) {
                        return Some(v);
                    }
                }
            }
            None
        }
        match parent {
            None => Some(&self.items),
            Some((id, arm)) => find(&self.items, id, arm),
        }
    }

    fn list_mut(&mut self, parent: Option<(u32, Arm)>) -> Option<&mut Vec<Item>> {
        fn find(seq: &mut [Item], id: u32, arm: Arm) -> Option<&mut Vec<Item>> {
            for it in seq {
                if it.id() == id {
                    return it
                        .arms_mut()
                        .into_iter()
                        .find(|(a, _)| *a == arm)
                        .map(|(_, v)| v);
                }
                for (_, a) in it.arms_mut() {
                    if let Some(v) = find(a, id, arm) {
                        return Some(v);
                    }
                }
            }
            None
        }
        match parent {
            None => Some(&mut self.items),
            Some((id, arm)) => find(&mut self.items, id, arm),
        }
    }

    /// Every block id in the list, in the order they are shown.
    pub fn ids(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for it in &self.items {
            it.ids(&mut out);
        }
        out
    }

    /// How many steps there are.
    pub fn len(&self) -> usize {
        self.ids().len()
    }

    /// Whether the main list has no step.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

// ------------------------------------------------------------------ graph -> list

/// How a list ended: at a block it leads into, or by itself (a *Stop*).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Exit {
    At(u32),
    Over,
}

struct Reader<'a> {
    flow: &'a Flow,
    seen: BTreeSet<u32>,
}

impl Reader<'_> {
    /// The blocks reachable from `from`, in the order a walk by distance finds them. The walk goes
    /// into a block of `bounds` but not beyond it.
    fn walk(&self, from: u32, bounds: &[u32]) -> Vec<u32> {
        let mut out = Vec::new();
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::new();
        seen.insert(from);
        queue.push_back(from);
        while let Some(id) = queue.pop_front() {
            out.push(id);
            if bounds.contains(&id) {
                continue;
            }
            if let Some(n) = self.flow.node(id) {
                for t in n.wires().into_iter().flatten() {
                    if seen.insert(t) {
                        queue.push_back(t);
                    }
                }
            }
        }
        out
    }

    /// The first block both parts of an *If / else* lead into.
    fn join(&self, then: u32, otherwise: u32, bounds: &[u32]) -> Option<u32> {
        if then == otherwise {
            return Some(then);
        }
        let from_then: BTreeSet<u32> = self.walk(then, bounds).into_iter().collect();
        self.walk(otherwise, bounds)
            .into_iter()
            .find(|id| from_then.contains(id))
    }

    /// Reads the list that starts at `cur` and ends where it reaches one of `bounds` (or at a *Stop*).
    fn list(&mut self, mut cur: Option<u32>, bounds: &[u32]) -> Option<(Vec<Item>, Exit)> {
        let mut items = Vec::new();
        loop {
            let id = cur?;
            if bounds.contains(&id) {
                return Some((items, Exit::At(id)));
            }
            let node = self.flow.node(id)?;
            if matches!(node.kind, NodeKind::Start | NodeKind::End) || !self.seen.insert(id) {
                return None;
            }
            match &node.kind {
                NodeKind::Stop => {
                    items.push(Item::Step(id));
                    return Some((items, Exit::Over));
                }
                NodeKind::Branch(_) => {
                    let (then, other) = (node.next?, node.alt?);
                    match self.join(then, other, bounds) {
                        Some(join) => {
                            let mut inner = bounds.to_vec();
                            inner.push(join);
                            let (t, te) = self.list(Some(then), &inner)?;
                            let (o, oe) = self.list(Some(other), &inner)?;
                            for e in [te, oe] {
                                if e != Exit::Over && e != Exit::At(join) {
                                    return None;
                                }
                            }
                            items.push(Item::Branch {
                                id,
                                then: t,
                                otherwise: o,
                            });
                            cur = Some(join);
                        }
                        None => {
                            // no common block: at most one part goes on, the other one ends
                            let (t, te) = self.list(Some(then), bounds)?;
                            let (o, oe) = self.list(Some(other), bounds)?;
                            match (te, oe) {
                                (Exit::Over, Exit::Over) => {
                                    items.push(Item::Branch {
                                        id,
                                        then: t,
                                        otherwise: o,
                                    });
                                    return Some((items, Exit::Over));
                                }
                                (Exit::Over, e) => {
                                    items.push(Item::Branch {
                                        id,
                                        then: t,
                                        otherwise: Vec::new(),
                                    });
                                    items.extend(o);
                                    return Some((items, e));
                                }
                                (e, Exit::Over) => {
                                    items.push(Item::Branch {
                                        id,
                                        then: Vec::new(),
                                        otherwise: o,
                                    });
                                    items.extend(t);
                                    return Some((items, e));
                                }
                                _ => return None,
                            }
                        }
                    }
                }
                NodeKind::Repeat(_) | NodeKind::While(_) | NodeKind::ForEach { .. } => {
                    let after = node.alt?;
                    let body = match node.next {
                        None => Vec::new(),
                        Some(first) => {
                            let mut inner = bounds.to_vec();
                            inner.push(id);
                            let (b, e) = self.list(Some(first), &inner)?;
                            if e != Exit::Over && e != Exit::At(id) {
                                return None;
                            }
                            b
                        }
                    };
                    items.push(Item::Loop { id, body });
                    cur = Some(after);
                }
                _ => {
                    items.push(Item::Step(id));
                    cur = node.next;
                }
            }
        }
    }
}

/// The flow as a list of steps, or `None` when its wires are not a structure the list can show
/// (see the module).
pub fn outline(flow: &Flow) -> Option<Outline> {
    let start = flow.start()?;
    let end = flow.end()?.id;
    let mut r = Reader {
        flow,
        seen: BTreeSet::new(),
    };
    let items = match start.next {
        None => Vec::new(),
        Some(first) => {
            let (items, exit) = r.list(Some(first), &[end])?;
            if exit != Exit::Over && exit != Exit::At(end) {
                return None;
            }
            items
        }
    };
    let loose = flow
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind, NodeKind::Start | NodeKind::End) && !r.seen.contains(&n.id))
        .map(|n| n.id)
        .collect();
    Some(Outline { items, loose })
}

// ------------------------------------------------------------------ list -> graph

/// Wires the blocks of `seq` one after another, the last one into `tail`. Returns the block the
/// list starts with (`tail` for an empty list).
fn link_list(nodes: &mut [FlowNode], seq: &[Item], tail: u32) -> u32 {
    let mut cont = tail;
    for it in seq.iter().rev() {
        cont = link_item(nodes, it, cont);
    }
    cont
}

fn link_item(nodes: &mut [FlowNode], it: &Item, cont: u32) -> u32 {
    let set = |nodes: &mut [FlowNode], id: u32, next: Option<u32>, alt: Option<u32>| {
        if let Some(n) = nodes.iter_mut().find(|n| n.id == id) {
            n.next = next;
            n.alt = alt;
        }
    };
    match it {
        Item::Step(id) => {
            let stop = nodes
                .iter()
                .any(|n| n.id == *id && matches!(n.kind, NodeKind::Stop));
            set(nodes, *id, if stop { None } else { Some(cont) }, None);
            *id
        }
        Item::Branch {
            id,
            then,
            otherwise,
        } => {
            let t = link_list(nodes, then, cont);
            let o = link_list(nodes, otherwise, cont);
            set(nodes, *id, Some(t), Some(o));
            *id
        }
        Item::Loop { id, body } => {
            let first = if body.is_empty() {
                None
            } else {
                Some(link_list(nodes, body, *id))
            };
            set(nodes, *id, first, Some(cont));
            *id
        }
    }
}

/// Writes `items` into the wires of `flow`: *Start*, the steps in order, *End*.
fn write(flow: &mut Flow, items: &[Item]) {
    let (Some(start), Some(end)) = (flow.start().map(|n| n.id), flow.end().map(|n| n.id)) else {
        return;
    };
    let first = link_list(&mut flow.nodes, items, end);
    if let Some(s) = flow.node_mut(start) {
        s.next = Some(first);
        s.alt = None;
    }
}

/// Whether every *Stop* is the last entry of its list: nothing after it could ever run.
fn valid(flow: &Flow, items: &[Item]) -> bool {
    let stop = |id: u32| {
        flow.node(id)
            .is_some_and(|n| matches!(n.kind, NodeKind::Stop))
    };
    fn walk(seq: &[Item], stop: &dyn Fn(u32) -> bool) -> bool {
        seq.iter().enumerate().all(|(i, it)| {
            let last = i + 1 == seq.len();
            (last || !it.ends(stop)) && it.arms().iter().all(|(_, a)| walk(a, stop))
        })
    }
    walk(items, &stop)
}

/// Whether a step can be added right after `item` (not after one that ends the list).
pub fn can_follow(flow: &Flow, item: &Item) -> bool {
    !item.ends(&|id| {
        flow.node(id)
            .is_some_and(|n| matches!(n.kind, NodeKind::Stop))
    })
}

/// Whether the list `seq` ends (so no step can be added at its end).
pub fn list_ends(flow: &Flow, seq: &[Item]) -> bool {
    seq.last().is_some_and(|i| !can_follow(flow, i))
}

/// Adds a block of `kind` at `spot` and wires it in. Returns its id, or `None` when the flow is
/// not a list, the place does not exist, the flow is full or the block would end a list that goes
/// on (a *Stop* that is not last).
pub fn insert_block(flow: &mut Flow, spot: Spot, kind: NodeKind) -> Option<u32> {
    if flow.nodes.len() >= super::MAX_FLOW_NODES {
        return None;
    }
    let mut o = outline(flow)?;
    let id = flow.next_node_id();
    let item = match &kind {
        NodeKind::Branch(_) => Item::Branch {
            id,
            then: Vec::new(),
            otherwise: Vec::new(),
        },
        NodeKind::Repeat(_) | NodeKind::While(_) | NodeKind::ForEach { .. } => Item::Loop {
            id,
            body: Vec::new(),
        },
        NodeKind::Start | NodeKind::End => return None,
        _ => Item::Step(id),
    };
    let list = o.list_mut(spot.parent)?;
    let at = spot.index.min(list.len());
    list.insert(at, item);
    flow.nodes.push(FlowNode {
        id,
        x: 0.0,
        y: 0.0,
        kind,
        next: None,
        alt: None,
    });
    if !valid(flow, &o.items) {
        flow.nodes.pop();
        return None;
    }
    write(flow, &o.items);
    Some(id)
}

/// Removes the block `id` and whatever is inside it. Returns whether something was removed.
pub fn remove_block(flow: &mut Flow, id: u32) -> bool {
    fn take(seq: &mut Vec<Item>, id: u32) -> Option<Item> {
        if let Some(i) = seq.iter().position(|it| it.id() == id) {
            return Some(seq.remove(i));
        }
        for it in seq.iter_mut() {
            for (_, arm) in it.arms_mut() {
                if let Some(x) = take(arm, id) {
                    return Some(x);
                }
            }
        }
        None
    }
    let Some(mut o) = outline(flow) else {
        return false;
    };
    let Some(gone) = take(&mut o.items, id) else {
        return false;
    };
    let mut ids = Vec::new();
    gone.ids(&mut ids);
    flow.nodes.retain(|n| !ids.contains(&n.id));
    write(flow, &o.items);
    true
}

enum Moved {
    No,
    Done,
    /// The entry was at the edge of its list and left it.
    Out(Item),
}

fn move_in(seq: &mut Vec<Item>, id: u32, up: bool) -> Moved {
    if let Some(i) = seq.iter().position(|it| it.id() == id) {
        let j = if up {
            i.checked_sub(1)
        } else {
            (i + 1 < seq.len()).then_some(i + 1)
        };
        return match j {
            Some(j) => {
                seq.swap(i, j);
                Moved::Done
            }
            None => Moved::Out(seq.remove(i)),
        };
    }
    for k in 0..seq.len() {
        let mut out = None;
        let mut found = false;
        for (_, arm) in seq[k].arms_mut() {
            match move_in(arm, id, up) {
                Moved::No => {}
                Moved::Done => {
                    found = true;
                    break;
                }
                Moved::Out(it) => {
                    out = Some(it);
                    found = true;
                    break;
                }
            }
        }
        if let Some(it) = out {
            seq.insert(if up { k } else { k + 1 }, it);
            return Moved::Done;
        }
        if found {
            return Moved::Done;
        }
    }
    Moved::No
}

/// Moves the block `id` one place up or down in its list. At the edge of a list inside an
/// *If / else* or a loop it moves out of it, just before or after that block. Returns whether it
/// moved.
pub fn move_block(flow: &mut Flow, id: u32, up: bool) -> bool {
    let Some(mut o) = outline(flow) else {
        return false;
    };
    match move_in(&mut o.items, id, up) {
        Moved::Done => {}
        // at the edge of the main list: nowhere to go
        Moved::Out(_) | Moved::No => return false,
    }
    if !valid(flow, &o.items) {
        return false;
    }
    write(flow, &o.items);
    true
}

/// The wires a block's output has, for tests and for the editor to tell what a step leads to.
pub fn follows(flow: &Flow, id: u32, port: Port) -> Option<u32> {
    flow.node(id).and_then(|n| n.wire(port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::flow::{CalcOp, Cond, ListOp};
    use crate::keys::{Shortcut, key};

    fn key_block() -> NodeKind {
        NodeKind::Key(Shortcut::new(key::F13, 0))
    }
    fn wait() -> NodeKind {
        NodeKind::Wait(10)
    }
    fn branch() -> NodeKind {
        NodeKind::Branch(Cond::default())
    }
    fn rep() -> NodeKind {
        NodeKind::Repeat(2)
    }
    fn top(flow: &Flow) -> Vec<u32> {
        outline(flow)
            .expect("a structure")
            .items
            .iter()
            .map(Item::id)
            .collect()
    }
    fn add(flow: &mut Flow, spot: Spot, kind: NodeKind) -> u32 {
        insert_block(flow, spot, kind).expect("it fits")
    }
    fn at(parent: Option<(u32, Arm)>, index: usize) -> Spot {
        Spot { parent, index }
    }

    #[test]
    fn a_new_flow_is_an_empty_list() {
        let f = Flow::new("f", "F");
        let o = outline(&f).unwrap();
        assert!(o.items.is_empty() && o.loose.is_empty());
    }

    #[test]
    fn steps_go_in_where_they_are_added_and_are_wired_in_order() {
        let mut f = Flow::new("f", "F");
        let a = add(&mut f, Spot::END, key_block());
        let b = add(&mut f, Spot::END, wait());
        let c = add(&mut f, at(None, 1), NodeKind::OpenUrl(String::new()));
        assert_eq!(top(&f), vec![a, c, b]);
        let chain: Vec<u32> = f.chain().iter().map(|n| n.id).collect();
        assert_eq!(chain[1..4], [a, c, b]);
        assert_eq!(f.chain().last().unwrap().kind, NodeKind::End);
    }

    #[test]
    fn a_branch_keeps_its_two_parts_and_both_meet_after_it() {
        let mut f = Flow::new("f", "F");
        let br = add(&mut f, Spot::END, branch());
        let t = add(&mut f, at(Some((br, Arm::Then)), 0), key_block());
        let e = add(&mut f, at(Some((br, Arm::Else)), 0), wait());
        let after = add(&mut f, Spot::END, NodeKind::OpenUrl(String::new()));
        let o = outline(&f).unwrap();
        assert_eq!(o.items.len(), 2);
        assert_eq!(
            o.items[0],
            Item::Branch {
                id: br,
                then: vec![Item::Step(t)],
                otherwise: vec![Item::Step(e)]
            }
        );
        assert_eq!(follows(&f, t, Port::Next), Some(after));
        assert_eq!(follows(&f, e, Port::Next), Some(after));
    }

    #[test]
    fn a_loop_body_leads_back_to_the_loop_and_done_leads_on() {
        let mut f = Flow::new("f", "F");
        let r = add(&mut f, Spot::END, rep());
        let k = add(&mut f, at(Some((r, Arm::Body)), 0), key_block());
        let after = add(&mut f, Spot::END, wait());
        assert_eq!(follows(&f, r, Port::Next), Some(k));
        assert_eq!(follows(&f, k, Port::Next), Some(r));
        assert_eq!(follows(&f, r, Port::Alt), Some(after));
        assert_eq!(top(&f), vec![r, after]);
    }

    #[test]
    fn an_empty_loop_has_no_body_wire() {
        let mut f = Flow::new("f", "F");
        let r = add(&mut f, Spot::END, rep());
        assert_eq!(follows(&f, r, Port::Next), None);
        assert!(outline(&f).is_some());
    }

    #[test]
    fn nested_loops_and_branches_round_trip() {
        let mut f = Flow::new("f", "F");
        let outer = add(&mut f, Spot::END, rep());
        let br = add(&mut f, at(Some((outer, Arm::Body)), 0), branch());
        let inner = add(
            &mut f,
            at(Some((br, Arm::Then)), 0),
            NodeKind::While(Cond::default()),
        );
        add(&mut f, at(Some((inner, Arm::Body)), 0), key_block());
        add(&mut f, at(Some((br, Arm::Else)), 0), wait());
        add(&mut f, at(Some((outer, Arm::Body)), 1), key_block());
        add(&mut f, Spot::END, wait());
        let o = outline(&f).unwrap();
        // writing the list again changes nothing
        let before = f.clone();
        write(&mut f, &o.items);
        assert_eq!(f, before);
        assert_eq!(outline(&f).unwrap(), o);
    }

    #[test]
    fn stop_ends_a_list_and_cannot_be_put_in_the_middle() {
        let mut f = Flow::new("f", "F");
        let a = add(&mut f, Spot::END, key_block());
        add(&mut f, Spot::END, NodeKind::Stop);
        // in front of the Stop is fine
        assert!(insert_block(&mut f, at(None, 1), wait()).is_some());
        // a Stop between two steps would cut the second off
        assert!(insert_block(&mut f, at(None, 0), NodeKind::Stop).is_none());
        let o = outline(&f).unwrap();
        assert!(list_ends(&f, &o.items));
        assert!(!can_follow(&f, o.items.last().unwrap()));
        assert!(can_follow(&f, &o.items[0]));
        assert_eq!(o.items[0].id(), a);
    }

    #[test]
    fn if_with_a_stop_on_one_side_reads_back_the_same() {
        let mut f = Flow::new("f", "F");
        let br = add(&mut f, Spot::END, branch());
        add(&mut f, at(Some((br, Arm::Then)), 0), NodeKind::Stop);
        let w = add(&mut f, Spot::END, wait());
        let o = outline(&f).unwrap();
        assert_eq!(top(&f), vec![br, w]);
        assert!(matches!(&o.items[0], Item::Branch { otherwise, .. } if otherwise.is_empty()));
        // else goes straight on to the wait
        assert_eq!(follows(&f, br, Port::Alt), Some(w));
    }

    #[test]
    fn removing_a_block_takes_what_is_inside_and_joins_the_neighbours() {
        let mut f = Flow::new("f", "F");
        let a = add(&mut f, Spot::END, key_block());
        let r = add(&mut f, Spot::END, rep());
        let inside = add(&mut f, at(Some((r, Arm::Body)), 0), wait());
        let z = add(&mut f, Spot::END, key_block());
        assert!(remove_block(&mut f, r));
        assert!(f.node(r).is_none() && f.node(inside).is_none());
        assert_eq!(top(&f), vec![a, z]);
        assert_eq!(follows(&f, a, Port::Next), Some(z));
        assert!(!remove_block(&mut f, 9999));
    }

    #[test]
    fn moving_swaps_neighbours_and_steps_out_at_the_edge() {
        let mut f = Flow::new("f", "F");
        let a = add(&mut f, Spot::END, key_block());
        let b = add(&mut f, Spot::END, wait());
        assert!(move_block(&mut f, b, true));
        assert_eq!(top(&f), vec![b, a]);
        assert!(!move_block(&mut f, b, true), "already first");
        assert!(!move_block(&mut f, a, false), "already last");
        // out of a loop body, before and after the loop
        let r = add(&mut f, Spot::END, rep());
        let x = add(&mut f, at(Some((r, Arm::Body)), 0), key_block());
        assert!(move_block(&mut f, x, true));
        assert_eq!(top(&f), vec![b, a, x, r]);
        let y = add(&mut f, at(Some((r, Arm::Body)), 0), wait());
        assert!(move_block(&mut f, y, false));
        assert_eq!(top(&f), vec![b, a, x, r, y]);
    }

    #[test]
    fn a_wire_that_jumps_into_the_middle_is_not_a_list() {
        let mut f = Flow::new("f", "F");
        let a = add(&mut f, Spot::END, key_block());
        let b = add(&mut f, Spot::END, wait());
        let c = add(&mut f, Spot::END, key_block());
        // a loops back to c: not a loop block, so no structure
        f.connect(c, Port::Next, a);
        assert!(outline(&f).is_none());
        let _ = b;
    }

    #[test]
    fn an_unplugged_branch_is_not_a_list() {
        let mut f = Flow::new("f", "F");
        let br = add(&mut f, Spot::END, branch());
        f.unplug_out(br, Port::Alt);
        assert!(outline(&f).is_none());
    }

    #[test]
    fn blocks_no_wire_leads_to_are_loose() {
        let mut f = Flow::new("f", "F");
        let a = add(&mut f, Spot::END, key_block());
        let lone = f.add_at(wait(), 300.0, 300.0);
        let o = outline(&f).unwrap();
        assert_eq!(o.loose, vec![lone]);
        assert_eq!(top(&f), vec![a]);
    }

    #[test]
    fn a_flow_made_on_the_canvas_with_the_old_add_button_reads_as_a_list() {
        use crate::config::flow::NativeOp;
        let mut f = Flow::new("f", "F");
        let mut last = None;
        for kind in [
            key_block(),
            wait(),
            NodeKind::Set {
                name: "x".into(),
                value: "1".into(),
            },
            NodeKind::Calc {
                out: "x".into(),
                a: "x".into(),
                op: CalcOp::Add,
                b: "1".into(),
            },
            NodeKind::List {
                op: ListOp::Split,
                list: "xs".into(),
                a: String::new(),
                b: String::new(),
                out: String::new(),
            },
            NodeKind::Native {
                op: NativeOp::Notify,
                a: String::new(),
                b: String::new(),
            },
            NodeKind::Call(String::new()),
        ] {
            last = Some(f.add_after(last, kind));
        }
        assert_eq!(outline(&f).unwrap().len(), 7);
        // an If added there gets both outputs on what followed it, and steps added after it
        // go in its then part
        let br = f.add_after(last, branch());
        f.add_after(Some(br), key_block());
        let o = outline(&f).expect("then with a step, else straight on");
        assert_eq!(o.len(), 9);
    }

    #[test]
    fn every_block_a_person_can_add_can_go_in_a_list() {
        let mut f = Flow::new("f", "F");
        for kind in [
            key_block(),
            branch(),
            rep(),
            NodeKind::While(Cond::default()),
            NodeKind::ForEach {
                list: "l".into(),
                item: "i".into(),
            },
            NodeKind::Stop,
        ] {
            assert!(
                insert_block(&mut f, Spot::END, kind.clone()).is_some(),
                "{kind:?}"
            );
            // a Stop ends the main list: nothing goes after it
            if matches!(kind, NodeKind::Stop) {
                break;
            }
        }
        assert!(outline(&f).is_some());
        assert!(insert_block(&mut f, Spot::END, NodeKind::End).is_none());
    }

    /// Small deterministic generator, so the test needs no crate.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self, n: usize) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((self.0 >> 33) as usize) % n.max(1)
        }
    }

    #[test]
    fn random_edits_always_leave_a_list_that_reads_back_and_a_clean_flow() {
        for seed in 1..60u64 {
            let mut rng = Rng(seed);
            let mut f = Flow::new("f", "F");
            for _ in 0..40 {
                let Some(o) = outline(&f) else {
                    panic!("seed {seed}: the flow stopped being a list: {f:?}")
                };
                let ids = o.ids();
                match rng.next(10) {
                    0..=5 => {
                        // pick a list and a place in it
                        let mut lists: Vec<Option<(u32, Arm)>> = vec![None];
                        fn collect(seq: &[Item], out: &mut Vec<Option<(u32, Arm)>>) {
                            for it in seq {
                                for (arm, a) in it.arms() {
                                    out.push(Some((it.id(), arm)));
                                    collect(a, out);
                                }
                            }
                        }
                        collect(&o.items, &mut lists);
                        let parent = lists[rng.next(lists.len())];
                        let len = o.list(parent).map_or(0, Vec::len);
                        let kind = match rng.next(6) {
                            0 => branch(),
                            1 => rep(),
                            2 => NodeKind::Stop,
                            _ => key_block(),
                        };
                        let _ = insert_block(&mut f, at(parent, rng.next(len + 1)), kind);
                    }
                    6 | 7 if !ids.is_empty() => {
                        let id = ids[rng.next(ids.len())];
                        let _ = move_block(&mut f, id, rng.next(2) == 0);
                    }
                    _ if !ids.is_empty() => {
                        let id = ids[rng.next(ids.len())];
                        remove_block(&mut f, id);
                    }
                    _ => {}
                }
                let mut clean = f.clone();
                clean.clean();
                assert_eq!(clean, f, "seed {seed}: clean() would change the flow");
            }
        }
    }
}
