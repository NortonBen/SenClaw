// Tiny fetch wrapper for the Shopee app REST API (served from the same origin
// in production; proxied to :4490 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

export interface Status {
  connected: boolean
  autonomy: string
  pending_drafts: number
}

export interface SettingsPublic {
  partner_id: string
  shop_id: string
  host: string
  autonomy: string
  partner_key_set: boolean
}

export interface Draft {
  id: number
  status: string
  conversation_id: string
  to_id: number
  to_name: string
  content: string
  source: string
  model: string
  error: string
}

export const api = {
  status: () => j<Status>('/api/status'),
  getSettings: () => j<SettingsPublic>('/api/settings'),
  setSettings: (body: Partial<SettingsPublic> & { partner_key?: string }) =>
    j<SettingsPublic>('/api/settings', { method: 'POST', body: JSON.stringify(body) }),
  account: () => j('/api/account'),
  oauthLink: (redirect: string) =>
    j<{ url?: string; error?: string }>(`/api/oauth/link?redirect=${encodeURIComponent(redirect)}`),
  orders: () => j('/api/orders'),
  conversations: () => j('/api/chat/conversations'),
  reply: (body: {
    conversation_id: string
    to_id: number
    to_name?: string
    content?: string
    customer_msg?: string
    context?: string
    order_sn?: string
  }) => j('/api/chat/reply', { method: 'POST', body: JSON.stringify(body) }),
  orderDetail: (sn: string) => j(`/api/orders/detail?sn=${encodeURIComponent(sn)}`),
  drafts: () => j<{ pending: Draft[] }>('/api/drafts'),
  approve: (id: number) => j(`/api/drafts/${id}/approve`, { method: 'POST' }),
  reject: (id: number) => j(`/api/drafts/${id}/reject`, { method: 'POST' }),
  activity: () => j<{ activity: any[] }>('/api/activity'),
  tick: () => j('/api/engine/tick', { method: 'POST' }),
  products: (status = 'NORMAL') => j(`/api/products?status=${encodeURIComponent(status)}`),
  updateStock: (item_id: number, stock: number) =>
    j('/api/products/stock', { method: 'POST', body: JSON.stringify({ item_id, stock }) }),
  updatePrice: (item_id: number, price: number) =>
    j('/api/products/price', { method: 'POST', body: JSON.stringify({ item_id, price }) }),
}
