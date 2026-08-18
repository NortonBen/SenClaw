// Hộp thoại cài kit: chọn nguồn → xem trước → cài → báo cáo.
//
// Xem trước là bước đồng ý, không phải bước trang trí. Một kit có thể mang theo
// Space App — tức là kéo hẳn một chương trình về máy — nên người dùng phải nhìn
// thấy chính xác nó chứa gì TRƯỚC khi bấm cài. Vì thế mọi đường vào (chọn file,
// dán JSON, lấy từ marketplace) đều đi qua đúng một hộp thoại này.
//
//   POST /api/kits/preview            multipart .zip/.json, hoặc JSON manifest
//   POST /api/kits/install            như trên, trả báo cáo từng mục
//   POST /api/kits/available/preview  {sourceId, name} — kit trong catalog
//   POST /api/kits/available/install  như trên
//
// Ba luật của daemon mà hộp thoại phải phản ánh trung thực:
//  1. Không ghi đè → `skipped` là "đã có sẵn, giữ nguyên", KHÔNG phải lỗi.
//  2. Không dừng giữa chừng → `ok:false` vẫn kèm báo cáo; luôn render nó.
//  3. Chỉ gỡ thứ đã tạo → mục `skipped` không vào sổ nên gỡ không đụng tới.

import { useCallback, useEffect, useState } from 'react';
import {
  Alert, Button, Descriptions, Divider, Empty, Input, Modal, Result, Space,
  Table, Tag, Tooltip, Typography, Upload, message, theme,
} from 'antd';
import {
  ApartmentOutlined, AppstoreOutlined, ClockCircleOutlined, CloudServerOutlined,
  FileZipOutlined, InboxOutlined, LinkOutlined, RobotOutlined, RocketOutlined,
  SafetyCertificateOutlined, ThunderboltOutlined, UpOutlined, DownOutlined,
} from '@ant-design/icons';
import type {
  KitInstallReport, KitItemOutcome, KitPreview, KitPreviewItem, KitWarning,
} from '../../types';
import KitParamsForm, {
  initialAnswers, missingRequired, type KitParamAnswers,
} from './KitParamsForm';
import { INSTALL_STATUS, KIND_LABEL, errorText, formatBytes, kindTag } from './kitCommon';

const { Text, Title, Paragraph } = Typography;

/** Kit đến từ đâu. `market` đã biết sẵn tên nên bỏ qua bước chọn file. */
export type KitPickSource =
  | { kind: 'local' }
  | { kind: 'market'; sourceId: string; sourceName: string; name: string };

interface Props {
  open: boolean;
  source: KitPickSource;
  onClose: () => void;
  /** Gọi sau khi cài xong (kể cả cài một phần) để trang làm mới danh sách. */
  onInstalled: () => void;
}

const COUNT_LABEL: Array<[keyof KitPreview['counts'], string]> = [
  ['agents', 'Persona'],
  ['skills', 'Skill'],
  ['workflows', 'Workflow'],
  ['hooks', 'Hook'],
  ['jobs', 'Lịch chạy'],
];

/** Thân request cho manifest dán tay: manifest + câu trả lời tham số.
 *
 * Người dùng có thể đã dán sẵn dạng bọc (`{"manifest": {...}}`) — bọc thêm một
 * lần nữa thì trường `manifest` của kit trở thành object và daemon parse hỏng.
 */
function buildJsonBody(text: string, answers: KitParamAnswers): Record<string, unknown> {
  const parsed = JSON.parse(text);
  const isObject = (v: unknown) => typeof v === 'object' && v !== null && !Array.isArray(v);
  const wrapped =
    isObject(parsed) &&
    (isObject((parsed as Record<string, unknown>).manifest) ||
      isObject((parsed as Record<string, unknown>).kit));
  return wrapped
    ? { ...(parsed as Record<string, unknown>), params: answers }
    : { manifest: parsed, params: answers };
}

export default function KitInstallDialog({ open, source, onClose, onInstalled }: Props) {
  const [file, setFile] = useState<File | null>(null);
  const [text, setText] = useState('');
  const [preview, setPreview] = useState<KitPreview | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const [answers, setAnswers] = useState<KitParamAnswers>({});
  const [installing, setInstalling] = useState(false);
  const [report, setReport] = useState<KitInstallReport | null>(null);

  const reset = useCallback(() => {
    setFile(null);
    setText('');
    setPreview(null);
    setPreviewError(null);
    setAnswers({});
    setReport(null);
  }, []);

  useEffect(() => {
    if (!open) reset();
  }, [open, reset]);

  // ── Xem trước ──────────────────────────────────────────────────────────────

  /** Gửi kit đi theo đúng dạng nó đến: file thì multipart, dán tay thì JSON. */
  const send = useCallback(
    async (path: 'preview' | 'install', withAnswers: KitParamAnswers, force = false) => {
      if (source.kind === 'market') {
        return fetch(`/api/kits/available/${path}`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            sourceId: source.sourceId,
            name: source.name,
            params: withAnswers,
            force,
          }),
        });
      }
      if (file) {
        const form = new FormData();
        form.append('file', file, file.name);
        form.append('params', JSON.stringify(withAnswers));
        form.append('force', String(force));
        // Không đặt Content-Type — trình duyệt tự thêm boundary của multipart.
        return fetch(`/api/kits/${path}`, { method: 'POST', body: form });
      }
      const body = buildJsonBody(text, withAnswers);
      return fetch(`/api/kits/${path}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ...body, force }),
      });
    },
    [source, file, text],
  );

  const runPreview = useCallback(
    async (nextAnswers: KitParamAnswers) => {
      setPreviewing(true);
      setPreviewError(null);
      try {
        const res = await send('preview', nextAnswers);
        if (!res.ok) {
          setPreview(null);
          setPreviewError(await errorText(res));
          return;
        }
        const data: KitPreview = await res.json();
        setPreview(data);
        // Điền mặc định đúng một lần, khi form tham số vừa xuất hiện.
        setAnswers((prev) =>
          Object.keys(prev).length ? prev : initialAnswers(data.params ?? []),
        );
      } catch (e) {
        setPreview(null);
        setPreviewError(String(e));
      } finally {
        setPreviewing(false);
      }
    },
    [send],
  );

  // Kit từ marketplace đã đủ thông tin để xem trước ngay khi mở.
  useEffect(() => {
    if (open && source.kind === 'market') void runPreview({});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, source.kind]);

  // Chọn file xong là xem trước luôn — chọn file chính là ý định "xem cái này".
  useEffect(() => {
    if (file) void runPreview({});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [file]);

  // ── Cài ────────────────────────────────────────────────────────────────────

  const install = useCallback(
    async (force: boolean) => {
      setInstalling(true);
      try {
        const res = await send('install', answers, force);
        const data = await res.json().catch(() => null);
        if (!res.ok || !data?.report) {
          message.error(data?.error ?? `HTTP ${res.status}`);
          return;
        }
        setReport(data.report as KitInstallReport);
        // `ok:false` vẫn là cài một phần — danh sách phải làm mới dù có lỗi.
        onInstalled();
        if (data.ok) message.success('Đã cài kit');
        else message.warning('Cài xong nhưng có mục lỗi — xem báo cáo');
      } catch (e) {
        message.error(String(e));
      } finally {
        setInstalling(false);
      }
    },
    [answers, send, onInstalled],
  );

  // ── Trạng thái nút ─────────────────────────────────────────────────────────

  const missing = preview ? missingRequired(preview.params ?? [], answers) : [];
  const hasKit = source.kind === 'market' || !!file || text.trim().length > 0;
  const canInstall = !!preview && missing.length === 0 && !preview.paramError;
  const blockedApps = (report?.items ?? []).filter(
    (i) => i.type === 'app' && i.status === 'failed' && (i.detail ?? '').includes('security scan'),
  );

  return (
    <Modal
      open={open}
      onCancel={onClose}
      width={760}
      destroyOnClose
      title={
        <Space>
          <RocketOutlined />
          {report ? 'Kết quả cài kit' : 'Cài kit'}
          {source.kind === 'market' && !report ? <Tag color="blue">{source.sourceName}</Tag> : null}
        </Space>
      }
      footer={
        report ? (
          <Space>
            {blockedApps.length > 0 ? (
              <Button
                danger
                loading={installing}
                onClick={() => void install(true)}
                icon={<SafetyCertificateOutlined />}
              >
                Cài lại, bỏ qua cảnh báo bảo mật
              </Button>
            ) : null}
            <Button type="primary" onClick={onClose}>
              Đóng
            </Button>
          </Space>
        ) : (
          <Space>
            <Button onClick={onClose}>Huỷ</Button>
            <Button
              type="primary"
              icon={<RocketOutlined />}
              disabled={!canInstall}
              loading={installing}
              onClick={() => void install(false)}
            >
              Cài kit
            </Button>
          </Space>
        )
      }
    >
      {report ? (
        <InstallReport report={report} />
      ) : (
        <Space direction="vertical" size={14} style={{ width: '100%' }}>
          {source.kind === 'local' ? (
            <KitPicker
              file={file}
              text={text}
              previewing={previewing}
              onFile={setFile}
              onText={(v) => {
                setText(v);
                setFile(null);
                setPreview(null);
              }}
              onPreviewText={() => void runPreview({})}
            />
          ) : null}

          {previewError ? <Alert type="error" showIcon message={previewError} /> : null}

          {preview ? (
            <KitSummary
              preview={preview}
              answers={answers}
              onAnswers={(next) => {
                setAnswers(next);
                void runPreview(next);
              }}
            />
          ) : hasKit ? null : (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description="Chọn tệp .zip / .json hoặc dán manifest để xem kit chứa gì"
            />
          )}
        </Space>
      )}
    </Modal>
  );
}

// ─── Chọn nguồn ───────────────────────────────────────────────────────────────

function KitPicker({
  file,
  text,
  previewing,
  onFile,
  onText,
  onPreviewText,
}: {
  file: File | null;
  text: string;
  previewing: boolean;
  onFile: (f: File) => void;
  onText: (v: string) => void;
  onPreviewText: () => void;
}) {
  return (
    <>
      <Upload.Dragger
        accept=".zip,.json,application/zip,application/json"
        maxCount={1}
        showUploadList={false}
        // Chặn upload mặc định của antd: file đi kèm request preview/install của
        // chính hộp thoại này, không phải một endpoint upload riêng.
        beforeUpload={(f) => {
          onFile(f as unknown as File);
          return false;
        }}
        style={{ padding: '8px 0' }}
      >
        <p className="ant-upload-drag-icon" style={{ marginBottom: 4 }}>
          <InboxOutlined />
        </p>
        <p className="ant-upload-text" style={{ fontSize: 14 }}>
          {file ? file.name : 'Kéo tệp vào đây, hoặc bấm để chọn'}
        </p>
        <p className="ant-upload-hint" style={{ fontSize: 12 }}>
          <b>.zip</b> — kit đầy đủ: <code>kit.json</code> khai báo, kèm thư mục{' '}
          <code>skills/</code>, <code>workflows/</code>, <code>apps/</code> chứa tệp cài đặt.{' '}
          <b>.json</b> — chỉ manifest, không mang theo được app.
        </p>
      </Upload.Dragger>

      {file ? (
        <Text type="secondary" style={{ fontSize: 12 }}>
          <FileZipOutlined /> {file.name} · {formatBytes(file.size)}
        </Text>
      ) : (
        <>
          <Divider plain style={{ margin: '4px 0' }}>
            <Text type="secondary" style={{ fontSize: 12 }}>
              hoặc dán manifest
            </Text>
          </Divider>
          <Input.TextArea
            rows={5}
            value={text}
            onChange={(e) => onText(e.target.value)}
            onBlur={() => text.trim() && onPreviewText()}
            placeholder={'{\n  "manifest": 2,\n  "id": "daily-report",\n  "agents": [ … ]\n}'}
            style={{ fontFamily: 'ui-monospace, monospace', fontSize: 12 }}
          />
          <Button size="small" loading={previewing} disabled={!text.trim()} onClick={onPreviewText}>
            Xem trước
          </Button>
        </>
      )}
    </>
  );
}

// ─── Tóm tắt kit ──────────────────────────────────────────────────────────────

function KitSummary({
  preview,
  answers,
  onAnswers,
}: {
  preview: KitPreview;
  answers: KitParamAnswers;
  onAnswers: (next: KitParamAnswers) => void;
}) {
  const { token } = theme.useToken();
  const counts = COUNT_LABEL.filter(([k]) => preview.counts[k] > 0);
  const bundleApps = preview.bundle?.apps ?? [];

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      <div>
        <Title level={5} style={{ margin: 0 }}>
          {preview.name || preview.id}{' '}
          <Text type="secondary" style={{ fontSize: 13, fontWeight: 400 }}>
            v{preview.version}
          </Text>
        </Title>
        <Text type="secondary" style={{ fontSize: 12 }}>
          id: <code>{preview.id}</code>
          {preview.bundle?.hasFiles ? ' · gói .zip' : ' · manifest JSON'}
        </Text>
        {preview.description ? (
          <Paragraph type="secondary" style={{ fontSize: 13, marginTop: 6, marginBottom: 0 }}>
            {preview.description}
          </Paragraph>
        ) : null}
      </div>

      {preview.installed ? (
        <Alert
          type="info"
          showIcon
          message={`Kit này đã cài (v${preview.installed.version})`}
          description="Cài lại sẽ không ghi đè: mục nào trùng tên sẽ được giữ nguyên và báo là “đã có sẵn”."
        />
      ) : null}

      <ItemList
        items={preview.items ?? []}
        summary={counts.map(([key, label]) => `${preview.counts[key]} ${label}`).join(', ')}
      />

      {/* App là thứ đáng nhìn kỹ nhất: cài kit đồng nghĩa kéo cả chương trình
          về máy, nên nó tách riêng chứ không lẫn vào hàng thẻ đếm. */}
      {bundleApps.length > 0 ? (
        <Alert
          type="warning"
          showIcon
          icon={<AppstoreOutlined />}
          message={`Kit này cài ${bundleApps.length} Space App vào máy`}
          description={
            <Space direction="vertical" size={2} style={{ width: '100%' }}>
              {bundleApps.map((a) => (
                <Text key={a.id} style={{ fontSize: 12 }}>
                  <code>{a.id}</code> · {formatBytes(a.bytes)}
                </Text>
              ))}
              <Text type="secondary" style={{ fontSize: 12 }}>
                Mỗi app đều qua bước quét bảo mật trước khi cài; app bị chặn sẽ báo lỗi riêng
                và phần còn lại của kit vẫn cài bình thường.
              </Text>
            </Space>
          }
        />
      ) : null}

      {preview.bundle?.hasFiles &&
      (preview.bundle.skills.length > 0 || preview.bundle.workflows.length > 0) ? (
        <Descriptions size="small" column={1} bordered>
          {preview.bundle.skills.length ? (
            <Descriptions.Item label="Skill trong gói">
              {preview.bundle.skills.join(', ')}
            </Descriptions.Item>
          ) : null}
          {preview.bundle.workflows.length ? (
            <Descriptions.Item label="Workflow trong gói">
              {preview.bundle.workflows.join(', ')}
            </Descriptions.Item>
          ) : null}
        </Descriptions>
      ) : null}

      {preview.warnings.length > 0 ? <Warnings items={preview.warnings} /> : null}

      {(preview.params ?? []).length > 0 ? (
        <div
          style={{
            border: `1px solid ${token.colorBorderSecondary}`,
            borderRadius: 8,
            padding: 12,
          }}
        >
          <Text strong style={{ fontSize: 13 }}>
            Kit cần bạn điền
          </Text>
          <div style={{ marginTop: 8 }}>
            <KitParamsForm params={preview.params} answers={answers} onChange={onAnswers} />
          </div>
          {preview.paramError ? (
            <Alert style={{ marginTop: 8 }} type="error" showIcon message={preview.paramError} />
          ) : null}
        </div>
      ) : null}
    </Space>
  );
}

// ─── Danh sách từng mục ───────────────────────────────────────────────────────

const ITEM_ICON: Record<KitPreviewItem['type'], React.ReactNode> = {
  agent: <RobotOutlined />,
  skill: <ThunderboltOutlined />,
  workflow: <ApartmentOutlined />,
  hook: <LinkOutlined />,
  job: <ClockCircleOutlined />,
  app: <AppstoreOutlined />,
  mcpServer: <CloudServerOutlined />,
};

/** Dòng phụ của một mục — ghép ở client vì nhãn phải theo ngôn ngữ. */
function itemSubtitle(item: KitPreviewItem): string {
  const parts: string[] = [];
  switch (item.type) {
    case 'job':
      if (item.cron) parts.push(item.cron);
      if (item.agentRef) parts.push(`agent: ${item.agentRef}`);
      // Cài ở trạng thái tạm dừng là điều đáng nói trước: người dùng sẽ đi tìm
      // xem vì sao lịch không chạy.
      if (item.enabled === false) parts.push('cài ở trạng thái tạm dừng');
      break;
    case 'hook':
      if (item.matcher) parts.push(`khớp: ${item.matcher}`);
      if (item.if) parts.push(`nếu: ${item.if}`);
      if (item.blocking) parts.push('có thể chặn vòng lặp agent');
      break;
    case 'app':
      if (item.bytes) parts.push(formatBytes(item.bytes));
      break;
    case 'mcpServer':
      parts.push('daemon không cài — cài qua trang MCP servers');
      break;
    default:
      if (item.description) parts.push(item.description);
      if (item.source === 'bundle') parts.push('tệp từ gói .zip');
      break;
  }
  return parts.join(' · ');
}

function ItemList({ items, summary }: { items: KitPreviewItem[]; summary: string }) {
  const { token } = theme.useToken();
  const [open, setOpen] = useState(true);

  if (items.length === 0) {
    return <Text type="secondary">Kit không khai báo mục nào để cài.</Text>;
  }

  return (
    <div>
      <div
        onClick={() => setOpen((v) => !v)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          cursor: 'pointer',
          userSelect: 'none',
          marginBottom: open ? 8 : 0,
        }}
      >
        <Text strong style={{ fontSize: 13 }}>
          Sẽ cài: {summary || `${items.length} mục`}
        </Text>
        {open ? <UpOutlined style={{ fontSize: 10 }} /> : <DownOutlined style={{ fontSize: 10 }} />}
      </div>

      {open ? (
        <Space direction="vertical" size={6} style={{ width: '100%' }}>
          {items.map((item, i) => {
            const subtitle = itemSubtitle(item);
            const muted = item.unsupported === true;
            return (
              <div
                key={`${item.type}-${item.name}-${i}`}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  padding: '8px 12px',
                  border: `1px solid ${token.colorBorderSecondary}`,
                  borderRadius: 8,
                  opacity: muted ? 0.6 : 1,
                }}
              >
                <span
                  style={{
                    width: 28,
                    height: 28,
                    borderRadius: 6,
                    background: token.colorFillAlter,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    color: token.colorTextSecondary,
                    flexShrink: 0,
                  }}
                >
                  {ITEM_ICON[item.type]}
                </span>
                <div style={{ minWidth: 0, flex: 1 }}>
                  <Text strong style={{ fontSize: 13 }}>
                    {item.name}
                  </Text>
                  {subtitle ? (
                    <div>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {subtitle}
                      </Text>
                    </div>
                  ) : null}
                </div>
                <Tag style={{ marginInlineEnd: 0 }}>{KIND_LABEL[item.type] ?? item.type}</Tag>
              </div>
            );
          })}
        </Space>
      ) : null}
    </div>
  );
}

function Warnings({ items }: { items: KitWarning[] }) {
  return (
    <Alert
      type="warning"
      showIcon
      message="Cảnh báo"
      description={
        <Space direction="vertical" size={2} style={{ width: '100%' }}>
          {items.map((w, i) => (
            <Text key={`${w.kind}-${i}`} style={{ fontSize: 12 }}>
              <b>{w.subject}</b>: {w.detail}
            </Text>
          ))}
        </Space>
      }
    />
  );
}

// ─── Báo cáo ──────────────────────────────────────────────────────────────────

function InstallReport({ report }: { report: KitInstallReport }) {
  const failed = report.items.filter((i) => i.status === 'failed').length;
  const created = report.items.filter((i) => i.status === 'created').length;
  const skipped = report.items.filter((i) => i.status === 'skipped').length;

  return (
    <Space direction="vertical" size={12} style={{ width: '100%' }}>
      <Result
        status={failed ? 'warning' : 'success'}
        style={{ padding: '8px 0' }}
        title={failed ? `Cài xong ${created} mục, ${failed} mục lỗi` : `Đã cài ${created} mục`}
        subTitle={
          skipped > 0
            ? `${skipped} mục đã có sẵn nên được giữ nguyên — gỡ kit sẽ không đụng tới chúng.`
            : undefined
        }
      />
      <Table<KitItemOutcome>
        rowKey={(r) => `${r.type}-${r.name}`}
        size="small"
        pagination={false}
        dataSource={report.items}
        scroll={{ x: 520 }}
        columns={[
          { title: 'Loại', dataIndex: 'type', width: 110, render: kindTag },
          { title: 'Tên', dataIndex: 'name' },
          {
            title: 'Kết quả',
            dataIndex: 'status',
            width: 120,
            render: (s: keyof typeof INSTALL_STATUS) => (
              <Tooltip title={INSTALL_STATUS[s].hint}>
                <Tag color={INSTALL_STATUS[s].color}>{INSTALL_STATUS[s].label}</Tag>
              </Tooltip>
            ),
          },
          {
            title: 'Chi tiết',
            dataIndex: 'detail',
            render: (d?: string) =>
              d ? <Text style={{ fontSize: 12 }}>{d}</Text> : <Text type="secondary">—</Text>,
          },
        ]}
      />
    </Space>
  );
}
