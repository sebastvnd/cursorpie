# Browser bridge: the optional extension for "Open URL" slices

> The extension can also control the browser (media, tabs, WhatsApp / Telegram Web): see `docs/BROWSER_EXTENSION.md`.

An *Open URL* slice with an `http(s)` link offers the choice **new window / the page that is open** (see
`docs/developer/OPEN_URL_CHOICE.md`) **only with this extension**: a browser does not tell other programs which tabs it has, and
guessing opens the same page twice.

| | Without the extension (default) | With the extension (Settings > Behavior > Browser extension) |
|---|---|---|
| Open URL slice | opens the link, no badges | shows the badges when a tab has that page |
| Yellow badge | - | goes to **that tab** and focuses its window |
| Green badge | - | a real new window, on every OS |
| Installs | nothing | the extension, once per browser |

## What ships in the program

Everything is built into the one executable. There is no second program and nothing to download.

| Piece | Where | Role |
|---|---|---|
| Extension | `assets/browser-extension/` (`background.js`, `manifest.chromium.json`, `manifest.firefox.json`), compiled in by `src/bridge/extension.rs` | reports open `http(s)` tabs, goes to a tab, opens a window |
| Native messaging host | `cursorpie` itself: `bridge::host`, recognized by the arguments the browser passes | relay with no window between the browser and the app |
| Host registration | `bridge::install` | a manifest file (Linux, macOS) or a registry key (Windows), for each installed browser |
| Server | `bridge` (threads `cursorpie-bridge*`) | token-protected loopback socket, the list of tabs |

```
extension ──native messaging (stdin/stdout)──▶ cursorpie --(started by the browser)── loopback TCP ──▶ running app
```

The extension version is the version of the build: `{{VERSION}}` in the manifests is replaced when the files are written
(`--export-extension`, or the **Show the extension files** button), so a version bump needs no edit there.

**After an update** the files are written again by the program (`src/refresh.rs`, at the first start of the new version,
before the bridge starts) when the folder was exported to before. A browser keeps running the old code until the extension
is reloaded, so when an extension says `hello` with another version than the build and the folder it most likely was loaded
from (`<support folder>/browser-extension/chromium` or `firefox`) holds the build's files, the app sends it
`{"type":"reload","version":"<build>"}` (`bridge::ask_to_reload_in`). The extension calls `runtime.reload()` once per version
(`CP.reloadFor` remembers the version in `storage.local`), and the app asks a browser at most 3 times per run, so an
extension loaded from another folder, which stays old, cannot be made to reload for ever. An extension from before this
message ignores it, so the update that first brings the message still needs one manual reload.

## Turning it on

1. Settings > Behavior > **Browser extension**: switch on *Know which web pages are open* and **save the settings**
   (Cmd+S / Ctrl+S; until then the line under the switch says "Not started yet"). CursorPie writes `bridge.json`
   (port and secret) in its support folder, listens on `127.0.0.1` and registers the host with the installed browsers.
2. **Show the extension files**, then add the extension once:
   * Chrome, Edge, Brave, Vivaldi: the extensions page, *Developer mode*, *Load unpacked*, the `chromium` folder;
   * Firefox: `about:debugging`, *This Firefox*, *Load Temporary Add-on*, `manifest.json` of the `firefox` folder. A
     temporary add-on is forgotten when Firefox closes. A permanent one needs a **signed** copy (see below).
3. The extension connects at once when CursorPie is running, and a moment after it starts otherwise: the host the browser
   started waits for the app and the app sends `{"type":"sync"}` to each new connection, which makes the extension say
   `hello` and send the open tabs without waiting for a tab event. The line under the
   switch then reads *Connected: Chrome (12 tabs)*. If it does not: reload the extension, or restart the browser.
4. Open a page, then hover its *Open URL* slice: the two badges appear.

Turning the switch off stops the server, removes `bridge.json` and removes the host registration.

## Privacy and security

* The extension sends the **address** of `http(s)` tabs (and tab and window numbers) to CursorPie on the same computer,
  and since the commands of `docs/BROWSER_EXTENSION.md` also how many tabs make sound and how many chats wait (numbers
  only). No page content, no titles, nothing over the network. Private windows are not included unless the person allowed
  the extension there.
* The browser decides who may start the host: the manifest names the extension (`allowed_origins` with the fixed id
  `mbfoipmefaenedenjnepgcdkgkmefpnk`, `allowed_extensions` with `browser-bridge@cursorpie`).
* The host reaches the app only on `127.0.0.1`, after sending the secret of `bridge.json`. The secret is random per run;
  the file is readable by the user only on Linux and macOS (on Windows it lives in the user's own `%APPDATA%`).
* The app takes only data from the extension (a list of tabs, counts, the result of a command). The extension opens only
  `http(s)` links, and the app asks it only for the commands of `docs/BROWSER_EXTENSION.md`.

## Panels

Besides tabs, state and commands, the extension may ask CursorPie to show a **panel** (`docs/PANELS.md`) and gets the
answer back: `panel`, `panel_update` and `panel_close` from the extension, `panel_result` from the app. The app refuses
every one of them (answering `unavailable` with the reason) unless **both** "Know which web pages are open..." and
"Let the browser extension show panels" are on in Settings, checks the description (size, limits, ids) before drawing,
allows at most 4 panels waiting at once and 5 minutes for each, and lets an extension update or close only the panels
it started. What an extension puts in a panel (for example tab titles in `ui.tabs`) is shown on screen only: it is not
stored and not sent anywhere, and the *Privacy* rule above about titles still holds for everything the extension sends
on its own.

## Browser by browser

| Browser | Linux | macOS | Windows |
|---|---|---|---|
| Chrome, Chromium | `~/.config/<browser>/NativeMessagingHosts/` | `~/Library/Application Support/<browser>/NativeMessagingHosts/` | `HKCU\Software\Google\Chrome\NativeMessagingHosts\<name>` (Chromium: `...\Chromium\...`) |
| Edge, Brave, Vivaldi | same pattern | same pattern | same pattern (own key) |
| Firefox | `~/.mozilla/native-messaging-hosts/` | `~/Library/Application Support/Mozilla/NativeMessagingHosts/` | `HKCU\SOFTWARE\Mozilla\NativeMessagingHosts\<name>` |

The Chrome, Chromium and Firefox locations and the manifest fields come from the browsers' documentation. The Edge, Brave
and Vivaldi folders follow the same pattern and **have not been tried**. Browsers in a sandbox (Flatpak, Snap) cannot start
a program outside it and are not covered.

## What is not done, or not verified

* **Not run on a real browser yet.** The code was written without a compiler or a browser at hand: build it and try the
  three things (open, go to the tab, new window) on Linux, macOS and Windows before a release.
* **Windows**: the release build has no console (`windows_subsystem = "windows"`). A native messaging host started by a
  browser should still receive its pipes, but this is the first thing to check. If it does not, the host needs a console
  build (a second small executable, or a `.bat` wrapper that Chromium accepts).
* **Wayland**: an extension cannot take the focus for a window there. After the tab is selected, CursorPie asks KWin (or the
  X11 server) to raise the browser, using the desktop ids in `platform::web::linux_app_ids`. Whether `bt`-style tab
  selection plus that raise gives the right window on every compositor is to be tried.
* **Releasing the extension.** A permanent install needs the stores: Chrome Web Store (the id there is assigned by the
  store, so add it to `extension::CHROMIUM_ID` handling in `install::manifest_json`) and a Firefox add-on **signed** by
  Mozilla (unlisted signing is enough: `web-ext sign` on the `firefox` folder). `scripts/package-extension.sh` writes the
  two zips from the built program.
* Tab matching is by address, then by site and path, then by site (the rules are in `docs/developer/OPEN_URL_CHOICE.md`).

## Command line

```
cursorpie --export-extension [DIR]     write the extension (default: <support folder>/browser-extension)
cursorpie --install-browser-bridge     register the host with the installed browsers
cursorpie --uninstall-browser-bridge   remove that registration
cursorpie --native-host                what a browser starts; for trying the link by hand
```

## Tests

`bridge::proto` (framing and the messages), `bridge::extension` (manifests valid, version, ids, export), `bridge::install`
(manifests, folders per OS, only installed browsers), `bridge::host` (recognizing a browser's start), `bridge` (tabs,
which browser wins, commands), `config` (the setting), `ui::settings::tabs::behavior` (status line).
