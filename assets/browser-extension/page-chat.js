// CursorPie Bridge: the code that runs INSIDE WhatsApp Web and Telegram Web (web.telegram.org,
// both the "K" and the "A" version).
//
// `CP.pageChat(action, opts)` is sent to the tab as text (see CP.runInTab), so it must not use
// anything outside its own body. It is synchronous: a command that has to wait for the page (type a
// name in the search, wait for the results, click one) is several calls with a pause in between,
// made by cmd-chat.js.
//
// These sites have no public API and their markup is minified and changes without notice, so
// every part of the page the code needs is a LIST of CSS selectors, tried in order, in the
// table below. When a site changes, the person can put a working selector first in the options page
// ("Selector overrides") without waiting for a new version.
//
// What it reads on its own: the number of unread chats and the name in the header of the open chat
// (to check that a message goes to the right person). The text of messages and the names of the
// chats that wait are read only by the actions "messages" and "unread_names", which cmd-chat.js
// calls only when the person turned on "Allow reading messages" in the options.
"use strict";

(function (g) {
  const CP = (g.CP = g.CP || {});

  // Exposed so the options page can show them as the starting point of an override.
  CP.CHAT_SELECTORS = {
    whatsapp: {
      rows: [
        "#pane-side [role='listitem']",
        "#pane-side [role='row']",
        "#pane-side [data-testid='cell-frame-container']",
      ],
      selected: ["[aria-selected='true']"],
      unread: [
        "[data-testid='icon-unread-count']",
        "span[data-icon='unread-count']",
        "span[aria-label*='unread' i]",
      ],
      search: [
        "div[contenteditable='true'][data-tab='3']",
        "[data-testid='chat-list-search']",
        "#side div[contenteditable='true']",
      ],
      composer: [
        "footer div[contenteditable='true'][data-tab='10']",
        "#main footer div[contenteditable='true']",
        "footer [contenteditable='true']",
      ],
      send: [
        "footer span[data-icon='send']",
        "[data-testid='compose-btn-send']",
        "footer button[aria-label='Send']",
      ],
      header: [
        "#main header [data-testid='conversation-info-header-chat-title']",
        "#main header span[title]",
        "#main header span[dir='auto']",
      ],
      scroll: ["#main [data-testid='conversation-panel-messages']", "#main .copyable-area [tabindex='0']"],
      // one message of the open conversation, the text inside it, and what says who wrote it
      messages: [
        "#main .message-in, #main .message-out",
        "#main [data-testid='msg-container']",
        "#main div[data-id]",
      ],
      text: [
        "span.selectable-text.copyable-text",
        "[data-testid='selectable-text']",
        "span.selectable-text",
        ".copyable-text",
      ],
      incoming: [".message-in", "[data-id^='false_']"],
      outgoing: [".message-out", "[data-id^='true_']"],
      // a chat row whose notifications are off, a row with an @mention, the folder tabs (WhatsApp has none)
      muted: ["[data-icon='muted']", "span[data-icon*='mute']", "[data-testid='muted']"],
      mention: [],
      folders: [],
    },
    telegram: {
      rows: [
        "a.chatlist-chat",
        ".chatlist-chat",
        ".chat-list .ListItem",
        "#LeftColumn .chat-list .Chat",
      ],
      selected: [".active", ".selected", "[aria-selected='true']"],
      unread: [
        ".dialog-subtitle-badge.is-visible",
        ".dialog-subtitle-badge.unread",
        ".ChatBadge.unread",
        ".ChatBadge:not(.muted):not(.pinned)",
      ],
      search: [
        "input.input-search-input",
        "#telegram-search-input",
        ".sidebar-header input[type='text']",
        "#LeftColumn input[type='text']",
      ],
      composer: [
        ".chat-input .input-message-input[contenteditable='true']",
        ".input-message-input[contenteditable='true']",
        "#editable-message-text",
        ".Composer [contenteditable='true']",
      ],
      send: [
        ".chat-input .btn-send",
        ".btn-send",
        ".Composer button.send",
        ".Composer .main-button",
      ],
      header: [
        ".chat-info .peer-title",
        ".topbar .peer-title",
        ".MiddleHeader .ChatInfo .fullName",
        ".MiddleHeader .ChatInfo .title",
      ],
      scroll: [".bubbles .scrollable", ".bubbles", ".MessageList"],
      messages: [
        ".bubbles .bubble[data-mid]",
        ".bubbles .bubble",
        ".MessageList .Message",
        ".messages-container .Message",
      ],
      text: [".message", ".text-content", ".message-text"],
      incoming: [".is-in", ".Message:not(.own)"],
      outgoing: [".is-out", ".Message.own"],
      muted: [".is-muted", ".badge-gray", ".ChatBadge.muted", ".dialog-subtitle-badge.muted"],
      mention: [
        ".dialog-subtitle-badge.mention",
        ".dialog-subtitle-badge-mention",
        ".badge.mention",
        ".ChatBadge.mention",
        ".ChatBadge .icon-mention",
      ],
      folders: [
        ".folders-tabs .menu-horizontal-div-item",
        ".menu-horizontal-div-item",
        ".ChatFolders .Tab",
        ".TabList .Tab",
        "#LeftColumn .Tab",
      ],
    },
  };

  CP.pageChat = function pageChat(action, opts, tables) {
    opts = opts || {};
    const host = location.hostname.toLowerCase();
    const site = host === "web.whatsapp.com" ? "whatsapp" : host === "web.telegram.org" ? "telegram" : "";
    if (!site) return { ok: false, msg: "Not a chat page", site: "" };

    // ------------------------------------------------------------ selectors
    // `tables` is CP.CHAT_SELECTORS with the person's overrides put in front of every list.
    const S = (tables && tables[site]) || {};
    const q1 = (s) => {
      try {
        return document.querySelector(s);
      } catch (e) {
        return null;
      }
    };
    const shown = (el) => !!el && (el.getClientRects().length > 0 || el.offsetWidth > 0);
    const first = (list, needShown) => {
      for (const s of list || []) {
        const el = q1(s);
        if (el && (!needShown || shown(el))) return el;
      }
      return null;
    };
    // the elements of the first selector that finds any visible ones
    const all = (list) => {
      for (const s of list || []) {
        let found = [];
        try {
          found = Array.from(document.querySelectorAll(s)).filter(shown);
        } catch (e) {
          found = [];
        }
        if (found.length) return found;
      }
      return [];
    };

    // ------------------------------------------------------------ small tools
    const keyEvent = (el, key, code, keyCode, extra) => {
      for (const type of ["keydown", "keypress", "keyup"]) {
        el.dispatchEvent(
          new KeyboardEvent(type, Object.assign({ key, code, keyCode, which: keyCode, bubbles: true, cancelable: true }, extra || {}))
        );
      }
    };
    // a click the way a person's mouse makes it, because these apps listen to more than "click"
    const click = (el) => {
      for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "click"]) {
        const Ctor = type.startsWith("pointer") && typeof PointerEvent === "function" ? PointerEvent : MouseEvent;
        el.dispatchEvent(new Ctor(type, { bubbles: true, cancelable: true, view: window, button: 0 }));
      }
    };
    const insert = (text) => {
      const parts = String(text).split("\n");
      let okAll = true;
      parts.forEach((part, i) => {
        if (i > 0) {
          try {
            document.execCommand("insertLineBreak");
          } catch (e) {
            okAll = false;
          }
        }
        if (part) {
          try {
            okAll = document.execCommand("insertText", false, part) && okAll;
          } catch (e) {
            okAll = false;
          }
        }
      });
      return okAll;
    };
    // Puts text in an input or a contenteditable box. `replace` false adds to the end, so a
    // half-written message is never lost.
    const setText = (el, text, replace) => {
      el.focus();
      if (el.tagName === "INPUT" || el.tagName === "TEXTAREA") {
        const proto = el.tagName === "INPUT" ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
        const set = Object.getOwnPropertyDescriptor(proto, "value").set;
        set.call(el, replace ? text : el.value + text);
        el.dispatchEvent(new Event("input", { bubbles: true }));
        return true;
      }
      const sel = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(el);
      if (!replace) range.collapse(false);
      sel.removeAllRanges();
      sel.addRange(range);
      if (insert(text)) return true;
      // the editor refused execCommand: offer the text as a paste
      try {
        const dt = new DataTransfer();
        dt.setData("text/plain", text);
        return el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      } catch (e) {
        return false;
      }
    };
    const done = (ok, msg, extra) => Object.assign({ ok, msg: msg || "", site }, extra || {});

    // ------------------------------------------------------------ the chat list
    const rowsNow = () => all(S.rows);
    const unreadOf = (row) => {
      for (const s of S.unread || []) {
        let b = null;
        try {
          b = row.querySelector(s);
        } catch (e) {
          b = null;
        }
        if (b) {
          const n = parseInt((b.textContent || "").replace(/\D/g, ""), 10);
          return Number.isFinite(n) && n > 0 ? n : 1;
        }
      }
      if (site === "whatsapp") {
        // The label of the badge depends on the language of the page, so as a second try look for
        // what it looks like: a short number, not in a name or a preview (those have a title), on a
        // coloured background.
        for (const sp of row.querySelectorAll("span")) {
          if (sp.children.length || sp.hasAttribute("title")) continue;
          const t = (sp.textContent || "").trim();
          if (!/^\d{1,4}\+?$/.test(t)) continue;
          let el = sp;
          for (let i = 0; i < 3 && el && el !== row; i++, el = el.parentElement) {
            const bg = getComputedStyle(el).backgroundColor;
            if (bg && bg !== "transparent" && !/^rgba\(\s*0,\s*0,\s*0,\s*0\s*\)$/.test(bg)) return parseInt(t, 10);
          }
        }
      }
      return 0;
    };
    const isSelected = (row) =>
      (S.selected || []).some((s) => {
        try {
          return row.matches(s) || !!row.querySelector(s);
        } catch (e) {
          return false;
        }
      });
    // Whether a row is, or holds, an element that one of the selectors finds.
    const has = (list, row) =>
      (list || []).some((sel) => {
        try {
          return row.matches(sel) || !!row.querySelector(sel);
        } catch (e) {
          return false;
        }
      });
    // The unread number of a row; 0 for a muted chat when the command says to skip those.
    const countOf = (row) => (opts.skipMuted && has(S.muted, row) ? 0 : unreadOf(row));
    // The next (dir 1) or previous (dir -1) of the row indexes after the open chat, going round the list.
    const cycle = (rows, indexes, dir) => {
      const cur = rows.findIndex(isSelected);
      let pick = dir > 0 ? indexes.find((i) => i > cur) : indexes.slice().reverse().find((i) => i < cur);
      if (pick === undefined) pick = dir > 0 ? indexes[0] : indexes[indexes.length - 1];
      return pick;
    };
    const headerName = () => {
      const el = first(S.header, true);
      return el ? (el.getAttribute("title") || el.textContent || "").trim() : "";
    };


    // ------------------------------------------------------------ names and messages
    // The name of a row of the chat list, as the page shows it.
    const rowName = (row) => {
      for (const sel of [".peer-title", ".fullName", "[title]"]) {
        let el = null;
        try {
          el = row.querySelector(sel);
        } catch (e) {
          el = null;
        }
        const t = el && (el.getAttribute("title") || el.textContent || "").trim();
        if (t) return t.slice(0, 200);
      }
      return ((row.textContent || "").trim().split("\n")[0] || "").slice(0, 200);
    };
    // Whether `el` is, is inside, or holds an element that one of the selectors finds.
    const marked = (list, el) =>
      (list || []).some((sel) => {
        try {
          return el.matches(sel) || !!el.closest(sel) || !!el.querySelector(sel);
        } catch (e) {
          return false;
        }
      });
    // "out" for a message the person wrote, "in" for one that came, "" when the page does not say.
    const dirOf = (el) => (marked(S.outgoing, el) ? "out" : marked(S.incoming, el) ? "in" : "");
    // What a page keeps in a message that is not text, or in the corner of the text (the time).
    const NOT_TEXT =
      ".time, .MessageMeta, .message-time, .reactions, .Reactions, [data-testid='msg-meta'], [data-testid='quoted-message'], .reply";
    const MEDIA =
      "img[src^='blob:'], img[src^='data:'], video, audio, canvas, [data-icon*='audio'], [data-icon*='ptt'], [data-icon*='document'], .media-container, .document, .audio, .attachment";
    // The text of a node as a person reads it: emoji are pictures with the emoji in `alt`.
    const plain = (node) => {
      let out = "";
      const walk = (n) => {
        for (const c of n.childNodes) {
          if (c.nodeType === 3) {
            out += c.data;
          } else if (c.nodeType === 1) {
            let skip = false;
            try {
              skip = c.matches(NOT_TEXT);
            } catch (e) {
              skip = false;
            }
            if (skip) continue;
            if (c.tagName === "IMG") out += c.getAttribute("alt") || "";
            else if (c.tagName === "BR") out += "\n";
            else walk(c);
          }
        }
      };
      walk(node);
      return out
        .replace(/[\u200e\u200f\u202a-\u202e]/g, "")
        .replace(/[ \t\u00a0]+/g, " ")
        .replace(/ ?\n ?/g, "\n")
        .trim();
    };
    const inQuote = (el) => {
      try {
        return !!el.closest("[data-testid='quoted-message'], .reply, .quote");
      } catch (e) {
        return false;
      }
    };
    // The words of one message, "[media]" for one that is only a picture, a voice note or a file,
    // "" for a line that is not a message (a date, a notice).
    const textOfMessage = (el) => {
      let parts = [];
      for (const sel of S.text || []) {
        let list = [];
        try {
          list = Array.from(el.querySelectorAll(sel));
        } catch (e) {
          list = [];
        }
        // the page may mark a text and the box around it: keep the outer one once
        list = list.filter((x) => !inQuote(x) && !list.some((y) => y !== x && y.contains(x)));
        if (list.length) {
          parts = list.map(plain).filter(Boolean);
          break;
        }
      }
      const text = parts.join("\n").slice(0, 2000);
      if (text) return text;
      try {
        return el.querySelector(MEDIA) ? "[media]" : "";
      } catch (e) {
        return "";
      }
    };
    // What tells one message from another: the page's own id when it has one.
    const keyOf = (el) => {
      const ATTRS = ["data-id", "data-mid", "data-message-id"];
      let n = el;
      for (let i = 0; n && i < 3; i++, n = n.parentElement) {
        for (const a of ATTRS) {
          const v = n.getAttribute && n.getAttribute(a);
          if (v) return v;
        }
      }
      let d = null;
      try {
        d = el.querySelector("[data-id],[data-mid],[data-message-id]");
      } catch (e) {
        d = null;
      }
      if (d) return ATTRS.map((a) => d.getAttribute(a)).find(Boolean) || "";
      return /^message/i.test(el.id || "") ? el.id : "";
    };

    // ------------------------------------------------------------ folders
    // The name of a folder tab without the little number of unread chats that sits in it.
    const folderLabel = (el) => {
      const c = el.cloneNode(true);
      try {
        c.querySelectorAll("[class*='badge' i], [class*='Badge']").forEach((b) => b.remove());
      } catch (e) {
        /* keep the text as it is */
      }
      return (c.textContent || "").replace(/\s+/g, " ").trim().slice(0, 100);
    };
    const folderActive = (el) => {
      try {
        return el.matches(".active, .is-active, .Tab--active, [aria-selected='true']");
      } catch (e) {
        return false;
      }
    };

    // ------------------------------------------------------------ actions
    switch (action) {
      case "info": {
        const rows = rowsNow();
        const counts = rows.map(countOf);
        return done(true, "", {
          ready: rows.length > 0,
          chats: rows.length,
          unreadChats: counts.filter((n) => n > 0).length,
          unreadMessages: counts.reduce((a, b) => a + b, 0),
          composer: !!first(S.composer, true),
        });
      }

      case "header":
        return done(true, "", { name: headerName() });

      case "open_unread": {
        const rows = rowsNow();
        if (!rows.length) return done(false, "Chat list not found (is the page loaded, and logged in?)");
        const dir = opts.dir < 0 ? -1 : 1;
        const unread = rows.map((r, i) => (countOf(r) > 0 ? i : -1)).filter((i) => i >= 0);
        if (!unread.length) return done(false, "No unread chats", { found: false });
        // the next unread chat after the open one, going round the list
        click(rows[cycle(rows, unread, dir)]);
        return done(true, "Opened the next unread chat", { found: true, unreadChats: unread.length });
      }

      // The next chat with an @mention (a muted chat counts: a mention reaches you there too).
      case "open_mention": {
        const rows = rowsNow();
        if (!rows.length) return done(false, "Chat list not found (is the page loaded, and logged in?)");
        const hits = rows.map((r, i) => (has(S.mention, r) ? i : -1)).filter((i) => i >= 0);
        if (!hits.length) return done(false, "No chat with a mention", { found: false });
        click(rows[cycle(rows, hits, opts.dir < 0 ? -1 : 1)]);
        return done(true, "Opened the next chat with a mention", { found: true, mentions: hits.length });
      }

      // The folder tabs of Telegram (All, Personal, Work...): their names and which one is open.
      case "folders": {
        const tabs = all(S.folders);
        if (!tabs.length) return done(false, "No folders found (Telegram shows them only when you have made some)", { found: false });
        return done(true, "", { folders: tabs.map((t) => ({ name: folderLabel(t), active: folderActive(t) })) });
      }

      // Goes to a folder by its name (the exact name first, then part of it) or by its number.
      case "folder": {
        const want = String(opts.name || "").trim().toLowerCase();
        if (!want) return done(false, "Say which folder");
        const tabs = all(S.folders);
        if (!tabs.length) return done(false, "No folders found (Telegram shows them only when you have made some)", { found: false });
        const labels = tabs.map(folderLabel);
        // the exact name, then (for a short number) the place in the row, then part of a name: so "4"
        // is the 4th folder and not the one called "2024"
        let i = labels.findIndex((l) => l.toLowerCase() === want);
        if (i < 0 && /^\d{1,2}$/.test(want)) {
          const n = parseInt(want, 10);
          if (n >= 1 && n <= tabs.length) i = n - 1;
        }
        if (i < 0 && !/^\d{1,2}$/.test(want)) i = labels.findIndex((l) => l.toLowerCase().includes(want));
        if (i < 0) return done(false, "No folder named '" + opts.name + "'", { found: false });
        click(tabs[i]);
        return done(true, "", { found: true, name: labels[i] });
      }

      // Opens a Telegram chat by @username, which does not depend on the language of the page.
      case "goto_user": {
        const u = String(opts.user || "").trim();
        if (site !== "telegram" || !/^@[A-Za-z][A-Za-z0-9_]{3,31}$/.test(u)) return done(false, "Not a Telegram @username");
        location.hash = "#" + u;
        return done(true);
      }

      case "open_adjacent": {
        const rows = rowsNow();
        if (!rows.length) return done(false, "Chat list not found (is the page loaded, and logged in?)");
        const cur = rows.findIndex(isSelected);
        const dir = opts.dir < 0 ? -1 : 1;
        const to = cur < 0 ? (dir > 0 ? 0 : rows.length - 1) : Math.min(rows.length - 1, Math.max(0, cur + dir));
        click(rows[to]);
        return done(true, "", { index: to });
      }

      case "search": {
        const box = first(S.search, true) || first(S.search, false);
        if (!box) return done(false, "Search box not found");
        return setText(box, String(opts.text || ""), true) ? done(true) : done(false, "Could not type in the search box");
      }

      case "pick": {
        const rows = rowsNow();
        if (!rows.length) return done(false, "No chat found", { found: false });
        const want = String(opts.name || "").trim().toLowerCase();
        const nameOf = (r) => {
          const t = r.querySelector("[title]");
          return ((t && t.getAttribute("title")) || r.textContent || "").trim().toLowerCase();
        };
        const exact = rows.find((r) => nameOf(r) === want);
        const part = rows.find((r) => (r.textContent || "").toLowerCase().includes(want));
        const row = exact || part || null;
        if (!row) return done(false, "No chat named '" + opts.name + "'", { found: false });
        click(row);
        return done(true, "", { found: true });
      }

      case "clear_search": {
        const box = first(S.search, false);
        if (box) {
          keyEvent(box, "Escape", "Escape", 27);
          if (box.tagName === "INPUT") {
            setText(box, "", true);
          }
        }
        return done(true);
      }

      case "draft": {
        const box = first(S.composer, true);
        if (!box) return done(false, "No chat is open (message box not found)");
        return setText(box, String(opts.text || ""), false)
          ? done(true, "Message typed, not sent")
          : done(false, "Could not type in the message box");
      }

      case "send": {
        const box = first(S.composer, true);
        if (!box) return done(false, "No chat is open (message box not found)");
        const mark = first(S.send, true);
        const button = mark && (mark.closest("button,[role='button']") || mark);
        if (button) {
          click(button);
          return done(true, "Sent");
        }
        box.focus();
        keyEvent(box, "Enter", "Enter", 13);
        return done(true, "Sent");
      }

      case "escape": {
        const el = document.activeElement || document.body;
        keyEvent(el, "Escape", "Escape", 27);
        return done(true);
      }

      case "bottom": {
        let best = null;
        for (const s of S.scroll || []) {
          let list = [];
          try {
            list = Array.from(document.querySelectorAll(s));
          } catch (e) {
            list = [];
          }
          for (const el of [].concat(list, list.flatMap((l) => Array.from(l.querySelectorAll("*"))))) {
            if (el.scrollHeight > el.clientHeight + 50 && (!best || el.clientHeight > best.clientHeight)) best = el;
          }
          if (best) break;
        }
        if (!best) return done(false, "No conversation to scroll");
        best.scrollTop = best.scrollHeight;
        return done(true);
      }

      case "unread_names": {
        const rows = rowsNow();
        if (!rows.length) return done(false, "Chat list not found (is the page loaded, and logged in?)");
        const names = [];
        for (const r of rows) {
          const n = countOf(r);
          if (n > 0) names.push({ name: rowName(r), unread: n });
        }
        return done(true, "", { names });
      }

      // The last messages of the open chat, oldest first: { dir: "in" | "out", text, key }. Only
      // lines the page marks as incoming or outgoing count, so a date or a notice is left out.
      case "messages": {
        const nodes = all(S.messages);
        if (!nodes.length) return done(false, "No messages found (is a chat open?)", { found: false });
        const want = Math.max(1, Math.min(60, parseInt(opts.n, 10) || 10));
        const list = [];
        let unknown = 0;
        for (const el of nodes.slice(-120)) {
          const d = dirOf(el);
          if (!d) {
            unknown++;
            continue;
          }
          const text = textOfMessage(el);
          if (text) list.push({ dir: d, text, key: keyOf(el) });
        }
        if (!list.length && unknown) {
          return done(false, "Could not tell sent messages from received ones: the page changed (see Selector overrides)", {
            found: false,
          });
        }
        return done(true, "", { messages: list.slice(-want), name: headerName() });
      }

      default:
        return done(false, "Unknown chat action: " + action);
    }
  };
})(typeof self !== "undefined" ? self : globalThis);
