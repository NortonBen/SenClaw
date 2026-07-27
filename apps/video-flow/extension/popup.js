const TYPE_LABELS = {
  GENERATE_IMAGE:           'GEN IMAGE',
  REGENERATE_IMAGE:         'REGEN IMAGE',
  EDIT_IMAGE:               'EDIT IMAGE',
  GENERATE_CHARACTER_IMAGE: 'GEN REF',
  REGENERATE_CHARACTER_IMAGE: 'REGEN REF',
  EDIT_CHARACTER_IMAGE:     'EDIT REF',
  GENERATE_VIDEO:           'GEN VIDEO',
  GENERATE_VIDEO_REFS:      'GEN VIDEO FROM REFS',
  UPSCALE_VIDEO:            'UPSCALE VIDEO',
  GEN_IMG:                  'GEN IMAGE',
  GEN_VID:                  'GEN VIDEO',
  GEN_VID_REF:              'GEN VIDEO FROM REFS',
  UPSCALE:                  'UPSCALE VIDEO',
  TRACKING:                 'TRACKING',
  URL_REFRESH:              'URL REFRESH',
};

function formatType(type) {
  if (!type) return '—';
  return TYPE_LABELS[type] || type.slice(0, 12).toUpperCase();
}

function formatTime(iso) {
  if (!iso) return '—';
  try {
    const d = new Date(iso);
    const hh = String(d.getHours()).padStart(2, '0');
    const mm = String(d.getMinutes()).padStart(2, '0');
    const ss = String(d.getSeconds()).padStart(2, '0');
    return `${hh}:${mm}:${ss}`;
  } catch {
    return '—';
  }
}

function formatAge(ms) {
  if (ms == null) return '—';
  const min = Math.floor(ms / 60000);
  if (min < 1) return 'vừa lấy';
  if (min < 60) return `${min} phút trước`;
  return `${Math.floor(min / 60)}h${min % 60 ? ` ${min % 60}p` : ''} trước`;
}

function escHtml(str) {
  return String(str)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function badgeHtml(status) {
  if (status === 'COMPLETED' || status === 'success') {
    return '<span class="badge badge-ok">&#10003; xong</span>';
  } else if (status === 'FAILED' || status === 'failed' || (typeof status === 'number' && status >= 400)) {
    return '<span class="badge badge-fail">&#10007; lỗi</span>';
  } else if (status === 'PROCESSING') {
    return '<span class="badge badge-proc">&#9203; đang chạy</span>';
  } else {
    return '<span class="badge badge-proc">&#9203; đã gửi</span>';
  }
}

// ---- status ---------------------------------------------------------------

/** Token older than this is likely expired (Google Flow tokens last ~60 min). */
const TOKEN_STALE_MS = 55 * 60 * 1000;

function setDot(id, cls) {
  const el = document.getElementById(id);
  el.className = `dot${cls ? ` ${cls}` : ''}`;
}

function setVal(id, text, cls) {
  const el = document.getElementById(id);
  el.textContent = text;
  el.className = `status-value${cls ? ` ${cls}` : ''}`;
}

/** The one line that tells the user what to actually do next. */
function renderHint(st, backendOk) {
  const hint = document.getElementById('hint');
  let msg;
  let bad = true;
  if (!st) {
    msg = 'Không đọc được trạng thái — thử tải lại extension.';
  } else if (st.manualDisconnect) {
    msg = 'Đang ngắt kết nối thủ công. Bấm “Kết nối lại” để bật lại cầu nối.';
  } else if (!st.agentConnected && !backendOk) {
    msg = `Chưa thấy app Video Flow ở cổng ${st.wsPort}/${st.httpPort}. Mở app trong SenClaw, hoặc sửa cổng ở mục “Kết nối” bên dưới.`;
  } else if (!st.agentConnected) {
    msg = `API app chạy ở :${st.httpPort} nhưng cầu nối WS :${st.wsPort} chưa nối được. Kiểm tra FLOWKIT_WS_PORT của app.`;
  } else if (!st.flowKeyPresent) {
    msg = 'Đã nối app, nhưng chưa bắt được token. Bấm “Mở Google Flow” và đăng nhập labs.google để lấy token.';
  } else if (st.tokenAge != null && st.tokenAge > TOKEN_STALE_MS) {
    msg = 'Token đã cũ (>55 phút) và có thể hết hạn — bấm “Lấy lại token”.';
  } else {
    msg = 'Sẵn sàng: app đã nối và token còn hiệu lực. Có thể sinh ảnh/video từ Video Flow.';
    bad = false;
  }
  hint.textContent = msg;
  hint.className = `hint${bad ? ' bad' : ''}`;
}

function renderStatus(st, backendOk, backendErr) {
  if (!st) {
    setDot('dot-agent', 'bad');
    setVal('val-agent', 'không rõ', 'bad');
    renderHint(null, false);
    return;
  }

  const agentOk = !!st.agentConnected;
  setDot('dot-agent', agentOk ? 'ok' : 'bad');
  setVal('val-agent', agentOk ? `đã nối :${st.wsPort}` : (st.manualDisconnect ? 'đã ngắt' : `mất kết nối :${st.wsPort}`), agentOk ? 'ok' : 'bad');

  setDot('dot-backend', backendOk ? 'ok' : 'bad');
  setVal('val-backend', backendOk ? `:${st.httpPort} ok` : `:${st.httpPort} ${backendErr ? 'không tới được' : '—'}`, backendOk ? 'ok' : 'bad');

  const tokenStale = st.tokenAge != null && st.tokenAge > TOKEN_STALE_MS;
  setDot('dot-token', st.flowKeyPresent ? (tokenStale ? 'warn' : 'ok') : 'bad');
  setVal('val-token', st.flowKeyPresent ? formatAge(st.tokenAge) : 'chưa có', st.flowKeyPresent && !tokenStale ? 'ok' : 'bad');

  const m = st.metrics || {};
  document.getElementById('m-total').textContent = m.requestCount ?? 0;
  document.getElementById('m-ok').textContent = m.successCount ?? 0;
  document.getElementById('m-fail').textContent = m.failedCount ?? 0;

  // Only fill the port inputs when untouched, so typing isn't clobbered by a refresh.
  const inWs = document.getElementById('in-ws');
  const inHttp = document.getElementById('in-http');
  if (document.activeElement !== inWs && !inWs.dataset.dirty) inWs.value = st.wsPort ?? '';
  if (document.activeElement !== inHttp && !inHttp.dataset.dirty) inHttp.value = st.httpPort ?? '';

  renderHint(st, backendOk);
}

function refreshStatus() {
  chrome.runtime.sendMessage({ type: 'STATUS' }, (st) => {
    if (chrome.runtime.lastError) return renderStatus(null, false);
    chrome.runtime.sendMessage({ type: 'PING_BACKEND' }, (res) => {
      if (chrome.runtime.lastError) return renderStatus(st, false);
      renderStatus(st, !!res?.ok, res?.error);
    });
  });
}

// ---- log ------------------------------------------------------------------

function renderLog(entries) {
  const list = document.getElementById('log-list');
  const countEl = document.getElementById('log-count');

  if (!entries || entries.length === 0) {
    list.innerHTML = '<div class="log-empty">Chưa có yêu cầu nào</div>';
    countEl.textContent = '0';
    return;
  }

  countEl.textContent = entries.length;

  list.innerHTML = entries.map((entry, i) => {
    const shortId = entry.id ? String(entry.id).slice(0, 8) : '—';
    const type = formatType(entry.type || entry.method);
    const time = formatTime(entry.time || entry.timestamp);
    const status = entry.status || 'pending';
    const error = entry.error || '';

    const urlDisplay = entry.url
      ? `<div class="detail-section">
           <div class="detail-label">URL</div>
           <div class="detail-value url" title="${escHtml(entry.url)}">${escHtml(entry.url)}</div>
         </div>`
      : '';

    const payloadDisplay = entry.payloadSummary
      ? `<div class="detail-section">
           <div class="detail-label">Payload</div>
           <div class="detail-value">${escHtml(entry.payloadSummary)}</div>
         </div>`
      : '';

    const responseDisplay = entry.responseSummary
      ? `<div class="detail-section">
           <div class="detail-label">Phản hồi${entry.httpStatus ? ` (${entry.httpStatus})` : ''}</div>
           <div class="detail-value">${escHtml(entry.responseSummary)}</div>
         </div>`
      : '';

    const errorDisplay = error
      ? `<div class="detail-section">
           <div class="detail-label">Lỗi</div>
           <div class="detail-value detail-error">${escHtml(error)}</div>
         </div>`
      : '';

    const hasDetails = entry.url || entry.payloadSummary || entry.responseSummary || error;

    return `<div class="entry" data-idx="${i}">
      <div class="entry-row">
        <span class="entry-id">${escHtml(shortId)}</span>
        <span class="entry-type">${escHtml(type)}</span>
        <span class="entry-time">${escHtml(time)}</span>
        ${badgeHtml(status)}
        ${hasDetails ? '<span class="expand-icon">&#9654;</span>' : '<span class="expand-icon" style="visibility:hidden">&#9654;</span>'}
      </div>
      ${hasDetails ? `<div class="entry-details">${urlDisplay}${payloadDisplay}${responseDisplay}${errorDisplay}</div>` : ''}
    </div>`;
  }).join('');

  list.querySelectorAll('.entry-row').forEach((row) => {
    row.addEventListener('click', () => {
      const entry = row.closest('.entry');
      if (entry.querySelector('.entry-details')) entry.classList.toggle('open');
    });
  });
}

function refreshLog() {
  chrome.runtime.sendMessage({ type: 'REQUEST_LOG' }, (data) => {
    if (chrome.runtime.lastError) return;
    if (data && data.log) renderLog(data.log);
  });
}

// ---- wiring ---------------------------------------------------------------

document.getElementById('btn-panel').addEventListener('click', () => {
  chrome.windows.getCurrent((win) => chrome.sidePanel.open({ windowId: win.id }));
});

document.getElementById('btn-reconnect').addEventListener('click', () => {
  chrome.runtime.sendMessage({ type: 'RECONNECT' }, () => setTimeout(refreshStatus, 400));
});

document.getElementById('btn-flow').addEventListener('click', () => {
  chrome.runtime.sendMessage({ type: 'OPEN_FLOW_TAB' }, () => {});
});

document.getElementById('btn-token').addEventListener('click', () => {
  chrome.runtime.sendMessage({ type: 'REFRESH_TOKEN' }, () => setTimeout(refreshStatus, 800));
});

document.getElementById('btn-clear').addEventListener('click', () => {
  chrome.runtime.sendMessage({ type: 'CLEAR_LOG' }, () => refreshLog());
});

for (const id of ['in-ws', 'in-http']) {
  document.getElementById(id).addEventListener('input', (e) => {
    e.target.dataset.dirty = '1';
  });
}

document.getElementById('btn-save').addEventListener('click', () => {
  const wsPort = Number(document.getElementById('in-ws').value) || 9222;
  const httpPort = Number(document.getElementById('in-http').value) || 4460;
  chrome.runtime.sendMessage({ type: 'SET_PORTS', wsPort, httpPort }, () => {
    const msg = document.getElementById('save-msg');
    msg.textContent = 'Đã lưu';
    document.getElementById('in-ws').dataset.dirty = '';
    document.getElementById('in-http').dataset.dirty = '';
    setTimeout(() => { msg.textContent = ''; }, 2000);
    setTimeout(refreshStatus, 600);
  });
});

// Live log updates pushed by the service worker.
chrome.runtime.onMessage.addListener((msg) => {
  if (msg?.type === 'REQUEST_LOG_UPDATE' && msg.log) renderLog(msg.log);
});

refreshStatus();
refreshLog();
setInterval(refreshStatus, 3000);
