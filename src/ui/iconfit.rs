//! Makes an image the user chose (or dropped) fit to be a slice icon, and tells them what happened.
//!
//! An icon is drawn at 30 to 120 points, and the texture made from it is at most
//! [`ICON_MAX_PX`](crate::ui::draw::ICON_MAX_PX) pixels wide. A 12 megapixel photo or a 5 MB PNG is
//! therefore wasted disk space, a slow decode every time the cache reloads it, and a memory spike
//! while it is decoded. Instead of refusing such files, [`fit`] shrinks them **once**, when they are
//! chosen:
//!
//! * a small PNG, JPEG or WebP (at most [`KEEP_BYTES`] and [`KEEP_SIDE`] px) is used as it is;
//! * a bigger one is scaled down to [`STORE_PX`] px (twice the texture size, so it stays sharp on a
//!   Retina display) and saved as a PNG in the icons folder. The original is never touched;
//! * any other format the decoders know (GIF, BMP, ICO, TIFF, TGA, QOI, PNM, see
//!   `platform::IMAGE_EXTENSIONS`) is converted to a PNG in the icons
//!   folder as well, because profile sharing only carries PNG, JPEG and WebP;
//! * a file that cannot be used is refused with a sentence that says **why** (it is a folder, an
//!   SVG, a HEIC photo, damaged, too large, ...). The sentence is shown in a pop-up sheet.
//!
//! There are two [`Mode`]s. A file picked with the chooser is used where it is ([`Mode::InPlace`]); a
//! file dropped on the window is always copied into the icons folder ([`Mode::Library`]), so it
//! shows up under "Mine" and survives the original being moved or deleted.
//!
//! The work runs on a worker thread, never on the UI thread. It uses the `image` crate that is
//! already a dependency. The core ([`fit_into`]) takes the directory as a parameter and is unit
//! tested with a scratch directory.

use crate::config::{icons_dir, tilde};
use crate::ui::draw::{open_reader, prepare_icon};
use image::{ImageFormat, RgbaImage};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Files up to this size (and [`KEEP_SIDE`]) are used unchanged.
pub const KEEP_BYTES: u64 = 512 * 1024;
/// Longest side, in pixels, of a file that is used unchanged.
pub const KEEP_SIDE: u32 = 512;
/// Longest side of the smaller copy that replaces a bigger image.
pub const STORE_PX: u32 = 256;
/// Images with more pixels than this are refused (decoded, they would take 4 bytes per pixel).
pub const MAX_IMPORT_PIXELS: u64 = 40_000_000;
/// Files bigger than this are refused without being opened.
pub const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

/// Where the result of an import goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A small PNG / JPEG / WebP stays where it is (what the file chooser does).
    InPlace,
    /// The result is always a file in the icons folder (what a drop does).
    Library,
}

/// The icon to use and, when something was changed or worth saying, a message for the user.
#[derive(Debug)]
pub struct Fitted {
    /// The icon string for `Item::icon` (`~/...` when the file is under the home directory).
    pub icon: String,
    pub note: Option<String>,
}

/// [`fit_into`] with the real icons folder. `src` is the path the file chooser returned.
pub fn fit(src: &str, mode: Mode) -> Result<Fitted, String> {
    fit_into(Path::new(src), &icons_dir(), &tilde, mode)
}

/// "340 KB" / "1.4 MB".
fn human(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes.div_ceil(1024).max(1))
    }
}

/// A library error message cut to something a sheet can show.
fn short(s: &str) -> String {
    const MAX: usize = 140;
    let s = s.trim();
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let cut: String = s.chars().take(MAX).collect();
        format!("{cut}\u{2026}")
    }
}

/// The reader's own name for a format, as people write it.
fn format_name(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::WebP => "WebP",
        ImageFormat::Gif => "GIF",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Tga => "TGA",
        ImageFormat::Qoi => "QOI",
        ImageFormat::Pnm => "PNM",
        _ => "image",
    }
}

/// Formats that profile sharing can carry, and that are therefore kept as they are.
fn is_native(f: ImageFormat) -> bool {
    matches!(f, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
}

/// The supported extensions, for a sentence.
fn supported_list() -> String {
    [
        "PNG", "JPG", "WebP", "GIF", "BMP", "ICO", "TIFF", "TGA", "QOI", "PNM",
    ]
    .join(", ")
}

/// Why a file could not be read, from the OS error.
fn io_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => {
            "Could not read that file: it does not exist (any more).".to_string()
        }
        std::io::ErrorKind::PermissionDenied => {
            "Could not read that file: CursorPie is not allowed to open it (permission denied)."
                .to_string()
        }
        _ => format!("Could not read that file ({}).", short(&e.to_string())),
    }
}

/// What a file is, when its first bytes say it is something that is **not** an image CursorPie can
/// use. `None` means "no objection": the decoder decides.
fn not_an_image(head: &[u8]) -> Option<&'static str> {
    let starts = |m: &[u8]| head.starts_with(m);
    let text = String::from_utf8_lossy(&head[..head.len().min(256)]).to_lowercase();
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("<svg") || (t.starts_with("<?xml") && t.contains("<svg")) || t.contains("<svg")
    {
        return Some(
            "SVG is a vector format and CursorPie cannot draw it yet. Export it as a PNG \
             (256 px or more) and import that.",
        );
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        let brand = &head[8..12];
        if matches!(
            brand,
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"mif1" | b"msf1"
        ) {
            return Some(
                "HEIC / HEIF photos are not supported. Convert it to PNG or JPG first \
                 (on macOS: open it in Preview and Export).",
            );
        }
        if matches!(brand, b"avif" | b"avis") {
            return Some("AVIF images are not supported. Convert it to PNG, JPG or WebP first.");
        }
    }
    if starts(&[0xff, 0x0a]) || starts(b"\0\0\0\x0cJXL ") {
        return Some("JPEG XL images are not supported. Convert it to PNG or JPG first.");
    }
    if starts(b"%PDF") {
        return Some("That is a PDF, not an image. Export the page as a PNG instead.");
    }
    if starts(b"8BPS") {
        return Some("Photoshop (PSD) files are not supported. Export it as a PNG first.");
    }
    if starts(b"icns") {
        return Some("macOS .icns files are not supported. Export it as a PNG first.");
    }
    if starts(b"PK\x03\x04") || starts(&[0x1f, 0x8b]) || starts(b"7z\xbc\xaf") {
        return Some("That is an archive, not an image. Extract it first.");
    }
    let pnm = head.len() >= 2 && head[0] == b'P' && (b'1'..=b'6').contains(&head[1]);
    let plain = !head.is_empty()
        && head
            .iter()
            .all(|&b| matches!(b, 9 | 10 | 13 | 32..=126) || b >= 0x80)
        && !head.contains(&0);
    if plain && !pnm && head.iter().filter(|&&b| b >= 0x80).count() * 10 < head.len() {
        return Some("That is a text file, not an image.");
    }
    None
}

/// The sentence for a decoder error of a file whose format was recognised.
fn decode_reason(e: &image::ImageError, f: ImageFormat) -> String {
    use image::ImageError as E;
    let name = format_name(f);
    match e {
        E::Unsupported(_) => format!(
            "This {name} file uses a feature CursorPie cannot decode ({}). \
             Re-save it as a plain PNG.",
            short(&e.to_string())
        ),
        E::Limits(_) => {
            "The image needs more memory to open than CursorPie allows. Choose a smaller one."
                .to_string()
        }
        E::IoError(io) if io.kind() != std::io::ErrorKind::UnexpectedEof => io_reason(io),
        E::Decoding(_) | E::IoError(_) => format!(
            "The {name} file is damaged or incomplete ({}). Open and re-save it in an image \
             editor, or choose another copy.",
            short(&e.to_string())
        ),
        _ => format!(
            "Could not decode the {name} image ({}).",
            short(&e.to_string())
        ),
    }
}

/// Everything [`fit_into`] needs to know before it decides: the format, the size in pixels and the
/// size of the file. Nothing is decoded; a refusal is a sentence that says why.
fn inspect(src: &Path) -> Result<(ImageFormat, (u32, u32), u64), String> {
    let meta = fs::metadata(src).map_err(|e| io_reason(&e))?;
    if meta.is_dir() {
        return Err("That is a folder, not an image file.".into());
    }
    if meta.len() == 0 {
        return Err("The file is empty (0 bytes).".into());
    }
    let mut head = [0u8; 300];
    let n = fs::File::open(src)
        .and_then(|mut f| f.read(&mut head))
        .map_err(|e| io_reason(&e))?;
    if let Some(why) = not_an_image(&head[..n]) {
        return Err(why.into());
    }
    let reader = open_reader(src).ok_or("Could not read that file.")?;
    let Some(format) = reader.format() else {
        return Err(format!(
            "CursorPie does not recognise this file as an image. It reads {}.",
            supported_list()
        ));
    };
    let dims = reader
        .into_dimensions()
        .map_err(|e| decode_reason(&e, format))?;
    Ok((format, dims, meta.len()))
}

/// Straight (not premultiplied) colors from premultiplied ones, to save a normal PNG.
fn unpremultiply(img: &mut RgbaImage) {
    for px in img.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for v in &mut px[..3] {
                *v = ((*v as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

/// A free file name `<stem>-icon.<ext>`, `<stem>-icon-2.<ext>`, ... inside `dir`.
fn free_name(dir: &Path, src: &Path, ext: &str) -> Option<PathBuf> {
    let stem: String = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(40)
        .collect();
    let stem = if stem.is_empty() {
        "icon".to_string()
    } else {
        stem
    };
    (1..1000)
        .map(|n| {
            if n == 1 {
                dir.join(format!("{stem}-icon.{ext}"))
            } else {
                dir.join(format!("{stem}-icon-{n}.{ext}"))
            }
        })
        .find(|p| !p.exists())
}

/// Checks `src`, and copies, converts or shrinks it into `dir` as `mode` and the image ask. `as_icon`
/// turns the final path into the icon string. The error is a sentence that says why the file was
/// refused, for a pop-up.
pub fn fit_into(
    src: &Path,
    dir: &Path,
    as_icon: &dyn Fn(&Path) -> String,
    mode: Mode,
) -> Result<Fitted, String> {
    let (format, (w, h), bytes) = inspect(src)?;
    if bytes > MAX_IMPORT_BYTES || u64::from(w) * u64::from(h) > MAX_IMPORT_PIXELS {
        return Err(format!(
            "That image is too large to use as an icon ({w} x {h} px, {}). Choose a smaller one.",
            human(bytes)
        ));
    }
    let native = is_native(format);
    let small = bytes <= KEEP_BYTES && w.max(h) <= KEEP_SIDE;
    let already_here = src.parent().is_some_and(|p| p == dir);
    if native && small {
        if mode == Mode::InPlace || already_here {
            return Ok(Fitted {
                icon: as_icon(src),
                note: None,
            });
        }
        let ext = match format {
            ImageFormat::Jpeg => "jpg",
            ImageFormat::WebP => "webp",
            _ => "png",
        };
        fs::create_dir_all(dir).map_err(|e| {
            format!(
                "Could not open the icons folder ({}).",
                short(&e.to_string())
            )
        })?;
        let dest = free_name(dir, src, ext).ok_or("Too many copies in the icons folder.")?;
        fs::copy(src, &dest).map_err(|e| {
            format!(
                "Could not copy the image into the icons folder ({}).",
                short(&e.to_string())
            )
        })?;
        return Ok(Fitted {
            icon: as_icon(&dest),
            note: None,
        });
    }

    let img = open_reader(src)
        .ok_or("Could not read that file.")?
        .decode()
        .map_err(|e| decode_reason(&e, format))?
        .into_rgba8();
    let mut small_img = prepare_icon(img, STORE_PX);
    unpremultiply(&mut small_img);
    fs::create_dir_all(dir).map_err(|e| {
        format!(
            "Could not open the icons folder ({}).",
            short(&e.to_string())
        )
    })?;
    let dest = free_name(dir, src, "png").ok_or("Too many copies in the icons folder.")?;
    small_img
        .save_with_format(&dest, ImageFormat::Png)
        .map_err(|e| {
            format!(
                "Could not save the smaller copy of the image ({}). Is the disk full or the \
                 icons folder read-only?",
                short(&e.to_string())
            )
        })?;
    let after = fs::metadata(&dest).map_or(0, |m| m.len());
    let mut note = if small {
        format!(
            "{} image converted to PNG ({} x {} px).",
            format_name(format),
            small_img.width(),
            small_img.height()
        )
    } else {
        format!(
            "Image shrunk for use as an icon: {w} x {h} px ({}) to {} x {} px ({}).",
            human(bytes),
            small_img.width(),
            small_img.height(),
            human(after)
        )
    };
    if !native && !small {
        note.push_str(&format!(
            " It was also converted from {} to PNG.",
            format_name(format)
        ));
    }
    note.push_str(" The new copy is in the icons folder; the original is untouched.");
    Ok(Fitted {
        icon: as_icon(&dest),
        note: Some(note),
    })
}

/// The result of importing several files at once (a drop): the icon to use, a note, and one line
/// `name: reason` for every file that was refused.
#[derive(Debug, Default)]
pub struct Batch {
    /// The first file that worked, which becomes the icon of the slice.
    pub icon: Option<String>,
    /// How many files worked.
    pub added: usize,
    /// What was done to the first one (shrunk, converted, ...).
    pub note: Option<String>,
    /// `(file name, reason)` of each refused file.
    pub failed: Vec<(String, String)>,
}

/// Imports every path of a drop into `dir` ([`Mode::Library`]). A folder is not walked: it is
/// reported, so a stray drop of "Downloads" does not copy a thousand files.
pub fn import_all(files: &[PathBuf], dir: &Path, as_icon: &dyn Fn(&Path) -> String) -> Batch {
    let mut out = Batch::default();
    for f in files {
        let name = f.file_name().map_or_else(
            || f.to_string_lossy().to_string(),
            |n| n.to_string_lossy().to_string(),
        );
        match fit_into(f, dir, as_icon, Mode::Library) {
            Ok(fitted) => {
                out.added += 1;
                if out.icon.is_none() {
                    out.icon = Some(fitted.icon);
                    out.note = fitted.note;
                }
            }
            Err(why) => out.failed.push((name, why)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cursorpie-fit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn as_icon(p: &Path) -> String {
        p.to_string_lossy().to_string()
    }

    #[test]
    fn a_small_image_is_used_as_it_is() {
        let dir = scratch("small");
        let src = dir.join("a.png");
        RgbaImage::from_pixel(64, 64, Rgba([9, 9, 9, 255]))
            .save(&src)
            .unwrap();
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let f = fit_into(&src, &out, &as_icon, Mode::InPlace).unwrap();
        assert_eq!(f.icon, as_icon(&src));
        assert!(f.note.is_none());
        assert_eq!(
            fs::read_dir(&out).unwrap().count(),
            0,
            "nothing was written"
        );
    }

    #[test]
    fn a_big_image_is_shrunk_into_the_icons_folder_and_the_user_is_told() {
        let dir = scratch("big");
        let src = dir.join("My photo.png");
        RgbaImage::from_pixel(1200, 800, Rgba([200, 30, 30, 255]))
            .save(&src)
            .unwrap();
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let f = fit_into(&src, &out, &as_icon, Mode::InPlace).unwrap();
        assert!(f.icon.ends_with("My_photo-icon.png"), "{}", f.icon);
        assert!(f.note.as_deref().unwrap().contains("1200 x 800"));
        let small = image::open(&f.icon).unwrap().to_rgba8();
        assert_eq!((small.width(), small.height()), (256, 171));
        assert_eq!(small.get_pixel(100, 80).0[3], 255);
        assert!(src.exists(), "the original is never touched");
        // choosing it again must not overwrite the first copy
        let again = fit_into(&src, &out, &as_icon, Mode::InPlace).unwrap();
        assert_ne!(again.icon, f.icon);
    }

    #[test]
    fn unreadable_and_missing_files_give_a_sentence() {
        let dir = scratch("bad");
        let junk = dir.join("x.png");
        fs::write(&junk, b"not an image").unwrap();
        assert!(
            fit_into(&junk, &dir, &as_icon, Mode::InPlace)
                .unwrap_err()
                .contains("not an image")
        );
        let missing = dir.join("nope.png");
        assert!(
            fit_into(&missing, &dir, &as_icon, Mode::InPlace)
                .unwrap_err()
                .contains("read")
        );
    }

    #[test]
    fn unpremultiply_restores_straight_colors() {
        let mut img = RgbaImage::from_pixel(1, 1, Rgba([100, 50, 20, 128]));
        unpremultiply(&mut img);
        assert_eq!(img.get_pixel(0, 0).0, [199, 100, 40, 128]);
    }

    #[test]
    fn sizes_read_well() {
        assert_eq!(human(100), "1 KB");
        assert_eq!(human(340 * 1024), "340 KB");
        assert_eq!(human(1536 * 1024), "1.5 MB");
    }

    fn refused(name: &str, bytes: &[u8]) -> String {
        let dir = scratch("refused");
        let f = dir.join(name);
        fs::write(&f, bytes).unwrap();
        fit_into(&f, &dir, &as_icon, Mode::Library).unwrap_err()
    }

    #[test]
    fn every_refusal_says_why() {
        assert!(refused("a.svg", b"<svg xmlns='x'></svg>").contains("SVG"));
        assert!(refused("a.png", b"<?xml version='1.0'?><svg></svg>").contains("SVG"));
        assert!(refused("a.heic", b"\0\0\0\x18ftypheic\0\0\0\0mif1heic").contains("HEIC"));
        assert!(refused("a.avif", b"\0\0\0\x20ftypavif\0\0\0\0mif1avif").contains("AVIF"));
        assert!(refused("a.pdf", b"%PDF-1.7\n%\xe2\xe3\xcf\xd3").contains("PDF"));
        assert!(refused("a.png", b"PK\x03\x04\x14\0\0\0").contains("archive"));
        assert!(refused("a.png", b"hello, this is text").contains("text file"));
        assert!(refused("a.png", b"").contains("empty"));
        // a PNG cut after its signature is recognised, but damaged
        let cut = refused(
            "a.png",
            &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0],
        );
        assert!(cut.contains("PNG") || cut.contains("read"), "{cut}");
        let nothing = refused("a.xyz", &[1, 2, 3, 4, 0, 0, 7, 7]);
        assert!(nothing.contains("does not recognise"), "{nothing}");
    }

    #[test]
    fn a_folder_and_a_missing_file_are_told_apart() {
        let dir = scratch("folder");
        let err = fit_into(&dir, &dir, &as_icon, Mode::Library).unwrap_err();
        assert!(err.contains("folder"), "{err}");
        let err = fit_into(&dir.join("gone.png"), &dir, &as_icon, Mode::Library).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    fn the_wider_codecs_are_decoded_and_stored_as_png() {
        use image::ImageFormat as F;
        let dir = scratch("codecs");
        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let cases = [
            (F::Gif, "a.gif", "GIF"),
            (F::Bmp, "a.bmp", "BMP"),
            (F::Ico, "a.ico", "ICO"),
            (F::Tiff, "a.tiff", "TIFF"),
            (F::Tga, "a.tga", "TGA"),
            (F::Qoi, "a.qoi", "QOI"),
        ];
        for (fmt, name, label) in cases {
            let src = dir.join(name);
            RgbaImage::from_pixel(32, 32, Rgba([10, 200, 30, 255]))
                .save_with_format(&src, fmt)
                .unwrap_or_else(|e| panic!("{label}: {e}"));
            let f = fit_into(&src, &out, &as_icon, Mode::InPlace)
                .unwrap_or_else(|e| panic!("{label}: {e}"));
            assert!(f.icon.ends_with(".png"), "{label}: {}", f.icon);
            assert!(f.note.as_deref().unwrap().contains(label), "{label}");
            let back = image::open(&f.icon).unwrap().to_rgba8();
            assert_eq!(back.get_pixel(5, 5).0, [10, 200, 30, 255], "{label}");
        }
        for ext in crate::platform::IMAGE_EXTENSIONS {
            assert!(crate::platform::is_image_extension(&ext.to_uppercase()));
        }
    }

    #[test]
    fn a_dropped_small_png_is_copied_into_the_library_not_referenced() {
        let dir = scratch("drop");
        let src = dir.join("logo.png");
        RgbaImage::from_pixel(40, 40, Rgba([1, 2, 3, 255]))
            .save(&src)
            .unwrap();
        let out = dir.join("lib");
        fs::create_dir_all(&out).unwrap();
        let f = fit_into(&src, &out, &as_icon, Mode::Library).unwrap();
        assert!(f.icon.starts_with(out.to_str().unwrap()), "{}", f.icon);
        assert!(f.note.is_none());
        // a file that is already in the library is not copied onto itself
        let again = fit_into(Path::new(&f.icon), &out, &as_icon, Mode::Library).unwrap();
        assert_eq!(again.icon, f.icon);
        assert_eq!(fs::read_dir(&out).unwrap().count(), 1);
    }

    #[test]
    fn a_batch_keeps_the_first_good_file_and_lists_the_bad_ones() {
        let dir = scratch("batch");
        let out = dir.join("lib");
        fs::create_dir_all(&out).unwrap();
        let (a, b, bad) = (dir.join("a.png"), dir.join("b.gif"), dir.join("c.svg"));
        RgbaImage::from_pixel(8, 8, Rgba([5, 5, 5, 255]))
            .save(&a)
            .unwrap();
        RgbaImage::from_pixel(8, 8, Rgba([6, 6, 6, 255]))
            .save_with_format(&b, image::ImageFormat::Gif)
            .unwrap();
        fs::write(&bad, "<svg/>").unwrap();
        let r = import_all(&[bad, a, b, dir.clone()], &out, &as_icon);
        assert_eq!(r.added, 2);
        assert!(r.icon.unwrap().contains("a-icon.png"));
        assert_eq!(r.failed.len(), 2);
        assert_eq!(r.failed[0].0, "c.svg");
        assert!(r.failed[0].1.contains("SVG"));
        assert!(r.failed[1].1.contains("folder"));
    }
}
