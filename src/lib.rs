//! # CursorPie
//!
//! A hold-to-open **pie menu for macOS, Windows and Linux**: hold a mouse button, flick toward a slice, release.
//! A slice can send a shortcut, launch an app, open a URL, run a shell command, open a submenu,
//! or remap the mouse's back/forward buttons. Every app can have its own pie ("profile").
//!
//! This is the documentation for **developers and contributors**. End-user documentation lives
//! in the repository's `README.md` and the `docs/` folder (start with `docs/USER_GUIDE.md`).
//!
//! The package is a library (this crate, which holds everything) plus a tiny binary
//! (`src/main.rs`) that calls [`run`]. Keeping the code in a library lets it be documented here.
//!
//! ## Start here
//!
//! | Guide | Read it to learn |
//! |---|---|
//! | [`guides::architecture`] | threads, message flow, locking rules, the gesture state machine |
//! | [`guides::config_format`] | the `config.json` schema, versioning and validation |
//! | [`guides::extending`] | step-by-step recipes: add an action, a setting, a tab, an icon... |
//! | [`guides::contributing`] | environment, build, test, style and pull-request checklist |
//!
//! ## Module map
//!
//! | Module | Runs on | Responsibility |
//! |---|---|---|
//! | [`config`] | any thread | data model, defaults, validation, `config.json`, the live global copy |
//! | [`tap`] | `cursorpie-tap` thread | global event tap and the gesture state machine |
//! | [`actions`] | `cursorpie-actions` thread | executes actions (shortcuts, `open`, shell) in order |
//! | [`cache`] | any thread | bounded least-recently-used cache (backs the icon textures) |
//! | [`diag`] | any thread | the opt-in diagnostic log for developers (`CURSORPIE_DEBUG=1`, `docs/DEBUGGING.md`) |
//! | [`history`] | UI thread | undo / redo snapshots for the Settings editor |
//! | [`side`] | any thread | which pie the back/forward buttons follow |
//! | [`geometry`] | any thread | pie sizes and slice hit-testing (shared by tap and UI) |
//! | [`keys`] | any thread | shortcut model (key code + modifiers) and its labels |
//! | [`msg`] | any thread | messages sent *to* the UI thread |
//! | [`platform`] | any thread (some main only) | OS glue: permissions, apps, login item, keyboard layout (macOS, Windows, Linux, stub) |
//! | [`ui`] | main thread | eframe app, pie renderer, Settings window, first-launch welcome window, menu bar icon |
//! | [`about`] | - | name, version, links shown in the About tab |
//!
//! ## Platform
//!
//! macOS (11 or later), Windows (10 1809 or later) and Linux (X11 sessions). Other targets build
//! against a do-nothing stub backend so the portable code can be unit tested there; see [`platform`].
//! The Windows port is described in `docs/WINDOWS_MIGRATION.md`, the Linux one in
//! `docs/LINUX_MIGRATION.md`.
//! To read these docs locally run `cargo docs` (an alias for
//! `cargo doc --no-deps --document-private-items --open`, see `.cargo/config.toml`).
//! When published, docs.rs builds them for the Apple targets listed in `Cargo.toml`.

// The stub (targets without a backend) has no hook, so the gesture machinery is only exercised by
// tests there.
#![cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]

pub mod about;
pub mod actions;
pub mod cache;
pub mod config;
pub mod diag;
pub mod geometry;
pub mod guides;
pub mod history;
pub mod keys;
pub mod msg;
pub mod platform;
pub mod side;
pub mod tap;
pub mod ui;

use eframe::egui;
use msg::Notifier;
use std::sync::mpsc;
use ui::app::App;

/// Starts CursorPie and blocks until the app quits.
///
/// Order matters here:
/// 1. a command-line switch (`--uninstall`) is handled and ends the run, then a copy started from
///    outside the install folder installs itself and hands over to the installed copy, then a
///    second copy of an app that already runs exits (all three are no-ops on macOS);
/// 2. the action worker thread is started ([`actions::init`]);
/// 3. the eframe window is created hidden, transparent and click-through (it renders with `glow`
///    on Windows and `wgpu` on macOS, chosen in `Cargo.toml`: see `docs/UI_DESIGN.md`);
/// 4. inside eframe's creator (which runs on the **main thread**, inside the event loop) the
///    Dock icon is hidden, the keyboard-layout snapshot is taken (TIS needs the main thread),
///    the event-tap thread is spawned and the menu bar icon is created.
pub fn run() -> eframe::Result<()> {
    if platform::handle_command_line() {
        return Ok(());
    }
    // Hand over before claiming the single-instance lock, so the installed copy can take it.
    if let Some(installed) = platform::ensure_installed() {
        match std::process::Command::new(&installed).spawn() {
            Ok(_) => return Ok(()),
            Err(e) => eprintln!("CursorPie: could not start {}: {e}", installed.display()),
        }
    }
    if !platform::single_instance() {
        return Ok(());
    }
    diag::init(); // the opt-in debug log; does nothing unless it is switched on
    actions::init();
    let (tx, rx) = mpsc::channel();
    let win = config::cfg().pie_size;

    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([win, win])
        .with_decorations(false)
        .with_transparent(true)
        .with_has_shadow(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_mouse_passthrough(true)
        .with_active(false)
        .with_visible(false);
    // no taskbar button for the hidden pie window (Windows only; macOS hides the Dock icon instead)
    #[cfg(windows)]
    let viewport = viewport.with_taskbar(false);
    // the X11 window class: lets a window manager rule or a launcher recognize CursorPie
    #[cfg(target_os = "linux")]
    let viewport = viewport.with_app_id("cursorpie");
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        about::APP_NAME,
        options,
        Box::new(move |cc| {
            // runs on the main thread inside the running event loop
            platform::set_accessory_policy(); // no Dock icon
            // no OS frame: on Windows the DWM would draw a white box and a border around the pie
            if let Some(handle) = ui::app::native_handle(cc) {
                platform::clear_window_frame(handle);
            }
            platform::layout::init(); // keyboard-layout snapshot (TIS must run on the main thread)
            diag::startup_snapshot();
            let notifier = Notifier {
                tx: tx.clone(),
                ctx: cc.egui_ctx.clone(),
            };
            tap::spawn(notifier.clone());
            let tray = ui::tray::build(&notifier);
            // the very first start: say hello (decided before anything can write config.json)
            if config::welcome_due() {
                let (x, y) = platform::cursor_pos().unwrap_or((400.0, 300.0));
                notifier.send(msg::Msg::OpenWelcome { x, y });
            }
            Ok(Box::new(App::new(rx, tx, win, tray)))
        }),
    )
}
