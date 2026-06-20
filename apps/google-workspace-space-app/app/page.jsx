'use client';

import { useEffect, useMemo, useState } from 'react';
import {
  Alert,
  Avatar,
  Badge,
  Button,
  Card,
  Col,
  Collapse,
  ConfigProvider,
  Empty,
  Form,
  Input,
  InputNumber,
  message,
  Modal,
  Row,
  Segmented,
  Skeleton,
  Space,
  Statistic,
  Steps,
  Switch,
  Tag,
  Timeline,
  Tooltip,
  Typography,
} from 'antd';
import {
  ApiOutlined,
  CalendarOutlined,
  CheckCircleFilled,
  CloseCircleOutlined,
  CloudSyncOutlined,
  CopyOutlined,
  DatabaseOutlined,
  DisconnectOutlined,
  FileTextOutlined,
  GoogleOutlined,
  InfoCircleOutlined,
  KeyOutlined,
  LinkOutlined,
  LockOutlined,
  MailOutlined,
  ReloadOutlined,
  SaveOutlined,
  SettingOutlined,
  SyncOutlined,
  ThunderboltFilled,
} from '@ant-design/icons';
import { SenclawSpace } from '@senclaw/space-sdk';

const { Title, Text, Paragraph } = Typography;

const SERVICES = [
  {
    id: 'gmail',
    label: 'Gmail',
    detail: 'Inbox sync · full-text searchable cache',
    icon: <MailOutlined />,
    color: '#ea4335',
    bg: 'rgba(234, 67, 53, 0.08)',
  },
  {
    id: 'calendar',
    label: 'Calendar',
    detail: 'Imported into Space Calendar',
    icon: <CalendarOutlined />,
    color: '#1a73e8',
    bg: 'rgba(26, 115, 232, 0.08)',
  },
  {
    id: 'notes',
    label: 'Drive & Notes',
    detail: 'Keep / Drive documents pipeline',
    icon: <FileTextOutlined />,
    color: '#fbbc04',
    bg: 'rgba(251, 188, 4, 0.12)',
  },
];

const SYNC_WINDOWS = [
  { label: '24h', value: 1 },
  { label: '7d', value: 7 },
  { label: '30d', value: 30 },
  { label: '90d', value: 90 },
];

const OAUTH_PLAYGROUND_URL = 'https://developers.google.com/oauthplayground/';
const GOOGLE_CONSOLE_URL = 'https://console.cloud.google.com/apis/credentials';

const defaultSettings = {
  days: 7,
  services: ['gmail', 'calendar', 'notes'],
  mcpPort: 4310,
  mcpName: 'google-workspace-mcp',
  clientId: '',
  clientSecret: '',
  tokens: null,
};

function normalizeSettings(value) {
  if (!value || typeof value !== 'object') return { ...defaultSettings };
  return {
    ...defaultSettings,
    ...value,
    days: Number(value.days || defaultSettings.days),
    mcpPort: Number(value.mcpPort || defaultSettings.mcpPort),
    services:
      Array.isArray(value.services) && value.services.length
        ? value.services
        : defaultSettings.services,
    clientId: value.clientId || '',
    clientSecret: value.clientSecret || '',
    tokens: value.tokens || null,
  };
}

function relativeTime(value) {
  if (!value) return '—';
  const diff = Date.now() - new Date(value).getTime();
  if (diff < 0) return 'just now';
  const minutes = Math.floor(diff / 60000);
  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}d ago`;
  return new Intl.DateTimeFormat(undefined, {
    month: 'short',
    day: 'numeric',
  }).format(new Date(value));
}

function friendlyError(err) {
  const text = err instanceof Error ? err.message : String(err);
  if (/DOCTYPE|<html|Unexpected token '</.test(text)) {
    return 'Space runtime is not responding — make sure the SenClaw daemon is running.';
  }
  if (/Failed to fetch|NetworkError/.test(text)) {
    return 'Network error — could not reach the Space proxy.';
  }
  return text;
}

async function copyToClipboard(text) {
  try {
    if (navigator?.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
    } else {
      const ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      document.execCommand('copy');
      document.body.removeChild(ta);
    }
    message.success('Copied to clipboard');
  } catch {
    message.error('Could not copy — copy the URL manually.');
  }
}

function openExternal(url) {
  // Iframe-hosted apps may have COOP/COEP blocking _blank navigation
  // to console.cloud.google.com. Try window.top first, fall back to copy.
  try {
    const win = window.open(url, '_blank', 'noopener,noreferrer');
    if (!win) throw new Error('blocked');
  } catch {
    copyToClipboard(url);
    message.info('Browser blocked the popup — URL copied instead.');
  }
}

export default function Page() {
  const [space, setSpace] = useState(null);
  const [settings, setSettings] = useState(defaultSettings);
  const [draftSettings, setDraftSettings] = useState(defaultSettings);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [connectOpen, setConnectOpen] = useState(false);
  const [tokenDraft, setTokenDraft] = useState('');
  const [advancedToken, setAdvancedToken] = useState('');
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [ready, setReady] = useState(false);
  const [banner, setBanner] = useState(null);
  const [result, setResult] = useState('');
  const [runs, setRuns] = useState([]);
  const [mcpStatus, setMcpStatus] = useState(null);

  const isConnected = !!settings.tokens?.access_token;
  const hasCredentials = !!(settings.clientId && settings.clientSecret);
  const enabledCount = settings.services.length;
  const lastRun = runs[0];

  const onboardingStep = useMemo(() => {
    if (!isConnected) return 0;
    if (!lastRun) return 1;
    return 2;
  }, [isConnected, lastRun]);

  useEffect(() => {
    let cancelled = false;
    SenclawSpace.init()
      .then(async (client) => {
        if (cancelled) return;
        setSpace(client);
        await ensureSchema(client);
        const saved = normalizeSettings(await client.getConfig('google-workspace-settings'));
        if (cancelled) return;
        setSettings(saved);
        setDraftSettings(saved);
        await loadRuns(client);
        await loadMcpStatus();
        setReady(true);
      })
      .catch((error) => {
        if (cancelled) return;
        setBanner({ type: 'error', text: friendlyError(error) });
        setReady(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const getClient = () => space ?? new SenclawSpace({ appId: 'google-workspace' });

  const ensureSchema = async (client) => {
    await client.sqlite(
      'CREATE TABLE IF NOT EXISTS sync_runs (id INTEGER PRIMARY KEY AUTOINCREMENT, service TEXT NOT NULL, status TEXT NOT NULL, created_at INTEGER NOT NULL)'
    );
  };

  const loadRuns = async (client) => {
    try {
      await ensureSchema(client);
      const data = await client.sqlite(
        'SELECT id, service, status, created_at FROM sync_runs ORDER BY id DESC LIMIT 10'
      );
      setRuns(data.rows ?? []);
    } catch (error) {
      setBanner({ type: 'warning', text: friendlyError(error) });
    }
  };

  const loadMcpStatus = async () => {
    try {
      const res = await fetch('/api/space/apps/google-workspace/mcp');
      if (!res.ok) return;
      const data = await res.json();
      setMcpStatus({
        status: data?.server?.status ?? 'unknown',
        tools: data?.server?.tools?.length ?? 0,
        autoRegister: !!data?.declared?.autoRegister,
        error: data?.server?.error ?? null,
      });
    } catch {
      // informational — silently ignore
    }
  };

  const persistSettings = async (next) => {
    const normalized = normalizeSettings(next);
    const client = getClient();
    await client.setConfig('google-workspace-settings', normalized);
    setSettings(normalized);
    return normalized;
  };

  const toggleService = async (id, enabled) => {
    const services = enabled
      ? Array.from(new Set([...settings.services, id]))
      : settings.services.filter((s) => s !== id);
    try {
      await persistSettings({ ...settings, services });
    } catch (error) {
      message.error(friendlyError(error));
    }
  };

  const changeWindow = async (value) => {
    try {
      await persistSettings({ ...settings, days: Number(value) });
    } catch (error) {
      message.error(friendlyError(error));
    }
  };

  const saveSettings = async () => {
    if (!draftSettings.services.length) {
      message.warning('Select at least one Google Workspace service.');
      return;
    }
    try {
      await persistSettings(draftSettings);
      setSettingsOpen(false);
      setBanner({ type: 'success', text: 'Settings saved.' });
      message.success('Settings saved');
    } catch (error) {
      message.error(friendlyError(error));
    }
  };

  const connectWithToken = async () => {
    const token = tokenDraft.trim();
    if (!token) {
      message.warning('Paste an access token first.');
      return;
    }
    try {
      await persistSettings({
        ...settings,
        tokens: { ...(settings.tokens || {}), access_token: token },
      });
      setConnectOpen(false);
      setTokenDraft('');
      setBanner({ type: 'success', text: 'Connected — access token saved.' });
      message.success('Connected');
    } catch (error) {
      message.error(friendlyError(error));
    }
  };

  const connectViaOauth = () => {
    if (!hasCredentials) {
      message.info('Add Client ID & Secret in Advanced first, or paste an access token.');
      return;
    }
    window.location.href = '/api/auth';
  };

  const disconnect = () => {
    Modal.confirm({
      title: 'Disconnect Google account?',
      content: 'Tokens stored locally will be removed. You can reconnect at any time.',
      okText: 'Disconnect',
      okButtonProps: { danger: true },
      onOk: async () => {
        try {
          await persistSettings({ ...settings, tokens: null });
          setBanner({ type: 'info', text: 'Disconnected from Google.' });
        } catch (error) {
          message.error(friendlyError(error));
        }
      },
    });
  };

  const sync = async ({ manualToken } = {}) => {
    if (!isConnected && !manualToken) {
      message.warning('Connect to Google or provide an access token first.');
      return;
    }
    const client = getClient();
    setSyncing(true);
    setBanner({ type: 'info', text: 'Sync in progress…' });
    setResult('');
    try {
      await ensureSchema(client);
      await client.sqlite(
        'INSERT INTO sync_runs (service, status, created_at) VALUES (?1, ?2, ?3)',
        ['google-workspace', 'started', Date.now()]
      );
      const body = {
        days: Number(settings.days || 7),
        services: settings.services,
      };
      const token = manualToken || settings.tokens?.access_token;
      if (token) body.token = token;
      const payload = await client.core('space/sync/google-workspace', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      await client.sqlite(
        'INSERT INTO sync_runs (service, status, created_at) VALUES (?1, ?2, ?3)',
        ['google-workspace', payload.status ?? 'completed', Date.now()]
      );
      await loadRuns(client);
      setResult(JSON.stringify(payload, null, 2));
      setBanner({ type: 'success', text: 'Sync completed.' });
      message.success('Sync completed');
    } catch (error) {
      await client
        .sqlite('INSERT INTO sync_runs (service, status, created_at) VALUES (?1, ?2, ?3)', [
          'google-workspace',
          'error',
          Date.now(),
        ])
        .catch(() => {});
      await loadRuns(client).catch(() => {});
      const text = friendlyError(error);
      setBanner({ type: 'error', text });
      message.error(text);
    } finally {
      setSyncing(false);
    }
  };

  const accountLabel =
    settings.tokens?.email ||
    settings.tokens?.id_token_email ||
    (settings.tokens?.access_token ? 'Google account connected' : 'Not connected');

  return (
    <ConfigProvider
      theme={{
        token: {
          borderRadius: 10,
          colorPrimary: '#1a73e8',
          colorInfo: '#1a73e8',
          fontFamily:
            'Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
        },
        components: {
          Card: { paddingLG: 20 },
        },
      }}
    >
      <main
        style={{
          minHeight: '100vh',
          background:
            'radial-gradient(1200px 600px at 0% -10%, rgba(26,115,232,0.10), transparent 60%), radial-gradient(900px 500px at 100% 0%, rgba(234,67,53,0.08), transparent 60%), #f5f7fb',
          padding: '24px 28px 48px',
        }}
      >
        <div style={{ maxWidth: 1180, margin: '0 auto' }}>
          {/* Header */}
          <Row align="middle" justify="space-between" gutter={[16, 12]} style={{ marginBottom: 18 }}>
            <Col>
              <Space align="center" size={14}>
                <div
                  style={{
                    width: 44,
                    height: 44,
                    borderRadius: 12,
                    background: '#fff',
                    border: '1px solid rgba(15, 23, 42, 0.08)',
                    boxShadow: '0 6px 20px rgba(15, 23, 42, 0.06)',
                    display: 'grid',
                    placeItems: 'center',
                    fontSize: 22,
                    color: '#1a73e8',
                  }}
                >
                  <GoogleOutlined />
                </div>
                <div>
                  <Title level={3} style={{ margin: 0, lineHeight: 1.15 }}>
                    Google Workspace
                  </Title>
                  <Text type="secondary">
                    Sync Gmail, Calendar, and Drive into your Space — powered by SenclawSpace SDK.
                  </Text>
                </div>
              </Space>
            </Col>
            <Col>
              <Space wrap>
                <Tooltip
                  title={
                    mcpStatus?.status === 'connected'
                      ? `MCP connected · ${mcpStatus.tools} tools`
                      : 'MCP server status'
                  }
                >
                  <Tag
                    icon={<ApiOutlined />}
                    color={
                      mcpStatus?.status === 'connected'
                        ? 'success'
                        : mcpStatus?.status === 'error'
                          ? 'error'
                          : 'default'
                    }
                    style={{ padding: '4px 10px', borderRadius: 999 }}
                  >
                    MCP {mcpStatus?.status ?? 'idle'}
                  </Tag>
                </Tooltip>
                <Button
                  icon={<ReloadOutlined />}
                  onClick={async () => {
                    if (space) await loadRuns(space);
                    await loadMcpStatus();
                  }}
                >
                  Refresh
                </Button>
                <Button
                  icon={<SettingOutlined />}
                  onClick={() => {
                    setDraftSettings(settings);
                    setSettingsOpen(true);
                  }}
                >
                  Settings
                </Button>
              </Space>
            </Col>
          </Row>

          {/* Connection hero */}
          <Card
            style={{
              border: '1px solid rgba(15, 23, 42, 0.06)',
              boxShadow: '0 10px 30px rgba(15, 23, 42, 0.05)',
              marginBottom: 16,
              overflow: 'hidden',
            }}
            bodyStyle={{ padding: 0 }}
          >
            <div
              style={{
                background: isConnected
                  ? 'linear-gradient(135deg, #0f172a 0%, #1a73e8 100%)'
                  : 'linear-gradient(135deg, #1a73e8 0%, #6366f1 60%, #8b5cf6 100%)',
                color: '#fff',
                padding: '22px 24px',
              }}
            >
              <Row align="middle" justify="space-between" gutter={[16, 12]}>
                <Col>
                  <Space size={14} align="center">
                    <Avatar
                      size={48}
                      style={{
                        background: 'rgba(255,255,255,0.16)',
                        border: '1px solid rgba(255,255,255,0.25)',
                        color: '#fff',
                      }}
                      icon={isConnected ? <CheckCircleFilled /> : <GoogleOutlined />}
                    />
                    <div>
                      <Text style={{ color: 'rgba(255,255,255,0.78)', fontSize: 12, letterSpacing: 0.4, textTransform: 'uppercase' }}>
                        {isConnected ? 'Connected' : 'Not connected'}
                      </Text>
                      <div style={{ fontSize: 18, fontWeight: 600, marginTop: 2 }}>
                        {accountLabel}
                      </div>
                      <Text style={{ color: 'rgba(255,255,255,0.72)', fontSize: 12 }}>
                        Last sync: {lastRun ? relativeTime(lastRun.created_at) : 'never'} ·{' '}
                        {enabledCount}/{SERVICES.length} services enabled · window {settings.days}d
                      </Text>
                    </div>
                  </Space>
                </Col>
                <Col>
                  <Space wrap>
                    {isConnected ? (
                      <>
                        <Button
                          icon={<DisconnectOutlined />}
                          onClick={disconnect}
                          style={{
                            background: 'rgba(255,255,255,0.12)',
                            border: '1px solid rgba(255,255,255,0.25)',
                            color: '#fff',
                          }}
                        >
                          Disconnect
                        </Button>
                        <Button
                          type="primary"
                          size="large"
                          icon={syncing ? <SyncOutlined spin /> : <CloudSyncOutlined />}
                          loading={syncing}
                          onClick={() => sync()}
                          style={{
                            background: '#fff',
                            color: '#1a73e8',
                            border: 'none',
                            fontWeight: 600,
                          }}
                        >
                          Sync now
                        </Button>
                      </>
                    ) : (
                      <Button
                        type="primary"
                        size="large"
                        icon={<GoogleOutlined />}
                        onClick={() => {
                          setTokenDraft('');
                          setConnectOpen(true);
                        }}
                        style={{
                          background: '#fff',
                          color: '#1a73e8',
                          border: 'none',
                          fontWeight: 600,
                        }}
                      >
                        Connect to Google
                      </Button>
                    )}
                  </Space>
                </Col>
              </Row>
            </div>

            {onboardingStep < 2 && (
              <div style={{ padding: '18px 24px', borderTop: '1px solid rgba(15,23,42,0.06)', background: '#fff' }}>
                <Steps
                  size="small"
                  current={onboardingStep}
                  items={[
                    {
                      title: 'Connect Google',
                      description: 'Paste an access token',
                      icon: <GoogleOutlined />,
                    },
                    {
                      title: 'Run first sync',
                      description: 'Pull selected services',
                      icon: <CloudSyncOutlined />,
                    },
                  ]}
                />
              </div>
            )}
          </Card>

          {banner && (
            <Alert
              style={{ marginBottom: 16 }}
              type={banner.type}
              showIcon
              closable
              onClose={() => setBanner(null)}
              message={banner.text}
            />
          )}

          {/* KPI strip */}
          <Row gutter={[16, 16]} style={{ marginBottom: 16 }}>
            <Col xs={12} md={6}>
              <Card>
                <Statistic
                  title="Services enabled"
                  value={enabledCount}
                  suffix={`/ ${SERVICES.length}`}
                  prefix={<ThunderboltFilled style={{ color: '#1a73e8' }} />}
                />
              </Card>
            </Col>
            <Col xs={12} md={6}>
              <Card>
                <Statistic
                  title="Sync window"
                  value={settings.days}
                  suffix="days"
                  prefix={<CalendarOutlined style={{ color: '#ea4335' }} />}
                />
              </Card>
            </Col>
            <Col xs={12} md={6}>
              <Card>
                <Statistic
                  title="Sync runs"
                  value={runs.length}
                  prefix={<DatabaseOutlined style={{ color: '#34a853' }} />}
                />
              </Card>
            </Col>
            <Col xs={12} md={6}>
              <Card>
                <Statistic
                  title="MCP tools"
                  value={mcpStatus?.tools ?? 0}
                  prefix={<ApiOutlined style={{ color: '#fbbc04' }} />}
                />
              </Card>
            </Col>
          </Row>

          {/* Main two-column */}
          <Row gutter={[16, 16]}>
            <Col xs={24} lg={16}>
              <Card
                title={
                  <Space>
                    <span>Services</span>
                    <Tag color="blue" style={{ borderRadius: 999 }}>
                      click to toggle
                    </Tag>
                  </Space>
                }
                extra={
                  <Segmented
                    size="small"
                    value={settings.days}
                    onChange={changeWindow}
                    options={SYNC_WINDOWS}
                  />
                }
              >
                <Row gutter={[12, 12]}>
                  {SERVICES.map((service) => {
                    const enabled = settings.services.includes(service.id);
                    return (
                      <Col xs={24} md={8} key={service.id}>
                        <div
                          onClick={() => toggleService(service.id, !enabled)}
                          style={{
                            cursor: 'pointer',
                            border: `1px solid ${enabled ? service.color : 'rgba(15,23,42,0.08)'}`,
                            background: enabled ? service.bg : '#fff',
                            borderRadius: 12,
                            padding: 14,
                            height: '100%',
                            transition: 'all 160ms ease',
                          }}
                        >
                          <Row align="middle" justify="space-between">
                            <Space size={10}>
                              <div
                                style={{
                                  width: 36,
                                  height: 36,
                                  borderRadius: 10,
                                  background: '#fff',
                                  border: `1px solid ${service.color}33`,
                                  display: 'grid',
                                  placeItems: 'center',
                                  color: service.color,
                                  fontSize: 18,
                                }}
                              >
                                {service.icon}
                              </div>
                              <div>
                                <Text strong>{service.label}</Text>
                                <div>
                                  <Text type="secondary" style={{ fontSize: 12 }}>
                                    {service.detail}
                                  </Text>
                                </div>
                              </div>
                            </Space>
                            <Switch
                              checked={enabled}
                              onChange={(checked) => toggleService(service.id, checked)}
                              onClick={(_, e) => e.stopPropagation()}
                            />
                          </Row>
                        </div>
                      </Col>
                    );
                  })}
                </Row>

                {result && (
                  <Collapse
                    style={{ marginTop: 16 }}
                    items={[
                      {
                        key: 'result',
                        label: (
                          <Space>
                            <InfoCircleOutlined />
                            <span>Last sync response</span>
                          </Space>
                        ),
                        children: (
                          <pre
                            style={{
                              margin: 0,
                              padding: 14,
                              borderRadius: 8,
                              background: '#0f172a',
                              color: '#dbeafe',
                              fontSize: 12,
                              lineHeight: 1.5,
                              maxHeight: 320,
                              overflow: 'auto',
                            }}
                          >
                            {result}
                          </pre>
                        ),
                      },
                    ]}
                  />
                )}
              </Card>
            </Col>

            <Col xs={24} lg={8}>
              <Card
                title="Activity"
                extra={
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    last 10
                  </Text>
                }
                style={{ height: '100%' }}
              >
                {!ready ? (
                  <Skeleton active paragraph={{ rows: 4 }} />
                ) : runs.length ? (
                  <Timeline
                    style={{ marginTop: 4 }}
                    items={runs.map((run) => {
                      const isError = run.status === 'error';
                      const isStart = run.status === 'started';
                      const color = isError ? 'red' : isStart ? 'blue' : 'green';
                      const dot = isError ? (
                        <CloseCircleOutlined style={{ color: '#dc2626' }} />
                      ) : isStart ? (
                        <SyncOutlined spin style={{ color: '#1a73e8' }} />
                      ) : (
                        <CheckCircleFilled style={{ color: '#34a853' }} />
                      );
                      return {
                        color,
                        dot,
                        children: (
                          <Space direction="vertical" size={0}>
                            <Space>
                              <Text strong style={{ textTransform: 'capitalize' }}>
                                {run.status}
                              </Text>
                              <Text type="secondary" style={{ fontSize: 12 }}>
                                {run.service}
                              </Text>
                            </Space>
                            <Text type="secondary" style={{ fontSize: 12 }}>
                              {relativeTime(run.created_at)}
                            </Text>
                          </Space>
                        ),
                      };
                    })}
                  />
                ) : (
                  <Empty
                    image={Empty.PRESENTED_IMAGE_SIMPLE}
                    description="No sync runs yet."
                  />
                )}
              </Card>
            </Col>
          </Row>

          {/* Advanced */}
          <Card style={{ marginTop: 16 }} bodyStyle={{ padding: 0 }}>
            <Collapse
              ghost
              activeKey={advancedOpen ? ['advanced'] : []}
              onChange={(keys) => setAdvancedOpen(keys.includes('advanced'))}
              items={[
                {
                  key: 'advanced',
                  label: (
                    <Space>
                      <LockOutlined />
                      <Text strong>Advanced</Text>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        Manual token, MCP details
                      </Text>
                    </Space>
                  ),
                  children: (
                    <Row gutter={[16, 16]} style={{ padding: '4px 16px 16px' }}>
                      <Col xs={24} md={14}>
                        <Card size="small" type="inner" title="One-shot access token">
                          <Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 8 }}>
                            Run a single sync with a different <code>ya29.…</code> token without
                            replacing the saved connection.
                          </Paragraph>
                          <Space.Compact style={{ width: '100%' }}>
                            <Input.Password
                              value={advancedToken}
                              onChange={(e) => setAdvancedToken(e.target.value)}
                              placeholder="ya29..."
                              autoComplete="off"
                            />
                            <Button
                              type="primary"
                              icon={<CloudSyncOutlined />}
                              loading={syncing}
                              disabled={!advancedToken.trim()}
                              onClick={() => sync({ manualToken: advancedToken.trim() })}
                            >
                              Sync with token
                            </Button>
                          </Space.Compact>
                        </Card>
                      </Col>
                      <Col xs={24} md={10}>
                        <Card size="small" type="inner" title="MCP server">
                          <Space direction="vertical" size={6} style={{ width: '100%' }}>
                            <Space wrap>
                              <Text strong>{settings.mcpName}</Text>
                              <Badge
                                status={
                                  mcpStatus?.status === 'connected'
                                    ? 'success'
                                    : mcpStatus?.status === 'error'
                                      ? 'error'
                                      : 'default'
                                }
                                text={mcpStatus?.status ?? 'unknown'}
                              />
                              {mcpStatus?.autoRegister && (
                                <Tag color="geekblue">auto-register</Tag>
                              )}
                            </Space>
                            <Text type="secondary" style={{ fontSize: 12 }}>
                              {mcpStatus?.tools ?? 0} tools registered · port {settings.mcpPort}
                            </Text>
                            {mcpStatus?.error && (
                              <Text type="danger" style={{ fontSize: 12 }}>
                                {mcpStatus.error}
                              </Text>
                            )}
                          </Space>
                        </Card>
                      </Col>
                    </Row>
                  ),
                },
              ]}
            />
          </Card>

          <div style={{ textAlign: 'center', marginTop: 22 }}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              SenClaw · Google Workspace Space App
            </Text>
          </div>
        </div>

        {/* Connect modal — simple paste-token flow */}
        <Modal
          title={
            <Space>
              <GoogleOutlined style={{ color: '#1a73e8' }} />
              <span>Connect to Google</span>
            </Space>
          }
          open={connectOpen}
          onCancel={() => setConnectOpen(false)}
          onOk={connectWithToken}
          okText="Save & connect"
          okButtonProps={{ icon: <CheckCircleFilled />, disabled: !tokenDraft.trim() }}
          width={620}
        >
          <Paragraph type="secondary" style={{ marginTop: 4 }}>
            Paste a Google OAuth access token (<code>ya29.…</code>). The token is stored locally
            via the SenclawSpace config KV and used for every sync until you disconnect.
          </Paragraph>
          <Form layout="vertical">
            <Form.Item label="Access token" required>
              <Input.Password
                value={tokenDraft}
                onChange={(e) => setTokenDraft(e.target.value)}
                placeholder="ya29.a0AbVbY6..."
                autoComplete="off"
                size="large"
              />
            </Form.Item>
          </Form>

          <Collapse
            ghost
            items={[
              {
                key: 'howto',
                label: (
                  <Space>
                    <InfoCircleOutlined />
                    <Text strong>How to get an access token</Text>
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size={10} style={{ width: '100%' }}>
                    <Text>
                      1. Open <strong>OAuth 2.0 Playground</strong>, pick the scopes you need
                      (Gmail, Calendar, Drive), authorize with your Google account, and exchange
                      the auth code for an access token.
                    </Text>
                    <Space wrap>
                      <Button
                        icon={<LinkOutlined />}
                        onClick={() => openExternal(OAUTH_PLAYGROUND_URL)}
                      >
                        Open OAuth Playground
                      </Button>
                      <Button
                        icon={<CopyOutlined />}
                        onClick={() => copyToClipboard(OAUTH_PLAYGROUND_URL)}
                      >
                        Copy URL
                      </Button>
                    </Space>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      Access tokens expire after ~1 hour. For long-lived connections, set up the
                      full OAuth client in Settings → Advanced and use <em>Connect via OAuth
                      redirect</em> instead.
                    </Text>
                  </Space>
                ),
              },
              {
                key: 'oauth',
                label: (
                  <Space>
                    <KeyOutlined />
                    <Text strong>Use my own OAuth client (advanced)</Text>
                  </Space>
                ),
                children: (
                  <Space direction="vertical" size={10} style={{ width: '100%' }}>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      Register a Web application client in Google Cloud Console with redirect URI{' '}
                      <code>http://127.0.0.1:4310/api/auth/callback</code>, then save Client ID
                      &amp; Secret in Settings → Advanced. After saving, click below to start the
                      browser-based OAuth flow.
                    </Text>
                    <Space wrap>
                      <Button
                        icon={<LinkOutlined />}
                        onClick={() => openExternal(GOOGLE_CONSOLE_URL)}
                      >
                        Open Google Cloud Console
                      </Button>
                      <Button
                        icon={<CopyOutlined />}
                        onClick={() => copyToClipboard(GOOGLE_CONSOLE_URL)}
                      >
                        Copy URL
                      </Button>
                      <Button
                        type="primary"
                        ghost
                        disabled={!hasCredentials}
                        onClick={connectViaOauth}
                      >
                        Connect via OAuth redirect
                      </Button>
                    </Space>
                    {!hasCredentials && (
                      <Text type="warning" style={{ fontSize: 12 }}>
                        No Client ID / Secret saved yet — configure them in Settings first.
                      </Text>
                    )}
                  </Space>
                ),
              },
            ]}
          />
        </Modal>

        {/* Settings modal */}
        <Modal
          title={
            <Space>
              <SettingOutlined />
              <span>Google Workspace settings</span>
            </Space>
          }
          open={settingsOpen}
          onCancel={() => setSettingsOpen(false)}
          onOk={saveSettings}
          okText="Save settings"
          okButtonProps={{ icon: <SaveOutlined /> }}
          width={720}
        >
          <Paragraph type="secondary" style={{ marginTop: 4 }}>
            These values are stored in the SenclawSpace config KV scoped to this app.
          </Paragraph>
          <Form layout="vertical">
            <Row gutter={12}>
              <Col xs={24} md={8}>
                <Form.Item label="Sync window">
                  <InputNumber
                    min={1}
                    max={90}
                    value={draftSettings.days}
                    onChange={(value) =>
                      setDraftSettings((cur) => ({ ...cur, days: value ?? 7 }))
                    }
                    addonAfter="days"
                    style={{ width: '100%' }}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={8}>
                <Form.Item label="MCP server name">
                  <Input
                    value={draftSettings.mcpName}
                    onChange={(e) =>
                      setDraftSettings((cur) => ({ ...cur, mcpName: e.target.value }))
                    }
                  />
                </Form.Item>
              </Col>
              <Col xs={24} md={8}>
                <Form.Item label="Local MCP port">
                  <InputNumber
                    min={1024}
                    max={65535}
                    value={draftSettings.mcpPort}
                    onChange={(value) =>
                      setDraftSettings((cur) => ({ ...cur, mcpPort: value ?? 4310 }))
                    }
                    style={{ width: '100%' }}
                  />
                </Form.Item>
              </Col>
            </Row>
            <Form.Item label="Services">
              <Row gutter={[10, 10]}>
                {SERVICES.map((service) => {
                  const checked = draftSettings.services.includes(service.id);
                  return (
                    <Col xs={24} md={8} key={service.id}>
                      <div
                        onClick={() =>
                          setDraftSettings((cur) => ({
                            ...cur,
                            services: checked
                              ? cur.services.filter((s) => s !== service.id)
                              : Array.from(new Set([...cur.services, service.id])),
                          }))
                        }
                        style={{
                          cursor: 'pointer',
                          border: `1px solid ${checked ? service.color : 'rgba(15,23,42,0.12)'}`,
                          background: checked ? service.bg : '#fff',
                          borderRadius: 10,
                          padding: 12,
                        }}
                      >
                        <Space align="start">
                          <Switch checked={checked} size="small" />
                          <div>
                            <Text strong>{service.label}</Text>
                            <div>
                              <Text type="secondary" style={{ fontSize: 12 }}>
                                {service.detail}
                              </Text>
                            </div>
                          </div>
                        </Space>
                      </div>
                    </Col>
                  );
                })}
              </Row>
            </Form.Item>

            <Collapse
              ghost
              items={[
                {
                  key: 'oauth',
                  label: (
                    <Space>
                      <KeyOutlined />
                      <Text strong>OAuth client (advanced)</Text>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        Only needed for browser-based redirect flow
                      </Text>
                    </Space>
                  ),
                  children: (
                    <>
                      <Paragraph type="secondary" style={{ fontSize: 12 }}>
                        Register a Web application client in Google Cloud Console with redirect
                        URI <code>http://127.0.0.1:4310/api/auth/callback</code>.{' '}
                        <Button
                          type="link"
                          size="small"
                          icon={<CopyOutlined />}
                          style={{ padding: 0 }}
                          onClick={() => copyToClipboard(GOOGLE_CONSOLE_URL)}
                        >
                          copy console URL
                        </Button>
                      </Paragraph>
                      <Row gutter={12}>
                        <Col xs={24} md={12}>
                          <Form.Item label="Google Client ID">
                            <Input
                              value={draftSettings.clientId}
                              onChange={(e) =>
                                setDraftSettings((cur) => ({ ...cur, clientId: e.target.value }))
                              }
                              placeholder="xxxxxxxxxx.apps.googleusercontent.com"
                            />
                          </Form.Item>
                        </Col>
                        <Col xs={24} md={12}>
                          <Form.Item label="Google Client Secret">
                            <Input.Password
                              value={draftSettings.clientSecret}
                              onChange={(e) =>
                                setDraftSettings((cur) => ({
                                  ...cur,
                                  clientSecret: e.target.value,
                                }))
                              }
                            />
                          </Form.Item>
                        </Col>
                      </Row>
                    </>
                  ),
                },
              ]}
            />
          </Form>
        </Modal>
      </main>
    </ConfigProvider>
  );
}
