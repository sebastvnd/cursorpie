# Writing flows as JSON (a reference for AI agents)

This page is written so that an AI agent (or a person scripting) can produce a **valid CursorPie flow** as JSON
without opening the app. It is complete on its own; the sources it is checked against are
`src/config/flow.rs` (the model, the repair rules, the file format) and `src/actions.rs` (what running a flow does).
The examples in [`examples/flows/`](examples/flows) are parsed by the unit tests, so they cannot drift from the code,
and [`flow.schema.json`](flow.schema.json) is a JSON Schema of the file for tools that can validate against one.

If you only read one section, read [The recipe](#the-recipe) and [The rules that make a flow wrong](#the-rules-that-make-a-flow-wrong).

## What a flow is

A **flow** is a small program that a pie slice runs: press a shortcut, open an app, open a URL or file, run a shell
command, wait, and also keep **variables**, **decide** (if / else), **loop** (repeat, while), **ask the person** for a
text or a yes / no, **call another flow**, and use the **system** (clipboard, notifications, files, the app in
front). It is drawn in the **Flow** tab of Settings as blocks joined by wires.

- It is a **graph**: `Start -> ... -> End`. A block has one input (any number of wires may lead into it, so two
  paths can join) and one output `next`; the blocks that decide have a second output `alt`.
- A wire may lead **back** to an earlier block. That is a loop. A run is limited (see
  [How a flow runs](#how-a-flow-runs)), so a loop that never ends is stopped.
- There is **no nested block and no expression language**: a condition compares two values, a calculation does one
  operation. Do not invent block kinds; the app refuses a file with an unknown kind.
- The app runs the graph from `Start` along the wires. A block that no path from `Start` reaches is **ignored**.

## Two places a flow can live

| Where | Shape | Use it when |
|---|---|---|
| **A flow file**, `<name>.cursorpie-flow.json` | `{ "cursorpie_flow": 1, "flow": { ... } }` | You hand the person a file. They press **Import** (arrow down) in the Flow tab. The flow is added as a **new** flow (new id, unique name); nothing of theirs is overwritten. **This is the safe default.** |
| **`flows` in `config.json`** | the top-level array `"flows": [ { ... }, ... ]` of the config, each item is the same `flow` object | You write the whole config (see [Attaching a flow to a slice](#attaching-a-flow-to-a-slice)). Read the warning there first. |

The `flow` object is the same in both places.

## The format

```jsonc
{
  "cursorpie_flow": 1,                 // file format version: 1, or 2 when the flow uses the blocks of "The logic blocks" and below
  "flow": {
    "id": "copy-switch-paste",         // string, not empty, unique among flows; replaced with a fresh id on import
    "name": "Copy, switch, paste",     // string; trimmed, cut to 60 characters; made unique ("Name (2)") on import
    "nodes": [                         // 2 to 128 blocks
      { "id": 1, "x": 40.0, "y": 60.0, "kind": "Start", "next": 2 },
      { "id": 2, "x": 240.0, "y": 60.0, "kind": { "Key": { "key": 8, "mods": 8 } }, "next": 3 },
      { "id": 3, "x": 540.0, "y": 60.0, "kind": "End", "next": null }
    ]
  }
}
```

### A block (a "node")

| Field | Type | Meaning |
|---|---|---|
| `id` | integer, 0 to 4294967295 | Unique **inside the flow**. Use 1, 2, 3, ... |
| `x`, `y` | number | Where the block sits in the editor (top-left corner). **Cosmetic**: they decide nothing about what runs. Any finite value within +-20000 is kept; outside that it is clamped. See [Placing blocks](#placing-blocks). |
| `kind` | string or object | What the block does. See the next sections. |
| `next` | integer or `null` | The `id` of the block the first output leads to. `null` or absent = no wire: **the run ends there**. |
| `alt` | integer or `null` | The second output. Only `Branch`, `Repeat` and `While` have one; on any other block it is removed. Absent = no wire. |

Serde's default external tagging is used: a block without data is a **bare string** (`"Start"`), a block with data is
an **object with one key** (`{ "Wait": 500 }`, `{ "Set": { "name": "n", "value": "1" } }`). A field of a block
that is written as an object may be left out; it then has its default (empty text, the first operation).

The file number `cursorpie_flow` is **1** when a flow only uses the first seven kinds and no `alt` (an older CursorPie
opens it too) and **2** otherwise. Write 2 for any flow with the blocks below; 3 or more is refused.

### The first seven kinds

| `kind` | JSON | What it does when it runs | Notes |
|---|---|---|---|
| Start | `"Start"` | nothing; the run begins here | exactly **one**; nothing may be wired into it |
| End | `"End"` | nothing; the run (or the called flow) ends here | exactly **one**; no outputs; it needs an input wire |
| Key | `{ "Key": { "key": 8, "mods": 8 } }` | presses the shortcut | see [Shortcuts](#shortcuts) |
| OpenApp | `{ "OpenApp": "Notes" }` | opens or raises an app | name or path, see [Apps, URLs, files](#apps-urls-and-files) |
| OpenUrl | `{ "OpenUrl": "https://example.com" }` | opens a URL or a file with the system default | |
| Command | `{ "Command": "echo hi" }` | runs a shell command line **without waiting** | read [Command blocks](#command-blocks-and-safety) |
| Wait | `{ "Wait": 500 }` | pauses, in **milliseconds** | 0 to 10000; more is cut to 10000 |

Empty `OpenApp`, `OpenUrl` and `Command` strings (or only spaces) do nothing.

### Variables

A run has variables: a **name** (letters, digits and `_`, at most 32, not starting with a digit) with a **text** as
value (at most 16384 characters; at most 128 variables). They start empty and are gone when the run ends.

- In **any text field** (`OpenApp`, `OpenUrl`, `Command`, `Set.value`, `Ask.message`, `Run.command`, the texts of
  `Native`...) `{{name}}` is replaced by the value. `{{name|sh}}` is replaced by the value **quoted for the shell**
  (single quotes on macOS and Linux; on Windows double quotes with `"`, `%` and line breaks dropped): use it inside a
  `Command` or `Run`. Only a **defined** variable is replaced, so a command that contains `{{x}}` for another reason
  is left as it is.
- The **operands** of `Calc`, `Logic`, `Branch` and `While` are a value (`3`, `yes`, `{{x}}`) or the bare **name of a
  variable** (`n`). A bare name that is defined means its value.
- A field that **receives** a result (`out`, `name`, and the `variable` fields of `Native`) takes a bare variable name,
  without braces.
- **Truth:** a text is false when it is empty, `0`, `false`, `no` or `off` (any case, spaces ignored), and true
  otherwise. Gates and `Calc` write `1` and `0`.
- **Numbers:** a text is a number when it parses as one (`3`, `-2.5`); whole results are written without a decimal point.
- Set by the blocks themselves: `ok` (`1` when the last block that can fail worked: `Calc`, `Ask`, `Run`, `Native`), `code` (the
  exit code of the last `Run`; `-1` when it could not start, was ended by the time limit or a signal) and `index` (the
  round of the innermost `Repeat` or `While`, counting from 1).

### The logic blocks

| `kind` | JSON | Behaviour |
|---|---|---|
| Set | `{ "Set": { "name": "n", "value": "{{n}}!" } }` | puts the value (variables filled in) in `n` |
| Calc | `{ "Calc": { "out": "i", "a": "i", "op": "Add", "b": "1" } }` | `out = a op b` as numbers. `op`: `Add`, `Sub`, `Mul`, `Div`, `Rem`, `Min`, `Max`. A text that is not a number, or a division by zero, leaves `out` empty and `ok` at `0` |
| Logic | `{ "Logic": { "out": "g", "a": "x", "op": "And", "b": "y" } }` | `out = a gate b` as `1` or `0`. `op`: `And`, `Or`, `Xor`, `Nand`, `Nor`, `Not` (`Not` ignores `b`) |
| Branch | `{ "Branch": { "a": "n", "op": "Gt", "b": "3" } }` | follows **`next` when the condition holds, `alt` when it does not** |
| Repeat | `{ "Repeat": 3 }` | goes through **`next` (the body)** 3 times (0 to 1000), then follows **`alt` (done)** |
| While | `{ "While": { "a": "i", "op": "Lt", "b": "n" } }` | goes through `next` (the body) while the condition holds (at most 1000 rounds), then follows `alt` |
| Call | `{ "Call": "other-flow-id" }` | runs the flow with that id (same variables), then goes on. Depth at most 8; a missing flow is skipped |
| Stop | `"Stop"` | ends the **whole run**, also inside a called flow. No outputs |

`op` of a condition (`Branch`, `While`): `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge` (two numbers are compared as numbers,
anything else as text), `Contains`, `StartsWith`, `EndsWith`, `IsTrue` (only `a`), `IsEmpty` (only `a`).

**A loop is a wire back.** The body of a `Repeat` or `While` is the chain that starts at `next`; its last block must be
wired **back to the loop block itself**. `alt` leads on. See `examples/flows/ask-and-loop.cursorpie-flow.json`.

### The blocks that ask and the blocks that use the system

| `kind` | JSON | Behaviour |
|---|---|---|
| Ask | `{ "Ask": { "mode": "Text", "message": "Name?", "preset": "Bob", "out": "name" } }` | shows a window of the system and waits. `mode`: `Text` (a box; `preset` is what it starts with) or `YesNo` (`out` becomes `1` or `0`). `ok` is `0` when it was cancelled or no window program exists (Linux needs `zenity` or `kdialog`); on Windows an empty answer counts as cancelled |
| Run | `{ "Run": { "command": "git rev-parse HEAD", "out": "rev", "timeout_s": 10 } }` | runs a shell command and **waits** (1 to 120 s, then it is ended). `out` gets what it printed (trailing line breaks cut, at most 256 KB), `code` its exit code. Every variable is also in its environment as `CURSORPIE_<NAME>` |
| Native | `{ "Native": { "op": "Notify", "a": "Title", "b": "Text" } }` | something the system does or tells; `op` decides what `a` and `b` are for (table below) |

`Native` operations (`a` and `b` are text; a field written as *variable* is a bare variable name):

| `op` | `a` | `b` |
|---|---|---|
| `ClipboardSet` | text to copy | |
| `ClipboardGet` | | variable |
| `TypeText` | text to type (pasted into the app in front; the clipboard is put back) | |
| `Notify` | title | text |
| `ReadFile` | path (UTF-8 text, at most 256 KB; `~/` is the home folder) | variable |
| `WriteFile` | path (replaces the file) | text |
| `AppendFile` | path | text |
| `FileExists` | path | variable (`1` or `0`) |
| `Env` | name of an environment variable | variable |
| `FrontApp` | | variable (the id of the app in front) |
| `ActivateApp` | app id | |
| `AppRunning` | app | variable (`1` or `0`) |
| `CursorPos` | variable for x | variable for y |
| `Random` | number: the result is below it | variable |
| `UnixTime` | | variable (seconds since 1970 UTC) |
| `Reveal` | path | |

The clipboard needs `pbcopy` / PowerShell / `wl-copy`, `xclip` or `xsel`; notifications need `osascript` / PowerShell /
`notify-send`. A missing tool makes `ok` `0`; nothing else happens. There is no block that moves or clicks the mouse
and none that makes a network request.

## Shortcuts

`Key` takes `{ "key": <code>, "mods": <bit mask> }` (an optional `"order"` exists; leave it out).

**`key` is a macOS virtual key code: a physical key position, not a character.** The same codes are used on every OS
(Windows and Linux translate them to the key in the same physical position, so a shortcut written with these codes
works there too). Do not put a character or a key name in `key`; it must be an integer.

**`mods` is a sum of bits:**

| Modifier | Bit | On Windows / Linux |
|---|---|---|
| Shift | 1 | Shift |
| Ctrl | 2 | Ctrl |
| Alt / Option | 4 | Alt |
| Cmd | 8 | the **Windows key** / Super |

Add the bits of the modifiers that are held: Cmd+Shift = 8 + 1 = `9`; Ctrl+Alt = 2 + 4 = `6`; no modifier = `0`.

> **Which OS is it for?** The "copy" shortcut is Cmd+C (`key` 8, `mods` 8) on a Mac but Ctrl+C (`key` 8, `mods` 2)
> on Windows and Linux. The key code is the same; the modifier differs. **Ask which OS the flow is for, or write
> one file per OS.** Never assume.

### Key codes (US/ANSI positions)

Letters: A 0, B 11, C 8, D 2, E 14, F 3, G 5, H 4, I 34, J 38, K 40, L 37, M 46, N 45, O 31, P 35, Q 12, R 15,
S 1, T 17, U 32, V 9, W 13, X 7, Y 16, Z 6.

Digits (top row): 1 18, 2 19, 3 20, 4 21, 5 23, 6 22, 7 26, 8 28, 9 25, 0 29.

Punctuation: `-` 27, `=` 24, `[` 33, `]` 30, `\` 42, `;` 41, `'` 39, `,` 43, `.` 47, `/` 44, `` ` `` 50.

Editing and navigation: Return 36, Tab 48, Space 49, Backspace 51, Esc 53, Delete (forward) 117, Home 115, End 119,
Page Up 116, Page Down 121, Left 123, Right 124, Down 125, Up 126.

Function keys: F1 122, F2 120, F3 99, F4 118, F5 96, F6 97, F7 98, F8 100, F9 101, F10 109, F11 103, F12 111,
F13 105, F14 107, F15 113, F16 106, F17 64, F18 79, F19 80, F20 90.

Media: Volume Up 72, Volume Down 73, Mute 74.

The codes name **positions**: on a non-US layout the key at position 6 may print another letter. The table is the US
one; if the person's layout differs and the shortcut must be the one *printed* on a key, say so in your answer.

Common shortcuts, macOS (`mods` 8 is Cmd): copy `{"key":8,"mods":8}`, paste `{"key":9,"mods":8}`, cut
`{"key":7,"mods":8}`, undo `{"key":6,"mods":8}`, select all `{"key":0,"mods":8}`, new tab `{"key":17,"mods":8}`, close
window `{"key":13,"mods":8}`, switch app `{"key":48,"mods":8}`. The same on Windows/Linux: replace `"mods":8` with `"mods":2`.

## Apps, URLs and files

- `OpenApp` takes what an **Open app** slice takes. macOS: an app name (`Safari`) or a path
  (`/Applications/X.app`). Windows: an executable name (`chrome`) or a full path (`C:\\Path\\App.exe`; in JSON every
  backslash is doubled). Linux: a command (`firefox`) or a path (`/usr/bin/app`).
- `OpenUrl` takes a URL (`https://...`, `mailto:...`) or a file path, and opens it with the system default.
- A block's text is plain: no variables, no `~` expansion promised, no environment substitution. Use full paths for
  files. (Shell expansion such as `$HOME` works only inside a `Command`, because that goes through a shell.)

## Command blocks and safety

A `Command` runs a shell command line **with the person's rights**. That makes it powerful and the one block that
needs care:

- Use it only when no other block does the job (typing text, calling a CLI, moving files). Say in your answer
  that the flow contains a command and **show the exact command**.
- Never put a secret, token or password in a flow: flows are exported and shared as plain text.
- Never generate a destructive command (`rm -rf`, disk, format, `curl ... | sh`) unless the person asked for exactly
  that and understood it.
- The app protects the receiver: after an import it warns when any block, wired or not, holds a command, and
  nothing runs until the person saves and uses the slice. Do not try to hide a command in an unwired block.
- Quote for the person's shell and mind JSON escaping: a `"` inside the command is `\"` in the JSON.
- **The command is started and not waited for**: it runs detached, and the next step follows 40 ms later whether
  or not the command has finished. If a later step depends on its result, add a `Wait`.
- The shell is `/bin/sh -c` on macOS and Linux and `cmd.exe` (hidden window) on Windows, so write the line for the
  target OS: `$HOME`, `>>` to `$HOME/x`, `date` and `open -a` are Unix; `%USERPROFILE%` and `echo %DATE%` are Windows.

## How a flow runs

1. The slice is released (or its mode button pressed) and the app copies the flow out of the config.
2. The run is on a worker thread. It starts at the block `Start` leads to and goes block by block along the wires.
   After a block that *does* something (`Key`, `OpenApp`, `OpenUrl`, `Command`, `Wait`, `Run`, `Native`) it pauses
   **40 ms**; blocks that only compute or decide do not pause.
3. It ends at `End`, at `Stop`, at an output without a wire, after **10000 blocks**, or after **600 seconds**
   (time spent waiting for an answer in `Ask` does not count). A loop is also limited to 1000 rounds.
4. If the slice points at a flow id that does not exist, nothing happens.

The worker does one thing at a time, so a long run keeps later actions waiting: keep flows short. Give an app time to
come to the front before a shortcut is sent to it: **put a `Wait` (300 to 800 ms) between `OpenApp` / `OpenUrl` and
a following `Key`.** A `Command` is started and **not waited for**; use `Run` when a later step needs its result.

## The recipe

To turn a request such as "copy the selection, open Notes, make a new note, paste" into JSON:

1. **Fix the target OS** (it decides `mods`, app names, and commands). Ask if unknown.
2. **Break the request into steps** and pick one kind per step from the kinds above. Insert a `Wait` after every
   step that opens something (`OpenApp`, `OpenUrl`, a shortcut that opens a window) and before the step that depends
   on it.
3. **Number the blocks** 1..n: `Start` is 1, the steps are 2..n-1, `End` is n.
4. **Wire them in order:** each block's `next` is the id of the following block; `End`'s `next` is `null`.
5. **Place them** (cosmetic, see [Placing blocks](#placing-blocks)).
6. **Wrap** in `{ "cursorpie_flow": 1, "flow": { "id", "name", "nodes" } }`. Pick a short lower-case `id`
   (`copy-to-notes`) and a readable `name`.
7. **Check** with [the checklist](#the-checklist-before-you-answer) and, when you can run code, with the validator
   snippet below.
8. **Tell the person** to save the file as `<name>.cursorpie-flow.json`, open **Settings > Flow**, press **Import**
   (the arrow down), then in **Menu** set a slice's action to **Flow** and pick it, then **Save**.

### Placing blocks

`x` and `y` only position the block in the editor, so any layout works; the editor's own layout is easy to copy and
looks tidy:

- `Start` at `(40, 60)`. A block is **250** wide, `Start` and `End` are **150** wide, and the gap between blocks is **50**.
- Each next block goes at `x = previous.x + previous.width + 50`, same `y`.
- If `x + 250 > 900` the row is full: continue at `x = 40`, `y = previous.y + 130`.
- `End` follows the same rule, as the last block.

(The widths are only for spacing. A simple alternative that is also fine: a row of blocks at `x = 40 + 300*i`, `y = 60`.)

## The rules that make a flow wrong

The app repairs many mistakes silently when it loads a flow, but **a repair is not what you meant**, so do not rely
on it. Note especially the ones that change behaviour:

| Rule | If you break it |
|---|---|
| Exactly one `"Start"` and one `"End"` | A duplicate is dropped. **A missing `Start` is added with no wire, so nothing runs.** A missing `End` is added after the last block of the `next` chain. |
| Nothing is wired into `Start` | A wire into `Start` is removed. |
| Every `next` and `alt` names an existing block id | A wire to a missing block is removed; the run ends there. |
| A block does not point at itself | The wire is removed. |
| `End` and `Stop` have no outputs; only `Branch`, `Repeat` and `While` have `alt` | The extra wire is removed. |
| Block `id`s unique | Duplicates are dropped (the later one). |
| At most 128 blocks (32 flows per config) | The extra blocks are cut off the end of the list. |
| `Wait` is 0 to 10000, `Repeat` 0 to 1000, `Run.timeout_s` 1 to 120, a text field at most 4000 characters, a variable name as described above | Cut or cleaned to the limit. A negative or fractional number is a wrong shape: file refused. |
| Block kinds, `op` and `mode` values are exactly the ones in this page, spelled with that capitalisation | **An unknown one refuses the whole file** ("That flow file is damaged."). |
| `cursorpie_flow` is present and is 1 or 2 | Missing: "not a CursorPie flow". Greater than 2: "made by a newer CursorPie". |
| The file is valid JSON, UTF-8, at most 1 MiB | Refused. No comments, no trailing commas (the `jsonc` in this page is for reading only). |

Several wires into one block and wires back to an earlier block are **allowed**. A wrong **type** (a string where a
number goes, an array for `next`) refuses the file; unknown extra fields are ignored.

## Attaching a flow to a slice

A flow does nothing until a slice runs it. In the app the person picks **Flow** as the action of a slice. In
`config.json` the action is `{ "Flow": "<flow id>" }`:

```json
{ "name": "Morning", "icon": "☀", "kind": { "Run": { "Flow": "morning" } } }
```

`Run` means "run it when the button is released over the slice". A flow can also be the action of a `Mode` slice's
`back`, `forward` or `extra` buttons (`{ "Flow": "id" }` anywhere an action goes). The string must equal the `id` of a
flow in the same config's `flows` array. A complete minimal config that does this is
[`examples/flows/config-with-flow.json`](examples/flows/config-with-flow.json) (the `version` key is required, any
other missing setting takes its default).

> **Do not edit the person's `config.json` while CursorPie is running.** The app reads it once at start and
> overwrites it on **Save**, so a change made behind its back is lost or fights with Settings. **Prefer the flow
> file and Import.** If you must write `config.json`, tell the person to quit CursorPie first, keep their existing
> content (read the file, add to `flows`, never replace it), and expect the app to repair whatever is out of range.
> A profile file (`cursorpie_profile`, see [CONFIGURATION.md](CONFIGURATION.md)) also carries `"flows": [...]` next to
> the profile; the same flow object applies there.

## Examples

All of these are real files in [`examples/flows/`](examples/flows) and are checked by `cargo test`.

**1. Open an app, wait, press a shortcut** (macOS): `open-app-and-new-note.cursorpie-flow.json`

```json
{
  "cursorpie_flow": 1,
  "flow": {
    "id": "open-app-and-new-note",
    "name": "Open Notes and start a note",
    "nodes": [
      { "id": 1, "x": 40.0,  "y": 60.0,  "kind": "Start", "next": 2 },
      { "id": 2, "x": 240.0, "y": 60.0,  "kind": { "OpenApp": "Notes" }, "next": 3 },
      { "id": 3, "x": 540.0, "y": 60.0,  "kind": { "Wait": 600 }, "next": 4 },
      { "id": 4, "x": 840.0, "y": 60.0,  "kind": { "Key": { "key": 45, "mods": 8 } }, "next": 5 },
      { "id": 5, "x": 40.0,  "y": 190.0, "kind": "End", "next": null }
    ]
  }
}
```

Chain: `Start(1) -> OpenApp(2) -> Wait(3) -> Key Cmd+N(4) -> End(5)`. Note the row wrap at block 5 and the `Wait`
between opening the app and the shortcut.

**2. Copy, switch window, paste** (macOS, Cmd = 8): `copy-switch-paste.cursorpie-flow.json`. Six steps in two
rows: `Key Cmd+C`, `Wait 150`, `Key Cmd+Tab` (`key` 48), `Wait 250`, `Key Cmd+V`. For Windows/Linux change each
`"mods": 8` to `"mods": 2`, and Cmd+Tab to `{"key":48,"mods":4}` (Alt+Tab).

**3. A link, a wait and a command** (macOS/Linux shell): `link-wait-command.cursorpie-flow.json`. Shows `OpenUrl`,
`Wait` and a `Command` with an escaped quote:

```json
{ "id": 4, "x": 40.0, "y": 190.0, "kind": { "Command": "date >> \"$HOME/cursorpie-flow.log\"" }, "next": 5 }
```

**4. Inside a config**: `config-with-flow.json` has a `flows` array and a slice whose action is
`{ "Flow": "morning" }`.

**5. Ask, decide, loop, use the system** (any OS): `ask-and-loop.cursorpie-flow.json`. Asks "How many times?"
(`Ask`, answer in `n`), stops at once when the question was cancelled (`Branch` on `ok`, its `alt` leads to `End`),
then counts `i` from 0 with a `While` whose body is a notification (`Native` `Notify`, with `{{index}}` and `{{n}}`)
and `Calc` `i = i + 1` wired back to the `While`; when it is done a last notification says so.

## The checklist before you answer

- [ ] The JSON parses (no comments, no trailing commas, double quotes, escaped backslashes and quotes).
- [ ] Top level is `{ "cursorpie_flow": 1 or 2, "flow": { "id", "name", "nodes" } }`; 2 whenever a block of the logic,
      ask or system sections or an `alt` is used.
- [ ] Exactly one `"Start"` and one `"End"`; `End` has no `next`.
- [ ] Every block has `id`, `x`, `y`, `kind`; ids are unique integers.
- [ ] Every `next` and `alt` names an existing block; only `Branch`, `Repeat` and `While` have an `alt`.
- [ ] Every block is reachable from `Start` (following `next` and `alt`); every path ends at `End`, at `Stop` or at a
      deliberate missing wire.
- [ ] A `Repeat` or `While` has its body on `next`, the last block of the body wired **back to the loop block**, and
      what follows on `alt`. A `While` changes something its condition reads.
- [ ] Each variable is **written before it is read**, and each result field holds a bare name (no braces).
- [ ] `Key.key` is a number from the table; `mods` is the sum of bits for the **target OS**.
- [ ] A `Wait` sits between a step that opens something and the step that needs it.
- [ ] If there is a `Command`, a `Run` or a block that writes a file, you have said so and quoted it for the person;
      text from a variable inside a shell command uses `{{name|sh}}` (or the environment variable of `Run`).
- [ ] The answer tells the person the file name and the import steps.

When you can run code, this check is short (Python):

```python
import json, sys
f = json.load(open(sys.argv[1]))
assert f["cursorpie_flow"] in (1, 2)
nodes = f["flow"]["nodes"]; by_id = {n["id"]: n for n in nodes}
assert len(by_id) == len(nodes) <= 128, "ids must be unique, at most 128 blocks"
name = lambda k: k if isinstance(k, str) else next(iter(k))
kinds = [name(n["kind"]) for n in nodes]
assert kinds.count("Start") == 1 and kinds.count("End") == 1
assert set(kinds) <= {"Start","End","Key","OpenApp","OpenUrl","Command","Wait","Set","Calc","Logic",
                      "Branch","Repeat","While","Ask","Run","Native","Call","Stop"}
two_out = {"Branch", "Repeat", "While"}
no_out = {"End", "Stop"}
for n in nodes:
    k = name(n["kind"])
    outs = [n.get("next"), n.get("alt")]
    assert all(t is None or t in by_id for t in outs), f"block {n['id']}: a wire to nothing"
    assert n.get("alt") is None or k in two_out, f"block {n['id']}: only Branch, Repeat, While have alt"
    assert not (k in no_out and n.get("next") is not None), f"block {n['id']}: no outputs"
    assert all(t != 1 for t in outs), "nothing goes into Start"
seen, todo = [], [next(n["id"] for n in nodes if n["kind"] == "Start")]
while todo:
    i = todo.pop()
    if i in seen: continue
    seen.append(i)
    todo += [t for t in (by_id[i].get("next"), by_id[i].get("alt")) if t is not None]
assert len(seen) == len(nodes), "a block is not reachable from Start"
for n in nodes:
    k = n["kind"]
    if isinstance(k, dict) and "Wait" in k: assert 0 <= k["Wait"] <= 10000
    if isinstance(k, dict) and "Repeat" in k: assert 0 <= k["Repeat"] <= 1000
    if isinstance(k, dict) and "Key" in k: assert 0 <= k["Key"]["mods"] <= 15
print("ok:", len(seen), "blocks reachable")
```

## Answering well

- Output the JSON in **one code block**, then the file name and the import steps. Do not add commentary inside the JSON.
- Name the target OS you assumed, and every assumption about app names and key positions.
- Keep flows short and every `Wait` purposeful. Prefer several small flows over one with 40 blocks.
- If the request needs something a flow cannot do (reading the screen, moving or clicking the mouse, a network
  request, a loop over the lines of a file), say so instead of faking it with extra block kinds.
- Explain every loop and every branch in one sentence ("the body repeats until `i` reaches `n`"), and say which
  variables the flow uses.
