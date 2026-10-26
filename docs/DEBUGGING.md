# Debugging with the diagnostic log

CursorPie can write a trace of what it does, so a problem that only happens on one machine ("the pie opens in
the middle of the screen, not under the pointer") can be found without a debugger and without editing the code.
The log is **off by default** and costs one atomic read per log call while it is off.

The code is `src/diag.rs`; the analyzer is `cargo tools diag` (`tools/src/diag.rs`).

## Turn it on

| Way | When |
|---|---|
| `CURSORPIE_DEBUG=1 cursorpie` | one run, from a terminal |
| an empty file named `debug` next to `config.json` | a copy that starts at login (remove the file to turn it off) |

The value is a list: `1` or `all` (every category), `tap,ui` (only those), `trace` (also the events that come in
the hundreds, such as every pointer move of a drag), `0` or `off` (off). A set variable wins over the file, so
`CURSORPIE_DEBUG=0` silences a `debug` file for one run.

| Category | What it records |
|---|---|
| `diag` | the first line: version, OS, session variables (`XDG_SESSION_TYPE`, `DISPLAY`, ...) |
| `sys` | the monitors and the pointer as the platform reports them at start |
| `x11` | the display scale chosen for the pointer coordinates |
| `tap` | the session type, the button grabs, every press and release (position from the event **and** from the X server), the verdict, the hold timer |
| `ui` | the message that opens the pie, the position the pie asks for, the monitor it picked, the cursor fallback |
| `win` | where the toolkit says the window really is, for two seconds after a window was placed |
| `panic` | any panic, with its location |

## Where the log goes

One file per run in the `logs` folder of the config folder, `cursorpie-<unix seconds>-<pid>.jsonl`
(`~/.config/cursorpie/logs` on Linux, `~/Library/Application Support/CursorPie/logs` on macOS,
`%APPDATA%\CursorPie\logs` on Windows). `CURSORPIE_DEBUG_DIR=/some/folder` moves it. The app prints the path on
stderr when it opens the file. Only the newest 10 files are kept and a file stops growing at 8 MiB. Every line is
written at once, so the log is complete after a crash.

Every line is one flat JSON object: `t` (seconds since start), `th` (thread), `cat`, `ev` and the fields of the
event.

```
{"cat":"ui","ev":"show_pie","left":650.0,"mon":"0,0 1920x1080","t":5.3,"th":"main","top":350.0,"x":800.0,"y":500.0}
```

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

## Add a log line

```rust
crate::diag_log!("ui", "show_pie", x = x, y = y, profile = profile);
crate::diag_trace!("tap", "drag", x = pos.x, y = pos.y); // only with `trace`
```

- The values are evaluated **only while the log is on**, so an argument may be expensive (a call to the X
  server, a `format!`). Do not put a side effect in one.
- Use numbers, strings and booleans only. The log is flat on purpose; the analyzer does not read nesting. Wrap
  anything else with `format!("{v:?}")`.
- Category = the layer (`tap`, `ui`, `x11`, `win`, `sys`), event = what happened, in `snake_case`. A field named
  `t`, `th`, `cat` or `ev` is renamed with a leading underscore.
- Never log the user's data: file names, typed text, the content of the config.
- To teach the analyzer a new check, add a branch in `analyze` of `tools/src/diag.rs` and a test with a few
  sample lines.

## A bug report that needs it

Ask for: the desktop and session (`echo $XDG_SESSION_TYPE $XDG_CURRENT_DESKTOP`), and the log of a run in which
the gesture was made two or three times at different places on the screen:

```
CURSORPIE_DEBUG=1 cursorpie 2> stderr.txt
cargo tools diag
```
