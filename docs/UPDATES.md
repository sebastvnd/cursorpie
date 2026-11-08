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
write to. Anything else gets **Download page**. On Linux only the program is replaced (not the desktop entry, the icons
or the udev rule); run the new `install.sh` for those. On macOS the app is signed ad hoc, so macOS may ask for the
Accessibility permission again after an update.

## Checking it by hand

There is no way to test a real swap in CI, so before a release that changes `src/update/` or the packaging, on each
system, install the *previous* release, publish (or point `LATEST_RELEASE_API_URL` at) a newer test release and check:

- [ ] Settings opens, the sheet appears once; reopening the same day does not show it again; the next day it does.
- [ ] **Not now**, Esc and a click outside close the sheet; About still shows the update.
- [ ] **Install update**: status goes *Downloading*, then the app quits and the new version starts (About shows the new
      number). Settings, pies and the login entry are unchanged.
- [ ] With unsaved changes in Settings the app does not quit; **Restart to update** works after saving.
- [ ] Offline: **Check now** says *Could not check*, nothing else changes. A tampered file (change one byte of the
      downloaded package while it sits in `updates/`) is refused as *not the file the release published*.
- [ ] A copy run from `target/release` (or an unzipped Windows download) shows **Download page**, not *Install update*.
- [ ] macOS: `/Applications` owned by an admin and by a standard user; Apple silicon and Intel. Windows: from the zip
      self-install and from Setup. Linux: `/usr/local/bin` (not writable as a normal user: **Download page**) and
      `~/.local/bin`; Ubuntu and Fedora.
