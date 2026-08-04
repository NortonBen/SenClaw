import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Alert, Button, Card, Col, ConfigProvider, Empty, Input, Modal, Row, Select, Space,
  Spin, Switch, Table, Tabs, Tag, Tooltip, Typography, message, theme,
} from 'antd'
import {
  BulbOutlined, DeleteOutlined, PlusOutlined, SafetyCertificateOutlined, ScanOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons'
import viVN from 'antd/locale/vi_VN'
import Dashboard from './Dashboard'
import Rules from './Rules'
import {
  api, gradeColor, SEV_COLOR, SEV_LABEL, SEV_ORDER,
  type Asset, type DiffEntry, type Finding, type Scan, type Severity,
} from './api'

const { Title, Text, Paragraph } = Typography

const THEME_KEY = 'secscan.theme'

export default function App() {
  const [dark, setDark] = useState(
    () => localStorage.getItem(THEME_KEY) === 'dark',
  )

  // Các lớp CSS tự viết đọc biến theo data-theme; antd đọc algorithm. Hai bên
  // phải cùng đổi, nếu không thẻ .finding sẽ trắng trên nền tối.
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
  const [assets, setAssets] = useState<Asset[]>([])
  const [selected, setSelected] = useState<number | null>(null)
  const [scans, setScans] = useState<Scan[]>([])
  const [findings, setFindings] = useState<Finding[]>([])
  const [current, setCurrent] = useState<Scan | null>(null)
  const [scanning, setScanning] = useState(false)
  const [loading, setLoading] = useState(true)
  const [reloadKey, setReloadKey] = useState(0)
  const [diff, setDiff] = useState<{ new: DiffEntry[]; fixed: DiffEntry[] } | null>(null)

  const asset = assets.find((a) => a.id === selected) ?? null

  const loadAssets = useCallback(async () => {
    const list = await api.assets()
    setAssets(list)
    setSelected((prev) => (prev != null && list.some((a) => a.id === prev) ? prev : list[0]?.id ?? null))
    setLoading(false)
  }, [])

  useEffect(() => { void loadAssets() }, [loadAssets])

  useEffect(() => {
    if (selected == null) { setFindings([]); setCurrent(null); setDiff(null); return }
    void (async () => {
      const list = await api.scans(selected)
      setScans(list)
      const done = list.filter((s) => s.status === 'done')
      if (done.length === 0) { setFindings([]); setCurrent(null); setDiff(null); return }
      const latest = done[0]
      const r = await api.scan_get(latest.id)
      setFindings(r.findings ?? [])
      setCurrent(r.scan ?? null)
      if (done.length > 1) {
        const d = await api.diff(done[1].id, latest.id)
        setDiff(d.ok ? { new: d.new ?? [], fixed: d.fixed ?? [] } : null)
      } else setDiff(null)
    })()
  }, [selected, reloadKey])

  const runScan = async () => {
    if (selected == null) return
    setScanning(true)
    const r = await api.scan(selected)
    setScanning(false)
    if (!r.ok) message.error(r.error ?? 'quét thất bại')
    else message.success(`Xong: ${r.score} điểm, hạng ${r.grade}`)
    setReloadKey((k) => k + 1)
  }

  const runActive = async () => {
    if (selected == null) return
    setScanning(true)
    const r = await api.scanActive(selected)
    setScanning(false)
    if (!r.ok) { message.error(r.error ?? 'quét thất bại'); return }
    // Chạm trần thì phải nói ra — im lặng cắt bớt khiến kết quả đọc như đã phủ hết.
    if (r.truncated) {
      message.warning(`Kết quả BÁN PHẦN: dừng ở ${r.requests} yêu cầu để giữ nhịp thấp.`)
    } else {
      message.success(`Xong: ${r.score} điểm (${r.requests} yêu cầu)`)
    }
    setReloadKey((k) => k + 1)
  }

  if (loading) return <div style={{ padding: 80, textAlign: 'center' }}><Spin size="large" /></div>

  return (
    <div className="wrap">
      <Row align="middle" style={{ marginBottom: 4 }}>
        <Col flex="auto">
          <Space align="center">
            <SafetyCertificateOutlined style={{ fontSize: 22, color: '#1677ff' }} />
            <Title level={3} style={{ margin: 0 }}>Quét Bảo Mật</Title>
          </Space>
        </Col>
        <Col>
          <Tooltip title={dark ? 'chuyển sang nền sáng' : 'chuyển sang nền tối'}>
            <Button type="text" icon={<BulbOutlined />} onClick={onToggleTheme} />
          </Tooltip>
        </Col>
      </Row>
      <Paragraph type="secondary" style={{ marginBottom: 16 }}>
        Đánh giá tư thế bảo mật cho website và máy chủ <strong>của chính bạn</strong>.
        Quét thụ động: chỉ một yêu cầu GET, không gửi payload tấn công nào — an toàn chạy trên production.
      </Paragraph>

      <AssetBar assets={assets} selected={selected} onSelect={setSelected} onChanged={loadAssets} />

      {assets.length === 0 ? (
        <Empty style={{ marginTop: 60 }} description="Chưa có tài sản nào. Thêm website của bạn để bắt đầu." />
      ) : (
        <>
          {asset && (
            <Card size="small" style={{ marginTop: 16 }}>
              <Row align="middle" gutter={16}>
                <Col flex="auto">
                  <Space direction="vertical" size={2}>
                    <Text strong style={{ fontSize: 15 }}>{asset.target}</Text>
                    <Space size={6} wrap>
                      <Tag>{asset.kind}</Tag>
                      {asset.verified_at && <Tag color="green">đã xác minh · {asset.verify_method}</Tag>}
                      {current && (
                        <Text className="muted">
                          quét lúc {new Date(current.started_at).toLocaleString('vi-VN')}
                        </Text>
                      )}
                    </Space>
                  </Space>
                </Col>
                <Col>
                  <Space>
                    <Button type="primary" icon={<ScanOutlined />} loading={scanning} onClick={runScan}>
                      Quét thụ động
                    </Button>
                    <Tooltip title="Dò tệp lộ ra ngoài, liệt kê thư mục, CORS, và đối chiếu CVE cho manifest bắt được. Nhịp thấp (~4 req/s), an toàn cho production.">
                      <Button
                        icon={<ThunderboltOutlined />} loading={scanning} onClick={runActive}
                      >
                        Quét chủ động
                      </Button>
                    </Tooltip>
                  </Space>
                </Col>
              </Row>
            </Card>
          )}

          <Tabs
            style={{ marginTop: 12 }}
            items={[
              {
                key: 'overview',
                label: 'Tổng quan',
                children: <Dashboard assetId={selected} reloadKey={reloadKey} />,
              },
              {
                key: 'findings',
                label: `Phát hiện${findings.length ? ` (${findings.length})` : ''}`,
                children: (
                  <Findings
                    findings={findings} diff={diff}
                    onChanged={() => setReloadKey((k) => k + 1)}
                  />
                ),
              },
              {
                key: 'history',
                label: `Lịch sử${scans.length ? ` (${scans.length})` : ''}`,
                children: <History scans={scans} />,
              },
              {
                key: 'rules',
                label: 'Tiêu chuẩn quét',
                children: <Rules />,
              },
              ...(asset
                ? [{
                    key: 'verify',
                    label: asset.verified_at ? 'Sở hữu (đã xác minh)' : 'Sở hữu (tuỳ chọn)',
                    children: <VerifyPanel asset={asset} onDone={loadAssets} />,
                  }]
                : []),
            ]}
          />
        </>
      )}
    </div>
  )
}

function Findings({
  findings, diff, onChanged,
}: {
  findings: Finding[]
  diff: { new: DiffEntry[]; fixed: DiffEntry[] } | null
  onChanged: () => void
}) {
  const [showInfo, setShowInfo] = useState(false)

  const counts = useMemo(() => {
    const c: Record<string, number> = {}
    for (const f of findings) c[f.severity] = (c[f.severity] ?? 0) + 1
    return c
  }, [findings])

  const visible = useMemo(
    () => findings.filter((f) => showInfo || f.severity !== 'info'),
    [findings, showInfo],
  )

  if (findings.length === 0) {
    return <Empty description="Chưa có lần quét nào — bấm Quét để bắt đầu" />
  }

  return (
    <>
      {diff && (diff.new.length > 0 || diff.fixed.length > 0) && (
        <Alert
          style={{ marginBottom: 14 }}
          type={diff.new.length > 0 ? 'warning' : 'success'} showIcon
          message="So với lần quét trước"
          description={
            <Space direction="vertical" size={4}>
              {diff.new.length > 0 && (
                <Text>
                  <strong>{diff.new.length} phát hiện mới:</strong>{' '}
                  {diff.new.slice(0, 3).map((f) => f.title).join('; ')}
                  {diff.new.length > 3 && ` … và ${diff.new.length - 3} nữa`}
                </Text>
              )}
              {diff.fixed.length > 0 && (
                <Text type="success">
                  <strong>{diff.fixed.length} đã hết:</strong>{' '}
                  {diff.fixed.slice(0, 3).map((f) => f.title).join('; ')}
                </Text>
              )}
            </Space>
          }
        />
      )}

      <Row align="middle" style={{ marginBottom: 10 }}>
        <Col flex="auto">
          <Space size={6} wrap>
            {SEV_ORDER.filter((s) => counts[s]).map((s) => (
              <Tag key={s} color={SEV_COLOR[s]}>{SEV_LABEL[s]}: {counts[s]}</Tag>
            ))}
          </Space>
        </Col>
        <Col>
          <Space size={8}>
            <Text className="muted">hiện mục thông tin{counts.info ? ` (${counts.info})` : ''}</Text>
            <Tooltip title="Mục 'thông tin' không phải lỗi — ví dụ thiếu Referrer-Policy, vì mặc định của trình duyệt vốn đã an toàn.">
              <Switch size="small" checked={showInfo} onChange={setShowInfo} />
            </Tooltip>
          </Space>
        </Col>
      </Row>

      {visible.map((f) => <FindingCard key={f.id} f={f} onChanged={onChanged} />)}

      <Alert
        style={{ marginTop: 20 }} type="info"
        message="Điểm cao không có nghĩa là an toàn"
        description={
          <Text style={{ fontSize: 13 }}>
            Công cụ tự động chỉ thấy được thứ quan sát được từ bên ngoài. Ba loại lỗ hổng nặng
            nhất — phân quyền sai theo vai trò, thiết kế không an toàn, và thiếu ghi log —{' '}
            <strong>không scanner nào tự tìm được</strong>. Xem tab “Tiêu chuẩn quét” để biết
            chính xác app kiểm những gì.
          </Text>
        }
      />
    </>
  )
}

function AssetBar({
  assets, selected, onSelect, onChanged,
}: {
  assets: Asset[]
  selected: number | null
  onSelect: (id: number) => void
  onChanged: () => void
}) {
  const [adding, setAdding] = useState(false)
  const [target, setTarget] = useState('')
  const [kind, setKind] = useState('website')

  const add = async () => {
    const t = target.trim()
    if (!t) return
    const r = await api.addAsset(kind, t, '')
    if (!r.ok) { message.error(r.error ?? 'không thêm được'); return }
    setTarget(''); setAdding(false); onChanged()
    message.success('đã thêm')
  }

  const remove = (a: Asset) => {
    Modal.confirm({
      title: `Xoá ${a.target}?`,
      content: 'Xoá luôn toàn bộ lịch sử quét và phát hiện của tài sản này. Không hoàn tác được.',
      okText: 'Xoá', okButtonProps: { danger: true }, cancelText: 'Thôi',
      onOk: async () => { await api.removeAsset(a.id); onChanged() },
    })
  }

  return (
    <Space wrap>
      <Select
        style={{ minWidth: 300 }} value={selected ?? undefined} onChange={onSelect}
        placeholder="chọn tài sản"
        options={assets.map((a) => ({
          value: a.id, label: `${a.target}${a.verified_at ? ' ✓' : ''}`,
        }))}
      />
      {!adding && <Button icon={<PlusOutlined />} onClick={() => setAdding(true)}>Thêm</Button>}
      {adding && (
        <Space.Compact>
          <Select
            value={kind} onChange={setKind} style={{ width: 110 }}
            options={[
              { value: 'website', label: 'website' },
              { value: 'domain', label: 'tên miền' },
              { value: 'host', label: 'máy chủ' },
            ]}
          />
          <Input
            autoFocus placeholder="https://example.com" value={target}
            onChange={(e) => setTarget(e.target.value)} onPressEnter={add} style={{ width: 260 }}
          />
          <Button type="primary" onClick={add}>Lưu</Button>
          <Button onClick={() => setAdding(false)}>Huỷ</Button>
        </Space.Compact>
      )}
      {selected != null && assets.some((a) => a.id === selected) && (
        <Button
          icon={<DeleteOutlined />} danger type="text"
          onClick={() => remove(assets.find((a) => a.id === selected)!)}
        />
      )}
    </Space>
  )
}

function VerifyPanel({ asset, onDone }: { asset: Asset; onDone: () => void }) {
  const [method, setMethod] = useState('dns-txt')
  const [instructions, setInstructions] = useState<string | null>(null)
  const [checking, setChecking] = useState(false)

  const gen = async () => {
    const r = await api.verifyToken(asset.id, method)
    if (!r.ok) { message.error(r.error ?? 'lỗi'); return }
    setInstructions(r.instructions ?? null)
  }

  const check = async () => {
    setChecking(true)
    const r = await api.verify(asset.id)
    setChecking(false)
    if (r.verified) { message.success('đã xác minh sở hữu'); onDone() }
    else message.warning(r.error ?? 'chưa thấy bằng chứng')
  }

  return (
    <Card size="small">
      <Paragraph type="secondary" style={{ fontSize: 13 }}>
        <strong>Không bắt buộc</strong> — mọi lớp quét chạy được không cần bước này. Nhưng nếu tài
        sản nằm trong mạng nội bộ (127.0.0.1, 192.168.x, 10.x…), phương thức "local" là cách
        khai báo điều đó để scanner cho phép chạm dải riêng. Không có nó thì rào SSRF sẽ từ chối
        — chính nó chặn scanner tự biến thành công cụ tấn công.
      </Paragraph>
      <Space wrap>
        <Select
          value={method} onChange={setMethod} style={{ width: 240 }}
          options={[
            { value: 'dns-txt', label: 'DNS TXT (mạnh nhất)' },
            { value: 'dns-cname', label: 'DNS CNAME (khi apex flatten)' },
            { value: 'well-known', label: 'Tệp /.well-known/' },
            { value: 'meta', label: 'Thẻ meta (yếu nhất)' },
            { value: 'local', label: 'Mạng nội bộ' },
          ]}
        />
        <Button onClick={gen}>Lấy hướng dẫn</Button>
        {instructions && <Button type="primary" loading={checking} onClick={check}>Kiểm tra</Button>}
      </Space>
      {instructions && <div className="fix" style={{ marginTop: 10 }}>{instructions}</div>}
      {asset.verify_error && (
        <Alert style={{ marginTop: 10 }} type="warning" showIcon message={asset.verify_error} />
      )}
    </Card>
  )
}

function FindingCard({ f, onChanged }: { f: Finding; onChanged: () => void }) {
  const [status, setStatus] = useState(f.status)

  const ack = async () => {
    const next = status === 'acked' ? 'open' : 'acked'
    await api.setStatus(f.id, next)
    setStatus(next); onChanged()
  }

  return (
    <div
      className="finding"
      style={{
        borderLeftColor: SEV_COLOR[f.severity as Severity],
        opacity: status === 'acked' ? 0.55 : 1,
      }}
    >
      <Row align="top" gutter={8}>
        <Col flex="auto">
          <h4>{f.title}</h4>
          {f.detail && <p>{f.detail}</p>}
          {f.fix && <div className="fix">{f.fix}</div>}
          <div className="tag-row">
            <Tag color={SEV_COLOR[f.severity as Severity]}>{SEV_LABEL[f.severity as Severity]}</Tag>
            <Tag>{f.category}</Tag>
            {f.wstg && <Tag color="blue">{f.wstg}</Tag>}
            {f.kev && <Tag color="red">KEV — đang bị khai thác thật</Tag>}
            {status === 'regressed' && <Tag color="volcano">tái phát</Tag>}
            {status === 'acked' && <Tag>đã chấp nhận rủi ro</Tag>}
          </div>
        </Col>
        <Col>
          <Button size="small" type="text" onClick={ack}>
            {status === 'acked' ? 'mở lại' : 'chấp nhận'}
          </Button>
        </Col>
      </Row>
    </div>
  )
}

function History({ scans }: { scans: Scan[] }) {
  if (scans.length === 0) return <Empty description="chưa có lần quét nào" />
  return (
    <Table
      size="small" rowKey="id" pagination={{ pageSize: 15, hideOnSinglePage: true }}
      dataSource={scans}
      columns={[
        {
          title: 'Lúc', dataIndex: 'started_at',
          render: (v: string) => new Date(v).toLocaleString('vi-VN'),
        },
        {
          title: 'Mất', key: 'dur', width: 80,
          render: (_: unknown, r: Scan) =>
            r.finished_at
              ? `${Math.max(0, Math.round((+new Date(r.finished_at) - +new Date(r.started_at)) / 1000))}s`
              : '—',
        },
        { title: 'Lớp', dataIndex: 'layer', width: 90 },
        {
          title: 'Điểm', dataIndex: 'score', width: 80,
          render: (v: number | null) => (v == null ? '—' : v),
        },
        {
          title: 'Hạng', dataIndex: 'grade', width: 80,
          render: (v: string | null) =>
            v ? <strong style={{ color: gradeColor(v) }}>{v}</strong> : '—',
        },
        {
          title: 'Trạng thái', dataIndex: 'status', width: 110,
          render: (v: string, r: Scan) =>
            v === 'failed'
              ? <Tooltip title={r.error ?? ''}><Tag color="red">lỗi</Tag></Tooltip>
              : <Tag color={v === 'done' ? 'green' : 'default'}>{v}</Tag>,
        },
      ]}
    />
  )
}
