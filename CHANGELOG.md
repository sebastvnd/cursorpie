# Changelog

## Unreleased

## 0.5.4

- **More tools to read the diagnostic log.** `cargo tools log` has seven views: `summary` (what was written and what
  floods the log), `timeline` (one row per press with the hold, UI and window times), `grep` (readable lines of one
  category or event, no `jq`), `slow` (slow timed steps), `diff` (what a fix changed between two logs), `follow` (a
  run that is still going) and `report` (facts and verdict for a bug report). `cargo tools diag` now also reports a
  slow pie opening, slow steps, a config that could not be saved and an action stuck in the queue.
- **`cargo tools diag-lint` keeps the log and the analyzer together.** It checks every log line in `src/` (known
  category, `snake_case`, no field named like user data, no repeated field) and that each event the analyzer reads is
  still written with its fields. It runs in `cargo tools ci --quick` and as a unit test.
- **More log points where bugs hide:** whether the input hook runs (`tap` / `hook`: a missing macOS permission or a
  Windows refusal is now in the log), why a hold did not open the pie (`hold_cancelled`), the double-click window and
  replayed clicks, which profile a press resolved to, which slice was selected, the side buttons, opening and closing
  Settings, config import, the icon warm-up and the sleep (timed), and every program an action starts (name only,
  never its arguments): whether it started, and how it ended. `cargo tools diag` turns the failures among them into
  findings.
- **New log points:** `cfg` (loading and saving `config.json`, so a failed save is no longer silent), `act` (kind,
  queue wait and run time of every action, never its content) and `tap` / `on_release` (every backend).

## 0.5.3

- **The diagnostic log is its own crate.** The code that writes the `CURSORPIE_DEBUG=1` log moved out of the
  app's sources (`src/diag.rs`) into `diag/`, a crate of the workspace with no dependencies; the app only keeps
  a small glue module (`src/debug.rs`). Log lines are now `diag::log!("ui", "show_pie", x, y = y, kind = ?kind)`:
  a bare name logs the variable, `?` logs the `Debug` text. New: `diag::trace!`, `diag::once!`, `diag::timed!`,
  `diag::event(..)` for names known only at run time, and `diag::Watch` (what was asked for against what
  happened). The switch accepts `all,-tap` (leave a category out) and `stderr` (also print on the terminal).
- **The diagnostic log is not in release builds.** It is part of debug builds only; an ordinary release build has
  none of its code. `cargo build --release --features debug-log` keeps it, for collecting a log on someone else's
  machine.
- **New developer tools:** `cargo tools clean-logs`, `cargo tools zip` (the project without `target/`) and
  `cargo tools info` (facts for a bug report).

## 0.5.2

- Diag code for developer
- Fix bug pie position in x11
- Fix bug pie position in linux
- New documentation and repair memory management

## 0.5.1

- **Fix bug many display on macOS**
- **Lower memory use in Settings.** The icon picker only draws (and only loads) the icons that are on
  screen, and loads a few per frame instead of all at once. Image icons larger than 4096 px on a side are no
  longer decoded, and a decoded image no longer exists in several full-size copies while it is scaled down.
  The "Add a slice" tiles are built once when the panel opens and freed when it closes. Importing a config
  reads at most 4 MiB.
- **Big images are shrunk when you choose them.** Choosing an image larger than 512 px or 512 KB as a slice icon
  saves a 256 px PNG copy in the icons folder and uses that (the original is untouched), and the status bar
  says what was done. An image that cannot be read, or has more than 40 megapixels, is refused with a message.
  The "Mine" page tells you when images in the icons folder are not listed because they are too big.
- **Fix bug pie draw center position.**
- **Add debug/diag tools.** 

## 0.5.0

- **Open app follows the app to its desktop or Space.** An *Open app* slice for a program that is already
  running on another virtual desktop (Windows) or Space (macOS) now takes you there and brings its window to
  the front, instead of leaving you where you were. **Settings > Behavior > Other desktops / Other Spaces**
  chooses: *Switch to the desktop where the app is* (the default), *Do nothing* (open the app on the current
  desktop as before) and, on macOS, *Bring the app's window to this Space* (experimental: it relies on private
  system calls and falls back to switching when macOS refuses them). On macOS the switch works with the
  Mission Control shortcuts Control+Left / Right (on by default) even when the system setting *When switching
  to an application, switch to a Space with open windows for the application* is off. If the app is not
  running, or has a window on the current desktop, nothing changes. On macOS the nearest Space with a window of
  the app is gone to directly, left or right and however many Spaces away, without stopping in between, and
  the window is raised and checked to be in front. How it works: `docs/OTHER_DESKTOPS.md`.
- **Linux (X11).** CursorPie now runs on Linux in an X11 session: the hold, flick, release and double-click
  gestures, per-app profiles (apps are recognized by their desktop-entry id, for example `firefox` or
  `org.mozilla.firefox`), side-button modes, shortcut recording, launching apps, links and commands, and
  "Launch at login" through an XDG autostart entry. No permission is needed. The mouse is watched with
  XInput2 grabs and shortcuts are sent with XTest. `scripts/package-linux.sh` builds a tarball with an
  installer. Building needs the development files of libx11, libxi, libxtst and libxrandr (see
  `CONTRIBUTING.md`). The design and what is and is not built are in `docs/LINUX_MIGRATION.md`.
- **Linux: Open app follows the app to its workspace.** The window manager does the switching: an *Open app*
  slice for a program that is already running asks it to activate the app's window, and the usual window
  managers go to the workspace that window is on. **Settings > Behavior > Other workspaces** offers *Switch*
  (the default) and *Do nothing*; *Bring the window* does not exist on Linux and acts as *Switch*. An app that
  has no window is started as before. How well this works depends on the window manager and has not been
  checked on real desktops.
- **Linux: defaults and files.** Built-in shortcuts use Ctrl and the Super key (instead of Cmd), and redo is
  Ctrl+Shift+Z. The Launch and quick-action slices run shell commands that work on every desktop (a file
  manager through `xdg-open`, the first terminal and screenshot tool that is installed, `loginctl
  lock-session`, Super+. for the emoji picker). Settings and backups are kept in `~/.config/cursorpie` (or
  `$XDG_CONFIG_HOME/cursorpie`), and exports go to your download folder as `user-dirs.dirs` names it, for
  example `~/Unduhan` on an Indonesian system. `config.json` keeps its format, so a file from macOS or Windows
  still opens (app ids from another OS are kept but do not match).
- **Linux: what is missing.** Wayland sessions only work for X11 and XWayland windows: log in with an X11
  session for full support. There is no tray icon yet, so Pause and Quit are in **Settings > General** and
  Settings opens by holding the trigger in the middle of the pie. Also missing: app icons in the pie,
  keyboard-layout labels on non-US layouts, and extra mouse buttons as the trigger (only the middle button).
  Handing a **drag** back to the app while the middle button is held (for example middle-drag scrolling) may
  lose the start of the drag; a quick click is passed on reliably. The file chooser buttons need `zenity` or
  `kdialog`.

## 0.4.2

- **Welcome window.** The first start on macOS and on Windows opens a three-page welcome window in the
  Settings style: how the gesture works, the step that is needed next (macOS: allow Accessibility, with a live
  status; Windows: where the tray icon is) and a "Launch at login" box. It is shown once (the empty file
  `welcome_shown` next to `config.json` remembers it), never to someone who already has a `config.json`, and
  **Welcome guide...** in the menu bar / tray menu opens it again.
- **Windows: Setup wizard.** `scripts\package-windows.ps1` also builds `CursorPie-<version>-setup.exe` when
  Inno Setup 6 is installed (`installer/cursorpie.iss`): a per-user installer (no administrator rights) with
  license page, Start Menu shortcut, "start at sign-in" option, update-in-place and an uninstaller that keeps
  your settings. A copy installed by the wizard no longer installs itself again or adds a second "Apps &
  features" entry; an older self-install entry is removed. `SIGN_CERT_THUMBPRINT` signs the files.
- **macOS: disk image.** `scripts/make-dmg.sh` builds `CursorPie-<version>-<arch>.dmg` with an Applications
  shortcut, optionally signed and notarized (`SIGN_ID`, `NOTARY_PROFILE`), plus a `.sha256` file.
  `UNIVERSAL=1` (also understood by `scripts/bundle.sh`) builds one app for Apple silicon and Intel. A
  Developer ID signature now uses the hardened runtime, which notarization requires.
- Tests that assumed macOS defaults are pinned to them, so the test suite passes on Windows.

## 0.4.1

- Fix dead code warnings on the Windows target

## 0.4.0

- **Developer tools.** `cargo tools` runs the repository's own tools (the `tools/` package, standard
  library only, not part of the app). `cargo tools bump patch|minor|major` raises the version in
  `Cargo.toml` and `Cargo.lock`, turns the notes under `## Unreleased` (or, without notes, the commit
  subjects since the last release) into the changelog entry, leaves a fresh `## Unreleased` section and
  can commit and tag the release from `main`. It lists the steps that stay manual and warns when
  something is probably stale. `cargo tools check` verifies that the files agree (and, with `--tag`,
  that a tag matches them), `cargo tools ci` runs every check CI runs, and `cargo tools hooks install`
  turns on Git hooks that check formatting and commit messages. This replaces the earlier
  `cargo run --example bump`. The workflow and its rules are in `CONTRIBUTING.md`, "Development
  workflow" and "Releasing".
- **Windows: CursorPie installs itself.** The first start copies the program to
  `%LOCALAPPDATA%\Programs\CursorPie`, turns on "Launch at login" for that copy, adds an "Apps & features"
  entry (`cursorpie.exe --uninstall` removes it, settings are kept) and continues from the installed copy.
  Before, the startup entry pointed at wherever the `.exe` was started from and had to be switched on by hand,
  so the app was gone after a restart as soon as the download folder was moved or cleaned. A newer download
  updates the installed copy. `cargo` builds and `CURSORPIE_NO_INSTALL=1` skip the install.
- **Windows: one running copy.** A second start exits at once instead of installing a second mouse hook.
- **Windows: "Launch at login" honors Task Manager.** An entry switched off in Task Manager reads as off in
  Settings, and ticking the box switches it on again.
- **Settings has a sidebar.** The sections (Menu, Behavior, General, About) are listed on the left and the
  selected page is on the right, like the settings of a large application. Behavior is one column of panels.
  The window is 1120 points wide (it shrinks on small displays) so the Menu editor keeps its room.

## 0.3.0

- **Windows 10/11 support.** The same code base now builds and runs on Windows and behaves almost
  identically to macOS: global mouse and keyboard hooks, app detection, per-app profiles, shortcuts,
  launchers, system tray, launch at login, Settings. Config files and shared profiles are portable
  between the two systems. See `docs/WINDOWS_MIGRATION.md`.
- **Truly transparent pie on Windows.** The Windows build uses the `glow` renderer, and the window
  frame that winit leaves on borderless windows is removed (`platform::clear_window_frame`): no white
  box or border around the pie, no old-style window flashing up when it opens, no thin white line
  along the top edge.
- Not at parity on Windows: extra mouse buttons. Only the middle button can be the trigger (Windows
  reports only the middle, back and forward buttons).
- Known limits on Windows: per-app profiles cannot tell UWP / Microsoft Store apps apart (they all
  report `applicationframehost.exe`), and programs running as administrator ignore CursorPie's input unless
  CursorPie runs as administrator too.
- macOS behavior is unchanged.

## 0.2.0

- macOS only. Config format version 2.
