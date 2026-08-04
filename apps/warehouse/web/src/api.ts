// Tiny fetch wrapper for the Warehouse app REST API (served from the same
// origin in production; proxied to :4630 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

export interface Status {
  ok: boolean
  products_active: number
  stock_value: number
  low_stock_count: number
}

export interface Product {
  id: number
  sku: string
  name: string
  unit: string
  category: string
  barcode: string
  cost_price: number
  sell_price: number
  min_stock: number
  status: string
  note: string
  on_hand: number
  avg_cost: number
  stock_value: number
  low_stock: boolean
}

export interface Warehouse {
  id: number
  name: string
  location: string
  note: string
  status: string
  sku_count: number
  stock_value: number
}

export interface Partner {
  id: number
  name: string
  kind: string
  phone: string
  address: string
  note: string
}

export interface MoveLine {
  id: number
  product_id: number
  sku: string
  product_name: string
  unit: string
  qty: number
  unit_price: number
  amount: number
}

export interface Move {
  id: number
  code: string
  kind: string
  warehouse_id: number
  warehouse_name: string
  to_warehouse_id: number | null
  to_warehouse_name: string | null
  partner_id: number | null
  partner_name: string | null
  move_date: string
  note: string
  line_count: number
  total_qty: number
  total_value: number
  lines?: MoveLine[]
}

export interface StockRow {
  product_id: number
  sku: string
  product_name: string
  unit: string
  warehouse_id: number
  warehouse_name: string
  qty: number
  avg_cost: number
  value: number
}

export interface CardRow {
  code: string
  kind: string
  date: string
  warehouse: string
  in_qty: number
  out_qty: number
  unit_price: number
  balance: number
  note: string
}

export interface InoutRow {
  month: string
  in_qty: number
  in_value: number
  out_qty: number
  out_value: number
  adjust_qty: number
  net_qty: number
}

export interface InsightItem {
  id: number
  sku: string
  name: string
  unit: string
  category: string
  on_hand: number
  stock_value: number
  avg_cost: number
  sell_price: number
  sold_qty: number
  sold_value: number
  received_qty: number
  velocity_30d: number
  days_of_stock: number | null
  margin_pct: number | null
  sell_through_pct: number
  last_sale_date: string | null
  class: string
}

export interface Insight {
  today: string
  window_days: number
  items: InsightItem[]
  summary: {
    potential_count: number
    steady_count: number
    slow_count: number
    dead_count: number
    idle_count: number
    dead_stock_value: number
    top_sellers: InsightItem[]
  }
}

export interface Dashboard {
  today: string
  products_active: number
  warehouses: Warehouse[]
  stock_value: number
  low_stock: { count: number; items: Product[] }
  out_of_stock_count: number
  in_30d: { qty: number; value: number }
  out_30d: { qty: number; value: number }
  inout_12m: InoutRow[]
  top_products: Product[]
  recent_moves: Move[]
}

export const MOVE_KIND_LABELS: Record<string, string> = {
  receipt: 'Nhập kho',
  issue: 'Xuất kho',
  transfer: 'Chuyển kho',
  adjust: 'Điều chỉnh',
}

export const MOVE_KIND_COLORS: Record<string, string> = {
  receipt: 'green',
  issue: 'volcano',
  transfer: 'blue',
  adjust: 'gold',
}

export const PARTNER_KIND_LABELS: Record<string, string> = {
  supplier: 'Nhà cung cấp',
  customer: 'Khách hàng',
  other: 'Khác',
}

export const CLASS_LABELS: Record<string, string> = {
  potential: 'Tiềm năng',
  steady: 'Ổn định',
  slow: 'Bán chậm',
  dead: 'Tồn đọng',
  idle: 'Chưa kinh doanh',
}

export const CLASS_COLORS: Record<string, string> = {
  potential: 'green',
  steady: 'blue',
  slow: 'orange',
  dead: 'red',
  idle: 'default',
}

export function fmtMoney(v: number | null | undefined): string {
  if (v === null || v === undefined) return '—'
  return `${new Intl.NumberFormat('vi-VN', { maximumFractionDigits: 2 }).format(v)} đ`
}

export function fmtQty(v: number | null | undefined): string {
  if (v === null || v === undefined) return '—'
  return new Intl.NumberFormat('vi-VN', { maximumFractionDigits: 3 }).format(v)
}

export const api = {
  status: () => j<Status>('/api/status'),
  dashboard: () => j<Dashboard>('/api/dashboard'),
  products: (q: { q?: string; category?: string; status?: string; low_stock?: boolean } = {}) => {
    const p = new URLSearchParams()
    if (q.q) p.set('q', q.q)
    if (q.category) p.set('category', q.category)
    if (q.status) p.set('status', q.status)
    if (q.low_stock) p.set('low_stock', 'true')
    return j<{ products: Product[] }>(`/api/products?${p}`)
  },
  productGet: (id: number) =>
    j<{ product: Product; by_warehouse: StockRow[]; card: CardRow[]; error?: string }>(`/api/products/${id}`),
  productAdd: (body: Partial<Product>) =>
    j<{ ok?: boolean; product?: Product; error?: string }>('/api/products', { method: 'POST', body: JSON.stringify(body) }),
  productUpdate: (id: number, patch: Partial<Product>) =>
    j<{ ok?: boolean; product?: Product; error?: string }>(`/api/products/${id}`, { method: 'POST', body: JSON.stringify(patch) }),
  warehouses: (status?: string) =>
    j<{ warehouses: Warehouse[] }>(`/api/warehouses${status ? `?status=${status}` : ''}`),
  warehouseAdd: (body: { name: string; location?: string; note?: string }) =>
    j<{ ok?: boolean; warehouse_id?: number; error?: string }>('/api/warehouses', { method: 'POST', body: JSON.stringify(body) }),
  warehouseUpdate: (id: number, patch: Partial<Warehouse>) =>
    j<{ ok?: boolean; error?: string }>(`/api/warehouses/${id}`, { method: 'POST', body: JSON.stringify(patch) }),
  partners: (kind?: string) => j<{ partners: Partner[] }>(`/api/partners${kind ? `?kind=${kind}` : ''}`),
  partnerAdd: (body: { name: string; kind?: string; phone?: string; address?: string; note?: string }) =>
    j<{ ok?: boolean; partner_id?: number; error?: string }>('/api/partners', { method: 'POST', body: JSON.stringify(body) }),
  moves: (q: { kind?: string; warehouse_id?: number; product_id?: number; date_from?: string; date_to?: string } = {}) => {
    const p = new URLSearchParams()
    if (q.kind) p.set('kind', q.kind)
    if (q.warehouse_id) p.set('warehouse_id', String(q.warehouse_id))
    if (q.product_id) p.set('product_id', String(q.product_id))
    if (q.date_from) p.set('date_from', q.date_from)
    if (q.date_to) p.set('date_to', q.date_to)
    return j<{ moves: Move[] }>(`/api/moves?${p}`)
  },
  moveGet: (id: number) => j<{ move?: Move; error?: string }>(`/api/moves/${id}`),
  moveCreate: (body: {
    kind: string
    warehouse_id: number
    to_warehouse_id?: number
    partner_id?: number
    move_date?: string
    note?: string
    lines: { product_id: number; qty: number; unit_price?: number }[]
  }) => j<{ ok?: boolean; move?: Move; error?: string }>('/api/moves', { method: 'POST', body: JSON.stringify(body) }),
  moveDelete: (id: number) => j<{ ok?: boolean; deleted?: string; error?: string }>(`/api/moves/${id}/delete`, { method: 'POST' }),
  stock: (q: { product_id?: number; warehouse_id?: number } = {}) => {
    const p = new URLSearchParams()
    if (q.product_id) p.set('product_id', String(q.product_id))
    if (q.warehouse_id) p.set('warehouse_id', String(q.warehouse_id))
    return j<{ stock: StockRow[]; total_value: number }>(`/api/stock?${p}`)
  },
  stockCard: (product_id: number, warehouse_id?: number) => {
    const p = new URLSearchParams({ product_id: String(product_id) })
    if (warehouse_id) p.set('warehouse_id', String(warehouse_id))
    return j<{ card: CardRow[]; error?: string }>(`/api/stock/card?${p}`)
  },
  inout: (months = 12) => j<{ inout: InoutRow[] }>(`/api/report/inout?months=${months}`),
  insight: (days = 90) => j<Insight>(`/api/insight/products?days=${days}`),
  analyze: (question = '') =>
    j<{ analysis: string; model: string }>('/api/analyze', { method: 'POST', body: JSON.stringify({ question }) }),
  analyzeProducts: (question = '', days = 90) =>
    j<{ analysis: string; model: string }>('/api/analyze/products', { method: 'POST', body: JSON.stringify({ question, days }) }),
  activity: () => j<{ activity: any[] }>('/api/activity'),
}
