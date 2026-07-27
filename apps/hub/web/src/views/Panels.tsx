// HMI panels: user-authored HTML pages rendered in a sandboxed iframe with
// live data binding. Tokens like {{101.temperature}} (device id + telemetry
// field / attribute / name / online / last_seen) are replaced with fresh
// values every poll tick.
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  App as AntApp,
  Button,
  Card,
  Col,
  Empty,
  Input,
  List,
  Popconfirm,
  Row,
  Space,
  Tooltip,
  Typography,
} from 'antd'
import {
  CodeOutlined,
  DeleteOutlined,
  ExpandOutlined,
  FileAddOutlined,
  SaveOutlined,
} from '@ant-design/icons'
import { api, type HtmlPanel } from '../api'
import { POLL_MS } from '../ui'

const { Text } = Typography

const TOKEN_RE = /\{\{\s*([\w-]+)\.([\w-]+)\s*\}\}/g

const DEFAULT_HTML = `<!-- Panel HMI: dùng {{<device_id>.<trường>}} để gắn dữ liệu trực tiếp.
     Trường hỗ trợ: tên telemetry (vd temperature), thuộc tính, name, online, last_seen -->
<style>
  body { font-family: system-ui; background: #0b1220; color: #e6edf7; margin: 0; padding: 24px; }
  .tile { display: inline-block; background: #16223a; border: 1px solid #27395c;
          border-radius: 12px; padding: 18px 26px; margin: 8px; min-width: 160px; }
  .v { font-size: 34px; font-weight: 700; color: #7ec8ff; }
  .k { font-size: 13px; color: #8ea0bd; margin-top: 4px; }
</style>
<h2>Trạm giám sát</h2>
<div class="tile"><div class="v">{{101.temperature}}°C</div><div class="k">Nhiệt độ · {{101.name}}</div></div>
<div class="tile"><div class="v">{{101.humidity}}%</div><div class="k">Độ ẩm</div></div>
<div class="tile"><div class="v">{{101.online}}</div><div class="k">Trạng thái</div></div>
`

/** Collect referenced device ids, fetch their data, substitute tokens. */
async function bindData(html: string): Promise<string> {
  const ids = [...new Set([...html.matchAll(TOKEN_RE)].map((m) => m[1]))]
  const data = new Map<string, Record<string, string>>()
  await Promise.all(
    ids.map(async (id) => {
      const vals: Record<string, string> = {}
      try {
        const d = await api.device(id)
        vals['name'] = d.name
        vals['online'] = d.online ? 'ONLINE' : 'OFFLINE'
        vals['last_seen'] = d.last_seen ?? '—'
        for (const [k, v] of Object.entries(d.attributes ?? {})) vals[k] = String(v)
        const points = await api.telemetry(id, '', 50)
        // points are newest-first — keep the first value seen per field
        for (const p of points) if (!(p.field in vals)) vals[p.field] = String(p.value)
      } catch {
        vals['name'] = `#${id}?`
      }
      data.set(id, vals)
    }),
  )
  return html.replace(TOKEN_RE, (_, id: string, key: string) => data.get(id)?.[key] ?? '—')
}

export default function Panels() {
  const { message } = AntApp.useApp()
  const [panels, setPanels] = useState<HtmlPanel[]>([])
  const [sel, setSel] = useState<HtmlPanel | null>(null)
  const [name, setName] = useState('')
  const [html, setHtml] = useState(DEFAULT_HTML)
  const [bound, setBound] = useState('')
  const [busy, setBusy] = useState(false)
  const previewRef = useRef<HTMLDivElement>(null)

  const load = useCallback(() => {
    api.panels().then((ps) => {
      setPanels(ps)
      if (!sel && ps.length > 0) select(ps[0])
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    load()
  }, [load])

  const select = (p: HtmlPanel | null) => {
    setSel(p)
    setName(p?.name ?? '')
    setHtml(p?.html ?? DEFAULT_HTML)
  }

  // Live data binding: rebind whenever the html changes and on every poll tick.
  useEffect(() => {
    let cancelled = false
    const tick = () => bindData(html).then((b) => !cancelled && setBound(b))
    tick()
    const t = setInterval(tick, POLL_MS)
    return () => {
      cancelled = true
      clearInterval(t)
    }
  }, [html])

  const save = async () => {
    if (!name.trim()) {
      message.error('Đặt tên cho panel trước.')
      return
    }
    setBusy(true)
    try {
      const p = await api.savePanel({ id: sel?.id, name: name.trim(), html })
      message.success('Đã lưu panel.')
      setSel(p)
      api.panels().then(setPanels)
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const remove = async (p: HtmlPanel) => {
    await api.deletePanel(p.id)
    message.success('Đã xoá panel.')
    if (sel?.id === p.id) select(null)
    api.panels().then(setPanels)
  }

  const fullscreen = () => previewRef.current?.requestFullscreen?.()

  const srcDoc = useMemo(() => bound, [bound])

  return (
    <Row gutter={[14, 14]} style={{ height: '100%' }}>
      <Col xs={24} md={6}>
        <Card
          title="Panel HMI"
          size="small"
          extra={
            <Button
              size="small"
              icon={<FileAddOutlined />}
              onClick={() => select(null)}
            >
              Mới
            </Button>
          }
        >
          {panels.length === 0 ? (
            <Empty description="Chưa có panel nào." image={Empty.PRESENTED_IMAGE_SIMPLE} />
          ) : (
            <List
              size="small"
              dataSource={panels}
              renderItem={(p) => (
                <List.Item
                  className="panel-item"
                  style={{
                    cursor: 'pointer',
                    background: sel?.id === p.id ? 'rgba(77,163,255,0.1)' : undefined,
                    borderRadius: 6,
                    paddingInline: 8,
                  }}
                  onClick={() => select(p)}
                  actions={[
                    <Popconfirm
                      key="del"
                      title="Xoá panel này?"
                      onConfirm={(e) => {
                        e?.stopPropagation()
                        remove(p)
                      }}
                      onCancel={(e) => e?.stopPropagation()}
                    >
                      <Button
                        size="small"
                        type="text"
                        danger
                        icon={<DeleteOutlined />}
                        onClick={(e) => e.stopPropagation()}
                      />
                    </Popconfirm>,
                  ]}
                >
                  <List.Item.Meta
                    title={p.name}
                    description={
                      <Text type="secondary" style={{ fontSize: 11 }}>
                        {p.updated_at}
                      </Text>
                    }
                  />
                </List.Item>
              )}
            />
          )}
        </Card>
      </Col>
      <Col xs={24} md={9}>
        <Card
          title={
            <Space>
              <CodeOutlined /> HTML
            </Space>
          }
          size="small"
          extra={
            <Button
              type="primary"
              size="small"
              icon={<SaveOutlined />}
              loading={busy}
              onClick={save}
            >
              Lưu
            </Button>
          }
        >
          <Input
            placeholder="Tên panel"
            value={name}
            onChange={(e) => setName(e.target.value)}
            style={{ marginBottom: 8 }}
          />
          <Input.TextArea
            className="mono-input"
            value={html}
            onChange={(e) => setHtml(e.target.value)}
            autoSize={{ minRows: 20, maxRows: 32 }}
          />
          <Text type="secondary" style={{ fontSize: 12, display: 'block', marginTop: 8 }}>
            Gắn dữ liệu trực tiếp bằng <Text code>{'{{<device_id>.<trường>}}'}</Text> — ví dụ{' '}
            <Text code>{'{{101.temperature}}'}</Text>, <Text code>{'{{101.name}}'}</Text>,{' '}
            <Text code>{'{{101.online}}'}</Text>. Giá trị tự cập nhật mỗi {POLL_MS / 1000}s.
          </Text>
        </Card>
      </Col>
      <Col xs={24} md={9}>
        <Card
          title="Xem trước"
          size="small"
          extra={
            <Tooltip title="Toàn màn hình">
              <Button size="small" icon={<ExpandOutlined />} onClick={fullscreen} />
            </Tooltip>
          }
          styles={{ body: { padding: 0 } }}
        >
          <div ref={previewRef} className="panel-preview">
            <iframe title="panel-preview" sandbox="allow-scripts" srcDoc={srcDoc} />
          </div>
        </Card>
      </Col>
    </Row>
  )
}
