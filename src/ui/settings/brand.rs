//! The CursorPie logo as the real artwork: the mark (a ring of four arcs around a diamond) and the
//! wordmark (the mark with the name written in the logo's own lettering), the same files the website
//! uses (`assets/cursorpie-mark.png`, `assets/cursorpie-logo-black.png`).
//!
//! Both files are black on transparent. They are turned into a white image that keeps the alpha, and
//! the painter tints it with the text color of the appearance: near black on the light panels, near
//! white on the dark ones, so the logo reads in both. Each is shrunk once to a few times the size it
//! is drawn at, because egui scales textures without mipmaps and a 1260 px image shown 120 px wide
//! would be jagged.

use eframe::egui::{self, Color32, Pos2, Rect, TextureHandle, TextureOptions};
use image::imageops::{self, FilterType};

static MARK_PNG: &[u8] = include_bytes!("../../../assets/cursorpie-mark.png");
static WORDMARK_PNG: &[u8] = include_bytes!("../../../assets/cursorpie-logo-black.png");

/// Which artwork.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Art {
    /// The ring and the diamond, square.
    Mark,
    /// The ring, the diamond and the name, about four times as wide as high.
    Wordmark,
}

impl Art {
    fn png(self) -> &'static [u8] {
        match self {
            Art::Mark => MARK_PNG,
            Art::Wordmark => WORDMARK_PNG,
        }
    }

    /// Pixels the stored texture is wide: a little over twice the widest it is drawn, so it is
    /// sharp on a screen that doubles the points.
    fn texels_wide(self) -> u32 {
        match self {
            Art::Mark => 256,
            Art::Wordmark => 640,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Art::Mark => "mark",
            Art::Wordmark => "wordmark",
        }
    }
}

/// The picture as a white image of the same alpha, at most `wide` pixels wide. None if `png` is not
/// a PNG.
pub fn silhouette(png: &[u8], wide: u32) -> Option<egui::ColorImage> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let new_w = wide.clamp(1, w);
    let new_h = ((h as f32 * new_w as f32 / w as f32).round() as u32).max(1);
    let mut img = imageops::resize(&img, new_w, new_h, FilterType::Lanczos3);
    // the colors of the file do not matter: the painter tints it
    for px in img.pixels_mut() {
        px.0 = [255, 255, 255, px.0[3]];
    }
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [new_w as usize, new_h as usize],
        img.as_raw(),
    ))
}

/// The texture of `art`, made on first use and kept in the context.
fn texture(ctx: &egui::Context, art: Art) -> Option<TextureHandle> {
    let id = egui::Id::new(("cursorpie-brand-texture", art.key()));
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(tex);
    }
    let image = silhouette(art.png(), art.texels_wide())?;
    let tex = ctx.load_texture(
        format!("brand-{}", art.key()),
        image,
        TextureOptions::LINEAR,
    );
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Draws `art` as large as fits in `rect`, centered, in `color`. False if the artwork could not be
/// loaded (the caller draws a stand-in).
pub fn paint(painter: &egui::Painter, art: Art, rect: Rect, color: Color32) -> bool {
    let Some(tex) = texture(painter.ctx(), art) else {
        return false;
    };
    let size = tex.size_vec2();
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let fit = Rect::from_center_size(rect.center(), size * scale);
    let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.image(tex.id(), fit, whole, color);
    true
}

static APP_ICON_PNG: &[u8] = include_bytes!("../../../assets/app-icon-256.png");

/// The texture of the application icon in its own colors (the rounded square of the dock, the
/// taskbar and the website), made on first use and kept in the context. Unlike the marks it is not
/// tinted.
fn app_icon_texture(ctx: &egui::Context) -> Option<TextureHandle> {
    let id = egui::Id::new("cursorpie-brand-texture-app-icon");
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return Some(tex);
    }
    let img = image::load_from_memory_with_format(APP_ICON_PNG, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    let tex = ctx.load_texture("brand-app-icon", image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    Some(tex)
}

/// Draws the application icon as large as fits in `rect`, centered. False if the picture could not
/// be loaded (the caller draws a stand-in).
pub fn paint_app_icon(painter: &egui::Painter, rect: Rect) -> bool {
    let Some(tex) = app_icon_texture(painter.ctx()) else {
        return false;
    };
    let size = tex.size_vec2();
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let fit = Rect::from_center_size(rect.center(), size * scale);
    let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    painter.image(tex.id(), fit, whole, Color32::WHITE);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_artwork_is_a_white_silhouette_of_the_right_shape() {
        for art in [Art::Mark, Art::Wordmark] {
            let img = silhouette(art.png(), art.texels_wide()).expect("the file is a PNG");
            let [w, h] = img.size;
            assert!(w <= art.texels_wide() as usize, "{art:?}");
            match art {
                Art::Mark => assert!((w as f32 / h as f32 - 1.0).abs() < 0.05, "square"),
                Art::Wordmark => assert!(w as f32 / h as f32 > 3.5, "wide"),
            }
            let drawn = img.pixels.iter().filter(|p| p.a() > 0).count();
            assert!(drawn > w * h / 40, "{art:?} has something in it");
            assert!(
                img.pixels
                    .iter()
                    .filter(|p| p.a() == 255)
                    .all(|p| (p.r(), p.g(), p.b()) == (255, 255, 255)),
                "opaque pixels are white, the painter tints them"
            );
        }
    }

    #[test]
    fn the_app_icon_is_a_square_picture_with_color() {
        let img = image::load_from_memory_with_format(APP_ICON_PNG, image::ImageFormat::Png)
            .expect("the file is a PNG");
        assert_eq!(img.width(), img.height());
    }

    #[test]
    fn a_picture_is_never_scaled_up_and_garbage_is_not_a_picture() {
        let img = silhouette(MARK_PNG, 10_000).unwrap();
        assert_eq!(img.size[0], 308, "the file is 308 px wide");
        assert!(silhouette(b"not a png", 256).is_none());
        assert!(silhouette(&[], 256).is_none());
    }
}
