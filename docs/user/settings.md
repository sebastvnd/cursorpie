# Settings and backups

Where you build menus, tune the gestures and keep your configuration safe.

## Contents

- [Opening, saving and quitting](#opening-saving-and-quitting)
- [Menu tab](#menu-tab)
- [Behavior tab](#behavior-tab)
- [General tab](#general-tab)
- [About tab and updates](#about-tab-and-updates)
- [Undo, redo and backups](#undo-redo-and-backups)
- [Sharing a profile](#sharing-a-profile)
- [Memory and performance](#memory-and-performance)

## Opening, saving and quitting

Open Settings from the menu bar or tray icon (macOS and Windows), or by holding the trigger in the center of the pie
(the only way on Linux, which has no tray icon yet).

| Control | What it does |
|---|---|
| Green round button (top right) | Saves and closes, applying and storing your changes |
| Red round button, **Esc**, **Cmd+W** / **Ctrl+W** | Closes without saving. With unsaved changes it first asks "Discard changes?": **Enter** discards, **Esc** or a click outside keeps you editing |
| **Cmd+S** / **Ctrl+S** | Saves and keeps Settings open |
| Yellow dot between the two buttons | Something is not saved yet |

There are no Undo and Redo buttons; use their keys (see [Undo, redo and backups](#undo-redo-and-backups)).

**The pie is off while Settings is open.** If you press the trigger then, Settings comes to the front (even from behind
other windows or minimized) and its footer says to Save or Cancel to use the pie again.

### Quitting

**Quit CursorPie** (in the sidebar, in **General**, or in the tray / menu bar icon while Settings is open) always asks first:

- With nothing unsaved: "Do you really want to quit?" with **Cancel** and **Quit CursorPie**.
- With unsaved changes: **Keep editing**, **Save and quit** or **Quit without saving**. Changes you undid back to what
  Settings opened with count as no changes.

**Esc**, the **x** button and a click outside the window mean "not yet" and return to Settings. With Settings closed,
Quit in the tray or menu bar icon quits at once. On Linux, Pause and Quit are at the bottom of **General**.

## Menu tab

Where you build pies.

- Choose, create, rename and delete profiles, list the apps each one applies to (type an id or use **Pick app...**),
  and **Share** a profile. The **...** button unfolds Rename, Export, Import and Delete as a row of icons (hover for
  their names) and turns into an **x** that folds them again; **+** is hidden meanwhile.
- An interactive pie shows the ring you are editing. Click a slice to edit it, click a selected submenu slice again to
  step into it, and click the center to go back. Two round buttons under the pie add a slice (**+**) and delete the
  selected one (the trash); you can also drag a slice onto the trash.
- Per slice: name, icon, kind, action (for **App**, **Pick...** opens the list of installed apps) and Move.
- Ready-made slices on the **+** button: *side-button modes* (History, Brackets, Clipboard, Browser, Tabs, Zoom, Scroll,
  Edit), *quick actions* and *custom* ones (Shortcut, Open app, Open URL, Command, Submenu).

The quick actions depend on the system:

| System | Quick actions |
|---|---|
| macOS | Screenshot, Mission Ctrl, Spotlight, Emoji, Lock |
| Windows | Screenshot, Task view, Search, Emoji, Lock |
| Linux | Screenshot, Files, Terminal, Emoji, Lock |

## Behavior tab

The knobs, grouped:

- **Trigger and timing:** trigger button, hold times for the pie and for Settings, pie size, accent color.
- **Look:** **Appearance** (System, Light or Dark, for Settings and the pie) and **Pie material** (*Solid* or *Glass*;
  Glass has sliders for Clearness, Frost and Shine).
- **Profiles:** whether holding opens the general pie or the app's own, and the double-click speed.
- **App detection:** the app under the pointer or in front, and whether to bring it forward before sending shortcuts.
- **Other desktops / Other Spaces:** what an Open app slice does when the app's window is elsewhere. Switch to it
  (default), bring the window to the current Space (macOS only, experimental) or just open the app here.
  How it works: [Other desktops](../developer/OTHER_DESKTOPS.md).
- **Side buttons:** default pie, pick-to-follow, forget on app switch, right-click switching, ring colors, preview time.
- **Other:** instant flick, hover sound, names under the icons, app icon in the pie center (glyph or color, and its
  shadow), launch at login, **Trackpad** (see [Trackpad hold](trackpad.md)), **Browser extension** and **Memory** (free
  caches after an idle time, on by default, 60 s).

## General tab

- The permission status (macOS), with shortcuts to System Settings. In a Linux Wayland session it shows a capability
  report instead: what works and the exact fix for what does not (see [Linux](linux.md)).
- The location of `config.json` and **Reveal in Finder** (Explorer, or the file manager on Linux).
- **Export...**, **Import...** (a file over 4 MiB is refused), **Reset to defaults** and **Backups**.
- **Mouse:** scan and forget mice (see [Profiles and mouse buttons](profiles-and-buttons.md#your-mouse-and-its-extra-buttons)).
- On Linux also **Pause** and **Quit**.

## About tab and updates

**About** shows the version, an **Updates** panel, the author, the privacy note and credits. Website, GitHub, Report an
issue and Downloads are the four round icon buttons in the corner of the top card.

Updates work the same on macOS, Windows and Linux:

1. When Settings opens, CursorPie looks at most once a day for a newer release and, if there is one, offers it in a sheet.
2. **Install update** downloads the package for your system from the release on GitHub, checks it against the release's
   checksum, replaces the installed copy and starts the new version. **Release page** opens the page in your browser;
   **Not now** closes the sheet.
3. The Updates panel always shows what the last look found and has **Check now**, **Install update** (or **Restart to
   update** once the download is ready) and the switch **Check for updates automatically** (on by default; save with the
   rest of Settings).
4. Unsaved changes in Settings are never thrown away: save or cancel them first.
5. A copy that was not installed in the usual place (run from a build folder, an unzipped Windows download or a folder you
   cannot write to) cannot update itself and gets **Download page** instead.

Everything needs `curl` or `wget`. See [Updates](updates.md) for what each system does.

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
[Configuration](configuration.md#where-the-file-lives).

## Sharing a profile

**Menu > Share** moves one profile between computers, with its icons, so it looks the same on arrival.

- **Export this profile...** writes `<name>.cursorpie-profile.json` to your Downloads folder and reveals it.
  Every image icon the profile uses, including those in submenus, is embedded, up to 2 MB each. Icons of apps,
  text and emoji need nothing embedded. Missing or oversized images show up as `?` for the receiver.
- **Import a profile...** adds the file as a **new** profile (nothing is overwritten) and selects it. The
  embedded images are copied into the `icons` folder next to `config.json`: an identical file is reused, a
  different one with the same name gets a number. Press **Save** to apply it, or **Undo** (Cmd+Z) to take it back.
- The flows the profile's slices use are embedded too. On import they are added as new flows (an identical one
  you already have is reused) and the slices are pointed at them.
- A profile must list at least one app. A name already taken gets ` (2)`, ` (3)`...
- Only PNG, JPEG and WebP data is written, whatever the file claims, and nothing outside the icons folder.

**Only import profiles you trust.** Actions can run shell commands. CursorPie warns you when an imported
profile contains any, and nothing runs until you press Save and then use the slice.

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
  Settings working copy and the cached icons it does not need first, and asks the system to take the freed pages
  back. The icons of your general pie stay, so the first press after a long idle time opens as fast as any other.
  Switch the option off to keep everything in memory.
- **Always listening.** CursorPie asks the system not to slow it down while it is hidden (App Nap on macOS,
  power throttling on Windows). On Windows it also notices when the system has silently dropped its mouse
  hooks (after waking from sleep or unlocking the session, for example) and installs them again within a few
  seconds. On macOS it turns the event tap back on if the system switched it off, and restarts the trackpad
  source after the Mac has slept.

What sleep cannot free is the text and graphics buffers the UI library keeps for the process lifetime: a fixed
base cost that does not grow with use.

If edges look soft on your screen, open an issue and mention your display scaling. How the interface is drawn is
explained in [UI_DESIGN.md](../developer/UI_DESIGN.md).
