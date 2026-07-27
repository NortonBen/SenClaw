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
function fmtDur(ms) {
  if (!ms) return "—";
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

// ---- tabs ----
document.querySelectorAll(".tabs button").forEach((b) => {
  b.addEventListener("click", () => {
    document.querySelectorAll(".tabs button").forEach((x) => x.classList.toggle("active", x === b));
    const tab = b.dataset.tab;
    document.querySelectorAll(".pane").forEach((p) => p.classList.toggle("active", p.id === `pane-${tab}`));
    document.getElementById("logToolbar").style.display = tab === "log" ? "flex" : "none";
    if (tab === "settings") renderTabList();
  });
});

// ---- log ----
let cachedLog = [];

function renderLog() {
  const el = document.getElementById("log");
  const onlyErr = document.getElementById("onlyErr").checked;
  const entries = onlyErr ? cachedLog.filter((e) => e.ok === false) : cachedLog;
  document.getElementById("logCount").textContent = String(cachedLog.length);
  if (!entries.length) {
    el.innerHTML = '<div class="empty">Chưa có hoạt động</div>';
    return;
  }
  const nearBottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 40;
  el.innerHTML = entries
    .map((e) => {
      const cls = e.kind === "conn" ? "conn" : e.ok === false ? "err" : "";
      const ms = e.ms != null && e.kind === "cmd" ? `${e.ms}ms` : "";
      return `<div class="e ${cls}"><span class="ts">${fmtTime(e.t)}</span><span class="m">${esc(e.method)}</span><span class="i">${esc(e.info)}</span><span class="ms">${ms}</span></div>`;
    })
    .join("");
  if (document.getElementById("autoscroll").checked && nearBottom) el.scrollTop = el.scrollHeight;
}

async function loadLog() {
  const { [LOG_KEY]: entries } = await chrome.storage.local.get(LOG_KEY);
  cachedLog = entries || [];
  renderLog();
}

document.getElementById("clear").addEventListener("click", () => {
  chrome.runtime.sendMessage({ type: "clear_log" }).catch(() => {});
  chrome.storage.local.set({ [LOG_KEY]: [] });
  cachedLog = [];
  renderLog();
});
document.getElementById("onlyErr").addEventListener("change", renderLog);

// ---- connection / status ----
async function renderStatus() {
  const { connected, stats } = await chrome.storage.local.get(["connected", "stats"]);
  const s = stats || { cmdCount: 0, errCount: 0, connects: 0, connectedSince: 0 };
  document.getElementById("dot").className = "dot" + (connected ? " on" : "");
  document.getElementById("statusText").textContent = connected ? "Đã kết nối" : "Chưa kết nối";
  document.getElementById("cStatus").textContent = connected ? "Đã kết nối" : "Chưa kết nối";
  document.getElementById("cCmd").textContent = String(s.cmdCount || 0);
  document.getElementById("cErr").textContent = String(s.errCount || 0);
  document.getElementById("cConnects").textContent = String(s.connects || 0);
  document.getElementById("cUptime").textContent = s.connectedSince ? fmtDur(Date.now() - s.connectedSince) : "—";
}

// ---- controlled-tab picker ----
async function renderTabList() {
  const { targetTabId } = await chrome.storage.local.get("targetTabId");
  let curLabel = "—";
  if (targetTabId) {
    try {
      const t = await chrome.tabs.get(targetTabId);
      curLabel = (t.title || t.url || String(targetTabId)).slice(0, 50);
    } catch (_) {
      curLabel = "(tab đã đóng)";
    }
  }
  document.getElementById("sTab").textContent = curLabel;

  const tabs = await chrome.tabs.query({ url: "*://*.tiktok.com/*" });
  const el = document.getElementById("tabList");
  if (!tabs.length) {
    el.innerHTML = '<div class="empty">Không thấy tab tiktok.com — mở tiktok.com rồi bấm nút cạnh tab.</div>';
    return;
  }
  el.innerHTML = tabs
    .map(
      (t) =>
        `<div class="taprow"><span class="title">${esc(t.title || t.url)}</span>${
          t.id === targetTabId ? '<span class="pill">● đang điều khiển</span>' : `<button class="btn primary" data-tab-id="${t.id}">Điều khiển</button>`
        }</div>`,
    )
    .join("");
  el.querySelectorAll("button[data-tab-id]").forEach((b) => {
    b.addEventListener("click", async () => {
      await chrome.storage.local.set({ targetTabId: Number(b.dataset.tabId) });
      renderTabList();
    });
  });
}

// ---- live updates ----
chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  if (changes[LOG_KEY]) {
    cachedLog = changes[LOG_KEY].newValue || [];
    renderLog();
  }
  if (changes.connected || changes.stats) renderStatus();
});

loadLog();
renderStatus();
renderTabList();
setInterval(renderStatus, 1000); // keep uptime ticking
