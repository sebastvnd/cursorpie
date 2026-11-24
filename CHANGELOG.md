# Changelog

## Unreleased

## 0.9.4

- **Fixed (macOS): the pill and the lock animation stood still for a moment, then jumped ahead, and the pie glitched
  when they followed each other.** The log of a session shows the cause: after the lock animation (and after every
  action) the window was hidden and mapped again for the pill 10 to 12 ms later (`mac_window_state` `visible=false`, then
  `hud_show`), after being shrunk to 262x262 by `hide_root`, and an unlock while the pill was on screen did the same the
  other way round. A window that is shown is reported as occluded by the system for 3 to 60 ms (`occlusion=8192`) and the
  toolkit draws nothing in that time, so the first frame of the pill, which starts its clock, was drawn before the window
  was on screen or at the size of the previous owner, the request for the next frame was lost with it, and the circle
  stood still until something else woke the window. Now (macOS only):
  - the pie that has faded and the lock animation hand the mapped window to the notice that waits for it instead of
    hiding it first (`App::hand_to_pending_hud`), and a pie that is asked for while a notice is on screen takes the
    mapped window the same way (`drop_hud(.., keep_mapped)`);
  - whoever takes a window that is hidden or at another size or place draws nothing, a transparent frame that also wipes
    the last owner's picture, until the system has carried out the size and the place, and starts its clock then
    (`App::arm_geom_gate`, logged as `ui` / `geom_gate_done`, at most 150 ms);
  - while the lock animation, the padlock of an unlock or a notice is on screen, every pass of `App::logic` also asks
    for a frame on a timer (`App::step_anim_watchdog`), so a lost request costs a frame, not seconds.

- **Fixed: the memory of the pie editor (full screen above all) and an idle size that grew with every visit to
  Settings.** The pie editor laid out its icon glyphs and labels at `size × zoom` with the zoom as a continuous number
  (a wheel, a pinch, and the "fit" that full screen does), and egui cuts a new set of glyphs into its font atlas for
  every size it is asked for. On macOS that atlas is as wide as the GPU allows (16384 texels) and egui only starts it
  over when it is 80% full, so one zoom gesture could fill hundreds of MB on the CPU and the same again on the GPU, which
  the allocator did not give back (the trim after closing reported 0 MB), so each visit left the program a little
  larger. Now: the zoom of the glyphs goes through a ladder of 8 sizes per doubling (`fonts::zoom_ladder`, about 30 sizes
  over the whole range); the atlas is capped at 4096 texels a side (`App::raw_input_hook`); and Settings rebuilds the
  atlas as soon as it is taller than 1024 rows (`fonts::guard_atlas`, logged as `ui` / `atlas_guard`).

- **Fixed: the Flow canvas kept the allocator busy and the program stayed large after it was closed.** The canvas ran
  the flow checker on every frame (it walks the descendants of every block: a few thousand short-lived allocations for a
  flow of 100 blocks) and walked the flow twice more to count the reachable blocks, so merely having the canvas open
  filled the heap with holes that were not given back when Settings closed. The result of the checker is now kept
  until the flow, or the list of flows it may call, changes (`FlowIssues`), and the walk is done once per frame. When
  zoomed, the text of blocks that are out of the view is no longer laid out again at the zoomed size, so it no longer
  cuts glyphs (and grows the font atlas) for text nobody sees. Closing Settings and sleeping now also ask the trim
  thread for a second trim a moment later, when the renderer has let go of what the last frames used.

- **Fixed: glitches and a pulled, stretched picture when the one window changes hands.** The pie, the notice pill, the
  lock animation, a panel and Settings share one window, and what owned it was spread over a dozen flags that each
  transition had to keep in step. There is now one place that names the owner and the rules of a hand-over
  (`ui/app/owner.rs`, logged as `ui` / `owner`), and five things that came from the old way are gone:
  - A pie asked for while the last one was still fading, right after a notice, after the lock animation or right after
    Settings closed was drawn into a window that was still mapped for the old owner, so for a few frames it showed
    stretched or at the old place. On Linux that window is now taken down and mapped again the way the first pie after
    a pause is.
  - A submenu asked for in the first moments of the bloom of the ring above reconfigured the mapped window (the old
    stutter). A level of the same gesture now always keeps its window.
  - The pill of the slice names blinked out and in at every submenu, because it faded with the bloom clock that a
    submenu starts again. It now fades only when the pie closes, and the pill of a finished pie never comes back with
    the next one.
  - Opening Settings during the lock animation left the animation behind. It kept the Windows stand-by from mapping
    the window, so the next pie came late. Settings now lets go of the animation and of the pill.
  - The place of the slice-name pill is worked out once per pointer position instead of in every frame (a list of the
    displays, a round trip to the X server on X11, in the frames that animate the pill).

## 0.9.3

- **New: the download of an update shows a progress bar.** The sheet _Updating CursorPie_ now has a bar that fills as the
  package comes in, with the percentage and "12.4 of 31.0 MB" under it, and a soft shine that runs along the fill. While
  CursorPie looks for the package the bar is a piece that runs back and forth, and it is full while the download is
  checked and prepared. The size is read from the file on disk, so it works the same with `curl` and with `wget`.

- **Improved: the video cards of the website.** The clips of _Watch it work_ are now little windows (title bar, a tag
  for what the clip is about, a numbered caption) on a full-width stage. The clip in the middle is lifted and glows in
  the colour of its tag, the others sit smaller and faded, and the edges of the stage fade out. A click on a card brings
  it to the middle, the mouse can drag the row, and a thin progress rail replaces the dots.

- **Fixed: the sheet _Icon from a website_ grew wider when the icon arrived.** The address row kept 112 points for its
  button, but _Fetch again_ is wider, so the row overflowed and pushed the sheet out by about 30 points. The button now
  keeps its own width and the field takes what is left, so the sheet has the same width while loading and after.
- **Improved: the preview of the icon.** One card with the icon as the site serves it and as it will be on the pie, both
  on a checkerboard so a transparent area can be told from a plain one, with the name and size of the picture and the
  icon at about its real size on a light and a dark slice. It is painted in a fixed rectangle, so a long address can no
  longer widen the sheet.

- **Fixed: a warning button had no outline.** _Remove cpd_, _Forget everything_, _Quit CursorPie_ and the buttons of a
  panel with the _Danger_ look were drawn bare, next to buttons that had the outline and the shadow of the theme. They
  now have them too, in Settings and in panels, with red text that turns on a pale red wash under the pointer. A
  button that is only an icon (remove a step, delete in a toolbar) stays bare until the pointer is over it, like the
  plain icon buttons beside it.

- **Fixed: the sources of an Intelligent slice were one tall column.** _Open apps_, _Frequent apps_, _Open pages_ and
  the others now sit side by side and wrap to the next line when there is no room.

- **New: a _Pick..._ button for a "URL / file" action.** It opens the list of sites (the same sheet as _Choose site_ in
  Behavior > Intelligent): the pages that are open in the browser now when the browser extension is connected, marked
  _Open now_ and kept with their full address, then the sites CursorPie has seen. The sheet also has an **Add** button
  for the address that is typed.

- **New: add apps and websites by hand to an Intelligent slice.** The slice editor has _Added by you_ with _Add app..._
  and _Add site..._. What is added is always in the ring, before the suggestions, and is not repeated when it is
  suggested too. It is saved as `pinned` in the slice, which is left out of `config.json` when empty.

- **Fixed: `cargo publish` could not build the package of the app** (`unresolved import diag::problems`). `diag/` had
  gained a module while its version stayed `0.1.0`, the one crates.io already had, and `cargo publish` builds the app
  against the published crate. `cursorpie-diag` is now `0.2.0` and has to be published first. New `cargo tools diag-seal`
  records what was published, and `cargo tools check` fails when `diag/` changes under a published version.

## 0.9.2

- **New: `cargo tools ci` does not run a step again that already passed on the same files.** It made a SHA-256 of the
  content of the working tree (saved edits count, the website does not) and kept a stamp per step in
  `target/cargo-tools-ci/`, so a second run, the pre-push hook after a manual run, or a run after a `touch` or a
  commit of what is already there takes no time. `--force` runs everything; the stamps hold for one compiler and
  one OS and are deleted after two weeks. On GitHub Actions nothing is skipped by it.

- **New: CI no longer repeats work that already passed on the same files.** A pull request whose files are, for a
  group of jobs, the very ones that passed before (found by a SHA-256 of their content, from any branch or earlier
  run) skips that group: the tests when only the website changed, the packages when only tests, tools or the
  changelog changed. `main`, version tags and manual runs always run everything. The check to require in the branch
  protection is now `CI result`. The caches are written by `main` only, so pull requests no longer push them out,
  and the Fedora jobs keep the compiled dependencies and the npm downloads are cached too.

- **Fixed: _Install_ on the Flow Hub page froze CursorPie.** The question that asks you to confirm never appeared and
  the program stopped answering ("Not responding") until it was closed. Asking for the question read the settings
  while the same thread already held them, and a thread that waits for a lock it holds itself never goes on; every
  other thread that read the settings, the window first of all, waited behind it. The settings are now copied out in
  one short lock before the rule is applied.

- **Fixed: a failing test of the browser extension.** The test that checks the names of the commands did not know the
  `ui.*` commands (panels on the desktop) that the extension has had since they were added.

- **New: the tests of the browser extension (jsdom) are part of `cargo tools ci` and of the CI workflow.** They ran
  by hand only, so a failure could stay unseen. The step installs jsdom on its first run and needs Node.js 18 or
  later; the workflow sets Node 22 up on all three systems.

## 0.9.1

- **Fixed: _Learn now_ (and _Forget everything_) showed nothing.** The answer was one line in the status bar, under
  the floating page switch and gone after six seconds, and the notice pill is never shown while Settings is open. Now a
  sheet opens at the click, says that CursorPie is working (it reads the list of installed apps, which takes a moment)
  and then tells what was learned. If the sheet was closed meanwhile, the answer goes to the status bar as before.

- **Changed: the _Intelligent_ page is no longer the last one of the Behavior switch.** The switch now reads Input,
  Look, Apps, Intelligent, System: the far end is System (startup and memory), as the other settings pages end with
  the system ones.

- **Changed: Developer mode (About) is written for people, not only for developers.** The panels are _Developer
  tools_ (the switch and the three steps of a bug report), _Diagnostic build_, _What went wrong_ (each problem in a
  sentence that says whether it matters, with the log's own words small under it; counts as "3 times", not
  `x3 · 11764.8 s`), _Technical log_ (the raw lines, folded away until asked for) and _Terminal command (cpd)_.
  _What to record_ moved under _Advanced_.

- **New: `cpd` is installed and removed from Settings.** _Install cpd_ downloads the program of the newest release for
  this system, checks it against the release's checksum, makes sure it starts and puts it in the `bin` folder next to
  `config.json` (on macOS and Linux also a link `~/.local/bin/cpd`, where nothing is). _Copy command_ puts the command
  with its whole path on the clipboard, so it runs before anything is in `PATH`; _Remove cpd_ deletes it again and the
  link with it. Nothing else on the computer is touched (no `PATH`, no registry, no administrator). The release
  workflow now attaches `cpd-*.sha256` files. Developer mode shows the terminal panel in the standard build too.

- **Changed: the notice of an action now goes on from the pill of the slice you chose.** While the pie is open the
  pill shows the icon and the name of the slice under the pointer; after you release, the notice used to switch to a
  plain gray glyph and "Open app" / "Open link". Now the badge keeps the icon of the slice (the app icon, the site
  icon, the symbol you picked) and the title keeps its name, with a short line under it for what happens (`github.com`,
  `Safari`, `Cmd + C`), left out when it would only repeat the name. A slice that runs a flow keeps its icon and name
  for the whole run and the line under it follows the blocks. A slice without a name, the side buttons and the lock
  notices look as before.

- **Fixed: the pie sometimes was not drawn although its action ran, and the first pies after a login were not
  drawn until Settings had been opened and closed once.** The toolkit (eframe 0.36) runs only `App::logic` for a hidden
  window, and at most once per 100 ms, so `ShowPie` was carried out late (a quick flick ended first), and the first
  frame that really paints (shaders, font atlas, surface) was paid by the first pie, not before it. On Windows the
  window is now mapped, empty and parked off screen when the trigger is pressed (and once at startup to draw the first
  frames unseen), so the pie is shown in the next frame. On every system a pie that was asked for and has not painted
  two frames after 0.7 s is shown again (up to three times; `pie_revive` in the debug log), which is what opening and
  closing Settings did by hand.

- **Fixed: unlocking while the trigger is still held now opens the pie at once, and the lock animation keeps your
  pie.** Before, opening the lock (two more fingers on the trackpad, or Esc with the mouse trigger) only played an
  animation, and the trigger had to be released and held again to get the pie. Now the hold that unlocks is the hold of
  the pie: it blooms under your finger or pointer with the padlock opening in its middle, and you can choose a slice
  and release as usual. With the mouse trigger a click of the left button also unlocks (like Esc), for a hand that is
  not on the keyboard; the click does not reach the app, and the app gets the release of the trigger that it was
  missing. Locking no longer swaps the pie for a plain ring of bare slices: your own pie (slices, icons, glass or
  Gen Z look) squeezes, turns and fades into the padlock in one continuous movement, with smoother easing; the icons
  and slices no longer vanish and come back plain.

- **New: lock the pie with Esc, for the mouse, the keyboard and the trackpad.** With a pie open (a mouse hold or a
  trackpad hold), pressing Esc closes it without running anything and locks CursorPie: the pie turns into the padlock,
  and a notice says _CursorPie locked_. While it is locked a press of the mouse trigger opens no pie (the click goes to
  the app as if CursorPie were not there) and a trackpad hold opens none either; after the hold time the notice says
  why. To unlock, hold the trigger (mouse button or trackpad) and press Esc again: the padlock opens and the next hold
  opens the pie. Esc is taken only in those two moments and passes through to your apps at every other time, and with a
  modifier it is never taken. The lock is the one the three-finger tap of the trackpad already had, now shared: either
  way locks and unlocks it, and it stops the mouse trigger too. It is still kept in memory only (restarting CursorPie
  unlocks it). New setting _Lock with Esc_ (Behavior > Trigger, on by default; `esc_lock`). Like Space, Esc is
  taken on macOS and Windows; on Linux with X11 the lock works but the key also reaches the app, and on Wayland
  CursorPie does not see the keyboard, so Esc does not lock there (the three-finger tap still does).

- **Fixed (root cause): the pie still did not appear over an app in full screen (macOS).** The pie worked (its
  slices were picked and ran) but nothing was drawn, because the 0.9.0 fix was not enough on current macOS. The window
  of one app is shown in the full-screen Space of _another_ app only with `CanJoinAllApplications` (macOS 13+) next to
  `CanJoinAllSpaces` and `FullScreenAuxiliary`, and above the level that Space draws: the status level it was raised to
  is below it, the screen saver level is not. On top of that the toolkit wrote its own "always on top" (the floating
  level) every time the pie was shown, so the window was ordered in at the wrong level and only a frame later was
  raised. Now the window gets the full set of bits, the screen saver level, and on macOS CursorPie no longer sends the
  toolkit's level at all (`keep_over_full_screen` owns it). Settings and the welcome window keep their normal level.
  The real reason the 0.9.0 fix never worked: the window is observed with KVO, so its class is
  `NSKVONotifying_WinitWindow`, and the test `starts_with("Winit")` never matched. The window is now found by that name.
  New in the diagnostic log (macOS): `ui:mac_window_class` and `ui:mac_window_state`, see `docs/DEBUGGING.md`.

## 0.9.0

- **Fixed: the pie did not appear over an app in full screen (macOS).** An app in full screen lives in a Space of its
  own, and a window is drawn there only if it says so. The pie window (also the notices and panels) now joins every
  Space, may sit beside a full-screen window and is raised above the Dock while it is shown, so the pie opens over a
  full-screen browser, editor or game window instead of behind it on the desktop. Settings keeps its normal behavior
  (one Space, below the system's own panels).

- **Fixed: the pie in the editor drew the names under the icons.** The pie itself shows the name of a slice in the
  pill, so the Menu tab and the pie editor now draw icons only, like the real pie.

- **Fixed: the pill with the name of the slice no longer moves with the pie.** It sat at the bottom of the pie
  window, so it changed place with every place the pie was called. While _Show the slice name in a pill_ is on, the
  pie window is now as big as the display for as long as the pie is open (still one window, with the same
  click-through, always-on-top and focus handling), so the pill stays at the bottom center like a notice (_Notice
  position_ is followed). With the setting off nothing changes: the window is the small one. The big window is made
  small again as soon as the pie is gone, so no display-sized surface is kept in memory.

- **Changed: the name of a slice is shown in the pill, not under its icon.** _Show the slice name in a pill_
  (Behavior > Look & feel; it was _Show names under the icons_) no longer writes the name under the icon, which
  crowded it. The pie and the pill are now on screen together: the pill sits where the notices of actions appear (_Notice position_;
  kept inside the pie window when that is out of reach) and, with every slice the pointer reaches, swaps its text for the name of that slice and its
  badge for the icon of that slice, with the animation of the other pills (it comes in on the first slice, the badge
  bounces on each change, the pill glides to the new width and goes away when the pointer leaves the slices). Icons are
  centered and a little larger in every case. The pill follows the pie's look (smoked on Glass) and the size set for
  notices.

- **New: _Reinstall as administrator_ on Windows (Settings > About > Administrator mode).** An install for your user
  cannot send input to windows of programs that run as administrator (Task Manager, an elevated terminal), because
  Windows blocks it. The new button asks for UAC once, copies CursorPie to `C:\Program Files\CursorPie` (where only
  administrators can write, so nothing can swap what runs with full rights), adds a machine-wide _Apps & features_
  entry and a logon task that starts it with administrator rights at every sign-in, and removes the per-user startup
  entry and folder. _Launch at login_ now switches that task's trigger. A copy started without the rights hands over to
  the installed one instead of running limited next to it, a newer download offers to update the installed copy, and
  uninstalling asks for UAC and removes the task. If the elevated step fails, a message says why and CursorPie starts
  again as it was. The notice about a window that runs as administrator now points to this button.

- **Fixed: a copy without administrator rights could start next to one that has them.** The single-instance lock of
  an elevated copy cannot be opened by a normal one; that refusal was taken for "cannot tell", so both ran and every
  gesture fired twice. Access denied now counts as "already running".

- **Fixed and improved: the intelligent pie.**
    - _Kept until you switch it off._ Holding Space now keeps the intelligent pie: closing the pie no longer
      ends it, and the next presses open it again, until Space is held once more. (Not saved: a restart starts with
      the normal pie.)
    - _Sorted into submenus._ The intelligent pie is now grouped by default into **Apps**, **Sites**, **Actions** and
      **Flows**, each a submenu of suggestions. _Mixed_ is still there in Behavior > Intelligent.
    - _The right icons._ A suggested site shows its own logo (fetched in the background with the lookup of _Icon from
      a website_, kept as `icons/site-<host>.png`, deleted by _Forget everything_; switch off with _Show the icon of a
      site_), and the icons of suggested apps are loaded ahead like the ones of any other pie, so the first frame no
      longer shows placeholders.
    - _Intelligent slice, easy to add._ New **Intelligent slice** panel (add it to the general pie or to the pie of
      one app), the same slice under _Add a slice > Custom_, and switching the feature on adds one to the general pie.
    - _Settings._ _Forget everything_ was drawn over its description; the ignore lists now use the app list and a
      new site list (with icons) instead of a text field.

- **New: intelligent pie and intelligent slices.** Hold Space for about a second in an open pie and it turns into a pie
  of suggestions made from what you use: the apps you open most and the ones that are open now, the sites you visit
  most and the pages that are open (with the browser extension), what you run most from the pie in the app in front,
  and your flows. Hold Space again to come back. A quick press of Space still opens Settings, but on the key-up while
  the feature is on. A new **Smart** slice kind puts the same suggestions in a submenu of any pie. CursorPie learns
  from the app in front, the page in view and the actions you run, fades what you stop doing, and tidies and saves
  while you are idle (or at once with _Learn now_). It is **off until switched on** in Behavior > Intelligent, keeps
  only sites (never titles or contents) unless you allow paths, has ignore lists, and stores everything in one local
  file, `smart-habits.json`. See [Intelligent pie](docs/user/intelligent-pie.md).

- **Changed: the loading animation of the website icon sheet sits in the middle.** It was stuck under the address
  field with the rest of the sheet empty; it now takes the room that is left and centres tile, ring and text in it.

- **Fixed: _Round a square icon_ did nothing for claude.ai's icon.** The test for "a full square" wanted every pixel
  opaque, and the site's picture has a few that are not (its corners). A picture whose pixels are 97% or more opaque is
  now a tile and gets the rounded corners.

- **Fixed: a website icon that is a full square (claude.ai's orange tile) stayed a sharp square.** Such a picture, with
  nothing transparent in it, now gets rounded corners like the other icons and no plate around it (_Round a square icon_,
  on by default, with the Roundness slider). _Trim_ no longer cuts a plain background that was not removed, which
  had cropped a tile down to the logo inside it.

- **Fixed: a website icon on a black square came out as a cropped square.** _Remove plain background_ turned itself
  on for a white square only, so GitHub's icon (a white circle on black) kept its black corners, and trimming then cut
  the picture right at the circle. It now turns on for a white **or black** square, and removes only the background that
  reaches the border, so the black cat inside the circle stays.

- **Fixed: "Use website icon" gave nothing, and could blank the icon it had.** The old lookup read the home page
  whole and gave up on a page bigger than 512 KB (GitHub's is), so it never saw the icon links, and it deleted the
  site's earlier icon file _before_ looking, leaving the slice with a **?** when the lookup failed. The page is now read
  only as far as its head, nothing is deleted, and the **Use website icon** button opens the _Icon from a website_
  sheet, with its loading animation and the tools that make site icons look alike. The old `ui::favicon` module is gone.

- **New: icon from a website, with a loading animation and a tidy-up step.** In the icon picker, **+ > From a website…**
  asks for an address (a slice that opens a link starts with its site, and the fetching starts at once). While CursorPie
  looks for the icon, the sheet shows a breathing tile with a comet circling it, what is being done (_Reading
  github.com_, _Downloading the icon (2 of 5)_) and a Cancel button. It reads the home page for `apple-touch-icon` and
  `rel="icon"` links and tries the biggest first, then `/favicon.ico`; only if the site gives nothing usable does it ask
  Google's favicon service (the host name is all it is told). The picture is then shown as the site serves it and as it
  will look on the pie, with controls that make every site icon look alike: _Remove plain background_ (turned on by
  itself for a white square), _Trim the empty border_, _Colour_ (original, white or black), _Plate_ (none, light or
  dark rounded square) with _Roundness_, and _Margin_. The look is remembered in `web-icon-style.json`, so the next
  site starts with it. The result is a 256 px PNG in the icons folder (_Mine_). The request is made by the system's
  `curl` (or `wget`) like the update check; only `https` is used.

- **New: lock CursorPie with three fingers on the trackpad.** While the pie of a trackpad hold is open, tap with two more
  fingers together (three on the trackpad in all): the pie turns into a padlock that snaps shut, then a pill says
  _CursorPie locked_. While locked, a hold opens no pie and shows the same pill instead (after the hold time), so it is
  clear why. Hold with one finger and tap with two more to unlock: the padlock opens into the pie and the pill says
  _CursorPie unlocked_. The lock covers the trackpad hold only (the mouse is unaffected), is kept in memory (restarting
  unlocks), and the click some systems make of a two-finger tap is swallowed. _Lock with three fingers_ (Settings >
  Behavior, on by default; `touch_lock`) turns it off, and two more fingers then close the pie as before. The lock
  pills show even with the action notice (`hud`) switched off.

- **New: Flow Hub, a page of ready-made flows on the website, with one-tap install.** `flowhub/` of the website lists
  flows the author wrote (for now only the author adds flows; `flowhub/catalog.json` and `flowhub/flows/`). With the
  browser extension and CursorPie running, **Install** hands the flow to CursorPie over the existing browser link:
  the app reads it exactly like **Import**, shows a panel that names the flow and what it can do (shell commands,
  files, messages) and, only after you press **Install**, adds it as a new flow. Without the extension, or with
  CursorPie closed or the new setting off, the same button downloads the `.json` file. The extension gets one content
  script that runs on the Flow Hub page only. _Let Flow Hub install flows_ (Settings > Behavior, on by default, needs the
  browser link) turns it off. `scripts/flowhub-check.py` checks the catalog before a commit.
  The **New flow** dialog gets a wide _Flow Hub_ tile under the three choices: it opens the page and shows whether the
  extension is connected (so Install will work) or not (the page will download the file).

- **Changed: the theme choice slides, and a change of theme spreads over the window.** In _Behavior > Look & feel_ the
  themes are no longer tiles that wrap into a second row: they are larger cards in one row that fills the panel and
  slides sideways (drag it, use the trackpad or Shift + wheel, or the two arrows next to the title), with a soft edge
  where more cards wait and one dot for each theme under it. The chosen card glides to the middle and wears a check.
  Choosing a theme starts a wave: a disc in the colors of the new theme grows from the card until it fills the
  window, the theme is swapped under it, and a round hole opens from the same point to uncover the new look. The
  wave is clipped to the rounded corners of the window.
- **Changed: the Welcome guide has six pages and asks what you do most.** The guide is now _Welcome_, _How it works_
  (the gesture next to a small pie that plays it), _About you_, _Your pie_, _Setup_ and _All set_. On _About you_ a
  person picks one or several of Everyday, Student, Developer, Designer, Writer and Creator; _Your pie_ shows the pie
  that follows from it. With "Get started" or "Open Settings" the general pie takes those slices and a pie for the
  apps of the pick (Design, Writing, Editing) is added next to the profiles that exist; Skip and Esc change nothing.
  The card is larger (900 x 640, at least 720 x 540), the sticker shadows are no longer cut off at the right and
  bottom edges of a page, and the gaps between cards, rows and buttons are wider.
- **Changed: Gen Z is the default theme, and Bubblegum ships with the app.** A fresh configuration (and a file that does
  not name a theme) now starts in **Gen Z**, the look of the website, with its lime accent. **Sage** is still one click
  away in _Behavior > Look & feel > Theme_, and so is the new built-in **Bubblegum**: Gen Z in candy pink (cotton-candy
  page, plum outlines, rounder corners, pink accent, a berry-tinted dark appearance). A theme file named `bubblegum.json`
  (the old docs example) is now skipped with a note, since the name belongs to the built-in theme; the example of the docs
  is `mint-soda.json` now. A saved choice (`ui_theme`) and a saved accent are kept as they are.
- **Changed: the Welcome guide follows the Gen Z style of the website.** Ink outlines and hard shadows on the banner,
  the three gesture cards and the step badges, candy-colored stickers that bob around the logo, a sticker label for
  each step, pill page dots with an outline, and a card that lifts into color as the gesture is shown. It uses the
  colors of the theme in use (cream or violet-black, ink or cream outlines), so it reads in both appearances.

- **New: the demo GIFs are in the README, the guides and the website.** Fourteen 720 x 405 animations now live in
  `docs/gifs/` (see its `README.md` for what each shows and where it is used): the problem hook and the intro at the top
  of the README, the gesture and its cancel in the README, _Getting started_ and _Using the pie_, any action and nested
  submenus in _Using the pie_, per-app pies and side buttons in _Profiles and mouse buttons_, the trackpad, flows,
  Settings, themes and sharing, and the platforms in the page each belongs to, and a **See it in motion** section on the
  landing page that loads each one only when you scroll to it. The folder is excluded from the crate (`Cargo.toml`) and
  `*.gif` is binary in `.gitattributes`.

## 0.8.22

- **Windows: the pie does not open over a program that runs as administrator, and nothing said why.** Windows never
  calls a low-level hook for the windows of such a program (an elevated graphics editor, an installer, a terminal)
  unless CursorPie runs as administrator too. The watchdog now notices when the program in front is one of them and
  shows a notice, once per program and run, that names the program and says what to do.
- **Fixed: on macOS the pie stopped choosing slices after a left or right click.** While the pie is open a click is
  swallowed, and the moves made with that button down arrive as drags of the left or right button, not of the trigger.
  Those drags were only swallowed, so the hover was never updated and the slice under the pointer stayed as it was
  until the click was released. A drag of any other button now also aims the open pie of a mouse hold, exactly like a
  move with the trigger held; the app under the pie still hears nothing.

- **Fixed: on Windows "Open app" did nothing for an app that was not already running (Telegram and the like).** An app
  given by name was handed to `ShellExecuteW`, which only looks in App Paths and `PATH`; programs installed in the
  user's own folders are in neither. The name is now first looked up where the app list finds apps (App Paths, the
  Start menu, the program folders) and the program is started by its full path. Anything that is a path or a link, or
  that cannot be found, is opened as before.

- **Changed: Space is the way into Settings by default, the hold is off.** _Open settings with Space_ is now on and
  _Open settings by holding_ is off in a fresh configuration (and in a file that does not name them). Space opens
  Settings in any pie that is open: the top ring, a submenu at any depth and the pie of an app. The hold keeps behaving
  exactly as before when it is switched on (only the untouched top ring of the pie that a hold opened). A saved choice
  is kept as it is.

- **Fixed: on Windows the pie sometimes could not be aimed: the pointer moved and no slice was chosen.** The hover only
  depended on every mouse move reaching the low-level hook, which Windows drops when the hook is late and removes
  when a callback times out (the watchdog installs it again only after a few seconds). While the pie of a mouse press
  is open the pointer is now also read every 16 ms and aims the pie like a move of the hook does, so a missed move
  never leaves the pie dead.

- **New: themes.** _Behavior > Look & feel > Theme_ now picks the look of Settings and of the Solid pie. **Sage** is the
  look it always had (and the default, so nothing changes until you choose); **Gen Z** is the look of the website: cream or
  violet-black, 2-point ink outlines, pill controls, hard shadows and a lime accent. Choosing a theme moves the accent to
  its own unless you picked one. Your own colors (_Colors of the light/dark appearance_) are laid over any theme.
    - **Make your own:** a theme is a small `.json` file in the new `themes/` folder (button _Open themes folder_) that
      extends Sage, Gen Z or another file and lists only what it changes (colors, radii, outlines, shadows). The text is kept
      readable whatever it says, a bad file is skipped with the reason shown, and _Reload themes_ picks up edits without a
      restart. See [Themes](docs/user/themes.md) and `docs/examples/themes/mint-soda.json`.
    - **For developers:** a theme is data (`ui/settings/theme/`: `Theme`, `Shape`, palettes), not drawing code. Widgets
      read colors from `pal()` and radii, outlines and shadows from `shape()`, so a new look needs no copy of any widget.
      `CONTROL_R`, `PANEL_R` and `CARD_RADIUS` (and their `_PT` forms) became the functions `control_r()`, `panel_r()` and
      `card_radius()` (`_pt()` for the number); `PAGES`, `PANELS`, `SIDEBARS`, `TEXTS` and `ACCENTS` became
      `theme::swatches()`; `Palette` has a new `line` color.

## 0.8.21

- **Fixed: the Solid pie could not be chosen in Behavior, Pie material.** The pies of _Pie material_ and the one of
  _Pie shape_ are drawn by the same demo, and each tile took its id from the id of the panel it was in, which two panels
  of one page can share: the tile of _Pie shape_ took the clicks of the first tile of _Pie material_. Glass and Gen Z
  worked, Solid did not. Every demo now gives its tiles ids of its own.

- **New: bring a whole pie in without dragging slice by slice (import tray).** The tray that opens beside your pie
  when you import a profile file used to take one slice at a time (a drag or a double click), or the whole file as a
  new pie, which failed for a file that lists no apps. Now:
    - **Select mode** (cursor button of the bar; Shift or Ctrl/Cmd + click does the same without it): a click picks or
      unpicks a slice and nothing is dragged, so a click can never move one by accident. Clicking the middle of a pie
      picks every slice of that ring, **Pick all** (or Ctrl/Cmd + A) picks the whole top ring, and a picked submenu
      goes with everything in it. **Add selected** (the arrow) then adds everything picked at once.
    - **Add every slice**, **As a submenu** and **Use as this pie** (new buttons of the tray bar) bring the file in with
      one press: to the end of the lit ring, as one submenu of it (it fits whenever the ring has one free place), or as
      the slices of the pie you are editing (its name and apps stay; Undo brings the old slices back).
    - A group is added whole or not at all: when it does not fit (ring of 8, 5 rings deep) nothing is changed, no
      image is written, and the notice says how many slices still fit. Images and flows come along, and slices that
      can run commands are reported, as for a single slice.
    - A profile file dropped on the window (pie editor, slice editor closed) opens in the tray, and so does a profile
      file copied as text and pasted with Ctrl/Cmd + V in the pie editor.
    - Logic lives in `config::pie_edit` (`insert_many`, `group_item`, `replace_items`, `top_paths`, `ring_paths`,
      `first_mode_path`) and is unit tested.

- **Added: Settings can be opened with Space while the pie is open, and the hold can be switched off (Behavior, Trigger).**
  _Open settings by holding_ (on by default, as before) decides whether keeping the trigger in the center opens Settings;
  off, the pie never turns into Settings by itself and the progress arc is not drawn. _Open settings with Space_ (off by
  default) opens Settings when Space is pressed while a pie is open, after one or two presses (_Presses of Space_; two
  must come within half a second). Space is swallowed, with its repeat and release, so it never reaches the app under the
  pie. The two work independently: whichever comes first opens Settings. Space works on macOS and Windows; on Linux X11 the
  key also reaches the app, and on Wayland CursorPie sees no keys. New settings `settings_by_hold`, `settings_by_space` and
  `settings_space_presses`; a file without them keeps the old behavior.

- **Changed: the Gen Z pie style is the default of a fresh install.** A saved `pie_style` is kept; a file that has none
  (written before the setting existed) now loads as `"GenZ"`.

- **Changed: a trackpad that is found switches the trackpad hold on, at any start.** It used to happen only at the very
  first start of a new user. Now the first start that finds a trackpad switches `touch_trigger` on once and sets the new
  `touch_auto_done`; turning the switch off (or on) in Settings, or pressing _Scan for trackpad_, sets it too, so a choice
  made in Settings is never undone. Someone who had turned the hold off before this version sees it on once more.

- **Fixed: on Windows the Windows key could not be recorded in a shortcut; it opened the Start menu instead.** While a
  shortcut is being recorded, the Windows key (left or right) is now taken for the recorder and never reaches Windows,
  so Win+S, Win+Shift+S, Win+L and the rest record like any other combination and no Start menu or shell shortcut
  runs. Its repeats and its release are swallowed as well, also when the recording ends before the key is let go.
  Outside a recording the key works as always.

- **Fixed: on Windows the default "Lock" slice (Win+L) did nothing.** Windows handles Win+L itself and ignores it
  when it is sent with `SendInput`, which is how every other shortcut slice works (Win+S, Win+Tab and the rest were
  never affected). A shortcut that is exactly Win+L, the default slice, one saved in `config.json` or one inside a
  flow, now locks the PC through `LockWorkStation`. Nothing has to be changed in an existing configuration.

- **Added: a Gen Z pie style (Behavior, Look & feel, Pie material).** Besides _Solid_ and _Glass_, the pie can be drawn
  like the one on the website: white slices with thick dark outlines, hard offset shadows that grow under the slice
  the pointer is on, and the accent as its fill. It looks the same in the light and the dark appearance, and the
  center disc and the badges of an "Open app" slice follow it. The setting is `pie_style: "GenZ"`; a file without it
  keeps the style it had.

- **Added: the pie can be given another shape (Behavior, Look & feel, Pie shape).** _Custom_ turns the circle into a
  regular polygon with rounded corners: how many sides (3 to 12), how round the corners are (from sharp to a circle)
  and how far it is turned. All three materials follow it, and so does the point where a drag over a submenu slice
  opens the submenu. The default is still the circle, and a file without the new settings keeps it.

- **Added: demos of the pie materials.** The Pie material panel shows Solid, Glass and Gen Z as three small pies that
  open and are passed over by the pointer, over a colorful backdrop; a click on one chooses it (it replaces the static
  strip that only Glass had).

- **New: Developer mode, a build with the diagnostic log that a person can install from About, and `cpd` for the
  terminal.** At the bottom of About, **Developer mode** offers to replace the running copy with the log build of the
  same release (`CursorPie-devlog-*`, built for every version tag; same download, checksum and swap as an update). With
  it running, About lists what the log found in plain words (errors first, repeats counted, a pie that was asked for
  and never drawn is named), shows the last events by category, switches the log and what it records, copies a
  report for an issue, opens the logs folder and goes back to the ordinary build. The command `cpd` (attached to every
  release for macOS, Windows and Linux) reads the same logs in a terminal and names the same problems: `cpd`,
  `cpd d`, `cpd s|t|g|sl|df|f|r`, `cpd on|off|path|clean`. The ordinary release is unchanged and has none of the log
  code. See `docs/DEBUGGING.md`, "Developer mode and `cpd`".

    Turning Developer mode **off** in the log build asks first, because the way back to the ordinary build is among the
    tools that go away with the mode: **Install standard build** replaces this copy with the ordinary one of the same
    version and switches developer mode and the log off once that swap has started (a download or a check that fails
    leaves everything as it was); **Keep log build** only hides the tools and stops the log at the next start; **Cancel**
    changes nothing.

- **Fixed: on Windows with a mouse, the pie did not come up after a panel of a flow had been closed, and the held
  trigger opened Settings instead.** A panel makes the shared window take the pointer, and nothing gave that back
  when the panel ended (Settings does it in `close_settings`). The pie of a mouse hold is click-through and flipped
  that state on the hidden window, after which the window never showed. A trackpad pie takes the pointer itself, so
  it never flipped anything and was not affected. Closing a panel (or the pie taking its window) now makes the
  window click-through again, while it is still mapped.

## 0.8.20

- **Changed: code that existed twice or more now exists once (no change in behavior).** The sheets of Settings
  share their heading, paragraph and bullet list (`sheet_heading`, `sheet_text`, `sheet_bullets`); the color swatch
  and the "any color" button are one function for Settings and the panel designer; the sliding highlight of the icon
  switch and the navigation is one `slide`; opening Settings and the welcome window share `take_window_for_card`, and
  the notice and a panel share `show_overlay`; the `curl` and `wget` arguments of a request and of a download share
  `base_args`; the first-frame and warm-up icon lists share `WarmList`; the process helpers of Linux and macOS (two
  identical files) are `platform/unix_process.rs`; the test helper that fakes the environment is `linux::env_of`.
  The leftover `src/diag.rs` (the log moved to the `diag/` crate and the file was no longer part of the build) is
  removed.

- **Fixed: an update gave no sign of life and CursorPie just vanished for minutes (Windows, macOS, Linux).** After
  _Install update_ the window disappeared while the new version was swapped in, with nothing to say whether it was
  still working or finished. Now a progress sheet stays over Settings while the update downloads and says what
  follows (CursorPie closes by itself, the new version is put in place and starts again, which can take a few
  minutes). On Windows, Setup now runs with its small progress window (`/SILENT` instead of `/VERYSILENT`), so the
  installation is visible. The first start after the update opens Settings on the About tab with a sheet that says
  "CursorPie is now X", or, when the swap did not happen (for example a cancelled password prompt), that the old
  version is running again and what to do.

## 0.8.19

- **Changed: the website is now a one-page scroll with a new look, and the documentation viewer is much better.** The
  landing page is a single scroll (hero with the live pie demo, a ticker of example actions, key numbers, a scroll
  driven "how it works", a feature grid, privacy, downloads, FAQ) with a neon, thick-outline style, light and dark
  themes, and animations that stop for `prefers-reduced-motion`. The documentation view gets a grouped, searchable
  sidebar (press `/`), an "On this page" outline that follows the scroll, breadcrumbs, reading time, previous and next
  links, Copy buttons on code blocks, callouts, and a drawer on phones; internal links no longer reload the page. The
  privacy text now says what is true: no account and no telemetry, and the only network request is the optional daily
  update check described in `docs/user/updates.md`.

- **Fixed: "Open app" opened a newly started app on the main display, not on the display of the pie (Windows, macOS,
  Linux).** With two displays and the pie opened on the second one, a slice that starts an app (that was not running
  yet) showed the window on the first display, because the system picks the display of a new window by itself and
  CursorPie only started the program. Right before it starts an app, CursorPie now notes the display the pointer is
  on and the windows that exist; a new main window of the app that appears within a few seconds and whose center is on
  another display is moved to the middle of the pie's display. Windows: the window is moved and scaled for the DPI of
  the two displays (and maximized again if it was maximized). macOS: the first window is moved through Accessibility
  (an app that already has a window is still dealt with by "Open app" as before). Linux: X11 through the window
  manager (`_NET_MOVERESIZE_WINDOW`); Wayland through the KWin script and the GNOME Shell extension (the extension
  has to be updated: it understands a new `move:` command; Sway and Hyprland tile their windows and are not
  changed). Windows that already existed, minimized and full-screen windows and windows that are already on the right
  display are left alone, and nothing is done with one display.

- **Added: the flow kit now teaches an AI agent how to build a panel.** Until now the kit had the format of a flow and
  one line about panels, so an agent that was asked for a panel guessed: too many fields, every button primary, panel
  JSON pasted into the flow without escaping, a remote control with no loop. The kit now holds
  `PANEL_DESIGN_FOR_AI.md` (when a panel is the right tool, the design rules with the real sizes and limits, four
  models: form, menu, tool panel and progress panel, how the panel is put in a flow file and wired, a table of the
  common mistakes, a checklist and a script that checks a panel), `PANELS.md` (the reference) and five panel examples
  (the new `panel-settings-form` is a menu followed by a form with sections and `show_if`). `START_HERE.md` and
  `FLOWS_FOR_AI.md` send the agent there whenever the flow shows a panel. A test loads every panel of the kit and of
  the guide with the same checks the app uses, so what an agent is shown as good can never be refused.

- **Fixed (macOS): the pointer disappeared while the pie of a mouse hold was open, and the first fix did not reach
  the cause.** A terminal, an editor, a browser or a video player hides the pointer (while you type, or when the
  mouse is idle) and shows it again only when it hears the mouse move. While the pie is open the tap swallows every
  drag of the held button at the HID level, so neither the window server nor any app heard a single move for the
  whole hold: the pie followed the hand and the pointer stayed hidden. The earlier fix posted one mouse move with no
  distance when the pie opened, which says that nothing moved (and could not help once the app hid the pointer
  again). Now (1) the swallowed drags are played back as plain mouse moves, no button and at most ten a second, so an
  app below the pie hears the mouse move as it would with the button up while the drag itself stays swallowed, and
  (2) a new pie posts a real move of one point and the move back (the pointer ends where it was) instead of a move
  with no distance. A trackpad hold is not affected: its moves were never swallowed. Windows is not affected either
  (its hook never swallows moves); Wayland was fixed in 0.8.14 by taking the pointer; on X11 the passive grab of the
  held button has the same shape as the macOS tap (see `docs/user/known-issues.md`).

- **Changed: the slice editor of the pie editor follows the design of the rest of CursorPie.** It was a small card at the
  top of an empty column, and its "Pick..." button was cut off at the edge when a slice opens an app. It is now a
  panel as tall as the canvas, on paper with one hairline and the same inside margin as the inspector of the Flow tab,
  with the slice at the top (icon, name, kind), what it does under a small title ("Action", "Submenu", "Back button"...),
  and a footer that stays at the bottom: where the slice stands in its ring ("Slice 2 of 6") and the two arrows that
  move it. With nothing selected the "Pick a slice" message is in the middle of the panel. In a narrow panel the kind of
  action has a line of its own and the fields a line under it, so nothing is cut off. The slice editor of the classic
  Menu tab keeps its card, with the same footer and the same narrow layout.

- **Fixed: in the pie editor, the groups of the list of slices did not take turns like the categories of the Flow tab.**
  The three round cells after the **+** (Quick actions, Side buttons, Custom) filled the one that was clicked at once, so
  the accent jumped from one to the other. They now have what the Flow bar has: a circle in the accent color under the
  chosen group that travels to the next one, stretching on the way, with the icons changing ink as it passes (the same
  0.28 s). The first choice appears in place, and a second click on the chosen group fades the circle out.

- **Changed: the web mode is now the default way a submenu opens.** The pie you came from shrinks to a node and the
  nodes keep the way back, as before this was a choice (`submenu_trail` is `true` by default). The classic mode, where a
  submenu replaces the pie, stays as the "off" setting of "Submenus keep a trail (web mode)". A configuration that was
  already saved keeps what it says; only a file that does not mention it (or a new install) gets the web mode.

- **Added: the submenu trail (web mode) has its own settings and demo.** A new panel in Settings, Behavior, shows the
  mode as a looping animation (a pie opens a submenu, the pie it left shrinks to a node, the nodes line up on an arc, the
  pointer returns to one) that follows the settings as they move: the distance of the nodes (`trail_reach`), their size
  (`trail_node`), how much smaller each older one is (`trail_shrink`), the spacing between them (`trail_fan`), whether the
  thread is drawn (`trail_thread`) and how fast the nodes glide (`trail_glide`), with a button for the defaults. The
  numbers are kept in range, and the shape is always usable: the nodes clear the pie, and big nodes never touch because
  the spacing is widened. The pie window has the room the chosen distance needs.

- **Fixed: entering a submenu, or coming back to the pie above it, stuttered.** Four things added up. (1) The pie was
  repainted without end whenever nothing was hovered at the top ring, long after the "hold for Settings" ring it was
  meant for had finished; every frame rebuilt the whole ring (glass is a mesh per slice), and that load fell exactly on the
  moment the pointer came back from a submenu. It now repaints on its own only while that ring is growing. (2) A change of
  level and the bloom of the ring were not kept apart: the bloom of a submenu (and of the ring that comes back when one is
  left) now always runs from the start, at any level and with or without the web mode, like the pie that opens the gesture,
  and it is smooth because nothing else happens in that frame (see 3 and 4). (The nodes of the web mode glide to their
  places.) (3) Every change of level said it all again to a window that was
  already on screen (size, decorations, always-on-top, click-through, and the move twice), which made the window manager
  reconfigure a mapped window and the drawing surface with it. A window that is already the right size now only gets its
  move, once. (4) The root of it: a submenu was shown by moving the pie window to be centered on the pointer, and a window
  move is carried out by the window manager a few frames after it is asked for, so for those frames the new ring was drawn
  for one window position while the window was still at the other: the pie was pulled aside, cut off or gone, and then
  jumped. The window is now large enough (the pie, the room of the trail, and 2.5 radii on every side, never more than the
  screen) for the ring of the next level to be drawn where the pointer is, away from the center of the window, and it stays
  where it is: a change of level is one ordinary frame, with nothing to move or resize. Only a ring that would leave the
  window (several levels in one direction) moves it, as before.

- **New: a "web" mode for submenus, with a way back.** Until now a submenu replaced the pie and the only way out was to
  release. With the new _Submenus keep a trail_ switch (Behavior tab, `submenu_trail` in `config.json`, off by default) the
  pie you came from shrinks to a small node showing the center of that pie, on a thread to the submenu. Every level you
  left stays as a node on an arc around it, in the order they were opened (the nearest is the last one, each older one
  smaller and paler, joined by a thread whose arrows point toward where you are), and moving the pointer onto one
  returns straight to that pie. The submenu opens centered on the pointer, and the nodes glide to their places. The
  window has a little more room around the pie for the nodes. Off, nothing changes.

- **Fixed: the middle of the pie slid away from the pointer near the edge of the screen.** A window manager keeps a window
  on the screen, so near an edge or a corner it put the pie window somewhere else, while the ring was still drawn at the
  center of the window and the slices were still measured from the pointer. The ring (and, in the web mode, its nodes) is
  now drawn on the point the pointer was on, wherever the window really is. The web mode made it easier to see: its window
  is larger, so the zone along the edges where it is pushed in is wider.

- **Fixed: the web mode did not follow the rules for giving memory back.** The icon of a node (a letter, an emoji or a glyph
  of the icon font) was laid out at a size that changed on every frame while the node glided to its place, and egui keeps
  every glyph size it ever cut. It is now cut at one of a few sizes (8 per doubling), the font atlas is rebuilt when the
  program sleeps after the web mode was used, and the trail of the last gesture is dropped when the pie has gone away and
  when the program sleeps. The test of the extra room of the window had bounds that the constants no longer met; it now
  states the real figures and checks that the window holds the farthest node and no more.

## 0.8.18

- **Changed: the pie editor selects and moves pies, and adds slices from a floating list.** The left button no longer
  moves the canvas: that is the job of the middle button (the trigger button), Space + drag, or the scroll, as in the Flow
  tab. A drag on the empty canvas now draws a selection box (Shift or Ctrl/Cmd adds to the selection, Ctrl/Cmd + A selects
  every pie), a click on the middle of a pie selects it (Shift or Ctrl/Cmd + click adds or removes it), and dragging the
  middle of a pie moves it and the other selected pies. The top pie stays where it is; the others are placed around it.
  Where a pie was put is saved with its slice (`at` in `config.json`, left out for a pie that was never moved), and the new
  _Tidy up_ button of the bar puts every pie back. The **+** of the bar no longer opens the "Add a
  slice" sheet: the three groups unfold in the bar and a searchable list opens over it, and the right button opens the same
  list where it is pressed, like the Flow canvas. Ctrl/Cmd + C, X and V copy, cut and paste the selected slice or pies
  (with everything in them) through the system clipboard, like the blocks of a flow, so a copy can go to another pie or
  profile. A selected pie sits on a tint of the accent and every pie has a soft
  shadow. The classic Menu tab still uses the sheet.

- **New: buttons and fields of a panel can keep their state in the variables of the flow.** A toggle button had a look
  (play or pause, muted or not) that only lived inside the window, so a flow had to keep one copy of the panel for every
  state (a remote needed four panels for play and mute) and the look was lost when the panel went away. Now a toggle can
  say `"bind": "keep_playing"`: it takes its look from that variable when the panel is shown and every answer writes
  `1` or `0` back, so one panel description serves every state, and with a `keep_` name the state survives closing
  CursorPie. A panel can say `"restore": true`: every input, number, switch, slider, color and list then starts with the
  variable named by its `id` (what it always wrote), and toggles without `bind` use their `id`. A bound toggle no longer
  takes the look of the panel it replaces, the variable wins. Also fixed: `on_label` of a button of the row at the
  bottom did not fill in `{{variables}}`, and the flow checks know the variables that toggles write. Nothing changes for
  a panel that uses neither setting. The panel designer has both: _Remember values_ under _Behavior_ of the panel, and _Remember in variable_ (card _Kept by the flow_) for a button that changes when pressed. New example flow `browser-remote-panel-state`. See _State kept by the flow_ in
  `PANELS.md`.
- **New: a flow can remember things between runs, also after CursorPie was closed.** Until now every run began with no
  variables and ended by forgetting them. Now a variable whose name starts with `keep_` is saved when the run ends and is
  back in the next run of that flow (a run that was stopped or cancelled is saved too). Every run also starts with
  `run_count`, `last_run` and `last_ending`, so a flow can tell its first run from the others and see how the last one
  ended. Three new _System_ blocks, _Save data_, _Load data_ and _Remove data_, keep names with a text that every flow
  shares, so one flow can leave something for another. Loading a name nobody saved is not reported as a problem: the
  variable is empty and `ok` is 0. All of it is plain JSON in the new folder `flow-state` of the support directory
  (`<flow id>.state.json` per flow and `data.json` for the shared data), written through a temporary file so a crash
  leaves no half file; a file that cannot be read is moved aside (`data.corrupt.json`) instead of overwritten. Existing
  flows are not changed: they have no `keep_` variables and the new variables only add names. See _Remembering things
  between runs_ in `FLOWS_FOR_AI.md`.
- **Changed: the inspector of the panel in the panel designer is calmer and follows the Settings look.** The long
  column of small labels is now four cards (_Header_, _Look_, _Size and place_, _Behavior_) of rows with the name on the
  left and one control on the right, so each line is one thing to decide. The look of the title, the subtitle and the
  field labels, and the space inside the card, are one step in: a row with a summary ("Bold, 22 pt") opens a page with a
  back arrow, instead of the three folding headers. There the letters are four toggles on one line, the size shows its
  number and a _Reset_, the colors are round swatches that include "the usual" (so one click goes back to it), and the
  nudge is two number boxes (X and Y) with one note. The inspector is 24 points wider. The same format controls are
  used for a selected text, so they look the same there. Nothing in the panel's JSON changes.
- **New: every panel has an X at the top right, and the buttons at the bottom are optional.** The X closes a panel like
  Esc does (it presses the button that cancels, or answers as cancelled). So a panel no longer needs a _Cancel_ or
  _Close_ button of its own: `"no_buttons": true` leaves out the row at the bottom, together with its room and gap
  (a panel that says nothing about its buttons still gets Cancel and OK, so existing flows work as before), and
  `"hide_close": true` leaves out the X. In the designer, _No buttons_ in the panel's inspector does the same, taking
  away the last button now leaves none (instead of bringing Cancel and OK back), and _Close button_ is a switch under
  _Behavior_. The _Button pad_ template uses the X instead of a _Close_ button.
- **Changed: the inspectors of a button and of every widget follow the same look.** What a button or a widget has is
  now in cards too (_Button_, _After a press_, _Size and shape_; _Input_, _Size of the box_, _Options_, _Visibility_, ...)
  with a name above each box instead of one long column of labels. Corners are a list instead of five squeezed choices,
  the color of a color widget is a row of round swatches, and the look of the text inside an input box is a page one
  step in (like the title of the panel) instead of a folding header.

- **Added: text format for the text inside an `input` box (a note box, a display), and a way to place any text by
  hand.** An `input` takes `value_fmt`, with the same parts as the other formats (`align`, `bold`, `italic`,
  `underline`, `strike`, `size`, `color`) and `valign` (`top`, `middle`, `bottom`: where the text sits in the height
  of the box, so the display of a calculator can sit in the middle). `title_fmt`, `subtitle_fmt` and `value_fmt` also take
  `dx` and `dy`, a nudge in points (to the right and down; negative goes left and up, at most 60 either way) applied
  after `align`. The title and the subtitle share the header with the round badge, so their _Center_ was a little to
  the right of the middle of the card: the designer has a _Center on the whole card_ button (`center` with `dx` -23).
  A bigger text makes its box taller. The designer shows _Look of the text inside the box_ for a selected input and
  _Nudge_ under every look of the header. A password box shows dots, so slant and lines do not apply to it. Panels
  written before look the same. See `docs/PANELS.md`.
  An `input` box also takes `width`, `height` and `text_padding` (the space between its edge and its text, per side),
  under _Size of the box_ in the designer.

- **Added: text format in panels and in the panel designer.** A `text` widget takes `fmt`, and the panel takes
  `title_fmt`, `subtitle_fmt` and `label_fmt` (the labels above the fields), each with `align` (`left`, `center`,
  `right`), `bold`, `italic`, `underline`, `strike`, `size` (8 to 48 points) and `color` (`#rrggbb`). Everything is
  optional and what is left out keeps the look it had, so a panel written before still looks the same and a panel
  without formats writes none. The designer shows the controls for a selected text and, in the panel's own fields,
  _Title look_, _Subtitle look_ and _Look of the labels above the fields_. The height of a text widget and of the header
  follows the size. See `docs/PANELS.md`.

- **Added: copy and paste in the Flow canvas and in the panel designer, and several widgets selected at once in the
  designer.** On the canvas, Ctrl/Cmd + C copies the selected blocks, Ctrl/Cmd + X cuts them and Ctrl/Cmd + V pastes
  them where the pointer is (a little down and to the right of the originals when it is off the canvas). The wires
  between the copied blocks stay, with ids of their own; _Start_ and _End_ are never copied, and a group that does not fit
  in the flow is not pasted at all. In the panel designer the widgets of the list or of the panel can be picked together
  (Ctrl/Cmd + click adds or removes one, Shift + click picks a range, Ctrl/Cmd + A picks all) and then removed,
  duplicated, moved up or down, cut, copied and pasted as a group; a pasted widget gets an id of its own and its
  `show_if` follows the widget it looked at, or goes when the panel has no such widget. Both copy through the system
  clipboard as text, so blocks go to another flow and widgets to another panel. Everything is undoable.

- **Fixed: a panel that stays open (`keep_open`) no longer loses the buttons pressed while the flow is working.** A
  panel that waits for the flow to ask again used to ignore every press, so a quick run of clicks on a calculator or a
  remote lost most of them. Now up to 8 presses are remembered and answered one by one, in order, as the flow asks
  again with the same panel `id`; a press that no longer means anything (the button is gone, or it cancels) is dropped.

- **Fixed: the progress panel of a run opened over the question the run was asking.** A flow that asks again and again
  (a calculator in a panel) counted the time the person spent in the questions as work, so after two seconds the
  progress panel ("Step 30/68") came up and competed with the question for the window. Time spent answering an _Ask_
  (text, yes/no or panel) no longer counts towards the two seconds, and the progress panel does not open while a
  question is open.

## 0.8.17

- **Added: the colors of Settings can be chosen, not only the accent.** _Look & feel_ in _Behavior_ has _Page
  background_, _Panels_, _Sidebar_ and _Text_, each with a row of suggested sage-tinted colors, like the accent, and a
  color picker for any other. The first suggestion is the color the appearance comes with and puts it back. The light and
  the dark appearance have colors of their own, so changing one never changes the other; the rows are those of the
  appearance in use. What depends on a color (the controls, the hairlines, the secondary text) follows it, and the text
  is always moved just far enough to read on every surface. The choice is previewed before it is saved and is kept in
  `config.json` as `"theme": { "dark": { "page": [r, g, b] } }`; a color left out keeps the one of the appearance.

- **Added: the space around the content of a panel can be set, on all four sides, two, or just one.** The _Panel_
  inspector of the designer has _Space between the edge and the content_: _All_ is one number, _H and V_ is two (left and
  right, top and bottom) and _Each side_ is four, so a single side can be changed (`"padding": { "bottom": 6 }` in the
  description; a side left out keeps the usual 18 points, the most is 60). _Usual space_ puts it back. The line under
  the buttons that is kept for a notice ("Fill in ... first") is now only there when the panel has a required box, so a
  panel without one no longer has a gap under its buttons on top of the padding.

- **Fixed: the panel designer needed a scroll to remove an element, and widgets could not be dragged.** Removing a
  button of a grid meant scrolling the inspector to its bottom. Now _Delete_ or _Backspace_ removes what is selected
  (unless a field is being typed in or a list is open), like on the canvas of a flow: the widget, the button at the
  bottom, or the button of a grid that was clicked on its own. The bar at the bottom of the preview has an _Add
  element_ button and a trash: a click on the trash removes the selection, and a row of the list (or the widget being
  dragged by its grip) is removed when it is let go over the trash. The grip beside the selected widget stays inside the
  preview however far the panel is zoomed or scrolled, so it can always be reached.

- **Added: two more ways to tidy the Flow canvas, from a list on the tidy button.** _Across_ is the layout there was
  (columns, left to right). _Down_ puts a run of blocks one under the other; a block that decides (_If / else_, a loop)
  has the blocks of its first output (_then_, the body) beside it, to the right, and what its second output leads to
  (_else_, what follows) goes on below, so a ladder of _If / else_ is one column with its actions in the next. _Mixed_
  does the same for the blocks that decide, but a plain run of blocks goes left to right. No block covers another;
  _End_ goes after the last column; blocks no wire reaches go below everything. The ways are three round cells (a
  glyph each, the name is the tooltip) that unfold from the tidy button and widen the capsule, as the categories do
  after the "+", and fold back after a click.

- **Fixed: wires on the Flow canvas showed as corners when zoomed in.** A wire is a curve drawn as short straight
  pieces, and there were always 24 of them, however long the wire was or how far the canvas was zoomed in, so at a high
  zoom (or on a long wire) the pieces were long enough to see as a jagged line. The wire is now cut so that each piece
  is a few pixels on the screen (`wire_points_at`). A wire that goes backwards (the input is left of the output) also
  leaves and arrives with a shorter reach, so it makes a tight S instead of a loop as wide as the gap between the dots.

- **Fixed: multi-select on the Flow canvas.** Shift (or Ctrl/Cmd) + click on a block that was not selected used to
  replace the selection with it (the press in the body selected it before the click was read, and the click then took
  it out again), so blocks could not be added one by one. Now Shift/Ctrl/Cmd + click adds a block to the selection or
  takes it out, and the group stays a group even when it is down to one block, so the inspector does not open and
  shrink the canvas while more blocks are picked. Shift/Ctrl/Cmd + click on the empty canvas keeps the selection (it
  used to clear it), Shift/Ctrl/Cmd + drag of a block that is not selected adds it and moves the whole group, and a
  selection box drawn with Shift/Ctrl/Cmd held toggles the blocks it touches (adds them, or removes the ones that were
  selected), like the box of a file manager. The hint line under the canvas mentions Shift + click.

- **Fixed: a click on the empty Flow canvas did not let go of the selection.** The press on the empty canvas is now
  also followed by where the blocks are, not only by what egui reports under the pointer, so a click on the empty
  canvas clears the selection (and closes the inspector) and a drag from it draws the selection box, even when the
  background does not receive the press. Only a block, its dots and the "+" of an output with nothing plugged in count
  as not empty: the room around a block is empty canvas.

- **Fixed: unclear message when an update fails on a full disk.** When the download or the installation of an update
  fails and the disk has no room, the About tab now says "There is not enough free disk space to install the update.
  Free some space and try again." Before, it said the package "could not be opened" or that GitHub could not be
  reached, because `curl`, `ditto` and `tar` only report that they failed. The disks are now asked, after a failure,
  whether they have room (a file of the size the update needs is written next to the installed copy and in the updates
  folder, and removed again); a folder that cannot be written for another reason is not called a full disk. Same on
  macOS, Windows and Linux; the error codes of each OS live in the platform backends (`platform::is_disk_full`).

- **Added: log lines for a failed macOS update.** `ui:unpack_failed` names the step that made "The downloaded package
  could not be opened" (`mount_dir`, `attach`, `no_program` or `copy`) with the exit code of `hdiutil` or `ditto`, and
  `ui:no_space` records the full-disk case. Both are in the table of `docs/DEBUGGING.md`.

## 0.8.16

- **Platform boundary enforced.** OS conditions (`cfg(target_os)`, `cfg(windows)`, `cfg(unix)`, `cfg!`) and native
  API calls (`std::os::*`, Win32, Cocoa, X11, `windows_subsystem` aside) now live only in `src/platform/<os>/**`,
  `src/tap/{macos,windows,linux,wayland}.rs`, and the wiring in `src/platform/mod.rs` and `src/tap/mod.rs`; every other
  layer uses `platform::OS`/`Os` or a platform function. Moved behind the backends: tray icon (`platform::build_tray`),
  the X11 window choice (`platform::event_loop_builder_hook`), process/file helpers (`hide_console`, `shell_line`,
  `terminate_tree`, `detach_from_parent`, `make_executable`, `create_private_file`), the installed-apps scan
  (`collect_installed_apps`), browser raising, and the touchpad sources (`evdev`, `multitouch`, `precision`, `ptp`,
  now under `platform/{linux,macos,windows}`; started with `platform::start_touch_source`). Test and feature gates stay.

- **Fixed: the two-finger pinch of a trackpad did not zoom the Flow canvas (or the panel designer) on Linux.** Windows
  gives a pinch to the app as the wheel with Ctrl held and macOS as a magnify event, and egui turns both into a zoom;
  on Linux the window is always an X11 window (through XWayland in a Wayland session) and winit's X11 backend does not
  know the pinch gestures of XInput 2.4, so nothing arrived. A small thread (`platform::linux::pinch`) now reads the
  pinch from the X server and hands it to both views as a zoom factor. Needs an X server or XWayland with XInput 2.4
  (X.Org 21.1, XWayland 23.1 or newer); without it nothing changes (Alt + scroll and Ctrl + scroll still zoom).

- **The center of the pie shows the CursorPie mark when it has nothing else to show.** With no app icon and no active
  mode (or parent submenu icon) the center disc was left empty; it now carries the logo's mark (the ring of four arcs
  around a diamond), tinted with the ink of the pie like the other center icons.

- **Fixed: a black square flashed in the pie every time it appeared (Wayland session).** The pie window is an
  XWayland window with a transparent pixel format. XWayland drops the Wayland surface of a window when it is
  unmapped and creates a new one, with no buffer, when it is mapped again, so every map (not only the first one)
  shows what the window holds before any drawing until the first buffer reaches the compositor: black. The empty
  frames that `show_wait` and `settle_frames` draw cannot help with that, because they are drawn after the map. Every
  map is now done invisible: the window carries `_NET_WM_WINDOW_OPACITY` 0 (honored by KWin and Mutter; compositors
  that ignore it behave as before) until the empty frames after the map were drawn and 60 ms have passed, at most
  300 ms, and the hint is removed at once if the window is closed meanwhile. New log line `ui.veil_lifted`. X11
  sessions, macOS and Windows are not touched.

- **Fixed: the pie of a trackpad hold opened at the wrong place, or jumped, in a Wayland session (KDE Plasma).** A
  mouse was fine and a trackpad was not, because they asked the compositor different things. The mouse asks KWin
  where the pointer is at the moment of the press. The trackpad hold used the latest report KWin had sent on its own,
  and the KWin script reports at most every 100 ms and says nothing more when the pointer stops, so the pie was placed
  where the pointer had been a moment before (and the slice picked at the lift could be the one under an old
  position). The hold now asks for the pointer when it arms and when the finger lifts (`platform::cursor_pos_fresh`),
  and while the hold runs the script is told to report every 16 ms, as it is for a mouse hold (`platform::pointer_hot`).
  Nothing changes on X11, macOS or Windows.

## 0.8.15

- **New: memory in the diagnostic log (category `mem`).** A memory problem is only a number that is too big at some
  moment, so the log now writes it where memory is gained or given back: a `mark` with the footprint (the number
  Activity Monitor, Task Manager and `top` show) at the start, the press, the pie opening and closing, Settings
  opening and closing, sleep, and every 30 s; the cost of each app icon (`icon_cost`); what the icon cache holds
  (`icon_cache`); the trim thread's result with the footprint before and after (`trim`); and image files that decode
  to 4 MiB or more (`big_image`). `cargo tools diag` now names the step that raised the footprint by more than 64 MiB,
  an app icon that was handed over at more than 8 MiB, a trim that gave nothing back while the footprint was large,
  and a footprint that kept rising while idle. Off unless `mem` (or `all`) is in `CURSORPIE_DEBUG`, and compiled away
  in a release build without `debug-log`. See "Watch the memory" in `docs/DEBUGGING.md`.

- **Fixed: the first pie after the start still took the memory up to about 500 MB (macOS).** The diagnostic log of
  such a press showed what the first fix could not reach: every app icon was a TIFF of **73,949,448 bytes** (about
  70 MiB: `iconForFile` plus `TIFFRepresentation` write out every size macOS has for the app, and the copy into a
  `Vec` doubles it), six of them in 0.7 s, and `icon_relief` and `trim` found nothing to give back (the memory is held
  by AppKit and Icon Services, not by the allocator's cache of freed blocks). Only one 128 px page of those 70 MiB
  was ever used. The app icon is now drawn at one size (256 px, about 256 KiB) with
  `-[NSImage CGImageForProposedRect:context:hints:]`; the multi-size TIFF is read only when the system gives no
  picture. `icon_decode` now shows `source_bytes` of 262144 for such an icon.

- **Fixed: the pie made CursorPie's memory jump to about 500 MB and fall back only very slowly.** Showing the pie
  read the icon of every app slice, and on macOS each one is a multi-size TIFF of several MiB that is copied and
  decoded; the freed blocks stayed in the allocator's cache and were handed back only after `sleep_after_s`
  (60 s) or when the next press allocated again. Now (1) the allocator is asked to give the pages back right after
  each big icon, (2) a small thread (`ui::trim`) does the same two seconds after the last icon and after the pie
  closed, without needing a frame of the hidden window, and (3) only the icons of the first frame (the ring, the
  center, the active mode) are read on the UI thread while the hold timer runs; the app icons of submenus are read
  on the icon worker thread. The debug log has `icon_decode`, `icon_relief` and `trim` events to check the numbers.

- **Fixed: the memory of Settings was not given back after closing it.** Opening Settings (or a Flow) raised the RAM to
  about 400 MB and closing it brought it down by a few MB only, even after a long wait; it fell to about 120 MB only
  after the trigger was pressed once (even without the pie opening). The cause: while its window is hidden, eframe
  runs no egui pass and paints nothing, so the textures Settings had freed and the font atlas it had replaced were
  only released by the next pass that paints, and the heap was trimmed before that. Closing Settings now keeps the
  (empty, click-through) window on screen for three frames, drops the icons that only Settings showed at once, then
  hides the window and trims the heap. The icons of the other profiles are decoded again when the trigger is pressed,
  as after an idle sleep.

- **Fixed: Settings came back half reset.** Closing and opening Settings again went back to the Menu tab, but the page
  kept its scroll position (scroll down Behavior, close, open: the page was still scrolled, and General and About
  shared the same position too). Each opening now starts clean: the first tab (About when it was opened from there),
  and every page at the top. Each of Behavior, General and About also keeps its own scroll position while the window
  is open.

- **New: delete an imported icon image from the icon picker.** An image in the **Mine** list can now be removed
  without opening the `icons` folder: drag it onto the trash strip that appears under the grid, or point at it and
  press Delete (Backspace on a Mac keyboard; ignored while a text field has the keyboard). Only images that sit
  directly in the icons folder are ever deleted, never the original of an imported picture or a file chosen in place,
  and the status bar says how many slices were still using the image (they show `?` until another icon is chosen).

- **Fixed: the browser extension did not know the open tabs until a tab was clicked.** When CursorPie started after the
  browser, the native messaging host ended at once (the app was not there yet) and the extension only tried again a
  minute later or at the next tab event, so _Open URL_ slices showed no badges until the person clicked a tab. The
  host now waits for the app (and again after the app restarts) and keeps the browser's port open; the app greets each
  new connection with a `sync` message and the extension answers with `hello` and the full list of tabs at once. An
  extension from before this change ignores `sync` and behaves as before until it is reloaded.

## 0.8.14

- **The Windows setup now says that programs running as administrator ignore the pie.** Many people thought
  CursorPie was broken when its slices did nothing in Task Manager or an elevated terminal: Windows does not let a
  normal program send input to those windows (UIPI), and that was written only in Settings and the docs. The setup
  wizard now has a short page after the license that explains it and how to run CursorPie as administrator, and
  the question that the first start from the zip asks says the same in one sentence. Silent installs and the
  automatic update skip the wizard page, as every page. `docs/user/install.md` and `known-issues.md` explain the
  cost of running as administrator (a UAC prompt at each start, and shell commands with full rights).

- **Fixed (Wayland): the cursor sometimes seemed to vanish when the pie opened.** In a Wayland session the pie of a
  mouse hold let the pointer through to the window below, so that window kept the say over the cursor image: a
  terminal, editor, browser or video player that had hidden its cursor (after typing, or after a moment without
  movement) kept it hidden while the pie was open, and the person saw a pie and no cursor. The pie of a mouse hold
  now takes the pointer, as the pie of a trackpad hold already did (the cursor is a plain arrow and the app below
  hears nothing), and lets it through again when it hides. The pie of the menu-bar preview never takes it. Sessions
  on X11, macOS and Windows are unchanged.

- **Fixed: the notice and a panel still jerked against each other, and the old panel flashed when the pie came.**
  The notice used to be hidden when it was over and the window mapped again for the panel, so the screen was bare
  for a moment and the keyboard focus was given away and asked for again (the pill seemed to stutter before the
  panel came). Now the window goes straight from the faded notice to the panel without being hidden, and the panel
  only asks for the keyboard while it does not have it. A panel that was closed left its last frame in the hidden
  window, and the next window to come up (the pie) showed it for a moment before its own first frame: the window
  now draws one empty frame before it is hidden.

- **Fixed: the notice pill vanished the moment a panel came up.** The pill and a panel share one window, and a
  panel that arrived while the pill was on screen (a flow that shows a notice and then asks something, or opens
  its progress panel) hid the pill at once, in the middle of its animation. It looked like something had gone
  wrong. Now the pill finishes coming in, goes away with its usual fade and sink, and the panel opens as soon as
  the window is free (a fraction of a second later). Notes that arrive while the pill is leaving no longer bring
  it back, and no new pill appears while a panel waits for the window. The pie and Settings still take the window
  at once, as before.

## 0.8.13

- Publish crates v.0.8.12

## 0.8.12

- **New: the notice follows a run from its start to its end.** A flow, a pause or a sequence with a pause in it
  used to show its notice for a moment at the start and then went quiet: while a _Wait_ block, a _Run and wait_
  command or a long flow was still busy nothing was on the screen, and then something appeared out of nowhere.
  Now the pill stays until the run is over and says what it is doing and how far it has come: a thin bar along the
  bottom edge and a small label at the right end. A _Wait_ shows a countdown (`Waiting`, `3.2 s`), a _Run and wait_
  block shows a moving bar and the seconds it has taken (`Running command`, `12 s`, the command itself is still never
  shown), a flow that goes straight through shows `3/8`, a _Repeat_ or _For each_ loop shows its round (`2/10`), a
  flow with a _While_ or a call shows `step 4`, and a sequence shows its step. When the run ends the bar fills and the
  pill goes away after its usual time (_Behavior > Notice_). A run that is hidden with _Hide notices_, or with the
  notice switched off, stays quiet as before, and a pill whose run is never reported as ended leaves by itself after
  150 seconds. New: `Host::progress` and `Host::wait` in `flowrun` (both have a default, so a host that does not
  care changes nothing), `HudProgress` and `Msg::HudEnd` in the notice.

- **Fixed: the notice glitched and stuttered while a flow ran.** Every new note of a run replaced the old one in a
  single frame: the pill jumped to the width of the new text (and to a new height when a bar appeared), the text and
  the icon swapped without a transition, the bar leapt to its new fill, and a moving bar started again from the left
  with every block. A quick flow made this flash by many times a second, and a note that arrived while the pill was
  already leaving made it snap back to full size. Now the pill glides to its new size (a run never makes it narrower),
  the old text and icon fade out while the new ones fade in, the bar glides to its new fill (and to full when the run
  ends), a moving bar keeps moving from one block to the next, a pill that was leaving eases back instead of
  snapping, and notes that follow each other faster than 260 ms wait their turn so that only the last of them is shown
  (a pause still counts its time from when its note arrived). The bounce of the badge is softer and does not start
  again while the last one is still ringing. The text is also laid out once per frame instead of three times.

- **New: a progress panel with a Stop button for long runs, and an _Update panel_ block.** On top of the small
  notice, a flow, a pause or a sequence that lasts more than about two seconds now opens a small panel with a bar,
  the step that is running (`Waiting`, `Running command`, `Open app: Safari`), how far it has come (`3.2 s`, `12 s`,
  `Round 2/10`, `Step 3/8`) and a **Stop** button. Stop ends the run at the next block and cuts a _Wait_ or a _Run
  and wait_ short (the command is ended); stopping is not reported as a problem and the notice says `Stopped`. The
  panel never takes the keyboard on Windows, so the keys a flow sends still reach their app; on macOS and Linux a run
  that presses keys keeps the small notice instead. Another panel or the pie takes the window from it at once. _Behavior >
  Show a progress panel for long runs_ (`progress_panel`, on by default) switches it off. The new flow block **Update
  panel** (`PanelUpdate`: _Open_, _Update_, _Close_) lets a flow open its own progress panel and set its title,
  label and bar (a fraction, `40%` or `busy`) while it works, with an optional Stop button; nothing waits for it and
  it goes away when the run ends. New: `Ending::Cancelled`, `Host::cancelled`, `Host::panel_command` and
  `parse_panel_value` in `flowrun`, `PanelSpec::passive`, the `runpanel` module, `platform::native::exec_until`,
  `docs/examples/flows/progress-panel.cursorpie-flow.json`.

- **New: building flows for the browser extension is much easier.** The _Browser command_ block used to be a text
  field in which the person had to type the exact name of a command (`media.seek_fwd 30`) from the documentation, and
  a mistake only showed when the flow ran. Now the block has the same list of commands as a slice (`Media: Next
track`, `Tabs: Go to a tab with this text`...) with a field beside it for the text the command takes, marked
  required or optional; in the list view it also says whether a browser can be reached right now (extension off, no
  browser connected, extension from another version). The add-block menus have a **Browser** group with ready
  commands (play / pause, next track, is the music playing?, address of the tab...). **New flow > Start from a
  template** adds a complete working flow to learn from: a _Browser remote_ (a panel with play, next, previous and
  volume that stays open, one Branch and one command per button), _Pause the music only if it plays_ and _Go to the
  tab that has a word_. A command name that is a `{{variable}}` is still typed. New: `config::flow_templates`,
  `docs/examples/flows/browser-*.cursorpie-flow.json`.

## 0.8.11

- **Fixed: Settings ended up behind another window after cancelling a file chooser (macOS).** When the chooser
  closed, the system gave the focus to the window below Settings, and the one request to bring Settings back could
  lose against it, leaving Settings covered and flickering. CursorPie now activates itself through the system's own
  call, asks again a moment later if the focus still is not back, and leaves a window that already has the focus alone.

- **Fixed: the app list missed apps that exist.** _macOS:_ Finder, Screen Sharing and the other apps in
  `/System/Library/CoreServices`, apps one folder down (Adobe, Setapp, Chrome apps, `~/Applications/...`) and
  iPhone/iPad apps were not listed; an app was named by `CFBundleName` (`Code`) instead of what Finder shows
  (`Visual Studio Code`), so searching `visual` found nothing, and the name in the system language (`Kalkulator`) was
  unknown. _Windows:_ the Start menu was read once and kept until CursorPie quit, so an app installed later never
  appeared, and a program that was once not found was never looked for again; apps that start through an
  `Update.exe` (Discord, GitHub Desktop, Slack, Postman) were dropped as updaters. _Linux:_ desktop entries in
  folders below `applications/` (`kde4/`, `wine/Programs/`) and entries with a byte that is not UTF-8 were skipped, the
  names in the language of the desktop were not searchable, and the Nix directories were not read. The search now
  also looks at the other names of an app and ignores spaces and punctuation (`vs code`, `whats app`).

- **Fixed: memory kept after the Flow editor was closed.** Zooming the Flow canvas (or the panel designer) cut a new
  set of glyphs for every zoom value a pinch or a wheel passed through, and egui never gives a glyph back, so memory
  grew from tens of MB to hundreds of MB, even toward 1 GB. The canvas now cuts its text at a ladder of 16 sizes
  (at most 10% from the real zoom, the text keeps its exact size on screen), and the font atlas is rebuilt when
  Settings closes or the UI sleeps. Closing Settings or sleeping also drops the flows of the working copy (and the
  two copies the undo history held of them), the open sheets and the Flow tab's selection.

## 0.8.10

- **New: Telegram folders, mentions and a way to skip muted chats.** Three new commands for Telegram Web: `chat.folder`
  (go to a folder tab by name or number, so `chat.next_unread` then walks that folder), `chat.folders` (the folder names,
  for a flow; it reads names, so it needs _Allow reading messages_ and is flagged like `chat.unread_names`) and
  `chat.next_mention` (the next chat where somebody mentioned you, muted ones included). They always act on a Telegram tab,
  also when WhatsApp is the one in front, and refuse `@whatsapp`. The new option **Skip muted chats** (on by default) makes
  `chat.next_unread`, `chat.prev_unread`, `chat.unread_names` and the count read from the chat list leave out muted chats;
  the number in the page title is not changed. Turn it off for the old behaviour.
- **New: saved replies.** Write short texts in the options (_Saved replies_, JSON: `{"omw": "On my way, back at {time}"}`)
  and use `chat.template omw` in the open chat or `chat.template omw :: Anna` to send it to a chat (checked like
  `chat.send_to`). `{time}` and `{date}` are filled in, and `{name}` / `{first}` with the chat's name: the one after `::`,
  else the open chat's, which needs _Allow reading messages_. It sends only with _Allow sending messages_ on, else it only
  types it, and an imported flow that has it is flagged like `chat.send`. `chat.templates` gives the names, not the texts.
- **New: open a Telegram chat by @username.** `chat.open @durov`, `chat.draft_to @durov :: hi` and `chat.template omw ::
@durov` go by address, so they do not depend on the language of the page or on the search box. The chat that opened shows
  the person's name, not the @username, so it cannot be checked: these type the message but `chat.send_to @user` and a saved
  reply to an @username **never send** (they say so). Nothing is typed when the open chat did not change.
- **New: two example flows for Telegram**, `telegram-folder-triage` (ask for a folder, show its unread chats, offer to open
  the next one) and `telegram-saved-reply` (choose a saved reply and a chat, confirm, send), in `docs/examples/flows/`, in
  the AI kit (`flow_kit::EXAMPLES`, now 10) and in the flow checker's test. Not run against a real Telegram Web.
- **Changed: the app and the extension know 92 commands** (87 before); `bridge::commands::READS_CHATS` has `chat.folders`
  and `is_sensitive` has `chat.template`. New tests in `tests/browser-extension/chat-telegram.test.js` (no jsdom) and
  `bridge::commands`. The selectors for muted chats, mentions and folders (`muted`, `mention`, `folders`, with the usual
  overrides) were written from how Telegram Web looks and are not checked against the live site: if a command does not find
  its tab, put a working selector first in _Selector overrides_.
- **New: a flow can read a chat and wait for the answer.** Six new browser commands for WhatsApp Web and Telegram Web:
  `chat.unread` (how many chats wait, a plain number), `chat.who` (which chat is open), `chat.unread_names` (the names of
  the chats that wait), `chat.last` (the last message that came), `chat.read` (the last messages, `them:` / `me:`) and
  `chat.wait` (waits for the next message and gives it). The ones that read a chat refuse until the new option **Allow
  reading messages** is turned on in the extension options (off by default); `chat.unread` reads numbers only. A send
  notes what is in the chat first, so `chat.send_to` then `chat.wait` gives the answer to that message and not an older
  one. What the extension remembers to tell messages apart is a number made from the text, never the text. The answer of
  a command can now be up to 4000 characters (it was 300; a notification still shows 300).
- **Changed: a flow waits for `chat.wait` as long as it was told, and that time does not count.** `chat.wait` is waited
  for as long as it was told (1 to 300 seconds, 60 without a number) and 15 seconds more, not the 20 seconds of the other
  chat commands, and like a `ui.*` panel it does not count against the 10 minutes a flow may run
  (`bridge::commands::wait_secs_for`, `waits_for_others`).
- **Changed: a command that reads a chat is flagged like one that sends.** An imported flow or profile that has
  `chat.who`, `chat.unread_names`, `chat.last`, `chat.read` or `chat.wait` is flagged, and its block is red.
- **Changed: the privacy page of the extension no longer says that it never reads a message.** It says what is read, by
  which commands, and only with _Allow reading messages_ on. `docs/BROWSER_EXTENSION.md` lists the new commands and
  options (and 87 commands, and 10 minutes where it said 2 by mistake).
- **New: two example flows for chats**, `quick-reply-unread-chats` (an unread chat at a time, a person types and confirms
  each reply, then `chat.send_to`) and `send-then-wait-for-answer`, in `docs/examples/flows/`, in the AI kit and in the
  tests, which run both against a scripted host.
- **Flow canvas: an inspector for the selected block, and a way to follow a variable.** Clicking a block now opens a
  panel at the right of the canvas (a press anywhere in the block selects it, not only its header). Its _Block_ page
  edits the block's settings, lists its problems in words, and shows what it _reads_ (is it set before this block? which
  blocks can the value come from?) and _writes_ (which blocks read it afterwards?), each block a button that jumps to
  it. Its _Variables_ page lists every variable of the flow with who sets and who reads it, and marks one that is read
  where nothing sets it. Following a variable rings the blocks that set it in green and the ones that read it in blue
  on the canvas and draws dashed lines along which its value travels. The analysis is public in
  `config::flow_check` (`variables`, `data_edges`, `is_set_before`, `block_reads`, `block_writes`).
- **A panel's height can be set, not only its width.** A panel used to be as tall as its content needed, with no way to
  change it. The panel description has a new optional `height` (200 to 900 points; without it nothing changes), and the
  panel designer has a _Height_ control under _Width_: _Auto_ keeps fitting the content, _Fixed_ gives a slider. With a
  fixed height a taller content scrolls in the body and the buttons stay at the bottom; a shorter one leaves room above
  the buttons. A panel is still never taller than the display leaves room for.
- **An update now brings the program's other files along, and the browser extension reloads itself.** An update used to
  replace only the executable, so everything CursorPie had written outside it stayed as the old release left it, and after
  every update the browser extension had to be exported again and reloaded by hand. The first start of a new version now
  writes those files again (`src/refresh.rs`): the browser extension folder (when it was exported before), the login entry
  (when _Launch at login_ is on, from an ordinary install only), and on Linux the desktop entry (when it is still
  CursorPie's own) and the GNOME Shell extension (when installed). The app then asks the extension in the browser to
  reload itself, once per version, so nobody goes to the extensions page. `config.json`, its backups and everything else
  that is yours are never touched. It works the same however the new version got there (the update button, Setup, a disk
  image, `install.sh`). One manual step is left, once: the extension already loaded in the browser when you move to this
  version is the old code, which does not know the request, so reload it by hand as before; from then on updates need
  nothing.
- **Fixed: zooming keeps the text sharp, in the Flow canvas and in the panel designer.** The blocks (and the panel in
  the designer's preview) used to be drawn once and then stretched, so the text and the icons turned blurry past 100%,
  which is why the zoom stopped at 125% (150% in the designer). The text is now laid out again at the zoomed size, so
  it is as crisp as a vector drawing at any zoom, and both go up to 400% (the Flow canvas from 30%, the designer from
  50%). Lines, dots and boxes were already sharp.
- **New: a flow can call the browser extension and wait for it.** The _System_ block has a new operation, **Browser
  command** (_Do something > Browser command_): the command line (`media.next`, `tab.find {{word}}`...) goes to the
  extension, the block waits for the answer, `ok` says whether it worked and a variable gets the browser's words (`tab.url`,
  `tab.count`, `media.state` and the other questions answer there, and a failure says why). A command that sends a message
  or closes a window makes the block red. The checker reads a command that is written out (`bad-browser-command`). New
  questions for flows: `tab.url`, `tab.count`, `media.state` (`playing`, `paused` or `none`). See
  `docs/BROWSER_EXTENSION.md`, _Using the commands in a flow_.
- **Fixed: the answer of a browser command was thrown away.** The extension said how a command went, but the app only showed
  it as a notification, so nothing could wait for it. The bridge now keeps who waits for which command and gives the answer
  to that one (and only to the browser the command went to); a browser that goes away ends the wait at once.
- **Fixed: "Starting one player pauses the others" woke the wrong player.** Pausing the others made the extension remember
  the paused tab as the one that played last, so _Play_ after a _Pause_ resumed the player that had just been silenced.
- **Fixed: `ui.video` was missing from `docs/BROWSER_EXTENSION.md`**, which made the test that every command is documented
  fail; the count of commands in that page was out of date too.
- **Fixed: two bridge log lines used the field names `cmd` and `command`**, which `cargo tools diag-lint` refuses.
- **Fixed: `docs/flow.schema.json` listed only 16 of the _System_ operations**, so a flow that used any later one (folders,
  copy, volume...) did not validate. It lists all of them now.

- **New: the Flow tab can fill the whole display.** The capsule that floats at the bottom of the Flow tab (the round
  "+" with the tidy and reset buttons, and the one of the list of steps) has a new button, and **F11** does the same:
  the sidebar goes away, the window becomes the display and the canvas gets all the room. Esc (after any sheet or
  popup that is open) or the same button goes back to the window as it was. Only the Flow tab has it. The "bring the
  blocks back into view" button now uses the viewfinder icon.
- **Changed: a tidier panel designer.** "Add" and "Start from" are two buttons that open under them, so the list of what
  the panel holds has the whole left column; the panel in the middle scrolls when it is taller than the room instead of
  being cut off; numbers (smallest / largest, starts at / step) sit side by side; the sheet is as big as the window
  allows (more in full screen).

- **New: a visual panel designer.** The _Ask_ block in Panel mode has a **Design...** button that opens a sheet: add
  widgets with a click, start from a template, see the real panel (the same drawing a flow uses) and click a widget to
  edit it in the inspector. No JSON to type; every change is written to the block at once and can be undone. The rules
  (new widgets, moving, copying, renaming without breaking a condition, options as lines of text, templates) are in the
  tested `panel::design`; the drawing is `ui::settings::panel_designer`.
- **New: more to interact with in a panel.** Widgets `number` (`-`/`+` and a box, with range, step and unit) and `color`
  (accent swatches and the system picker, answers `#rrggbb`); `choice` layout `dropdown`; and `show_if` on every widget,
  so a form can show a field only when another one says so. What the designer writes is short JSON (defaults left out,
  plain options as texts). See `docs/PANELS.md`.

- **New: panels, a base for interactive windows.** Until now CursorPie could only _announce_ (the pill) or ask through
  a window of the system (a text, yes or no). A **panel** is a small window of CursorPie's own with a title, widgets
  (text, boxes, switches, sliders, single and multiple choice lists with search, a progress bar) and up to four buttons,
  in the look of Settings, that hands back what the person entered. It is plain data (JSON, `docs/PANELS.md`), so flows,
  the browser extension and later extensions share one base instead of each writing window code, and a custom look
  replaces only the drawing (`ui::settings::panel_form`). New module `panel` (`spec`, `form`, `layout`, `broker`): the
  description with limits, the live form with the rules (required boxes, snapping, patches), the size worked out before
  the first frame, and `panel::ask` for any thread. It is a fourth mode of the one window: not click-through, takes the
  keyboard, placed next to the pointer, centered or at an edge; the pie and Settings take it over (the panel is answered
  as cancelled), and a panel asked for while the window is busy waits in a short queue.
- **New: the _Ask_ block has a Panel mode.** The block's text is the JSON of a panel; every field sets the variable
  named by its `id`, `out` gets the button that was pressed, `ok` says whether a confirming button was. Variables are
  filled into the _texts_ of the panel after the JSON is read, so a value cannot add a widget. The editor's checks know
  the variables of the fields. Example: `docs/examples/flows/panel-form.cursorpie-flow.json`. New `Host::ask_panel`
  (default: cannot).
- **New: the browser extension can show panels.** Bridge messages `panel`, `panel_update`, `panel_close` and
  `panel_result`; `CP.ui` in the extension (`show`, `update`, `close`, `confirm`, `prompt`, `pick`) in new `ui.js`; a new
  group of commands, **Panels**: `ui.tabs` (searchable list of open tabs), `ui.find_tab`, `ui.confirm_close_others`,
  `ui.media`. Off by default: Settings, Behavior, _Let the browser extension show panels_ (`extension_panels`; needs
  the bridge). The app checks every description, allows 4 waiting panels and 5 minutes each, and lets an extension
  change only its own panels. New config fields `panels` (flows, default on) and `extension_panels` (default off).
  Not compiled or run on a machine here: see "Known limits" in `docs/PANELS.md`; the JavaScript tests
  (`tests/browser-extension/ui.test.js`) pass.

- **New: build a flow with an AI agent.** **New flow** (in the header of the Flow tab, and on the page that shows when
  there is no flow yet) now opens a sheet that asks how to make it. **Build it myself** adds an empty flow, as before.
  **Build it with AI** shows a box like the one of a chat, but nothing is sent: you say what the flow should do and
  press **Copy kit**. CursorPie puts a zip on the clipboard (the flow guide, its JSON Schema, checked examples, your
  words and the name of your system), so you can paste it into any AI agent, add a prompt such as "make this", and
  bring the file it returns back with **Import result**. **Show in folder** and **Copy path** are there for an agent
  that does not take a pasted file (the kit is `ai-kit/CursorPie-flow-kit.zip` next to `config.json`). The app makes no
  new connection: the zip is made on your computer. On Linux, putting a file on the clipboard needs `wl-copy` or
  `xclip`.

- **Change: the Behavior tab is split into four pages.** It had grown to fourteen panels in one long column. A switch at
  the top now picks a page: **Input** (Trigger, Side buttons, Trackpad), **Look** (Look & feel, Pie material, App icon
  in the pie, Action notice), **Apps** (Pie of the app in front, App detection, Open app and web page slices, Browser
  extension, Desktops) and **System** (Startup, Memory). The panels themselves are unchanged. The page that is open is
  kept while Settings is open.

- **Change: Flow tab, the list of flows and the buttons.** New flow and Import moved from the bottom of the list of flows
  to the header of the page, into one capsule with Duplicate, Export and Delete (new and import, duplicate and export,
  delete, with thin lines between the groups; New is filled with the accent). The list of flows is only the flows now:
  a title with the number of flows, a list that scrolls (and shows the open flow when it comes out), names that are too
  long cut with an ellipsis (the tooltip has the whole name), the open flow with a wash of the accent. The list slides
  in from the left edge of the page with one eased movement and a fade, and goes back a moment after the pointer has
  left it (no flicker when the pointer slips off the edge).

- **New: a notice on the screen tells what CursorPie is doing.** When a slice, a flow or a sequence runs, a small pill
  appears near the bottom of the display (icon in the accent color, a title such as "Open app" and the name below it:
  "Safari", "github.com", "Cmd + C", the name of the flow) and goes away after 1.5 s. It comes in as a round badge that rises from below, then stretches into the pill and shows its text; it leaves the other way round. Each step of a sequence or block
  of a flow announces itself, so a flow can be followed. It follows the look of the pie (light or dark card, smoked
  graphite for the Glass material) and the accent color. A shell command is announced as "Run command" and never shown
  (it may hold a secret); the back and forward buttons stay quiet. Settings, Behavior, **Action notice** switches it
  off, picks the position (bottom, top or below the pointer), the time, and previews the pill. It is a third mode of
  the one window (small, click-through, always on top), so the pie and Settings take it over at once; a notice that
  arrives while the pie is still fading waits for it. New config fields `hud`, `hud_position`, `hud_ms`. On Windows
  the window is shown with `WS_EX_NOACTIVATE` so it does not take the keyboard from the app the action is for.

- **Fix (Windows): the yellow badge of an "Open app" slice opened a new window.** The yellow circle with a square means
  "go to the app that is open", but on Windows that path started the program again whenever its window was already on
  the desktop that is shown (only a window on another virtual desktop was handled). Chrome, Explorer, terminals and
  editors answer a second start with a new window. `reveal_app` now puts an existing window in front (the one on this
  desktop first, restoring it if minimized) and starts the program only when it has no window. Also fixes a plain
  "Open app" slice of a running app, which went through the same call. With the desktop setting "Do nothing", a
  running app is still started again as before.

- **Fix: the pie did not show (or the press was lost) after starting or a long idle time, and Settings opened instead.**
  A held trigger opens the pie and, if the pie is still untouched after `settings_hold_ms`, Settings in its place. That
  second step was judged by the clock alone, counted from the moment the tap _asked_ for the pie. When the window was
  slow to show it (first press after the program started or after a long idle time) the person saw nothing, kept
  holding, and Settings opened over the missing pie; releasing early looked like a lost press that had to be repeated.
  Now the UI tells the tap when the pie has really been drawn (`tap::pie_painted`, on the second drawn frame, so the
  window is mapped). Settings only opens for a pie that has been on screen, untouched, for the whole dwell time
  (`settings_hold_ms - pie_hold_ms`), and the Settings ring starts counting at the same moment. While the pie has not
  been drawn the tap looks again every 40 ms and wakes the UI, asks for the pie a second time after 350 ms, and after
  4 s gives up without opening anything. `show_pie` also always asks for a frame, since a hidden window only draws when
  somebody asks. New log events (`CURSORPIE_DEBUG=1`): `tap/hold_pie_resend`, `tap/hold_settings_no_pie`,
  `tap/hold_settings_no_timer`. Not reproduced on a Mac: the cause is deduced from the code, and the log tells whether
  the pie was late (`hold_pie_resend`) when it still happens.

- **Icons: more image formats, drag and drop, and pop-ups that say why.** Besides PNG, JPEG and WebP, an icon can now
  be a GIF, BMP, ICO, TIFF, TGA, QOI or PNM image (the file choosers offer them too). Those are converted to a PNG in
  the icons folder when they are imported, so profile sharing keeps working. You can **drag image files onto the
  Settings window**: the first one becomes the icon of the selected slice (with the icon picker open or not), the
  others are added to "Mine". Dropped files are copied into the icons folder. An image that cannot be used no longer
  produces one line in the status bar that vanishes: a **pop-up** names the file and the reason (a folder, SVG, HEIC /
  AVIF, PDF, archive, empty or damaged file, too large, permission denied, file gone ...).

- **The browser extension can now control the browser.** New slice kind **Browser**: 73 commands that play, pause and
  skip songs (YouTube, YouTube Music, Spotify, SoundCloud, Deezer, TIDAL, Bandcamp, Netflix...), change volume, speed and
  position, like a track, skip an ad; switch, close, reopen, find, move, pin, mute, discard and de-duplicate tabs;
  manage windows; scroll; and move around WhatsApp Web and Telegram Web (next unread chat, open a chat by name, type or
  send a message, how many chats wait). A command acts on the tab that makes sound, or the one that played last, or the
  chat tab in front, and goes to the right browser when several are connected. Messages are protected: nothing is sent
  unless "Allow sending messages" is on in the extension's new options page, and `chat.send_to` checks the name in the
  header of the opened chat first. The Settings line shows sound and unread counts and says when the extension in the
  browser is older than the app. Importing a profile that can send messages is flagged. See
  `docs/BROWSER_EXTENSION.md`. The extension's pages selectors were written from how the sites are built and tested
  against copies of them, not the live sites: the manual says how to correct one without a new version.

- **Lists, filters and more of the system in flows.** A flow can now hold a **list** (an array): the new **List**
  block makes one from a text, adds, takes, gets, changes and removes items, counts, joins, finds, sorts, reverses,
  removes duplicates, shuffles and adds up; the new **For each item** block goes through a list one item at a time. A
  list is a variable that holds a JSON array like `["a","b"]`, so it can be shown, saved or handed to a command.
  `{{name}}` takes **filters** that can be chained (`{{name|trim|upper}}`): `trim upper lower title reverse len int
round abs num bool json url basename dirname ext count first last join item default replace` besides `sh`. The
  **System** block has twelve more operations: list a folder, make a folder, copy, move or rename, delete a file (never
  a folder), a file's size and change time, facts about this computer, speak a text, save a screenshot, read and set the
  volume. The red-wire check and the failure report know all of it (a list that was never made, a body that does not
  come back, a position that is not a number, a filter that does not exist, a folder that cannot be listed...). A flow
  that uses these blocks cannot be opened by an older CursorPie. Details: `docs/FLOWS_FOR_AI.md`,
  `guides::flow_checks`.
- **Red wires.** The Flow editor now checks a flow while you draw it, without running it. A wire that cannot work
  (a block that uses `{{name}}` that nothing before it sets, the body of a Repeat or While that never leads back, a
  loop nothing can leave, a Call to a flow that does not exist, a Calculate on something that is not a number or on
  zero) is drawn **red**; one that is probably a mistake (a shortcut right after Open app, a Shell command that is not
  waited for, a wire out of a block no run reaches) is **amber**. Each has a `!` whose tooltip says why, the block is
  outlined the same way, and the strip under the canvas counts them. Nothing is blocked. The rules are in
  `guides::flow_checks` and, for agents, in `docs/FLOWS_FOR_AI.md`.
- **Flows that fail now say so.** A block that cannot do its work (a command with an error code, a file that cannot be
  read or written, the clipboard, a missing called flow, a bad calculation, the step or time limit) is written down
  with the reason. Unless the flow looked at it (an If / else or While that reads `ok` or `code`), you get a desktop
  notification, the block is outlined in red in the editor, and while Settings is open a sheet lists every problem in
  full. A loop that fails each round is said once, with a count.

- **The Flow canvas is steadier and tidier.** The right button now opens the list of blocks the moment it is pressed,
  anywhere on the canvas (on a block or on one of its fields too), so a slow press or a trackpad that moves a little no
  longer does nothing, and a press while another list was open no longer just closes it. Long names are cut with an
  ellipsis (with the whole name as a tooltip) instead of running under the caret or the check of a list, a line break
  in a one-line field is shown as a pilcrow instead of an empty box, and the hint line sits in a strip of its own that
  no block can cover (and shortens itself in a narrow window). New **Tidy** button: lines the blocks up in columns,
  left to right, so none covers another.

- **Importing a file now says why it failed, or what was repaired in it.** A refused flow or profile file opens a
  sheet with the reason (not valid JSON with the line and column, a profile given to the Flow tab or the other way
  round, a newer file format, a damaged block with the name of the unknown kind). A flow that loads but had to be
  repaired, or that will probably not work, lists it too: a missing or doubled Start / End, wires that were removed,
  blocks that can never run, a file number older than the blocks it uses, a Command that is not waited for, a shortcut
  right after opening an app without a Wait, commands made for another system, and AppleScript that puts a path on the
  clipboard as text instead of a file.

## 0.8.9

- **Flows are now programs, not only chains: variables, decisions, loops, questions and system blocks.** A flow is a
  graph. Blocks that decide (**If**, **Repeat**, **While**) have a second output (then / else, body / done); several
  wires may lead into a block and a wire may lead back, which makes a loop (a run still ends: at **End**, at the new
  **Stop**, at an output without a wire, after 10000 blocks or 10 minutes). A run has variables; `{{name}}` in any text
  field is replaced by the value and `{{name|sh}}` by the value quoted for the shell. New blocks: **Set**, **Calculate**,
  **Gate** (AND, OR, XOR, NAND, NOR, NOT), **If**, **Repeat**, **While**, **Call** (another flow), **Stop**, **Ask** (a
  window of the system asks for a text or yes / no, so a flow can wait for input), **Run and wait** (a shell command
  that is waited for, with its output and exit code kept; the variables are in its environment) and **System** (16
  operations: clipboard, type text, notification, read / write / append a file, file exists, environment variable, app
  in front, bring an app forward, app has a window, pointer position, random number, time, show in the file manager).
  The editor shows the second output, lets you unplug one wire at a time, and a block that runs a command or writes a
  file is red; importing a flow that has one warns before you save. Up to 128 blocks per flow (was 64). Flows made
  only of the old blocks still save as `cursorpie_flow: 1` and open in older versions; the others are `2`. Old
  configs load unchanged. No new dependency and no network request: the system blocks run the tools each system ships
  (`pbcopy`, PowerShell, `wl-copy` / `xclip`, `osascript`, `zenity` / `kdialog`, `notify-send`). There is still no
  block that moves or clicks the mouse.
- **Updates: CursorPie now tells you when a newer release is out, the same on macOS, Windows and Linux.** When Settings
  opens, it looks at most once a day for a newer stable release on GitHub. A newer one is offered once that day in a
  sheet over Settings (**Download** opens the release page in your browser, **Not now** closes it), and the About tab has
  a new **Updates** panel that always shows what the last look found, a **Check now** button and a **Check for updates
  automatically** switch (new setting `check_updates`, on by default, also for an existing `config.json`). CursorPie never
  downloads or installs anything by itself. The look runs the system's `curl` (or `wget` where there is no `curl`), so
  there is no new dependency; it sends only your IP address and the CursorPie version. This is the first network
  connection the app makes, so the privacy text in About and the README now say so. What is remembered between runs
  lives in `update_state.json` next to `config.json`. New log lines `ui.update_checked`, `ui.update_offered` and
  `ui.update_download`.
- **"Open app" badges on Windows and Linux: a window that was hidden and is shown again is no longer learned as a new
  window.** There the request for a new window is the app started again, and an app that keeps one instance (a
  launcher closed to the tray) answers by showing the window it had hidden, which a count taken before says is one more
  window. The windows are now told apart by number (new `platform::window_ids`, `windowing::Windows` and
  `Baseline`): Windows takes every top-level window handle of the program, visible or not, before the request; X11 and
  KWin take the ids of the window manager's list (KWin lists hidden windows too). Only a window whose number was not
  known counts as new, and the log line `run.watched` says with `by_number` which way was used. macOS keeps the count
  taken right before Cmd+N. Limits: a program that destroys its window when it hides and makes another when it
  shows still looks new, and on X11 a window unmapped before CursorPie first listed it is not known. Written without
  a Windows machine and not compiled; the Linux part follows the same functions as the count.
- **"Open app" badges: a window shown by bringing the app forward is no longer learned as a new window.** An app that
  never opened a window (Artcraft Launcher: Cmd+N does nothing) was learned as one that does, four times in a row, and
  kept its two badges. The windows were counted before the app was brought forward (`open -a`) and again after the
  key, so a window that coming forward showed by itself (one closed to the menu bar, one made on activation) looked
  like the answer to the key. On macOS the count is now taken right before Cmd+N, with the app in front and its
  windows settled (`platform::open_app_new_window` returns `windowing::Asked`, the other systems say
  `Sent(None)` and keep the old count), and when the app never came to the front, so no key went out, nothing is
  learned (new log line `run.ask_not_sent`). `window-habits.json` has a generation: a file written by the old count
  loses its successes once, when it is read (the failures stand), so a wrong "takes new windows" goes back to doubt
  and two failures condemn it. Found by reading the code and the habit file, not reproduced with the app itself; the
  tests cover the model, not a session.
- **"Open app" badges: an app that failed twice in a row stops showing the green badge, even with older slips in its
  record.** The memory of `window-habits.json` only decided at a chance below 0.26 or from 0.7, and a record that mixed
  a few false successes with the failures (Zed: worked 2.0, failed 2.5, a chance of 0.46) stayed in between for ever,
  so the pie kept offering a badge that had just failed. Each record now also keeps its **streak** (the latest outcomes
  of one kind in a row; older files read as 0): two failures in a row make an app a one-window app whatever came
  before, for as long as the evidence has not faded (60 days), then the counts decide again; a success ends the run.
  Zed is also in the built-in list (Cmd+N there makes a file, not a window). Learning something now also makes the
  next press ask again instead of reusing the answer of the last 2 s (`running::invalidate`), and the log lines say
  more: `run.watched` has `peak` (the most windows counted), `run.learned` has `streak`. Found by reading the code and
  the habit file of a user, not reproduced with Zed itself; the tests cover the model, not a session.
- **Windows (and the other systems): fewer ways for the input hook to stall or lose a press.** Found by reading the
  code, not reproduced on a machine; the tests cover the timer logic, not a Windows session. Every press of the
  trigger button used to start a new OS thread (2 MiB of stack) from inside the low-level hook, for the hold timer
  and again for the double-click timer. Starting a thread runs the attach routine of every loaded DLL, which
  antivirus and overlay programs hook, and a hook that answers too slowly is dropped by Windows; when memory was
  short `thread::spawn` panicked, the hook failed open, but the press was left in `Pending` with its button already
  swallowed, so the click seemed to hang. Now one small thread, `cursorpie-timers`, started with the tap, runs both
  timers (a push into a channel is all the hook does), and if no timer can be started the press goes to the app
  at once and the gesture ends. The same was done for `set_active` (remembering the mode of a profile, called from
  the hook): a thread per switch became one `cursorpie-config-saver` thread that writes the newest state, which also
  means an older write can no longer land after a newer one. And the icon reader (`bitmap_bgra`) no longer leaks a
  screen DC when the bitmap is missing: a process that runs out of GDI handles (10 000) cannot draw any more.
- **Removed: the virtual trackpad developer tool.** It did not work well and was of no use. Gone are
  `cargo tools trackpad`, the Cargo feature `vtrackpad` (and its `CURSORPIE_VTRACKPAD*` variables), the floating
  trackpad windows (`ui::vtrackpad`), the virtual pointer and `platform::click_at` on every OS, the feed into the
  recognizer (`tap::virtual_*`), the two extra steps of `cargo tools ci` that checked it, and its sections in the
  documentation. Real trackpads are not affected. The older changelog entries about it stay as history.
- **Learning which apps keep one window: a popup is no longer taken for a new window.** The window count now leaves
  out what is not a main window of the app: Windows skips windows owned by another window and modal dialogs, X11
  keeps only `_NET_WM_WINDOW_TYPE_NORMAL` windows that are not transients (`WM_TRANSIENT_FOR`), KWin skips dialogs,
  utility windows, toolbars, menus and transients (the script reports a new `sec` field), and macOS skips windows
  smaller than 200 x 140 pt (alerts, sheets and small dialogs; a size guess, since only the Accessibility API says
  what a window is). A new window must also still be there on two looks in a row (about 400 ms), so a splash screen or
  notice that comes and goes does not count. And it takes two successes to vouch for an app (it still takes two
  failures to condemn one), so one popup that slipped through cannot override the built-in list.
- **"Open app" badges learn which apps cannot open another window.** Every time the green badge (new window) is used
  on an app that has windows, CursorPie counts the app's windows before and for up to 5 s after (new
  `platform::window_count`: Windows top-level windows of the program, X11 and KWin window list, macOS layer-0 windows
  of the process) and remembers whether one appeared (`learn`, `window-habits.json` next to `config.json`: app names
  and two decaying counts, nothing else). Two failures make an app a one-window app (no badges), one success makes it a
  multi-window app, whatever the built-in list says; evidence halves every 60 days and a success halves old failures,
  so a wrong verdict heals itself. An unknown count or an app that quit is not counted as a failure. New log lines
  `run.watched` and `run.learned`. The macOS counter has only been compiled by reading the code, not run.
- **"Open app" badges: not offered for an app that cannot open another window (all OSes).** The green + / yellow
  square pair appeared for every running app, also for a music player, a chat client or a calculator, where a new
  window can only fail. New module `windowing` decides it the same way everywhere: an app is "one window" when it
  says so (Linux: `SingleMainWindow=true` in its `.desktop` file, new `platform::declares_single_window`, a no-op on
  Windows and macOS, which have no such declaration) or when a built-in list of well-known one-window apps names it
  (matched by name, so `Spotify.exe`, `com.spotify.client`, `Spotify.app` and `Spotify` are the same app). Such a slice
  just brings the app forward, as before; every other app keeps the two badges. The question is asked on the worker
  thread with the "is it running" answer, so hovering is still a cache lookup (`running::offers_choice`).
- **Windows: the first click after Settings opens is no longer lost.** Settings (and a Settings window raised by a press
  of the trigger) took the focus through winit's `focus_window`, which injects an Alt key press and release; Windows
  then read the next click as the end of a menu activation and swallowed it, so the pie needed two clicks before one
  hold worked. The UI no longer sends `ViewportCommand::Focus` on Windows: `platform::raise_own_window` now brings the
  window forward with `SetForegroundWindow` (thread input attached for the call), from the next frame once the window
  is on screen, with no injected key.
- **Docs: Linux is described as X11 and Wayland everywhere.** The README, install guide, user guide, configuration
  reference, known issues, contributing guide, trackpad plan, the website and the package description still said that
  Linux means X11 (and that Wayland only works with XWayland windows). They now say that both session types work, what
  Wayland needs (the udev rule), and that the Wayland layer has not been run on real hardware yet. The user guide
  section is now `#trackpad-macos-windows-and-linux`.

## 0.8.8

- **Settings no longer stops answering clicks.** When the pie was closed while Settings was open (the hook
  watchdog resetting a gesture, a press that began before Settings opened, a cancelled trackpad hold), the shared
  window became click-through, so Settings stayed on screen but Close and Save did nothing, while the tray menu and
  CPU use looked normal. Hiding the pie now leaves Settings clickable on macOS, Windows and Linux.

## 0.8.7

- **Updates: CursorPie tells you when a newer release is out and can install it, the same on macOS, Windows and
  Linux.** When Settings opens, it looks at most once a day for a newer stable release on GitHub and offers it once that
  day in a sheet. **Install update** downloads the package for your system (the universal `.dmg` on macOS, the Setup
  program on Windows, the Ubuntu or Fedora `.tar.gz` on Linux x86_64), checks its size and SHA-256 against what GitHub
  publishes (nothing is installed without a checksum), replaces the installed copy and starts the new version. The
  downloaded program is started once with the new `--selftest` switch before it replaces the working one (macOS and
  Linux), and the old copy is put back if the swap fails. A copy that is not an ordinary install (a build folder, an
  unzipped Windows download, a folder you cannot write to) gets a **Download page** button instead. The About tab has
  a new **Updates** panel with the status, **Check now**, the install button and a **Check for updates automatically**
  switch (new setting `check_updates`, on by default, also for an existing `config.json`). Everything runs the system's
  `curl` (or `wget` where there is no `curl`), so there is no new dependency. This is the first network use of the app,
  so the privacy text in About and the README now say so. What is remembered between runs lives in
  `update_state.json` next to `config.json`. The Setup wizard script now starts the new version after a silent run with
  `/RESTARTAPP=1`. On macOS the update may ask you to allow Accessibility again, because the app is signed ad hoc.
  New log lines `ui.update_checked`, `ui.update_offered`, `ui.update_install`, `ui.update_staged`, `ui.update_apply`
  and `ui.update_download`.
- **"Open app" badges on Windows and Linux: a window that was hidden and is shown again is no longer learned as a new
  window.** There the request for a new window is the app started again, and an app that keeps one instance (a
  launcher closed to the tray) answers by showing the window it had hidden, which a count taken before says is one more
  window. The windows are now told apart by number (new `platform::window_ids`, `windowing::Windows` and
  `Baseline`): Windows takes every top-level window handle of the program, visible or not, before the request; X11 and
  KWin take the ids of the window manager's list (KWin lists hidden windows too). Only a window whose number was not
  known counts as new, and the log line `run.watched` says with `by_number` which way was used. macOS keeps the count
  taken right before Cmd+N. Limits: a program that destroys its window when it hides and makes another when it
  shows still looks new, and on X11 a window unmapped before CursorPie first listed it is not known. Written without
  a Windows machine and not compiled; the Linux part follows the same functions as the count.
- **"Open app" badges: a window shown by bringing the app forward is no longer learned as a new window.** An app that
  never opened a window (Artcraft Launcher: Cmd+N does nothing) was learned as one that does, four times in a row, and
  kept its two badges. The windows were counted before the app was brought forward (`open -a`) and again after the
  key, so a window that coming forward showed by itself (one closed to the menu bar, one made on activation) looked
  like the answer to the key. On macOS the count is now taken right before Cmd+N, with the app in front and its
  windows settled (`platform::open_app_new_window` returns `windowing::Asked`, the other systems say
  `Sent(None)` and keep the old count), and when the app never came to the front, so no key went out, nothing is
  learned (new log line `run.ask_not_sent`). `window-habits.json` has a generation: a file written by the old count
  loses its successes once, when it is read (the failures stand), so a wrong "takes new windows" goes back to doubt
  and two failures condemn it. Found by reading the code and the habit file, not reproduced with the app itself; the
  tests cover the model, not a session.
- **"Open app" badges: an app that failed twice in a row stops showing the green badge, even with older slips in its
  record.** The memory of `window-habits.json` only decided at a chance below 0.26 or from 0.7, and a record that mixed
  a few false successes with the failures (Zed: worked 2.0, failed 2.5, a chance of 0.46) stayed in between for ever,
  so the pie kept offering a badge that had just failed. Each record now also keeps its **streak** (the latest outcomes
  of one kind in a row; older files read as 0): two failures in a row make an app a one-window app whatever came
  before, for as long as the evidence has not faded (60 days), then the counts decide again; a success ends the run.
  Zed is also in the built-in list (Cmd+N there makes a file, not a window). Learning something now also makes the
  next press ask again instead of reusing the answer of the last 2 s (`running::invalidate`), and the log lines say
  more: `run.watched` has `peak` (the most windows counted), `run.learned` has `streak`. Found by reading the code and
  the habit file of a user, not reproduced with Zed itself; the tests cover the model, not a session.
- **Windows (and the other systems): fewer ways for the input hook to stall or lose a press.** Found by reading the
  code, not reproduced on a machine; the tests cover the timer logic, not a Windows session. Every press of the
  trigger button used to start a new OS thread (2 MiB of stack) from inside the low-level hook, for the hold timer
  and again for the double-click timer. Starting a thread runs the attach routine of every loaded DLL, which
  antivirus and overlay programs hook, and a hook that answers too slowly is dropped by Windows; when memory was
  short `thread::spawn` panicked, the hook failed open, but the press was left in `Pending` with its button already
  swallowed, so the click seemed to hang. Now one small thread, `cursorpie-timers`, started with the tap, runs both
  timers (a push into a channel is all the hook does), and if no timer can be started the press goes to the app
  at once and the gesture ends. The same was done for `set_active` (remembering the mode of a profile, called from
  the hook): a thread per switch became one `cursorpie-config-saver` thread that writes the newest state, which also
  means an older write can no longer land after a newer one. And the icon reader (`bitmap_bgra`) no longer leaks a
  screen DC when the bitmap is missing: a process that runs out of GDI handles (10 000) cannot draw any more.
- **Removed: the virtual trackpad developer tool.** It did not work well and was of no use. Gone are
  `cargo tools trackpad`, the Cargo feature `vtrackpad` (and its `CURSORPIE_VTRACKPAD*` variables), the floating
  trackpad windows (`ui::vtrackpad`), the virtual pointer and `platform::click_at` on every OS, the feed into the
  recognizer (`tap::virtual_*`), the two extra steps of `cargo tools ci` that checked it, and its sections in the
  documentation. Real trackpads are not affected. The older changelog entries about it stay as history.
- **Learning which apps keep one window: a popup is no longer taken for a new window.** The window count now leaves
  out what is not a main window of the app: Windows skips windows owned by another window and modal dialogs, X11
  keeps only `_NET_WM_WINDOW_TYPE_NORMAL` windows that are not transients (`WM_TRANSIENT_FOR`), KWin skips dialogs,
  utility windows, toolbars, menus and transients (the script reports a new `sec` field), and macOS skips windows
  smaller than 200 x 140 pt (alerts, sheets and small dialogs; a size guess, since only the Accessibility API says
  what a window is). A new window must also still be there on two looks in a row (about 400 ms), so a splash screen or
  notice that comes and goes does not count. And it takes two successes to vouch for an app (it still takes two
  failures to condemn one), so one popup that slipped through cannot override the built-in list.
- **"Open app" badges learn which apps cannot open another window.** Every time the green badge (new window) is used
  on an app that has windows, CursorPie counts the app's windows before and for up to 5 s after (new
  `platform::window_count`: Windows top-level windows of the program, X11 and KWin window list, macOS layer-0 windows
  of the process) and remembers whether one appeared (`learn`, `window-habits.json` next to `config.json`: app names
  and two decaying counts, nothing else). Two failures make an app a one-window app (no badges), one success makes it a
  multi-window app, whatever the built-in list says; evidence halves every 60 days and a success halves old failures,
  so a wrong verdict heals itself. An unknown count or an app that quit is not counted as a failure. New log lines
  `run.watched` and `run.learned`. The macOS counter has only been compiled by reading the code, not run.
- **"Open app" badges: not offered for an app that cannot open another window (all OSes).** The green + / yellow
  square pair appeared for every running app, also for a music player, a chat client or a calculator, where a new
  window can only fail. New module `windowing` decides it the same way everywhere: an app is "one window" when it
  says so (Linux: `SingleMainWindow=true` in its `.desktop` file, new `platform::declares_single_window`, a no-op on
  Windows and macOS, which have no such declaration) or when a built-in list of well-known one-window apps names it
  (matched by name, so `Spotify.exe`, `com.spotify.client`, `Spotify.app` and `Spotify` are the same app). Such a slice
  just brings the app forward, as before; every other app keeps the two badges. The question is asked on the worker
  thread with the "is it running" answer, so hovering is still a cache lookup (`running::offers_choice`).
- **Windows: the first click after Settings opens is no longer lost.** Settings (and a Settings window raised by a press
  of the trigger) took the focus through winit's `focus_window`, which injects an Alt key press and release; Windows
  then read the next click as the end of a menu activation and swallowed it, so the pie needed two clicks before one
  hold worked. The UI no longer sends `ViewportCommand::Focus` on Windows: `platform::raise_own_window` now brings the
  window forward with `SetForegroundWindow` (thread input attached for the call), from the next frame once the window
  is on screen, with no injected key.
- **Docs: Linux is described as X11 and Wayland everywhere.** The README, install guide, user guide, configuration
  reference, known issues, contributing guide, trackpad plan, the website and the package description still said that
  Linux means X11 (and that Wayland only works with XWayland windows). They now say that both session types work, what
  Wayland needs (the udev rule), and that the Wayland layer has not been run on real hardware yet. The user guide
  section is now `#trackpad-macos-windows-and-linux`.

## 0.8.6

- **Linux: no more black flash when the pie or Settings appears.** The window was shown right after it was resized
  (and with whatever its buffer held), so for a frame or two a transparent window was on screen with nothing drawn
  into it at its new size: a black square for the pie, a black rectangle for Settings. It now stays hidden until it
  has its size and two empty frames were drawn at that size (at most 100 ms), and only then is mapped; a pie window
  that is on screen when a held trigger opens Settings is hidden for the resize. New log line `ui.show_wait_done`.
- **Wayland, trackpad: the pointer no longer freezes while the pie is open.** During a trackpad hold the Wayland layer
  grabs the touchpad's whole node (`EVIOCGRAB`) to swallow the extra finger's click and the scroll, but that also took
  the pad's pointer motion away from the compositor, so the pie opened and the cursor could not reach a slice. The
  reader still sees every finger, so while it holds the pad it now moves the pointer itself: the first finger's travel
  goes to a small virtual mouse (`CursorPie virtual pointer`, `/dev/uinput`), 10 px per mm before the compositor's own
  pointer speed (`CURSORPIE_PAD_GAIN` changes it). Without write access to `/dev/uinput` the pad is not grabbed at all,
  so the cursor keeps moving and only the swallowing of the extra click and scroll is lost.
- **Linux, Wayland session: the app starts again instead of failing with `Glutin(NotSupported("provided native
window is not supported"))`.** The window was meant to run through XWayland (only eframe's `x11` feature is asked
  for), but eframe's `accesskit` feature pulls in `accesskit_winit`, whose default features switch on `winit/wayland`;
  winit prefers Wayland whenever `WAYLAND_DISPLAY` is set, so the pie got a native Wayland window that glutin could not
  make an EGL surface for. `run()` now forces winit's X11 backend when `DISPLAY` is set (`force_x11`).

## 0.8.5

- **Open URL slices: new window or the page that is open, with the browser extension that ships in the program.** An
  "Open URL" slice with an `http(s)` link now offers the same two badges as "Open app" (green + = new window, yellow =
  go to the tab that has the page), **only when the browser extension is on** (Settings > Behavior > Browser
  extension, off by default). Without it the slice just opens its link: a browser does not say which pages are open,
  and guessing opened the same page twice. The extension (`assets/browser-extension/`, compiled in with
  `include_str!`; Firefox and Chrome, Edge, Brave, Vivaldi) reports the open `http(s)` tabs; a tab is the page of a
  link by address, then by site and path (redirects, `www.`, query), then by site. CursorPie is its own native
  messaging host (`bridge::host`, started by the browser; no second program), registered per browser by
  `bridge::install` (a file on Linux and macOS, a registry key on Windows), and talks to the running app over a
  token-protected loopback socket (`bridge`). New switches: `--export-extension [DIR]`, `--install-browser-bridge`,
  `--uninstall-browser-bridge`. See `docs/OPEN_URL_CHOICE.md` and `docs/BROWSER_BRIDGE.md`. Not yet run on a real
  browser, and a release needs a signed Firefox add-on to stay installed.
- **The first press after a long idle time is no longer slow, and the listeners keep listening (macOS, Windows).**
  After minutes of doing nothing the pie sometimes took long to appear, or the press seemed lost and had to be
  repeated. These are the causes found by reading the code (not yet reproduced on a machine), each addressed:
  _Windows_ trimmed the whole working set when the UI went to sleep (`SetProcessWorkingSetSize(-1, -1)`), which also
  evicted the code of the mouse hooks and the UI, so the next press page-faulted it all back in; a low-level hook
  that answers too slowly is dropped by Windows. Sleep now compacts the heap (`HeapCompact`), which gives back freed
  memory and leaves what is in use alone. Windows can also remove low-level hooks silently (it is known to after a slow
  callback, and reported after a resume or an unlock) and the message loop then never ended, so the hooks were never
  installed again: a **watchdog**
  notices that the system still sees input while no hook has run, and reinstalls both hooks at once (at most every
  15 s); a watchdog tick that arrives more than 10 s late means the machine slept, and the hooks are then
  reinstalled before the first press. The hook thread runs a little above normal priority, and the process opts out of power throttling (EcoQoS)
  and of the timer-resolution rule for hidden programs, which Windows 11 applies to a program with no window.
  _macOS_ can put the hidden accessory app into App Nap, which stretches timers (the hold timer, the trackpad thread) and
  lowers thread priority; the process now declares a user-initiated activity that still lets the Mac sleep. The event
  tap is now looked at every 2 s and turned on again when macOS switched it off while nothing was calling the
  callback. The trackpad source decided "the Mac slept" with `Instant`, which does not count sleep, so a real sleep
  went unnoticed (the trackpad stayed dead until a restart) while a throttled thread was taken for one and restarted
  a healthy source; it now also compares the wall clock.
  _Both:_ sleep emptied the whole icon cache, which saved a few KiB and made the first pie decode its icons (a
  multi-size TIFF each on macOS) while the hold timer ran. The icons of the general pie now stay decoded
  (`icons_to_keep_asleep`, at most 32 strings). New: `platform::stay_responsive`, `IconCache::keep_only`,
  `platform/windows/power.rs`, `platform/macos/activity.rs`. Not yet run on Windows or on a Mac: check with
  `CURSORPIE_DEBUG=1` (new events `sys/power_throttling`, `sys/app_nap`, `tap/hook` with `restarting` and
  `re_enabled`).
  Icons in a submenu that showed their text first and the app icon a second later: the warm-up on press covered
  only one level of submenus (it now walks every level, shallowest first), and a warm-up miss on a cold system was
  noted as a failure, which kept the pie's own lookup from trying again for 3 s (the pie now hands such an icon to
  the worker thread at once, which notes a real failure).
- **Wayland: adapters for Hyprland, Sway and GNOME (written from their documentation, not run, not compiled where
  written).** Per-app profiles, "app under the cursor" and "bring the app forward" were KDE-only. `wayland/adapter.rs`
  now picks an adapter from the compositor, and every adapter fills the same state as the KWin one, so the rest of
  the program is unchanged. **Hyprland** is read through its control socket (`j/clients`, `j/monitors`,
  `j/activewindow`, `j/cursorpos`); **Sway** through its IPC socket (`GET_TREE`), without a pointer, because Sway has
  no call to read it: the focused app then stands in for the app under the cursor and the report says so; **GNOME**
  through a small extension, `packaging/linux/gnome-extension/`, that pushes the pointer and the window list (no
  titles) to CursorPie's D-Bus service and answers one `Sync` call, and which the user installs and enables (the
  report text gives the steps). Settings and the welcome page name the adapter that is in use. `Session` also finds
  Hyprland and Sway by `HYPRLAND_INSTANCE_SIGNATURE` and `SWAYSOCK` when `XDG_CURRENT_DESKTOP` is empty. Still open:
  COSMIC and niri, a layer-shell reader of the pointer for Sway, and the new spikes SW11 to SW15 in
  `docs/WAYLAND_SUPPORT.md`.

- **Flows can be written by an AI agent.** New `docs/FLOWS_FOR_AI.md` describes the flow JSON for someone who cannot
  open the app: the two places a flow lives (a `.cursorpie-flow.json` file to import, or `flows` in `config.json`),
  every field, the seven block kinds, shortcut key codes and modifier bits, what running a flow does (40 ms between
  steps, commands are not waited for), the repair rules and which mistakes silently change behaviour (a missing
  `Start` leaves nothing to run), a recipe, a checklist and a short validator. `docs/flow.schema.json` is a JSON
  Schema of the file and `docs/examples/flows/` holds examples that the unit tests parse, so the page cannot drift
  from `config::flow`.
- **Trackpad scan, and the trackpad hold is on from the first start (macOS, Windows, Linux).** The trackpad hold
  was off until someone found the switch, although the welcome window already says "touch and hold the trackpad".
  Now `tap::touch::probe` counts the trackpads without starting anything: the Raw Input device list on Windows (the
  same precision-touchpad test the source uses, no registration), `/proc/bus/input/devices` on Linux (readable
  without the `input` group) and `ioreg` on macOS (the private framework stays unloaded). A **new** user who has one
  gets the hold switched on before the tap starts; it is written to `config.json` when the welcome window ends, so
  a quit during the welcome still shows it again. Someone with a `config.json` already keeps their choice. Settings
  has a **Scan for trackpad** button (it switches the hold on when it finds one; Save keeps it) and "Detect again"
  of the Mouse panel counts the trackpads too. The count on macOS (`ioreg` classes `AppleMultitouchTrackpad`,
  `BNBTrackpadDevice`) is not yet run on a Mac.
- **Windows: Settings no longer needs two clicks with a trackpad.** While Settings was open, every pause of the
  finger on the trackpad (aiming at a button) looked like a hold, was answered with "the pie is off", and made the
  window take the focus again. On Windows the toolkit does that by injecting an Alt key press, which turned the next
  click into a menu activation: two clicks, or a very long hold, were needed. A trackpad hold that lands on a Settings
  window that already has the focus is now ignored; one that lands while Settings is behind another window still
  brings it back. A mouse press is answered as before, except that a focused window is not raised again.
- **Wayland: the pie opens on the cursor at fractional scales.** The pie and the Settings card are XWayland windows
  placed in the toolkit's points, while the pointer comes from KWin in its own, and the two differ at a fractional scale
  or with KDE's "scale X11 applications by the system" setting. The position is now mapped from the KWin output it is
  on to the X monitor of the same screen (`wayland::coords`, `platform::window_point`), which is exact whatever either
  scale is; X11 sessions, other systems and screens that cannot be matched keep the position as it was. Not yet run on
  Plasma 6.8; the size of the pie is not converted (see `docs/WAYLAND_SUPPORT.md`).
- **Wayland: the welcome page and the lint.** The second page of the welcome window no longer tells a Plasma 6.8
  user to "log in with an X11 session" (there is none). In a Wayland session it shows the two steps of the udev rule
  and a live status that follows `wayland::report`: **Mouse access** (Allowed / Missing), **Mouse capture** and the
  **KWin adapter** (when it is off, only per-app profiles are, and the page says so); it turns to Ready by itself once
  the rule is in place and the session has been restarted. In an X11 session the page says that a Wayland session
  works too. `uinput::decode_all` uses `as_chunks` (clippy `chunks_exact_to_as_chunks`, Rust 1.88 and later), which
  fixes `cargo tools ci`.
- **Wayland: the rest of the first pass, and the checks pass.** `zbus` is now an ordinary Linux dependency (the
  `wayland-kde` feature is gone: the KDE adapter is always built on Linux). New: while the screen is locked or the
  session is in the background (a VT or user switch) the mouse is given back to the desktop (`logind`, the same
  path as the pause switch); a shortcut is recorded from the Settings window's own key events (no keyboard is ever
  opened, so the udev rule gives no program a way to read what is typed; Super and the order of the modifiers are
  recorded too); the trackpad's extra-finger click and two-finger scroll are swallowed by grabbing the touchpad's
  node while a hold runs (never for more than 15 s); the udev rule also covers touchpads, and `install.sh` installs
  it when it runs as root. The `wayland` log category is in `docs/DEBUGGING.md` and the lint, which fixes
  `cargo tools ci`. Still not run on a Plasma 6.8 machine; see `docs/WAYLAND_SUPPORT.md`.
- **Wayland backend (first code, not yet run on a Plasma 6.8 machine).** In a Wayland session CursorPie
  now grabs the mouse through evdev and writes everything it does not use to a virtual mouse it makes
  with `/dev/uinput` (`tap/wayland.rs`, `platform/linux/uinput.rs`); shortcuts go through a virtual
  keyboard. The state machine, `config.json` and the X11 backend are unchanged, and
  `CURSORPIE_BACKEND=x11` still selects the X11 layer. It needs the udev rule
  `packaging/linux/70-cursorpie.rules`; without it capture stays off and Settings says what to do. The
  KDE Plasma adapter (pointer, windows, app under the cursor, bring an app forward) is a script loaded
  into KWin over D-Bus. Not done yet: a native pie window, adapters for other compositors. See
  `docs/WAYLAND_SUPPORT.md`.

## 0.8.4

- **Flow tab: a node editor for chains of steps.** A new **Flow** section in Settings draws a sequence of actions as
  blocks joined by wires, like the graphs of Unity or Blender. Add **Shortcut**, **App**, **URL / file**,
  **Command** and **Wait** blocks, drag from the dot on the right of a block to the dot on the left of the next one,
  and the flow runs the blocks in that order, from **Start** to **End** (a new flow is Start wired to End; new
  blocks go in before End, and a flow from before End existed gets one after its last block). Drag a header to move
  a block, click a wired input dot to unplug it; blocks that are not wired in are marked and never run. The canvas
  has a dot grid and moves like a node editor: drag the empty canvas, hold the middle button (the trigger button is
  let through over this canvas while Settings is open), or use two fingers or the wheel; **Alt/Option + scroll**,
  a pinch or Ctrl/Cmd + scroll zoom from 30% to 125% (further in the text would turn blurry). The **App** block has
  a **Pick...** button that opens the list of installed apps, like the one in a slice. A slice runs a flow with the
  new **Flow** action (Menu > a slice > Action > Flow). Flows live in `config.json` (`flows`, with `Action::Flow` and
  `Action::Wait`), so they are saved, backed up and undone like everything else. Files from before flows load as
  they were; a config that uses flows cannot be read by older versions.
- **Sharing flows.** **Export** writes `<name>.cursorpie-flow.json` to Downloads, **Import** adds a file as a new
  flow (new id, unique name; the file is checked and limited like a config). A profile file now also carries the
  flows its slices use, so a shared profile keeps working; CursorPie warns when an imported flow or profile holds a
  shell command.

## 0.8.3

- **A recorded shortcut keeps the order you pressed it in.** Pressing Cmd, Shift, Opt and then W was saved and shown
  as "Opt + Shift + Cmd + W": modifiers were stored as a bitmask, so the order was lost and the label always used one
  fixed order. The recorder now follows every modifier press (a `FlagsChanged` event on macOS, the modifier key
  events of the hook on Windows and Linux), saves the order as `"order"` next to `"mods"` in `config.json`
  (left out when it is the old fixed order, so existing configs load and save unchanged), and the label shows it.
  Playing the shortcut presses the modifiers in that same order.
- **Windows: launch at login that survives odd paths.** The installed copy compared its own path with the install
  folder as plain text. A short 8.3 name (`C:\Users\JOHNDO~1`), a junction, or the other case of a non-ASCII user
  name made it believe it was a _different_ copy: it started "the installed copy" (itself) and exited, and so did
  that one, over and over, so nothing ever reached the tray at login. Paths are now compared as the file system
  sees them, and the copy that was handed over to never hands over again (`--handover`; `--autostart` is passed on).
  A zip copy no longer copies itself over an install made by the Setup wizard.
- **Windows: a login entry that cannot work is repaired.** The Settings checkbox now also checks that the program the
  entry names still exists, and the entry is rewritten when it points at a deleted or moved folder or is an old one
  without `--autostart`. A login start waits for the notification area of the taskbar (not only the taskbar window)
  before it creates the tray icon.
- **Windows: app icons.** The icon of an app is now looked up in `App Paths` (per user, per machine, 32-bit view), in
  the Start menu shortcuts and in the program folders, not only among the running programs and `PATH`, so apps that
  are not running show their icon. A program picked with the file chooser is remembered. The icon itself comes from
  the shell (`IShellItemImageFactory`, 256 px with alpha, the one Explorer shows) with the old calls as the fallback.
- **Windows: the app list.** Settings lists the apps of the Start menu (by their shortcut names) plus those under
  `App Paths`. It reads the registry and the shortcut files directly instead of the text of `reg.exe`, which is
  localized and in the OEM code page, so it also works on non-English Windows and with non-ASCII paths.

## 0.8.2

- **Settings comes forward with the trackpad trigger too.** With Settings open behind another window, a press of the
  mouse trigger brought it back but a trackpad hold did nothing. The hold now does the same as the press.

- **Windows: the install step asks first.** The first start of an unzipped copy no longer copies itself, registers
  a startup entry and starts the copy without a word (which behavior-based antivirus engines judge as a dropper);
  it asks once. "No" is remembered. Updating an app that is already installed stays silent.
  `scripts/sign-local.ps1` signs a build with a self-made certificate for your own PCs.

- **The app icon is the same artwork on every system.** macOS had the Windows icon as a hard-edged full-bleed square
  with only four sizes; `app-icon.icns` now has every size from 16 to 1024 px (with the @2x ones) in Apple's rounded
  shape with its margin and shadow. Linux had no window icon at all and a single 512 px file: the window now carries
  its icon (taskbar, Alt+Tab), and the package installs 16 to 512 px into `hicolor` and refreshes the icon cache.
  Windows is unchanged.

- **Login start and antivirus.** Windows: the startup entry now passes `--autostart`, and such a start waits for the
  taskbar (at most 90 s) before it creates the tray icon and the hooks, so the tray icon is no longer lost at login;
  the program no longer rewrites its registry entries at every start; the `.exe` has version information and a
  manifest (`asInvoker`); the Setup wizard has version information too. macOS: the LaunchAgent starts the app with
  `open -a` instead of the bare binary and refuses a path Gatekeeper copied temporarily (App Translocation); the
  ad-hoc signature is tied to the bundle id so Accessibility is not forgotten after a rebuild. Linux: the autostart
  entry waits 5 s so the display is ready. Antivirus programs can still object to an unsigned build: see
  KNOWN_ISSUES.md.

- **Fix: the virtual trackpad feels like a trackpad.** The pointer's speed was worked out from the time between two
  frames of the window, which can be one millisecond: a burst of frames made the pointer jump and stutter. The speed
  is now averaged over time (a frame weighs as much as it lasts), the gain is a smooth curve (3.5 points per
  millimeter when slow, up to four times that in a flick), a long gap is no motion, and the arrow's window sits on
  whole points so it no longer trembles.
- **Fix: the mouse buttons of the virtual trackpad did nothing.** A click on the glass only made the surface sink; no
  click ever reached the app under the virtual pointer, and a click made while the pie was open was not the extra
  click either (the virtual trackpad has one finger, the extra click wanted two). Now the left and right buttons are
  the primary and the secondary click of the trackpad: when the button comes up, a click is made at the virtual
  pointer (`platform::click_at`; the system's pointer goes there and back at once, on macOS the event carries the
  place), with the click count of a double click. A press that belongs to the pie (the pie was open, or opened while
  the button was held) is never a click for the app, and while a pie of the virtual trackpad is open a click is its
  extra click. Holding the button and moving off the glass no longer lifts the finger, and a moment without a
  pointer (the click's trip) no longer lifts it either. Not yet: dragging with the button held.

- **Virtual trackpad (developer tool: `cargo tools trackpad`).** Starts the app with a trackpad that floats exactly in
  the middle of the screen, to try the trackpad gestures with a plain mouse. Your real pointer is the finger: while it
  is over the glass it touches, leaving the glass lifts it. A virtual pointer, drawn as an arrow on the screen outside
  the trackpad, is what the finger moves, relatively and with acceleration like a real trackpad (the surface counts as
  160 x 100 mm, so a slide of so many millimeters moves it so far). The pie opens at the virtual pointer, aims with it
  and acts on the app under it; a hold opens it, lifting the finger runs the slice, and a click of the mouse is the
  extra click. It feeds the same recognizer as a real trackpad (macOS, Windows and Linux alike). Its feel stands in
  for the haptic click a mouse cannot give: the surface sinks at a click, a ring spreads, the frame glows and a short
  click sounds; the ticks of the pie are shown the same way and are also sent to the haptic engine, which a finger on
  a real trackpad feels. A slider in the lower frame sets the size and a switch the click sound. It is behind the Cargo feature `vtrackpad`, which the command turns on: an ordinary build has none of its code. Options:
  `--width POINTS`, `--silent`, `--release`. Nothing is written to the settings. While the trackpad is open the main
  window is parked (1 x 1 point, click-through) instead of hidden, which keeps the trackpad's own windows alive.
- **Fix: messages from the tap could wait for the wrong window.** The tap woke the UI with a plain
  `request_repaint`, which means "the window drawn last". With a second window (the virtual trackpad) that could be
  the wrong one, and a pie that had been released stayed on the screen. The wake-up now asks for the root window.

- **Fix: the pie ran actions behind an open Settings window.** The gesture state machine of the tap did not know
  that Settings was open: a pie that was not drawn still opened, hovered and, on release, ran the slice under the
  pointer (opening apps and so on), and the side buttons ran their actions too. While Settings is open a press of the
  trigger now starts no gesture (it only brings Settings forward and says why), the side buttons are left to the app,
  and a pie that was open when Settings opened runs nothing when it closes.
- **While a pie is open, no click reaches the app under it, for the mouse and the trackpad, on all three systems.**
  Each path handled its own buttons (the trigger, the right-click switch, mapped side buttons, the extra finger) and
  every other press fell through: a left click, a right click with the switch off, a middle or extra button no mode
  uses. The app under the pie got a click that was aimed at the pie (on Linux a kept press was even sent again at its
  release). Now, as the last step of the state machine, a press while a pie is open is swallowed with its drags and
  its release, even when the pie closed in between; the wheel is swallowed too while a mouse pie is open. Every press
  decides afresh, so a lost release never leaves the next click stuck. New `tap` log event `pie_click_swallowed`
  (`button`). The macOS and Windows taps read the left button and the wheel for this only while a pie is open.

- **A trackpad pie no longer lets the pointer's movement through to the page below.** A mouse hold has a button down,
  so the apps under the pie hear no motion. A trackpad hold has none: the finger moves the real pointer, and the pie
  window was click-through, so every move reached the app under it (a video player showed its controls, links
  highlighted). The pie window now takes the pointer while a trackpad hold runs, and is click-through again when the
  pie closes; a mouse pie is unchanged. New `ui` log event `pie_input` (`takes_pointer`).

- **No more scrolling leaks under a trackpad pie, and the pie stays smooth (macOS, Windows, Linux).** The two-finger
  scroll that the holding finger and the extra finger make was swallowed only while the hold ran. The gesture ends
  the moment a finger lifts, but the scroll does not: its last deltas and its glide (momentum on macOS and on Windows
  precision touchpads) arrived after that and reached the app under the pie, which then scrolled by itself, and an app
  that sees the middle of a scroll it never saw begin jumps and stutters. Now a scroll that the pie began to swallow is
  swallowed to its end: after a pie that opened closes, wheel events keep being swallowed for 300 ms, each one extends
  that by 120 ms (a glide is eaten to its last event), and never beyond 1.2 s. The tail ends at once when a new finger
  lands, so a scroll of your own is never eaten, and a hold that never opened (a tap, a slide, a second finger that came
  early) starts none. On Linux the wheel is grabbed for the same time, on its own (clicks pass during the tail), and a
  wheel press no longer asks the X server where the pointer is. New `touch` log events: `scroll_tail`,
  `scroll_tail_end`, `scroll_swallowed` and, with `trace`, `scroll_drop` (counts and reasons only).

- **Faster, quieter defaults.** The pie opens after a hold of 150 ms by default (was 500 ms) and the hold can be as
  short as 50 ms (was 150 ms; the Settings slider and the file limit both go down to 50). The default pie shows
  icons only, without the name under each icon (`show_labels` is `false` for a new config; a saved config keeps its
  value, and a file that does not have the field keeps the names). The welcome guide says "150 ms" instead of a
  rounded "0.1 s". The website demo pie shows icons only too, and its text says 150 ms.

- **A mouse now aims a pie that a trackpad hold opened.** Moving the pointer with a mouse while the finger kept
  still did nothing, because the pie only heard the finger move (a mouse press has drag events, a trackpad hold has
  none). While a trackpad hold runs, the pointer is now watched every 10 ms on its own thread, so the mouse, the
  trackpad and a mix of both hover the slices, and lifting the finger runs the slice under the pointer. Nothing runs
  while no hold is running.

- **The two-finger trackpad hold is gone, on macOS, Windows and Linux.** Two fingers on a trackpad are what the
  system scrolls with, so the pointer did not move and the pie had to be aimed by the fingers themselves, which was
  never reliable. A hold is now always one finger, with one more finger for the extra click. Removed: the _Fingers to
  hold_ setting, the config field `touch_fingers` (an old file that has it still loads; the field is ignored and
  dropped at the next save), the code that aimed the pie by finger travel, and on Linux the X11 pointer grab that
  went with it. Scrolling is still swallowed while a hold runs, because the extra finger makes two.

## 0.8.1

- **Linux touchpad reader builds on Windows and macOS again, and platform checks follow the project rules.** The
  Linux reader's kernel calls (`ioctl`, `poll`, opening `/dev/input` nodes) moved from `tap/touch/evdev.rs` to
  `platform/linux/evdev_io.rs`, so `cargo clippy --all-targets` and `cargo test` no longer fail on Windows. Code
  outside the platform layer asks `platform::OS` instead of using `cfg!` (app icon names, settings window resize,
  icon worker count, the hidden pie window's taskbar button and window class). Dead code is reported on Linux
  again. No change in behavior for users.

## 0.8.0

- **README, website and docs now say that the trackpad works on all three systems.** The README (tagline,
  highlights, "Try it", platform table and a new _Trackpad_ section), the website (description, hero, steps, feature
  card, platform cards and FAQ), `docs/INSTALL.md` (a new _Trackpad hold_ section with the `input` group on Linux) and
  the plan's title and summary describe trackpad hold on macOS, Windows (precision touchpad) and Linux (X11).

- **Trackpad hold and haptics on macOS (off by default).** `docs/TRACKPAD_HAPTICS_PLAN.md` is implemented: with
  _Settings > Behavior > Trackpad_ on, touching and holding opens the pie, moving the finger hovers a slice and
  lifting runs it; one more finger that clicks or taps switches the mode (or picks the hovered mode, or does
  nothing); the pie ticks on a Force Touch trackpad. Mouse and trackpad are one input: the same state machine
  (`Gesture` now records its `Source`), the same timings and profiles. New config fields `touch_trigger`,
  `touch_fingers`, `touch_still_mm`, `touch_click_action`, `touch_typing_guard_ms`, `haptics`, `haptic_hover`
  (an old file loads unchanged). The touches come from Apple's private `MultitouchSupport.framework`, loaded at run
  time only while the feature is on and checked before it is trusted. The recognizer and the rest of the core are
  tested on every OS; the macOS layer has not been run on a Mac yet. The `touch` category is added to the diagnostic log.

- **Trackpad hold on Linux (X11), off by default.** The same feature as on macOS and Windows, for a touchpad under
  X11: hold to open the pie, move to a slice, lift to run it, one more finger to click. The fingers are read from
  `/dev/input/event*` (libinput hides them from X), only while _Open the pie with a trackpad hold_ is on, which needs
  the `input` group; Settings says so when it is missing. While a hold runs the left and right buttons are grabbed so
  the extra click never reaches the app (any other click is sent again at its release), and the typing guard listens
  for key presses (not which key). No haptic ticks on Linux. The reader repeats the last frame every 20 ms because
  resting fingers send no events. The parser, device detection and grab logic are tested; the device thread has not
  been run on a real touchpad yet. New `touch` log events: `device_open`, `device_closed`, `x_grabs`. **`cursorpie --touchpad-debug [seconds]`** checks the
  chain (session, saved switch, `input` group, why each device was taken or left out, permission, surface size) and then
  shows the fingers and the recognizer's decisions live, ending with what to fix.

## 0.7.7

- **A quieter top bar in Settings.** Undo and Redo buttons are gone (Ctrl/Cmd+Z and Ctrl/Cmd+Shift+Z still
  work). Cancel and Save are two small round dots in the corner, in the sage theme: red (the theme's "bad"
  color) cancels and the accent color saves and closes. A third, ochre dot shows between them while there are
  unsaved changes. Their icons appear on hover.

- **Ask before throwing changes away.** Cancel, Esc and the window's close button now open a "Discard changes?"
  sheet when something is not saved. Enter discards, Esc or a click outside keeps editing. With nothing unsaved
  they close at once as before.

- **Ctrl+S / Cmd+S saves.** It writes the config (with its backup) and keeps Settings open; the unsaved dot
  goes away and the footer says "Saved.". The saving code moved to `App::persist_edit`, shared with the
  Save dot.

- **Profile actions as a row of icons.** In Menu, "..." now unfolds Rename, Export, Import and Delete as icons
  to its left, hides "+" meanwhile and turns into an X that folds them again. The text menu is gone; names
  are tooltips.

- **A tidier About card.** Website, GitHub, Report an issue and Downloads are round icon buttons in the top
  right corner of the card instead of a row of text buttons, and the tagline has room below it.

- **A plan for trackpad and haptic support on macOS.** `docs/TRACKPAD_HAPTICS_PLAN.md` describes how a hold on a
  trackpad can open the pie, how an extra finger can switch the mode, how haptic feedback can be added and how
  mouse and trackpad stay one input. It is a plan: nothing in it is implemented yet.

- **Documentation brought in line with the code.** `docs/CONFIGURATION.md` lists `app_icon_shadow`, `appearance`,
  `pie_style` and the three `glass_*` fields and gives the real default accent (`[90, 120, 94]`); `docs/USER_GUIDE.md`
  describes Appearance and Pie material and the per-system quick actions and default profiles; `docs/INSTALL.md`
  says the welcome window opens on Linux too; `CONTRIBUTING.md` lists the `diag-lint` CI step and the options of
  `clean-logs` and `zip`; the release text no longer says that only "stable" tags publish assets, which the
  workflow does not check.

- **The name in Isometra.** "CursorPie" is now set in Isometra Regular, the lettering of the logo, in the
  About tab, the welcome window and the fallback wordmark. Isometra is SIL OFL 1.1 and is listed under
  "Built with" in About.

- **A livelier welcome window.** The first page has a banner with the name, the version and rings that pulse
  around the logo, and the three steps (Hold, Flick, Release) are numbered cards that light up one after the
  other. Pages fade in, the page dots stretch, and pages 2 and 3 say "Step 2 of 3" and "Step 3 of 3". Pages 2
  and 3 use two columns, so nothing is left empty or cut off, even on the smallest card.

- **Fixed: the links in About did nothing on Linux and Windows.** "GitHub repository", "Report an issue",
  "Downloads", the email link and the links under "Built with" used egui's `open_url`, which does nothing when
  eframe is built without its `web_browser` feature, as it is on these two systems. They now use
  `platform::open_url` (`xdg-open`, `ShellExecute`, `open`). The diagnostic log gets `ui/open_link` and
  `act/open_url`.

- **Sliders with anchors.** Every slider in Settings shows marks and round numbers under its rail and snaps to
  them, so a round value is easy to land on. Hold Shift to move freely; the arrow keys move one step.

- **A simpler profile toolbar in Menu.** The profile picker is a chip with the icon of the profile's app and a
  list of every profile with its icon and number of apps. The apps button shows the real icons of the apps.
  New, Rename and Apps open as dialogs in the middle of the window instead of small popups, and Rename, Export,
  Import and Delete moved into one "..." menu. In the Apps dialog every app is a tile with its icon; hover a
  tile to remove it.

- **Choose an app from a list, the same on every system.** Where Settings asked for an app through the
  system's file chooser, it now shows the installed apps with their icons and a search box. The list comes from
  the desktop entries on Linux, the `.app` bundles on macOS and `App Paths` on Windows. "Browse files" is still
  there for an app that is not listed. The diagnostic log records `installed_apps` with the number of apps found.

- **Shadow under the app's icon.** The icon in the center of an app's pie gets a soft shadow on a plain light
  center, so it stands out from the white. Behavior > App icon in the pie > Shadow under the icon lets you
  choose Automatic (the default), Soft, Strong or None. A saved config without the setting gets Automatic.

## 0.7.6

- **Pick... of an Open app action uses the same app list as Add app.** The button now opens the searchable list
  of installed apps, with icons, that **Add app** in the profile's apps already used. **Browse files...** at the
  bottom of the list is still there for an app that is not listed. A slice still takes the icon of the app you
  choose, as before. The list stores the name of the app on macOS and its id on Windows and Linux.

- **The Menu tab pie follows "Show labels".** With the names off in Behavior, the pie of the Menu tab shows icons
  only, like the real pie.

- **Drag slices in the Menu tab.** Drag a slice onto another one to move it there, or onto the trash button of the
  editor to delete it (the trash turns red when the slice is over it). The "+" slice in the pie is gone: **+** now
  sits next to the trash under the editor, and in the empty editor when no slice is selected.

- **A slice that opens an app can wear that app's icon.** Select an **Open app** slice in the Menu tab and a new
  row, **Icon of this app**, offers three looks: **Full color** (the app's own icon), **White** (a white glyph, for
  dark and glass pies) and **Black** (a black glyph, for light pies). Choosing an app with **Pick...** puts its
  color icon on a slice that still has the starting icon, and keeps the look of a slice that already shows an app's
  icon. In `config.json` the icon of the slice is `appcolor:<app id>`, `app:<app id>` or `appblack:<app id>`.

## 0.7.5

- **About links to the CursorPie website.** A **Website** button sits first among the links at the top of About,
  and the **Project** panel has a **Website** row with the address.

- **Add and delete a slice sit under the pie, each in its own circle.** They used to be in the far corner of the
  editor, a long way from the pie. Now two round buttons, **+** and the trash, are right under the pie. The trash
  deletes the selected slice (it is grayed out while no slice is selected) and is still where you drop a dragged
  slice to delete it; it turns red while a slice is over it. The editor keeps only Move left and Move right.

## 0.7.4

- **CursorPie looks like its website.** Settings, the welcome window and the pie now use the colors of the site: a
  cream page with paper-white panels and sage-green neutrals (and graphite with a faint sage tint in the dark
  appearance, so the accent still stands out). A new **Sage** accent is the first swatch and the default of a new
  install; the other accents are still there, and a saved accent is not changed. The logo is now the real artwork,
  with the name in the sidebar, drawn in near black on the light appearance and near white on the dark one. Links and
  the version badge in the accent are lightened or darkened until they can be read.

- **App icons on Linux.** The pie can now show the real icon of an app in the middle of its own pie, in
  both styles of **App icon in the pie**. CursorPie reads the `Icon=` of the app's desktop entry and finds it in
  the icon themes (the user's theme, the ones it inherits from, `hicolor`, `pixmaps`, Flatpak and Snap). PNG
  icons need nothing more; an icon that is only an SVG needs `rsvg-convert` (`librsvg2-tools`) and is skipped
  without it. Roles such as `@browser` use the icon of the app that fills them.

- **Choose how the app's icon looks in the middle of an app's pie.** Behavior has a new panel, **App icon in the
  pie**: **One color** (the silhouette in the ink of the pie, as before and still the default) or **Full color**
  (the app's own icon in its own colors). A small animation shows the icon flying into the pie and either keeping
  its colors or losing them. The setting is `app_icon_style` in `config.json`; it only applies while **Show the
  app's icon in the center** is on. The icon in the **Look & feel** animation follows the choice too.

- **The "Stop waiting" button is now a red circle with an X.** It sits under the "Waiting for the file chooser..."
  pill and shows "Stop waiting" as a tooltip when the pointer rests on it.

## 0.7.3

- **Linux: cancelling the file chooser ends the wait.** After Cancel or the close button, CursorPie could keep
  showing "Waiting for the file chooser..." because it waited for the chooser's output to close, which a helper
  the chooser leaves behind can hold open. It now waits for the chooser itself to end. The waiting screen also has
  a **Stop waiting** button for a chooser that never answers or is lost behind other windows.
- **macOS: the file chooser is handled the same way.** `osascript`, which shows the Finder's chooser, now goes
  through the same code as zenity and kdialog on Linux: CursorPie waits for the chooser itself to end, not for
  its output pipe to close. **Windows** needed no change: its chooser returns the moment you press Cancel or the
  close button. The dimmed "Waiting for the file chooser..." screen and the **Stop waiting** button are the same
  on every system.
- **Settings waits calmly while a file chooser is open.** Choosing an app, an icon image or a config file opens
  the system's own chooser beside Settings, and Settings stayed live under it: a second click opened a second
  chooser, Esc closed Settings, and a press of the trigger pulled Settings over the chooser. Settings is now
  dimmed with "Waiting for the file chooser..." until the chooser closes, then comes back to the front. **Import...**
  now hands the focus back too.
- **Linux: pressing the trigger while Settings is open brings it forward again.** With Settings open on another
  workspace, or behind the app you were using, a press (or holding the trigger in the middle of the pie) only made
  its taskbar button blink: the window manager refuses the focus request the toolkit sends. CursorPie now asks the
  way a pager does, goes to the workspace Settings is on and raises it, as it already does on macOS. If it still
  does not work on your desktop, start with `CURSORPIE_DEBUG=1` and run `cargo tools diag`: it says whether the
  window manager refused the request.
- **A website for CursorPie.** The repository now has a landing page (`index.html`) that detects your system and
  links the matching download, and shows the project's Markdown documentation (install guide, user guide,
  configuration, known issues, this changelog) in the browser, read straight from the files in the repository. It
  is published to GitHub Pages by `.github/workflows/static.yml` on every push to `main`. See `README-WEBSITE.md`.
- **The project now lives at `github.com/sebastvnd/cursorpie`.** The clone and download links in `docs/INSTALL.md`
  and `CONTRIBUTING.md` point there, and the PNG logos and sample icons in `icon/` are no longer ignored by git.
- **Windows: the build works again.** Asking the window manager for a workspace (Linux code that is also built and
  tested on every system) converted a `u32` with `c_long::from`, which does not exist where `c_long` is 32 bits
  wide, so the Windows build failed. It is now a plain cast, with a test that the workspace number survives it.

## 0.7.2

- **More settings that show what they do, and bigger demos.** The animated demos have a taller stage (190 points
  instead of 140), so the pies, the mouse and the captions are no longer cramped. Four panels got a demo of
  their own: **Look & feel** (the pie at the size you set, with or without names and the app's icon, the accent
  on the slice under the pointer, solid or glass), **Side buttons** (a side-button press runs the pie they
  follow; the ring colors and the right-click switch show which one that is), **Memory** (the idle timer runs
  out, the icons and the editor's copy are dropped, the trigger loads them again) and **Mouse** (each extra
  button is pressed, heard and listed, then Done saves them).
- **Glass pie is always dark.** The glass pie now keeps its smoked graphite look in the light appearance too.
  Pale glass over a light page looked washed out and nothing like Liquid Glass. Solid still follows the
  appearance, and so does the rest of Settings.
- **Icons only.** A new switch in Settings, **Show names under the icons** (Behavior > Look & feel), hides
  the names in the pie so each slice shows just its icon, a little larger. It is on by default; the names stay
  in Settings, so you can still tell the slices apart while you edit them. Old config files keep the names.
- **Settings that show what they do.** On **Behavior**, the panels _Trigger_, _Pie of the app in front_, _App
  detection_ and _Other Spaces / desktops / workspaces_ now start with a small animation instead of only a
  sentence: the button going down and the pie opening after your hold time, which pie a hold and a double-click
  open, which window the pie works on, and what "Open app" does to an app on another desktop. The animation
  follows the choice and the sliders as you change them, and it only runs while you can see it.
- **A calmer footer in Settings.** The notice that the pie is off while Settings is open, and every other short
  message in the footer, now use the small grey text of the shortcut hint. "Open System Settings" is a plain text
  link with an arrow instead of a boxed button next to a red pill.

## 0.7.1

- **Linux: "Open app" no longer starts a second copy of an app that is already open.** The slice compared what
  you typed (`firefox`, `gnome-terminal`, a path) with the one name the window manager gives the window
  (`org.mozilla.firefox`, `org.gnome.terminal`), found no match and started the app again. It now compares every
  name a window has (its class, its instance and the desktop entry's id, window class and program), so the
  existing window is raised, on whichever workspace it is.
- **Linux: the Files, Terminal and Browser slices raise the window that is open.** They were shell commands, which
  always start a new copy (a new Firefox window on every click). They are now "Open app" slices that name a role:
  `@browser`, `@files` or `@terminal`. CursorPie asks the system which app that is (`xdg-settings`, `xdg-mime`, or
  the first terminal it finds), then raises its window or starts it. Your existing `config.json` is upgraded
  when it is loaded (the file itself changes at the next save): only the three unchanged default commands are
  replaced, your own commands stay. You can
  type these roles in any "Open app" slice. If the system cannot name the app, the old command runs.

## 0.7.0

- **Quit asks before it quits.** Pressing **Quit CursorPie** while Settings is open (sidebar, General, or the
  tray / menu bar icon) opens a sheet instead of quitting at once. With nothing unsaved it asks "Do you really
  want to quit?" (**Cancel** / **Quit CursorPie**). With unsaved changes it says so and offers **Keep editing**,
  **Save and quit** and **Quit without saving**. Esc, the close button and a click outside mean "not yet". A
  quit from the tray while Settings is open brings the window to the front to ask; with Settings closed there
  is no window, so it quits at once. Cancel, Esc and the window's close button still discard edits without
  asking, as before.
- **Pressing the trigger while Settings is open now shows it.** The pie is off while Settings is open, and a
  Settings window hidden behind other windows made the press look like it did nothing. The press now brings
  Settings to the front (also from minimized), asks the system to draw attention to it and says in its footer
  why the pie did not open.
- **A new look: modern flat, light and roomy.** Settings, the welcome guide and the pie are redrawn with large
  rounded cards, pill buttons, more space, a full-height sidebar with icons and a calmer palette. The default
  accent is indigo, the color of the logo (an existing `config.json` keeps the accent it has).
- **One kind of popup.** Adding a slice and choosing an icon are sheets over a dimmed window; New, Rename,
  All apps and Share are popovers under their button. All close with Esc or a click outside.
- **Inter and Phosphor.** The interface uses the Inter font and one icon set everywhere, both embedded in the
  binary (about 340 KB, MIT and OFL licenses, see About). Slice icons are named `ico:<name>` in `config.json`;
  the letter icons of older files (`C/V`, `Tab`, `>_`...) are shown as the matching picture.
- **The pie is light** to match Settings; the hovered slice and its icon follow the accent.
- **Light and dark.** Settings and the pie have a dark appearance. Behavior, Look & feel offers System (the
  default), Light and Dark, and the window changes as soon as you choose.
- **Glass pie.** Behavior, Pie material: the slices can be frosted glass, with sliders for Clearness, Frost and
  Shine and a preview. The frost is drawn, not a blur of the desktop behind the pie.
- **Settings layout.** The page is centered instead of leaving an empty strip on the right, and the sidebar
  folds to icons (button next to the title, or Ctrl / Cmd + B). Settings now opens folded (compact) by default.
- **Calmer choices.** The pills are gone: a choice between a few options (Appearance, Pie material, slice kind)
  is a segmented control, the kind of an action is a drop-down with icons, the icon picker's groups are icon
  tabs sharing one line, and its Image / Folder / Reload buttons are one "+" menu.
- **Smoother glass.** The glass pie is painted as continuous gradients instead of five flat bands and a
  polygon outline: the frost, the sheen and the rim are one smooth field, and the rim catches a light from the
  top left like Apple's Liquid Glass. The pie's shadow is a single blurred shape.
- **Drop-down lists and popups** share the look of the rest of Settings (they were drawn in egui's dark style,
  with black text fields).

## 0.6.1

- **macOS: "Open app" now opens the app on the screen you clicked.** With two screens, an app that already had
  a window on the other screen (Finder, say) was taken for "already here", so the slice only focused the window
  on the screen you were not looking at. Now, with more than one screen, the app's front window on the screen
  under the pointer is raised and given the focus; when the app has none there, its front window is moved to
  that screen, centered, and raised. Full-screen windows, apps without a window, single-screen setups and "Do
  nothing" behave as before; when a window cannot be moved the app is opened as before. Uses Accessibility only
  (no private calls).

## 0.6.0

- **Side-button slices can use every button of the mouse, not only back and forward.** A slice set to "Side
  buttons" now lists one more action for each extra button of your mouse (button 5 and up). The new `extra` map
  of a `Mode` in `config.json` stores them; files without it load as before, and a mode without extras is written
  exactly as it was. Buttons nothing is set for still reach the app.
- **CursorPie knows your mouse.** A new `mice.json` (next to `config.json`) keeps a profile for every mouse it has
  met, keyed by its USB or Bluetooth id: the buttons the system reports and the ones you scanned. Every five
  seconds it looks at what is connected and tells a new mouse from one it already knows (a note in Settings). On
  Linux the buttons come from `/proc/bus/input/devices`; on Windows from the number of buttons; macOS has no list
  CursorPie can read, so scan the mouse there.
- **Settings, General, Mouse:** "Scan this mouse" (press every extra button once; the buttons do nothing else
  meanwhile), "Detect again" and a list of known mice with "Forget".
- **Linux:** extra buttons are numbered like the other systems (X button 10 and up are button 5 and up) and are
  grabbed only while a mode uses them.

## 0.5.4

- **More tools to read the diagnostic log.** `cargo tools log` has seven views: `summary` (what was written and what
  floods the log), `timeline` (one row per press with the hold, UI and window times), `grep` (readable lines of one
  category or event, no `jq`), `slow` (slow timed steps), `diff` (what a fix changed between two logs), `follow` (a
  run that is still going) and `report` (facts and verdict for a bug report). `cargo tools diag` now also reports a
  slow pie opening, slow steps, a config that could not be saved and an action stuck in the queue.
- **`cargo tools diag-lint` keeps the log and the analyzer together.** It checks every log line in `src/` (known
  category, `snake_case`, no field named like user data, no repeated field) and that each event the analyzer reads is
  still written with its fields. It runs in `cargo tools ci --quick` and as a unit test.
- **More log points where bugs hide:** whether the input hook runs (`tap` / `hook`: a missing macOS permission or a
  Windows refusal is now in the log), why a hold did not open the pie (`hold_cancelled`), the double-click window and
  replayed clicks, which profile a press resolved to, which slice was selected, the side buttons, opening and closing
  Settings, config import, the icon warm-up and the sleep (timed), and every program an action starts (name only,
  never its arguments): whether it started, and how it ended. `cargo tools diag` turns the failures among them into
  findings.
- **New log points:** `cfg` (loading and saving `config.json`, so a failed save is no longer silent), `act` (kind,
  queue wait and run time of every action, never its content) and `tap` / `on_release` (every backend).

## 0.5.3

- **The diagnostic log is its own crate.** The code that writes the `CURSORPIE_DEBUG=1` log moved out of the
  app's sources (`src/diag.rs`) into `diag/`, a crate of the workspace with no dependencies; the app only keeps
  a small glue module (`src/debug.rs`). Log lines are now `diag::log!("ui", "show_pie", x, y = y, kind = ?kind)`:
  a bare name logs the variable, `?` logs the `Debug` text. New: `diag::trace!`, `diag::once!`, `diag::timed!`,
  `diag::event(..)` for names known only at run time, and `diag::Watch` (what was asked for against what
  happened). The switch accepts `all,-tap` (leave a category out) and `stderr` (also print on the terminal).
- **The diagnostic log is not in release builds.** It is part of debug builds only; an ordinary release build has
  none of its code. `cargo build --release --features debug-log` keeps it, for collecting a log on someone else's
  machine.
- **New developer tools:** `cargo tools clean-logs`, `cargo tools zip` (the project without `target/`) and
  `cargo tools info` (facts for a bug report).

## 0.5.2

- Diag code for developer
- Fix bug pie position in x11
- Fix bug pie position in linux
- New documentation and repair memory management

## 0.5.1

- **Fix bug many display on macOS**
- **Lower memory use in Settings.** The icon picker only draws (and only loads) the icons that are on
  screen, and loads a few per frame instead of all at once. Image icons larger than 4096 px on a side are no
  longer decoded, and a decoded image no longer exists in several full-size copies while it is scaled down.
  The "Add a slice" tiles are built once when the panel opens and freed when it closes. Importing a config
  reads at most 4 MiB.
- **Big images are shrunk when you choose them.** Choosing an image larger than 512 px or 512 KB as a slice icon
  saves a 256 px PNG copy in the icons folder and uses that (the original is untouched), and the status bar
  says what was done. An image that cannot be read, or has more than 40 megapixels, is refused with a message.
  The "Mine" page tells you when images in the icons folder are not listed because they are too big.
- **Fix bug pie draw center position.**
- **Add debug/diag tools.**

## 0.5.0

- **Open app follows the app to its desktop or Space.** An _Open app_ slice for a program that is already
  running on another virtual desktop (Windows) or Space (macOS) now takes you there and brings its window to
  the front, instead of leaving you where you were. **Settings > Behavior > Other desktops / Other Spaces**
  chooses: _Switch to the desktop where the app is_ (the default), _Do nothing_ (open the app on the current
  desktop as before) and, on macOS, _Bring the app's window to this Space_ (experimental: it relies on private
  system calls and falls back to switching when macOS refuses them). On macOS the switch works with the
  Mission Control shortcuts Control+Left / Right (on by default) even when the system setting _When switching
  to an application, switch to a Space with open windows for the application_ is off. If the app is not
  running, or has a window on the current desktop, nothing changes. On macOS the nearest Space with a window of
  the app is gone to directly, left or right and however many Spaces away, without stopping in between, and
  the window is raised and checked to be in front. How it works: `docs/OTHER_DESKTOPS.md`.
- **Linux (X11).** CursorPie now runs on Linux in an X11 session: the hold, flick, release and double-click
  gestures, per-app profiles (apps are recognized by their desktop-entry id, for example `firefox` or
  `org.mozilla.firefox`), side-button modes, shortcut recording, launching apps, links and commands, and
  "Launch at login" through an XDG autostart entry. No permission is needed. The mouse is watched with
  XInput2 grabs and shortcuts are sent with XTest. `scripts/package-linux.sh` builds a tarball with an
  installer. Building needs the development files of libx11, libxi, libxtst and libxrandr (see
  `CONTRIBUTING.md`). The design and what is and is not built are in `docs/LINUX_MIGRATION.md`.
- **Linux: Open app follows the app to its workspace.** The window manager does the switching: an _Open app_
  slice for a program that is already running asks it to activate the app's window, and the usual window
  managers go to the workspace that window is on. **Settings > Behavior > Other workspaces** offers _Switch_
  (the default) and _Do nothing_; _Bring the window_ does not exist on Linux and acts as _Switch_. An app that
  has no window is started as before. How well this works depends on the window manager and has not been
  checked on real desktops.
- **Linux: defaults and files.** Built-in shortcuts use Ctrl and the Super key (instead of Cmd), and redo is
  Ctrl+Shift+Z. The Launch and quick-action slices run shell commands that work on every desktop (a file
  manager through `xdg-open`, the first terminal and screenshot tool that is installed, `loginctl
lock-session`, Super+. for the emoji picker). Settings and backups are kept in `~/.config/cursorpie` (or
  `$XDG_CONFIG_HOME/cursorpie`), and exports go to your download folder as `user-dirs.dirs` names it, for
  example `~/Unduhan` on an Indonesian system. `config.json` keeps its format, so a file from macOS or Windows
  still opens (app ids from another OS are kept but do not match).
- **Linux: what is missing.** Wayland sessions only work for X11 and XWayland windows: log in with an X11
  session for full support. There is no tray icon yet, so Pause and Quit are in **Settings > General** and
  Settings opens by holding the trigger in the middle of the pie. Also missing: app icons in the pie,
  keyboard-layout labels on non-US layouts, and extra mouse buttons as the trigger (only the middle button).
  Handing a **drag** back to the app while the middle button is held (for example middle-drag scrolling) may
  lose the start of the drag; a quick click is passed on reliably. The file chooser buttons need `zenity` or
  `kdialog`.

## 0.4.2

- **Welcome window.** The first start on macOS and on Windows opens a three-page welcome window in the
  Settings style: how the gesture works, the step that is needed next (macOS: allow Accessibility, with a live
  status; Windows: where the tray icon is) and a "Launch at login" box. It is shown once (the empty file
  `welcome_shown` next to `config.json` remembers it), never to someone who already has a `config.json`, and
  **Welcome guide...** in the menu bar / tray menu opens it again.
- **Windows: Setup wizard.** `scripts\package-windows.ps1` also builds `CursorPie-<version>-setup.exe` when
  Inno Setup 6 is installed (`installer/cursorpie.iss`): a per-user installer (no administrator rights) with
  license page, Start Menu shortcut, "start at sign-in" option, update-in-place and an uninstaller that keeps
  your settings. A copy installed by the wizard no longer installs itself again or adds a second "Apps &
  features" entry; an older self-install entry is removed. `SIGN_CERT_THUMBPRINT` signs the files.
- **macOS: disk image.** `scripts/make-dmg.sh` builds `CursorPie-<version>-<arch>.dmg` with an Applications
  shortcut, optionally signed and notarized (`SIGN_ID`, `NOTARY_PROFILE`), plus a `.sha256` file.
  `UNIVERSAL=1` (also understood by `scripts/bundle.sh`) builds one app for Apple silicon and Intel. A
  Developer ID signature now uses the hardened runtime, which notarization requires.
- Tests that assumed macOS defaults are pinned to them, so the test suite passes on Windows.

## 0.4.1

- Fix dead code warnings on the Windows target

## 0.4.0

- **Developer tools.** `cargo tools` runs the repository's own tools (the `tools/` package, standard
  library only, not part of the app). `cargo tools bump patch|minor|major` raises the version in
  `Cargo.toml` and `Cargo.lock`, turns the notes under `## Unreleased` (or, without notes, the commit
  subjects since the last release) into the changelog entry, leaves a fresh `## Unreleased` section and
  can commit and tag the release from `main`. It lists the steps that stay manual and warns when
  something is probably stale. `cargo tools check` verifies that the files agree (and, with `--tag`,
  that a tag matches them), `cargo tools ci` runs every check CI runs, and `cargo tools hooks install`
  turns on Git hooks that check formatting and commit messages. This replaces the earlier
  `cargo run --example bump`. The workflow and its rules are in `CONTRIBUTING.md`, "Development
  workflow" and "Releasing".
- **Windows: CursorPie installs itself.** The first start copies the program to
  `%LOCALAPPDATA%\Programs\CursorPie`, turns on "Launch at login" for that copy, adds an "Apps & features"
  entry (`cursorpie.exe --uninstall` removes it, settings are kept) and continues from the installed copy.
  Before, the startup entry pointed at wherever the `.exe` was started from and had to be switched on by hand,
  so the app was gone after a restart as soon as the download folder was moved or cleaned. A newer download
  updates the installed copy. `cargo` builds and `CURSORPIE_NO_INSTALL=1` skip the install.
- **Windows: one running copy.** A second start exits at once instead of installing a second mouse hook.
- **Windows: "Launch at login" honors Task Manager.** An entry switched off in Task Manager reads as off in
  Settings, and ticking the box switches it on again.
- **Settings has a sidebar.** The sections (Menu, Behavior, General, About) are listed on the left and the
  selected page is on the right, like the settings of a large application. Behavior is one column of panels.
  The window is 1120 points wide (it shrinks on small displays) so the Menu editor keeps its room.

## 0.3.0

- **Windows 10/11 support.** The same code base now builds and runs on Windows and behaves almost
  identically to macOS: global mouse and keyboard hooks, app detection, per-app profiles, shortcuts,
  launchers, system tray, launch at login, Settings. Config files and shared profiles are portable
  between the two systems. See `docs/WINDOWS_MIGRATION.md`.
- **Truly transparent pie on Windows.** The Windows build uses the `glow` renderer, and the window
  frame that winit leaves on borderless windows is removed (`platform::clear_window_frame`): no white
  box or border around the pie, no old-style window flashing up when it opens, no thin white line
  along the top edge.
- Not at parity on Windows: extra mouse buttons. Only the middle button can be the trigger (Windows
  reports only the middle, back and forward buttons).
- Known limits on Windows: per-app profiles cannot tell UWP / Microsoft Store apps apart (they all
  report `applicationframehost.exe`), and programs running as administrator ignore CursorPie's input unless
  CursorPie runs as administrator too.
- macOS behavior is unchanged.

## 0.2.0

- macOS only. Config format version 2.
