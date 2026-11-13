# Trackpad hold (macOS, Windows and Linux)

On a Mac a trackpad, on Windows a **precision touchpad** and on Linux a touchpad can do what the middle button does. It is **off by default**: turn on *Open the pie with a
trackpad hold* in **Settings > Behavior > Trackpad** (and press Save). A mouse keeps working exactly as before, and
a mouse and a trackpad can be used in any order, with the same pie, hold time, double-click speed and profiles.

## The gesture

| You do this | With a mouse | With a trackpad |
|---|---|---|
| Open the general pie | Hold the middle button | Touch and hold still with the chosen number of fingers for the hold time |
| Choose a slice | Move toward it | Move the finger toward it |
| Run it | Release the button | Lift the finger |
| Cancel | Release in the center | Lift the finger in the center |
| Open Settings | Keep holding in the center | Keep holding in the center |
| Switch the mode | Right-click | Click (or tap) with one more finger |
| Open the app's own pie | Double-click the trigger | Tap, then touch and hold again within the double-click speed |

## Details

- One finger holds and one more finger clicks. A hold with two fingers is not supported: the system scrolls with
  two fingers, so the pointer would not move.
- **How still the finger must stay** is measured in millimeters on the trackpad. A finger that slides before the hold
  time ran out just moves the pointer, as always; scrolling, tapping and dragging are not affected.
- Palms and the thin border of the trackpad are ignored, and a hold does not start within the **typing guard** time
  after a key press.
- **Extra click** can switch the pie like the right-click (default), make the hovered Mode slice the active mode, or do
  nothing. The click never reaches the app under the pie.
- **Haptic feedback** (macOS only): a tick when the pie opens, a soft one when the slice changes and a distinct one
  when the mode switches. Only a trackpad ticks; the system's *Force Click and haptic feedback* setting stays in charge.

## Per system

On a Mac the fingers are read through an undocumented macOS interface, loaded only while the feature is on. If macOS
changes it, Settings says *Trackpad support is not available on this macOS* and the mouse is unaffected.

On Windows the touchpad's reports are read through Raw Input, a public interface, and nothing is registered while the
feature is off. Only a **precision touchpad** works: an older touchpad that Windows treats as a plain mouse sends no
finger positions, and Settings says *No precision touchpad found*. If a tap followed by a hold clicks the app under the
pie, turn off *Tap twice and drag to multi-select* in Settings > Bluetooth & devices > Touchpad.

## On Linux (X11 and Wayland)

- Status: on X11 this is part of the *pre-stable* Linux support; on Wayland it is *experimental* (see [Linux](linux.md)).
  The device reading and the permissions have not yet been verified on many touchpads.
- Everything above works the same way, except **haptic feedback**: a Linux touchpad has no standard way to tick, so
  those switches are not shown.
- The fingers are read straight from `/dev/input/event*` (libinput does not pass touch positions on to X11), and
  only while *Open the pie with a trackpad hold* is on. Your user must be in the **`input` group**:
  `sudo usermod -aG input $USER`, then log out and in. Without it Settings says so under the switch; the mouse is
  unaffected. Only touchpads are opened (never a touchscreen, a graphics tablet or a program's virtual device), and
  nothing is recorded: positions are used to decide and then dropped.
- While a hold is running, the left and right buttons are grabbed so the click of the extra finger never reaches the
  app; a click that is not the extra one is sent again when it is released, a moment late.
- *Ignore holds after typing* hears that a key was pressed (not which key) while the switch is on.
- Nothing happens? Run `cursorpie --touchpad-debug` in a terminal: it says which step fails (switch not saved, no
  `input` group, device not recognized, the hold not still enough) and shows your fingers live.
- On Wayland the same switch works: while a hold runs CursorPie takes the touchpad away from the desktop, so the
  extra finger's click and the scrolling do not reach the app below. It needs the udev rule (or the `input` group).
