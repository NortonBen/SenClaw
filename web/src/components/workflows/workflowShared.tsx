import { Button, Collapse, Descriptions, Dropdown, Input, Modal, Space, Tag, Tooltip, Typography, message, theme } from 'antd';
import {
  BookOutlined, CaretRightOutlined, DeleteOutlined, DownOutlined, DownloadOutlined,
  EditOutlined, RedoOutlined, RightOutlined, StopOutlined,
} from '@ant-design/icons';
import ReactMarkdown from 'react-markdown';
import { useState } from 'react';

const { Text, Paragraph } = Typography;

// ─── Wire types (mirror src/workflow REST payloads) ──────────────────────────

export interface WorkflowInputDef {
  name: string;
  required?: boolean;
  default?: string;
  description?: string;
}

export interface WorkflowDefSummary {
  name: string;
  description?: string;
  stepCount: number;
  inputs: WorkflowInputDef[];
}

export interface StepRun {
  id: string;
  kind: 'agent' | 'script';
  persona?: string;
  dependsOn?: string[];
  status: 'pending' | 'running' | 'done' | 'failed' | 'skipped';
  result: string;
  error?: string;
  observe?: { label: string; as: 'inline' | 'artifact'; content?: string; artifactPath?: string };
  startedAt?: string;
  completedAt?: string;
}

export interface WorkflowRun {
  id: string;
  workflowName: string;
  /** Optional user-given display name (rename). */
  label?: string;
  inputs: Record<string, string>;
  status: 'running' | 'done' | 'partial-failed' | 'cancelled' | 'interrupted';
  runDir: string;
  steps: StepRun[];
  trigger?: string;
  createdAt: string;
  completedAt?: string;
}

/** Sentinel "jid" marking a workflow session in the chat sidebar. Never a
 *  real chat group — ChatPage renders the flow pane for these. */
export const WFRUN_JID_PREFIX = 'wfrun:';
export const wfRunJid = (runId: string) => `${WFRUN_JID_PREFIX}${runId}`;

export async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  if (!res.ok) {
    let detail = '';
    try { detail = (await res.json())?.error ?? ''; } catch { /* not json */ }
    throw new Error(detail || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

export const RUN_STATUS_COLOR: Record<WorkflowRun['status'], string> = {
  running: 'processing',
  done: 'success',
  'partial-failed': 'warning',
  cancelled: 'default',
  interrupted: 'error',
};

export const STEP_STATUS_COLOR: Record<StepRun['status'], string> = {
  pending: 'default',
  running: 'processing',
  done: 'success',
  failed: 'error',
  skipped: 'default',
};

export function fmtTime(iso?: string): string {
  if (!iso) return '—';
  return new Date(iso).toLocaleString('vi-VN');
}

export const runTitle = (r: WorkflowRun) => r.label?.trim() || r.id;

/** Rename a run (empty clears the label). */
export async function renameRun(id: string, label: string): Promise<void> {
  await apiFetch(`/api/workflows/runs/${encodeURIComponent(id)}`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ label }),
  });
}

/** Delete a run record (server rejects while running). */
export async function deleteRun(id: string): Promise<void> {
  await apiFetch(`/api/workflows/runs/${encodeURIComponent(id)}`, { method: 'DELETE' });
}

/** Assemble one markdown document for a whole run (download / wiki). */
export function runToMarkdown(r: WorkflowRun): string {
  const lines: string[] = [
    `# ${runTitle(r)}`,
    '',
    `- Workflow: \`${r.workflowName}\``,
    `- Run: \`${r.id}\` — **${r.status}**`,
    `- Bắt đầu: ${fmtTime(r.createdAt)}${r.completedAt ? ` · Kết thúc: ${fmtTime(r.completedAt)}` : ''}`,
  ];
  if (Object.keys(r.inputs).length) {
    lines.push(`- Inputs: ${Object.entries(r.inputs).map(([k, v]) => `\`${k}=${v}\``).join(', ')}`);
  }
  for (const s of r.steps) {
    lines.push('', `## ${s.id} (${s.kind}${s.persona ? ` · ${s.persona}` : ''}) — ${s.status}`, '');
    if (s.error) lines.push(`> ⚠️ ${s.error}`, '');
    if (s.observe?.content) lines.push(`### ${s.observe.label}`, '', s.observe.content, '');
    if (s.result) lines.push(s.result);
  }
  return lines.join('\n');
}

export function downloadText(fileName: string, content: string) {
  const blob = new Blob([content], { type: 'text/markdown' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = fileName;
  a.click();
  URL.revokeObjectURL(url);
}

/** Save markdown into the personal wiki under `workflows/…`. */
export async function saveToWiki(path: string, content: string): Promise<void> {
  await apiFetch('/api/wiki/file', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      path,
      content,
      source: 'workflow',
      tags: ['workflow'],
      commit_msg: `workflow: save ${path}`,
    }),
  });
}

const sanitizeFile = (s: string) => s.replace(/[^A-Za-z0-9._-]+/g, '_');

export function fmtDuration(a?: string, b?: string): string {
  if (!a || !b) return '';
  const ms = new Date(b).getTime() - new Date(a).getTime();
  if (ms < 0) return '';
  if (ms < 1000) return `${ms}ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)}s`;
  return `${Math.floor(ms / 60_000)}m${Math.round((ms % 60_000) / 1000)}s`;
}

// ─── Shared run-inputs modal (run / re-run) ──────────────────────────────────

export function RunInputsModal({ target, presetInputs, onClose, onStarted }: {
  target: WorkflowDefSummary | null;
  presetInputs?: Record<string, string>;
  onClose: () => void;
  /** Fired with the new run id after a successful start. */
  onStarted: (runId: string) => void;
}) {
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [starting, setStarting] = useState(false);
  const [initedFor, setInitedFor] = useState<string | null>(null);

  // (Re)initialize the field values when a new target opens.
  if (target && initedFor !== target.name) {
    const init: Record<string, string> = {};
    for (const i of target.inputs) init[i.name] = presetInputs?.[i.name] ?? i.default ?? '';
    setInputs(init);
    setInitedFor(target.name);
  }
  if (!target && initedFor !== null) setInitedFor(null);

  const start = async () => {
    if (!target) return;
    const missing = target.inputs.filter((i) => i.required && !(inputs[i.name] ?? '').trim());
    if (missing.length) {
      message.warning(`Thiếu input bắt buộc: ${missing.map((m) => m.name).join(', ')}`);
      return;
    }
    setStarting(true);
    try {
      const body: Record<string, string> = {};
      for (const [k, v] of Object.entries(inputs)) if (v.trim() !== '') body[k] = v;
      const r = await apiFetch<{ runId: string }>(
        `/api/workflows/${encodeURIComponent(target.name)}/run`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ inputs: body }),
        });
      message.success(`Đã chạy: ${r.runId}`);
      onClose();
      onStarted(r.runId);
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setStarting(false);
    }
  };

  return (
    <Modal
      title={target ? `Chạy workflow: ${target.name}` : ''}
      open={!!target}
      okText="Chạy"
      cancelText="Huỷ"
      okButtonProps={{ icon: <CaretRightOutlined /> }}
      confirmLoading={starting}
      onOk={start}
      onCancel={onClose}
    >
      {target && target.inputs.length === 0 && (
        <Text type="secondary">Workflow này không cần input.</Text>
      )}
      {target?.inputs.map((i) => (
        <div key={i.name} style={{ marginBottom: 10 }}>
          <Text strong style={{ fontSize: 13 }}>
            {i.name}{i.required && <Text type="danger"> *</Text>}
          </Text>
          <Input
            style={{ marginTop: 4 }}
            placeholder={i.default !== undefined ? `mặc định: ${i.default}` : ''}
            value={inputs[i.name] ?? ''}
            onChange={(e) => setInputs((prev) => ({ ...prev, [i.name]: e.target.value }))}
          />
        </div>
      ))}
    </Modal>
  );
}

// ─── Markdown block (observe / results) ──────────────────────────────────────

function MarkdownBlock({ content }: { content: string }) {
  return (
    <div className="wf-md" style={{ fontSize: 13, lineHeight: 1.65, overflowX: 'auto' }}>
      <ReactMarkdown>{content}</ReactMarkdown>
    </div>
  );
}

// ─── Shared read-only run detail (header + info + step cards) ────────────────

export function WorkflowRunDetailView({ run, onCancel, onRerun, onRenamed, onDeleted }: {
  run: WorkflowRun;
  onCancel: () => void;
  onRerun: () => void;
  /** Refresh hook after a successful rename. */
  onRenamed?: () => void;
  /** Called after a successful delete (e.g. clear selection). */
  onDeleted?: () => void;
}) {
  const { token } = theme.useToken();
  // Per-step collapse (default expanded); the caret in each header toggles.
  const [collapsedSteps, setCollapsedSteps] = useState<Set<string>>(new Set());
  const toggleStep = (id: string) => setCollapsedSteps((prev) => {
    const next = new Set(prev);
    if (next.has(id)) next.delete(id); else next.add(id);
    return next;
  });

  const doRename = () => {
    let value = run.label ?? '';
    Modal.confirm({
      title: 'Đổi tên run',
      icon: <EditOutlined />,
      content: (
        <Input
          defaultValue={value}
          placeholder={run.id}
          onChange={(e) => { value = e.target.value; }}
          onPressEnter={(e) => { value = (e.target as HTMLInputElement).value; }}
        />
      ),
      okText: 'Lưu',
      cancelText: 'Huỷ',
      onOk: async () => {
        await renameRun(run.id, value);
        message.success('Đã đổi tên');
        onRenamed?.();
      },
    });
  };

  const doDelete = () => {
    Modal.confirm({
      title: `Xoá run "${runTitle(run)}"?`,
      content: 'Chỉ xoá bản ghi lịch sử — file trong workspace được giữ lại.',
      okText: 'Xoá',
      okButtonProps: { danger: true },
      cancelText: 'Huỷ',
      onOk: async () => {
        await deleteRun(run.id);
        message.success('Đã xoá run');
        onDeleted?.();
      },
    });
  };

  const saveRunToWiki = async () => {
    const path = `workflows/${sanitizeFile(run.workflowName)}/${sanitizeFile(run.id)}.md`;
    try {
      await saveToWiki(path, runToMarkdown(run));
      message.success(`Đã lưu vào wiki: ${path}`);
    } catch (e) {
      message.error(`Lưu wiki thất bại: ${(e as Error).message}`);
    }
  };

  return (
    <Space direction="vertical" style={{ width: '100%', maxWidth: 860 }} size={14}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <div style={{ flex: 1, minWidth: 200 }}>
          <Text strong style={{ fontSize: 18 }}>{runTitle(run)}</Text>
          {run.label && (
            <Text type="secondary" style={{ fontSize: 12, marginLeft: 8 }}>{run.id}</Text>
          )}
          <Tag color={RUN_STATUS_COLOR[run.status]} style={{ marginLeft: 8 }}>{run.status}</Tag>
        </div>
        <Space size={4}>
          <Tooltip title="Đổi tên run">
            <Button size="small" type="text" icon={<EditOutlined />} onClick={doRename} />
          </Tooltip>
          <Tooltip title="Tải toàn bộ kết quả (.md)">
            <Button size="small" type="text" icon={<DownloadOutlined />}
              onClick={() => downloadText(`${sanitizeFile(runTitle(run))}.md`, runToMarkdown(run))} />
          </Tooltip>
          <Tooltip title="Lưu toàn bộ vào wiki">
            <Button size="small" type="text" icon={<BookOutlined />} onClick={saveRunToWiki} />
          </Tooltip>
          <Tooltip title={run.status === 'running' ? 'Huỷ run trước khi xoá' : 'Xoá run'}>
            <Button size="small" type="text" danger icon={<DeleteOutlined />}
              disabled={run.status === 'running'} onClick={doDelete} />
          </Tooltip>
          {run.status === 'running' ? (
            <Button danger size="small" icon={<StopOutlined />} onClick={onCancel}>
              Huỷ run
            </Button>
          ) : (
            <Button size="small" type="primary" ghost icon={<RedoOutlined />} onClick={onRerun}>
              Chạy lại
            </Button>
          )}
        </Space>
      </div>

      <Descriptions size="small" column={2} bordered items={[
        { key: 'wf', label: 'Workflow', children: run.workflowName },
        { key: 'trigger', label: 'Trigger', children: run.trigger ?? '—' },
        { key: 'start', label: 'Bắt đầu', children: fmtTime(run.createdAt) },
        {
          key: 'end', label: 'Kết thúc',
          children: `${fmtTime(run.completedAt)}${run.completedAt ? ` (${fmtDuration(run.createdAt, run.completedAt)})` : ''}`,
        },
        {
          key: 'inputs', label: 'Inputs', span: 2,
          children: Object.keys(run.inputs).length === 0 ? '—' : (
            <Space size={4} wrap>
              {Object.entries(run.inputs).map(([k, v]) => (
                <Tag key={k}><b>{k}</b>={v}</Tag>
              ))}
            </Space>
          ),
        },
        {
          key: 'dir', label: 'Workspace', span: 2,
          children: <Text code style={{ fontSize: 11 }}>{run.runDir}</Text>,
        },
      ]} />

      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Text strong style={{ fontSize: 15, flex: 1 }}>Steps ({run.steps.length})</Text>
        <Button size="small" type="text" style={{ fontSize: 11, color: token.colorTextTertiary }}
          onClick={() => setCollapsedSteps(
            collapsedSteps.size === run.steps.length
              ? new Set()
              : new Set(run.steps.map((s) => s.id)))}>
          {collapsedSteps.size === run.steps.length ? 'Mở rộng tất cả' : 'Thu gọn tất cả'}
        </Button>
      </div>
      {run.steps.map((s) => {
        const stepOpen = !collapsedSteps.has(s.id);
        return (
        <div key={s.id} style={{
          borderRadius: 10, border: `1px solid ${token.colorBorderSecondary}`,
          padding: '10px 14px',
        }}>
          <div
            onClick={() => toggleStep(s.id)}
            style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap', cursor: 'pointer', userSelect: 'none' }}
          >
            <Tooltip title={stepOpen ? 'Thu gọn' : 'Mở rộng'}>
              <Button size="small" type="text"
                icon={stepOpen ? <DownOutlined style={{ fontSize: 9 }} /> : <RightOutlined style={{ fontSize: 9 }} />}
                style={{ width: 20, height: 20, padding: 0 }}
                onClick={(e) => { e.stopPropagation(); toggleStep(s.id); }} />
            </Tooltip>
            <Tag color={STEP_STATUS_COLOR[s.status]}>{s.status}</Tag>
            <Text strong>{s.id}</Text>
            <Tag color={s.kind === 'agent' ? 'geekblue' : 'purple'}>{s.kind}</Tag>
            {s.persona && <Tag>{s.persona}</Tag>}
            {(s.dependsOn?.length ?? 0) > 0 && (
              <Text type="secondary" style={{ fontSize: 12 }}>← {s.dependsOn!.join(', ')}</Text>
            )}
            <div style={{ flex: 1 }} />
            <Tooltip title={`${fmtTime(s.startedAt)} → ${fmtTime(s.completedAt)}`}>
              <Text type="secondary" style={{ fontSize: 12 }}>
                {fmtDuration(s.startedAt, s.completedAt)}
              </Text>
            </Tooltip>
          </div>

          {stepOpen && s.error && (
            <Paragraph type="danger" style={{ margin: '8px 0 0', fontSize: 12, whiteSpace: 'pre-wrap' }}>
              {s.error}
            </Paragraph>
          )}

          {stepOpen && s.observe?.content && (
            <div style={{
              marginTop: 8, padding: '10px 14px', borderRadius: 10,
              background: token.colorFillQuaternary,
              borderLeft: `3px solid ${token.colorPrimary}`,
            }}>
              <Text type="secondary" style={{ fontSize: 11, letterSpacing: 0.5 }}>
                {s.observe.label.toUpperCase()}
              </Text>
              <MarkdownBlock content={s.observe.content} />
            </div>
          )}
          {stepOpen && s.observe?.artifactPath && (
            <Paragraph style={{ margin: '8px 0 0', fontSize: 12 }}>
              <Text type="secondary">{s.observe.label}: </Text>
              <Text code style={{ fontSize: 11 }}>{s.observe.artifactPath}</Text>
            </Paragraph>
          )}

          {stepOpen && s.result && (
            <Collapse
              ghost
              size="small"
              style={{ marginTop: 6 }}
              items={[{
                key: 'r',
                label: <Text type="secondary" style={{ fontSize: 12 }}>Kết quả ({s.result.length.toLocaleString('vi-VN')} ký tự)</Text>,
                extra: (
                  <Space size={0} onClick={(e) => e.stopPropagation()}>
                    <Tooltip title="Tải kết quả step (.md)">
                      <Button size="small" type="text" icon={<DownloadOutlined />}
                        onClick={() => downloadText(
                          `${sanitizeFile(run.id)}-${sanitizeFile(s.id)}.md`, s.result)} />
                    </Tooltip>
                    <Tooltip title="Lưu kết quả step vào wiki">
                      <Button size="small" type="text" icon={<BookOutlined />}
                        onClick={async () => {
                          const path = `workflows/${sanitizeFile(run.workflowName)}/${sanitizeFile(run.id)}-${sanitizeFile(s.id)}.md`;
                          try {
                            await saveToWiki(path, `# ${runTitle(run)} — ${s.id}\n\n${s.result}`);
                            message.success(`Đã lưu vào wiki: ${path}`);
                          } catch (e) {
                            message.error(`Lưu wiki thất bại: ${(e as Error).message}`);
                          }
                        }} />
                    </Tooltip>
                  </Space>
                ),
                children: (
                  <div style={{ maxHeight: 480, overflowY: 'auto' }}>
                    <MarkdownBlock content={s.result} />
                  </div>
                ),
              }]}
            />
          )}
        </div>
        );
      })}
    </Space>
  );
}
