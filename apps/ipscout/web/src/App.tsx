import { useCallback, useEffect, useState } from 'react'
import {
  Button, Card, ConfigProvider, Empty, Input, Modal, Popconfirm, Row, Col, Select, Space,
  Spin, Table, Tabs, Tag, Tooltip, Typography, message, theme,
} from 'antd'
import {
  ApartmentOutlined, BulbOutlined, DeleteOutlined, GlobalOutlined, PlusOutlined,
  RadarChartOutlined, SafetyCertificateOutlined,
} from '@ant-design/icons'
import viVN from 'antd/locale/vi_VN'
import Profile from './Profile'
import Ports from './Ports'
import Trace from './Trace'
import History from './History'
import {
  api, SEV_COLOR, SEV_LABEL, when,
  type Finding, type Project, type Run, type Severity, type Target,
} from './api'

const { Title, Text, Paragraph } = Typography

const THEME_KEY = 'ipscout.theme'

export default function App() {
  const [dark, setDark] = useState(() => localStorage.getItem(THEME_KEY) === 'dark')

  // Các lớp CSS tự viết đọc biến theo data-theme; antd đọc algorithm. Hai bên
  // phải đổi cùng lúc, nếu không khối .banner sẽ trắng trên nền tối.
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? 'dark' : 'light'
    localStorage.setItem(THEME_KEY, dark ? 'dark' : 'light')
  }, [dark])

  return (
    <ConfigProvider
      locale={viVN}
      theme={{
        algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        // motion:false — animation của antd gây giật trong webview nhúng của desktop app.
        token: { motion: false },
      }}
    >
      <Shell dark={dark} onToggleTheme={() => setDark((v) => !v)} />
    </ConfigProvider>
  )
}

function Shell({ dark, onToggleTheme }: { dark: boolean; onToggleTheme: () => void }) {
  const [projects, setProjects] = useState<Project[]>([])
  const [projectId, setProjectId] = useState<number>(1)
  const [targets, setTargets] = useState<Target[]>([])
  const [targetId, setTargetId] = useState<number | null>(null)
  const [runs, setRuns] = useState<Run[]>([])
  const [findings, setFindings] = useState<Finding[]>([])
  const [profileData, setProfileData] = useState<Record<string, any> | null>(null)
  const [scanData, setScanData] = useState<Record<string, any> | null>(null)
  const [traceData, setTraceData] = useState<Record<string, any> | null>(null)
  const [loading, setLoading] = useState(true)
  const [profiling, setProfiling] = useState(false)
  const [scanning, setScanning] = useState(false)
  const [tracing, setTracing] = useState(false)
  const [reload, setReload] = useState(0)

  const loadProjects = useCallback(async () => {
    setProjects(await api.projects())
  }, [])

  const loadTargets = useCallback(async () => {
    const list = await api.targets(projectId)
    setTargets(list)
    setTargetId((prev) => (prev != null && list.some((t) => t.id === prev) ? prev : list[0]?.id ?? null))
    setLoading(false)
  }, [projectId])

  useEffect(() => {
    void loadProjects()
  }, [loadProjects])
  useEffect(() => {
    void loadTargets()
  }, [loadTargets])

  // Nạp lại toàn bộ trạng thái của mục tiêu đang chọn. Hồ sơ và kết quả quét lấy
  // từ lần chạy **đã hoàn tất** gần nhất của từng loại — lần chạy hỏng không
  // được đè lên kết quả tốt trước đó.
  useEffect(() => {
    if (targetId == null) {
      setRuns([])
      setFindings([])
      setProfileData(null)
      setScanData(null)
      setTraceData(null)
      return
    }
    void (async () => {
      const list = await api.runs(targetId)
      setRuns(list)
      setFindings(await api.findings(targetId))
      const ok = (layer: string) =>
        list.find((r) => r.layer === layer && r.status === 'done')?.summary ?? null
      setProfileData(ok('profile'))
      setScanData(ok('ports'))
      setTraceData(ok('trace'))
    })()
  }, [targetId, reload])

  const runProfile = async () => {
    if (targetId == null) return
    setProfiling(true)
    const r = await api.profile(targetId)
    setProfiling(false)
    if (r.ok === false) message.error(r.error ?? 'lập hồ sơ thất bại')
    else message.success('Đã lập hồ sơ')
    setReload((k) => k + 1)
  }

  const runScan = async (profile: string, ports: string) => {
    if (targetId == null) return
    setScanning(true)
    const r = await api.scan(targetId, profile, ports || undefined)
    setScanning(false)
    if (r.ok === false) message.error(r.error ?? 'quét thất bại')
    else message.success(`Xong: ${r.result?.open ?? 0}/${r.result?.scanned ?? 0} cổng mở`)
    setReload((k) => k + 1)
  }

  const runTrace = async (maxHops: number) => {
    if (targetId == null) return
    setTracing(true)
    const r = await api.trace(targetId, maxHops)
    setTracing(false)
    if (r.ok === false) message.error(r.error ?? 'traceroute thất bại')
    else message.success(`Xong: ${r.result?.responded_hops ?? 0}/${r.result?.total_hops ?? 0} hop trả lời`)
    setReload((k) => k + 1)
  }

  if (loading) {
    return (
      <div style={{ padding: 80, textAlign: 'center' }}>
        <Spin size="large" />
      </div>
    )
  }

  return (
    <div className="wrap">
      <Row align="middle" style={{ marginBottom: 4 }}>
        <Col flex="auto">
          <Space align="center">
            <RadarChartOutlined style={{ fontSize: 22, color: '#1677ff' }} />
            <Title level={3} style={{ margin: 0 }}>
              Điều Tra IP
            </Title>
          </Space>
        </Col>
        <Col>
          <Tooltip title={dark ? 'chuyển sang nền sáng' : 'chuyển sang nền tối'}>
            <Button type="text" icon={<BulbOutlined />} onClick={onToggleTheme} />
          </Tooltip>
        </Col>
      </Row>
      <Paragraph type="secondary" style={{ marginBottom: 16 }}>
        Một IP <strong>là ai</strong>, <strong>ở đâu</strong>, traffic <strong>đi qua đâu</strong>,{' '}
        <strong>cổng nào mở</strong> và <strong>chạy gì</strong>. Lớp hồ sơ đọc cơ sở dữ liệu công
        khai (không chạm mục tiêu); lớp quét cổng gửi gói TCP thật, có ghi log ở phía máy chủ —
        chỉ dùng với hạ tầng bạn có quyền kiểm tra.
      </Paragraph>

      <TargetBar
        projects={projects}
        projectId={projectId}
        onProject={setProjectId}
        targets={targets}
        targetId={targetId}
        onTarget={setTargetId}
        onChanged={async () => {
          await loadProjects()
          await loadTargets()
        }}
        onProfile={runProfile}
        profiling={profiling}
      />

      {targetId == null ? (
        <Card>
          <Empty description="Chưa có mục tiêu nào. Thêm một IP hoặc tên miền để bắt đầu." />
        </Card>
      ) : (
        <Tabs
          defaultActiveKey="profile"
          items={[
            {
              key: 'profile',
              label: <Space size={6}><GlobalOutlined />Hồ sơ</Space>,
              children: <Profile data={profileData} />,
            },
            {
              key: 'ports',
              label: (
                <Space size={6}>
                  <SafetyCertificateOutlined />
                  Cổng & Dịch vụ
                  {scanData?.open != null && <Tag>{scanData.open}</Tag>}
                </Space>
              ),
              children: <Ports data={scanData} scanning={scanning} onScan={runScan} />,
            },
            {
              key: 'trace',
              label: (
                <Space size={6}>
                  <ApartmentOutlined />
                  Đường đi
                  {traceData?.responded_hops != null && (
                    <Tag>{traceData.responded_hops}/{traceData.total_hops}</Tag>
                  )}
                </Space>
              ),
              children: <Trace data={traceData} running={tracing} onRun={runTrace} />,
            },
            {
              key: 'findings',
              label: (
                <Space size={6}>
                  Phát hiện
                  {findings.length > 0 && <Tag>{findings.length}</Tag>}
                </Space>
              ),
              children: <Findings rows={findings} />,
            },
            {
              key: 'history',
              label: 'Lịch sử',
              children: <History runs={runs} targetId={targetId} />,
            },
          ]}
        />
      )}
    </div>
  )
}

function TargetBar(props: {
  projects: Project[]
  projectId: number
  onProject: (id: number) => void
  targets: Target[]
  targetId: number | null
  onTarget: (id: number) => void
  onChanged: () => Promise<void>
  onProfile: () => void
  profiling: boolean
}) {
  const [adding, setAdding] = useState(false)
  const [input, setInput] = useState('')
  const [label, setLabel] = useState('')
  const [newProject, setNewProject] = useState('')

  const target = props.targets.find((t) => t.id === props.targetId) ?? null

  const add = async () => {
    if (!input.trim()) return
    const r = await api.addTarget(input.trim(), props.projectId, label.trim())
    if (r.ok === false) message.error(r.error ?? 'không thêm được')
    else {
      message.success(`Đã thêm ${r.host}`)
      setInput('')
      setLabel('')
      setAdding(false)
      await props.onChanged()
    }
  }

  const addProject = async () => {
    if (!newProject.trim()) return
    const r = await api.addProject(newProject.trim())
    if (r.ok === false) message.error(r.error ?? 'không tạo được project')
    else {
      setNewProject('')
      await props.onChanged()
      props.onProject(r.id)
    }
  }

  return (
    <>
      <Card size="small" style={{ marginBottom: 16 }}>
        <Space wrap>
          <Select
            value={props.projectId}
            onChange={props.onProject}
            style={{ minWidth: 190 }}
            options={props.projects.map((p) => ({
              value: p.id,
              label: `${p.name} (${p.targets})`,
            }))}
            popupRender={(menu) => (
              <>
                {menu}
                <div style={{ display: 'flex', gap: 6, padding: 8 }}>
                  <Input
                    size="small"
                    placeholder="project mới"
                    value={newProject}
                    onChange={(e) => setNewProject(e.target.value)}
                    onPressEnter={addProject}
                  />
                  <Button size="small" type="primary" onClick={addProject}>
                    Tạo
                  </Button>
                </div>
              </>
            )}
          />

          <Select
            value={props.targetId ?? undefined}
            onChange={props.onTarget}
            style={{ minWidth: 280 }}
            placeholder="chọn mục tiêu"
            options={props.targets.map((t) => ({
              value: t.id,
              label: `${t.host}${t.label ? ` — ${t.label}` : ''}`,
            }))}
          />

          <Button icon={<PlusOutlined />} onClick={() => setAdding(true)}>
            Thêm mục tiêu
          </Button>

          {target && (
            <>
              <Button type="primary" loading={props.profiling} onClick={props.onProfile}>
                Lập hồ sơ
              </Button>
              <Popconfirm
                title="Xoá mục tiêu này?"
                description="Toàn bộ lịch sử điều tra của nó cũng mất."
                okText="Xoá"
                cancelText="Thôi"
                onConfirm={async () => {
                  await api.deleteTarget(target.id)
                  await props.onChanged()
                }}
              >
                <Button type="text" danger icon={<DeleteOutlined />} />
              </Popconfirm>
            </>
          )}
        </Space>
      </Card>

      <Modal
        title="Thêm mục tiêu"
        open={adding}
        onOk={add}
        onCancel={() => setAdding(false)}
        okText="Thêm"
        cancelText="Thôi"
      >
        <Space direction="vertical" style={{ width: '100%' }}>
          <Input
            placeholder="1.2.3.4 · example.com · https://example.com/x"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onPressEnter={add}
            autoFocus
          />
          <Input placeholder="nhãn (tuỳ chọn)" value={label} onChange={(e) => setLabel(e.target.value)} />
          <Text type="secondary" style={{ fontSize: 12 }}>
            App tự rút host từ URL. Thêm xong quét được ngay — app không kiểm sở hữu, hãy chắc chắn
            đây là hạ tầng của bạn hoặc bạn có uỷ quyền trước khi bấm “Quét cổng”.
          </Text>
        </Space>
      </Modal>
    </>
  )
}

function Findings({ rows }: { rows: Finding[] }) {
  if (rows.length === 0) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description="Chưa có phát hiện nào. Chạy “Lập hồ sơ” hoặc quét cổng trước."
      />
    )
  }
  return (
    <div className="scroll-x">
      <Table<Finding>
        size="small"
        rowKey="id"
        dataSource={rows}
        pagination={{ pageSize: 15, hideOnSinglePage: true }}
        expandable={{
          rowExpandable: (r) => Boolean(r.detail || r.fix),
          expandedRowRender: (r) => (
            <Space direction="vertical" size={6}>
              {r.detail && <Text>{r.detail}</Text>}
              {r.fix && (
                <Text type="success">
                  <strong>Cách sửa:</strong> {r.fix}
                </Text>
              )}
              <Text type="secondary" style={{ fontSize: 12 }}>
                Thấy lần đầu {when(r.first_seen)} · gần nhất {when(r.last_seen)}
              </Text>
            </Space>
          ),
        }}
        columns={[
          {
            title: 'Mức',
            dataIndex: 'severity',
            width: 130,
            render: (s: Severity) => <Tag color={SEV_COLOR[s]}>{SEV_LABEL[s]}</Tag>,
          },
          { title: 'Nhóm', dataIndex: 'category', width: 110 },
          { title: 'Phát hiện', dataIndex: 'title' },
          { title: 'Lần chạy', dataIndex: 'run_id', width: 90, render: (v: number) => `#${v}` },
        ]}
      />
    </div>
  )
}
