//! Drawing helpers shared by the pie and the settings window: icons, shapes, easing, colors.

use crate::ui::{fonts, symbols};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Vec2};
use std::f32::consts::TAU;
use std::path::PathBuf;

// --------------------------------------------------------------------- icons
/// If the icon string points to an image file, return its path. Otherwise it is text/emoji.
pub fn icon_path(s: &str) -> Option<PathBuf> {
    crate::config::image_path(s)
}

/// Longest side, in pixels, of an icon texture. Larger images are scaled down once, at load time.
pub const ICON_MAX_PX: u32 = 128;

/// Size an `w` x `h` image gets when it must fit in a `max` x `max` box, keeping its aspect
/// ratio. Images that already fit are returned unchanged: icons are never enlarged here, the GPU
/// does that when the icon is drawn.
pub fn fit_within(w: u32, h: u32, max: u32) -> (u32, u32) {
    let longest = w.max(h);
    if longest <= max || longest == 0 {
        return (w, h);
    }
    let k = max as f32 / longest as f32;
    let scale = |v: u32| ((v as f32 * k).round() as u32).max(1);
    (scale(w), scale(h))
}

/// Scales `img` down to at most `max` px and returns it with **premultiplied** alpha.
///
/// Two things make the result sharper than a plain thumbnail:
///
/// * the colors are premultiplied *before* resizing, so the transparent pixels around a glyph
///   (which are often black in the file) do not bleed a dark halo into its edge;
/// * the resize uses a Lanczos filter, which keeps edges crisp when an icon is drawn at about
///   half of its texture size, as on a Retina display.
///
/// Color and alpha are resized as two separate images. That keeps the result independent of how
/// the `image` crate treats alpha while resizing (it may or may not weight by it).
///
/// Lanczos can overshoot, and a premultiplied pixel must never have a color channel above its
/// alpha (the GPU would add light instead of blending), so the channels are clamped afterwards.
pub fn prepare_icon(mut img: image::RgbaImage, max: u32) -> image::RgbaImage {
    let premul = |c: u8, a: u8| ((c as u16 * a as u16 + 127) / 255) as u8;
    let (src_w, src_h) = (img.width(), img.height());
    let (w, h) = fit_within(src_w, src_h, max);

    // Already small enough: premultiply in place. No second bitmap is allocated (a premultiplied
    // channel is never above its alpha, so the clamp of the resize path is not needed here).
    if (w, h) == (src_w, src_h) {
        for px in img.as_chunks_mut::<4>().0 {
            let a = px[3];
            for v in &mut px[..3] {
                *v = premul(*v, a);
            }
        }
        return img;
    }

    let mut color = image::RgbImage::new(src_w, src_h);
    let mut alpha = image::GrayImage::new(src_w, src_h);
    for (x, y, p) in img.enumerate_pixels() {
        let [r, g, b, a] = p.0;
        color.put_pixel(x, y, image::Rgb([premul(r, a), premul(g, a), premul(b, a)]));
        alpha.put_pixel(x, y, image::Luma([a]));
    }
    // The full-size source is no longer needed: give it back before the (allocating) resize, so a
    // big image never exists in three copies at once.
    drop(img);
    let lanczos = image::imageops::FilterType::Lanczos3;
    color = image::imageops::resize(&color, w, h, lanczos);
    alpha = image::imageops::resize(&alpha, w, h, lanczos);
    let mut out = image::RgbaImage::new(w, h);
    for (x, y, p) in out.enumerate_pixels_mut() {
        let [r, g, b] = color.get_pixel(x, y).0;
        let a = alpha.get_pixel(x, y).0[0];
        *p = image::Rgba([r.min(a), g.min(a), b.min(a), a]);
    }
    out
}

/// Longest side, in pixels, of a source image that is decoded at all. A decoded bitmap costs
/// 4 bytes per pixel, so 4096 x 4096 is already 64 MiB for something drawn at 30 to 120 points.
/// Bigger files are refused (the slice shows "?") instead of risking a memory spike.
pub const MAX_SOURCE_SIDE: u32 = 4096;

/// Opens `path` with its format guessed from the content, without decoding anything yet.
pub(crate) fn open_reader(
    path: &std::path::Path,
) -> Option<image::ImageReader<std::io::BufReader<std::fs::File>>> {
    image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()
}

/// Bytes as MiB with one decimal, for the log.
fn round_mb(bytes: u64) -> f64 {
    (bytes as f64 / (1024.0 * 1024.0) * 10.0).round() / 10.0
}

/// Decodes an icon file to RGBA, but only if its size (read from the header, nothing is decoded
/// for that) is within [`MAX_SOURCE_SIDE`].
fn decode_icon(path: &std::path::Path) -> Option<image::RgbaImage> {
    let (w, h) = open_reader(path)?.into_dimensions().ok()?;
    if w > MAX_SOURCE_SIDE || h > MAX_SOURCE_SIDE {
        diag::log!("mem", "image_refused", width = w, height = h);
        return None;
    }
    // a big source costs 4 bytes per pixel while it is decoded: say so when it is 4 MiB or more
    if u64::from(w) * u64::from(h) * 4 >= 4 << 20 {
        diag::log!(
            "mem",
            "big_image",
            width = w,
            height = h,
            decoded_mb = round_mb(u64::from(w) * u64::from(h) * 4)
        );
    }
    // `into_rgba8` reuses the buffer when the file is already RGBA, `to_rgba8` would copy it
    Some(open_reader(path)?.decode().ok()?.into_rgba8())
}

/// Loads an image file as a texture (scaled down to at most [`ICON_MAX_PX`], see [`prepare_icon`]).
/// Only the small texture is kept: the decoded source bitmap is dropped before this returns.
pub fn load_icon(
    ctx: &egui::Context,
    path: &std::path::Path,
    name: &str,
) -> Option<egui::TextureHandle> {
    let rgba = prepare_icon(decode_icon(path)?, ICON_MAX_PX);
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_premultiplied(size, rgba.as_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

/// Draws an icon centered at `c` in a box of `size`, in the color `ink`.
///
/// An icon string is one of three things, see [`crate::ui::symbols`]:
///
/// * an image file: drawn as it is, so a colored icon stays colored; only `ink`'s opacity applies;
/// * `ico:<name>` (or one of the old letter icons, which are mapped to a picture): a glyph of the
///   icon font, drawn in `ink`;
/// * any other text or emoji: drawn in `ink` with the text font.
pub fn draw_icon(
    painter: &egui::Painter,
    tex: Option<&egui::TextureHandle>,
    icon: &str,
    c: Pos2,
    size: f32,
    ink: Color32,
) {
    if let Some(t) = tex {
        draw_texture(painter, t, c, size, Color32::from_white_alpha(ink.a()));
    } else if let Some(glyph) = symbols::resolve(icon) {
        fonts::paint_icon(painter, c, glyph, size, ink, false);
    } else {
        // an image path that failed to load shows "?" so the problem is visible
        let text = if icon_path(icon).is_some() { "?" } else { icon };
        // a long text (an old "Shot" or "Find") must still fit the box
        let shrink = if text.chars().count() > 2 { 0.5 } else { 0.8 };
        painter.text(
            c,
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(size * shrink),
            ink,
        );
    }
}

/// Draws a texture centered at `c`, scaled to fit a box of `size`, multiplied by `tint`.
/// The white glyph of an app icon is drawn with a dark `tint` to make it dark.
pub fn draw_texture(
    painter: &egui::Painter,
    tex: &egui::TextureHandle,
    c: Pos2,
    size: f32,
    tint: Color32,
) {
    let sz = tex.size_vec2();
    let k = size / sz.x.max(sz.y).max(1.0);
    let rect = egui::Rect::from_center_size(c, sz * k);
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    painter.image(tex.id(), rect, uv, tint);
}

// ------------------------------------------------------------------- drawing
/// Moves `v` toward `target` exponentially (frame-rate independent). Returns `true` while still moving,
/// so the caller knows to request another repaint.
pub fn smooth_approach(v: &mut f32, target: f32, dt: f32, speed: f32) -> bool {
    let diff = target - *v;
    if diff.abs() < 0.001 {
        *v = target;
        return false;
    }
    *v += diff * (1.0 - (-dt * speed).exp());
    true
}

/// Moves `v` toward `target` at a constant speed, so that a full 0 to 1 trip takes `seconds`.
/// Returns the new value and whether it is still moving.
///
/// Open and close animations use this instead of [`smooth_approach`]: an exponential approach
/// that is then fed through an easing curve is eased twice, which flattens the overshoot and
/// leaves a long, nearly invisible tail that reads as lag. A linear clock plus *one* easing
/// curve ([`ease_out_cubic`], [`ease_out_back`]) has a fixed duration and a crisp end.
pub fn step_linear(v: f32, target: f32, dt: f32, seconds: f32) -> (f32, bool) {
    let step = dt.max(0.0) / seconds.max(1e-3);
    let next = if v < target {
        (v + step).min(target)
    } else {
        (v - step).max(target)
    };
    (next, next != target)
}

/// Easing with a little overshoot at the end.
pub fn ease_out_back(t: f32) -> f32 {
    ease_out_back_with(t, 1.70158)
}

/// [`ease_out_back`] with the strength of the overshoot chosen: `1.70158` overshoots by about 10%,
/// `1.0` by about 5%.
pub fn ease_out_back_with(t: f32, c1: f32) -> f32 {
    let c3 = c1 + 1.0;
    let u = t - 1.0;
    1.0 + c3 * u * u * u + c1 * u * u
}

/// A damped spring that bounces: it starts at 0, overshoots 1 by about 13%, dips under it and
/// settles on exactly 1 at `t = 1` (and stays there). `t` is clamped to 0..=1.
pub fn spring_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t >= 1.0 {
        return 1.0;
    }
    // 1.25 turns: the cosine is 0 at t = 1, so the spring arrives at rest without a jump
    1.0 - (-5.0 * t).exp() * (std::f32::consts::TAU * 1.25 * t).cos()
}

/// Easing that starts fast and settles smoothly, with no overshoot. `t` is clamped to 0..=1.
pub fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

// --------------------------------------------------------------------- contrast
/// Text color on a light surface or a dark accent: near black.
pub const INK: Color32 = Color32::from_rgb(28, 28, 32);

/// Relative luminance of `c` (WCAG 2), 0 for black and 1 for white.
pub fn luminance(c: Color32) -> f32 {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b, _] = c.to_array();
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

/// WCAG contrast ratio of two colors: 1 (identical) to 21 (black on white).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Text color for something filled with `accent`: dark or white, whichever reads better.
/// The accent is chosen by the user, so a fixed white would be unreadable on a light one.
pub fn on_accent(accent: Color32) -> Color32 {
    if contrast(accent, INK) >= contrast(accent, Color32::WHITE) {
        INK
    } else {
        Color32::WHITE
    }
}

// ------------------------------------------------------------------- appearance and pie palette
/// Whether the interface is drawn dark right now. One flag for the whole UI thread, set once a
/// frame by [`set_dark`] (see `ui::settings::theme::apply_appearance`).
static DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the interface is drawn dark right now.
pub fn dark() -> bool {
    DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Switches the interface between light and dark.
pub fn set_dark(on: bool) {
    DARK.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The colors of the pie in one appearance.
#[derive(Clone, Copy, Debug)]
pub struct PieColors {
    /// A slice at rest. The hovered slice blends toward the accent.
    pub slice: [u8; 3],
    /// A slice that is only a "+" to add one in the Settings pie.
    pub ghost: [u8; 3],
    /// The icon and name on a slice at rest.
    pub ink: Color32,
    /// The center disc.
    pub core: Color32,
}

/// The pie colors for the current appearance: frosted paper white, or smoked graphite with a sage
/// tint (the neutrals of the settings palette, see `ui::settings::theme`).
pub fn pie_colors() -> PieColors {
    pie_colors_for(dark())
}

/// The pie colors of one appearance, whatever the interface is showing right now.
pub fn pie_colors_for(dark: bool) -> PieColors {
    if dark {
        PieColors {
            slice: [47, 51, 49],
            ghost: [37, 41, 39],
            ink: Color32::from_rgb(244, 244, 239),
            core: Color32::from_rgb(31, 34, 32),
        }
    } else {
        PieColors {
            slice: [255, 254, 250],
            ghost: [238, 238, 229],
            ink: Color32::from_rgb(41, 54, 47),
            core: Color32::from_rgb(255, 254, 250),
        }
    }
}

/// The colors of the glass pie. Glass is always smoked, in the light appearance too: frosted
/// white glass over a light page has nothing to catch the light against and looks washed out,
/// which is not how Liquid Glass looks.
pub fn glass_colors() -> PieColors {
    pie_colors_for(true)
}

/// `[r, g, b]` as an egui color.
pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Linear blend of two colors; `t` = 0 gives `a`, 1 gives `b`.
pub fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

/// Rounds `v` (in points) to the nearest physical pixel, given `ppp` pixels per point.
/// Lines and edges that sit between two pixels are blurred over both; this puts them on one.
pub fn snap_to_pixel(v: f32, ppp: f32) -> f32 {
    if ppp > 0.0 {
        (v * ppp).round() / ppp
    } else {
        v
    }
}

/// [`snap_to_pixel`] for both corners of a rectangle.
pub fn snap_rect(r: Rect, ppp: f32) -> Rect {
    Rect::from_min_max(
        egui::pos2(snap_to_pixel(r.min.x, ppp), snap_to_pixel(r.min.y, ppp)),
        egui::pos2(snap_to_pixel(r.max.x, ppp), snap_to_pixel(r.max.y, ppp)),
    )
}

/// How many straight segments an arc of `radius` and `sweep` radians needs so that the curve
/// never strays more than `tolerance` points from the true circle.
///
/// A fixed angular step (the old `0.06` rad) is too coarse for a big ring and wasteful for a
/// small one; the error of a chord is `r * (1 - cos(step / 2))`, which this inverts.
pub fn arc_segments(radius: f32, sweep: f32, tolerance: f32) -> usize {
    let r = radius.max(0.5);
    let tol = tolerance.clamp(0.001, r);
    let step = 2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos();
    ((sweep.abs() / step.max(0.01)).ceil() as usize).max(2)
}

/// Largest distance, in points, between a drawn arc and the true circle.
const ARC_TOLERANCE: f32 = 0.02;

/// Points along an arc, for stroking progress arcs and outlines.
pub fn arc_points(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let steps = arc_segments(r, a1 - a0, ARC_TOLERANCE);
    (0..=steps)
        .map(|k| {
            let a = a0 + (a1 - a0) * k as f32 / steps as f32;
            c + Vec2::new(a.cos(), a.sin()) * r
        })
        .collect()
}

/// The closed outline of a donut slice (outer arc one way, inner arc back), for a hairline stroke.
pub fn ring_slice_outline(c: Pos2, r0: f32, r1: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let mut pts = arc_points(c, r1, a0, a1);
    pts.extend(arc_points(c, r0, a1, a0));
    pts
}

/// Mesh of a donut slice whose edges fade out over `feather` points.
///
/// egui anti-aliases the shapes it tessellates itself (circles, rects, paths), but a hand-made
/// [`egui::Mesh`] is drawn as is, so a plain triangle strip has hard, stair-stepped edges. This
/// mesh adds one extra ring of vertices outside every edge with a fully transparent color. The
/// GPU interpolates between the solid color and transparency across `feather` points, which is
/// exactly what egui's own anti-aliasing does. Use `1 / pixels_per_point` for one pixel.
///
/// The vertices form a 4-row grid (inner fringe, inner edge, outer edge, outer fringe), centered
/// on the true edges, with the same on the two radial sides. A full circle has no radial sides
/// and so no seam. An empty mesh is returned when the slice has no area.
pub fn ring_slice_mesh(
    c: Pos2,
    r0: f32,
    r1: f32,
    a0: f32,
    a1: f32,
    color: Color32,
    feather: f32,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let sweep = a1 - a0;
    if r1 - r0 < 0.25 || sweep <= 0.0 || color == Color32::TRANSPARENT {
        return mesh;
    }
    let h = (feather / 2.0).clamp(0.0, (r1 - r0) / 4.0);
    let closed = sweep >= TAU - 1e-3;
    let radii = [r0 - h, r0 + h, r1 - h, r1 + h];
    let steps = arc_segments(r1, sweep, ARC_TOLERANCE);
    let cols = if closed { steps + 1 } else { steps + 3 };

    for (row, &r) in radii.iter().enumerate() {
        let solid_row = row == 1 || row == 2;
        // the radial sides are `h` away from the edge whatever the radius: an angle of h / r
        let ang = (h / r.max(1.0)).min(sweep / 4.0);
        for col in 0..cols {
            let (a, solid_col) = if closed {
                (a0 + sweep * col as f32 / steps as f32, true)
            } else if col == 0 {
                (a0 - ang, false)
            } else if col == cols - 1 {
                (a1 + ang, false)
            } else {
                let k = (col - 1) as f32 / steps as f32;
                (a0 + ang + (sweep - 2.0 * ang) * k, true)
            };
            let p = c + Vec2::new(a.cos(), a.sin()) * r;
            let fill = if solid_row && solid_col {
                color
            } else {
                Color32::TRANSPARENT
            };
            mesh.colored_vertex(p, fill);
        }
    }
    for row in 0..3u32 {
        for col in 0..(cols as u32 - 1) {
            let i = row * cols as u32 + col;
            let below = i + cols as u32;
            mesh.add_triangle(i, i + 1, below);
            mesh.add_triangle(i + 1, below + 1, below);
        }
    }
    mesh
}

/// Donut slice with anti-aliased edges, see [`ring_slice_mesh`]. The feather is one physical
/// pixel of `painter`'s display, so it is crisp on both standard and Retina screens.
pub fn ring_slice(
    painter: &egui::Painter,
    c: Pos2,
    r0: f32,
    r1: f32,
    a0: f32,
    a1: f32,
    color: Color32,
) -> Shape {
    let feather = 1.0 / painter.ctx().pixels_per_point().max(1.0);
    let mesh = ring_slice_mesh(c, r0, r1, a0, a1, color, feather);
    if mesh.is_empty() {
        Shape::Noop
    } else {
        Shape::mesh(mesh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    fn dist(c: Pos2, v: &egui::epaint::Vertex) -> f32 {
        (v.pos - c).length()
    }

    // ------------------------------------------------------------ pie palette
    #[test]
    fn glass_is_always_smoked_whatever_the_appearance() {
        let g = glass_colors();
        let smoked = pie_colors_for(true);
        assert_eq!(g.slice, smoked.slice);
        assert_eq!(g.core, smoked.core);
        assert_ne!(g.slice, pie_colors_for(false).slice);
        // the name and the icon are light and read on the dark slice
        assert!(contrast(g.ink, rgb(g.slice)) >= 7.0);
    }

    // ------------------------------------------------------------ icons
    #[test]
    fn fit_within_keeps_aspect_and_never_enlarges() {
        assert_eq!(fit_within(64, 32, 128), (64, 32), "already fits");
        assert_eq!(fit_within(128, 128, 128), (128, 128));
        assert_eq!(fit_within(512, 256, 128), (128, 64));
        assert_eq!(fit_within(100, 1000, 100), (10, 100));
        assert_eq!(fit_within(1000, 1, 10), (10, 1), "never collapses to 0");
    }

    #[test]
    fn prepare_icon_downscales_and_keeps_opaque_color() {
        let src = image::RgbaImage::from_pixel(300, 150, image::Rgba([200, 30, 30, 255]));
        let out = prepare_icon(src, 128);
        assert_eq!((out.width(), out.height()), (128, 64));
        let mid = out.get_pixel(64, 32).0;
        assert_eq!(mid[3], 255);
        assert!((mid[0] as i32 - 200).abs() <= 2, "color survives: {mid:?}");
    }

    #[test]
    fn prepare_icon_does_not_bleed_color_from_transparent_pixels() {
        // left half opaque white, right half fully transparent but *red*: a naive resize would
        // pull red into the edge. Premultiplying first makes the transparent half contribute 0.
        let mut src = image::RgbaImage::new(256, 4);
        for (x, _, p) in src.enumerate_pixels_mut() {
            *p = if x < 128 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([255, 0, 0, 0])
            };
        }
        let out = prepare_icon(src, 64);
        for p in out.pixels() {
            let [r, g, b, a] = p.0;
            assert!(r <= a && g <= a && b <= a, "premultiplied invariant: {p:?}");
            if a > 0 {
                // premultiplied: g == b == a means pure white, no red tint
                assert!((g as i32 - b as i32).abs() <= 1, "no tint: {p:?}");
                assert!((g as i32 - a as i32).abs() <= 2, "white stays white: {p:?}");
            }
        }
    }

    #[test]
    fn prepare_icon_small_image_is_only_premultiplied() {
        let src = image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 100, 50, 128]));
        let out = prepare_icon(src, 128);
        assert_eq!((out.width(), out.height()), (8, 8));
        assert_eq!(out.get_pixel(0, 0).0, [100, 50, 25, 128]);
    }

    #[test]
    fn oversized_source_images_are_not_decoded() {
        let dir = std::env::temp_dir().join(format!("cursorpie-draw-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (big, ok) = (dir.join("big.png"), dir.join("ok.png"));
        image::RgbaImage::new(MAX_SOURCE_SIDE + 1, 1)
            .save(&big)
            .unwrap();
        image::RgbaImage::from_pixel(64, 48, image::Rgba([1, 2, 3, 255]))
            .save(&ok)
            .unwrap();
        assert!(
            decode_icon(&big).is_none(),
            "too wide: refused from the header"
        );
        assert_eq!(decode_icon(&ok).map(|i| i.dimensions()), Some((64, 48)));
        assert!(decode_icon(&dir.join("missing.png")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ------------------------------------------------------------ math
    #[test]
    fn step_linear_takes_a_fixed_time_and_stops_exactly() {
        let (mut v, mut moving) = (0.0, true);
        let mut frames = 0;
        while moving && frames < 1000 {
            (v, moving) = step_linear(v, 1.0, 1.0 / 60.0, 0.2);
            frames += 1;
        }
        assert_eq!(v, 1.0, "ends exactly on the target");
        assert!((11..=13).contains(&frames), "0.2 s at 60 fps: {frames}");
        let (v, moving) = step_linear(1.0, 1.0, 0.016, 0.2);
        assert!(v == 1.0 && !moving, "already there: not moving");
        let (v, moving) = step_linear(0.05, 0.0, 1.0, 0.2);
        assert!(v == 0.0 && !moving, "a long frame does not overshoot");
    }

    #[test]
    fn step_linear_is_frame_rate_independent() {
        let run = |dt: f32| {
            let (mut v, mut t) = (0.0f32, 0.0f32);
            while v < 1.0 {
                (v, _) = step_linear(v, 1.0, dt, 0.25);
                t += dt;
            }
            t
        };
        assert!((run(1.0 / 30.0) - run(1.0 / 144.0)).abs() < 0.04);
    }

    #[test]
    fn easing_curves_hit_their_endpoints() {
        assert!(ease_out_cubic(0.0).abs() < 1e-6 && (ease_out_cubic(1.0) - 1.0).abs() < 1e-6);
        assert_eq!(ease_out_cubic(2.0), 1.0, "clamped");
        assert!(ease_out_back(0.0).abs() < 1e-5 && (ease_out_back(1.0) - 1.0).abs() < 1e-5);
        assert!(ease_out_back(0.8) > 1.0, "overshoots before settling");
    }

    #[test]
    fn snapping_lands_on_the_pixel_grid() {
        assert_eq!(snap_to_pixel(10.4, 1.0), 10.0);
        assert_eq!(snap_to_pixel(10.26, 2.0), 10.5);
        assert_eq!(snap_to_pixel(10.3, 0.0), 10.3, "bad scale leaves it alone");
        let r = snap_rect(
            Rect::from_min_max(egui::pos2(0.3, 0.6), egui::pos2(99.4, 49.5)),
            1.0,
        );
        assert_eq!((r.min.x, r.min.y, r.max.x, r.max.y), (0.0, 1.0, 99.0, 50.0));
    }

    #[test]
    fn arc_segments_keep_the_curve_within_tolerance() {
        for r in [4.0f32, 40.0, 90.0, 180.0, 400.0] {
            let n = arc_segments(r, TAU, 0.02);
            let step = TAU / n as f32;
            let error = r * (1.0 - (step / 2.0).cos());
            assert!(error <= 0.02 + 1e-4, "r={r}: chord error {error}");
        }
        assert!(arc_segments(180.0, TAU, 0.02) > arc_segments(20.0, TAU, 0.02));
        assert!(arc_segments(10.0, 0.0, 0.02) >= 2, "never fewer than 2");
    }

    #[test]
    fn arc_points_start_and_end_on_the_circle() {
        let c = egui::pos2(10.0, -5.0);
        let pts = arc_points(c, 50.0, 0.0, PI);
        assert!(((pts[0] - c).length() - 50.0).abs() < 1e-3);
        assert!(((pts[pts.len() - 1] - c).length() - 50.0).abs() < 1e-3);
        assert!((pts[0].x - 60.0).abs() < 1e-3 && (pts[pts.len() - 1].x + 40.0).abs() < 1e-3);
    }

    #[test]
    fn outline_is_one_loop_around_the_slice() {
        let c = egui::pos2(0.0, 0.0);
        let pts = ring_slice_outline(c, 30.0, 60.0, 0.0, 1.0);
        let outer = arc_points(c, 60.0, 0.0, 1.0).len();
        let inner = arc_points(c, 30.0, 1.0, 0.0).len();
        assert_eq!(pts.len(), outer + inner);
        assert!(
            ((pts[outer] - c).length() - 30.0).abs() < 1e-3,
            "turns inward"
        );
    }

    // ------------------------------------------------------------ mesh
    #[test]
    fn slice_mesh_is_well_formed() {
        let color = Color32::from_rgba_unmultiplied(40, 80, 120, 200);
        let m = ring_slice_mesh(egui::pos2(100.0, 100.0), 30.0, 80.0, 0.2, 1.4, color, 1.0);
        assert!(!m.is_empty());
        assert_eq!(m.indices.len() % 3, 0);
        let n = m.vertices.len() as u32;
        assert!(m.indices.iter().all(|&i| i < n), "every index is valid");
    }

    #[test]
    fn slice_mesh_fades_out_beyond_every_edge() {
        let c = egui::pos2(0.0, 0.0);
        let color = Color32::from_rgb(200, 100, 50);
        let (r0, r1, a0, a1, f) = (30.0, 80.0, 0.2, 1.4, 1.0);
        let m = ring_slice_mesh(c, r0, r1, a0, a1, color, f);
        let steps = arc_segments(r1, a1 - a0, ARC_TOLERANCE);
        assert_eq!(
            m.vertices.len(),
            4 * (steps + 3),
            "4 rows, steps + 3 columns"
        );
        let solid = m.vertices.iter().filter(|v| v.color == color).count();
        assert_eq!(solid, 2 * (steps + 1), "only the core is opaque");
        assert!(
            m.vertices
                .iter()
                .all(|v| v.color == color || v.color == Color32::TRANSPARENT),
            "no other colors: the fade is made by the GPU"
        );
        let far = m.vertices.iter().map(|v| dist(c, v)).fold(0.0f32, f32::max);
        let near = m
            .vertices
            .iter()
            .map(|v| dist(c, v))
            .fold(f32::MAX, f32::min);
        assert!((far - (r1 + f / 2.0)).abs() < 1e-3, "outer fringe: {far}");
        assert!((near - (r0 - f / 2.0)).abs() < 1e-3, "inner fringe: {near}");
    }

    #[test]
    fn slice_mesh_covers_the_same_area_as_before() {
        // the fade is centered on the edge, so the opaque core is half a feather smaller on
        // each side and the whole slice has the size it always had
        let c = egui::pos2(0.0, 0.0);
        let color = Color32::WHITE;
        let m = ring_slice_mesh(c, 30.0, 80.0, 0.0, 1.0, color, 1.0);
        let core_r: Vec<f32> = m
            .vertices
            .iter()
            .filter(|v| v.color == color)
            .map(|v| dist(c, v))
            .collect();
        let max = core_r.iter().cloned().fold(0.0f32, f32::max);
        let min = core_r.iter().cloned().fold(f32::MAX, f32::min);
        assert!((max - 79.5).abs() < 1e-3 && (min - 30.5).abs() < 1e-3);
    }

    #[test]
    fn full_ring_has_no_seam() {
        let color = Color32::from_rgb(10, 20, 30);
        let m = ring_slice_mesh(egui::pos2(0.0, 0.0), 30.0, 80.0, 0.0, TAU, color, 1.0);
        let solid = m.vertices.iter().filter(|v| v.color == color).count();
        assert_eq!(
            solid * 2,
            m.vertices.len(),
            "two solid rows out of four, all columns"
        );
    }

    #[test]
    fn degenerate_slices_make_no_mesh() {
        let c = egui::pos2(0.0, 0.0);
        let w = Color32::WHITE;
        assert!(
            ring_slice_mesh(c, 50.0, 50.0, 0.0, 1.0, w, 1.0).is_empty(),
            "no thickness"
        );
        assert!(
            ring_slice_mesh(c, 30.0, 80.0, 1.0, 1.0, w, 1.0).is_empty(),
            "no sweep"
        );
        assert!(ring_slice_mesh(c, 30.0, 80.0, 0.0, 1.0, Color32::TRANSPARENT, 1.0).is_empty());
    }

    #[test]
    fn a_thin_slice_does_not_fold_over_itself() {
        // sweep much smaller than the feather's angle at the inner radius
        let m = ring_slice_mesh(
            egui::pos2(0.0, 0.0),
            5.0,
            12.0,
            0.0,
            0.05,
            Color32::WHITE,
            1.0,
        );
        assert!(!m.is_empty());
        assert!(
            m.vertices
                .iter()
                .all(|v| v.pos.x.is_finite() && v.pos.y.is_finite())
        );
    }
}
