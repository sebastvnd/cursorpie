// CursorPie Bridge: the options page (options.html). Reads and writes `settings` in the extension's
// local storage; the background script picks changes up by itself (storage.onChanged).
"use strict";

(function () {
  const CP = self.CP;
  const api = CP.api;
  const $ = (id) => document.getElementById(id);
  const BOOLS = ["media", "tabs", "chat", "ui", "allowSend", "allowRead", "skipMuted", "autoPauseOthers"];
  const NUMS = ["seekSeconds", "volumeStep", "speedStep"];

  async function load() {
    const r = await api.storage.local.get("settings");
    const s = Object.assign({}, CP.DEFAULTS, (r && r.settings) || {});
    BOOLS.forEach((k) => ($(k).checked = !!s[k]));
    NUMS.forEach((k) => ($(k).value = s[k]));
    $("notify").value = s.notify;
    $("selectors").value = Object.keys(s.selectors || {}).length ? JSON.stringify(s.selectors, null, 2) : "";
    $("templates").value = Object.keys(s.templates || {}).length ? JSON.stringify(s.templates, null, 2) : "";
  }

  // The saved replies as written: an object of name -> text. A name that chat.template cannot use is
  // refused here, so nobody saves a reply that then never answers.
  function readTemplates() {
    const text = $("templates").value.trim();
    if (!text) return {};
    const t = JSON.parse(text);
    if (!t || typeof t !== "object" || Array.isArray(t)) throw new Error("must be an object like {\"omw\": \"On my way\"}");
    for (const [k, v] of Object.entries(t)) {
      if (!/^[A-Za-z0-9][A-Za-z0-9_-]{0,31}$/.test(k)) throw new Error("the name '" + k + "' can only have letters, digits, - and _ (up to 32)");
      if (typeof v !== "string" || !v.trim()) throw new Error("the reply '" + k + "' must be a text");
      if (v.length > 2000) throw new Error("the reply '" + k + "' is longer than 2000 characters");
    }
    return t;
  }

  async function save() {
    let selectors = {};
    const text = $("selectors").value.trim();
    $("bad").textContent = "";
    if (text) {
      try {
        selectors = JSON.parse(text);
        if (!selectors || typeof selectors !== "object" || Array.isArray(selectors)) throw new Error("must be an object");
      } catch (e) {
        $("bad").textContent = "The selector overrides are not valid JSON: " + e.message;
        return;
      }
    }
    let templates = {};
    $("badTemplates").textContent = "";
    try {
      templates = readTemplates();
    } catch (e) {
      $("badTemplates").textContent = "The saved replies are not valid: " + e.message;
      return;
    }
    const s = { selectors, templates, notify: $("notify").value };
    BOOLS.forEach((k) => (s[k] = $(k).checked));
    NUMS.forEach((k) => {
      const n = parseFloat($(k).value);
      s[k] = Number.isFinite(n) && n > 0 ? n : CP.DEFAULTS[k];
    });
    await api.storage.local.set({ settings: s });
    $("saved").textContent = "Saved";
    setTimeout(() => ($("saved").textContent = ""), 2000);
  }

  async function showPermission() {
    let has = false;
    try {
      has = await api.permissions.contains({ origins: ["<all_urls>"] });
    } catch (e) {
      has = false;
    }
    $("permState").textContent = has ? "Allowed on all sites" : "Only the player sites, WhatsApp Web and Telegram Web";
    $("perm").textContent = has ? "Take it back" : "Allow all sites";
    $("perm").onclick = async () => {
      try {
        if (has) await api.permissions.remove({ origins: ["<all_urls>"] });
        else await api.permissions.request({ origins: ["<all_urls>"] });
      } catch (e) {
        $("bad").textContent = String((e && e.message) || e);
      }
      showPermission();
    };
  }

  $("save").onclick = save;
  load();
  showPermission();
})();
