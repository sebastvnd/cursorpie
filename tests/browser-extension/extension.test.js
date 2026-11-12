// Tests of the browser extension (assets/browser-extension/), run with:
//
//     cd tests/browser-extension && npm install && npm test
//
// The extension runs in a real browser only, so these tests give it the next best thing: the page
// scripts run against small copies of the pages in jsdom, and the background script runs against a
// fake `chrome` API whose tabs are those jsdom pages. They check the logic (which tab a command
// acts on, what is pressed, what is refused). They cannot check that YouTube, Spotify, WhatsApp
// Web or Telegram Web still look the way the selectors expect: that needs a browser and an
// account (see docs/BROWSER_EXTENSION.md, "What is tested").
"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { JSDOM } = require("jsdom");

const DIR = path.join(__dirname, "..", "..", "assets", "browser-extension");
const read = (f) => fs.readFileSync(path.join(DIR, f), "utf8");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const SCRIPTS = ["util.js", "page-media.js", "page-chat.js", "ui.js", "cmd-tab.js", "cmd-media.js", "cmd-chat.js", "cmd-ui.js"];

// ---------------------------------------------------------------------------------- pages

// A page: jsdom with the layout stubs the scripts rely on (jsdom does no layout).
function page(url, html) {
  const dom = new JSDOM(`<!doctype html><body>${html}</body>`, { url, pretendToBeVisual: true, runScripts: "outside-only" });
  const w = dom.window;
  w.Element.prototype.getClientRects = function () {
    return [{ width: 1, height: 1 }];
  };
  // a media element that remembers what was asked of it
  w.HTMLMediaElement.prototype.play = function () {
    this._paused = false;
    return Promise.resolve();
  };
  w.HTMLMediaElement.prototype.pause = function () {
    this._paused = true;
  };
  Object.defineProperty(w.HTMLMediaElement.prototype, "paused", {
    get() {
      return this._paused !== false;
    },
  });
  return dom;
}

function video(dom, { playing = false, duration = 200, time = 0 } = {}) {
  const v = dom.window.document.querySelector("video");
  v._paused = !playing;
  Object.defineProperty(v, "duration", { value: duration, configurable: true });
  Object.defineProperty(v, "readyState", { value: 4, configurable: true });
  v.currentTime = time;
  return v;
}

// loads the page scripts (and nothing else) into a fresh context, for calling CP.pageMedia directly
function loadPageFns() {
  const ctx = vm.createContext({ console, setTimeout, URL });
  ctx.self = ctx;
  for (const f of ["util.js", "page-media.js", "page-chat.js"]) {
    // util.js wants a chrome-like api at load time
    if (f === "util.js") {
      ctx.chrome = { storage: { local: { get: async () => ({}) }, onChanged: { addListener() {} } } };
    }
    vm.runInContext(read(f), ctx, { filename: f });
  }
  return ctx.CP;
}
const CPpage = loadPageFns();

// runs one of the page functions inside a jsdom window, the way scripting.executeScript does
const inPage = (dom, fn, args) => dom.window.eval("(" + fn.toString() + ")")(...args);

// ---------------------------------------------------------------------------------- media pages

test("media: toggle pauses a playing video and plays a paused one", () => {
  const dom = page("https://www.youtube.com/watch?v=1", "<video></video>");
  const v = video(dom, { playing: true });
  let r = inPage(dom, CPpage.pageMedia, ["toggle", {}]);
  assert.equal(r.ok, true);
  assert.equal(v.paused, true);
  assert.equal(r.playing, false);
  r = inPage(dom, CPpage.pageMedia, ["toggle", {}]);
  assert.equal(v.paused, false);
  assert.equal(r.playing, true);
  // "play" on something that plays is not an error and does not toggle it back
  r = inPage(dom, CPpage.pageMedia, ["play", {}]);
  assert.equal(v.paused, false);
  assert.match(r.msg, /Already/);
});

test("media: YouTube next presses the player's next button, previous restarts a single video", () => {
  const dom = page(
    "https://www.youtube.com/watch?v=1",
    "<video></video><button class='ytp-next-button' id='n'></button>"
  );
  const v = video(dom, { playing: true, time: 50 });
  let clicked = 0;
  dom.window.document.getElementById("n").addEventListener("click", () => clicked++);
  assert.equal(inPage(dom, CPpage.pageMedia, ["next", {}]).ok, true);
  assert.equal(clicked, 1);
  const r = inPage(dom, CPpage.pageMedia, ["prev", {}]);
  assert.equal(r.ok, true);
  assert.equal(v.currentTime, 0, "no previous button: the track starts again");
});

test("media: a disabled or hidden next button is not pressed", () => {
  const dom = page(
    "https://www.youtube.com/watch?v=1",
    "<video></video><button class='ytp-next-button' style='display:none'></button>"
  );
  video(dom, { playing: true });
  const r = inPage(dom, CPpage.pageMedia, ["next", {}]);
  assert.equal(r.ok, false);
});

test("media: YouTube next goes to the suggested video when the player has no next button", () => {
  const dom = page(
    "https://www.youtube.com/watch?v=1",
    "<video></video><div><a id='s' href='/watch?v=2'>x</a></div>"
  );
  video(dom, { playing: true });
  // the suggestion list element name used by the selector
  dom.window.document.body.innerHTML += "";
  const el = dom.window.document.createElement("ytd-compact-video-renderer");
  el.innerHTML = "<a id='sug' href='/watch?v=3'>y</a>";
  dom.window.document.body.appendChild(el);
  let hit = false;
  dom.window.document.getElementById("sug").addEventListener("click", (e) => {
    e.preventDefault();
    hit = true;
  });
  const r = inPage(dom, CPpage.pageMedia, ["next", {}]);
  assert.equal(r.ok, true);
  assert.equal(hit, true);
});

test("media: seek stays inside the track, volume and speed stay in range", () => {
  const dom = page("https://vimeo.example/", "<video></video>");
  const v = video(dom, { playing: true, duration: 100, time: 95 });
  inPage(dom, CPpage.pageMedia, ["seek", { by: 10 }]);
  assert.equal(v.currentTime, 100);
  inPage(dom, CPpage.pageMedia, ["seek", { by: -500 }]);
  assert.equal(v.currentTime, 0);
  v.volume = 0.95;
  inPage(dom, CPpage.pageMedia, ["volume", { by: 10 }]);
  assert.equal(v.volume, 1);
  inPage(dom, CPpage.pageMedia, ["volume", { by: -300 }]);
  assert.equal(v.volume, 0);
  v.playbackRate = 3.9;
  inPage(dom, CPpage.pageMedia, ["speed", { by: 0.25 }]);
  assert.equal(v.playbackRate, 4);
  inPage(dom, CPpage.pageMedia, ["speed", { to: 1 }]);
  assert.equal(v.playbackRate, 1);
});

test("media: a live stream cannot be sought", () => {
  const dom = page("https://live.example/", "<video></video>");
  video(dom, { playing: true, duration: Infinity });
  assert.equal(inPage(dom, CPpage.pageMedia, ["seek", { by: 10 }]).ok, false);
});

test("media: volume up un-mutes", () => {
  const dom = page("https://a.example/", "<video></video>");
  const v = video(dom, { playing: true });
  v.muted = true;
  v.volume = 0.5;
  inPage(dom, CPpage.pageMedia, ["volume", { by: 10 }]);
  assert.equal(v.muted, false);
});

test("media: Spotify has no usable element, so its own buttons are pressed and its label says if it plays", () => {
  const dom = page(
    "https://open.spotify.com/",
    `<button data-testid='control-button-playpause' aria-label='Pause'></button>
     <button data-testid='control-button-skip-forward'></button>`
  );
  const seen = [];
  for (const id of ["control-button-playpause", "control-button-skip-forward"]) {
    dom.window.document.querySelector(`[data-testid='${id}']`).addEventListener("click", () => seen.push(id));
  }
  assert.equal(inPage(dom, CPpage.pageMedia, ["info", {}]).playing, true, "label 'Pause' = it plays");
  inPage(dom, CPpage.pageMedia, ["toggle", {}]);
  inPage(dom, CPpage.pageMedia, ["next", {}]);
  assert.deepEqual(seen, ["control-button-playpause", "control-button-skip-forward"]);
});

test("media: on an unknown site 'next' only looks near the player, never at the page's own pagination", () => {
  const dom = page(
    "https://shop.example/",
    `<a href='?page=2' role='button' aria-label='Next'>Next</a>
     <section><div class='player'><video></video><button aria-label='Next track' id='n'></button></div></section>`
  );
  video(dom, { playing: true });
  let hits = 0;
  dom.window.document.getElementById("n").addEventListener("click", () => hits++);
  assert.equal(inPage(dom, CPpage.pageMedia, ["next", {}]).ok, true);
  assert.equal(hits, 1);
  const bare = page("https://shop.example/", "<a role='button' aria-label='Next'>Next</a><video></video>");
  video(bare, { playing: true });
  assert.equal(inPage(bare, CPpage.pageMedia, ["next", {}]).ok, false);
});

test("media: skip ad presses only the site's own skip button", () => {
  const dom = page("https://www.youtube.com/watch?v=1", "<video></video><button class='ytp-skip-ad-button'></button>");
  video(dom, { playing: true });
  let n = 0;
  dom.window.document.querySelector(".ytp-skip-ad-button").addEventListener("click", () => n++);
  assert.equal(inPage(dom, CPpage.pageMedia, ["skipad", {}]).ok, true);
  assert.equal(n, 1);
  const none = page("https://www.youtube.com/watch?v=1", "<video></video>");
  video(none, { playing: true });
  assert.equal(inPage(none, CPpage.pageMedia, ["skipad", {}]).ok, false);
});

test("media: what is playing comes from the page's Media Session", () => {
  const dom = page("https://music.example/", "<video></video>");
  video(dom, { playing: true });
  dom.window.navigator.mediaSession = { metadata: { title: "Song", artist: "Band" }, playbackState: "playing" };
  const r = inPage(dom, CPpage.pageMedia, ["info", {}]);
  assert.equal(r.title, "Song");
  assert.equal(r.artist, "Band");
  assert.equal(r.playing, true);
});

// ---------------------------------------------------------------------------------- chat pages

// jsdom has no editing: execCommand puts the text in the editable box that holds the selection
function typeInto(dom) {
  return (cmd, _ui, text) => {
    const sel = dom.window.getSelection();
    const node = sel.anchorNode && (sel.anchorNode.nodeType === 1 ? sel.anchorNode : sel.anchorNode.parentElement);
    const box = node && node.closest("[contenteditable]");
    if (!box) return false;
    if (cmd === "insertText") box.textContent += text;
    if (cmd === "insertLineBreak") box.textContent += "\n";
    return true;
  };
}

const WA = `
<div id="side"><div contenteditable="true" data-tab="3" id="search"></div><div id="pane-side">
  <div role="listitem" id="c1"><span title="Anna">Anna</span><span title="hi">hi</span></div>
  <div role="listitem" id="c2"><span title="Budi">Budi</span><span aria-label="3 unread messages">3</span></div>
  <div role="listitem" id="c3"><span title="Citra">Citra</span>
     <span style="background-color: rgb(37, 211, 102)">12</span></div>
  <div role="listitem" id="c4"><span title="2024">2024</span></div>
</div></div>
<div id="main"><header><span title="Budi" id="hd">Budi</span></header>
  <footer><div contenteditable="true" data-tab="10" id="box"></div>
  <button aria-label="Send" id="send"><span data-icon="send"></span></button></footer></div>`;

function whatsapp(selectedId) {
  const dom = page("https://web.whatsapp.com/", WA);
  const d = dom.window.document;
  if (selectedId) d.getElementById(selectedId).setAttribute("aria-selected", "true");
  const log = { clicked: [], sent: 0 };
  for (const id of ["c1", "c2", "c3", "c4"]) {
    d.getElementById(id).addEventListener("click", () => log.clicked.push(id));
  }
  d.getElementById("send").addEventListener("click", () => log.sent++);
  // jsdom has no editing: execCommand puts the text in the box
  d.execCommand = typeInto(dom);
  return { dom, log, d };
}
const tablesOf = () => JSON.parse(JSON.stringify(CPpage.CHAT_SELECTORS));
const chatCall = (dom, action, opts) => inPage(dom, CPpage.pageChat, [action, opts, tablesOf()]);

test("chat: unread chats are found by the badge and by what a badge looks like", () => {
  const { dom } = whatsapp();
  const r = chatCall(dom, "info", {});
  assert.equal(r.ready, true);
  assert.equal(r.chats, 4);
  // c2 has a labelled badge, c3 only a green number; "2024" in a name is not a badge
  assert.equal(r.unreadChats, 2);
  assert.equal(r.unreadMessages, 15);
});

test("chat: next unread goes round the list, previous goes back", () => {
  let { dom, log } = whatsapp("c1");
  assert.equal(chatCall(dom, "open_unread", { dir: 1 }).ok, true);
  assert.deepEqual(log.clicked.slice(-1), ["c2"]);
  ({ dom, log } = whatsapp("c3"));
  chatCall(dom, "open_unread", { dir: 1 });
  assert.deepEqual(log.clicked.slice(-1), ["c2"], "after the last unread it wraps to the first");
  ({ dom, log } = whatsapp("c2"));
  chatCall(dom, "open_unread", { dir: -1 });
  assert.deepEqual(log.clicked.slice(-1), ["c3"], "before the first unread it wraps to the last");
});

test("chat: with nothing unread it says so and clicks nothing", () => {
  const { dom, log, d } = whatsapp();
  d.getElementById("c2").querySelector("[aria-label]").remove();
  d.getElementById("c3").querySelector("[style]").remove();
  const r = chatCall(dom, "open_unread", { dir: 1 });
  assert.equal(r.ok, false);
  assert.equal(r.found, false);
  assert.equal(log.clicked.length, 0);
});

test("chat: next and previous chat stop at the ends of the list", () => {
  let { dom, log } = whatsapp("c4");
  chatCall(dom, "open_adjacent", { dir: 1 });
  assert.deepEqual(log.clicked, ["c4"], "no chat after the last one");
  ({ dom, log } = whatsapp("c2"));
  chatCall(dom, "open_adjacent", { dir: -1 });
  assert.deepEqual(log.clicked, ["c1"]);
});

test("chat: a chat is picked by name, the exact name first", () => {
  const { dom, log } = whatsapp();
  assert.equal(chatCall(dom, "pick", { name: "budi" }).found, true);
  assert.deepEqual(log.clicked, ["c2"]);
  assert.equal(chatCall(dom, "pick", { name: "Zed" }).found, false);
  assert.equal(log.clicked.length, 1, "no match, no click");
});

test("chat: draft adds to what is already typed, and only send presses Send", () => {
  const { dom, log, d } = whatsapp();
  d.getElementById("box").textContent = "half a sentence, ";
  assert.equal(chatCall(dom, "draft", { text: "on my way\nsoon" }).ok, true);
  assert.equal(d.getElementById("box").textContent, "half a sentence, on my way\nsoon");
  assert.equal(log.sent, 0, "typing never sends");
  assert.equal(chatCall(dom, "send", {}).ok, true);
  assert.equal(log.sent, 1);
  assert.equal(chatCall(dom, "header", {}).name, "Budi");
});

test("chat: a selector override is tried before the built-in ones", () => {
  const { dom, d } = whatsapp();
  d.getElementById("main").insertAdjacentHTML("beforeend", "<div id='other' contenteditable='true' class='mine'></div>");
  const t = tablesOf();
  t.whatsapp.composer.unshift(".mine");
  inPage(dom, CPpage.pageChat, ["draft", { text: "x" }, t]);
  assert.equal(d.getElementById("other").textContent, "x");
});

test("chat: another site is not a chat page", () => {
  const dom = page("https://example.com/", "<p>x</p>");
  assert.equal(chatCall(dom, "info", {}).ok, false);
});

// ---------------------------------------------------------------------------------- the background

// A fake browser: tabs are { id, windowId, url, title, ... } with an optional jsdom page behind them.
function browser(tabList, settings) {
  const state = { tabs: tabList.map((t, i) => Object.assign({ index: i, active: false, audible: false, pinned: false, windowId: 1, title: "" }, t)) };
  const calls = [];
  const posted = [];
  const listeners = {};
  const ev = (name) => ({ addListener: (f) => (listeners[name] = f) });
  const find = (id) => state.tabs.find((t) => t.id === id);
  const stored = { settings: settings || {} };
  const chrome = {
    runtime: {
      connectNative: () => ({
        postMessage: (m) => posted.push(m),
        onMessage: { addListener: (f) => (listeners.host = f) },
        onDisconnect: { addListener() {} },
      }),
      getManifest: () => ({ version: "9.9.9" }),
      lastError: undefined,
      onStartup: ev("startup"),
      onInstalled: ev("installed"),
    },
    alarms: { create() {}, onAlarm: ev("alarm") },
    storage: {
      local: { get: async (k) => ({ [k]: stored[k] }), set: async (o) => Object.assign(stored, o) },
      onChanged: { addListener() {} },
    },
    permissions: { contains: async () => false },
    sessions: { restore: async () => calls.push(["restore"]) },
    tabs: {
      onCreated: ev("created"), onRemoved: ev("removed"), onReplaced: ev("replaced"), onAttached: ev("attached"),
      onDetached: ev("detached"), onActivated: ev("activated"), onUpdated: ev("updated"),
      query: async (q) =>
        state.tabs.filter((t) => {
          if (q.audible !== undefined && t.audible !== q.audible) return false;
          if (q.active !== undefined && t.active !== q.active) return false;
          if (q.windowId !== undefined && t.windowId !== q.windowId) return false;
          return true;
        }),
      get: async (id) => {
        const t = find(id);
        if (!t) throw new Error("No tab with id: " + id);
        return t;
      },
      update: async (id, p) => {
        calls.push(["update", id, p]);
        const t = find(id);
        if (p.active) state.tabs.forEach((x) => (x.active = x.windowId === t.windowId ? x.id === id : x.active));
        Object.assign(t, p.active ? {} : p);
      },
      remove: async (ids) => {
        ids = [].concat(ids);
        calls.push(["remove", ids]);
        state.tabs = state.tabs.filter((t) => !ids.includes(t.id));
        state.tabs.forEach((t, i) => (t.index = i));
      },
      create: async (p) => calls.push(["create", p]),
      move: async (id, p) => calls.push(["move", id, p]),
    },
    windows: { update: async (id, p) => calls.push(["win", id, p]) },
    scripting: {
      executeScript: async ({ target, func, args }) => {
        const t = find(target.tabId);
        if (!t || !t.dom) throw new Error("Cannot access contents of the page. Extension manifest must request permission");
        return [{ result: t.dom.window.eval("(" + func.toString() + ")")(...args) }];
      },
    },
  };
  return { chrome, state, calls, posted, listeners, stored };
}

// loads the whole extension into a fresh context and waits for it to connect
async function extension(tabs, settings) {
  const b = browser(tabs, settings);
  const ctx = vm.createContext({ chrome: b.chrome, console, setTimeout, clearTimeout, URL, navigator: { userAgent: "Chrome/120" } });
  ctx.self = ctx;
  for (const f of SCRIPTS.concat("background.js")) vm.runInContext(read(f), ctx, { filename: f });
  await sleep(30);
  const CP = ctx.CP;
  const run = async (cmd, arg) => {
    await CP.loadSettings();
    return CP.dispatch(cmd, arg || "");
  };
  // what the app does: send a command line and wait for its result
  let seq = 0;
  const ask = async (cmd, arg) => {
    const id = ++seq;
    b.listeners.host({ type: "command", id, cmd, arg: arg || "" });
    for (let i = 0; i < 200; i++) {
      const r = b.posted.find((m) => m.type === "result" && m.id === id);
      if (r) return r;
      await sleep(10);
    }
    throw new Error("no result for " + cmd);
  };
  return Object.assign(b, { CP, run, ask });
}

const yt = (extra) => page("https://www.youtube.com/watch?v=1", "<video></video><button class='ytp-next-button' id='n'></button>" + (extra || ""));

test("background: hello lists the commands, state counts players and chats and never sends a title", async () => {
  const dom = yt();
  const e = await extension([
    { id: 1, url: "https://www.youtube.com/watch?v=1", title: "Secret video - YouTube", audible: true, dom },
    { id: 2, url: "https://web.whatsapp.com/", title: "(3) WhatsApp" },
    { id: 3, url: "https://example.com/", title: "Bank statement" },
  ]);
  const hello = e.posted.find((m) => m.type === "hello");
  assert.equal(hello.browser, "chrome");
  assert.ok(hello.commands.includes("media.next") && hello.commands.includes("chat.send_to"));
  const state = e.posted.filter((m) => m.type === "state").pop();
  assert.deepEqual(JSON.parse(JSON.stringify(state)), { type: "state", audible: 1, mediaTabs: 1, chats: [{ site: "whatsapp", unread: 3 }] });
  assert.ok(!JSON.stringify(e.posted).includes("Secret video"), "titles never go to the app on their own");
  assert.ok(!JSON.stringify(e.posted).includes("Bank statement"));
});

test("media: the tab that makes sound is the target, even in the background", async () => {
  const playing = yt();
  video(playing, { playing: true });
  const other = page("https://open.spotify.com/", "<button data-testid='control-button-playpause' aria-label='Play'></button>");
  const e = await extension([
    { id: 1, url: "https://open.spotify.com/", active: true, dom: other },
    { id: 2, url: "https://www.youtube.com/watch?v=1", audible: true, dom: playing },
  ]);
  const r = await e.run("media.pause");
  assert.equal(r.ok, true);
  assert.equal(playing.window.document.querySelector("video").paused, true);
  assert.equal(e.CP.mem.lastMedia, 2);
});

test("media: with nothing playing, 'toggle' resumes the tab that played last", async () => {
  const a = yt();
  const b = page("https://soundcloud.com/x", "<video></video>");
  const vb = video(b, { playing: false });
  video(a, { playing: false });
  const e = await extension([
    { id: 1, url: "https://www.youtube.com/watch?v=1", active: true, dom: a },
    { id: 2, url: "https://soundcloud.com/x", dom: b },
  ]);
  e.CP.mem.lastMedia = 2;
  const r = await e.run("media.toggle");
  assert.equal(r.ok, true);
  assert.equal(vb.paused, false, "the tab that played last, not the one in front");
});

test("media: no player at all is an honest failure, not a guess", async () => {
  const e = await extension([{ id: 1, url: "https://example.com/", active: true }]);
  const r = await e.run("media.next");
  assert.equal(r.ok, false);
  assert.match(r.message, /No player found/);
});

test("media: a page the extension may not touch gives a message the person can act on", async () => {
  const e = await extension([{ id: 1, url: "https://www.youtube.com/watch?v=1", audible: true }]); // no dom: scripting refuses
  const r = await e.run("media.pause");
  assert.equal(r.ok, false);
  assert.match(r.message, /options/);
});

test("media: pause_all stops every tab that plays and remembers the last one", async () => {
  const a = yt(), b = yt();
  const va = video(a, { playing: true }), vb = video(b, { playing: true });
  const e = await extension([
    { id: 1, url: "https://www.youtube.com/watch?v=1", audible: true, dom: a },
    { id: 2, url: "https://www.youtube.com/watch?v=2", audible: true, dom: b },
  ]);
  e.CP.mem.audibleAt = { 1: 10, 2: 20 };
  const r = await e.run("media.pause_all");
  assert.equal(r.ok, true);
  assert.equal(va.paused && vb.paused, true);
  assert.equal(e.CP.mem.lastMedia, 2);
});

test("media: seek and volume use the step of the options unless the slice says a number", async () => {
  const d = yt();
  const v = video(d, { playing: true, duration: 500, time: 100 });
  const e = await extension([{ id: 1, url: "https://www.youtube.com/watch?v=1", audible: true, dom: d }], { seekSeconds: 30 });
  await e.run("media.seek_fwd");
  assert.equal(v.currentTime, 130);
  await e.run("media.seek_back", "5");
  assert.equal(v.currentTime, 125);
});

test("background: a command's result says whether to show it (errors by default, questions always)", async () => {
  const d = yt();
  video(d, { playing: true });
  d.window.navigator.mediaSession = { metadata: { title: "Song", artist: "Band" } };
  const e = await extension([{ id: 1, url: "https://www.youtube.com/watch?v=1", audible: true, active: true, dom: d }]);
  const fine = await e.ask("media.next");
  assert.equal(fine.ok, true);
  assert.equal(fine.notify, false, "a success is silent");
  const q = await e.ask("media.now_playing");
  assert.equal(q.notify, true);
  assert.match(q.message, /Song.*Band/);
  const bad = await e.ask("media.like");
  assert.equal(bad.ok, false);
  assert.equal(bad.notify, true, "an error is shown");
  const unknown = await e.ask("media.explode");
  assert.equal(unknown.ok, false);
});

test("background: the notification option 'none' silences errors but not a question", async () => {
  const d = yt();
  video(d, { playing: true });
  d.window.navigator.mediaSession = { metadata: { title: "Song", artist: "" } };
  const e = await extension([{ id: 1, url: "https://www.youtube.com/watch?v=1", audible: true, dom: d }], { notify: "none" });
  assert.equal((await e.ask("media.like")).notify, false);
  assert.equal((await e.ask("media.now_playing")).notify, true);
});

test("background: switching a group off in the options refuses its commands", async () => {
  const e = await extension([{ id: 1, url: "https://example.com/", active: true }], { tabs: false });
  const r = await e.run("tab.close");
  assert.equal(r.ok, false);
  assert.match(r.message, /switched off/);
  assert.equal(e.calls.length, 0, "nothing was closed");
});

test("background: results and malformed messages from the app are handled", async () => {
  const e = await extension([{ id: 1, url: "https://example.com/", active: true }]);
  e.listeners.host(null);
  e.listeners.host({ type: "command" });
  e.listeners.host({ type: "command", id: "x", cmd: "tab.close" });
  e.listeners.host({ type: "command", id: 5, cmd: "tab.close", arg: 42 }); // a non-text argument is ignored
  await sleep(30);
  assert.equal(e.posted.filter((m) => m.type === "result").length, 1);
  assert.equal(e.state.tabs.length, 0, "tab.close ran once, for the one valid message");
});

// ---------------------------------------------------------------------------------- tabs

const three = () => [
  { id: 1, url: "https://a.example/", title: "A", active: false },
  { id: 2, url: "https://b.example/", title: "B inbox", active: true },
  { id: 3, url: "https://c.example/", title: "C", active: false },
];
const shown = (e) => e.calls.filter((c) => c[0] === "update" && c[2].active).map((c) => c[1]);

test("tabs: next and previous wrap round the window", async () => {
  const e = await extension(three());
  await e.run("tab.next");
  assert.deepEqual(shown(e), [3]);
  const e2 = await extension(three().map((t) => ({ ...t, active: t.id === 1 })));
  await e2.run("tab.prev");
  assert.deepEqual(shown(e2), [3]);
});

test("tabs: recent goes back to the tab used before, skipping tabs that were closed", async () => {
  const e = await extension(three());
  e.CP.mem.mru = { 1: [3, 1, 2] };
  await e.run("tab.recent");
  assert.deepEqual(shown(e), [1]);
  const e2 = await extension(three());
  e2.CP.mem.mru = { 1: [9, 8, 2] };
  assert.equal((await e2.run("tab.recent")).ok, false, "nothing earlier that still exists");
});

test("tabs: find looks in titles and addresses and never picks the tab you are on", async () => {
  const e = await extension(three());
  assert.equal((await e.run("tab.find", "inbox")).ok, false, "the only match is the current tab");
  assert.equal((await e.run("tab.find", "c.example")).ok, true);
  assert.deepEqual(shown(e), [3]);
  assert.equal((await e.run("tab.find", "")).ok, false);
});

test("tabs: close_dupes keeps the tab you are on and the pinned ones", async () => {
  const e = await extension([
    { id: 1, url: "https://a.example/x#top", active: false },
    { id: 2, url: "https://a.example/x", active: true },
    { id: 3, url: "https://a.example/x/", active: false },
    { id: 4, url: "https://b.example/", active: false },
    { id: 5, url: "https://a.example/x", active: false, pinned: true },
  ]);
  const r = await e.run("tab.close_dupes");
  assert.match(r.message, /Closed 2/);
  assert.deepEqual(e.calls.find((c) => c[0] === "remove")[1].sort(), [1, 3]);
});

test("tabs: close_others spares pinned tabs and the current one", async () => {
  const e = await extension([
    { id: 1, url: "https://a.example/", pinned: true },
    { id: 2, url: "https://b.example/", active: true },
    { id: 3, url: "https://c.example/" },
  ]);
  await e.run("tab.close_others");
  assert.deepEqual(e.calls.find((c) => c[0] === "remove")[1], [3]);
});

test("tabs: tab.new opens only web addresses", async () => {
  const e = await extension(three());
  assert.equal((await e.run("tab.new", "javascript:alert(1)")).ok, false);
  assert.equal((await e.run("tab.new", "file:///etc/passwd")).ok, false);
  assert.equal((await e.run("tab.new", "https://ok.example/")).ok, true);
  assert.deepEqual(JSON.parse(JSON.stringify(e.calls.filter((c) => c[0] === "create"))), [["create", { url: "https://ok.example/" }]]);
});

// ---------------------------------------------------------------------------------- chats

function chatTab(opts) {
  const w = whatsapp(opts && opts.selected);
  return w;
}

test("chat: next_unread brings the tab forward and opens the chat", async () => {
  const w = chatTab("c1");
  const e = await extension([
    { id: 1, url: "https://example.com/", active: true },
    { id: 2, url: "https://web.whatsapp.com/", title: "(2) WhatsApp", dom: w.dom },
  ]);
  const r = await e.run("chat.next_unread");
  assert.equal(r.ok, true);
  assert.deepEqual(shown(e), [2]);
  assert.deepEqual(w.log.clicked, ["c2"]);
});

test("chat: with no chat tab it fails, except focus which opens WhatsApp Web (or Telegram with @telegram)", async () => {
  const e = await extension([{ id: 1, url: "https://example.com/", active: true }]);
  assert.equal((await e.run("chat.next_unread")).ok, false);
  await e.run("chat.focus");
  await e.run("chat.focus", "@telegram");
  assert.deepEqual(
    e.calls.filter((c) => c[0] === "create").map((c) => c[1].url),
    ["https://web.whatsapp.com/", "https://web.telegram.org/k/"]
  );
});

test("chat: status reports counts without showing a tab or naming a person", async () => {
  const w = chatTab();
  const e = await extension([
    { id: 1, url: "https://example.com/", active: true },
    { id: 2, url: "https://web.whatsapp.com/", title: "(7) WhatsApp", dom: w.dom },
  ]);
  const r = await e.run("chat.status");
  assert.match(r.message, /WhatsApp: 7 unread/);
  assert.equal(r.notify, true);
  assert.deepEqual(shown(e), [], "it stayed where it was");
  assert.ok(!/Anna|Budi|Citra/.test(r.message));
});

test("chat: send_to types the message and does NOT send while sending is off", async () => {
  const w = chatTab();
  const e = await extension([{ id: 2, url: "https://web.whatsapp.com/", active: true, dom: w.dom }]);
  const r = await e.run("chat.send_to", "Budi :: on my way");
  assert.equal(r.ok, false);
  assert.match(r.message, /not sent/i);
  assert.equal(w.d.getElementById("box").textContent, "on my way", "the message waits in the box");
  assert.equal(w.log.sent, 0);
});

test("chat: send_to sends when it is allowed and the open chat is the right one", async () => {
  const w = chatTab();
  const e = await extension([{ id: 2, url: "https://web.whatsapp.com/", active: true, dom: w.dom }], { allowSend: true });
  const r = await e.run("chat.send_to", "Budi :: on my way");
  assert.equal(r.ok, true, r.message);
  assert.equal(w.log.sent, 1);
  assert.deepEqual(w.log.clicked, ["c2"]);
});

test("chat: send_to refuses to send when the chat that opened is not the one asked for", async () => {
  const w = chatTab();
  // the page opens "Budi" whatever is asked: its header says Budi
  const e = await extension([{ id: 2, url: "https://web.whatsapp.com/", active: true, dom: w.dom }], { allowSend: true });
  const r = await e.run("chat.send_to", "Anna :: hello");
  assert.equal(r.ok, false);
  assert.match(r.message, /not 'Anna'/);
  assert.equal(w.log.sent, 0);
  assert.equal(w.d.getElementById("box").textContent, "", "nothing was typed either");
});

test("chat: send_to refuses to send when the header cannot be read", async () => {
  const w = chatTab();
  w.d.getElementById("hd").remove();
  const e = await extension([{ id: 2, url: "https://web.whatsapp.com/", active: true, dom: w.dom }], { allowSend: true });
  const r = await e.run("chat.send_to", "Budi :: hi");
  assert.equal(r.ok, false);
  assert.match(r.message, /nothing was sent/);
  assert.equal(w.log.sent, 0);
});

test("chat: send_to needs both a name and a message", async () => {
  const w = chatTab();
  const e = await extension([{ id: 2, url: "https://web.whatsapp.com/", active: true, dom: w.dom }], { allowSend: true });
  assert.equal((await e.run("chat.send_to", "just text")).ok, false);
  assert.equal((await e.run("chat.send_to", "Budi ::")).ok, false);
  assert.equal(w.log.sent, 0);
});

test("chat: an unknown name is not guessed", async () => {
  const w = chatTab();
  const e = await extension([{ id: 2, url: "https://web.whatsapp.com/", active: true, dom: w.dom }], { allowSend: true });
  const r = await e.run("chat.send_to", "Zed :: hi");
  assert.equal(r.ok, false);
  assert.equal(w.log.sent, 0);
});

test("chat: chat.send types in the open chat and sends only when allowed", async () => {
  const off = chatTab();
  const e1 = await extension([{ id: 2, url: "https://web.whatsapp.com/", active: true, dom: off.dom }]);
  assert.equal((await e1.run("chat.send", "ok")).ok, false);
  assert.equal(off.log.sent, 0);
  const on = chatTab();
  const e2 = await extension([{ id: 2, url: "https://web.whatsapp.com/", active: true, dom: on.dom }], { allowSend: true });
  assert.equal((await e2.run("chat.send", "ok")).ok, true);
  assert.equal(on.log.sent, 1);
});

// ---------------------------------------------------------------------------------- the whole set

test("every registered command has a group the options can switch, and a name of the form group.verb", async () => {
  const e = await extension([]);
  const ids = Object.keys(e.CP.commands);
  assert.ok(ids.length >= 60, "expected a big set, got " + ids.length);
  for (const id of ids) {
    assert.match(id, /^(media|tab|window|page|chat)\.[a-z_]+$/);
    assert.equal(e.CP.commands[id].group, id.split(".")[0]);
  }
});

test("the Firefox manifest lists the same scripts the service worker loads, in the same order", () => {
  const ff = JSON.parse(read("manifest.firefox.json"));
  const loaded = [...read("background.js").matchAll(/importScripts\(([\s\S]*?)\)/g)][0][1].match(/"([^"]+)"/g).map((s) => s.slice(1, -1));
  assert.deepEqual(ff.background.scripts, loaded.concat("background.js"));
  const cr = JSON.parse(read("manifest.chromium.json"));
  assert.equal(cr.background.service_worker, "background.js");
  for (const f of loaded.concat("background.js", "options.html", "options.js")) assert.ok(fs.existsSync(path.join(DIR, f)), f);
});

test("both manifests allow the same sites, and the player list of the code is covered by them", () => {
  const cr = JSON.parse(read("manifest.chromium.json"));
  const ff = JSON.parse(read("manifest.firefox.json"));
  const ffHosts = ff.permissions.filter((p) => p.includes("://"));
  assert.deepEqual(cr.host_permissions, ffHosts);
  const ctx = vm.createContext({ chrome: browser([]).chrome, console, setTimeout, URL });
  ctx.self = ctx;
  for (const f of SCRIPTS) vm.runInContext(read(f), ctx, { filename: f });
  for (const site of ctx.CP.PLAYER_SITES) {
    assert.ok(
      cr.host_permissions.some((p) => p.endsWith("." + site + "/*") || p.endsWith("//" + site + "/*") || p.includes(site)),
      site + " is a player site of the code but the manifest does not allow it"
    );
  }
});

test("Firefox path: without the scripting API the page function is sent as text to tabs.executeScript", async () => {
  const dom = yt();
  const v = video(dom, { playing: true });
  const b = browser([{ id: 1, url: "https://www.youtube.com/watch?v=1", audible: true, dom }]);
  delete b.chrome.scripting; // a Firefox (Manifest V2) browser has no `scripting` here
  const sent = [];
  b.chrome.tabs.executeScript = async (id, { code }) => {
    sent.push(code);
    return [dom.window.eval(code)];
  };
  const ctx = vm.createContext({ chrome: b.chrome, console, setTimeout, clearTimeout, URL, navigator: { userAgent: "Firefox/130" } });
  ctx.self = ctx;
  for (const f of SCRIPTS) vm.runInContext(read(f), ctx, { filename: f });
  const r = await ctx.CP.dispatch("media.pause", "");
  assert.equal(r.ok, true);
  assert.equal(v.paused, true);
  assert.match(sent[0], /^\(function pageMedia/);
  assert.match(sent[0], /\.apply\(null, \["pause",\{\}\]\)$/);
});
