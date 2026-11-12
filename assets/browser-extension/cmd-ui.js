// CursorPie Bridge: the ui.* commands (see docs/BROWSER_EXTENSION.md and docs/PANELS.md). Each one
// asks the person something in a panel of CursorPie and then does what they chose, using the same
// commands as a slice does (tab.find, tab.close_others, media.*). They need "Let the browser
// extension show panels" in CursorPie's Settings; without it they say so.
"use strict";

(function (g) {
  const CP = g.CP;
  const api = CP.api;
  const reg = CP.reg;

  const clip = (s, n) => {
    s = String(s || "");
    return s.length > n ? s.slice(0, n - 1) + "\u2026" : s;
  };

  // What to do with an answer: `fn(answer)` when something was chosen; an error that says why when
  // the panel could not be shown; nothing when it was cancelled or nobody answered.
  const answered = async (r, fn) => {
    if (r.outcome === "submitted") return fn(r);
    if (r.outcome === "unavailable") return CP.fail(r.reason || "CursorPie could not show the panel");
    return CP.ok();
  };

  // ------------------------------------------------------------------ a tab from a list

  // The open pages, the current window and the most recently used first, with a search box.
  reg("ui.tabs", "ui", async () => {
    const cur = await CP.currentTab();
    const all = (await api.tabs.query({})).filter((t) => t.url && CP.WEB.test(t.url) && (!cur || t.id !== cur.id));
    if (!all.length) return CP.fail("No other page is open");
    const recent = (t) => (CP.mem.mru[t.windowId] || []).lastIndexOf(t.id);
    const here = (t) => (cur && t.windowId === cur.windowId ? 1 : 0);
    all.sort((a, b) => here(b) - here(a) || recent(b) - recent(a));
    const shown = all.slice(0, 60);
    const r = await CP.ui.show({
      id: "ui.tabs",
      title: "Go to a tab",
      subtitle: all.length + (all.length === 1 ? " other page" : " other pages") + " open",
      icon: "browser",
      place: "center",
      width: 460,
      widgets: [
        {
          type: "choice",
          id: "tab",
          submit: true,
          search: true,
          options: shown.map((t) => ({ value: String(t.id), label: clip(t.title || CP.hostOf(t.url), 100), hint: CP.hostOf(t.url) })),
        },
      ],
    });
    return answered(r, async (res) => {
      const tab = shown.find((t) => String(t.id) === String(res.values.tab));
      if (!tab) return CP.fail("The tab was closed");
      try {
        await CP.show(tab);
      } catch (e) {
        return CP.fail("The tab was closed");
      }
      return CP.ok();
    });
  });

  // ------------------------------------------------------------------ ask, then go

  // A box for a word, then the same as tab.find. The text after the command is the heading.
  reg("ui.find_tab", "ui", async ({ arg }) => {
    const r = await CP.ui.show({
      id: "ui.find_tab",
      title: arg || "Find a tab",
      icon: "browser",
      place: "center",
      width: 400,
      widgets: [{ type: "input", id: "text", hint: "Words in the title or address", required: true }],
      buttons: [
        { id: "cancel", label: "Cancel", cancel: true },
        { id: "go", label: "Go", tone: "primary" },
      ],
    });
    return answered(r, (res) => CP.dispatch("tab.find", String(res.values.text || "").trim()));
  });

  // ------------------------------------------------------------------ ask, then close

  // Says how many tabs will go, and closes them only after the person agrees.
  reg("ui.confirm_close_others", "ui", async () => {
    const tab = await CP.currentTab();
    if (!tab) return CP.fail("No tab is open");
    const n = (await api.tabs.query({ windowId: tab.windowId })).filter((t) => t.id !== tab.id && !t.pinned).length;
    if (!n) return CP.ok("No other tabs to close");
    const r = await CP.ui.show({
      id: "ui.confirm_close_others",
      title: "Close the other tabs?",
      icon: "browser",
      place: "center",
      widgets: [{ type: "text", text: "Close " + n + (n === 1 ? " other tab" : " other tabs") + " in this window? Pinned tabs stay." }],
      buttons: [
        { id: "no", label: "Keep them", cancel: true },
        { id: "yes", label: "Close " + n, tone: "danger" },
      ],
    });
    return answered(r, (res) => (res.button === "yes" ? CP.dispatch("tab.close_others", "") : CP.ok()));
  });

  // ------------------------------------------------------------------ a menu for the player

  const MEDIA_MENU = [
    ["media.toggle", "Play / pause"],
    ["media.next", "Next track"],
    ["media.prev", "Previous track"],
    ["media.volume_up", "Volume up"],
    ["media.volume_down", "Volume down"],
    ["media.mute", "Mute / unmute"],
    ["media.like", "Like"],
  ];

  // One list at the bottom of the screen; the pick runs that media command.
  reg("ui.media", "ui", async () => {
    const r = await CP.ui.show({
      id: "ui.media",
      title: "Media",
      icon: "browser",
      place: "bottom",
      width: 320,
      widgets: [{ type: "choice", id: "action", submit: true, search: false, options: MEDIA_MENU.map((m) => ({ value: m[0], label: m[1] })) }],
    });
    return answered(r, (res) => {
      const id = String(res.values.action);
      if (!MEDIA_MENU.some((m) => m[0] === id)) return CP.fail("Unknown choice");
      return CP.dispatch(id, "");
    });
  });

  // ------------------------------------------------------------------ a remote for the video

  // The buttons of the remote, in the order they are shown. `{n}` is the seconds of one skip.
  // Every id is a command that media.* already has, so the remote can do nothing else.
  const VIDEO_REMOTE = [
    ["media.prev", "Previous video"],
    ["media.seek_back", "Back {n}s"],
    ["media.toggle", "Play / pause"],
    ["media.seek_fwd", "Forward {n}s"],
    ["media.next", "Next video"],
    ["media.play", "Play (resume)"],
    ["media.pause", "Pause"],
    ["media.restart", "Restart"],
    ["media.volume_down", "Volume down"],
    ["media.mute", "Mute / unmute"],
    ["media.volume_up", "Volume up"],
    ["media.speed_down", "Slower"],
    ["media.speed_reset", "Normal speed"],
    ["media.speed_up", "Faster"],
    ["media.captions", "Captions"],
    ["media.skip_ad", "Skip ad"],
    ["media.like", "Like"],
    ["media.focus", "Show the tab"],
  ];
  const VIDEO_MAX_ROUNDS = 200; // a remote that is never closed still ends
  const VIDEO_STEP_MIN = 5;
  const VIDEO_STEP_MAX = 60;

  const videoStep = (v, dflt) => {
    const n = Math.round(Number(v));
    return Number.isFinite(n) ? Math.min(VIDEO_STEP_MAX, Math.max(VIDEO_STEP_MIN, n)) : dflt;
  };

  // What is playing, for the panel's second line. Empty when it cannot be known; never throws.
  const videoNow = async () => {
    try {
      const found = await CP.mediaTab();
      if (!found) return "No player found: open YouTube in a tab";
      const info = await CP.runInTab(found.tab.id, CP.pageMedia, ["info", {}]);
      if (info && info.title) return (info.playing ? "Playing: " : "Paused: ") + clip(info.title, 70);
    } catch (e) {
      /* the panel works without it */
    }
    return "";
  };

  // A remote control that stays on the screen: every press runs the media command and the panel
  // comes back, until it is closed (Close, Esc), times out, or CursorPie is gone. One slider sets
  // how far "Back" and "Forward" jump; it starts at the extension's "seek seconds" setting.
  reg("ui.video", "ui", async ({ settings }) => {
    let step = videoStep(settings.seekSeconds, 10);
    let note = "";
    for (let round = 0; round < VIDEO_MAX_ROUNDS; round++) {
      const subtitle = note || (await videoNow());
      const r = await CP.ui.show({
        id: "ui.video",
        title: "Video remote",
        subtitle,
        icon: "browser",
        place: "bottom",
        width: 460,
        timeout_s: 300,
        widgets: [
          {
            type: "choice",
            id: "action",
            layout: "chips",
            submit: true,
            search: false,
            options: VIDEO_REMOTE.map((m) => ({ value: m[0], label: m[1].replace("{n}", String(step)) })),
          },
          { type: "slider", id: "step", label: "Skip by", min: VIDEO_STEP_MIN, max: VIDEO_STEP_MAX, step: 5, value: step, unit: "s" },
        ],
        buttons: [{ id: "close", label: "Close", cancel: true }],
      });
      if (r.outcome !== "submitted") return answered(r, () => CP.ok());
      step = videoStep(r.values.step, step);
      const id = String(r.values.action || "");
      if (!VIDEO_REMOTE.some((m) => m[0] === id)) {
        note = "Unknown choice";
        continue;
      }
      const res = await CP.dispatch(id, id === "media.seek_fwd" || id === "media.seek_back" ? String(step) : "");
      note = res.ok ? "" : res.message || "Did not work";
      // the page needs a moment to change the video before its title is read again
      await CP.sleep(id === "media.next" || id === "media.prev" ? 900 : 150);
    }
    return CP.ok();
  });
})(typeof self !== "undefined" ? self : globalThis);
