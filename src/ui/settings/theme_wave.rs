//! The wave that carries Settings from one theme to the next.
//!
//! Choosing a theme does not swap the colors in one frame. A disc grows out of the tile that was
//! clicked until it fills the whole window, wearing the colors of the new theme (an accent rim, the
//! sidebar tone, then the page). When the window is full the theme is really changed, and a round
//! hole opens from the same point and uncovers the new look, the rim of accent running ahead of it.
//!
//! ```text
//!   flood (grow)  ->  swap (the window is covered)  ->  reveal (the hole grows)
//! ```
//!
//! Everything is drawn on a layer of its own over the window, clipped to the rounded corners of the
//! window card, so nothing shows outside it. The only state is one small value in the egui memory:
//! where the wave started, when, and which theme it brings. The theme of the config is changed by
//! [`drive`] at the swap, so a window that closes half way never keeps a theme the person did not
//! see arrive.

use super::theme::registry;
use crate::ui::draw::{dark, rgb};
use eframe::egui::{self, Color32, Id, Pos2, Rect, Shape, Vec2};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// Seconds the disc takes to fill the window.
const FLOOD: f64 = 0.42;
/// Seconds the window stays covered, which is when the theme is swapped.
const HOLD: f64 = 0.05;
/// Seconds the hole takes to uncover the new look.
const REVEAL: f64 = 0.52;
/// A wave older than this is a leftover of a window that was closed: it is dropped, not applied.
const STALE: f64 = 3.0;
/// Width of the accent rim at its widest, in points.
const RIM: f32 = 16.0;
/// How many slices the round shapes are cut in.
const SLICES: usize = 144;
/// Segments of one rounded corner of the window.
const CORNER_SEGMENTS: usize = 10;

#[derive(Clone, Debug)]
struct Wave {
    /// Id of the theme the wave brings.
    to: String,
    /// Where it starts: the middle of the tile that was clicked.
    origin: Pos2,
    /// `egui` time at the start, in seconds.
    start: f64,
    /// The window of Settings it belongs to (`State::session`).
    session: u64,
    /// Whether the theme was swapped already.
    swapped: bool,
}

fn key() -> Id {
    Id::new("cursorpie_theme_wave")
}

fn get(ctx: &egui::Context) -> Option<Wave> {
    ctx.data(|d| d.get_temp::<Wave>(key()))
}

/// Whether a wave is on its way. A second choice waits for it to end.
pub(super) fn active(ctx: &egui::Context) -> bool {
    get(ctx).is_some()
}

/// The theme a wave is bringing, shown as chosen at once so the tile answers the click.
pub(super) fn pending(ctx: &egui::Context) -> Option<String> {
    get(ctx).map(|w| w.to)
}

/// Starts a wave toward `theme`, growing from `origin`. Does nothing while another one runs.
pub(super) fn start(ctx: &egui::Context, session: u64, theme: &str, origin: Pos2) {
    if active(ctx) {
        return;
    }
    let wave = Wave {
        to: theme.to_owned(),
        origin,
        start: ctx.input(|i| i.time),
        session,
        swapped: false,
    };
    ctx.data_mut(|d| d.insert_temp(key(), wave));
    ctx.request_repaint();
}

/// Moves the wave one frame on and draws it over `card` (the window, with corners of `radius`).
/// At the swap, writes the new theme (and the accent that goes with it) into `current` and `accent`.
/// Call it once a frame while Settings is open.
pub(super) fn drive(
    ctx: &egui::Context,
    session: u64,
    card: Rect,
    radius: f32,
    current: &mut String,
    accent: &mut [u8; 3],
) {
    let Some(mut wave) = get(ctx) else {
        return;
    };
    let t = ctx.input(|i| i.time) - wave.start;
    if wave.session != session || !(0.0..STALE).contains(&t) {
        ctx.data_mut(|d| d.remove::<Wave>(key()));
        return;
    }
    let to = registry::find(&wave.to);
    // an accent nobody changed follows the theme; a chosen one stays
    let theme_accent = |c: &str, a: [u8; 3]| {
        if a == registry::find(c).accent {
            to.accent
        } else {
            a
        }
    };
    let rim = rgb(if wave.swapped {
        *accent
    } else {
        theme_accent(current, *accent)
    });
    let p = to.palette(dark());

    let layer = egui::LayerId::new(egui::Order::Foreground, key());
    let painter = egui::Painter::new(ctx.clone(), layer, card.expand(2.0));
    let shape = rounded_polygon(card, radius);
    let reach = farthest_corner(wave.origin, card) + 6.0;

    if t < FLOOD {
        // the disc grows; its edge is three bands: accent, the sidebar tone, the page
        let r = ease_in_out((t / FLOOD) as f32) * reach;
        let disc = |r: f32| clip_convex(circle(wave.origin, r), &shape);
        for (radius, color) in [(r, rim), (r - 10.0, p.sidebar), (r - 22.0, p.bg)] {
            if radius > 0.5 {
                fill_convex(&painter, disc(radius), color);
            }
        }
    } else {
        if !wave.swapped {
            // the window is covered now: this is the moment to really change the theme
            *accent = theme_accent(current, *accent);
            *current = wave.to.clone();
            wave.swapped = true;
            ctx.data_mut(|d| d.insert_temp(key(), wave.clone()));
        }
        let u = ((t - FLOOD - HOLD) / REVEAL).clamp(0.0, 1.0) as f32;
        let hole = ease_in_out(u) * reach;
        let rim_w = RIM * (1.0 - u * u);
        // the page that is still covering the new look, outside the rim
        annulus(
            &painter,
            wave.origin,
            hole + rim_w,
            reach * 1.08,
            p.bg,
            &shape,
        );
        // the accent rim that runs ahead of the hole
        if rim_w > 0.5 {
            annulus(&painter, wave.origin, hole, hole + rim_w, rim, &shape);
        }
    }

    if t >= FLOOD + HOLD + REVEAL {
        ctx.data_mut(|d| d.remove::<Wave>(key()));
    }
    ctx.request_repaint();
}

// ------------------------------------------------------------------ motion

/// Slow at both ends, quick in the middle.
fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

// ------------------------------------------------------------------ geometry

/// Distance from `from` to the farthest corner of `rect`: a disc this big covers the window.
fn farthest_corner(from: Pos2, rect: Rect) -> f32 {
    [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
    .iter()
    .map(|c| c.distance(from))
    .fold(0.0, f32::max)
}

fn circle(center: Pos2, r: f32) -> Vec<Pos2> {
    (0..SLICES)
        .map(|k| {
            let a = TAU * k as f32 / SLICES as f32;
            center + r * Vec2::new(a.cos(), a.sin())
        })
        .collect()
}

/// The outline of `rect` with corners of `radius`, as a convex polygon.
fn rounded_polygon(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if r <= 0.5 {
        return vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
    }
    let corners = [
        (rect.right_top() + Vec2::new(-r, r), -FRAC_PI_2),
        (rect.right_bottom() + Vec2::new(-r, -r), 0.0),
        (rect.left_bottom() + Vec2::new(r, -r), FRAC_PI_2),
        (rect.left_top() + Vec2::new(r, r), PI),
    ];
    let mut out = Vec::with_capacity(4 * (CORNER_SEGMENTS + 1));
    for (c, from) in corners {
        for k in 0..=CORNER_SEGMENTS {
            let a = from + FRAC_PI_2 * k as f32 / CORNER_SEGMENTS as f32;
            out.push(c + r * Vec2::new(a.cos(), a.sin()));
        }
    }
    out
}

/// Where the segment `p`-`q` crosses the line through `a` and `b`.
fn crossing(p: Pos2, q: Pos2, a: Pos2, b: Pos2) -> Pos2 {
    let d1 = q - p;
    let d2 = b - a;
    let denom = d1.x * d2.y - d1.y * d2.x;
    if denom.abs() < 1e-6 {
        return q;
    }
    let t = ((a.x - p.x) * d2.y - (a.y - p.y) * d2.x) / denom;
    p + d1 * t
}

/// `subject` cut to the inside of the convex polygon `clip` (Sutherland-Hodgman). Both are convex,
/// so the result is too.
fn clip_convex(subject: Vec<Pos2>, clip: &[Pos2]) -> Vec<Pos2> {
    let area: f32 = (0..clip.len())
        .map(|i| {
            let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let side = if area >= 0.0 { 1.0 } else { -1.0 };
    let mut out = subject;
    for i in 0..clip.len() {
        let (a, b) = (clip[i], clip[(i + 1) % clip.len()]);
        let inside =
            |p: Pos2| ((b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)) * side >= 0.0;
        let input = std::mem::take(&mut out);
        let Some(&last) = input.last() else {
            break;
        };
        let mut prev = last;
        for &cur in &input {
            let (cur_in, prev_in) = (inside(cur), inside(prev));
            if cur_in != prev_in {
                out.push(crossing(prev, cur, a, b));
            }
            if cur_in {
                out.push(cur);
            }
            prev = cur;
        }
    }
    out
}

// ------------------------------------------------------------------ painting

/// A convex polygon, soft at its edge.
fn fill_convex(painter: &egui::Painter, points: Vec<Pos2>, color: Color32) {
    if points.len() >= 3 {
        painter.add(Shape::convex_polygon(points, color, egui::Stroke::NONE));
    }
}

/// The ring between the radii `r0` and `r1` around `center`, cut to the window `shape`. It is made
/// of slices that share their cut edges, so no seam shows between them.
fn annulus(
    painter: &egui::Painter,
    center: Pos2,
    r0: f32,
    r1: f32,
    color: Color32,
    shape: &[Pos2],
) {
    if r1 <= r0 {
        return;
    }
    let dir = |k: usize| {
        let a = TAU * k as f32 / SLICES as f32;
        Vec2::new(a.cos(), a.sin())
    };
    let mut mesh = egui::Mesh::default();
    for k in 0..SLICES {
        let (d0, d1) = (dir(k), dir(k + 1));
        let quad = vec![
            center + r0 * d0,
            center + r1 * d0,
            center + r1 * d1,
            center + r0 * d1,
        ];
        let cut = clip_convex(quad, shape);
        if cut.len() < 3 {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        for p in &cut {
            mesh.colored_vertex(*p, color);
        }
        for i in 1..cut.len() as u32 - 1 {
            mesh.add_triangle(base, base + i, base + i + 1);
        }
    }
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(400.0, 300.0))
    }

    fn area(points: &[Pos2]) -> f32 {
        (0..points.len())
            .map(|i| {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f32>()
            .abs()
            / 2.0
    }

    #[test]
    fn the_ease_runs_from_zero_to_one_and_never_goes_back() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert!((ease_in_out(1.0) - 1.0).abs() < 1e-6);
        let mut last = 0.0;
        for k in 0..=100 {
            let v = ease_in_out(k as f32 / 100.0);
            assert!(v >= last - 1e-6, "step {k}");
            last = v;
        }
    }

    #[test]
    fn a_disc_that_reaches_the_far_corner_covers_the_window() {
        let (card, from) = (rect(), Pos2::new(60.0, 50.0));
        let shape = rounded_polygon(card, 24.0);
        let cut = clip_convex(circle(from, farthest_corner(from, card) + 6.0), &shape);
        let (got, want) = (area(&cut), area(&shape));
        assert!((got - want).abs() < want * 1e-3, "{got} against {want}");
    }

    #[test]
    fn nothing_is_left_outside_the_rounded_corners() {
        let card = rect();
        let shape = rounded_polygon(card, 24.0);
        let cut = clip_convex(circle(card.center(), 5000.0), &shape);
        // the corner point of the rectangle is cut away by the rounding
        assert!(cut.iter().all(|p| p.distance(card.left_top()) > 5.0));
        assert!(cut.iter().all(|p| card.expand(0.01).contains(*p)));
    }

    #[test]
    fn a_small_disc_is_not_changed_by_the_clip() {
        let card = rect();
        let shape = rounded_polygon(card, 24.0);
        let disc = circle(card.center(), 40.0);
        let cut = clip_convex(disc.clone(), &shape);
        assert!((area(&cut) - area(&disc)).abs() < 1e-2);
    }

    #[test]
    fn a_disc_far_from_the_window_leaves_nothing() {
        let shape = rounded_polygon(rect(), 24.0);
        assert!(clip_convex(circle(Pos2::new(-900.0, -900.0), 20.0), &shape).is_empty());
    }

    #[test]
    fn both_windings_of_the_window_give_the_same_cut() {
        let card = rect();
        let mut shape = rounded_polygon(card, 24.0);
        let a = area(&clip_convex(circle(card.center(), 90.0), &shape));
        shape.reverse();
        let b = area(&clip_convex(circle(card.center(), 90.0), &shape));
        assert!((a - b).abs() < 1e-2);
    }
}
