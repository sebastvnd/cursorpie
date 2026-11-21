//! Icons of websites: fetched from the site, then **made to match** the other icons of the pie.
//!
//! What a site offers as its icon is a lottery: a 16 px `favicon.ico`, a 180 px `apple-touch-icon`
//! with a white square behind the logo, a logo that touches the border, another with a wide margin.
//! Put side by side in a pie they look uneven. This module has two halves:
//!
//! * [`fetch`] finds the best icon of a site. It reads the home page for `<link rel="icon">` and
//!   `apple-touch-icon` tags, tries the largest first, then `/favicon.ico`, and only when the site
//!   gives nothing usable asks Google's favicon service (which is told the host name and nothing
//!   else). The requests are made by the system's `curl` through [`http`], like the update check:
//!   CursorPie still has no HTTP library. It blocks, so it runs on a worker thread.
//! * [`harmonize`] gives the picture one look: remove a plain background, trim the empty border,
//!   one colour (or all white / all black), the same margin, and optionally the same plate (a
//!   rounded square) behind it. It is pure: the same picture and [`Style`] always give the same
//!   [`OUT_PX`] square, so the Settings sheet can redraw the preview while a slider moves.
//!
//! The [`Style`] the user chose is remembered in `web-icon-style.json` so the next site gets the
//! same look.

use crate::platform::http::{self, HttpError};
use image::imageops::{self, FilterType};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Side of the square that [`harmonize`] makes, in pixels (twice the texture size of a pie icon).
pub const OUT_PX: u32 = 256;
/// Only this much of the home page is read: the `<link>` tags are in its head.
const PAGE_MAX: u64 = 400 * 1024;
/// An icon file bigger than this is refused.
const ICON_MAX: u64 = 4 * 1024 * 1024;
/// Seconds one request may take.
const TIMEOUT_S: u64 = 8;
/// Most icon addresses tried for one site.
const MAX_TRIES: usize = 8;
/// The whole search stops after this long.
const BUDGET: Duration = Duration::from_secs(25);
/// An icon with a side of at least this many pixels ends the search at once.
const GOOD_PX: u32 = 128;

/// The colour of the icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tint {
    /// The colours of the site.
    #[default]
    Original,
    /// One colour: white.
    White,
    /// One colour: black.
    Black,
}

/// What sits behind the icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Plate {
    /// Nothing: the icon floats.
    #[default]
    None,
    /// A light rounded square.
    Light,
    /// A dark rounded square.
    Dark,
}

/// The look given to every website icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    /// Cut away the empty (or plain) border around the logo.
    pub trim: bool,
    /// A picture that is a full square with no transparent area (a coloured tile, like the icon of
    /// claude.ai) gets rounded corners, like the other icons, and no plate behind it.
    pub round_square: bool,
    /// Make a plain background transparent. Not remembered: it depends on the picture.
    #[serde(skip)]
    pub knock_out: bool,
    /// The colour of the logo.
    pub tint: Tint,
    /// What sits behind the logo.
    pub plate: Plate,
    /// Margin around the logo, in percent of the side (0 to 30).
    pub padding: u8,
    /// Roundness of the plate, in percent of the side (0 to 50; 50 is a circle).
    pub radius: u8,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            trim: true,
            round_square: true,
            knock_out: false,
            tint: Tint::Original,
            plate: Plate::None,
            padding: 10,
            radius: 22,
        }
    }
}

impl Style {
    /// The same style with every number inside its range (a hand-edited file may hold anything).
    pub fn clamped(mut self) -> Self {
        self.padding = self.padding.min(30);
        self.radius = self.radius.min(50);
        self
    }
}

/// Where the remembered style is kept.
fn style_path() -> PathBuf {
    crate::config::app_support_dir().join("web-icon-style.json")
}

/// The style used last time, or the default one.
pub fn load_style() -> Style {
    std::fs::read_to_string(style_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Style>(&t).ok())
        .map_or_else(Style::default, Style::clamped)
}

/// Remembers `style` for the next website icon. A failure is not worth a message.
pub fn save_style(style: &Style) {
    if let Ok(text) = serde_json::to_string_pretty(style) {
        let _ = std::fs::write(style_path(), text);
    }
}

// ------------------------------------------------------------------------------------ the site

/// A site, from what the user typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    /// `https://host[:port]`, no path.
    pub origin: String,
    /// The host name only, lower case.
    pub host: String,
}

/// Reads `github.com`, `www.github.com/x?y` or `https://github.com/` as a site. Only web addresses
/// are accepted (and only over `https`: a typed `http://` is upgraded).
///
/// # Errors
///
/// A sentence for the user when the text is empty, is not a web address, or is not a host name.
pub fn parse_site(input: &str) -> Result<Site, String> {
    let t = input.trim();
    if t.is_empty() {
        return Err("Type the address of a website first.".into());
    }
    if t.chars().any(char::is_whitespace) {
        return Err("A web address has no spaces in it.".into());
    }
    let lower = t.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        r
    } else if let Some(r) = lower.strip_prefix("http://") {
        r
    } else if lower.contains("://") {
        return Err("Only web addresses (https://\u{2026}) can be used.".into());
    } else {
        lower.as_str()
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    let ok = !host.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if !ok {
        return Err(format!(
            "\u{201c}{t}\u{201d} does not look like a website address (try example.com)."
        ));
    }
    Ok(Site {
        origin: format!("https://{authority}"),
        host: host.to_string(),
    })
}

/// The value of the attribute `name` in the text of one tag, if it has one.
fn attr(tag: &str, name: &str) -> Option<String> {
    // ASCII lower-casing keeps every byte offset, so the index found in it fits `tag`
    let lower = tag.to_ascii_lowercase();
    let needle = format!("{name}=");
    let mut from = 0;
    while let Some(i) = lower[from..].find(&needle) {
        let at = from + i;
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let start = at + needle.len();
        if before_ok {
            let rest = &tag[start..];
            let value = match rest.chars().next() {
                Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or("").to_string(),
                Some(_) => rest
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or("")
                    .to_string(),
                None => String::new(),
            };
            return Some(value);
        }
        from = start;
    }
    None
}

/// The largest side a `sizes` attribute names (`"16x16 32x32"` is 32; `"any"` is 0).
fn largest_size(sizes: &str) -> u32 {
    sizes
        .split_whitespace()
        .filter_map(|s| {
            let (w, h) = s
                .to_ascii_lowercase()
                .split_once('x')
                .map(|(a, b)| (a.parse::<u32>().unwrap_or(0), b.parse::<u32>().unwrap_or(0)))?;
            Some(w.max(h))
        })
        .max()
        .unwrap_or(0)
}

/// An absolute `https` address for a `href` found on the home page of `origin`.
fn resolve(origin: &str, href: &str) -> Option<String> {
    let h = href.trim();
    if h.is_empty() || h.starts_with("data:") || h.starts_with("javascript:") {
        return None;
    }
    let lower = h.to_ascii_lowercase();
    Some(if lower.starts_with("https://") {
        h.to_string()
    } else if lower.starts_with("http://") {
        format!("https://{}", &h[7..])
    } else if let Some(r) = h.strip_prefix("//") {
        format!("https://{r}")
    } else if h.starts_with('/') {
        format!("{origin}{h}")
    } else {
        format!("{origin}/{}", h.trim_start_matches("./"))
    })
}

/// The icons a home page names, best first: an `apple-touch-icon` before a plain icon, bigger
/// before smaller. SVG icons are left out (they cannot be drawn), and so are `mask-icon`s.
pub fn link_candidates(html: &str, origin: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut found: Vec<(u32, String)> = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<link") {
        let start = from + i;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        let tag = &html[start..end];
        from = end.max(start + 5);
        let Some(rel) = attr(tag, "rel") else {
            continue;
        };
        let rel = rel.to_ascii_lowercase();
        let tokens: Vec<&str> = rel.split_whitespace().collect();
        let touch = tokens.iter().any(|t| t.starts_with("apple-touch-icon"));
        let plain = tokens.contains(&"icon");
        if !(touch || plain) {
            continue;
        }
        let Some(href) = attr(tag, "href") else {
            continue;
        };
        let kind = attr(tag, "type").unwrap_or_default().to_ascii_lowercase();
        let path = href
            .split(['?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if kind.contains("svg") || path.ends_with(".svg") {
            continue;
        }
        let Some(url) = resolve(origin, &href) else {
            continue;
        };
        let size = attr(tag, "sizes").map_or(0, |s| largest_size(&s));
        let score = if touch {
            1000 + if size == 0 { 180 } else { size }
        } else {
            size
        };
        found.push((score, url));
    }
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    let mut out: Vec<String> = Vec::new();
    for (_, url) in found {
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// Every address to try for `site`, best first: what the home page names, the usual file names,
/// and, last, Google's favicon service.
fn plan(site: &Site, html: Option<&str>) -> Vec<String> {
    let mut urls: Vec<String> = html
        .map(|h| link_candidates(h, &site.origin))
        .unwrap_or_default();
    urls.push(format!("{}/apple-touch-icon.png", site.origin));
    urls.push(format!("{}/favicon.ico", site.origin));
    urls.push(format!(
        "https://www.google.com/s2/favicons?domain={}&sz=256",
        site.host
    ));
    let mut out: Vec<String> = Vec::new();
    for u in urls {
        if !out.contains(&u) {
            out.push(u);
        }
    }
    out.truncate(MAX_TRIES);
    out
}

/// A free name for a temporary file.
fn temp_file() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("cursorpie-web-{}-{n}.tmp", std::process::id()))
}

/// Downloads `url` to a temporary file and hands back its bytes (the file is deleted).
fn get_bytes(url: &str, max: u64) -> Result<Vec<u8>, HttpError> {
    let tmp = temp_file();
    let got = http::download(url, &tmp, TIMEOUT_S, max);
    let bytes = got.and_then(|()| std::fs::read(&tmp).map_err(|_| HttpError::Failed));
    let _ = std::fs::remove_file(&tmp);
    bytes
}

/// A picture that was found.
pub struct Fetched {
    /// The picture as the site serves it.
    pub image: RgbaImage,
    /// Where it came from.
    pub source: String,
    /// The host name, for the file name.
    pub host: String,
}

/// What the worker is doing now, in words, for the loading view.
pub type Stage = Arc<Mutex<String>>;

fn say(stage: &Stage, text: impl Into<String>) {
    if let Ok(mut s) = stage.lock() {
        *s = text.into();
    }
}

const NO_CLIENT: &str = "CursorPie fetches website icons with curl (or wget), and neither is \
installed. Install curl and try again, or choose an image file instead.";

/// Finds the best icon of the website `input` names. Blocks for up to about half a minute, so call
/// it from a worker thread. `stage` is told what is being done; `cancel` ends the search early.
///
/// # Errors
///
/// A sentence for the user: a wrong address, no `curl`, no network, or a site without an icon.
pub fn fetch(input: &str, stage: &Stage, cancel: &AtomicBool) -> Result<Fetched, String> {
    let site = parse_site(input)?;
    let started = Instant::now();
    say(stage, format!("Reading {}", site.host));
    let html = match http::get_head(&format!("{}/", site.origin), TIMEOUT_S, PAGE_MAX) {
        Ok(b) => Some(String::from_utf8_lossy(&b).into_owned()),
        Err(HttpError::NoClient) => return Err(NO_CLIENT.into()),
        Err(_) => None,
    };
    let urls = plan(&site, html.as_deref());
    let total = urls.len();
    let mut best: Option<(RgbaImage, String)> = None;
    for (n, url) in urls.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled.".into());
        }
        if started.elapsed() > BUDGET {
            break;
        }
        say(
            stage,
            format!("Downloading the icon ({} of {total})", n + 1),
        );
        let bytes = match get_bytes(&url, ICON_MAX) {
            Ok(b) => b,
            Err(HttpError::NoClient) => return Err(NO_CLIENT.into()),
            Err(_) => continue,
        };
        say(stage, "Checking the picture");
        let Ok(img) = image::load_from_memory(&bytes) else {
            continue;
        };
        let img = img.to_rgba8();
        let side = img.width().max(img.height());
        if side < 8 {
            continue;
        }
        let better = best
            .as_ref()
            .is_none_or(|(b, _)| side > b.width().max(b.height()));
        if better {
            best = Some((img, url));
        }
        if best
            .as_ref()
            .is_some_and(|(b, _)| b.width().max(b.height()) >= GOOD_PX)
        {
            break;
        }
    }
    match best {
        Some((image, source)) => Ok(Fetched {
            image,
            source,
            host: site.host,
        }),
        None => Err(format!(
            "No icon could be fetched from {}. Check the address and your connection, or choose \
             an image file instead.",
            site.host
        )),
    }
}

// ------------------------------------------------------------------------------------ the look

/// The largest difference between one channel of `p` and the colour `bg`.
fn dist(p: &Rgba<u8>, bg: [u8; 3]) -> u32 {
    (0..3)
        .map(|i| u32::from(p[i].abs_diff(bg[i])))
        .max()
        .unwrap_or(0)
}

/// The colour of a plain background: the four corners are opaque and (nearly) the same colour.
pub fn plain_background(img: &RgbaImage) -> Option<[u8; 3]> {
    let (w, h) = img.dimensions();
    if w < 2 || h < 2 {
        return None;
    }
    let corners = [
        *img.get_pixel(0, 0),
        *img.get_pixel(w - 1, 0),
        *img.get_pixel(0, h - 1),
        *img.get_pixel(w - 1, h - 1),
    ];
    if corners.iter().any(|p| p[3] < 250) {
        return None;
    }
    let bg = [corners[0][0], corners[0][1], corners[0][2]];
    corners.iter().all(|p| dist(p, bg) <= 24).then_some(bg)
}

/// Whether the picture sits on a plain **white or black** (or near) square, the usual favicon
/// background: the pictures the sheet turns "Remove plain background" on for. A background of
/// another colour is a tile that belongs to the logo, so it is kept unless the user removes it.
pub fn has_neutral_background(img: &RgbaImage) -> bool {
    plain_background(img)
        .is_some_and(|bg| bg.iter().all(|&c| c >= 232) || bg.iter().all(|&c| c <= 40))
}

/// Whether no pixel is transparent at all.
pub fn is_opaque(img: &RgbaImage) -> bool {
    img.pixels().all(|p| p[3] == 255)
}

/// How far a pixel may be from the background colour and still count as background.
const BG_TOLERANCE: u32 = 24;

/// Whether the picture is a filled rectangle, like the tile of an app icon: nearly every pixel is
/// opaque. A few transparent or half transparent pixels (a corner the site already rounded a hair,
/// an anti-aliased edge) do not matter, which is why this is not [`is_opaque`]: sites serve such
/// tiles with a pixel or two missing, and a strict test took them for logos. A picture that is
/// already clearly rounded, and a round or free-form logo, cover less of the square and are not
/// tiles.
pub fn fills_its_square(img: &RgbaImage) -> bool {
    let total = u64::from(img.width()) * u64::from(img.height());
    let filled = img.pixels().filter(|p| p[3] >= 200).count() as u64;
    total > 0 && filled * 100 >= total * 97
}

/// Makes the plain background transparent, with a soft edge, and removes its tint from the pixels
/// on the edge so no halo is left around the logo.
///
/// Only the background that **reaches the border** of the picture goes (it is found by flooding in
/// from the edges). A part of the logo that has the same colour as the background, like the black
/// cat inside GitHub's white circle on a black square, is enclosed by the logo and stays.
fn knock_out(img: &mut RgbaImage) {
    let Some(bg) = plain_background(img) else {
        return;
    };
    let (w, h) = img.dimensions();
    let at = |x: u32, y: u32| (y * w + x) as usize;
    let mut gone = vec![false; (w * h) as usize];
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        if gone[at(x, y)] || dist(img.get_pixel(x, y), bg) > BG_TOLERANCE {
            continue;
        }
        gone[at(x, y)] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    for y in 0..h {
        for x in 0..w {
            if gone[at(x, y)] {
                img.get_pixel_mut(x, y)[3] = 0;
                continue;
            }
            let beside = (x > 0 && gone[at(x - 1, y)])
                || (x + 1 < w && gone[at(x + 1, y)])
                || (y > 0 && gone[at(x, y - 1)])
                || (y + 1 < h && gone[at(x, y + 1)]);
            if !beside {
                continue;
            }
            // the pixel on the edge of the logo: partly background, so partly transparent
            let p = img.get_pixel_mut(x, y);
            let d = dist(p, bg);
            if d >= 60 {
                continue;
            }
            let alpha = (d - BG_TOLERANCE) * 255 / (60 - BG_TOLERANCE);
            if alpha < u32::from(p[3]) {
                p[3] = alpha as u8;
            }
            if alpha > 0 {
                for i in 0..3 {
                    let (b, v) = (f32::from(bg[i]), f32::from(p[i]));
                    p[i] = (b + (v - b) * 255.0 / alpha as f32).clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// Cuts away the transparent border around the logo. A plain background that was **not** removed
/// is part of the picture (the orange tile of an app icon) and is never cut.
fn trim(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p[3] > 12 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x1 < x0 || y1 < y0 {
        return img.clone();
    }
    imageops::crop_imm(img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

/// All of the picture in one colour, keeping its shape (its alpha).
fn tint(img: &mut RgbaImage, t: Tint) {
    let c = match t {
        Tint::Original => return,
        Tint::White => 255,
        Tint::Black => 0,
    };
    for p in img.pixels_mut() {
        p[0] = c;
        p[1] = c;
        p[2] = c;
    }
}

fn premultiply(img: &mut RgbaImage) {
    for p in img.pixels_mut() {
        let a = u32::from(p[3]);
        for i in 0..3 {
            p[i] = ((u32::from(p[i]) * a + 127) / 255) as u8;
        }
    }
}

fn unpremultiply(img: &mut RgbaImage) {
    for p in img.pixels_mut() {
        let a = u32::from(p[3]);
        if a == 0 {
            *p = Rgba([0, 0, 0, 0]);
        } else if a < 255 {
            for i in 0..3 {
                p[i] = ((u32::from(p[i]) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// How much of the pixel at (`x`, `y`) lies inside a `w` by `h` rectangle with corners of radius
/// `r`: 0 to 1, with a soft edge.
fn rect_cover(x: u32, y: u32, w: f32, h: f32, r: f32) -> f32 {
    let r = r.min(w.min(h) / 2.0);
    let qx = (x as f32 + 0.5 - w / 2.0).abs() - (w / 2.0 - r);
    let qy = (y as f32 + 0.5 - h / 2.0).abs() - (h / 2.0 - r);
    let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// Rounds the corners of the whole picture, with a soft edge.
fn round_corners(img: &mut RgbaImage, r: f32) {
    let (w, h) = (img.width() as f32, img.height() as f32);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let cover = rect_cover(x, y, w, h, r);
        p[3] = (f32::from(p[3]) * cover).round() as u8;
    }
}

/// Gives `src` the look of `style`: an [`OUT_PX`] square, the same for every site.
pub fn harmonize(src: &RgbaImage, style: &Style) -> RgbaImage {
    let style = style.clamped();
    let mut work = src.clone();
    if style.knock_out {
        knock_out(&mut work);
    }
    if style.trim {
        work = trim(&work);
    }
    tint(&mut work, style.tint);
    // a full square tile is rounded itself and needs no plate
    let tile = style.round_square && fills_its_square(&work);

    let margin = (OUT_PX as f32 * f32::from(style.padding) / 100.0).round() as u32;
    let room = OUT_PX.saturating_sub(2 * margin).max(8);
    let (w, h) = work.dimensions();
    let scale = room as f32 / w.max(h).max(1) as f32;
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    premultiply(&mut work);
    let mut logo = imageops::resize(&work, nw, nh, FilterType::Lanczos3);
    unpremultiply(&mut logo);
    if tile {
        let r = nw.min(nh) as f32 * f32::from(style.radius) / 100.0;
        round_corners(&mut logo, r);
    }

    let mut canvas = RgbaImage::from_pixel(OUT_PX, OUT_PX, Rgba([0, 0, 0, 0]));
    let fill = match style.plate {
        _ if tile => None,
        Plate::None => None,
        Plate::Light => Some([244u8, 244, 246]),
        Plate::Dark => Some([36u8, 36, 40]),
    };
    if let Some(c) = fill {
        let r = OUT_PX as f32 * f32::from(style.radius) / 100.0;
        for (x, y, p) in canvas.enumerate_pixels_mut() {
            let cover = rect_cover(x, y, OUT_PX as f32, OUT_PX as f32, r);
            *p = Rgba([c[0], c[1], c[2], (cover * 255.0).round() as u8]);
        }
    }
    let ox = i64::from((OUT_PX - nw.min(OUT_PX)) / 2);
    let oy = i64::from((OUT_PX - nh.min(OUT_PX)) / 2);
    imageops::overlay(&mut canvas, &logo, ox, oy);
    canvas
}

/// Saves `img` as a PNG in `dir`, named after `host`, never replacing a file that is there.
///
/// # Errors
///
/// A sentence for the user when the file cannot be written.
pub fn save_png(img: &RgbaImage, dir: &Path, host: &str) -> Result<PathBuf, String> {
    let safe: String = host
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let mut path = dir.join(format!("web-{safe}.png"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("web-{safe}-{n}.png"));
        n += 1;
    }
    img.save(&path)
        .map_err(|e| format!("Could not save the icon ({e})."))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_read_from_what_people_type() {
        let s = parse_site("  GitHub.com/x?y=1 ").unwrap();
        assert_eq!(s.origin, "https://github.com");
        assert_eq!(s.host, "github.com");
        let s = parse_site("http://www.example.org:8080/a").unwrap();
        assert_eq!(s.origin, "https://www.example.org:8080");
        assert_eq!(s.host, "www.example.org");
        assert!(parse_site("").is_err());
        assert!(parse_site("ftp://a.b").is_err());
        assert!(parse_site("a b.com").is_err());
        assert!(parse_site("localhost").is_err());
    }

    #[test]
    fn home_page_icons_are_ranked() {
        let html = r#"<head>
            <link rel="icon" href="/small.png" sizes="16x16">
            <link rel="shortcut icon" href="favicon.ico">
            <link rel="apple-touch-icon" sizes="152x152" href="//cdn.x.io/a.png">
            <link rel="icon" type="image/svg+xml" href="/logo.svg">
            <link rel="stylesheet" href="/x.css">
            <LINK REL='icon' HREF='/big.png' SIZES='192x192'>
        </head>"#;
        let got = link_candidates(html, "https://x.io");
        assert_eq!(
            got,
            vec![
                "https://cdn.x.io/a.png",
                "https://x.io/big.png",
                "https://x.io/small.png",
                "https://x.io/favicon.ico",
            ]
        );
    }

    fn logo_on_white() -> RgbaImage {
        let mut img = RgbaImage::from_pixel(40, 40, Rgba([255, 255, 255, 255]));
        for y in 10..30 {
            for x in 10..30 {
                img.put_pixel(x, y, Rgba([200, 30, 30, 255]));
            }
        }
        img
    }

    #[test]
    fn a_white_square_is_removed_and_the_logo_fills_the_icon() {
        let img = logo_on_white();
        assert!(has_neutral_background(&img));
        let style = Style {
            knock_out: true,
            ..Style::default()
        };
        let out = harmonize(&img, &style);
        assert_eq!(out.dimensions(), (OUT_PX, OUT_PX));
        assert_eq!(out.get_pixel(0, 0)[3], 0, "the margin is transparent");
        assert_eq!(out.get_pixel(3, OUT_PX / 2)[3], 0);
        let mid = out.get_pixel(OUT_PX / 2, OUT_PX / 2);
        assert!(mid[3] == 255 && mid[0] > 150 && mid[1] < 80);
        // the 20 px logo was trimmed out of 40 px of white and now fills the room inside the margin
        assert_eq!(out.get_pixel(40, OUT_PX / 2)[3], 255);
    }

    #[test]
    fn only_the_background_that_reaches_the_border_is_removed() {
        // white ring on black, with black inside it (GitHub's icon is like this)
        let mut img = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 255]));
        for y in 8..32 {
            for x in 8..32 {
                let inside = (14..26).contains(&x) && (14..26).contains(&y);
                if !inside {
                    img.put_pixel(x, y, Rgba([255, 255, 255, 255]));
                }
            }
        }
        assert!(has_neutral_background(&img));
        let style = Style {
            knock_out: true,
            ..Style::default()
        };
        let out = harmonize(&img, &style);
        assert_eq!(out.get_pixel(0, 0)[3], 0, "the black square is gone");
        let mid = out.get_pixel(OUT_PX / 2, OUT_PX / 2);
        assert!(
            mid[3] == 255 && mid[0] < 10,
            "the black inside the ring stays"
        );
    }

    #[test]
    fn a_square_tile_gets_rounded_corners_and_no_plate() {
        // an app icon: full-bleed orange square, nothing transparent
        let tile = RgbaImage::from_pixel(40, 40, Rgba([220, 110, 70, 255]));
        assert!(!has_neutral_background(&tile));
        let style = Style {
            plate: Plate::Dark,
            ..Style::default()
        };
        let out = harmonize(&tile, &style);
        assert!(
            out.get_pixel(28, 28)[3] < 20,
            "the corner of the tile is cut round"
        );
        let top = out.get_pixel(OUT_PX / 2, 40);
        assert!(top[3] == 255 && top[0].abs_diff(220) <= 2 && top[2].abs_diff(70) <= 2);
        assert_eq!(out.get_pixel(4, OUT_PX / 2)[3], 0, "no plate rim around it");
        // a tile that the site served with a corner pixel or two missing is still a tile
        let mut nicked = tile.clone();
        nicked.put_pixel(0, 0, Rgba([255, 255, 255, 0]));
        nicked.put_pixel(39, 39, Rgba([255, 255, 255, 90]));
        assert!(fills_its_square(&nicked) && !is_opaque(&nicked));
        let out = harmonize(&nicked, &Style::default());
        assert!(out.get_pixel(28, 28)[3] < 20);
        // a round logo is not
        let mut disc = RgbaImage::from_pixel(40, 40, Rgba([0, 0, 0, 0]));
        for y in 0..40u32 {
            for x in 0..40u32 {
                if (x as i32 - 20).pow(2) + (y as i32 - 20).pow(2) < 400 {
                    disc.put_pixel(x, y, Rgba([200, 30, 30, 255]));
                }
            }
        }
        assert!(!fills_its_square(&disc));
        let square = harmonize(
            &tile,
            &Style {
                round_square: false,
                ..Style::default()
            },
        );
        assert_eq!(square.get_pixel(30, 30)[3], 255, "kept square when asked");
    }

    #[test]
    fn one_colour_and_a_plate() {
        let style = Style {
            knock_out: true,
            round_square: false,
            tint: Tint::White,
            plate: Plate::Dark,
            radius: 50,
            ..Style::default()
        };
        let out = harmonize(&logo_on_white(), &style);
        assert_eq!(
            out.get_pixel(0, 0)[3],
            0,
            "a circle leaves the corner empty"
        );
        let mid = out.get_pixel(OUT_PX / 2, OUT_PX / 2);
        assert_eq!(*mid, Rgba([255, 255, 255, 255]));
        let ring = out.get_pixel(OUT_PX / 2, 4);
        assert_eq!(ring[3], 255);
        assert!(ring[0] < 60, "the plate is dark");
    }

    #[test]
    fn style_is_clamped_and_read_with_missing_fields() {
        let s: Style = serde_json::from_str(r#"{"padding": 99, "tint": "Black"}"#).unwrap();
        let s = s.clamped();
        assert_eq!((s.padding, s.tint, s.trim), (30, Tint::Black, true));
    }
}
