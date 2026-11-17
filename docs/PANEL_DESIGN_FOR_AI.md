# Designing panels (a guide for AI agents)

This page tells you **how to build a good panel**, not only what the JSON looks like. `PANELS.md` is the reference for
every field and limit; read it too. This page adds what the reference leaves out: when a panel is the right tool, how
big things are, which choices look right, how a panel is wired into a flow, and the mistakes that agents make most.

A panel is a small window of CursorPie. You describe it as JSON, a flow shows it with an **Ask** block in **Panel**
mode, and what the person did comes back as variables. You never write drawing code: the look (colors, fonts, round
corners, dark mode) is fixed by CursorPie. Your job is to choose **which widgets**, **in which order**, **with which
words**, and **how the flow reacts**.

## 1. Is a panel the right tool?

| The flow needs...                                          | Use                                                                                  |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| one line of text                                           | `Ask` mode `Text`                                                                    |
| yes or no                                                  | `Ask` mode `YesNo`                                                                   |
| two or more answers at once, a choice, a switch, a number  | `Ask` mode `Panel` (a **form**)                                                      |
| a list to pick one item from                               | `Ask` mode `Panel` with one `choice` and `"submit": true` (a **menu**)               |
| a set of buttons that each do something (a remote control) | `Ask` mode `Panel` with a `buttons` grid, `keep_open` and a loop (a **tool panel**)   |
| to show how far a long job is                              | `PanelUpdate` blocks (a **progress panel**), nothing waits for it                    |
| only to announce something                                 | `Native` `Notify`                                                                    |

Do not use a panel for a single question that `Text` or `YesNo` answers: those are faster for the person.
A panel is not a web page: there are no images (only icons on buttons), no tables, no side-by-side fields, no file
chooser. If the request needs those, say so instead of faking it.

## 2. The steps to follow

1. Say what the person must **decide** (the fields) and what must **happen** after each button (the flow).
2. Pick the shape: form, menu, tool panel or progress panel (section 5).
3. Write the panel JSON with the rules of section 4.
4. Put it in an **Ask** block (section 3) and wire what comes after it (section 6).
5. Run the checklist of section 9 and, if you can run code, the script at the end.
6. In your answer, describe the panel in words (title, fields, buttons) so the person can check it against what they
   asked for.

## 3. Putting a panel in a flow file

The panel is a **string** inside the flow file: the value of `message` in an `Ask` block whose `mode` is `"Panel"`.
That is JSON inside JSON, and it is the most common place to go wrong:

```json
{ "id": 2, "x": 240.0, "y": 60.0,
  "kind": { "Ask": { "mode": "Panel", "out": "button", "preset": "",
    "message": "{\"id\":\"greeting\",\"title\":\"Say hello\",\"widgets\":[{\"type\":\"input\",\"id\":\"name\",\"label\":\"Name\",\"required\":true}],\"buttons\":[{\"id\":\"cancel\",\"label\":\"Cancel\",\"cancel\":true},{\"id\":\"go\",\"label\":\"Say it\",\"tone\":\"primary\"}]}" } },
  "next": 3 }
```

- Write the panel as a normal JSON object first, then turn it into a string: **every `"` becomes `\"`**, every newline
  `\n` (or write it on one line), every backslash `\\`. If you can run code, do it with `json.dumps(json.dumps(panel))`
  style code instead of by hand.
- The panel text may be at most **16 384 characters** (`MAX_PANEL_CHARS`). A bigger panel is refused. Keep it small:
  that also makes it a better panel.
- `out` names the variable that gets the **id of the button** that was pressed. `ok` is `1` when a button that confirms
  was pressed and `0` when the person cancelled (Cancel, Esc, the X) or it timed out. A cancelled panel is an answer,
  not an error: always test `ok` first.
- Every field with an `id` sets the variable of the same name (`{{name}}`). A toggle is `1`/`0`, a `multi` is a JSON
  list, a number has no useless `.0`.
- `{{variable}}` works **inside the texts** of the panel (`title`, `subtitle`, `label`, `text`, option labels, `value`).
  It is filled in after the JSON was read, so a value can never add a widget.
- Flow files with an `Ask` in `Panel` mode need `"cursorpie_flow": 2`.

## 4. Design rules

### One screen, one purpose

- **One decision per panel.** A panel that asks for the name, the schedule and the file format is three panels or a
  settings form (see below). If the answer to "what is this panel for?" needs the word "and", split it.
- **Title: what the person is doing**, as a short noun phrase or verb phrase, at most about 30 characters
  ("Rename tab", "Send reply", "Timer"). The limit is 80, but the header is one line, so a long title crowds the badge and the X. `subtitle` is for one
  quieter line (what happens next, or the thing being acted on). Do not repeat the title in it.
- No decorative text. Every `text` widget must tell the person something they need to decide. Use `style: "muted"` for
  secondary lines, `heading` to start a group, `code` for something they may copy.

### Widgets: pick the smallest one that fits

| The person gives...                  | Widget                         | Notes                                                                                          |
| ------------------------------------ | ------------------------------ | ---------------------------------------------------------------------------------------------- |
| a short text                         | `input`                        | `hint` shows what to type; `required: true` when the flow cannot go on without it              |
| a long text                          | `input` with `multiline: true` | a box about 92 points high; give `height` (24 to 400) for more                                 |
| a secret                             | `input` with `secret: true`    | shows dots                                                                                     |
| on or off                            | `toggle`                       | label says what is **on** ("Keep the window open"), never a question                           |
| a number with a known range          | `slider`                       | `min`, `max`, `step`, `unit`                                                                   |
| an exact number                      | `number`                       | `-`, `+` and a box; `min`, `max`, `step`, `unit`                                               |
| one of 2 to 4 short options          | `choice` with `layout: "chips"`| a row of round chips; the best look for sizes, modes, priorities                               |
| one of 5 to 8 options                | `choice` (`list`, the default) | shows the options as rows (`hint` gives a second line)                                         |
| one of many options                  | `choice` with `layout: "dropdown"` or a list with `search` | a list of more than 8 gets a search box on its own                |
| several of some options              | `multi`                        | the value is a list: use it with `ForEach`                                                      |
| a color                              | `color`                        | answers `#rrggbb`                                                                              |
| a pick that answers at once          | `choice` with `submit: true`   | a **menu**: no OK button, the button id is `pick`                                              |
| actions, each a button               | `buttons` grid                 | see "Tool panels"                                                                              |

- **At most 6 or 7 fields in a form.** The limit is 24 widgets, but a form that does not fit a display is bad design.
  More than that: split into steps (several panels) or use `show_if`.
- **Order = the order the person thinks in**: the thing they came for first, options after, switches last. Put the
  field they must fill first, because it gets the keyboard.
- **Give every field a `label`** (short noun: "Name", "Delay", "Folder"). A label is not a sentence and has no colon.
- **Give a starting value** (`value`) whenever a sensible default exists: the person then only presses Enter.
- **Use `show_if` to hide what does not matter yet** (an "Advanced" switch that reveals three fields) instead of
  showing everything. The window is sized for all widgets, hidden or not, so a hidden field leaves empty room: keep the
  conditional widgets **at the end** of the panel, just above the buttons.
- **Group with `separator` and a `heading` text** only when the panel has more than about four fields. Never put a
  separator at the very top or bottom.
- **Keep fields in the same panel related.** A `required` field that is hidden does not block the confirming buttons.

### Buttons

- **At most 4 buttons** in the bottom row, and normally **two**: one that cancels on the left, one primary on the
  right (`buttons_align` `end` is the default).
- **Exactly one `primary`** button: the main action. Enter presses it. Do not make every button primary. (A tool panel whose only bottom button is a **Close** has none.)
- **The cancelling button has `"cancel": true`** (Esc presses it) and an id such as `cancel`. A cancel button can
  never be a `toggle`.
- **Labels are verbs that say what happens**: "Send", "Rename", "Start timer", "Close tabs" – not "OK", "Yes", "Submit".
  Use `tone: "danger"` for the button that deletes or closes something that cannot be undone.
- A panel with **no `buttons`** gets Cancel and OK. A menu (`submit` choice) gets only Cancel. If you want neither,
  set `"no_buttons": true` (tool panels and menus that answer by themselves).
- Ids of buttons are **unique over the whole panel**, including the buttons of every `buttons` grid.

### Size and place

| Setting     | Rule of thumb                                                                                                                              |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `width`     | The default **380** suits a form. **320 to 340** for a tool panel or a menu. **440 to 520** only for a panel with a `multiline` box or long option labels. Range 280 to 640. |
| `height`    | **Leave it out.** The panel is as tall as its content. Set it only for a long list or text, and then it scrolls and the buttons stay at the bottom. Range 200 to 900. |
| `place`     | `cursor` (default) for a quick question about what the pointer is on. `center` for a form the person fills in. `bottom` for a remote control that must stay out of the way. `top` for a status panel. |
| `timeout_s` | The default 120 suits a question. A tool panel that stays open uses `0` (never). Never above 600.                                          |
| `padding`   | Leave it out. 18 points on every side is what the rest of CursorPie uses. Change one side only for a reason (`{ "bottom": 22 }` for a panel with big round buttons). |
| `accent`    | Leave it out. The panel then uses the accent the person chose in Settings. Set it only when the panel is for something with its own color, and then use one color for the whole panel. |
| `icon`      | The round badge in the header: `app`, `link`, `file`, `shortcut`, `command`, `browser`, `flow` (default). Choose the one that names the subject. |

Sizes you can count on (points): padding 18, space between widgets 12, header 40 (+20 with a subtitle), button, text
box, switch row and menu row 36, label above a field 24, line of text 20, `multiline` box 92, chip 32, progress bar 8,
separator 9, gap between buttons 8. A form of a header, three single-line fields with labels, and a button row is about
18 + 40 + 3 × (24 + 36 + 12) + 12 + 36 + 18 ≈ **340** points high. A panel taller than the display scrolls between the
header and the buttons, so it still works, but it is not a good panel.

### Text styling: almost never

`fmt`, `value_fmt`, `title_fmt`, `subtitle_fmt` and `label_fmt` (size, color, bold, alignment) exist for **special
panels**: a big display for a calculator, a centered "Done!" message, a big timer. A form does not use them. Do not
color text to make it "nicer": the default look is already the design of CursorPie. If you do use them:
`align: "center"` for a short message, `size` 20 to 32 for a display, `color` only for meaning (green `#16a34a` for
done, red `#dc2626` for a failure), and **never below size 11**.

### Icons

- A button may have an `icon` and no `label` only when the icon is universal (`play`, `pause`, `skip-back`,
  `skip-forward`, `speaker-high`, `speaker-slash`, `plus`, `minus`, `x`, `check`, `arrows-clockwise`, `trash`).
  Otherwise give a `label` too: a button with an icon **and** a label reads best.
- Names must exist (an unknown name draws nothing). The ones below are in the app (`ico:` in front is allowed): `play pause stop skip-back skip-forward
  rewind fast-forward speaker-high speaker-slash microphone camera video-camera music-notes arrows-out arrows-in
  arrow-left arrow-right arrow-up arrow-down arrows-clockwise swap check x plus minus check-circle x-circle warning
  question star heart flag bookmark-simple pencil-simple trash copy clipboard-text scissors link paperclip push-pin
  magnifying-glass-plus magnifying-glass-minus lock-simple lock-simple-open eye eye-slash bell clock calendar-blank
  house gear-six sun moon wifi-high bluetooth battery-full monitor laptop keyboard mouse-simple printer folder-simple
  folder-open files note-pencil envelope-simple archive package cloud rocket-launch book-open chart-bar file-text
  browser globe terminal-window terminal command option text-aa code translate scan lightning fire tag lightbulb key`.
  The panel designer lists all of them. An icon name that does not exist shows nothing: do not invent one.
- Picture files (`file:/path/to/picture.png`) work only in a panel of a flow. Do not use them unless the person gave a
  path.

## 5. The four shapes, with a model of each

### Form

Fields, then **Cancel** and one primary button. The flow reads the variables.

```json
{
  "id": "new-reminder",
  "title": "New reminder",
  "subtitle": "It shows up as a notice",
  "place": "center",
  "widgets": [
    { "type": "input", "id": "text", "label": "Reminder", "hint": "What should I remind you of?", "required": true },
    { "type": "choice", "id": "when", "label": "In", "layout": "chips", "options": ["5 min", "15 min", "1 hour"], "value": "15 min" },
    { "type": "toggle", "id": "repeat", "label": "Repeat every time" },
    { "type": "slider", "id": "times", "label": "Repeat", "min": 2, "max": 10, "step": 1, "value": 3, "unit": "times",
      "show_if": { "id": "repeat" } }
  ],
  "buttons": [
    { "id": "cancel", "label": "Cancel", "cancel": true },
    { "id": "save", "label": "Set reminder", "tone": "primary" }
  ]
}
```

Why it is good: one purpose, the field that must be filled comes first and has the keyboard, the chips have a default,
the optional slider appears only when it matters and is **last**, the primary button says what it does.

### Menu

A `choice` with `submit: true`. The person picks, the panel answers at once with the button id `pick` and the value in
the variable of the choice. No OK.

```json
{
  "id": "send-to",
  "title": "Send to",
  "place": "cursor",
  "widgets": [
    { "type": "choice", "id": "target", "submit": true,
      "options": [
        { "value": "mum", "label": "Mum", "hint": "WhatsApp" },
        { "value": "team", "label": "Team chat", "hint": "Telegram" },
        { "value": "me", "label": "Me" }
      ] }
  ]
}
```

Keep a menu to **8 options or fewer** (more gets a search box: then Enter picks the first match, which is fine for
long lists). Options are short; use `hint` for the second line. No title is needed when the place makes it obvious.

### Tool panel (a remote control)

A panel that **stays open** and whose buttons each run an action. This is the only kind that needs a loop in the flow
(section 6). Rules: `"keep_open": true`, `"no_buttons": true` (or one plain **Close** button with `"cancel": true`, as `browser-remote-panel` has), an `id`, `"timeout_s": 0`, `width` 320 to 340, `place`
`bottom`, a `Native` `Quiet` block right after `Start`. Buttons live in `buttons` grids, **3 per row at most**, the
most used one first and `primary`; related controls share a grid.

```json
{
  "id": "remote",
  "title": "Music",
  "icon": "browser",
  "place": "bottom",
  "width": 330,
  "timeout_s": 0,
  "keep_open": true,
  "no_buttons": true,
  "widgets": [
    { "type": "buttons", "columns": 3, "buttons": [
      { "id": "prev", "icon": "skip-back", "height": 56, "radius": 28 },
      { "id": "toggle", "icon": "play", "tone": "primary", "toggle": true, "on_icon": "pause", "bind": "keep_playing", "height": 56, "radius": 28 },
      { "id": "next", "icon": "skip-forward", "height": 56, "radius": 28 }
    ] },
    { "type": "buttons", "columns": 3, "buttons": [
      { "id": "vol_down", "icon": "minus" },
      { "id": "mute", "icon": "speaker-high", "toggle": true, "on_icon": "speaker-slash", "on_tone": "danger", "bind": "keep_muted" },
      { "id": "vol_up", "icon": "plus" }
    ] }
  ]
}
```

- **A button that changes** (play/pause, mute) uses `toggle: true`, `on_icon` or `on_label`, and `bind` with a
  `keep_` variable. The flow must **not** flip that variable itself: the panel writes it after each press. Set a
  default before the first `Ask` (`Set keep_playing = {{keep_playing|default:0}}`).
- Big round buttons: `height` 56 to 72 and `radius` up to 36 (a pill). Keep the same height inside a grid.
- With `no_buttons` the X at the top right closes the panel. A tool panel the person leaves open all day usually keeps
  it.
- The window comes back in the same place after each press (the person may have moved it), because the next `Ask`
  uses the **same panel `id`**.

### Progress panel

Not a form: no `Ask`. Use `PanelUpdate` blocks: `Open` (title, label, value, optional `stop: true`), `Update`
(label and value change; a field left empty stays), `Close`. `value` is a fraction `0.4`, a percentage `40%` or
`busy`. Put a `Calc` (`index` divided by the count) before each `Update` to get the fraction. Example:
`examples/flows/progress-panel.cursorpie-flow.json`. It never takes the keyboard, so `Key` and `TypeText` blocks still
reach their app. Do not use `Ask` panels to show progress.

## 6. Wiring the flow around a panel

**A form** (once):

```
Start -> Ask(Panel, out=button) -> Branch ok IsTrue --yes--> (use {{name}}, {{size}}...) -> End
                                                    \--no---> End          (cancelled: do nothing)
```

**Several buttons** (the form has a `Save` and a `Delete`): after the `ok` test, chain `Branch` blocks on `button`:
`Branch button Eq save` -> save path, `alt` -> `Branch button Eq delete` -> ..., and the last `alt` goes back to the
`Ask` (or to `End`). Every `Branch` has both wires.

**A menu**: `Ask(Panel, out=button)` -> `Branch ok IsTrue` -> use the variable of the choice (it holds the `value` of
the option) -> maybe `Branch target Eq mum`, and so on.

**A tool panel** (loops forever, until the person closes it):

```
Start -> Native Quiet on -> Set keep_x defaults -> Ask(Panel, out=button) <-----------------+
                                                     |                                       |
                                          Branch ok IsTrue --no--> End                       |
                                                     | yes                                   |
                              Branch button Eq a --yes--> action a --(refresh state)---------+
                                                     | no
                              Branch button Eq b --yes--> action b --(refresh state)---------+
                                                     | no
                                                  ... last alt goes back to the Ask ---------+
```

Every action path **ends back at the same `Ask` block**; the last unmatched `alt` also goes back. Without the loop the
panel answers once and the flow ends. `docs/examples/flows/browser-remote-panel-state.cursorpie-flow.json` is a whole
working one.

Other things the flow must do:

- Read `ok` **before** the variables of the panel. After a cancel the field variables are not trustworthy.
- If the panel text comes from the person or from a page, put it in a **variable** and use `{{var}}` in a `text`
  widget; do not paste it into the JSON.
- A flow that shows a panel and then presses keys (`Key`, `TypeText`) must give the keyboard back to the app first:
  the panel takes it. Use `Native` `ActivateApp` and a `Wait` of 200 to 400 ms after the panel answers, or use a `passive`
  panel (progress only).
- Panels can be switched off (*Behavior > Panels of flows*). Then the `Ask` fails with `ok` 0: your `no` path must
  end sensibly (a `Notify`, then `End`).

## 7. The most common mistakes

| Mistake                                                                                  | What happens                                  | Fix                                                                |
| ---------------------------------------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------------------------ |
| The panel JSON is pasted into `message` as an object, not as an escaped string           | the flow file does not load                   | `message` is a **string**: escape every quote (section 3)          |
| A widget id is `ok`, `index` or `code`, starts with a digit, or has a `-`                | the panel is refused with a reason            | letters, digits and `_`; unique; not those three                   |
| A button id is used twice (also across grids)                                            | refused                                       | unique ids over the whole panel                                    |
| More than 4 buttons in the bottom row                                                    | refused ("a panel has at most 4 buttons")     | put the rest in a `buttons` grid                                   |
| Every button is `primary`                                                                | no visible main action                        | one `primary`                                                      |
| Labels "OK", "Yes", "Submit"                                                             | the person must read the title to understand  | a verb: "Send", "Rename"                                           |
| A `show_if` that names a field of another panel, or one that does not exist             | refused                                       | only fields of the same panel                                      |
| `show_if` widgets in the middle of the panel                                             | empty gaps (the room is kept)                 | conditional widgets last                                           |
| A slider with `min` not below `max`; a `number` without range for a bounded value        | refused / bad values                          | give a real range and a `step`                                     |
| An `options` list with empty or repeated entries                                         | dropped silently                              | clean values; at most 60                                           |
| A cancel button that is `toggle`                                                         | the toggle is ignored (a cancel keeps no state) | cancel buttons are plain                                         |
| `keep_open` panel without a loop in the flow                                             | answers once, then the flow ends              | loop back to the `Ask` with the same panel `id`                    |
| The flow flips the `bind` variable itself                                                | the button shows the opposite of the state    | the panel writes it after each press; the flow only acts           |
| A `keep_` variable has no default                                                        | first look is whatever `on` says              | `Set` it with `{{keep_x\|default:0}}` before the `Ask`             |
| `height` set "to make room"                                                              | wasted space, or scrolling                    | leave it out                                                       |
| `fmt` colors and sizes everywhere                                                        | the panel looks unlike the rest of CursorPie  | leave the default look                                             |
| 10 fields in one panel                                                                   | a tall form that scrolls                      | split it, or use `show_if`                                         |
| An icon name that does not exist                                                         | an empty button                               | use the list in section 4                                          |
| Testing a variable of the panel before testing `ok`                                      | acts on a cancelled panel                     | `Branch ok IsTrue` first                                           |
| A panel for a one-line question                                                          | slower than `Ask` `Text`                      | use `Text` or `YesNo`                                              |

## 8. Limits (the app refuses what is outside them)

Widgets 24 · bottom buttons 4 · buttons per grid 24, 48 in all grids · `columns` 1 to 6 · options 60 · `width` 280 to
640 · `height` 200 to 900 · `padding` 0 to 60 · `timeout_s` 0 to 600 · text size 8 to 48 · button `width` 24 to 640,
`height` 24 to 72, `radius` 0 to 36 · `spacer` 2 to 200 · the description 64 KiB, the text in a flow block 16 KiB ·
title 80 characters, subtitle and labels 120, a `text` 1 000, a value 2 000. What is not usable is refused **with a
reason** (it is shown in the run report), it is not silently changed (texts that are too long are cut).

## 9. The checklist before you answer

- [ ] I chose the right tool (section 1) and the right shape (section 5).
- [ ] The panel does **one** thing; the title says what, in about 30 characters or less.
- [ ] 7 fields or fewer; each has a `label`, and a default `value` where one makes sense.
- [ ] Every id is unique, valid and not `ok`, `index` or `code`; `show_if` names a field of the same panel.
- [ ] Bottom row: Cancel (`"cancel": true`) and **one** `primary` with a verb. At most 4 buttons. Or `no_buttons`
      on purpose.
- [ ] No `height`, `padding`, `accent` or text styling unless I can say why.
- [ ] A tool panel has `keep_open`, `no_buttons`, `timeout_s` 0, an `id`, `Quiet`, and a loop that returns to the
      same `Ask`.
- [ ] `message` is an escaped **string**; the flow file has `"cursorpie_flow": 2`.
- [ ] After the `Ask`: `Branch ok IsTrue` comes first; every `Branch` has both wires; every path ends.
- [ ] Icons are from the list; a button without a label has a universal icon.
- [ ] My answer describes the panel (title, fields, buttons) and what each button does.

If you can run code, check the panel (not the flow) with this script. It prints every problem it finds:

```python
import json, re, sys

def check_panel(p):
    bad = []
    ids, buttons = [], []
    w = p.get("widgets", [])
    if len(w) > 24: bad.append("more than 24 widgets")
    if len(w) > 7: bad.append(f"{len(w)} widgets: is this one purpose? (soft limit 7)")
    if len(p.get("title", "")) > 80: bad.append("title over 80 characters")
    elif len(p.get("title", "")) > 30: bad.append("title over 30 characters (soft)")
    if not 280 <= p.get("width", 380) <= 640: bad.append("width must be 280 to 640")
    if "height" in p: bad.append("height set: is it needed?")
    if not 0 <= p.get("timeout_s", 120) <= 600: bad.append("timeout_s must be 0 to 600")
    bb = p.get("buttons", [])
    if len(bb) > 4: bad.append("more than 4 bottom buttons")
    only_close = len(bb) == 1 and bb[0].get("cancel")  # a tool panel may have just a Close button
    if bb and not only_close and sum(b.get("tone") == "primary" for b in bb) != 1: bad.append("bottom row needs exactly one primary")
    if bb and not any(b.get("cancel") for b in bb) and not p.get("no_buttons"): bad.append("no cancelling button")
    if p.get("keep_open") and not p.get("id"): bad.append("keep_open needs a panel id")
    kinds = {"text","separator","input","number","toggle","slider","color","choice","multi","progress","buttons","spacer"}
    fields = {x.get("id") for x in w if x.get("type") in {"input","number","toggle","slider","color","choice","multi"}}
    grid = 0
    for x in w:
        t = x.get("type")
        if t not in kinds: bad.append(f"unknown widget type {t!r}")
        i = x.get("id")
        if t in {"input","number","toggle","slider","color","choice","multi"}:
            if not i or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", i) or i in ("ok","index","code"):
                bad.append(f"bad field id {i!r}")
            if not x.get("label") and t != "choice": bad.append(f"field {i!r} has no label")
        if i: ids.append(i)
        if t in ("slider",) and not x["min"] < x["max"]: bad.append(f"slider {i!r}: min must be below max")
        if t in ("choice","multi") and len(x.get("options", [])) > 60: bad.append(f"{i!r}: over 60 options")
        s = x.get("show_if")
        if s and s.get("id") not in fields: bad.append(f"show_if names {s.get('id')!r}, not a field of this panel")
        if t == "buttons":
            grid += len(x.get("buttons", []))
            if len(x.get("buttons", [])) > 24: bad.append("a grid has more than 24 buttons")
            buttons += x.get("buttons", [])
    buttons += bb
    if grid > 48: bad.append("more than 48 grid buttons")
    for b in buttons:
        if not b.get("id"): bad.append("a button has no id")
        if b.get("cancel") and b.get("toggle"): bad.append(f"button {b.get('id')!r}: a cancel button cannot toggle")
        if not b.get("label") and not b.get("icon"): bad.append(f"button {b.get('id')!r}: no label and no icon")
        if b.get("label") in ("OK", "Yes", "Submit"): bad.append(f"button {b.get('id')!r}: use a verb")
    bid = [b.get("id") for b in buttons]
    if len(set(bid)) != len(bid): bad.append("a button id is used twice")
    if len(set(ids)) != len(ids): bad.append("a widget id is used twice")
    return bad

flow = json.load(open(sys.argv[1]))["flow"]
for n in flow["nodes"]:
    k = n["kind"]
    if isinstance(k, dict) and k.get("Ask", {}).get("mode") == "Panel":
        text = k["Ask"]["message"]
        assert len(text) <= 16384, "panel text over 16 KiB"
        for problem in check_panel(json.loads(text)):
            print(f"block {n['id']}: {problem}")
print("checked")
```

## 10. Ready-made examples

All are in `examples/flows/` and load without a red wire:

| File                                             | Shape         | What to learn from it                                                           |
| ------------------------------------------------ | ------------- | ------------------------------------------------------------------------------- |
| `panel-form.cursorpie-flow.json`                 | form          | the smallest form: input, chips, toggle; `ok` test; `Notify` with the values    |
| `panel-settings-form.cursorpie-flow.json`        | form + menu   | sections, `show_if` last, a menu before the form, several buttons, a cancel path |
| `progress-panel.cursorpie-flow.json`             | progress      | `PanelUpdate` Open / Update / Close, `Calc` for the fraction                    |
| `browser-remote-panel.cursorpie-flow.json`       | tool panel    | `keep_open`, `no_buttons`, the loop back to the `Ask`                           |
| `browser-remote-panel-state.cursorpie-flow.json` | tool panel    | the same with `toggle` + `bind` + `keep_` defaults                              |

Start from the closest one and change it; it is safer than starting from nothing.
