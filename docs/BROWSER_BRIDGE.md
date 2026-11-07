# Browser bridge: the optional extension for "Open URL" slices

An *Open URL* slice with an `http(s)` link offers the choice **new window / the page that is open** (see
`docs/OPEN_URL_CHOICE.md`) **only with this extension**: a browser does not tell other programs which tabs it has, and
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

## Turning it on

1. Settings > Behavior > **Browser extension**: switch on *Know which web pages are open* and **save the settings**
   (Cmd+S / Ctrl+S; until then the line under the switch says "Not started yet"). CursorPie writes `bridge.json`
   (port and secret) in its support folder, listens on `127.0.0.1` and registers the host with the installed browsers.
2. **Show the extension files**, then add the extension once:
   * Chrome, Edge, Brave, Vivaldi: the extensions page, *Developer mode*, *Load unpacked*, the `chromium` folder;
   * Firefox: `about:debugging`, *This Firefox*, *Load Temporary Add-on*, `manifest.json` of the `firefox` folder. A
     temporary add-on is forgotten when Firefox closes. A permanent one needs a **signed** copy (see below).
3. The extension connects within a minute (it retries every minute while CursorPie is not running). The line under the
   switch then reads *Connected: Chrome (12 tabs)*. If it does not: reload the extension, or restart the browser.
4. Open a page, then hover its *Open URL* slice: the two badges appear.

Turning the switch off stops the server, removes `bridge.json` and removes the host registration.

## Privacy and security

* The extension sends the **address** of `http(s)` tabs (and tab and window numbers) to CursorPie on the same computer.
  No page content, no titles, nothing over the network. Private windows are not included unless the person allowed the
  extension there.
* The browser decides who may start the host: the manifest names the extension (`allowed_origins` with the fixed id
  `mbfoipmefaenedenjnepgcdkgkmefpnk`, `allowed_extensions` with `browser-bridge@cursorpie`).
* The host reaches the app only on `127.0.0.1`, after sending the secret of `bridge.json`. The secret is random per run;
  the file is readable by the user only on Linux and macOS (on Windows it lives in the user's own `%APPDATA%`).
* The app takes only data from the extension (a list of tabs). The extension opens only `http(s)` links.

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
* Tab matching is by address, then by site and path, then by site (the rules are in `docs/OPEN_URL_CHOICE.md`).

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
