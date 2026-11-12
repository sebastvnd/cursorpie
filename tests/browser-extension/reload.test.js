// Tests of the reload that follows an update of CursorPie: the app asks the extension to reload
// itself (so nobody has to go to the extensions page), once per version. No jsdom needed: the
// extension runs in a vm against a small fake of the `chrome` API, so `node --test` can run this
// file on its own.
"use strict";

const test = require("node:test");
const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const DIR = path.join(__dirname, "..", "..", "assets", "browser-extension");

// `version`: what the loaded manifest says; `store`: what `storage.local` holds; `broken`: storage fails
function load({ version = "0.8.9", store = {}, broken = false } = {}) {
  const state = { reloads: 0, store: { ...store } };
  const chrome = {
    storage: {
      local: {
        get: async (key) => {
          if (broken) throw new Error("storage is gone");
          return key in state.store ? { [key]: state.store[key] } : {};
        },
        set: async (items) => {
          if (broken) throw new Error("storage is gone");
          Object.assign(state.store, items);
        },
      },
      session: undefined,
      onChanged: { addListener() {} },
    },
    runtime: {
      getManifest: () => ({ version }),
      reload: () => {
        state.reloads += 1;
      },
    },
    tabs: {},
    windows: {},
  };
  const ctx = { chrome, console, setTimeout, clearTimeout, Promise, self: null };
  ctx.self = ctx;
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(DIR, "util.js"), "utf8"), ctx, { filename: "util.js" });
  return { CP: ctx.CP, state };
}

test("an extension of another version reloads itself when the app asks", async () => {
  const { CP, state } = load({ version: "0.8.9" });
  assert.strictEqual(await CP.reloadFor("0.9.0"), true);
  assert.strictEqual(state.reloads, 1);
  assert.strictEqual(state.store.reloadAsked, "0.9.0");
});

test("the same request again does not reload again, so a folder that was not refreshed cannot loop", async () => {
  // the extension came back as 0.8.9 after the reload: the folder it is loaded from is still old
  const { CP, state } = load({ version: "0.8.9", store: { reloadAsked: "0.9.0" } });
  assert.strictEqual(await CP.reloadFor("0.9.0"), false);
  assert.strictEqual(state.reloads, 0);
});

test("a newer version asked later reloads again", async () => {
  const { CP, state } = load({ version: "0.8.9", store: { reloadAsked: "0.9.0" } });
  assert.strictEqual(await CP.reloadFor("0.9.1"), true);
  assert.strictEqual(state.reloads, 1);
});

test("an extension that already is that version does nothing", async () => {
  const { CP, state } = load({ version: "0.9.0" });
  assert.strictEqual(await CP.reloadFor("0.9.0"), false);
  assert.strictEqual(state.reloads, 0);
  assert.deepStrictEqual(JSON.stringify(state.store), "{}");
});

test("a request without a usable version is ignored", async () => {
  const { CP, state } = load();
  for (const bad of [undefined, null, "", 9, {}, []]) {
    assert.strictEqual(await CP.reloadFor(bad), false);
  }
  assert.strictEqual(state.reloads, 0);
});

test("when the version cannot be remembered nothing reloads (it could not be stopped from looping)", async () => {
  const { CP, state } = load({ broken: true });
  assert.strictEqual(await CP.reloadFor("0.9.0"), false);
  assert.strictEqual(state.reloads, 0);
});

test("background.js passes the app's request to CP.reloadFor", () => {
  const text = fs.readFileSync(path.join(DIR, "background.js"), "utf8");
  assert.match(text, /m\.type === "reload"\) CP\.reloadFor\(m\.version\)/);
});
