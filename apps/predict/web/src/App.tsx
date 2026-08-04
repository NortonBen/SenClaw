import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Alert, App as AntApp, Button, Card, Col, ConfigProvider, Empty, Flex, Input,
  InputNumber, Layout, Modal, Row, Segmented, Select, Space, Statistic, Table, Tabs, Tag, Typography, theme,
} from 'antd'
import {
  AimOutlined, DeleteOutlined, DesktopOutlined, EditOutlined, ExperimentOutlined, MoonOutlined,
  PlusOutlined, ReadOutlined, ReloadOutlined, RocketOutlined, SunOutlined, SyncOutlined,
} from '@ant-design/icons'
import dayjs from 'dayjs'
import { api } from './api'

const { Text, Title, Paragraph } = Typography

const fmtTs = (ts?: number) => (ts ? dayjs(ts * 1000).format('DD/MM HH:mm') : '—')
const pct = (p?: number) => (p == null ? '—' : `${Math.round(p * 100)}%`)
const SOURCE_ICON: Record<string, string> = {
  gold: '🪙', weather: '🌦', lottery: '🎰', football: '⚽', manual: '📋',
}

function Sparkline({ series, color }: { series: [any, number][]; color: string }) {
  if (!series?.length) return <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa đủ dữ liệu" />
  const w = 560
  const h = 110
  const ps = series.map((p) => p[1])
  const lo = Math.min(...ps)
  const hi = Math.max(...ps)
  const span = hi - lo || 1
  const pts = series
    .map((p, i) => `${(i / Math.max(series.length - 1, 1)) * w},${h - ((p[1] - lo) / span) * (h - 10) - 5}`)
    .join(' ')
  return (
    <svg width="100%" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none" style={{ display: 'block' }}>
      <polyline points={pts} fill="none" stroke={color} strokeWidth={2} />
    </svg>
  )
}

type ThemeMode = 'system' | 'light' | 'dark'

/// Theme: chọn Sáng / Tối / Theo hệ thống. Lưu localStorage (áp dụng ngay,
/// không cần chờ backend) và đồng bộ vào settings để mở máy khác vẫn nhớ.
function useThemeMode(): [ThemeMode, (m: ThemeMode) => void, boolean] {
  const [mode, setMode] = useState<ThemeMode>(
    () => (localStorage.getItem('predict-theme') as ThemeMode) || 'system',
  )
  const [systemDark, setSystemDark] = useState(
    () => window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false,
  )
  useEffect(() => {
    const mq = window.matchMedia?.('(prefers-color-scheme: dark)')
    if (!mq) return
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches)
    mq.addEventListener('change', onChange)
    return () => mq.removeEventListener('change', onChange)
  }, [])
  useEffect(() => {
    // Nếu server đã lưu lựa chọn thì lấy về (lần đầu mở trên máy mới).
    if (!localStorage.getItem('predict-theme')) {
      api.settings().then((s) => {
        if (s?.theme && s.theme !== 'system') setMode(s.theme as ThemeMode)
      }).catch(() => {})
    }
  }, [])
  const update = (m: ThemeMode) => {
    setMode(m)
    localStorage.setItem('predict-theme', m)
    api.saveSettings({ theme: m }).catch(() => {})
  }
  const isDark = mode === 'dark' || (mode === 'system' && systemDark)
  return [mode, update, isDark]
}

export default function App() {
  const [mode, setMode, isDark] = useThemeMode()
  useEffect(() => {
    // Nền trang khớp theme để không bị viền trắng/đen khi cuộn.
    document.body.style.background = isDark ? '#000' : '#f5f5f5'
    document.documentElement.style.colorScheme = isDark ? 'dark' : 'light'
  }, [isDark])
  return (
    <ConfigProvider
      theme={{
        algorithm: isDark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: { colorPrimary: '#7c4dff' },
      }}
    >
      <AntApp>
        <Main themeMode={mode} setThemeMode={setMode} />
      </AntApp>
    </ConfigProvider>
  )
}

function Main({ themeMode, setThemeMode }: { themeMode: ThemeMode; setThemeMode: (m: ThemeMode) => void }) {
  const { message } = AntApp.useApp()
  const [tab, setTab] = useState('overview')
  const [busy, setBusy] = useState(false)
  const [topics, setTopics] = useState<any[]>([])
  const [selTopic, setSelTopic] = useState<number | null>(null)

  const loadTopics = useCallback(async () => {
    const r = await api.topics()
    setTopics(r.topics || [])
    return r.topics || []
  }, [])
  useEffect(() => {
    loadTopics().then((ts: any[]) => {
      if (ts.length && selTopic == null) setSelTopic(ts[0].id)
    })
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const openTopic = (id: number) => {
    setSelTopic(id)
    setTab('topics')
  }

  const tick = async () => {
    setBusy(true)
    try {
      await api.tick()
      message.success('Đã cập nhật dữ liệu nguồn + sync các chủ đề')
      loadTopics()
    } catch {
      message.error('Cập nhật lỗi')
    } finally {
      setBusy(false)
    }
  }

  return (
    <Layout style={{ minHeight: '100%' }}>
      <Layout.Header style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <Title level={4} style={{ color: '#fff', margin: 0, flex: 1 }}>
          🔮 Siêu Dự Đoán
        </Title>
        <Segmented
          size="small"
          value={themeMode}
          onChange={(v) => setThemeMode(v as ThemeMode)}
          options={[
            { value: 'light', icon: <SunOutlined />, title: 'Sáng' },
            { value: 'dark', icon: <MoonOutlined />, title: 'Tối' },
            { value: 'system', icon: <DesktopOutlined />, title: 'Theo hệ thống' },
          ]}
        />
        <Button icon={<ReloadOutlined />} onClick={tick} loading={busy} ghost>
          Cập nhật dữ liệu
        </Button>
      </Layout.Header>
      <Layout.Content style={{ padding: 16 }}>
        <Tabs
          activeKey={tab}
          onChange={setTab}
          items={[
            {
              key: 'overview',
              label: (<span><AimOutlined /> Tổng quan</span>),
              children: <OverviewTab topics={topics} onOpenTopic={openTopic} onChanged={loadTopics} />,
            },
            {
              key: 'topics',
              label: (<span><ExperimentOutlined /> Chủ đề</span>),
              children: (
                <TopicHub
                  topics={topics}
                  sel={selTopic}
                  setSel={setSelTopic}
                  onChanged={loadTopics}
                />
              ),
            },
            {
              key: 'ledger',
              label: (<span><AimOutlined /> Sổ dự đoán</span>),
              children: <LedgerTab />,
            },
            {
              key: 'method',
              label: (<span><ReadOutlined /> Tri thức</span>),
              children: <MethodTab />,
            },
            {
              key: 'settings',
              label: 'Cài đặt',
              children: <SettingsTab />,
            },
          ]}
        />
      </Layout.Content>
    </Layout>
  )
}

// ---- Tổng quan: lưới chủ đề + sổ điểm + hoạt động ----

function OverviewTab({ topics, onOpenTopic, onChanged }: { topics: any[]; onOpenTopic: (id: number) => void; onChanged: () => void }) {
  const [overview, setOverview] = useState<any>(null)
  const [showBuilder, setShowBuilder] = useState(false)
  useEffect(() => {
    api.overview().then(setOverview).catch(() => {})
  }, [])
  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card
        size="small"
        title="Chủ đề dự đoán của bạn"
        extra={<Button type="primary" icon={<PlusOutlined />} onClick={() => setShowBuilder(true)}>Build chủ đề</Button>}
      >
        {topics.length ? (
          <Row gutter={[12, 12]}>
            {topics.map((t) => (
              <Col xs={24} sm={12} lg={8} xl={6} key={t.id}>
                <Card size="small" hoverable onClick={() => onOpenTopic(t.id)}>
                  <Space direction="vertical" size={4} style={{ width: '100%' }}>
                    <Flex align="center" gap={8}>
                      <span style={{ fontSize: 22 }}>{SOURCE_ICON[t.source?.kind] || '📋'}</span>
                      <Text strong ellipsis style={{ flex: 1 }}>{t.name}</Text>
                    </Flex>
                    <Flex gap={6} wrap>
                      <Tag>{t.records} bản ghi</Tag>
                      <Tag>{t.rules} quy luật</Tag>
                      {!!t.docs && <Tag>{t.docs} tài liệu</Tag>}
                      {t.source?.kind && t.source.kind !== 'manual' && <Tag color="cyan">auto-sync</Tag>}
                    </Flex>
                  </Space>
                </Card>
              </Col>
            ))}
          </Row>
        ) : (
          <Empty description={'Chưa có chủ đề nào — bấm "Build chủ đề" để bắt đầu (giá vàng, thời tiết, xổ số, bóng đá hoặc tự thiết lập)'} />
        )}
      </Card>
      <Card size="small" title="Sổ dự đoán — điểm số">
        <ScoreTable data={overview?.ledger || []} />
      </Card>
      <Card size="small" title="Hoạt động gần đây">
        <Table
          size="small"
          rowKey={(r: any) => `${r.created_at}-${r.text}`}
          dataSource={overview?.activity || []}
          pagination={{ pageSize: 8 }}
          showHeader={false}
          columns={[
            { dataIndex: 'created_at', width: 110, render: fmtTs },
            { dataIndex: 'kind', width: 90, render: (v: string) => <Tag>{v}</Tag> },
            { dataIndex: 'text' },
          ]}
        />
      </Card>
      <BuilderModal open={showBuilder} onClose={() => setShowBuilder(false)} onCreated={(id) => { onChanged(); onOpenTopic(id) }} />
    </Space>
  )
}

function ScoreTable({ data }: { data: any[] }) {
  const cols = useMemo(
    () => [
      { title: 'Domain', dataIndex: 'domain', render: (v: string) => <Tag color="purple">{v}</Tag> },
      { title: 'Tổng', dataIndex: 'total', width: 70 },
      { title: 'Đang mở', dataIndex: 'open', width: 90 },
      { title: 'Đã chấm', dataIndex: 'resolved', width: 90 },
      { title: 'Đúng', dataIndex: 'hits', width: 70 },
      { title: 'Accuracy', dataIndex: 'accuracy', width: 100, render: (v: number) => (v == null ? '—' : pct(v)) },
      { title: 'Brier TB', dataIndex: 'avg_brier', width: 100, render: (v: number) => v ?? '—' },
    ],
    [],
  )
  return <Table size="small" rowKey="domain" dataSource={data} columns={cols} pagination={false} />
}

// ---- Công cụ build chủ đề (templates) ----

function BuilderModal({ open, onClose, onCreated }: { open: boolean; onClose: () => void; onCreated: (id: number) => void }) {
  const { message } = AntApp.useApp()
  const [mode, setMode] = useState<'free' | 'template'>('free')
  const [templates, setTemplates] = useState<any[]>([])
  const [sel, setSel] = useState<string>('gold')
  const [params, setParams] = useState<Record<string, string>>({})
  const [places, setPlaces] = useState<string[]>([])
  const [leagues, setLeagues] = useState<any[]>([])
  const [busy, setBusy] = useState(false)

  // Free-form: mô tả → AI thiết kế → proposal SỬA ĐƯỢC trước khi tạo.
  const [wish, setWish] = useState('')
  const [designed, setDesigned] = useState(false)
  const [pName, setPName] = useState('')
  const [pDesc, setPDesc] = useState('')
  const [statics, setStatics] = useState<{ name: string; value: string }[]>([{ name: 'vị trí', value: '' }])
  const [guide, setGuide] = useState('')
  const [fields, setFields] = useState<{ name: string; kind: string }[]>([
    { name: 'ngày', kind: 'date' },
    { name: 'giá trị', kind: 'number' },
  ])
  const [questions, setQuestions] = useState<string[]>([])

  useEffect(() => {
    if (open) {
      api.topicTemplates().then((r) => setTemplates(r.templates || []))
      api.settings().then((s) => {
        setPlaces(s.suggested_places || [])
        setLeagues(s.suggested_leagues || [])
      })
    }
  }, [open])

  const design = async () => {
    setBusy(true)
    try {
      const r = await api.topicDesign(wish)
      if (r.error) return message.error(r.error)
      const p = r.proposal || {}
      setPName(p.name || '')
      setPDesc(p.description || '')
      setStatics((p.static || []).map((x: any) => ({ name: x.name, value: x.value || '' })))
      setGuide(p.guide || '')
      setFields((p.fields || []).map((f: any) => ({ name: f.name, kind: f.kind })))
      setQuestions(p.sample_questions || [])
      setDesigned(true)
      message.success('AI đã thiết kế — sửa thoải mái rồi bấm Build')
    } finally {
      setBusy(false)
    }
  }

  const create = async () => {
    setBusy(true)
    try {
      let r: any
      if (mode === 'template' && sel !== 'blank') {
        r = await api.topicFromTemplate(sel, params)
      } else {
        if (!pName.trim()) return message.error('Đặt tên chủ đề (hoặc bấm "AI thiết kế" trước)')
        r = await api.topicCreate({
          name: pName,
          description: pDesc,
          fields: fields.filter((f) => f.name.trim()),
          static: statics.filter((x) => x.name.trim() && x.value.trim()),
          guide,
        })
      }
      if (r.error) return message.error(r.error)
      message.success(`Đã build chủ đề${r.synced ? ` (+${r.synced} bản ghi từ connector)` : ''}`)
      onClose()
      onCreated(r.id)
    } finally {
      setBusy(false)
    }
  }

  const staticEditor = (
    <Space direction="vertical" style={{ width: '100%' }} size={6}>
      {statics.map((x, i) => (
        <Flex gap={8} key={i}>
          <Input style={{ width: 200 }} placeholder="tên (vd vị trí)" value={x.name}
            onChange={(e) => setStatics(statics.map((y, j) => (j === i ? { ...y, name: e.target.value } : y)))} />
          <Input style={{ flex: 1 }} placeholder="giá trị cố định (vd Đà Lạt)" value={x.value}
            onChange={(e) => setStatics(statics.map((y, j) => (j === i ? { ...y, value: e.target.value } : y)))} />
          <Button type="text" icon={<DeleteOutlined />} onClick={() => setStatics(statics.filter((_, j) => j !== i))} />
        </Flex>
      ))}
      <Button size="small" icon={<PlusOutlined />} onClick={() => setStatics([...statics, { name: '', value: '' }])}>Thêm thông số tĩnh</Button>
    </Space>
  )

  const guideEditor = (
    <Input.TextArea
      autoSize={{ minRows: 3, maxRows: 8 }}
      placeholder="Tài liệu hướng dẫn phân tích / prompt cho chủ đề này — AI sẽ tuân thủ khi phân tích, rút quy luật và dự đoán. VD: nhiệt độ giảm theo độ cao; gió mạnh thì ít sương muối…"
      value={guide}
      onChange={(e) => setGuide(e.target.value)}
    />
  )

  const fieldEditor = (
    <Space direction="vertical" style={{ width: '100%' }} size={6}>
      {fields.map((f, i) => (
        <Flex gap={8} key={i}>
          <Input style={{ width: 240 }} placeholder="tên trường" value={f.name} onChange={(e) => setFields(fields.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
          <Select
            style={{ width: 130 }}
            value={f.kind}
            onChange={(v) => setFields(fields.map((x, j) => (j === i ? { ...x, kind: v } : x)))}
            options={[
              { value: 'text', label: 'chữ' }, { value: 'number', label: 'số' },
              { value: 'date', label: 'ngày' }, { value: 'bool', label: 'có/không' },
            ]}
          />
          <Button type="text" icon={<DeleteOutlined />} onClick={() => setFields(fields.filter((_, j) => j !== i))} />
        </Flex>
      ))}
      <Button size="small" icon={<PlusOutlined />} onClick={() => setFields([...fields, { name: '', kind: 'text' }])}>Thêm trường</Button>
    </Space>
  )

  return (
    <Modal open={open} onCancel={onClose} onOk={create} okText="Build" confirmLoading={busy} title="Công cụ build chủ đề" width={680}>
      <Space direction="vertical" size={12} style={{ width: '100%' }}>
        <Segmented
          value={mode}
          onChange={(v) => setMode(v as any)}
          options={[
            { label: '✨ Tự do — mô tả, AI thiết kế', value: 'free' },
            { label: 'Template có sẵn', value: 'template' },
          ]}
        />

        {mode === 'free' && (
          <>
            <Flex gap={8}>
              <Input.TextArea
                autoSize={{ minRows: 2, maxRows: 4 }}
                style={{ flex: 1 }}
                placeholder={'Mô tả chủ đề bạn muốn theo dõi & dự đoán — bất kỳ thứ gì.\nVD: "theo dõi doanh số shop mỗi ngày, dự đoán ngày nào bán chạy" · "cân nặng của tôi, dự đoán 2 tháng giảm 5kg"'}
                value={wish}
                onChange={(e) => setWish(e.target.value)}
              />
              <Button type="primary" loading={busy} disabled={!wish.trim()} onClick={design} icon={<RocketOutlined />}>
                AI thiết kế
              </Button>
            </Flex>
            {(designed || pName) && (
              <Card size="small" title="Bản thiết kế (sửa thoải mái)">
                <Space direction="vertical" style={{ width: '100%' }} size={8}>
                  <Flex gap={8}>
                    <Input style={{ width: 240 }} placeholder="Tên chủ đề" value={pName} onChange={(e) => setPName(e.target.value)} />
                    <Input style={{ flex: 1 }} placeholder="Mô tả" value={pDesc} onChange={(e) => setPDesc(e.target.value)} />
                  </Flex>
                  <Text strong>Cấu hình TĨNH — bối cảnh cố định (vị trí, thông số)</Text>
                  {staticEditor}
                  <Text strong>Tài liệu hướng dẫn phân tích (prompt của chủ đề)</Text>
                  {guideEditor}
                  <Text strong>Trường ĐỘNG — dữ liệu nhập theo thời gian (ngày, giờ, nhiệt độ, gió…)</Text>
                  {fieldEditor}
                  {!!questions.length && (
                    <div>
                      <Text type="secondary">Câu hỏi dự đoán gợi ý: </Text>
                      {questions.map((q, i) => <Tag key={i} style={{ whiteSpace: 'normal', marginBottom: 4 }}>{q}</Tag>)}
                    </div>
                  )}
                </Space>
              </Card>
            )}
            {!designed && !pName && (
              <Space direction="vertical" size={4}>
                <Text type="secondary">…hoặc bỏ qua AI và tự thiết lập trường ngay:</Text>
                <Flex gap={8}>
                  <Input style={{ width: 240 }} placeholder="Tên chủ đề" value={pName} onChange={(e) => setPName(e.target.value)} />
                  <Input style={{ flex: 1 }} placeholder="Mô tả" value={pDesc} onChange={(e) => setPDesc(e.target.value)} />
                </Flex>
                <Text strong>Cấu hình TĨNH — bối cảnh cố định</Text>
                {staticEditor}
                <Text strong>Tài liệu hướng dẫn phân tích (tuỳ chọn)</Text>
                {guideEditor}
                <Text strong>Trường ĐỘNG — dữ liệu theo thời gian</Text>
                {fieldEditor}
              </Space>
            )}
          </>
        )}

        {mode === 'template' && (
          <>
            <Row gutter={[8, 8]}>
              {templates.filter((t) => t.key !== 'blank').map((t) => (
                <Col span={12} key={t.key}>
                  <Card
                    size="small"
                    hoverable
                    onClick={() => { setSel(t.key); setParams({}) }}
                    style={sel === t.key ? { borderColor: '#7c4dff' } : undefined}
                  >
                    <Text strong>{t.icon} {t.name}</Text>
                    <Paragraph type="secondary" style={{ margin: 0, fontSize: 12 }}>{t.description}</Paragraph>
                  </Card>
                </Col>
              ))}
            </Row>
            {sel === 'weather' && (
              <Space direction="vertical" style={{ width: '100%' }} size={6}>
                <Input
                  style={{ maxWidth: 320 }}
                  placeholder="Địa điểm bất kỳ (Buôn Ma Thuột, Tokyo…)"
                  value={params['city'] ?? ''}
                  onChange={(e) => setParams({ ...params, city: e.target.value })}
                />
                <Flex gap={4} wrap>
                  {places.slice(0, 8).map((c) => (
                    <Tag key={c} style={{ cursor: 'pointer' }} onClick={() => setParams({ ...params, city: c })}>{c}</Tag>
                  ))}
                </Flex>
                <Text type="secondary">Nơi chưa có sẵn sẽ được tìm toạ độ tự động (Open-Meteo, miễn phí).</Text>
              </Space>
            )}
            {sel === 'football' && (
              <Space direction="vertical" style={{ width: '100%' }} size={6}>
                <Flex gap={8}>
                  <Input
                    style={{ width: 150 }}
                    placeholder="id giải (vd 4344)"
                    value={params['league'] ?? ''}
                    onChange={(e) => setParams({ ...params, league: e.target.value })}
                  />
                  <Input
                    style={{ maxWidth: 240 }}
                    placeholder="Tên hiển thị (tuỳ chọn)"
                    value={params['league_name'] ?? ''}
                    onChange={(e) => setParams({ ...params, league_name: e.target.value })}
                  />
                </Flex>
                <Flex gap={4} wrap>
                  {leagues.map((l: any) => (
                    <Tag key={l.id} style={{ cursor: 'pointer' }} onClick={() => setParams({ ...params, league: l.id, league_name: l.name })}>{l.name}</Tag>
                  ))}
                </Flex>
                <Text type="secondary">Tra id giải tại thesportsdb.com.</Text>
              </Space>
            )}
          </>
        )}
      </Space>
    </Modal>
  )
}

// ---- Chủ đề: chọn + dashboard riêng ----

function TopicHub({ topics, sel, setSel, onChanged }: { topics: any[]; sel: number | null; setSel: (v: number | null) => void; onChanged: () => void }) {
  const [showBuilder, setShowBuilder] = useState(false)
  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Flex gap={8} align="center" wrap>
        <Select
          style={{ minWidth: 260 }}
          placeholder="Chọn chủ đề"
          value={sel}
          onChange={setSel}
          options={topics.map((t) => ({
            value: t.id,
            label: `${SOURCE_ICON[t.source?.kind] || '📋'} ${t.name} (${t.records})`,
          }))}
        />
        <Button icon={<PlusOutlined />} onClick={() => setShowBuilder(true)}>Build chủ đề</Button>
      </Flex>
      {sel != null ? (
        <TopicDashboard key={sel} id={sel} onChanged={onChanged} onDeleted={() => { setSel(null); onChanged() }} />
      ) : (
        <Empty description="Chọn hoặc build một chủ đề" />
      )}
      <BuilderModal open={showBuilder} onClose={() => setShowBuilder(false)} onCreated={(id) => { onChanged(); setSel(id) }} />
    </Space>
  )
}

function TopicDashboard({ id, onChanged, onDeleted }: { id: number; onChanged: () => void; onDeleted: () => void }) {
  const { message, modal } = AntApp.useApp()
  const [d, setD] = useState<any>(null)
  const [busy, setBusy] = useState(false)

  // data section
  const [form, setForm] = useState<Record<string, string>>({})
  const [importText, setImportText] = useState('')
  const [q, setQ] = useState('')
  const [records, setRecords] = useState<any[]>([])

  // edit section
  const [editing, setEditing] = useState(false)

  // ai section
  const [analysis, setAnalysis] = useState('')
  const [newRule, setNewRule] = useState('')
  const [question, setQuestion] = useState('')
  const [dueDays, setDueDays] = useState<number | null>(30)
  const [askResult, setAskResult] = useState<any>(null)

  const loadDash = useCallback(async () => {
    const r = await api.topicDashboard(id)
    if (!r.error) setD(r)
  }, [id])
  const loadRecords = useCallback(async (query: string) => {
    const r = await api.topicRecords(id, query)
    setRecords(r.records || [])
  }, [id])
  useEffect(() => {
    loadDash()
    loadRecords('')
  }, [loadDash, loadRecords])

  const withBusy = (fn: () => Promise<void>) => async () => {
    setBusy(true)
    try { await fn() } finally { setBusy(false) }
  }

  if (!d) return <Empty description="Đang tải dashboard…" />
  const fields: any[] = d.fields || []
  const isConnector = d.source?.kind && d.source.kind !== 'manual'
  const seriesEntries: [string, [any, number][]][] = Object.entries(d.series || {}) as any

  const recordCols = [
    { title: '#', dataIndex: 'id', width: 60 },
    ...fields.map((f: any) => ({
      title: f.name,
      render: (_: any, r: any) => {
        const v = r.data?.[f.name]
        return v === undefined ? '—' : typeof v === 'boolean' ? (v ? '✓' : '✗') : String(v)
      },
    })),
    { title: 'Lúc', dataIndex: 'created_at', width: 100, render: fmtTs },
    {
      title: '',
      width: 50,
      render: (_: any, r: any) => (
        <Button size="small" type="text" icon={<DeleteOutlined />} onClick={async () => {
          await api.topicDeleteRecord(id, r.id)
          loadRecords(q); loadDash(); onChanged()
        }} />
      ),
    },
  ]

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      {/* Header */}
      <Card size="small">
        <Flex gap={10} align="center" wrap>
          <span style={{ fontSize: 26 }}>{SOURCE_ICON[d.source?.kind] || '📋'}</span>
          <div style={{ flex: 1, minWidth: 220 }}>
            <Text strong style={{ fontSize: 16 }}>{d.name}</Text>
            <div><Text type="secondary">{d.description}</Text></div>
          </div>
          <Tag color={isConnector ? 'cyan' : 'default'}>
            {isConnector
              ? `nguồn: ${d.source.kind}${d.source.city ? ` · ${d.source.city}` : ''}${d.source.league ? ` · giải ${d.source.league}` : ''}`
              : 'nhập tay'}
          </Tag>
          <Tag>{d.records_total} bản ghi</Tag>
          <Button icon={<EditOutlined />} onClick={() => setEditing(true)}>Sửa</Button>
          {isConnector && (
            <Button icon={<SyncOutlined />} loading={busy} onClick={withBusy(async () => {
              const r = await api.topicSync(id)
              if (r.error) message.error(r.error)
              else message.success(`Sync: +${r.appended} bản ghi`)
              loadDash(); loadRecords(q); onChanged()
            })}>Sync</Button>
          )}
          <Button danger icon={<DeleteOutlined />} onClick={() =>
            modal.confirm({
              title: `Xoá chủ đề "${d.name}"?`,
              content: 'Xoá cả dữ liệu + quy luật.',
              okType: 'danger',
              onOk: async () => { await api.topicDelete(id); onDeleted() },
            })
          } />
        </Flex>
      </Card>

      {/* Bối cảnh TĨNH + tài liệu hướng dẫn */}
      {(Object.keys(d.static || {}).length > 0 || d.guide) && (
        <Card size="small" title="Bối cảnh cố định & hướng dẫn phân tích">
          <Space direction="vertical" size={6} style={{ width: '100%' }}>
            <Flex gap={6} wrap>
              {Object.entries(d.static || {}).map(([k, v]) => (
                <Tag key={k}><Text type="secondary">{k}:</Text> <Text strong>{String(v)}</Text></Tag>
              ))}
            </Flex>
            {d.guide && <Paragraph type="secondary" style={{ margin: 0, whiteSpace: 'pre-wrap' }}>{d.guide}</Paragraph>}
          </Space>
        </Card>
      )}

      {/* Nguồn dữ liệu — cấu hình riêng của chủ đề này */}
      {isConnector && (d.source.kind === 'weather' || d.source.kind === 'football') && (
        <SourceConfig
          topicId={id}
          source={d.source}
          onSaved={() => { loadDash(); loadRecords(q); onChanged() }}
        />
      )}

      {/* Stats + score */}
      <Row gutter={[12, 12]}>
        {Object.entries(d.stats || {}).slice(0, 3).map(([name, st]: any) => (
          <Col xs={12} md={6} key={name}>
            <Card size="small">
              <Statistic
                title={`${name} (mới nhất)`}
                value={st.latest ?? (st.true_share != null ? pct(st.true_share) : '—')}
                suffix={st.mean != null ? <Text type="secondary" style={{ fontSize: 12 }}>TB {st.mean}</Text> : undefined}
              />
              {st.min != null && <Text type="secondary" style={{ fontSize: 12 }}>min {st.min} · max {st.max} · {st.count} điểm</Text>}
            </Card>
          </Col>
        ))}
        <Col xs={12} md={6}>
          <Card size="small">
            <Statistic
              title="Sổ điểm chủ đề"
              value={d.score?.accuracy != null ? pct(d.score.accuracy) : '—'}
              suffix={<Text type="secondary" style={{ fontSize: 12 }}>{d.score ? `Brier ${d.score.avg_brier ?? '—'} · ${d.score.resolved} đã chấm` : 'chưa có dự đoán'}</Text>}
            />
          </Card>
        </Col>
      </Row>

      {/* Charts */}
      {seriesEntries.length > 0 && (
        <Row gutter={[12, 12]}>
          {seriesEntries.slice(0, 3).map(([name, series], i) => (
            <Col xs={24} md={seriesEntries.length === 1 ? 24 : 12} lg={8} key={name} flex={seriesEntries.length === 1 ? undefined : '1 1 320px'}>
              <Card size="small" title={`${name} theo thời gian (${series.length} điểm)`}>
                <Sparkline series={series.slice(-120)} color={['#faad14', '#7c4dff', '#13c2c2'][i % 3]} />
              </Card>
            </Col>
          ))}
        </Row>
      )}

      <Row gutter={[16, 16]}>
        {/* Data */}
        <Col xs={24} xl={13}>
          <Card size="small" title="Dữ liệu — nhập tay / import / tìm kiếm">
            <Space direction="vertical" style={{ width: '100%' }}>
              <Flex gap={8} wrap>
                {fields.map((f: any) => (
                  <Input
                    key={f.name}
                    style={{ width: 165 }}
                    addonBefore={f.name}
                    placeholder={f.kind === 'date' ? 'YYYY-MM-DD' : f.kind === 'bool' ? 'có/không' : f.kind === 'number' ? 'số' : ''}
                    value={form[f.name] ?? ''}
                    onChange={(e) => setForm({ ...form, [f.name]: e.target.value })}
                  />
                ))}
                <Button type="primary" onClick={async () => {
                  const data: Record<string, any> = {}
                  for (const [k, v] of Object.entries(form)) if (v !== '') data[k] = v
                  const r = await api.topicAddRecord(id, data)
                  if (r.error) return message.error(r.error)
                  setForm({}); loadRecords(q); loadDash(); onChanged()
                }}>Thêm</Button>
              </Flex>
              <Input.TextArea rows={2} placeholder="Import: dán CSV (dòng đầu là tên trường) hoặc JSON [{...}]" value={importText} onChange={(e) => setImportText(e.target.value)} />
              <Flex gap={8}>
                <Button disabled={!importText.trim()} onClick={async () => {
                  const body = importText.trim().startsWith('[') ? { records: JSON.parse(importText) } : { csv: importText }
                  const r = await api.topicImport(id, body)
                  if (r.error) return message.error(r.error)
                  message.success(`Import ${r.imported} bản ghi${r.errors?.length ? `, ${r.errors.length} lỗi` : ''}`)
                  setImportText(''); loadRecords(q); loadDash(); onChanged()
                }}>Import</Button>
                <Input.Search style={{ maxWidth: 280 }} placeholder="Tìm trong dữ liệu…" value={q} onChange={(e) => setQ(e.target.value)} onSearch={(v) => loadRecords(v)} allowClear />
              </Flex>
              <Table size="small" rowKey="id" dataSource={records} columns={recordCols as any} pagination={{ pageSize: 8 }} />
            </Space>
          </Card>
        </Col>

        {/* Tài liệu / thông tin ngoài số liệu */}
        <Col xs={24} xl={13}>
          <DocsCard topicId={id} docs={d.docs || []} fields={fields} onChanged={loadDash} />
        </Col>

        {/* AI: rules + ask + open predictions */}
        <Col xs={24} xl={11}>
          <Card
            size="small"
            title="AI phân tích & quy luật siêu dự đoán"
            extra={
              <Space>
                <Button size="small" loading={busy} onClick={withBusy(async () => {
                  const r = await api.topicAnalyze(id)
                  if (r.error) message.error(r.error)
                  else setAnalysis(r.analysis || '')
                })}>Phân tích AI</Button>
                <Button size="small" loading={busy} onClick={withBusy(async () => {
                  const r = await api.topicDeriveRules(id)
                  if (r.error) message.error(r.error)
                  else { loadDash(); message.success('Đã rút quy luật') }
                })}>Rút quy luật</Button>
              </Space>
            }
          >
            {analysis && <Paragraph style={{ whiteSpace: 'pre-wrap' }}>{analysis}</Paragraph>}
            {(d.rules || []).map((r: any) => (
              <Flex key={r.id} gap={8} align="center" style={{ padding: '2px 0' }}>
                <Tag color={r.source === 'ai' ? 'purple' : r.source === 'lesson' ? 'gold' : 'blue'}>{Math.round(r.confidence * 100)}%</Tag>
                <Text style={{ flex: 1 }}>{r.rule}</Text>
                <Button size="small" type="text" icon={<DeleteOutlined />} onClick={async () => {
                  await api.topicDeleteRule(id, r.id); loadDash()
                }} />
              </Flex>
            ))}
            {!(d.rules || []).length && <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa có quy luật" />}
            <Input
              style={{ marginTop: 8 }}
              placeholder="Thêm quy luật thủ công… (Enter)"
              value={newRule}
              onChange={(e) => setNewRule(e.target.value)}
              onPressEnter={async () => {
                if (!newRule.trim()) return
                await api.topicAddRule(id, newRule, 0.5)
                setNewRule(''); loadDash()
              }}
            />
          </Card>

          <Card size="small" style={{ marginTop: 16 }} title="Siêu dự báo: điều này có xảy ra không?">
            <Space direction="vertical" style={{ width: '100%' }}>
              <Input.TextArea rows={2} placeholder="VD: Tuần tới giá có vượt mốc 125 không?" value={question} onChange={(e) => setQuestion(e.target.value)} />
              <Flex gap={8}>
                <InputNumber style={{ width: 170 }} min={0} max={365} value={dueDays} onChange={setDueDays} addonBefore="Biết KQ sau" addonAfter="ngày" />
                <Button type="primary" icon={<RocketOutlined />} loading={busy} disabled={!question.trim()} onClick={withBusy(async () => {
                  const r = await api.ask(String(id), question, dueDays ?? 30)
                  if (r.error) message.error(r.error)
                  else { setAskResult(r); loadDash() }
                })}>Siêu dự đoán</Button>
              </Flex>
              {askResult && <AskTrace r={askResult} />}
            </Space>
          </Card>

          <EditTopicModal
            open={editing}
            topic={d}
            onClose={() => setEditing(false)}
            onSaved={() => { setEditing(false); loadDash(); loadRecords(q); onChanged() }}
          />
          {!!(d.open_predictions || []).length && (
            <Card size="small" style={{ marginTop: 16 }} title="Dự đoán đang mở của chủ đề">
              {(d.open_predictions || []).map((p: any) => (
                <Flex key={p.id} gap={8} align="center" style={{ padding: '3px 0' }}>
                  <Tag>#{p.id}</Tag>
                  <Text style={{ flex: 1 }} ellipsis>{p.subject}</Text>
                  <Text type="secondary">{pct(p.probs?.yes)}</Text>
                  <ResolveInline id={p.id} probs={p.probs} onDone={() => loadDash()} />
                </Flex>
              ))}
            </Card>
          )}
        </Col>
      </Row>
    </Space>
  )
}

/// Kho TÀI LIỆU / thông tin ngoài số liệu: ghi chú, tin tức, giải thích — gắn
/// theo ngày hoặc theo giá trị/trường. Được đưa vào mọi lần AI phân tích & dự đoán.
function DocsCard({ topicId, docs, fields, onChanged }: { topicId: number; docs: any[]; fields: any[]; onChanged: () => void }) {
  const { message } = AntApp.useApp()
  const [title, setTitle] = useState('')
  const [content, setContent] = useState('')
  const [date, setDate] = useState('')
  const [ref, setRef] = useState('')
  const [q, setQ] = useState('')
  const [busy, setBusy] = useState(false)

  const shown = q.trim()
    ? docs.filter((d) => `${d.title} ${d.content} ${d.date} ${d.ref}`.toLowerCase().includes(q.trim().toLowerCase()))
    : docs

  const add = async () => {
    if (!title.trim() && !content.trim()) return message.error('Nhập tiêu đề hoặc nội dung')
    setBusy(true)
    try {
      const r = await api.topicDocAdd(topicId, { title, content, date, ref })
      if (r.error) return message.error(r.error)
      setTitle(''); setContent(''); setRef('')
      message.success('Đã lưu tài liệu')
      onChanged()
    } finally { setBusy(false) }
  }

  return (
    <Card size="small" title={`Tài liệu & thông tin ngoài số liệu (${docs.length})`}>
      <Space direction="vertical" style={{ width: '100%' }} size={8}>
        <Flex gap={8} wrap>
          <Input style={{ flex: 1, minWidth: 200 }} placeholder="Tiêu đề (vd Đợt lạnh tăng cường)" value={title} onChange={(e) => setTitle(e.target.value)} />
          <Input style={{ width: 150 }} placeholder="Ngày YYYY-MM-DD" value={date} onChange={(e) => setDate(e.target.value)} />
          <Select
            style={{ width: 170 }}
            allowClear
            showSearch
            placeholder="Gắn với giá trị/trường"
            value={ref || undefined}
            onChange={(v) => setRef(v || '')}
            options={fields.map((f: any) => ({ value: f.name, label: f.name }))}
          />
        </Flex>
        <Input.TextArea
          autoSize={{ minRows: 2, maxRows: 6 }}
          placeholder="Nội dung — tin tức, ghi chú, giải thích bối cảnh cho ngày/giá trị này…"
          value={content}
          onChange={(e) => setContent(e.target.value)}
        />
        <Flex gap={8}>
          <Button type="primary" loading={busy} onClick={add}>Lưu tài liệu</Button>
          <Input.Search style={{ maxWidth: 260 }} placeholder="Tìm tài liệu…" value={q} onChange={(e) => setQ(e.target.value)} allowClear />
        </Flex>
        {shown.length ? (
          <Space direction="vertical" size={6} style={{ width: '100%' }}>
            {shown.slice(0, 20).map((doc: any) => (
              <Flex key={doc.id} gap={8} align="start" style={{ padding: '4px 0', borderTop: '1px solid rgba(127,127,127,.15)' }}>
                <Space direction="vertical" size={2} style={{ flex: 1 }}>
                  <Flex gap={6} wrap align="center">
                    <Text strong>{doc.title || '(không tiêu đề)'}</Text>
                    {doc.date && <Tag color="blue">{doc.date}</Tag>}
                    {doc.ref && <Tag>{doc.ref}</Tag>}
                  </Flex>
                  {doc.content && <Text type="secondary" style={{ whiteSpace: 'pre-wrap' }}>{doc.content}</Text>}
                </Space>
                <Button size="small" type="text" icon={<DeleteOutlined />} onClick={async () => {
                  await api.topicDocDelete(topicId, doc.id)
                  onChanged()
                }} />
              </Flex>
            ))}
          </Space>
        ) : (
          <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="Chưa có tài liệu — thêm tin tức/ghi chú để AI dùng khi phân tích & dự đoán" />
        )}
      </Space>
    </Card>
  )
}

/// Cấu hình NGUỒN của riêng chủ đề: địa điểm (weather) hoặc giải (football).
function SourceConfig({ topicId, source, onSaved }: { topicId: number; source: any; onSaved: () => void }) {
  const { message } = AntApp.useApp()
  const [city, setCity] = useState(source.city || '')
  const [league, setLeague] = useState(source.league || '')
  const [leagueName, setLeagueName] = useState('')
  const [busy, setBusy] = useState(false)
  const [suggest, setSuggest] = useState<any>(null)
  useEffect(() => {
    api.settings().then(setSuggest).catch(() => {})
  }, [])

  const save = async (patch: any) => {
    setBusy(true)
    try {
      const r = await api.topicSourceUpdate(topicId, patch)
      if (r.error) return message.error(r.error)
      message.success(`Đã đổi nguồn${r.appended ? ` (+${r.appended} bản ghi)` : ''}`)
      onSaved()
    } finally { setBusy(false) }
  }

  return (
    <Card size="small" title="Nguồn dữ liệu của chủ đề này">
      {source.kind === 'weather' ? (
        <Space direction="vertical" style={{ width: '100%' }} size={8}>
          <Flex gap={8} wrap>
            <Input
              style={{ maxWidth: 300 }}
              placeholder="Địa điểm bất kỳ (Buôn Ma Thuột, Tokyo…)"
              value={city}
              onChange={(e) => setCity(e.target.value)}
              onPressEnter={() => save({ city })}
            />
            <Button type="primary" loading={busy} disabled={!city.trim() || city === source.city} onClick={() => save({ city })}>
              Đổi địa điểm
            </Button>
          </Flex>
          <Flex gap={4} wrap>
            {(suggest?.suggested_places || []).slice(0, 8).map((p: string) => (
              <Tag key={p} style={{ cursor: 'pointer' }} onClick={() => { setCity(p); save({ city: p }) }}>{p}</Tag>
            ))}
          </Flex>
          <Text type="secondary">
            Toạ độ lấy tự động qua Open-Meteo Geocoding (miễn phí, không cần key).
            Đổi địa điểm KHÔNG xoá bản ghi cũ — chúng vẫn thuộc nơi trước đó (xem cột ghi chú).
          </Text>
        </Space>
      ) : (
        <Space direction="vertical" style={{ width: '100%' }} size={8}>
          <Flex gap={8} wrap>
            <Input style={{ width: 140 }} placeholder="id giải" value={league} onChange={(e) => setLeague(e.target.value)} />
            <Input style={{ maxWidth: 220 }} placeholder="Tên hiển thị (tuỳ chọn)" value={leagueName} onChange={(e) => setLeagueName(e.target.value)} />
            <Button type="primary" loading={busy} disabled={!league.trim() || (league === source.league && !leagueName)} onClick={() => save({ league, league_name: leagueName })}>
              Đổi giải
            </Button>
          </Flex>
          <Flex gap={4} wrap>
            {(suggest?.suggested_leagues || []).map((l: any) => (
              <Tag key={l.id} style={{ cursor: 'pointer' }} onClick={() => { setLeague(l.id); save({ league: l.id, league_name: l.name }) }}>{l.name}</Tag>
            ))}
          </Flex>
          <Text type="secondary">Tra id giải tại thesportsdb.com (vd 4328 = Ngoại hạng Anh, 4344 = V-League).</Text>
        </Space>
      )}
    </Card>
  )
}

/// Sửa chủ đề: tên, mô tả, và schema trường (thêm/xoá/đổi kiểu).
function EditTopicModal({ open, topic, onClose, onSaved }: { open: boolean; topic: any; onClose: () => void; onSaved: () => void }) {
  const { message } = AntApp.useApp()
  const [name, setName] = useState('')
  const [desc, setDesc] = useState('')
  const [fields, setFields] = useState<{ name: string; kind: string }[]>([])
  const [statics, setStatics] = useState<{ name: string; value: string }[]>([])
  const [guide, setGuide] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (open) {
      setName(topic.name || '')
      setDesc(topic.description || '')
      setFields((topic.fields || []).map((f: any) => ({ name: f.name, kind: f.kind })))
      setStatics(Object.entries(topic.static || {}).map(([k, v]) => ({ name: k, value: String(v) })))
      setGuide(topic.guide || '')
    }
  }, [open, topic])

  const save = async () => {
    const clean = fields.filter((f) => f.name.trim())
    if (!name.trim()) return message.error('Tên chủ đề không được trống')
    if (!clean.length) return message.error('Cần ít nhất một trường dữ liệu')
    setBusy(true)
    try {
      const r = await api.topicUpdate(topic.id, {
        name, description: desc, fields: clean,
        static: statics.filter((x) => x.name.trim() && x.value.trim()),
        guide,
      })
      if (r.error) return message.error(r.error)
      message.success(r.predictions_moved ? `Đã lưu (${r.predictions_moved} dự đoán cũ chuyển theo tên mới)` : 'Đã lưu chủ đề')
      onSaved()
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal open={open} onCancel={onClose} onOk={save} okText="Lưu" confirmLoading={busy} title={`Sửa chủ đề "${topic.name}"`} width={640}>
      <Space direction="vertical" size={10} style={{ width: '100%' }}>
        <Flex gap={8}>
          <Input style={{ width: 240 }} placeholder="Tên chủ đề" value={name} onChange={(e) => setName(e.target.value)} />
          <Input style={{ flex: 1 }} placeholder="Mô tả" value={desc} onChange={(e) => setDesc(e.target.value)} />
        </Flex>
        <Text strong>Cấu hình TĨNH — bối cảnh cố định (vị trí, thông số không đổi)</Text>
        <Space direction="vertical" style={{ width: '100%' }} size={6}>
          {statics.map((x, i) => (
            <Flex gap={8} key={i}>
              <Input style={{ width: 200 }} placeholder="tên (vd vị trí)" value={x.name}
                onChange={(e) => setStatics(statics.map((y, j) => (j === i ? { ...y, name: e.target.value } : y)))} />
              <Input style={{ flex: 1 }} placeholder="giá trị" value={x.value}
                onChange={(e) => setStatics(statics.map((y, j) => (j === i ? { ...y, value: e.target.value } : y)))} />
              <Button type="text" icon={<DeleteOutlined />} onClick={() => setStatics(statics.filter((_, j) => j !== i))} />
            </Flex>
          ))}
          <Button size="small" icon={<PlusOutlined />} onClick={() => setStatics([...statics, { name: '', value: '' }])}>Thêm thông số tĩnh</Button>
        </Space>
        <Text strong>Tài liệu hướng dẫn phân tích (prompt của chủ đề)</Text>
        <Input.TextArea autoSize={{ minRows: 3, maxRows: 8 }} value={guide} onChange={(e) => setGuide(e.target.value)}
          placeholder="AI sẽ tuân thủ hướng dẫn này khi phân tích, rút quy luật và dự đoán chủ đề." />
        <Text strong>Trường ĐỘNG — dữ liệu nhập theo thời gian</Text>
        <Text type="secondary">Sửa tên/kiểu, thêm hoặc bớt. Bản ghi cũ được giữ nguyên; trường bị bỏ chỉ thôi hiển thị.</Text>
        {fields.map((f, i) => (
          <Flex gap={8} key={i}>
            <Input style={{ width: 240 }} placeholder="tên trường" value={f.name} onChange={(e) => setFields(fields.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
            <Select
              style={{ width: 130 }}
              value={f.kind}
              onChange={(v) => setFields(fields.map((x, j) => (j === i ? { ...x, kind: v } : x)))}
              options={[
                { value: 'text', label: 'chữ' }, { value: 'number', label: 'số' },
                { value: 'date', label: 'ngày' }, { value: 'bool', label: 'có/không' },
              ]}
            />
            <Button type="text" icon={<DeleteOutlined />} onClick={() => setFields(fields.filter((_, j) => j !== i))} />
          </Flex>
        ))}
        <Button size="small" icon={<PlusOutlined />} onClick={() => setFields([...fields, { name: '', kind: 'text' }])}>Thêm trường</Button>
        {topic.source?.kind && topic.source.kind !== 'manual' && (
          <Alert type="warning" showIcon message="Chủ đề có connector — nếu đổi tên trường khỏi schema gốc, connector sẽ vẫn ghi theo tên cũ ở lần sync sau." />
        )}
      </Space>
    </Modal>
  )
}

// ---- Sổ dự đoán ----

function LedgerTab() {
  const { message } = AntApp.useApp()
  const [ledger, setLedger] = useState<any[]>([])
  const [score, setScore] = useState<any>(null)
  const [ledDomain, setLedDomain] = useState('')
  const [ledStatus, setLedStatus] = useState('')
  const [newSubject, setNewSubject] = useState('')
  const [newP, setNewP] = useState<number | null>(0.7)
  const [newDue, setNewDue] = useState<number | null>(7)

  const load = useCallback(async (domain: string, status: string) => {
    const r = await api.ledger(domain, status)
    setLedger(r.predictions || [])
    setScore(await api.ledgerScore())
  }, [])
  useEffect(() => {
    load(ledDomain, ledStatus)
  }, [ledDomain, ledStatus, load])

  const domains = useMemo(() => {
    const ds = new Set<string>((score?.summary || []).map((x: any) => x.domain))
    return ['', ...Array.from(ds)]
  }, [score])

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card size="small" title="Điểm số & Calibration (Brier: 0 = hoàn hảo, 2 = sai hoàn toàn)">
        <ScoreTable data={score?.summary || []} />
        {!!score?.calibration?.length && (
          <Table
            size="small"
            style={{ marginTop: 12 }}
            rowKey="band"
            dataSource={score.calibration}
            pagination={false}
            columns={[
              { title: 'Mức tự tin', dataIndex: 'band' },
              { title: 'Số dự đoán', dataIndex: 'n' },
              { title: 'Tỷ lệ đúng thực tế', dataIndex: 'hit_rate', render: (v: number) => pct(v) },
            ]}
          />
        )}
      </Card>
      <Card size="small" title="Ghi dự đoán mới">
        <Flex gap={8} wrap>
          <Input style={{ width: 320 }} placeholder="VD: Việt Nam thắng Thái Lan trận tới" value={newSubject} onChange={(e) => setNewSubject(e.target.value)} />
          <InputNumber style={{ width: 140 }} min={0.01} max={0.99} step={0.05} value={newP} onChange={setNewP} addonBefore="P(yes)" />
          <InputNumber style={{ width: 140 }} min={0} max={365} value={newDue} onChange={setNewDue} addonBefore="Hạn (ngày)" />
          <Button type="primary" onClick={async () => {
            if (!newSubject.trim() || newP == null) return
            const r = await api.ledgerMake({ subject: newSubject, p: newP, due_days: newDue ?? 7 })
            if (r.error) message.error(r.error)
            else { message.success(`Đã ghi sổ #${r.id}`); setNewSubject(''); load(ledDomain, ledStatus) }
          }}>Ghi sổ</Button>
        </Flex>
      </Card>
      <Card
        size="small"
        title="Các dự đoán"
        extra={
          <Space>
            <Select style={{ width: 170 }} value={ledDomain} onChange={setLedDomain}
              options={domains.map((d) => ({ value: d, label: d || 'Mọi domain' }))} />
            <Select style={{ width: 130 }} value={ledStatus} onChange={setLedStatus}
              options={[{ value: '', label: 'Tất cả' }, { value: 'open', label: 'Đang mở' }, { value: 'resolved', label: 'Đã chấm' }]} />
          </Space>
        }
      >
        <Table
          size="small"
          rowKey="id"
          dataSource={ledger}
          pagination={{ pageSize: 15 }}
          columns={[
            { title: '#', dataIndex: 'id', width: 60 },
            { title: 'Domain', dataIndex: 'domain', width: 130, render: (v: string) => <Tag color="purple">{v}</Tag> },
            { title: 'Dự đoán', dataIndex: 'subject' },
            {
              title: 'Xác suất',
              width: 200,
              render: (_: any, r: any) =>
                r.probs && typeof r.probs === 'object'
                  ? Object.entries(r.probs).map(([k, v]: any) => <Tag key={k}>{k}: {pct(v)}</Tag>)
                  : '—',
            },
            { title: 'Hạn', dataIndex: 'due_at', width: 110, render: fmtTs },
            {
              title: 'Kết quả',
              width: 210,
              render: (_: any, r: any) =>
                r.resolved_at ? (
                  <Space size={4}>
                    <Tag color={r.correct ? 'green' : 'red'}>{r.outcome}</Tag>
                    <Text type="secondary">brier {r.brier?.toFixed?.(3)}</Text>
                  </Space>
                ) : (
                  <ResolveInline id={r.id} probs={r.probs} onDone={() => load(ledDomain, ledStatus)} />
                ),
            },
          ]}
        />
      </Card>
    </Space>
  )
}

// ---- Cài đặt ----

function SettingsTab() {
  const { message } = AntApp.useApp()
  const [settings, setSettings] = useState<any>(null)
  const [sources, setSources] = useState<any>(null)
  const [mode, setMode] = useState<'auto' | 'manual'>('auto')
  const [picked, setPicked] = useState<string[]>([])
  const [busy, setBusy] = useState(false)

  const load = useCallback(async () => {
    const s = await api.settings()
    setSettings(s)
    const sel: string = s.search_mcp || 'auto'
    setMode(sel === 'auto' ? 'auto' : 'manual')
    setPicked(sel === 'auto' ? [] : sel.split(',').map((x: string) => x.trim()).filter(Boolean))
    setSources(await api.searchSources())
  }, [])
  useEffect(() => { load() }, [load])

  const save = async () => {
    setBusy(true)
    try {
      const value = mode === 'auto' ? 'auto' : picked
      const s = await api.saveSettings({ search_mcp: value })
      setSettings(s)
      setSources(await api.searchSources())
      message.success('Đã lưu nguồn tìm kiếm')
    } finally { setBusy(false) }
  }

  const active = settings?.active_sources || {}
  const list: any[] = sources?.sources || []

  return (
    <Space direction="vertical" size={16} style={{ maxWidth: 860, width: '100%' }}>
      <Card
        size="small"
        title="Nguồn tìm kiếm cho siêu dự báo — chọn MCP server"
        extra={<Button size="small" icon={<ReloadOutlined />} onClick={load}>Quét lại</Button>}
      >
        <Space direction="vertical" size={10} style={{ width: '100%' }}>
          <Segmented
            value={mode}
            onChange={(v) => setMode(v as any)}
            options={[
              { label: '⚡ Tự động (chọn nguồn tốt nhất đang chạy)', value: 'auto' },
              { label: 'Chọn thủ công', value: 'manual' },
            ]}
          />
          {mode === 'auto' ? (
            <Alert
              type="info"
              showIcon
              message={
                sources?.active?.length
                  ? <span>Đang dùng: {sources.active.map((k: string) => <Tag key={k} color="blue">{k}</Tag>)}</span>
                  : 'Chưa phát hiện MCP server nào có công cụ tìm kiếm đang chạy.'
              }
              description="App tự hỏi daemon xem có những MCP server nào, chấm điểm công cụ tìm kiếm (ưu tiên tin tức/web/nghiên cứu) và dùng nguồn tốt nhất — không cần khai địa chỉ."
            />
          ) : (
            <Select
              mode="multiple"
              style={{ width: '100%' }}
              placeholder="Chọn một hoặc nhiều nguồn MCP"
              value={picked}
              onChange={setPicked}
              optionFilterProp="label"
              options={list.map((x) => ({
                value: x.key,
                label: `${x.key} · ${x.description?.slice(0, 60) || ''}`,
              }))}
            />
          )}
          <Button type="primary" loading={busy} onClick={save}>Lưu nguồn tìm kiếm</Button>
          {!!list.length && (
            <Table
              size="small"
              rowKey="key"
              dataSource={list}
              pagination={{ pageSize: 8 }}
              columns={[
                { title: 'MCP server', dataIndex: 'server', width: 170 },
                { title: 'Công cụ', dataIndex: 'tool', width: 160, render: (v: string) => <Tag>{v}</Tag> },
                { title: 'Điểm', dataIndex: 'score', width: 70 },
                { title: 'Mô tả', dataIndex: 'description', render: (v: string) => <Text type="secondary">{v}</Text> },
              ]}
            />
          )}
        </Space>
      </Card>

      <Card size="small" title="Nguồn dữ liệu đang được các chủ đề kéo về">
        <Space direction="vertical" size={8} style={{ width: '100%' }}>
          <div>
            <Text type="secondary">Thời tiết: </Text>
            {(active.weather_places || []).length
              ? (active.weather_places || []).map((p: string) => <Tag key={p}>{p}</Tag>)
              : <Text type="secondary">chưa có chủ đề thời tiết nào</Text>}
          </div>
          <div>
            <Text type="secondary">Bóng đá: </Text>
            {(active.football_leagues || []).map((l: any) => <Tag key={l.id}>{l.name}</Tag>)}
          </div>
          <Alert
            type="info"
            showIcon
            message="Cấu hình nguồn nằm trong từng chủ đề"
            description='Muốn đổi địa điểm thời tiết hay giải bóng đá, mở chủ đề tương ứng ở tab "Chủ đề" → thẻ "Nguồn dữ liệu của chủ đề này".'
          />
        </Space>
      </Card>
    </Space>
  )
}

// ---- Siêu dự báo trace + Tri thức ----

function AskTrace({ r }: { r: any }) {
  const t = r.trace || {}
  const isSF = r.mode === 'superforecast'
  const li = (xs: any[], render: (x: any) => string) =>
    (xs || []).map((x, i) => <li key={i}>{render(x)}</li>)
  return (
    <Card size="small">
      <Space direction="vertical" style={{ width: '100%' }} size={8}>
        <Flex gap={12} align="center" wrap>
          <Statistic title="Xác suất xảy ra" value={Math.round((r.p_yes ?? 0) * 100)} suffix="%" />
          {t.confidence && <Tag color={t.confidence === 'cao' ? 'green' : t.confidence === 'thấp' ? 'orange' : 'blue'}>tin cậy {t.confidence}</Tag>}
          <Tag>{isSF ? 'pipeline Siêu Dự Báo' : 'dự đoán nhanh'}</Tag>
          <Tag color="purple">sổ #{r.ledger_id}</Tag>
        </Flex>
        {isSF ? (
          <>
            {t.outside_view?.rationale && (
              <Alert
                type="warning"
                message={<span><Text strong>Outside view (base rate{t.outside_view.base_rate != null ? ` ${Math.round(t.outside_view.base_rate * 100)}%` : ''}):</Text> {t.outside_view.rationale}</span>}
              />
            )}
            <Row gutter={12}>
              <Col span={12}>
                <Text strong type="success">Bằng chứng thuận</Text>
                <ul style={{ margin: '4px 0', paddingInlineStart: 20 }}>{li(t.evidence_for, (x) => (typeof x === 'string' ? x : JSON.stringify(x)))}</ul>
              </Col>
              <Col span={12}>
                <Text strong type="danger">Bằng chứng nghịch</Text>
                <ul style={{ margin: '4px 0', paddingInlineStart: 20 }}>{li(t.evidence_against, (x) => (typeof x === 'string' ? x : JSON.stringify(x)))}</ul>
              </Col>
            </Row>
            {!!(t.adjustments || []).length && (
              <div>
                <Text strong>Điều chỉnh từng bước:</Text>
                <ul style={{ margin: '4px 0', paddingInlineStart: 20 }}>
                  {li(t.adjustments, (a) =>
                    typeof a === 'string' ? a : `${a.delta > 0 ? '+' : ''}${Math.round((a.delta ?? 0) * 100)}%: ${a.reason ?? ''}`)}
                </ul>
              </div>
            )}
            {t.premortem && <Alert type="error" message={<span><Text strong>Premortem (nếu sai thì vì):</Text> {t.premortem}</span>} />}
            {!!(t.update_triggers || []).length && (
              <div>
                <Text strong>Điều kiện cập nhật:</Text>
                <ul style={{ margin: '4px 0', paddingInlineStart: 20 }}>{li(t.update_triggers, (x) => (typeof x === 'string' ? x : JSON.stringify(x)))}</ul>
              </div>
            )}
            {t.granularity_note && <Text type="secondary">Vì sao đúng con số này: {t.granularity_note}</Text>}
          </>
        ) : (
          <Paragraph style={{ whiteSpace: 'pre-wrap', margin: 0 }}>{t.reasoning || r.reasoning}</Paragraph>
        )}
        {!!(r.external_evidence || []).length && (
          <div>
            <Text strong>Tin ngoài (Search app · {r.external_evidence.length}):</Text>
            <ul style={{ margin: '4px 0', paddingInlineStart: 20 }}>
              {r.external_evidence.slice(0, 6).map((e: any, i: number) => (
                <li key={i}>
                  {e.url ? <a href={e.url} target="_blank" rel="noreferrer">{e.title || e.url}</a> : <Text>{e.title}</Text>}
                  {e.snippet && <Text type="secondary"> — {e.snippet.slice(0, 120)}…</Text>}
                </li>
              ))}
            </ul>
          </div>
        )}
        {r.evidence_note && <Text type="secondary">{r.evidence_note}</Text>}
      </Space>
    </Card>
  )
}

/// Tri thức đánh giá: mặc định seed từ sách Siêu Dự Báo, người dùng sửa được
/// (nguyên tắc, kỹ thuật, pipeline, và CHECKLIST bơm vào mọi lần tổng hợp).
function MethodTab() {
  const { message, modal } = AntApp.useApp()
  const [m, setM] = useState<any>(null)
  const [editing, setEditing] = useState(false)
  const [busy, setBusy] = useState(false)
  const [draft, setDraft] = useState<any>(null)

  const load = useCallback(async () => {
    const r = await api.method()
    setM(r)
    return r
  }, [])
  useEffect(() => { load().catch(() => {}) }, [load])

  const startEdit = () => {
    setDraft(JSON.parse(JSON.stringify(m)))
    setEditing(true)
  }
  const save = async () => {
    setBusy(true)
    try {
      const r = await api.methodUpdate(draft)
      if (r.error) return message.error(r.error)
      setM(r); setEditing(false)
      message.success('Đã cập nhật tri thức — checklist mới áp dụng cho mọi dự đoán sau')
    } finally { setBusy(false) }
  }
  const reset = () =>
    modal.confirm({
      title: 'Khôi phục tri thức mặc định?',
      content: 'Mọi chỉnh sửa của bạn sẽ bị thay bằng bản gốc từ sách Siêu Dự Báo (Tetlock).',
      okType: 'danger',
      onOk: async () => {
        const r = await api.methodReset()
        setM(r); setEditing(false)
        message.success('Đã khôi phục bản mặc định')
      },
    })

  if (!m) return <Empty description="Đang tải tri thức…" />

  const upd = (patch: any) => setDraft({ ...draft, ...patch })
  const updList = (key: string, i: number, patch: any) =>
    upd({ [key]: draft[key].map((x: any, j: number) => (j === i ? { ...x, ...patch } : x)) })

  return (
    <Space direction="vertical" size={16} style={{ maxWidth: 900, width: '100%' }}>
      <Flex gap={8} align="center" wrap>
        <Alert style={{ flex: 1, minWidth: 280 }} type="info" showIcon message={editing ? draft.source : m.source} />
        <Tag color={m.customized ? 'gold' : 'default'}>{m.customized ? 'đã tuỳ chỉnh' : 'bản mặc định'}</Tag>
        {editing ? (
          <>
            <Button type="primary" loading={busy} onClick={save}>Lưu tri thức</Button>
            <Button onClick={() => setEditing(false)}>Huỷ</Button>
          </>
        ) : (
          <>
            <Button icon={<EditOutlined />} onClick={startEdit}>Sửa tri thức</Button>
            {m.customized && <Button danger onClick={reset}>Về mặc định</Button>}
          </>
        )}
      </Flex>

      {editing && (
        <Card size="small" title="Nguồn tri thức">
          <Input value={draft.source} onChange={(e) => upd({ source: e.target.value })} />
        </Card>
      )}

      <Card size="small" title="Checklist bơm vào mọi lần tổng hợp dự đoán">
        {editing ? (
          <Input.TextArea autoSize={{ minRows: 6, maxRows: 20 }} value={draft.checklist} onChange={(e) => upd({ checklist: e.target.value })} />
        ) : (
          <Paragraph style={{ whiteSpace: 'pre-wrap', margin: 0 }} type="secondary">{m.checklist}</Paragraph>
        )}
      </Card>

      <Card size="small" title="Pipeline siêu dự báo của app">
        {editing ? (
          <Space direction="vertical" style={{ width: '100%' }} size={6}>
            {(draft.pipeline || []).map((s: string, i: number) => (
              <Flex gap={8} key={i}>
                <Input value={s} onChange={(e) => upd({ pipeline: draft.pipeline.map((x: string, j: number) => (j === i ? e.target.value : x)) })} />
                <Button type="text" icon={<DeleteOutlined />} onClick={() => upd({ pipeline: draft.pipeline.filter((_: any, j: number) => j !== i) })} />
              </Flex>
            ))}
            <Button size="small" icon={<PlusOutlined />} onClick={() => upd({ pipeline: [...draft.pipeline, ''] })}>Thêm bước</Button>
          </Space>
        ) : (
          <ol style={{ margin: 0, paddingInlineStart: 20 }}>
            {(m.pipeline || []).map((s: string, i: number) => <li key={i} style={{ padding: '2px 0' }}>{s.replace(/^\d+\.\s*/, '')}</li>)}
          </ol>
        )}
      </Card>

      <Card size="small" title={editing ? 'Nguyên tắc dự báo' : `Nguyên tắc dự báo (${(m.principles || []).length})`}>
        {editing ? (
          <Space direction="vertical" style={{ width: '100%' }} size={10}>
            {(draft.principles || []).map((p: any, i: number) => (
              <Flex gap={8} key={i} align="start">
                <Space direction="vertical" style={{ flex: 1 }} size={4}>
                  <Input placeholder="Tiêu đề nguyên tắc" value={p.title} onChange={(e) => updList('principles', i, { title: e.target.value })} />
                  <Input.TextArea autoSize={{ minRows: 2, maxRows: 6 }} placeholder="Diễn giải cách áp dụng" value={p.body} onChange={(e) => updList('principles', i, { body: e.target.value })} />
                </Space>
                <Button type="text" icon={<DeleteOutlined />} onClick={() => upd({ principles: draft.principles.filter((_: any, j: number) => j !== i) })} />
              </Flex>
            ))}
            <Button size="small" icon={<PlusOutlined />} onClick={() => upd({ principles: [...draft.principles, { key: 'custom', title: '', body: '' }] })}>Thêm nguyên tắc</Button>
          </Space>
        ) : (
          <Space direction="vertical" size={10} style={{ width: '100%' }}>
            {(m.principles || []).map((p: any, i: number) => (
              <div key={i}>
                <Text strong>{p.title}</Text>
                <Paragraph type="secondary" style={{ margin: 0 }}>{p.body}</Paragraph>
              </div>
            ))}
          </Space>
        )}
      </Card>

      <Card size="small" title="Kỹ thuật lõi">
        {editing ? (
          <Space direction="vertical" style={{ width: '100%' }} size={6}>
            {(draft.techniques || []).map((t: any, i: number) => (
              <Flex gap={8} key={i}>
                <Input style={{ width: 150 }} placeholder="key" value={t.key} onChange={(e) => updList('techniques', i, { key: e.target.value })} />
                <Input.TextArea autoSize={{ minRows: 1, maxRows: 4 }} value={t.body} onChange={(e) => updList('techniques', i, { body: e.target.value })} />
                <Button type="text" icon={<DeleteOutlined />} onClick={() => upd({ techniques: draft.techniques.filter((_: any, j: number) => j !== i) })} />
              </Flex>
            ))}
            <Button size="small" icon={<PlusOutlined />} onClick={() => upd({ techniques: [...draft.techniques, { key: 'custom', body: '' }] })}>Thêm kỹ thuật</Button>
          </Space>
        ) : (
          <Space direction="vertical" size={6} style={{ width: '100%' }}>
            {(m.techniques || []).map((t: any, i: number) => (
              <div key={i}><Tag>{t.key}</Tag> {t.body}</div>
            ))}
          </Space>
        )}
      </Card>
    </Space>
  )
}

function ResolveInline({ id, probs, onDone }: { id: number; probs: any; onDone: () => void }) {
  const { message } = AntApp.useApp()
  const keys = probs && typeof probs === 'object' ? Object.keys(probs) : []
  const [val, setVal] = useState<string | undefined>(undefined)
  if (!keys.length) return <Tag>chờ tự chấm</Tag>
  return (
    <Space size={4}>
      <Select size="small" style={{ width: 90 }} placeholder="kết quả" value={val} onChange={setVal}
        options={keys.map((k) => ({ value: k, label: k }))} />
      <Button size="small" disabled={!val} onClick={async () => {
        const r = await api.ledgerResolve(id, val!)
        if (r.error) message.error(r.error)
        else message.success(r.lesson ? `Đã chấm (brier ${r.brier}) + rút bài học` : `Đã chấm: brier ${r.brier}`)
        onDone()
      }}>Chấm</Button>
    </Space>
  )
}
