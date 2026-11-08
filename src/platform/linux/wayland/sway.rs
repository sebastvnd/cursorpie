//! The Sway adapter: the windows, the outputs, which window has the focus, and "bring a window
//! forward", asked of Sway's IPC socket (the i3 protocol, `sway-ipc(7)`).
//!
//! **Sway cannot say where the pointer is.** Its IPC has no call for it (there is one to *move* the
//! cursor, `seat <name> cursor set`, and none to read it), and a Wayland client is not told the
//! position of a pointer that is not over its own surface. So this adapter reports no pointer
//! (`adapter::Adapter::knows_pointer` is `false`) and two things follow, both said in Settings:
//!
//! * "the app under the cursor" is the focused app (`backend::bundle_id_at`). That is the app under
//!   the cursor in Sway's default, `focus_follows_mouse yes`, and wrong with `no`;
//! * the pie may open away from the cursor: where it opens comes from XWayland, which only knows
//!   the pointer while it is over an X11 window.
//!
//! A way that would work on Sway (a transparent layer-shell surface that reads the pointer when it
//! enters, as `wl-find-cursor` does) needs a Wayland client in this program, which it does not have
//! yet (`docs/WAYLAND_SUPPORT.md`, milestone W3).
//!
//! **How it is asked.** `$SWAYSOCK` is a Unix socket. A message is `i3-ipc`, the length of the
//! payload and the message type (both 32 bits, in the byte order of the machine), then the payload;
//! the reply has the same header. `GET_TREE` (4) returns the whole layout as one JSON tree: the
//! outputs, their workspaces, and the windows, each with its rectangle in the layout's logical
//! points; `RUN_COMMAND` (0) runs a Sway command (`[con_id=12] focus`).
//!
//! **Not run.** Written from `sway-ipc(7)` and the JSON `swaymsg -t get_tree` prints.

use super::adapter::{bool_of, f64_of, i64_of, str_of};
use super::kwin::{self, MAX_WINDOWS, Snapshot, WinInfo};
use crate::platform::ScreenRect;
use serde_json::Value;
use std::path::PathBuf;

/// The start of every message and reply.
const MAGIC: &[u8] = b"i3-ipc";
/// Magic, payload length, message type.
pub(crate) const HEADER_LEN: usize = 6 + 4 + 4;
/// `RUN_COMMAND`: the payload is a Sway command.
pub(crate) const RUN_COMMAND: u32 = 0;
/// `GET_TREE`: the reply is the layout tree.
pub(crate) const GET_TREE: u32 = 4;
/// The largest reply that is read at all.
const MAX_PAYLOAD: usize = 4 * 1024 * 1024;
/// Deepest nesting walked, and most nodes looked at: a tree is a few levels deep and some hundred
/// nodes wide, so these only stop a reply that is not one.
const MAX_DEPTH: usize = 40;
const MAX_NODES: usize = 8192;

// ---------------------------------------------------------------------------------- the protocol

/// A message: the header and the payload.
pub(crate) fn encode(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(payload);
    out
}

/// `(payload length, type)` of a header. `None` if it does not start with the magic or announces a
/// payload larger than is read.
pub(crate) fn parse_header(head: &[u8; HEADER_LEN]) -> Option<(usize, u32)> {
    if &head[..6] != MAGIC {
        return None;
    }
    let len = u32::from_ne_bytes([head[6], head[7], head[8], head[9]]) as usize;
    let kind = u32::from_ne_bytes([head[10], head[11], head[12], head[13]]);
    (len <= MAX_PAYLOAD).then_some((len, kind))
}

/// The socket of this session: `SWAYSOCK`, if it is an absolute path.
pub(crate) fn socket_path(get: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    get("SWAYSOCK")
        .map(|p| p.trim().to_string())
        .filter(|p| p.starts_with('/'))
        .map(PathBuf::from)
}

/// The Sway command that gives the focus to the window `id` (its `con_id`, decimal digits), which
/// also takes the screen to its workspace. `None` for anything that is not such an id, so nothing
/// else is ever sent.
pub(crate) fn focus_command(id: &str) -> Option<String> {
    ((1..=20).contains(&id.len()) && id.chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("[con_id={id}] focus"))
}

// ------------------------------------------------------------------------------------ the tree

/// What the walk over the tree collects.
#[derive(Default)]
struct Walk {
    fullscreen: Vec<WinInfo>,
    floating: Vec<WinInfo>,
    tiled: Vec<WinInfo>,
    outputs: Vec<ScreenRect>,
    focused: Option<String>,
    seen: usize,
}

/// A rectangle object (`{"x": 0, "y": 0, "width": 1920, "height": 1080}`) at `key`.
fn rect_at(node: &Value, key: &str) -> Option<ScreenRect> {
    let r = node.get(key)?;
    kwin::rect_of(
        f64_of(r, "x")?,
        f64_of(r, "y")?,
        f64_of(r, "width")?,
        f64_of(r, "height")?,
    )
}

/// Whether the node is a window and not a container, a workspace or an output: only a window has
/// an `app_id` (a native Wayland one, `null` when it set none) or `window_properties` (an X11 one).
fn is_window(node: &Value) -> bool {
    node.get("app_id").is_some_and(|v| !v.is_null())
        || node.get("window_properties").is_some_and(Value::is_object)
}

/// The window a node describes, if it is one that passes the checks.
fn read_window(node: &Value) -> Option<WinInfo> {
    let id = i64_of(node, "id")?.to_string();
    let props = node.get("window_properties");
    let prop = |key: &str| props.map(|p| str_of(p, key)).unwrap_or_default();
    let app_id = str_of(node, "app_id");
    // a native window names itself with its app id, an X11 one with its class and instance
    let class = if app_id.is_empty() {
        prop("class")
    } else {
        app_id.clone()
    };
    let instance = {
        let i = prop("instance");
        if i.is_empty() { app_id } else { i }
    };
    Some(WinInfo {
        id: Some(id).filter(|i| kwin::valid_id(i))?,
        class: kwin::clip(&class),
        instance: kwin::clip(&instance),
        desktop_file: String::new(),
        rect: rect_at(node, "rect")?,
        hidden: !bool_of(node, "visible").unwrap_or(true),
        skip: false,
        secondary: false,
        desktop: None,
    })
}

fn walk(node: &Value, floating: bool, depth: usize, acc: &mut Walk) {
    if depth > MAX_DEPTH || acc.seen >= MAX_NODES {
        return;
    }
    acc.seen += 1;
    let kind = str_of(node, "type");
    if kind == "output"
        && let Some(r) = rect_at(node, "rect").filter(|r| r.w > 0.0 && r.h > 0.0)
    {
        // the pseudo output `__i3` that holds the scratchpad has no size, so it is not listed here
        acc.outputs.push(r);
    }
    let floating = floating || kind == "floating_con";
    if is_window(node)
        && let Some(w) = read_window(node)
    {
        if bool_of(node, "focused") == Some(true) {
            acc.focused = Some(w.id.clone());
        }
        let full = i64_of(node, "fullscreen_mode").is_some_and(|m| m >= 1);
        if full {
            acc.fullscreen.push(w);
        } else if floating {
            acc.floating.push(w);
        } else {
            acc.tiled.push(w);
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(Value::as_array) {
            for child in children {
                walk(child, floating || key == "floating_nodes", depth + 1, acc);
            }
        }
    }
}

/// The layout of a `GET_TREE` reply as a snapshot: the windows top-most first, the focused window,
/// the outputs. `None` if the text is too large or is not a tree.
///
/// The order, top-most first: windows in full screen, the floating windows (Sway draws the last of
/// a workspace's floating list on top, so they are reversed), then the tiled ones. A window that is
/// not shown (another workspace, a tab behind the front one, the scratchpad) has `visible: false`
/// and is `hidden`: the hit test skips it and "bring forward" still finds it. There is no pointer
/// in the snapshot (see the top of this file).
pub(crate) fn parse_tree(text: &str) -> Option<Snapshot> {
    if text.len() > MAX_PAYLOAD {
        return None;
    }
    let root: Value = serde_json::from_str(text).ok()?;
    if !root.is_object() {
        return None;
    }
    let mut acc = Walk::default();
    walk(&root, false, 0, &mut acc);
    let mut windows = acc.fullscreen;
    windows.extend(acc.floating.into_iter().rev());
    windows.extend(acc.tiled);
    windows.truncate(MAX_WINDOWS);
    Some(Snapshot {
        pointer: None,
        active: acc.focused,
        desktop: None,
        windows,
        outputs: acc.outputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        }
    }

    // The shape `swaymsg -t get_tree -r` prints, cut down to what matters: the scratchpad's pseudo
    // output, two real ones, a tiled native window, a tiled X11 window on a workspace that is not
    // shown, a floating native window, a full-screen one, and a split container (not a window).
    const TREE: &str = r#"{
      "id":1,"type":"root","name":"root","rect":{"x":0,"y":0,"width":3840,"height":1080},
      "focused":false,"nodes":[
        {"id":2,"type":"output","name":"__i3","rect":{"x":0,"y":0,"width":0,"height":0},
         "nodes":[{"id":3,"type":"workspace","name":"__i3_scratch","nodes":[],
           "floating_nodes":[
             {"id":30,"type":"floating_con","app_id":"scratch","visible":false,
              "rect":{"x":100,"y":100,"width":500,"height":400},"focused":false}]}]},
        {"id":4,"type":"output","name":"DP-1","rect":{"x":0,"y":0,"width":1920,"height":1080},
         "nodes":[
           {"id":5,"type":"workspace","name":"1","rect":{"x":0,"y":0,"width":1920,"height":1080},
            "nodes":[
              {"id":6,"type":"con","layout":"splith","rect":{"x":0,"y":0,"width":1920,"height":1080},
               "nodes":[
                 {"id":7,"type":"con","app_id":"foot","pid":10,"visible":true,"focused":true,
                  "shell":"xdg_shell","fullscreen_mode":0,
                  "rect":{"x":0,"y":0,"width":960,"height":1080}},
                 {"id":8,"type":"con","app_id":"firefox","visible":true,"focused":false,
                  "rect":{"x":960,"y":0,"width":960,"height":1080}}]}],
            "floating_nodes":[
              {"id":9,"type":"floating_con","app_id":"pavucontrol","visible":true,"focused":false,
               "rect":{"x":300,"y":200,"width":600,"height":400}},
              {"id":10,"type":"floating_con","app_id":"mpv","visible":true,"focused":false,
               "rect":{"x":400,"y":250,"width":600,"height":400}}]},
           {"id":11,"type":"workspace","name":"2","rect":{"x":0,"y":0,"width":1920,"height":1080},
            "nodes":[
              {"id":12,"type":"con","app_id":null,"visible":false,"focused":false,
               "window_properties":{"class":"Steam","instance":"steam","title":"Steam"},
               "rect":{"x":0,"y":0,"width":1920,"height":1080}}],
            "floating_nodes":[]}]},
        {"id":13,"type":"output","name":"HDMI-A-1","rect":{"x":1920,"y":0,"width":1920,"height":1080},
         "nodes":[
           {"id":14,"type":"workspace","name":"3","rect":{"x":1920,"y":0,"width":1920,"height":1080},
            "nodes":[
              {"id":15,"type":"con","app_id":"mupdf","visible":true,"focused":false,
               "fullscreen_mode":1,"rect":{"x":1920,"y":0,"width":1920,"height":1080}}],
            "floating_nodes":[]}]}]}"#;

    #[test]
    fn a_message_has_the_magic_the_length_and_the_type() {
        let m = encode(GET_TREE, b"abc");
        assert_eq!(m.len(), HEADER_LEN + 3);
        assert_eq!(&m[..6], b"i3-ipc");
        assert_eq!(u32::from_ne_bytes(m[6..10].try_into().unwrap()), 3);
        assert_eq!(u32::from_ne_bytes(m[10..14].try_into().unwrap()), 4);
        assert_eq!(&m[14..], b"abc");
        let empty = encode(RUN_COMMAND, b"");
        assert_eq!(empty.len(), HEADER_LEN);
    }

    #[test]
    fn a_header_is_read_back_and_a_bad_one_is_refused() {
        let m = encode(RUN_COMMAND, b"focus");
        let head: [u8; HEADER_LEN] = m[..HEADER_LEN].try_into().unwrap();
        assert_eq!(parse_header(&head), Some((5, RUN_COMMAND)));
        let mut wrong = head;
        wrong[0] = b'x';
        assert_eq!(parse_header(&wrong), None, "not the magic");
        let mut huge = head;
        huge[6..10].copy_from_slice(&((MAX_PAYLOAD as u32) + 1).to_ne_bytes());
        assert_eq!(parse_header(&huge), None, "larger than is read");
    }

    #[test]
    fn the_socket_is_swaysock_when_it_is_an_absolute_path() {
        let sock = "/run/user/1000/sway-ipc.1000.4242.sock";
        let p = socket_path(&env(&[("SWAYSOCK", sock)]));
        assert_eq!(p, Some(PathBuf::from(sock)));
        assert_eq!(socket_path(&env(&[("SWAYSOCK", "relative/sock")])), None);
        assert_eq!(socket_path(&env(&[("SWAYSOCK", "  ")])), None);
        assert_eq!(socket_path(&env(&[])), None);
    }

    #[test]
    fn only_a_decimal_id_becomes_a_command() {
        assert_eq!(focus_command("12").as_deref(), Some("[con_id=12] focus"));
        let long = "1".repeat(21);
        for bad in ["", "1a", "-1", "1 2", "1];exec x", "{12}", long.as_str()] {
            assert_eq!(focus_command(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_windows_are_found_and_the_containers_are_not() {
        let s = parse_tree(TREE).unwrap();
        let mut ids: Vec<&str> = s.windows.iter().map(|w| w.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["10", "12", "15", "30", "7", "8", "9"]);
    }

    #[test]
    fn the_order_is_full_screen_then_floating_last_listed_first_then_tiled() {
        let s = parse_tree(TREE).unwrap();
        let classes: Vec<&str> = s.windows.iter().map(|w| w.class.as_str()).collect();
        // 15 is full screen. The floating windows are met as 30 (scratchpad), 9, 10 and reversed,
        // so 10 (on top in Sway) comes first. The tiled ones follow in tree order.
        assert_eq!(
            classes,
            [
                "mupdf",
                "mpv",
                "pavucontrol",
                "scratch",
                "foot",
                "firefox",
                "Steam"
            ]
        );
    }

    #[test]
    fn a_native_window_is_named_by_its_app_id_and_an_x11_one_by_its_class() {
        let s = parse_tree(TREE).unwrap();
        let by = |id: &str| s.windows.iter().find(|w| w.id == id).unwrap();
        assert_eq!(by("7").class, "foot");
        assert_eq!(by("7").instance, "foot");
        assert_eq!(by("12").class, "Steam");
        assert_eq!(by("12").instance, "steam");
        assert_eq!(by("12").desktop_file, "");
    }

    #[test]
    fn a_window_that_is_not_shown_is_hidden_but_still_listed() {
        let s = parse_tree(TREE).unwrap();
        let by = |id: &str| s.windows.iter().find(|w| w.id == id).unwrap();
        assert!(!by("7").hidden);
        assert!(by("12").hidden, "workspace 2 is not shown");
        assert!(by("30").hidden, "the scratchpad");
        assert!(s.windows.iter().all(|w| !w.skip && w.desktop.is_none()));
    }

    #[test]
    fn the_focused_window_and_the_outputs() {
        let s = parse_tree(TREE).unwrap();
        assert_eq!(s.active.as_deref(), Some("7"));
        assert_eq!(s.frontmost(None).as_deref(), Some("foot"));
        assert_eq!(s.pointer, None, "Sway cannot say");
        assert_eq!(s.outputs.len(), 2, "the pseudo output has no size");
        assert_eq!(s.outputs[1].x, 1920.0);
        assert_eq!(s.outputs[1].w, 1920.0);
    }

    #[test]
    fn the_app_by_a_point_is_the_one_on_top_that_is_shown() {
        let s = parse_tree(TREE).unwrap();
        // the full-screen window covers the second output
        assert_eq!(
            s.app_at(Point::new(2500.0, 500.0), None).as_deref(),
            Some("mupdf")
        );
        // floating mpv is above floating pavucontrol where they overlap, and above tiled windows
        assert_eq!(
            s.app_at(Point::new(500.0, 300.0), None).as_deref(),
            Some("mpv")
        );
        assert_eq!(
            s.app_at(Point::new(310.0, 210.0), None).as_deref(),
            Some("pavucontrol")
        );
        assert_eq!(
            s.app_at(Point::new(100.0, 900.0), None).as_deref(),
            Some("foot")
        );
        // the window of the hidden workspace is never under a point, yet it can be brought forward
        let steam = s.window_of("steam", None).map(|w| w.id.as_str());
        assert_eq!(steam, Some("12"));
    }

    #[test]
    fn a_reply_that_is_not_a_tree_is_refused() {
        assert!(parse_tree("not json").is_none());
        assert!(parse_tree("[]").is_none());
        assert!(parse_tree(&" ".repeat(MAX_PAYLOAD + 1)).is_none());
        let empty = parse_tree("{}").unwrap();
        assert!(empty.windows.is_empty() && empty.active.is_none());
    }

    #[test]
    fn a_window_with_a_bad_rectangle_is_left_out_and_the_rest_is_kept() {
        let text = r#"{"type":"root","nodes":[
          {"id":1,"type":"con","app_id":"a","rect":{"x":1e12,"y":0,"width":10,"height":10}},
          {"id":2,"type":"con","app_id":"b","rect":{"x":0,"y":0,"width":-4,"height":10}},
          {"id":3,"type":"con","app_id":"c"},
          {"id":4,"type":"con","app_id":"d","rect":{"x":0,"y":0,"width":10,"height":10}}]}"#;
        let s = parse_tree(text).unwrap();
        assert_eq!(s.windows.len(), 1);
        assert_eq!(s.windows[0].class, "d");
    }

    #[test]
    fn a_tree_that_nests_too_deep_is_cut_not_followed() {
        const BOX: &str = r#""rect":{"x":0,"y":0,"width":9,"height":9}"#;
        let depth = MAX_DEPTH + 10;
        let mut text = format!(r#"{{"id":1,"type":"con","app_id":"top",{BOX},"nodes":["#);
        for i in 0..depth {
            let id = i + 2;
            text.push_str(&format!(r#"{{"id":{id},"type":"con",{BOX},"nodes":["#));
        }
        text.push_str(&"]}".repeat(depth));
        text.push_str("]}");
        let s = parse_tree(&text).expect("it is read, only not to the bottom");
        assert_eq!(s.windows.len(), 1, "the one window that is near the top");
    }
}
