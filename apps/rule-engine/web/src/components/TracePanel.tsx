// Bottom drawer: live hop trace and chain log, both fed by SSE and reloadable
// from REST.

import { useState } from 'react'
import { Alert, Button, Drawer, Empty, Modal, Select, Space, Table, Tabs, Tag, Typography } from 'antd'
import { ReloadOutlined } from '@ant-design/icons'
import dayjs from 'dayjs'
import type { LogRow, RunRow, TraceHop } from '../types'

const LEVEL_COLOR: Record<string, string> = {
  error: 'red',
  warn: 'orange',
  warning: 'orange',
  info: 'blue',
  debug: 'default',
}

const RUN_COLOR: Record<string, string> = {
  running: 'processing',
  done: 'success',
  failed: 'error',
  timeout: 'warning',
}

const ts = (ms: number) => (ms ? dayjs(ms).format('HH:mm:ss.SSS') : '—')

export default function TracePanel({
  open,
  onClose,
  chainDebug,
  runs,
  runId,
  onSelectRun,
  hops,
  logs,
  onReloadRuns,
  onReloadLogs,
  nodeLabel,
}: {
  open: boolean
  onClose: () => void
  chainDebug: boolean
  runs: RunRow[]
  runId: number | null
  onSelectRun: (id: number) => void
  hops: TraceHop[]
  logs: LogRow[]
  onReloadRuns: () => void
  onReloadLogs: () => void
  nodeLabel: (id: string) => string
}) {
  const [detail, setDetail] = useState<{ title: string; body: string } | null>(null)

  return (
    <>
      <Drawer
        open={open}
        onClose={onClose}
        placement="bottom"
        size="60%"
        mask={false}
        title="Trace & Log"
        styles={{ body: { paddingTop: 8 } }}
      >
        {!chainDebug && (
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 10 }}
            title="Bật Debug để ghi trace từng bước"
            description="Luồng đang tắt Debug, nên chỉ những node tự bật debug mới sinh hop. Log vẫn ghi bình thường."
          />
        )}

        <Tabs
          size="small"
          defaultActiveKey="trace"
          items={[
            {
              key: 'trace',
              label: `Trace (${hops.length})`,
              children: (
                <div>
                  <Space style={{ marginBottom: 8 }} wrap>
                    <Select
                      size="small"
                      style={{ minWidth: 300 }}
                      placeholder="Chọn run…"
                      value={runId ?? undefined}
                      onChange={onSelectRun}
                      options={runs.map((r) => ({
                        value: r.id,
                        label: `#${r.id} · ${r.status} · ${r.trigger_node} · ${ts(r.started_at)} · ${r.hops} hop`,
                      }))}
                      notFoundContent={<Empty description="Chưa có run nào" />}
                    />
                    <Button size="small" icon={<ReloadOutlined />} onClick={onReloadRuns}>
                      Tải lại
                    </Button>
                    {runs.find((r) => r.id === runId) && (
                      <Tag color={RUN_COLOR[runs.find((r) => r.id === runId)!.status] ?? 'default'}>
                        {runs.find((r) => r.id === runId)!.status}
                      </Tag>
                    )}
                  </Space>

                  <Table<TraceHop>
                    size="small"
                    rowKey="key"
                    dataSource={hops}
                    pagination={{ pageSize: 50, size: 'small', hideOnSinglePage: true }}
                    scroll={{ x: 900 }}
                    locale={{ emptyText: 'Chưa có hop nào — chạy thử hoặc chọn một run.' }}
                    columns={[
                      { title: '#', dataIndex: 'seq', width: 56 },
                      {
                        title: 'Node',
                        dataIndex: 'node',
                        width: 180,
                        render: (v: string, r) => (
                          <span>
                            {nodeLabel(v)}{' '}
                            <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                              {r.rule}
                            </Typography.Text>
                          </span>
                        ),
                      },
                      {
                        title: 'Cổng',
                        key: 'ports',
                        width: 150,
                        render: (_, r) => (
                          <span style={{ fontSize: 11.5 }}>
                            <code>{r.inPort || '—'}</code> → <code>{r.outPort || '—'}</code>
                          </span>
                        ),
                      },
                      {
                        title: 'Loại',
                        dataIndex: 'kind',
                        width: 90,
                        render: (v: string) => (
                          <Tag color={v === 'error' ? 'red' : 'default'}>{v || 'data'}</Tag>
                        ),
                      },
                      {
                        title: 'Thời gian',
                        dataIndex: 'durMs',
                        width: 90,
                        render: (v: number) => `${v} ms`,
                      },
                      {
                        title: 'Dữ liệu',
                        dataIndex: 'data',
                        render: (v: string, r) => (
                          <span
                            className="json-cell"
                            title="Bấm để xem JSON đầy đủ"
                            onClick={() =>
                              setDetail({
                                title: `Hop #${r.seq} · ${nodeLabel(r.node)}`,
                                body: r.error ? `${r.error}\n\n${v}` : v,
                              })
                            }
                          >
                            {r.error ? `⛔ ${r.error}` : v || '—'}
                          </span>
                        ),
                      },
                    ]}
                  />
                </div>
              ),
            },
            {
              key: 'log',
              label: `Log (${logs.length})`,
              children: (
                <div>
                  <Button
                    size="small"
                    icon={<ReloadOutlined />}
                    style={{ marginBottom: 8 }}
                    onClick={onReloadLogs}
                  >
                    Tải lại
                  </Button>
                  <Table<LogRow>
                    size="small"
                    rowKey={(r) => `${r.id}-${r.ts}-${r.message}`}
                    dataSource={logs}
                    pagination={{ pageSize: 50, size: 'small', hideOnSinglePage: true }}
                    scroll={{ x: 760 }}
                    locale={{ emptyText: 'Chưa có log nào.' }}
                    columns={[
                      { title: 'Lúc', dataIndex: 'ts', width: 110, render: (v: number) => ts(v) },
                      {
                        title: 'Mức',
                        dataIndex: 'level',
                        width: 84,
                        render: (v: string) => (
                          <Tag color={LEVEL_COLOR[v?.toLowerCase()] ?? 'default'}>{v}</Tag>
                        ),
                      },
                      {
                        title: 'Node',
                        dataIndex: 'node',
                        width: 170,
                        render: (v: string | null) => (v ? nodeLabel(v) : '—'),
                      },
                      { title: 'Nội dung', dataIndex: 'message' },
                    ]}
                  />
                </div>
              ),
            },
          ]}
        />
      </Drawer>

      <Modal
        open={Boolean(detail)}
        onCancel={() => setDetail(null)}
        footer={null}
        width={720}
        title={detail?.title}
      >
        <pre
          className="mono"
          style={{ maxHeight: '60vh', overflow: 'auto', fontSize: 12, whiteSpace: 'pre-wrap' }}
        >
          {detail?.body}
        </pre>
      </Modal>
    </>
  )
}
