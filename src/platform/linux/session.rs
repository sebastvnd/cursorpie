//! Which kind of Linux session CursorPie runs in. Pure: it reads the environment through a lookup
//! function, so every session shape can be tested on any OS.

/// The display server of the session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayServer {
    X11,
    Wayland,
    Unknown,
}

/// The compositor of a Wayland session, as far as `XDG_CURRENT_DESKTOP` says (and, for Hyprland and
/// Sway, the variable each sets for its own socket). Plasma, GNOME, Hyprland and Sway have an
/// adapter (see `wayland::adapter`); the others get mouse capture and shortcuts without per-app
/// features.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compositor {
    Plasma,
    Gnome,
    Sway,
    Hyprland,
    Cosmic,
    Other,
}

impl Compositor {
    /// The name for Settings and the log.
    pub fn name(self) -> &'static str {
        match self {
            Compositor::Plasma => "KDE Plasma",
            Compositor::Gnome => "GNOME",
            Compositor::Sway => "Sway",
            Compositor::Hyprland => "Hyprland",
            Compositor::Cosmic => "COSMIC",
            Compositor::Other => "This desktop",
        }
    }
}

/// What the environment says about the session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub server: DisplayServer,
    /// Lower-case entries of `XDG_CURRENT_DESKTOP` (it may list several: `ubuntu:GNOME`).
    pub desktops: Vec<String>,
    /// Whether an X server (or XWayland) can be reached through `DISPLAY`.
    pub has_x_display: bool,
    /// `HYPRLAND_INSTANCE_SIGNATURE` is set: Hyprland started this session, whatever
    /// `XDG_CURRENT_DESKTOP` says.
    pub hyprland_signature: bool,
    /// `SWAYSOCK` is set: Sway started this session.
    pub sway_socket: bool,
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
            hyprland_signature: nonempty(get, "HYPRLAND_INSTANCE_SIGNATURE").is_some(),
            sway_socket: nonempty(get, "SWAYSOCK").is_some(),
        }
    }

    /// The session of this process.
    pub fn current() -> Session {
        Session::detect(&|k| std::env::var(k).ok())
    }

    /// The compositor, from the desktop names. The first name that is known wins (`ubuntu:GNOME`).
    /// When none is known, the variable Hyprland or Sway sets for its own socket decides: a session
    /// started from a login manager or a script often leaves `XDG_CURRENT_DESKTOP` empty.
    pub fn compositor(&self) -> Compositor {
        self.desktops
            .iter()
            .find_map(|d| match d.as_str() {
                "kde" | "plasma" => Some(Compositor::Plasma),
                "gnome" => Some(Compositor::Gnome),
                "sway" => Some(Compositor::Sway),
                "hyprland" => Some(Compositor::Hyprland),
                "cosmic" => Some(Compositor::Cosmic),
                _ => None,
            })
            .or(self.hyprland_signature.then_some(Compositor::Hyprland))
            .or(self.sway_socket.then_some(Compositor::Sway))
            .unwrap_or(Compositor::Other)
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
                "this is a Wayland session and the X11 backend was asked for: it only sees X11 and \
                 XWayland windows, so the gesture works over those only. Unset CURSORPIE_BACKEND to \
                 use the Wayland backend (docs/WAYLAND_SUPPORT.md).",
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

    #[test]
    fn the_compositor_comes_from_the_desktop_names() {
        let c = |d: &str| {
            Session::detect(&env(&[
                ("XDG_SESSION_TYPE", "wayland"),
                ("XDG_CURRENT_DESKTOP", d),
            ]))
            .compositor()
        };
        assert_eq!(c("KDE"), Compositor::Plasma);
        assert_eq!(c("ubuntu:GNOME"), Compositor::Gnome);
        assert_eq!(c("sway"), Compositor::Sway);
        assert_eq!(c("Hyprland"), Compositor::Hyprland);
        assert_eq!(c("COSMIC"), Compositor::Cosmic);
        assert_eq!(c("river"), Compositor::Other);
        assert_eq!(c(""), Compositor::Other);
    }

    #[test]
    fn hyprland_and_sway_are_found_by_their_own_variables_when_the_desktop_name_is_missing() {
        let with = |pairs: &[(&str, &str)]| Session::detect(&env(pairs)).compositor();
        assert_eq!(
            with(&[
                ("XDG_SESSION_TYPE", "wayland"),
                ("HYPRLAND_INSTANCE_SIGNATURE", "abc_123"),
            ]),
            Compositor::Hyprland
        );
        assert_eq!(
            with(&[
                ("XDG_SESSION_TYPE", "wayland"),
                ("SWAYSOCK", "/run/user/1000/sway-ipc.sock"),
            ]),
            Compositor::Sway
        );
        // a desktop name that is known wins over the variables
        assert_eq!(
            with(&[
                ("XDG_CURRENT_DESKTOP", "KDE"),
                ("SWAYSOCK", "/run/user/1000/s")
            ]),
            Compositor::Plasma
        );
        // an empty value is not set
        assert_eq!(with(&[("SWAYSOCK", "  ")]), Compositor::Other);
    }
}
