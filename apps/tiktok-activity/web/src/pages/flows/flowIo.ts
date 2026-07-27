import type { Flow, FlowAction, FlowAtomic } from "../../types/api";

function newStepId(): string {
  return `step_${Date.now()}_${Math.random().toString(16).slice(2, 10)}`;
}

function newAtomicId(): string {
  return `atom_${Date.now()}_${Math.random().toString(16).slice(2, 10)}`;
}

/** Gán id mới cho mọi step + thay mọi chuỗi config/params (kể cả {{step.old.*}}) theo map id cũ → mới. */
export function remapFlowActionsForCopy(actions: FlowAction[]): FlowAction[] {
  const idMap = new Map<string, string>();
  for (const a of actions) {
    const oldId = String(a.id ?? "").trim();
    if (!oldId) continue;
    if (!idMap.has(oldId)) {
      idMap.set(oldId, newStepId());
    }
  }
  const pairs = [...idMap.entries()].sort((a, b) => b[0].length - a[0].length);
  const remap = (s: string) => {
    let out = s;
    for (const [oldId, newId] of pairs) {
      out = out.split(oldId).join(newId);
    }
    return out;
  };
  const remapAtomics = (atomics: FlowAtomic[] | undefined): FlowAtomic[] | undefined => {
    if (!atomics?.length) return atomics;
    return atomics.map((at) => ({
      ...at,
      id: newAtomicId(),
      params: at.params
        ? Object.fromEntries(Object.entries(at.params).map(([k, v]) => [k, remap(String(v ?? ""))]))
        : undefined,
    }));
  };
  return actions.map((a) => {
    const oldId = String(a.id ?? "").trim();
    const nextId = oldId && idMap.has(oldId) ? idMap.get(oldId)! : newStepId();
    return {
      ...a,
      id: nextId,
      config: Object.fromEntries(Object.entries(a.config ?? {}).map(([k, v]) => [k, remap(String(v ?? ""))])),
      params: a.params
        ? Object.fromEntries(Object.entries(a.params).map(([k, v]) => [k, remap(String(v ?? ""))]))
        : undefined,
      atomics: remapAtomics(a.atomics),
    };
  });
}

/** Chuẩn bị body POST /api/flows: luôn id rỗng + id step mới (tránh đè / gãy nhánh). */
export function flowForNewDatabaseRow(flow: Pick<Flow, "name" | "actions" | "params">): Omit<Flow, "updatedAt"> {
  const name = (flow.name ?? "").trim() || "Flow";
  const actions = remapFlowActionsForCopy(flow.actions ?? []);
  const params = flow.params && typeof flow.params === "object" ? flow.params : {};
  return { id: "", name, params, actions };
}

/** Engine dùng step start nếu có; nếu thiếu thì chèn một step start đầu chuỗi. */
export function ensureStartStep(actions: FlowAction[]): FlowAction[] {
  if (actions.some((a) => a.type === "start")) {
    return actions;
  }
  const start: FlowAction = {
    id: `step_${Date.now()}_${Math.random().toString(16).slice(2, 10)}`,
    type: "start",
    name: "Start",
    timeoutSeconds: 0,
    config: { _stage: "1" },
  };
  return [start, ...actions];
}

/**
 * Nối nhánh ok tuyến tính: mỗi bước (trừ bước cuối) gán `_next_on_success_step_id` → id bước kế tiếp theo thứ tự mảng.
 * Xóa `_next_on_success` (số stage) để tránh lệch sau normalize — canvas và runner đều ưu tiên *_step_id.
 * Bỏ qua bước `loop_repeat` (giữ config nhánh loop/done nếu AI đã gửi).
 */
export function wireLinearSuccessEdgesByOrder(steps: FlowAction[]): FlowAction[] {
  if (steps.length === 0) return steps;
  const skipFrom = new Set<string>(["loop_repeat"]);
  return steps.map((s, i) => {
    const cfg = { ...(s.config ?? {}) };
    if (i >= steps.length - 1) {
      delete cfg._next_on_success_step_id;
      delete cfg._next_on_success;
      return { ...s, config: cfg };
    }
    if (skipFrom.has(s.type)) {
      return { ...s, config: cfg };
    }
    const next = steps[i + 1];
    if (!next?.id) return { ...s, config: cfg };
    cfg._next_on_success_step_id = next.id;
    delete cfg._next_on_success;
    return { ...s, config: cfg };
  });
}

export function parseFlowImportJSON(text: string): Pick<Flow, "name" | "actions" | "params"> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error("JSON không hợp lệ");
  }
  if (!parsed || typeof parsed !== "object") {
    throw new Error("Root phải là object");
  }
  const o = parsed as Record<string, unknown>;
  const actions = o.actions;
  if (!Array.isArray(actions)) {
    throw new Error("Thiếu mảng actions");
  }
  const name = typeof o.name === "string" ? o.name : "Flow (import)";
  const params =
    o.params && typeof o.params === "object" && !Array.isArray(o.params)
      ? (o.params as Record<string, string>)
      : {};
  const out: FlowAction[] = [];
  for (let i = 0; i < actions.length; i++) {
    const a = actions[i];
    if (!a || typeof a !== "object") {
      throw new Error(`actions[${i}] không hợp lệ`);
    }
    const row = a as Record<string, unknown>;
    if (!row.id || typeof row.id !== "string") {
      throw new Error(`actions[${i}]: thiếu id`);
    }
    if (!row.type || typeof row.type !== "string") {
      throw new Error(`actions[${i}]: thiếu type`);
    }
    const timeoutSeconds =
      typeof row.timeoutSeconds === "number" && Number.isFinite(row.timeoutSeconds)
        ? row.timeoutSeconds
        : parseInt(String(row.timeoutSeconds ?? "15"), 10) || 15;
    const config =
      row.config && typeof row.config === "object" && !Array.isArray(row.config)
        ? (row.config as Record<string, string>)
        : {};
    const params =
      row.params && typeof row.params === "object" && !Array.isArray(row.params)
        ? (row.params as Record<string, string>)
        : undefined;
    let atomics: FlowAtomic[] | undefined;
    if (Array.isArray(row.atomics)) {
      atomics = row.atomics.map((x, j) => {
        if (!x || typeof x !== "object") {
          throw new Error(`actions[${i}].atomics[${j}] không hợp lệ`);
        }
        const ax = x as Record<string, unknown>;
        if (!ax.kind || typeof ax.kind !== "string") {
          throw new Error(`actions[${i}].atomics[${j}]: thiếu kind`);
        }
        const ap =
          ax.params && typeof ax.params === "object" && !Array.isArray(ax.params)
            ? (ax.params as Record<string, string>)
            : undefined;
        return {
          id: typeof ax.id === "string" ? ax.id : undefined,
          name: typeof ax.name === "string" ? ax.name : undefined,
          kind: ax.kind,
          params: ap,
        };
      });
    }
    out.push({
      id: row.id,
      type: row.type as FlowAction["type"],
      name: typeof row.name === "string" ? row.name : String(row.type),
      config,
      timeoutSeconds,
      params,
      atomics,
    });
  }
  return { name, params, actions: out };
}

export function downloadFlowJson(flow: Partial<Flow> & { name: string; actions: FlowAction[] }, filenameHint?: string) {
  const exportObj = {
    id: flow.id,
    name: flow.name,
    params: flow.params ?? {},
    actions: flow.actions,
    ...(flow.updatedAt ? { updatedAt: flow.updatedAt } : {}),
  };
  const blob = new Blob([JSON.stringify(exportObj, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  const base = filenameHint ?? flow.name ?? "flow";
  a.download = `${sanitizeFilename(base)}.json`;
  a.click();
  URL.revokeObjectURL(url);
}

function sanitizeFilename(s: string): string {
  const t = s.replace(/[^\w\-\s\u00C0-\u024F]+/g, "_").trim().replace(/\s+/g, "_");
  return (t || "flow").slice(0, 80);
}

function normalizeAtomicsFromUnknownArray(rows: unknown[], pathPrefix: string): FlowAtomic[] {
  const out: FlowAtomic[] = [];
  for (let j = 0; j < rows.length; j++) {
    const x = rows[j];
    if (!x || typeof x !== "object") {
      throw new Error(`${pathPrefix}[${j}] không hợp lệ`);
    }
    const ax = x as Record<string, unknown>;
    if (!ax.kind || typeof ax.kind !== "string") {
      throw new Error(`${pathPrefix}[${j}]: thiếu kind`);
    }
    const ap =
      ax.params && typeof ax.params === "object" && !Array.isArray(ax.params)
        ? (ax.params as Record<string, string>)
        : undefined;
    out.push({
      id: typeof ax.id === "string" ? ax.id : undefined,
      name: typeof ax.name === "string" ? ax.name : undefined,
      kind: ax.kind,
      params: ap,
    });
  }
  return out;
}

/** Kết quả import chuỗi atomic từ JSON (chỉ atomics hoặc cả file flow). */
export type AtomicsImportResult = {
  atomics: FlowAtomic[];
  /** Có khi root là export flow */
  flowName?: string;
  /** Mô tả nguồn (tên step playwright_atomics hoặc gộp nhiều step) */
  sourceHint?: string;
};

/**
 * Parse JSON cho Import atomics: hỗ trợ
 * - `{ "atomics": [ ... ] }`
 * - export flow `{ "name"?, "actions": [ { "type":"playwright_atomics", "atomics": [...] }, ... ] }` — gộp theo thứ tự các step playwright_atomics.
 */
export function parseAtomicsImportPayload(text: string): AtomicsImportResult {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error("JSON không hợp lệ");
  }
  if (!parsed || typeof parsed !== "object") {
    throw new Error("Root phải là object");
  }
  const o = parsed as Record<string, unknown>;

  if (Array.isArray(o.atomics)) {
    const flowName = typeof o.name === "string" ? o.name : undefined;
    return {
      atomics: normalizeAtomicsFromUnknownArray(o.atomics, "atomics"),
      flowName,
    };
  }

  if (Array.isArray(o.actions)) {
    const actions = o.actions;
    const collected: FlowAtomic[] = [];
    const stepLabels: string[] = [];
    for (let i = 0; i < actions.length; i++) {
      const a = actions[i];
      if (!a || typeof a !== "object") continue;
      const row = a as Record<string, unknown>;
      if (row.type !== "playwright_atomics") continue;
      if (!Array.isArray(row.atomics) || row.atomics.length === 0) continue;
      const chunk = normalizeAtomicsFromUnknownArray(row.atomics, `actions[${i}].atomics`);
      collected.push(...chunk);
      const nm = typeof row.name === "string" ? row.name : `step ${i}`;
      stepLabels.push(nm);
    }
    if (collected.length === 0) {
      throw new Error('Flow không có step type "playwright_atomics" nào chứa atomics');
    }
    const flowName = typeof o.name === "string" ? o.name : undefined;
    const sourceHint =
      stepLabels.length > 1
        ? `Đã gộp ${stepLabels.length} step: ${stepLabels.join(" · ")}`
        : stepLabels[0]
          ? `Lấy từ step: ${stepLabels[0]}`
          : undefined;
    return { atomics: collected, flowName, sourceHint };
  }

  throw new Error('Cần { "atomics": [...] } hoặc object flow có mảng actions với step playwright_atomics');
}
