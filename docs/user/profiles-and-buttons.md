# Profiles and mouse buttons

How one pie becomes many (per-app profiles), how the back and forward buttons choose which pie they follow, and how
CursorPie knows your mouse's extra buttons.

## Contents

- [Profiles](#profiles)
- [Side buttons and app pies](#side-buttons-and-app-pies)
- [Your mouse and its extra buttons](#your-mouse-and-its-extra-buttons)

## Profiles

A profile is a pie plus the apps it applies to. Up to 16 profiles are allowed.

- Apps are identified by bundle id on macOS (for example `com.apple.Safari`), executable name on Windows (for
  example `chrome.exe`, not case sensitive) and desktop-entry id on Linux (for example `firefox`).
- There is **exactly one general profile**: the one with **no apps**. It serves every app without a profile of
  its own, is created for you and cannot be deleted.
- Every other profile **belongs to at least one app**. **New profile** asks for a name and an app, and the last
  app of a profile cannot be removed.
- Two ways to combine them, switchable in **Behavior**:
  - *Hold = general pie, double-click = pie of the app* (default).
  - *Classic*: hold opens the app's own pie if it has one, otherwise the general pie.

## Side buttons and app pies

An app with its own profile has two pies: its own and the general one. The back and forward buttons follow
exactly one of them. Each pie shows which one with its ring color: **accent = the side buttons follow this pie,
red = they follow the other one** (the colors can be changed).

- **Default:** the app's pie. Pick the general pie instead in **Settings > Behavior > Side buttons**.
- **Choosing:** pick a Mode slice in a pie and the side buttons follow that pie from then on, in that app. Hold
  the trigger, pick *History* in the general pie, and Undo and Redo are on your side buttons even in an app with
  its own pie.
- **Switching:** with a pie open, **right-click**. The pie stays open and its ring changes color. Without
  opening a pie, use *Switch side-button pie* in the menu bar; the pie that now owns the side buttons appears
  for a moment (time adjustable, 0 turns it off).
- The choice is remembered per app while CursorPie runs. It can be forgotten when you switch to another app, and
  saving Settings clears it.

**Which app is "the app"?** By default the app whose window is **under the pointer**, so you do not have to
click a window to focus it first. Shortcuts always go to the app in front, so CursorPie brings that app to the
front right before it sends one, unless it already is. Both are options in Behavior: switch to *the app in
front* for the old behavior. If CursorPie cannot tell which window is under the pointer, it uses the app in
front.

## Your mouse and its extra buttons

A Mode slice lists one action for each extra button your mouse has: **Back button**, **Forward button**, then
**Extra button 6**, **Extra button 7** and so on. Which buttons are listed depends on the mouse CursorPie thinks
you use. A button with the action *None* keeps working in the app as before.

CursorPie keeps a profile of every mouse it has met (the file `mice.json` next to `config.json`, saved at once,
not with Save). Every few seconds it looks at the connected mice and tells a **new** mouse from one it **knows**
(a note appears in Settings). Two mice of the same model share one profile, because their buttons are the same.

**Settings > General > Mouse:**

| Button | What it does |
|---|---|
| Scan this mouse | Press every extra button of the mouse once, then **Done**. While the scan runs the buttons do nothing else. What you pressed is stored in the profile and wins over what the system reported. Cancel, or 45 seconds without a press, ends the scan |
| Detect again | Asks the system which mice are connected now |
| Forget | Removes a mouse from the list. It is met as a new mouse the next time it is connected |

What the system can tell about your mouse differs:

- **Linux (X11 and Wayland):** the kernel's list of input devices gives the name, the hardware id and the extra buttons.
  Scan if a button is missing or one is listed that does not exist. On X11 a thumb button reaches CursorPie as an X
  button numbered 10 to 20 (button 5 of a mode is X button 10); on Wayland it is read from the device itself, so a
  scan hears the physical button, not a remapped one.
- **Windows:** only the number of buttons, and only two extra buttons (back and forward) are seen by any program
  that watches the mouse. A mouse with a different number of buttons counts as another mouse.
- **macOS:** the system does not list mice for CursorPie, so scan the mouse once; the result is kept.

The trigger button is not offered as an extra button of a mode. If your mouse does nothing when scanned, its
extra buttons may be handled by the maker's own software.
