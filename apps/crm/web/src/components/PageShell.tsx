import { Input } from 'antd'
import { SearchOutlined } from '@ant-design/icons'

/// The per-page top bar the reference CRM uses: title left; search, filter slot
/// and a primary action right. Every page renders through this so the header
/// geometry stays identical across the app.
export function PageShell({
  title,
  subtitle,
  search,
  onSearch,
  searchPlaceholder,
  filters,
  actions,
  children,
  bare,
}: {
  title: string
  subtitle?: React.ReactNode
  search?: string
  onSearch?: (v: string) => void
  searchPlaceholder?: string
  filters?: React.ReactNode
  actions?: React.ReactNode
  children: React.ReactNode
  /// Skip the body padding — for panes that manage their own scroll (Inbox).
  bare?: boolean
}) {
  return (
    <div className="page">
      <header className="page-head">
        <div className="page-head-title">
          <h1>{title}</h1>
          {subtitle && <div className="muted small">{subtitle}</div>}
        </div>
        <div className="page-head-actions">
          {onSearch && (
            <Input
              allowClear
              prefix={<SearchOutlined />}
              placeholder={searchPlaceholder}
              value={search}
              onChange={(e) => onSearch(e.target.value)}
              style={{ width: 240 }}
            />
          )}
          {filters}
          {actions}
        </div>
      </header>
      <div className={bare ? 'page-body bare' : 'page-body'}>{children}</div>
    </div>
  )
}
