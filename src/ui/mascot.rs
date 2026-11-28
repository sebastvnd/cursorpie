//! The mascot: the bubblegum octopus in all its poses, ready to put in any corner of the UI.
//!
//! Everything about the mascot lives here, so a screen that wants it needs one line and a pose:
//!
//! ```ignore
//! mascot::show(ui, Pose::Success, 56.0);                       // in the layout, like a label
//! mascot::paint(&painter, Pose::Idea, rect, Style::Full);      // at a place you chose
//! mascot::paint_faded(&painter, Pose::Welcome, rect, page, 0.18, Style::Watermark); // behind a page
//! ```
//!
//! | Piece | What it does |
//! |---|---|
//! | [`Pose`] | every picture of the octopus, named after the moment it fits (`Success`, `Offline`, ...) |
//! | [`Style`] | [`Style::Full`] is the artwork as drawn, [`Style::Watermark`] is the quiet gray ghost for a page background |
//! | [`show`] | a pose in the layout, `size` points wide, like a label or an icon |
//! | [`paint`], [`paint_faded`] | a pose at a rectangle you chose; the second fades toward the edges of `fade_bounds` |
//! | [`release`] | frees the textures (called when Settings closes and when the UI goes to sleep) |
//!
//! # Adding a pose
//!
//! 1. Put the SVG in `assets/picto/mascot/`, named in `snake_case` after the moment it fits
//!    (`low_battery.svg`, not `31_octopus_low_battery.svg`: the folder already says it is the octopus).
//!    The file is 512 x 512 (most have a `-24 -24 560 560` view box, so the octopus keeps a margin),
//!    drawn with plain shapes: `egui_extras` draws no text, filters or linked pictures.
//! 2. Add one line to the `poses!` list below: `LowBattery => "low_battery",`.
//!
//! The test `every_file_in_the_folder_is_a_pose_and_every_pose_has_its_file` fails when the two
//! disagree, so a forgotten line or a typo is caught by `cargo test`, not by a panic on screen.
//!
//! # How it behaves
//!
//! * Pictures are **made on first use** (the SVG is read and drawn by `egui_extras`), shrunk to a
//!   size that fits where they are drawn (128, 256 or 512 pixels) and kept in the egui context. egui
//!   scales textures without mipmaps, so a 512 px picture shown 40 points wide would be jagged: the
//!   same reason `settings::brand` shrinks its logos once.
//! * The cache is **bounded** ([`CAPACITY`], an entry is at most 1 MiB) and drops the entry used
//!   longest ago. [`release`] drops all of it; the pictures are made again when next needed.
//! * A picture that cannot be read is **never a panic**: `paint` returns `false` and `show` leaves
//!   an empty space, so the screen is the same without it.
//!
//! # Where it belongs
//!
//! One mascot per screen at the most, small, away from the text it would sit on, and only where
//! it says something: a welcome, a result, a problem, an empty page. Never in a list row, a
//! toolbar or the pie. See `docs/developer/MASCOT.md`.

use eframe::egui::{self, Color32, Pos2, Rect, TextureHandle, TextureOptions, Vec2};
use image::imageops::{self, FilterType};

/// Declares [`Pose`]: the variant, and the file in `assets/picto/mascot/` (without `.svg`) that
/// draws it. One line per pose is all a new picture needs.
macro_rules! poses {
    ($($variant:ident => $file:literal,)+) => {
        /// A picture of the octopus. Named after the moment it fits, not after what it draws.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Pose {
            $($variant,)+
        }

        impl Pose {
            /// Every pose, in the order of the list.
            pub const ALL: &'static [Pose] = &[$(Pose::$variant,)+];

            /// The name of the file of the pose, without `.svg`: also the name of the texture.
            pub const fn file(self) -> &'static str {
                match self {
                    $(Pose::$variant => $file,)+
                }
            }

            /// The SVG, compiled into the program.
            fn svg(self) -> &'static [u8] {
                match self {
                    $(Pose::$variant => &include_bytes!(concat!(
                        "../../assets/picto/mascot/",
                        $file,
                        ".svg"
                    ))[..],)+
                }
            }
        }
    };
}

poses! {
    // the app talking to the person
    Welcome => "welcome",
    GetStarted => "get_started",
    WhatsNew => "whats_new",
    Notification => "notification",
    Goodbye => "goodbye",
    Gift => "gift",
    // how it went
    Success => "success",
    Celebrate => "celebrate",
    Winner => "winner",
    MessageSent => "message_sent",
    Error => "error",
    Report => "report",
    Sad => "sad",
    Angry => "angry",
    Confused => "confused",
    Surprised => "surprised",
    Sick => "sick",
    // what it is doing
    Thinking => "thinking",
    Working => "working",
    Searching => "searching",
    Downloading => "downloading",
    PleaseWait => "please_wait",
    Offline => "offline",
    Secure => "secure",
    Settings => "settings",
    Idea => "idea",
    // moods and pastimes
    Base => "base",
    Cool => "cool",
    LoveIt => "love_it",
    Sleeping => "sleeping",
    CoffeeBreak => "coffee_break",
    Music => "music",
    Reading => "reading",
    Photo => "photo",
    Painting => "painting",
    Cooking => "cooking",
}

/// How a pose is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Style {
    /// The artwork in its own colors.
    Full,
    /// Almost gray and very light: a ghost for the background of a page that must stay quiet.
    Watermark,
}

/// How much color a [`Style::Watermark`] keeps (0 is gray, 1 is the artwork).
const WATERMARK_SATURATION: f32 = 0.08;
/// How opaque a [`Style::Watermark`] is.
const WATERMARK_OPACITY: f32 = 0.14;

/// Sizes (in pixels) a picture is made at. The smallest that is at least as wide as the place it is
/// drawn at is used, so a small mascot is never a shrunk big one and a big one is never blurry.
const TEXEL_SIZES: [u32; 3] = [128, 256, 512];

/// Most pictures kept in the context (the one on screen, and a few that came and went). The
/// biggest is 1 MiB, so this is a few MiB at the very most.
pub const CAPACITY: usize = 6;

/// A picture in the cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    pose: Pose,
    texels: u32,
    style: Style,
}

/// The pictures kept in the egui context, the one used longest ago first.
#[derive(Clone, Default)]
struct Cache(Vec<(Key, TextureHandle)>);

fn cache_id() -> egui::Id {
    egui::Id::new("cursorpie-mascot-cache")
}

/// The smallest of [`TEXEL_SIZES`] that has `wanted` pixels, or the biggest.
fn texel_size(wanted: f32) -> u32 {
    TEXEL_SIZES
        .into_iter()
        .find(|&t| t as f32 >= wanted)
        .unwrap_or(TEXEL_SIZES[TEXEL_SIZES.len() - 1])
}

/// Turns the artwork into the ghost of [`Style::Watermark`], in place.
fn watermark(image: &mut egui::ColorImage) {
    for pixel in &mut image.pixels {
        let [r, g, b, a] = pixel.to_srgba_unmultiplied();
        // pull the colors toward the gray of the same brightness
        let gray = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
        let faded = |channel: u8| {
            (gray + (channel as f32 - gray) * WATERMARK_SATURATION)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        *pixel = Color32::from_rgba_unmultiplied(
            faded(r),
            faded(g),
            faded(b),
            (a as f32 * WATERMARK_OPACITY).round() as u8,
        );
    }
}

/// Draws the SVG of `pose` and shrinks it to at most `texels` pixels wide (it is never scaled up).
/// None if the file cannot be read.
fn rasterize(pose: Pose, texels: u32, style: Style) -> Option<egui::ColorImage> {
    let mut pic = egui_extras::image::load_svg_bytes(pose.svg(), &Default::default()).ok()?;
    let [w, h] = pic.size;
    if w == 0 || h == 0 {
        return None;
    }
    let side = (texels as usize).clamp(1, w);
    if side < w {
        let new_h = ((h * side) as f32 / w as f32).round().max(1.0) as usize;
        // the pixels are premultiplied, which is what a filter must average: averaging straight
        // colors would leave a dark fringe around the soft edges
        let raw: Vec<u8> = pic.pixels.iter().flat_map(|p| p.to_array()).collect();
        let src = image::RgbaImage::from_raw(w as u32, h as u32, raw)?;
        // a triangle filter (no negative lobes, so no color brighter than its alpha) that looks at
        // every source pixel when it shrinks: it is the mipmap egui does not have
        let small = imageops::resize(&src, side as u32, new_h as u32, FilterType::Triangle);
        pic = egui::ColorImage::from_rgba_premultiplied([side, new_h], small.as_raw());
    }
    if style == Style::Watermark {
        watermark(&mut pic);
    }
    Some(pic)
}

/// The texture of `key`, made on first use and kept in the context.
fn texture(ctx: &egui::Context, key: Key) -> Option<TextureHandle> {
    let id = cache_id();
    // a hit moves to the end of the list: the one at the front is the one used longest ago
    let hit = ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_insert_with(id, Cache::default);
        let at = cache.0.iter().position(|(k, _)| *k == key)?;
        let entry = cache.0.remove(at);
        let tex = entry.1.clone();
        cache.0.push(entry);
        Some(tex)
    });
    if hit.is_some() {
        return hit;
    }
    // not inside `data_mut`: loading a texture takes the same lock
    let image = rasterize(key.pose, key.texels, key.style)?;
    let tex = ctx.load_texture(
        format!("mascot-{}-{}-{:?}", key.pose.file(), key.texels, key.style),
        image,
        TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| {
        let cache = d.get_temp_mut_or_insert_with(id, Cache::default);
        cache.0.retain(|(k, _)| *k != key);
        while cache.0.len() >= CAPACITY {
            drop(cache.0.remove(0));
        }
        cache.0.push((key, tex.clone()));
    });
    Some(tex)
}

/// The texture of `pose` for a place `side` points wide.
fn texture_for(ctx: &egui::Context, pose: Pose, side: f32, style: Style) -> Option<TextureHandle> {
    let texels = texel_size(side * ctx.pixels_per_point());
    texture(
        ctx,
        Key {
            pose,
            texels,
            style,
        },
    )
}

/// The largest square that fits in `rect`, centered (every pose is square).
fn square_in(rect: Rect) -> Rect {
    Rect::from_center_size(rect.center(), Vec2::splat(rect.width().min(rect.height())))
}

/// Draws `pose` as large as fits in `rect`, centered. False if the picture could not be made (the
/// caller may draw something else, or nothing).
pub fn paint(painter: &egui::Painter, pose: Pose, rect: Rect, style: Style) -> bool {
    let square = square_in(rect);
    let Some(tex) = texture_for(painter.ctx(), pose, square.width(), style) else {
        return false;
    };
    let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.image(tex.id(), square, whole, Color32::WHITE);
    true
}

/// Puts `pose` in the layout, `size` points wide and high, like a label or an icon. The space is
/// kept even when the picture cannot be made, so the layout does not depend on it.
pub fn show(ui: &mut egui::Ui, pose: Pose, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), pose, rect, Style::Full);
    }
    response
}

/// Draws `pose` in `rect` with a smooth fade toward the edges of `fade_bounds`: over the last
/// `fade_fraction` of its width and height the picture goes from opaque to clear. What lies
/// outside `fade_bounds` is not seen, so a picture bigger than the page can sit over its corner.
/// False if the picture could not be made.
pub fn paint_faded(
    painter: &egui::Painter,
    pose: Pose,
    rect: Rect,
    fade_bounds: Rect,
    fade_fraction: f32,
    style: Style,
) -> bool {
    const STEPS: usize = 16;

    let image_rect = square_in(rect);
    let Some(tex) = texture_for(painter.ctx(), pose, image_rect.width(), style) else {
        return false;
    };

    let fade_x = (fade_bounds.width() * fade_fraction).max(1.0);
    let fade_y = (fade_bounds.height() * fade_fraction).max(1.0);

    // smooth transition from transparent to opaque
    let smooth_fade = |distance: f32, width: f32| {
        let t = (distance / width).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };

    let mut mesh = egui::Mesh::with_texture(tex.id());
    mesh.reserve_vertices((STEPS + 1) * (STEPS + 1));
    mesh.reserve_triangles(STEPS * STEPS * 2);

    // one vertex per grid point, its alpha from how far it is from each edge of the bounds
    for y in 0..=STEPS {
        let v = y as f32 / STEPS as f32;
        for x in 0..=STEPS {
            let u = x as f32 / STEPS as f32;
            let pos = Pos2::new(
                image_rect.left() + image_rect.width() * u,
                image_rect.top() + image_rect.height() * v,
            );
            let dx = (pos.x - fade_bounds.left()).min(fade_bounds.right() - pos.x);
            let dy = (pos.y - fade_bounds.top()).min(fade_bounds.bottom() - pos.y);
            let alpha = smooth_fade(dx, fade_x) * smooth_fade(dy, fade_y);
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: Pos2::new(u, v),
                color: Color32::from_white_alpha((alpha * 255.0).round() as u8),
            });
        }
    }

    // two triangles per grid cell
    let stride = (STEPS + 1) as u32;
    for y in 0..STEPS {
        for x in 0..STEPS {
            let top_left = (y * (STEPS + 1) + x) as u32;
            let top_right = top_left + 1;
            let bottom_left = top_left + stride;
            let bottom_right = bottom_left + 1;
            mesh.add_triangle(top_left, top_right, bottom_left);
            mesh.add_triangle(top_right, bottom_right, bottom_left);
        }
    }

    painter.add(egui::Shape::mesh(mesh));
    true
}

/// Frees every picture. They are made again when they are next drawn, so nothing is lost: it is
/// called when Settings closes and when the UI goes to sleep (see `App::enter_sleep`).
pub fn release(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Cache>(cache_id()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_file_in_the_folder_is_a_pose_and_every_pose_has_its_file() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/picto/mascot");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("the mascot folder")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if path.extension()? != "svg" {
                    return None;
                }
                Some(path.file_stem()?.to_str()?.to_owned())
            })
            .collect();
        on_disk.sort();
        let mut listed: Vec<String> = Pose::ALL.iter().map(|p| p.file().to_owned()).collect();
        listed.sort();
        assert_eq!(
            on_disk, listed,
            "assets/picto/mascot/ and the `poses!` list of src/ui/mascot.rs must name the same files"
        );
    }

    #[test]
    fn names_are_unique_and_plain_snake_case() {
        let mut seen = HashSet::new();
        for pose in Pose::ALL {
            let name = pose.file();
            assert!(seen.insert(name), "{name} is listed twice");
            assert!(
                !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{name}: use lowercase words joined by `_`, no numbers in front, no `octopus`"
            );
            assert!(
                !name.starts_with(|c: char| c.is_ascii_digit()) && !name.contains("octopus"),
                "{name}"
            );
        }
    }

    #[test]
    fn every_pose_draws_something_at_every_size() {
        for &pose in Pose::ALL {
            for texels in [TEXEL_SIZES[0], TEXEL_SIZES[2]] {
                let img = rasterize(pose, texels, Style::Full)
                    .unwrap_or_else(|| panic!("{} cannot be drawn", pose.file()));
                let [w, h] = img.size;
                assert!(w <= texels as usize && h <= texels as usize, "{pose:?}");
                let drawn = img.pixels.iter().filter(|p| p.a() > 0).count();
                assert!(
                    drawn > w * h / 20,
                    "{pose:?} at {texels} has something in it"
                );
            }
        }
    }

    #[test]
    fn a_picture_is_never_scaled_up() {
        let img = rasterize(Pose::Welcome, 10_000, Style::Full).unwrap();
        assert_eq!(img.size[0], 512, "the files are 512 px wide");
    }

    #[test]
    fn the_watermark_is_light_and_nearly_gray() {
        let img = rasterize(Pose::Welcome, 128, Style::Watermark).unwrap();
        let max_alpha = (WATERMARK_OPACITY * 255.0).ceil() as u8;
        for p in &img.pixels {
            assert!(p.a() <= max_alpha, "alpha {} over {max_alpha}", p.a());
        }
    }

    #[test]
    fn the_smallest_size_that_is_sharp_is_chosen() {
        assert_eq!(texel_size(1.0), 128);
        assert_eq!(texel_size(128.0), 128);
        assert_eq!(texel_size(129.0), 256);
        assert_eq!(texel_size(512.0), 512);
        assert_eq!(texel_size(5000.0), 512);
    }

    #[test]
    fn the_cache_is_bounded_and_can_be_released() {
        let ctx = egui::Context::default();
        let id = cache_id();
        for &pose in Pose::ALL.iter().take(CAPACITY + 3) {
            let key = Key {
                pose,
                texels: 128,
                style: Style::Full,
            };
            assert!(texture(&ctx, key).is_some());
        }
        let kept = ctx
            .data(|d| d.get_temp::<Cache>(id))
            .map_or(0, |c| c.0.len());
        assert_eq!(kept, CAPACITY);
        // asking again for one that is there does not add a second copy
        let again = Key {
            pose: Pose::ALL[CAPACITY + 2],
            texels: 128,
            style: Style::Full,
        };
        assert!(texture(&ctx, again).is_some());
        let kept = ctx
            .data(|d| d.get_temp::<Cache>(id))
            .map_or(0, |c| c.0.len());
        assert_eq!(kept, CAPACITY);
        release(&ctx);
        assert!(ctx.data(|d| d.get_temp::<Cache>(id)).is_none());
    }
}
