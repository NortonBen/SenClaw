import { useCallback, useEffect, useState } from 'react'
import {
  App as AntApp,
  Button,
  Card,
  Drawer,
  Empty,
  List,
  Segmented,
  Space,
  Table,
  Tag,
  theme,
  Typography,
} from 'antd'
import { DeleteOutlined, FileTextOutlined, ReloadOutlined } from '@ant-design/icons'
import {
  api,
  type Evidence,
  type ReportDetail,
  type ReportSummary,
  type RunSummary,
  type SearchOutcome,
} from './api'
import Claims from './Claims'
import EvidenceModal from './EvidenceModal'
import Md from './Md'
import { kindColor } from './theme'

const { Title, Text, Paragraph } = Typography

type Tab = 'reports' | 'runs'

function when(s: string): string {
  return s.slice(0, 16).replace('T', ' ')
}

/** Compact evidence line, reused for a run's detail. */
function EvidenceItem({ e, token }: { e: Evidence; token: { colorTextSecondary: string } }) {
  return (
    <List.Item style={{ paddingInline: 0 }}>
      {e.url ? (
        <a href={e.url} target="_blank" rel="noreferrer" style={{ fontWeight: 600 }}>
          {e.title || e.url}
        </a>
      ) : (
        <Text strong>{e.title || '(không có tiêu đề)'}</Text>
      )}
      <div style={{ color: token.colorTextSecondary, margin: '4px 0', fontSize: 13.5 }}>
        {e.snippet}
      </div>
      <Space size={[4, 4]} wrap>
        {e.domain && <Tag bordered={false}>{e.domain}</Tag>}
        {e.hits.map((h) => (
          <Tag key={h.source_id} bordered={false} color={kindColor(h.kind)}>
            {h.source_id}
          </Tag>
        ))}
        {e.independent_kinds > 1 && (
          <Tag color="success" bordered={false}>
            {e.independent_kinds} loại nguồn
          </Tag>
        )}
      </Space>
    </List.Item>
  )
}

export default function HistoryPage() {
  const { token } = theme.useToken()
  const { message, modal } = AntApp.useApp()

  const [tab, setTab] = useState<Tab>('reports')
  const [reports, setReports] = useState<ReportSummary[]>([])
  const [runs, setRuns] = useState<RunSummary[]>([])
  const [loading, setLoading] = useState(false)

  const [report, setReport] = useState<ReportDetail | null>(null)
  const [run, setRun] = useState<(SearchOutcome & { id: string }) | null>(null)
  const [open, setOpen] = useState(false)
  const [detailLoading, setDetailLoading] = useState(false)
  /** 1-based citation opened from a saved report's `[n]`. */
  const [cite, setCite] = useState<number | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const [r, ru] = await Promise.all([api.reports(), api.runs()])
      setReports(r.reports)
      setRuns(ru.runs)
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [message])

  useEffect(() => {
    load()
  }, [load])

  async function openReport(runId: string) {
    setReport(null)
    setRun(null)
    setOpen(true)
    setDetailLoading(true)
    try {
      setReport(await api.report(runId))
    } catch (e) {
      message.error((e as Error).message)
      setOpen(false)
    } finally {
      setDetailLoading(false)
    }
  }

  async function openRun(id: string) {
    setReport(null)
    setRun(null)
    setOpen(true)
    setDetailLoading(true)
    try {
      setRun(await api.run(id))
    } catch (e) {
      message.error((e as Error).message)
      setOpen(false)
    } finally {
      setDetailLoading(false)
    }
  }

  function confirmDelete(id: string, label: string) {
    modal.confirm({
      title: 'Xoá lần chạy này?',
      content: `“${label}” — xoá cả báo cáo, khẳng định và bằng chứng đã lưu. Không hoàn tác được.`,
      okText: 'Xoá',
      okButtonProps: { danger: true },
      cancelText: 'Huỷ',
      onOk: async () => {
        try {
          await api.deleteRun(id)
          message.success('đã xoá')
          load()
        } catch (e) {
          message.error((e as Error).message)
        }
      },
    })
  }

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Space style={{ width: '100%', justifyContent: 'space-between' }} wrap>
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>
            Lịch sử
          </Title>
          <Paragraph type="secondary" style={{ marginBottom: 0 }}>
            Mở lại báo cáo và các lần tìm đã lưu mà không phải chạy lại.
          </Paragraph>
        </div>
        <Space>
          <Segmented<Tab>
            value={tab}
            onChange={setTab}
            options={[
              { label: `Báo cáo (${reports.length})`, value: 'reports' },
              { label: `Lần chạy (${runs.length})`, value: 'runs' },
            ]}
          />
          <Button icon={<ReloadOutlined />} onClick={load} loading={loading}>
            Tải lại
          </Button>
        </Space>
      </Space>

      {tab === 'reports' && (
        <Card size="small">
          <Table<ReportSummary>
            size="small"
            rowKey="run_id"
            loading={loading}
            dataSource={reports}
            pagination={{ pageSize: 15, hideOnSinglePage: true }}
            locale={{
              emptyText: (
                <Empty
                  image={Empty.PRESENTED_IMAGE_SIMPLE}
                  description="Chưa có báo cáo. Chạy Nghiên cứu để tạo báo cáo đầu tiên."
                />
              ),
            }}
            onRow={(r) => ({ onClick: () => openReport(r.run_id), style: { cursor: 'pointer' } })}
            columns={[
              {
                title: 'Tiêu đề',
                dataIndex: 'title',
                render: (t: string, r) => (
                  <Space>
                    <FileTextOutlined style={{ color: token.colorPrimary }} />
                    <Text strong>{t || r.query}</Text>
                    {r.version > 1 && <Tag>v{r.version}</Tag>}
                  </Space>
                ),
              },
              {
                title: 'Câu hỏi',
                dataIndex: 'query',
                ellipsis: true,
                render: (q: string) => (
                  <Text type="secondary" style={{ fontSize: 12.5 }}>
                    {q}
                  </Text>
                ),
              },
              { title: 'Thời gian', dataIndex: 'created_at', width: 150, render: when },
              {
                title: '',
                key: 'action',
                width: 44,
                render: (_, r) => (
                  <Button
                    size="small"
                    type="text"
                    danger
                    icon={<DeleteOutlined />}
                    onClick={(e) => {
                      e.stopPropagation()
                      confirmDelete(r.run_id, r.title || r.query)
                    }}
                  />
                ),
              },
            ]}
          />
        </Card>
      )}

      {tab === 'runs' && (
        <Card size="small">
          <Table<RunSummary>
            size="small"
            rowKey="id"
            loading={loading}
            dataSource={runs}
            pagination={{ pageSize: 15, hideOnSinglePage: true }}
            locale={{
              emptyText: (
                <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa có lần chạy nào." />
              ),
            }}
            onRow={(r) => ({ onClick: () => openRun(r.id), style: { cursor: 'pointer' } })}
            columns={[
              {
                title: 'Câu hỏi',
                dataIndex: 'query',
                ellipsis: true,
                render: (q: string) => <Text strong>{q}</Text>,
              },
              {
                title: 'Kiểu',
                dataIndex: 'verify_level',
                width: 110,
                render: (v?: string) =>
                  v ? (
                    <Tag color={v === 'research' ? 'purple' : v === 'corroborate' ? 'blue' : 'default'}>
                      {v === 'research' ? 'nghiên cứu' : v === 'corroborate' ? 'kiểm chứng' : v}
                    </Tag>
                  ) : null,
              },
              { title: 'Bằng chứng', dataIndex: 'evidence_count', width: 100 },
              {
                title: 'Thời gian',
                dataIndex: 'created_at',
                width: 150,
                render: when,
              },
              {
                title: '',
                key: 'action',
                width: 44,
                render: (_, r) => (
                  <Button
                    size="small"
                    type="text"
                    danger
                    icon={<DeleteOutlined />}
                    onClick={(e) => {
                      e.stopPropagation()
                      confirmDelete(r.id, r.query)
                    }}
                  />
                ),
              },
            ]}
          />
        </Card>
      )}

      <Drawer
        title={report ? report.title : run ? 'Lần chạy đã lưu' : 'Chi tiết'}
        width={Math.min(760, window.innerWidth)}
        open={open}
        loading={detailLoading}
        onClose={() => setOpen(false)}
      >
        {report && (
          <Space direction="vertical" size={16} style={{ width: '100%' }}>
            <Space size={[4, 4]} wrap>
              <Tag color="blue">{report.claims.length} khẳng định</Tag>
              <Tag color="cyan">{report.run?.evidence?.length ?? 0} bằng chứng</Tag>
              <Tag>{when(report.created_at)}</Tag>
              <Tag>{report.run_id}</Tag>
            </Space>
            <Card size="small">
              <Md onCite={setCite}>{report.body_md}</Md>
            </Card>
            {report.claims.length > 0 && (
              <Card size="small" title="Khẳng định đã kiểm chứng">
                <Claims
                  claims={report.claims}
                  contradictions={report.contradictions}
                  evidence={report.run?.evidence ?? []}
                  onCite={setCite}
                />
              </Card>
            )}
          </Space>
        )}

        {run && (
          <Space direction="vertical" size={12} style={{ width: '100%' }}>
            <Space size={[4, 4]} wrap>
              <Text strong>{run.query}</Text>
              <Tag color="cyan">{run.evidence.length} bằng chứng</Tag>
              <Tag>{run.ms} ms</Tag>
            </Space>
            <List
              itemLayout="vertical"
              dataSource={run.evidence}
              rowKey={(e) => e.id}
              renderItem={(e) => <EvidenceItem e={e} token={token} />}
            />
          </Space>
        )}

        <EvidenceModal
          evidence={report?.run?.evidence ?? []}
          index={cite}
          onClose={() => setCite(null)}
          onNavigate={setCite}
        />
      </Drawer>
    </Space>
  )
}
