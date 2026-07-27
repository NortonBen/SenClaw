// YouTube — SenClaw · MV3 service worker.
//
// Bridges the SenClaw YouTube app (a local daemon on 127.0.0.1) to the user's
// signed-in YouTube session. Two jobs:
//   1. Report auth state (is a SAPISID cookie present → is the user signed in).
//   2. Proxy `yt_fetch` RPCs: issue an authenticated InnerTube fetch from this
//      extension's context (SAPISIDHASH auth + cookies + DNR-set Origin/Referer),
//      which BotGuard treats as a genuine same-origin browser request.
//
// Ports are configurable (the daemon assigns the app's HTTP port dynamically);
// set them in the popup. Defaults: WS 9223, HTTP 4491.

const DEFAULTS = { wsPort: 9223, httpPort: 4491 };
const ORIGIN = 'https://www.youtube.com';

let ws = null;
let callbackSecret = null;
let reconnectDelay = 1000;
const BACKOFF = [1000, 2000, 4000, 8000, 16000, 30000];

// ---- settings ----

async function settings() {
  const s = await chrome.storage.local.get(['wsPort', 'httpPort']);
  return { wsPort: s.wsPort || DEFAULTS.wsPort, httpPort: s.httpPort || DEFAULTS.httpPort };
}

// ---- auth helpers ----

async function getCookie(name) {
  try {
    const c = await chrome.cookies.get({ url: ORIGIN, name });
    return c?.value || null;
  } catch {
    return null;
  }
}

// SAPISID is the account cookie; some accounts only expose the __Secure- variants.
async function getSapisid() {
  return (
    (await getCookie('SAPISID')) ||
    (await getCookie('__Secure-3PAPISID')) ||
    (await getCookie('__Secure-1PAPISID'))
  );
}

// Authorization: SAPISIDHASH <ts>_<sha1(ts + " " + SAPISID + " " + origin)>
async function sapisidHash(sapisid, origin) {
  const ts = Math.floor(Date.now() / 1000);
  const data = new TextEncoder().encode(`${ts} ${sapisid} ${origin}`);
  const buf = await crypto.subtle.digest('SHA-1', data);
  const hex = [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
  return `SAPISIDHASH ${ts}_${hex}`;
}

// The live InnerTube context (clientVersion / visitorData) scraped from the page
// by content.js/injected.js — keeps our proxied payload consistent with the
// session's own requests.
async function getContext() {
  const s = await chrome.storage.local.get('ytContext');
  return s.ytContext || null;
}

async function pushAuthState() {
  const sapisid = await getSapisid();
  const ctx = await getContext();
  sendEvent({
    type: 'token_captured',
    data: {
      hasSapisid: !!sapisid,
      loggedIn: !!sapisid,
      clientVersion: ctx?.clientVersion || null,
      updatedAt: Date.now(),
    },
  });
}

// ---- the proxied InnerTube fetch ----

async function ytFetch(params) {
  const { url, method = 'POST', body } = params || {};
  if (!url) throw new Error('yt_fetch: thiếu url');

  const headers = { 'Content-Type': 'application/json' };
  const sapisid = await getSapisid();
  if (sapisid) {
    headers['Authorization'] = await sapisidHash(sapisid, ORIGIN);
    headers['X-Goog-AuthUser'] = '0';
  }

  // Overlay the live page's fresher client version / visitor data when we have it.
  const sendBody = body ? JSON.parse(JSON.stringify(body)) : {};
  const ctx = await getContext();
  if (ctx && sendBody.context && sendBody.context.client) {
    if (ctx.clientVersion) sendBody.context.client.clientVersion = ctx.clientVersion;
    if (ctx.visitorData) sendBody.context.client.visitorData = ctx.visitorData;
  }

  // credentials:'include' + host_permissions → the session cookies ride along;
  // rules.json (DNR) sets Origin/Referer (forbidden fetch headers) so the call
  // looks same-origin to YouTube.
  const resp = await fetch(url, {
    method,
    headers,
    body: JSON.stringify(sendBody),
    credentials: 'include',
  });
  let json = null;
  try {
    json = await resp.json();
  } catch {
    /* non-JSON (e.g. an HTML challenge page) → leave null */
  }
  return { httpStatus: resp.status, json };
}

// ---- UI remote control (trusted CDP input via chrome.debugger) ----
//
// For surfaces InnerTube has no API for (e.g. the community-post composer) we
// drive the real page. Input goes through chrome.debugger's Input.* domain so the
// events are TRUSTED — synthetic DOM events from a content script are ignored by
// YouTube's editors. Movement/typing is paced like a human's.

let uiTabId = null;
const attachedTabs = new Set();

// MV3 suspends idle service workers, wiping module state. The UI flow spans
// several RPCs (open → snapshot → type → submit), so the driven tab id MUST
// survive a restart — otherwise a mid-flow snapshot would silently open a fresh
// page and destroy the half-filled composer.
async function setUiTab(id) {
  uiTabId = id;
  try {
    await chrome.storage.session.set({ uiTabId: id });
  } catch {
    await chrome.storage.local.set({ uiTabId: id });
  }
}

async function getUiTab() {
  if (uiTabId === null) {
    try {
      const s = await chrome.storage.session.get('uiTabId');
      uiTabId = s.uiTabId ?? null;
    } catch {
      const s = await chrome.storage.local.get('uiTabId');
      uiTabId = s.uiTabId ?? null;
    }
  }
  if (uiTabId === null) return null;
  // The tab may have been closed while we were suspended.
  try {
    await chrome.tabs.get(uiTabId);
    return uiTabId;
  } catch {
    await setUiTab(null);
    return null;
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const rnd = (a, b) => a + Math.random() * (b - a);

function cdp(tabId, method, params) {
  return new Promise((resolve, reject) => {
    chrome.debugger.sendCommand({ tabId }, method, params, (res) => {
      const err = chrome.runtime.lastError;
      if (err) reject(new Error(err.message));
      else resolve(res);
    });
  });
}

async function attachDebugger(tabId) {
  if (attachedTabs.has(tabId)) return;
  try {
    await chrome.debugger.attach({ tabId }, '1.3');
  } catch (e) {
    // After a service-worker restart `attachedTabs` is empty but Chrome may still
    // hold our attachment — that's fine, we can keep sending commands.
    const msg = String(e?.message || e);
    if (!/already attached/i.test(msg)) throw e;
  }
  attachedTabs.add(tabId);
}

async function detachDebugger(tabId) {
  if (!attachedTabs.has(tabId)) return;
  try {
    await chrome.debugger.detach({ tabId });
  } catch {
    /* already gone */
  }
  attachedTabs.delete(tabId);
}

chrome.tabs.onRemoved.addListener((tabId) => {
  attachedTabs.delete(tabId);
  if (uiTabId === tabId) setUiTab(null);
});

function waitForLoad(tabId, timeout = 20000) {
  return new Promise((resolve) => {
    const t0 = Date.now();
    const check = async () => {
      try {
        const t = await chrome.tabs.get(tabId);
        if (t.status === 'complete') return resolve(true);
      } catch {
        return resolve(false);
      }
      if (Date.now() - t0 > timeout) return resolve(false);
      setTimeout(check, 300);
    };
    setTimeout(check, 400);
  });
}

async function uiOpen(params) {
  const url = params?.url || 'https://www.youtube.com/';
  const origin = new URL(url).origin;

  // Reuse only a tab that is ALREADY on this origin (and preferably this exact
  // URL). Never navigate an unrelated YouTube tab away — that would hijack what
  // the user is watching.
  const sameOrigin = await chrome.tabs.query({ url: `${origin}/*` });
  let tab = sameOrigin.find((t) => t.url && t.url.startsWith(url));
  if (tab) {
    await chrome.tabs.update(tab.id, { active: true });
  } else {
    // Prefer re-using the tab we already drive, if it is on this origin.
    const current = await getUiTab();
    const driven = current && sameOrigin.find((t) => t.id === current);
    if (driven) {
      tab = driven;
      await chrome.tabs.update(tab.id, { url, active: true });
    } else {
      tab = await chrome.tabs.create({ url, active: true });
    }
  }

  await setUiTab(tab.id);
  await waitForLoad(tab.id);
  // Let the SPA settle before anyone snapshots.
  await sleep(rnd(700, 1400));
  return { tabId: tab.id, url };
}

// Runs IN the page: tag visible interactive elements with data-yt-idx.
function snapshotFn() {
  const SEL =
    'a,button,input,textarea,select,[role="button"],[role="textbox"],[contenteditable="true"],[tabindex]';
  const out = [];
  let idx = 0;
  document.querySelectorAll(SEL).forEach((el) => {
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return;
    if (r.bottom < 0 || r.top > innerHeight) return;
    const st = getComputedStyle(el);
    if (st.visibility === 'hidden' || st.display === 'none' || st.opacity === '0') return;
    el.setAttribute('data-yt-idx', String(idx));
    const tag = el.tagName.toLowerCase();
    const type = (el.getAttribute('type') || '').toLowerCase();
    // `editable` and `clickable` let the caller target precisely instead of
    // substring-matching text (which mis-fires on nav links like "Posts").
    const editable =
      el.isContentEditable ||
      tag === 'textarea' ||
      (tag === 'input' && (type === '' || type === 'text' || type === 'search'));
    const role = el.getAttribute('role') || (el.isContentEditable ? 'textbox' : '');
    out.push({
      idx,
      tag,
      type,
      role,
      editable,
      clickable: tag === 'button' || role === 'button' || tag === 'a',
      name: (el.getAttribute('name') || el.id || '').toLowerCase(),
      label: (el.getAttribute('aria-label') || el.placeholder || '').trim().slice(0, 120),
      text: (el.innerText || el.value || el.getAttribute('aria-label') || el.placeholder || '')
        .trim()
        .slice(0, 120),
      x: Math.round(r.x + r.width / 2),
      y: Math.round(r.y + r.height / 2),
      w: Math.round(r.width),
      h: Math.round(r.height),
      area: Math.round(r.width * r.height),
    });
    idx++;
  });
  return { url: location.href, title: document.title, count: out.length, elements: out };
}

async function uiSnapshot() {
  // Deliberately does NOT open a page on its own: silently navigating mid-flow
  // would wipe a half-filled composer. The caller must yt_ui_open first.
  const tabId = await getUiTab();
  if (tabId === null) throw new Error('chưa có tab nào đang được điều khiển — gọi yt_ui_open trước');
  const [res] = await chrome.scripting.executeScript({ target: { tabId }, func: snapshotFn });
  return res?.result || { elements: [] };
}

// Resolve an element's on-screen centre (scrolling it into view first). The
// settle delay matters: reading the rect mid smooth-scroll gives stale coords.
async function coordsOf(tabId, index) {
  const [res] = await chrome.scripting.executeScript({
    target: { tabId },
    args: [index],
    func: (i) =>
      new Promise((resolve) => {
        const el = document.querySelector(`[data-yt-idx="${i}"]`);
        if (!el) return resolve(null);
        el.scrollIntoView({ block: 'center' });
        setTimeout(() => {
          const r = el.getBoundingClientRect();
          resolve({ x: Math.round(r.x + r.width / 2), y: Math.round(r.y + r.height / 2) });
        }, 260);
      }),
  });
  return res?.result || null;
}

async function humanClick(tabId, x, y) {
  // Approach from a plausible offset with an ease-in-out path + jitter.
  const sx = x - rnd(40, 120);
  const sy = y - rnd(40, 120);
  for (let i = 1; i <= 6; i++) {
    const t = i / 6;
    const e = t < 0.5 ? 2 * t * t : -1 + (4 - 2 * t) * t;
    await cdp(tabId, 'Input.dispatchMouseEvent', {
      type: 'mouseMoved',
      x: sx + (x - sx) * e + rnd(-1, 1),
      y: sy + (y - sy) * e + rnd(-1, 1),
      button: 'none',
      clickCount: 0,
    });
    await sleep(rnd(12, 30));
  }
  await cdp(tabId, 'Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'left', clickCount: 1 });
  await sleep(rnd(40, 110));
  await cdp(tabId, 'Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'left', clickCount: 1 });
}

async function humanType(tabId, text) {
  for (const ch of String(text)) {
    await cdp(tabId, 'Input.dispatchKeyEvent', { type: 'char', text: ch });
    await sleep(rnd(40, 150));
  }
}

const KEYS = {
  Enter: { key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 },
  Tab: { key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 },
  Escape: { key: 'Escape', code: 'Escape', windowsVirtualKeyCode: 27 },
  Backspace: { key: 'Backspace', code: 'Backspace', windowsVirtualKeyCode: 8 },
};

async function pressKey(tabId, name) {
  const k = KEYS[name];
  if (!k) return humanType(tabId, name);
  await cdp(tabId, 'Input.dispatchKeyEvent', { type: 'keyDown', ...k });
  await sleep(rnd(30, 80));
  await cdp(tabId, 'Input.dispatchKeyEvent', { type: 'keyUp', ...k });
}

async function uiAct(params) {
  const { action, index, text, key } = params || {};
  const tabId = await getUiTab();
  if (tabId === null) throw new Error('chưa có tab nào đang được điều khiển — gọi yt_ui_open trước');
  await attachDebugger(tabId);

  if (action === 'click' || action === 'type') {
    const c = await coordsOf(tabId, index);
    if (!c) throw new Error(`không tìm thấy element idx=${index} — hãy snapshot lại`);
    await humanClick(tabId, c.x, c.y);
    if (action === 'type') {
      await sleep(rnd(120, 260));
      await humanType(tabId, text || '');
    }
  } else if (action === 'press') {
    await pressKey(tabId, key || 'Enter');
  } else {
    throw new Error(`action không hợp lệ: ${action}`);
  }
  return { ok: true, action, index: index ?? null };
}

// ---- RPC dispatch ----

async function handleCommand(msg) {
  const { id, method, params } = msg;
  try {
    let data;
    switch (method) {
      case 'yt_fetch':
        data = await ytFetch(params);
        break;
      case 'yt_ui_open':
        data = await uiOpen(params);
        break;
      case 'yt_ui_snapshot':
        data = await uiSnapshot();
        break;
      case 'yt_ui_act':
        data = await uiAct(params);
        break;
      case 'yt_ui_release': {
        const t = await getUiTab();
        if (t !== null) await detachDebugger(t);
        data = { released: true };
        break;
      }
      case 'ping':
        data = { pong: true };
        break;
      default:
        throw new Error(`unknown method: ${method}`);
    }
    reply(id, 'ok', data);
  } catch (e) {
    reply(id, 'error', null, String(e && e.message ? e.message : e));
  }
}

// Reply to the app. Prefer the HTTP callback (survives a WS drop mid-request),
// fall back to the WS socket.
async function reply(id, status, data, message) {
  const payload = { id, status, data, message: message || null, secret: callbackSecret };
  const { httpPort } = await settings();
  try {
    await fetch(`http://127.0.0.1:${httpPort}/api/ext/callback`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    return;
  } catch {
    /* fall through to WS */
  }
  wsSend({ id, status, data, message: message || null });
}

function sendEvent(obj) {
  wsSend(obj);
}

function wsSend(obj) {
  if (ws && ws.readyState === WebSocket.OPEN) {
    try {
      ws.send(JSON.stringify(obj));
    } catch {
      /* ignore */
    }
  }
}

// ---- WebSocket lifecycle ----

async function connect() {
  const { wsPort } = await settings();
  try {
    ws = new WebSocket(`ws://127.0.0.1:${wsPort}/`);
  } catch {
    scheduleReconnect();
    return;
  }

  ws.onopen = () => {
    reconnectDelay = 1000;
    wsSend({ type: 'extension_ready' });
    pushAuthState();
  };

  ws.onmessage = (ev) => {
    let msg;
    try {
      msg = JSON.parse(ev.data);
    } catch {
      return;
    }
    if (msg.type === 'callback_secret') {
      callbackSecret = msg.secret;
      return;
    }
    if (msg.type === 'pong') return;
    if (msg.id && msg.method) {
      handleCommand(msg);
    }
  };

  ws.onclose = () => {
    ws = null;
    scheduleReconnect();
  };
  ws.onerror = () => {
    try {
      ws && ws.close();
    } catch {
      /* ignore */
    }
  };
}

function scheduleReconnect() {
  const delay = reconnectDelay;
  reconnectDelay = BACKOFF[Math.min(BACKOFF.indexOf(reconnectDelay) + 1, BACKOFF.length - 1)] || 30000;
  setTimeout(connect, delay);
}

// MV3 suspends idle workers → an alarm re-pokes the connection + refreshes auth.
chrome.alarms.create('yt-keepalive', { periodInMinutes: 0.4 });
chrome.alarms.onAlarm.addListener((a) => {
  if (a.name !== 'yt-keepalive') return;
  if (!ws || ws.readyState > WebSocket.OPEN) connect();
  pushAuthState();
});

// Re-report auth whenever a relevant cookie changes.
chrome.cookies.onChanged.addListener((info) => {
  const n = info.cookie?.name || '';
  if (/APISID|SID|LOGIN_INFO/.test(n)) pushAuthState();
});

// Live InnerTube context from the page.
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg && msg.type === 'yt_context' && msg.data) {
    chrome.storage.local.set({ ytContext: msg.data });
    sendEvent({ type: 'yt_context', data: { clientVersion: msg.data.clientVersion || null } });
    sendResponse && sendResponse({ ok: true });
  }
  return false;
});

chrome.runtime.onInstalled.addListener(() => connect());
chrome.runtime.onStartup.addListener(() => connect());
connect();
