import React, { useEffect, useState, useCallback, useRef } from 'react';
import {
  Typography,
  Table,
  Tag,
  Button,
  Space,
  Progress,
  Popconfirm,
  message,
  Alert,
  Tooltip,
  Input,
  InputNumber,
  Card,
  Divider,
  Switch,
  Select,
  Form,
  Row,
  Col,
  Radio,
} from 'antd';
import {
  CloudDownloadOutlined,
  DeleteOutlined,
  StopOutlined,
  ReloadOutlined,
  CheckCircleOutlined,
  CopyOutlined,
  LinkOutlined,
  PoweroffOutlined,
  PlayCircleOutlined,
  ApiOutlined,
  SaveOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

type DownloadStatus =
  | 'queued'
  | 'listing'
  | 'downloading'
  | 'done'
  | 'error'
  | 'cancelled';

interface DownloadState {
  model_id: string;
  status: DownloadStatus;
  total_bytes: number;
  downloaded_bytes: number;
  current_file: string | null;
  files_total: number;
  files_done: number;
  error: string | null;
}

interface ModelEntry {
  id: string;
  label: string;
  approx_size_gb: number;
  context_length: number;
  native_supported: boolean;
  installed: boolean;
  on_disk_path: string;
  download: DownloadState | null;
  custom: boolean;
  loaded: boolean;
}

interface RuntimeInfo {
  runtime: string;
  python_available: boolean;
  python_binary: string | null;
  local_models_dir: string;
  platform: string;
}


const fmtBytes = (n: number): string => {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
};

interface InferenceSettings {
  kv_cache_bits: number | null;
  /** MLX Metal packed KV (4/8 bit); distinct from turboquant-rs */
  mlx_kv_cache_bits: number | null;
  tq_activate_at: number | null;
  enable_thinking: boolean | null;
  max_prompt_tokens: number | null;
  max_new_tokens: number | null;
  /** KV cache sliding window (tokens). Bounds prompt + decode in memory. */
  max_kv_tokens: number | null;
  /** Sampling temperature; `0` = greedy. Server defaults to ~0.65 for Gemma‑3. */
  temperature: number | null;
  /** HuggingFace-style repetition penalty; `1` = off. Server defaults to ~1.15 for Gemma‑3. */
  repetition_penalty: number | null;
  /** 0 = off; omit/null defaults to 60s on server; min 60 when explicitly set nonzero */
  idle_unload_secs: number | null;
  /** MLX: drop per-session KV/prefix cache the moment a chat session ends (tool-call-free final turn). Weights stay loaded. null/false = off. */
  release_cache_after_session: boolean | null;
  /** "mlx" | "candle" | null (auto) */
  preferred_backend: string | null;
}

const DEFAULT_SETTINGS: InferenceSettings = {
  kv_cache_bits: null,
  mlx_kv_cache_bits: null,
  tq_activate_at: null,
  enable_thinking: false,
  max_prompt_tokens: null,
  max_new_tokens: null,
  max_kv_tokens: null,
  temperature: null,
  repetition_penalty: null,
  idle_unload_secs: 60,
  release_cache_after_session: false,
  preferred_backend: null,
};

export const LocalModelsSettings: React.FC = () => {
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [runtime, setRuntime] = useState<RuntimeInfo | null>(null);
  const [hfInput, setHfInput] = useState('');
  const [installing, setInstalling] = useState(false);
  const [loading, setLoading] = useState(true);
  const pollRef = useRef<number | null>(null);
  const [settings, setSettings] = useState<InferenceSettings>(DEFAULT_SETTINGS);
  const [settingsSaving, setSettingsSaving] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const [listRes, rtRes, settingsRes] = await Promise.all([
        fetch('/api/local-models'),
        fetch('/api/local-models/runtime'),
        fetch('/api/local-models/settings'),
      ]);
      if (!listRes.ok) throw new Error(`list failed (${listRes.status})`);
      if (!rtRes.ok) throw new Error(`runtime failed (${rtRes.status})`);
      const list = await listRes.json();
      const rt = await rtRes.json();
      setModels(list.models || []);
      setRuntime(rt);
      if (settingsRes.ok) {
        const s = await settingsRes.json();
        setSettings({ ...DEFAULT_SETTINGS, ...s });
      }
    } catch (e: any) {
      message.error(`Failed to load local models: ${e.message}`);
    } finally {
      setLoading(false);
    }
  }, []);

  const saveSettings = useCallback(async () => {
    setSettingsSaving(true);
    try {
      const res = await fetch('/api/local-models/settings', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(settings),
      });
      if (!res.ok) {
        const txt = await res.text();
        message.error(`Failed to save: ${txt}`);
        return;
      }
      message.success('Inference settings saved');
    } catch (e: any) {
      message.error(`Save error: ${e.message}`);
    } finally {
      setSettingsSaving(false);
    }
  }, [settings]);

  const handleInstallFromHf = useCallback(async () => {
    const raw = hfInput.trim();
    if (!raw) return;
    setInstalling(true);
    try {
      const res = await fetch(
        `/api/local-models/${encodeURIComponent(raw)}/download`,
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: '{}',
        }
      );
      if (!res.ok) {
        const txt = await res.text();
        message.error(`Install failed: ${txt}`);
        return;
      }
      const j = await res.json();
      message.success(`Download started: ${j.id}`);
      setHfInput('');
      refresh();
    } finally {
      setInstalling(false);
    }
  }, [hfInput, refresh]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Poll while any download is active.
  useEffect(() => {
    const hasActive = models.some(
      (m) =>
        m.download &&
        (m.download.status === 'queued' ||
          m.download.status === 'listing' ||
          m.download.status === 'downloading')
    );
    if (hasActive && pollRef.current === null) {
      pollRef.current = window.setInterval(refresh, 1500);
    } else if (!hasActive && pollRef.current !== null) {
      window.clearInterval(pollRef.current);
      pollRef.current = null;
    }
    return () => {
      if (pollRef.current !== null) {
        window.clearInterval(pollRef.current);
        pollRef.current = null;
      }
    };
  }, [models, refresh]);

  const handleDownload = async (id: string) => {
    const res = await fetch(
      `/api/local-models/${encodeURIComponent(id)}/download`,
      { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: '{}' }
    );
    if (!res.ok) {
      const txt = await res.text();
      message.error(`Download failed to start: ${txt}`);
      return;
    }
    message.success(`Download started for ${id}`);
    refresh();
  };

  const handleCancel = async (id: string) => {
    await fetch(`/api/local-models/${encodeURIComponent(id)}/cancel`, {
      method: 'POST',
    });
    refresh();
  };

  const handleDelete = async (id: string) => {
    const res = await fetch(`/api/local-models/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
    if (!res.ok) {
      message.error(`Delete failed`);
      return;
    }
    message.success(`Removed ${id}`);
    refresh();
  };

  const handleLoad = async (id: string) => {
    // Use preferred_backend from settings: mlx → /load-mlx, else → /load (Candle)
    const useMlx = settings.preferred_backend === 'mlx';
    const endpoint = useMlx
      ? `/api/local-models/${encodeURIComponent(id)}/load-mlx`
      : `/api/local-models/${encodeURIComponent(id)}/load`;
    const res = await fetch(endpoint, { method: 'POST' });
    if (!res.ok) {
      const txt = await res.text();
      message.error(`Load failed: ${txt}`);
      return;
    }
    message.success(`Loaded ${id} (${useMlx ? 'MLX native' : 'Candle'})`);
    refresh();
  };

  const handleUnload = async (id: string) => {
    const res = await fetch(`/api/local-models/${encodeURIComponent(id)}/unload`, {
      method: 'POST',
    });
    if (!res.ok) {
      message.error(`Unload failed`);
      return;
    }
    message.success(`Unloaded ${id}`);
    refresh();
  };

  /** Unload every loaded engine (Candle + MLX). Useful to reclaim RAM
   *  without waiting for `idle_unload_secs`. Returns the list that was
   *  actually dropped so we can show feedback. */
  const handleUnloadAll = async () => {
    const res = await fetch(`/api/local-models/unload-all`, { method: 'POST' });
    if (!res.ok) {
      message.error(`Unload-all failed`);
      return;
    }
    const body: { count: number; unloaded: string[] } = await res.json();
    if (body.count === 0) {
      message.info('No models were loaded — nothing to unload');
    } else {
      message.success(`Unloaded ${body.count} model(s): ${body.unloaded.join(', ')}`);
    }
    refresh();
  };

  const handleUseAsLlm = async (id: string) => {
    // Pass ?backend= so the server uses the right profile type.
    const backendParam = settings.preferred_backend
      ? `?backend=${settings.preferred_backend}`
      : '';
    const res = await fetch(
      `/api/local-models/${encodeURIComponent(id)}/use-as-llm${backendParam}`,
      { method: 'POST' }
    );
    if (!res.ok) {
      const txt = await res.text();
      message.error(`Failed to add as LLM: ${txt}`);
      return;
    }
    const j = await res.json();
    if (j.existed) {
      message.info(`Already in LLM Models: ${j.config.label}`);
      return;
    }
    message.success(
      j.active
        ? `Added as LLM profile and set active: ${j.config.label}`
        : `Added as LLM profile: ${j.config.label}`
    );
  };

  const handleCopyLlmConfig = (m: ModelEntry) => {
    const cfg = {
      label: `Local ${m.label}`,
      provider: 'local-mlx',
      modelName: m.id,
      baseURL: '',
      apiKey: '',
      adapt: 'local-mlx-native',
      maxTokens: 2048,
      contextLength: m.context_length,
    };
    navigator.clipboard.writeText(JSON.stringify(cfg, null, 2));
    message.success('LLM config JSON copied to clipboard');
  };

  const columns = [
    {
      title: 'Model',
      key: 'model',
      render: (_: any, m: ModelEntry) => (
        <Space direction="vertical" size={0}>
          <Space size={6}>
            <Text strong>{m.label}</Text>
            {m.custom && <Tag color="blue">custom</Tag>}
          </Space>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {m.id}
          </Text>
        </Space>
      ),
    },
    {
      title: 'Size',
      key: 'size',
      width: 80,
      render: (_: any, m: ModelEntry) =>
        m.approx_size_gb > 0 ? `~${m.approx_size_gb} GB` : '—',
    },
    {
      title: 'Context',
      key: 'context',
      width: 110,
      render: (_: any, m: ModelEntry) =>
        m.context_length > 0 ? `${(m.context_length / 1024).toFixed(0)}K` : '—',
    },
    {
      title: 'Sidecar',
      key: 'native',
      width: 110,
      render: (_: any, m: ModelEntry) =>
        m.native_supported ? (
          <Tag color="green">supported</Tag>
        ) : (
          <Tooltip title="Sidecar supports any mlx-community model via Python mlx_lm.server.">
            <Tag color="green">supported</Tag>
          </Tooltip>
        ),
    },
    {
      title: 'Status',
      key: 'status',
      render: (_: any, m: ModelEntry) => {
        const d = m.download;
        if (d && (d.status === 'downloading' || d.status === 'listing' || d.status === 'queued')) {
          const pct =
            d.total_bytes > 0
              ? Math.min(
                  100,
                  Math.round((d.downloaded_bytes / d.total_bytes) * 100)
                )
              : 0;
          return (
            <Space direction="vertical" size={2} style={{ width: 240 }}>
              <Progress
                percent={pct}
                size="small"
                status="active"
                format={(p) => `${p}%`}
              />
              <Text type="secondary" style={{ fontSize: 11 }}>
                {d.status === 'listing'
                  ? 'Listing files…'
                  : `${d.files_done}/${d.files_total} files · ${fmtBytes(
                      d.downloaded_bytes
                    )} / ${fmtBytes(d.total_bytes)}`}
              </Text>
              {d.current_file && (
                <Text type="secondary" style={{ fontSize: 11 }} ellipsis>
                  {d.current_file}
                </Text>
              )}
            </Space>
          );
        }
        if (d && d.status === 'error') {
          return (
            <Tooltip title={d.error || 'unknown'}>
              <Tag color="red">error</Tag>
            </Tooltip>
          );
        }
        if (d && d.status === 'cancelled') {
          return <Tag>cancelled</Tag>;
        }
        if (m.installed) {
          return (
            <Space size={4}>
              <Tag color="green" icon={<CheckCircleOutlined />}>
                installed
              </Tag>
              {m.loaded && <Tag color="blue">loaded</Tag>}
            </Space>
          );
        }
        return <Tag>not installed</Tag>;
      },
    },
    {
      title: 'Actions',
      key: 'actions',
      width: 380,
      render: (_: any, m: ModelEntry) => {
        const d = m.download;
        const isActive =
          d &&
          (d.status === 'downloading' ||
            d.status === 'listing' ||
            d.status === 'queued');
        return (
          <Space>
            {!m.installed && !isActive && (
              <Button
                size="small"
                type="primary"
                icon={<CloudDownloadOutlined />}
                onClick={() => handleDownload(m.id)}
              >
                Download
              </Button>
            )}
            {isActive && (
              <Button
                size="small"
                danger
                icon={<StopOutlined />}
                onClick={() => handleCancel(m.id)}
              >
                Cancel
              </Button>
            )}
            {m.installed && !m.loaded && (
              <Tooltip
                title={
                  settings.preferred_backend === 'mlx'
                    ? 'Load with MLX native (mlx-rs, ~60–100 tok/s)'
                    : settings.preferred_backend === 'candle'
                    ? 'Load with Candle (CPU+Accelerate, ~12 tok/s)'
                    : 'Load model into memory (backend: auto)'
                }
              >
                <Button
                  size="small"
                  icon={<PlayCircleOutlined />}
                  onClick={() => handleLoad(m.id)}
                >
                  Load{settings.preferred_backend === 'mlx' ? ' (MLX)' : settings.preferred_backend === 'candle' ? ' (Candle)' : ''}
                </Button>
              </Tooltip>
            )}
            {m.installed && m.loaded && (
              <Tooltip title="Free this model's memory">
                <Button
                  size="small"
                  icon={<PoweroffOutlined />}
                  onClick={() => handleUnload(m.id)}
                >
                  Unload
                </Button>
              </Tooltip>
            )}
            {m.installed && (
              <>
                <Tooltip
                  title={
                    settings.preferred_backend === 'mlx'
                      ? 'Create an LLM profile using MLX native backend (mlx-rs)'
                      : settings.preferred_backend === 'candle'
                      ? 'Create an LLM profile using Candle backend'
                      : 'Create an LLM profile (backend: auto-detect)'
                  }
                >
                  <Button
                    size="small"
                    icon={<ApiOutlined />}
                    onClick={() => handleUseAsLlm(m.id)}
                  >
                    Use in LLM
                  </Button>
                </Tooltip>
                <Tooltip title="Copy LLM config JSON manually">
                  <Button
                    size="small"
                    icon={<CopyOutlined />}
                    onClick={() => handleCopyLlmConfig(m)}
                  />
                </Tooltip>
                <Popconfirm
                  title={`Remove ${m.id} from disk?`}
                  onConfirm={() => handleDelete(m.id)}
                  okText="Remove"
                  okButtonProps={{ danger: true }}
                >
                  <Button size="small" danger icon={<DeleteOutlined />} />
                </Popconfirm>
              </>
            )}
          </Space>
        );
      },
    },
  ];

  return (
    <div>
      <Space style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}>
        <Title level={3} style={{ margin: 0 }}>
          Local Models
        </Title>
        <Space>
          <Tooltip title="Drop all in-memory model weights + prefix caches now. Use this to reclaim ~5–10 GB RAM without waiting for the idle timer.">
            <Button
              size="small"
              danger
              icon={<PoweroffOutlined />}
              onClick={handleUnloadAll}
            >
              Unload all
            </Button>
          </Tooltip>
          <Button icon={<ReloadOutlined />} onClick={refresh}>
            Refresh
          </Button>
        </Space>
      </Space>

      <Paragraph type="secondary">
        Download HuggingFace MLX models, then click <b>Load</b> to start local
        inference. <b>Use in LLM</b> creates an OpenAI-compatible profile
        pointing at this daemon.
      </Paragraph>

      {runtime && runtime.platform !== 'macos' && (
        <Alert
          style={{ marginBottom: 16 }}
          type="info"
          showIcon
          message={`Detected platform: ${runtime.platform}`}
          description="MLX inference only runs on macOS Apple Silicon."
        />
      )}
      {runtime && (
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          Models directory: <code>{runtime.local_models_dir}</code>
          {runtime.python_binary && (
            <>
              {' · '}Python: <code>{runtime.python_binary}</code>
            </>
          )}
        </Paragraph>
      )}

      {/* ── Install from HuggingFace URL/repo ─────────────────────────── */}
      <Card
        size="small"
        title={
          <Space>
            <LinkOutlined /> Install from HuggingFace
          </Space>
        }
        style={{ marginBottom: 16 }}
      >
        <Space.Compact style={{ width: '100%' }}>
          <Input
            placeholder="e.g. mlx-community/Qwen3-4B-bf16  or  https://huggingface.co/mlx-community/..."
            value={hfInput}
            onChange={(e) => setHfInput(e.target.value)}
            onPressEnter={handleInstallFromHf}
            allowClear
          />
          <Button
            type="primary"
            icon={<CloudDownloadOutlined />}
            loading={installing}
            onClick={handleInstallFromHf}
            disabled={!hfInput.trim()}
          >
            Install
          </Button>
        </Space.Compact>
        <Paragraph type="secondary" style={{ fontSize: 12, margin: '8px 0 0' }}>
          Accepts <code>org/repo</code> or any full HuggingFace URL. Custom
          installs appear in the table below tagged as <Tag>custom</Tag>.
        </Paragraph>
      </Card>

      {/* ── Inference settings ───────────────────────────────────────── */}
      <Card
        size="small"
        title={
          <Space>
            <ThunderboltOutlined />
            Inference Settings
          </Space>
        }
        style={{ marginBottom: 16 }}
        extra={
          <Button
            type="primary"
            size="small"
            icon={<SaveOutlined />}
            loading={settingsSaving}
            onClick={saveSettings}
          >
            Save
          </Button>
        }
      >
        <Form layout="vertical" size="small">
          {/* ── Backend selector ──────────────────────────────────── */}
          <Form.Item
            label={
              <Tooltip title="Which inference engine to use for Load and Use in LLM. MLX native uses mlx-rs (Apple Silicon only, ~60–100 tok/s). Candle runs on CPU with Apple Accelerate BLAS (~12 tok/s) or Metal (~7 tok/s).">
                Inference backend
              </Tooltip>
            }
            style={{ marginBottom: 16 }}
          >
            <Radio.Group
              value={settings.preferred_backend ?? 'auto'}
              onChange={(e) => {
                const v = e.target.value;
                setSettings((s) => ({
                  ...s,
                  preferred_backend: v === 'auto' ? null : v,
                }));
              }}
              optionType="button"
              buttonStyle="solid"
            >
              <Radio.Button value="auto">
                <Tooltip title="Auto: MLX native when compiled with --features local-mlx, else Candle">
                  Auto
                </Tooltip>
              </Radio.Button>
              <Radio.Button value="mlx">
                <Space size={4}>
                  <ThunderboltOutlined style={{ color: '#faad14' }} />
                  MLX native
                  <Tag color="green" style={{ marginLeft: 2, fontSize: 11, padding: '0 4px' }}>
                    ~60–100 tok/s
                  </Tag>
                </Space>
              </Radio.Button>
              <Radio.Button value="candle">
                <Space size={4}>
                  Candle
                  <Tag color="default" style={{ marginLeft: 2, fontSize: 11, padding: '0 4px' }}>
                    ~12 tok/s
                  </Tag>
                </Space>
              </Radio.Button>
            </Radio.Group>
          </Form.Item>

          <Form.Item
            label={
              <Tooltip title="Unload weights after idle (MLX or Candle). Resets each run and after Load. Default 60s when unset. Set 0 to disable. Explicit nonzero values must be ≥ 60 (server). Max 604800 (7d).">
                Auto unload after idle (seconds)
              </Tooltip>
            }
            style={{ marginBottom: 12 }}
          >
            <Space align="center">
              <InputNumber
                style={{ width: 140 }}
                min={0}
                max={604800}
                step={60}
                value={settings.idle_unload_secs ?? undefined}
                placeholder="60 — 0 off"
                onChange={(v) =>
                  setSettings((s) => ({
                    ...s,
                    idle_unload_secs: v === null || v === undefined ? null : v,
                  }))
                }
              />
              <Text type="secondary" style={{ fontSize: 12 }}>
                · default 60 · 0 = off
              </Text>
            </Space>
          </Form.Item>

          <Divider style={{ margin: '8px 0 16px' }} />

          <Row gutter={24}>
            {/* KV cache TurboQuant */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip title="Quantize KV cache entries to reduce memory during long generation. TQ4 = 4-bit total (3-bit key, 4-bit value). TQ3 = 3-bit total. Auto enables TQ4 when the model weights are 4-bit.">
                    KV TurboQuant bits
                  </Tooltip>
                }
              >
                <Select
                  value={settings.kv_cache_bits ?? 'auto'}
                  onChange={(v) =>
                    setSettings((s) => ({
                      ...s,
                      kv_cache_bits: v === 'auto' ? null : (v as number),
                    }))
                  }
                  options={[
                    { value: 'auto', label: 'Auto (4-bit for 4-bit models)' },
                    { value: 4, label: 'TQ4 — 4-bit total (recommended)' },
                    { value: 3, label: 'TQ3 — 3-bit total' },
                    { value: 0, label: 'Off — FP16 (no quantization)' },
                  ]}
                />
              </Form.Item>
            </Col>

            {/* MLX packed KV (Metal) */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip title="MLX native only: quantize KV cache on GPU (mlx.core.quantize). Saves RAM vs FP16. Requires rebuild with local-mlx. Reload model after change.">
                    MLX packed KV (Metal)
                  </Tooltip>
                }
              >
                <Select
                  value={settings.mlx_kv_cache_bits ?? 'off'}
                  onChange={(v) =>
                    setSettings((s) => ({
                      ...s,
                      mlx_kv_cache_bits: v === 'off' ? null : (v as number),
                    }))
                  }
                  options={[
                    { value: 'off', label: 'Off — FP16' },
                    { value: 4, label: '4-bit packed' },
                    { value: 8, label: '8-bit packed' },
                  ]}
                />
              </Form.Item>
            </Col>

            {/* TQ activate-at */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip
                    title={
                      <span>
                        Number of cached tokens before TurboQuant kicks in.
                        Once activated, new KV tokens route through a
                        CPU-side 4-bit pack (~20× slower per token) — only
                        worthwhile for very long contexts where the RAM
                        saving justifies the latency. The auto-disable guard
                        keeps TQ off any turn where{' '}
                        <code>prompt + max_new &gt; tq_activate_at</code> so
                        decode never stalls.
                        <br />
                        <br />
                        Default: <strong>16384</strong>. Raise to keep TQ on
                        for longer prompts; lower (or to 0) to quantize
                        aggressively. Setting{' '}
                        <code>kv_cache_bits = null</code> disables TQ
                        entirely.
                      </span>
                    }
                  >
                    TQ activate after (tokens)
                  </Tooltip>
                }
              >
                <InputNumber
                  style={{ width: '100%' }}
                  min={0}
                  max={262144}
                  step={1024}
                  value={settings.tq_activate_at ?? undefined}
                  placeholder="16384 (default)"
                  onChange={(v) =>
                    setSettings((s) => ({ ...s, tq_activate_at: v ?? null }))
                  }
                />
              </Form.Item>
            </Col>

            {/* Thinking mode */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip title="Enable chain-of-thought thinking for Qwen3 models. Off = model answers directly without a <think> block (faster, uses fewer tokens). Enable for complex reasoning tasks.">
                    Thinking mode (Qwen3)
                  </Tooltip>
                }
              >
                <Switch
                  checked={settings.enable_thinking === true}
                  onChange={(v) =>
                    setSettings((s) => ({ ...s, enable_thinking: v }))
                  }
                  checkedChildren="On"
                  unCheckedChildren="Off"
                />
              </Form.Item>
            </Col>

            {/* Release cache after session (MLX) */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip title="MLX only: free the model's KV / prefix cache (hundreds of MB) as soon as a chat session ends — the final answer that makes no tool calls. Weights stay loaded, so the next session is still warm; only the per-session KV is dropped immediately instead of waiting for the idle-unload timer. Within a session, tool-call turns keep the cache (prefix-cache hits still work). Best for RAM-tight machines.">
                    Release cache after session (MLX)
                  </Tooltip>
                }
              >
                <Switch
                  checked={settings.release_cache_after_session === true}
                  onChange={(v) =>
                    setSettings((s) => ({ ...s, release_cache_after_session: v }))
                  }
                  checkedChildren="On"
                  unCheckedChildren="Off"
                />
              </Form.Item>
            </Col>

            {/* Max prompt tokens */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip title="Hard cap on prompt length after chat-template encoding. Acts together with Max KV tokens — the effective per-turn budget is the smaller of the two (minus decode headroom). Default: 128000.">
                    Max prompt tokens
                  </Tooltip>
                }
              >
                <InputNumber
                  style={{ width: '100%' }}
                  min={512}
                  max={262144}
                  step={1024}
                  value={settings.max_prompt_tokens ?? undefined}
                  placeholder="128000 (default)"
                  onChange={(v) =>
                    setSettings((s) => ({ ...s, max_prompt_tokens: v ?? null }))
                  }
                />
              </Form.Item>
            </Col>

            {/* Max new tokens */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip title="Maximum number of tokens to generate per request. Default: 8192.">
                    Max new tokens
                  </Tooltip>
                }
              >
                <InputNumber
                  style={{ width: '100%' }}
                  min={1}
                  max={8192}
                  step={256}
                  value={settings.max_new_tokens ?? undefined}
                  placeholder="8192 (default)"
                  onChange={(v) =>
                    setSettings((s) => ({ ...s, max_new_tokens: v ?? null }))
                  }
                />
              </Form.Item>
            </Col>

            {/* Max KV-cache tokens (sliding window) */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip
                    title={
                      <span>
                        KV-cache sliding window. Bounds prompt + decode in
                        memory; ~25 % is reserved for decode so the prompt
                        budget is roughly 75 % of this value. Raise for many
                        MCP tools or long history; drop on low-RAM devices
                        (Candle CPU is O(L²) — large values get slow). Default:
                        16384.
                      </span>
                    }
                  >
                    Max KV tokens
                  </Tooltip>
                }
              >
                <InputNumber
                  style={{ width: '100%' }}
                  min={128}
                  max={262144}
                  step={1024}
                  value={settings.max_kv_tokens ?? undefined}
                  placeholder="16384 (default)"
                  onChange={(v) =>
                    setSettings((s) => ({ ...s, max_kv_tokens: v ?? null }))
                  }
                />
              </Form.Item>
            </Col>

            {/* Temperature (native MLX sampling) */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip title="`0` = greedy argmax (default for Qwen/Llama via server). Leave empty to use server default (~0.65 for Gemma‑3) to reduce repetitive garbage output.">
                    Temperature (MLX)
                  </Tooltip>
                }
              >
                <InputNumber
                  style={{ width: '100%' }}
                  min={0}
                  max={4}
                  step={0.05}
                  value={settings.temperature ?? undefined}
                  placeholder="default (Gemma ≈0.65)"
                  onChange={(v) =>
                    setSettings((s) => ({ ...s, temperature: v ?? null }))
                  }
                />
              </Form.Item>
            </Col>

            {/* Repetition penalty */}
            <Col xs={24} sm={12} md={8}>
              <Form.Item
                label={
                  <Tooltip title="HuggingFace-style penalty on recent token logits. `1` = off. Leave empty for server default (~1.15 on Gemma‑3).">
                    Repetition penalty (MLX)
                  </Tooltip>
                }
              >
                <InputNumber
                  style={{ width: '100%' }}
                  min={1}
                  max={2}
                  step={0.05}
                  value={settings.repetition_penalty ?? undefined}
                  placeholder="default (Gemma ≈1.15)"
                  onChange={(v) =>
                    setSettings((s) => ({ ...s, repetition_penalty: v ?? null }))
                  }
                />
              </Form.Item>
            </Col>
          </Row>
        </Form>
      </Card>

      <Table
        rowKey="id"
        columns={columns as any}
        dataSource={models}
        loading={loading}
        pagination={false}
        size="middle"
      />
    </div>
  );
};
