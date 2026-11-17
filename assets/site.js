(function () {
  'use strict';

  const root = document.documentElement;
  const repo = 'https://github.com/sebastvnd/cursorpie';
  const release = repo + '/releases/latest/download/';
  const $ = (sel, ctx) => (ctx || document).querySelector(sel);
  const $$ = (sel, ctx) => Array.from((ctx || document).querySelectorAll(sel));
  // Respect the OS "Reduce motion" setting unless the visitor opted back in with ?motion=full.
  const reduceMotion = root.dataset.motion === 'full' ? false : window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const finePointer = window.matchMedia('(hover: hover) and (pointer: fine)').matches;

  // ---------------------------------------------------------------
  // Theme
  // ---------------------------------------------------------------
  const themeStorageKey = 'cursorpie-theme';
  const THEME_COLOR = { light: '#fff8ec', dark: '#100f14' };

  function readSavedTheme() {
    try {
      const saved = window.localStorage.getItem(themeStorageKey);
      return saved === 'light' || saved === 'dark' ? saved : null;
    } catch (_) { return null; }
  }

  function applyTheme(theme, persist) {
    if (theme !== 'light' && theme !== 'dark') return;
    root.dataset.theme = theme;
    $$('[data-theme-toggle]').forEach((button) => {
      const dark = theme === 'dark';
      button.innerHTML = dark
        ? '<span class="icon" aria-hidden="true">&#xe472;</span>'
        : '<span class="icon" aria-hidden="true">&#xe330;</span>';
      button.setAttribute('aria-label', dark ? 'Switch to light theme' : 'Switch to dark theme');
      button.setAttribute('data-tip', dark ? 'Light theme' : 'Dark theme');
    });
    const themeColor = $('meta[name="theme-color"]');
    if (themeColor) themeColor.setAttribute('content', THEME_COLOR[theme]);
    if (persist) {
      try { window.localStorage.setItem(themeStorageKey, theme); } catch (_) { /* theme still applies for this page */ }
    }
  }

  const systemDark = () => window.matchMedia('(prefers-color-scheme: dark)').matches;
  applyTheme(readSavedTheme() ||
    (root.dataset.theme === 'light' || root.dataset.theme === 'dark' ? root.dataset.theme : (systemDark() ? 'dark' : 'light')), false);

  function switchTheme(theme) {
    const done = () => requestAnimationFrame(() => requestAnimationFrame(() => root.classList.remove('theme-switching')));
    root.classList.add('theme-switching');
    if (typeof document.startViewTransition === 'function' && !reduceMotion) {
      const t = document.startViewTransition(() => applyTheme(theme, true));
      t.finished.then(done, done);
    } else { applyTheme(theme, true); done(); }
  }

  $$('[data-theme-toggle]').forEach((button) => {
    button.addEventListener('click', () => switchTheme(root.dataset.theme === 'dark' ? 'light' : 'dark'));
  });

  window.addEventListener('storage', (event) => {
    if (event.key !== themeStorageKey) return;
    const next = event.newValue;
    applyTheme(next === 'light' || next === 'dark' ? next : (systemDark() ? 'dark' : 'light'), false);
  });

  // ---------------------------------------------------------------
  // Small shared bits: year, release links, OS detection
  // ---------------------------------------------------------------
  $$('[data-year]').forEach((e) => { e.textContent = String(new Date().getFullYear()); });
  $$('[data-release]').forEach((a) => { a.href = release + a.dataset.release; });

  const ua = navigator.userAgent;
  let detectedOS = 'win';
  if (/Mac/i.test(ua) && !/iPhone|iPad/i.test(ua)) detectedOS = 'mac';
  else if (/Linux|X11/i.test(ua) && !/Android/i.test(ua)) detectedOS = 'linux';
  else if (/Android|iPhone|iPad/i.test(ua)) detectedOS = 'mobile';

  const platformNames = { win: 'Windows', mac: 'macOS', linux: 'Linux' };
  $$('[data-os-card]').forEach((card) => { if (card.dataset.osCard === detectedOS) card.classList.add('recommended'); });

  const deviceCopy = $('[data-device-copy]');
  if (deviceCopy) {
    deviceCopy.textContent = detectedOS === 'mobile'
      ? 'CursorPie is made for desktop computers. Open this page on Windows, macOS or Linux to download it.'
      : 'We spotted ' + platformNames[detectedOS] + ' and highlighted that build. Pick another if it is not yours.';
  }

  const heroDownload = $('#hero-download');
  if (heroDownload) {
    const arrow = ' <span class="icon" aria-hidden="true">&#xe5de;</span>';
    const next = ' <span class="icon" aria-hidden="true">&#xe13a;</span>';
    if (detectedOS === 'win') { heroDownload.href = release + 'CursorPie-windows-setup.exe'; heroDownload.innerHTML = 'Download for Windows' + arrow; }
    else if (detectedOS === 'mac') { heroDownload.href = release + 'CursorPie-macos-universal.dmg'; heroDownload.innerHTML = 'Download for macOS' + arrow; }
    else if (detectedOS === 'linux') { heroDownload.href = '#download'; heroDownload.innerHTML = 'Choose a Linux build' + next; }
    else { heroDownload.href = '#download'; heroDownload.innerHTML = 'View desktop downloads' + next; }
  }

  // ---------------------------------------------------------------
  // Page chrome: scroll progress, mobile menu, nav scrollspy, glow
  // ---------------------------------------------------------------
  const progress = $('#progress');
  let ticking = false;
  function onScroll() {
    if (ticking) return;
    ticking = true;
    requestAnimationFrame(() => {
      const max = document.documentElement.scrollHeight - window.innerHeight;
      if (progress) progress.style.transform = 'scaleX(' + (max > 0 ? Math.min(1, window.scrollY / max) : 0) + ')';
      if (!reduceMotion) $$('[data-parallax]').forEach((el) => {
        el.style.transform = 'translate3d(0,' + (window.scrollY * parseFloat(el.dataset.parallax)).toFixed(1) + 'px,0)';
      });
      ticking = false;
    });
  }
  window.addEventListener('scroll', onScroll, { passive: true });
  window.addEventListener('resize', onScroll);
  onScroll();

  const topbar = $('#topbar');
  const menuBtn = $('#menu-btn');
  function setMenu(open) {
    if (!topbar || !menuBtn) return;
    topbar.classList.toggle('menu-open', open);
    menuBtn.setAttribute('aria-expanded', String(open));
    menuBtn.setAttribute('aria-label', open ? 'Close menu' : 'Open menu');
  }
  if (menuBtn) menuBtn.addEventListener('click', () => setMenu(menuBtn.getAttribute('aria-expanded') !== 'true'));

  const glow = $('#cursor-glow');
  if (glow && finePointer) {
    let gx = 0, gy = 0, pending = false;
    window.addEventListener('pointermove', (e) => {
      gx = e.clientX; gy = e.clientY;
      glow.classList.add('on');
      if (pending) return;
      pending = true;
      requestAnimationFrame(() => { glow.style.transform = 'translate3d(' + gx + 'px,' + gy + 'px,0)'; pending = false; });
    }, { passive: true });
    // 'pointerleave' does not fire on document in every engine, so also listen to mouse events.
    const glowOff = () => glow.classList.remove('on');
    document.documentElement.addEventListener('mouseleave', glowOff);
    document.addEventListener('mouseout', (e) => { if (!e.relatedTarget) glowOff(); });
    window.addEventListener('blur', glowOff);
  }

  // Magnetic buttons and tilting cards (fine pointers only)
  if (finePointer && !reduceMotion) {
    $$('[data-magnetic]').forEach((el) => {
      el.addEventListener('pointermove', (e) => {
        const r = el.getBoundingClientRect();
        el.style.translate = ((e.clientX - r.left - r.width / 2) * 0.18).toFixed(1) + 'px ' + ((e.clientY - r.top - r.height / 2) * 0.28).toFixed(1) + 'px';
      });
      el.addEventListener('pointerleave', () => { el.style.translate = ''; });
    });
    $$('[data-tilt]').forEach((el) => {
      const max = el.classList.contains('demo-card') ? 5 : 4;
      el.addEventListener('pointermove', (e) => {
        if (e.pointerType !== 'mouse' || e.buttons) return;
        const r = el.getBoundingClientRect();
        const px = (e.clientX - r.left) / r.width - 0.5;
        const py = (e.clientY - r.top) / r.height - 0.5;
        el.style.transform = 'perspective(900px) rotateX(' + (-py * max).toFixed(2) + 'deg) rotateY(' + (px * max).toFixed(2) + 'deg) translateZ(0)';
      });
      el.addEventListener('pointerleave', () => { el.style.transform = ''; });
    });
  }

  // Reveal on scroll + count-up numbers
  function countUp(el) {
    const target = parseFloat(el.dataset.count);
    const suffix = el.dataset.suffix || '';
    if (reduceMotion || !isFinite(target) || target === 0) { el.textContent = (isFinite(target) ? target : '') + suffix; return; }
    const start = performance.now();
    const dur = 1100;
    (function tick(now) {
      const t = Math.min(1, (now - start) / dur);
      el.textContent = Math.round(target * (1 - Math.pow(1 - t, 3))) + suffix;
      if (t < 1) requestAnimationFrame(tick);
    })(start);
  }

  if ('IntersectionObserver' in window) {
    const revealObserver = new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        const el = entry.target;
        el.classList.add('in');
        setTimeout(() => el.classList.add('done'), 1000);
        $$('[data-count]', el).forEach(countUp);
        revealObserver.unobserve(el);
      });
    }, { threshold: 0.12, rootMargin: '0px 0px -6% 0px' });
    $$('.rv, .section-heading').forEach((el) => revealObserver.observe(el));
  } else {
    $$('.rv, .section-heading').forEach((el) => el.classList.add('in', 'done'));
  }

  // Highlight the current section in the nav while scrolling the landing page
  const spyLinks = $$('[data-spy]');
  if ('IntersectionObserver' in window && spyLinks.length) {
    const spy = new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        spyLinks.forEach((a) => a.classList.toggle('active', a.dataset.spy === entry.target.id));
      });
    }, { rootMargin: '-45% 0px -50% 0px' });
    spyLinks.forEach((a) => { const s = document.getElementById(a.dataset.spy); if (s) spy.observe(s); });
    const hero = $('#top');
    if (hero) new IntersectionObserver((es) => { if (es[0].isIntersecting) spyLinks.forEach((a) => a.classList.remove('active')); }, { rootMargin: '-30% 0px -30% 0px' }).observe(hero);
  }

  // ---------------------------------------------------------------
  // Documentation map: one source of truth for sidebar, breadcrumbs,
  // titles and the previous/next pager.
  // ---------------------------------------------------------------
  const docGroups = [
    { title: 'Start here', items: [
      { file: 'docs/README.md', label: 'Documentation overview', icon: '\ue23a' },
      { file: 'docs/user/getting-started.md', label: 'Getting started', icon: '\ue13a' },
      { file: 'docs/user/install.md', label: 'Installation', icon: '\ue20c' }
    ] },
    { title: 'Using CursorPie', items: [
      { file: 'docs/user/README.md', label: 'User guide', icon: '\ue0e6' },
      { file: 'docs/user/using-the-pie.md', label: 'Using the pie', icon: '\ue15a' },
      { file: 'docs/user/profiles-and-buttons.md', label: 'Profiles and mouse buttons', icon: '\ue644' },
      { file: 'docs/user/trackpad.md', label: 'Trackpad hold', icon: '\ue586' },
      { file: 'docs/user/flows.md', label: 'Flows', icon: '\ue2d8' },
      { file: 'docs/user/settings.md', label: 'Settings and backups', icon: '\ue434' },
      { file: 'docs/user/linux.md', label: 'CursorPie on Linux', icon: '\ue1aa' },
      { file: 'docs/user/updates.md', label: 'Updates', icon: '\ue1a0' },
      { file: 'docs/user/configuration.md', label: 'Configuration', icon: '\ue434' },
      { file: 'docs/user/known-issues.md', label: 'Known issues', icon: '\ue4e0' }
    ] },
    { title: 'Flows and extensions', items: [
      { file: 'docs/FLOWS_FOR_AI.md', label: 'Flows for AI agents', icon: '\ue2d8' },
      { file: 'docs/examples/README.md', label: 'Example flows', icon: '\ue0e6' },
      { file: 'docs/PANELS.md', label: 'Panels', icon: '\ue0e6' },
      { file: 'docs/PANEL_DESIGN_FOR_AI.md', label: 'Panel design for AI', icon: '\ue150' },
      { file: 'docs/BROWSER_EXTENSION.md', label: 'Browser extension', icon: '\ue0e6' },
      { file: 'docs/DEBUGGING.md', label: 'Debugging', icon: '\ueae8' }
    ] },
    { title: 'Project and engineering', items: [
      { file: 'README.md', label: 'Project README', icon: '\ue576' },
      { file: 'CONTRIBUTING.md', label: 'Contributing', icon: '\ue2a8' },
      { file: 'CHANGELOG.md', label: 'Changelog', icon: '\ue1a0' },
      { file: 'docs/developer/README.md', label: 'Developer guide', icon: '\ue6a2' },
      { file: 'docs/developer/LINUX_MIGRATION.md', label: 'Linux migration', icon: '\ue1aa' },
      { file: 'docs/developer/WAYLAND_SUPPORT.md', label: 'Wayland support plan', icon: '\ue1aa' },
      { file: 'docs/developer/WINDOWS_MIGRATION.md', label: 'Windows migration', icon: '\ue32e' },
      { file: 'docs/developer/OTHER_DESKTOPS.md', label: 'Other desktops', icon: '\ue586' },
      { file: 'docs/developer/UI_DESIGN.md', label: 'UI design', icon: '\ue150' },
      { file: 'docs/developer/TRACKPAD_HAPTICS_PLAN.md', label: 'Trackpad and haptics plan', icon: '\ue586' },
      { file: 'docs/developer/BROWSER_BRIDGE.md', label: 'Browser bridge', icon: '\ue6a2' },
      { file: 'docs/developer/UPDATES_TESTING.md', label: 'Testing updates', icon: '\ue1a0' },
      { file: 'docs/developer/OPEN_URL_CHOICE.md', label: 'Open URL choice', icon: '\ue6a2' },
      { file: 'diag/README.md', label: 'Diagnostic crate notes', icon: '\ueae8' }
    ] }
  ];
  const documentMap = [];
  docGroups.forEach((g) => g.items.forEach((it) => { it.group = g.title; documentMap.push(it); }));

  const landingView = $('#landing-view');
  const documentationView = $('#documentation-view');
  const markdownContainer = $('#markdown-content');
  const docsSidebar = $('#docs-sidebar');
  const docsBackdrop = $('#docs-backdrop');
  const docsMenuBtn = $('#docs-menu-btn');

  // Remember the landing page's head so it can be restored after reading docs.
  const landingHead = {
    title: document.title,
    description: ($('meta[name="description"]') || {}).content,
    canonical: ($('link[rel="canonical"]') || {}).href,
    ogUrl: ($('meta[property="og:url"]') || {}).content,
    ogTitle: ($('meta[property="og:title"]') || {}).content,
    ogDesc: ($('meta[property="og:description"]') || {}).content,
    robots: ($('meta[name="robots"]') || {}).content
  };

  function normalizeDocPath(path) {
    const parts = [];
    String(path || '').replace(/\\/g, '/').split('/').forEach((part) => {
      if (!part || part === '.') return;
      if (part === '..') parts.pop(); else parts.push(part);
    });
    return parts.join('/');
  }

  function isAllowedMarkdown(path) {
    const normalized = normalizeDocPath(path);
    return documentMap.some((item) => item.file === normalized) ||
      (/^docs\/(?:(?:user|developer|examples)\/)?[A-Za-z0-9._-]+\.md$/i.test(normalized)) ||
      (/^diag\/[A-Za-z0-9._-]+\.md$/i.test(normalized));
  }

  const pageRootURL = () => new URL('./', window.location.href);

  function routeForMarkdown(path, hash) {
    const url = new URL('./index.html', pageRootURL());
    url.searchParams.set('doc', normalizeDocPath(path));
    if (hash) url.hash = hash;
    return url.href;
  }

  function slugify(text) {
    return text.toLowerCase().trim().replace(/[`*_~]/g, '').replace(/[^\p{L}\p{N}\s-]/gu, '').replace(/\s+/g, '-').replace(/-+/g, '-');
  }

  function assignHeadingIds(container) {
    const used = new Map();
    $$('h1, h2, h3, h4, h5, h6', container).forEach((heading) => {
      const base = slugify(heading.textContent) || 'section';
      const count = used.get(base) || 0;
      used.set(base, count + 1);
      heading.id = count ? base + '-' + count : base;
    });
  }

  function sanitizeAndResolve(container, markdownPath) {
    $$('script, iframe, object, embed, form', container).forEach((e) => e.remove());
    const markdownURL = new URL(markdownPath, pageRootURL());
    const siteRoot = pageRootURL();

    $$('*', container).forEach((element) => {
      Array.from(element.attributes).forEach((attribute) => {
        const name = attribute.name.toLowerCase();
        const value = attribute.value.trim();
        if (name.startsWith('on')) element.removeAttribute(attribute.name);
        if ((name === 'href' || name === 'src' || name === 'xlink:href') && /^\s*javascript:/i.test(value)) element.removeAttribute(attribute.name);
      });
    });

    $$('a[href]', container).forEach((anchor) => {
      const raw = anchor.getAttribute('href');
      if (!raw || raw.startsWith('#') || /^(mailto:|tel:|https?:)/i.test(raw)) {
        if (/^https?:/i.test(raw || '')) { anchor.target = '_blank'; anchor.rel = 'noopener noreferrer'; }
        return;
      }
      try {
        const target = new URL(raw, markdownURL);
        if (target.origin !== siteRoot.origin || !target.pathname.startsWith(siteRoot.pathname)) return;
        const relativePath = decodeURIComponent(target.pathname.slice(siteRoot.pathname.length));
        anchor.href = /\.md$/i.test(relativePath) && isAllowedMarkdown(relativePath) ? routeForMarkdown(relativePath, target.hash) : target.href;
      } catch (_) { anchor.removeAttribute('href'); }
    });

    $$('img[src], source[src]', container).forEach((image) => {
      const raw = image.getAttribute('src');
      if (!raw || /^(data:|https?:|\/\/)/i.test(raw)) return;
      try { image.src = new URL(raw, markdownURL).href; } catch (_) { image.removeAttribute('src'); }
    });

    assignHeadingIds(container);
  }

  // ---------------------------------------------------------------
  // Head / SEO helpers
  // ---------------------------------------------------------------
  function setHeadMeta(selector, create, attr, value) {
    let element = document.head.querySelector(selector);
    if (!element) { element = create(); document.head.appendChild(element); }
    element.setAttribute(attr, value);
  }
  const mk = (tag, attrs) => () => { const e = document.createElement(tag); Object.keys(attrs).forEach((k) => e.setAttribute(k, attrs[k])); return e; };

  function setHead(o) {
    document.title = o.title;
    setHeadMeta('link[rel="canonical"]', mk('link', { rel: 'canonical' }), 'href', o.url);
    setHeadMeta('meta[name="description"]', mk('meta', { name: 'description' }), 'content', o.description);
    setHeadMeta('meta[property="og:url"]', mk('meta', { property: 'og:url' }), 'content', o.url);
    setHeadMeta('meta[property="og:title"]', mk('meta', { property: 'og:title' }), 'content', o.ogTitle || o.title);
    setHeadMeta('meta[property="og:description"]', mk('meta', { property: 'og:description' }), 'content', o.description);
    setHeadMeta('meta[name="robots"]', mk('meta', { name: 'robots' }), 'content', o.robots);
  }

  // ---------------------------------------------------------------
  // Documentation sidebar
  // ---------------------------------------------------------------
  const docNav = $('#doc-nav');
  const docFilter = $('#doc-filter');
  const docEmpty = $('#doc-empty');

  function buildSidebar() {
    if (!docNav) return;
    docNav.innerHTML = '';
    docGroups.forEach((group) => {
      const section = document.createElement('div');
      section.className = 'doc-group';
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = group.title;
      button.setAttribute('aria-expanded', 'true');
      button.addEventListener('click', () => {
        const closed = section.classList.toggle('closed');
        button.setAttribute('aria-expanded', String(!closed));
      });
      const links = document.createElement('div');
      links.className = 'doc-links';
      group.items.forEach((item) => {
        const a = document.createElement('a');
        a.href = routeForMarkdown(item.file);
        a.dataset.docFile = item.file;
        a.dataset.search = (item.label + ' ' + item.file).toLowerCase();
        const icon = document.createElement('span');
        icon.className = 'icon';
        icon.setAttribute('aria-hidden', 'true');
        icon.textContent = item.icon;
        const text = document.createElement('span');
        text.textContent = item.label;
        a.append(icon, text);
        links.appendChild(a);
      });
      section.append(button, links);
      docNav.appendChild(section);
    });
  }

  function applyFilter() {
    if (!docNav) return;
    const q = (docFilter.value || '').trim().toLowerCase();
    let any = false;
    $$('.doc-group', docNav).forEach((group) => {
      let groupHas = false;
      $$('a', group).forEach((a) => {
        const match = !q || a.dataset.search.includes(q);
        a.hidden = !match;
        if (match) groupHas = true;
      });
      group.hidden = !groupHas;
      if (q && groupHas) group.classList.remove('closed');
      if (groupHas) any = true;
    });
    if (docEmpty) docEmpty.hidden = any;
  }
  if (docFilter) {
    docFilter.addEventListener('input', applyFilter);
    docFilter.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { const first = $('.doc-links a:not([hidden])', docNav); if (first) first.click(); }
      if (e.key === 'Escape') { docFilter.value = ''; applyFilter(); docFilter.blur(); }
    });
  }
  buildSidebar();

  function setDrawer(open) {
    if (!docsSidebar) return;
    docsSidebar.classList.toggle('open', open);
    if (docsBackdrop) docsBackdrop.hidden = !open;
    if (docsMenuBtn) docsMenuBtn.setAttribute('aria-expanded', String(open));
  }
  if (docsMenuBtn) docsMenuBtn.addEventListener('click', () => setDrawer(!docsSidebar.classList.contains('open')));
  if (docsBackdrop) docsBackdrop.addEventListener('click', () => setDrawer(false));

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { setDrawer(false); setMenu(false); }
    if (documentationView && !documentationView.hidden) {
      const typing = /^(INPUT|TEXTAREA|SELECT)$/.test((document.activeElement || {}).tagName || '');
      if ((e.key === '/' && !typing) || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k')) {
        e.preventDefault();
        setDrawer(true);
        if (docFilter) { docFilter.focus(); docFilter.select(); }
      }
    }
  });

  // ---------------------------------------------------------------
  // Markdown enhancement: code blocks, tables, callouts, anchors,
  // meta row, quick links, table of contents, pager
  // ---------------------------------------------------------------
  const COPY_SVG = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/></svg>';

  function enhanceCode(container) {
    $$('pre', container).forEach((pre) => {
      if (pre.parentElement.classList.contains('code-block')) return;
      const code = pre.querySelector('code');
      const langClass = code && Array.from(code.classList).find((c) => c.startsWith('language-'));
      const lang = langClass ? langClass.slice(9) : 'text';
      const wrapper = document.createElement('div');
      wrapper.className = 'code-block';
      const head = document.createElement('div');
      head.className = 'code-head';
      const label = document.createElement('span');
      label.textContent = lang;
      const copy = document.createElement('button');
      copy.type = 'button';
      copy.className = 'code-copy';
      copy.innerHTML = COPY_SVG + ' Copy';
      copy.addEventListener('click', async () => {
        const text = (code || pre).textContent;
        try {
          await navigator.clipboard.writeText(text);
        } catch (_) {
          const ta = document.createElement('textarea');
          ta.value = text; ta.style.position = 'fixed'; ta.style.opacity = '0';
          document.body.appendChild(ta); ta.select();
          try { document.execCommand('copy'); } catch (__) { /* nothing else to try */ }
          ta.remove();
        }
        copy.classList.add('done');
        copy.innerHTML = COPY_SVG + ' Copied!';
        setTimeout(() => { copy.classList.remove('done'); copy.innerHTML = COPY_SVG + ' Copy'; }, 1600);
      });
      head.append(label, copy);
      pre.parentNode.insertBefore(wrapper, pre);
      wrapper.append(head, pre);
    });
  }

  function enhanceTables(container) {
    $$('table', container).forEach((table) => {
      if (table.parentElement.classList.contains('table-wrap')) return;
      const wrap = document.createElement('div');
      wrap.className = 'table-wrap';
      table.parentNode.insertBefore(wrap, table);
      wrap.appendChild(table);
    });
  }

  function enhanceCallouts(container) {
    $$('blockquote', container).forEach((bq) => {
      const first = bq.querySelector('strong, b');
      const text = ((first && first.textContent) || '').toLowerCase();
      if (/^(warning|caution|danger|important|heads up)/.test(text)) bq.classList.add('callout-warn');
      else if (/^(tip|hint|best practice)/.test(text)) bq.classList.add('callout-tip');
      else if (/^(note|info)/.test(text)) bq.classList.add('callout-note');
    });
  }

  function enhanceAnchors(container) {
    $$('h2, h3, h4', container).forEach((h) => {
      if (!h.id) return;
      const a = document.createElement('a');
      a.className = 'anchor';
      a.href = '#' + h.id;
      a.setAttribute('aria-label', 'Link to this section');
      a.textContent = '#';
      h.insertBefore(a, h.firstChild);
    });
  }

  function addMeta(container, requested, info) {
    const words = (container.textContent || '').trim().split(/\s+/).length;
    const minutes = Math.max(1, Math.round(words / 220));
    const meta = document.createElement('div');
    meta.className = 'doc-meta';
    const mkTag = (tag, html, cls) => { const e = document.createElement(tag); e.className = 'tagm' + (cls ? ' ' + cls : ''); e.innerHTML = html; return e; };
    meta.appendChild(mkTag('span', info.group, 'lime'));
    meta.appendChild(mkTag('span', minutes + ' min read'));
    const src = mkTag('span', '<code></code>');
    src.querySelector('code').textContent = requested;
    meta.appendChild(src);
    const edit = mkTag('a', 'Edit on GitHub ↗');
    edit.href = repo + '/edit/main/' + requested;
    edit.target = '_blank'; edit.rel = 'noopener noreferrer';
    meta.appendChild(edit);
    const h1 = container.querySelector('h1');
    if (h1) h1.after(meta); else container.prepend(meta);
    return meta;
  }

  function addQuickLinks(meta) {
    const cards = [
      { file: 'docs/user/README.md', icon: '\ue0e6', title: 'Using CursorPie', text: 'The user guide: gestures, slices, profiles and settings.' },
      { file: 'docs/user/install.md', icon: '\ue20c', title: 'Install it', text: 'Packages, permissions and building for every system.' },
      { file: 'docs/FLOWS_FOR_AI.md', icon: '\ue2d8', title: 'Write a flow', text: 'The flow format and extension commands, made for AI agents too.' },
      { file: 'docs/developer/README.md', icon: '\ue576', title: 'Change the code', text: 'Design records and the engineering notes.' }
    ];
    const grid = document.createElement('div');
    grid.className = 'doc-quick';
    cards.forEach((c) => {
      const a = document.createElement('a');
      a.href = routeForMarkdown(c.file);
      const i = document.createElement('span'); i.className = 'qi'; i.textContent = c.icon; i.setAttribute('aria-hidden', 'true');
      const box = document.createElement('span');
      const b = document.createElement('b'); b.textContent = c.title;
      const s = document.createElement('small'); s.textContent = c.text;
      box.append(b, s);
      a.append(i, box);
      grid.appendChild(a);
    });
    meta.after(grid);
  }

  let tocObserver = null;
  function buildToc(container) {
    const toc = $('#doc-toc');
    const list = $('#doc-toc-list');
    if (!toc || !list) return;
    if (tocObserver) { tocObserver.disconnect(); tocObserver = null; }
    list.innerHTML = '';
    let heads = $$('h2, h3', container);
    if (heads.length > 40) heads = heads.filter((h) => h.tagName === 'H2');
    heads = heads.slice(0, 60);
    if (heads.length < 2) { toc.hidden = true; return; }
    toc.hidden = false;
    const map = new Map();
    heads.forEach((h) => {
      const li = document.createElement('li');
      const a = document.createElement('a');
      a.href = '#' + h.id;
      a.className = h.tagName === 'H3' ? 'l3' : 'l2';
      a.textContent = h.textContent.replace(/^#/, '').trim();
      li.appendChild(a);
      list.appendChild(li);
      map.set(h, a);
    });
    if (!('IntersectionObserver' in window)) return;
    tocObserver = new IntersectionObserver((entries) => {
      entries.forEach((entry) => {
        if (!entry.isIntersecting) return;
        map.forEach((a) => a.classList.remove('on'));
        const a = map.get(entry.target);
        if (a) { a.classList.add('on'); const box = $('#doc-toc'); if (box) { const r = a.getBoundingClientRect(), b = box.getBoundingClientRect(); if (r.top < b.top || r.bottom > b.bottom) a.scrollIntoView({ block: 'nearest' }); } }
      });
    }, { rootMargin: '-90px 0px -72% 0px' });
    heads.forEach((h) => tocObserver.observe(h));
  }

  function buildPager(requested) {
    const pager = $('#doc-pager');
    if (!pager) return;
    pager.innerHTML = '';
    const i = documentMap.findIndex((d) => d.file === requested);
    if (i < 0) return;
    const make = (item, cls, label) => {
      const a = document.createElement('a');
      a.className = cls;
      a.href = routeForMarkdown(item.file);
      const s = document.createElement('small'); s.textContent = label;
      const b = document.createElement('b'); b.textContent = item.label;
      a.append(s, b);
      return a;
    };
    if (documentMap[i - 1]) pager.appendChild(make(documentMap[i - 1], 'prev', '← Previous'));
    if (documentMap[i + 1]) pager.appendChild(make(documentMap[i + 1], 'next', 'Next →'));
  }

  function buildCrumbs(info, label) {
    const crumbs = $('#doc-crumbs');
    if (!crumbs) return;
    crumbs.innerHTML = '';
    const home = document.createElement('a'); home.href = './index.html'; home.textContent = 'Home';
    const docs = document.createElement('a'); docs.href = routeForMarkdown('docs/README.md'); docs.textContent = 'Docs';
    const sep = () => { const s = document.createElement('span'); s.className = 'sep'; s.textContent = '/'; return s; };
    const current = document.createElement('b'); current.textContent = label;
    crumbs.append(home, sep(), docs, sep());
    if (info && info.group) { const g = document.createElement('span'); g.textContent = info.group; crumbs.append(g, sep()); }
    crumbs.append(current);
  }

  // ---------------------------------------------------------------
  // Documentation page: fetch + render
  // ---------------------------------------------------------------
  let loadToken = 0;

  function renderDocumentError(message, path) {
    if (!markdownContainer) return;
    setHeadMeta('meta[name="robots"]', mk('meta', { name: 'robots' }), 'content', 'noindex');
    markdownContainer.innerHTML = '<div class="doc-error"><span class="icon" aria-hidden="true">&#xe4e0;</span><h2>Could not load this document</h2><p></p><p class="doc-error-path"></p><p>Make sure the website is in the repository root and the Markdown file is available at that path.</p><p>For local testing, start a web server from the repository root, for example <code>python3 -m http.server 8000</code>.</p></div>';
    markdownContainer.querySelector('p').textContent = message;
    markdownContainer.querySelector('.doc-error-path').textContent = path || '';
    const toc = $('#doc-toc'); if (toc) toc.hidden = true;
    const pager = $('#doc-pager'); if (pager) pager.innerHTML = '';
  }

  async function loadMarkdown(path, info, label) {
    if (!markdownContainer) return;
    const token = ++loadToken;
    markdownContainer.innerHTML = '<div class="markdown-loading"><span class="loading-dot"></span> Loading documentation…</div>';
    try {
      const response = await fetch(new URL(path, pageRootURL()).href, { cache: 'no-cache' });
      if (!response.ok) throw new Error('The server returned HTTP ' + response.status + '.');
      const markdown = await response.text();
      if (token !== loadToken) return;
      if (!window.marked || typeof window.marked.parse !== 'function') throw new Error('The local Markdown parser could not be loaded from assets/js/marked.js.');
      markdownContainer.innerHTML = window.marked.parse(markdown, { gfm: true, breaks: false, mangle: false, headerIds: false });
      sanitizeAndResolve(markdownContainer, path);
      enhanceCode(markdownContainer);
      enhanceTables(markdownContainer);
      enhanceCallouts(markdownContainer);
      const meta = addMeta(markdownContainer, path, info);
      if (path === 'docs/README.md') addQuickLinks(meta);
      enhanceAnchors(markdownContainer);
      buildToc(markdownContainer);
      buildPager(path);
      markdownContainer.classList.remove('doc-fade');
      void markdownContainer.offsetWidth;
      markdownContainer.classList.add('doc-fade');
      if (window.location.hash) {
        const id = decodeURIComponent(window.location.hash.slice(1));
        const target = document.getElementById(id);
        if (target) target.scrollIntoView();
      }
    } catch (error) {
      if (token !== loadToken) return;
      renderDocumentError(error.message || 'An unexpected error occurred while reading the Markdown file.', path);
    }
  }

  function showDocumentation(rawDoc) {
    if (!landingView || !documentationView || !markdownContainer) return;
    landingView.hidden = true;
    documentationView.hidden = false;
    $$('[data-doc-link]').forEach((l) => l.setAttribute('aria-current', 'page'));
    $$('[data-spy]').forEach((l) => l.classList.remove('active'));
    setDrawer(false);

    const requested = normalizeDocPath(rawDoc);
    if (!isAllowedMarkdown(requested)) {
      buildCrumbs(null, 'Not found');
      renderDocumentError('That Markdown page is not part of the configured documentation sources.', requested);
      return;
    }
    const info = documentMap.find((item) => item.file === requested) || { group: 'Docs' };
    const label = info.label || requested.split('/').pop().replace(/\.md$/i, '').replace(/[_-]+/g, ' ');
    const url = routeForMarkdown(requested);
    setHead({
      title: label + ' — CursorPie docs',
      url: url,
      description: label + ' for CursorPie, the free open-source pie menu for Windows, macOS and Linux.',
      robots: landingHead.robots || 'index, follow'
    });
    buildCrumbs(info, label);
    $$('[data-doc-file]').forEach((link) => {
      if (normalizeDocPath(link.dataset.docFile) === requested) link.setAttribute('aria-current', 'page');
      else link.removeAttribute('aria-current');
    });
    const active = $('.doc-links a[aria-current="page"]', docNav);
    if (active) { const g = active.closest('.doc-group'); if (g) g.classList.remove('closed'); }
    loadMarkdown(requested, info, label);
  }

  function showLanding() {
    if (!landingView || !documentationView) return;
    loadToken++;
    documentationView.hidden = true;
    landingView.hidden = false;
    $$('[data-doc-link]').forEach((l) => l.removeAttribute('aria-current'));
    setDrawer(false);
    setHead({
      title: landingHead.title,
      url: landingHead.canonical,
      description: landingHead.description,
      ogTitle: landingHead.ogTitle,
      robots: landingHead.robots
    });
    setHeadMeta('meta[property="og:description"]', mk('meta', { property: 'og:description' }), 'content', landingHead.ogDesc);
    if (tocObserver) { tocObserver.disconnect(); tocObserver = null; }
    if (typeof window.__cursorpieRefreshPies === 'function') window.__cursorpieRefreshPies();
  }

  // ---------------------------------------------------------------
  // Router: the whole site is one page. ?doc=... shows documentation,
  // otherwise the landing page. Internal links never reload the page.
  // ---------------------------------------------------------------
  function currentDoc() {
    const d = new URLSearchParams(window.location.search).get('doc');
    return d === null ? null : decodeURIComponent(d);
  }

  let shownDoc;   // undefined = nothing rendered yet
  function render(fromNav) {
    const doc = currentDoc();
    const same = shownDoc === doc;
    if (same && !fromNav) return;         // hash-only change: the browser scrolls
    shownDoc = doc;
    if (doc === null) showLanding(); else showDocumentation(doc);
    afterRender(doc, same);
  }

  let firstRender = true;
  function afterRender(doc, same) {
    const hash = window.location.hash;
    const initial = firstRender;
    firstRender = false;
    const go = () => {
      if (hash) {
        const el = document.getElementById(decodeURIComponent(hash.slice(1)));
        if (el) { el.scrollIntoView({ behavior: doc === null && same ? 'smooth' : 'auto' }); return; }
      }
      if (!same && !initial) window.scrollTo(0, 0);
    };
    requestAnimationFrame(go);
  }

  const indexPath = () => new URL('./index.html', pageRootURL()).pathname;
  const rootPath = () => pageRootURL().pathname;

  document.addEventListener('click', (e) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a || (a.target && a.target !== '_self') || a.hasAttribute('download')) return;
    const raw = a.getAttribute('href');
    if (!raw || raw.startsWith('#')) { setMenu(false); return; }
    let u;
    try { u = new URL(a.href, window.location.href); } catch (_) { return; }
    if (u.origin !== window.location.origin) return;
    if (u.pathname !== indexPath() && u.pathname !== rootPath()) return;
    e.preventDefault();
    setMenu(false);
    setDrawer(false);
    window.history.pushState({}, '', u.href);
    render(true);
  });

  window.addEventListener('popstate', () => render(false));
  render(true);

  // ---------------------------------------------------------------
  // Pie menus: hero demo (interactive) and "how it works" (scripted)
  // ---------------------------------------------------------------
  const SVG_NS = 'http://www.w3.org/2000/svg';
  const pieItems = [
    { name: 'History', glyph: '\ue08a' },
    { name: 'Clipboard', glyph: '\ue198' },
    { name: 'Brackets', glyph: '\ue85e' },
    { name: 'Launch', glyph: '\ue3fe' }
  ];
  const outerRadius = 145;
  const innerRadius = outerRadius * 0.36;
  const sweep = Math.PI * 2 / pieItems.length;
  const polar = (radius, angle) => [radius * Math.cos(angle), radius * Math.sin(angle)];

  // Slice 0 (History) is centred on the left side of the ring.
  function buildPie(svg) {
    const gap = 0.05;
    const slices = [];
    pieItems.forEach((item, index) => {
      const start = Math.PI - sweep / 2 + index * sweep;
      const a0 = start + gap / 2, a1 = start + sweep - gap / 2;
      const p1 = polar(outerRadius, a0), p2 = polar(outerRadius, a1), p3 = polar(innerRadius, a1), p4 = polar(innerRadius, a0);
      const path = document.createElementNS(SVG_NS, 'path');
      path.setAttribute('d', `M${p1[0]},${p1[1]} A${outerRadius},${outerRadius} 0 0 1 ${p2[0]},${p2[1]} L${p3[0]},${p3[1]} A${innerRadius},${innerRadius} 0 0 0 ${p4[0]},${p4[1]} Z`);
      path.setAttribute('class', 'pie-slice');
      svg.appendChild(path);
      const ip = polar((outerRadius + innerRadius) / 2, start + sweep / 2);
      const icon = document.createElementNS(SVG_NS, 'text');
      icon.setAttribute('x', ip[0]); icon.setAttribute('y', ip[1]);
      icon.setAttribute('class', 'pie-icon');
      icon.textContent = item.glyph;
      svg.appendChild(icon);
      slices.push(path);
    });
    const outline = document.createElementNS(SVG_NS, 'circle');
    outline.setAttribute('r', outerRadius); outline.setAttribute('class', 'pie-outline');
    svg.appendChild(outline);
    const core = document.createElementNS(SVG_NS, 'circle');
    core.setAttribute('r', innerRadius * 0.86); core.setAttribute('class', 'pie-core');
    svg.appendChild(core);
    const coreIcon = document.createElementNS(SVG_NS, 'text');
    coreIcon.setAttribute('x', '0'); coreIcon.setAttribute('y', '1'); coreIcon.setAttribute('class', 'pie-core-icon');
    coreIcon.textContent = '\ue08a';
    svg.appendChild(coreIcon);
    return slices;
  }

  // ----- hero demo -----
  const stage = $('#pie-stage');
  const demoSvg = $('#pie-demo');
  let resetDemo = null;
  if (stage && demoSvg) {
    const slices = buildPie(demoSvg);
    const hint = $('#pie-hint');
    let centerX = 0, centerY = 0, highlighted = -1, pointerDown = false;

    const setPosition = (x, y) => {
      const bounds = stage.getBoundingClientRect();
      const margin = outerRadius + 12;
      centerX = Math.max(margin, Math.min(Math.max(margin, bounds.width - margin), x));
      centerY = Math.max(margin, Math.min(Math.max(margin, bounds.height - margin), y));
      demoSvg.style.left = centerX + 'px';
      demoSvg.style.top = centerY + 'px';
      demoSvg.style.transform = 'translate(-50%, -50%)';
    };
    const setHighlighted = (index) => {
      if (index === highlighted) return;
      if (highlighted >= 0) slices[highlighted].classList.remove('hot');
      highlighted = index;
      if (highlighted >= 0) slices[highlighted].classList.add('hot');
    };
    resetDemo = () => {
      setPosition(stage.clientWidth / 2, stage.clientHeight * 0.48);
      stage.classList.remove('on');
      setHighlighted(-1);
    };
    const localPoint = (event) => { const b = stage.getBoundingClientRect(); return [event.clientX - b.left, event.clientY - b.top]; };

    stage.addEventListener('pointerdown', (event) => {
      if (event.button !== 0) return;
      pointerDown = true;
      stage.setPointerCapture(event.pointerId);
      const p = localPoint(event);
      setPosition(p[0], p[1]);
      stage.classList.add('on');
      setHighlighted(-1);
      if (hint) hint.innerHTML = 'Move toward a slice, then <strong>release</strong>.';
    });
    stage.addEventListener('pointermove', (event) => {
      if (!pointerDown) return;
      const p = localPoint(event);
      const dx = p[0] - centerX, dy = p[1] - centerY;
      if (Math.hypot(dx, dy) < innerRadius * 0.7) { setHighlighted(-1); return; }
      let angle = Math.atan2(dy, dx);
      if (angle < 0) angle += Math.PI * 2;
      const adjusted = (angle - (Math.PI - sweep / 2) + Math.PI * 2) % (Math.PI * 2);
      setHighlighted(Math.floor(adjusted / sweep) % pieItems.length);
    });
    const endGesture = () => {
      if (!pointerDown) return;
      pointerDown = false;
      const choice = highlighted >= 0 ? pieItems[highlighted].name : null;
      if (hint) hint.innerHTML = choice ? 'Selected: <strong>' + choice + '</strong>. Try another slice.' : 'No slice selected. Try again.';
      resetDemo();
    };
    stage.addEventListener('pointerup', endGesture);
    stage.addEventListener('pointercancel', endGesture);
    stage.addEventListener('contextmenu', (event) => event.preventDefault());
    window.addEventListener('resize', () => { if (!pointerDown) resetDemo(); });
    resetDemo();
  }

  // ----- how it works: scrolling drives the animation -----
  const howStage = $('#how-stage');
  const howSvg = $('#how-pie');
  if (howStage && howSvg) {
    const slices = buildPie(howSvg);
    const label = $('#hs-label');
    const labels = ['Hold the trigger', 'Flick toward a slice', 'Release to act'];
    const steps = $$('.how-step');
    const setStep = (n) => {
      howStage.dataset.step = String(n);
      if (label) label.textContent = labels[n - 1];
      slices.forEach((s) => s.classList.remove('hot', 'fire'));
      if (n === 2) slices[1].classList.add('hot');
      if (n === 3) slices[1].classList.add('fire');
      steps.forEach((s) => s.classList.toggle('active', s.dataset.step === String(n)));
    };
    setStep(1);
    if ('IntersectionObserver' in window) {
      const io = new IntersectionObserver((entries) => {
        entries.forEach((en) => { if (en.isIntersecting) setStep(parseInt(en.target.dataset.step, 10)); });
      }, { rootMargin: '-42% 0px -42% 0px' });
      steps.forEach((s) => io.observe(s));
    } else {
      // Without IntersectionObserver, loop through the steps on a timer.
      let n = 1; setInterval(() => { n = n % 3 + 1; setStep(n); }, 2200);
    }
  }

  // ----- feature card: per-app pies -----
  const appdemo = $('#appdemo');
  if (appdemo && !reduceMotion) {
    const sets = [
      ['Back', 'Reload', 'New tab', 'Close tab'],
      ['Run', 'Format', 'Comment', 'Terminal'],
      ['Undo', 'Zoom', 'Export', 'Layers'],
      ['Copy', 'Paste', 'Clear', 'History']
    ];
    const tabs = $$('.app-tabs span', appdemo);
    const box = $('.app-slices', appdemo);
    let i = 0;
    setInterval(() => {
      if (document.hidden || (landingView && landingView.hidden)) return;
      i = (i + 1) % sets.length;
      tabs.forEach((t, k) => t.classList.toggle('on', k === i));
      box.innerHTML = sets[i].map((s) => '<b>' + s + '</b>').join('');
    }, 2200);
  }

  window.__cursorpieRefreshPies = () => { if (resetDemo) requestAnimationFrame(resetDemo); };
  window.__cursorpieRefreshPies();
  window.__cursorpieReady = true;
})();
