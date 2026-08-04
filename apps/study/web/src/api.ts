/** Thin fetch wrapper. Every error surfaces the server's own message — the
 *  backend spends effort explaining *why* something was refused (no TTS model,
 *  quote not verifiable, plan doesn't fit) and swallowing that would waste it. */
async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, {
    headers: init?.body instanceof FormData ? undefined : { 'Content-Type': 'application/json' },
    ...init,
  })
  const text = await res.text()
  let body: any = null
  try {
    body = text ? JSON.parse(text) : null
  } catch {
    body = text
  }
  if (!res.ok) throw new Error(body?.error ?? body ?? `HTTP ${res.status}`)
  return body as T
}

export const get = <T,>(p: string) => req<T>(p)
export const post = <T,>(p: string, body?: unknown) =>
  req<T>(p, { method: 'POST', body: JSON.stringify(body ?? {}) })
export const patch = <T,>(p: string, body?: unknown) =>
  req<T>(p, { method: 'PATCH', body: JSON.stringify(body ?? {}) })
export const del = <T,>(p: string) => req<T>(p, { method: 'DELETE' })
export const upload = <T,>(p: string, form: FormData) =>
  req<T>(p, { method: 'POST', body: form })

// ── Shapes ──────────────────────────────────────────────────────────────────

/** A short line the cleaner flagged but did not remove. */
export interface Suspect {
  line: string
  count: number
}

export interface Doc {
  id: string
  title: string
  filename: string
  ext: string
  chars: number
  extractNote?: string | null
  summary?: string | null
  status: string
  sectionCount: number
  chunkCount: number
  addedAt: number
  suspectedFurniture?: Suspect[]
}

export interface Section {
  id: string
  docId: string
  ord: number
  title: string
  level: number
  charStart: number
  charEnd: number
  summary?: string | null
  keyPoints: string[]
  difficulty: number
  estMinutes: number
  prereq: string[]
  enrichedAt?: number | null
}

export interface Template {
  key: string
  label: string
  detail?: string | null
  days: number
  minPerDay: number
  reviewOffsets: number[]
  blocks: string[]
  contentRatio: number
}

export interface PlanItem {
  kind: string
  sectionId?: string | null
  sectionTitle: string
  estMinutes: number
  part: number
  parts: number
}

export interface PlanSession {
  ord: number
  date: string
  startHm: string
  minutes: number
  title: string
  items: PlanItem[]
}

export interface Preview {
  feasible: boolean
  sessions: PlanSession[]
  totalEstMinutes: number
  contentBudgetMinutes: number
  budgetMinutes: number
  spanDays: number
  dropped: { sectionId: string; title: string; estMinutes: number }[]
  options: string[]
  notes: string[]
  warnings?: string[]
}

export interface Plan {
  id: string
  title: string
  goal?: string | null
  docIds: string[]
  templateKey?: string | null
  startDate: string
  days: number
  minPerDay: number
  slotHm: string
  status: string
  sessionCount: number
  doneCount: number
  syncedCount: number
}

export interface SessionItem extends PlanItem {
  id: string
  doneAt?: number | null
  text?: string
  summary?: string | null
  keyPoints?: string[]
  docId?: string
}

export interface Session {
  id: string
  planId?: string
  ord: number
  date: string
  startHm: string
  minutes: number
  title: string
  status: string
  eventId?: string | null
  items: SessionItem[]
}

export interface Card {
  id: string
  docId?: string | null
  sectionId?: string | null
  front: string
  back: string
  kind: string
  source: string
  level: number
  nextReview?: string | null
  isUrgent: boolean
  reviews: number
  lapses: number
}

export interface Question {
  id: string
  docId: string
  sectionId?: string | null
  kind: string
  stem: string
  options: any
  difficulty: number
}

export interface Evidence {
  id: string
  kind: string
  title: string
  text: string
  docId?: string | null
  sectionId?: string | null
  chunkId?: number | null
  charStart?: number | null
  charEnd?: number | null
  url?: string | null
  source?: string | null
}

export interface Answer {
  id: string
  question: string
  answerMd: string
  evidence: Evidence[]
  degraded: boolean
  notes: string[]
  external: boolean
  sourcesAvailable?: { key: string; server: string; tool: string; score: number }[]
  sourcesUsed?: string[]
}
