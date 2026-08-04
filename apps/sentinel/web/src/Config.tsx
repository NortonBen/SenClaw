import { useEffect, useState } from 'react'
import { Card, Table, Tag, Space, Button, Select, Typography, Empty, Descriptions, Alert, message } from 'antd'
import { api, fmtTs } from './api'

const { Text, Paragraph } = Typography

const KIND_LABEL: Record<string, string> = {
  mcp_servers: 'MCP server',
  mcp_tool_manifest: 'Manifest tool (chống rug pull)',
  tool_rules: 'Luật tự động cho qua',
  groups: 'Nhóm & quyền',
  hooks: 'Hook',
  admin_permissions: 'Cờ phê duyệt',
  skills: 'Skill',
  plugins: 'Plugin',
  schedules: 'Lịch',
}

export default function Config() {
  const [snaps, setSnaps] = useState<any[]>([])
  const [diffs, setDiffs] = useState<any[]>([])
  const [kind, setKind] = useState<string | undefined>()
  const [sources, setSources] = useState<any>(null)
  const [busy, setBusy] = useState(false)

  const load = async () => {
    const s: any = await api.snapshots({ kind })
    setSnaps(s.snapshots ?? [])
    const d: any = await api.diffs({ kind })
    setDiffs(d.diffs ?? [])
    setSources(await api.sources())
  }
  useEffect(() => {
    load()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind])

  const take = async () => {
    setBusy(true)
    try {
      const r: any = await api.takeSnapshot()
      message.success(
        r.changed?.length ? `Có thay đổi: ${r.changed.join(', ')}` : 'Đã chụp — cấu hình không đổi',
      )
      await load()
    } finally {
      setBusy(false)
    }
  }

  const db = sources?.daemon_db
  const trimmed = db?.stats?.tool_executions_trimmed ?? 0

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      <Alert
        type="info"
        showIcon
        message="Vì sao cần ảnh chụp"
        description="SenClaw ghi đè cấu hình tại chỗ — không version, không mốc thay đổi, không ghi ai đổi. Ảnh chụp định kỳ của Sentinel là nguồn duy nhất trả lời được câu hỏi 'hôm qua khác hôm nay chỗ nào'."
      />

      <Card size="small" title="Nguồn chứng cứ">
        {sources && (
          <Descriptions column={1} size="small" bordered>
            <Descriptions.Item label="DB daemon">
              {db?.ok ? (
                <Space wrap>
                  <Tag color="green">đọc được (chỉ-đọc)</Tag>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {db.stats.tool_executions} lượt tool còn trong daemon
                  </Text>
                  {trimmed > 0 && (
                    <Tag color="orange">
                      {trimmed} lượt đã bị daemon xoá — Sentinel giữ lại bản chép
                    </Tag>
                  )}
                </Space>
              ) : (
                <Tag color="red">không đọc được: {db?.error}</Tag>
              )}
            </Descriptions.Item>
            <Descriptions.Item label="Nhật ký LLM">
              {sources.llm_logs?.available ? (
                <Space wrap>
                  <Tag color="green">{sources.llm_logs.file_count} file</Tag>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    {Math.round((sources.llm_logs.total_bytes ?? 0) / 1048576)} MB — nơi duy nhất còn
                    giữ đối số tool. Sentinel chỉ lập chỉ mục, không chép nội dung.
                  </Text>
                </Space>
              ) : (
                <Tag color="orange">không có</Tag>
              )}
            </Descriptions.Item>
            <Descriptions.Item label="REST daemon">
              <Space>
                <Tag color={sources.daemon_rest?.reachable ? 'green' : 'red'}>
                  {sources.daemon_rest?.reachable ? 'kết nối được' : 'không kết nối được'}
                </Tag>
                <span className="mono">{sources.daemon_rest?.base}</span>
              </Space>
            </Descriptions.Item>
          </Descriptions>
        )}
      </Card>

      <Card
        size="small"
        title="Ảnh chụp cấu hình"
        extra={
          <Space>
            <Select
              size="small"
              style={{ width: 240 }}
              allowClear
              placeholder="Tất cả nhóm"
              value={kind}
              onChange={setKind}
              options={Object.entries(KIND_LABEL).map(([value, label]) => ({ value, label }))}
            />
            <Button size="small" type="primary" loading={busy} onClick={take}>
              Chụp ngay
            </Button>
          </Space>
        }
      >
        <Table
          rowKey="id"
          size="small"
          pagination={{ pageSize: 10, showSizeChanger: false }}
          dataSource={snaps}
          locale={{ emptyText: 'Chưa có ảnh chụp nào' }}
          columns={
            [
              { title: 'Nhóm', dataIndex: 'kind', width: 250, render: (k: string) => KIND_LABEL[k] ?? k },
              { title: 'Chụp lúc', dataIndex: 'taken_at', width: 180, render: (v: string) => fmtTs(v) },
              { title: 'Băm', dataIndex: 'body_hash', ellipsis: true, render: (v: string) => <span className="mono">{v?.slice(0, 24)}…</span> },
              { title: 'Kích thước', dataIndex: 'bytes', width: 110, render: (v: number) => `${v} B` },
            ] as any
          }
        />
      </Card>

      <Card size="small" title={`Thay đổi đã phát hiện (${diffs.length})`}>
        {diffs.length ? (
          diffs.map((d) => (
            <Card key={d.id} size="small" style={{ marginBottom: 10 }} type="inner" title={
              <Space>
                <Tag>{KIND_LABEL[d.kind] ?? d.kind}</Tag>
                <Text type="secondary" style={{ fontSize: 12 }}>{fmtTs(d.detected_at)}</Text>
              </Space>
            }>
              {d.added?.length > 0 && (
                <Paragraph style={{ margin: '4px 0' }}>
                  <Tag color="green">+ thêm {d.added.length}</Tag>
                  <span className="mono">{d.added.map((a: any) => a.key).join(', ')}</span>
                </Paragraph>
              )}
              {d.removed?.length > 0 && (
                <Paragraph style={{ margin: '4px 0' }}>
                  <Tag color="red">− xoá {d.removed.length}</Tag>
                  <span className="mono">{d.removed.map((a: any) => a.key).join(', ')}</span>
                </Paragraph>
              )}
              {d.changed?.length > 0 && (
                <Paragraph style={{ margin: '4px 0' }}>
                  <Tag color="orange">~ sửa {d.changed.length}</Tag>
                  <span className="mono">{d.changed.map((a: any) => a.key).join(', ')}</span>
                </Paragraph>
              )}
            </Card>
          ))
        ) : (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description="Chưa ghi nhận thay đổi cấu hình nào kể từ lần chụp đầu"
          />
        )}
      </Card>
    </Space>
  )
}
