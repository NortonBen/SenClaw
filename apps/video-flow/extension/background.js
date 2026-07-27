/**
 * Video Flow — Chrome Extension Background Service Worker
 *
 * Bridges the SenClaw Video Flow app (apps/video-flow) to Google Flow:
 * captures the bearer token, solves reCAPTCHA, proxies API calls through the
 * browser session.
 *
 * Ports are CONFIGURABLE (popup → Kết nối) because the SenClaw daemon assigns
 * the app's HTTP port; hardcoding it is how the callback URL silently rotted
 * when the backend moved from :8101 to :4460.
 */

const DEFAULT_WS_PORT = 9222;   // app's extension bridge (FLOWKIT_WS_PORT)
const DEFAULT_HTTP_PORT = 4460; // app's HTTP port (manifest runtime.port)

// Flow's UI is locale-scoped (`/fx/vi/tools/flow`, `/fx/tools/flow`, …). The
// tRPC/media APIs are locale-independent, so any of these tabs works for
// capture — but opening the user's own locale skips a redirect hop. Kept as one
// place so the tools-flow path isn't spelled out inline in five spots.
const FLOW_LOCALE = 'vi';
const FLOW_BASE = `https://labs.google/fx/${FLOW_LOCALE}/tools/flow`;
// Match any locale (and the locale-less form) when finding an existing tab.
const FLOW_TAB_GLOBS = [
  'https://labs.google/fx/tools/flow*',
  'https://labs.google/fx/*/tools/flow*',
];
function flowProjectUrl(projectId) {
  return projectId
    ? `${FLOW_BASE}/project/${encodeURIComponent(projectId)}`
    : FLOW_BASE;
}

let wsPort = DEFAULT_WS_PORT;
let httpPort = DEFAULT_HTTP_PORT;

function agentWsUrl() {
  return `ws://127.0.0.1:${wsPort}`;
}
function callbackUrl() {
  return `http://127.0.0.1:${httpPort}/api/ext/callback`;
}

/** Load saved ports before the first connect; safe to call repeatedly. */
async function loadPorts() {
  try {
    const s = await chrome.storage.local.get(['wsPort', 'httpPort']);
    wsPort = Number(s.wsPort) || DEFAULT_WS_PORT;
    httpPort = Number(s.httpPort) || DEFAULT_HTTP_PORT;
  } catch {
    /* keep defaults */
  }
}
// NOTE: This is a browser-restricted public API key — safe to ship in extension bundles.
const API_KEY = 'AIzaSyBtrm0o5ab1c-Ec8ZuLcGt3oJAA5VWt3pY';

let ws = null;
let flowKey = null;
let callbackSecret = null;  // Auth secret for HTTP callback, received from server on WS connect
let state = 'off'; // off | idle | running
let manualDisconnect = false;
let metrics = {
  tokenCapturedAt: null,
  requestCount: 0,   // captcha-consuming requests only (gen image/video/upscale)
  successCount: 0,
  failedCount: 0,
  lastError: null,
};

const EXT_DEBUG = true;
function debugLog(...args) {
  if (!EXT_DEBUG) return;
  console.log('[FlowAgent][debug]', ...args);
}

// ─── URL → Log Type Classifier ─────────────────────────────

// Visible log types — only these appear in the request log
const _VISIBLE_TYPES = new Set(['GEN_IMG', 'GEN_VID', 'GEN_VID_REF', 'UPSCALE', 'TRACKING', 'URL_REFRESH']);

function _classifyApiUrl(url) {
  if (url.includes('uploadImage'))                     return 'UPLOAD';
  if (url.includes('batchGenerateImages'))              return 'GEN_IMG';
  if (url.includes('UpsampleVideo'))                   return 'UPSCALE';
  if (url.includes('ReferenceImages'))                 return 'GEN_VID_REF';
  if (url.includes('batchAsyncGenerateVideo'))          return 'GEN_VID';
  if (url.includes('batchCheckAsync'))                  return 'POLL';
  if (url.includes('upsampleImage'))                   return 'UPS_IMG';
  if (url.includes('/media/'))                         return 'MEDIA';
  if (url.includes('/credits'))                        return 'CREDITS';
  return 'API';
}

// ─── Request Log ────────────────────────────────────────────

let requestLog = [];

function addRequestLog(entry) {
  requestLog.unshift(entry);
  if (requestLog.length > 100) requestLog.pop();
  broadcastRequestLog();
}

function updateRequestLog(id, updates) {
  const entry = requestLog.find((e) => e.id === id);
  if (entry) Object.assign(entry, updates);
  broadcastRequestLog();
}

function broadcastRequestLog() {
  chrome.runtime.sendMessage({ type: 'REQUEST_LOG_UPDATE', log: requestLog }).catch(() => {});
}

// ─── Startup ────────────────────────────────────────────────

chrome.runtime.onInstalled.addListener(init);
chrome.runtime.onStartup.addListener(init);
chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name === 'reconnect') connectToAgent();
  if (alarm.name === 'keepAlive') { keepAlive(); reportFlowProjectId(); }
  if (alarm.name === 'token-refresh') {
    await captureTokenFromFlowTab();
    reportFlowProjectId();
  }
});

// Catch the moment the user opens/navigates to a Flow project so the app learns
// the real project id promptly (not only on the periodic alarm).
chrome.tabs.onUpdated.addListener((tabId, info, tab) => {
  if (_scrapeTabs.has(tabId)) return; // app's own scrape tab — not a real project
  if (info.status === 'complete' && /\/tools\/flow\/project\//.test(tab?.url || '')) {
    reportFlowProjectId();
  }
});

async function init() {
  const data = await chrome.storage.local.get(['flowKey', 'metrics', 'callbackSecret']);
  if (data.flowKey) flowKey = data.flowKey;
  if (data.metrics) Object.assign(metrics, data.metrics);
  if (data.callbackSecret) callbackSecret = data.callbackSecret;
  connectToAgent();
  chrome.alarms.create('keepAlive', { periodInMinutes: 0.4 });
}

// ─── Token Capture ──────────────────────────────────────────

chrome.webRequest.onBeforeSendHeaders.addListener(
  (details) => {
    if (!details?.requestHeaders?.length) return;
    const authHeader = details.requestHeaders.find(
      (h) => h.name?.toLowerCase() === 'authorization',
    );
    const value = authHeader?.value || '';
    if (!value.startsWith('Bearer ya29.')) return;

    const token = value.replace(/^Bearer\s+/i, '').trim();
    if (!token) return;

    // Always update — even if same token string, refresh the timestamp
    flowKey = token;
    metrics.tokenCapturedAt = Date.now();
    chrome.storage.local.set({ flowKey, metrics });
    console.log('[FlowAgent] Bearer token captured');

    // Notify agent
    if (ws?.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'token_captured', flowKey }));
    }
  },
  { urls: ['https://aisandbox-pa.googleapis.com/*', 'https://labs.google/*'] },
  ['requestHeaders', 'extraHeaders'],
);

let _openingFlowTab = false;
let _lastReportedProjectId = null;
// Tabs the app opened itself for URL scraping — their project id may be our own
// non-browsable id, so it must never be captured as the user's real project.
const _scrapeTabs = new Set();

// The app invents its own project id, but Flow doesn't create a browsable
// project for it — generation works, yet the project page 404s and the URL
// scrape finds nothing. So capture the REAL project id from an open Flow project
// tab (`…/tools/flow/project/<uuid>`) and hand it to the app, which then targets
// that real project for generation + scraping.
async function reportFlowProjectId() {
  try {
    const tabs = await chrome.tabs.query({ url: ['https://labs.google/fx/*/tools/flow/project/*', 'https://labs.google/fx/tools/flow/project/*'] });
    for (const t of tabs) {
      if (_scrapeTabs.has(t.id)) continue; // skip the app's own scrape tabs
      const m = (t.url || '').match(/\/tools\/flow\/project\/([0-9a-f-]{36})/i);
      if (m) {
        const pid = m[1];
        if (pid !== _lastReportedProjectId && ws?.readyState === WebSocket.OPEN) {
          _lastReportedProjectId = pid;
          ws.send(JSON.stringify({ type: 'flow_project_id', projectId: pid }));
          console.log('[FlowAgent] reported real Flow project id', pid);
        }
        return pid;
      }
    }
  } catch (e) {
    console.warn('[FlowAgent] reportFlowProjectId failed', e?.message || e);
  }
  return null;
}

async function captureTokenFromFlowTab() {
  const tabs = await chrome.tabs.query({
    url: FLOW_TAB_GLOBS,
  });
  if (!tabs.length) {
    if (_openingFlowTab) {
      console.log('[FlowAgent] Flow tab already opening, skipping');
      return;
    }
    _openingFlowTab = true;
    try {
      console.log('[FlowAgent] No Flow tab found — opening one in background');
      await chrome.tabs.create({ url: FLOW_BASE, active: false });
      await sleep(3000);
      const retryTabs = await chrome.tabs.query({
        url: FLOW_TAB_GLOBS,
      });
      if (!retryTabs.length) {
        console.log('[FlowAgent] Flow tab not ready yet after open');
        return;
      }
      await chrome.scripting.executeScript({
        target: { tabId: retryTabs[0].id },
        files: ['content.js'],
      });
      console.log('[FlowAgent] Token refresh triggered on newly opened Flow tab');
    } catch (e) {
      console.error('[FlowAgent] Token refresh failed after opening tab:', e);
    } finally {
      _openingFlowTab = false;
    }
    return;
  }
  try {
    await chrome.scripting.executeScript({
      target: { tabId: tabs[0].id },
      files: ['content.js'],
    });
    console.log('[FlowAgent] Token refresh triggered on Flow tab');
  } catch (e) {
    console.error('[FlowAgent] Token refresh failed:', e);
  }
}

// ─── WebSocket to Agent ─────────────────────────────────────

async function connectToAgent() {
  if (manualDisconnect) return;
  if (ws?.readyState === WebSocket.CONNECTING) return;
  if (ws?.readyState === WebSocket.OPEN) return;

  await loadPorts();
  try {
    ws = new WebSocket(agentWsUrl());
  } catch (e) {
    console.error('[FlowAgent] WS connect error:', e);
    scheduleReconnect();
    return;
  }

  ws.onopen = () => {
    console.log('[FlowAgent] Connected to agent');
    chrome.alarms.clear('reconnect');
    setState('idle');

    // Token refresh alarm — 45 min gives buffer before ~60 min expiry
    chrome.alarms.create('token-refresh', { periodInMinutes: 45 });

    // Send current state + resend token if we have one
    ws.send(JSON.stringify({
      type: 'extension_ready',
      flowKeyPresent: !!flowKey,
      tokenAge: flowKey && metrics.tokenCapturedAt ? Date.now() - metrics.tokenCapturedAt : null,
    }));
    if (flowKey) {
      ws.send(JSON.stringify({ type: 'token_captured', flowKey }));
    }
    _lastReportedProjectId = null; // force a fresh report on this connection
    reportFlowProjectId();
  };

  ws.onmessage = async ({ data }) => {
    try {
      const msg = JSON.parse(data);
      debugLog('ws inbound', { id: msg.id || null, method: msg.method || null, type: msg.type || null });

      if (msg.method === 'api_request') {
        await handleApiRequest(msg);
      } else if (msg.method === 'trpc_request') {
        await handleTrpcRequest(msg);
      } else if (msg.method === 'open_flow_project') {
        await handleOpenFlowProject(msg);
      } else if (msg.method === 'solve_captcha') {
        await handleSolveCaptcha(msg);
      } else if (msg.method === 'get_status') {
        sendToAgent({
          id: msg.id,
          result: {
            state,
            flowKeyPresent: !!flowKey,
            manualDisconnect,
            tokenAge: metrics.tokenCapturedAt ? Date.now() - metrics.tokenCapturedAt : null,
            metrics,
          },
        });
      } else if (msg.type === 'callback_secret') {
        callbackSecret = msg.secret;
        chrome.storage.local.set({ callbackSecret: msg.secret });
        console.log('[FlowAgent] Received callback secret');
      } else if (msg.type === 'pong') {
        // keepalive response
      }
    } catch (e) {
      console.error('[FlowAgent] Message error:', e);
    }
  };

  ws.onclose = () => {
    setState('off');
    chrome.alarms.clear('token-refresh');
    if (!manualDisconnect) scheduleReconnect();
  };

  ws.onerror = (e) => {
    console.error('[FlowAgent] WS error:', e);
    metrics.lastError = 'WS_ERROR';
    chrome.storage.local.set({ metrics });
  };
}

function scheduleReconnect() {
  chrome.alarms.create('reconnect', { delayInMinutes: 0.083 }); // ~5s
}

function keepAlive() {
  if (ws?.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: 'ping' }));
  } else {
    connectToAgent();
  }
}

function sendToAgent(msg) {
  // API responses (with msg.id) go via HTTP — immune to WS disconnect
  if (msg.id) {
    debugLog('sendToAgent HTTP callback -> backend', {
      id: msg.id,
      status: msg.status ?? null,
      hasError: !!msg.error,
    });
    fetch(callbackUrl(), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(msg),
    }).then((resp) => {
      debugLog('callback HTTP response', { id: msg.id, status: resp.status, ok: resp.ok });
    }).catch((err) => {
      debugLog('callback HTTP failed, fallback WS', { id: msg.id, err: err?.message || String(err) });
      // HTTP failed — fallback to WS
      if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify(msg));
    });
    return;
  }
  // Non-response messages (ping, status) or no secret yet — use WS
  if (ws?.readyState === WebSocket.OPEN) {
    debugLog('sendToAgent WS', { type: msg.type || null, hasId: !!msg.id });
    ws.send(JSON.stringify(msg));
  }
}

// ─── reCAPTCHA Solving ──────────────────────────────────────

async function requestCaptchaFromTab(tabId, requestId, pageAction) {
  try {
    return await chrome.tabs.sendMessage(tabId, {
      type: 'GET_CAPTCHA',
      requestId,
      pageAction,
    });
  } catch (error) {
    const msg = error?.message || '';
    const shouldInject =
      msg.includes('Receiving end does not exist') ||
      msg.includes('Could not establish connection');
    if (!shouldInject) throw error;

    // Inject content script and retry
    await chrome.scripting.executeScript({
      target: { tabId },
      files: ['content.js'],
    });
    await sleep(200);
    return await chrome.tabs.sendMessage(tabId, {
      type: 'GET_CAPTCHA',
      requestId,
      pageAction,
    });
  }
}

async function solveCaptcha(requestId, captchaAction) {
  const tabs = await chrome.tabs.query({
    url: FLOW_TAB_GLOBS,
  });

  if (!tabs.length) {
    // Auto-open Flow tab and wait briefly before returning error
    try {
      await chrome.tabs.create({ url: FLOW_BASE, active: false });
      await sleep(3000);
      // Retry tab query after opening
      const retryTabs = await chrome.tabs.query({
        url: FLOW_TAB_GLOBS,
      });
      if (!retryTabs.length) return { error: 'NO_FLOW_TAB' };
      const resp = await Promise.race([
        requestCaptchaFromTab(retryTabs[0].id, requestId, captchaAction),
        new Promise((_, rej) => setTimeout(() => rej(new Error('CAPTCHA_TIMEOUT')), 30000)),
      ]);
      return resp;
    } catch (e) {
      return { error: e.message || 'NO_FLOW_TAB' };
    }
  }

  try {
    const resp = await Promise.race([
      requestCaptchaFromTab(tabs[0].id, requestId, captchaAction),
      new Promise((_, rej) => setTimeout(() => rej(new Error('CAPTCHA_TIMEOUT')), 30000)),
    ]);
    return resp;
  } catch (e) {
    return { error: e.message };
  }
}

async function handleSolveCaptcha(msg) {
  const { id, params } = msg;
  const result = await solveCaptcha(id, params?.captchaAction || 'VIDEO_GENERATION');

  // Standalone captcha solve counts as captcha-consuming
  metrics.requestCount++;
  if (result?.token) {
    metrics.successCount++;
  } else {
    metrics.failedCount++;
    metrics.lastError = result?.error || 'NO_TOKEN';
  }
  chrome.storage.local.set({ metrics });

  sendToAgent({ id, result });
}

// ─── API Request Proxy ──────────────────────────────────────

// Return a loaded Flow tab id, opening a background one if none exists. Flow
// tRPC (createProject / searchUserProjects) must run FROM the page: it is
// authenticated by the next-auth **session cookie** + same-origin/referer, not
// by the aisandbox Bearer token — so a background service-worker fetch (wrong
// Origin, wrong/no auth) gets rejected. Running in the tab's page context makes
// the call identical to what the Flow web app itself issues.
async function getOrOpenFlowTab() {
  let tabs = await chrome.tabs.query({ url: FLOW_TAB_GLOBS });
  let tab = tabs.find((t) => t.status === 'complete') || tabs[0];
  if (tab) return tab.id;
  tab = await chrome.tabs.create({ url: FLOW_BASE, active: false });
  // Wait for the SPA to be usable (its session cookie is already set, but give
  // the document a moment to finish loading before we script into it).
  for (let i = 0; i < 40; i++) {
    await sleep(250);
    const t = await chrome.tabs.get(tab.id).catch(() => null);
    if (t && t.status === 'complete') break;
  }
  return tab.id;
}

async function handleTrpcRequest(msg) {
  const { id, params } = msg;
  const { url, method = 'POST', body } = params;

  if (!url || !url.startsWith('https://labs.google/')) {
    sendToAgent({ id, error: 'INVALID_TRPC_URL' });
    return;
  }

  setState('running');
  const logId = id;

  try {
    const tabId = await getOrOpenFlowTab();
    if (tabId == null) {
      sendToAgent({ id, status: 503, error: 'NO_FLOW_TAB' });
      return;
    }
    // Execute the fetch in the page (MAIN world) so it carries the Flow session
    // cookie + correct Origin/Referer — exactly like the Flow app's own call.
    const results = await chrome.scripting.executeScript({
      target: { tabId },
      world: 'MAIN',
      args: [url, method, body ?? null],
      func: async (u, m, b) => {
        try {
          const r = await fetch(u, {
            method: m,
            headers: { 'content-type': 'application/json' },
            body: b != null ? JSON.stringify(b) : undefined,
            credentials: 'include',
          });
          const text = await r.text();
          let data;
          try { data = JSON.parse(text); } catch { data = text; }
          return { status: r.status, data };
        } catch (e) {
          return { status: 0, error: String((e && e.message) || e) };
        }
      },
    });
    const out = results && results[0] && results[0].result;
    if (!out) {
      sendToAgent({ id, error: 'TRPC_NO_RESULT' });
    } else if (out.error) {
      sendToAgent({ id, error: out.error });
    } else {
      updateRequestLog(logId, { status: 'success' });
      sendToAgent({ id, status: out.status, data: out.data });
    }
  } catch (e) {
    console.error('[FlowAgent] tRPC request failed:', e);
    updateRequestLog(logId, { status: 'failed', error: e.message || 'TRPC_FETCH_FAILED' });
    sendToAgent({ id, error: e.message || 'TRPC_FETCH_FAILED' });
  } finally {
    setState('idle');
  }
}

/**
 * Load a Flow project page in a background tab so its own tRPC calls run.
 *
 * Flow's generation API stopped returning video URLs — a finished clip comes
 * back as a media id only. The signed URLs exist solely in the tRPC payloads
 * the Flow web app fetches for its project view, which `injected.js` already
 * intercepts. So to get a URL we make the page fetch it: open the project,
 * dwell while it loads, close the tab. The interception then forwards the URLs
 * to the agent through the normal `media_urls_refresh` path.
 */
async function handleOpenFlowProject(msg) {
  const { id, params } = msg;
  const projectId = (params?.projectId || '').trim();
  // Long enough for the SPA to boot and issue its media queries, capped so a
  // wedged page can't hold a tab open indefinitely.
  const dwellMs = Math.min(Math.max(Number(params?.dwellMs) || 9000, 2000), 30000);
  const url = flowProjectUrl(projectId);

  let tabId = null;
  try {
    const tab = await chrome.tabs.create({ url, active: false });
    tabId = tab.id;
    // Mark this as an app-opened scrape tab so its project id (which may be our
    // own non-browsable id) is NOT mistaken for a real one the user is viewing.
    if (tabId != null) _scrapeTabs.add(tabId);
    await sleep(dwellMs);
    // The fetch monkey-patch only sees tRPC responses, and Flow doesn't always
    // put the video URL there (it may load a clip via `<video src>` or XHR). So
    // also harvest the rendered page directly: DOM media src + every media URL
    // the browser actually loaded (performance resource timing) + any signed
    // link left in the HTML. This is what recovers video links tRPC scraping
    // misses.
    let scraped = 0;
    try {
      scraped = await harvestFlowTabMedia(tabId);
    } catch (e) {
      console.warn('[FlowAgent] DOM harvest failed:', e?.message || e);
    }
    sendToAgent({ id, status: 200, data: { ok: true, url, dwellMs, scraped } });
  } catch (e) {
    sendToAgent({ id, error: e.message || 'OPEN_FLOW_PROJECT_FAILED' });
  } finally {
    // Opened for scraping only — never leave it behind in the user's window.
    if (tabId != null) {
      _scrapeTabs.delete(tabId);
      try { await chrome.tabs.remove(tabId); } catch { /* already closed */ }
    }
  }
}

// Run in the Flow tab: scroll to trigger lazy media, then collect every media
// URL reachable from the page — element src, resource-timing (catches XHR and
// <video> loads the fetch hook misses), and signed links still in the HTML.
async function harvestFlowTabMedia(tabId) {
  const [{ result } = {}] = await chrome.scripting.executeScript({
    target: { tabId },
    func: () => {
      // Nudge a virtualized scene list so off-screen videos attach their src.
      try {
        const sc = document.scrollingElement || document.documentElement;
        for (let y = 0; y <= sc.scrollHeight; y += Math.max(300, innerHeight)) sc.scrollTop = y;
        sc.scrollTop = 0;
      } catch {}
      const urls = new Set();
      for (const el of document.querySelectorAll('video[src], source[src], img[src]')) {
        if (el.src) urls.add(el.src);
      }
      for (const e of performance.getEntriesByType('resource')) {
        if (e.name && /storage\.googleapis\.com|\/video\/|\/image\/|\.mp4/.test(e.name)) urls.add(e.name);
      }
      return { urls: [...urls], html: document.documentElement.outerHTML };
    },
  });
  if (!result) return 0;
  // Element/resource URLs are already unescaped; HTML may hold JSON-escaped
  // links. Feed both through the shared extractor.
  const blob = [...(result.urls || []), result.html || ''].join('\n');
  const entries = _extractMediaEntries(blob);
  return _forwardMediaEntries(entries, { source: 'DOM' });
}

// Human-readable label for a failed Flow response. Prefers Google's structured
// `{error:{message,details:[{reason}]}}` so the log shows the actual cause.
function _flowErrorLabel(data, status) {
  try {
    const err = data && typeof data === 'object' ? data.error : null;
    if (err && typeof err === 'object') {
      const msg = typeof err.message === 'string' ? err.message : '';
      const reason = Array.isArray(err.details)
        ? (err.details.find((d) => d && d.reason)?.reason ?? '')
        : '';
      if (msg && reason) return `${msg} (${reason})`;
      if (msg) return msg;
      if (reason) return reason;
    }
  } catch { /* fall through */ }
  return `API_${status}`;
}

async function handleApiRequest(msg) {
  const { id, params } = msg;
  const { url, method, headers, body, captchaAction } = params;
  debugLog('handleApiRequest start', {
    id,
    method: method || 'POST',
    url,
    hasCaptcha: !!captchaAction,
    hasBody: !!body,
  });

  if (!url) {
    sendToAgent({ id, error: 'MISSING_URL' });
    return;
  }

  if (!url.startsWith('https://aisandbox-pa.googleapis.com/')) {
    sendToAgent({ id, error: 'INVALID_URL' });
    return;
  }

  setState('running');
  const hasCaptcha = !!captchaAction;
  if (hasCaptcha) metrics.requestCount++;

  const logId = id;
  const logType = _classifyApiUrl(url);
  if (_VISIBLE_TYPES.has(logType)) {
    // Keep a short summary for the table and a fuller copy for the detail view.
    // Capped so 100 log entries can't balloon memory, but generous enough to
    // hold a whole generation payload/response for debugging.
    const payloadStr = body ? JSON.stringify(body) : '';
    const payloadSummary = payloadStr ? payloadStr.slice(0, 200) : null;
    const payloadFull = payloadStr ? payloadStr.slice(0, 20000) : null;
    addRequestLog({ id: logId, type: logType, time: new Date().toISOString(), status: 'processing', error: null, outputUrl: null, url, payloadSummary, payloadFull });
  }

  try {
    // Step 1: Solve captcha if needed
    let captchaToken = null;
    if (captchaAction) {
      debugLog('captcha solving start', { id, captchaAction });
      const captchaResult = await solveCaptcha(id, captchaAction);
      captchaToken = captchaResult?.token || null;
      if (!captchaToken) {
        // Cannot proceed without captcha — API will 403
        const err = captchaResult?.error || 'CAPTCHA_FAILED';
        console.error(`[FlowAgent] Captcha failed for ${captchaAction}: ${err}`);
        sendToAgent({ id, status: 403, error: `CAPTCHA_FAILED: ${err}` });
        if (hasCaptcha) { metrics.failedCount++; metrics.lastError = `CAPTCHA_FAILED: ${err}`; }
        chrome.storage.local.set({ metrics });
        updateRequestLog(logId, { status: 'failed', error: `CAPTCHA_FAILED: ${err}` });
        setState('idle');
        return;
      }
      debugLog('captcha solving success', { id, tokenLen: captchaToken.length });
    }

    // Step 2: Inject captcha token into body
    let finalBody = body;
    if (captchaToken && finalBody) {
      finalBody = JSON.parse(JSON.stringify(finalBody)); // deep clone
      if (finalBody.clientContext?.recaptchaContext) {
        finalBody.clientContext.recaptchaContext.token = captchaToken;
      }
      if (finalBody.requests && Array.isArray(finalBody.requests)) {
        for (const req of finalBody.requests) {
          if (req.clientContext?.recaptchaContext) {
            req.clientContext.recaptchaContext.token = captchaToken;
          }
        }
      }
    }

    // Step 3: Use flowKey for auth
    const activeFlowKey = flowKey;
    if (!activeFlowKey) {
      sendToAgent({ id, status: 503, error: 'NO_FLOW_KEY' });
      if (hasCaptcha) { metrics.failedCount++; metrics.lastError = 'NO_FLOW_KEY'; }
      chrome.storage.local.set({ metrics });
      updateRequestLog(logId, { status: 'failed', error: 'NO_FLOW_KEY' });
      setState('idle');
      return;
    }
    debugLog('api auth ready', { id, flowKeyLen: activeFlowKey.length });

    const fetchHeaders = { ...(headers || {}) };
    fetchHeaders['authorization'] = `Bearer ${activeFlowKey}`;

    // Step 4: Make the API call from browser context
    const response = await fetch(url, {
      method: method || 'POST',
      headers: fetchHeaders,
      credentials: 'include',
      body: method === 'GET' ? undefined : JSON.stringify(finalBody),
    });
    debugLog('api fetch done', { id, httpStatus: response.status, ok: response.ok });

    let responseData;
    const responseText = await response.text();
    try {
      responseData = JSON.parse(responseText);
    } catch {
      responseData = responseText;
    }

    sendToAgent({
      id,
      status: response.status,
      data: responseData,
    });

    const responseSummary = responseText ? responseText.slice(0, 300) : null;
    const responseFull = responseText ? responseText.slice(0, 20000) : null;
    // First media URL in the response, whatever the GCS bucket (Flow renames it
    // — a fixed bucket name is why every row showed "—"). Images return a URL
    // synchronously here; video is async so its URL arrives later via the tRPC
    // scrape and is back-filled by media id below.
    let outputUrl = null;
    const urlMatch = responseText && responseText.match(
      /https:\/\/storage\.googleapis\.com\/[^"'\s\\]*?\/(?:image|video)\/[0-9a-f-]{36}\?[^"'\s\\]+/,
    );
    if (urlMatch) outputUrl = urlMatch[0].replace(/\\u0026/gi, '&').replace(/\\/g, '');
    // Media ids in the response, so a later scraped URL can be matched back to
    // this row (the only way to show async video once its link appears).
    const mediaIds = responseText ? [...new Set(responseText.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi) || [])] : [];
    const mediaKind = logType === 'GEN_VID' || logType === 'GEN_VID_REF' || logType === 'UPSCALE' ? 'video'
      : logType === 'GEN_IMG' ? 'image' : null;
    if (response.ok) {
      if (hasCaptcha) { metrics.successCount++; metrics.lastError = null; }
      updateRequestLog(logId, { status: 'success', httpStatus: response.status, responseSummary, responseFull, outputUrl, mediaIds, mediaKind });
    } else {
      // Surface Google's real reason ("reCAPTCHA evaluation failed
      // (PUBLIC_ERROR_UNUSUAL_ACTIVITY)") instead of a bare "API_403", so the
      // panel is actually diagnostic.
      const errLabel = _flowErrorLabel(responseData, response.status);
      if (hasCaptcha) { metrics.failedCount++; metrics.lastError = errLabel; }
      updateRequestLog(logId, { status: 'failed', error: errLabel, httpStatus: response.status, responseSummary, responseFull, outputUrl, mediaIds, mediaKind });
    }
  } catch (e) {
    debugLog('api request exception', { id, err: e.message || 'API_REQUEST_FAILED' });
    sendToAgent({
      id,
      status: 500,
      error: e.message || 'API_REQUEST_FAILED',
    });
    if (hasCaptcha) { metrics.failedCount++; metrics.lastError = e.message; }
    updateRequestLog(logId, { status: 'failed', error: e.message || 'API_REQUEST_FAILED' });
  }

  chrome.storage.local.set({ metrics });
  setState('idle');
}

// ─── State & Popup ──────────────────────────────────────────

function setState(newState) {
  state = newState;
  const badges = { idle: '●', running: '▶', off: '○' };
  const colors = { idle: '#22c55e', running: '#f59e0b', off: '#6b7280' };
  chrome.action.setBadgeText({ text: badges[state] || '' });
  chrome.action.setBadgeBackgroundColor({ color: colors[state] || '#000' });
  broadcastStatus();
}

function broadcastStatus() {
  chrome.runtime.sendMessage({ type: 'STATUS_PUSH' }).catch(() => {});
}

chrome.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (msg.type === 'STATUS') {
    reply({
      connected: ws?.readyState === WebSocket.OPEN,
      agentConnected: ws?.readyState === WebSocket.OPEN,
      flowKeyPresent: !!flowKey,
      manualDisconnect,
      wsPort,
      httpPort,
      tokenAge: metrics.tokenCapturedAt ? Date.now() - metrics.tokenCapturedAt : null,
      metrics: {
        requestCount: metrics.requestCount,
        successCount: metrics.successCount,
        failedCount: metrics.failedCount,
        lastError: metrics.lastError,
      },
      state,
    });
  }

  if (msg.type === 'DISCONNECT') {
    manualDisconnect = true;
    if (ws) ws.close();
    reply({ ok: true });
    return true;
  }

  if (msg.type === 'RECONNECT') {
    manualDisconnect = false;
    connectToAgent();
    reply({ ok: true });
    return true;
  }

  // Save the app's ports and immediately reconnect on them.
  if (msg.type === 'SET_PORTS') {
    const ws_ = Number(msg.wsPort) || DEFAULT_WS_PORT;
    const http_ = Number(msg.httpPort) || DEFAULT_HTTP_PORT;
    chrome.storage.local.set({ wsPort: ws_, httpPort: http_ }).then(() => {
      wsPort = ws_;
      httpPort = http_;
      manualDisconnect = false;
      if (ws) try { ws.close(); } catch { /* already closed */ }
      connectToAgent();
      reply({ ok: true, wsPort, httpPort });
    });
    return true;
  }

  // Is the app's HTTP API actually reachable on the configured port?
  if (msg.type === 'PING_BACKEND') {
    fetch(`http://127.0.0.1:${httpPort}/api/status`)
      .then((r) => r.json())
      .then((j) => reply({ ok: true, status: j }))
      .catch((e) => reply({ ok: false, error: e?.message || String(e) }));
    return true;
  }

  if (msg.type === 'REQUEST_LOG') {
    reply({ log: requestLog });
    return true;
  }

  if (msg.type === 'OPEN_FLOW_TAB') {
    chrome.tabs.query({
      url: FLOW_TAB_GLOBS,
    }).then((tabs) => {
      if (tabs.length) {
        chrome.tabs.update(tabs[0].id, { active: true });
        reply({ ok: true, tabId: tabs[0].id });
      } else {
        chrome.tabs.create({ url: FLOW_BASE })
          .then((tab) => reply({ ok: true, tabId: tab.id }))
          .catch((e) => reply({ error: e.message }));
      }
    }).catch((e) => reply({ error: e.message }));
    return true;
  }

  if (msg.type === 'DELETE_LOG_ENTRY') {
    requestLog = requestLog.filter((e) => e.id !== msg.id);
    broadcastRequestLog();
    reply({ ok: true });
    return true;
  }

  if (msg.type === 'CLEAR_LOG') {
    requestLog = [];
    broadcastRequestLog();
    reply({ ok: true });
    return true;
  }

  if (msg.type === 'REFRESH_TOKEN') {
    captureTokenFromFlowTab()
      .then(() => reply({ ok: true }))
      .catch((e) => reply({ error: e.message }));
    return true;
  }

  if (msg.type === 'TEST_CAPTCHA') {
    solveCaptcha(`test-${Date.now()}`, msg.pageAction || 'IMAGE_GENERATION')
      .then((r) => reply(r))
      .catch((e) => reply({ error: e.message }));
    return true;
  }

  if (msg.type === 'TRPC_MEDIA_URLS') {
    handleTrpcMediaUrls(msg.trpcUrl, msg.body);
    reply({ ok: true });
    return true;
  }

  if (msg.type === 'TRPC_PROJECT_IDS') {
    // Ignore tRPC from the app's own scrape tabs — only genuine user browsing.
    if (!(_sender?.tab && _scrapeTabs.has(_sender.tab.id))) {
      handleTrpcProjectIds(msg.trpcUrl, msg.body);
    }
    reply({ ok: true });
    return true;
  }

  return true;
});

// A real, browsable project id out of Flow's own tRPC data. For a single-project
// call (`/project/<id>` in the URL) that id is the one the user is viewing — the
// best pick; otherwise take the first project id in the body (project list).
function handleTrpcProjectIds(trpcUrl, bodyText) {
  try {
    let pid = null;
    // `…/tools/flow/project/<id>` page-style, or a tRPC `input={"projectId":"…"}`
    // (URL-encoded) — decode so both forms match.
    let decodedUrl = trpcUrl || '';
    try { decodedUrl = decodeURIComponent(decodedUrl); } catch { /* keep raw */ }
    const inUrl = decodedUrl.match(/\/project\/([0-9a-f-]{36})/i)
      || decodedUrl.match(/"projectId"\s*:\s*"([0-9a-f-]{36})"/i);
    if (inUrl) pid = inUrl[1];
    if (!pid) {
      // Require the id to sit next to real project metadata (title/createTime),
      // so an error body for a non-existent project can't be mistaken for one.
      const m = bodyText.match(/"(?:projectId|id)"\s*:\s*"([0-9a-f-]{36})"[^}]{0,200}"(?:title|displayName|createTime)"/i)
        || bodyText.match(/"(?:title|displayName|createTime)"[^}]{0,200}"(?:projectId|id)"\s*:\s*"([0-9a-f-]{36})"/i);
      if (m) pid = m[1];
    }
    if (pid && pid !== _lastReportedProjectId && ws?.readyState === WebSocket.OPEN) {
      _lastReportedProjectId = pid;
      ws.send(JSON.stringify({ type: 'flow_project_id', projectId: pid }));
      console.log('[FlowAgent] learned real Flow project id from tRPC', pid);
    }
  } catch (e) {
    console.warn('[FlowAgent] handleTrpcProjectIds failed', e?.message || e);
  }
}

// ─── Media URL Extractor ───────────────────────────────────

// Every signed storage.googleapis.com media link in a blob of text, whatever
// the bucket (Flow renames it — keying on a fixed bucket name is what made URL
// capture rot). Shape is `/<image|video>/<uuid>?<signature>`.
function _extractMediaEntries(text) {
  const urlRegex = /https:\/\/storage\.googleapis\.com\/[^"'\\\s]*?\/(image|video)\/([0-9a-f-]{36})\?[^"'\\\s]+/gi;
  const urlMap = {};
  let m;
  while ((m = urlRegex.exec(text)) !== null) {
    const url = m[0].replace(/\\u0026/gi, '&').replace(/\\/g, ''); // unescape JSON-escaped
    urlMap[m[2]] = { mediaType: m[1].toLowerCase(), url, mediaId: m[2] };
  }
  return Object.values(urlMap);
}

// Push captured URLs to the DB (scene columns) + back-fill the side-panel log.
// `source` is just for the console line.
function _forwardMediaEntries(entries, { videoModel = null, source = 'scrape' } = {}) {
  if (!entries.length && !videoModel) return 0;

  // Back-fill the side-panel log: a video row is created at submit time with no
  // URL (async), so attach the freshly scraped link to whichever row referenced
  // this media id. Images already have their URL from the sync response, but
  // refreshing keeps an expired one live.
  let backfilled = 0;
  for (const { mediaId, url, mediaType } of entries) {
    const row = requestLog.find((e) => Array.isArray(e.mediaIds) && e.mediaIds.includes(mediaId));
    if (row) {
      row.outputUrl = url;
      if (!row.mediaKind) row.mediaKind = mediaType;
      backfilled++;
    }
  }
  if (backfilled) broadcastRequestLog();

  if (entries.length || videoModel) {
    console.log(
      `[FlowAgent] ${entries.length} media URL(s) from ${source}` +
        (videoModel ? ` (model ${videoModel})` : ''),
    );
  }
  if ((entries.length || videoModel) && ws?.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: 'media_urls_refresh', urls: entries, videoModel }));
  }
  return entries.length;
}

function handleTrpcMediaUrls(trpcUrl, bodyText) {
  try {
    const entries = _extractMediaEntries(bodyText);
    // Also learn which Veo model this project is on. The app builds its own
    // generateVideo requests and needs the exact internal model key (e.g.
    // `veo_3_1_i2v_s_lite_portrait`) — guessing it breaks generation, so we read
    // the real one Flow itself uses out of the project response instead.
    const modelKeys = [];
    const modelRe = /"(?:videoModelKey|model)"\s*:\s*"(veo[0-9a-z_]+)"/gi;
    let mm;
    while ((mm = modelRe.exec(bodyText)) !== null) modelKeys.push(mm[1]);
    const videoModel = modelKeys.length ? modelKeys[modelKeys.length - 1] : null;
    _forwardMediaEntries(entries, { videoModel, source: 'tRPC' });
  } catch (e) {
    console.error('[FlowAgent] Failed to extract TRPC media URLs:', e);
  }
}

function sleep(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// ─── Human-like Telemetry ──────────────────────────────────
// Periodically send tracking events to Google's analytics endpoints
// to mimic normal browser behavior.

const _UA = navigator.userAgent;
let _telemetrySessionId = `;${Date.now()}`;

function _rand(min, max) { return Math.floor(Math.random() * (max - min + 1)) + min; }

function _buildBatchLogPayload() {
  const events = [];
  const types = ['FLOW_IMAGE_LATENCY', 'FLOW_VIDEO_LATENCY'];
  const count = _rand(1, 3);
  for (let i = 0; i < count; i++) {
    events.push({
      event: types[_rand(0, types.length - 1)],
      eventProperties: [
        { key: 'CURRENT_TIME_MS', doubleValue: Date.now() },
        { key: 'DURATION_MS', doubleValue: _rand(150, 800) },
        { key: 'USER_AGENT', stringValue: _UA },
        { key: 'IS_DESKTOP', booleanValue: true },
      ],
      eventMetadata: { sessionId: _telemetrySessionId },
      eventTime: new Date().toISOString(),
    });
  }
  return { appEvents: events };
}

function _buildFrontendEventsPayload() {
  const eventTypes = [
    'FLOW_IMAGE_LATENCY', 'FLOW_VIDEO_LATENCY', 'GRID_SCROLL_DEPTH',
    'FLOW_PROJECT_OPEN', 'FLOW_SCENE_VIEW',
  ];
  const count = _rand(1, 4);
  const events = [];
  for (let i = 0; i < count; i++) {
    const et = eventTypes[_rand(0, eventTypes.length - 1)];
    const params = {
      USER_AGENT: { '@type': 'type.googleapis.com/google.protobuf.StringValue', value: _UA },
      IS_DESKTOP: { '@type': 'type.googleapis.com/google.protobuf.StringValue', value: 'true' },
    };
    if (et.includes('LATENCY')) {
      params.CURRENT_TIME_MS = { '@type': 'type.googleapis.com/google.protobuf.StringValue', value: String(Date.now()) };
      params.DURATION_MS = { '@type': 'type.googleapis.com/google.protobuf.StringValue', value: String(_rand(100, 600)) };
    }
    if (et === 'GRID_SCROLL_DEPTH') {
      params.MEDIA_GENERATION_PAYGATE_TIER = { '@type': 'type.googleapis.com/google.protobuf.StringValue', value: 'PAYGATE_TIER_TWO' };
    }
    events.push({
      eventType: et,
      metadata: {
        sessionId: _telemetrySessionId,
        createTime: new Date().toISOString(),
        additionalParams: params,
      },
    });
  }
  return { events };
}

async function sendTelemetry() {
  if (!flowKey || state === 'off') return;

  const headers = {
    'Content-Type': 'text/plain;charset=UTF-8',
    'authorization': `Bearer ${flowKey}`,
  };

  // Telemetry is silent — don't show in request log
  try {
    if (Math.random() < 0.5) {
      await fetch(`https://aisandbox-pa.googleapis.com/v1:batchLog`, {
        method: 'POST', headers, credentials: 'include',
        body: JSON.stringify(_buildBatchLogPayload()),
      });
    } else {
      await fetch(`https://aisandbox-pa.googleapis.com/v1/flow:batchLogFrontendEvents`, {
        method: 'POST', headers, credentials: 'include',
        body: JSON.stringify(_buildFrontendEventsPayload()),
      });
    }
  } catch {}
}

// Send telemetry at random intervals (45-120s) to look organic
function scheduleTelemetry() {
  const delay = _rand(45, 120) * 1000;
  setTimeout(async () => {
    await sendTelemetry();
    scheduleTelemetry(); // reschedule with new random interval
  }, delay);
}

// Refresh session ID every ~30min like a real user
setInterval(() => { _telemetrySessionId = `;${Date.now()}`; }, _rand(25, 35) * 60 * 1000);

scheduleTelemetry();

console.log('[FlowAgent] Extension loaded');
