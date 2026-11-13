# "Open URL" slices: new window or the page that is open

An *Open app* slice offers two round badges when its app runs: a green **+** (a new window) and a yellow one (the app
that is open). An *Open URL* slice with an `http(s)` link offers the same two badges, **only with the browser
extension** (Settings > Behavior > Browser extension, see `docs/developer/BROWSER_BRIDGE.md`).

| | What the slice does |
|---|---|
| extension off, or no browser connected | opens the link, no badges, as it always did |
| extension on, a tab has that page | hovering shows the two badges: green **+** = the link in a **new browser window**, yellow = **go to that tab** |
| extension on, no tab has it | opens the link, no badges |
| a file path, `mailto:` or another scheme | opened as it always was |

## Why only with the extension

A browser does not tell other programs which tabs it has. Guessing (for example "CursorPie opened that link earlier")
was tried first and opened the same page twice as often as it helped, so it is not used: without the extension there is
no way to know, and the slice does not pretend to.

## When a tab "is" the page of a link

The extension reports the address of each `http(s)` tab. A tab is the page of a link when, in this order of preference:

1. it has the same address (scheme and host in any case, `#fragment` and a trailing `/` ignored);
2. it has the same site and path, whatever the scheme (`http`/`https`), a leading `www.`, the query (`?...`) or the
   `#fragment`: a page reached by a redirect, or with a tracking parameter;
3. the link names a whole site (`https://youtube.com`) and the tab is on that site.

Among equals the tab that is showing wins, then the browser heard from last. A link that names a page
(`https://github.com/foo/bar`) does not match another page of the site.

## How it works

```
Action::open_choice_key(bridge)   an app, or a web link when the extension is on
        |  OpenTarget::key()      app as typed, or  url:<link>
        v
running::warm -> worker -> ask_platform
        |                     |- apps  -> platform::running_among     (as before)
        |                     `- links -> platform::web::urls_open    (bridge::urls_open)
        v
tap::resolve::eval  -> badges (ui::pie, unchanged) -> actions::open_web
                                                        |- None      -> platform::open_url
                                                        |- NewWindow -> platform::web::open_new_window
                                                        `- Existing  -> platform::web::reveal
```

* `config::model`: `OpenTarget`, `is_web_url`, `Action::open_choice_target`, `Action::open_choice_key`.
* `config::tree::open_apps_of` lists links only when `Config::browser_bridge` is on, so a pie without the extension costs
  nothing extra per press.
* `bridge`: the list of tabs and `find_tab` (the matching above). `platform::web` is the same on every OS.

## Tests

`config::tree` (links are asked about only with the extension), `actions` (choice target), `running` (key split, cache),
`platform::web` (normalizing, site and path), `bridge` (matching, redirects, ties).
