// Tests of the panels of the extension (ui.js) and the ui.* commands (cmd-ui.js).
// They need no browser and no jsdom: the browser API is a small fake.
//
//   cd tests/browser-extension && node --test ui.test.js
"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const DIR = path.join(__dirname, "..", "..", "assets", "browser-extension");
const read = (f) => fs.readFileSync(path.join(DIR, f), "utf8");

// A fake browser with the tabs given, and an "app" that records what the extension sent it.
function world(tabs) {
  const sent = [];
  const closed = [];
  const shown = [];
  const chrome = {
    storage: {
      local: { get: async () => ({}) },
      session: { get: async () => ({}), set: async () => {} },
      onChanged: { addListener() {} },
    },
    tabs: {
      query: async (q) => {
        let r = tabs.slice();
        if (q && q.active) r = r.filter((t) => t.active);
        if (q && q.windowId !== undefined) r = r.filter((t) => t.windowId === q.windowId);
        return r;
      },
      update: async (id) => shown.push(id),
      remove: async (ids) => closed.push(...[].concat(ids)),
    },
    windows: { update: async () => {} },
  };
  // timers that do not keep the test process alive (a panel waits minutes for an answer)
  const timer = (f, ms) => {
    const t = setTimeout(f, ms);
    if (t && t.unref) t.unref();
    return t;
  };
  const ctx = vm.createContext({ chrome, console, setTimeout: timer, clearTimeout, URL });
  ctx.self = ctx;
  for (const f of ["util.js", "ui.js", "cmd-tab.js", "cmd-ui.js"]) vm.runInContext(read(f), ctx, { filename: f });
  const CP = ctx.CP;
  CP.ui.transport = (m) => {
    sent.push(m);
    return true;
  };
  // the app answers the last panel with `result`
  const answer = (result, req) => {
    const last = sent.filter((m) => m.type === "panel").pop();
    CP.ui.handle({ type: "panel_result", req: req === undefined ? last.req : req, result });
  };
  return { CP, sent, closed, shown, answer };
}

const TABS = [
  { id: 1, windowId: 1, index: 0, active: true, url: "https://a.example/x", title: "Alpha" },
  { id: 2, windowId: 1, index: 1, active: false, url: "https://b.example/y", title: "Beta mail" },
  { id: 3, windowId: 2, index: 0, active: false, url: "https://c.example/z", title: "" },
  { id: 4, windowId: 1, index: 2, active: false, url: "about:blank", title: "Blank" },
];

// objects made inside the fake browser have another Object prototype: compare them as plain data
const plain = (x) => JSON.parse(JSON.stringify(x));

const submitted = (button, values) => ({ panel: "", outcome: "submitted", button, values: values || {}, reason: "" });

test("show sends the panel with a request number and resolves with the answer", async () => {
  const w = world(TABS);
  const p = w.CP.ui.show({ id: "x", title: "Hi" });
  const m = w.sent[0];
  assert.equal(m.type, "panel");
  assert.equal(m.spec.title, "Hi");
  assert.equal(m.spec.timeout_s, 120, "the extension says how long it waits");
  w.answer(submitted("ok", { a: "b" }));
  const r = await p;
  assert.equal(r.outcome, "submitted");
  assert.equal(r.values.a, "b");
});

test("two panels are told apart by their request number", async () => {
  const w = world(TABS);
  const a = w.CP.ui.show({ title: "A" });
  const b = w.CP.ui.show({ title: "B" });
  assert.notEqual(w.sent[0].req, w.sent[1].req);
  w.answer(submitted("second"), w.sent[1].req);
  w.answer(submitted("first"), w.sent[0].req);
  assert.equal((await a).button, "first");
  assert.equal((await b).button, "second");
});

test("without a connection a panel is unavailable at once, and never rejects", async () => {
  const w = world(TABS);
  w.CP.ui.transport = () => false;
  const r = await w.CP.ui.show({ title: "x" });
  assert.equal(r.outcome, "unavailable");
  assert.match(r.reason, /not running/);
  assert.equal((await w.CP.ui.show(null)).outcome, "unavailable");
  assert.equal((await w.CP.ui.show([1])).outcome, "unavailable");
});

test("a closed connection ends every panel that was waiting", async () => {
  const w = world(TABS);
  const p = w.CP.ui.show({ id: "a" });
  w.CP.ui.disconnected();
  const r = await p;
  assert.equal(r.outcome, "unavailable");
  assert.equal(r.panel, "a");
});

test("an answer nobody asked for is ignored, and a strange one is made safe", async () => {
  const w = world(TABS);
  w.CP.ui.handle({ type: "panel_result", req: 999, result: {} });
  const p = w.CP.ui.show({});
  w.CP.ui.handle({ type: "panel_result", req: w.sent[0].req, result: "nonsense" });
  const r = await p;
  assert.equal(r.outcome, "unavailable");
  assert.deepEqual(plain(r.values), {});
});

test("the extension never asks for more than five minutes", () => {
  const w = world(TABS);
  w.CP.ui.show({ timeout_s: 99999 });
  w.CP.ui.show({ timeout_s: 0 });
  assert.equal(w.sent[0].spec.timeout_s, 300);
  assert.equal(w.sent[1].spec.timeout_s, 120);
});

test("update and close are sent by panel id", () => {
  const w = world(TABS);
  assert.equal(w.CP.ui.update("p", { widget: "bar", value: 0.5 }), true);
  assert.equal(w.CP.ui.close("p"), true);
  assert.deepEqual(plain(w.sent[0]), { type: "panel_update", panel: "p", patch: { widget: "bar", value: 0.5 } });
  assert.deepEqual(plain(w.sent[1]), { type: "panel_close", panel: "p" });
});

test("confirm, prompt and pick return a plain answer", async () => {
  const w = world(TABS);
  let p = w.CP.ui.confirm("Sure?");
  w.answer(submitted("yes"));
  assert.equal(await p, true);
  p = w.CP.ui.confirm("Sure?");
  w.answer({ outcome: "cancelled", button: "no", values: {} });
  assert.equal(await p, false);
  p = w.CP.ui.prompt("Name");
  assert.equal(w.sent[w.sent.length - 1].spec.widgets[0].required, true);
  w.answer(submitted("ok", { value: "Ann" }));
  assert.equal(await p, "Ann");
  p = w.CP.ui.pick(["a", "b"]);
  assert.equal(w.sent[w.sent.length - 1].spec.widgets[0].submit, true);
  w.answer(submitted("pick", { pick: "b" }));
  assert.equal(await p, "b");
  p = w.CP.ui.pick(["a"]);
  w.answer({ outcome: "timeout", values: {} });
  assert.equal(await p, null);
});

test("ui.tabs lists the other web pages, this window first, and goes to the one picked", async () => {
  const w = world(TABS);
  const run = w.CP.dispatch("ui.tabs", "");
  await new Promise((r) => setImmediate(r));
  const spec = w.sent[0].spec;
  const opts = spec.widgets[0].options;
  assert.equal(spec.widgets[0].submit, true);
  assert.deepEqual(opts.map((o) => o.value), ["2", "3"], "no other window's page before this window's; about: pages left out");
  assert.equal(opts[0].label, "Beta mail");
  assert.equal(opts[1].label, "c.example", "the host stands in for a missing title");
  w.answer(submitted("pick", { tab: "3" }));
  const res = await run;
  assert.equal(res.ok, true);
  assert.deepEqual(w.shown, [3]);
});

test("a cancelled panel does nothing and a refused one says why", async () => {
  let w = world(TABS);
  let run = w.CP.dispatch("ui.tabs", "");
  await new Promise((r) => setImmediate(r));
  w.answer({ outcome: "cancelled", button: "cancel", values: {} });
  let res = await run;
  assert.equal(res.ok, true);
  assert.deepEqual(w.shown, []);
  w = world(TABS);
  run = w.CP.dispatch("ui.media", "");
  await new Promise((r) => setImmediate(r));
  w.answer({ outcome: "unavailable", reason: "Panels from the browser extension are switched off", values: {} });
  res = await run;
  assert.equal(res.ok, false);
  assert.match(res.message, /switched off/);
});

test("ui.confirm_close_others counts the tabs and closes them only after a yes", async () => {
  let w = world(TABS);
  let run = w.CP.dispatch("ui.confirm_close_others", "");
  await new Promise((r) => setImmediate(r));
  assert.match(w.sent[0].spec.widgets[0].text, /Close 2 other tabs/);
  w.answer(submitted("no"));
  await run;
  assert.deepEqual(w.closed, []);
  w = world(TABS);
  run = w.CP.dispatch("ui.confirm_close_others", "");
  await new Promise((r) => setImmediate(r));
  w.answer(submitted("yes"));
  await run;
  assert.deepEqual(w.closed.sort(), [2, 4]);
});

test("ui.media only runs the commands of its menu", async () => {
  const w = world(TABS);
  let run = w.CP.dispatch("ui.media", "");
  await new Promise((r) => setImmediate(r));
  const values = w.sent[0].spec.widgets[0].options.map((o) => o.value);
  assert.ok(values.includes("media.toggle") && values.every((v) => v.startsWith("media.")));
  w.answer(submitted("pick", { action: "tab.close" }));
  const res = await run;
  assert.equal(res.ok, false, "a value outside the menu is refused");
});

test("the ui commands are switched off with the group", async () => {
  const w = world(TABS);
  await w.CP.loadSettings();
  w.CP.settings().ui = false;
  const res = await w.CP.dispatch("ui.tabs", "");
  assert.equal(res.ok, false);
  assert.match(res.message, /switched off/);
  assert.equal(w.sent.length, 0);
});
