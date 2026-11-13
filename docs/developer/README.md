# Developer guide

For people changing CursorPie. Users: see the [user guide](../user/README.md).

Start with [CONTRIBUTING.md](../../CONTRIBUTING.md) (setup, workflow, pull requests, releasing). The technical reference is
the Rust docs (`cargo docs`): architecture, config format and step-by-step recipes for common changes.

## Tools and debugging

| Document | What it covers |
|---|---|
| [DEBUGGING.md](../DEBUGGING.md) | The diagnostic log (`CURSORPIE_DEBUG=1`) and how to add log lines (the `diag/` crate) |
| [UPDATES_TESTING.md](UPDATES_TESTING.md) | The manual checklist for the updater |
| [PANELS.md](../PANELS.md) | The panel format and how to add a caller or a look |
| [BROWSER_BRIDGE.md](BROWSER_BRIDGE.md) | How the browser extension talks to the app |
| [OPEN_URL_CHOICE.md](OPEN_URL_CHOICE.md) | The "new window / page that is open" choice of Open URL slices |

## Design records

These explain *why* the code is the way it is and hold the manual test checklists of each platform. A record keeps its
history, but its status line must say what is true today.

| Document | Status | What it covers |
|---|---|---|
| [WINDOWS_MIGRATION.md](WINDOWS_MIGRATION.md) | Done; Windows is stable | How the macOS-only code base became macOS and Windows, and the Windows checklist |
| [LINUX_MIGRATION.md](LINUX_MIGRATION.md) | **X11: pre-stable** (implemented and in use; checklist being completed). Wayland: experimental | The Linux design, what is implemented, and the Linux checklist |
| [WAYLAND_SUPPORT.md](WAYLAND_SUPPORT.md) | **Experimental**: written, not yet run on real Plasma, GNOME, Hyprland or Sway | Why Wayland support is needed, what each compositor offers, the parity contract, milestones and spikes |
| [OTHER_DESKTOPS.md](OTHER_DESKTOPS.md) | Done | How *Open app* follows an app to another desktop or Space |
| [UI_DESIGN.md](UI_DESIGN.md) | Living | Look, rendering rules and the visual checklist |
| [TRACKPAD_HAPTICS_PLAN.md](TRACKPAD_HAPTICS_PLAN.md) | Implemented; native layers awaiting verification on real hardware | How trackpads (macOS, Windows, Linux) and Mac haptics work with the same pie as the mouse |

## Where the Linux code is

| Part | Files |
|---|---|
| Session detection, backend choice | `src/platform/linux/session.rs`, `backend.rs` |
| X11 backend | `src/tap/linux.rs`, `src/platform/linux/x11.rs`, `xlib.rs` |
| Wayland backend | `src/tap/wayland.rs`, `src/platform/linux/wayland/` (adapters: `kwin.rs`, `gnome.rs`, `hyprland.rs`, `sway.rs`), `uinput.rs`, `evdev_io.rs` |
| Packaging | `scripts/package-linux.sh`, `packaging/linux/` |
