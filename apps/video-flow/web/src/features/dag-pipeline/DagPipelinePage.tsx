import {
  AudioOutlined,
  BranchesOutlined,
  CheckCircleOutlined,
  ClockCircleOutlined,
  CloseCircleOutlined,
  DeleteOutlined,
  FieldTimeOutlined,
  FileTextOutlined,
  LoadingOutlined,
  PauseCircleOutlined,
  PlayCircleOutlined,
  ReloadOutlined,
  RedoOutlined,
  RobotOutlined,
  ScissorOutlined,
  StopOutlined,
  UserOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Badge,
  Button,
  Card,
  Col,
  Descriptions,
  Form,
  Input,
  List,
  message,
  Modal,
  Row,
  Segmented,
  Select,
  Space,
  Spin,
  Tag,
  Tooltip,
  Typography,
} from "antd";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, type CharacterRow, type DagTaskRow, type PipelineRow, type ProjectRow, type SceneRow } from "@/lib/api/client";
import { CustomWorkflowBuilder } from "./CustomWorkflowBuilder";
import { WorkflowPipelinePanel } from "./WorkflowPipelinePanel";

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

const { Title, Text, Paragraph } = Typography;
const { TextArea } = Input;

// ---- constants ----

const AGENT_ICON: Record<string, React.ReactNode> = {
  orchestrator: <BranchesOutlined />,
  script_parser: <FileTextOutlined />,
  character: <UserOutlined />,
  image: <RobotOutlined />,
  video: <VideoCameraOutlined />,
  audio: <AudioOutlined />,
  concat: <ScissorOutlined />,
};

const STATUS_TAG: Record<string, { color: string; icon: React.ReactNode; label: string }> = {
  registered: { color: "default", icon: <ClockCircleOutlined />, label: "Chờ" },
  active: { color: "processing", icon: <LoadingOutlined spin />, label: "Đang chạy" },
  done: { color: "success", icon: <CheckCircleOutlined />, label: "Xong" },
  error: { color: "error", icon: <CloseCircleOutlined />, label: "Lỗi" },
  timeout: { color: "warning", icon: <FieldTimeOutlined />, label: "Timeout" },
};

const PIPELINE_STATUS_TAG: Record<string, { color: string; label: string }> = {
  queued: { color: "default", label: "Đã tạo" },
  active: { color: "processing", label: "Đang chạy" },
  paused: { color: "warning", label: "Tạm dừng" },
  done: { color: "success", label: "Hoàn thành" },
  failed: { color: "error", label: "Thất bại" },
};

type DashEvent = { event: string; data: Record<string, unknown> };

function durationStr(startedAt: string | null, completedAt: string | null) {
  if (!startedAt) return null;
  const start = new Date(startedAt).getTime();
  const end = completedAt ? new Date(completedAt).getTime() : Date.now();
  const sec = Math.round((end - start) / 1000);
  return sec < 60 ? `${sec}s` : `${Math.floor(sec / 60)}m ${sec % 60}s`;
}

// ---- TaskCard ----

function TaskCard({
  task,
  pipelineId,
  onRetried,
}: {
  task: DagTaskRow;
  pipelineId: string;
  onRetried: () => void;
}) {
  const st = STATUS_TAG[task.status] ?? STATUS_TAG.registered;
  const icon = AGENT_ICON[task.agent_type] ?? <RobotOutlined />;
  const dur = durationStr(task.started_at, task.completed_at);
  const canRetry = task.status === "error" || task.status === "timeout";

  const retryM = useMutation({
    mutationFn: () => api.retryTask(pipelineId, task.id),
    onSuccess: onRetried,
  });

  return (
    <Card
      size="small"
      styles={{ body: { padding: "12px 14px" } }}
      style={{
        borderColor:
          task.status === "active" ? "#5b8def"
          : task.status === "done" ? "#3dcca8"
          : task.status === "error" ? "#e85d5d"
          : undefined,
      }}
    >
      <Space direction="vertical" size={4} style={{ width: "100%" }}>
        <Space style={{ width: "100%", justifyContent: "space-between" }}>
          <Space>
            <Text style={{ fontSize: 16, color: "var(--muted)" }}>{icon}</Text>
            <Text strong style={{ fontSize: 13 }}>{task.label}</Text>
            <Tag color={st.color} icon={st.icon} style={{ margin: 0 }}>{st.label}</Tag>
          </Space>
          {canRetry && (
            <Tooltip title="Retry task này">
              <Button
                size="small"
                icon={<RedoOutlined />}
                loading={retryM.isPending}
                onClick={() => retryM.mutate()}
              />
            </Tooltip>
          )}
        </Space>
        <Text type="secondary" style={{ fontSize: 11 }}>{task.agent_type}</Text>
        {(task.depends_on ?? []).length > 0 && (
          <Text type="secondary" style={{ fontSize: 11 }}>→ {task.depends_on.join(", ")}</Text>
        )}
        {dur && <Text style={{ fontSize: 11, color: "var(--muted)" }}>⏱ {dur}</Text>}
      </Space>
    </Card>
  );
}

// ---- PipelineView ----

function PipelineView({
  pipeline,
  projectId,
  onStart,
  onPause,
  onCancel,
  onDeleted,
  isStarting,
  isPausing,
  onRefresh,
}: {
  pipeline: PipelineRow;
  projectId: string;
  onStart: () => void;
  onPause: () => void;
  onCancel: () => void;
  onDeleted: () => void;
  isStarting: boolean;
  isPausing: boolean;
  onRefresh: () => void;
}) {
  const qc = useQueryClient();
  const st = PIPELINE_STATUS_TAG[pipeline.status] ?? { color: "default", label: pipeline.status };
  const tasks = pipeline.tasks ?? [];
  const total = tasks.length;
  const done = tasks.filter((t) => t.status === "done").length;
  const active = tasks.filter((t) => t.status === "active").length;
  const errors = tasks.filter((t) => t.status === "error" || t.status === "timeout").length;
  const isRunning = pipeline.status === "active" || pipeline.status === "queued";
  const canDelete = pipeline.status === "done" || pipeline.status === "failed" || pipeline.status === "paused";

  const deleteM = useMutation({
    mutationFn: () => api.deletePipeline(pipeline.id, projectId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["project-pipelines", projectId] });
      onDeleted();
    },
  });

  const confirmDelete = () => {
    Modal.confirm({
      title: "Xoá Pipeline?",
      content: "Thao tác này sẽ xoá toàn bộ dữ liệu liên quan: scenes, videos, characters của project này. Không thể hoàn tác.",
      okText: "Xoá",
      okType: "danger",
      cancelText: "Huỷ",
      onOk: () => deleteM.mutate(),
    });
  };

  const confirmStop = () => {
    Modal.confirm({
      title: "Dừng Pipeline?",
      content: "Pipeline sẽ bị đánh dấu là thất bại. Có thể xoá sau để tạo mới.",
      okText: "Dừng",
      okType: "danger",
      cancelText: "Huỷ",
      onOk: onCancel,
    });
  };

  return (
    <Card
      title={
        <Space>
          <Text strong>Pipeline</Text>
          <Tag color={st.color}>{st.label}</Tag>
          <Text type="secondary" style={{ fontSize: 12 }}>{pipeline.id.slice(0, 8)}…</Text>
        </Space>
      }
      extra={
        <Space>
          {pipeline.status === "queued" && (
            <Button type="primary" icon={<PlayCircleOutlined />} size="small" loading={isStarting} onClick={onStart}>
              Bắt đầu
            </Button>
          )}
          {pipeline.status === "active" && (
            <Button icon={<PauseCircleOutlined />} size="small" loading={isPausing} onClick={onPause}>
              Tạm dừng
            </Button>
          )}
          {pipeline.status === "paused" && (
            <Button type="primary" icon={<PlayCircleOutlined />} size="small" loading={isStarting} onClick={onStart}>
              Tiếp tục
            </Button>
          )}
          {isRunning && (
            <Button danger icon={<StopOutlined />} size="small" onClick={confirmStop}>
              Dừng
            </Button>
          )}
          {canDelete && (
            <Button
              danger
              icon={<DeleteOutlined />}
              size="small"
              loading={deleteM.isPending}
              onClick={confirmDelete}
            >
              Xoá Pipeline
            </Button>
          )}
        </Space>
      }
      style={{ marginBottom: 16 }}
    >
      <Descriptions size="small" column={4} style={{ marginBottom: 12 }}>
        <Descriptions.Item label="Orientation">{pipeline.orientation}</Descriptions.Item>
        <Descriptions.Item label="Xong">{done}/{total}</Descriptions.Item>
        <Descriptions.Item label="Chạy">{active}</Descriptions.Item>
        <Descriptions.Item label="Lỗi">
          {errors > 0 ? <Text type="danger">{errors}</Text> : 0}
        </Descriptions.Item>
      </Descriptions>

      {total > 0 ? (
        <>
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fill, minmax(210px, 1fr))",
              gap: 10,
              marginBottom: 16,
            }}
          >
            {tasks.map((task) => (
              <TaskCard key={task.id} task={task} pipelineId={pipeline.id} onRetried={onRefresh} />
            ))}
          </div>
          <Card
            size="small"
            title="Danh sách task"
            styles={{ body: { padding: "8px 10px" } }}
          >
            <List
              size="small"
              dataSource={tasks}
              renderItem={(t, idx) => {
                const itemStatus = STATUS_TAG[t.status] ?? STATUS_TAG.registered;
                return (
                  <List.Item style={{ padding: "8px 4px" }}>
                    <Space style={{ width: "100%", justifyContent: "space-between" }}>
                      <Space size={8}>
                        <Text type="secondary" style={{ width: 18, fontSize: 12 }}>
                          {idx + 1}.
                        </Text>
                        <Text style={{ fontSize: 12 }}>{t.label}</Text>
                      </Space>
                      <Tag color={itemStatus.color} icon={itemStatus.icon} style={{ margin: 0 }}>
                        {itemStatus.label}
                      </Tag>
                    </Space>
                  </List.Item>
                );
              }}
            />
          </Card>
        </>
      ) : (
        <Text type="secondary">Chưa có tasks</Text>
      )}

      {/* Show create-new only after pipeline is done/failed */}
      {canDelete && (
        <div style={{ marginTop: 16, textAlign: "right" }}>
          <Text type="secondary" style={{ fontSize: 12 }}>
            Xoá pipeline để tạo pipeline mới cho project này.
          </Text>
        </div>
      )}
    </Card>
  );
}

// ---- WebSocket hook ----

function useDashboardWS(onEvent: (e: DashEvent) => void) {
  const cbRef = useRef(onEvent);
  cbRef.current = onEvent;
  useEffect(() => {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    const url = `${proto}//${window.location.host}/ws/dashboard`;
    let ws: WebSocket;
    let closed = false;
    function connect() {
      ws = new WebSocket(url);
      ws.onmessage = (e) => {
        try { cbRef.current(JSON.parse(e.data) as DashEvent); } catch { /* ignore */ }
      };
      ws.onclose = () => { if (!closed) setTimeout(connect, 3000); };
    }
    connect();
    return () => { closed = true; ws?.close(); };
  }, []);
}

// ---- Main page ----

type Props = { initialProjectId?: string };

export function DagPipelinePage({ initialProjectId }: Props) {
  const qc = useQueryClient();

  const [engine, setEngine] = useState<"workflow" | "dag">("workflow");
  const [workflowRunning, setWorkflowRunning] = useState(false);
  const [projectId, setProjectId] = useState(initialProjectId ?? "");
  const [script, setScript] = useState("");
  const [orientation, setOrientation] = useState<"VERTICAL" | "HORIZONTAL">("VERTICAL");
  const [pipelineMode, setPipelineMode] = useState<"production" | "full">("production");
  const [pipelineId, setPipelineId] = useState<string | null>(null);
  const [parseResult, setParseResult] = useState<string | null>(null);
  const [showPreview, setShowPreview] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const llmSettingsQ = useQuery({ queryKey: ["llm-settings"], queryFn: () => api.getLLMSettings(), staleTime: 60_000 });
  const provider = llmSettingsQ.data?.provider ?? "gemini";

  useEffect(() => { if (initialProjectId) setProjectId(initialProjectId); }, [initialProjectId]);

  const projectsQ = useQuery({
    queryKey: ["projects"],
    queryFn: () => api.listProjects(),
    staleTime: 30_000,
  });

  // Load all pipelines for the selected project
  const projectPipelinesQ = useQuery({
    queryKey: ["project-pipelines", projectId],
    queryFn: () => api.listProjectPipelines(projectId),
    enabled: !!projectId && engine === "dag",
    staleTime: 5_000,
  });

  // Auto-select the most recent active pipeline when project changes
  useEffect(() => {
    if (!projectPipelinesQ.data) return;
    const pipelines = projectPipelinesQ.data;
    // Find the first non-failed pipeline (most recent, since sorted DESC)
    const active = pipelines.find((p) => p.status !== "failed");
    if (active) {
      setPipelineId(active.id);
    } else if (pipelines.length > 0) {
      // All are failed — show the most recent so user can delete
      setPipelineId(pipelines[0].id);
    } else {
      setPipelineId(null);
    }
  }, [projectPipelinesQ.data]);

  const pipelineQ = useQuery({
    queryKey: ["pipeline", pipelineId],
    queryFn: () => api.getPipeline(pipelineId!),
    enabled: !!pipelineId && engine === "dag",
    refetchInterval: (query) => {
      const st = query.state.data?.status;
      return st === "active" || st === "queued" ? 3_000 : false;
    },
  });

  const handleWsEvent = useCallback(
    (e: DashEvent) => {
      if (e.event === "pipeline:updated" || e.event === "agent:state") {
        void qc.invalidateQueries({ queryKey: ["pipeline", pipelineId] });
      }
    },
    [qc, pipelineId]
  );
  useDashboardWS(handleWsEvent);

  const parseM = useMutation({
    mutationFn: () => api.parseScript({ script, provider }),
    onSuccess: (data) => {
      const summary =
        `✓ ${data.scenes.length} cảnh · ${data.characters.length} nhân vật\n` +
        data.scenes.map((s) => `Cảnh ${s.display_order}: ${s.prompt.slice(0, 80)}${s.prompt.length > 80 ? "…" : ""}`).join("\n");
      setParseResult(summary);
      setShowPreview(true);
      setErr(null);
    },
    onError: (e: Error) => setErr(e.message),
  });

  const createM = useMutation({
    mutationFn: () => api.createPipeline({ project_id: projectId, script, orientation, pipeline_mode: pipelineMode }),
    onSuccess: (data) => {
      setPipelineId(data.id);
      setErr(null);
      void qc.invalidateQueries({ queryKey: ["pipeline", data.id] });
      void qc.invalidateQueries({ queryKey: ["project-pipelines", projectId] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const startM = useMutation({
    mutationFn: () => api.startPipeline(pipelineId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["pipeline", pipelineId] }),
    onError: (e: Error) => setErr(e.message),
  });

  const pauseM = useMutation({
    mutationFn: () => api.pausePipeline(pipelineId!),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["pipeline", pipelineId] }),
    onError: (e: Error) => setErr(e.message),
  });

  const cancelM = useMutation({
    mutationFn: () => api.cancelPipeline(pipelineId!),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["pipeline", pipelineId] });
      void qc.invalidateQueries({ queryKey: ["project-pipelines", projectId] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const handleDeleted = () => {
    setPipelineId(null);
    void qc.invalidateQueries({ queryKey: ["project-pipelines", projectId] });
  };

  const projects = (projectsQ.data ?? []) as ProjectRow[];
  const projectOptions = projects.map((p) => ({ label: String(p.name ?? p.id), value: String(p.id) }));

  const pipeline = pipelineQ.data;
  // Show form when: project selected and no pipeline exists (or all pipelines failed and user deleted)
  const hasActivePipeline = !!pipelineId;
  const isDag = engine === "dag";
  const showForm = isDag && !!projectId && !hasActivePipeline;
  const showPipeline = isDag && !!pipelineId;

  return (
    <div style={{ maxWidth: 960, margin: "0 auto", padding: "24px 16px 48px" }}>
      <div style={{ marginBottom: 20 }}>
        <Title level={3} style={{ margin: 0 }}>Smart Pipeline</Title>
        <Text type="secondary">
          {engine === "workflow"
            ? "Workflow engine — mỗi cảnh một node, các cảnh dựng ảnh & video song song"
            : "Multi-Agent DAG — OrchestratorAgent phân rã kịch bản thành DAG tasks và chạy tự động"}
        </Text>
      </div>

      <Segmented
        value={engine}
        onChange={(v) => setEngine(v as "workflow" | "dag")}
        options={[
          { label: "Workflow (song song)", value: "workflow" },
          { label: "DAG cũ", value: "dag" },
        ]}
        style={{ marginBottom: 16 }}
      />

      {err && (
        <Alert type="error" message={err} closable onClose={() => setErr(null)} style={{ marginBottom: 16 }} />
      )}

      {/* Project selector */}
      <Card style={{ marginBottom: 16 }}>
        <Form layout="inline" size="middle">
          <Form.Item label="Project" required style={{ marginBottom: 0, flex: 1 }}>
            <Space.Compact style={{ width: "100%" }}>
              <Select
                placeholder="Chọn project để xem hoặc tạo pipeline"
                options={projectOptions}
                value={projectId || undefined}
                onChange={(v) => {
                  setProjectId(v);
                  setPipelineId(null); // reset; useEffect will auto-load
                }}
                loading={projectsQ.isLoading}
                showSearch
                style={{ minWidth: 280, width: "100%" }}
                filterOption={(inp, opt) =>
                  (opt?.label ?? "").toLowerCase().includes(inp.toLowerCase())
                }
              />
              <Button
                icon={<ReloadOutlined />}
                loading={projectsQ.isFetching}
                onClick={() => void projectsQ.refetch()}
              >
                Reload
              </Button>
            </Space.Compact>
          </Form.Item>
        </Form>
      </Card>

      {/* ---- Workflow engine mode ---- */}
      {engine === "workflow" && projectId && (
        <>
          <CustomWorkflowBuilder projectId={projectId} orientation={orientation} />
          <WorkflowPipelinePanel projectId={projectId} onRunningChange={setWorkflowRunning} />
        </>
      )}

      {/* ---- Legacy DAG engine mode ---- */}
      {engine === "dag" && projectId && (projectPipelinesQ.data?.length ?? 0) > 0 && (
        <Card title="Pipelines của project" size="small" style={{ marginBottom: 16 }}>
          <List
            size="small"
            dataSource={projectPipelinesQ.data ?? []}
            renderItem={(p) => {
              const st = PIPELINE_STATUS_TAG[p.status] ?? { color: "default", label: p.status };
              const isCurrent = p.id === pipelineId;
              return (
                <List.Item
                  style={{
                    cursor: "pointer",
                    background: isCurrent ? "var(--surface-raised, #f0f0f0)" : undefined,
                    borderRadius: 6,
                    padding: "6px 8px",
                  }}
                  onClick={() => setPipelineId(p.id)}
                  actions={[<Tag key="s" color={st.color}>{st.label}</Tag>]}
                >
                  <Space>
                    <Text style={{ fontSize: 12, fontFamily: "var(--mono)" }}>{p.id.slice(0, 8)}…</Text>
                    <Text type="secondary" style={{ fontSize: 11 }}>{p.orientation}</Text>
                    <Text type="secondary" style={{ fontSize: 11 }}>
                      {new Date(p.created_at).toLocaleString()}
                    </Text>
                    {isCurrent && <Badge status="processing" text="đang xem" />}
                  </Space>
                </List.Item>
              );
            }}
          />
        </Card>
      )}

      {/* Create form — only shown when no pipeline for this project */}
      {showForm && (
        <Card style={{ marginBottom: 16 }}>
          <Form layout="vertical" size="middle">
            <Row gutter={[16, 0]}>
              <Col xs={12} sm={6}>
                <Form.Item label="Orientation">
                  <Select
                    value={orientation}
                    onChange={setOrientation}
                    options={[
                      { label: "Dọc (9:16)", value: "VERTICAL" },
                      { label: "Ngang (16:9)", value: "HORIZONTAL" },
                    ]}
                  />
                </Form.Item>
              </Col>
              <Col xs={24} sm={18}>
                <Form.Item
                  label="Pipeline Mode"
                  tooltip="Production: nhập screenplay sẵn. Full: nhập concept/ý tưởng, agent tự viết kịch bản."
                >
                  <Select
                    value={pipelineMode}
                    onChange={setPipelineMode}
                    options={[
                      { label: "🎬 Production — nhập screenplay", value: "production" },
                      { label: "✨ Full — từ concept (Director → Screenwriter → ...)", value: "full" },
                    ]}
                  />
                </Form.Item>
              </Col>
            </Row>

            <Form.Item
              label={pipelineMode === "full" ? "Concept / Ý tưởng" : "Kịch bản (Markdown screenplay)"}
              required
              extra={
                projectId && (
                  <Button
                    type="link"
                    size="small"
                    style={{ padding: 0, height: "auto" }}
                    onClick={() => {
                      const proj = projects.find((p) => String(p.id) === projectId);
                      const storyText = String(proj?.story ?? "").trim();
                      if (storyText) setScript(storyText);
                    }}
                  >
                    Dùng Story từ Project →
                  </Button>
                )
              }
            >
              <TextArea
                rows={8}
                placeholder={
                  pipelineMode === "full"
                    ? "Nhập concept / logline / tóm tắt cốt truyện..."
                    : "# Cảnh 1\nNam đứng trên cánh đồng lúa vàng rực.\n\n# Cảnh 2\nHoa ngồi bên bờ sông."
                }
                value={script}
                onChange={(e) => setScript(e.target.value)}
                style={{ fontFamily: "var(--mono)", fontSize: 13 }}
              />
            </Form.Item>

            <Space wrap>
              <Tooltip title="Parse kịch bản để xem trước scenes và characters">
                <Button
                  icon={<FileTextOutlined />}
                  onClick={() => parseM.mutate()}
                  loading={parseM.isPending}
                  disabled={!script.trim()}
                >
                  Preview kịch bản
                </Button>
              </Tooltip>
              <Button
                type="primary"
                icon={<BranchesOutlined />}
                onClick={() => createM.mutate()}
                loading={createM.isPending}
                disabled={!script.trim() || !projectId}
              >
                Tạo Pipeline
              </Button>
            </Space>
          </Form>

          {showPreview && parseResult && (
            <Card
              title="Kết quả parse"
              extra={<Button size="small" onClick={() => setShowPreview(false)}>Đóng</Button>}
              style={{ marginTop: 16 }}
            >
              <Paragraph>
                <pre style={{ fontFamily: "var(--mono)", fontSize: 12, margin: 0, whiteSpace: "pre-wrap" }}>
                  {parseResult}
                </pre>
              </Paragraph>
            </Card>
          )}
        </Card>
      )}

      {/* No project selected hint */}
      {isDag && !projectId && (
        <Card style={{ textAlign: "center", padding: 32 }}>
          <Text type="secondary">Chọn project để bắt đầu</Text>
        </Card>
      )}

      {/* Loading state */}
      {showPipeline && pipelineQ.isLoading && (
        <div style={{ textAlign: "center", padding: 40 }}>
          <Spin size="large" />
        </div>
      )}

      {/* Pipeline run view */}
      {showPipeline && pipeline && (
        <PipelineView
          pipeline={pipeline}
          projectId={projectId}
          onStart={() => startM.mutate()}
          onPause={() => pauseM.mutate()}
          onCancel={() => cancelM.mutate()}
          onDeleted={handleDeleted}
          isStarting={startM.isPending}
          isPausing={pauseM.isPending}
          onRefresh={() => void qc.invalidateQueries({ queryKey: ["pipeline", pipelineId] })}
        />
      )}

      {/* Studio preview — entities & scenes written by the pipeline */}
      {projectId && (
        <StudioSection
          projectId={projectId}
          orientation={pipeline?.orientation ?? orientation}
          isActive={
            engine === "workflow"
              ? workflowRunning
              : pipeline?.status === "active" || pipeline?.status === "queued"
          }
        />
      )}

      {/* Agents list — only when no pipeline loaded */}
      {isDag && !showPipeline && <AgentsPanel />}
    </div>
  );
}

const STUDIO_STATUS_COLOR: Record<string, string> = {
  PENDING: "default",
  PROCESSING: "processing",
  COMPLETED: "success",
  FAILED: "error",
};

function StudioSection({
  projectId,
  orientation,
  isActive,
}: {
  projectId: string;
  orientation: string;
  isActive: boolean;
}) {
  const interval = isActive ? 5000 : false;
  const qc = useQueryClient();
  const [preview, setPreview] = useState<{ url: string; type: "image" | "video"; title: string } | null>(null);

  // A clip can be COMPLETED with no URL: Flow stopped returning one from the
  // generation API. This scrapes the Flow project page via the extension and
  // pulls the assets local.
  const fetchUrlsM = useMutation({
    mutationFn: () => api.fetchMediaUrls(projectId),
    onSuccess: (r) => {
      message[r.downloaded > 0 ? "success" : "warning"](
        r.downloaded > 0
          ? `Đã lấy và tải về ${r.downloaded} file.`
          : "Chưa lấy được link — mở Google Flow trong extension rồi thử lại."
      );
      void qc.invalidateQueries({ queryKey: ["studio-scenes"] });
      void qc.invalidateQueries({ queryKey: ["studio-chars", projectId] });
    },
    onError: (e: Error) => message.error(e.message),
  });

  const charsQ = useQuery({
    queryKey: ["studio-chars", projectId],
    queryFn: () => api.listProjectCharacters(projectId),
    enabled: !!projectId,
    refetchInterval: interval,
  });

  const videosQ = useQuery({
    queryKey: ["studio-videos", projectId],
    queryFn: () => api.listVideos(projectId),
    enabled: !!projectId,
    refetchInterval: interval,
  });

  const videoId = str((videosQ.data ?? [])[0]?.id ?? "");

  const scenesQ = useQuery({
    queryKey: ["studio-scenes", videoId],
    queryFn: () => api.listScenes(videoId),
    enabled: !!videoId,
    refetchInterval: interval,
  });

  const chars = (charsQ.data ?? []) as CharacterRow[];
  const scenes = (scenesQ.data ?? []) as SceneRow[];
  const isH = orientation.toUpperCase() === "HORIZONTAL";

  if (!chars.length && !scenes.length) return null;

  return (
    <div style={{ marginTop: 24 }}>
      {chars.length > 0 && (
        <Card
          title={
            <Space>
              <UserOutlined />
              <span>Entities ({chars.length})</span>
            </Space>
          }
          size="small"
          style={{ marginBottom: 16 }}
        >
          <div style={{ display: "flex", flexWrap: "wrap", gap: 10 }}>
            {chars.map((c) => {
              const imgUrl = str(c.reference_image_url);
              return (
                <div
                  key={str(c.id)}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    padding: "6px 10px",
                    background: "var(--surface-raised, #f5f5f5)",
                    borderRadius: 8,
                    minWidth: 140,
                  }}
                >
                  {imgUrl ? (
                    <img
                      src={imgUrl}
                      alt={str(c.name)}
                      onClick={() => setPreview({ url: imgUrl, type: "image", title: str(c.name) })}
                      style={{ width: 40, height: 40, objectFit: "cover", borderRadius: "50%", border: "1px solid var(--border)", cursor: "zoom-in" }}
                    />
                  ) : (
                    <div
                      style={{
                        width: 40, height: 40, borderRadius: "50%",
                        background: "var(--surface, #e8e8e8)",
                        display: "flex", alignItems: "center", justifyContent: "center",
                      }}
                    >
                      <UserOutlined style={{ color: "var(--muted)" }} />
                    </div>
                  )}
                  <div>
                    <Text strong style={{ fontSize: 12 }}>{str(c.name)}</Text>
                    <br />
                    <Text type="secondary" style={{ fontSize: 11 }}>{str(c.entity_type) || "character"}</Text>
                  </div>
                </div>
              );
            })}
          </div>
        </Card>
      )}

      {scenes.length > 0 && (
        <Card
          title={
            <Space>
              <VideoCameraOutlined />
              <span>Scenes ({scenes.length})</span>
            </Space>
          }
          size="small"
        >
          <div
            style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fill, minmax(170px, 1fr))",
              gap: 10,
            }}
          >
            {scenes.map((sc, idx) => {
              const imgUrl = str(isH ? sc.horizontal_image_url : sc.vertical_image_url);
              const videoUrl = str(isH ? sc.horizontal_video_url : sc.vertical_video_url);
              const imgStatus = str(isH ? sc.horizontal_image_status : sc.vertical_image_status) || "PENDING";
              const videoStatus = str(isH ? sc.horizontal_video_status : sc.vertical_video_status) || "PENDING";
              const prompt = str(sc.prompt);
              return (
                <Card key={str(sc.id)} size="small" styles={{ body: { padding: 8 } }}>
                  {imgUrl ? (
                    <img
                      src={imgUrl}
                      alt={`Scene ${idx + 1}`}
                      onClick={() => setPreview({ url: imgUrl, type: "image", title: `Cảnh ${idx + 1}` })}
                      style={{ width: "100%", height: 96, objectFit: "cover", borderRadius: 4, display: "block", marginBottom: 6, cursor: "zoom-in" }}
                    />
                  ) : (
                    <div
                      style={{
                        width: "100%", height: 96,
                        background: "var(--surface-raised, #f0f0f0)",
                        borderRadius: 4, display: "flex", alignItems: "center", justifyContent: "center", marginBottom: 6,
                      }}
                    >
                      <Tag color={STUDIO_STATUS_COLOR[imgStatus] ?? "default"} style={{ margin: 0 }}>{imgStatus}</Tag>
                    </div>
                  )}
                  <Space size={4} style={{ marginBottom: 4 }}>
                    <Tag color={STUDIO_STATUS_COLOR[imgStatus] ?? "default"} style={{ margin: 0, fontSize: 10 }}>Ảnh</Tag>
                    {videoUrl ? (
                      <Tag
                        color="success"
                        style={{ margin: 0, fontSize: 10, cursor: "pointer" }}
                        onClick={() => setPreview({ url: videoUrl, type: "video", title: `Cảnh ${idx + 1}` })}
                      >
                        Video ▶
                      </Tag>
                    ) : videoStatus === "COMPLETED" ? (
                      <Tag
                        color="warning"
                        style={{ margin: 0, fontSize: 10, cursor: "pointer" }}
                        onClick={() => fetchUrlsM.mutate()}
                      >
                        {fetchUrlsM.isPending ? "Đang lấy…" : "Lấy link"}
                      </Tag>
                    ) : (
                      <Tag color={STUDIO_STATUS_COLOR[videoStatus] ?? "default"} style={{ margin: 0, fontSize: 10 }}>Video</Tag>
                    )}
                  </Space>
                  <Text style={{ fontSize: 11, display: "block" }}>
                    <Text type="secondary" style={{ fontSize: 11 }}>#{idx + 1} </Text>
                    {prompt.slice(0, 60)}{prompt.length > 60 ? "…" : ""}
                  </Text>
                </Card>
              );
            })}
          </div>
        </Card>
      )}

      {/* Plays whatever URL the scene has — a local /api/media file or a remote
          Flow URL — so nothing has to be downloaded before it can be watched. */}
      <Modal
        open={!!preview}
        onCancel={() => setPreview(null)}
        footer={null}
        centered
        width={preview?.type === "video" ? 720 : 620}
        title={preview?.title ?? ""}
        styles={{ body: { padding: 8, textAlign: "center" } }}
      >
        {preview?.type === "image" && (
          <img src={preview.url} alt={preview.title} style={{ maxWidth: "100%", maxHeight: "78vh", borderRadius: 6 }} />
        )}
        {preview?.type === "video" && (
          <>
            <video src={preview.url} controls autoPlay style={{ maxWidth: "100%", maxHeight: "72vh", borderRadius: 6 }} />
            <div style={{ marginTop: 8, textAlign: "left" }}>
              <Typography.Text type="secondary" style={{ fontSize: 11 }}>
                {preview.url.startsWith("/api/media/")
                  ? "Đang phát từ file đã tải về máy."
                  : "Đang phát trực tiếp từ Google Flow — link này sẽ hết hạn, bấm “Lấy link” để tải về máy."}
              </Typography.Text>
            </div>
          </>
        )}
      </Modal>
    </div>
  );
}

function AgentsPanel() {
  const q = useQuery({ queryKey: ["agents"], queryFn: () => api.listAgents(), staleTime: 120_000 });
  if (!q.data?.length) return null;
  return (
    <Card title="Available Agents" size="small" style={{ marginTop: 24 }}>
      <Row gutter={[10, 10]}>
        {q.data.filter((a) => a.enabled !== false).map((agent) => (
          <Col key={agent.type} xs={24} sm={12} md={8}>
            <Card size="small" styles={{ body: { padding: "10px 12px" } }}>
              <Space>
                <Text style={{ fontSize: 16 }}>{AGENT_ICON[agent.type] ?? <RobotOutlined />}</Text>
                <div>
                  <Text strong style={{ fontSize: 12 }}>{agent.type}</Text>
                  <br />
                  <Text type="secondary" style={{ fontSize: 11 }}>{agent.soul_summary?.slice(0, 60) ?? ""}</Text>
                </div>
              </Space>
            </Card>
          </Col>
        ))}
      </Row>
    </Card>
  );
}
