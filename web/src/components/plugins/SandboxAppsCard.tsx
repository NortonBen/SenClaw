import React, { useCallback, useEffect, useState } from 'react';
import { Alert, Button, Card, Space, Table, Tag, Tooltip, Typography, message } from 'antd';
import { Modal } from 'antd';
import { SafetyCertificateOutlined, ReloadOutlined, MonitorOutlined } from '@ant-design/icons';
import SpaceAppSandboxModal from '../settings/SpaceAppSandboxModal';
import AppRuntimePanel from '../space/AppRuntimePanel';

const { Text } = Typography;

interface AppRow {
  id: string;
  name: string;
  icon: string | null;
  config: {
    enabled: boolean;
    readMode: string;
    network: string;
    hosts: string[];
    daemonApi: boolean;
    folders: number;
  };
  running: boolean;
  /** Up, but started by someone other than this daemon (an orphan it adopted). */
  adopted: boolean;
  isolation: string | null;
  pid: number | null;
  port: number | null;
  uptimeMs: number | null;
  launches: number;
  cpu: number | null;
  rssMb: number | null;
  processes: number | null;
  proxy: { port: number; stats: { allowed: number; denied: number; recentDenied: string[] } } | null;
}

interface Overview {
  apps: AppRow[];
  caps: { isolation: string; enforceable: boolean; networkEnforceable: boolean };
}

const NET_LABEL: Record<string, { vi: string; en: string; color?: string }> = {
  all: { vi: 'toàn bộ', en: 'everything' },
  hosts: { vi: 'chỉ vài trang', en: 'only some sites', color: 'blue' },
  off: { vi: 'không có mạng', en: 'no network', color: 'green' },
};

// Sorting, matching the desktop card key for key. Numbers missing (an app that
// is not running) sort below 0.0% rather than above it.
const num_ = (v: number | null | undefined) => (v == null ? -1 : v);
const byName = (a: AppRow, b: AppRow) =>
  (a.name ?? a.id).toLowerCase().localeCompare((b.name ?? b.id).toLowerCase());
/**
 * Ties fall back to the name (flipped along with the column, as antd negates the
 * whole comparator) instead of leaving equal rows in arbitrary order.
 */
const withNameTiebreak =
  (f: (a: AppRow, b: AppRow) => number) => (a: AppRow, b: AppRow) =>
    f(a, b) || byName(a, b);

const uptime = (ms: number) => {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
};

/**
 * Every Space App that has a server process, and what the sandbox is actually
 * doing to it — the fleet view for the Sandbox screen.
 *
 * The column that earns its place is "đang chạy": an app can be *configured* as
 * confined and be running unconfined, because a profile is fixed at launch and
 * the settings can be edited afterwards. This is the only screen where that gap
 * is visible, so it says so and offers the restart that closes it.
 */
export const SandboxAppsCard: React.FC<{ vi: boolean }> = ({ vi }) => {
  const [data, setData] = useState<Overview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [configuring, setConfiguring] = useState<{ id: string; name: string } | null>(null);
  const [monitoring, setMonitoring] = useState<{ id: string; name: string } | null>(null);
  const [restarting, setRestarting] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const r = await fetch('/api/space/apps/sandbox-overview');
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      setData(await r.json());
      setError(null);
    } catch (e: any) {
      setError(String(e.message ?? e));
    }
  }, []);

  useEffect(() => {
    void load();
    const t = setInterval(load, 5000);
    return () => clearInterval(t);
  }, [load]);

  const restart = async (id: string) => {
    setRestarting(id);
    try {
      const r = await fetch(`/api/space/apps/${encodeURIComponent(id)}/restart`, { method: 'POST' });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      message.success(vi ? 'Đã khởi động lại app' : 'App restarted');
    } catch (e: any) {
      message.error(String(e.message ?? e));
    } finally {
      setRestarting(null);
      void load();
    }
  };

  const title = vi ? 'Space Apps — sandbox từng app' : 'Space Apps — per-app sandbox';
  // Running first by default, not alphabetical: with 47 apps installed and two
  // running, an A→Z list opens on whatever starts with "A" and the rows worth
  // looking at are three pages down. A clicked column header overrides this.
  const apps = [...(data?.apps ?? [])].sort(
    withNameTiebreak((a, b) => Number(b.running) - Number(a.running)),
  );

  return (
    <>
      <Card
        title={
          <Space>
            <SafetyCertificateOutlined />
            <span>{title}</span>
            <Tag>{apps.length}</Tag>
          </Space>
        }
        size="small"
        style={{ marginTop: 16 }}
        extra={
          <Button size="small" icon={<ReloadOutlined />} onClick={() => void load()}>
            {vi ? 'Làm mới' : 'Refresh'}
          </Button>
        }
      >
        {error && <Alert type="error" showIcon className="mb-2" message={error} />}
        {data && !data.caps.enforceable && (
          <Alert
            type="warning"
            showIcon
            className="mb-2"
            message={
              vi
                ? `Máy này không cách ly được app đang phục vụ (cơ chế: ${data.caps.isolation}) — các công tắc dưới đây được lưu nhưng không cưỡng chế.`
                : `This machine cannot confine a served app (isolation: ${data.caps.isolation}) — the switches below are stored but not enforced.`
            }
          />
        )}
        <Table<AppRow>
          size="small"
          rowKey="id"
          dataSource={apps}
          // Ten per page, matching the desktop card: 47 installed apps is a
          // normal number and an unpaged list buries every other card here.
          pagination={{ pageSize: 10, hideOnSinglePage: true, showSizeChanger: false }}
          showSorterTooltip={{ title: vi ? 'Bấm để sắp xếp' : 'Click to sort' }}
          locale={{ emptyText: vi ? 'Chưa có Space App nào chạy server' : 'No server Space App installed' }}
          columns={[
            {
              title: 'App',
              dataIndex: 'name',
              sorter: byName,
              // The only flexible column. Truncation is done here rather than
              // with the column's `ellipsis`, which would cut the id tag off
              // too — and the id is the part you need to act on the row.
              render: (_: string, r: AppRow) => (
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, minWidth: 0 }}>
                  <span style={{ flex: 'none' }}>{r.icon ?? '🧩'}</span>
                  <Tooltip title={r.name}>
                    <span
                      style={{
                        flex: 1,
                        minWidth: 0,
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {r.name}
                    </span>
                  </Tooltip>
                  <Tag style={{ flex: 'none', marginInlineEnd: 0 }}>{r.id}</Tag>
                </div>
              ),
            },
            {
              title: 'Sandbox',
              key: 'sandbox',
              width: 160,
              sorter: withNameTiebreak(
                (a, b) => Number(a.config.enabled) - Number(b.config.enabled),
              ),
              sortDirections: ['descend', 'ascend'] as const,
              render: (_: unknown, r: AppRow) => {
                // Configured vs. what the live process actually got. They differ
                // whenever the settings changed without a restart, and only the
                // second one is true right now.
                // Two ways a "confined" app can be running unconfined: the
                // profile predates the setting, or this daemon never built one
                // because it adopted a process that was already up.
                const stale =
                  r.running && r.config.enabled && (r.isolation === 'none' || r.adopted);
                if (!r.config.enabled) {
                  return <Tag>{vi ? 'tắt' : 'off'}</Tag>;
                }
                return (
                  <Space size={4} wrap>
                    <Tag color={stale ? 'orange' : 'green'}>
                      {!r.running
                        ? vi ? 'đã bật' : 'enabled'
                        : r.adopted
                          ? vi ? 'không rõ' : 'unknown'
                          : r.isolation}
                    </Tag>
                    {stale && (
                      <Tooltip
                        title={
                          vi
                            ? 'Tiến trình đang chạy không do daemon này khởi chạy (hoặc chạy từ trước khi bật sandbox), nên nó KHÔNG bị nhốt. Bấm Khởi động lại để áp dụng.'
                            : 'The running process was not launched by this daemon (or predates the setting), so it is NOT confined. Restart to apply.'
                        }
                      >
                        <Tag color="orange">{vi ? 'cần khởi động lại' : 'restart needed'}</Tag>
                      </Tooltip>
                    )}
                  </Space>
                );
              },
            },
            {
              title: vi ? 'Đọc đĩa' : 'Disk read',
              dataIndex: ['config', 'readMode'],
              width: 84,
              render: (v: string, r: AppRow) =>
                r.config.enabled ? (
                  <Tag color={v === 'open' ? undefined : 'blue'}>{v}</Tag>
                ) : (
                  <Text type="secondary">—</Text>
                ),
            },
            {
              title: vi ? 'Mạng' : 'Network',
              key: 'net',
              width: 150,
              sorter: withNameTiebreak((a, b) =>
                (a.config.network ?? '').localeCompare(b.config.network ?? ''),
              ),
              render: (_: unknown, r: AppRow) => {
                if (!r.config.enabled) return <Text type="secondary">—</Text>;
                const l = NET_LABEL[r.config.network] ?? { vi: r.config.network, en: r.config.network };
                return (
                  <Space size={4} wrap>
                    <Tag color={l.color}>{vi ? l.vi : l.en}</Tag>
                    {r.config.network === 'hosts' && (
                      <Text type="secondary" className="text-xs">
                        {r.config.hosts.length} {vi ? 'trang' : 'sites'}
                      </Text>
                    )}
                    {r.proxy && r.proxy.stats.denied > 0 && (
                      <Tooltip
                        title={
                          (vi ? 'App đang cần: ' : 'The app wanted: ') +
                          (r.proxy.stats.recentDenied.join(', ') || '—')
                        }
                      >
                        <Tag color="orange">{r.proxy.stats.denied} {vi ? 'bị chặn' : 'refused'}</Tag>
                      </Tooltip>
                    )}
                  </Space>
                );
              },
            },
            {
              title: vi ? 'Tiến trình' : 'Process',
              key: 'proc',
              width: 190,
              // First click puts running first, then the app restarted most — a
              // launch counter climbing on its own is the row you want on top.
              sorter: withNameTiebreak(
                (a, b) =>
                  Number(a.running) - Number(b.running) || a.launches - b.launches,
              ),
              sortDirections: ['descend', 'ascend'] as const,
              render: (_: unknown, r: AppRow) =>
                r.running ? (
                  <Space size={6}>
                    <Tag color={r.adopted ? 'cyan' : 'green'}>
                      {r.adopted
                        ? vi ? 'chạy (ngoài daemon)' : 'running (adopted)'
                        : vi ? 'đang chạy' : 'running'}
                    </Tag>
                    {/* One line, not three chips: the launch count belongs with
                        the other facts about this process, and a wrapped row
                        costs more attention than it is worth. */}
                    <Tooltip
                      title={
                        vi
                          ? 'Số lần daemon khởi chạy app. Tự tăng đều = app đang chết đi sống lại.'
                          : 'Times the daemon launched this app. Climbing on its own = a crash loop.'
                      }
                    >
                      <Text
                        type={r.launches > 3 ? 'warning' : 'secondary'}
                        className="text-xs"
                        style={{ whiteSpace: 'nowrap' }}
                      >
                        pid {r.pid} · {uptime(r.uptimeMs ?? 0)}
                        {r.adopted ? '' : ` · ${r.launches}×`}
                      </Text>
                    </Tooltip>
                  </Space>
                ) : (
                  <Tag>{vi ? 'không chạy' : 'stopped'}</Tag>
                ),
            },
            {
              // No spaces: with the action column wider, "CPU / RAM" wrapped
              // the header onto two lines and made every row taller.
              title: 'CPU/RAM',
              key: 'res',
              width: 118,
              sorter: withNameTiebreak(
                (a, b) => num_(a.cpu) - num_(b.cpu) || num_(a.rssMb) - num_(b.rssMb),
              ),
              sortDirections: ['descend', 'ascend'] as const,
              render: (_: unknown, r: AppRow) =>
                r.running && r.cpu != null ? (
                  <Text className="text-xs" style={{ whiteSpace: 'nowrap' }}>
                    {r.cpu.toFixed(1)}% · {(r.rssMb ?? 0).toFixed(0)} MB
                  </Text>
                ) : (
                  <Text type="secondary">—</Text>
                ),
            },
            {
              title: '',
              key: 'act',
              width: 155,
              render: (_: unknown, r: AppRow) => (
                <Space size={4}>
                  <Tooltip title={vi ? 'Theo dõi chi tiết' : 'Process monitor'}>
                    <Button
                      size="small"
                      icon={<MonitorOutlined />}
                      onClick={() => setMonitoring({ id: r.id, name: r.name })}
                    />
                  </Tooltip>
                  <Button size="small" onClick={() => setConfiguring({ id: r.id, name: r.name })}>
                    {vi ? 'Cấu hình' : 'Configure'}
                  </Button>
                  <Button
                    size="small"
                    icon={<ReloadOutlined />}
                    loading={restarting === r.id}
                    onClick={() => restart(r.id)}
                  />
                </Space>
              ),
            },
          ]}
        />
      </Card>

      <Modal
        open={!!monitoring}
        onCancel={() => setMonitoring(null)}
        footer={null}
        width={760}
        title={`${vi ? 'Theo dõi tiến trình' : 'Process monitor'} — ${monitoring?.name ?? ''}`}
        destroyOnHidden
      >
        {monitoring && <AppRuntimePanel appId={monitoring.id} />}
      </Modal>

      <SpaceAppSandboxModal
        appId={configuring?.id ?? null}
        appName={configuring?.name}
        open={!!configuring}
        onClose={() => {
          setConfiguring(null);
          void load();
        }}
      />
    </>
  );
};

export default SandboxAppsCard;
