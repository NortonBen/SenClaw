import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { Button, Dropdown, Input, Modal, Spin, Typography, message, theme } from 'antd';
import {
  ApartmentOutlined, DeleteOutlined, DownOutlined, EditOutlined, MoreOutlined,
  RightOutlined, StopOutlined,
} from '@ant-design/icons';
import {
  WorkflowRun, apiFetch, deleteRun, renameRun, runTitle, wfRunJid,
} from './workflowShared';

const { Text } = Typography;

const STATUS_DOT: Record<WorkflowRun['status'], string> = {
  running: '#5BBFE8',
  done: '#52c41a',
  'partial-failed': '#faad14',
  cancelled: '#8c8c8c',
  interrupted: '#ff4d4f',
};

/** "Workflows" section in the chat sidebar. Shows EVERY running run plus the
 *  most recent finished ones up to 5 items total, then a "More" link into the
 *  run monitor. Items can be renamed/deleted from the ⋯ menu. */
export function WorkflowSidebarSection({ selectedJid, onSelect }: {
  selectedJid: string | null;
  onSelect: (jid: string) => void;
}) {
  const { token } = theme.useToken();
  const navigate = useNavigate();
  const [runs, setRuns] = useState<WorkflowRun[]>([]);
  const [collapsed, setCollapsed] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const r = await apiFetch<{ runs: WorkflowRun[] }>('/api/workflows/runs');
      setRuns(r.runs ?? []);
    } catch { /* daemon may be restarting — next poll retries */ }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);
  const hasActive = runs.some((r) => r.status === 'running');
  useEffect(() => {
    const t = setInterval(refresh, hasActive ? 4000 : 20_000);
    return () => clearInterval(t);
  }, [hasActive, refresh]);

  if (runs.length === 0) return null;

  // ALL running runs + recent finished, minimum 5 rows total.
  const running = runs.filter((r) => r.status === 'running');
  const finished = runs.filter((r) => r.status !== 'running');
  const visible = [...running, ...finished.slice(0, Math.max(0, 5 - running.length))];

  const doRename = (r: WorkflowRun) => {
    let value = r.label ?? '';
    Modal.confirm({
      title: 'Đổi tên run',
      icon: <EditOutlined />,
      content: (
        <Input defaultValue={value} placeholder={r.id}
          onChange={(e) => { value = e.target.value; }} />
      ),
      okText: 'Lưu', cancelText: 'Huỷ',
      onOk: async () => { await renameRun(r.id, value); refresh(); },
    });
  };

  const doDelete = (r: WorkflowRun) => {
    Modal.confirm({
      title: `Xoá run "${runTitle(r)}"?`,
      content: 'Chỉ xoá bản ghi lịch sử — file trong workspace được giữ lại.',
      okText: 'Xoá', okButtonProps: { danger: true }, cancelText: 'Huỷ',
      onOk: async () => {
        try { await deleteRun(r.id); refresh(); }
        catch (e) { message.error((e as Error).message); }
      },
    });
  };

  const cancelRun = async (r: WorkflowRun) => {
    try {
      await apiFetch(`/api/workflows/runs/${encodeURIComponent(r.id)}/cancel`, { method: 'POST' });
      message.success(`Đã gửi yêu cầu huỷ ${r.id}`);
      refresh();
    } catch (e) { message.error((e as Error).message); }
  };

  return (
    <div>
      <div
        className="px-4 pt-3 pb-1 flex items-center cursor-pointer select-none"
        onClick={() => setCollapsed((c) => !c)}
      >
        <span className="text-[10px] font-semibold tracking-widest uppercase flex-1"
          style={{ color: token.colorTextTertiary }}>
          Workflows
        </span>
        {collapsed
          ? <RightOutlined style={{ fontSize: 9, color: token.colorTextTertiary }} />
          : <DownOutlined style={{ fontSize: 9, color: token.colorTextTertiary }} />}
      </div>
      {!collapsed && (
        <>
          {visible.map((r) => {
            const jid = wfRunJid(r.id);
            const isSelected = selectedJid === jid;
            const doneSteps = r.steps.filter((s) => s.status === 'done').length;
            return (
              <div
                key={r.id}
                onClick={() => onSelect(jid)}
                className="group flex items-center gap-2 px-3 py-1.5 mx-2 rounded-lg cursor-pointer transition-colors"
                style={{ background: isSelected ? `${token.colorPrimary}14` : 'transparent' }}
                onMouseEnter={(e) => { if (!isSelected) (e.currentTarget as HTMLDivElement).style.background = token.colorFillAlter; }}
                onMouseLeave={(e) => { if (!isSelected) (e.currentTarget as HTMLDivElement).style.background = 'transparent'; }}
              >
                {r.status === 'running'
                  ? <Spin size="small" style={{ transform: 'scale(0.7)', width: 14 }} />
                  : <ApartmentOutlined style={{ fontSize: 13, color: STATUS_DOT[r.status] }} />}
                <div className="flex-1 min-w-0">
                  <div className="truncate text-xs"
                    style={{ color: isSelected ? token.colorPrimary : token.colorText, fontWeight: isSelected ? 500 : 400 }}>
                    {runTitle(r)}
                  </div>
                  <div className="text-[10px]" style={{ color: token.colorTextTertiary }}>
                    {r.status} · {doneSteps}/{r.steps.length} steps
                  </div>
                </div>
                <Dropdown
                  trigger={['click']}
                  placement="bottomRight"
                  menu={{
                    items: [
                      { key: 'rename', label: 'Đổi tên', icon: <EditOutlined />, onClick: () => doRename(r) },
                      ...(r.status === 'running'
                        ? [{ key: 'cancel', label: 'Huỷ run', icon: <StopOutlined />, danger: true, onClick: () => cancelRun(r) }]
                        : [{ key: 'delete', label: 'Xoá', icon: <DeleteOutlined />, danger: true, onClick: () => doDelete(r) }]),
                    ],
                  }}
                >
                  <Button size="small" type="text" icon={<MoreOutlined />}
                    className="opacity-0 group-hover:opacity-100 flex-shrink-0"
                    style={{ width: 20, height: 20, padding: 0 }}
                    onClick={(e) => e.stopPropagation()} />
                </Dropdown>
              </div>
            );
          })}
          {runs.length > visible.length && (
            <div className="px-3 mx-2">
              <Button type="link" size="small" style={{ fontSize: 11, padding: '0 4px' }}
                onClick={() => navigate('/workflows/runs')}>
                Xem thêm {runs.length - visible.length} workflow →
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
