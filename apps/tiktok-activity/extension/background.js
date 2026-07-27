// TikTok Activity Controller — MV3 service worker.
//
// Dials the app's ext-WS bridge (ws://127.0.0.1:9225/), receives {id, method,
// params}, runs each primitive against ONE controlled TikTok tab via
// chrome.debugger (Runtime.evaluate + Input.*, which — unlike page-injected
// eval — is not subject to the page CSP), and replies {id, result} / {id, error}.

const WS_PORT = 9225;
const WS_URL = `ws://127.0.0.1:${WS_PORT}/`;

let ws = null;
let callbackSecret = null;
let reconnectTimer = null;

// ---- control log (for the popup panel) ----
// A capped ring buffer mirrored into chrome.storage.local so the popup can show
// live activity and read history even if opened after the fact.
const LOG_KEY = "ctrlLog";
const LOG_MAX = 200;
let logBuf = [];
let logFlush = null;

async function loadLog() {
  const { [LOG_KEY]: saved } = await chrome.storage.local.get(LOG_KEY);
  if (Array.isArray(saved)) logBuf = saved;
}

// Running counters shown in the panel's "Kết nối" tab.
const stats = { cmdCount: 0, errCount: 0, connects: 0, connectedSince: 0 };
function saveStats() {
  chrome.storage.local.set({ stats });
}

function pushLog(kind, method, info, ok, ms) {
  logBuf.push({ t: Date.now(), kind, method, info: info || "", ok, ms });
  if (logBuf.length > LOG_MAX) logBuf = logBuf.slice(-LOG_MAX);
  if (kind === "cmd") {
    stats.cmdCount++;
    if (ok === false) stats.errCount++;
    saveStats();
  }
  // Throttle storage writes; the panel/popup also poll as a fallback.
  if (!logFlush) {
    logFlush = setTimeout(() => {
      logFlush = null;
      chrome.storage.local.set({ [LOG_KEY]: logBuf });
    }, 150);
  }
}

// Short, human-readable summary of a method's params for the log line.
function summarize(method, params) {
  const p = params || {};
  const clip = (s, n) => {
    s = String(s == null ? "" : s);
    return s.length > n ? s.slice(0, n) + "…" : s;
  };
  switch (method) {
    case "navigate":
      return clip(p.url, 80);
    case "eval":
      return clip(String(p.js || "").replace(/\s+/g, " "), 90);
    case "mouse_click":
      return `(${Math.round(p.x)}, ${Math.round(p.y)})`;
    case "wheel":
      return `Δ(${p.dx || 0}, ${p.dy || 0})`;
    case "type_text":
      return `"${clip(p.text, 40)}"`;
    case "press_key":
      return String(p.key || "");
    default:
      return "";
  }
}

// ---- controlled tab ----

async function getTargetTabId() {
  const { targetTabId } = await chrome.storage.local.get("targetTabId");
  if (targetTabId) {
    try {
      await chrome.tabs.get(targetTabId);
      return targetTabId;
    } catch (_) {
      /* tab gone; fall through */
    }
  }
  // Fall back to the active tiktok.com tab.
  const tabs = await chrome.tabs.query({ url: "*://*.tiktok.com/*" });
  if (tabs.length) {
    await chrome.storage.local.set({ targetTabId: tabs[0].id });
    return tabs[0].id;
  }
  throw new Error("Chưa có tab TikTok — mở tiktok.com rồi bấm 'Điều khiển tab này' trong popup");
}

const attached = new Set();

async function ensureAttached(tabId) {
  if (attached.has(tabId)) return;
  await chrome.debugger.attach({ tabId }, "1.3");
  attached.add(tabId);
}

function dbg(tabId, method, params) {
  return new Promise((resolve, reject) => {
    chrome.debugger.sendCommand({ tabId }, method, params || {}, (res) => {
      const err = chrome.runtime.lastError;
      if (err) reject(new Error(err.message));
      else resolve(res);
    });
  });
}

chrome.debugger.onDetach.addListener((source) => {
  if (source.tabId) attached.delete(source.tabId);
});

// ---- primitives ----

const KEY_MAP = {
  Enter: ["Enter", "Enter", 13],
  Return: ["Enter", "Enter", 13],
  Tab: ["Tab", "Tab", 9],
  Backspace: ["Backspace", "Backspace", 8],
  Delete: ["Delete", "Delete", 46],
  Escape: ["Escape", "Escape", 27],
  Esc: ["Escape", "Escape", 27],
  ArrowUp: ["ArrowUp", "ArrowUp", 38],
  ArrowDown: ["ArrowDown", "ArrowDown", 40],
  ArrowLeft: ["ArrowLeft", "ArrowLeft", 37],
  ArrowRight: ["ArrowRight", "ArrowRight", 39],
  Space: [" ", "Space", 32],
};

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function methodHandler(method, params) {
  const tabId = await getTargetTabId();
  await ensureAttached(tabId);

  switch (method) {
    case "url": {
      const tab = await chrome.tabs.get(tabId);
      return tab.url || "";
    }
    case "navigate": {
      await chrome.tabs.update(tabId, { url: params.url });
      const timeout = Number(params.timeout_ms || 45000);
      const deadline = Date.now() + timeout;
      while (Date.now() < deadline) {
        const t = await chrome.tabs.get(tabId);
        if (t.status === "complete") break;
        await sleep(200);
      }
      return true;
    }
    case "eval": {
      const r = await dbg(tabId, "Runtime.evaluate", {
        expression: params.js,
        returnByValue: true,
        awaitPromise: true,
      });
      if (r.exceptionDetails) {
        throw new Error(r.exceptionDetails.text || "eval exception");
      }
      return r.result ? r.result.value : null;
    }
    case "mouse_click": {
      const base = { x: Number(params.x), y: Number(params.y), button: "left", clickCount: 1 };
      await dbg(tabId, "Input.dispatchMouseEvent", { type: "mousePressed", ...base });
      await sleep(40);
      await dbg(tabId, "Input.dispatchMouseEvent", { type: "mouseReleased", ...base });
      return true;
    }
    case "type_text": {
      for (const ch of String(params.text || "")) {
        await dbg(tabId, "Input.dispatchKeyEvent", { type: "char", text: ch, key: ch });
        await sleep(30 + Math.floor(Math.random() * 60));
      }
      return true;
    }
    case "press_key": {
      const m = KEY_MAP[params.key];
      if (!m) {
        for (const ch of String(params.key || "")) {
          await dbg(tabId, "Input.dispatchKeyEvent", { type: "char", text: ch, key: ch });
        }
        return true;
      }
      const [key, code, vk] = m;
      await dbg(tabId, "Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode: vk });
      await sleep(30);
      await dbg(tabId, "Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: vk });
      return true;
    }
    case "wheel": {
      await dbg(tabId, "Input.dispatchMouseEvent", {
        type: "mouseWheel",
        x: Number(params.x),
        y: Number(params.y),
        deltaX: Number(params.dx || 0),
        deltaY: Number(params.dy || 0),
      });
      return true;
    }
    case "ping":
      return "pong";
    default:
      throw new Error(`method không hỗ trợ: ${method}`);
  }
}

// ---- WS transport ----

function reply(id, result, error) {
  const msg = error ? { id, error: String(error) } : { id, result };
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify(msg));
  } else if (callbackSecret) {
    // Resilient fallback if the socket dropped mid-call.
    fetch(`http://127.0.0.1:4580/api/ext/callback`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ...msg, secret: callbackSecret }),
    }).catch(() => {});
  }
}

async function onMessage(text) {
  let m;
  try {
    m = JSON.parse(text);
  } catch (_) {
    return;
  }
  if (m.type === "callback_secret") {
    callbackSecret = m.secret;
    return;
  }
  if (m.type === "pong") return;
  if (typeof m.id === "string" && typeof m.method === "string") {
    const started = Date.now();
    const info = summarize(m.method, m.params);
    try {
      const result = await methodHandler(m.method, m.params || {});
      pushLog("cmd", m.method, info, true, Date.now() - started);
      reply(m.id, result, null);
    } catch (e) {
      const msg = e.message || String(e);
      pushLog("cmd", m.method, info ? `${info} — ${msg}` : msg, false, Date.now() - started);
      reply(m.id, null, msg);
    }
  }
}

function connect() {
  clearTimeout(reconnectTimer);
  try {
    ws = new WebSocket(WS_URL);
  } catch (_) {
    scheduleReconnect();
    return;
  }
  ws.onopen = () => {
    ws.send(JSON.stringify({ type: "extension_ready" }));
    chrome.storage.local.set({ connected: true });
    stats.connects++;
    stats.connectedSince = Date.now();
    saveStats();
    pushLog("conn", "connected", `app ws://127.0.0.1:${WS_PORT}`, true);
  };
  ws.onmessage = (ev) => onMessage(ev.data);
  ws.onclose = () => {
    chrome.storage.local.set({ connected: false });
    stats.connectedSince = 0;
    saveStats();
    pushLog("conn", "disconnected", "mất kết nối app — tự thử lại", false);
    scheduleReconnect();
  };
  ws.onerror = () => {
    try {
      ws.close();
    } catch (_) {}
  };
}

function scheduleReconnect() {
  clearTimeout(reconnectTimer);
  reconnectTimer = setTimeout(connect, 3000);
}

// Popup → worker control (clear the log panel).
chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  if (msg && msg.type === "clear_log") {
    logBuf = [];
    chrome.storage.local.set({ [LOG_KEY]: [] });
    sendResponse({ ok: true });
  }
  return false;
});

// Keep the service worker connecting (MV3 workers can be evicted; the alarm
// wakes it to re-check the socket).
chrome.runtime.onInstalled.addListener(connect);
chrome.runtime.onStartup.addListener(connect);
loadLog().finally(connect);
