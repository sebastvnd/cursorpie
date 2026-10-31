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
| `sys` | the monitors and the pointer as the platform reports them at start |
| `x11` | the display scale chosen for the pointer coordinates |
| `tap` | the session type, the button grabs, every press and release (position from the event **and** from the X server), the verdict, the hold timer (`hold_open`, or `hold_cancelled` when it gave up), the double-click window, which profile a press resolved to (`resolve`: whether a target app was found, never which), the slice that was selected, the side buttons, and whether the input hook runs (`hook`: `running`, or why not: `blocked` on macOS, `refused` on Windows, `no_x_server` or `grab_failed` on Linux) |
| `ui` | the message that opens the pie, the position the pie asks for, the monitor it picked, the cursor fallback; closing the pie, opening and closing Settings (`close_settings` is timed: saving), importing a config, and the timed `warm` (icon loading) and `sleep` (freeing memory) |
| `win` | where the toolkit says the window really is, for two seconds after a window was placed |
| `cfg` | loading `config.json` (outcome, size, time) and every save of it (`ok`, and the error kind when it failed: a failed save is otherwise silent) |
| `act` | every action the worker runs: its kind (never its content), how long it waited in the queue (`wait_ms`) and how long it ran (`ms`); every program it starts (`exited` with its exit code, `spawn_failed` with the error kind, on Windows `shell_execute` with the code), by program name only, never its arguments |
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

## Facts for a bug report

`cargo tools info` prints the OS, the desktop session variables, the tool versions and the number of logs, as a
block to paste into the issue.

## A bug report that needs it

Ask for: the desktop and session (`echo $XDG_SESSION_TYPE $XDG_CURRENT_DESKTOP`), and the log of a run in which
the gesture was made two or three times at different places on the screen:

```
CURSORPIE_DEBUG=1 cursorpie 2> stderr.txt
cargo tools diag
```
