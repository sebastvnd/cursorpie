// CursorPie Bridge: the code that runs INSIDE a page to control its player.
//
// `CP.pageMedia(action, opts)` is sent to the tab as text (see CP.runInTab), so it must not use
// anything outside its own body. It works in two layers:
//   1. a site adapter (YouTube, YouTube Music, Spotify, SoundCloud, ...) that presses the
//      buttons of that site's own player, which is the only way to reach "next" and "like";
//   2. the HTML <video>/<audio> element, which every other site has: play, pause, seek, volume,
//      speed.
// What is playing comes from the page's own Media Session (`navigator.mediaSession`), the same
// information the operating system shows in its media widget.
//
// The selectors of the adapters are the part that breaks when a site redesigns its player; they
// are listed in one table at the top so a fix is one line.
"use strict";

(function (g) {
  const CP = (g.CP = g.CP || {});

  CP.pageMedia = function pageMedia(action, opts) {
    opts = opts || {};
    const host = location.hostname.replace(/^www\./, "").toLowerCase();

    // ------------------------------------------------------------ site adapters
    // Each list is tried in order; the first button that exists and is not disabled is pressed.
    const SITES = [
      {
        name: "YouTube Music",
        match: (h) => h === "music.youtube.com",
        toggle: ["#play-pause-button", "ytmusic-player-bar .play-pause-button"],
        next: ["ytmusic-player-bar .next-button", ".next-button.ytmusic-player-bar"],
        prev: ["ytmusic-player-bar .previous-button", ".previous-button.ytmusic-player-bar"],
        like: ["ytmusic-player-bar #like-button-renderer .like button", "ytmusic-player-bar .like"],
        title: [".title.ytmusic-player-bar"],
        artist: [".byline.ytmusic-player-bar"],
        playingBy: (root) => {
          const b = root.querySelector("#play-pause-button");
          return b ? /pause/i.test(b.getAttribute("title") || b.getAttribute("aria-label") || "") : null;
        },
      },
      {
        name: "YouTube",
        match: (h) => h === "youtube.com" || h.endsWith(".youtube.com"),
        toggle: [".ytp-play-button"],
        next: [".ytp-next-button"],
        prev: [".ytp-prev-button"],
        like: [
          "like-button-view-model button",
          "#segmented-like-button button",
          "ytd-toggle-button-renderer#like-button button",
        ],
        captions: [".ytp-subtitles-button"],
        skipAd: [
          ".ytp-skip-ad-button",
          ".ytp-ad-skip-button-modern",
          ".ytp-ad-skip-button",
          ".ytp-ad-skip-button-container button",
        ],
        // when there is no "next" button (a single video), go where autoplay would go
        nextFallback: [
          "ytd-watch-next-secondary-results-renderer a[href^='/watch']",
          "yt-lockup-view-model a[href^='/watch']",
          "ytd-compact-video-renderer a[href^='/watch']",
        ],
        title: ["h1.ytd-watch-metadata yt-formatted-string", "#title h1", "h1.title"],
        artist: ["ytd-channel-name a", "#owner #channel-name a"],
      },
      {
        name: "Spotify",
        match: (h) => h === "open.spotify.com",
        toggle: ["[data-testid='control-button-playpause']"],
        next: ["[data-testid='control-button-skip-forward']"],
        prev: ["[data-testid='control-button-skip-back']"],
        like: [
          "[data-testid='now-playing-widget'] button[aria-label*='Liked' i]",
          "[data-testid='now-playing-widget'] button[aria-label*='Add to' i]",
          "[data-testid='control-button-heart']",
        ],
        shuffle: ["[data-testid='control-button-shuffle']"],
        repeat: ["[data-testid='control-button-repeat']"],
        playingBy: (root) => {
          const b = root.querySelector("[data-testid='control-button-playpause']");
          return b ? /pause/i.test(b.getAttribute("aria-label") || "") : null;
        },
        title: ["[data-testid='context-item-link']"],
        artist: ["[data-testid='context-item-info-artist']"],
      },
      {
        name: "SoundCloud",
        match: (h) => h === "soundcloud.com",
        toggle: [".playControls__play", ".playControl"],
        next: [".playControls__next", ".skipControl__next"],
        prev: [".playControls__prev", ".skipControl__previous"],
        like: [".playbackSoundBadge__like"],
        playingBy: (root) => {
          const b = root.querySelector(".playControls__play, .playControl");
          return b ? b.classList.contains("playing") : null;
        },
        title: [".playbackSoundBadge__titleLink span[aria-hidden='true']", ".playbackSoundBadge__titleLink"],
        artist: [".playbackSoundBadge__lightLink"],
      },
      {
        name: "Deezer",
        match: (h) => h === "deezer.com",
        toggle: ["[data-testid='play_button_pause']", "[data-testid='play_button_play']", "[data-testid='play_button']"],
        next: ["[data-testid='next_track_button']"],
        prev: ["[data-testid='previous_track_button']"],
      },
      {
        name: "TIDAL",
        match: (h) => h === "tidal.com" || h === "listen.tidal.com",
        toggle: ["[data-test='play']", "[data-test='pause']"],
        next: ["[data-test='next']"],
        prev: ["[data-test='previous']"],
      },
      {
        name: "Bandcamp",
        match: (h) => h === "bandcamp.com" || h.endsWith(".bandcamp.com"),
        toggle: [".playbutton"],
        next: [".nextbutton"],
        prev: [".prevbutton"],
      },
      {
        name: "Twitch",
        match: (h) => h === "twitch.tv",
        toggle: ["button[data-a-target='player-play-pause-button']"],
      },
      {
        name: "Netflix",
        match: (h) => h === "netflix.com",
        next: [
          "[data-uia='next-episode-seamless-button']",
          "[data-uia='next-episode-seamless-button-draining']",
        ],
        skipAd: ["[data-uia='player-skip-intro']", "[data-uia='player-skip-recap']"],
      },
    ];
    const site = SITES.find((s) => s.match(host)) || null;

    // ------------------------------------------------------------ helpers
    const q = (s, root) => {
      try {
        return (root || document).querySelector(s);
      } catch (e) {
        return null;
      }
    };
    const qa = (s, root) => {
      try {
        return Array.from((root || document).querySelectorAll(s));
      } catch (e) {
        return [];
      }
    };
    const usable = (el) =>
      !!el &&
      !el.disabled &&
      el.getAttribute("aria-disabled") !== "true" &&
      getComputedStyle(el).display !== "none" &&
      getComputedStyle(el).visibility !== "hidden";
    // Presses the first usable button of a list of selectors.
    const press = (list) => {
      for (const s of list || []) {
        const el = q(s);
        if (usable(el)) {
          el.click();
          return true;
        }
      }
      return false;
    };
    const textOf = (list) => {
      for (const s of list || []) {
        const el = q(s);
        const t = el && (el.getAttribute("title") || el.textContent || "").trim();
        if (t) return t;
      }
      return "";
    };
    const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

    // The element that is "the player": one that plays, else the biggest one that has a source.
    const medias = qa("video,audio");
    const area = (m) => (m.videoWidth || 0) * (m.videoHeight || 0) + (m.duration > 0 ? 1 : 0);
    const media =
      medias.filter((m) => !m.paused && !m.ended).sort((a, b) => area(b) - area(a))[0] ||
      medias.filter((m) => m.readyState > 0 || m.currentSrc).sort((a, b) => area(b) - area(a))[0] ||
      medias[0] ||
      null;

    const session = (() => {
      try {
        const md = navigator.mediaSession && navigator.mediaSession.metadata;
        return md ? { title: md.title || "", artist: md.artist || "" } : null;
      } catch (e) {
        return null;
      }
    })();

    const nowPlaying = () => {
      const title = (session && session.title) || (site && textOf(site.title)) || document.title;
      const artist = (session && session.artist) || (site && textOf(site.artist)) || "";
      return { title: title.trim(), artist: artist.trim() };
    };
    const isPlaying = () => {
      if (site && site.playingBy) {
        const v = site.playingBy(document);
        if (v !== null && v !== undefined) return v;
      }
      if (media) return !media.paused && !media.ended;
      try {
        if (navigator.mediaSession && navigator.mediaSession.playbackState === "playing") return true;
      } catch (e) {
        /* ignore */
      }
      return false;
    };
    const done = (ok, msg, extra) => Object.assign({ ok, msg: msg || "", site: site ? site.name : host }, extra || {});
    const after = (msg) => done(true, msg, { playing: isPlaying() });

    // A button for "next" or "previous" near the player of a site that has no adapter. Only the
    // surroundings of the player are searched, so a "Next" of a page's pagination is never pressed.
    const nearPlayer = (re) => {
      let box = media;
      // a few levels up, never as far as the whole page
      for (let i = 0; box && i < 4; i++) {
        box = box.parentElement;
        if (!box || box === document.body || box === document.documentElement) break;
        const hit = qa("button,[role='button']", box).find(
          (b) => usable(b) && re.test((b.getAttribute("aria-label") || b.getAttribute("title") || "").trim())
        );
        if (hit) return hit;
      }
      return null;
    };

    // ------------------------------------------------------------ actions
    switch (action) {
      case "info": {
        const np = nowPlaying();
        return done(true, "", {
          playing: isPlaying(),
          title: np.title,
          artist: np.artist,
          hasMedia: !!media || !!(site && site.toggle && q(site.toggle[0])),
          muted: media ? media.muted : null,
          rate: media ? media.playbackRate : null,
          t: media ? media.currentTime : null,
          d: media && Number.isFinite(media.duration) ? media.duration : null,
        });
      }

      case "play":
      case "pause":
      case "toggle": {
        const playing = isPlaying();
        if (action === "play" && playing) return after("Already playing");
        if (action === "pause" && !playing) return after("Already paused");
        // the element first: it is exact. A site that has no usable element (Spotify) uses its button.
        if (media && media.readyState > 0) {
          if (playing) {
            media.pause();
          } else {
            const p = media.play();
            if (p && p.catch) p.catch(() => {});
          }
          return done(true, "", { playing: !playing });
        }
        if (site && press(site.toggle)) return done(true, "", { playing: !playing });
        if (media) {
          const p = media.play();
          if (p && p.catch) p.catch(() => {});
          return done(true, "", { playing: true });
        }
        return done(false, "No player found on this page");
      }

      case "next":
      case "prev": {
        if (site && press(site[action])) return after("");
        if (action === "next" && site && site.nextFallback && press(site.nextFallback)) {
          return after("Went to the suggested video");
        }
        const b = nearPlayer(action === "next" ? /^(next|skip|next (track|episode|video|song))\b/i : /^(prev|previous|back|previous (track|episode|video|song))\b/i);
        if (b) {
          b.click();
          return after("");
        }
        if (action === "prev" && media) {
          // like a hardware player: "previous" starts the track again
          media.currentTime = 0;
          return after("Restarted (this site has no previous button)");
        }
        return done(false, "This site's player has no " + (action === "next" ? "next" : "previous") + " button");
      }

      case "restart":
        if (!media) return done(false, "No player found on this page");
        media.currentTime = 0;
        return after("");

      case "seek": {
        if (!media || !Number.isFinite(media.duration)) return done(false, "Cannot seek here (a live stream, or no player)");
        const to = opts.to !== undefined ? opts.to : media.currentTime + (opts.by || 0);
        media.currentTime = clamp(to, 0, media.duration);
        return after("");
      }

      case "volume": {
        if (!media) return done(false, "No player found on this page");
        if (media.muted && (opts.by || 0) > 0) media.muted = false;
        media.volume = clamp(Math.round((media.volume + (opts.by || 0) / 100) * 100) / 100, 0, 1);
        return done(true, "Volume " + Math.round(media.volume * 100) + "%", { playing: isPlaying(), volume: media.volume });
      }

      case "mute": {
        if (!media) return done(false, "No player found on this page");
        media.muted = opts.to === undefined ? !media.muted : !!opts.to;
        return done(true, media.muted ? "Muted" : "Sound on", { playing: isPlaying(), muted: media.muted });
      }

      case "speed": {
        if (!media) return done(false, "No player found on this page");
        const now = media.playbackRate || 1;
        const to = opts.to !== undefined ? opts.to : now + (opts.by || 0);
        media.playbackRate = clamp(Math.round(to * 100) / 100, 0.25, 4);
        return done(true, "Speed " + media.playbackRate + "x", { playing: isPlaying(), rate: media.playbackRate });
      }

      case "like":
        if (site && press(site.like)) return after("");
        return done(false, "No like button known for this site");

      case "captions":
        if (site && press(site.captions)) return after("");
        if (media && media.textTracks && media.textTracks.length) {
          const tracks = Array.from(media.textTracks);
          const on = tracks.some((t) => t.mode === "showing");
          tracks.forEach((t, i) => (t.mode = !on && i === 0 ? "showing" : "disabled"));
          return after(on ? "Captions off" : "Captions on");
        }
        return done(false, "No captions found");

      case "skipad":
        if (site && press(site.skipAd)) return after("Skipped");
        return done(false, "Nothing to skip right now");

      case "shuffle":
      case "repeat":
        if (site && press(site[action])) return after("");
        return done(false, "No " + action + " button known for this site");

      default:
        return done(false, "Unknown media action: " + action);
    }
  };
})(typeof self !== "undefined" ? self : globalThis);
