# CursorPie on Linux

Everything a Linux user needs in one place: how mature each session type is, what to set up and what to expect.

## Support level

| Session | Level | What it means |
|---|---|---|
| **X11** (Xorg, i3, XFCE, Plasma 6.7 and older, any desktop's X11 session) | **Pre-stable** | The whole feature set works and needs no permission. It is implemented, built and tested on every release, and in use. The manual checklist on real desktops is still being completed, so rough edges are possible. Report them. |
| **Wayland** (KDE Plasma 6.8+, GNOME, Hyprland, Sway, others) | **Experimental** | Written and unit tested, but not yet run on real Plasma, GNOME, Hyprland or Sway machines. It needs a udev rule. Expect bugs; the Settings > General tab says what is missing. |

*Pre-stable* means feature-complete and in use, but not yet declared stable. *Experimental* means it may fail in ways
that are not documented yet. Windows is stable; macOS is supported.

Which one am I on? Run `echo $XDG_SESSION_TYPE`: it prints `x11` or `wayland`. KDE Plasma 6.8 (October 2026) and GNOME
50 no longer offer an X11 session, so on those desktops you are on Wayland.

## Packages

x86_64 archives are built for **Ubuntu 24.04+** and **Fedora 44**. Other distributions can build from source
(see [Install](install.md#linux)).

## X11: nothing to set up

Install, start CursorPie and hold the middle button. In an X11 session it needs no permission. It can start at login from
**Settings > Behavior**. Only the middle button can be the trigger.

## Wayland: one extra step

A Wayland program may not watch other programs' mouse buttons. CursorPie therefore grabs your mouse through
`/dev/input`, and writes everything it does not use to a virtual mouse it creates. That needs the udev rule
`70-cursorpie.rules` from the package:

```sh
sudo cp 70-cursorpie.rules /etc/udev/rules.d/
sudo udevadm control --reload && sudo udevadm trigger
```

Then **log out and in once**. `install.sh` installs the rule by itself when run as root; otherwise it prints this command.
The rule gives your logged-in user access to `/dev/uinput`, mice and touchpads (never keyboards); no group membership
is needed. Remove the file to take the permission away.

CursorPie's windows still run through XWayland, so XWayland must be running (it is by default).
**Settings > General** shows a live report: mouse access, mouse capture and the compositor adapter.

### What works on each compositor

Mouse capture and shortcuts work on every Wayland compositor. *Per-app profiles* and "bring the app forward" need to
know the pointer and the windows, which each compositor must be asked in its own way:

| Compositor | Per-app profiles, app under the pointer, bring app forward | Needs |
|---|---|---|
| KDE Plasma | yes | nothing more (a KWin script loaded over D-Bus) |
| Hyprland | yes | nothing more (its control socket) |
| GNOME (Shell 47 to 50) | yes, after installing the extension | the extension below |
| Sway | yes, except "app under the pointer": it uses the app in front, because Sway cannot say where the pointer is | nothing more |
| COSMIC, niri and others | no: only the general profile is used | not available yet |

None of these adapters has been run on a real machine yet; all are *experimental*.

**GNOME extension.** Copy `gnome-extension/cursorpie-adapter@sebastvnd.github.io` from the package to
`~/.local/share/gnome-shell/extensions/`, log out and in, then run
`gnome-extensions enable cursorpie-adapter@sebastvnd.github.io`. It sends only window ids, app names and positions,
never titles.

### Forcing X11 mode

`CURSORPIE_BACKEND=x11 cursorpie` uses the X11 layer inside a Wayland session. It then sees only X11 and XWayland windows,
so the gesture works over those only. Use it for bug reports, not daily use.

## Things that are the same on every Linux session

- **No tray icon yet.** Pause and Quit are in **Settings > General**; Settings opens by holding the trigger in the middle
  of the pie. The welcome window appears only on the first start.
- **Trackpad hold** reads the touchpad from `/dev/input`: your user must be in the `input` group
  (`sudo usermod -aG input $USER`, then log out and in), or on Wayland have the udev rule. If nothing happens, run
  `cursorpie --touchpad-debug` in a terminal. See [Trackpad hold](trackpad.md#on-linux-x11-and-wayland).
- **App icons** come from the `Icon=` of the app's desktop entry. A PNG is read directly; an SVG-only icon needs
  `rsvg-convert` (`librsvg2-tools` on Fedora).
- **File chooser buttons** need `zenity` or `kdialog`. The Ask block also needs one of them.
- **Clipboard blocks** need `wl-copy` (Wayland) or `xclip` (X11); notifications need `notify-send`.
- App ids are desktop-entry ids in lower case (for example `firefox`), the same on X11 and Wayland, so a profile matches
  the same app in both.
- A minimal system may lack libraries the window toolkit loads at run time (OpenGL, libxkbcommon-x11, libxcursor).
- Config lives in `~/.config/cursorpie/` (or `$XDG_CONFIG_HOME/cursorpie/`).

## Reporting a problem

Say which distribution, desktop, session type (X11 or Wayland), GPU and display scaling you use, and attach the
diagnostic log if you can (`CURSORPIE_DEBUG=1 cursorpie`, see [Debugging](../DEBUGGING.md)). Known limits are in
[Known issues](known-issues.md#linux).
