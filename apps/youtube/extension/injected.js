// Runs in the PAGE's MAIN world (has access to window.ytcfg). Reads the live
// InnerTube context YouTube ships in the page and posts it to the content script,
// which relays it to the background worker. This keeps the proxied payload's
// clientVersion/visitorData consistent with the session's own requests.
(function () {
  function readCtx() {
    try {
      const cfg = window.ytcfg && window.ytcfg.data_ ? window.ytcfg.data_ : {};
      const ctx = (cfg.INNERTUBE_CONTEXT && cfg.INNERTUBE_CONTEXT.client) || {};
      return {
        clientVersion: cfg.INNERTUBE_CONTEXT_CLIENT_VERSION || ctx.clientVersion || null,
        visitorData: cfg.VISITOR_DATA || ctx.visitorData || null,
        apiKey: cfg.INNERTUBE_API_KEY || null,
        loggedIn: !!cfg.LOGGED_IN,
      };
    } catch {
      return null;
    }
  }

  function post() {
    const data = readCtx();
    if (data && data.clientVersion) {
      window.postMessage({ __senclawYt: true, data }, '*');
    }
  }

  // ytcfg populates shortly after load; try a few times.
  post();
  let n = 0;
  const t = setInterval(() => {
    post();
    if (++n > 10) clearInterval(t);
  }, 1000);
})();
