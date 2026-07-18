// Enum vocabularies + their presentation metadata (icon / colour / href).
// Labels deliberately live in i18n.ts — look them up with `tk(t, '<prefix>', value)`.

// ---- deal stages (deals.stage) ----
export const STAGE_ORDER = ['qualifying', 'proposal', 'negotiation', 'won', 'lost']
export const STAGE_COLORS: Record<string, string> = {
  qualifying: '#8e8e93',
  proposal: '#007aff',
  negotiation: '#5e4ae3',
  won: '#34c759',
  lost: '#ff3b30',
}

// ---- sale stages (customers.sale_stage) ----
export const SALE_STAGES = [
  'new_lead',
  'engaged',
  'qualified',
  'consult_scheduled',
  'consult_done',
  'closed_won',
  'churned',
]
export const SALE_STAGE_COLORS: Record<string, string> = {
  new_lead: '#8e8e93',
  engaged: '#007aff',
  qualified: '#5e4ae3',
  consult_scheduled: '#af52de',
  consult_done: '#ff9500',
  closed_won: '#34c759',
  churned: '#ff3b30',
}

export const TEMPERATURES = ['cold', 'warm', 'hot', 'churned']
export const TEMP_META: Record<string, { icon: string; color: string }> = {
  cold: { icon: '🧊', color: '#007aff' },
  warm: { icon: '🌤', color: '#ff9500' },
  hot: { icon: '🔥', color: '#ff3b30' },
  churned: { icon: '💤', color: '#8e8e93' },
}

export const SALE_INTENTS = [
  'welcome_and_value',
  'share_value_content',
  'soft_offer_consultation',
  're_engage_soft',
]

// ---- organizations ----
export const ORG_KINDS = [
  'direct_customer',
  'affiliated_company',
  'partner',
  'supplier',
  'prospect',
]
export const ORG_KIND_COLORS: Record<string, string> = {
  direct_customer: '#34c759',
  affiliated_company: '#5e4ae3',
  partner: '#007aff',
  supplier: '#ff9500',
  prospect: '#8e8e93',
}

// ---- services ----
export const SERVICE_KINDS = ['service', 'hardware']
export const SERVICE_KIND_COLORS: Record<string, string> = {
  service: '#5e4ae3',
  hardware: '#ff9500',
}
export const PRICING_MODELS = ['fixed', 'hourly', 'daily', 'monthly', 'yearly']
export const PRICING_COLORS: Record<string, string> = {
  fixed: '#8e8e93',
  hourly: '#007aff',
  daily: '#af52de',
  monthly: '#5e4ae3',
  yearly: '#34c759',
}

// ---- inbox channel kinds (OUR connected accounts) ----
export const INBOX_CHANNEL_KINDS = ['telegram', 'zalo', 'facebook', 'tiktok', 'websocket']
export const INBOX_CHANNEL_META: Record<string, { icon: string; color: string }> = {
  telegram: { icon: '✈️', color: '#26a5e4' },
  zalo: { icon: '💬', color: '#0068ff' },
  facebook: { icon: '📘', color: '#1877f2' },
  tiktok: { icon: '🎵', color: '#000000' },
  websocket: { icon: '🔌', color: '#5e4ae3' },
}
export function inboxChannelMeta(kind: string) {
  return INBOX_CHANNEL_META[kind] ?? { icon: '💬', color: '#8e8e93' }
}

/// Config fields each channel kind expects, and which of them are secrets.
/// A field named token/secret/api_key renders as `Input.Password` and arrives
/// back from the API as `••••••` meaning "unchanged".
export const CHANNEL_CONFIG_FIELDS: Record<string, Array<{ key: string; secret?: boolean }>> = {
  telegram: [{ key: 'token', secret: true }],
  zalo: [{ key: 'access_token', secret: true }, { key: 'oa_id' }],
  facebook: [
    { key: 'access_token', secret: true },
    { key: 'app_secret', secret: true },
    { key: 'page_id' },
  ],
  tiktok: [{ key: 'access_token', secret: true }, { key: 'refresh_token', secret: true }],
  websocket: [{ key: 'url' }, { key: 'api_key', secret: true }],
}
export const SECRET_MASK = '••••••'
export function isSecretField(key: string): boolean {
  return /token|secret|api_key|password/i.test(key)
}

// ---- contact roles ----
export type RoleMeta = { short: string; icon: string; color: string }
export const ROLE_META: Record<string, RoleMeta> = {
  lead: { short: 'Lead', icon: '🌱', color: '#8e8e93' },
  prospect: { short: 'Prospect', icon: '🔍', color: '#007aff' },
  customer: { short: 'Customer', icon: '🤝', color: '#34c759' },
  vip: { short: 'VIP', icon: '⭐', color: '#af52de' },
  contact: { short: 'Contact', icon: '👤', color: '#5e4ae3' },
  partner: { short: 'Partner', icon: '🤝', color: '#0ea5e9' },
  referrer: { short: 'Referrer', icon: '📣', color: '#eab308' },
  supplier: { short: 'Supplier', icon: '📦', color: '#ff9500' },
  investor: { short: 'Investor', icon: '💰', color: '#14b8a6' },
  employee: { short: 'Employee', icon: '🧑‍💼', color: '#8b5cf6' },
  former: { short: 'Former', icon: '🕰', color: '#94a3b8' },
  paused: { short: 'Paused', icon: '⏸', color: '#b45309' },
  lost: { short: 'Lost', icon: '❌', color: '#ff3b30' },
}
export const ROLE_ORDER = [
  'lead',
  'prospect',
  'customer',
  'vip',
  'contact',
  'partner',
  'referrer',
  'supplier',
  'investor',
  'employee',
  'former',
  'paused',
  'lost',
]
export function roleMeta(role: string): RoleMeta {
  return ROLE_META[role] ?? { short: role, icon: '•', color: '#8e8e93' }
}

// ---- contact channels (THEIR identities) ----
export type ChannelMeta = {
  icon: string
  color: string
  placeholder: string
  href: (value: string) => string
}
export const CHANNEL_META: Record<string, ChannelMeta> = {
  phone: { icon: '📞', color: '#0ea5e9', placeholder: '0900…', href: (v) => `tel:${v.replace(/\s/g, '')}` },
  email: { icon: '✉️', color: '#5e4ae3', placeholder: 'user@example.com', href: (v) => `mailto:${v}` },
  zalo: {
    icon: '💬',
    color: '#0068ff',
    // Placeholders stay language-neutral — they are examples, not prose.
    placeholder: '0900… / user',
    href: (v) =>
      /^\d+$/.test(v.replace(/\s/g, ''))
        ? `https://zalo.me/${v.replace(/\s/g, '')}`
        : `https://zalo.me/${v.replace(/^@/, '')}`,
  },
  facebook: {
    icon: '📘',
    color: '#1877f2',
    placeholder: 'username / URL',
    href: (v) => (v.startsWith('http') ? v : `https://facebook.com/${v.replace(/^@/, '')}`),
  },
  messenger: {
    icon: '💌',
    color: '#00b2ff',
    placeholder: 'username',
    href: (v) => (v.startsWith('http') ? v : `https://m.me/${v.replace(/^@/, '')}`),
  },
  instagram: {
    icon: '📷',
    color: '#e4405f',
    placeholder: 'username',
    href: (v) => (v.startsWith('http') ? v : `https://instagram.com/${v.replace(/^@/, '')}`),
  },
  linkedin: {
    icon: '💼',
    color: '#0a66c2',
    placeholder: 'profile URL',
    href: (v) => (v.startsWith('http') ? v : `https://linkedin.com/in/${v.replace(/^@/, '')}`),
  },
  x: {
    icon: '🐦',
    color: '#000000',
    placeholder: 'username',
    href: (v) => (v.startsWith('http') ? v : `https://x.com/${v.replace(/^@/, '')}`),
  },
  tiktok: {
    icon: '🎵',
    color: '#000000',
    placeholder: 'username',
    href: (v) => (v.startsWith('http') ? v : `https://tiktok.com/@${v.replace(/^@/, '')}`),
  },
  youtube: {
    icon: '▶️',
    color: '#ff0000',
    placeholder: '@channel / URL',
    href: (v) => (v.startsWith('http') ? v : `https://youtube.com/${v.startsWith('@') ? v : '@' + v}`),
  },
  github: {
    icon: '🐙',
    color: '#181717',
    placeholder: 'username',
    href: (v) => (v.startsWith('http') ? v : `https://github.com/${v.replace(/^@/, '')}`),
  },
  telegram: {
    icon: '✈️',
    color: '#26a5e4',
    placeholder: '@username',
    href: (v) => (v.startsWith('http') ? v : `https://t.me/${v.replace(/^@/, '')}`),
  },
  whatsapp: { icon: '📱', color: '#25d366', placeholder: '84900…', href: (v) => `https://wa.me/${v.replace(/[^\d]/g, '')}` },
  signal: { icon: '🔒', color: '#3a76f0', placeholder: '+84…', href: (v) => `https://signal.me/#p/${v.replace(/\s/g, '')}` },
  line: {
    icon: '💚',
    color: '#00c300',
    placeholder: 'lineid',
    href: (v) => (v.startsWith('http') ? v : `https://line.me/ti/p/${v.replace(/^@/, '')}`),
  },
  wechat: { icon: '🇨🇳', color: '#07c160', placeholder: 'wechatid', href: (v) => `weixin://dl/chat?${v.replace(/^@/, '')}` },
  skype: { icon: '☁️', color: '#00aff0', placeholder: 'skypeid', href: (v) => `skype:${v}?chat` },
  viber: { icon: '🍇', color: '#7360f2', placeholder: '84900…', href: (v) => `viber://chat?number=%2B${v.replace(/[^\d]/g, '')}` },
  discord: {
    icon: '🎮',
    color: '#5865f2',
    placeholder: 'username',
    href: (v) => (v.startsWith('http') ? v : `https://discord.com/users/${v.replace(/^@/, '')}`),
  },
  website: { icon: '🌐', color: '#8e8e93', placeholder: 'https://…', href: (v) => (v.startsWith('http') ? v : `https://${v}`) },
}
export const CHANNEL_KINDS = Object.keys(CHANNEL_META)
export function channelMeta(kind: string): ChannelMeta {
  return CHANNEL_META[kind] ?? { icon: '🔗', color: '#8e8e93', placeholder: 'value', href: (v) => v }
}

// ---- relationships ----
export const REL_ORDER = [
  'referred_by',
  'introduced_by',
  'colleague_of',
  'spouse_of',
  'family_of',
  'friend_of',
  'reports_to',
  'partner_of',
  'supplier_of',
  'competitor_of',
  'contact_of',
]

// ---- interaction kinds ----
export const KIND_ICONS: Record<string, string> = {
  call: '📞',
  email: '✉️',
  meeting: '🤝',
  note: '📝',
  task: '✅',
  profile_update: '✏️',
  deal_update: '💼',
}
export const KIND_ORDER = ['call', 'email', 'meeting', 'note', 'task', 'profile_update', 'deal_update']

// ---- dynamic dashboard charts (db_dashboard.rs registry) ----

/// Series colours for chart buckets with no semantic colour of their own
/// (organization names, industries, currencies…). Ordered so neighbouring
/// slices stay distinguishable.
export const CHART_PALETTE = [
  '#5e4ae3',
  '#007aff',
  '#34c759',
  '#ff9500',
  '#af52de',
  '#ff3b30',
  '#0ea5e9',
  '#14b8a6',
  '#eab308',
  '#8e8e93',
]

/// The reference CRM prints a currency on money charts; `ChartResult` carries
/// no currency (the SUM crosses whatever currencies the rows are in), so the
/// dashboard renders money in one house currency — same assumption the KPI row
/// already makes.
export const DEFAULT_CURRENCY = 'VND'

/// How to render an enum field's raw values: which i18n prefix names them, and
/// which colour map paints them.
///
/// Keyed by `<element>.<field>`, not by field alone, because the same field key
/// means different things per element — `kind` is an org kind on `organization`
/// and a service kind on `service`. This is presentation metadata only; which
/// fields exist and which are groupable still comes from `/dashboard/schema`.
export const CHART_FIELD_VOCAB: Record<string, { prefix: string; colors?: Record<string, string> }> = {
  'contact.role': { prefix: 'role', colors: Object.fromEntries(Object.entries(ROLE_META).map(([k, v]) => [k, v.color])) },
  'contact.sale_stage': { prefix: 'saleStage', colors: SALE_STAGE_COLORS },
  'contact.temperature': { prefix: 'temp', colors: Object.fromEntries(Object.entries(TEMP_META).map(([k, v]) => [k, v.color])) },
  'contact.unsubscribed': { prefix: 'boolVal' },
  'organization.kind': { prefix: 'orgKind', colors: ORG_KIND_COLORS },
  'deal.stage': { prefix: 'dealStage', colors: STAGE_COLORS },
  'service.kind': { prefix: 'svcKind', colors: SERVICE_KIND_COLORS },
  'service.pricing_model': { prefix: 'pricing', colors: PRICING_COLORS },
  'service.active': { prefix: 'boolVal' },
  'task.status': { prefix: 'taskStatus', colors: { open: '#007aff', done: '#34c759' } },
}

// ---- design tokens shared with CSS ----
export const ACCENT = '#5e4ae3'
export const SEMANTIC = {
  success: '#34c759',
  warning: '#ff9500',
  danger: '#ff3b30',
  info: '#007aff',
}
