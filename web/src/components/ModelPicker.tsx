import { useState, useEffect } from 'react';
import { Dropdown, Button, theme, Spin } from 'antd';
import { DownOutlined } from '@ant-design/icons';

interface LlmConfig {
  id: string;
  label: string;
  provider: string;
  modelName: string;
}

interface Props {
  modelId?: string | null;
  onChange: (modelId: string | null) => void;
}

export function ModelPicker({ modelId, onChange }: Props) {
  const { token } = theme.useToken();
  const [models, setModels] = useState<LlmConfig[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    setLoading(true);
    fetch('/api/llm-config')
      .then(r => r.json())
      .then((data) => {
        setModels(data.configs ?? []);
        setActiveId(data.activeId ?? null);
      })
      .catch(() => setModels([]))
      .finally(() => setLoading(false));
  }, []);

  // Strip provider noise + parenthetical qualifiers so the chip stays compact.
  // "Local Gemma 4 E2B-it 4-bit — text-only (...) (MLX) (default)" → "Gemma 4 E2B-it 4-bit"
  const shortLabel = (s?: string) => {
    if (!s) return '';
    return s
      .replace(/\([^)]*\)/g, '')        // drop "(MLX)", "(default)", etc.
      .replace(/^\s*Local\s+/i, '')     // drop leading "Local "
      .split(/[—–-]/)[0]                // keep text before em/en/hyphen dash separator
      .trim();
  };

  const selected = models.find(m => m.id === modelId);
  const activeDefault = models.find(m => m.id === activeId);
  const displayLabel = shortLabel(selected?.label)
    || (modelId ? shortLabel(modelId) : shortLabel(activeDefault?.label) || 'Default');

  // Dropdown items keep the full label so users can still identify exact variant.
  const items = [
    { key: '', label: activeDefault ? `Default · ${activeDefault.label}` : 'Default model' },
    ...models.map(m => ({ key: m.id, label: `${m.label} · ${m.modelName}` })),
  ];

  return (
    <Dropdown
      menu={{
        items,
        selectedKeys: [modelId ?? ''],
        onClick: ({ key }) => onChange(key || null),
      }}
      trigger={['click']}
      placement="topLeft"
    >
      <Button
        type="text"
        size="small"
        title={selected?.label ?? activeDefault?.label ?? 'Default model'}
        style={{
          color: token.colorTextSecondary,
          fontSize: 11,
          padding: '0 4px',
          maxWidth: 160,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          whiteSpace: 'nowrap',
          display: 'inline-flex',
          alignItems: 'center',
        }}
      >
        {loading ? (
          <Spin size="small" />
        ) : (
          <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
            {displayLabel}
          </span>
        )}
        <DownOutlined style={{ fontSize: 9, marginLeft: 3, flexShrink: 0 }} />
      </Button>
    </Dropdown>
  );
}
