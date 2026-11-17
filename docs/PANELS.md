# Panels: small interactive windows for flows and extensions

The notice of what CursorPie is doing (the pill at the bottom of the display) only _announces_. A **panel** is its
interactive sibling: a small window of CursorPie with a title, **widgets** (text, boxes to type in, switches, sliders,
lists, a progress bar) and up to four **buttons**. It is drawn in the look of Settings, it takes the keyboard, and it
hands back what the person entered.

> **Writing a panel for a flow, as an AI agent or by hand?** This page is the reference of the format. How to *design* a
> good panel (which widget, how big, how the flow is wired around it, the usual mistakes) is in
> [PANEL_DESIGN_FOR_AI.md](PANEL_DESIGN_FOR_AI.md). Read that one first.

A panel is plain data (JSON). That is the point: it is the **base** that flows, the browser extension and anything you
add later build on, instead of each one needing its own window code.

```
              describes                 shows                   answers
 flow / extension / your code ──► PanelSpec (JSON) ──► the window ──► PanelResult (values + button)
```

| Layer       | Where                                                  | What it is                                                                        |
| ----------- | ------------------------------------------------------ | --------------------------------------------------------------------------------- |
| Description | `src/panel/spec.rs`                                    | `PanelSpec`: the JSON below, with limits. Safe to take from anywhere              |
| State       | `src/panel/form.rs`                                    | `Form`: what is typed and picked, how it becomes an answer, live `Patch`es        |
| Transport   | `src/panel/broker.rs`                                  | `panel::ask(spec, source)`: any thread asks and waits; who may ask                |
| Window      | `src/ui/app/panel.rs`, `src/ui/settings/panel_form.rs` | The fourth mode of the one window; draws a `Form` and reports what the person did |

A custom look replaces only the last row: `panel_form::draw` paints a `Form` and returns an `Event`; every rule (what an
empty required box does, what a number snaps to, when the window closes) lives above it.

## Who may show a panel

| Caller                                    | Allowed when                                                         | Setting (Settings, Behavior, Apps)      |
| ----------------------------------------- | -------------------------------------------------------------------- | --------------------------------------- |
| A flow (an _Ask_ block in **Panel** mode) | `panels` is on (default **on**: you wrote the flow)                  | _Panels of flows_                       |
| The browser extension                     | `browser_bridge` **and** `extension_panels` are on (default **off**) | _Let the browser extension show panels_ |

The extension is off by default because a panel is content that something outside CursorPie puts on your screen.
Everything an extension writes in a panel (for example the titles of your open tabs in `ui.tabs`) is shown on screen
only, never stored and never sent anywhere. A panel from the extension waits at most 5 minutes.

## The format

```json
{
    "id": "greeting",
    "title": "Say hello",
    "subtitle": "A panel made by a flow",
    "icon": "flow",
    "place": "center",
    "width": 380,
    "height": 420,
    "timeout_s": 120,
    "accent": "#ff8a00",
    "widgets": [
        {
            "type": "input",
            "id": "name",
            "label": "Name",
            "hint": "Who is it for?",
            "required": true
        },
        {
            "type": "choice",
            "id": "size",
            "label": "Size",
            "layout": "chips",
            "options": ["small", "medium", "large"],
            "value": "medium"
        },
        { "type": "toggle", "id": "loud", "label": "Shout it" }
    ],
    "buttons": [
        { "id": "cancel", "label": "Cancel", "cancel": true },
        { "id": "go", "label": "Say it", "tone": "primary" }
    ]
}
```

### The panel

| Field               | Meaning                                                                                                                                                                                                                                                                                                                                               |
| ------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `id`                | Names the panel. A new panel with the same `id` replaces the open one; an update or a close finds it by it. Letters, digits and `_ - . :` (at most 64). Optional                                                                                                                                                                                      |
| `title`, `subtitle` | Heading and a quieter line under it (80 and 120 characters)                                                                                                                                                                                                                                                                                           |
| `icon`              | The badge: `app`, `link`, `file`, `shortcut`, `command`, `browser`, `flow` (default)                                                                                                                                                                                                                                                                  |
| `place`             | `cursor` (default, next to the pointer), `center`, `top`, `bottom`                                                                                                                                                                                                                                                                                    |
| `width`             | 280 to 640 points (default 380)                                                                                                                                                                                                                                                                                                                       |
| `padding`           | The space between the edge of the panel and what is in it, in points (0 to 60), for each side: `{ "top": 18, "right": 18, "bottom": 18, "left": 18 }`. A side left out keeps the usual 18, so `{ "bottom": 6 }` changes only the bottom. The designer edits it as one number for all sides, two (left and right, top and bottom) or one for each side |
| `height`            | 200 to 900 points. When missing the panel is as tall as its content needs. When set it keeps that height: a taller content scrolls and the buttons stay at the bottom                                                                                                                                                                                 |
| `timeout_s`         | Seconds until it gives up: `0` never, at most 600, default 120                                                                                                                                                                                                                                                                                        |
| `accent`            | `#rrggbb` for the badge and the main button; the accent of Settings when missing                                                                                                                                                                                                                                                                      |
| `widgets`           | At most 24                                                                                                                                                                                                                                                                                                                                            |
| `buttons`           | At most 4. When there are none: **Cancel** and **OK**, or only **Cancel** for a menu                                                                                                                                                                                                                                                                  |
| `no_buttons`        | `true`: no row of buttons at the bottom, not even Cancel and OK. The card is that much shorter and the **X** at the top right closes it. For a panel that answers through a `buttons` grid, a menu or the flow itself. Ignored when the panel has `buttons` of its own                                                                                |
| `hide_close`        | `true` leaves out the **X** at the top right that every panel has. It closes the panel like Esc: it presses the button that cancels, or answers as cancelled when there is none                                                                                                                                                                       |

### Widgets

Every widget has `"type"`. The ones that hold something the person enters have a required `id`: it names the value in
the answer (and the variable in a flow), so it must be a name: letters, digits and `_`, not starting with a digit, and
not `ok`, `index` or `code` (a flow sets those itself). Ids are unique in a panel.

| `type`      | Fields                                                                                                                                                                                                                                                                              | Answer                                                                                           |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `text`      | `text`, `style`: `body` (default), `heading`, `muted`, `code`; optional `id`                                                                                                                                                                                                        | none                                                                                             |
| `separator` | none                                                                                                                                                                                                                                                                                | none                                                                                             |
| `input`     | `id`, `label`, `hint`, `value`, `secret` (dots), `multiline`, `required`, `max_chars`, `value_fmt` (how the text inside the box looks), `width`, `height` (points; left out: as wide as the card, the usual height), `text_padding` (space between the edge and the text, per side) | text                                                                                             |
| `number`    | `id`, `label`, `value`, `min`, `max`, `step` (1 when missing), `unit`                                                                                                                                                                                                               | number (kept inside the range, snapped to `step`); `-` and `+` buttons and a box to type or drag |
| `toggle`    | `id`, `label`, `value`                                                                                                                                                                                                                                                              | `true` / `false`                                                                                 |
| `slider`    | `id`, `label`, `min`, `max`, `step`, `value`, `unit`                                                                                                                                                                                                                                | number (snapped to `step`)                                                                       |
| `color`     | `id`, `label`, `value` (`#rrggbb`)                                                                                                                                                                                                                                                  | the text `#rrggbb` (the accent swatches of Settings and the color picker of the system)          |
| `choice`    | `id`, `label`, `options`, `value`, `layout`: `list` (default), `chips` or `dropdown` (one line high however many options), `submit`, `search`                                                                                                                                       | the `value` of the option                                                                        |
| `multi`     | `id`, `label`, `options`, `values`                                                                                                                                                                                                                                                  | list of values                                                                                   |
| `progress`  | optional `id`, `label`, `value`: `0` to `1`, or negative for "busy"                                                                                                                                                                                                                 | none                                                                                             |

`options` are texts (`"red"`) or `{ "value": "red", "label": "Red", "hint": "a second line" }` (at most 60). A `choice`
with `submit: true` is a **menu**: picking an option answers at once, with the button `pick`, and there is no OK. A
list of more than 8 options gets a search box (`search: true` or `false` to choose); in a menu, Enter picks the first
match.

Every widget (a `separator` too) may carry `"show_if"`, so a form grows as the person answers:
`{ "id": "advanced" }` shows it while the field `advanced` is on (a switch that is on, a box that is not empty, a number
that is not 0, a list with something picked); `{ "id": "mode", "is": "custom" }` while that field holds `custom` (a `multi`
holds it among its picks); `"not": true` turns the test round. The field has to be a field of the same panel (the
description is refused otherwise), and a widget that waits for a hidden one is hidden too. A required box that is hidden
does not stop the confirming buttons. A hidden field still answers with the value it holds. The window is sized for
**all** widgets, hidden or not, so it never changes size: what is hidden leaves its room empty above the buttons.

### Text format

A `text` widget, the title, the subtitle and the labels above the fields can say how their words are written. The
format is an object with parts that are all optional; a part left out keeps the usual look, so
`{ "align": "center", "italic": true }` changes only those two things.

```json
{
    "type": "text",
    "text": "Done!",
    "fmt": { "align": "center", "bold": true, "size": 22, "color": "#16a34a" }
}
```

| Where                                                                                                                | Field          | Notes                                                                                                                       |
| -------------------------------------------------------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------------------------------------------------------- |
| a `text` widget                                                                                                      | `fmt`          | Any size from 8 to 48                                                                                                       |
| the text inside an `input` box (a note box, a display)                                                               | `value_fmt`    | Any size from 8 to 48; the box grows with it. A `secret` box shows dots, so `italic`, `underline` and `strike` do not apply |
| the panel's title                                                                                                    | `title_fmt`    | Size 12 to 28                                                                                                               |
| the panel's subtitle                                                                                                 | `subtitle_fmt` | Size 8 to 20                                                                                                                |
| the labels of every field (`input`, `number`, `toggle`, `slider`, `color`, `choice`, `multi`, `progress`, `buttons`) | `label_fmt`    | Size 8 to 20                                                                                                                |

| Part                            | Meaning                                                                                                                                                                                                               |
| ------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `align`                         | `left` (default), `center` or `right`                                                                                                                                                                                 |
| `bold`                          | `true` or `false`. Left out: what the style says (a heading, a title and a label are bold, the rest is regular), so `false` makes a heading light. The code style has no heavier weight, so `bold` does not change it |
| `italic`, `underline`, `strike` | `true` for slanted letters, a line under the text, a line through it                                                                                                                                                  |
| `size`                          | Points, 8 to 48 (the places above have a smaller top). Left out: the size of the style                                                                                                                                |
| `color`                         | `#rrggbb`. A value that is not one is dropped. Left out: the color of the style                                                                                                                                       |

Two more parts place a text by hand (the title, the subtitle and the text inside an `input`):

| Part       | Meaning                                                                                                                                                                        |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `valign`   | `top`, `middle` or `bottom`: where the text sits in the height of its box. Only `value_fmt` uses it. Left out: the middle of a one-line box, the top of a box of several lines |
| `dx`, `dy` | A nudge in points after `align` has put the text: `dx` to the right, `dy` down (negative: left, up), at most 60 either way                                                     |

The title and the subtitle start after the round badge, so `"align": "center"` puts them in the middle of the space
that is left, a little (23 points) to the right of the middle of the card. `{ "align": "center", "dx": 0 }` puts
them in the exact middle; the designer does this with _Center on the whole card_.

```json
{
    "type": "input",
    "id": "disp",
    "multiline": true,
    "value": "{{disp}}",
    "value_fmt": {
        "align": "right",
        "valign": "middle",
        "size": 28,
        "bold": true
    }
}
```

An `input` box also says its own size and the space around its text: `width` (60 up to the width of the card),
`height` (24 to 400) and `text_padding`, like the panel's `padding` (`{ "right": 16, "left": 4 }`; 0 to 60 points; the
usual is 12 at the sides and 4 above and below one line, 8 for several lines). A box of several lines with a `height`
grows when its text is taller. The designer has them under _Size of the box_.

The label of a slider shares its line with the number, so `align` does not move it. The header has a fixed height: a
bigger title makes room for itself only when there is a subtitle under it. The panel designer has the same controls
(_Position_, _Letters_, _Size_, _Color_) for a selected text, and for the panel itself under _Look_: the rows _Title_,
_Subtitle_ and _Field labels_ each open a page with those controls.

### Button grids, icons and layout

A `buttons` widget is a grid of buttons that can sit anywhere among the other widgets. Pressing one answers like a
button of the bottom row (`out` gets its `id`).

```json
{
    "type": "buttons",
    "columns": 3,
    "label": "Playback",
    "buttons": [
        { "id": "prev", "icon": "skip-back" },
        {
            "id": "play_pause",
            "label": "Play",
            "icon": "play",
            "tone": "primary"
        },
        { "id": "next", "icon": "skip-forward" }
    ]
}
```

| Field                                   | Meaning                                                                                                                                                                                                                                                                           |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `columns`                               | Buttons per row, 1 to 6; `0` or missing: as many as there are, at most 4                                                                                                                                                                                                          |
| `buttons`                               | At most 24 per grid and 48 in all the grids of a panel. Ids are unique over the whole panel                                                                                                                                                                                       |
| button `icon`                           | The name of an icon of the app (`play`, `skip-forward`, `ico:heart`; the designer lists them) or, in a panel of a flow, `file:/path/to/picture.png` (`~/` works; at most 4 MB; pictures are not drawn for the browser extension). A button with an icon and no label is icon only |
| panel `passive`                         | `true`: the panel does not take the keyboard from the app that is in front (for a panel that only shows progress). It also gives the window up to any other panel that is asked for. Where the window system decides for itself (macOS, Linux) it is a request                    |
| button `keep_open`                      | This button does not make the panel go away (see below)                                                                                                                                                                                                                           |
| button `toggle`                         | `true`: a button with two looks. Each press changes it to the other look and the next press changes it back (see _Buttons that change_)                                                                                                                                           |
| button `on_label`, `on_icon`, `on_tone` | The second look of a `toggle` button: its text, its icon (same names as `icon`) and its tone. What is left out stays as in the first look                                                                                                                                         |
| button `bind`                           | Name of a flow variable that keeps the state of a `toggle`: read when the panel is shown, written (`1` or `0`) by every answer. Same rules as a variable name. See _State kept by the flow_ |
| panel `restore`                         | `true`: every field starts with the variable named by its `id` (a variable that is not set leaves the field's own `value`), and every toggle without `bind` uses its `id` as its variable. Only for panels of a flow. See _State kept by the flow_ |
| button `on`                             | `true`: a `toggle` button that starts in its second look                                                                                                                                                                                                                          |
| button `width`                          | Width in points, 24 to 640. 0 or left out: as wide as the text needs in the row at the bottom, the whole cell in a grid                                                                                                                                                           |
| button `height`                         | Height in points, 24 to 72. 0 or left out: 36, like the other controls. A row (of a grid, or the one at the bottom) is as tall as its tallest button                                                                                                                              |
| button `radius`                         | How round the corners are, in points: 0 is square, a big number makes a pill (never more than half the height). Left out: the usual                                                                                                                                               |
| button `side`                           | Where a button of the row at the bottom sits: `start` (left), `center` or `end` (right). Left out: where `buttons_align` puts it                                                                                                                                                  |
| grid `align`                            | Where a button that is narrower than its cell (it says its `width`) sits in the cell: `start`, `center` (the default) or `end`                                                                                                                                                    |

`{ "type": "spacer", "height": 16 }` is empty room (2 to 200 points).

The row at the bottom is placed with `"buttons_align"`: `end` (right, default), `start`, `center` or `stretch` (all the same
width). Its buttons take `icon` too. A panel that has a `buttons` grid and no buttons of its own only gets **Cancel**.

### Staying open (no blink)

By default a panel answers and goes away, and a flow that asks again shows a new window. For a remote control that is a
blink after every press. With `"keep_open": true` on the panel (or on a button) the panel **stays on screen** after the
button: the flow gets the answer, runs its action, and its next _Ask_ with the **same panel `id`** takes the window over
where it is (the person may have moved it). It goes away when the flow ends, when a button that cancels is pressed, or after 6
seconds without a new panel. A button pressed while it waits is **remembered** (up to 8) and answered, in order, as the flow
asks again, so fast clicking is not lost. The new description is drawn when the flow asks again, so a `{{variable}}` in a
widget's `value` (the display of a calculator) shows what the flow just computed. Give the panel an `id`, `"keep_open": true`
and, for a tool that stays for a long time, `"timeout_s": 0`. Put a `Hide notices` block (_Quiet_) after _Start_ so no
notice appears between presses.

### Buttons

`{ "id": "go", "label": "Say it", "tone": "primary", "cancel": false }`. `tone` is `secondary` (default), `primary` or
`danger`. Enter presses the first `primary` button (else the first that does not cancel); Esc presses the first button
with `"cancel": true`. A button that cancels closes the panel without values. Any other button is refused with a note
while a `required` box is empty.

### Size, shape and place of buttons

Every button can say how big and how round it is, and a button of the row at the bottom can say which side it sits on,
so a _Close_ does not have to be next to the answer buttons:

```json
"buttons": [
  { "id": "close", "label": "Close", "cancel": true, "side": "start", "radius": 36 },
  { "id": "go", "label": "Go", "tone": "primary", "width": 160, "height": 44 }
]
```

The buttons on the same side form a group, in the order they are written; the left group sits against the left edge, the
right one against the right edge and the middle one in the middle (it moves aside when it would touch another group).
With `"buttons_align": "stretch"` the buttons share the row, and a button with its own `width` keeps it. When the buttons
are too wide for the row they all shrink alike. The window grows with a taller button.

### Buttons that change

A play button that turns into _Pause_, a mute button that turns red: add `"toggle": true` and the second look.

```json
{
    "id": "play",
    "label": "Play",
    "icon": "play",
    "tone": "primary",
    "toggle": true,
    "on_label": "Pause",
    "on_icon": "pause"
}
```

The press is answered as always (the flow gets `play` as the button). Only what the person sees changes, and it stays so
while the panel stays open (`keep_open`): when the flow asks again with the same panel `id`, a toggle that is still a toggle
keeps the look it had. A button that cancels cannot be a toggle. A panel that goes away after every press starts its
toggles again each time it is shown (`"on": true` makes one start in the second look).

Without more, the look is **not checked against the world**: a play button shows _Pause_ after one press even if the browser
was already playing when the panel opened, and the flow has no way to know which look the button has. To make the flow and
the button agree, give the toggle a `bind` (next section).

### State kept by the flow (`bind` and `restore`)

A toggle button, a switch, a slider or an input has a state, and a flow that shows the panel again has to give it back. The
description of a panel is read before the `{{variables}}` are filled in, and `on` and `value: true` are true/false, not
text, so they cannot say `{{keep_playing}}`. Two settings read the variables of the flow for you, so **one panel
description serves every state** (no copy of the panel for each look):

- **`"bind": "keep_playing"` on a toggle button.** When the panel is shown, the button takes the look from the variable:
  `1`, `true`, `on` and `yes` are the second look, `0`, `false`, `off` and `no` the first, in any case. A variable that is
  not set (or says something else) leaves the button as its own `on` says. Every answer of the panel writes the state back
  to the variable (`1` or `0`), **after** the press, so the variable already has the new look when the flow runs its
  action. The flow must therefore **not** flip the variable itself (a `Logic` _Not_ would undo it).
- **`"restore": true` on the panel.** Every field (`input`, `number`, `toggle`, `slider`, `color`, `choice`, `multi`) that
  finds a variable named by its `id` starts with that value instead of its own `value`. Fields always wrote their value to
  that variable; now they read it back, so a slider that was moved is where it was left. A number is read as a number, a
  `choice` only takes an option the list has, a `multi` reads a JSON list (`["a","b"]`, how a flow writes a list) and what
  cannot be read leaves the field's own value. A toggle button without `bind` uses its own `id` as the variable.

```json
{
    "id": "remote",
    "keep_open": true,
    "widgets": [
        { "type": "buttons", "buttons": [
            { "id": "toggle", "icon": "play", "on_icon": "pause", "on_tone": "danger", "toggle": true, "bind": "keep_playing" },
            { "id": "mute", "icon": "speaker-high", "on_icon": "speaker-slash", "toggle": true, "bind": "keep_muted" }
        ] }
    ]
}
```

Name the variables `keep_...` and they are also **saved between runs** (see _Kept variables_ in `FLOWS_FOR_AI.md`), so
the buttons come back as the person left them even after CursorPie was closed. Set a default first
(`keep_muted` = `{{keep_muted|default:0}}`) so the first run has a value. A bound toggle that the flow asks for again with
the same panel `id` (`keep_open`) takes the look of the variable, not the one the old panel remembered: the variable is the
truth. In the panel designer, `restore` is the switch *Remember values* under *Behavior*, and `bind` is the field *Remember in variable* of a button that _Changes when pressed_. The example `browser-remote-panel-state` shows the whole flow. The browser extension has no flow variables, so
`bind` and `restore` do nothing there.

### The answer

```json
{
    "panel": "greeting",
    "outcome": "submitted",
    "button": "go",
    "values": { "name": "Ann", "size": "medium", "loud": true }
}
```

`outcome` is `submitted`, `cancelled` (a cancel button, Esc, closed, replaced), `timeout`, or `unavailable` (panels are
switched off, Settings is open, too many panels wait; `reason` says which). `values` is only filled when submitted.

### Limits

The description is checked before anything is drawn (`PanelSpec::checked`): at most 64 KiB, every text is cut to its
limit, control characters are removed, ids are checked, an option that is empty or repeated is dropped, a slider needs
`min < max`. What cannot be used is an **error that says why**, not a silent change. A panel taller than the display
scrolls between its header and its buttons.

## In a flow

Add an **Ask** block and set its mode to **Panel** (in the list editor: _Ask and run_, _Show a panel_). The block's text
is the JSON above.

- Every field sets the variable named by its `id`: `{{name}}`, `{{size}}`. A toggle is `1` or `0`, a number has no
  useless `.0`, a `multi` is a JSON list (use it with _For each_).
- `out` gets the id of the button that was pressed (`go`, or `pick` for a menu).
- `ok` is `1` when a button that confirms was pressed, `0` when it was cancelled or timed out. That is an answer, not a
  failure.
- `{{variables}}` may be used **inside the texts** of the panel (title, labels, options, values). They are filled in
  _after_ the JSON is read, one text at a time, so a value with a quote in it can never add a widget or a button.
- A description that is not valid, panels switched off, or the window being busy are **failures** with the reason, so the
  run report says so; the flow goes on with `ok` at `0`.
- The editor's checks know the variables the fields set, so `{{name}}` is not reported as unset.

`docs/examples/flows/panel-form.cursorpie-flow.json` is a complete example (a test loads it).

### The panel designer

Nobody has to write the JSON by hand. In the Flow tab, an _Ask_ block in Panel mode has a **Design...** button (_Design
panel..._ in the canvas) that opens a sheet with three columns:

- **left**: _Add_ (one click adds a widget after the selected one), _Start from_ (ready-made panels: a question, a
  confirmation, a menu, a settings form, a progress bar) and _Structure_ (the panel, its widgets and its buttons; click to
  select);
- **middle**: the panel itself, drawn by the same code a flow uses, so it is what will be shown. Its widgets work, and a
  click on one selects it. Widgets a `show_if` would hide are drawn anyway, so they can be selected;
- **right**: what is selected: texts, ranges, options (one per line: `value | label | hint`), the answer name, **Show only
  when** (which field, _has a value_ or _equals_), move, duplicate and remove.

Every change is written back to the block at once (undo and redo work), and a panel that is not valid yet (two boxes with
the same name) says why under the preview and is still kept. The JSON box stays for people who prefer it. The rules are
in `src/panel/design.rs` (tested); the drawing is in `src/ui/settings/panel_designer.rs`.

## From the browser extension

The extension talks to the app over the bridge (`docs/developer/BROWSER_BRIDGE.md`). Three messages:

| Direction       | Message                                                                                                                                |
| --------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| extension → app | `{ "type": "panel", "req": 7, "spec": { ...a panel... } }`                                                                             |
| extension → app | `{ "type": "panel_update", "panel": "id", "patch": { "widget": "bar", "value": 0.5 } }` and `{ "type": "panel_close", "panel": "id" }` |
| app → extension | `{ "type": "panel_result", "req": 7, "result": { ...the answer... } }`                                                                 |

`ui.js` wraps them in `CP.ui`, which any code of the extension can use:

```js
const r = await CP.ui.show({
    title: "Rename",
    widgets: [{ type: "input", id: "name", required: true }],
});
if (r.outcome === "submitted") rename(r.values.name);

await CP.ui.confirm("Close 12 tabs?", { danger: true }); // true or false
await CP.ui.prompt("Name", { hint: "..." }); // the text, or null
await CP.ui.pick(["a", "b", { value: "c", label: "C" }]); // the value, or null
CP.ui.update("job", { widget: "bar", value: 0.4 }); // live progress
CP.ui.close("job");
```

`show` never rejects: without a connection, with panels switched off, or with an app that is too old it resolves
`unavailable` with the reason. A panel update or close only reaches a panel **that the same extension started**.

Commands built on it (the **Panels** group of the _Browser_ action, `docs/BROWSER_EXTENSION.md`): `ui.tabs` (a searchable
list of open tabs), `ui.find_tab`, `ui.confirm_close_others`, `ui.media` (a media menu). They are small: each one asks,
then runs commands that already exist.

## Adding another caller

1. Build a `PanelSpec` (`PanelSpec::parse(json)` or `from_value`; both check it).
2. Call `panel::ask(spec, Source::...)` from a thread that may wait, or `ask_in_background(spec, source, done)` from one
   that must keep reading. Never from the UI thread. Add a `Source` variant and its rule in `broker::allowed`.
3. `panel::patch` and `panel::close` change or close it.

To change how panels look, replace `ui::settings::panel_form::draw`; to add a widget, add a variant to `Widget`
(`spec.rs`: its limits and cleaning), its field to `Form` (`form.rs`), its height to `layout.rs` and its drawing.

## Known limits

- Written without a compiler at hand, like the rest of this branch: `cargo test` has not been run on it. The JavaScript
  side is tested (`tests/browser-extension/ui.test.js`, `node --test`). (The additions of the designer, `number`, `color`,
  `dropdown` and `show_if`, were compiled and tested with `cargo test`; the whole window was not looked at on a screen.)
- Not run on all systems. The first frames ask the window for the keyboard several times because a window that is
  still being mapped may ignore the first request; on Windows it relies on `raise_own_window`. Check that typing works on
  each system (the checklist is below).
- There is no keyboard navigation inside a list (arrow keys); the mouse and the search box work, and Enter picks the first
  match of a menu.
- A panel is not resized when a patch adds options; it keeps its size and scrolls.
- The designer moves widgets and buttons by dragging (a row of the list, or the grip beside the selected widget in the
  preview) and with the arrows. There are no side-by-side widgets (use a button grid for rows of buttons) and no file or
  folder chooser widget yet.
- Pictures only on buttons (`file:` icons); no other images and no custom drawing inside a panel yet.
- Written without a compiler at hand: the additions of button grids, icons, `keep_open` and the drag and drop were not
  built or run. Run `cargo test` and look at the panel on a screen.

### Manual checklist

1. Run the example flow: the panel appears next to the pointer or centered, the first box has the keyboard, Enter presses
   the main button, Esc cancels, the notification shows the values.
2. Leave **Name** empty and press the main button: a note says what to fill in, the panel stays.
3. Hold the pie trigger while a panel is open: the panel goes away (cancelled) and the pie opens.
4. Open Settings while a panel waits in the queue: it is answered as cancelled.
5. Extension (switched on in Settings): run `ui.tabs`, pick a tab; type in the search box, press Enter.
6. Extension, switched off in Settings: `ui.tabs` shows an error saying panels are switched off.
7. A display smaller than the panel: the widgets scroll, the buttons stay.

## Progress panels

A run that takes a while can be followed in a small panel with a bar, the step that is running and a **Stop** button.

- **By itself.** A flow, a pause or a sequence that lasts more than about two seconds opens one (_Behavior > Show a
  progress panel for long runs_, `progress_panel` in the config). It says `Waiting` with a countdown, `Running
command` with the seconds it has taken (the command itself is never shown), the block of a flow (`Step 3/8`) or the
  round of a loop (`Round 2/10`). When the run ends the bar fills and the panel goes away. **Stop** ends the run at
  the next block and cuts a _Wait_ or a _Run and wait_ short. If another panel or the pie needs the window, the
  progress panel gives way and the small notice carries on. Off Windows, a run that presses keys keeps the small notice
  instead, because showing a window may take the keyboard from the app those keys are for.
- **By the flow.** The _Update panel_ block (`PanelUpdate`, see `docs/FLOWS_FOR_AI.md`) opens a progress panel with a
  title, a label and a bar, changes them as the flow works, and closes it. `docs/examples/flows/progress-panel.cursorpie-flow.json`
  is an example (a test loads it). The panel is an ordinary panel with `"passive": true`, a `progress` widget with the id
  `bar` and, when asked for, a button with the id `stop`.
