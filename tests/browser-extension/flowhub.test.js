// Tests of the Flow Hub door of the extension: the content script that runs on the Flow Hub page
// (flowhub-page.js) and the relay in background.js. Run with:
//
//     cd tests/browser-extension && npm install && npm test
//
// The background runs in a vm against a fake `chrome`; the page script runs in jsdom, and its
// `runtime.sendMessage` goes straight to the listener the background registered, with the `sender` a
// browser would give (which is what the background checks). The "app" is a function of the test that
// answers what the native port is sent.
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
const SCRIPTS = [
    "util.js",
    "page-media.js",
    "page-chat.js",
    "ui.js",
    "cmd-tab.js",
    "cmd-media.js",
    "cmd-chat.js",
    "cmd-ui.js",
];
const HUB = "https://sebastvnd.github.io/cursorpie/flowhub/";

// The extension with a native port whose app is `app(message, send)`; with no `app` nobody answers,
// like the host does while CursorPie is not running. `connectable: false` makes connectNative throw.
async function extension({ app, connectable = true } = {}) {
    const posted = [];
    const l = {};
    let sink = null;
    const chrome = {
        runtime: {
            id: "ext-id",
            connectNative: () => {
                if (!connectable) throw new Error("no host");
                sink = (m) => l.host && l.host(m);
                return {
                    postMessage: (m) => {
                        posted.push(m);
                        if (app) app(m, sink);
                    },
                    onMessage: { addListener: (f) => (l.host = f) },
                    onDisconnect: { addListener: (f) => (l.disconnect = f) },
                };
            },
            getManifest: () => ({ version: "9.9.9" }),
            lastError: undefined,
            onStartup: { addListener() {} },
            onInstalled: { addListener() {} },
            onMessage: { addListener: (f) => (l.message = f) },
        },
        alarms: { create() {}, onAlarm: { addListener() {} } },
        storage: {
            local: { get: async () => ({}), set: async () => {} },
            onChanged: { addListener() {} },
        },
        permissions: { contains: async () => false },
        sessions: {},
        tabs: {
            onCreated: { addListener() {} },
            onRemoved: { addListener() {} },
            onReplaced: { addListener() {} },
            onAttached: { addListener() {} },
            onDetached: { addListener() {} },
            onActivated: { addListener() {} },
            onUpdated: { addListener() {} },
            query: async () => [],
        },
        windows: {},
        scripting: {},
    };
    const ctx = vm.createContext({
        chrome,
        console,
        setTimeout,
        clearTimeout,
        URL,
        navigator: { userAgent: "Chrome/120" },
    });
    ctx.self = ctx;
    for (const f of SCRIPTS.concat("background.js"))
        vm.runInContext(read(f), ctx, { filename: f });
    await sleep(30);
    return { chrome, posted, l };
}

// What a browser passes to the listener for a message from a content script.
const sender = (over) =>
    Object.assign(
        { id: "ext-id", tab: { id: 1, url: HUB }, frameId: 0, url: HUB },
        over,
    );

// Calls the background like a content script does and waits for sendResponse.
function call(ext, message, from) {
    return new Promise((resolve) => {
        const keep = ext.l.message(message, from || sender(), resolve);
        if (keep !== true) resolve("sync");
    });
}

// The Flow Hub page in jsdom with the content script in it, wired to `ext`.
function hubPage(ext, url = HUB) {
    const dom = new JSDOM("<!doctype html><body></body>", {
        url,
        runScripts: "outside-only",
        pretendToBeVisual: true,
    });
    const w = dom.window;
    w.chrome = {
        runtime: {
            getManifest: () => ({ version: "9.9.9" }),
            sendMessage: (message) =>
                new Promise((resolve) =>
                    ext.l.message(message, sender({ url }), resolve),
                ),
        },
    };
    // jsdom delivers a posted message without its `source` and `origin`, which a browser always sets
    // and the page script checks: deliver it the way a browser does
    w.postMessage = (data) =>
        setTimeout(
            () =>
                w.dispatchEvent(
                    new w.MessageEvent("message", {
                        data,
                        origin: w.location.origin,
                        source: w,
                    }),
                ),
            0,
        );
    w.eval(read("flowhub-page.js"));
    const seen = [];
    w.addEventListener(
        "message",
        (e) => e.data && e.data.cursorpie === "bridge" && seen.push(e.data),
    );
    let id = 0;
    const say = (m) =>
        w.postMessage(
            Object.assign({ cursorpie: "flowhub", v: 1, id: ++id }, m),
            url.replace(/^(https:\/\/[^/]+).*/, "$1"),
        );
    const wait = async (type) => {
        for (let i = 0; i < 300; i++) {
            const r = seen.find((m) => m.type === type && m.id === id);
            if (r) return r;
            await sleep(10);
        }
        throw new Error("no " + type);
    };
    return { w, seen, say, wait };
}

const FLOW = JSON.stringify({
    cursorpie_flow: 2,
    flow: { id: "x", name: "X", nodes: [] },
});

// ---------------------------------------------------------------------------------- the relay

test("a ping is answered by the app, with whether it would take a flow", async () => {
    const ext = await extension({
        app: (m, send) =>
            m.type === "flow_ping" &&
            send({ type: "flow_pong", req: m.req, install: true, reason: "" }),
    });
    assert.deepEqual(
        { ...(await call(ext, { kind: "flowhub_ping" })) },
        { app: true, install: true, reason: "" },
    );
    const off = await extension({
        app: (m, send) =>
            m.type === "flow_ping" &&
            send({
                type: "flow_pong",
                req: m.req,
                install: false,
                reason: "switched off",
            }),
    });
    assert.deepEqual(
        { ...(await call(off, { kind: "flowhub_ping" })) },
        { app: true, install: false, reason: "switched off" },
    );
});

test("no answer to a ping means the app is not running", async () => {
    const ext = await extension({}); // the host drops what is said while the app is not running
    const t = Date.now();
    assert.deepEqual(
        { ...(await call(ext, { kind: "flowhub_ping" })) },
        { app: false },
    );
    assert.ok(Date.now() - t < 3000, "it gives up quickly");
});

test("with no native host at all the answer is the same, and nothing throws", async () => {
    const ext = await extension({ connectable: false });
    assert.deepEqual(
        { ...(await call(ext, { kind: "flowhub_ping" })) },
        { app: false },
    );
    const r = await call(ext, { kind: "flowhub_install", flow: FLOW });
    assert.equal(r.state, "offline");
});

test("an install goes to the app as it is and the answer comes back with the same number", async () => {
    const ext = await extension({
        app: (m, send) =>
            m.type === "flow_install" &&
            send({
                type: "flow_result",
                req: m.req,
                state: "installed",
                message: "ok",
            }),
    });
    const r = await call(ext, {
        kind: "flowhub_install",
        author: "sebastvnd",
        flow: FLOW,
    });
    assert.deepEqual({ ...r }, { state: "installed", message: "ok" });
    const sent = ext.posted.find((m) => m.type === "flow_install");
    assert.equal(sent.flow, FLOW, "the text is not changed");
    assert.equal(sent.author, "sebastvnd");
    assert.equal(typeof sent.req, "number");
});

test("two requests at once are told apart by their number", async () => {
    const held = [];
    const ext = await extension({
        app: (m, send) =>
            m.type === "flow_install" &&
            held.push(() =>
                send({
                    type: "flow_result",
                    req: m.req,
                    state: m.flow === "a" ? "declined" : "installed",
                    message: m.flow,
                }),
            ),
    });
    const a = call(ext, { kind: "flowhub_install", flow: "a" });
    const b = call(ext, { kind: "flowhub_install", flow: "b" });
    await sleep(20);
    held.reverse().forEach((f) => f()); // answered in the other order
    assert.equal((await a).state, "declined");
    assert.equal((await b).state, "installed");
});

test("a flow that is not text, empty or too big never reaches the app", async () => {
    const ext = await extension({
        app: () => assert.fail("nothing should be sent"),
    });
    for (const flow of [undefined, 5, "", "x".repeat(1024 * 1024 + 1)]) {
        const r = await call(ext, { kind: "flowhub_install", flow });
        assert.equal(r.state, "invalid");
    }
    assert.equal(ext.posted.filter((m) => m.type === "flow_install").length, 0);
});

test("the link closing ends the wait at once", async () => {
    const ext = await extension({ app: () => {} }); // takes the message and never answers
    const r = call(ext, { kind: "flowhub_install", flow: FLOW });
    await sleep(20);
    ext.l.disconnect();
    const out = await r;
    assert.equal(out.state, "offline");
});

test("only the content script of the Flow Hub page is served", async () => {
    const ext = await extension({
        app: (m, send) =>
            m.type === "flow_ping" &&
            send({ type: "flow_pong", req: m.req, install: true, reason: "" }),
    });
    const ping = { kind: "flowhub_ping" };
    const refused = [
        sender({ id: "another-extension" }), // another extension
        sender({ tab: undefined }), // no tab: not a content script
        sender({ frameId: 3 }), // a frame inside the page
        sender({ url: "https://sebastvnd.github.io/cursorpie/index.html" }), // the same site, another page
        sender({ url: "https://sebastvnd.github.io/cursorpie/flowhubx/" }),
        sender({ url: "https://evil.example/cursorpie/flowhub/" }),
        sender({ url: "http://sebastvnd.github.io/cursorpie/flowhub/" }),
    ];
    for (const from of refused)
        assert.equal(await call(ext, ping, from), null, JSON.stringify(from));
    assert.equal((await call(ext, ping, sender())).app, true);
    assert.equal(
        (
            await call(
                ext,
                ping,
                sender({ url: HUB + "?x=1#top", tab: { id: 1, url: HUB } }),
            )
        ).app,
        true,
    );
    assert.equal(
        ext.posted.filter((m) => m.type === "flow_ping").length,
        2,
        "refused ones sent nothing",
    );
});

test("other messages are not the relay's business", async () => {
    const ext = await extension({});
    assert.equal(
        ext.l.message({ kind: "something-else" }, sender(), () =>
            assert.fail("no answer"),
        ),
        false,
    );
    assert.equal(
        ext.l.message(null, sender(), () => assert.fail("no answer")),
        false,
    );
});

// ---------------------------------------------------------------------------------- the page side

test("the page can tell the extension is there before it asks anything", async () => {
    const ext = await extension({});
    const p = hubPage(ext);
    assert.equal(
        p.w.document.documentElement.getAttribute("data-cursorpie-bridge"),
        "9.9.9",
    );
});

test("pong carries what the page needs to choose between installing and downloading", async () => {
    const ext = await extension({
        app: (m, send) =>
            m.type === "flow_ping" &&
            send({ type: "flow_pong", req: m.req, install: true, reason: "" }),
    });
    const p = hubPage(ext);
    p.say({ type: "ping" });
    const r = await p.wait("pong");
    assert.equal(r.app, true);
    assert.equal(r.install, true);
    assert.equal(r.version, "9.9.9");

    const off = await extension({});
    const q = hubPage(off);
    q.say({ type: "ping" });
    const n = await q.wait("pong");
    assert.equal(n.app, false);
    assert.equal(n.install, false, "no app, no install");
});

test("an install made by the page comes back to the page", async () => {
    const ext = await extension({
        app: (m, send) =>
            m.type === "flow_install" &&
            send({
                type: "flow_result",
                req: m.req,
                state: "declined",
                message: "no thanks",
            }),
    });
    const p = hubPage(ext);
    p.say({ type: "install", author: "me", flow: FLOW });
    const r = await p.wait("install_result");
    assert.equal(r.state, "declined");
    assert.equal(r.message, "no thanks");
});

test("the page script ignores messages from anything but its own page and the four it knows", async () => {
    const ext = await extension({
        app: (m) =>
            m.type.startsWith("flow_") &&
            assert.fail("nothing should be asked"),
    });
    const p = hubPage(ext);
    const own = "https://sebastvnd.github.io";
    p.w.postMessage({ cursorpie: "flowhub", v: 2, id: 1, type: "ping" }, own); // another version
    p.w.postMessage({ cursorpie: "other", v: 1, id: 1, type: "ping" }, own); // not ours
    p.w.postMessage({ cursorpie: "flowhub", v: 1, type: "ping" }, own); // no id
    p.w.postMessage(
        {
            cursorpie: "flowhub",
            v: 1,
            id: 1,
            type: "command",
            cmd: "tab.close",
        },
        own,
    ); // nothing but ping and install
    p.w.postMessage("ping", own);
    // a message that comes from another window is dropped: its source is not this window
    const other = new JSDOM("", {
        url: "https://sebastvnd.github.io/cursorpie/flowhub/",
    }).window;
    p.w.dispatchEvent(
        new p.w.MessageEvent("message", {
            data: { cursorpie: "flowhub", v: 1, id: 7, type: "ping" },
            origin: own,
            source: other,
        }),
    );
    await sleep(150);
    assert.deepEqual(p.seen, []);
    assert.deepEqual(
        ext.posted.filter((m) => m.type.startsWith("flow_")),
        [],
        "nothing was relayed",
    );
});

test("a flow that is not text never leaves the page script", async () => {
    const ext = await extension({
        app: () => assert.fail("nothing should be sent"),
    });
    const p = hubPage(ext);
    p.say({ type: "install", flow: { not: "text" } });
    assert.equal((await p.wait("install_result")).state, "invalid");
    p.say({ type: "install", flow: "x".repeat(1024 * 1024 + 1) });
    assert.equal((await p.wait("install_result")).state, "invalid");
});

test("a closed extension is an offline answer, not an error on the page", async () => {
    const ext = await extension({});
    const p = hubPage(ext);
    p.w.chrome.runtime.sendMessage = () =>
        Promise.reject(new Error("Extension context invalidated."));
    p.say({ type: "ping" });
    const r = await p.wait("pong");
    assert.equal(r.app, false);
    p.say({ type: "install", flow: FLOW });
    assert.equal((await p.wait("install_result")).state, "offline");
});
