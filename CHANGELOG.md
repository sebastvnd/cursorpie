# Changelog

## Unreleased

- **Docs: Linux is described as X11 and Wayland everywhere.** The README, install guide, user guide, configuration
  reference, known issues, contributing guide, trackpad plan, the website and the package description still said that
  Linux means X11 (and that Wayland only works with XWayland windows). They now say that both session types work, what
  Wayland needs (the udev rule), and that the Wayland layer has not been run on real hardware yet. The user guide
  section is now `#trackpad-macos-windows-and-linux`.

## 0.8.6

- **Linux: no more black flash when the pie or Settings appears.** The window was shown right after it was resized
  (and with whatever its buffer held), so for a frame or two a transparent window was on screen with nothing drawn
  into it at its new size: a black square for the pie, a black rectangle for Settings. It now stays hidden until it
  has its size and two empty frames were drawn at that size (at most 100 ms), and only then is mapped; a pie window
  that is on screen when a held trigger opens Settings is hidden for the resize. New log line `ui.show_wait_done`.
- **Wayland, trackpad: the pointer no longer freezes while the pie is open.** During a trackpad hold the Wayland layer
  grabs the touchpad's whole node (`EVIOCGRAB`) to swallow the extra finger's click and the scroll, but that also took
  the pad's pointer motion away from the compositor, so the pie opened and the cursor could not reach a slice. The
  reader still sees every finger, so while it holds the pad it now moves the pointer itself: the first finger's travel
  goes to a small virtual mouse (`CursorPie virtual pointer`, `/dev/uinput`), 10 px per mm before the compositor's own
  pointer speed (`CURSORPIE_PAD_GAIN` changes it). Without write access to `/dev/uinput` the pad is not grabbed at all,
  so the cursor keeps moving and only the swallowing of the extra click and scroll is lost.
- **Linux, Wayland session: the app starts again instead of failing with `Glutin(NotSupported("provided native
  window is not supported"))`.** The window was meant to run through XWayland (only eframe's `x11` feature is asked
  for), but eframe's `accesskit` feature pulls in `accesskit_winit`, whose default features switch on `winit/wayland`;
  winit prefers Wayland whenever `WAYLAND_DISPLAY` is set, so the pie got a native Wayland window that glutin could not
  make an EGL surface for. `run()` now forces winit's X11 backend when `DISPLAY` is set (`force_x11`).

## 0.8.5

- **Open URL slices: new window or the page that is open, with the browser extension that ships in the program.** An
  "Open URL" slice with an `http(s)` link now offers the same two badges as "Open app" (green + = new window, yellow =
  go to the tab that has the page), **only when the browser extension is on** (Settings > Behavior > Browser
  extension, off by default). Without it the slice just opens its link: a browser does not say which pages are open,
  and guessing opened the same page twice. The extension (`assets/browser-extension/`, compiled in with
  `include_str!`; Firefox and Chrome, Edge, Brave, Vivaldi) reports the open `http(s)` tabs; a tab is the page of a
  link by address, then by site and path (redirects, `www.`, query), then by site. CursorPie is its own native
  messaging host (`bridge::host`, started by the browser; no second program), registered per browser by
  `bridge::install` (a file on Linux and macOS, a registry key on Windows), and talks to the running app over a
  token-protected loopback socket (`bridge`). New switches: `--export-extension [DIR]`, `--install-browser-bridge`,
  `--uninstall-browser-bridge`. See `docs/OPEN_URL_CHOICE.md` and `docs/BROWSER_BRIDGE.md`. Not yet run on a real
  browser, and a release needs a signed Firefox add-on to stay installed.
- **The first press after a long idle time is no longer slow, and the listeners keep listening (macOS, Windows).**
  After minutes of doing nothing the pie sometimes took long to appear, or the press seemed lost and had to be
  repeated. These are the causes found by reading the code (not yet reproduced on a machine), each addressed:
  *Windows* trimmed the whole working set when the UI went to sleep (`SetProcessWorkingSetSize(-1, -1)`), which also
  evicted the code of the mouse hooks and the UI, so the next press page-faulted it all back in; a low-level hook
  that answers too slowly is dropped by Windows. Sleep now compacts the heap (`HeapCompact`), which gives back freed
  memory and leaves what is in use alone. Windows can also remove low-level hooks silently (it is known to after a slow
  callback, and reported after a resume or an unlock) and the message loop then never ended, so the hooks were never
  installed again: a **watchdog**
  notices that the system still sees input while no hook has run, and reinstalls both hooks at once (at most every
  15 s); a watchdog tick that arrives more than 10 s late means the machine slept, and the hooks are then
  reinstalled before the first press. The hook thread runs a little above normal priority, and the process opts out of power throttling (EcoQoS)
  and of the timer-resolution rule for hidden programs, which Windows 11 applies to a program with no window.
  *macOS* can put the hidden accessory app into App Nap, which stretches timers (the hold timer, the trackpad thread) and
  lowers thread priority; the process now declares a user-initiated activity that still lets the Mac sleep. The event
  tap is now looked at every 2 s and turned on again when macOS switched it off while nothing was calling the
  callback. The trackpad source decided "the Mac slept" with `Instant`, which does not count sleep, so a real sleep
  went unnoticed (the trackpad stayed dead until a restart) while a throttled thread was taken for one and restarted
  a healthy source; it now also compares the wall clock.
  *Both:* sleep emptied the whole icon cache, which saved a few KiB and made the first pie decode its icons (a
  multi-size TIFF each on macOS) while the hold timer ran. The icons of the general pie now stay decoded
  (`icons_to_keep_asleep`, at most 32 strings). New: `platform::stay_responsive`, `IconCache::keep_only`,
  `platform/windows/power.rs`, `platform/macos/activity.rs`. Not yet run on Windows or on a Mac: check with
  `CURSORPIE_DEBUG=1` (new events `sys/power_throttling`, `sys/app_nap`, `tap/hook` with `restarting` and
  `re_enabled`).
  Icons in a submenu that showed their text first and the app icon a second later: the warm-up on press covered
  only one level of submenus (it now walks every level, shallowest first), and a warm-up miss on a cold system was
  noted as a failure, which kept the pie's own lookup from trying again for 3 s (the pie now hands such an icon to
  the worker thread at once, which notes a real failure).
- **Wayland: adapters for Hyprland, Sway and GNOME (written from their documentation, not run, not compiled where
  written).** Per-app profiles, "app under the cursor" and "bring the app forward" were KDE-only. `wayland/adapter.rs`
  now picks an adapter from the compositor, and every adapter fills the same state as the KWin one, so the rest of
  the program is unchanged. **Hyprland** is read through its control socket (`j/clients`, `j/monitors`,
  `j/activewindow`, `j/cursorpos`); **Sway** through its IPC socket (`GET_TREE`), without a pointer, because Sway has
  no call to read it: the focused app then stands in for the app under the cursor and the report says so; **GNOME**
  through a small extension, `packaging/linux/gnome-extension/`, that pushes the pointer and the window list (no
  titles) to CursorPie's D-Bus service and answers one `Sync` call, and which the user installs and enables (the
  report text gives the steps). Settings and the welcome page name the adapter that is in use. `Session` also finds
  Hyprland and Sway by `HYPRLAND_INSTANCE_SIGNATURE` and `SWAYSOCK` when `XDG_CURRENT_DESKTOP` is empty. Still open:
  COSMIC and niri, a layer-shell reader of the pointer for Sway, and the new spikes SW11 to SW15 in
  `docs/WAYLAND_SUPPORT.md`.

- **Flows can be written by an AI agent.** New `docs/FLOWS_FOR_AI.md` describes the flow JSON for someone who cannot
  open the app: the two places a flow lives (a `.cursorpie-flow.json` file to import, or `flows` in `config.json`),
  every field, the seven block kinds, shortcut key codes and modifier bits, what running a flow does (40 ms between
  steps, commands are not waited for), the repair rules and which mistakes silently change behaviour (a missing
  `Start` leaves nothing to run), a recipe, a checklist and a short validator. `docs/flow.schema.json` is a JSON
  Schema of the file and `docs/examples/flows/` holds examples that the unit tests parse, so the page cannot drift
  from `config::flow`.
- **Trackpad scan, and the trackpad hold is on from the first start (macOS, Windows, Linux).** The trackpad hold
  was off until someone found the switch, although the welcome window already says "touch and hold the trackpad".
  Now `tap::touch::probe` counts the trackpads without starting anything: the Raw Input device list on Windows (the
  same precision-touchpad test the source uses, no registration), `/proc/bus/input/devices` on Linux (readable
  without the `input` group) and `ioreg` on macOS (the private framework stays unloaded). A **new** user who has one
  gets the hold switched on before the tap starts; it is written to `config.json` when the welcome window ends, so
  a quit during the welcome still shows it again. Someone with a `config.json` already keeps their choice. Settings
  has a **Scan for trackpad** button (it switches the hold on when it finds one; Save keeps it) and "Detect again"
  of the Mouse panel counts the trackpads too. The count on macOS (`ioreg` classes `AppleMultitouchTrackpad`,
  `BNBTrackpadDevice`) is not yet run on a Mac.
- **Windows: Settings no longer needs two clicks with a trackpad.** While Settings was open, every pause of the
  finger on the trackpad (aiming at a button) looked like a hold, was answered with "the pie is off", and made the
  window take the focus again. On Windows the toolkit does that by injecting an Alt key press, which turned the next
  click into a menu activation: two clicks, or a very long hold, were needed. A trackpad hold that lands on a Settings
  window that already has the focus is now ignored; one that lands while Settings is behind another window still
  brings it back. A mouse press is answered as before, except that a focused window is not raised again.
- **Wayland: the pie opens on the cursor at fractional scales.** The pie and the Settings card are XWayland windows
  placed in the toolkit's points, while the pointer comes from KWin in its own, and the two differ at a fractional scale
  or with KDE's "scale X11 applications by the system" setting. The position is now mapped from the KWin output it is
  on to the X monitor of the same screen (`wayland::coords`, `platform::window_point`), which is exact whatever either
  scale is; X11 sessions, other systems and screens that cannot be matched keep the position as it was. Not yet run on
  Plasma 6.8; the size of the pie is not converted (see `docs/WAYLAND_SUPPORT.md`).
- **Wayland: the welcome page and the lint.** The second page of the welcome window no longer tells a Plasma 6.8
  user to "log in with an X11 session" (there is none). In a Wayland session it shows the two steps of the udev rule
  and a live status that follows `wayland::report`: **Mouse access** (Allowed / Missing), **Mouse capture** and the
  **KWin adapter** (when it is off, only per-app profiles are, and the page says so); it turns to Ready by itself once
  the rule is in place and the session has been restarted. In an X11 session the page says that a Wayland session
  works too. `uinput::decode_all` uses `as_chunks` (clippy `chunks_exact_to_as_chunks`, Rust 1.88 and later), which
  fixes `cargo tools ci`.
- **Wayland: the rest of the first pass, and the checks pass.** `zbus` is now an ordinary Linux dependency (the
  `wayland-kde` feature is gone: the KDE adapter is always built on Linux). New: while the screen is locked or the
  session is in the background (a VT or user switch) the mouse is given back to the desktop (`logind`, the same
  path as the pause switch); a shortcut is recorded from the Settings window's own key events (no keyboard is ever
  opened, so the udev rule gives no program a way to read what is typed; Super and the order of the modifiers are
  recorded too); the trackpad's extra-finger click and two-finger scroll are swallowed by grabbing the touchpad's
  node while a hold runs (never for more than 15 s); the udev rule also covers touchpads, and `install.sh` installs
  it when it runs as root. The `wayland` log category is in `docs/DEBUGGING.md` and the lint, which fixes
  `cargo tools ci`. Still not run on a Plasma 6.8 machine; see `docs/WAYLAND_SUPPORT.md`.
- **Wayland backend (first code, not yet run on a Plasma 6.8 machine).** In a Wayland session CursorPie
  now grabs the mouse through evdev and writes everything it does not use to a virtual mouse it makes
  with `/dev/uinput` (`tap/wayland.rs`, `platform/linux/uinput.rs`); shortcuts go through a virtual
  keyboard. The state machine, `config.json` and the X11 backend are unchanged, and
  `CURSORPIE_BACKEND=x11` still selects the X11 layer. It needs the udev rule
  `packaging/linux/70-cursorpie.rules`; without it capture stays off and Settings says what to do. The
  KDE Plasma adapter (pointer, windows, app under the cursor, bring an app forward) is a script loaded
  into KWin over D-Bus. Not done yet: a native pie window, adapters for other compositors. See
  `docs/WAYLAND_SUPPORT.md`.

## 0.8.4

- **Flow tab: a node editor for chains of steps.** A new **Flow** section in Settings draws a sequence of actions as
  blocks joined by wires, like the graphs of Unity or Blender. Add **Shortcut**, **App**, **URL / file**,
  **Command** and **Wait** blocks, drag from the dot on the right of a block to the dot on the left of the next one,
  and the flow runs the blocks in that order, from **Start** to **End** (a new flow is Start wired to End; new
  blocks go in before End, and a flow from before End existed gets one after its last block). Drag a header to move
  a block, click a wired input dot to unplug it; blocks that are not wired in are marked and never run. The canvas
  has a dot grid and moves like a node editor: drag the empty canvas, hold the middle button (the trigger button is
  let through over this canvas while Settings is open), or use two fingers or the wheel; **Alt/Option + scroll**,
  a pinch or Ctrl/Cmd + scroll zoom from 30% to 125% (further in the text would turn blurry). The **App** block has
  a **Pick...** button that opens the list of installed apps, like the one in a slice. A slice runs a flow with the
  new **Flow** action (Menu > a slice > Action > Flow). Flows live in `config.json` (`flows`, with `Action::Flow` and
  `Action::Wait`), so they are saved, backed up and undone like everything else. Files from before flows load as
  they were; a config that uses flows cannot be read by older versions.
- **Sharing flows.** **Export** writes `<name>.cursorpie-flow.json` to Downloads, **Import** adds a file as a new
  flow (new id, unique name; the file is checked and limited like a config). A profile file now also carries the
  flows its slices use, so a shared profile keeps working; CursorPie warns when an imported flow or profile holds a
  shell command.

## 0.8.3

- **A recorded shortcut keeps the order you pressed it in.** Pressing Cmd, Shift, Opt and then W was saved and shown
  as "Opt + Shift + Cmd + W": modifiers were stored as a bitmask, so the order was lost and the label always used one
  fixed order. The recorder now follows every modifier press (a `FlagsChanged` event on macOS, the modifier key
  events of the hook on Windows and Linux), saves the order as `"order"` next to `"mods"` in `config.json`
  (left out when it is the old fixed order, so existing configs load and save unchanged), and the label shows it.
  Playing the shortcut presses the modifiers in that same order.
- **Windows: launch at login that survives odd paths.** The installed copy compared its own path with the install
  folder as plain text. A short 8.3 name (`C:\Users\JOHNDO~1`), a junction, or the other case of a non-ASCII user
  name made it believe it was a _different_ copy: it started "the installed copy" (itself) and exited, and so did
  that one, over and over, so nothing ever reached the tray at login. Paths are now compared as the file system
  sees them, and the copy that was handed over to never hands over again (`--handover`; `--autostart` is passed on).
  A zip copy no longer copies itself over an install made by the Setup wizard.
- **Windows: a login entry that cannot work is repaired.** The Settings checkbox now also checks that the program the
  entry names still exists, and the entry is rewritten when it points at a deleted or moved folder or is an old one
  without `--autostart`. A login start waits for the notification area of the taskbar (not only the taskbar window)
  before it creates the tray icon.
- **Windows: app icons.** The icon of an app is now looked up in `App Paths` (per user, per machine, 32-bit view), in
  the Start menu shortcuts and in the program folders, not only among the running programs and `PATH`, so apps that
  are not running show their icon. A program picked with the file chooser is remembered. The icon itself comes from
  the shell (`IShellItemImageFactory`, 256 px with alpha, the one Explorer shows) with the old calls as the fallback.
- **Windows: the app list.** Settings lists the apps of the Start menu (by their shortcut names) plus those under
  `App Paths`. It reads the registry and the shortcut files directly instead of the text of `reg.exe`, which is
  localized and in the OEM code page, so it also works on non-English Windows and with non-ASCII paths.

## 0.8.2

- **Settings comes forward with the trackpad trigger too.** With Settings open behind another window, a press of the
  mouse trigger brought it back but a trackpad hold did nothing. The hold now does the same as the press.

- **Windows: the install step asks first.** The first start of an unzipped copy no longer copies itself, registers
  a startup entry and starts the copy without a word (which behavior-based antivirus engines judge as a dropper);
  it asks once. "No" is remembered. Updating an app that is already installed stays silent.
  `scripts/sign-local.ps1` signs a build with a self-made certificate for your own PCs.

- **The app icon is the same artwork on every system.** macOS had the Windows icon as a hard-edged full-bleed square
  with only four sizes; `app-icon.icns` now has every size from 16 to 1024 px (with the @2x ones) in Apple's rounded
  shape with its margin and shadow. Linux had no window icon at all and a single 512 px file: the window now carries
  its icon (taskbar, Alt+Tab), and the package installs 16 to 512 px into `hicolor` and refreshes the icon cache.
  Windows is unchanged.

- **Login start and antivirus.** Windows: the startup entry now passes `--autostart`, and such a start waits for the
  taskbar (at most 90 s) before it creates the tray icon and the hooks, so the tray icon is no longer lost at login;
  the program no longer rewrites its registry entries at every start; the `.exe` has version information and a
  manifest (`asInvoker`); the Setup wizard has version information too. macOS: the LaunchAgent starts the app with
  `open -a` instead of the bare binary and refuses a path Gatekeeper copied temporarily (App Translocation); the
  ad-hoc signature is tied to the bundle id so Accessibility is not forgotten after a rebuild. Linux: the autostart
  entry waits 5 s so the display is ready. Antivirus programs can still object to an unsigned build: see
  KNOWN_ISSUES.md.

- **Fix: the virtual trackpad feels like a trackpad.** The pointer's speed was worked out from the time between two
  frames of the window, which can be one millisecond: a burst of frames made the pointer jump and stutter. The speed
  is now averaged over time (a frame weighs as much as it lasts), the gain is a smooth curve (3.5 points per
  millimeter when slow, up to four times that in a flick), a long gap is no motion, and the arrow's window sits on
  whole points so it no longer trembles.
- **Fix: the mouse buttons of the virtual trackpad did nothing.** A click on the glass only made the surface sink; no
  click ever reached the app under the virtual pointer, and a click made while the pie was open was not the extra
  click either (the virtual trackpad has one finger, the extra click wanted two). Now the left and right buttons are
  the primary and the secondary click of the trackpad: when the button comes up, a click is made at the virtual
  pointer (`platform::click_at`; the system's pointer goes there and back at once, on macOS the event carries the
  place), with the click count of a double click. A press that belongs to the pie (the pie was open, or opened while
  the button was held) is never a click for the app, and while a pie of the virtual trackpad is open a click is its
  extra click. Holding the button and moving off the glass no longer lifts the finger, and a moment without a
  pointer (the click's trip) no longer lifts it either. Not yet: dragging with the button held.

- **Virtual trackpad (developer tool: `cargo tools trackpad`).** Starts the app with a trackpad that floats exactly in
  the middle of the screen, to try the trackpad gestures with a plain mouse. Your real pointer is the finger: while it
  is over the glass it touches, leaving the glass lifts it. A virtual pointer, drawn as an arrow on the screen outside
  the trackpad, is what the finger moves, relatively and with acceleration like a real trackpad (the surface counts as
  160 x 100 mm, so a slide of so many millimeters moves it so far). The pie opens at the virtual pointer, aims with it
  and acts on the app under it; a hold opens it, lifting the finger runs the slice, and a click of the mouse is the
  extra click. It feeds the same recognizer as a real trackpad (macOS, Windows and Linux alike). Its feel stands in
  for the haptic click a mouse cannot give: the surface sinks at a click, a ring spreads, the frame glows and a short
  click sounds; the ticks of the pie are shown the same way and are also sent to the haptic engine, which a finger on
  a real trackpad feels. A slider in the lower frame sets the size and a switch the click sound. It is behind the Cargo feature `vtrackpad`, which the command turns on: an ordinary build has none of its code. Options:
  `--width POINTS`, `--silent`, `--release`. Nothing is written to the settings. While the trackpad is open the main
  window is parked (1 x 1 point, click-through) instead of hidden, which keeps the trackpad's own windows alive.
- **Fix: messages from the tap could wait for the wrong window.** The tap woke the UI with a plain
  `request_repaint`, which means "the window drawn last". With a second window (the virtual trackpad) that could be
  the wrong one, and a pie that had been released stayed on the screen. The wake-up now asks for the root window.

- **Fix: the pie ran actions behind an open Settings window.** The gesture state machine of the tap did not know
  that Settings was open: a pie that was not drawn still opened, hovered and, on release, ran the slice under the
  pointer (opening apps and so on), and the side buttons ran their actions too. While Settings is open a press of the
  trigger now starts no gesture (it only brings Settings forward and says why), the side buttons are left to the app,
  and a pie that was open when Settings opened runs nothing when it closes.
- **While a pie is open, no click reaches the app under it, for the mouse and the trackpad, on all three systems.**
  Each path handled its own buttons (the trigger, the right-click switch, mapped side buttons, the extra finger) and
  every other press fell through: a left click, a right click with the switch off, a middle or extra button no mode
  uses. The app under the pie got a click that was aimed at the pie (on Linux a kept press was even sent again at its
  release). Now, as the last step of the state machine, a press while a pie is open is swallowed with its drags and
  its release, even when the pie closed in between; the wheel is swallowed too while a mouse pie is open. Every press
  decides afresh, so a lost release never leaves the next click stuck. New `tap` log event `pie_click_swallowed`
  (`button`). The macOS and Windows taps read the left button and the wheel for this only while a pie is open.

- **A trackpad pie no longer lets the pointer's movement through to the page below.** A mouse hold has a button down,
  so the apps under the pie hear no motion. A trackpad hold has none: the finger moves the real pointer, and the pie
  window was click-through, so every move reached the app under it (a video player showed its controls, links
  highlighted). The pie window now takes the pointer while a trackpad hold runs, and is click-through again when the
  pie closes; a mouse pie is unchanged. New `ui` log event `pie_input` (`takes_pointer`).

- **No more scrolling leaks under a trackpad pie, and the pie stays smooth (macOS, Windows, Linux).** The two-finger
  scroll that the holding finger and the extra finger make was swallowed only while the hold ran. The gesture ends
  the moment a finger lifts, but the scroll does not: its last deltas and its glide (momentum on macOS and on Windows
  precision touchpads) arrived after that and reached the app under the pie, which then scrolled by itself, and an app
  that sees the middle of a scroll it never saw begin jumps and stutters. Now a scroll that the pie began to swallow is
  swallowed to its end: after a pie that opened closes, wheel events keep being swallowed for 300 ms, each one extends
  that by 120 ms (a glide is eaten to its last event), and never beyond 1.2 s. The tail ends at once when a new finger
  lands, so a scroll of your own is never eaten, and a hold that never opened (a tap, a slide, a second finger that came
  early) starts none. On Linux the wheel is grabbed for the same time, on its own (clicks pass during the tail), and a
  wheel press no longer asks the X server where the pointer is. New `touch` log events: `scroll_tail`,
  `scroll_tail_end`, `scroll_swallowed` and, with `trace`, `scroll_drop` (counts and reasons only).

- **Faster, quieter defaults.** The pie opens after a hold of 150 ms by default (was 500 ms) and the hold can be as
  short as 50 ms (was 150 ms; the Settings slider and the file limit both go down to 50). The default pie shows
  icons only, without the name under each icon (`show_labels` is `false` for a new config; a saved config keeps its
  value, and a file that does not have the field keeps the names). The welcome guide says "150 ms" instead of a
  rounded "0.1 s". The website demo pie shows icons only too, and its text says 150 ms.

- **A mouse now aims a pie that a trackpad hold opened.** Moving the pointer with a mouse while the finger kept
  still did nothing, because the pie only heard the finger move (a mouse press has drag events, a trackpad hold has
  none). While a trackpad hold runs, the pointer is now watched every 10 ms on its own thread, so the mouse, the
  trackpad and a mix of both hover the slices, and lifting the finger runs the slice under the pointer. Nothing runs
  while no hold is running.

- **The two-finger trackpad hold is gone, on macOS, Windows and Linux.** Two fingers on a trackpad are what the
  system scrolls with, so the pointer did not move and the pie had to be aimed by the fingers themselves, which was
  never reliable. A hold is now always one finger, with one more finger for the extra click. Removed: the _Fingers to
  hold_ setting, the config field `touch_fingers` (an old file that has it still loads; the field is ignored and
  dropped at the next save), the code that aimed the pie by finger travel, and on Linux the X11 pointer grab that
  went with it. Scrolling is still swallowed while a hold runs, because the extra finger makes two.

## 0.8.1

- **Linux touchpad reader builds on Windows and macOS again, and platform checks follow the project rules.** The
  Linux reader's kernel calls (`ioctl`, `poll`, opening `/dev/input` nodes) moved from `tap/touch/evdev.rs` to
  `platform/linux/evdev_io.rs`, so `cargo clippy --all-targets` and `cargo test` no longer fail on Windows. Code
  outside the platform layer asks `platform::OS` instead of using `cfg!` (app icon names, settings window resize,
  icon worker count, the hidden pie window's taskbar button and window class). Dead code is reported on Linux
  again. No change in behavior for users.

## 0.8.0

- **README, website and docs now say that the trackpad works on all three systems.** The README (tagline,
  highlights, "Try it", platform table and a new _Trackpad_ section), the website (description, hero, steps, feature
  card, platform cards and FAQ), `docs/INSTALL.md` (a new _Trackpad hold_ section with the `input` group on Linux) and
  the plan's title and summary describe trackpad hold on macOS, Windows (precision touchpad) and Linux (X11).

- **Trackpad hold and haptics on macOS (off by default).** `docs/TRACKPAD_HAPTICS_PLAN.md` is implemented: with
  _Settings > Behavior > Trackpad_ on, touching and holding opens the pie, moving the finger hovers a slice and
  lifting runs it; one more finger that clicks or taps switches the mode (or picks the hovered mode, or does
  nothing); the pie ticks on a Force Touch trackpad. Mouse and trackpad are one input: the same state machine
  (`Gesture` now records its `Source`), the same timings and profiles. New config fields `touch_trigger`,
  `touch_fingers`, `touch_still_mm`, `touch_click_action`, `touch_typing_guard_ms`, `haptics`, `haptic_hover`
  (an old file loads unchanged). The touches come from Apple's private `MultitouchSupport.framework`, loaded at run
  time only while the feature is on and checked before it is trusted. The recognizer and the rest of the core are
  tested on every OS; the macOS layer has not been run on a Mac yet. The `touch` category is added to the diagnostic log.

- **Trackpad hold on Linux (X11), off by default.** The same feature as on macOS and Windows, for a touchpad under
  X11: hold to open the pie, move to a slice, lift to run it, one more finger to click. The fingers are read from
  `/dev/input/event*` (libinput hides them from X), only while _Open the pie with a trackpad hold_ is on, which needs
  the `input` group; Settings says so when it is missing. While a hold runs the left and right buttons are grabbed so
  the extra click never reaches the app (any other click is sent again at its release), and the typing guard listens
  for key presses (not which key). No haptic ticks on Linux. The reader repeats the last frame every 20 ms because
  resting fingers send no events. The parser, device detection and grab logic are tested; the device thread has not
  been run on a real touchpad yet. New `touch` log events: `device_open`, `device_closed`, `x_grabs`. **`cursorpie --touchpad-debug [seconds]`** checks the
  chain (session, saved switch, `input` group, why each device was taken or left out, permission, surface size) and then
  shows the fingers and the recognizer's decisions live, ending with what to fix.

## 0.7.7

- **A quieter top bar in Settings.** Undo and Redo buttons are gone (Ctrl/Cmd+Z and Ctrl/Cmd+Shift+Z still
  work). Cancel and Save are two small round dots in the corner, in the sage theme: red (the theme's "bad"
  color) cancels and the accent color saves and closes. A third, ochre dot shows between them while there are
  unsaved changes. Their icons appear on hover.

- **Ask before throwing changes away.** Cancel, Esc and the window's close button now open a "Discard changes?"
  sheet when something is not saved. Enter discards, Esc or a click outside keeps editing. With nothing unsaved
  they close at once as before.

- **Ctrl+S / Cmd+S saves.** It writes the config (with its backup) and keeps Settings open; the unsaved dot
  goes away and the footer says "Saved.". The saving code moved to `App::persist_edit`, shared with the
  Save dot.

- **Profile actions as a row of icons.** In Menu, "..." now unfolds Rename, Export, Import and Delete as icons
  to its left, hides "+" meanwhile and turns into an X that folds them again. The text menu is gone; names
  are tooltips.

- **A tidier About card.** Website, GitHub, Report an issue and Downloads are round icon buttons in the top
  right corner of the card instead of a row of text buttons, and the tagline has room below it.

- **A plan for trackpad and haptic support on macOS.** `docs/TRACKPAD_HAPTICS_PLAN.md` describes how a hold on a
  trackpad can open the pie, how an extra finger can switch the mode, how haptic feedback can be added and how
  mouse and trackpad stay one input. It is a plan: nothing in it is implemented yet.

- **Documentation brought in line with the code.** `docs/CONFIGURATION.md` lists `app_icon_shadow`, `appearance`,
  `pie_style` and the three `glass_*` fields and gives the real default accent (`[90, 120, 94]`); `docs/USER_GUIDE.md`
  describes Appearance and Pie material and the per-system quick actions and default profiles; `docs/INSTALL.md`
  says the welcome window opens on Linux too; `CONTRIBUTING.md` lists the `diag-lint` CI step and the options of
  `clean-logs` and `zip`; the release text no longer says that only "stable" tags publish assets, which the
  workflow does not check.

- **The name in Isometra.** "CursorPie" is now set in Isometra Regular, the lettering of the logo, in the
  About tab, the welcome window and the fallback wordmark. Isometra is SIL OFL 1.1 and is listed under
  "Built with" in About.

- **A livelier welcome window.** The first page has a banner with the name, the version and rings that pulse
  around the logo, and the three steps (Hold, Flick, Release) are numbered cards that light up one after the
  other. Pages fade in, the page dots stretch, and pages 2 and 3 say "Step 2 of 3" and "Step 3 of 3". Pages 2
  and 3 use two columns, so nothing is left empty or cut off, even on the smallest card.

- **Fixed: the links in About did nothing on Linux and Windows.** "GitHub repository", "Report an issue",
  "Downloads", the email link and the links under "Built with" used egui's `open_url`, which does nothing when
  eframe is built without its `web_browser` feature, as it is on these two systems. They now use
  `platform::open_url` (`xdg-open`, `ShellExecute`, `open`). The diagnostic log gets `ui/open_link` and
  `act/open_url`.

- **Sliders with anchors.** Every slider in Settings shows marks and round numbers under its rail and snaps to
  them, so a round value is easy to land on. Hold Shift to move freely; the arrow keys move one step.

- **A simpler profile toolbar in Menu.** The profile picker is a chip with the icon of the profile's app and a
  list of every profile with its icon and number of apps. The apps button shows the real icons of the apps.
  New, Rename and Apps open as dialogs in the middle of the window instead of small popups, and Rename, Export,
  Import and Delete moved into one "..." menu. In the Apps dialog every app is a tile with its icon; hover a
  tile to remove it.

- **Choose an app from a list, the same on every system.** Where Settings asked for an app through the
  system's file chooser, it now shows the installed apps with their icons and a search box. The list comes from
  the desktop entries on Linux, the `.app` bundles on macOS and `App Paths` on Windows. "Browse files" is still
  there for an app that is not listed. The diagnostic log records `installed_apps` with the number of apps found.

- **Shadow under the app's icon.** The icon in the center of an app's pie gets a soft shadow on a plain light
  center, so it stands out from the white. Behavior > App icon in the pie > Shadow under the icon lets you
  choose Automatic (the default), Soft, Strong or None. A saved config without the setting gets Automatic.

## 0.7.6

- **Pick... of an Open app action uses the same app list as Add app.** The button now opens the searchable list
  of installed apps, with icons, that **Add app** in the profile's apps already used. **Browse files...** at the
  bottom of the list is still there for an app that is not listed. A slice still takes the icon of the app you
  choose, as before. The list stores the name of the app on macOS and its id on Windows and Linux.

- **The Menu tab pie follows "Show labels".** With the names off in Behavior, the pie of the Menu tab shows icons
  only, like the real pie.

- **Drag slices in the Menu tab.** Drag a slice onto another one to move it there, or onto the trash button of the
  editor to delete it (the trash turns red when the slice is over it). The "+" slice in the pie is gone: **+** now
  sits next to the trash under the editor, and in the empty editor when no slice is selected.

- **A slice that opens an app can wear that app's icon.** Select an **Open app** slice in the Menu tab and a new
  row, **Icon of this app**, offers three looks: **Full color** (the app's own icon), **White** (a white glyph, for
  dark and glass pies) and **Black** (a black glyph, for light pies). Choosing an app with **Pick...** puts its
  color icon on a slice that still has the starting icon, and keeps the look of a slice that already shows an app's
  icon. In `config.json` the icon of the slice is `appcolor:<app id>`, `app:<app id>` or `appblack:<app id>`.

## 0.7.5

- **About links to the CursorPie website.** A **Website** button sits first among the links at the top of About,
  and the **Project** panel has a **Website** row with the address.

- **Add and delete a slice sit under the pie, each in its own circle.** They used to be in the far corner of the
  editor, a long way from the pie. Now two round buttons, **+** and the trash, are right under the pie. The trash
  deletes the selected slice (it is grayed out while no slice is selected) and is still where you drop a dragged
  slice to delete it; it turns red while a slice is over it. The editor keeps only Move left and Move right.

## 0.7.4

- **CursorPie looks like its website.** Settings, the welcome window and the pie now use the colors of the site: a
  cream page with paper-white panels and sage-green neutrals (and graphite with a faint sage tint in the dark
  appearance, so the accent still stands out). A new **Sage** accent is the first swatch and the default of a new
  install; the other accents are still there, and a saved accent is not changed. The logo is now the real artwork,
  with the name in the sidebar, drawn in near black on the light appearance and near white on the dark one. Links and
  the version badge in the accent are lightened or darkened until they can be read.

- **App icons on Linux.** The pie can now show the real icon of an app in the middle of its own pie, in
  both styles of **App icon in the pie**. CursorPie reads the `Icon=` of the app's desktop entry and finds it in
  the icon themes (the user's theme, the ones it inherits from, `hicolor`, `pixmaps`, Flatpak and Snap). PNG
  icons need nothing more; an icon that is only an SVG needs `rsvg-convert` (`librsvg2-tools`) and is skipped
  without it. Roles such as `@browser` use the icon of the app that fills them.

- **Choose how the app's icon looks in the middle of an app's pie.** Behavior has a new panel, **App icon in the
  pie**: **One color** (the silhouette in the ink of the pie, as before and still the default) or **Full color**
  (the app's own icon in its own colors). A small animation shows the icon flying into the pie and either keeping
  its colors or losing them. The setting is `app_icon_style` in `config.json`; it only applies while **Show the
  app's icon in the center** is on. The icon in the **Look & feel** animation follows the choice too.

- **The "Stop waiting" button is now a red circle with an X.** It sits under the "Waiting for the file chooser..."
  pill and shows "Stop waiting" as a tooltip when the pointer rests on it.

## 0.7.3

- **Linux: cancelling the file chooser ends the wait.** After Cancel or the close button, CursorPie could keep
  showing "Waiting for the file chooser..." because it waited for the chooser's output to close, which a helper
  the chooser leaves behind can hold open. It now waits for the chooser itself to end. The waiting screen also has
  a **Stop waiting** button for a chooser that never answers or is lost behind other windows.
- **macOS: the file chooser is handled the same way.** `osascript`, which shows the Finder's chooser, now goes
  through the same code as zenity and kdialog on Linux: CursorPie waits for the chooser itself to end, not for
  its output pipe to close. **Windows** needed no change: its chooser returns the moment you press Cancel or the
  close button. The dimmed "Waiting for the file chooser..." screen and the **Stop waiting** button are the same
  on every system.
- **Settings waits calmly while a file chooser is open.** Choosing an app, an icon image or a config file opens
  the system's own chooser beside Settings, and Settings stayed live under it: a second click opened a second
  chooser, Esc closed Settings, and a press of the trigger pulled Settings over the chooser. Settings is now
  dimmed with "Waiting for the file chooser..." until the chooser closes, then comes back to the front. **Import...**
  now hands the focus back too.
- **Linux: pressing the trigger while Settings is open brings it forward again.** With Settings open on another
  workspace, or behind the app you were using, a press (or holding the trigger in the middle of the pie) only made
  its taskbar button blink: the window manager refuses the focus request the toolkit sends. CursorPie now asks the
  way a pager does, goes to the workspace Settings is on and raises it, as it already does on macOS. If it still
  does not work on your desktop, start with `CURSORPIE_DEBUG=1` and run `cargo tools diag`: it says whether the
  window manager refused the request.
- **A website for CursorPie.** The repository now has a landing page (`index.html`) that detects your system and
  links the matching download, and shows the project's Markdown documentation (install guide, user guide,
  configuration, known issues, this changelog) in the browser, read straight from the files in the repository. It
  is published to GitHub Pages by `.github/workflows/static.yml` on every push to `main`. See `README-WEBSITE.md`.
- **The project now lives at `github.com/sebastvnd/cursorpie`.** The clone and download links in `docs/INSTALL.md`
  and `CONTRIBUTING.md` point there, and the PNG logos and sample icons in `icon/` are no longer ignored by git.
- **Windows: the build works again.** Asking the window manager for a workspace (Linux code that is also built and
  tested on every system) converted a `u32` with `c_long::from`, which does not exist where `c_long` is 32 bits
  wide, so the Windows build failed. It is now a plain cast, with a test that the workspace number survives it.

## 0.7.2

- **More settings that show what they do, and bigger demos.** The animated demos have a taller stage (190 points
  instead of 140), so the pies, the mouse and the captions are no longer cramped. Four panels got a demo of
  their own: **Look & feel** (the pie at the size you set, with or without names and the app's icon, the accent
  on the slice under the pointer, solid or glass), **Side buttons** (a side-button press runs the pie they
  follow; the ring colors and the right-click switch show which one that is), **Memory** (the idle timer runs
  out, the icons and the editor's copy are dropped, the trigger loads them again) and **Mouse** (each extra
  button is pressed, heard and listed, then Done saves them).
- **Glass pie is always dark.** The glass pie now keeps its smoked graphite look in the light appearance too.
  Pale glass over a light page looked washed out and nothing like Liquid Glass. Solid still follows the
  appearance, and so does the rest of Settings.
- **Icons only.** A new switch in Settings, **Show names under the icons** (Behavior > Look & feel), hides
  the names in the pie so each slice shows just its icon, a little larger. It is on by default; the names stay
  in Settings, so you can still tell the slices apart while you edit them. Old config files keep the names.
- **Settings that show what they do.** On **Behavior**, the panels _Trigger_, _Pie of the app in front_, _App
  detection_ and _Other Spaces / desktops / workspaces_ now start with a small animation instead of only a
  sentence: the button going down and the pie opening after your hold time, which pie a hold and a double-click
  open, which window the pie works on, and what "Open app" does to an app on another desktop. The animation
  follows the choice and the sliders as you change them, and it only runs while you can see it.
- **A calmer footer in Settings.** The notice that the pie is off while Settings is open, and every other short
  message in the footer, now use the small grey text of the shortcut hint. "Open System Settings" is a plain text
  link with an arrow instead of a boxed button next to a red pill.

## 0.7.1

- **Linux: "Open app" no longer starts a second copy of an app that is already open.** The slice compared what
  you typed (`firefox`, `gnome-terminal`, a path) with the one name the window manager gives the window
  (`org.mozilla.firefox`, `org.gnome.terminal`), found no match and started the app again. It now compares every
  name a window has (its class, its instance and the desktop entry's id, window class and program), so the
  existing window is raised, on whichever workspace it is.
- **Linux: the Files, Terminal and Browser slices raise the window that is open.** They were shell commands, which
  always start a new copy (a new Firefox window on every click). They are now "Open app" slices that name a role:
  `@browser`, `@files` or `@terminal`. CursorPie asks the system which app that is (`xdg-settings`, `xdg-mime`, or
  the first terminal it finds), then raises its window or starts it. Your existing `config.json` is upgraded
  when it is loaded (the file itself changes at the next save): only the three unchanged default commands are
  replaced, your own commands stay. You can
  type these roles in any "Open app" slice. If the system cannot name the app, the old command runs.

## 0.7.0

- **Quit asks before it quits.** Pressing **Quit CursorPie** while Settings is open (sidebar, General, or the
  tray / menu bar icon) opens a sheet instead of quitting at once. With nothing unsaved it asks "Do you really
  want to quit?" (**Cancel** / **Quit CursorPie**). With unsaved changes it says so and offers **Keep editing**,
  **Save and quit** and **Quit without saving**. Esc, the close button and a click outside mean "not yet". A
  quit from the tray while Settings is open brings the window to the front to ask; with Settings closed there
  is no window, so it quits at once. Cancel, Esc and the window's close button still discard edits without
  asking, as before.
- **Pressing the trigger while Settings is open now shows it.** The pie is off while Settings is open, and a
  Settings window hidden behind other windows made the press look like it did nothing. The press now brings
  Settings to the front (also from minimized), asks the system to draw attention to it and says in its footer
  why the pie did not open.
- **A new look: modern flat, light and roomy.** Settings, the welcome guide and the pie are redrawn with large
  rounded cards, pill buttons, more space, a full-height sidebar with icons and a calmer palette. The default
  accent is indigo, the color of the logo (an existing `config.json` keeps the accent it has).
- **One kind of popup.** Adding a slice and choosing an icon are sheets over a dimmed window; New, Rename,
  All apps and Share are popovers under their button. All close with Esc or a click outside.
- **Inter and Phosphor.** The interface uses the Inter font and one icon set everywhere, both embedded in the
  binary (about 340 KB, MIT and OFL licenses, see About). Slice icons are named `ico:<name>` in `config.json`;
  the letter icons of older files (`C/V`, `Tab`, `>_`...) are shown as the matching picture.
- **The pie is light** to match Settings; the hovered slice and its icon follow the accent.
- **Light and dark.** Settings and the pie have a dark appearance. Behavior, Look & feel offers System (the
  default), Light and Dark, and the window changes as soon as you choose.
- **Glass pie.** Behavior, Pie material: the slices can be frosted glass, with sliders for Clearness, Frost and
  Shine and a preview. The frost is drawn, not a blur of the desktop behind the pie.
- **Settings layout.** The page is centered instead of leaving an empty strip on the right, and the sidebar
  folds to icons (button next to the title, or Ctrl / Cmd + B). Settings now opens folded (compact) by default.
- **Calmer choices.** The pills are gone: a choice between a few options (Appearance, Pie material, slice kind)
  is a segmented control, the kind of an action is a drop-down with icons, the icon picker's groups are icon
  tabs sharing one line, and its Image / Folder / Reload buttons are one "+" menu.
- **Smoother glass.** The glass pie is painted as continuous gradients instead of five flat bands and a
  polygon outline: the frost, the sheen and the rim are one smooth field, and the rim catches a light from the
  top left like Apple's Liquid Glass. The pie's shadow is a single blurred shape.
- **Drop-down lists and popups** share the look of the rest of Settings (they were drawn in egui's dark style,
  with black text fields).

## 0.6.1

- **macOS: "Open app" now opens the app on the screen you clicked.** With two screens, an app that already had
  a window on the other screen (Finder, say) was taken for "already here", so the slice only focused the window
  on the screen you were not looking at. Now, with more than one screen, the app's front window on the screen
  under the pointer is raised and given the focus; when the app has none there, its front window is moved to
  that screen, centered, and raised. Full-screen windows, apps without a window, single-screen setups and "Do
  nothing" behave as before; when a window cannot be moved the app is opened as before. Uses Accessibility only
  (no private calls).

## 0.6.0

- **Side-button slices can use every button of the mouse, not only back and forward.** A slice set to "Side
  buttons" now lists one more action for each extra button of your mouse (button 5 and up). The new `extra` map
  of a `Mode` in `config.json` stores them; files without it load as before, and a mode without extras is written
  exactly as it was. Buttons nothing is set for still reach the app.
- **CursorPie knows your mouse.** A new `mice.json` (next to `config.json`) keeps a profile for every mouse it has
  met, keyed by its USB or Bluetooth id: the buttons the system reports and the ones you scanned. Every five
  seconds it looks at what is connected and tells a new mouse from one it already knows (a note in Settings). On
  Linux the buttons come from `/proc/bus/input/devices`; on Windows from the number of buttons; macOS has no list
  CursorPie can read, so scan the mouse there.
- **Settings, General, Mouse:** "Scan this mouse" (press every extra button once; the buttons do nothing else
  meanwhile), "Detect again" and a list of known mice with "Forget".
- **Linux:** extra buttons are numbered like the other systems (X button 10 and up are button 5 and up) and are
  grabbed only while a mode uses them.

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

- **Open app follows the app to its desktop or Space.** An _Open app_ slice for a program that is already
  running on another virtual desktop (Windows) or Space (macOS) now takes you there and brings its window to
  the front, instead of leaving you where you were. **Settings > Behavior > Other desktops / Other Spaces**
  chooses: _Switch to the desktop where the app is_ (the default), _Do nothing_ (open the app on the current
  desktop as before) and, on macOS, _Bring the app's window to this Space_ (experimental: it relies on private
  system calls and falls back to switching when macOS refuses them). On macOS the switch works with the
  Mission Control shortcuts Control+Left / Right (on by default) even when the system setting _When switching
  to an application, switch to a Space with open windows for the application_ is off. If the app is not
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
- **Linux: Open app follows the app to its workspace.** The window manager does the switching: an _Open app_
  slice for a program that is already running asks it to activate the app's window, and the usual window
  managers go to the workspace that window is on. **Settings > Behavior > Other workspaces** offers _Switch_
  (the default) and _Do nothing_; _Bring the window_ does not exist on Linux and acts as _Switch_. An app that
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
