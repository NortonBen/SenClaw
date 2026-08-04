/** Knowledge Graph liên kết tài liệu (/kg) — đồ thị mermaid + bảng cạnh,
 * số liệu deterministic từ backend (không AI). */
import { useCallback, useEffect, useMemo, useState } from 'react'
import { App, Button, Card, Empty, Table, Tag, Typography } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import MarkdownView from './md'
import { get } from './api'

export default function KgPanel({
  projectId,
  onOpenDoc,
}: {
  projectId: number
  onOpenDoc: (id: number) => void
}) {
  const { message } = App.useApp()
  const [kg, setKg] = useState<any>(null)

  const load = useCallback(async () => {
    try {
      setKg(await get(`/projects/${projectId}/kg`))
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }, [projectId, message])

  useEffect(() => {
    load()
  }, [load])

  const titleOf = useMemo(() => {
    const m = new Map<number, string>()
    for (const n of kg?.nodes ?? []) m.set(n.id, n.title)
    return m
  }, [kg])

  if (!kg) return null
  const edges = (kg.edges ?? []).map((e: any, i: number) => ({ ...e, key: i }))

  return (
    <Card
      size="small"
      title="Knowledge Graph — tài liệu nào liên kết tài liệu nào"
      extra={<Button size="small" icon={<ReloadOutlined />} onClick={load} />}
    >
      <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
        Cạnh nét đứt: quan hệ upstream (tài liệu sau đọc tài liệu trước khi sinh). Cạnh nét liền:
        tham chiếu ID thật trong nội dung (FR/US/AC...). Sửa một tài liệu thì nhìn cạnh đi ra để
        biết lan sang đâu — hoặc mở CR để cập nhật đồng bộ.
      </Typography.Paragraph>
      {kg.mermaid ? (
        <MarkdownView md={'```mermaid\n' + kg.mermaid + '\n```'} />
      ) : (
        <Empty
          image={Empty.PRESENTED_IMAGE_SIMPLE}
          description={kg.note || 'Chưa có tài liệu nào để vẽ đồ thị'}
        />
      )}
      {edges.length > 0 && (
        <Table
          size="small"
          pagination={{ pageSize: 15, hideOnSinglePage: true }}
          dataSource={edges}
          style={{ marginTop: 10 }}
          columns={[
            {
              title: 'Từ tài liệu',
              dataIndex: 'from',
              render: (v: number) => <a onClick={() => onOpenDoc(v)}>{titleOf.get(v) ?? `#${v}`}</a>,
            },
            {
              title: 'Liên kết',
              dataIndex: 'kind',
              width: 130,
              render: (k: string, r: any) =>
                k === 'upstream' ? (
                  <Tag>upstream</Tag>
                ) : (
                  <Tag color="geekblue">{r.count} ID tham chiếu</Tag>
                ),
            },
            {
              title: 'Tới tài liệu',
              dataIndex: 'to',
              render: (v: number) => <a onClick={() => onOpenDoc(v)}>{titleOf.get(v) ?? `#${v}`}</a>,
            },
          ]}
        />
      )}
    </Card>
  )
}
