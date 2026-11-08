<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/cursorpie-logo-white.png">
  <img src="assets/cursorpie-logo-black.png" alt="CursorPie" width="420">
</picture>

### Your actions, one gesture away.

**Hold a mouse button or a finger on your trackpad. Flick toward an action. Release.**

A configurable pie menu for your cursor — available on macOS, Windows and Linux, with a mouse or a trackpad.

[**Visit the website**](https://sebastvnd.github.io/cursorpie/) · [Download latest release](https://github.com/sebastvnd/cursorpie/releases/latest) · [Explore the docs](https://sebastvnd.github.io/cursorpie/index.html?doc=docs/README.md)

[![CI](https://github.com/sebastvnd/cursorpie/actions/workflows/ci.yml/badge.svg)](https://github.com/sebastvnd/cursorpie/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/sebastvnd/cursorpie?include_prereleases)](https://github.com/sebastvnd/cursorpie/releases)
[![License: MIT](https://img.shields.io/github/license/sebastvnd/cursorpie)](LICENSE)
![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-blue)

[Installation](docs/INSTALL.md) · [User guide](docs/USER_GUIDE.md) · [Configuration](docs/CONFIGURATION.md) · [Contributing](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)

</div>

---

## What is CursorPie?

CursorPie puts a radial menu right under your pointer. Hold the middle mouse button (or touch and hold a trackpad), move toward the action you need, then release. No hunting through menus and no need to remember a long list of shortcuts.

A slice can send a keyboard shortcut, open an application, URL or file, run a command, switch a mouse-button mode, or open a submenu. You can keep a general menu and create a different profile for each application.

Built with **Rust** and [egui](https://github.com/emilk/egui), CursorPie is inspired by [Kando](https://github.com/kando-menu/kando).

## Highlights

- **A quick, natural gesture.** Hold for 150 ms by default, flick toward a slice, and release. Hold in the center to open Settings.
- **A menu for every app.** Keep a general pie and create app-specific profiles for the tools you use most.
- **Mouse or trackpad.** Touch and hold a trackpad instead of pressing a button, on macOS, Windows and Linux (X11 and Wayland). It is off by default and uses the same pie, timings and profiles; see [Trackpad](#trackpad).
- **Useful mouse buttons.** Assign actions to back, forward and other supported extra buttons with configurable modes.
- **Actions that fit your workflow.** Trigger shortcuts, open apps, links and files, run shell commands, or chain actions into a sequence.
- **Nested menus.** Organize up to 8 slices per ring and 5 levels of submenus.
- **Room to experiment.** Undo and redo edits in Settings, with automatic backups of previous saves.
- **Easy to share.** Export a profile, including its icons, as a single JSON file.
- **Private and lightweight.** No account or telemetry. Its only network use is a daily look for a newer release when you open Settings (which you can switch off) and, when you press **Install update**, the download of that release. The menu stays out of the way while idle.

A clickable mouse wheel, or a trackpad, is enough to get started. Some features and button options vary by operating system; see [Platform support](#platform-support).

## Download and install

Get the latest ready-to-install build from [GitHub Releases](https://github.com/sebastvnd/cursorpie/releases/latest):

| Platform | Package |
|---|---|
| **macOS 11+** (Intel and Apple silicon) | [Universal DMG](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-macos-universal.dmg) |
| **Windows 10/11** (recommended) | [Setup installer](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-windows-setup.exe) · [ZIP bundle](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-windows.zip) |
| **Ubuntu 24.04+** (x86_64) | [Ubuntu package](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-ubuntu-24.04-x86_64.tar.gz) |
| **Fedora 44** (x86_64) | [Fedora package](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-fedora-44-x86_64.tar.gz) |

For step-by-step instructions, including the Linux `install.sh` script and macOS permissions, read the [installation guide](docs/INSTALL.md). Build artifacts from individual CI runs are temporary; public downloads are published with GitHub Releases when a version tag is pushed.

## Try it

1. Install and launch CursorPie.
2. Hold the **middle mouse button**, or touch and hold your **trackpad**, for a split second (150 ms by default).
3. Move the pointer toward a slice.
4. Release to run the action.

No middle button? Turn on *Open the pie with a trackpad hold* in **Settings > Behavior > Trackpad** and touch and hold the trackpad instead. Hold the trigger in the center of the pie to open Settings and customize your menus. The [user guide](docs/USER_GUIDE.md) explains gestures, actions, profiles and settings in detail.

## Platform support

| Platform | Status | Notes |
|---|---|---|
| **macOS 11+** | Supported | Requires the Accessibility permission. The trigger can be the middle button or one of three extra mouse buttons. Any trackpad works; Force Touch trackpads also give haptic ticks. |
| **Windows 10 (1809) / 11** | Supported · stable | No special permission required. The middle button is the only trigger; Windows exposes the back and forward buttons as extra buttons. Trackpad hold needs a precision touchpad. |
| **Linux (X11 and Wayland)** | Supported · experimental | x86_64 packages are provided for Ubuntu 24.04+ and Fedora 44. X11 sessions need nothing set up. Wayland sessions (KDE Plasma 6.8 and newer) are handled by a separate layer that needs a udev rule; per-app profiles work on KDE Plasma. It is written and tested but not yet run on a Plasma 6.8 machine ([status](docs/WAYLAND_SUPPORT.md)). There is no tray icon yet. Trackpad hold reads the touchpad from `/dev/input` and needs your user in the `input` group (on Wayland the udev rule covers it). |

Linux support is still being tested across distributions and desktop environments. Read [Known issues](docs/KNOWN_ISSUES.md) before installing, especially when using Wayland.

### Trackpad

Touch and hold opens the pie, moving the finger hovers a slice, lifting runs it, and one more finger that taps or clicks switches the mode. It is **off by default** and works on all three systems:

| System | What is read | Needs |
|---|---|---|
| **macOS** | any trackpad, through Apple's private `MultitouchSupport` framework (loaded only while the feature is on) | the Accessibility permission; haptic ticks on a Force Touch trackpad |
| **Windows** | a *precision* touchpad, through Raw Input | nothing extra; no haptics |
| **Linux (X11 and Wayland)** | a touchpad, from `/dev/input/event*` | your user in the `input` group (`sudo usermod -aG input $USER`, then log out and in), or on Wayland the udev rule `70-cursorpie.rules`; no haptics |

The trackpad paths are new and have not been verified on every device yet; see [Known issues](docs/KNOWN_ISSUES.md) and the [user guide](docs/USER_GUIDE.md#trackpad-macos-windows-and-linux). On Linux, `cursorpie --touchpad-debug` shows which step fails.

## Documentation

| Topic | Guide |
|---|---|
| Install a release or build from source | [Installation](docs/INSTALL.md) |
| Learn gestures, slices, profiles and settings | [User guide](docs/USER_GUIDE.md) |
| Read or edit `config.json` | [Configuration](docs/CONFIGURATION.md) |
| Check current limitations | [Known issues](docs/KNOWN_ISSUES.md) |
| Debug a problem | [Debugging](docs/DEBUGGING.md) |
| Contribute or build the project | [Contributing](CONTRIBUTING.md) |
| Browse design and platform notes | [Documentation index](docs/README.md) |
| See what changed between versions | [Changelog](CHANGELOG.md) |

The same guides are available in the [CursorPie website](https://sebastvnd.github.io/cursorpie/), which renders the Markdown files directly from the repository.

## Build from source

To compile CursorPie yourself, install [Rust](https://rustup.rs) 1.88 or later and follow the platform-specific prerequisites in [CONTRIBUTING.md](CONTRIBUTING.md).

```sh
git clone https://github.com/sebastvnd/cursorpie.git
cd cursorpie
cargo run --release
```

To run the checks used by CI:

```sh
cargo tools hooks install   # once per clone
cargo tools ci              # format, lint, tests and documentation checks
```

## Contributing

Contributions, bug reports and ideas are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) for the project workflow, development setup and pull-request guidelines.

Found a bug? [Open an issue](https://github.com/sebastvnd/cursorpie/issues) and include your operating system, desktop/session type (when relevant), and steps to reproduce it.

## Privacy

CursorPie runs locally. It has no account system or telemetry. Its only network connections are the update check (when you open Settings it asks GitHub, at most once a day, for the newest release) and, only when you press **Install update**, the download of that release; GitHub sees your IP address and the CursorPie version. Switch the check off with **About > Updates > Check for updates automatically**.

## License

CursorPie is released under the [MIT License](LICENSE). It uses egui/eframe, tray-icon, serde and image, and is inspired by [Kando](https://github.com/kando-menu/kando).
