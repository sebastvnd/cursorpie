# CursorPie Website

This is a root-level, Markdown-driven website for the CursorPie repository.

## Files

- `index.html` — the landing page and documentation viewer in one file.
- `assets/site.css` — shared styling and responsive layout.
- `assets/site.js` — interactions, OS detection, download links, and the Markdown documentation engine.
- `assets/js/marked.js` — the local Markdown parser used to render project `.md` files.
- `assets/fonts/` — the project's Inter and Phosphor icon fonts, with their license files.
- `assets/cursorpie-logo.png` and `assets/cursorpie-mark.png` — CursorPie logo assets.

## How the documentation engine works

The site does not contain converted copies of the documentation. It reads the Markdown files already present in the repository and renders them in the browser:

- `docs/README.md`
- `docs/INSTALL.md`
- `docs/USER_GUIDE.md`
- `docs/CONFIGURATION.md`
- `docs/KNOWN_ISSUES.md`
- `docs/DEBUGGING.md`
- `docs/LINUX_MIGRATION.md`
- `docs/WINDOWS_MIGRATION.md`
- `docs/OTHER_DESKTOPS.md`
- `docs/UI_DESIGN.md`
- `docs/TRACKPAD_HAPTICS_PLAN.md`
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
│   ├── INSTALL.md
│   ├── USER_GUIDE.md
│   └── ...existing documentation files...
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
