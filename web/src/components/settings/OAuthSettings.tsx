import React, { useCallback, useEffect, useRef, useState } from 'react';
import {
  Typography,
  AutoComplete,
  Button,
  Space,
  Tag,
  Table,
  Modal,
  Input,
  Tooltip,
  Collapse,
  Skeleton,
  Spin,
  theme,
  message,
} from 'antd';
import {
  LinkOutlined,
  ReloadOutlined,
  DisconnectOutlined,
  PlusOutlined,
  WarningOutlined,
  ExportOutlined,
  CopyOutlined,
  CheckCircleFilled,
  CloseCircleFilled,
  ExperimentOutlined,
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

interface OAuthProvider {
  id: string;
  displayName: string;
  riskNotice: string;
  brandColor: string;
  brandMark: string;
  flow: 'auth_code_pkce' | 'device_code';
  adapt: string;
  baseURL: string;
  defaultMaxTokens: number;
  defaultContextLength: number;
  models: Array<{ id: string; name: string }>;
  requiresFixedPort: boolean;
}

/**
 * Square brand badge.
 *
 * A monogram on the vendor's colour rather than a traced logo — redrawing
 * someone's trademark inaccurately looks worse than a clean initial, and
 * remote logo files can't load under the daemon's CSP anyway.
 */
const ProviderLogo: React.FC<{ color: string; mark: string; size?: number }> = ({
  color,
  mark,
  size = 32,
}) => (
  <div
    style={{
      width: size,
      height: size,
      minWidth: size,
      borderRadius: size * 0.28,
      // Tint of the brand colour so the badge sits well in both themes.
      background: `${color}22`,
      border: `1px solid ${color}55`,
      color,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      fontSize: mark.length > 1 ? size * 0.36 : size * 0.46,
      fontWeight: 700,
      letterSpacing: mark.length > 1 ? -0.5 : 0,
      lineHeight: 1,
      userSelect: 'none',
    }}
    aria-hidden
  >
    {mark}
  </div>
);

interface OAuthAccount {
  id: string;
  provider: string;
  label: string;
  email: string | null;
  expiresAt: number | null;
  expiresIn: number | null;
  expired: boolean;
  hasRefreshToken: boolean;
  scope: string | null;
  createdAt: number;
  lastRefreshAt: number | null;
  lastError: string | null;
}

interface CatalogProvider {
  id: string;
  displayName: string;
  baseURL: string;
  adapt: string;
  auth: 'api_key' | 'none';
  signupUrl: string | null;
  note: string;
  brandColor: string;
  brandMark: string;
  urlPlaceholder: string | null;
  defaultMaxTokens: number;
  defaultContextLength: number;
  models: Array<{ id: string; name: string }>;
}

/** Outcome of a live model probe. */
interface ModelProbe {
  ok: boolean;
  latencyMs: number;
  error?: string;
  reply?: string;
}

/** A device-code sign-in waiting on the user. */
interface DeviceChallenge {
  provider: OAuthProvider;
  userCode: string;
  verificationUri: string;
}

/** Human-readable countdown for a token expiry. */
function formatExpiry(account: OAuthAccount): React.ReactNode {
  if (account.expired) return <Tag color="red">Expired</Tag>;
  if (account.expiresIn == null) return <Tag>No expiry reported</Tag>;

  const secs = account.expiresIn;
  const label =
    secs > 86400
      ? `${Math.floor(secs / 86400)}d`
      : secs > 3600
        ? `${Math.floor(secs / 3600)}h`
        : `${Math.max(1, Math.floor(secs / 60))}m`;
  return <Tag color={secs < 600 ? 'orange' : 'green'}>{label} left</Tag>;
}

export const OAuthSettings: React.FC = () => {
  const { token } = theme.useToken();
  const [providers, setProviders] = useState<OAuthProvider[]>([]);
  const [accounts, setAccounts] = useState<OAuthAccount[]>([]);
  const [catalog, setCatalog] = useState<CatalogProvider[]>([]);
  const [loading, setLoading] = useState(true);
  const [connecting, setConnecting] = useState<string | null>(null);
  const [device, setDevice] = useState<DeviceChallenge | null>(null);

  // Bind-to-model modal
  const [bindAccount, setBindAccount] = useState<OAuthAccount | null>(null);
  const [bindModel, setBindModel] = useState('');
  const [binding, setBinding] = useState(false);
  /** Probe results per model id, for the account currently being bound. */
  const [probes, setProbes] = useState<Record<string, ModelProbe>>({});
  const [probing, setProbing] = useState(false);
  /** Models the account itself reports, when the provider publishes a list. */
  const [accountModels, setAccountModels] = useState<Array<{ id: string; name: string }>>([]);
  const [modelSource, setModelSource] = useState<'discovered' | 'registry' | null>(null);
  /** Why discovery fell back, when it did. */
  const [modelReason, setModelReason] = useState<string | null>(null);
  const [loadingModels, setLoadingModels] = useState(false);

  // Free-tier preset modal
  const [preset, setPreset] = useState<CatalogProvider | null>(null);
  const [presetKey, setPresetKey] = useState('');
  const [presetModel, setPresetModel] = useState('');
  const [presetUrlValue, setPresetUrlValue] = useState('');
  const [addingPreset, setAddingPreset] = useState(false);

  const pollRef = useRef<number | null>(null);

  const loadAccounts = useCallback(async () => {
    try {
      const r = await fetch('/api/oauth/accounts');
      if (!r.ok) return;
      const data = await r.json();
      setAccounts(data.accounts ?? []);
    } catch {
      /* daemon restarting — the next poll picks it up */
    }
  }, []);

  useEffect(() => {
    (async () => {
      try {
        const [p, c] = await Promise.all([
          fetch('/api/oauth/providers').then((r) => r.json()),
          fetch('/api/provider-catalog').then((r) => r.json()),
        ]);
        setProviders(p.providers ?? []);
        setCatalog(c.providers ?? []);
        await loadAccounts();
      } catch (e) {
        message.error(`Could not load providers: ${e}`);
      } finally {
        setLoading(false);
      }
    })();
    return () => {
      if (pollRef.current) window.clearInterval(pollRef.current);
    };
  }, [loadAccounts]);

  /** Kick off a sign-in and poll until it resolves. */
  const connect = async (provider: OAuthProvider) => {
    const providerId = provider.id;
    setConnecting(providerId);
    try {
      const r = await fetch(`/api/oauth/${providerId}/start`, { method: 'POST' });
      const data = await r.json();
      if (!r.ok) throw new Error(data.error ?? 'could not start sign-in');

      window.open(data.authorizeUrl, '_blank', 'noopener,noreferrer');

      if (data.kind === 'device_code') {
        // The user has to type this code on the vendor's page, so it stays on
        // screen until the poll resolves.
        setDevice({
          provider,
          userCode: data.userCode ?? '',
          verificationUri: data.authorizeUrl,
        });
      } else {
        message.info('Finish the sign-in in the browser tab that just opened.');
      }

      if (pollRef.current) window.clearInterval(pollRef.current);
      pollRef.current = window.setInterval(async () => {
        try {
          const sr = await fetch(`/api/oauth/flows/${data.flowId}`);
          if (!sr.ok) return;
          const state = await sr.json();
          if (state.status === 'completed') {
            window.clearInterval(pollRef.current!);
            pollRef.current = null;
            setConnecting(null);
            setDevice(null);
            message.success(`Connected ${state.label}`);
            await loadAccounts();
          } else if (state.status === 'failed') {
            window.clearInterval(pollRef.current!);
            pollRef.current = null;
            setConnecting(null);
            setDevice(null);
            message.error(state.error ?? 'sign-in failed');
          }
        } catch {
          /* keep polling */
        }
      }, 2000);
    } catch (e) {
      setConnecting(null);
      setDevice(null);
      message.error(String(e));
    }
  };

  const refreshAccount = async (id: string) => {
    try {
      const r = await fetch(`/api/oauth/accounts/${id}/refresh`, { method: 'POST' });
      const data = await r.json();
      if (!r.ok) throw new Error(data.error ?? 'refresh failed');
      message.success('Token refreshed');
      await loadAccounts();
    } catch (e) {
      message.error(String(e));
    }
  };

  const disconnect = (account: OAuthAccount) => {
    Modal.confirm({
      title: `Disconnect ${account.label}?`,
      content:
        'SenClaw forgets the stored tokens. Any model configuration bound to this account stops working until you connect again.',
      okText: 'Disconnect',
      okButtonProps: { danger: true },
      onOk: async () => {
        const r = await fetch(`/api/oauth/accounts/${account.id}`, { method: 'DELETE' });
        if (r.ok || r.status === 204) {
          message.success('Disconnected');
          await loadAccounts();
        } else {
          message.error('Could not disconnect');
        }
      },
    });
  };

  /** Ask the provider which models this account may actually use. */
  const loadAccountModels = async (accountId: string) => {
    setLoadingModels(true);
    setAccountModels([]);
    setModelSource(null);
    setModelReason(null);
    try {
      const r = await fetch(`/api/oauth/accounts/${accountId}/models`);
      const data = await r.json();
      if (!r.ok) throw new Error(data.error ?? 'could not list models');
      setAccountModels(data.models ?? []);
      setModelSource(data.source ?? 'registry');
      setModelReason(data.reason ?? null);
    } catch {
      // Fall back to the registry list the provider card already carries.
      setModelSource('registry');
    } finally {
      setLoadingModels(false);
    }
  };

  /** Run one model through the real adapter and record the verdict. */
  const testModel = async (accountId: string, model: string): Promise<ModelProbe> => {
    try {
      const r = await fetch('/api/oauth/test-model', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ accountId, modelName: model }),
      });
      const data = await r.json();
      if (!r.ok) throw new Error(data.error ?? 'probe failed');
      const probe: ModelProbe = {
        ok: !!data.ok,
        latencyMs: data.latencyMs ?? 0,
        error: data.error,
        reply: data.reply,
      };
      setProbes((prev) => ({ ...prev, [model]: probe }));
      return probe;
    } catch (e) {
      const probe: ModelProbe = { ok: false, latencyMs: 0, error: String(e) };
      setProbes((prev) => ({ ...prev, [model]: probe }));
      return probe;
    }
  };

  /** Probe every suggested model in turn.
   *
   * Sequential on purpose: these are real completions against a subscription,
   * and firing a dozen at once is the fastest way to trip a rate limit. */
  const testAllModels = async () => {
    if (!bindAccount || !bindProvider) return;
    setProbing(true);
    try {
      for (const m of bindModelOptions) {
        await testModel(bindAccount.id, m.id);
      }
    } finally {
      setProbing(false);
    }
  };

  const submitBind = async () => {
    if (!bindAccount || !bindModel.trim()) return;
    setBinding(true);
    try {
      const r = await fetch('/api/oauth/bind', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ accountId: bindAccount.id, modelName: bindModel.trim() }),
      });
      const data = await r.json();
      if (!r.ok) throw new Error(data.error ?? 'could not create the model config');
      message.success(`Added "${data.label}" to your models`);
      setBindAccount(null);
      setBindModel('');
    } catch (e) {
      message.error(String(e));
    } finally {
      setBinding(false);
    }
  };

  const submitPreset = async () => {
    if (!preset || !presetModel.trim()) return;
    if (preset.urlPlaceholder && !presetUrlValue.trim()) {
      message.error(`${preset.urlPlaceholder} is required`);
      return;
    }
    setAddingPreset(true);
    try {
      const baseURL = preset.urlPlaceholder
        ? preset.baseURL.replace(`{${preset.urlPlaceholder}}`, presetUrlValue.trim())
        : preset.baseURL;

      const r = await fetch('/api/llm-config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          label: `${preset.displayName} — ${presetModel.trim()}`,
          provider: preset.id,
          baseURL,
          apiKey: presetKey.trim(),
          modelName: presetModel.trim(),
          adapt: preset.adapt,
          maxTokens: preset.defaultMaxTokens,
          contextLength: preset.defaultContextLength,
        }),
      });
      if (!r.ok) {
        const data = await r.json().catch(() => ({}));
        throw new Error(data.error ?? 'could not save the model config');
      }
      message.success(`Added ${preset.displayName}`);
      setPreset(null);
      setPresetKey('');
      setPresetModel('');
      setPresetUrlValue('');
    } catch (e) {
      message.error(String(e));
    } finally {
      setAddingPreset(false);
    }
  };

  const accountColumns = [
    {
      title: 'Account',
      dataIndex: 'label',
      key: 'label',
      render: (label: string, row: OAuthAccount) => {
        const p = providers.find((x) => x.id === row.provider);
        return (
          <Space align="start">
            <ProviderLogo
              color={p?.brandColor ?? '#888888'}
              mark={p?.brandMark ?? '?'}
              size={26}
            />
            <Space direction="vertical" size={0}>
              <Text strong>{label}</Text>
              {row.email && (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {row.email}
                </Text>
              )}
            </Space>
          </Space>
        );
      },
    },
    {
      title: 'Token',
      key: 'expiry',
      render: (_: unknown, row: OAuthAccount) => (
        <Space direction="vertical" size={0}>
          {formatExpiry(row)}
          {!row.hasRefreshToken && (
            <Tooltip title="This provider issued no refresh token, so the account must be reconnected by hand when it expires.">
              <Tag color="orange">No auto-refresh</Tag>
            </Tooltip>
          )}
        </Space>
      ),
    },
    {
      title: 'Status',
      key: 'status',
      render: (_: unknown, row: OAuthAccount) =>
        row.lastError ? (
          <Tooltip title={row.lastError}>
            <Tag color="red">Needs attention</Tag>
          </Tooltip>
        ) : (
          <Tag color="green">OK</Tag>
        ),
    },
    {
      title: '',
      key: 'actions',
      align: 'right' as const,
      render: (_: unknown, row: OAuthAccount) => (
        <Space>
          <Button
            size="small"
            icon={<PlusOutlined />}
            onClick={() => {
              setBindAccount(row);
              setProbes({});
              const p = providers.find((x) => x.id === row.provider);
              setBindModel(p?.models[0]?.id ?? '');
              void loadAccountModels(row.id);
            }}
          >
            Use as model
          </Button>
          <Tooltip title="Refresh the access token now">
            <Button
              size="small"
              icon={<ReloadOutlined />}
              disabled={!row.hasRefreshToken}
              onClick={() => refreshAccount(row.id)}
            />
          </Tooltip>
          <Tooltip title="Disconnect">
            <Button
              size="small"
              danger
              icon={<DisconnectOutlined />}
              onClick={() => disconnect(row)}
            />
          </Tooltip>
        </Space>
      ),
    },
  ];

  const bindProvider = bindAccount
    ? providers.find((p) => p.id === bindAccount.provider)
    : null;

  /** What the picker offers: the account's own list when the provider
   *  publishes one, the curated registry list otherwise. */
  const bindModelOptions =
    accountModels.length > 0 ? accountModels : (bindProvider?.models ?? []);

  const accountsFor = (providerId: string) =>
    accounts.filter((a) => a.provider === providerId);

  const needsAttention = accounts.filter((a) => a.lastError || a.expired).length;

  return (
    <div>
      <Space
        align="start"
        style={{ width: '100%', justifyContent: 'space-between', marginBottom: 4 }}
      >
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>
            Provider sign-in
          </Title>
          <Text type="secondary">
            Connect a subscription account, or add a free-tier endpoint with an API key.
          </Text>
        </div>
        {accounts.length > 0 && (
          <Space size={6}>
            <Tag color="green" style={{ marginInlineEnd: 0 }}>
              {accounts.length} connected
            </Tag>
            {needsAttention > 0 && (
              <Tag color="red" style={{ marginInlineEnd: 0 }}>
                {needsAttention} need attention
              </Tag>
            )}
          </Space>
        )}
      </Space>

      <Collapse
        ghost
        style={{
          marginTop: 12,
          marginBottom: 20,
          background: token.colorWarningBg,
          border: `1px solid ${token.colorWarningBorder}`,
          borderRadius: token.borderRadiusLG,
        }}
        items={[
          {
            key: 'risk',
            label: (
              <Space size={8}>
                <WarningOutlined style={{ color: token.colorWarning }} />
                <Text style={{ color: token.colorWarning }}>
                  Subscription sign-in is against the vendors&apos; terms of service
                </Text>
              </Space>
            ),
            children: (
              <Paragraph type="secondary" style={{ marginBottom: 0 }}>
                Subscription credentials are licensed for each vendor&apos;s own clients. Using
                them from SenClaw can get the account suspended, and the vendors detect it.
                SenClaw identifies itself honestly rather than imitating the vendor client, so a
                provider that blocks third-party access returns a clear error instead of failing
                silently. For anything you depend on, use an API key.
              </Paragraph>
            ),
          },
        ]}
      />

      {/* ---- Subscription providers ---- */}
      <Title level={5} style={{ marginBottom: 12 }}>
        Subscription accounts
      </Title>

      {loading ? (
        <Skeleton active paragraph={{ rows: 4 }} />
      ) : (
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))',
            gap: 12,
            marginBottom: 28,
          }}
        >
          {providers.map((p) => {
            const mine = accountsFor(p.id);
            const busy = connecting === p.id;
            return (
              <div
                key={p.id}
                style={{
                  border: `1px solid ${mine.length ? `${p.brandColor}66` : token.colorBorderSecondary}`,
                  borderRadius: token.borderRadiusLG,
                  background: token.colorBgContainer,
                  padding: 14,
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 10,
                  transition: 'border-color 0.2s',
                }}
              >
                <Space align="start" style={{ width: '100%' }}>
                  <ProviderLogo color={p.brandColor} mark={p.brandMark} size={38} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <Space size={6} align="center">
                      <Text strong style={{ fontSize: 14 }}>
                        {p.displayName}
                      </Text>
                      {mine.length > 0 && (
                        <CheckCircleFilled style={{ color: token.colorSuccess, fontSize: 13 }} />
                      )}
                    </Space>
                    <div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {p.flow === 'device_code' ? 'Device code' : 'Browser redirect'}
                        {p.requiresFixedPort && ' · port 1455'}
                      </Text>
                    </div>
                  </div>
                  <Tooltip title={p.riskNotice}>
                    <WarningOutlined
                      style={{ color: token.colorWarning, fontSize: 13, cursor: 'help' }}
                    />
                  </Tooltip>
                </Space>

                {mine.length > 0 && (
                  <Space direction="vertical" size={4} style={{ width: '100%' }}>
                    {mine.map((a) => (
                      <div
                        key={a.id}
                        style={{
                          display: 'flex',
                          alignItems: 'center',
                          justifyContent: 'space-between',
                          gap: 8,
                          padding: '4px 8px',
                          borderRadius: token.borderRadius,
                          background: token.colorFillQuaternary,
                        }}
                      >
                        <Text
                          ellipsis
                          style={{ fontSize: 12, flex: 1, minWidth: 0 }}
                          title={a.email ?? a.label}
                        >
                          {a.email ?? a.label}
                        </Text>
                        {formatExpiry(a)}
                      </div>
                    ))}
                  </Space>
                )}

                <Button
                  size="small"
                  block
                  type={mine.length ? 'default' : 'primary'}
                  ghost={!mine.length}
                  icon={busy ? undefined : <LinkOutlined />}
                  loading={busy}
                  onClick={() => connect(p)}
                >
                  {mine.length ? 'Add another' : 'Connect'}
                </Button>
              </div>
            );
          })}
        </div>
      )}

      {/* ---- Connected accounts ---- */}
      {accounts.length > 0 && (
        <>
          <Title level={5} style={{ marginBottom: 12 }}>
            Connected accounts
          </Title>
          <Table
            rowKey="id"
            size="small"
            pagination={false}
            dataSource={accounts}
            columns={accountColumns}
            style={{ marginBottom: 28 }}
          />
        </>
      )}

      {/* ---- Free-tier presets ---- */}
      <Title level={5} style={{ marginBottom: 4 }}>
        Free-tier providers
      </Title>
      <Paragraph type="secondary" style={{ marginBottom: 12 }}>
        Ready-made endpoints with a free allowance. Each needs its own API key unless marked
        otherwise.
      </Paragraph>

      {loading ? (
        <Skeleton active paragraph={{ rows: 4 }} />
      ) : (
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))',
            gap: 12,
          }}
        >
          {catalog.map((c) => (
            <div
              key={c.id}
              style={{
                border: `1px solid ${token.colorBorderSecondary}`,
                borderRadius: token.borderRadiusLG,
                background: token.colorBgContainer,
                padding: 14,
                display: 'flex',
                flexDirection: 'column',
                gap: 10,
              }}
            >
              <Space align="start" style={{ width: '100%' }}>
                <ProviderLogo color={c.brandColor} mark={c.brandMark} size={34} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <Space size={6} wrap align="center">
                    <Text strong style={{ fontSize: 14 }}>
                      {c.displayName}
                    </Text>
                    {c.auth === 'none' && (
                      <Tag color="green" style={{ marginInlineEnd: 0 }}>
                        No key
                      </Tag>
                    )}
                    {c.urlPlaceholder && (
                      <Tag color="blue" style={{ marginInlineEnd: 0 }}>
                        needs {c.urlPlaceholder}
                      </Tag>
                    )}
                  </Space>
                  <div>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {c.note}
                    </Text>
                  </div>
                </div>
              </Space>

              <Space size={4} wrap>
                {c.models.slice(0, 3).map((m) => (
                  <Tag
                    key={m.id}
                    style={{
                      marginInlineEnd: 0,
                      fontSize: 11,
                      background: token.colorFillQuaternary,
                      border: 'none',
                    }}
                  >
                    {m.name}
                  </Tag>
                ))}
                {c.models.length > 3 && (
                  <Text type="secondary" style={{ fontSize: 11 }}>
                    +{c.models.length - 3}
                  </Text>
                )}
              </Space>

              <Space style={{ width: '100%', justifyContent: 'flex-end' }} size={4}>
                {c.signupUrl && (
                  <Button
                    size="small"
                    type="text"
                    icon={<ExportOutlined />}
                    href={c.signupUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    Get key
                  </Button>
                )}
                <Button
                  size="small"
                  icon={<PlusOutlined />}
                  onClick={() => {
                    setPreset(c);
                    setPresetModel(c.models[0]?.id ?? '');
                    setPresetKey('');
                    setPresetUrlValue('');
                  }}
                >
                  Add
                </Button>
              </Space>
            </div>
          ))}
        </div>
      )}

      {/* ---- Device-code challenge ---- */}
      <Modal
        title={
          device ? (
            <Space>
              <ProviderLogo
                color={device.provider.brandColor}
                mark={device.provider.brandMark}
                size={26}
              />
              <span>Connect {device.provider.displayName}</span>
            </Space>
          ) : null
        }
        open={!!device}
        footer={null}
        onCancel={() => {
          // Closing only stops watching; the daemon's poll task ends on its
          // own when the device code expires.
          if (pollRef.current) window.clearInterval(pollRef.current);
          pollRef.current = null;
          setDevice(null);
          setConnecting(null);
        }}
      >
        <Paragraph>
          Enter this code at{' '}
          <a href={device?.verificationUri} target="_blank" rel="noopener noreferrer">
            {device?.verificationUri}
          </a>
          .
        </Paragraph>
        <div
          style={{
            fontSize: 32,
            fontWeight: 700,
            letterSpacing: 8,
            textAlign: 'center',
            padding: '20px 0',
            borderRadius: token.borderRadiusLG,
            background: token.colorFillQuaternary,
            fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
          }}
        >
          {device?.userCode}
        </div>
        <Space style={{ width: '100%', justifyContent: 'center', marginTop: 12 }}>
          <Button
            size="small"
            icon={<CopyOutlined />}
            onClick={() => {
              if (device?.userCode) {
                navigator.clipboard
                  ?.writeText(device.userCode)
                  .then(() => message.success('Code copied'))
                  .catch(() => message.error('Could not copy'));
              }
            }}
          >
            Copy code
          </Button>
          <Button
            size="small"
            type="link"
            icon={<ExportOutlined />}
            href={device?.verificationUri}
            target="_blank"
            rel="noopener noreferrer"
          >
            Open page
          </Button>
        </Space>
        <Paragraph
          type="secondary"
          style={{ fontSize: 12, marginTop: 16, marginBottom: 0, textAlign: 'center' }}
        >
          <Spin size="small" style={{ marginRight: 8 }} />
          Waiting for approval. This closes on its own once the provider confirms.
        </Paragraph>
      </Modal>

      {/* ---- Bind an account to a model ---- */}
      <Modal
        title={`Use ${bindAccount?.label ?? ''} as a model`}
        open={!!bindAccount}
        onCancel={() => setBindAccount(null)}
        onOk={submitBind}
        confirmLoading={binding}
        okText="Add model"
        okButtonProps={{ disabled: !bindModel.trim() }}
      >
        <Paragraph type="secondary">
          Creates a model entry backed by this account. No token is written into your config file
          — only a reference to the account.
        </Paragraph>
        {/* One field, not a dropdown plus a duplicate text box: the suggested
            ids are a convenience, but any model id must be typeable. */}
        <AutoComplete
          style={{ width: '100%' }}
          value={bindModel}
          onChange={setBindModel}
          placeholder="Model id — pick one or type your own"
          // Match on the label so typing either the display name or the id
          // narrows the list.
          // An input that exactly matches an option is a *selection*, not a
          // search term. Filtering on it would collapse the list to that one
          // entry — which is what hid the other models behind the pre-filled
          // default. Show everything until the user actually types something
          // new.
          filterOption={(input, option) => {
            if (bindModelOptions.some((m) => m.id === input)) return true;
            return String(option?.label ?? option?.value ?? '')
              .toLowerCase()
              .includes(input.toLowerCase());
          }}
          // Plain-string labels: AutoComplete paints the matched option's
          // label into the input itself, so a React node there renders on top
          // of the typed value.
          options={bindModelOptions.map((m) => ({
            value: m.id,
            label: `${m.name} — ${m.id}`,
          }))}
        />

        <div style={{ marginTop: 6 }}>
          {loadingModels ? (
            <Text type="secondary" style={{ fontSize: 12 }}>
              <Spin size="small" style={{ marginRight: 6 }} />
              Asking {bindProvider?.displayName} which models this account has…
            </Text>
          ) : modelSource === 'discovered' ? (
            <Text type="secondary" style={{ fontSize: 12 }}>
              <CheckCircleFilled style={{ color: token.colorSuccess, marginRight: 6 }} />
              {accountModels.length} models reported by this account.
            </Text>
          ) : modelSource === 'registry' ? (
            // Two different situations, and conflating them misleads: the
            // provider may have no listing endpoint at all, or the call may
            // have failed (expired token, no entitlement, network).
            <Tooltip title={modelReason ?? undefined}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {modelReason
                  ? `Could not read this account's model list (${modelReason.slice(0, 60)}) — showing known ids.`
                  : `${bindProvider?.displayName} publishes no model list — showing known ids.`}{' '}
                Probe one to confirm it works.
              </Text>
            </Tooltip>
          ) : null}
        </div>

        <Space style={{ marginTop: 12 }} size={8}>
          <Button
            size="small"
            icon={<ExperimentOutlined />}
            loading={probing}
            disabled={!bindModel.trim()}
            onClick={async () => {
              if (!bindAccount) return;
              const probe = await testModel(bindAccount.id, bindModel.trim());
              if (probe.ok) message.success(`${bindModel.trim()} works (${probe.latencyMs} ms)`);
              else message.error(probe.error ?? 'model unavailable');
            }}
          >
            Test this model
          </Button>
          {bindModelOptions.length > 1 && (
            <Button size="small" type="text" loading={probing} onClick={testAllModels}>
              Test all {bindModelOptions.length}
            </Button>
          )}
        </Space>

        {Object.keys(probes).length > 0 && (
          <div style={{ marginTop: 12 }}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              Probe results — a model that fails here is not available on this account.
            </Text>
            <div style={{ marginTop: 6, maxHeight: 220, overflowY: 'auto' }}>
              {bindModelOptions
                .filter((m) => probes[m.id])
                .map((m) => {
                  const p = probes[m.id];
                  return (
                    <div
                      key={m.id}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 8,
                        padding: '4px 8px',
                        borderRadius: token.borderRadius,
                        background:
                          bindModel === m.id ? token.colorFillSecondary : 'transparent',
                        cursor: 'pointer',
                      }}
                      onClick={() => setBindModel(m.id)}
                    >
                      {p.ok ? (
                        <CheckCircleFilled style={{ color: token.colorSuccess, fontSize: 13 }} />
                      ) : (
                        <CloseCircleFilled style={{ color: token.colorError, fontSize: 13 }} />
                      )}
                      <Text style={{ fontSize: 12, flex: 1 }}>{m.name}</Text>
                      {p.ok ? (
                        <Text type="secondary" style={{ fontSize: 11 }}>
                          {p.latencyMs} ms
                        </Text>
                      ) : (
                        <Tooltip title={p.error}>
                          <Text type="danger" style={{ fontSize: 11 }}>
                            unavailable
                          </Text>
                        </Tooltip>
                      )}
                    </div>
                  );
                })}
            </div>
          </div>
        )}
      </Modal>

      {/* ---- Add a free-tier preset ---- */}
      <Modal
        title={
          preset ? (
            <Space>
              <ProviderLogo color={preset.brandColor} mark={preset.brandMark} size={26} />
              <span>Add {preset.displayName}</span>
            </Space>
          ) : null
        }
        open={!!preset}
        onCancel={() => setPreset(null)}
        onOk={submitPreset}
        confirmLoading={addingPreset}
        okText="Add model"
        okButtonProps={{ disabled: !presetModel.trim() }}
      >
        <Space direction="vertical" style={{ width: '100%' }} size={12}>
          {preset?.urlPlaceholder && (
            <div>
              <Text>{preset.urlPlaceholder}</Text>
              <Input
                value={presetUrlValue}
                onChange={(e) => setPresetUrlValue(e.target.value)}
                placeholder={`Your ${preset.urlPlaceholder}`}
              />
            </div>
          )}
          {preset?.auth === 'api_key' && (
            <div>
              <Text>API key</Text>
              <Input.Password
                value={presetKey}
                onChange={(e) => setPresetKey(e.target.value)}
                placeholder="Paste the key"
              />
            </div>
          )}
          <div>
            <Text>Model</Text>
            <AutoComplete
              style={{ width: '100%' }}
              value={presetModel}
              onChange={setPresetModel}
              placeholder="Model id — pick one or type your own"
              filterOption={(input, option) => {
                if ((preset?.models ?? []).some((m) => m.id === input)) return true;
                return String(option?.label ?? option?.value ?? '')
                  .toLowerCase()
                  .includes(input.toLowerCase());
              }}
              options={(preset?.models ?? []).map((m) => ({
                value: m.id,
                label: `${m.name} — ${m.id}`,
              }))}
            />
          </div>
        </Space>
      </Modal>
    </div>
  );
};
