import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Button, Empty, Spin, Tooltip, Typography, message, theme } from 'antd';
import { ApartmentOutlined, ExportOutlined } from '@ant-design/icons';
import {
  RunInputsModal, WorkflowDefSummary, WorkflowRun, WorkflowRunDetailView,
  apiFetch, runTitle, wfRunJid,
} from './workflowShared';
import { WorkflowActivityFeed } from './WorkflowActivityFeed';

const { Text } = Typography;

/** Read-only "workflow session" shown in place of the chat conversation:
 *  header + live step-by-step flow activity. No composer — flow info only. */
export function WorkflowSessionPane({ runId, onSelectSession }: {
  runId: string;
  /** Select another workflow session (e.g. the run created by Re-run). */
  onSelectSession: (jid: string) => void;
}) {
  const { token } = theme.useToken();
  const navigate = useNavigate();
  const [runs, setRuns] = useState<WorkflowRun[]>([]);
  const [defs, setDefs] = useState<WorkflowDefSummary[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [rerunTarget, setRerunTarget] = useState<WorkflowDefSummary | null>(null);
  // Activity panel: default collapsed (slim strip with the action count).
  const [feedOpen, setFeedOpen] = useState(false);

  const run = runs.find((r) => r.id === runId) ?? null;

  const refresh = useCallback(async () => {
    try {
      const [r, d] = await Promise.all([
        apiFetch<{ runs: WorkflowRun[] }>('/api/workflows/runs'),
        apiFetch<{ workflows: WorkflowDefSummary[] }>('/api/workflows'),
      ]);
      setRuns(r.runs ?? []);
      setDefs(d.workflows ?? []);
    } catch { /* transient — next poll retries */ }
    setLoaded(true);
  }, []);

  useEffect(() => { refresh(); }, [refresh, runId]);

  // Poll fast while the run is live; slow once terminal.
  const active = run?.status === 'running';
  useEffect(() => {
    const t = setInterval(refresh, active || !loaded ? 2500 : 10_000);
    return () => clearInterval(t);
  }, [active, loaded, refresh]);

  const cancel = async () => {
    if (!run) return;
    try {
      await apiFetch(`/api/workflows/runs/${encodeURIComponent(run.id)}/cancel`, { method: 'POST' });
      message.success(`Đã gửi yêu cầu huỷ ${run.id}`);
      refresh();
    } catch (e) {
      message.error((e as Error).message);
    }
  };

  const rerun = () => {
    if (!run) return;
    const d = defs.find((x) => x.name === run.workflowName);
    if (!d) {
      message.error(`Định nghĩa "${run.workflowName}" không còn tồn tại`);
      return;
    }
    setRerunTarget(d);
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header — flow-flavored, no chat affordances. */}
      <div style={{
        display: 'flex', alignItems: 'center', gap: 8, padding: '10px 16px',
        borderBottom: `1px solid ${token.colorBorderSecondary}`,
      }}>
        <ApartmentOutlined style={{ color: token.colorPrimary, fontSize: 16 }} />
        <Text strong style={{ fontSize: 14, flex: 1 }} ellipsis>
          {run ? runTitle(run) : runId}
        </Text>
        <Text type="secondary" style={{ fontSize: 11 }}>workflow session</Text>
        <Tooltip title="Mở run monitor">
          <Button size="small" type="text" icon={<ExportOutlined />}
            onClick={() => navigate(`/workflows/runs?run=${encodeURIComponent(runId)}`)} />
        </Tooltip>
      </div>

      {/* Body — left: live agent activity (think / tool calls / messages);
          right: the read-only flow view. */}
      <div style={{ flex: 1, display: 'flex', minHeight: 0 }}>
        <div style={{
          width: feedOpen ? 340 : 40,
          minWidth: feedOpen ? 260 : 40,
          borderRight: `1px solid ${token.colorBorderSecondary}`,
          background: token.colorFillQuaternary, minHeight: 0,
          transition: 'width .15s',
        }}>
          <WorkflowActivityFeed
            runId={runId}
            active={active ?? false}
            collapsed={!feedOpen}
            onToggle={() => setFeedOpen((v) => !v)}
          />
        </div>
        <div style={{ flex: 1, overflowY: 'auto', padding: 20 }}>
          {!loaded ? (
            <div style={{ display: 'flex', justifyContent: 'center', padding: 48 }}><Spin /></div>
          ) : !run ? (
            <Empty style={{ marginTop: 64 }}
              description={`Không tìm thấy run "${runId}" (lịch sử có thể đã xoay vòng)`} />
          ) : (
            <WorkflowRunDetailView
              run={run}
              onCancel={cancel}
              onRerun={rerun}
              onRenamed={refresh}
              onDeleted={refresh}
            />
          )}
        </div>
      </div>

      <RunInputsModal
        target={rerunTarget}
        presetInputs={run?.inputs}
        onClose={() => setRerunTarget(null)}
        onStarted={(id) => onSelectSession(wfRunJid(id))}
      />
    </div>
  );
}
