import {
  CheckCircleOutlined,
  ClockCircleOutlined,
  CloseCircleOutlined,
  LoadingOutlined,
  PictureOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  StopOutlined,
  ThunderboltOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Button,
  Card,
  Col,
  Collapse,
  Empty,
  Modal,
  Progress,
  Row,
  Select,
  Space,
  Switch,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import { useEffect, useState } from "react";
import { api, type WorkflowRunJson } from "@/lib/api/client";
import {
  countProgress,
  groupSteps,
  isFailedStatus,
  isSkippedStatus,
  isTerminalRunStatus,
  normalizeWorkflowRun,
  statusMeta,
  type NormalizedStep,
} from "./workflowRun";

const { Title, Text, Paragraph } = Typography;

const POLL_MS = 2500;

/**
 * Heuristic: the daemon workflow endpoints are missing / not reachable.
 *
 * Note the third case — an older daemon serves the SPA `index.html` with a 200
 * for any unknown `/api/...` path instead of a 404, so the failure surfaces as
 * a JSON `SyntaxError` on an HTML body rather than as an HTTP status. Without
 * this branch the user gets "Unexpected token '<'" instead of a real message.
 */
function isApiUnavailable(e: unknown): boolean {
  const msg = e instanceof Error ? e.message : String(e ?? "");
  return (
    /^(404|405|501|502|503|504)\b/.test(msg) ||
    /Failed to fetch|NetworkError/i.test(msg) ||
    (e instanceof SyntaxError && /Unexpected token|not valid JSON/i.test(msg))
  );
}

function errMsg(e: unknown): string {
  return e instanceof Error ? e.message : String(e ?? "");
}

// ---- small pieces ----

function StatusTag({ status, style }: { status: string; style?: React.CSSProperties }) {
  const meta = statusMeta(status);
  const icon =
    meta.color === "success" ? <CheckCircleOutlined />
    : meta.color === "processing" ? <LoadingOutlined spin />
    : meta.color === "error" ? <CloseCircleOutlined />
    : <ClockCircleOutlined />;
  return (
    <Tag color={meta.color} icon={icon} style={{ margin: 0, ...style }}>
      {meta.label}
    </Tag>
  );
}

function StageChip({ step }: { step: NormalizedStep }) {
  const chip = (
    <Space
      size={6}
      style={{
        padding: "5px 10px",
        borderRadius: 8,
        background: "var(--surface-raised, #f5f5f5)",
        border: "1px solid var(--border, #eee)",
      }}
    >
      <Text style={{ fontSize: 12, fontFamily: "var(--mono)" }}>{step.label ?? step.id}</Text>
      <StatusTag status={step.status} />
    </Space>
  );
  return step.error ? <Tooltip title={step.error}>{chip}</Tooltip> : chip;
}

function SceneCell({ step, icon, title }: { step?: NormalizedStep; icon: React.ReactNode; title: string }) {
  const body = (
    <Space size={5}>
      <Text style={{ fontSize: 12, color: "var(--muted)" }}>{icon}</Text>
      <Text type="secondary" style={{ fontSize: 11 }}>{title}</Text>
      {step ? <StatusTag status={step.status} style={{ fontSize: 10 }} /> : <Tag style={{ margin: 0, fontSize: 10 }}>—</Tag>}
    </Space>
  );
  return step?.error ? <Tooltip title={step.error}>{body}</Tooltip> : body;
}

// ---- main panel ----

export function WorkflowPipelinePanel({
  projectId,
  onRunningChange,
}: {
  projectId: string;
  /** Lets the page keep its studio preview polling while a run is in flight. */
  onRunningChange?: (running: boolean) => void;
}) {
  const qc = useQueryClient();

  const [runId, setRunId] = useState<string | null>(null);
  const [orientation, setOrientation] = useState<"VERTICAL" | "HORIZONTAL">("VERTICAL");
  const [withAudio, setWithAudio] = useState(true);
  const [withCritic, setWithCritic] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  // Resume an existing run when the project changes / on mount.
  const existingQ = useQuery({
    queryKey: ["workflow-project-run", projectId],
    queryFn: () => api.getProjectWorkflowRun(projectId),
    enabled: !!projectId,
    retry: false,
    staleTime: 5_000,
  });

  useEffect(() => {
    setRunId(null);
    setErr(null);
  }, [projectId]);

  useEffect(() => {
    const existing = existingQ.data?.run_id;
    if (existing) setRunId(existing);
  }, [existingQ.data]);

  const runQ = useQuery({
    queryKey: ["workflow-run", runId],
    queryFn: () => api.getWorkflowRun(runId!),
    enabled: !!runId,
    retry: false,
    refetchInterval: (query) => {
      const data = query.state.data as WorkflowRunJson | undefined;
      if (!data) return POLL_MS;
      const { status } = normalizeWorkflowRun(data);
      return isTerminalRunStatus(status) ? false : POLL_MS;
    },
  });

  const startM = useMutation({
    mutationFn: () =>
      api.startWorkflowPipeline({
        project_id: projectId,
        orientation,
        with_audio: withAudio,
        with_critic: withCritic,
      }),
    onSuccess: (data) => {
      setErr(null);
      setRunId(data.run_id);
      void qc.invalidateQueries({ queryKey: ["workflow-project-run", projectId] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const cancelM = useMutation({
    mutationFn: () => api.cancelWorkflowRun(runId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["workflow-run", runId] }),
    onError: (e: Error) => setErr(e.message),
  });

  // The workflow API is considered unavailable only when the *probe* fails that
  // way — a failed start is surfaced as a normal error instead.
  const apiDown =
    (existingQ.isError && isApiUnavailable(existingQ.error)) ||
    (startM.isError && isApiUnavailable(startM.error));

  const raw = runQ.data;
  const run = raw ? normalizeWorkflowRun(raw) : null;
  const progress = run ? countProgress(run.steps) : null;
  const grouped = run ? groupSteps(run.steps) : null;
  // A slot with no scene behind it reports "skipped". Showing those as scene
  // cards made the header disagree with the project's real scene count, so
  // they are counted separately instead.
  const activeScenes = (grouped?.scenes ?? []).filter(
    (sc) => !(isSkippedStatus(sc.image?.status ?? "") && isSkippedStatus(sc.video?.status ?? "")),
  );
  const emptySlots = (grouped?.scenes.length ?? 0) - activeScenes.length;
  const isRunning = !!run && !isTerminalRunStatus(run.status);
  const runMeta = run ? statusMeta(run.status) : null;

  useEffect(() => {
    onRunningChange?.(isRunning);
  }, [isRunning, onRunningChange]);

  const confirmCancel = () => {
    Modal.confirm({
      title: "Huỷ workflow?",
      content: "Các node đang chạy sẽ bị dừng. Không thể hoàn tác.",
      okText: "Huỷ run",
      okType: "danger",
      cancelText: "Đóng",
      onOk: () => cancelM.mutate(),
    });
  };

  if (!projectId) {
    return (
      <Card style={{ textAlign: "center", padding: 32 }}>
        <Text type="secondary">Chọn project để bắt đầu</Text>
      </Card>
    );
  }

  return (
    <>
      {apiDown && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 16 }}
          message="Workflow engine chưa sẵn sàng"
          description={
            <>
              Daemon chưa expose API <code>/api/pipeline/workflow</code> (hoặc không kết nối được).
              Hãy cập nhật &amp; khởi động lại daemon, hoặc chuyển sang chế độ <b>DAG cũ</b> ở phía trên
              để chạy pipeline tuần tự.
            </>
          }
        />
      )}

      {err && !apiDown && (
        <Alert type="error" message={err} closable onClose={() => setErr(null)} style={{ marginBottom: 16 }} />
      )}

      {/* Controls */}
      <Card style={{ marginBottom: 16 }}>
        <Row gutter={[16, 12]} align="middle">
          <Col xs={24} sm={8}>
            <Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 4 }}>
              Orientation
            </Text>
            <Select
              value={orientation}
              onChange={setOrientation}
              style={{ width: "100%" }}
              options={[
                { label: "Dọc (9:16)", value: "VERTICAL" },
                { label: "Ngang (16:9)", value: "HORIZONTAL" },
              ]}
            />
          </Col>
          <Col xs={12} sm={5}>
            <Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 4 }}>
              Lồng tiếng
            </Text>
            <Switch checked={withAudio} onChange={setWithAudio} checkedChildren="Có" unCheckedChildren="Không" />
          </Col>
          <Col xs={12} sm={5}>
            <Text type="secondary" style={{ fontSize: 12, display: "block", marginBottom: 4 }}>
              Critic
            </Text>
            <Switch checked={withCritic} onChange={setWithCritic} checkedChildren="Có" unCheckedChildren="Không" />
          </Col>
          <Col xs={24} sm={6} style={{ textAlign: "right" }}>
            <Space>
              {isRunning ? (
                <Button danger icon={<StopOutlined />} loading={cancelM.isPending} onClick={confirmCancel}>
                  Huỷ
                </Button>
              ) : (
                <Button
                  type="primary"
                  icon={<PlayCircleOutlined />}
                  loading={startM.isPending}
                  disabled={apiDown}
                  onClick={() => startM.mutate()}
                >
                  Chạy pipeline
                </Button>
              )}
              <Tooltip title="Làm mới trạng thái">
                <Button
                  icon={<ReloadOutlined />}
                  loading={runQ.isFetching}
                  disabled={!runId}
                  onClick={() => void runQ.refetch()}
                />
              </Tooltip>
            </Space>
          </Col>
        </Row>
        <Paragraph type="secondary" style={{ fontSize: 12, margin: "12px 0 0" }}>
          <ThunderboltOutlined /> Mỗi cảnh là một node riêng — ảnh và video của các cảnh chạy song song
          (tối đa 5 node cùng lúc), thay vì xử lý tuần tự từng cảnh.
        </Paragraph>
      </Card>

      {/* Run view */}
      {!runId && !startM.isPending && (
        <Card>
          <Empty description="Chưa có run nào cho project này. Bấm “Chạy pipeline” để bắt đầu." />
        </Card>
      )}

      {runId && runQ.isError && !apiDown && (
        <Alert
          type="error"
          showIcon
          message="Không đọc được trạng thái run"
          description={errMsg(runQ.error)}
          style={{ marginBottom: 16 }}
        />
      )}

      {runId && run && (
        <Card
          title={
            <Space>
              <Text strong>Workflow run</Text>
              {runMeta && <Tag color={runMeta.color}>{runMeta.label}</Tag>}
              <Text type="secondary" style={{ fontSize: 12, fontFamily: "var(--mono)" }}>
                {runId.slice(0, 8)}…
              </Text>
            </Space>
          }
          style={{ marginBottom: 16 }}
        >
          {!run.matched ? (
            <Alert
              type="info"
              showIcon
              message="Không nhận diện được cấu trúc run JSON"
              description="Hiển thị dữ liệu thô bên dưới."
              style={{ marginBottom: 12 }}
            />
          ) : (
            <>
              {progress && progress.total > 0 && (
                <div style={{ marginBottom: 16 }}>
                  <Space style={{ width: "100%", justifyContent: "space-between", marginBottom: 4 }}>
                    <Text style={{ fontSize: 12 }}>
                      {progress.done}/{progress.total} node xong
                    </Text>
                    <Space size={6}>
                      {progress.running > 0 && <Tag color="processing">{progress.running} đang chạy</Tag>}
                      {progress.failed > 0 && <Tag color="error">{progress.failed} lỗi</Tag>}
                    </Space>
                  </Space>
                  <Progress
                    percent={Math.round((progress.done / progress.total) * 100)}
                    status={
                      progress.failed > 0 ? "exception"
                      : progress.done === progress.total ? "success"
                      : "active"
                    }
                  />
                </div>
              )}

              {grouped && grouped.planning.length > 0 && (
                <div style={{ marginBottom: 16 }}>
                  <Title level={5} style={{ fontSize: 13, marginBottom: 8 }}>Chuẩn bị</Title>
                  <Space wrap size={8}>
                    {grouped.planning.map((s) => <StageChip key={s.id} step={s} />)}
                  </Space>
                </div>
              )}

              {grouped && activeScenes.length > 0 && (
                <div style={{ marginBottom: 16 }}>
                  <Title level={5} style={{ fontSize: 13, marginBottom: 8 }}>
                    Cảnh ({activeScenes.length}) — chạy song song
                    {emptySlots > 0 && (
                      <Text type="secondary" style={{ fontSize: 12, fontWeight: 400, marginLeft: 8 }}>
                        + {emptySlots} chỗ trống
                      </Text>
                    )}
                  </Title>
                  <div
                    style={{
                      display: "grid",
                      gridTemplateColumns: "repeat(auto-fill, minmax(210px, 1fr))",
                      gap: 10,
                    }}
                  >
                    {activeScenes.map((sc) => {
                      const failed =
                        (sc.image && isFailedStatus(sc.image.status)) ||
                        (sc.video && isFailedStatus(sc.video.status));
                      return (
                        <Card
                          key={sc.index}
                          size="small"
                          styles={{ body: { padding: "10px 12px" } }}
                          style={{ borderColor: failed ? "#e85d5d" : undefined }}
                        >
                          <Space direction="vertical" size={6} style={{ width: "100%" }}>
                            <Text strong style={{ fontSize: 12 }}>Cảnh {sc.index + 1}</Text>
                            <SceneCell step={sc.image} icon={<PictureOutlined />} title="Ảnh" />
                            <SceneCell step={sc.video} icon={<VideoCameraOutlined />} title="Video" />
                          </Space>
                        </Card>
                      );
                    })}
                  </div>
                </div>
              )}

              {grouped && grouped.post.length > 0 && (
                <div style={{ marginBottom: 8 }}>
                  <Title level={5} style={{ fontSize: 13, marginBottom: 8 }}>Hậu kỳ</Title>
                  <Space wrap size={8}>
                    {grouped.post.map((s) => <StageChip key={s.id} step={s} />)}
                  </Space>
                </div>
              )}
            </>
          )}

          <Collapse
            ghost
            size="small"
            style={{ marginTop: 8 }}
            items={[
              {
                key: "raw",
                label: <Text type="secondary" style={{ fontSize: 12 }}>Run JSON (thô)</Text>,
                children: (
                  <pre
                    style={{
                      fontFamily: "var(--mono)",
                      fontSize: 11,
                      margin: 0,
                      maxHeight: 320,
                      overflow: "auto",
                      whiteSpace: "pre-wrap",
                    }}
                  >
                    {JSON.stringify(raw, null, 2)}
                  </pre>
                ),
              },
            ]}
          />
        </Card>
      )}
    </>
  );
}
