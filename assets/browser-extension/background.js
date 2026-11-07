// CursorPie Bridge: the browser side of the "Open URL" choice (see docs/BROWSER_BRIDGE.md).
//
// It does three things, all through native messaging with the CursorPie app on this computer
// (nothing goes over the network):
//   1. tells the app which http(s) tabs are open (url, tab id, window id; never page content),
//   2. goes to a tab when the app asks ("activate"),
//   3. opens a link in a new window or tab when the app asks ("open").
// The same file runs in Firefox (event page) and in Chromium browsers (service worker).
"use strict";

const HOST = "io.github.cursorpie.bridge"; // keep in sync with src/bridge/proto.rs
const api = typeof browser !== "undefined" ? browser : chrome;
const WEB = /^https?:\/\//i;

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
}

// Many tab events come in bursts (a page loading, a window closing): one list after they settle.
function scheduleTabs() {
  if (pending) return;
  pending = setTimeout(() => {
    pending = null;
    sendTabs();
  }, 250);
}

function connect() {
  if (port) return;
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
  });
  browserName().then((name) => {
    post({ type: "hello", browser: name, version: api.runtime.getManifest().version });
    sendTabs();
  });
}

function onTabEvent() {
  connect();
  scheduleTabs();
}

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

function onHostMessage(m) {
  if (!m || typeof m !== "object") return;
  if (m.type === "activate") activate(m);
  else if (m.type === "open") open(m);
}

api.tabs.onCreated.addListener(onTabEvent);
api.tabs.onRemoved.addListener(onTabEvent);
api.tabs.onReplaced.addListener(onTabEvent);
api.tabs.onAttached.addListener(onTabEvent);
api.tabs.onDetached.addListener(onTabEvent);
api.tabs.onActivated.addListener(onTabEvent);
api.tabs.onUpdated.addListener((id, change) => {
  if (change.url || change.status === "complete") onTabEvent();
});

// While CursorPie is not running the host cannot connect and the port closes at once; try again
// every minute, and at every start of the browser.
api.alarms.create("cursorpie-reconnect", { periodInMinutes: 1 });
api.alarms.onAlarm.addListener(() => connect());
if (api.runtime.onStartup) api.runtime.onStartup.addListener(connect);
if (api.runtime.onInstalled) api.runtime.onInstalled.addListener(connect);

connect();
