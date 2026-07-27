const $ = (id) => document.getElementById(id);

async function load() {
  const s = await chrome.storage.local.get(['wsPort', 'httpPort', 'ytContext']);
  $('wsPort').value = s.wsPort || 9223;
  $('httpPort').value = s.httpPort || 4491;
  const ctx = s.ytContext;
  $('status').innerHTML = ctx?.clientVersion
    ? `<span class="ok">✓ Đọc được YouTube (client ${ctx.clientVersion})</span>`
    : `<span class="off">Chưa thấy phiên YouTube — mở youtube.com đã đăng nhập</span>`;
}

$('save').addEventListener('click', async () => {
  const wsPort = parseInt($('wsPort').value, 10) || 9223;
  const httpPort = parseInt($('httpPort').value, 10) || 4491;
  await chrome.storage.local.set({ wsPort, httpPort });
  $('status').textContent = 'Đã lưu. Đang kết nối lại…';
  // The background worker reconnects on its keepalive alarm; nudge it now.
  try {
    await chrome.runtime.sendMessage({ type: 'reconnect' });
  } catch {
    /* ignore */
  }
  setTimeout(load, 800);
});

load();
