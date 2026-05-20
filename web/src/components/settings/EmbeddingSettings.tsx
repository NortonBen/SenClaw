import React, { useState, useEffect } from 'react';
import {
  Typography,
  Form,
  Select,
  Input,
  InputNumber,
  Button,
  Alert,
  Spin,
  Divider,
  Card,
  Space,
  Tag,
  Progress,
  message,
} from 'antd';
import {
  SaveOutlined,
  DatabaseOutlined,
  CloudDownloadOutlined,
  ThunderboltOutlined,
  CheckCircleOutlined,
  CloseCircleOutlined,
} from '@ant-design/icons';

const { Title, Text } = Typography;
const { Option } = Select;

interface EmbeddingConfig {
  provider: string;
  apiKey: string;
  baseURL: string;
  modelName: string;
  modelPath: string;
  dimensions: number | null;
}

const PROVIDER_DEFAULTS: Record<string, Partial<EmbeddingConfig>> = {
  none: { apiKey: '', baseURL: '', modelName: '', modelPath: '' },
  openai: {
    baseURL: 'https://api.openai.com/v1',
    modelName: 'text-embedding-3-small',
    modelPath: '',
  },
  openrouter: {
    baseURL: 'https://openrouter.ai/api/v1',
    modelName: 'openai/text-embedding-3-small',
    modelPath: '',
  },
  ollama: {
    baseURL: 'http://localhost:11434',
    modelName: 'nomic-embed-text',
    apiKey: '',
    modelPath: '',
  },
  local: {
    baseURL: '',
    apiKey: '',
    modelName: 'all-MiniLM-L6-v2',
    modelPath: '',
  },
};

const PROVIDER_LABELS: Record<string, string> = {
  none: 'None (FTS only)',
  openai: 'OpenAI',
  openrouter: 'OpenRouter',
  ollama: 'Ollama (local server)',
  local: 'Local (candle / on-device)',
};

const PROVIDER_DESCRIPTIONS: Record<string, string> = {
  none: 'Full-text search only — no vector embeddings.',
  openai: 'Remote API. Requires an OpenAI API key.',
  openrouter: 'Route through OpenRouter. Requires an OpenRouter API key.',
  ollama: 'Self-hosted Ollama server running locally.',
  local: 'Pure-Rust BERT inference via candle. No network required. Build with --features local-embed.',
};

const LOCAL_MODEL_OPTIONS = [
  { value: 'all-MiniLM-L6-v2', label: 'all-MiniLM-L6-v2 (384d, ~90MB)' },
  { value: 'all-MiniLM-L12-v2', label: 'all-MiniLM-L12-v2 (384d, ~120MB)' },
  { value: 'multilingual-e5-small', label: 'multilingual-e5-small (384d, ~120MB)' },
  { value: 'multilingual-e5-base', label: 'multilingual-e5-base (768d, ~280MB)' },
  { value: 'paraphrase-multilingual-MiniLM-L12-v2', label: 'paraphrase-multilingual-MiniLM-L12-v2 (384d)' },
];

interface EmbeddingFeatures {
  candle: boolean;
  candle_metal: boolean;
  mlx_static: boolean;
  models_dir: string;
}

interface BackendModel {
  id: string;
  repo: string;
  dimensions: number;
  size_hint: string;
  installed: boolean;
  on_disk_path: string;
}

export const EmbeddingSettings: React.FC = () => {
  const [form] = Form.useForm();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [provider, setProvider] = useState<string>('none');
  const [status, setStatus] = useState<{ msg: string; type: 'success' | 'error' | 'info' | '' }>({ msg: '', type: '' });
  const [features, setFeatures] = useState<EmbeddingFeatures | null>(null);
  const [downloading, setDownloading] = useState(false);
  /**
   * Curated model catalog from the backend, kept in sync with the on-disk
   * cache. Drives the "Downloaded" badge in the dropdown and the
   * Download / Re-download button label.
   */
  const [backendModels, setBackendModels] = useState<BackendModel[]>([]);

  /**
   * Live download progress state.
   *
   * Shape mirrors the backend SSE `DlEvent` payloads. Kept as a single
   * object instead of N pieces so a re-download fully resets the preview
   * card with `setProgress(null)`.
   */
  // Watch the modelName field so the "Downloaded" hint + button label
  // re-render when the user picks a different option from the dropdown.
  // Without this, `form.getFieldValue('modelName')` reads stale at render time.
  const watchedModelName: string | undefined = Form.useWatch('modelName', form);
  interface DownloadProgress {
    repo: string;                                       // HF repo id
    files: string[];                                    // ordered list of files
    currentFile: string | null;                         // active file name
    perFile: Record<string, { downloaded: number; total: number | null; done: boolean }>;
    done: boolean;                                      // whole download finished
    dir: string | null;                                 // destination dir on success
    error: string | null;                               // populated on `error` event
  }
  const [progress, setProgress] = useState<DownloadProgress | null>(null);

  /** Backend feature introspection — drives the MLX hint + download UI. */
  const fetchFeatures = async () => {
    try {
      const r = await fetch('/api/embedding/features');
      if (r.ok) setFeatures(await r.json());
    } catch {
      /* non-fatal */
    }
  };

  /** Refresh the on-disk catalog so the "Downloaded" tag stays current. */
  const fetchModels = async () => {
    try {
      const r = await fetch('/api/embedding/models');
      if (!r.ok) return;
      const body = await r.json();
      setBackendModels(body.models ?? []);
    } catch {
      /* non-fatal */
    }
  };

  /**
   * Download the currently-selected local model from HuggingFace into
   * ~/.senclaw/models/<name>/, **streaming live progress** via SSE.
   *
   * Backend emits one JSON event per SSE `data:` line:
   *   { phase: "start", repo, files }
   *   { phase: "file_start", file, total }
   *   { phase: "progress", file, downloaded, total }
   *   { phase: "file_done", file }
   *   { phase: "done", dir }
   *   { phase: "error", message }
   *
   * EventSource can't POST a body, so we use fetch + ReadableStream and
   * parse the `text/event-stream` framing manually (events delimited by
   * blank lines; payload on `data:` lines).
   */
  const downloadModel = async () => {
    const name: string = form.getFieldValue('modelName');
    if (!name) {
      message.warning('Pick a model first');
      return;
    }
    setDownloading(true);
    setProgress({
      repo: '',
      files: [],
      currentFile: null,
      perFile: {},
      done: false,
      dir: null,
      error: null,
    });

    try {
      const r = await fetch('/api/embedding/download-model', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model: name }),
      });

      // Errors come back as JSON (not SSE) — surface them as a single error event.
      if (!r.ok) {
        const body = await r.json().catch(() => ({ error: r.statusText }));
        throw new Error(body.error ?? `HTTP ${r.status}`);
      }

      const reader = r.body?.getReader();
      if (!reader) throw new Error('streaming not supported in this browser');

      const decoder = new TextDecoder();
      let buf = '';

      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });

        // SSE frames are separated by a blank line. Process whole frames
        // only — keep the trailing partial in `buf`.
        let idx: number;
        while ((idx = buf.indexOf('\n\n')) !== -1) {
          const frame = buf.slice(0, idx);
          buf = buf.slice(idx + 2);
          // Each frame may have multiple lines like `data: …`. Concatenate.
          const dataLines = frame
            .split('\n')
            .filter(l => l.startsWith('data:'))
            .map(l => l.slice(5).trim());
          if (dataLines.length === 0) continue;
          try {
            const ev = JSON.parse(dataLines.join('\n'));
            setProgress(prev => applyEvent(prev, ev));
          } catch (e) {
            console.warn('bad SSE frame', e, dataLines);
          }
        }
      }
    } catch (e: any) {
      setProgress(prev => ({
        ...(prev ?? {
          repo: '',
          files: [],
          currentFile: null,
          perFile: {},
          done: false,
          dir: null,
        }),
        error: e?.message ?? String(e),
      }));
      message.error(`Download failed: ${e?.message ?? e}`);
    } finally {
      setDownloading(false);
      // The on-disk cache may have changed (success or partial). Refresh
      // so the "Downloaded" badges in the dropdown are accurate.
      fetchModels();
    }
  };

  /** Fold one SSE event into the progress state. Pure-ish — for setState. */
  function applyEvent(
    prev: typeof progress,
    ev: any,
  ): typeof progress {
    const base = prev ?? {
      repo: '',
      files: [] as string[],
      currentFile: null as string | null,
      perFile: {} as Record<string, { downloaded: number; total: number | null; done: boolean }>,
      done: false,
      dir: null as string | null,
      error: null as string | null,
    };
    switch (ev.phase) {
      case 'start':
        return {
          ...base,
          repo: ev.repo,
          files: ev.files,
          perFile: Object.fromEntries(
            (ev.files as string[]).map(f => [f, { downloaded: 0, total: null, done: false }]),
          ),
        };
      case 'file_start':
        return {
          ...base,
          currentFile: ev.file,
          perFile: {
            ...base.perFile,
            [ev.file]: { downloaded: 0, total: ev.total ?? null, done: false },
          },
        };
      case 'progress':
        return {
          ...base,
          perFile: {
            ...base.perFile,
            [ev.file]: {
              ...(base.perFile[ev.file] ?? { downloaded: 0, total: null, done: false }),
              downloaded: ev.downloaded,
              total: ev.total ?? base.perFile[ev.file]?.total ?? null,
            },
          },
        };
      case 'file_done':
        return {
          ...base,
          perFile: {
            ...base.perFile,
            [ev.file]: {
              ...(base.perFile[ev.file] ?? { downloaded: 0, total: null, done: false }),
              done: true,
            },
          },
        };
      case 'done':
        return { ...base, done: true, dir: ev.dir };
      case 'error':
        return { ...base, error: ev.message };
      default:
        return base;
    }
  }

  function fmtBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
    return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  const fetchConfig = async () => {
    setLoading(true);
    try {
      const r = await fetch('/api/embedding-config');
      const d: EmbeddingConfig = await r.json();
      form.setFieldsValue(d);
      setProvider(d.provider ?? 'none');
    } catch {
      setStatus({ msg: 'Failed to load embedding configuration', type: 'error' });
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchConfig();
    fetchFeatures();
    fetchModels();
  }, []);

  const handleProviderChange = (p: string) => {
    setProvider(p);
    const defaults = PROVIDER_DEFAULTS[p] ?? {};
    form.setFieldsValue({ provider: p, ...defaults, dimensions: null });
    setStatus({ msg: '', type: '' });
  };

  const onFinish = async (values: EmbeddingConfig) => {
    setSaving(true);
    try {
      const r = await fetch('/api/embedding-config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(values),
      });
      if (!r.ok) throw new Error('Save failed');
      setStatus({ msg: 'Configuration saved. Restart the daemon to apply changes.', type: 'success' });
    } catch {
      setStatus({ msg: 'Failed to save configuration', type: 'error' });
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return <div style={{ textAlign: 'center', padding: 40 }}><Spin size="large" /></div>;
  }

  const needsKey = provider === 'openai' || provider === 'openrouter';
  const needsUrl = provider === 'openai' || provider === 'openrouter' || provider === 'ollama';
  const isLocal = provider === 'local';
  const isNone = provider === 'none';

  return (
    <div style={{ maxWidth: 720 }}>
      <div style={{ marginBottom: 28 }}>
        <Title level={4} style={{ margin: 0 }}>Embedding Provider</Title>
        <Text type="secondary">
          Configure vector embeddings for semantic memory search. Changes take effect on restart.
        </Text>
      </div>

      {provider !== 'none' && (
        <Card
          style={{ borderRadius: 12, marginBottom: 20, background: 'rgba(91,191,232,0.06)', border: '1px solid rgba(91,191,232,0.2)' }}
          styles={{ body: { padding: '12px 18px' } }}
        >
          <Space>
            <DatabaseOutlined style={{ color: '#5BBFE8' }} />
            <Text style={{ fontSize: 13 }}>{PROVIDER_DESCRIPTIONS[provider]}</Text>
            {isLocal && <Tag color="green" style={{ fontSize: 10 }}>On-device</Tag>}
          </Space>
        </Card>
      )}

      <Form form={form} layout="vertical" onFinish={onFinish} initialValues={{ provider: 'none' }}>
        <Form.Item name="provider" label="Provider" rules={[{ required: true }]}>
          <Select onChange={handleProviderChange} style={{ width: '100%' }}>
            {Object.entries(PROVIDER_LABELS).map(([k, v]) => (
              <Option key={k} value={k}>{v}</Option>
            ))}
          </Select>
        </Form.Item>

        {!isNone && (
          <>
            {needsUrl && (
              <Form.Item name="baseURL" label="Base URL" rules={[{ required: true }]}>
                <Input placeholder={provider === 'ollama' ? 'http://localhost:11434' : 'https://api.openai.com/v1'} />
              </Form.Item>
            )}

            {needsKey && (
              <Form.Item name="apiKey" label="API Key" rules={[{ required: true }]}>
                <Input.Password placeholder="Enter your API key" />
              </Form.Item>
            )}

            {isLocal ? (
              <>
                <Form.Item name="modelName" label="Model" rules={[{ required: true }]}>
                  <Select
                    placeholder="Select a model"
                    // Use option-label-with-tag pattern; allow filter by typing.
                    optionLabelProp="label"
                    showSearch
                    filterOption={(input, opt) =>
                      String(opt?.value ?? '').toLowerCase().includes(input.toLowerCase())
                    }
                  >
                    {/* Backend-driven list — if the backend hasn't responded
                        yet, fall back to the static catalog so the dropdown
                        is never empty. */}
                    {(backendModels.length > 0
                      ? backendModels.map(m => ({
                          value: m.id,
                          label: `${m.id} (${m.dimensions}d, ${m.size_hint})`,
                          installed: m.installed,
                          repo: m.repo,
                        }))
                      : LOCAL_MODEL_OPTIONS.map(m => ({
                          value: m.value,
                          label: m.label,
                          installed: false,
                          repo: '',
                        }))
                    ).map(m => (
                      <Option key={m.value} value={m.value} label={m.label}>
                        <div
                          style={{
                            display: 'flex',
                            justifyContent: 'space-between',
                            alignItems: 'center',
                            gap: 8,
                          }}
                        >
                          <span>{m.label}</span>
                          {m.installed && (
                            <Tag
                              color="green"
                              icon={<CheckCircleOutlined />}
                              style={{ fontSize: 10, margin: 0 }}
                            >
                              Downloaded
                            </Tag>
                          )}
                        </div>
                      </Option>
                    ))}
                  </Select>
                </Form.Item>

                {/* Download button + live progress preview. */}
                <Form.Item
                  label="Model files"
                  help={(() => {
                    // Prefer the on-disk path of the selected model when
                    // it's installed — gives users an immediate "yes, this
                    // is on disk" anchor that matches the dropdown badge.
                    const selectedId = watchedModelName;
                    const found = backendModels.find(m => m.id === selectedId);
                    if (found?.installed) return `Installed at ${found.on_disk_path}`;
                    return features
                      ? `Cache directory: ${features.models_dir}`
                      : 'Cache directory: ~/.senclaw/models/';
                  })()}
                >
                  <Space wrap>
                    <Button
                      icon={<CloudDownloadOutlined />}
                      loading={downloading}
                      onClick={downloadModel}
                      disabled={features ? !features.candle : false}
                    >
                      {(() => {
                        // Re-download is still allowed; just label honestly
                        // so the user knows the click means "fetch again".
                        const selectedId = form.getFieldValue('modelName');
                        const found = backendModels.find(m => m.id === selectedId);
                        return found?.installed
                          ? 'Re-download from HuggingFace'
                          : 'Download from HuggingFace';
                      })()}
                    </Button>
                    {(() => {
                      const selectedId = form.getFieldValue('modelName');
                      const found = backendModels.find(m => m.id === selectedId);
                      return found?.installed ? (
                        <Tag color="green" icon={<CheckCircleOutlined />} style={{ fontSize: 11 }}>
                          Already downloaded
                        </Tag>
                      ) : null;
                    })()}
                    {features && !features.candle && (
                      <Tag color="orange">
                        Rebuild with <code>--features local-embed</code> to enable downloads
                      </Tag>
                    )}
                    {features?.mlx_static && (
                      <Tag color="cyan" icon={<ThunderboltOutlined />}>
                        MLX-native available — set SENCLAW_LOCAL_EMBED_BACKEND=mlx
                      </Tag>
                    )}
                  </Space>

                  {/* Live progress card — appears as soon as a download starts. */}
                  {progress && (
                    <Card
                      size="small"
                      style={{
                        marginTop: 12,
                        borderRadius: 10,
                        borderColor: progress.error
                          ? '#ff7875'
                          : progress.done
                          ? '#52c41a'
                          : 'rgba(91,191,232,0.5)',
                        background: progress.error
                          ? 'rgba(255,77,79,0.05)'
                          : progress.done
                          ? 'rgba(82,196,26,0.05)'
                          : 'rgba(91,191,232,0.04)',
                      }}
                      styles={{ body: { padding: '12px 16px' } }}
                    >
                      <Space direction="vertical" size={6} style={{ width: '100%' }}>
                        <Space>
                          {progress.error ? (
                            <CloseCircleOutlined style={{ color: '#ff4d4f' }} />
                          ) : progress.done ? (
                            <CheckCircleOutlined style={{ color: '#52c41a' }} />
                          ) : (
                            <Spin size="small" />
                          )}
                          <Text strong style={{ fontSize: 13 }}>
                            {progress.error
                              ? 'Download failed'
                              : progress.done
                              ? 'Download complete'
                              : `Downloading ${progress.repo || form.getFieldValue('modelName')}`}
                          </Text>
                        </Space>

                        {progress.error && (
                          <Alert
                            type="error"
                            message={progress.error}
                            style={{ marginTop: 4 }}
                          />
                        )}

                        {progress.done && progress.dir && (
                          <Text type="success" style={{ fontSize: 12 }}>
                            Saved to <code>{progress.dir}</code>
                          </Text>
                        )}

                        {/* Per-file progress bars — ordered same as backend `files`. */}
                        {progress.files.map(file => {
                          const pf = progress.perFile[file] ?? {
                            downloaded: 0,
                            total: null as number | null,
                            done: false,
                          };
                          const percent = pf.total
                            ? Math.min(100, Math.round((pf.downloaded / pf.total) * 100))
                            : pf.done
                            ? 100
                            : 0;
                          const status: 'normal' | 'active' | 'success' = pf.done
                            ? 'success'
                            : progress.currentFile === file
                            ? 'active'
                            : 'normal';
                          return (
                            <div key={file}>
                              <div
                                style={{
                                  display: 'flex',
                                  justifyContent: 'space-between',
                                  fontSize: 11,
                                  marginBottom: 2,
                                  opacity: pf.done || progress.currentFile === file ? 1 : 0.5,
                                }}
                              >
                                <span>{file}</span>
                                <span>
                                  {fmtBytes(pf.downloaded)}
                                  {pf.total ? ` / ${fmtBytes(pf.total)}` : ''}
                                </span>
                              </div>
                              <Progress
                                size="small"
                                percent={percent}
                                status={status}
                                showInfo={false}
                              />
                            </div>
                          );
                        })}
                      </Space>
                    </Card>
                  )}
                </Form.Item>

                <Form.Item
                  name="modelPath"
                  label="Custom model path (optional)"
                  help="Leave blank to auto-download. Set if your weights live elsewhere."
                >
                  <Input placeholder="/path/to/model/directory" />
                </Form.Item>
              </>
            ) : (
              <Form.Item name="modelName" label="Model name" rules={[{ required: true }]}>
                <Input placeholder={
                  provider === 'openai' ? 'text-embedding-3-small' :
                  provider === 'openrouter' ? 'openai/text-embedding-3-small' :
                  'nomic-embed-text'
                } />
              </Form.Item>
            )}

            <Form.Item
              name="dimensions"
              label="Dimensions (optional)"
              help="Leave blank to use the provider default. Only set if using a custom model."
            >
              <InputNumber style={{ width: 160 }} min={64} max={4096} placeholder="auto" />
            </Form.Item>
          </>
        )}

        {status.msg && (
          <Alert message={status.msg} type={status.type as any} showIcon style={{ marginBottom: 20 }} />
        )}

        <Divider />

        <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
          <Button
            type="primary"
            htmlType="submit"
            icon={<SaveOutlined />}
            loading={saving}
            style={{ borderRadius: 8, height: 40, paddingInline: 24 }}
          >
            Save
          </Button>
        </div>
      </Form>
    </div>
  );
};
