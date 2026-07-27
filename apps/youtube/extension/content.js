// Content script on youtube.com. Injects injected.js into the MAIN world (to read
// window.ytcfg) and relays the InnerTube context it posts back to the background
// service worker.
(function () {
  try {
    const s = document.createElement('script');
    s.src = chrome.runtime.getURL('injected.js');
    s.onload = () => s.remove();
    (document.head || document.documentElement).appendChild(s);
  } catch {
    /* ignore */
  }

  window.addEventListener('message', (ev) => {
    if (ev.source !== window) return;
    const d = ev.data;
    if (d && d.__senclawYt && d.data) {
      try {
        chrome.runtime.sendMessage({ type: 'yt_context', data: d.data });
      } catch {
        /* worker may be asleep; the alarm will re-sync */
      }
    }
  });
})();
