//! # CursorPie
//!
//! A hold-to-open **pie menu for macOS, Windows and Linux**: hold a mouse button or touch and hold a trackpad, flick toward a slice, release.
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
//! | [`running`] | `cursorpie-running` thread | whether the app of an "Open app" slice already runs, and the new-window / open-app choice that follows |
//! | [`bridge`] | `cursorpie-bridge*` threads | the optional browser extension (embedded in the program): which web pages are open, go to one, open a new window; also the native messaging host mode |
//! | [`cache`] | any thread | bounded least-recently-used cache (backs the icon textures) |
//! | [`debug`] | any thread | switches the opt-in diagnostic log on and holds the app's probes; the logging itself is the separate `diag` crate (`diag/`, `CURSORPIE_DEBUG=1`, `docs/DEBUGGING.md`) |
//! | [`history`] | UI thread | undo / redo snapshots for the Settings editor |
//! | [`side`] | any thread | which pie the back/forward buttons follow |
//! | [`geometry`] | any thread | pie sizes and slice hit-testing (shared by tap and UI) |
//! | [`keys`] | any thread | shortcut model (key code + modifiers) and its labels |
//! | [`msg`] | any thread | messages sent *to* the UI thread |
//! | [`platform`] | any thread (some main only) | OS glue: permissions, apps, login item, keyboard layout (macOS, Windows, Linux, stub) |
//! | [`ui`] | main thread | eframe app, pie renderer, Settings window, first-launch welcome window, menu bar icon |
//! | [`about`] | - | name, version, links shown in the About tab |
//! | [`update`] | UI thread, one short-lived worker | the daily look for a newer release, the offer (About tab, sheet when Settings opens) and the install (download, checksum, swap) |
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
// tests there. macOS, Windows and Linux have a backend, so dead code is reported on them.
#![cfg_attr(
    not(any(target_os = "macos", windows, target_os = "linux")),
    allow(dead_code)
)]

pub mod about;
pub mod actions;
pub mod bridge;
pub mod cache;
pub mod config;
pub mod debug;
pub mod geometry;
pub mod guides;
pub mod history;
pub mod keys;
pub mod learn;
pub mod mouse;
pub mod msg;
pub mod platform;
pub mod running;
pub mod side;
pub mod tap;
pub mod ui;
pub mod update;
pub mod windowing;

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
    // A browser starting the native messaging host, or a switch of the browser bridge: neither
    // shows a window nor claims the single-instance lock, so this comes before everything else.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == about::SELFTEST_ARG) {
        println!("{} {}", about::APP_NAME, about::VERSION);
        return Ok(());
    }
    if bridge::host::wants_host(&args) {
        std::process::exit(bridge::host::run());
    }
    if let Some(code) = bridge::host::command_line(&args) {
        std::process::exit(code);
    }
    if platform::handle_command_line() {
        return Ok(());
    }
    // Hand over before claiming the single-instance lock, so the installed copy can take it.
    if let Some(installed) = platform::ensure_installed() {
        // The arguments go along (`--autostart` keeps its meaning) plus the switch that stops the
        // installed copy from handing over once more.
        match std::process::Command::new(&installed)
            .args(std::env::args_os().skip(1))
            .arg(about::HANDOVER_ARG)
            .spawn()
        {
            Ok(_) => return Ok(()),
            Err(e) => eprintln!("CursorPie: could not start {}: {e}", installed.display()),
        }
    }
    if !platform::single_instance() {
        return Ok(());
    }
    // Started by the login entry: let the desktop come up first (tray, taskbar, antivirus scan of
    // the new session), otherwise the tray icon or the input hooks can fail at that moment.
    if std::env::args().skip(1).any(|a| a == about::AUTOSTART_ARG) {
        platform::wait_for_login_ready();
    }
    debug::init(); // the opt-in debug log; does nothing unless it is switched on
    platform::stay_responsive(); // a hidden program must still answer the first press at once
    actions::init();
    let (tx, rx) = mpsc::channel();
    let win = geometry::window_size(config::cfg().pie_size);

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
    let viewport = if platform::OS == platform::Os::Windows {
        viewport.with_taskbar(false)
    } else {
        viewport
    };
    // the X11 window class: lets a window manager rule or a launcher recognize CursorPie
    let viewport = if platform::OS == platform::Os::Linux {
        let viewport = viewport.with_app_id("cursorpie");
        // Windows takes the icon from the .exe and macOS from the .app; on X11 the window has to
        // carry it itself, or the taskbar and Alt+Tab show a generic icon.
        match window_icon() {
            Some(icon) => viewport.with_icon(icon),
            None => viewport,
        }
    } else {
        viewport
    };
    let options = eframe::NativeOptions {
        viewport,
        event_loop_builder: force_x11(),
        ..Default::default()
    };

    eframe::run_native(
        about::APP_NAME,
        options,
        Box::new(move |cc| {
            // runs on the main thread inside the running event loop
            ui::fonts::install(&cc.egui_ctx); // Inter and the icon font, before the first frame
            platform::set_accessory_policy(); // no Dock icon
            // no OS frame: on Windows the DWM would draw a white box and a border around the pie
            if let Some(handle) = ui::app::native_handle(cc) {
                platform::clear_window_frame(handle);
            }
            platform::layout::init(); // keyboard-layout snapshot (TIS must run on the main thread)
            debug::startup_snapshot();
            let notifier = Notifier {
                tx: tx.clone(),
                ctx: cc.egui_ctx.clone(),
            };
            running::init(notifier.clone()); // before the tap: its presses ask what runs
            bridge::apply(config::cfg().browser_bridge); // the optional browser extension link
            // the very first start: decided before anything can write config.json, and before the
            // tap starts, so a trackpad that is there works from the first hold (the switch is
            // turned on for a new user who has one, see tap::touch::probe)
            let first_start = config::welcome_due();
            tap::enable_trackpad_on_first_start(first_start);
            tap::spawn(notifier.clone());
            mouse::spawn_watcher(notifier.clone());
            let tray = ui::tray::build(&notifier);
            // the very first start: say hello
            if first_start {
                let (x, y) = platform::cursor_pos().unwrap_or((400.0, 300.0));
                notifier.send(msg::Msg::OpenWelcome { x, y });
            }
            Ok(Box::new(App::new(rx, tx, win, tray)))
        }),
    )
}

/// Linux: makes winit open the window on the X server (XWayland in a Wayland session).
///
/// winit prefers its native Wayland backend whenever `WAYLAND_DISPLAY` is set, and that backend is
/// compiled in even though only eframe's `x11` feature is asked for (`accesskit_winit` enables
/// `winit/wayland` by itself). A native Wayland window can neither be placed under the pointer nor
/// stay on top, and glutin refuses to make an EGL surface for it with a transparent pixel format:
/// `Glutin(NotSupported("provided native window is not supported"))`. The pie, the X11 helpers
/// (`platform::linux::x11`) and the scale logic all assume an X window, so ask for one. Only when
/// there is no X server at all (`DISPLAY` empty: XWayland switched off) winit's own choice stays.
#[cfg(target_os = "linux")]
fn force_x11() -> Option<eframe::EventLoopBuilderHook> {
    let has_x = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty());
    if !has_x {
        return None;
    }
    Some(Box::new(|builder| {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }))
}

#[cfg(not(target_os = "linux"))]
fn force_x11() -> Option<eframe::EventLoopBuilderHook> {
    None
}

/// The application icon as pixels for the window (`_NET_WM_ICON` on X11): the same artwork as the
/// Windows and macOS icons, rounded like the other icons of a Linux desktop.
fn window_icon() -> Option<egui::IconData> {
    const PNG: &[u8] = include_bytes!("../assets/app-icon-256.png");
    let img = image::load_from_memory_with_format(PNG, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}
