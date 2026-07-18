export type Bot = {
  key: string
  name: string
  system_prompt: string
  greeting: string
  model: string
  knowledge_scope: string
  allowed_mcp: string[]
  allowed_skills: string[]
  use_tools: boolean
  use_knowledge: boolean
  auto_ingest: boolean
  auto_issue: boolean
  enabled: boolean
  created_at: number
}

export type Issue = {
  id: number
  session_id: number | null
  bot_key: string
  external_id: string
  title: string
  description: string
  status: string
  priority: string
  category: string
  sentiment: string
  ai_summary: string
  tags: string[]
  resolution_note: string
  assignee: string
  created_at: number
  updated_at: number
  resolved_at: number | null
}

export type Analytics = {
  issues: { total: number; open: number; byStatus: Record<string, number>; byPriority: Record<string, number>; byCategory: Record<string, number>; bySentiment: Record<string, number> }
  sessions: { total: number; openHandoffs: number; byChannel: Record<string, number> }
  llmCalls: number
  tokensIn: number
  tokensOut: number
}

export type SessionAnalysis = {
  sentiment?: string
  quality?: number
  resolved?: boolean
  summary?: string
  category?: string
  suggestions?: string
  raw?: boolean
}

export type Channel = {
  id: number
  botKey: string
  kind: string
  name: string
  config: Record<string, unknown>
  enabled: boolean
  lastSyncAt: number | null
  lastStatus: string
  lastError: string
}

export type CrmProfile = {
  id: number
  name: string
  company?: string
  title?: string
  role?: string
  phone?: string
  email?: string
  tags?: string[]
  notes?: string
  url?: string
  none?: boolean
}

export type Session = {
  id: number
  bot_key: string
  channel_kind: string
  external_id: string
  customer_name: string
  handoff_state: string
  last_activity: number
  context?: { crm?: CrmProfile } & Record<string, unknown>
}

export type Msg = { id: number; role: string; content: string; created_at: number }

export type Conversation = {
  id: number
  externalId: string
  customerName: string
  lastActivity: number
  channelKind: string
  messageCount: number
  preview: string
}

export type CrmCustomer = {
  id: number
  name: string
  company?: string
  phone?: string
  /** Whether we can reach them on the channel passed to crmSearch. */
  reachable?: boolean
  /** Their id on that channel (when known). */
  channelValue?: string | null
}

export type ToolItem = { name: string; description?: string }
export type McpServer = { name: string; description: string; builtin: boolean; tools: ToolItem[] }
export type Inventory = { core: ToolItem[]; servers: McpServer[] }
export type SkillInventory = { skills: ToolItem[]; personas: ToolItem[] }
export type Stats = Record<string, number | string>

async function req<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(`/api${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...opts,
  })
  const text = await res.text()
  const data = text ? JSON.parse(text) : {}
  if (!res.ok) throw new Error(data.error || res.statusText)
  return data as T
}

export const api = {
  status: () => req<{ ok: boolean }>('/status'),
  llmInfo: () => req<{ available: boolean; config?: Record<string, unknown> }>('/llm-info'),
  stats: () => req<Stats>('/stats'),

  listBots: () => req<{ bots: Bot[] }>('/bots').then((r) => r.bots),
  createBot: (b: { name: string; system_prompt?: string; greeting?: string }) =>
    req<{ bot: Bot }>('/bots', { method: 'POST', body: JSON.stringify(b) }).then((r) => r.bot),
  updateBot: (key: string, patch: Partial<Bot>) =>
    req<{ bot: Bot }>(`/bots/${key}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteBot: (key: string) => req(`/bots/${key}`, { method: 'DELETE' }),
  botKnowledge: (key: string) => req<{ space: string; count: number }>(`/bots/${key}/knowledge`),

  listChannels: (bot?: string) =>
    req<{ channels: Channel[] }>(`/channels${bot ? `?bot=${bot}` : ''}`).then((r) => r.channels),
  createChannel: (b: { botKey: string; kind: string; name: string; config: Record<string, unknown> }) =>
    req<{ channel: Channel }>('/channels', { method: 'POST', body: JSON.stringify(b) }),
  updateChannel: (id: number, patch: { name?: string; config?: Record<string, unknown>; enabled?: boolean }) =>
    req<{ channel: Channel }>(`/channels/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteChannel: (id: number) => req(`/channels/${id}`, { method: 'DELETE' }),
  testChannel: (id: number) => req<{ ok: boolean; message: string }>(`/channels/${id}/test`, { method: 'POST' }),

  listSessions: (bot?: string) =>
    req<{ sessions: Session[] }>(`/sessions${bot ? `?bot=${bot}` : ''}`).then((r) => r.sessions),
  getSession: (id: number) => req<{ session: Session; messages: Msg[] }>(`/sessions/${id}`),
  deleteSession: (id: number) => req(`/sessions/${id}`, { method: 'DELETE' }),
  listConversations: (bot: string, kind = 'websocket') =>
    req<{ conversations: Conversation[] }>(`/conversations?bot=${bot}&kind=${kind}`).then((r) => r.conversations),
  createConversation: (b: { bot: string; channelId: number; crmCustomerId?: number; externalId?: string; name?: string }) =>
    req<{ sessionId: number; externalId: string; channelKind: string; customerName: string }>('/conversations', {
      method: 'POST',
      body: JSON.stringify(b),
    }),
  conversationSend: (id: number, text: string) =>
    req(`/conversations/${id}/send`, { method: 'POST', body: JSON.stringify({ text }) }),
  chat: (b: { bot: string; externalId?: string; text: string }) =>
    req<{ sessionId: number; externalId: string; reply: string | null; escalated: boolean }>('/chat', {
      method: 'POST',
      body: JSON.stringify(b),
    }),
  setHandoff: (id: number, state: string) =>
    req(`/handoff/${id}`, { method: 'POST', body: JSON.stringify({ state }) }),
  handoffReply: (id: number, text: string) =>
    req(`/handoff/${id}/reply`, { method: 'POST', body: JSON.stringify({ text }) }),
  analyzeSession: (id: number) => req<SessionAnalysis>(`/sessions/${id}/analyze`, { method: 'POST' }),

  listIssues: (q: { status?: string; priority?: string; bot?: string; search?: string } = {}) => {
    const params = new URLSearchParams(Object.entries(q).filter(([, v]) => v) as [string, string][])
    return req<{ issues: Issue[] }>(`/issues?${params}`).then((r) => r.issues)
  },
  createIssue: (b: { botKey?: string; sessionId?: number; title: string; description?: string; priority?: string; category?: string }) =>
    req<{ issue: Issue }>('/issues', { method: 'POST', body: JSON.stringify(b) }).then((r) => r.issue),
  getIssue: (id: number) => req<{ issue: Issue; events: unknown[] }>(`/issues/${id}`),
  updateIssue: (id: number, patch: Partial<Pick<Issue, 'status' | 'priority' | 'category' | 'assignee' | 'resolution_note'>>) =>
    req<{ issue: Issue }>(`/issues/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  analytics: () => req<Analytics>('/analytics'),
  crmSearch: (q: string, channel?: string) =>
    req<{ customers: CrmCustomer[] }>(
      `/crm/search?q=${encodeURIComponent(q)}${channel ? `&channel=${encodeURIComponent(channel)}` : ''}`,
    ).then((r) => r.customers),

  writeKnowledge: (botKey: string, text: string, wiki: boolean) =>
    req<{ ok: boolean; space: string }>('/knowledge', {
      method: 'POST',
      body: JSON.stringify({ botKey, text, wiki }),
    }),
  uploadKnowledge: async (botKey: string, file: File) => {
    const fd = new FormData()
    fd.append('bot', botKey)
    fd.append('file', file)
    const res = await fetch('/api/knowledge/upload', { method: 'POST', body: fd })
    const data = await res.json()
    if (!res.ok) throw new Error(data.error || res.statusText)
    return data as { ok: boolean; filename: string }
  },
  searchKnowledge: (bot: string, q: string) =>
    req<{ hits: Array<{ name: string; summary: string; score: number }> }>(
      `/knowledge?bot=${bot}&q=${encodeURIComponent(q)}`,
    ),

  mcpInventory: () => req<Inventory>('/mcp-inventory'),
  skillsInventory: () => req<SkillInventory>('/skills-inventory'),
  getSettings: () => req<{ features: Record<string, boolean>; language: string; crmEnabled: boolean; crmBase: string }>('/settings'),
  updateSettings: (b: { features?: Record<string, boolean>; language?: string; crmEnabled?: boolean; crmBase?: string }) =>
    req<{ features: Record<string, boolean>; language: string; crmEnabled: boolean; crmBase: string }>('/settings', {
      method: 'POST',
      body: JSON.stringify(b),
    }),
}
