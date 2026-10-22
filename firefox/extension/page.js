// Runs in the page itself (main world), in every document and iframe, before
// the page's own scripts. Talks to content.js through window messages.
//
// 1. Keeps a hidden account's game running, cheaply: while Idlerx says the
//    account is not on screen, the page's animation callbacks run from
//    timers a few times per second instead of every frame (the game logic
//    keeps going, almost nothing is drawn), and the page is told it is
//    visible (many games pause themselves when hidden). Without "keep games
//    running in the background", they wait until the account is shown.
// 2. Records the user's clicks and keys for a macro, while Idlerx asks for
//    it: only watches, never blocks or changes anything. An iframe passes its
//    events to its parent, which adds the iframe's position, so the top
//    document reports every point relative to the whole page.
(() => {
  if (window.__idlerx) return;
  /** Frames per second of a hidden account. */
  const HIDDEN_FPS = 5;
  const nativeRaf = window.requestAnimationFrame.bind(window);
  const nativeCancel = window.cancelAnimationFrame.bind(window);
  const nativeSetTimeout = window.setTimeout.bind(window);
  const nativeClearTimeout = window.clearTimeout.bind(window);
  const st = { hidden: false, awake: true, rec: false, seq: 1e9, nextHidden: 0 };
  /** handle -> { cb, timer, frame } of every callback still waiting */
  const pending = new Map();

  // the page always hears "visible": hidden accounts keep playing
  const proto = Document.prototype;
  const hiddenDesc = Object.getOwnPropertyDescriptor(proto, 'hidden');
  const stateDesc = Object.getOwnPropertyDescriptor(proto, 'visibilityState');
  try {
    if (hiddenDesc) Object.defineProperty(proto, 'hidden', { configurable: true, enumerable: true, get() { return false; } });
    if (stateDesc) Object.defineProperty(proto, 'visibilityState', { configurable: true, enumerable: true, get() { return 'visible'; } });
  } catch (e) {}
  document.addEventListener('visibilitychange', (ev) => ev.stopImmediatePropagation(), true);

  const run = (h, e, t) => { pending.delete(h); e.cb(t); };
  const schedule = (h, e) => {
    if (st.hidden && !st.awake) return; // asleep: runs again when shown
    if (st.hidden) {
      const wait = Math.max(0, st.nextHidden - performance.now());
      e.timer = nativeSetTimeout(() => {
        e.timer = 0;
        const now = performance.now();
        if (now >= st.nextHidden) st.nextHidden = now + 1000 / HIDDEN_FPS;
        run(h, e, now);
      }, wait);
    } else {
      e.frame = nativeRaf((t) => { e.frame = 0; run(h, e, t); });
    }
  };
  const reschedule = () => {
    for (const [h, e] of pending) {
      if (e.timer) { nativeClearTimeout(e.timer); e.timer = 0; }
      if (e.frame) { nativeCancel(e.frame); e.frame = 0; }
      schedule(h, e);
    }
  };
  window.requestAnimationFrame = function (cb) {
    const h = ++st.seq;
    const e = { cb, timer: 0, frame: 0 };
    pending.set(h, e);
    schedule(h, e);
    return h;
  };
  window.cancelAnimationFrame = function (h) {
    const e = pending.get(h);
    if (!e) { nativeCancel(h); return; }
    if (e.timer) nativeClearTimeout(e.timer);
    if (e.frame) nativeCancel(e.frame);
    pending.delete(h);
  };
  Object.defineProperty(window, '__idlerx', { value: true });

  // ---- recording ----
  const top = window.parent === window;
  const report = (ev) => {
    if (top) {
      if (ev.kind === 'click') {
        ev.x = ev.x / Math.max(1, window.innerWidth);
        ev.y = ev.y / Math.max(1, window.innerHeight);
      }
      window.postMessage({ __idlerxOut: { rec: ev } }, '*');
    } else {
      try { window.parent.postMessage({ __idlerxRecEvent: ev }, '*'); } catch (e) {}
    }
  };
  window.addEventListener('mousedown', (e) => {
    if (!st.rec || !e.isTrusted || e.button === 1 || e.button > 2) return;
    report({ kind: 'click', x: e.clientX, y: e.clientY, button: e.button === 2 ? 'right' : 'left', detail: e.detail, t: Date.now() });
  }, true);
  window.addEventListener('keydown', (e) => {
    if (!st.rec || !e.isTrusted || e.repeat) return;
    report({ kind: 'key', code: e.code, t: Date.now() });
  }, true);

  window.addEventListener('message', (e) => {
    const d = e.data;
    if (!d || typeof d !== 'object') return;
    if (d.__idlerxIn && e.source === window) {
      // from content.js (Idlerx)
      const s = d.__idlerxIn;
      const awake = s.awake !== false;
      if (typeof s.hidden === 'boolean' && (s.hidden !== st.hidden || awake !== st.awake)) {
        st.hidden = s.hidden; st.awake = awake; reschedule();
      }
      if (typeof s.rec === 'boolean') st.rec = s.rec;
    } else if (d.__idlerxRecEvent && st.rec && e.source && e.source !== window) {
      // from one of our iframes: its position is added to the point
      const ev = d.__idlerxRecEvent;
      if (ev.kind === 'click') {
        const frame = [...document.querySelectorAll('iframe, frame')].find((f) => f.contentWindow === e.source);
        if (!frame) return;
        const r = frame.getBoundingClientRect();
        ev.x = Number(ev.x) + r.left + frame.clientLeft;
        ev.y = Number(ev.y) + r.top + frame.clientTop;
      }
      report(ev);
    }
  });
})();
