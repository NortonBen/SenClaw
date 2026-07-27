// YouTube adapter.
//
// Kept for parity with the Rust `Platform::Youtube` (social_connect accepts
// "youtube"). Deep YouTube work lives in the standalone `apps/youtube` app —
// this adapter covers the light path so the platform is never half-wired.
import type { Adapter } from './types'

export const youtube: Adapter = {
  id: "youtube",
  hosts: ["youtube.com"],
  sessionCookie: "SAPISID",
  // InnerTube auth is an SAPISIDHASH Authorization header derived from cookies.
  captureHeaders: ["authorization", "x-goog-authuser", "x-youtube-client-version"],
  sign: "none", // SAPISIDHASH is computable from the cookie, not a page VM
  endpointHint: "youtubei/v1/* (InnerTube)",
  loginUrl: "https://accounts.google.com/ServiceLogin?service=youtube",
  capabilities: { post: "official", dm: "none", search: "official", browse: "replay" },
  // Channel handle needs a signed InnerTube call; confirm the session for now.
  whoami: async (ctx) => {
    const c = await ctx.cookie("SAPISID", "youtube.com")
    return c ? {} : {}
  },
}
