// Typed fetch client for the CRM backend (/api/*).

export interface Customer {
  id: number
  name: string
  email: string
  phone: string
  company: string
  title: string
  avatar_url: string
  notes: string
  tags: string[]
  role: string
  source: string
  address: string
  birthday: string
  created_at: number
  updated_at: number
  interaction_count: number
  last_interaction_at: number | null
}

export interface Relationship {
  id: number
  from_id: number
  from_name: string
  to_id: number
  to_name: string
  kind: string
  note: string
  confidence: number
  source: string
  created_at: number
}

export interface GraphNode {
  id: number
  name: string
  role: string
  company: string
  avatar_url: string
  interaction_count: number
}

export interface SearchHit {
  entity_type: 'customer' | 'interaction' | 'mention'
  entity_id: number
  customer_id: number | null
  customer_name: string | null
  title: string
  snippet: string
}

export interface CustomerChannel {
  id: number
  customer_id: number
  kind: string
  value: string
  label: string
  created_at: number
  updated_at: number
}

export interface Mention {
  id: number
  source_customer_id: number
  source_customer_name: string
  name: string
  role_guess: string
  kind_guess: string
  context: string
  confidence: number
  resolved_customer_id: number | null
  created_at: number
}

export interface Interaction {
  id: number
  customer_id: number
  kind: string
  summary: string
  details: string
  occurred_at: number
  created_at: number
}

export interface Stats {
  customers: number
  interactions: number
  open_tasks: number
  overdue_tasks: number
  open_deals: number
  pipeline_value: number
  won_value: number
  by_role: Record<string, number>
  by_stage: Record<string, { count: number; value: number }>
}

export interface Deal {
  id: number
  customer_id: number
  customer_name: string
  title: string
  amount: number
  currency: string
  stage: string
  probability: number
  expected_close_at: number | null
  closed_at: number | null
  notes: string
  /// The account the deal is booked at. 0 = unlinked.
  organization_id: number
  organization_name: string
  /// Delivery window — the reference CRM's "Project Period".
  period_start: number | null
  period_end: number | null
  created_at: number
  updated_at: number
}

export interface Task {
  id: number
  customer_id: number | null
  customer_name: string | null
  title: string
  details: string
  due_at: number | null
  done: boolean
  done_at: number | null
  created_at: number
  updated_at: number
}

export interface ActivityItem {
  id: number
  customer_id: number
  customer_name: string
  kind: string
  summary: string
  details: string
  occurred_at: number
}

export interface Upcoming {
  now: number
  window_days: number
  tasks: Array<{ id: number; title: string; due_at: number; customer_id: number | null; customer_name: string | null }>
  birthdays: Array<{ customer_id: number; customer_name: string; birthday: string; next_at: number }>
}

export interface CustomerDetail {
  customer: Customer
  interactions: Interaction[]
}

export type CustomerInput = Partial<Omit<Customer, 'id' | 'created_at' | 'updated_at' | 'interaction_count' | 'last_interaction_at'>> & {
  name?: string
}

// ---------- organizations (db_org.rs::Organization) ----------

export interface Organization {
  id: number
  name: string
  kind: string
  website: string
  domain: string
  industry: string
  size: string
  address: string
  logo_url: string
  notes: string
  tags: string[]
  created_at: number
  updated_at: number
  contact_count: number
  deal_count: number
  open_deal_value: number
}

export type OrganizationInput = Partial<
  Omit<Organization, 'id' | 'created_at' | 'updated_at' | 'contact_count' | 'deal_count' | 'open_deal_value'>
> & { name?: string }

/// A person's membership of an org, from the person's side (db_org.rs::OrgMembership).
export interface OrgMembership {
  organization_id: number
  name: string
  kind: string
  logo_url: string
  role_title: string
  is_primary: boolean
}

/// A contact of an org, from the org's side (db_org.rs::OrgContact).
export interface OrgContact {
  customer_id: number
  name: string
  email: string
  avatar_url: string
  role: string
  role_title: string
  is_primary: boolean
}

export interface OrgDetail {
  organization: Organization
  contacts: OrgContact[]
  deals: Deal[]
}

// ---------- services (db_org.rs::Service) ----------

export interface Service {
  id: number
  name: string
  kind: string
  amount: number
  currency: string
  pricing_model: string
  unit: string
  sku: string
  description: string
  active: boolean
  created_at: number
  updated_at: number
  deal_count: number
}

export type ServiceInput = Partial<
  Omit<Service, 'id' | 'created_at' | 'updated_at' | 'deal_count'>
> & { name?: string }

/// One catalogue entry attached to one deal (db_org.rs::DealService).
export interface DealService {
  id: number
  deal_id: number
  service_id: number
  name: string
  kind: string
  pricing_model: string
  currency: string
  quantity: number
  unit_amount: number
  line_total: number
  note: string
  created_at: number
}

export interface DealServices {
  services: DealService[]
  quantity: number
  total: number
}

// ---------- inbox (db_inbox.rs) ----------

/// OUR connected account. Note `config` secrets arrive masked as `••••••`, and
/// sending that value back means "unchanged" (db_inbox.rs::merge_config).
export interface InboxChannel {
  id: number
  kind: string
  name: string
  config: Record<string, unknown>
  enabled: boolean
  last_sync_at: number | null
  last_status: string
  last_error: string
  created_at: number
}

export interface InboxChannelInput {
  kind: string
  name?: string
  config?: Record<string, unknown>
}

export interface InboxChannelPatch {
  name?: string
  config?: Record<string, unknown>
  enabled?: boolean
}

export interface Conversation {
  id: number
  channel_id: number
  channel_kind: string
  external_id: string
  /// 0 means the thread is not linked to any contact yet.
  customer_id: number
  customer_name: string
  customer_avatar: string
  display_name: string
  status: string
  handoff_state: string
  assignee: string
  unread: number
  last_message_at: number | null
  created_at: number
  preview: string
  message_count: number
}

export interface ConvMessage {
  id: number
  conversation_id: number
  customer_id: number
  direction: string
  role: string
  content: string
  channel: string
  status: string
  created_at: number
}

export interface ConversationDetail {
  conversation: Conversation
  messages: ConvMessage[]
  customer: Customer | null
}

/// db_inbox.rs::inbox_stats — hand-built camelCase, unlike the serde structs.
export interface InboxStats {
  openConversations: number
  unread: number
  waitingOnHuman: number
  unlinked: number
  connectedChannels: number
}

// ---------- sale (db_sale.rs) ----------

export interface SaleState {
  customer_id: number
  name: string
  sale_stage: string
  temperature: string
  lead_score: number
  intent_signals: string[]
  unsubscribed: boolean
  unsubscribed_at: number | null
  last_inbound_at: number | null
  last_interaction_at: number | null
  checkin_count: number
  last_checkin_at: number | null
  owner: string
  source: string
}

export interface Review {
  id: number
  customer_id: number
  customer_name: string
  draft: string
  channel: string
  subject: string
  risk_reason: string
  status: string
  edited: string
  approved_by: string
  approved_at: number | null
  created_at: number
}

export interface Escalation {
  id: number
  customer_id: number
  customer_name: string
  reason: string
  context: string
  draft: string
  status: string
  resolved_by: string
  resolved_at: number | null
  created_at: number
}

export interface SaleAction {
  id: number
  customer_id: number | null
  action_type: string
  reasoning: string
  tool_calls: string
  tokens: number
  cost: number
  needs_review: boolean
  created_at: number
}

export interface Sequence {
  key: string
  name: string
  description: string
  steps: unknown
  enabled: boolean
  created_at: number
}

export interface SequenceRun {
  id: number
  customer_id: number
  sequence_key: string
  current_step: number
  status: string
  started_at: number
  completed_at: number | null
  last_sent_at: number | null
}

export interface SaleJob {
  id: number
  customer_id: number
  job_type: string
  run_at: number
  payload: string
  status: string
  executed_at: number | null
  error: string
  created_at: number
}

export interface LeadDetail {
  lead: SaleState
  customer: Customer | null
  organizations: OrgMembership[]
  messages: ConvMessage[]
  actions: SaleAction[]
  runs: SequenceRun[]
  jobs: SaleJob[]
  reviews: Review[]
}

/// db_sale.rs::sale_stats — hand-built camelCase.
export interface SaleStats {
  funnel: Record<string, number>
  won: number
  churned: number
  winRate: number
  hotLeads: number
  pendingReviews: number
  openEscalations: number
  unsubscribed: number
  tokens: number
}

// ---------- dynamic dashboard (db_dashboard.rs) ----------

export type FieldKind = 'enum' | 'text' | 'number' | 'date' | 'bool' | 'relation'

/// One filterable/groupable column of an element, as the registry describes it.
/// `values` is a fixed vocabulary; empty means an open set, and the candidate
/// list comes from `/dashboard/values` instead.
export interface DashField {
  key: string
  kind: FieldKind
  groupable: boolean
  operators: string[]
  values: string[]
}

/// `isMoney` is camelCase here — schema_json() hand-builds this one, unlike the
/// serde structs elsewhere in the file.
export interface DashMetric {
  key: string
  isMoney: boolean
}

export interface DashElement {
  key: string
  metrics: DashMetric[]
  fields: DashField[]
}

/// The whole chart registry. The builder renders every dropdown from this, so
/// the UI never carries a second copy of which metric belongs to which element.
export interface DashSchema {
  elements: DashElement[]
  displayTypes: string[]
  sizes: string[]
}

export interface ChartFilter {
  field: string
  op: string
  /// `in`/`notIn` take many, comparisons one, `between` two, `isNull` none.
  values: Array<string | number | boolean>
}

/// A free-form blob the UI owns end to end — the backend stores and returns it
/// without looking inside.
export interface ChartDisplay {
  type?: string
  showFilters?: boolean
  reverseX?: boolean
  reverseY?: boolean
  /// Per-bucket colour override, positional. Absent = derive from the value's
  /// semantic colour, else the shared palette.
  colors?: string[]
}

export interface Chart {
  id: number
  name: string
  element: string
  metric: string
  /// Empty = no grouping; the data is then one row with `bucket: ''`.
  grouping: string
  filters: ChartFilter[]
  display: ChartDisplay
  size: string
  sort: number
  is_template: boolean
  created_at: number
  updated_at: number
}

export interface ChartRow {
  bucket: string
  value: number
}

export interface ChartResult {
  rows: ChartRow[]
  /// Sum over the RETURNED buckets. When `truncated`, that is the top 200 only
  /// — not the grand total, so the card must never print it unqualified.
  total: number
  groups: number
  is_money: boolean
  /// Currencies that actually contributed to a money metric. Exactly one means
  /// `total` is a real amount in that currency. More than one means SUM added
  /// different currencies together and the number is NOT an amount — render it
  /// without a symbol and say so. Empty for counts.
  currencies: string[]
  /// One phrase per filter, positionally aligned with `Chart.filters`. Built
  /// server-side from raw keys ("stage not in won"), so it is displayed as-is
  /// or rebuilt client-side for localized field names — never parsed.
  filter_summary: string[]
  /// True when the grouping had more than 200 buckets and the tail was dropped
  /// (db_dashboard.rs::MAX_BUCKETS).
  truncated: boolean
}

/// `/dashboard/charts` resolves each chart's data server-side, so one card
/// either has `data` or an `error` — never both, never neither.
export interface ChartCell {
  chart: Chart
  data?: ChartResult
  error?: string
}

export interface ChartInput {
  name: string
  element: string
  metric: string
  grouping: string
  filters: ChartFilter[]
  display: ChartDisplay
  size: string
  is_template: boolean
}

/// The spec a preview runs — a chart minus everything presentational.
export interface ChartSpec {
  element: string
  metric: string
  grouping: string
  filters: ChartFilter[]
}

/// api_sale.rs::EXPOSED — the settings allowlist.
export interface CrmSettings {
  brand_voice?: string
  risky_keywords?: string
  complaint_keywords?: string
  max_messages_per_customer_24h?: string
  auto_welcome?: string
  language?: string
}

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(`/api${path}`, {
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers || {}) },
  })
  if (!r.ok) {
    let msg = `HTTP ${r.status}`
    try {
      const b = await r.json()
      if (b?.error) msg = b.error
    } catch {}
    throw new Error(msg)
  }
  if (r.status === 204) return undefined as T
  return r.json() as Promise<T>
}

export const api = {
  status: () => req<{ ok: boolean }>('/status'),
  stats: () => req<Stats>('/stats'),
  tags: () => req<{ tags: string[] }>('/tags').then((r) => r.tags),
  listCustomers: (params: { q?: string; tag?: string; role?: string; limit?: number } = {}) => {
    const qs = new URLSearchParams()
    if (params.q) qs.set('q', params.q)
    if (params.tag) qs.set('tag', params.tag)
    if (params.role) qs.set('role', params.role)
    if (params.limit) qs.set('limit', String(params.limit))
    const s = qs.toString()
    return req<{ customers: Customer[]; count: number }>(`/customers${s ? '?' + s : ''}`).then((r) => r.customers)
  },
  getCustomer: (id: number) => req<CustomerDetail>(`/customers/${id}`),
  createCustomer: (body: CustomerInput) =>
    req<Customer>('/customers', { method: 'POST', body: JSON.stringify(body) }),
  updateCustomer: (id: number, patch: CustomerInput & { change_note?: string }) =>
    req<Customer>(`/customers/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteCustomer: (id: number) => req<{ ok: boolean }>(`/customers/${id}`, { method: 'DELETE' }),
  listInteractions: (id: number) =>
    req<{ interactions: Interaction[] }>(`/customers/${id}/interactions`).then((r) => r.interactions),
  addInteraction: (id: number, body: { kind: string; summary: string; details?: string; occurred_at?: number }) =>
    req<Interaction>(`/customers/${id}/interactions`, { method: 'POST', body: JSON.stringify(body) }),
  deleteInteraction: (id: number) =>
    req<{ ok: boolean }>(`/interactions/${id}`, { method: 'DELETE' }),
  summarize: (id: number) => req<{ text: string; model: string }>(`/customers/${id}/summary`, { method: 'POST' }),
  nextStep: (id: number) => req<{ text: string; model: string }>(`/customers/${id}/next-step`, { method: 'POST' }),

  listDeals: (stage?: string) => {
    const qs = stage ? `?stage=${encodeURIComponent(stage)}` : ''
    return req<{ deals: Deal[]; count: number }>(`/deals${qs}`).then((r) => r.deals)
  },
  customerDeals: (id: number) => req<{ deals: Deal[] }>(`/customers/${id}/deals`).then((r) => r.deals),
  createDeal: (customerId: number, body: Partial<Deal>) =>
    req<Deal>(`/customers/${customerId}/deals`, { method: 'POST', body: JSON.stringify(body) }),
  updateDeal: (id: number, patch: Partial<Deal> & { change_note?: string }) =>
    req<{ deal: Deal }>(`/deals/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteDeal: (id: number) => req<{ ok: boolean }>(`/deals/${id}`, { method: 'DELETE' }),

  listTasks: (params: { open_only?: boolean; limit?: number } = {}) => {
    const qs = new URLSearchParams()
    if (params.open_only !== undefined) qs.set('open_only', String(params.open_only))
    if (params.limit) qs.set('limit', String(params.limit))
    const s = qs.toString()
    return req<{ tasks: Task[] }>(`/tasks${s ? '?' + s : ''}`).then((r) => r.tasks)
  },
  customerTasks: (id: number) => req<{ tasks: Task[] }>(`/customers/${id}/tasks`).then((r) => r.tasks),
  createTask: (body: { title: string; details?: string; due_at?: number; customer_id?: number }) => {
    const { customer_id, ...rest } = body
    const path = customer_id != null ? `/customers/${customer_id}/tasks` : '/tasks'
    return req<Task>(path, { method: 'POST', body: JSON.stringify(rest) })
  },
  toggleTask: (id: number, done: boolean) =>
    req<{ ok: boolean }>(`/tasks/${id}`, { method: 'PATCH', body: JSON.stringify({ done }) }),
  deleteTask: (id: number) => req<{ ok: boolean }>(`/tasks/${id}`, { method: 'DELETE' }),

  upcoming: (days = 14) => req<Upcoming>(`/upcoming?days=${days}`),
  activity: (limit = 100) => req<{ items: ActivityItem[] }>(`/activity?limit=${limit}`).then((r) => r.items),

  customerRelationships: (id: number) =>
    req<{ relationships: Relationship[] }>(`/customers/${id}/relationships`).then((r) => r.relationships),
  createRelationship: (body: { from_id: number; to_id: number; kind: string; note?: string; confidence?: number }) =>
    req<Relationship>(`/relationships`, { method: 'POST', body: JSON.stringify({ ...body, source: 'user' }) }),
  deleteRelationship: (id: number) => req<{ ok: boolean }>(`/relationships/${id}`, { method: 'DELETE' }),
  graph: () => req<{ nodes: GraphNode[]; edges: Relationship[] }>(`/graph`),
  graphPath: (from: number, to: number) =>
    req<{ found: boolean; hops: number; path_ids: number[] | null; nodes: GraphNode[]; edges: Relationship[] }>(
      `/graph/path?from=${from}&to=${to}`,
    ),
  graphExpand: (focus: number, hops: number) =>
    req<{ focus: number; hops: number; nodes: GraphNode[]; edges: Relationship[] }>(
      `/graph/expand?focus=${focus}&hops=${hops}`,
    ),
  listChannels: (customerId: number) =>
    req<{ channels: CustomerChannel[] }>(`/customers/${customerId}/channels`).then((r) => r.channels),
  addChannel: (customerId: number, body: { kind: string; value: string; label?: string }) =>
    req<CustomerChannel>(`/customers/${customerId}/channels`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  updateChannel: (id: number, patch: { kind?: string; value?: string; label?: string }) =>
    req<{ ok: boolean }>(`/channels/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteChannel: (id: number) => req<{ ok: boolean }>(`/channels/${id}`, { method: 'DELETE' }),

  pathAi: (from: number, to: number) =>
    req<{
      from: number
      to: number
      model: string
      summary: string
      connections: Array<{ type: string; detail: string; strength: string }>
      bfs_path_ids: number[] | null
      bfs_path_names: string[] | null
    }>(`/graph/path_ai?from=${from}&to=${to}`, { method: 'POST' }),
  getState: <T = unknown>(key: string) =>
    req<{ key: string; value: T | null }>(`/state/${encodeURIComponent(key)}`).then((r) => r.value),
  putState: (key: string, value: unknown) =>
    req<{ ok: boolean }>(`/state/${encodeURIComponent(key)}`, {
      method: 'PUT',
      body: JSON.stringify(value),
    }),
  deleteState: (key: string) =>
    req<{ ok: boolean }>(`/state/${encodeURIComponent(key)}`, { method: 'DELETE' }),
  findCommon: (id: number) =>
    req<{
      focus_id: number
      model: string
      themes: Array<{ theme: string; why: string; customer_ids: number[] }>
      highlight_ids: number[]
    }>(`/customers/${id}/find_common`, { method: 'POST' }),
  similar: (id: number) =>
    req<{
      similar: Array<{ customer: Customer; score: number; reasons: string[] }>
      count: number
    }>(`/customers/${id}/similar`),
  search: (q: string, limit = 30) => {
    const qs = new URLSearchParams({ q, limit: String(limit) })
    return req<{ hits: SearchHit[]; count: number }>(`/search?${qs}`).then((r) => r.hits)
  },
  mentions: (unresolved_only = false, limit = 100) => {
    const qs = new URLSearchParams({ unresolved_only: String(unresolved_only), limit: String(limit) })
    return req<{ mentions: Mention[] }>(`/mentions?${qs}`).then((r) => r.mentions)
  },
  extract: (id: number) =>
    req<{ model: string; extracted: number; mentions_saved: number; relationships_created: number; resolved: Array<{ name: string; resolved_customer_id: number; kind: string; confidence: number }> }>(
      `/customers/${id}/extract`,
      { method: 'POST' },
    ),

  aggregateReport: () =>
    req<{
      text: string
      model: string
      generated_at: number
      grounding: {
        customers: number
        open_deals: number
        pipeline_value: number
        top_deals: number
        recent_events: number
        overdue_tasks: number
      }
    }>(`/report`, { method: 'POST' }),

  // ---------- organizations ----------
  listOrganizations: (params: { q?: string; kind?: string; limit?: number } = {}) =>
    req<{ organizations: Organization[] }>(`/organizations${qs(params)}`).then((r) => r.organizations),
  getOrganization: (id: number) => req<OrgDetail>(`/organizations/${id}`),
  createOrganization: (body: OrganizationInput) =>
    req<{ organization: Organization }>('/organizations', {
      method: 'POST',
      body: JSON.stringify(body),
    }).then((r) => r.organization),
  updateOrganization: (id: number, patch: OrganizationInput) =>
    req<{ organization: Organization }>(`/organizations/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(patch),
    }).then((r) => r.organization),
  deleteOrganization: (id: number) => req<{ ok: boolean }>(`/organizations/${id}`, { method: 'DELETE' }),
  orgContacts: (id: number) =>
    req<{ contacts: OrgContact[] }>(`/organizations/${id}/contacts`).then((r) => r.contacts),
  orgDeals: (id: number) => req<{ deals: Deal[] }>(`/organizations/${id}/deals`).then((r) => r.deals),

  // ---------- person ↔ org ----------
  customerOrgs: (id: number) =>
    req<{ organizations: OrgMembership[] }>(`/customers/${id}/organizations`).then((r) => r.organizations),
  /// Accepts `{organization_id}` OR `{organization_name}` — the latter is
  /// resolved-or-created server-side, which is what the type-ahead relies on.
  linkCustomerOrg: (
    id: number,
    body: { organization_id?: number; organization_name?: string; role_title?: string; is_primary?: boolean },
  ) =>
    req<{ organizations: OrgMembership[] }>(`/customers/${id}/organizations`, {
      method: 'POST',
      body: JSON.stringify(body),
    }).then((r) => r.organizations),
  unlinkCustomerOrg: (id: number, orgId: number) =>
    req<{ organizations: OrgMembership[] }>(`/customers/${id}/organizations/${orgId}`, {
      method: 'DELETE',
    }).then((r) => r.organizations),

  // ---------- services ----------
  listServices: (params: { q?: string; kind?: string; active_only?: boolean; limit?: number } = {}) =>
    req<{ services: Service[] }>(`/services${qs(params)}`).then((r) => r.services),
  getService: (id: number) => req<{ service: Service }>(`/services/${id}`).then((r) => r.service),
  createService: (body: ServiceInput) =>
    req<{ service: Service }>('/services', { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.service,
    ),
  updateService: (id: number, patch: ServiceInput) =>
    req<{ service: Service }>(`/services/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.service,
    ),
  /// 400 + a "deactivate it instead" message when the entry priced a real deal.
  deleteService: (id: number) => req<{ ok: boolean }>(`/services/${id}`, { method: 'DELETE' }),

  // ---------- deal line items ----------
  dealServices: (dealId: number) => req<DealServices>(`/deals/${dealId}/services`),
  attachService: (
    dealId: number,
    body: { service_id: number; quantity?: number; unit_amount?: number; note?: string },
  ) =>
    req<{ services: DealService[] }>(`/deals/${dealId}/services`, {
      method: 'POST',
      body: JSON.stringify(body),
    }).then((r) => r.services),
  detachService: (dealId: number, serviceId: number) =>
    req<{ services: DealService[] }>(`/deals/${dealId}/services/${serviceId}`, {
      method: 'DELETE',
    }).then((r) => r.services),

  // ---------- inbox ----------
  inboxStats: () => req<InboxStats>('/inbox/stats'),
  listInboxChannels: () => req<{ channels: InboxChannel[] }>('/inbox/channels').then((r) => r.channels),
  createInboxChannel: (body: InboxChannelInput) =>
    req<{ id: number }>('/inbox/channels', { method: 'POST', body: JSON.stringify(body) }),
  updateInboxChannel: (id: number, patch: InboxChannelPatch) =>
    req<{ ok: boolean }>(`/inbox/channels/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }),
  deleteInboxChannel: (id: number) => req<{ ok: boolean }>(`/inbox/channels/${id}`, { method: 'DELETE' }),
  testInboxChannel: (id: number) =>
    req<{ ok: boolean; info?: unknown; error?: string }>(`/inbox/channels/${id}/test`, { method: 'POST' }),

  listConversations: (
    params: { status?: string; kind?: string; customer_id?: number; q?: string; limit?: number } = {},
  ) => req<{ conversations: Conversation[] }>(`/inbox/conversations${qs(params)}`).then((r) => r.conversations),
  getConversation: (id: number) => req<ConversationDetail>(`/inbox/conversations/${id}`),
  startConversation: (body: {
    kind: string
    channel_id?: number
    customer_id?: number
    external_id?: string
    text?: string
  }) =>
    req<{ conversation: Conversation }>('/inbox/conversations', {
      method: 'POST',
      body: JSON.stringify(body),
    }).then((r) => r.conversation),
  sendConvMessage: (id: number, body: { text: string; by?: string }) =>
    req<{ ok: boolean; id: number }>(`/inbox/conversations/${id}/send`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  linkConversation: (id: number, customer_id: number) =>
    req<{ conversation: Conversation }>(`/inbox/conversations/${id}/link`, {
      method: 'POST',
      body: JSON.stringify({ customer_id }),
    }).then((r) => r.conversation),
  setConversationStatus: (id: number, status: string) =>
    req<{ ok: boolean }>(`/inbox/conversations/${id}/status`, {
      method: 'POST',
      body: JSON.stringify({ status }),
    }),
  setHandoff: (id: number, state: string) =>
    req<{ ok: boolean }>(`/inbox/conversations/${id}/handoff`, {
      method: 'POST',
      body: JSON.stringify({ state }),
    }),
  markConversationRead: (id: number) =>
    req<{ ok: boolean }>(`/inbox/conversations/${id}/read`, { method: 'POST' }),

  // ---------- sale ----------
  saleStats: () => req<SaleStats>('/sale/stats'),
  listLeads: (params: { stage?: string; temperature?: string; q?: string; limit?: number } = {}) =>
    req<{ leads: SaleState[] }>(`/sale/leads${qs(params)}`).then((r) => r.leads),
  getLead: (id: number) => req<LeadDetail>(`/sale/leads/${id}`),
  leadNextAction: (id: number, body: { intent?: string; channel?: string }) =>
    req<Record<string, unknown>>(`/sale/leads/${id}/next-action`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  leadDraft: (id: number, body: { intent?: string }) =>
    req<{ draft: string }>(`/sale/leads/${id}/draft`, { method: 'POST', body: JSON.stringify(body) }),
  setLeadStage: (id: number, body: { stage?: string; temperature?: string; lead_score?: number }) =>
    req<{ lead: SaleState }>(`/sale/leads/${id}/stage`, { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.lead,
    ),
  setLeadUnsubscribed: (id: number, on: boolean) =>
    req<{ lead: SaleState }>(`/sale/leads/${id}/unsubscribe`, {
      method: 'POST',
      body: JSON.stringify({ on }),
    }).then((r) => r.lead),
  startLeadSequence: (id: number, sequence_key: string) =>
    req<{ run_id: number; runs: SequenceRun[] }>(`/sale/leads/${id}/sequence`, {
      method: 'POST',
      body: JSON.stringify({ sequence_key }),
    }),
  sendToLead: (id: number, body: { text: string; channel?: string; is_reply?: boolean }) =>
    req<Record<string, unknown>>(`/sale/leads/${id}/send`, { method: 'POST', body: JSON.stringify(body) }),
  listSequences: () => req<{ sequences: Sequence[] }>('/sale/sequences').then((r) => r.sequences),
  setSequenceEnabled: (key: string, enabled: boolean) =>
    req<{ ok: boolean }>(`/sale/sequences/${encodeURIComponent(key)}/enabled`, {
      method: 'POST',
      body: JSON.stringify({ enabled }),
    }),

  listReviews: (params: { status?: string; limit?: number } = {}) =>
    req<{ reviews: Review[] }>(`/sale/reviews${qs(params)}`).then((r) => r.reviews),
  approveReview: (id: number, body: { edited?: string; by?: string }) =>
    req<Record<string, unknown>>(`/sale/reviews/${id}/approve`, {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  rejectReview: (id: number, body: { by?: string } = {}) =>
    req<{ ok: boolean }>(`/sale/reviews/${id}/reject`, { method: 'POST', body: JSON.stringify(body) }),

  listEscalations: (params: { status?: string; limit?: number } = {}) =>
    req<{ escalations: Escalation[] }>(`/sale/escalations${qs(params)}`).then((r) => r.escalations),
  resolveEscalation: (id: number, body: { by?: string } = {}) =>
    req<{ ok: boolean }>(`/sale/escalations/${id}/resolve`, { method: 'POST', body: JSON.stringify(body) }),

  listSaleActions: (params: { customer_id?: number; limit?: number } = {}) =>
    req<{ actions: SaleAction[] }>(`/sale/actions${qs(params)}`).then((r) => r.actions),
  listSaleJobs: (params: { customer_id?: number; limit?: number } = {}) =>
    req<{ jobs: SaleJob[] }>(`/sale/jobs${qs(params)}`).then((r) => r.jobs),

  // ---------- dynamic dashboard ----------
  dashboardSchema: () => req<DashSchema>('/dashboard/schema'),
  /// Every chart WITH its data already resolved — one round-trip, not N+1.
  listCharts: () => req<{ charts: ChartCell[] }>('/dashboard/charts').then((r) => r.charts),
  chartData: (id: number) => req<{ data: ChartResult }>(`/dashboard/charts/${id}/data`).then((r) => r.data),
  /// 400 + a readable `error` when the combination doesn't compile — `req`
  /// rethrows that message verbatim for the builder to show.
  createChart: (body: Partial<ChartInput>) =>
    req<{ chart: Chart }>('/dashboard/charts', { method: 'POST', body: JSON.stringify(body) }).then(
      (r) => r.chart,
    ),
  updateChart: (id: number, patch: Partial<ChartInput>) =>
    req<{ chart: Chart }>(`/dashboard/charts/${id}`, { method: 'PATCH', body: JSON.stringify(patch) }).then(
      (r) => r.chart,
    ),
  deleteChart: (id: number) => req<{ ok: boolean }>(`/dashboard/charts/${id}`, { method: 'DELETE' }),
  /// The full ordered id list, as produced by a drag.
  reorderCharts: (ids: number[]) =>
    req<{ ok: boolean }>('/dashboard/charts/reorder', { method: 'POST', body: JSON.stringify({ ids }) }),
  /// Run an unsaved spec — what makes the builder's preview live.
  previewChart: (spec: ChartSpec) =>
    req<{ data: ChartResult }>('/dashboard/preview', { method: 'POST', body: JSON.stringify(spec) }).then(
      (r) => r.data,
    ),
  /// Candidate filter values: the fixed vocabulary, or distinct values from the
  /// data for open sets (industry, source, currency).
  chartFieldValues: (element: string, field: string) =>
    req<{ values: string[] }>(`/dashboard/values${qs({ element, field })}`).then((r) => r.values),

  // ---------- settings ----------
  getSettings: () => req<CrmSettings>('/settings'),
  updateSettings: (patch: CrmSettings) =>
    req<CrmSettings>('/settings', { method: 'POST', body: JSON.stringify(patch) }),
}

/// Build a `?a=1&b=2` string, dropping undefined/empty values.
function qs(params: Record<string, string | number | boolean | undefined>): string {
  const p = new URLSearchParams()
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === '' || v === null) continue
    p.set(k, String(v))
  }
  const s = p.toString()
  return s ? `?${s}` : ''
}

export function formatMoney(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(amount)
  } catch {
    return `${amount.toLocaleString()} ${currency}`
  }
}

export function fmtDate(secs: number | null | undefined): string {
  if (!secs) return '—'
  const d = new Date(secs * 1000)
  return d.toLocaleDateString(undefined, { year: 'numeric', month: '2-digit', day: '2-digit' })
}

export function fmtDateTime(secs: number | null | undefined): string {
  if (!secs) return '—'
  const d = new Date(secs * 1000)
  return d.toLocaleString(undefined, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

/// Deterministic initials for the avatar fallback (skip diacritics-only words).
export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean)
  if (parts.length === 0) return '?'
  if (parts.length === 1) return parts[0]!.slice(0, 2).toUpperCase()
  return (parts[0]![0]! + parts[parts.length - 1]![0]!).toUpperCase()
}

/// Deterministic HSL hue for a customer, from the name → stable colour without
/// pulling a colour library.
export function hueFromName(name: string): number {
  let h = 0
  for (const ch of name) h = (h * 31 + ch.charCodeAt(0)) | 0
  return ((h % 360) + 360) % 360
}
