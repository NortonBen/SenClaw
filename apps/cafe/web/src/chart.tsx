import { Empty, Flex, Tooltip } from 'antd'
import { fmtMoney } from './api'

export interface BarPoint {
  label: string
  /** Nhãn đầy đủ trong tooltip (mặc định = label). */
  title?: string
  a: number
  b?: number
}

/**
 * Biểu đồ cột tự vẽ (flex divs) — cùng cách làm với các Space App khác,
 * khỏi kéo thêm thư viện chart. Series a bắt buộc, b tuỳ chọn.
 */
export function BarChart({
  points,
  aName,
  bName,
  aColor = '#10b981',
  bColor = '#d97706',
  height = 140,
  fmt = fmtMoney,
  empty = 'Chưa có dữ liệu',
}: {
  points: BarPoint[]
  aName: string
  bName?: string
  aColor?: string
  bColor?: string
  height?: number
  fmt?: (v: number) => string
  empty?: string
}) {
  if (!points.length) return <Empty description={empty} image={Empty.PRESENTED_IMAGE_SIMPLE} />
  const max = Math.max(...points.map((p) => Math.max(p.a, p.b ?? 0)), 1)
  return (
    <div>
      <Flex gap={4} align="end" style={{ height, padding: '0 4px' }}>
        {points.map((p, i) => (
          <Tooltip
            key={`${p.label}-${i}`}
            title={
              <>
                <div>{p.title ?? p.label}</div>
                <div>
                  {aName}: {fmt(p.a)}
                </div>
                {bName !== undefined && p.b !== undefined && (
                  <div>
                    {bName}: {fmt(p.b)}
                  </div>
                )}
              </>
            }
          >
            <Flex gap={2} align="end" style={{ flex: 1, height: '100%', cursor: 'default' }}>
              <div
                style={{
                  flex: 1,
                  height: `${(Math.max(p.a, 0) / max) * 100}%`,
                  background: aColor,
                  borderRadius: 3,
                  minHeight: 2,
                }}
              />
              {p.b !== undefined && (
                <div
                  style={{
                    flex: 1,
                    height: `${(Math.max(p.b, 0) / max) * 100}%`,
                    background: bColor,
                    borderRadius: 3,
                    minHeight: 2,
                  }}
                />
              )}
            </Flex>
          </Tooltip>
        ))}
      </Flex>
      <Flex gap={4} style={{ padding: '4px 4px 0' }}>
        {points.map((p, i) => (
          <div
            key={`${p.label}-${i}`}
            style={{ flex: 1, textAlign: 'center', fontSize: 10, opacity: 0.65, overflow: 'hidden', whiteSpace: 'nowrap' }}
          >
            {p.label}
          </div>
        ))}
      </Flex>
      <Flex gap={12} style={{ padding: '6px 4px 0', fontSize: 12, opacity: 0.8 }}>
        <span>
          <span style={{ display: 'inline-block', width: 10, height: 10, background: aColor, borderRadius: 2, marginRight: 4 }} />
          {aName}
        </span>
        {bName !== undefined && (
          <span>
            <span style={{ display: 'inline-block', width: 10, height: 10, background: bColor, borderRadius: 2, marginRight: 4 }} />
            {bName}
          </span>
        )}
      </Flex>
    </div>
  )
}
