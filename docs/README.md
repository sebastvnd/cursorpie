# Documentation

## For users

| Document | What it covers |
|---|---|
| [INSTALL.md](INSTALL.md) | Installing or building CursorPie on macOS, Windows and Linux, and the permissions it needs |
| [USER_GUIDE.md](USER_GUIDE.md) | The gesture, slices and actions, profiles, side buttons, Settings, undo and backups, sharing |
| [CONFIGURATION.md](CONFIGURATION.md) | Where `config.json` lives, its structure and every field |
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
| [LINUX_MIGRATION.md](LINUX_MIGRATION.md) | X11 supported but experimental, checklist under internal observation; Wayland designed | The Linux design, what is implemented, and the Linux checklist |
| [OTHER_DESKTOPS.md](OTHER_DESKTOPS.md) | Done | How *Open app* follows an app to another desktop or Space |
| [UI_DESIGN.md](UI_DESIGN.md) | Living | Look, rendering rules and the visual checklist |

## Keeping the docs in sync

- A user-visible change updates [USER_GUIDE.md](USER_GUIDE.md) or [CONFIGURATION.md](CONFIGURATION.md) in the
  same pull request, and adds a line under `## Unreleased` in `CHANGELOG.md`.
- `README.md` stays short: it introduces the project and links here. Do not write version-specific sentences in
  it ("Version 1.2 adds ..."); they go stale, and the release tool warns about them.
- A design record keeps its history, but its status line at the top must say what is true today.
