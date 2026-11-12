# Updates

CursorPie looks for a newer release and can install it. The same steps on macOS, Windows and Linux; this page says
what each system does and how to test it. The code is `src/update/` (`mod.rs`: the daily rule, `install.rs`: the
install, `sha256.rs`) and `src/platform/http.rs` (the requests).

## When

When Settings opens (never the welcome window), at most once a day (UTC days) and only while **About > Updates >
Check for updates automatically** is on (`check_updates`): CursorPie asks `api.github.com` for the newest *stable*
release. A newer one is offered once that day in a sheet; the About tab always shows the status. **Check now** works
with the switch off. A check that fails (no network, no `curl`/`wget`) is tried again the next time Settings opens.
What is remembered (day of the last check, newest version, day of the last offer) is `update_state.json` next to
`config.json`.

## What *Install update* does

1. Chooses the package of the release for this system (names from `.github/workflows/ci.yml`):
   macOS `CursorPie-macos-universal.dmg`, Windows `CursorPie-windows-setup.exe`, Linux x86_64
   `CursorPie-ubuntu-24.04-x86_64.tar.gz` (Fedora, RHEL and CentOS family: `CursorPie-fedora-44-x86_64.tar.gz`).
2. Downloads it with `curl` (or `wget`) from `https://github.com/sebastvnd/cursorpie/releases/download/<tag>/<name>`,
   an address made from the tag, never taken from the answer, into `updates/` in the app support folder.
3. Checks the size and the SHA-256 against what GitHub publishes for the file (`digest` in the release data, or the
   `.sha256` file of the macOS image). **With no checksum nothing is installed.** This guards against a damaged or
   swapped download; it is not a signature.
4. Stages it: macOS mounts the image and copies `CursorPie.app` next to the installed one; Linux unpacks the archive and
   copies the program next to the installed one; both run the new program once with `--selftest` (so a build that does
   not start on this system, for example with a C library that is too old, never replaces the working one). Windows
   keeps the Setup program as it is.
5. Swaps it in. CursorPie quits (unsaved changes in Settings stop this: save or cancel, then **Restart to update**).
   macOS and Linux: a small shell script waits for CursorPie to end, moves the old copy aside, moves the new one in
   (the old one is put back if that fails) and starts the program. Windows: Setup runs silently
   (`/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /MERGETASKS=!startup /RESTARTAPP=1`), replaces the files for the current
   user and starts the new version; the "start at login" choice is left as it is.

Only an ordinary install is updated: macOS `CursorPie.app` in a folder you can write to, Windows
`%LOCALAPPDATA%\Programs\CursorPie\cursorpie.exe`, Linux a program outside a `target/` folder in a folder you can
write to. Anything else gets **Download page**. On macOS the app is signed ad hoc, so macOS may ask for the
Accessibility permission again after an update.

## What an update brings besides the program

The swap replaces the program. The files CursorPie keeps *outside* its executable, which it wrote for you and which a
release can change, are brought up to date by the program itself at the **first start of a new version** (`src/refresh.rs`),
so it makes no difference how the new version got there (the update button, Setup, a disk image, `install.sh`, a package
manager). The version that did it last is remembered in `program_version` in the support folder; a copy from before that
file existed counts as "not done yet".

| File | Brought up to date when |
|---|---|
| Browser extension (`<support folder>/browser-extension`) | you exported it there before (**Show the extension files**); nothing is created for someone who never did. The extension that is loaded then reloads itself, see `docs/BROWSER_EXTENSION.md` |
| Login entry (LaunchAgent, `Run` key, autostart file) | *Launch at login* is on and this is an ordinary install (the test of the update: a build folder never takes it over) |
| Linux: `~/.local/share/applications/cursorpie.desktop` | it is there and still CursorPie's own (its `Name` and `Exec` lines are the shipped ones); one you edited stays |
| Linux: the GNOME Shell extension in `~/.local/share/gnome-shell/extensions/` | the folder is there. GNOME reads an extension once per session, so the new code runs after the next log in |

**Never touched:** `config.json` (profiles, slices, flows, settings), its backups, `update_state.json`. What "Add a slice"
offers, the panel templates and the flow kit are compiled into the program, so a new version has them without any file
being written. Not written either, because they are not in your own folders: the udev rule (`/etc/udev/rules.d`, needs
root) and the icons; `install.sh` does those. Everything is best effort: a file that cannot be written stays as it was.

## Checking it by hand

There is no way to test a real swap in CI, so before a release that changes `src/update/` or the packaging, on each
system, install the *previous* release, publish (or point `LATEST_RELEASE_API_URL` at) a newer test release and check:

- [ ] Settings opens, the sheet appears once; reopening the same day does not show it again; the next day it does.
- [ ] **Not now**, Esc and a click outside close the sheet; About still shows the update.
- [ ] **Install update**: status goes *Downloading*, then the app quits and the new version starts (About shows the new
      number). Settings, pies and the login entry are unchanged.
- [ ] With unsaved changes in Settings the app does not quit; **Restart to update** works after saving.
- [ ] After the update, with the extension exported before and loaded: `browser-extension/` holds the new version (its
      `manifest.json`), and within a minute the extension has reloaded itself (the line under the switch no longer says
      *from another version*) without pressing **Show the extension files**. Loaded from another folder: it is asked at most
      a few times and the line says what to do.
- [ ] Linux: an old `~/.local/share/applications/cursorpie.desktop` is the new one; an edited one is left alone.
- [ ] Offline: **Check now** says *Could not check*, nothing else changes. A tampered file (change one byte of the
      downloaded package while it sits in `updates/`) is refused as *not the file the release published*.
- [ ] A copy run from `target/release` (or an unzipped Windows download) shows **Download page**, not *Install update*.
- [ ] macOS: `/Applications` owned by an admin and by a standard user; Apple silicon and Intel. Windows: from the zip
      self-install and from Setup. Linux: `/usr/local/bin` (not writable as a normal user: **Download page**) and
      `~/.local/bin`; Ubuntu and Fedora.
