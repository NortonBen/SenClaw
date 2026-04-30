import { useState, useEffect, useRef } from 'react';
import { theme, Typography, Button, Tag, Input, Space, Tooltip, message, Flex } from 'antd';
import {
  RightOutlined,
  InfoCircleOutlined,
  SaveOutlined,
  CheckCircleFilled,
  ExclamationCircleFilled
} from '@ant-design/icons';

const { Text, Title, Paragraph } = Typography;
const { TextArea } = Input;

function validateHooksJson(text: string): string | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch (e) {
    return `Invalid JSON: ${(e as Error).message}`;
  }
  if (typeof parsed !== 'object' || parsed === null || !('hooks' in parsed)) {
    return 'Root object must have a "hooks" key';
  }
  const hooks = (parsed as Record<string, unknown>).hooks;
  if (typeof hooks !== 'object' || hooks === null || Array.isArray(hooks)) {
    return '"hooks" must be a plain object';
  }
  for (const [event, configs] of Object.entries(hooks as Record<string, unknown>)) {
    if (!Array.isArray(configs)) {
      return `Event "${event}": value must be an array`;
    }
    for (let i = 0; i < configs.length; i++) {
      const cfg = configs[i] as Record<string, unknown>;
      if (!Array.isArray(cfg?.hooks)) {
        return `Event "${event}"[${i}]: each item must have a "hooks" array`;
      }
      for (let j = 0; j < (cfg.hooks as unknown[]).length; j++) {
        const hook = (cfg.hooks as Record<string, unknown>[])[j];
        if (hook.type !== 'command' && hook.type !== 'prompt') {
          return `Event "${event}"[${i}].hooks[${j}]: type must be "command" or "prompt"`;
        }
        if (hook.type === 'command' && !hook.command) {
          return `Event "${event}"[${i}].hooks[${j}]: type "command" requires a "command" field`;
        }
        if (hook.type === 'prompt' && !hook.prompt) {
          return `Event "${event}"[${i}].hooks[${j}]: type "prompt" requires a "prompt" field`;
        }
      }
    }
  }
  return null;
}

// Events that are fully injected in the codebase
const ACTIVE_EVENTS = [
  'UserPromptSubmit',
  'PreToolUse',
  'PostToolUse',
  'PermissionRequest',
  'Stop',
  'SessionStart',
  'PreCompact',
  'PostCompact',
];

const FIELDS: [string, string, boolean][] = [
  ['type', '"command" | "prompt"', true],
  ['command', 'Shell command to run (for type "command")', false],
  ['prompt', 'Prompt text to inject (for type "prompt")', false],
  ['matcher', 'Glob matched against tool name, e.g. "Bash", "Bash,Write", "*" (optional)', false],
  ['if', 'Regex matched against tool_input content (optional)', false],
  ['timeout', 'Max runtime in seconds (default 10)', false],
  ['blocking', 'Block agent if hook fails (default true)', false],
  ['async', 'Fire-and-forget, no waiting (default false)', false],
];

const MINI_EXAMPLE = `{
  "hooks": {
    "PostToolUse": [{
      "matcher": "Bash",
      "hooks": [{
        "type": "command",
        "command": "echo done"
      }]
    }]
  }
}`;

export function HooksPanel() {
  const { token } = theme.useToken();
  const [text, setText] = useState('');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const [refOpen, setRefOpen] = useState(false);
  const successTimer = useRef<ReturnType<typeof setTimeout>>();

  useEffect(() => {
    fetch('/api/hooks')
      .then(r => r.json())
      .then(data => { setText(JSON.stringify(data, null, 2)); setLoading(false); })
      .catch(() => { setText('{\n  "hooks": {}\n}'); setLoading(false); });
  }, []);

  async function handleSave() {
    setError(null);
    setSuccess(false);
    const validationError = validateHooksJson(text);
    if (validationError) { setError(validationError); return; }
    setSaving(true);
    try {
      const res = await fetch('/api/hooks', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: text,
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

  return (
    <Flex vertical style={{ height: '100%', background: token.colorBgLayout }}>
      {/* Reference panel */}
      <div
        style={{
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          backgroundColor: token.colorBgContainer,
          flexShrink: 0
        }}
      >
        <Button
          type="text"
          onClick={() => setRefOpen(o => !o)}
          style={{
            width: '100%',
            height: 'auto',
            display: 'flex',
            alignItems: 'center',
            gap: '8px',
            padding: '12px 20px',
            textAlign: 'left',
            borderRadius: 0
          }}
        >
          <RightOutlined
            style={{
              fontSize: '10px',
              color: token.colorTextDescription,
              transition: 'transform 0.2s',
              transform: refOpen ? 'rotate(90deg)' : 'rotate(0deg)'
            }}
          />
          <Text
            strong
            style={{
              fontSize: '11px',
              color: token.colorTextDescription,
              textTransform: 'uppercase',
              letterSpacing: '0.05em'
            }}
          >
            Reference Guide
          </Text>
        </Button>

        {refOpen && (
          <div style={{ padding: '0 20px 20px 20px' }}>
            <Flex gap={24} wrap="wrap">
              {/* Events */}
              <div style={{ flex: '1 1 200px' }}>
                <Text strong style={{ fontSize: '12px', display: 'block', marginBottom: '8px' }}>Supported Events</Text>
                <Space wrap size={[4, 8]}>
                  {ACTIVE_EVENTS.map(e => (
                    <Tag key={e} color="processing" style={{ margin: 0, fontFamily: token.fontFamilyCode, fontSize: '10px' }}>{e}</Tag>
                  ))}
                </Space>
              </div>

              {/* Example */}
              <div style={{ flex: '1 1 200px' }}>
                <Text strong style={{ fontSize: '12px', display: 'block', marginBottom: '8px' }}>Minimal Example</Text>
                <pre
                  style={{
                    backgroundColor: token.colorFillAlter,
                    border: `1px solid ${token.colorBorderSecondary}`,
                    borderRadius: token.borderRadius,
                    padding: '10px',
                    fontSize: '10px',
                    lineHeight: '1.5',
                    margin: 0,
                    overflowX: 'auto',
                    color: token.colorTextSecondary,
                    fontFamily: token.fontFamilyCode
                  }}
                >
                  {MINI_EXAMPLE}
                </pre>
              </div>

              {/* Fields */}
              <div style={{ flex: '1 1 300px' }}>
                <Text strong style={{ fontSize: '12px', display: 'block', marginBottom: '8px' }}>Hook Fields</Text>
                <div style={{ maxHeight: '150px', overflowY: 'auto' }}>
                  <table style={{ width: '100%', borderCollapse: 'collapse' }}>
                    <tbody>
                      {FIELDS.map(([field, desc, required]) => (
                        <tr key={field} style={{ verticalAlign: 'top' }}>
                          <td style={{ padding: '2px 12px 2px 0' }}>
                            <Text code style={{ color: token.colorPrimary, fontSize: '10px' }}>
                              {field}{required && <span style={{ color: token.colorError, marginLeft: '2px' }}>*</span>}
                            </Text>
                          </td>
                          <td style={{ padding: '2px 0' }}>
                            <Text type="secondary" style={{ fontSize: '11px' }}>{desc}</Text>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>

              {/* Variables */}
              <div style={{ flex: '1 1 200px' }}>
                <Text strong style={{ fontSize: '12px', display: 'block', marginBottom: '8px' }}>Variables</Text>
                <Flex vertical gap={4}>
                  <Space>
                    <Text code style={{ fontSize: '10px' }}>{'${SEMACLAW_ROOT}'}</Text>
                    <Text type="secondary" style={{ fontSize: '11px' }}>Global config</Text>
                  </Space>
                  <Space>
                    <Text code style={{ fontSize: '10px' }}>{'${AGENT_WORKSPACE}'}</Text>
                    <Text type="secondary" style={{ fontSize: '11px' }}>Agent dir</Text>
                  </Space>
                  <Text type="secondary" style={{ fontSize: '10px', fontStyle: 'italic', marginTop: 8 }}>
                    Structure: <Text code style={{ fontSize: '9px' }}>hooks[event] → EventConfig[] → hooks[]</Text>
                  </Text>
                </Flex>
              </div>
            </Flex>
          </div>
        )}
      </div>

      {/* JSON editor */}
      <div style={{ flex: 1, padding: '16px', overflow: 'hidden' }}>
        <TextArea
          value={loading ? 'Loading…' : text}
          onChange={e => { setText(e.target.value); setError(null); setSuccess(false); }}
          disabled={loading || saving}
          spellCheck={false}
          style={{
            height: '100%',
            fontFamily: token.fontFamilyCode,
            fontSize: '12px',
            backgroundColor: token.colorBgContainer,
            color: token.colorText,
            borderRadius: token.borderRadiusLG,
            padding: '16px',
            resize: 'none',
            border: `1px solid ${token.colorBorderSecondary}`
          }}
          placeholder={'{\n  "hooks": {}\n}'}
        />
      </div>

      {/* Footer */}
      <Flex
        align="center"
        justify="space-between"
        gap={16}
        style={{
          borderTop: `1px solid ${token.colorBorderSecondary}`,
          backgroundColor: token.colorBgContainer,
          padding: '12px 20px',
          flexShrink: 0
        }}
      >
        <div style={{ flex: 1, minWidth: 0 }}>
          {error && (
            <Space size={4} style={{ color: token.colorError }}>
              <ExclamationCircleFilled style={{ fontSize: '14px' }} />
              <Text type="danger" style={{ fontSize: '12px' }}>{error}</Text>
            </Space>
          )}
          {success && (
            <Space size={4} style={{ color: token.colorSuccess }}>
              <CheckCircleFilled style={{ fontSize: '14px' }} />
              <Text type="success" style={{ fontSize: '12px' }}>Saved successfully</Text>
            </Space>
          )}
        </div>
        <Button
          type="primary"
          icon={<SaveOutlined />}
          onClick={handleSave}
          loading={saving}
          disabled={loading}
          size="middle"
          style={{ borderRadius: token.borderRadius, boxShadow: '0 2px 8px rgba(0,0,0,0.1)' }}
        >
          {saving ? 'Saving...' : 'Save Configuration'}
        </Button>
      </Flex>
    </Flex>
  );
}
