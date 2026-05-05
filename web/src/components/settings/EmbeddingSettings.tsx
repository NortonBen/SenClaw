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
} from 'antd';
import { SaveOutlined, DatabaseOutlined } from '@ant-design/icons';

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

export const EmbeddingSettings: React.FC = () => {
  const [form] = Form.useForm();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [provider, setProvider] = useState<string>('none');
  const [status, setStatus] = useState<{ msg: string; type: 'success' | 'error' | 'info' | '' }>({ msg: '', type: '' });

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

  useEffect(() => { fetchConfig(); }, []);

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
                  <Select placeholder="Select a model">
                    {LOCAL_MODEL_OPTIONS.map(m => (
                      <Option key={m.value} value={m.value}>{m.label}</Option>
                    ))}
                  </Select>
                </Form.Item>
                <Form.Item
                  name="modelPath"
                  label="Custom model path (optional)"
                  help="Leave blank to auto-download from HuggingFace into ~/.senclaw/models/"
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
