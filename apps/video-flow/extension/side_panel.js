/**
 * Video Flow — Side Panel
 */

// ── Type label map ───────────────────────────────────────────

const TYPE_LABELS = {
  GENERATE_IMAGE:             'GEN IMAGE',
  REGENERATE_IMAGE:           'REGEN IMG',
  EDIT_IMAGE:                 'EDIT IMG',
  GENERATE_CHARACTER_IMAGE:   'GEN REF',
  REGENERATE_CHARACTER_IMAGE: 'REGEN REF',
  EDIT_CHARACTER_IMAGE:       'EDIT REF',
  GENERATE_VIDEO:             'GEN VIDEO',
  GENERATE_VIDEO_REFS:        'VID+REFS',
  UPSCALE_VIDEO:              'UPSCALE',
  IMAGE_GENERATION:           'GEN IMAGE',
  VIDEO_GENERATION:           'GEN VIDEO',
  GEN_IMG:                    'GEN IMAGE',
  GEN_VID:                    'GEN VIDEO',
  GEN_VID_REF:                'VID+REFS',
  UPSCALE:                    'UPSCALE',
  UPS_IMG:                    'UPS IMG',
  POLL:                       'POLL',
  CREDITS:                    'CREDITS',
  CREATE_PROJECT:             'NEW PROJ',
  UPLOAD:                     'UPLOAD',
  MEDIA:                      'MEDIA',
  TRACKING:                   'TRACK',
  URL_REFRESH:                'URL REF',
  TRPC:                       'TRPC',
  API:                        'API',
};

function formatType(type) {
  if (!type) return '—';
  return TYPE_LABELS[type] || type.replace(/^(GENERATE_|REGENERATE_)/, '').slice(0, 8).toUpperCase();
}

// ── Time formatting ──────────────────────────────────────────

function formatTime(iso) {
  if (!iso) return '—';
  try {
    const d = new Date(iso);
    return `${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}:${String(d.getSeconds()).padStart(2,'0')}`;
  } catch { return '—'; }
}

function escHtml(str) {
  return String(str ?? '')
    .replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

function truncate(str, len) {
  if (!str || str.length <= len) return str;
  return str.slice(0, len) + '…';
}

// Pretty-print a JSON string; return it unchanged if it isn't valid JSON (or was
// truncated to the storage cap, which would no longer parse).
function prettyJson(str) {
  try { return JSON.stringify(JSON.parse(str), null, 2); } catch { return str; }
}

// The Flow project id this request belongs to. Prefer the request payload
// (`clientContext.projectId`), fall back to the response (`workflows[].projectId`
// / `media[].projectId`), then a loose UUID scan — so the button works even when
// a body was truncated to the storage cap and no longer parses as JSON.
function extractProjectId(entry) {
  const payload = entry.payloadFull || entry.payloadSummary || '';
  const response = entry.responseFull || entry.responseSummary || '';
  try {
    const p = JSON.parse(payload);
    const id = p?.clientContext?.projectId;
    if (id) return id;
  } catch { /* fall through */ }
  const m = payload.match(/"projectId"\s*:\s*"([0-9a-f-]{36})"/i)
    || response.match(/"projectId"\s*:\s*"([0-9a-f-]{36})"/i);
  return m ? m[1] : '';
}

// ── Detect media type from URL / request type ────────────────

function detectMediaKind(entry) {
  const url = entry.outputUrl || '';
  if (!url) return null;
  // Kind tagged at submit time by request type (VID/UPSCALE → video) wins.
  if (entry.mediaKind === 'video' || entry.mediaKind === 'image') return entry.mediaKind;
  const lo = url.toLowerCase();
  if (lo.includes('/video/') || lo.match(/\.(mp4|webm|mov)/)) return 'video';
  if (lo.includes('/image/') || lo.match(/\.(png|jpg|jpeg|webp)/)) return 'image';
  // Fallback: classify by request type
  const t = (entry.type || '').toUpperCase();
  if (t.includes('VID') || t.includes('UPSCALE')) return 'video';
  return 'image';
}

// ── Status update ────────────────────────────────────────────

function updateStatus(data) {
  if (!data) return;

  const dot = document.getElementById('conn-dot');
  dot.className = data.agentConnected ? 'on' : '';

  const toggle = document.getElementById('main-toggle');
  const toggleLabel = document.getElementById('toggle-label');
  const isOn = data.state !== 'off';
  toggle.checked = isOn;
  toggleLabel.textContent = isOn ? 'ON' : 'OFF';

  const stateBadge = document.getElementById('state-badge');
  const st = data.state || 'off';
  stateBadge.textContent = st;
  stateBadge.className = st;

  const tokenEl = document.getElementById('token-status');
  if (data.flowKeyPresent) {
    const ageMs = data.tokenAge || 0;
    const ageMin = Math.round(ageMs / 60000);
    if (ageMs > 3600000) {
      tokenEl.textContent = 'token hết hạn';
      tokenEl.className = 'warn';
    } else {
      tokenEl.textContent = ageMin < 1 ? 'token vừa lấy' : `token ${ageMin} phút trước`;
      tokenEl.className = 'ok';
    }
    if (ageMs > 3300000 && data.agentConnected) {
      chrome.runtime.sendMessage({ type: 'REFRESH_TOKEN' });
    }
  } else {
    tokenEl.textContent = 'chưa có token';
    tokenEl.className = 'bad';
  }

  const m = data.metrics || {};
  document.getElementById('m-total').textContent   = m.requestCount || 0;
  document.getElementById('m-success').textContent = m.successCount || 0;
  document.getElementById('m-failed').textContent  = m.failedCount  || 0;
}

// ── Request log ──────────────────────────────────────────────

let _logEntries = [];

function updateRequestLog(entries) {
  const tbody    = document.getElementById('log-body');
  const countEl  = document.getElementById('log-count');

  _logEntries = entries || [];
  countEl.textContent = _logEntries.length;

  if (!_logEntries.length) {
    tbody.innerHTML = '<tr><td colspan="6" class="log-empty">Chưa có yêu cầu nào</td></tr>';
    return;
  }

  const rows = _logEntries.map((entry) => {
    const shortId   = entry.id ? String(entry.id).slice(0, 8) : '—';
    const typeLabel = formatType(entry.type || entry.method);
    const time      = formatTime(entry.time || entry.timestamp || entry.createdAt);
    const status    = entry.status || entry.state || 'pending';
    const mediaKind = detectMediaKind(entry);
    const outputUrl = entry.outputUrl || '';

    // Status badge
    let badge;
    if (status === 'COMPLETED' || status === 'success') {
      badge = '<span class="badge badge-ok">✓ done</span>';
    } else if (status === 'FAILED' || status === 'failed' || (typeof status === 'number' && status >= 400)) {
      badge = '<span class="badge badge-fail">✗ fail</span>';
    } else if (status === 'PROCESSING' || status === 'processing') {
      badge = '<span class="badge badge-proc">⏳ gen</span>';
    } else {
      badge = '<span class="badge badge-proc">⏳ sent</span>';
    }

    // Output cell
    let outCell = '—';
    if (outputUrl && mediaKind === 'image') {
      outCell = `<img class="thumb" src="${escHtml(outputUrl)}" data-url="${escHtml(outputUrl)}" data-kind="image" title="Xem ảnh" alt="out">`;
    } else if (outputUrl && mediaKind === 'video') {
      outCell = `<span class="play-btn" data-url="${escHtml(outputUrl)}" data-kind="video" title="Xem video">▶</span>`;
    }

    return `<tr>
      <td class="td-id" data-request-id="${escHtml(entry.id || '')}">${escHtml(shortId)}</td>
      <td class="td-type">${escHtml(typeLabel)}</td>
      <td class="td-time">${escHtml(time)}</td>
      <td>${badge}</td>
      <td class="td-out">${outCell}</td>
      <td class="td-del">
        <button class="del-btn" data-del-id="${escHtml(entry.id || '')}" title="Xóa">×</button>
      </td>
    </tr>`;
  });

  tbody.innerHTML = rows.join('');

  // Click: row ID → detail modal
  tbody.querySelectorAll('.td-id[data-request-id]').forEach((td) => {
    td.addEventListener('click', () => {
      const id = td.getAttribute('data-request-id');
      if (id) showDetail(id);
    });
  });

  // Click: thumbnail / play button → preview modal
  tbody.querySelectorAll('[data-url][data-kind]').forEach((el) => {
    el.addEventListener('click', () => {
      const url  = el.getAttribute('data-url');
      const kind = el.getAttribute('data-kind');
      if (url) openPreview(url, kind);
    });
  });

  // Click: delete button
  tbody.querySelectorAll('.del-btn[data-del-id]').forEach((btn) => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const id = btn.getAttribute('data-del-id');
      if (!id) return;
      chrome.runtime.sendMessage({ type: 'DELETE_LOG_ENTRY', id }, () => {
        if (chrome.runtime.lastError) return;
        // Optimistic local remove
        _logEntries = _logEntries.filter((e) => e.id !== id);
        updateRequestLog(_logEntries);
      });
    });
  });
}

// ── Detail modal ─────────────────────────────────────────────

function showDetail(reqId) {
  const entry = _logEntries.find((e) => e.id === reqId);
  if (!entry) return;

  document.getElementById('detail-title').textContent = `Request ${String(reqId).slice(0, 12)}`;

  // Project this request belongs to — pulled from its own payload/response so the
  // "open project" button targets exactly this task's Flow project.
  const flowProjectId = extractProjectId(entry);
  const openProjectHtml = flowProjectId
    ? `<div class="detail-actions">
         <button class="open-project-btn" data-project="${escHtml(flowProjectId)}">▶ Mở project trên Google Flow</button>
       </div>`
    : '';

  const mediaKind = detectMediaKind(entry);
  const outputUrl = entry.outputUrl || '';

  let mediaHtml = '';
  if (outputUrl && mediaKind === 'image') {
    mediaHtml = `<div class="detail-media">
      <img src="${escHtml(outputUrl)}" data-url="${escHtml(outputUrl)}" data-kind="image" title="Click để xem đầy đủ" alt="output">
    </div>`;
  } else if (outputUrl && mediaKind === 'video') {
    mediaHtml = `<div class="detail-media">
      <video src="${escHtml(outputUrl)}" controls></video>
    </div>`;
  }

  // Short fields shown inline.
  const fields = [
    ['ID',       entry.id],
    ['Type',     formatType(entry.type || entry.method)],
    ['Time',     formatTime(entry.time || entry.timestamp || entry.createdAt)],
    ['Status',   entry.status || entry.state || 'pending'],
    ['HTTP',     entry.httpStatus || '—'],
    ['Error',    entry.error || '—'],
  ];
  const rowsHtml = fields.map(([label, value]) => {
    let cls = 'detail-val';
    if (label === 'Error' && value && value !== '—') cls += ' err';
    if (label === 'Status' && (value === 'COMPLETED' || value === 'success')) cls += ' good';
    return `<div class="detail-row">
      <div class="detail-lbl">${escHtml(label)}</div>
      <div class="${cls}">${escHtml(String(value ?? '—'))}</div>
    </div>`;
  }).join('');

  // Long fields shown in full, in a scrollable block with a Copy button.
  const blocks = [
    ['URL', entry.url || ''],
    ['Payload', entry.payloadFull || entry.payloadSummary || ''],
    ['Response', entry.responseFull || entry.responseSummary || ''],
  ];
  const blocksHtml = blocks.map(([label, value], i) => {
    if (!value) return '';
    const pretty = label === 'URL' ? value : prettyJson(value);
    return `<div class="detail-block">
      <div class="detail-block-head">
        <span class="detail-lbl">${escHtml(label)}</span>
        <button class="copy-btn" data-copy-idx="${i}">Copy</button>
      </div>
      <pre class="detail-pre" data-copy-src="${i}">${escHtml(pretty)}</pre>
    </div>`;
  }).join('');

  document.getElementById('detail-body').innerHTML = openProjectHtml + mediaHtml + rowsHtml + blocksHtml;

  // Open this task's project on Google Flow in a new tab.
  const openBtn = document.querySelector('#detail-body .open-project-btn');
  if (openBtn) {
    openBtn.addEventListener('click', () => {
      const pid = openBtn.getAttribute('data-project');
      window.open(`https://labs.google/fx/vi/tools/flow/project/${encodeURIComponent(pid)}`, '_blank');
    });
  }

  // Copy buttons → clipboard (raw value, not the escaped HTML).
  const rawByIdx = Object.fromEntries(blocks.map(([label, value], i) => [i, value]));
  document.querySelectorAll('#detail-body .copy-btn').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const idx = btn.getAttribute('data-copy-idx');
      try {
        await navigator.clipboard.writeText(rawByIdx[idx] || '');
        const prev = btn.textContent; btn.textContent = 'Đã copy ✓';
        setTimeout(() => { btn.textContent = prev; }, 1200);
      } catch {
        // Fallback: select the <pre> text so the user can Cmd/Ctrl-C.
        const pre = document.querySelector(`#detail-body .detail-pre[data-copy-src="${idx}"]`);
        if (pre) { const r = document.createRange(); r.selectNodeContents(pre); const s = getSelection(); s.removeAllRanges(); s.addRange(r); }
      }
    });
  });

  // Allow clicking detail image to open full preview
  const detailImg = document.querySelector('#detail-body .detail-media img[data-url]');
  if (detailImg) {
    detailImg.addEventListener('click', () => openPreview(detailImg.getAttribute('data-url'), 'image'));
  }

  document.getElementById('detail-overlay').classList.add('open');
}

document.getElementById('detail-close').addEventListener('click', () => {
  document.getElementById('detail-overlay').classList.remove('open');
});

document.getElementById('detail-overlay').addEventListener('click', (e) => {
  if (e.target === e.currentTarget) e.currentTarget.classList.remove('open');
});

// ── Media preview modal ──────────────────────────────────────

function openPreview(url, kind) {
  const body = document.getElementById('preview-body');
  const title = document.getElementById('preview-title');
  title.textContent = kind === 'video' ? 'Video Preview' : 'Image Preview';
  if (kind === 'image') {
    body.innerHTML = `<img src="${escHtml(url)}" alt="preview">`;
  } else {
    body.innerHTML = `<video src="${escHtml(url)}" controls autoplay></video>`;
  }
  document.getElementById('preview-overlay').classList.add('open');
}

document.getElementById('preview-close').addEventListener('click', () => {
  document.getElementById('preview-overlay').classList.remove('open');
  document.getElementById('preview-body').innerHTML = '';
});

document.getElementById('preview-overlay').addEventListener('click', (e) => {
  if (e.target === e.currentTarget) {
    e.currentTarget.classList.remove('open');
    document.getElementById('preview-body').innerHTML = '';
  }
});

// ── Clear all log ────────────────────────────────────────────

document.getElementById('btn-clear-log').addEventListener('click', () => {
  if (!_logEntries.length) return;
  if (!confirm('Xóa toàn bộ request log?')) return;
  chrome.runtime.sendMessage({ type: 'CLEAR_LOG' }, () => {
    if (chrome.runtime.lastError) return;
    updateRequestLog([]);
  });
});

// ── Status fetch ─────────────────────────────────────────────

function fetchStatus() {
  chrome.runtime.sendMessage({ type: 'STATUS' }, (data) => {
    if (chrome.runtime.lastError) return;
    updateStatus(data);
  });
}

function fetchLog() {
  chrome.runtime.sendMessage({ type: 'REQUEST_LOG' }, (data) => {
    if (chrome.runtime.lastError) return;
    if (data && data.log) updateRequestLog(data.log);
  });
}

// ── Push listeners ───────────────────────────────────────────

chrome.runtime.onMessage.addListener((msg) => {
  if (msg.type === 'STATUS_PUSH')          fetchStatus();
  if (msg.type === 'REQUEST_LOG_UPDATE' && msg.log) updateRequestLog(msg.log);
});

// ── Toggle ───────────────────────────────────────────────────

document.getElementById('main-toggle').addEventListener('change', (e) => {
  const msgType = e.target.checked ? 'RECONNECT' : 'DISCONNECT';
  chrome.runtime.sendMessage({ type: msgType }, () => {
    if (chrome.runtime.lastError) return;
    setTimeout(fetchStatus, 400);
  });
});

// ── Action buttons ───────────────────────────────────────────

document.getElementById('btn-flow').addEventListener('click', () => {
  chrome.runtime.sendMessage({ type: 'OPEN_FLOW_TAB' }, () => {
    if (chrome.runtime.lastError) return;
  });
});

document.getElementById('btn-token').addEventListener('click', () => {
  const btn = document.getElementById('btn-token');
  btn.textContent = 'Opening…';
  btn.disabled = true;
  chrome.runtime.sendMessage({ type: 'REFRESH_TOKEN' }, () => {
    btn.textContent = 'Refresh Token';
    btn.disabled = false;
  });
});

// ── Init ─────────────────────────────────────────────────────

document.addEventListener('DOMContentLoaded', () => {
  fetchStatus();
  fetchLog();
});
