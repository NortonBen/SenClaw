import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  App as AntApp,
  AutoComplete,
  Button,
  Card,
  ConfigProvider,
  Divider,
  Drawer,
  Input,
  Modal,
  Segmented,
  Select,
  Space,
  Spin,
  Statistic,
  Switch,
  Tabs,
  Tag,
  Timeline,
  Tooltip,
  theme as antTheme,
} from 'antd'
import {
  BulbOutlined,
  DeleteOutlined,
  DownloadOutlined,
  EditOutlined,
  MoonOutlined,
  PlusOutlined,
  SearchOutlined,
  SettingOutlined,
  SplitCellsOutlined,
  SunOutlined,
} from '@ant-design/icons'
import {
  api,
  fmtDate,
  fmtDateTime,
  formatMoney,
  hueFromName,
  initials,
  type ActivityItem,
  type Customer,
  type CustomerChannel,
  type CustomerDetail,
  type CustomerInput,
  type Deal,
  type GraphNode,
  type Interaction,
  type Relationship,
  type SearchHit,
  type Stats,
  type Task,
  type Upcoming,
} from './api'

type View = 'dashboard' | 'customers' | 'pipeline' | 'tasks' | 'activity' | 'network'

const STAGE_ORDER = ['qualifying', 'proposal', 'negotiation', 'won', 'lost']
const STAGE_LABELS: Record<string, string> = {
  qualifying: 'Đang xác định',
  proposal: 'Đã báo giá',
  negotiation: 'Đàm phán',
  won: 'Thắng',
  lost: 'Đã mất',
}

/// 13 vai trò — richer than the old status enum. Each has a label, a color for
/// the graph nodes, and an emoji so the custom select trigger reads at a glance.
type RoleMeta = { label: string; short: string; icon: string; color: string }
const ROLE_META: Record<string, RoleMeta> = {
  lead:     { label: 'Đầu mối',        short: 'Lead',      icon: '🌱', color: '#6b7280' },
  prospect: { label: 'Tiềm năng',      short: 'Prospect',  icon: '🔍', color: '#3b82f6' },
  customer: { label: 'Khách hàng',     short: 'Customer',  icon: '🤝', color: '#10b981' },
  vip:      { label: 'VIP',            short: 'VIP',       icon: '⭐', color: '#a855f7' },
  contact:  { label: 'Người liên hệ',  short: 'Contact',   icon: '👤', color: '#6366f1' },
  partner:  { label: 'Đối tác',        short: 'Partner',   icon: '🤝', color: '#0ea5e9' },
  referrer: { label: 'Người giới thiệu', short: 'Referrer', icon: '📣', color: '#eab308' },
  supplier: { label: 'Nhà cung cấp',   short: 'Supplier',  icon: '📦', color: '#f97316' },
  investor: { label: 'Nhà đầu tư',     short: 'Investor',  icon: '💰', color: '#14b8a6' },
  employee: { label: 'Nhân viên',      short: 'Employee',  icon: '🧑‍💼', color: '#8b5cf6' },
  former:   { label: 'Khách cũ',       short: 'Former',    icon: '🕰', color: '#94a3b8' },
  paused:   { label: 'Tạm dừng',       short: 'Paused',    icon: '⏸', color: '#b45309' },
  lost:     { label: 'Đã mất',         short: 'Lost',      icon: '❌', color: '#ef4444' },
}
const ROLE_ORDER = [
  'lead', 'prospect', 'customer', 'vip',
  'contact', 'partner', 'referrer', 'supplier', 'investor', 'employee',
  'former', 'paused', 'lost',
]
function roleMeta(role: string): RoleMeta {
  return ROLE_META[role] ?? { label: role, short: role, icon: '•', color: '#6b7280' }
}

/// Every extra contact channel kind — phone extras + socials + website + email
/// extras. `href(value)` builds the clickable URL/tel/mailto so the UI can
/// jump straight into the network from the customer detail card.
type ChannelMeta = {
  label: string
  icon: string
  color: string
  placeholder: string
  href: (value: string) => string
}
const CHANNEL_META: Record<string, ChannelMeta> = {
  phone: { label: 'Điện thoại', icon: '📞', color: '#0ea5e9', placeholder: '0900…', href: (v) => `tel:${v.replace(/\s/g, '')}` },
  email: { label: 'Email', icon: '✉️', color: '#6366f1', placeholder: 'user@example.com', href: (v) => `mailto:${v}` },
  zalo: { label: 'Zalo', icon: '💬', color: '#0068ff', placeholder: '0900… hoặc user', href: (v) => (/^\d+$/.test(v.replace(/\s/g, '')) ? `https://zalo.me/${v.replace(/\s/g, '')}` : `https://zalo.me/${v.replace(/^@/, '')}`) },
  facebook: { label: 'Facebook', icon: '📘', color: '#1877f2', placeholder: 'username hoặc URL', href: (v) => (v.startsWith('http') ? v : `https://facebook.com/${v.replace(/^@/, '')}`) },
  messenger: { label: 'Messenger', icon: '💌', color: '#00b2ff', placeholder: 'username', href: (v) => (v.startsWith('http') ? v : `https://m.me/${v.replace(/^@/, '')}`) },
  instagram: { label: 'Instagram', icon: '📷', color: '#e4405f', placeholder: 'username', href: (v) => (v.startsWith('http') ? v : `https://instagram.com/${v.replace(/^@/, '')}`) },
  linkedin: { label: 'LinkedIn', icon: '💼', color: '#0a66c2', placeholder: 'URL profile', href: (v) => (v.startsWith('http') ? v : `https://linkedin.com/in/${v.replace(/^@/, '')}`) },
  x: { label: 'X (Twitter)', icon: '🐦', color: '#000000', placeholder: 'username', href: (v) => (v.startsWith('http') ? v : `https://x.com/${v.replace(/^@/, '')}`) },
  tiktok: { label: 'TikTok', icon: '🎵', color: '#000000', placeholder: 'username', href: (v) => (v.startsWith('http') ? v : `https://tiktok.com/@${v.replace(/^@/, '')}`) },
  youtube: { label: 'YouTube', icon: '▶️', color: '#ff0000', placeholder: '@channel hoặc URL', href: (v) => (v.startsWith('http') ? v : `https://youtube.com/${v.startsWith('@') ? v : '@' + v}`) },
  github: { label: 'GitHub', icon: '🐙', color: '#181717', placeholder: 'username', href: (v) => (v.startsWith('http') ? v : `https://github.com/${v.replace(/^@/, '')}`) },
  telegram: { label: 'Telegram', icon: '✈️', color: '#26a5e4', placeholder: '@username', href: (v) => (v.startsWith('http') ? v : `https://t.me/${v.replace(/^@/, '')}`) },
  whatsapp: { label: 'WhatsApp', icon: '📱', color: '#25d366', placeholder: '84900…', href: (v) => `https://wa.me/${v.replace(/[^\d]/g, '')}` },
  signal: { label: 'Signal', icon: '🔒', color: '#3a76f0', placeholder: '+84…', href: (v) => `https://signal.me/#p/${v.replace(/\s/g, '')}` },
  line: { label: 'LINE', icon: '💚', color: '#00c300', placeholder: 'lineid', href: (v) => (v.startsWith('http') ? v : `https://line.me/ti/p/${v.replace(/^@/, '')}`) },
  wechat: { label: 'WeChat', icon: '🇨🇳', color: '#07c160', placeholder: 'wechatid', href: (v) => `weixin://dl/chat?${v.replace(/^@/, '')}` },
  skype: { label: 'Skype', icon: '☁️', color: '#00aff0', placeholder: 'skypeid', href: (v) => `skype:${v}?chat` },
  viber: { label: 'Viber', icon: '🍇', color: '#7360f2', placeholder: '84900…', href: (v) => `viber://chat?number=%2B${v.replace(/[^\d]/g, '')}` },
  discord: { label: 'Discord', icon: '🎮', color: '#5865f2', placeholder: 'username', href: (v) => (v.startsWith('http') ? v : `https://discord.com/users/${v.replace(/^@/, '')}`) },
  website: { label: 'Website', icon: '🌐', color: '#6b7280', placeholder: 'https://…', href: (v) => (v.startsWith('http') ? v : `https://${v}`) },
}
const CHANNEL_KINDS = Object.keys(CHANNEL_META)
function channelMeta(kind: string): ChannelMeta {
  return CHANNEL_META[kind] ?? { label: kind, icon: '🔗', color: '#6b7280', placeholder: 'value', href: (v) => v }
}

const REL_LABELS: Record<string, string> = {
  referred_by: 'được giới thiệu bởi',
  introduced_by: 'do người này giới thiệu',
  colleague_of: 'đồng nghiệp của',
  spouse_of: 'vợ / chồng của',
  family_of: 'gia đình của',
  friend_of: 'bạn của',
  reports_to: 'cấp dưới của',
  partner_of: 'đối tác với',
  supplier_of: 'nhà cung cấp của',
  competitor_of: 'đối thủ của',
  contact_of: 'liên hệ của',
}
const REL_ORDER = Object.keys(REL_LABELS)

const KIND_META: Record<string, { icon: string; label: string }> = {
  call: { icon: '📞', label: 'Cuộc gọi' },
  email: { icon: '✉️', label: 'Email' },
  meeting: { icon: '🤝', label: 'Gặp mặt' },
  note: { icon: '📝', label: 'Ghi chú' },
  task: { icon: '✅', label: 'Việc' },
  profile_update: { icon: '✏️', label: 'Sửa hồ sơ' },
  deal_update: { icon: '💼', label: 'Sửa deal' },
}

type Theme = 'light' | 'dark' | null

function readTheme(): Theme {
  try {
    const s = localStorage.getItem('crm-theme')
    return s === 'light' || s === 'dark' ? s : null
  } catch {
    return null
  }
}

function detectSystemTheme(): 'light' | 'dark' {
  return window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches
    ? 'dark'
    : 'light'
}

export default function App() {
  const [theme, setTheme] = useState<Theme>(readTheme)
  const [systemTheme, setSystemTheme] = useState<'light' | 'dark'>(detectSystemTheme)
  const effectiveTheme: 'light' | 'dark' = theme ?? systemTheme

  useEffect(() => {
    const root = document.documentElement
    if (theme === null) {
      root.removeAttribute('data-theme')
      try {
        localStorage.removeItem('crm-theme')
      } catch {}
    } else {
      root.setAttribute('data-theme', theme)
      try {
        localStorage.setItem('crm-theme', theme)
      } catch {}
    }
  }, [theme])

  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const onChange = (e: MediaQueryListEvent) => setSystemTheme(e.matches ? 'dark' : 'light')
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])

  return (
    <ConfigProvider
      theme={{
        algorithm: effectiveTheme === 'dark' ? antTheme.darkAlgorithm : antTheme.defaultAlgorithm,
        token: {
          colorPrimary: effectiveTheme === 'dark' ? '#818cf8' : '#4f46e5',
          borderRadius: 8,
        },
      }}
    >
      <AntApp>
        <AppInner theme={theme} setTheme={setTheme} />
      </AntApp>
    </ConfigProvider>
  )
}

/// Every piece of Network view state that we want to persist between tab
/// switches AND across reloads (via /state/graph in the DB). Kept in the App
/// scope so switching tabs doesn't unmount and lose it.
type NetState = {
  nameFilter: string
  roleFilter: string[]
  kindFilter: string[]
  focus: { id: number; hops: number } | null
  pathState: { from?: number; to?: number; ids: number[]; hops: number } | null
  common: {
    focus_id: number
    themes: Array<{ theme: string; why: string; customer_ids: number[] }>
    highlight_ids: number[]
  } | null
  aiPath: {
    from: number
    to: number
    summary: string
    connections: Array<{ type: string; detail: string; strength: string }>
    bfs_path_names: string[] | null
  } | null
}
const NET_DEFAULT: NetState = {
  nameFilter: '',
  roleFilter: [],
  kindFilter: [],
  focus: null,
  pathState: null,
  common: null,
  aiPath: null,
}

type CrmSettings = {
  splitRight: View | null
  syncSpaceCalendar: boolean
  lastSyncedAt: number | null
}
const SETTINGS_DEFAULT: CrmSettings = {
  splitRight: null,
  syncSpaceCalendar: true,
  lastSyncedAt: null,
}

function AppInner({ theme, setTheme }: { theme: Theme; setTheme: (t: Theme) => void }) {
  const [view, setView] = useState<View>('dashboard')
  const viewRef = useRef<View>(view)
  useEffect(() => {
    viewRef.current = view
  }, [view])
  const [net, setNet] = useState<NetState>(NET_DEFAULT)
  const [settings, setSettings] = useState<CrmSettings>(SETTINGS_DEFAULT)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const settingsHydrated = useRef(false)
  useEffect(() => {
    api.getState<CrmSettings>('settings').then((v) => {
      if (v) setSettings({ ...SETTINGS_DEFAULT, ...v })
      settingsHydrated.current = true
    })
  }, [])
  useEffect(() => {
    if (!settingsHydrated.current) return
    const h = setTimeout(() => api.putState('settings', settings).catch(() => {}), 400)
    return () => clearTimeout(h)
  }, [settings])
  // AI busy tracker — a single string that names the current operation ('common' | 'ai_path' | null).
  const [netBusy, setNetBusy] = useState<null | 'common' | 'ai_path'>(null)
  const notify = AntApp.useApp().notification

  // Hydrate from DB on first load. If nothing stored, keep defaults.
  const netHydrated = useRef(false)
  useEffect(() => {
    api.getState<NetState>('graph').then((v) => {
      if (v) setNet({ ...NET_DEFAULT, ...v })
      netHydrated.current = true
    })
  }, [])

  // Debounced persist to DB whenever state changes (after hydration).
  useEffect(() => {
    if (!netHydrated.current) return
    const h = setTimeout(() => {
      api.putState('graph', net).catch(() => {})
    }, 400)
    return () => clearTimeout(h)
  }, [net])
  const [stats, setStats] = useState<Stats | null>(null)
  const [customers, setCustomers] = useState<Customer[]>([])
  const [allTags, setAllTags] = useState<string[]>([])
  const [q, setQ] = useState('')
  const [tag, setTag] = useState<string | null>(null)
  const [roleFilter, setRoleFilter] = useState<string | null>(null)
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [detail, setDetail] = useState<CustomerDetail | null>(null)
  const [showNew, setShowNew] = useState(false)
  const [err, setErr] = useState('')

  const refreshList = useCallback(async () => {
    try {
      const [rows, tags, s] = await Promise.all([
        api.listCustomers({ q, tag: tag ?? undefined, role: roleFilter ?? undefined, limit: 200 }),
        api.tags(),
        api.stats(),
      ])
      setCustomers(rows)
      setAllTags(tags)
      setStats(s)
      if (!selectedId && rows.length) setSelectedId(rows[0]!.id)
    } catch (e) {
      setErr(String(e))
    }
  }, [q, tag, roleFilter, selectedId])

  const refreshDetail = useCallback(async () => {
    if (!selectedId) {
      setDetail(null)
      return
    }
    try {
      setDetail(await api.getCustomer(selectedId))
    } catch (e) {
      setDetail(null)
      setErr(String(e))
    }
  }, [selectedId])

  useEffect(() => {
    refreshList()
  }, [refreshList])

  useEffect(() => {
    refreshDetail()
  }, [refreshDetail])

  async function onCreate(input: CustomerInput, channels?: Array<{ kind: string; value: string; label?: string }>) {
    const c = await api.createCustomer(input)
    // Bulk-add any pre-collected channels alongside the new customer — parallel
    // POSTs so the modal closes quickly even for a big social-network haul.
    if (channels && channels.length) {
      await Promise.all(
        channels
          .filter((ch) => ch.value.trim())
          .map((ch) => api.addChannel(c.id, ch).catch(() => null)),
      )
    }
    setShowNew(false)
    setSelectedId(c.id)
    await refreshList()
  }

  async function onPatch(patch: CustomerInput) {
    if (!selectedId) return
    await api.updateCustomer(selectedId, patch)
    await Promise.all([refreshList(), refreshDetail()])
  }

  async function onDelete() {
    if (!selectedId) return
    if (!confirm('Xoá khách hàng này? Hành động không thể hoàn tác.')) return
    await api.deleteCustomer(selectedId)
    setSelectedId(null)
    setDetail(null)
    await refreshList()
  }

  return (
    <div className="app">
      <header className="topbar slim">
        <div className="brand">
          <span className="logo">👥</span>
          <div>
            SenClaw CRM
            <small>Personal CRM cho SenClaw</small>
          </div>
        </div>
        <GlobalSearch
          onPickCustomer={(id) => {
            setView('customers')
            setSelectedId(id)
          }}
        />
        <Tooltip title="Chia đôi màn hình">
          <Button
            shape="circle"
            icon={<SplitCellsOutlined />}
            type={settings.splitRight ? 'primary' : 'default'}
            onClick={() =>
              setSettings((s) => ({ ...s, splitRight: s.splitRight ? null : 'tasks' }))
            }
          />
        </Tooltip>
        <Tooltip title="Thiết lập">
          <Button shape="circle" icon={<SettingOutlined />} onClick={() => setSettingsOpen(true)} />
        </Tooltip>
        <ThemeToggle theme={theme} setTheme={setTheme} />
      </header>

      <Tabs
        activeKey={view}
        onChange={(k) => setView(k as View)}
        className="viewnav-antd"
        items={[
          { key: 'dashboard', label: '📊 Dashboard' },
          { key: 'customers', label: '👥 Danh sách' },
          { key: 'network', label: '🕸 Mạng lưới' },
          { key: 'pipeline', label: '🗂 Pipeline' },
          { key: 'tasks', label: '✅ Việc & Nhắc' },
          { key: 'activity', label: '🕐 Hoạt động' },
        ]}
      />

      {err && (
        <div className="err">
          {err}
          <button onClick={() => setErr('')}>×</button>
        </div>
      )}

      {view === 'dashboard' && (
        <DashboardView
          stats={stats}
          onOpenNew={() => setShowNew(true)}
          onPickCustomer={(id) => {
            setView('customers')
            setSelectedId(id)
          }}
          onGoto={(v) => setView(v)}
        />
      )}
      {/* NetworkView is always rendered so hydrated state + running LLM tasks
          survive tab switches; visibility is toggled via the wrapper div. */}
      <div style={{ display: view === 'network' ? 'block' : 'none' }}>
        <NetworkView
          net={net}
          setNet={setNet}
          busy={netBusy}
          setBusy={setNetBusy}
          onBackgroundResult={(msg) => {
            // Read the CURRENT tab via a ref — the closure captured `view` at
            // click time, but the user may have switched tabs during the LLM
            // roundtrip. Only fire the toast if they're no longer on Network.
            if (viewRef.current !== 'network') {
              notify.info({
                message: 'Kết quả AI đã sẵn sàng',
                description: msg + ' — bấm để xem trong Mạng lưới.',
                onClick: () => setView('network'),
                duration: 6,
              })
            }
          }}
          onPickCustomer={(id) => {
            setView('customers')
            setSelectedId(id)
          }}
        />
      </div>
      {view === 'pipeline' && <PipelineView />}
      {view === 'tasks' && <TasksView customers={customers} />}
      {view === 'activity' && <ActivityView onPickCustomer={(id) => { setView('customers'); setSelectedId(id) }} />}

      {/* Split-screen right panel — an independent second view with its own
          full tab bar, chosen independently from the left. Uses position: fixed
          so it doesn't disrupt the main layout. */}
      {settings.splitRight && (
        <aside className="split-side-panel">
          <Tabs
            activeKey={settings.splitRight}
            onChange={(k) => setSettings((s) => ({ ...s, splitRight: k as View }))}
            className="viewnav-antd split-side-tabs"
            tabBarExtraContent={
              <Button size="small" onClick={() => setSettings((s) => ({ ...s, splitRight: null }))}>
                Đóng ×
              </Button>
            }
            items={[
              { key: 'dashboard', label: '📊 Dashboard' },
              { key: 'customers', label: '👥 Danh sách' },
              { key: 'network', label: '🕸 Mạng lưới' },
              { key: 'pipeline', label: '🗂 Pipeline' },
              { key: 'tasks', label: '✅ Việc & Nhắc' },
              { key: 'activity', label: '🕐 Hoạt động' },
            ]}
          />
          <div className="split-side-body">
            {settings.splitRight === 'dashboard' && (
              <DashboardView
                stats={stats}
                onOpenNew={() => setShowNew(true)}
                onPickCustomer={(id) => {
                  setSettings((s) => ({ ...s, splitRight: 'customers' }))
                  setSelectedId(id)
                }}
                onGoto={(v) => setSettings((s) => ({ ...s, splitRight: v }))}
              />
            )}
            {settings.splitRight === 'customers' && (
              <div className="layout-split">
                <aside className="sidebar sidebar-split">
                  <div className="sidebar-actions">
                    <Button type="primary" icon={<PlusOutlined />} block onClick={() => setShowNew(true)}>
                      Khách mới
                    </Button>
                  </div>
                  <div className="list">
                    {customers.length === 0 && (
                      <div className="empty">Chưa có khách phù hợp.</div>
                    )}
                    {customers.map((c) => (
                      <CustomerRow
                        key={c.id}
                        c={c}
                        selected={c.id === selectedId}
                        onPick={() => setSelectedId(c.id)}
                      />
                    ))}
                  </div>
                </aside>
                <main className="detail detail-split">
                  {detail ? (
                    <CustomerDetailView
                      detail={detail}
                      onPatch={onPatch}
                      onDelete={onDelete}
                      onInteractionsChanged={refreshDetail}
                    />
                  ) : (
                    <div className="empty big">Chọn một khách để xem chi tiết.</div>
                  )}
                </main>
              </div>
            )}
            {settings.splitRight === 'network' && (
              <NetworkView
                net={net}
                setNet={setNet}
                busy={netBusy}
                setBusy={setNetBusy}
                onBackgroundResult={() => {}}
                onPickCustomer={(id) => {
                  setSettings((s) => ({ ...s, splitRight: 'customers' }))
                  setSelectedId(id)
                }}
              />
            )}
            {settings.splitRight === 'tasks' && <TasksView customers={customers} />}
            {settings.splitRight === 'activity' && (
              <ActivityView
                onPickCustomer={(id) => {
                  setSettings((s) => ({ ...s, splitRight: 'customers' }))
                  setSelectedId(id)
                }}
              />
            )}
            {settings.splitRight === 'pipeline' && <PipelineView />}
          </div>
        </aside>
      )}

      <SettingsModal
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        settings={settings}
        setSettings={setSettings}
      />

      {view === 'customers' && (
      <div className="layout">
        <aside className="sidebar">
          <div className="sidebar-actions">
            <Button type="primary" icon={<PlusOutlined />} block onClick={() => setShowNew(true)}>
              Khách mới
            </Button>
          </div>
          <div className="search">
            <input
              type="search"
              placeholder="Tìm tên / email / SĐT / công ty…"
              value={q}
              onChange={(e) => setQ(e.target.value)}
            />
          </div>
          <div className="chips">
            <span className={'chip' + (roleFilter === null ? ' on' : '')} onClick={() => setRoleFilter(null)}>
              Tất cả vai trò
            </span>
            {ROLE_ORDER.map((s) => (
              <span
                key={s}
                className={'chip role-chip' + (roleFilter === s ? ' on' : '')}
                onClick={() => setRoleFilter(roleFilter === s ? null : s)}
                style={roleFilter === s ? { background: roleMeta(s).color, borderColor: roleMeta(s).color } : undefined}
              >
                {roleMeta(s).icon} {roleMeta(s).label}
              </span>
            ))}
          </div>
          {allTags.length > 0 && (
            <div className="chips tags">
              <span className={'chip tag' + (tag === null ? ' on' : '')} onClick={() => setTag(null)}>
                Mọi tag
              </span>
              {allTags.map((t) => (
                <span
                  key={t}
                  className={'chip tag' + (tag === t ? ' on' : '')}
                  onClick={() => setTag(tag === t ? null : t)}
                >
                  #{t}
                </span>
              ))}
            </div>
          )}
          <div className="list">
            {customers.length === 0 && <div className="empty">Chưa có khách phù hợp.</div>}
            {customers.map((c) => (
              <CustomerRow
                key={c.id}
                c={c}
                selected={c.id === selectedId}
                onPick={() => setSelectedId(c.id)}
              />
            ))}
          </div>
        </aside>

        <main className="detail">
          {detail ? (
            <CustomerDetailView
              detail={detail}
              onPatch={onPatch}
              onDelete={onDelete}
              onInteractionsChanged={refreshDetail}
            />
          ) : (
            <div className="empty big">Chọn một khách hàng để xem chi tiết, hoặc thêm khách mới ở góc phải.</div>
          )}
        </main>
      </div>

      )}

      {showNew && <NewCustomerModal onClose={() => setShowNew(false)} onCreate={onCreate} />}
    </div>
  )
}

/// Compact money format for the top-bar tiles: 1.2M, 400k, 12.
function formatShortMoney(n: number): string {
  if (!n) return '0'
  if (Math.abs(n) >= 1_000_000_000) return `${(n / 1_000_000_000).toFixed(1)}B`
  if (Math.abs(n) >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (Math.abs(n) >= 1_000) return `${Math.round(n / 1_000)}k`
  return String(Math.round(n))
}

function Avatar({ name, url, size = 40 }: { name: string; url?: string; size?: number }) {
  const [broken, setBroken] = useState(false)
  const hue = hueFromName(name || '?')
  const style = {
    width: size,
    height: size,
    background: `hsl(${hue} 65% 55%)`,
    fontSize: size * 0.4,
  }
  if (url && !broken) {
    return (
      <img
        className="avatar"
        src={url}
        alt={name}
        style={{ width: size, height: size }}
        onError={() => setBroken(true)}
      />
    )
  }
  return (
    <div className="avatar fallback" style={style} aria-label={name}>
      {initials(name)}
    </div>
  )
}

function CustomerRow({ c, selected, onPick }: { c: Customer; selected: boolean; onPick: () => void }) {
  const rm = roleMeta(c.role)
  return (
    <div className={'row' + (selected ? ' sel' : '')} onClick={onPick}>
      <Avatar name={c.name} url={c.avatar_url} size={40} />
      <div className="rowbody">
        <div className="line1">
          <span className="name">{c.name}</span>
          <span className="role-badge small" style={{ color: rm.color, borderColor: rm.color + '55' }}>
            {rm.icon} {rm.short}
          </span>
        </div>
        <div className="line2">{c.company || c.email || c.phone || '—'}</div>
        <div className="line3">
          {c.tags.slice(0, 3).map((t) => (
            <span key={t} className="minitag">
              #{t}
            </span>
          ))}
          {c.interaction_count > 0 && (
            <span className="lastseen">
              {c.interaction_count} tương tác · {fmtDate(c.last_interaction_at)}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}

function CustomerDetailView({
  detail,
  onPatch,
  onDelete,
  onInteractionsChanged,
}: {
  detail: CustomerDetail
  onPatch: (patch: CustomerInput) => Promise<void>
  onDelete: () => Promise<void>
  onInteractionsChanged: () => Promise<void>
}) {
  const c = detail.customer
  const [editing, setEditing] = useState(false)

  return (
    <div>
      <div className="dhead">
        <Avatar name={c.name} url={c.avatar_url} size={72} />
        <div className="dheadmain">
          <div className="dtitle">
            <h1>{c.name}</h1>
            <RolePicker value={c.role} onChange={(role) => onPatch({ role })} />
          </div>
          <div className="dsub">
            {c.title && <span>{c.title}</span>}
            {c.title && c.company && <span className="dot">·</span>}
            {c.company && <span>{c.company}</span>}
          </div>
          <div className="dtags">
            {c.tags.map((t) => (
              <span key={t} className="tagchip">
                #{t}
              </span>
            ))}
          </div>
        </div>
        <Space>
          <Button icon={<EditOutlined />} onClick={() => setEditing((v) => !v)}>
            {editing ? 'Xong' : 'Sửa'}
          </Button>
          <Button danger icon={<DeleteOutlined />} onClick={onDelete}>
            Xoá
          </Button>
        </Space>
      </div>

      {editing ? (
        <EditForm customer={c} onPatch={onPatch} />
      ) : (
        <ContactCard c={c} />
      )}

      <AISection customerId={c.id} />
      <ChannelsSection customerId={c.id} />
      <RelationshipsSection customer={c} />
      <DealsSection customerId={c.id} />
      <TasksSection customerId={c.id} />
      <InteractionsSection
        customerId={c.id}
        interactions={detail.interactions}
        onChanged={onInteractionsChanged}
      />
    </div>
  )
}

function RolePicker({ value, onChange, style }: { value: string; onChange: (v: string) => void; style?: React.CSSProperties }) {
  const rm = roleMeta(value)
  return (
    <Select
      value={value}
      onChange={onChange}
      style={{ minWidth: 170, ...style }}
      variant="outlined"
      popupMatchSelectWidth={220}
      options={ROLE_ORDER.map((r) => {
        const m = roleMeta(r)
        return {
          value: r,
          label: (
            <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
              <span>{m.icon}</span>
              <span>{m.label}</span>
            </span>
          ),
        }
      })}
      // Render selected value with the role's colour band.
      labelRender={() => (
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, color: rm.color, fontWeight: 500 }}>
          <span>{rm.icon}</span>
          <span>{rm.label}</span>
        </span>
      )}
    />
  )
}

function ContactCard({ c }: { c: Customer }) {
  const rows: Array<[string, React.ReactNode]> = []
  if (c.email) rows.push(['Email', <a href={`mailto:${c.email}`}>{c.email}</a>])
  if (c.phone) rows.push(['Điện thoại', <a href={`tel:${c.phone}`}>{c.phone}</a>])
  if (c.company) rows.push(['Công ty', c.company])
  if (c.title) rows.push(['Chức danh', c.title])
  const rm = roleMeta(c.role)
  rows.push([
    'Vai trò',
    <span className="role-badge" style={{ color: rm.color, borderColor: rm.color + '55' }}>
      {rm.icon} {rm.label}
    </span>,
  ])
  if (c.source) rows.push(['Nguồn', c.source])
  if (c.address) rows.push(['Địa chỉ', c.address])
  if (c.birthday) rows.push(['Sinh nhật', c.birthday])
  rows.push(['Cập nhật', fmtDateTime(c.updated_at)])
  return (
    <div className="card">
      <div className="rows">
        {rows.map(([k, v]) => (
          <div className="row2" key={k}>
            <div className="k">{k}</div>
            <div className="v">{v}</div>
          </div>
        ))}
      </div>
      {c.notes && (
        <>
          <div className="section-title">📝 Ghi chú</div>
          <div className="notes">{c.notes}</div>
        </>
      )}
    </div>
  )
}

function EditForm({ customer, onPatch }: { customer: Customer; onPatch: (p: CustomerInput & { change_note?: string }) => Promise<void> }) {
  const [form, setForm] = useState<CustomerInput>({
    name: customer.name,
    email: customer.email,
    phone: customer.phone,
    company: customer.company,
    title: customer.title,
    avatar_url: customer.avatar_url,
    notes: customer.notes,
    tags: customer.tags,
    source: customer.source,
    address: customer.address,
    birthday: customer.birthday,
    role: customer.role,
  })
  const [tagInput, setTagInput] = useState('')
  const [changeNote, setChangeNote] = useState('')
  const [busy, setBusy] = useState(false)
  const fileRef = useRef<HTMLInputElement>(null)

  function set<K extends keyof CustomerInput>(k: K, v: CustomerInput[K]) {
    setForm((f) => ({ ...f, [k]: v }))
  }

  async function onAvatarFile(f: File | undefined) {
    if (!f) return
    if (f.size > 512 * 1024) {
      alert('Ảnh quá lớn — chọn ảnh dưới 512 KB (avatar được nhúng base64).')
      return
    }
    const dataUrl = await fileToDataUrl(f)
    set('avatar_url', dataUrl)
  }

  async function save() {
    setBusy(true)
    try {
      await onPatch({
        ...form,
        tags: form.tags,
        change_note: changeNote.trim() || undefined,
      })
      setChangeNote('')
    } finally {
      setBusy(false)
    }
  }

  const tags = form.tags ?? []

  return (
    <div className="card">
      <div className="edit-grid">
        <Field label="Tên">
          <input value={form.name ?? ''} onChange={(e) => set('name', e.target.value)} />
        </Field>
        <Field label="Email">
          <input value={form.email ?? ''} onChange={(e) => set('email', e.target.value)} placeholder="a@example.com" />
        </Field>
        <Field label="Điện thoại">
          <input value={form.phone ?? ''} onChange={(e) => set('phone', e.target.value)} />
        </Field>
        <Field label="Công ty">
          <input value={form.company ?? ''} onChange={(e) => set('company', e.target.value)} />
        </Field>
        <Field label="Chức danh">
          <input value={form.title ?? ''} onChange={(e) => set('title', e.target.value)} />
        </Field>
        <Field label="Nguồn">
          <input value={form.source ?? ''} onChange={(e) => set('source', e.target.value)} />
        </Field>
        <Field label="Địa chỉ">
          <input value={form.address ?? ''} onChange={(e) => set('address', e.target.value)} />
        </Field>
        <Field label="Sinh nhật">
          <input value={form.birthday ?? ''} onChange={(e) => set('birthday', e.target.value)} placeholder="YYYY-MM-DD" />
        </Field>
        <Field label="Avatar URL" full>
          <div className="avatar-row">
            <Avatar name={form.name ?? customer.name} url={form.avatar_url} size={48} />
            <input
              value={form.avatar_url ?? ''}
              onChange={(e) => set('avatar_url', e.target.value)}
              placeholder="https://… hoặc data:image/…"
            />
            <input
              ref={fileRef}
              type="file"
              accept="image/*"
              style={{ display: 'none' }}
              onChange={(e) => onAvatarFile(e.target.files?.[0])}
            />
            <button type="button" className="btn ghost" onClick={() => fileRef.current?.click()}>
              Tải ảnh…
            </button>
            {form.avatar_url && (
              <button type="button" className="btn ghost" onClick={() => set('avatar_url', '')}>
                Xoá
              </button>
            )}
          </div>
        </Field>
        <Field label="Tags" full>
          <div className="tag-editor">
            {tags.map((t) => (
              <span key={t} className="tagchip">
                #{t}
                <button
                  type="button"
                  onClick={() =>
                    set(
                      'tags',
                      tags.filter((x) => x !== t),
                    )
                  }
                >
                  ×
                </button>
              </span>
            ))}
            <input
              value={tagInput}
              onChange={(e) => setTagInput(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ',') {
                  e.preventDefault()
                  const t = tagInput.trim().replace(/^#/, '')
                  if (t && !tags.includes(t)) set('tags', [...tags, t])
                  setTagInput('')
                }
              }}
              placeholder="Nhập tag rồi Enter…"
            />
          </div>
        </Field>
        <Field label="Ghi chú" full>
          <textarea
            rows={4}
            value={form.notes ?? ''}
            onChange={(e) => set('notes', e.target.value)}
            placeholder="Sở thích, lịch sử, ghi chú nội bộ…"
          />
        </Field>
        <Field label="Lý do thay đổi (ghi vào hoạt động)" full>
          <input
            value={changeNote}
            onChange={(e) => setChangeNote(e.target.value)}
            placeholder="VD: 'Cập nhật SĐT mới sau đám cưới'"
          />
        </Field>
      </div>
      <div className="formactions">
        <button className="btn primary" onClick={save} disabled={busy}>
          {busy ? 'Đang lưu…' : 'Lưu (ghi vào hoạt động)'}
        </button>
      </div>
    </div>
  )
}

function Field({ label, children, full }: { label: string; children: React.ReactNode; full?: boolean }) {
  return (
    <label className={'field' + (full ? ' full' : '')}>
      <div className="lbl">{label}</div>
      {children}
    </label>
  )
}

function AISection({ customerId }: { customerId: number }) {
  const [summary, setSummary] = useState<{ text: string; model: string } | null>(null)
  const [next, setNext] = useState<{ text: string; model: string } | null>(null)
  const [busy, setBusy] = useState<'sum' | 'next' | null>(null)
  const [err, setErr] = useState('')

  async function runSum() {
    setBusy('sum')
    setErr('')
    try {
      setSummary(await api.summarize(customerId))
    } catch (e) {
      setErr('Cần bật LLM trong daemon SenClaw để dùng AI briefing. ' + String(e))
    } finally {
      setBusy(null)
    }
  }
  async function runNext() {
    setBusy('next')
    setErr('')
    try {
      setNext(await api.nextStep(customerId))
    } catch (e) {
      setErr('Cần bật LLM trong daemon SenClaw để đề xuất bước tiếp. ' + String(e))
    } finally {
      setBusy(null)
    }
  }
  return (
    <div className="card ai">
      <div className="section-title">✨ AI briefing</div>
      <div className="inline">
        <button className="btn primary" onClick={runSum} disabled={busy === 'sum'}>
          {busy === 'sum' ? 'Đang tóm tắt…' : 'Tóm tắt hồ sơ'}
        </button>
        <button className="btn" onClick={runNext} disabled={busy === 'next'}>
          {busy === 'next' ? 'Đang gợi ý…' : 'Gợi ý bước tiếp theo'}
        </button>
      </div>
      {summary && <div className="ai-out">{summary.text}</div>}
      {next && <div className="ai-out next">👉 {next.text}</div>}
      {err && <div className="err inline">{err}</div>}
    </div>
  )
}

function InteractionsSection({
  customerId,
  interactions,
  onChanged,
}: {
  customerId: number
  interactions: Interaction[]
  onChanged: () => Promise<void>
}) {
  const [kind, setKind] = useState<string>('note')
  const [summary, setSummary] = useState('')
  const [details, setDetails] = useState('')
  const [busy, setBusy] = useState(false)

  async function add() {
    if (!summary.trim()) return
    setBusy(true)
    try {
      await api.addInteraction(customerId, { kind, summary: summary.trim(), details: details.trim() || undefined })
      setSummary('')
      setDetails('')
      await onChanged()
    } finally {
      setBusy(false)
    }
  }

  async function del(id: number) {
    if (!confirm('Xoá tương tác này?')) return
    await api.deleteInteraction(id)
    await onChanged()
  }

  return (
    <div className="card">
      <div className="section-title">🕐 Lịch sử tương tác</div>
      <div className="new-interaction">
        <select value={kind} onChange={(e) => setKind(e.target.value)}>
          {Object.entries(KIND_META).map(([k, m]) => (
            <option key={k} value={k}>
              {m.icon} {m.label}
            </option>
          ))}
        </select>
        <input
          placeholder="Tóm tắt: 'Alo hỏi thăm', 'Gửi báo giá'…"
          value={summary}
          onChange={(e) => setSummary(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && add()}
        />
        <button className="btn primary" onClick={add} disabled={busy || !summary.trim()}>
          {busy ? 'Đang ghi…' : 'Ghi'}
        </button>
      </div>
      <textarea
        rows={2}
        placeholder="Chi tiết (tuỳ chọn)…"
        value={details}
        onChange={(e) => setDetails(e.target.value)}
      />
      <div className="timeline">
        {interactions.length === 0 && <div className="empty">Chưa có tương tác nào — thêm ở trên.</div>}
        {interactions.map((i) => {
          const meta = KIND_META[i.kind] ?? { icon: '•', label: i.kind }
          return (
            <div className="tl-item" key={i.id}>
              <div className="tl-dot" title={meta.label}>
                {meta.icon}
              </div>
              <div className="tl-body">
                <div className="tl-head">
                  <span className="tl-summary">{i.summary}</span>
                  <span className="tl-when">{fmtDateTime(i.occurred_at)}</span>
                  <button className="tl-del" onClick={() => del(i.id)} title="Xoá">
                    ×
                  </button>
                </div>
                {i.details && <div className="tl-details">{i.details}</div>}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

function NewCustomerModal({
  onClose,
  onCreate,
}: {
  onClose: () => void
  onCreate: (c: CustomerInput, channels?: Array<{ kind: string; value: string; label?: string }>) => Promise<void>
}) {
  const [form, setForm] = useState<CustomerInput>({ name: '', role: 'lead', tags: [] })
  const [tagInput, setTagInput] = useState('')
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [channels, setChannels] = useState<Array<{ kind: string; value: string; label: string }>>([])
  const fileRef = useRef<HTMLInputElement>(null)

  const tags = form.tags ?? []

  function set<K extends keyof CustomerInput>(k: K, v: CustomerInput[K]) {
    setForm((f) => ({ ...f, [k]: v }))
  }

  async function onAvatarFile(f: File | undefined) {
    if (!f) return
    if (f.size > 512 * 1024) {
      alert('Ảnh quá lớn — chọn ảnh dưới 512 KB.')
      return
    }
    set('avatar_url', await fileToDataUrl(f))
  }

  async function submit() {
    if (!form.name?.trim()) {
      setErr('Tên khách hàng là bắt buộc.')
      return
    }
    setBusy(true)
    setErr('')
    try {
      const nonEmpty = channels.filter((c) => c.value.trim())
      await onCreate({ ...form, name: form.name!.trim() }, nonEmpty)
    } catch (e) {
      setErr(String(e))
    } finally {
      setBusy(false)
    }
  }

  function addChannel(kind: string = 'zalo') {
    setChannels((cs) => [...cs, { kind, value: '', label: '' }])
  }
  function patchChannel(i: number, patch: Partial<{ kind: string; value: string; label: string }>) {
    setChannels((cs) => cs.map((c, idx) => (idx === i ? { ...c, ...patch } : c)))
  }
  function removeChannel(i: number) {
    setChannels((cs) => cs.filter((_, idx) => idx !== i))
  }

  const previewName = form.name?.trim() || 'Khách mới'
  return (
    <Modal
      open
      onCancel={onClose}
      title="Thêm khách hàng"
      width={620}
      footer={[
        <Button key="cancel" onClick={onClose}>Huỷ</Button>,
        <Button key="ok" type="primary" loading={busy} onClick={submit}>Tạo khách hàng</Button>,
      ]}
    >
      <div className="modal-avatar">
        <Avatar name={previewName} url={form.avatar_url} size={80} />
        <div className="modal-avatar-actions">
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            style={{ display: 'none' }}
            onChange={(e) => onAvatarFile(e.target.files?.[0])}
          />
          <Button onClick={() => fileRef.current?.click()}>Chọn ảnh…</Button>
          <Input
            value={form.avatar_url ?? ''}
            onChange={(e) => set('avatar_url', e.target.value)}
            placeholder="… hoặc dán URL"
          />
        </div>
      </div>
      <div className="edit-grid">
        <Field label="Tên *">
          <Input autoFocus value={form.name ?? ''} onChange={(e) => set('name', e.target.value)} />
        </Field>
        <Field label="Vai trò">
          <RolePicker value={form.role ?? 'lead'} onChange={(role) => set('role', role)} />
        </Field>
        <Field label="Email">
          <Input value={form.email ?? ''} onChange={(e) => set('email', e.target.value)} />
        </Field>
        <Field label="Điện thoại">
          <Input value={form.phone ?? ''} onChange={(e) => set('phone', e.target.value)} />
        </Field>
        <Field label="Công ty">
          <Input value={form.company ?? ''} onChange={(e) => set('company', e.target.value)} />
        </Field>
        <Field label="Chức danh">
          <Input value={form.title ?? ''} onChange={(e) => set('title', e.target.value)} />
        </Field>
        <Field label="Nguồn">
          <Input value={form.source ?? ''} onChange={(e) => set('source', e.target.value)} placeholder="Giới thiệu, sự kiện…" />
        </Field>
        <Field label="Sinh nhật">
          <Input value={form.birthday ?? ''} onChange={(e) => set('birthday', e.target.value)} placeholder="YYYY-MM-DD" />
        </Field>
        <Field label="Tags" full>
          <Select
            mode="tags"
            value={tags}
            onChange={(v) => set('tags', v)}
            style={{ width: '100%' }}
            placeholder="vip, thiết kế, hà nội…"
            tokenSeparators={[',']}
          />
          {/* keep tagInput ref to satisfy the unused-var lint until we can drop it */}
          <input type="hidden" value={tagInput} onChange={(e) => setTagInput(e.target.value)} />
        </Field>
        <Field label="Ghi chú" full>
          <Input.TextArea
            rows={3}
            value={form.notes ?? ''}
            onChange={(e) => set('notes', e.target.value)}
            placeholder="Sở thích, lịch sử, mong muốn…"
          />
        </Field>
        <Field label="Kênh liên hệ khác (nhiều SĐT · Zalo · Facebook · LinkedIn · Instagram · X · TikTok · Telegram · WhatsApp · YouTube · GitHub · …)" full>
          <div className="modal-channels">
            {channels.map((ch, i) => {
              const meta = channelMeta(ch.kind)
              return (
                <div key={i} className="channel-form" style={{ marginTop: 0 }}>
                  <Select
                    value={ch.kind}
                    onChange={(v) => patchChannel(i, { kind: v })}
                    style={{ minWidth: 160 }}
                    options={CHANNEL_KINDS.map((k) => ({
                      value: k,
                      label: (
                        <span>
                          {CHANNEL_META[k]!.icon} {CHANNEL_META[k]!.label}
                        </span>
                      ),
                    }))}
                  />
                  <Input
                    value={ch.value}
                    placeholder={meta.placeholder}
                    onChange={(e) => patchChannel(i, { value: e.target.value })}
                  />
                  <Input
                    value={ch.label}
                    placeholder="Ghi chú (Công việc / Cá nhân)"
                    style={{ maxWidth: 200 }}
                    onChange={(e) => patchChannel(i, { label: e.target.value })}
                  />
                  <Button size="small" danger type="text" onClick={() => removeChannel(i)}>
                    ×
                  </Button>
                </div>
              )
            })}
            <Space wrap style={{ marginTop: channels.length ? 6 : 0 }}>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('phone')}>
                📞 SĐT
              </Button>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('zalo')}>
                💬 Zalo
              </Button>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('facebook')}>
                📘 Facebook
              </Button>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('linkedin')}>
                💼 LinkedIn
              </Button>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('instagram')}>
                📷 Instagram
              </Button>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('telegram')}>
                ✈️ Telegram
              </Button>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('whatsapp')}>
                📱 WhatsApp
              </Button>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('website')}>
                🌐 Website
              </Button>
              <Button size="small" icon={<PlusOutlined />} onClick={() => addChannel('zalo')}>
                + Kênh khác…
              </Button>
            </Space>
          </div>
        </Field>
      </div>
      {err && <div className="err inline">{err}</div>}
    </Modal>
  )
}

function SettingsModal({
  open,
  onClose,
  settings,
  setSettings,
}: {
  open: boolean
  onClose: () => void
  settings: CrmSettings
  setSettings: (u: (s: CrmSettings) => CrmSettings) => void
}) {
  const [syncing, setSyncing] = useState(false)
  const [reindexing, setReindexing] = useState(false)
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null)

  async function syncNow() {
    setSyncing(true)
    setMessage(null)
    try {
      const r = await fetch('/api/sync/calendar', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ space_calendar: settings.syncSpaceCalendar }),
      }).then((x) => x.json())
      setSettings((s) => ({ ...s, lastSyncedAt: Math.floor(Date.now() / 1000) }))
      setMessage({
        ok: true,
        text: `Đồng bộ xong: ${r.pushed_tasks ?? 0} việc, ${r.pushed_birthdays ?? 0} sinh nhật đẩy sang lịch. ${r.note ?? ''}`,
      })
    } catch (e) {
      setMessage({ ok: false, text: 'Không đồng bộ được: ' + String(e) })
    } finally {
      setSyncing(false)
    }
  }

  async function reindex() {
    setReindexing(true)
    try {
      await fetch('/api/reindex', { method: 'POST' })
      setMessage({ ok: true, text: 'Đã rebuild lại FTS5 search index.' })
    } finally {
      setReindexing(false)
    }
  }

  return (
    <Modal
      open={open}
      onCancel={onClose}
      title={
        <span>
          <SettingOutlined /> Thiết lập
        </span>
      }
      footer={<Button onClick={onClose}>Đóng</Button>}
      width={560}
    >
      <Space direction="vertical" style={{ width: '100%' }} size="large">
        <div>
          <div className="section-title" style={{ marginTop: 0 }}>Giao diện</div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <div>
              <div>Chia đôi màn hình (2 view song song)</div>
              <div className="muted small">Bật để mở panel bên phải chọn view thứ 2.</div>
            </div>
            <Switch
              checked={settings.splitRight !== null}
              onChange={(v) => setSettings((s) => ({ ...s, splitRight: v ? 'tasks' : null }))}
            />
          </div>
        </div>

        <Divider style={{ margin: 0 }} />

        <div>
          <div className="section-title" style={{ marginTop: 0 }}>Đồng bộ Space Calendar</div>
          <div className="muted small" style={{ marginBottom: 10 }}>
            Đẩy <b>việc + sinh nhật</b> của CRM sang <b>Space Calendar</b> (Space App riêng).
            Chỉ 1 chiều CRM → Calendar cho batch sync. Khi bạn <b>sửa thời gian</b> hoặc <b>xoá</b> event
            trên Space Calendar, thay đổi đó sẽ tự đồng bộ ngược về CRM chỉ cho <b>event đó</b>.
          </div>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
            <span>📅 Đồng bộ với Space Calendar</span>
            <Switch
              checked={settings.syncSpaceCalendar}
              onChange={(v) => setSettings((s) => ({ ...s, syncSpaceCalendar: v }))}
            />
          </div>
          <div style={{ marginTop: 8, display: 'flex', gap: 8, alignItems: 'center' }}>
            <Button type="primary" loading={syncing} onClick={syncNow} disabled={!settings.syncSpaceCalendar}>
              🔄 Đồng bộ ngay
            </Button>
            <span className="muted small">
              {settings.lastSyncedAt ? `Lần cuối: ${fmtDateTime(settings.lastSyncedAt)}` : 'Chưa đồng bộ.'}
            </span>
          </div>
        </div>

        <Divider style={{ margin: 0 }} />

        <div>
          <div className="section-title" style={{ marginTop: 0 }}>Dữ liệu</div>
          <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            <Button loading={reindexing} onClick={reindex}>🔧 Rebuild FTS5 search index</Button>
            <Button href="/api/export.csv" download icon={<DownloadOutlined />}>Xuất CSV toàn bộ khách</Button>
          </div>
        </div>

        {message && (
          <div className={message.ok ? 'ai-out' : 'err inline'}>{message.text}</div>
        )}
      </Space>
    </Modal>
  )
}

function ThemeToggle({ theme, setTheme }: { theme: Theme; setTheme: (t: Theme) => void }) {
  function next() {
    setTheme(theme === null ? 'light' : theme === 'light' ? 'dark' : null)
  }
  const meta =
    theme === null
      ? { icon: <BulbOutlined />, label: 'Theo hệ điều hành' }
      : theme === 'light'
        ? { icon: <SunOutlined />, label: 'Ép giao diện sáng' }
        : { icon: <MoonOutlined />, label: 'Ép giao diện tối' }
  return (
    <Tooltip title={meta.label}>
      <Button shape="circle" icon={meta.icon} onClick={next} aria-label={meta.label} />
    </Tooltip>
  )
}

function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader()
    r.onload = () => resolve(String(r.result ?? ''))
    r.onerror = () => reject(r.error)
    r.readAsDataURL(file)
  })
}

// ---------- Deals under a customer ----------

function DealsSection({ customerId }: { customerId: number }) {
  const [deals, setDeals] = useState<Deal[]>([])
  const [showNew, setShowNew] = useState(false)
  const [editingId, setEditingId] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    setDeals(await api.customerDeals(customerId))
  }, [customerId])

  useEffect(() => {
    refresh()
  }, [refresh])

  async function quickStage(d: Deal, stage: string) {
    // Quick inline stage change — auto-logged, no user note.
    await api.updateDeal(d.id, { stage })
    await refresh()
  }
  async function del(id: number) {
    if (!confirm('Xoá deal này?')) return
    await api.deleteDeal(id)
    await refresh()
  }

  return (
    <div className="card">
      <div className="section-title">
        📊 Deals ({deals.length})
        <button className="btn ghost tiny right" onClick={() => setShowNew(true)}>
          + Thêm deal
        </button>
      </div>
      {deals.length === 0 && !showNew && <div className="empty small">Chưa có deal nào.</div>}
      {deals.map((d) => {
        const isEditing = editingId === d.id
        return (
          <div key={d.id}>
            <div className="deal-row">
              <div className="deal-row-main">
                <div className="deal-title">{d.title}</div>
                <div className="deal-sub">
                  {formatMoney(d.amount, d.currency)} · {d.probability}%
                  {d.expected_close_at ? ' · dự kiến ' + fmtDate(d.expected_close_at) : ''}
                </div>
              </div>
              <select
                className={'statuspick stage stage-' + d.stage}
                value={d.stage}
                onChange={(e) => quickStage(d, e.target.value)}
              >
                {STAGE_ORDER.map((s) => (
                  <option key={s} value={s}>
                    {STAGE_LABELS[s]}
                  </option>
                ))}
              </select>
              <button className="btn ghost tiny" onClick={() => setEditingId(isEditing ? null : d.id)} title="Sửa">
                {isEditing ? '×' : '✎'}
              </button>
              <button className="tl-del" onClick={() => del(d.id)} title="Xoá">
                ×
              </button>
            </div>
            {isEditing && (
              <EditDealForm
                deal={d}
                onClose={() => setEditingId(null)}
                onSaved={async () => {
                  setEditingId(null)
                  await refresh()
                }}
              />
            )}
          </div>
        )
      })}
      {showNew && (
        <NewDealForm
          customerId={customerId}
          onClose={() => setShowNew(false)}
          onCreated={async () => {
            setShowNew(false)
            await refresh()
          }}
        />
      )}
    </div>
  )
}

function EditDealForm({
  deal,
  onClose,
  onSaved,
}: {
  deal: Deal
  onClose: () => void
  onSaved: () => Promise<void>
}) {
  const [title, setTitle] = useState(deal.title)
  const [amount, setAmount] = useState(deal.amount)
  const [currency, setCurrency] = useState(deal.currency)
  const [stage, setStage] = useState(deal.stage)
  const [probability, setProbability] = useState(deal.probability)
  const [close, setClose] = useState(
    deal.expected_close_at ? new Date(deal.expected_close_at * 1000).toISOString().slice(0, 10) : '',
  )
  const [notes, setNotes] = useState(deal.notes)
  const [changeNote, setChangeNote] = useState('')
  const [busy, setBusy] = useState(false)

  async function save() {
    if (!title.trim()) return
    setBusy(true)
    try {
      const expected_close_at = close ? Math.floor(new Date(close).getTime() / 1000) : null
      await api.updateDeal(deal.id, {
        title: title.trim(),
        amount,
        currency,
        stage,
        probability,
        expected_close_at,
        notes,
        change_note: changeNote.trim() || undefined,
      } as Partial<Deal> & { change_note?: string })
      await onSaved()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="edit-inline">
      <div className="edit-inline-title">Sửa deal</div>
      <div className="edit-grid">
        <Field label="Tên deal">
          <input value={title} onChange={(e) => setTitle(e.target.value)} />
        </Field>
        <Field label="Giai đoạn">
          <select value={stage} onChange={(e) => setStage(e.target.value)}>
            {STAGE_ORDER.map((s) => (
              <option key={s} value={s}>
                {STAGE_LABELS[s]}
              </option>
            ))}
          </select>
        </Field>
        <Field label="Giá trị">
          <input type="number" value={amount} onChange={(e) => setAmount(Number(e.target.value) || 0)} />
        </Field>
        <Field label="Tiền tệ">
          <input value={currency} onChange={(e) => setCurrency(e.target.value.toUpperCase().slice(0, 4))} />
        </Field>
        <Field label="Xác suất (%)">
          <input
            type="number"
            min={0}
            max={100}
            value={probability}
            onChange={(e) => setProbability(Math.max(0, Math.min(100, Number(e.target.value) || 0)))}
          />
        </Field>
        <Field label="Ngày dự kiến đóng">
          <input type="date" value={close} onChange={(e) => setClose(e.target.value)} />
        </Field>
        <Field label="Ghi chú deal" full>
          <textarea rows={2} value={notes} onChange={(e) => setNotes(e.target.value)} />
        </Field>
        <Field label="Lý do thay đổi (ghi vào hoạt động)" full>
          <input
            value={changeNote}
            onChange={(e) => setChangeNote(e.target.value)}
            placeholder="VD: 'Khách yêu cầu giảm 10%'"
          />
        </Field>
      </div>
      <div className="formactions">
        <button className="btn ghost" onClick={onClose}>
          Huỷ
        </button>
        <button className="btn primary" onClick={save} disabled={busy || !title.trim()}>
          {busy ? 'Đang lưu…' : 'Lưu (ghi vào hoạt động)'}
        </button>
      </div>
    </div>
  )
}

function NewDealForm({
  customerId,
  onClose,
  onCreated,
}: {
  customerId: number
  onClose: () => void
  onCreated: () => Promise<void>
}) {
  const [title, setTitle] = useState('')
  const [amount, setAmount] = useState(0)
  const [currency, setCurrency] = useState('VND')
  const [stage, setStage] = useState('qualifying')
  const [close, setClose] = useState('')
  const [busy, setBusy] = useState(false)

  async function save() {
    if (!title.trim()) return
    setBusy(true)
    try {
      const expected_close_at = close ? Math.floor(new Date(close).getTime() / 1000) : undefined
      await api.createDeal(customerId, { title: title.trim(), amount, currency, stage, expected_close_at } as Partial<Deal>)
      await onCreated()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="new-deal">
      <input placeholder="Tên deal (vd: 'Gói yearly')" value={title} onChange={(e) => setTitle(e.target.value)} />
      <div className="row-inline">
        <input
          type="number"
          placeholder="Giá trị"
          value={amount || ''}
          onChange={(e) => setAmount(Number(e.target.value) || 0)}
        />
        <input
          placeholder="VND"
          value={currency}
          onChange={(e) => setCurrency(e.target.value.toUpperCase().slice(0, 4))}
          style={{ width: 70 }}
        />
        <select value={stage} onChange={(e) => setStage(e.target.value)}>
          {STAGE_ORDER.map((s) => (
            <option key={s} value={s}>
              {STAGE_LABELS[s]}
            </option>
          ))}
        </select>
        <input type="date" value={close} onChange={(e) => setClose(e.target.value)} />
      </div>
      <div className="formactions">
        <button className="btn ghost" onClick={onClose}>
          Huỷ
        </button>
        <button className="btn primary" onClick={save} disabled={busy || !title.trim()}>
          {busy ? 'Đang tạo…' : 'Thêm deal'}
        </button>
      </div>
    </div>
  )
}

// ---------- Tasks under a customer ----------

function TasksSection({ customerId }: { customerId: number }) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [title, setTitle] = useState('')
  const [due, setDue] = useState('')

  const refresh = useCallback(async () => {
    setTasks(await api.customerTasks(customerId))
  }, [customerId])

  useEffect(() => {
    refresh()
  }, [refresh])

  async function add() {
    if (!title.trim()) return
    const due_at = due ? Math.floor(new Date(due).getTime() / 1000) : undefined
    await api.createTask({ customer_id: customerId, title: title.trim(), due_at })
    setTitle('')
    setDue('')
    await refresh()
  }
  async function toggle(t: Task) {
    await api.toggleTask(t.id, !t.done)
    await refresh()
  }
  async function del(id: number) {
    await api.deleteTask(id)
    await refresh()
  }

  return (
    <div className="card">
      <div className="section-title">✅ Việc cần làm ({tasks.filter((t) => !t.done).length} mở)</div>
      <div className="new-task">
        <input
          placeholder="Việc: 'Gọi lại tuần sau'…"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && add()}
        />
        <input type="date" value={due} onChange={(e) => setDue(e.target.value)} />
        <button className="btn primary" onClick={add} disabled={!title.trim()}>
          Thêm
        </button>
      </div>
      <div className="tasklist">
        {tasks.length === 0 && <div className="empty small">Chưa có việc nào.</div>}
        {tasks.map((t) => (
          <TaskRow key={t.id} t={t} onToggle={() => toggle(t)} onDelete={() => del(t.id)} />
        ))}
      </div>
    </div>
  )
}

function TaskRow({ t, onToggle, onDelete }: { t: Task; onToggle: () => void; onDelete: () => void }) {
  const overdue = !t.done && t.due_at !== null && t.due_at < Date.now() / 1000
  return (
    <div className={'task-row' + (t.done ? ' done' : '') + (overdue ? ' overdue' : '')}>
      <input type="checkbox" checked={t.done} onChange={onToggle} />
      <div className="task-body">
        <div className="task-title">{t.title}</div>
        <div className="task-sub">
          {t.due_at ? '📅 ' + fmtDate(t.due_at) : 'không hạn'}
          {t.customer_name && ` · ${t.customer_name}`}
          {overdue && <span className="warn"> · quá hạn</span>}
        </div>
      </div>
      <button className="tl-del" onClick={onDelete} title="Xoá">
        ×
      </button>
    </div>
  )
}

// ---------- Pipeline (Kanban) view ----------

function PipelineView() {
  const [deals, setDeals] = useState<Deal[]>([])
  const [dragging, setDragging] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    setDeals(await api.listDeals())
  }, [])

  useEffect(() => {
    refresh()
  }, [refresh])

  return <PipelineBoard deals={deals} setDragging={setDragging} dragging={dragging} refresh={refresh} />
}

function PipelineBoard({
  deals,
  setDragging,
  dragging,
  refresh,
}: {
  deals: Deal[]
  setDragging: (n: number | null) => void
  dragging: number | null
  refresh: () => Promise<void>
}) {

  const drop = async (dealId: number, stage: string) => {
    setDragging(null)
    await api.updateDeal(dealId, { stage })
    await refresh()
  }

  const columns = STAGE_ORDER.map((stage) => ({
    stage,
    deals: deals.filter((d) => d.stage === stage),
    total: deals.filter((d) => d.stage === stage).reduce((s, d) => s + d.amount, 0),
  }))

  return (
    <div className="pipeline">
      {columns.map((col) => (
        <div
          key={col.stage}
          className={'kanban-col stage-' + col.stage}
          onDragOver={(e) => e.preventDefault()}
          onDrop={(e) => {
            e.preventDefault()
            if (dragging != null) drop(dragging, col.stage)
          }}
        >
          <div className="kanban-head">
            <span>{STAGE_LABELS[col.stage]}</span>
            <span className="kanban-count">{col.deals.length}</span>
          </div>
          <div className="kanban-sub">{formatMoney(col.total, col.deals[0]?.currency ?? 'VND')}</div>
          <div className="kanban-cards">
            {col.deals.map((d) => (
              <div
                key={d.id}
                className="kanban-card"
                draggable
                onDragStart={() => setDragging(d.id)}
                onDragEnd={() => setDragging(null)}
              >
                <div className="kc-title">{d.title}</div>
                <div className="kc-customer">{d.customer_name}</div>
                <div className="kc-amount">
                  {formatMoney(d.amount, d.currency)} · {d.probability}%
                </div>
                {d.expected_close_at && <div className="kc-close">📅 {fmtDate(d.expected_close_at)}</div>}
              </div>
            ))}
            {col.deals.length === 0 && <div className="kanban-empty">—</div>}
          </div>
        </div>
      ))}
      {deals.length === 0 && (
        <div className="pipeline-hint">Chưa có deal nào. Mở một khách hàng và bấm "+ Thêm deal".</div>
      )}
    </div>
  )
}

// ---------- Dashboard ----------

const STAGE_COLORS: Record<string, string> = {
  qualifying: '#6b7280',
  proposal: '#3b82f6',
  negotiation: '#a855f7',
  won: '#10b981',
  lost: '#ef4444',
}

function DashboardView({
  stats,
  onOpenNew,
  onPickCustomer,
  onGoto,
}: {
  stats: Stats | null
  onOpenNew: () => void
  onPickCustomer: (id: number) => void
  onGoto: (v: View) => void
}) {
  const [topDeals, setTopDeals] = useState<Deal[]>([])
  const [upcoming, setUpcoming] = useState<Upcoming | null>(null)
  const [activity, setActivity] = useState<ActivityItem[]>([])

  useEffect(() => {
    api.listDeals().then((deals) => {
      const open = deals.filter((d) => d.stage !== 'won' && d.stage !== 'lost')
      open.sort((a, b) => b.amount - a.amount)
      setTopDeals(open.slice(0, 5))
    })
    api.upcoming(14).then(setUpcoming)
    api.activity(5).then(setActivity)
  }, [])

  return (
    <div className="dashboard">
      <div className="dash-head">
        <div>
          <h1>Dashboard</h1>
          <div className="muted small">Tổng hợp toàn CRM · cập nhật realtime từ SQLite</div>
        </div>
        <Space>
          <Button icon={<DownloadOutlined />} href="/api/export.csv" download>
            Xuất CSV
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={onOpenNew}>
            Khách mới
          </Button>
        </Space>
      </div>

      {stats && (
        <div className="stat-grid">
          <StatTile label="Khách hàng" value={String(stats.customers)} accent="#6366f1" onClick={() => onGoto('customers')} />
          <StatTile
            label="Deals mở"
            value={String(stats.open_deals)}
            sub={formatMoney(stats.pipeline_value, topDeals[0]?.currency ?? 'VND') + ' đang mở'}
            accent="#3b82f6"
            onClick={() => onGoto('pipeline')}
          />
          <StatTile
            label="Đã chốt"
            value={formatShortMoney(stats.won_value)}
            sub={(stats.by_stage?.won?.count ?? 0) + ' deal thắng'}
            accent="#10b981"
            onClick={() => onGoto('pipeline')}
          />
          <StatTile
            label="Việc mở"
            value={String(stats.open_tasks)}
            sub={stats.overdue_tasks > 0 ? `${stats.overdue_tasks} quá hạn` : 'trong hạn'}
            accent={stats.overdue_tasks > 0 ? 'var(--warn)' : '#10b981'}
            warn={stats.overdue_tasks > 0}
            onClick={() => onGoto('tasks')}
          />
          <StatTile
            label="Tương tác"
            value={String(stats.interactions)}
            sub="tổng cộng"
            accent="#a855f7"
            onClick={() => onGoto('activity')}
          />
        </div>
      )}

      {stats && Object.keys(stats.by_stage ?? {}).length > 0 && (
        <div className="card">
          <div className="section-title">🗂 Pipeline theo giai đoạn</div>
          <StageBar byStage={stats.by_stage} />
        </div>
      )}

      <AggregateReportCard />

      <div className="dash-grid">
        <div className="card">
          <div className="section-title">
            🔥 Top deal đang mở
            <button className="linklike right" onClick={() => onGoto('pipeline')}>
              Xem tất cả →
            </button>
          </div>
          {topDeals.length === 0 && <div className="empty small">Chưa có deal nào.</div>}
          {topDeals.map((d) => (
            <div key={d.id} className="deal-row" onClick={() => onPickCustomer(d.customer_id)} style={{ cursor: 'pointer' }}>
              <div className="deal-row-main">
                <div className="deal-title">{d.title}</div>
                <div className="deal-sub">
                  {d.customer_name} · {formatMoney(d.amount, d.currency)} · {d.probability}%
                </div>
              </div>
              <span className={'status stage stage-' + d.stage}>{STAGE_LABELS[d.stage] ?? d.stage}</span>
            </div>
          ))}
        </div>

        <div className="card">
          <div className="section-title">
            🎂 Sắp tới (14 ngày)
            <button className="linklike right" onClick={() => onGoto('tasks')}>
              Xem tất cả →
            </button>
          </div>
          {upcoming && upcoming.birthdays.length === 0 && upcoming.tasks.length === 0 && (
            <div className="empty small">Không có sự kiện.</div>
          )}
          {upcoming?.birthdays.slice(0, 4).map((b) => (
            <div key={'b' + b.customer_id} className="upcoming-row" onClick={() => onPickCustomer(b.customer_id)} style={{ cursor: 'pointer' }}>
              <span className="upcoming-icon">🎂</span>
              <div>
                <b>{b.customer_name}</b>
                <div className="task-sub">Sinh nhật · {fmtDate(b.next_at)}</div>
              </div>
            </div>
          ))}
          {upcoming?.tasks.slice(0, 4).map((t) => (
            <div
              key={'t' + t.id}
              className="upcoming-row"
              onClick={() => t.customer_id && onPickCustomer(t.customer_id)}
              style={{ cursor: t.customer_id ? 'pointer' : 'default' }}
            >
              <span className="upcoming-icon">📌</span>
              <div>
                <b>{t.title}</b>
                <div className="task-sub">
                  Hạn {fmtDate(t.due_at)} {t.customer_name && `· ${t.customer_name}`}
                </div>
              </div>
            </div>
          ))}
        </div>

        <div className="card wide">
          <div className="section-title">
            🕐 Hoạt động gần đây
            <button className="linklike right" onClick={() => onGoto('activity')}>
              Xem tất cả →
            </button>
          </div>
          {activity.length === 0 && <div className="empty small">Chưa có tương tác.</div>}
          <div className="timeline compact">
            {activity.map((i) => {
              const meta = KIND_META[i.kind] ?? { icon: '•', label: i.kind }
              return (
                <div className="tl-item" key={i.id}>
                  <div className="tl-dot">{meta.icon}</div>
                  <div className="tl-body">
                    <div className="tl-head">
                      <span className="tl-summary">
                        <button className="linklike" onClick={() => onPickCustomer(i.customer_id)}>
                          {i.customer_name}
                        </button>
                        {' — '}
                        {i.summary}
                      </span>
                      <span className="tl-when">{fmtDateTime(i.occurred_at)}</span>
                    </div>
                  </div>
                </div>
              )
            })}
          </div>
        </div>
      </div>
    </div>
  )
}

function StatTile({
  label,
  value,
  sub,
  accent,
  warn,
  onClick,
}: {
  label: string
  value: string
  sub?: string
  accent: string
  warn?: boolean
  onClick?: () => void
}) {
  return (
    <Card
      hoverable={!!onClick}
      onClick={onClick}
      className={'stattile-card' + (warn ? ' warn' : '')}
      styles={{ body: { padding: 16, borderLeft: `4px solid ${accent}` } }}
    >
      <Statistic
        title={label}
        value={value}
        valueStyle={{ color: warn ? 'var(--warn)' : accent, fontSize: 26, fontWeight: 700 }}
      />
      {sub && <div style={{ color: 'var(--muted)', fontSize: 12, marginTop: 3 }}>{sub}</div>}
    </Card>
  )
}

function StageBar({ byStage }: { byStage: Record<string, { count: number; value: number }> }) {
  const entries = STAGE_ORDER.map((s) => ({ stage: s, ...(byStage[s] ?? { count: 0, value: 0 }) }))
  const total = entries.reduce((sum, e) => sum + e.value, 0)
  return (
    <div>
      <div className="stagebar">
        {entries.map((e) => {
          const pct = total > 0 ? (e.value / total) * 100 : 0
          if (pct === 0) return null
          return (
            <div
              key={e.stage}
              className="stagebar-seg"
              style={{ width: pct + '%', background: STAGE_COLORS[e.stage] }}
              title={`${STAGE_LABELS[e.stage]}: ${e.count} deal, ${formatShortMoney(e.value)}`}
            >
              {pct >= 12 ? STAGE_LABELS[e.stage] : ''}
            </div>
          )
        })}
        {total === 0 && <div className="stagebar-empty">Chưa có deal nào để hiển thị.</div>}
      </div>
      <div className="stage-legend">
        {entries.map((e) => (
          <div key={e.stage} className="legend-item">
            <span className="legend-dot" style={{ background: STAGE_COLORS[e.stage] }} />
            <span className="legend-label">{STAGE_LABELS[e.stage]}</span>
            <span className="legend-count">
              {e.count} · {formatShortMoney(e.value)}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

// ---------- Contact channels (multi-phone + socials) ----------

function ChannelsSection({ customerId }: { customerId: number }) {
  const [channels, setChannels] = useState<CustomerChannel[]>([])
  const [showAdd, setShowAdd] = useState(false)
  const [editingId, setEditingId] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    setChannels(await api.listChannels(customerId))
  }, [customerId])
  useEffect(() => {
    refresh()
  }, [refresh])

  async function del(id: number) {
    if (!confirm('Xoá kênh liên hệ này?')) return
    await api.deleteChannel(id)
    await refresh()
  }

  return (
    <Card
      className="channels-card"
      style={{ margin: '14px 0' }}
      title={
        <span className="section-title" style={{ margin: 0, textTransform: 'none', letterSpacing: 0, fontSize: 13 }}>
          📞 Kênh liên hệ ({channels.length})
        </span>
      }
      extra={
        <Button size="small" icon={<PlusOutlined />} onClick={() => setShowAdd(true)}>
          Thêm kênh
        </Button>
      }
    >
      {channels.length === 0 && !showAdd && (
        <div className="empty small">
          Chưa có kênh liên hệ nào. Bấm "Thêm kênh" để thêm SĐT thứ hai, Zalo, Facebook, LinkedIn, Instagram…
        </div>
      )}
      <div className="channel-list">
        {channels.map((ch) => {
          const meta = channelMeta(ch.kind)
          if (editingId === ch.id) {
            return (
              <ChannelForm
                key={ch.id}
                initial={ch}
                onCancel={() => setEditingId(null)}
                onSave={async (patch) => {
                  await api.updateChannel(ch.id, patch)
                  setEditingId(null)
                  await refresh()
                }}
              />
            )
          }
          return (
            <div key={ch.id} className="channel-row" style={{ borderLeft: `3px solid ${meta.color}` }}>
              <span className="channel-icon" style={{ background: meta.color + '22', color: meta.color }}>
                {meta.icon}
              </span>
              <div className="channel-body">
                <div className="channel-line1">
                  <a
                    href={meta.href(ch.value)}
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{ color: meta.color, fontWeight: 500 }}
                  >
                    {ch.value}
                  </a>
                  {ch.label && <Tag>{ch.label}</Tag>}
                </div>
                <div className="channel-line2 muted small">{meta.label}</div>
              </div>
              <Button size="small" type="text" onClick={() => setEditingId(ch.id)}>
                ✎
              </Button>
              <Button size="small" type="text" danger onClick={() => del(ch.id)}>
                ×
              </Button>
            </div>
          )
        })}
      </div>
      {showAdd && (
        <ChannelForm
          onCancel={() => setShowAdd(false)}
          onSave={async (v) => {
            await api.addChannel(customerId, { kind: v.kind!, value: v.value!, label: v.label })
            setShowAdd(false)
            await refresh()
          }}
        />
      )}
    </Card>
  )
}

function ChannelForm({
  initial,
  onCancel,
  onSave,
}: {
  initial?: CustomerChannel
  onCancel: () => void
  onSave: (v: { kind?: string; value?: string; label?: string }) => Promise<void>
}) {
  const [kind, setKind] = useState<string>(initial?.kind ?? 'zalo')
  const [value, setValue] = useState<string>(initial?.value ?? '')
  const [label, setLabel] = useState<string>(initial?.label ?? '')
  const [busy, setBusy] = useState(false)
  const meta = channelMeta(kind)

  async function save() {
    if (!value.trim()) return
    setBusy(true)
    try {
      await onSave({ kind, value: value.trim(), label: label.trim() })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="channel-form">
      <Select
        value={kind}
        onChange={setKind}
        style={{ minWidth: 170 }}
        options={CHANNEL_KINDS.map((k) => ({
          value: k,
          label: (
            <span>
              {CHANNEL_META[k]!.icon} {CHANNEL_META[k]!.label}
            </span>
          ),
        }))}
      />
      <Input
        value={value}
        placeholder={meta.placeholder}
        onChange={(e) => setValue(e.target.value)}
        onPressEnter={save}
      />
      <Input
        value={label}
        placeholder="Ghi chú (tuỳ chọn) — vd: Công việc"
        onChange={(e) => setLabel(e.target.value)}
        style={{ maxWidth: 200 }}
      />
      <Button size="small" onClick={onCancel}>Huỷ</Button>
      <Button size="small" type="primary" loading={busy} disabled={!value.trim()} onClick={save}>
        Lưu
      </Button>
    </div>
  )
}

// ---------- Relationships (per-customer) ----------

function RelationshipsSection({ customer }: { customer: Customer }) {
  const [rels, setRels] = useState<Relationship[]>([])
  const [showAdd, setShowAdd] = useState(false)
  const [extract, setExtract] = useState<{ busy: boolean; result: string; err: string }>({ busy: false, result: '', err: '' })

  const refresh = useCallback(async () => {
    setRels(await api.customerRelationships(customer.id))
  }, [customer.id])

  useEffect(() => {
    refresh()
  }, [refresh])

  async function del(id: number) {
    if (!confirm('Xoá quan hệ này?')) return
    await api.deleteRelationship(id)
    await refresh()
  }

  async function runExtract() {
    setExtract({ busy: true, result: '', err: '' })
    try {
      const r = await api.extract(customer.id)
      setExtract({
        busy: false,
        result: `Trích ${r.extracted} người, lưu ${r.mentions_saved} mention, tạo ${r.relationships_created} quan hệ mới.`,
        err: '',
      })
      await refresh()
    } catch (e) {
      setExtract({ busy: false, result: '', err: 'Cần bật LLM. ' + String(e) })
    }
  }

  return (
    <div className="card">
      <div className="section-title">
        🕸 Quan hệ ({rels.length})
        <span className="right">
          <button className="btn ghost tiny" onClick={runExtract} disabled={extract.busy}>
            {extract.busy ? '✨ Đang trích…' : '✨ AI trích'}
          </button>
          <button className="btn ghost tiny" onClick={() => setShowAdd(true)}>
            + Thêm
          </button>
        </span>
      </div>
      {rels.length === 0 && !showAdd && (
        <div className="empty small">Chưa có quan hệ. Bấm "+ Thêm" hoặc "✨ AI trích" để tự phát hiện từ notes/interactions.</div>
      )}
      {rels.map((r) => {
        const isFrom = r.from_id === customer.id
        const otherId = isFrom ? r.to_id : r.from_id
        const otherName = isFrom ? r.to_name : r.from_name
        // Reading direction from THIS customer's perspective.
        const verb = isFrom ? REL_LABELS[r.kind] ?? r.kind : `${REL_LABELS[r.kind] ?? r.kind} (ngược lại)`
        return (
          <div key={r.id} className="rel-row">
            <div className="rel-dot">🔗</div>
            <div className="rel-body">
              <div>
                <span className="rel-verb">{verb}</span>{' '}
                <button className="linklike" onClick={() => {}} title={`id=${otherId}`}>
                  {otherName}
                </button>
                {r.source === 'ai' && <span className="rel-ai">✨ AI</span>}
              </div>
              {r.note && <div className="task-sub">{r.note}</div>}
            </div>
            <button className="tl-del" onClick={() => del(r.id)} title="Xoá">
              ×
            </button>
          </div>
        )
      })}
      {showAdd && (
        <AddRelationshipForm
          fromId={customer.id}
          fromName={customer.name}
          onClose={() => setShowAdd(false)}
          onCreated={async () => {
            setShowAdd(false)
            await refresh()
          }}
        />
      )}
      {extract.result && <div className="ai-out">{extract.result}</div>}
      {extract.err && <div className="err inline">{extract.err}</div>}
    </div>
  )
}

function AddRelationshipForm({
  fromId,
  fromName,
  onClose,
  onCreated,
}: {
  fromId: number
  fromName: string
  onClose: () => void
  onCreated: () => Promise<void>
}) {
  const [customers, setCustomers] = useState<Customer[]>([])
  const [toId, setToId] = useState<number | ''>('')
  const [kind, setKind] = useState<string>('contact_of')
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    api.listCustomers({ limit: 500 }).then((all) => setCustomers(all.filter((c) => c.id !== fromId)))
  }, [fromId])

  async function save() {
    if (toId === '' || !kind) return
    setBusy(true)
    try {
      await api.createRelationship({ from_id: fromId, to_id: Number(toId), kind, note: note.trim() || undefined })
      await onCreated()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="add-rel">
      <div className="add-rel-line">
        <b>{fromName}</b>
        <select value={kind} onChange={(e) => setKind(e.target.value)}>
          {REL_ORDER.map((k) => (
            <option key={k} value={k}>
              {REL_LABELS[k]}
            </option>
          ))}
        </select>
        <select value={toId} onChange={(e) => setToId(e.target.value === '' ? '' : Number(e.target.value))}>
          <option value="">— chọn khách —</option>
          {customers.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name} {c.company && `· ${c.company}`}
            </option>
          ))}
        </select>
      </div>
      <input placeholder="Ghi chú (tuỳ chọn)…" value={note} onChange={(e) => setNote(e.target.value)} />
      <div className="formactions">
        <button className="btn ghost" onClick={onClose}>
          Huỷ
        </button>
        <button className="btn primary" onClick={save} disabled={busy || toId === ''}>
          {busy ? 'Đang lưu…' : 'Thêm quan hệ'}
        </button>
      </div>
    </div>
  )
}

// ---------- Global FTS5 search box ----------

function GlobalSearch({ onPickCustomer }: { onPickCustomer: (id: number) => void }) {
  const [q, setQ] = useState('')
  const [hits, setHits] = useState<SearchHit[]>([])

  useEffect(() => {
    if (!q.trim()) {
      setHits([])
      return
    }
    const h = setTimeout(async () => {
      try {
        setHits(await api.search(q.trim(), 15))
      } catch {
        setHits([])
      }
    }, 200)
    return () => clearTimeout(h)
  }, [q])

  const kindColor: Record<string, string> = { customer: 'purple', interaction: 'green', mention: 'gold' }
  const options = hits.map((h) => ({
    value: `${h.entity_type}-${h.entity_id}`,
    label: (
      <div className="search-hit">
        <Tag color={kindColor[h.entity_type] || 'default'} style={{ margin: 0 }}>
          {h.entity_type}
        </Tag>
        <div className="hit-body">
          <div className="hit-title">{h.title}</div>
          <div className="hit-snippet">{h.snippet}</div>
          {h.customer_name && <div className="hit-customer">→ {h.customer_name}</div>}
        </div>
      </div>
    ),
    customerId: h.customer_id,
  }))

  return (
    <AutoComplete
      className="globalsearch-antd"
      value={q}
      options={options}
      onSearch={setQ}
      onSelect={(_, opt) => {
        const cid = (opt as { customerId?: number | null }).customerId
        if (cid) onPickCustomer(cid)
        setQ('')
      }}
      notFoundContent={q.trim() ? `Không có kết quả cho "${q}"` : null}
      style={{ width: 320 }}
    >
      <Input.Search
        placeholder="Tìm mọi thứ trong CRM (FTS5)…"
        allowClear
        prefix={<SearchOutlined />}
      />
    </AutoComplete>
  )
}

// ---------- Network (force-directed graph) ----------

function NetworkView({
  net,
  setNet,
  busy,
  setBusy,
  onBackgroundResult,
  onPickCustomer,
}: {
  net: NetState
  setNet: (updater: (s: NetState) => NetState) => void
  busy: null | 'common' | 'ai_path'
  setBusy: (b: null | 'common' | 'ai_path') => void
  onBackgroundResult: (msg: string) => void
  onPickCustomer: (id: number) => void
}) {
  const [allNodes, setAllNodes] = useState<GraphNode[]>([])
  const [allEdges, setAllEdges] = useState<Relationship[]>([])
  const [positions, setPositions] = useState<Record<number, { x: number; y: number }>>({})
  const [dragging, setDragging] = useState<number | null>(null)
  const [hovered, setHovered] = useState<number | null>(null)
  const svgRef = useRef<SVGSVGElement>(null)
  const width = 900
  const height = 560

  // Persisted state (via `net`).
  const nameFilter = net.nameFilter
  const roleFilter = net.roleFilter
  const kindFilter = net.kindFilter
  const focus = net.focus
  const pathState = net.pathState
  const common = net.common
  const aiPath = net.aiPath
  const setNameFilter = (v: string) => setNet((s) => ({ ...s, nameFilter: v }))
  const setRoleFilter = (v: string[]) => setNet((s) => ({ ...s, roleFilter: v }))
  const setKindFilter = (v: string[]) => setNet((s) => ({ ...s, kindFilter: v }))
  const setFocus = (v: NetState['focus']) => setNet((s) => ({ ...s, focus: v }))
  const setPathState = (v: NetState['pathState']) => setNet((s) => ({ ...s, pathState: v }))
  const setCommon = (v: NetState['common']) => setNet((s) => ({ ...s, common: v }))
  const setAiPath = (v: NetState['aiPath']) => setNet((s) => ({ ...s, aiPath: v }))

  // Ephemeral UI state (doesn't need persist).
  const [drawerId, setDrawerId] = useState<number | null>(null)
  const [pathModal, setPathModal] = useState(false)

  // Filter graph by name/role/kind + focus subgraph.
  const { nodes, edges } = useMemo(() => {
    let ns = allNodes
    let es = allEdges
    if (focus) {
      // BFS on the client so filters compose with focus.
      const adj = new Map<number, number[]>()
      for (const e of allEdges) {
        if (!adj.has(e.from_id)) adj.set(e.from_id, [])
        if (!adj.has(e.to_id)) adj.set(e.to_id, [])
        adj.get(e.from_id)!.push(e.to_id)
        adj.get(e.to_id)!.push(e.from_id)
      }
      const seen = new Set<number>([focus.id])
      let frontier = [focus.id]
      for (let i = 0; i < focus.hops; i++) {
        const next: number[] = []
        for (const v of frontier) {
          for (const n of adj.get(v) ?? []) if (!seen.has(n)) { seen.add(n); next.push(n) }
        }
        frontier = next
      }
      ns = allNodes.filter((n) => seen.has(n.id))
      es = allEdges.filter((e) => seen.has(e.from_id) && seen.has(e.to_id))
    }
    if (roleFilter.length > 0) ns = ns.filter((n) => roleFilter.includes(n.role))
    if (nameFilter.trim()) {
      const q = nameFilter.trim().toLowerCase()
      ns = ns.filter((n) => n.name.toLowerCase().includes(q) || n.company.toLowerCase().includes(q))
    }
    const idSet = new Set(ns.map((n) => n.id))
    es = es.filter((e) => idSet.has(e.from_id) && idSet.has(e.to_id))
    if (kindFilter.length > 0) es = es.filter((e) => kindFilter.includes(e.kind))
    return { nodes: ns, edges: es }
  }, [allNodes, allEdges, focus, roleFilter, kindFilter, nameFilter])

  const pathEdgeSet = useMemo(() => {
    const s = new Set<string>()
    if (pathState?.ids && pathState.ids.length > 1) {
      for (let i = 0; i < pathState.ids.length - 1; i++) {
        s.add(`${pathState.ids[i]}-${pathState.ids[i + 1]}`)
        s.add(`${pathState.ids[i + 1]}-${pathState.ids[i]}`)
      }
    }
    return s
  }, [pathState])

  useEffect(() => {
    api.graph().then((g) => {
      setAllNodes(g.nodes)
      setAllEdges(g.edges)
    })
  }, [])

  // Simple force-directed layout: 200 iterations of repulsion + spring.
  useEffect(() => {
    if (nodes.length === 0) return
    const pos: Record<number, { x: number; y: number; vx: number; vy: number }> = {}
    // Seed positions in a circle so the first render doesn't overlap.
    nodes.forEach((n, i) => {
      const angle = (i / nodes.length) * Math.PI * 2
      pos[n.id] = {
        x: width / 2 + Math.cos(angle) * 200,
        y: height / 2 + Math.sin(angle) * 200,
        vx: 0,
        vy: 0,
      }
    })
    const iterations = 300
    const repel = 4500
    const spring = 0.02
    const edgeLen = 140
    const damp = 0.82
    for (let it = 0; it < iterations; it++) {
      // Repulsion between every pair.
      for (const a of nodes) {
        for (const b of nodes) {
          if (a.id === b.id) continue
          const dx = pos[a.id]!.x - pos[b.id]!.x
          const dy = pos[a.id]!.y - pos[b.id]!.y
          const d2 = dx * dx + dy * dy + 0.01
          const d = Math.sqrt(d2)
          const f = repel / d2
          pos[a.id]!.vx += (dx / d) * f
          pos[a.id]!.vy += (dy / d) * f
        }
      }
      // Springs on edges.
      for (const e of edges) {
        const a = pos[e.from_id]
        const b = pos[e.to_id]
        if (!a || !b) continue
        const dx = b.x - a.x
        const dy = b.y - a.y
        const d = Math.sqrt(dx * dx + dy * dy) + 0.01
        const f = (d - edgeLen) * spring
        a.vx += (dx / d) * f
        a.vy += (dy / d) * f
        b.vx -= (dx / d) * f
        b.vy -= (dy / d) * f
      }
      // Integrate.
      for (const n of nodes) {
        const p = pos[n.id]!
        p.vx *= damp
        p.vy *= damp
        p.x += p.vx
        p.y += p.vy
        // Keep in bounds.
        p.x = Math.max(30, Math.min(width - 30, p.x))
        p.y = Math.max(30, Math.min(height - 30, p.y))
      }
    }
    const final: Record<number, { x: number; y: number }> = {}
    for (const n of nodes) final[n.id] = { x: pos[n.id]!.x, y: pos[n.id]!.y }
    setPositions(final)
  }, [nodes, edges])

  function onMouseDown(id: number) {
    return (e: React.MouseEvent) => {
      e.preventDefault()
      setDragging(id)
    }
  }
  function onMouseMove(e: React.MouseEvent) {
    if (dragging == null || !svgRef.current) return
    const rect = svgRef.current.getBoundingClientRect()
    const scaleX = width / rect.width
    const scaleY = height / rect.height
    const x = (e.clientX - rect.left) * scaleX
    const y = (e.clientY - rect.top) * scaleY
    setPositions((p) => ({ ...p, [dragging]: { x, y } }))
  }
  function onMouseUp() {
    setDragging(null)
  }

  const roleCounts = useMemo(() => {
    const c: Record<string, number> = {}
    for (const n of nodes) c[n.role] = (c[n.role] ?? 0) + 1
    return c
  }, [nodes])

  return (
    <div className="network">
      <div className="network-head">
        <div>
          <h2>🕸 Mạng lưới quan hệ</h2>
          <div className="muted small">
            {nodes.length}/{allNodes.length} người · {edges.length}/{allEdges.length} kết nối · click node để mở panel
          </div>
        </div>
        <Space wrap>
          <Input.Search
            placeholder="Lọc theo tên/công ty…"
            allowClear
            style={{ width: 200 }}
            value={nameFilter}
            onChange={(e) => setNameFilter(e.target.value)}
          />
          <Select
            mode="multiple"
            allowClear
            placeholder="Lọc vai trò"
            style={{ minWidth: 180 }}
            value={roleFilter}
            onChange={setRoleFilter}
            maxTagCount="responsive"
            options={ROLE_ORDER.map((r) => ({
              value: r,
              label: `${roleMeta(r).icon} ${roleMeta(r).label}`,
            }))}
          />
          <Select
            mode="multiple"
            allowClear
            placeholder="Lọc loại quan hệ"
            style={{ minWidth: 200 }}
            value={kindFilter}
            onChange={setKindFilter}
            maxTagCount="responsive"
            options={REL_ORDER.map((k) => ({ value: k, label: REL_LABELS[k] }))}
          />
          <Button onClick={() => setPathModal(true)}>🧭 Tìm đường</Button>
          {focus && (
            <Button danger onClick={() => setFocus(null)}>
              Thoát gốc
            </Button>
          )}
        </Space>
      </div>
      {focus && (
        <Card size="small" style={{ marginBottom: 12 }}>
          <Space wrap>
            <span>
              🎯 Gốc: <b>{allNodes.find((n) => n.id === focus.id)?.name}</b>
            </span>
            <span>Mở rộng:</span>
            <Segmented
              options={[1, 2, 3, 4].map((n) => ({ label: `${n} hop`, value: n }))}
              value={focus.hops}
              onChange={(v) => setFocus({ ...focus, hops: Number(v) })}
            />
            <span className="muted small">→ {nodes.length} người trong bán kính</span>
          </Space>
        </Card>
      )}
      {pathState && pathState.ids.length > 1 && (
        <Card
          size="small"
          style={{ marginBottom: 12, borderLeft: '3px solid var(--accent)' }}
          extra={<Button size="small" onClick={() => setPathState(null)}>Xoá</Button>}
        >
          🧭 <b>Đường đi ({pathState.hops} hop):</b>{' '}
          {pathState.ids
            .map((id) => allNodes.find((n) => n.id === id)?.name ?? `#${id}`)
            .join(' → ')}
        </Card>
      )}
      {pathState && pathState.ids.length === 0 && (
        <Card size="small" style={{ marginBottom: 12, borderLeft: '3px solid var(--warn)' }} extra={<Button size="small" onClick={() => setPathState(null)}>Đóng</Button>}>
          Không có đường đi qua các quan hệ hiện có.
        </Card>
      )}
      {aiPath && (
        <Card
          size="small"
          style={{ marginBottom: 12, borderLeft: '3px solid #ec4899' }}
          title={
            <span>
              🧠 AI phân tích kết nối:{' '}
              <b>{allNodes.find((n) => n.id === aiPath.from)?.name}</b> ↔{' '}
              <b>{allNodes.find((n) => n.id === aiPath.to)?.name}</b>
            </span>
          }
          extra={<Button size="small" onClick={() => setAiPath(null)}>Xoá</Button>}
        >
          <div style={{ marginBottom: 8, fontStyle: 'italic', color: 'var(--muted)' }}>
            {aiPath.summary}
          </div>
          {aiPath.bfs_path_names && aiPath.bfs_path_names.length > 1 && (
            <div style={{ marginBottom: 8 }}>
              <Tag color="gold">Đường BFS</Tag>{' '}
              {aiPath.bfs_path_names.join(' → ')}
            </div>
          )}
          <Space direction="vertical" style={{ width: '100%' }}>
            {aiPath.connections.map((c, i) => {
              const typeMeta: Record<string, { label: string; color: string; icon: string }> = {
                shared_interest: { label: 'Sở thích chung', color: 'purple', icon: '🎯' },
                common_market: { label: 'Cùng thị trường', color: 'geekblue', icon: '📊' },
                possible_bridge: { label: 'Cầu nối tiềm năng', color: 'orange', icon: '🌉' },
                explicit_path: { label: 'Đường trực tiếp', color: 'gold', icon: '🛤' },
                weak_tie: { label: 'Kết nối yếu', color: 'default', icon: '➰' },
                shared_person: { label: 'Người trung gian', color: 'magenta', icon: '👥' },
              }
              const meta = typeMeta[c.type] ?? { label: c.type, color: 'default', icon: '•' }
              const strengthColor = c.strength === 'strong' ? 'green' : c.strength === 'medium' ? 'blue' : 'default'
              return (
                <div key={i}>
                  <Tag color={meta.color}>{meta.icon} {meta.label}</Tag>
                  <Tag color={strengthColor}>{c.strength || 'unknown'}</Tag>
                  <span style={{ marginLeft: 4 }}>{c.detail}</span>
                </div>
              )
            })}
          </Space>
        </Card>
      )}
      {common && (
        <Card
          size="small"
          style={{ marginBottom: 12, borderLeft: '3px solid #ec4899' }}
          title={
            <span>
              ✨ Điểm chung với <b>{allNodes.find((n) => n.id === common.focus_id)?.name}</b> — {common.themes.length} chủ đề, {common.highlight_ids.length} khách
            </span>
          }
          extra={<Button size="small" onClick={() => setCommon(null)}>Xoá</Button>}
        >
          <Space direction="vertical" style={{ width: '100%' }}>
            {common.themes.map((t, i) => (
              <div key={i}>
                <Tag color="magenta">{t.theme}</Tag>
                <span className="muted small"> {t.why}</span>
                <div style={{ marginTop: 4 }}>
                  {t.customer_ids.map((cid) => {
                    const n = allNodes.find((x) => x.id === cid)
                    if (!n) return null
                    return (
                      <Button
                        key={cid}
                        size="small"
                        type="link"
                        style={{ padding: '0 6px' }}
                        onClick={() => setDrawerId(cid)}
                      >
                        {n.name}
                      </Button>
                    )
                  })}
                </div>
              </div>
            ))}
          </Space>
        </Card>
      )}
      <div className="network-canvas card" style={{ position: 'relative' }}>
        {busy && (
          <div className="network-lock">
            <Spin size="large" tip={busy === 'common' ? 'AI đang tìm điểm chung…' : 'AI đang phân tích kết nối…'}>
              <div style={{ padding: 60 }} />
            </Spin>
            <div className="muted small" style={{ textAlign: 'center', marginTop: 8 }}>
              Bạn vẫn có thể chuyển tab — sẽ báo khi xong.
            </div>
          </div>
        )}
        <svg
          ref={svgRef}
          viewBox={`0 0 ${width} ${height}`}
          onMouseMove={onMouseMove}
          onMouseUp={onMouseUp}
          onMouseLeave={onMouseUp}
        >
          {/* Edges */}
          {edges.map((e) => {
            const a = positions[e.from_id]
            const b = positions[e.to_id]
            if (!a || !b) return null
            const active = hovered === e.from_id || hovered === e.to_id
            const dashed = e.source === 'ai'
            const onPath = pathEdgeSet.has(`${e.from_id}-${e.to_id}`)
            return (
              <g key={e.id}>
                <line
                  x1={a.x}
                  y1={a.y}
                  x2={b.x}
                  y2={b.y}
                  stroke={onPath ? '#eab308' : active ? 'var(--accent)' : 'var(--muted)'}
                  strokeWidth={onPath ? 3 : active ? 2 : 1}
                  strokeDasharray={dashed ? '4 3' : undefined}
                  opacity={onPath ? 1 : hovered != null && !active ? 0.15 : 0.65}
                />
                {(active || onPath) && (
                  <text
                    x={(a.x + b.x) / 2}
                    y={(a.y + b.y) / 2}
                    fontSize={10}
                    fill={onPath ? '#eab308' : 'var(--accent)'}
                    textAnchor="middle"
                    style={{ pointerEvents: 'none' }}
                  >
                    {REL_LABELS[e.kind] ?? e.kind}
                  </text>
                )}
              </g>
            )
          })}
          {/* AI-common themed edges: dashed pink lines from focus to highlighted nodes */}
          {common && common.focus_id && positions[common.focus_id] && common.highlight_ids.map((hid) => {
            const a = positions[common.focus_id]
            const b = positions[hid]
            if (!a || !b) return null
            return (
              <line
                key={'common-' + hid}
                x1={a.x}
                y1={a.y}
                x2={b.x}
                y2={b.y}
                stroke="#ec4899"
                strokeWidth={2}
                strokeDasharray="6 4"
                opacity={0.7}
              />
            )
          })}

          {/* Nodes */}
          {nodes.map((n) => {
            const p = positions[n.id]
            if (!p) return null
            const meta = roleMeta(n.role)
            const r = 20 + Math.min(12, n.interaction_count)
            const active = hovered === n.id
            const isFocus = common?.focus_id === n.id
            const isHighlight = common?.highlight_ids.includes(n.id) ?? false
            return (
              <g
                key={n.id}
                transform={`translate(${p.x},${p.y})`}
                onMouseDown={onMouseDown(n.id)}
                onMouseEnter={() => setHovered(n.id)}
                onMouseLeave={() => setHovered(null)}
                onClick={() => dragging == null && setDrawerId(n.id)}
                style={{ cursor: 'pointer' }}
              >
                {(isFocus || isHighlight) && (
                  <circle
                    r={r + 8}
                    fill="none"
                    stroke={isFocus ? '#ec4899' : '#f9a8d4'}
                    strokeWidth={isFocus ? 3 : 2}
                    opacity={0.85}
                  >
                    <animate attributeName="r" from={r + 4} to={r + 12} dur="1.4s" repeatCount="indefinite" />
                    <animate attributeName="opacity" from="0.85" to="0.15" dur="1.4s" repeatCount="indefinite" />
                  </circle>
                )}
                <circle
                  r={r}
                  fill={meta.color}
                  stroke={active ? 'white' : meta.color}
                  strokeWidth={active ? 3 : 1}
                  opacity={0.9}
                />
                <text
                  y={4}
                  fontSize={12}
                  fill="white"
                  textAnchor="middle"
                  style={{ pointerEvents: 'none', fontWeight: 600 }}
                >
                  {meta.icon}
                </text>
                <text
                  y={r + 14}
                  fontSize={11}
                  fill="var(--text)"
                  textAnchor="middle"
                  style={{ pointerEvents: 'none' }}
                >
                  {n.name}
                </text>
              </g>
            )
          })}
        </svg>
      </div>
      <div className="net-legend card">
        <div className="section-title">Vai trò</div>
        <div className="net-legend-grid">
          {ROLE_ORDER.filter((r) => roleCounts[r]).map((r) => {
            const meta = roleMeta(r)
            return (
              <div key={r} className="net-legend-item">
                <span className="legend-dot" style={{ background: meta.color }} />
                <span>{meta.label}</span>
                <span className="muted">· {roleCounts[r]}</span>
              </div>
            )
          })}
        </div>
      </div>

      <NodeDrawer
        id={drawerId}
        allNodes={allNodes}
        onClose={() => setDrawerId(null)}
        onOpenCustomer={onPickCustomer}
        onSetFocus={(id) => {
          setFocus({ id, hops: 1 })
          setDrawerId(null)
        }}
        onFindPath={(from) => {
          setPathModal(true)
          setDrawerId(null)
          setTimeout(() => setPathState({ from, ids: [], hops: 0 }), 0)
        }}
        onFindCommon={async (id) => {
          setDrawerId(null)
          setBusy('common')
          try {
            const r = await api.findCommon(id)
            setCommon(r)
            onBackgroundResult(`Đã tìm điểm chung cho ${allNodes.find(n=>n.id===id)?.name}.`)
          } catch (e) {
            alert('Cần bật LLM. ' + String(e))
          } finally {
            setBusy(null)
          }
        }}
      />

      <PathFinderModal
        open={pathModal}
        allNodes={allNodes}
        initialFrom={pathState?.from}
        onClose={() => setPathModal(false)}
        onResult={(from, to, ids, hops) => {
          setPathState({ from, to, ids, hops })
          setPathModal(false)
        }}
        onAiSearch={async (from, to) => {
          setPathModal(false)
          setBusy('ai_path')
          try {
            const r = await api.pathAi(from, to)
            setAiPath({
              from: r.from,
              to: r.to,
              summary: r.summary,
              connections: r.connections,
              bfs_path_names: r.bfs_path_names,
            })
            onBackgroundResult(`Đã phân tích kết nối ${allNodes.find(n=>n.id===from)?.name} ↔ ${allNodes.find(n=>n.id===to)?.name}.`)
          } catch (e) {
            alert('Cần bật LLM. ' + String(e))
          } finally {
            setBusy(null)
          }
        }}
      />
    </div>
  )
}

// ---------- Drawer for one node (focus / expand / similar / path) ----------

function NodeDrawer({
  id,
  allNodes,
  onClose,
  onOpenCustomer,
  onSetFocus,
  onFindPath,
  onFindCommon,
}: {
  id: number | null
  allNodes: GraphNode[]
  onClose: () => void
  onOpenCustomer: (id: number) => void
  onSetFocus: (id: number) => void
  onFindPath: (fromId: number) => void
  onFindCommon: (id: number) => Promise<void>
}) {
  const [commonBusy, setCommonBusy] = useState(false)
  const node = id != null ? allNodes.find((n) => n.id === id) : null
  const [similar, setSimilar] = useState<Array<{ customer: Customer; score: number; reasons: string[] }>>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (id == null) return
    setLoading(true)
    api.similar(id).then((r) => {
      setSimilar(r.similar)
      setLoading(false)
    }).catch(() => setLoading(false))
  }, [id])

  if (!node) return <Drawer open={false} onClose={onClose} />
  const meta = roleMeta(node.role)
  return (
    <Drawer
      open={id != null}
      onClose={onClose}
      title={
        <Space>
          <Avatar name={node.name} url={node.avatar_url} size={36} />
          <div>
            <div style={{ fontWeight: 600 }}>{node.name}</div>
            <div style={{ color: 'var(--muted)', fontSize: 12 }}>
              {node.company || 'Không rõ công ty'}
            </div>
          </div>
        </Space>
      }
      width={380}
    >
      <div style={{ marginBottom: 12 }}>
        <Tag color={meta.color}>
          {meta.icon} {meta.label}
        </Tag>
        <span className="muted small" style={{ marginLeft: 8 }}>
          {node.interaction_count} tương tác
        </span>
      </div>

      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <Space wrap>
          <Button type="primary" onClick={() => onSetFocus(node.id)}>
            🎯 Đặt làm gốc
          </Button>
          <Button onClick={() => onOpenCustomer(node.id)}>Mở hồ sơ</Button>
          <Button onClick={() => onFindPath(node.id)}>🧭 Tìm đường từ đây</Button>
          <Button
            loading={commonBusy}
            style={{ background: '#ec4899', color: 'white', borderColor: '#ec4899' }}
            onClick={async () => {
              setCommonBusy(true)
              try {
                await onFindCommon(node.id)
              } finally {
                setCommonBusy(false)
              }
            }}
          >
            ✨ AI tìm điểm chung
          </Button>
        </Space>

        <Card size="small" title={<span>✨ Khách tương đồng (AI + heuristic)</span>}>
          {loading && <div className="muted">Đang tính…</div>}
          {!loading && similar.length === 0 && <div className="empty small">Không có ai đủ giống.</div>}
          {similar.map((s) => (
            <div key={s.customer.id} className="sim-row">
              <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <Avatar name={s.customer.name} url={s.customer.avatar_url} size={28} />
                <Button type="link" style={{ padding: 0, fontWeight: 500 }} onClick={() => onOpenCustomer(s.customer.id)}>
                  {s.customer.name}
                </Button>
                <Tag>{s.score.toFixed(2)}</Tag>
              </div>
              <ul style={{ margin: '4px 0 6px 22px', padding: 0, color: 'var(--muted)', fontSize: 12 }}>
                {s.reasons.map((r, i) => (
                  <li key={i}>{r}</li>
                ))}
              </ul>
            </div>
          ))}
        </Card>
      </Space>
    </Drawer>
  )
}

function PathFinderModal({
  open,
  allNodes,
  initialFrom,
  onClose,
  onResult,
  onAiSearch,
}: {
  open: boolean
  allNodes: GraphNode[]
  initialFrom?: number
  onClose: () => void
  onResult: (from: number, to: number, ids: number[], hops: number) => void
  onAiSearch: (from: number, to: number) => Promise<void>
}) {
  const [from, setFrom] = useState<number | undefined>(initialFrom)
  const [to, setTo] = useState<number | undefined>()
  const busy = false // handled by parent Spin overlay

  useEffect(() => {
    if (open) setFrom(initialFrom)
  }, [open, initialFrom])

  const opts = allNodes.map((n) => ({ value: n.id, label: `${n.name} (${roleMeta(n.role).label})` }))

  // The AI handler runs on parent; deterministic BFS result piggybacks in the
  // AI response, so we keep the onResult prop for potential future direct use.
  void onResult

  async function runAi() {
    if (from == null || to == null) return
    // AI search fires asynchronously — the modal closes immediately; the busy
    // Spin overlay + notification take over.
    await onAiSearch(from, to)
  }

  const disabled = from == null || to == null || from === to

  return (
    <Modal
      open={open}
      onCancel={onClose}
      title="🧠 Tìm kết nối giữa 2 người bằng AI"
      footer={[
        <Button key="c" onClick={onClose}>Huỷ</Button>,
        <Button
          key="ai"
          type="primary"
          loading={busy}
          disabled={disabled}
          onClick={runAi}
          style={{ background: '#ec4899', borderColor: '#ec4899' }}
        >
          🧠 Tìm bằng AI (sở thích, điểm chung, cầu nối…)
        </Button>,
      ]}
      width={560}
    >
      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        <div>
          <div style={{ marginBottom: 4, color: 'var(--muted)' }}>Từ:</div>
          <Select
            showSearch
            style={{ width: '100%' }}
            value={from}
            onChange={setFrom}
            options={opts}
            optionFilterProp="label"
            placeholder="Chọn khách hàng…"
          />
        </div>
        <div>
          <div style={{ marginBottom: 4, color: 'var(--muted)' }}>Đến:</div>
          <Select
            showSearch
            style={{ width: '100%' }}
            value={to}
            onChange={setTo}
            options={opts.filter((o) => o.value !== from)}
            optionFilterProp="label"
            placeholder="Chọn khách hàng…"
          />
        </div>
        <div className="muted small">
          LLM sẽ đọc bối cảnh của 2 khách + đường quan hệ trực tiếp (nếu có) rồi suy luận
          các kết nối mềm: <b>sở thích chung, cùng ngành/thị trường, cầu nối qua người khác,
          người trung gian, mối quan hệ yếu</b>. Kết quả gồm summary + list connections
          có phân loại và độ mạnh. Đường trực tiếp (nếu có) tự được đưa vào phần "explicit_path".
        </div>
      </Space>
    </Modal>
  )
}

// ---------- Aggregate AI report card ----------

function AggregateReportCard() {
  const [report, setReport] = useState<{
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
  } | null>(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')

  async function run() {
    setBusy(true)
    setErr('')
    try {
      setReport(await api.aggregateReport())
    } catch (e) {
      setErr('Cần bật LLM trong daemon SenClaw để dùng thống kê AI. ' + String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="card ai report">
      <div className="section-title">
        ✨ Thống kê tổng hợp bằng AI
        <button className="btn primary tiny right" onClick={run} disabled={busy}>
          {busy ? 'Đang tổng hợp…' : report ? 'Làm lại' : 'Sinh báo cáo'}
        </button>
      </div>
      {!report && !err && !busy && (
        <div className="muted small">
          Tóm tắt toàn CRM: tổng quan pipeline, deal đáng chú ý, khách năng động, sinh nhật &amp;
          việc quá hạn, và đề xuất bước tiếp theo. Được LLM sinh, có căn cứ dữ liệu.
        </div>
      )}
      {report && (
        <>
          <div className="ai-out markdown" dangerouslySetInnerHTML={{ __html: renderMd(report.text) }} />
          <div className="report-foot">
            <span>
              📊 {report.grounding.customers} khách · {report.grounding.open_deals} deal mở ·{' '}
              {report.grounding.overdue_tasks} việc quá hạn
            </span>
            <span className="report-meta">
              {report.model} · {fmtDateTime(report.generated_at)}
            </span>
          </div>
        </>
      )}
      {err && <div className="err inline">{err}</div>}
    </div>
  )
}

/// Tiny markdown → HTML for the report card: **bold**, - bullets, blank lines
/// as paragraph breaks. Keeps the LLM output rendered without pulling a
/// dependency. Escapes < > & to keep it safe against a wayward model.
function renderMd(md: string): string {
  const esc = (s: string) => s.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' })[c]!)
  const inline = (s: string) => esc(s).replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
  const lines = md.split(/\r?\n/)
  let out = ''
  let inList = false
  const closeList = () => {
    if (inList) {
      out += '</ul>'
      inList = false
    }
  }
  for (const raw of lines) {
    const line = raw.trim()
    if (!line) {
      closeList()
      continue
    }
    if (line.startsWith('- ')) {
      if (!inList) {
        out += '<ul>'
        inList = true
      }
      out += `<li>${inline(line.slice(2))}</li>`
    } else {
      closeList()
      out += `<p>${inline(line)}</p>`
    }
  }
  closeList()
  return out
}

// ---------- Tasks view (global) ----------

function TasksView({ customers }: { customers: Customer[] }) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [upcoming, setUpcoming] = useState<Upcoming | null>(null)
  const [openOnly, setOpenOnly] = useState(true)
  const [showNew, setShowNew] = useState(false)

  const refresh = useCallback(async () => {
    const [t, u] = await Promise.all([api.listTasks({ open_only: openOnly, limit: 300 }), api.upcoming(30)])
    setTasks(t)
    setUpcoming(u)
  }, [openOnly])

  useEffect(() => {
    refresh()
  }, [refresh])

  return (
    <div className="tasksview">
      <div className="tasksview-main">
        <div className="tasksview-head">
          <h2>Việc & Nhắc</h2>
          <label className="inline">
            <input type="checkbox" checked={openOnly} onChange={(e) => setOpenOnly(e.target.checked)} /> chỉ mở
          </label>
          <button className="btn primary" onClick={() => setShowNew(true)}>
            + Thêm việc
          </button>
        </div>
        <div className="tasklist card">
          {tasks.length === 0 && <div className="empty">Không có việc nào.</div>}
          {tasks.map((t) => (
            <TaskRow
              key={t.id}
              t={t}
              onToggle={async () => {
                await api.toggleTask(t.id, !t.done)
                await refresh()
              }}
              onDelete={async () => {
                await api.deleteTask(t.id)
                await refresh()
              }}
            />
          ))}
        </div>
      </div>
      <aside className="upcoming card">
        <div className="section-title">🎂 Sắp tới (30 ngày)</div>
        {upcoming && upcoming.birthdays.length === 0 && upcoming.tasks.length === 0 && (
          <div className="empty small">Chưa có sự kiện.</div>
        )}
        {upcoming?.birthdays.map((b) => (
          <div key={b.customer_id} className="upcoming-row">
            <span className="upcoming-icon">🎂</span>
            <div>
              <b>{b.customer_name}</b>
              <div className="task-sub">{fmtDate(b.next_at)}</div>
            </div>
          </div>
        ))}
        {upcoming?.tasks.map((t) => (
          <div key={t.id} className="upcoming-row">
            <span className="upcoming-icon">📌</span>
            <div>
              <b>{t.title}</b>
              <div className="task-sub">
                {fmtDate(t.due_at)} {t.customer_name && `· ${t.customer_name}`}
              </div>
            </div>
          </div>
        ))}
      </aside>
      {showNew && (
        <NewTaskModal
          customers={customers}
          onClose={() => setShowNew(false)}
          onCreated={async () => {
            setShowNew(false)
            await refresh()
          }}
        />
      )}
    </div>
  )
}

function NewTaskModal({
  customers,
  onClose,
  onCreated,
}: {
  customers: Customer[]
  onClose: () => void
  onCreated: () => Promise<void>
}) {
  const [title, setTitle] = useState('')
  const [customerId, setCustomerId] = useState<number | ''>('')
  const [due, setDue] = useState('')
  const [details, setDetails] = useState('')
  const [busy, setBusy] = useState(false)

  async function save() {
    if (!title.trim()) return
    setBusy(true)
    try {
      const due_at = due ? Math.floor(new Date(due).getTime() / 1000) : undefined
      await api.createTask({
        title: title.trim(),
        details: details.trim() || undefined,
        due_at,
        customer_id: customerId === '' ? undefined : Number(customerId),
      })
      await onCreated()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="modalbg" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modaltitle">
          <h2>Thêm việc</h2>
          <button className="btn ghost" onClick={onClose}>
            ×
          </button>
        </div>
        <div className="edit-grid">
          <Field label="Tiêu đề" full>
            <input autoFocus value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Việc cần nhắc…" />
          </Field>
          <Field label="Khách hàng (tuỳ chọn)">
            <select value={customerId} onChange={(e) => setCustomerId(e.target.value === '' ? '' : Number(e.target.value))}>
              <option value="">— không gắn khách —</option>
              {customers.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name} {c.company && `· ${c.company}`}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Hạn">
            <input type="date" value={due} onChange={(e) => setDue(e.target.value)} />
          </Field>
          <Field label="Chi tiết" full>
            <textarea rows={3} value={details} onChange={(e) => setDetails(e.target.value)} />
          </Field>
        </div>
        <div className="formactions">
          <button className="btn ghost" onClick={onClose}>
            Huỷ
          </button>
          <button className="btn primary" onClick={save} disabled={busy || !title.trim()}>
            {busy ? 'Đang tạo…' : 'Thêm việc'}
          </button>
        </div>
      </div>
    </div>
  )
}

// ---------- Activity view (global) ----------

function ActivityView({ onPickCustomer }: { onPickCustomer: (id: number) => void }) {
  const [items, setItems] = useState<ActivityItem[]>([])

  useEffect(() => {
    api.activity(200).then(setItems)
  }, [])

  return (
    <div className="detail" style={{ padding: '20px 22px' }}>
      <h2 style={{ marginTop: 0 }}>🕐 Hoạt động gần đây</h2>
      <Card>
        {items.length === 0 ? (
          <div className="empty">Chưa có hoạt động.</div>
        ) : (
          <Timeline
            items={items.map((i) => {
              const meta = KIND_META[i.kind] ?? { icon: '•', label: i.kind }
              return {
                dot: <span style={{ fontSize: 16 }}>{meta.icon}</span>,
                children: (
                  <div>
                    <div style={{ display: 'flex', justifyContent: 'space-between', gap: 12 }}>
                      <div>
                        <Button type="link" size="small" style={{ padding: 0 }} onClick={() => onPickCustomer(i.customer_id)}>
                          {i.customer_name}
                        </Button>
                        {' — '}
                        <span style={{ fontWeight: 500 }}>{i.summary}</span>
                      </div>
                      <span style={{ color: 'var(--muted)', fontSize: 12, whiteSpace: 'nowrap' }}>
                        {fmtDateTime(i.occurred_at)}
                      </span>
                    </div>
                    {i.details && (
                      <div style={{ color: 'var(--muted)', whiteSpace: 'pre-wrap', fontSize: 13, marginTop: 3 }}>
                        {i.details}
                      </div>
                    )}
                  </div>
                ),
              }
            })}
          />
        )}
      </Card>
    </div>
  )
}

