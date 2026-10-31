//! The glue between the app and the diagnostic log.
//!
//! The log itself is the separate `diag` crate (`diag/`, with its own documentation: `cargo docs`
//! or `docs/DEBUGGING.md`). It knows nothing about CursorPie, so everything that does is here:
//! where the files live, which environment variables are copied into the first line, and the two
//! probes that need the platform layer (the monitors at start, the window position).
//!
//! The rest of the app writes log lines directly with `diag::log!` and `diag::trace!`.

use crate::{config, platform};
use diag::Value;

/// Environment variables copied into the first line: the ones that decide how a session behaves.
const ENV_REPORTED: [&str; 10] = [
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "WINIT_UNIX_BACKEND",
    "WINIT_X11_SCALE_FACTOR",
    "GDK_SCALE",
    "CURSORPIE_BACKEND",
    "CURSORPIE_NO_INSTALL",
];

/// Where the pie asked for its window. For two seconds after a request, [`observe_window`] logs
/// where the window manager really put it, which tells "the position was ignored" from "the
/// position was wrong".
static WINDOW: diag::Watch = diag::Watch::new("win", "win_obs", 2000, 12);

/// Opens the log if it is switched on (`CURSORPIE_DEBUG`, or a `debug` file next to
/// `config.json`). Call it once, early, after the single-instance check. Never fails.
pub fn init() {
    diag::Builder::new("cursorpie")
        .support_dir(config::app_support_dir())
        .version(env!("CARGO_PKG_VERSION"))
        .report_env(&ENV_REPORTED)
        .init();
}

/// The displays and the pointer as the platform reports them. Call it once the window exists
/// (inside the eframe creator).
pub fn startup_snapshot() {
    if !diag::enabled("sys") {
        return;
    }
    let rects = platform::display_rects();
    let mut event = diag::event("sys", "startup").field("monitors", rects.len());
    for (i, r) in rects.iter().enumerate() {
        event = event.field(format!("mon{i}"), diag::rect_str(r.x, r.y, r.w, r.h));
    }
    let cursor = platform::cursor_pos();
    event = event.field("cursor_ok", cursor.is_some());
    if let Some((x, y)) = cursor {
        event = event.field("cursor_x", x).field("cursor_y", y);
    }
    event.write();
}

/// Says where the window was just asked to be (top-left, in points).
pub fn expect_window(x: f32, y: f32) {
    WINDOW.expect(&[("x", f64::from(x)), ("y", f64::from(y))]);
}

/// Whether [`observe_window`] has work to do (one atomic read, for the per-frame call).
pub fn observing() -> bool {
    WINDOW.active()
}

/// Logs the window rectangle `[x, y, width, height]` (points) the toolkit reports, when it
/// changed. Call it every frame while [`observing`] is true.
pub fn observe_window(outer: Option<[f32; 4]>, pixels_per_point: Option<f32>) {
    let actual = outer.map(|[x, y, w, h]| {
        [
            ("x", f64::from(x)),
            ("y", f64::from(y)),
            ("w", f64::from(w)),
            ("h", f64::from(h)),
        ]
    });
    let extra: Vec<(&str, Value)> = pixels_per_point
        .map(|p| ("ppp", Value::from(p)))
        .into_iter()
        .collect();
    WINDOW.observe(actual.as_ref().map(|a| a.as_slice()), &extra);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_log_that_is_off_costs_nothing_and_writes_nothing() {
        // the tests never switch the log on
        assert!(!diag::enabled("ui"));
        diag::log!("ui", "never", x = 1.0);
        diag::trace!("ui", "never");
        assert!(!observing());
        expect_window(1.0, 2.0);
        assert!(!observing(), "nothing is watched while the log is off");
        observe_window(None, None);
        startup_snapshot();
    }

    #[test]
    fn the_reported_variables_are_the_ones_the_analyzer_prints() {
        for name in [
            "XDG_SESSION_TYPE",
            "XDG_CURRENT_DESKTOP",
            "DISPLAY",
            "WAYLAND_DISPLAY",
        ] {
            assert!(ENV_REPORTED.contains(&name), "{name}");
        }
    }
}
