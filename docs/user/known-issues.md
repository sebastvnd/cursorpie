# Known issues and limitations

What does not work yet, or works differently, grouped by system. Linux is split by session type: **X11 is pre-stable** and
**Wayland is experimental** (see [CursorPie on Linux](linux.md)). If you hit something that is not listed, please
[open an issue](https://github.com/sebastvnd/cursorpie/issues) with your system version, how you started
CursorPie, the trigger button and the output of `cargo run --release` from a terminal.

## All systems

- Shortcut labels and the shifted-symbol display assume a US/ANSI layout. Key codes themselves are physical keys.
- The back and forward side buttons are fixed: they are the mouse's back and forward buttons (buttons 3 and 4 in
  [Configuration](configuration.md#extra-mouse-buttons), X buttons 8 and 9 on Linux). Only the trigger button is
  configurable.
- With a per-app profile and the default gesture, a plain middle-click in that app is delivered after the
  double-click window (260 ms by default). Use classic mode, or a shorter window, if that bothers you.
- The right-click that switches the side-button pie is swallowed only while a pie is open. Otherwise right-clicks
  pass through untouched.
- "App under the pointer" looks at normal app windows (not the Dock, menu bar or desktop). Bringing it to the
  front activates the whole app, not one particular window.
- Sequences cannot be created or edited in Settings yet, only in `config.json`. Settings shows how many steps a
  sequence has and keeps it.
- **Image icons have size limits.** An image over 4096 px on a side, or one that cannot be decoded, is not drawn: the slice shows `?`. The **Mine** page of the icon picker lists at most 120 images of 2 MB each and says how many it left out. Choosing a big image with **Image...** avoids all of this: CursorPie saves a smaller copy in the `icons` folder and tells you (see [Using the pie](using-the-pie.md#icons-and-look)).
- **Import... refuses a config file over 4 MiB**, and Menu > Share refuses a profile file over 24 MiB.
- Undo covers one Settings session. After you save and close, use **Settings > General > Backups** (the last
  five saves). Changes Settings does not make, such as the active side-button mode saved when you pick a mode in
  the pie, are not backed up on their own.
- If the app icon does not show in the center of an app's own pie, start CursorPie from a terminal: a line
  starting with `CursorPie: app icon for` says why it could not be read. The icon only appears in the pie of an
  app that has its own profile.

## macOS

- CursorPie cannot list the mice on a Mac: scan the mouse in **Settings > General > Mouse** to tell it which extra
  buttons it has, and it will not notice that you changed mouse.
- Ad-hoc signed builds may need the Accessibility permission again after each rebuild. Sign with a stable
  identity (`SIGN_ID`) to avoid that.
- **Trackpad** (off by default): it uses Apple's private `MultitouchSupport` framework, which Apple may change in any
  release (the feature turns itself off and says so), so it cannot be in a Mac App Store build. The surface of a
  Magic Mouse is not used. Haptic ticks need a Force Touch trackpad and the system's haptic setting on. **Not yet
  verified on a Mac:** see the manual checklist in [the trackpad plan](../developer/TRACKPAD_HAPTICS_PLAN.md#manual-verification-checklist).
- **Launch at login** points at the executable that was running when you enabled it. Toggle it off and on again
  if you move the app.
- **Other Spaces** tries three ways, each only when the previous one did not bring a window of the app onto the
  screen: `open -a` (works when *When switching to an application, switch to a Space with open windows for the
  application* is on), then Control+Right or Control+Left pressed directly, then the same through System Events
  (which asks for the Automation permission once). The arrow keys need the Mission Control shortcuts *Move left /
  right a space* to be on. If nothing works you stay where you were and `CursorPie: could not reach the Space of
  <app>` is printed. Start with `CURSORPIE_DEBUG=1 cargo run --release` to see what each way found. How it works:
  [Other desktops](../developer/OTHER_DESKTOPS.md).
- **Two screens:** "Open app" shows the app on the screen you clicked: its front window there is raised, or,
  when it has none there, its front window is moved to that screen through Accessibility. A full-screen window
  cannot be moved that way: the app is opened as before and stays on its own screen. Needs the Accessibility
  permission.
- **Bring the window to this Space** is experimental. macOS has no public call that moves another app's window,
  so CursorPie uses private SkyLight calls (the ones Hammerspoon and yabai use), checks that the window really
  arrived and switches to the app's Space when it did not. Apple may change or block those calls in any release;
  Hammerspoon reports they silently do nothing on macOS 27, so expect the fallback on some systems.

## Windows

- **Antivirus stops CursorPie or it does not start at login.** CursorPie watches the mouse and keyboard with
  low-level hooks and sends shortcuts, which is also what a keylogger does, so some antivirus programs
  are wary of an unsigned `cursorpie.exe`. What the program does about it: the `.exe` carries version information
  and a manifest, it only writes to the registry when it installs or updates (not at every start), and a start from
  the login entry (`--autostart`) waits for the taskbar before it creates its tray icon and hooks. What it cannot do
  by itself: a code signing certificate (`SIGN_CERT_THUMBPRINT` in `scripts/package-windows.ps1`) is the one fix
  that lasts. Until then, add an exclusion for `%LOCALAPPDATA%\Programs\CursorPie` (Windows Security > Virus & threat
  protection > Manage settings > Exclusions) or report the file as a false positive at
  <https://www.microsoft.com/wdsi/filesubmission> (other products have a similar form).
  Without any outside party: the first start no longer installs itself silently (it asks once), and
  `scripts\sign-local.ps1` signs your own build with a certificate you create and trusts it on your PC. That
  helps only the PCs that trust the certificate; it does not help other people's PCs. Building from source
  (`cargo build --release`) on the PC that will run it also avoids most "new file" detections.

- **Touchpad** (off by default): only a Windows *precision* touchpad can open the pie; an older touchpad that Windows
  treats as a plain mouse cannot be read. It is read through Raw Input, so no driver and no permission is needed. A tap followed by a hold may start a Windows tap-and-drag and click the app under the pie unless *Tap
  twice and drag to multi-select* is off. No haptic ticks. **Not yet verified on Windows:** see the manual checklist
  in [the trackpad plan](../developer/TRACKPAD_HAPTICS_PLAN.md#windows-precision-touchpad).
- Only the middle button can be the trigger. Windows reports only the middle, back and forward buttons, so a
  mode can use only those two side buttons, and CursorPie cannot tell two mice apart, only mice with a different
  number of buttons.
- Programs running **as administrator** ignore the input CursorPie sends unless CursorPie also runs as
  administrator (a Windows rule, UIPI). This is the most common reason for "CursorPie does nothing in this
  window" on Windows; see [Install](install.md#windows) for how to run it as administrator and what that costs.
- **UWP apps cannot be told apart.** Store apps such as Calculator, Photos or Settings run inside one shared host
  process, so every one of them reports `applicationframehost.exe`. A profile with that id applies to all of them
  at once. Normal desktop programs (Chrome, VS Code, Explorer, Office...) are detected correctly.
- **Other desktops** presses Win+Ctrl+Right or Win+Ctrl+Left, one desktop at a time, until a window of the app
  is on the desktop that is shown (at most ten each way), then brings it to the front. Windows has no public call
  that switches to a given desktop, so the desktops flick past on the way. If the app cannot be found, you stay
  where you are and the app is opened as before. There is no "bring the window here" choice on Windows.
- With monitors at different scales the pie may be placed slightly off when it moves between them, and Windows
  may refuse to bring a window to the front.
- **Launch at login** always points at the installed copy, so moving or deleting the download does not matter.
  If you switched the entry off in Task Manager, ticking **Launch at login** switches it back on.

## Linux

Support levels: **X11 is pre-stable**, **Wayland is experimental** (what that means: [CursorPie on Linux](linux.md#support-level)).
A surprise here may be a bug rather than a limit, so please report it with your distribution, desktop, session type
and scaling.

### Both X11 and Wayland

- **No tray icon yet.** Pause and Quit are in **Settings > General**, and Settings opens by holding the trigger in the
  middle of the pie.
- **App icons** in the pie come from the `Icon=` of the app's desktop entry. A PNG is read directly; an icon that exists
  only as SVG (many GNOME apps) is drawn by `rsvg-convert`, which has to be installed (`librsvg2-tools` on Fedora).
  Without it such an app shows the icon of the mode.
- No keyboard-layout labels on non-US layouts.
- Only the middle button can be the trigger.
- Whether your window manager follows the request to switch workspace for **Open app** has not been checked on real desktops.
- Install `zenity` or `kdialog` to use the file chooser buttons.
- **Trackpad hold** (off by default): the touchpad is read from `/dev/input`, so your user must be in the `input` group
  (Settings says so when not); on Wayland the udev rule covers it. No haptic ticks. Touchpads that report no finger count
  or no size (some virtual or very old ones) are skipped. **Not yet verified on real hardware:** the parser, the
  recognizer and the grab logic are tested; the device thread and the permissions are not.
- Other Unixes build a do-nothing stub that exists only so `cargo test` runs.

### X11 only (pre-stable)

- **Extra buttons of a mode** (button 5 and up) use the buttons the X server numbers 10 to 20. A mouse that maps its thumb
  buttons differently, or a button map changed with `xinput set-button-map`, may need a scan, and the scan hears the
  physical button, not the remapped one. Mice listed twice by a receiver count as one.
- Handing a **drag** back to the app while the middle button is held (for example middle-drag scrolling) may lose the
  start of the drag. A quick click is passed on reliably.
- The click of the extra finger of a trackpad hold is swallowed by grabbing the left and right buttons while the hold runs;
  a click that is not the extra one reaches the app a moment late, like a side-button click.
- The checklist of [Linux support](../developer/LINUX_MIGRATION.md#manual-verification-checklist) is still being completed
  on real desktops.

### Wayland only (experimental)

Written and tested with recorded input, **not yet run on a real Plasma 6.8, GNOME, Hyprland or Sway session.**

- Without the udev rule `70-cursorpie.rules` nothing is captured (see [CursorPie on Linux](linux.md#wayland-one-extra-step)).
- The pie is still an XWayland window, so it may open off-centre at a fractional scale, and the app below the pie may see
  hover events while a mouse hold is open.
- **Per-app profiles:** KDE Plasma, Hyprland and GNOME (with the extension) are written but unverified. On Sway "app under
  the pointer" is the app in front, and the pie may open away from the cursor because Sway cannot say where the pointer is.
  COSMIC, niri and other compositors have no adapter: capture and shortcuts work, per-app profiles do not.
- The screen lock gives the mouse back within half a second.
- After a trackpad pie the first touch may move the pointer once. While a hold runs, CursorPie takes the touchpad's node
  away from the desktop (the udev rule covers the permission).
- No `.deb` or `.rpm`; `install.sh` only prints the udev command unless run as root.

What exists and what does not is listed in [the Wayland implementation status](../developer/WAYLAND_SUPPORT.md#implementation-status).
