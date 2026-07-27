import {
  AudioOutlined,
  BranchesOutlined,
  CheckCircleOutlined,
  ClockCircleOutlined,
  CloseOutlined,
  CopyOutlined,
  EyeOutlined,
  FileTextOutlined,
  LoadingOutlined,
  MinusCircleOutlined,
  RobotOutlined,
  ScissorOutlined,
  StopOutlined,
  UserOutlined,
  VideoCameraOutlined,
  WarningOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge, Button, Modal, Popconfirm, Select, Space, Tabs, Tag, Tooltip, Typography, message } from "antd";
import { useMemo, useState } from "react";
import { api, type AgentLogEntry } from "@/lib/api/client";

const { Text, Paragraph } = Typography;

const AGENT_ICON: Record<string, React.ReactNode> = {
  orchestrator:  <BranchesOutlined />,
  script_parser: <FileTextOutlined />,
  character:     <UserOutlined />,
  image:         <RobotOutlined />,
  video:         <VideoCameraOutlined />,
  audio:         <AudioOutlined />,
  concat:        <ScissorOutlined />,
  // pre-production
  director:       <BranchesOutlined />,
  screenwriter:   <FileTextOutlined />,
  scene_plan:     <RobotOutlined />,
  shot_design:    <VideoCameraOutlined />,
  visual_asset:   <UserOutlined />,
  critic:         <WarningOutlined />,
  director_frame: <ScissorOutlined />,
};

const STATUS_CONFIG: Record<string, { color: string; tag: string; tagColor: string }> = {
  active:     { color: "var(--accent)",  tag: "chạy",    tagColor: "processing" },
  registered: { color: "var(--muted)",   tag: "chờ",     tagColor: "default"    },
  done:       { color: "#52c41a",        tag: "xong",    tagColor: "success"    },
  error:      { color: "#ff4d4f",        tag: "lỗi",     tagColor: "error"      },
  timeout:    { color: "#fa8c16",        tag: "timeout", tagColor: "warning"    },
  blocked:    { color: "#8c8c8c",        tag: "bị chặn", tagColor: "default"    },
};

function elapsed(startedAt: string | null): string {
  if (!startedAt) return "";
  const sec = Math.round((Date.now() - new Date(startedAt).getTime()) / 1000);
  if (sec < 60) return `${sec}s`;
  return `${Math.floor(sec / 60)}m ${sec % 60}s`;
}

function duration(startedAt: string | null, completedAt: string | null): string {
  if (!startedAt || !completedAt) return "";
  const sec = Math.round((new Date(completedAt).getTime() - new Date(startedAt).getTime()) / 1000);
  if (sec < 60) return `${sec}s`;
  return `${Math.floor(sec / 60)}m ${sec % 60}s`;
}

function formatTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  return d.toLocaleTimeString("vi-VN", { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

/** Cùng pipeline: theo thứ tự bắt đầu thực thi; nhiều pipeline: cái hoàn tất mới hơn lên trước. */
function sortAgentHistory<T extends { pipeline_id: string; task_label: string; started_at: string | null; completed_at: string | null }>(
  rows: T[],
): T[] {
  if (rows.length < 2) return rows;
  const byPipeline = new Map<string, T[]>();
  for (const e of rows) {
    if (!byPipeline.has(e.pipeline_id)) byPipeline.set(e.pipeline_id, []);
    byPipeline.get(e.pipeline_id)!.push(e);
  }
  const groups = [...byPipeline.entries()].sort((a, b) => {
    const maxB = Math.max(0, ...b[1].map((x) => new Date(x.completed_at || 0).getTime()));
    const maxA = Math.max(0, ...a[1].map((x) => new Date(x.completed_at || 0).getTime()));
    return maxB - maxA;
  });
  const out: T[] = [];
  for (const [, tasks] of groups) {
    out.push(
      ...[...tasks].sort((x, y) => {
        const ta = new Date(x.started_at || 0).getTime();
        const tb = new Date(y.started_at || 0).getTime();
        if (ta !== tb) return ta - tb;
        return x.task_label.localeCompare(y.task_label);
      }),
    );
  }
  return out;
}

/** Pretty-print JSON string; fall back to raw if parse fails. */
function prettyJSON(raw: string): string {
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

// ---------- Output Modal ----------

interface OutputModalProps {
  entry: AgentLogEntry | null;
  onClose: () => void;
}

function OutputModal({ entry, onClose }: OutputModalProps) {
  const [msg, msgCtx] = message.useMessage();

  if (!entry) return null;

  const pretty = entry.result ? prettyJSON(entry.result) : "";
  const cfg = STATUS_CONFIG[entry.status] ?? STATUS_CONFIG.registered;

  const handleCopy = () => {
    navigator.clipboard.writeText(pretty).then(
      () => void msg.success("Đã copy"),
      () => void msg.error("Copy thất bại"),
    );
  };

  return (
    <>
      {msgCtx}
      <Modal
        open
        onCancel={onClose}
        footer={null}
        width={680}
        title={
          <Space size={8}>
            <Text strong style={{ fontSize: 14 }}>{entry.task_label}</Text>
            <Tag color={cfg.tagColor} style={{ fontSize: 11, margin: 0 }}>{cfg.tag}</Tag>
            <Text type="secondary" style={{ fontSize: 12, fontWeight: 400 }}>
              {entry.agent_type}
            </Text>
          </Space>
        }
        styles={{ body: { padding: "12px 16px" } }}
      >
        {/* Meta row */}
        <div style={{ display: "flex", gap: 16, marginBottom: 10, flexWrap: "wrap" }}>
          <Text type="secondary" style={{ fontSize: 11 }}>
            Pipeline: <code style={{ fontSize: 11 }}>{entry.pipeline_id.slice(0, 12)}…</code>
          </Text>
          {entry.started_at && entry.completed_at && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              Thời gian: <strong>{duration(entry.started_at, entry.completed_at)}</strong>
            </Text>
          )}
          {entry.completed_at && (
            <Text type="secondary" style={{ fontSize: 11 }}>
              Hoàn thành lúc: {formatTime(entry.completed_at)}
            </Text>
          )}
        </div>

        {/* Result JSON */}
        {pretty ? (
          <>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6 }}>
              <Text style={{ fontSize: 12, fontWeight: 600 }}>Output JSON</Text>
              <Button
                size="small"
                icon={<CopyOutlined />}
                onClick={handleCopy}
                style={{ fontSize: 11 }}
              >
                Copy
              </Button>
            </div>
            <div
              style={{
                background: "var(--bg, #f5f5f5)",
                border: "1px solid var(--border, #e0e0e0)",
                borderRadius: 6,
                padding: "10px 12px",
                maxHeight: 480,
                overflowY: "auto",
              }}
            >
              <pre
                style={{
                  margin: 0,
                  fontSize: 11,
                  lineHeight: 1.55,
                  fontFamily: "var(--mono, 'Fira Mono', monospace)",
                  whiteSpace: "pre-wrap",
                  wordBreak: "break-all",
                  color: "var(--text, #1a1a1a)",
                }}
              >
                {pretty}
              </pre>
            </div>
          </>
        ) : (
          <Paragraph type="secondary" style={{ fontSize: 12, margin: 0 }}>
            Không có output data.
          </Paragraph>
        )}
      </Modal>
    </>
  );
}

// ---------- Log Row ----------

function LogRow({
  entry,
  showTime,
  onViewOutput,
  onStop,
  stopping,
}: {
  entry: AgentLogEntry;
  showTime?: boolean;
  onViewOutput?: (e: AgentLogEntry) => void;
  onStop?: (e: AgentLogEntry) => void;
  stopping?: boolean;
}) {
  const icon = AGENT_ICON[entry.agent_type] ?? <RobotOutlined />;
  const cfg = STATUS_CONFIG[entry.status] ?? STATUS_CONFIG.registered;
  const isActive  = entry.status === "active";
  const isError   = entry.status === "error" || entry.status === "timeout";
  const isBlocked = entry.status === "blocked";
  const isDone    = entry.status === "done";
  const hasOutput = !!entry.result;

  const statusIcon = isActive
    ? <LoadingOutlined spin />
    : isBlocked
      ? <MinusCircleOutlined />
      : isError
        ? entry.status === "timeout" ? <ClockCircleOutlined /> : <WarningOutlined />
        : isDone
          ? <CheckCircleOutlined />
          : icon;

  return (
    <div
      style={{
        padding: "8px 12px",
        borderBottom: "1px solid var(--border)",
        display: "flex",
        alignItems: "flex-start",
        gap: 8,
        background: isError   ? "rgba(255,77,79,0.05)"
                  : isBlocked ? "rgba(0,0,0,0.03)"
                  : undefined,
        opacity: isBlocked ? 0.75 : 1,
      }}
    >
      <div style={{ paddingTop: 2, color: cfg.color, flexShrink: 0 }}>
        {statusIcon}
      </div>
      <div style={{ minWidth: 0, flex: 1 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6, flexWrap: "wrap" }}>
          <Text strong style={{ fontSize: 12 }}>{entry.task_label}</Text>
          <Tag color={cfg.tagColor} style={{ fontSize: 10, margin: 0 }}>{cfg.tag}</Tag>
          {isDone && entry.started_at && entry.completed_at && (
            <Text type="secondary" style={{ fontSize: 10 }}>
              {duration(entry.started_at, entry.completed_at)}
            </Text>
          )}
          {/* View output button — shown when result is available */}
          {hasOutput && onViewOutput && (
            <Tooltip title="Xem output JSON">
              <Button
                type="text"
                size="small"
                icon={<EyeOutlined />}
                style={{ fontSize: 10, padding: "0 4px", height: 18, color: "var(--muted)" }}
                onClick={() => onViewOutput(entry)}
              />
            </Tooltip>
          )}
          {/* Force-stop button — shown for active and registered tasks */}
          {onStop && (
            <Popconfirm
              title="Dừng agent này?"
              okText="Dừng" cancelText="Huỷ"
              okButtonProps={{ danger: true, size: "small" }}
              onConfirm={() => onStop(entry)}
            >
              <Tooltip title="Dừng ngay">
                <Button
                  type="text"
                  size="small"
                  danger
                  loading={stopping}
                  icon={<StopOutlined />}
                  style={{ fontSize: 10, padding: "0 4px", height: 18 }}
                />
              </Tooltip>
            </Popconfirm>
          )}
        </div>
        <Text type="secondary" style={{ fontSize: 11 }}>
          {entry.agent_type}
          {isActive && entry.started_at ? ` · ${elapsed(entry.started_at)}` : ""}
        </Text>

        {isBlocked && entry.blocked_by && (
          <div style={{ marginTop: 4, padding: "3px 6px", background: "rgba(0,0,0,0.06)", borderRadius: 4, border: "1px solid rgba(0,0,0,0.12)" }}>
            <Text style={{ fontSize: 10, color: "var(--muted)" }}>
              Chờ: <strong>{entry.blocked_by}</strong> thất bại
            </Text>
          </div>
        )}

        {isError && entry.error_message && (
          <Tooltip title={entry.error_message}>
            <div style={{ marginTop: 4, padding: "3px 6px", background: "rgba(255,77,79,0.1)", borderRadius: 4, border: "1px solid rgba(255,77,79,0.3)", cursor: "default" }}>
              <Text style={{ fontSize: 10, color: "#ff4d4f", display: "block" }}>
                {entry.error_message.length > 80
                  ? entry.error_message.slice(0, 80) + "…"
                  : entry.error_message}
              </Text>
            </div>
          </Tooltip>
        )}
        <br />
        <Text type="secondary" style={{ fontSize: 10 }}>
          pipeline {entry.pipeline_id.slice(0, 8)}…
          {showTime && entry.completed_at ? ` · ${formatTime(entry.completed_at)}` : ""}
        </Text>
      </div>
    </div>
  );
}

function SectionHeader({ label, color }: { label: string; color: string }) {
  return (
    <div style={{ padding: "5px 12px", background: "var(--bg)", borderBottom: "1px solid var(--border)" }}>
      <Text style={{ fontSize: 10, color, fontWeight: 600, textTransform: "uppercase" }}>
        {label}
      </Text>
    </div>
  );
}

// ---------- Drawer ----------

interface AgentLogDrawerProps {
  open: boolean;
  onClose: () => void;
}

export function AgentLogDrawer({ open, onClose }: AgentLogDrawerProps) {
  const qc = useQueryClient();
  const [outputEntry, setOutputEntry] = useState<AgentLogEntry | null>(null);
  const [stoppingIds, setStoppingIds] = useState<Set<string>>(new Set());
  const [historyStatusFilter, setHistoryStatusFilter] = useState<
    "all" | "done" | "error" | "timeout" | "blocked"
  >("all");

  const stopM = useMutation({
    mutationFn: (entry: AgentLogEntry) =>
      api.stopTask(entry.pipeline_id, entry.task_id),
    onMutate: (entry) =>
      setStoppingIds((s) => new Set(s).add(entry.task_id)),
    onSettled: (_, __, entry) => {
      setStoppingIds((s) => { const n = new Set(s); n.delete(entry.task_id); return n; });
      void qc.invalidateQueries({ queryKey: ["agent-log"] });
    },
  });

  const logQ = useQuery({
    queryKey: ["agent-log"],
    queryFn: () => api.listAgentLog(),
    refetchInterval: open ? 3_000 : false,
    enabled: open,
  });

  const historyQ = useQuery({
    queryKey: ["agent-history"],
    queryFn: () => api.listAgentHistory(),
    refetchInterval: open ? 10_000 : false,
    enabled: open,
  });

  const clearM = useMutation({
    mutationFn: () => api.clearAgentHistory(),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["agent-history"] }),
  });

  const entries = logQ.data ?? [];
  const active  = entries.filter((e) => e.status === "active");
  const pending = entries.filter((e) => e.status === "registered");

  const historyRaw = historyQ.data ?? [];
  const history =
    historyStatusFilter === "all"
      ? historyRaw
      : historyRaw.filter((e) => e.status === historyStatusFilter);
  const historyErrors = history.filter((e) => e.status === "error" || e.status === "timeout");
  const historyBlocked = history.filter((e) => e.status === "blocked");
  const historyDone = history.filter((e) => e.status === "done");

  const historyDoneSorted = useMemo(() => sortAgentHistory(historyDone), [historyDone]);
  const historyErrorsSorted = useMemo(() => sortAgentHistory(historyErrors), [historyErrors]);
  const historyBlockedSorted = useMemo(() => sortAgentHistory(historyBlocked), [historyBlocked]);

  if (!open) return null;

  const activeTabLabel = (
    <Space size={4}>
      <span>Active</span>
      {active.length > 0 && (
        <Badge count={active.length} style={{ backgroundColor: "var(--accent)", fontSize: 9 }} />
      )}
    </Space>
  );

  const historyTabLabel = (
    <Space size={4}>
      <span>History</span>
      {(historyErrors.length + historyBlocked.length) > 0 && (
        <Badge count={historyErrors.length + historyBlocked.length} style={{ backgroundColor: "#ff4d4f", fontSize: 9 }} />
      )}
    </Space>
  );

  return (
    <>
      <OutputModal entry={outputEntry} onClose={() => setOutputEntry(null)} />

      <div
        style={{
          position: "fixed",
          top: 0, right: 0,
          width: 320,
          height: "100vh",
          background: "var(--surface)",
          borderLeft: "1px solid var(--border)",
          display: "flex",
          flexDirection: "column",
          zIndex: 1000,
          boxShadow: "-4px 0 16px rgba(0,0,0,0.18)",
        }}
      >
        {/* Header */}
        <div style={{ padding: "12px 14px", borderBottom: "1px solid var(--border)", display: "flex", alignItems: "center", justifyContent: "space-between", flexShrink: 0 }}>
          <Text strong style={{ fontSize: 13 }}>Agent Log</Text>
          <CloseOutlined style={{ cursor: "pointer", color: "var(--muted)", fontSize: 13 }} onClick={onClose} />
        </div>

        {/* Tabs */}
        <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
          <Tabs
            defaultActiveKey="active"
            size="small"
            style={{ flex: 1, display: "flex", flexDirection: "column" }}
            tabBarStyle={{ margin: 0, padding: "0 12px", flexShrink: 0 }}
            items={[
              {
                key: "active",
                label: activeTabLabel,
                children: (
                  <div style={{ height: "calc(100vh - 120px)", overflowY: "auto" }}>
                    <div style={{ padding: "6px 14px", borderBottom: "1px solid var(--border)", display: "flex", gap: 12, flexWrap: "wrap" }}>
                      <Text style={{ fontSize: 11 }}>
                        <span style={{ color: "var(--accent)" }}>●</span>{" "}
                        <span style={{ color: "var(--text)" }}>{active.length} đang chạy</span>
                      </Text>
                      <Text style={{ fontSize: 11 }}>
                        <span style={{ color: "var(--muted)" }}>○</span>{" "}
                        <span style={{ color: "var(--muted)" }}>{pending.length} chờ</span>
                      </Text>
                    </div>

                    {logQ.isLoading && (
                      <div style={{ padding: 20, textAlign: "center" }}>
                        <LoadingOutlined style={{ color: "var(--muted)" }} />
                      </div>
                    )}
                    {!logQ.isLoading && entries.length === 0 && (
                      <div style={{ padding: "24px 14px", textAlign: "center" }}>
                        <Text type="secondary" style={{ fontSize: 12 }}>Không có agent nào đang chạy</Text>
                      </div>
                    )}

                    {active.length > 0 && (
                      <>
                        <SectionHeader label={`Đang chạy (${active.length})`} color="var(--accent)" />
                        {active.map((e) => (
                          <LogRow
                            key={e.task_id}
                            entry={e}
                            onStop={(entry) => stopM.mutate(entry)}
                            stopping={stoppingIds.has(e.task_id)}
                          />
                        ))}
                      </>
                    )}
                    {pending.length > 0 && (
                      <>
                        <SectionHeader label={`Đã lên lịch (${pending.length})`} color="var(--muted)" />
                        {pending.map((e) => (
                          <LogRow
                            key={e.task_id}
                            entry={e}
                            onStop={(entry) => stopM.mutate(entry)}
                            stopping={stoppingIds.has(e.task_id)}
                          />
                        ))}
                      </>
                    )}

                    <div style={{ padding: "6px 14px", borderTop: "1px solid var(--border)", display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                      <Text type="secondary" style={{ fontSize: 10 }}>Tự cập nhật mỗi 3s</Text>
                      <Popconfirm
                        title="Xoá toàn bộ lịch sử?"
                        onConfirm={() => clearM.mutate()}
                        okText="Xoá" cancelText="Huỷ"
                        okButtonProps={{ danger: true }}
                      >
                        <Button danger size="small" loading={clearM.isPending} style={{ fontSize: 10 }}>
                          Xóa lịch sử
                        </Button>
                      </Popconfirm>
                    </div>
                  </div>
                ),
              },
              {
                key: "history",
                label: historyTabLabel,
                children: (
                  <div style={{ height: "calc(100vh - 120px)", overflowY: "auto" }}>
                    <div
                      style={{
                        padding: "6px 14px",
                        borderBottom: "1px solid var(--border)",
                        display: "flex",
                        justifyContent: "space-between",
                        alignItems: "center",
                        gap: 8,
                      }}
                    >
                      <Text type="secondary" style={{ fontSize: 11 }}>
                        Lọc trạng thái
                      </Text>
                      <Select
                        size="small"
                        value={historyStatusFilter}
                        onChange={(v) => setHistoryStatusFilter(v)}
                        style={{ minWidth: 120 }}
                        options={[
                          { value: "all", label: "Tất cả" },
                          { value: "done", label: "Hoàn thành" },
                          { value: "error", label: "Lỗi" },
                          { value: "timeout", label: "Timeout" },
                          { value: "blocked", label: "Bị chặn" },
                        ]}
                      />
                    </div>
                    {historyQ.isLoading && (
                      <div style={{ padding: 20, textAlign: "center" }}>
                        <LoadingOutlined style={{ color: "var(--muted)" }} />
                      </div>
                    )}
                    {!historyQ.isLoading && history.length === 0 && (
                      <div style={{ padding: "24px 14px", textAlign: "center" }}>
                        <Text type="secondary" style={{ fontSize: 12 }}>Chưa có lịch sử chạy</Text>
                      </div>
                    )}

                    {historyErrors.length > 0 && (
                      <>
                        <SectionHeader label={`Lỗi (${historyErrors.length})`} color="#ff4d4f" />
                        {historyErrorsSorted.map((e) => (
                          <LogRow key={e.task_id} entry={e} showTime onViewOutput={setOutputEntry} />
                        ))}
                      </>
                    )}
                    {historyBlocked.length > 0 && (
                      <>
                        <SectionHeader label={`Bị chặn (${historyBlocked.length})`} color="#8c8c8c" />
                        {historyBlockedSorted.map((e) => (
                          <LogRow key={e.task_id} entry={e} showTime />
                        ))}
                      </>
                    )}
                    {historyDone.length > 0 && (
                      <>
                        <SectionHeader label={`Hoàn thành (${historyDone.length})`} color="#52c41a" />
                        {historyDoneSorted.map((e) => (
                          <LogRow key={e.task_id} entry={e} showTime onViewOutput={setOutputEntry} />
                        ))}
                      </>
                    )}

                    <div style={{ padding: "6px 14px", borderTop: "1px solid var(--border)" }}>
                      <Text type="secondary" style={{ fontSize: 10 }}>
                        100 task gần nhất · cập nhật mỗi 10s
                      </Text>
                    </div>
                  </div>
                ),
              },
            ]}
          />
        </div>
      </div>
    </>
  );
}
