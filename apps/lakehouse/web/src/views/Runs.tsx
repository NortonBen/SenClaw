import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  App,
  Button,
  Drawer,
  Progress,
  Select,
  Space,
  Table,
  Tag,
  Typography,
} from 'antd'
import { FileTextOutlined, ReloadOutlined, StopOutlined } from '@ant-design/icons'
import { cancelRun, getRunLogs, listRuns } from '../api'
import { DataTable } from '../components/DataTable'
import { useDashboardWS } from '../ws'
import { errMsg, fmtTime, isActiveStatus, isTerminalStatus, statusColor } from '../util'
import type { Run, StepRun } from '../types'
import { getRun } from '../api'

const STATUSES = ['queued', 'running', 'success', 'failed', 'cancelled']

export function Runs() {
  const { message } = App.useApp()
  const qc = useQueryClient()
  const [statusFilter, setStatusFilter] = useState<string | undefined>()
  const [logsRun, setLogsRun] = useState<string | null>(null)

  const runs = useQuery({
    queryKey: ['runs', 'list', statusFilter],
    queryFn: () => listRuns({ status: statusFilter, limit: 200 }),
    refetchInterval: 30000, // fallback poll; WS làm real-time bên dưới
  })

  // WS: chỉ invalidate khi có chuyển trạng thái — không diff delta.
  useDashboardWS((e) => {
    if (e.type === 'run:status' || e.type === 'step:progress') {
      qc.invalidateQueries({ queryKey: ['runs'] })
      const rid = e.data.run_id as string | undefined
      if (rid) qc.invalidateQueries({ queryKey: ['run', rid] })
    }
  })

  const cancel = useMutation({
    mutationFn: (id: string) => cancelRun(id),
    onSuccess: () => {
      message.success('Đã gửi yêu cầu huỷ')
      qc.invalidateQueries({ queryKey: ['runs'] })
    },
    onError: (e) => message.error(errMsg(e)),
  })

  return (
    <div>
      <Space style={{ marginBottom: 12 }}>
        <Typography.Title level={4} style={{ margin: 0 }}>
          Runs
        </Typography.Title>
        <Select
          allowClear
          placeholder="Lọc trạng thái"
          style={{ width: 180 }}
          value={statusFilter}
          onChange={setStatusFilter}
          options={STATUSES.map((s) => ({ value: s, label: s }))}
        />
        <Button icon={<ReloadOutlined />} onClick={() => runs.refetch()} loading={runs.isFetching}>
          Làm mới
        </Button>
      </Space>

      <DataTable<Run>
        rowKey="id"
        loading={runs.isLoading}
        dataSource={runs.data?.runs ?? []}
        expandable={{
          expandedRowRender: (r) => <StepTable runId={r.id} />,
        }}
        columns={[
          { title: 'Run', dataIndex: 'id', render: (v: string) => <code>{v.slice(0, 8)}</code> },
          { title: 'Flow', dataIndex: 'flow_id', ellipsis: true },
          {
            title: 'Trạng thái',
            dataIndex: 'status',
            render: (v: string) => <Tag color={statusColor(v)}>{v}</Tag>,
          },
          { title: 'Trigger', dataIndex: 'trigger' },
          { title: 'Bắt đầu', dataIndex: 'started_at', render: (v: string | null) => fmtTime(v) },
          { title: 'Kết thúc', dataIndex: 'ended_at', render: (v: string | null) => fmtTime(v) },
          {
            title: 'Thao tác',
            key: 'act',
            render: (_: unknown, r: Run) => (
              <Space size="small">
                <Button
                  size="small"
                  icon={<FileTextOutlined />}
                  onClick={() => setLogsRun(r.id)}
                >
                  Logs
                </Button>
                {isActiveStatus(r.status) && (
                  <Button
                    size="small"
                    danger
                    icon={<StopOutlined />}
                    loading={cancel.isPending && cancel.variables === r.id}
                    onClick={() => cancel.mutate(r.id)}
                  >
                    Huỷ
                  </Button>
                )}
              </Space>
            ),
          },
        ]}
      />

      <LogsDrawer runId={logsRun} onClose={() => setLogsRun(null)} />
    </div>
  )
}

function StepTable({ runId }: { runId: string }) {
  const run = useQuery({
    queryKey: ['run', runId],
    queryFn: () => getRun(runId),
    refetchInterval: (q) => {
      const st = q.state.data?.run.status
      return st && !isTerminalStatus(st) ? 5000 : false
    },
  })
  return (
    <Table<StepRun>
      size="small"
      rowKey="step_id"
      loading={run.isLoading}
      pagination={false}
      dataSource={run.data?.steps ?? []}
      columns={[
        { title: 'Bước', dataIndex: 'step_id' },
        {
          title: 'Trạng thái',
          dataIndex: 'status',
          render: (v: string) => <Tag color={statusColor(v)}>{v}</Tag>,
        },
        {
          title: 'Tiến độ',
          key: 'prog',
          width: 200,
          render: (_: unknown, s: StepRun) => (
            <Progress
              percent={s.status === 'success' ? 100 : s.status === 'running' ? 60 : 0}
              status={
                s.status === 'failed' || s.status === 'error'
                  ? 'exception'
                  : s.status === 'running'
                    ? 'active'
                    : undefined
              }
              size="small"
            />
          ),
        },
        { title: 'Đọc', dataIndex: 'rows_read', align: 'right' },
        { title: 'Ghi', dataIndex: 'rows_written', align: 'right' },
        { title: 'Lỗi', dataIndex: 'error', render: (v: string | null) => v ?? '—', ellipsis: true },
      ]}
    />
  )
}

function LogsDrawer({ runId, onClose }: { runId: string | null; onClose: () => void }) {
  const logs = useQuery({
    queryKey: ['run-logs', runId],
    queryFn: () => getRunLogs(runId!, 200),
    enabled: !!runId,
    refetchInterval: 5000,
  })
  return (
    <Drawer title={`Logs · ${runId?.slice(0, 8) ?? ''}`} width={720} open={!!runId} onClose={onClose}>
      <div style={{ fontFamily: 'monospace', fontSize: 12, whiteSpace: 'pre-wrap' }}>
        {logs.data?.logs.length ? (
          logs.data.logs.map((l) => (
            <div key={l.seq}>
              <Typography.Text type="secondary">{fmtTime(l.ts)} </Typography.Text>
              <Tag color={l.level === 'error' ? 'red' : l.level === 'warn' ? 'gold' : 'blue'}>
                {l.level}
              </Tag>
              {l.step_id ? `[${l.step_id}] ` : ''}
              {l.message}
            </div>
          ))
        ) : (
          <Typography.Text type="secondary">Chưa có log</Typography.Text>
        )}
      </div>
    </Drawer>
  )
}
