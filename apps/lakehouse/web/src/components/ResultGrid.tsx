import { Empty, Table } from 'antd'
import type { ColumnsType } from 'antd/es/table'

// Render {columns: string[], rows: unknown[][]} thành bảng cuộn ngang.
export function ResultGrid({
  columns,
  rows,
  pageSize = 50,
}: {
  columns: string[]
  rows: unknown[][]
  pageSize?: number
}) {
  if (!columns.length) return <Empty description="Chưa có kết quả" />

  const cols: ColumnsType<Record<string, unknown>> = columns.map((c, i) => ({
    title: c,
    dataIndex: String(i),
    key: String(i),
    ellipsis: true,
    render: (v: unknown) => renderCell(v),
  }))

  const data = rows.map((r, ri) => {
    const o: Record<string, unknown> = { __k: ri }
    r.forEach((v, ci) => (o[String(ci)] = v))
    return o
  })

  return (
    <Table
      size="small"
      columns={cols}
      dataSource={data}
      rowKey="__k"
      scroll={{ x: 'max-content', y: 480 }}
      pagination={{ pageSize, size: 'small', showSizeChanger: false }}
    />
  )
}

function renderCell(v: unknown): string {
  if (v === null || v === undefined) return '∅'
  if (typeof v === 'object') return JSON.stringify(v)
  return String(v)
}
