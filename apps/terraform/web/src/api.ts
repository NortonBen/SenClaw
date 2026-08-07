// REST client cho backend app Terraform (cùng origin, /api/*).

export interface Workspace {
  id: number
  name: string
  source: 'folder' | 'git'
  dir: string
  repo_url: string
  branch: string
  var_file: string
  auto_sync: boolean
  status: 'ready' | 'cloning' | 'error'
  last_error: string
  created_at: number
  updated_at: number
  /** Root Terraform trong repo ('' = gốc). */
  subdir: string
}

export interface Run {
  id: number
  workspace_id: number | null
  kind: string
  status: 'running' | 'success' | 'failed' | 'canceled'
  exit_code: number | null
  started_at: number
  finished_at: number | null
}

export interface RunLine {
  seq: number
  stream: 'out' | 'err' | 'sys'
  line: string
  at: number
}

export interface VarDef {
  name: string
  var_type: string
  description: string
  default: unknown | null
  sensitive: boolean
  file: string
}

export interface CliInfo {
  found: boolean
  path?: string
  version?: string | null
  source?: string
  platform?: string | null
  managed_dir?: string
}

export interface WsDetail {
  ok: boolean
  workspace: Workspace
  git: { is_git: boolean; branch?: string; commit?: string; dirty_files?: number; remote?: string }
  work_dir: string
  work_dir_exists: boolean
  tfvars_files: string[]
  running_run: number | null
  last_run: Run | null
  initialized: boolean
}

export interface FsEntry {
  name: string
  path: string
  has_tf: boolean
}

async function req<T>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  const body = (await r.json()) as T & { ok?: boolean; error?: string }
  if (body && body.ok === false) throw new Error(body.error || 'lỗi không rõ')
  return body
}

export const api = {
  status: () => req<{ workspaces: number; running: number }>('/api/status'),
  cli: () => req<CliInfo>('/api/cli'),
  cliInstall: (version?: string) =>
    req<{ run_id: number }>('/api/cli/install', { method: 'POST', body: JSON.stringify({ version }) }),
  fs: (path?: string) =>
    req<{ path: string; parent: string | null; home: string; has_tf: boolean; entries: FsEntry[] }>(
      '/api/fs' + (path ? `?path=${encodeURIComponent(path)}` : ''),
    ),
  workspaces: () => req<{ workspaces: Workspace[] }>('/api/workspaces'),
  wsAdd: (body: {
    source: 'folder' | 'git'
    name?: string
    path?: string
    repo_url?: string
    branch?: string
    subdir?: string
  }) => req<{ workspace: Workspace; run_id?: number }>('/api/workspaces', { method: 'POST', body: JSON.stringify(body) }),
  wsGet: (id: number) => req<WsDetail>(`/api/workspaces/${id}`),
  wsPatch: (id: number, body: { name?: string; var_file?: string; auto_sync?: boolean; subdir?: string }) =>
    req<{ workspace: Workspace }>(`/api/workspaces/${id}`, { method: 'POST', body: JSON.stringify(body) }),
  subdirs: (id: number) =>
    req<{ root_has_tf: boolean; subdir: string; subdirs: string[] }>(`/api/workspaces/${id}/subdirs`),
  tfvarsFiles: (id: number) =>
    req<{ files: { rel: string; display: string; in_work_dir: boolean }[]; current: string }>(
      `/api/workspaces/${id}/tfvars-files`,
    ),
  openDir: (id: number) => req<{ dir: string }>(`/api/workspaces/${id}/open-dir`, { method: 'POST', body: '{}' }),
  wsDelete: (id: number) =>
    req<{ ok: boolean }>(`/api/workspaces/${id}/delete`, { method: 'POST', body: JSON.stringify({ confirm: true }) }),
  wsSync: (id: number) => req<{ run_id: number }>(`/api/workspaces/${id}/sync`, { method: 'POST', body: '{}' }),
  variables: (id: number) =>
    req<{ variables: VarDef[]; parse_errors: string[]; tfvars_files: string[]; var_file: string }>(
      `/api/workspaces/${id}/variables`,
    ),
  tfvarsGet: (id: number, file?: string) =>
    req<{ file: string | null; exists: boolean; values: Record<string, unknown> }>(
      `/api/workspaces/${id}/tfvars` + (file ? `?file=${encodeURIComponent(file)}` : ''),
    ),
  tfvarsSet: (id: number, file: string, values: Record<string, unknown>) =>
    req<{ saved: number; values: Record<string, unknown> }>(`/api/workspaces/${id}/tfvars`, {
      method: 'POST',
      body: JSON.stringify({ file, values }),
    }),
  run: (id: number, command: string, opts?: { var_file?: string; confirm?: boolean }) =>
    req<{ run_id: number; steps: string[] }>(`/api/workspaces/${id}/run`, {
      method: 'POST',
      body: JSON.stringify({ command, ...opts }),
    }),
  runs: (workspaceId?: number) =>
    req<{ runs: Run[] }>('/api/runs' + (workspaceId ? `?workspace_id=${workspaceId}` : '')),
  runGet: (id: number, after: number) =>
    req<{ run: Run; lines: RunLine[]; next_after: number }>(`/api/runs/${id}?after=${after}`),
  runCancel: (id: number) => req<{ ok: boolean }>(`/api/runs/${id}/cancel`, { method: 'POST', body: '{}' }),
  explain: (id: number) => req<{ text: string }>(`/api/runs/${id}/explain`, { method: 'POST', body: '{}' }),
}

export const KIND_LABEL: Record<string, string> = {
  init: 'terraform init',
  validate: 'terraform validate',
  plan: 'terraform plan',
  apply: 'terraform apply',
  destroy: 'terraform destroy',
  output: 'terraform output',
  sync: 'git pull',
  clone: 'git clone',
  install: 'cài Terraform CLI',
}

export function fmtTime(epoch: number | null): string {
  if (!epoch) return '—'
  return new Date(epoch * 1000).toLocaleString('vi-VN', { hour12: false })
}
