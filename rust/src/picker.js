// "Block element": the user points at an ad and hides it for good. The
// element under the mouse is outlined; a click picks it (the page does not
// get that click), and a bar offers to hide it, to take a bigger or smaller
// piece of the page, or to cancel. Hiding sends the element's CSS selector
// to Idlerx (chrome.webview.postMessage), which keeps it as a rule of the
// site. Running this again (or Esc) cancels.
(() => {
  if (window.__idlerxPicker) {
    window.__idlerxPicker.stop();
    return;
  }
  const pt = (navigator.language || '').toLowerCase().startsWith('pt');
  const T = pt
    ? { point: 'Clique no anúncio que quer esconder · Esc cancela', hide: 'Esconder', bigger: 'Maior', smaller: 'Menor', cancel: 'Cancelar' }
    : { point: 'Click the ad you want to hide · Esc cancels', hide: 'Hide', bigger: 'Bigger', smaller: 'Smaller', cancel: 'Cancel' };
  const top = 2147483647;
  const box = document.createElement('div');
  box.style.cssText = `position:fixed;z-index:${top};pointer-events:none;background:rgba(20,184,166,.22);outline:2px solid #14b8a6;display:none`;
  const bar = document.createElement('div');
  bar.style.cssText = `position:fixed;z-index:${top};left:50%;top:10px;transform:translateX(-50%);background:#18181b;color:#fafafa;font:13px system-ui,sans-serif;padding:8px 12px;border-radius:8px;box-shadow:0 4px 18px rgba(0,0,0,.45);display:flex;gap:8px;align-items:center;max-width:90vw`;
  const text = document.createElement('span');
  text.style.cssText = 'overflow:hidden;text-overflow:ellipsis;white-space:nowrap;max-width:50vw';
  bar.append(text);
  const button = (label, primary, on) => {
    const b = document.createElement('button');
    b.textContent = label;
    b.style.cssText = `font:inherit;border:0;border-radius:6px;padding:4px 10px;cursor:pointer;color:#fafafa;background:${primary ? '#14b8a6' : '#3f3f46'}`;
    b.addEventListener('click', (e) => { e.preventDefault(); e.stopPropagation(); on(); });
    return b;
  };
  const ours = (el) => el === bar || bar.contains(el) || el === box;

  // a selector that finds this element only (ids and classes that look
  // stable, then the path from a parent)
  const esc = (s) => CSS.escape(s);
  const stable = (s) => s && s.length < 40 && !/\d{4,}/.test(s);
  const unique = (sel) => { try { return document.querySelectorAll(sel).length === 1; } catch (_) { return false; } };
  const selectorOf = (el) => {
    const parts = [];
    for (let e = el; e && e.nodeType === 1 && e !== document.documentElement && e !== document.body; e = e.parentElement) {
      if (e.id && stable(e.id)) {
        parts.unshift('#' + esc(e.id));
        const s = parts.join(' > ');
        if (unique(s)) return s;
        continue;
      }
      let part = e.localName;
      const classes = [...e.classList].filter(stable).slice(0, 3);
      part += classes.map((c) => '.' + esc(c)).join('');
      const same = e.parentElement ? [...e.parentElement.children].filter((c) => c.localName === e.localName) : [];
      if (!classes.length && same.length > 1) part += `:nth-of-type(${same.indexOf(e) + 1})`;
      parts.unshift(part);
      const s = parts.join(' > ');
      if (unique(s)) return s;
      if (parts.length >= 8) break;
    }
    return parts.join(' > ');
  };

  let target = null;
  let picked = false;
  const draw = () => {
    if (!target) { box.style.display = 'none'; return; }
    const r = target.getBoundingClientRect();
    Object.assign(box.style, { display: 'block', left: r.left + 'px', top: r.top + 'px', width: r.width + 'px', height: r.height + 'px' });
  };
  const point = () => {
    picked = false;
    text.textContent = T.point;
    bar.replaceChildren(text, button(T.cancel, false, stop));
  };
  const offer = () => {
    picked = true;
    text.textContent = selectorOf(target);
    bar.replaceChildren(
      text,
      button(T.hide, true, hide),
      button(T.bigger, false, () => { if (target.parentElement && target.parentElement !== document.body) { target = target.parentElement; draw(); offer(); } }),
      button(T.smaller, false, () => { const c = [...target.children].find((c) => c.getBoundingClientRect().width > 0); if (c) { target = c; draw(); offer(); } }),
      button(T.cancel, false, stop),
    );
  };
  const hide = () => {
    const selector = selectorOf(target);
    const style = document.createElement('style');
    style.textContent = `${selector}{display:none!important}`;
    (document.head || document.documentElement).appendChild(style);
    try { window.chrome.webview.postMessage({ __idlerxPick: { selector, url: location.href } }); } catch (_) {}
    stop();
  };

  const onMove = (e) => {
    if (picked) return;
    const el = document.elementFromPoint(e.clientX, e.clientY);
    if (!el || ours(el)) return;
    target = el;
    draw();
  };
  // the page gets none of the clicks while picking
  const swallow = (e) => {
    if (ours(e.target)) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.type === 'click' && !picked && target) offer();
  };
  const onKey = (e) => {
    if (e.key === 'Escape') { e.preventDefault(); e.stopImmediatePropagation(); stop(); }
  };
  const kinds = ['pointerdown', 'pointerup', 'mousedown', 'mouseup', 'click', 'dblclick', 'contextmenu', 'touchstart'];
  function stop() {
    removeEventListener('mousemove', onMove, true);
    for (const k of kinds) removeEventListener(k, swallow, true);
    removeEventListener('keydown', onKey, true);
    box.remove();
    bar.remove();
    delete window.__idlerxPicker;
  }
  addEventListener('mousemove', onMove, true);
  for (const k of kinds) addEventListener(k, swallow, true);
  addEventListener('keydown', onKey, true);
  document.documentElement.append(box, bar);
  point();
  window.__idlerxPicker = { stop };
})();
