import { useCallback, useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Col,
  Divider,
  Input,
  InputNumber,
  message,
  Popconfirm,
  Progress,
  Row,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Tooltip,
  Typography,
} from 'antd';
import {
  DeleteOutlined,
  PoweroffOutlined,
  ReloadOutlined,
  StopOutlined,
} from '@ant-design/icons';

const { Title, Text } = Typography;

/** Sandbox management — Plugins → Sandbox. Data from /api/sandbox/* (the
 * OS-sandbox engine built into the daemon: src/sandbox). */

interface SandboxRow {
  id: string;
  name: string;
  backend: string;
  image?: string | null;
  workdir: string;
  network: boolean;
  cpus: number;
  memoryMb: number;
  timeoutMs: number;
  fsMode: string;
  traceEnabled: boolean;
  status: string;
  lastError?: string | null;
  createdAt: number;
  lastUsedAt?: number | null;
}

interface RunRow {
  id: string;
  sandboxId: string;
  kind: string;
  language?: string | null;
  source: string;
  exitCode?: number | null;
  timedOut: boolean;
  isolation: string;
  network: boolean;
  durationMs: number;
  createdAt: number;
}

interface ProcRow {
  pid: number;
  ppid?: number;
  cpu?: number;
  memPercent?: number;
  rssMb?: number;
  elapsed?: string;
  command?: string;
}

interface Stats {
  running: boolean;
  source?: string;
  cpu?: number;
  rssMb?: number;
  memoryLimitMb?: number | null;
  note?: string | null;
  processes?: ProcRow[];
}

interface ExecPolicy {
  execShell: boolean;
  execNetwork: boolean;
  execFsMode: string;
  runPython: boolean;
  runNode: boolean;
  codeNetwork: boolean;
  schedulerScript: boolean;
  schedulerNetwork: boolean;
}

interface Defaults {
  defaultFsMode: string;
  allowlist: string[];
  defaultNetwork: boolean;
  defaultMemoryMb: number;
  defaultCpus: number;
  defaultTimeoutMs: number;
}

interface Caps {
  backends: string[];
  direct?: { kind: string; detail: string; available: boolean };
  docker?: { detail: string; available: boolean };
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`/api/sandbox${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error((body as { error?: string }).error || `HTTP ${res.status}`);
  return body as T;
}

const fmtTime = (ms?: number | null) => (ms ? new Date(ms).toLocaleString() : '—');

const isolationTag = (iso: string) => {
  const color =
    iso === 'seatbelt' || iso === 'bubblewrap' || iso === 'appcontainer'
      ? 'green'
      : iso === 'container'
        ? 'blue'
        : iso === 'none'
          ? 'default'
          : 'orange';
  return <Tag color={color}>{iso}</Tag>;
};

export function SandboxPanel() {
  const [caps, setCaps] = useState<Caps | null>(null);
  const [rows, setRows] = useState<SandboxRow[]>([]);
  const [runs, setRuns] = useState<RunRow[]>([]);
  const [stats, setStats] = useState<Record<string, Stats>>({});
  const [expanded, setExpanded] = useState<string[]>([]);
  const [policy, setPolicy] = useState<ExecPolicy | null>(null);
  const [defaults, setDefaults] = useState<Defaults | null>(null);
  const [allowlistText, setAllowlistText] = useState('');
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [sb, rn] = await Promise.all([
        api<{ sandboxes: SandboxRow[] }>('/sandboxes'),
        api<{ runs: RunRow[] }>('/runs?limit=30'),
      ]);
      setRows(sb.sandboxes);
      setRuns(rn.runs);
    } catch (e) {
      message.error(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    api<Caps>('/caps').then(setCaps).catch(() => {});
    api<ExecPolicy>('/exec-policy').then(setPolicy).catch(() => {});
    api<Defaults>('/settings')
      .then((d) => {
        setDefaults(d);
        setAllowlistText(d.allowlist.join('\n'));
      })
      .catch(() => {});
    const t = setInterval(refresh, 8000);
    return () => clearInterval(t);
  }, [refresh]);

  // Live stats for expanded rows only — one request per open panel.
  useEffect(() => {
    if (expanded.length === 0) return;
    let stop = false;
    const tick = async () => {
      for (const id of expanded) {
        try {
          const s = await api<Stats>(`/sandboxes/${id}/stats`);
          if (!stop) setStats((prev) => ({ ...prev, [id]: s }));
        } catch {
          /* row may be gone */
        }
      }
    };
    tick();
    const t = setInterval(tick, 3000);
    return () => {
      stop = true;
      clearInterval(t);
    };
  }, [expanded]);

  const savePolicy = async (patch: Partial<ExecPolicy>) => {
    if (!policy) return;
    const next = { ...policy, ...patch };
    setPolicy(next);
    try {
      setPolicy(await api<ExecPolicy>('/exec-policy', { method: 'PUT', body: JSON.stringify(next) }));
      message.success('Đã lưu cơ chế sandbox');
    } catch (e) {
      message.error(String(e));
    }
  };

  const saveDefaults = async (patch: Partial<Defaults>, allowlist?: string[]) => {
    if (!defaults) return;
    const next = { ...defaults, ...patch, allowlist: allowlist ?? defaults.allowlist };
    try {
      const saved = await api<Defaults>('/settings', { method: 'PUT', body: JSON.stringify(next) });
      setDefaults(saved);
      setAllowlistText(saved.allowlist.join('\n'));
      message.success('Đã lưu cài đặt mặc định');
    } catch (e) {
      message.error(String(e));
    }
  };

  const stopSandbox = async (id: string) => {
    try {
      await api(`/sandboxes/${id}/stop`, { method: 'POST', body: '{}' });
      message.success('Đã dừng');
      refresh();
    } catch (e) {
      message.error(String(e));
    }
  };

  const killAll = async (id: string) => {
    try {
      await api(`/sandboxes/${id}/kill`, { method: 'POST', body: '{}' });
      message.success('Đã dừng mọi tiến trình của sandbox');
    } catch (e) {
      message.error(String(e));
    }
  };

  const killPid = async (id: string, pid: number) => {
    try {
      await api(`/sandboxes/${id}/kill`, { method: 'POST', body: JSON.stringify({ pid }) });
      message.success(`Đã dừng tiến trình ${pid}`);
    } catch (e) {
      message.error(String(e));
    }
  };

  const deleteSandbox = async (id: string, purge: boolean) => {
    try {
      await api(`/sandboxes/${id}?purge=${purge}`, { method: 'DELETE' });
      message.success(purge ? 'Đã xoá sandbox và toàn bộ file' : 'Đã xoá sandbox (file còn trên đĩa)');
      refresh();
    } catch (e) {
      message.error(String(e));
    }
  };

  const columns = [
    {
      title: 'Luồng / Sandbox',
      dataIndex: 'name',
      key: 'name',
      render: (name: string, r: SandboxRow) => (
        <Space direction="vertical" size={0}>
          <Text strong>{name}</Text>
          <Text type="secondary" style={{ fontSize: 12 }}>
            {r.workdir}
          </Text>
        </Space>
      ),
    },
    {
      title: 'Backend',
      dataIndex: 'backend',
      key: 'backend',
      render: (b: string, r: SandboxRow) => (
        <Space direction="vertical" size={0}>
          <Tag color={b === 'docker' ? 'blue' : 'purple'}>{b}</Tag>
          {r.image ? (
            <Text type="secondary" style={{ fontSize: 11 }}>
              {r.image}
            </Text>
          ) : null}
        </Space>
      ),
    },
    {
      title: 'Đọc đĩa',
      dataIndex: 'fsMode',
      key: 'fsMode',
      render: (m: string) => <Tag>{m}</Tag>,
    },
    {
      title: 'Mạng',
      dataIndex: 'network',
      key: 'network',
      render: (n: boolean) => (n ? <Tag color="orange">on</Tag> : <Tag color="green">off</Tag>),
    },
    {
      title: 'Giới hạn',
      key: 'limits',
      render: (_: unknown, r: SandboxRow) => (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {r.cpus} CPU · {r.memoryMb} MB · {(r.timeoutMs / 1000).toFixed(0)}s
        </Text>
      ),
    },
    {
      title: 'Trạng thái',
      dataIndex: 'status',
      key: 'status',
      render: (s: string, r: SandboxRow) => (
        <Tooltip title={r.lastError || undefined}>
          <Tag color={s === 'running' ? 'green' : s === 'error' ? 'red' : 'default'}>{s}</Tag>
        </Tooltip>
      ),
    },
    { title: 'Dùng lần cuối', key: 'used', render: (_: unknown, r: SandboxRow) => fmtTime(r.lastUsedAt ?? r.createdAt) },
    {
      title: '',
      key: 'actions',
      render: (_: unknown, r: SandboxRow) => (
        <Space>
          <Tooltip title="Dừng mọi tiến trình đang chạy">
            <Button size="small" icon={<StopOutlined />} onClick={() => killAll(r.id)} />
          </Tooltip>
          {r.backend === 'docker' && r.status === 'running' && (
            <Tooltip title="Dừng container">
              <Button size="small" icon={<PoweroffOutlined />} onClick={() => stopSandbox(r.id)} />
            </Tooltip>
          )}
          <Popconfirm
            title="Xoá sandbox này?"
            description="Giữ file trên đĩa (OK) hoặc xoá sạch (nút đỏ bên trong bảng)."
            onConfirm={() => deleteSandbox(r.id, false)}
          >
            <Button size="small" danger icon={<DeleteOutlined />} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  const expandedRowRender = (r: SandboxRow) => {
    const s = stats[r.id];
    return (
      <div style={{ padding: '4px 8px' }}>
        <Space align="center" size="large" wrap>
          <span>
            CPU:{' '}
            <Progress
              type="circle"
              size={40}
              percent={Math.min(100, Math.round(s?.cpu ?? 0))}
              format={(p) => `${p}%`}
            />
          </span>
          <span>
            RAM: <Text strong>{(s?.rssMb ?? 0).toFixed(0)} MB</Text>
            {s?.memoryLimitMb ? <Text type="secondary"> / {s.memoryLimitMb} MB</Text> : null}
          </span>
          {s?.note ? <Text type="secondary">{s.note}</Text> : null}
          <Button size="small" icon={<StopOutlined />} onClick={() => killAll(r.id)}>
            Dừng tất cả
          </Button>
          <Popconfirm title="Xoá sandbox VÀ toàn bộ file? Không khôi phục được." onConfirm={() => deleteSandbox(r.id, true)}>
            <Button size="small" danger>
              Xoá kèm file
            </Button>
          </Popconfirm>
        </Space>
        <Table<ProcRow>
          size="small"
          style={{ marginTop: 12 }}
          rowKey="pid"
          pagination={false}
          dataSource={s?.processes ?? []}
          locale={{ emptyText: 'Không có tiến trình nào đang chạy' }}
          columns={[
            { title: 'PID', dataIndex: 'pid', width: 90 },
            { title: '%CPU', dataIndex: 'cpu', width: 90, render: (v?: number) => (v ?? 0).toFixed(1) },
            { title: 'RAM (MB)', dataIndex: 'rssMb', width: 110, render: (v?: number) => (v ?? 0).toFixed(0) },
            { title: 'Thời gian', dataIndex: 'elapsed', width: 110 },
            { title: 'Lệnh', dataIndex: 'command', ellipsis: true },
            {
              title: '',
              width: 60,
              render: (_: unknown, p: ProcRow) => (
                <Button size="small" danger icon={<StopOutlined />} onClick={() => killPid(r.id, p.pid)} />
              ),
            },
          ]}
        />
      </div>
    );
  };

  return (
    <div style={{ padding: 24 }}>
      <div style={{ maxWidth: 1200, margin: '0 auto', width: '100%' }}>
          <Space align="baseline" style={{ justifyContent: 'space-between', width: '100%' }}>
            <div>
              <Title level={2} style={{ margin: 0 }}>
                Sandbox
              </Title>
              <Text type="secondary">
                Chạy lệnh và mã nguồn cách ly khỏi máy thật — quản lý luồng, giám sát CPU/RAM và cơ
                chế cưỡng chế exec/python/node/script.
              </Text>
            </div>
            <Button icon={<ReloadOutlined />} onClick={refresh} loading={loading}>
              Làm mới
            </Button>
          </Space>

          {caps && (
            <Alert
              style={{ marginTop: 16 }}
              type={caps.direct?.available || caps.docker?.available ? 'info' : 'warning'}
              showIcon
              message={
                <Space wrap>
                  <span>Cách ly khả dụng:</span>
                  {caps.direct && (
                    <span>
                      direct {isolationTag(caps.direct.kind)}{' '}
                      <Text type="secondary">({caps.direct.detail})</Text>
                    </span>
                  )}
                  <Divider type="vertical" />
                  <span>
                    docker{' '}
                    {caps.docker?.available ? <Tag color="blue">sẵn sàng</Tag> : <Tag>không</Tag>}{' '}
                    <Text type="secondary">({caps.docker?.detail})</Text>
                  </span>
                </Space>
              }
            />
          )}

          {policy && (
            <Card title="Cơ chế bảo mật — chạy trên sandbox" size="small" style={{ marginTop: 16 }}>
              <Row gutter={[24, 12]}>
                <Col xs={24} md={12}>
                  <Space direction="vertical" style={{ width: '100%' }}>
                    <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                      <span>
                        <Text strong>Exec (tool Bash của agent)</Text>
                        <br />
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          Lệnh shell của agent chạy trong OS sandbox, chỉ được ghi vào thư mục làm
                          việc của chat. Lưu ý: build cache ngoài workspace (npm/cargo…) sẽ bị chặn
                          ghi.
                        </Text>
                      </span>
                      <Switch checked={policy.execShell} onChange={(v) => savePolicy({ execShell: v })} />
                    </Space>
                    {policy.execShell && (
                      <Space size="large" style={{ paddingLeft: 12 }}>
                        <span>
                          Mạng{' '}
                          <Switch
                            size="small"
                            checked={policy.execNetwork}
                            onChange={(v) => savePolicy({ execNetwork: v })}
                          />
                        </span>
                        <span>
                          Đọc đĩa{' '}
                          <Select
                            size="small"
                            style={{ width: 130 }}
                            value={policy.execFsMode}
                            onChange={(v) => savePolicy({ execFsMode: v })}
                            options={[
                              { value: 'strict', label: 'strict' },
                              { value: 'allowlist', label: 'allowlist' },
                              { value: 'open', label: 'open' },
                            ]}
                          />
                        </span>
                      </Space>
                    )}
                    <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                      <span>
                        <Text strong>Script hẹn giờ (scheduler)</Text>
                        <br />
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          Lệnh của task chế độ script/script-agent chạy trong sandbox dùng-một-lần.
                        </Text>
                      </span>
                      <Switch
                        checked={policy.schedulerScript}
                        onChange={(v) => savePolicy({ schedulerScript: v })}
                      />
                    </Space>
                    {policy.schedulerScript && (
                      <span style={{ paddingLeft: 12 }}>
                        Mạng{' '}
                        <Switch
                          size="small"
                          checked={policy.schedulerNetwork}
                          onChange={(v) => savePolicy({ schedulerNetwork: v })}
                        />
                      </span>
                    )}
                  </Space>
                </Col>
                <Col xs={24} md={12}>
                  <Space direction="vertical" style={{ width: '100%' }}>
                    <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                      <span>
                        <Text strong>Run Python</Text>
                        <br />
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          Cho phép chạy Python thật (REPL /api/code/run + tool sbx). Luôn trong
                          sandbox; tắt là từ chối chạy.
                        </Text>
                      </span>
                      <Switch checked={policy.runPython} onChange={(v) => savePolicy({ runPython: v })} />
                    </Space>
                    <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                      <span>
                        <Text strong>Run Node.js</Text>
                        <br />
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          Cho phép chạy Node.js thật. Luôn trong sandbox; tắt là từ chối chạy.
                        </Text>
                      </span>
                      <Switch checked={policy.runNode} onChange={(v) => savePolicy({ runNode: v })} />
                    </Space>
                    <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                      <span>
                        <Text strong>Mạng cho Python/Node</Text>
                        <br />
                        <Text type="secondary" style={{ fontSize: 12 }}>
                          Mặc định tắt — bật khi snippet cần gọi mạng.
                        </Text>
                      </span>
                      <Switch checked={policy.codeNetwork} onChange={(v) => savePolicy({ codeNetwork: v })} />
                    </Space>
                  </Space>
                </Col>
              </Row>
            </Card>
          )}

          <Card
            title={`Luồng đang quản lý (${rows.length})`}
            size="small"
            style={{ marginTop: 16 }}
          >
            <Table<SandboxRow>
              size="small"
              rowKey="id"
              dataSource={rows}
              columns={columns}
              pagination={{ pageSize: 8, hideOnSinglePage: true }}
              expandable={{
                expandedRowRender,
                expandedRowKeys: expanded,
                onExpandedRowsChange: (keys) => setExpanded(keys as string[]),
              }}
              locale={{ emptyText: 'Chưa có sandbox nào — agent sẽ tự tạo khi chạy code, hoặc tạo qua tool sbx_create' }}
            />
          </Card>

          <Card title="Lịch sử chạy gần nhất" size="small" style={{ marginTop: 16 }}>
            <Table<RunRow>
              size="small"
              rowKey="id"
              dataSource={runs}
              pagination={{ pageSize: 8, hideOnSinglePage: true }}
              columns={[
                { title: 'Lúc', dataIndex: 'createdAt', width: 170, render: fmtTime },
                {
                  title: 'Loại',
                  key: 'kind',
                  width: 110,
                  render: (_: unknown, r: RunRow) => <Tag>{r.language || r.kind}</Tag>,
                },
                { title: 'Lệnh / mã', dataIndex: 'source', ellipsis: true },
                {
                  title: 'Kết quả',
                  key: 'exit',
                  width: 110,
                  render: (_: unknown, r: RunRow) =>
                    r.timedOut ? (
                      <Tag color="red">timeout</Tag>
                    ) : r.exitCode === 0 ? (
                      <Tag color="green">exit 0</Tag>
                    ) : (
                      <Tag color="red">exit {r.exitCode ?? '?'}</Tag>
                    ),
                },
                { title: 'Cách ly', dataIndex: 'isolation', width: 120, render: isolationTag },
                {
                  title: 'ms',
                  dataIndex: 'durationMs',
                  width: 80,
                  render: (v: number) => v.toLocaleString(),
                },
              ]}
            />
          </Card>

          {defaults && (
            <Card title="Mặc định cho sandbox mới" size="small" style={{ marginTop: 16, marginBottom: 24 }}>
              <Row gutter={[24, 12]}>
                <Col xs={24} md={8}>
                  <Space direction="vertical" style={{ width: '100%' }}>
                    <span>
                      Đọc đĩa mặc định{' '}
                      <Select
                        size="small"
                        style={{ width: 140 }}
                        value={defaults.defaultFsMode}
                        onChange={(v) => saveDefaults({ defaultFsMode: v })}
                        options={[
                          { value: 'strict', label: 'strict' },
                          { value: 'allowlist', label: 'allowlist' },
                          { value: 'open', label: 'open' },
                        ]}
                      />
                    </span>
                    <span>
                      Mạng mặc định{' '}
                      <Switch
                        size="small"
                        checked={defaults.defaultNetwork}
                        onChange={(v) => saveDefaults({ defaultNetwork: v })}
                      />
                    </span>
                  </Space>
                </Col>
                <Col xs={24} md={8}>
                  <Space direction="vertical">
                    <span>
                      RAM (MB){' '}
                      <InputNumber
                        size="small"
                        min={64}
                        max={65536}
                        value={defaults.defaultMemoryMb}
                        onChange={(v) => v && saveDefaults({ defaultMemoryMb: v })}
                      />
                    </span>
                    <span>
                      CPU{' '}
                      <InputNumber
                        size="small"
                        min={0.1}
                        max={32}
                        step={0.5}
                        value={defaults.defaultCpus}
                        onChange={(v) => v && saveDefaults({ defaultCpus: v })}
                      />
                    </span>
                    <span>
                      Deadline (ms){' '}
                      <InputNumber
                        size="small"
                        min={1000}
                        max={600000}
                        step={1000}
                        value={defaults.defaultTimeoutMs}
                        onChange={(v) => v && saveDefaults({ defaultTimeoutMs: v })}
                      />
                    </span>
                  </Space>
                </Col>
                <Col xs={24} md={8}>
                  <Space direction="vertical" style={{ width: '100%' }}>
                    <Text>Allowlist (mỗi dòng một đường dẫn tuyệt đối — dùng ở chế độ allowlist)</Text>
                    <Input.TextArea
                      rows={3}
                      value={allowlistText}
                      onChange={(e) => setAllowlistText(e.target.value)}
                      placeholder="/Users/ban/du-lieu"
                    />
                    <Button
                      size="small"
                      onClick={() =>
                        saveDefaults(
                          {},
                          allowlistText
                            .split('\n')
                            .map((s) => s.trim())
                            .filter(Boolean),
                        )
                      }
                    >
                      Lưu allowlist
                    </Button>
                  </Space>
                </Col>
              </Row>
            </Card>
          )}
      </div>
    </div>
  );
}
