// Thin REST client. Every call goes through `req` so an error response body
// (`{"error": "..."}`) becomes a thrown Error carrying the server's own words —
// the alternative is a UI that shows "Failed to fetch" for a message that
// already explained exactly what to fix.

export type DirectKind = 'seatbelt' | 'bubblewrap' | 'degraded' | 'unsupported'

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

/// How each read-isolation mode is described, everywhere it appears.
export function fsModeLabel(m: FsMode): {
  title: string
  tag: string
  color: string
  detail: string
} {
  switch (m) {
    case 'strict':
      return {
        title: 'Cách ly toàn bộ',
        tag: 'an toàn nhất',
        color: 'green',
        detail:
          'Chỉ thấy thư mục sandbox và các thư mục bạn gắn vào. Phần còn lại của đĩa không đọc được. (Thư viện hệ thống vẫn đọc được — không có chúng thì Python không chạy nổi.)',
      }
    case 'allowlist':
      return {
        title: 'Cách ly + danh sách cho phép',
        tag: 'vừa phải',
        color: 'blue',
        detail:
          'Như trên, cộng thêm các thư mục bạn khai sẵn trong Cài đặt mặc định — khỏi phải gắn lại từng lần.',
      }
    case 'open':
      return {
        title: 'Không cách ly đọc',
        tag: 'rộng nhất',
        color: 'orange',
        detail:
          'Đọc được cả đĩa (trừ ~/.ssh, ~/.aws, Keychain và dữ liệu SenClaw). Vẫn không ghi được ra ngoài sandbox.',
      }
  }
}

/// Vietnamese label for the isolation actually applied to a run.
export function isolationLabel(iso: string): { text: string; color: string } {
  switch (iso) {
    case 'seatbelt':
      return { text: 'macOS Seatbelt', color: 'green' }
    case 'bubblewrap':
      return { text: 'Linux bubblewrap', color: 'green' }
    case 'container':
      return { text: 'Docker container', color: 'blue' }
    case 'degraded':
      return { text: 'KHÔNG cách ly', color: 'red' }
    default:
      return { text: iso, color: 'default' }
  }
}
