// Threads adapter. Shares the Instagram login/session.
import type { Adapter } from './types'

export const threads: Adapter = {
  id: "threads",
  hosts: ["threads.net", "threads.com"],
  sessionCookie: "sessionid",
  captureHeaders: ["x-ig-app-id"], // Threads app-id 238260118697367
  sign: "meta",
  endpointHint: "web read-only; write via official Threads API (Rust)",
  loginUrl: "https://www.threads.net/login",
  capabilities: { post: "official", dm: "none", search: "official", browse: "replay" },
  // Threads rides the Instagram session; the ds_user_id lives on the IG cookie.
  whoami: async (ctx) => {
    const c = await ctx.cookie("ds_user_id", "instagram.com")
    return c ? { handle: c, id: c } : {}
  },
}
