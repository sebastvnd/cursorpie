# User guide

Everything about using CursorPie once it is installed. New here? Read [The gesture](#the-gesture) and
[Slices and actions](#slices-and-actions), and skim the rest when you need it.

## Contents

- [The gesture](#the-gesture)
- [Slices and actions](#slices-and-actions)
- [Profiles](#profiles)
- [Side buttons and app pies](#side-buttons-and-app-pies)
- [Your mouse and its extra buttons](#your-mouse-and-its-extra-buttons)
- [Trackpad (macOS, Windows and Linux X11)](#trackpad-macos-windows-and-linux-x11)
- [Settings](#settings)
- [Undo, redo and backups](#undo-redo-and-backups)
- [Sharing a profile](#sharing-a-profile)
- [Default setup](#default-setup)
- [Memory and performance](#memory-and-performance)

## The gesture

1. **Hold** the trigger (the middle mouse button by default) for 150 ms. The pie opens under the cursor.
2. **Move** toward a slice. It highlights as you hover.
3. **Release** to run it. Release in the center dead zone to cancel.

What else the trigger does:

| You do this | CursorPie does this |
|---|---|
| **Hold** for 150 ms | Opens your general pie under the cursor |
| **Double-click** (default mode) | Opens the pie of the app under the pointer, instantly, if that app has its own profile |
| **Right-click** while the pie is open | Switches which pie the side buttons follow, and the ring changes color at once |
| **Click** quickly | Passes the click through to the app. If the app has its own profile, the click is delivered after the double-click window (260 ms) |
| **Press and drag** before the pie opens | Hands the drag back to the app, so middle-drag panning still works |
| **Release** over a slice | Runs that slice |
| **Drag outward** across the rim of a submenu slice | Opens the submenu, re-centered on the cursor |
| **Keep holding** in the center | After 1200 ms in total, opens Settings. A progress arc fills around the center disc |

In *classic* mode (see [Profiles](#profiles)) there is no double-click: holding opens the app's own pie if it has
one, otherwise the general pie. Every timing can be changed in **Settings > Behavior**.

**Instant flick** is an option: drag right after pressing to open the pie with no hold delay. It disables
middle-drag pass-through in other apps. The trigger can be the middle button, or on macOS one of three extra
mouse buttons.

## Slices and actions

A slice is one of three kinds:

- **Run** does something when you release the button over it.
- **Mode** makes your mouse's back and forward buttons send the actions you assigned to this mode, for example
  Undo and Redo, or Cmd+[ and Cmd+] on macOS and Alt+Left and Alt+Right on Windows. The active mode is
  remembered per profile and shown in the center of the pie. If an action is *None*, the native back or
  forward click passes through. A mode can also give an action to **every other extra button** of your mouse, see
  [Your mouse and its extra buttons](#your-mouse-and-its-extra-buttons).
- **Submenu** opens a nested ring. A ring holds up to 8 slices and menus go up to 5 levels deep. A dot on the
  rim marks a submenu.

Actions are available for Run slices and for each side button of a Mode:

| Action | What it does |
|---|---|
| None | Nothing. For side buttons, the click passes through to the app |
| Shortcut | Sends a key combination. Click the button and press the keys to record it (Esc cancels) |
| Open app | macOS: `open -a <name or path>`, for example `Safari` or `/Applications/Safari.app`. Windows: a program name or the path of an `.exe`, for example `chrome` or `C:\Tools\App.exe`. Linux: a desktop-entry id, a program name or a path, or a role that the system fills: `@browser`, `@files` or `@terminal`. If the app already runs on another virtual desktop or Space, CursorPie goes to it |
| Open URL / file | Opens the target with the system, for example `https://example.com` or a file path |
| Shell command | macOS and Linux: `/bin/sh -c "<command>"`. Windows: `cmd /C "<command>"` without a console window |
| Sequence | Several actions in order, 40 ms apart. Edit these in `config.json` (see [Configuration](CONFIGURATION.md)) |

Actions run on one worker thread, so they always execute in the order you triggered them.

**Icons** can be text, emoji, or an image (`.png`, `.jpg`, `.jpeg`, `.webp`); a path may start with `~/`. An icon is drawn at no more than about 120 px, so a big image is wasted. When you choose an image with **Image...** in the icon picker, CursorPie prepares it: up to 512 px and 512 KB it is used as it is; a bigger one is shrunk to a 256 px PNG copy in the `icons` folder next to `config.json`, the original is never touched, and the status bar tells you what it did. An image with more than 40 megapixels or 64 MB, or a file that is not an image, is refused with a message. The **Mine** page of the picker lists the images in the `icons` folder (up to 120 files of at most 2 MB each) and says how many it left out. The pie size (140 to 360 px), accent color and sounds are adjustable. An optional soft tick plays when the hovered
slice changes.

## Profiles

A profile is a pie plus the apps it applies to. Up to 16 profiles are allowed.

- Apps are identified by bundle id on macOS (for example `com.apple.Safari`), executable name on Windows (for
  example `chrome.exe`, not case sensitive) and desktop-entry id on Linux (for example `firefox`).
- There is **exactly one general profile**: the one with **no apps**. It serves every app without a profile of
  its own, is created for you and cannot be deleted.
- Every other profile **belongs to at least one app**. **New profile** asks for a name and an app, and the last
  app of a profile cannot be removed.
- Two ways to combine them, switchable in **Behavior**:
  - *Hold = general pie, double-click = pie of the app* (default).
  - *Classic*: hold opens the app's own pie if it has one, otherwise the general pie.

## Side buttons and app pies

An app with its own profile has two pies: its own and the general one. The back and forward buttons follow
exactly one of them. Each pie shows which one with its ring color: **accent = the side buttons follow this pie,
red = they follow the other one** (the colors can be changed).

- **Default:** the app's pie. Pick the general pie instead in **Settings > Behavior > Side buttons**.
- **Choosing:** pick a Mode slice in a pie and the side buttons follow that pie from then on, in that app. Hold
  the trigger, pick *History* in the general pie, and Undo and Redo are on your side buttons even in an app with
  its own pie.
- **Switching:** with a pie open, **right-click**. The pie stays open and its ring changes color. Without
  opening a pie, use *Switch side-button pie* in the menu bar; the pie that now owns the side buttons appears
  for a moment (time adjustable, 0 turns it off).
- The choice is remembered per app while CursorPie runs. It can be forgotten when you switch to another app, and
  saving Settings clears it.

**Which app is "the app"?** By default the app whose window is **under the pointer**, so you do not have to
click a window to focus it first. Shortcuts always go to the app in front, so CursorPie brings that app to the
front right before it sends one, unless it already is. Both are options in Behavior: switch to *the app in
front* for the old behavior. If CursorPie cannot tell which window is under the pointer, it uses the app in
front.

## Trackpad (macOS, Windows and Linux X11)

On a Mac a trackpad, on Windows a **precision touchpad** and on Linux (X11) a touchpad can do what the middle button does. It is **off by default**: turn on *Open the pie with a
trackpad hold* in **Settings > Behavior > Trackpad** (and press Save). A mouse keeps working exactly as before, and
a mouse and a trackpad can be used in any order, with the same pie, hold time, double-click speed and profiles.

| You do this | With a mouse | With a trackpad |
|---|---|---|
| Open the general pie | Hold the middle button | Touch and hold still with the chosen number of fingers for the hold time |
| Choose a slice | Move toward it | Move the finger toward it |
| Run it | Release the button | Lift the finger |
| Cancel | Release in the center | Lift the finger in the center |
| Open Settings | Keep holding in the center | Keep holding in the center |
| Switch the mode | Right-click | Click (or tap) with one more finger |
| Open the app's own pie | Double-click the trigger | Tap, then touch and hold again within the double-click speed |

- One finger holds and one more finger clicks. A hold with two fingers is not supported: the system scrolls with
  two fingers, so the pointer would not move.
- **How still the finger must stay** is measured in millimeters on the trackpad. A finger that slides before the hold
  time ran out just moves the pointer, as always; scrolling, tapping and dragging are not affected.
- Palms and the thin border of the trackpad are ignored, and a hold does not start within the **typing guard** time
  after a key press.
- **Extra click** can switch the pie like the right-click (default), make the hovered Mode slice the active mode, or do
  nothing. The click never reaches the app under the pie.
- **Haptic feedback** (macOS only): a tick when the pie opens, a soft one when the slice changes and a distinct one
  when the mode switches. Only a trackpad ticks; the system's *Force Click and haptic feedback* setting stays in charge.

On a Mac the fingers are read through an undocumented macOS interface, loaded only while the feature is on. If macOS
changes it, Settings says *Trackpad support is not available on this macOS* and the mouse is unaffected.

On Windows the touchpad's reports are read through Raw Input, a public interface, and nothing is registered while the
feature is off. Only a **precision touchpad** works: an older touchpad that Windows treats as a plain mouse sends no
finger positions, and Settings says *No precision touchpad found*. If a tap followed by a hold clicks the app under the
pie, turn off *Tap twice and drag to multi-select* in Settings > Bluetooth & devices > Touchpad.

### On Linux (X11)

- Everything above works the same way, except **haptic feedback**: a Linux touchpad has no standard way to tick, so
  those switches are not shown.
- The fingers are read straight from `/dev/input/event*` (libinput does not pass touch positions on to X11), and
  only while *Open the pie with a trackpad hold* is on. Your user must be in the **`input` group**:
  `sudo usermod -aG input $USER`, then log out and in. Without it Settings says so under the switch; the mouse is
  unaffected. Only touchpads are opened (never a touchscreen, a graphics tablet or a program's virtual device), and
  nothing is recorded: positions are used to decide and then dropped.
- While a hold is running, the left and right buttons are grabbed so the click of the extra finger never reaches the
  app; a click that is not the extra one is sent again when it is released, a moment late.
- *Ignore holds after typing* hears that a key was pressed (not which key) while the switch is on.
- Nothing happens? Run `cursorpie --touchpad-debug` in a terminal: it says which step fails (switch not saved, no
  `input` group, device not recognized, the hold not still enough) and shows your fingers live.
- Wayland is not covered: the evdev reading would work there, but CursorPie's mouse layer is X11 only for now.

## Your mouse and its extra buttons

A Mode slice lists one action for each extra button your mouse has: **Back button**, **Forward button**, then
**Extra button 6**, **Extra button 7** and so on. Which buttons are listed depends on the mouse CursorPie thinks
you use. A button with the action *None* keeps working in the app as before.

CursorPie keeps a profile of every mouse it has met (the file `mice.json` next to `config.json`, saved at once,
not with Save). Every few seconds it looks at the connected mice and tells a **new** mouse from one it **knows**
(a note appears in Settings). Two mice of the same model share one profile, because their buttons are the same.

**Settings > General > Mouse:**

| Button | What it does |
|---|---|
| Scan this mouse | Press every extra button of the mouse once, then **Done**. While the scan runs the buttons do nothing else. What you pressed is stored in the profile and wins over what the system reported. Cancel, or 45 seconds without a press, ends the scan |
| Detect again | Asks the system which mice are connected now |
| Forget | Removes a mouse from the list. It is met as a new mouse the next time it is connected |

What the system can tell about your mouse differs:

- **Linux:** the kernel's list of input devices gives the name, the hardware id and the extra buttons. Scan if a
  button is missing or one is listed that does not exist.
- **Windows:** only the number of buttons, and only two extra buttons (back and forward) are seen by any program
  that watches the mouse. A mouse with a different number of buttons counts as another mouse.
- **macOS:** the system does not list mice for CursorPie, so scan the mouse once; the result is kept.

The trigger button is not offered as an extra button of a mode. If your mouse does nothing when scanned, its
extra buttons may be handled by the maker's own software.

## Settings

Open it from the menu bar or tray icon, or by holding the trigger in the center of the pie. The top right
corner has two small round buttons, like the dots of a window: the green one (the accent color) **saves** and
closes, applying and storing your changes; the red one **cancels**. **Cmd+S** (Ctrl+S elsewhere) saves and
keeps Settings open. **Cancel**, **Esc** and **Cmd+W** (Ctrl+W elsewhere) close Settings without saving. While
something is not saved, a yellow dot shows between the two buttons, and Cancel, Esc and Cmd+W first ask
"Discard changes?": **Enter** discards the changes, **Esc** or a click outside keeps you editing. There are
no Undo and Redo buttons; use their keys (see [Undo, redo and backups](#undo-redo-and-backups)).

**Quitting.** **Quit CursorPie** (in the sidebar, in **General**, or in the tray / menu bar icon while Settings
is open) always asks first:

- With nothing unsaved: "Do you really want to quit?" with **Cancel** and **Quit CursorPie**.
- With unsaved changes: the window says so and offers **Keep editing**, **Save and quit** (applies and stores
  your changes, then quits) and **Quit without saving**. Changes you undid back to what Settings opened with
  count as no changes.

**Esc**, the **x** button and a click outside the window mean "not yet" and return to Settings. With Settings
closed, Quit in the tray or menu bar icon quits at once.

**The pie is off while Settings is open.** If you press the trigger then, Settings comes to the front (even
from behind other windows or minimized) and its footer says to Save or Cancel to use the pie again.

**Menu** is where you build pies.

- Choose, create, rename and delete profiles, list the apps each one applies to (type an id or use **Pick
  app...**), and **Share** a profile. The **...** button unfolds Rename, Export, Import and Delete as a row of
  icons (hover for their names) and turns into an **x** that folds them again; **+** is hidden meanwhile.
- An interactive pie shows the ring you are editing. Click a slice to edit it, a selected
  submenu slice again to step into it, and the center to go back. Two round buttons under the pie add a slice
  (**+**) and delete the selected one (the trash); you can also drag a slice onto the trash.
- Per slice: name, icon, kind, action (for **App**, **Pick...** opens the list of installed apps), and Move.
- Ready-made slices on the **+** button: *side-button modes* (History, Brackets, Clipboard, Browser, Tabs, Zoom,
  Scroll, Edit), *quick actions* and *custom* ones (Shortcut, Open app, Open URL, Command, Submenu). The quick
  actions depend on the system: Screenshot, Mission Ctrl, Spotlight, Emoji and Lock on macOS; Screenshot, Task
  view, Search, Emoji and Lock on Windows; Screenshot, Files, Terminal, Emoji and Lock on Linux.

**Behavior** holds the knobs.

- Trigger button, hold times for the pie and for Settings, pie size, accent color.
- Look: **Appearance** (System, Light or Dark, for Settings and the pie) and **Pie material** (*Solid* or *Glass*;
  Glass has sliders for Clearness, Frost and Shine).
- Whether holding opens the general pie or the app's own, and the double-click speed.
- App detection: under the pointer or in front, and whether to bring it forward before sending shortcuts.
- *Other desktops / Other Spaces*: what an Open app slice does when the app's window is elsewhere. Switch to it
  (default), bring the window to the current Space (macOS, experimental) or just open the app here. See
  [OTHER_DESKTOPS.md](OTHER_DESKTOPS.md).
- Side-button options: default pie, pick-to-follow, forget on app switch, right-click switching, ring colors,
  preview time.
- Instant flick, hover sound, names under the icons, app icon in the pie center (with its look: glyph or color,
  and its shadow), launch at login, and **Memory** (free caches after an
  idle time, on by default, 60 s).

**General** shows the permission status (macOS) with shortcuts to System Settings, the config file location,
**Reveal in Finder** (Explorer, or the file manager on Linux), **Export...**, **Import...** (a file over
4 MiB is refused), **Reset to defaults** and **Backups**. On Linux it also has Pause and Quit.

**About** shows the version, author, privacy note and credits. Website, GitHub, Report an issue and Downloads
are the four round icon buttons in the corner of the top card.

## Undo, redo and backups

Deleting a slice or a profile by accident is easy to fix.

**While Settings is open**, every change is recorded: adding, deleting or moving slices and profiles, editing
actions and apps, changing options, importing a file and **Reset to defaults**.

- **Undo** and **Redo** are on the keyboard: **Cmd+Z** and **Cmd+Shift+Z** (**Cmd+Y** also redoes; Ctrl
  instead of Cmd on Windows and Linux).
- Typing and dragging a slider count as **one** step: a step ends when you pause for a moment.
- Up to 50 steps are kept. A new change after an undo discards the steps you undid.
- In a text field, **Cmd+Z** undoes the text first. While you record a shortcut, the keys go to the recorder.
- The history lives only as long as the Settings window. Closing it, with Save or Cancel, forgets it. **Cmd+S**
  keeps it, so you can still undo after saving.

**After you have saved**, use backups. Every **Save** first stores the version it replaces as
`config.backup.1.json` (the newest) next to `config.json` and keeps the last five. Open **Settings > General >
Backups** and pick one: it is loaded into the editor, and nothing changes until you press **Save**. Undo still
brings back what you had before restoring.

A Save that would store exactly what is already in `config.json` makes no backup. The file is written through a
temporary one, so a crash or a full disk cannot leave half a file. Where the files live is in
[CONFIGURATION.md](CONFIGURATION.md#where-the-file-lives).

## Sharing a profile

**Menu > Share** moves one profile between computers, with its icons, so it looks the same on arrival.

- **Export this profile...** writes `<name>.cursorpie-profile.json` to your Downloads folder and reveals it.
  Every image icon the profile uses, including those in submenus, is embedded, up to 2 MB each. Icons of apps,
  text and emoji need nothing embedded. Missing or oversized images show up as `?` for the receiver.
- **Import a profile...** adds the file as a **new** profile (nothing is overwritten) and selects it. The
  embedded images are copied into the `icons` folder next to `config.json`: an identical file is reused, a
  different one with the same name gets a number. Press **Save** to apply it, or **Undo** (Cmd+Z) to take it back.
- A profile must list at least one app. A name already taken gets ` (2)`, ` (3)`...
- Only PNG, JPEG and WebP data is written, whatever the file claims, and nothing outside the icons folder.

**Only import profiles you trust.** Actions can run shell commands. CursorPie warns you when an imported
profile contains any, and nothing runs until you press Save and then use the slice.

## Default setup

A fresh install comes with three profiles:

| Profile | Apps | Slices |
|---|---|---|
| Default (general) | all apps without a profile | History (undo/redo), Clipboard (copy/paste), Brackets, and a *Launch* submenu with a file manager, terminal, browser and screenshot tool |
| Browsers | The common browsers of your system (on macOS Safari, Chrome, Firefox, Arc, Brave and Edge) | Browser (back/forward), Tabs (previous/next), Zoom, Scroll (Page Up/Down) |
| Code | The code editors of your system (on macOS VS Code and Xcode; on Windows VS Code and Visual Studio; on Linux VS Code and its variants) | History, Brackets, Edit (Return/Delete) |

The Launch submenu and the app ids adapt to your system (for example Finder on macOS, Explorer on Windows and the
file manager, terminal and browser roles on Linux).

## Memory and performance

CursorPie sits in the menu bar all day, so it is built to cost almost nothing while you are not using it.

- **No drawing while idle.** The window is hidden and nothing repaints until you press the trigger.
- **Icons are cached** (at most 96) and shared by the pie and Settings, and loaded during the hold time so the
  pie appears without a hitch. Images are scaled down to at most 128 px when they are loaded, and an image
  over 4096 px on a side is not loaded at all.
- **Settings draws only what you see.** The icon picker builds and loads just the rows on screen, a few images
  per frame, so a big `icons` folder neither freezes Settings nor fills memory. The list of ready-made slices
  exists only while the **Add a slice** panel is open.
- **Sleep.** After 60 seconds without a pie or Settings (**Settings > Behavior > Memory**), CursorPie drops the
  cached icons and the Settings working copy and asks the system to take the freed pages back. The next press
  loads what the pie needs again. Switch the option off to keep everything in memory.

What sleep cannot free is the text and graphics buffers the UI library keeps for the process lifetime: a fixed
base cost that does not grow with use.

If edges look soft on your screen, open an issue and mention your display scaling. How the interface is drawn is
explained in [UI_DESIGN.md](UI_DESIGN.md).
