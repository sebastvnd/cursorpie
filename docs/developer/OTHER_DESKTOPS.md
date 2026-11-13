# Open app on another desktop or Space: how it works

An *Open app* slice for a program that is already running used to do one thing: start it with the
system's own call (`open -a` on macOS), which leaves you where you were when the window is on another
virtual desktop (Windows) or Space (macOS). **Settings > Behavior > Other desktops / Other Spaces** now
chooses what happens (`desktop_mode` in `config.json`, the type is `DesktopMode`):

| Mode | macOS | Windows | Linux (X11) |
|---|---|---|---|
| `SwitchToApp` (default) | go to the Space of the app and raise its window | go to the desktop of the app and raise its window | ask the window manager to activate the app's window, which usually goes to its workspace |
| `BringToCurrent` | move the window to this Space (experimental, private calls), else as `SwitchToApp` | not possible, acts as `SwitchToApp` | not possible, acts as `SwitchToApp` |
| `DoNothing` | `open -a` as before | start or raise as before | start or raise as before |

On Linux the window manager does the switching: *Open app* asks it to activate the app's window with
`_NET_ACTIVE_WINDOW` (`Conn::raise`). The app is started only when it has no window. None of the search code
described below is used there, and whether a given window manager follows the request has not been checked on
real desktops (see `docs/developer/LINUX_MIGRATION.md`). The rest of this document is about macOS and Windows.

Nothing changes when the app does not run, is hidden, has no window, or has one on the desktop that is
shown. `DesktopMode::effective` turns a mode the OS lacks into `SwitchToApp`.

## Where the code is

| Part | File | Tested on every OS |
|---|---|---|
| The search for the desktop: `walk_to_target`, `steps_between`, `presence_of`, `signature`, path matching | `src/platform/desktops.rs` | yes, with a fake row of desktops |
| macOS: window list, Spaces, keys, Accessibility, SkyLight | `src/platform/macos/spaces.rs` | no, it needs a Mac |
| Windows: `IVirtualDesktopManager`, window enumeration | `src/platform/windows/desktops.rs` | the logic yes, the COM calls on Windows only |
| The setting | `src/config/model.rs`, `src/ui/settings/tabs/behavior.rs` | yes |

The decision logic is kept free of OS calls on purpose: it is written against the `Desktops` trait
(`target_here`, `current`, `step`, `route`), so the same search runs on macOS and Windows and its tests
run in CI on Linux.

## How the macOS flow goes

1. **Is it worth it?** `visible_running_pid` finds the running app by bundle id, name or `.app` path
   (trailing slash and letter case ignored). Not running or hidden: plain `open -a`.
2. **Where are its windows?** `CGWindowListCopyWindowInfo` with all Spaces lists every window and says
   which are on screen. `presence_of` gives `Here`, `Elsewhere(window)` or `Nowhere`. Only *ordinary*
   windows count (layer 0, visible, not tiny), so menu bar items, helpers and a pie that fades out do not
   make an app look present. Anything but `Elsewhere` is plain `open -a`.
3. **Cheapest way first.** `open -a`, a Dock click: with the system setting *switch to a Space with open
   windows for the application* macOS goes there itself. If a window of the app is on screen within 1.2 s,
   that is the end.
4. **Go there like the user would.** Control+Left / Control+Right, one Space at a time
   (`walk_to_target`). The order of the Spaces and the Spaces of the app's windows come from the window
   server (`SLSCopyManagedDisplaySpaces`, `SLSCopySpacesForWindows`), so the way is known: the nearest
   Space with a window of the app, to the left or right, however many Spaces away. The same keys are tried
   through System Events if the posted ones are ignored.
5. **Raise the window.** After the switch animation (700 ms) `focus_app` sets `AXFrontmost`, raises a window
   with `AXRaise` (main, focused, else the first of `AXWindows`), activates the app and runs `open -a`,
   then checks that the frontmost ordinary window is the app's, up to three times.

`CURSORPIE_DEBUG=1` prints what every step saw (`CursorPie[spaces]: ...`) to stderr.

## Problems met, and what was done about them

| Problem | Cause | Solution |
|---|---|---|
| The app's window cannot be found or raised by Accessibility | Accessibility only lists the windows of the Space that is shown; `AXMainWindow` of an app with windows elsewhere gives error -25212 | Use the window list of the window server to *find* the window, and Accessibility only *after* arriving |
| No public call switches to a Space | Apple has none | Press the Mission Control shortcuts, which work on macOS 27 when they are on |
| Moving a window to this Space | Only private calls can, and macOS 27 accepts them without doing anything | Ask where the window is afterwards (`SLSCopySpacesForWindows`) and fall back to switching |
| `/Applications/Helium.app/` was not found | The system reports the path without the trailing slash | `same_app_path` and `app_name_of_path` compare ignoring slash and case |
| Only the Space changed, another app's window was in front | `open -a` was run 50 ms into the animation and does not raise a particular window | Wait for the animation, raise the window itself, check, retry (`focus_app`) |
| A window on the left was found only after a step into the right end | The search always went right first | The order of the Spaces is read, and the nearest side is taken (`steps_between`) |
| Nothing happened from an empty desktop | The Space was told by the windows on it, and an empty one has none | The id of the shown Space is part of the answer (`SLSGetActiveSpace`) |
| It stopped at a Space in between (Helium before Terminal) | During the animation the windows of two Spaces are on screen, so "the app is here" came true early | A known route is taken to the end and checked only then; a step counts when the Space id changes, followed by 350 ms for the animation |

## Rules that follow from this

* A step that cannot be told from a move is never trusted alone: the end of the row does nothing, so the
  walk back stops when the starting Space shows again, and a search without a known route is bounded
  (`MAX_STEPS`, ten each way).
* On failure the user is left where they started, and the cheapest result is always `open -a`.
* Private calls are looked up with `dlsym`, never linked, are used only to *read* the order of the Spaces and
  to *try* the move, and every answer is checked. A macOS without them uses the blind walk.
* Every new rule of the search gets a test against the fake row (`desktops.rs`); the macOS code is checked
  by hand with the list below.

## Known limits

* The arrow keys need the shortcuts *Move left / right a space* to be on.
* With *Displays have separate Spaces*, the Spaces of the display that holds the app are used; the arrows
  move the display under the pointer.
* Full screen apps are Spaces of their own and are crossed like any other.
* Windows has no public call to switch to, list or move windows of other programs between desktops, so the
  switch there is Win+Ctrl+Left / Right with the same search; see `docs/developer/WINDOWS_MIGRATION.md`.

## Manual check on a Mac (macOS 26 and 27)

Spaces: one with Terminal, one with a browser, one empty, in different orders.

1. From each Space, open the app of every other Space with an Open app slice: left, right, two or more Spaces
   away, and across the Space of a third app. You must end on the app's Space with its window in front.
2. Start from the empty Space.
3. Mode *Do nothing*: the app opens on the current Space as before.
4. Mode *Bring the window*: either the window arrives, or you are switched and `could not move a window` is
   printed.
5. With the system shortcuts off nothing moves and `could not reach the Space` is printed.
