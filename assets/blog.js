/* CursorPie blog: theme, mobile menu, reading progress, table of contents, copy buttons.
   Same theme key as the main site, so the choice follows the reader from page to page. */
(function () {
  'use strict';
  var root = document.documentElement;
  var $ = function (s, c) { return (c || document).querySelector(s); };
  var $$ = function (s, c) { return Array.prototype.slice.call((c || document).querySelectorAll(s)); };
  var reduce = window.matchMedia && matchMedia('(prefers-reduced-motion: reduce)').matches;

  /* ---------------------------------------------------------------- theme */
  function setTheme(t) {
    root.dataset.theme = t;
    try { localStorage.setItem('cursorpie-theme', t); } catch (e) {}
    var m = $('meta[name="theme-color"]');
    if (m) m.setAttribute('content', t === 'dark' ? '#100f14' : '#fff8ec');
    $$('[data-theme-toggle]').forEach(function (b) {
      var dark = t === 'dark';
      var ic = $('.icon', b);
      if (ic) ic.innerHTML = dark ? '&#xe472;' : '&#xe330;';
      b.setAttribute('aria-label', dark ? 'Switch to light theme' : 'Switch to dark theme');
      b.setAttribute('data-tip', dark ? 'Light theme' : 'Dark theme');
    });
  }
  setTheme(root.dataset.theme === 'dark' ? 'dark' : 'light');
  $$('[data-theme-toggle]').forEach(function (b) {
    b.addEventListener('click', function () { setTheme(root.dataset.theme === 'dark' ? 'light' : 'dark'); });
  });

  /* ---------------------------------------------------------------- mobile menu */
  var topbar = $('#topbar');
  var menuBtn = $('#menu-btn');
  if (menuBtn && topbar) {
    menuBtn.addEventListener('click', function () {
      var open = menuBtn.getAttribute('aria-expanded') !== 'true';
      topbar.classList.toggle('menu-open', open);
      menuBtn.setAttribute('aria-expanded', String(open));
      menuBtn.setAttribute('aria-label', open ? 'Close menu' : 'Open menu');
    });
  }

  $$('[data-year]').forEach(function (n) { n.textContent = new Date().getFullYear(); });

  /* ---------------------------------------------------------------- reading progress */
  var bar = $('#progress');
  var article = $('#post');
  if (bar && article) {
    var tick = false;
    var update = function () {
      tick = false;
      var r = article.getBoundingClientRect();
      var total = r.height - window.innerHeight * 0.6;
      var done = Math.min(1, Math.max(0, (-r.top + window.innerHeight * 0.25) / Math.max(total, 1)));
      bar.style.transform = 'scaleX(' + done.toFixed(4) + ')';
    };
    window.addEventListener('scroll', function () {
      if (!tick) { tick = true; requestAnimationFrame(update); }
    }, { passive: true });
    window.addEventListener('resize', update);
    update();
  }

  /* ---------------------------------------------------------------- table of contents */
  var toc = $('#toc');
  if (toc && article) {
    var heads = $$('h2[id]', article).filter(function (h) { return !h.closest('.tldr, .post-cta'); });
    if (heads.length) {
      var h4 = document.createElement('h4');
      h4.textContent = 'On this page';
      var ol = document.createElement('ol');
      heads.forEach(function (h) {
        var li = document.createElement('li');
        var a = document.createElement('a');
        a.href = '#' + h.id;
        var clone = h.cloneNode(true);
        $$('.num', clone).forEach(function (n) { n.remove(); });
        a.textContent = clone.textContent.trim();
        li.appendChild(a);
        ol.appendChild(li);
      });
      toc.appendChild(h4);
      toc.appendChild(ol);
      var links = $$('a', ol);
      if ('IntersectionObserver' in window) {
        var io = new IntersectionObserver(function (entries) {
          entries.forEach(function (e) {
            if (e.isIntersecting) {
              links.forEach(function (l) { l.classList.toggle('on', l.getAttribute('href') === '#' + e.target.id); });
            }
          });
        }, { rootMargin: '-15% 0px -70% 0px' });
        heads.forEach(function (h) { io.observe(h); });
      }
    }
  }

  /* ---------------------------------------------------------------- copy buttons on code */
  $$('.post pre').forEach(function (pre) {
    var code = $('code', pre);
    if (!code || !navigator.clipboard) return;
    var b = document.createElement('button');
    b.type = 'button';
    b.className = 'copy-btn';
    b.textContent = 'Copy';
    b.addEventListener('click', function () {
      navigator.clipboard.writeText(code.innerText).then(function () {
        b.textContent = 'Copied!';
        setTimeout(function () { b.textContent = 'Copy'; }, 1600);
      }, function () {
        b.textContent = 'Press Ctrl+C';
        setTimeout(function () { b.textContent = 'Copy'; }, 1600);
      });
    });
    pre.appendChild(b);
  });

  /* smooth in-page jumps respect reduced motion */
  if (reduce) root.style.scrollBehavior = 'auto';
})();
