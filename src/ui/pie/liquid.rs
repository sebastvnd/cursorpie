//! The liquid look of the glass pie: [rust-glass](https://crates.io/crates/rust-glass) draws the
//! slices off screen, egui shows the picture.
//!
//! ## Why it is not a drop-in
//!
//! rust-glass is a framework of its own (wgpu and winit) and CursorPie draws with egui on a
//! transparent window (`glow` on Windows and Linux), so the two cannot share a surface. Three
//! things shape this module:
//!
//! - **Off screen.** A worker thread owns a rust-glass `Headless` renderer. The pie sends it a
//!   [`Scene`] (the slices as numbers), it draws the glass on its own GPU device and sends back one
//!   picture; the pie turns the picture into an egui texture and paints it *under* the icons. The
//!   UI never waits for the GPU: the newest scene wins, and a picture that is a frame old is shown
//!   until the next one arrives.
//! - **Its glass is opaque and only knows rounded rectangles.** The renderer clears to an opaque
//!   color and a glass shape is a superellipse, never a ring sector. So each slice is a chain of
//!   overlapping circles along the ring that the shader fuses like drops of liquid (see
//!   [`circles`]), and the alpha comes from a second copy of the same shapes in the right half of
//!   the canvas, drawn as flat white on black (see [`pack`]). One canvas, one read back.
//! - **There is nothing to refract.** A transparent window cannot read the screen behind it, so
//!   the glass bends a soft graphite backdrop of its own (`gpu::backdrop`) and is shown with the
//!   transparency of the *Clearness* setting. The rim, the dispersion, the bevel and the highlight
//!   that follows one light are the real material of rust-glass; what is behind the pie shows
//!   through the translucency, not through the lens.
//!
//! Everything that can go wrong falls back to the painted glass of [`super::glass`]: a build
//! without the `liquid-glass` feature, a machine without a usable GPU, a renderer that panics,
//! a custom pie outline (the shapes only follow a circle), or the first moments after start while
//! the worker is still being set up.

use super::glass::Glass;
use eframe::egui;
use std::f32::consts::TAU;
use std::sync::mpsc::{Receiver, Sender};

/// Whether this build can draw liquid glass (the `liquid-glass` feature).
pub const AVAILABLE: bool = cfg!(feature = "liquid-glass");

/// Points of room between the pie and the edge of its picture. The glass samples the backdrop up to
/// 30 points away from its outline (bend, bleed and blur), and the right half of the canvas must
/// stay out of reach of the left one.
const MARGIN: f32 = 40.0;
/// Most pixels along one side of the picture. A bigger pie is drawn smaller and scaled up by egui.
const MAX_SIDE_PX: f32 = 800.0;
/// Fewest pixels per point: a pie as big as the screen is still drawn at this density.
const MIN_SCALE: f32 = 0.75;
/// How far, in points, neighbouring circles of one slice melt into each other. It must stay below
/// the gap between two slices, or they would join.
const FUSE: f32 = 4.0;
/// Distance between the centers of neighbouring circles of a slice, in radii of a circle. Small
/// enough that the scallops between two circles are filled by [`FUSE`].
const STEP: f32 = 0.35;

/// One piece of glass: a slice of the ring, the whole ring of a lone slice, or the center disc
/// (`r0` of 0). Angles are the ones the pie paints with (radians, y down), lengths are points
/// from the center of the pie.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub a0: f32,
    pub a1: f32,
    pub r0: f32,
    pub r1: f32,
    /// The color of the glass (the slice color, blended toward the accent while hovered).
    pub tint: [u8; 3],
    /// How strong that color is, `0.0..=1.0`.
    pub tint_a: f32,
    /// How opaque the piece is on the screen (the *Clearness* setting), `0.0..=1.0`.
    pub alpha: f32,
    /// How far the piece has come out (`0.0..=1.0`): it scales the bend of the light and the alpha.
    pub presence: f32,
    /// An even brightening, the response to the pointer, `0.0..=1.0`.
    pub lit: f32,
}

impl Piece {
    /// A slice between the radii `(inner, outer)` and the angles `(s0, s1)`. `h` is how hovered it
    /// is and `lf` how far its entrance went, both `0.0..=1.0`; `base` is its color.
    pub fn slice(
        (s0, s1): (f32, f32),
        (inner, outer): (f32, f32),
        base: [u8; 3],
        (h, lf): (f32, f32),
        glass: Glass,
    ) -> Self {
        let body = glass.body_alpha();
        Self {
            a0: s0,
            a1: s1,
            r0: inner,
            r1: outer,
            tint: base,
            tint_a: 0.30 + 0.45 * h,
            // a hovered slice becomes more solid, so the accent reads (the painted glass does too)
            alpha: body + (1.0 - body) * 0.55 * h,
            presence: lf,
            lit: 0.06 * h,
        }
    }

    /// The disc in the middle, of `radius`, tinted `base`, as present as `fade`.
    pub fn disc(radius: f32, base: [u8; 3], fade: f32, glass: Glass) -> Self {
        Self {
            a0: 0.0,
            a1: TAU,
            r0: 0.0,
            r1: radius,
            tint: base,
            tint_a: 0.60,
            alpha: (glass.body_alpha() + 0.25).min(1.0),
            presence: fade,
            lit: 0.0,
        }
    }
}

/// Everything the worker needs to draw one picture. Two equal scenes are the same picture.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// Side of the square picture, in points. The center of the pie is the middle of it.
    pub side: f32,
    /// Side of the left half of the canvas, in pixels. The canvas is `2 * px` wide and `px` high.
    pub px: u32,
    /// The frost and the shine of the settings, `0.0..=1.0`.
    pub frost: f32,
    pub shine: f32,
    /// Which pie this is. Set by [`Link::submit`]; a picture of an older pie is never shown.
    pub epoch: u32,
    pub pieces: Vec<Piece>,
}

impl Scene {
    /// The scene of `pieces` for a pie whose ring reaches `room` points from the center, on a
    /// display of `ppp` pixels per point.
    pub fn new(room: f32, ppp: f32, glass: Glass, pieces: Vec<Piece>) -> Self {
        let (side, px) = canvas(room, ppp);
        Self {
            side,
            px,
            frost: glass.frost,
            shine: glass.shine,
            epoch: 0,
            pieces,
        }
    }
}

/// The size of the picture of a pie whose ring reaches `room` points from its center: its side in
/// points (a whole, even number) and in pixels. The density is the one of the display, at most 2
/// and low enough for the side to stay within the maximum side length.
pub fn canvas(room: f32, ppp: f32) -> (f32, u32) {
    let side = (room.max(1.0) + MARGIN).ceil() * 2.0;
    let scale = ppp.clamp(1.0, 2.0).min(MAX_SIDE_PX / side).max(MIN_SCALE);
    let px = (side * scale).round().max(16.0) as u32;
    (side, px)
}

/// One circle of a chain, with its center measured from the center of the pie.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Circle {
    pub x: f32,
    pub y: f32,
    pub r: f32,
}

/// The circles whose union is `piece`.
///
/// The glass shader knows rounded rectangles only, so a ring sector is built from circles of the
/// thickness of the ring: they run along the middle of the ring, the first and the last one a cap
/// inside the slice, and the shader melts them using the fuse operation.
/// The slice comes out as a capsule
/// bent along the ring, with round ends, which is what a drop of liquid on a ring would be. The
/// disc in the middle is one circle, and a lone slice (a full turn) is a closed chain.
pub fn circles(piece: &Piece) -> Vec<Circle> {
    let thick = piece.r1 - piece.r0;
    let sweep = piece.a1 - piece.a0;
    // (written so that a NaN gives nothing)
    if !(thick >= 0.5 && sweep > 1e-3) {
        return Vec::new();
    }
    if piece.r0 <= 0.5 {
        return vec![Circle {
            x: 0.0,
            y: 0.0,
            r: piece.r1,
        }];
    }
    let mid = (piece.r0 + piece.r1) / 2.0;
    let rc = thick / 2.0;
    let at = |a: f32, r: f32| Circle {
        x: mid * a.cos(),
        y: mid * a.sin(),
        r,
    };
    let spacing = rc * STEP;
    if sweep >= TAU - 1e-3 {
        let n = ((TAU * mid) / spacing).ceil().max(8.0) as usize;
        return (0..n)
            .map(|k| at(piece.a0 + sweep * k as f32 / n as f32, rc))
            .collect();
    }
    // the angle a circle of the thickness of the ring reaches to either side of its center
    let cap = (rc / mid).min(1.0).asin();
    let run = sweep - 2.0 * cap;
    if run <= 1e-3 {
        // a slice narrower than the ring is thick: one circle that fits between its sides
        let r = rc.min(mid * (sweep / 2.0).sin()).max(0.5);
        return vec![at(piece.a0 + sweep / 2.0, r)];
    }
    let n = ((run * mid) / spacing).ceil() as usize + 1;
    (0..n)
        .map(|k| at(piece.a0 + cap + run * k as f32 / (n - 1) as f32, rc))
        .collect()
}

/// Turns the canvas the worker read back (`2 * px` by `px`, RGBA, no padding) into the picture to
/// show: the color of the left half and, as its alpha, the red of the right half, where the same
/// shapes were drawn white on black. Premultiplied, as egui wants it. `None` when `rgba` is too
/// short for the canvas.
pub fn pack(rgba: &[u8], px: u32) -> Option<egui::ColorImage> {
    let w = px as usize;
    let row = w * 2 * 4;
    if w == 0 || rgba.len() < row * w {
        return None;
    }
    let mut out = Vec::with_capacity(w * w * 4);
    for line in rgba.chunks_exact(row).take(w) {
        for x in 0..w {
            let color = &line[x * 4..x * 4 + 4];
            let alpha = u32::from(line[(w + x) * 4]);
            for &channel in &color[..3] {
                out.push(((u32::from(channel) * alpha + 127) / 255) as u8);
            }
            out.push(alpha as u8);
        }
    }
    Some(egui::ColorImage::from_rgba_premultiplied([w, w], &out))
}

/// What the worker sends back.
pub enum Reply {
    /// A picture of `side` points, drawn for the pie of `epoch`.
    Frame {
        image: egui::ColorImage,
        side: f32,
        epoch: u32,
    },
    /// The renderer cannot go on (no GPU, a panic): the painted glass takes over for good.
    Dead(String),
}

/// The two ends of the line to the worker.
type Wire = (Sender<Scene>, Receiver<Reply>);

/// What the pie should do with its glass this frame.
#[derive(Clone, Copy, Debug)]
pub enum Draw {
    /// Paint the glass as before (no liquid glass here, or not yet).
    Painted,
    /// The worker is running but has no picture of this pie yet: draw no glass for a frame or two,
    /// the pie is still coming out and is nearly clear.
    Pending,
    /// Paint this texture, `side` points wide, centered on the pie, below the icons.
    Ready { id: egui::TextureId, side: f32 },
}

#[derive(Default)]
enum Run {
    /// Not started: the worker is made the first time the pie asks.
    #[default]
    Off,
    On(Sender<Scene>, Receiver<Reply>),
    /// Gone for good (see [`Reply::Dead`]).
    Dead,
}

/// The pie's end of the worker: starts it when it is first needed, sends it scenes, keeps the
/// newest picture as a texture. It lives in the app, on the UI thread.
#[derive(Default)]
pub struct Link {
    run: Run,
    tex: Option<egui::TextureHandle>,
    /// Side, in points, and pie of the picture in `tex`.
    tex_side: f32,
    tex_epoch: u32,
    /// A picture has come at least once since the worker started.
    warm: bool,
    /// The pie that is open now (it counts up every time a pie has closed).
    epoch: u32,
    /// The last scene given to the worker, so an unchanged pie does not send anything.
    sent: Option<Scene>,
}

impl Link {
    /// Collects what the worker has sent and says what to draw. Starts the worker the first time.
    pub fn poll(&mut self, ctx: &egui::Context) -> Draw {
        if matches!(self.run, Run::Off) {
            self.run = match spawn(ctx) {
                Ok((tx, rx)) => Run::On(tx, rx),
                Err(why) => {
                    diag::log!("ui", "liquid_off", why = ?why);
                    Run::Dead
                }
            };
        }
        let mut frame = None;
        let mut dead = None;
        if let Run::On(_, rx) = &self.run {
            while let Ok(reply) = rx.try_recv() {
                match reply {
                    Reply::Frame { image, side, epoch } => frame = Some((image, side, epoch)),
                    Reply::Dead(why) => dead = Some(why),
                }
            }
        }
        if let Some(why) = dead {
            diag::log!("ui", "liquid_off", why = ?why);
            self.run = Run::Dead;
            self.sent = None;
        }
        if let Some((image, side, epoch)) = frame {
            match self.tex.as_mut() {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.tex = Some(ctx.load_texture(
                        "cursorpie-liquid",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }
            self.tex_side = side;
            self.tex_epoch = epoch;
            self.warm = true;
        }
        match (&self.run, &self.tex) {
            (Run::Dead, _) => Draw::Painted,
            (_, Some(tex)) if self.tex_epoch == self.epoch => Draw::Ready {
                id: tex.id(),
                side: self.tex_side,
            },
            _ if self.warm => Draw::Pending,
            _ => Draw::Painted,
        }
    }

    /// Hands the scene of this frame to the worker, unless it is the one it already has.
    pub fn submit(&mut self, mut scene: Scene) {
        scene.epoch = self.epoch;
        if self.sent.as_ref() == Some(&scene) {
            return;
        }
        let sent = match &self.run {
            Run::On(tx, _) => Some(tx.send(scene.clone()).is_ok()),
            _ => None,
        };
        match sent {
            Some(true) => self.sent = Some(scene),
            Some(false) => self.run = Run::Dead,
            None => {}
        }
    }

    /// A pie has closed: the picture of it must not be seen again, and the next pie starts from
    /// its first scene.
    pub fn pie_closed(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.sent = None;
    }

    /// Gives back what the glass holds: the worker (and with it the GPU device and the fonts
    /// rust-glass loads), the texture and the scene. The next pie that asks starts it again.
    pub fn release(&mut self) {
        *self = Self::default();
    }
}

#[cfg(not(feature = "liquid-glass"))]
fn spawn(_: &egui::Context) -> Result<Wire, String> {
    Err("built without the liquid-glass feature".to_owned())
}

#[cfg(feature = "liquid-glass")]
use gpu::spawn;

/// The worker: rust-glass on a thread of its own. Nothing of rust-glass leaves this module.
#[cfg(feature = "liquid-glass")]
mod gpu {
    use super::{Circle, FUSE, Piece, Reply, Scene, Wire, circles, pack};
    use eframe::egui;
    use rust_glass::headless::Headless;
    use rust_glass::{
        App, Color, Context, Corner, FrameInfo, Glass, GlassShape, Id, Material, Order, Rect, vec2,
    };
    use std::f32::consts::FRAC_PI_4;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::mpsc::{Receiver, Sender, channel};

    /// Starts the worker. `repaint` is woken every time a picture is ready.
    pub fn spawn(repaint: &egui::Context) -> Result<Wire, String> {
        let (scene_tx, scene_rx) = channel::<Scene>();
        let (reply_tx, reply_rx) = channel::<Reply>();
        let repaint = repaint.clone();
        std::thread::Builder::new()
            .name("cursorpie-liquid".into())
            .spawn(move || {
                let outcome =
                    catch_unwind(AssertUnwindSafe(|| work(scene_rx, &reply_tx, &repaint)));
                let why = match outcome {
                    Ok(Ok(())) => return,
                    Ok(Err(why)) => why,
                    Err(_) => "the renderer panicked".to_owned(),
                };
                let _ = reply_tx.send(Reply::Dead(why));
                repaint.request_repaint();
            })
            .map_err(|e| e.to_string())?;
        Ok((scene_tx, reply_rx))
    }

    /// Draws every scene it is given, newest first, until the app lets go of the line.
    fn work(
        scenes: Receiver<Scene>,
        out: &Sender<Reply>,
        repaint: &egui::Context,
    ) -> Result<(), String> {
        // the renderer, and the canvas size and density it was made for
        let mut canvas: Option<(Headless, (u32, u32))> = None;
        while let Ok(mut scene) = scenes.recv() {
            while let Ok(newer) = scenes.try_recv() {
                scene = newer;
            }
            let ppp = scene.px as f32 / scene.side;
            let key = (scene.px, (ppp * 1000.0).round() as u32);
            if canvas.as_ref().map(|c| c.1) != Some(key) {
                // the old device goes before the new one is made
                drop(canvas.take());
                canvas = Some((Headless::new(scene.px * 2, scene.px, ppp)?, key));
            }
            let Some((gl, _)) = canvas.as_mut() else {
                continue;
            };
            let (px, side, epoch) = (scene.px, scene.side, scene.epoch);
            let mut app = Canvas { scene };
            gl.step(&mut app, 1.0 / 60.0, &[]);
            let rgba = gl.pixels();
            let Some(image) = pack(&rgba, px) else {
                continue;
            };
            if out.send(Reply::Frame { image, side, epoch }).is_err() {
                return Ok(());
            }
            repaint.request_repaint();
        }
        Ok(())
    }

    /// The rust-glass app that draws a scene.
    struct Canvas {
        scene: Scene,
    }

    impl App for Canvas {
        fn update(&mut self, ctx: &mut Context, _: &FrameInfo) {
            draw(ctx, &self.scene);
        }
    }

    /// Draws every piece twice: in the left half of the canvas as glass over the backdrop, in the
    /// right half as flat white (a gray of `alpha`) over black, which is its alpha.
    fn draw(ctx: &mut Context, scene: &Scene) {
        ctx.clear = Color::BLACK;
        backdrop(ctx, scene.side);
        let layer = ctx.layer(Id::new("cursorpie-liquid"), Order::Content);
        let clip = ctx.screen;
        let mut painter = ctx.painter(layer, clip);
        let middle = vec2(scene.side / 2.0, scene.side / 2.0);
        for piece in &scene.pieces {
            let list = circles(piece);
            if list.is_empty() {
                continue;
            }
            for (dx, is_mask) in [(0.0, false), (scene.side, true)] {
                let shapes = list
                    .iter()
                    .map(|c| shape(c, vec2(middle.x + dx, middle.y)))
                    .collect();
                let material = if is_mask {
                    mask_material(piece.alpha)
                } else {
                    color_material(piece, scene)
                };
                let mut glass = Glass::group(shapes, material)
                    .presence(piece.presence)
                    .fuse(FUSE)
                    // the pie is smoked glass in either appearance
                    .darkness(1.0);
                // the light comes from the top left, as it does on the painted glass
                glass.light_bend = -FRAC_PI_4;
                glass.lit = if is_mask { 0.0 } else { piece.lit };
                painter.glass(&glass);
            }
        }
    }

    fn shape(c: &Circle, origin: rust_glass::Vec2) -> GlassShape {
        let center = vec2(origin.x + c.x, origin.y + c.y);
        GlassShape::new(
            Rect::from_center_size(center, vec2(c.r * 2.0, c.r * 2.0)),
            Corner::Round(c.r),
        )
    }

    /// The material of a slice: the regular glass of rust-glass, shaped by the three settings. The
    /// shadow is off, the pie paints its own.
    fn color_material(piece: &Piece, scene: &Scene) -> Material {
        let [r, g, b] = piece.tint.map(|v| f32::from(v) / 255.0);
        Material {
            refraction: 16.0,
            depth: 14.0,
            outer_refraction: 3.0,
            outer_depth: 4.0,
            chroma: 0.10 + 0.10 * scene.shine,
            blur: 1.5 + 9.0 * scene.frost,
            dark_range: (0.06 + 0.10 * scene.frost, 0.52 + 0.18 * scene.frost),
            saturation: 1.25,
            rim: 0.30 + 0.70 * scene.shine,
            rim_base: 0.30,
            dark_edge: 0.22,
            bleed: 0.12,
            inner_glow: 0.10 + 0.22 * scene.shine,
            inner_shadow: 0.05,
            shadow: 0.0,
            shadow_radius: 2.0,
            shadow_y: 0.0,
            tint: Some(Color::rgba(r, g, b, piece.tint_a)),
            tint_follow: 0.6,
            ..Material::REGULAR
        }
    }

    /// A material that paints a flat gray of `level` whatever is behind it: the face is mapped to
    /// `level..level`, nothing bends, blurs, shines or casts a shadow. Over black, what is left is
    /// the coverage of the shape times `level`.
    fn mask_material(level: f32) -> Material {
        let level = level.clamp(0.0, 1.0);
        Material {
            refraction: 0.0,
            outer_refraction: 0.0,
            pinch: 0.0,
            chroma: 0.0,
            blur: 0.0,
            light_range: (level, level),
            dark_range: (level, level),
            saturation: 1.0,
            rim: 0.0,
            dark_edge: 0.0,
            bleed: 0.0,
            inner_glow: 0.0,
            inner_shadow: 0.0,
            shadow: 0.0,
            shadow_radius: 1.0,
            shadow_y: 0.0,
            tint: None,
            frosts: 0.0,
            ..Material::REGULAR
        }
    }

    /// What the glass bends: graphite with two soft pools of light, so the rim has something to
    /// catch. Only the left half of the canvas; the right half stays black for the masks.
    fn backdrop(ctx: &mut Context, side: f32) {
        let mut painter = ctx.background();
        let all = Rect::from_min_size(vec2(0.0, 0.0), vec2(side, side));
        painter.gradient(
            all,
            0.0_f32,
            Color::rgb(0.23, 0.25, 0.30),
            Color::rgb(0.06, 0.06, 0.08),
            vec2(0.30, 1.0),
        );
        let pool = |cx: f32, cy: f32, w: f32, h: f32| {
            Rect::from_center_size(vec2(side * cx, side * cy), vec2(side * w, side * h))
        };
        painter.shadow(
            pool(0.30, 0.28, 0.50, 0.36),
            Corner::Round(side * 0.2),
            side * 0.12,
            Color::rgba(0.55, 0.62, 0.80, 0.35),
        );
        painter.shadow(
            pool(0.72, 0.74, 0.46, 0.30),
            Corner::Round(side * 0.15),
            side * 0.10,
            Color::rgba(0.80, 0.58, 0.48, 0.22),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glass() -> Glass {
        Glass {
            clear: 0.6,
            frost: 0.45,
            shine: 0.6,
        }
    }

    fn slice(a0: f32, a1: f32) -> Piece {
        Piece::slice((a0, a1), (80.0, 220.0), [47, 51, 49], (0.0, 1.0), glass())
    }

    #[test]
    fn the_picture_is_a_whole_even_number_of_points_and_never_too_big() {
        for (room, ppp) in [(120.0, 1.0), (238.0, 2.0), (600.0, 2.0), (30.0, 3.0)] {
            let (side, px) = canvas(room, ppp);
            assert_eq!(side % 2.0, 0.0, "{room}");
            assert!(side >= 2.0 * (room + MARGIN) - 1e-3, "{room}");
            assert!(
                px as f32 <= MAX_SIDE_PX.max(side * MIN_SCALE) + 1.0,
                "{room}: {px}"
            );
            assert!(px as f32 / side >= MIN_SCALE - 0.01, "{room}");
        }
    }

    #[test]
    fn a_small_pie_is_drawn_at_the_density_of_the_display_up_to_two() {
        let (side, px) = canvas(100.0, 1.0);
        assert!((px as f32 / side - 1.0).abs() < 0.01);
        let (side, px) = canvas(100.0, 3.0);
        assert!((px as f32 / side - 2.0).abs() < 0.01);
    }

    #[test]
    fn a_slice_is_a_chain_of_circles_along_the_middle_of_the_ring() {
        let p = slice(0.0, 2.0);
        let list = circles(&p);
        assert!(list.len() >= 3, "{list:?}");
        let mid = 150.0;
        for c in &list {
            assert!((c.r - 70.0).abs() < 1e-3);
            assert!((c.x.hypot(c.y) - mid).abs() < 1e-3, "{c:?}");
            let a = c.y.atan2(c.x);
            assert!((0.0..=2.0).contains(&a), "{a}");
        }
        // neighbours overlap deeply: the shader only has to melt the scallops
        for w in list.windows(2) {
            let d = (w[1].x - w[0].x).hypot(w[1].y - w[0].y);
            assert!(d < 70.0, "{d}");
        }
    }

    #[test]
    fn the_ends_of_a_slice_stay_inside_its_sides() {
        let p = slice(0.4, 2.4);
        let list = circles(&p);
        for c in [list[0], list[list.len() - 1]] {
            // the point of the circle that is farthest round the ring
            let a = c.y.atan2(c.x);
            let reach = (c.r / 150.0).asin();
            assert!(a - reach >= 0.4 - 1e-3 && a + reach <= 2.4 + 1e-3, "{a}");
        }
    }

    #[test]
    fn a_slice_narrower_than_the_ring_is_one_circle_that_fits() {
        let p = slice(1.0, 1.2);
        let list = circles(&p);
        assert_eq!(list.len(), 1);
        let c = list[0];
        // it touches neither side: the distance from the center to a side line is mid * sin(half)
        assert!(c.r <= 150.0 * (0.1_f32).sin() + 1e-3, "{}", c.r);
        assert!((c.y.atan2(c.x) - 1.1).abs() < 1e-3);
    }

    #[test]
    fn a_lone_slice_is_a_closed_chain_and_the_core_is_one_circle() {
        let ring = circles(&slice(0.0, TAU));
        assert!(ring.len() >= 8);
        let (first, last) = (ring[0], ring[ring.len() - 1]);
        // the chain closes on itself: the last circle is as near the first as any two neighbours
        let gap = (last.x - first.x).hypot(last.y - first.y);
        let step = (ring[1].x - ring[0].x).hypot(ring[1].y - ring[0].y);
        assert!((gap - step).abs() < step * 0.2, "{gap} {step}");
        let core = circles(&Piece::disc(60.0, [31, 34, 32], 1.0, glass()));
        assert_eq!(
            core,
            vec![Circle {
                x: 0.0,
                y: 0.0,
                r: 60.0
            }]
        );
    }

    #[test]
    fn nothing_is_drawn_for_a_slice_without_area_or_with_a_wrong_number() {
        assert!(circles(&slice(1.0, 1.0)).is_empty());
        for bad in [
            Piece {
                r1: 80.2,
                ..slice(0.0, 1.0)
            },
            Piece {
                a1: f32::NAN,
                ..slice(0.0, 1.0)
            },
            Piece {
                r1: f32::NAN,
                ..slice(0.0, 1.0)
            },
        ] {
            assert!(circles(&bad).is_empty(), "{bad:?}");
        }
    }

    #[test]
    fn a_hovered_slice_is_more_solid_and_more_tinted_and_the_glass_numbers_are_used() {
        let rest = Piece::slice((0.0, 1.0), (80.0, 220.0), [1, 2, 3], (0.0, 1.0), glass());
        let hot = Piece::slice((0.0, 1.0), (80.0, 220.0), [1, 2, 3], (1.0, 1.0), glass());
        assert!(hot.alpha > rest.alpha && hot.tint_a > rest.tint_a && hot.lit > rest.lit);
        assert_eq!(rest.alpha, glass().body_alpha());
        assert!(hot.alpha <= 1.0 && hot.tint_a <= 1.0);
        let entering = Piece::slice((0.0, 1.0), (80.0, 220.0), [1, 2, 3], (0.0, 0.25), glass());
        assert_eq!(entering.presence, 0.25);
    }

    /// A canvas of `px` by `px` per half whose left half is `color` and whose right half is
    /// `mask` in the red channel.
    fn canvas_of(px: usize, color: [u8; 3], mask: u8) -> Vec<u8> {
        let mut v = Vec::new();
        for _ in 0..px {
            for _ in 0..px {
                v.extend_from_slice(&[color[0], color[1], color[2], 255]);
            }
            for _ in 0..px {
                v.extend_from_slice(&[mask, 0, 0, 255]);
            }
        }
        v
    }

    #[test]
    fn the_alpha_of_the_picture_is_the_right_half_and_the_color_is_premultiplied() {
        let img = pack(&canvas_of(4, [200, 100, 50], 255), 4).expect("a picture");
        assert_eq!(img.size, [4, 4]);
        let p = img.pixels[5];
        assert_eq!((p.r(), p.g(), p.b(), p.a()), (200, 100, 50, 255));
        let img = pack(&canvas_of(4, [200, 100, 50], 128), 4).expect("a picture");
        let p = img.pixels[0];
        assert_eq!(p.a(), 128);
        assert_eq!(p.r(), ((200u32 * 128 + 127) / 255) as u8);
        let img = pack(&canvas_of(4, [200, 100, 50], 0), 4).expect("a picture");
        assert!(img.pixels.iter().all(|p| p.a() == 0 && p.r() == 0));
    }

    #[test]
    fn a_canvas_that_is_too_short_gives_no_picture() {
        let v = canvas_of(4, [1, 2, 3], 9);
        assert!(pack(&v[..v.len() - 1], 4).is_none());
        assert!(pack(&[], 0).is_none());
    }

    #[test]
    fn an_unchanged_scene_is_not_sent_again_and_a_closed_pie_starts_over() {
        let scene = || Scene::new(238.0, 2.0, glass(), vec![slice(0.0, 1.0)]);
        assert_eq!(scene(), scene());
        let mut other = scene();
        other.pieces[0].presence = 0.5;
        assert_ne!(scene(), other);
        // a link without a worker takes scenes and does nothing with them
        let mut link = Link::default();
        link.submit(scene());
        assert!(link.sent.is_none());
        let before = link.epoch;
        link.pie_closed();
        assert_eq!(link.epoch, before.wrapping_add(1));
    }

    #[test]
    fn releasing_the_link_forgets_everything() {
        let mut link = Link {
            epoch: 7,
            warm: true,
            ..Link::default()
        };
        link.release();
        assert_eq!(link.epoch, 0);
        assert!(!link.warm && link.tex.is_none() && matches!(link.run, Run::Off));
    }
}
