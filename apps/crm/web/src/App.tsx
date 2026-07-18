import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  App as AntApp,
  AutoComplete,
  Button,
  ConfigProvider,
  Input,
  Tabs,
  Tag,
  Tooltip,
  theme as antTheme,
} from 'antd'
import enUS from 'antd/locale/en_US'
import viVN from 'antd/locale/vi_VN'
import {
  BulbOutlined,
  MoonOutlined,
  SearchOutlined,
  SplitCellsOutlined,
  SunOutlined,
} from '@ant-design/icons'
import { api, type Customer, type CustomerDetail, type CustomerInput, type SearchHit, type Stats } from './api'
import { ACCENT } from './constants'
import { makeT, type Lang } from './i18n'
import { subscribeEvents } from './events'
import { Sidebar, type NavBadges, type View } from './components/Sidebar'
import { DashboardPage } from './pages/DashboardPage'
import { ContactsPage, NewCustomerModal } from './pages/ContactsPage'
import { OrganizationsPage } from './pages/OrganizationsPage'
import { ServicesPage } from './pages/ServicesPage'
import { DealsPage } from './pages/DealsPage'
import { TasksPage } from './pages/TasksPage'
import { ActivityPage } from './pages/ActivityPage'
import { InboxPage } from './pages/InboxPage'
import { ReviewsPage } from './pages/ReviewsPage'
import { EscalationsPage } from './pages/EscalationsPage'
import { PipelinePage } from './pages/PipelinePage'
import { NetworkPage, NET_DEFAULT, type NetState } from './pages/NetworkPage'
import { SettingsPage, UI_SETTINGS_DEFAULT, type UiSettings } from './pages/SettingsPage'

const FONT_FAMILY =
  "Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif"

type Theme = 'light' | 'dark' | null

/// Tri-state: 'light' | 'dark' force it, `null` follows the OS.
function readTheme(): Theme {
  try {
    const s = localStorage.getItem('crm-theme')
    return s === 'light' || s === 'dark' ? s : null
  } catch {
    return null
  }
}

function detectSystemTheme(): 'light' | 'dark' {
  return window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

export default function App() {
  const [theme, setTheme] = useState<Theme>(readTheme)
  const [systemTheme, setSystemTheme] = useState<'light' | 'dark'>(detectSystemTheme)
  const [lang, setLang] = useState<Lang>('vi')
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

  // Language comes from the server's `language` setting at boot.
  useEffect(() => {
    api
      .getSettings()
      .then((s) => {
        if (s.language === 'vi' || s.language === 'en') setLang(s.language)
      })
      .catch(() => {})
  }, [])

  return (
    <ConfigProvider
      locale={lang === 'vi' ? viVN : enUS}
      theme={{
        algorithm: effectiveTheme === 'dark' ? antTheme.darkAlgorithm : antTheme.defaultAlgorithm,
        token: {
          colorPrimary: ACCENT,
          colorInfo: ACCENT,
          colorSuccess: '#34c759',
          colorWarning: '#ff9500',
          colorError: '#ff3b30',
          borderRadius: 8,
          fontFamily: FONT_FAMILY,
        },
      }}
    >
      <AntApp>
        <AppInner theme={theme} setTheme={setTheme} lang={lang} setLang={setLang} />
      </AntApp>
    </ConfigProvider>
  )
}

function AppInner({
  theme,
  setTheme,
  lang,
  setLang,
}: {
  theme: Theme
  setTheme: (t: Theme) => void
  lang: Lang
  setLang: (l: Lang) => void
}) {
  const t = useMemo(() => makeT(lang), [lang])

  const [view, setView] = useState<View>('dashboard')
  const viewRef = useRef<View>(view)
  useEffect(() => {
    viewRef.current = view
  }, [view])

  const [collapsed, setCollapsed] = useState(false)
  const [net, setNet] = useState<NetState>(NET_DEFAULT)
  const [settings, setSettings] = useState<UiSettings>(UI_SETTINGS_DEFAULT)
  const [netBusy, setNetBusy] = useState<null | 'common' | 'ai_path'>(null)
  const notify = AntApp.useApp().notification

  // ---- persisted UI state (via /api/state/*) ----
  const settingsHydrated = useRef(false)
  useEffect(() => {
    api.getState<UiSettings>('settings').then((v) => {
      if (v) setSettings({ ...UI_SETTINGS_DEFAULT, ...v })
      settingsHydrated.current = true
    })
  }, [])
  useEffect(() => {
    if (!settingsHydrated.current) return
    const h = setTimeout(() => api.putState('settings', settings).catch(() => {}), 400)
    return () => clearTimeout(h)
  }, [settings])

  const netHydrated = useRef(false)
  useEffect(() => {
    api.getState<NetState>('graph').then((v) => {
      if (v) setNet({ ...NET_DEFAULT, ...v })
      netHydrated.current = true
    })
  }, [])
  useEffect(() => {
    if (!netHydrated.current) return
    const h = setTimeout(() => api.putState('graph', net).catch(() => {}), 400)
    return () => clearTimeout(h)
  }, [net])

  // ---- shared customer data ----
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
  const [badges, setBadges] = useState<NavBadges>({})

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

  // ---- sidebar badges, refreshed live off the SSE stream ----
  const refreshBadges = useCallback(async () => {
    const [inbox, sale, s] = await Promise.all([
      api.inboxStats().catch(() => null),
      api.saleStats().catch(() => null),
      api.stats().catch(() => null),
    ])
    setBadges({
      inbox: inbox?.unread ?? 0,
      reviews: sale?.pendingReviews ?? 0,
      escalations: sale?.openEscalations ?? 0,
      tasks: s?.overdue_tasks ?? 0,
    })
  }, [])

  useEffect(() => {
    refreshBadges()
    return subscribeEvents(refreshBadges)
  }, [refreshBadges])

  // ---- hash deep links (#customer-{id} / #task-{id}) — the calendar sync
  // relies on these, so they stay even though there is no router.
  useEffect(() => {
    function applyHash() {
      const h = window.location.hash
      const c = /^#customer-(\d+)$/.exec(h)
      if (c) {
        setView('contacts')
        setSelectedId(Number(c[1]))
        return
      }
      const tsk = /^#task-(\d+)$/.exec(h)
      if (tsk) setView('tasks')
    }
    applyHash()
    window.addEventListener('hashchange', applyHash)
    return () => window.removeEventListener('hashchange', applyHash)
  }, [])

  const gotoCustomer = useCallback((id: number) => {
    setView('contacts')
    setSelectedId(id)
  }, [])

  async function onCreate(
    input: CustomerInput,
    channels?: Array<{ kind: string; value: string; label?: string }>,
  ) {
    const c = await api.createCustomer(input)
    // Bulk-add the pre-collected channels alongside the new customer — parallel
    // POSTs so the modal closes quickly even for a big social-network haul.
    if (channels && channels.length) {
      await Promise.all(
        channels.filter((ch) => ch.value.trim()).map((ch) => api.addChannel(c.id, ch).catch(() => null)),
      )
    }
    setShowNew(false)
    setSelectedId(c.id)
    setView('contacts')
    await refreshList()
  }

  async function onPatch(patch: CustomerInput & { change_note?: string }) {
    if (!selectedId) return
    await api.updateCustomer(selectedId, patch)
    await Promise.all([refreshList(), refreshDetail()])
  }

  async function onDelete() {
    if (!selectedId) return
    if (!confirm(t('deleteCustomer'))) return
    await api.deleteCustomer(selectedId)
    setSelectedId(null)
    setDetail(null)
    await refreshList()
  }

  /// One page, by key. Used for both the main area and the split-screen panel.
  function renderPage(v: View, onGoto: (x: View) => void, pick: (id: number) => void) {
    switch (v) {
      case 'dashboard':
        return (
          <DashboardPage
            stats={stats}
            t={t}
            onOpenNew={() => setShowNew(true)}
            onPickCustomer={pick}
            onGoto={onGoto}
          />
        )
      case 'contacts':
        return (
          <ContactsPage
            t={t}
            customers={customers}
            allTags={allTags}
            q={q}
            setQ={setQ}
            tag={tag}
            setTag={setTag}
            roleFilter={roleFilter}
            setRoleFilter={setRoleFilter}
            selectedId={selectedId}
            setSelectedId={setSelectedId}
            detail={detail}
            onPatch={onPatch}
            onDelete={onDelete}
            refreshDetail={refreshDetail}
            onOpenNew={() => setShowNew(true)}
          />
        )
      case 'organizations':
        return <OrganizationsPage t={t} onPickCustomer={pick} />
      case 'services':
        return <ServicesPage t={t} />
      case 'deals':
        return <DealsPage t={t} onPickCustomer={pick} />
      case 'tasks':
        return <TasksPage t={t} customers={customers} />
      case 'activity':
        return <ActivityPage t={t} onPickCustomer={pick} />
      case 'inbox':
        return <InboxPage t={t} customers={customers} onPickCustomer={pick} />
      case 'reviews':
        return <ReviewsPage t={t} onPickCustomer={pick} />
      case 'escalations':
        return <EscalationsPage t={t} onPickCustomer={pick} />
      case 'pipeline':
        return <PipelinePage t={t} onPickCustomer={pick} />
      case 'settings':
        return (
          <SettingsPage t={t} lang={lang} setLang={setLang} settings={settings} setSettings={setSettings} />
        )
      case 'network':
        // Rendered by the always-mounted instance below.
        return null
    }
  }

  return (
    <div className="app-shell">
      <Sidebar
        view={view}
        onNavigate={setView}
        collapsed={collapsed}
        onToggleCollapse={() => setCollapsed((v) => !v)}
        badges={badges}
        t={t}
      />

      <div className="app-main">
        <header className="topbar slim">
          <GlobalSearch t={t} onPickCustomer={gotoCustomer} />
          <div className="topbar-spacer" />
          <Tooltip title={t('splitScreen')}>
            <Button
              shape="circle"
              icon={<SplitCellsOutlined />}
              type={settings.splitRight ? 'primary' : 'default'}
              onClick={() => setSettings((s) => ({ ...s, splitRight: s.splitRight ? null : 'tasks' }))}
            />
          </Tooltip>
          <ThemeToggle theme={theme} setTheme={setTheme} t={t} />
        </header>

        {err && (
          <div className="err">
            {err}
            <button onClick={() => setErr('')}>×</button>
          </div>
        )}

        <div className="app-content">
          {view !== 'network' && renderPage(view, setView, gotoCustomer)}

          {/* NetworkPage stays mounted so hydrated state + running LLM tasks
              survive page switches; visibility is toggled on the wrapper. */}
          <div style={{ display: view === 'network' ? 'block' : 'none' }}>
            <NetworkPage
              net={net}
              setNet={setNet}
              busy={netBusy}
              setBusy={setNetBusy}
              t={t}
              onBackgroundResult={(msg) => {
                // Read the CURRENT page via a ref — the closure captured `view`
                // at click time, but the user may have navigated during the LLM
                // roundtrip. Only toast if they're no longer on Network.
                if (viewRef.current !== 'network') {
                  notify.info({
                    message: t('aiResultReady'),
                    description: msg + t('aiResultClick'),
                    onClick: () => setView('network'),
                    duration: 6,
                  })
                }
              }}
              onPickCustomer={gotoCustomer}
            />
          </div>
        </div>
      </div>

      {/* Split-screen right panel — an independent second view with its own
          nav, chosen independently from the left. */}
      {settings.splitRight && (
        <aside className="split-side-panel">
          <Tabs
            activeKey={settings.splitRight}
            onChange={(k) => setSettings((s) => ({ ...s, splitRight: k as View }))}
            className="viewnav-antd split-side-tabs"
            tabBarExtraContent={
              <Button size="small" onClick={() => setSettings((s) => ({ ...s, splitRight: null }))}>
                {t('close')} ×
              </Button>
            }
            items={(
              [
                ['dashboard', t('navDashboard')],
                ['inbox', t('navInbox')],
                ['contacts', t('navContacts')],
                ['organizations', t('navOrganizations')],
                ['deals', t('navDeals')],
                ['services', t('navServices')],
                ['pipeline', t('navPipeline')],
                ['tasks', t('navTasks')],
                ['activity', t('navActivity')],
              ] as Array<[View, string]>
            ).map(([key, label]) => ({ key, label }))}
          />
          <div className="split-side-body">
            {renderPage(
              settings.splitRight,
              (v) => setSettings((s) => ({ ...s, splitRight: v })),
              (id) => {
                setSettings((s) => ({ ...s, splitRight: 'contacts' }))
                setSelectedId(id)
              },
            )}
          </div>
        </aside>
      )}

      {showNew && <NewCustomerModal t={t} onClose={() => setShowNew(false)} onCreate={onCreate} />}
    </div>
  )
}

/// Global FTS5 search — debounced, jumps straight to the hit's customer.
function GlobalSearch({ t, onPickCustomer }: { t: ReturnType<typeof makeT>; onPickCustomer: (id: number) => void }) {
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

  const kindColor: Record<string, string> = {
    customer: 'purple',
    interaction: 'green',
    mention: 'gold',
    organization: 'blue',
    service: 'orange',
  }
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
      notFoundContent={q.trim() ? `${t('noResults')}: "${q}"` : null}
      style={{ width: 340 }}
    >
      <Input.Search placeholder={t('globalSearchPh')} allowClear prefix={<SearchOutlined />} />
    </AutoComplete>
  )
}

function ThemeToggle({
  theme,
  setTheme,
  t,
}: {
  theme: Theme
  setTheme: (t: Theme) => void
  t: ReturnType<typeof makeT>
}) {
  function next() {
    setTheme(theme === null ? 'light' : theme === 'light' ? 'dark' : null)
  }
  const meta =
    theme === null
      ? { icon: <BulbOutlined />, label: t('themeSystem') }
      : theme === 'light'
        ? { icon: <SunOutlined />, label: t('themeLight') }
        : { icon: <MoonOutlined />, label: t('themeDark') }
  return (
    <Tooltip title={meta.label}>
      <Button shape="circle" icon={meta.icon} onClick={next} aria-label={meta.label} />
    </Tooltip>
  )
}
