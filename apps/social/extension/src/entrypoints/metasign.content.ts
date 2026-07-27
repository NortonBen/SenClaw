// MAIN-world reader for Meta (Facebook) page tokens.
//
// Runs in the page's own JS context so it can reach Facebook's module system
// (`require`) — but modern FB builds don't always expose `window.require`, so we
// fall back to scraping the same values out of the page's inline bootstrap JSON.
// It reads the real logged-in identity plus the fb_dtsg/lsd/access_token that
// web-API (GraphQL) requests must carry, and posts them to the window where the
// ISOLATED relay forwards them to the service worker.
//
// These are long-lived Facebook internals, not the rotating anti-bot signers.

export default defineContentScript({
  matches: ['*://*.facebook.com/*'],
  world: 'MAIN',
  runAt: 'document_end',
  main() {
    const req = (window as unknown as { require?: (name: string) => unknown }).require

    function viaRequire(name: string): Record<string, unknown> | null {
      try {
        return (req && (req(name) as Record<string, unknown>)) || null
      } catch {
        return null
      }
    }

    // Scrape a value out of the page's inline scripts (the __d/RelayPrefetched
    // bootstrap JSON) when `require` isn't reachable.
    function scrape(re: RegExp): string {
      try {
        const m = document.documentElement.innerHTML.match(re)
        return m ? m[1] : ''
      } catch {
        return ''
      }
    }

    function jazoest(dtsg: string): string {
      let sum = 0
      for (let i = 0; i < dtsg.length; i++) sum += dtsg.charCodeAt(i)
      return '2' + sum
    }

    function collect() {
      const cui = viaRequire('CurrentUserInitialData') // { ACCOUNT_ID, NAME, USER_ID }
      const dtsg = viaRequire('DTSGInitialData') // { token }
      const lsd = viaRequire('LSD') // { token }

      let id = cui ? String(cui.ACCOUNT_ID || cui.USER_ID || '') : ''
      let name = cui ? String(cui.NAME || '') : ''
      let fb_dtsg = dtsg ? String(dtsg.token || '') : ''
      let lsdTok = lsd ? String(lsd.token || '') : ''

      // Fallbacks: pull the same fields out of the page HTML.
      if (!id) id = scrape(/"(?:ACCOUNT_ID|USER_ID)":"(\d{5,})"/)
      if (!name) name = scrape(/"NAME":"([^"]{1,120})"/).replace(/\\u([0-9a-fA-F]{4})/g, (_, h) => String.fromCharCode(parseInt(h, 16)))
      if (!fb_dtsg) fb_dtsg = scrape(/"DTSGInitialData"[^}]*?"token":"([^"]+)"/) || scrape(/name="fb_dtsg" value="([^"]+)"/)
      if (!lsdTok) lsdTok = scrape(/\["LSD",\[\],\{"token":"([^"]+)"/)
      const access_token = scrape(/"accessToken":"(EAA[A-Za-z0-9]+)"/) || scrape(/access_token=(EAA[A-Za-z0-9]+)/)

      return { id, name, fb_dtsg, lsd: lsdTok, access_token }
    }

    function publish() {
      const { id, name, fb_dtsg, lsd, access_token } = collect()
      // Nothing useful yet — let a later retry try again.
      if (!id && !fb_dtsg && !access_token) return false

      window.postMessage(
        {
          __senclaw_social: 'meta_tokens',
          platform: 'facebook',
          id,
          name,
          fb_dtsg,
          lsd,
          access_token,
          jazoest: fb_dtsg ? jazoest(fb_dtsg) : '',
        },
        '*',
      )
      // Keep retrying until we have at least identity AND a write token.
      return !!(id && fb_dtsg)
    }

    if (!publish()) {
      let tries = 0
      const t = setInterval(() => {
        tries += 1
        if (publish() || tries >= 8) clearInterval(t)
      }, 1200)
    }

    // Re-post while the tab stays open. MV3 kills the idle service worker (and
    // its in-memory token cache); a periodic re-post wakes it and refreshes the
    // cache, so WhoAmI works even long after the page first loaded.
    setInterval(publish, 20000)
  },
})
