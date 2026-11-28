/* Flow Hub page: lists flowhub/catalog.json and installs a flow through the CursorPie Bridge
   extension when it is there, or downloads the file when it is not.

   The talk with the extension (flowhub-page.js, a content script on this page only):
     here -> ext  { cursorpie: "flowhub", v: 1, id, type: "ping" | "install", author, flow }
     ext -> here  { cursorpie: "bridge",  v: 1, id, type: "pong" | "install_result", ... }
   `pong` says whether the extension is there at all (it answered), whether the app runs (`app`) and
   whether it would take a flow now (`install`). Anything else falls back to a plain download. */
(function () {
    "use strict";

    var $ = function (s, r) {
        return (r || document).querySelector(s);
    };
    var el = function (tag, cls, text) {
        var n = document.createElement(tag);
        if (cls) n.className = cls;
        if (text != null) n.textContent = text;
        return n;
    };
    /* The icons of the site: names of the Phosphor set (src/ui/symbols.rs) and their code in the
     CursorPieIcons font that site.css loads. */
    var GLYPHS = {
        "note-pencil": "e34c",
        "clipboard-text": "e198",
        "arrows-clockwise": "e094",
        "app-window": "e5da",
        clock: "e19a",
        browser: "e0f4",
        headphones: "e2a6",
        "sliders-horizontal": "e434",
        "envelope-simple": "e218",
        "terminal-window": "eae8",
        lightning: "e2de",
        keyboard: "e2d8",
        link: "e2e2",
        "folder-open": "e256",
        bell: "e0ce",
        globe: "e288",
        scissors: "eae0",
        copy: "e1ca",
        play: "e3d0",
        "music-notes": "e340",
        code: "e1bc",
        "rocket-launch": "e3fe",
        star: "e46a",
        sparkle: "e6a2",
        warning: "e4e0",
        check: "e182",
        "download-simple": "e20c",
        "caret-right": "e13a",
        "shield-check": "e40c",
        "arrow-square-out": "e5de",
        "chart-pie-slice": "e15a",
    };
    /* An <i class="icon"> with the glyph of `name` (never text from the catalog). */
    function icon(name) {
        var i = el("i", "icon");
        i.setAttribute("aria-hidden", "true");
        i.textContent = String.fromCodePoint(
            parseInt(GLYPHS[name] || GLYPHS["chart-pie-slice"], 16),
        );
        return i;
    }
    /* Replaces the content of `node` with an icon and a label. */
    function withIcon(node, name, label) {
        node.textContent = "";
        node.appendChild(icon(name));
        node.appendChild(document.createTextNode(label));
    }
    var OS_NAMES = { macos: "macOS", windows: "Windows", linux: "Linux" };
    var NEED_TEXT = {
        extension: "needs the browser extension",
        panels: "uses panels",
    };
    var WARN_TEXT = {
        commands: "runs shell commands",
        files: "changes files",
        messages: "sends messages",
    };

    /* ---------------------------------------------------------------- theme, menu, year */
    var root = document.documentElement;
    function setTheme(t) {
        root.dataset.theme = t;
        try {
            localStorage.setItem("cursorpie-theme", t);
        } catch (e) {}
        var m = $('meta[name="theme-color"]');
        if (m) m.setAttribute("content", t === "dark" ? "#100f14" : "#fff8ec");
        document.querySelectorAll("[data-theme-toggle]").forEach(function (b) {
            var dark = t === "dark";
            b.innerHTML =
                '<span class="icon" aria-hidden="true">' +
                (dark ? "&#xe472;" : "&#xe330;") +
                "</span>";
            b.setAttribute(
                "aria-label",
                dark ? "Switch to light theme" : "Switch to dark theme",
            );
            b.setAttribute("data-tip", dark ? "Light theme" : "Dark theme");
        });
    }
    setTheme(root.dataset.theme === "dark" ? "dark" : "light");
    document.querySelectorAll("[data-theme-toggle]").forEach(function (b) {
        b.addEventListener("click", function () {
            setTheme(root.dataset.theme === "dark" ? "light" : "dark");
        });
    });
    var topbar = $("#topbar");
    var menuBtn = $("#menu-btn");
    if (menuBtn) {
        menuBtn.addEventListener("click", function () {
            var open = menuBtn.getAttribute("aria-expanded") !== "true";
            topbar.classList.toggle("menu-open", open);
            menuBtn.setAttribute("aria-expanded", String(open));
            menuBtn.setAttribute(
                "aria-label",
                open ? "Close menu" : "Open menu",
            );
        });
    }
    document.querySelectorAll("[data-year]").forEach(function (n) {
        n.textContent = new Date().getFullYear();
    });

    /* ---------------------------------------------------------------- the bridge */
    var bridge = { ext: false, app: false, install: false, reason: "" };
    var pending = {};
    var seq = 0;

    window.addEventListener("message", function (e) {
        if (e.source !== window || e.origin !== location.origin) return;
        var m = e.data;
        if (!m || m.cursorpie !== "bridge" || m.v !== 1) return;
        var p = pending[m.id];
        if (p) {
            delete pending[m.id];
            clearTimeout(p.timer);
            p.done(m);
        }
    });

    /* Sends one message to the extension; calls back with its answer, or with null after `ms`. */
    function ask(message, ms) {
        return new Promise(function (resolve) {
            var id = ++seq;
            var timer = setTimeout(function () {
                delete pending[id];
                resolve(null);
            }, ms);
            pending[id] = { timer: timer, done: resolve };
            window.postMessage(
                Object.assign({ cursorpie: "flowhub", v: 1, id: id }, message),
                location.origin,
            );
        });
    }

    function refresh() {
        return ask({ type: "ping" }, 2500).then(function (r) {
            bridge.ext = !!r;
            bridge.app = !!(r && r.app);
            bridge.install = !!(r && r.install);
            bridge.reason = (r && r.reason) || "";
            paintStatus();
            paintButtons();
        });
    }

    function paintStatus() {
        var box = $("#hub-status");
        var text = $("#hub-status-text");
        var help = $("#hub-status-help");
        var state,
            msg,
            showHelp = false;
        if (bridge.install) {
            state = "ready";
            msg = "CursorPie is connected: Install adds the flow for you.";
        } else if (bridge.ext && bridge.app) {
            state = "partial";
            msg =
                "CursorPie is running but installs are off, so you get a download.";
            showHelp = true;
        } else if (bridge.ext) {
            state = "partial";
            msg =
                "Extension found, but CursorPie is not running. Open it for one-tap installs; for now you get a download.";
        } else {
            state = "none";
            msg = "Extension not found, so buttons download the file.";
            showHelp = true;
        }
        box.dataset.state = state;
        text.textContent = msg;
        help.hidden = !showHelp;
    }

    /* ---------------------------------------------------------------- data */
    var catalog = null;
    var tag = "";
    var cards = {}; // slug -> { card, btn, flow }
    var installed = {}; // slug -> true for this visit

    function fileUrl(f) {
        return new URL(f.file, location.href).href;
    }
    function fileName(f) {
        return f.file.split("/").pop();
    }

    function toast(text, kind, action) {
        var box = $("#hub-toasts");
        var t = el("div", "toast" + (kind ? " " + kind : ""), text);
        if (action) {
            var b = el("button", null, action.label);
            b.type = "button";
            b.addEventListener("click", function () {
                action.run();
                t.remove();
            });
            t.appendChild(b);
        }
        box.appendChild(t);
        setTimeout(
            function () {
                t.remove();
            },
            action ? 12000 : 6500,
        );
    }

    function download(f) {
        var a = document.createElement("a");
        a.href = fileUrl(f);
        a.download = fileName(f);
        document.body.appendChild(a);
        a.click();
        a.remove();
    }

    function labelOf() {
        return bridge.install
            ? ["lightning", "Install"]
            : ["download-simple", "Download"];
    }
    function paintMain(node) {
        var l = labelOf();
        withIcon(node, l[0], l[1]);
    }

    function paintButtons() {
        Object.keys(cards).forEach(function (slug) {
            var c = cards[slug];
            if (c.busy) return;
            paintMain(c.btn);
            c.btn.title = bridge.install
                ? "CursorPie adds it for you"
                : "Saves the .json file; import it in Settings > Flow";
        });
        var dlg = $("#hub-dialog");
        var main = dlg && dlg.querySelector("[data-main]");
        if (main) paintMain(main);
    }

    function setBusy(slug, busy) {
        var c = cards[slug];
        if (!c) return;
        c.busy = busy;
        c.btn.disabled = busy;
        if (busy) c.btn.textContent = "Waiting for CursorPie…";
        else paintButtons();
    }

    function act(f) {
        if (!bridge.install) {
            download(f);
            toast(
                "Saved " + fileName(f) + ". Import it in Settings > Flow.",
                "good",
            );
            return;
        }
        var c = cards[f.slug];
        if (c && c.busy) return;
        setBusy(f.slug, true);
        toast("Check CursorPie: it will ask you to confirm.");
        fetch(fileUrl(f), { cache: "no-cache" })
            .then(function (r) {
                if (!r.ok) throw new Error("HTTP " + r.status);
                return r.text();
            })
            .then(function (text) {
                return ask(
                    { type: "install", author: f.author, flow: text },
                    140000,
                );
            })
            .then(function (r) {
                setBusy(f.slug, false);
                var state = r ? r.state : "timeout";
                var msg = (r && r.message) || "CursorPie did not answer.";
                if (state === "installed" || state === "in_settings") {
                    installed[f.slug] = true;
                    if (cards[f.slug])
                        cards[f.slug].card.classList.add("is-in");
                    toast(msg, "good");
                } else if (state === "declined") {
                    toast(msg);
                } else {
                    // everything else: say why, and keep the plain download one tap away
                    toast(msg, "bad", {
                        label: "Download instead",
                        run: function () {
                            download(f);
                        },
                    });
                    if (
                        state === "offline" ||
                        state === "timeout" ||
                        state === "disabled"
                    )
                        refresh();
                }
            })
            .catch(function () {
                setBusy(f.slug, false);
                toast("Could not load the flow file.", "bad", {
                    label: "Open it",
                    run: function () {
                        window.open(fileUrl(f), "_blank", "noopener");
                    },
                });
            });
    }

    /* ---------------------------------------------------------------- cards */
    function badges(f) {
        var box = el("div", "fcard-badges");
        (f.os || []).forEach(function (o) {
            box.appendChild(el("span", "badge", OS_NAMES[o] || o));
        });
        (f.needs || []).forEach(function (n) {
            box.appendChild(el("span", "badge need", NEED_TEXT[n] || n));
        });
        (f.warns || []).forEach(function (w) {
            var b = el("span", "badge warn");
            b.appendChild(icon("warning"));
            b.appendChild(document.createTextNode(WARN_TEXT[w] || w));
            box.appendChild(b);
        });
        if (!(f.warns || []).length) {
            var ok = el("span", "badge ok");
            ok.appendChild(icon("shield-check"));
            ok.appendChild(document.createTextNode("no commands"));
            box.appendChild(ok);
        }
        return box;
    }

    function author(f) {
        var a = (catalog.authors || {})[f.author] || { name: f.author };
        var p = el("p", "fcard-by");
        p.appendChild(document.createTextNode("by "));
        var b = el("b", null, a.name || f.author);
        p.appendChild(b);
        p.appendChild(document.createTextNode(" · " + f.blocks + " blocks"));
        return p;
    }

    function build(f) {
        var card = el("article", "fcard");
        card.dataset.color = f.color || "lime";
        card.id = "flow-" + f.slug;
        var top = el("div", "fcard-top");
        var tile = el("div", "fcard-icon");
        tile.appendChild(icon(f.icon));
        top.appendChild(tile);
        var head = el("div");
        head.appendChild(el("h3", null, f.title));
        head.appendChild(author(f));
        top.appendChild(head);
        var done = el("span", "fcard-in");
        done.appendChild(icon("check"));
        done.appendChild(document.createTextNode("installed"));
        card.appendChild(done);
        card.appendChild(top);
        card.appendChild(el("p", "tag", f.tagline));
        card.appendChild(badges(f));
        var actions = el("div", "fcard-actions");
        var btn = el("button", "btn btn-install");
        paintMain(btn);
        btn.type = "button";
        btn.addEventListener("click", function () {
            act(f);
        });
        var more = el("button", "btn btn-sm fcard-more");
        withIcon(more, "caret-right", "Details");
        more.type = "button";
        more.addEventListener("click", function () {
            openDetails(f);
        });
        actions.appendChild(btn);
        actions.appendChild(more);
        card.appendChild(actions);
        cards[f.slug] = { card: card, btn: btn, flow: f, busy: false };
        return card;
    }

    /* ---------------------------------------------------------------- details */
    function list(tagName, items) {
        var l = el(tagName);
        items.forEach(function (i) {
            l.appendChild(el("li", null, i));
        });
        return l;
    }

    function openDetails(f) {
        var body = $("#hd-body");
        body.textContent = "";
        var tile = el("div", "fcard-icon hd-icon");
        tile.dataset.color = f.color || "lime";
        tile.appendChild(icon(f.icon));
        body.appendChild(tile);
        var h = el("h2", null, f.title);
        h.id = "hd-title";
        body.appendChild(h);
        body.appendChild(el("p", "tag", f.tagline));
        body.appendChild(badges(f));
        body.appendChild(el("h4", null, "What it does"));
        body.appendChild(list("ol", f.steps || []));
        var needs = (f.needs || []).map(function (n) {
            return NEED_TEXT[n] || n;
        });
        if (needs.length) {
            body.appendChild(el("h4", null, "Needs"));
            body.appendChild(list("ul", needs));
        }
        if (f.notes) {
            body.appendChild(el("h4", null, "Good to know"));
            body.appendChild(el("p", null, f.notes));
        }
        var warns = (f.warns || []).map(function (w) {
            return WARN_TEXT[w] || w;
        });
        if (warns.length) {
            var call = el("div", "hd-callout");
            call.appendChild(icon("warning"));
            call.appendChild(
                document.createTextNode(
                    "This flow " +
                        warns.join(" and ") +
                        ". Read its blocks before you use it. CursorPie asks you again before it adds it.",
                ),
            );
            body.appendChild(call);
        }
        var actions = el("div", "hd-actions");
        var main = el("button", "btn btn-lime");
        paintMain(main);
        main.type = "button";
        main.setAttribute("data-main", "");
        main.addEventListener("click", function () {
            act(f);
        });
        var dl = el("button", "btn");
        withIcon(dl, "download-simple", "Download .json");
        dl.type = "button";
        dl.addEventListener("click", function () {
            download(f);
        });
        var raw = el("a", "btn");
        withIcon(raw, "arrow-square-out", "View file");
        raw.href = fileUrl(f);
        raw.target = "_blank";
        raw.rel = "noopener";
        actions.appendChild(main);
        actions.appendChild(dl);
        actions.appendChild(raw);
        body.appendChild(actions);
        body.appendChild(
            el("p", "hd-meta", fileName(f) + " · added " + (f.added || "")),
        );
        var dlg = $("#hub-dialog");
        if (!dlg.open) {
            if (dlg.showModal) dlg.showModal();
            else dlg.setAttribute("open", "");
        }
        try {
            history.replaceState(null, "", "#" + f.slug);
        } catch (e) {}
    }

    $("#hub-dialog").addEventListener("close", function () {
        try {
            history.replaceState(null, "", location.pathname + location.search);
        } catch (e) {}
    });
    $("#hub-dialog").addEventListener("click", function (e) {
        if (e.target === this) this.close();
    });

    /* ---------------------------------------------------------------- filters */
    function matches(f) {
        var q = $("#hub-q").value.trim().toLowerCase();
        var os = $("#hub-os").value;
        if (tag && (f.tags || []).indexOf(tag) < 0) return false;
        if (os && (f.os || []).indexOf(os) < 0) return false;
        if (!q) return true;
        var hay = [
            f.title,
            f.tagline,
            (f.tags || []).join(" "),
            (f.steps || []).join(" "),
            f.author,
        ]
            .join(" ")
            .toLowerCase();
        return q.split(/\s+/).every(function (w) {
            return hay.indexOf(w) >= 0;
        });
    }

    function apply() {
        var shown = 0;
        catalog.flows.forEach(function (f) {
            var ok = matches(f);
            cards[f.slug].card.hidden = !ok;
            if (ok) shown++;
        });
        $("#hub-empty").hidden = shown > 0;
        $("#hub-count").textContent =
            shown + (shown === 1 ? " flow" : " flows");
    }

    function chips() {
        var all = {};
        catalog.flows.forEach(function (f) {
            (f.tags || []).forEach(function (t) {
                all[t] = (all[t] || 0) + 1;
            });
        });
        var box = $("#hub-chips");
        ["", ...Object.keys(all).sort()].forEach(function (t) {
            var b = el("button", "hub-chip", t || "All");
            b.type = "button";
            b.setAttribute("aria-pressed", String(t === tag));
            b.addEventListener("click", function () {
                tag = t;
                box.querySelectorAll(".hub-chip").forEach(function (x) {
                    x.setAttribute("aria-pressed", String(x === b));
                });
                apply();
            });
            box.appendChild(b);
        });
    }

    /* ---------------------------------------------------------------- start */
    function start(data) {
        catalog = data;
        var grid = $("#hub-grid");
        data.flows.forEach(function (f) {
            grid.appendChild(build(f));
        });
        chips();
        apply();
        $("#hub-q").addEventListener("input", apply);
        $("#hub-os").addEventListener("change", apply);
        var slug = decodeURIComponent((location.hash || "").slice(1));
        var hit = data.flows.filter(function (f) {
            return f.slug === slug;
        })[0];
        if (hit) openDetails(hit);
    }

    fetch("catalog.json", { cache: "no-cache" })
        .then(function (r) {
            if (!r.ok) throw new Error("HTTP " + r.status);
            return r.json();
        })
        .then(start)
        .catch(function () {
            $("#hub-count").textContent = "";
            var e = $("#hub-empty");
            e.hidden = false;
            e.textContent =
                "Could not load the flow list. Try again in a moment.";
        });

    // the content script marks the page before any script runs: show "checking" only when it is there
    function boot() {
        if (document.documentElement.hasAttribute("data-cursorpie-bridge"))
            return refresh();
        // the content script may still be injecting: give it a moment, then settle on "not found"
        // without waiting for a ping nobody can answer
        var done = false;
        window.addEventListener(
            "cursorpie-bridge-ready",
            function () {
                if (!done) {
                    done = true;
                    refresh();
                }
            },
            { once: true },
        );
        setTimeout(function () {
            if (done) return;
            done = true;
            if (document.documentElement.hasAttribute("data-cursorpie-bridge"))
                refresh();
            else {
                paintStatus();
                paintButtons();
            }
        }, 700);
    }
    boot();
    // CursorPie may be started or closed while the page is open
    function again() {
        if (
            !document.hidden &&
            document.documentElement.hasAttribute("data-cursorpie-bridge")
        )
            refresh();
    }
    document.addEventListener("visibilitychange", again);
    setInterval(again, 20000);
})();
