import React, { useEffect, useState } from 'react';
import { Alert, Button, Descriptions, Table, Tag, Tooltip, Typography, theme } from 'antd';
import { ApiOutlined, ReloadOutlined } from '@ant-design/icons';

const { Text } = Typography;

interface Proc {
  pid: number;
  ppid: number;
  cpu: number;
  memPercent: number;
  rssMb: number;
  elapsed: string;
  command: string;
}

interface Conn {
  pid: number;
  command: string;
  proto: string;
  local: string;
  remote: string | null;
  state: string;
}

export interface RuntimeSnapshot {
  appId: string;
  running: boolean;
  /** Up, but this daemon did not start it — it adopted a healthy port. */
  adopted: boolean;
  launches: number;
  process: {
    pid: number;
    pgid: number;
    port: number;
    url: string;
    uptimeMs: number | null;
    isolation: string;
    adopted?: boolean;
  } | null;
  health: { url: string; ok: boolean; status?: number; ms?: number; error?: string } | null;
  resources: {
    cpu: number;
    rssMb: number;
    processes: Proc[];
    note?: string | null;
  } | null;
  network: {
    connections: Conn[];
    note?: string | null;
    proxy?: { port: number; stats: { allowed: number; denied: number; recentDenied: string[] } } | null;
  };
  sandbox: { enabled: boolean; readMode: string; network: string; hosts: string[] };
  log: { path: string; bytes: number };
  launch: { cwd: string; command: string | null; env: [string, string][] };
}

const humanUptime = (ms: number) => {
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  const h = Math.floor(s / 3600);
  return `${h}h ${Math.floor((s % 3600) / 60)}m`;
};

/**
 * Live view of one Space App's process: is it up, since when, what is it
 * burning, who is it talking to.
 *
 * Polls while mounted. The interesting states are the unhappy ones — an app
 * that answers 500, a launch counter that keeps climbing on its own (a crash
 * loop looks exactly like a healthy app in every other view), a proxy refusing
 * the host the app actually needs — so those are what this leads with.
 */
export const AppRuntimePanel: React.FC<{ appId: string; onRestart?: () => void }> = ({
  appId,
  onRestart,
}) => {
  const { token } = theme.useToken();
  const [snap, setSnap] = useState<RuntimeSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const r = await fetch(`/api/space/apps/${encodeURIComponent(appId)}/runtime`);
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        const d = await r.json();
        if (alive) {
          setSnap(d);
          setError(null);
        }
      } catch (e: any) {
        if (alive) setError(String(e.message ?? e));
      }
    };
    void load();
    const t = setInterval(load, 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [appId]);

  if (error) return <Alert type="error" showIcon message={`Không đọc được trạng thái: ${error}`} />;
  if (!snap) return <Text type="secondary">Đang đọc trạng thái…</Text>;

  const p = snap.process;
  const health = snap.health;

  return (
    <div className="flex flex-col gap-3">
      {/* Status line — the answer to "is it working" before any detail. */}
      <div className="flex items-center gap-2 flex-wrap">
        <Tag color={snap.running ? (health?.ok ? (snap.adopted ? 'cyan' : 'green') : 'orange') : 'red'}>
          {!snap.running
            ? 'không chạy'
            : !health?.ok
              ? 'chạy nhưng không trả lời'
              : snap.adopted
                ? 'đang chạy (ngoài daemon)'
                : 'đang chạy'}
        </Tag>
        {p && <Tag>pid {p.pid}</Tag>}
        {p && (
          <Tag>
            cổng {p.port}
          </Tag>
        )}
        {p?.uptimeMs != null && <Tag>đã chạy {humanUptime(p.uptimeMs)}</Tag>}
        {!snap.adopted && (
          <Tooltip title="Số lần daemon khởi chạy app kể từ lúc daemon bật. Tự tăng đều = app đang chết đi sống lại.">
            <Tag color={snap.launches > 3 ? 'orange' : undefined}>{snap.launches} lần khởi chạy</Tag>
          </Tooltip>
        )}
        {p && <Tag color={p.isolation === 'none' ? undefined : 'blue'}>sandbox: {p.isolation}</Tag>}
        {health && (
          <Text type="secondary" className="text-xs">
            {health.ok ? `health ${health.status} · ${health.ms}ms` : health.error ?? `health ${health.status}`}
          </Text>
        )}
        {onRestart && (
          <Button size="small" icon={<ReloadOutlined />} onClick={onRestart}>
            Khởi động lại
          </Button>
        )}
      </div>

      {snap.adopted && (
        <Alert
          type="info"
          showIcon
          message="Tiến trình này đang chạy nhưng KHÔNG do daemon hiện tại khởi chạy — nó đã sống sẵn trên cổng của app (thường là còn sót lại sau khi daemon khởi động lại). Vì vậy không biết được nó có bị sandbox nhốt hay không; khởi động lại app nếu bạn cần chắc chắn."
        />
      )}
      {snap.running && !snap.adopted && snap.launches > 3 && (
        <Alert
          type="warning"
          showIcon
          message="App đã được khởi chạy nhiều lần — nhiều khả năng nó chết rồi được supervisor bật lại. Xem log bên dưới để biết lý do."
        />
      )}

      {/* Resources */}
      {snap.resources && (
        <div>
          <div className="flex items-center gap-4 mb-1">
            <Text strong>CPU {snap.resources.cpu.toFixed(1)}%</Text>
            <Text strong>RAM {snap.resources.rssMb.toFixed(1)} MB</Text>
            <Text type="secondary" className="text-xs">
              {snap.resources.processes.length} tiến trình
            </Text>
          </div>
          {snap.resources.note && (
            <Alert type="info" showIcon className="mb-1" message={snap.resources.note} />
          )}
          <Table
            size="small"
            pagination={false}
            rowKey="pid"
            dataSource={snap.resources.processes}
            columns={[
              { title: 'PID', dataIndex: 'pid', width: 70 },
              { title: 'CPU %', dataIndex: 'cpu', width: 70, render: (v: number) => v.toFixed(1) },
              {
                title: 'RAM MB',
                dataIndex: 'rssMb',
                width: 80,
                render: (v: number) => v.toFixed(1),
              },
              { title: 'Chạy được', dataIndex: 'elapsed', width: 90 },
              {
                title: 'Lệnh',
                dataIndex: 'command',
                render: (v: string) => <Text code className="text-xs">{v}</Text>,
              },
            ]}
          />
        </div>
      )}

      {/* Network */}
      <div>
        <div className="flex items-center gap-2 mb-1">
          <ApiOutlined style={{ color: token.colorPrimary }} />
          <Text strong>Mạng</Text>
          {snap.network.proxy && (
            <Text type="secondary" className="text-xs">
              proxy allowlist 127.0.0.1:{snap.network.proxy.port} — {snap.network.proxy.stats.allowed} cho
              qua, {snap.network.proxy.stats.denied} bị chặn
            </Text>
          )}
        </div>
        {snap.network.proxy && snap.network.proxy.stats.recentDenied.length > 0 && (
          <Alert
            type="warning"
            showIcon
            className="mb-1"
            message={
              <span>
                App đang cần những trang chưa khai:{' '}
                {snap.network.proxy.stats.recentDenied.map(h => (
                  <Tag key={h} color="orange">{h}</Tag>
                ))}
                — thêm ở nút Sandbox.
              </span>
            }
          />
        )}
        {snap.network.note && <Alert type="info" showIcon className="mb-1" message={snap.network.note} />}
        <Table
          size="small"
          pagination={false}
          rowKey={(r: Conn, i) => `${r.pid}-${r.local}-${r.remote ?? ''}-${i}`}
          dataSource={snap.network.connections}
          locale={{ emptyText: snap.running ? 'Không có socket nào' : 'App không chạy' }}
          columns={[
            { title: '', dataIndex: 'proto', width: 50 },
            { title: 'Tại máy', dataIndex: 'local', render: (v: string) => <Text code className="text-xs">{v}</Text> },
            {
              title: 'Đầu kia',
              dataIndex: 'remote',
              render: (v: string | null) =>
                v ? <Text code className="text-xs">{v}</Text> : <Text type="secondary">—</Text>,
            },
            {
              title: 'Trạng thái',
              dataIndex: 'state',
              width: 110,
              render: (v: string) => (v === 'LISTEN' ? <Tag color="green">LISTEN</Tag> : <Tag>{v}</Tag>),
            },
          ]}
        />
      </div>

      {/* Everything needed to reproduce the launch by hand. */}
      <Descriptions size="small" column={1} bordered>
        <Descriptions.Item label="Thư mục">
          <Text copyable className="text-xs">{snap.launch.cwd}</Text>
        </Descriptions.Item>
        <Descriptions.Item label="Lệnh chạy">
          <Text code copyable className="text-xs">{snap.launch.command ?? '—'}</Text>
        </Descriptions.Item>
        <Descriptions.Item label="Biến môi trường">
          {snap.launch.env.map(([k, v]) => (
            <div key={k}>
              <Text code className="text-xs">{k}={v}</Text>
            </div>
          ))}
        </Descriptions.Item>
        <Descriptions.Item label="File log">
          <Text copyable className="text-xs">{snap.log.path}</Text>{' '}
          <Text type="secondary" className="text-xs">({(snap.log.bytes / 1024).toFixed(1)} KB)</Text>
        </Descriptions.Item>
      </Descriptions>
    </div>
  );
};

export default AppRuntimePanel;
