import { useEffect, useState } from 'react'
import {
  App as AntApp,
  Badge,
  Button,
  Card,
  Input,
  InputNumber,
  List,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
} from 'antd'
import { PlusOutlined, ReloadOutlined, SyncOutlined } from '@ant-design/icons'
import {
  api,
  type McpToolInfo,
  type SourceInfo,
  type SourceTemplate,
  type SyncReport,
} from './api'
import { healthStatus, kindColor } from './theme'

const { Text } = Typography
const { TextArea } = Input

function reasonOf(s: SourceInfo): string | null {
  return 'reason' in s.health ? s.health.reason : null
}

const BUILT_IN = ['web', 'knowledge', 'wiki', 'memory', 'corpus']
const QUERY_PARAM = /^(query|q|text|keyword|search|term)$/
const LIMIT_PARAM = /^(limit|count|num|top_k|max_results|n)$/

/** How likely is this tool to be a full-text search over a corpus? */
function searchiness(t: McpToolInfo): number {
  const name = t.name.toLowerCase()
  const params = Object.keys(t.inputSchema?.properties ?? {})
  let score = 0
  if (/(^|_)search$/.test(name)) score += 3
  else if (name.includes('search')) score += 1
  if (params.some((p) => QUERY_PARAM.test(p))) score += 2
  if (params.some((p) => LIMIT_PARAM.test(p))) score += 0.5
  if (/_by_/.test(name)) score -= 3
  if (/(create|update|delete|remove|send|post|write)/.test(name)) score -= 5
  return score
}

export default function Sources({
  sources,
  onChanged,
}: {
  sources: SourceInfo[]
  onChanged: () => void
}) {
  const { message } = AntApp.useApp()
  const [templates, setTemplates] = useState<SourceTemplate[]>([])
  const [sync, setSync] = useState<SyncReport[] | null>(null)
  const [busy, setBusy] = useState(false)

  // add-source form
  const [target, setTarget] = useState('')
  const [tools, setTools] = useState<McpToolInfo[] | null>(null)
  const [tool, setTool] = useState('')
  const [id, setId] = useState('')
  const [queryArg, setQueryArg] = useState('query')
  const [limitArg, setLimitArg] = useState('')
  const [extraArgs, setExtraArgs] = useState('')

  useEffect(() => {
    api.templates().then((r) => setTemplates(r.templates)).catch(() => {})
  }, [])

  async function guard(fn: () => Promise<unknown>) {
    setBusy(true)
    try {
      await fn()
      onChanged()
    } catch (e) {
      message.error((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  function targetPayload() {
    const t = target.trim()
    return t.startsWith('http') ? { rpc_url: t } : { app_id: t }
  }

  async function loadTools() {
    setTools(null)
    setId('')
    setTool('')
    setExtraArgs('')
    await guard(async () => {
      const r = await api.mcpTools(targetPayload())
      setTools(r.tools)
      const best = r.tools
        .map((t) => ({ t, score: searchiness(t) }))
        .sort((a, b) => b.score - a.score)[0]
      if (best && best.score >= 2) pickTool(best.t)
    })
  }

  function pickTool(t: McpToolInfo) {
    setTool(t.name)
    const props = Object.keys(t.inputSchema?.properties ?? {})
    setQueryArg(props.find((p) => QUERY_PARAM.test(p)) ?? 'query')
    setLimitArg(props.find((p) => LIMIT_PARAM.test(p)) ?? '')
    if (!id) setId(t.name.replace(/_?(search|query|find).*$/, '') || t.name)
    const missing = (t.inputSchema?.required ?? []).filter(
      (r) => !QUERY_PARAM.test(r) && !LIMIT_PARAM.test(r),
    )
    setExtraArgs(
      missing.length
        ? JSON.stringify(Object.fromEntries(missing.map((m) => [m, ''])), null, 1)
        : '',
    )
  }

  async function add() {
    let extra: Record<string, unknown> | undefined
    if (extraArgs.trim()) {
      try {
        extra = JSON.parse(extraArgs)
      } catch {
        message.error('`extra_args` không phải JSON hợp lệ')
        return
      }
    }
    await guard(async () => {
      await api.addSource({
        id: id.trim(),
        label: id.trim(),
        ...targetPayload(),
        tool,
        query_arg: queryArg || 'query',
        limit_arg: limitArg || undefined,
        extra_args: extra,
      })
      message.success(`đã thêm nguồn ${id.trim()}`)
      setTools(null)
      setTool('')
      setId('')
      setExtraArgs('')
    })
  }

  return (
    <Space direction="vertical" size={16} style={{ width: '100%' }}>
      <Card size="small" title="Nguồn tìm kiếm">
        <Table<SourceInfo>
          size="small"
          rowKey="id"
          dataSource={sources}
          pagination={false}
          columns={[
            {
              title: '',
              dataIndex: 'enabled',
              width: 48,
              render: (enabled: boolean, s) => (
                <Switch
                  size="small"
                  checked={enabled}
                  disabled={busy}
                  onChange={(v) => guard(() => api.setSource(s.id, { enabled: v }))}
                />
              ),
            },
            {
              title: 'Nguồn',
              dataIndex: 'label',
              render: (label: string, s) => (
                <Space size={6}>
                  <Badge status={healthStatus(s.health.state)} />
                  <Text>{label}</Text>
                  <Tag color={kindColor(s.kind)} bordered={false}>
                    {s.kind}
                  </Tag>
                </Space>
              ),
            },
            {
              title: 'Trọng số',
              dataIndex: 'weight',
              width: 96,
              render: (weight: number, s) => (
                <InputNumber
                  size="small"
                  min={0}
                  max={10}
                  step={0.1}
                  value={Number(weight.toFixed(1))}
                  disabled={busy}
                  style={{ width: 72 }}
                  onChange={(v) => v != null && guard(() => api.setSource(s.id, { weight: v }))}
                />
              ),
            },
            {
              title: '',
              key: 'action',
              width: 60,
              render: (_, s) =>
                BUILT_IN.includes(s.id) ? (
                  <Text type="secondary" style={{ fontSize: 12 }} title={reasonOf(s) ?? ''}>
                    có sẵn
                  </Text>
                ) : (
                  <Button
                    size="small"
                    danger
                    type="link"
                    disabled={busy}
                    onClick={() => guard(() => api.removeSource(s.id))}
                  >
                    gỡ
                  </Button>
                ),
            },
          ]}
        />
        <Button
          icon={<SyncOutlined />}
          size="small"
          style={{ marginTop: 12 }}
          disabled={busy}
          onClick={() => guard(async () => setSync((await api.sync()).sources))}
        >
          Quét lại app đã cài
        </Button>
        {sync && (
          <List
            size="small"
            style={{ marginTop: 10 }}
            dataSource={sync}
            renderItem={(r) => (
              <List.Item style={{ paddingInline: 0 }}>
                <Space>
                  <Tag color={r.registered ? 'success' : 'default'}>{r.registered ? '+' : '−'}</Tag>
                  <Text strong>{r.id}</Text>
                  <Text type="secondary" style={{ fontSize: 12.5 }}>
                    {r.reason}
                  </Text>
                </Space>
              </List.Item>
            )}
          />
        )}
      </Card>

      <Card size="small" title="Thêm nguồn từ MCP bất kỳ">
        <Space.Compact style={{ width: '100%' }}>
          <Input
            placeholder="id app đã cài (vd: youtube) hoặc URL JSON-RPC"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            onPressEnter={loadTools}
          />
          <Button
            type="primary"
            icon={<ReloadOutlined />}
            disabled={busy || !target.trim()}
            onClick={loadTools}
          >
            Xem công cụ
          </Button>
        </Space.Compact>

        {tools && tools.length === 0 && (
          <Text type="secondary" style={{ display: 'block', marginTop: 10 }}>
            MCP này không có công cụ nào.
          </Text>
        )}

        {tools && tools.length > 0 && (
          <>
            <Space size={[6, 6]} wrap style={{ marginTop: 12 }}>
              {tools.map((t) => (
                <Tag.CheckableTag
                  key={t.name}
                  checked={tool === t.name}
                  onChange={() => pickTool(t)}
                >
                  {t.name}
                </Tag.CheckableTag>
              ))}
            </Space>

            {tool && (
              <Space direction="vertical" size={8} style={{ width: '100%', marginTop: 12 }}>
                <label>
                  <Text type="secondary" style={{ fontSize: 12.5 }}>
                    Tên nguồn
                  </Text>
                  <Input value={id} onChange={(e) => setId(e.target.value)} />
                </label>
                <Space style={{ width: '100%' }}>
                  <label style={{ flex: 1 }}>
                    <Text type="secondary" style={{ fontSize: 12.5 }}>
                      Tham số truy vấn
                    </Text>
                    <Input value={queryArg} onChange={(e) => setQueryArg(e.target.value)} />
                  </label>
                  <label style={{ flex: 1 }}>
                    <Text type="secondary" style={{ fontSize: 12.5 }}>
                      Tham số giới hạn (bỏ trống nếu không có)
                    </Text>
                    <Input value={limitArg} onChange={(e) => setLimitArg(e.target.value)} />
                  </label>
                </Space>
                <label>
                  <Text type="secondary" style={{ fontSize: 12.5 }}>
                    Tham số cố định (JSON)
                  </Text>
                  <TextArea
                    rows={3}
                    placeholder='{"platform":"threads","handle":"@ten_cua_ban"}'
                    value={extraArgs}
                    onChange={(e) => setExtraArgs(e.target.value)}
                  />
                </label>
                <Button
                  type="primary"
                  icon={<PlusOutlined />}
                  disabled={busy || !id.trim()}
                  onClick={add}
                >
                  Thêm nguồn
                </Button>
              </Space>
            )}
          </>
        )}
      </Card>

      {templates.length > 0 && (
        <Card size="small" title="Cần bạn cấu hình thêm">
          <List
            size="small"
            dataSource={templates}
            renderItem={(t) => (
              <List.Item style={{ display: 'block', paddingInline: 0 }}>
                <Text strong>{t.label}</Text>{' '}
                <Text type="secondary" style={{ fontSize: 12.5 }}>
                  — {t.why}
                </Text>
                <div>
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    cần: {t.required_args.map((a) => a.name).join(', ')} · app <code>{t.app_id}</code>,
                    công cụ <code>{t.tool}</code>
                  </Text>
                </div>
              </List.Item>
            )}
          />
        </Card>
      )}
    </Space>
  )
}
