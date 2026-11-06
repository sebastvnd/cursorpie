//! Which kind of Linux session CursorPie runs in. Pure: it reads the environment through a lookup
//! function, so every session shape can be tested on any OS.

/// The display server of the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

/// What the environment says about the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub server: DisplayServer,
    /// Lower-case entries of `XDG_CURRENT_DESKTOP` (it may list several: `ubuntu:GNOME`).
    pub desktops: Vec<String>,
    /// Whether an X server (or XWayland) can be reached through `DISPLAY`.
    pub has_x_display: bool,
}

fn nonempty(get: &dyn Fn(&str) -> Option<String>, key: &str) -> Option<String> {
    get(key)
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

impl Session {
    /// Reads the session from an environment lookup. `CURSORPIE_BACKEND=x11|wayland` overrides the
    /// display server (for debugging and bug reports).
    pub fn detect(get: &dyn Fn(&str) -> Option<String>) -> Session {
        let forced = nonempty(get, "CURSORPIE_BACKEND").map(|v| v.to_ascii_lowercase());
        let kind = nonempty(get, "XDG_SESSION_TYPE").map(|v| v.to_ascii_lowercase());
        let server = match (forced.as_deref(), kind.as_deref()) {
            (Some("x11"), _) => DisplayServer::X11,
            (Some("wayland"), _) => DisplayServer::Wayland,
            (_, Some("x11")) => DisplayServer::X11,
            (_, Some("wayland")) => DisplayServer::Wayland,
            _ => {
                if nonempty(get, "WAYLAND_DISPLAY").is_some() {
                    DisplayServer::Wayland
                } else if nonempty(get, "DISPLAY").is_some() {
                    DisplayServer::X11
                } else {
                    DisplayServer::Unknown
                }
            }
        };
        let desktops = nonempty(get, "XDG_CURRENT_DESKTOP")
            .map(|v| {
                v.split(':')
                    .map(|d| d.trim().to_ascii_lowercase())
                    .filter(|d| !d.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Session {
            server,
            desktops,
            has_x_display: nonempty(get, "DISPLAY").is_some(),
        }
    }

    /// The session of this process.
    pub fn current() -> Session {
        Session::detect(&|k| std::env::var(k).ok())
    }

    /// Whether the current desktop environment list names `name` (case-insensitive).
    #[cfg(test)]
    pub fn is_desktop(&self, name: &str) -> bool {
        self.desktops.iter().any(|d| d.eq_ignore_ascii_case(name))
    }

    /// Whether the X11 backend can see every window and mouse event: a real X11 session.
    #[cfg(test)]
    pub fn x11_sees_everything(&self) -> bool {
        self.server == DisplayServer::X11 && self.has_x_display
    }

    /// Why CursorPie will not fully work here, in one sentence for the log and Settings; `None`
    /// when this is a plain X11 session.
    pub fn problem(&self) -> Option<&'static str> {
        match (self.server, self.has_x_display) {
            (_, false) => Some(
                "no X display (DISPLAY is not set). CursorPie needs an X11 session or XWayland.",
            ),
            (DisplayServer::Wayland, true) => Some(
                "this is a Wayland session: the X11 backend only sees X11 and XWayland windows, so \
                 the gesture works over those only. Wayland support is planned \
                 (docs/LINUX_MIGRATION.md); log in with an X11 session for full support.",
            ),
            _ => None,
        }
    }
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

    #[test]
    fn a_plain_x11_session() {
        let s = Session::detect(&env(&[
            ("XDG_SESSION_TYPE", "x11"),
            ("DISPLAY", ":0"),
            ("XDG_CURRENT_DESKTOP", "XFCE"),
        ]));
        assert_eq!(s.server, DisplayServer::X11);
        assert!(s.x11_sees_everything());
        assert!(s.problem().is_none());
        assert!(s.is_desktop("xfce"));
    }

    #[test]
    fn gnome_on_wayland_with_xwayland_is_not_full_support() {
        let s = Session::detect(&env(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0"),
            ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
        ]));
        assert_eq!(s.server, DisplayServer::Wayland);
        assert!(!s.x11_sees_everything());
        assert_eq!(s.desktops, ["ubuntu", "gnome"]);
        assert!(s.is_desktop("GNOME"));
        assert!(s.problem().unwrap().contains("Wayland"));
    }

    #[test]
    fn the_session_type_is_inferred_when_it_is_missing() {
        let w = Session::detect(&env(&[("WAYLAND_DISPLAY", "wayland-1")]));
        assert_eq!(w.server, DisplayServer::Wayland);
        let x = Session::detect(&env(&[("DISPLAY", ":1")]));
        assert_eq!(x.server, DisplayServer::X11);
        let none = Session::detect(&env(&[]));
        assert_eq!(none.server, DisplayServer::Unknown);
        assert!(none.problem().unwrap().contains("DISPLAY"));
    }

    #[test]
    fn the_override_wins_over_the_environment() {
        let s = Session::detect(&env(&[
            ("CURSORPIE_BACKEND", "X11"),
            ("XDG_SESSION_TYPE", "wayland"),
            ("DISPLAY", ":0"),
        ]));
        assert_eq!(s.server, DisplayServer::X11);
        assert!(s.x11_sees_everything());
    }

    #[test]
    fn blank_variables_count_as_unset() {
        let s = Session::detect(&env(&[("DISPLAY", "  "), ("XDG_CURRENT_DESKTOP", ":")]));
        assert!(!s.has_x_display);
        assert!(s.desktops.is_empty());
    }
}
