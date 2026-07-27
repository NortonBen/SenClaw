// Node palette built from GET /api/registry. Drag onto the canvas, or
// double-click to drop one in the middle.

import { useMemo, useState } from 'react'
import { Collapse, Empty, Input, Tooltip, Typography, theme } from 'antd'
import type { RuleCategory, RuleSpec } from '../types'

export const DRAG_MIME = 'application/rule-node'

const CATEGORY_LABEL: Record<RuleCategory, string> = {
  source: 'Nguồn',
  logic: 'Logic / rẽ nhánh',
  transform: 'Biến đổi',
  filter: 'Bộ lọc',
  sink: 'Đầu ra',
  ai: 'AI & SenClaw',
}

const ORDER: RuleCategory[] = ['source', 'logic', 'transform', 'filter', 'ai', 'sink']

export default function Palette({
  rules,
  onAdd,
}: {
  rules: RuleSpec[]
  onAdd: (rule: RuleSpec) => void
}) {
  const { token } = theme.useToken()
  const [q, setQ] = useState('')

  const groups = useMemo(() => {
    const needle = q.trim().toLowerCase()
    const filtered = needle
      ? rules.filter(
          (r) =>
            r.name.toLowerCase().includes(needle) ||
            r.id.toLowerCase().includes(needle) ||
            r.description.toLowerCase().includes(needle),
        )
      : rules
    const map = new Map<RuleCategory, RuleSpec[]>()
    for (const r of filtered) {
      const list = map.get(r.category) ?? []
      list.push(r)
      map.set(r.category, list)
    }
    return ORDER.filter((c) => (map.get(c) ?? []).length > 0).map((c) => ({
      category: c,
      rules: (map.get(c) ?? []).sort((a, b) => a.name.localeCompare(b.name, 'vi')),
    }))
  }, [rules, q])

  return (
    <div
      style={{
        width: 240,
        flex: 'none',
        height: '100%',
        overflowY: 'auto',
        borderRight: `1px solid ${token.colorBorderSecondary}`,
        background: token.colorBgContainer,
      }}
    >
      <div style={{ padding: 8, position: 'sticky', top: 0, zIndex: 2, background: token.colorBgContainer }}>
        <Input.Search
          size="small"
          allowClear
          placeholder="Tìm node…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
      </div>

      {groups.length === 0 ? (
        <Empty style={{ marginTop: 32 }} description="Không có node phù hợp" />
      ) : (
        <Collapse
          size="small"
          ghost
          defaultActiveKey={ORDER}
          items={groups.map((g) => ({
            key: g.category,
            label: (
              <span style={{ fontSize: 12, fontWeight: 600 }}>
                {CATEGORY_LABEL[g.category]}{' '}
                <Typography.Text type="secondary" style={{ fontWeight: 400 }}>
                  ({g.rules.length})
                </Typography.Text>
              </span>
            ),
            children: (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                {g.rules.map((r) => (
                  <Tooltip key={r.id} title={r.description} placement="right" mouseEnterDelay={0.4}>
                    <div
                      className="palette-item"
                      draggable
                      onDragStart={(e) => {
                        e.dataTransfer.setData(DRAG_MIME, r.id)
                        e.dataTransfer.effectAllowed = 'move'
                      }}
                      onDoubleClick={() => onAdd(r)}
                    >
                      <span className="palette-item__icon">{r.icon}</span>
                      <span className="palette-item__name" style={{ color: r.color }}>
                        {r.name}
                      </span>
                    </div>
                  </Tooltip>
                ))}
              </div>
            ),
          }))}
        />
      )}
    </div>
  )
}
