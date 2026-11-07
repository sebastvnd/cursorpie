//! The Wayland backend of the Linux platform layer: what the facade functions of `backend.rs` call
//! in a Wayland session. See `docs/WAYLAND_SUPPORT.md`.
//!
//! * [`Sink`] and the registry below: where the events the mouse layer keeps are written (a clone
//!   of the mouse, made with `/dev/uinput`), and where a click the state machine re-posts goes.
//! * [`press_shortcut`]: shortcuts, through a virtual keyboard made the same way.
//! * [`report`]: the capability report: what works in this session and, if something does not,
//!   the exact fix. Pure, so every combination is tested on every OS.
//! * [`coords`]: where the pie window goes, from KWin's points to the toolkit's (fractional scales).
//! * [`kwin`]: the KDE adapter (pointer, windows, outputs), a small script run inside KWin, and the
//!   state every adapter writes to.
//! * [`adapter`]: which adapter this session has; [`gnome`] (a GNOME Shell extension), [`hyprland`]
//!   and [`sway`] (their sockets) are the others.
//! * `logind` (Linux only, so not a link): whether the session is locked or in the background (the grabs are lifted then).
//!
//! The X11 backend is not touched: the session decides once which of the two answers (see
//! [`active`]).

pub(crate) mod adapter;
pub(crate) mod coords;
pub(crate) mod gnome;
pub(crate) mod hyprland;
pub(crate) mod kwin;
pub(crate) mod sway;

#[cfg(target_os = "linux")]
pub(crate) mod bus;

#[cfg(target_os = "linux")]
pub(crate) mod hyprland_io;

#[cfg(target_os = "linux")]
pub(crate) mod poller;

#[cfg(target_os = "linux")]
pub(crate) mod sway_io;

#[cfg(target_os = "linux")]
pub(crate) mod logind;

use self::adapter::Adapter;
use super::session::{DisplayServer, Session};
use super::uinput::{CloneSpec, InputEvent, UinputDevice, code_for_button};
use crate::keys::Shortcut;
use std::io;
use std::sync::{Arc, Mutex, OnceLock};

// ----------------------------------------------------------------------------------- the choice

/// Whether this process uses the Wayland backend: the session is a Wayland one (and
/// `CURSORPIE_BACKEND=x11` did not say otherwise, see `Session::detect`). Decided once.
pub(crate) fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| Session::current().server == DisplayServer::Wayland)
}

// ------------------------------------------------------------------------------------ the sinks

/// Where events are written to: a virtual device.
pub(crate) trait Sink: Send {
    /// Writes a frame (events, ending with `SYN_REPORT`).
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()>;
    /// Releases every key and button that is down.
    fn release_all(&mut self) -> io::Result<()>;
}

impl Sink for UinputDevice {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        UinputDevice::emit(self, events)
    }

    fn release_all(&mut self) -> io::Result<()> {
        UinputDevice::release_all(self)
    }
}

/// A sink shared between the loop that reads a mouse and the threads that re-post clicks.
pub(crate) type SharedSink = Arc<Mutex<dyn Sink>>;

/// The sink of the mouse that last had a button the state machine looks at: a click the state
/// machine re-posts goes to the same virtual device the rest of that gesture (its motion, its
/// release) goes through, so the compositor sees one pointer press and release, not two devices'.
static CURRENT: Mutex<Option<SharedSink>> = Mutex::new(None);

/// Makes `sink` the one [`inject_button`] writes to.
pub(crate) fn set_current_sink(sink: &SharedSink) {
    let mut cur = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    if !cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, sink)) {
        *cur = Some(sink.clone());
    }
}

/// Forgets the current sink if it is `sink` (its mouse went away).
pub(crate) fn forget_sink(sink: &SharedSink) {
    let mut cur = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    if cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, sink)) {
        *cur = None;
    }
}

/// Presses or releases the button `button` (CursorPie numbering) on the current virtual mouse.
/// `false` if there is none yet or the write failed.
pub(crate) fn inject_button(button: i64, press: bool) -> bool {
    let Some(code) = code_for_button(button) else {
        return false;
    };
    let Some(sink) = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
        return false;
    };
    let mut s = sink.lock().unwrap_or_else(|e| e.into_inner());
    s.emit(&[InputEvent::key(code, press), InputEvent::SYN])
        .is_ok()
}

// ---------------------------------------------------------------------------------- shortcuts

/// The events of a shortcut, in the order of the X11 backend (`Conn::press_shortcut`): the
/// modifiers down in the order the shortcut was recorded in, the key down and up, the modifiers up
/// in reverse. Every key is a frame of its own. Linux key codes, no offset (the X11 backend adds 8).
/// `None` for a key the table has no code for.
pub(crate) fn shortcut_events(sc: Shortcut) -> Option<Vec<InputEvent>> {
    let key = crate::platform::keymap::linux_code_for(sc.key)?;
    let mods: Vec<u16> = sc
        .ordered_mods()
        .iter()
        .filter_map(|bit| {
            crate::platform::keymap::LINUX_MODIFIER_KEYS
                .iter()
                .find(|(b, _)| b == bit)
        })
        .map(|(_, code)| *code)
        .collect();
    let mut out = Vec::with_capacity(2 * (mods.len() + 1) * 2);
    let mut frame = |code: u16, down: bool| {
        out.push(InputEvent::key(code, down));
        out.push(InputEvent::SYN);
    };
    for m in &mods {
        frame(*m, true);
    }
    frame(key, true);
    frame(key, false);
    for m in mods.iter().rev() {
        frame(*m, false);
    }
    Some(out)
}

/// The virtual keyboard. Made when the tap starts ([`prepare`]) so the desktop has opened it by
/// the time the first shortcut is sent.
static KEYBOARD: Mutex<Option<UinputDevice>> = Mutex::new(None);

/// Makes the virtual keyboard, if there is none. Returns whether there is one.
pub(crate) fn prepare() -> bool {
    let mut k = KEYBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if k.is_none() {
        match UinputDevice::create(&CloneSpec::keyboard()) {
            Ok(dev) => *k = Some(dev),
            Err(e) => {
                diag::log!(
                    "wayland",
                    "keyboard_failed",
                    error = format!("{:?}", e.kind())
                );
                return false;
            }
        }
    }
    true
}

/// Sends a shortcut to the window that has the keyboard focus.
pub(crate) fn press_shortcut(sc: Shortcut) {
    let Some(events) = shortcut_events(sc) else {
        return;
    };
    if !prepare() {
        return;
    }
    let mut k = KEYBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(dev) = k.as_mut()
        && dev.emit(&events).is_err()
    {
        // the device went bad: drop it so the next shortcut makes a new one
        *k = None;
    }
}

// ----------------------------------------------------------------------------------- the report

/// What can be done in this session right now: the answers of the probes, as plain facts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    /// An evdev node of a mouse can be opened for reading (and so grabbed).
    pub read_input: bool,
    /// `/dev/uinput` can be opened for writing.
    pub write_uinput: bool,
    /// The adapter of the compositor is running: the pointer and the windows come from it (KWin's
    /// script, the GNOME extension, Hyprland's or Sway's socket).
    pub adapter: bool,
}

/// What CursorPie can do in a Wayland session, for Settings and the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The mouse can be captured: the gesture works.
    pub ready: bool,
    /// Per-app profiles, "app under the cursor" and "bring the app forward" work.
    pub per_app: bool,
    /// One short paragraph: what is on, and what to do if something is off.
    pub text: String,
}

/// The udev rule that gives the logged-in user `/dev/uinput`, shipped in `packaging/linux/`.
pub const UDEV_RULE_FILE: &str = "70-cursorpie.rules";

/// Builds the report. Pure: the session is read from the environment lookup of
/// [`Session::detect`], the facts come from [`Probe`].
pub fn report(session: &Session, probe: Probe) -> Report {
    if session.server != DisplayServer::Wayland {
        return Report {
            ready: true,
            per_app: true,
            text: "X11 session: nothing to set up.".to_string(),
        };
    }
    let mut missing = Vec::new();
    if !probe.write_uinput {
        missing.push("create a virtual mouse (/dev/uinput)");
    }
    if !probe.read_input {
        missing.push("read your mouse (/dev/input/event*)");
    }
    if !missing.is_empty() {
        return Report {
            ready: false,
            per_app: false,
            text: format!(
                "CursorPie needs permission to {}. Install {UDEV_RULE_FILE} (it is in the \
                 package, or in packaging/linux/ of the source) into /etc/udev/rules.d/, run \
                 `sudo udevadm control --reload && sudo udevadm trigger`, and log in again. \
                 Until then CursorPie does not touch your mouse.",
                missing.join(" and ")
            ),
        };
    }
    let adapter = Adapter::for_compositor(session.compositor());
    let name = session.compositor().name();
    match (adapter, probe.adapter) {
        (Adapter::Kwin, true) => Report {
            ready: true,
            per_app: true,
            text: "Ready. Mouse capture is on and CursorPie sees your windows through KWin."
                .to_string(),
        },
        (Adapter::Kwin, false) => Report {
            ready: true,
            per_app: false,
            text: "Mouse capture is on. The KWin adapter is not running, so per-app profiles, \
                   \"app under the cursor\" and \"bring the app forward\" are off, and the pie \
                   may open away from the cursor."
                .to_string(),
        },
        (Adapter::Gnome, true) => Report {
            ready: true,
            per_app: true,
            text: "Ready. Mouse capture is on and CursorPie sees your windows through its GNOME \
                   Shell extension."
                .to_string(),
        },
        (Adapter::Gnome, false) => Report {
            ready: true,
            per_app: false,
            text: format!(
                "Mouse capture is on. The CursorPie extension for GNOME Shell is not running, so \
                 per-app profiles, \"app under the cursor\" and \"bring the app forward\" are off, \
                 and the pie may open away from the cursor. {}",
                gnome::enable_steps()
            ),
        },
        (Adapter::Hyprland, true) => Report {
            ready: true,
            per_app: true,
            text: "Ready. Mouse capture is on and CursorPie sees your windows through Hyprland."
                .to_string(),
        },
        (Adapter::Hyprland, false) => Report {
            ready: true,
            per_app: false,
            text: "Mouse capture is on. CursorPie cannot reach Hyprland's control socket \
                   (HYPRLAND_INSTANCE_SIGNATURE and XDG_RUNTIME_DIR must be set for it), so \
                   per-app profiles, \"app under the cursor\" and \"bring the app forward\" are \
                   off, and the pie may open away from the cursor."
                .to_string(),
        },
        (Adapter::Sway, true) => Report {
            ready: true,
            per_app: true,
            text: "Mouse capture is on and CursorPie sees your windows through Sway. Sway cannot \
                   say where the pointer is, so \"app under the cursor\" is the focused app (right \
                   with focus_follows_mouse yes) and the pie may open away from the cursor."
                .to_string(),
        },
        (Adapter::Sway, false) => Report {
            ready: true,
            per_app: false,
            text: "Mouse capture is on. CursorPie cannot reach Sway's IPC socket (SWAYSOCK must \
                   be set for it), so per-app profiles, \"app under the cursor\" and \"bring the \
                   app forward\" are off, and the pie may open away from the cursor."
                .to_string(),
        },
        (Adapter::Off, _) => Report {
            ready: true,
            per_app: false,
            text: format!(
                "Mouse capture is on. {name} has no adapter yet, so per-app profiles, \"app under \
                 the cursor\" and \"bring the app forward\" are off, and the pie may open away \
                 from the cursor."
            ),
        },
    }
}

/// The name of this session's adapter, for the status row of Settings and the welcome page.
pub fn adapter_label() -> &'static str {
    adapter::current().label()
}

/// The probes of this machine. Opens `/dev/uinput` for writing and one mouse node for reading, and
/// closes both again: nothing is created or grabbed.
pub fn probe() -> Probe {
    let write_uinput = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/uinput")
        .or_else(|_| {
            std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/input/uinput")
        })
        .is_ok();
    let nodes = std::fs::read_to_string("/proc/bus/input/devices")
        .map(|t| super::inputdev::parse_mouse_nodes(&t))
        .unwrap_or_default();
    // with no mouse listed there is nothing to read: not a permission problem
    let read_input = nodes.is_empty()
        || nodes
            .iter()
            .any(|n| std::fs::File::open(format!("/dev/input/{}", n.node)).is_ok());
    Probe {
        read_input,
        write_uinput,
        adapter: kwin::running(),
    }
}

/// The report of this machine now.
pub fn current_report() -> Report {
    report(&Session::current(), probe())
}

/// [`current_report`], but the machine is asked at most every two seconds: Settings draws many
/// times a second and the probes open files.
pub fn cached_report() -> Report {
    static CACHE: Mutex<Option<(std::time::Instant, Report)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, r)) = c.as_ref()
        && at.elapsed() < std::time::Duration::from_secs(2)
    {
        return r.clone();
    }
    let fresh = current_report();
    *c = Some((std::time::Instant::now(), fresh.clone()));
    fresh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{MOD_CMD, MOD_CTRL, MOD_SHIFT};

    fn session(pairs: &[(&str, &str)]) -> Session {
        Session::detect(&|k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| v.to_string())
        })
    }

    fn plasma() -> Session {
        session(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("DISPLAY", ":0"),
            ("XDG_CURRENT_DESKTOP", "KDE"),
        ])
    }

    const ALL: Probe = Probe {
        read_input: true,
        write_uinput: true,
        adapter: true,
    };

    #[test]
    fn plasma_with_everything_is_ready_and_sees_windows() {
        let r = report(&plasma(), ALL);
        assert!(r.ready && r.per_app);
        assert!(r.text.starts_with("Ready"), "{}", r.text);
    }

    #[test]
    fn plasma_without_the_adapter_captures_the_mouse_but_not_apps() {
        let r = report(
            &plasma(),
            Probe {
                adapter: false,
                ..ALL
            },
        );
        assert!(r.ready);
        assert!(!r.per_app);
        assert!(r.text.contains("KWin adapter"), "{}", r.text);
    }

    #[test]
    fn a_missing_permission_says_exactly_what_to_do_and_turns_capture_off() {
        for probe in [
            Probe {
                write_uinput: false,
                ..ALL
            },
            Probe {
                read_input: false,
                ..ALL
            },
        ] {
            let r = report(&plasma(), probe);
            assert!(!r.ready && !r.per_app);
            assert!(r.text.contains(UDEV_RULE_FILE), "{}", r.text);
            assert!(r.text.contains("does not touch your mouse"), "{}", r.text);
        }
        let both = report(
            &plasma(),
            Probe {
                read_input: false,
                write_uinput: false,
                ..ALL
            },
        );
        assert!(both.text.contains(" and "), "both are named: {}", both.text);
        assert!(both.text.contains("/dev/uinput") && both.text.contains("/dev/input"));
    }

    fn wayland_on(desktop: &str, extra: &[(&str, &str)]) -> Session {
        let mut pairs = vec![
            ("XDG_SESSION_TYPE", "wayland"),
            ("XDG_CURRENT_DESKTOP", desktop),
            ("DISPLAY", ":0"),
        ];
        pairs.extend_from_slice(extra);
        session(&pairs)
    }

    #[test]
    fn a_compositor_without_an_adapter_names_itself_and_has_no_per_app_features() {
        for desktop in ["river", "COSMIC"] {
            let r = report(&wayland_on(desktop, &[]), ALL);
            assert!(r.ready && !r.per_app, "{desktop}");
            assert!(r.text.contains("no adapter yet"), "{}", r.text);
        }
        let r = report(&wayland_on("river", &[]), ALL);
        assert!(r.text.contains("This desktop"), "{}", r.text);
    }

    #[test]
    fn gnome_is_ready_with_its_extension_and_says_how_to_install_it_without() {
        let gnome = wayland_on("ubuntu:GNOME", &[]);
        let on = report(&gnome, ALL);
        assert!(on.ready && on.per_app, "{}", on.text);
        assert!(on.text.contains("GNOME"), "{}", on.text);
        let off = report(
            &gnome,
            Probe {
                adapter: false,
                ..ALL
            },
        );
        assert!(off.ready && !off.per_app);
        assert!(off.text.contains(gnome::EXTENSION_UUID), "{}", off.text);
        assert!(off.text.contains("gnome-extensions enable"), "{}", off.text);
    }

    #[test]
    fn hyprland_is_ready_with_its_socket_and_names_the_variable_without() {
        let hypr = wayland_on("Hyprland", &[]);
        let on = report(&hypr, ALL);
        assert!(on.ready && on.per_app && on.text.starts_with("Ready"));
        let off = report(
            &hypr,
            Probe {
                adapter: false,
                ..ALL
            },
        );
        assert!(!off.per_app);
        assert!(
            off.text.contains("HYPRLAND_INSTANCE_SIGNATURE"),
            "{}",
            off.text
        );
    }

    #[test]
    fn sway_works_per_app_but_says_it_cannot_find_the_pointer() {
        let sway = wayland_on("sway", &[]);
        let on = report(&sway, ALL);
        assert!(on.ready && on.per_app);
        assert!(
            on.text.contains("cannot say where the pointer is"),
            "{}",
            on.text
        );
        assert!(on.text.contains("focus_follows_mouse"), "{}", on.text);
        let off = report(
            &sway,
            Probe {
                adapter: false,
                ..ALL
            },
        );
        assert!(!off.per_app);
        assert!(off.text.contains("SWAYSOCK"), "{}", off.text);
    }

    #[test]
    fn a_session_that_names_no_desktop_is_found_by_its_socket_variable() {
        let hypr = session(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("HYPRLAND_INSTANCE_SIGNATURE", "abc_1_2"),
        ]);
        assert!(report(&hypr, ALL).text.contains("Hyprland"));
    }

    #[test]
    fn an_x11_session_needs_nothing() {
        let x = session(&[("XDG_SESSION_TYPE", "x11"), ("DISPLAY", ":0")]);
        let r = report(
            &x,
            Probe {
                read_input: false,
                write_uinput: false,
                adapter: false,
            },
        );
        assert!(r.ready && r.per_app);
    }

    #[test]
    fn a_shortcut_is_modifiers_down_key_modifiers_up_in_reverse() {
        // macOS code 6 is Z; Ctrl+Shift+Z recorded in the usual order (Ctrl, then Shift)
        let ev = shortcut_events(Shortcut::new(6, MOD_CTRL | MOD_SHIFT)).unwrap();
        let keys: Vec<(u16, bool)> = ev
            .iter()
            .filter(|e| e.ty == super::super::uinput::EV_KEY)
            .map(|e| (e.code, e.value == 1))
            .collect();
        // KEY_LEFTCTRL 29, KEY_LEFTSHIFT 42, KEY_Z 44
        assert_eq!(
            keys,
            [
                (29, true),
                (42, true),
                (44, true),
                (44, false),
                (42, false),
                (29, false)
            ]
        );
        // every key is a frame of its own
        assert_eq!(ev.len(), 12);
        assert!(ev.iter().skip(1).step_by(2).all(|e| *e == InputEvent::SYN));
    }

    #[test]
    fn a_shortcut_uses_linux_key_codes_without_the_x11_offset() {
        let ev = shortcut_events(Shortcut::new(6, MOD_CMD)).unwrap();
        let down: Vec<u16> = ev
            .iter()
            .filter(|e| e.ty == super::super::uinput::EV_KEY && e.value == 1)
            .map(|e| e.code)
            .collect();
        assert_eq!(down, [125, 44], "Super, then Z");
    }

    #[test]
    fn a_key_without_a_code_sends_nothing() {
        assert!(shortcut_events(Shortcut::new(0x7fff, 0)).is_none());
    }
}
