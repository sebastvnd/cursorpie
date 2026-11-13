# The browser extension: controlling the browser from a slice

CursorPie ships a small extension for Firefox and Chromium browsers (Chrome, Edge, Brave, Vivaldi, Opera). Since it
only told CursorPie which pages were open (see `docs/developer/BROWSER_BRIDGE.md`), it now also **does things in the browser** when
a slice asks: pause the song, skip to the next one, close a tab, open the next unread chat in WhatsApp Web.

This page is the manual of those commands. How the extension is installed and how it talks to the app is in
`docs/developer/BROWSER_BRIDGE.md`.

## Quick start

1. Settings > Behavior > Browser extension: turn on *Know which web pages are open, and control the browser*, Save.
2. Press *Show the extension files* and load the folder `chromium` (Chrome, Edge, Brave, Vivaldi, Opera: extensions page,
   Developer mode, Load unpacked) or `firefox` (about:debugging > This Firefox > Load Temporary Add-on > `manifest.json`).
   **After an update of CursorPie nothing needs doing**: at the first start of the new version CursorPie writes the files to
   that folder again and asks the extension to reload itself (once per version, so it cannot loop). It only does this for a
   folder you exported to before, and only when the extension was loaded from that folder; one loaded from somewhere else
   keeps its old version, and the settings line says "the extension in Chrome is from another version" until you load the
   folder *Show the extension files* opens. Firefox forgets a temporary add-on when it closes, so there it still has to be
   loaded again after a restart of Firefox. The one exception is the update that first brings this: the extension that is
   loaded then is the old code, which does not know the request, so reload it by hand that once.
3. Make a slice: *+ Add item* > *Custom* > **Browser** (or change the kind of any slice to *Browser*). Choose a command in
   the list, for example *Media: Next track*. Commands that take text show a field beside the list.
4. Open YouTube (or Spotify, SoundCloud...) in the browser, start a song, and use the slice.

The kind **Browser** is the action `{"Browser": "media.next"}` in `config.json`: one line, the command and then its text.

```json
{ "name": "Next song", "icon": "ico:play", "action": { "Browser": "media.next" } }
{ "name": "Skip 30 s",  "icon": "ico:play", "action": { "Browser": "media.seek_fwd 30" } }
{ "name": "Mom: on my way", "icon": "ico:globe", "action": { "Browser": "chat.send_to Mom :: on my way" } }
```

A **Browser** action can be one step of a *Sequence*. In a flow the same thing is the **Browser command** block (below).

### The quickest way in: a template

**Flow tab > New flow > Start from a template** adds a complete, working flow to start from:

* *Browser remote*: a panel with play / pause, next, previous and volume that **stays open** between presses (an *Ask* block
  in Panel mode with `keep_open`, one *Branch* per button, one *Browser command* per branch).
* *Pause the music only if it plays*: `media.state` into a variable, a *Branch*, then `media.pause` or a notification.
* *Go to the tab that has a word*: asks for a word, runs `tab.find {{word}}`, and says when no tab has it.

They are also files in `docs/examples/flows/` (`browser-remote-panel`, `browser-pause-if-playing`, `browser-go-to-tab`).

## Which tab does a command act on?

The hard part of "next song" is *which* player, not pressing the button. The extension decides like this:

* **Media commands**: 1) the tab that is making sound right now (the one that started last if several), even in
  another window or in the background; 2) the tab that played last, so `media.play` resumes what `media.pause` stopped;
  3) a tab on a known player site, the one showing first.
* **Tab, window and page commands**: the active tab of the window that has the focus.
* **Chat commands**: the chat tab you are looking at, else the one you used last, else WhatsApp before Telegram.
  (`chat.folder`, `chat.folders` and `chat.next_mention` exist on Telegram only and always act on a Telegram tab.)
* **Several browsers connected**: the app sends the command to the browser that is making sound (media), the one that has
  a chat site open (chat), otherwise the one heard from last.

## The commands

### Media

Acts on the tab that makes sound; with none, on the tab that played last; then on a tab with a known player.

| Command | In the editor | Text after it | What it does |
|---|---|---|---|
| `media.toggle` | Play / pause |  | Pauses what plays; if nothing plays, resumes the player that played last. |
| `media.play` | Play |  | Resumes (does nothing if it already plays). |
| `media.pause` | Pause |  | Pauses. |
| `media.next` | Next track |  | Presses the player's own *next* button (YouTube, YouTube Music, Spotify, SoundCloud, Deezer, TIDAL, Bandcamp, Netflix). On a single YouTube video it goes to the suggested one. On other sites it looks for a "Next" button inside the player only. |
| `media.prev` | Previous track |  | Presses *previous*; where the site has none the track starts again. |
| `media.restart` | Restart the track |  | Back to the start of the track. |
| `media.seek_fwd` | Skip forward | optional: seconds | Jumps forward (default 10 s, change it in the options). Not on live streams. |
| `media.seek_back` | Skip back | optional: seconds | Jumps back. |
| `media.volume_up` | Volume up | optional: percent | Raises the volume of the player (default step 10 %); un-mutes. |
| `media.volume_down` | Volume down | optional: percent | Lowers it. |
| `media.mute` | Mute / unmute |  | Mutes or un-mutes the player. |
| `media.speed_up` | Faster | optional: step, like 0.25 | Playback speed up (0.25x to 4x). |
| `media.speed_down` | Slower | optional: step, like 0.25 | Playback speed down. |
| `media.speed_reset` | Normal speed |  | Speed back to 1x. |
| `media.like` | Like |  | Presses the like / heart button (YouTube, YouTube Music, Spotify, SoundCloud). |
| `media.captions` | Captions on / off |  | Turns captions on or off. |
| `media.skip_ad` | Skip ad or intro |  | Presses the site's own *Skip ad* (YouTube) or *Skip intro / recap* (Netflix) button when it is shown. |
| `media.shuffle` | Shuffle |  | Spotify's shuffle button. |
| `media.repeat` | Repeat |  | Spotify's repeat button. |
| `media.state` | Say if the player plays or is paused (for a flow) |  | Answers `playing`, `paused` or `none` (no player at all; it does not fail). No title leaves the page. Made for flows (below). |
| `media.now_playing` | Show what is playing |  | Shows a notification with the title and artist (read from the page's Media Session) and whether it plays. |
| `media.pause_all` | Pause everything |  | Pauses every tab that makes sound; `media.play` brings back the one that played last. |
| `media.focus` | Go to the player's tab |  | Goes to the tab of the player. |

### Tabs

Acts on the tab you are looking at (the active tab of the window that has the focus).

| Command | In the editor | Text after it | What it does |
|---|---|---|---|
| `tab.next` | Next tab |  | Next tab of the window (wraps round). |
| `tab.prev` | Previous tab |  | Previous tab (wraps round). |
| `tab.recent` | Back to the last tab used |  | The tab you were on before this one, like Alt+Tab for tabs. |
| `tab.first` | First tab |  | First tab. |
| `tab.end` | Last tab |  | Last tab. |
| `tab.goto_audible` | Go to the tab with sound |  | The tab that makes sound. |
| `tab.find` | Go to a tab with this text | required: text in the title or address | The first other tab whose title or address contains the text. |
| `tab.move_left` | Move the tab left |  | Moves the tab one place left. |
| `tab.move_right` | Move the tab right |  | Moves the tab one place right. |
| `tab.close` | Close the tab |  | Closes the tab. |
| `tab.reopen` | Reopen the closed tab |  | Reopens the tab closed last. |
| `tab.new` | New tab | optional: https:// address | A new tab, blank or with an `http(s)` address. Anything else is refused. |
| `tab.duplicate` | Duplicate the tab |  | Duplicates the tab. |
| `tab.to_window` | Move the tab to its own window |  | Moves the tab to a window of its own. |
| `tab.close_others` | Close the other tabs |  | Closes the other tabs of the window (pinned tabs stay). |
| `tab.close_right` | Close the tabs to the right |  | Closes the tabs to the right (pinned tabs stay). |
| `tab.close_dupes` | Close duplicate tabs |  | Closes tabs that show the same page (the `#fragment` is ignored); the one you are on and pinned tabs stay. |
| `tab.discard_others` | Unload the other tabs (free memory) |  | Unloads the other tabs to free memory; they reload when opened. Tabs with sound and pinned tabs are left alone. |
| `tab.pin` | Pin / unpin the tab |  | Pins or unpins. |
| `tab.mute` | Mute / unmute the tab |  | Mutes or un-mutes the whole tab. |
| `tab.reload` | Reload |  | Reloads. |
| `tab.hard_reload` | Reload without the cache |  | Reloads without the cache. |
| `tab.back` | Go back |  | Back in the tab's history. |
| `tab.forward` | Go forward |  | Forward in the tab's history. |
| `tab.zoom_in` | Zoom in |  | Next zoom step. |
| `tab.zoom_out` | Zoom out |  | Previous zoom step. |
| `tab.zoom_reset` | Zoom 100% |  | 100 %. |
| `tab.url` | Tell the tab's address (for a flow) |  | Answers with the address of the current tab (`http` / `https` only; cut at 300 characters). Fails on a page without one. |
| `tab.count` | Tell how many tabs the window has (for a flow) |  | Answers with the number of tabs of the current window. |

### Windows

| Command | In the editor | Text after it | What it does |
|---|---|---|---|
| `window.new` | New window |  | A new window. |
| `window.private` | New private window |  | A new private window (the extension must be allowed in private windows). |
| `window.minimize` | Minimize the window |  | Minimizes the window. |
| `window.maximize` | Maximize / restore the window |  | Maximizes, or restores. |
| `window.fullscreen` | Full screen on / off |  | Full screen on or off. |
| `window.close` | Close the window |  | Closes the window with all its tabs. Counts as a sensitive command when a profile is imported. |

### Scrolling

| Command | In the editor | Text after it | What it does |
|---|---|---|---|
| `page.top` | Scroll to the top |  | Scrolls to the top (smoothly). Needs *all sites* allowed, see below. |
| `page.bottom` | Scroll to the bottom |  | Scrolls to the bottom. |
| `page.up` | Scroll up a page |  | Scrolls up a page. |
| `page.down` | Scroll down a page |  | Scrolls down a page. |

### WhatsApp Web and Telegram Web

Acts on the chat tab you are looking at, else the one you used last, else WhatsApp before Telegram. Put `@telegram` or `@whatsapp` first in the text to choose: `chat.next_unread @telegram`. The tab is brought to the front (except `chat.status`).

| Command | In the editor | Text after it | What it does |
|---|---|---|---|
| `chat.focus` | Go to WhatsApp / Telegram | optional: @telegram | Goes to WhatsApp Web (or Telegram Web with `@telegram`); opens it when it is not open. |
| `chat.next_unread` | Next unread chat |  | Opens the next chat that has unread messages, going round the list. |
| `chat.prev_unread` | Previous unread chat |  | Opens the previous unread chat. |
| `chat.next` | Next chat |  | Opens the chat below the open one. |
| `chat.prev` | Previous chat |  | Opens the chat above. |
| `chat.close` | Close the chat |  | Closes the open chat (the Escape key). |
| `chat.bottom` | Jump to the latest message |  | Scrolls the conversation to the latest message. |
| `chat.status` | Show how many chats wait |  | Notification such as `WhatsApp: 3 unread · Telegram: nothing new`. Does not switch tabs. |
| `chat.search` | Search the chats | required: text | Types the text in the chat search box. |
| `chat.open` | Open a chat | required: name | Opens a chat by name: types the name in the search box and picks the match (the exact name first). |
| `chat.draft` | Type a message (not sent) | required: message | Types the text in the message box of the open chat, after what is already there. **Does not send.** |
| `chat.draft_to` | Type a message in a chat (not sent) | required: Name :: message | `Name :: message`: opens that chat, checks the name, types the message. **Does not send.** |
| `chat.send` | Send a message in the open chat | required: message | Types the text in the open chat and sends it, **only when "Allow sending messages" is on** in the extension options; otherwise it only types it. |
| `chat.send_to` | Send a message to a chat | required: Name :: message | `Name :: message`: opens that chat, checks that the name in its header matches, then sends, **only when "Allow sending messages" is on**. Nothing is sent when the name does not match or cannot be read. |
| `chat.unread` | Tell how many chats wait (for a flow) | optional: @telegram | The number of unread chats as a plain number, adding the chat sites that are open (`5`). Reads numbers only, so it works without *Allow reading messages*. |
| `chat.who` | Tell which chat is open (for a flow) | | The name in the header of the open chat. Needs **Allow reading messages**. Fails when no chat is open. |
| `chat.unread_names` | Tell the names of the chats that wait (for a flow) | | The names of the chats that have unread messages, one to a line (empty when none wait). Needs **Allow reading messages**. |
| `chat.last` | Tell the last message that came (for a flow) | | The text of the last message that came in the open chat (empty when none did; `[media]` for a picture, a voice note or a file). Needs **Allow reading messages**. |
| `chat.read` | Tell the last messages of the chat (for a flow) | optional: how many, up to 20 | The last messages of the open chat, oldest first, one per line start: `them: ...` came in, `me: ...` was sent (5 without a number). Needs **Allow reading messages**. |
| `chat.wait` | Wait for a message and tell it (for a flow) | optional: seconds, up to 300 | Waits for a message to come in the open chat and gives its text (60 seconds without a number). "Next" means after the last message that a chat command saw in that chat, and a send notes what is there first, so the answer to a message you just sent is not missed and an old one is not taken. Nothing came in time: it worked and the text is empty. Stops with a reason when the open chat changes. Needs **Allow reading messages**. |
| `chat.folder` | Go to a Telegram folder | required: folder name or number | **Telegram Web only.** Goes to a folder tab (All, Personal, Work...): the exact name first, then a number (`2` is the second tab), then part of a name (`wor` finds `Work`). Without `@telegram` it still acts on Telegram, even when WhatsApp is the tab in front. Fails when you have no folders. |
| `chat.folders` | Tell the names of the Telegram folders (for a flow) |  | **Telegram Web only.** The names of the folder tabs, one to a line. Needs **Allow reading messages**: a folder name says how you sort your chats. |
| `chat.next_mention` | Next Telegram chat with a mention |  | **Telegram Web only.** Opens the next chat that has an @mention badge, going round the list. A muted chat counts: a mention reaches you there too. None: it works and says so. |
| `chat.template` | Send a saved reply | required: reply, or reply :: Chat | Types the saved reply `name` (written in the options, see *Saved replies*) in the open chat, and sends it **only when "Allow sending messages" is on**. `name :: Chat` opens that chat first and checks its name like `chat.send_to`. |
| `chat.templates` | Tell the names of the saved replies (for a flow) |  | The names of the saved replies (not what they say), as a notification or for a flow. |

The commands that read a chat (`chat.who`, `chat.unread_names`, `chat.last`, `chat.read`, `chat.wait`, `chat.folders`) do not bring the tab to the front, so a flow can wait for a chat that stays in the background. They refuse until **Allow reading messages** is turned on in the extension options.

#### Telegram: folders, mentions, muted chats

* **Muted chats.** *Skip muted chats* (on by default, in the options) leaves out the chats whose notifications are off when
  `chat.next_unread`, `chat.prev_unread` and `chat.unread_names` look for unread chats, and in the unread number that is read
  from the chat list. The number in the **page title** (`(3) Telegram`), which `chat.status` and `chat.unread` use first, is
  the page's own and is not changed by it. Turn it off to get the old behaviour.
* **Folders.** `chat.folder Work` (or `chat.folder 2`) changes folder, so `chat.next_unread` then walks the unread chats of
  that folder only. Make a slice for each folder you use, or a flow that asks (`Ask`, then `chat.folder {{choice}}`).
* **Mentions.** `chat.next_mention` goes to the chats where somebody wrote your @name, even if they are muted.
* **By @username.** On Telegram Web `chat.open @durov` opens the chat by its address, which does not depend on the language
  of the page (the search box does). The header of a chat shows the person's name and not the @username, so the chat that
  opened **cannot be checked against what you asked**: `chat.draft_to @durov :: hi` types the message and
  `chat.send_to @durov :: hi` types it too but **never sends it** (it says so); press Send yourself. When the chat that was
  already open is the one you asked for, nothing is typed (the extension cannot tell that anything opened); use `chat.draft`
  or `chat.send` then.
* **Saved replies.** Write short texts in the options, *Saved replies* (`{"omw": "On my way, back at {time}", "hi": "Hi
  {first}!"}`) and use `chat.template omw`, or `chat.template hi :: Anna` to send it to a chat. `{time}` and `{date}` are
  filled in; `{name}` is the chat's name and `{first}` its first word: the one written after `::`, else the name in the
  header of the open chat, which is read only with **Allow reading messages** on (without it a reply that uses them types
  nothing and says why). A saved reply is sent like `chat.send`: only with **Allow sending messages** on, else it is only
  typed.

### Panels

These ask you something in a **panel** of CursorPie (`docs/PANELS.md`) and then act. They need **Settings, Behavior,
"Let the browser extension show panels"**; without it they show an error that says so.

| Command | In the editor | Text after it | What it does |
|---|---|---|---|
| `ui.tabs` | Pick a tab from a list | | A searchable list of the other open pages (this window and the most recently used first); picking one goes to it. Enter picks the first match. |
| `ui.find_tab` | Ask for a word, then go to that tab | optional: heading of the box | A box for a word, then the same as `tab.find`. |
| `ui.confirm_close_others` | Ask, then close the other tabs | | Says how many tabs would close (pinned tabs stay) and closes them only after you agree. |
| `ui.video` | Video remote (stays open: skip, next, pause, volume...) | | A small remote that **stays open**: each press runs the matching `media.*` command and the remote comes back, until you close it (Close, Esc), it times out, or CursorPie is gone. One slider sets how far *Back* and *Forward* jump (it starts at the *seek seconds* option). |
| `ui.media` | Media menu (play, next, volume...) | | A list of play / pause, next, previous, volume, mute and like; the pick runs that `media.*` command. |

`media.*` work on: YouTube, YouTube Music, Spotify (web player), SoundCloud, Deezer, TIDAL, Bandcamp, Twitch, Netflix,
Vimeo, Dailymotion, Apple Music, Prime Video. Play, pause, seek, volume, mute and speed work on **any** page with a
`<video>` or `<audio>` once *all sites* is allowed (below); *next*, *previous*, *like*, *shuffle* and *skip ad* need the
site's own buttons and only exist for the sites that have an adapter in `page-media.js`.

## Using the commands in a flow

A flow has a block for this: **Do something > Browser command** (it is the *System* block with the operation
`Browser`). Its first field is the command line, the same as in a slice and with `{{variables}}` filled in
(`tab.find {{word}}`, `chat.draft_to {{who}} :: {{text}}`). Its second field is a **variable** that receives what the
browser answers; it may stay empty.

In the editor the first field is the **same list of commands as a slice has** (`Media: Next track`, `Tabs: New tab`...), with
a field beside it for the text the chosen command takes (it says whether that text is required or optional). Under the block,
the list view says whether a command can reach a browser right now: extension off, no browser connected, or an extension from
another version. A command name that is only known when the flow runs (`{{cmd}}`) is typed instead of picked. The *add a
block* menu has a **Browser** group with the common commands ready to use.

Unlike a slice, which sends the command and moves on, the block **waits** until the browser says how it went:

* `ok` is `1` when it worked and `0` when it did not (no browser connected, the extension is off or too old, the page
  cannot be reached, no player, a chat that does not match...). A failure that the flow does not look at is reported at
  the end of the run, with the browser's own reason.
* The variable gets the browser's words, also when it failed (they say why). For the questions it is the answer:
  `tab.url` the address, `tab.count` a number, `media.state` `playing`, `paused` or `none`, `media.now_playing` and
  `chat.status` a sentence.
* The answer of a command a flow waits for is **not** shown as a notification: the flow decides what to say.
* It waits 20 seconds. A `ui.*` panel waits as long as you need, up to 5 minutes, and `chat.wait` as long as it was told
  to wait (up to 300 seconds) and 15 seconds more; that time is not counted in the 10 minutes a flow may run. Nothing
  answers if the browser is closed or goes away during the wait: that is a failure.
* A command that sends a message (`chat.send`, `chat.send_to`, `chat.template`), closes a window or reads a chat
  (`chat.who`, `chat.unread_names`, `chat.last`, `chat.read`, `chat.wait`, `chat.folders`), and a command that is only a
  `{{variable}}`, count as sensitive: the block is red and an
  imported flow that has one is flagged, like a `Run` block. What a flow reads from a chat could be given away by the
  blocks after it (a link opened with it, a command that gets it).
* `chat.send_to` followed by `chat.wait` is the way to send a message and wait for the answer; see
  `examples/flows/send-then-wait-for-answer.cursorpie-flow.json` and `quick-reply-unread-chats.cursorpie-flow.json` in
  `docs/FLOWS_FOR_AI.md`.

Example: pause the music only when it plays, and tell the person where they are:

```
Browser command   media.state            -> state
Branch            {{state}} = playing
  yes: Browser command   media.pause
Browser command   tab.url                -> where
Notify            Paused on {{where}}
```

Flows can also be written as JSON (`docs/FLOWS_FOR_AI.md`): `{"Native": {"op": "Browser", "a": "media.state", "b": "state"}}`.

## What you see

* A command that works is silent. One that fails shows a notification from CursorPie saying why ("No player found: open
  YouTube, Spotify, SoundCloud... in a tab first", "WhatsApp Web or Telegram Web is not open in the browser").
* Questions (`media.now_playing`, `media.state`, `tab.url`, `tab.count`, `chat.status`) are always answered with a
  notification. Inside a flow the answer goes to the flow instead (below).
* If the command cannot even be sent (the bridge is off, no browser is connected, the extension is too old) the app
  says so.
* The extension options (the extension's *Options* page in the browser) have **Notifications**: *only what went wrong*
  (default), *every result*, or *nothing*.

## The options page of the extension

Open it from the browser's extensions page (Details > Extension options; in Firefox: Add-ons > the extension >
Preferences).

| Option | Default | Meaning |
|---|---|---|
| Media / Tabs, windows and scrolling / Chats / Panels | on | Switch a whole group of commands off: the extension then refuses them. |
| **Allow reading messages** | **off** | Off: `chat.who`, `chat.unread_names`, `chat.last`, `chat.read`, `chat.wait` and `chat.folders` refuse. On: they hand the text of messages and the names of chats to CursorPie on this computer, for a flow to use. Nothing leaves the computer. |
| **Allow sending messages** | **off** | Off: `chat.send`, `chat.send_to` and `chat.template` only **type** the message and leave it in the box for you to send. |
| Skip muted chats | on | `chat.next_unread`, `chat.prev_unread`, `chat.unread_names` and the unread number read from the chat list leave out muted chats. |
| Saved replies | empty | The texts of `chat.template`, as JSON: `{"omw": "On my way, back at {time}"}`. |
| Starting one player pauses the others | off | When a tab starts to make sound, every other tab that plays is paused. |
| Seek / Volume / Speed step | 10 s / 10 % / 0.25 | The default of the commands that take a number. |
| Notifications | only errors | See above. |
| All sites | not allowed | See below. |
| Selector overrides | empty | See "When a site changes". |

## Permissions: what the browser asks, and why

| Permission | Why |
|---|---|
| `tabs` | Know the address (and title) of the open tabs, to find the right one. |
| `nativeMessaging` | Talk to CursorPie on this computer. |
| `scripting` (Chromium) / the site list (Firefox) | Press the player's buttons in the pages of the sites below. |
| `storage` | The options page. |
| `sessions` | `tab.reopen`. |
| `alarms` | Reconnect when CursorPie starts after the browser. |
| Sites: YouTube, Spotify, SoundCloud, Apple Music, Deezer, TIDAL, Bandcamp, Twitch, Vimeo, Netflix, Prime Video, Dailymotion, WhatsApp Web, Telegram Web | Where the code of `page-media.js` and `page-chat.js` may run. |
| *Optional*: all sites | Only if you press **Allow all sites** in the options page. Needed for `page.*` and for the media commands on sites that are not in the list. |

The browser's install warning ("read and change your data on these sites") is the site list above. Nothing is run on a
page unless CursorPie asks for a command.

## Privacy

* Everything goes over native messaging to CursorPie **on this computer**. The extension has no server, makes no request
  and has no analytics.
* To the app, on its own, the extension sends: the address of `http(s)` tabs, tab and window numbers, **how many** tabs
  make sound, **how many** tabs are on a player site, which chat sites are open with the **unread number from the page
  title** (`(3) WhatsApp`). **Never** a page title, a song, a chat name or a message on its own.
* A title leaves a page only when you ask: `media.now_playing` reads the page's Media Session (title and artist) and
  shows it as a notification.
* On its own, the chat code reads only the number of unread chats and the **name in the header of the open chat** (to
  check that a message goes to the right person).
* The **text of messages**, the name of the open chat, the names of the chats that wait and the names of the Telegram folders
  are read only by `chat.who`, `chat.unread_names`, `chat.last`, `chat.read`, `chat.wait` and `chat.folders`, and only when **Allow reading messages** is on in the
  extension options (it is **off** by default). They go to CursorPie on this computer, to a flow that asked for them,
  and nowhere else. What the extension remembers to tell one message from the next is a short number made from the text,
  never the text itself.
* The app sends the extension only commands of the list above. A line that is not in the list is never sent, text after a
  command is limited to 2000 characters, and `tab.new` only opens `http(s)` addresses.
* **Importing** a profile or flow that has `chat.send`, `chat.send_to`, `window.close` or a command that reads a chat is
  flagged like one that runs shell commands ("can run shell commands, write files or send messages - review them before you Save").

### Messages are not taken back, so:

1. Sending is **off** until you turn on *Allow sending messages* in the extension. Until then `chat.send*` only types.
2. `chat.send_to Name :: text` sends only after it has opened the chat and read its header: when the name in the header
   does not contain the name you wrote (or the header cannot be read) **nothing is sent and nothing is typed**.
3. `chat.draft`, `chat.draft_to` add to what is already in the box; they never replace a half-written message.
4. Use `chat.draft_to` when you want the message ready and to press Send yourself.

## When a site changes

YouTube, Spotify, WhatsApp and Telegram change their pages without notice, and these commands press the buttons of
those pages. This is the part of the extension that will break, and it is built to be fixed in one line:

* `page-media.js`: a table `SITES` at the top; each command is a list of CSS selectors tried in order.
* `page-chat.js`: the table `CP.CHAT_SELECTORS` (`rows`, `selected`, `unread`, `search`, `composer`, `send`, `header`,
  `scroll`, and for reading `messages`, `text`, `incoming`, `outgoing`, and `muted`, `mention`, `folders` for the Telegram
  extras) for WhatsApp and Telegram (versions K and A).
* **Without a new version**: in the options page, *Selector overrides*, put a working selector first:

```json
{ "whatsapp": { "composer": ["footer div[contenteditable='true']"] },
  "telegram": { "send": ["button.my-send"] } }
```

To find a selector: open the site, right-click the button, *Inspect*, and use a stable attribute (`data-testid`, `data-icon`,
`aria-label`, an `id`) rather than a class like `x1a2b3c`.

## What is tested, and what is not

Tested (`cargo test`, and `cd tests/browser-extension && npm install && npm test`; `flow-commands.test.js` alone needs no `npm install`):

* the app and the extension know exactly the same 92 commands (a test reads the extension's source), and every command is in
  this page;
* a command line is read and refused correctly; what is sent to which browser; ids; results;
* the page code against small copies of the pages in jsdom: toggle, next/previous, seek, volume and speed limits, skip ad,
  "Next" never pressed outside the player, unread chats found by badge and by look, cycling through unread chats, picking
  a chat by name, drafting without sending;
* the background: which tab is the target, resume of the last player, tab commands (wrap, recent, find, close dupes, close
  others, pinned tabs spared), the options switching groups off, notification rules, `tab.new` refusing `javascript:` and
  `file:`, the Firefox way of running code in a page;
* the safety rules of messages: no sending while it is off, no sending to a chat whose name does not match, none when the name
  cannot be read;
* the Telegram extras (`chat-telegram.test.js`, no jsdom needed): which row is picked with muted chats skipped or not,
  mentions, folder choice by name and number, the saved replies and their `{time}` / `{name}`, and that a chat opened by
  `@username` is never sent to. The page side runs against a tiny fake of the page where each selector finds what the test
  says, so it proves the logic, **not** that the selectors match the live Telegram Web (see below).

**Not tested, because it needs a real browser and an account:** that the selectors (the new `muted`, `mention` and `folders` ones included) still match the live YouTube, Spotify,
SoundCloud, WhatsApp Web and Telegram Web today; Firefox and Chromium loading the extension; window state changes on
Wayland. The adapters were written from how those pages are built, and the chat selectors in particular are the most
likely to need an override. If a command says "not found", see *When a site changes*. Please report what you find in
`docs/user/known-issues.md`.

## Known limits

* Full screen and Picture-in-Picture need a click by a person: a page refuses them from a script, so there is no command.
* Spotify and a few players do not expose a `<video>`: seek, volume and speed do not work there, play, pause, next, previous
  and like do.
* WhatsApp Web only draws the chats near the top of the list: `chat.next_unread` knows about those.
* WhatsApp's own "marked as unread" dot is found only if its markup matches the selectors.
* The extension is loaded unpacked (Chromium) or as a temporary add-on (Firefox) until it is published to the stores; a
  Firefox temporary add-on is forgotten when Firefox closes.
* A flow waits at most 20 seconds for a command (330 for a `ui.*` panel, the told time plus 15 for `chat.wait`). A `ui.video` remote that is still open when
  that ends is not closed: the flow goes on and the remote stays until you close it.

## For developers

Files (`assets/browser-extension/`): `background.js` (the link, state and results), `util.js` (settings, `CP.runInTab`, the
command registry), `cmd-tab.js`, `cmd-media.js`, `cmd-chat.js` (the commands), `page-media.js`, `page-chat.js` (code that
runs inside pages), `options.html/js`, the two manifests.

To add a command:

1. Register it on one line, `reg("media.example", "media", async ({ arg, settings }) => ...)`, in the matching `cmd-*.js`.
   It returns `CP.ok(message, explicit)` or `CP.fail(message)`. If it has to act inside a page, add an action to the page
   function; it is sent as text, so it must not use anything outside its own body, and must not be `async` (split the
   waiting in steps in the command, as `cmd-chat.js` does).
2. Add it to `CATALOG` in `src/bridge/commands.rs` (id, group, name in the editor, what text it takes).
3. Add a row to the table above. A test fails until the three agree.
4. Add a test in `tests/browser-extension/extension.test.js` (or, for a command that only needs the `chrome` API and
   no page, in `flow-commands.test.js`, which runs without jsdom).
5. A command that answers a question for flows returns the answer as `CP.ok(answer, true)` and never puts a page title in
   it unless the command's job is to say it (`media.now_playing`): a flow may send the answer anywhere.

Protocol (one JSON object per line, see `src/bridge/proto.rs`):

```text
app -> extension   {"type":"command","id":7,"cmd":"media.seek_fwd","arg":"30"}
extension -> app   {"type":"result","id":7,"ok":true,"message":"","notify":false}
extension -> app   {"type":"state","audible":1,"mediaTabs":2,"chats":[{"site":"whatsapp","unread":3}]}
extension -> app   {"type":"hello","browser":"chrome","version":"0.8.9","commands":["media.next", "..."]}
app -> extension   {"type":"reload","version":"0.9.0"}   after an update: the files in the extension's folder are this version, reload to take them
```
