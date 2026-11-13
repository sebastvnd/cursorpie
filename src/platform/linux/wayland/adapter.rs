//! Which adapter brings the pointer and the windows to CursorPie in this Wayland session.
//!
//! A Wayland client cannot ask for either (`docs/developer/WAYLAND_SUPPORT.md`, "Why an X11 program cannot do
//! the job"), so each compositor has to be asked in its own way. What every way produces is the
//! same thing, a `kwin::Snapshot` and a pointer position, written to the state in `kwin.rs`; the
//! rest of the program reads that state and does not know which compositor wrote it.
//!
//! | Adapter | How it asks | Pointer | Windows | Bring forward |
//! |---|---|---|---|---|
//! | `Kwin` | a script run inside KWin, over D-Bus (`kwin.rs`, `bus.rs`) | yes | yes | yes |
//! | `Gnome` | a GNOME Shell extension, over D-Bus (`gnome.rs`, `bus.rs`) | yes | yes | yes |
//! | `Hyprland` | its control socket (`hyprland.rs`) | yes | yes | yes |
//! | `Sway` | its i3-style IPC socket (`sway.rs`) | **no**: Sway has no call for it | yes | yes |
//! | `Off` | nothing | no | no | no |
//!
//! The two that are asked over a socket (Hyprland, Sway) are read by `poller::start_poller`: one thread
//! that wakes up when something asks for a fresh answer (`poller::request_sync`, at the press of the
//! trigger) and otherwise reads the pointer every 100 ms and everything every second, so the
//! answer is never far out of date.
//!
//! **Not run.** Everything in this file and in the adapters it starts was written from the
//! documentation of each compositor, not run on one (see `docs/developer/WAYLAND_SUPPORT.md`).

use super::super::session::{Compositor, Session};
use serde_json::Value;
use std::sync::OnceLock;

/// How the pointer and the windows reach CursorPie in this session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Adapter {
    /// A script inside KWin (KDE Plasma).
    Kwin,
    /// An extension inside GNOME Shell.
    Gnome,
    /// Hyprland's control socket.
    Hyprland,
    /// Sway's IPC socket.
    Sway,
    /// No adapter for this compositor: capture and shortcuts work, per-app features are off.
    Off,
}

impl Adapter {
    /// The adapter of a compositor.
    pub(crate) fn for_compositor(c: Compositor) -> Adapter {
        match c {
            Compositor::Plasma => Adapter::Kwin,
            Compositor::Gnome => Adapter::Gnome,
            Compositor::Hyprland => Adapter::Hyprland,
            Compositor::Sway => Adapter::Sway,
            Compositor::Cosmic | Compositor::Other => Adapter::Off,
        }
    }

    /// The name of the adapter, for the status row of Settings and the welcome page.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Adapter::Kwin => "KWin adapter",
            Adapter::Gnome => "GNOME extension",
            Adapter::Hyprland => "Hyprland adapter",
            Adapter::Sway => "Sway adapter",
            Adapter::Off => "Window adapter",
        }
    }

    /// Whether the compositor can say where the pointer is. Sway cannot: it has no call for it, and
    /// a Wayland client is not told either.
    pub(crate) fn knows_pointer(self) -> bool {
        !matches!(self, Adapter::Sway | Adapter::Off)
    }
}

/// The adapter of this process, decided once from the session.
pub(crate) fn current() -> Adapter {
    static CURRENT: OnceLock<Adapter> = OnceLock::new();
    *CURRENT.get_or_init(|| Adapter::for_compositor(Session::current().compositor()))
}

// ------------------------------------------------------------------------------- reading JSON

// The compositors write JSON that differs a little between versions (a flag that was a boolean and
// is a number now, a field that is `null` when it is empty). These readers take what is there and
// give a neutral value for anything else, so one odd field never throws a whole reply away.

/// The string at `key`, or `""`.
pub(super) fn str_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The number at `key`.
pub(super) fn f64_of(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

/// The integer at `key`.
pub(super) fn i64_of(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(Value::as_i64)
}

/// The boolean at `key`.
pub(super) fn bool_of(v: &Value, key: &str) -> Option<bool> {
    v.get(key).and_then(Value::as_bool)
}

/// The two numbers of the array at `key` (`[x, y]`).
pub(super) fn pair_of(v: &Value, key: &str) -> Option<(f64, f64)> {
    let a = v.get(key)?.as_array()?;
    Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_compositor_has_an_adapter_or_says_it_has_none() {
        assert_eq!(Adapter::for_compositor(Compositor::Plasma), Adapter::Kwin);
        assert_eq!(Adapter::for_compositor(Compositor::Gnome), Adapter::Gnome);
        assert_eq!(
            Adapter::for_compositor(Compositor::Hyprland),
            Adapter::Hyprland
        );
        assert_eq!(Adapter::for_compositor(Compositor::Sway), Adapter::Sway);
        assert_eq!(Adapter::for_compositor(Compositor::Cosmic), Adapter::Off);
        assert_eq!(Adapter::for_compositor(Compositor::Other), Adapter::Off);
    }

    #[test]
    fn only_sway_and_no_adapter_cannot_say_where_the_pointer_is() {
        assert!(Adapter::Kwin.knows_pointer());
        assert!(Adapter::Gnome.knows_pointer());
        assert!(Adapter::Hyprland.knows_pointer());
        assert!(!Adapter::Sway.knows_pointer());
        assert!(!Adapter::Off.knows_pointer());
    }

    #[test]
    fn each_adapter_has_its_own_label() {
        let labels = [
            Adapter::Kwin,
            Adapter::Gnome,
            Adapter::Hyprland,
            Adapter::Sway,
            Adapter::Off,
        ]
        .map(Adapter::label);
        for (i, a) in labels.iter().enumerate() {
            assert!(!a.is_empty());
            assert!(!labels[i + 1..].contains(a), "{a} is used twice");
        }
    }

    #[test]
    fn json_readers_give_a_neutral_value_for_anything_odd() {
        let v: Value = serde_json::from_str(
            r#"{"s":"a","n":1.5,"i":3,"b":true,"p":[1,2],"nul":null,"flag":0,"short":[1]}"#,
        )
        .unwrap();
        assert_eq!(str_of(&v, "s"), "a");
        assert_eq!(str_of(&v, "nul"), "", "null is empty");
        assert_eq!(str_of(&v, "missing"), "");
        assert_eq!(str_of(&v, "n"), "", "a number is not text");
        assert_eq!(f64_of(&v, "n"), Some(1.5));
        assert_eq!(i64_of(&v, "i"), Some(3));
        assert_eq!(i64_of(&v, "n"), None);
        assert_eq!(bool_of(&v, "b"), Some(true));
        assert_eq!(bool_of(&v, "flag"), None, "a number is not a boolean");
        assert_eq!(pair_of(&v, "p"), Some((1.0, 2.0)));
        assert_eq!(pair_of(&v, "short"), None);
        assert_eq!(pair_of(&v, "s"), None);
    }
}
