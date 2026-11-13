# Install CursorPie

CursorPie runs on macOS, Windows and Linux. Install a ready-made package from the latest
[GitHub Release](https://github.com/sebastvnd/cursorpie/releases/latest), or build from source.

## Contents

- [Choose a package](#choose-a-package)
- [Requirements](#requirements)
- [macOS](#macos)
- [Windows](#windows)
- [Linux](#linux)
- [First launch](#first-launch)
- [Trackpad hold (optional)](#trackpad-hold-optional)

## Choose a package

The links point to the file attached to the latest release. CI publishes them when a `v*` version tag is pushed and every
platform job succeeds.

| System | Package | Level |
|---|---|---|
| macOS 11+ (Intel and Apple silicon) | [CursorPie-macos-universal.dmg](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-macos-universal.dmg) | Supported |
| Windows 10 (1809) / 11 | [CursorPie-windows-setup.exe](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-windows-setup.exe) (installer) or [CursorPie-windows.zip](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-windows.zip) | Stable |
| Ubuntu 24.04+ (x86_64) | [CursorPie-ubuntu-24.04-x86_64.tar.gz](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-ubuntu-24.04-x86_64.tar.gz) | X11 pre-stable, Wayland experimental |
| Fedora 44 (x86_64) | [CursorPie-fedora-44-x86_64.tar.gz](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-fedora-44-x86_64.tar.gz) | X11 pre-stable, Wayland experimental |

The macOS CI build has an ad-hoc signature and is not notarized; the Windows CI binaries are not code-signed. macOS may
need **right-click > Open** on first launch and Windows SmartScreen may show a warning. The release page also has the
version's release notes.

## Requirements

| | macOS | Windows | Linux |
|---|---|---|---|
| System | macOS 11 or later | Windows 10 (1809) or 11 | A distribution with an **X11 or Wayland session** (XWayland must be running on Wayland) |
| To build | Rust 1.88+, Xcode Command Line Tools | Rust 1.88+, MSVC build tools | Rust 1.88+, a C linker, libx11, libxi, libxtst and libxrandr development files |
| Permission | Accessibility | none | none on X11; the udev rule `70-cursorpie.rules` on Wayland |

Install Rust with [rustup](https://rustup.rs). The build tools of each system are listed in
[CONTRIBUTING.md](../../CONTRIBUTING.md#prerequisites). To try the source without installing:

```sh
git clone https://github.com/sebastvnd/cursorpie.git
cd cursorpie
cargo run --release
```

On macOS, grant the permission to your terminal app first (see [Permissions on macOS](#permissions-on-macos)).

## macOS

**Install the ready-made build:** download
[CursorPie-macos-universal.dmg](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-macos-universal.dmg),
open it, then drag `CursorPie.app` into **Applications**. This universal image supports both Apple silicon and Intel Macs.
The CI build uses an ad-hoc signature and is not notarized, so the first launch may need **right-click > Open**.

**Build from source:**

```sh
scripts/bundle.sh            # builds target/bundle/CursorPie.app
```

Move `CursorPie.app` to `/Applications` and open it. The app lives in the menu bar and has no Dock icon.

By default the app gets an ad-hoc signature. To sign with your own identity, which also keeps the
Accessibility permission across rebuilds:

```sh
SIGN_ID="Developer ID Application: Your Name (TEAMID)" scripts/bundle.sh
```

**Disk image.** To hand the app to someone else, build a `.dmg` with an Applications shortcut:

```sh
scripts/make-dmg.sh                  # target/bundle/CursorPie-<version>-<arch>.dmg
UNIVERSAL=1 scripts/make-dmg.sh      # one image for Apple silicon and Intel
SIGN_ID="Developer ID Application: Your Name (TEAMID)" NOTARY_PROFILE=my-profile scripts/make-dmg.sh
```

With `SIGN_ID` and `NOTARY_PROFILE` (saved with `xcrun notarytool store-credentials`) the image is signed,
notarized and stapled, so Gatekeeper opens it without a warning. Without them, the first start on another
Mac needs a right-click, then **Open**. A `.sha256` file is written next to the image. If
[`create-dmg`](https://github.com/create-dmg/create-dmg) is installed, the window gets a fixed size and
icon positions.

### Permissions on macOS

CursorPie needs **Accessibility** to watch the mouse and trackpad and send shortcuts, and macOS may also ask for
**Input Monitoring**.

1. Open **System Settings > Privacy & Security**.
2. Enable CursorPie (or your terminal app, if you used `cargo run`) under **Accessibility**, and under
   **Input Monitoring** if asked.

CursorPie checks again every 2 seconds, so it starts working as soon as you grant access. If **Settings >
General** still shows *Event tap: Not running (retrying every 2 s)*, quit and reopen the app. That tab also has buttons that jump
to the right System Settings pages.

An ad-hoc signed build may need the permission again after each rebuild. Sign with `SIGN_ID` to avoid it.

## Windows

**Install the ready-made build:** download and run
[CursorPie-windows-setup.exe](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-windows-setup.exe).
The per-user installer does not need administrator rights, and it shows a short note about programs that run as
administrator (see below). Alternatively, download
[CursorPie-windows.zip](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-windows.zip),
extract it and run `cursorpie.exe`; the first run installs that copy for your user. CI binaries are unsigned,
so Windows SmartScreen may display a warning.

**Build from source:** in PowerShell, with the MSVC build tools installed:

```powershell
scripts\package-windows.ps1     # builds target\bundle\CursorPie-windows.zip
```

**Setup wizard.** Install [Inno Setup 6](https://jrsoftware.org/isinfo.php) (`winget install JRSoftware.InnoSetup`)
and the same script also builds `CursorPie-<version>-setup.exe` (`-NoInstaller` skips it). The wizard installs for
your user only, needs no administrator rights, adds a Start Menu shortcut and an *Apps & features* entry, asks
whether to start CursorPie at sign-in, and updates an older version in place. Your settings are never touched.
Set `SIGN_CERT_THUMBPRINT` to sign the `.exe` files; unsigned installers make SmartScreen show a warning.

**Without the wizard**, unzip and run `cursorpie.exe`. A pie icon appears in the system tray. The first start
installs CursorPie for your user: it copies itself to `%LOCALAPPDATA%\Programs\CursorPie`, turns on
**Launch at login**, adds an entry to **Settings > Apps** and continues from the installed copy. You can delete
the unzipped folder afterwards. Running a newer `cursorpie.exe` updates the installed copy and keeps your
settings. Builds made with `cargo` are never installed, and `CURSORPIE_NO_INSTALL=1` turns the install step off
for any copy.

**Uninstall** from **Settings > Apps > CursorPie > Uninstall**, or run `cursorpie.exe --uninstall`. Your
`config.json` is kept. To only stop it starting at login, untick **Launch at login** in Settings or in the
tray menu.

Windows needs no permission. **One exception: programs that run as administrator.** Windows blocks the input of a
normal program (clicks, shortcuts) to a window that runs as administrator (Task Manager, a terminal opened with
*Run as administrator*, an installer that asked for UAC). In those windows the pie opens, but its slices do nothing.
This is a Windows rule (UIPI), not a fault, and it is why the setup wizard, the first start from the zip and the
welcome screen all mention it. To use the pie there too, right-click the CursorPie shortcut (or `cursorpie.exe`)
and choose **Run as administrator**. Do this only if you need it: it asks for UAC at each start, and everything a
slice runs, a *Shell command* block included, then runs with full rights.

## Linux

Linux support levels: **X11 is pre-stable, Wayland is experimental.** The full picture, including what each Wayland
compositor can do, is in [CursorPie on Linux](linux.md). You do not need development packages for the ready-made archives.

**Install a ready-made build.** Download the archive for your distribution, extract it and run the installer:

```sh
# Ubuntu 24.04:
tar -xzf CursorPie-ubuntu-24.04-x86_64.tar.gz
# Fedora 44: use CursorPie-fedora-44-x86_64.tar.gz instead

cd cursorpie-*-linux-x86_64
./install.sh --user          # installs under ~/.local; no sudo needed
```

`./install.sh` without options installs under `/usr/local` (needs root); `--prefix DIR` chooses another folder.
The script also refreshes the icon cache, so no image tool is needed.

**Build from source.** Install the development packages, then build and install:

```sh
# Debian, Ubuntu
sudo apt install build-essential libx11-dev libxi-dev libxtst-dev libxrandr-dev
# Fedora
sudo dnf install gcc libX11-devel libXi-devel libXtst-devel libXrandr-devel

scripts/package-linux.sh                      # builds target/bundle/cursorpie-<version>-linux-<arch>.tar.gz
tar -xzf target/bundle/cursorpie-*-linux-*.tar.gz
cd cursorpie-*-linux-*
./install.sh --user
```

Build on an older distribution if the binary must also start on older ones (glibc is not backwards compatible).

**Start it** from your applications menu or run `cursorpie`. It can start at login from **Settings > Behavior**.

### Linux checklist

| Your session | Do this |
|---|---|
| X11 | Nothing else. |
| Wayland | Install the udev rule and log out and in once: `sudo cp 70-cursorpie.rules /etc/udev/rules.d/ && sudo udevadm control --reload && sudo udevadm trigger`. Running `install.sh` as root does it for you. |
| Wayland on GNOME, for per-app profiles | Also install the GNOME extension (steps in [CursorPie on Linux](linux.md#what-works-on-each-compositor)). |
| Any, for the file chooser | Install `zenity` or `kdialog`. |

There is no tray icon on Linux yet: Pause and Quit are in **Settings > General**.

## First launch

On every system a short welcome window opens the first time. It shows the gesture, then the one thing to do next (macOS:
allow Accessibility, with a live status; Windows: where the tray icon is; Linux: that there is no tray icon, and on
Wayland the udev rule steps with a live status) and ends with a **Launch at login** box. Skip it with **Skip** or `Esc`.
**Welcome guide...** in the menu bar or tray menu shows it again (Linux has no tray icon, so there the window appears only
on the first start). The empty file `welcome_shown` next to `config.json` records that it was shown; delete it to see the
window at the next start.

## Trackpad hold (optional)

Touching and holding a trackpad can open the pie instead of the middle button. It is off by default: turn on *Open the pie
with a trackpad hold* in **Settings > Behavior > Trackpad** and press Save.

- **macOS:** any trackpad; needs the same Accessibility permission as the mouse.
- **Windows:** needs a *precision* touchpad (most laptops since Windows 10). No permission needed.
- **Linux:** your user must be in the `input` group (`sudo usermod -aG input $USER`, then log out and in); on Wayland the
  udev rule covers it too. If nothing happens, run `cursorpie --touchpad-debug` in a terminal.

The gesture and its settings are explained in [Trackpad hold](trackpad.md).

## Next steps

- [Getting started](getting-started.md)
- [Using the pie](using-the-pie.md)
- [Configuration](configuration.md)
