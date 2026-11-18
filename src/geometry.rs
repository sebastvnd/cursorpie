//! Pie geometry: sizes and slice hit-testing.
//!
//! Pure math with no drawing and no OS types, shared by the event tap (which decides what is hovered) and the UI
//! (which draws it), so both always agree on where a slice is.
//!
//! Slice 0 is centered on the left (angle pi) and slices run clockwise on screen.
//!
//! ```
//! use cursorpie::geometry::{pie_inner, slice_at};
//! use cursorpie::geometry::Point;
//!
//! let center = Point::new(100.0, 100.0);
//! // inside the dead zone nothing is hovered
//! assert_eq!(slice_at(center, center, 4, pie_inner(200.0)), None);
//! // directly left of the center is slice 0
//! assert_eq!(slice_at(center, Point::new(20.0, 100.0), 4, pie_inner(200.0)), Some(0));
//! ```

use std::f32::consts::{PI, TAU};

/// A point in global screen space, in logical points (the unit egui uses, y grows downwards).
/// The native event layers convert to this so everything above them is OS independent.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    /// A point at `(x, y)`.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Extra room, in points, the pie window has on every side beyond the ring's own square, for the
/// badges of an "Open app" slice (see [`running`](crate::running)), which sit outside the rim. The
/// window is transparent and click-through, so the room costs nothing to look at.
pub const BADGE_ROOM: f32 = 36.0;

/// Side of the (square) pie window for a pie of size `win` (`Config::pie_size`): the pie plus
/// [`BADGE_ROOM`] on every side. The ring is still drawn and hit-tested for `win`, around the center.
pub fn window_size(win: f32) -> f32 {
    win + 2.0 * BADGE_ROOM
}

/// Outer radius of the ring for a window of size `win`.
pub fn pie_outer(win: f32) -> f32 {
    win / 2.0 - 10.0
}
/// Inner radius (the dead zone around the center disc).
pub fn pie_inner(win: f32) -> f32 {
    pie_outer(win) * 0.36
}
/// Trail ("web") mode. The rings that were left are drawn as small nodes on an arc around the ring
/// on screen, in the order they were opened, joined by a thread that ends at the center of the
/// ring on screen, and moving the pointer onto one returns to that ring. The nodes are all at the
/// same distance from the center (so every one can be reached in a straight line, and the window
/// needs very little room, see [`trail_room`]), the nearest first, and each older one a step
/// further round the arc (see [`trail_layout`]). How far, how big and how far apart is the
/// person's to choose (see [`TrailShape`]).
///
/// Defaults of the four numbers of [`TrailShape`], in the units the settings use. Distance of the
/// nodes from the center, in percent of the outer radius of the ring (1.8 radii clears the badges
/// of an "Open app" slice, which reach about 1.4):
pub const TRAIL_REACH_PCT: u16 = 180;
/// Radius of the node of the ring just left, in percent of the outer radius.
pub const TRAIL_NODE_PCT: u16 = 28;
/// How big each older node is next to the one before it, in percent.
pub const TRAIL_SHRINK_PCT: u16 = 78;
/// Angle between one node and the next around the center, in degrees.
pub const TRAIL_FAN_DEG: u16 = 38;
/// The smallest a node gets, as a share of the nearest.
pub const TRAIL_SMALLEST: f32 = 0.35;

/// Limits of the four numbers, as the settings and the configuration file keep them.
pub const TRAIL_REACH_RANGE: (u16, u16) = (150, 240);
pub const TRAIL_NODE_RANGE: (u16, u16) = (14, 45);
pub const TRAIL_SHRINK_RANGE: (u16, u16) = (55, 100);
pub const TRAIL_FAN_RANGE: (u16, u16) = (20, 60);
/// How fast the nodes glide to their places, in percent of the usual speed.
pub const TRAIL_GLIDE_RANGE: (u16, u16) = (40, 300);

/// How the trail is laid out: the person's choice of distance, size and spacing of the nodes. The
/// tap (which decides which node the pointer is on) and the UI (which draws them) are given the
/// same shape, so they always agree on where the nodes are.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailShape {
    /// Distance of the nodes from the center of the ring on screen, in outer radii.
    pub reach: f32,
    /// Radius of the node of the ring just left, in outer radii.
    pub node: f32,
    /// Size of each older node next to the one before it (1 = all the same).
    pub shrink: f32,
    /// Angle between one node and the next around the center, in radians.
    pub fan: f32,
}

impl TrailShape {
    /// The shape of the settings' numbers (see the `TRAIL_*_PCT` constants for their units). Every
    /// number is kept within its range, and the result is always usable: the nodes clear the ring
    /// (the distance is at least a ring and a node and a gap), and two neighbours never touch
    /// (the angle between them is widened when the nodes are big), so the way to a node never
    /// crosses another.
    pub fn from_percent(reach: u16, node: u16, shrink: u16, fan_deg: u16) -> Self {
        let clamp = |v: u16, (lo, hi): (u16, u16)| f32::from(v.clamp(lo, hi));
        let node = clamp(node, TRAIL_NODE_RANGE) / 100.0;
        let reach = (clamp(reach, TRAIL_REACH_RANGE) / 100.0).max(node + 1.2);
        let shrink = clamp(shrink, TRAIL_SHRINK_RANGE) / 100.0;
        let fan = clamp(fan_deg, TRAIL_FAN_RANGE).to_radians();
        // the two biggest nodes (the nearest and the next) sit `reach` from the center, `fan`
        // apart: their centers are 2 * reach * sin(fan / 2) apart and must clear both radii
        let apart = (node * (1.0 + shrink) * 1.15 / (2.0 * reach)).min(1.0);
        Self {
            reach,
            node,
            shrink,
            fan: fan.max(2.0 * apart.asin()),
        }
    }
}

impl Default for TrailShape {
    fn default() -> Self {
        Self::from_percent(
            TRAIL_REACH_PCT,
            TRAIL_NODE_PCT,
            TRAIL_SHRINK_PCT,
            TRAIL_FAN_DEG,
        )
    }
}

/// Room, in points, that the pie window gains on every side while the trail mode is on: what the
/// nodes need beyond the 46 points the window already has around the ring. It is the same at every
/// level, so the window never has to be resized while a gesture runs.
pub fn trail_room(win: f32, shape: TrailShape) -> f32 {
    (pie_outer(win) * (shape.reach + shape.node - 1.0) - 46.0).max(0.0) + 6.0
}

/// Radius of the node of the ring just left (trail mode). Older nodes are smaller.
pub fn trail_node_radius(win: f32, shape: TrailShape) -> f32 {
    pie_outer(win) * shape.node
}

/// One node of the trail, where it is drawn and where the pointer returns to its ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrailNode {
    /// The depth of the ring this node stands for (0 is the pie that was opened): going back to it
    /// leaves the path with this many steps.
    pub level: usize,
    /// Offset of the node from the center of the ring on screen, in points of the pie.
    pub x: f32,
    pub y: f32,
    /// Radius of the node.
    pub r: f32,
}

/// Where the nodes of the trail go. `offsets` are the centers of the rings that were left, minus
/// the center of the ring on screen, outermost first, in points of the pie (one point of the
/// pointer is `scale` of them, see `platform::window_scale`). The result is the nearest ring first.
///
/// The nearest node is on the line to the center of the ring it stands for (the way back), at
/// [`TrailShape::reach`] outer radii; each older one is [`TrailShape::fan`] further round the arc, always the
/// same way, so that the order reads along the arc: the oldest is at its far end. Each is a little
/// smaller than the one before. Nothing else depends on where an older ring really was, so the
/// nodes never cross each other or leave the window, and the tap and the UI, which both call this,
/// agree on where they are.
pub fn trail_layout(offsets: &[(f32, f32)], win: f32, shape: TrailShape) -> Vec<TrailNode> {
    let outer = pie_outer(win);
    let Some(&(px, py)) = offsets.last() else {
        return Vec::new();
    };
    // a ring that was left exactly where this one is has no direction: use the left
    let base = if px.hypot(py) < 1e-3 {
        PI
    } else {
        py.atan2(px)
    };
    let first = trail_node_radius(win, shape);
    let n = offsets.len();
    (0..n)
        .map(|slot| {
            let angle = base + slot as f32 * shape.fan;
            let dist = outer * shape.reach;
            TrailNode {
                level: n - 1 - slot,
                x: angle.cos() * dist,
                y: angle.sin() * dist,
                r: first * shape.shrink.powi(slot as i32).max(TRAIL_SMALLEST),
            }
        })
        .collect()
}

/// The ring to return to when the pointer is at `(dx, dy)` from the center of the ring on screen
/// (points of the pie): the level of the node it is on, if any.
pub fn trail_node_at(nodes: &[TrailNode], dx: f32, dy: f32) -> Option<usize> {
    nodes
        .iter()
        .find(|n| (dx - n.x).hypot(dy - n.y) < n.r * 0.95)
        .map(|n| n.level)
}

/// The outline of the pie, as the settings give it: a circle, or a regular polygon whose corners
/// are rounded. Sides of a polygon, the roundness of its corners in percent (100 = a circle) and
/// the turn of the whole outline in degrees (0 = a flat side faces left, where slice 0 is).
pub const PIE_SIDES_RANGE: (u8, u8) = (3, 12);
pub const PIE_SIDES_DEFAULT: u8 = 4;
pub const PIE_ROUND_DEFAULT: u8 = 45;
pub const PIE_TURN_MAX: u16 = 359;

/// How far from the center the rim of the pie is in each direction, in outer radii (at most 1, so
/// the pie always fits the window that was made for a circle). The tap (which decides when a drag
/// has crossed the rim) and the UI (which draws it) are given the same outline, so they always
/// agree on where the rim is.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Outline {
    /// Sides of the polygon; 0 is a circle.
    sides: u8,
    /// Roundness of the corners, 0 (sharp) to 1 (a circle).
    round: f32,
    /// Turn of the outline, in radians.
    turn: f32,
}

impl Outline {
    /// The circle: the pie of every version before the outline could be chosen.
    pub const CIRCLE: Self = Self {
        sides: 0,
        round: 0.0,
        turn: 0.0,
    };

    /// The outline of the settings' numbers (see the `PIE_*` constants for their units). `custom`
    /// off gives the circle. Every number is kept within its range.
    pub fn from_settings(custom: bool, sides: u8, round_pct: u8, turn_deg: u16) -> Self {
        if !custom {
            return Self::CIRCLE;
        }
        Self {
            sides: sides.clamp(PIE_SIDES_RANGE.0, PIE_SIDES_RANGE.1),
            round: f32::from(round_pct.min(100)) / 100.0,
            turn: f32::from(turn_deg.min(PIE_TURN_MAX)).to_radians(),
        }
    }

    /// Whether this is the circle.
    pub fn is_circle(self) -> bool {
        self.sides == 0
    }

    /// The distance of the rim from the center toward `angle` (radians, as on screen), in outer
    /// radii. The corners of a polygon reach 1 (so a sharp one fits the circle around it), its flat
    /// sides are nearer; the corners are rounded with arcs of a radius that grows with the
    /// roundness until the arcs meet and the outline is a circle again.
    pub fn radius(self, angle: f32) -> f32 {
        if self.is_circle() {
            return 1.0;
        }
        // a polygon with corners at distance 1: its sides are `flat` away from the center
        let half = PI / f32::from(self.sides);
        let flat = half.cos();
        // the corners are rounded with arcs of radius `arc`, around the corners of a smaller polygon
        let arc = flat * self.round;
        let inset = flat - arc;
        // the flat part of a side is what is left of it between two arcs
        let reach = inset * half.tan();
        // the direction measured from the nearest side's normal, in `-half..half`
        let phi = (angle - (PI + self.turn) + half).rem_euclid(2.0 * half) - half;
        let (sin, cos) = phi.sin_cos();
        let t = if flat * (sin / cos).abs() <= reach {
            flat / cos
        } else {
            // on an arc: where the ray leaves the circle around the corner `(inset, reach)`
            let corner = (inset, reach * phi.signum());
            let along = cos * corner.0 + sin * corner.1;
            let off = corner.0 * corner.0 + corner.1 * corner.1 - arc * arc;
            along + (along * along - off).max(0.0).sqrt()
        };
        // scaled so that the farthest point is at 1; at a sharp corner the square root above sits
        // on a difference that is zero up to rounding, so it can overshoot by about 2e-4
        (t / (1.0 - self.round * (1.0 - flat))).min(1.0)
    }
}

/// Start angle (radians) of slice `i` out of `total`.
pub fn slice_start(i: usize, total: usize) -> f32 {
    let sl = TAU / total as f32;
    PI - sl / 2.0 + i as f32 * sl
}

/// Which slice is `p` over, relative to `center`? None inside the dead zone.
pub fn slice_at(center: Point, p: Point, total: usize, dead_zone: f32) -> Option<usize> {
    let dx = (p.x - center.x) as f32;
    let dy = (p.y - center.y) as f32;
    if dx.hypot(dy) < dead_zone || total == 0 {
        return None;
    }
    let mut angle = dy.atan2(dx);
    if angle < 0.0 {
        angle += TAU;
    }
    let slice_angle = TAU / total as f32;
    let mut adjusted = angle - slice_start(0, total);
    while adjusted < 0.0 {
        adjusted += TAU;
    }
    Some((adjusted / slice_angle).floor() as usize % total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_1_SQRT_2;

    fn outline(sides: u8, round: u8) -> Outline {
        Outline::from_settings(true, sides, round, 0)
    }

    #[test]
    fn the_circle_is_one_all_round_and_the_default() {
        assert_eq!(Outline::default(), Outline::CIRCLE);
        assert_eq!(Outline::from_settings(false, 5, 30, 90), Outline::CIRCLE);
        assert!((0..360).all(|d| Outline::CIRCLE.radius((d as f32).to_radians()) == 1.0));
    }

    #[test]
    fn a_sharp_square_reaches_one_at_its_corners_and_less_on_its_sides() {
        let sq = outline(4, 0);
        // a flat side faces left (slice 0); the corners are between the slices
        assert!((sq.radius(PI) - FRAC_1_SQRT_2).abs() < 1e-4);
        assert!((sq.radius(PI + PI / 4.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn full_roundness_is_a_circle_again() {
        for sides in PIE_SIDES_RANGE.0..=PIE_SIDES_RANGE.1 {
            let o = outline(sides, 100);
            assert!((0..360).all(|d| (o.radius((d as f32).to_radians()) - 1.0).abs() < 1e-3));
        }
    }

    #[test]
    fn the_outline_fits_the_circle_is_continuous_and_repeats_every_side() {
        for sides in PIE_SIDES_RANGE.0..=PIE_SIDES_RANGE.1 {
            for round in [0, 25, 45, 80] {
                let o = outline(sides, round);
                let step = TAU / f32::from(sides);
                let mut last = o.radius(0.0);
                for k in 1..=720 {
                    let a = TAU * k as f32 / 720.0;
                    let r = o.radius(a);
                    assert!(r > 0.4 && r <= 1.0 + 1e-4, "{sides} sides, {round} %: {r}");
                    assert!((r - last).abs() < 0.03, "{sides} sides, {round} %: a jump");
                    assert!((r - o.radius(a + step)).abs() < 1e-3, "not periodic");
                    last = r;
                }
            }
        }
    }

    #[test]
    fn the_turn_moves_the_outline_and_the_settings_are_kept_in_range() {
        let turned = Outline::from_settings(true, 4, 0, 45);
        assert!(
            (turned.radius(PI) - 1.0).abs() < 1e-4,
            "a corner now faces left"
        );
        let wild = Outline::from_settings(true, 200, 250, 9000);
        assert_eq!(
            wild,
            Outline::from_settings(true, PIE_SIDES_RANGE.1, 100, PIE_TURN_MAX)
        );
    }

    fn at(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn sizes_follow_the_window() {
        assert_eq!(pie_outer(200.0), 90.0);
        assert!((pie_inner(200.0) - 32.4).abs() < 1e-4);
    }

    #[test]
    fn four_slices_run_clockwise_from_the_left() {
        let c = at(100.0, 100.0);
        let dz = pie_inner(200.0);
        assert_eq!(slice_at(c, at(40.0, 100.0), 4, dz), Some(0), "left");
        assert_eq!(
            slice_at(c, at(100.0, 40.0), 4, dz),
            Some(1),
            "up (y grows downwards)"
        );
        assert_eq!(slice_at(c, at(160.0, 100.0), 4, dz), Some(2), "right");
        assert_eq!(slice_at(c, at(100.0, 160.0), 4, dz), Some(3), "down");
    }

    #[test]
    fn the_dead_zone_and_empty_rings_hover_nothing() {
        let c = at(0.0, 0.0);
        assert_eq!(slice_at(c, at(10.0, 0.0), 4, 32.4), None);
        assert_eq!(slice_at(c, at(50.0, 0.0), 0, 32.4), None);
    }

    #[test]
    fn eight_slices_cover_the_circle() {
        let c = at(0.0, 0.0);
        let mut seen = [false; 8];
        for deg in (0..360).step_by(5) {
            let a = (deg as f64).to_radians();
            let p = at(60.0 * a.cos(), 60.0 * a.sin());
            seen[slice_at(c, p, 8, 32.4).unwrap()] = true;
        }
        assert!(seen.iter().all(|s| *s));
    }

    #[test]
    fn the_nearest_trail_node_is_on_the_way_to_its_ring_and_clear_of_the_ring_on_screen() {
        let outer = pie_outer(200.0);
        // a ring left 1 outer radius to the right: its node is on that line, at the reach
        let shape = TrailShape::default();
        let nodes = trail_layout(&[(outer, 0.0)], 200.0, shape);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].level, 0);
        assert!((nodes[0].x - outer * shape.reach).abs() < 1e-3 && nodes[0].y.abs() < 1e-3);
        // however far it really was, the node is at the same distance
        let far = trail_layout(&[(0.0, 5.0 * outer)], 200.0, shape);
        assert!((far[0].y - outer * shape.reach).abs() < 1e-3);
        // the ring on screen reaches `outer` from its center; the node starts after a gap
        assert!(nodes[0].x - nodes[0].r > outer * 1.1);
    }

    #[test]
    fn the_trail_nodes_read_in_order_along_an_arc_and_stay_in_the_window() {
        for win in [140.0, 200.0, 360.0] {
            let outer = pie_outer(win);
            // five rings left, the nearest one to the left
            let offs: Vec<(f32, f32)> = (0..5).map(|i| (-(outer + i as f32), 3.0)).collect();
            let shape = TrailShape::default();
            let nodes = trail_layout(&offs, win, shape);
            assert_eq!(nodes.len(), 5);
            assert_eq!(nodes[0].level, 4, "the nearest first");
            let half = window_size(win) / 2.0 + trail_room(win, shape);
            for (i, a) in nodes.iter().enumerate() {
                assert!(
                    a.x.abs() + a.r < half && a.y.abs() + a.r < half,
                    "in the window"
                );
                // all at the same distance from the center, each smaller than the one before
                assert!((a.x.hypot(a.y) - outer * shape.reach).abs() < 1e-2);
                if i > 0 {
                    assert!(a.r < nodes[i - 1].r);
                }
                for b in &nodes[i + 1..] {
                    // no two nodes touch, so the way to one never crosses another
                    assert!((a.x - b.x).hypot(a.y - b.y) > a.r + b.r, "{a:?} {b:?}");
                }
                // and each one is found by the pointer on it, as the level it stands for
                assert_eq!(trail_node_at(&nodes, a.x, a.y), Some(a.level));
            }
            assert_eq!(
                trail_node_at(&nodes, 0.0, 0.0),
                None,
                "the center is no node"
            );
        }
    }

    #[test]
    fn the_extra_window_of_the_trail_stays_modest() {
        // The nodes sit at the default reach (1.8) outer radii so that they clear the badges of an
        // "Open app" slice (which reach about 1.4), and the window must hold them: 57 points
        // more on every side for the default pie (200), 144 for the largest (360). The bounds
        // are a little above that, to catch a change that makes the window (and the surface the
        // renderer keeps for it) grow without anyone meaning it.
        let shape = TrailShape::default();
        assert!(
            trail_room(200.0, shape) < 60.0,
            "{}",
            trail_room(200.0, shape)
        );
        assert!(
            trail_room(360.0, shape) < 150.0,
            "{}",
            trail_room(360.0, shape)
        );
        // and it is exactly what the farthest node needs, no more: the node reaches
        // REACH + NODE outer radii from the center, the window already has 46 points past the ring
        for win in [140.0, 200.0, 360.0] {
            let need = pie_outer(win) * (shape.reach + shape.node);
            let half = window_size(win) / 2.0 + trail_room(win, shape);
            assert!(half >= need && half - need < 8.0, "{win}: {half} vs {need}");
        }
    }

    #[test]
    fn older_trail_nodes_step_down_in_size_in_clear_tiers() {
        let offs = [(-100.0, 0.0); 5];
        let r: Vec<f32> = trail_layout(&offs, 200.0, TrailShape::default())
            .iter()
            .map(|n| n.r)
            .collect();
        for w in r.windows(2) {
            assert!(w[1] <= w[0] * 0.8, "each at least a fifth smaller: {r:?}");
        }
        assert!(r[4] >= r[0] * TRAIL_SMALLEST - 1e-3);
    }

    #[test]
    fn the_default_shape_is_the_one_of_the_default_numbers() {
        let d = TrailShape::default();
        assert!((d.reach - 1.8).abs() < 1e-6 && (d.node - 0.28).abs() < 1e-6);
        assert!((d.shrink - 0.78).abs() < 1e-6);
        assert!((d.fan - 38.0_f32.to_radians()).abs() < 1e-6, "{}", d.fan);
    }

    #[test]
    fn the_shape_keeps_every_number_in_its_range_whatever_it_is_given() {
        let wild = TrailShape::from_percent(0, 0, 0, 0);
        let huge = TrailShape::from_percent(u16::MAX, u16::MAX, u16::MAX, u16::MAX);
        for s in [wild, huge] {
            assert!(s.reach.is_finite() && s.node.is_finite() && s.fan.is_finite());
            assert!(s.node >= 0.14 - 1e-6 && s.node <= 0.45 + 1e-6, "{s:?}");
            assert!(s.shrink >= 0.55 - 1e-6 && s.shrink <= 1.0 + 1e-6, "{s:?}");
            assert!(s.reach >= 1.5 - 1e-6 && s.reach <= 2.4 + 1e-6, "{s:?}");
        }
    }

    #[test]
    fn whatever_the_shape_the_nodes_clear_the_ring_and_each_other_and_stay_in_the_window() {
        for win in [140.0, 200.0, 360.0] {
            let outer = pie_outer(win);
            for reach in [150, 180, 240] {
                for node in [14, 28, 45] {
                    for shrink in [55, 78, 100] {
                        for fan in [20, 38, 60] {
                            let shape = TrailShape::from_percent(reach, node, shrink, fan);
                            let offs: Vec<(f32, f32)> =
                                (0..5).map(|i| (outer + i as f32, 1.0)).collect();
                            let nodes = trail_layout(&offs, win, shape);
                            let half = window_size(win) / 2.0 + trail_room(win, shape);
                            for (i, a) in nodes.iter().enumerate() {
                                let tag = format!("{win} {shape:?} {a:?}");
                                assert!(
                                    a.x.hypot(a.y) - a.r > outer * 1.1,
                                    "clear of the ring: {tag}"
                                );
                                assert!(a.x.abs() + a.r <= half + 1e-3, "in the window: {tag}");
                                assert!(a.y.abs() + a.r <= half + 1e-3, "in the window: {tag}");
                                for b in &nodes[i + 1..] {
                                    assert!(
                                        (a.x - b.x).hypot(a.y - b.y) > a.r + b.r,
                                        "apart: {tag} {b:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_farther_or_bigger_trail_asks_the_window_for_more_room() {
        let near = TrailShape::from_percent(150, 14, 78, 38);
        let far = TrailShape::from_percent(240, 45, 78, 38);
        assert!(trail_room(200.0, far) > trail_room(200.0, near));
        assert!(trail_node_radius(200.0, far) > trail_node_radius(200.0, near));
    }

    #[test]
    fn slice_zero_starts_half_a_slice_before_the_left() {
        let start = slice_start(0, 4);
        assert!((start - (PI - PI / 4.0)).abs() < 1e-6);
        assert!((slice_start(1, 4) - start - PI / 2.0).abs() < 1e-6);
    }
}
