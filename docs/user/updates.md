# Updates

CursorPie looks for a newer release and can install it. The steps are the same on macOS, Windows and Linux; this page
says what each system does. How to test the updater before a release is in the
[developer notes](../developer/UPDATES_TESTING.md).

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
| Browser extension (`<support folder>/browser-extension`) | you exported it there before (**Show the extension files**); nothing is created for someone who never did. The extension that is loaded then reloads itself, see [the browser extension guide](../BROWSER_EXTENSION.md) |
| Login entry (LaunchAgent, `Run` key, autostart file) | *Launch at login* is on and this is an ordinary install (the test of the update: a build folder never takes it over) |
| Linux: `~/.local/share/applications/cursorpie.desktop` | it is there and still CursorPie's own (its `Name` and `Exec` lines are the shipped ones); one you edited stays |
| Linux: the GNOME Shell extension in `~/.local/share/gnome-shell/extensions/` | the folder is there. GNOME reads an extension once per session, so the new code runs after the next log in |

**Never touched:** `config.json` (profiles, slices, flows, settings), its backups, `update_state.json`. What "Add a slice"
offers, the panel templates and the flow kit are compiled into the program, so a new version has them without any file
being written. Not written either, because they are not in your own folders: the udev rule (`/etc/udev/rules.d`, needs
root) and the icons; `install.sh` does those. Everything is best effort: a file that cannot be written stays as it was.

> **Linux note.** The updater only handles an ordinary install: a program outside a `target/` folder, in a folder you can
> write to (for example `~/.local/bin`). A copy in `/usr/local/bin` that your user cannot write to shows **Download page**.
> The Fedora package is chosen for the Fedora, RHEL and CentOS family, the Ubuntu one for everything else.
