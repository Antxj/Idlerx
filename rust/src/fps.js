// Injected by Idlerx into every document (iframes included) before the
// page's own scripts.
//
// 1. Limits how many times per second the page draws (requestAnimationFrame).
//    Idle games do not need 60 drawings a second. Games that compute
//    progress from the real time keep the same speed; only the drawing
//    becomes less frequent. Limit 0 = no limit.
// 3. Records the user's clicks and keys for a macro, while Idlerx asks for it
//    ("rec"): only watches, never blocks or changes anything. An iframe
//    passes its events to its parent, which adds the iframe's position, so
//    the top document reports every point relative to the whole page.
// 2. Keeps the game awake while the account is not on screen ("awake", on
//    with "Keep games running in the background"): a hidden page gets no
//    animation frames at all, so games that run their logic in the drawing
//    loop (Phaser and most canvas games) would freeze, and many also pause
//    themselves when the page says it is hidden. While hidden, the page is
//    told it is still visible and its animation callbacks run from timers,
//    at a few frames per second.
// 4. Stops the sound work of a muted account ("quiet"): the browser only
//    silences a muted page, its sound keeps being mixed. While quiet, the
//    page's audio contexts are suspended and sounds started meanwhile are
//    held back (short ones just end, music starts when the sound is back).
//    The page still sees its contexts running, so games neither stop nor
//    ask for a click to turn the sound on.
(() => {
  if (window.__idlerxFps) return;
  /** Frames per second of a hidden page that is kept awake. */
  const HIDDEN_FPS = 5;
  const nativeRaf = window.requestAnimationFrame.bind(window);
  const nativeCancel = window.cancelAnimationFrame.bind(window);
  // the browser's own timers, taken before the page loads: frameworks that
  // wrap timers (Angular) do not see the limiter's work as page activity
  const nativeSetTimeout = window.setTimeout.bind(window);
  const nativeClearTimeout = window.clearTimeout.bind(window);
  const nativeSetInterval = window.setInterval.bind(window);
  const st = { fps: 0, awake: true, next: 0, frameT: -1, nextHidden: 0, seq: 1e9, rate: -1 };
  /** Time between screen refreshes (ms), learned from the frames seen. */
  let vsync = 0;
  /** handle -> { cb, timer, frame } of every callback still waiting */
  const pending = new Map();

  // the real visibility, whatever the page is told
  const proto = Document.prototype;
  const hiddenDesc = Object.getOwnPropertyDescriptor(proto, 'hidden');
  const stateDesc = Object.getOwnPropertyDescriptor(proto, 'visibilityState');
  const realHidden = () => (hiddenDesc ? hiddenDesc.get.call(document) : false);
  try {
    if (hiddenDesc) {
      Object.defineProperty(proto, 'hidden', {
        configurable: true, enumerable: true,
        get() { return st.awake ? false : hiddenDesc.get.call(this); },
      });
    }
    if (stateDesc) {
      Object.defineProperty(proto, 'visibilityState', {
        configurable: true, enumerable: true,
        get() { return st.awake ? 'visible' : stateDesc.get.call(this); },
      });
    }
  } catch (e) {}

  // frames actually drawn per second (shown by Idlerx): distinct animation
  // frames whose callbacks ran during the last second
  let counted = 0, lastFrame = -1, windowStart = performance.now();
  const countFrame = (t) => {
    if (t !== lastFrame) { lastFrame = t; counted++; }
    const now = performance.now();
    if (now - windowStart >= 1000) {
      st.rate = Math.round(counted * 1000 / (now - windowStart));
      counted = 0; windowStart = now;
    }
  };
  // no animation for 3 s: nothing is being drawn
  nativeSetInterval(() => { if (performance.now() - windowStart > 3000) { st.rate = 0; counted = 0; windowStart = performance.now(); } }, 3000);

  const run = (h, e, t) => { pending.delete(h); countFrame(t); e.cb(t); };

  // waits for the next frame the right way for the current state
  const schedule = (h, e) => {
    if (st.awake && realHidden()) {
      // hidden: no real frames come, timers stand in for them
      const rate = Math.min(st.fps || HIDDEN_FPS, HIDDEN_FPS);
      const wait = Math.max(0, st.nextHidden - performance.now());
      e.timer = nativeSetTimeout(() => {
        e.timer = 0;
        const now = performance.now();
        if (now >= st.nextHidden) st.nextHidden = now + 1000 / rate;
        run(h, e, now);
      }, wait);
      return;
    }
    if (!st.fps) {
      e.frame = nativeRaf((t) => { e.frame = 0; run(h, e, t); });
      return;
    }
    // Limited: sleeps on a timer until shortly before the frame is due,
    // then follows the screen's own frames until it is. The schedule lives
    // in frame time and advances by whole intervals, so the limit lands
    // on screen refreshes (60 on a 180 Hz screen = every 3rd one) instead
    // of drifting above or below it.
    const interval = 1000 / st.fps;
    const wait = Math.max(0, st.next - performance.now() - (vsync || 8));
    e.timer = nativeSetTimeout(() => {
      e.timer = 0;
      let prev = 0;
      const onFrame = (t) => {
        e.frame = 0;
        if (prev && t > prev) vsync = vsync ? Math.min(vsync * 1.001, t - prev) : t - prev;
        prev = t;
        // other callbacks of a frame that was let through run in it too
        if (t !== st.frameT) {
          if (t < st.next - (vsync ? vsync / 2 : 2)) {
            e.frame = nativeRaf(onFrame);
            return;
          }
          st.frameT = t;
          st.next = t - st.next > interval ? t + interval : st.next + interval;
        }
        run(h, e, t);
      };
      e.frame = nativeRaf(onFrame);
    }, wait);
  };

  // the state changed: callbacks already waiting move to the right way
  const reschedule = () => {
    for (const [h, e] of pending) {
      if (e.timer) { nativeClearTimeout(e.timer); e.timer = 0; }
      if (e.frame) { nativeCancel(e.frame); e.frame = 0; }
      schedule(h, e);
    }
  };

  // the limit, "awake" and "quiet" also go to the iframes (games often draw
  // and play inside one)
  st.set = (n, awake, quiet) => {
    const fps = n > 0 ? n : 0;
    const wake = awake !== false;
    const changed = fps !== st.fps || wake !== st.awake;
    st.fps = fps;
    st.awake = wake;
    if (changed) reschedule();
    setQuiet(quiet === true);
    for (let i = 0; i < window.frames.length; i++) {
      try { window.frames[i].postMessage({ __idlerxFps: st.fps, awake: st.awake, quiet: st.quiet, rec: st.rec }, '*'); } catch (e) {}
    }
  };
  Object.defineProperty(window, '__idlerxFps', { value: st });

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

  // registered before any page script: sees the real change first, and the
  // page does not hear about it while it is kept awake
  document.addEventListener('visibilitychange', (ev) => {
    reschedule();
    if (st.awake) ev.stopImmediatePropagation();
  }, true);

  // ---- recording ----
  st.rec = false;
  const top = window.parent === window;
  st.record = (on) => {
    st.rec = !!on;
    for (let i = 0; i < window.frames.length; i++) {
      try { window.frames[i].postMessage({ __idlerxRec: st.rec }, '*'); } catch (e) {}
    }
  };
  // a recorded event at (x, y) in this document's pixels
  const report = (ev) => {
    if (top) {
      if (ev.kind === 'click') {
        ev.x = ev.x / Math.max(1, window.innerWidth);
        ev.y = ev.y / Math.max(1, window.innerHeight);
      }
      try { window.chrome.webview.postMessage({ __idlerxRec: ev }); } catch (e) {}
    } else {
      try { window.parent.postMessage({ __idlerxRecEvent: ev }, '*'); } catch (e) {}
    }
  };
  window.addEventListener('mousedown', (e) => {
    if (!st.rec || !e.isTrusted || e.button > 2 || e.button === 1) return;
    report({ kind: 'click', x: e.clientX, y: e.clientY, button: e.button === 2 ? 'right' : 'left', detail: e.detail, t: Date.now() });
  }, true);
  window.addEventListener('keydown', (e) => {
    if (!st.rec || !e.isTrusted || e.repeat) return;
    report({ kind: 'key', code: e.code, t: Date.now() });
  }, true);

  // ---- sound of a muted account ----
  st.quiet = false;
  const AC = window.AudioContext;
  /** The audio contexts of this document, and whether the page wants each one running. */
  const contexts = new Map();
  /** Sounds started while quiet: source -> its timer (0 = endless, started when the sound is back). */
  const held = new Map();
  let setQuiet = () => {};
  if (AC) {
    const proto = AC.prototype;
    const base = Object.getPrototypeOf(proto);
    const stateDesc = Object.getOwnPropertyDescriptor(base, 'state');
    const realState = (c) => stateDesc.get.call(c);
    const nResume = proto.resume, nSuspend = proto.suspend, nClose = proto.close;
    // what the page is told: running, as long as it did not suspend it itself
    const shown = (c) => {
      const real = realState(c);
      return real === 'suspended' && st.quiet && contexts.get(c) ? 'running' : real;
    };
    const quietly = (p) => { p.catch(() => {}); };
    const track = (c) => {
      contexts.set(c, true);
      // registered first: the page does not hear about the changes it is not told about
      let seen = shown(c);
      c.addEventListener('statechange', (ev) => {
        const now = shown(c);
        if (now === seen) ev.stopImmediatePropagation();
        seen = now;
      });
      if (st.quiet) quietly(nSuspend.call(c));
    };
    const wrapped = new Proxy(AC, {
      construct(t, args, nt) { const c = Reflect.construct(t, args, nt); track(c); return c; },
    });
    if (window.webkitAudioContext === AC) window.webkitAudioContext = wrapped;
    window.AudioContext = wrapped;
    try { Object.defineProperty(proto, 'constructor', { value: wrapped, writable: true, configurable: true }); } catch (e) {}
    try {
      Object.defineProperty(base, 'state', {
        configurable: true, enumerable: true,
        get() { return contexts.has(this) ? shown(this) : realState(this); },
      });
    } catch (e) {}
    proto.resume = function () {
      if (!contexts.has(this)) return nResume.call(this);
      contexts.set(this, true);
      return st.quiet ? Promise.resolve() : nResume.call(this);
    };
    proto.suspend = function () {
      if (contexts.has(this)) contexts.set(this, false);
      return nSuspend.call(this);
    };
    proto.close = function () {
      contexts.delete(this);
      return nClose.call(this);
    };

    // sounds started while quiet: a suspended context would keep them all
    // and play them at once when the sound comes back. (Buffer sources have
    // a start of their own.)
    const ended = (n) => { held.delete(n); try { n.dispatchEvent(new Event('ended')); } catch (e) {} };
    const natives = new Map();
    for (const name of ['AudioScheduledSourceNode', 'AudioBufferSourceNode']) {
      const pr = window[name] && window[name].prototype;
      if (!pr) continue;
      if (Object.prototype.hasOwnProperty.call(pr, 'start')) {
        const nStart = pr.start;
        natives.set(pr, nStart);
        pr.start = function (...args) {
          if (!st.quiet || !contexts.has(this.context) || held.has(this)) return nStart.apply(this, args);
          const buf = this.buffer;
          if (buf && !this.loop) {
            // a short sound: it ends when it would have ended
            const rate = this.playbackRate ? Math.abs(this.playbackRate.value) || 1 : 1;
            const len = args[2] !== undefined ? Number(args[2]) : buf.duration - (Number(args[1]) || 0);
            held.set(this, nativeSetTimeout(() => ended(this), Math.max(0, len / rate) * 1000));
          } else {
            held.set(this, 0);
          }
        };
      }
      if (Object.prototype.hasOwnProperty.call(pr, 'stop')) {
        const nStop = pr.stop;
        pr.stop = function (...args) {
          if (!held.has(this)) return nStop.apply(this, args);
          nativeClearTimeout(held.get(this));
          held.set(this, -1);
          nativeSetTimeout(() => ended(this), 0);
        };
      }
    }
    // the browser's own start of a source
    const nativeStart = (n) => {
      for (let pr = Object.getPrototypeOf(n); pr; pr = Object.getPrototypeOf(pr)) {
        if (natives.has(pr)) return natives.get(pr).call(n);
      }
    };

    setQuiet = (on) => {
      if (on === st.quiet) return;
      st.quiet = on;
      for (const [c, wanted] of contexts) {
        if (realState(c) === 'closed') { contexts.delete(c); continue; }
        if (wanted) quietly(on ? nSuspend.call(c) : nResume.call(c));
      }
      if (!on) {
        // music and other endless sounds start now
        for (const [n, timer] of held) {
          if (timer !== 0) continue;
          held.delete(n);
          try { nativeStart(n); } catch (e) {}
        }
      }
    };
  }

  window.addEventListener('message', (e) => {
    const d = e.data;
    if (!d || typeof d !== 'object') return;
    if (typeof d.__idlerxFps === 'number' && e.source === window.parent && window.parent !== window) {
      st.set(d.__idlerxFps, d.awake, d.quiet);
      if (typeof d.rec === 'boolean') st.record(d.rec);
    } else if (typeof d.__idlerxRec === 'boolean' && e.source === window.parent && !top) {
      st.record(d.__idlerxRec);
    } else if (d.__idlerxRecEvent && st.rec && e.source) {
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
    } else if (d.__idlerxFpsAsk && e.source) {
      try { e.source.postMessage({ __idlerxFps: st.fps, awake: st.awake, quiet: st.quiet, rec: st.rec }, '*'); } catch (err) {}
    }
  });

  // an iframe created later asks the current limit
  if (window.parent !== window) {
    try { window.parent.postMessage({ __idlerxFpsAsk: 1 }, '*'); } catch (e) {}
  }
})();
