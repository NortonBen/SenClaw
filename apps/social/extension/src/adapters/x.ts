// X (Twitter) adapter.
import type { Adapter } from './types'

export const x: Adapter = {
  id: "x",
  hosts: ["x.com", "twitter.com"],
  sessionCookie: "auth_token",
  // ct0 (=x-csrf-token) is the in-page token; bearer is hardcoded in site JS.
  captureHeaders: ["authorization", "x-csrf-token"],
  sign: "none", // ct0 is a cookie mirror, no page-VM signature needed
  endpointHint: "x.com/i/api/graphql/*",
  loginUrl: "https://x.com/home",
  capabilities: { post: "official", dm: "replay", search: "replay", browse: "replay" },
  // Real handle via the site's own settings endpoint. Uses a captured bearer/ct0
  // when available, else the stable public web bearer + the ct0 cookie — so it
  // works even without waiting for the timeline to fire an authenticated request.
  whoami: async (ctx) => {
    const PUBLIC_BEARER =
      "Bearer AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs=1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA"
    const bearer = ctx.headers["authorization"] || PUBLIC_BEARER
    const ct0 = ctx.headers["x-csrf-token"] || (await ctx.cookie("ct0", "x.com")) || ""
    if (ct0) {
      const r = await ctx.fetch({
        url: "https://api.x.com/1.1/account/settings.json",
        headers: { authorization: bearer, "x-csrf-token": ct0 },
      })
      const sn = (r.json as { screen_name?: string } | null)?.screen_name
      if (sn) return { handle: "@" + sn, name: sn }
    }
    const twid = await ctx.cookie("twid", "x.com")
    const id = twid && decodeURIComponent(twid).replace(/^u=/, "")
    return id ? { id } : {}
  },
}
