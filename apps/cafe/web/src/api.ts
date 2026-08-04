// Tiny fetch wrapper for the Cafe app REST API (served from the same origin in
// production; proxied to :4700 in `npm run dev`).

async function j<T = any>(url: string, init?: RequestInit): Promise<T> {
  const r = await fetch(url, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  return (await r.json()) as T
}

// ------------------------------------------------------------------- types

export interface Ingredient {
  id: number
  name: string
  unit: string
  min_stock: number
  avg_cost: number
  note: string
  status: string
  stock: number
  stock_display: string
  stock_value: number
  low_stock: boolean
  avg_daily_14d: number
  days_left: number | null
}

export interface CardRow {
  date: string
  kind: string
  qty: number
  unit_cost: number
  balance: number
  ref: string
  note: string
}

export interface StockCard {
  ingredient?: { id: number; name: string; unit: string }
  opening?: number
  closing?: number
  rows?: CardRow[]
  error?: string
}

export interface PurchaseLine {
  id: number
  ingredient_id: number
  name: string
  unit: string
  qty: number
  qty_input: number
  unit_input: string
  unit_price: number
  amount: number
}

export interface Purchase {
  id: number
  code: string
  supplier: string
  purchase_date: string
  note: string
  total: number
  line_count?: number
  lines?: PurchaseLine[]
}

export interface RecipeLine {
  id: number
  ingredient_id: number
  name: string
  unit: string
  qty: number
  unit_cost: number
  cost: number
}

export interface MenuItem {
  id: number
  name: string
  category: string
  price: number
  instructions: string
  status: string
  cost: number
  margin: number
  margin_pct: number
  has_recipe: boolean
  recipe?: RecipeLine[]
}

export interface SaleLine {
  id: number
  menu_id: number
  menu_name: string
  qty: number
  unit_price: number
  amount: number
  cogs: number
}

export interface Sale {
  id: number
  code: string
  sale_date: string
  note: string
  total: number
  cogs: number
  profit: number
  status: string
  items?: string
  lines?: SaleLine[]
  warnings?: string[]
}

export interface DayPoint {
  date: string
  revenue: number
  profit: number
}

export interface Dashboard {
  today: { date: string; orders: number; revenue: number; cogs: number; profit: number }
  last7: { from: string; orders: number; revenue: number; profit: number }
  revenue_14d: DayPoint[]
  top_items_7d: { name: string; qty: number; revenue: number }[]
  low_stock: { id: number; name: string; unit: string; stock: number; stock_display: string; min_stock: number; days_left: number | null }[]
  negative_stock: { id: number; name: string; stock_display: string }[]
  no_recipe: { id: number; name: string }[]
  stock_value: number
  menu_count: number
  ingredient_count: number
  recent_sales: Sale[]
  alerts: string[]
}

export interface RevenueReport {
  group_by: string
  from: string
  to: string
  rows: any[]
  orders: number
  items_sold: number
  revenue: number
  cogs: number
  profit: number
}

export interface PurchaseReport {
  group_by: string
  from: string
  to: string
  rows: any[]
  purchase_count: number
  total_amount: number
}

export interface ForecastItem {
  menu_id: number
  name: string
  price: number
  forecast_qty: number
  forecast_revenue: number
  forecast_profit: number
  per_day: number[]
}

export interface ForecastSales {
  days: number
  future: DayPoint[]
  items: ForecastItem[]
  total_revenue: number
  total_profit: number
  note: string
}

export interface ForecastIngRow {
  ingredient_id: number
  name: string
  unit: string
  stock: number
  stock_display: string
  min_stock: number
  avg_cost: number
  avg_daily_14d: number
  forecast_usage: number
  usage_display: string
  days_left: number | null
  stockout_date: string | null
  need: number
  need_display: string
  est_cost: number
}

export interface ForecastIngredients {
  days: number
  rows: ForecastIngRow[]
  note: string
}

export interface PurchaseSuggest {
  days: number
  rows: ForecastIngRow[]
  est_total_cost: number
  note: string
}

export interface Status {
  ok: boolean
  menu_count: number
  ingredient_count: number
  today_orders: number
  today_revenue: number
  low_stock_count: number
  stock_value: number
}

// -------------------------------------------------------------- formatters

export const MOVE_KIND_LABELS: Record<string, string> = {
  purchase: 'Nhập hàng',
  sale: 'Bán hàng',
  adjust: 'Điều chỉnh',
  void: 'Hoàn kho (huỷ đơn)',
}

export const MOVE_KIND_COLORS: Record<string, string> = {
  purchase: 'green',
  sale: 'volcano',
  adjust: 'gold',
  void: 'blue',
}

export const BASE_UNITS = ['g', 'ml', 'cái']

/** Đơn vị được phép khai trên phiếu nhập theo đơn vị gốc. */
export const PURCHASE_UNITS: Record<string, string[]> = {
  g: ['g', 'kg'],
  ml: ['ml', 'l'],
  'cái': ['cái'],
}

export function fmtMoney(v: number | null | undefined): string {
  if (v === null || v === undefined) return '—'
  return `${new Intl.NumberFormat('vi-VN', { maximumFractionDigits: 0 }).format(v)} đ`
}

export function fmtQty(v: number | null | undefined): string {
  if (v === null || v === undefined) return '—'
  return new Intl.NumberFormat('vi-VN', { maximumFractionDigits: 3 }).format(v)
}

export function fmtDate(iso: string | undefined): string {
  if (!iso) return '—'
  const [y, m, d] = iso.split('-')
  return d && m && y ? `${d}/${m}/${y}` : iso
}

export function todayISO(): string {
  const d = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

export function addDaysISO(iso: string, days: number): string {
  const d = new Date(`${iso}T00:00:00`)
  d.setDate(d.getDate() + days)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

// -------------------------------------------------------------------- api

export const api = {
  status: () => j<Status>('/api/status'),
  dashboard: () => j<Dashboard>('/api/dashboard'),

  ingredients: (q: { q?: string; low_only?: boolean; include_inactive?: boolean } = {}) => {
    const p = new URLSearchParams()
    if (q.q) p.set('q', q.q)
    if (q.low_only) p.set('low_only', 'true')
    if (q.include_inactive) p.set('include_inactive', 'true')
    return j<{ ingredients: Ingredient[] }>(`/api/ingredients?${p}`)
  },
  ingredientAdd: (body: { name: string; unit: string; min_stock?: number; note?: string }) =>
    j<{ ok?: boolean; ingredient?: Ingredient; error?: string }>('/api/ingredients', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  ingredientUpdate: (id: number, patch: Record<string, unknown>) =>
    j<{ ok?: boolean; ingredient?: Ingredient; error?: string }>(`/api/ingredients/${id}`, {
      method: 'POST',
      body: JSON.stringify(patch),
    }),
  stockAdjust: (body: { ingredient_id: number; delta?: number; set_qty?: number; reason?: string }) =>
    j<{ ok?: boolean; stock?: number; stock_display?: string; error?: string }>('/api/stock/adjust', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  stockCard: (id: number, limit = 200) => j<StockCard>(`/api/ingredients/${id}/card?limit=${limit}`),

  purchases: (q: { from?: string; to?: string; supplier?: string; limit?: number } = {}) => {
    const p = new URLSearchParams()
    if (q.from) p.set('from', q.from)
    if (q.to) p.set('to', q.to)
    if (q.supplier) p.set('supplier', q.supplier)
    if (q.limit) p.set('limit', String(q.limit))
    return j<{ purchases: Purchase[] }>(`/api/purchases?${p}`)
  },
  purchaseGet: (id: number) => j<{ purchase?: Purchase; error?: string }>(`/api/purchases/${id}`),
  purchaseCreate: (body: {
    supplier?: string
    date?: string
    note?: string
    lines: { ingredient_id: number; qty: number; unit: string; unit_price: number }[]
  }) =>
    j<{ ok?: boolean; purchase?: Purchase; error?: string }>('/api/purchases', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  purchaseReport: (q: { from?: string; to?: string; group_by?: string } = {}) => {
    const p = new URLSearchParams()
    if (q.from) p.set('from', q.from)
    if (q.to) p.set('to', q.to)
    if (q.group_by) p.set('group_by', q.group_by)
    return j<PurchaseReport>(`/api/report/purchases?${p}`)
  },
  purchaseSuggest: (days = 7) => j<PurchaseSuggest>(`/api/purchase-suggest?days=${days}`),

  menu: (q: { q?: string; category?: string; include_inactive?: boolean } = {}) => {
    const p = new URLSearchParams()
    if (q.q) p.set('q', q.q)
    if (q.category) p.set('category', q.category)
    if (q.include_inactive) p.set('include_inactive', 'true')
    return j<{ menu: MenuItem[] }>(`/api/menu?${p}`)
  },
  menuGet: (id: number) => j<{ menu?: MenuItem; error?: string }>(`/api/menu/${id}`),
  menuAdd: (body: { name: string; category?: string; price: number; instructions?: string }) =>
    j<{ ok?: boolean; menu?: MenuItem; error?: string }>('/api/menu', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  menuUpdate: (id: number, patch: Record<string, unknown>) =>
    j<{ ok?: boolean; menu?: MenuItem; error?: string }>(`/api/menu/${id}`, {
      method: 'POST',
      body: JSON.stringify(patch),
    }),
  recipeSet: (menuId: number, items: { ingredient_id: number; qty: number }[]) =>
    j<{ ok?: boolean; menu?: MenuItem; error?: string }>(`/api/menu/${menuId}/recipe`, {
      method: 'POST',
      body: JSON.stringify({ items }),
    }),

  sales: (q: { from?: string; to?: string; status?: string; limit?: number } = {}) => {
    const p = new URLSearchParams()
    if (q.from) p.set('from', q.from)
    if (q.to) p.set('to', q.to)
    if (q.status) p.set('status', q.status)
    if (q.limit) p.set('limit', String(q.limit))
    return j<{ sales: Sale[] }>(`/api/sales?${p}`)
  },
  saleGet: (id: number) => j<{ sale?: Sale; error?: string }>(`/api/sales/${id}`),
  saleCreate: (body: {
    date?: string
    note?: string
    lines: { menu_id: number; qty: number; unit_price?: number }[]
  }) =>
    j<{ ok?: boolean; sale?: Sale; error?: string }>('/api/sales', {
      method: 'POST',
      body: JSON.stringify(body),
    }),
  saleVoid: (id: number, reason = '') =>
    j<{ ok?: boolean; sale?: Sale; error?: string }>(`/api/sales/${id}/void`, {
      method: 'POST',
      body: JSON.stringify({ reason }),
    }),

  revenueReport: (q: { from?: string; to?: string; group_by?: string } = {}) => {
    const p = new URLSearchParams()
    if (q.from) p.set('from', q.from)
    if (q.to) p.set('to', q.to)
    if (q.group_by) p.set('group_by', q.group_by)
    return j<RevenueReport>(`/api/report/revenue?${p}`)
  },
  inventoryReport: () =>
    j<{ items: Ingredient[]; total_value: number; low_count: number; negative: Ingredient[] }>(
      '/api/report/inventory',
    ),
  forecastSales: (days = 7) => j<ForecastSales>(`/api/forecast/sales?days=${days}`),
  forecastIngredients: (days = 7) => j<ForecastIngredients>(`/api/forecast/ingredients?days=${days}`),

  analyze: (question = '') =>
    j<{ analysis: string; model: string }>('/api/analyze', {
      method: 'POST',
      body: JSON.stringify({ question }),
    }),
  menuSuggest: (idea = '', target_margin_pct?: number) =>
    j<{ suggestion: string; model: string }>('/api/menu-suggest', {
      method: 'POST',
      body: JSON.stringify({ idea, target_margin_pct }),
    }),
}
