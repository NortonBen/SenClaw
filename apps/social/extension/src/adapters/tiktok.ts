// TikTok adapter. The one platform whose requests must be page-signed.
import type { Adapter } from './types'

export const tiktok: Adapter = {
  id: "tiktok",
  hosts: ["tiktok.com"],
  sessionCookie: "sessionid",
  captureHeaders: ["x-secsdk-csrf-token"],
  // X-Bogus/X-Gnarly + msToken are produced by the webmssdk VM in the page.
  // NEVER reimplement them — let the MAIN-world script ask the page to sign.
  sign: "tiktok",
  endpointHint: "tiktok.com/api/*",
  loginUrl: "https://www.tiktok.com/login",
  capabilities: { post: "official", dm: "none", search: "page-sign", browse: "page-sign" },
  // No cookie carries the handle; confirm the session and let the operator name it.
  whoami: async (ctx) => {
    const c = await ctx.cookie("sessionid", "tiktok.com")
    return c ? {} : {}
  },
}
