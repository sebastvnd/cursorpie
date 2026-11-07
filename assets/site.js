(function () {
  'use strict';

  const root = document.documentElement;
  const repo = 'https://github.com/sebastvnd/cursorpie';
  const release = repo + '/releases/latest/download/';

  // ---------------------------------------------------------------
  // Shared site controls
  // ---------------------------------------------------------------
  const themeStorageKey = 'cursorpie-theme';

  function readSavedTheme() {
    try {
      const saved = window.localStorage.getItem(themeStorageKey);
      return saved === 'light' || saved === 'dark' ? saved : null;
    } catch (_) {
      return null;
    }
  }

  function applyTheme(theme, persist) {
    if (theme !== 'light' && theme !== 'dark') return;

    root.dataset.theme = theme;

    document.querySelectorAll('[data-theme-toggle]').forEach((button) => {
      const dark = theme === 'dark';
      button.innerHTML = dark
        ? '<span class="icon" aria-hidden="true">&#xe472;</span>'
        : '<span class="icon" aria-hidden="true">&#xe330;</span>';
      button.setAttribute('aria-label', dark ? 'Switch to light theme' : 'Switch to dark theme');
      button.setAttribute('data-tip', dark ? 'Light theme' : 'Dark theme');
    });

    const themeColor = document.querySelector('meta[name="theme-color"]');
    if (themeColor) {
      themeColor.setAttribute('content', theme === 'dark' ? '#202922' : '#f5f3ec');
    }

    if (persist) {
      try {
        window.localStorage.setItem(themeStorageKey, theme);
      } catch (_) {
        // The theme still applies for this page when storage is unavailable.
      }
    }
  }

  const savedTheme = readSavedTheme();
  const initialTheme = savedTheme ||
    (root.dataset.theme === 'light' || root.dataset.theme === 'dark'
      ? root.dataset.theme
      : (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'));

  applyTheme(initialTheme, false);

  // Switch the palette in one frame. While `theme-switching` is set, CSS turns
  // off every transition, so nothing fades at a different moment than the rest.
  // Where supported, the View Transitions API cross-fades the whole page.
  function switchTheme(theme) {
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    const release = () => requestAnimationFrame(() => requestAnimationFrame(() => {
      root.classList.remove('theme-switching');
    }));

    root.classList.add('theme-switching');

    if (typeof document.startViewTransition === 'function' && !reduceMotion) {
      const transition = document.startViewTransition(() => applyTheme(theme, true));
      transition.finished.then(release, release);
    } else {
      applyTheme(theme, true);
      release();
    }
  }

  document.querySelectorAll('[data-theme-toggle]').forEach((button) => {
    button.addEventListener('click', () => {
      switchTheme(root.dataset.theme === 'dark' ? 'light' : 'dark');
    });
  });

  // Keep the selected theme in sync when another tab changes it.
  window.addEventListener('storage', (event) => {
    if (event.key !== themeStorageKey) return;

    const nextTheme = event.newValue;
    applyTheme(
      nextTheme === 'light' || nextTheme === 'dark'
        ? nextTheme
        : (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'),
      false
    );
  });

  document.querySelectorAll('[data-year]').forEach((element) => {
    element.textContent = String(new Date().getFullYear());
  });

  document.querySelectorAll('[data-release]').forEach((anchor) => {
    anchor.href = release + anchor.dataset.release;
  });

  const userAgent = navigator.userAgent;
  let detectedOS = 'win';
  if (/Mac/i.test(userAgent) && !/iPhone|iPad/i.test(userAgent)) detectedOS = 'mac';
  else if (/Linux|X11/i.test(userAgent) && !/Android/i.test(userAgent)) detectedOS = 'linux';
  else if (/Android|iPhone|iPad/i.test(userAgent)) detectedOS = 'mobile';

  const platformNames = { win: 'Windows', mac: 'macOS', linux: 'Linux' };
  document.querySelectorAll('[data-os-card]').forEach((card) => {
    if (card.dataset.osCard === detectedOS) card.classList.add('recommended');
  });

  const deviceCopy = document.querySelector('[data-device-copy]');
  if (deviceCopy) {
    deviceCopy.textContent = detectedOS === 'mobile'
      ? 'CursorPie is made for desktop computers. Open this page on Windows, macOS, or Linux to download it.'
      : 'The ' + platformNames[detectedOS] + ' package is highlighted below. Choose the build that matches your system.';
  }

  const heroDownload = document.getElementById('hero-download');
  if (heroDownload) {
    if (detectedOS === 'win') {
      heroDownload.href = release + 'CursorPie-windows-setup.exe';
      heroDownload.innerHTML = 'Download for Windows <span class="icon" aria-hidden="true">&#xe5de;</span>';
    } else if (detectedOS === 'mac') {
      heroDownload.href = release + 'CursorPie-macos-universal.dmg';
      heroDownload.innerHTML = 'Download for macOS <span class="icon" aria-hidden="true">&#xe5de;</span>';
    } else if (detectedOS === 'linux') {
      heroDownload.href = '#download';
      heroDownload.innerHTML = 'Choose a Linux build <span class="icon" aria-hidden="true">&#xe13a;</span>';
    } else {
      heroDownload.href = '#download';
      heroDownload.innerHTML = 'View desktop downloads <span class="icon" aria-hidden="true">&#xe13a;</span>';
    }
  }

  // ---------------------------------------------------------------
  // Markdown documentation engine
  //
  // The website lives at the repository root. Documentation remains
  // in its original .md files under docs/, with project notes such as
  // CHANGELOG.md and CONTRIBUTING.md at the repository root.
  // ---------------------------------------------------------------
  const documentMap = [
    { file: 'docs/README.md', label: 'Documentation overview' },
    { file: 'docs/INSTALL.md', label: 'Installation' },
    { file: 'docs/USER_GUIDE.md', label: 'User guide' },
    { file: 'docs/CONFIGURATION.md', label: 'Configuration' },
    { file: 'docs/KNOWN_ISSUES.md', label: 'Known issues' },
    { file: 'docs/DEBUGGING.md', label: 'Debugging' },
    { file: 'CONTRIBUTING.md', label: 'Contributing' },
    { file: 'CHANGELOG.md', label: 'Changelog' },
    { file: 'docs/LINUX_MIGRATION.md', label: 'Linux migration' },
    { file: 'docs/WAYLAND_SUPPORT.md', label: 'Wayland support plan' },
    { file: 'docs/WINDOWS_MIGRATION.md', label: 'Windows migration' },
    { file: 'docs/OTHER_DESKTOPS.md', label: 'Other desktops' },
    { file: 'docs/UI_DESIGN.md', label: 'UI design' },
    { file: 'docs/TRACKPAD_HAPTICS_PLAN.md', label: 'Trackpad and haptics plan' },
    { file: 'README.md', label: 'Project README' },
    { file: 'diag/README.md', label: 'Diagnostic crate notes' }
  ];

  const docParam = new URLSearchParams(window.location.search).get('doc');
  const landingView = document.getElementById('landing-view');
  const documentationView = document.getElementById('documentation-view');
  const markdownContainer = document.getElementById('markdown-content');

  function normalizeDocPath(path) {
    const parts = [];
    String(path || '').replace(/\\/g, '/').split('/').forEach((part) => {
      if (!part || part === '.') return;
      if (part === '..') parts.pop();
      else parts.push(part);
    });
    return parts.join('/');
  }

  function isAllowedMarkdown(path) {
    const normalized = normalizeDocPath(path);
    return documentMap.some((item) => item.file === normalized) ||
      (/^docs\/[A-Za-z0-9._-]+\.md$/i.test(normalized)) ||
      (/^diag\/[A-Za-z0-9._-]+\.md$/i.test(normalized));
  }

  function pageRootURL() {
    return new URL('./', window.location.href);
  }

  function routeForMarkdown(path, hash) {
    const normalized = normalizeDocPath(path);
    const url = new URL('./index.html', pageRootURL());
    url.searchParams.set('doc', normalized);
    if (hash) url.hash = hash;
    return url.href;
  }

  function slugify(text) {
    return text.toLowerCase().trim()
      .replace(/[`*_~]/g, '')
      .replace(/[^\p{L}\p{N}\s-]/gu, '')
      .replace(/\s+/g, '-')
      .replace(/-+/g, '-');
  }

  function assignHeadingIds(container) {
    const used = new Map();
    container.querySelectorAll('h1, h2, h3, h4, h5, h6').forEach((heading) => {
      const base = slugify(heading.textContent) || 'section';
      const count = used.get(base) || 0;
      used.set(base, count + 1);
      heading.id = count ? base + '-' + count : base;
    });
  }

  function sanitizeAndResolve(container, markdownPath) {
    const forbidden = container.querySelectorAll('script, iframe, object, embed, form');
    forbidden.forEach((element) => element.remove());

    const markdownURL = new URL(markdownPath, pageRootURL());
    const siteRoot = pageRootURL();

    container.querySelectorAll('*').forEach((element) => {
      Array.from(element.attributes).forEach((attribute) => {
        const name = attribute.name.toLowerCase();
        const value = attribute.value.trim();
        if (name.startsWith('on')) element.removeAttribute(attribute.name);
        if ((name === 'href' || name === 'src' || name === 'xlink:href') && /^\s*javascript:/i.test(value)) {
          element.removeAttribute(attribute.name);
        }
      });
    });

    container.querySelectorAll('a[href]').forEach((anchor) => {
      const raw = anchor.getAttribute('href');
      if (!raw || raw.startsWith('#') || /^(mailto:|tel:|https?:)/i.test(raw)) {
        if (/^https?:/i.test(raw || '')) {
          anchor.target = '_blank';
          anchor.rel = 'noopener noreferrer';
        }
        return;
      }
      try {
        const target = new URL(raw, markdownURL);
        if (target.origin !== siteRoot.origin || !target.pathname.startsWith(siteRoot.pathname)) return;
        const relativePath = decodeURIComponent(target.pathname.slice(siteRoot.pathname.length));
        if (/\.md$/i.test(relativePath) && isAllowedMarkdown(relativePath)) {
          anchor.href = routeForMarkdown(relativePath, target.hash);
        } else {
          anchor.href = target.href;
        }
      } catch (_) {
        anchor.removeAttribute('href');
      }
    });

    container.querySelectorAll('img[src], source[src]').forEach((image) => {
      const raw = image.getAttribute('src');
      if (!raw || /^(data:|https?:|\/\/)/i.test(raw)) return;
      try { image.src = new URL(raw, markdownURL).href; } catch (_) { image.removeAttribute('src'); }
    });

    assignHeadingIds(container);
  }

  function showDocumentation() {
    if (!landingView || !documentationView || !markdownContainer) return;
    landingView.hidden = true;
    documentationView.hidden = false;
    document.querySelectorAll('[data-doc-link]').forEach((link) => link.setAttribute('aria-current', 'page'));
    document.querySelectorAll('[data-home-link]').forEach((link) => link.removeAttribute('aria-current'));

    const requested = normalizeDocPath(decodeURIComponent(docParam || ''));
    if (!isAllowedMarkdown(requested)) {
      renderDocumentError('That Markdown page is not part of the configured documentation sources.', requested);
      return;
    }

    const metadata = documentMap.find((item) => item.file === requested);
    const label = metadata ? metadata.label : requested.split('/').pop().replace(/\.md$/i, '').replace(/[_-]+/g, ' ');
    document.title = label + ' — CursorPie';
    updateSeoForDocument(requested, label);
    const subtitle = document.getElementById('doc-page-subtitle');
    if (subtitle) subtitle.textContent = 'Source: ' + requested + ' · Rendered from the repository Markdown file';

    document.querySelectorAll('[data-doc-file]').forEach((link) => {
      if (normalizeDocPath(link.dataset.docFile) === requested) link.setAttribute('aria-current', 'page');
      else link.removeAttribute('aria-current');
    });

    loadMarkdown(requested);
  }

  function setHeadMeta(selector, create, attr, value) {
    let element = document.head.querySelector(selector);
    if (!element) { element = create(); document.head.appendChild(element); }
    element.setAttribute(attr, value);
  }

  function updateSeoForDocument(requested, label) {
    const url = routeForMarkdown(requested);
    const description = label + ' for CursorPie, the free open-source pie menu for Windows, macOS and Linux.';
    setHeadMeta('link[rel="canonical"]', () => { const l = document.createElement('link'); l.rel = 'canonical'; return l; }, 'href', url);
    setHeadMeta('meta[name="description"]', () => { const m = document.createElement('meta'); m.name = 'description'; return m; }, 'content', description);
    setHeadMeta('meta[property="og:url"]', () => { const m = document.createElement('meta'); m.setAttribute('property', 'og:url'); return m; }, 'content', url);
    setHeadMeta('meta[property="og:title"]', () => { const m = document.createElement('meta'); m.setAttribute('property', 'og:title'); return m; }, 'content', document.title);
    setHeadMeta('meta[property="og:description"]', () => { const m = document.createElement('meta'); m.setAttribute('property', 'og:description'); return m; }, 'content', description);
  }

  function renderDocumentError(message, path) {
    if (!markdownContainer) return;
    // Never let broken documentation URLs get indexed.
    setHeadMeta('meta[name="robots"]', () => { const m = document.createElement('meta'); m.name = 'robots'; return m; }, 'content', 'noindex');
    markdownContainer.innerHTML = '<div class="doc-error"><span class="icon" aria-hidden="true">&#xe4e0;</span><h2>Could not load this document</h2><p></p><p class="doc-error-path"></p><p>Make sure the website is in the repository root and the Markdown file is available at that path.</p><p>For local testing, start a web server from the repository root, for example <code>python3 -m http.server 8000</code>.</p></div>';
    markdownContainer.querySelector('p').textContent = message;
    markdownContainer.querySelector('.doc-error-path').textContent = path || '';
  }

  async function loadMarkdown(path) {
    if (!markdownContainer) return;
    markdownContainer.innerHTML = '<div class="markdown-loading"><span class="loading-dot"></span> Loading documentation…</div>';
    try {
      const response = await fetch(new URL(path, pageRootURL()).href, { cache: 'no-cache' });
      if (!response.ok) throw new Error('The server returned HTTP ' + response.status + '.');
      const markdown = await response.text();
      if (!window.marked || typeof window.marked.parse !== 'function') {
        throw new Error('The local Markdown parser could not be loaded from assets/js/marked.js.');
      }
      const html = window.marked.parse(markdown, { gfm: true, breaks: false, mangle: false, headerIds: false });
      markdownContainer.innerHTML = html;
      sanitizeAndResolve(markdownContainer, path);
      if (window.location.hash) {
        const id = decodeURIComponent(window.location.hash.slice(1));
        const target = document.getElementById(id);
        if (target) target.scrollIntoView();
      } else {
        window.scrollTo(0, 0);
      }
    } catch (error) {
      renderDocumentError(error.message || 'An unexpected error occurred while reading the Markdown file.', path);
    }
  }

  if (docParam) showDocumentation();

  // ---------------------------------------------------------------
  // Interactive four-slice demo: mirrors the default general pie: icons only, no names.
  // Slice 0 is History, centered on the left side of the ring.
  // ---------------------------------------------------------------
  const stage = document.getElementById('pie-stage');
  const svg = document.getElementById('pie-demo');
  if (!stage || !svg) return;

  const SVG_NS = 'http://www.w3.org/2000/svg';
  const items = [
    { name: 'History', glyph: '\ue08a' },
    { name: 'Clipboard', glyph: '\ue198' },
    { name: 'Brackets', glyph: '\ue85e' },
    { name: 'Launch', glyph: '\ue3fe' }
  ];
  const outerRadius = 145;
  const innerRadius = outerRadius * 0.36;
  const gap = 0.05;
  const sweep = Math.PI * 2 / items.length;
  const slices = [];
  const icons = [];
  const point = (radius, angle) => [radius * Math.cos(angle), radius * Math.sin(angle)];

  items.forEach((item, index) => {
    const start = Math.PI - sweep / 2 + index * sweep;
    const a0 = start + gap / 2;
    const a1 = start + sweep - gap / 2;
    const p1 = point(outerRadius, a0);
    const p2 = point(outerRadius, a1);
    const p3 = point(innerRadius, a1);
    const p4 = point(innerRadius, a0);
    const path = document.createElementNS(SVG_NS, 'path');
    path.setAttribute('d', `M${p1[0]},${p1[1]} A${outerRadius},${outerRadius} 0 0 1 ${p2[0]},${p2[1]} L${p3[0]},${p3[1]} A${innerRadius},${innerRadius} 0 0 0 ${p4[0]},${p4[1]} Z`);
    path.setAttribute('class', 'pie-slice');
    svg.appendChild(path);
    slices.push(path);

    const middle = start + sweep / 2;
    const iconPoint = point((outerRadius + innerRadius) / 2, middle);
    const icon = document.createElementNS(SVG_NS, 'text');
    icon.setAttribute('x', iconPoint[0]);
    icon.setAttribute('y', iconPoint[1]);
    icon.setAttribute('class', 'pie-icon');
    icon.textContent = item.glyph;
    svg.appendChild(icon);
    icons.push(icon);
  });

  const outline = document.createElementNS(SVG_NS, 'circle');
  outline.setAttribute('r', outerRadius);
  outline.setAttribute('class', 'pie-outline');
  svg.appendChild(outline);

  const core = document.createElementNS(SVG_NS, 'circle');
  core.setAttribute('r', innerRadius * 0.86);
  core.setAttribute('class', 'pie-core');
  svg.appendChild(core);

  const coreIcon = document.createElementNS(SVG_NS, 'text');
  coreIcon.setAttribute('x', '0');
  coreIcon.setAttribute('y', '1');
  coreIcon.setAttribute('class', 'pie-core-icon');
  coreIcon.textContent = '\ue08a';
  svg.appendChild(coreIcon);

  let centerX = 0;
  let centerY = 0;
  let highlighted = -1;
  let pointerDown = false;
  const hint = document.getElementById('pie-hint');

  function setPosition(x, y) {
    const bounds = stage.getBoundingClientRect();
    const margin = outerRadius + 12;
    centerX = Math.max(margin, Math.min(bounds.width - margin, x));
    centerY = Math.max(margin, Math.min(bounds.height - margin, y));
    svg.style.left = centerX + 'px';
    svg.style.top = centerY + 'px';
    svg.style.transform = 'translate(-50%, -50%)';
  }

  function setHighlighted(index) {
    if (index === highlighted) return;
    if (highlighted >= 0) {
      slices[highlighted].classList.remove('hot');
      icons[highlighted].classList.remove('hot');
    }
    highlighted = index;
    if (highlighted >= 0) {
      slices[highlighted].classList.add('hot');
      icons[highlighted].classList.add('hot');
    }
  }

  function resetPie() {
    setPosition(stage.clientWidth / 2, stage.clientHeight * 0.48);
    stage.classList.remove('on');
    setHighlighted(-1);
  }

  function localPoint(event) {
    const bounds = stage.getBoundingClientRect();
    return [event.clientX - bounds.left, event.clientY - bounds.top];
  }

  stage.addEventListener('pointerdown', (event) => {
    if (event.button !== 0) return;
    pointerDown = true;
    stage.setPointerCapture(event.pointerId);
    const position = localPoint(event);
    setPosition(position[0], position[1]);
    stage.classList.add('on');
    setHighlighted(-1);
    if (hint) hint.innerHTML = 'Move toward a slice, then <strong>release</strong>.';
  });

  stage.addEventListener('pointermove', (event) => {
    if (!pointerDown) return;
    const position = localPoint(event);
    const dx = position[0] - centerX;
    const dy = position[1] - centerY;
    if (Math.hypot(dx, dy) < innerRadius * 0.7) {
      setHighlighted(-1);
      return;
    }
    let angle = Math.atan2(dy, dx);
    if (angle < 0) angle += Math.PI * 2;
    const adjusted = (angle - (Math.PI - sweep / 2) + Math.PI * 2) % (Math.PI * 2);
    setHighlighted(Math.floor(adjusted / sweep) % items.length);
  });

  function endGesture() {
    if (!pointerDown) return;
    pointerDown = false;
    const choice = highlighted >= 0 ? items[highlighted].name : null;
    if (hint) {
      hint.innerHTML = choice
        ? 'Selected: <strong>' + choice + '</strong>. Try another slice.'
        : 'No slice selected. Try again.';
    }
    resetPie();
  }

  stage.addEventListener('pointerup', endGesture);
  stage.addEventListener('pointercancel', endGesture);
  stage.addEventListener('contextmenu', (event) => event.preventDefault());
  window.addEventListener('resize', () => { if (!pointerDown) resetPie(); });
  resetPie();
})();
