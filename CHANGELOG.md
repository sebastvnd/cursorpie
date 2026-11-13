# Changelog

## Unreleased

## 0.8.13

- Publish crates v.0.8.12

## 0.8.12

- **New: the notice follows a run from its start to its end.** A flow, a pause or a sequence with a pause in it
  used to show its notice for a moment at the start and then went quiet: while a *Wait* block, a *Run and wait*
  command or a long flow was still busy nothing was on the screen, and then something appeared out of nowhere.
  Now the pill stays until the run is over and says what it is doing and how far it has come: a thin bar along the
  bottom edge and a small label at the right end. A *Wait* shows a countdown (`Waiting`, `3.2 s`), a *Run and wait*
  block shows a moving bar and the seconds it has taken (`Running command`, `12 s`, the command itself is still never
  shown), a flow that goes straight through shows `3/8`, a *Repeat* or *For each* loop shows its round (`2/10`), a
  flow with a *While* or a call shows `step 4`, and a sequence shows its step. When the run ends the bar fills and the
  pill goes away after its usual time (*Behavior > Notice*). A run that is hidden with *Hide notices*, or with the
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

- **New: a progress panel with a Stop button for long runs, and an *Update panel* block.** On top of the small
  notice, a flow, a pause or a sequence that lasts more than about two seconds now opens a small panel with a bar,
  the step that is running (`Waiting`, `Running command`, `Open app: Safari`), how far it has come (`3.2 s`, `12 s`,
  `Round 2/10`, `Step 3/8`) and a **Stop** button. Stop ends the run at the next block and cuts a *Wait* or a *Run
  and wait* short (the command is ended); stopping is not reported as a problem and the notice says `Stopped`. The
  panel never takes the keyboard on Windows, so the keys a flow sends still reach their app; on macOS and Linux a run
  that presses keys keeps the small notice instead. Another panel or the pie takes the window from it at once. *Behavior >
  Show a progress panel for long runs* (`progress_panel`, on by default) switches it off. The new flow block **Update
  panel** (`PanelUpdate`: *Open*, *Update*, *Close*) lets a flow open its own progress panel and set its title,
  label and bar (a fraction, `40%` or `busy`) while it works, with an optional Stop button; nothing waits for it and
  it goes away when the run ends. New: `Ending::Cancelled`, `Host::cancelled`, `Host::panel_command` and
  `parse_panel_value` in `flowrun`, `PanelSpec::passive`, the `runpanel` module, `platform::native::exec_until`,
  `docs/examples/flows/progress-panel.cursorpie-flow.json`.

- **New: building flows for the browser extension is much easier.** The *Browser command* block used to be a text
  field in which the person had to type the exact name of a command (`media.seek_fwd 30`) from the documentation, and
  a mistake only showed when the flow ran. Now the block has the same list of commands as a slice (`Media: Next
  track`, `Tabs: Go to a tab with this text`...) with a field beside it for the text the command takes, marked
  required or optional; in the list view it also says whether a browser can be reached right now (extension off, no
  browser connected, extension from another version). The add-block menus have a **Browser** group with ready
  commands (play / pause, next track, is the music playing?, address of the tab...). **New flow > Start from a
  template** adds a complete working flow to learn from: a *Browser remote* (a panel with play, next, previous and
  volume that stays open, one Branch and one command per button), *Pause the music only if it plays* and *Go to the
  tab that has a word*. A command name that is a `{{variable}}` is still typed. New: `config::flow_templates`,
  `docs/examples/flows/browser-*.cursorpie-flow.json`.

## 0.8.11

- **Fixed: Settings ended up behind another window after cancelling a file chooser (macOS).** When the chooser
  closed, the system gave the focus to the window below Settings, and the one request to bring Settings back could
  lose against it, leaving Settings covered and flickering. CursorPie now activates itself through the system's own
  call, asks again a moment later if the focus still is not back, and leaves a window that already has the focus alone.

- **Fixed: the app list missed apps that exist.** *macOS:* Finder, Screen Sharing and the other apps in
  `/System/Library/CoreServices`, apps one folder down (Adobe, Setapp, Chrome apps, `~/Applications/...`) and
  iPhone/iPad apps were not listed; an app was named by `CFBundleName` (`Code`) instead of what Finder shows
  (`Visual Studio Code`), so searching `visual` found nothing, and the name in the system language (`Kalkulator`) was
  unknown. *Windows:* the Start menu was read once and kept until CursorPie quit, so an app installed later never
  appeared, and a program that was once not found was never looked for again; apps that start through an
  `Update.exe` (Discord, GitHub Desktop, Slack, Postman) were dropped as updaters. *Linux:* desktop entries in
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
  for a flow; it reads names, so it needs *Allow reading messages* and is flagged like `chat.unread_names`) and
  `chat.next_mention` (the next chat where somebody mentioned you, muted ones included). They always act on a Telegram tab,
  also when WhatsApp is the one in front, and refuse `@whatsapp`. The new option **Skip muted chats** (on by default) makes
  `chat.next_unread`, `chat.prev_unread`, `chat.unread_names` and the count read from the chat list leave out muted chats;
  the number in the page title is not changed. Turn it off for the old behaviour.
- **New: saved replies.** Write short texts in the options (*Saved replies*, JSON: `{"omw": "On my way, back at {time}"}`)
  and use `chat.template omw` in the open chat or `chat.template omw :: Anna` to send it to a chat (checked like
  `chat.send_to`). `{time}` and `{date}` are filled in, and `{name}` / `{first}` with the chat's name: the one after `::`,
  else the open chat's, which needs *Allow reading messages*. It sends only with *Allow sending messages* on, else it only
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
  its tab, put a working selector first in *Selector overrides*.
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
  which commands, and only with *Allow reading messages* on. `docs/BROWSER_EXTENSION.md` lists the new commands and
  options (and 87 commands, and 10 minutes where it said 2 by mistake).
- **New: two example flows for chats**, `quick-reply-unread-chats` (an unread chat at a time, a person types and confirms
  each reply, then `chat.send_to`) and `send-then-wait-for-answer`, in `docs/examples/flows/`, in the AI kit and in the
  tests, which run both against a scripted host.
- **Flow canvas: an inspector for the selected block, and a way to follow a variable.** Clicking a block now opens a
  panel at the right of the canvas (a press anywhere in the block selects it, not only its header). Its *Block* page
  edits the block's settings, lists its problems in words, and shows what it *reads* (is it set before this block? which
  blocks can the value come from?) and *writes* (which blocks read it afterwards?), each block a button that jumps to
  it. Its *Variables* page lists every variable of the flow with who sets and who reads it, and marks one that is read
  where nothing sets it. Following a variable rings the blocks that set it in green and the ones that read it in blue
  on the canvas and draws dashed lines along which its value travels. The analysis is public in
  `config::flow_check` (`variables`, `data_edges`, `is_set_before`, `block_reads`, `block_writes`).
- **A panel's height can be set, not only its width.** A panel used to be as tall as its content needed, with no way to
  change it. The panel description has a new optional `height` (200 to 900 points; without it nothing changes), and the
  panel designer has a *Height* control under *Width*: *Auto* keeps fitting the content, *Fixed* gives a slider. With a
  fixed height a taller content scrolls in the body and the buttons stay at the bottom; a shorter one leaves room above
  the buttons. A panel is still never taller than the display leaves room for.
- **An update now brings the program's other files along, and the browser extension reloads itself.** An update used to
  replace only the executable, so everything CursorPie had written outside it stayed as the old release left it, and after
  every update the browser extension had to be exported again and reloaded by hand. The first start of a new version now
  writes those files again (`src/refresh.rs`): the browser extension folder (when it was exported before), the login entry
  (when *Launch at login* is on, from an ordinary install only), and on Linux the desktop entry (when it is still
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
- **New: a flow can call the browser extension and wait for it.** The *System* block has a new operation, **Browser
  command** (*Do something > Browser command*): the command line (`media.next`, `tab.find {{word}}`...) goes to the
  extension, the block waits for the answer, `ok` says whether it worked and a variable gets the browser's words (`tab.url`,
  `tab.count`, `media.state` and the other questions answer there, and a failure says why). A command that sends a message
  or closes a window makes the block red. The checker reads a command that is written out (`bad-browser-command`). New
  questions for flows: `tab.url`, `tab.count`, `media.state` (`playing`, `paused` or `none`). See
  `docs/BROWSER_EXTENSION.md`, *Using the commands in a flow*.
- **Fixed: the answer of a browser command was thrown away.** The extension said how a command went, but the app only showed
  it as a notification, so nothing could wait for it. The bridge now keeps who waits for which command and gives the answer
  to that one (and only to the browser the command went to); a browser that goes away ends the wait at once.
- **Fixed: "Starting one player pauses the others" woke the wrong player.** Pausing the others made the extension remember
  the paused tab as the one that played last, so *Play* after a *Pause* resumed the player that had just been silenced.
- **Fixed: `ui.video` was missing from `docs/BROWSER_EXTENSION.md`**, which made the test that every command is documented
  fail; the count of commands in that page was out of date too.
- **Fixed: two bridge log lines used the field names `cmd` and `command`**, which `cargo tools diag-lint` refuses.
- **Fixed: `docs/flow.schema.json` listed only 16 of the *System* operations**, so a flow that used any later one (folders,
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

- **New: a visual panel designer.** The *Ask* block in Panel mode has a **Design...** button that opens a sheet: add
  widgets with a click, start from a template, see the real panel (the same drawing a flow uses) and click a widget to
  edit it in the inspector. No JSON to type; every change is written to the block at once and can be undone. The rules
  (new widgets, moving, copying, renaming without breaking a condition, options as lines of text, templates) are in the
  tested `panel::design`; the drawing is `ui::settings::panel_designer`.
- **New: more to interact with in a panel.** Widgets `number` (`-`/`+` and a box, with range, step and unit) and `color`
  (accent swatches and the system picker, answers `#rrggbb`); `choice` layout `dropdown`; and `show_if` on every widget,
  so a form can show a field only when another one says so. What the designer writes is short JSON (defaults left out,
  plain options as texts). See `docs/PANELS.md`.

- **New: panels, a base for interactive windows.** Until now CursorPie could only *announce* (the pill) or ask through
  a window of the system (a text, yes or no). A **panel** is a small window of CursorPie's own with a title, widgets
  (text, boxes, switches, sliders, single and multiple choice lists with search, a progress bar) and up to four buttons,
  in the look of Settings, that hands back what the person entered. It is plain data (JSON, `docs/PANELS.md`), so flows,
  the browser extension and later extensions share one base instead of each writing window code, and a custom look
  replaces only the drawing (`ui::settings::panel_form`). New module `panel` (`spec`, `form`, `layout`, `broker`): the
  description with limits, the live form with the rules (required boxes, snapping, patches), the size worked out before
  the first frame, and `panel::ask` for any thread. It is a fourth mode of the one window: not click-through, takes the
  keyboard, placed next to the pointer, centered or at an edge; the pie and Settings take it over (the panel is answered
  as cancelled), and a panel asked for while the window is busy waits in a short queue.
- **New: the *Ask* block has a Panel mode.** The block's text is the JSON of a panel; every field sets the variable
  named by its `id`, `out` gets the button that was pressed, `ok` says whether a confirming button was. Variables are
  filled into the *texts* of the panel after the JSON is read, so a value cannot add a widget. The editor's checks know
  the variables of the fields. Example: `docs/examples/flows/panel-form.cursorpie-flow.json`. New `Host::ask_panel`
  (default: cannot).
- **New: the browser extension can show panels.** Bridge messages `panel`, `panel_update`, `panel_close` and
  `panel_result`; `CP.ui` in the extension (`show`, `update`, `close`, `confirm`, `prompt`, `pick`) in new `ui.js`; a new
  group of commands, **Panels**: `ui.tabs` (searchable list of open tabs), `ui.find_tab`, `ui.confirm_close_others`,
  `ui.media`. Off by default: Settings, Behavior, *Let the browser extension show panels* (`extension_panels`; needs
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
  second step was judged by the clock alone, counted from the moment the tap *asked* for the pie. When the window was
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
  *Windows* trimmed the whole working set when the UI went to sleep (`SetProcessWorkingSetSize(-1, -1)`), which also
  evicted the code of the mouse hooks and the UI, so the next press page-faulted it all back in; a low-level hook
  that answers too slowly is dropped by Windows. Sleep now compacts the heap (`HeapCompact`), which gives back freed
  memory and leaves what is in use alone. Windows can also remove low-level hooks silently (it is known to after a slow
  callback, and reported after a resume or an unlock) and the message loop then never ended, so the hooks were never
  installed again: a **watchdog**
  notices that the system still sees input while no hook has run, and reinstalls both hooks at once (at most every
  15 s); a watchdog tick that arrives more than 10 s late means the machine slept, and the hooks are then
  reinstalled before the first press. The hook thread runs a little above normal priority, and the process opts out of power throttling (EcoQoS)
  and of the timer-resolution rule for hidden programs, which Windows 11 applies to a program with no window.
  *macOS* can put the hidden accessory app into App Nap, which stretches timers (the hold timer, the trackpad thread) and
  lowers thread priority; the process now declares a user-initiated activity that still lets the Mac sleep. The event
  tap is now looked at every 2 s and turned on again when macOS switched it off while nothing was calling the
  callback. The trackpad source decided "the Mac slept" with `Instant`, which does not count sleep, so a real sleep
  went unnoticed (the trackpad stayed dead until a restart) while a throttled thread was taken for one and restarted
  a healthy source; it now also compares the wall clock.
  *Both:* sleep emptied the whole icon cache, which saved a few KiB and made the first pie decode its icons (a
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
