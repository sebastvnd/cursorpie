//! Identity of the app: name, version, author, links and credits.
//!
//! Everything the About tab shows lives here, and [`BUNDLE_ID`] doubles as the LaunchAgent label.
//! Edit these constants (and `Cargo.toml`, `LICENSE`, `scripts/bundle.sh`) to rebrand a fork.

/// Display name, also used as the window title and tray tooltip.
pub const APP_NAME: &str = "CursorPie";
/// Version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// One-line description shown under the name in About.
pub const TAGLINE: &str = "A hold-to-open pie menu for your mouse or trackpad";

/// Used as the LaunchAgent label and the .app bundle identifier on macOS (on Windows the startup entry is named
/// after [`APP_NAME`]) (keep it unique, reverse-DNS style).
pub const BUNDLE_ID: &str = "io.github.cursorpie";

/// Author name shown in About.
pub const AUTHOR_NAME: &str = "Sebastvnd";

/// Command-line switch the startup entry adds: "I was started by the login, not by the user".
/// The Windows build then waits for the desktop to be ready before it installs its hooks and its
/// tray icon (see `platform::wait_for_login_ready`). Other platforms ignore it.
pub const AUTOSTART_ARG: &str = "--autostart";

/// Command-line switch the installing copy adds when it hands over to the installed copy: "you were
/// started by another copy of me, do not hand over again". Without it two paths that name the same
/// file but are written differently (short 8.3 names, a junction, upper/lower case of a non-ASCII
/// user name) make the installed copy believe it is somewhere else and start itself again, forever.
pub const HANDOVER_ARG: &str = "--handover";

/// Command-line switch that makes the program print its name and version and exit at once, before
/// anything else starts. The update runs the downloaded program with it, so a build that cannot start
/// on this system (a C library that is too old, say) is found out before it replaces the working one.
pub const SELFTEST_ARG: &str = "--selftest";
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
/// The page of the newest stable release. GitHub redirects it, so it never goes out of date; the
/// update offer opens this page and never an address that came from the network.
pub const LATEST_RELEASE_URL: &str = "https://github.com/sebastvnd/cursorpie/releases/latest";
/// What the daily update check asks (see [`crate::update`]): the newest stable release, as JSON.
/// It is the one address CursorPie connects to by itself; a package is only downloaded from
/// [`RELEASES_URL`] when the person asks for the install.
pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/sebastvnd/cursorpie/releases/latest";

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
pub const PRIVACY: &str = "CursorPie works on your computer and collects no data. Once a day, when you open Settings, it asks GitHub for the newest release so it can offer an update. Only if you press Install update does it download that release's package from GitHub. These are the only network connections it makes. GitHub sees your IP address and the CursorPie version, like any web request, and nothing else is sent. You can switch the daily check off under Updates. \
Keyboard events are only inspected while you are recording a shortcut in Settings, and are never stored or logged.";
