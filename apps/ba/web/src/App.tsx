/** BA Studio — shell: chọn dự án → (Tổng quan | Tài liệu dự án | CR | Hỏi đáp)
 * → trang tính năng. Deep-link ?project=&feature=&doc= (host forward query vào
 * iframe). */
import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  App as AntApp,
  Button,
  Card,
  Col,
  Empty,
  Input,
  Layout,
  Modal,
  Row,
  Space,
  Tabs,
  Tag,
  Typography,
} from 'antd'
import { EyeOutlined, MoonOutlined, PlusOutlined, ProjectOutlined, SunOutlined, ThunderboltOutlined } from '@ant-design/icons'
import { fmtTime, get, post, STATUS_COLOR, STATUS_LABEL, type CatalogItem, type Doc, type Phase } from './api'

/** Mở trang preview trong CÙNG một tab đặt tên — bấm lại khi đang mở thì
 * focus tab cũ chứ không mở thêm. */
export function openPreview(projectId: number, featureId?: number) {
  const url = `/api/preview?project_id=${projectId}${featureId != null ? `&feature_id=${featureId}` : ''}`
  const w = window.open(url, `ba-preview-${projectId}`)
  w?.focus()
}
import Dashboard from './dashboard'
import FeaturePage from './feature'
import CrPanel from './crs'
import AskPanel from './ask'
import KgPanel from './kg'
import GenerateModal from './generate'
import DocViewer from './docview'

export default function App({ dark, onToggleTheme }: { dark: boolean; onToggleTheme: () => void }) {
  const { message } = AntApp.useApp()
  const [projects, setProjects] = useState<any[]>([])
  const [phases, setPhases] = useState<Phase[]>([])
  const [projectId, setProjectId] = useState<number | null>(null)
  const [featureId, setFeatureId] = useState<number | null>(null)
  const [features, setFeatures] = useState<any[]>([])
  const [refreshKey, setRefreshKey] = useState(0)
  const [createOpen, setCreateOpen] = useState(false)
  const [pName, setPName] = useState('')
  const [pDesc, setPDesc] = useState('')
  const [pCtx, setPCtx] = useState('')
  const [viewDoc, setViewDoc] = useState<number | null>(null)

  const loadProjects = useCallback(async () => {
    try {
      const r = await get('/projects')
      setProjects(r.projects ?? [])
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }, [message])

  useEffect(() => {
    loadProjects()
    get('/catalog').then((c) => setPhases(c.phases ?? []))
    const q = new URLSearchParams(window.location.search)
    const p = q.get('project')
    const f = q.get('feature')
    const d = q.get('doc')
    if (p) setProjectId(Number(p))
    if (f) setFeatureId(Number(f))
    if (d) setViewDoc(Number(d))
  }, [loadProjects])

  useEffect(() => {
    const q = new URLSearchParams()
    if (projectId != null) q.set('project', String(projectId))
    if (featureId != null) q.set('feature', String(featureId))
    const s = q.toString()
    window.history.replaceState(null, '', s ? `?${s}` : window.location.pathname)
  }, [projectId, featureId])

  useEffect(() => {
    if (projectId == null) return
    get(`/projects/${projectId}/features`)
      .then((r) => setFeatures(r.features ?? []))
      .catch(() => setFeatures([]))
  }, [projectId, refreshKey])

  const createProject = async () => {
    try {
      const r = await post('/projects', { name: pName, description: pDesc, context: pCtx })
      message.success('Đã tạo dự án')
      setCreateOpen(false)
      setPName('')
      setPDesc('')
      setPCtx('')
      await loadProjects()
      setProjectId(r.project.id)
    } catch (e: any) {
      message.error(String(e.message ?? e))
    }
  }

  const bump = () => setRefreshKey((k) => k + 1)

  return (
    <Layout style={{ minHeight: '100vh' }}>
      <Layout.Header style={{ display: 'flex', alignItems: 'center', gap: 12, paddingInline: 20 }}>
        <span
          style={{ fontSize: 17, fontWeight: 800, color: '#b9a8ff', cursor: 'pointer', whiteSpace: 'nowrap' }}
          onClick={() => {
            setFeatureId(null)
            setProjectId(null)
          }}
        >
          📐 BA Studio
        </span>
        {/* Header luôn nền tối ở cả 2 theme — màu chữ cố định, không theo token */}
        <Typography.Text style={{ fontSize: 12, color: '#8f9bb3' }}>
          Trợ lý Business Analyst — 9 giai đoạn · workflow · tài liệu có truy vết
        </Typography.Text>
        <span style={{ flex: 1 }} />
        <Button
          size="small"
          type="text"
          style={{ color: '#b9a8ff' }}
          icon={dark ? <SunOutlined /> : <MoonOutlined />}
          onClick={onToggleTheme}
          title="Đổi giao diện sáng/tối"
        />
      </Layout.Header>
      <Layout.Content style={{ padding: 16, maxWidth: 1280, width: '100%', margin: '0 auto' }}>
        {projectId == null ? (
          <ProjectsHome
            projects={projects}
            onOpen={(id) => setProjectId(id)}
            onCreate={() => setCreateOpen(true)}
          />
        ) : featureId == null ? (
          <ProjectView
            key={projectId}
            projectId={projectId}
            projects={projects}
            features={features}
            phases={phases}
            refreshKey={refreshKey}
            onBump={bump}
            onBack={() => setProjectId(null)}
            onOpenFeature={(id) => setFeatureId(id)}
            onOpenDoc={(id) => setViewDoc(id)}
          />
        ) : (
          <FeaturePage
            key={featureId}
            projectId={projectId}
            featureId={featureId}
            phases={phases}
            onBack={() => {
              setFeatureId(null)
              bump()
            }}
          />
        )}
      </Layout.Content>

      <Modal open={createOpen} onCancel={() => setCreateOpen(false)} onOk={createProject} title="Dự án mới" okText="Tạo">
        <Space direction="vertical" style={{ width: '100%' }}>
          <Input placeholder="Tên dự án" value={pName} onChange={(e) => setPName(e.target.value)} />
          <Input.TextArea rows={2} placeholder="Mô tả ngắn" value={pDesc} onChange={(e) => setPDesc(e.target.value)} />
          <Input.TextArea
            rows={4}
            placeholder="Bối cảnh cho AI: domain, thị trường, nền tảng đích, đối tượng người dùng… (càng kỹ tài liệu càng sát)"
            value={pCtx}
            onChange={(e) => setPCtx(e.target.value)}
          />
        </Space>
      </Modal>
      <DocViewer docId={viewDoc} onClose={() => setViewDoc(null)} onChanged={bump} />
    </Layout>
  )
}

function ProjectsHome({
  projects,
  onOpen,
  onCreate,
}: {
  projects: any[]
  onOpen: (id: number) => void
  onCreate: () => void
}) {
  return (
    <div>
      <Space style={{ marginBottom: 14, justifyContent: 'space-between', width: '100%' }}>
        <Typography.Title level={4} style={{ margin: 0 }}>
          <ProjectOutlined /> Dự án
        </Typography.Title>
        <Button type="primary" icon={<PlusOutlined />} onClick={onCreate}>
          Dự án mới
        </Button>
      </Space>
      {projects.length === 0 ? (
        <Card>
          <Empty description={
            <span>
              Chưa có dự án nào. Tạo dự án rồi đi theo workflow: <code>/prd</code> → bóc tính năng →{' '}
              <code>/brainstorm</code> → <code>/srs</code> → wireframe → story → test.
            </span>
          }>
            <Button type="primary" onClick={onCreate}>Tạo dự án đầu tiên</Button>
          </Empty>
        </Card>
      ) : (
        <Row gutter={[12, 12]}>
          {projects.map((p) => (
            <Col key={p.id} xs={24} sm={12} md={8}>
              <Card hoverable size="small" onClick={() => onOpen(p.id)}>
                <Typography.Title level={5} style={{ marginTop: 0 }}>
                  {p.name}
                </Typography.Title>
                <Typography.Paragraph type="secondary" ellipsis={{ rows: 2 }} style={{ fontSize: 12.5, minHeight: 38 }}>
                  {p.description || '—'}
                </Typography.Paragraph>
                <Space size={6}>
                  <Tag>{p.features} tính năng</Tag>
                  <Tag>{p.documents} tài liệu</Tag>
                  <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                    {fmtTime(p.updated_at)}
                  </Typography.Text>
                </Space>
              </Card>
            </Col>
          ))}
        </Row>
      )}
    </div>
  )
}

function ProjectView({
  projectId,
  projects,
  features,
  phases,
  refreshKey,
  onBump,
  onBack,
  onOpenFeature,
  onOpenDoc,
}: {
  projectId: number
  projects: any[]
  features: any[]
  phases: Phase[]
  refreshKey: number
  onBump: () => void
  onBack: () => void
  onOpenFeature: (id: number) => void
  onOpenDoc: (id: number) => void
}) {
  const { message } = AntApp.useApp()
  const project = projects.find((p) => p.id === projectId)
  const [projectDocs, setProjectDocs] = useState<Doc[]>([])
  const [genItem, setGenItem] = useState<CatalogItem | null>(null)

  useEffect(() => {
    get(`/docs?project_id=${projectId}&feature_id=project`)
      .then((r) => setProjectDocs(r.documents ?? []))
      .catch(() => setProjectDocs([]))
  }, [projectId, refreshKey])

  const projectItems = useMemo(
    () => phases.flatMap((p) => p.items).filter((i) => i.scope === 'project'),
    [phases],
  )
  const docOf = useMemo(() => {
    const m = new Map<string, Doc>()
    for (const d of projectDocs) m.set(`${d.doc_type}/${d.subtype}`, d)
    return m
  }, [projectDocs])

  return (
    <div>
      <Space style={{ marginBottom: 12 }} wrap>
        <Button onClick={onBack}>← Danh sách</Button>
        <Typography.Title level={4} style={{ margin: 0 }}>
          {project?.name ?? `Dự án #${projectId}`}
        </Typography.Title>
        <Button size="small" icon={<EyeOutlined />} onClick={() => openPreview(projectId)}>
          Preview toàn bộ
        </Button>
      </Space>
      <Tabs
        items={[
          {
            key: 'dash',
            label: 'Tổng quan',
            children: (
              <Dashboard projectId={projectId} onOpenFeature={onOpenFeature} onOpenDoc={onOpenDoc} refreshKey={refreshKey} />
            ),
          },
          {
            key: 'pdocs',
            label: 'Tài liệu dự án',
            children: (
              <Card size="small" title="Tài liệu cấp dự án (PRD, roadmap, discovery, biên bản họp, tài liệu dùng chung)">
                {projectItems.map((item) => {
                  const doc = docOf.get(`${item.doc_type}/${item.subtype}`)
                  return (
                    <Row
                      key={`${item.doc_type}/${item.subtype}`}
                      align="middle"
                      gutter={8}
                      style={{ padding: '8px 4px', borderBottom: '1px solid rgba(255,255,255,0.06)' }}
                    >
                      <Col flex="130px">
                        <span className="skill-chip">{item.skill}</span>
                      </Col>
                      <Col flex="auto">
                        <div style={{ fontSize: 13 }}>{item.title}</div>
                        <Typography.Text type="secondary" style={{ fontSize: 11.5 }}>
                          {item.desc}
                        </Typography.Text>
                      </Col>
                      <Col>
                        {doc && (
                          <Space size={4}>
                            <Tag color={STATUS_COLOR[doc.status]}>{STATUS_LABEL[doc.status]}</Tag>
                            <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                              v{doc.version}
                            </Typography.Text>
                          </Space>
                        )}
                      </Col>
                      <Col>
                        <Space size={4}>
                          {doc && (
                            <Button size="small" icon={<EyeOutlined />} onClick={() => onOpenDoc(doc.id)}>
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
              </Card>
            ),
          },
          {
            key: 'crs',
            label: 'Change Request',
            children: <CrPanel projectId={projectId} features={features} onDocChanged={onBump} />,
          },
          {
            key: 'kg',
            label: 'Knowledge Graph',
            children: <KgPanel projectId={projectId} onOpenDoc={onOpenDoc} />,
          },
          {
            key: 'ask',
            label: 'Hỏi đáp',
            children: <AskPanel projectId={projectId} onOpenDoc={onOpenDoc} />,
          },
        ]}
      />
      <GenerateModal
        open={genItem != null}
        onClose={() => setGenItem(null)}
        projectId={projectId}
        featureId={null}
        item={genItem}
        onDone={(doc, warnings) => {
          onBump()
          if (warnings.length) message.warning(`Cảnh báo: ${warnings.join('; ')}`, 8)
          onOpenDoc(doc.id)
        }}
      />
    </div>
  )
}
