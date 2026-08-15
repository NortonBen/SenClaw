import { Fragment } from 'react';
import { Typography, theme } from 'antd';

const { Text } = Typography;

/**
 * Tool arguments, rendered as labelled rows instead of raw JSON.
 *
 * The permission card is a decision prompt: the user has a second to judge
 * whether to let the agent do this. Braces, quotes and snake_case keys make
 * that harder than it needs to be — `{"start_local": "2026-08-15 19:23"}` is
 * the same information as `Bắt đầu · 2026-08-15 19:23`, just less legible.
 *
 * Falls back to the original text whenever the payload is not a flat JSON
 * object, so a tool that sends prose, an array, or malformed JSON still shows
 * exactly what it sent rather than nothing.
 */

/** snake_case / camelCase key → human label. Unknown keys are humanised. */
const KEY_LABELS: Record<string, string> = {
  title: 'Tiêu đề',
  name: 'Tên',
  start_local: 'Bắt đầu',
  end_local: 'Kết thúc',
  start_at: 'Bắt đầu',
  end_at: 'Kết thúc',
  all_day: 'Cả ngày',
  location: 'Địa điểm',
  description: 'Mô tả',
  content: 'Nội dung',
  path: 'Đường dẫn',
  file_path: 'Tệp',
  command: 'Lệnh',
  url: 'Đường dẫn',
  query: 'Truy vấn',
  reminder_min: 'Nhắc trước (phút)',
  event_id: 'Mã sự kiện',
  field: 'Trường',
  value: 'Giá trị',
  directive: 'Quy tắc',
  tier: 'Phạm vi',
  limit: 'Giới hạn',
  timeout: 'Hết hạn (giây)',
};

function humanise(key: string): string {
  if (KEY_LABELS[key]) return KEY_LABELS[key];
  const spaced = key.replace(/_/g, ' ').replace(/([a-z])([A-Z])/g, '$1 $2');
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}

/** Render one value. Objects/arrays keep JSON — nesting has no flat form. */
function renderValue(v: unknown): string {
  if (v === null || v === undefined) return '—';
  if (typeof v === 'boolean') return v ? 'Có' : 'Không';
  if (typeof v === 'string') return v.trim() === '' ? '—' : v;
  if (typeof v === 'number') return String(v);
  return JSON.stringify(v);
}

export function ToolParams({ content }: { content: string }) {
  const { token } = theme.useToken();

  const raw = (
    <div
      style={{
        whiteSpace: 'pre-wrap',
        fontFamily: 'monospace',
        fontSize: 12,
        color: token.colorTextSecondary,
        padding: 8,
        borderRadius: 8,
        background: token.colorFillAlter,
        marginBottom: 8,
      }}
    >
      {content}
    </div>
  );

  const text = content?.trim();
  if (!text) return null;

  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return raw;
  }
  if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return raw;

  const entries = Object.entries(parsed as Record<string, unknown>);
  // `{}` is what a no-argument tool sends. A card reading "{}" tells the user
  // nothing; showing no parameter block at all says the same thing quietly.
  if (entries.length === 0) return null;

  return (
    <div
      style={{
        padding: 8,
        borderRadius: 8,
        background: token.colorFillAlter,
        marginBottom: 8,
        display: 'grid',
        gridTemplateColumns: 'max-content 1fr',
        columnGap: 12,
        rowGap: 4,
        fontSize: 12,
      }}
    >
      {entries.map(([k, v]) => (
        <Fragment key={k}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {humanise(k)}
          </Text>
          <Text style={{ fontSize: 12, wordBreak: 'break-word' }}>{renderValue(v)}</Text>
        </Fragment>
      ))}
    </div>
  );
}
