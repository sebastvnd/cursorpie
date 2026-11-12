// CursorPie Bridge: the chat.* commands for WhatsApp Web and Telegram Web (see
// docs/BROWSER_EXTENSION.md).
//
// Safety rules, because a message cannot be taken back and a chat is private:
//   * nothing is sent unless "Allow sending messages" is on in the options (off by default); with it
//     off, chat.send and chat.send_to only type the message and leave it for the person to send;
//   * chat.send_to opens the chat by name and sends only after the name in the header of the open
//     chat has been checked to match; when it cannot be checked or does not match, nothing is sent;
//   * the text of messages, the name of the open chat, the names of the chats that wait and the names
//     of the Telegram folders are read only by chat.last, chat.read, chat.wait, chat.who,
//     chat.unread_names and chat.folders, and only when "Allow reading messages" is on in the options
//     (off by default). Everything else reads numbers only;
//   * chat.template sends like chat.send: only with "Allow sending messages" on, else it types it.
"use strict";

(function (g) {
  const CP = g.CP;
  const api = CP.api;
  const reg = CP.reg;

  const SITES = {
    whatsapp: { label: "WhatsApp", host: "web.whatsapp.com", home: "https://web.whatsapp.com/" },
    telegram: { label: "Telegram", host: "web.telegram.org", home: "https://web.telegram.org/k/" },
  };
  CP.CHAT_SITES = SITES;

  // `@telegram text` -> { site: "telegram", rest: "text" }. Without the prefix the site is not named.
  const split = (arg) => {
    const m = /^@(whatsapp|telegram)\b\s*([\s\S]*)$/i.exec(arg || "");
    return m ? { site: m[1].toLowerCase(), rest: m[2].trim() } : { site: "", rest: (arg || "").trim() };
  };

  // The selector tables with the person's overrides put in front.
  const tables = (settings) => {
    const out = {};
    for (const site of Object.keys(CP.CHAT_SELECTORS)) {
      out[site] = {};
      const over = (settings.selectors && settings.selectors[site]) || {};
      for (const key of Object.keys(CP.CHAT_SELECTORS[site])) {
        const extra = [].concat(over[key] || []).filter((s) => typeof s === "string" && s);
        out[site][key] = extra.concat(CP.CHAT_SELECTORS[site][key]);
      }
    }
    return out;
  };

  // The unread number a page puts in its title: "(3) WhatsApp".
  CP.titleCount = (title) => {
    const m = /^\s*\((\d+)\+?\)/.exec(title || "");
    return m ? parseInt(m[1], 10) : null;
  };

  const siteOf = (url) => {
    for (const [name, s] of Object.entries(SITES)) if (CP.hostIs(url, s.host)) return name;
    return "";
  };
  CP.chatSiteOf = siteOf;

  // The chat tab to act on: the one you are looking at, then the one you used last, then any.
  const chatTab = async (want) => {
    const tabs = (await api.tabs.query({})).filter((t) => {
      const s = siteOf(t.url);
      return s && (!want || s === want);
    });
    if (!tabs.length) return null;
    // the tab you are looking at, then the one you used most recently, then WhatsApp before Telegram
    const rank = (t) => (t.active ? 1000 : 0) + (CP.mem.mru[t.windowId] || []).indexOf(t.id);
    tabs.sort((a, b) => rank(b) - rank(a) || siteOf(a.url).localeCompare(siteOf(b.url)) * -1);
    return tabs[0];
  };

  // Whether next_unread, unread_names and the counts leave out the chats whose notifications are off.
  const skipMuted = (settings) => settings.skipMuted !== false;

  const chat = (tab, action, opts, settings) => CP.runInTab(tab.id, CP.pageChat, [action, opts || {}, tables(settings)]);

  // Runs `fn(tab, site)` on the chat tab and brings it to the front first (a page in the
  // background is slow to react, and the person sees where the command went).
  // `options.only` ("telegram") is for a command that exists on one site only.
  const onChat = (fn, options) => async ({ arg, settings }) => {
    const given = split(arg);
    const only = options && options.only;
    if (only && given.site && given.site !== only) return CP.fail("This command works on " + SITES[only].label + " Web only");
    const want = only || given.site;
    const rest = given.rest;
    const tab = await chatTab(want);
    if (!tab) {
      const s = SITES[want || "whatsapp"];
      if (options && options.open) {
        await api.tabs.create({ url: s.home });
        return CP.ok("Opening " + s.label + " Web");
      }
      return CP.fail((want ? s.label : "WhatsApp Web or Telegram Web") + " is not open in the browser");
    }
    if (!(options && options.quiet)) await CP.show(tab);
    try {
      return await fn(tab, rest, settings);
    } catch (e) {
      return CP.fail(CP.friendly(e, tab));
    }
  };

  const toResult = (r, okMessage) => (r && r.ok ? CP.ok(okMessage || r.msg) : CP.fail((r && r.msg) || "The page did not answer"));

  reg("chat.focus", "chat", onChat(async () => CP.ok(), { open: true }));

  reg("chat.next_unread", "chat", onChat(async (tab, rest, st) => {
    const r = await chat(tab, "open_unread", { dir: 1, skipMuted: skipMuted(st) }, st);
    return r && r.found === false ? CP.ok(r.msg, true) : toResult(r);
  }));
  reg("chat.prev_unread", "chat", onChat(async (tab, rest, st) => {
    const r = await chat(tab, "open_unread", { dir: -1, skipMuted: skipMuted(st) }, st);
    return r && r.found === false ? CP.ok(r.msg, true) : toResult(r);
  }));
  reg("chat.next", "chat", onChat(async (tab, rest, st) => toResult(await chat(tab, "open_adjacent", { dir: 1 }, st))));
  reg("chat.prev", "chat", onChat(async (tab, rest, st) => toResult(await chat(tab, "open_adjacent", { dir: -1 }, st))));
  reg("chat.close", "chat", onChat(async (tab, rest, st) => toResult(await chat(tab, "escape", {}, st))));
  reg("chat.bottom", "chat", onChat(async (tab, rest, st) => toResult(await chat(tab, "bottom", {}, st))));

  // How many chats wait for you, on every open chat tab. Quiet: it does not bring a tab forward.
  reg("chat.status", "chat", async ({ settings }) => {
    const parts = [];
    for (const name of Object.keys(SITES)) {
      const tab = await chatTab(name);
      if (!tab) continue;
      let n = CP.titleCount(tab.title);
      if (n === null) {
        try {
          const r = await chat(tab, "info", { skipMuted: skipMuted(settings) }, settings);
          n = r && r.ready ? r.unreadChats : null;
        } catch (e) {
          n = null;
        }
      }
      parts.push(SITES[name].label + ": " + (n === null ? "unknown" : n === 0 ? "nothing new" : n + " unread"));
    }
    return parts.length ? CP.ok(parts.join(" · "), true) : CP.ok("No chat tab is open", true);
  });

  reg("chat.search", "chat", onChat(async (tab, text, st) => {
    if (!text) return CP.fail("Say what to search for, for example: chat.search Anna");
    return toResult(await chat(tab, "search", { text }, st));
  }));

  // Opens a chat by typing its name in the search box and choosing the match. Returns the result
  // of the last page call, and `headerOk`: whether the open chat is the one asked for.
  // A Telegram @username is opened by address, which does not depend on the language of the page.
  // The header shows the person's name, not the @username, so the chat that opened cannot be checked
  // against what was asked: callers then type but never send (`byUser`).
  const isUser = (tab, name) => siteOf(tab.url) === "telegram" && /^@[A-Za-z][A-Za-z0-9_]{3,31}$/.test(name);
  const openByUser = async (tab, name, st) => {
    const head = async () => (((await chat(tab, "header", {}, st)) || {}).name || "").trim();
    const before = await head();
    const r = await chat(tab, "goto_user", { user: name }, st);
    if (!r || !r.ok) return { r };
    // wait until the open chat is another one, so nothing is ever typed in the chat that was open before
    let shown = "";
    for (let i = 0; i < 8; i++) {
      await CP.sleep(500);
      shown = await head();
      if (shown && shown !== before) break;
    }
    if (!shown || shown === before) {
      return { r: { ok: false, msg: "Could not tell that " + name + " opened (it may be the chat that is already open): nothing was typed" } };
    }
    return { r, shown, byUser: true, headerOk: false };
  };

  const openByName = async (tab, name, st) => {
    if (isUser(tab, name)) return openByUser(tab, name, st);
    let r = await chat(tab, "search", { text: name }, st);
    if (!r || !r.ok) return { r };
    await CP.sleep(800);
    r = await chat(tab, "pick", { name }, st);
    if (!r || !r.ok) {
      await chat(tab, "clear_search", {}, st);
      return { r };
    }
    await CP.sleep(600);
    const h = await chat(tab, "header", {}, st);
    const shown = ((h && h.name) || "").toLowerCase();
    const want = name.toLowerCase();
    return { r, shown, headerOk: !!shown && (shown.includes(want) || want.includes(shown)) };
  };

  reg("chat.open", "chat", onChat(async (tab, name, st) => {
    if (!name) return CP.fail("Say which chat, for example: chat.open Anna");
    const { r } = await openByName(tab, name, st);
    return toResult(r, "Opened " + name);
  }));

  reg("chat.draft", "chat", onChat(async (tab, text, st) => {
    if (!text) return CP.fail("Say what to type, for example: chat.draft On my way");
    return toResult(await chat(tab, "draft", { text }, st), "Message typed, not sent");
  }));

  // ------------------------------------------------------------ reading (opt-in)

  const NO_READ = "Reading messages is off: turn on 'Allow reading messages' in the CursorPie Bridge options";
  const MAX_SEEN = 40;

  // A short number for a text, so what is remembered is never the text itself.
  const hash = (text) => {
    let h = 5381;
    for (let i = 0; i < text.length; i++) h = ((h * 33) ^ text.charCodeAt(i)) >>> 0;
    return h.toString(36);
  };

  // What tells the last message that came in a chat from the one before it: the page's id for the
  // message when it has one, else a number made from its text and how many came in.
  const lastIn = (list) => {
    const ins = (list || []).filter((m) => m.dir === "in");
    const m = ins[ins.length - 1];
    return m ? { marker: m.key || "#" + hash(m.text) + "|" + ins.length, text: m.text } : null;
  };

  // The last message of each chat that a command has seen, so that chat.wait gives the next one.
  const seenKey = (tab, name) => tab.id + "|" + (name || "");
  const remember = (tab, name, marker) => {
    const seen = (CP.mem.chatSeen = CP.mem.chatSeen || {});
    const k = seenKey(tab, name);
    delete seen[k];
    seen[k] = marker || "";
    const keys = Object.keys(seen);
    for (const old of keys.slice(0, Math.max(0, keys.length - MAX_SEEN))) delete seen[old];
    CP.saveMem();
  };

  // The last messages of the open chat, oldest first, or a failed result to give back.
  const messagesOf = async (tab, n, st) => {
    const r = await chat(tab, "messages", { n }, st);
    return r && r.ok ? { list: r.messages || [], name: r.name || "" } : { fail: toResult(r) };
  };

  // Notes what is in the open chat now as seen, so that chat.wait after a send waits for the answer
  // to it and not for something older. Never makes a send fail.
  const markSeen = async (tab, st) => {
    if (!st.allowRead) return;
    try {
      const got = await messagesOf(tab, 30, st);
      if (!got.fail) remember(tab, got.name, (lastIn(got.list) || {}).marker);
    } catch (e) {
      /* nothing to note */
    }
  };

  // What a command that only reads does: it needs the option, and it does not bring the tab to the
  // front (a flow can wait for a chat that stays in the background).
  const reads = (fn, only) =>
    onChat(async (tab, rest, st) => (st.allowRead ? fn(tab, rest, st) : CP.fail(NO_READ)), { quiet: true, only });

  // The unread number of the chat tabs that are open, as a plain number (for a flow to compare).
  reg("chat.unread", "chat", async ({ arg, settings }) => {
    const { site: want } = split(arg);
    let total = 0;
    let open = 0;
    let known = 0;
    for (const name of want ? [want] : Object.keys(SITES)) {
      const tab = await chatTab(name);
      if (!tab) continue;
      open++;
      let n = CP.titleCount(tab.title);
      if (n === null) {
        try {
          const r = await chat(tab, "info", { skipMuted: skipMuted(settings) }, settings);
          n = r && r.ready ? r.unreadChats : null;
        } catch (e) {
          n = null;
        }
      }
      if (n !== null) {
        total += n;
        known++;
      }
    }
    if (!open) return CP.fail((want ? SITES[want].label : "WhatsApp Web or Telegram Web") + " is not open in the browser");
    if (!known) return CP.fail("Could not read the unread number (is the page loaded, and logged in?)");
    return CP.ok(String(total), true);
  });

  reg("chat.who", "chat", reads(async (tab, rest, st) => {
    const h = await chat(tab, "header", {}, st);
    const name = (h && h.name) || "";
    return name ? CP.ok(name, true) : CP.fail("No chat is open (could not read its name)");
  }));

  // The names of the chats that have unread messages, one to a line.
  reg("chat.unread_names", "chat", reads(async (tab, rest, st) => {
    const r = await chat(tab, "unread_names", { skipMuted: skipMuted(st) }, st);
    if (!r || !r.ok) return toResult(r);
    return CP.ok((r.names || []).map((c) => c.name).join("\n"), true);
  }));

  // The last message that came in the open chat ("" when none did).
  reg("chat.last", "chat", reads(async (tab, rest, st) => {
    await chat(tab, "bottom", {}, st); // what is read is the end of the conversation, not where it was scrolled
    await CP.sleep(150);
    const got = await messagesOf(tab, 30, st);
    if (got.fail) return got.fail;
    const last = lastIn(got.list);
    remember(tab, got.name, last && last.marker);
    return CP.ok(last ? last.text : "", true);
  }));

  // The last messages of the open chat, one to a line start: `them: ...` came in, `me: ...` was sent.
  reg("chat.read", "chat", reads(async (tab, rest, st) => {
    const n = Math.max(1, Math.min(20, Math.round(CP.num(rest, 5))));
    await chat(tab, "bottom", {}, st);
    await CP.sleep(150);
    const got = await messagesOf(tab, n, st);
    if (got.fail) return got.fail;
    const last = lastIn(got.list);
    remember(tab, got.name, last && last.marker);
    return CP.ok(got.list.map((m) => (m.dir === "in" ? "them: " : "me: ") + m.text).join("\n"), true);
  }));

  // Waits for a message to come in the open chat and gives its text. "Next" means after the last one
  // a chat command of this extension saw in that chat (chat.last, chat.read, chat.wait, and a send
  // note it too), so a message that came while the flow was busy is not lost. With nothing seen yet
  // it waits for what comes after now. Nothing came in time: ok, and the text is empty.
  reg("chat.wait", "chat", reads(async (tab, rest, st) => {
    const secs = Math.max(1, Math.min(300, Math.round(CP.num(rest, 60))));
    const end = Date.now() + secs * 1000;
    let start = null; // the chat that was open when the wait began
    let base;
    let bad = 0;
    for (let first = true; ; first = false) {
      const got = await messagesOf(tab, 30, st);
      if (got.fail) {
        // a page that is busy redrawing may fail once in a while; one that fails at the start has no chat
        if (first || ++bad > 5) return got.fail;
      } else {
        bad = 0;
        const cur = lastIn(got.list);
        if (first) {
          start = got.name;
          const seen = (CP.mem.chatSeen || {})[seenKey(tab, got.name)];
          base = seen === undefined ? (cur ? cur.marker : "") : seen;
          if (seen === undefined) remember(tab, got.name, base);
        } else if (got.name && start && got.name !== start) {
          return CP.fail("The open chat changed while waiting");
        }
        if (cur && cur.marker !== base) {
          remember(tab, got.name, cur.marker);
          return CP.ok(cur.text, true);
        }
      }
      if (Date.now() >= end) return CP.ok("", false);
      await CP.sleep(700);
    }
  }));

  // Types the message in the open chat and sends it when sending is allowed.
  const sendHere = async (tab, text, st) => {
    await markSeen(tab, st);
    const typed = await chat(tab, "draft", { text }, st);
    if (!typed || !typed.ok) return toResult(typed);
    if (!st.allowSend) {
      return CP.fail("Typed, but not sent: turn on 'Allow sending messages' in the CursorPie Bridge options");
    }
    await CP.sleep(200);
    return toResult(await chat(tab, "send", {}, st), "Sent");
  };

  reg("chat.send", "chat", onChat(async (tab, text, st) => {
    if (!text) return CP.fail("Say what to send, for example: chat.send On my way");
    return sendHere(tab, text, st);
  }));

  // Opens the chat `name`, checks it is the right one, types the message and sends it when `send` is true
  // and sending is allowed.
  const toTarget = async (tab, name, text, st, send) => {
    const { r, shown, headerOk, byUser } = await openByName(tab, name, st);
    if (!r || !r.ok) return toResult(r);
    if (byUser) {
      const typed = await chat(tab, "draft", { text }, st);
      if (!typed || !typed.ok) return toResult(typed);
      return send
        ? CP.fail("Typed in " + name + ", but not sent: a chat opened by @username cannot be checked (press Send yourself)")
        : CP.ok("Message typed, not sent");
    }
    if (send && !headerOk) {
      return CP.fail(
        shown
          ? "Opened '" + shown + "', which is not '" + name + "': nothing was sent"
          : "Could not check which chat opened: nothing was sent (use chat.draft_to to type it only)"
      );
    }
    if (!send && shown && !headerOk) return CP.fail("Opened '" + shown + "', which is not '" + name + "': nothing was typed");
    return send ? sendHere(tab, text, st) : toResult(await chat(tab, "draft", { text }, st), "Message typed, not sent");
  };

  // `Name :: message`: the person's own words go to the chat of that name.
  const toChat = (send) => onChat(async (tab, rest, st) => {
    const i = rest.indexOf("::");
    const name = i < 0 ? "" : rest.slice(0, i).trim();
    const text = i < 0 ? "" : rest.slice(i + 2).trim();
    if (!name || !text) return CP.fail("Write it as: Name :: message");
    return toTarget(tab, name, text, st, send);
  });
  reg("chat.send_to", "chat", toChat(true));
  reg("chat.draft_to", "chat", toChat(false));

  // ------------------------------------------------------------ Telegram: folders, mentions
  // These work on Telegram Web only (WhatsApp Web has no folders).

  // Goes to a folder of Telegram (All, Personal, Work...) by name or number.
  reg("chat.folder", "chat", onChat(async (tab, name, st) => {
    if (!name) return CP.fail("Say which folder, for example: chat.folder Work (or its number)");
    const r = await chat(tab, "folder", { name }, st);
    return r && r.ok ? CP.ok("Folder: " + (r.name || name)) : toResult(r);
  }, { only: "telegram" }));

  // The names of the folders, one to a line. A folder name is what the person wrote, but it says
  // how they sort their chats, so it needs the same option as the names of chats.
  reg("chat.folders", "chat", reads(async (tab, rest, st) => {
    const r = await chat(tab, "folders", {}, st);
    if (!r || !r.ok) return toResult(r);
    return CP.ok((r.folders || []).map((f) => f.name).join("\n"), true);
  }, "telegram"));

  // The next chat that has an @mention, going round the list (a muted chat counts).
  reg("chat.next_mention", "chat", onChat(async (tab, rest, st) => {
    const r = await chat(tab, "open_mention", { dir: 1 }, st);
    return r && r.found === false ? CP.ok(r.msg, true) : toResult(r);
  }, { only: "telegram" }));

  // ------------------------------------------------------------ saved replies
  // Short texts the person wrote in the options ({"omw": "On my way, back at {time}"}); `{time}`,
  // `{date}`, `{name}` and `{first}` (the first word of the name) are filled in. {name} is the chat's
  // name: the one written after `::`, else the name in the header of the open chat, which is read
  // only when "Allow reading messages" is on.
  const TEMPLATE_KEY = /^[a-z0-9][a-z0-9_-]{0,31}$/;
  const templatesOf = (st) => {
    const out = {};
    const t = st && st.templates;
    if (t && typeof t === "object" && !Array.isArray(t)) {
      for (const [k, v] of Object.entries(t)) {
        const key = String(k).trim().toLowerCase();
        if (TEMPLATE_KEY.test(key) && typeof v === "string" && v.trim()) out[key] = v.slice(0, 2000);
      }
    }
    return out;
  };
  CP.chatTemplates = templatesOf;

  const two = (n) => String(n).padStart(2, "0");
  const fillTemplate = async (tab, text, st, chatName) => {
    const d = new Date(Date.now());
    let out = text
      .replace(/\{time\}/gi, two(d.getHours()) + ":" + two(d.getMinutes()))
      .replace(/\{date\}/gi, d.getFullYear() + "-" + two(d.getMonth() + 1) + "-" + two(d.getDate()));
    if (/\{(name|first)\}/i.test(out)) {
      let name = chatName || "";
      if (!name) {
        if (!st.allowRead) {
          return { fail: CP.fail("This saved reply uses {name}: write the chat after :: (name :: Chat), or turn on 'Allow reading messages'") };
        }
        const h = await chat(tab, "header", {}, st);
        name = ((h && h.name) || "").trim();
        if (!name) return { fail: CP.fail("No chat is open (could not read its name)") };
      }
      out = out.replace(/\{name\}/gi, name).replace(/\{first\}/gi, name.replace(/^@/, "").split(/\s+/)[0]);
    }
    return { text: out.slice(0, 2000) };
  };

  // `key` types the saved reply in the open chat; `key :: Chat` opens that chat first and checks it,
  // like chat.send_to. Sent only when "Allow sending messages" is on.
  reg("chat.template", "chat", onChat(async (tab, rest, st) => {
    const i = rest.indexOf("::");
    const key = (i < 0 ? rest : rest.slice(0, i)).trim().toLowerCase();
    const target = i < 0 ? "" : rest.slice(i + 2).trim();
    if (!key) return CP.fail("Say which saved reply, for example: chat.template omw");
    if (i >= 0 && !target) return CP.fail("Write it as: reply :: Chat");
    const all = templatesOf(st);
    if (!all[key]) {
      const keys = Object.keys(all);
      return CP.fail(
        keys.length
          ? "No saved reply '" + key + "'. You have: " + keys.join(", ")
          : "No saved replies yet: write them in the CursorPie Bridge options"
      );
    }
    const f = await fillTemplate(tab, all[key], st, target);
    if (f.fail) return f.fail;
    return target ? toTarget(tab, target, f.text, st, true) : sendHere(tab, f.text, st);
  }));

  // The names of the saved replies, for the person or a flow (what they say is not given).
  reg("chat.templates", "chat", async ({ settings }) => {
    const keys = Object.keys(templatesOf(settings));
    return keys.length ? CP.ok(keys.join(", "), true) : CP.ok("No saved replies yet", true);
  });
})(typeof self !== "undefined" ? self : globalThis);
