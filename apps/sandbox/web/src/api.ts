// Thin REST client. Every call goes through `req` so an error response body
// (`{"error": "..."}`) becomes a thrown Error carrying the server's own words —
// the alternative is a UI that shows "Failed to fetch" for a message that
// already explained exactly what to fix.

export type DirectKind = 'seatbelt' | 'bubblewrap' | 'appcontainer' | 'degraded' | 'unsupported'

export interface Caps {
  os: string
  arch: string
  direct: { available: boolean; kind: DirectKind; detail: string }
  docker: {
    cli: boolean
    available: boolean
    clientVersion?: string | null
    serverVersion?: string | null
    detail: string
  }
  hostInterpreters: string[]
  backends: string[]
  probedAtMs: number
}

export interface Sandbox {
  id: string
  name: string
  backend: string
  image: string | null
  workdir: string
  network: boolean
  cpus: number
  memoryMb: number
  timeoutMs: number
  status: string
  mounts: Mount[]
  fsMode: FsMode
  traceEnabled: boolean
  ports: PortPolicy
  lastError: string | null
  createdAt: number
  lastUsedAt: number | null
}

export interface Run {
  id: string
  sandboxId: string
  kind: string
  language: string | null
  source: string
  exitCode: number | null
  stdout: string
  stderr: string
  truncated: boolean
  timedOut: boolean
  isolation: string
  network: boolean
  durationMs: number
  createdAt: number
}

export type FsMode = 'strict' | 'allowlist' | 'open'

export interface AppSettings {
  defaultFsMode: FsMode
  allowlist: string[]
  defaultNetwork: boolean
  defaultMemoryMb: number
  defaultCpus: number
  defaultTimeoutMs: number
}

export interface TraceEvent {
  tsMs: number
  pid: number
  source: string
  kind: string
  target: string
  detail: string
}

export interface PortPolicy {
  listen: number[]
  connect: number[]
}

export interface Mount {
  source: string
  target: string
  readOnly: boolean
}

export interface Proc {
  pid: number
  ppid: number
  cpu: number
  memPercent: number
  rssMb: number
  elapsed: string
  command: string
}

export interface Stats {
  source: string
  processes: Proc[]
  cpu: number
  rssMb: number
  memoryLimitMb: number | null
  running: boolean
  note: string | null
}

export interface FileEntry {
  name: string
  path: string
  dir: boolean
  size: number
  modifiedMs: number
}

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  const text = await res.text()
  let body: unknown = null
  try {
    body = text ? JSON.parse(text) : null
  } catch {
    // Non-JSON body — keep the raw text, it is usually the useful part.
  }
  if (!res.ok) {
    const msg =
      (body as { error?: string } | null)?.error ?? text ?? `HTTP ${res.status}`
    throw new Error(msg)
  }
  return body as T
}

export const api = {
  caps: (refresh = false) => req<Caps>(`/caps?refresh=${refresh}`),
  status: () => req<{ caps: Caps; sandboxes: number; defaultImage: string }>('/status'),
  languages: () => req<{ languages: string[] }>('/languages'),

  listSandboxes: () => req<{ sandboxes: Sandbox[] }>('/sandboxes'),
  createSandbox: (body: Record<string, unknown>) =>
    req<Sandbox>('/sandboxes', { method: 'POST', body: JSON.stringify(body) }),
  getSandbox: (id: string) => req<Sandbox>(`/sandboxes/${id}`),
  updateSandbox: (id: string, body: Record<string, unknown>) =>
    req<{ sandbox: Sandbox; restarted: boolean }>(`/sandboxes/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(body),
    }),
  deleteSandbox: (id: string, purge: boolean) =>
    req<{ ok: boolean }>(`/sandboxes/${id}?purge=${purge}`, { method: 'DELETE' }),
  startSandbox: (id: string) => req<Sandbox>(`/sandboxes/${id}/start`, { method: 'POST' }),
  stopSandbox: (id: string) => req<{ ok: boolean }>(`/sandboxes/${id}/stop`, { method: 'POST' }),

  exec: (id: string, command: string, timeoutMs?: number) =>
    req<Run>(`/sandboxes/${id}/exec`, {
      method: 'POST',
      body: JSON.stringify({ command, timeoutMs }),
    }),
  runCode: (id: string, language: string, code: string, timeoutMs?: number) =>
    req<Run>(`/sandboxes/${id}/run`, {
      method: 'POST',
      body: JSON.stringify({ language, code, timeoutMs }),
    }),
  install: (id: string, manager: string, packages: string[]) =>
    req<Run>(`/sandboxes/${id}/install`, {
      method: 'POST',
      body: JSON.stringify({ manager, packages }),
    }),

  listFiles: (id: string, path: string) =>
    req<{ entries: FileEntry[] }>(`/sandboxes/${id}/files?path=${encodeURIComponent(path)}`),
  readFile: (id: string, path: string) =>
    req<{ content: string }>(`/sandboxes/${id}/file?path=${encodeURIComponent(path)}`),
  writeFile: (id: string, path: string, content: string) =>
    req<{ ok: boolean }>(`/sandboxes/${id}/file`, {
      method: 'PUT',
      body: JSON.stringify({ path, content }),
    }),
  deleteFile: (id: string, path: string) =>
    req<{ ok: boolean }>(`/sandboxes/${id}/file?path=${encodeURIComponent(path)}`, {
      method: 'DELETE',
    }),

  settings: () => req<AppSettings>('/settings'),
  saveSettings: (s: AppSettings) =>
    req<AppSettings>('/settings', { method: 'PUT', body: JSON.stringify(s) }),
  setFsMode: (id: string, fsMode: FsMode) =>
    req<Sandbox>(`/sandboxes/${id}/fs-mode`, {
      method: 'POST',
      body: JSON.stringify({ fsMode }),
    }),

  setPorts: (id: string, listen: number[], connect: number[]) =>
    req<{ sandbox: Sandbox; note: string | null }>(`/sandboxes/${id}/ports`, {
      method: 'POST',
      body: JSON.stringify({ listen, connect }),
    }),

  setTrace: (id: string, enabled: boolean) =>
    req<Sandbox>(`/sandboxes/${id}/trace`, {
      method: 'POST',
      body: JSON.stringify({ enabled }),
    }),
  events: (id: string, kind?: string, runId?: string, limit = 500) =>
    req<{ events: TraceEvent[] }>(
      `/sandboxes/${id}/events?limit=${limit}` +
        (kind ? `&kind=${kind}` : '') +
        (runId ? `&runId=${runId}` : ''),
    ),
  clearEvents: (id: string) =>
    req<{ ok: boolean }>(`/sandboxes/${id}/events`, { method: 'DELETE' }),

  stats: (id: string) => req<Stats>(`/sandboxes/${id}/stats`),
  kill: (id: string, pid?: number) =>
    req<{ ok: boolean }>(`/sandboxes/${id}/kill`, {
      method: 'POST',
      body: JSON.stringify({ pid: pid ?? null }),
    }),

  addMount: (id: string, source: string, target: string, readOnly: boolean) =>
    req<{ sandbox: Sandbox }>(`/sandboxes/${id}/mounts`, {
      method: 'POST',
      body: JSON.stringify({ source, target, readOnly }),
    }),
  removeMount: (id: string, target: string) =>
    req<{ sandbox: Sandbox }>(`/sandboxes/${id}/mounts/remove`, {
      method: 'POST',
      body: JSON.stringify({ target }),
    }),

  runs: (sandboxId?: string, limit = 50) =>
    req<{ runs: Run[] }>(
      `/runs?limit=${limit}${sandboxId ? `&sandboxId=${sandboxId}` : ''}`,
    ),
}

/// Labels live in the dictionary, so both helpers take it rather than owning
/// copies of the wording.
export function fsModeLabel(
  m: FsMode,
  t: {
    fsStrictTitle: string; fsStrictTag: string; fsStrictBody: string
    fsAllowlistTitle: string; fsAllowlistTag: string; fsAllowlistBody: string
    fsOpenTitle: string; fsOpenTag: string; fsOpenBody: string
  },
): { title: string; tag: string; color: string; detail: string } {
  switch (m) {
    case 'strict':
      return { title: t.fsStrictTitle, tag: t.fsStrictTag, color: 'green', detail: t.fsStrictBody }
    case 'allowlist':
      return {
        title: t.fsAllowlistTitle,
        tag: t.fsAllowlistTag,
        color: 'blue',
        detail: t.fsAllowlistBody,
      }
    case 'open':
      return { title: t.fsOpenTitle, tag: t.fsOpenTag, color: 'orange', detail: t.fsOpenBody }
  }
}

/// What actually confined a run, named for the reader.
export function isolationLabel(
  iso: string,
  t: {
    isoSeatbelt: string; isoBubblewrap: string; isoContainer: string
    isoAppContainer: string; isoNone: string
  },
): { text: string; color: string } {
  switch (iso) {
    case 'seatbelt':
      return { text: t.isoSeatbelt, color: 'green' }
    case 'bubblewrap':
      return { text: t.isoBubblewrap, color: 'green' }
    case 'appcontainer':
      return { text: t.isoAppContainer, color: 'green' }
    case 'container':
      return { text: t.isoContainer, color: 'blue' }
    case 'degraded':
      return { text: t.isoNone, color: 'red' }
    default:
      return { text: iso, color: 'default' }
  }
}
