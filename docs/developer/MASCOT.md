# The mascot

The bubblegum octopus lives in one place: `src/ui/mascot.rs` (code) and `assets/picto/mascot/` (the pictures). A screen
that wants it needs one line and a pose. Nothing else in the UI knows how a picture is read, shrunk or kept.

## Use it

```rust
use crate::ui::mascot::{self, Pose, Style};

mascot::show(ui, Pose::Success, 56.0);                    // in the layout, like a label
mascot::paint(&painter, Pose::Idea, rect, Style::Full);   // at a rectangle you chose
mascot::paint_faded(&painter, Pose::Welcome, rect, page, 0.18, Style::Watermark); // behind a page
```

* `show` keeps its space even when the picture cannot be made, so a layout never depends on it.
* `paint` and `paint_faded` return `false` when the picture cannot be made; nothing panics.
* Pictures are made on first use, at 128, 256 or 512 pixels (the smallest that is sharp where it is drawn), kept in the
  egui context (at most `mascot::CAPACITY`) and freed by `mascot::release` when Settings closes and when the UI sleeps.
  A new screen needs nothing for this.
* For a sheet (`widgets::sheet`), `sheet_heading_pose(ui, pose, title)` replaces `sheet_heading`. It is 14 points taller,
  so give the sheet 14 more points of height.

## Add a pose

1. Put the SVG in `assets/picto/mascot/`, named in `snake_case` after the moment it fits: `low_battery.svg`. No number in
   front, no `octopus` (the folder says it). 512 x 512, plain shapes only (the renderer draws no text, filters or linked
   pictures).
2. Add one line to the `poses!` list in `src/ui/mascot.rs`: `LowBattery => "low_battery",`.
3. `cargo test mascot`. One test fails when the folder and the list disagree; another draws every pose.

Every pose is compiled into the program (about 270 KiB for all of them). To drop one, delete its file and its line.

## Where it is today

| Screen | Pose | How |
|---|---|---|
| Welcome, first page | `Welcome` | faint watermark behind the logo (`Style::Watermark`), placed as a fraction of the page |
| Welcome, last page | `Celebrate` | in the free margin right of the toggles, only when that margin is at least 150 points |
| Update available | `Notification` | in the sheet's heading |
| Downloading the update | `Downloading` | in the sheet's heading |
| Update finished / not finished | `Success` / `Sad` | in the sheet's heading |
| Export failed | `Error` | in the sheet's heading |

## Where it belongs (and where it does not)

The mascot says something: a welcome, a result, a problem, a page with nothing on it. So:

* **One per screen at most**, small, and never over text. If there is no free corner, there is no mascot.
* **Yes:** the welcome guide, the heading of a sheet that brings news or a result, an empty list ("nothing here yet").
* **No:** list rows, toolbars, buttons, the pie, the HUD, anything that appears again and again while someone works.
* Match the pose to the mood of the message: `Success` for good news, `Sad` or `Error` for a failure, `Offline` for no
  network, `PleaseWait` or `Working` for something slow. Do not use a happy pose on a failure.
* Prefer the `Full` style for a small mascot next to text and `Watermark` only for a large background.

## Poses

| Group | Poses |
|---|---|
| The app talking | `Welcome`, `GetStarted`, `WhatsNew`, `Notification`, `Goodbye`, `Gift` |
| How it went | `Success`, `Celebrate`, `Winner`, `MessageSent`, `Error`, `Report`, `Sad`, `Angry`, `Confused`, `Surprised`, `Sick` |
| What it is doing | `Thinking`, `Working`, `Searching`, `Downloading`, `PleaseWait`, `Offline`, `Secure`, `Settings`, `Idea` |
| Moods and pastimes | `Base`, `Cool`, `LoveIt`, `Sleeping`, `CoffeeBreak`, `Music`, `Reading`, `Photo`, `Painting`, `Cooking` |

`assets/picto/source/bubblegum.svg` is the large master drawing; it is not compiled in. The `assets/picto/picto_*.png`
files are the pictures of the README.
