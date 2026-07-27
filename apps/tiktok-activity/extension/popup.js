const LOG_KEY = "ctrlLog";

function pad(n) {
  return String(n).padStart(2, "0");
}
function fmtTime(t) {
  const d = new Date(t);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function esc(s) {
  return String(s == null ? "" : s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" })[c]);
}

async function refreshStatus() {
  const { connected, targetTabId } = await chrome.storage.local.get(["connected", "targetTabId"]);
  document.getElementById("dot").className = "dot" + (connected ? " on" : "");
  document.getElementById("status").textContent = connected ? "Đã kết nối app (:9225)" : "Chưa kết nối app";
  let tabLabel = "—";
  if (targetTabId) {
    try {
      const t = await chrome.tabs.get(targetTabId);
      tabLabel = (t.title || t.url || String(targetTabId)).slice(0, 34);
    } catch (_) {
      tabLabel = "(tab đã đóng)";
    }
  }
  document.getElementById("tab").textContent = tabLabel;
}

function renderLog(entries) {
  const el = document.getElementById("log");
  if (!entries || !entries.length) {
    el.innerHTML = '<div class="empty">Chưa có hoạt động</div>';
    return;
  }
  // newest at the bottom; keep the view pinned to the latest unless scrolled up
  const nearBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 30;
  el.innerHTML = entries
    .map((e) => {
      const cls = e.kind === "conn" ? "conn" : e.ok === false ? "err" : "";
      const ms = e.ms != null && e.kind === "cmd" ? `${e.ms}ms` : "";
      return `<div class="e ${cls}"><span class="ts">${fmtTime(e.t)}</span><span class="m">${esc(e.method)}</span><span class="i">${esc(e.info)}</span><span class="ms">${ms}</span></div>`;
    })
    .join("");
  if (nearBottom) el.scrollTop = el.scrollHeight;
}

async function refreshLog() {
  const { [LOG_KEY]: entries } = await chrome.storage.local.get(LOG_KEY);
  renderLog(entries || []);
}

document.getElementById("ctrl").addEventListener("click", async () => {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab) return;
  await chrome.storage.local.set({ targetTabId: tab.id });
  refreshStatus();
});

document.getElementById("openPanel").addEventListener("click", async () => {
  const url = chrome.runtime.getURL("panel.html");
  const existing = await chrome.tabs.query({ url });
  if (existing.length) {
    chrome.tabs.update(existing[0].id, { active: true });
    if (existing[0].windowId != null) chrome.windows.update(existing[0].windowId, { focused: true });
  } else {
    chrome.tabs.create({ url });
  }
});

document.getElementById("clear").addEventListener("click", () => {
  chrome.runtime.sendMessage({ type: "clear_log" }).catch(() => {});
  chrome.storage.local.set({ [LOG_KEY]: [] });
  renderLog([]);
});

// Live updates: react to storage writes from the worker, plus a slow poll as a
// fallback (status/tab title can change without a storage event).
chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  if (changes[LOG_KEY]) renderLog(changes[LOG_KEY].newValue || []);
  if (changes.connected || changes.targetTabId) refreshStatus();
});

refreshStatus();
refreshLog();
setInterval(refreshStatus, 1500);
