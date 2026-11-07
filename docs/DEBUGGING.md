# Debugging with the diagnostic log

CursorPie can write a trace of what it does, so a problem that only happens on one machine ("the pie opens in
the middle of the screen, not under the pointer") can be found without a debugger and without editing the code.
The log is **off by default** and costs one atomic read per log call while it is off.

The log is written by the `diag` crate (`diag/`, a crate of its own with no dependencies); the app sets it up in
`src/debug.rs`; the analyzer is `cargo tools diag` (`tools/src/diag.rs`).

## Debug builds only

The log is part of **debug builds** (`cargo run`). A **release build has none of its code**: the log lines are
removed when the program is compiled, so the shipped app writes no log, reads no `CURSORPIE_DEBUG`, and installs
no panic hook. `CURSORPIE_DEBUG=1` does nothing in it.

To collect a log from a user's machine, give them a release build that keeps it:

```
cargo build --release --features debug-log
```

(the `debug-log` feature of the app switches on the `force` feature of the `diag` crate). To check that an
ordinary release build is clean, build it without the feature and search the binary for a log event name, for
example `strings target/release/cursorpie | grep -c show_pie`: it should print `0`.

## Turn it on

| Way | When |
|---|---|
| `CURSORPIE_DEBUG=1 cursorpie` | one run, from a terminal |
| an empty file named `debug` next to `config.json` | a copy that starts at login (remove the file to turn it off) |

The value is a list of words:

| Word | Meaning |
|---|---|
| `1`, `all` | every category |
| `tap,ui` | only those categories |
| `-tap` | not that category (`all,-tap` is everything but `tap`) |
| `trace` | also the events that come in the hundreds, such as every pointer move of a drag |
| `stderr` | also print every line on the terminal |
| `0`, `off` | off |

A set variable wins over the file, so `CURSORPIE_DEBUG=0` silences a `debug` file for one run.

| Category | What it records |
|---|---|
| `diag` | the first line: version, OS, session variables (`XDG_SESSION_TYPE`, `DISPLAY`, ...) |
| `sys` | the monitors and the pointer as the platform reports them at start, and whether the OS agreed not to throttle the hidden program (`power_throttling` on Windows, `app_nap` on macOS: `off`) |
| `x11` | the display scale chosen for the pointer coordinates; bringing Settings forward: `raise_own` (the window the window manager listed, its workspace and the one shown) and, 200 ms later, `raise_own_seen` (whether it became the active window and which workspace is shown) |
| `wayland` | the Wayland backend (Linux): `keyboard_failed` (the virtual keyboard could not be made: error kind), `session_away` (the login session is locked or in the background, so the grabs are lifted: `away`), and the KWin adapter: `bus_failed` (no session bus: error), `script_loaded` (`id`, `started`), `script_load_failed` and `sync_failed`, and the other adapters: `adapter_unreachable` (`adapter`: `hyprland` or `sway`; the compositor's socket did not answer, written once per outage). Counts and error kinds only: never a window title or an app name |
| `tap` | the session type, the button grabs, every press and release, `pie_click_swallowed` (a click while a pie is open that no path took, swallowed: `button`) (position from the event **and** from the X server), the verdict, the hold timer (`hold_open`, or `hold_cancelled` when it gave up), the double-click window, which profile a press resolved to (`resolve`: whether a target app was found, never which), the slice that was selected, the side buttons, the mice (`mouse_look`: how many are connected and whether it was a new one, a returning one or the same; `mouse_scan`: how many buttons a scan found; never a name), and whether the input hook runs (`hook`: `running`, or why not: `blocked` on macOS, `refused` on Windows, `no_x_server` or `grab_failed` on Linux; `restarting` on Windows is the watchdog installing the hooks again after the system stopped calling them, `re_enabled` on macOS is the tap turned back on after the system switched it off, with `ok` telling whether that worked) |
| `touch` | the trackpad source (macOS, Windows and Linux, off unless `touch_trigger` is on): `framework` (macOS: whether the system's touch library loaded), `source` (`running`, `stopped`; Windows also `refused`, `no_window`), `layout` (Windows: finger slots and the size in mm of a touchpad whose descriptor was read), `devices` (on Linux also `denied`: touchpads found but not readable, so the `input` group is missing; how many trackpads were seen and started), `device_open` and `device_closed` (Linux: one touchpad's reader and its size in mm), `x_grabs` (Linux: the left and right buttons, and the wheel (`wheel`), are grabbed for a hold or the tail of its scroll, or let go), `device_skipped` (a device that is not trackpad-sized: its size in mm; on Windows a touchpad whose descriptor is not understood: `why`), `arm` (a hold was armed: `elapsed_ms`, `double`, `started`), `open`, `release`, `cancel` (`why`: `slid`, `too_many_fingers`, `stale`, `device_lost`), `scroll_tail` (a pie that opened closed while its scrolling may still be coming: `fingers` still down, `dropped` wheel events swallowed so far), `scroll_tail_end` (`why`: `quiet`, `capped`, `new_touch`, `next_gesture`, `stopped`; `dropped` in all, `in_tail` after the pie closed), `pad_grab` (Wayland: the touchpad was taken from the desktop for a hold, or given back: `on`, `ok`), `scroll_swallowed` (a hold that never opened swallowed `dropped` wheel events), `scroll_drop` (`trace` only: one line per swallowed wheel event, `tail`), `extra_click` (`via`: `press`, `tap`, `tap_click`), `extra_action` (the action and whether it did something) and `haptic` (which tick, whether it played). Counts and decisions only: never a finger position |
| `ui` | the message that opens the pie, the position the pie asks for, the monitor it picked, the cursor fallback; `pie_input` (whether the pie window takes the pointer: a trackpad hold does), closing the pie, opening and closing Settings (`close_settings` is timed: saving; `raise_settings` says why Settings was brought forward; `chooser_open`, `chooser_busy`, `chooser_closed` (with `waited_ms`) and `chooser_abandoned` (the user pressed Stop waiting) follow the native file chooser), importing a config, and the timed `warm` (icon loading) and `sleep` (freeing memory) |
| `win` | where the toolkit says the window really is, for two seconds after a window was placed |
| `cfg` | loading `config.json` (outcome, size, time) and every save of it (`ok`, and the error kind when it failed: a failed save is otherwise silent) |
| `act` | every action the worker runs: its kind (never its content), how long it waited in the queue (`wait_ms`) and how long it ran (`ms`); every program it starts (`exited` with its exit code, `spawn_failed` with the error kind, on Windows `shell_execute` with the code), by program name only, never its arguments |
| `bridge` | the optional browser extension link (`docs/BROWSER_BRIDGE.md`): `start` (how many browsers the native messaging host was registered with, and how many registrations failed). Never an address or a tab |
| `run` | which "Open app" apps are running, asked once per press of the trigger on the `cursorpie-running` worker: `scan` (timed: `apps` asked about), `answer` (`stored`: whether the answer was kept or was already stale, `running`: how many of the apps run) and `spawn_failed` (the worker thread could not start, so no badges ever appear) |
| `panic` | any panic, with its location |

## Where the log goes

One file per run in the `logs` folder of the config folder, `cursorpie-<unix seconds>-<pid>.jsonl`
(`~/.config/cursorpie/logs` on Linux, `~/Library/Application Support/CursorPie/logs` on macOS,
`%APPDATA%\CursorPie\logs` on Windows). `CURSORPIE_DEBUG_DIR=/some/folder` moves it. The app prints the path on
stderr when it opens the file. Only the newest 10 files are kept and a file stops growing at 8 MiB. Every line is
written at once, so the log is complete after a crash.

Every line is one flat JSON object: `t` (seconds since start), `th` (thread), `cat`, `ev` and the fields of the
event, in the order they were written.

```
{"t":5.300,"th":"main","cat":"ui","ev":"show_pie","x":800.0,"y":500.0,"left":650.0,"top":350.0,"mon":"0,0 1920x1080"}
```

To delete logs by hand: `cargo tools clean-logs` (`--keep N`, `--older-than DAYS`, `--all`, `--dry-run`).

## Read it

```
cargo tools diag                 # the newest log
cargo tools diag path/to/file.jsonl
```

The analyzer follows one gesture through the layers and compares what each one says:

```
press (tap)  ->  msg_show_pie (UI)  ->  show_pie (asked position)  ->  win_obs (real position)
```

It reports, for example: the pointer from the event differs from the one the X server reports (scale problem);
the position changed between the tap thread and the UI; the window was asked to be at one place and the toolkit
reports another; the window sits in the middle of its monitor (the window manager placed it and ignored the
request); the app's scale differs from the toolkit's.

For a quick look without the tool: `jq -c 'select(.cat=="ui" or .cat=="win")' cursorpie-*.jsonl`.

The verdict also reports: a pie shown more than 300 ms after the hold timer fired (UI or tap thread busy), a timed
step slower than 250 ms, a config that could not be saved or read, and an action that waited more than 200 ms in the
queue.

### Other views of a log: `cargo tools log`

`diag` says where the chain broke. These say what happened, and none of them needs `jq`. Without a file they read
the newest log.

| Command | Shows |
|---|---|
| `cargo tools log summary` | lines, threads, how often each event was written; a `<- floods the log` mark on an event that should be `trace!` |
| `cargo tools log timeline` | one row per press: hold timer, UI lag, time to the pie, time held, where the window was asked and where it was seen |
| `cargo tools log grep ui:show_pie` | the lines of a category (`ui`), an event of a category (`ui:show_pie`) or an event of any (`:press`) as readable text; `--field x=800`, `--after S`, `--before S`, `--limit N`, `--raw` |
| `cargo tools log slow --ms 100` | the `timed!` steps that took that long, slowest first |
| `cargo tools log diff before.jsonl after.jsonl` | what a change did: findings gone, findings new, events whose count changed |
| `cargo tools log follow` | prints the lines of a run that is still going, as they are written (Ctrl-C to stop) |
| `cargo tools log report` | the block of `cargo tools info` and the verdict, to paste into a bug report |

A good loop for a fix: keep the log of the broken run, change the code, make the same gesture again, then
`cargo tools log diff old.jsonl new.jsonl`. Findings that differ only in their numbers count as the same finding.

## Add a log line

```rust
diag::log!("ui", "show_pie", x = x, y = y, profile, kind = ?kind);
```

The first two arguments are the category (the layer) and the event (what happened). Then any number of fields, in
three forms that can be mixed:

| Form | Writes |
|---|---|
| `x = pos.x` | the value under the name `x` |
| `profile` | the variable `profile` under its own name |
| `kind = ?kind` | the `Debug` text of anything, for a value that is not a number, text or boolean |

Other macros and helpers, all in the `diag` crate:

| Use | For |
|---|---|
| `diag::trace!(..)` | events in the hundreds (a pointer move); written only with `trace` in the switch |
| `diag::once!(..)` | "this branch ran": written the first time the line is reached |
| `let _t = diag::timed!("ui", "load", n = 3);` | how long a block took: the line, with `ms`, is written when `_t` is dropped |
| `diag::event(cat, ev).field(name, v).write()` | field names that are built at run time (`format!("mon{i}")`) |
| `diag::enabled("sys")` | a guard in front of work that only the log needs |
| `diag::Watch` | what was asked for against what happened, for a short time afterwards (see `src/debug.rs`: the window position) |

Rules:

- The values are evaluated **only while the log is on**, so an argument may be expensive (a call to the X
  server, a `format!`). Do not put a side effect in one.
- Values are numbers, strings, booleans, paths, durations and `Option` of those. The log is flat on purpose; the
  analyzer does not read nesting. Anything else goes through `?` and becomes one text.
- Category = the layer (`tap`, `ui`, `x11`, `win`, `sys`), event = what happened, in `snake_case`. A field named
  `t`, `th`, `cat` or `ev` is renamed with a leading underscore.
- Log the input and the decision of a layer side by side, so a reader sees where two numbers that should agree
  stop agreeing.
- Never log the user's data: file names, typed text, the content of the config.
- To teach the analyzer a new check, add a branch in `analyze` of `tools/src/diag.rs` (or in `extra_findings` of
  `tools/src/logview.rs`) and a test with a few sample lines, and list the events and fields it reads in `EXPECTED`
  (`tools/src/diag.rs`).
- `cargo tools diag-lint` checks all of this against the source: category in the table above, `snake_case` names, no
  field named like the user's data (`path`, `file`, `text`, `cmd`, `url`, ...), no field written twice, and that every
  event in `EXPECTED` is still written by a line in `src/` with its fields. It is a quick step of `cargo tools ci`
  (so the `pre-commit` hook runs it) and a unit test, so renaming an event without teaching the analyzer fails
  instead of making the analysis silently blind. A new category goes in `CATEGORIES` (`tools/src/lint.rs`) and in the
  table above.

The full reference (every function, the switch, the line format, how to test code that logs) is the documentation
of the crate: `cargo docs`, crate `cursorpie_diag`. Another program of the workspace can use the same log with
`diag::Builder::new("name")...init()`; the environment variables and the file names follow the name.

## The touchpad does nothing (Linux)

`cursorpie --touchpad-debug [seconds]` (default 20) prints why, without starting the app (stop a running
CursorPie first). It checks, in order, and marks each line `[ok]`, `[..]` (information) or `[!!]` (a problem):

1. the session (`XDG_SESSION_TYPE`, `DISPLAY`, `WAYLAND_DISPLAY`) and the saved `touch_trigger` (the switch must be
   on **and saved**);
2. the groups of this session (`input`), and whether `/etc/group` lists you in it before you logged in again;
3. every device of `/proc/bus/input/devices` that has anything to do with touch, and why it was taken as a touchpad or
   left out (a touchscreen, a virtual device, no finger count, no finger positions);
4. for each touchpad: whether `/dev/input/eventN` can be opened (permission), and the size of the surface in mm;
5. live: each change of the fingers with their position in mm and, as `-> recognizer:`, what the hold logic decides
   (`HoldArmed`, `HoldOpen`, `Release`, `Cancel(..)`) with your config (hold time, how still, typing guard).

It ends with `[->]` lines naming the broken link. The positions are printed to the terminal only because you ran
it; the app never logs them. If this says everything works but the pie does not open, run the app with
`CURSORPIE_DEBUG=1` and read the `touch` and `tap` lines (`touch`: `devices`, `device_open`, `arm`, `open`, `cancel`,
`x_grabs`).

## Facts for a bug report

Linux touchpad: paste the output of `cursorpie --touchpad-debug` as well.

`cargo tools info` prints the OS, the desktop session variables, the tool versions and the number of logs, as a
block to paste into the issue.

## A bug report that needs it

Ask for: the desktop and session (`echo $XDG_SESSION_TYPE $XDG_CURRENT_DESKTOP`), and the log of a run in which
the gesture was made two or three times at different places on the screen:

```
CURSORPIE_DEBUG=1 cursorpie 2> stderr.txt
cargo tools diag
```

## Trying the trackpad gestures without a trackpad

`cargo tools trackpad` builds the app with the `vtrackpad` feature and starts it with a *virtual trackpad*: a trackpad that floats exactly in the middle of the
screen. Your real pointer is the finger on it (over the glass it touches, off the glass it is lifted), and a virtual
pointer, an arrow drawn on the screen outside the trackpad, is what the finger moves, relatively and with acceleration
like a real trackpad does. The pie opens at the virtual pointer, aims with it and acts on the app under it.

It feeds the same recognizer as a real trackpad, so the whole chain (hold, aim, lift, extra click, the palm and edge
rules) is the real one, and `CURSORPIE_DEBUG=touch` logs it as it logs a real trackpad (the device id is `0x56495254`).
The left and right buttons of the mouse are the primary and the secondary click of the trackpad: the click is made at
the virtual pointer when the button comes up, so the app under the arrow gets it (a double click too). While the pie is
open, a click is the pie's extra click and goes nowhere else. Holding a button and dragging is not made yet.
Nothing is written to the settings; the red dot closes the trackpad, and the app keeps running. The app allows one
copy at a time: if another CursorPie is running (the installed one, or another `cargo run`), quit it first, or the
tool says so and stops.

| Option | Meaning |
|---|---|
| `--width POINTS` | width of the trackpad, 240 to 640 (default 360); the slider in its lower frame changes it too |
| `--silent` | no click sound (the switch in the lower frame does the same) |
| `--release` | build the app with optimizations |

The tool is behind the Cargo feature `vtrackpad`: an ordinary build, debug or release, has none of its code (and
ignores `CURSORPIE_VTRACKPAD`). `cargo tools ci` checks it with the feature on. Behind the command is the environment
the app reads (in a build that has the feature): `CURSORPIE_VTRACKPAD=1` (turns the tool on),
`CURSORPIE_VTRACKPAD_WIDTH` and `CURSORPIE_VTRACKPAD_SOUND=0`. The haptic click of a real trackpad cannot be given by a
mouse; the tool shows it (the surface sinks, a ring, a glow) and plays a click, and the tick is also sent to the
system's haptic engine, which only a finger on a real trackpad feels.
