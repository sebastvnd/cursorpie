# Documentation

## For users

| Document | What it covers |
|---|---|
| [INSTALL.md](INSTALL.md) | Installing or building CursorPie on macOS, Windows and Linux, and the permissions it needs |
| [UPDATES.md](UPDATES.md) | The daily update check and the one-click install on each system, and how to test it |
| [USER_GUIDE.md](USER_GUIDE.md) | The gesture, slices and actions, profiles, side buttons, Settings, undo and backups, sharing |
| [CONFIGURATION.md](CONFIGURATION.md) | Where `config.json` lives, its structure and every field |
| [FLOWS_FOR_AI.md](FLOWS_FOR_AI.md) | How to write a flow as JSON: the format, the rules, a recipe, shell and clipboard pitfalls, macOS permissions, browser patterns, debugging and checked examples. Written for AI agents; a JSON Schema is in `flow.schema.json` |
| [PANELS.md](PANELS.md) | Panels: small interactive windows (fields, lists, buttons) that flows and the browser extension can show; the JSON format, how to use them in a flow and from an extension, and how to add a caller or a look |
| [KNOWN_ISSUES.md](KNOWN_ISSUES.md) | What does not work yet, grouped by system |

## For contributors

Start with [CONTRIBUTING.md](../CONTRIBUTING.md). The technical reference is the Rust docs (`cargo docs`):
architecture, config format and step-by-step recipes for common changes.
To find a problem that only shows on one machine, use the diagnostic log: [DEBUGGING.md](DEBUGGING.md) (it also
says how to add log lines, which is what the `diag/` crate is for).

## Design records

These explain *why* the code is the way it is, and hold the manual test checklists for each platform. They are
written for people changing the code, not for users.

| Document | Status | What it covers |
|---|---|---|
| [WINDOWS_MIGRATION.md](WINDOWS_MIGRATION.md) | Done, Windows is stable | How the macOS-only code base became macOS and Windows, and the Windows checklist |
| [LINUX_MIGRATION.md](LINUX_MIGRATION.md) | X11 supported but experimental, checklist under internal observation; Wayland partly written, not yet run on real hardware | The Linux design, what is implemented, and the Linux checklist |
| [WAYLAND_SUPPORT.md](WAYLAND_SUPPORT.md) | Research, plan and a first implementation; not yet run on a Plasma 6.8 machine | Why Wayland support is needed now (current KDE Plasma has no X11 session), what Plasma offers, what "behaves like the other platforms" means, and the milestones and spikes |
| [OTHER_DESKTOPS.md](OTHER_DESKTOPS.md) | Done | How *Open app* follows an app to another desktop or Space |
| [UI_DESIGN.md](UI_DESIGN.md) | Living | Look, rendering rules and the visual checklist |
| [TRACKPAD_HAPTICS_PLAN.md](TRACKPAD_HAPTICS_PLAN.md) | Implemented in code; native macOS, Windows and Linux (X11 and Wayland) layers awaiting verification on real hardware | How a trackpad on macOS, Windows and Linux (hold, extra-finger click) and haptic feedback on a Mac can work with the same pie as the mouse, and the spikes to run first |

## Keeping the docs in sync

- A user-visible change updates [USER_GUIDE.md](USER_GUIDE.md) or [CONFIGURATION.md](CONFIGURATION.md) in the
  same pull request, and adds a line under `## Unreleased` in `CHANGELOG.md`.
- `README.md` stays short: it introduces the project and links here. Do not write version-specific sentences in
  it ("Version 1.2 adds ..."); they go stale, and the release tool warns about them.
- A design record keeps its history, but its status line at the top must say what is true today.
