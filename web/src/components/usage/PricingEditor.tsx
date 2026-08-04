import { useCallback, useEffect, useState } from 'react';
import { Button, Card, Input, InputNumber, Popconfirm, Space, Table, Typography, message } from 'antd';
import { DeleteOutlined, PlusOutlined, SaveOutlined } from '@ant-design/icons';

const { Text } = Typography;

const INK = 'rgba(255,255,255,0.85)';
const INK_2 = 'rgba(255,255,255,0.45)';
const CARD_STYLE = {
  background: 'rgba(13, 13, 31, 0.4)',
  borderColor: 'rgba(255,255,255,0.05)',
  borderRadius: '12px',
} as const;

interface PricingRow {
  model: string;
  inputPer1m: number;
  outputPer1m: number;
  cacheReadPer1m: number | null;
  cacheWritePer1m: number | null;
}

/** Draft row being edited: numbers may be temporarily empty. */
interface Draft {
  model: string;
  inputPer1m: number | null;
  outputPer1m: number | null;
  cacheReadPer1m: number | null;
  cacheWritePer1m: number | null;
  isNew?: boolean;
}

/** Admin table for `model_pricing` — the price card that turns raw token
 * counts into the $ figures everywhere else on this page. Prefix matching:
 * a row `claude-opus-5` also prices `claude-opus-5-20260101`. */
export function PricingEditor() {
  const [rows, setRows] = useState<PricingRow[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saving, setSaving] = useState(false);
  const [msg, msgCtx] = message.useMessage();

  const load = useCallback(async () => {
    try {
      const r = await fetch('/api/usage/pricing').then(r => r.json());
      setRows(r?.rows ?? []);
    } catch {
      setRows([]);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const save = async () => {
    if (!draft) return;
    const model = draft.model.trim();
    if (!model) {
      msg.warning('Model id is required');
      return;
    }
    if (draft.inputPer1m == null || draft.outputPer1m == null) {
      msg.warning('Input and output prices are required');
      return;
    }
    setSaving(true);
    try {
      const res = await fetch('/api/usage/pricing', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          model,
          inputPer1m: draft.inputPer1m,
          outputPer1m: draft.outputPer1m,
          cacheReadPer1m: draft.cacheReadPer1m,
          cacheWritePer1m: draft.cacheWritePer1m,
        }),
      });
      if (!res.ok) throw new Error(await res.text());
      msg.success(`Saved pricing for ${model}`);
      setDraft(null);
      await load();
    } catch (e) {
      msg.error(`Save failed: ${e}`);
    } finally {
      setSaving(false);
    }
  };

  const remove = async (model: string) => {
    try {
      const res = await fetch(`/api/usage/pricing/${encodeURIComponent(model)}`, {
        method: 'DELETE',
      });
      if (!res.ok) throw new Error(await res.text());
      msg.success(`Removed ${model}`);
      await load();
    } catch (e) {
      msg.error(`Delete failed: ${e}`);
    }
  };

  // Trim float artifacts (0.30000000000000004 → 0.3) without forcing a
  // fixed decimal count.
  const priceCell = (v: number | null | undefined) =>
    v == null ? (
      <Text style={{ color: INK_2 }}>—</Text>
    ) : (
      <Text style={{ color: INK }}>${parseFloat(v.toFixed(6))}</Text>
    );

  const numInput = (
    value: number | null,
    onChange: (v: number | null) => void,
    required = false,
  ) => (
    <InputNumber
      size="small"
      min={0}
      step={0.05}
      value={value}
      status={required && value == null ? 'error' : undefined}
      onChange={v => onChange(v == null ? null : Number(v))}
      style={{ width: 90 }}
    />
  );

  const editing = (m: string) => draft && !draft.isNew && draft.model === m;

  const columns = [
    {
      title: <Text style={{ color: INK_2 }}>Model (prefix match)</Text>,
      dataIndex: 'model',
      key: 'model',
      render: (v: string) => <Text style={{ color: INK }} code>{v}</Text>,
    },
    {
      title: <Text style={{ color: INK_2 }}>In $/1M</Text>,
      key: 'in',
      width: 110,
      render: (_: unknown, r: PricingRow) =>
        editing(r.model)
          ? numInput(draft!.inputPer1m, v => setDraft({ ...draft!, inputPer1m: v }), true)
          : priceCell(r.inputPer1m),
    },
    {
      title: <Text style={{ color: INK_2 }}>Out $/1M</Text>,
      key: 'out',
      width: 110,
      render: (_: unknown, r: PricingRow) =>
        editing(r.model)
          ? numInput(draft!.outputPer1m, v => setDraft({ ...draft!, outputPer1m: v }), true)
          : priceCell(r.outputPer1m),
    },
    {
      title: <Text style={{ color: INK_2 }}>Cache read $/1M</Text>,
      key: 'cr',
      width: 130,
      render: (_: unknown, r: PricingRow) =>
        editing(r.model)
          ? numInput(draft!.cacheReadPer1m, v => setDraft({ ...draft!, cacheReadPer1m: v }))
          : priceCell(r.cacheReadPer1m),
    },
    {
      title: <Text style={{ color: INK_2 }}>Cache write $/1M</Text>,
      key: 'cw',
      width: 130,
      render: (_: unknown, r: PricingRow) =>
        editing(r.model)
          ? numInput(draft!.cacheWritePer1m, v => setDraft({ ...draft!, cacheWritePer1m: v }))
          : priceCell(r.cacheWritePer1m),
    },
    {
      title: '',
      key: 'actions',
      width: 120,
      render: (_: unknown, r: PricingRow) =>
        editing(r.model) ? (
          <Space size={4}>
            <Button size="small" type="primary" icon={<SaveOutlined />} loading={saving} onClick={save} />
            <Button size="small" onClick={() => setDraft(null)}>
              Cancel
            </Button>
          </Space>
        ) : (
          <Space size={4}>
            <Button size="small" onClick={() => setDraft({ ...r, isNew: false })}>
              Edit
            </Button>
            <Popconfirm title={`Remove pricing for ${r.model}?`} onConfirm={() => remove(r.model)}>
              <Button size="small" danger icon={<DeleteOutlined />} />
            </Popconfirm>
          </Space>
        ),
    },
  ];

  return (
    <Card style={{ ...CARD_STYLE, marginTop: 24 }} bodyStyle={{ padding: '16px 24px 16px' }}>
      {msgCtx}
      <Space style={{ width: '100%', justifyContent: 'space-between' }}>
        <div>
          <Text style={{ color: INK_2 }}>Model pricing (USD per 1M tokens)</Text>
          <br />
          <Text style={{ color: INK_2, fontSize: 12 }}>
            Rows match by exact id first, then by prefix — tokens from models with no matching row
            are counted as “without pricing”, never as $0.
          </Text>
        </div>
        <Button
          icon={<PlusOutlined />}
          disabled={!!draft}
          onClick={() =>
            setDraft({
              model: '',
              inputPer1m: null,
              outputPer1m: null,
              cacheReadPer1m: null,
              cacheWritePer1m: null,
              isNew: true,
            })
          }
        >
          Add model
        </Button>
      </Space>
      {draft?.isNew && (
        <Space style={{ marginTop: 12 }} wrap>
          <Input
            size="small"
            placeholder="model id, e.g. gpt-5.2"
            value={draft.model}
            status={draft.model.trim() ? undefined : 'error'}
            onChange={e => setDraft({ ...draft, model: e.target.value })}
            style={{ width: 220 }}
          />
          {numInput(draft.inputPer1m, v => setDraft({ ...draft, inputPer1m: v }), true)}
          {numInput(draft.outputPer1m, v => setDraft({ ...draft, outputPer1m: v }), true)}
          {numInput(draft.cacheReadPer1m, v => setDraft({ ...draft, cacheReadPer1m: v }))}
          {numInput(draft.cacheWritePer1m, v => setDraft({ ...draft, cacheWritePer1m: v }))}
          <Button size="small" type="primary" icon={<SaveOutlined />} loading={saving} onClick={save}>
            Save
          </Button>
          <Button size="small" onClick={() => setDraft(null)}>
            Cancel
          </Button>
        </Space>
      )}
      <Table<PricingRow>
        size="small"
        rowKey="model"
        columns={columns}
        dataSource={rows}
        pagination={rows.length > 12 ? { pageSize: 12 } : false}
        locale={{ emptyText: <Text style={{ color: 'rgba(255,255,255,0.3)' }}>No pricing rows</Text> }}
        style={{ marginTop: 12 }}
      />
    </Card>
  );
}
