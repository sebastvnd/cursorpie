# Example flows

Files you can import into CursorPie: **Settings > Flow > Import**, choose the file, review the blocks and press **Save**.
An import adds a **new** flow and never overwrites yours. Several examples run shell commands or talk to the browser,
so read a flow before you save it (CursorPie warns you after importing one).

The format is explained in [FLOWS_FOR_AI.md](../FLOWS_FOR_AI.md) and checked by [flow.schema.json](../flow.schema.json).
These files are also compiled into the app and tested, so every one of them loads without a red wire.

## Which system is it written for?

A `Key` block stores a **macOS key code** and a modifier bitmask: `mods` 8 is Cmd on a Mac, the Windows key on Windows
and Super on Linux. The three simplest examples use `mods` 8, so they are **macOS shortcuts**. On Windows and Linux open
the flow in the editor and re-record each shortcut (Ctrl instead of Cmd), or change `"mods": 8` to `"mods": 2` in the file.

| File | What it does | Works on | Needs |
|---|---|---|---|
| `open-app-and-new-note` | Opens Notes, waits, presses Cmd+N | macOS (app name `Notes`, Cmd) | |
| `copy-switch-paste` | Cmd+C, waits, Cmd+Tab, waits, Cmd+V | macOS (change `mods` to `2` elsewhere) | |
| `link-wait-command` | Opens a page, waits, appends the date to `~/cursorpie-flow.log` | macOS and Linux (the command is POSIX shell) | |
| `ask-and-loop` | Asks how many, then notifies that many times | all | `notify-send` on Linux; `zenity` or `kdialog` for the question |
| `copy-newest-file-and-paste` | Puts the newest exported image or PDF on the clipboard as a file, then pastes | macOS only (`stat -f`, AppleScript) | Files and Folders permission for Downloads |
| `panel-form` | Asks with a panel (text, slider, switch) and says the answer | all | [Panels](../PANELS.md) on |
| `progress-panel` | Five rounds of work with a progress panel | all | Panels on |
| `browser-go-to-tab` | Asks for a word and goes to the tab that has it | all | [browser extension](../BROWSER_EXTENSION.md) |
| `browser-pause-if-playing` | Pauses the music only if it plays | all | browser extension |
| `browser-remote-panel` | A browser remote panel that stays open | all | browser extension, panels |
| `quick-reply-unread-chats` | Quick reply to every unread chat | all | browser extension; sends messages |
| `send-then-wait-for-answer` | Sends a message, then waits for the answer | all | browser extension; sends messages |
| `telegram-folder-triage` | Looks at one Telegram folder at a time | all | browser extension (Telegram Web) |
| `telegram-saved-reply` | Sends a saved reply on Telegram or WhatsApp | all | browser extension; sends messages |
| `config-with-flow.json` | A whole `config.json` with one flow and a slice that runs it. Not a flow file: use **Settings > General > Import** | all | |

Files named `*.cursorpie-flow.json` are single flows; `"cursorpie_flow": 2` is the current file format and `1` (used by
the three oldest examples) is still read.

## Writing your own

Use **New flow > Build it with AI** in the Flow section, or give an agent [FLOWS_FOR_AI.md](../FLOWS_FOR_AI.md).
