// Registry helpers + the generic credentialed-fetch primitive.

import type { Adapter, FetchReq, FetchResult } from './types'

/** Resolve the adapter that owns a hostname. */
export function adapterForHost(list: Adapter[], host: string): Adapter | null {
  return list.find((a) => a.hosts.some((d) => host.endsWith(d))) ?? null
}

export function adapterById(list: Adapter[], id: string): Adapter | null {
  return list.find((a) => a.id === id) ?? null
}

/**
 * Shared credentialed fetch — the generic 'replay' primitive. Runs as the
 * logged-in user (cookies included). Text over ~20KB is truncated.
 */
export async function credentialedFetch(req: FetchReq): Promise<FetchResult> {
  const resp = await fetch(req.url, {
    method: req.method || 'GET',
    headers: (req.headers as Record<string, string>) || {},
    body: req.body ? (typeof req.body === 'string' ? req.body : JSON.stringify(req.body)) : undefined,
    credentials: 'include',
  })
  const text = await resp.text()
  let json: unknown = null
  try {
    json = JSON.parse(text)
  } catch {
    /* not JSON */
  }
  return { status: resp.status, json, text: json ? undefined : text.slice(0, 20000), url: resp.url }
}
