import { useEffect, useMemo, useState } from "react";
import { Button, Card, Form, Input, Modal, Space, Typography, message } from "antd";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  ReactFlow,
  Background,
  Controls,
  Handle,
  MarkerType,
  Position,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { api } from "../api";
import type { Flow, FlowAction } from "../types/api";
import { StepConfigFields } from "./flows/StepConfigFields";
import { StepParamsEditor } from "./flows/StepParamsEditor";
import { getBranchPortsByType, ui, type BranchPortRule } from "./flows/constants";
import { branchSummary } from "./flows/branchUtils";
import { getActionStage, normalizeFlowStages } from "./flows/flowStage";

type FlowNodeData = {
  title: string;
  subtitle: string;
  ports: BranchPortRule[];
  hasInput: boolean;
};

function FlowStepNode(props: NodeProps) {
  const d = props.data as FlowNodeData;
  const selected = props.selected;
  const n = d.ports.length;
  const pos = (idx: number) => `${((idx + 1) * 100) / (n + 1)}%`;
  return (
    <div
      style={{
        minWidth: 220,
        borderRadius: 10,
        border: selected ? "1px solid #1677ff" : "1px solid var(--flow-chain-card-border)",
        background: "var(--flow-step-card-bg)",
        color: "var(--text)",
        boxShadow: "0 2px 12px rgba(0, 0, 0, 0.18)",
        textAlign: "center",
        padding: "10px 12px",
        fontWeight: 700,
      }}
    >
      {d.hasInput ? <Handle type="target" position={Position.Top} id="in" /> : null}
      {d.ports.map((p, idx) => (
        <Handle
          key={p.id}
          type="source"
          position={Position.Bottom}
          id={p.id}
          style={{ left: pos(idx), background: p.color }}
        />
      ))}
      <div>{d.title}</div>
      <div style={{ fontWeight: 600, marginTop: 4, color: "var(--muted-text)" }}>{d.subtitle}</div>
    </div>
  );
}

function createStartStep(): FlowAction {
  return {
    id: "step_start",
    type: "start",
    name: "Start",
    timeoutSeconds: 0,
    config: { _stage: "1" },
  };
}

function isStartStep(step: FlowAction): boolean {
  return step.type === "start";
}

const maxParallelPerStage = 5;

function prepareEditorLikeSteps(flow: Flow): FlowAction[] {
  const raw = (flow.actions ?? []).map((a) => ({ ...a, config: { ...(a.config ?? {}) } }));
  const withStart = raw.some(isStartStep) ? raw : [createStartStep(), ...raw];
  const prepared = withStart.map((a) =>
    isStartStep(a) ? { ...a, id: "step_start", config: { ...(a.config ?? {}), _stage: "1" } } : a
  );
  return normalizeFlowStages(prepared, maxParallelPerStage);
}

export function FlowDetailPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const [flowId, setFlowId] = useState<string | null>(null);
  const [flowName, setFlowName] = useState("");
  const [steps, setSteps] = useState<FlowAction[]>([]);
  const [selectedStepId, setSelectedStepId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!id) return;
    void (async () => {
      try {
        setError(null);
        const list = await api<Flow[] | null>("/api/flows");
        const f = (Array.isArray(list) ? list : []).find((x) => x.id === id);
        if (!f) {
          setError("Không tìm thấy flow");
          setFlowId(null);
          setSteps([]);
          return;
        }
        setFlowId(f.id);
        setFlowName(f.name);
        setSteps(prepareEditorLikeSteps(f));
        setSelectedStepId(null);
      } catch (e) {
        setError(String(e));
        setFlowId(null);
        setSteps([]);
      }
    })();
  }, [id]);

  const selectedStep = useMemo(() => steps.find((s) => s.id === selectedStepId) ?? null, [steps, selectedStepId]);

  const branchPortsByStep = useMemo(() => {
    const m = new Map<string, BranchPortRule[]>();
    steps.forEach((s) => m.set(s.id, getBranchPortsByType(s.type)));
    return m;
  }, [steps]);

  const stageFirstStep = useMemo(() => {
    const map = new Map<number, string>();
    steps.forEach((s, idx) => {
      const st = getActionStage(s, idx + 1);
      if (!map.has(st)) map.set(st, s.id);
    });
    return map;
  }, [steps]);

  const flowNodes = useMemo<Node<FlowNodeData>[]>(
    () =>
      steps.map((s, idx) => {
        const fallbackStage = getActionStage(s, idx + 1);
        const sx = Number(s.config?._x);
        const sy = Number(s.config?._y);
        const x = Number.isFinite(sx) ? sx : 80 + (fallbackStage - 1) * 280;
        const y = Number.isFinite(sy) ? sy : 60 + (idx % maxParallelPerStage) * 130;
        return {
          id: s.id,
          type: "flowStep",
          data: {
            title: `${idx + 1}. ${s.name}`,
            subtitle: s.type,
            ports: branchPortsByStep.get(s.id) ?? getBranchPortsByType(s.type),
            hasInput: !isStartStep(s),
          },
          position: { x, y },
        };
      }),
    [steps, branchPortsByStep]
  );

  const flowEdges = useMemo<Edge[]>(() => {
    const out: Edge[] = [];
    steps.forEach((s) => {
      const ports = branchPortsByStep.get(s.id) ?? getBranchPortsByType(s.type);
      ports.forEach((p) => {
        const targetByStep = s.config?.[`${p.configKey}_step_id`];
        let targetStepId = targetByStep;
        if (!targetStepId) {
          const targetStageRaw = s.config?.[p.configKey];
          const targetStage = Number(targetStageRaw);
          if (Number.isFinite(targetStage) && targetStage > 0) {
            targetStepId = stageFirstStep.get(targetStage);
          }
        }
        if (!targetStepId) return;
        out.push({
          id: `branch_${s.id}_${targetStepId}_${p.id}`,
          source: s.id,
          sourceHandle: p.id,
          target: targetStepId,
          targetHandle: "in",
          label: p.label,
          type: "bezier",
          animated: false,
          style: { stroke: p.color },
        });
      });
    });
    return out;
  }, [steps, stageFirstStep, branchPortsByStep]);

  const saveFlow = async () => {
    if (!flowId || !flowName.trim()) {
      message.error("Thiếu tên flow");
      return;
    }
    try {
      setSaving(true);
      setError(null);
      await api<Flow>("/api/flows", "POST", {
        id: flowId,
        name: flowName.trim(),
        actions: steps.some(isStartStep) ? steps : [createStartStep(), ...steps],
      });
      message.success("Đã lưu flow");
    } catch (e) {
      setError(String(e));
      message.error(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (!id) {
    return (
      <Card>
        <Typography.Text type="danger">Thiếu ID flow.</Typography.Text>
      </Card>
    );
  }

  if (error && !flowId) {
    return (
      <Card title="Flow" extra={<Button onClick={() => navigate("/flows")}>Về danh sách</Button>}>
        <pre className="error">{error}</pre>
      </Card>
    );
  }

  if (!flowId) {
    return (
      <Card>
        <Typography.Text type="secondary">Đang tải…</Typography.Text>
      </Card>
    );
  }

  const noop = () => {};

  return (
    <div className="page" style={ui.page}>
      {error && flowId ? <pre className="error">{error}</pre> : null}

      <Card
        title="Xem flow"
        extra={
          <Space wrap>
            <Link to={`/flows/${encodeURIComponent(flowId)}/edit`}>
              <Button>Sửa flow</Button>
            </Link>
            <Link to="/flows/actions">
              <Button type="link">Danh sách actions</Button>
            </Link>
            <Button onClick={() => navigate("/flows")}>Về danh sách</Button>
          </Space>
        }
      >
        <div style={ui.editorHeader}>
          <div style={{ flex: 1, minWidth: 320 }}>
            <div style={{ fontWeight: 700, marginBottom: 6 }}>Flow name</div>
            <Input value={flowName} onChange={(e) => setFlowName(e.target.value)} placeholder="Tên flow" />
            <Typography.Text type="secondary" style={{ fontSize: 12, display: "block", marginTop: 4 }}>
              ID: {flowId} · canvas và từng bước chỉ xem — có thể đổi tên rồi lưu
            </Typography.Text>
          </div>
          <Space wrap>
            <Button type="primary" loading={saving} onClick={() => void saveFlow()} disabled={!flowName.trim()}>
              Lưu flow
            </Button>
          </Space>
        </div>

        <div style={ui.editorBody}>
          <div style={ui.leftPanel}>
            <div style={ui.leftPanelHeader}>
              <div style={{ fontWeight: 700, marginBottom: 8 }}>Gợi ý</div>
              <Typography.Paragraph type="secondary" style={{ fontSize: 13, marginBottom: 0 }}>
                Giao diện giống màn sửa flow: canvas React Flow chỉ xem (không kéo thả, không nối dây). Click một step để xem cấu hình
                (read-only). Để chỉnh sửa bước hoặc nhánh, dùng nút <b>Sửa flow</b>.
              </Typography.Paragraph>
            </div>
          </div>

          <div style={ui.canvasWrap}>
            <div style={ui.canvasTopBar}>
              <Typography.Text strong>Canvas</Typography.Text>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                Chỉ xem · click step để mở chi tiết (không chỉnh sửa)
              </Typography.Text>
            </div>
            <div
              style={{
                border: "1px dashed var(--flow-panel-border)",
                borderRadius: 10,
                height: 560,
                overflow: "hidden",
                background: "var(--flow-canvas-bg)",
              }}
            >
              <ReactFlow
                nodes={flowNodes}
                edges={flowEdges}
                nodeTypes={{ flowStep: FlowStepNode }}
                fitView
                nodesDraggable={false}
                nodesConnectable={false}
                elementsSelectable
                panOnScroll
                zoomOnScroll
                deleteKeyCode={null}
                defaultEdgeOptions={{ markerEnd: { type: MarkerType.ArrowClosed } }}
                onNodeClick={(_, node) => setSelectedStepId(node.id)}
                onPaneClick={() => setSelectedStepId(null)}
              >
                <Background />
                <Controls />
              </ReactFlow>
            </div>
          </div>
        </div>
      </Card>

      <Modal
        title={selectedStep ? `Step (chỉ xem): ${selectedStep.name}` : "Step"}
        open={!!selectedStep}
        onCancel={() => setSelectedStepId(null)}
        footer={null}
        destroyOnClose
        width={selectedStep?.type === "playwright_atomics" ? 900 : 520}
        style={selectedStep?.type === "playwright_atomics" ? { maxWidth: "calc(100vw - 24px)" } : undefined}
      >
        {selectedStep ? (
          <>
            <Typography.Paragraph type="secondary" style={{ marginBottom: 12, fontSize: 12 }}>
              Cấu hình step chỉ đọc. Đổi tên flow hoặc chỉnh bước tại màn <Link to={`/flows/${encodeURIComponent(flowId)}/edit`}>Sửa flow</Link>.
            </Typography.Paragraph>
            <Form layout="vertical">
              <Form.Item label="Name">
                <Input value={selectedStep.name} disabled />
              </Form.Item>
              <Form.Item label="Timeout (seconds)">
                <Input type="number" value={selectedStep.timeoutSeconds} disabled />
              </Form.Item>
            </Form>
            <div
              style={{
                marginBottom: 10,
                border: "1px solid var(--flow-panel-border)",
                borderRadius: 10,
                background: "var(--flow-panel-bg)",
                padding: "10px 12px",
              }}
            >
              {(branchPortsByStep.get(selectedStep.id) ?? getBranchPortsByType(selectedStep.type)).map((p) => (
                <div key={p.id} style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
                  <span
                    style={{
                      width: 10,
                      height: 10,
                      borderRadius: 999,
                      display: "inline-block",
                      background: p.color,
                    }}
                  />
                  <Typography.Text style={{ fontSize: 13 }}>
                    <b>{p.label}</b> {"->"} <code>{p.configKey}</code>
                  </Typography.Text>
                </div>
              ))}
            </div>
            {branchSummary(selectedStep) ? (
              <Typography.Text type="secondary" style={{ display: "block", marginBottom: 12, fontSize: 12 }}>
                Nhánh: {branchSummary(selectedStep)}
              </Typography.Text>
            ) : null}
            {selectedStep.type === "playwright_atomics" ? (
              <>
                <StepParamsEditor readOnly params={selectedStep.params ?? {}} onChange={noop} />
                <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
                  Chuỗi atomic (JSON)
                </Typography.Text>
                <pre
                  style={{
                    margin: 0,
                    padding: 12,
                    borderRadius: 10,
                    border: "1px solid var(--flow-panel-border)",
                    background: "var(--flow-canvas-bg)",
                    fontSize: 11,
                    maxHeight: 420,
                    overflow: "auto",
                  }}
                >
                  {JSON.stringify(selectedStep.atomics ?? [], null, 2)}
                </pre>
              </>
            ) : null}
            <StepConfigFields step={selectedStep} readOnly onChange={noop} />
          </>
        ) : null}
      </Modal>
    </div>
  );
}
