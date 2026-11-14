// CursorPie Bridge: the browser side of CursorPie (see docs/developer/BROWSER_BRIDGE.md and
// docs/BROWSER_EXTENSION.md).
//
// Everything goes through native messaging with the CursorPie app on this computer (nothing goes
// over the network). In both directions:
//   extension -> app   "hello"   which browser, which version, which commands it knows
//                      "tabs"    the http(s) tabs that are open (address, tab id, window id)
//                      "state"   how much is playing and how many chats wait (numbers only)
//                      "result"  what happened to a command
//                      "panel", "panel_update", "panel_close"  show, change or close a panel (ui.js)
//   app -> extension   "activate" go to a tab        "open"    open a link
//                      "command"  do something in the browser: media.next, tab.close, chat.next_unread...
//                      "panel_result"  the answer to a panel
//                      "reload"   the app wrote the files of a newer version where this extension was
//                                 loaded from (after an update): reload to take them (CP.reloadFor)
//                      "sync"     the app just accepted this connection (it started after the browser,
//                                 or restarted): say hello and send the tabs now
//
// The commands live in cmd-tab.js, cmd-media.js, cmd-chat.js and cmd-ui.js; the code they run inside
// pages in page-media.js and page-chat.js; the panels in ui.js. The same files run in Firefox (an event page whose scripts the
// manifest lists) and in Chromium browsers (a service worker that loads them here).
"use strict";

if (typeof importScripts === "function") {
  importScripts(
    "util.js",
    "page-media.js",
    "page-chat.js",
    "ui.js",
    "cmd-tab.js",
    "cmd-media.js",
    "cmd-chat.js",
    "cmd-ui.js"
  );
}

(function (g) {
  const CP = g.CP;
  const api = CP.api;
  const HOST = "io.github.cursorpie.bridge"; // keep in sync with src/bridge/proto.rs
  const WEB = CP.WEB;

  let port = null;
  let who = null;
  let pending = null;

  async function browserName() {
    if (who) return who;
    const ua = navigator.userAgent || "";
    if (ua.includes("Firefox/")) who = "firefox";
    else if (ua.includes("Edg/")) who = "edge";
    else if (ua.includes("Vivaldi/")) who = "vivaldi";
    else if (ua.includes("OPR/")) who = "opera";
    else {
      let brave = false;
      try {
        brave = !!(navigator.brave && (await navigator.brave.isBrave()));
      } catch (e) {
        brave = false;
      }
      who = brave ? "brave" : "chrome";
    }
    return who;
  }

  function post(message) {
    if (!port) return;
    try {
      port.postMessage(message);
    } catch (e) {
      port = null;
    }
  }

  // ---------------------------------------------------------------- what the app is told

  async function sendTabs() {
    if (!port) return;
    let tabs = [];
    try {
      tabs = await api.tabs.query({});
    } catch (e) {
      return;
    }
    post({
      type: "tabs",
      tabs: tabs
        .filter((t) => t.url && WEB.test(t.url))
        .map((t) => ({ id: t.id, windowId: t.windowId, url: t.url, active: !!t.active })),
    });
    // the same list says how much is playing and how many chats wait: numbers, never titles
    const chats = [];
    for (const t of tabs) {
      const site = CP.chatSiteOf && CP.chatSiteOf(t.url);
      if (site && !chats.some((c) => c.site === site)) {
        chats.push({ site, unread: CP.titleCount(t.title) });
      }
    }
    post({
      type: "state",
      audible: tabs.filter((t) => t.audible).length,
      mediaTabs: tabs.filter((t) => CP.isPlayerUrl(t.url)).length,
      chats,
    });
  }

  // Many tab events come in bursts (a page loading, a window closing): one list after they settle.
  function scheduleTabs() {
    if (pending) return;
    pending = setTimeout(() => {
      pending = null;
      sendTabs();
    }, 250);
  }

  let lastTry = 0;

  // `force` skips the pause between two attempts: a tab event can come many times a second (a title
  // that changes), and each attempt starts a program while CursorPie is not running.
  function connect(force) {
    if (port) return;
    if (!force && Date.now() - lastTry < 5000) return;
    lastTry = Date.now();
    let p;
    try {
      p = api.runtime.connectNative(HOST);
    } catch (e) {
      return;
    }
    port = p;
    p.onMessage.addListener(onHostMessage);
    p.onDisconnect.addListener(() => {
      // Read the error so the browser does not log it: it is normal while CursorPie is not running.
      void api.runtime.lastError;
      if (port === p) port = null;
      // nobody is going to answer the panels that were open
      CP.ui.disconnected();
    });
    hello();
  }

  // Who we are and what is open. Sent when the port opens and again on "sync": the host keeps the
  // port open while CursorPie is not running, and drops what is said then, so the app asks again
  // (sync) as soon as it is there instead of waiting for a tab event.
  function hello() {
    return browserName().then((name) => {
      post({
        type: "hello",
        browser: name,
        version: api.runtime.getManifest().version,
        commands: Object.keys(CP.commands),
      });
      return sendTabs();
    });
  }

  function onTabEvent() {
    connect();
    scheduleTabs();
  }

  // ---------------------------------------------------------------- what the app asks

  async function activate(m) {
    try {
      await api.tabs.update(m.tabId, { active: true });
      await api.windows.update(m.windowId, { focused: true });
    } catch (e) {
      // The tab is gone: open the page instead of doing nothing.
      if (typeof m.url === "string" && WEB.test(m.url)) {
        await api.tabs.create({ url: m.url });
      }
    }
  }

  async function open(m) {
    if (typeof m.url !== "string" || !WEB.test(m.url)) return;
    if (m.newWindow) await api.windows.create({ url: m.url, focused: true });
    else await api.tabs.create({ url: m.url });
  }

  // Whether the app should show the person what a command said: a question they asked (the handler
  // says so with `notify`) is always shown; the rest follows the option "Notifications".
  function shouldShow(res, mode) {
    if (!res.message) return false;
    if (res.notify) return true;
    if (mode === "all") return true;
    return mode !== "none" && !res.ok;
  }

  async function command(m) {
    if (typeof m.id !== "number" || typeof m.cmd !== "string") return;
    let res;
    try {
      res = await CP.dispatch(m.cmd, typeof m.arg === "string" ? m.arg : "");
    } catch (e) {
      res = { ok: false, message: String((e && e.message) || e) };
    }
    post({
      type: "result",
      id: m.id,
      ok: !!res.ok,
      message: String(res.message || "").slice(0, CP.MAX_REPLY),
      notify: shouldShow(res, CP.settings().notify),
    });
  }

  function onHostMessage(m) {
    if (!m || typeof m !== "object") return;
    if (m.type === "activate") activate(m);
    else if (m.type === "open") open(m);
    else if (m.type === "command") command(m);
    else if (m.type === "panel_result") CP.ui.handle(m);
    else if (m.type === "reload") CP.reloadFor(m.version);
    else if (m.type === "sync") hello();
  }

  // Panels (ui.js) go to the app through the same connection; false when there is none.
  CP.ui.transport = (message) => {
    if (!port) return false;
    post(message);
    return !!port;
  };

  // ---------------------------------------------------------------- what the browser tells us

  // The order in which tabs were used, per window, for tab.recent.
  function noteActive(info) {
    const list = (CP.mem.mru[info.windowId] = (CP.mem.mru[info.windowId] || []).filter((id) => id !== info.tabId));
    list.push(info.tabId);
    if (list.length > 30) list.shift();
    CP.saveMem();
  }

  api.tabs.onCreated.addListener(onTabEvent);
  api.tabs.onRemoved.addListener((id, info) => {
    const list = CP.mem.mru[info && info.windowId];
    if (list) CP.mem.mru[info.windowId] = list.filter((t) => t !== id);
    delete CP.mem.audibleAt[id];
    onTabEvent();
  });
  api.tabs.onReplaced.addListener(onTabEvent);
  api.tabs.onAttached.addListener(onTabEvent);
  api.tabs.onDetached.addListener(onTabEvent);
  api.tabs.onActivated.addListener((info) => {
    noteActive(info);
    onTabEvent();
  });
  api.tabs.onUpdated.addListener((id, change) => {
    if (change.audible === true) {
      CP.mem.audibleAt[id] = Date.now();
      CP.mem.lastMedia = id;
      CP.saveMem();
      // the option "Starting one player pauses the others"
      if (CP.settings().autoPauseOthers) CP.pauseOthers(id).catch(() => {});
    }
    // the title carries the unread number of a chat page: "(3) WhatsApp"
    if (change.url || change.status === "complete" || change.audible !== undefined || change.title) onTabEvent();
  });

  // While CursorPie is not running the host cannot connect and the port closes at once; try again
  // every minute, and at every start of the browser.
  api.alarms.create("cursorpie-reconnect", { periodInMinutes: 1 });
  api.alarms.onAlarm.addListener(() => connect(true));
  if (api.runtime.onStartup) api.runtime.onStartup.addListener(() => connect(true));
  if (api.runtime.onInstalled) api.runtime.onInstalled.addListener(() => connect(true));

  CP.loadSettings();
  CP.loadMem().then(() => connect(true));
})(typeof self !== "undefined" ? self : globalThis);
