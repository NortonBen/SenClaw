import { useEffect, useMemo, useState } from 'react'
import type { ReactNode } from 'react'
import {
  Row, Col, Card, Button, Input, Switch, Select, Collapse, Checkbox, Space, Tag, Segmented,
  Typography, message, Popconfirm, Modal, Form, Avatar, Badge, Divider, Tooltip,
} from 'antd'
import {
  PlusOutlined, LockOutlined, RobotOutlined, SaveOutlined, DeleteOutlined, PoweroffOutlined,
  ReadOutlined, BulbOutlined, WarningOutlined, ToolOutlined, ApiOutlined, IdcardOutlined,
} from '@ant-design/icons'
import { api } from '../api'
import type { Bot, Inventory, SkillInventory } from '../api'
import type { T } from '../i18n'

export default function BotsPage({ t, bots, refresh }: { t: T; bots: Bot[]; refresh: () => void }) {
  const [selKey, setSelKey] = useState('')
  const [creating, setCreating] = useState(false)
  const [form] = Form.useForm()
  const sel = bots.find((b) => b.key === selKey) || bots[0]

  const doCreate = async (v: { name: string }) => {
    const bot = await api.createBot({ name: v.name })
    setCreating(false)
    form.resetFields()
    await refresh()
    setSelKey(bot.key)
  }

  return (
    <Row gutter={20} wrap={false} align="top">
      <Col flex="270px">
        <Button type="primary" icon={<PlusOutlined />} block size="large" onClick={() => setCreating(true)} style={{ marginBottom: 14 }}>
          {t('newBot')}
        </Button>
        <Space direction="vertical" size={10} style={{ width: '100%' }}>
          {bots.map((b) => {
            const active = sel?.key === b.key
            return (
              <Card
                key={b.key}
                size="small"
                hoverable
                onClick={() => setSelKey(b.key)}
                styles={{ body: { padding: 12 } }}
                style={{ borderColor: active ? '#1890ff' : undefined, boxShadow: active ? '0 0 0 1px #1890ff inset' : undefined }}
              >
                <Space>
                  <Badge dot color={b.enabled ? 'green' : 'default'} offset={[-4, 30]}>
                    <Avatar shape="square" style={{ background: active ? '#1890ff' : '#8c8c8c' }} icon={<RobotOutlined />} />
                  </Badge>
                  <div>
                    <div style={{ fontWeight: 600 }}>{b.name}</div>
                    <Typography.Text type="secondary" style={{ fontSize: 12 }} code>ai-chat:{b.key}</Typography.Text>
                  </div>
                </Space>
              </Card>
            )
          })}
        </Space>
      </Col>
      <Col flex="auto" style={{ minWidth: 0 }}>{sel && <BotEditor key={sel.key} t={t} bot={sel} refresh={refresh} />}</Col>
      <Modal title={t('newBot')} open={creating} onCancel={() => setCreating(false)} onOk={() => form.submit()} okText={t('save')}>
        <Form form={form} layout="vertical" onFinish={doCreate}>
          <Form.Item name="name" label={t('name')} rules={[{ required: true }]}>
            <Input size="large" placeholder="Trợ lý bán hàng" prefix={<RobotOutlined />} />
          </Form.Item>
        </Form>
      </Modal>
    </Row>
  )
}

/** One toggle row: icon + label + description + Switch. */
function SwitchRow({ icon, label, desc, checked, onChange }: { icon: ReactNode; label: string; desc: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 12, padding: '10px 14px', border: '1px solid var(--app-border)', borderRadius: 8 }}>
      <span style={{ fontSize: 18, color: checked ? '#1890ff' : '#8c8c8c', display: 'flex' }}>{icon}</span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontWeight: 500 }}>{label}</div>
        <div style={{ fontSize: 12, opacity: 0.6 }}>{desc}</div>
      </div>
      <Switch checked={checked} onChange={onChange} />
    </div>
  )
}

function BotEditor({ t, bot, refresh }: { t: T; bot: Bot; refresh: () => void }) {
  const [f, setF] = useState<Bot>(bot)
  const [inv, setInv] = useState<Inventory | null>(null)
  const [skills, setSkills] = useState<SkillInventory | null>(null)
  const [saving, setSaving] = useState(false)

  useEffect(() => setF(bot), [bot])
  useEffect(() => {
    api.mcpInventory().then(setInv).catch(() => {})
    api.skillsInventory().then(setSkills).catch(() => {})
  }, [])

  const set = <K extends keyof Bot>(k: K, v: Bot[K]) => setF((p) => ({ ...p, [k]: v }))
  const toggleGroup = (opts: Array<{ value: string }>, checked: string[]) => {
    const groupVals = new Set(opts.map((o) => o.value))
    set('allowed_mcp', [...f.allowed_mcp.filter((x) => !groupVals.has(x)), ...checked])
  }

  const groups = useMemo(() => {
    if (!inv) return []
    const g: Array<{ key: string; label: string; opts: Array<{ label: string; value: string }> }> = [
      { key: 'core', label: t('coreTools'), opts: inv.core.map((x) => ({ label: `${x.name}${x.description ? ' — ' + x.description : ''}`, value: x.name })) },
    ]
    for (const s of inv.servers) {
      g.push({ key: s.name, label: s.name + (s.builtin ? ' (builtin)' : ''), opts: s.tools.map((x) => ({ label: x.name.replace(/^mcp__[^_]+__/, ''), value: x.name })) })
    }
    return g
  }, [inv, t])

  const save = async () => {
    setSaving(true)
    try {
      await api.updateBot(bot.key, {
        name: f.name, system_prompt: f.system_prompt, greeting: f.greeting, model: f.model,
        knowledge_scope: f.knowledge_scope, allowed_mcp: f.allowed_mcp, allowed_skills: f.allowed_skills,
        use_tools: f.use_tools, use_knowledge: f.use_knowledge, auto_ingest: f.auto_ingest,
        auto_issue: f.auto_issue, enabled: f.enabled,
      })
      message.success(t('saved'))
      refresh()
    } catch (e) {
      message.error(String(e))
    } finally {
      setSaving(false)
    }
  }
  const del = async () => { await api.deleteBot(bot.key); refresh() }

  const toolCount = f.allowed_mcp.length

  return (
    <Space direction="vertical" size={16} style={{ width: '100%', maxWidth: 900 }}>
      {/* Header strip: identity + actions */}
      <Card styles={{ body: { padding: '14px 20px' } }}>
        <Row align="middle" gutter={12}>
          <Col flex="none">
            <Avatar size={44} shape="square" style={{ background: 'linear-gradient(135deg,#36CFC9,#1890FF,#2F54EB)' }} icon={<RobotOutlined />} />
          </Col>
          <Col flex="auto">
            <div style={{ fontSize: 17, fontWeight: 700 }}>{f.name || t('name')}</div>
            <Space size={6} wrap>
              <Tag color={f.enabled ? 'green' : 'default'} style={{ marginInlineEnd: 0 }}>{f.enabled ? t('enabled') : 'off'}</Tag>
              {f.use_tools && <Tag color="blue">{toolCount} tools</Tag>}
              {f.use_knowledge && <Tag color="cyan">RAG</Tag>}
              {f.auto_issue && <Tag color="orange">auto-ticket</Tag>}
            </Space>
          </Col>
          <Col flex="none">
            <Space>
              <Button type="primary" icon={<SaveOutlined />} loading={saving} onClick={save}>{t('save')}</Button>
              <Popconfirm title={t('confirmDelete')} onConfirm={del}>
                <Tooltip title={t('delete')}><Button danger icon={<DeleteOutlined />} /></Tooltip>
              </Popconfirm>
            </Space>
          </Col>
        </Row>
      </Card>

      {/* Profile */}
      <Card title={<Space><IdcardOutlined />{t('profile')}</Space>}>
        <Form layout="vertical" size="large">
          <Form.Item label={t('name')} style={{ marginBottom: 14 }}>
            <Input value={f.name} onChange={(e) => set('name', e.target.value)} />
          </Form.Item>
          <Form.Item label={t('greeting')} style={{ marginBottom: 14 }}>
            <Input value={f.greeting} onChange={(e) => set('greeting', e.target.value)} />
          </Form.Item>
          <Form.Item label={t('systemPrompt')} style={{ marginBottom: 0 }}>
            <Input.TextArea rows={4} value={f.system_prompt} onChange={(e) => set('system_prompt', e.target.value)} />
          </Form.Item>
        </Form>
      </Card>

      {/* Behaviour */}
      <Card title={<Space><BulbOutlined />{t('behaviour')}</Space>}>
        <Row gutter={16} style={{ marginBottom: 16 }}>
          <Col span={12}>
            <Typography.Text type="secondary">{t('model')}</Typography.Text>
            <Input value={f.model} placeholder="(default)" onChange={(e) => set('model', e.target.value)} />
          </Col>
          <Col span={12}>
            <Typography.Text type="secondary">{t('knowledgeScope')}</Typography.Text>
            <br />
            <Segmented
              value={f.knowledge_scope}
              onChange={(v) => set('knowledge_scope', String(v))}
              options={[{ label: 'Bot', value: 'bot' }, { label: 'Session', value: 'session' }, { label: 'User', value: 'user' }]}
            />
          </Col>
        </Row>
        <Row gutter={[12, 12]}>
          <Col span={12}><SwitchRow icon={<PoweroffOutlined />} label={t('enabled')} desc="Bật/tắt toàn bộ bot" checked={f.enabled} onChange={(v) => set('enabled', v)} /></Col>
          <Col span={12}><SwitchRow icon={<ReadOutlined />} label={t('useKnowledge')} desc="Chèn kiến thức liên quan vào câu trả lời" checked={f.use_knowledge} onChange={(v) => set('use_knowledge', v)} /></Col>
          <Col span={12}><SwitchRow icon={<ReadOutlined />} label={t('autoIngest')} desc="Tự lưu hội thoại vào kiến thức" checked={f.auto_ingest} onChange={(v) => set('auto_ingest', v)} /></Col>
          <Col span={12}><SwitchRow icon={<WarningOutlined />} label={t('autoIssue')} desc="Bot tự mở ticket khi phát hiện vấn đề" checked={f.auto_issue} onChange={(v) => set('auto_issue', v)} /></Col>
          <Col span={12}><SwitchRow icon={<ToolOutlined />} label={t('useTools')} desc="Cho phép bot dùng công cụ (theo allowlist)" checked={f.use_tools} onChange={(v) => set('use_tools', v)} /></Col>
        </Row>
      </Card>

      {/* Tool policy */}
      {f.use_tools && (
        <Card title={<Space><LockOutlined style={{ color: '#faad14' }} />{t('toolPolicy')}</Space>} extra={<Tag color="blue">{toolCount}</Tag>}>
          <Typography.Paragraph type="secondary" style={{ marginTop: -4 }}>{t('toolPolicyHint')}</Typography.Paragraph>
          <Collapse
            size="small"
            items={groups.map((g) => {
              const groupVals = new Set(g.opts.map((o) => o.value))
              const checked = f.allowed_mcp.filter((x) => groupVals.has(x))
              return {
                key: g.key,
                label: <Space>{g.label}{checked.length > 0 && <Tag color="blue" style={{ marginInlineEnd: 0 }}>{checked.length}</Tag>}</Space>,
                children: (
                  <Checkbox.Group value={checked} onChange={(v) => toggleGroup(g.opts, v as string[])} options={g.opts} style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 4 }} />
                ),
              }
            })}
          />
        </Card>
      )}

      {/* Skills */}
      <Card title={<Space><ApiOutlined />{t('skillPolicy')}</Space>}>
        <Select
          mode="multiple"
          allowClear
          size="large"
          style={{ width: '100%' }}
          placeholder={t('skillPolicy')}
          value={f.allowed_skills}
          onChange={(v) => set('allowed_skills', v)}
          options={(skills?.skills || []).map((s) => ({ label: s.name, value: s.name }))}
        />
      </Card>

      <Divider style={{ margin: '4px 0' }} />
      <Space>
        <Button type="primary" size="large" icon={<SaveOutlined />} loading={saving} onClick={save}>{t('save')}</Button>
        <Popconfirm title={t('confirmDelete')} onConfirm={del}>
          <Button danger size="large" icon={<DeleteOutlined />}>{t('delete')}</Button>
        </Popconfirm>
      </Space>
    </Space>
  )
}
