# Configuration

CursorPie keeps all settings in one JSON file. You can edit them in Settings, or by hand.

## Where the file lives

| System | `config.json` |
|---|---|
| macOS | `~/Library/Application Support/CursorPie/config.json` |
| Windows | `%APPDATA%\CursorPie\config.json` |
| Linux | `~/.config/cursorpie/config.json` (or `$XDG_CONFIG_HOME/cursorpie/`) |

**Settings > General** shows the exact path and has a button that reveals it. A change made by hand is picked
up the next time CursorPie starts. Next to the file you may find:

```text
config.json            the live config
config.backup.1.json   the version before your last Save (up to config.backup.5.json)
config.broken.json     a config that could not be read (see below)
mice.json              the mice CursorPie has met and their extra buttons (not part of the config, see below)
icons/                 images copied in by a profile import, and the smaller copies made when you choose a big image
welcome_shown          empty file: the welcome window was already shown
logs/                  diagnostic logs, only when logging is on (see DEBUGGING.md)
debug                  optional empty file that turns the diagnostic log on (see DEBUGGING.md)
```

Export writes `cursorpie-config.json` to your Downloads folder.

## Extra mouse buttons

A `Mode` has `back` and `forward`, and an optional `extra`: an object from the number of an extra mouse button
(`"5"` to `"15"`) to an `Action`. Button numbers are the macOS ones: 3 back, 4 forward, 5 and up extra; on Linux
button 5 is X button 10. `extra` is left out of the file when empty, a file without it loads as before, and
numbers outside 5 to 15 are removed when the config is loaded. A button without an entry (or with `Nothing`)
passes through to the app.

## mice.json

`mice.json` holds device state, not settings: it is written by CursorPie as soon as a mouse is met or scanned,
and Save, Undo and the backups do not touch it. Delete it to make CursorPie forget every mouse. Per mouse:

| Field | Meaning |
|---|---|
| `id` | `hw:<vendor>:<product>` or, when the system gives no id, `name:<lower-case name>` |
| `name` | The name the system gave |
| `reported` | Extra buttons the system reported when the mouse was last seen |
| `scanned` | Extra buttons pressed in a scan; when not empty, these are the mouse's buttons |
| `first_seen`, `last_seen`, `times` | Unix seconds, and how often it was connected |

At most 24 mice are kept (the one seen longest ago goes first). `active` names the mouse in use.

## Image icons

An `icon` that ends in `.png`, `.jpg`, `.jpeg` or `.webp` is an image file (a path may start with `~/`). CursorPie reads its size from the file header first and does not decode an image over 4096 px on a side: the slice shows `?`. A loaded image is scaled down to at most 128 px. Choose images in Settings with **Image...** and big ones are shrunk for you; if you write a path by hand, use a file that is already small (at most 512 px).

## Structure
`profiles[] > items[] > kind`, where `kind` is `Mode`, `Run` or `Submenu`. Actions are `Nothing`, `Key`,
`OpenApp`, `OpenUrl`, `Command`, `Sequence`, `Wait` (milliseconds) or `Flow` (the `id` of a flow). Enums use serde's
default external tagging, for example `{"Run": {"OpenApp": "Notes"}}`.

### Flows

`flows[]` is the list the **Flow** tab edits. Each flow is `{ "id", "name", "nodes": [...] }`; a node is
`{ "id", "x", "y", "kind", "next" }` where `kind` is `Start`, `End`, `{"Key": ..}`, `{"OpenApp": ".."}`,
`{"OpenUrl": ".."}`, `{"Command": ".."}` or `{"Wait": 500}` and `next` is the `id` of the node its wire leads to (or
`null`). The flow runs from the one `Start` node along the `next` wires to the one `End` node (which has no `next`; a flow
without one gets it when the file is loaded). At most 32 flows of 64 nodes; a wait is at
most 10000 ms. Anything outside the limits is repaired when the file is loaded. The full reference for writing a flow by
hand or with an AI agent (key codes, rules, recipe, examples, a JSON Schema) is [FLOWS_FOR_AI.md](FLOWS_FOR_AI.md).

```json
{
  "version": 2,
  "pie_hold_ms": 150,
  "settings_hold_ms": 1200,
  "pie_size": 200.0,
  "accent": [90, 120, 94],
  "trigger_button": 2,
  "flick": false,
  "sound": false,
  "launch_at_login": false,
  "app_gesture": "DoubleClick",
  "double_click_ms": 260,
  "target_app": "UnderCursor",
  "focus_target": true,
  "desktop_mode": "SwitchToApp",
  "open_app_choice": true,
  "side_default": "App",
  "side_adopt": true,
  "side_forget": false,
  "right_click_switch": true,
  "side_indicator": true,
  "inactive_color": [232, 86, 86],
  "preview_ms": 900,
  "center_app_icon": true,
  "app_icon_style": "Glyph",
  "app_icon_shadow": "Auto",
  "show_labels": false,
  "sleep_after_s": 60,
  "appearance": "System",
  "pie_style": "Solid",
  "glass_clear": 60,
  "glass_frost": 45,
  "glass_shine": 60,
  "touch_trigger": false,
  "touch_still_mm": 2.0,
  "touch_click_action": "SwitchPie",
  "touch_typing_guard_ms": 400,
  "haptics": true,
  "haptic_hover": true,
  "profiles": [
    {
      "name": "Default",
      "apps": [],
      "active": [0],
      "items": [
        {
          "name": "History",
          "icon": "<>",
          "kind": {
            "Mode": {
              "back": { "Key": { "key": 6, "mods": 8 } },
              "forward": { "Key": { "key": 6, "mods": 9 } },
              "extra": { "5": { "OpenUrl": "https://example.com" } }
            }
          }
        },
        {
          "name": "Notes",
          "icon": "~/Pictures/notes.png",
          "kind": { "Run": { "OpenApp": "Notes" } }
        },
        {
          "name": "Tools",
          "icon": "Go",
          "kind": {
            "Submenu": [
              { "name": "Docs", "icon": "URL", "kind": { "Run": { "OpenUrl": "https://example.com" } } },
              {
                "name": "Both",
                "icon": "2x",
                "kind": { "Run": { "Sequence": [ { "OpenApp": "Safari" }, { "OpenApp": "Notes" } ] } }
              }
            ]
          }
        }
      ]
    }
  ]
}
```

## Field reference

| Field | Meaning |
|---|---|
| `trigger_button` | `2` = middle, `5` / `6` / `7` = extra buttons (shown as "Extra button 6/7/8"). Only the middle button works on Windows and Linux |
| `accent` | Accent color as `[r, g, b]`. The default is `[90, 120, 94]`; the old default blue `[64, 156, 255]` is replaced by the new default when a file is loaded |
| `pie_hold_ms` | Hold time before the pie opens (50 to 2000 in the file; the Settings slider goes from 50 to 1500; default 150) |
| `settings_hold_ms` | Hold time before Settings opens (always at least `pie_hold_ms` + 300; the Settings slider goes from 500 to 4000) |
| `pie_size` | Pie window size in px (140 to 360) |
| `app_gesture` | `"DoubleClick"` (hold = general pie, double-click = app pie) or `"Merged"` (classic) |
| `double_click_ms` | Window for the second click (150 to 500) |
| `target_app` | `"UnderCursor"` (app whose window is under the pointer) or `"Focused"` (app in front) |
| `focus_target` | With `UnderCursor`: bring that app to the front before sending it a shortcut |
| `desktop_mode` | `"SwitchToApp"` (default), `"BringToCurrent"` or `"DoNothing"`: what an Open app slice does when the app's window is on another desktop or Space. `BringToCurrent` is macOS only (experimental) and acts as `SwitchToApp` elsewhere. See [OTHER_DESKTOPS.md](OTHER_DESKTOPS.md) |
| `open_app_choice` | `true` (default) or `false`. When an Open app slice's app is already running, hovering the slice shows two round badges side by side just outside the ring, in front of the slice: a green one with a + (a new window, on the side where the slice starts) and a yellow one with a small square (the app that is open, on the side where it ends). Releasing on the half of the slice nearer a badge picks it. `false`: the slice always does what it did before. The running apps are looked up once per press, off the input thread, and only when a pie has an Open app slice. A new window is `open -a` then Cmd+N on macOS, and starting the app again on Windows and Linux |
| `side_default` | `"App"` or `"General"`: the pie the side buttons follow in an app that has its own pie, until you choose |
| `side_adopt` | Picking a side-button mode in a pie makes the side buttons follow that pie |
| `side_forget` | Forget that choice when another app becomes the target |
| `right_click_switch` | Right-click while the pie is open to switch the side-button pie |
| `side_indicator` | Color the pie ring by who owns the side buttons; `inactive_color` is the "other pie" color |
| `preview_ms` | How long the pie stays on screen after switching from the menu bar (0 to 3000, 0 = off) |
| `center_app_icon` | Show the app's icon in the center of an app's own pie instead of the active mode's icon |
| `app_icon_style` | `"Glyph"` or `"Color"`: that icon as a one-color silhouette in the pie's ink (default), or in the app's own colors |
| `app_icon_shadow` | `"Auto"` (default), `"Soft"`, `"Strong"` or `"Off"`: the shadow under the app's icon in the center of the pie. `Auto` adds it only on a plain light center |
| `show_labels` | Show each slice's name under its icon (default `false` = icons only; a file that lacks the field keeps the names) |
| `appearance` | `"System"` (default), `"Light"` or `"Dark"`: the look of Settings and the pie |
| `pie_style` | `"Solid"` (default) or `"Glass"`: opaque slices, or see-through frosted ones (always smoked dark) |
| `glass_clear`, `glass_frost`, `glass_shine` | Glass style only, in percent (0 to 100, larger values are clamped): how see-through the slices are (default 60), how frosted (default 45) and how bright the rim and sheen are (default 60) |
| `touch_trigger` | macOS trackpad, Windows precision touchpad and Linux (X11) touchpad: a hold opens the pie (default `false`; while it is off the system's touch library is never loaded on macOS, nothing is registered with Windows and `/dev/input` is never opened on Linux). See [the user guide](USER_GUIDE.md#trackpad-macos-windows-and-linux-x11) |
| `touch_still_mm` | 0.5 to 6.0 (default `2.0`): how far, in millimeters on the trackpad, the holding finger may move before the hold counts |
| `touch_click_action` | `"SwitchPie"` (default, like the right-click), `"PickHovered"` (the hovered Mode slice becomes the active mode) or `"Nothing"`: what the extra click does |
| `touch_typing_guard_ms` | 0 to 2000 (default `400`, `0` = off): no hold is armed this soon after a key press |
| `haptics`, `haptic_hover` | macOS only (ignored on Windows and Linux): trackpad ticks when the pie opens and the mode switches (default on), and when the hovered slice changes (default on, needs `haptics`) |
| `sleep_after_s` | Seconds without a pie or Settings before cached icons (except those of the general pie) and the Settings copy are freed (10 to 3600, `0` = never) |
| `apps` | App ids: bundle identifiers on macOS, executable names on Windows, desktop-entry ids on Linux. An empty list marks the general profile; only the first such profile is kept, all others need at least one app |
| `active` | Path of the active side-button mode, as indices (`[3, 1]` = slice 1 inside slice 3) |
| `Key` | `key` is a macOS virtual key code (a physical key; Windows and Linux map it to the key in the same position). `mods` is a bitmask: Shift = 1, Ctrl = 2, Opt/Alt = 4, Cmd/Win/Super = 8 |

## What happens to bad files

- Out-of-range values are clamped when the file is loaded. Rings are trimmed to 8 slices and menus to 5 levels.
- A file without a `"version"` field (the old v1 format) or with invalid JSON is rejected: CursorPie copies it
  to `config.broken.json` next to the original and starts with the defaults.
- The next save (pressing Save in Settings, or picking a side-button mode in the pie) overwrites `config.json`,
  so fix or move the broken file first if you want to keep your edits.
- A config from another system still loads. App ids of the other system are kept but do not match anything.

## Security

`Command` actions run shell commands with your user's permissions. **Only import configs and profiles you
trust.** CursorPie warns you after importing a file that contains any.

## For developers

How the format is versioned and how to add a setting is in the Rust docs (`guides::config_format` and
`guides::extending`; run `cargo docs`).
