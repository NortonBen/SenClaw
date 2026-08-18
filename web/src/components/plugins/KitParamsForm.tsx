// Form các tham số một kit hỏi trước khi cài (`params[]` trong manifest).
//
// Mỗi loại tham số ra một control: string → ô chữ (secret → ô mật khẩu),
// number → ô số có min/max/step, boolean → công tắc, select → dropdown,
// folder → ô đường dẫn + nút duyệt thư mục trên máy chạy daemon.
//
// Giá trị giữ nguyên kiểu JSON (number là số, boolean là bool) rồi gửi
// nguyên vẹn cho `/api/kits/install`; daemon mới là nơi kiểm tra thật sự —
// form chỉ dựng sẵn giá trị mặc định và chặn những lỗi thấy ngay.

import { useState } from 'react';
import { Button, Input, InputNumber, Select, Space, Switch, Tag, Typography, theme } from 'antd';
import { FolderOpenOutlined } from '@ant-design/icons';
import type { KitParam } from '../../types';
import FolderPicker from '../shared/FolderPicker';

const { Text } = Typography;

export type KitParamAnswers = Record<string, unknown>;

/** Giá trị khởi tạo từ `default` của từng tham số. */
export function initialAnswers(params: KitParam[]): KitParamAnswers {
  const out: KitParamAnswers = {};
  for (const p of params) {
    if (p.default !== undefined && p.default !== null) {
      out[p.key] = p.default;
      continue;
    }
    // Không có default: công tắc vẫn phải có trạng thái, còn lại để trống để
    // daemon phân biệt "chưa trả lời" với "trả lời rỗng".
    if (p.type === 'boolean') out[p.key] = false;
  }
  return out;
}

/** Tham số bắt buộc nào còn trống — để tắt nút Cài trước khi gọi mạng. */
export function missingRequired(params: KitParam[], answers: KitParamAnswers): KitParam[] {
  return params.filter((p) => {
    if (!p.required) return false;
    const v = answers[p.key];
    if (v === undefined || v === null) return true;
    if (typeof v === 'string' && v.trim() === '') return true;
    return false;
  });
}

interface Props {
  params: KitParam[];
  answers: KitParamAnswers;
  onChange: (next: KitParamAnswers) => void;
}

export default function KitParamsForm({ params, answers, onChange }: Props) {
  const { token } = theme.useToken();
  const [picking, setPicking] = useState<KitParam | null>(null);

  if (params.length === 0) return null;

  const set = (key: string, value: unknown) => onChange({ ...answers, [key]: value });

  const control = (p: KitParam) => {
    const value = answers[p.key];

    switch (p.type) {
      case 'boolean':
        return (
          <Switch
            size="small"
            checked={value === true || value === 'true'}
            onChange={(v) => set(p.key, v)}
          />
        );

      case 'number':
        return (
          <InputNumber
            style={{ width: 200 }}
            value={typeof value === 'number' ? value : value == null ? null : Number(value)}
            min={p.min}
            max={p.max}
            step={p.step ?? 1}
            placeholder={p.placeholder || undefined}
            // `null` khi xoá trắng — đừng gửi NaN cho daemon.
            onChange={(v) => set(p.key, v ?? null)}
          />
        );

      case 'select':
        return (
          <Select
            style={{ width: 240 }}
            value={value == null ? undefined : String(value)}
            placeholder={p.placeholder || 'Chọn…'}
            onChange={(v) => set(p.key, v)}
            options={p.options.map((o) => ({ value: o.value, label: o.label || o.value }))}
          />
        );

      case 'folder':
        return (
          <Space.Compact style={{ width: 420, maxWidth: '100%' }}>
            <Input
              value={value == null ? '' : String(value)}
              placeholder={p.placeholder || '~/Projects/…'}
              onChange={(e) => set(p.key, e.target.value)}
              style={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: 12 }}
            />
            <Button icon={<FolderOpenOutlined />} onClick={() => setPicking(p)}>
              Duyệt…
            </Button>
          </Space.Compact>
        );

      default:
        return p.secret ? (
          <Input.Password
            style={{ width: 320, maxWidth: '100%' }}
            value={value == null ? '' : String(value)}
            placeholder={p.placeholder || undefined}
            autoComplete="new-password"
            onChange={(e) => set(p.key, e.target.value)}
          />
        ) : (
          <Input
            style={{ width: 320, maxWidth: '100%' }}
            value={value == null ? '' : String(value)}
            placeholder={p.placeholder || undefined}
            onChange={(e) => set(p.key, e.target.value)}
          />
        );
    }
  };

  return (
    <div
      style={{
        border: `1px solid ${token.colorBorderSecondary}`,
        borderRadius: 8,
        padding: 12,
        background: token.colorFillQuaternary,
      }}
    >
      <Text strong style={{ fontSize: 13 }}>Tham số cài đặt</Text>
      <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 10 }}>
        Kit này hỏi {params.length} thông tin trước khi cài. Giá trị được thay vào
        chỗ <code>{'{{param.<key>}}'}</code> trong manifest.
      </Text>

      <Space direction="vertical" size={12} style={{ width: '100%' }}>
        {params.map((p) => (
          <div key={p.key}>
            <Space size={6} wrap style={{ marginBottom: 4 }}>
              <Text style={{ fontSize: 13 }}>{p.label || p.key}</Text>
              {p.required ? <Tag color="red">bắt buộc</Tag> : null}
              {p.secret ? <Tag color="purple">bí mật</Tag> : null}
              <Text type="secondary" style={{ fontSize: 11 }}>
                <code>{`{{param.${p.key}}}`}</code>
              </Text>
            </Space>
            <div>{control(p)}</div>
            {p.description ? (
              <Text type="secondary" style={{ fontSize: 11, display: 'block', marginTop: 3 }}>
                {p.description}
              </Text>
            ) : null}
          </div>
        ))}
      </Space>

      {picking ? (
        <FolderPicker
          open
          title={`Chọn thư mục cho “${picking.label || picking.key}”`}
          initialPath={
            typeof answers[picking.key] === 'string' ? String(answers[picking.key]) : undefined
          }
          onCancel={() => setPicking(null)}
          onPick={(path) => {
            set(picking.key, path);
            setPicking(null);
          }}
        />
      ) : null}
    </div>
  );
}
