// Thin REST client. Every path is resolved against the directory the SPA was
// served from so the same bundle works at `/` and behind the SenClaw proxy
// path `/api/space/apps/rule-engine/proxy/`.

import type {
  Chain,
  ChainEdge,
  ChainNode,
  EngineStatus,
  HopRow,
  Issue,
  LogRow,
  RunRow,
  RuleSpec,
} from './types'

/** Directory part of the current URL, always ending in `/`. Captured once. */
export const BASE_PATH: string = (() => {
  const p = window.location.pathname
  return p.endsWith('/') ? p : p.slice(0, p.lastIndexOf('/') + 1)
})()

export const apiUrl = (path: string) => `${BASE_PATH}api/${path.replace(/^\/+/, '')}`

interface Envelope {
  ok?: boolean
  error?: string
}

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response
  try {
    res = await fetch(apiUrl(path), init)
  } catch {
    throw new Error('Không kết nối được tới máy chủ rule-engine.')
  }
  let body: (Envelope & Record<string, unknown>) | null = null
  try {
    body = (await res.json()) as Envelope & Record<string, unknown>
  } catch {
    body = null
  }
  if (!res.ok || (body && body.ok === false)) {
    throw new Error(body?.error || `HTTP ${res.status}`)
  }
  return (body ?? {}) as T
}

const send = (path: string, method: string, body?: unknown) =>
  req<Record<string, unknown>>(path, {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })

/** What `PUT /api/chains/:id/graph` accepts for a node. */
export interface GraphNodeDto {
  id: string
  rule: string
  name: string
  config: Record<string, unknown>
  opts: ChainNode['opts']
  x: number
  y: number
  debug: boolean
}

export const api = {
  status: () => req<EngineStatus>('status'),

  registry: () => req<{ rules: RuleSpec[] }>('registry').then((r) => r.rules ?? []),

  listChains: () => req<{ chains: Chain[] }>('chains').then((r) => r.chains ?? []),

  createChain: (name: string, description: string) =>
    send('chains', 'POST', { name, description }).then((r) => r.chain as Chain),

  getChain: (id: number) =>
    req<{
      chain: Chain
      nodes: ChainNode[]
      edges: ChainEdge[]
      issues: Issue[]
      deployed: boolean
    }>(`chains/${id}`),

  patchChain: (id: number, patch: { name?: string; description?: string; debug?: boolean }) =>
    send(`chains/${id}`, 'PATCH', patch),

  putGraph: (id: number, nodes: GraphNodeDto[], edges: ChainEdge[]) =>
    send(`chains/${id}/graph`, 'PUT', { nodes, edges }) as Promise<{
      ok: boolean
      issues: Issue[]
      redeployed: boolean
    }>,

  validate: (id: number) =>
    send(`chains/${id}/validate`, 'POST') as Promise<{ ok: boolean; issues: Issue[] }>,

  activate: (id: number) =>
    send(`chains/${id}/activate`, 'POST') as Promise<{ ok: boolean; issues: Issue[] }>,

  deactivate: (id: number) => send(`chains/${id}/deactivate`, 'POST'),

  deleteChain: (id: number) => send(`chains/${id}`, 'DELETE'),

  trigger: (id: number, body: { node?: string; port?: string; data?: unknown; meta?: unknown }) =>
    send(`chains/${id}/trigger`, 'POST', body).then((r) => r.runId as number),

  runs: (id: number, limit = 50) =>
    req<{ runs: RunRow[] }>(`chains/${id}/runs?limit=${limit}`).then((r) => r.runs ?? []),

  logs: (id: number, limit = 200) =>
    req<{ logs: LogRow[] }>(`chains/${id}/logs?limit=${limit}`).then((r) => r.logs ?? []),

  hops: (runId: number) =>
    req<{ hops: HopRow[] }>(`runs/${runId}/hops`).then((r) => r.hops ?? []),

  clearState: (id: number) => send(`chains/${id}/state`, 'DELETE'),
}
