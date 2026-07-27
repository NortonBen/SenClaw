// Adapter registry. Every `Platform` in the Rust side must have a matching
// entry here (guarded by `every_platform_has_an_extension_adapter`).

import type { Adapter } from './types'
import { x } from './x'
import { facebook } from './facebook'
import { instagram } from './instagram'
import { threads } from './threads'
import { tiktok } from './tiktok'
import { youtube } from './youtube'

export const ADAPTERS: Adapter[] = [x, facebook, instagram, threads, tiktok, youtube]

export * from './types'
export { adapterForHost, adapterById, credentialedFetch } from './base'
