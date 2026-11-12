// Tests of the ui.video command (the video remote, cmd-ui.js).
// They need no browser and no jsdom: the browser API and the media commands are small fakes.
//
//   cd tests/browser-extension && node --test video.test.js
"use strict";

const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const DIR = path.join(__dirname, "..", "..", "assets", "browser-extension");
const read = (f) => fs.readFileSync(path.join(DIR, f), "utf8");

function world() {
  const sent = [];
  const calls = [];
  const chrome = {
    storage: { local: { get: async () => ({}) }, session: { get: async () => ({}), set: async () => {} }, onChanged: { addListener() {} } },
    tabs: { query: async () => [] },
    windows: { update: async () => {} },
  };
  const timer = (f, ms) => {
    const t = setTimeout(f, ms < 5000 ? 1 : ms);
    if (t && t.unref) t.unref();
    return t;
  };
  const ctx = vm.createContext({ chrome, console, setTimeout: timer, clearTimeout, URL });
  ctx.self = ctx;
  for (const f of ["util.js", "ui.js", "cmd-ui.js"]) vm.runInContext(read(f), ctx, { filename: f });
  const CP = ctx.CP;
  // the media commands the remote may call: fakes that record what they were given
  for (const id of ["media.prev", "media.next", "media.toggle", "media.play", "media.pause", "media.restart", "media.seek_fwd", "media.seek_back", "media.volume_up", "media.volume_down", "media.mute", "media.speed_up", "media.speed_down", "media.speed_reset", "media.captions", "media.skip_ad", "media.like", "media.focus"]) {
    CP.reg(id, "media", async ({ arg }) => {
      calls.push([id, arg]);
      return CP.ok();
    });
  }
  CP.reg("tab.close", "tab", async () => {
    calls.push(["tab.close", ""]);
    return CP.ok();
  });
  CP.mediaTab = async () => ({ tab: { id: 7 }, playing: true });
  CP.pageMedia = () => {};
  CP.runInTab = async () => ({ title: "A long video", playing: true });
  CP.ui.transport = (m) => {
    sent.push(m);
    return true;
  };
  const panels = () => sent.filter((m) => m.type === "panel");
  // waits for the n-th panel to be shown, then answers it
  const answer = async (n, result) => {
    for (let i = 0; i < 200 && panels().length < n; i++) await new Promise((r) => setTimeout(r, 2));
    assert.ok(panels().length >= n, "panel " + n + " was shown");
    CP.ui.handle({ type: "panel_result", req: panels()[n - 1].req, result });
  };
  return { CP, sent, calls, panels, answer };
}

// objects made inside the fake browser have another Object prototype: compare them as plain data
const plain = (x) => JSON.parse(JSON.stringify(x));

const picked = (action, step) => ({ panel: "ui.video", outcome: "submitted", button: "pick", values: { action, step: step === undefined ? 10 : step }, reason: "" });
const closed = { panel: "ui.video", outcome: "cancelled", button: "close", values: {}, reason: "" };

test("the remote stays open: every press runs the command and shows the panel again", async () => {
  const w = world();
  const run = w.CP.dispatch("ui.video", "");
  await w.answer(1, picked("media.next"));
  await w.answer(2, picked("media.toggle"));
  await w.answer(3, picked("media.prev"));
  await w.answer(4, closed);
  const res = await run;
  assert.equal(res.ok, true);
  assert.deepEqual(plain(w.calls.map((c) => c[0])), ["media.next", "media.toggle", "media.prev"]);
  assert.equal(w.panels().length, 4);
});

test("the panel is a chip list that answers at once, with a slider and a Close button", async () => {
  const w = world();
  const run = w.CP.dispatch("ui.video", "");
  await w.answer(1, closed);
  await run;
  const spec = w.panels()[0].spec;
  assert.equal(spec.id, "ui.video");
  assert.equal(spec.subtitle, "Playing: A long video");
  const [choice, slider] = spec.widgets;
  assert.equal(choice.type, "choice");
  assert.equal(choice.submit, true);
  assert.equal(choice.layout, "chips");
  assert.ok(choice.options.every((o) => o.value.startsWith("media.")));
  assert.deepEqual(plain(choice.options.slice(0, 5).map((o) => o.value)), ["media.prev", "media.seek_back", "media.toggle", "media.seek_fwd", "media.next"]);
  assert.equal(slider.type, "slider");
  assert.equal(slider.value, 10);
  assert.ok(spec.buttons.length <= 4 && spec.buttons[0].cancel === true);
});

test("Back and Forward jump by the slider, and the labels follow it", async () => {
  const w = world();
  const run = w.CP.dispatch("ui.video", "");
  await w.answer(1, picked("media.seek_fwd", 30));
  await w.answer(2, picked("media.seek_back", 999)); // out of range: kept inside the slider
  await w.answer(3, closed);
  await run;
  assert.deepEqual(plain(w.calls), [["media.seek_fwd", "30"], ["media.seek_back", "60"]]);
  const labels = (n) => w.panels()[n].spec.widgets[0].options.map((o) => o.label);
  assert.ok(labels(1).includes("Forward 30s"));
  assert.ok(labels(2).includes("Back 60s"));
});

test("a command that is not on the remote is refused, and the remote stays open", async () => {
  const w = world();
  const run = w.CP.dispatch("ui.video", "");
  await w.answer(1, picked("tab.close"));
  await w.answer(2, closed);
  await run;
  assert.deepEqual(plain(w.calls), []);
  assert.equal(w.panels()[1].spec.subtitle, "Unknown choice");
});

test("a failing command is said on the next panel", async () => {
  const w = world();
  w.CP.reg("media.like", "media", async () => w.CP.fail("This page has no like button"));
  const run = w.CP.dispatch("ui.video", "");
  await w.answer(1, picked("media.like"));
  await w.answer(2, closed);
  await run;
  assert.equal(w.panels()[1].spec.subtitle, "This page has no like button");
});

test("panels that cannot be shown say why, and Esc or a timeout end it quietly", async () => {
  let w = world();
  let run = w.CP.dispatch("ui.video", "");
  await w.answer(1, { outcome: "unavailable", reason: "Panels from the browser extension are switched off", values: {} });
  let res = await run;
  assert.equal(res.ok, false);
  assert.match(res.message, /switched off/);
  w = world();
  run = w.CP.dispatch("ui.video", "");
  await w.answer(1, { outcome: "timeout", values: {} });
  res = await run;
  assert.equal(res.ok, true);
});
