// Console một run: poll từng dòng mới, auto-scroll, huỷ + AI giải thích lỗi.
import { useEffect, useRef, useState } from 'react'
import { App as AntApp, Button, Drawer, Modal, Space, Spin, Table, Tag, Typography } from 'antd'
import { api, fmtTime, KIND_LABEL, type Run, type RunLine } from './api'

const { Text } = Typography

export function StatusTag({ status }: { status: Run['status'] }) {
  const map: Record<Run['status'], [string, string]> = {
    running: ['processing', 'đang chạy'],
    success: ['success', 'thành công'],
    failed: ['error', 'thất bại'],
    canceled: ['warning', 'đã huỷ'],
  }
  const [color, label] = map[status] ?? ['default', status]
  return <Tag color={color}>{label}</Tag>
}

export function ConsoleView({ runId }: { runId: number }) {
  const { message } = AntApp.useApp()
  const [run, setRun] = useState<Run | null>(null)
  const [lines, setLines] = useState<RunLine[]>([])
  const [explaining, setExplaining] = useState(false)
  const [explanation, setExplanation] = useState<string | null>(null)
  const afterRef = useRef(0)
  const preRef = useRef<HTMLPreElement>(null)

  useEffect(() => {
    afterRef.current = 0
    setLines([])
    setRun(null)
    setExplanation(null)
    let stop = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const tick = async () => {
      try {
        const r = await api.runGet(runId, afterRef.current)
        if (stop) return
        setRun(r.run)
        if (r.lines.length > 0) {
          afterRef.current = r.next_after
          setLines((prev) => [...prev, ...r.lines])
        }
        if (r.run.status === 'running') timer = setTimeout(tick, 900)
      } catch {
        if (!stop) timer = setTimeout(tick, 2000)
      }
    }
    tick()
    return () => {
      stop = true
      if (timer) clearTimeout(timer)
    }
  }, [runId])

  useEffect(() => {
    const el = preRef.current
    if (!el) return
    // Chỉ tự cuộn khi user đang ở gần đáy.
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 120
    if (nearBottom) el.scrollTop = el.scrollHeight
  }, [lines])

  const explain = async () => {
    setExplaining(true)
    try {
      const r = await api.explain(runId)
      setExplanation(r.text)
    } catch (e) {
      message.error(String(e))
    } finally {
      setExplaining(false)
    }
  }

  return (
    <div>
      <Space style={{ marginBottom: 8 }} wrap>
        {run ? <StatusTag status={run.status} /> : <Spin size="small" />}
        <Text type="secondary">
          {run ? `${KIND_LABEL[run.kind] ?? run.kind} · bắt đầu ${fmtTime(run.started_at)}` : ''}
          {run?.exit_code != null ? ` · exit ${run.exit_code}` : ''}
        </Text>
        {run?.status === 'running' && (
          <Button danger size="small" onClick={() => api.runCancel(runId).catch((e) => message.error(String(e)))}>
            Huỷ run
          </Button>
        )}
        {run && run.status !== 'running' && (
          <Button size="small" loading={explaining} onClick={explain}>
            🤖 AI giải thích
          </Button>
        )}
      </Space>
      <pre className="console" ref={preRef} style={{ maxHeight: '58vh', minHeight: 220 }}>
        {lines.length === 0 ? (
          <span className="l-sys">… chưa có output …</span>
        ) : (
          lines.map((l) => (
            <div key={l.seq} className={`l-${l.stream}`}>
              {l.line || ' '}
            </div>
          ))
        )}
      </pre>
      {explanation != null && (
        <Modal
          open
          title="AI giải thích run này"
          footer={null}
          width={640}
          onCancel={() => setExplanation(null)}
        >
          <div style={{ whiteSpace: 'pre-wrap', lineHeight: 1.6 }}>{explanation}</div>
        </Modal>
      )}
    </div>
  )
}

/** Drawer console toàn cục — mở khi bấm chạy lệnh / cài CLI / clone. */
export function ConsoleDrawer({
  runId,
  onClose,
}: {
  runId: number | null
  onClose: () => void
}) {
  return (
    <Drawer
      open={runId != null}
      onClose={onClose}
      width={760}
      title={runId != null ? `Console — run #${runId}` : ''}
      destroyOnHidden
    >
      {runId != null && <ConsoleView runId={runId} />}
    </Drawer>
  )
}

/** Tab lịch sử run của một workspace. */
export function RunsTab({
  workspaceId,
  onOpenRun,
}: {
  workspaceId: number
  onOpenRun: (id: number) => void
}) {
  const [runs, setRuns] = useState<Run[]>([])
  const [loading, setLoading] = useState(false)

  const load = async () => {
    setLoading(true)
    try {
      const r = await api.runs(workspaceId)
      setRuns(r.runs)
    } finally {
      setLoading(false)
    }
  }
  useEffect(() => {
    load()
    const t = setInterval(load, 4000)
    return () => clearInterval(t)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceId])

  return (
    <Table
      size="small"
      rowKey="id"
      loading={loading && runs.length === 0}
      dataSource={runs}
      pagination={{ pageSize: 12, hideOnSinglePage: true }}
      onRow={(r) => ({ onClick: () => onOpenRun(r.id), style: { cursor: 'pointer' } })}
      columns={[
        { title: '#', dataIndex: 'id', width: 60 },
        {
          title: 'Lệnh',
          dataIndex: 'kind',
          render: (k: string) => <Text code>{KIND_LABEL[k] ?? k}</Text>,
        },
        {
          title: 'Trạng thái',
          dataIndex: 'status',
          width: 120,
          render: (s: Run['status']) => <StatusTag status={s} />,
        },
        { title: 'Bắt đầu', dataIndex: 'started_at', width: 170, render: fmtTime },
        {
          title: 'Thời lượng',
          width: 100,
          render: (_: unknown, r: Run) =>
            r.finished_at ? `${r.finished_at - r.started_at}s` : '…',
        },
      ]}
    />
  )
}
