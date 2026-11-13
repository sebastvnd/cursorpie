# CursorPie documentation

Pick the door that fits you.

| I am... | Start here |
|---|---|
| **Using CursorPie** | [User guide](user/README.md) |
| **Changing the code** | [Developer guide](developer/README.md) |
| **Writing a flow or a browser command** (by hand or with an AI agent) | [Reference for flow and extension authors](#reference-for-flow-and-extension-authors) |

## Platform support at a glance

| Platform | Level |
|---|---|
| Windows 10 (1809) / 11 | Stable |
| macOS 11+ | Supported |
| Linux, **X11** session | **Pre-stable**: complete and in use, manual checklist still being finished |
| Linux, **Wayland** session | **Experimental**: written and unit tested, not yet run on real compositors; needs a udev rule |

Details: [CursorPie on Linux](user/linux.md).

## Reference for flow and extension authors

These files stay at the top of `docs/` because the app and the website read them by path (the "Build it with AI" kit
embeds the first three; the extension tests read the browser manual).

| Document | What it covers |
|---|---|
| [FLOWS_FOR_AI.md](FLOWS_FOR_AI.md) | The flow file format, the rules that make a flow wrong, a recipe, shell and clipboard pitfalls, checked examples. Written for AI agents |
| [flow.schema.json](flow.schema.json) | JSON Schema of a flow file |
| [examples/](examples/README.md) | Importable example flows, with the system each one is written for |
| [PANELS.md](PANELS.md) | Panels: small interactive windows that flows and the browser extension can show |
| [BROWSER_EXTENSION.md](BROWSER_EXTENSION.md) | The commands the browser extension runs (media, tabs, chats) |
| [DEBUGGING.md](DEBUGGING.md) | The diagnostic log, and how to add a log line |

## Keeping the docs in sync

- A user-visible change updates the matching page in `docs/user/` in the same pull request, and adds a line under
  `## Unreleased` in `CHANGELOG.md`.
- `README.md` stays short and has no version-specific sentences ("Version 1.2 adds ..."); they go stale.
- Platform status (Stable, Supported, Pre-stable, Experimental) is defined once, in [user/linux.md](user/linux.md#support-level);
  other pages repeat the word, not the definition.
- A design record in `docs/developer/` keeps its history, but its status line at the top must say what is true today.
