// Facebook adapter.
import type { Adapter } from './types'

export const facebook: Adapter = {
  id: "facebook",
  hosts: ["facebook.com"],
  sessionCookie: "c_user",
  captureHeaders: ["x-fb-lsd"],
  sign: "meta", // fb_dtsg/jazoest/lsd read from the page by the MAIN-world script
  endpointHint: "facebook.com/api/graphql/ (doc_id)",
  loginUrl: "https://www.facebook.com/login",
  capabilities: { post: "official", dm: "dom", search: "replay", browse: "replay" },
  // The MAIN-world content script reads CurrentUserInitialData (real name + id)
  // and DTSGInitialData (fb_dtsg) off the page and caches them; the c_user cookie
  // backs up the id. The handle is the real username (vanity), resolved from the
  // /me/ redirect — NOT the display name. `web_config` carries the actual
  // web-session tokens so the app can persist + use them for web-API access.
  whoami: async (ctx) => {
    const m = ctx.meta
    const name = m?.name || ""
    const id = m?.id || (await ctx.cookie("c_user", "facebook.com")) || ""

    // Resolve the vanity username via the /me/ redirect (e.g. /bacnd.120).
    let handle = ""
    try {
      const r = await ctx.fetch({ url: "https://www.facebook.com/me/", method: "HEAD" })
      const seg = (r.url || "").split("?")[0].replace(/\/+$/, "").split("/").pop() || ""
      if (seg && seg !== "me" && !seg.startsWith("profile.php") && !/^\d+$/.test(seg)) handle = seg
    } catch {
      /* fall back to the id */
    }
    if (!handle) handle = id
    if (!handle && !name) return {}

    const web: Record<string, string> = {}
    if (id) web.user_id = id
    if (m?.fb_dtsg) web.fb_dtsg = m.fb_dtsg
    if (m?.lsd) web.lsd = m.lsd
    if (m?.access_token) web.access_token = m.access_token

    return {
      handle,
      name,
      id,
      tokens: { fb_dtsg: !!m?.fb_dtsg, lsd: !!m?.lsd, access_token: !!m?.access_token },
      web_config: Object.keys(web).length ? web : undefined,
    }
  },
}
