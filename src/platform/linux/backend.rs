//! The functions every platform backend provides (see the table in `platform/mod.rs`), for Linux.
//!
//! X11 sessions use the X11 backend (`x11`); a Wayland session uses the Wayland one (`wayland`, chosen
//! once from the session, see `wayland::active`), which answers first and leaves the X11 code as the
//! fallback for what it does not know yet. This first release supports X11 sessions (and XWayland for the windows it can see). What it does
//! not do yet, and why, is listed in `docs/LINUX_MIGRATION.md`: a tray icon and keyboard-layout
//! labels fall back to the same behavior as the stub, and Wayland-native windows are invisible to
//! it.

use super::desktop_entry::Index;
use super::icontheme::{self, Found};
use super::launch::{self, Chooser, Launch};
use super::roles::{self, Role};
use super::{autostart, inputdev, wayland, x11, xdg, xlib};
use crate::config::DesktopMode;
use crate::geometry::Point;
use crate::keys::Shortcut;
use crate::platform::{FileKind, NativeIcon, ScreenRect, chooser, spawn_reaped};
use crate::tap::Kind;
use crate::windowing::{Asked, Windows};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

// ------------------------------------------------------------------------------------ apps

/// The app in front.
pub fn frontmost_bundle_id() -> Option<String> {
    // Wayland: what KWin says; until it has reported, what XWayland knows
    if wayland::active()
        && let Some(app) = wayland::kwin::frontmost()
    {
        return Some(app);
    }
    x11::conn()?.frontmost()
}

/// The app of the window under the logical point `p`.
pub fn bundle_id_at(p: Point) -> Option<String> {
    if wayland::active() && wayland::kwin::snapshot().is_some() {
        // Sway cannot say where the pointer is, so `p` would be XWayland's guess: the focused app
        // stands in (it is the one under the pointer when focus follows the pointer)
        if !wayland::kwin::knows_pointer() {
            return wayland::kwin::frontmost();
        }
        return wayland::kwin::app_at(p);
    }
    x11::conn()?.app_at(p)
}

/// The app id of a picked file (a `.desktop` file or an executable).
pub fn bundle_id_of(app_path: &str) -> Option<String> {
    launch::app_id_of_path(app_path)
}

/// The real icon of an app: the `Icon=` of its desktop entry, found in the icon themes (see
/// [`icontheme`]). `bundle` is an app id or a role such as `@browser`. A PNG is read directly; an
/// SVG needs `rsvg-convert` (see [`render_svg`]). None while the desktop entries have not been
/// scanned yet (the icon is asked for again a little later), for an app without an entry or icon,
/// and when no file can be read; the pie then shows its own glyph.
pub fn app_icon(bundle: &str) -> Option<NativeIcon> {
    let index = x11::index()?;
    let app = match Role::parse(bundle) {
        Some(role) => role_app(role, &index)?,
        None => bundle.trim().to_string(),
    };
    let Some(icon) = index.entry_for(&app).and_then(|e| e.icon.clone()) else {
        eprintln!("CursorPie: app icon for {bundle}: no desktop entry with an Icon= line");
        return None;
    };
    let home = crate::config::home();
    let data_home = std::env::var("XDG_DATA_HOME").ok();
    let data_dirs = std::env::var("XDG_DATA_DIRS").ok();
    let icon_dirs = xdg::icon_dirs(&home, data_home.as_deref(), data_dirs.as_deref());
    let pixmaps = xdg::pixmap_dirs(&home, data_home.as_deref(), data_dirs.as_deref());
    let themes = icon_themes(&home, &icon_dirs);
    let found = icontheme::find(&icon, themes, &icon_dirs, &pixmaps, icontheme::WANT);
    if found.is_empty() {
        eprintln!(
            "CursorPie: app icon for {bundle}: no PNG or SVG named {icon:?} in the icon themes"
        );
    }
    found.iter().find_map(|f| match f {
        Found::Png(path) => read_png(path),
        Found::Svg(path) => render_svg(path),
    })
}

/// The icon themes to search: the one the desktop uses, the ones it inherits from, `hicolor`. It
/// is worked out once, so a theme changed while CursorPie runs is noticed after a restart.
fn icon_themes(home: &Path, icon_dirs: &[PathBuf]) -> &'static [String] {
    static THEMES: OnceLock<Vec<String>> = OnceLock::new();
    THEMES.get_or_init(|| {
        let config = xdg::config_home(home, std::env::var("XDG_CONFIG_HOME").ok().as_deref());
        let read = |path: PathBuf| std::fs::read_to_string(path).ok();
        // KDE and GTK write the theme to a file; GNOME keeps it in gsettings (which also answers
        // on KDE, with GNOME's default), so it is asked last
        let current = read(config.join("kdeglobals"))
            .and_then(|t| icontheme::kde_theme(&t))
            .or_else(|| {
                read(config.join("gtk-3.0/settings.ini")).and_then(|t| icontheme::gtk_theme(&t))
            })
            .or_else(|| {
                let out = Command::new("gsettings")
                    .args(["get", "org.gnome.desktop.interface", "icon-theme"])
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .output()
                    .ok()?;
                if !out.status.success() {
                    return None;
                }
                icontheme::gsettings_theme(&String::from_utf8_lossy(&out.stdout))
            });
        icontheme::theme_chain(current.as_deref(), |name| {
            icon_dirs
                .iter()
                .find_map(|d| std::fs::read_to_string(d.join(name).join("index.theme")).ok())
        })
    })
}

/// The side of the biggest icon file that is read (an icon is shrunk to 128 px anyway).
const MAX_ICON_SIDE: u32 = 2048;

/// A PNG file as pixels.
fn read_png(path: &Path) -> Option<NativeIcon> {
    let (w, h) = image::image_dimensions(path).ok()?;
    if w > MAX_ICON_SIDE || h > MAX_ICON_SIDE {
        return None;
    }
    let img = image::open(path).ok()?.into_rgba8();
    Some(NativeIcon::Rgba {
        width: img.width(),
        height: img.height(),
        data: img.into_raw(),
    })
}

/// An SVG file as pixels, drawn by `rsvg-convert` (package `librsvg2-tools` on Fedora,
/// `librsvg2-bin` on Debian and Ubuntu) because CursorPie has no SVG renderer of its own. Without
/// that program an icon that exists only as SVG is not shown; the one hint says so.
fn render_svg(path: &Path) -> Option<NativeIcon> {
    static HINTED: AtomicBool = AtomicBool::new(false);
    let side = icontheme::WANT.to_string();
    let out = match Command::new("rsvg-convert")
        .args(["-w", &side, "-h", &side, "-a"])
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound && !HINTED.swap(true, Ordering::SeqCst) {
                eprintln!(
                    "CursorPie: some app icons are SVG only; install rsvg-convert \
                     (librsvg2-tools) to show them"
                );
            }
            return None;
        }
    };
    if !out.status.success() {
        return None;
    }
    let img = image::load_from_memory_with_format(&out.stdout, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    Some(NativeIcon::Rgba {
        width: img.width(),
        height: img.height(),
        data: img.into_raw(),
    })
}

/// There is no Dock to hide the app from.
pub fn set_accessory_policy() {}

/// The window manager draws no frame around an undecorated window.
pub fn clear_window_frame(_handle: isize) {}

/// X11 window managers (KWin, ...) may place a window that is mapped again at the center of the
/// screen, ignoring the position it was given while hidden, so the pie asks for it again.
pub fn window_manager_may_recenter() -> bool {
    true
}

/// The pointer position in logical points.
pub fn cursor_pos() -> Option<(f64, f64)> {
    if wayland::active()
        && let Some((_, p)) = wayland::kwin::pointer()
    {
        return Some((p.x, p.y));
    }
    x11::conn()?.pointer()
}

/// Where to ask the toolkit to put a window whose center is `(x, y)`, a point in logical points
/// like [`cursor_pos`]. In a Wayland session the pointer is KWin's and the window is an XWayland
/// one: the point is mapped from the KWin output to the X monitor of the same screen, so the pie
/// is centered on the cursor at any scale (see `wayland::coords`). X11 sessions, and a Wayland
/// session where the screens cannot be matched, keep the point as it is.
pub fn window_point(x: f64, y: f64) -> (f64, f64) {
    if wayland::active() {
        let outputs = wayland::kwin::displays();
        if !outputs.is_empty()
            && let Some(c) = x11::conn()
            && let Some(p) = wayland::coords::to_window_point((x, y), &outputs, &c.monitors())
        {
            return p;
        }
    }
    (x, y)
}

/// The monitors in logical points.
pub fn display_rects() -> Vec<ScreenRect> {
    if wayland::active() {
        let outputs = wayland::kwin::displays();
        if !outputs.is_empty() {
            return outputs;
        }
    }
    x11::conn().map(|c| c.monitors()).unwrap_or_default()
}

// ------------------------------------------------------------------------------ permission

/// X11 asks for no permission.
pub fn is_trusted() -> bool {
    true
}
/// X11 asks for no permission.
pub fn is_trusted_cached() -> bool {
    true
}
/// Nothing to open.
pub fn open_accessibility_settings() {}
/// Nothing to open.
pub fn open_input_monitoring_settings() {}

// ------------------------------------------------------------------------------- injection

/// Sends a shortcut to the window that has the keyboard focus.
pub fn press_shortcut(sc: Shortcut) {
    if wayland::active() {
        wayland::press_shortcut(sc);
        return;
    }
    if let Some(c) = x11::conn() {
        c.press_shortcut(sc);
    }
}

/// The X button number of a macOS button number (0 left, 1 right, 2 middle, 3 back, 4 forward, 5 and
/// up extra). The extra buttons follow back and forward in the order the driver numbers them: X
/// button 10, 11, 12... (`BTN_FORWARD`, `BTN_BACK`, `BTN_TASK`...), up to `mouse::MAX_BUTTON`.
pub(crate) fn x_button_for(button: i64) -> Option<i32> {
    match button {
        0 => Some(1),
        1 => Some(3),
        2 => Some(2),
        b if crate::mouse::is_extra(b) => Some(b as i32 + 5),
        _ => None,
    }
}

/// The mice the kernel lists right now, with the extra buttons each one has (see [`inputdev`]). Empty
/// when the list cannot be read (the app then relies on a scan in Settings).
pub fn mouse_devices() -> Vec<crate::mouse::Device> {
    std::fs::read_to_string("/proc/bus/input/devices")
        .map(|text| inputdev::parse_devices(&text))
        .unwrap_or_default()
}

/// Re-posts a swallowed press or release of `button`. The event goes to the current pointer
/// position, so `_pos` is not needed.
pub fn post_mouse(_pos: Point, kind: Kind, button: i64) {
    if wayland::active() {
        match kind {
            Kind::Down => wayland::inject_button(button, true),
            Kind::Up => wayland::inject_button(button, false),
            Kind::Drag => false,
        };
        return;
    }
    let Some(xb) = x_button_for(button) else {
        return;
    };
    let press = match kind {
        Kind::Down => true,
        Kind::Up => false,
        Kind::Drag => return,
    };
    if let Some(c) = x11::conn() {
        c.inject_button(xb, press);
    }
}

/// Brings the top-most window of `bundle` to the front.
pub fn activate_app(bundle: &str) {
    if wayland::active() && wayland::kwin::raise(bundle) {
        return;
    }
    if let Some(c) = x11::conn() {
        c.activate(bundle);
    }
}

/// How often [`raise_own_window`] looks for the window while the window manager does not list it.
const RAISE_STEP: Duration = Duration::from_millis(25);
/// How long [`raise_own_window`] keeps looking. A window is listed within a few milliseconds of
/// being shown; a second is for a loaded machine.
const RAISE_WAIT: Duration = Duration::from_millis(1000);
/// How long after the request the diagnostic log looks at who has the focus.
const RAISE_SETTLE: Duration = Duration::from_millis(200);

/// Set while a thread of [`raise_own_window`] still looks for the window.
static RAISE_WAITING: AtomicBool = AtomicBool::new(false);

/// Brings CursorPie's own window (Settings, or the welcome window) to the front, on whichever
/// workspace it is. Returns at once: the work is done on a thread of its own.
///
/// The toolkit's focus request cannot do this on X11: it is sent as an application, and a window
/// manager that sees another program with the focus only flashes the taskbar (see
/// [`toplevels::SOURCE_PAGER`](super::toplevels::SOURCE_PAGER)). This asks as a pager does, like
/// "Open app" does for another app's window. The window may have been shown a moment ago and not be
/// known to the window manager yet, so the request is repeated until it is listed. One thread
/// waiting is enough: a request that comes while it waits is the same request.
pub fn raise_own_window() {
    if RAISE_WAITING.swap(true, Ordering::SeqCst) {
        diag::log!("x11", "raise_own_merged");
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("cursorpie-raise".into())
        .spawn(raise_own_and_watch);
    if spawned.is_err() {
        RAISE_WAITING.store(false, Ordering::SeqCst);
    }
}

/// The body of the thread of [`raise_own_window`].
fn raise_own_and_watch() {
    let Some(conn) = x11::conn() else {
        RAISE_WAITING.store(false, Ordering::SeqCst);
        return;
    };
    let started = Instant::now();
    let mut tries = 0u32;
    let raised = loop {
        tries += 1;
        if let Some(r) = conn.raise_own() {
            break Some(r);
        }
        if started.elapsed() >= RAISE_WAIT {
            break None;
        }
        std::thread::sleep(RAISE_STEP);
    };
    RAISE_WAITING.store(false, Ordering::SeqCst);
    // what the window manager was asked, next to what the window looked like when it was asked
    diag::log!(
        "x11",
        "raise_own",
        found = raised.is_some(),
        tries = tries,
        waited_ms = started.elapsed(),
        window = raised.map(|r| r.window),
        window_desktop = raised.and_then(|r| r.window_desktop),
        current_desktop = raised.and_then(|r| r.current_desktop),
        hidden = raised.map(|r| r.hidden),
        was_active = raised.map(|r| r.was_active)
    );
    // ...and what it did about it: only worth the wait while the log is on
    if let Some(r) = raised
        && diag::enabled("x11")
    {
        std::thread::sleep(RAISE_SETTLE);
        diag::log!(
            "x11",
            "raise_own_seen",
            active_ours = conn.is_active(r.window),
            current_desktop = conn.current_desktop(),
            asked_from_desktop = r.current_desktop,
            window_desktop = r.window_desktop
        );
    }
}

// --------------------------------------------------------------------------- launching things

/// Whether `name` is an executable file somewhere on `PATH`.
fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        std::fs::metadata(dir.join(name))
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    })
}

/// A command with no input and no output, in its own process group so a signal sent to CursorPie
/// does not reach it.
fn detached(program: &str) -> Command {
    let mut c = Command::new(program);
    c.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    c
}

/// Starts `cmd` and reports whether it could be started.
fn started(cmd: &mut Command) -> bool {
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(e) => {
            diag::log!(
                "act",
                "spawn_failed",
                program = cmd.get_program().to_string_lossy().into_owned(),
                error = format!("{:?}", e.kind())
            );
            false
        }
    }
}

/// The app a role stands for on this machine right now, if it is installed. The system is asked
/// each time: the user may change the default browser at any moment.
fn role_app(role: Role, index: &Index) -> Option<String> {
    let id = match role.query() {
        Some((program, args)) => {
            let out = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            roles::app_from_query(&String::from_utf8_lossy(&out.stdout))?
        }
        // `xdg-terminal-exec` knows the user's choice better than a list does
        None if on_path("xdg-terminal-exec") => return None,
        None => roles::terminal_app(index)?,
    };
    index.find(&id).map(|e| e.id.clone())
}

/// Opens an app: a desktop id, a `.desktop` file, a program on `PATH`, a path or a role such as
/// `@browser`.
pub fn open_app(app: &str) {
    let a = app.trim();
    if a.is_empty() {
        return;
    }
    let index = x11::index().unwrap_or_else(|| std::sync::Arc::new(Index::default()));
    if let Some(role) = Role::parse(a) {
        match role_app(role, &index) {
            Some(id) => open_app(&id),
            None => run_shell(role.command()),
        }
        return;
    }
    let how = if a.ends_with(".desktop") && a.contains('/') {
        Launch::Desktop(a.to_string())
    } else {
        launch::resolve(a, &index)
    };
    match how {
        Launch::Desktop(path) => {
            let mut gio = detached("gio");
            gio.arg("launch").arg(&path);
            if !started(&mut gio) {
                spawn_reaped(detached("xdg-open").arg(&path));
            }
        }
        Launch::Program(p) => spawn_reaped(&mut detached(&p)),
    }
}

/// Opens an app, going to its workspace when it already runs on another one. The window manager
/// does the switching when it is asked to activate a window, so this raises the app's window if it
/// has one and starts the app otherwise. `mode` is `SwitchToApp` here: `DoNothing` never gets this far
/// and `BringToCurrent` is turned into `SwitchToApp` for Linux (`DesktopMode::effective`).
///
/// A role (`@browser`) is first turned into the app that fills it, so its window is found too.
pub fn reveal_app(app: &str, _mode: DesktopMode) {
    let a = app.trim();
    if a.is_empty() {
        return;
    }
    if let Some(role) = Role::parse(a) {
        let index = x11::index().unwrap_or_else(|| std::sync::Arc::new(Index::default()));
        match role_app(role, &index) {
            Some(id) => raise_or_open(&id),
            None => run_shell(role.command()),
        }
        return;
    }
    raise_or_open(a);
}

/// Opens a new window of an app that already runs: the app is started again, which is what a
/// launcher does too. An app that keeps one process (Firefox, Chromium, the terminals) answers with
/// a new window; one that cannot have two copies shows the one it has.
pub fn open_app_new_window(app: &str) -> Asked {
    open_app(app);
    Asked::Sent(None)
}

/// Whether the desktop entry of `app` says it has a single main window (`SingleMainWindow=true`),
/// so that starting it again cannot open a second one. False for an app with no entry, and for a
/// role such as `@browser` (browsers and terminals take new windows).
pub fn declares_single_window(app: &str) -> bool {
    let a = app.trim();
    if a.is_empty() || Role::parse(a).is_some() {
        return false;
    }
    x11::index()
        .and_then(|index| index.entry_for(a).map(|e| e.single_window))
        .unwrap_or(false)
}

/// For each of `apps` (as typed in "Open app" slices), whether it has a window right now, on any
/// workspace and minimized or not: the same windows [`reveal_app`] would raise. One look at the
/// window manager's list serves them all. A role (`@browser`) is first turned into the app that
/// fills it. Meant for a worker thread: it asks the X server, and a role asks the system.
pub fn running_among(apps: &[String]) -> Vec<bool> {
    let wayland_answers = wayland::active() && wayland::kwin::snapshot().is_some();
    let Some(conn) = x11::conn().filter(|_| !wayland_answers) else {
        if !wayland_answers {
            return vec![false; apps.len()];
        }
        let index = x11::index().unwrap_or_else(|| std::sync::Arc::new(Index::default()));
        let ids: Vec<Option<String>> = apps
            .iter()
            .map(|a| {
                let a = a.trim();
                match Role::parse(a) {
                    Some(role) => role_app(role, &index),
                    None => Some(a.to_string()),
                }
            })
            .collect();
        return wayland::kwin::running_among(&ids);
    };
    let index = x11::index().unwrap_or_else(|| std::sync::Arc::new(Index::default()));
    let ids: Vec<Option<String>> = apps
        .iter()
        .map(|a| {
            let a = a.trim();
            match Role::parse(a) {
                Some(role) => role_app(role, &index),
                None => Some(a.to_string()),
            }
        })
        .collect();
    conn.running_among(&ids)
}

/// How many windows `app` has right now, on any workspace, minimized or not (the windows
/// [`running_among`] looks at). `None` when the window manager cannot be asked. A role (`@browser`)
/// is first turned into the app that fills it. Meant for a worker thread.
pub fn window_count(app: &str) -> Option<usize> {
    let a = app.trim();
    if a.is_empty() {
        return None;
    }
    let index = x11::index().unwrap_or_else(|| std::sync::Arc::new(Index::default()));
    let id = match Role::parse(a) {
        Some(role) => role_app(role, &index)?,
        None => a.to_string(),
    };
    if wayland::active() && wayland::kwin::snapshot().is_some() {
        return wayland::kwin::window_count(&id);
    }
    x11::conn().map(|conn| conn.window_count(&id))
}

/// The windows of `app` told apart, for what [`window_count`] counts: where windows keep a number
/// for as long as they live, a window that was hidden and is shown again is not taken for a new
/// one (see [`crate::windowing::Baseline`]). `None` when the window manager cannot be asked. A
/// role (`@browser`) is first turned into the app that fills it. Meant for a worker thread.
pub fn window_ids(app: &str) -> Option<Windows> {
    let a = app.trim();
    if a.is_empty() {
        return None;
    }
    let index = x11::index().unwrap_or_else(|| std::sync::Arc::new(Index::default()));
    let id = match Role::parse(a) {
        Some(role) => role_app(role, &index)?,
        None => a.to_string(),
    };
    if wayland::active() && wayland::kwin::snapshot().is_some() {
        return wayland::kwin::window_ids(&id);
    }
    x11::conn().map(|conn| conn.window_ids(&id))
}

/// Raises the window of the app `a` if it has one, otherwise starts it.
fn raise_or_open(a: &str) {
    if wayland::active() && wayland::kwin::raise(a) {
        return;
    }
    if let Some(c) = x11::conn()
        && c.raise(a)
    {
        return;
    }
    open_app(a);
}

/// Raises the window of the app `a` if it has one; `false` when it has none (nothing is started).
/// Used to bring the browser that holds a page to the front (see `platform::web`).
pub fn raise_if_running(a: &str) -> bool {
    if wayland::active() && wayland::kwin::raise(a) {
        return true;
    }
    x11::conn().is_some_and(|c| c.raise(a))
}

/// Opens a link or a file with the default application.
pub fn open_url(target: &str) {
    // only the scheme is logged: the rest of a link can be personal
    diag::log!(
        "act",
        "open_url",
        scheme = target.split(':').next().unwrap_or(""),
        xdg_open = on_path("xdg-open")
    );
    spawn_reaped(detached("xdg-open").arg(target));
}

/// Opens a web link in a **new window** of the default browser: the browser's program is started
/// with `--new-window <link>` when it is one that takes it (see [`roles::new_window_args`]). Any
/// other browser, or a system that cannot say which one it is, gets the link from `xdg-open`, which
/// most browsers answer with a new tab.
///
/// Used by the choice of an "Open URL" slice (see `platform::web`).
pub fn open_url_new_window(target: &str) {
    let index = x11::index().unwrap_or_else(|| std::sync::Arc::new(Index::default()));
    let launch = role_app(Role::Browser, &index)
        .and_then(|id| index.find(&id).and_then(|e| e.exec_name.clone()))
        .and_then(|program| roles::new_window_args(&program, target).map(|args| (program, args)));
    // only the scheme is logged: the rest of a link can be personal
    diag::log!(
        "act",
        "open_url_new_window",
        scheme = target.split(':').next().unwrap_or(""),
        known_browser = launch.is_some()
    );
    if let Some((program, args)) = launch {
        let mut cmd = detached(&program);
        cmd.args(args);
        if started(&mut cmd) {
            return;
        }
    }
    spawn_reaped(detached("xdg-open").arg(target));
}

/// Runs a shell command.
pub fn run_shell(cmd: &str) {
    spawn_reaped(detached("/bin/sh").arg("-c").arg(cmd));
}

/// Opens a file or folder with the default application.
pub fn open_path(path: &Path) {
    spawn_reaped(detached("xdg-open").arg(path));
}

/// Shows `path` in the file manager: a folder is opened, a file's folder is.
pub fn reveal_in_file_manager(path: &Path) {
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"))
    };
    spawn_reaped(detached("xdg-open").arg(dir));
}

/// Asks the user for a file with `zenity` or `kdialog`. `None` if they cancel or neither exists.
pub fn choose_file(kind: FileKind, prompt: &str) -> Option<String> {
    let tool = if on_path("zenity") {
        Chooser::Zenity
    } else if on_path("kdialog") {
        Chooser::Kdialog
    } else {
        eprintln!("CursorPie: install zenity or kdialog to choose files, or type the path");
        return None;
    };
    let mut cmd = Command::new(tool.program());
    cmd.args(launch::chooser_args(tool, kind, prompt))
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    chooser::run(&mut cmd)
}

/// No haptics: only a Mac trackpad ticks.
pub fn haptic(_kind: crate::platform::HapticKind) {}

/// A soft hover sound through the sound theme, if `canberra-gtk-play` is installed. At most about
/// fifteen times a second.
pub fn tick() {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    if !*AVAILABLE.get_or_init(|| on_path("canberra-gtk-play")) {
        return;
    }
    {
        let mut last = crate::config::lock(&LAST);
        if last.is_some_and(|t| t.elapsed() < Duration::from_millis(66)) {
            return;
        }
        *last = Some(Instant::now());
    }
    spawn_reaped(detached("canberra-gtk-play").args(["-i", "button-pressed"]));
}

// ------------------------------------------------------------------------ session lifecycle

fn autostart_path() -> PathBuf {
    let home = crate::config::home();
    xdg::config_home(&home, std::env::var("XDG_CONFIG_HOME").ok().as_deref())
        .join("autostart")
        .join(autostart::FILE_NAME)
}

/// Whether the autostart entry exists and no desktop has switched it off.
pub fn launch_at_login_enabled() -> bool {
    std::fs::read_to_string(autostart_path())
        .map(|t| autostart::is_enabled(&t))
        .unwrap_or(false)
}

/// Writes or removes the autostart entry. The entry starts the program that is running now: the
/// AppImage file itself for an AppImage, not its temporary mount.
pub fn set_launch_at_login(on: bool) -> std::io::Result<()> {
    let path = autostart_path();
    if !on {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let exec = match std::env::var("APPIMAGE") {
        Ok(p) if !p.trim().is_empty() => p,
        _ => std::env::current_exe()?.to_string_lossy().into_owned(),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, autostart::contents(&exec))
}

/// Distributions install the program; there is no self-install.
pub fn ensure_installed() -> Option<PathBuf> {
    None
}

/// The one command-line switch: `--touchpad-debug` (the app does not start).
pub fn handle_command_line() -> bool {
    // `cursorpie --touchpad-debug [seconds]`: a report on the touchpad, no window
    crate::tap::touchpad_debug_from_args()
}

/// Nothing to wait for: the login entry already starts the app when the session is ready.
pub fn wait_for_login_ready() {}

/// Takes a lock file in the runtime directory. `false` if another copy holds it.
pub fn single_instance() -> bool {
    static HELD: OnceLock<std::fs::File> = OnceLock::new();
    let path = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if d.starts_with('/') => PathBuf::from(d).join("cursorpie.lock"),
        // SAFETY: `getuid` has no arguments and cannot fail.
        _ => PathBuf::from(format!("/tmp/cursorpie-{}.lock", unsafe { xlib::getuid() })),
    };
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
    else {
        return true; // cannot tell: do not stop the user from starting
    };
    use std::os::fd::AsRawFd;
    // SAFETY: the descriptor belongs to `file`, which is kept alive below.
    let locked = unsafe { xlib::flock(file.as_raw_fd(), xlib::LOCK_EX_NB) } == 0;
    if locked {
        let _ = HELD.set(file);
    }
    locked
}

/// Nothing throttles a hidden X11 program the way Windows and macOS do.
pub fn stay_responsive() {}

/// Gives freed heap pages back to the OS (glibc only). The amount is not measured.
pub fn release_unused_memory() -> usize {
    #[cfg(target_env = "gnu")]
    // SAFETY: `malloc_trim` is always safe to call.
    unsafe {
        xlib::malloc_trim(0);
    }
    0
}

/// Keyboard layout: there is no snapshot yet, so labels use the US/ANSI fallback table.
pub mod layout {
    /// No snapshot to build.
    pub fn init() {}
    /// No snapshot to build.
    pub fn refresh() {}
    /// Always `None`.
    pub fn char_for(_code: u16, _shift: bool) -> Option<String> {
        None
    }
    /// Always `None`.
    pub fn find_key(_text: &str) -> Option<(u16, u8)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_buttons_map_to_x_buttons() {
        assert_eq!(x_button_for(0), Some(1));
        assert_eq!(x_button_for(1), Some(3), "right is X button 3");
        assert_eq!(x_button_for(2), Some(2), "middle is X button 2");
        assert_eq!(x_button_for(3), Some(8), "back");
        assert_eq!(x_button_for(4), Some(9), "forward");
        assert_eq!(x_button_for(5), Some(10), "the first extra button");
        assert_eq!(x_button_for(15), Some(20), "the last one CursorPie handles");
        assert_eq!(x_button_for(16), None);
        assert_eq!(x_button_for(-1), None);
    }

    #[test]
    fn autostart_lives_in_the_config_directory() {
        let p = autostart_path();
        assert!(p.ends_with("autostart/cursorpie.desktop"), "{p:?}");
    }

    #[test]
    fn programs_on_path_are_found() {
        assert!(on_path("sh"));
        assert!(!on_path("definitely-not-a-program-cursorpie"));
    }
}
