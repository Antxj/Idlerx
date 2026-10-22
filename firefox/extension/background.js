// Bridge between Idlerx and its Firefox (native messaging): one container and
// one window per account, the page's state for Idlerx, and Idlerx's commands
// for the page. Idlerx embeds each window in its own, found by the marker put
// in the window's title.
'use strict';

const HOST = 'io.github.antxj.idlerx';
/** uBlock Origin's fixed internal address in Idlerx's profiles (user.js). */
const UBO_UUID = '9c3c1f0e-1d6b-4f0e-9a8e-1d3e5b7a2c40';

let port = null;
let retry = 1000;
/** account id -> { windowId, tabId, cookieStoreId, hidden, awake, rec, private, closing } */
const accounts = new Map();
/** tab id -> account id (the account's own page) */
const byTab = new Map();
/** container -> account id (also the windows its pages open) */
const byStore = new Map();
/** pending questions to Idlerx: request id -> resolve */
const asks = new Map();
let nextAsk = 1;
let queue = Promise.resolve();

const send = (m) => { if (port) { try { port.postMessage(m); } catch (e) {} } };
const marker = (id) => '⁣idlerx:' + id + '⁣ ';
const containerName = (id, priv) => 'Idlerx ' + id + (priv ? ' ·p' : '');

function connect() {
  port = browser.runtime.connectNative(HOST);
  // one command at a time, in order (an open must not race a sync)
  port.onMessage.addListener((m) => {
    retry = 1000;
    queue = queue.then(() => command(m)).catch((e) => send({ ev: 'error', what: m.cmd + ': ' + String(e) }));
  });
  port.onDisconnect.addListener(() => {
    port = null;
    setTimeout(connect, retry);
    retry = Math.min(retry * 2, 30000);
  });
  send({ ev: 'hello', version: browser.runtime.getManifest().version });
}

async function container(id, priv) {
  const name = containerName(id, priv);
  const found = await browser.contextualIdentities.query({ name });
  if (priv) {
    // InPrivate: always a new, empty one
    for (const c of found) await browser.contextualIdentities.remove(c.cookieStoreId);
  } else if (found.length) {
    return found[0].cookieStoreId;
  }
  const c = await browser.contextualIdentities.create({ name, color: priv ? 'purple' : 'turquoise', icon: 'circle' });
  return c.cookieStoreId;
}

const stateOf = (a) => ({ hidden: !!a.hidden, awake: a.awake !== false, rec: !!a.rec, macro: !!a.macro });
const pushState = (a) => {
  if (a.tabId != null) browser.tabs.sendMessage(a.tabId, { state: stateOf(a) }).catch(() => {});
};

async function command(m) {
  const a = m.id != null ? accounts.get(m.id) : null;
  switch (m.cmd) {
    case 'open': {
      if (a) { await closeAccount(m.id, a); }
      // the container is named after the account's profile ("clear data"
      // gives the account a new, empty one)
      const cookieStoreId = await container(m.container || m.id, !!m.private);
      const w = await browser.windows.create({ url: m.url || 'about:blank', cookieStoreId, state: 'minimized', focused: false });
      const tabId = w.tabs[0].id;
      const acc = { windowId: w.id, tabId, cookieStoreId, hidden: true, awake: true, rec: false, private: !!m.private, closing: false };
      accounts.set(m.id, acc);
      byTab.set(tabId, m.id);
      byStore.set(cookieStoreId, m.id);
      await browser.windows.update(w.id, { titlePreface: marker(m.id) });
      if (m.muted) await browser.tabs.update(tabId, { muted: true });
      if (m.zoom && Math.abs(m.zoom - 1) > 0.001) await browser.tabs.setZoom(tabId, m.zoom).catch(() => {});
      send({ ev: 'opened', id: m.id, marker: marker(m.id).trim() });
      break;
    }
    case 'cookies': {
      // logins brought from the previous engine, into the account's container
      const storeId = await container(m.container, false);
      let ok = 0;
      for (const c of m.list || []) {
        try { await browser.cookies.set(Object.assign({}, c, { storeId })); ok++; } catch (e) {}
      }
      send({ ev: 'log', what: 'cookies trazidos: ' + ok + '/' + (m.list || []).length });
      break;
    }
    case 'storage': {
      // localStorage brought from the previous engine: kept until the
      // account opens that site (content.js puts it back)
      const storeId = await container(m.container, false);
      const key = 'restore:' + storeId;
      const all = (await browser.storage.local.get(key))[key] || {};
      const site = all[m.origin] || (all[m.origin] = {});
      for (const [k, v] of m.items || []) site[k] = m.append ? (site[k] || '') + v : v;
      await browser.storage.local.set({ [key]: all });
      break;
    }
    case 'close':
      if (a) await closeAccount(m.id, a);
      break;
    case 'sync': {
      // containers of deleted accounts (or of cleared data), and InPrivate
      // ones left over from a previous run, go with everything in them
      const keep = new Set(m.keep || []);
      const inUse = new Set([...accounts.values()].map((x) => x.cookieStoreId));
      for (const c of await browser.contextualIdentities.query({})) {
        if (!c.name.startsWith('Idlerx ') || inUse.has(c.cookieStoreId)) continue;
        const priv = c.name.endsWith(' ·p');
        const profile = c.name.slice('Idlerx '.length, priv ? -3 : undefined);
        if (priv || !keep.has(profile)) await browser.contextualIdentities.remove(c.cookieStoreId).catch(() => {});
      }
      break;
    }
    case 'nav':
      if (a) await browser.tabs.update(a.tabId, { url: m.url });
      break;
    case 'back':
      if (a) await browser.tabs.goBack(a.tabId).catch(() => {});
      break;
    case 'forward':
      if (a) await browser.tabs.goForward(a.tabId).catch(() => {});
      break;
    case 'reload':
      if (a) await browser.tabs.reload(a.tabId);
      break;
    case 'zoom':
      if (a) await browser.tabs.setZoom(a.tabId, m.zoom);
      break;
    case 'mute':
      if (a) await browser.tabs.update(a.tabId, { muted: !!m.muted });
      break;
    case 'hidden':
      if (a) { a.hidden = !!m.on; a.awake = m.awake !== false; pushState(a); }
      break;
    case 'record':
      if (a) { a.rec = !!m.on; pushState(a); }
      break;
    case 'macro':
      if (a) { a.macro = !!m.on; pushState(a); }
      break;
    case 'capture': {
      let data = null;
      if (a) {
        try {
          const url = await browser.tabs.captureTab(a.tabId, { format: 'png' });
          data = url.slice(url.indexOf(',') + 1);
        } catch (e) {}
      }
      send({ ev: 'captured', req: m.req, data });
      break;
    }
    case 'logins': {
      const done = asks.get(m.req);
      if (done) { asks.delete(m.req); done(m.list || []); }
      break;
    }
    case 'scheme':
      await browser.browserSettings.overrideContentColorScheme.set({ value: m.value }).catch(() => {});
      break;
    case 'dashboard':
      await browser.windows.create({ url: 'moz-extension://' + UBO_UUID + '/dashboard.html', type: 'normal' });
      break;
    case 'ublock':
      // uBlock Origin's own panel for the account's page
      if (a) {
        await browser.windows.create({
          url: 'moz-extension://' + UBO_UUID + '/popup-fenix.html?tabId=' + a.tabId,
          type: 'popup', width: 380, height: 640, left: m.x, top: m.y,
        });
      }
      break;
  }
}

async function closeAccount(id, a) {
  a.closing = true;
  accounts.delete(id);
  byTab.delete(a.tabId);
  await browser.windows.remove(a.windowId).catch(() => {});
  if (a.private) {
    byStore.delete(a.cookieStoreId);
    await browser.contextualIdentities.remove(a.cookieStoreId).catch(() => {});
  }
}

browser.tabs.onUpdated.addListener((tabId, change, tab) => {
  const id = byTab.get(tabId);
  if (id == null) return;
  if (change.url === undefined && change.title === undefined && change.status === undefined && change.audible === undefined) return;
  send({ ev: 'page', id, url: tab.url, title: tab.title, loading: tab.status === 'loading', audible: !!tab.audible });
});

browser.tabs.onZoomChange.addListener((z) => {
  const id = byTab.get(z.tabId);
  if (id != null) send({ ev: 'zoom', id, zoom: z.newZoomFactor });
});

browser.webNavigation.onCommitted.addListener((d) => {
  const id = byTab.get(d.tabId);
  if (id != null && d.frameId === 0) send({ ev: 'loaded', id });
});

browser.windows.onRemoved.addListener((windowId) => {
  for (const [id, a] of accounts) {
    if (a.windowId === windowId && !a.closing) {
      accounts.delete(id);
      byTab.delete(a.tabId);
      send({ ev: 'closed', id });
    }
  }
});

browser.runtime.onMessage.addListener((m, sender) => {
  const tab = sender.tab;
  if (!tab) return;
  const own = byTab.get(tab.id);
  // the account's own page, or a window it opened (same container)
  const id = own != null ? own : byStore.get(tab.cookieStoreId);
  if (id == null) return;
  switch (m.t) {
    case 'state': {
      const a = accounts.get(own);
      return Promise.resolve(a ? stateOf(a) : { hidden: false, rec: false });
    }
    case 'rec':
      if (own != null) send({ ev: 'rec', id, data: m.data });
      return;
    case 'key':
      if (own != null) send({ ev: 'key', id, key: m.key });
      return;
    case 'focus':
      if (own != null) send({ ev: 'focus', id });
      return;
    case 'logins':
      return new Promise((resolve) => {
        const req = nextAsk++;
        asks.set(req, resolve);
        send({ ev: 'logins', req, id, origin: m.origin });
        setTimeout(() => { if (asks.delete(req)) resolve([]); }, 5000);
      });
    case 'restore':
      // a site of the account opened: its localStorage from the previous
      // engine, once
      return (async () => {
        const key = 'restore:' + tab.cookieStoreId;
        const all = (await browser.storage.local.get(key))[key];
        if (!all || !all[m.origin]) return null;
        const site = all[m.origin];
        delete all[m.origin];
        if (Object.keys(all).length) await browser.storage.local.set({ [key]: all });
        else await browser.storage.local.remove(key);
        return site;
      })();
    case 'restored':
      // the page loads again, now with its saves
      send({ ev: 'log', what: 'save do site ' + m.origin + ' trazido (' + m.n + ' itens)' });
      browser.tabs.reload(tab.id).catch(() => {});
      return;
    case 'login':
      send({ ev: 'login', id, origin: m.origin, username: m.username, password: m.password });
      return;
  }
});

connect();
