// Nhãn + helper dùng chung giữa trang Kits và hộp thoại cài đặt.
//
// Tách ra vì hai bên hiển thị cùng một vốn từ của daemon: `created/skipped/
// unsupported/failed` khi cài, `removed/missing/failed` khi gỡ. Để mỗi bên tự
// định nghĩa thì chỉ cần lệch một chữ là cùng một trạng thái đọc ra hai nghĩa.

import { Tag } from 'antd';
import type { KitItemStatus, KitRemoveStatus } from '../../types';

/** Trạng thái từng mục khi cài.
 *
 * `skipped` KHÔNG phải lỗi — đó là luật không-ghi-đè của daemon, và mục bị bỏ
 * qua không vào sổ biên nhận nên gỡ kit cũng không đụng tới nó.
 */
export const INSTALL_STATUS: Record<
  KitItemStatus,
  { color: string; label: string; hint: string }
> = {
  created: { color: 'green', label: 'đã tạo', hint: 'Kit tạo mới mục này.' },
  skipped: {
    color: 'default',
    label: 'đã có sẵn',
    hint: 'Tên này đã tồn tại — daemon giữ nguyên bản cũ và không ghi đè. Gỡ kit sẽ không đụng tới nó.',
  },
  unsupported: {
    color: 'orange',
    label: 'không cài',
    hint: 'Daemon đọc được nhưng không tự cài loại này — nó có luồng đồng ý riêng.',
  },
  failed: {
    color: 'red',
    label: 'lỗi',
    hint: 'Mục này thất bại; các mục khác vẫn được xử lý tiếp.',
  },
};

export const REMOVE_STATUS: Record<KitRemoveStatus, { color: string; label: string }> = {
  removed: { color: 'green', label: 'đã gỡ' },
  missing: { color: 'default', label: 'không còn' },
  failed: { color: 'red', label: 'lỗi' },
};

export const KIND_LABEL: Record<string, string> = {
  agent: 'Persona',
  skill: 'Skill',
  workflow: 'Workflow',
  hook: 'Hook',
  job: 'Lịch chạy',
  mcpServer: 'MCP server',
  app: 'Space App',
};

export function kindTag(kind: string) {
  return <Tag>{KIND_LABEL[kind] ?? kind}</Tag>;
}

/** Đọc `{"error": "..."}` của AppError; không phải JSON thì trả HTTP code. */
export async function errorText(res: Response): Promise<string> {
  try {
    const body = await res.json();
    if (body && typeof body.error === 'string') return body.error;
  } catch {
    /* không phải JSON */
  }
  return `HTTP ${res.status}`;
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}
