// Injected by Idlerx into every document (iframes included) before the
// page's own scripts: limits how many times per second the page draws
// (requestAnimationFrame). Idle games do not need 60 drawings a second.
// Games that compute progress from the real time keep the same speed; only
// the drawing becomes less frequent. Limit 0 = no limit.
(() => {
  if (window.__idlerxFps) return;
  const nativeRaf = window.requestAnimationFrame.bind(window);
  const nativeCancel = window.cancelAnimationFrame.bind(window);
  const st = { fps: 0, next: 0, seq: 1e9, timers: new Map(), frames: new Map() };

  // the limit also goes to the iframes (games often draw inside one)
  st.set = (n) => {
    st.fps = n > 0 ? n : 0;
    for (let i = 0; i < window.frames.length; i++) {
      try { window.frames[i].postMessage({ __idlerxFps: st.fps }, '*'); } catch (e) {}
    }
  };
  Object.defineProperty(window, '__idlerxFps', { value: st });

  window.requestAnimationFrame = function (cb) {
    if (!st.fps) return nativeRaf(cb);
    const h = ++st.seq;
    const wait = Math.max(0, st.next - performance.now());
    st.timers.set(h, setTimeout(() => {
      st.timers.delete(h);
      st.frames.set(h, nativeRaf((t) => {
        st.frames.delete(h);
        const now = performance.now();
        if (now >= st.next) st.next = now + 1000 / st.fps;
        cb(t);
      }));
    }, wait));
    return h;
  };

  window.cancelAnimationFrame = function (h) {
    if (st.timers.has(h)) { clearTimeout(st.timers.get(h)); st.timers.delete(h); return; }
    if (st.frames.has(h)) { nativeCancel(st.frames.get(h)); st.frames.delete(h); return; }
    nativeCancel(h);
  };

  window.addEventListener('message', (e) => {
    const d = e.data;
    if (!d || typeof d !== 'object') return;
    if (typeof d.__idlerxFps === 'number' && e.source === window.parent && window.parent !== window) {
      st.set(d.__idlerxFps);
    } else if (d.__idlerxFpsAsk && e.source) {
      try { e.source.postMessage({ __idlerxFps: st.fps }, '*'); } catch (err) {}
    }
  });

  // an iframe created later asks the current limit
  if (window.parent !== window) {
    try { window.parent.postMessage({ __idlerxFpsAsk: 1 }, '*'); } catch (e) {}
  }
})();
