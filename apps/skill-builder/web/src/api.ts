// Typed client for the Skill Builder backend.

export interface SkillInfo {
  name: string
  description?: string
  triggers?: string[]
  source?: string
  disabled?: boolean
}
export interface SubagentInfo {
  name: string
  description?: string
}
export interface McpToolInfo {
  name: string
  description?: string
}
export interface McpServerInfo {
  name: string
  description?: string
  transport?: string
  tools?: (string | McpToolInfo)[]
}
export interface Inventory {
  skills: SkillInfo[]
  subagents: SubagentInfo[]
  mcpServers: McpServerInfo[]
}

export interface DraftSkill {
  name: string
  description: string
  triggers: string[]
  content: string
  uses_mcp: string[]
  uses_subagents: string[]
  rationale: string
  model?: string
}

async function j<T>(res: Response): Promise<T> {
  const body = await res.json().catch(() => ({}))
  if (!res.ok) throw new Error((body as any)?.error || `HTTP ${res.status}`)
  return body as T
}

export const api = {
  inventory: () => fetch('/api/inventory').then(j<Inventory>),
  skills: () => fetch('/api/skills').then(j<{ skills: SkillInfo[] }>),
  generate: (requirement: string, when_to_run: string) =>
    fetch('/api/generate', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ requirement, when_to_run }),
    }).then(j<DraftSkill>),
  install: (d: {
    name: string
    description: string
    content: string
    triggers: string[]
    overwrite?: boolean
  }) =>
    fetch('/api/install', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(d),
    }).then(j<{ ok: boolean; name: string }>),
  remove: (name: string) =>
    fetch(`/api/skills/${encodeURIComponent(name)}`, { method: 'DELETE' }).then(
      j<{ ok: boolean }>,
    ),
}
