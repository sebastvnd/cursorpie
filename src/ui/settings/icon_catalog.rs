//! Built-in icon catalog plus the user's own icons.
//!
//! - `CATEGORIES`: icons of the built-in set ([`symbols`]) usable as slice icons, each stored in
//!   the config as `ico:<name>`. To add one, append a line `ic("ico:name", "Name", "search keywords")`
//!   (the name must exist in `symbols.rs`; a test checks it).
//! - `scan_user_icons()`: images (see `platform::IMAGE_EXTENSIONS`) found in the folder
//!   `~/Library/Application Support/CursorPie/icons/`.
//!
//! Any other text or emoji can still be typed into the picker's text field; it is drawn with the
//! text font.

use crate::config::{icons_dir, tilde};
use crate::platform;
use crate::ui::symbols;
use std::fs;
use std::path::PathBuf;

/// One catalog icon.
pub struct IconDef {
    /// The slice icon string, `ico:<name>`.
    pub icon: &'static str,
    pub name: &'static str,
    /// lower-case search keywords, separated by spaces
    pub tags: &'static str,
}

/// A named group of icons shown as a tab in the picker.
pub struct Category {
    pub title: &'static str,
    /// The icon on its tab.
    pub icon: char,
    pub icons: &'static [IconDef],
}

/// Shorthand used to write the catalog: the icon's name in the icon set, a title and keywords.
const fn ic(icon: &'static str, name: &'static str, tags: &'static str) -> IconDef {
    IconDef { icon, name, tags }
}

/// The built-in catalog. Add icons here.
pub const CATEGORIES: &[Category] = &[
    Category {
        title: "Edit",
        icon: symbols::PENCIL_SIMPLE,
        icons: &[
            ic("ico:scissors", "Cut", "cut scissors"),
            ic("ico:copy", "Copy", "copy duplicate"),
            ic("ico:clipboard-text", "Paste", "paste clipboard"),
            ic("ico:arrow-u-up-left", "Undo", "undo back"),
            ic("ico:arrow-u-up-right", "Redo", "redo forward"),
            ic("ico:pencil-simple", "Edit", "pencil write edit"),
            ic("ico:trash", "Delete", "trash delete remove"),
            ic("ico:push-pin", "Pin", "pin"),
            ic("ico:link", "Link", "link url chain"),
            ic("ico:paperclip", "Attach", "clip attach"),
            ic("ico:check", "Done", "check ok yes tick"),
            ic("ico:x", "Close", "close cancel x"),
            ic("ico:bookmark-simple", "Bookmark", "bookmark"),
            ic("ico:selection-all", "Select all", "select all"),
            ic("ico:eraser", "Erase", "erase clear"),
        ],
    },
    Category {
        title: "Navigate",
        icon: symbols::ARROW_RIGHT,
        icons: &[
            ic("ico:arrow-left", "Left", "left arrow back"),
            ic("ico:arrow-right", "Right", "right arrow forward"),
            ic("ico:arrow-up", "Up", "up arrow"),
            ic("ico:arrow-down", "Down", "down arrow"),
            ic("ico:arrows-left-right", "Horizontal", "horizontal sides"),
            ic("ico:arrows-down-up", "Vertical", "vertical scroll page"),
            ic("ico:house", "Home", "home"),
            ic("ico:arrows-clockwise", "Refresh", "reload refresh sync"),
            ic("ico:rewind", "Rewind", "rewind fast back"),
            ic("ico:fast-forward", "Fast forward", "fast forward"),
            ic("ico:magnifying-glass-plus", "Zoom in", "plus add zoom in"),
            ic("ico:magnifying-glass-minus", "Zoom out", "minus zoom out"),
            ic("ico:arrows-out", "Expand", "full screen expand"),
            ic("ico:arrows-in", "Collapse", "collapse shrink"),
            ic("ico:arrow-line-right", "Tab", "tab next"),
            ic("ico:swap", "Swap", "swap switch"),
        ],
    },
    Category {
        title: "Media",
        icon: symbols::PLAY,
        icons: &[
            ic("ico:play", "Play", "play"),
            ic("ico:pause", "Pause", "pause"),
            ic("ico:stop", "Stop", "stop"),
            ic("ico:skip-back", "Previous", "previous track"),
            ic("ico:skip-forward", "Next", "next track"),
            ic("ico:speaker-high", "Volume", "volume sound"),
            ic("ico:speaker-slash", "Mute", "mute silent"),
            ic("ico:microphone", "Microphone", "microphone record"),
            ic("ico:camera", "Camera", "camera photo"),
            ic("ico:crop", "Screenshot", "screenshot capture crop"),
            ic("ico:video-camera", "Video", "video record"),
            ic("ico:music-notes", "Music", "music notes"),
        ],
    },
    Category {
        title: "System",
        icon: symbols::GEAR_SIX,
        icons: &[
            ic("ico:gear-six", "Settings", "settings gear preferences"),
            ic("ico:lock-simple", "Lock", "lock"),
            ic("ico:lock-simple-open", "Unlock", "unlock"),
            ic("ico:wifi-high", "Wi-Fi", "wifi network signal"),
            ic("ico:bluetooth", "Bluetooth", "bluetooth"),
            ic("ico:sun", "Light", "sun light day brightness"),
            ic("ico:moon", "Dark", "moon night dark"),
            ic("ico:battery-full", "Battery", "battery power"),
            ic("ico:keyboard", "Keyboard", "keyboard keys"),
            ic("ico:monitor", "Display", "display screen monitor desktop"),
            ic("ico:laptop", "Laptop", "laptop computer"),
            ic("ico:mouse-simple", "Mouse", "mouse cursor"),
            ic("ico:printer", "Print", "print printer"),
            ic("ico:calendar-blank", "Calendar", "calendar date"),
            ic("ico:clock", "Clock", "clock time"),
            ic("ico:device-mobile", "Phone", "phone mobile"),
            ic("ico:bell", "Notifications", "bell notifications"),
            ic("ico:globe", "Web", "globe web internet browser"),
            ic("ico:cpu", "Processor", "cpu chip"),
            ic("ico:headphones", "Headphones", "headphones audio"),
        ],
    },
    Category {
        title: "Files",
        icon: symbols::FOLDER_SIMPLE,
        icons: &[
            ic("ico:folder-simple", "Folder", "folder finder"),
            ic("ico:folder-open", "Open folder", "open folder"),
            ic("ico:files", "Files", "files archive"),
            ic("ico:note-pencil", "Notes", "notes memo text"),
            ic("ico:envelope-simple", "Mail", "mail email envelope"),
            ic("ico:archive", "Archive", "archive box"),
            ic("ico:package", "Package", "package box"),
            ic("ico:cloud", "Cloud", "cloud sync"),
            ic("ico:rocket-launch", "Launch", "launch rocket start"),
            ic("ico:book-open", "Book", "book read"),
            ic("ico:chart-bar", "Chart", "chart stats data"),
            ic("ico:file-text", "Document", "document file text"),
            ic("ico:browser", "Browser", "browser window"),
            ic("ico:columns", "Columns", "columns tabs split"),
            ic("ico:squares-four", "Apps", "apps grid launcher"),
            ic("ico:sidebar-simple", "Sidebar", "sidebar panel"),
        ],
    },
    Category {
        title: "Symbols",
        icon: symbols::STAR,
        icons: &[
            ic("ico:bug", "Bug", "bug debug"),
            ic("ico:lightning", "Quick", "zap quick fast lightning"),
            ic("ico:fire", "Fire", "fire hot"),
            ic("ico:check-circle", "Success", "success check done"),
            ic("ico:x-circle", "Error", "error cross no"),
            ic("ico:question", "Help", "help question"),
            ic("ico:warning", "Alert", "alert important warning"),
            ic("ico:star", "Star", "star favorite"),
            ic("ico:heart", "Heart", "heart love like"),
            ic("ico:flag", "Flag", "flag finish"),
            ic("ico:target", "Target", "target goal"),
            ic("ico:diamond", "Gem", "gem diamond"),
            ic("ico:gift", "Gift", "gift present"),
            ic("ico:thumbs-up", "Like", "like thumbs up"),
            ic("ico:eye", "Show", "eye show visible"),
            ic("ico:eye-slash", "Hide", "eye hide hidden"),
            ic("ico:tag", "Tag", "tag label"),
            ic("ico:lightbulb", "Idea", "idea light bulb"),
            ic("ico:key", "Key", "key password"),
            ic("ico:paint-brush", "Paint", "paint brush draw"),
            ic("ico:eyedropper", "Pick color", "eyedropper color pick"),
            ic("ico:magic-wand", "Magic", "magic wand"),
        ],
    },
    Category {
        title: "Code",
        icon: symbols::TERMINAL_WINDOW,
        icons: &[
            ic(
                "ico:brackets-angle",
                "Angle brackets",
                "brackets angle html",
            ),
            ic("ico:brackets-curly", "Braces", "braces code curly"),
            ic("ico:brackets-square", "Brackets", "brackets square"),
            ic("ico:terminal-window", "Terminal", "terminal command shell"),
            ic("ico:terminal", "Console", "console prompt"),
            ic("ico:command", "Command", "command cmd key"),
            ic("ico:option", "Option", "option alt key"),
            ic("ico:arrow-elbow-down-left", "Return", "return enter"),
            ic("ico:text-aa", "Text", "text font type"),
            ic("ico:hash", "Hash", "hash number tag"),
            ic("ico:at", "At", "at mention email"),
            ic("ico:code", "Code", "code develop"),
            ic("ico:translate", "Translate", "translate language"),
            ic("ico:scan", "Scan", "scan qr"),
        ],
    },
];

/// Search results. `cat` = 0 for all categories, `n` for category `n - 1` (0-based).
pub fn filtered(query: &str, cat: usize) -> Vec<&'static IconDef> {
    let q = query.trim().to_lowercase();
    let mut out = Vec::new();
    for (ci, c) in CATEGORIES.iter().enumerate() {
        if cat != 0 && cat != ci + 1 {
            continue;
        }
        for d in c.icons {
            if q.is_empty() || d.name.to_lowercase().contains(&q) || d.tags.contains(&q) {
                out.push(d);
            }
        }
    }
    out
}

// ------------------------------------------------------------ user icons folder

/// Most images listed from the user icons folder.
pub const MAX_USER_ICONS: usize = 120;
/// Larger images are ignored (they are decoded at full size before scaling).
const MAX_ICON_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// `~/Library/Application Support/CursorPie/icons` (created if missing).
pub fn user_icons_dir() -> PathBuf {
    icons_dir()
}

/// What [`scan_user_icons`] found.
pub struct UserIcons {
    /// Images that can be listed, as strings usable directly as `Item::icon`.
    pub icons: Vec<String>,
    /// Images that exist but are left out: over [`MAX_ICON_FILE_BYTES`], or past [`MAX_USER_ICONS`].
    pub not_listed: usize,
}

/// Images in the icons folder, and how many were left out (so the picker can say so).
pub fn scan_user_icons() -> UserIcons {
    let Ok(rd) = fs::read_dir(user_icons_dir()) else {
        return UserIcons {
            icons: Vec::new(),
            not_listed: 0,
        };
    };
    let mut too_big = 0;
    let mut v: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .map(crate::platform::is_image_extension)
                .unwrap_or(false)
        })
        .filter(|p| {
            let fits = p
                .metadata()
                .map(|m| m.len() <= MAX_ICON_FILE_BYTES)
                .unwrap_or(false);
            too_big += usize::from(!fits);
            fits
        })
        .map(|p| tilde(&p))
        .collect();
    v.sort();
    let over_limit = v.len().saturating_sub(MAX_USER_ICONS);
    v.truncate(MAX_USER_ICONS);
    UserIcons {
        icons: v,
        not_listed: too_big + over_limit,
    }
}

/// Deletes the image `icon` (an icon string, `~/` allowed) from the user icons folder. Only a file
/// that is an image and sits directly in that folder is ever removed: an icon string that points
/// anywhere else (a picture the person chose in place, with `Mode::InPlace`) is refused, so the
/// button can never delete a file the person did not import. The original of an imported image is
/// never touched, only the copy in the folder.
pub fn delete_user_icon(icon: &str) -> Result<(), String> {
    delete_in(&user_icons_dir(), icon)
}

fn delete_in(dir: &std::path::Path, icon: &str) -> Result<(), String> {
    let path =
        crate::config::image_path(icon).ok_or_else(|| "That is not an image.".to_string())?;
    let (Ok(dir), Ok(file)) = (fs::canonicalize(dir), fs::canonicalize(&path)) else {
        return Err("The image is already gone.".to_string());
    };
    if file.parent() != Some(dir.as_path()) {
        return Err("Only images in the icons folder can be deleted here.".to_string());
    }
    fs::remove_file(&file).map_err(|e| format!("Could not delete the image: {e}"))
}

/// Opens the icons folder in the file manager.
pub fn open_icons_folder() {
    platform::open_path(&user_icons_dir());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_icon_exists_in_the_icon_set() {
        for c in CATEGORIES {
            for d in c.icons {
                assert!(
                    symbols::slice_icon(d.icon).is_some(),
                    "{} is not in symbols.rs",
                    d.icon
                );
            }
        }
    }

    #[test]
    fn no_icon_is_listed_twice_within_a_category() {
        for c in CATEGORIES {
            for (i, d) in c.icons.iter().enumerate() {
                assert!(
                    !c.icons[..i].iter().any(|o| o.icon == d.icon),
                    "{} twice in {}",
                    d.icon,
                    c.title
                );
            }
        }
    }

    #[test]
    fn only_images_in_the_icons_folder_are_deleted() {
        let base = std::env::temp_dir().join(format!("cursorpie-del-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let icons = base.join("icons");
        fs::create_dir_all(&icons).unwrap();
        let mine = icons.join("a.png");
        let other = base.join("b.png");
        fs::write(&mine, b"x").unwrap();
        fs::write(&other, b"x").unwrap();
        // outside the folder, and not an image: refused, nothing removed
        assert!(delete_in(&icons, other.to_str().unwrap()).is_err());
        assert!(other.exists());
        assert!(delete_in(&icons, "ico:copy").is_err());
        // inside the folder: removed; a second time it is already gone
        delete_in(&icons, mine.to_str().unwrap()).unwrap();
        assert!(!mine.exists());
        assert!(delete_in(&icons, mine.to_str().unwrap()).is_err());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn search_finds_by_name_and_by_keyword() {
        assert!(filtered("copy", 0).iter().any(|d| d.icon == "ico:copy"));
        assert!(filtered("scissors", 0).iter().any(|d| d.name == "Cut"));
        assert!(filtered("zzzz", 0).is_empty());
        assert!(
            filtered("", 2).iter().all(|d| d.icon != "ico:copy"),
            "category 2 is Navigate"
        );
    }
}
