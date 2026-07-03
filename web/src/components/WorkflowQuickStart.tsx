import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Button, Divider, Input, Modal, Select, Space, Tag, Typography, message, theme } from 'antd';
import { ApartmentOutlined, CaretRightOutlined, RobotOutlined, SettingOutlined } from '@ant-design/icons';

const { Text, Paragraph } = Typography;

interface WorkflowInputDef {
  name: string;
  required?: boolean;
  default?: string;
  description?: string;
}

interface WorkflowDefSummary {
  name: string;
  description?: string;
  stepCount: number;
  inputs: WorkflowInputDef[];
}

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  if (!res.ok) {
    let detail = '';
    try { detail = (await res.json())?.error ?? ''; } catch { /* not json */ }
    throw new Error(detail || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

/** New-session Workflow tab: pick a saved workflow (fill inputs → run), or
 *  describe a new routine and let a one-shot agent author + save it — the
 *  fresh workflow is auto-selected, ready to run. */
export function WorkflowQuickStart({ onRunSelected }: {
  /** When provided, a started run is opened as a chat "workflow session"
   *  (wfrun:<id>) instead of navigating to the run monitor page. */
  onRunSelected?: (jid: string) => void;
}) {
  const { token } = theme.useToken();
  const navigate = useNavigate();
  const [defs, setDefs] = useState<WorkflowDefSummary[]>([]);
  const [selectedName, setSelectedName] = useState<string | null>(null);
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [starting, setStarting] = useState(false);
  const [desc, setDesc] = useState('');
  const [drafting, setDrafting] = useState(false);
  // Draft review: the agent's output is shown in an editor before anything
  // is saved — the user can tweak it, Save, or Cancel to discard.
  const [review, setReview] = useState<string | null>(null);
  const [savingReview, setSavingReview] = useState(false);

  const selected = defs.find((d) => d.name === selectedName) ?? null;

  const loadDefs = useCallback(async () => {
    try {
      const r = await apiFetch<{ workflows: WorkflowDefSummary[] }>('/api/workflows');
      setDefs(r.workflows ?? []);
      return r.workflows ?? [];
    } catch (e) {
      message.error(`Không tải được workflow: ${(e as Error).message}`);
      return [];
    }
  }, []);

  useEffect(() => { loadDefs(); }, [loadDefs]);

  const pick = (name: string, list?: WorkflowDefSummary[]) => {
    setSelectedName(name);
    const d = (list ?? defs).find((x) => x.name === name);
    const init: Record<string, string> = {};
    for (const i of d?.inputs ?? []) init[i.name] = i.default ?? '';
    setInputs(init);
  };

  const run = async () => {
    if (!selected) return;
    const missing = selected.inputs.filter((i) => i.required && !(inputs[i.name] ?? '').trim());
    if (missing.length) {
      message.warning(`Thiếu input bắt buộc: ${missing.map((m) => m.name).join(', ')}`);
      return;
    }
    setStarting(true);
    try {
      const body: Record<string, string> = {};
      for (const [k, v] of Object.entries(inputs)) if (v.trim() !== '') body[k] = v;
      const r = await apiFetch<{ runId: string }>(
        `/api/workflows/${encodeURIComponent(selected.name)}/run`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ inputs: body }),
        });
      message.success(`Đã chạy: ${r.runId}`);
      if (onRunSelected) {
        // Stay in Chat: the run opens as a "workflow session" pane.
        onRunSelected(`wfrun:${r.runId}`);
      } else {
        navigate(`/workflows/runs?run=${encodeURIComponent(r.runId)}`);
      }
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setStarting(false);
    }
  };

  const createWithAi = async () => {
    if (!desc.trim()) {
      message.warning('Hãy mô tả quy trình trước');
      return;
    }
    setDrafting(true);
    try {
      // Agent authors a validated draft (never touches disk) — show it for
      // review; nothing is saved until the user confirms.
      const draft = await apiFetch<{ name: string; content: string }>('/api/workflows/draft', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ description: desc }),
      });
      setReview(draft.content);
    } catch (e) {
      message.error(`Tạo workflow thất bại: ${(e as Error).message}`);
    } finally {
      setDrafting(false);
    }
  };

  const saveReview = async (overwrite = false) => {
    if (review === null) return;
    setSavingReview(true);
    try {
      const saved = await apiFetch<{ name: string }>('/api/workflows', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ content: review, overwrite }),
      });
      const list = await loadDefs();
      pick(saved.name, list);
      setReview(null);
      setDesc('');
      message.success(`Đã lưu workflow "${saved.name}" — điền inputs rồi bấm Chạy`);
    } catch (e) {
      const msg = (e as Error).message;
      if (msg.includes('already exists')) {
        Modal.confirm({
          title: 'Workflow đã tồn tại',
          content: `${msg}. Ghi đè bản hiện có?`,
          okText: 'Ghi đè',
          cancelText: 'Huỷ',
          onOk: () => saveReview(true),
        });
      } else {
        // Validation error after manual edits — keep the editor open.
        message.error(msg);
      }
    } finally {
      setSavingReview(false);
    }
  };

  const cardStyle: React.CSSProperties = {
    background: token.colorBgContainer,
    border: `1px solid ${token.colorBorderSecondary}`,
    borderRadius: 16,
    padding: '16px 20px',
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* ── Pick & run an existing workflow ── */}
      <div style={cardStyle}>
        <Space style={{ marginBottom: 8 }}>
          <ApartmentOutlined style={{ color: token.colorPrimary }} />
          <Text strong>Chạy workflow có sẵn</Text>
        </Space>
        <Select
          showSearch
          style={{ width: '100%' }}
          placeholder={defs.length === 0 ? 'Chưa có workflow nào — tạo bằng AI bên dưới' : 'Chọn workflow…'}
          value={selectedName}
          onChange={(v) => pick(v)}
          optionFilterProp="label"
          options={defs.map((d) => ({
            value: d.name,
            label: d.name,
          }))}
          optionRender={(opt) => {
            const d = defs.find((x) => x.name === opt.value);
            return (
              <div>
                <Text strong style={{ fontSize: 13 }}>{d?.name}</Text>
                <Tag style={{ marginLeft: 6 }}>{d?.stepCount} step</Tag>
                {d?.description && (
                  <div><Text type="secondary" style={{ fontSize: 12 }}>{d.description}</Text></div>
                )}
              </div>
            );
          }}
        />
        {selected && (
          <div style={{ marginTop: 12 }}>
            {selected.description && (
              <Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 8 }}>
                {selected.description}
              </Paragraph>
            )}
            {selected.inputs.map((i) => (
              <div key={i.name} style={{ marginBottom: 8 }}>
                <Text strong style={{ fontSize: 13 }}>
                  {i.name}{i.required && <Text type="danger"> *</Text>}
                </Text>
                {i.description && (
                  <Text type="secondary" style={{ fontSize: 12 }}> — {i.description}</Text>
                )}
                <Input
                  style={{ marginTop: 4 }}
                  placeholder={i.default !== undefined ? `mặc định: ${i.default}` : ''}
                  value={inputs[i.name] ?? ''}
                  onChange={(e) => setInputs((prev) => ({ ...prev, [i.name]: e.target.value }))}
                  onPressEnter={run}
                />
              </div>
            ))}
            <Button type="primary" icon={<CaretRightOutlined />} loading={starting} onClick={run} block>
              Chạy workflow
            </Button>
          </div>
        )}
      </div>

      <Divider plain style={{ margin: '2px 0' }}>
        <Text type="secondary" style={{ fontSize: 12 }}>hoặc</Text>
      </Divider>

      {/* ── Create a new one with the agent ── */}
      <div style={cardStyle}>
        <Space style={{ marginBottom: 8 }}>
          <RobotOutlined style={{ color: token.colorPrimary }} />
          <Text strong>✨ Tạo workflow mới bằng AI agent</Text>
        </Space>
        <Input.TextArea
          value={desc}
          onChange={(e) => setDesc(e.target.value)}
          placeholder="Mô tả quy trình… VD: Mỗi tuần điều tra một chủ đề theo 3 góc song song, tải dữ liệu giá bằng script, rồi tổng hợp thành báo cáo."
          autoSize={{ minRows: 3, maxRows: 6 }}
          disabled={drafting}
        />
        <Button
          style={{ marginTop: 8 }}
          icon={<RobotOutlined />}
          loading={drafting}
          onClick={createWithAi}
          block
        >
          {drafting ? 'Agent đang soạn (30–120s)…' : 'Tạo workflow'}
        </Button>
        <Paragraph type="secondary" style={{ fontSize: 11, margin: '8px 0 0', textAlign: 'center' }}>
          Agent soạn xong sẽ mở editor để bạn duyệt/sửa trước khi lưu.
          {' '}
          <Button type="link" size="small" icon={<SettingOutlined />} style={{ fontSize: 11, padding: 0 }}
            onClick={() => navigate('/plugins?nav=workflows')}>
            Quản lý template
          </Button>
        </Paragraph>
      </div>

      {/* ── Review-draft editor: nothing is saved until the user confirms ── */}
      <Modal
        title="Duyệt bản nháp — sửa nếu cần rồi Lưu"
        open={review !== null}
        width={720}
        okText="Lưu workflow"
        cancelText="Bỏ qua"
        confirmLoading={savingReview}
        onOk={() => saveReview()}
        onCancel={() => {
          setReview(null);
          message.info('Đã bỏ bản nháp — không lưu gì cả');
        }}
      >
        <Paragraph type="secondary" style={{ fontSize: 12 }}>
          Nội dung được kiểm tra hợp lệ (DAG, persona, vòng lặp…) khi lưu; bấm <b>Bỏ qua</b> để
          hủy bản nháp.
        </Paragraph>
        <Input.TextArea
          value={review ?? ''}
          onChange={(e) => setReview(e.target.value)}
          autoSize={{ minRows: 16, maxRows: 26 }}
          style={{ fontFamily: 'monospace', fontSize: 12 }}
        />
      </Modal>
    </div>
  );
}
