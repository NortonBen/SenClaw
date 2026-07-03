import { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { Button, Dropdown, Empty, Modal, Spin, Tag, Tooltip, Typography, message, theme } from 'antd';
import {
  ArrowLeftOutlined, CheckOutlined, ClockCircleOutlined, DeleteOutlined, EditOutlined,
  FilterOutlined, FolderOutlined, MenuOutlined, MoreOutlined, ReloadOutlined,
  SortAscendingOutlined, StopOutlined,
} from '@ant-design/icons';
import {
  RUN_STATUS_COLOR, RunInputsModal, WorkflowDefSummary, WorkflowRun,
  WorkflowRunDetailView, apiFetch, deleteRun, fmtTime, renameRun, runTitle,
} from '../components/workflows/workflowShared';

const { Text } = Typography;

type GroupMode = 'workflow' | 'date' | 'none';
type SortMode = 'recent' | 'created' | 'name';
const PAGE = 10;

function dateBucket(iso: string): string {
  const d = new Date(iso);
  const now = new Date();
  const sameDay = (a: Date, b: Date) =>
    a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
  if (sameDay(d, now)) return 'Hôm nay';
  const y = new Date(now); y.setDate(now.getDate() - 1);
  if (sameDay(d, y)) return 'Hôm qua';
  const diff = (now.getTime() - d.getTime()) / 86_400_000;
  if (diff <= 7) return '7 ngày qua';
  if (diff <= 30) return '30 ngày qua';
  return 'Cũ hơn';
}

/** Run monitor: grouped/sorted run list (left, 10-per-page) + full detail of
 *  the selected run (right). Templates live in Plugins → Workflow. */
export function WorkflowRunsPage() {
  const { token } = theme.useToken();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const [runs, setRuns] = useState<WorkflowRun[]>([]);
  const [defs, setDefs] = useState<WorkflowDefSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [rerunTarget, setRerunTarget] = useState<WorkflowDefSummary | null>(null);
  const [groupMode, setGroupMode] = useState<GroupMode>('date');
  const [sortMode, setSortMode] = useState<SortMode>('recent');
  const [limit, setLimit] = useState(PAGE);

  const selectedId = searchParams.get('run');
  const selected = useMemo(
    () => runs.find((r) => r.id === selectedId) ?? runs[0] ?? null,
    [runs, selectedId],
  );

  const refresh = useCallback(async (silent = false) => {
    try {
      const [r, d] = await Promise.all([
        apiFetch<{ runs: WorkflowRun[] }>('/api/workflows/runs'),
        apiFetch<{ workflows: WorkflowDefSummary[] }>('/api/workflows'),
      ]);
      setRuns(r.runs ?? []);
      setDefs(d.workflows ?? []);
    } catch (e) {
      if (!silent) message.error(`Không tải được runs: ${(e as Error).message}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);
  const hasActive = runs.some((r) => r.status === 'running');
  useEffect(() => {
    const t = setInterval(() => refresh(true), hasActive ? 2000 : 6000);
    return () => clearInterval(t);
  }, [hasActive, refresh]);

  // ── Sort → limit → group ──
  const sorted = useMemo(() => {
    const arr = [...runs];
    if (sortMode === 'name') {
      arr.sort((a, b) => runTitle(a).localeCompare(runTitle(b)));
    } else {
      // recent = last activity (completedAt||createdAt); created = createdAt.
      const ts = (r: WorkflowRun) =>
        new Date(sortMode === 'recent' ? (r.completedAt ?? r.createdAt) : r.createdAt).getTime();
      arr.sort((a, b) => ts(b) - ts(a));
    }
    return arr;
  }, [runs, sortMode]);

  const visible = sorted.slice(0, limit);
  const grouped = useMemo(() => {
    if (groupMode === 'none') return [{ label: '', items: visible }];
    const map = new Map<string, WorkflowRun[]>();
    for (const r of visible) {
      const key = groupMode === 'workflow' ? r.workflowName : dateBucket(r.createdAt);
      (map.get(key) ?? map.set(key, []).get(key)!).push(r);
    }
    return [...map.entries()].map(([label, items]) => ({ label, items }));
  }, [visible, groupMode]);

  // ── Actions ──
  const cancelRun = async (id: string) => {
    try {
      await apiFetch(`/api/workflows/runs/${encodeURIComponent(id)}/cancel`, { method: 'POST' });
      message.success(`Đã gửi yêu cầu huỷ ${id}`);
      refresh(true);
    } catch (e) { message.error((e as Error).message); }
  };

  const rerun = (r: WorkflowRun) => {
    const d = defs.find((x) => x.name === r.workflowName);
    if (!d) { message.error(`Định nghĩa "${r.workflowName}" không còn tồn tại`); return; }
    setRerunTarget(d);
  };

  const doRename = (r: WorkflowRun) => {
    let value = r.label ?? '';
    Modal.confirm({
      title: 'Đổi tên run',
      icon: <EditOutlined />,
      content: (
        <input
          defaultValue={value}
          placeholder={r.id}
          onChange={(e) => { value = e.target.value; }}
          className="w-full text-sm px-2 py-1 rounded border outline-none"
          style={{ borderColor: token.colorBorderSecondary, background: 'transparent', color: token.colorText }}
        />
      ),
      okText: 'Lưu', cancelText: 'Huỷ',
      onOk: async () => { await renameRun(r.id, value); refresh(true); },
    });
  };

  const doDelete = (r: WorkflowRun) => {
    Modal.confirm({
      title: `Xoá run "${runTitle(r)}"?`,
      content: 'Chỉ xoá bản ghi lịch sử — file trong workspace được giữ lại.',
      okText: 'Xoá', okButtonProps: { danger: true }, cancelText: 'Huỷ',
      onOk: async () => {
        try {
          await deleteRun(r.id);
          if (selectedId === r.id) setSearchParams({});
          refresh(true);
        } catch (e) { message.error((e as Error).message); }
      },
    });
  };

  const groupSortMenu = {
    items: [
      {
        key: 'g', type: 'group' as const, label: 'Nhóm theo',
        children: [
          { key: 'g-wf', label: 'Workflow', icon: <FolderOutlined />, onClick: () => setGroupMode('workflow'), extra: groupMode === 'workflow' ? <CheckOutlined /> : undefined },
          { key: 'g-date', label: 'Ngày', icon: <ClockCircleOutlined />, onClick: () => setGroupMode('date'), extra: groupMode === 'date' ? <CheckOutlined /> : undefined },
          { key: 'g-none', label: 'Không nhóm', icon: <MenuOutlined />, onClick: () => setGroupMode('none'), extra: groupMode === 'none' ? <CheckOutlined /> : undefined },
        ],
      },
      { type: 'divider' as const },
      {
        key: 's', type: 'group' as const, label: 'Sắp xếp',
        children: [
          { key: 's-recent', label: 'Hoạt động gần đây', icon: <ClockCircleOutlined />, onClick: () => setSortMode('recent'), extra: sortMode === 'recent' ? <CheckOutlined /> : undefined },
          { key: 's-created', label: 'Ngày tạo', icon: <SortAscendingOutlined />, onClick: () => setSortMode('created'), extra: sortMode === 'created' ? <CheckOutlined /> : undefined },
          { key: 's-name', label: 'Tên A–Z', icon: <SortAscendingOutlined />, onClick: () => setSortMode('name'), extra: sortMode === 'name' ? <CheckOutlined /> : undefined },
        ],
      },
    ],
  };

  const runItemMenu = (r: WorkflowRun) => ({
    items: [
      { key: 'rename', label: 'Đổi tên', icon: <EditOutlined />, onClick: () => doRename(r) },
      ...(r.status === 'running'
        ? [{ key: 'cancel', label: 'Huỷ run', icon: <StopOutlined />, danger: true, onClick: () => cancelRun(r.id) }]
        : [{ key: 'delete', label: 'Xoá', icon: <DeleteOutlined />, danger: true, onClick: () => doDelete(r) }]),
    ],
  });

  return (
    <div className="flex h-screen" style={{ background: token.colorBgBase }}>
      {/* ── Left: run list ── */}
      <div style={{
        width: 330, borderRight: `1px solid ${token.colorBorderSecondary}`,
        display: 'flex', flexDirection: 'column',
      }}>
        <div style={{
          padding: '12px 12px 12px 8px', display: 'flex', alignItems: 'center', gap: 4,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
        }}>
          <Button size="small" type="text" icon={<ArrowLeftOutlined />}
            onClick={() => navigate('/plugins?nav=workflows')} />
          <Text strong style={{ fontSize: 15, flex: 1 }}>Workflow runs</Text>
          <Dropdown trigger={['click']} placement="bottomRight" menu={groupSortMenu}>
            <Tooltip title="Nhóm & sắp xếp">
              <Button size="small" type="text" icon={<FilterOutlined />} />
            </Tooltip>
          </Dropdown>
          <Button size="small" type="text" icon={<ReloadOutlined />} onClick={() => refresh()} />
        </div>
        <div style={{ flex: 1, overflowY: 'auto', padding: 8 }}>
          {loading ? (
            <div style={{ display: 'flex', justifyContent: 'center', padding: 24 }}><Spin /></div>
          ) : runs.length === 0 ? (
            <Empty style={{ marginTop: 48 }} description="Chưa có run nào" />
          ) : grouped.map((g) => (
            <div key={g.label || '_'}>
              {g.label && (
                <div className="px-2 pt-2 pb-1">
                  <span className="text-[10px] font-semibold tracking-widest uppercase"
                    style={{ color: token.colorTextTertiary }}>{g.label}</span>
                </div>
              )}
              {g.items.map((r) => (
                <div
                  key={r.id}
                  onClick={() => setSearchParams({ run: r.id })}
                  className="group"
                  style={{
                    padding: '9px 12px', marginBottom: 6, borderRadius: 10, cursor: 'pointer',
                    border: `1px solid ${selected?.id === r.id ? token.colorPrimary : token.colorBorderSecondary}`,
                    background: selected?.id === r.id ? token.colorPrimaryBg : token.colorBgContainer,
                    transition: 'border-color .15s',
                  }}
                >
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                    <Text strong style={{ fontSize: 13, flex: 1 }} ellipsis>{runTitle(r)}</Text>
                    <Tag color={RUN_STATUS_COLOR[r.status]} style={{ marginRight: 0 }}>{r.status}</Tag>
                    <Dropdown trigger={['click']} placement="bottomRight" menu={runItemMenu(r)}>
                      <Button size="small" type="text" icon={<MoreOutlined />}
                        className="opacity-0 group-hover:opacity-100"
                        style={{ width: 20, height: 20, padding: 0 }}
                        onClick={(e) => e.stopPropagation()} />
                    </Dropdown>
                  </div>
                  <div style={{ display: 'flex', gap: 8 }}>
                    <Text type="secondary" style={{ fontSize: 11, flex: 1 }} ellipsis>
                      {r.workflowName}
                    </Text>
                    <Text type="secondary" style={{ fontSize: 11 }}>{fmtTime(r.createdAt)}</Text>
                  </div>
                </div>
              ))}
            </div>
          ))}
          {sorted.length > limit && (
            <Button block size="small" type="dashed" style={{ marginTop: 4, borderRadius: 8 }}
              onClick={() => setLimit((n) => n + PAGE)}>
              Xem thêm ({sorted.length - limit})
            </Button>
          )}
        </div>
      </div>

      {/* ── Right: run detail (shared view) ── */}
      <div style={{ flex: 1, overflowY: 'auto', padding: 20 }}>
        {!selected ? (
          <Empty style={{ marginTop: 96 }} description="Chọn một run để xem chi tiết" />
        ) : (
          <WorkflowRunDetailView
            run={selected}
            onCancel={() => cancelRun(selected.id)}
            onRerun={() => rerun(selected)}
            onRenamed={() => refresh(true)}
            onDeleted={() => { setSearchParams({}); refresh(true); }}
          />
        )}
      </div>

      <RunInputsModal
        target={rerunTarget}
        presetInputs={selected?.inputs}
        onClose={() => setRerunTarget(null)}
        onStarted={(id) => { setSearchParams({ run: id }); refresh(true); }}
      />
    </div>
  );
}
