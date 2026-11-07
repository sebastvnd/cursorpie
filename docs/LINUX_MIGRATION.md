# Linux support: design and migration plan

**Status: X11 (phases L1 to L4, with the gaps listed in [Implementation
status](#implementation-status)) is implemented and supported, but experimental. The manual
[checklist](#manual-verification-checklist) is under internal observation: the maintainers are
watching how CursorPie behaves on real desktops and whether any part of the X11 implementation is
wrong, so no item is signed off yet and no results are recorded here. Wayland (L5 to L7) is partly
written and has not yet been run on real hardware: see [Implementation status](WAYLAND_SUPPORT.md#implementation-status).** Releases up to 0.4.2 supported macOS and Windows only (Linux built a do-nothing stub so
`cargo test` runs); the X11 backend arrived in 0.5.0 (see `CHANGELOG.md`).

**Update, November 2026:** KDE Plasma no longer offers an X11 session from its 6.8 release (October 2026), and GNOME
dropped it before that, so the Wayland phases below are no longer optional for Linux users on current desktops. The
research behind that, a plan that starts with KDE Plasma on Wayland, and the definition of "behaves like the other
platforms" are in [WAYLAND_SUPPORT.md](WAYLAND_SUPPORT.md). Nothing in the X11 design below changes.

This document says how to make Linux behave like the other two, what
stands in the way, and in which order to do it so macOS and Windows never change along the way. It follows the structure of [WINDOWS_MIGRATION.md](WINDOWS_MIGRATION.md) and reuses its rules
(portable core, backends with identical function names, characterization tests first).

Every statement about a Wayland compositor or a desktop environment is still a design assumption.
Assumptions that decide whether the design works are collected as spikes in
[Phase 0](#phase-0-spikes); the X11 ones were answered against a real X server (Xvfb), see the results
there.

## Contents

- [Summary](#summary)
- [Implementation status](#implementation-status)
- [Goals and non-goals](#goals-and-non-goals)
- [Why Linux is different](#why-linux-is-different)
- [Support tiers](#support-tiers)
- [What is OS-specific today and its Linux counterpart](#what-is-os-specific-today-and-its-linux-counterpart)
- [Design decisions](#design-decisions)
- [Subsystem designs](#subsystem-designs)
- [Obstacles and solutions](#obstacles-and-solutions)
- [Alternatives considered](#alternatives-considered)
- [Parity matrix](#parity-matrix)
- [Phases](#phases)
- [Phase 0 spikes](#phase-0-spikes)
- [Test strategy](#test-strategy)
- [Manual verification checklist](#manual-verification-checklist)
- [Security and privacy](#security-and-privacy)
- [Packaging](#packaging)
- [Dependencies](#dependencies)
- [Documentation changes](#documentation-changes)
- [Open decisions for the maintainer](#open-decisions-for-the-maintainer)
- [Appendix A: key code table](#appendix-a-key-code-table)
- [Appendix B: mouse button table](#appendix-b-mouse-button-table)
- [Appendix C: session detection](#appendix-c-session-detection)
- [Appendix D: new files](#appendix-d-new-files)

## Summary

macOS and Windows each have one input API that does everything CursorPie needs: watch the mouse
globally, swallow events, send keys, ask which app is under the pointer, put a transparent window at a
coordinate. Linux has no such API. It has two display systems with opposite rules:

- **X11** lets any client do all of the above with no permission. Full parity is possible.
- **Wayland** forbids, by design, global input capture, input injection, reading the pointer position,
  positioning your own windows and listing other apps' windows. Each of these needs a different
  mechanism, and some depend on the compositor (GNOME, KDE, Sway, Hyprland...).

Decision record:

1. **One binary, backend chosen at start-up.** macOS and Windows pick a backend at compile time. Linux
   cannot: the same executable runs in X11 and in Wayland sessions. The Linux backend is a set of
   capabilities (capture, inject, pointer, windows, overlay, tray, autostart) selected at run time
   from the session. Call sites stay free of `cfg` and of session checks.
2. **X11 first, and complete.** Tier 1 is everything the macOS build does, with no permission, using
   XInput2 and XTest. It ships first and alone.
3. **Wayland through evdev + uinput.** The only compositor-independent way to see *and swallow* a mouse
   button on Wayland is to grab the physical device and re-emit its events through a virtual one. It
   needs a udev rule. Everything the compositor will not tell us (pointer position, focused app,
   windows) comes from per-compositor adapters. GNOME is the weakest tier.
4. **No config format change.** `CONFIG_VERSION` stays 2. Key codes remain macOS virtual key codes and
   are translated at the edge; the Linux table is a position table, like the Windows one.
5. **App ids are desktop-entry ids**, lower case, normalized from `WM_CLASS` (X11) or `app_id`
   (Wayland), so one profile matches the same app in both sessions.
6. **`glow` renderer on Linux**, the same choice as Windows, for transparency and for machines
   without Vulkan.

The single biggest risk is the pie window on Wayland: eframe/winit cannot create layer-shell surfaces,
which is the right tool there. See [Obstacles](#obstacles-and-solutions), O-4.

## Implementation status

What exists in the code today, what differs from the design below, and what was and was not checked.

**Built (X11, Tier 1):**

- L1, the seam: `Os::Linux`, the Linux key tables (`platform/keymap.rs`), session detection, desktop-entry
  parsing and app-id matching, window hit test, XDG paths, autostart file text, launch resolution,
  display-scale rules (all in `platform/linux/`, pure and tested on every OS), Linux defaults
  (`config/defaults.rs`), config and export locations (`config/store.rs`), welcome text.
- L2, the skeleton: `cfg(target_os = "linux")` selects the Linux backend; other Unixes keep the stub;
  `eframe` uses `glow` and `x11` on Linux (`Cargo.toml`).
- L3, the X11 backend: mouse capture with XInput2 passive grabs (`tap/linux.rs`), key and mouse injection
  with XTest, pointer, monitors (XRandR), frontmost app and app under the pointer (EWMH), activating an
  app, keyboard grab while recording a shortcut.
- L4, the parts that need no new dependency: XDG autostart, `xdg-open`/`gio`/shell launching, *Open app*
  that follows an app to its workspace (`reveal_app`, see `docs/OTHER_DESKTOPS.md`), a file
  chooser through `zenity` or `kdialog`, a single-instance lock (`flock`), `malloc_trim`, a hover sound
  through `canberra-gtk-play`, Pause and Quit buttons in Settings > General, `scripts/package-linux.sh`
  and `packaging/linux/cursorpie.desktop`.

**Differences from the design:**

| Design | Implemented | Why |
|---|---|---|
| `x11rb` crate | hand-written Xlib, XInput2, XTest and XRandR bindings (`platform/linux/xlib.rs`) | no new dependency, and every layout was checked against a real server |
| `xkbcommon` layout labels | none; labels use the US/ANSI fallback table | not done yet |
| StatusNotifierItem tray, D-Bus single instance, `--settings` command line | no tray; `flock` file; Pause and Quit in Settings | needs `zbus`, not done yet |
| Ignore events from the XTEST device | the grabs are lifted around an injected press | the grab-lifting sequence is what was tested |
| Right button grabbed only while a pie is open | the right button is not grabbed at all; it reaches CursorPie while the trigger is held | an active grab reports every button; tested |
| Extra mouse buttons as trigger (Appendix B) | only the middle button | the mapping needs real mice |
| Extra mouse buttons in a side-button mode (Appendix B) | X buttons 10 to 20 are buttons 5 to 15, grabbed only while a mode uses them; the mice come from `/proc/bus/input/devices` (`platform/linux/inputdev.rs`) | not yet checked on real mice |
| App icons | the `Icon=` of the desktop entry, found in the icon themes (`platform/linux/icontheme.rs`); PNG is read directly, SVG through `rsvg-convert` when it is installed | the lookup is tested with files on disk; not yet checked on a real desktop |
| Window hints, XShape fallback | none; the window uses winit's defaults | not done yet |
| Packages | a tarball with `install.sh` | `.deb`, `.rpm`, AppImage not done |

**Not built:** everything Wayland (L5 to L7), the tray, keyboard-layout labels, the XShape
fallback for a missing compositor, the `--doctor` report, Flatpak.

**What was checked:**

- The X11 mechanics, in a throw-away prototype with the same calls against a real X server (Xvfb): passive
  grabs swallow a press and the motion that follows, XInput2 and key events decode into the structures used
  in `xlib.rs`, a swallowed click is sent again correctly after lifting the grabs, the grabs come back, a
  right click during a held trigger is reported, `XGrabKeyboard` reports the key with its modifiers, RandR
  monitors, `WM_CLASS`, format-32 properties, window geometry in root coordinates, and the
  `_NET_ACTIVE_WINDOW` client message as a window manager receives it. Spikes S1 and S2 below record the
  findings.
- The key tables against the real `linux/input-event-codes.h`: all 54 special-key rows and 48 layout-key
  rows match, and the modifier codes.
- The expected values of the pure helpers' tests, by re-implementing their algorithms separately.

**Compiled and tested:** CI builds the Linux backend and runs every test on Ubuntu (`cargo tools
ci`, see `.github/workflows/ci.yml`), and the pure helpers are tested on every OS. **Under internal
observation, not signed off:** behavior on real desktops. That means a real mouse, window managers,
compositors, high-DPI displays and the distributions people use. The state handling in
`tap/linux.rs`, `Conn::activate`, the window gathering and the scale rule were first exercised only
through the prototype above, so treat them as the likeliest places for a wrong implementation. Until
the observation ends, a surprising behavior is as likely to be a bug in the implementation as a
limit of X11: report it with the distribution, desktop, session type, GPU and scaling (see
[KNOWN_ISSUES.md](KNOWN_ISSUES.md)).

## Goals and non-goals

Goals

- One code base, one `config.json` format, three platforms. A profile shared from a Mac imports on
  Linux, with the same caveat as on Windows (app ids differ).
- **No behavior change on macOS or Windows.** Linux work is additive; shared code changes only where
  `Os` gains a third value, and every such change is covered by a test pinned to the old values first.
- Everything that is a decision stays plain Rust that runs on any OS: session detection, app id
  normalization, desktop-entry parsing, key tables, hit-testing of window lists, XDG path rules,
  capability reports.
- X11: no special permission, like Windows.
- The hold, flick, double-click, drag pass-through, side-button modes, right-click switching and
  Settings behave as on macOS (see the [parity matrix](#parity-matrix) for the honest exceptions).

Non-goals (first Linux release)

- BSDs and other Unixes. They keep the stub.
- Remote sessions without local input devices (RDP, VNC, `xrdp`, SSH X forwarding): best effort only.
- TTY/console use.
- Touchpads as a trigger: now an opt-in feature on X11, see [TRACKPAD_HAPTICS_PLAN.md](TRACKPAD_HAPTICS_PLAN.md#linux-x11) and *Trackpad* in the user guide. Wayland: still no.
- GNOME Wayland parity without an extension (see [Support tiers](#support-tiers)).
- A Flatpak with Wayland capture. A sandbox cannot grab `/dev/input` devices without broad device access.

## Why Linux is different

What CursorPie needs, and what each session offers:

| Need | macOS / Windows | Linux X11 | Linux Wayland |
|---|---|---|---|
| See mouse buttons and motion globally | event tap / low-level hook | XInput2 | **forbidden**; only evdev (`/dev/input`) with a permission |
| Swallow the trigger press | tap returns nothing / hook returns 1 | passive button grab | only by exclusively grabbing the device (evdev) |
| Replay a swallowed click | `CGEvent::post` / `SendInput` | XTest | uinput (permission) |
| Send a shortcut | same | XTest | uinput (permission), or the RemoteDesktop portal with a consent dialog |
| Pointer position | `CGEvent` location / hook struct | `XQueryPointer` | **forbidden**; compositor IPC or a probe surface |
| App in front / under the pointer | `NSWorkspace`, `CGWindowList` / `GetForegroundWindow`, `WindowFromPoint` | EWMH properties | **forbidden**; compositor IPC only |
| Bring an app to the front | `open -b` / `SetForegroundWindow` | `_NET_ACTIVE_WINDOW` | **forbidden**; compositor IPC only |
| Window at a screen coordinate, always on top, click-through, transparent | window API | window hints, XShape, compositor for alpha | **forbidden** for normal windows; layer-shell where the compositor has it |
| Keyboard layout | TIS / `ToUnicodeEx` | XKB | not exposed to other clients |
| Tray icon | `NSStatusItem` / notification area | StatusNotifierItem, if the desktop has a host | same |

Two consequences shape everything below. First, "Linux" is at least four products (X11, and Wayland on
KDE, wlroots-based compositors, GNOME). Second, the pointer position and the app list are *questions*, so
they need an abstraction with several answers, not a function with one body.

## Support tiers

| Tier | Session | Meaning |
|---|---|---|
| 1 | X11 (Xorg, or a Wayland desktop run in its X11 session) | Everything the macOS build does. No permission. |
| 2 | Wayland on KDE Plasma 6, Sway, Hyprland, COSMIC, river, labwc (compositors with layer-shell or IPC we can use) | Same features. Needs the input permission (udev rule). Per-app profiles and "app under the pointer" need the compositor adapter. |
| 3 | GNOME on Wayland | Gestures and shortcuts work with the permission. The pie is drawn in a full-screen fallback window. Per-app profiles need the CursorPie GNOME Shell extension; without it only the general profile is used. |
| - | anything else (remote, TTY, unknown compositor with no adapter) | Capture and shortcuts may work; the app reports what is missing in Settings > General and falls back to the general profile and "app in front". |

The tiers are a plan, not a measurement. Which compositors really belong in Tier 2 is decided by
[spikes](#phase-0-spikes) S4 to S6. KDE Plasma 6 is the first Tier 2 target; its spikes, mechanisms and
milestones are in [WAYLAND_SUPPORT.md](WAYLAND_SUPPORT.md).

## What is OS-specific today and its Linux counterpart

Same layout as the table in `WINDOWS_MIGRATION.md`. `Wayland` means "the Tier 2/3 mechanism".

| Area | Files | X11 | Wayland |
|---|---|---|---|
| Global mouse/keyboard hook | `tap/mod.rs`, `tap/*.rs` | XInput2 passive grab on the trigger and side buttons, raw events for motion; `tap/linux/x11.rs` | evdev: grab pointer devices, forward through a uinput device, `tap/linux/evdev.rs` |
| Event types in the state machine | `tap/trigger.rs` and friends | unchanged (`Ev`, `Kind`, `Verdict`, `Point`) | unchanged |
| Sending keys / replaying clicks | `actions.rs` | XTest | uinput virtual keyboard and mouse |
| Frontmost app, app under pointer | `platform/apps.rs` | `_NET_ACTIVE_WINDOW`, `_NET_CLIENT_LIST_STACKING`, `WM_CLASS`, hit test | adapter: KWin script, Sway IPC, Hyprland IPC, GNOME extension |
| App icons | `platform/apps.rs`, `ui/appicon.rs` | desktop entry `Icon=` resolved through the icon theme (PNG/SVG), `NativeIcon::Rgba` | same (does not depend on the session) |
| Permission | `platform/permissions.rs` | none | access to `/dev/input/event*` and `/dev/uinput`, checked and reported |
| Keyboard layout | `platform/layout.rs` | `_XKB_RULES_NAMES` plus `xkbcommon` | `localectl`/`org.freedesktop.locale1`, compositor adapter, else US |
| Launch at login | `platform/login_item.rs` | XDG autostart `.desktop` file | same; Flatpak: Background portal |
| Install, uninstall | `platform/windows/install.rs` | none: distribution packages install it | same |
| One running copy | `platform/windows/instance.rs` | session-bus name, else a `flock` file in `$XDG_RUNTIME_DIR` | same |
| Return memory to the OS | `platform/memory.rs` | `malloc_trim(0)` on glibc, no-op on musl | same |
| File chooser, reveal, open, tick | `platform/shell.rs` | FileChooser portal, `zenity`/`kdialog` fallback; `xdg-open`; `canberra-gtk-play` if present | same |
| Run actions | `actions.rs` | `gio launch` / `PATH` / `xdg-open`; `/bin/sh -c` | same |
| Menu bar icon | `ui/tray.rs` | StatusNotifierItem over D-Bus | same |
| Dock icon | `lib.rs` | none | none |
| Config location | `config/store.rs` | `$XDG_CONFIG_HOME/cursorpie` | same |
| Display bounds | `ui/screen.rs` | XRandR monitors | `xdg-output`, or the adapter |
| Defaults | `config/defaults.rs` | Ctrl shortcuts, desktop-entry ids, portable commands | same |
| Build gate | `lib.rs`, `Cargo.toml`, `platform/mod.rs`, `tap/mod.rs`, `ui/tray.rs`, `main.rs` | `cfg(target_os = "linux")`; other targets keep the stub | same |
| UI text | `ui/settings/**`, `about.rs` | per-OS strings gain a Linux arm | session-specific strings |
| Packaging | `scripts/bundle.sh` | `scripts/package-linux.sh`, `packaging/linux/` | plus the udev rule |

## Design decisions

1. **`Os` gets a third value, `Os::Linux`.** Today `Os::current()` returns `Mac` for everything that is not
   Windows, "so the macOS characterization tests run unchanged" on Linux CI (see `platform/mod.rs`).
   That shortcut must go. The tests that call the unparametrized forms (`Config::default()`,
   `presets()`, `quick_actions()`, `Os::current()`) in `config/defaults.rs`, `config/mod.rs`,
   `config/share.rs`, `config/sanitize.rs` and `side.rs` will then see Linux values on Linux CI, so they
   are pinned to `_for(Os::Mac)` *in a commit that precedes the change* (phase L1), the same technique
   as 0.4.2 used for Windows. `cfg(not(windows))` guards in `config/store.rs` and `config/defaults.rs`
   tests are reviewed in the same step.
2. **A `Session` describes the running Linux session**, a plain struct built by a pure function from an
   environment lookup (like `home_from`): display server, desktop, compositor, which adapters are
   usable. Everything that varies inside Linux takes a `Session` or a capability, never reads the
   environment itself. Appendix C lists the inputs.
3. **Capabilities, not one backend.** `platform/linux/` exposes the facade functions every backend has
   (the table in `platform/mod.rs`), and implements them by delegating to capability objects chosen once:
   `Capture`, `Inject`, `Pointer`, `Toplevels`, `Overlay`, `Tray`, `Autostart`, `Shell`. A missing
   capability returns a typed "unavailable, because X" that Settings shows. The pattern "same function
   names in every backend" is kept; what is new is a run-time table behind the Linux names.
4. **Key codes stay macOS virtual key codes in `config.json`.** The Linux table maps them to Linux
   evdev key codes (`KEY_*`), which are also X11 keycodes minus 8. Layout keys reuse the existing PS/2
   scan-code table (`LAYOUT_KEYS`): for the main block, Linux key codes equal set-1 scan codes
   (assumption, verified by a unit test against `linux/input-event-codes.h` values listed in
   Appendix A). Special keys get a second table like `SPECIAL_KEYS`. Physical-position semantics are
   kept, so a shortcut recorded on a Mac lands on the same key on a QWERTZ Linux keyboard.
5. **Primary modifier is Ctrl; `MOD_CMD` is Super.** Labels say `Super`. Presets follow Linux
   conventions where they differ from Windows: redo is **Ctrl+Shift+Z** (GTK/Qt/Electron), not Ctrl+Y.
6. **App id = desktop-entry id.** See [App identity](#app-identity).
7. **Coordinates are logical points**, as everywhere. X11 has one global scale (no per-monitor scale),
   taken from the toolkit's scale factor; Wayland reports logical coordinates natively.
8. **The event layer stays portable.** `tap/linux/{x11,evdev}.rs` translate native events into `Ev` and
   call `dispatch`. The state machine and the lock order (*gesture, then config, then side state*)
   are untouched. As on Windows, pointer moves are never dropped on X11; on evdev they are forwarded
   unless the state machine says `Drop`.
9. **Our own synthetic events are recognized without a tag value.** macOS and Windows put `SYNTH_TAG` in
   the event. X11 XTest events carry no such field, so the X11 backend lifts its own grabs while it sends a
   press and puts them back afterwards, and never sees its own event (implemented this way; ignoring the
   "Virtual core XTEST pointer" device was the alternative). The evdev tap never opens the uinput devices
   it created (matched by device name and a marker in the physical-path string).
10. **`glow` on Linux.** `eframe` is declared with `default-features = false`, `glow`, `x11` and
    `wayland` on Linux, `wgpu` stays on macOS only. Rule from `UI_DESIGN.md` carries over: never
    enable `wgpu` in the Linux table, and check with `cargo tree -i wgpu --target
    x86_64-unknown-linux-gnu`.
11. **New dependencies are few, pure Rust, and justified per phase** ([Dependencies](#dependencies)).
12. **Linux code is type-checkable on any OS** with a `check-linux` feature, like `check-windows`, so
    macOS and Windows contributors can compile it without Linux.

## Subsystem designs

### Session detection and backend selection

`Session::detect(env)` reads `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY`, `DISPLAY`, `XDG_CURRENT_DESKTOP`,
`XDG_SESSION_DESKTOP`, `SWAYSOCK`, `HYPRLAND_INSTANCE_SIGNATURE`, `KDE_FULL_SESSION`
(Appendix C). Names are hints; the **capability probe** decides: try to open the X display, try to
connect to the compositor's IPC, try to open `/dev/uinput`. The result is a `Capabilities` value
(per capability: `Ready`, `Degraded(reason)` or `Missing(reason)`) shown in Settings > General.

- `CURSORPIE_BACKEND=x11|wayland` forces a choice (debugging and bug reports).
- A Wayland session with `DISPLAY` set still has XWayland. XWayland sees only X clients, so X11 capture
  there would silently miss Wayland-native windows. The probe therefore trusts `XDG_SESSION_TYPE`
  and `WAYLAND_DISPLAY` first and only uses the X11 backend in a Wayland session when asked to.
- If probing changes (the compositor restarts, the X server dies), the backend is rebuilt. The retry
  loop is the Windows one: every 2 seconds, `TAP_ACTIVE` reports the truth.

### Input capture

**X11 (`tap/linux/x11.rs`).**

- Passive grab (`XIGrabButton`) on the root window for the trigger button and the two side buttons, in
  asynchronous mode. The press is delivered to CursorPie, not to the app: the swallow. The right button is
  **not** grabbed: while the trigger is held its active grab reports every button (tested), which is
  when the state machine wants right clicks, and outside a gesture right clicks never reach CursorPie.
- Motion comes from XInput2 raw events or `XQueryPointer`, only while a gesture is running, as on
  Windows (`on_mouse` returns early when the gesture is `Idle`).
- A grab catches synthetic events from XTest too. To hand a click or a drag back to the app, the tap
  **lifts its grabs, injects through XTest, and grabs again**, so it never sees its own event. A quick
  click (both swallowed, then sent again after the physical release) works (tested). Handing a **drag**
  back while the button is still held is best effort: see S2.
- Key events are needed only to record a shortcut. While `RECORDING` is true the tap grabs the keyboard
  (core `XGrabKeyboard`, which reports `XKeyEvent`s), captures one key with its modifiers, and ungrabs. Outside
  recording no keyboard grab exists, same promise as the README's privacy section.
- The loop waits on the X connection's file descriptor and a wake pipe. It does not block in `dispatch`.

**Wayland (`tap/linux/evdev.rs`).**

- Opens pointer devices that can be a trigger: `EV_REL` with `REL_X` and `REL_Y`, and `BTN_MIDDLE`.
  Touchpads, tablets, keyboards and our own uinput devices are skipped.
- `EVIOCGRAB` takes the device exclusively; every event the state machine does not swallow is written
  to a uinput device that **copies the original's capabilities and identity** (name, vendor, product),
  event by event, with `SYN_REPORT` boundaries preserved. All codes are forwarded verbatim, including
  `REL_WHEEL_HI_RES` and `REL_HWHEEL_HI_RES` next to `REL_WHEEL`, otherwise scrolling doubles or stops.
- Hotplug through a udev monitor (or inotify on `/dev/input`): devices appear and vanish with
  receivers, KVM switches, Bluetooth sleep and suspend.
- **Never lock the user out.** If the tap hangs while it holds a grab, the mouse is dead. Rules:
  a watchdog thread checks a heartbeat from the tap loop and releases every grab if it is older than
  250 ms; `SIGTERM`/`SIGINT` release before exit; the kernel releases a grab when the file descriptor
  closes, so a crash is safe too; a panic out of `dispatch` forwards the event (fail open).
- Pause on session lock (logind `LockedHint`/`Lock` signal) and when the seat is inactive; resume on
  unlock. A lock screen must never receive a swallowed click.
- Keys: keyboards are **not** opened. On Wayland, shortcut recording uses the key events the focused
  Settings window itself receives, not a global grab. Shortcuts the compositor reserves (for example
  Super+Tab) cannot be recorded by pressing them; Settings says so and offers typing the combination.
  This keeps the udev rule limited to pointer devices ([Security](#security-and-privacy)).

### Input injection

- **X11:** XTest. Keycode = Linux key code + 8 (assumption S1: true for the `evdev` XKB rules used by
  every mainstream distribution). A shortcut presses its modifiers in a fixed order, the key, then
  releases in reverse, with the same pacing as the other backends. Layout does not matter: positions
  are sent, like on Windows scan codes.
- **Wayland:** a uinput virtual keyboard (key codes straight from the table) and the forwarding mouse
  device. A replayed click needs no coordinates: the pointer is already there, so `post_mouse` ignores
  its `Point` on Linux. (The trait keeps the parameter for the other backends.)
- The RemoteDesktop portal (libei) is the alternative for key injection with an explicit consent dialog
  and no `/dev/uinput` access. It does not replace uinput here, because capture needs uinput anyway
  to forward grabbed events. It stays an option for sandboxes ([Alternatives](#alternatives-considered)).

### Pointer position

A `Pointer` capability answers "where is the cursor, in logical points". Answers, in order:

| Strategy | Used on | Notes |
|---|---|---|
| `XQueryPointer` | X11 | exact |
| Compositor query | Hyprland (`hyprctl cursorpos`), KWin (a small script that reports `workspace.cursorPos`), GNOME (extension method) | exact; one IPC round trip, which is why the tap asks on **press**, during the 500 ms hold |
| Probe surface | wlroots-based and KWin through layer-shell | a transparent surface appears under the pointer on press; the compositor's `enter` event carries the exact position and later the exact accelerated motion |
| Dead reckoning | last resort | integrate relative motion from a known point. **Inaccurate**: libinput's acceleration happens in the compositor, so the numbers differ. Only used to keep an already open pie hovering; never to place a pie |

If no strategy works, the pie opens at the center of the display the last known position was on, and
Settings > General says "pointer position: unavailable".

### App identity

An **app id** is the key of per-app profiles. Linux defines it as follows.

1. Raw identifier: Wayland `app_id`, or X11 `WM_CLASS` (class, and instance as a second candidate).
2. Canonical form: look the raw identifier up in the installed desktop entries (`XDG_DATA_DIRS`, plus
   the Flatpak and Snap export directories): first a match on `StartupWMClass`, then on the entry's
   file name without `.desktop`, case-insensitive. The canonical id is that **file name, lower case,
   without `.desktop`** (for example `org.mozilla.firefox`, `google-chrome`, `code`).
3. No entry found: the raw identifier, lower case.
4. Matching a profile's `apps` entry is case-insensitive and also accepts any of the aliases the
   matched desktop entry has (`StartupWMClass`, file name), so a hand-typed `firefox` matches
   `org.mozilla.firefox`.

The executable name is **not** used. A sandboxed app (Flatpak, Snap) reports the PID of the sandbox
helper, an AppImage runs from a temporary mount, and many apps are launchers for other binaries.
`_NET_WM_PID` and `/proc/<pid>/exe` are a last resort for windows with no class at all.

Because one app has several plausible ids, the built-in "Browsers" and "Code" profiles list several per
app (for example Firefox as `firefox` and `org.mozilla.firefox`, Chrome as `google-chrome`,
`chromium`, `com.google.Chrome`, `org.chromium.Chromium`; VS Code as `code`, `code-oss`, `vscodium`,
`com.visualstudio.code`).

The desktop-entry index and the matching are pure functions over file contents, tested with fixtures.

### Toplevel list and "app under the pointer"

The Windows and macOS backends ask the OS "which window is at this point". On Linux the answer is
computed: each adapter returns the windows (`app id`, rectangle, stacking order, minimized, workspace)
and one pure function does the hit test, skipping docks, desktops, notifications, hidden windows,
windows on other workspaces and CursorPie's own window. On X11 the list is `_NET_CLIENT_LIST_STACKING`
with `_NET_FRAME_EXTENTS`; on Sway it is `get_tree`; on Hyprland `hyprctl clients -j`; on KWin and
GNOME the script or extension. The hit test is the one thing that is tested on every OS.

"Bring the app to the front" (`focus_target`): X11 sends `_NET_ACTIVE_WINDOW`; Wayland asks the
adapter (KWin script, `swaymsg [app_id=...] focus`, `hyprctl dispatch focuswindow`, extension). With no
adapter the option is unavailable, `target_app` is treated as `Focused`, and Settings says why.

### The pie window

Behavior to keep: transparent, borderless, click-through, always on top, `pie_size` square, centered
under the cursor, never takes focus, no taskbar button, no animation of the OS around it.

**X11.**

- Window type hints (`_NET_WM_WINDOW_TYPE` notification or utility), `_NET_WM_STATE_ABOVE`,
  skip-taskbar and skip-pager, input hint off, undecorated (winit does the last). Per-pixel alpha needs
  a **compositing manager**. If none runs (`_NET_WM_CM_S0` has no owner: bare i3, bspwm, some XFCE and
  openbox setups) a transparent window shows a black box. Fallback: shape the window with the XShape
  extension to the union of the ring and the center disc (hard edges, no feather, still no box), and tell
  the user that a compositor such as picom gives the smooth look. Click-through uses XShape's input
  region, which winit's mouse passthrough already does.
- `platform::clear_window_frame` has an X11 counterpart: a per-frame, idempotent function, fed with the
  native window id, that makes sure the hints above are still set after winit rewrites them. The
  native-handle accessor in `ui/app` currently returns only Windows handles and gains X11 and Wayland
  arms.
- Tiling window managers may tile the window despite the hints. Spike S3 tests the common ones and
  decides between hints, override-redirect while the window is unmapped, or documented floating rules.

**Wayland.** A normal toplevel cannot choose its position, stay on top or be placed under the cursor.

| Strategy | Compositors | Notes |
|---|---|---|
| layer-shell overlay, `pie_size` square, anchored top-left, margins = position | KWin, wlroots-based, COSMIC | exact placement, always above, no focus. Needs its own Wayland surface, see O-4 |
| full-screen transparent toplevel, empty input region | GNOME (Mutter) and others without layer-shell | click-through works, the pie is drawn inside a screen-sized buffer (memory cost, see O-8); the compositor may give it focus |
| X11 window through XWayland | any with XWayland | simplest; position of override-redirect windows and stacking above Wayland windows are compositor-dependent (spike S3) |

### Keyboard layout

`layout::char_for` and `find_key` only produce labels and the shortcut recorder's text matching. On
Linux they use `xkbcommon` with a keymap built from rule names, which is pure data and needs no display
to test. The names come from `_XKB_RULES_NAMES` on the X11 root window (exact), from
`org.freedesktop.locale1` or the adapter on Wayland (the configured layout, not the live one), else US.
With several layouts configured the first group with Latin letters is used; following the live group is
a follow-up.

### Tray icon

`tray-icon` on Linux needs GTK and a running GTK main loop, which clashes with winit owning the event
loop. The Linux tray is a **StatusNotifierItem** with a dbusmenu menu, built on the D-Bus library the
app needs anyway (single instance, portals, KWin script). Menu items are the same: Settings, About,
Welcome guide, Pause (check), Launch at login (check), Quit. The icon is the accent-colored pie, as on
Windows, because the desktop's light or dark panel is unknown.

GNOME shows no tray without the AppIndicator extension. If no StatusNotifierWatcher is on the bus,
CursorPie keeps running with no tray and offers the same actions elsewhere: desktop-entry actions
(`Settings`, `Pause`, `Quit` in the launcher's right-click menu) and `cursorpie --settings`,
`--pause`, `--quit` that talk to the running copy. The welcome window opens on first start and tells the
user where the controls are, which is the Windows page 2 idea ("where the tray icon is").

### Single instance, command line and autostart

- **One copy:** own the D-Bus name `io.github.cursorpie` (the `BUNDLE_ID` constant); without a session
  bus, `flock` a file in `$XDG_RUNTIME_DIR`. A second start sends its command line to the first and
  exits. `--settings`, `--pause`, `--resume`, `--quit` and `--doctor` (prints the capability report)
  use that channel.
- **Launch at login:** a `~/.config/autostart/cursorpie.desktop` file; "off" is the file removed, or
  `Hidden=true` written by a desktop's settings panel (read as off, like Task Manager on Windows).
  `Exec=` is the absolute path of the **installed** program: for an AppImage the path in `$APPIMAGE`,
  not the temporary mount; for a Flatpak the Background portal. Sessions with no XDG autostart
  handler (a bare Sway or Hyprland config) need `exec cursorpie`, which the docs and the welcome page
  give.
- **No self-install.** The Windows self-install exists because Windows has no package manager. Linux
  distributions do. `ensure_installed` and `handle_command_line("--uninstall")` stay no-ops on Linux.

### Launching things

- *Open app*: a desktop-entry id launches through `gio launch`; a name found on `PATH` or an absolute
  path is spawned directly in its own session; the resolution is a pure function over a desktop-entry
  index and a `PATH` list.
- *Open app, already running*: the text of the slice (an id, a program name or a path) is expanded by
  `Index::aliases` into every name the app may go by: the id of the entry it names, found by id, by
  window class, by a unique `.<name>` suffix or by the program of its `Exec` line, plus that entry's
  `StartupWMClass` and program. A window belongs to the app when its class, its instance or its canonical
  id is one of them (`toplevels::belongs_to`). Only when no window matches is the app started.
- *Roles*: the default Files, Terminal and Browser slices are "Open app" slices with the text `@files`,
  `@terminal` and `@browser` (`platform/linux/roles.rs`). `backend::role_app` asks `xdg-mime query default
  inode/directory` or `xdg-settings get default-web-browser` (a terminal comes from `xdg-terminal-exec` if it
  exists, else the first installed one of a list) and then continues as for any app id, so a window that is
  open is raised. A shell command in the same file runs when the system names nothing installed. Configs of
  0.7.0 and earlier hold these three as shell commands: `Config::sanitize` turns the unchanged ones into roles
  on Linux.
- *Open URL / file*: `xdg-open`. *Shell command*: `/bin/sh -c`, as on macOS. Processes are reaped by
  `spawn_reaped`, no zombies.
- *File chooser*: the FileChooser portal over D-Bus; `zenity` or `kdialog` if no portal answers.
  "Pick app" lets the user choose a `.desktop` file or an executable, and returns its app id.
- *Reveal*: the FileManager1 D-Bus interface (`ShowItems`), else `xdg-open` on the folder.
- *Tick sound*: `canberra-gtk-play` if installed, otherwise silent. Detected once.
- *Memory*: `malloc_trim(0)` on glibc.

### Config location and defaults

- `$XDG_CONFIG_HOME/cursorpie`, falling back to `~/.config/cursorpie`. One directory keeps the invariant
  that `config.json`, its backups, `icons/` and `welcome_shown` sit together (export, import, "Reveal"
  and first-run detection rely on it). Strict XDG would split data and state; the trade-off is
  consistency with the other two platforms. Flatpak redirects `XDG_CONFIG_HOME` by itself.
- `Export` writes to the user's download directory, read from `~/.config/user-dirs.dirs` (pure parser).
  The name is localized: on an Indonesian system it is `~/Unduhan`, not `~/Downloads`.
- `~/...` icon paths work as on macOS (both use `/`), so image icons travel between macOS and Linux
  better than between macOS and Windows.

Proposed Linux defaults (decided in phase L4, pure and tested with `Config::default_for(Os::Linux)`):

| Slice | Default | Why |
|---|---|---|
| History | Ctrl+Z / Ctrl+Shift+Z | GTK, Qt and Electron convention |
| Browser | Alt+Left / Alt+Right | as Windows |
| Tabs | Ctrl+Shift+Tab / Ctrl+Tab | as Windows |
| Clipboard, Zoom, Brackets, Scroll, Edit | Ctrl-based, as Windows | |
| Launch > Files | `xdg-open "$HOME"` | no neutral app name |
| Launch > Terminal | a `sh -c` chain: `xdg-terminal-exec`, else the first installed of a list | no standard |
| Launch > Browser | `xdg-open https://` | the default browser |
| Quick: Lock | `loginctl lock-session` | works across desktops |
| Quick: Screenshot | a `sh -c` chain over `spectacle`, `gnome-screenshot`, `xfce4-screenshooter`, `flameshot`, `grim` | no standard |
| Quick: Emoji | Super+. | GNOME and KDE both use it |
| Browsers profile ids | the alias lists from [App identity](#app-identity) | |

Shortcuts for keys a Mac keyboard does not have (Print Screen, media keys, Menu) cannot be expressed in
the macOS key code space. The defaults avoid them by using commands. Extending the key space is a
config-format question and is deliberately out of scope here.

### Permission, welcome window and Settings

`Os::needs_permission` is a constant today. On Linux it depends on the session, so it becomes a
function of the capability report. Page 2 of the welcome window and Settings > General show, per
capability, one of: nothing to do (X11), "Allow input access" with the exact fix (install the package,
or add the udev rule and log in again) and a live status that rechecks every second like the macOS
Accessibility status, or what is degraded and why. `cursorpie --doctor` prints the same report for
bug reports. All of the text is a pure function of `Os` and `Session`, tested for every combination.

## Obstacles and solutions

| ID | Obstacle | Effect | Solution | Remaining risk |
|---|---|---|---|---|
| O-1 | Wayland gives no global input | no gesture without a workaround | evdev + uinput behind a udev rule; see [Input capture](#input-capture) | permission is a real install step and a security trade-off ([Security](#security-and-privacy)) |
| O-2 | X11 and Wayland in one binary | compile-time `cfg` does not fit | run-time capabilities behind identical facade names | more code paths to test; mitigated by pure cores and Xvfb tests |
| O-3 | Per-device pointer settings on the forwarded device (**evdev only**) | after a grab, the compositor sees a *new* device, so a per-device acceleration profile, speed or "natural scroll" set for the physical mouse may not apply | the uinput clone copies name, vendor and product so keyed settings can match; documented | unresolved until spike S5; X11 tier is unaffected, which is a reason to keep it first |
| O-4 | eframe/winit cannot make a layer-shell surface | the right Wayland pie window needs a second UI shell | Phase L6: a small Wayland client (surface + EGL) that gives egui the same `RawInput`/textures; X11 window under XWayland as an interim | largest engineering item; if it fails, Tier 2 ships with the XWayland pie or the full-screen window |
| O-5 | GNOME has no layer-shell and hides pointer, window list and focus | Tier 3 | a CursorPie GNOME Shell extension (D-Bus methods for pointer, toplevels, focus); full-screen transparent window for drawing | extension API changes with each GNOME release; ships separately and is optional |
| O-6 | Wayland pointer position is unknown | pie cannot be centered | ordered strategies in [Pointer position](#pointer-position) | dead reckoning drifts; only used after the pie is placed |
| O-7 | Wayland refuses focus stealing | "bring the app to the front" impossible generically | adapter per compositor; otherwise the option is off and `target_app` is `Focused` | no adapter, no pointer-based targeting |
| O-8 | A full-screen transparent surface costs memory | 4K RGBA is about 33 MB per buffer, times the swapchain; against the "light on memory" promise | layer-shell sized to the pie; the full-screen window only on GNOME, hidden and released on sleep like all caches | GNOME users pay some memory while the pie is open |
| O-9 | No compositor on X11 | transparent window shows a black box | detect `_NET_WM_CM_S0`; XShape fallback shape; tell the user | hard edges in the fallback |
| O-10 | Tiling window managers tile or resize the pie window | pie appears in a layout | window type hints; spike S3; documented floating rules per WM | WM-specific |
| O-11 | App ids are inconsistent (`WM_CLASS`, `app_id`, desktop id, Flatpak, Snap, Electron, AppImage) | a profile for "Firefox" does not match | [App identity](#app-identity): canonicalize through desktop entries, accept aliases, several ids in defaults | apps with no desktop entry fall back to the raw class |
| O-12 | Sandboxes (Flatpak, Snap) | no `/dev/input`, no `/dev/uinput`, hidden other processes | X11 tier works in a Flatpak with the X11 socket; Wayland capture does not; deb, rpm, AppImage and a tarball are the primary formats | Flatpak is X11-only |
| O-13 | Tray is missing on GNOME; GTK clashes with winit | no controls | StatusNotifierItem on D-Bus; fallbacks: desktop actions and `--settings` | users on stock GNOME need the AppIndicator extension for the tray |
| O-14 | Extra mouse buttons have no stable numbers | the trigger cannot be one of macOS's extra buttons | fixed table in Appendix B, plus a verified list of mice; "learn a button" is a follow-up | Linux can *exceed* Windows here, but the mapping needs real mice |
| O-15 | User remaps buttons (`xinput set-button-map`) | X11 logical vs physical buttons differ | X11 uses logical numbers (what the user configured); evdev uses physical; documented | the two tiers differ for remapped mice |
| O-16 | Mouse polling at 1000 to 8000 Hz through a forwarding loop | CPU use and latency | batch by `SYN_REPORT`, one write per frame, no allocation in the loop, measure in S5 | needs a budget: under 1 percent of a core at 1000 Hz, under 1 ms added |
| O-17 | Hung tap holds an exclusive grab | the mouse is dead | watchdog, signal handlers, fail-open, kernel release on exit | a stuck kernel thread is out of our hands |
| O-18 | Grab on the wrong devices | breaks tablets, touchpads, gamepads, receivers that expose several nodes | strict capability filter, never our own devices, per-device error isolation | exotic composite devices; an allow/deny list in the config directory |
| O-19 | Lock screen, VT switch, suspend | swallowed clicks at a lock screen; stale device nodes | pause on logind lock and inactive seat; reopen on hotplug | none known |
| O-20 | Keyboard layout is not available on Wayland | wrong labels on non-US layouts | locale1 and adapters; US fallback; positions never depend on it | labels may lag a live layout switch |
| O-21 | Recording a shortcut on Wayland cannot see compositor-reserved keys | cannot record Super+Tab by pressing it | window-local recording, text entry for reserved combinations | feature gap vs macOS and Windows |
| O-22 | `Os::current()` returns `Mac` for Linux today | adding `Os::Linux` changes test results | pin tests to `_for(Os::Mac)` first, in their own commit | a missed test fails loudly on Linux CI, the intended signal |
| O-23 | glibc vs musl, old distributions | `malloc_trim` absent; binaries refuse to start | no-op on musl; build releases on an old-glibc image; AppImage | one more CI image |
| O-24 | Fractional scaling and mixed DPI | pie size or hit-testing off | X11: one global scale from the toolkit; Wayland: logical coordinates | X11 mixed DPI is not supported by X11 itself |
| O-25 | CI has no display or input devices | untested glue | Xvfb for X11 capture and injection (XTest and XInput2 work there); a headless compositor for overlay; uinput tests only on a runner that has `/dev/uinput` | evdev glue is mostly manual |
| O-26 | Dependencies and license review | the project keeps dependencies few | table in [Dependencies](#dependencies), one PR each, with a reason | maintainer decision |
| O-27 | Config and share files from macOS carry macOS bundle ids | profiles import but never match | `clean_apps_for` gains a Linux arm; imported apps are kept and shown; same behavior as Windows | users retype ids |

## Alternatives considered

| Alternative | Why not |
|---|---|
| **X11 only, tell Wayland users to use XWayland** | XWayland sees only X clients: no pointer events over Wayland-native windows, so the gesture silently fails on most of a modern desktop. |
| **libinput directly** | needs the same permissions, applies its own acceleration, and cannot swallow a button. |
| **Compositor key bindings** (Sway `bindsym --whole-window`, Hyprland `bind = , mouse:274`) that call `cursorpie --trigger down/up` | no permission needed and clean swallowing, but the user must edit each compositor's config, there is no GNOME or KDE equivalent, and replaying a quick click still needs injection. Kept as a documented manual option, not the design. |
| **XDG GlobalShortcuts portal** | keyboard shortcuts only, no mouse buttons, no hold-and-release with position. |
| **InputCapture portal** | designed for screen-edge barriers (Barrier, Input Leap), not for arbitrary global buttons. |
| **RemoteDesktop portal for injection only** | needs a consent dialog and does not remove the need for uinput to forward grabbed events. Reserved for sandboxes. |
| **Run capture as a root system service** | a bigger attack surface than a `uaccess` rule for the logged-in user, and an installation step for every user. |
| **`tray-icon` with GTK** | GTK main loop beside winit, a heavy C dependency, no gain over StatusNotifierItem. |
| **Electron or a web shell like Kando** | not this project. |

## Parity matrix

Legend: yes = same as macOS; ~ = works with a stated difference; no = not available.

| Feature | macOS | Windows | Linux X11 | Wayland, KDE or wlroots | Wayland, GNOME |
|---|---|---|---|---|---|
| Hold, flick, release, drag pass-through | yes | yes | yes | yes | yes |
| Double-click app pie | yes | yes | yes | ~ needs adapter | ~ needs extension |
| Per-app profiles | yes | ~ UWP merged | yes | ~ needs adapter | ~ needs extension |
| App under the pointer | yes | yes | yes | ~ adapter | ~ extension |
| Bring target to the front | yes | ~ best effort | yes | ~ adapter | ~ extension |
| Trigger: middle button | yes | yes | yes | yes | yes |
| Trigger: extra buttons | yes | no | ~ per Appendix B | ~ per Appendix B | ~ per Appendix B |
| Side-button modes | yes | yes | yes | yes | yes |
| Shortcuts reach the app | yes | ~ not into admin windows | yes | yes | yes |
| Record a shortcut | yes | yes | yes, global | ~ window-local | ~ window-local |
| Transparent pie | yes | yes | yes with a compositor, ~ shaped without | yes | ~ full-screen window |
| Tray | menu bar | tray | StatusNotifierItem | StatusNotifierItem | ~ needs AppIndicator extension |
| Launch at login | yes | yes | yes | yes, plus a manual line in a bare compositor | yes |
| Permission needed | Accessibility | none | none | `/dev/input`, `/dev/uinput` | same |
| Labels on non-US layouts | yes | yes | yes | ~ configured layout | ~ configured layout |

## Phases

Each phase is one or a few commits, leaves macOS and Windows unchanged and green, and writes tests first.
Phases L1 and L2 change no behavior on any OS.

| # | Phase | Exit criteria |
|---|---|---|
| 0 | This document, and the [spikes](#phase-0-spikes) | reviewed; S1 and S2 answered (below); S3 before the window hints, S4 to S6 before L5, S7 and S8 before L6 and L7 |
| L1 | **Characterization and the seam.** Pin the tests that assume `Os::current()` is `Mac` on Linux; add `Os::Linux` and every pure table: names, primary modifier, hints, app id normalization, `Session::detect`, XDG paths and `user-dirs.dirs` parser, desktop-entry parser and matcher, toplevel hit test, Linux key tables, button tables, command-line parsing | all old and new tests pass on macOS, Windows and Linux; no runtime behavior changes |
| L2 | **Linux skeleton.** `cfg(target_os = "linux")` replaces the stub on Linux (other targets keep it); `platform/linux/` with capability plumbing and unavailable-because messages; `check-linux` feature; eframe features per target; CI job that builds on Linux with the system libraries | `cargo build` on Linux produces an app that starts, reports "capabilities: none ready" and exits cleanly; macOS and Windows builds unchanged (`cargo tree` shows no new crates there) |
| L3 | **X11 backend (Tier 1).** capture, injection, apps, toplevels, displays, layout, window hints, shape fallback | Xvfb integration tests pass; manual checklist items 1 to 11 pass on Xorg under at least GNOME, KDE, XFCE and one tiling WM |
| L4 | **Desktop integration.** tray, single instance and command line, autostart, launching, file chooser, sound, memory, defaults, welcome and Settings text, README and docs, packaging (deb, rpm, tarball, AppImage) | a fresh user on X11 installs a package, sees the welcome window, and gets a working pie after login |
| L5 | **Wayland capture and injection.** evdev grab and forward, uinput, hotplug, watchdog, lock and seat handling, udev rule and doctor | no lock-out in the fault-injection tests; polling budget met; pointer settings behavior understood (O-3) |
| L6 | **Wayland pie and adapters.** layer-shell overlay, pointer strategies, KWin, Sway, Hyprland, COSMIC adapters | Tier 2 checklist passes on KDE Plasma 6 and Sway or Hyprland |
| L7 | **GNOME.** extension and full-screen fallback | Tier 3 checklist passes on a current GNOME |
| L8 | **Sign-off.** run the full checklist on the support matrix; list what was and was not verified, like "Status" in the Windows document | matrix table filled in |

Order of value: L1 to L4 give a complete Linux product for X11 users, which is the largest group that has
no workaround today. L5 to L7 are independent of each other after L5.

## Phase 0 spikes

Throwaway programs that answer one question each. None is merged.

| # | Question | Pass condition |
|---|---|---|
| S1 | Is X keycode = Linux key code + 8 on current distributions? Are Linux key codes equal to set-1 scan codes for the main block? | XTest sends the expected key on GNOME, KDE and XFCE X11 sessions; the table in Appendix A matches `linux/input-event-codes.h`. **Result:** the table matches the kernel header in full, and on Xvfb's `evdev` keymap keycode = code + 8 for A, Z, Return, Left, Ctrl, Shift, Alt and Super (F13 reports `XF86Tools`, as the physical key does). Not yet seen on GNOME, KDE and XFCE sessions. |
| S2 | Can a swallowed press be handed back to the app, for a click and for a drag? | a decision. **Result (Xvfb):** after the physical release, lifting the grabs and sending press and release works and the next press is grabbed again. While the button is still down, the sent press was dropped by the server (the button was already down; on Xvfb the hardware and the injected events share one device, so this is not conclusive for a real mouse). A synchronous grab with `XIAllowEvents(ReplayDevice)` hands the press back perfectly, but it freezes the pointer (its position did not change until released), so it cannot be held for the 500 ms hold. **Decision:** asynchronous grab, quick clicks are reliable, a drag handed back may lose its start; revisit on real hardware. |
| S3 | What do the window hints produce on GNOME, KDE, XFCE, i3, bspwm, awesome, openbox? Does an override-redirect window under XWayland sit above Wayland windows? | for each: floats, no focus, correct position, always above, no frame |
| S4 | Does `EVIOCGRAB` plus a cloned uinput device keep the desktop's acceleration, speed and natural-scroll settings on GNOME, KDE, Sway? | per compositor: identical or not; if not, the documented workaround |
| S5 | CPU and latency of the forwarding loop at 1000 and 8000 Hz; scrolling with hi-res wheels | within the budget in O-16; no duplicated or lost scroll |
| S6 | Can the pointer position be had on press on KDE (script), Hyprland (IPC), Sway (probe surface), GNOME (extension)? How late? | position known within the 500 ms hold, error under 2 px |
| S7 | Can a layer-shell surface carry egui through EGL without winit? How much code and memory? | a pie drawn at a margin position on KDE and Sway at 60 fps |
| S8 | On GNOME, does a full-screen transparent toplevel with an empty input region keep the focused window focused and let clicks through? | yes or no, per GNOME version |

## Test strategy

- **Characterization first (L1).** Existing constants are asserted literally before `Os::Linux` exists.
  Linux values are asserted literally in new tests, parametrized on `Os` as in the other two ports.
- **Pure tables get consistency tests:** no duplicate codes, every mapped key round-trips
  (`mac code -> Linux code -> mac code`), every named special key has a Linux code, the Appendix A values
  equal the kernel header's.
- **Pure decisions get fixtures:** desktop entries (Firefox, Chrome, a Flatpak, a Snap, an Electron app
  with an odd `StartupWMClass`), `user-dirs.dirs` including a localized one, environment maps for each
  session in Appendix C, toplevel lists with overlapping, minimized and other-workspace windows.
- **Xvfb integration tests (L3).** The X11 backend runs against Xvfb: injection through XTest is read
  back through XInput2, grabs and the ungrab and re-grab sequence are exercised, EWMH properties are
  set by a fake window manager fixture. This is a stronger check than the Windows port had.
- **Headless compositor tests (L6).** A headless wlroots compositor for layer-shell and the Sway IPC
  adapter. KWin, GNOME and Hyprland are manual.
- **uinput tests** run only on a runner with `/dev/uinput`; elsewhere they are skipped with a message,
  never silently green.
- **Fault injection for grabs (L5):** kill the tap thread, stall it, send `SIGKILL`: the mouse must
  recover each time.
- **CI** (the workflow file is not in this repository snapshot; its change is part of L2): a Linux job
  builds with the system libraries, runs `cargo test`, `cargo clippy -D warnings` and
  `cargo check --features check-windows`; a separate job runs `cargo check --features check-linux` on
  macOS and Windows; `cargo tools ci` gains the Linux step. Rule from `CONTRIBUTING.md` holds: zero
  warnings, and `cargo tools ci` is the single definition of "the checks pass".

## Manual verification checklist

**Status: under internal observation.** Nothing below is signed off. When an item is checked, write
the result in the pull request or issue with the date, distribution, desktop, session type, GPU and
scaling, and move the item to "verified" here only when it passed on more than one setup.

Items 1 to 11 are the ones in `WINDOWS_MIGRATION.md`, run on Linux. Items 12 and later are specific to
Linux. Record the distribution, desktop, session type, GPU and scaling with each result.

1. Hold the middle button for the hold time (150 ms by default): pie opens under the cursor; release over a slice runs it.
2. Quick middle click passes through, and in a browser the click does **not** paste the primary
   selection twice; middle-drag still pans.
3. Double-click opens the app's own pie.
4. Back and forward send the active mode's shortcuts; right-click while the pie is open switches pies.
5. Settings: record a shortcut (Esc cancels), undo and redo, save, backup restore, import and export.
6. A shortcut slice reaches the app under the pointer.
7. Tray or its fallback: Settings, About, Pause, Launch at login (log out and in), Quit.
8. Non-US layout (QWERTZ, AZERTY): labels and presets match the layout.
9. One and two monitors, 100, 125, 150 and 200 percent scale.
10. Idle for the sleep time, then open the pie.
11. The pie and the Settings margin are see-through over a bright window, with no box, border, shadow
    or flash.
12. Session matrix: Xorg and Wayland on GNOME, KDE, XFCE, Sway, Hyprland; the capability report in
    Settings > General matches what works.
13. No compositor on X11: the shaped fallback appears and Settings says so.
14. A tiling window manager does not tile the pie.
15. Wayland: with the udev rule absent, the welcome page says how to fix it and the status turns to ready
    without a restart once it is.
16. Wayland: unplug and replug the mouse, suspend and resume, lock and unlock, switch VT: the mouse is
    never stuck and the gesture works again.
17. Wayland: the mouse's own acceleration, speed and scroll direction feel the same with CursorPie running
    and stopped.
18. Wayland: a hi-res scroll wheel scrolls once per notch.
19. Kill CursorPie with `SIGKILL` and with `SIGSTOP` while a button is held: the mouse recovers.
20. A Flatpak, a Snap and an AppImage of the same browser all match one profile.
21. No tray on stock GNOME: `cursorpie --settings` works and the welcome window explained it.
22. A game or app with pointer lock still receives smooth relative motion.
23. Extra buttons as trigger on at least two mouse models, per Appendix B.
24. `cursorpie --doctor` output is complete enough for a bug report.
25. Settings open on another workspace, minimized, or behind another app: pressing or holding the trigger
    brings it forward and goes to its workspace (`CURSORPIE_DEBUG=1`, then `cargo tools diag`, shows a refusal).

## Security and privacy

- **X11:** any client can already do everything CursorPie does. Nothing new is exposed.
- **Wayland, input permission:** the udev rule gives the logged-in user's processes access to pointer
  devices and to `/dev/uinput`. Reading pointer devices exposes button and motion events of the whole
  session to any process of that user; writing to uinput lets any such process type into any window.
  That is a weaker boundary than Wayland normally gives. The rule is therefore:
  - limited to pointer devices (`ID_INPUT_MOUSE`) and `uinput`, never keyboards;
  - granted through `uaccess` (the active local seat's user), not the broad `input` group;
  - written in the docs in plain words, including how to remove it.
- **Keyboards are not opened** on Wayland, so the README promise ("key events are never stored or logged,
  and are only looked at to record a shortcut") remains true by construction.
- **Command actions** still run shell commands with the user's permissions. The import warning stays.
- **No network.** The D-Bus calls are local. Nothing in this design adds a network connection.
- The GNOME extension is a separate, optional download that runs inside the shell; its code is reviewed
  as such and exposes only pointer position, window list and focus on the session bus, to the same user.

## Packaging

| Format | Notes |
|---|---|
| `.deb`, `.rpm` | built from the release binary; install the desktop entry, icons, AppStream data, the udev rule and a `modules-load.d` entry for `uinput` |
| Tarball with `install.sh` | for other distributions; same files under a prefix |
| AppImage | `Exec=` for autostart is the `$APPIMAGE` path; no udev rule (needs root); X11 tier works at once, Wayland needs the rule installed separately |
| Flatpak | X11 tier only (O-12) |
| AUR, Nix | community packages, welcome but not part of the plan |

Files live under `packaging/linux/` (Appendix D). `scripts/package-linux.sh` mirrors
`scripts/package-windows.ps1` and `scripts/make-dmg.sh`: it reads the version from `Cargo.toml` and
writes into `target/bundle/`. Release steps in `CONTRIBUTING.md` gain the Linux artifacts. The release
build runs on an image with an old glibc so the binary starts on current LTS distributions.

## Dependencies

The project keeps dependencies few. These are the proposals, each a separate pull request with its
reason; license and maintenance status are confirmed in that pull request.

| Crate | Phase | Used for | Why not std |
|---|---|---|---|
| ~~`x11rb`~~ | L3 | not used: hand-written bindings to libX11, libXi, libXtst and libXrandr instead (see [Implementation status](#implementation-status)) | |
| `xkbcommon` | later | layout labels from rule names | needs the keymap compiler |
| `zbus` (blocking) | L4 | single instance, portals, StatusNotifierItem, dbusmenu, KWin script, locale1, logind | no D-Bus in std; pure Rust, no libdbus |
| a StatusNotifierItem helper (for example `ksni`) | L4 | the tray | optional; could be written on `zbus` |
| `evdev` | L5 | device enumeration, grabs, uinput devices | raw ioctls are error-prone |
| `rustix` or `libc` | L4, L5 | `flock`, `malloc_trim`, signals, `poll` | not in std; likely already in the tree |
| `wayland-client` and `smithay-client-toolkit` | L6 | layer-shell, `xdg-output` | the Wayland protocol |

macOS and Windows builds must show no new crates: every one is under
`[target.'cfg(target_os = "linux")'.dependencies]` or behind `check-linux`.

## Documentation changes

Done with the X11 release:

- `README.md` (title, tagline, platform table), `docs/INSTALL.md` (Linux install and packages),
  `docs/KNOWN_ISSUES.md` (the Linux limits) and `docs/USER_GUIDE.md` (Linux notes).
- `CONTRIBUTING.md`: prerequisites per distribution, build table, what each OS can check.
- `Cargo.toml` description, `about.rs` tagline, `docs/UI_DESIGN.md` ("Transparent window on Linux").
- `docs/WINDOWS_MIGRATION.md`: the non-goal "Linux support" points here.
- `CHANGELOG.md`: the Linux entries under `## Unreleased`.

Still to do, with the phases that need them:

- `docs/INSTALL.md` and `docs/KNOWN_ISSUES.md`: packages (deb, rpm, AppImage) and the udev rule once Wayland capture
  exists, and the parity matrix, honestly.
- `src/lib.rs`, `src/platform/mod.rs`, `src/tap/mod.rs`, `src/guides/architecture.rs`: the D-Bus and watchdog
  threads and the capability table, when they are built.

## Open decisions for the maintainer

1. Ship X11 (L1 to L4) as the first Linux release and Wayland later? This document recommends yes.
2. Is the udev rule acceptable as the Wayland permission model, given the trade-off above?
3. Which dependencies in the table are acceptable?
4. Is the GNOME extension in scope, as a separate repository or directory?
5. Is Flatpak (X11 only) worth shipping?
6. Minimum supported glibc and distributions for the release build.
7. Whether config keys for keys a Mac lacks (Print Screen, media) justify a future format change.

## Appendix A: key code table

Verified against `linux/input-event-codes.h` (all rows match) and by the unit tests in
`platform/keymap.rs`. The macOS
code is the one stored in `config.json`; the Linux code is an evdev key code (X11 keycode minus 8).

Layout keys: Linux key code equals the set-1 scan code already in `LAYOUT_KEYS`
(for example A 30, S 31, Z 44, Q 16, 1 2, 0 11, the ISO extra key 86).

Special keys:

| Key | macOS | Linux | Key | macOS | Linux |
|---|---|---|---|---|---|
| Return | 36 | 28 | Home | 115 | 102 |
| numpad Enter | 76 | 96 | End | 119 | 107 |
| Tab | 48 | 15 | Page Up | 116 | 104 |
| Space | 49 | 57 | Page Down | 121 | 109 |
| Backspace | 51 | 14 | Delete | 117 | 111 |
| Esc | 53 | 1 | Insert (mac Help) | 114 | 110 |
| F1 to F10 | 122, 120, 99, 118, 96, 97, 98, 100, 101, 109 | 59 to 68 | Left | 123 | 105 |
| F11, F12 | 103, 111 | 87, 88 | Up | 126 | 103 |
| F13 to F20 | 105, 107, 113, 106, 64, 79, 80, 90 | 183 to 190 | Right | 124 | 106 |
| numpad 0 to 9 | 82 to 89, 91, 92 | 82, 79, 80, 81, 75, 76, 77, 71, 72, 73 | Down | 125 | 108 |
| numpad `*`, `+`, `-`, `.`, `/` | 67, 69, 78, 65, 75 | 55, 78, 74, 83, 98 | Volume up, down, mute | 72, 73, 74 | 115, 114, 113 |

Modifiers: Shift 42, Ctrl 29, Alt 56, Super (`MOD_CMD`) 125 (left keys). Numpad Clear has no certain
counterpart and is left unmapped until verified.

## Appendix B: mouse button table

Draft, to be verified on real mice (O-14, checklist item 23). Config and state machine use the macOS
numbers (0 left, 1 right, 2 middle, 3 back, 4 forward, 5 to 7 extra).

| Meaning | Number | evdev code | X11 (logical) |
|---|---|---|---|
| left | 0 | `BTN_LEFT` | 1 |
| middle (trigger) | 2 | `BTN_MIDDLE` | 2 |
| right | 1 | `BTN_RIGHT` | 3 |
| back | 3 | `BTN_SIDE` | 8 |
| forward | 4 | `BTN_EXTRA` | 9 |
| extra 5, 6, 7... (up to 15) | 5, 6, 7... | `BTN_FORWARD`, `BTN_BACK`, `BTN_TASK`, then the codes up to `0x11f` | 10, 11, 12... (button + 5), as xf86-input-libinput numbers them. Used by the extra buttons of a mode; **not offered as a trigger**: Linux offers only the middle button for now |

Some mice put their thumb buttons on `BTN_FORWARD` and `BTN_BACK` instead. The table is the default; the
**Scan this mouse** control of Settings (General, Mouse) is the "learn a button" fix: it listens for raw button
presses (XInput2 `RawButtonPress`) and stores what was pressed in `mice.json`. The buttons a mouse has come from
the `KEY` bitmap of its entry in `/proc/bus/input/devices`; only entries that have `BTN_LEFT`, `REL_X` and
`REL_Y` count, so touchpads, tablets and keyboards are left out, and so are virtual devices of programs
(`ydotool`, `uinput`).

## Appendix C: session detection

| Signal | Meaning |
|---|---|
| `XDG_SESSION_TYPE=x11` or `wayland` | the display server (first choice) |
| `WAYLAND_DISPLAY` set | a Wayland compositor is reachable |
| `DISPLAY` set | an X server or XWayland is reachable |
| `XDG_CURRENT_DESKTOP`, `XDG_SESSION_DESKTOP` | desktop names, colon-separated, may be several (`ubuntu:GNOME`) |
| `KDE_FULL_SESSION` | KDE |
| `SWAYSOCK` | Sway IPC |
| `HYPRLAND_INSTANCE_SIGNATURE` | Hyprland IPC |
| `CURSORPIE_BACKEND` | forces `x11` or `wayland` |
| `XDG_RUNTIME_DIR` | runtime sockets and the lock file |
| `container`, `FLATPAK_ID`, `SNAP`, `APPIMAGE` | packaging environment (paths, sandbox limits) |

The list is an input to `Session::detect`; the capability probe, not the names, makes the final decision.

## Appendix D: new files

This new file, up realese in version **0.5.0**..

```text
docs/LINUX_MIGRATION.md                this document
src/platform/linux/                    L2+  capability plumbing, facade functions, session, doctor
  session.rs                           L1   Session::detect (pure)
  desktop_entry.rs                     L1   desktop entry parser, index, app id matching (pure)
  xdg.rs                               L1   config dir, user-dirs.dirs (pure)
  toplevels.rs                         L1   hit test over a window list (pure)
  x11/                                 L3   apps, input, displays, layout, window hints
  wayland/                             L5+  evdev, uinput, adapters (kwin, sway, hyprland, gnome), layer-shell
  autostart.rs, shell.rs, instance.rs  L4
src/platform/keymap.rs                 L1   gains the Linux tables
src/tap/linux/                         L3+  x11.rs, evdev.rs (event glue; the state machine is untouched)
src/ui/tray.rs                         L4   gains a StatusNotifierItem arm
packaging/linux/                       L4   cursorpie.desktop, AppStream metadata, icons, 70-cursorpie.rules, uinput.conf
scripts/package-linux.sh               L4
extras/gnome-shell-extension/          L7   optional, separate release
```
