# Flows: chains of steps

A flow is a small program of blocks and wires that one slice runs. This page explains the **Flow** section of Settings.
Examples you can import are listed in [Examples](../examples/README.md); the file format for writing one by hand or
with an AI agent is in [FLOWS_FOR_AI.md](../FLOWS_FOR_AI.md).

The **Flow** section of Settings is a small node editor. A *flow* is a small program made of blocks and wires that one
slice runs: shortcuts, apps, links, shell commands and waits, one after the other, for example "copy, open Notes, wait
half a second, paste", and also variables, conditions, loops, questions to the person and things the system can do.

## Contents

- [Building a flow](#building-a-flow)
- [Variables, decisions and loops](#variables-decisions-and-loops)
- [Filters and lists](#filters-and-lists)
- [Asking and the system](#asking-and-the-system)
- [Moving around the canvas](#moving-around-the-canvas)
- [The inspector and the variables](#the-inspector-and-the-variables)
- [Red and amber wires](#red-and-amber-wires)
- [When a flow fails](#when-a-flow-fails)
- [Let an AI write one](#let-an-ai-write-one)
- [Sharing](#sharing)

## Building a flow

1. **+** makes a new flow. Give it a name.
2. **Add block** (top right) opens a list of every block, in groups. A flow always runs from **Start** to **End**: a
   block added this way is wired in after the selected one (before **End** when nothing is selected), and a block
   added in the middle of the chain goes between its neighbours. **Right-click** on the empty canvas opens the same
   list at the pointer; a block added that way is put exactly there and has no wires yet. **Start** and **End**
   cannot be removed.
3. Fill in each block: record a shortcut, type an app (or press **Pick...** to choose one of the installed apps,
   as in a slice), type a URL or a command, set the wait in milliseconds (at most 10 s, because actions run one at
   a time).
4. To change the order, drag from a dot on the right of a block to the dot on the left of another one. An output gives
   one wire, so a new wire replaces the old one; an input takes any number of wires, so two paths can join, and a wire
   may lead back to an earlier block, which makes a loop. Click a wired output dot to unplug that wire, a wired input
   dot to unplug every wire into it. The number in the header of a block is its place in the walk from **Start**; a
   block marked *not wired* never runs. **x**, or **Delete** / **Backspace** on the selected block (when no field is
   being typed in), removes a block and joins its neighbours.
5. In the **Menu** section choose a slice, set its action to **Flow** and pick the flow.

## Variables, decisions and loops

A run has *variables*: a name and a text. A block that computes, asks or looks at the system puts its result in one;
in any text field `{{name}}` is replaced by the value. After the name you can add **filters** that change how the value
is written, one after the other: `{{name|trim|upper}}` gives the value without the spaces around it, in capitals
(`{{name|sh}}` quotes it for the shell, use it inside a command). The full list is in
[Filters](#filters-and-lists) below. A condition or a calculation takes a value or the bare name of a variable.

| Block | What it does |
|---|---|
| **Set** | puts a text in a variable |
| **Calculate** | `result = a + b` (also `-`, `*`, `/`, `%`, min, max) |
| **Gate** | `result = a AND b` (also OR, XOR, NAND, NOR, NOT), as 1 or 0 |
| **If** | follows **then** when `a == b` (or `<`, `>`, contains, is empty...) holds, **else** when it does not |
| **Repeat** | goes through **body** a number of times, then **done**; the variable `index` counts the rounds from 1 |
| **While** | goes through **body** as long as its condition holds, then **done** |
| **List** | makes, changes and reads a **list** (an array) kept in a variable: make it from a text, add, take, get, change or remove an item, count, join into a text, find, sort, reverse, remove duplicates, shuffle, add up, empty |
| **For each item** | goes through **body** once for every item of a list, with the item in a variable and its position in `index`, then **done** |
| **Call** | runs another flow with the same variables |
| **Stop** | ends the whole run |

The body of a loop is the chain that starts at its **body** dot; wire its last block back to the loop block. A run
ends at **End**, at **Stop**, at a dot without a wire, after 10000 blocks or after 10 minutes, so a loop that never
ends is stopped for you.

## Filters and lists

**Filters.** `{{name|filter}}` writes the value of `name` through a filter; chain as many as you like, left to right.
A name that was never set can be given a stand-in with `default`. A filter that does not exist, or cannot work on the
value (`count` on a text that is not a list), leaves the whole `{{...}}` as it was written, and the editor warns you.

| Filter | What it gives |
|---|---|
| `sh` | the value quoted for the shell |
| `trim` `upper` `lower` `title` `reverse` | without the spaces around it, in capitals, in small letters, each word capitalized, backwards |
| `len` | how many characters |
| `int` `round` `abs` `num` | a number cut, rounded, without its sign, or written the plain way |
| `bool` | `1` or `0` |
| `json` | as a JSON text, quotes included |
| `url` | safe to put in a web address |
| `basename` `dirname` `ext` | the file name, the folder, the extension (without the dot) of a path |
| `count` `first` `last` | for a list: how many items, the first, the last |
| `join:, ` | for a list: the items with this text between them (`\n` is a line break) |
| `item:2` | for a list: the item at that position (from 1; `-1` is the last) |
| `default:none` | `none` when the value is empty, or the variable was never set |
| `replace:a:b` | every `a` replaced by `b` |

**Lists.** A list is kept in a variable as a JSON array of texts, like `["a","b"]`, so you can also show it, save it in
a file or hand it to a command. A **List** block works on the list in the variable named in its first field. Positions
count from 1; a negative one counts from the end. *Make from text* cuts a text at a separator (empty: at every line;
`\n` and `\t` stand for a line break and a tab). *Sort* sorts as numbers when every item is a number, as text
otherwise. The **List a folder** operation of the **System** block gives a list of the names in a folder. To do
something for every item, use **For each item**, and wire the last block of its body back to it.

## Asking and the system

| Block | What it does |
|---|---|
| **Ask** | shows a window of the system with a question and puts the answer (a text, or 1 / 0 for yes / no) in a variable; the variable `ok` says whether it was answered. On Linux it needs `zenity` or `kdialog` |
| **Run and wait** | runs a shell command, waits for it (1 to 120 s) and keeps what it printed and its exit code (`code`); every variable is in its environment as `CURSORPIE_<NAME>` |
| **System** | one of: copy to / read the clipboard, type a text into the app in front, a notification, read / write / append a file, whether a file exists, an environment variable, the app in front, bring an app forward, whether an app has a window, the pointer position, a random number, the time, show a file in the file manager; and **list a folder**, **make a folder**, **copy**, **move or rename** and **delete a file** (never a folder), a file's **size** and **change time**, facts about **this computer** (system, processor, user, host, home and temporary folder), **speak a text aloud**, **save a screenshot** to a PNG file, **read** and **set the volume** |

A block that runs a command (**Command**, **Run and wait**) or writes a file is drawn in red, and importing a flow or a
profile with one warns you before you save. The clipboard, notifications and questions use the tools of each system
(`pbcopy`, PowerShell, `wl-copy` / `xclip`, `osascript`, `notify-send`...); when one is missing the block does
nothing and `ok` is 0.

## Moving around the canvas

Drag the header of a block to move it. The canvas has a dot grid and moves like any node editor: drag the empty
canvas, hold the **middle button** anywhere on it (even though it is the trigger button, CursorPie lets it through
over this canvas while Settings is open), or scroll with two fingers on a trackpad or the mouse wheel. Hold
**Alt** (**Option** on a Mac) and scroll, pinch, or hold Ctrl/Cmd and scroll to zoom between 30% and 400%; the
text is laid out again at every zoom, so it stays sharp like a vector drawing. The button with the arrows brings the blocks back into view at
100%. Flows are saved with **Save**, and **Undo** (Cmd+Z) works in this section too. The button with four squares
(**Tidy**) lines the blocks up in columns, left to right, so none covers another.

## The inspector and the variables

 Click a block (its header, or anywhere in its body) and a panel opens at the
right of the canvas. Its **Block** page has the settings of the block, to read and edit with room to spare, the
problems of the block in words, and what it does with variables: **Reads** (each variable it uses, whether a block
before it sets it, and the blocks its value may come from) and **Writes** (each variable it sets and the blocks that
read that value afterwards). Every block named there is a button that selects it and brings it to the middle of the
canvas. The **Variables** page lists every variable of the flow with how many blocks set and read it (`!` marks one
that is read where nothing sets it). Click a variable, here or in a block's page, to **follow** it: the blocks that set
it get a **green** ring, the blocks that read it a **blue** one, and dashed lines show the way its value travels (it
stops at the next block that sets the same variable). Click it again to stop. The button next to the full-screen one,
under the canvas, opens the panel when no block is selected (it then shows the variables); a click on the empty
canvas deselects the block and closes the panel.

## Red and amber wires

 Any wire can be drawn, but not every wire can work. The editor checks the flow while you
draw, without running it: a wire that **cannot work** turns **red** and one that is **probably a mistake** turns
**amber**. Each has a `!` in the middle (hover it to read why), the block it leads to is outlined in the same color
with the same mark on its corner, and the strip under the canvas counts them. Nothing is blocked: you can still save
and run the flow. The usual red ones:

- a block uses `{{name}}` but no block before it sets `name` (add a **Set variable**, an **Ask**, a **Run and wait**
  or a **System** block that puts its result in `name`);
- the body of a **Repeat** or a **While** never leads back to it (wire the last block of the body to the left dot of
  the loop), or a loop has no **If / else**, **Repeat** or **While** to leave it;
- a **Call flow** block points at a flow that does not exist;
- a **Calculate** block has a value that is not a number, or divides by zero.

The usual amber ones: a shortcut right after **Open app** with no **Wait** between them, a **Shell command** that is
not waited for before a block that needs its result, a wire out of a block that nothing leads to, an empty block.

## When a flow fails

 A block that cannot do its work (a command that ends with an error, a file that cannot be
read, a clipboard that does not answer, a **Call flow** to a flow that is gone...) does not stop the flow: it sets
`ok` to 0 and the flow goes on. If the flow does **not** look at that (no **If / else** or **While** after it reads
`ok` or `code`), CursorPie tells you: a desktop notification names the block and the reason, the block is outlined
in red in the **Flow** section (hover its `!`), and while Settings is open a sheet lists every problem in full. A
failure that the flow handles itself is never reported, so a command that is *meant* to fail sometimes (a `grep`
that finds nothing) stays quiet as long as an **If / else** looks at its `ok` or `code`.

## Let an AI write one

 Press **New flow** and choose **Build it with AI**. Say in a few words what the flow should do and
press **Copy kit**: CursorPie puts a zip on the clipboard (a guide for the agent, the file format, examples and what you
wrote). Paste it into the AI agent you use, add a prompt such as "make this", save the file it gives you as
`<name>.cursorpie-flow.json` and press **Import result** (or **Import** in the Flow section). Nothing is sent from
CursorPie. If your agent does not take a pasted file, **Show in folder** opens the zip so you can attach it, and **Copy
path** is for agents that run in a terminal. On Linux this needs `wl-copy` (Wayland) or `xclip` (X11). The file format itself is
documented for agents in [FLOWS_FOR_AI.md](../FLOWS_FOR_AI.md), so you can also give them that page.

## Sharing

 **Export** (the arrow up) writes `<name>.cursorpie-flow.json` to your Downloads folder; **Import** (the
arrow down) adds such a file as a **new** flow and never overwrites one of yours. A shared profile (see
[Sharing a profile](settings.md#sharing-a-profile)) carries the flows its slices use. **Only import flows you trust:** a
*Shell command* block runs whatever it says. CursorPie warns after importing one, and nothing runs until you Save
and use the slice. If you delete a flow that a slice uses, that slice does nothing until you pick another flow.
