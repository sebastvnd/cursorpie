# Wayland support: research and plan

**Status: research, plan, and an implementation of W1, W2 and W4 (7 November 2026). The code compiles, `cargo tools ci`
passes (clippy, 680+ tests), and it has not been run on a Plasma 6.8 machine.** What was written, what was not, and what rests on an assumption is
listed in [Implementation status](#implementation-status).

Every claim carries one of three tags, so a reader can tell how far to trust it:

- **[src]** comes from a source outside this repository (listed under [Sources](#sources)).
- **[code]** was read in this repository (file named).
- **[assume]** is a design assumption. A [spike](#spikes) decides it. Until then, do not build on it.

Research date: 7 November 2026. This page adds to [LINUX_MIGRATION.md](LINUX_MIGRATION.md), which already designs
Wayland (phases L5 to L7) in general terms. This page does three things that document does not: it records *why* the
work is urgent now, it narrows the design to **KDE Plasma 6.8** as the first Wayland target, and it defines what
"behaves like the other platforms" means, so the result can be checked.

## Contents

- [Summary](#summary)
- [Implementation status](#implementation-status)
- [Why this is needed now](#why-this-is-needed-now)
- [What CursorPie does in a Plasma 6.8 session today](#what-cursorpie-does-in-a-plasma-68-session-today)
- [Research: what Plasma on Wayland offers](#research-what-plasma-on-wayland-offers)
- [Decisions](#decisions)
- [The parity contract](#the-parity-contract)
- [Architecture](#architecture)
- [Plan](#plan)
- [Other compositors (W5)](#other-compositors-w5)
- [Spikes](#spikes)
- [Risks](#risks)
- [Security and permissions](#security-and-permissions)
- [Test strategy](#test-strategy)
- [Manual checklist for Plasma 6.8](#manual-checklist-for-plasma-68)
- [Documentation changes](#documentation-changes)
- [Open decisions for the maintainer](#open-decisions-for-the-maintainer)
- [Sources](#sources)

## Summary

1. **The cause.** KDE Plasma 6.8 (released 14 October 2026) no longer offers an X11 session. X11 *applications* still
   run, through XWayland, but the desktop is Wayland only [src]. CursorPie's Linux backend is an X11 program. In a
   Wayland session it can only see X11 windows, so on Plasma 6.8 the gesture silently does nothing over most of the
   screen [code: `tap/linux.rs`, `platform/linux/session.rs`].
2. **X11 support stays.** Plasma 6.7 and older, XFCE, i3, and every other X11 session keep working through the
   existing backend, untouched. Wayland is added beside it, chosen at run time.
3. **The hard parts** are the same five that `LINUX_MIGRATION.md` lists: seeing and swallowing the trigger button,
   sending keys and clicks, knowing where the pointer is, knowing which app is where, and putting the pie under the
   cursor. On Plasma each has a known mechanism (table in [Research](#research-what-plasma-on-wayland-offers)).
4. **Behaving like the other platforms** is achieved by keeping the whole gesture state machine
   (`tap::dispatch`, `Ev`, `Verdict`, every timer) exactly as it is, and writing one new *input layer* that feeds it
   the same events the X11 layer does. Where Wayland cannot match a behavior exactly, the
   [parity contract](#the-parity-contract) says what the substitute is and how to check it.
5. **Order of work.** First the input layer and the KDE adapter, with the pie window still going through XWayland
   (the window code that exists today). A native layer-shell pie window is a later, optional step, decided by a spike.
   Seven milestones, W0 to W6, are in [Plan](#plan).

## Implementation status

State of the repository after the second Wayland patch. "Written" means the code and its tests are in the tree.
Everything compiles and every test and `cargo clippy -D warnings` passes on Linux (and the Windows backend still
type-checks), but **nothing was run on a real Plasma 6.8 session**: the first step now is the spikes (W0), and the
second the manual checklist. The `zbus` calls were checked by the compiler, not against a running KWin.

### Written

| Milestone | Item | Where | Tests written |
|---|---|---|---|
| W1 | `EVIOCGRAB`, device id, name and capability bitmaps, generic `ioctl` helpers | `platform/linux/evdev_io.rs` | ioctl numbers against the kernel's (x86-64) |
| W1 | Virtual device through `/dev/uinput`: clone of a mouse (name, ids, every button and every relative axis, so hi-res and horizontal wheels are copied), virtual keyboard, event codec, button numbering (BTN codes to the macOS numbers, same as `inputdev`) | `platform/linux/uinput.rs` | clone description from fixture bitmaps, `UI_DEV_SETUP` bytes, codec, button tables, ioctl numbers; one test that skips without `/dev/uinput` |
| W1 | Which mice may be grabbed: relative X and Y, left and middle button, no absolute axes, never a device a program made (`/devices/virtual/`, so the clone is never read back) | `platform/linux/inputdev.rs` (`parse_mouse_nodes`) | fixtures: gaming mouse, clone, touchpad, keyboard, tablet, ydotool |
| W1 | The event layer: engine (pure) and loop. Same `Ev` and `Verdict` as the X11 layer, `Keep` writes the event to the clone, `Drop` writes nothing; a press the state machine hands back is released through the same clone; motion always forwarded and turned into `Kind::Drag` while the trigger is held; hotplug rescan every second; pause switch lets go of the mice; clone settles 400 ms before the grab; watchdog releases every grab if the loop is silent for 250 ms; a panic in the engine forwards the events untouched | `tap/wayland.rs` | recorded streams against a fake state machine: motion and wheel unchanged, swallowed trigger, quick click, drag hand-back, side buttons, extra buttons only when used or scanned, `SYN_DROPPED`, frame boundaries, same `Ev` sequence as the X11 layer |
| W1 | Shortcuts through the virtual keyboard, same order and pacing as X11 | `platform/linux/wayland/mod.rs` | order, Linux codes without the +8 offset |
| W1 | Capability report (what works, the exact fix if `/dev/uinput` or `/dev/input` is not usable), and the probes | `platform/linux/wayland/mod.rs` | one test per combination, like the macOS and Windows permission texts |
| W1 | Udev rule, shipped in the Linux tarball | `packaging/linux/70-cursorpie.rules`, `scripts/package-linux.sh` | none |
| W1 | Backend choice at start-up (`wayland::active`), compositor name in `Session`, `problem()` text | `platform/linux/session.rs`, `tap/mod.rs` | compositor from `XDG_CURRENT_DESKTOP` |
| W1 | Facade arms: the Wayland answer first, the X11 code as fallback | `platform/linux/backend.rs` | none |
| W2 | KWin script (text generated at run time, four jobs only, no user text), range-checked parsers for what it reports, window hit test and app id on the existing pure helpers, command queue (`arm`, `disarm`, `activate:<id>`), state | `platform/linux/wayland/kwin.rs` | script has no unfilled constant and nothing forbidden; reports: bounds, clipping, bad ids refused, too large refused; one profile id for a native and an XWayland window of one app; hit test; commands collected once |
| W2 | D-Bus service and script loading (`zbus`): owns the name, serves `Report` and `Pointer`, loads the script from `$XDG_RUNTIME_DIR`, checks every two seconds that it is still loaded, invokes the script's shortcut when asked | `platform/linux/wayland/bus.rs` | none (needs a session bus) |
| W2 | Settings: the General tab shows the capability report in a Wayland session; the welcome page title no longer says "X11 session" | `ui/settings/tabs/general.rs`, `ui/settings/welcome.rs` | none |
| W2 | Where the pie and the Settings card go under XWayland: the position (center of the pie, center of the card) is mapped from the KWin output it is on to the X monitor of the same screen, linearly, so it is exact at any scale and with mixed scales, and does not assume what either side's scale is; screens that cannot be matched (other count, other shape) keep today's behavior | `platform/linux/wayland/coords.rs` (`to_window_point`), `platform/linux/backend.rs` (`window_point`, identity on every other backend), `ui/app/windows.rs` | identity at equal scales, 150% with an unscaled X11, two screens of different scales, lists in any order, a point off every screen, unmatched lists left alone |
| W2 | Where the pointer is at the press: the trigger asks the compositor for the pointer and waits at most 25 ms for the answer (`kwin::sample_pointer`), other routed buttons take the latest answer, and the X server's pointer is only the last resort; the engine's own sum of mouse motion is no longer the source of the pie's place. The distance of the pointer from the center of the pie is converted to the toolkit's points before it is compared with the slices, with the same monitor match as the placement | `tap/wayland.rs` (`Engine::anchor`), `platform/linux/wayland/kwin.rs` (`sample_pointer`), `wayland/coords.rs` (`scale_at`), `platform/linux/backend.rs` (`window_scale`, 1 on every other backend), `tap/resolve.rs` | first press from the compositor, a pointer moved by another device, a late answer, only the trigger waits, scale 1.5 and 2, screens that cannot be matched |
| W2 | Welcome page, second step, in a Wayland session: the two steps of the udev rule and a live status (mouse access, mouse capture, KWin adapter) that follows `wayland::report` and turns to ready by itself; the stale "log in with an X11 session" advice is gone | `ui/settings/welcome.rs` | the texts (rule file name, length, no X11-only advice); the page is drawn by the existing smoke tests (they run on the host's session) |

| W1 | Pause on lock screen, seat change and VT switch: `org.freedesktop.login1.Session` `Active` and `LockedHint`, read every 500 ms; while the session is away every grab is lifted (same path as the pause switch); an unreachable logind means "not away" (fail open) | `platform/linux/wayland/logind.rs`, `tap/wayland.rs` | the decision (`away`) |
| W2 | Shortcut recording: from the Settings window's own key events (egui reports the modifier keys, Super included, and the key by position), so no keyboard is opened and the udev rule needs nothing for it; plain Esc cancels; the order the modifiers were pressed in is kept | `ui/settings/wayland_keys.rs` | the recorder, the key table against the Linux key table |
| W4 | Trackpad on Wayland: while a hold runs (or the tail of its scroll is open) the touchpad's node is grabbed, so the extra finger's click and the two-finger scroll never reach the app; the reader goes on reading it; never held for more than 15 s; released when the reader stops; **while held, the first finger's travel moves the pointer through a virtual mouse (`PadPointer`), because the grab also takes the pad's own pointer motion; no `/dev/uinput` access means no grab** | `tap/touch/evdev.rs` (`PadGrab`) | the take/give policy and the cap |
| Build | `zbus` is an ordinary Linux dependency (pure Rust, `async-io`, no system library); `Cargo.lock` has it | `Cargo.toml` | none |
| Log | the `wayland` category | `tools/src/lint.rs`, `docs/DEBUGGING.md` | the lint |

`CURSORPIE_BACKEND=x11` still selects the X11 layer in a Wayland session, and the X11 backend's own code and tests were
not changed (D1).

### Written in the third pass (not run, not compiled where it was written)

| Milestone | Item | Where | Tests written |
|---|---|---|---|
| W5 | Choice of adapter per compositor, tolerant JSON readers (`adapter.rs`); a shared poller thread, Linux only (`poller.rs`) | `platform/linux/wayland/adapter.rs`, `poller.rs` | the choice, the labels, the readers |
| W5 | Hyprland: socket path (signature checked), parsers of `j/clients`, `j/monitors`, `j/activewindow`, `j/cursorpos`, the snapshot (order, hidden windows, logical size), the focus command (`hyprland.rs`); the socket client, Linux only (`hyprland_io.rs`) | `wayland/hyprland.rs`, `hyprland_io.rs` | fixtures in the shape of `hyprctl -j`, odd and hostile input, hit test |
| W5 | Sway: the i3 IPC framing, the `GET_TREE` walk (native and X11 windows, floating, full screen, scratchpad, outputs), the focus command (`sway.rs`); the socket client, Linux only (`sway_io.rs`); no pointer, so the focused app stands in for "app under the cursor" | `wayland/sway.rs`, `sway_io.rs`, `platform/linux/backend.rs` | framing, a tree fixture, order, depth limit |
| W5 | GNOME: the extension, the names shared with the program, the `Sync` call and the "is it running" check; enable steps in the report | `packaging/linux/gnome-extension/`, `wayland/gnome.rs`, `wayland/bus.rs` | the two sides agree on every name; the extension reads and starts nothing; syntax-checked with `node` |
| W5 | One report text per adapter; the status row of Settings and the welcome page uses the adapter's name | `wayland/mod.rs`, `ui/settings/welcome.rs` | one test per adapter |
| W5 | `Session` finds Hyprland and Sway by their own variables when `XDG_CURRENT_DESKTOP` is empty | `platform/linux/session.rs` | yes |

### Not written

| Milestone | Item | Effect today |
|---|---|---|
| W0 | **All spikes** (SW1 to SW10). No decision D2 to D8 was confirmed or changed | every [assume] in this page is still one |
| W1 | Fault-injection test (kill -9, hang, panic, unplug, suspend; SW9) | untested: the mouse-never-dead claim is by design, not measured |
| W1 | Latency and CPU measurement of the loop at 1000 and 8000 Hz (SW3) | unknown |
| W1 | Keeping per-device pointer settings on the clone (SW2) | the clone copies name and ids; whether KDE applies speed, acceleration and natural scroll to it is unknown |
| W2 | Size of the pie under fractional scaling (SW1): the window is placed with the conversion below, but its size and the hover distances are still in one unit each (slice radius in the toolkit's points, pointer distance in KWin's), so at a ratio other than 1 the slices answer to a pointer that moves a little more or less than they are drawn | measure at 150% and 200% with and without the "scale X11 applications" setting |
| W2 | The pie taking the pointer during a mouse hold so the app below hears no hover (SW5) | the app below sees hover and enter/leave events while the pie is open |
| W2 | Exact hover position during a hold | between two answers of KWin the position is the answer plus the raw motion since (D6 allows this only briefly); how far it drifts is not measured |
| W2 | Open-animation, focus and stacking of the XWayland pie above native windows (SW1) | unverified |
| W3 | Native layer-shell pie window | not started (optional) |
| W4 | Trackpad on Wayland, still open: the compositor is not told that the fingers lifted while the pad was held, so the first touch after a trackpad pie may move the pointer once (a kernel grab has no "release" to forward); measure it (SW4) | the pad is swallowed correctly, with one possible pointer jump afterwards |
| W5 | COSMIC, niri and other compositors: no adapter | capture and shortcuts work, per-app features are off, and the report says so (see [Other compositors](#other-compositors-w5) for why each is missing) |
| W5 | Hyprland, Sway and GNOME adapters, **written but not run on any of them** | see [Other compositors](#other-compositors-w5) |
| W6 | `.deb`, `.rpm`; `install.sh` installing the udev rule (it only prints the command); the matrix of the checklist | none |
| Docs | `INSTALL.md`, `KNOWN_ISSUES.md`, `README.md`, `USER_GUIDE.md` now describe the Wayland layer as written but unverified; they drop "experimental" only once the checklist passes | final wording after the checklist |

### Written from documentation, not confirmed (look here first if something fails)

- **`logind.rs`**: `org.freedesktop.login1.Session` at `/org/freedesktop/login1/session/auto`, properties `Active` and
  `LockedHint` read through `org.freedesktop.DBus.Properties` (no cache, because the signals come from the real
  session path and not from `auto`). That KDE's screen locker sets `LockedHint` is from its documentation.
- **`bus.rs`**: `org.kde.kwin.Scripting` (`loadScript`, `isScriptLoaded`, `start`), the per-script object
  `/Scripting/Script<id>` with `run`, and `org.kde.kglobalaccel.Component.invokeShortcut` on `/component/kwin`.
  The shortcut call may take another argument list on Plasma 6.8. The zbus calls were written for version 5 without a
  compiler.
- **The script**: the KWin 6 property names (`workspace.stackingOrder`, `activeWindow`, `cursorPos`, `screens`,
  `currentDesktop.x11DesktopNumber`; `window.internalId`, `resourceClass`, `desktopFileName`, `frameGeometry`,
  `desktops`) and the signals it connects (`cursorPosChanged`, `windowActivated`, `windowAdded`, `windowRemoved`,
  `stackingOrderChanged`). A missing property leaves its field empty; a missing signal is skipped.
- **Freshness**: the window list is as old as the last report (on window and stacking changes, throttled to 200 ms,
  and on every press through the shortcut). A window moved by hand is not reported until the next one; the app under
  the pointer can be wrong for a moment.
- **The clone**: that the desktop opens a new virtual device within 400 ms, and that a clone with the mouse's own name
  and ids is accepted.
- **Udev rule**: that `TAG+="uaccess"` on `/dev/uinput` and on mouse nodes is enough on Kubuntu, Fedora KDE and
  Arch (SW10).
- **Mice with absolute axes** on the same node are left alone (a clone would not carry them): a mouse that exposes
  them is not captured.

### Behavior to expect on a first run

Plasma 6.8 with the udev rule: the middle button should open the pie, a quick click should
reach the app, and "app under the cursor" should work for native and XWayland windows. If the KWin adapter does not start (no session bus, or KWin refuses the script), capture and
shortcuts still work and the report says per-app features are off. Without the udev rule nothing is touched and the General
tab says what to install.

## Why this is needed now

**Plasma stopped offering X11.** KDE announced in November 2025 that Plasma 6.8 would be Wayland only [src: Phoronix,
OMG! Ubuntu]. Plasma 6.8 was released on 14 October 2026 without an X11 session at login; 6.8.1 and 6.8.2
(27 October) followed [src: Linux Journal, Linuxiac]. The reasons KDE gave: the large majority of its users already
use the Wayland session (over 95% of Plasma 6.6 users, by KDE's own metrics [src: hwbusters]), and dropping the X11
session lets the developers work faster and build features that only make sense on Wayland [src: OMG! Ubuntu,
Phoronix]. Plasma 6.7 is the last release with an X11 session, and it is expected to get extra bug-fix releases into
early 2027 [src: Phoronix]. GNOME 50 removed its X11 session earlier in 2026 [src: OMG! Ubuntu], and Kubuntu 25.10
already stopped installing the Plasma X11 session by default [src: OMG! Ubuntu].

So the people who need Wayland support are not a small group on an unusual setup. They are everyone who runs a current
Plasma or GNOME, and the number grows with every distribution upgrade. X11 remains important for older releases, LTS
distributions and light desktops, which is why it must not be touched.

**Why an X11 program cannot do the job on Wayland.** This is by design, not a bug in CursorPie. The Wayland protocol
gives a client no way to watch input meant for other windows, to ask where the pointer is, to move its own window, or
to list other programs' windows [src: `LINUX_MIGRATION.md`, "Why Linux is different"; winit documentation]. XWayland is a
compatibility layer. It hands an X client the X clients' windows and the pointer events that happen over them, and
nothing else. The XInput2 grab, `XQueryPointer`, the EWMH window list and XTest that the X11 backend uses are all
answered by XWayland for X clients only [assume: confirm in SW1]. This is the situation the `LINUX_MIGRATION.md` table
"Alternatives considered" rejected as the long-term answer.

## What CursorPie does in a Plasma 6.8 session today

All read from the code of version 0.8.4:

- `Cargo.toml` enables only the `x11` window system of eframe on Linux, on purpose: "in a Wayland session the window
  then runs through XWayland, where it can be placed under the pointer" [code].
- `Session::detect` already tells a Wayland session from an X11 one, and `Session::problem()` returns a sentence for
  it. `tap::linux::spawn` only prints that sentence and starts the X11 tap anyway [code: `tap/linux.rs`]. The
  X11 tap then connects through `DISPLAY` (XWayland), so it works over X11 windows only.
- `platform/linux/backend.rs` answers every question (`frontmost_bundle_id`, `bundle_id_at`, `cursor_pos`,
  `display_rects`, `press_shortcut`, `post_mouse`, `activate_app`) by calling `x11::conn()` [code]. Shortcuts and
  clicks are therefore sent to X clients only.
- Settings and the welcome window tell the user to "log in with an X11 session" [code: `ui/settings/tabs/general.rs`,
  `ui/settings/welcome.rs`]. On Plasma 6.8 that advice cannot be followed.
- The touchpad reader (`tap/touch/evdev.rs`) reads `/dev/input/event*` without grabbing it, so *seeing* fingers does
  not depend on the display server. The parts that swallow the extra finger's click and the two-finger scroll use X
  button grabs [code: `tap/linux.rs`, `tap/touch/extra.rs`]. `--touchpad-debug` tells a Wayland-only user that the
  mouse layer needs X11 [code: `tap/touch/evdev/debug.rs`].

What already works and can be reused as it is: the state machine; the Linux key table (`platform/keymap.rs`, which
holds Linux key codes; the X11 backend adds 8); desktop-entry parsing and app-id matching; the pure window hit test
(`platform/linux/toplevels.rs`); icon lookup; XDG autostart; the single-instance lock; and the evdev helpers
(`platform/linux/evdev_io.rs`, `platform/linux/inputdev.rs`).

## Research: what Plasma on Wayland offers

The first column is something CursorPie needs. The last says how sure we are.

| Need | Mechanism on Plasma 6.8 | Notes | Confidence |
|---|---|---|---|
| See the trigger and side buttons, and swallow them | Grab the physical mouse's evdev node (`EVIOCGRAB`) and forward every event the state machine keeps to a uinput clone of the same device | The only compositor-independent way. Already the design in `LINUX_MIGRATION.md`. Needs read access to `/dev/input/event*` and write access to `/dev/uinput` | [src] design; [assume] SW2, SW3 |
| Replay a swallowed click; hand a drag back | The same uinput device: write the button press when the decision is made, keep forwarding motion | Simpler and more reliable than X11, where grabs have to be lifted and put back (spike S2 in `LINUX_MIGRATION.md` found a drag hand-back can lose its start) | [assume] SW3 |
| Send a shortcut | A uinput virtual keyboard, key codes taken straight from `platform/keymap.rs` (no +8) | The RemoteDesktop portal (libei) is the alternative for sandboxes. Plasma's portal supports a restore token, so the consent dialog can be skipped after the first time [src: KDE bugs 479013, 480235]. Kando uses this portal on KDE Wayland [src] | [src] portal; [assume] uinput |
| Where the pointer is | A KWin script reads `workspace.cursorPos`; the script reports it to CursorPie over D-Bus with `callDBus` | A KDE forum answer calls a KWin script the only way to read the pointer on KDE Wayland [src]. `cursorPos` and `cursorPosChanged` exist in the scripting API [src: KDE scripting API, KWin MR 3134]. wdotool and kdotool read it the same way [src] | [src] API; [assume] latency (SW4) |
| Which app has focus, which app is under the pointer, bring an app forward | The same KWin script: `workspace.stackingOrder`, `workspace.activeWindow` (read and write), and for each window its class, desktop-file name and frame geometry | Fits "app id = desktop-entry id" from `LINUX_MIGRATION.md`. The pure hit test already exists. Plasma 6.8 also lets screen readers ask KWin which window is under the pointer, which is a lead to check [src: Linuxiac] | [src] API; [assume] property names on 6.8 (SW6) |
| Run the script and get data out of it | Load it through the `org.kde.KWin` `/Scripting` D-Bus interface (`loadScript`, `start`). A script can only talk to the outside through `callDBus` and its own config | A persistent script needs reloading when KWin restarts (SW7) | [src] |
| Put the pie under the cursor | Three ways, in order of effort: (1) the existing X11 window through XWayland; (2) a KWin script or window rule that places and pins a normal window; (3) a layer-shell surface | See [the pie window](#the-pie-window) | [assume] SW1, SW8 |
| Overlay surface with no focus, above everything | KWin implements `zwlr_layer_shell_v1` [src: KWin `layershell_v1.cpp`] | winit cannot create one, and it does not support setting a window's position or always-on-top on Wayland [src: winit docs] | [src] |
| Place a surface under the cursor from the client | `org_kde_plasma_surface.open_under_cursor` (before any buffer is attached) and `set_position` in the KDE shell protocol. Klipper uses the first one for its menu [src] | KDE specific. Whether KWin allows an ordinary client to use it is unknown | [assume] SW8 |
| Outputs and scale | KWin script (`workspace.screens`, geometry, scale), or `xdg-output` | Needed to map between X11 (XWayland) coordinates and compositor coordinates under fractional scaling (SW1) | [assume] |

**Mechanisms looked at and not chosen.**

| Alternative | Why not the main path |
|---|---|
| A KWin C++ effect that intercepts the mouse | Exact, and no udev rule, but effects are tied to the KWin version and must be rebuilt for each release, and every distribution would have to ship one. Kept as a possible long-term option |
| libinput observe-only (read, never grab) | Cannot swallow. On Linux a middle click usually pastes the primary selection, so a trigger press that reaches the app does damage. There is no safe "degraded" mode for the middle button |
| Global shortcuts portal | Keyboard shortcuts only; no mouse buttons, no hold-and-release |
| InputCapture portal | Built for screen-edge barriers (Barrier, Input Leap), not for arbitrary buttons |
| Compositor key bindings that call `cursorpie --trigger` | KDE has no equivalent for a held mouse button; already rejected as the design in `LINUX_MIGRATION.md` |

## Decisions

| # | Decision | Why |
|---|---|---|
| D1 | The X11 backend and its tests are not changed. Wayland code is additive; the backend is picked once at start-up from the session | "No behavior change" is the rule of the whole Linux port; Plasma 6.7 and older still run X11 |
| D2 | **Plasma 6.8 on Wayland is the first Wayland target.** The other compositors follow with the same architecture | It is the case that broke; it has a script API, a layer-shell and a portal that make every need above answerable |
| D3 | Input is evdev grab plus uinput clone. There is no observe-only mode for the trigger button | See the table above: an un-swallowed middle press pastes |
| D4 | The KDE adapter is a KWin script generated by CursorPie at run time and loaded over D-Bus. CursorPie does not edit the user's KWin configuration | Nothing is left behind when CursorPie is removed |
| D5 | **The state machine does not change.** The Wayland tap produces the same `Ev` values and obeys the same `Verdict` | This is what makes it behave like the other platforms (see the contract) |
| D6 | The pointer position always comes from the compositor. It is never rebuilt by adding up raw mouse movement, except for the few milliseconds between a press and the first answer | The compositor applies its own acceleration, so summed raw motion does not match where the cursor really is |
| D7 | The first Wayland milestone keeps the pie and Settings in the existing window, through XWayland. A native layer-shell pie is milestone W3, optional | XWayland is present in Plasma 6.8 and the window code already works with it; the native window is the largest single piece of work (O-4 in `LINUX_MIGRATION.md`) |
| D8 | No half-working mode. In a Wayland session where the Wayland backend is not ready (no permission, no KWin), capture is off and Settings shows the exact fix | Today's behavior, working over some windows and silently failing over others, is the worst of both. `CURSORPIE_BACKEND=x11` keeps it available on purpose |

## The parity contract

### What does not change (D5)

These are the things that make a gesture feel the same everywhere. The Wayland layer must not edit, wrap or re-time any
of them [code]:

- `tap::dispatch`, `Ev`, `Kind`, `Verdict`, the `Gesture` states, and the lock order (gesture, then config, then side
  state).
- The hold time (`pie_hold_ms`, 150 ms by default), `DRAG_TOLERANCE` (6 points), `FLICK_DISTANCE` (14 points), the
  double-click rule, the right-click switching, the side-button modes, and the `pie_clicks` filter that keeps clicks
  from reaching the app under an open pie.
- `config.json` and its version. A profile exported on macOS imports on Wayland with the same caveat as on X11 (app
  ids differ).
- Coordinates are logical points everywhere.

### Behavior by behavior

"Reference" is what macOS, Windows and X11 do today. "Gap" is where Wayland cannot match it exactly.

| Behavior | Reference | Wayland (Plasma) | Gap and how it is closed | Checked by |
|---|---|---|---|---|
| Trigger press is swallowed | Hook drops it (macOS, Windows); passive grab (X11) | The evdev loop does not write it to the clone | None, if the grab is held for the whole gesture. The mouse must never be left grabbed with nobody forwarding (see [Security](#security-and-permissions)) | SW3, SW9 |
| Quick click is passed on | Press and release are re-sent after the release | The loop writes press then release to the clone | None expected. Order and spacing must match the other layers (`post_mouse` in `actions.rs`) | SW3 |
| Hold turns out to be a drag | macOS replays the drag; X11 is best effort and may lose the start | The loop writes the press, then keeps forwarding motion | Better than X11, because the app sees a normal press followed by motion | SW3 |
| Cursor keeps moving during the hold | The cursor moves; apps under it hear no motion (X11 grab owner gets it; macOS drops drags) | Forwarded motion also reaches the app below, so it would see hover and enter/leave events while the pie is open | **Real gap.** Plan: motion is forwarded, and the pie surface takes the pointer for the duration of a mouse hold, so the app below hears nothing (the code already does this for a trackpad hold: `takes_pointer` in `ui/app/windows.rs`). The fallback is to accept the hover leak and document it | SW5 |
| Wheel scrolls once per notch | Native | The clone must copy all wheel codes (`REL_WHEEL`, `REL_WHEEL_HI_RES`, horizontal ones) or scrolling doubles or stops | Copy every code the original has, event by event | SW3 |
| Pointer speed, acceleration, natural scroll | Unchanged by CursorPie | The compositor sees a new device after the grab. Its per-device settings may not apply | The clone copies name, vendor and product so settings keyed by device can match. If they do not, document the workaround | SW2 |
| Pie opens centered on the cursor | Exact position at press | Asked from KWin at press (SW4), because the evdev loop only knows relative motion | At 150 ms hold there is time for one round trip. The pie is placed with the answer. If the answer is late, the pie opens at the last known position of that display and Settings says so | SW4 |
| Hover follows the cursor | Absolute positions come with every event | Absolute position is needed during the hold (D6) | The pie surface's own pointer events (exact, only inside the pie), and the KWin script's `cursorPosChanged` as a second source. Never summed raw motion | SW4, SW5 |
| Per-app profiles, app under cursor | OS answers | KWin script: class, desktop-file name, frame geometry; the existing pure hit test | App id must equal the desktop-entry id so one profile matches in both sessions. XWayland windows must give the same id as native ones | SW6 |
| Bring the target app forward | OS call | Script sets `workspace.activeWindow`. Allowed because the compositor does it, not a client | If it fails, `target_app` is treated as `Focused` and Settings says why | SW6 |
| Shortcut reaches the focused app | Injected key events | uinput virtual keyboard; modifiers pressed in the fixed or recorded order, then the key, then released in reverse, with the same pacing as X11 | The pie surface must not take keyboard focus (layer-shell: keyboard interactivity none; XWayland window: input hint off) or the shortcut lands in the pie | SW1, SW8 |
| Record a shortcut | A global grab | The Settings window records the keys it receives | Keys KWin reserves (Super+Tab, and others) cannot be recorded by pressing them. Settings offers typing the combination. Same as `LINUX_MIGRATION.md` O-21 | W2 checklist |
| Pie window look | Transparent, borderless, click-through, always on top, no focus, no taskbar entry, `pie_size` square | XWayland window with the hints the X11 path uses; later a layer-shell surface | KWin may animate a window opening (scale or fade) and the X11 hints may not stop it. A layer-shell surface avoids a normal window's open animation [assume] | SW1, SW8 |
| Pie lands in the right place on a scaled screen | One scale per backend | XWayland coordinates and compositor coordinates differ under fractional scaling, and which one an X client sees depends on a KDE setting | Measure the ratio between the X11 monitor size and KWin's output geometry at run time and convert; do not assume | SW1 |
| Settings window position | Placed by the app | A native Wayland window cannot choose it; an XWayland window can | Same as today while the UI is in XWayland | SW1 |
| Side-button modes, extra buttons as trigger | Button numbers | evdev button codes (`BTN_SIDE`, `BTN_EXTRA`, `BTN_FORWARD`, `BTN_BACK`, `BTN_TASK`, and others) map straight to the macOS numbering already used. The X11 numbers 10 and up are not needed | The mapping table in `LINUX_MIGRATION.md` Appendix B is reused; `inputdev.rs` already lists which buttons each mouse has | SW3 |
| Trackpad hold | Fingers from the touchpad | Reading the touchpad already works; swallowing the extra finger's click and the scroll needs something new | Plan: the pie surface takes the pointer during a trackpad hold, so the clicks and scroll land on it instead of the app. No grab of the touchpad is needed. Left to milestone W4 | W4 |
| Launch at login, single instance, file chooser, run actions | Linux code already written | Unchanged | An app started from the pie should receive focus (XDG activation token). Follow-up, not a blocker | W2 |

## Architecture

### One binary, one choice at start-up

The session decides the backend once (`LINUX_MIGRATION.md`, decision 3 and Appendix C):

| Session | Backend | Pie window |
|---|---|---|
| X11 | the existing X11 backend, unchanged | the existing window |
| Wayland, Plasma | Wayland backend with the KWin adapter | the existing window through XWayland (W1, W2); native layer-shell (W3, optional) |
| Wayland, anything else | Wayland backend with no adapter: capture and shortcuts work, per-app features are off and Settings says why | as above |
| `CURSORPIE_BACKEND=x11` | the X11 backend, even in a Wayland session | the existing window |

The facade functions in `platform/linux/backend.rs` keep their names. Today each calls `x11::conn()`. They become a
dispatch over a small set of capability objects (capture, inject, pointer, toplevels, activate, outputs). The X11
bodies move behind the X11 arm without changing. `Session` gains a compositor name (Plasma, Sway, Hyprland, GNOME, other)
and a pure function that builds the *capability report* from the environment and from probes: can `/dev/input` be read,
can `/dev/uinput` be written, is `org.kde.KWin` on the session bus.

### A hold on Wayland

```
physical mouse --evdev (EVIOCGRAB)--> CursorPie tap thread ----> Ev ----> tap::dispatch (unchanged)
                                            |                                   |
                                            | Keep: write to uinput clone       | Verdict::Drop: write nothing
                                            v                                   |
                                   uinput clone ------> KWin ------> the app under the cursor

press ---> ask KWin for the pointer (script + D-Bus) ---> pie opens there (XWayland window, or layer-shell later)
hold  ---> pie takes the pointer; hover from the pie's own pointer events, or from the script
release -> the verdict decides: run the slice, or write press and release to the clone (a quick click)
```

### New and changed files

Names follow Appendix D of `LINUX_MIGRATION.md` where it already chose one.

| File | What | Pure part, tested everywhere |
|---|---|---|
| `platform/linux/uinput.rs` | create the clone: copy event types, key bits, rel bits and the device's name, vendor, product; write events; `SYN_REPORT` framing | which bits to copy, from a fixture of the kernel's capability bitmaps |
| `platform/linux/evdev_io.rs` (extend) | `EVIOCGRAB`, `EVIOCGNAME`, `EVIOCGID`, `EVIOCGBIT`, the uinput ioctls | ioctl numbers per architecture, with the same restriction `eviocgabs` already has (x86, ARM, RISC-V) |
| `tap/wayland.rs` | the evdev loop: pick mice, grab, read, build `Ev`, call `dispatch`, forward or drop; hotplug; watchdog; lock and seat pause | the loop is written against two small traits (an event source and an event sink), so recorded event streams drive it in tests |
| `platform/linux/wayland/kwin.rs` | build the script text; the D-Bus calls to load, start and stop it; parse the replies | the script text (golden test) and the reply parser |
| `platform/linux/wayland/mod.rs` | the capability report and the facade arms for Wayland | report text for every combination, like the macOS and Windows permission text |
| `platform/linux/session.rs` (extend) | compositor name; `problem()` stops saying "log in with X11" on Plasma | environment tables per session |
| `ui/settings/**`, `welcome.rs` | the permission page and status for Wayland; the texts that today tell the user to use X11 | text is a pure function of `Os` and `Session` |
| `packaging/linux/` | the udev rule and its install step | none |

**The KWin script** does four things and nothing else: report the pointer when asked, report the window list when
asked, activate a window by id when told, and report the output geometry. It is generated text with no user-supplied
string spliced into it (ids are numbers; the D-Bus name it calls is CursorPie's own). Whether "when asked" is done by
loading and running a one-shot script each time, or by one persistent script triggered through the global shortcut
interface, is decided by SW4 on measured latency.

**D-Bus.** CursorPie needs D-Bus for the KWin adapter, and later for the tray, logind (lock and seat) and the portal.
`LINUX_MIGRATION.md` already plans `zbus` for the tray. Using one library for all of it is the plan; shelling out to
`qdbus` is fine for loading the script but too slow for a per-press query. This is an open decision
([OD-W1](#open-decisions-for-the-maintainer)).

### The pie window

`ui/app/windows.rs` today sends, for each pie: `InnerSize`, `Decorations(false)`, `MousePassthrough`,
`WindowLevel(AlwaysOnTop)`, `OuterPosition` and `Visible` [code]. In a native Wayland window winit does not support
setting the position or the always-on-top level [src], so that sequence cannot be the Wayland path. The three options:

| Option | Pros | Cons |
|---|---|---|
| **XWayland window (W1, W2)** | no UI work; the same code and the same hints as X11; the window can be positioned | depends on XWayland (present in Plasma 6.8, which keeps it for X11 apps [src]); X and compositor coordinates differ under fractional scaling; focus, stacking above native windows and open animation are unverified (SW1) |
| **Normal Wayland window placed by the KWin script** | no second UI shell | a new toplevel may take focus from the app below; position and pinning go through the script, which means more round trips; fragile |
| **Layer-shell surface (W3)** | the right tool: exact position, always above, no focus, no taskbar entry | needs a second UI shell: a small Wayland client with its own EGL surface that feeds egui the same input and textures; the largest single piece of work in the whole port (O-4) |

Recommendation: ship W1 and W2 on XWayland, and let SW1 and SW8 decide whether W3 is worth doing. If SW1 shows the
XWayland window is placed correctly, stays above native windows and takes no focus, W3 can wait until someone
needs a Wayland session without XWayland.

## Plan

Status of each milestone is in [Implementation status](#implementation-status): W1 and W2 are written but unverified, W0 and W3 to W6 are not.

Each milestone leaves X11, macOS and Windows unchanged and green. Tests come first; the pure parts are written so they
run on every OS.

| # | Milestone | Contents | Exit criteria |
|---|---|---|---|
| W0 | **Spikes on a real Plasma 6.8 machine** | SW1 to SW7 and SW9, SW10. Throwaway programs only | each answer written into [Spikes](#spikes); D2 to D8 confirmed or changed; nothing merged |
| W1 | **Capture and injection, mouse only** | `uinput.rs`, `evdev_io.rs` additions, `tap/wayland.rs`, hotplug, watchdog, lock and seat pause, the capability report and the permission page, the udev rule. The pie still opens in the XWayland window at a position given by the next milestone's script (a temporary fixed position is acceptable for testing) | the fault-injection tests pass (kill -9, hang, panic, unplug, suspend, VT switch: the mouse never stays dead); quick click, drag hand-back and wheel behave as unmodified; CPU under the budget in O-16 |
| W2 | **KDE adapter** | `kwin.rs`: pointer on press, window list, app ids, hit test, activate, outputs and scale mapping; the XWayland pie placed correctly on integer and fractional scales; Settings and welcome texts; user docs | the [checklist](#manual-checklist-for-plasma-68) passes on Plasma 6.8.2; per-app profiles match for native and XWayland apps |
| W3 | **Native pie window (optional)** | layer-shell surface with egui, only if SW8 passes and SW1 shows a need | pie drawn at a margin position at 60 fps on Plasma; memory while idle no worse than now |
| W4 | **Trackpad on Wayland** | the pie surface takes the pointer during a trackpad hold; no touchpad grab | extra-finger click and scroll do not reach the app below; no change to the X11 path |
| W5 | **Other compositors and GNOME** | Hyprland, Sway and GNOME adapters are written ([Other compositors](#other-compositors-w5)); COSMIC and niri are not; the full-screen fallback is not (phases L6 and L7 of `LINUX_MIGRATION.md`) | their own checklists |
| W6 | **Packaging and sign-off** | `.deb`, `.rpm`, tarball with the udev rule; the checklist run and the result recorded, like "Status" in `WINDOWS_MIGRATION.md` | matrix filled in |

Order of value: W1 and W2 make Plasma 6.8 work. Everything after that is independent.

## Spikes

Throwaway programs that answer one question each, in the same spirit as the spikes in `LINUX_MIGRATION.md` (whose S4
to S8 are the general versions of SW2, SW3, SW4, SW8 and the GNOME case). Test machine: Plasma 6.8.2 on Wayland, a real
mouse, one integer-scale and one fractional-scale display. Nothing below has been run.

| # | Question | Pass condition |
|---|---|---|
| SW1 | With today's build in a Plasma 6.8 session: is the pie placed under the cursor, above native windows, without taking focus and without an open animation? How do X coordinates relate to compositor coordinates at 100%, 150% and 200% scale, with KDE's setting for how scaling is applied to X11 applications in both positions? | placement error under 2 px after conversion, or a documented conversion rule; focus unchanged; a yes or no on the animation |
| SW2 | After `EVIOCGRAB` plus a cloned uinput device, do KDE's per-device pointer settings (speed, acceleration profile, natural scroll) still apply? | identical feel, or the documented workaround |
| SW3 | CPU and latency of the forwarding loop at 1000 and 8000 Hz; hi-res and horizontal wheel; quick click, drag hand-back, side buttons | under 1% of a core at 1000 Hz and under 1 ms added; no duplicate or lost scroll; the app sees a normal click and a normal drag |
| SW4 | Pointer position through a KWin script: one-shot script against one persistent script triggered by the global shortcut interface. How long does it take, and how exact is it under fractional scaling? | known within the 150 ms hold, p95 under 40 ms, error under 2 px |
| SW5 | While a mouse hold is on and motion is forwarded: does the app below see hover? Does a pie surface that takes the pointer stop that, without taking keyboard focus and without changing which window is active? | the app below hears nothing; focus is unchanged |
| SW6 | Window properties on Plasma 6.8: do the class and desktop-file name give the desktop-entry id for Firefox (native and Flatpak), Chrome, Konsole, Dolphin, VS Code, Steam, and an XWayland app? Can a script activate a window by id? | the same id as the X11 backend gives for the same app; activation works |
| SW7 | Script lifecycle: how is it loaded at session start, and after KWin restarts? Can it be loaded from a file in `$XDG_RUNTIME_DIR` without touching `kwinrc`? | the script is back within 2 s after a KWin restart, with no change to the user's configuration |
| SW8 | Can a layer-shell surface carry egui through EGL without winit? Does KWin let an ordinary client use `open_under_cursor` or `set_position`? | a pie drawn at a margin position on Plasma at 60 fps, and a yes or no for each protocol request |
| SW9 | Safety: kill -9, a hang, a panic in `dispatch`, unplugging the mouse, suspend and resume, lock and unlock, a VT switch, all while the mouse is grabbed | the mouse works again within 300 ms in every case; a lock screen never receives a swallowed click |
| SW10 | Plasma 6.8 specifics: how is `/dev/uinput` and `/dev/input/event*` access granted on current Kubuntu, Fedora KDE and Arch; did anything in the 6.8 release change the scripting API? | a udev rule that works on each, or a list of what differs |

## Risks

| # | Risk | Effect | Mitigation |
|---|---|---|---|
| R1 | A bug leaves the mouse grabbed with nothing forwarding | the user loses the mouse | the watchdog, signal handlers and fail-open rules of `LINUX_MIGRATION.md` (heartbeat older than 250 ms releases every grab; the kernel also releases on close); SW9 before any release |
| R2 | The permission is a real install step | many users stop at the first screen | a clear page with the exact fix and a live status; the rule shipped in the package; honest text about what the permission allows |
| R3 | The KWin scripting API changes between Plasma releases | the adapter stops working after an update | keep the script small; test against the first beta of each Plasma release; fail soft (per-app features off, gesture still works) |
| R4 | XWayland is a moving target (scaling modes, focus, animation) | the pie lands in the wrong place or flashes | SW1 first; the layer-shell pie (W3) as the way out |
| R5 | Any program of the same user can call CursorPie's D-Bus name | a bad pointer value or a huge string reaches the app | the service only *receives* data; every value is range checked and every string length limited; it never starts an action |
| R6 | Per-device pointer settings do not carry over to the clone | the mouse feels different while CursorPie runs | SW2; documented workaround |
| R7 | A pie surface that takes the pointer changes hover behavior of the app below | apps flash hover states | SW5; fall back to accepting the leak and say so |
| R8 | One mouse exposes several evdev nodes (receivers, gaming mice) | the wrong node is grabbed; extra buttons are missed | strict filter (relative X and Y plus a middle button), never our own clone, errors isolated per device; `inputdev.rs` already merges mice listed twice |
| R9 | Maintainer time. The native pie (W3) is large | the project stalls on the hard part | W1 and W2 do not need it |

## Security and permissions

The model in `LINUX_MIGRATION.md` ("Security and privacy") applies and is not repeated. What this plan adds:

- **What is opened.** Pointer devices only: read and grab on the mouse's node, write on `/dev/uinput`. Keyboards are not
  opened, so the promise that key events are never read stays true. The shortcut recorder uses the keys the focused
  Settings window itself receives.
- **How access is given.** Two options, for the maintainer to choose ([OD-W2](#open-decisions-for-the-maintainer)):
  the `input` group (what the trackpad feature already asks for, and which gives read access to *every* input device,
  keyboards included), or a udev rule that tags `/dev/uinput` for the logged-in seat user. The rule's exact text and
  whether a distribution already tags `/dev/uinput` is part of SW10 and is not assumed here.
- **The KWin script runs inside the compositor** with no sandbox [src: KWin scripting notes]. So it is kept to the four
  jobs above, it contains no user-supplied text, and its source is a pure function that a test pins.
- **D-Bus is a user-level boundary** (R5).
- **Nothing is written to the user's KDE configuration** (D4).

## Test strategy

- **Pure first.** The script text (golden test), the reply parser, the capability report for every combination, the
  clone's capability computation from fixture bitmaps, the existing hit test with fixtures from Plasma
  (`resourceClass` and desktop-file name for a native and an XWayland window of the same app), and the coordinate
  conversion between XWayland and the compositor at several scales.
- **The loop against fakes.** `tap/wayland.rs` takes an event source and an event sink. Recorded streams (a quick
  click, a drag, a wheel burst at hi-res, a hotplug, a device that disappears mid-gesture) run through it on every OS,
  and assert that what reaches the sink equals what the unmodified mouse would have sent, minus the swallowed button.
  A test also asserts that the state machine saw the same `Ev` values it sees from the X11 layer for the same gesture.
- **Fault injection** (SW9) as a scripted test on a machine with `/dev/uinput`. It is skipped where the device is not
  there.
- **A headless compositor** for the script, if KWin can run without a display for tests (to be checked in W0); otherwise
  the script is tested on a real session only.
- **CI** keeps building and testing the X11 path exactly as now; the new files add tests that run everywhere and
  tests that skip when the devices are missing.

## Manual checklist for Plasma 6.8

Run on Plasma 6.8.2 (or newer) in a Wayland session, once with integer scale and once with a fractional scale. Add it
to `LINUX_MIGRATION.md`'s checklist when W2 is built. Nothing here has been run.

1. The welcome page shows the permission step; after the fix, the live status turns to ready without a restart.
2. Hold the middle button over a native Wayland window, an XWayland window and the desktop: the pie opens centered on
   the cursor each time, and the middle click does not paste.
3. A quick middle click (shorter than the hold time) is passed on to the app once.
4. Middle-drag over an app that scrolls by it: the drag works from its first pixel.
5. Pointer speed, acceleration and natural scroll feel the same with CursorPie running and stopped.
6. A hi-res scroll wheel scrolls once per notch; horizontal scroll works.
7. While the pie is open, the app below shows no hover.
8. Shortcuts from a slice reach the focused app, including with the pie closing at the same moment.
9. Per-app profiles pick the right pie for Firefox, Konsole, Dolphin, VS Code and an XWayland app; "app under the
   cursor" and "bring the app forward" work.
10. Unplug and replug the mouse, suspend and resume, lock and unlock, switch to a VT and back: the mouse never stays dead.
11. After KWin restarts (a compositor crash, or a log out and in with CursorPie left running by autostart): the adapter
    is back without restarting CursorPie.
12. With the permission missing, CursorPie says exactly what to do and the mouse is not touched.
13. `CURSORPIE_BACKEND=x11` in the same session gives today's behavior.
14. Two displays at different scales: the pie is centered on the cursor on both, and on the seam.
15. Settings opens and works; recording a shortcut works for ordinary keys and says what to do for reserved ones.

## Documentation changes

Done in the same change as this page: this page; its row in the index (`docs/README.md`) and in the website's list
(`index.html`, `assets/site.js`, `sitemap.xml`, `README-WEBSITE.md`); a pointer from `LINUX_MIGRATION.md`;
the Linux limits in `KNOWN_ISSUES.md`, `INSTALL.md`, `CONTRIBUTING.md` and `README.md` now say that Plasma 6.8 has no
X11 session and link here. They still say Wayland is not supported, because it is not yet.

To do with the code: the texts in `ui/settings/tabs/general.rs` and `ui/settings/welcome.rs` that tell the user to log in
with an X11 session; `Session::problem()`; the doc comments in `tap/linux.rs` and `platform/linux/backend.rs`; the
user guide's trackpad section; a `## Unreleased` line in `CHANGELOG.md` when behavior changes.

## Open decisions for the maintainer

| # | Question | Leaning |
|---|---|---|
| OD-W1 | One D-Bus library (`zbus`) for the adapter, the tray, logind and the portal, or hand-written bindings like `xlib.rs`? | one library, because four features need it. The project's habit of few dependencies argues for a short review of its dependency tree first |
| OD-W2 | `input` group or a udev rule for `/dev/uinput`? | the rule, shipped in the package, with the group as the documented fallback; decided with SW10 |
| OD-W3 | Is a Wayland session without XWayland a target? | no for W1 and W2 (Plasma 6.8 ships XWayland); yes only if W3 is built |
| OD-W4 | Install the KWin script as a package, or load it at run time? | run time (D4); revisit only if SW7 shows reloading is unreliable |
| OD-W5 | Minimum Plasma version? | 6.0, which has the scripting API this plan uses; Plasma 5 is out of scope |
| OD-W6 | Flatpak? | no for Wayland capture (no access to `/dev/input`); the X11 tier in a Flatpak is unchanged, as in `LINUX_MIGRATION.md` O-12 |
| OD-W7 | Is the KWin C++ effect worth a second look in the long term? | not before W2 is stable |

## Other compositors (W5)

Written after reading each compositor's own documentation. **None of this was run**, and the Rust was not compiled
where it was written (no toolchain); the pure parts have tests that need a `cargo test` run. Every adapter writes the
same state (`kwin::publish`, `kwin::set_pointer`) and reads the same commands (`kwin::take_commands`), so the backend
facade did not change shape. `wayland/adapter.rs` picks one from the compositor (`XDG_CURRENT_DESKTOP`, or
`HYPRLAND_INSTANCE_SIGNATURE` / `SWAYSOCK` when that is empty).

| Compositor | How it is asked | Pointer | Windows, focus, bring forward | Where |
|---|---|---|---|---|
| KDE Plasma | a script inside KWin, over D-Bus | yes | yes | `kwin.rs`, `bus.rs` |
| GNOME | a GNOME Shell extension, over D-Bus; the user installs and enables it | yes (`global.get_pointer()`) | yes (`Main.activateWindow`) | `gnome.rs`, `packaging/linux/gnome-extension/` |
| Hyprland | its control socket, `j/clients`, `j/monitors`, `j/activewindow`, `j/cursorpos`, `dispatch focuswindow` | yes | yes | `hyprland.rs` |
| Sway | its i3-style IPC socket, `GET_TREE`, `RUN_COMMAND` | **no**: the IPC has no call for it | yes | `sway.rs` |
| COSMIC, niri, others | none yet | no | no | |

**What each source says, and what it does not.**

- **Hyprland** is the easy case: it prints the cursor position as JSON on its socket [src: wiki.hypr.land/IPC; the
  JSON of `j/cursorpos` is `{"x":..,"y":..}`]. The wiki warns that a connection left open freezes Hyprland until a
  five-second timeout, so every request opens, writes, reads and drops. It does not print a stacking list: the order
  used is full screen, then floating, then tiled, each by `focusHistoryID` [assume]. `at` is taken as logical
  layout points and monitor `width`/`height` as pixels divided by `scale` [assume: confirm on a scaled screen].
- **Sway** has `seat <name> cursor set|move` to *move* the cursor and no call to read it [src: `sway-input(5)`,
  `sway-ipc(7)`]. So the adapter reports no pointer; "app under the cursor" becomes the focused app (right with the
  default `focus_follows_mouse yes`), and the pie is placed from XWayland's idea of the pointer, which is stale over
  native windows. The way out is a transparent layer-shell surface that reads the pointer when it enters (what
  `wl-find-cursor` does), which needs a Wayland client in this program: milestone W3. Until then Sway is the weakest
  of the four, and the report says so.
- **GNOME** has no external API: Mutter offers nothing a program can call, and `Shell.Eval` is closed. An extension
  is the only route, as every window-listing tool on GNOME does [src: gjs.guide; the *Window Calls*, *Window
  Control* and *computer-use-linux* extensions]. The extension here pushes to CursorPie instead of exporting a window
  list (other extensions export one with no access control, which any program of the session can read), sends no
  titles, and exports one method, `Sync`. The GNOME Shell 50 porting guide lists no change to `extension.js` or
  `metadata.json`; GNOME 49 removed `Meta.MaximizeFlags` and `get_maximized()` (not used) and 48 moved
  `Meta.get_window_actors` (not used: `global.get_window_actors()` is). A new extension is only found after a log
  out on Wayland, so the report text gives the copy-and-enable steps; CursorPie does not install it by itself.
- **niri** has IPC for windows and focus but, as far as the tool `wlr-capture` documents, no rectangle in global
  coordinates (its layout scrolls), and nothing for the pointer: only "frontmost" and "bring forward" could be
  done. **COSMIC** and wlroots compositors without an IPC would need the `ext-foreign-toplevel-list` or
  `wlr-foreign-toplevel-management` protocols, which also give no geometry, and a Wayland client crate this program
  does not depend on. Neither is started.

**Spikes this adds (to run on each compositor).**

| # | Question | Pass |
|---|---|---|
| SW11 | Hyprland: are `at`, `size` and the monitor `x`/`y` in the same logical space as `j/cursorpos`, at scale 1, 1.5 and 2? Is a rotated monitor's size before or after the rotation? | a click at the pointer lands in the window `app_at` names |
| SW12 | Hyprland: the order `build_snapshot` gives against what is drawn, with a floating window over a tiled one and a full-screen window | the same top window |
| SW13 | Sway: is the app under the cursor the focused one with `focus_follows_mouse yes`; what does the pie do over a native window | the report text is accurate |
| SW14 | GNOME 47 to 50: the extension loads; `Report` reaches the service; `Sync` is answered; `activate:` raises across workspaces and un-minimizes; the pointer at 16 ms costs little CPU | all five |
| SW15 | GNOME: does `global.get_window_actors()` plus `sort_windows_by_stacking` give the real stacking order, and does `get_frame_rect()` match the pointer's space at a fractional scale | the hit test names the window under the pointer |

## Sources

Read on 7 November 2026. Paraphrased, not quoted.

- KDE Plasma 6.8 goes Wayland only, announcement and reaction: Phoronix, "KDE Plasma 6.8 Will Go Wayland-Exclusive In
  Dropping X11 Session Support" (26 November 2025), https://www.phoronix.com/news/KDE-Plasma-68-Wayland-Exclusive ;
  OMG! Ubuntu, "KDE Plasma 6.8 Drops X11 Session, Goes Wayland-Only",
  https://omgubuntu.co.uk/2025/11/kde-plasma-6-8-no-x11-wayland-only ;
  PC Games Hardware, https://www.pcgameshardware.de/Linux-Software-26761/News/KDE-Plasma-68-drops-X11-Wayland-becomes-mandatory-from-October-2026-1544518/
- The release: Linux Journal, "KDE Plasma 6.8 Marks 30 Years With a Major Shift to Wayland" (15 October 2026),
  https://www.linuxjournal.com/content/kde-plasma-68-marks-30-years-major-shift-wayland-and-more-powerful-linux-desktop ;
  Linuxiac, "KDE Plasma 6.8 Released, Drops X11 Sessions", https://linuxiac.com/kde-plasma-6-8-released-drops-x11-sessions-after-30-years-of-kde/ ;
  Linuxiac, "KDE Plasma 6.8: Here's What's Coming", https://linuxiac.com/kde-plasma-6-8-heres-whats-coming/ ;
  Linuxiac on 6.8.2, https://linuxiac.com/kde-plasma-6-8-2-released-with-kwin-wayland-and-desktop-fixes/ ;
  hwbusters, https://hwbusters.com/?p=119630
- KWin scripting: the API reference, https://develop.kde.org/docs/plasma/kwin/api/ ; adding the cursor position to
  the scripting API, https://invent.kde.org/plasma/kwin/-/merge_requests/3134 ; KDE forum, "Is there a better way to
  obtain the mouse cursor location?", https://discuss.kde.org/t/is-there-a-better-way-to-obtain-the-mouse-cursor-location/20559 ;
  KDE forum, "Does KWin emit a signal (via DBus) on window change?", https://discuss.kde.org/t/does-kwin-emit-a-signal-via-dbus-on-window-change/43923 ;
  wdotool (libei, KWin scripting, uinput backends), https://github.com/cushycush/wdotool
- Layer-shell in KWin: `layershell_v1.cpp`, https://lxr.kde.org/source/plasma/kwin/src/wayland/layershell_v1.cpp ; the
  protocol, https://lxr.kde.org/source/plasma/layer-shell-qt/src/wlr-layer-shell-unstable-v1.xml
- KDE shell protocol (`open_under_cursor`, `set_position`): https://wayland.app/protocols/kde-plasma-shell ;
  Klipper using it, https://invent.kde.org/plasma/plasma-workspace/-/merge_requests/1527
- RemoteDesktop portal persistence on Plasma: https://bugs.kde.org/show_bug.cgi?id=479013 and
  https://bugs.kde.org/show_bug.cgi?id=480235
- Kando on KDE Wayland (KWin scripting API and the Remote Desktop portal), https://github.com/hmpthcs/kando
- Hyprland IPC (the two sockets, `j/` flags, the synchronous connection warning, the events), https://wiki.hypr.land/IPC/ ;
  an example that reads `j/cursorpos` for the cursor position on Hyprland, https://github.com/getopenscreen/openscreen/pull/157
- Sway: `sway-ipc(7)` (message format, `GET_TREE`, `RUN_COMMAND`), https://man.archlinux.org/man/sway-ipc.7 ;
  `sway-input(5)` (`seat cursor move|set`, no read), https://github.com/swaywm/sway/blob/efd83cb8b911d88a17263a4ee8e62cf07dbfa4bd/sway/sway-input.5.scd ;
  `wlr-capture`, which relies on each compositor's own IPC because "Wayland gives a client no way to query the
  pointer", and notes that niri exposes no window rectangle, https://docs.rs/crate/wlr-capture/latest/source/src/focus.rs
- GNOME Shell: porting guides for 45, 48, 49 and 50, https://gjs.guide/extensions/upgrading/gnome-shell-50.html (and
  `-49`, `-48`, `-45`); an extension that lists windows and activates one with `Main.activateWindow` on GNOME 49,
  https://docs.rs/crate/computer-use-linux/0.3.0/source/gnome-shell-extension/computer-use-linux@avifenesh.dev/extension.js ;
  a project reporting that `Main.activateWindow` raises reliably on GNOME 50 Wayland, across workspaces and from
  minimized, https://github.com/jonocodes/deckd/issues/116 ; reviews of *Window Control*, which warn that its
  D-Bus interface has no access control, https://extensions.gnome.org/review/77087
- winit: positioning and always-on-top are unsupported on Wayland, https://docs.rs/winit/latest/winit/window/struct.Window.html and
  https://github.com/rust-windowing/winit/issues/806
