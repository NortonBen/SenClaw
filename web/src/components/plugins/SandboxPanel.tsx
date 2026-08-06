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
import SandboxAppsCard from './SandboxAppsCard';

const { Title, Text } = Typography;

/** Sandbox management — Plugins → Sandbox. Data from /api/sandbox/* (the
 * OS-sandbox engine built into the daemon: src/sandbox). Renders in the app
 * language; this screen no longer carries a language switch of its own. */

// ── i18n ────────────────────────────────────────────────────────────────────

const STRINGS = {
  en: {
    subtitle:
      'Run commands and code isolated from the real machine — sessions, CPU/RAM monitoring, and the exec/python/node/script enforcement switches.',
    refresh: 'Refresh',
    capsTitle: 'Available isolation:',
    ready: 'ready',
    notAvailable: 'no',
    policyTitle: 'Security enforcement — run through the sandbox',
    execTitle: 'Exec (agent Bash tool)',
    execDesc:
      "Agent shell commands run inside the OS sandbox and can only write to the chat's working directory. Note: build caches outside the workspace (npm/cargo…) will be blocked from writing.",
    network: 'Network',
    diskRead: 'Disk read',
    localPortsDesc:
      "Local ports the agent's shell may call (e.g. the dev server it is working on). Empty = none: loopback is where SenClaw's own API and every Space App live, and none of them ask for credentials.",
    schedTitle: 'Scheduled scripts (scheduler)',
    schedDesc: 'script / script-agent task commands run in a throwaway sandbox.',
    runPythonTitle: 'Run Python',
    runPythonDesc:
      'Allow real Python (REPL /api/code/run + sbx tools). Always sandboxed; switching off refuses to run.',
    runNodeTitle: 'Run Node.js',
    runNodeDesc: 'Allow real Node.js. Always sandboxed; switching off refuses to run.',
    codeNetTitle: 'Network for Python/Node',
    codeNetDesc: 'Off by default — enable when a snippet needs network access.',
    sessionsTitle: (n: number) => `Managed sandboxes (${n})`,
    emptySandboxes:
      'No sandboxes yet — the agent creates one when it runs code, or create one with sbx_create.',
    colSandbox: 'Sandbox',
    colBackend: 'Backend',
    colDiskRead: 'Disk read',
    colNetwork: 'Network',
    colLimits: 'Limits',
    colStatus: 'Status',
    colLastUsed: 'Last used',
    on: 'on',
    off: 'off',
    killAllTip: 'Stop all running processes',
    stopContainerTip: 'Stop the container',
    deleteTitle: 'Delete this sandbox?',
    deleteDesc: 'Keeps files on disk. Use the red button inside the expanded row to purge.',
    purgeTitle: 'Delete the sandbox AND all its files? This cannot be undone.',
    purgeBtn: 'Delete with files',
    stopAll: 'Stop all',
    measuring: 'Measuring…',
    noProcs: 'No processes running',
    colPid: 'PID',
    colCpu: '%CPU',
    colRam: 'RAM (MB)',
    colElapsed: 'Elapsed',
    colCommand: 'Command',
    runsTitle: 'Recent runs',
    colWhen: 'Time',
    colKind: 'Type',
    colSource: 'Command / code',
    colResult: 'Result',
    colIsolation: 'Isolation',
    defaultsTitle: 'Defaults for new sandboxes',
    defaultDiskRead: 'Default disk read',
    defaultNetwork: 'Default network',
    ram: 'RAM (MB)',
    cpu: 'CPU',
    deadline: 'Deadline (ms)',
    allowlistLabel:
      'Allowlist — extra folders the sandbox may READ in allowlist mode (writes stay blocked)',
    allowlistEmpty: 'No folders yet.',
    addPathPlaceholder: '/Users/you/data',
    add: 'Add',
    needAbsolute: 'An absolute path is required (starts with / or C:\\)',
    alreadyListed: 'Already in the allowlist',
    savedPolicy: 'Enforcement saved',
    savedDefaults: 'Defaults saved',
    stoppedAll: 'Stopped all processes in the sandbox',
    stoppedContainer: 'Stopped',
    stoppedPid: (pid: number) => `Stopped process ${pid}`,
    deleted: 'Deleted (files kept on disk)',
    deletedPurge: 'Deleted the sandbox and all its files',
  },
  vi: {
    subtitle:
      'Chạy lệnh và mã nguồn cách ly khỏi máy thật — quản lý luồng, giám sát CPU/RAM và cơ chế cưỡng chế exec/python/node/script.',
    refresh: 'Làm mới',
    capsTitle: 'Cách ly khả dụng:',
    ready: 'sẵn sàng',
    notAvailable: 'không',
    policyTitle: 'Cơ chế bảo mật — chạy trên sandbox',
    execTitle: 'Exec (tool Bash của agent)',
    execDesc:
      'Lệnh shell của agent chạy trong OS sandbox, chỉ được ghi vào thư mục làm việc của chat. Lưu ý: build cache ngoài workspace (npm/cargo…) sẽ bị chặn ghi.',
    network: 'Mạng',
    diskRead: 'Đọc đĩa',
    localPortsDesc:
      'Cổng local mà shell của agent được gọi (ví dụ dev server đang làm). Để trống = không cổng nào: loopback là chỗ API của chính SenClaw và mọi Space App chạy, đều không hỏi mật khẩu.',
    schedTitle: 'Script hẹn giờ (scheduler)',
    schedDesc: 'Lệnh của task chế độ script/script-agent chạy trong sandbox dùng-một-lần.',
    runPythonTitle: 'Run Python',
    runPythonDesc:
      'Cho phép chạy Python thật (REPL /api/code/run + tool sbx). Luôn trong sandbox; tắt là từ chối chạy.',
    runNodeTitle: 'Run Node.js',
    runNodeDesc: 'Cho phép chạy Node.js thật. Luôn trong sandbox; tắt là từ chối chạy.',
    codeNetTitle: 'Mạng cho Python/Node',
    codeNetDesc: 'Mặc định tắt — bật khi snippet cần gọi mạng.',
    sessionsTitle: (n: number) => `Luồng đang quản lý (${n})`,
    emptySandboxes:
      'Chưa có sandbox nào — agent sẽ tự tạo khi chạy code, hoặc tạo qua tool sbx_create.',
    colSandbox: 'Luồng / Sandbox',
    colBackend: 'Backend',
    colDiskRead: 'Đọc đĩa',
    colNetwork: 'Mạng',
    colLimits: 'Giới hạn',
    colStatus: 'Trạng thái',
    colLastUsed: 'Dùng lần cuối',
    on: 'bật',
    off: 'tắt',
    killAllTip: 'Dừng mọi tiến trình đang chạy',
    stopContainerTip: 'Dừng container',
    deleteTitle: 'Xoá sandbox này?',
    deleteDesc: 'File trên đĩa được giữ lại. Muốn xoá sạch, dùng nút đỏ trong hàng mở rộng.',
    purgeTitle: 'Xoá sandbox VÀ toàn bộ file? Không khôi phục được.',
    purgeBtn: 'Xoá kèm file',
    stopAll: 'Dừng tất cả',
    measuring: 'Đang đo…',
    noProcs: 'Không có tiến trình nào đang chạy',
    colPid: 'PID',
    colCpu: '%CPU',
    colRam: 'RAM (MB)',
    colElapsed: 'Thời gian',
    colCommand: 'Lệnh',
    runsTitle: 'Lịch sử chạy gần nhất',
    colWhen: 'Lúc',
    colKind: 'Loại',
    colSource: 'Lệnh / mã',
    colResult: 'Kết quả',
    colIsolation: 'Cách ly',
    defaultsTitle: 'Mặc định cho sandbox mới',
    defaultDiskRead: 'Đọc đĩa mặc định',
    defaultNetwork: 'Mạng mặc định',
    ram: 'RAM (MB)',
    cpu: 'CPU',
    deadline: 'Deadline (ms)',
    allowlistLabel:
      'Allowlist — thư mục sandbox được ĐỌC thêm ở chế độ allowlist (ghi vẫn bị chặn)',
    allowlistEmpty: 'Chưa có thư mục nào.',
    addPathPlaceholder: '/Users/ban/du-lieu',
    add: 'Thêm',
    needAbsolute: 'Cần đường dẫn tuyệt đối (bắt đầu bằng / hoặc C:\\)',
    alreadyListed: 'Đã có trong allowlist',
    savedPolicy: 'Đã lưu cơ chế sandbox',
    savedDefaults: 'Đã lưu cài đặt mặc định',
    stoppedAll: 'Đã dừng mọi tiến trình của sandbox',
    stoppedContainer: 'Đã dừng',
    stoppedPid: (pid: number) => `Đã dừng tiến trình ${pid}`,
    deleted: 'Đã xoá (file còn trên đĩa)',
    deletedPurge: 'Đã xoá sandbox và toàn bộ file',
  },
} as const;

type Lang = keyof typeof STRINGS;

const LANG_KEY = 'senclaw:lang';

/// The display language, in the order the app decides it everywhere else: an
/// explicit choice if one was stored, otherwise the browser's own locale.
/// This screen no longer offers its own switch — Settings owns that.
function loadLang(): Lang {
  try {
    const stored = localStorage.getItem(LANG_KEY);
    if (stored === 'vi' || stored === 'en') return stored;
  } catch {
    /* private mode */
  }
  return typeof navigator !== 'undefined' && navigator.language?.startsWith('vi')
    ? 'vi'
    : 'en';
}

// ── API types ───────────────────────────────────────────────────────────────

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
  execLoopback: number[];
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

// ── Panel ───────────────────────────────────────────────────────────────────

export function SandboxPanel() {
  // Read once, no control: the app already has one language setting, and a
  // second switch on a single screen is how the two end up disagreeing.
  const lang = loadLang();
  const [caps, setCaps] = useState<Caps | null>(null);
  const [rows, setRows] = useState<SandboxRow[]>([]);
  const [runs, setRuns] = useState<RunRow[]>([]);
  const [stats, setStats] = useState<Record<string, Stats>>({});
  const [expanded, setExpanded] = useState<string[]>([]);
  const [policy, setPolicy] = useState<ExecPolicy | null>(null);
  const [defaults, setDefaults] = useState<Defaults | null>(null);
  const [newAllowPath, setNewAllowPath] = useState('');
  const [loading, setLoading] = useState(false);

  const L = STRINGS[lang];

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
    api<Defaults>('/settings').then(setDefaults).catch(() => {});
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
      message.success(L.savedPolicy);
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
      message.success(L.savedDefaults);
    } catch (e) {
      message.error(String(e));
    }
  };

  const addAllowPath = () => {
    if (!defaults) return;
    const path = newAllowPath.trim();
    if (!path) return;
    // Only absolute paths mean anything to Seatbelt/bwrap: '/' or 'C:\'.
    if (!(path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path))) {
      message.warning(L.needAbsolute);
      return;
    }
    if (defaults.allowlist.includes(path)) {
      message.info(L.alreadyListed);
      return;
    }
    setNewAllowPath('');
    saveDefaults({}, [...defaults.allowlist, path]);
  };

  const stopSandbox = async (id: string) => {
    try {
      await api(`/sandboxes/${id}/stop`, { method: 'POST', body: '{}' });
      message.success(L.stoppedContainer);
      refresh();
    } catch (e) {
      message.error(String(e));
    }
  };

  const killAll = async (id: string) => {
    try {
      await api(`/sandboxes/${id}/kill`, { method: 'POST', body: '{}' });
      message.success(L.stoppedAll);
    } catch (e) {
      message.error(String(e));
    }
  };

  const killPid = async (id: string, pid: number) => {
    try {
      await api(`/sandboxes/${id}/kill`, { method: 'POST', body: JSON.stringify({ pid }) });
      message.success(L.stoppedPid(pid));
    } catch (e) {
      message.error(String(e));
    }
  };

  const deleteSandbox = async (id: string, purge: boolean) => {
    try {
      await api(`/sandboxes/${id}?purge=${purge}`, { method: 'DELETE' });
      message.success(purge ? L.deletedPurge : L.deleted);
      refresh();
    } catch (e) {
      message.error(String(e));
    }
  };

  const columns = [
    {
      title: L.colSandbox,
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
      title: L.colBackend,
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
      title: L.colDiskRead,
      dataIndex: 'fsMode',
      key: 'fsMode',
      render: (m: string) => <Tag>{m}</Tag>,
    },
    {
      title: L.colNetwork,
      dataIndex: 'network',
      key: 'network',
      render: (n: boolean) =>
        n ? <Tag color="orange">{L.on}</Tag> : <Tag color="green">{L.off}</Tag>,
    },
    {
      title: L.colLimits,
      key: 'limits',
      render: (_: unknown, r: SandboxRow) => (
        <Text type="secondary" style={{ fontSize: 12 }}>
          {r.cpus} CPU · {r.memoryMb} MB · {(r.timeoutMs / 1000).toFixed(0)}s
        </Text>
      ),
    },
    {
      title: L.colStatus,
      dataIndex: 'status',
      key: 'status',
      render: (s: string, r: SandboxRow) => (
        <Tooltip title={r.lastError || undefined}>
          <Tag color={s === 'running' ? 'green' : s === 'error' ? 'red' : 'default'}>{s}</Tag>
        </Tooltip>
      ),
    },
    {
      title: L.colLastUsed,
      key: 'used',
      render: (_: unknown, r: SandboxRow) => fmtTime(r.lastUsedAt ?? r.createdAt),
    },
    {
      title: '',
      key: 'actions',
      render: (_: unknown, r: SandboxRow) => (
        <Space>
          <Tooltip title={L.killAllTip}>
            <Button size="small" icon={<StopOutlined />} onClick={() => killAll(r.id)} />
          </Tooltip>
          {r.backend === 'docker' && r.status === 'running' && (
            <Tooltip title={L.stopContainerTip}>
              <Button size="small" icon={<PoweroffOutlined />} onClick={() => stopSandbox(r.id)} />
            </Tooltip>
          )}
          <Popconfirm
            title={L.deleteTitle}
            description={L.deleteDesc}
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
            {L.stopAll}
          </Button>
          <Popconfirm title={L.purgeTitle} onConfirm={() => deleteSandbox(r.id, true)}>
            <Button size="small" danger>
              {L.purgeBtn}
            </Button>
          </Popconfirm>
        </Space>
        <Table<ProcRow>
          size="small"
          style={{ marginTop: 12 }}
          rowKey="pid"
          pagination={false}
          dataSource={s?.processes ?? []}
          locale={{ emptyText: L.noProcs }}
          columns={[
            { title: L.colPid, dataIndex: 'pid', width: 90 },
            { title: L.colCpu, dataIndex: 'cpu', width: 90, render: (v?: number) => (v ?? 0).toFixed(1) },
            { title: L.colRam, dataIndex: 'rssMb', width: 110, render: (v?: number) => (v ?? 0).toFixed(0) },
            { title: L.colElapsed, dataIndex: 'elapsed', width: 110 },
            { title: L.colCommand, dataIndex: 'command', ellipsis: true },
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
            <Text type="secondary">{L.subtitle}</Text>
          </div>
          <Space>
            <Button icon={<ReloadOutlined />} onClick={refresh} loading={loading}>
              {L.refresh}
            </Button>
          </Space>
        </Space>

        {caps && (
          <Alert
            style={{ marginTop: 16 }}
            type={caps.direct?.available || caps.docker?.available ? 'info' : 'warning'}
            showIcon
            message={
              <Space wrap>
                <span>{L.capsTitle}</span>
                {caps.direct && (
                  <span>
                    direct {isolationTag(caps.direct.kind)}{' '}
                    <Text type="secondary">({caps.direct.detail})</Text>
                  </span>
                )}
                <Divider type="vertical" />
                <span>
                  docker{' '}
                  {caps.docker?.available ? (
                    <Tag color="blue">{L.ready}</Tag>
                  ) : (
                    <Tag>{L.notAvailable}</Tag>
                  )}{' '}
                  <Text type="secondary">({caps.docker?.detail})</Text>
                </span>
              </Space>
            }
          />
        )}

        {policy && (
          <Card title={L.policyTitle} size="small" style={{ marginTop: 16 }}>
            <Row gutter={[24, 12]}>
              <Col xs={24} md={12}>
                <Space direction="vertical" style={{ width: '100%' }}>
                  <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                    <span>
                      <Text strong>{L.execTitle}</Text>
                      <br />
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {L.execDesc}
                      </Text>
                    </span>
                    <Switch checked={policy.execShell} onChange={(v) => savePolicy({ execShell: v })} />
                  </Space>
                  {policy.execShell && (
                    <Space size="large" style={{ paddingLeft: 12 }}>
                      <span>
                        {L.network}{' '}
                        <Switch
                          size="small"
                          checked={policy.execNetwork}
                          onChange={(v) => savePolicy({ execNetwork: v })}
                        />
                      </span>
                      <span>
                        {L.diskRead}{' '}
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
                  {policy.execShell && (
                    <div style={{ paddingLeft: 12 }}>
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {L.localPortsDesc}
                      </Text>
                      <Input
                        size="small"
                        style={{ marginTop: 4, fontFamily: 'monospace' }}
                        defaultValue={(policy.execLoopback ?? []).join(', ')}
                        placeholder="3000, 5173"
                        onBlur={(e) => {
                          const ports = e.target.value
                            .split(/[,\s]+/)
                            .map((x) => Number(x.trim()))
                            .filter((n) => Number.isInteger(n) && n > 0 && n < 65536);
                          if (ports.join() !== (policy.execLoopback ?? []).join())
                            savePolicy({ execLoopback: ports });
                        }}
                      />
                    </div>
                  )}
                  <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                    <span>
                      <Text strong>{L.schedTitle}</Text>
                      <br />
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {L.schedDesc}
                      </Text>
                    </span>
                    <Switch
                      checked={policy.schedulerScript}
                      onChange={(v) => savePolicy({ schedulerScript: v })}
                    />
                  </Space>
                  {policy.schedulerScript && (
                    <span style={{ paddingLeft: 12 }}>
                      {L.network}{' '}
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
                      <Text strong>{L.runPythonTitle}</Text>
                      <br />
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {L.runPythonDesc}
                      </Text>
                    </span>
                    <Switch checked={policy.runPython} onChange={(v) => savePolicy({ runPython: v })} />
                  </Space>
                  <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                    <span>
                      <Text strong>{L.runNodeTitle}</Text>
                      <br />
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {L.runNodeDesc}
                      </Text>
                    </span>
                    <Switch checked={policy.runNode} onChange={(v) => savePolicy({ runNode: v })} />
                  </Space>
                  <Space style={{ justifyContent: 'space-between', width: '100%' }}>
                    <span>
                      <Text strong>{L.codeNetTitle}</Text>
                      <br />
                      <Text type="secondary" style={{ fontSize: 12 }}>
                        {L.codeNetDesc}
                      </Text>
                    </span>
                    <Switch checked={policy.codeNetwork} onChange={(v) => savePolicy({ codeNetwork: v })} />
                  </Space>
                </Space>
              </Col>
            </Row>
          </Card>
        )}

        {/* The apps this machine actually runs, before the engine's own
            throwaway sandboxes: that is the list people come here to check. */}
        <SandboxAppsCard vi={lang === 'vi'} />

        <Card title={L.sessionsTitle(rows.length)} size="small" style={{ marginTop: 16 }}>
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
            locale={{ emptyText: L.emptySandboxes }}
          />
        </Card>

        <Card title={L.runsTitle} size="small" style={{ marginTop: 16 }}>
          <Table<RunRow>
            size="small"
            rowKey="id"
            dataSource={runs}
            pagination={{ pageSize: 8, hideOnSinglePage: true }}
            columns={[
              { title: L.colWhen, dataIndex: 'createdAt', width: 170, render: fmtTime },
              {
                title: L.colKind,
                key: 'kind',
                width: 110,
                render: (_: unknown, r: RunRow) => <Tag>{r.language || r.kind}</Tag>,
              },
              { title: L.colSource, dataIndex: 'source', ellipsis: true },
              {
                title: L.colResult,
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
              { title: L.colIsolation, dataIndex: 'isolation', width: 120, render: isolationTag },
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
          <Card title={L.defaultsTitle} size="small" style={{ marginTop: 16, marginBottom: 24 }}>
            <Row gutter={[24, 12]}>
              <Col xs={24} md={8}>
                <Space direction="vertical" style={{ width: '100%' }}>
                  <span>
                    {L.defaultDiskRead}{' '}
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
                    {L.defaultNetwork}{' '}
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
                    {L.ram}{' '}
                    <InputNumber
                      size="small"
                      min={64}
                      max={65536}
                      value={defaults.defaultMemoryMb}
                      onChange={(v) => v && saveDefaults({ defaultMemoryMb: v })}
                    />
                  </span>
                  <span>
                    {L.cpu}{' '}
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
                    {L.deadline}{' '}
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
                <Space direction="vertical" style={{ width: '100%' }} size={4}>
                  <Text>{L.allowlistLabel}</Text>
                  {defaults.allowlist.length === 0 ? (
                    <Text type="secondary" style={{ fontSize: 12 }}>
                      {L.allowlistEmpty}
                    </Text>
                  ) : (
                    <Space direction="vertical" size={2} style={{ width: '100%' }}>
                      {defaults.allowlist.map((path) => (
                        <Tag
                          key={path}
                          closable
                          onClose={(e) => {
                            e.preventDefault();
                            saveDefaults(
                              {},
                              defaults.allowlist.filter((p) => p !== path),
                            );
                          }}
                          style={{
                            fontFamily: 'monospace',
                            maxWidth: '100%',
                            whiteSpace: 'normal',
                            wordBreak: 'break-all',
                          }}
                        >
                          {path}
                        </Tag>
                      ))}
                    </Space>
                  )}
                  <Space.Compact style={{ width: '100%' }}>
                    <Input
                      size="small"
                      value={newAllowPath}
                      onChange={(e) => setNewAllowPath(e.target.value)}
                      onPressEnter={addAllowPath}
                      placeholder={L.addPathPlaceholder}
                      style={{ fontFamily: 'monospace' }}
                    />
                    <Button size="small" onClick={addAllowPath}>
                      {L.add}
                    </Button>
                  </Space.Compact>
                </Space>
              </Col>
            </Row>
          </Card>
        )}
      </div>
    </div>
  );
}
