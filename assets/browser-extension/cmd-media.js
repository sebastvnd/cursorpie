// CursorPie Bridge: the media.* commands (see docs/BROWSER_EXTENSION.md).
//
// The hard part is not pressing the button, it is knowing WHICH tab the person means. The order is:
//   1. a tab that is making sound right now (the most recent one if there are several),
//   2. the tab that played last (so "play" resumes what "pause" stopped, even from another window),
//   3. a tab on a known player site (YouTube, Spotify, ...), the one showing first.
"use strict";

(function (g) {
  const CP = g.CP;
  const api = CP.api;
  const reg = CP.reg;

  // Keep in step with host_permissions of the manifests.
  const PLAYER_SITES = [
    "youtube.com",
    "open.spotify.com",
    "soundcloud.com",
    "music.apple.com",
    "deezer.com",
    "tidal.com",
    "bandcamp.com",
    "twitch.tv",
    "vimeo.com",
    "netflix.com",
    "primevideo.com",
    "dailymotion.com",
  ];
  CP.PLAYER_SITES = PLAYER_SITES;
  CP.isPlayerUrl = (url) => !!url && CP.WEB.test(url) && PLAYER_SITES.some((d) => CP.hostIs(url, d));

  // The tab the media commands act on, and whether it is making sound. `null` when there is none.
  CP.mediaTab = async () => {
    const audible = await api.tabs.query({ audible: true });
    if (audible.length) {
      audible.sort((a, b) => (CP.mem.audibleAt[b.id] || 0) - (CP.mem.audibleAt[a.id] || 0));
      return { tab: audible[0], playing: true };
    }
    if (CP.mem.lastMedia !== null && CP.mem.lastMedia !== undefined) {
      try {
        return { tab: await api.tabs.get(CP.mem.lastMedia), playing: false };
      } catch (e) {
        CP.mem.lastMedia = null;
      }
    }
    const known = (await api.tabs.query({})).filter((t) => CP.isPlayerUrl(t.url));
    if (!known.length) return null;
    known.sort((a, b) => Number(b.active) - Number(a.active) || (CP.mem.audibleAt[b.id] || 0) - (CP.mem.audibleAt[a.id] || 0));
    return { tab: known[0], playing: false };
  };

  const NONE = "No player found: open YouTube, Spotify, SoundCloud... in a tab first";

  // Runs a media action in the target tab and turns the answer of the page into a result.
  const act = async (action, opts, tab) => {
    const found = tab ? { tab } : await CP.mediaTab();
    if (!found) return { res: CP.fail(NONE) };
    let r;
    try {
      r = await CP.runInTab(found.tab.id, CP.pageMedia, [action, opts || {}]);
    } catch (e) {
      return { res: CP.fail(CP.friendly(e, found.tab)) };
    }
    if (!r) return { res: CP.fail("The page did not answer") };
    if (r.ok) {
      CP.mem.lastMedia = found.tab.id;
      CP.saveMem();
    }
    return { res: r.ok ? CP.ok(r.msg) : CP.fail(r.msg), r, tab: found.tab };
  };

  const simple = (action, optsOf) => async ({ arg, settings }) => (await act(action, optsOf ? optsOf(arg, settings) : {})).res;

  reg("media.play", "media", simple("play"));
  reg("media.pause", "media", simple("pause"));
  reg("media.toggle", "media", simple("toggle"));
  reg("media.restart", "media", simple("restart"));
  reg("media.like", "media", simple("like"));
  reg("media.captions", "media", simple("captions"));
  reg("media.skip_ad", "media", simple("skipad"));
  reg("media.shuffle", "media", simple("shuffle"));
  reg("media.repeat", "media", simple("repeat"));
  reg("media.mute", "media", simple("mute"));

  // next / previous: when the person wants to see results, say what is playing now (the page needs
  // a moment to change the track).
  const track = (action) => async ({ settings }) => {
    const { res, tab } = await act(action, {});
    if (res.ok && settings.notify === "all" && tab) {
      await CP.sleep(900);
      try {
        const info = await CP.runInTab(tab.id, CP.pageMedia, ["info", {}]);
        if (info && info.title) res.message = describe(info);
      } catch (e) {
        /* keep the plain answer */
      }
    }
    return res;
  };
  reg("media.next", "media", track("next"));
  reg("media.prev", "media", track("prev"));

  reg("media.seek_fwd", "media", simple("seek", (arg, s) => ({ by: Math.abs(CP.num(arg, s.seekSeconds)) })));
  reg("media.seek_back", "media", simple("seek", (arg, s) => ({ by: -Math.abs(CP.num(arg, s.seekSeconds)) })));
  reg("media.volume_up", "media", simple("volume", (arg, s) => ({ by: Math.abs(CP.num(arg, s.volumeStep)) })));
  reg("media.volume_down", "media", simple("volume", (arg, s) => ({ by: -Math.abs(CP.num(arg, s.volumeStep)) })));
  reg("media.speed_up", "media", simple("speed", (arg, s) => ({ by: Math.abs(CP.num(arg, s.speedStep)) })));
  reg("media.speed_down", "media", simple("speed", (arg, s) => ({ by: -Math.abs(CP.num(arg, s.speedStep)) })));
  reg("media.speed_reset", "media", simple("speed", () => ({ to: 1 })));

  const describe = (info) => {
    const what = info.artist ? info.title + " — " + info.artist : info.title;
    return (info.playing ? "Playing: " : "Paused: ") + what + (info.site ? " (" + info.site + ")" : "");
  };

  // Says what is playing. This is the one place a title leaves the page, and only because the person
  // asked: the answer goes to the app, which shows it as a notification.
  reg("media.now_playing", "media", async () => {
    const { res, r } = await act("info", {});
    if (!res.ok) return res;
    if (!r.title) return CP.ok("Nothing is playing", true);
    return CP.ok(describe(r), true);
  });

  // Answers "playing", "paused" or "none" (no player at all). Made for flows, which branch on it;
  // unlike the other media commands it does not fail when there is no player. No title leaves the
  // page.
  reg("media.state", "media", async () => {
    const found = await CP.mediaTab();
    if (!found) return CP.ok("none", true);
    const { res, r } = await act("info", {}, found.tab);
    if (!res.ok) return res;
    if (!r.hasMedia && !r.title) return CP.ok("none", true);
    return CP.ok(r.playing ? "playing" : "paused", true);
  });

  // Stops every tab that is making sound.
  reg("media.pause_all", "media", async () => {
    const tabs = await api.tabs.query({ audible: true });
    if (!tabs.length) return CP.ok("Nothing was playing");
    tabs.sort((a, b) => (CP.mem.audibleAt[b.id] || 0) - (CP.mem.audibleAt[a.id] || 0));
    let n = 0;
    for (const t of tabs) {
      const { res } = await act("pause", {}, t);
      if (res.ok) n++;
    }
    // the one that played last is what "play" brings back
    CP.mem.lastMedia = tabs[0].id;
    CP.saveMem();
    return CP.ok("Paused " + n + (n === 1 ? " tab" : " tabs"));
  });

  // Shows the tab with the player.
  reg("media.focus", "media", async () => {
    const found = await CP.mediaTab();
    if (!found) return CP.fail(NONE);
    await CP.show(found.tab);
    return CP.ok();
  });

  // The behaviour behind the option "Starting one player pauses the others". Called by background.js
  // when a tab starts making sound.
  CP.pauseOthers = async (startedId) => {
    const tabs = await api.tabs.query({ audible: true });
    for (const t of tabs) {
      if (t.id !== startedId) await act("pause", {}, t);
    }
    // act() remembers the tab it paused as "the one that played last": that is the one that just
    // started, or "play" after a "pause" would wake the player that was silenced.
    CP.mem.lastMedia = startedId;
    CP.saveMem();
  };
})(typeof self !== "undefined" ? self : globalThis);
