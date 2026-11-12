// Tests of the chat commands a flow can use to read and wait: chat.unread, chat.who,
// chat.unread_names, chat.last, chat.read and chat.wait. Like flow-commands.test.js they do not need
// jsdom: the extension runs in a vm against a small fake of the `chrome` API, and what the chat page
// answers is scripted by the test (the page side is tested in extension.test.js, against small copies
// of the pages). Run on its own with `node --test chat-flow.test.js`.
"use strict";

const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const DIR = path.join(__dirname, "..", "..", "assets", "browser-extension");

// `tabs`: the chat tabs; `page(action, opts, tabId)`: what the chat page answers to an action;
// `settings`: the options. Time is fake: CP.sleep moves the clock, so a wait of 60 s takes no time.
function load({ tabs, page, settings = {} }) {
  const clock = { now: 1_000_000 };
  const calls = [];
  const chrome = {
    storage: { local: { get: async () => ({}) }, session: undefined, onChanged: { addListener() {} } },
    tabs: {
      query: async (q) => tabs.filter((t) => !q.active || t.active),
      update: async () => {},
      get: async (id) => tabs.find((t) => t.id === id),
    },
    windows: { update: async () => {} },
    runtime: {},
  };
  class FakeDate {
    static now() {
      return clock.now;
    }
  }
  const ctx = { chrome, console, setTimeout, clearTimeout, Promise, URL, Date: FakeDate, self: null };
  ctx.self = ctx;
  vm.createContext(ctx);
  for (const f of ["util.js", "page-chat.js", "cmd-chat.js"]) {
    vm.runInContext(fs.readFileSync(path.join(DIR, f), "utf8"), ctx, { filename: f });
  }
  const CP = ctx.CP;
  CP.saveMem = () => {};
  CP.sleep = async (ms) => {
    clock.now += ms;
  };
  CP.show = async () => {};
  // runInTab is what reaches the page: the test answers in its place
  CP.runInTab = async (tabId, fn, args) => {
    calls.push(args[0]);
    return page(args[0], args[1], tabId);
  };
  const all = Object.assign({}, CP.DEFAULTS, settings);
  const run = async (cmd, arg = "") => JSON.parse(JSON.stringify(await CP.commands[cmd].run({ arg, settings: all })));
  return { CP, run, calls, clock };
}

const wa = (extra) => Object.assign({ id: 1, windowId: 1, active: true, title: "WhatsApp", url: "https://web.whatsapp.com/" }, extra);
const tg = (extra) => Object.assign({ id: 2, windowId: 1, active: false, title: "Telegram", url: "https://web.telegram.org/k/" }, extra);
const msg = (dir, text, key) => ({ dir, text, key: key || dir + ":" + text });
const ok = (extra) => Object.assign({ ok: true, msg: "", site: "whatsapp" }, extra);
const READ = { allowRead: true };

test("every command that reads a chat refuses while 'Allow reading messages' is off", async () => {
  const { run, calls } = load({ tabs: [wa()], page: () => ok({ messages: [msg("in", "secret")], name: "Anna", names: [] }) });
  for (const cmd of ["chat.who", "chat.unread_names", "chat.last", "chat.read", "chat.wait"]) {
    const r = await run(cmd);
    assert.strictEqual(r.ok, false, cmd);
    assert.match(r.message, /Allow reading messages/, cmd);
  }
  assert.deepStrictEqual(calls, [], "the page was not even asked");
});

test("reading is off unless the person turns it on", () => {
  const { CP } = load({ tabs: [], page: () => ok() });
  assert.strictEqual(CP.DEFAULTS.allowRead, false);
  assert.strictEqual(CP.DEFAULTS.allowSend, false);
});

test("chat.unread gives a plain number, adding the chat sites that are open", async () => {
  const { run } = load({ tabs: [wa({ title: "(3) WhatsApp" }), tg({ title: "(2) Telegram" })], page: () => ok() });
  assert.strictEqual((await run("chat.unread")).message, "5");
  assert.strictEqual((await run("chat.unread", "@telegram")).message, "2");
});

test("chat.unread asks the page when the title has no number, and works without 'Allow reading'", async () => {
  const { run } = load({ tabs: [wa()], page: (a) => (a === "info" ? ok({ ready: true, unreadChats: 4 }) : ok()) });
  const r = await run("chat.unread");
  assert.deepStrictEqual(r, { ok: true, message: "4", notify: true });
});

test("chat.unread fails with a reason when no chat is open or the page cannot be read", async () => {
  let r = await load({ tabs: [], page: () => ok() }).run("chat.unread");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /not open/);
  r = await load({ tabs: [wa()], page: () => ok({ ready: false }) }).run("chat.unread");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /Could not read/);
  r = await load({ tabs: [wa()], page: () => ok() }).run("chat.unread", "@telegram");
  assert.match(r.message, /Telegram is not open/);
});

test("chat.who and chat.unread_names answer with the names", async () => {
  const page = (a) =>
    a === "header" ? ok({ name: "Anna" }) : a === "unread_names" ? ok({ names: [{ name: "Anna", unread: 3 }, { name: "Team Kopi", unread: 1 }] }) : ok();
  const { run } = load({ tabs: [wa()], page, settings: READ });
  assert.strictEqual((await run("chat.who")).message, "Anna");
  assert.strictEqual((await run("chat.unread_names")).message, "Anna\nTeam Kopi");
  const none = load({ tabs: [wa()], page: (a) => (a === "header" ? ok({ name: "" }) : ok({ names: [] })), settings: READ });
  assert.strictEqual((await none.run("chat.who")).ok, false);
  const empty = await none.run("chat.unread_names");
  assert.strictEqual(empty.ok, true);
  assert.strictEqual(empty.message, "", "nothing waits: an empty list, not an error");
});

test("chat.last gives the last message that came in, not the last one sent", async () => {
  const list = [msg("in", "Jadi jam 5?"), msg("out", "Jadi!")];
  const { run, calls } = load({ tabs: [wa()], page: (a) => (a === "messages" ? ok({ messages: list, name: "Anna" }) : ok()), settings: READ });
  const r = await run("chat.last");
  assert.deepStrictEqual(r, { ok: true, message: "Jadi jam 5?", notify: true });
  assert.ok(calls.includes("bottom"), "it reads the end of the conversation");
});

test("chat.last is empty, not an error, when nothing came in", async () => {
  const { run } = load({ tabs: [wa()], page: (a) => (a === "messages" ? ok({ messages: [msg("out", "hi")], name: "Anna" }) : ok()), settings: READ });
  const r = await run("chat.last");
  assert.strictEqual(r.ok, true);
  assert.strictEqual(r.message, "");
});

test("chat.last fails with the page's reason when no chat is open", async () => {
  const page = (a) => (a === "messages" ? { ok: false, msg: "No messages found (is a chat open?)" } : ok());
  const r = await load({ tabs: [wa()], page, settings: READ }).run("chat.last");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /is a chat open/);
});

test("chat.read tells who wrote each line and limits how many", async () => {
  const asked = [];
  const list = [msg("in", "Halo"), msg("out", "Hai"), msg("in", "baris 1\nbaris 2")];
  const page = (a, o) => (a === "messages" ? (asked.push(o.n), ok({ messages: list, name: "Anna" })) : ok());
  const { run } = load({ tabs: [wa()], page, settings: READ });
  assert.strictEqual((await run("chat.read")).message, "them: Halo\nme: Hai\nthem: baris 1\nbaris 2");
  await run("chat.read", "3");
  await run("chat.read", "500");
  await run("chat.read", "0");
  assert.deepStrictEqual(asked, [5, 3, 20, 1], "default 5, between 1 and 20");
});

test("a command that reads does not bring the tab to the front, one that types does", async () => {
  const shown = [];
  const t = load({ tabs: [wa()], page: (a) => (a === "messages" ? ok({ messages: [], name: "A" }) : ok()), settings: READ });
  t.CP.show = async (tab) => shown.push(tab.id);
  await t.run("chat.last");
  await t.run("chat.wait", "1");
  assert.deepStrictEqual(shown, []);
  await t.run("chat.next");
  assert.deepStrictEqual(shown, [1]);
});

// A chat that gets a new message after some polls of the page
function conversation(steps) {
  let polls = 0;
  const page = (a) => {
    if (a !== "messages") return ok();
    const list = steps[Math.min(polls, steps.length - 1)];
    polls++;
    return ok({ messages: list, name: "Anna" });
  };
  return { page, polls: () => polls };
}

test("chat.wait gives the message that comes while it waits", async () => {
  const a = msg("in", "pertama", "k1");
  const c = conversation([[a], [a], [a], [a, msg("in", "kedua", "k2")]]);
  const { run } = load({ tabs: [wa()], page: c.page, settings: READ });
  const r = await run("chat.wait", "30");
  assert.deepStrictEqual(r, { ok: true, message: "kedua", notify: true });
  assert.strictEqual(c.polls(), 4);
});

test("chat.wait gives an empty answer, still ok, when nothing comes in time", async () => {
  const a = msg("in", "pertama", "k1");
  const { run, clock } = load({ tabs: [wa()], page: conversation([[a]]).page, settings: READ });
  const before = clock.now;
  const r = await run("chat.wait", "10");
  assert.deepStrictEqual(r, { ok: true, message: "", notify: false });
  assert.ok(clock.now - before >= 10_000 && clock.now - before < 12_000, "it waited the 10 seconds it was given");
});

test("chat.wait has a default of a minute and never more than five", async () => {
  const a = msg("in", "x", "k1");
  let t = load({ tabs: [wa()], page: conversation([[a]]).page, settings: READ });
  let before = t.clock.now;
  await t.run("chat.wait");
  assert.ok(Math.abs(t.clock.now - before - 60_000) < 2_000);
  t = load({ tabs: [wa()], page: conversation([[a]]).page, settings: READ });
  before = t.clock.now;
  await t.run("chat.wait", "9999");
  assert.ok(Math.abs(t.clock.now - before - 300_000) < 2_000);
});

test("chat.wait returns at once for a message that came after the last one a read command saw", async () => {
  const a = msg("in", "pertama", "k1");
  const b = msg("in", "kedua", "k2");
  const c = conversation([[a], [a, b], [a, b]]);
  const { run } = load({ tabs: [wa()], page: c.page, settings: READ });
  await run("chat.last"); // saw k1
  const r = await run("chat.wait", "30"); // k2 arrived meanwhile
  assert.strictEqual(r.message, "kedua");
  // and it is not given twice
  const again = await run("chat.wait", "2");
  assert.strictEqual(again.message, "", "k2 was already given");
});

test("a message that is sent notes what is already there, so a quick answer is not missed and an old one is not taken", async () => {
  const old = msg("in", "lama", "k0");
  const reply = msg("in", "jawaban", "k9");
  // the answer is in the page as soon as the message has been sent, before the wait begins
  const chat = (answers) => {
    let sent = false;
    return (a) => {
      if (a === "send") sent = true;
      if (a === "messages") return ok({ messages: sent && answers ? [old, msg("out", "pertanyaan", "o1"), reply] : [old], name: "Anna" });
      return ok();
    };
  };
  const both = Object.assign({ allowSend: true }, READ);
  const t = load({ tabs: [wa()], page: chat(true), settings: both });
  assert.strictEqual((await t.run("chat.send", "pertanyaan")).ok, true);
  assert.strictEqual((await t.run("chat.wait", "30")).message, "jawaban", "the answer came before the wait began");

  // nobody answered: the old message, seen before the send, is not the answer
  const t2 = load({ tabs: [wa()], page: chat(false), settings: both });
  await t2.run("chat.send", "pertanyaan");
  assert.strictEqual((await t2.run("chat.wait", "3")).message, "");
});

test("chat.wait stops with a reason when the open chat changes", async () => {
  const a = msg("in", "x", "k1");
  let n = 0;
  const page = (act) => (act === "messages" ? ok({ messages: [a], name: n++ < 2 ? "Anna" : "Budi" }) : ok());
  const { run } = load({ tabs: [wa()], page, settings: READ });
  const r = await run("chat.wait", "30");
  assert.strictEqual(r.ok, false);
  assert.match(r.message, /chat changed/);
});

test("chat.wait fails at once when no chat is open, and puts up with a page that is busy for a moment", async () => {
  const closed = load({ tabs: [wa()], page: (a) => (a === "messages" ? { ok: false, msg: "No messages found (is a chat open?)" } : ok()), settings: READ });
  const r = await closed.run("chat.wait", "30");
  assert.strictEqual(r.ok, false);
  assert.ok(closed.calls.filter((c) => c === "messages").length === 1, "no point in waiting for a chat that is not open");

  const a = msg("in", "x", "k1");
  let n = 0;
  const page = (act) => {
    if (act !== "messages") return ok();
    n++;
    return n === 3 ? { ok: false, msg: "busy" } : ok({ messages: n < 5 ? [a] : [a, msg("in", "y", "k2")], name: "Anna" });
  };
  const busy = load({ tabs: [wa()], page, settings: READ });
  assert.strictEqual((await busy.run("chat.wait", "30")).message, "y");
});

test("the messages that are remembered are never the text of a message", async () => {
  // no page id: what is kept to tell one message from the next is a number made from the text
  const a = { dir: "in", text: "very private words", key: "" };
  const t = load({ tabs: [wa()], page: conversation([[a]]).page, settings: READ });
  await t.run("chat.last");
  assert.ok(!JSON.stringify(t.CP.mem).includes("private"), JSON.stringify(t.CP.mem));
});

test("the answer to a command can be longer than a notification", () => {
  const { CP } = load({ tabs: [], page: () => ok() });
  assert.ok(CP.MAX_REPLY >= 2000, "a message of 2000 characters has to come back whole");
});

test("the app and the extension know the new commands", () => {
  const { CP } = load({ tabs: [], page: () => ok() });
  for (const id of ["chat.unread", "chat.who", "chat.unread_names", "chat.last", "chat.read", "chat.wait"]) {
    assert.ok(CP.commands[id], id);
    assert.strictEqual(CP.commands[id].group, "chat");
  }
});
