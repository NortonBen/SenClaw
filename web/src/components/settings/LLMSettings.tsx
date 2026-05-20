import React, { useState, useEffect } from 'react';
import { 
  Typography, 
  Button, 
  Card, 
  Space, 
  Table, 
  Tag, 
  Modal, 
  Form, 
  Input, 
  Select, 
  Popconfirm, 
  message,
  Tooltip,
  Switch,
  InputNumber,
  Divider,
  Alert,
  Spin,
  Checkbox
} from 'antd';
import { 
  PlusOutlined, 
  DeleteOutlined, 
  GlobalOutlined,
  ApiOutlined,
  CheckCircleOutlined,
  EyeOutlined,
  EyeInvisibleOutlined,
  ReloadOutlined
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;
const { Option } = Select;

// ===== Types & Constants =====

interface LLMConfig {
  id: string;
  label: string;
  provider: string;
  baseURL: string;
  apiKey: string;
  modelName: string;
  adapt: 'openai' | 'anthropic';
  maxTokens: number;
  contextLength: number;
  /** Explicitly declare whether vision input is supported; undefined = auto-infer from modelName */
  vision?: boolean;
}

interface ProviderDef {
  name: string;
  baseURL: string;
  modelsUrl?: string;
  baseURLPlaceholder?: string;
  apiKeyPlaceholder?: string;
  defaultAdapt: 'openai' | 'anthropic';
  defaultMaxTokens?: number;
  defaultContextLength?: number;
}

const PROVIDERS: Record<string, ProviderDef> = {
  anthropic:  { name: 'Anthropic',          baseURL: 'https://api.anthropic.com',                         defaultAdapt: 'anthropic', apiKeyPlaceholder: 'Your Anthropic API key' },
  openai:     { name: 'OpenAI',             baseURL: 'https://api.openai.com/v1',                         defaultAdapt: 'openai',    apiKeyPlaceholder: 'Your OpenAI API key' },
  kimi:       { name: 'Kimi (Moonshot)',     baseURL: 'https://api.moonshot.cn/v1',                        defaultAdapt: 'openai',    apiKeyPlaceholder: 'Your Moonshot API key' },
  minimax:    { name: 'MiniMax',            baseURL: 'https://api.minimaxi.com/anthropic',                 defaultAdapt: 'anthropic', apiKeyPlaceholder: 'Your MiniMax API key' },
  deepseek:   { name: 'DeepSeek',           baseURL: 'https://api.deepseek.com/anthropic', modelsUrl: 'https://api.deepseek.com/v1',        defaultAdapt: 'anthropic', apiKeyPlaceholder: 'Your DeepSeek API key' },
  glm:        { name: 'GLM (Zhipu)',          baseURL: 'https://open.bigmodel.cn/api/paas/v4',              defaultAdapt: 'openai',    apiKeyPlaceholder: 'Your Zhipu API key' },
  openrouter: { name: 'OpenRouter',         baseURL: 'https://openrouter.ai/api',          modelsUrl: 'https://openrouter.ai/api/v1',       defaultAdapt: 'openai', apiKeyPlaceholder: 'Your OpenRouter API key' },
  qwen:       { name: 'Qwen (Alibaba)',      baseURL: 'https://dashscope.aliyuncs.com/compatible-mode/v1', defaultAdapt: 'openai',   apiKeyPlaceholder: 'Your Alibaba Cloud API key' },
  custom:     { name: 'Custom LLM endpoint',    baseURL: '',                                                   defaultAdapt: 'openai',    baseURLPlaceholder: 'https://your-api.com/v1', apiKeyPlaceholder: 'Your API key' },
};

const PROVIDER_ORDER = ['anthropic','openai','kimi','minimax','deepseek','glm','openrouter','qwen','custom'];

const MODEL_LIMITS_TABLE: Array<[string, { maxTokens: number; contextLength: number }]> = [
  ['claude-opus-4',       { maxTokens: 32000,   contextLength: 200000  }],
  ['claude-sonnet-4',     { maxTokens: 64000,   contextLength: 200000  }],
  ['claude-haiku-4',      { maxTokens: 16000,   contextLength: 200000  }],
  ['claude-3-7-sonnet',   { maxTokens: 64000,   contextLength: 200000  }],
  ['claude-3-5-sonnet',   { maxTokens: 8192,    contextLength: 200000  }],
  ['claude-3-5-haiku',    { maxTokens: 8192,    contextLength: 200000  }],
  ['claude-3-opus',       { maxTokens: 4096,    contextLength: 200000  }],
  ['claude-3-sonnet',     { maxTokens: 4096,    contextLength: 200000  }],
  ['claude-3-haiku',      { maxTokens: 4096,    contextLength: 200000  }],
  ['o3-mini',             { maxTokens: 65536,   contextLength: 200000  }],
  ['o3',                  { maxTokens: 100000,  contextLength: 200000  }],
  ['o1-mini',             { maxTokens: 65536,   contextLength: 128000  }],
  ['o1',                  { maxTokens: 32768,   contextLength: 200000  }],
  ['gpt-4o-mini',         { maxTokens: 16384,   contextLength: 128000  }],
  ['gpt-4o',              { maxTokens: 16384,   contextLength: 128000  }],
  ['gpt-4-turbo',         { maxTokens: 4096,    contextLength: 128000  }],
  ['gpt-4',               { maxTokens: 8192,    contextLength: 8192    }],
  ['gpt-3.5-turbo',       { maxTokens: 4096,    contextLength: 16384   }],
  ['deepseek-r1',         { maxTokens: 32000,   contextLength: 64000   }],
  ['deepseek-v3',         { maxTokens: 32000,   contextLength: 64000   }],
  ['deepseek-chat',       { maxTokens: 8192,    contextLength: 64000   }],
  ['deepseek-reasoner',   { maxTokens: 8192,    contextLength: 64000   }],
  ['deepseek-coder',      { maxTokens: 8192,    contextLength: 16000   }],
  ['kimi-k2',             { maxTokens: 32000,   contextLength: 131072  }],
  ['moonshot-v1-128k',    { maxTokens: 8192,    contextLength: 128000  }],
  ['moonshot-v1-32k',     { maxTokens: 8192,    contextLength: 32000   }],
  ['moonshot-v1-8k',      { maxTokens: 8192,    contextLength: 8000    }],
  ['minimax-m1',          { maxTokens: 40960,   contextLength: 1000000 }],
  ['abab6.5',             { maxTokens: 8192,    contextLength: 245760  }],
  ['glm-4-long',          { maxTokens: 8192,    contextLength: 1000000 }],
  ['glm-4-flash',         { maxTokens: 8192,    contextLength: 128000  }],
  ['glm-4',               { maxTokens: 8192,    contextLength: 128000  }],
  ['glm-z1',              { maxTokens: 32768,   contextLength: 32768   }],
  ['qwen3',               { maxTokens: 32768,   contextLength: 32768   }],
  ['qwen-long',           { maxTokens: 8192,    contextLength: 1000000 }],
  ['qwen-max',            { maxTokens: 8192,    contextLength: 32000   }],
  ['qwen-plus',           { maxTokens: 8192,    contextLength: 131072  }],
  ['qwen-turbo',          { maxTokens: 8192,    contextLength: 131072  }],
  ['qwq',                 { maxTokens: 32768,   contextLength: 131072  }],
  ['gemini-2.5-pro',      { maxTokens: 65536,   contextLength: 1000000 }],
  ['gemini-2.5-flash',    { maxTokens: 65536,   contextLength: 1000000 }],
  ['gemini-2.0-flash',    { maxTokens: 8192,    contextLength: 1000000 }],
  ['gemini-1.5-pro',      { maxTokens: 8192,    contextLength: 1000000 }],
  ['gemini-1.5-flash',    { maxTokens: 8192,    contextLength: 1000000 }],
  ['llama-3.3',           { maxTokens: 32768,   contextLength: 131072  }],
  ['llama-3.1',           { maxTokens: 32768,   contextLength: 131072  }],
  ['llama-3',             { maxTokens: 8192,    contextLength: 8192    }],
];

function lookupModelLimits(modelName: string): { maxTokens: number; contextLength: number } | null {
  const lower = modelName.toLowerCase();
  for (const [prefix, limits] of MODEL_LIMITS_TABLE) {
    if (lower.startsWith(prefix)) return limits;
  }
  return null;
}

// ===== Vision Support =====

// Vision patterns matching sema-core util/vision.ts inference rules
const VISION_PATTERNS: RegExp[] = [
  /^gpt-4o/i,
  /^gpt-4(\.\d+)?-vision/i,
  /^gpt-5/i,
  /^o1/i,
  /^o3/i,
  /^chatgpt-4o/i,
  /^claude-3/i,
  /^claude-(opus|sonnet|haiku)-[34]/i,
  /^anthropic\/claude-3/i,
  /qwen.*-vl/i,
  /qwen2(\.\d+)?-vl/i,
  /qvq/i,
  /moonshot-v1-.*-vision/i,
  /kimi.*vision/i,
  /kimi-latest/i,
  /glm-4v/i,
  /glm-4\.\d+v/i,
  /deepseek-vl/i,
  /gemini.*pro/i,
  /gemini.*flash/i,
  /gemini-1\.5/i,
  /gemini-2/i,
  /llama-3\.2.*vision/i,
  /-vl-/i,
  /-vision/i,
  /-vlm/i,
];

function inferVision(modelName: string): boolean {
  if (!modelName) return false;
  return VISION_PATTERNS.some(re => re.test(modelName));
}

/** Effective vision state: explicit override takes priority, otherwise infer from modelName */
function effectiveVision(c: { vision?: boolean; modelName: string }): boolean {
  if (typeof c.vision === 'boolean') return c.vision;
  return inferVision(c.modelName);
}

export const LLMSettings: React.FC = () => {
  const [configs, setConfigs] = useState<LLMConfig[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [activeQuickId, setActiveQuickId] = useState<string | null>(null);
  const [activeCognitiveId, setActiveCognitiveId] = useState<string | null>(null);
  const [semaModel, setSemaModel] = useState<{ modelName: string; provider: string } | null>(null);
  const [semaQuickModel, setSemaQuickModel] = useState<{ modelName: string; provider: string } | null>(null);
  const [thinkingEnabled, setThinkingEnabled] = useState(true);
  const [loading, setLoading] = useState(true);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [form] = Form.useForm();

  // State for fetching model list
  const [fetchingModels, setFetchingModels] = useState(false);
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [isManualModel, setIsManualModel] = useState(false);
  const [testStatus, setTestStatus] = useState<{ msg: string; type: 'success' | 'error' | 'info' | '' }>({ msg: '', type: '' });
  const [saving, setSaving] = useState(false);
  const [connOk, setConnOk] = useState(false);
  // Vision toggle: null = follow inference; true/false = explicit user override
  const [visionOverride, setVisionOverride] = useState<boolean | null>(null);

  const fetchConfigs = async () => {
    setLoading(true);
    try {
      const r = await fetch('/api/llm-config');
      const d = await r.json();
      if (Array.isArray(d)) {
        setConfigs(d);
      } else {
        setConfigs(d.configs ?? d.data ?? []);
        setActiveId(d.activeId ?? null);
        setActiveQuickId(d.activeQuickId ?? null);
        setActiveCognitiveId(d.activeCognitiveId ?? null);
        setSemaModel(d.semaModel ?? null);
        setSemaQuickModel(d.semaQuickModel ?? null);
        setThinkingEnabled(d.thinkingEnabled ?? true);
      }
    } catch (e) {
      message.error('Failed to load LLM configurations');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchConfigs();
  }, []);

  const handleProviderChange = (p: string) => {
    const def = PROVIDERS[p];
    form.setFieldsValue({
      baseURL: def.baseURL,
      adapt: def.defaultAdapt,
      apiKey: '',
      modelName: '',
      maxTokens: def.defaultMaxTokens ?? 8192,
      contextLength: def.defaultContextLength ?? 128000
    });
    setAvailableModels([]);
    setIsManualModel(false);
    setConnOk(false);
    setTestStatus({ msg: '', type: '' });
    setVisionOverride(null);
  };

  const handleFetchModels = async () => {
    const values = form.getFieldsValue();
    if (!values.baseURL || !values.apiKey) {
      message.warning('Base URL and API key are required to fetch models');
      return;
    }
    
    setFetchingModels(true);
    setTestStatus({ msg: 'Fetching model list...', type: 'info' });
    
    const providerKey = form.getFieldValue('provider');
    const fetchBaseURL = PROVIDERS[providerKey]?.modelsUrl ?? values.baseURL;
    
    try {
      const r = await fetch('/api/llm-config/models', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ baseURL: fetchBaseURL, apiKey: values.apiKey, adapt: values.adapt }),
      });
      const data = await r.json() as { success: boolean; models?: string[]; message?: string };
      if (data.success && data.models?.length) {
        setAvailableModels(data.models);
        form.setFieldsValue({ modelName: data.models[0] });
        setTestStatus({ msg: `Loaded ${data.models.length} model(s)`, type: 'success' });
      } else {
        setTestStatus({ msg: data.message ?? 'Failed to fetch models', type: 'error' });
      }
    } catch (e) {
      setTestStatus({ msg: 'Network error fetching models', type: 'error' });
    } finally {
      setFetchingModels(false);
    }
  };

  const handleTestConnection = async () => {
    const values = form.getFieldsValue();
    if (!values.baseURL || !values.apiKey) {
      message.warning('Base URL and API key are required to test connection');
      return;
    }
    
    setTestStatus({ msg: 'Testing connection...', type: 'info' });
    const providerKey = form.getFieldValue('provider');
    const testBaseURL = PROVIDERS[providerKey]?.modelsUrl ?? values.baseURL;
    
    try {
      const r = await fetch('/api/llm-config/test', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ baseURL: testBaseURL, apiKey: values.apiKey, adapt: values.adapt }),
      });
      const data = await r.json() as { success: boolean; message?: string };
      setConnOk(data.success);
      setTestStatus({ 
        msg: data.success ? 'Connection successful!' : (data.message ?? 'Connection failed'), 
        type: data.success ? 'success' : 'error' 
      });
    } catch (e) {
      setConnOk(false);
      setTestStatus({ msg: 'Network error testing connection', type: 'error' });
    }
  };

  const onFinish = async (values: any) => {
    if (!connOk) {
      message.warning('Please test connection successfully before saving');
      return;
    }
    
    setSaving(true);
    const providerName = PROVIDERS[values.provider]?.name ?? values.provider;
    const label = `${values.modelName} (${providerName})`;
    
    try {
      const r = await fetch('/api/llm-config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...values, label, vision: visionOverride }),
      });
      if (!r.ok) throw new Error('Save failed');
      message.success('Model added successfully');
      setIsModalOpen(false);
      fetchConfigs();
    } catch (e) {
      message.error('Failed to save LLM configuration');
    } finally {
      setSaving(false);
    }
  };

  const handleSetMain = async (id: string) => {
    await fetch('/api/llm-config/active', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, type: 'main' }),
    });
    setActiveId(id);
    const cfg = configs.find(c => c.id === id);
    if (cfg) setSemaModel({ modelName: cfg.modelName, provider: cfg.provider });
  };

  const handleSetQuick = async (id: string) => {
    await fetch('/api/llm-config/active', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, type: 'quick' }),
    });
    setActiveQuickId(id);
    const cfg = configs.find(c => c.id === id);
    if (cfg) setSemaQuickModel({ modelName: cfg.modelName, provider: cfg.provider });
  };

  /** Pick the LLM used by cognitive memory's cognify pipeline. */
  const handleSetCognitive = async (id: string) => {
    await fetch('/api/llm-config/active', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, type: 'cognitive' }),
    });
    setActiveCognitiveId(id);
  };

  const handleThinkingToggle = async () => {
    const next = !thinkingEnabled;
    setThinkingEnabled(next);
    await fetch('/api/thinking', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled: next }),
    });
  };

  const handleDelete = async (id: string) => {
    try {
      const r = await fetch(`/api/llm-config/${encodeURIComponent(id)}`, { method: 'DELETE' });
      if (!r.ok) throw new Error('Delete failed');
      message.success('Model removed');
      fetchConfigs();
    } catch (e) {
      message.error('Failed to remove model');
    }
  };

  const handleToggleVision = async (record: LLMConfig) => {
    const next = !effectiveVision(record);
    const inferred = inferVision(record.modelName);
    // Reset to null if matches inferred value, otherwise set explicit override
    const visionPatch: { vision: boolean | null } = { vision: next === inferred ? null : next };
    
    try {
      const r = await fetch(`/api/llm-config/${encodeURIComponent(record.id)}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(visionPatch),
      });
      if (!r.ok) throw new Error('Update failed');
      message.success('Vision setting updated');
      fetchConfigs();
    } catch (e) {
      message.error('Failed to update vision setting');
    }
  };

  const activeConfig = configs.find(c => c.id === activeId);
  const activeQuickConfig = configs.find(c => c.id === activeQuickId);
  const activeCognitiveConfig = configs.find(c => c.id === activeCognitiveId);
  const displayCognitive = activeCognitiveConfig
    ? {
        modelName: activeCognitiveConfig.modelName,
        providerLabel:
          PROVIDERS[activeCognitiveConfig.provider]?.name ?? activeCognitiveConfig.provider,
      }
    : null;

  const displayMain = activeConfig
    ? { modelName: activeConfig.modelName, providerLabel: PROVIDERS[activeConfig.provider]?.name ?? activeConfig.provider }
    : semaModel
    ? { modelName: semaModel.modelName, providerLabel: PROVIDERS[semaModel.provider]?.name ?? semaModel.provider }
    : null;

  const displayQuick = activeQuickConfig
    ? { modelName: activeQuickConfig.modelName, providerLabel: PROVIDERS[activeQuickConfig.provider]?.name ?? activeQuickConfig.provider }
    : semaQuickModel
    ? { modelName: semaQuickModel.modelName, providerLabel: PROVIDERS[semaQuickModel.provider]?.name ?? semaQuickModel.provider }
    : displayMain;

  const columns = [
    {
      title: 'Model',
      key: 'model',
      render: (_: any, record: LLMConfig) => {
        const isMain  = record.id === activeId;
        const isQuick = record.id === activeQuickId;
        return (
          <Space>
            <div style={{
              width: 32, height: 32, borderRadius: 8, backgroundColor: '#f0f0f0',
              display: 'flex', alignItems: 'center', justifyContent: 'center'
            }}>
              <ApiOutlined />
            </div>
            <div>
              <Space size={4}>
                <Text strong>{record.modelName}</Text>
                {isMain  && <Tag color="blue"  style={{ fontSize: 10, padding: '0 4px' }}>Main</Tag>}
                {isQuick && <Tag color="purple" style={{ fontSize: 10, padding: '0 4px' }}>Quick</Tag>}
              </Space>
              <br />
              <Text type="secondary" style={{ fontSize: 12 }}>{PROVIDERS[record.provider]?.name || record.provider}</Text>
            </div>
          </Space>
        );
      },
    },
    {
      title: 'Provider',
      dataIndex: 'baseURL',
      key: 'baseURL',
      render: (url: string) => (
        <Space>
          <GlobalOutlined style={{ color: '#bfbfbf' }} />
          <Text type="secondary" style={{ fontSize: 13 }}>{url}</Text>
        </Space>
      ),
    },
    {
      title: 'Limits',
      key: 'limits',
      render: (_: any, record: LLMConfig) => (
        <Space orientation="vertical" size={0}>
          <Text style={{ fontSize: 12 }}>Ctx: {record.contextLength.toLocaleString()}</Text>
          <Text style={{ fontSize: 12 }}>Max: {record.maxTokens.toLocaleString()}</Text>
        </Space>
      ),
    },
    {
      title: 'Vision',
      key: 'vision',
      render: (_: any, record: LLMConfig) => {
        const visionOn = effectiveVision(record);
        const inferred = inferVision(record.modelName);
        const visionExplicit = typeof record.vision === 'boolean';
        const visionLabel = visionExplicit
          ? (visionOn ? 'Force ON' : 'Force OFF')
          : `Auto (${inferred ? 'Yes' : 'No'})`;
        return (
          <Space size="small">
            <Switch
              size="small"
              checked={visionOn}
              onChange={() => handleToggleVision(record)}
            />
            <Text style={{ fontSize: 10, color: '#8c8c8c' }}>{visionLabel}</Text>
            {visionExplicit && (
              <Button
                size="small"
                type="link"
                style={{ fontSize: 10, padding: 0, height: 'auto', marginLeft: 4 }}
                onClick={async () => {
                  try {
                    await fetch(`/api/llm-config/${encodeURIComponent(record.id)}`, {
                      method: 'PATCH',
                      headers: { 'Content-Type': 'application/json' },
                      body: JSON.stringify({ vision: null }),
                    });
                    message.success('Vision reset to auto-infer');
                    fetchConfigs();
                  } catch {
                    message.error('Failed to reset vision setting');
                  }
                }}
              >
                Reset
              </Button>
            )}
          </Space>
        );
      },
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: any, record: LLMConfig) => {
        const isMain  = record.id === activeId;
        const isQuick = record.id === activeQuickId;
        const isCog   = record.id === activeCognitiveId;
        return (
          <Space>
            <Tooltip title={isMain ? 'Current main model' : 'Set as Main Model'}>
              <Button
                size="small"
                type={isMain ? 'primary' : 'default'}
                disabled={isMain}
                onClick={() => handleSetMain(record.id)}
                style={{ fontSize: 11 }}
              >
                {isMain ? '● Main' : 'Main'}
              </Button>
            </Tooltip>
            <Tooltip title={isQuick ? 'Current quick model' : 'Set as Quick Model'}>
              <Button
                size="small"
                type={isQuick ? 'primary' : 'default'}
                disabled={isQuick}
                onClick={() => handleSetQuick(record.id)}
                style={{ fontSize: 11, ...(isQuick ? { background: '#7c3aed', borderColor: '#7c3aed' } : {}) }}
              >
                {isQuick ? '● Quick' : 'Quick'}
              </Button>
            </Tooltip>
            <Tooltip title={isCog ? 'Current cognitive model (cog_cognify)' : 'Set as Cognitive Model (used by cog_cognify)'}>
              <Button
                size="small"
                type={isCog ? 'primary' : 'default'}
                disabled={isCog}
                onClick={() => handleSetCognitive(record.id)}
                style={{ fontSize: 11, ...(isCog ? { background: '#10B981', borderColor: '#10B981' } : {}) }}
              >
                {isCog ? '● Cognitive' : 'Cognitive'}
              </Button>
            </Tooltip>
            <Popconfirm
              title="Remove this model?"
              onConfirm={() => handleDelete(record.id)}
              okText="Yes"
              cancelText="No"
              okButtonProps={{ danger: true }}
            >
              <Button type="text" danger icon={<DeleteOutlined />} />
            </Popconfirm>
          </Space>
        );
      },
    },
  ];

  if (loading && configs.length === 0) {
    return <div style={{ textAlign: 'center', padding: '40px' }}><Spin size="large" /></div>;
  }

  return (
    <div style={{ maxWidth: 1000 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 32 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>LLM Models</Title>
          <Text type="secondary">Configure and test your Large Language Model providers.</Text>
        </div>
        <Button 
          type="primary" 
          icon={<PlusOutlined />} 
          onClick={() => {
            setIsModalOpen(true);
            form.resetFields();
            handleProviderChange('anthropic');
            setVisionOverride(null);
          }}
          style={{ borderRadius: 8, height: 40 }}
        >
          Add Model
        </Button>
      </div>

      {(displayMain || displayQuick) && (
        <Card style={{ borderRadius: 12, marginBottom: 16 }} styles={{ body: { padding: '16px 20px' } }}>
          <Text type="secondary" style={{ fontSize: 11, fontWeight: 600, textTransform: 'uppercase', letterSpacing: 1 }}>
            Current Models in Use
          </Text>
          <div style={{ marginTop: 12, display: 'flex', flexDirection: 'column', gap: 10 }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <div style={{ width: 8, height: 8, borderRadius: '50%', background: '#5BBFE8', flexShrink: 0 }} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <Text strong style={{ fontSize: 13 }}>{displayMain?.modelName ?? '—'}</Text>
                <Text type="secondary" style={{ fontSize: 11, marginLeft: 6 }}>{displayMain?.providerLabel}</Text>
              </div>
              <Tag color="blue" style={{ fontSize: 10 }}>Main Model</Tag>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <div style={{ width: 8, height: 8, borderRadius: '50%', background: '#7c3aed', flexShrink: 0 }} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <Text strong style={{ fontSize: 13 }}>{displayQuick?.modelName ?? '—'}</Text>
                <Text type="secondary" style={{ fontSize: 11, marginLeft: 6 }}>{displayQuick?.providerLabel}</Text>
              </div>
              <Tag color="purple" style={{ fontSize: 10 }}>Quick Model</Tag>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <div style={{ width: 8, height: 8, borderRadius: '50%', background: '#10B981', flexShrink: 0 }} />
              <div style={{ flex: 1, minWidth: 0 }}>
                <Text strong style={{ fontSize: 13 }}>{displayCognitive?.modelName ?? '— (not configured)'}</Text>
                <Text type="secondary" style={{ fontSize: 11, marginLeft: 6 }}>{displayCognitive?.providerLabel ?? 'cog_cognify will fail until set'}</Text>
              </div>
              <Tag color="green" style={{ fontSize: 10 }}>Cognitive Model</Tag>
            </div>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingTop: 8, borderTop: '1px solid rgba(0,0,0,0.06)' }}>
              <Text type="secondary" style={{ fontSize: 12 }}>Thinking</Text>
              <Switch size="small" checked={thinkingEnabled} onChange={handleThinkingToggle} />
            </div>
          </div>
        </Card>
      )}

      <Card
        style={{ borderRadius: 12, border: '1px solid #f0f0f0' }}
        styles={{ body: { padding: 0 } }}
      >
        <Table
          columns={columns}
          dataSource={configs}
          rowKey="id"
          pagination={false}
          locale={{ emptyText: 'No models configured yet.' }}
        />
      </Card>

      <Modal
        title="Add New LLM Model"
        open={isModalOpen}
        onCancel={() => setIsModalOpen(false)}
        footer={null}
        width={600}
        destroyOnClose
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={onFinish}
          style={{ marginTop: 24 }}
        >
          <Form.Item
            name="provider"
            label="Provider"
            rules={[{ required: true }]}
          >
            <Select onChange={handleProviderChange}>
              {PROVIDER_ORDER.map(p => (
                <Option key={p} value={p}>{PROVIDERS[p].name}</Option>
              ))}
            </Select>
          </Form.Item>

          <Form.Item
            name="baseURL"
            label="API Base URL"
            rules={[{ required: true }]}
          >
            <Input placeholder={PROVIDERS[form.getFieldValue('provider')]?.baseURLPlaceholder} />
          </Form.Item>

          <Form.Item
            name="apiKey"
            label="API Key"
            rules={[{ required: true }]}
          >
            <Input.Password placeholder="Enter your API key" />
          </Form.Item>

          <Form.Item label="Model Selection">
            <Space orientation="vertical" style={{ width: '100%' }}>
              <div style={{ display: 'flex', gap: '8px' }}>
                <Form.Item
                  name="modelName"
                  noStyle
                  rules={[{ required: true, message: 'Required' }]}
                >
                  {isManualModel || availableModels.length === 0 ? (
                    <Input placeholder="e.g. gpt-4o" style={{ flex: 1 }} />
                  ) : (
                    <Select placeholder="Select a model" style={{ flex: 1 }}>
                      {availableModels.map(m => <Option key={m} value={m}>{m}</Option>)}
                    </Select>
                  )}
                </Form.Item>
                <Button 
                  icon={<ReloadOutlined />} 
                  loading={fetchingModels} 
                  onClick={handleFetchModels}
                  disabled={!form.getFieldValue('apiKey')}
                >
                  Fetch
                </Button>
              </div>
              <Checkbox 
                checked={isManualModel} 
                onChange={(e: any) => setIsManualModel(e.target.checked)}
              >
                Enter model name manually
              </Checkbox>
            </Space>
          </Form.Item>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px' }}>
            <Form.Item name="maxTokens" label="Max Tokens">
              <InputNumber style={{ width: '100%' }} />
            </Form.Item>
            <Form.Item name="contextLength" label="Context Length">
              <InputNumber style={{ width: '100%' }} />
            </Form.Item>
          </div>

          <Form.Item name="adapt" label="Protocol Adaptation">
            <Select>
              <Option value="openai">OpenAI Compatible</Option>
              <Option value="anthropic">Anthropic Compatible</Option>
            </Select>
          </Form.Item>

          <Form.Item label="Vision Support">
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <div style={{ flex: 1, paddingRight: 16 }}>
                <Text style={{ fontSize: 11, color: '#8c8c8c' }}>
                  Enable image input (Vision)
                </Text>
                <Text style={{ fontSize: 10, color: '#bfbfbf', marginTop: 2 }}>
                  {visionOverride === null
                    ? `Auto-infer: ${inferVision(form.getFieldValue('modelName') || '') ? 'Supported' : 'Not supported'}`
                    : visionOverride
                      ? 'Force enabled: Images will be sent as-is'
                      : 'Force disabled: Images will be downgraded to placeholders'}
                </Text>
              </div>
              <Switch
                checked={visionOverride ?? inferVision(form.getFieldValue('modelName') || '')}
                onChange={(checked: boolean) => {
                  // When toggled, set explicit override; clear back to null if matches inferred value
                  const inferred = inferVision(form.getFieldValue('modelName') || '');
                  setVisionOverride(checked === inferred ? null : checked);
                }}
              />
            </div>
          </Form.Item>

          {testStatus.msg && (
            <Alert 
              message={testStatus.msg} 
              type={testStatus.type as any} 
              showIcon 
              style={{ marginBottom: 24 }} 
            />
          )}

          <Divider />

          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
            <Button 
              icon={<CheckCircleOutlined />} 
              onClick={handleTestConnection}
              disabled={!form.getFieldValue('apiKey')}
            >
              Test Connection
            </Button>
            <Space>
              <Button onClick={() => setIsModalOpen(false)}>Cancel</Button>
              <Button type="primary" htmlType="submit" loading={saving} disabled={!connOk}>
                Add Model
              </Button>
            </Space>
          </div>
        </Form>
      </Modal>
    </div>
  );
};
