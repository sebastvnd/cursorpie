//! The Hyprland adapter: the pointer, the windows, the outputs and "bring a window forward", asked of
//! Hyprland's own control socket. Hyprland is the one wlroots-style compositor that can say where
//! the pointer is, so everything works, as on Plasma.
//!
//! **How it is asked.** Hyprland listens on `$XDG_RUNTIME_DIR/hypr/<signature>/.socket.sock`
//! (`<signature>` is the value of `HYPRLAND_INSTANCE_SIGNATURE`). A request is one line written to a
//! new connection, `[flags]/command args`; `j` as a flag asks for JSON (`j/clients`, `j/monitors`,
//! `j/activewindow`, `j/cursorpos`). The reply is written and the connection is closed. Hyprland
//! answers a connection **synchronously**: one that is left open freezes the compositor for five
//! seconds, so every request here opens a connection, writes, reads to the end and drops it, with a
//! timeout (the wiki of Hyprland says the same: "open the socket immediately before writing and
//! close it afterward").
//!
//! **What is pure** (built and tested on every OS): the socket path, the parsers of the four
//! replies, the conversion to a [`Snapshot`] and the command that focuses a window. **What is not**
//! (Linux only, `hyprland_io.rs`): the sockets and the reader that `poller.rs` runs on a thread.
//!
//! **Not run.** Written from the Hyprland wiki and from the JSON `hyprctl -j` prints; the fields it
//! relies on are the ones listed in [`parse_clients`] and [`parse_monitors`]. A field that is
//! missing leaves its part empty and the rest is kept.

use super::adapter::{bool_of, f64_of, i64_of, pair_of, str_of};
use super::kwin::{self, MAX_OUTPUTS, MAX_WINDOWS, Snapshot, WinInfo};
use crate::geometry::Point;
use crate::platform::ScreenRect;
use serde_json::Value;
use std::path::PathBuf;

/// The largest reply that is read at all. A list of a few hundred windows is some hundred
/// kilobytes.
pub(super) const MAX_REPLY: usize = 2 * 1024 * 1024;

// ------------------------------------------------------------------------------- the socket path

/// `HYPRLAND_INSTANCE_SIGNATURE` as it is used in a path: letters, digits, `_` and `-` only, so a
/// value can never walk out of the directory (`..`, a slash).
fn valid_signature(sig: &str) -> bool {
    (1..=128).contains(&sig.len())
        && sig
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
}

/// Where the control socket may be, most likely first: Hyprland 0.40 and newer keep it in the
/// runtime directory, older ones in `/tmp/hypr`. Empty when the signature is not set or not safe.
pub(crate) fn socket_paths(get: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let Some(sig) = get("HYPRLAND_INSTANCE_SIGNATURE")
        .map(|s| s.trim().to_string())
        .filter(|s| valid_signature(s))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(dir) = get("XDG_RUNTIME_DIR").filter(|d| d.starts_with('/')) {
        out.push(
            PathBuf::from(dir)
                .join("hypr")
                .join(&sig)
                .join(".socket.sock"),
        );
    }
    out.push(PathBuf::from("/tmp/hypr").join(&sig).join(".socket.sock"));
    out
}

// ------------------------------------------------------------------------------- what it says

/// One window as `j/clients` describes it, after checking.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Client {
    /// The address without its `0x` (what `focus_command` is given back). Hex digits.
    pub id: String,
    pub class: String,
    pub initial_class: String,
    pub rect: ScreenRect,
    /// The workspace it is on (special workspaces have negative numbers).
    pub workspace: i64,
    pub floating: bool,
    pub fullscreen: bool,
    /// Hyprland's own flag: a window that is hidden, for instance behind another in a group.
    pub hidden: bool,
    /// `focusHistoryID`: 0 is the window focused most recently.
    pub focus_rank: i64,
}

/// One screen as `j/monitors` describes it, after checking.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Monitor {
    /// Position and size in logical points (the size Hyprland prints is in pixels, divided by the
    /// scale here).
    pub rect: ScreenRect,
    /// The workspaces it shows: the normal one and the special one (the scratchpad), if open.
    pub shown: Vec<i64>,
}

/// The address Hyprland writes (`0x5632a5a7a5c0`) as the hex digits after the `0x`. `None` for
/// anything else, so nothing but hex can ever become part of a command.
fn address_id(address: &str) -> Option<String> {
    let id = address.strip_prefix("0x")?;
    ((1..=32).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| id.to_ascii_lowercase())
}

/// The windows of a `j/clients` reply. Reads `address`, `mapped`, `hidden`, `at`, `size`,
/// `workspace.id`, `floating`, `class`, `initialClass`, `fullscreen` and `focusHistoryID`. Windows
/// that are not mapped, have no usable address or have a rectangle that does not pass the checks of
/// `kwin::rect_of` are left out. `None` if the text is not a JSON array or is too large.
pub(crate) fn parse_clients(text: &str) -> Option<Vec<Client>> {
    if text.len() > MAX_REPLY {
        return None;
    }
    let v: Value = serde_json::from_str(text).ok()?;
    let list = v.as_array()?;
    Some(
        list.iter()
            .take(MAX_WINDOWS)
            .filter(|c| bool_of(c, "mapped") != Some(false))
            .filter_map(|c| {
                let (x, y) = pair_of(c, "at")?;
                let (w, h) = pair_of(c, "size")?;
                Some(Client {
                    id: address_id(&str_of(c, "address"))?,
                    class: kwin::clip(&str_of(c, "class")),
                    initial_class: kwin::clip(&str_of(c, "initialClass")),
                    rect: kwin::rect_of(x, y, w, h)?,
                    workspace: c
                        .get("workspace")
                        .and_then(|ws| i64_of(ws, "id"))
                        .unwrap_or(0),
                    floating: bool_of(c, "floating").unwrap_or(false),
                    // a number in current Hyprland (2 is full screen), a boolean in old ones
                    fullscreen: i64_of(c, "fullscreen").is_some_and(|f| f >= 2)
                        || bool_of(c, "fullscreen") == Some(true),
                    hidden: bool_of(c, "hidden").unwrap_or(false),
                    focus_rank: i64_of(c, "focusHistoryID").unwrap_or(0),
                })
            })
            .collect(),
    )
}

/// The screens of a `j/monitors` reply. Reads `x`, `y`, `width`, `height`, `scale`, `disabled`,
/// `activeWorkspace.id` and `specialWorkspace.id`. `width` and `height` are pixels; the position
/// and the windows' `at` are in the layout's logical points, so the size is divided by `scale`.
/// (Whether a rotated screen prints its size before or after the rotation is not checked: the size
/// is only used to tell the screens apart and to keep the pie on one.)
pub(crate) fn parse_monitors(text: &str) -> Option<Vec<Monitor>> {
    if text.len() > MAX_REPLY {
        return None;
    }
    let v: Value = serde_json::from_str(text).ok()?;
    let list = v.as_array()?;
    Some(
        list.iter()
            .take(MAX_OUTPUTS)
            .filter(|m| bool_of(m, "disabled") != Some(true))
            .filter_map(|m| {
                let scale = f64_of(m, "scale")
                    .filter(|s| s.is_finite() && *s > 0.0)
                    .unwrap_or(1.0);
                let rect = kwin::rect_of(
                    f64_of(m, "x")?,
                    f64_of(m, "y")?,
                    f64_of(m, "width")? / scale,
                    f64_of(m, "height")? / scale,
                )
                .filter(|r| r.w > 0.0 && r.h > 0.0)?;
                // the workspace id 0 of a closed special workspace is not one
                let shown = ["activeWorkspace", "specialWorkspace"]
                    .iter()
                    .filter_map(|k| m.get(*k).and_then(|ws| i64_of(ws, "id")))
                    .filter(|id| *id != 0)
                    .collect();
                Some(Monitor { rect, shown })
            })
            .collect(),
    )
}

/// The window with the keyboard focus, from a `j/activewindow` reply: its id. `None` when no window
/// has the focus (Hyprland then prints an empty object) or the reply is not what is expected.
pub(crate) fn parse_active(text: &str) -> Option<String> {
    if text.len() > MAX_REPLY {
        return None;
    }
    let v: Value = serde_json::from_str(text).ok()?;
    address_id(&str_of(&v, "address"))
}

/// The pointer from a `j/cursorpos` reply (`{"x": 120, "y": 340}`, in the layout's logical
/// points). `None` if it is not that or a number is out of range.
pub(crate) fn parse_cursor(text: &str) -> Option<Point> {
    if text.len() > 256 {
        return None;
    }
    let v: Value = serde_json::from_str(text).ok()?;
    let (x, y) = (f64_of(&v, "x")?, f64_of(&v, "y")?);
    (kwin::finite_in_range(x) && kwin::finite_in_range(y)).then(|| Point::new(x, y))
}

/// What the replies add up to: the windows top-most first, which of them are on a screen right now,
/// and the screens.
///
/// A window is **hidden** (for the hit test; "bring forward" still finds it) when its workspace is
/// not the one a screen shows, or Hyprland says it is hidden. Hyprland has one workspace per screen
/// and no single "current desktop", so every window has `desktop: None` and the screens' workspaces
/// decide instead.
///
/// The order, top-most first: a window in full screen covers everything, then the floating ones
/// (above the tiled), each group by how recently it was focused (`focusHistoryID`): focusing a
/// floating window raises it. This is the order Hyprland draws in as far as its documentation says;
/// it does not print a stacking list.
pub(crate) fn build_snapshot(
    mut clients: Vec<Client>,
    monitors: &[Monitor],
    pointer: Option<Point>,
    active: Option<String>,
) -> Snapshot {
    clients.sort_by_key(|c| (!c.fullscreen, !c.floating, c.focus_rank));
    let on_screen = |ws: i64| monitors.is_empty() || monitors.iter().any(|m| m.shown.contains(&ws));
    let windows = clients
        .into_iter()
        .map(|c| WinInfo {
            hidden: c.hidden || !on_screen(c.workspace),
            id: c.id,
            instance: c.initial_class,
            class: c.class,
            // Hyprland does not know the desktop entry: the id comes from the class
            desktop_file: String::new(),
            rect: c.rect,
            skip: false,
            desktop: None,
        })
        .collect();
    Snapshot {
        pointer,
        active: active.filter(|a| kwin::valid_id(a)),
        desktop: None,
        windows,
        outputs: monitors.iter().map(|m| m.rect).collect(),
    }
}

/// The request that gives the focus to the window `id` (hex digits), which also takes the screen
/// to its workspace. `None` for anything that is not an id, so nothing else is ever sent.
pub(crate) fn focus_command(id: &str) -> Option<String> {
    ((1..=32).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("/dispatch focuswindow address:0x{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    // The shape `hyprctl clients -j` prints (fields the adapter does not read are left in).
    const CLIENTS: &str = r#"[
      {"address":"0x5632a5a7a5c0","mapped":true,"hidden":false,"at":[12,36],"size":[1896,1032],
       "workspace":{"id":1,"name":"1"},"floating":false,"monitor":0,"class":"kitty",
       "title":"~","initialClass":"kitty","initialTitle":"kitty","pid":1234,"xwayland":false,
       "pinned":false,"fullscreen":0,"fullscreenClient":0,"grouped":[],"focusHistoryID":1},
      {"address":"0x5632a5a90f10","mapped":true,"hidden":false,"at":[400,200],"size":[800,600],
       "workspace":{"id":1,"name":"1"},"floating":true,"monitor":0,"class":"org.mozilla.firefox",
       "title":"x","initialClass":"firefox","initialTitle":"x","pid":99,"xwayland":false,
       "pinned":false,"fullscreen":0,"focusHistoryID":0},
      {"address":"0x5632a5aa1200","mapped":true,"hidden":false,"at":[0,0],"size":[1920,1080],
       "workspace":{"id":2,"name":"2"},"floating":false,"monitor":1,"class":"Steam",
       "initialClass":"Steam","fullscreen":2,"focusHistoryID":2},
      {"address":"0x5632a5aa9999","mapped":false,"at":[0,0],"size":[10,10],
       "workspace":{"id":1,"name":"1"},"class":"gone"}
    ]"#;

    const MONITORS: &str = r#"[
      {"id":0,"name":"DP-1","width":3840,"height":2160,"x":0,"y":0,
       "activeWorkspace":{"id":1,"name":"1"},"specialWorkspace":{"id":0,"name":""},
       "scale":2.00,"transform":0,"focused":true,"disabled":false},
      {"id":1,"name":"HDMI-A-1","width":1920,"height":1080,"x":1920,"y":0,
       "activeWorkspace":{"id":3,"name":"3"},"specialWorkspace":{"id":-99,"name":"special:s"},
       "scale":1.00,"transform":0,"focused":false,"disabled":false},
      {"id":2,"name":"off","width":1920,"height":1080,"x":0,"y":0,
       "activeWorkspace":{"id":9,"name":"9"},"scale":1.0,"disabled":true}
    ]"#;

    #[test]
    fn the_socket_is_in_the_runtime_directory_then_in_tmp() {
        let paths = socket_paths(&env(&[
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc123_1700000000_42"),
            ("XDG_RUNTIME_DIR", "/run/user/1000"),
        ]));
        assert_eq!(
            paths,
            [
                PathBuf::from("/run/user/1000/hypr/abc123_1700000000_42/.socket.sock"),
                PathBuf::from("/tmp/hypr/abc123_1700000000_42/.socket.sock"),
            ]
        );
    }

    #[test]
    fn a_signature_that_could_leave_the_directory_is_refused() {
        for bad in ["", "..", "../etc", "a/b", "a b", "a\nb", "x;y"] {
            let paths = socket_paths(&env(&[
                ("HYPRLAND_INSTANCE_SIGNATURE", bad),
                ("XDG_RUNTIME_DIR", "/run/user/1000"),
            ]));
            assert!(paths.is_empty(), "{bad:?}");
        }
        assert!(socket_paths(&env(&[("XDG_RUNTIME_DIR", "/run/user/1000")])).is_empty());
        // a relative runtime directory is not used, the fixed one is still tried
        let paths = socket_paths(&env(&[
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc"),
            ("XDG_RUNTIME_DIR", "run/user"),
        ]));
        assert_eq!(paths, [PathBuf::from("/tmp/hypr/abc/.socket.sock")]);
    }

    #[test]
    fn the_windows_are_read_and_an_unmapped_one_is_left_out() {
        let c = parse_clients(CLIENTS).unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].id, "5632a5a7a5c0");
        assert_eq!(c[0].class, "kitty");
        assert_eq!(
            c[0].rect,
            ScreenRect {
                x: 12.0,
                y: 36.0,
                w: 1896.0,
                h: 1032.0
            }
        );
        assert!(c[1].floating && !c[0].floating);
        assert_eq!(c[1].initial_class, "firefox");
        assert!(c[2].fullscreen && !c[0].fullscreen);
        assert_eq!(c[2].workspace, 2);
        assert_eq!(c[1].focus_rank, 0);
    }

    #[test]
    fn what_is_odd_in_a_reply_is_left_out_not_fatal() {
        // an address that is not hex, a rectangle that is out of range, a null class, a boolean
        // where a number is expected: only the first two windows are lost
        let text = r#"[
          {"address":"0xZZ","at":[0,0],"size":[1,1]},
          {"address":"0xabc","at":[1e12,0],"size":[1,1]},
          {"address":"0xdef","at":[0,0],"size":[5,5],"class":null,"fullscreen":true,"workspace":null}
        ]"#;
        let c = parse_clients(text).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].id, "def");
        assert_eq!(c[0].class, "");
        assert!(c[0].fullscreen, "an old Hyprland prints a boolean");
        assert_eq!(c[0].workspace, 0);
        assert!(parse_clients("not json").is_none());
        assert!(parse_clients("{}").is_none(), "an object is not a list");
        assert!(parse_clients(&" ".repeat(MAX_REPLY + 1)).is_none());
        assert_eq!(parse_clients("[]").unwrap().len(), 0);
    }

    #[test]
    fn the_screens_are_in_logical_points_with_the_workspaces_they_show() {
        let m = parse_monitors(MONITORS).unwrap();
        assert_eq!(m.len(), 2, "a disabled screen is not one");
        assert_eq!(
            m[0].rect,
            ScreenRect {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0
            },
            "3840x2160 at scale 2"
        );
        assert_eq!(m[0].shown, [1]);
        assert_eq!(m[1].shown, [3, -99], "the open special workspace counts");
        assert_eq!(m[1].rect.x, 1920.0);
    }

    #[test]
    fn a_missing_or_silly_scale_is_one() {
        for scale in ["", r#","scale":0"#, r#","scale":-2"#, r#","scale":null"#] {
            let text = format!(
                r#"[{{"x":0,"y":0,"width":1600,"height":900{scale},"activeWorkspace":{{"id":1}}}}]"#
            );
            let m = parse_monitors(&text).unwrap();
            assert_eq!(m[0].rect.w, 1600.0, "{scale}");
        }
    }

    #[test]
    fn the_active_window_and_the_pointer() {
        assert_eq!(
            parse_active(r#"{"address":"0x5632A5A7A5C0","class":"kitty"}"#).as_deref(),
            Some("5632a5a7a5c0"),
            "lower case, so it equals the id of the list"
        );
        assert_eq!(parse_active("{}"), None, "no window has the focus");
        assert_eq!(parse_active("Invalid"), None);
        assert_eq!(
            parse_cursor(r#"{"x": 1234, "y": 567}"#),
            Some(Point::new(1234.0, 567.0))
        );
        assert_eq!(
            parse_cursor(r#"{"x": -5.5, "y": 0}"#),
            Some(Point::new(-5.5, 0.0))
        );
        for bad in [r#"{"x":1}"#, "{}", "garbage", r#"{"x":1e12,"y":0}"#, ""] {
            assert_eq!(parse_cursor(bad), None, "{bad:?}");
        }
    }

    fn snapshot() -> Snapshot {
        build_snapshot(
            parse_clients(CLIENTS).unwrap(),
            &parse_monitors(MONITORS).unwrap(),
            Some(Point::new(500.0, 300.0)),
            Some("5632a5a7a5c0".to_string()),
        )
    }

    #[test]
    fn the_order_is_full_screen_then_floating_then_tiled() {
        let s = snapshot();
        let classes: Vec<&str> = s.windows.iter().map(|w| w.class.as_str()).collect();
        assert_eq!(classes, ["Steam", "org.mozilla.firefox", "kitty"]);
    }

    #[test]
    fn a_window_on_a_workspace_no_screen_shows_is_hidden_but_still_listed() {
        let s = snapshot();
        let by = |class: &str| s.windows.iter().find(|w| w.class == class).unwrap();
        assert!(!by("kitty").hidden, "workspace 1 is on DP-1");
        assert!(!by("org.mozilla.firefox").hidden);
        assert!(by("Steam").hidden, "workspace 2 is on no screen");
        assert!(s.windows.iter().all(|w| w.desktop.is_none() && !w.skip));
    }

    #[test]
    fn the_hit_test_finds_the_floating_window_over_the_tiled_one() {
        let s = snapshot();
        // (500, 300) is inside the floating Firefox and the tiled kitty; Steam is hidden
        assert_eq!(
            s.app_at(Point::new(500.0, 300.0), None).as_deref(),
            Some("org.mozilla.firefox")
        );
        assert_eq!(
            s.app_at(Point::new(50.0, 50.0), None).as_deref(),
            Some("kitty")
        );
        assert_eq!(s.frontmost(None).as_deref(), Some("kitty"));
        assert_eq!(s.pointer, Some(Point::new(500.0, 300.0)));
        assert_eq!(s.outputs.len(), 2);
    }

    #[test]
    fn a_hidden_window_can_still_be_brought_forward_by_its_app() {
        let s = snapshot();
        let w = s.window_of("steam", None).expect("found by its class");
        assert_eq!(w.id, "5632a5aa1200");
        assert_eq!(
            focus_command(&w.id).as_deref(),
            Some("/dispatch focuswindow address:0x5632a5aa1200")
        );
    }

    #[test]
    fn without_a_list_of_screens_every_window_counts_as_shown() {
        let s = build_snapshot(parse_clients(CLIENTS).unwrap(), &[], None, None);
        assert!(s.windows.iter().all(|w| !w.hidden));
        assert_eq!(s.active, None);
        assert!(s.outputs.is_empty());
    }

    #[test]
    fn an_id_that_is_not_hex_never_becomes_a_command() {
        let long = "a".repeat(33);
        for bad in ["", "xyz", "12 34", "12;34", "0x12", "$(id)", long.as_str()] {
            assert_eq!(focus_command(bad), None, "{bad:?}");
        }
        assert_eq!(
            focus_command("DEADbeef").as_deref(),
            Some("/dispatch focuswindow address:0xDEADbeef")
        );
    }

    #[test]
    fn the_active_id_is_checked_like_every_other() {
        let s = build_snapshot(Vec::new(), &[], None, Some("not hex!".to_string()));
        assert_eq!(s.active, None);
    }
}
