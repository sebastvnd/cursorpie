// CursorPie Bridge: the tab.*, window.* and page.* commands (see docs/BROWSER_EXTENSION.md).
// These use only the browser's own tab and window API, except page.* which scrolls the page.
"use strict";

(function (g) {
  const CP = g.CP;
  const api = CP.api;
  const reg = CP.reg;

  const ZOOMS = [0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];

  // Runs `fn(tab, tabsOfItsWindow)` for the tab that has the user's attention.
  const withTab = (fn) => async () => {
    const tab = await CP.currentTab();
    if (!tab) return CP.fail("No tab is open");
    const tabs = (await api.tabs.query({ windowId: tab.windowId })).sort((a, b) => a.index - b.index);
    return fn(tab, tabs);
  };

  const go = async (tab) => {
    await CP.show(tab);
    return CP.ok();
  };

  // ------------------------------------------------------------------ moving between tabs

  reg("tab.next", "tab", withTab((tab, tabs) => go(tabs[(tab.index + 1) % tabs.length])));
  reg("tab.prev", "tab", withTab((tab, tabs) => go(tabs[(tab.index - 1 + tabs.length) % tabs.length])));
  reg("tab.first", "tab", withTab((tab, tabs) => go(tabs[0])));
  reg("tab.end", "tab", withTab((tab, tabs) => go(tabs[tabs.length - 1])));

  // The tab you were on before this one (like Alt+Tab, for tabs).
  reg("tab.recent", "tab", async () => {
    const tab = await CP.currentTab();
    if (!tab) return CP.fail("No tab is open");
    const list = (CP.mem.mru[tab.windowId] || []).filter((id) => id !== tab.id).reverse();
    for (const id of list) {
      try {
        return await go(await api.tabs.get(id));
      } catch (e) {
        /* closed since: try the one before */
      }
    }
    return CP.fail("No earlier tab in this window");
  });

  // The tab that is making sound: the quickest way to find the page that is playing.
  reg("tab.goto_audible", "tab", async () => {
    const tabs = await api.tabs.query({ audible: true });
    if (!tabs.length) return CP.fail("No tab is playing sound");
    tabs.sort((a, b) => (CP.mem.audibleAt[b.id] || 0) - (CP.mem.audibleAt[a.id] || 0));
    return go(tabs[0]);
  });

  // The first tab whose title or address has the text (the current window first).
  reg("tab.find", "tab", async ({ arg }) => {
    if (!arg) return CP.fail("Say what to look for, for example: tab.find inbox");
    const want = arg.toLowerCase();
    const cur = await CP.currentTab();
    const tabs = (await api.tabs.query({})).filter(
      (t) => ((t.title || "") + " " + (t.url || "")).toLowerCase().includes(want) && (!cur || t.id !== cur.id)
    );
    tabs.sort((a, b) => (cur && b.windowId === cur.windowId ? 1 : 0) - (cur && a.windowId === cur.windowId ? 1 : 0));
    if (!tabs.length) return CP.fail("No other tab has '" + arg + "'");
    return go(tabs[0]);
  });

  reg("tab.move_left", "tab", withTab(async (tab) => {
    await api.tabs.move(tab.id, { index: Math.max(0, tab.index - 1) });
    return CP.ok();
  }));
  reg("tab.move_right", "tab", withTab(async (tab) => {
    await api.tabs.move(tab.id, { index: tab.index + 1 });
    return CP.ok();
  }));

  // ------------------------------------------------------------------ closing and opening

  reg("tab.close", "tab", withTab(async (tab) => {
    await api.tabs.remove(tab.id);
    return CP.ok();
  }));

  reg("tab.reopen", "tab", async () => {
    if (!api.sessions || !api.sessions.restore) return CP.fail("This browser cannot reopen closed tabs");
    await api.sessions.restore();
    return CP.ok();
  });

  reg("tab.new", "tab", async ({ arg }) => {
    // only a web address is taken from the app: nothing else is opened from here
    if (arg && !CP.WEB.test(arg)) return CP.fail("Only http(s) addresses can be opened");
    await api.tabs.create(arg ? { url: arg } : {});
    return CP.ok();
  });

  reg("tab.duplicate", "tab", withTab(async (tab) => {
    await api.tabs.duplicate(tab.id);
    return CP.ok();
  }));

  reg("tab.to_window", "tab", withTab(async (tab, tabs) => {
    if (tabs.length < 2) return CP.fail("This is the only tab of its window");
    await api.windows.create({ tabId: tab.id, focused: true });
    return CP.ok();
  }));

  reg("tab.close_others", "tab", withTab(async (tab, tabs) => {
    const ids = tabs.filter((t) => t.id !== tab.id && !t.pinned).map((t) => t.id);
    if (ids.length) await api.tabs.remove(ids);
    return CP.ok("Closed " + ids.length + (ids.length === 1 ? " tab" : " tabs"));
  }));

  reg("tab.close_right", "tab", withTab(async (tab, tabs) => {
    const ids = tabs.filter((t) => t.index > tab.index && !t.pinned).map((t) => t.id);
    if (ids.length) await api.tabs.remove(ids);
    return CP.ok("Closed " + ids.length + (ids.length === 1 ? " tab" : " tabs"));
  }));

  // Closes the second, third... tab that has the same page open (the address without #fragment).
  reg("tab.close_dupes", "tab", withTab(async (tab, tabs) => {
    const seen = new Map();
    const dupes = [];
    // the tab you are on is the one that is kept
    for (const t of [tab, ...tabs.filter((x) => x.id !== tab.id)]) {
      if (!t.url || !CP.WEB.test(t.url) || t.pinned) continue;
      const key = t.url.split("#")[0].replace(/\/$/, "");
      if (seen.has(key)) dupes.push(t.id);
      else seen.set(key, t.id);
    }
    if (dupes.length) await api.tabs.remove(dupes);
    return CP.ok(dupes.length ? "Closed " + dupes.length + " duplicate" + (dupes.length === 1 ? "" : "s") : "No duplicates", true);
  }));

  // Frees memory: the other tabs stay in the bar but are unloaded until you open them.
  reg("tab.discard_others", "tab", withTab(async (tab, tabs) => {
    let n = 0;
    for (const t of tabs) {
      if (t.id === tab.id || t.pinned || t.audible || t.discarded) continue;
      try {
        await api.tabs.discard(t.id);
        n++;
      } catch (e) {
        /* a tab that cannot be discarded stays */
      }
    }
    return CP.ok("Unloaded " + n + (n === 1 ? " tab" : " tabs"), true);
  }));

  // ------------------------------------------------------------------ the current tab

  reg("tab.pin", "tab", withTab(async (tab) => {
    await api.tabs.update(tab.id, { pinned: !tab.pinned });
    return CP.ok(tab.pinned ? "Unpinned" : "Pinned");
  }));

  reg("tab.mute", "tab", withTab(async (tab) => {
    const muted = !(tab.mutedInfo && tab.mutedInfo.muted);
    await api.tabs.update(tab.id, { muted });
    return CP.ok(muted ? "Tab muted" : "Tab sound on");
  }));

  reg("tab.reload", "tab", withTab(async (tab) => {
    await api.tabs.reload(tab.id);
    return CP.ok();
  }));
  reg("tab.hard_reload", "tab", withTab(async (tab) => {
    await api.tabs.reload(tab.id, { bypassCache: true });
    return CP.ok();
  }));

  reg("tab.back", "tab", withTab(async (tab) => {
    if (!api.tabs.goBack) return CP.fail("This browser cannot go back from here");
    await api.tabs.goBack(tab.id);
    return CP.ok();
  }));
  reg("tab.forward", "tab", withTab(async (tab) => {
    if (!api.tabs.goForward) return CP.fail("This browser cannot go forward from here");
    await api.tabs.goForward(tab.id);
    return CP.ok();
  }));

  const zoomBy = (dir) =>
    withTab(async (tab) => {
      const now = await api.tabs.getZoom(tab.id);
      let to;
      if (dir === 0) to = 1;
      else if (dir > 0) to = ZOOMS.find((z) => z > now + 0.001) || ZOOMS[ZOOMS.length - 1];
      else to = [...ZOOMS].reverse().find((z) => z < now - 0.001) || ZOOMS[0];
      await api.tabs.setZoom(tab.id, to);
      return CP.ok("Zoom " + Math.round(to * 100) + "%");
    });
  reg("tab.zoom_in", "tab", zoomBy(1));
  reg("tab.zoom_out", "tab", zoomBy(-1));
  reg("tab.zoom_reset", "tab", zoomBy(0));

  // Two questions for flows (the answer goes to the flow, or shows as a notification). The address
  // is what the app is sent anyway with the tab list; the title is never part of an answer here.
  // (one line each: the Rust test that compares the commands reads `reg("id"` at a line start)
  const urlOf = async (tab) => (CP.WEB.test(tab.url || "") ? CP.ok(tab.url, true) : CP.fail("This tab has no web address"));
  reg("tab.url", "tab", withTab(urlOf));
  reg("tab.count", "tab", withTab(async (tab, tabs) => CP.ok(String(tabs.length), true)));

  // ------------------------------------------------------------------ windows

  const withWindow = (fn) => async (c) => {
    const tab = await CP.currentTab();
    const win = tab ? await api.windows.get(tab.windowId) : await api.windows.getLastFocused();
    return fn(win, c);
  };

  reg("window.new", "window", async () => {
    await api.windows.create({ focused: true });
    return CP.ok();
  });

  reg("window.private", "window", async () => {
    try {
      await api.windows.create({ incognito: true, focused: true });
      return CP.ok();
    } catch (e) {
      return CP.fail("Allow the extension in private windows first (in the browser's extension settings)");
    }
  });

  reg("window.minimize", "window", withWindow(async (win) => {
    await api.windows.update(win.id, { state: "minimized" });
    return CP.ok();
  }));

  reg("window.maximize", "window", withWindow(async (win) => {
    await api.windows.update(win.id, { state: win.state === "maximized" ? "normal" : "maximized" });
    return CP.ok();
  }));

  reg("window.fullscreen", "window", withWindow(async (win) => {
    await api.windows.update(win.id, { state: win.state === "fullscreen" ? "normal" : "fullscreen" });
    return CP.ok();
  }));

  reg("window.close", "window", withWindow(async (win) => {
    await api.windows.remove(win.id);
    return CP.ok();
  }));

  // ------------------------------------------------------------------ the page

  // Runs inside the page (self-contained, see CP.runInTab).
  CP.pageScroll = function pageScroll(where) {
    const el = document.scrollingElement || document.documentElement;
    const page = Math.round(window.innerHeight * 0.85);
    const top =
      where === "top" ? 0 : where === "bottom" ? el.scrollHeight : el.scrollTop + (where === "up" ? -page : page);
    window.scrollTo({ top, behavior: "smooth" });
    return true;
  };

  const scroll = (where) =>
    withTab(async (tab) => {
      if (!tab.url || !CP.WEB.test(tab.url)) return CP.fail("This page cannot be scrolled from here");
      try {
        await CP.runInTab(tab.id, CP.pageScroll, [where]);
        return CP.ok();
      } catch (e) {
        return CP.fail(CP.friendly(e, tab));
      }
    });
  reg("page.top", "page", scroll("top"));
  reg("page.bottom", "page", scroll("bottom"));
  reg("page.up", "page", scroll("up"));
  reg("page.down", "page", scroll("down"));
})(typeof self !== "undefined" ? self : globalThis);
