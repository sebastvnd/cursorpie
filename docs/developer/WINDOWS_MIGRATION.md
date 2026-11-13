# Windows support: design and status

**Status: Windows 10/11 is supported and stable.** CursorPie 0.2.0 was macOS only; since 0.3.0 the
same code base builds and runs on Windows, and it behaves almost identically to macOS: the
hold-to-open pie, per-app profiles, shortcuts, launchers, submenus, Settings and the tray icon are
the same features as on macOS, and the pie is drawn on a truly transparent window. What Windows does
**not** have is the extra mouse buttons: the trigger can only be the middle button (Windows reports
only the middle, back and forward buttons, so macOS buttons 6 to 8 do not exist), and the
back/forward side buttons are the only other buttons CursorPie uses. See [Status](#status) for what
was verified and which limits remain.

This document explains what was macOS-specific, the design that supports macOS **and** Windows 10/11, and
the order in which the code was migrated so macOS behavior never changed along the way. The text below
describes the design and the history of the port; where it says "today" it means 0.2.0, the last macOS-only
release.

> **Since this port:** Linux (X11) was added as a third platform. It is described in
> [LINUX_MIGRATION.md](LINUX_MIGRATION.md). This document stays the record of the Windows port, and the
> "0.3.0" and "0.2.0" labels below are the versions it was written for.

Status of each phase is tracked at the end ([Progress](#progress)).

## Goals and non-goals

Goals

- One code base, one `config.json` format, two supported platforms: macOS 11+ and Windows 10 (1809+) / 11.
- **No behavior change on macOS.** Existing `config.json` files, shared profile files and the
  `.app` bundle keep working. macOS code is moved, not rewritten.
- Everything that is a *decision* (gesture state machine, shortcut model, defaults, config paths,
  DPI math, key tables) is plain Rust that can be unit tested on any OS, including Linux CI.
- Windows needs no special permission and no installer: a single `cursorpie.exe` in the tray.

Non-goals (0.3.0)

- Linux support. At the time, a *stub* backend existed only so `cargo test` runs on Linux. The real Linux
  backend came later and is described in [LINUX_MIGRATION.md](LINUX_MIGRATION.md).
- Extra mouse buttons 5 to 7 as trigger (Windows only reports middle, back and forward).
- Perfect behavior on mixed-DPI multi-monitor setups (see [Risks](#risks-and-known-limitations)).

## What was macOS-specific (0.2.0)

| Area | Files | macOS API | Windows counterpart |
|---|---|---|---|
| Global mouse/keyboard hook | `tap/mod.rs`, `tap/*.rs` | `CGEventTap` + `CFRunLoop` | `SetWindowsHookExW(WH_MOUSE_LL / WH_KEYBOARD_LL)` + message loop |
| Event types in the state machine | `tap/trigger.rs`, `side_buttons.rs`, `right_click.rs`, `keyboard.rs`, `gesture.rs`, `resolve.rs`, `geometry.rs` | `CGEvent`, `CGEventType`, `CGPoint` | portable `Point`, `Kind`, `Verdict` |
| Sending keys / replaying clicks | `actions.rs` | `CGEvent::post` | `SendInput` |
| Frontmost app, app under pointer | `platform/apps.rs` | `NSWorkspace`, `CGWindowList` | `GetForegroundWindow`, `WindowFromPoint`, `QueryFullProcessImageNameW` |
| App icons | `platform/apps.rs`, `ui/appicon.rs` | `NSWorkspace` icon as TIFF | `SHGetFileInfoW` + `GetDIBits` (RGBA) |
| Permission | `platform/permissions.rs` | `AXIsProcessTrusted`, System Settings URLs | none needed |
| Keyboard layout | `platform/layout.rs` | TIS + `UCKeyTranslate` | `GetKeyboardLayout`, `MapVirtualKeyExW`, `ToUnicodeEx` |
| Launch at login | `platform/login_item.rs` | LaunchAgent plist | `HKCU\...\Run` value pointing at the installed copy |
| Install, uninstall | `platform/windows/install.rs` | none (the `.app` is the install) | copy to `%LOCALAPPDATA%\Programs\CursorPie`, `HKCU\...\Uninstall` entry, `--uninstall` |
| One running copy | `platform/windows/instance.rs` | none | named mutex `Local\CursorPie-single-instance` |
| Return memory to the OS | `platform/memory.rs` | `malloc_zone_pressure_relief` | `HeapCompact` on the process heap (`platform/windows/power.rs`); not `SetProcessWorkingSetSize(-1, -1)`, which evicts the hook code too and makes the first press after idle slow |
| Not being throttled while hidden | `platform/macos/activity.rs` | `NSProcessInfo beginActivity` (opts out of App Nap) | `SetProcessInformation(ProcessPowerThrottling)` (opts out of EcoQoS), in `platform/windows/power.rs` |
| File chooser, reveal, open, tick | `platform/shell.rs`, `ui/settings/icon_catalog.rs` | `osascript`, `open`, `mdls`, `afplay` | `GetOpenFileNameW`, `explorer`, `ShellExecuteW`, `MessageBeep` |
| Run actions | `actions.rs` | `open -a`, `open`, `/bin/sh -c` | `ShellExecuteW`, `cmd /C` |
| Menu bar icon | `ui/tray.rs` | `tray-icon` (template image) | `tray-icon` (coloured image) |
| Dock icon | `lib.rs`, `platform/apps.rs` | `NSApplication` activation policy | none (use `with_taskbar(false)`) |
| Config location | `config/store.rs` | `~/Library/Application Support/CursorPie` | `%APPDATA%\CursorPie` |
| Display bounds | `ui/screen.rs` | `CGDisplay` | `EnumDisplayMonitors` |
| Defaults | `config/defaults.rs` | Cmd shortcuts, bundle ids, Finder/Safari | Ctrl/Win shortcuts, exe names, Explorer/Edge |
| Build gate | `lib.rs`, `Cargo.toml` | `compile_error!`, unconditional macOS crates | `cfg` gating, target-specific dependencies |
| UI text | `ui/settings/**`, `about.rs` | "Cmd", "Finder", "Accessibility", "Mac" | per-platform strings |
| Packaging | `scripts/bundle.sh` | `.app` + `codesign` | `scripts/package-windows.ps1` |

## Design decisions

1. **Key codes stay macOS virtual key codes in `config.json`.** `Shortcut { key, mods }` is a
   physical key position (the same idea as a PS/2 scan code), so the file format does not change and
   a profile exported on a Mac imports on Windows. The Windows backend translates at the edge:
   layout keys (letters, digits, punctuation) by scan code, special keys (Return, arrows, F-keys,
   numpad...) by virtual-key code. The tables live in plain Rust (`platform/keymap.rs`) and are tested.
2. **A "primary modifier" per OS.** `MOD_CMD` keeps meaning the Command key on macOS and the Windows
   key on Windows. Built-in presets use `MOD_PRIMARY` (Cmd on macOS, Ctrl on Windows), so "Undo" is
   Cmd+Z on a Mac and Ctrl+Z on Windows. Labels use the platform's names (Cmd/Opt vs Win/Alt).
3. **"Bundle id" becomes "app id".** On macOS it is still the bundle identifier. On Windows it is the
   lower-case executable name (`chrome.exe`). Profiles store it in the same `apps` field. Matching is
   case-insensitive on Windows only.
4. **Coordinates are logical points.** `geometry::Point` is in the same unit egui uses. macOS already
   reports points; the Windows backend divides physical pixels by the monitor scale (`dpi / 96`).
5. **The event layer is portable.** Native glue (`tap/macos.rs`, `tap/windows.rs`) converts a native
   event into `Ev` and the portable `dispatch` returns a `Verdict` (`Keep` or `Drop`). The state
   machine and all its locking rules (see `guides::architecture`) are untouched. On Windows, mouse
   *moves* are never swallowed: the hook always passes them on, because dropping them would freeze
   the cursor.
6. **Backends are modules with identical function names.** `platform/mod.rs` picks `macos`, `windows`
   or `stub` with `cfg` and re-exports it, so call sites do not contain `cfg` attributes (except
   the window builder in `lib.rs`).
7. **Windows code is type-checkable on any OS.** The Windows backend avoids `std::os::windows`,
   compiles under `--features check-windows` on Linux/macOS, and its pure helpers have unit tests that
   run everywhere.
8. **No new dependencies besides `windows-sys`** (Windows only). The tray, config and image crates
   are unchanged; `tray-icon` is restricted to macOS and Windows.

## Phases

Each phase is one commit and leaves macOS working. Tests come first.

| # | Phase | Exit criteria |
|---|---|---|
| 0 | This document | reviewed |
| 1 | **Characterization tests** (before touching any code) pinning today's macOS behavior: shortcut labels, key names, presets, default config, JSON shape of actions, config parsing, geometry, path helpers | tests added, pass on macOS |
| 2 | **Portable core:** `geometry::Point`; the tap split into portable `dispatch` + `tap/macos.rs` glue; `platform/` split into `macos/` and `stub/`; Cargo target-specific dependencies; `compile_error!` removed | all old and new tests pass; `cargo test` also passes on Linux through the stub |
| 3 | **Windows backend:** hook, `SendInput`, app lookup, DPI, layout, registry, shell, file chooser, icons, tray | `cargo check --features check-windows` is clean; pure helpers tested |
| 4 | **Per-OS defaults and text**, config paths, case-insensitive app ids, README / CONTRIBUTING / rustdoc guides, CI for macOS + Windows + Linux, Windows packaging script | tests cover both `Os::Mac` and `Os::Windows` defaults |
| 5 | **Manual verification on real machines** using the checklist below | done: main flows verified on real Windows machines; Windows is treated as stable, and the checklist stays the regression list |

## Test strategy

- **Characterization first (phase 1).** Constants are asserted literally (for example Cmd+Z is
  `Shortcut { key: 6, mods: 8 }`) so a refactor that changes macOS output fails loudly. Tests that
  need the keyboard layout use the built-in US/ANSI fallback, which is what test threads see on
  every OS (the layout snapshot is only built on the main thread).
- **Parametrize on `Os`.** Defaults, labels and paths take the target OS as an argument
  (`presets_for(Os::Windows)`), so one test run covers both platforms.
- **Plain data tables are tested for consistency:** no duplicate scan codes, every mapped key
  round-trips (`mac code -> native -> mac code`), special names exist for every special key.
- **What cannot be unit tested** (hooks, `SendInput`, tray, window behavior) is covered by the manual
  checklist and by CI compiling the real target.

### Manual verification checklist (phase 5)

Run on macOS 11+ and on Windows 10 and 11:

1. Hold the middle button for the hold time (150 ms by default): pie opens under the cursor; release over a slice runs it.
2. Quick middle click passes through; middle-drag still pans in a browser or design tool.
3. Double-click opens the app's own pie in an app that has a profile.
4. Back/forward buttons send the active mode's shortcuts; right-click while the pie is open switches pies.
5. Settings: record a shortcut (including Esc to cancel), undo/redo, save, backup restore, import/export.
6. A shortcut slice reaches the app under the pointer (focus is moved first).
7. Tray menu: Settings, About, Pause, Launch at login (reboot or re-login), Quit.
7a. Install: run `cursorpie.exe` from an unzipped folder. The pie icon appears, the file exists in
    `%LOCALAPPDATA%\Programs\CursorPie`, and CursorPie is listed in Settings > Apps. Delete the unzipped folder,
    sign out and in: the tray icon comes back. Start the downloaded exe again: nothing new appears (one copy only).
7b. Uninstall: Settings > Apps > CursorPie > Uninstall. The tray icon disappears, the install folder is gone a
    few seconds later, `config.json` is still there, and the next sign-in does not start it.
7c. Switch "Launch at login" off in Task Manager > Startup, open Settings: the box is unticked.
7d. Setup wizard (`CursorPie-<version>-setup.exe`): it runs without a UAC prompt and installs into
    `%LOCALAPPDATA%\Programs\CursorPie`. The Start Menu has a CursorPie shortcut, and Settings > Apps lists
    CursorPie once (not twice). With "Start CursorPie when I sign in" ticked the tray icon returns after a
    sign-out; unticked it does not. Run the wizard again over a running copy: it replaces the program and
    keeps `config.json`. Uninstall from Settings > Apps: the folder and the startup entry are gone, `config.json` stays.
    Start the program from the install folder: no second install happens and no second Apps entry appears.
7e. Welcome window: with `welcome_shown` and `config.json` deleted from `%APPDATA%\CursorPie`, the first start
    shows three pages (the gesture, the system tray, launch at login). Skip or Esc closes it, and the next
    start does not show it again. "Welcome guide..." in the tray menu shows it again. On macOS the second
    page shows the Accessibility status and changes to "Granted" by itself.
8. Non-US keyboard layout (AZERTY or QWERTZ): labels and preset shortcuts match the layout.
9. Two monitors, same scale, and 125 % / 150 % scaling on Windows: pie size and hit-testing match.
10. Idle for the sleep time, then open the pie: icons reload, no crash.
11. Windows: the pie and the margin around the Settings card are see-through (no black or white box, no
    border, no thin line along the top edge), also over a bright window, and no old-style white window
    flashes up when the pie opens.
12. Virtual desktops and Spaces (`platform/windows/desktops.rs`, `platform/macos/spaces.rs`). With three
    desktops (Spaces), start a program on desktop 3 and go back to desktop 1. An "Open app" slice for it takes you
    to desktop 3 and puts its window in front. Repeat from desktop 3 for a program on desktop 1 (the search runs to
    the left). With Settings > Behavior > Other desktops (Other Spaces) set to "Do nothing", the program opens on the
    current desktop as it did before. A program that is not running starts on the current desktop, and one with a
    window on this desktop is opened as before. Whenever the program cannot be reached you are left on the desktop
    you started on. On a desktop with no windows at all nothing is switched on Windows and the program is opened as
    before.
12a. macOS only: turn off System Settings > Desktop & Dock > Mission Control > "When switching to an application,
    switch to a Space with open windows": "Switch" still reaches the app, by pressing Control+Arrow. Turn off the
    Mission Control shortcuts too: nothing is switched, you stay where you were and the app is only activated. An
    app with all windows minimized or hidden opens as before and does not flick through the Spaces.
12c. macOS only: when "Switch" does not reach the app, run from a terminal with `CURSORPIE_DEBUG=1 cargo run
    --release` and press the slice. The `CursorPie[spaces]:` lines say whether the app and its windows were found,
    whether `open -a` and the Accessibility raise brought a window, and, for every arrow key pressed, whether the
    Space changed. Put those lines in the pull request.
12b. macOS only: "Bring the app's window to this Space" (experimental). The window of an app on another Space
    arrives on the current one and is raised, or, when macOS refuses, you are taken to the app's Space and
    `CursorPie: could not move a window of ...` appears on stderr (run `cargo run --release` from a terminal).
    Note the macOS version in the pull request.

## Risks and known limitations

- **Low-level hooks have a timeout.** Windows removes a hook whose callback is too slow (about 300 ms
  by default) without telling the app. The callback only does atomic loads and short locks, like the
  macOS tap, and never blocks. A watchdog that reinstalls the hook is a possible follow-up.
- **Elevated windows.** A normal process cannot send input to an administrator window (UIPI);
  shortcuts to such windows are ignored. Run CursorPie as administrator if you need that.
- **UWP apps share one host process** (`ApplicationFrameHost.exe`), so per-app profiles cannot tell
  them apart. UWP (Microsoft Store / packaged) apps such as Calculator, Photos or Settings are drawn by
  Windows inside frame windows owned by that one host process. The Windows backend identifies an app from
  the executable of the window's process (`platform/windows/apps.rs`), so all of them report
  `applicationframehost.exe`: one profile with that id covers all UWP apps, and "focus the app" brings the
  top-most host window to the front, which is not necessarily the one meant. Telling them apart would need
  the packaged app's identity (AUMID / package family name) instead of the executable name, which changes
  what an "app id" is in `config.json`; it is a possible follow-up, not part of 0.3.0.
- **Foreground rules.** `SetForegroundWindow` can be refused by Windows; the "focus target" option is
  best effort there.
- **Virtual desktops have no public switch.** "Other desktops" asks `IVirtualDesktopManager` (documented) which
  desktop a window is on and presses Win+Ctrl+Left / Right (what the user does) to get there. The interface can
  neither switch desktops nor move a window of another process (`MoveWindowToDesktop` refuses it), and nothing
  lists the desktops, so the search steps one at a time, right first, then left (at most ten each way) and walks
  back when it finds nothing. An undocumented interface (`IVirtualDesktopManagerInternal`) could do it directly,
  but its ids change between Windows builds, so it is not used. A "bring the window here" mode was left out of the
  Windows build for the same reason (macOS has one, see `platform/macos/spaces.rs`).
- **Mixed DPI.** With monitors at different scales the pie window may be placed slightly off when it
  moves between monitors; single-scale setups are exact.
- **Icons in a copied `config.json`.** Image icons are stored as `~/...` paths, which differ per OS. Profile
  *share files* embed the images and are portable; copying a whole `config.json` between OSes is not.
- **Trigger button / extra mouse buttons.** Only the middle button is offered as the trigger on Windows, and
  the back/forward side buttons are the only other buttons used. This is the one feature that is not
  at parity with macOS (buttons 6 to 8 are not reported by Windows).
- **Transparent window needs the `glow` renderer *and* a frameless window.** eframe 0.36 defaults to `wgpu`,
  which gives an opaque window on Windows (a black box around the pie). The Windows build therefore uses
  `glow` (OpenGL), selected in `Cargo.toml`. That alone still left a white box with a border, a white
  old-style window flashing up when the pie opened and a 1 px white line along the top edge, because winit
  keeps `WS_CAPTION` on borderless windows and asks for an "undecorated shadow". `platform::clear_window_frame`
  (`platform/windows/window.rs`) removes all of that; see `docs/developer/UI_DESIGN.md`, "Transparent window on
  Windows". OpenGL 2.0 or newer is needed, which every Windows 10/11 GPU driver provides, but a machine
  with a broken OpenGL driver or a remote desktop session without GPU acceleration may fail to create the
  window.

## Status

Verified by hand on a real Windows machine: the pie opens under the cursor and runs slices,
Settings opens and saves, the tray icon and its menu work, and the pie and the Settings card are drawn on
a transparent window with no white box, border, flash or top line (after the window-frame fix described in
`docs/developer/UI_DESIGN.md`). Behavior is almost identical to macOS.

Not at parity: extra mouse buttons (see [Risks](#risks-and-known-limitations)).

Windows is treated as stable by the maintainers. The limits that remain come from Windows itself and
are listed for users in [Known issues](../user/known-issues.md): programs running as administrator
(UIPI), Store (UWP) apps that share one host process, switching to another virtual desktop, and
monitors with different scales. The checklist above is the regression list: run the items a change
touches (the keyboard-layout, scaling and elevated-window items first), and report anything you see
with the Windows version, GPU and scaling.

Automated checks that need no Windows machine: `cargo check --features check-windows` and
`cargo test --features check-windows` (including the Windows pure helpers) pass on macOS and Linux, and CI
builds and tests the backend natively on `windows-latest`.

## Progress

| Phase | State |
|---|---|
| 0 | done |
| 1 | done: 40+ characterization tests, all green on the refactored code |
| 2 | done: `cargo test` runs on Linux through the stub; macOS glue moved |
| 3 | done: `cargo check --features check-windows` clean; Windows pure helpers tested on Linux; runs on Windows |
| 4 | done: per-OS defaults, paths, app ids, docs, CI, packaging script |
| 5 | done: Windows is supported and stable; the checklist stays the regression list |
| 6 | done (0.3.0): transparent, frameless window on Windows (`glow` + `clear_window_frame`) |
