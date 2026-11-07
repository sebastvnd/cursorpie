# Writing flows as JSON (a reference for AI agents)

This page is written so that an AI agent (or a person scripting) can produce a **valid CursorPie flow** as JSON
without opening the app. It is complete on its own; the sources it is checked against are
`src/config/flow.rs` (the model, the repair rules, the file format) and `src/actions.rs` (what running a flow does).
The examples in [`examples/flows/`](examples/flows) are parsed by the unit tests, so they cannot drift from the code,
and [`flow.schema.json`](flow.schema.json) is a JSON Schema of the file for tools that can validate against one.

If you only read one section, read [The recipe](#the-recipe) and [The rules that make a flow wrong](#the-rules-that-make-a-flow-wrong).

## What a flow is

A **flow** is a *chain* of steps that a pie slice runs one after the other: press a shortcut, open an app, open a
URL or file, run a shell command, wait. It is drawn in the **Flow** tab of Settings as blocks joined by wires.

- It is a **single straight chain**: `Start -> step -> step -> ... -> End`. Each block has at most one wire going
  out (`next`) and at most one coming in.
- There is **no branching, no condition, no loop, no variable, no nested flow, no "type this text" step.** Do not
  invent block kinds; the app refuses a file with an unknown kind. To type text or do anything else, use a
  `Command` block (see [Command blocks](#command-blocks-and-safety)).
- The app runs the chain from `Start` along the `next` wires. A block that is not on that chain is **ignored**.

## Two places a flow can live

| Where | Shape | Use it when |
|---|---|---|
| **A flow file**, `<name>.cursorpie-flow.json` | `{ "cursorpie_flow": 1, "flow": { ... } }` | You hand the person a file. They press **Import** (arrow down) in the Flow tab. The flow is added as a **new** flow (new id, unique name); nothing of theirs is overwritten. **This is the safe default.** |
| **`flows` in `config.json`** | the top-level array `"flows": [ { ... }, ... ]` of the config, each item is the same `flow` object | You write the whole config (see [Attaching a flow to a slice](#attaching-a-flow-to-a-slice)). Read the warning there first. |

The `flow` object is the same in both places.

## The format

```jsonc
{
  "cursorpie_flow": 1,                 // file format version: always 1 (a file with a higher number is refused)
  "flow": {
    "id": "copy-switch-paste",         // string, not empty, unique among flows; replaced with a fresh id on import
    "name": "Copy, switch, paste",     // string; trimmed, cut to 60 characters; made unique ("Name (2)") on import
    "nodes": [                         // 2 to 64 blocks
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
| `kind` | string or object | What the block does. See the next table. |
| `next` | integer or `null` | The `id` of the block the wire from this block's output leads to. `null` or absent = no wire. |

Serde's default external tagging is used: a block without data is a **bare string** (`"Start"`), a block with data is
an **object with one key** (`{ "Wait": 500 }`).

### The seven kinds

| `kind` | JSON | What it does when it runs | Notes |
|---|---|---|---|
| Start | `"Start"` | nothing; the chain begins here | exactly **one**; nothing may be wired into it |
| End | `"End"` | nothing; the chain ends here | exactly **one**; its `next` is `null`; it needs an input wire |
| Key | `{ "Key": { "key": 8, "mods": 8 } }` | presses the shortcut | see [Shortcuts](#shortcuts) |
| OpenApp | `{ "OpenApp": "Notes" }` | opens or raises an app | name or path, see [Apps, URLs, files](#apps-urls-and-files) |
| OpenUrl | `{ "OpenUrl": "https://example.com" }` | opens a URL or a file with the system default | |
| Command | `{ "Command": "echo hi" }` | runs a shell command line | read [Command blocks](#command-blocks-and-safety) |
| Wait | `{ "Wait": 500 }` | pauses, in **milliseconds** | 0 to 10000; more is cut to 10000 |

Empty `OpenApp`, `OpenUrl` and `Command` strings (or only spaces) do nothing. `OpenApp` and `OpenUrl` text is trimmed before it is used.

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

1. The slice is released (or its mode button pressed) and the app copies the flow's steps out.
2. Steps run on a worker thread, in chain order, **40 ms apart**, and a `Wait` adds its own pause on top of that.
3. `Start` and `End` are not steps. Unwired blocks are not steps.
4. If the slice points at a flow id that does not exist, nothing happens.

Because actions run one at a time, a long flow keeps the worker busy: a `Wait` is capped at 10 s. Give an app time to
come to the front before a shortcut is sent to it: **put a `Wait` (300 to 800 ms) between `OpenApp` / `OpenUrl` and
a following `Key`.** A shortcut sent too early goes to the app that was in front before.

## The recipe

To turn a request such as "copy the selection, open Notes, make a new note, paste" into JSON:

1. **Fix the target OS** (it decides `mods`, app names, and commands). Ask if unknown.
2. **Break the request into steps** and pick one kind per step from the seven kinds. Insert a `Wait` after every
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
| Exactly one `"Start"` and one `"End"` | A duplicate is dropped. **A missing `Start` is added with no wire, so the chain is just that `Start` and every other block is ignored: the flow does nothing.** A missing `End` is added after the last block of the chain. |
| `Start` is the first block of the chain and nothing is wired into it | A wire into `Start` is removed. |
| Every `next` names an existing block id | A wire to a missing block is removed; the chain stops there. |
| One wire **into** each block | If two blocks point at the same target, only the first keeps its wire. |
| One wire **out of** each block (`next` is a single id) | A list is a wrong shape and the file is refused. |
| No loops, no wire from a block to itself | The wire that closes the loop is removed. |
| `End` has no `next` | It is cleared. |
| Block `id`s unique | Duplicates are dropped (the later one). |
| At most 64 blocks (32 flows per config) | The extra blocks are cut off the end of the list. |
| `Wait` is 0 to 10000 | Cut to 10000. A negative or fractional number is a wrong shape: file refused. |
| Block kinds are exactly the seven above, spelled with that capitalisation | **An unknown kind refuses the whole file** ("That flow file is damaged."). |
| `cursorpie_flow` is present and is 1 | Missing: "not a CursorPie flow". Greater than 1: "made by a newer CursorPie". |
| The file is valid JSON, UTF-8, at most 1 MiB | Refused. No comments, no trailing commas (the `jsonc` in this page is for reading only). |

A wrong **type** (a string where a number goes, an array for `next`) refuses the file; unknown extra fields are ignored.

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

## The checklist before you answer

- [ ] The JSON parses (no comments, no trailing commas, double quotes, escaped backslashes and quotes).
- [ ] Top level is `{ "cursorpie_flow": 1, "flow": { "id", "name", "nodes" } }`.
- [ ] Exactly one `"Start"` and one `"End"`; `End.next` is `null`.
- [ ] Every block has `id`, `x`, `y`, `kind`; ids are unique integers.
- [ ] Following `next` from `Start` visits **every** block once and ends at `End` (no block left over, no loop, no
      two blocks pointing at the same one).
- [ ] Each `kind` is one of `"Start"`, `"End"`, `{"Key":{"key":int,"mods":int}}`, `{"OpenApp":str}`,
      `{"OpenUrl":str}`, `{"Command":str}`, `{"Wait":int 0..10000}`.
- [ ] `Key.key` is a number from the table; `mods` is the sum of bits for the **target OS**.
- [ ] A `Wait` sits between a step that opens something and the step that needs it.
- [ ] If there is a `Command`, you have said so and quoted it for the person.
- [ ] The answer tells the person the file name and the import steps.

When you can run code, this check is short (Python):

```python
import json, sys
f = json.load(open(sys.argv[1]))
assert f["cursorpie_flow"] == 1
nodes = f["flow"]["nodes"]; by_id = {n["id"]: n for n in nodes}
assert len(by_id) == len(nodes) <= 64, "ids must be unique, at most 64 blocks"
kinds = [n["kind"] if isinstance(n["kind"], str) else next(iter(n["kind"])) for n in nodes]
assert kinds.count("Start") == 1 and kinds.count("End") == 1
assert set(kinds) <= {"Start","End","Key","OpenApp","OpenUrl","Command","Wait"}
cur, seen = next(n for n in nodes if n["kind"] == "Start"), []
while cur:
    assert cur["id"] not in seen, "loop"
    seen.append(cur["id"])
    cur = by_id.get(cur.get("next")) if cur.get("next") is not None else None
assert len(seen) == len(nodes), "a block is not on the chain"
assert by_id[seen[-1]]["kind"] == "End", "the chain must end at End"
for n in nodes:
    k = n["kind"]
    if isinstance(k, dict) and "Wait" in k: assert 0 <= k["Wait"] <= 10000
    if isinstance(k, dict) and "Key" in k: assert 0 <= k["Key"]["mods"] <= 15
print("ok:", " -> ".join(str(i) for i in seen))
```

## Answering well

- Output the JSON in **one code block**, then the file name and the import steps. Do not add commentary inside the JSON.
- Name the target OS you assumed, and every assumption about app names and key positions.
- Keep flows short and every `Wait` purposeful. Prefer several small flows over one with 40 blocks.
- If the request needs something a flow cannot do (branching, repeating, reading the screen, typing arbitrary text
  without a command), say so instead of faking it with extra block kinds.
