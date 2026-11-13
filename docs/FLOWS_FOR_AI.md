# Writing flows as JSON (a reference for AI agents)

This page is written so that an AI agent (or a person scripting) can produce a **valid CursorPie flow** as JSON
without opening the app. It is complete on its own; the sources it is checked against are
`src/config/flow.rs` (the model, the repair rules, the file format) and `src/actions.rs` (what running a flow does).
The examples in [`examples/flows/`](examples/flows) are parsed by the unit tests, so they cannot drift from the code,
and [`flow.schema.json`](flow.schema.json) is a JSON Schema of the file for tools that can validate against one.

The app can hand this page to an agent for the person: **New flow, Build it with AI** in the Flow tab puts a zip with this
page, `flow.schema.json`, the examples and the person's request (`REQUEST.md`) on the clipboard (the code is
`config::flow_kit`, and `flow-kit/START_HERE.md` is what the agent reads first).

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
  is left as it is. After the name any number of **filters** can follow (`{{name|trim|upper}}`), see "Filters and
  lists" below.
- The **operands** of `Calc`, `Logic`, `Branch` and `While` are a value (`3`, `yes`, `{{x}}`) or the bare **name of a
  variable** (`n`). A bare name that is defined means its value.
- A field that **receives** a result (`out`, `name`, and the `variable` fields of `Native`) takes a bare variable name,
  without braces.
- **Truth:** a text is false when it is empty, `0`, `false`, `no` or `off` (any case, spaces ignored), and true
  otherwise. Gates and `Calc` write `1` and `0`.
- **Numbers:** a text is a number when it parses as one (`3`, `-2.5`); whole results are written without a decimal point.
- Set by the blocks themselves: `ok` (`1` when the last block that can fail worked: `Calc`, `Ask`, `PanelUpdate`, `Run`, `Native`), `code` (the
  exit code of the last `Run`; `-1` when it could not start, was ended by the time limit or a signal) and `index` (the
  round of the innermost `Repeat`, `While` or `ForEach`, counting from 1).

### The logic blocks

| `kind` | JSON | Behaviour |
|---|---|---|
| Set | `{ "Set": { "name": "n", "value": "{{n}}!" } }` | puts the value (variables filled in) in `n` |
| Calc | `{ "Calc": { "out": "i", "a": "i", "op": "Add", "b": "1" } }` | `out = a op b` as numbers. `op`: `Add`, `Sub`, `Mul`, `Div`, `Rem`, `Min`, `Max`. A text that is not a number, or a division by zero, leaves `out` empty and `ok` at `0` |
| Logic | `{ "Logic": { "out": "g", "a": "x", "op": "And", "b": "y" } }` | `out = a gate b` as `1` or `0`. `op`: `And`, `Or`, `Xor`, `Nand`, `Nor`, `Not` (`Not` ignores `b`) |
| Branch | `{ "Branch": { "a": "n", "op": "Gt", "b": "3" } }` | follows **`next` when the condition holds, `alt` when it does not** |
| Repeat | `{ "Repeat": 3 }` | goes through **`next` (the body)** 3 times (0 to 1000), then follows **`alt` (done)** |
| While | `{ "While": { "a": "i", "op": "Lt", "b": "n" } }` | goes through `next` (the body) while the condition holds (at most 1000 rounds), then follows `alt` |
| List | `{ "List": { "op": "Push", "list": "xs", "a": "{{item}}", "b": "", "out": "" } }` | works on the **list** kept in the variable `list` (see "Filters and lists"); `op` decides what `a`, `b` and `out` are for |
| ForEach | `{ "ForEach": { "list": "xs", "item": "x" } }` | goes through **`next` (the body)** once for every item of the list in `list`, with the item in `item` and its position (from 1) in `index`, then follows **`alt` (done)**. At most 1000 rounds. A variable that does not hold a list ends the loop at once (and `ok` is `0`) |
| Call | `{ "Call": "other-flow-id" }` | runs the flow with that id (same variables), then goes on. Depth at most 8; a missing flow is skipped |
| Stop | `"Stop"` | ends the **whole run**, also inside a called flow. No outputs |

`op` of a condition (`Branch`, `While`): `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge` (two numbers are compared as numbers,
anything else as text), `Contains`, `StartsWith`, `EndsWith`, `IsTrue` (only `a`), `IsEmpty` (only `a`).

**A loop is a wire back.** The body of a `Repeat`, `While` or `ForEach` is the chain that starts at `next`; its last block must be
wired **back to the loop block itself**. `alt` leads on. See `examples/flows/ask-and-loop.cursorpie-flow.json`.

### The blocks that ask and the blocks that use the system

| `kind` | JSON | Behaviour |
|---|---|---|
| Ask | `{ "Ask": { "mode": "Text", "message": "Name?", "preset": "Bob", "out": "name" } }` | shows a window of the system and waits. `mode`: `Text` (a box; `preset` is what it starts with), `YesNo` (`out` becomes `1` or `0`) or `Panel` (`message` is the JSON of a panel, see `docs/PANELS.md`: each field sets the variable named by its `id`, `out` gets the id of the button that was pressed, `ok` is `1` only when a confirming button was pressed). `ok` is `0` when it was cancelled or no window program exists (Linux needs `zenity` or `kdialog`); on Windows an empty answer counts as cancelled |
| PanelUpdate | `{ "PanelUpdate": { "op": "Open", "panel": "progress", "title": "Copying", "label": "Starting", "value": "0", "stop": true } }` | a **progress panel** the flow runs itself; **nothing waits for it**. `op`: `Open` (shows it: `title`, a bar with `label` and `value`, and with `stop: true` a Stop button that ends the whole run when pressed), `Update` (changes `title`, `label` and `value`; a field left empty stays as it was) or `Close`. `panel` is its id (empty: `progress`; use another id for a second one, which takes the window from the first). `value` is a fraction from 0 to 1 (`0.4`), a percentage (`40%`) or `busy` (a bar that moves, no number); a plain number above 1 is a full bar, so write `50%`. The texts may hold `{{variables}}`. Put a `Calc` (`index` / count) before an `Update` to get a fraction. `ok` is `0` when panels are switched off (*Behavior > Panels of flows*) or the value cannot be read. The panel goes away by itself when the run ends. It never takes the keyboard, so `Key` and `TypeText` blocks still reach their app. Example: `docs/examples/flows/progress-panel.cursorpie-flow.json` |
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
| `ListDir` | folder path | variable: **a list** of the names in it (sorted, no path; at most 5000) |
| `MakeDir` | folder path (and the folders above it) | |
| `CopyFile` | from file (a folder is not copied) | to file (replaced) |
| `MoveFile` | from path | to path (across disks a file is copied, then removed) |
| `DeleteFile` | file path. **A folder is never deleted**, and neither is a link to one | |
| `FileSize` | file path | variable (bytes) |
| `FileTime` | file or folder path | variable (when it was last changed, Unix seconds) |
| `SystemInfo` | one of `os` (`macos`, `windows`, `linux`), `arch`, `user`, `host`, `home`, `temp` | variable |
| `Say` | text to speak (waits until it is said; macOS `say`, Windows PowerShell, Linux `spd-say`, `espeak-ng` or `espeak`) | |
| `Screenshot` | path of a `.png` file for a picture of the whole screen (macOS asks for the *Screen Recording* permission) | |
| `VolumeGet` | | variable (0 to 100; macOS, Linux with `wpctl`, `pactl` or `amixer`; **not Windows**) |
| `VolumeSet` | number 0 to 100 (same systems) | |
| `Browser` | a command of the browser extension (`media.next`, `tab.find {{word}}`, `chat.draft_to Anna :: hi`; the list is in `docs/BROWSER_EXTENSION.md`) | variable for the browser's reply (optional) |
| `OpenAppSmart` | app name or id | how: `smart` (default: go to it when open, start it when not), `existing` (only go to it when open; `ok` is `0` when it is not), `new` (a new window), `plain` (as the *Open app* block) |
| `OpenUrlSmart` | link or file | how, as above: `smart` goes to the tab that has the page when the browser extension reports it open, `existing` only does that (`ok` is `0` when no tab has it), `new` opens a new browser window |
| `Quiet` | `on` (or empty) or `off` | |

`Quiet` hides the notices of the rest of the run: the pill that announces keys, apps and links, and the `Notify` blocks.
Put it right after *Start* in a flow that asks again and again (a remote control in a panel).

`Browser` **waits** until the extension says how the command went (20 seconds at most; a `ui.*` panel as long as a person
needs, up to 5 minutes, and `chat.wait` as long as it was told to wait, up to 300 seconds, plus 15; that time does not
count against the run). `ok` is `1` if it worked. The variable gets the
browser's words: the answer of a question (`tab.url` the address, `tab.count` the number of tabs, `media.state` `playing`,
`paused` or `none`), or the reason when it failed. Without the extension (or with no browser connected) `ok` is `0` and
the run reports it unless the flow looks at `ok`. A command that sends a message, closes a window or reads a chat
(`chat.send`, `chat.send_to`, `chat.template`, `window.close`, `chat.who`, `chat.unread_names`, `chat.folders`, `chat.last`,
`chat.read`, `chat.wait`), or
one that is only a `{{variable}}`, makes the block red like `Run`.

The chat questions for a flow: `chat.unread` (a number), `chat.unread_names` (the names of the chats that wait, one to a
line: `List` `Split` makes a list of it), `chat.who`, `chat.last`, `chat.read N` and `chat.wait SECONDS` (the next message
that comes in the open chat; empty with `ok` `1` when none came in time). The ones that read a chat refuse (`ok` `0`, the
variable says why) until the person turns on *Allow reading messages* in the extension options, and `chat.send_to` only
types the message until *Allow sending messages* is on. Ask the person for the name of a chat, or take it from
`chat.unread_names`; never invent one. Always put a `Branch` on `ok` after a chat command and an `Ask` `YesNo` before
`chat.send_to`: a message cannot be taken back.

Telegram extras for a flow: `chat.folder NAME` (a folder tab, by name or number) before `chat.next_unread` walks that folder's
unread chats; `chat.folders` (the folder names, one to a line, like `chat.unread_names`; needs *Allow reading messages*);
`chat.next_mention` (a chat where somebody wrote the person's @name); `chat.template NAME` or `chat.template NAME :: Chat` (a
reply the person saved in the extension options, sent like `chat.send_to`: ask first, and never invent a name; `chat.templates`
lists the names). `chat.open @username` and `chat.draft_to @username :: text` open a Telegram chat by address; the chat cannot
be checked then, so `chat.send_to @username` only types the message and answers `ok` `0`: do not use it to send.

The clipboard needs `pbcopy` / PowerShell / `wl-copy`, `xclip` or `xsel`; notifications need `osascript` / PowerShell /
`notify-send`. A missing tool makes `ok` `0` (and the run reports it unless the flow looks at `ok`). There is no block
that moves or clicks the mouse and none that makes a network request (use `Run` with `curl`).

`MakeDir`, `CopyFile`, `MoveFile`, `DeleteFile`, `WriteFile`, `AppendFile` and `Screenshot` **change the computer**: the
block is drawn in red and importing a flow with one warns the person. Say in your answer which files the flow touches,
and prefer a `Notify` or a `Branch` that asks (`Ask` with `YesNo`) before a `DeleteFile` or a `MoveFile`.

### Filters and lists

**Filters.** `{{name|f1|f2}}` applies the filters from left to right. An argument comes after a `:` and is **not**
trimmed (a separator can be a space); `\n` and `\t` in an argument are a line break and a tab. A variable that was never
set is only replaced when a `default:` filter is in the chain. A filter that does not exist, has the wrong argument, or
cannot work on the value leaves the **whole** `{{...}}` as written (and the editor shows an amber warning).

| Filter | Result |
|---|---|
| `sh` | quoted as one word for the shell |
| `trim`, `upper`, `lower`, `title`, `reverse` | spaces around it cut; in capitals; in small letters; each word capitalized; backwards |
| `len` | how many characters |
| `int`, `round`, `abs`, `num` | the number cut, rounded, without its sign, written the plain way (not a number: the text stays as written) |
| `bool` | `1` or `0`, read like a condition reads it |
| `json` | a JSON string, quotes included (for building JSON by hand) |
| `url` | percent-encoded (safe in a URL) |
| `basename`, `dirname`, `ext` | last part of a path; the folder; the extension without the dot |
| `count`, `first`, `last` | of a list: how many items, the first, the last |
| `join` / `join:SEP` | of a list: the items with `, ` (or `SEP`) between them |
| `item:N` | of a list: the item at position `N` (from 1, `-1` the last) |
| `default:TEXT` | `TEXT` when the value is empty (or the variable was never set) |
| `replace:A:B` | every `A` replaced by `B` |

**Lists** are kept in a variable as a **JSON array of texts**: `["a","b"]` (numbers and other values in a file you write
by hand become their text). An empty or unset variable is the empty list. A text that is not a JSON array is not a
list: a block that needs one fails (`ok` `0`, and the run reports it). The list in a variable must stay under 16384
characters. Positions count from 1, a negative one from the end. In the `List` block `list` is the **bare name** of the
variable; `a`, `b` take `{{variables}}` like any text field and `out` is a bare variable name.

| `op` | `a` | `b` | `out` | Does |
|---|---|---|---|---|
| `Split` (default) | text to cut | separator (empty or `\n`: every line; `\t` is a tab) | | makes the list from the text |
| `Push` | item to add | | | adds at the end |
| `Pop` | | | variable | takes the last item out and puts it in `out` (empty list: `ok` `0`) |
| `Get` | position | | variable | puts the item in `out` (no such item: `ok` `0`) |
| `SetItem` | position | new value | | replaces the item |
| `Remove` | position | | | removes the item |
| `Length` | | | variable | how many items |
| `Join` | text between the items | | variable | the items as one text |
| `IndexOf` | item to find | | variable | its position, `0` when it is not there |
| `Sort` | | | | numbers as numbers when **all** are numbers, else as text |
| `Reverse`, `Unique`, `Shuffle`, `Clear` | | | | reverses; keeps the first of equal items; random order; empties |
| `Sum` | | | variable | the sum (an item that is not a number: `ok` `0`) |

To do something for **every item**, use `ForEach` and wire the last block of its body back to it. The item is in the
variable named by `item`, its position in `index`. A position in `a` is a number or `{{variable}}`: a bare name such as
`i` is **not** read as a variable there.

```text
Set text = "a\nb\nc"  ->  List Split (list "xs", a "{{text}}")  ->  ForEach (list "xs", item "x")
   body: Notify (a "Item {{index}}", b "{{x|upper}}")  ->  back to the ForEach
```

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

## Writing shell lines that work (`Run` and `Command`)

These are facts about how the line is started, and each one has caused a flow that "does nothing":

- **Only standard output comes back.** `Run` captures what the line prints to stdout into `out`. **Standard
  error is thrown away.** A message written with `echo "..." >&2` is never seen: print the reason to stdout
  (`echo "why"`), then `exit 1`, and show `{{out}}` in a `Notify`. `Run` still stores stdout when the exit code
  is not 0, and `ok` is `1` only when the exit code is `0`.
- **The shell is `/bin/sh`, started by the app, not by the person's terminal.** It does not read `.zshrc` or
  `.bash_profile`, and `PATH` is the short one of a graphical app: Homebrew tools (`/opt/homebrew/bin`,
  `/usr/local/bin`) are usually **not** found. Call them by full path, or use only what the system ships.
- **`!` binds to the first command only.** `if ! a || b; then fail` means `(not a) or b`, so it fails when `a`
  *works* and `b` also does. To say "neither worked" write `if ! { a || b; }; then`.
- **BSD versus GNU tools.** On macOS `stat -f '%m %N'` prints the modification time and the name; on Linux it is
  `stat -c '%Y %n'`. `date`, `sed -i`, `find -printf` and `readlink -f` differ the same way. Write the line for the
  system the flow is for (see [Which OS is it for?](#shortcuts)).
- **Quote every path.** File names have spaces and brackets. Use `"$HOME/..."` in a line and `{{name|sh}}` for a
  variable. Splitting `find` output with `cut -d ' '` is fine only when the file name is the **last** field.
- **Say what happened.** A flow that fails silently cannot be fixed by someone who was not looking. Make the line
  print a sentence for each way it can fail and show it:

  ```
  Run   (out = why)   ... || { echo "No export found in the last 2 hours."; exit 1; }
  Branch ok IsTrue    then -> go on,  else -> Notify "{{why}}"
  ```

## Putting a file on the clipboard

`Native` `ClipboardSet` puts **text** on the clipboard, never a file. A path pasted as text is not an attachment.
To make a *copy of a file* (the thing that attaches in a chat or a mail when pasted), use a `Run`:

| System | Line (the path is in `$f`) | Notes |
|---|---|---|
| macOS | `osascript -l JavaScript -e 'ObjC.import("AppKit");function run(a){var p=$.NSPasteboard.generalPasteboard;p.clearContents;return p.writeObjects($([$.NSURL.fileURLWithPath(a[0])]))}' "$f"` | Writes the file URL the way Finder does (the tested flow tries this first). Fallback, tried second: `osascript -e 'on run argv' -e 'set the clipboard to ((POSIX file (item 1 of argv)) as alias)' -e 'end run' "$f"` |
| Windows | `powershell -NoProfile -STA -Command "Set-Clipboard -LiteralPath $env:CURSORPIE_FILE"` | `-STA` is the safe choice for the clipboard. Pass the path as the variable `file`: `Run` gives it to the line as `CURSORPIE_FILE`, which avoids quoting. For anything longer than a line, use `-EncodedCommand` (UTF-16LE, base64) |
| Linux | depends on the desktop (`wl-copy` / `xclip` with a `text/uri-list` or `x-special/gnome-copied-files` target) | Not the same on every desktop: test it before you promise it |

Two mistakes to avoid on macOS:

- `set the clipboard to (item 1 of argv) as «class furl»` is **not** a file: the argument is text, so the clipboard
  holds a path as text. The path must be made a file reference first (`POSIX file ...`, then `as alias`).
- `set the clipboard to "/path/file.png"` is text as well.

Re-copy the file **right before** the paste (a second `Run` with `{{file|sh}}`): a dialog, a notification or a
clipboard manager may have replaced it in between. The finished example is
[`examples/flows/copy-newest-file-and-paste.cursorpie-flow.json`](examples/flows/copy-newest-file-and-paste.cursorpie-flow.json)
(macOS).

## What the system asks permission for (macOS)

A flow can be correct and still be refused by the system. None of these shows an error in a flow, because the
refused line prints nothing:

| What the flow does | What macOS requires | Symptom |
|---|---|---|
| `Key` (a shortcut) | CursorPie in **Privacy & Security > Accessibility** | the shortcut is never pressed |
| `Run` that reads `~/Downloads`, `~/Desktop`, `~/Documents` | CursorPie in **Privacy & Security > Files and Folders** (or Full Disk Access) | `ls` and `find` print nothing and the exit code is not 0; a `find ... 2>/dev/null` looks like "no files" |
| `Native` `Notify` | notifications allowed for the app that shows them | no banner |

So test access first and say it: `ls "$HOME/Downloads" >/dev/null 2>&1 || { echo "macOS blocks Downloads: allow CursorPie in Privacy & Security > Files and Folders."; exit 1; }`.
Windows and most Linux desktops do not ask.

## Web pages in a browser

`OpenUrl` returns as soon as the browser is told, **not** when the page has loaded, and a web app does nothing with
a shortcut until it is loaded, signed in, and showing the right place. A flow cannot read the screen, so it cannot
see any of that. The pattern that works for "open a web app, then paste":

1. `OpenUrl` the app, then a `Wait` of 2000 to 3000 ms.
2. `Native` `FrontApp` into a variable (`app`): this is the browser, whichever one the person uses.
3. `Ask` `YesNo`: tell the person what to do by hand (log in, open the chat), and that **Yes** continues.
4. `Native` `ActivateApp` with `{{app}}`, a `Wait` of about 700 ms (the dialog has to close and the browser to take
   the focus back), then the `Key`.

Chat web apps (Telegram Web, WhatsApp Web, Slack) take a pasted file only when a conversation is open and the page
is in front. Pasting a file usually opens a preview that still waits for **Enter** or **Send**: do not press Send
for the person.

## When a flow "does nothing"

Work through these in order; each takes a minute.

0. **Is anything red?** Open the flow: a red or amber wire, or a block outlined in red, has a `!` that says what is
   wrong (see "Wires the editor marks" below). After a run that failed, a notification and the sheet in Settings
   name the block and the reason, unless an `If / else` on `ok` or `code` handles the failure.
1. **Did it start?** Put a `Notify` right after `Start`. If it does not show, the slice is not pointing at this flow
   (or notifications are blocked), not a problem of the flow.
2. **Which block was the last to run?** Add a `Notify` with `{{code}}` and `{{ok}}` after each `Run`. `code` `-1`
   means the line could not start, ran out of time, or was killed.
3. **Run the line by hand.** Paste the `Run` text in a terminal, with the variables replaced. If the terminal works
   and the flow does not, the cause is the short `PATH`, a missing permission (above), or standard error.
4. **Look for what the flow cannot see.** A browser that has not loaded, a window that did not come to the front,
   a clipboard that was replaced. Add a `Wait`, an `Ask`, or a second copy of the file.
5. **Leave a trace.** `echo "$(date '+%F %T') $f" >> "$HOME/flow.log"` inside the `Run` shows, afterwards, what it
   saw and when.
6. **Ending a flow in two places.** There is exactly one `End`. Every other path ends with `Stop` (or a missing
   wire). A second `End` is dropped on load, and the wires that led to it are cut.

## How a flow runs

1. The slice is released (or its mode button pressed) and the app copies the flow out of the config.
2. The run is on a worker thread. It starts at the block `Start` leads to and goes block by block along the wires.
   After a block that *does* something (`Key`, `OpenApp`, `OpenUrl`, `Command`, `Wait`, `Run`, `Native`) it pauses
   **40 ms**; blocks that only compute or decide do not pause.
3. It ends at `End`, at `Stop`, at an output without a wire, after **10000 blocks**, or after **600 seconds**
   (time spent waiting for an answer in `Ask` does not count). A loop is also limited to 1000 rounds.
4. If the slice points at a flow id that does not exist, nothing happens.
5. After the run, a block that could not do its work and that no `If / else` looked at (`ok` / `code`) is reported:
   a desktop notification (at most one every few seconds), a red outline on the block in the editor, and, while
   Settings is open, a sheet with the reason.

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

## Wires the editor marks (and how to avoid them)

A flow that loads is not a flow that works. The Flow editor checks every flow without running it and draws a wire
that cannot work **red**, one that is probably a mistake **amber**. Nothing is refused, but **a red wire is a flow
that will not do what the wire says**, so do not hand one to a person. Check these before you answer:

| Mark | The mistake | Write it like this instead |
|---|---|---|
| red | A block uses `{{name}}`, or compares or calculates with the bare `name`, and **no block on the way sets it** (a run starts with no variables, so `{{name}}` stays as typed) | Put a `Set`, `Ask`, `Run`, `Calc`, `Logic` or a `Native` that returns a value (`ClipboardGet`, `ReadFile`, `Env`...) **before** the block, on every wire that leads to it. A variable set at the end of a loop body and read at its start is fine |
| red | The `next` (body) of a `Repeat` of 2 or more, or of a `While`, never leads back to it | Wire the last block of the body to the loop block's input. The loop leaves through `alt` |
| red | A cycle with no `Branch`, `Repeat` or `While` in it | Add one that can leave the cycle. Only the 10000-block limit would stop it |
| red | `Call` names an id that is not in the config's `flows` (or is empty) | Use the `id` of a flow that exists, or ship it in the same file |
| red | `Calc` with a text that is not a number, an empty operand, or `Div` / `Rem` by the number 0 | Numbers, or the names of variables that hold numbers |
| red | `Set`, `Calc`, `Logic`, or a `Native` that returns a value, with no valid variable name | Letters, digits and `_`, not starting with a digit |
| amber | `OpenApp` / `OpenUrl` and then a `Key` straight away | Put a `Wait` of 300 to 800 between them |
| amber | A `Command` followed by a block that needs its result (a `Key`, `ClipboardGet`, `ReadFile`, `FileExists`) | Use `Run`, which waits |
| amber | Both outputs of a `Branch` go to the same block | One of them was meant to go somewhere else |
| red | A `List` (other than `Split`) or a `ForEach` works on a variable that **no block before it sets** | Make the list first: `List` with `Split`, or a `Native` that returns one (`ListDir`) |
| red | The body of a `ForEach` never leads back to it (like `Repeat` and `While`) | Wire the last block of the body to the `ForEach` |
| red | A position (`Get`, `SetItem`, `Remove`) that is not a number, or a `VolumeSet` that is not 0 to 100 | A number, or `{{variable}}` |
| red | A `Browser` whose command, written out, is empty, is not a command of the extension, or lacks the text it needs | A command from `docs/BROWSER_EXTENSION.md` |
| amber | A wire out of a block that no run reaches; an empty `OpenApp`, `OpenUrl`, `Command` or `Run`; `Repeat` 0; `Call` to itself; `{{name\|filter}}` with a filter that does not exist | Remove it, or fill it in |

The check follows variables along the wires and is lenient: a variable set on *any* path that reaches the wire counts,
and after a `Call` anything may be set. It does not know **values**, so a `Calc` on a variable that holds text is
not caught.

### What happens when a block fails at run time

A block that cannot do its work (`Run` ends with a code other than 0, a file cannot be read or written, the
clipboard does not answer, a `Call` to a missing flow, a `Calc` on text...) does not stop the run: it sets `ok` to
0 and the flow goes on. The person is **told** (desktop notification, a red outline in the editor, a sheet with the
reason) unless the flow **looked at it**: a later `Branch` or `While` whose condition reads `ok` or `code` (the bare
name, or `{{ok}}` / `{{code}}` in a text) counts as handling the **last** failure. So:

- when a step can fail and the flow can carry on, **put a `Branch` on `ok` (or `code`) right after it**, and let the
  `else` path say something (`Notify`) or end with `Stop`. That is both better for the person and quiet when all is well;
- when a step failing means the flow is pointless, still put the `Branch`: its `else` should explain *what* failed in
  words that fit the task, which is better than the generic sentence the app writes;
- a cancelled `Ask` is not a failure (`ok` is 0, nothing is reported);
- `Command`, `OpenApp` and `OpenUrl` have no result the flow can see, so their failures cannot be reported: use
  `Run` when it matters.

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

**6. A file on the clipboard, then paste** (macOS): `copy-newest-file-and-paste.cursorpie-flow.json`. Finds the newest
export in `Downloads`, `Desktop` or `Documents`, puts it on the clipboard **as a file** (`Run`), remembers the app in
front (`FrontApp`), asks the person to put the cursor where the file goes (`Ask`), brings the app back
(`ActivateApp`, `Wait`) and presses Cmd+V (`Key`). Every way it can fail ends in a `Notify` and a `Stop`.

**7. Quick reply to every unread chat, with a person confirming** (needs *Allow reading messages*):
`quick-reply-unread-chats.cursorpie-flow.json`. `chat.unread_names` into `names` (a `Branch` on `ok` says why when reading
is off, one on `IsEmpty` says that nothing waits), `List` `Split` into `chats`, then a `ForEach`: `chat.open {{chat}}`,
`chat.last` into `last`, an `Ask` `Text` that shows what came and starts with a quick reply (Cancel skips the chat), an
`Ask` `YesNo` that shows exactly what will be sent, and only then `chat.send_to {{chat}} :: {{reply}}`. Every path ends
back at the `ForEach`.

**8. Send, then wait for the answer**: `send-then-wait-for-answer.cursorpie-flow.json`. Asks for the name and the message
(`Ask`), asks to confirm (`Ask` `YesNo`), `chat.send_to`, then `chat.wait {{secs}}` into `answer` and a `Notify` that says
the answer or that nobody answered in time. The wait does not count against the run.

**9. Telegram, one folder at a time** (needs *Allow reading messages*): `telegram-folder-triage.cursorpie-flow.json`. Asks for
a folder name or number (`Ask`; Cancel or an empty answer ends the flow), `chat.folder {{folder}}` into `said`,
`chat.unread_names` into `names`, and a `Notify` that lists the unread chats of that folder (or says that none waits).
A last `Ask` `YesNo` offers `chat.next_unread`. Every `chat.*` block is followed by a `Branch` on `ok` that notifies the
reason on failure. Muted chats are left out by the extension option *Skip muted chats*.

**10. Send a saved reply** (WhatsApp Web or Telegram Web): `telegram-saved-reply.cursorpie-flow.json`. `chat.templates`
into `names` shows what the person saved, an `Ask` `Text` takes the name of one (`key`) and another the chat (`target`,
empty means the open chat), an `Ask` `YesNo` confirms, and a `Branch` on `target` `IsEmpty` picks `chat.template {{key}}` or
`chat.template {{key}} :: {{target}}`. A `Branch` on `ok` ends in a `Notify` of the result. Sending still needs *Allow
sending messages*; without it the reply is only typed and `ok` is `0`.

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
- [ ] A line that can fail prints the reason to **stdout** (stderr is dropped) and the flow shows it.
- [ ] A file is put on the clipboard with a `Run` ([the table](#putting-a-file-on-the-clipboard)), not with `ClipboardSet`.
- [ ] A flow that reads `~/Downloads`, `~/Desktop` or `~/Documents`, or presses a key, says which macOS permission it needs.
- [ ] Opening a web page is followed by a `Wait` and by an `Ask` for what the flow cannot see.
- [ ] Extra endings use `Stop`, not a second `End`.
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
                      "Branch","Repeat","While","ForEach","List","Ask","Run","Native","Call","Stop"}
two_out = {"Branch", "Repeat", "While", "ForEach"}
no_out = {"End", "Stop"}
for n in nodes:
    k = name(n["kind"])
    outs = [n.get("next"), n.get("alt")]
    assert all(t is None or t in by_id for t in outs), f"block {n['id']}: a wire to nothing"
    assert n.get("alt") is None or k in two_out, f"block {n['id']}: only Branch, Repeat, While, ForEach have alt"
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
- Say what you could **not** test. A flow that touches the clipboard, a browser or a permission has only been
  proven when the person has run it on their system: ask for the result, and for the text of the notification.
- Explain every loop and every branch in one sentence ("the body repeats until `i` reaches `n`"), and say which
  variables the flow uses.
