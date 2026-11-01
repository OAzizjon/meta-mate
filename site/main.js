// The landing page's panel navigation: wheel, keys, dots and hash (no libraries).
// A file, not inline, so a strict Content-Security-Policy (script-src 'self') allows it.
(function () {
  'use strict';

  var strip = document.getElementById('main');
  if (!strip) return;

  var panels = Array.prototype.slice.call(strip.querySelectorAll('.panel'));
  var dots = Array.prototype.slice.call(document.querySelectorAll('.panel-dot'));
  var hashLinks = Array.prototype.slice.call(document.querySelectorAll('a[href^="#"]'));
  var progressFill = document.getElementById('progress-fill');
  var progressTrack = document.querySelector('.progress-track');
  var prevArrow = document.querySelector('.panel-arrow-prev');
  var nextArrow = document.querySelector('.panel-arrow-next');
  var navHint = document.getElementById('nav-hint');
  var navHintClose = navHint ? navHint.querySelector('.nav-hint-close') : null;
  if (!panels.length) return;

  var wideQuery = window.matchMedia ? window.matchMedia('(min-width: 700px)') : null;
  var reducedQuery = window.matchMedia ? window.matchMedia('(prefers-reduced-motion: reduce)') : null;
  var supportsSnap = Boolean(
    wideQuery &&
      window.CSS &&
      typeof window.CSS.supports === 'function' &&
      window.CSS.supports('scroll-snap-type', 'x mandatory') &&
      window.CSS.supports('display', 'flex')
  );
  if (supportsSnap) {
    document.documentElement.classList.add('has-js');
  }

  var hashAliases = {
    features: 'how-it-works',
    cards: 'lesson-cards',
    scam: 'scam-check',
    safety: 'safety-and-privacy',
    privacy: 'safety-and-privacy',
    meta: 'built-with-meta'
  };

  var wheelAttached = false;
  var wheelAccum = 0;
  var lastWheelSign = 0;
  var lastWheelTime = 0;
  var wheelLockUntil = 0;
  // One wheel gesture moves one panel: after a step, the gesture's inertia is ignored until the
  // wheel pauses; a step taken while the strip still slides counts from its target panel.
  var stepping = false;
  var targetIndex = -1;
  var slideEndsAt = 0;
  var lastVerticalScrollTime = 0;

  function isHorizontalMode() {
    return supportsSnap && wideQuery.matches && strip.scrollWidth > strip.clientWidth + 1;
  }

  function resetDocumentScroll() {
    if (window.scrollTo && (window.scrollY || document.documentElement.scrollTop)) {
      window.scrollTo(0, 0);
    }
  }

  function prefersReducedMotion() {
    return Boolean(reducedQuery && reducedQuery.matches);
  }

  function getCurrentIndex() {
    var width = strip.clientWidth || 1;
    var raw = Math.round(strip.scrollLeft / width);
    if (raw < 0) return 0;
    if (raw >= panels.length) return panels.length - 1;
    return raw;
  }

  function updateIndicators(activeIndex) {
    for (var i = 0; i < dots.length; i += 1) {
      if (i === activeIndex) {
        dots[i].setAttribute('aria-current', 'step');
        dots[i].classList.add('is-active');
      } else {
        dots[i].removeAttribute('aria-current');
        dots[i].classList.remove('is-active');
      }
    }
    if (progressFill) {
      progressFill.setAttribute('data-step', String(activeIndex + 1));
    }
    if (progressTrack) {
      progressTrack.setAttribute('aria-valuenow', String(activeIndex + 1));
    }
    if (prevArrow) {
      prevArrow.hidden = activeIndex <= 0;
    }
    if (nextArrow) {
      nextArrow.hidden = activeIndex >= panels.length - 1;
    }
  }

  function scrollToPanel(index, forceInstant) {
    var clamped = Math.max(0, Math.min(panels.length - 1, index));
    var targetLeft = panels[clamped].offsetLeft;
    if (forceInstant || prefersReducedMotion()) {
      strip.scrollLeft = targetLeft;
    } else {
      strip.scrollTo({ left: targetLeft, behavior: 'smooth' });
    }
    updateIndicators(clamped);
  }

  function resolveHashIndex(rawHash) {
    if (!rawHash || rawHash === '#' || rawHash === '#main') return -1;
    var cleanId = decodeURIComponent(rawHash.replace(/^#/, ''));
    var mappedId = hashAliases[cleanId] || cleanId;
    var target = document.getElementById(mappedId);
    if (!target) return -1;
    var panel = target.closest ? target.closest('.panel') : target;
    return panels.indexOf(panel);
  }

  function canScrollVertically(startNode, deltaY) {
    var element =
      startNode && startNode.nodeType === 1
        ? startNode
        : startNode && startNode.parentElement;
    while (
      element &&
      element !== strip &&
      element !== document.body &&
      element !== document.documentElement
    ) {
      var computed = window.getComputedStyle(element);
      var overflowY = computed.overflowY;
      if (
        (overflowY === 'auto' || overflowY === 'scroll') &&
        element.scrollHeight > element.clientHeight + 1
      ) {
        if (
          deltaY > 0 &&
          Math.ceil(element.scrollTop + element.clientHeight) < element.scrollHeight - 1
        ) {
          return true;
        }
        if (deltaY < 0 && element.scrollTop > 1) {
          return true;
        }
      }
      element = element.parentElement;
    }
    return false;
  }

  function toPixelDelta(delta, mode) {
    if (mode === 1) return delta * 32;
    if (mode === 2) return delta * (strip.clientWidth || 800);
    return delta;
  }

  function onWheel(event) {
    if (!isHorizontalMode() || event.defaultPrevented) return;
    if (event.ctrlKey || event.metaKey || event.altKey) return;

    var dx = toPixelDelta(event.deltaX, event.deltaMode);
    var dy = toPixelDelta(event.deltaY, event.deltaMode);
    if (dy === 0 || Math.abs(dx) >= Math.abs(dy)) return;

    var now = window.performance ? window.performance.now() : Date.now();
    if (canScrollVertically(event.target, dy)) {
      lastVerticalScrollTime = now;
      wheelAccum = 0;
      return;
    }
    if (now - lastVerticalScrollTime < 260) {
      wheelAccum = 0;
      return;
    }

    var current = getCurrentIndex();
    if (dy < 0 && current <= 0 && strip.scrollLeft <= 1) {
      wheelAccum = 0;
      return;
    }
    if (
      dy > 0 &&
      current >= panels.length - 1 &&
      Math.ceil(strip.scrollLeft + strip.clientWidth) >= strip.scrollWidth - 1
    ) {
      wheelAccum = 0;
      return;
    }

    event.preventDefault();
    var sign = dy > 0 ? 1 : -1;
    var sincePrevious = now - lastWheelTime;
    lastWheelTime = now;
    if (stepping) {
      // The rest of the gesture that already moved a panel (trackpad inertia, a fast flick).
      if (now < wheelLockUntil || sincePrevious < 200) {
        return;
      }
      stepping = false;
      wheelAccum = 0;
    }
    if (sincePrevious > 200 || sign !== lastWheelSign) {
      wheelAccum = 0;
    }
    lastWheelSign = sign;
    wheelAccum += dy;

    if (Math.abs(wheelAccum) >= 50) {
      var instant = prefersReducedMotion();
      var base = now < slideEndsAt && targetIndex >= 0 ? targetIndex : current;
      var next = Math.max(0, Math.min(panels.length - 1, base + sign));
      wheelAccum = 0;
      stepping = true;
      wheelLockUntil = now + (instant ? 200 : 500);
      slideEndsAt = now + (instant ? 0 : 650);
      targetIndex = next;
      if (next !== current) {
        scrollToPanel(next, instant);
      }
    }
  }

  function syncWheelBinding() {
    var shouldBind = supportsSnap && wideQuery.matches;
    if (shouldBind && !wheelAttached) {
      strip.addEventListener('wheel', onWheel, { passive: false });
      wheelAttached = true;
    } else if (!shouldBind && wheelAttached) {
      strip.removeEventListener('wheel', onWheel);
      wheelAttached = false;
    }
  }

  strip.addEventListener(
    'scroll',
    function () {
      if (supportsSnap && wideQuery && wideQuery.matches) {
        var activeIndex = getCurrentIndex();
        updateIndicators(activeIndex);
        // The navigation hint is only needed until the first panel change.
        if (hintVisible && activeIndex !== 0) {
          hideNavHint();
        }
      }
    },
    { passive: true }
  );

  window.addEventListener(
    'resize',
    function () {
      syncWheelBinding();
      if (isHorizontalMode()) {
        resetDocumentScroll();
        updateIndicators(getCurrentIndex());
      }
    },
    { passive: true }
  );

  if (wideQuery) {
    if (typeof wideQuery.addEventListener === 'function') {
      wideQuery.addEventListener('change', syncWheelBinding);
    } else if (typeof wideQuery.addListener === 'function') {
      wideQuery.addListener(syncWheelBinding);
    }
  }

  document.addEventListener('keydown', function (event) {
    if (!isHorizontalMode() || event.defaultPrevented) return;
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;

    var tag = event.target && event.target.tagName;
    if (
      tag === 'INPUT' ||
      tag === 'TEXTAREA' ||
      tag === 'SELECT' ||
      (event.target && event.target.isContentEditable)
    ) {
      return;
    }

    var current = getCurrentIndex();
    var targetIndex = -1;
    if (event.key === 'ArrowRight' || event.key === 'PageDown') {
      targetIndex = current + 1;
    } else if (event.key === 'ArrowLeft' || event.key === 'PageUp') {
      targetIndex = current - 1;
    } else if (event.key === 'Home') {
      targetIndex = 0;
    } else if (event.key === 'End') {
      targetIndex = panels.length - 1;
    }

    if (targetIndex >= 0 && targetIndex < panels.length && targetIndex !== current) {
      event.preventDefault();
      scrollToPanel(targetIndex, false);
    } else if (
      (event.key === 'Home' && current === 0) ||
      (event.key === 'End' && current === panels.length - 1)
    ) {
      event.preventDefault();
      scrollToPanel(current, false);
    }
  });

  for (var i = 0; i < hashLinks.length; i += 1) {
    hashLinks[i].addEventListener('click', function (event) {
      var href = this.getAttribute('href');
      if (!href || !isHorizontalMode()) return;
      if (href === '#main') {
        event.preventDefault();
        strip.focus({ preventScroll: true });
        return;
      }
      var idx = resolveHashIndex(href);
      if (idx < 0) return;
      event.preventDefault();
      scrollToPanel(idx, false);
      if (window.history && typeof window.history.pushState === 'function') {
        window.history.pushState(null, '', '#' + panels[idx].id);
      }
      panels[idx].focus({ preventScroll: true });
    });
  }

  window.addEventListener('hashchange', function () {
    if (!isHorizontalMode()) return;
    var idx = resolveHashIndex(window.location.hash);
    if (idx >= 0) {
      scrollToPanel(idx, false);
    }
  });

  // Big edge arrows: one panel per click, hidden on the first/last panel.
  if (prevArrow) {
    prevArrow.addEventListener('click', function () {
      if (!isHorizontalMode()) return;
      scrollToPanel(getCurrentIndex() - 1, false);
    });
  }
  if (nextArrow) {
    nextArrow.addEventListener('click', function () {
      if (!isHorizontalMode()) return;
      scrollToPanel(getCurrentIndex() + 1, false);
    });
  }

  // Clickable progress bar: jump to the panel nearest the click.
  if (progressTrack) {
    progressTrack.addEventListener('click', function (event) {
      if (!isHorizontalMode()) return;
      var rect = progressTrack.getBoundingClientRect();
      var ratio = rect.width ? (event.clientX - rect.left) / rect.width : 0;
      scrollToPanel(Math.round(ratio * (panels.length - 1)), false);
    });
  }

  // "How to turn the pages" hint: once per visitor (localStorage), or once per
  // load when storage is unavailable; hides on first panel change, on close,
  // or after 10 s.
  var HINT_KEY = 'mm-site-nav-hint';
  var hintVisible = false;
  var hintTimer = 0;

  function hintWasSeen() {
    try {
      return window.localStorage.getItem(HINT_KEY) === '1';
    } catch (err) {
      return false;
    }
  }

  function markHintSeen() {
    try {
      window.localStorage.setItem(HINT_KEY, '1');
    } catch (err) {
      // No storage: the hint falls back to once per load, nothing to persist.
    }
  }

  function showNavHint() {
    if (!navHint || hintVisible || hintWasSeen()) return;
    if (!isHorizontalMode() || getCurrentIndex() !== 0) return;
    hintVisible = true;
    markHintSeen();
    navHint.classList.add('is-visible');
    hintTimer = window.setTimeout(hideNavHint, 10000);
  }

  function hideNavHint() {
    if (!hintVisible) return;
    hintVisible = false;
    if (hintTimer) {
      window.clearTimeout(hintTimer);
      hintTimer = 0;
    }
    navHint.classList.remove('is-visible');
  }

  if (navHintClose) {
    navHintClose.addEventListener('click', hideNavHint);
  }

  // Opening sequence: a short comic intro on the hero panel only (horizontal
  // mode, first panel, motion allowed). A click or a key ends it at once.
  var INTRO_MS = 1800;
  var introActive = false;
  var introTimer = 0;

  function endIntro() {
    if (!introActive) return;
    introActive = false;
    if (introTimer) {
      window.clearTimeout(introTimer);
      introTimer = 0;
    }
    document.documentElement.classList.remove('intro');
    document.removeEventListener('pointerdown', endIntro, true);
    document.removeEventListener('keydown', endIntro, true);
  }

  function startIntro() {
    if (introActive || !isHorizontalMode() || prefersReducedMotion()) return;
    if (getCurrentIndex() !== 0) return;
    introActive = true;
    document.documentElement.classList.add('intro');
    document.addEventListener('pointerdown', endIntro, true);
    document.addEventListener('keydown', endIntro, true);
    introTimer = window.setTimeout(endIntro, INTRO_MS);
  }

  syncWheelBinding();
  var initialIndex = resolveHashIndex(window.location.hash);
  if (isHorizontalMode()) {
    resetDocumentScroll();
    if (initialIndex >= 0) {
      scrollToPanel(initialIndex, true);
    } else {
      updateIndicators(0);
    }
    showNavHint();
    startIntro();
  } else {
    updateIndicators(0);
  }
})();

