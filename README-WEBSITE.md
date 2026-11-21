# CursorPie Website

This is a root-level, Markdown-driven website for the CursorPie repository.

## Files

- `index.html` — the one-page landing site and the documentation viewer in one file. The landing page is a single scroll (hero, marquee, stats, how it works, features, privacy, download, FAQ). Opening `?doc=...` swaps it for the documentation view without reloading the page.
- `assets/site.css` — design tokens (light and dark), the Gen Z look (neon palette, thick outlines, hard shadows), animations, documentation layout and responsive rules. Motion is switched off for `prefers-reduced-motion`.
- `assets/site.js` — theme, scroll animations, the interactive pie demo, OS detection and download links, the in-page router and the Markdown documentation engine.
- `assets/js/marked.js` — the local Markdown parser used to render project `.md` files.
- `assets/fonts/` — the project's Inter and Phosphor icon fonts, with their license files.
- `assets/cursorpie-logo.png` and `assets/cursorpie-mark.png` — CursorPie logo assets.
- `docs/gifs/` — the 720 x 405 demo animations. The README and the user guide link to them with relative paths (the viewer resolves them against the page's own file), and the landing page shows all of them in the *See it in motion* section (`#tour`), loaded lazily as they scroll into view. The workflow copies `docs/` into the site, so nothing else is needed to publish them. See `docs/gifs/README.md`.

## Documentation viewer features

- Sidebar built from the `docGroups` list at the top of the documentation section of `assets/site.js` (grouped, collapsible, filterable). Press `/` or `Ctrl/Cmd+K` to search page titles.
- "On this page" table of contents with scroll tracking, breadcrumbs, reading time, an *Edit on GitHub* link, and previous/next page links.
- Code blocks get a language label and a Copy button; wide tables scroll inside their own frame; blockquotes starting with **Note**, **Tip** or **Warning** become callouts.
- Internal links never reload the page, and the browser back button works. On phones the sidebar becomes a drawer.
- To add a documentation page, add it to `docGroups` (it is then listed in the sidebar and the pager) and keep the file path inside `docs/`, `docs/user/`, `docs/developer/`, `docs/examples/` or `diag/`.

## How the documentation engine works

The site does not contain converted copies of the documentation. It reads the Markdown files already present in the repository and renders them in the browser:

- `docs/README.md`
- `docs/user/getting-started.md`
- `docs/user/install.md`
- `docs/user/README.md`
- `docs/user/using-the-pie.md`
- `docs/user/profiles-and-buttons.md`
- `docs/user/trackpad.md`
- `docs/user/flows.md`
- `docs/user/settings.md`
- `docs/user/themes.md`
- `docs/user/linux.md`
- `docs/user/updates.md`
- `docs/user/configuration.md`
- `docs/user/known-issues.md`
- `docs/FLOWS_FOR_AI.md`
- `docs/examples/README.md`
- `docs/PANELS.md`
- `docs/PANEL_DESIGN_FOR_AI.md`
- `docs/BROWSER_EXTENSION.md`
- `docs/DEBUGGING.md`
- `docs/developer/README.md`
- `docs/developer/LINUX_MIGRATION.md`
- `docs/developer/WAYLAND_SUPPORT.md`
- `docs/developer/WINDOWS_MIGRATION.md`
- `docs/developer/OTHER_DESKTOPS.md`
- `docs/developer/UI_DESIGN.md`
- `docs/developer/TRACKPAD_HAPTICS_PLAN.md`
- `docs/developer/BROWSER_BRIDGE.md`
- `docs/developer/UPDATES_TESTING.md`
- `docs/developer/OPEN_URL_CHOICE.md`
- `CONTRIBUTING.md`
- `CHANGELOG.md`
- `README.md`
- `diag/README.md`

Markdown links to other `.md` files are converted into website documentation links automatically. Relative images and other local resources are resolved from the directory of the source Markdown file.

## Installation in the CursorPie repository

1. Put `index.html` in the repository root, alongside `README.md`, `CHANGELOG.md`, and the `docs/` directory.
2. Copy the supplied `assets/` directory beside `index.html`.
3. Keep the original Markdown files and their existing paths. Do not convert or move them.
4. Publish the repository root with GitHub Pages (or another static web host).

The website expects this layout:

```text
cursorpie/
├── index.html
├── README.md
├── CHANGELOG.md
├── CONTRIBUTING.md
├── docs/
│   ├── README.md
│   ├── user/            (user guide, one .md per page)
│   ├── developer/       (design records)
│   └── ...reference files (FLOWS_FOR_AI.md, PANELS.md, ...)
├── diag/
│   └── README.md
└── assets/
    ├── site.css
    ├── site.js
    ├── js/marked.js
    ├── fonts/
    ├── cursorpie-logo.png
    └── cursorpie-mark.png
```

## Local preview

Browsers commonly block `fetch()` from local `file://` pages. Start a small HTTP server from the repository root instead:

```sh
python3 -m http.server 8000
```

Then open <http://localhost:8000>.

## Updating documentation

Edit the existing `.md` file in `docs/` (or the relevant root-level Markdown file) and publish the changes. The website reads the file when a documentation page is opened, so there is no HTML conversion or duplicate documentation to maintain.

## Search engine optimization (SEO)

- `index.html` carries the title, description, canonical URL, Open Graph/Twitter tags and JSON-LD (`SoftwareApplication`).
- `assets/site.js` updates the title, canonical URL and description for every `?doc=...` page.
- `robots.txt` and `sitemap.xml` live in the repository root and are copied to the site by `.github/workflows/static.yml`.
- `assets/og-image.png` (1200x630) is the preview image for links shared on social networks and chat apps.
- The site URL `https://sebastvnd.github.io/cursorpie/` is written in `index.html`, `robots.txt` and `sitemap.xml`. If the address changes (for example a custom domain), replace it in those three files.
- Google Search Console: either upload the `googleXXXXXXXX.html` file it gives you to the repository root (the workflow copies `google*.html`), or use the commented `google-site-verification` meta tag in `index.html`.
