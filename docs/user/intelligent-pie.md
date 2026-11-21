# Intelligent pie and intelligent slices

CursorPie can learn what you use and put it under your pointer: the apps you open most, the apps that are open now,
the pages you visit most, the pages that are open, what you run most from the pie in the app in front, and your flows.
This page says how to use it, what it learns, and what it never does.

## Contents

- [Quick start](#quick-start)
- [The intelligent pie](#the-intelligent-pie)
- [Intelligent slices](#intelligent-slices)
- [What it suggests](#what-it-suggests)
- [What it learns, and when](#what-it-learns-and-when)
- [Privacy](#privacy)
- [Use cases](#use-cases)
- [Settings reference](#settings-reference)
- [Limits](#limits)
- [Troubleshooting](#troubleshooting)

## Quick start

1. Open **Settings > Behavior > Intelligent** and switch **Learn and suggest** on. It is off until you do: nothing is
   observed or stored before that. Press **Save**.
2. Use your computer as usual for a day or two. Press **Learn now** whenever you want to see what was picked up
   (the page says *how many apps, pages and actions*).
3. Hold the trigger to open a pie, then **hold Space** (about one second). The pie turns into the intelligent pie.
   Hold Space again to come back to the pie you had. Until you do, the intelligent pie is **kept**: closing the pie
   does not end it, and the next presses open it again.

A quick press of Space still opens Settings, as before.

## The intelligent pie

While a pie is open (any ring, a submenu, or the pie of an app), hold Space for the time set in
**Hold Space to switch** (900 ms by default, 400 to 5000):

| You hold Space in... | The pie turns into... |
|---|---|
| any normal pie | the **intelligent pie**: every slice is a suggestion |
| the intelligent pie | the **pie you came from** (its top ring) |

The mouse button stays down the whole time: release it over a suggestion to run it, exactly like a normal slice. The
center of the intelligent pie shows a sparkle.

Two layouts, chosen in Settings:

- **Mixed** (default): one ring with the best suggestions of every source together, best first. Up to 3 to 8 slices
  (**Slices in the mixed pie**).
- **Grouped**: one slice per source (*Open apps*, *Frequent pages*, *Actions for this app*...). Each opens its own ring
  of suggestions like a submenu: drag outward across the rim.

When there is nothing to suggest the pie has one slice that says why: *Intelligent is off*, *Still learning* or
*Nothing to suggest*.

### Space: press or hold

With the intelligent pie **on**, the key-down of Space cannot yet tell whether you mean to open Settings or to switch
pies. So:

- **Held** for the hold time: the pie switches. Settings does not open.
- **Pressed and released quickly**: Settings opens, **on the key-up** (a tiny delay compared to before).

With the intelligent pie **off**, nothing changes: Space opens Settings on the key-down.

If you switched **Open settings with Space** off, a quick press does nothing and a hold still switches.

## Intelligent slices

An **intelligent slice** is a submenu whose slices are suggested when the pie opens instead of written by you. It can
sit in any pie, next to ordinary slices, at any level.

To add one: on the Intelligent page use **Intelligent slice > Add** (general pie or the pie of one app), choose
*Intelligent* under *Add a slice > Custom*, or select a slice in the pie editor and choose **Intelligent** next to
*Action*, *Side buttons* and *Submenu*. Switching the feature on adds one to the general pie.
Then choose:

- **Sources**: what it draws from. None chosen means everything that is switched on in Behavior > Intelligent. Pick
  one (say *Open pages*) and the slice is a "what is open in my browser" ring whatever the global switches say.
- **Suggestions**: how many slices at most (1 to 8).

Give it a name and an icon like any slice. Drag outward across its rim to open its ring. While the intelligent pie is
off, an intelligent slice is empty and does not open.

In `config.json` it is `{"Smart": {"sources": ["OpenPages"], "count": 6}}` (see [Configuration](configuration.md)).

## What it suggests

| Source | Suggests | Needs |
|---|---|---|
| **Open apps** | Apps that are running now, the ones you use most first | The app was seen in front before |
| **Frequent apps** | The apps you use most, weighed by time spent, how lately, the hour of the day and the app you usually go to *after* the one in front | |
| **Open pages** | One slice per site that is open in your browser, going to that tab | The [browser extension](../BROWSER_EXTENSION.md) |
| **Frequent pages** | The sites you visit most | The browser extension |
| **Actions for this app** | What you run most from the pie while this app is the target: shortcuts, commands, links, sub-actions | You ran it from the pie before |
| **Flows** | Your flows, the ones you run most first (never-run flows come last) | Flows in the Flow tab |

The app in front and the page in view are never suggested: you are already there. CursorPie itself is never suggested.

How the sources are mixed in one ring: each source ranks its own list, the best of each counts as 1.0 and every next
one 0.8 of the one before, then the sources are weighed against each other (open apps and actions 1.0, open pages
0.95, frequent apps 0.9, frequent pages 0.85, flows 0.8). So the second app is worth about as much as the best page,
and one source does not fill the whole pie unless it is the only one asked for. A slice offered twice (a flow that is
also a learned action) appears once.

### The ring does not move under your pointer

The suggestions are made once per gesture (when you press the trigger) and kept until you release it. A slice you see
under the pointer is the slice that runs, even if the system changes meanwhile.

## What it learns, and when

CursorPie looks at the system every **Look every** seconds (10 by default) while learning is on:

- **Apps**: which app is in front. The time since the last look is credited to it. It counts a *visit* when the app
  comes to the front and remembers which app came before (so it can suggest "after Figma you usually open Slack").
  A gap longer than three looks (sleep, lid shut) is not credited and links nothing.
- **Pages**: when a browser is in front and the extension is connected, the page in view, as its **site**. A browser
  is recognized by its name (Chrome, Firefox, Safari, Edge, Brave, Vivaldi, Opera, Arc, Zen and others).
- **Actions**: every time you release a slice that runs something, the action, the slice's name and icon, and the app
  that was the target. This happens when you use any pie, intelligent or not.

Everything fades: the half-life is a quarter of **Forget after** (30 days with the default 120). A habit older than
**Forget after** days, or too weak to matter, is deleted. At most 300 apps, 300 pages and 200 actions are kept; the
weakest makes room.

**Time of day** is counted from the system clock in UTC. That is not your hour, but the same shift is used when
learning and when asking, so "what I open at this time" works without knowing your time zone.

### Idle learning

Looking is cheap. The heavy part, **tidying** (fading old habits, forgetting, finding the names of the installed apps,
saving), runs:

- with **Tidy up only when idle** on (default): after the pointer has been still for **Idle after** seconds (60 by
  default), and at most every ten minutes;
- with it off: about a minute after something changed.

**Learn now** (Settings > Behavior > Intelligent) does it at once, whatever the schedule says. What is learned in
between is kept in memory and saved at most every five minutes, so a crash loses a few minutes at the most.

## Privacy

- **Off until you switch it on.** Nothing is observed, stored or suggested before that.
- **Local only.** The habits are one file, `smart-habits.json`, in the same folder as `config.json`
  ([where](configuration.md#where-the-file-lives)). It is readable only by you (`0600` on macOS and Linux). Nothing is
  sent anywhere.
- **Never read:** window titles, page titles, page content, what you type, files you open. A page is its **address
  only**, and by default only its **site** (`github.com`). A path is kept only if you turn on **Keep the path of a
  page**, and then only its first two parts. The query (`?token=...`), the `#fragment`, a user name, a password and a
  port in an address are always dropped.
- **Per kind:** switch **Learn apps**, **Learn web pages** and **Learn what I run from the pie** off one by one.
- **Never learn these apps / sites:** an ignored app is not learned, not suggested, and not even remembered as "the app
  that came before" another. A site on the list covers its subdomains (`bank.example` covers `www.bank.example`).
  Entries already learned are removed at the next tidy-up.
- **Forget everything** deletes every habit and the file at once. It cannot be undone.
- A profile you export or share never carries habits. An intelligent slice in it is only the instruction "suggest
  here"; the suggestions are made on the computer that opens it, from its own habits.

## Use cases

**Jump between the apps of the job.** A designer lives in Figma, Slack and a browser. With *Frequent apps* and *Open
apps*, the pie opened in Figma puts Slack first, because after Figma that is where she usually goes.

**One ring for "what is open in my browser".** Add an intelligent slice named *Tabs*, sources *Open pages*, to your
general pie. Dragging out of it shows one slice per site; releasing goes to that tab. No tab bar to hunt in.

**The few sites you open every day.** An intelligent slice with *Frequent pages* is a dynamic bookmark bar that
follows your habits, without ever being edited. Keep **Keep the path of a page** off and it stays a list of sites.

**Per-app shortcuts that learn.** In an editor you run *Format* from the pie ten times a day and *Rename* twice. The
*Actions for this app* source puts *Format* in the intelligent pie while that editor is the target, and not in other
apps.

**A morning routine.** A flow that opens mail, calendar and chat is suggested first at the time of day you usually
run it, when **Weigh the time of day** is on.

**Quiet by design.** A banking site and a password manager on the ignore lists are never learned or shown, and a
shared screen never shows them in the pie.

**Try it without commitment.** Switch **Learn and suggest** on, use the computer for a day, look at the intelligent
pie. If you do not like it, switch it off (the pie goes back to being yours alone) and press **Forget everything**.

## Settings reference

All on Settings > Behavior > Intelligent. Switches and sliders apply on **Save**; **Learn now** and **Forget
everything** act at once. The same fields live in the `smart` object of `config.json`.

| Setting | `config.json` | Default | Meaning |
|---|---|---|---|
| Learn and suggest | `enabled` | off | The master switch |
| Hold Space to switch | `hold_ms` | 900 | Hold time, 400 to 5000 ms |
| Layout | `layout` | `Mixed` | `Mixed` or `Grouped` |
| Slices in the mixed pie | `pie_slices` | 8 | 3 to 8 |
| Weigh the time of day | `time_of_day` | on | Raise what is usual at this hour |
| What it suggests | `sources` | all six | `OpenApps`, `FrequentApps`, `OpenPages`, `FrequentPages`, `Actions`, `Flows` |
| Learn apps / web pages / what I run | `learn_apps`, `learn_pages`, `learn_actions` | on | What may be learned |
| Tidy up only when idle | `learn_when_idle` | on | Do the heavy part only when idle |
| Idle after | `idle_s` | 60 | Still pointer time that counts as idle, 10 to 3600 s |
| Look every | `sample_s` | 10 | 2 to 120 s |
| Forget after | `keep_days` | 120 | 7 to 730 days; the half-life is a quarter of it |
| Keep the path of a page | `keep_page_paths` | off | Keep two path parts, not only the site |
| Never learn these apps | `ignore_apps` | empty | App ids, up to 64 |
| Never learn these sites | `ignore_sites` | empty | Sites, subdomains included, up to 64 |

## Limits

- **Wayland.** CursorPie does not see keys on Wayland (see [Linux](linux.md)), so the hold does not work there.
  Intelligent slices work everywhere. On macOS and Windows both work; on Linux with X11 the hold works, but, as with
  Settings, the Space key also reaches the app under the pie (the keyboard is watched there, not taken).
- **Looking every few seconds** misses an app that is in front for less than that. It is a habit counter, not a
  time tracker.
- **"Open now" apps** are only the apps CursorPie has already seen in front; it asks the system about the 40 strongest
  habits when you press the trigger, not about every process.
- **Open pages** need the browser extension and show sites, not tab titles (the extension does not send titles).
  Only the showing tab of the browser heard from last counts as "in view".
- **Names of apps** are found when the habits are tidied (the installed apps are listed then). Until the first tidy-up
  a suggested app is named from its id (`com.apple.Safari` becomes *Safari*); **Learn now** names them at once.
- **Time of day** uses UTC buckets (see above); a change of time zone is learned away by fading.

## Troubleshooting

| Symptom | Cause and fix |
|---|---|
| Holding Space does nothing | **Learn and suggest** is off or not saved; or Wayland; or the hold is shorter than **Hold Space to switch**: lower it |
| Settings opens when I mean to hold | Release came before the hold time: raise **Hold Space to switch** or hold longer |
| The pie says *Still learning* | Nothing is learned yet: use apps for a while, then press **Learn now** |
| The pie says *Nothing to suggest* | The sources that are on have nothing: check that the browser extension is connected for pages, or switch more sources on |
| An app has an odd name | Names come from the installed apps at tidy-up: press **Learn now** |
| An app or a site should never appear | Add it to the ignore lists; it disappears at the next tidy-up (**Learn now** for at once) |
| An intelligent slice does not open | It has nothing to suggest, or Learn and suggest is off |

## See also

[Using the pie](using-the-pie.md) · [Settings](settings.md) · [Configuration](configuration.md) ·
[Browser extension](../BROWSER_EXTENSION.md) · [Flows](flows.md)
