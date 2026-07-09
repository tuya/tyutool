/* tyutool usage guide — theme toggle + table-of-contents popover.
   Vanilla JS, no dependencies. Runs on DOMContentLoaded.
   Theme contract: <html data-theme="light"|"dark"> or absent (= system).
   Persisted in localStorage["tyutool-docs-theme"]. An early inline <head>
   script applies the attribute before paint (prevents FOUC); this file
   only syncs the button UI and wires interactions. */
(function () {
  'use strict';
  var STORAGE_KEY = 'tyutool-docs-theme';

  /* ── theme ───────────────────────────────────────────── */
  function currentMode() {
    var saved;
    try { saved = localStorage.getItem(STORAGE_KEY); } catch (e) { saved = null; }
    if (saved === 'light' || saved === 'dark') return saved;
    return 'system';
  }

  function syncThemeButton(btn) {
    // the icon shown reflects the *preference*: light / dark / auto(system).
    // aria-label is a static prefix from the page; we append the mode word.
    var pref = currentMode();
    btn.dataset.mode = pref === 'system' ? 'system' : pref;
    var word = { light: 'light', dark: 'dark', system: 'auto/system' }[pref];
    var prefix = btn.getAttribute('aria-label') || 'Theme';
    // strip any previously-appended mode word so we don't accumulate
    prefix = prefix.replace(/:.*$/, '');
    btn.setAttribute('aria-label', prefix + ': ' + word);
  }

  function cycleTheme() {
    var pref = currentMode();
    var next = pref === 'light' ? 'dark' : pref === 'dark' ? 'system' : 'light';
    try { localStorage.setItem(STORAGE_KEY, next); } catch (e) {}
    if (next === 'system') {
      delete document.documentElement.dataset.theme;
    } else {
      document.documentElement.dataset.theme = next;
    }
    var btn = document.querySelector('.theme-toggle');
    if (btn) syncThemeButton(btn);
  }

  function initTheme() {
    var btn = document.querySelector('.theme-toggle');
    if (!btn) return;
    btn.addEventListener('click', cycleTheme);
    syncThemeButton(btn);
    // live icon updates when OS changes while in system mode
    if (window.matchMedia) {
      window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function () {
        if (currentMode() === 'system') {
          var b = document.querySelector('.theme-toggle');
          if (b) syncThemeButton(b);
        }
      });
    }
  }

  /* ── table of contents ───────────────────────────────── */
  function initToc() {
    var btn = document.querySelector('.toc-toggle');
    if (!btn) return;
    var headings = document.querySelectorAll('.content h2[id], .content h3[id]');
    if (!headings.length) { btn.style.display = 'none'; return; }

    // build popover
    var nav = document.createElement('nav');
    nav.className = 'toc-popover';
    nav.setAttribute('hidden', '');
    var title = document.createElement('div');
    title.className = 'toc-title';
    title.textContent = btn.dataset.tocTitle || 'On this page';
    nav.appendChild(title);
    var links = [];
    Array.prototype.forEach.call(headings, function (h) {
      var a = document.createElement('a');
      a.href = '#' + h.id;
      a.textContent = h.textContent.replace(/^\s*\d+\.\s*/, '').trim();
      if (h.tagName === 'H3') a.className = 'level-3';
      a.addEventListener('click', function () { close(); });
      nav.appendChild(a);
      links.push({ el: a, heading: h });
    });
    document.body.appendChild(nav);

    function open() { nav.removeAttribute('hidden'); btn.setAttribute('aria-expanded', 'true'); }
    function close() { nav.setAttribute('hidden', ''); btn.setAttribute('aria-expanded', 'false'); }
    function toggle() { nav.hasAttribute('hidden') ? open() : close(); }
    btn.addEventListener('click', function (e) { e.stopPropagation(); toggle(); });
    nav.addEventListener('click', function (e) { e.stopPropagation(); });
    document.addEventListener('click', function (e) { if (!nav.hasAttribute('hidden')) close(); });
    document.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && !nav.hasAttribute('hidden')) { close(); btn.focus(); }
    });

    // scroll-spy (throttled via rAF)
    var ticking = false;
    var reduced = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    function spy() {
      var active = null;
      var top = 100;
      for (var i = 0; i < headings.length; i++) {
        if (headings[i].getBoundingClientRect().top < top) active = headings[i];
      }
      // if nothing above the line yet, mark the first
      if (!active && headings.length) active = headings[0];
      for (var j = 0; j < links.length; j++) {
        if (links[j].heading === active) links[j].el.classList.add('is-active');
        else links[j].el.classList.remove('is-active');
      }
      ticking = false;
    }
    window.addEventListener('scroll', function () {
      if (!ticking) { window.requestAnimationFrame(spy); ticking = true; }
    }, { passive: true });
    spy();
  }

  document.addEventListener('DOMContentLoaded', function () {
    initTheme();
    initToc();
  });
})();
