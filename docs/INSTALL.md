# Install CursorPie

CursorPie runs on macOS, Windows and Linux (X11). You can install a ready-made package from the latest
[GitHub Release](https://github.com/sebastvnd/cursorpie/releases/latest), or build from source.

## Download a ready-made package

These links point to the matching file attached to the latest release. CI publishes them automatically
when a `v*` version tag is pushed and every platform job succeeds. Until that first release is
published, the direct-download links return 404. Choose the package
for your system:

| System | Ready-to-use file |
|---|---|
| macOS 11+ (Intel and Apple silicon) | [CursorPie-macos-universal.dmg](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-macos-universal.dmg) |
| Windows 10/11 | [CursorPie-windows-setup.exe](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-windows-setup.exe) (installer) or [CursorPie-windows.zip](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-windows.zip) |
| Ubuntu 24.04+ (x86_64) | [CursorPie-ubuntu-24.04-x86_64.tar.gz](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-ubuntu-24.04-x86_64.tar.gz) |
| Fedora 44 (x86_64) | [CursorPie-fedora-44-x86_64.tar.gz](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-fedora-44-x86_64.tar.gz) |

The macOS CI build uses an ad-hoc signature and is not notarized; the Windows CI binaries are not code-signed.
macOS may require **right-click > Open** on first launch, and Windows SmartScreen may show a warning. Linux
packages are built on the named distribution and require an X11 session. The release page also contains the
version-specific release notes.

## Requirements

| | macOS | Windows | Linux |
|---|---|---|---|
| System | macOS 11 or later | Windows 10 (1809) or 11 | A distribution with an **X11 session** |
| To build | Rust 1.88+, Xcode Command Line Tools | Rust 1.88+, MSVC build tools | Rust 1.88+, a C linker, libx11, libxi, libxtst and libxrandr development files |
| Permission | Accessibility | none | none |

Install Rust with [rustup](https://rustup.rs). Platform build tools are listed in
[CONTRIBUTING.md](../CONTRIBUTING.md#prerequisites).

## Try it without installing

```sh
git clone https://github.com/sebastvnd/cursorpie.git
cd cursorpie
cargo run --release
```

On macOS, grant the permission to your terminal app (see [Permissions](#permissions-on-macos)).

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

CursorPie needs **Accessibility** to watch the mouse and send shortcuts, and macOS may also ask for
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
The per-user installer does not need administrator rights. Alternatively, download
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

Windows needs no permission. To control windows of programs that run as administrator, start CursorPie as
administrator too, because Windows blocks input to them otherwise.

## Linux (X11)

Linux (X11) is supported but **experimental**: it works, but its behavior on different distributions and
desktops is still being observed, so expect rough edges and please report them. You do not need development
packages to install one of the ready-made archives. Log in with an X11 session before launching CursorPie.

**Install a ready-made build:** download the package matching the distribution where you will run CursorPie:

- [Ubuntu 24.04+ (x86_64)](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-ubuntu-24.04-x86_64.tar.gz)
- [Fedora 44 (x86_64)](https://github.com/sebastvnd/cursorpie/releases/latest/download/CursorPie-fedora-44-x86_64.tar.gz)

Extract the downloaded archive, enter its versioned directory, and run the included installer:

```sh
# Ubuntu 24.04:
tar -xzf CursorPie-ubuntu-24.04-x86_64.tar.gz
# Fedora 44: use CursorPie-fedora-44-x86_64.tar.gz instead

cd cursorpie-*-linux-x86_64
./install.sh --user          # installs under ~/.local; no sudo needed
```

**Build from source:** install the development packages first, then build and install:

```sh
# Debian, Ubuntu
sudo apt install build-essential libx11-dev libxi-dev libxtst-dev libxrandr-dev
# Fedora
sudo dnf install gcc libX11-devel libXi-devel libXtst-devel libXrandr-devel

scripts/package-linux.sh                      # builds target/bundle/cursorpie-<version>-linux-<arch>.tar.gz
tar -xzf target/bundle/cursorpie-*-linux-*.tar.gz
cd cursorpie-*-linux-*
./install.sh --user                           # under ~/.local; plain ./install.sh uses /usr/local
```

Start CursorPie from your applications menu or run `cursorpie`. It needs no permission and can start at login
from **Settings > Behavior**. The package carries the icon in every size (16 to 512 px) and `install.sh` refreshes
the icon cache, so no image tool is needed. Build on an older distribution if the binary must also start on older ones.

Good to know:

- Log in with an **X11 session**. In a Wayland session CursorPie only sees X11 and XWayland windows. See
  [KNOWN_ISSUES.md](KNOWN_ISSUES.md).
- There is no tray icon yet. Pause and Quit are in **Settings > General**, and Settings opens by holding the
  trigger in the middle of the pie.
- A minimal system may lack libraries the window toolkit loads at run time (OpenGL, libxkbcommon-x11, libxcursor).
- Install `zenity` or `kdialog` to use the file chooser buttons.

## Trackpad hold (optional)

Touching and holding a trackpad can open the pie instead of the middle button. It is off by default: turn on *Open
the pie with a trackpad hold* in **Settings > Behavior > Trackpad** and press Save.

- **macOS:** works with any trackpad and needs the same Accessibility permission as the mouse.
- **Windows:** needs a *precision* touchpad (most laptops since Windows 10). No permission is needed.
- **Linux (X11):** your user must be in the `input` group: `sudo usermod -aG input $USER`, then log out and in.
  If nothing happens, run `cursorpie --touchpad-debug` in a terminal.

The [user guide](USER_GUIDE.md#trackpad-macos-windows-and-linux-x11) explains the gesture and its settings.

## First launch

On every system a short welcome window opens the first time. It shows the gesture, then the one thing to
do next (on macOS, allow Accessibility with a live status; on Windows, where to find the tray icon; on Linux, that
there is no tray icon and that Pause and Quit are at the bottom of Settings) and ends with a **Launch at login**
box. Skip it with **Skip** or `Esc`. **Welcome guide...** in the menu bar or tray menu shows it again (Linux has
no tray icon, so there the window appears only on the first start). The "already shown" flag is the empty file `welcome_shown` next to `config.json`; delete it to see the
window at the next start.

## Next steps

- [User guide](USER_GUIDE.md): gestures, slices, profiles and Settings.
- [Configuration](CONFIGURATION.md): where `config.json` lives and what it contains.
