import { Badge, Tooltip } from 'antd'
import {
  ApartmentOutlined,
  AppstoreOutlined,
  ClockCircleOutlined,
  DashboardOutlined,
  DeploymentUnitOutlined,
  ExclamationCircleOutlined,
  FundProjectionScreenOutlined,
  InboxOutlined,
  MenuFoldOutlined,
  MenuUnfoldOutlined,
  SafetyCertificateOutlined,
  SettingOutlined,
  ShopOutlined,
  TeamOutlined,
  CheckSquareOutlined,
} from '@ant-design/icons'
import type { T } from '../i18n'

export type View =
  | 'dashboard'
  | 'inbox'
  | 'tasks'
  | 'contacts'
  | 'organizations'
  | 'deals'
  | 'services'
  | 'network'
  | 'pipeline'
  | 'reviews'
  | 'escalations'
  | 'activity'
  | 'settings'

export type NavBadges = {
  inbox?: number
  reviews?: number
  escalations?: number
  tasks?: number
}

type Item = { key: View; icon: React.ReactNode; label: string; badge?: number; badgeColor?: string }

/// Section captions + items. The grouping mirrors the reference CRM exactly.
export function navSections(t: T, badges: NavBadges): Array<{ caption: string; items: Item[] }> {
  return [
    {
      caption: t('groupOverview'),
      items: [
        { key: 'dashboard', icon: <DashboardOutlined />, label: t('navDashboard') },
        { key: 'inbox', icon: <InboxOutlined />, label: t('navInbox'), badge: badges.inbox },
        {
          key: 'tasks',
          icon: <CheckSquareOutlined />,
          label: t('navTasks'),
          badge: badges.tasks,
          badgeColor: '#ff9500',
        },
      ],
    },
    {
      caption: t('groupCrm'),
      items: [
        { key: 'contacts', icon: <TeamOutlined />, label: t('navContacts') },
        { key: 'organizations', icon: <ShopOutlined />, label: t('navOrganizations') },
        { key: 'deals', icon: <AppstoreOutlined />, label: t('navDeals') },
        { key: 'services', icon: <ApartmentOutlined />, label: t('navServices') },
        { key: 'network', icon: <DeploymentUnitOutlined />, label: t('navNetwork') },
      ],
    },
    {
      caption: t('groupSales'),
      items: [
        { key: 'pipeline', icon: <FundProjectionScreenOutlined />, label: t('navPipeline') },
        {
          key: 'reviews',
          icon: <SafetyCertificateOutlined />,
          label: t('navReviews'),
          badge: badges.reviews,
          badgeColor: '#ff9500',
        },
        {
          key: 'escalations',
          icon: <ExclamationCircleOutlined />,
          label: t('navEscalations'),
          badge: badges.escalations,
          badgeColor: '#ff3b30',
        },
      ],
    },
    {
      caption: t('groupWorkspace'),
      items: [
        { key: 'activity', icon: <ClockCircleOutlined />, label: t('navActivity') },
        { key: 'settings', icon: <SettingOutlined />, label: t('navSettings') },
      ],
    },
  ]
}

export function Sidebar({
  view,
  onNavigate,
  collapsed,
  onToggleCollapse,
  badges,
  t,
}: {
  view: View
  onNavigate: (v: View) => void
  collapsed: boolean
  onToggleCollapse: () => void
  badges: NavBadges
  t: T
}) {
  const sections = navSections(t, badges)
  return (
    <aside className={'nav-rail' + (collapsed ? ' collapsed' : '')}>
      <div className="nav-brand">
        <span className="nav-logo">👥</span>
        {!collapsed && (
          <div className="nav-brand-text">
            <div className="nav-brand-name">{t('appName')}</div>
            <div className="nav-brand-sub">{t('tagline')}</div>
          </div>
        )}
      </div>

      <nav className="nav-sections">
        {sections.map((sec) => (
          <div key={sec.caption} className="nav-section">
            {!collapsed && <div className="nav-caption">{sec.caption}</div>}
            {sec.items.map((it) => {
              const body = (
                <button
                  key={it.key}
                  className={'nav-item' + (view === it.key ? ' active' : '')}
                  onClick={() => onNavigate(it.key)}
                >
                  <span className="nav-item-icon">{it.icon}</span>
                  {!collapsed && <span className="nav-item-label">{it.label}</span>}
                  {!!it.badge && it.badge > 0 && (
                    <Badge
                      count={it.badge}
                      overflowCount={99}
                      color={it.badgeColor}
                      className={collapsed ? 'nav-badge-dot' : 'nav-badge'}
                    />
                  )}
                </button>
              )
              return collapsed ? (
                <Tooltip key={it.key} title={it.label} placement="right">
                  {body}
                </Tooltip>
              ) : (
                body
              )
            })}
          </div>
        ))}
      </nav>

      <button className="nav-collapse" onClick={onToggleCollapse} title={collapsed ? t('expand') : t('collapse')}>
        {collapsed ? <MenuUnfoldOutlined /> : <MenuFoldOutlined />}
        {!collapsed && <span>{t('collapse')}</span>}
      </button>
    </aside>
  )
}
