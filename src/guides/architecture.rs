//! # Architecture
//!
//! CursorPie is a small program with a strict split between *observing input*, *deciding what it
//! means*, *doing something about it* and *showing it*. This guide explains how those parts are
//! wired together and which rules keep it responsive and crash-free.
//!
//! * [Threads](#threads)
//! * [One window for everything](#one-window-for-everything)
//! * [Data flow](#data-flow)
//! * [The gesture state machine](#the-gesture-state-machine)
//! * [Shared state and lock order](#shared-state-and-lock-order)
//! * [Side-button routing](#side-button-routing)
//! * [Idle sleep and caches](#idle-sleep-and-caches)
//! * [Undo, redo and backups](#undo-redo-and-backups)
//! * [Rules for the event-tap callback](#rules-for-the-event-tap-callback)
//! * [OS APIs and external tools](#os-apis-and-external-tools)
//!
//! ## Threads
//!
//! | Thread | Started by | Owns | Talks to the others through |
//! |---|---|---|---|
//! | **main / UI** | `eframe::run_native` in [`crate::run`] | the [`App`](crate::ui::app::App), every egui window, the menu bar icon, the keyboard-layout snapshot | receives [`Msg`](crate::msg::Msg) values |
//! | **`cursorpie-tap`** | [`tap::spawn`](crate::tap::spawn) | the OS hook (`CGEventTap` + `CFRunLoop`, the Windows low-level hooks + message loop, or on Linux the XInput2 grabs on the shared X connection), the gesture state | sends `Msg` (`Warm`, `ShowPie`, ...) through a [`Notifier`](crate::msg::Notifier); enqueues actions |
//! | **`cursorpie-mouse`** | [`mouse::spawn_watcher`](crate::mouse::spawn_watcher) | the book of known mice (`mice.json`) | looks at the connected mice every few seconds; sends `Msg::MouseChanged` when one arrives that is new or comes back |
//! | **`cursorpie-actions`** | [`actions::init`](crate::actions::init) | the queue of [`Action`](crate::config::Action)s to execute | receives jobs from [`actions::run_in`](crate::actions::run_in) |
//! | **`cursorpie-timers`** | [`tap::spawn`](crate::tap::spawn) | the queue of delayed jobs of the tap: the hold timer and the double-click timer, each a short job that runs when it is due | jobs lock the gesture state, send `Msg`, post a click; started with the tap so a press inside the hook never has to start a thread |
//! | **`cursorpie-config-saver`** | [`tap::spawn`](crate::tap::spawn) | the writes of the config that `set_active` asks for; several in a row become one write of the newest state | receives the config to write through a channel |
//! | short-lived | on demand | file chooser, icon scans, launched programs, update check | send `Msg` or touch shared state |
//!
//! Why separate threads?
//!
//! * The tap callback is on the input path of the whole system. If it is slow the OS disables the
//!   hook (macOS says so; Windows silently removes it). On X11 nothing disables a grab, but a stalled
//!   tap thread would hold back the events of the grabbed buttons until it recovers. Either way it only
//!   *decides* and never *does* anything slow.
//! * Actions (sending keys, spawning `open`, sleeping 40 ms between sequence steps) must run in
//!   the order the user triggered them, so they all go through one worker.
//! * egui must be driven from the main thread.
//!
//! ## One window for everything
//!
//! There is exactly **one** native window. In *pie mode* it is transparent, borderless,
//! click-through, always on top, `pie_size` pixels square and moved so its center is under the
//! cursor. In *Settings mode* the same window is resized to the card size, made interactive,
//! lowered to the normal window level and centered on the display that contains the cursor.
//! [`ui::app`](crate::ui::app) switches between the two (`show_pie`, `open_settings`,
//! `close_settings`). The window only looks transparent if the renderer supports it: Windows and
//! Linux use the `glow` renderer for that reason, macOS keeps `wgpu`, and on Windows the OS frame
//! (white box, border, shadow) is stripped every frame by `platform::clear_window_frame`. On Linux that
//! function does nothing, and per-pixel alpha needs a compositing manager on X11 (see
//! `docs/developer/UI_DESIGN.md`).
//! A third mode, the *notice* (`ui::hud`, `ui::app::notice`), makes the window a small pill near the
//! edge of the display for a second or so: it says what the actions worker is doing. It is
//! click-through like the pie and is taken over by the pie or Settings at once.
//! Because the window is click-through, the pie never steals mouse events: the
//! event tap is what sees the mouse, and the UI only draws.
//!
//! ## Data flow
//!
//! ```text
//!  mouse / keyboard
//!        |
//!        v
//!  +--------------------+  Msg::ShowPie / Hover / HidePie / OpenSettings
//!  | cursorpie-tap      |------------------------------------------------+
//!  |  tap::handle       |                                                |
//!  |  gesture machine   |--- Action ---> +-------------------+           v
//!  +--------------------+                | cursorpie-actions |   +---------------+
//!        ^   reads                       |  CGEvent / open / |   | main thread   |
//!        |                               |  /bin/sh          |   |  ui::app::App |
//!   config::cfg()  <---- writes ---------+-------------------+   |  pie / Settings|
//!   side::STATE                                                   +---------------+
//!        ^                                                              |
//!        +------------------- Save in Settings ------------------------+
//! ```
//!
//! 1. The tap thread classifies each event ([`tap`](crate::tap)): trigger button, side button,
//!    right button or key.
//! 2. When something visible happens it sends a [`Msg`](crate::msg::Msg). `Notifier::send` also
//!    calls `request_repaint`, which wakes the UI even while its window is hidden.
//! 3. [`App::logic`](crate::ui::app::App) drains the channel every frame and updates UI state;
//!    drawing happens in `App::ui`.
//! 4. When the user releases over a slice the **tap thread** (not the UI) picks the slice and
//!    queues its action. The UI is told to hide the pie but is not involved in running anything.
//!
//! ## The gesture state machine
//!
//! The trigger button (middle button by default) drives a small state machine,
//! `tap::gesture::Gesture`, held behind a mutex that the tap callback and the timer thread share.
//! Each timer carries the `id` of the press that created it and does nothing if the state moved on.
//!
//! ```text
//!                    press
//!        Idle ------------------> Pending --------------- hold pie_hold_ms ---------+
//!         ^                          |  |                                           |
//!         |                          |  +-- drag > 6 px (14 px with "flick") -------+--> ActivePie
//!         |   quick release,         |        (not flick: Passthrough, press and     |   (flick: opens
//!         |   no app profile:        |         drag are handed back to the app)      |    the pie now)
//!         |   click is replayed      |                                               |
//!         +--------------------------+                                               v
//!         |                          | quick release, app has its own profile    +----------+
//!         |                          v                                           | ActivePie|
//!         |                     WaitSecond --- 2nd press within double_click_ms -->  (app pie)
//!         |                          |                                           +----------+
//!         |   timer expires: first   |                                                |
//!         +---- click is replayed ---+        hover / cross rim (submenu) ----------+ |
//!         |                                                                          | |
//!         +<------------------- release: run the slice, Msg::HidePie ----------------+ |
//!         +<-- release (InSettings / Passthrough) <-- hold settings_hold_ms in the center
//! ```
//!
//! Notes:
//!
//! * Every non-idle state carries its `Source` (`Mouse` or `Touch`). A trackpad hold (macOS, Windows and Linux X11, see
//!   `tap/touch/`) feeds the same states through `trigger::touch_press`, `touch_move`, `touch_release`
//!   and `touch_cancel`; a trackpad gesture that ends early replays no click, and the mouse trigger
//!   passes through while a trackpad gesture runs.
//! * A quick click must still reach the app, so the press is *swallowed* and, if it turns out to
//!   be a click, **replayed** with [`actions::post_mouse`](crate::actions::post_mouse). Replayed
//!   events carry [`SYNTH_TAG`](crate::actions::SYNTH_TAG) so the tap ignores them.
//! * With a per-app profile and the default `DoubleClick` mode, a plain click in that app is
//!   delivered only after the double-click window has passed (see `double_click_ms`).
//! * Settings can only be opened by *holding* in the center of the pie that the hold opened. A pie
//!   opened by flicking or by a double-click has no settings timer. (The menu bar also opens it.)
//! * Crossing the outer rim over a submenu slice pushes the slice onto `path`, re-centers the pie
//!   on the cursor and sends `Msg::ShowPie` again (the UI animates it as a new ring).
//! * While [`PAUSED`](crate::tap::PAUSED) is set, or when macOS disables the tap, the state is reset
//!   to `Idle` and a `Msg::HidePie` is sent, so nothing is ever left half-finished.
//!
//! ## Shared state and lock order
//!
//! | State | Type | Readers / writers |
//! |---|---|---|
//! | the live config | [`config::cfg()`](crate::config::cfg) (`Mutex<Config>`) | everyone reads; Settings *Save* and `set_active` write |
//! | gesture state | `Arc<Mutex<Gesture>>` | tap callback and its timer thread |
//! | side-button choices | private `Mutex` in [`side`](crate::side) | tap, UI |
//! | flags | [`RECORDING`](crate::tap::RECORDING), [`SWALLOW_KEY`](crate::tap::SWALLOW_KEY), [`PAUSED`](crate::tap::PAUSED), [`TAP_ACTIVE`](crate::tap::TAP_ACTIVE) | atomics |
//!
//! **Lock order: gesture, then config, then side state.** Never take the gesture lock while
//! holding the config guard, and never lock the config while holding the `side` lock (functions in
//! `side` take `&Config` as a parameter for exactly this reason). Always lock with
//! [`config::lock`](crate::config::lock), which recovers from poisoning, so a panic in one thread
//! can never wedge the event tap.
//!
//! Settings edits a **copy** of the config (`App::edit`) and replaces the global one on *Save*,
//! after [`Config::sanitize`](crate::config::Config::sanitize).
//!
//! ## Side-button routing
//!
//! The back/forward buttons run the actions of the *active mode* slice. An app with its own
//! profile has two pies, so something has to decide which of them the buttons follow. That
//! decision (the *owner*) lives in [`side`](crate::side) and is documented there. In short: the
//! app's pie by default, or whichever pie the user last picked a mode in, or toggled with a
//! right-click or the menu bar item. The pie ring is tinted by [`side::view_of`](crate::side::view_of).
//!
//! ## Idle sleep and caches
//!
//! The window is hidden almost all the time, and egui does not repaint a hidden window, so the idle
//! cost is already near zero. What stays allocated is *state*: decoded icon textures and the
//! Settings working copy. Five mechanisms keep that small without making the pie slower.
//!
//! **1. A bounded icon cache** ([`ui::icons`](crate::ui::icons), built on [`cache::Lru`](crate::cache::Lru)).
//! Decoding an app icon (a multi-size TIFF from macOS) or scaling an image file takes far longer
//! than a frame, so decoded textures are kept: at most [`CAPACITY`](crate::ui::icons::CAPACITY)
//! entries, evicting the least recently used. Eviction happens once per frame in `App::logic`
//! and never removes an entry the previous frame used, so a screen showing more icons than the
//! capacity (the icon picker) grows the cache while it is open instead of reloading icons every
//! frame. The cache **survives** Settings: opening or closing it only calls
//! [`IconCache::revalidate`](crate::ui::icons::IconCache::revalidate), which drops image files whose
//! modification time or size changed (one `stat` each), and failed loads so they can be retried.
//!
//! **2. Warming up on press.** [`tap::trigger`](crate::tap) sends [`Msg::Warm`](crate::msg::Msg::Warm)
//! as soon as the trigger goes down, carrying the profiles that may open. The UI loads their icons
//! ([`icons_to_warm`](crate::ui::icons::icons_to_warm)) while the hold timer runs, which is
//! `pie_hold_ms` (150 ms by default) before the first pie frame. A warm cache costs only hash
//! lookups, so this is free in the common case. Sending the message follows the tap rules below:
//! it never blocks.
//!
//! **3. Bounded at the door, drawn only when visible.** A cache limits what it *keeps*; these rules limit
//! what is *read and drawn* in the first place:
//!
//! * An image is refused from its header, before it is decoded, when a side is over
//!   [`MAX_SOURCE_SIDE`](crate::ui::draw::MAX_SOURCE_SIDE) (4096 px). A decoded bitmap costs 4 bytes per pixel,
//!   so the limit is also a memory limit. [`prepare_icon`](crate::ui::draw::prepare_icon) premultiplies a small
//!   image in place and drops the full-size source before it resizes a big one, so a big image never exists in
//!   several copies at once. Only the texture (at most 128 px) is kept.
//! * [`ui::iconfit`](crate::ui::iconfit) prepares an image the user chooses, on the file chooser's worker
//!   thread: it is used as it is when small, saved as a 256 px PNG in the icons folder when big, or refused
//!   with a sentence for the status bar. The user is told, never left with a silent `?`.
//! * A grid that can pass a screenful is virtualized (`ScrollArea::show_rows`): the icon picker builds and
//!   loads only the visible rows, and at most `LOADS_PER_FRAME` new images per frame, asking for another frame
//!   while some are waiting.
//! * Data that does not change between frames, such as the tiles of the "Add a slice" panel
//!   (`Ed::add_catalog`), is built once when its panel opens and dropped when it is not showing.
//! * A user-chosen text file is read with `Read::take` ([`read_text_capped`](crate::config::read_text_capped)),
//!   so a wrong file is never loaded whole.
//!
//! **4. Sleep.** [`ui::sleep::Sleeper`](crate::ui::sleep::Sleeper) is a small state machine over
//! `Instant`s:
//!
//! ```text
//!   Busy ---- window hides ----> Idle ---- sleep_after_s ----> Asleep
//!    ^                            |                              |
//!    +- pie / Settings shows -----+-- Msg::Warm (poke) ----------+
//! ```
//!
//! `App::sleep_step` drives it from both `logic` and `ui`, and asks egui for one frame at the
//! moment the idle time ends (`request_repaint_after`), because a hidden window schedules nothing
//! itself. Entering sleep (`App::enter_sleep`) trims the icon cache to the icons of the general pie
//! ([`icons_to_keep_asleep`](crate::ui::icons::icons_to_keep_asleep), a few dozen KiB), drops the
//! Settings working copy and everything derived from it, then calls
//! [`platform::release_unused_memory`](crate::platform::release_unused_memory)
//! (`malloc_zone_pressure_relief` on macOS, `HeapCompact` on Windows), because freed memory is
//! otherwise kept by the allocator and still counted against the process. A press counts as
//! activity (`Sleeper::poke`), otherwise the first press after a long idle time could load icons
//! and have them dropped by the sleep that was already due.
//!
//! **5. Trim.** Sleep waits `sleep_after_s` and needs a frame of the window, and a press can
//! allocate far more than it keeps: on macOS one app icon is a multi-size TIFF of several MiB
//! that is copied, decoded and reduced to 128 px, and libmalloc keeps the freed blocks in a cache
//! that it empties only slowly, as other large blocks come and go. A pie with a dozen app icons
//! showed hundreds of MiB in Activity Monitor that nothing used, falling only at the next press.
//! So the icon loader ([`appicon`](crate::ui::appicon)) gives the pages back at once after a big
//! source (`BIG_SOURCE`, 1 MiB), and tells [`ui::trim`](crate::ui::trim) about every icon it
//! made. `trim` is a thread of its own with a small state machine ([`Debt`](crate::ui::trim::Debt)):
//! two seconds after the last icon, and after the pie or Settings went away, it calls
//! [`platform::release_unused_memory`](crate::platform::release_unused_memory). It needs no frame
//! and never runs while something is on screen. The icons of submenus are not read in the hold
//! timer any more: [`icons_on_first_frame`](crate::ui::icons::icons_on_first_frame) names what the
//! first frame paints, the rest goes to the icon worker thread.
//!
//! **Sleep must never make the next press slow.** The program exists to answer one press after
//! hours of doing nothing, so what sleep frees is only what the next press does not touch: icons
//! of other profiles, the editor, allocator slack. Two things were tried and are wrong. Emptying the
//! whole icon cache saved a few KiB and made the first pie decode its icons while the hold timer
//! ran. Trimming the whole working set on Windows (`SetProcessWorkingSetSize(-1, -1)`) evicted the
//! code of the hooks and of the UI too, so the first press after a long idle time page-faulted
//! thousands of pages, and a low-level hook that answers too slowly is dropped by Windows.
//! `platform::stay_responsive` (called once from [`run`](crate::run)) also keeps the OS from
//! throttling a program that shows no window: App Nap on macOS, power throttling (EcoQoS) on
//! Windows. The hooks themselves are watched: see "Rules for the event-tap callback" below.
//!
//! | State | Freed when Settings closes | Freed when the UI sleeps | Never freed |
//! |---|---|---|---|
//! | `App::edit` (config copy), undo history, picker lists, backup list, `Ed::add_catalog` | yes | yes | |
//! | icon textures | all but the general pie's (`icons_to_keep_asleep`), at once | all but the general pie's (`icons_to_keep_asleep`) | the general pie's |
//! | the live config, side-button state, gesture state | | | yes |
//! | egui font atlas | rebuilt on every close (`fonts::reset_atlas`) | rebuilt, only if zoomed text was drawn | the glyphs of the normal sizes |
//! | GPU surface | | | yes |
//! | `App::edit.flows`, `App::run_report`, the Flow tab's selection and sheets | yes | yes | |
//!
//! egui never evicts a glyph from its atlas, so **never lay text out at a size taken from user input**
//! (a zoom, a slider) without snapping it to a short ladder: the Flow canvas uses `crisp_scale`, 4 sizes per
//! doubling. A size that comes from a continuous value cuts a whole new set of glyphs per value.
//!
//! **A hidden window frees nothing by itself.** eframe runs no egui pass and paints nothing while
//! the window is hidden, only `App::logic`. Dropped textures, the font atlas that `reset_atlas` replaced
//! and every other free that rides on a pass are therefore carried out only when the window next
//! shows, and a `malloc_trim` before that finds nothing to give back. So `App::close_settings` does
//! not hide the window at once: `App::begin_close_flush` keeps it on screen, click-through and empty,
//! for a few frames, and `App::finish_close` then hides it and trims the heap. Anything that frees a
//! large amount of memory while the window is hidden must do the same.
//!
//! `Config::sleep_after_s` sets the idle time (0 = never sleep). Rules for new code: a cache must
//! be bounded, must be cleared (or trimmed to what the next press needs) in `enter_sleep`, and must never be filled on the frame that opens
//! the pie (warm it instead). A list longer than a screenful is virtualized, and input is limited
//! where it enters (see 3 above).
//!
//! ## Undo, redo and backups
//!
//! Settings edits a copy of the config, `App::edit`. [`History`](crate::history::History) keeps
//! whole snapshots of it: simple, and impossible to get out of sync with editors that change the
//! value from dozens of places. `window.rs` calls `observe` once per frame, after the widgets ran.
//! A burst of changes (typing, dragging a slider) is coalesced into one step, committed once the
//! value has not changed for [`SETTLE`](crate::history::SETTLE); undoing in the middle of a burst
//! returns to the value before it. At most `HISTORY_LIMIT` (50) steps are kept. Import, *Reset to
//! defaults* and restoring a backup replace `edit` and are therefore undoable too. The editor
//! selection (`Ed`) is not part of the history, so after an undo `Ed::fit_to` makes it valid again.
//! Cmd+Z is left to egui while a text field has focus, and to the recorder while a shortcut is
//! being recorded.
//!
//! The history ends with the Settings session. For mistakes that were saved,
//! [`save_config_with_backup`](crate::config::save_config_with_backup) keeps the file it replaces as
//! `config.backup.1.json` (shifting older ones, five in all, see [`config::backup`](crate::config::backup)).
//! The General tab lists them and loads one into the editor, still subject to *Save* and *Undo*.
//!
//! ## Rules for the event-tap callback
//!
//! Everything reachable from `tap::handle` must follow these rules:
//!
//! 1. **Never block for long.** No sleeping, no waiting for a process, no file I/O. Spawn a
//!    thread instead (see the hold timer, `config::set_active`).
//! 2. **Never panic out of it.** `tap::guarded` wraps the handler in `catch_unwind` and
//!    returns `Keep` (the event passes through) on a panic.
//! 3. **Fail open.** When unsure, `Keep` the event. Only swallow events that this program is
//!    sure it owns (the trigger button, the side buttons while they run an action, the right
//!    button while a pie is open, a key while recording a shortcut).
//! 4. **Release the gesture lock before sending messages or posting events.** The transition
//!    functions in `tap::trigger` take the `MutexGuard` by value so they can `drop` it first.
//! 5. **Ignore your own events.** Anything posted by [`actions`](crate::actions) is tagged with
//!    `SYNTH_TAG` and skipped by the native hook glue before `dispatch`. X11 events carry no such
//!    tag, so the Linux backend lifts its own grabs while it posts a press and puts them back after
//!    the server has handled it (`Conn::inject_button` in `platform/linux/x11.rs`), and never sees
//!    its own event.
//!
//! ## OS APIs and external tools
//!
//! Each need is met by one function of the [`platform`](crate::platform) facade (or by the `tap`
//! glue); the macOS, Windows and Linux implementations sit side by side in `platform/macos/`,
//! `platform/windows/` and `platform/linux/`. The
//! [Windows migration plan](https://github.com/sebastvnd/cursorpie/blob/main/docs/developer/WINDOWS_MIGRATION.md)
//! and the [Linux migration plan](https://github.com/sebastvnd/cursorpie/blob/main/docs/developer/LINUX_MIGRATION.md)
//! explain the designs; the Linux one also says what is built and what is not.
//!
//! | Need | macOS | Windows | Linux (X11) | Where |
//! |---|---|---|---|---|
//! | global mouse/keyboard events | `CGEventTap` (HID, head insert, active) | `WH_MOUSE_LL` + `WH_KEYBOARD_LL` | XInput2 passive grabs (`XIGrabButton`, asynchronous) on the trigger and the side buttons; a core keyboard grab only while a shortcut is recorded | [`tap`](crate::tap) |
//! | send keys / replay clicks | `CGEvent` posted to the HID tap location | `SendInput` | XTest (`XTestFakeKeyEvent`, `XTestFakeButtonEvent`); key code = Linux key code + 8; grabs lifted around a press | `platform::press_shortcut`, `platform::post_mouse` |
//! | permission | `AXIsProcessTrusted` (Accessibility) | none | none (X11) | [`platform`](crate::platform) |
//! | frontmost app | `NSWorkspace` | `GetForegroundWindow` + process image name | `_NET_ACTIVE_WINDOW` + `WM_CLASS`, mapped to a desktop-entry id | `platform::frontmost_bundle_id` |
//! | app under the pointer | `CGWindowListCopyWindowInfo` + `NSRunningApplication` | `WindowFromPoint` | `_NET_CLIENT_LIST_STACKING` + window geometry, hit test in `platform/linux/toplevels.rs` | `platform::bundle_id_at` |
//! | real app icon | `NSWorkspace.iconForFile` as TIFF, decoded by the `tiff` crate | `PrivateExtractIconsW` as RGBA | `Icon=` of the desktop entry through the icon themes (`platform/linux/icontheme.rs`), PNG read directly, SVG through `rsvg-convert` | [`ui::appicon`](crate::ui::appicon) |
//! | give freed memory back | `malloc_zone_pressure_relief` | `HeapCompact` on the process heap (never a working-set trim) | `malloc_trim` (glibc only) | `platform::release_unused_memory` |
//! | key labels on any layout | TIS + `UCKeyTranslate`, snapshot taken on the main thread | `MapVirtualKeyExW` + `ToUnicodeEx` | not built yet: the US/ANSI fallback table | [`platform::layout`](crate::platform::layout) |
//! | tray icon | `tray-icon` / `muda` (menu bar) | `tray-icon` / `muda` (notification area) | not built yet: Pause and Quit are in Settings > General | [`ui::tray`](crate::ui::tray) |
//! | no Dock / taskbar entry | `NSApplicationActivationPolicyAccessory` | `with_taskbar(false)` | nothing set yet (no window-type hints) | `platform::set_accessory_policy`, [`run`](crate::run) |
//! | launch at login | a LaunchAgent plist | `HKCU\...\Run` value, pointing at the installed copy | an XDG autostart entry, `~/.config/autostart/cursorpie.desktop` | `platform::set_launch_at_login` |
//! | install, uninstall, one running copy | none (the `.app` is the install; `scripts/make-dmg.sh` packs it into a `.dmg`) | the program installs itself (copy to `%LOCALAPPDATA%\Programs\CursorPie`, `HKCU\...\Uninstall` entry, named mutex), or the Setup wizard `installer/cursorpie.iss` does and the program then leaves the install alone (`unins000.exe` next to it) | none (packages, or the tarball of `scripts/package-linux.sh`); a `flock` on `$XDG_RUNTIME_DIR/cursorpie.lock` | `platform::ensure_installed`, `platform::handle_command_line`, `platform::single_instance` |
//! | open apps / URLs, file chooser, app id of a file, hover tick | `open`, `osascript`, `mdls`, `afplay` | `ShellExecuteW`, `GetOpenFileNameW`, file name, `MessageBeep` | `gio launch`, `xdg-open`, `/bin/sh -c`, `zenity` or `kdialog`, desktop-entry file name, `canberra-gtk-play` | [`platform`](crate::platform) |
//!
//! The macOS tools ship with the system, so the app needs nothing installed on macOS or Windows. On
//! Linux the program links to `libX11`, `libXi`, `libXtst` and `libXrandr`, and uses `xdg-open` and
//! `gio` when they are present; `zenity` or `kdialog` (file chooser) and `canberra-gtk-play` (hover
//! tick) are optional.
//!
//! The welcome window of the first start has no platform-specific code: it is drawn by
//! `ui/settings/welcome.rs` and only its texts depend on the OS (`Os::needs_permission`,
//! `Os::tray_name`, `Os::has_tray`). Whether it is due is decided by the marker file `welcome_shown` next to
//! `config.json` (`config/first_run.rs`).
//!
//! ### Portable layer
//!
//! The hook glue converts each native event to an `Ev` and calls `tap::dispatch`, which answers
//! `Keep` or `Drop`. Key codes in events and in `config.json` are **macOS virtual key codes**; the
//! Windows and Linux backends translate at the edge with [`platform::keymap`](crate::platform::keymap)
//! (on Linux the X11 keycode is the Linux key code plus 8).
//! Coordinates are logical points ([`geometry::Point`](crate::geometry::Point)); on Windows the
//! hook divides pixels by the monitor scale, and on Linux by the one global X11 scale. On Windows a
//! pointer move is never swallowed. On X11 a move reaches the tap only while a grabbed button is
//! held, and the tap never swallows a move outside a gesture because it gets none.
