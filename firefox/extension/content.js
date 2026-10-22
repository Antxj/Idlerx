// Isolated content script, in every document and iframe: the link between
// the page (page.js) and Idlerx (through background.js), plus the account's
// own saved passwords.
(() => {
  const toPage = (state) => window.postMessage({ __idlerxIn: state }, '*');

  // state of the account (hidden, recording), now and when it changes
  browser.runtime.sendMessage({ t: 'state' }).then((s) => s && toPage(s), () => {});
  browser.runtime.onMessage.addListener((m) => {
    if (m && m.state) toPage(m.state);
  });

  // recorded clicks and keys (only the top document reports them)
  window.addEventListener('message', (e) => {
    const d = e.data;
    if (e.source !== window || !d || !d.__idlerxOut) return;
    if (d.__idlerxOut.rec && window.top === window) {
      browser.runtime.sendMessage({ t: 'rec', data: d.__idlerxOut.rec }).catch(() => {});
    }
  });

  // keys that belong to Idlerx: F11 (focus mode) and Esc (leaves it)
  window.addEventListener('keydown', (e) => {
    if (!e.isTrusted) return;
    if (e.key === 'F11') {
      e.preventDefault();
      e.stopImmediatePropagation();
      browser.runtime.sendMessage({ t: 'key', key: 'F11' }).catch(() => {});
    } else if (e.key === 'Escape') {
      browser.runtime.sendMessage({ t: 'key', key: 'Escape' }).catch(() => {});
    }
  }, true);

  // the account got the mouse or keyboard (Idlerx selects it)
  let lastFocus = 0;
  const focused = () => {
    const now = Date.now();
    if (now - lastFocus < 1000) return;
    lastFocus = now;
    browser.runtime.sendMessage({ t: 'focus' }).catch(() => {});
  };
  window.addEventListener('mousedown', (e) => { if (e.isTrusted) focused(); }, true);
  window.addEventListener('focus', focused, true);

  // ---- saved passwords of this account (kept by Idlerx, never by Firefox) ----
  if (!/^https?:$/.test(location.protocol)) return;
  const origin = location.origin;
  let logins = null;
  let asked = false;
  const setValue = (input, value) => {
    const desc = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
    desc.set.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  };
  // the user name field that goes with a password field
  const userField = (pw) => {
    const scope = pw.form || document;
    const fields = [...scope.querySelectorAll('input')].filter((i) =>
      ['text', 'email', 'tel', ''].includes(i.type) && i.offsetParent !== null);
    const before = fields.filter((i) => i.compareDocumentPosition(pw) & Node.DOCUMENT_POSITION_FOLLOWING);
    return before[before.length - 1] || fields[0] || null;
  };
  const fill = (pw) => {
    if (!logins || !logins.length || pw.dataset.idlerx) return;
    pw.dataset.idlerx = '1';
    const user = userField(pw);
    if (logins.length === 1) {
      if (user && !user.value) setValue(user, logins[0].username);
      if (!pw.value) setValue(pw, logins[0].password);
      return;
    }
    // several logins of this account: its user names are offered in the
    // field, and choosing one fills its password
    if (!user) return;
    const list = document.createElement('datalist');
    list.id = 'idlerx-logins-' + Math.random().toString(36).slice(2);
    for (const l of logins) {
      const o = document.createElement('option');
      o.value = l.username;
      list.appendChild(o);
    }
    document.documentElement.appendChild(list);
    user.setAttribute('list', list.id);
    user.addEventListener('input', () => {
      const l = logins.find((x) => x.username === user.value);
      if (l) setValue(pw, l.password);
    });
  };
  const scan = () => {
    const pws = [...document.querySelectorAll('input[type=password]')];
    if (!pws.length) return;
    if (logins) { pws.forEach(fill); return; }
    if (asked) return;
    asked = true;
    browser.runtime.sendMessage({ t: 'logins', origin }).then((list) => {
      logins = Array.isArray(list) ? list : [];
      scan();
    }, () => { logins = []; });
  };
  // after a login, Idlerx offers to save it for this account
  let lastSent = '';
  const capture = (scope) => {
    const pw = [...(scope || document).querySelectorAll('input[type=password]')].find((i) => i.value);
    if (!pw) return;
    const user = userField(pw);
    const username = user ? user.value : '';
    const key = username + '\n' + pw.value;
    if (key === lastSent) return;
    lastSent = key;
    if (logins && logins.some((l) => l.username === username && l.password === pw.value)) return;
    browser.runtime.sendMessage({ t: 'login', origin, username, password: pw.value }).catch(() => {});
  };
  document.addEventListener('submit', (e) => capture(e.target), true);
  document.addEventListener('keydown', (e) => {
    if (e.isTrusted && e.key === 'Enter' && e.target && e.target.type === 'password') capture(e.target.form);
  }, true);
  document.addEventListener('click', (e) => {
    const b = e.target && e.target.closest && e.target.closest('button, input[type=submit], [role=button]');
    if (e.isTrusted && b) capture(b.form || b.closest('form') || document);
  }, true);
  // login forms that show up later: looked for at most twice a second, and
  // only during the first minute (games change the page all the time)
  const start = () => {
    scan();
    let queued = false;
    const watch = new MutationObserver(() => {
      if (queued) return;
      queued = true;
      setTimeout(() => {
        queued = false;
        scan();
        if (logins && logins.length && document.querySelector('input[type=password][data-idlerx]')) watch.disconnect();
      }, 500);
    });
    watch.observe(document.documentElement, { childList: true, subtree: true });
    setTimeout(() => watch.disconnect(), 60000);
  };
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start);
  else start();
})();
