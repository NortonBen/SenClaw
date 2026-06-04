import { useState, useEffect, useRef, useCallback } from 'react';
import { theme, Typography, Button, Tag, Input, Space, message, Flex, Select, Switch, Tooltip, Modal, Collapse, Badge, Empty, Divider, InputNumber } from 'antd';
import {
  PlusOutlined,
  DeleteOutlined,
  SaveOutlined,
  CodeOutlined,
  ThunderboltOutlined,
  EditOutlined,
  CheckCircleFilled,
  ExclamationCircleFilled,
  CopyOutlined,
  QuestionCircleOutlined,
  DragOutlined,
  CloseOutlined,
  EyeOutlined,
  SettingOutlined,
} from '@ant-design/icons';

const { Text, Title } = Typography;
const { TextArea } = Input;
const { Panel } = Collapse;
const { Option } = Select;

// ─── Types ────────────────────────────────────────────────────────────────────

interface HookItem {
  id: string;
  type: 'command' | 'prompt';
  command?: string;
  prompt?: string;
  include_history?: boolean;
  history_limit?: number;
  timeout?: number;
  blocking?: boolean;
  async?: boolean;
}

interface EventConfig {
  id: string;
  matcher?: string;
  if?: string;
  hooks: HookItem[];
}

interface HooksData {
  hooks: Record<string, EventConfig[]>;
}

// ─── Constants ────────────────────────────────────────────────────────────────

const ALL_EVENTS = [
  { name: 'UserPromptSubmit', color: '#6366f1', desc: 'Fired when user submits a prompt' },
  { name: 'PreToolUse', color: '#8b5cf6', desc: 'Before any tool is called' },
  { name: 'PostToolUse', color: '#06b6d4', desc: 'After any tool completes' },
  { name: 'PermissionRequest', color: '#f59e0b', desc: 'When permission is requested' },
  { name: 'Stop', color: '#ef4444', desc: 'When the agent stops' },
  { name: 'SessionStart', color: '#10b981', desc: 'When a session begins' },
  { name: 'SessionEnd', color: '#6b7280', desc: 'When a session ends' },
  { name: 'PreCompact', color: '#ec4899', desc: 'Before context compaction' },
  { name: 'PostCompact', color: '#14b8a6', desc: 'After context compaction' },
  { name: 'Error', color: '#f43f5e', desc: 'On agent error' },
];

const EVENT_MAP = Object.fromEntries(ALL_EVENTS.map(e => [e.name, e]));

// ─── Helpers ─────────────────────────────────────────────────────────────────

function uid() {
  return Math.random().toString(36).slice(2, 9);
}

function hooksDataToModel(data: HooksData): Record<string, EventConfig[]> {
  const result: Record<string, EventConfig[]> = {};
  for (const [event, configs] of Object.entries(data.hooks ?? {})) {
    result[event] = (configs as any[]).map(cfg => ({
      id: uid(),
      matcher: cfg.matcher,
      if: cfg.if,
      hooks: (cfg.hooks ?? []).map((h: any) => ({
        id: uid(),
        type: h.type ?? 'command',
        command: h.command,
        prompt: h.prompt,
        include_history: h.include_history,
        history_limit: h.history_limit,
        timeout: h.timeout,
        blocking: h.blocking,
        async: h.async,
      })),
    }));
  }
  return result;
}

function modelToHooksData(model: Record<string, EventConfig[]>): HooksData {
  const hooks: Record<string, any[]> = {};
  for (const [event, configs] of Object.entries(model)) {
    if (configs.length === 0) continue;
    hooks[event] = configs.map(cfg => {
      const out: any = {};
      if (cfg.matcher) out.matcher = cfg.matcher;
      if (cfg.if) out.if = cfg.if;
      out.hooks = cfg.hooks.map(h => {
        const hOut: any = { type: h.type };
        if (h.type === 'command' && h.command) hOut.command = h.command;
        if (h.type === 'prompt' && h.prompt) hOut.prompt = h.prompt;
        if (h.include_history !== undefined) hOut.include_history = h.include_history;
        if (h.history_limit !== undefined) hOut.history_limit = h.history_limit;
        if (h.timeout !== undefined) hOut.timeout = h.timeout;
        if (h.blocking !== undefined) hOut.blocking = h.blocking;
        if (h.async !== undefined) hOut.async = h.async;
        return hOut;
      });
      return out;
    });
  }
  return { hooks };
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function HookItemEditor({
  hook,
  onChange,
  onDelete,
  token,
}: {
  hook: HookItem;
  onChange: (h: HookItem) => void;
  onDelete: () => void;
  token: any;
}) {
  const [expanded, setExpanded] = useState(false);

  return (
    <div style={{
      background: token.colorFillAlter,
      border: `1px solid ${token.colorBorderSecondary}`,
      borderRadius: token.borderRadiusLG,
      overflow: 'hidden',
    }}>
      {/* Hook header */}
      <Flex
        align="center"
        gap={8}
        style={{ padding: '8px 12px', cursor: 'pointer', userSelect: 'none' }}
        onClick={() => setExpanded(e => !e)}
      >
        <div style={{
          width: 6, height: 6, borderRadius: '50%',
          background: hook.type === 'command' ? '#6366f1' : '#06b6d4',
          flexShrink: 0,
        }} />
        <Tag
          color={hook.type === 'command' ? 'purple' : 'cyan'}
          style={{ margin: 0, fontSize: '10px', letterSpacing: '0.03em' }}
        >
          {hook.type}
        </Tag>
        <Text
          style={{
            flex: 1,
            fontSize: '12px',
            color: token.colorText,
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            whiteSpace: 'nowrap',
            fontFamily: token.fontFamilyCode,
          }}
        >
          {hook.type === 'command'
            ? (hook.command || <span style={{ color: token.colorTextDisabled }}>No command set</span>)
            : (hook.prompt || <span style={{ color: token.colorTextDisabled }}>No prompt set</span>)
          }
        </Text>
        <Button
          type="text"
          size="small"
          icon={<SettingOutlined />}
          style={{ color: token.colorTextTertiary, fontSize: '11px' }}
          onClick={e => { e.stopPropagation(); setExpanded(x => !x); }}
        />
        <Button
          type="text"
          size="small"
          danger
          icon={<DeleteOutlined />}
          style={{ fontSize: '11px' }}
          onClick={e => { e.stopPropagation(); onDelete(); }}
        />
      </Flex>

      {/* Hook body */}
      {expanded && (
        <div style={{ padding: '0 12px 12px', borderTop: `1px solid ${token.colorBorderSecondary}` }}>
          <Flex vertical gap={10} style={{ paddingTop: 10 }}>
            {/* Type */}
            <Flex align="center" gap={8}>
              <Text style={{ fontSize: '12px', color: token.colorTextSecondary, width: 90, flexShrink: 0 }}>Type *</Text>
              <Select
                size="small"
                value={hook.type}
                onChange={v => onChange({ ...hook, type: v, command: undefined, prompt: undefined })}
                style={{ width: 120 }}
              >
                <Option value="command">command</Option>
                <Option value="prompt">prompt</Option>
              </Select>
            </Flex>

            {/* Command / Prompt */}
            {hook.type === 'command' ? (
              <Flex align="flex-start" gap={8}>
                <Text style={{ fontSize: '12px', color: token.colorTextSecondary, width: 90, flexShrink: 0, paddingTop: 4 }}>Command *</Text>
                <Input
                  size="small"
                  value={hook.command ?? ''}
                  onChange={e => onChange({ ...hook, command: e.target.value })}
                  placeholder="e.g. echo done"
                  style={{ fontFamily: token.fontFamilyCode, fontSize: '12px', flex: 1 }}
                />
              </Flex>
            ) : (
              <Flex align="flex-start" gap={8}>
                <Text style={{ fontSize: '12px', color: token.colorTextSecondary, width: 90, flexShrink: 0, paddingTop: 4 }}>Prompt *</Text>
                <TextArea
                  size="small"
                  rows={2}
                  value={hook.prompt ?? ''}
                  onChange={e => onChange({ ...hook, prompt: e.target.value })}
                  placeholder="e.g. Review this tool call for security issues"
                  style={{ fontFamily: token.fontFamilyCode, fontSize: '12px', flex: 1, resize: 'vertical' }}
                />
              </Flex>
            )}

            {/* Advanced options row */}
            <Divider style={{ margin: '4px 0', fontSize: '10px', color: token.colorTextTertiary }}>Advanced</Divider>

            <Flex wrap="wrap" gap={16}>
              {hook.type === 'prompt' && (
                <>
                  <Flex align="center" gap={6}>
                    <Text style={{ fontSize: '11px', color: token.colorTextSecondary }}>include_history</Text>
                    <Switch
                      size="small"
                      checked={hook.include_history ?? false}
                      onChange={v => onChange({ ...hook, include_history: v })}
                    />
                  </Flex>
                  {hook.include_history && (
                    <Flex align="center" gap={6}>
                      <Text style={{ fontSize: '11px', color: token.colorTextSecondary }}>history_limit</Text>
                      <InputNumber
                        size="small"
                        min={1} max={100}
                        value={hook.history_limit ?? 10}
                        onChange={v => onChange({ ...hook, history_limit: v ?? 10 })}
                        style={{ width: 65 }}
                      />
                    </Flex>
                  )}
                </>
              )}
              <Flex align="center" gap={6}>
                <Text style={{ fontSize: '11px', color: token.colorTextSecondary }}>timeout (s)</Text>
                <InputNumber
                  size="small"
                  min={1} max={600}
                  value={hook.timeout ?? 10}
                  onChange={v => onChange({ ...hook, timeout: v ?? 10 })}
                  style={{ width: 65 }}
                />
              </Flex>
              <Flex align="center" gap={6}>
                <Text style={{ fontSize: '11px', color: token.colorTextSecondary }}>blocking</Text>
                <Switch
                  size="small"
                  checked={hook.blocking ?? true}
                  onChange={v => onChange({ ...hook, blocking: v })}
                />
              </Flex>
              <Flex align="center" gap={6}>
                <Text style={{ fontSize: '11px', color: token.colorTextSecondary }}>async</Text>
                <Switch
                  size="small"
                  checked={hook.async ?? false}
                  onChange={v => onChange({ ...hook, async: v })}
                />
              </Flex>
            </Flex>
          </Flex>
        </div>
      )}
    </div>
  );
}

function EventConfigCard({
  event,
  configs,
  onUpdate,
  onDeleteConfig,
  onAddConfig,
  token,
}: {
  event: string;
  configs: EventConfig[];
  onUpdate: (id: string, cfg: EventConfig) => void;
  onDeleteConfig: (id: string) => void;
  onAddConfig: () => void;
  token: any;
}) {
  const meta = EVENT_MAP[event] ?? { color: '#6b7280', desc: '' };

  return (
    <div style={{
      background: token.colorBgContainer,
      border: `1px solid ${token.colorBorderSecondary}`,
      borderRadius: token.borderRadiusLG,
      overflow: 'hidden',
    }}>
      {/* Event header */}
      <Flex
        align="center"
        gap={10}
        style={{
          padding: '10px 16px',
          background: `linear-gradient(90deg, ${meta.color}18 0%, transparent 100%)`,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
        }}
      >
        <div style={{
          width: 8, height: 8, borderRadius: '50%',
          background: meta.color,
          boxShadow: `0 0 6px ${meta.color}80`,
          flexShrink: 0,
        }} />
        <Text strong style={{ fontSize: '13px', color: token.colorText, fontFamily: token.fontFamilyCode }}>
          {event}
        </Text>
        <Badge count={configs.reduce((s, c) => s + c.hooks.length, 0)} color={meta.color} style={{ fontSize: '10px' }} />
        <Text type="secondary" style={{ fontSize: '11px', flex: 1 }}>{meta.desc}</Text>
      </Flex>

      {/* Configs */}
      <div style={{ padding: '12px 16px' }}>
        <Flex vertical gap={12}>
          {configs.map((cfg, ci) => (
            <div
              key={cfg.id}
              style={{
                border: `1px solid ${token.colorBorderSecondary}`,
                borderRadius: token.borderRadius,
                overflow: 'hidden',
                background: token.colorBgLayout,
              }}
            >
              {/* Config header */}
              <Flex
                align="center"
                gap={8}
                style={{ padding: '8px 12px', background: token.colorFillAlter, borderBottom: `1px solid ${token.colorBorderSecondary}` }}
              >
                <DragOutlined style={{ color: token.colorTextTertiary, cursor: 'grab', fontSize: '12px' }} />
                <Text style={{ fontSize: '11px', color: token.colorTextSecondary }}>Config #{ci + 1}</Text>

                <Flex align="center" gap={4} style={{ marginLeft: 8 }}>
                  <Text style={{ fontSize: '11px', color: token.colorTextTertiary }}>matcher:</Text>
                  <Input
                    size="small"
                    value={cfg.matcher ?? ''}
                    onChange={e => onUpdate(cfg.id, { ...cfg, matcher: e.target.value || undefined })}
                    placeholder="* (all tools)"
                    style={{ width: 120, fontSize: '11px', fontFamily: token.fontFamilyCode }}
                  />
                </Flex>
                <Flex align="center" gap={4}>
                  <Text style={{ fontSize: '11px', color: token.colorTextTertiary }}>if:</Text>
                  <Input
                    size="small"
                    value={cfg.if ?? ''}
                    onChange={e => onUpdate(cfg.id, { ...cfg, if: e.target.value || undefined })}
                    placeholder="regex (optional)"
                    style={{ width: 130, fontSize: '11px', fontFamily: token.fontFamilyCode }}
                  />
                </Flex>

                <div style={{ flex: 1 }} />
                <Button
                  type="text" size="small" danger
                  icon={<CloseOutlined />}
                  style={{ fontSize: '11px' }}
                  onClick={() => onDeleteConfig(cfg.id)}
                />
              </Flex>

              {/* Hook items */}
              <div style={{ padding: '10px 12px' }}>
                <Flex vertical gap={8}>
                  {cfg.hooks.length === 0 && (
                    <Text type="secondary" style={{ fontSize: '12px', textAlign: 'center', padding: '8px 0' }}>
                      No hooks — add one below
                    </Text>
                  )}
                  {cfg.hooks.map(hook => (
                    <HookItemEditor
                      key={hook.id}
                      hook={hook}
                      token={token}
                      onChange={updated => {
                        onUpdate(cfg.id, {
                          ...cfg,
                          hooks: cfg.hooks.map(h => h.id === updated.id ? updated : h),
                        });
                      }}
                      onDelete={() => {
                        onUpdate(cfg.id, {
                          ...cfg,
                          hooks: cfg.hooks.filter(h => h.id !== hook.id),
                        });
                      }}
                    />
                  ))}
                  <Button
                    type="dashed"
                    size="small"
                    icon={<PlusOutlined />}
                    onClick={() => {
                      onUpdate(cfg.id, {
                        ...cfg,
                        hooks: [...cfg.hooks, { id: uid(), type: 'command', command: '' }],
                      });
                    }}
                    style={{ fontSize: '11px', height: 28 }}
                  >
                    Add Hook
                  </Button>
                </Flex>
              </div>
            </div>
          ))}

          <Button
            type="dashed"
            size="small"
            icon={<PlusOutlined />}
            onClick={onAddConfig}
            style={{ fontSize: '11px', height: 28 }}
          >
            Add Config Group
          </Button>
        </Flex>
      </div>
    </div>
  );
}

// ─── Main Component ───────────────────────────────────────────────────────────

export function HooksPanel() {
  const { token } = theme.useToken();
  const [model, setModel] = useState<Record<string, EventConfig[]>>({});
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [selectedEvent, setSelectedEvent] = useState<string | null>(null);
  const [showJson, setShowJson] = useState(false);
  const [activeTab, setActiveTab] = useState<'builder' | 'json'>('builder');
  const [rawJson, setRawJson] = useState('');
  const [jsonError, setJsonError] = useState<string | null>(null);
  const successTimer = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => {
    fetch('/api/hooks')
      .then(r => r.json())
      .then(data => {
        const m = hooksDataToModel(data);
        setModel(m);
        setRawJson(JSON.stringify(modelToHooksData(m), null, 2));
        setLoading(false);
      })
      .catch(() => {
        setModel({});
        setRawJson('{\n  "hooks": {}\n}');
        setLoading(false);
      });
  }, []);

  const syncJsonFromModel = useCallback((m: Record<string, EventConfig[]>) => {
    setRawJson(JSON.stringify(modelToHooksData(m), null, 2));
  }, []);

  async function handleSave() {
    setError(null);
    setSuccess(false);
    setSaving(true);
    try {
      let payload: any;
      if (activeTab === 'json') {
        try { payload = JSON.parse(rawJson); } catch (e) {
          setError(`Invalid JSON: ${(e as Error).message}`);
          setSaving(false);
          return;
        }
      } else {
        payload = modelToHooksData(model);
      }
      const res = await fetch('/api/hooks', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      if (!res.ok) {
        const data = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
        setError((data as { error?: string }).error ?? `HTTP ${res.status}`);
      } else {
        setSuccess(true);
        message.success('Hooks saved successfully');
        clearTimeout(successTimer.current);
        successTimer.current = setTimeout(() => setSuccess(false), 2500);
      }
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  }

  function addEvent(eventName: string) {
    if (model[eventName]) return;
    const next = { ...model, [eventName]: [] };
    setModel(next);
    syncJsonFromModel(next);
    setSelectedEvent(eventName);
  }

  function removeEvent(eventName: string) {
    const next = { ...model };
    delete next[eventName];
    setModel(next);
    syncJsonFromModel(next);
    if (selectedEvent === eventName) setSelectedEvent(null);
  }

  function updateConfig(eventName: string, id: string, cfg: EventConfig) {
    const next = {
      ...model,
      [eventName]: model[eventName].map(c => c.id === id ? cfg : c),
    };
    setModel(next);
    syncJsonFromModel(next);
  }

  function deleteConfig(eventName: string, id: string) {
    const next = {
      ...model,
      [eventName]: model[eventName].filter(c => c.id !== id),
    };
    setModel(next);
    syncJsonFromModel(next);
  }

  function addConfig(eventName: string) {
    const next = {
      ...model,
      [eventName]: [...(model[eventName] ?? []), { id: uid(), hooks: [] }],
    };
    setModel(next);
    syncJsonFromModel(next);
  }

  const activeEvents = Object.keys(model);
  const inactiveEvents = ALL_EVENTS.filter(e => !activeEvents.includes(e.name));
  const currentConfigs = selectedEvent ? (model[selectedEvent] ?? []) : [];

  // ── Render ──

  return (
    <Flex style={{ height: '100%', background: token.colorBgLayout, overflow: 'hidden' }}>

      {/* ── LEFT SIDEBAR: Event list ── */}
      <Flex
        vertical
        style={{
          width: 220,
          flexShrink: 0,
          borderRight: `1px solid ${token.colorBorderSecondary}`,
          background: token.colorBgContainer,
          overflow: 'hidden',
        }}
      >
        {/* Sidebar header */}
        <div style={{
          padding: '14px 16px 10px',
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
        }}>
          <Flex align="center" gap={8}>
            <ThunderboltOutlined style={{ color: token.colorPrimary, fontSize: '14px' }} />
            <Text strong style={{ fontSize: '13px' }}>Events</Text>
            <Badge count={activeEvents.length} color={token.colorPrimary} size="small" />
          </Flex>
        </div>

        {/* Active events */}
        <div style={{ flex: 1, overflowY: 'auto', padding: '8px 8px' }}>
          {activeEvents.length === 0 && (
            <Text type="secondary" style={{ fontSize: '11px', padding: '8px', display: 'block' }}>
              No events configured
            </Text>
          )}
          {activeEvents.map(name => {
            const meta = EVENT_MAP[name] ?? { color: '#6b7280' };
            const isSelected = selectedEvent === name;
            const count = (model[name] ?? []).reduce((s, c) => s + c.hooks.length, 0);
            return (
              <Flex
                key={name}
                align="center"
                gap={8}
                onClick={() => setSelectedEvent(isSelected ? null : name)}
                style={{
                  padding: '7px 10px',
                  borderRadius: token.borderRadius,
                  cursor: 'pointer',
                  marginBottom: 2,
                  background: isSelected ? `${meta.color}20` : 'transparent',
                  border: `1px solid ${isSelected ? meta.color + '60' : 'transparent'}`,
                  transition: 'all 0.15s',
                }}
              >
                <div style={{
                  width: 7, height: 7, borderRadius: '50%',
                  background: meta.color,
                  boxShadow: isSelected ? `0 0 5px ${meta.color}` : 'none',
                  flexShrink: 0,
                }} />
                <Text style={{
                  fontSize: '12px',
                  flex: 1,
                  color: isSelected ? meta.color : token.colorText,
                  fontWeight: isSelected ? 600 : 400,
                  fontFamily: token.fontFamilyCode,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}>
                  {name}
                </Text>
                {count > 0 && (
                  <Badge count={count} color={meta.color} size="small" style={{ fontSize: '9px' }} />
                )}
                <Button
                  type="text" size="small" danger
                  icon={<CloseOutlined style={{ fontSize: '10px' }} />}
                  style={{ padding: '0 2px', height: 'auto', opacity: 0.5 }}
                  onClick={e => { e.stopPropagation(); removeEvent(name); }}
                />
              </Flex>
            );
          })}
        </div>

        {/* Add event */}
        <div style={{ padding: '8px', borderTop: `1px solid ${token.colorBorderSecondary}` }}>
          <Text style={{ fontSize: '11px', color: token.colorTextTertiary, display: 'block', marginBottom: 6, paddingLeft: 4 }}>
            Add Event
          </Text>
          <Flex vertical gap={2}>
            {inactiveEvents.map(({ name, color }) => (
              <Button
                key={name}
                type="text"
                size="small"
                icon={<PlusOutlined style={{ fontSize: '10px', color }} />}
                onClick={() => addEvent(name)}
                style={{
                  width: '100%',
                  textAlign: 'left',
                  fontSize: '11px',
                  height: 26,
                  justifyContent: 'flex-start',
                  color: token.colorTextSecondary,
                  fontFamily: token.fontFamilyCode,
                }}
              >
                {name}
              </Button>
            ))}
            {inactiveEvents.length === 0 && (
              <Text type="secondary" style={{ fontSize: '11px', padding: '4px', textAlign: 'center' }}>
                All events added
              </Text>
            )}
          </Flex>
        </div>
      </Flex>

      {/* ── MAIN CONTENT ── */}
      <Flex vertical style={{ flex: 1, overflow: 'hidden' }}>

        {/* Top bar */}
        <Flex
          align="center"
          gap={8}
          style={{
            padding: '10px 20px',
            borderBottom: `1px solid ${token.colorBorderSecondary}`,
            background: token.colorBgContainer,
            flexShrink: 0,
          }}
        >
          <ThunderboltOutlined style={{ color: token.colorPrimary, fontSize: '16px' }} />
          <Title level={5} style={{ margin: 0, fontSize: '14px' }}>Hooks Configuration</Title>
          <div style={{ flex: 1 }} />

          {/* Tab switcher */}
          <Flex
            style={{
              background: token.colorFillAlter,
              borderRadius: token.borderRadius,
              padding: 2,
              border: `1px solid ${token.colorBorderSecondary}`,
            }}
          >
            {(['builder', 'json'] as const).map(tab => (
              <Button
                key={tab}
                type={activeTab === tab ? 'primary' : 'text'}
                size="small"
                icon={tab === 'builder' ? <EditOutlined /> : <CodeOutlined />}
                onClick={() => setActiveTab(tab)}
                style={{ fontSize: '12px', borderRadius: token.borderRadiusSM, height: 26 }}
              >
                {tab === 'builder' ? 'Visual' : 'JSON'}
              </Button>
            ))}
          </Flex>

          {/* Copy JSON */}
          <Tooltip title="Copy JSON">
            <Button
              type="text"
              size="small"
              icon={<CopyOutlined />}
              onClick={() => {
                navigator.clipboard.writeText(rawJson);
                message.success('Copied to clipboard');
              }}
              style={{ fontSize: '13px' }}
            />
          </Tooltip>

          {/* Save */}
          <Button
            type="primary"
            icon={<SaveOutlined />}
            onClick={handleSave}
            loading={saving}
            disabled={loading}
            size="middle"
            style={{ borderRadius: token.borderRadius, minWidth: 120 }}
          >
            {saving ? 'Saving…' : 'Save Configuration'}
          </Button>
        </Flex>

        {/* Status bar */}
        {(error || success) && (
          <div style={{
            padding: '6px 20px',
            background: error ? `${token.colorError}10` : `${token.colorSuccess}10`,
            borderBottom: `1px solid ${error ? token.colorErrorBorder : token.colorSuccessBorder}`,
            flexShrink: 0,
          }}>
            <Space size={6}>
              {error
                ? <ExclamationCircleFilled style={{ color: token.colorError, fontSize: '13px' }} />
                : <CheckCircleFilled style={{ color: token.colorSuccess, fontSize: '13px' }} />
              }
              <Text
                style={{ fontSize: '12px', color: error ? token.colorError : token.colorSuccess }}
              >
                {error ?? 'Saved successfully'}
              </Text>
            </Space>
          </div>
        )}

        {/* Content */}
        <div style={{ flex: 1, overflow: 'hidden', display: 'flex' }}>
          {activeTab === 'builder' ? (
            // ── BUILDER TAB ──
            <div style={{ flex: 1, overflowY: 'auto', padding: '20px' }}>
              {loading ? (
                <Flex align="center" justify="center" style={{ height: '100%' }}>
                  <Text type="secondary">Loading…</Text>
                </Flex>
              ) : !selectedEvent ? (
                <Flex vertical align="center" justify="center" style={{ height: '200px' }} gap={12}>
                  <ThunderboltOutlined style={{ fontSize: '32px', color: token.colorTextTertiary }} />
                  <Text type="secondary" style={{ fontSize: '13px' }}>
                    {activeEvents.length === 0
                      ? 'Select an event from the left sidebar to get started'
                      : 'Select an event from the left sidebar to configure hooks'
                    }
                  </Text>
                  {activeEvents.length === 0 && (
                    <Text type="secondary" style={{ fontSize: '12px', color: token.colorTextQuaternary }}>
                      Click "+ Add Event" to begin
                    </Text>
                  )}
                </Flex>
              ) : (
                <EventConfigCard
                  event={selectedEvent}
                  configs={currentConfigs}
                  token={token}
                  onUpdate={(id, cfg) => updateConfig(selectedEvent, id, cfg)}
                  onDeleteConfig={(id) => deleteConfig(selectedEvent, id)}
                  onAddConfig={() => addConfig(selectedEvent)}
                />
              )}
            </div>
          ) : (
            // ── JSON TAB ──
            <Flex vertical style={{ flex: 1, padding: '16px', gap: 8 }}>
              {jsonError && (
                <Space size={4}>
                  <ExclamationCircleFilled style={{ color: token.colorError, fontSize: '12px' }} />
                  <Text type="danger" style={{ fontSize: '11px' }}>{jsonError}</Text>
                </Space>
              )}
              <TextArea
                value={rawJson}
                onChange={e => {
                  setRawJson(e.target.value);
                  setJsonError(null);
                  try {
                    const parsed = JSON.parse(e.target.value);
                    setModel(hooksDataToModel(parsed));
                  } catch (err) {
                    setJsonError(`Invalid JSON: ${(err as Error).message}`);
                  }
                }}
                spellCheck={false}
                style={{
                  flex: 1,
                  height: '100%',
                  fontFamily: token.fontFamilyCode,
                  fontSize: '12px',
                  lineHeight: '1.6',
                  backgroundColor: token.colorBgContainer,
                  border: `1px solid ${jsonError ? token.colorError : token.colorBorderSecondary}`,
                  borderRadius: token.borderRadiusLG,
                  padding: '16px',
                  resize: 'none',
                }}
              />
            </Flex>
          )}
        </div>
      </Flex>
    </Flex>
  );
}
