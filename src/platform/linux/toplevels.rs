//! Windows as the window manager reports them, and the decision "which app is under this point".
//! Pure: the X11 backend gathers the list, this module decides.

use std::os::raw::c_long;

/// The "source indication" of a `_NET_ACTIVE_WINDOW` request (its first data word): who asks for the
/// window to be activated. A window manager applies focus-stealing prevention to an *application*
/// (`1`): while another program has the focus it does not raise the window or go to its workspace,
/// it only flashes the taskbar. It does what a *pager* (`2`) asks, because a pager stands for the
/// user. The toolkit's own `Focus` request is an application request, which is why Settings could
/// not be brought forward with it.
pub const SOURCE_PAGER: c_long = 2;

/// The five data words of a `_NET_ACTIVE_WINDOW` client message that asks, the way a pager does, for
/// a window to be activated: the source indication, a timestamp (`0`: now), the window that is
/// active now (`0`: unknown) and two unused words.
pub fn pager_request() -> [c_long; 5] {
    [SOURCE_PAGER, 0, 0, 0, 0]
}

/// The data words of the `_NET_CURRENT_DESKTOP` client message that asks for workspace `desktop`
/// to be shown (the second word is the timestamp, `0`: now).
pub fn switch_desktop_request(desktop: u32) -> [c_long; 5] {
    [desktop as c_long, 0, 0, 0, 0]
}

/// The workspace to go to before activating a window that is on `window`, when `current` is shown:
/// `None` if the window is on every workspace, already on the shown one, or the shown one is unknown.
pub fn desktop_to_switch_to(window: Option<u32>, current: Option<u32>) -> Option<u32> {
    match (window, current) {
        (Some(w), Some(c)) if w != c => Some(w),
        _ => None,
    }
}

/// One top-level window.
#[derive(Clone, Debug, PartialEq)]
pub struct Toplevel {
    /// Canonical app id of the window (see `desktop_entry`).
    pub app: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Minimized or hidden.
    pub hidden: bool,
    /// A dock, desktop, popup, tooltip, notification or CursorPie's own window.
    pub skip: bool,
    /// The workspace it is on; `None` = on all of them.
    pub desktop: Option<u32>,
}

/// Index of the window under `(px, py)` in `windows`, given **top-most first**: the first visible,
/// non-skipped window on the current workspace that contains the point.
pub fn hit_test(
    windows: &[Toplevel],
    px: f64,
    py: f64,
    current_desktop: Option<u32>,
) -> Option<usize> {
    windows.iter().position(|t| {
        !t.hidden
            && !t.skip
            && match (t.desktop, current_desktop) {
                (Some(d), Some(c)) => d == c,
                _ => true,
            }
            && px >= t.x
            && px < t.x + t.w
            && py >= t.y
            && py < t.y + t.h
    })
}

/// Whether a window that reports `names` (its canonical app id, class and instance, lower case)
/// belongs to an app that goes by `aliases` (see `Index::aliases`).
pub fn belongs_to(names: &[String], aliases: &[String]) -> bool {
    names.iter().any(|n| !n.is_empty() && aliases.contains(n))
}

/// `(instance, class)` from the bytes of a `WM_CLASS` property (two NUL-terminated strings). A
/// single string is used for both. `None` if there is nothing readable.
pub fn parse_wm_class(bytes: &[u8]) -> Option<(String, String)> {
    let mut parts = bytes
        .split(|b| *b == 0)
        .map(|p| String::from_utf8_lossy(p).trim().to_string())
        .filter(|p| !p.is_empty());
    let instance = parts.next()?;
    let class = parts.next().unwrap_or_else(|| instance.clone());
    Some((instance, class))
}

/// Whether a window with these `_NET_WM_WINDOW_TYPE` atom names is not a real application window.
pub fn is_skipped_type(type_names: &[&str]) -> bool {
    type_names.iter().any(|t| {
        matches!(
            *t,
            "_NET_WM_WINDOW_TYPE_DOCK"
                | "_NET_WM_WINDOW_TYPE_DESKTOP"
                | "_NET_WM_WINDOW_TYPE_NOTIFICATION"
                | "_NET_WM_WINDOW_TYPE_TOOLTIP"
                | "_NET_WM_WINDOW_TYPE_MENU"
                | "_NET_WM_WINDOW_TYPE_DROPDOWN_MENU"
                | "_NET_WM_WINDOW_TYPE_POPUP_MENU"
                | "_NET_WM_WINDOW_TYPE_COMBO"
                | "_NET_WM_WINDOW_TYPE_DND"
        )
    })
}

/// Whether a window is a main window of its app rather than one of its popups: its type is
/// `_NET_WM_WINDOW_TYPE_NORMAL` (or it has none, which means normal), and it is not a transient of
/// another window (`transient`: it has `WM_TRANSIENT_FOR`, which toolkits set on dialogs that did
/// not say so with a type). Dialogs, splash screens, utility windows and the like are not.
pub fn is_main_window(type_names: &[&str], transient: bool) -> bool {
    if transient {
        return false;
    }
    type_names.is_empty() || type_names.contains(&"_NET_WM_WINDOW_TYPE_NORMAL")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_or_a_splash_is_not_a_main_window() {
        assert!(is_main_window(&[], false), "no type means normal");
        assert!(is_main_window(&["_NET_WM_WINDOW_TYPE_NORMAL"], false));
        assert!(!is_main_window(&["_NET_WM_WINDOW_TYPE_DIALOG"], false));
        assert!(!is_main_window(&["_NET_WM_WINDOW_TYPE_SPLASH"], false));
        assert!(!is_main_window(&["_NET_WM_WINDOW_TYPE_UTILITY"], false));
        assert!(
            !is_main_window(&["_NET_WM_WINDOW_TYPE_NORMAL"], true),
            "a transient is a popup of another window"
        );
        assert!(!is_main_window(&[], true));
    }

    #[test]
    fn a_window_is_activated_as_a_pager_would_not_as_an_application() {
        // winit's `Focus` says 1 ("application") with no timestamp: Mutter, KWin and Openbox then
        // only flash the taskbar when another program has the focus. Only the pager's 2 is obeyed.
        let data = pager_request();
        assert_eq!(data[0], 2, "source indication");
        assert_ne!(data[0], 1, "an application request is refused");
        assert_eq!(data[1], 0, "no timestamp: now");
        assert_eq!(&data[2..], [0, 0, 0], "nothing else is set");
    }

    #[test]
    fn another_workspace_is_switched_to_before_activating() {
        assert_eq!(desktop_to_switch_to(Some(0), Some(1)), Some(0));
        assert_eq!(
            desktop_to_switch_to(Some(1), Some(1)),
            None,
            "already shown"
        );
        assert_eq!(
            desktop_to_switch_to(None, Some(1)),
            None,
            "on every workspace"
        );
        assert_eq!(
            desktop_to_switch_to(Some(2), None),
            None,
            "shown one unknown"
        );
        assert_eq!(switch_desktop_request(3), [3, 0, 0, 0, 0]);
    }

    fn win(app: &str, x: f64, y: f64, w: f64, h: f64) -> Toplevel {
        Toplevel {
            app: app.into(),
            x,
            y,
            w,
            h,
            hidden: false,
            skip: false,
            desktop: None,
        }
    }

    #[test]
    fn the_topmost_window_under_the_point_wins() {
        let list = [
            win("front", 100.0, 100.0, 200.0, 200.0),
            win("back", 0.0, 0.0, 800.0, 600.0),
        ];
        assert_eq!(hit_test(&list, 150.0, 150.0, None), Some(0));
        assert_eq!(hit_test(&list, 50.0, 50.0, None), Some(1));
        assert_eq!(hit_test(&list, 900.0, 50.0, None), None);
    }

    #[test]
    fn edges_are_half_open() {
        let list = [win("a", 10.0, 10.0, 100.0, 50.0)];
        assert_eq!(hit_test(&list, 10.0, 10.0, None), Some(0));
        assert_eq!(hit_test(&list, 110.0, 30.0, None), None);
        assert_eq!(hit_test(&list, 50.0, 60.0, None), None);
    }

    #[test]
    fn hidden_skipped_and_other_workspace_windows_are_ignored() {
        let mut hidden = win("hidden", 0.0, 0.0, 500.0, 500.0);
        hidden.hidden = true;
        let mut dock = win("dock", 0.0, 0.0, 500.0, 500.0);
        dock.skip = true;
        let mut elsewhere = win("elsewhere", 0.0, 0.0, 500.0, 500.0);
        elsewhere.desktop = Some(2);
        let mut sticky = win("sticky", 0.0, 0.0, 500.0, 500.0);
        sticky.desktop = None;
        let list = [hidden, dock, elsewhere, sticky];
        assert_eq!(hit_test(&list, 10.0, 10.0, Some(0)), Some(3));
        let on_two = [win("two", 0.0, 0.0, 9.0, 9.0)];
        assert_eq!(hit_test(&on_two, 1.0, 1.0, Some(0)), Some(0));
    }

    #[test]
    fn a_window_on_the_current_workspace_matches() {
        let mut w = win("here", 0.0, 0.0, 100.0, 100.0);
        w.desktop = Some(1);
        assert_eq!(hit_test(&[w.clone()], 5.0, 5.0, Some(1)), Some(0));
        assert_eq!(hit_test(&[w.clone()], 5.0, 5.0, Some(0)), None);
        assert_eq!(
            hit_test(&[w], 5.0, 5.0, None),
            Some(0),
            "unknown workspace: assume visible"
        );
    }

    #[test]
    fn wm_class_has_an_instance_and_a_class() {
        assert_eq!(
            parse_wm_class(b"navigator\0Firefox\0"),
            Some(("navigator".into(), "Firefox".into()))
        );
        assert_eq!(
            parse_wm_class(b"xterm\0"),
            Some(("xterm".into(), "xterm".into()))
        );
        assert_eq!(parse_wm_class(b"\0\0"), None);
        assert_eq!(parse_wm_class(b""), None);
    }

    #[test]
    fn a_window_belongs_to_an_app_when_any_name_matches() {
        let list = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let aliases = list(&["firefox", "org.mozilla.firefox"]);
        let by_id = list(&["navigator", "org.mozilla.firefox"]);
        assert!(belongs_to(&by_id, &aliases));
        assert!(belongs_to(&list(&["firefox"]), &aliases));
        assert!(!belongs_to(&list(&["chromium"]), &aliases));
        assert!(!belongs_to(&list(&[""]), &list(&[""])), "no name at all");
        assert!(!belongs_to(&[], &aliases));
    }

    #[test]
    fn docks_and_popups_are_not_apps() {
        assert!(is_skipped_type(&["_NET_WM_WINDOW_TYPE_DOCK"]));
        assert!(is_skipped_type(&[
            "_NET_WM_WINDOW_TYPE_NORMAL",
            "_NET_WM_WINDOW_TYPE_TOOLTIP"
        ]));
        assert!(!is_skipped_type(&["_NET_WM_WINDOW_TYPE_NORMAL"]));
        assert!(!is_skipped_type(&["_NET_WM_WINDOW_TYPE_DIALOG"]));
        assert!(!is_skipped_type(&[]));
    }

    #[test]
    fn desktop_request_preserves_the_protocol_value() {
        for desktop in [0, 1, 3, u32::MAX] {
            assert_eq!(switch_desktop_request(desktop)[0] as u32, desktop);
        }
    }
}
