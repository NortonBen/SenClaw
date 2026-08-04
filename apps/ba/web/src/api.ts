/** Thin fetch wrapper. Every error surfaces the server's own message — the
 * backend spends effort explaining *why* something was refused (template scope,
 * workflow state, interview mode...) and swallowing that would waste it. */

export async function req<T = any>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  let body: any = null
  try {
    body = await res.json()
  } catch {
    throw new Error(`HTTP ${res.status} — server không trả JSON`)
  }
  if (body && typeof body === 'object' && body.error) throw new Error(body.error)
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return body as T
}

export const get = <T = any>(path: string) => req<T>(path)
export const post = <T = any>(path: string, body: any) =>
  req<T>(path, { method: 'POST', body: JSON.stringify(body) })
export const patch = <T = any>(path: string, body: any) =>
  req<T>(path, { method: 'PATCH', body: JSON.stringify(body) })
export const del = <T = any>(path: string) => req<T>(path, { method: 'DELETE' })

/** Poll một job LLM tới khi xong. Job kết quả {error} sẽ throw để caller hiện message. */
export async function waitJob(jobId: number, onTick?: (ms: number) => void): Promise<any> {
  const started = Date.now()
  for (;;) {
    await new Promise((r) => setTimeout(r, 1300))
    onTick?.(Date.now() - started)
    const j = await get(`/jobs/${jobId}`)
    if (j.status === 'running') continue
    const result = j.result ?? {}
    if (result.error) throw new Error(result.error)
    return result
  }
}

export type Doc = {
  id: number
  project_id: number
  feature_id: number | null
  doc_type: string
  subtype: string
  title: string
  content?: string
  format: string
  status: string
  version: number
  source: string
  confidence: string
  updated_at: number
  chars: number
}

export type CatalogItem = {
  doc_type: string
  subtype: string
  skill: string
  title: string
  desc: string
  scope: 'project' | 'feature'
  format: string
  has_interview: boolean
}

export type Phase = { phase: number; name: string; items: CatalogItem[] }

export const fmtTime = (ms: number) => {
  if (!ms) return ''
  const d = new Date(ms)
  return `${d.toLocaleDateString('vi-VN')} ${d.toLocaleTimeString('vi-VN', { hour: '2-digit', minute: '2-digit' })}`
}

export const STATUS_COLOR: Record<string, string> = {
  draft: 'gold',
  in_review: 'blue',
  revisions: 'red',
  approved: 'green',
  shipped: 'purple',
}
export const STATUS_LABEL: Record<string, string> = {
  draft: 'Draft',
  in_review: 'In review',
  revisions: 'Revisions',
  approved: 'Approved',
  shipped: 'Shipped',
}
