# Trackpad and haptics: design and plan (macOS, with Windows and Linux X11 sections)

**Status: implemented in code (phases T1 to T5 and T7), and for Linux X11 (see [Linux (X11)](#linux-x11)); the native layers are not yet verified on a Mac or on a Linux touchpad.** The
portable core (recognizer, bridge, extra click, haptic limiter, config, Settings panel) is unit tested on every OS.
The code that talks to `MultitouchSupport.framework`, the added `LeftMouse*` events of the tap and the haptic call
compile only on macOS and were written without a Mac: spikes S1 to S10 and the manual checklist below are still to
be run, and not implemented: the live finger demo in Settings, the *tap, then hold* trigger style, `touch_zone`,
actuator waveforms (all in "later"). The rest of this document is the design it follows. It says how to
let a Mac trackpad open and drive the pie, how to give haptic feedback on it, and how to keep the mouse and the
trackpad one seamless experience. Every statement about a macOS API is a **design assumption** until the
[spikes](#phase-0-spikes) answer it on a real Mac; the CI runners cannot touch a trackpad.

It follows the structure of [LINUX_MIGRATION.md](LINUX_MIGRATION.md) and reuses its rules: a portable core that
is tested everywhere, a thin native layer per OS, and no change of behavior for anyone who does not turn the
feature on.

## Contents

- [Summary](#summary)
- [Linux (X11)](#linux-x11)
- [Goals and non-goals](#goals-and-non-goals)
- [What the user will experience](#what-the-user-will-experience)
- [How the code works today](#how-the-code-works-today)
- [Design](#design)
- [Haptic feedback](#haptic-feedback)
- [Mouse and trackpad together](#mouse-and-trackpad-together)
- [Config and Settings](#config-and-settings)
- [Obstacles and solutions](#obstacles-and-solutions)
- [Alternatives considered](#alternatives-considered)
- [Phases](#phases)
- [Phase 0 spikes](#phase-0-spikes)
- [Test strategy](#test-strategy)
- [Manual verification checklist](#manual-verification-checklist)
- [Security and privacy](#security-and-privacy)
- [Documentation changes](#documentation-changes)
- [Windows Precision Touchpad](#windows-precision-touchpad)
- [Open decisions for the maintainer](#open-decisions-for-the-maintainer)
- [References](#references)

## Summary

1. **Touch input becomes a second source of the same gesture.** The state machine in `src/tap/trigger.rs` already
   turns "button down, held, moved, released" into "pie opens, slice is hovered, slice runs". A trackpad hold
   is the same story with different words, so a new *touch source* is translated into the events the state
   machine already understands. The pie, the profiles, the app detection and the actions do not change.
2. **Raw touches need Apple's private `MultitouchSupport.framework`.** No public API gives a background app the
   number and position of the fingers on a trackpad. The private framework is the one that the existing
   open-source tools use, so it is the recommended source, loaded at run time and failing soft (see
   [Capturing the touches](#capturing-the-touches)).
3. **The extra finger is classified by finger count, not by what macOS decides the click is.** One finger holding
   plus one finger clicking is two fingers at the moment of the click, which macOS may treat as a secondary click.
   CursorPie counts the fingers itself, swallows the click and runs one action, whatever the system mapping is.
   (A hold with two fingers was dropped: the system scrolls with two fingers, so it was never reliable.)
4. **Haptics use the public `NSHapticFeedbackManager`.** It needs no private call, is ignored by the system when
   no finger touches the trackpad, and so is harmless with a mouse. A private actuator API exists for richer
   waveforms; it is a later, optional step.
5. **Everything is off by default.** macOS first; Windows (precision touchpad) and Linux X11 were added afterwards, so all three systems are covered.

## Goals and non-goals

**Goals**

- Touch and hold on a trackpad opens the pie under the pointer; moving the finger hovers a slice; lifting the
  finger runs it (the same as holding and releasing the middle button).
- While the pie is open, one more finger that clicks switches the mode, like the right-click does today
  (see [What the extra click does](#what-the-extra-click-does)).
- The number of fingers is a setting: hold with **1** finger and click with a second, or hold with **2** fingers and
  click with a third.
- Haptic ticks when the pie opens, when the hovered slice changes and when the mode switches.
- A mouse, a built-in trackpad and an external Magic Trackpad all work at once, in any order, with the same
  pie, the same timings and the same profiles, and without a mode to switch.

**Non-goals**

- No code is written by this document. It is the research and the plan.
- No support for the Magic Mouse's touch surface in the first release (see [Open decisions](#open-decisions-for-the-maintainer)).
- No Wayland touchpad work. Linux X11 has a design, in [Linux (X11)](#linux-x11); the Windows Precision Touchpad has one, in [Windows Precision Touchpad](#windows-precision-touchpad).
- No custom gestures (swipes, pinches, rotations) and no replacement of the system's own trackpad gestures.
- No Mac App Store build. CursorPie is distributed as a DMG, and a private framework is not allowed in the store.

## What the user will experience

| You do this | With a mouse (today) | With a trackpad (planned) |
|---|---|---|
| Open the general pie | Hold the middle button for the hold time | Touch and hold with the chosen number of fingers, without moving, for the hold time |
| Choose a slice | Move toward it | Move the finger toward it |
| Run it | Release the button | Lift the finger |
| Cancel | Release in the center | Lift the finger in the center |
| Open Settings | Keep holding in the center | Keep holding in the center, same time |
| Switch the mode (the pie the side buttons follow) | Right-click | Click with one more finger |
| Open the app's own pie | Double-click the trigger (default mode) | Tap, then touch and hold again within the double-click window (to be confirmed in S5) |

The hold time, the dead zone of the center, the flick distance, the pie size, the colors and the sounds are the
same settings for both. The only trackpad settings are the ones that have no mouse counterpart (how
still the finger must stay, haptics).

Fingers and what they do:

| Hold | Extra click | Fingers at the click | Conflict to expect |
|---|---|---|---|
| one finger resting | a second finger clicks | 2 | macOS maps a two-finger click to a secondary click by default |

There is no two-finger hold: the system turns the movement of two fingers into scrolling, so the pointer would not
move and the pie would have to be aimed by the fingers themselves. That mode was removed from every OS.

## How the code works today

These are the seams the plan uses. Names are the real ones in `src/`.

- `tap/mod.rs` turns every native event into an `Ev` (`Key` or `Mouse { kind, button, pos }`) and calls
  `dispatch`, which answers `Keep` or `Drop`. Button numbers are the macOS ones on every OS.
- `tap/trigger.rs` is the state machine (`Gesture`: `Idle`, `Pending`, `WaitSecond`, `ActivePie`, `InSettings`,
  `Passthrough`). The hold timer thread (`spawn_hold_timer`) opens the pie after `pie_hold_ms` and Settings after
  `settings_hold_ms`. A move beyond `DRAG_TOLERANCE` (6 px) before the pie opens hands the press back to the app.
- `ActivePie` follows `Kind::Drag` events (`on_drag_in_pie`) to compute the hovered slice with `resolve::eval`,
  and the release (`on_release_in_pie`) runs the slice.
- `tap/right_click.rs` switches the pie the side buttons follow while a pie is open (`side::toggle`), and
  `tap/side_buttons.rs` runs the active mode's actions for the back, forward and extra buttons.
- `tap/macos.rs` is a `CGEventTap` that listens for `OtherMouse*`, `RightMouse*` and `Key*` events only. It does
  not see `MouseMoved` or `LeftMouse*`, and it has no notion of fingers.
- The tap callback rules (`guides::architecture`) apply to everything below: never block, never panic out of it,
  pass events through when unsure.
- `mouse.rs` keeps a book of mice (`mice.json`) and a scan. Trackpads are not in it, and should not be (see
  [Mouse and trackpad together](#mouse-and-trackpad-together)).

Two facts shape the design. First, a finger resting on a trackpad is **not** a pressed button, so the pointer
moves with `MouseMoved` events, not `Drag` events. Second, the hover computation only needs a pointer position,
so any source that can say "the pointer is here" can drive it.

## Design

### Capturing the touches

| Option | What it gives | Verdict |
|---|---|---|
| **A. `MultitouchSupport.framework` (private)**, loaded with `dlopen` | Per-frame list of fingers: id, normalized position, size, pressure, state, timestamp; for every trackpad, built in or Magic Trackpad | **Recommended.** The only source with finger positions and counts for a background app |
| B. Gesture events (`CGEventType` 29) in the existing `CGEventTap` | A notice that a gesture started or changed | Not enough: it carries no finger positions and, as reported by developers, the touch set read from it can be empty. May still help to cancel a system gesture (S3) |
| C. `NSEvent` global monitor | A read-only copy of events | Cannot intercept, and touch details are tied to a view in the app's own window. Not usable |
| D. Public `NSTouch` events in CursorPie's own window | Real touches | Only while the pointer is over that window and the app is focused. Not usable for a background menu |

Option A is what the community tools use to read raw contact frames: enumerate the devices
(`MTDeviceCreateList`), register a contact-frame callback on each (`MTRegisterContactFrameCallback`) and start
each device (`MTDeviceStart`). Notes for the implementation:

- **Load it at run time** (`dlopen` on the framework path, `dlsym` for the four or five symbols) instead of
  linking it. If the file or a symbol is missing, or the self-check below fails, CursorPie logs it, shows
  "Trackpad support is not available on this macOS" in Settings and carries on with the mouse. A future macOS
  must never stop the app from starting.
- **The touch record layout is not public and has changed between macOS releases** (one open-source tool states
  that it handles a changed layout on the newest macOS). Read only the few fields needed (state, id, normalized
  x and y, size) through one small `repr(C)` view, and add a **self-check at start**: values must be in range
  (positions in 0 to 1, finger ids small, timestamps increasing) for the first frames, otherwise the source
  turns itself off for the session. Keep the layout in one file with a comment saying which macOS it was
  verified on.
- **Pick the right devices.** The list also holds other surfaces (the Magic Mouse, the Touch Bar on older
  Macs). Accept only trackpad-sized devices; tools do it by device family or by the surface size. Rescan when a
  trackpad is connected or disconnected (a Bluetooth Magic Trackpad appears after start), at the same 5 s
  rhythm as `mouse::WATCH_EVERY`.
- **Threading.** The contact-frame callback runs on a thread of the framework. It must follow the same rules as
  the event tap callback: copy the frame into a small struct, feed the recognizer, never block, never panic out
  (wrap in `catch_unwind`).

### The recognizer: a portable core

All decisions live in one pure module, `src/tap/touch/recognizer.rs`, with no macOS types, so it is unit tested
on every OS with recorded frames. It is a state machine over frames:

```text
Frame { time, fingers: [Finger { id, x, y, size, state }] }   x, y in 0..1 of the device surface
        |
        v
Recognizer ---- TouchEvent::HoldArmed      (N fingers down and still: start counting)
            |-- TouchEvent::HoldOpen       (still for the hold time: open the pie)
            |-- TouchEvent::Move           (the holding finger moved)
            |-- TouchEvent::ExtraClick     (one more finger clicked while the pie is open)
            |-- TouchEvent::Release        (the last holding finger lifted)
            `-- TouchEvent::Cancel         (too many fingers, a palm, a scroll started, the hold was broken)
```

Rules the recognizer applies (all numbers are settings or named constants, checked in S-spikes):

- **Still means still on the surface, in millimeters**, not in screen pixels. Pointer acceleration makes a
  few pixels a poor measure; the touch coordinates and the size of the device surface give a real distance (S1 finds where
  the size is read, or a table by device family is used). Default tolerance: about 2 mm.
- **The count must be exact.** Exactly *N* fingers when the hold starts. A 3rd finger during a 1-finger hold
  before the pie opens is not an extra click; it cancels the hold (the user is doing something else).
- **Palm and edge rejection.** Ignore a contact whose size is far above a fingertip, and contacts in a thin
  border of the surface, unless the user chose an edge zone (see [Obstacles](#obstacles-and-solutions)).
- **Typing guard.** Do not arm while a key was pressed within the last 400 ms (the tap already sees key events),
  so resting a hand while typing does not open the pie.
- **Scroll guard.** If the pointer moved (the finger slid) before the hold time ran out, the hold is cancelled
  and macOS keeps the movement: the finger was moving the cursor, not holding.
- **No timers inside.** The recognizer is fed frames and a clock value, so it is deterministic; the native layer
  owns the timer thread, like `spawn_hold_timer` does today.

### Feeding the existing state machine

The touch source does not get a second state machine. It translates:

| `TouchEvent` | Becomes | Result |
|---|---|---|
| `HoldArmed` | `Ev::Mouse { Down, button: trigger, pos: cursor }` | `on_press`: resolves the profiles, warms the icons, starts the hold timer |
| `HoldOpen` | nothing (the existing timer already opens the pie) or the pie opens directly | see the note below |
| `Move` | `Ev::Mouse { Drag, button: trigger, pos: cursor }` | `on_drag_in_pie`: hover and submenus |
| `Release` | `Ev::Mouse { Up, button: trigger, pos: cursor }` | `on_release_in_pie`: run or cancel |
| `ExtraClick` | the same call the right-click makes | switch the mode |
| `Cancel` | `reset` | back to `Idle`, pie closed |

Two details need care:

1. **Who owns the timer.** To keep one definition of "hold", the recognizer reports `HoldArmed` at once and lets
   the existing timer (`pie_hold_ms`) open the pie; a `Cancel` before then makes `on_release_pending` run with no
   click replay (a trackpad hold has no click to hand back). A `Gesture::Pending` that came from touch must not
   post a synthetic middle click on early release, because nothing was pressed. Add a `source` field to
   `Gesture::Pending` and `Gesture::ActivePie` (`Source::Mouse` or `Source::Touch`) so the early-release path
   can tell the two apart.
2. **Where the pointer is.** In a touch hold the pointer is moved by the system, so `Kind::Drag` never arrives from
   the tap. The contact-frame callback reads the cursor position (`platform::cursor_pos()`) on every frame while
   a touch pie is active and sends it as the `Drag`. Reading it in the touch callback avoids adding `MouseMoved`
   to the event tap, which would call the tap for every pointer movement on the machine; S8 measures the cost
   of both.

Because the pie, `side`, `resolve` and the actions only ever see `Ev`, the mouse path is byte-for-byte unchanged
when the touch source is off.

### What the extra click does

The existing UI calls "mode" the pie that the side buttons follow (`side::toggle`, "Switch side-button pie"), and a
*Mode* slice is what picks the actions of the back and forward buttons. The request says that a second finger's
click "changes the mode". Two readings are possible; this plan proposes the first as the default and keeps the
second as a setting, and asks the maintainer to choose (see [Open decisions](#open-decisions-for-the-maintainer)).

| Setting `touch_click_action` | Behavior | Reuses |
|---|---|---|
| `SwitchPie` (default) | Same as the right-click while a pie is open: the side buttons now follow the other pie, the ring changes color, the pie stays open | `right_click::handle_right`, `side::toggle` |
| `PickHovered` | The hovered *Mode* slice becomes the active mode of its profile and the side buttons follow that pie, without closing the pie; over any other slice the click does nothing | the rule that picking a Mode slice calls `side::adopt` |

The click itself must be recognized whatever macOS thinks it is:

- The event tap must also see `LeftMouseDown` and `LeftMouseUp` (it already sees the right ones), but only act on
  them while a **touch pie is active**; in every other state the callback returns `Keep` after one atomic read,
  so ordinary clicking costs nothing.
- While a touch pie is active, a click event of any kind that arrives while two fingers are on the surface is
  the extra click. It is swallowed (press and release), like `RIGHT_SWALLOWED` does for the right button.
- A **tap** (touching without pressing, with *tap to click* on) produces the click when the finger lifts; the
  recognizer sees the second finger come and go quickly with no travel and reports `ExtraClick` itself, and the
  system click that follows is swallowed by the same rule. A **press** (a physical click) is recognized from the
  click event plus the finger count.
- The system may turn the second finger into a secondary click. The Settings panel says so (see
  [Obstacles](#obstacles-and-solutions)).

### Hand-over and arbitration between touch and mouse

The state machine has one owner at a time; see [Mouse and trackpad together](#mouse-and-trackpad-together).

## Haptic feedback

**API.** `NSHapticFeedbackManager.defaultPerformer` returns a performer; `perform(_:performanceTime:)` plays one of
three patterns: *generic*, *alignment* and *level change*, at the *default*, *now* or *drawing finished* time.
There is no intensity or waveform parameter. Apple documents that the system may override the call, for example a
Force Touch trackpad gives no feedback when no finger is on it, and that haptics should answer a user action and
not be used for events the user did not cause. The same patterns are used by other apps for slider detents (a
soft *alignment* tick for fine steps, a firmer *level change* for deliberate ones). The Rust crate
`objc2-app-kit`, which the project already uses on macOS, has bindings for the manager (`NSHapticFeedback`),
so no new dependency is expected; only a feature flag in `Cargo.toml` (S4 confirms the exact name).

**Which events tick**

| Moment | Pattern | Why |
|---|---|---|
| The pie opens after the hold | `LevelChange` | a firm "it is open" confirmation, replaces looking |
| The hovered slice changes | `Alignment` | the soft detent, like the optional hover sound |
| A slice runs on release | none (the finger is lifting, nothing to feel) | avoids a tick with no finger |
| The mode switches (extra click) | `Generic` | a distinct "click" from the hover ticks |
| The Settings hold completes | `LevelChange` | same as the pie opening |
| The hold is cancelled | none | silence means "nothing happened" |

**Rules**

- **Off the tap thread.** `perform` is called from the UI thread: the tap and recognizer threads send a message
  (`Msg::Haptic(HapticKind)`) through the `Notifier`, never call AppKit from the callback. S4 checks whether the
  call is safe from a background thread; until proven, the UI thread is used.
- **Rate limit.** At most one tick per 40 ms and none for a hover change that only flickers back to the previous
  slice within 80 ms, so a finger on a slice border does not buzz. Excessive feedback makes users turn all haptics
  off, which Apple warns about.
- **Respect the system.** The system's *Force Click and haptic feedback* setting stays in charge; there is no
  CursorPie override. If it is off, the calls do nothing.
- **Touch only.** A mouse press never calls it (the call would do nothing and costs a message). The source is
  carried on the gesture (`Source::Touch`), so a pie opened by the mouse is silent without any special case.
  External Magic Trackpads with a haptic actuator work through the same call; older trackpads without one are
  silent.
- **A setting for it.** `haptics` (default on when the touch trigger is on) and `haptic_hover` (default on), see
  [Config](#config-and-settings).
- **A later option, private and experimental:** the same private framework has an actuator interface
  (`MTActuatorOpen`, `MTActuatorActuate`, `MTActuatorClose`) that plays numbered waveforms and is used by a
  community tool to drive the trackpad's actuator directly. It would allow different feels for each event, but it
  is undocumented, can break with any macOS release and, being for the actuator itself, bypasses the user's
  system haptic setting. It is **not** part of the first release; if added, it sits behind its own setting that is
  off by default and falls back to the public API.

## Mouse and trackpad together

The goal is that nobody has to choose. These rules make the two sources one input.

| Situation | Rule |
|---|---|
| Both devices idle | Either can start a gesture; the first one to arm wins |
| A mouse gesture is running | Touch frames are ignored until the gesture returns to `Idle` |
| A touch gesture is running | Mouse trigger presses are ignored (they pass through); the **mouse right-click still switches the mode**, because it acts on the shared pie, not on the source |
| The user moves the mouse during a touch hold | The pointer moved without the finger moving: the hold is cancelled (S9 checks that the two can be told apart) |
| A mouse is connected, the touch trigger is on | Nothing changes for the mouse; trackpad touches still count |
| An external Magic Trackpad connects or disconnects | Rescanned within 5 s; no restart |
| Two trackpads (built-in and external) | Both are watched; a hold is tracked per device, and the first to arm owns the gesture |
| The trigger button is not the middle button (macOS allows extra buttons) | Touch ignores it: the touch trigger has its own setting; the mouse trigger keeps working |
| Touch trigger switched off in Settings | The source stops (`MTDeviceStop`), no frames are read, no private call is made after that |

Same behavior, one set of timings: the hold time (`pie_hold_ms`), the Settings hold (`settings_hold_ms`), the
double-click window (`double_click_ms`), the instant flick, the app and general profiles, `target_app` and
`focus_target` apply to both sources. The trackpad does **not** get a copy of them. A setting that exists only for
touch carries the `touch_` prefix and lives in its own panel.

Trackpads are **not** added to `mice.json`. That book exists to learn the extra buttons of a mouse; a trackpad has
none that matter, and mixing them would make a trackpad "a new mouse" in the notes of Settings. The touch devices
are only listed in the new panel (name and whether haptics are available).

## Config and Settings

Following the recipe in `guides::extending` ("Add a setting"). All fields are new, default off or neutral, and
the config version stays 2 because the struct is `#[serde(default)]`; an older file loads unchanged.

| Field | Type | Default | Range / values | Meaning |
|---|---|---|---|---|
| `touch_trigger` | bool | `false` | | Master switch. Off: the private framework is never loaded |
| `touch_still_mm` | f32 | `2.0` | 0.5 to 6.0 | How little the holding finger may move before the hold counts |
| `touch_click_action` | enum | `SwitchPie` | `SwitchPie`, `PickHovered`, `Nothing` | What the extra click does |
| `touch_typing_guard_ms` | u64 | `400` | 0 to 2000 (0 = off) | No arming this soon after a key press |
| `haptics` | bool | `true` | | Haptic ticks on a trackpad (macOS) |
| `haptic_hover` | bool | `true` | | Tick when the hovered slice changes |

`sanitize.rs` clamps the numbers and resets unknown values. The default for `touch_trigger` is `false` because a
finger resting on a trackpad is the most common thing a person does with it; opening a pie by surprise would be a
bug, not a feature (see [Obstacles](#obstacles-and-solutions)).

**Settings UI.** A new panel **Trackpad** at the end of **Behavior** (`ui/settings/tabs/behavior.rs`), shown
only on macOS (`Os::has_trackpad()`, a new capability next to `has_tray()`). It holds: the switch,
*How still* (a slider with anchors, like the other sliders), *Extra click*
(a drop-down), *Haptic feedback* and *Tick when the slice changes* (toggles), and a **status line**:
"Trackpad: Built-in trackpad (haptics available)", "No trackpad found", or "Not available on this macOS". A
short hint under the switch says that the system may call the second finger a click of its own. A live demo, like
`demo::hold_demo`, shows the hold with fingers instead of a button. The pie in **Menu** is unchanged.

## Obstacles and solutions

| Obstacle | Solution |
|---|---|
| A resting finger opens the pie by accident | Off by default; exact finger count; millimeter still-test; typing guard; palm and size filter; a separate, longer default for the touch hold may be needed (S6 measures false opens in a day of normal use) |
| Holding still is also how a click or scroll starts | If the finger moves before the hold time, the hold is cancelled and the system's movement is untouched; nothing is swallowed before the pie opens, so scrolling, tapping and dragging keep working |
| macOS reads the second finger's click as something else (secondary click, Look up, three-finger drag) | Classify by finger count and swallow the click while a touch pie is open; the panel says that CursorPie swallows it. Whether the tap can drop the system gesture itself is S3 |
| The private framework changes or disappears | `dlopen` with soft failure, the layout self-check, one file for the layout, an on/off setting, and the mouse always working |
| The tap cost grows with `MouseMoved` | Do not add it: read the cursor in the touch callback, only while a touch pie is open |
| Haptics from the wrong thread | Message to the UI thread; S4 |
| Different surface sizes (built-in, Magic Trackpad) | Distances in millimeters from the device size, not in normalized units |
| Hot-plug of a Bluetooth trackpad | Rescan every 5 s |
| Testing without a trackpad in CI | Pure recognizer fed with recorded frames; the native layer is small and checked by hand on a Mac |
| Permission surprises | The existing Accessibility permission is needed to post and swallow events; whether the touch source needs another one (Input Monitoring) is S7; the welcome window and Settings explain it if so |
| Edge-only triggering (a safer zone) | Optional later: a `touch_zone` setting (anywhere, or a corner) if false opens remain a problem |

## Alternatives considered

- **Force-click hold** (press hard and hold): gives a clear intent and no accidental opens, but it needs the
  pressure stream, which is not reliably available to a background app, and it is not what was asked for.
- **Tap, then hold** (tap-and-a-half): fewer false opens than a plain hold. Kept as a later `touch_trigger_style`
  option if S6 shows that the plain hold is too easy to trigger; the recognizer's state list already has room for it.
- **Make the trackpad emit a fake middle button** (the three-finger tap tools): reuses the mouse path with no
  recognizer, but has no hold, no hover and no finger count, so it cannot do this feature.
- **Public gesture events only:** would avoid the private framework but cannot give the finger count and
  positions that the hold and the extra click need (see the table above).
- **A kernel or HID driver:** far too heavy and needs privileges; rejected.

## Phases

Each phase leaves the mouse behavior unchanged and CI green; tests first.

| # | Phase | Exit criteria |
|---|---|---|
| 0 | This document and the [spikes](#phase-0-spikes) | reviewed; S1, S2, S4 answered before T2 |
| T1 | **The seam, no behavior.** Add `Source` to the gesture states; add the pure `touch::recognizer` and its tests; add the config fields, defaults, sanitize and tests; `Os::has_trackpad()` | all tests pass on macOS, Windows and Linux; a config without the fields loads as before; no new dependency |
| T2 | **Touch source on macOS.** Run-time loading of the framework, device filter, layout self-check, contact-frame callback, cursor reading, translation into `Ev` | spikes S1 and S5 pass; hold, hover and lift run a slice on a built-in trackpad; mouse unchanged |
| T3 | **The extra click.** `LeftMouse*` in the tap with the atomic guard, finger-count classification, `SwitchPie`, `PickHovered` | S2 and S3 answered; works with N = 1 and N = 2 and with tap-to-click on and off |
| T4 | **Settings.** The Trackpad panel, status line, demo, hint text, diagnostic events | the panel edits and saves; Undo works; hidden on Windows and Linux |
| T5 | **Haptics.** `platform/macos/haptics.rs`, `Msg::Haptic`, the rate limiter, the two settings | S4 answered; ticks felt on built-in and Magic Trackpad; silent with a mouse |
| T6 | **Hardening.** Hot-plug, two trackpads, mouse and trackpad switching, sleep and wake, a false-open measurement | checklist below passes on at least two macOS releases and both trackpad kinds |
| T7 | **Docs and release.** User guide, configuration, known issues, welcome text, changelog | [Documentation changes](#documentation-changes) done |
| later | Optional: actuator waveforms, Magic Mouse surface, `touch_zone`, tap-then-hold | each separately decided |

## Phase 0 spikes

Throwaway programs that answer one question each. None is merged.

| # | Question | Pass condition |
|---|---|---|
| S1 | On the macOS releases in use, can the contact-frame callback be registered by `dlopen` and `dlsym` from a Rust binary, and do the fields read by the recognizer (state, id, x, y, size) have the expected values? | a log of 20 s of touching with sane values on the built-in trackpad, a Magic Trackpad and (if present) the oldest and newest macOS available |
| S2 | While one finger rests, does the event tap receive the second finger's physical click and its tap-to-click as `LeftMouse*` or `RightMouse*`, with which fields, and can it be dropped? | for 1 + 1 and 2 + 1 fingers, press and tap, with the system's secondary-click setting at its default and changed: a table of event types, and the click does not reach the app when dropped |
| S3 | Does dropping the click also stop the system's three-finger Look up, three-finger drag and Mission Control swipe, or must the user turn them off? Do gesture events (type 29) in the tap help? | a yes or no per gesture, with the exact system settings that make N = 2 reliable |
| S4 | Does `perform` produce a tick from a background thread and while another app is in front? Which feature flag of `objc2-app-kit` is needed? Which of the three patterns feels like what? | a tick while the pie is open over another app, from the UI thread, and a decision on the thread |
| S5 | What is the latency and the rate of the contact-frame callback, and can the double-click (tap, then hold) be told from a hold? | callback under 2 ms of work, hover updates no slower than the mouse, and the tap-then-hold window works at 260 ms |
| S6 | How often does a still finger open the pie in normal use at 500 ms, 650 ms and 800 ms, with and without the typing guard? | fewer than one accidental open per hour of normal use at the chosen default |
| S7 | Does reading the touches need a permission beyond Accessibility (Input Monitoring) on a clean user account? | the exact prompt and System Settings page, or "none" |
| S8 | What is the CPU cost of adding `MouseMoved` to the tap against reading the cursor in the touch callback? | both measured while moving the pointer for 60 s; the cheaper one is used |
| S9 | Can a pointer that moved by the mouse during a touch hold be told from the finger moving it? | the touch coordinates show no travel while the pointer moved, on 20 trials |
| S10 | Do sleep, wake, lid close and a screen lock leave the touch source running, stopped or crashed? | it recovers without a restart; no stuck swallowed click |

## Test strategy

- **Recognizer (every OS).** Frames as data in test fixtures: a hold that opens, a hold broken by a slide, a hold
  broken by a palm, a hold with an extra click (press and tap), a second finger too early, a key
  press just before, two devices, a device that disappears. Assertions on `TouchEvent` and on the time of each.
- **Translation (every OS).** A test double for `dispatch` checks that a touch hold and a mouse hold produce the
  same `Msg` sequence (`ShowPie`, `Hover`, the release result) for the same pointer path, which is the proof of
  "seamless".
- **Config.** Defaults, clamps, and an old file without the fields loading with the defaults.
- **Haptic limiter (every OS).** Pure function of (time, event, previous) with a table of cases.
- **The native layer.** Too small to unit test and too dependent on the hardware; covered by the checklist.
- **CI.** The macOS job already builds and tests; no new job. `cargo tools ci` is unchanged.

## Manual verification checklist

Run on a Mac with a built-in trackpad and on one with a Magic Trackpad, once with a mouse connected and once
without. Nothing is signed off yet.

1. Off by default: a fresh config has `touch_trigger` false and the private framework is not loaded (the log
   says so).
2. With the switch on, a still finger for the hold time opens the pie under the pointer; lifting it in the center
   cancels.
3. Moving the finger hovers slices and enters submenus exactly as the mouse does.
4. Lifting over a slice runs it; the same slice with the middle button does the same thing.
5. A finger that slides before the hold time moves the cursor normally and opens nothing.
6. Typing, then resting a hand: no pie.
7. A second finger's press switches the pie (ring color changes); the same with tap-to-click.
8. The extra click never reaches the app under the pie (no stray click in a text field).
9. Haptics: a tick when the pie opens, soft ticks between slices, a distinct one on the mode switch; none with
    the mouse; none when the system haptic setting is off.
10. Use the mouse and the trackpad alternately for five minutes: no stuck state, no lost click.
11. Connect and disconnect a Magic Trackpad while running.
12. Sleep and wake; lock and unlock.
13. Switch the touch trigger off: the pie stops responding to fingers at once, the mouse still works.
14. Settings: Undo and Redo, Save, Cancel, Reset to defaults, import of a config that has the new fields and of one that
    does not.

## Security and privacy

- Raw touch data stays in memory for the frame being processed. Coordinates and finger positions are **never**
  written to the diagnostic log or to disk; the log gets counts and decisions only (the same rule as the other
  categories: "never which").
- No network access, no new telemetry.
- The private framework is loaded only after the user turns the feature on, and only by its fixed system path.
- Swallowing clicks is limited to the moment a touch pie is open and the finger count matches; everything else
  returns `Keep`. A watchdog resets the state if the touch source stops while a click is swallowed, so a click can
  never stay swallowed.
- Documentation must say plainly that the feature uses an undocumented system interface.

## Documentation changes

When the work lands, in the same pull requests as the code:

| File | Change |
|---|---|
| `docs/USER_GUIDE.md` | A "Trackpad" section: the gestures table, fingers, the extra click, the system settings to turn off, haptics |
| `docs/CONFIGURATION.md` | The seven fields in the field table and the example |
| `docs/KNOWN_ISSUES.md` | macOS: private framework, system gesture conflicts, no Magic Mouse surface, haptics need a Force Touch trackpad |
| `docs/INSTALL.md` | The permissions section, if S7 finds one |
| `docs/DEBUGGING.md` | The `touch` category and its events |
| `src/guides/architecture.rs` and `extending.rs` | The touch source and the recognizer; the recipe "Add a gesture source" |
| `docs/README.md` | The status of this record changes from plan to done |
| `CHANGELOG.md` | One entry per phase that changes behavior |
| Welcome window and **Behavior** tab text | Mentions of the trackpad on macOS only |

## Windows Precision Touchpad

**Status: implemented in code, not yet run on a Windows laptop.** The pure half (`src/tap/touch/ptp.rs`) is unit
tested on every OS; the Win32 half (`src/tap/touch/precision.rs`) is type-checked with `--features check-windows` and
has to be verified by hand (checklist below). Everything in the recognizer, the bridge, the extra click, the
config fields and the Settings panel is the same as on macOS; only the source of the fingers differs.

**Source.** A Windows Precision Touchpad (PTP) is a HID device on the *Digitizer / Touch Pad* usage page (`0x0D`/`0x05`).
Unlike on macOS this is a **public** interface: `RegisterRawInputDevices` with `RIDEV_INPUTSINK | RIDEV_DEVNOTIFY` on a
hidden window of its own thread delivers one `WM_INPUT` per report, also in the background. The descriptor
(`GetRawInputDeviceInfo(RIDI_PREPARSEDDATA)`, `HidP_GetValueCaps`) says where each finger slot's X, Y, Contact ID, Tip Switch
and Confidence are, and `HidP_GetUsageValue` / `HidP_GetUsages` read them. Hybrid mode (more fingers than slots in a
report) is put together by `ptp::Assembler`. Palm rejection is the device's own *Confidence* bit; the optional contact
width and height are not used. The surface size in millimeters comes from the physical range and unit of X and Y.

**What is shared / what differs.**

| | macOS | Windows |
|---|---|---|
| Source | private `MultitouchSupport.framework`, loaded at run time | Raw Input, public |
| Devices | trackpads (size filter) | precision touchpads (usage page), older touchpads are plain mice and are not seen |
| Extra click | `LeftMouse*` in the event tap | `WM_LBUTTONDOWN/UP` (and the right click of a two-finger tap) in the `WH_MOUSE_LL` hook, only while a touch pie is open |
| Pointer position | read from the system in the touch thread | the same (`platform::cursor_pos`) |
| Haptics | `NSHapticFeedbackManager` | none (the Settings switches are hidden) |
| Off | devices stopped, framework idle | `RIDEV_REMOVE`, nothing registered |

**Windows-only obstacles.**

| Obstacle | Solution |
|---|---|
| Windows' own gestures take the extra finger (two-finger tap is a right click, three-finger gestures) | classify by finger count, swallow the click while the touch pie is open (as on macOS); the panel names the Windows settings to change |
| A tap followed by a hold starts Windows' tap-and-drag and presses the left button under the pie | documented; turn off *Tap twice and drag to multi-select* (S-W5 checks if CursorPie can do better) |
| Report layouts differ between vendors | nothing is hard-coded: the layout is read from the descriptor, and a device whose descriptor is not understood is ignored and logged (`touch/device_skipped`, `why = no_layout`) |
| A resting finger may stop producing reports | assumed not (the PTP requirements ask for a steady report rate while a contact is present); if it does, the recognizer's stale rule cancels the hold after 500 ms (S-W2) |
| The physical size is missing or in an odd unit | `(0, 0)` is passed and the recognizer assumes a typical trackpad (130 x 90 mm) |
| Hot-plug, sleep | `WM_INPUT_DEVICE_CHANGE` and a 5 s count; the registration is renewed after a long gap between ticks |

**Spikes** (throwaway programs or `CURSORPIE_DEBUG=1` runs on a Windows laptop):

| # | Question | Pass condition |
|---|---|---|
| W1 | Does `RegisterRawInputDevices` for `0x0D`/`0x05` with `RIDEV_INPUTSINK` deliver `WM_INPUT` to a hidden top-level window while another app is in front? | reports arrive, one per ~8 ms while touching |
| W2 | Does a still finger keep producing reports? | yes, on at least two laptops (otherwise the stale rule must be loosened for this source) |
| W3 | Does `layout_from_caps` accept the descriptor of the test devices (5 slots, count, mm size)? | `touch/layout` is logged with the right size |
| W4 | Does Windows' default setup (tap to click, two-finger tap right-click) still deliver the click of the extra finger to the hook while a pie is open? | the extra click runs once per tap or press |
| W5 | Does a tap-then-hold click the app under the pie, and can the left press be swallowed instead of asking the user to change a setting? | decided from the result |

**Manual checklist (Windows).**

- [ ] Settings > Behavior > Trackpad shows *Precision touchpad found.* with the switch on and Saved; *No precision touchpad found* on a PC without one.
- [ ] Touch and hold one finger: the pie opens under the pointer after the hold time; moving the finger hovers a slice; lifting runs it; lifting in the center cancels.
- [ ] Sliding a finger before the hold time moves the pointer as usual and opens nothing; scrolling and tapping are unchanged.
- [ ] A palm resting on the pad opens nothing; a hand resting while typing opens nothing.
- [ ] With the pie open, a second finger tapped or pressed switches the mode once, and the app under the pie gets no click or right-click.
- [ ] The middle button of a mouse still works at any time, and a mouse right-click still switches the mode during a touch hold.
- [ ] Switching the feature off and Saving stops the source (`touch/source stopped` in the log); no `WM_INPUT` work happens afterwards.
- [ ] Unplugging or disabling the touchpad mid-hold ends the gesture; sleep and wake keep working.

## Open decisions for the maintainer

1. **What does "switch the mode" mean?** This plan proposes `SwitchPie` (like the right-click) as the default and
   `PickHovered` as an option. Is that right, or should the click cycle through the Mode slices of the ring?
2. **Fingers.** Decided: hold with one finger, click with one more. A hold with two or more fingers is not supported
   (the system scrolls with two fingers and takes three for its own gestures).
3. **Is a private framework acceptable** for the first trackpad release, given it is off by default, loaded at run
   time and fails soft?
4. **Default hold time for touch:** the same 500 ms as the mouse, or a longer one (S6 decides)?
5. **Magic Mouse's touch surface:** in scope later, or never?
6. **Private actuator waveforms:** in scope later, or only the public haptic API?
7. **Tap-then-hold** as a safer trigger style: worth planning now?
8. Whether Linux touchpads deserve the same design, using the portable recognizer. (Windows has it now, see below.)

## References

Read while writing this plan; the behavior of every item is still to be confirmed by the spikes.

- Apple, *NSHapticFeedbackPerformer* and its `perform(_:performanceTime:)`: developer.apple.com/documentation/appkit/nshapticfeedbackperformer
- Apple, *Handling Trackpad Events* (multitouch, gesture and mouse events from a trackpad): developer.apple.com/library/mac/documentation/Cocoa/Conceptual/EventOverview/HandlingTouchEvents/HandlingTouchEvents.html
- Apple, *CGEventType*: developer.apple.com/documentation/coregraphics/cgeventtype
- Apple mailing list, discussion of event taps and gesture events: lists.apple.com/archives/cocoa-dev/2017/Apr/msg00020.html and lists.apple.com/archives/accessibility-dev/2017/Apr/msg00002.html
- HS ModulesContrib multitouch: device list, contact-frame callback, device families: github.com/catokolas/HS_ModulesContrib-multitouch
- MiddleDrag and MiddleClick: three-finger tap and drag through the private framework, Accessibility permission, layout change on newer macOS: github.com/NullPointerDepressiveDisorder/MiddleDrag and lillodipiazza.github.io/middleclick
- EdgePad: trackpad-sized device filter and dynamic loading of the framework: github.com/forkiron/EdgePad
- mactic: actuator functions of the private framework: github.com/MatMercer/mactic
- BetterTouchTool community thread on three-finger tap conflicts with system settings: community.folivora.ai/t/tip-tap-middle-to-middleclick-action-not-reliable/45198
- `objc2-app-kit` bindings for `NSHapticFeedback`: docs.rs/objc2-app-kit

## Linux (X11)

The portable core (recognizer, bridge, extra click, config) is reused unchanged; Linux adds a native source and a
little X work. No haptics (no standard Linux interface), so `Os::has_haptics` is Mac only.

- **Why evdev.** With libinput, X11 clients get no touch positions from a touchpad, so `src/tap/touch/evdev.rs`
  reads `/dev/input/eventN` (multitouch protocol B) directly. This needs the `input` group; without it the status
  says `NoPermission` and nothing else changes. Touchpads are found in `/proc/bus/input/devices`
  (`platform/linux/inputdev.rs`): they have `BTN_TOOL_FINGER` and `ABS_MT_POSITION_X/Y`, no `INPUT_PROP_DIRECT`, and
  are not a program's virtual device. The surface size in mm comes from the axis resolution.
- **Frames.** A resting finger sends no events, so a reader repeats the last frame every 20 ms while a finger is
  down; the recognizer's timers (arm, tap) then work as on macOS. A thread per device feeds one `Sink`; a device
  that vanishes cancels its gesture. Devices are rescanned every 5 s (hot plug) and nothing is opened while
  `touch_trigger` is off.
- **Extra click.** X grabs the left and right buttons (`XIGrabButton`) only while a touch hold is active, so the
  click of the extra finger reaches `touch::extra` and can be swallowed; any other click is replayed at its
  release (`pending_basic` in `tap/linux.rs`). The grabs are kept while a kept or swallowed click is still down.
- **Typing guard.** `XI_RawKeyPress` is selected only while `touch_trigger` is on, and only calls `touch::note_key`.
- **Not verified:** the thread, the `EVIOCGABS` ioctl, permissions and the grab timing on real hardware. The parser,
  the repeat-frame logic, the device detection and the grab arithmetic are unit tested.
- **Not covered:** Wayland (the mouse layer is X11 only), touchscreens, tablets.
