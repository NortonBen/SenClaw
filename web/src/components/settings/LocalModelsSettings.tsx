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
  Card,
  Divider,
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

export const LocalModelsSettings: React.FC = () => {
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [runtime, setRuntime] = useState<RuntimeInfo | null>(null);
  const [hfInput, setHfInput] = useState('');
  const [installing, setInstalling] = useState(false);
  const [loading, setLoading] = useState(true);
  const pollRef = useRef<number | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [listRes, rtRes] = await Promise.all([
        fetch('/api/local-models'),
        fetch('/api/local-models/runtime'),
      ]);
      if (!listRes.ok) throw new Error(`list failed (${listRes.status})`);
      if (!rtRes.ok) throw new Error(`runtime failed (${rtRes.status})`);
      const list = await listRes.json();
      const rt = await rtRes.json();
      setModels(list.models || []);
      setRuntime(rt);
    } catch (e: any) {
      message.error(`Failed to load local models: ${e.message}`);
    } finally {
      setLoading(false);
    }
  }, []);

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
    const res = await fetch(`/api/local-models/${encodeURIComponent(id)}/load`, {
      method: 'POST',
    });
    if (!res.ok) {
      const txt = await res.text();
      message.error(`Load failed: ${txt}`);
      return;
    }
    message.success(`Loaded ${id}`);
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

  const handleUseAsLlm = async (id: string) => {
    const res = await fetch(
      `/api/local-models/${encodeURIComponent(id)}/use-as-llm`,
      { method: 'POST' }
    );
    if (!res.ok) {
      const txt = await res.text();
      message.error(`Failed to add as LLM: ${txt}`);
      return;
    }
    const j = await res.json();
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
              <Tooltip title="Keep this model warm in memory for fast inference">
                <Button
                  size="small"
                  icon={<PlayCircleOutlined />}
                  onClick={() => handleLoad(m.id)}
                >
                  Load
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
                <Tooltip title="Create an LLM profile pointing at this local model">
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
          Local Models (MLX)
        </Title>
        <Button icon={<ReloadOutlined />} onClick={refresh}>
          Refresh
        </Button>
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
