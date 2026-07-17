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

  /* ── table of contents ─────────────────────────────────
     Permanently visible (no toggle). Built from .content h2[id]/h3[id].
     Hidden on pages with no content headings (home/landing). */
  function initToc() {
    var headings = document.querySelectorAll('.content h2[id], .content h3[id]');
    if (!headings.length) return;
    var zh = document.documentElement.lang === 'zh-CN';

    var nav = document.createElement('nav');
    nav.className = 'toc-popover';
    nav.setAttribute('aria-label', zh ? '本页目录' : 'On this page');
    var title = document.createElement('div');
    title.className = 'toc-title';
    title.textContent = zh ? '本页目录' : 'On this page';
    nav.appendChild(title);
    var links = [];
    Array.prototype.forEach.call(headings, function (h) {
      var a = document.createElement('a');
      a.href = '#' + h.id;
      a.textContent = h.textContent.trim();
      if (h.tagName === 'H3') a.className = 'level-3';
      nav.appendChild(a);
      links.push({ el: a, heading: h });
    });
    document.body.appendChild(nav);

    // scroll-spy (throttled via rAF). anchor-jump smoothing respects
    // prefers-reduced-motion via the html{scroll-behavior} rule in style.css.
    var ticking = false;
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
