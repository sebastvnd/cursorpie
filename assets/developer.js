/* The developer page (developer/) and the public Kanban board (developer/kanban/). The board is
   read from kanban/BOARD.md, which `cargo tools kanban` generates from the cards. Everything read
   from those files is put on the page as text, never as HTML. */
(function () {
    "use strict";

    var $ = function (s, r) {
        return (r || document).querySelector(s);
    };
    var all = function (s, r) {
        return Array.prototype.slice.call((r || document).querySelectorAll(s));
    };
    var el = function (tag, cls, text) {
        var n = document.createElement(tag);
        if (cls) n.className = cls;
        if (text != null) n.textContent = text;
        return n;
    };

    /* ---------------------------------------------------------------- theme, menu, year */
    var root = document.documentElement;
    function setTheme(t) {
        root.dataset.theme = t;
        var m = $('meta[name="theme-color"]');
        if (m) m.setAttribute("content", t === "dark" ? "#100f14" : "#fff8ec");
        all("[data-theme-toggle]").forEach(function (b) {
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
    all("[data-theme-toggle]").forEach(function (b) {
        b.addEventListener("click", function () {
            var t = root.dataset.theme === "dark" ? "light" : "dark";
            setTheme(t);
            try {
                localStorage.setItem("cursorpie-theme", t);
            } catch (e) {}
        });
    });
    var menuBtn = $("#menu-btn");
    if (menuBtn) {
        menuBtn.addEventListener("click", function () {
            var open = menuBtn.getAttribute("aria-expanded") !== "true";
            $("#topbar").classList.toggle("menu-open", open);
            menuBtn.setAttribute("aria-expanded", String(open));
            menuBtn.setAttribute(
                "aria-label",
                open ? "Close menu" : "Open menu",
            );
        });
    }
    all("[data-year]").forEach(function (n) {
        n.textContent = new Date().getFullYear();
    });

    /* ---------------------------------------------------------------- developer page */
    all("[data-copy]").forEach(function (b) {
        b.addEventListener("click", function () {
            var done = function (label) {
                b.textContent = label;
                $("#dev-copied").textContent = label;
                setTimeout(function () {
                    b.textContent = "Copy";
                }, 1600);
            };
            if (!navigator.clipboard) {
                done("Select it");
                return;
            }
            navigator.clipboard.writeText(b.getAttribute("data-copy")).then(
                function () {
                    done("Copied");
                },
                function () {
                    done("Select it");
                },
            );
        });
    });
    var devSearch = $("#dev-q");
    if (devSearch) {
        devSearch.addEventListener("input", function () {
            var q = devSearch.value.trim().toLowerCase();
            var shown = 0;
            all(".dev-group").forEach(function (group) {
                var title = $("h2", group).textContent.toLowerCase();
                var inGroup = 0;
                all(".dev-grid li", group).forEach(function (li) {
                    var match =
                        (title + " " + li.textContent.toLowerCase()).indexOf(
                            q,
                        ) !== -1;
                    li.hidden = !match;
                    if (match) inGroup++;
                });
                group.hidden = !inGroup;
                shown += inGroup;
            });
            $("#dev-empty").hidden = shown > 0;
        });
    }

    /* ---------------------------------------------------------------- Kanban board */
    var board = $("#kb-board");
    if (!board) return;
    var FILES = "../../kanban/";
    var SOURCE = "https://github.com/sebastvnd/cursorpie/blob/main/kanban/";
    var TONES = ["sky", "orange", "lime", "violet"];
    var status = $("#kb-status");
    var columns = [];

    function load(path) {
        return fetch(FILES + path, { cache: "no-cache" }).then(function (r) {
            if (!r.ok) throw new Error(String(r.status));
            return r.text();
        });
    }

    /* The columns are the `## ` headings of BOARD.md, the cards the rows of their tables. A `|` in a
     title is written `\|`. */
    function parseBoard(text) {
        var out = [];
        text.split(/\r?\n/).forEach(function (line) {
            if (line.indexOf("## ") === 0) {
                out.push({ name: line.slice(3).trim(), cards: [] });
                return;
            }
            var link = /^\| \[(\d+)\]\(cards\/(\d+\.md)\) \|(.*)\|\s*$/.exec(
                line,
            );
            if (!link || !out.length) return;
            var cells = link[3]
                .replace(/\\\|/g, "\u0000")
                .split("|")
                .map(function (c) {
                    return c.replace(/\u0000/g, "|").trim();
                });
            out[out.length - 1].cards.push({
                id: link[1],
                file: link[2],
                title: cells[0],
                owner: cells[1] === "-" ? "" : cells[1],
                updated: cells[2],
                notes: Number(cells[3]) || 0,
            });
        });
        return out;
    }

    function openCard(card, column) {
        var body = $("#kb-body");
        $("#kb-title").textContent = card.title;
        $("#kb-meta").textContent = [
            "#" + card.id,
            column,
            card.owner || "No owner",
            "updated " + card.updated,
        ].join(" · ");
        $("#kb-source").href = SOURCE + "cards/" + card.file;
        body.textContent = "Loading…";
        body.dataset.card = card.file;
        $("#kb-dialog").showModal();
        load("cards/" + card.file)
            .then(function (text) {
                if (body.dataset.card !== card.file) return;
                var rest = text
                    .replace(/\r\n/g, "\n")
                    .replace(/^---\n[\s\S]*?\n---\n?/, "")
                    .trim();
                body.textContent = "";
                body.appendChild(el("div", "kb-text", rest || "No notes yet."));
            })
            .catch(function () {
                if (body.dataset.card === card.file)
                    body.textContent = "The notes could not be loaded.";
            });
    }

    function draw() {
        var q = $("#kb-q").value.trim().toLowerCase();
        var shown = 0;
        var total = 0;
        board.textContent = "";
        columns.forEach(function (column, index) {
            var cards = column.cards.filter(function (c) {
                return (
                    ("#" + c.id + " " + c.title + " " + c.owner)
                        .toLowerCase()
                        .indexOf(q) !== -1
                );
            });
            shown += cards.length;
            total += column.cards.length;
            var section = el("section", "kb-column");
            section.dataset.tone = TONES[index % TONES.length];
            var head = el("h2", null, column.name);
            head.appendChild(el("span", "kb-count", String(cards.length)));
            section.appendChild(head);
            var list = el("ul");
            cards.forEach(function (card) {
                var button = el("button", "kb-card");
                button.type = "button";
                button.appendChild(el("span", "kb-id", "#" + card.id));
                button.appendChild(el("b", null, card.title));
                button.appendChild(
                    el(
                        "span",
                        "kb-meta",
                        (card.owner || "No owner") +
                            " · " +
                            card.updated +
                            (card.notes
                                ? " · " +
                                  card.notes +
                                  (card.notes === 1 ? " note" : " notes")
                                : ""),
                    ),
                );
                button.addEventListener("click", function () {
                    openCard(card, column.name);
                });
                var li = el("li");
                li.appendChild(button);
                list.appendChild(li);
            });
            section.appendChild(list);
            if (!cards.length)
                section.appendChild(
                    el("p", "kb-none", q ? "No match" : "No cards"),
                );
            board.appendChild(section);
        });
        status.textContent = q
            ? shown + " of " + total + " cards"
            : total + (total === 1 ? " card" : " cards");
    }

    load("BOARD.md")
        .then(function (text) {
            columns = parseBoard(text);
            if (!columns.length) throw new Error("empty");
            draw();
            $("#kb-q").addEventListener("input", draw);
        })
        .catch(function () {
            status.textContent = "The board could not be loaded. ";
            var a = el("a", null, "Read it on GitHub");
            a.href = SOURCE + "BOARD.md";
            status.appendChild(a);
        });
})();
