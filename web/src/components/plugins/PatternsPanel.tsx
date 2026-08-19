// Plugins → Patterns — duyệt, chạy, import và đồng bộ Zen Pattern.
//
// Pattern là một system prompt đặt tên sẵn: chữ vào → chữ ra, một lượt LLM,
// không tool, không vòng lặp. Vì thế chúng KHÔNG phải skill (200+ skill sẽ
// nhấn chìm bộ đối sánh trigger) — xem src/patterns/mod.rs.
//
//   GET    /api/patterns                     → danh sách đã khử trùng tên + nguồn + strategy
//   GET    /api/patterns/:name               → nội dung system.md/user.md
//   POST   /api/patterns/run                 → render (dryRun) hoặc chạy thật
//   POST   /api/patterns                     → tạo/ghi đè trong nguồn ghi được
//   POST   /api/patterns/import              → upload .zip các thư mục pattern
//   GET    /api/patterns/catalog             → nguồn cài được mà không cần gõ URL
//   POST   /api/patterns/catalog/:id/install → cài một trong số đó
//   DELETE /api/patterns/:name?source=       → xoá
//   GET/POST /api/patterns/sources           → liệt kê / thêm nguồn git
//   POST   /api/patterns/sources/:id/sync    → clone hoặc pull
//   POST   /api/patterns/sources/:id/toggle  → bật/tắt mà không xoá
//   DELETE /api/patterns/sources/:id         → gỡ nguồn + xoá tệp

import { useCallback, useEffect, useMemo, useState } from 'react';
import {
  Alert, Button, Card, Col, Drawer, Empty, Form, Input, Modal, Popconfirm, Row,
  Collapse, Select, Space, Spin, Switch, Table, Tag, Tooltip, Typography, Upload,
  message,
} from 'antd';
import {
  CheckCircleOutlined, CloudDownloadOutlined, CopyOutlined, DeleteOutlined,
  ExperimentOutlined,
  EyeInvisibleOutlined, EyeOutlined, InboxOutlined, LockOutlined, PlayCircleOutlined,
  PlusOutlined, ReloadOutlined, RocketOutlined, SearchOutlined, SyncOutlined,
  ThunderboltOutlined, WarningOutlined,
} from '@ant-design/icons';

const { Text, Title, Paragraph } = Typography;

// ─── Kiểu dữ liệu (khớp src/patterns) ─────────────────────────────────────────

interface PatternRow {
  name: string;
  source: string;
  description: string;
  shadowedIn?: string[];
  writable: boolean;
}

interface SourceRow {
  id: string;
  name: string;
  kind: 'local' | 'git';
  url?: string;
  ref: string;
  subdir: string;
  strategiesSubdir?: string;
  enabled: boolean;
  installedBy?: string;
  lastSyncedAt?: string;
  lastError?: string;
  count: number;
  writable: boolean;
}

interface StrategyRow {
  name: string;
  description: string;
  prompt: string;
}

interface CatalogEntry {
  id: string;
  name: string;
  description: string;
  /** 'bundled' cài offline; 'git' phải clone. */
  kind: 'bundled' | 'git';
  count: number;
  license: string;
  url?: string;
  gitRef?: string;
  subdir?: string;
  strategiesSubdir?: string;
  installed: boolean;
  pinned: boolean;
}

interface PatternFiles {
  name: string;
  source: string;
  system: string;
  user?: string;
  path: string;
  writable: boolean;
}

const USER_SOURCE = 'user';

/// Dòng mô tả rút từ thân pattern — cùng quy tắc daemon dùng cho danh sách:
/// dòng văn xuôi đầu tiên sau tiêu đề `# IDENTITY and PURPOSE`.
function describeSystem(body: string): string {
  const line = body
    .split('\n')
    .map((l) => l.trim())
    .find((l) => l && !l.startsWith('#') && !l.startsWith('---'));
  return line ?? '';
}

/** Đọc thông điệp lỗi daemon trả về, không nuốt thành "request failed". */
async function errText(res: Response): Promise<string> {
  try {
    const body = await res.json();
    if (typeof body?.error === 'string') return body.error;
  } catch {
    /* không phải JSON — rơi xuống dưới */
  }
  return `HTTP ${res.status}`;
}

// ─── Hộp chạy pattern ─────────────────────────────────────────────────────────

/// Khung chạy một pattern.
///
/// Bố cục theo thứ tự người dùng thực sự cần: **ô nhập → kết quả → prompt**.
/// Bản trước đổ nguyên `system.md` ra giữa hộp thoại, nên thứ chiếm nhiều chỗ
/// nhất lại là thứ ít ai đọc — kết quả bị đẩy xuống dưới màn hình.
function RunBox({
  pattern,
  strategies,
}: {
  pattern: PatternFiles;
  strategies: StrategyRow[];
}) {
  const [input, setInput] = useState('');
  const [strategy, setStrategy] = useState<string | undefined>();
  // Mặc định "auto": thư viện Fabric viết bằng tiếng Anh và hầu hết pattern ép
  // output tiếng Anh, nên input tiếng Việt sẽ nhận lại bản tóm tắt tiếng Anh.
  const [language, setLanguage] = useState<string>('auto');
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [isPreview, setIsPreview] = useState(false);
  const [unresolved, setUnresolved] = useState<string[]>([]);
  const [meta, setMeta] = useState<string>('');

  const run = useCallback(
    async (dryRun: boolean) => {
      setBusy(true);
      setResult(null);
      try {
        const res = await fetch('/api/patterns/run', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            name: pattern.name,
            input,
            strategy,
            language: language === 'off' ? undefined : language,
            dryRun,
          }),
        });
        if (!res.ok) {
          message.error(await errText(res));
          return;
        }
        const body = await res.json();
        setIsPreview(dryRun);
        if (dryRun) {
          setResult(
            `# SYSTEM\n\n${body.rendered.system}\n\n# USER\n\n${body.rendered.user}`,
          );
          setUnresolved(body.rendered.unresolved ?? []);
          setMeta('prompt đã ghép — chưa gọi model');
        } else {
          setResult(body.text ?? '');
          setUnresolved(body.unresolved ?? []);
          setMeta(`${body.model} · ${body.latencyMs}ms`);
        }
      } catch (e) {
        message.error(String(e));
      } finally {
        setBusy(false);
      }
    },
    [pattern.name, input, strategy, language],
  );

  const canRun = input.trim().length > 0 || pattern.system.includes('{{input}}');

  return (
    <Space direction="vertical" size="middle" style={{ width: '100%' }}>
      <Input.TextArea
        rows={5}
        value={input}
        onChange={(e) => setInput(e.target.value)}
        placeholder="Dán văn bản cần xử lý — bài báo, transcript, log, ghi chú…"
        style={{ resize: 'vertical' }}
      />

      <Space wrap size="small" style={{ width: '100%' }}>
        <Select
          allowClear
          size="small"
          style={{ minWidth: 190 }}
          placeholder="Strategy"
          value={strategy}
          onChange={setStrategy}
          options={strategies.map((s) => ({
            value: s.name,
            label: s.name,
            title: s.description,
          }))}
        />
        <Select
          size="small"
          style={{ minWidth: 170 }}
          value={language}
          onChange={setLanguage}
          options={[
            { value: 'auto', label: 'Ngôn ngữ: theo input' },
            { value: 'Vietnamese', label: 'Ngôn ngữ: tiếng Việt' },
            { value: 'English', label: 'Ngôn ngữ: English' },
            { value: 'off', label: 'Ngôn ngữ: pattern tự quyết' },
          ]}
        />
        <div style={{ flex: 1 }} />
        <Tooltip title="Chỉ ghép prompt để xem trước — không tốn lượt gọi model">
          <Button
            size="small"
            icon={<ExperimentOutlined />}
            disabled={busy}
            onClick={() => run(true)}
          >
            Xem prompt
          </Button>
        </Tooltip>
        <Button
          size="small"
          type="primary"
          icon={<PlayCircleOutlined />}
          loading={busy}
          disabled={!canRun}
          onClick={() => run(false)}
        >
          Chạy
        </Button>
      </Space>

      {unresolved.length > 0 && (
        <Alert
          type="warning"
          showIcon
          message={`Biến chưa điền: ${unresolved.join(', ')}`}
          description="Pattern giữ nguyên chỗ trống thay vì xoá đi, nên chúng vẫn nằm trong prompt."
        />
      )}

      {result !== null && (
        <Card
          size="small"
          title={
            <Space size={6}>
              <Text style={{ fontSize: 13 }}>
                {isPreview ? 'Prompt sẽ gửi' : 'Kết quả'}
              </Text>
              <Text type="secondary" style={{ fontSize: 11, fontWeight: 400 }}>
                {meta}
              </Text>
            </Space>
          }
          extra={
            <Tooltip title="Chép">
              <Button
                size="small"
                type="text"
                icon={<CopyOutlined />}
                onClick={() => {
                  void navigator.clipboard.writeText(result);
                  message.success('Đã chép');
                }}
              />
            </Tooltip>
          }
          styles={{ body: { padding: 0 } }}
        >
          <pre
            style={{
              margin: 0,
              padding: 12,
              maxHeight: 340,
              overflow: 'auto',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              fontFamily: 'ui-monospace, monospace',
              fontSize: 12,
              lineHeight: 1.6,
            }}
          >
            {result}
          </pre>
        </Card>
      )}
    </Space>
  );
}

// ─── Thẻ catalog ──────────────────────────────────────────────────────────────

/// Một nguồn cài được bằng một cú bấm.
///
/// `bundled` nằm sẵn trong binary nên cài offline và tức thì; `git` phải clone
/// nên nút phải nói trước là sẽ mất một lúc. Hai chuyện đó khác nhau đủ để
/// người dùng cần biết trước khi bấm.
function CatalogCard({
  entry,
  busy,
  disabled,
  onInstall,
}: {
  entry: CatalogEntry;
  busy: boolean;
  disabled: boolean;
  onInstall: () => void;
}) {
  return (
    <Card size="small" style={{ height: '100%' }}>
      <Space direction="vertical" size={4} style={{ width: '100%' }}>
        <Space>
          <Text strong>{entry.name}</Text>
          <Tag color={entry.kind === 'bundled' ? 'green' : 'blue'}>
            {entry.kind === 'bundled' ? 'đi kèm' : 'git'}
          </Tag>
          <Tag>{entry.count} pattern</Tag>
        </Space>
        <Text type="secondary" style={{ fontSize: 12 }}>
          {entry.description}
        </Text>
        <Space size={4} wrap style={{ fontSize: 11 }}>
          <Text type="secondary" style={{ fontSize: 11 }}>
            {entry.license}
          </Text>
          {entry.gitRef && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              · {entry.gitRef}
            </Text>
          )}
          {/* Một pattern nằm ở vị trí system prompt, nên "bám nhánh" là rủi ro
              cần nói ra, không phải chi tiết kỹ thuật giấu đi. */}
          {!entry.pinned && (
            <Tooltip title="Nguồn này bám một nhánh đang chạy: một commit phía trên có thể lặng lẽ viết lại chỉ thị mà agent sẽ tuân theo.">
              <Text type="warning" style={{ fontSize: 11 }}>
                <WarningOutlined /> chưa ghim tag
              </Text>
            </Tooltip>
          )}
        </Space>
        <div style={{ marginTop: 8 }}>
          {entry.installed ? (
            <Button size="small" disabled icon={<CheckCircleOutlined />}>
              Đã cài
            </Button>
          ) : (
            <Button
              size="small"
              type="primary"
              loading={busy}
              disabled={disabled && !busy}
              icon={entry.kind === 'bundled' ? <ThunderboltOutlined /> : <CloudDownloadOutlined />}
              onClick={onInstall}
            >
              {entry.kind === 'bundled' ? 'Cài ngay' : 'Tải về'}
            </Button>
          )}
        </div>
      </Space>
    </Card>
  );
}

// ─── Chip nguồn ───────────────────────────────────────────────────────────────

/// Nguồn vừa là thông tin vừa là bộ lọc — gộp làm một chip bấm được, các nút
/// quản lý chỉ hiện trên chip đang chọn để dải không bị rối.
function SourceChip({
  label,
  count,
  kind,
  active,
  dimmed,
  error,
  actions,
  onClick,
}: {
  label: string;
  count: number;
  kind?: string;
  active: boolean;
  dimmed?: boolean;
  error?: string | null;
  actions?: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <div
      onClick={onClick}
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        padding: actions ? '2px 4px 2px 10px' : '4px 10px',
        borderRadius: 6,
        cursor: 'pointer',
        userSelect: 'none',
        opacity: dimmed ? 0.5 : 1,
        border: `1px solid ${active ? 'var(--ant-color-primary)' : 'var(--ant-color-border)'}`,
        background: active ? 'var(--ant-color-primary-bg)' : 'transparent',
      }}
    >
      <Text style={{ fontSize: 13 }}>{label}</Text>
      {kind === 'git' && (
        <Text type="secondary" style={{ fontSize: 10 }}>
          git
        </Text>
      )}
      <Tag style={{ marginInlineEnd: 0 }}>{count}</Tag>
      {error && (
        <Tooltip title={error}>
          <WarningOutlined style={{ color: '#d48806' }} />
        </Tooltip>
      )}
      {actions}
    </div>
  );
}

// ─── Panel chính ──────────────────────────────────────────────────────────────

export default function PatternsPanel() {
  const [rows, setRows] = useState<PatternRow[]>([]);
  const [sources, setSources] = useState<SourceRow[]>([]);
  const [strategies, setStrategies] = useState<StrategyRow[]>([]);
  const [catalog, setCatalog] = useState<CatalogEntry[]>([]);
  const [installing, setInstalling] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [sourceFilter, setSourceFilter] = useState<string | undefined>();
  const [busySource, setBusySource] = useState<string | null>(null);

  const [open, setOpen] = useState<PatternFiles | null>(null);
  const [addSource, setAddSource] = useState(false);
  const [newPattern, setNewPattern] = useState(false);
  const [importing, setImporting] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const [res, catRes] = await Promise.all([
        fetch('/api/patterns'),
        fetch('/api/patterns/catalog'),
      ]);
      const body = res.ok ? await res.json() : null;
      // Catalog là phần thêm: hỏng nó không được làm hỏng danh sách pattern.
      const cat = catRes.ok ? await catRes.json() : null;
      setCatalog(Array.isArray(cat?.catalog) ? cat.catalog : []);
      // Daemon cũ trả trang SPA cho /api lạ → không phải object có `patterns`.
      if (!body || !Array.isArray(body.patterns)) {
        setLoadError(
          'Daemon này chưa phục vụ /api/patterns — cần build lại và khởi động daemon mới.',
        );
        setRows([]);
        setSources([]);
        return;
      }
      setRows(body.patterns);
      setSources(body.sources ?? []);
      setStrategies(body.strategies ?? []);
    } catch (e) {
      setLoadError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Lọc phía client: danh sách đã nằm sẵn trong bộ nhớ, gọi lại API mỗi lần gõ
  // chỉ tốn round-trip mà không thêm kết quả nào.
  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return rows.filter((r) => {
      if (sourceFilter && r.source !== sourceFilter) return false;
      if (!q) return true;
      return (
        r.name.toLowerCase().includes(q) || r.description.toLowerCase().includes(q)
      );
    });
  }, [rows, query, sourceFilter]);

  /// Cài một mục trong catalog. Bản `bundled` không chạm mạng; bản `git` clone
  /// nên có thể mất một lúc — nút phải khoá lại trong lúc đó.
  const installFromCatalog = useCallback(
    async (entry: CatalogEntry) => {
      setInstalling(entry.id);
      try {
        const res = await fetch(
          `/api/patterns/catalog/${encodeURIComponent(entry.id)}/install`,
          { method: 'POST' },
        );
        if (!res.ok) {
          message.error(await errText(res));
          return;
        }
        const body = await res.json();
        const n = body.sync?.patterns ?? body.installed?.length ?? 0;
        message.success(`Đã cài ${n} pattern từ "${entry.name}"`);
        await load();
      } catch (e) {
        message.error(String(e));
      } finally {
        setInstalling(null);
      }
    },
    [load],
  );

  const openPattern = useCallback(async (name: string) => {
    try {
      const res = await fetch(`/api/patterns/${encodeURIComponent(name)}`);
      if (!res.ok) {
        message.error(await errText(res));
        return;
      }
      const body = await res.json();
      setOpen(body.pattern);
    } catch (e) {
      message.error(String(e));
    }
  }, []);

  const syncSource = useCallback(
    async (id: string) => {
      setBusySource(id);
      try {
        const res = await fetch(`/api/patterns/sources/${encodeURIComponent(id)}/sync`, {
          method: 'POST',
        });
        if (!res.ok) {
          message.error(await errText(res));
          return;
        }
        const body = await res.json();
        message.success(`${body.sync.patterns} pattern từ "${id}"`);
        await load();
      } catch (e) {
        message.error(String(e));
      } finally {
        setBusySource(null);
      }
    },
    [load],
  );

  const toggleSource = useCallback(
    async (id: string) => {
      setBusySource(id);
      try {
        const res = await fetch(`/api/patterns/sources/${encodeURIComponent(id)}/toggle`, {
          method: 'POST',
        });
        if (!res.ok) message.error(await errText(res));
        else await load();
      } finally {
        setBusySource(null);
      }
    },
    [load],
  );

  const removeSource = useCallback(
    async (id: string) => {
      const res = await fetch(`/api/patterns/sources/${encodeURIComponent(id)}`, {
        method: 'DELETE',
      });
      if (!res.ok) message.error(await errText(res));
      else {
        message.success(`Đã gỡ nguồn "${id}"`);
        await load();
      }
    },
    [load],
  );

  const removePattern = useCallback(
    async (row: PatternRow) => {
      const res = await fetch(
        `/api/patterns/${encodeURIComponent(row.name)}?source=${encodeURIComponent(row.source)}`,
        { method: 'DELETE' },
      );
      if (!res.ok) message.error(await errText(res));
      else {
        message.success(`Đã xoá "${row.name}"`);
        await load();
      }
    },
    [load],
  );

  const columns = [
    {
      title: 'Pattern',
      dataIndex: 'name',
      key: 'name',
      width: 260,
      render: (name: string, row: PatternRow) => (
        <Space direction="vertical" size={0}>
          <a onClick={() => void openPattern(name)}>{name}</a>
          {(row.shadowedIn?.length ?? 0) > 0 && (
            <Tooltip
              title={`Cũng có trong: ${row.shadowedIn!.join(', ')} — bản ở "${row.source}" được dùng`}
            >
              <Text type="secondary" style={{ fontSize: 11 }}>
                đè lên {row.shadowedIn!.length} nguồn khác
              </Text>
            </Tooltip>
          )}
        </Space>
      ),
    },
    {
      title: 'Mô tả',
      dataIndex: 'description',
      key: 'description',
      render: (d: string) => <Text type="secondary">{d}</Text>,
    },
    {
      title: 'Nguồn',
      dataIndex: 'source',
      key: 'source',
      width: 140,
      render: (s: string) => <Tag>{s}</Tag>,
    },
    {
      title: '',
      key: 'actions',
      width: 60,
      render: (_: unknown, row: PatternRow) =>
        row.writable ? (
          <Popconfirm title={`Xoá "${row.name}"?`} onConfirm={() => void removePattern(row)}>
            <Button type="text" danger size="small" icon={<DeleteOutlined />} />
          </Popconfirm>
        ) : (
          <Tooltip title="Nguồn git — sửa bằng cách lưu bản riêng cùng tên vào nguồn của bạn">
            <Button type="text" size="small" disabled icon={<DeleteOutlined />} />
          </Tooltip>
        ),
    },
  ];

  return (
    <div style={{ padding: 24 }}>
      <Space direction="vertical" size="large" style={{ width: '100%' }}>
        <div>
          <Title level={4} style={{ marginBottom: 4 }}>
            Patterns
          </Title>
          <Paragraph type="secondary" style={{ marginBottom: 0 }}>
            Prompt đặt tên sẵn cho một phép biến đổi văn bản: chữ vào → chữ ra, một
            lượt model, không tool. Dùng cho tóm tắt, trích ý, phân tích log,
            viết lại… Agent gọi chúng qua <Text code>pattern_run</Text>.
          </Paragraph>
        </div>

        {loadError && <Alert type="error" showIcon message={loadError} />}

        {/* ─── Bắt đầu: catalog khi chưa có gì ───
            Màn hình rỗng trước đây chỉ ghi "0 pattern" và một nút mở form 5 ô
            trống. Cái người dùng cần thấy đầu tiên là *cài cái gì*. */}
        {!loading && rows.length === 0 && (
          <Card
            size="small"
            title={
              <Space>
                <RocketOutlined />
                <span>Bắt đầu</span>
              </Space>
            }
          >
            <Row gutter={[12, 12]}>
              {catalog.map((entry) => (
                <Col key={entry.id} xs={24} md={12} xl={8}>
                  <CatalogCard
                    entry={entry}
                    busy={installing === entry.id}
                    disabled={!!installing}
                    onInstall={() => void installFromCatalog(entry)}
                  />
                </Col>
              ))}
              {/* Import .zip là một cách CÀI, nên nó thuộc về đây — bản trước
                  chỉ để nút này trong toolbar của bảng danh sách, mà bảng đó
                  bị ẩn khi chưa có pattern nào: màn hình rỗng thành ra không
                  có đường nào tới import. */}
              <Col xs={24} md={12} xl={8}>
                <Card size="small" style={{ height: '100%' }}>
                  <Space direction="vertical" size={4} style={{ width: '100%' }}>
                    <Space>
                      <Text strong>Từ tệp .zip</Text>
                      <Tag>ngoại tuyến</Tag>
                    </Space>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      Một zip mà mỗi thư mục con là một pattern có{' '}
                      <Text code>system.md</Text>. Zip tải từ GitHub cũng được — thư
                      mục bọc ngoài được bỏ tự động.
                    </Text>
                    <div style={{ marginTop: 8 }}>
                      <Button size="small" icon={<InboxOutlined />} onClick={() => setImporting(true)}>
                        Chọn tệp .zip
                      </Button>
                    </div>
                  </Space>
                </Card>
              </Col>
              <Col xs={24} md={12} xl={8}>
                <Card size="small" style={{ height: '100%' }}>
                  <Space direction="vertical" size={4} style={{ width: '100%' }}>
                    <Text strong>Nguồn khác</Text>
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      Một repo git bất kỳ có các thư mục chứa <Text code>system.md</Text>,
                      hoặc pattern anh tự viết.
                    </Text>
                    <Space style={{ marginTop: 8 }}>
                      <Button
                        size="small"
                        icon={<CloudDownloadOutlined />}
                        onClick={() => setAddSource(true)}
                      >
                        Nguồn git
                      </Button>
                      <Button size="small" icon={<PlusOutlined />} onClick={() => setNewPattern(true)}>
                        Tự viết
                      </Button>
                    </Space>
                  </Space>
                </Card>
              </Col>
            </Row>
          </Card>
        )}

        {/* ─── Nguồn: một dải lọc, không phải một lưới thẻ ───
            Nguồn vừa là thông tin vừa là bộ lọc, nên gộp làm một: bấm để lọc,
            các nút quản lý nằm ngay trên chip đang chọn. */}
        {rows.length > 0 && (
          <Card size="small" styles={{ body: { padding: '10px 12px' } }}>
            <Space wrap size={[8, 8]} style={{ width: '100%' }}>
              <SourceChip
                label="Tất cả"
                count={rows.length}
                active={!sourceFilter}
                onClick={() => setSourceFilter(undefined)}
              />
              {sources.map((s) => (
                <SourceChip
                  key={s.id}
                  label={s.name || s.id}
                  count={s.count}
                  kind={s.kind}
                  dimmed={!s.enabled}
                  error={s.lastError}
                  active={sourceFilter === s.id}
                  onClick={() =>
                    setSourceFilter(sourceFilter === s.id ? undefined : s.id)
                  }
                  actions={
                    sourceFilter === s.id ? (
                      <Space size={0}>
                        {s.kind === 'git' && (
                          <Tooltip title="Tải lại từ git">
                            <Button
                              type="text"
                              size="small"
                              icon={<SyncOutlined spin={busySource === s.id} />}
                              onClick={(e) => {
                                e.stopPropagation();
                                void syncSource(s.id);
                              }}
                            />
                          </Tooltip>
                        )}
                        <Tooltip title={s.enabled ? 'Tắt nguồn này' : 'Bật lại'}>
                          <Button
                            type="text"
                            size="small"
                            icon={s.enabled ? <EyeOutlined /> : <EyeInvisibleOutlined />}
                            onClick={(e) => {
                              e.stopPropagation();
                              void toggleSource(s.id);
                            }}
                          />
                        </Tooltip>
                        {s.id !== USER_SOURCE && (
                          <Popconfirm
                            title={`Gỡ "${s.id}" và xoá ${s.count} pattern của nó?`}
                            onConfirm={() => void removeSource(s.id)}
                          >
                            <Button
                              type="text"
                              size="small"
                              danger
                              icon={<DeleteOutlined />}
                              onClick={(e) => e.stopPropagation()}
                            />
                          </Popconfirm>
                        )}
                      </Space>
                    ) : null
                  }
                />
              ))}
              <Button
                size="small"
                type="dashed"
                icon={<PlusOutlined />}
                onClick={() => setAddSource(true)}
              >
                Nguồn
              </Button>
            </Space>
          </Card>
        )}

        {/* ─── Danh sách pattern ─── */}
        {rows.length > 0 && (
          <Card
            size="small"
            title={
              <Space size={6}>
                <span>{visible.length} pattern</span>
                {sourceFilter && <Tag>{sourceFilter}</Tag>}
              </Space>
            }
            extra={
              <Space>
                <Input
                  allowClear
                  size="small"
                  prefix={<SearchOutlined style={{ opacity: 0.45 }} />}
                  style={{ width: 240 }}
                  placeholder="Tìm theo tên hoặc mô tả"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                <Tooltip title="Thêm pattern tự viết">
                  <Button size="small" icon={<PlusOutlined />} onClick={() => setNewPattern(true)} />
                </Tooltip>
                <Tooltip title="Import .zip các thư mục pattern">
                  <Button size="small" icon={<InboxOutlined />} onClick={() => setImporting(true)} />
                </Tooltip>
                <Tooltip title="Tải lại">
                  <Button size="small" icon={<ReloadOutlined />} onClick={() => void load()} />
                </Tooltip>
              </Space>
            }
          >
            {loading ? (
              <Spin />
            ) : visible.length === 0 ? (
              <Empty description="Không khớp bộ lọc" />
            ) : (
              <Table
                size="small"
                rowKey="name"
                dataSource={visible}
                columns={columns}
                pagination={{
                  pageSize: 25,
                  showSizeChanger: true,
                  size: 'small',
                  hideOnSinglePage: true,
                }}
              />
            )}
          </Card>
        )}
      </Space>

      {/* ─── Xem + chạy ─── */}
      <Drawer
        width={760}
        open={!!open}
        onClose={() => setOpen(null)}
        title={
          <Space size={8}>
            <Text strong>{open?.name}</Text>
            <Tag style={{ marginInlineEnd: 0 }}>{open?.source}</Tag>
            {open && !open.writable && (
              <Tooltip title="Nguồn git — sửa bằng cách lưu bản riêng cùng tên vào nguồn của bạn">
                <LockOutlined style={{ opacity: 0.45 }} />
              </Tooltip>
            )}
          </Space>
        }
        styles={{ body: { paddingTop: 12 } }}
      >
        {open && (
          <Space direction="vertical" size="middle" style={{ width: '100%' }}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              {describeSystem(open.system)}
            </Text>
            <RunBox pattern={open} strategies={strategies} />
            {/* Gập lại theo mặc định: đây là thứ chiếm nhiều chỗ nhất nhưng ít
                người đọc, và bản trước để nó đẩy kết quả xuống dưới màn hình. */}
            <Collapse
              size="small"
              ghost
              items={[
                {
                  key: 'src',
                  label: (
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      Xem system.md {open.user ? '+ user.md' : ''}
                    </Text>
                  ),
                  children: (
                    <>
                      <pre
                        style={{
                          margin: 0,
                          padding: 12,
                          maxHeight: 380,
                          overflow: 'auto',
                          whiteSpace: 'pre-wrap',
                          wordBreak: 'break-word',
                          fontFamily: 'ui-monospace, monospace',
                          fontSize: 12,
                          lineHeight: 1.6,
                          border: '1px solid var(--ant-color-border)',
                          borderRadius: 6,
                        }}
                      >
                        {open.system}
                        {open.user ? `\n\n---- user.md ----\n\n${open.user}` : ''}
                      </pre>
                      <Text type="secondary" style={{ fontSize: 11 }}>
                        {open.path}
                      </Text>
                    </>
                  ),
                },
              ]}
            />
          </Space>
        )}
      </Drawer>

      <AddSourceModal
        open={addSource}
        catalog={catalog}
        installing={installing}
        onInstall={(entry) => {
          void installFromCatalog(entry).then(() => setAddSource(false));
        }}
        onImport={() => {
          setAddSource(false);
          setImporting(true);
        }}
        onClose={() => setAddSource(false)}
        onDone={() => {
          setAddSource(false);
          void load();
        }}
      />
      <NewPatternModal
        open={newPattern}
        onClose={() => setNewPattern(false)}
        onDone={() => {
          setNewPattern(false);
          void load();
        }}
      />
      <ImportModal
        open={importing}
        sources={sources.filter((s) => s.writable)}
        onClose={() => setImporting(false)}
        onDone={() => {
          setImporting(false);
          void load();
        }}
      />
    </div>
  );
}

// ─── Thêm nguồn git ───────────────────────────────────────────────────────────

/// Catalog trước, form sau.
///
/// Bản đầu tiên của hộp thoại này là 5 ô trống — chỉ điền đúng được nếu đã đọc
/// bố cục repo của người khác (Fabric để pattern ở `data/patterns`, strategy ở
/// `data/strategies`). Giờ trường hợp thường gặp là một cú bấm, còn ô trống
/// lùi xuống phần "nguồn khác" phải mở ra mới thấy.
function AddSourceModal({
  open,
  catalog,
  installing,
  onInstall,
  onImport,
  onClose,
  onDone,
}: {
  open: boolean;
  catalog: CatalogEntry[];
  installing: string | null;
  onInstall: (entry: CatalogEntry) => void;
  onImport: () => void;
  onClose: () => void;
  onDone: () => void;
}) {
  const [form] = Form.useForm();
  const [busy, setBusy] = useState(false);
  const [manual, setManual] = useState(false);

  const offered = catalog.filter((e) => !e.installed);

  const submit = async () => {
    const values = await form.validateFields();
    setBusy(true);
    try {
      const res = await fetch('/api/patterns/sources', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...values, sync: true }),
      });
      if (!res.ok) {
        message.error(await errText(res));
        return;
      }
      const body = await res.json();
      message.success(`Đã tải ${body.sync?.patterns ?? 0} pattern`);
      form.resetFields();
      onDone();
    } catch (e) {
      message.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onCancel={onClose}
      footer={
        manual
          ? [
              <Button key="back" onClick={() => setManual(false)}>
                Quay lại
              </Button>,
              <Button key="ok" type="primary" loading={busy} onClick={() => void submit()}>
                Thêm và tải về
              </Button>,
            ]
          : [
              <Button key="close" onClick={onClose}>
                Đóng
              </Button>,
            ]
      }
      title="Thêm nguồn pattern"
      width={640}
    >
      {!manual ? (
        <Space direction="vertical" size="middle" style={{ width: '100%' }}>
          {offered.length === 0 ? (
            <Empty description="Mọi nguồn đi kèm đều đã cài" />
          ) : (
            <Row gutter={[12, 12]}>
              {offered.map((entry) => (
                <Col key={entry.id} xs={24} md={12}>
                  <CatalogCard
                    entry={entry}
                    busy={installing === entry.id}
                    disabled={!!installing}
                    onInstall={() => onInstall(entry)}
                  />
                </Col>
              ))}
            </Row>
          )}
          <Space direction="vertical" size={8} style={{ width: '100%' }}>
            <Button block type="dashed" icon={<CloudDownloadOutlined />} onClick={() => setManual(true)}>
              Nguồn git khác — nhập URL thủ công
            </Button>
            <Button block type="dashed" icon={<InboxOutlined />} onClick={onImport}>
              Có sẵn tệp .zip — import từ máy
            </Button>
          </Space>
        </Space>
      ) : (
        <>
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 16 }}
            message="Nên ghim tag, đừng để nhánh"
            description="Pattern được đặt vào vị trí system prompt. Theo một nhánh đang chạy nghĩa là một commit phía trên có thể lặng lẽ viết lại chỉ thị mà agent sẽ tuân theo."
          />
          <Form form={form} layout="vertical" initialValues={{ ref: 'main' }}>
            <Form.Item
              name="url"
              label="Git URL"
              rules={[{ required: true, message: 'Cần URL repo' }]}
            >
              <Input placeholder="https://github.com/nguoi-nao-do/prompt-library" />
            </Form.Item>
            <Row gutter={12}>
              <Col span={12}>
                <Form.Item name="id" label="Id nguồn" tooltip="Bỏ trống sẽ lấy theo tên repo">
                  <Input placeholder="prompt-library" />
                </Form.Item>
              </Col>
              <Col span={12}>
                <Form.Item name="ref" label="Branch / tag / sha">
                  <Input placeholder="v1.0.0" />
                </Form.Item>
              </Col>
            </Row>
            <Form.Item
              name="subdir"
              label="Thư mục chứa pattern"
              tooltip="Đường dẫn trong repo tới các thư mục pattern. Để trống nếu chúng nằm ngay gốc repo."
            >
              <Input placeholder="data/patterns" />
            </Form.Item>
            <Form.Item name="strategiesSubdir" label="Thư mục strategies (không bắt buộc)">
              <Input placeholder="data/strategies" />
            </Form.Item>
          </Form>
        </>
      )}
    </Modal>
  );
}

// ─── Pattern mới ──────────────────────────────────────────────────────────────

function NewPatternModal({
  open,
  onClose,
  onDone,
}: {
  open: boolean;
  onClose: () => void;
  onDone: () => void;
}) {
  const [form] = Form.useForm();
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const values = await form.validateFields();
    setBusy(true);
    try {
      const res = await fetch('/api/patterns', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...values, source: USER_SOURCE }),
      });
      if (!res.ok) {
        message.error(await errText(res));
        return;
      }
      message.success('Đã lưu');
      form.resetFields();
      onDone();
    } catch (e) {
      message.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal
      open={open}
      onCancel={onClose}
      onOk={() => void submit()}
      confirmLoading={busy}
      okText="Lưu"
      title="Pattern mới"
      width={720}
    >
      <Form form={form} layout="vertical">
        <Form.Item
          name="name"
          label="Tên"
          rules={[{ required: true, message: 'Cần tên' }]}
          tooltip="Chữ, số, - và _ . Tên có dấu sẽ được gấp thành slug."
        >
          <Input placeholder="tom_tat_hop" />
        </Form.Item>
        <Form.Item
          name="system"
          label="system.md"
          rules={[{ required: true, message: 'Cần system prompt' }]}
          extra="Quy ước Fabric: # IDENTITY and PURPOSE → # STEPS → # OUTPUT INSTRUCTIONS → # INPUT. Dùng {{input}} nếu muốn chèn văn bản vào giữa prompt; không có thì văn bản sẽ thành user message."
        >
          <Input.TextArea
            rows={16}
            style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12 }}
            placeholder={'# IDENTITY and PURPOSE\n\nBạn là…\n\n# OUTPUT INSTRUCTIONS\n\n- …\n\n# INPUT:'}
          />
        </Form.Item>
      </Form>
    </Modal>
  );
}

// ─── Import .zip ──────────────────────────────────────────────────────────────

/// Import từ tệp — đường cài duy nhất không cần mạng lẫn không cần gõ gì.
///
/// Hộp thoại phải nói trước **cấu trúc zip nào là đúng**: cái sai hay gặp là
/// zip một thư mục chứa toàn `.md` phẳng, và khi đó server trả "không tìm thấy
/// pattern nào" mà người dùng không đoán được vì sao.
function ImportModal({
  open,
  sources,
  onClose,
  onDone,
}: {
  open: boolean;
  sources: SourceRow[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [target, setTarget] = useState(USER_SOURCE);
  const [overwrite, setOverwrite] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<{ found: number; imported: string[] } | null>(
    null,
  );

  const upload = async (file: File) => {
    setBusy(true);
    setResult(null);
    try {
      const fd = new FormData();
      fd.append('file', file);
      fd.append('source', target);
      fd.append('overwrite', String(overwrite));
      const res = await fetch('/api/patterns/import', { method: 'POST', body: fd });
      if (!res.ok) {
        message.error(await errText(res));
        return;
      }
      const body = await res.json();
      setResult({ found: body.found ?? 0, imported: body.imported ?? [] });
      onDone();
    } catch (e) {
      message.error(String(e));
    } finally {
      setBusy(false);
    }
  };

  const writable = sources.filter((s) => s.writable);

  return (
    <Modal
      open={open}
      onCancel={() => {
        setResult(null);
        onClose();
      }}
      footer={null}
      title="Import pattern từ tệp .zip"
      width={620}
    >
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        <Card size="small" styles={{ body: { padding: 12 } }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            Mỗi pattern là <b>một thư mục</b> chứa <Text code>system.md</Text>:
          </Text>
          <pre
            style={{
              margin: '8px 0 0',
              fontSize: 11,
              lineHeight: 1.6,
              opacity: 0.75,
              fontFamily: 'ui-monospace, monospace',
            }}
          >{`thu-vien.zip
├── tom_tat/
│   ├── system.md      ← bắt buộc
│   └── user.md        ← tuỳ chọn
└── phan_tich_log/
    └── system.md`}</pre>
          <Text type="secondary" style={{ fontSize: 11 }}>
            Zip tải từ GitHub có một thư mục bọc ngoài — nó được bỏ tự động. Tệp
            khác <Text code>system.md</Text> / <Text code>user.md</Text> đều bị
            bỏ qua.
          </Text>
        </Card>

        <Space size="middle" wrap>
          <span>
            <Text type="secondary" style={{ fontSize: 12, marginInlineEnd: 8 }}>
              Vào nguồn
            </Text>
            <Select
              size="small"
              style={{ minWidth: 160 }}
              value={target}
              onChange={setTarget}
              options={writable.map((s) => ({
                value: s.id,
                label: `${s.name || s.id} (${s.count})`,
              }))}
            />
          </span>
          <Tooltip title="Mặc định giữ nguyên pattern đã có cùng tên — bật để ghi đè">
            <span>
              <Text type="secondary" style={{ fontSize: 12, marginInlineEnd: 8 }}>
                Ghi đè trùng tên
              </Text>
              <Switch size="small" checked={overwrite} onChange={setOverwrite} />
            </span>
          </Tooltip>
        </Space>

        <Upload.Dragger
          accept=".zip"
          maxCount={1}
          disabled={busy}
          beforeUpload={(file) => {
            void upload(file as File);
            return false;
          }}
          showUploadList={false}
        >
          <p className="ant-upload-drag-icon">
            {busy ? <Spin /> : <InboxOutlined />}
          </p>
          <p className="ant-upload-text">
            {busy ? 'Đang đọc tệp…' : 'Kéo tệp .zip vào đây hoặc bấm để chọn'}
          </p>
        </Upload.Dragger>

        {result && (
          <Alert
            type={result.imported.length > 0 ? 'success' : 'warning'}
            showIcon
            message={
              result.imported.length > 0
                ? `Đã import ${result.imported.length}/${result.found} pattern vào "${target}"`
                : `Tìm thấy ${result.found} pattern nhưng không import cái nào`
            }
            description={
              result.imported.length < result.found ? (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {result.found - result.imported.length} cái đã có sẵn cùng tên và
                  được giữ nguyên. Bật “Ghi đè trùng tên” rồi thử lại nếu muốn thay
                  chúng.
                </Text>
              ) : (
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {result.imported.slice(0, 12).join(', ')}
                  {result.imported.length > 12 ? '…' : ''}
                </Text>
              )
            }
          />
        )}
      </Space>
    </Modal>
  );
}
