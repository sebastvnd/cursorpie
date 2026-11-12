// CursorPie Bridge: panels (see docs/PANELS.md). A panel is a small window that CursorPie shows on
// the desktop, with text, boxes, switches, sliders, lists and buttons; the answer comes back here.
// This file is the extension's side of it: `CP.ui`, which commands (cmd-ui.js) and any code of this
// extension use to ask the person something with more than a yes or a no.
//
//   const r = await CP.ui.show({ title: "Rename", widgets: [{ type: "input", id: "name", required: true }] });
//   if (r.outcome === "submitted") use(r.values.name);
//
//   r = { panel, outcome: "submitted" | "cancelled" | "timeout" | "unavailable", button, values, reason }
//
// The app only shows a panel when the person switched on "Let the browser extension show panels"
// in CursorPie's Settings (Behavior); until then every panel comes back "unavailable" with the
// reason. Nothing here touches a page and nothing leaves the computer.
"use strict";

(function (g) {
  const CP = g.CP;
  const GRACE_MS = 10000; // the app answers first; this is for an app that never will
  const MAX_SECONDS = 300; // what the app allows an extension's panel to wait
  const DEFAULT_SECONDS = 120;

  const waiting = new Map(); // req -> { resolve, timer, panel }
  let next = 1;

  const unavailable = (reason, panel) => ({
    panel: panel || "",
    outcome: "unavailable",
    button: "",
    values: {},
    reason: reason || "",
  });

  const ui = (CP.ui = {});

  // Set by background.js: send(message) -> whether it went to the app.
  ui.transport = null;

  // Shows `spec` (the format of docs/PANELS.md) and resolves with the answer. Never rejects.
  ui.show = (spec) =>
    new Promise((resolve) => {
      if (!spec || typeof spec !== "object" || Array.isArray(spec)) {
        resolve(unavailable("A panel is an object"));
        return;
      }
      const panel = typeof spec.id === "string" ? spec.id : "";
      const asked = Number.isFinite(spec.timeout_s) && spec.timeout_s > 0 ? spec.timeout_s : DEFAULT_SECONDS;
      const seconds = Math.min(asked, MAX_SECONDS);
      const req = next++;
      const timer = setTimeout(() => {
        waiting.delete(req);
        resolve(unavailable("CursorPie did not answer: is it up to date?", panel));
      }, seconds * 1000 + GRACE_MS);
      waiting.set(req, { resolve, timer, panel });
      const sent = ui.transport && ui.transport({ type: "panel", req, spec: Object.assign({}, spec, { timeout_s: seconds }) });
      if (!sent) {
        clearTimeout(timer);
        waiting.delete(req);
        resolve(unavailable("CursorPie is not running", panel));
      }
    });

  // Changes the panel `panel` that is open: { widget, text, label, value, options, subtitle }.
  ui.update = (panel, patch) => !!(ui.transport && ui.transport({ type: "panel_update", panel: String(panel), patch: patch || {} }));

  // Closes the panel `panel` (its `show` resolves "cancelled").
  ui.close = (panel) => !!(ui.transport && ui.transport({ type: "panel_close", panel: String(panel) }));

  // ---------------------------------------------------------------- the three common questions

  // true when the person pressed the confirming button.
  ui.confirm = async (message, o) => {
    o = o || {};
    const r = await ui.show({
      id: o.id,
      title: o.title || "Are you sure?",
      icon: o.icon || "browser",
      widgets: [{ type: "text", text: String(message) }],
      buttons: [
        { id: "no", label: o.noLabel || "Cancel", cancel: true },
        { id: "yes", label: o.yesLabel || "OK", tone: o.danger ? "danger" : "primary" },
      ],
    });
    return r.outcome === "submitted" && r.button === "yes";
  };

  // The text the person typed, or null.
  ui.prompt = async (label, o) => {
    o = o || {};
    const r = await ui.show({
      id: o.id,
      title: o.title || label,
      icon: o.icon || "browser",
      widgets: [{ type: "input", id: "value", label: o.title ? label : "", hint: o.hint || "", value: o.value || "", required: o.required !== false }],
      buttons: [
        { id: "cancel", label: "Cancel", cancel: true },
        { id: "ok", label: o.okLabel || "OK", tone: "primary" },
      ],
    });
    return r.outcome === "submitted" ? String(r.values.value || "") : null;
  };

  // The `value` of the option the person picked (options: strings or { value, label, hint }), or null.
  ui.pick = async (options, o) => {
    o = o || {};
    const r = await ui.show({
      id: o.id,
      title: o.title || "Pick one",
      subtitle: o.subtitle,
      icon: o.icon || "browser",
      place: o.place,
      width: o.width,
      widgets: [{ type: "choice", id: "pick", options, submit: true, search: o.search }],
    });
    return r.outcome === "submitted" ? String(r.values.pick) : null;
  };

  // ---------------------------------------------------------------- what the app says

  // background.js calls this with a { type: "panel_result", req, result } message.
  ui.handle = (m) => {
    const w = waiting.get(m && m.req);
    if (!w) return;
    waiting.delete(m.req);
    clearTimeout(w.timer);
    const r = m.result && typeof m.result === "object" ? m.result : {};
    w.resolve({
      panel: typeof r.panel === "string" ? r.panel : w.panel,
      outcome: typeof r.outcome === "string" ? r.outcome : "unavailable",
      button: typeof r.button === "string" ? r.button : "",
      values: r.values && typeof r.values === "object" ? r.values : {},
      reason: typeof r.reason === "string" ? r.reason : "",
    });
  };

  // background.js calls this when the connection to the app closes: nobody is going to answer.
  ui.disconnected = () => {
    for (const [req, w] of Array.from(waiting)) {
      waiting.delete(req);
      clearTimeout(w.timer);
      w.resolve(unavailable("CursorPie closed the connection", w.panel));
    }
  };
})(typeof self !== "undefined" ? self : globalThis);
