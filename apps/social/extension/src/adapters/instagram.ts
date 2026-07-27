// Instagram adapter.
import type { Adapter } from './types'

export const instagram: Adapter = {
  id: "instagram",
  hosts: ["instagram.com"],
  sessionCookie: "sessionid",
  captureHeaders: ["x-csrftoken", "x-ig-app-id", "x-ig-www-claim"],
  sign: "meta",
  endpointHint: "i.instagram.com/api/v1/, www.instagram.com/graphql/query",
  loginUrl: "https://www.instagram.com/accounts/login/",
  capabilities: { post: "official", dm: "replay", search: "replay", browse: "replay" },
  // Prefer the real username via the web-API; fall back to the ds_user_id cookie.
  whoami: async (ctx) => {
    const appId = ctx.headers["x-ig-app-id"] || "936619743392459"
    try {
      const r = await ctx.fetch({
        url: "https://i.instagram.com/api/v1/accounts/current_user/?edit=true",
        headers: { "x-ig-app-id": appId },
      })
      const u = (r.json as { user?: { username?: string; full_name?: string; pk?: string } } | null)?.user
      if (u && u.username) return { handle: "@" + u.username, name: u.full_name || "", id: String(u.pk || "") }
    } catch {
      /* fall through to the cookie */
    }
    const c = await ctx.cookie("ds_user_id", "instagram.com")
    return c ? { handle: c, id: c } : {}
  },
}
