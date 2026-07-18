import { Select, Tag } from 'antd'
import {
  ORG_KIND_COLORS,
  ORG_KINDS,
  PRICING_COLORS,
  PRICING_MODELS,
  ROLE_ORDER,
  SALE_STAGE_COLORS,
  SALE_STAGES,
  SERVICE_KIND_COLORS,
  STAGE_COLORS,
  STAGE_ORDER,
  TEMP_META,
  TEMPERATURES,
  roleMeta,
} from '../constants'
import { tk, type T } from '../i18n'

/// A hairline outlined chip in an arbitrary colour — the reference CRM's badge
/// cell. AntD's `<Tag color>` only takes preset names or a solid fill, so the
/// tinted-outline look is done by hand here.
export function Chip({
  color,
  children,
  onClick,
  active,
  title,
}: {
  color: string
  children: React.ReactNode
  onClick?: () => void
  active?: boolean
  title?: string
}) {
  return (
    <span
      className={'ui-chip' + (onClick ? ' clickable' : '') + (active ? ' on' : '')}
      title={title}
      onClick={onClick}
      style={{
        color: active ? '#fff' : color,
        borderColor: active ? color : color + '55',
        background: active ? color : color + '14',
      }}
    >
      {children}
    </span>
  )
}

export function RoleBadge({ role, t, short }: { role: string; t: T; short?: boolean }) {
  const m = roleMeta(role)
  return (
    <Chip color={m.color}>
      {m.icon} {short ? m.short : tk(t, 'role', role)}
    </Chip>
  )
}

export function DealStageBadge({ stage, t }: { stage: string; t: T }) {
  return <Chip color={STAGE_COLORS[stage] ?? '#8e8e93'}>{tk(t, 'dealStage', stage)}</Chip>
}

export function SaleStageBadge({ stage, t }: { stage: string; t: T }) {
  return <Chip color={SALE_STAGE_COLORS[stage] ?? '#8e8e93'}>{tk(t, 'saleStage', stage)}</Chip>
}

export function TempBadge({ temp, t }: { temp: string; t: T }) {
  const m = TEMP_META[temp] ?? { icon: '•', color: '#8e8e93' }
  return (
    <Chip color={m.color}>
      {m.icon} {tk(t, 'temp', temp)}
    </Chip>
  )
}

export function OrgKindBadge({ kind, t }: { kind: string; t: T }) {
  return <Chip color={ORG_KIND_COLORS[kind] ?? '#8e8e93'}>{tk(t, 'orgKind', kind)}</Chip>
}

export function ServiceKindBadge({ kind, t }: { kind: string; t: T }) {
  return <Chip color={SERVICE_KIND_COLORS[kind] ?? '#8e8e93'}>{tk(t, 'svcKind', kind)}</Chip>
}

export function PricingBadge({ model, t }: { model: string; t: T }) {
  return <Chip color={PRICING_COLORS[model] ?? '#8e8e93'}>{tk(t, 'pricing', model)}</Chip>
}

/// Horizontal filter-chip row — `null` value means "all".
export function FilterChips({
  value,
  onChange,
  options,
  allLabel,
}: {
  value: string | null
  onChange: (v: string | null) => void
  options: Array<{ value: string; label: React.ReactNode; color: string }>
  allLabel: string
}) {
  return (
    <div className="filter-chips">
      <Chip color="#8e8e93" active={value === null} onClick={() => onChange(null)}>
        {allLabel}
      </Chip>
      {options.map((o) => (
        <Chip
          key={o.value}
          color={o.color}
          active={value === o.value}
          onClick={() => onChange(value === o.value ? null : o.value)}
        >
          {o.label}
        </Chip>
      ))}
    </div>
  )
}

/// Role select with the colour band on the selected value.
export function RolePicker({
  value,
  onChange,
  t,
  style,
}: {
  value: string
  onChange: (v: string) => void
  t: T
  style?: React.CSSProperties
}) {
  const rm = roleMeta(value)
  return (
    <Select
      value={value}
      onChange={onChange}
      style={{ minWidth: 170, ...style }}
      variant="outlined"
      popupMatchSelectWidth={220}
      options={ROLE_ORDER.map((r) => ({
        value: r,
        label: (
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span>{roleMeta(r).icon}</span>
            <span>{tk(t, 'role', r)}</span>
          </span>
        ),
      }))}
      labelRender={() => (
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6, color: rm.color, fontWeight: 500 }}>
          <span>{rm.icon}</span>
          <span>{tk(t, 'role', value)}</span>
        </span>
      )}
    />
  )
}

export function dealStageOptions(t: T) {
  return STAGE_ORDER.map((s) => ({ value: s, label: tk(t, 'dealStage', s) }))
}
export function saleStageOptions(t: T) {
  return SALE_STAGES.map((s) => ({ value: s, label: tk(t, 'saleStage', s) }))
}
export function tempOptions(t: T) {
  return TEMPERATURES.map((s) => ({
    value: s,
    label: `${TEMP_META[s]?.icon ?? ''} ${tk(t, 'temp', s)}`,
  }))
}
export function orgKindOptions(t: T) {
  return ORG_KINDS.map((s) => ({ value: s, label: tk(t, 'orgKind', s) }))
}
export function pricingOptions(t: T) {
  return PRICING_MODELS.map((s) => ({ value: s, label: tk(t, 'pricing', s) }))
}

/// Plain AntD tag for free-text tags — kept so `#tag` reads consistently.
export function TagChip({ tag }: { tag: string }) {
  return <Tag style={{ margin: 0 }}>#{tag}</Tag>
}
