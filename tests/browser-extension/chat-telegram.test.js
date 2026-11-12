// Tests of the Telegram commands: chat.folder, chat.folders, chat.next_mention, the muted-chat
// option, saved replies (chat.template, chat.templates) and opening a chat by @username.
// Like chat-flow.test.js they do not need jsdom. Two parts:
//   * the page code (page-chat.js) against a very small fake of the page, where each selector of the
//     table finds what the test says it finds. This checks the logic (which row is picked, what is
//     skipped), NOT that the real selectors of Telegram Web match: that needs a browser and a login
//     (see "What is tested, and what is not" in docs/BROWSER_EXTENSION.md);
//   * the commands (cmd-chat.js) against a scripted page.
// Run on its own with `node --test chat-telegram.test.js`.
"use strict";

const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const DIR = path.join(__dirname, "..", "..", "assets", "browser-extension");
const read = (f) => fs.readFileSync(path.join(DIR, f), "utf8");

// ------------------------------------------------------------------ the page code

// An element that answers only what page-chat.js asks of it. `sels`: the selectors it matches;
// `kids`: selector -> the elements that selector finds inside it.
function el({ sels = [], kids = {}, text = "", label = null, attrs = {} } = {}) {
  const e = {
    clicks: 0,
    text,
    getClientRects: () => [1],
    offsetWidth: 1,
    matches: (s) => s.split(",").some((one) => sels.includes(one.trim())),
    querySelector: (s) => (kids[s] ? [].concat(kids[s])[0] : null),
    querySelectorAll: (s) => (kids[s] ? [].concat(kids[s]) : []),
    getAttribute: (n) => (n in attrs ? attrs[n] : null),
    hasAttribute: (n) => n in attrs,
    dispatchEvent: (ev) => {
      if (ev.type === "click") e.clicks++;
      return true;
    },
    cloneNode: () => {
      // a folder tab: the badge inside it is taken out when the code removes it
      const c = { textContent: text, querySelectorAll: () => (label === null ? [] : [{ remove: () => (c.textContent = label) }]) };
      return c;
    },
  };
  Object.defineProperty(e, "textContent", { get: () => e.text });
  return e;
}

// a chat row: `n` unread, `muted`, `mention`
function row(name, { n = 0, muted = false, mention = false, selected = false } = {}) {
  const kids = {};
  if (n) kids[".dialog-subtitle-badge.is-visible"] = el({ text: String(n) });
  if (mention) kids[".dialog-subtitle-badge.mention"] = el();
  const sels = [];
  if (muted) sels.push(".is-muted");
  if (selected) sels.push(".active");
  const r = el({ sels, kids, text: name });
  r.name = name;
  return r;
}

function folder(name, { badge = "", active = false } = {}) {
  return el({ sels: active ? [".active"] : [], text: name + badge, label: name });
}

function loadPage(map, { host = "web.telegram.org" } = {}) {
  const document = {
    querySelector: (s) => (map[s] && map[s][0]) || null,
    querySelectorAll: (s) => map[s] || [],
    activeElement: null,
    body: el(),
  };
  const location = { hostname: host, hash: "" };
  class Ev {
    constructor(type) {
      this.type = type;
    }
  }
  const ctx = { document, location, window: {}, getComputedStyle: () => ({}), KeyboardEvent: Ev, MouseEvent: Ev, PointerEvent: Ev };
  ctx.self = ctx;
  vm.createContext(ctx);
  vm.runInContext(read("page-chat.js"), ctx, { filename: "page-chat.js" });
  const CP = ctx.CP;
  return { run: (action, opts) => JSON.parse(JSON.stringify(CP.pageChat(action, opts || {}, CP.CHAT_SELECTORS))), location };
}

const ROWS = "a.chatlist-chat";
const FOLDERS = ".folders-tabs .menu-horizontal-div-item";

test("a muted chat is skipped by next_unread unless told otherwise", () => {
  const a = row("Grup bisu", { n: 5, muted: true });
  const b = row("Anna", { n: 1 });
  const p = loadPage({ [ROWS]: [a, b] });
  const r = p.run("open_unread", { dir: 1, skipMuted: true });
  assert.strictEqual(r.ok, true);
  assert.deepStrictEqual([a.clicks, b.clicks], [0, 1], "Anna, not the muted group");
  const q = loadPage({ [ROWS]: [a, b] });
  q.run("open_unread", { dir: 1, skipMuted: false });
  assert.strictEqual(a.clicks, 1, "with the option off the first unread chat is opened");
});

test("only muted chats wait: there is nothing to open, and the number is 0", () => {
  const a = row("Grup bisu", { n: 5, muted: true });
  const p = loadPage({ [ROWS]: [a] });
  const r = p.run("open_unread", { dir: 1, skipMuted: true });
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.found, false);
  assert.strictEqual(p.run("info", { skipMuted: true }).unreadChats, 0);
  assert.strictEqual(p.run("info", { skipMuted: false }).unreadChats, 1);
});

test("unread_names leaves out muted chats only when asked to", () => {
  const p = loadPage({ [ROWS]: [row("Anna", { n: 2 }), row("Grup bisu", { n: 9, muted: true })] });
  assert.deepStrictEqual(p.run("unread_names", { skipMuted: true }).names.map((c) => c.name), ["Anna"]);
  assert.deepStrictEqual(p.run("unread_names", {}).names.map((c) => c.name), ["Anna", "Grup bisu"]);
});

test("next_unread still goes round the list after the open chat", () => {
  const rows = [row("A", { n: 1 }), row("B", { n: 1, selected: true }), row("C", { n: 1 })];
  loadPage({ [ROWS]: rows, ".active": [] }).run("open_unread", { dir: 1, skipMuted: true });
  assert.deepStrictEqual(rows.map((r) => r.clicks), [0, 0, 1], "after B comes C");
  const again = [row("A", { n: 1 }), row("B", { n: 1 }), row("C", { n: 1, selected: true })];
  loadPage({ [ROWS]: again }).run("open_unread", { dir: 1, skipMuted: true });
  assert.deepStrictEqual(again.map((r) => r.clicks), [1, 0, 0], "after the last it goes back to the first");
  const back = [row("A", { n: 1 }), row("B", { n: 1, selected: true }), row("C", { n: 1 })];
  loadPage({ [ROWS]: back }).run("open_unread", { dir: -1, skipMuted: true });
  assert.deepStrictEqual(back.map((r) => r.clicks), [1, 0, 0]);
});

test("next_mention opens a chat with a mention, even a muted one, and says so when there is none", () => {
  const a = row("Anna", { n: 3 });
  const b = row("Grup kerja", { n: 8, muted: true, mention: true });
  const p = loadPage({ [ROWS]: [a, b] });
  const r = p.run("open_mention", { dir: 1 });
  assert.strictEqual(r.ok, true);
  assert.deepStrictEqual([a.clicks, b.clicks], [0, 1]);
  const none = loadPage({ [ROWS]: [row("Anna", { n: 3 })] }).run("open_mention", { dir: 1 });
  assert.strictEqual(none.ok, false);
  assert.strictEqual(none.found, false);
});

test("folders: the names without the badge, and which one is open", () => {
  const tabs = [folder("All", { badge: "12", active: true }), folder("Work", { badge: "3" }), folder("Family")];
  const r = loadPage({ [FOLDERS]: tabs }).run("folders");
  assert.deepStrictEqual(r.folders, [
    { name: "All", active: true },
    { name: "Work", active: false },
    { name: "Family", active: false },
  ]);
  const none = loadPage({}).run("folders");
  assert.strictEqual(none.ok, false);
  assert.match(none.msg, /No folders/);
});

test("folder: the exact name first, then part of a name, then the number", () => {
  const mk = () => [folder("Work"), folder("Work friends"), folder("2024"), folder("Family")];
  let tabs = mk();
  loadPage({ [FOLDERS]: tabs }).run("folder", { name: "work" });
  assert.deepStrictEqual(tabs.map((t) => t.clicks), [1, 0, 0, 0], "the exact one, not the first that contains it");
  tabs = mk();
  loadPage({ [FOLDERS]: tabs }).run("folder", { name: "fam" });
  assert.strictEqual(tabs[3].clicks, 1);
  tabs = mk();
  loadPage({ [FOLDERS]: tabs }).run("folder", { name: "4" });
  assert.strictEqual(tabs[3].clicks, 1, "a number is the 4th folder");
  tabs = mk();
  loadPage({ [FOLDERS]: tabs }).run("folder", { name: "2024" });
  assert.strictEqual(tabs[2].clicks, 1, "a folder named 2024 is not the 2nd");
  tabs = mk();
  loadPage({ [FOLDERS]: tabs }).run("folder", { name: "2" });
  assert.strictEqual(tabs[1].clicks, 1, "a short number is a place, not a part of 2024");
  tabs = mk();
  const bad = loadPage({ [FOLDERS]: tabs }).run("folder", { name: "nothing" });
  assert.strictEqual(bad.ok, false);
  assert.strictEqual(bad.found, false);
  assert.ok(tabs.every((t) => t.clicks === 0));
  assert.strictEqual(loadPage({ [FOLDERS]: tabs }).run("folder", { name: "  " }).ok, false, "an empty name matches nothing");
  assert.strictEqual(loadPage({ [FOLDERS]: tabs }).run("folder", { name: "9" }).ok, false, "no 9th folder");
});

test("goto_user sets the address of a Telegram @username and nothing else", () => {
  const p = loadPage({});
  assert.strictEqual(p.run("goto_user", { user: "@durov" }).ok, true);
  assert.strictEqual(p.location.hash, "#@durov");
  for (const bad of ["durov", "@du", "@a b", "@1abcde", "@x/../y", "#@durov", "@" + "a".repeat(40)]) {
    const q = loadPage({});
    assert.strictEqual(q.run("goto_user", { user: bad }).ok, false, bad);
    assert.strictEqual(q.location.hash, "", bad);
  }
  const wa = loadPage({}, { host: "web.whatsapp.com" });
  assert.strictEqual(wa.run("goto_user", { user: "@durov" }).ok, false, "not on WhatsApp");
  assert.strictEqual(wa.location.hash, "");
});

test("every selector table has the new keys, so an override for them is used", () => {
  const { CP } = loadPage({}) && { CP: null };
  void CP;
  const ctx = { self: null };
  ctx.self = ctx;
  vm.createContext(ctx);
  vm.runInContext(read("page-chat.js"), ctx);
  for (const site of ["whatsapp", "telegram"]) {
    for (const key of ["muted", "mention", "folders"]) {
      assert.ok(Array.isArray(ctx.CP.CHAT_SELECTORS[site][key]), site + "." + key);
    }
  }
});

// ------------------------------------------------------------------ the commands

// Time is fake: CP.sleep moves the clock. `page(action, opts, tabId)` is what the chat page answers.
function load({ tabs, page, settings = {} }) {
  const clock = { now: new Date(2026, 10, 12, 9, 5, 0).getTime() };
  const calls = [];
  const chrome = {
    storage: { local: { get: async () => ({}) }, session: undefined, onChanged: { addListener() {} } },
    tabs: { query: async (q) => tabs.filter((t) => !q.active || t.active), update: async () => {}, get: async (id) => tabs.find((t) => t.id === id) },
    windows: { update: async () => {} },
    runtime: {},
  };
  class FakeDate extends Date {
    static now() {
      return clock.now;
    }
  }
  const ctx = { chrome, console, setTimeout, clearTimeout, Promise, URL, Date: FakeDate, self: null };
  ctx.self = ctx;
  vm.createContext(ctx);
  for (const f of ["util.js", "page-chat.js", "cmd-chat.js"]) vm.runInContext(read(f), ctx, { filename: f });
  const CP = ctx.CP;
  CP.saveMem = () => {};
  CP.sleep = async (ms) => {
    clock.now += ms;
  };
  CP.show = async () => {};
  CP.runInTab = async (tabId, fn, args) => {
    calls.push({ action: args[0], opts: args[1], tabId });
    return page(args[0], args[1], tabId);
  };
  const all = Object.assign({}, CP.DEFAULTS, settings);
  const run = async (cmd, arg = "") => JSON.parse(JSON.stringify(await CP.commands[cmd].run({ arg, settings: all })));
  const actions = () => calls.map((c) => c.action);
  return { CP, run, calls, actions, clock };
}

const wa = (extra) => Object.assign({ id: 1, windowId: 1, active: true, title: "WhatsApp", url: "https://web.whatsapp.com/" }, extra);
const tg = (extra) => Object.assign({ id: 2, windowId: 1, active: false, title: "Telegram", url: "https://web.telegram.org/k/" }, extra);
const ok = (extra) => Object.assign({ ok: true, msg: "", site: "telegram" }, extra);

test("the folder commands act on Telegram even when WhatsApp is the tab in front, and refuse @whatsapp", async () => {
  const { run, calls } = load({ tabs: [wa(), tg()], page: () => ok({ found: true, name: "Work", folders: [{ name: "All" }, { name: "Work" }] }), settings: { allowRead: true } });
  const r = await run("chat.folder", "work");
  assert.strictEqual(r.ok, true);
  assert.strictEqual(r.message, "Folder: Work");
  assert.deepStrictEqual(calls.map((c) => [c.action, c.tabId]), [["folder", 2]]);
  assert.strictEqual((await run("chat.folder", "@whatsapp work")).ok, false);
  assert.strictEqual((await run("chat.folder", "@telegram work")).ok, true);
  assert.strictEqual((await run("chat.folders")).message, "All\nWork");
  assert.strictEqual((await run("chat.next_mention", "@whatsapp")).ok, false);
});

test("the folder commands say Telegram is not open, and chat.folder needs a name", async () => {
  const { run, calls } = load({ tabs: [wa()], page: () => ok() });
  assert.match((await run("chat.folder", "work")).message, /Telegram is not open/);
  assert.match((await run("chat.next_mention")).message, /Telegram is not open/);
  assert.strictEqual((await run("chat.folder", "")).ok, false);
  assert.deepStrictEqual(calls, []);
});

test("chat.folders reads names, so it needs 'Allow reading messages'", async () => {
  const { run, calls } = load({ tabs: [tg()], page: () => ok({ folders: [{ name: "Work" }] }) });
  const r = await run("chat.folders");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /Allow reading messages/);
  assert.deepStrictEqual(calls, []);
});

test("chat.next_mention says so when no chat has one, as a result and not a failure", async () => {
  const { run } = load({ tabs: [tg()], page: () => ok({ found: false, msg: "No chat with a mention" }) });
  const r = await run("chat.next_mention");
  assert.strictEqual(r.ok, true);
  assert.strictEqual(r.message, "No chat with a mention");
});

test("muted chats are skipped by default and the option turns it off", async () => {
  let seen;
  const page = (a, o) => {
    seen = o;
    return ok({ found: true, ready: true, unreadChats: 1, names: [] });
  };
  await load({ tabs: [tg()], page }).run("chat.next_unread");
  assert.strictEqual(seen.skipMuted, true);
  await load({ tabs: [tg()], page, settings: { skipMuted: false } }).run("chat.prev_unread");
  assert.strictEqual(seen.skipMuted, false);
  await load({ tabs: [tg()], page, settings: { allowRead: true } }).run("chat.unread_names");
  assert.strictEqual(seen.skipMuted, true);
});

// saved replies

const TEMPLATES = { omw: "On my way, back at {time}", hi: "Hi {first}!", Bad_Key: "x", "bad key": "y", empty: "  ", long: "z".repeat(3000) };

function chatPage({ header = ["Anna Putri"], onDraft } = {}) {
  let h = 0;
  return (action, opts) => {
    if (action === "messages") return ok({ messages: [], name: header[Math.min(h, header.length - 1)] });
    if (action === "header") return ok({ name: header[Math.min(h++, header.length - 1)] });
    if (action === "search" || action === "pick") return ok({ found: true });
    if (action === "draft") {
      if (onDraft) onDraft(opts.text);
      return ok();
    }
    if (action === "send") return ok();
    return ok();
  };
}

test("saved replies: only well-formed names are kept, and the list shows names, not texts", async () => {
  const { run, CP } = load({ tabs: [tg()], page: chatPage(), settings: { templates: TEMPLATES } });
  assert.deepStrictEqual(Object.keys(CP.chatTemplates({ templates: TEMPLATES })).sort(), ["bad_key", "hi", "long", "omw"]);
  assert.strictEqual(CP.chatTemplates({ templates: TEMPLATES }).long.length, 2000);
  const r = await run("chat.templates");
  assert.strictEqual(r.ok, true);
  assert.ok(!r.message.includes("On my way"), "the text of a reply is not given");
  assert.strictEqual((await load({ tabs: [], page: () => ok() }).run("chat.templates")).message, "No saved replies yet");
  assert.deepStrictEqual(Object.keys(CP.chatTemplates({ templates: [1, 2] })), []);
  assert.deepStrictEqual(Object.keys(CP.chatTemplates({})), []);
});

test("chat.template types the reply with the time filled in and sends only when allowed", async () => {
  const typed = [];
  const t = load({ tabs: [tg()], page: chatPage({ onDraft: (x) => typed.push(x) }), settings: { templates: TEMPLATES } });
  const r = await t.run("chat.template", "omw");
  assert.deepStrictEqual(typed, ["On my way, back at 09:05"]);
  assert.strictEqual(r.ok, false, "sending is off: it is typed, not sent");
  assert.match(r.message, /not sent/);
  assert.ok(!t.actions().includes("send"));

  const typed2 = [];
  const s = load({ tabs: [tg()], page: chatPage({ onDraft: (x) => typed2.push(x) }), settings: { templates: TEMPLATES, allowSend: true } });
  const r2 = await s.run("chat.template", "OMW");
  assert.strictEqual(r2.ok, true, "the name is not case sensitive");
  assert.ok(s.actions().includes("send"));
  assert.deepStrictEqual(typed2, ["On my way, back at 09:05"]);
});

test("an unknown saved reply types nothing and names the ones there are", async () => {
  const t = load({ tabs: [tg()], page: chatPage(), settings: { templates: TEMPLATES, allowSend: true } });
  const r = await t.run("chat.template", "nope");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /omw/);
  assert.ok(!t.actions().includes("draft") && !t.actions().includes("send"));
  const none = load({ tabs: [tg()], page: chatPage() });
  assert.match((await none.run("chat.template", "omw")).message, /No saved replies yet/);
  assert.strictEqual((await t.run("chat.template", "")).ok, false);
  assert.strictEqual((await t.run("chat.template", "omw ::")).ok, false);
});

test("{name} and {first} need a name: the chat written after :: or, with reading allowed, the open chat", async () => {
  // reading is off and no chat is named: nothing is typed
  const a = load({ tabs: [tg()], page: chatPage(), settings: { templates: TEMPLATES, allowSend: true } });
  const r = await a.run("chat.template", "hi");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /Allow reading messages/);
  assert.ok(!a.actions().includes("draft"));
  // reading is on: the name in the header of the open chat
  const typed = [];
  const b = load({ tabs: [tg()], page: chatPage({ header: ["Anna Putri"], onDraft: (x) => typed.push(x) }), settings: { templates: TEMPLATES, allowRead: true, allowSend: true } });
  assert.strictEqual((await b.run("chat.template", "hi")).ok, true);
  assert.deepStrictEqual(typed, ["Hi Anna!"]);
  // the name written by the person works without reading
  const typed2 = [];
  const c = load({ tabs: [tg()], page: chatPage({ header: ["Anna Putri"], onDraft: (x) => typed2.push(x) }), settings: { templates: TEMPLATES, allowSend: true } });
  assert.strictEqual((await c.run("chat.template", "hi :: Anna")).ok, true);
  assert.deepStrictEqual(typed2, ["Hi Anna!"]);
});

test("a saved reply sent to a chat is checked like chat.send_to: a wrong chat gets nothing", async () => {
  const t = load({ tabs: [tg()], page: chatPage({ header: ["Budi"] }), settings: { templates: TEMPLATES, allowSend: true } });
  const r = await t.run("chat.template", "omw :: Anna");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /nothing was sent/);
  assert.ok(!t.actions().includes("draft") && !t.actions().includes("send"));
  const good = load({ tabs: [tg()], page: chatPage({ header: ["Anna Putri"] }), settings: { templates: TEMPLATES, allowSend: true } });
  assert.strictEqual((await good.run("chat.template", "omw :: Anna")).ok, true);
  assert.ok(good.actions().includes("send"));
});

// @username

test("chat.open @username goes by address, on Telegram only", async () => {
  const t = load({ tabs: [tg()], page: chatPage({ header: ["Old chat", "Pavel Durov"] }) });
  const r = await t.run("chat.open", "@durov");
  assert.strictEqual(r.ok, true);
  assert.ok(t.actions().includes("goto_user"));
  assert.ok(!t.actions().includes("search"), "the search box is not used");
  // WhatsApp has no usernames: it is searched like any name
  const w = load({ tabs: [wa()], page: chatPage({ header: ["@durov"] }) });
  await w.run("chat.open", "@durov");
  assert.ok(!w.actions().includes("goto_user"));
  assert.ok(w.actions().includes("search"));
});

test("a chat opened by @username is typed in but never sent", async () => {
  const typed = [];
  const t = load({ tabs: [tg()], page: chatPage({ header: ["Old chat", "Pavel Durov"], onDraft: (x) => typed.push(x) }), settings: { allowSend: true } });
  const r = await t.run("chat.send_to", "@durov :: hello");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /cannot be checked/);
  assert.deepStrictEqual(typed, ["hello"], "typed, so the person can press Send");
  assert.ok(!t.actions().includes("send"));
  const typed2 = [];
  const d = load({ tabs: [tg()], page: chatPage({ header: ["Old chat", "Pavel Durov"], onDraft: (x) => typed2.push(x) }) });
  assert.strictEqual((await d.run("chat.draft_to", "@durov :: hello")).ok, true);
  assert.deepStrictEqual(typed2, ["hello"]);
  const tpl = [];
  const e = load({ tabs: [tg()], page: chatPage({ header: ["Old chat", "Pavel Durov"], onDraft: (x) => tpl.push(x) }), settings: { allowSend: true, templates: { omw: "hi" } } });
  const r3 = await e.run("chat.template", "omw :: @durov");
  assert.strictEqual(r3.ok, false);
  assert.deepStrictEqual(tpl, ["hi"]);
  assert.ok(!e.actions().includes("send"));
});

test("when the chat that is open does not change, nothing is typed in it", async () => {
  const typed = [];
  const t = load({ tabs: [tg()], page: chatPage({ header: ["Same chat"], onDraft: (x) => typed.push(x) }), settings: { allowSend: true } });
  for (const cmd of ["chat.send_to", "chat.draft_to"]) {
    const r = await t.run(cmd, "@durov :: hello");
    assert.strictEqual(r.ok, false, cmd);
    assert.match(r.message, /nothing was typed/, cmd);
  }
  assert.deepStrictEqual(typed, []);
  assert.ok(!t.actions().includes("send"));
});

test("a name that only looks like a username is searched, not opened by address", async () => {
  for (const name of ["Anna", "@du", "@ana lisa", "anna@home"]) {
    const t = load({ tabs: [tg()], page: chatPage({ header: [name] }) });
    await t.run("chat.open", name);
    assert.ok(!t.actions().includes("goto_user"), name);
  }
});
