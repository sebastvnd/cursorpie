//! Moving and inserting slices anywhere in the tree of a pie: what the **pie editor** (the canvas of
//! the Menu tab) and the **import tray** (the split screen where the slices of an imported pie are
//! dragged into yours) do when a slice is dropped.
//!
//! Everything here works on plain `Vec<Item>` and paths (see the `tree` module), so the rules
//! are unit tested without a window:
//!
//! * a ring holds at most [`MAX_ITEMS`] slices;
//! * submenus nest at most [`MAX_DEPTH`] rings deep (what [`Config::sanitize`](super::Config)
//!   would otherwise cut away on Save);
//! * the top ring keeps at least one slice;
//! * a submenu cannot be dropped inside itself.
//!
//! Every error is a sentence for the status bar. Nothing is changed when an error is returned.
//!
//! The same module holds the little arithmetic of *where a pie sits on the canvas* ([`nudge_pies`],
//! [`settle_pies`], [`tidy_pies`]): the editor lets a submenu's pie be dragged away from the place
//! it is given, and that place is kept in [`Item::at`].

use super::sanitize::clean_items;
use super::{
    Item, ItemKind, MAX_DEPTH, MAX_ITEMS, PIE_AT_LIMIT, item_at, item_at_mut, item_runs_commands,
    items_at, ring_mut,
};
use std::collections::BTreeSet;

/// Where in a ring a slice lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Before the slice that is at this index (the others move on), like dropping on that slice.
    At(usize),
    /// After the last slice.
    End,
}

/// Where a slice ended up: the path of its ring and its index in it.
pub type Landed = (Vec<usize>, usize);

/// How many rings deep the slice reaches below its own ring: 0 for an ordinary slice and for an
/// empty submenu, 1 for a submenu of ordinary slices, and so on.
pub fn levels(item: &Item) -> usize {
    match &item.kind {
        ItemKind::Submenu(children) if !children.is_empty() => {
            1 + children.iter().map(levels).max().unwrap_or(0)
        }
        _ => 0,
    }
}

/// Whether `ring` (a path) is the submenu at `ring_of_item` + `[idx]` or inside it.
fn inside(ring: &[usize], ring_of_item: &[usize], idx: usize) -> bool {
    ring.len() > ring_of_item.len()
        && ring[..ring_of_item.len()] == *ring_of_item
        && ring[ring_of_item.len()] == idx
}

/// Whether a slice that reaches `levels` rings deep fits into the ring at `ring`, and how many
/// slices that ring has.
fn check_room(items: &[Item], ring: &[usize], item: &Item) -> Result<usize, String> {
    let Some(target) = items_at(items, ring) else {
        return Err("That ring does not exist any more.".into());
    };
    if target.len() >= MAX_ITEMS {
        return Err(format!("A ring holds {MAX_ITEMS} slices at most."));
    }
    if ring.len() + levels(item) >= MAX_DEPTH {
        return Err(format!(
            "Submenus nest {MAX_DEPTH} rings deep at most, and that slice would go deeper."
        ));
    }
    Ok(target.len())
}

/// Moves the pies of the submenus at `paths` (the path of a submenu is the path of the slice that
/// opens it) by `dx`, `dy` canvas units. A path that is empty (the top pie, which is where the
/// others are placed from), that no longer exists or that is not a submenu is skipped. Returns how
/// many pies moved.
pub fn nudge_pies<'a>(
    items: &mut [Item],
    paths: impl IntoIterator<Item = &'a Vec<usize>>,
    dx: f32,
    dy: f32,
) -> usize {
    let mut moved = 0;
    for path in paths {
        if let Some(it) = item_at_mut(items, path)
            && matches!(it.kind, ItemKind::Submenu(_))
        {
            let [x, y] = it.at.unwrap_or([0.0, 0.0]);
            it.at = Some([
                (x + dx).clamp(-PIE_AT_LIMIT, PIE_AT_LIMIT),
                (y + dy).clamp(-PIE_AT_LIMIT, PIE_AT_LIMIT),
            ]);
            moved += 1;
        }
    }
    moved
}

/// Ends a move: rounds the places of the pies at `paths` to whole canvas units (so the file does
/// not fill with digits), and takes the place away from a pie that is back where the editor puts it.
pub fn settle_pies<'a>(items: &mut [Item], paths: impl IntoIterator<Item = &'a Vec<usize>>) {
    for path in paths {
        if let Some(it) = item_at_mut(items, path)
            && let Some([x, y]) = it.at
        {
            let (x, y) = (x.round(), y.round());
            it.at = if x == 0.0 && y == 0.0 {
                None
            } else {
                Some([x, y])
            };
        }
    }
}

/// Whether any pie in `items` (at any depth) was moved from its place.
pub fn has_layout(items: &[Item]) -> bool {
    items.iter().any(|it| {
        it.at.is_some()
            || match &it.kind {
                ItemKind::Submenu(kids) => has_layout(kids),
                _ => false,
            }
    })
}

/// Puts every pie back where the editor places it. Returns how many were moved.
pub fn tidy_pies(items: &mut [Item]) -> usize {
    let mut n = 0;
    for it in items {
        if it.at.take().is_some() {
            n += 1;
        }
        if let ItemKind::Submenu(kids) = &mut it.kind {
            n += tidy_pies(kids);
        }
    }
    n
}

/// What the text of copied slices starts with on the clipboard (the slices follow as JSON), so a
/// paste can tell them from any other text, and a copy can cross from one pie or profile to another.
pub const CLIP_SLICES: &str = "cursorpie-pie-slices:";

/// The most a paste reads: a bigger clipboard text is not a copy of slices.
const CLIP_MAX_BYTES: usize = 1_000_000;

/// Whether `path` lies inside another path of `paths` (a slice inside a selected submenu goes with it).
fn inside_another(path: &[usize], paths: &BTreeSet<Vec<usize>>) -> bool {
    paths
        .iter()
        .any(|q| !q.is_empty() && q.len() < path.len() && path[..q.len()] == q[..])
}

/// The slices at `paths`, in the order of the paths, each with everything in it. The empty path (the
/// top pie), a path that is gone and a slice that lies inside another selected one are left out.
pub fn slices_at(items: &[Item], paths: &BTreeSet<Vec<usize>>) -> Vec<Item> {
    paths
        .iter()
        .filter(|p| !p.is_empty() && !inside_another(p, paths))
        .filter_map(|p| item_at(items, p).cloned())
        .collect()
}

/// The text for the clipboard when `slices` are copied: [`CLIP_SLICES`] and then the slices as
/// JSON. `None` when there is nothing to copy.
pub fn copy_slices(slices: &[Item]) -> Option<String> {
    if slices.is_empty() {
        return None;
    }
    let json = serde_json::to_string(slices).ok()?;
    Some(format!("{CLIP_SLICES}{json}"))
}

/// Takes the slices at `paths` out of the tree (what *cut* does after the copy). The same slices
/// as [`slices_at`] are meant; the top ring keeps at least one slice. Returns how many went.
pub fn remove_slices(items: &mut Vec<Item>, paths: &BTreeSet<Vec<usize>>) -> usize {
    let mut gone = 0;
    // the later slices first, so the places of the earlier ones do not move under it
    for path in paths.iter().rev() {
        let Some((last, ring_path)) = path.split_last() else {
            continue;
        };
        if inside_another(path, paths) {
            continue;
        }
        let top = ring_path.is_empty();
        let Some(ring) = ring_mut(items, ring_path) else {
            continue;
        };
        if *last < ring.len() && (!top || ring.len() > 1) {
            ring.remove(*last);
            gone += 1;
        }
    }
    gone
}

/// What [`paste_slices`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasteSlices {
    /// The text is not a copy of slices (or the ring is gone): nothing happened.
    NotSlices,
    /// The ring has no room for the copies: nothing happened.
    Full,
    /// A copy would nest deeper than a pie can: nothing happened.
    TooDeep,
    /// The copies are at the end of the ring, from `first`; `runs` says that one of them can run a
    /// shell command, write files or send messages.
    Done {
        first: usize,
        count: usize,
        runs: bool,
    },
}

/// Adds the slices of a copy ([`copy_slices`]) to the end of the ring `ring`. A group is pasted
/// whole or not at all. A pasted submenu starts where the editor puts it ([`Item::at`] is dropped),
/// and what is read from the clipboard is cleaned like a file that is loaded.
pub fn paste_slices(items: &mut Vec<Item>, ring: &[usize], text: &str) -> PasteSlices {
    if text.len() > CLIP_MAX_BYTES {
        return PasteSlices::NotSlices;
    }
    let Some(json) = text.trim_start().strip_prefix(CLIP_SLICES) else {
        return PasteSlices::NotSlices;
    };
    let Ok(mut slices) = serde_json::from_str::<Vec<Item>>(json.trim()) else {
        return PasteSlices::NotSlices;
    };
    let Some(target) = items_at(items, ring) else {
        return PasteSlices::NotSlices;
    };
    if slices.is_empty() {
        return PasteSlices::NotSlices;
    }
    let first = target.len();
    if first + slices.len() > MAX_ITEMS {
        return PasteSlices::Full;
    }
    if slices.iter().any(|it| ring.len() + levels(it) >= MAX_DEPTH) {
        return PasteSlices::TooDeep;
    }
    clean_items(&mut slices, ring.len());
    let runs = slices.iter().any(item_runs_commands);
    let count = slices.len();
    for it in slices {
        // the room was checked above
        let _ = insert_item(items, ring, Place::End, it);
    }
    PasteSlices::Done { first, count, runs }
}

/// Puts `item` into the ring at `ring`, at `place`. Returns where it landed.
pub fn insert_item(
    items: &mut Vec<Item>,
    ring: &[usize],
    place: Place,
    mut item: Item,
) -> Result<Landed, String> {
    let len = check_room(items, ring, &item)?;
    // a submenu that comes from another ring (or another file) has no place on this canvas yet
    item.at = None;
    let at = match place {
        Place::At(j) => j.min(len),
        Place::End => len,
    };
    let Some(target) = ring_mut(items, ring) else {
        return Err("That ring does not exist any more.".into());
    };
    target.insert(at, item);
    Ok((ring.to_vec(), at))
}

/// Moves the slice `from` of the ring `from_ring` to `place` of the ring `to_ring` (the same ring
/// or another one). Returns where it landed.
///
/// Inside one ring `Place::At(j)` makes the slice the one at index `j` afterwards, the way the
/// slices of the classic Menu tab swap places.
pub fn move_item(
    items: &mut Vec<Item>,
    from_ring: &[usize],
    from: usize,
    to_ring: &[usize],
    place: Place,
) -> Result<Landed, String> {
    let Some(src) = items_at(items, from_ring) else {
        return Err("That slice does not exist any more.".into());
    };
    let Some(item) = src.get(from).cloned() else {
        return Err("That slice does not exist any more.".into());
    };
    if inside(to_ring, from_ring, from) {
        return Err("A submenu cannot be put inside itself.".into());
    }

    if from_ring == to_ring {
        let len = src.len();
        let to = match place {
            Place::At(j) => j.min(len - 1),
            Place::End => len - 1,
        };
        let Some(ring) = ring_mut(items, from_ring) else {
            return Err("That ring does not exist any more.".into());
        };
        let it = ring.remove(from);
        ring.insert(to, it);
        return Ok((to_ring.to_vec(), to));
    }

    check_room(items, to_ring, &item)?;
    if from_ring.is_empty() && src.len() <= 1 {
        return Err("The pie needs at least one slice.".into());
    }
    // taking the slice out shifts the later slices of its ring, and with them the path of a target
    // that lies in one of those slices
    let mut to_ring = to_ring.to_vec();
    if to_ring.len() > from_ring.len()
        && to_ring[..from_ring.len()] == *from_ring
        && to_ring[from_ring.len()] > from
    {
        to_ring[from_ring.len()] -= 1;
    }
    if let Some(ring) = ring_mut(items, from_ring) {
        ring.remove(from);
    }
    insert_item(items, &to_ring, place, item)
}

// ------------------------------------------------------------------ bringing in many slices at once

/// The paths of every slice of the top ring of `items` (`[0]`, `[1]`, ...). A submenu stands for
/// everything in it, so this is what *select all* of the import tray picks.
pub fn top_paths(items: &[Item]) -> BTreeSet<Vec<usize>> {
    (0..items.len()).map(|i| vec![i]).collect()
}

/// The paths of the slices of the ring at `ring` (`ring + [i]`), or none when the ring is gone.
pub fn ring_paths(items: &[Item], ring: &[usize]) -> Vec<Vec<usize>> {
    items_at(items, ring).map_or_else(Vec::new, |r| {
        (0..r.len())
            .map(|i| {
                let mut p = ring.to_vec();
                p.push(i);
                p
            })
            .collect()
    })
}

/// Adds all of `new` to the end of the ring at `ring` in one go: either every slice fits (room in
/// the ring and in the depth of the tree) or nothing is changed. Returns the index of the first
/// slice added and how many were added.
pub fn insert_many(
    items: &mut Vec<Item>,
    ring: &[usize],
    new: Vec<Item>,
) -> Result<(usize, usize), String> {
    if new.is_empty() {
        return Err("There is nothing to add.".into());
    }
    let Some(target) = items_at(items, ring) else {
        return Err("That ring does not exist any more.".into());
    };
    let first = target.len();
    let room = MAX_ITEMS.saturating_sub(first);
    if new.len() > room {
        return Err(if room == 0 {
            format!(
                "That ring is full ({MAX_ITEMS} slices). Pick another pie, or use \"As a submenu\"."
            )
        } else {
            format!(
                "That ring has room for {room} more slice{} and {} were chosen. Choose fewer, or use \"As a submenu\".",
                if room == 1 { "" } else { "s" },
                new.len()
            )
        });
    }
    // a trial on a copy, so a slice that goes too deep leaves the pie untouched
    let count = new.len();
    let mut trial = items.clone();
    for it in new {
        insert_item(&mut trial, ring, Place::End, it)?;
    }
    *items = trial;
    Ok((first, count))
}

/// One submenu slice that holds `children` (the whole of an imported pie): it is put in a ring
/// that is `ring_depth` rings deep, so what is inside is cut to the limits of the pie like a file
/// that is loaded ([`MAX_ITEMS`] a ring, [`MAX_DEPTH`] rings).
pub fn group_item(name: &str, icon: &str, mut children: Vec<Item>, ring_depth: usize) -> Item {
    clean_items(&mut children, ring_depth + 1);
    Item::submenu(name, icon, children)
}

/// The path of the first side-button mode slice of `items` (submenus included), or empty when
/// there is none. That is the slice a pie starts with after its slices were replaced.
pub fn first_mode_path(items: &[Item]) -> Vec<usize> {
    for (i, it) in items.iter().enumerate() {
        match &it.kind {
            ItemKind::Mode { .. } => return vec![i],
            ItemKind::Submenu(kids) => {
                let mut p = first_mode_path(kids);
                if !p.is_empty() {
                    p.insert(0, i);
                    return p;
                }
            }
            ItemKind::Run(_) | ItemKind::Smart(_) => {}
        }
    }
    Vec::new()
}

/// Makes `new` the slices of the pie `items` (the slices that were there are gone: Undo brings them
/// back). What is put in is cleaned like a file that is loaded. An empty `new` is refused and
/// nothing is changed.
pub fn replace_items(items: &mut Vec<Item>, mut new: Vec<Item>) -> Result<(), String> {
    if new.is_empty() {
        return Err("That file has no slices to put in your pie.".into());
    }
    clean_items(&mut new, 0);
    for it in &mut new {
        it.at = None;
    }
    *items = new;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Action;

    fn leaf(n: &str) -> Item {
        Item::run(n, "ico:app-window", Action::OpenApp(n.into()))
    }

    fn sub(n: &str, kids: Vec<Item>) -> Item {
        Item {
            name: n.into(),
            icon: "ico:folder".into(),
            kind: ItemKind::Submenu(kids),
            at: None,
        }
    }

    fn names(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    fn sample() -> Vec<Item> {
        vec![
            leaf("a"),
            sub("s", vec![leaf("x"), leaf("y")]),
            leaf("b"),
            leaf("c"),
        ]
    }

    #[test]
    fn levels_count_the_rings_below() {
        assert_eq!(levels(&leaf("a")), 0);
        assert_eq!(levels(&sub("e", vec![])), 0);
        assert_eq!(levels(&sub("s", vec![leaf("x")])), 1);
        assert_eq!(levels(&sub("s", vec![sub("t", vec![leaf("x")])])), 2);
    }

    #[test]
    fn a_slice_moves_inside_its_ring_to_the_place_dropped_on() {
        let mut v = sample();
        let landed = move_item(&mut v, &[], 0, &[], Place::At(2)).unwrap();
        assert_eq!(landed, (vec![], 2));
        assert_eq!(names(&v), ["s", "b", "a", "c"]);
        move_item(&mut v, &[], 3, &[], Place::At(0)).unwrap();
        assert_eq!(names(&v), ["c", "s", "b", "a"]);
        move_item(&mut v, &[], 0, &[], Place::End).unwrap();
        assert_eq!(names(&v), ["s", "b", "a", "c"]);
    }

    #[test]
    fn a_slice_moves_into_a_submenu_and_out_of_it() {
        let mut v = sample();
        let landed = move_item(&mut v, &[], 0, &[1], Place::At(1)).unwrap();
        // the submenu was at index 1 and is at index 0 after `a` left
        assert_eq!(landed, (vec![0], 1));
        assert_eq!(names(&v), ["s", "b", "c"]);
        assert_eq!(names(items_at(&v, &[0]).unwrap()), ["x", "a", "y"]);

        let landed = move_item(&mut v, &[0], 2, &[], Place::End).unwrap();
        assert_eq!(landed, (vec![], 3));
        assert_eq!(names(&v), ["s", "b", "c", "y"]);
    }

    #[test]
    fn the_target_path_follows_a_slice_that_shifted() {
        // `b` (index 2) goes into the submenu at index 1: nothing shifts for it
        let mut v = sample();
        let landed = move_item(&mut v, &[], 2, &[1], Place::End).unwrap();
        assert_eq!(landed.0, vec![1]);
        // `a` (index 0) goes into the submenu at index 1: that submenu is at index 0 afterwards
        let mut v = sample();
        let landed = move_item(&mut v, &[], 0, &[1], Place::End).unwrap();
        assert_eq!(landed.0, vec![0]);
        assert_eq!(names(items_at(&v, &landed.0).unwrap()), ["x", "y", "a"]);
    }

    #[test]
    fn a_submenu_cannot_go_inside_itself() {
        let mut v = sample();
        let before = v.clone();
        assert!(move_item(&mut v, &[], 1, &[1], Place::End).is_err());
        let mut deep = vec![sub("s", vec![sub("t", vec![leaf("x")])]), leaf("b")];
        let before_deep = deep.clone();
        assert!(move_item(&mut deep, &[], 0, &[0, 0], Place::End).is_err());
        assert_eq!(v, before);
        assert_eq!(deep, before_deep);
    }

    #[test]
    fn a_full_ring_refuses_and_nothing_changes() {
        let mut v = sample();
        let mut full: Vec<Item> = (0..MAX_ITEMS).map(|i| leaf(&format!("f{i}"))).collect();
        v.push(sub("full", std::mem::take(&mut full)));
        let before = v.clone();
        assert!(move_item(&mut v, &[], 0, &[4], Place::End).is_err());
        assert_eq!(v, before);
        assert!(insert_item(&mut v, &[4], Place::End, leaf("n")).is_err());
        assert_eq!(v, before);
    }

    #[test]
    fn the_top_ring_keeps_one_slice() {
        let mut v = vec![leaf("a"), sub("s", vec![])];
        assert!(move_item(&mut v, &[], 0, &[1], Place::End).is_ok());
        let mut single = vec![sub("s", vec![])];
        assert!(move_item(&mut single, &[], 0, &[0], Place::End).is_err());
        // a submenu may be emptied
        let mut v = vec![leaf("a"), sub("s", vec![leaf("x")])];
        assert!(move_item(&mut v, &[1], 0, &[], Place::End).is_ok());
    }

    #[test]
    fn nesting_stops_at_the_maximum_depth() {
        // a chain of submenus MAX_DEPTH - 1 rings deep
        let mut chain = leaf("leaf");
        for i in 0..(MAX_DEPTH - 1) {
            chain = sub(&format!("s{i}"), vec![chain]);
        }
        let mut v = vec![leaf("a"), chain.clone()];
        // the chain itself is as deep as allowed: it cannot move one ring down
        assert_eq!(levels(&chain), MAX_DEPTH - 1);
        assert!(insert_item(&mut v, &[1], Place::End, chain).is_err());
        // an ordinary slice goes at the deepest ring
        let mut path = vec![1];
        path.extend(std::iter::repeat_n(0, MAX_DEPTH - 2));
        assert_eq!(path.len(), MAX_DEPTH - 1);
        assert!(insert_item(&mut v, &path, Place::End, leaf("n")).is_ok());
    }

    #[test]
    fn insert_puts_a_copy_before_the_slice_dropped_on() {
        let mut v = sample();
        let landed = insert_item(&mut v, &[], Place::At(1), leaf("n")).unwrap();
        assert_eq!(landed, (vec![], 1));
        assert_eq!(names(&v), ["a", "n", "s", "b", "c"]);
        let landed = insert_item(&mut v, &[2], Place::At(99), leaf("m")).unwrap();
        assert_eq!(landed, (vec![2], 2));
        assert!(insert_item(&mut v, &[9], Place::End, leaf("z")).is_err());
    }

    #[test]
    fn a_submenu_that_lands_in_another_ring_forgets_where_its_pie_was() {
        // inside one ring the slice keeps its place on the canvas
        let mut v = sample();
        v[1].at = Some([10.0, 20.0]);
        move_item(&mut v, &[], 1, &[], Place::End).unwrap();
        assert_eq!(v[3].at, Some([10.0, 20.0]));
        // in another ring it starts where the editor puts it
        let mut x = vec![sub("p", vec![leaf("k")]), sub("q", vec![])];
        x[1].at = Some([1.0, 2.0]);
        move_item(&mut x, &[], 1, &[0], Place::End).unwrap();
        let ItemKind::Submenu(kids) = &x[0].kind else {
            panic!("not a submenu");
        };
        assert_eq!(kids[1].at, None);
    }

    #[test]
    fn pies_are_nudged_settled_and_tidied() {
        let mut v = vec![
            leaf("a"),
            sub("s", vec![leaf("x"), sub("t", vec![leaf("y")])]),
            sub("u", vec![]),
        ];
        assert!(!has_layout(&v));
        // the top pie (empty path), a slice that is not a submenu and a path that is gone are skipped
        let paths = [vec![], vec![0], vec![9], vec![1], vec![1, 1]];
        assert_eq!(nudge_pies(&mut v, paths.iter(), 10.4, -3.6), 2);
        assert_eq!(v[1].at, Some([10.4, -3.6]));
        assert_eq!(v[0].at, None);
        assert!(has_layout(&v));
        // a second nudge adds to the first one
        nudge_pies(&mut v, [vec![1]].iter(), 0.2, 0.2);
        let [x, y] = v[1].at.unwrap();
        assert!((x - 10.6).abs() < 1e-4 && (y + 3.4).abs() < 1e-4);
        // far away is held on the canvas
        nudge_pies(&mut v, [vec![2]].iter(), 1.0e9, -1.0e9);
        assert_eq!(v[2].at, Some([PIE_AT_LIMIT, -PIE_AT_LIMIT]));
        // settled: whole numbers, and a pie that is back at its place has none
        settle_pies(&mut v, [vec![1], vec![2]].iter());
        assert_eq!(v[1].at, Some([11.0, -3.0]));
        nudge_pies(&mut v, [vec![1]].iter(), -11.0, 3.2);
        settle_pies(&mut v, [vec![1]].iter());
        assert_eq!(v[1].at, None);
        // tidy reaches the pies of submenus too
        nudge_pies(&mut v, [vec![1, 1], vec![2]].iter(), 5.0, 5.0);
        assert!(has_layout(&v));
        assert_eq!(tidy_pies(&mut v), 2);
        assert!(!has_layout(&v));
    }

    #[test]
    fn slices_are_copied_with_what_is_in_them_and_pasted_at_the_end_of_a_ring() {
        let mut v = sample();
        v[1].at = Some([5.0, 5.0]);
        let paths = BTreeSet::from([vec![], vec![0], vec![1], vec![1, 0], vec![9]]);
        // the top pie, the gone path and the slice inside the selected submenu are left out
        let got = slices_at(&v, &paths);
        assert_eq!(names(&got), ["a", "s"]);
        let text = copy_slices(&got).unwrap();
        assert!(text.starts_with(CLIP_SLICES));
        assert_eq!(copy_slices(&[]), None);

        let before = v.len();
        let PasteSlices::Done { first, count, runs } = paste_slices(&mut v, &[], &text) else {
            panic!("the copy fits");
        };
        assert_eq!((first, count, runs), (before, 2, false));
        assert_eq!(names(&v), ["a", "s", "b", "c", "a", "s"]);
        // a pasted submenu starts where the editor puts it, and keeps its slices
        assert_eq!(v[5].at, None);
        assert_eq!(v[1].at, Some([5.0, 5.0]));
        assert!(matches!(&v[5].kind, ItemKind::Submenu(k) if k.len() == 2));
        // into a submenu
        assert!(matches!(
            paste_slices(&mut v, &[1], &copy_slices(&[leaf("n")]).unwrap()),
            PasteSlices::Done {
                first: 2,
                count: 1,
                ..
            }
        ));
    }

    #[test]
    fn a_paste_that_is_not_slices_or_does_not_fit_changes_nothing() {
        let mut v = sample();
        let before = v.clone();
        assert_eq!(paste_slices(&mut v, &[], "hello"), PasteSlices::NotSlices);
        assert_eq!(paste_slices(&mut v, &[], ""), PasteSlices::NotSlices);
        let broken = format!("{CLIP_SLICES}[{{\"name\":");
        assert_eq!(paste_slices(&mut v, &[], &broken), PasteSlices::NotSlices);
        assert_eq!(
            paste_slices(&mut v, &[], &format!("{CLIP_SLICES}[]")),
            PasteSlices::NotSlices
        );
        let one = copy_slices(&[leaf("n")]).unwrap();
        assert_eq!(paste_slices(&mut v, &[7], &one), PasteSlices::NotSlices);
        // the ring holds MAX_ITEMS at most, and a group goes in whole or not at all
        let many: Vec<Item> = (0..MAX_ITEMS).map(|i| leaf(&format!("m{i}"))).collect();
        assert_eq!(
            paste_slices(&mut v, &[], &copy_slices(&many).unwrap()),
            PasteSlices::Full
        );
        // too deep
        let mut chain = sub("deep", vec![]);
        for _ in 0..MAX_DEPTH {
            chain = sub("deep", vec![chain]);
        }
        assert_eq!(
            paste_slices(&mut v, &[], &copy_slices(&[chain]).unwrap()),
            PasteSlices::TooDeep
        );
        assert_eq!(v, before);
        // a command is reported
        let cmd = Item::run("c", "ico:terminal-window", Action::Command("ls".into()));
        assert!(matches!(
            paste_slices(&mut v, &[], &copy_slices(&[cmd]).unwrap()),
            PasteSlices::Done { runs: true, .. }
        ));
    }

    #[test]
    fn cut_removes_what_copy_took_and_keeps_one_slice_in_the_top_ring() {
        let mut v = sample();
        // a slice of a submenu, a slice of the top ring, and a slice inside a selected submenu
        let paths = BTreeSet::from([vec![0], vec![1], vec![1, 0], vec![3]]);
        assert_eq!(remove_slices(&mut v, &paths), 3);
        assert_eq!(names(&v), ["b"]);
        let mut w = sample();
        assert_eq!(
            remove_slices(&mut w, &BTreeSet::from([vec![1, 1], vec![1, 0]])),
            2
        );
        let ItemKind::Submenu(kids) = &w[1].kind else {
            panic!("not a submenu");
        };
        assert!(kids.is_empty());
        // the last slice of the top ring stays
        let mut one = vec![leaf("only")];
        assert_eq!(remove_slices(&mut one, &BTreeSet::from([vec![0]])), 0);
        assert_eq!(names(&one), ["only"]);
        assert_eq!(remove_slices(&mut w, &BTreeSet::from([vec![], vec![9]])), 0);
    }

    #[test]
    fn many_slices_go_in_together_or_not_at_all() {
        let mut v = sample(); // 4 slices
        let (first, count) = insert_many(&mut v, &[], vec![leaf("p"), leaf("q")]).unwrap();
        assert_eq!((first, count), (4, 2));
        assert_eq!(names(&v), ["a", "s", "b", "c", "p", "q"]);
        // 6 + 3 > 8: nothing changes
        let before = v.clone();
        let err = insert_many(&mut v, &[], vec![leaf("x1"), leaf("x2"), leaf("x3")]).unwrap_err();
        assert!(err.contains("room for 2"), "{err}");
        assert_eq!(v, before);
        // into a submenu
        let (first, count) = insert_many(&mut v, &[1], vec![leaf("z")]).unwrap();
        assert_eq!((first, count), (2, 1));
        assert!(insert_many(&mut v, &[], vec![]).is_err());
        assert!(
            insert_many(&mut v, &[0], vec![leaf("n")]).is_err(),
            "a leaf has no ring"
        );
    }

    #[test]
    fn many_slices_that_go_too_deep_leave_the_pie_untouched() {
        let mut v = vec![leaf("a")];
        // five rings below the top one: one more than a pie can hold
        let deep = sub(
            "1",
            vec![sub(
                "2",
                vec![sub("3", vec![sub("4", vec![sub("5", vec![leaf("x")])])])],
            )],
        );
        assert_eq!(levels(&deep), MAX_DEPTH);
        let before = v.clone();
        assert!(insert_many(&mut v, &[], vec![leaf("ok"), deep]).is_err());
        assert_eq!(v, before, "the slice that fit was not kept either");
    }

    #[test]
    fn select_all_picks_the_top_ring_and_a_ring_lists_its_slices() {
        let v = sample();
        assert_eq!(top_paths(&v).len(), 4);
        assert_eq!(ring_paths(&v, &[1]), vec![vec![1, 0], vec![1, 1]]);
        assert!(ring_paths(&v, &[0]).is_empty());
        assert!(ring_paths(&v, &[9]).is_empty());
        // picked together with slices_at: a slice inside a picked submenu is not taken twice
        let mut paths = top_paths(&v);
        paths.extend(ring_paths(&v, &[1]));
        assert_eq!(slices_at(&v, &paths).len(), 4);
    }

    #[test]
    fn a_whole_pie_becomes_one_submenu_cut_to_the_limits() {
        let many: Vec<Item> = (0..10).map(|i| leaf(&format!("s{i}"))).collect();
        let g = group_item("Photo", "ico:folder-simple", many, 0);
        assert_eq!(g.name, "Photo");
        let ItemKind::Submenu(kids) = &g.kind else {
            panic!("not a submenu");
        };
        assert_eq!(kids.len(), MAX_ITEMS);
        let mut v = sample();
        let (first, _) = insert_many(&mut v, &[], vec![g]).unwrap();
        assert_eq!(first, 4);
        assert_eq!(levels(&v[4]), 1);
    }

    #[test]
    fn replacing_a_pie_cleans_what_comes_in_and_refuses_nothing() {
        let mut v = sample();
        let mut incoming = vec![leaf("n1"), leaf("n2")];
        incoming[0].at = Some([5.0, 5.0]);
        replace_items(&mut v, incoming).unwrap();
        assert_eq!(names(&v), ["n1", "n2"]);
        assert_eq!(v[0].at, None);
        let before = v.clone();
        assert!(replace_items(&mut v, Vec::new()).is_err());
        assert_eq!(v, before);
    }

    #[test]
    fn the_first_mode_is_found_inside_submenus_too() {
        let mode = Item::mode(
            "m",
            "ico:mouse-simple",
            crate::keys::Shortcut::new(1, 0),
            crate::keys::Shortcut::new(2, 0),
        );
        assert!(first_mode_path(&sample()).is_empty());
        let v = vec![leaf("a"), sub("s", vec![leaf("x"), mode.clone()]), mode];
        assert_eq!(first_mode_path(&v), vec![1, 1]);
    }
}
