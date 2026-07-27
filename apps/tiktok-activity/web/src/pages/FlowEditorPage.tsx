import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button, Card, Form, Input, Modal, Select, Space, Tag, Typography, message } from "antd";
import {
  CopyOutlined,
  DeleteOutlined,
  DownloadOutlined,
  ReloadOutlined,
  ThunderboltOutlined,
  UploadOutlined,
} from "@ant-design/icons";
import {
  ReactFlow,
  Background,
  Controls,
  Handle,
  MarkerType,
  Position,
  type Connection,
  type Edge,
  type Node,
  type NodeChange,
  type NodeProps,
  type ReactFlowInstance,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Link, useMatch, useNavigate, useParams } from "react-router-dom";
import { api } from "../api";
import {
  type Flow,
  type FlowAction,
  type FlowAtomic,
  type FlowGenerateAIResponse,
  type FlowGenerateAIStep,
  type FlowRun,
  type PaletteAction,
  type SavedFlowAction,
  type TikTokAccount,
} from "../types/api";
import { AtomicChainEditor } from "./flows/AtomicChainEditor";
import { StepParamsEditor } from "./flows/StepParamsEditor";
import { ACTIONS, getBranchPortsByType, ui, type BranchPortRule } from "./flows/constants";
import { StepConfigFields } from "./flows/StepConfigFields";
import { getActionStage, normalizeFlowStages } from "./flows/flowStage";
import {
  downloadFlowJson,
  ensureStartStep,
  flowForNewDatabaseRow,
  parseFlowImportJSON,
  wireLinearSuccessEdgesByOrder,
} from "./flows/flowIo";

function normalizeErrText(err: unknown): string {
  const raw = String(err ?? "").trim();
  if (!raw) return "Lỗi không xác định";
  if (raw.startsWith("Error: ")) return raw.slice("Error: ".length).trim();
  return raw;
}

function clonePresetAtomics(source: FlowAtomic[] | undefined): FlowAtomic[] {
  if (!source?.length) return [];
  const t = Date.now();
  return source.map((a, i) => ({
    id: `atom_${t}_${i}_${Math.random().toString(16).slice(2)}`,
    name: a.name,
    kind: a.kind,
    params: { ...(a.params ?? {}) },
  }));
}

function isUsableAtomic(a: FlowAtomic | undefined): boolean {
  if (!a || typeof a !== "object") return false;
  return typeof a.kind === "string" && a.kind.trim() !== "";
}

function chooseAtomicsOrPreset(aiAtomics: FlowAtomic[] | undefined, preset: FlowAtomic[] | undefined): FlowAtomic[] {
  const validAI = (aiAtomics ?? []).filter(isUsableAtomic);
  if (validAI.length > 0) return clonePresetAtomics(validAI);
  return clonePresetAtomics(preset);
}

function stripBranchConfigKeys(cfg: Record<string, string> | undefined): Record<string, string> {
  const c = { ...(cfg ?? {}) };
  for (const k of [
    "_next_on_success",
    "_next_on_error",
    "_next_on_success_step_id",
    "_next_on_error_step_id",
    "_next_alt",
    "_next_alt_step_id",
  ]) {
    delete c[k];
  }
  return c;
}

function expandAIGeneratedSteps(genSteps: FlowGenerateAIStep[], paletteById: Map<string, PaletteAction>): FlowAction[] {
  return genSteps.map((step, idx) => {
    const pa = paletteById.get(step.paletteId);
    if (!pa) {
      throw new Error(`paletteId không có trong catalog: ${step.paletteId}`);
    }
    const id = newStepId();
    const aiCfg = step.config ?? {};
    const stageStr = aiCfg._stage && String(aiCfg._stage).trim() !== "" ? String(aiCfg._stage) : String(idx + 1);

    const tpl = pa.savedStepTemplate;
    if (tpl && typeof tpl === "object" && tpl.type === "playwright_atomics") {
      const cfg = stripBranchConfigKeys(tpl.config);
      const merged: Record<string, string> = { ...cfg, ...aiCfg, _stage: stageStr };
      const ts =
        typeof step.timeoutSeconds === "number" && Number.isFinite(step.timeoutSeconds) && step.timeoutSeconds > 0
          ? Math.floor(step.timeoutSeconds)
          : tpl.timeoutSeconds > 0
            ? tpl.timeoutSeconds
            : 15;
      const atomics = chooseAtomicsOrPreset(step.atomics, tpl.atomics);
      return {
        ...tpl,
        id,
        type: "playwright_atomics",
        name: (step.name || tpl.name || pa.name || "Playwright atomics").trim() || "Playwright atomics",
        timeoutSeconds: ts,
        config: merged,
        params: { ...(tpl.params ?? {}), ...(step.params ?? {}) },
        atomics,
      };
    }

    const mergedBase: Record<string, string> = { ...aiCfg, _stage: stageStr };

    if (pa.type === "playwright_atomics") {
      const ts =
        typeof step.timeoutSeconds === "number" && Number.isFinite(step.timeoutSeconds) && step.timeoutSeconds > 0
          ? Math.floor(step.timeoutSeconds)
          : 15;
      const atomics = chooseAtomicsOrPreset(step.atomics, pa.presetAtomics);
      return {
        id,
        type: "playwright_atomics",
        name: (step.name || pa.name).trim() || "Playwright atomics",
        timeoutSeconds: ts,
        config: mergedBase,
        params: { ...(step.params ?? {}) },
        atomics,
      };
    }

    const ts =
      typeof step.timeoutSeconds === "number" && Number.isFinite(step.timeoutSeconds) && step.timeoutSeconds > 0
        ? Math.floor(step.timeoutSeconds)
        : 15;
    const row: FlowAction = {
      id,
      type: pa.type,
      name: (step.name || pa.name).trim() || pa.type,
      timeoutSeconds: ts,
      config: mergedBase,
    };
    if (step.params && Object.keys(step.params).length > 0) {
      row.params = { ...step.params };
    }
    return row;
  });
}

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

function newStepId(): string {
  return `step_${Date.now()}_${Math.random().toString(16).slice(2, 10)}`;
}

function newAtomicRowId(): string {
  return `atom_${Date.now()}_${Math.random().toString(16).slice(2, 10)}`;
}

/** Bản sao step mới id; atomics id mới; lệch nhẹ _x/_y trên canvas nếu có. */
function duplicateFlowAction(s: FlowAction): FlowAction {
  const cfg = { ...(s.config ?? {}) };
  const ox = Number.parseFloat(String(cfg._x ?? ""));
  const oy = Number.parseFloat(String(cfg._y ?? ""));
  if (Number.isFinite(ox)) cfg._x = String(Math.round(ox + 48));
  if (Number.isFinite(oy)) cfg._y = String(Math.round(oy + 48));
  return {
    ...s,
    id: newStepId(),
    name: `${s.name} (bản sao)`,
    config: cfg,
    params: s.params ? { ...s.params } : undefined,
    atomics: s.atomics?.map((a) => ({
      ...a,
      id: newAtomicRowId(),
      params: a.params ? { ...a.params } : undefined,
    })),
  };
}

export function FlowEditorPage() {
  const navigate = useNavigate();
  const isNewFlow = Boolean(useMatch("/flows/new"));
  const { id: routeFlowId } = useParams<{ id: string }>();
  const reactFlowWrapRef = useRef<HTMLDivElement | null>(null);

  const [editingFlowId, setEditingFlowId] = useState<string | null>(null);
  const [flowName, setFlowName] = useState("");
  const [steps, setSteps] = useState<FlowAction[]>([]);
  const [selectedStepId, setSelectedStepId] = useState<string | null>(null);
  const [paletteQ, setPaletteQ] = useState("");
  const [paletteFilter, setPaletteFilter] = useState<"all" | "engine" | "atomics">("all");
  const [savedFlowActions, setSavedFlowActions] = useState<SavedFlowAction[]>([]);
  const [savedPaletteLoading, setSavedPaletteLoading] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [flowParamsRows, setFlowParamsRows] = useState<Array<{ key: string; value: string }>>([]);
  const [flowInstance, setFlowInstance] = useState<ReactFlowInstance | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedEdgeMeta, setSelectedEdgeMeta] = useState<{ sourceId: string; configKey: string } | null>(null);
  const importFileRef = useRef<HTMLInputElement | null>(null);
  /** Danh sách flow để chọn trong step Run next flow (loại trừ flow đang sửa ở StepConfigFields). */
  const [flowPickerList, setFlowPickerList] = useState<{ id: string; name: string }[]>([]);
  const [aiGenOpen, setAiGenOpen] = useState(false);
  const [aiGenPrompt, setAiGenPrompt] = useState("");
  const [aiGenLoading, setAiGenLoading] = useState(false);
  const [aiGenAccountId, setAiGenAccountId] = useState("");
  const [aiGenAccounts, setAiGenAccounts] = useState<Array<{ id: string; username: string }>>([]);
  const [aiGenAccountsLoading, setAiGenAccountsLoading] = useState(false);
  const [aiBrowserOpening, setAiBrowserOpening] = useState(false);
  /** URL tải trang khi bắt đầu probe; mặc định TikTok home. Chỉ sinh flow sau khi skill-probe thành công. */
  const [aiGenPageUrl, setAiGenPageUrl] = useState("");

  const maxParallelPerStage = 5;

  const asArray = <T,>(v: T[] | null | undefined): T[] => (Array.isArray(v) ? v : []);
  const mapToRows = (m?: Record<string, string>) => Object.entries(m ?? {}).map(([key, value]) => ({ key, value: String(value ?? "") }));
  const rowsToMap = (rows: Array<{ key: string; value: string }>) => {
    const out: Record<string, string> = {};
    for (const r of rows) {
      const k = r.key.trim();
      if (!k) continue;
      out[k] = r.value;
    }
    return out;
  };

  const refreshSavedFlowActions = async () => {
    try {
      setSavedPaletteLoading(true);
      const raw = await api<SavedFlowAction[] | null>("/api/saved-flow-actions");
      setSavedFlowActions(Array.isArray(raw) ? raw : []);
    } catch {
      setSavedFlowActions([]);
      message.warning("Không tải được action đã lưu từ server — kiểm tra API / mạng.");
    } finally {
      setSavedPaletteLoading(false);
    }
  };

  useEffect(() => {
    void refreshSavedFlowActions();
  }, [routeFlowId, isNewFlow]);

  useEffect(() => {
    if (!aiGenOpen || !isNewFlow) return;
    void (async () => {
      setAiGenAccountsLoading(true);
      try {
        const raw = await api<{ items: TikTokAccount[]; total: number } | null>("/api/accounts?page=1&pageSize=500");
        const items = Array.isArray(raw?.items) ? raw.items : [];
        setAiGenAccounts(items.map((x) => ({ id: x.id, username: x.username })));
      } catch {
        setAiGenAccounts([]);
      } finally {
        setAiGenAccountsLoading(false);
      }
    })();
  }, [aiGenOpen, isNewFlow]);

  const openBrowserPreview = async () => {
    const aid = aiGenAccountId.trim();
    if (!aid) {
      message.warning("Chọn account để mở trình duyệt");
      return;
    }
    setAiBrowserOpening(true);
    try {
      const run = await api<FlowRun>("/api/runs/browser-preview", "POST", { accountId: aid });
      message.success({
        content: (
          <span>
            Đã xếp hàng mở trình duyệt (TikTok home). Run <Typography.Text code>{run.id}</Typography.Text> — xem{" "}
            <Link to="/history">Lịch sử</Link>.
          </span>
        ),
        duration: 8,
      });
    } catch (e) {
      message.error(normalizeErrText(e));
    } finally {
      setAiBrowserOpening(false);
    }
  };

  const aiFlowCatalog = useMemo(() => {
    const rows: { paletteId: string; type: string; name: string; implementation: string }[] = [];
    for (const a of ACTIONS) {
      rows.push({ paletteId: a.paletteId, type: a.type, name: a.name, implementation: a.implementation });
    }
    for (const x of savedFlowActions) {
      if (x.step?.type === "playwright_atomics") {
        rows.push({
          paletteId: `saved_sfa_${x.id}`,
          type: "playwright_atomics",
          name: x.name,
          implementation: "atomics",
        });
      }
    }
    return rows;
  }, [savedFlowActions]);

  const aiPaletteById = useMemo(() => {
    const m = new Map<string, PaletteAction>();
    for (const a of ACTIONS) {
      m.set(a.paletteId, a);
    }
    for (const x of savedFlowActions) {
      if (x.step?.type === "playwright_atomics") {
        m.set(`saved_sfa_${x.id}`, {
          paletteId: `saved_sfa_${x.id}`,
          type: "playwright_atomics",
          name: x.name,
          implementation: "atomics",
          savedStepTemplate: { ...x.step },
        });
      }
    }
    return m;
  }, [savedFlowActions]);

  useEffect(() => {
    void (async () => {
      try {
        const raw = await api<Flow[] | null>("/api/flows");
        setFlowPickerList(asArray(raw).map((x) => ({ id: x.id, name: x.name })));
      } catch {
        setFlowPickerList([]);
      }
    })();
  }, [routeFlowId, isNewFlow]);

  useEffect(() => {
    if (isNewFlow) {
      setEditingFlowId(null);
      setFlowName("");
      setSteps([createStartStep()]);
      setFlowParamsRows([]);
      setSelectedStepId(null);
      setError(null);
      return;
    }
    if (!routeFlowId) return;
    void (async () => {
      try {
        setError(null);
        const fRaw = await api<Flow[] | null>("/api/flows");
        const f = asArray(fRaw).find((x) => x.id === routeFlowId);
        if (!f) {
          message.error("Không tìm thấy flow");
          navigate("/flows", { replace: true });
          return;
        }
        setEditingFlowId(f.id);
        setFlowName(f.name);
        setFlowParamsRows(mapToRows(f.params));
        const raw = (f.actions ?? []).map((a) => ({ ...a, config: { ...(a.config ?? {}) } }));
        const withStart = raw.some(isStartStep) ? raw : [createStartStep(), ...raw];
        const prepared = withStart.map((a) =>
          isStartStep(a) ? { ...a, id: "step_start", config: { ...(a.config ?? {}), _stage: "1" } } : a
        );
        setSteps(normalizeFlowStages(prepared, maxParallelPerStage));
        setSelectedStepId(null);
      } catch (e) {
        setError(String(e));
      }
    })();
  }, [isNewFlow, routeFlowId, navigate]);

  const patchStep = (id: string, key: string, value: string) => {
    setSteps((prev) => prev.map((s) => (s.id === id ? { ...s, config: { ...(s.config ?? {}), [key]: value } } : s)));
  };

  const mergeStepConfig = useCallback((id: string, patch: Record<string, string | undefined>) => {
    setSteps((prev) =>
      prev.map((s) => {
        if (s.id !== id) return s;
        const cfg = { ...(s.config ?? {}) };
        for (const [k, val] of Object.entries(patch)) {
          if (val === undefined) delete cfg[k];
          else cfg[k] = val;
        }
        return { ...s, config: cfg };
      })
    );
  }, []);

  const patchStepMeta = (id: string, patch: Partial<FlowAction>) => {
    setSteps((prev) => prev.map((s) => (s.id === id ? { ...s, ...patch } : s)));
  };

  const removeStep = (id: string) => setSteps((prev) => prev.filter((s) => s.id !== id || isStartStep(s)));
  const moveStep = (id: string, dir: -1 | 1) => {
    setSteps((prev) => {
      const idx = prev.findIndex((s) => s.id === id);
      if (idx < 0) return prev;
      if (isStartStep(prev[idx]!)) return prev;
      const target = idx + dir;
      if (target < 0 || target >= prev.length) return prev;
      if (isStartStep(prev[target]!)) return prev;
      const copy = [...prev];
      const [item] = copy.splice(idx, 1);
      if (item === undefined) return prev;
      copy.splice(target, 0, item);
      return copy;
    });
  };

  const duplicateStep = (id: string) => {
    const idx = steps.findIndex((s) => s.id === id);
    if (idx < 0) return;
    const orig = steps[idx]!;
    if (isStartStep(orig)) {
      message.warning("Không nhân bản step Start");
      return;
    }
    const dup = duplicateFlowAction(orig);
    setSteps((prev) => {
      const i = prev.findIndex((s) => s.id === id);
      if (i < 0) return prev;
      const next = [...prev];
      next.splice(i + 1, 0, dup);
      return next;
    });
    setSelectedStepId(dup.id);
    message.success("Đã nhân bản step — chỉnh nhánh trên canvas nếu cần");
  };

  const saveFlowReturnId = async (): Promise<string | null> => {
    try {
      setError(null);
      const body: Record<string, unknown> = {
        name: flowName.trim(),
        params: rowsToMap(flowParamsRows),
        actions: steps.some(isStartStep) ? steps : [createStartStep(), ...steps],
      };
      if (editingFlowId) body.id = editingFlowId;
      const saved = await api<Flow>("/api/flows", "POST", body);
      setEditingFlowId(saved.id);
      return saved.id;
    } catch (err) {
      setError(String(err));
      return null;
    }
  };

  const saveFlow = async () => {
    const id = await saveFlowReturnId();
    if (!id) return;
    message.success("Đã lưu flow");
    navigate("/flows");
  };

  const exportFlowFile = () => {
    const actions = steps.some(isStartStep) ? steps : [createStartStep(), ...steps];
    downloadFlowJson(
      { id: editingFlowId ?? "", name: flowName.trim() || "flow", params: rowsToMap(flowParamsRows), actions },
      flowName.trim() || "flow"
    );
    message.success("Đã tải JSON flow");
  };

  const applyCanvasImport = () => {
    try {
      const parsed = parseFlowImportJSON(importText);
      const { name, params, actions } = flowForNewDatabaseRow(parsed);
      const withStart = ensureStartStep(actions);
      setFlowName(name);
      setFlowParamsRows(mapToRows(params));
      setEditingFlowId(null);
      setSelectedStepId(null);
      setSteps(normalizeFlowStages(withStart, maxParallelPerStage));
      setImportOpen(false);
      setImportText("");
      message.success("Đã import canvas — Save Flow sẽ tạo flow mới trong CSDL");
    } catch (e) {
      message.error(normalizeErrText(e));
    }
  };

  const submitAiGen = async () => {
    const p = aiGenPrompt.trim();
    if (!p) {
      message.warning("Nhập yêu cầu cho AI");
      return;
    }
    const aid = aiGenAccountId.trim();
    if (!aid) {
      message.warning("Chọn account (bắt buộc) — server chạy skill-probe trên trình duyệt thật rồi mới sinh flow");
      return;
    }
    if (aiFlowCatalog.length === 0) {
      message.error("Catalog actions rỗng");
      return;
    }
    setAiGenLoading(true);
    try {
      const body: {
        prompt: string;
        actionsCatalog: typeof aiFlowCatalog;
        accountId: string;
        pageUrl?: string;
      } = { prompt: p, actionsCatalog: aiFlowCatalog, accountId: aid };
      const u = aiGenPageUrl.trim();
      if (u) body.pageUrl = u;
      const out = await api<FlowGenerateAIResponse>("/api/flows/generate-ai", "POST", body);
      const expanded = expandAIGeneratedSteps(out.actions, aiPaletteById);
      const withStart = ensureStartStep(expanded);
      const normalized = normalizeFlowStages(withStart, maxParallelPerStage);
      const wired = wireLinearSuccessEdgesByOrder(normalized);
      setFlowName((out.name ?? "").trim() || "Flow (AI)");
      setFlowParamsRows(mapToRows(out.params ?? {}));
      setEditingFlowId(null);
      setSelectedStepId(null);
      setSteps(wired);
      message.success("Đã sinh flow — kiểm tra canvas rồi Save");
      setAiGenOpen(false);
      setAiGenPrompt("");
      setAiGenAccountId("");
      setAiGenPageUrl("");
    } catch (e) {
      const errMsg = normalizeErrText(e);
      Modal.error({
        title: "Không sinh được JSON flow từ AI",
        content: (
          <Space direction="vertical" size={4}>
            <Typography.Text>{errMsg}</Typography.Text>
            <Typography.Text type="secondary">
              Gợi ý: kiểm tra lại account đang đăng nhập TikTok, mô tả mục tiêu ngắn gọn hơn, hoặc tăng
              `TIKTOK_SKILL_PROBE_MAX_STEPS`.
            </Typography.Text>
          </Space>
        ),
        width: 720,
      });
    } finally {
      setAiGenLoading(false);
    }
  };

  const paletteItems = useMemo((): PaletteAction[] => {
    const s = paletteQ.trim().toLowerCase();
    const searchedActions = !s
      ? [...ACTIONS]
      : ACTIONS.filter(
          (a) =>
            a.name.toLowerCase().includes(s) ||
            a.type.toLowerCase().includes(s) ||
            a.paletteId.toLowerCase().includes(s)
        );
    const fromActions = searchedActions.filter((a) => paletteFilter === "all" || a.implementation === paletteFilter);

    const fromSaved: PaletteAction[] = savedFlowActions
      .filter((x) => x.step?.type === "playwright_atomics")
      .map((x) => ({
        paletteId: `saved_sfa_${x.id}`,
        type: "playwright_atomics" as const,
        name: x.name,
        implementation: "atomics" as const,
        savedStepTemplate: { ...x.step },
      }));

    const filteredSaved = !s
      ? fromSaved
      : fromSaved.filter(
          (a) =>
            a.name.toLowerCase().includes(s) ||
            a.paletteId.toLowerCase().includes(s) ||
            a.type.toLowerCase().includes(s)
        );
    const savedByFilter = paletteFilter === "engine" ? [] : filteredSaved;

    return [...savedByFilter, ...fromActions];
  }, [paletteQ, paletteFilter, savedFlowActions]);
  const selectedStep = useMemo(() => steps.find((s) => s.id === selectedStepId) ?? null, [steps, selectedStepId]);

  const stageCounts = useMemo(() => {
    const map = new Map<number, number>();
    steps.forEach((s, idx) => {
      const st = getActionStage(s, idx + 1);
      map.set(st, (map.get(st) ?? 0) + 1);
    });
    return map;
  }, [steps]);

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
          data: {
            sourceId: s.id,
            configKey: p.configKey,
          },
        });
      });
    });
    return out;
  }, [steps, stageFirstStep, branchPortsByStep]);

  const onNodeChanges = (changes: NodeChange[]) => {
    const moved = new Map<string, { x: number; y: number }>();
    changes.forEach((c) => {
      if (c.type === "position" && c.position) moved.set(c.id, c.position);
    });
    if (!moved.size) return;
    setSteps((prev) =>
      prev.map((s) => {
        const p = moved.get(s.id);
        if (!p) return s;
        if (isStartStep(s)) return s;
        return {
          ...s,
          config: { ...(s.config ?? {}), _x: String(Math.round(p.x)), _y: String(Math.round(p.y)) },
        };
      })
    );
  };

  const onConnect = (conn: Connection) => {
    if (!conn.source || !conn.target || !conn.sourceHandle) return;
    const sourceStep = steps.find((s) => s.id === conn.source);
    if (!sourceStep) return;
    const ports = branchPortsByStep.get(sourceStep.id) ?? getBranchPortsByType(sourceStep.type);
    const branchKey = ports.find((p) => p.id === conn.sourceHandle)?.configKey ?? "";
    if (!branchKey) return;
    const targetStep = steps.find((s) => s.id === conn.target);
    if (!targetStep) return;
    const targetStage = String(getActionStage(targetStep, 1));
    setSteps((prev) =>
      prev.map((s) =>
        s.id === conn.source
          ? {
              ...s,
              config: {
                ...(s.config ?? {}),
                [branchKey]: targetStage,
                [`${branchKey}_step_id`]: conn.target!,
              },
            }
          : s
      )
    );
  };

  const removeSelectedConnection = () => {
    if (!selectedEdgeMeta) return;
    setSteps((prev) =>
      prev.map((s) => {
        if (s.id !== selectedEdgeMeta.sourceId) return s;
        const cfg = { ...(s.config ?? {}) };
        delete cfg[selectedEdgeMeta.configKey];
        delete cfg[`${selectedEdgeMeta.configKey}_step_id`];
        return { ...s, config: cfg };
      })
    );
    setSelectedEdgeMeta(null);
  };

  const onCanvasDrop = (e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    const raw = e.dataTransfer.getData("application/x-flow-action");
    if (!raw) return;
    const parsed = JSON.parse(raw) as Partial<PaletteAction> & { type: PaletteAction["type"]; name: string };
    const flowPos = flowInstance?.screenToFlowPosition({ x: e.clientX, y: e.clientY });
    const rect = reactFlowWrapRef.current?.getBoundingClientRect();
    const x = Number.isFinite(flowPos?.x) ? flowPos!.x : rect ? e.clientX - rect.left : 120;
    const y = Number.isFinite(flowPos?.y) ? flowPos!.y : rect ? e.clientY - rect.top : 100;
    const rawStage = Math.floor(Math.max(0, x - 80) / 280) + 1;
    let stage = Math.max(1, rawStage);
    while ((stageCounts.get(stage) ?? 0) >= maxParallelPerStage) stage += 1;
    const id = `step_${Date.now()}_${Math.random().toString(16).slice(2)}`;

    const tpl = parsed.savedStepTemplate;
    if (tpl && typeof tpl === "object" && tpl.type === "playwright_atomics") {
      const cfg = { ...(tpl.config ?? {}) };
      delete cfg._next_on_success;
      delete cfg._next_on_error;
      delete cfg._next_on_success_step_id;
      delete cfg._next_on_error_step_id;
      delete cfg._next_alt;
      delete cfg._next_alt_step_id;
      const step: FlowAction = {
        ...tpl,
        id,
        type: "playwright_atomics",
        name: (tpl.name || parsed.name || "Playwright atomics").trim() || "Playwright atomics",
        timeoutSeconds: tpl.timeoutSeconds > 0 ? tpl.timeoutSeconds : 15,
        config: {
          ...cfg,
          _stage: String(stage),
          _x: String(Math.round(x)),
          _y: String(Math.round(y)),
        },
        params: { ...(tpl.params ?? {}) },
        atomics: clonePresetAtomics(tpl.atomics),
      };
      setSteps((prev) => prev.concat(step));
      setSelectedStepId(id);
      return;
    }

    const presetAtomics =
      Array.isArray(parsed.presetAtomics) && parsed.presetAtomics.length > 0
        ? (parsed.presetAtomics as FlowAtomic[])
        : undefined;
    const action: PaletteAction = {
      paletteId:
        typeof parsed.paletteId === "string" && parsed.paletteId.length > 0 ? parsed.paletteId : parsed.type,
      type: parsed.type,
      name: parsed.name,
      implementation:
        parsed.implementation ?? (parsed.type === "playwright_atomics" ? "atomics" : "engine"),
      presetAtomics,
    };
    const base: FlowAction = {
      id,
      type: action.type,
      name: action.name,
      timeoutSeconds: 15,
      config: { _stage: String(stage), _x: String(Math.round(x)), _y: String(Math.round(y)) },
    };
    const step: FlowAction =
      action.type === "playwright_atomics"
        ? { ...base, atomics: clonePresetAtomics(action.presetAtomics), params: {} }
        : base;
    setSteps((prev) => prev.concat(step));
    setSelectedStepId(id);
  };

  const onPaletteDragStart = (e: React.DragEvent<HTMLDivElement>, action: PaletteAction) => {
    e.dataTransfer.setData("application/x-flow-action", JSON.stringify(action));
    e.dataTransfer.effectAllowed = "copy";
  };

  return (
    <div className="page" style={ui.page}>
      {error && <pre className="error">{error}</pre>}

      <Card
          title={isNewFlow ? "Thêm flow" : "Sửa flow"}
          extra={
            <Space wrap size="middle">
              <Button variant="outlined" color="primary" onClick={() => navigate("/flows/actions")}>
                Danh sách actions
              </Button>
              <Button variant="outlined" color="primary" onClick={() => navigate("/flows/actions/build")}>
                Tạo action (atomic)
              </Button>
              {isNewFlow ? (
                <Button
                  variant="outlined"
                  color="primary"
                  icon={<ThunderboltOutlined />}
                  onClick={() => setAiGenOpen(true)}
                >
                  Sinh bằng AI
                </Button>
              ) : null}
              <Button variant="outlined" onClick={() => navigate("/flows")}>
                Về danh sách
              </Button>
            </Space>
          }
        >
          <div style={ui.editorHeader}>
            <div style={{ flex: 1, minWidth: 320 }}>
              <div style={{ fontWeight: 700, marginBottom: 6 }}>Flow name</div>
              <Input value={flowName} onChange={(e) => setFlowName(e.target.value)} placeholder="Tên flow" />
              <div style={{ marginTop: 10, border: "1px solid var(--flow-panel-border)", borderRadius: 8, padding: 10 }}>
                <Typography.Text strong style={{ display: "block", marginBottom: 8 }}>
                  Flow params (default) - thêm/xóa key-value
                </Typography.Text>
                <Space direction="vertical" style={{ width: "100%" }}>
                  {flowParamsRows.map((row, idx) => (
                    <Space key={`fp-${idx}`} wrap>
                      <Input
                        placeholder="key"
                        value={row.key}
                        onChange={(e) =>
                          setFlowParamsRows((prev) => prev.map((x, i) => (i === idx ? { ...x, key: e.target.value } : x)))
                        }
                        style={{ width: 160 }}
                      />
                      <Input
                        placeholder="default value"
                        value={row.value}
                        onChange={(e) =>
                          setFlowParamsRows((prev) => prev.map((x, i) => (i === idx ? { ...x, value: e.target.value } : x)))
                        }
                        style={{ width: 220 }}
                      />
                      <Button danger onClick={() => setFlowParamsRows((prev) => prev.filter((_, i) => i !== idx))}>
                        Xóa
                      </Button>
                    </Space>
                  ))}
                  <Button onClick={() => setFlowParamsRows((prev) => [...prev, { key: "", value: "" }])}>+ Thêm param</Button>
                </Space>
              </div>
              {editingFlowId ? (
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  ID: {editingFlowId}
                </Typography.Text>
              ) : null}
            </div>
            <Space wrap>
              <Button icon={<DownloadOutlined />} onClick={exportFlowFile}>
                Export JSON
              </Button>
              <Button icon={<UploadOutlined />} onClick={() => setImportOpen(true)}>
                Import JSON
              </Button>
              <Button type="primary" onClick={() => void saveFlow()} disabled={!flowName.trim()}>
                Save Flow
              </Button>
            </Space>
          </div>

          <div style={ui.editorBody}>
            <div style={ui.leftPanel}>
              <div style={ui.leftPanelHeader}>
                <div style={{ fontWeight: 700, marginBottom: 8 }}>Palette</div>
                <Space.Compact style={{ width: "100%" }}>
                  <Input
                    value={paletteQ}
                    onChange={(e) => setPaletteQ(e.target.value)}
                    placeholder="Search actions"
                    allowClear
                    style={{ flex: 1 }}
                  />
                  <Button
                    icon={<ReloadOutlined />}
                    loading={savedPaletteLoading}
                    onClick={() => void refreshSavedFlowActions()}
                    title="Làm mới action đã lưu (server)"
                  />
                </Space.Compact>
                <Typography.Text type="secondary" style={{ fontSize: 12, display: "block", marginTop: 8 }}>
                  Kéo vào canvas: <Tag style={{ marginInline: 4 }}>engine</Tag> code Go · <Tag color="purple">atomics</Tag> chỉnh
                  bằng chuỗi atomic.
                </Typography.Text>
                <Space size={6} style={{ marginTop: 8 }}>
                  <Button size="small" type={paletteFilter === "all" ? "primary" : "default"} onClick={() => setPaletteFilter("all")}>
                    all
                  </Button>
                  <Button size="small" type={paletteFilter === "engine" ? "primary" : "default"} onClick={() => setPaletteFilter("engine")}>
                    engine
                  </Button>
                  <Button size="small" type={paletteFilter === "atomics" ? "primary" : "default"} onClick={() => setPaletteFilter("atomics")}>
                    atomics
                  </Button>
                </Space>
              </div>
              <div style={ui.paletteList}>
                {paletteItems.map((a) => (
                  <div key={a.paletteId} draggable onDragStart={(e) => onPaletteDragStart(e, a)} style={ui.paletteItem}>
                    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8, marginBottom: 4 }}>
                      <span style={{ fontWeight: 700 }}>{a.name}</span>
                      <span style={{ display: "flex", gap: 4, flexWrap: "wrap", justifyContent: "flex-end" }}>
                        {a.paletteId.startsWith("saved_sfa_") ? (
                          <Tag color="cyan" style={{ margin: 0, fontSize: 10 }}>
                            server
                          </Tag>
                        ) : null}
                        <Tag color={a.implementation === "atomics" ? "purple" : "processing"} style={{ margin: 0, fontSize: 10 }}>
                          {a.implementation === "atomics" ? "atomics" : "engine"}
                        </Tag>
                      </span>
                    </div>
                    <div style={{ fontSize: 12, color: "var(--muted-text)" }}>{a.type}</div>
                  </div>
                ))}
              </div>
            </div>

            <div style={ui.canvasWrap}>
              <div style={ui.canvasTopBar}>
                <Typography.Text strong>Canvas</Typography.Text>
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  Kéo action từ palette vào đây. Click step để mở config. Kéo step để đổi thứ tự.
                </Typography.Text>
              </div>
              <div
                ref={reactFlowWrapRef}
                style={{
                  border: "1px dashed var(--flow-panel-border)",
                  borderRadius: 10,
                  height: 560,
                  overflow: "hidden",
                  background: "var(--flow-canvas-bg)",
                }}
                onDragOver={(e) => {
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "copy";
                }}
                onDrop={onCanvasDrop}
              >
                <ReactFlow
                  nodes={flowNodes}
                  edges={flowEdges}
                  nodeTypes={{ flowStep: FlowStepNode }}
                  fitView
                  onInit={setFlowInstance}
                  defaultEdgeOptions={{ markerEnd: { type: MarkerType.ArrowClosed } }}
                  onNodesChange={onNodeChanges}
                  onConnect={onConnect}
                  onNodeClick={(_, node) => setSelectedStepId(node.id)}
                  onEdgeClick={(_, edge) => {
                    const sourceId = String(edge.data?.sourceId ?? edge.source ?? "");
                    const configKey = String(edge.data?.configKey ?? "");
                    if (!sourceId || !configKey) return;
                    setSelectedEdgeMeta({ sourceId, configKey });
                  }}
                  onPaneClick={() => setSelectedEdgeMeta(null)}
                >
                  <Background />
                  <Controls />
                </ReactFlow>
                {selectedEdgeMeta ? (
                  <Button
                    danger
                    type="primary"
                    icon={<DeleteOutlined />}
                    style={{ position: "absolute", top: 10, right: 10, zIndex: 10 }}
                    onClick={removeSelectedConnection}
                  >
                    Xóa kết nối
                  </Button>
                ) : null}
              </div>
            </div>
          </div>

          <Modal
            title={selectedStep ? `Step Config: ${selectedStep.name}` : "Step Config"}
            open={!!selectedStep}
            onCancel={() => setSelectedStepId(null)}
            footer={null}
            destroyOnClose
            width={selectedStep?.type === "playwright_atomics" ? 1240 : 520}
            style={selectedStep?.type === "playwright_atomics" ? { maxWidth: "calc(100vw - 24px)" } : undefined}
          >
            {selectedStep ? (
              <>
                {selectedStep.type === "check_login" ? (
                  <Typography.Paragraph type="secondary" style={{ marginBottom: 12 }}>
                    Step này kiểm tra trạng thái đăng nhập. Có thể cấu hình nhánh lỗi để chuyển qua stage login.
                  </Typography.Paragraph>
                ) : null}
                {selectedStep.type === "open_url" ? (
                  <Typography.Paragraph type="secondary" style={{ marginBottom: 12 }}>
                    Mở bất kỳ URL <code>http/https</code> (config <code>url</code>). Tùy chọn: <code>wait_until</code> (domcontentloaded | load | …),{" "}
                    <code>timeout_ms</code>. Có thể dùng <code>{"{{prev.*}}"}</code> trong config khi lưu flow.
                  </Typography.Paragraph>
                ) : null}
                {selectedStep.type === "wait_page_ready" ? (
                  <Typography.Paragraph type="secondary" style={{ marginBottom: 12 }}>
                    Chờ trang <b>hiện tại</b> đạt <code>state</code> (mặc định <code>load</code>). Dùng sau bước mở trang nếu chỉ dùng{" "}
                    <code>domcontentloaded</code> và bước sau cần DOM/network ổn định hơn.
                  </Typography.Paragraph>
                ) : null}
                {selectedStep.type === "if_condition" ? (
                  <Typography.Paragraph type="secondary" style={{ marginBottom: 12 }}>
                    Điều kiện đúng → nhánh <b>ok</b> (<code>_next_on_success</code>), sai → nhánh <b>err</b> (<code>_next_on_error</code>). Cùng tham số với atomic{" "}
                    <code>assert</code> (expect, selector, selectors, value, pattern, text, timeout_ms).
                  </Typography.Paragraph>
                ) : null}
                {selectedStep.type === "random_yes_no" ? (
                  <Typography.Paragraph type="secondary" style={{ marginBottom: 12 }}>
                    Gieo ngẫu nhiên 0–99: nếu <code>roll &lt; yes_percent</code> thì <b>yes</b> (<code>_next_on_success</code>), ngược lại <b>no</b> (
                    <code>_next_on_error</code>). Output: <code>result</code>, <code>roll</code>, <code>yes_percent</code> (dùng <code>{"{{prev.*}}"}</code>).
                  </Typography.Paragraph>
                ) : null}
                {selectedStep.type === "playwright_atomics" ? (
                  <Typography.Paragraph type="secondary" style={{ marginBottom: 12 }}>
                    Ghép các <b>atomic</b> (click, fill, chờ, goto, …) theo thứ tự — engine chạy tuần tự qua Playwright. Có thể export/import JSON.
                  </Typography.Paragraph>
                ) : null}
                <Form layout="vertical">
                  <Form.Item label="Name">
                    <Input
                      value={selectedStep.name}
                      onChange={(e) => patchStepMeta(selectedStep.id, { name: e.target.value })}
                      disabled={isStartStep(selectedStep)}
                    />
                  </Form.Item>
                  <Form.Item label="Timeout (seconds)">
                    <Input
                      type="number"
                      value={selectedStep.timeoutSeconds}
                      disabled={isStartStep(selectedStep)}
                      onChange={(e) => patchStepMeta(selectedStep.id, { timeoutSeconds: Number(e.target.value) || 15 })}
                    />
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
                        <b>{p.label}</b> {"->"} lưu vào <code>{p.configKey}</code>
                      </Typography.Text>
                    </div>
                  ))}
                </div>
                {selectedStep.type === "playwright_atomics" ? (
                  <>
                    <StepParamsEditor
                      params={selectedStep.params ?? {}}
                      onChange={(next) => patchStepMeta(selectedStep.id, { params: next })}
                    />
                    <AtomicChainEditor
                      atomics={selectedStep.atomics ?? []}
                      onChange={(next) => patchStepMeta(selectedStep.id, { atomics: next })}
                    />
                  </>
                ) : null}
                <StepConfigFields
                  step={selectedStep}
                  onChange={(k, v) => patchStep(selectedStep.id, k, v)}
                  onMergeConfig={(patch) => mergeStepConfig(selectedStep.id, patch)}
                  flowPickerOptions={flowPickerList}
                  excludeFlowId={editingFlowId}
                />
                <Space style={{ marginTop: 12 }} wrap>
                  <Button onClick={() => moveStep(selectedStep.id, -1)} disabled={isStartStep(selectedStep)}>
                    Move Up
                  </Button>
                  <Button onClick={() => moveStep(selectedStep.id, 1)} disabled={isStartStep(selectedStep)}>
                    Move Down
                  </Button>
                  <Button icon={<CopyOutlined />} onClick={() => duplicateStep(selectedStep.id)} disabled={isStartStep(selectedStep)}>
                    Nhân bản step
                  </Button>
                  <Button danger onClick={() => removeStep(selectedStep.id)} disabled={isStartStep(selectedStep)}>
                    Delete Step
                  </Button>
                </Space>
              </>
            ) : null}
          </Modal>

          <input
            ref={importFileRef}
            type="file"
            accept="application/json,.json"
            style={{ display: "none" }}
            onChange={(e) => {
              const f = e.target.files?.[0];
              if (!f) return;
              void f.text().then((t) => {
                setImportText(t);
                message.info("Đã đọc file — kiểm tra rồi nhấn Thay canvas");
              });
              e.target.value = "";
            }}
          />
          <Modal
            title="Sinh flow bằng AI"
            open={aiGenOpen}
            onCancel={() => {
              if (!aiGenLoading) {
                setAiGenOpen(false);
                setAiGenPrompt("");
                setAiGenAccountId("");
                setAiGenPageUrl("");
              }
            }}
            closable={!aiGenLoading}
            maskClosable={!aiGenLoading}
            keyboard={!aiGenLoading}
            destroyOnClose
            footer={[
              <Button
                key="cancel"
                disabled={aiGenLoading}
                onClick={() => {
                  if (!aiGenLoading) {
                    setAiGenOpen(false);
                    setAiGenPrompt("");
                    setAiGenAccountId("");
                    setAiGenPageUrl("");
                  }
                }}
              >
                Đóng
              </Button>,
              <Button key="go" type="primary" loading={aiGenLoading} onClick={() => void submitAiGen()}>
                {aiGenLoading ? "Đang probe trình duyệt & sinh flow…" : "Sinh flow"}
              </Button>,
            ]}
          >
            <Typography.Paragraph type="secondary" style={{ marginBottom: 8 }}>
              Mô tả flow (tiếng Việt). Server mở Chromium theo <b>profile account</b>, tải URL, rồi chạy{" "}
              <b>skill-probe</b>: AI điều khiển trang thật (đọc DOM, thử thao tác) theo mô tả cho đến khi kết thúc bằng{" "}
              <Typography.Text code>done</Typography.Text> với tóm tắt thành công đó mới chụp DOM và gọi LLM ghép
              catalog. Nếu probe thất bại, làm rõ mục tiêu hoặc tăng{" "}
              <Typography.Text code>TIKTOK_SKILL_PROBE_MAX_STEPS</Typography.Text>. Trong lúc chạy (có thể lâu).
            </Typography.Paragraph>
            <div style={{ marginBottom: 12 }}>
              <Typography.Text strong style={{ display: "block", marginBottom: 6 }}>
                Account (bắt buộc)
              </Typography.Text>
              <Space wrap style={{ width: "100%" }} align="start">
                <Select
                  style={{ minWidth: 280 }}
                  placeholder="Chọn account — probe + sinh flow"
                  showSearch
                  optionFilterProp="label"
                  loading={aiGenAccountsLoading}
                  disabled={aiGenLoading}
                  value={aiGenAccountId || undefined}
                  onChange={(v) => setAiGenAccountId(typeof v === "string" ? v : "")}
                  options={aiGenAccounts.map((x) => ({
                    value: x.id,
                    label: `${x.username} (${x.id})`,
                  }))}
                />
                <Button
                  disabled={!aiGenAccountId.trim() || aiGenLoading || aiBrowserOpening}
                  loading={aiBrowserOpening}
                  onClick={() => void openBrowserPreview()}
                >
                  Mở trình duyệt kiểm tra
                </Button>
              </Space>
              <Input
                style={{ marginTop: 10, maxWidth: 560 }}
                placeholder="URL quét trước khi sinh (để trống = https://www.tiktok.com/)"
                value={aiGenPageUrl}
                onChange={(e) => setAiGenPageUrl(e.target.value)}
                disabled={aiGenLoading}
              />
              <Typography.Text type="secondary" style={{ fontSize: 12, display: "block", marginTop: 6 }}>
                «Mở trình duyệt» chạy flow tối giản (Start → Open home) trên profile đã chọn — xem{" "}
                <Link to="/history">Lịch sử</Link>. Sinh flow cần Playwright (cài driver như khi chạy automation). Không gửi mật
                khẩu cho LLM.
              </Typography.Text>
            </div>
            <Input.TextArea
              rows={6}
              value={aiGenPrompt}
              onChange={(e) => setAiGenPrompt(e.target.value)}
              disabled={aiGenLoading}
              placeholder="Ví dụ: Mở home TikTok, chờ trang tải, xem video 25s, like bằng preset atomic, random delay 2–5s rồi chuyển video tiếp…"
            />
          </Modal>
          <Modal
            title="Import JSON vào canvas"
            open={importOpen}
            onCancel={() => {
              setImportOpen(false);
              setImportText("");
            }}
            onOk={applyCanvasImport}
            okText="Thay canvas"
            width={640}
            destroyOnClose
          >
            <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 8 }}>
              Thay toàn bộ bước hiện tại. Id step được map lại; sau đó <b>Save Flow</b> tạo bản ghi mới (id flow rỗng).
            </Typography.Paragraph>
            <Space style={{ marginBottom: 8 }}>
              <Button icon={<UploadOutlined />} onClick={() => importFileRef.current?.click()}>
                Chọn file .json
              </Button>
            </Space>
            <Input.TextArea rows={14} value={importText} onChange={(e) => setImportText(e.target.value)} placeholder='{"name":"...","actions":[...]}' />
          </Modal>
        </Card>
    </div>
  );
}

