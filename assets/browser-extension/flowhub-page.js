// CursorPie Bridge: the part that runs on the Flow Hub page of the CursorPie website (the manifests
// list it under `content_scripts`, for that address only). See docs/BROWSER_EXTENSION.md, "Flow Hub".
//
// The page cannot reach the extension by itself, and must not be able to reach anything else of it:
// this script is the only door, and it opens for four messages only. The page and this script talk
// with `window.postMessage`; this script and the extension with `runtime.sendMessage`.
//
//   page -> here   { cursorpie: "flowhub", v: 1, id, type: "ping" }
//                  { cursorpie: "flowhub", v: 1, id, type: "install", author, flow }   `flow` is the file's text
//   here -> page   { cursorpie: "bridge",  v: 1, id, type: "pong", version, app, install, reason }
//                  { cursorpie: "bridge",  v: 1, id, type: "install_result", state, message }
//
// What the page learns from `pong`: that the extension is here (it got an answer at all), whether the
// CursorPie app is running (`app`) and whether it would take a flow now (`install`). The page decides
// between installing and a plain download from that. Nothing of the person's is sent to the page.
//
// A page cannot make this script do more than ask: the app shows the flow to the person and adds it
// only after they press *Install* in CursorPie.
"use strict";

(function () {
    const api = typeof browser !== "undefined" ? browser : chrome;
    const VERSION = 1;
    // the most text of a flow file (the app refuses a bigger one too)
    const MAX_FLOW = 1024 * 1024;

    function reply(message) {
        window.postMessage(
            Object.assign({ cursorpie: "bridge", v: VERSION }, message),
            location.origin,
        );
    }

    // Asks the extension's background; a closed or reloading extension answers with an error, which
    // is "not available" for the page, not a crash.
    function ask(message) {
        try {
            return Promise.resolve(api.runtime.sendMessage(message)).catch(
                () => null,
            );
        } catch (e) {
            return Promise.resolve(null);
        }
    }

    async function onPing(m) {
        const r = await ask({ kind: "flowhub_ping" });
        reply({
            id: m.id,
            type: "pong",
            version: api.runtime.getManifest().version,
            app: !!(r && r.app),
            install: !!(r && r.app && r.install),
            reason: String((r && r.reason) || "").slice(0, 300),
        });
    }

    async function onInstall(m) {
        const bad = (message) =>
            reply({
                id: m.id,
                type: "install_result",
                state: "invalid",
                message,
            });
        if (typeof m.flow !== "string" || !m.flow)
            return bad("There is no flow to install.");
        if (m.flow.length > MAX_FLOW)
            return bad("That flow file is too large.");
        const r = await ask({
            kind: "flowhub_install",
            author: typeof m.author === "string" ? m.author.slice(0, 120) : "",
            flow: m.flow,
        });
        reply({
            id: m.id,
            type: "install_result",
            state: String((r && r.state) || "offline"),
            message: String(
                (r && r.message) || "CursorPie did not answer.",
            ).slice(0, 600),
        });
    }

    window.addEventListener("message", (event) => {
        // only this page, from its own address: not a frame inside it, not another window
        if (event.source !== window || event.origin !== location.origin) return;
        const m = event.data;
        if (
            !m ||
            typeof m !== "object" ||
            m.cursorpie !== "flowhub" ||
            m.v !== VERSION
        )
            return;
        if (typeof m.id !== "number" && typeof m.id !== "string") return;
        if (m.type === "ping") onPing(m);
        else if (m.type === "install") onInstall(m);
    });

    // A page that loads after this script (or one that never pings) can still tell the extension is
    // here: the attribute is a flag, not a promise that the app runs.
    document.documentElement.setAttribute(
        "data-cursorpie-bridge",
        api.runtime.getManifest().version,
    );
    window.dispatchEvent(new CustomEvent("cursorpie-bridge-ready"));
})();
