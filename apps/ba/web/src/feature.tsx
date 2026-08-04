/** Trang tính năng: workflow stepper + catalog 9 giai đoạn (mỗi mục một
 * "skill" sinh được) + truy vết coverage/pipeline/staleness. */
import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  App,
  Button,
  Card,
  Col,
  Collapse,
  Dropdown,
  Empty,
  Progress,
  Row,
  Select,
  Space,
  Statistic,
  Steps,
  Tag,
  Tooltip,
  Typography,
} from 'antd'
import {
  ArrowLeftOutlined,
  CaretRightOutlined,
  CheckOutlined,
  ExportOutlined,
  EyeOutlined,
  ForwardOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import GenerateModal from './generate'
import DocViewer from './docview'
import { openPreview } from './App'
import { fmtTime, get, post, waitJob, STATUS_COLOR, STATUS_LABEL, type CatalogItem, type Doc, type Phase } from './api'

export default function FeaturePage({
  projectId,
  featureId,
  phases,
  onBack,
}: {
  projectId: number
  featureId: number
  phases: Phase[]
  onBack: () => void
}) {
  const { message } = App.useApp()
  const [feature, setFeature] = useState<any>(null)
  const [docs, setDocs] = useState<Doc[]>([])
  const [projectDocs, setProjectDocs] = useState<Doc[]>([])
  const [trace, setTrace] = useState<any>(null)
  const [wf, setWf] = useState<any>(null)
  const [wfTemplate, setWfTemplate] = useState('full-lifecycle')
  const [wfTemplates, setWfTemplates] = useState<any[]>([])
  const [genItem, setGenItem] = useState<CatalogItem | null>(null)
  const [viewDoc, setViewDoc] = useState<number | null>(null)
  const [running, setRunning] = useState<number | null>(null)

  const load = useCallback(async () => {
    try {
      const [f, d, pd, t, w, wt] = await Promise.all([
        get(`/features/${featureId}`),
        get(`/docs?project_id=${projectId}&feature_id=${featureId}`),
        get(`/docs?project_id=${projectId}&feature_id=project`),
        get(`/features/${featureId}/trace`),
        get(`/features/${featureId}/workflow`),
        get(`/workflow-templates`),
      ])
      setFeature(f.feature)
      setDocs(d.documents ?? [])
      setProjectDocs(pd.documents ?? [])
      setTrace(t)
      setWf(w.workflow)
      setWfTemplates(wt.templates ?? [])
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }, [projectId, featureId, message])

  useEffect(() => {
    load()
  }, [load])

  const docOf = useMemo(() => {
    const m = new Map<string, Doc>()
    for (const d of docs) m.set(`${d.doc_type}/${d.subtype}`, d)
    for (const d of projectDocs) if (!m.has(`${d.doc_type}/${d.subtype}`)) m.set(`${d.doc_type}/${d.subtype}`, d)
    return m
  }, [docs, projectDocs])

  const startWorkflow = async () => {
    try {
      await post(`/features/${featureId}/workflow`, { template: wfTemplate })
      message.success('Đã bắt đầu workflow')
      load()
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }

  const advance = async (index: number, action: string) => {
    if (!wf) return
    try {
      if (action === 'run') {
        setRunning(index)
        const r = await post(`/workflows/${wf.id}/advance`, { index, action: 'run' })
        const result = await waitJob(r.job_id)
        if (result.needs_input) {
          // Chuyển sang modal generate có phỏng vấn cho đúng bước này.
          const step = wf.steps[index]
          const item = phases.flatMap((p) => p.items).find((i) => i.doc_type === step.doc_type && i.subtype === step.subtype)
          if (item) setGenItem(item)
          message.info('AI cần thêm thông tin — trả lời trong hộp thoại')
          return
        }
        message.success('Bước hoàn thành')
      } else {
        await post(`/workflows/${wf.id}/advance`, { index, action })
      }
      load()
    } catch (e: any) {
      message.error(String(e.message ?? e), 6)
    } finally {
      setRunning(null)
    }
  }

  const stepItems = (wf?.steps ?? []).map((s: any, i: number) => {
    const item = phases.flatMap((p) => p.items).find((x) => x.doc_type === s.doc_type && x.subtype === s.subtype)
    const doc = docOf.get(`${s.doc_type}/${s.subtype}`)
    const status = s.status === 'done' ? 'finish' : s.status === 'skipped' ? 'error' : i === wf?.next_step ? 'process' : 'wait'
    return {
      title: (
        <Space size={4}>
          <span className="skill-chip">{item?.skill ?? s.doc_type}</span>
          {s.status === 'skipped' && <Tag>bỏ qua</Tag>}
        </Space>
      ),
      description: (
        <Space direction="vertical" size={2}>
          <span style={{ fontSize: 12 }}>{item?.title ?? s.doc_type}</span>
          <Space size={4} wrap>
            {(s.status === 'pending' || s.status === 'skipped') && (
              <>
                <Tooltip title="AI sinh tài liệu bước này">
                  <Button
                    size="small"
                    type="primary"
                    icon={<CaretRightOutlined />}
                    loading={running === i}
                    onClick={() => advance(i, 'run')}
                  />
                </Tooltip>
                {doc && (
                  <Tooltip title="Đã có tài liệu — đánh dấu xong">
                    <Button size="small" icon={<CheckOutlined />} onClick={() => advance(i, 'done')} />
                  </Tooltip>
                )}
                {s.status === 'pending' && (
                  <Tooltip title="Bỏ qua bước">
                    <Button size="small" icon={<ForwardOutlined />} onClick={() => advance(i, 'skip')} />
                  </Tooltip>
                )}
              </>
            )}
            {s.status === 'done' && (s.doc_id ?? doc?.id) != null && (
              <Button size="small" icon={<EyeOutlined />} onClick={() => setViewDoc(s.doc_id ?? doc!.id)}>
                Xem
              </Button>
            )}
          </Space>
        </Space>
      ),
      status: status as any,
    }
  })

  const cov = trace?.coverage
  const pipe = trace?.pipeline
  const stale = trace?.staleness

  const uncoveredTags = (label: string, arr: string[] | undefined, color: string) =>
    arr && arr.length > 0 ? (
      <div style={{ marginBottom: 6 }}>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {label}:{' '}
        </Typography.Text>
        {arr.map((x) => (
          <Tag key={x} color={color} style={{ fontFamily: 'ui-monospace, Menlo, monospace', fontSize: 11 }}>
            {x}
          </Tag>
        ))}
      </div>
    ) : null

  return (
    <div>
      <Space style={{ marginBottom: 12 }} wrap>
        <Button icon={<ArrowLeftOutlined />} onClick={onBack}>
          Dự án
        </Button>
        <Typography.Title level={4} style={{ margin: 0 }}>
          {feature?.name}
        </Typography.Title>
        <Tag color={feature?.priority === 'P0' ? 'red' : feature?.priority === 'P1' ? 'orange' : 'default'}>
          {feature?.priority}
        </Tag>
        <Tag style={{ fontFamily: 'ui-monospace, Menlo, monospace' }}>{feature?.slug}</Tag>
        <Dropdown
          menu={{
            items: [
              {
                key: 'preview',
                label: 'Trang preview (HTML)',
                onClick: () => openPreview(projectId, featureId),
              },
              {
                key: 'md',
                label: 'Tải gói Markdown',
                onClick: () =>
                  window.open(`/api/export/download?project_id=${projectId}&feature_id=${featureId}&format=md`, '_blank'),
              },
              {
                key: 'html',
                label: 'Tải HTML tự chứa',
                onClick: () =>
                  window.open(`/api/export/download?project_id=${projectId}&feature_id=${featureId}&format=html`, '_blank'),
              },
            ],
          }}
        >
          <Button icon={<ExportOutlined />}>Xuất / Preview</Button>
        </Dropdown>
      </Space>

      <Row gutter={[12, 12]}>
        <Col xs={24} md={8}>
          <Card size="small">
            <Statistic
              title="Truy vết FR → User story"
              value={cov?.coverage_pct ?? '—'}
              suffix={cov?.coverage_pct != null ? '%' : ''}
            />
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {cov ? `${cov.fr_covered}/${cov.fr_total} FR có story · ${cov.oq_open} OQ chưa chốt` : 'chưa có SRS'}
            </Typography.Text>
          </Card>
        </Col>
        <Col xs={24} md={8}>
          <Card size="small">
            <Statistic title="Pipeline 8 chặng" value={pipe ? `${pipe.done}/${pipe.total}` : '—'} />
            <Progress percent={pipe?.pct ?? 0} size="small" showInfo={false} />
            <Space size={2} wrap style={{ marginTop: 4 }}>
              {(pipe?.stages ?? []).map((s: any) => (
                <Tag key={s.stage} color={s.done ? 'green' : 'default'} style={{ fontSize: 10, marginInlineEnd: 2 }}>
                  {s.stage}
                </Tag>
              ))}
            </Space>
          </Card>
        </Col>
        <Col xs={24} md={8}>
          <Card size="small">
            <Statistic title="Độ tươi tài liệu" value={stale?.avg ?? 100} suffix="đ" />
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {(stale?.chain ?? []).length > 0
                ? `${stale.chain.length} tài liệu stale vì upstream đổi sau nó`
                : 'không có stale chain'}
            </Typography.Text>
          </Card>
        </Col>
      </Row>

      <Card
        size="small"
        title="Workflow"
        style={{ marginTop: 12 }}
        extra={
          !wf && (
            <Space>
              <Select
                size="small"
                style={{ width: 260 }}
                value={wfTemplate}
                onChange={setWfTemplate}
                options={wfTemplates.map((t: any) => ({ value: t.key, label: `${t.name}` }))}
              />
              <Button size="small" type="primary" onClick={startWorkflow}>
                Bắt đầu
              </Button>
            </Space>
          )
        }
      >
        {wf ? (
          <>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {wf.name} {wf.status === 'done' && <Tag color="green">hoàn tất</Tag>}
            </Typography.Text>
            <Steps
              direction="horizontal"
              size="small"
              responsive
              items={stepItems}
              style={{ marginTop: 10 }}
              current={wf.next_step ?? wf.steps?.length}
            />
          </>
        ) : (
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description="Chọn một workflow: trọn vòng đời / story trước / prototype trước — hoặc sinh tài liệu tự do ở catalog dưới"
          />
        )}
      </Card>

      {(cov?.fr_uncovered?.length || cov?.us_orphans?.length || cov?.fr_without_test?.length || cov?.us_without_ac?.length) ? (
        <Card size="small" title="Lỗ hổng truy vết (deterministic)" style={{ marginTop: 12 }}>
          {uncoveredTags('FR chưa có story phủ', cov.fr_uncovered, 'orange')}
          {uncoveredTags('FR chưa có test', cov.fr_without_test, 'red')}
          {uncoveredTags('Story mồ côi (không trỏ FR)', cov.us_orphans, 'volcano')}
          {uncoveredTags('Story thiếu AC', cov.us_without_ac, 'gold')}
          {uncoveredTags('Use case chưa có test', cov.uc_without_test, 'magenta')}
        </Card>
      ) : null}

      <Card size="small" title="Tài liệu theo 9 giai đoạn" style={{ marginTop: 12 }}>
        <Collapse
          defaultActiveKey={['2', '4']}
          items={phases.map((p) => ({
            key: String(p.phase),
            label: (
              <Space>
                <span className="phase-badge">{p.phase}</span>
                <b>{p.name}</b>
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {p.items.filter((i) => docOf.has(`${i.doc_type}/${i.subtype}`)).length}/{p.items.length} tài liệu
                </Typography.Text>
              </Space>
            ),
            children: (
              <div>
                {p.items.map((item) => {
                  const doc = docOf.get(`${item.doc_type}/${item.subtype}`)
                  const projScope = item.scope === 'project'
                  return (
                    <Row
                      key={`${item.doc_type}/${item.subtype}`}
                      align="middle"
                      style={{ padding: '6px 4px', borderBottom: '1px solid rgba(255,255,255,0.06)' }}
                      gutter={8}
                    >
                      <Col flex="130px">
                        <span className="skill-chip">{item.skill}</span>
                      </Col>
                      <Col flex="auto">
                        <div style={{ fontSize: 13 }}>
                          {item.title}
                          {projScope && (
                            <Tag style={{ marginLeft: 6, fontSize: 10 }} color="cyan">
                              cấp dự án
                            </Tag>
                          )}
                        </div>
                        <Typography.Text type="secondary" style={{ fontSize: 11.5 }}>
                          {item.desc}
                        </Typography.Text>
                      </Col>
                      <Col>
                        {doc && (
                          <Space size={4}>
                            <Tag color={STATUS_COLOR[doc.status]}>{STATUS_LABEL[doc.status]}</Tag>
                            <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                              v{doc.version} · {fmtTime(doc.updated_at)}
                            </Typography.Text>
                          </Space>
                        )}
                      </Col>
                      <Col>
                        <Space size={4}>
                          {doc && (
                            <Button size="small" icon={<EyeOutlined />} onClick={() => setViewDoc(doc.id)}>
                              Xem
                            </Button>
                          )}
                          <Button
                            size="small"
                            type={doc ? 'default' : 'primary'}
                            icon={<ThunderboltOutlined />}
                            onClick={() => setGenItem(item)}
                          >
                            {doc ? 'Sinh lại' : 'Sinh AI'}
                          </Button>
                        </Space>
                      </Col>
                    </Row>
                  )
                })}
              </div>
            ),
          }))}
        />
      </Card>

      <GenerateModal
        open={genItem != null}
        onClose={() => setGenItem(null)}
        projectId={projectId}
        featureId={featureId}
        item={genItem}
        onDone={(doc, warnings) => {
          load()
          if (warnings.length) message.warning(`Cảnh báo: ${warnings.join('; ')}`, 8)
          setViewDoc(doc.id)
        }}
      />
      <DocViewer
        docId={viewDoc}
        onClose={() => setViewDoc(null)}
        onChanged={load}
        onRegenerate={(doc) => {
          const item = phases.flatMap((p) => p.items).find((i) => i.doc_type === doc.doc_type && i.subtype === doc.subtype)
          if (item) {
            setViewDoc(null)
            setGenItem(item)
          }
        }}
      />
    </div>
  )
}
