// Tests of the commands a flow can call (`tab.url`, `tab.count`, `media.state`) and of the
// auto-pause fix. They do not need jsdom: the extension runs in a vm against a small fake of the
// `chrome` API, so `node --test` can run this file on its own.
"use strict";

const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const DIR = path.join(__dirname, "..", "..", "assets", "browser-extension");

// tabs: [{ id, windowId, index, active, audible, url }]; pageAnswers: what pageMedia says by action
function load({ tabs, answers = {} }) {
  const ran = [];
  const chrome = {
    storage: { local: { get: async () => ({}) }, session: undefined, onChanged: { addListener() {} } },
    tabs: {
      query: async (q) =>
        tabs.filter((t) => (q.audible === undefined || t.audible === q.audible) && (q.windowId === undefined || t.windowId === q.windowId) && (!q.active || t.active)),
      get: async (id) => {
        const t = tabs.find((x) => x.id === id);
        if (!t) throw new Error("No tab with id " + id);
        return t;
      },
    },
    windows: {},
    runtime: {},
  };
  const ctx = { chrome, console, setTimeout, clearTimeout, Promise, self: null };
  ctx.self = ctx;
  vm.createContext(ctx);
  for (const f of ["util.js", "cmd-tab.js", "cmd-media.js"]) {
    vm.runInContext(fs.readFileSync(path.join(DIR, f), "utf8"), ctx, { filename: f });
  }
  const CP = ctx.CP;
  CP.currentTab = async () => tabs.find((t) => t.active) || null;
  CP.saveMem = () => {};
  // pageMedia runs in a page; here it is replaced by the answers of the test
  CP.runInTab = async (id, fn, args) => {
    ran.push([id, args[0]]);
    const a = answers[args[0]];
    return typeof a === "function" ? a(id) : a;
  };
  return { CP, ran };
}

const ok = (extra) => ({ ok: true, msg: "", ...extra });

test("tab.url answers with the address of the current tab", async () => {
  const { CP } = load({ tabs: [{ id: 1, windowId: 1, index: 0, active: true, url: "https://example.org/a?b=1" }] });
  const r = await CP.commands["tab.url"].run({ arg: "", settings: CP.settings() });
  // objects made inside the vm have another Object prototype: compare them as text
  assert.strictEqual(JSON.stringify(r), JSON.stringify({ ok: true, message: "https://example.org/a?b=1", notify: true }));
});

test("tab.url fails on a page that has no web address", async () => {
  const { CP } = load({ tabs: [{ id: 1, windowId: 1, index: 0, active: true, url: "about:blank" }] });
  const r = await CP.commands["tab.url"].run({ arg: "", settings: CP.settings() });
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /no web address/);
});

test("tab.count counts the tabs of the current window only", async () => {
  const { CP } = load({
    tabs: [
      { id: 1, windowId: 1, index: 0, active: true, url: "https://a.org/" },
      { id: 2, windowId: 1, index: 1, url: "https://b.org/" },
      { id: 3, windowId: 2, index: 0, url: "https://c.org/" },
    ],
  });
  const r = await CP.commands["tab.count"].run({ arg: "", settings: CP.settings() });
  assert.strictEqual(r.message, "2");
  assert.strictEqual(r.ok, true);
});

test("media.state says none when there is no player, without failing", async () => {
  const { CP } = load({ tabs: [{ id: 1, windowId: 1, index: 0, active: true, url: "https://example.org/" }] });
  const r = await CP.commands["media.state"].run({ arg: "", settings: CP.settings() });
  assert.strictEqual(r.ok, true);
  assert.strictEqual(r.message, "none");
});

test("media.state says playing or paused and never names the track", async () => {
  const tabs = [{ id: 7, windowId: 1, index: 0, active: true, audible: true, url: "https://www.youtube.com/watch?v=x" }];
  const answers = { info: ok({ playing: true, title: "Secret song", artist: "Someone", hasMedia: true }) };
  const { CP } = load({ tabs, answers });
  let r = await CP.commands["media.state"].run({ arg: "", settings: CP.settings() });
  assert.strictEqual(r.message, "playing");
  assert.ok(!JSON.stringify(r).includes("Secret"));
  answers.info = ok({ playing: false, title: "Secret song", hasMedia: true });
  tabs[0].audible = false;
  r = await CP.commands["media.state"].run({ arg: "", settings: CP.settings() });
  assert.strictEqual(r.message, "paused");
});

test("a page that cannot be reached makes media.state fail with a reason", async () => {
  const tabs = [{ id: 7, windowId: 1, index: 0, active: true, audible: true, url: "https://www.youtube.com/" }];
  const { CP } = load({ tabs, answers: { info: () => { throw new Error("Cannot access contents of the page"); } } });
  const r = await CP.commands["media.state"].run({ arg: "", settings: CP.settings() });
  assert.strictEqual(r.ok, false);
  assert.ok(r.message.length > 0);
});

test("auto-pause leaves the player that started as the one that played last", async () => {
  const tabs = [
    { id: 1, windowId: 1, index: 0, audible: true, url: "https://www.youtube.com/watch?v=a" },
    { id: 2, windowId: 1, index: 1, audible: true, url: "https://open.spotify.com/" },
  ];
  const { CP, ran } = load({ tabs, answers: { pause: ok() } });
  await CP.pauseOthers(2);
  assert.strictEqual(JSON.stringify(ran), JSON.stringify([[1, "pause"]]));
  assert.strictEqual(CP.mem.lastMedia, 2, "play after a pause must wake the player that started");
});
