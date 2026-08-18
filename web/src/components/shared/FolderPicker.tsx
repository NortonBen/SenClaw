// Chọn một thư mục trên MÁY CHẠY DAEMON, từ trình duyệt.
//
// Trình duyệt không bao giờ trả đường dẫn tuyệt đối (`<input webkitdirectory>`
// chỉ cho tên tương đối), nên duyệt thư mục phải đi qua daemon — nó chạy ngay
// trên máy đó. Dùng lại `GET /api/workspace/files?path=…&depth=1`, lọc lấy
// `is_dir`.
//
// Một giới hạn có thật: endpoint đó bỏ qua mọi mục bắt đầu bằng dấu chấm, nên
// không thể *duyệt* vào `~/.senclaw`. Ô nhập đường dẫn ở đầu hộp thoại vẫn nhận
// mọi đường dẫn gõ tay — đó là lý do nó luôn hiện, không phải chỉ khi lạc đường.

import { useCallback, useEffect, useState } from 'react';
import { Alert, Button, Input, List, Modal, Space, Typography, theme } from 'antd';
import {
  ArrowUpOutlined, FolderOpenOutlined, FolderOutlined, HomeOutlined, ReloadOutlined,
} from '@ant-design/icons';

const { Text } = Typography;

interface Entry {
  name: string;
  path: string;
  is_dir: boolean;
}

interface Props {
  open: boolean;
  /** Thư mục mở ra lúc đầu; rỗng = `~`. */
  initialPath?: string;
  title?: string;
  onCancel: () => void;
  onPick: (path: string) => void;
}

/** Cha của một đường dẫn POSIX/Windows, hoặc null khi đã ở gốc. */
function parentOf(path: string): string | null {
  const trimmed = path.replace(/[/\\]+$/, '');
  const cut = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
  if (cut < 0) return null;
  if (cut === 0) return '/';
  return trimmed.slice(0, cut);
}

export default function FolderPicker({
  open, initialPath, title, onCancel, onPick,
}: Props) {
  const { token } = theme.useToken();
  const [path, setPath] = useState(initialPath?.trim() || '~');
  const [draft, setDraft] = useState(initialPath?.trim() || '~');
  const [entries, setEntries] = useState<Entry[]>([]);
  const [resolved, setResolved] = useState('');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (target: string) => {
    setLoading(true);
    setError(null);
    try {
      const res = await fetch(
        `/api/workspace/files?path=${encodeURIComponent(target)}&depth=1`,
      );
      if (!res.ok) {
        let msg = `HTTP ${res.status}`;
        try {
          const body = await res.json();
          if (typeof body?.error === 'string') msg = body.error;
        } catch { /* không phải JSON */ }
        throw new Error(msg);
      }
      const json = await res.json();
      // `root` là đường dẫn daemon đã nở `~` — đó mới là thứ đáng trả về.
      setResolved(json.root ?? target);
      setEntries((json.entries ?? []).filter((e: Entry) => e.is_dir));
    } catch (e) {
      setError(String(e instanceof Error ? e.message : e));
      setEntries([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    const start = initialPath?.trim() || '~';
    setPath(start);
    setDraft(start);
    void load(start);
  }, [open, initialPath, load]);

  const goto = (next: string) => {
    setPath(next);
    setDraft(next);
    void load(next);
  };

  const parent = parentOf(resolved || path);

  return (
    <Modal
      open={open}
      title={title ?? 'Chọn thư mục'}
      onCancel={onCancel}
      width={620}
      footer={[
        <Button key="cancel" onClick={onCancel}>Huỷ</Button>,
        <Button
          key="ok"
          type="primary"
          disabled={!resolved}
          onClick={() => onPick(resolved)}
        >
          Chọn thư mục này
        </Button>,
      ]}
    >
      <Space direction="vertical" size={10} style={{ width: '100%' }}>
        <Space.Compact style={{ width: '100%' }}>
          <Input
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onPressEnter={() => goto(draft.trim())}
            placeholder="~/Projects hoặc /Users/…"
            style={{ fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace', fontSize: 12 }}
          />
          <Button onClick={() => goto(draft.trim())} loading={loading} icon={<ReloadOutlined />} />
        </Space.Compact>

        <Space size={6} wrap>
          <Button size="small" icon={<HomeOutlined />} onClick={() => goto('~')}>Home</Button>
          <Button
            size="small"
            icon={<ArrowUpOutlined />}
            disabled={!parent}
            onClick={() => parent && goto(parent)}
          >
            Lên một cấp
          </Button>
          <Text type="secondary" style={{ fontSize: 12, wordBreak: 'break-all' }}>
            {resolved || path}
          </Text>
        </Space>

        {error ? <Alert type="error" showIcon message={error} /> : null}

        <div style={{
          maxHeight: 300,
          overflowY: 'auto',
          border: `1px solid ${token.colorBorderSecondary}`,
          borderRadius: 8,
        }}>
          <List
            size="small"
            loading={loading}
            dataSource={entries}
            locale={{ emptyText: 'Không có thư mục con' }}
            renderItem={(item) => (
              <List.Item
                style={{ cursor: 'pointer', padding: '6px 12px' }}
                onClick={() => goto(item.path)}
              >
                <Space size={8}>
                  <FolderOutlined style={{ color: token.colorWarning }} />
                  <Text style={{ fontSize: 13 }}>{item.name}</Text>
                </Space>
              </List.Item>
            )}
          />
        </div>

        <Text type="secondary" style={{ fontSize: 11 }}>
          <FolderOpenOutlined /> Thư mục ẩn (bắt đầu bằng dấu chấm) không hiện trong danh
          sách — gõ thẳng đường dẫn vào ô trên nếu cần.
        </Text>
      </Space>
    </Modal>
  );
}
