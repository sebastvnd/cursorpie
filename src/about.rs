//! Identity of the app: name, version, author, links and credits.
//!
//! Everything the About tab shows lives here, and [`BUNDLE_ID`] doubles as the LaunchAgent label.
//! Edit these constants (and `Cargo.toml`, `LICENSE`, `scripts/bundle.sh`) to rebrand a fork.

/// Display name, also used as the window title and tray tooltip.
pub const APP_NAME: &str = "CursorPie";
/// Version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// One-line description shown under the name in About.
pub const TAGLINE: &str = "A hold-to-open pie menu for your mouse";

/// Used as the LaunchAgent label and the .app bundle identifier on macOS (on Windows the startup entry is named
/// after [`APP_NAME`]) (keep it unique, reverse-DNS style).
pub const BUNDLE_ID: &str = "io.github.cursorpie";

/// Author name shown in About.
pub const AUTHOR_NAME: &str = "Sebastvnd";

/// Command-line switch the startup entry adds: "I was started by the login, not by the user".
/// The Windows build then waits for the desktop to be ready before it installs its hooks and its
/// tray icon (see `platform::wait_for_login_ready`). Other platforms ignore it.
pub const AUTOSTART_ARG: &str = "--autostart";
/// GitHub user name of the author (without `@`).
pub const AUTHOR_GITHUB_USER: &str = "sebastvnd";
/// GitHub profile of the author.
pub const AUTHOR_GITHUB_URL: &str = "https://github.com/sebastvnd";
/// Contact address of the author.
pub const AUTHOR_EMAIL: &str = "sebastvnd@gmail.com";

/// The CursorPie website.
pub const WEBSITE_URL: &str = "https://sebastvnd.github.io/cursorpie/";
/// Source repository.
pub const REPO_URL: &str = "https://github.com/sebastvnd/cursorpie";
/// Where to report bugs and ideas.
pub const ISSUES_URL: &str = "https://github.com/sebastvnd/cursorpie/issues";
/// Where to download builds.
pub const RELEASES_URL: &str = "https://github.com/sebastvnd/cursorpie/releases";

/// Name of the license.
pub const LICENSE_NAME: &str = "MIT License";
/// Copyright line.
pub const COPYRIGHT: &str = "Copyright (c) 2026 Sebastvnd";

/// `(name, url)` of the project that inspired CursorPie.
pub const INSPIRED_BY: (&str, &str) = ("Kando", "https://github.com/kando-menu/kando");

/// (name, url, license) of the main open-source building blocks.
pub const CREDITS: &[(&str, &str, &str)] = &[
    ("Inter (font)", "https://rsms.me/inter/", "SIL OFL 1.1"),
    (
        "Isometra (font)",
        "https://github.com/field2/isometra",
        "SIL OFL 1.1",
    ),
    (
        "Phosphor Icons",
        "https://github.com/phosphor-icons/core",
        "MIT",
    ),
    (
        "egui / eframe",
        "https://github.com/emilk/egui",
        "MIT / Apache-2.0",
    ),
    (
        "core-graphics",
        "https://github.com/servo/core-foundation-rs",
        "MIT / Apache-2.0",
    ),
    (
        "tray-icon / muda",
        "https://github.com/tauri-apps/tray-icon",
        "MIT / Apache-2.0",
    ),
    (
        "objc2",
        "https://github.com/madsmtm/objc2",
        "MIT / Zlib / Apache-2.0",
    ),
    (
        "windows-sys",
        "https://github.com/microsoft/windows-rs",
        "MIT / Apache-2.0",
    ),
    (
        "serde",
        "https://github.com/serde-rs/serde",
        "MIT / Apache-2.0",
    ),
    (
        "image",
        "https://github.com/image-rs/image",
        "MIT / Apache-2.0",
    ),
];

/// Privacy statement shown in About.
pub const PRIVACY: &str = "CursorPie works entirely on your computer. It makes no network connections and collects no data. \
Keyboard events are only inspected while you are recording a shortcut in Settings, and are never stored or logged.";
