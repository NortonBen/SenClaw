import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  Button, Card, Drawer, Empty, Input, InputNumber, Modal, Popconfirm, Popover, Space, Spin,
  Table, Tag, Tooltip, Typography, Upload, message, theme,
} from 'antd';
import {
  ApartmentOutlined, CaretRightOutlined, ControlOutlined, DeleteOutlined, DownloadOutlined,
  EditOutlined, EyeOutlined, HistoryOutlined, PlusOutlined, ReloadOutlined, RobotOutlined,
  SettingOutlined, UploadOutlined,
} from '@ant-design/icons';

const { Text, Paragraph } = Typography;

// ─── Types (mirror src/workflow REST payloads) ────────────────────────────────

interface WorkflowInputDef {
  name: string;
  required?: boolean;
  default?: string;
  description?: string;
}

interface WorkflowStepDef {
  id: string;
  kind: 'agent' | 'script';
  dependsOn?: string[];
  persona?: string;
  guidance?: string;
  timeout?: number;
}

interface WorkflowDefSummary {
  name: string;
  description?: string;
  stepCount: number;
  inputs: WorkflowInputDef[];
  guidance?: string;
  workspace?: string;
  steps: WorkflowStepDef[];
}

interface StepRun {
  id: string;
  kind: 'agent' | 'script';
  persona?: string;
  dependsOn?: string[];
  status: 'pending' | 'running' | 'done' | 'failed' | 'skipped';
  result: string;
  error?: string;
  observe?: { label: string; as: 'inline' | 'artifact'; content?: string; artifactPath?: string };
}

interface WorkflowRun {
  id: string;
  workflowName: string;
  inputs: Record<string, string>;
  status: 'running' | 'done' | 'partial-failed' | 'cancelled' | 'interrupted';
  runDir: string;
  steps: StepRun[];
  trigger?: string;
  createdAt: string;
  completedAt?: string;
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  if (!res.ok) {
    let detail = '';
    try {
      const body = await res.json();
      detail = body?.error ?? '';
    } catch { /* not json */ }
    throw new Error(detail || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

const NEW_WORKFLOW_TEMPLATE = `---
name: my-workflow
description: Mô tả ngắn quy trình này làm gì
inputs:
  - { name: topic, required: true }
steps:
  - id: fetch
    kind: script
    run: |
      echo "input là: $WF_INPUT_TOPIC"
  - id: analyze
    kind: agent
    persona: researcher
    prompt: |
      Phân tích "{{input.topic}}". Dữ liệu thô: {{steps.fetch.result}}
    observe: { label: "Kết quả", from: result, as: inline }
---
(Ghi chú cho người đọc — phần thân markdown không ảnh hưởng thực thi)
`;

const RUN_STATUS_COLOR: Record<WorkflowRun['status'], string> = {
  running: 'processing',
  done: 'success',
  'partial-failed': 'warning',
  cancelled: 'default',
  interrupted: 'error',
};

// ─── Panel ────────────────────────────────────────────────────────────────────

/** Template manager (Plugins → Workflow): author/import/export/edit/tune/
 *  delete definitions. Runs live on their own page: /workflows/runs. */
export default function WorkflowsPanel() {
  const { token } = theme.useToken();
  const navigate = useNavigate();
  const [defs, setDefs] = useState<WorkflowDefSummary[]>([]);
  const [runs, setRuns] = useState<WorkflowRun[]>([]);
  const [loading, setLoading] = useState(false);

  // Detail drawer
  const [detail, setDetail] = useState<WorkflowDefSummary | null>(null);
  // Editor modal (create or edit)
  const [editor, setEditor] = useState<{ mode: 'create' | 'edit'; name?: string; content: string } | null>(null);
  const [saving, setSaving] = useState(false);
  // Run modal
  const [runTarget, setRunTarget] = useState<WorkflowDefSummary | null>(null);
  const [runInputs, setRunInputs] = useState<Record<string, string>>({});
  const [starting, setStarting] = useState(false);
  // Draft-by-agent modal
  const [draftOpen, setDraftOpen] = useState(false);
  const [draftDesc, setDraftDesc] = useState('');
  const [drafting, setDrafting] = useState(false);
  // Runtime settings (LLM parallelism + retries)
  const [settings, setSettings] = useState<{ llmParallel: number; agentRetries: number }>({
    llmParallel: 1, agentRetries: 1,
  });
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [savingSettings, setSavingSettings] = useState(false);
  // Tune-guidance modal
  const [tune, setTune] = useState<{
    name: string;
    guidance: string;
    workspace: string;
    steps: { id: string; kind: 'agent' | 'script'; persona?: string; guidance: string; timeout?: number }[];
  } | null>(null);
  const [tuning, setTuning] = useState(false);

  const refresh = useCallback(async (silent = false) => {
    if (!silent) setLoading(true);
    try {
      const [d, r] = await Promise.all([
        apiFetch<{ workflows: WorkflowDefSummary[] }>('/api/workflows'),
        apiFetch<{ runs: WorkflowRun[] }>('/api/workflows/runs'),
      ]);
      setDefs(d.workflows ?? []);
      setRuns(r.runs ?? []);
    } catch (e) {
      if (!silent) message.error(`Không tải được workflow: ${(e as Error).message}`);
    } finally {
      if (!silent) setLoading(false);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  useEffect(() => {
    apiFetch<{ llmParallel: number; agentRetries: number }>('/api/workflows/settings')
      .then(setSettings)
      .catch(() => { /* daemon cũ chưa có endpoint — giữ default */ });
  }, []);

  const saveSettings = async () => {
    setSavingSettings(true);
    try {
      const applied = await apiFetch<{ llmParallel: number; agentRetries: number }>(
        '/api/workflows/settings', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(settings),
        });
      setSettings(applied);
      setSettingsOpen(false);
      message.success('Đã lưu cài đặt — áp dụng ngay cả với run đang chạy');
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setSavingSettings(false);
    }
  };

  // Light polling only while a run is live, to keep the drawer's
  // recent-runs card fresh. Full monitoring lives on /workflows/runs.
  const hasActive = runs.some((r) => r.status === 'running');
  useEffect(() => {
    if (!hasActive) return;
    const t = setInterval(() => refresh(true), 5000);
    return () => clearInterval(t);
  }, [hasActive, refresh]);

  // ── Actions ──

  const openEdit = async (name: string) => {
    try {
      const d = await apiFetch<{ fileName: string; content: string }>(
        `/api/workflows/${encodeURIComponent(name)}/definition`);
      setEditor({ mode: 'edit', name, content: d.content });
    } catch (e) {
      message.error(`Không đọc được định nghĩa: ${(e as Error).message}`);
    }
  };

  const saveEditor = async () => {
    if (!editor) return;
    setSaving(true);
    try {
      if (editor.mode === 'create') {
        const r = await apiFetch<{ name: string }>('/api/workflows', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ content: editor.content }),
        });
        message.success(`Đã tạo workflow "${r.name}"`);
      } else {
        await apiFetch<{ name: string }>(
          `/api/workflows/${encodeURIComponent(editor.name!)}/definition`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ content: editor.content }),
          });
        message.success(`Đã lưu "${editor.name}"`);
      }
      setEditor(null);
      refresh(true);
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  const doImport = async (file: File) => {
    try {
      const content = await file.text();
      try {
        const r = await apiFetch<{ name: string }>('/api/workflows', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ content }),
        });
        message.success(`Đã import workflow "${r.name}"`);
        refresh(true);
      } catch (e) {
        const msg = (e as Error).message;
        if (msg.includes('already exists')) {
          Modal.confirm({
            title: 'Workflow đã tồn tại',
            content: `${msg}. Ghi đè bản hiện có?`,
            okText: 'Ghi đè',
            cancelText: 'Huỷ',
            onOk: async () => {
              await apiFetch<{ name: string }>('/api/workflows', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ content, overwrite: true }),
              });
              message.success('Đã ghi đè workflow');
              refresh(true);
            },
          });
        } else {
          message.error(msg);
        }
      }
    } catch {
      message.error('Không đọc được file');
    }
    return false; // prevent antd Upload auto-post
  };

  const doExport = async (name: string) => {
    try {
      const d = await apiFetch<{ fileName: string; content: string }>(
        `/api/workflows/${encodeURIComponent(name)}/definition`);
      const blob = new Blob([d.content], { type: 'text/markdown' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = d.fileName;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      message.error(`Export thất bại: ${(e as Error).message}`);
    }
  };

  const doDelete = async (name: string) => {
    try {
      await apiFetch(`/api/workflows/${encodeURIComponent(name)}`, { method: 'DELETE' });
      message.success(`Đã xoá "${name}" (lịch sử run và workspace được giữ lại)`);
      refresh(true);
    } catch (e) {
      message.error((e as Error).message);
    }
  };

  const openRun = (d: WorkflowDefSummary, preset?: Record<string, string>) => {
    const init: Record<string, string> = {};
    for (const i of d.inputs) init[i.name] = i.default ?? '';
    setRunInputs({ ...init, ...(preset ?? {}) });
    setRunTarget(d);
  };

  const doDraft = async () => {
    if (!draftDesc.trim()) {
      message.warning('Hãy mô tả quy trình trước');
      return;
    }
    setDrafting(true);
    try {
      const r = await apiFetch<{ name: string; content: string }>('/api/workflows/draft', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ description: draftDesc }),
      });
      setDraftOpen(false);
      setEditor({ mode: 'create', content: r.content });
      message.success(`Agent đã soạn bản nháp "${r.name}" — duyệt rồi bấm Lưu`);
    } catch (e) {
      message.error(`Soạn nháp thất bại: ${(e as Error).message}`);
    } finally {
      setDrafting(false);
    }
  };

  const openTune = (d: WorkflowDefSummary) => {
    setTune({
      name: d.name,
      guidance: d.guidance ?? '',
      workspace: d.workspace ?? '',
      steps: d.steps.map((s) => ({
        id: s.id,
        kind: s.kind,
        persona: s.persona,
        guidance: s.guidance ?? '',
        timeout: s.timeout,
      })),
    });
  };

  const saveTune = async () => {
    if (!tune) return;
    setTuning(true);
    try {
      await apiFetch(`/api/workflows/${encodeURIComponent(tune.name)}/definition`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          guidance: tune.guidance,
          workspace: tune.workspace,
          steps: tune.steps.map((s) => ({
            id: s.id,
            guidance: s.guidance,
            ...(s.timeout && s.timeout > 0 ? { timeout: s.timeout } : {}),
          })),
        }),
      });
      message.success(`Đã lưu guidance cho "${tune.name}"`);
      setTune(null);
      refresh(true);
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setTuning(false);
    }
  };

  const startRun = async () => {
    if (!runTarget) return;
    const missing = runTarget.inputs.filter((i) => i.required && !(runInputs[i.name] ?? '').trim());
    if (missing.length) {
      message.warning(`Thiếu input bắt buộc: ${missing.map((m) => m.name).join(', ')}`);
      return;
    }
    setStarting(true);
    try {
      const inputs: Record<string, string> = {};
      for (const [k, v] of Object.entries(runInputs)) if (v.trim() !== '') inputs[k] = v;
      const r = await apiFetch<{ runId: string }>(
        `/api/workflows/${encodeURIComponent(runTarget.name)}/run`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ inputs }),
        });
      setRunTarget(null);
      message.success(`Đã chạy: ${r.runId}`);
      // Theo dõi tiến độ ở màn run monitor.
      navigate(`/workflows/runs?run=${encodeURIComponent(r.runId)}`);
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setStarting(false);
    }
  };

  // ── Render helpers ──

  const defColumns = [
    {
      title: 'Workflow',
      dataIndex: 'name',
      key: 'name',
      render: (_: string, d: WorkflowDefSummary) => (
        <div>
          <Text strong>{d.name}</Text>
          {d.description && (
            <Paragraph type="secondary" style={{ margin: 0, fontSize: 12 }} ellipsis={{ rows: 1 }}>
              {d.description}
            </Paragraph>
          )}
        </div>
      ),
    },
    {
      title: 'Steps',
      key: 'steps',
      width: 90,
      render: (_: unknown, d: WorkflowDefSummary) => <Tag>{d.stepCount} step</Tag>,
    },
    {
      title: 'Inputs',
      key: 'inputs',
      width: 220,
      render: (_: unknown, d: WorkflowDefSummary) => (
        <Space size={4} wrap>
          {d.inputs.map((i) => (
            <Tag key={i.name} color={i.required ? 'blue' : undefined}>{i.name}</Tag>
          ))}
        </Space>
      ),
    },
    {
      title: '',
      key: 'actions',
      width: 210,
      render: (_: unknown, d: WorkflowDefSummary) => (
        <Space size={2}>
          <Tooltip title="Chạy"><Button size="small" type="primary" ghost icon={<CaretRightOutlined />} onClick={() => openRun(d)} /></Tooltip>
          <Tooltip title="Tinh chỉnh guidance"><Button size="small" type="text" icon={<ControlOutlined />} onClick={() => openTune(d)} /></Tooltip>
          <Tooltip title="Chi tiết"><Button size="small" type="text" icon={<EyeOutlined />} onClick={() => setDetail(d)} /></Tooltip>
          <Tooltip title="Sửa file"><Button size="small" type="text" icon={<EditOutlined />} onClick={() => openEdit(d.name)} /></Tooltip>
          <Tooltip title="Export .md"><Button size="small" type="text" icon={<DownloadOutlined />} onClick={() => doExport(d.name)} /></Tooltip>
          <Popconfirm title={`Xoá workflow "${d.name}"?`} okText="Xoá" cancelText="Huỷ" onConfirm={() => doDelete(d.name)}>
            <Button size="small" type="text" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ padding: 16, display: 'flex', flexDirection: 'column', gap: 12, height: '100%' }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
        <Space>
          <ApartmentOutlined style={{ fontSize: 18, color: token.colorPrimary }} />
          <Text strong style={{ fontSize: 16 }}>Workflow templates</Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            định nghĩa quy trình nhiều bước (agent + script)
          </Text>
        </Space>
        <Space>
          <Button size="small" icon={<HistoryOutlined />}
            onClick={() => navigate('/workflows/runs')}>
            Lịch sử run{runs.length > 0 ? ` (${runs.length})` : ''}
          </Button>
          <Button size="small" icon={<RobotOutlined />} onClick={() => setDraftOpen(true)}>
            ✨ Nhờ agent soạn
          </Button>
          <Upload accept=".md,.markdown,.txt" showUploadList={false} beforeUpload={doImport}>
            <Button size="small" icon={<UploadOutlined />}>Import</Button>
          </Upload>
          <Button size="small" icon={<PlusOutlined />} type="primary"
            onClick={() => setEditor({ mode: 'create', content: NEW_WORKFLOW_TEMPLATE })}>
            Thêm workflow
          </Button>
          <Popover
            open={settingsOpen}
            onOpenChange={setSettingsOpen}
            trigger="click"
            placement="bottomRight"
            content={
              <div style={{ width: 300 }}>
                <div style={{ marginBottom: 10 }}>
                  <Text strong style={{ fontSize: 13 }}>Số request LLM song song</Text>
                  <Paragraph type="secondary" style={{ fontSize: 11, margin: '2px 0 4px' }}>
                    Nhiều provider chỉ cho 1 request một lúc. Agent step vượt hạn mức sẽ
                    <b> chờ (pending)</b> — chưa chạy nên không tính timeout.
                  </Paragraph>
                  <InputNumber
                    min={1} max={16} style={{ width: '100%' }}
                    value={settings.llmParallel}
                    onChange={(v) => setSettings((p) => ({ ...p, llmParallel: v ?? 1 }))}
                  />
                </div>
                <div style={{ marginBottom: 12 }}>
                  <Text strong style={{ fontSize: 13 }}>Retry khi không có kết quả</Text>
                  <Paragraph type="secondary" style={{ fontSize: 11, margin: '2px 0 4px' }}>
                    Agent step lỗi session hoặc trả về rỗng sẽ được thử lại chừng này lần
                    trước khi đánh dấu failed.
                  </Paragraph>
                  <InputNumber
                    min={0} max={5} style={{ width: '100%' }}
                    value={settings.agentRetries}
                    onChange={(v) => setSettings((p) => ({ ...p, agentRetries: v ?? 0 }))}
                  />
                </div>
                <Button type="primary" size="small" block loading={savingSettings} onClick={saveSettings}>
                  Lưu
                </Button>
              </div>
            }
          >
            <Tooltip title="Cài đặt thực thi (LLM song song, retry)">
              <Button size="small" icon={<SettingOutlined />} />
            </Tooltip>
          </Popover>
          <Button size="small" icon={<ReloadOutlined />} onClick={() => refresh()} loading={loading} />
        </Space>
      </div>

      {loading ? (
        <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}><Spin /></div>
      ) : defs.length === 0 ? (
        <Empty style={{ marginTop: 48 }}
          description={<span>Chưa có workflow nào — bấm <b>Thêm workflow</b>, <b>Import</b> file .md, hoặc <b>✨ Nhờ agent soạn</b></span>} />
      ) : (
        <Table rowKey="name" size="small" columns={defColumns} dataSource={defs} pagination={false} />
      )}

      {/* ── Detail drawer ── */}
      <Drawer
        title={detail ? `Workflow: ${detail.name}` : ''}
        open={!!detail}
        width={560}
        onClose={() => setDetail(null)}
        extra={detail && (
          <Space>
            <Button size="small" type="primary" ghost icon={<CaretRightOutlined />}
              onClick={() => { openRun(detail); }}>Chạy</Button>
            <Button size="small" icon={<ControlOutlined />} onClick={() => openTune(detail)}>Tinh chỉnh</Button>
            <Button size="small" icon={<EditOutlined />} onClick={() => openEdit(detail.name)}>Sửa file</Button>
          </Space>
        )}
      >
        {detail && (
          <Space direction="vertical" style={{ width: '100%' }} size={12}>
            {detail.description && <Paragraph type="secondary">{detail.description}</Paragraph>}
            {detail.workspace && (
              <Text type="secondary" style={{ fontSize: 12 }}>Workspace: <Text code>{detail.workspace}</Text></Text>
            )}
            {detail.inputs.length > 0 && (
              <Card size="small" title="Inputs">
                {detail.inputs.map((i) => (
                  <div key={i.name} style={{ marginBottom: 4 }}>
                    <Tag color={i.required ? 'blue' : undefined}>{i.name}</Tag>
                    {i.required && <Text type="secondary" style={{ fontSize: 12 }}>bắt buộc</Text>}
                    {i.default !== undefined && <Text type="secondary" style={{ fontSize: 12 }}> mặc định: <Text code>{i.default}</Text></Text>}
                    {i.description && <Text type="secondary" style={{ fontSize: 12 }}> — {i.description}</Text>}
                  </div>
                ))}
              </Card>
            )}
            {detail.guidance && (
              <Card size="small" title="Guidance (áp dụng mọi agent step)">
                <Paragraph style={{ whiteSpace: 'pre-wrap', fontSize: 12, margin: 0 }}>{detail.guidance}</Paragraph>
              </Card>
            )}
            <Card size="small" title={`Steps (${detail.steps.length})`}>
              {detail.steps.map((s) => (
                <div key={s.id} style={{
                  padding: '8px 10px', marginBottom: 8, borderRadius: 8,
                  border: `1px solid ${token.colorBorderSecondary}`,
                }}>
                  <Space size={6} wrap>
                    <Text strong>{s.id}</Text>
                    <Tag color={s.kind === 'agent' ? 'geekblue' : 'purple'}>{s.kind}</Tag>
                    {s.persona && <Tag>{s.persona}</Tag>}
                    {s.timeout && <Text type="secondary" style={{ fontSize: 12 }}>{s.timeout}s</Text>}
                  </Space>
                  {(s.dependsOn?.length ?? 0) > 0 && (
                    <div style={{ marginTop: 4 }}>
                      <Text type="secondary" style={{ fontSize: 12 }}>← chờ: {s.dependsOn!.join(', ')}</Text>
                    </div>
                  )}
                  {s.guidance && (
                    <Paragraph type="secondary" style={{ fontSize: 12, margin: '4px 0 0' }} ellipsis={{ rows: 2 }}>
                      {s.guidance}
                    </Paragraph>
                  )}
                </div>
              ))}
            </Card>
            <Card size="small" title="Run gần đây"
              extra={<Button size="small" type="link" icon={<HistoryOutlined />}
                onClick={() => navigate('/workflows/runs')}>Xem tất cả</Button>}>
              {runs.filter((r) => r.workflowName === detail.name).slice(0, 5).map((r) => (
                <div key={r.id} style={{ marginBottom: 6, cursor: 'pointer' }}
                  onClick={() => navigate(`/workflows/runs?run=${encodeURIComponent(r.id)}`)}>
                  <Space size={6}>
                    <Text code style={{ fontSize: 12 }}>{r.id}</Text>
                    <Tag color={RUN_STATUS_COLOR[r.status]}>{r.status}</Tag>
                    <Text type="secondary" style={{ fontSize: 12 }}>{new Date(r.createdAt).toLocaleString('vi-VN')}</Text>
                  </Space>
                </div>
              ))}
              {runs.filter((r) => r.workflowName === detail.name).length === 0 && (
                <Text type="secondary" style={{ fontSize: 12 }}>Chưa chạy lần nào</Text>
              )}
            </Card>
          </Space>
        )}
      </Drawer>

      {/* ── Editor modal (create / edit) ── */}
      <Modal
        title={editor?.mode === 'create' ? 'Thêm workflow mới' : `Sửa workflow: ${editor?.name}`}
        open={!!editor}
        width={720}
        okText="Lưu"
        cancelText="Huỷ"
        confirmLoading={saving}
        onOk={saveEditor}
        onCancel={() => setEditor(null)}
      >
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          File markdown với YAML frontmatter. Nội dung được kiểm tra hợp lệ (DAG, persona, vòng lặp…) trước khi lưu.
        </Paragraph>
        <Input.TextArea
          value={editor?.content ?? ''}
          onChange={(e) => setEditor((prev) => (prev ? { ...prev, content: e.target.value } : prev))}
          autoSize={{ minRows: 18, maxRows: 28 }}
          style={{ fontFamily: 'monospace', fontSize: 12 }}
        />
      </Modal>

      {/* ── Draft-by-agent modal ── */}
      <Modal
        title="✨ Nhờ agent soạn workflow"
        open={draftOpen}
        okText={drafting ? 'Đang soạn…' : 'Soạn nháp'}
        cancelText="Huỷ"
        confirmLoading={drafting}
        onOk={doDraft}
        onCancel={() => { if (!drafting) setDraftOpen(false); }}
      >
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          Mô tả quy trình bằng lời — agent sẽ chọn persona phù hợp trong số persona hiện có,
          dựng các bước + guidance, và trả về bản nháp để bạn duyệt trước khi lưu.
          Quá trình mất khoảng 30–120 giây.
        </Paragraph>
        <Input.TextArea
          value={draftDesc}
          onChange={(e) => setDraftDesc(e.target.value)}
          placeholder="VD: Mỗi tuần điều tra một chủ đề theo 3 góc (kỹ thuật, thị trường, đối thủ) song song, tải dữ liệu giá bằng script, rồi tổng hợp thành báo cáo."
          autoSize={{ minRows: 4, maxRows: 8 }}
          disabled={drafting}
        />
      </Modal>

      {/* ── Tune-guidance modal ── */}
      <Modal
        title={tune ? `Tinh chỉnh guidance: ${tune.name}` : ''}
        open={!!tune}
        width={680}
        okText="Lưu"
        cancelText="Huỷ"
        confirmLoading={tuning}
        onOk={saveTune}
        onCancel={() => setTune(null)}
      >
        {tune && (
          <Space direction="vertical" style={{ width: '100%' }} size={12}>
            <Paragraph type="secondary" style={{ fontSize: 12, margin: 0 }}>
              Guidance là lớp <b>luật</b> của agent step (persona = danh tính, prompt = nhiệm vụ).
              Sửa ở đây không đụng vào cấu trúc DAG; để trống = xoá guidance.
            </Paragraph>
            <div>
              <Text strong style={{ fontSize: 13 }}>Guidance toàn workflow</Text>
              <Text type="secondary" style={{ fontSize: 12 }}> — áp cho mọi agent step</Text>
              <Input.TextArea
                value={tune.guidance}
                onChange={(e) => setTune((p) => (p ? { ...p, guidance: e.target.value } : p))}
                autoSize={{ minRows: 2, maxRows: 6 }}
                style={{ marginTop: 4 }}
              />
            </div>
            <div>
              <Text strong style={{ fontSize: 13 }}>Workspace</Text>
              <Text type="secondary" style={{ fontSize: 12 }}> — cwd của mọi step, giữ giữa các run; để trống = thư mục mặc định</Text>
              <Input
                value={tune.workspace}
                onChange={(e) => setTune((p) => (p ? { ...p, workspace: e.target.value } : p))}
                placeholder="/path/to/workspace"
                style={{ marginTop: 4 }}
              />
            </div>
            {tune.steps.map((s, idx) => (
              <div key={s.id} style={{
                padding: '10px 12px', borderRadius: 8,
                border: `1px solid ${token.colorBorderSecondary}`,
              }}>
                <Space size={6} wrap>
                  <Text strong style={{ fontSize: 13 }}>{s.id}</Text>
                  <Tag color={s.kind === 'agent' ? 'geekblue' : 'purple'}>{s.kind}</Tag>
                  {s.persona && <Tag>{s.persona}</Tag>}
                  <span>
                    <Text type="secondary" style={{ fontSize: 12 }}>timeout (giây): </Text>
                    <Input
                      size="small"
                      style={{ width: 90 }}
                      type="number"
                      min={1}
                      value={s.timeout ?? ''}
                      placeholder="600"
                      onChange={(e) => {
                        const v = e.target.value ? Number(e.target.value) : undefined;
                        setTune((p) => {
                          if (!p) return p;
                          const steps = [...p.steps];
                          steps[idx] = { ...steps[idx], timeout: v };
                          return { ...p, steps };
                        });
                      }}
                    />
                  </span>
                </Space>
                {s.kind === 'agent' && (
                  <Input.TextArea
                    value={s.guidance}
                    onChange={(e) => {
                      const v = e.target.value;
                      setTune((p) => {
                        if (!p) return p;
                        const steps = [...p.steps];
                        steps[idx] = { ...steps[idx], guidance: v };
                        return { ...p, steps };
                      });
                    }}
                    placeholder="Luật cho step này: định dạng output, phạm vi, giọng điệu…"
                    autoSize={{ minRows: 2, maxRows: 6 }}
                    style={{ marginTop: 8 }}
                  />
                )}
              </div>
            ))}
          </Space>
        )}
      </Modal>

      {/* ── Run modal ── */}
      <Modal
        title={runTarget ? `Chạy workflow: ${runTarget.name}` : ''}
        open={!!runTarget}
        okText="Chạy"
        cancelText="Huỷ"
        confirmLoading={starting}
        onOk={startRun}
        onCancel={() => setRunTarget(null)}
      >
        {runTarget && runTarget.inputs.length === 0 && (
          <Text type="secondary">Workflow này không cần input.</Text>
        )}
        {runTarget?.inputs.map((i) => (
          <div key={i.name} style={{ marginBottom: 10 }}>
            <Text strong style={{ fontSize: 13 }}>
              {i.name}{i.required && <Text type="danger"> *</Text>}
            </Text>
            {i.description && (
              <Paragraph type="secondary" style={{ fontSize: 12, margin: '0 0 4px' }}>{i.description}</Paragraph>
            )}
            <Input
              placeholder={i.default !== undefined ? `mặc định: ${i.default}` : ''}
              value={runInputs[i.name] ?? ''}
              onChange={(e) => setRunInputs((prev) => ({ ...prev, [i.name]: e.target.value }))}
            />
          </div>
        ))}
      </Modal>
    </div>
  );
}
