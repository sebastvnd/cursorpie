// CursorPie Bridge: shared helpers (settings, remembered state, running code in a tab, the command
// registry). Loaded first by background.js; see docs/BROWSER_EXTENSION.md.
//
// Everything hangs on one global, `CP`, because the same files run in Firefox (an event page whose
// scripts the manifest lists) and in Chromium browsers (a service worker that `importScripts`
// them).
"use strict";

(function (g) {
  const CP = (g.CP = g.CP || {});
  CP.api = typeof browser !== "undefined" ? browser : chrome;
  const api = CP.api;

  CP.WEB = /^https?:\/\//i;
  // The most characters of an answer that go back to the app (a message read from a chat can be long).
  // The app shows at most 300 of them in a notification; a flow gets all of them.
  CP.MAX_REPLY = 4000;
  CP.sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  // ---------------------------------------------------------------- settings

  // What the options page edits. Every group of commands can be switched off.
  CP.DEFAULTS = {
    media: true, // media.* : play, pause, next...
    tabs: true, // tab.*, window.*, page.*
    chat: true, // chat.* : WhatsApp Web and Telegram Web
    ui: true, // ui.* : panels that ask something on the desktop (needs a setting in CursorPie)
    allowSend: false, // chat.send and chat.send_to really send; off: they only type the message
    allowRead: false, // chat.last, chat.read, chat.wait, chat.who, chat.unread_names, chat.folders read chats; off: they refuse
    autoPauseOthers: false, // starting one player pauses the others
    seekSeconds: 10,
    volumeStep: 10, // percent
    speedStep: 0.25,
    notify: "errors", // what the app shows after a command: "none", "errors" or "all"
    skipMuted: true, // chat.next_unread, chat.unread_names and the counts leave out muted chats
    templates: {}, // saved replies for chat.template: { "omw": "On my way, back at {time}" }
    selectors: {}, // overrides for the chat sites, see page-chat.js
  };

  let cached = Object.assign({}, CP.DEFAULTS);
  let loaded = null;

  CP.settings = () => cached;

  CP.loadSettings = () => {
    if (!loaded) {
      loaded = Promise.resolve(api.storage.local.get("settings"))
        .then((r) => {
          cached = Object.assign({}, CP.DEFAULTS, (r && r.settings) || {});
        })
        .catch(() => {});
    }
    return loaded;
  };

  if (api.storage && api.storage.onChanged) {
    api.storage.onChanged.addListener((changes, area) => {
      if (area === "local" && changes.settings) {
        cached = Object.assign({}, CP.DEFAULTS, changes.settings.newValue || {});
      }
    });
  }

  // ----------------------------------------------- remembered state (survives a restart)

  // A service worker is stopped when idle, so what the commands learn is kept in the session
  // storage where the browser has one (memory otherwise).
  CP.mem = {
    mru: {}, // windowId -> tab ids, most recent last
    audibleAt: {}, // tabId -> when it last made sound
    lastMedia: null, // tab that played last, what "play" resumes
  };
  const store = api.storage && api.storage.session;
  let saving = null;

  CP.saveMem = () => {
    if (!store || saving) return;
    saving = setTimeout(() => {
      saving = null;
      try {
        Promise.resolve(store.set({ mem: CP.mem })).catch(() => {});
      } catch (e) {
        /* no session storage */
      }
    }, 500);
  };

  CP.loadMem = async () => {
    if (!store) return;
    try {
      const r = await store.get("mem");
      if (r && r.mem && typeof r.mem === "object") {
        CP.mem = Object.assign(CP.mem, r.mem);
      }
    } catch (e) {
      /* no session storage */
    }
  };

  // ---------------------------------------------------------------- running code in a tab

  // Runs `func(...args)` in the page of `tabId` and gives back what it returns. `func` must be
  // self-contained (it is sent to the page as text) and must not be async: the Firefox path cannot
  // wait for a promise, so a command that needs to wait is split in steps, with `CP.sleep` in
  // between (see cmd-chat.js).
  CP.runInTab = async (tabId, func, args) => {
    args = args || [];
    if (api.scripting && api.scripting.executeScript) {
      const r = await api.scripting.executeScript({ target: { tabId }, func, args });
      return r && r[0] ? r[0].result : undefined;
    }
    const code = "(" + func.toString() + ").apply(null, " + JSON.stringify(args) + ")";
    const r = await api.tabs.executeScript(tabId, { code });
    return r && r[0];
  };

  // ---------------------------------------------------------------- the command registry

  CP.commands = {};

  // reg(id, group, run): `run({ arg, settings })` returns { ok, message, notify } (or throws).
  // The id is what the app sends (`media.next`) and what docs/BROWSER_EXTENSION.md lists. Keep
  // every id on one line like `reg("media.next", ...)`: a test in src/bridge/commands.rs reads
  // this file to check the app and the extension know the same commands.
  CP.reg = (id, group, run) => {
    CP.commands[id] = { id, group, run };
  };

  const GROUP_SWITCH = { media: "media", tab: "tabs", window: "tabs", page: "tabs", chat: "chat", ui: "ui" };

  CP.dispatch = async (id, arg) => {
    await CP.loadSettings();
    const c = CP.commands[id];
    if (!c) return { ok: false, message: "Unknown command: " + id };
    const group = id.split(".")[0];
    if (GROUP_SWITCH[group] && cached[GROUP_SWITCH[group]] === false) {
      return { ok: false, message: "'" + group + "' commands are switched off in the extension options." };
    }
    return c.run({ arg: (arg || "").trim(), settings: cached });
  };

  // ---------------------------------------------------------------- small helpers

  CP.ok = (message, notify) => ({ ok: true, message: message || "", notify: !!notify });
  CP.fail = (message) => ({ ok: false, message: message || "Did not work", notify: false });

  // What to tell the person when a script could not run in a page. The browser's own message
  // ("Cannot access contents of the page...") says nothing they can act on.
  CP.friendly = (e, tab) => {
    const text = String((e && e.message) || e || "");
    if (/cannot access|permission|host|missing host|not allowed/i.test(text)) {
      return (
        "The extension is not allowed on " +
        (CP.hostOf(tab && tab.url) || "this page") +
        ": open the CursorPie Bridge options and allow all sites"
      );
    }
    if (/no tab with id|tab.*(closed|not found)/i.test(text)) return "The tab was closed";
    return text || "Did not work";
  };

  // The number the user typed after a command, or `dflt`.
  CP.num = (arg, dflt) => {
    const n = parseFloat(String(arg || "").replace(",", "."));
    return Number.isFinite(n) ? n : dflt;
  };

  // The tab that has the user's attention: the active tab of the focused window.
  CP.currentTab = async () => {
    let tabs = await api.tabs.query({ active: true, lastFocusedWindow: true });
    if (!tabs.length) tabs = await api.tabs.query({ active: true });
    return tabs[0] || null;
  };

  // Goes to a tab and focuses its window.
  CP.show = async (tab) => {
    await api.tabs.update(tab.id, { active: true });
    await api.windows.update(tab.windowId, { focused: true });
  };

  // `https://www.youtube.com/watch?v=1` -> `youtube.com`
  CP.hostOf = (url) => {
    try {
      return new URL(url).hostname.replace(/^www\./, "").toLowerCase();
    } catch (e) {
      return "";
    }
  };

  CP.hostIs = (url, domain) => {
    const h = CP.hostOf(url);
    return h === domain || h.endsWith("." + domain);
  };

  // ---------------------------------------------------------------- reload after an update

  // After an update the app writes the files of the new version into the folder this extension was
  // loaded from, and asks it ("reload") to take them. `runtime.reload()` re-reads an unpacked
  // extension (and a Firefox temporary add-on) from its folder, so nobody has to go to the extensions
  // page. A folder that was not refreshed would give the same old version again, so each version is
  // acted on once: the version asked for is remembered, and a repeated request for it is ignored.
  // Resolves to whether it reloaded.
  CP.reloadFor = async (version) => {
    if (typeof version !== "string" || !version) return false;
    if (version === api.runtime.getManifest().version) return false;
    try {
      const got = await api.storage.local.get("reloadAsked");
      if (got && got.reloadAsked === version) return false;
      await api.storage.local.set({ reloadAsked: version });
    } catch (e) {
      return false;
    }
    api.runtime.reload();
    return true;
  };
})(typeof self !== "undefined" ? self : globalThis);
