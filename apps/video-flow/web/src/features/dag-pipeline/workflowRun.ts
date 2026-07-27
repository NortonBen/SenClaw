/**
 * Defensive normalizer for the SenClaw workflow-run JSON.
 *
 * The daemon owns the shape of this payload and it has changed shape before,
 * so nothing here assumes a specific schema. We probe a list of plausible key
 * names for the run status and the per-step array, and report `matched: false`
 * when we cannot find anything — the UI then falls back to rendering the raw
 * JSON instead of showing a misleadingly empty progress view.
 */

export type NormalizedStep = {
  id: string;
  status: string;
  label?: string;
  error?: string;
};

export type NormalizedRun = {
  status: string;
  steps: NormalizedStep[];
  /** false when we could not recognise the payload at all */
  matched: boolean;
};

const STATUS_KEYS = ["status", "state", "run_status", "runStatus", "phase"];
const STEPS_KEYS = [
  "steps",
  "nodes",
  "stepRuns",
  "step_runs",
  "nodeRuns",
  "node_runs",
  "tasks",
  "step_states",
  "stepStates",
];
const ID_KEYS = ["id", "stepId", "step_id", "nodeId", "node_id", "key", "name", "step"];
const LABEL_KEYS = ["label", "title", "name", "description"];
const ERROR_KEYS = ["error", "error_message", "errorMessage", "message", "failure"];

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function pickString(obj: Record<string, unknown>, keys: string[]): string | undefined {
  for (const k of keys) {
    const v = obj[k];
    if (typeof v === "string" && v.trim()) return v.trim();
  }
  return undefined;
}

/** Candidate roots to probe: the payload itself plus common wrapper keys. */
function roots(raw: unknown): Record<string, unknown>[] {
  const out: Record<string, unknown>[] = [];
  if (!isRecord(raw)) return out;
  out.push(raw);
  for (const k of ["run", "workflow", "data", "result", "state"]) {
    const v = raw[k];
    if (isRecord(v)) out.push(v);
  }
  return out;
}

function toStep(item: unknown, fallbackId: string): NormalizedStep | null {
  if (typeof item === "string") return { id: item, status: "unknown" };
  if (!isRecord(item)) return null;
  const id = pickString(item, ID_KEYS) ?? fallbackId;
  const status = pickString(item, STATUS_KEYS) ?? "unknown";
  const label = pickString(item, LABEL_KEYS);
  const error = pickString(item, ERROR_KEYS);
  return { id, status, label: label === id ? undefined : label, error };
}

function extractSteps(root: Record<string, unknown>): NormalizedStep[] | null {
  for (const key of STEPS_KEYS) {
    const v = root[key];
    if (Array.isArray(v)) {
      const steps = v
        .map((item, i) => toStep(item, `#${i}`))
        .filter((s): s is NormalizedStep => s !== null);
      if (steps.length) return steps;
    }
    // Some encodings use a map of id -> step object rather than an array.
    if (isRecord(v)) {
      const steps = Object.entries(v)
        .map(([k, item]) => {
          if (typeof item === "string") return { id: k, status: item };
          return toStep(item, k);
        })
        .filter((s): s is NormalizedStep => s !== null)
        .map((s, i) => ({ ...s, id: s.id || `#${i}` }));
      if (steps.length) return steps;
    }
  }
  return null;
}

export function normalizeWorkflowRun(raw: unknown): NormalizedRun {
  let status: string | undefined;
  let steps: NormalizedStep[] | null = null;

  for (const root of roots(raw)) {
    status ??= pickString(root, STATUS_KEYS);
    steps ??= extractSteps(root);
    if (status && steps) break;
  }

  return {
    status: status ?? "unknown",
    steps: steps ?? [],
    matched: !!(status || steps),
  };
}

// ---- status vocabulary ----

const DONE = new Set(["done", "completed", "complete", "success", "succeeded", "ok", "finished"]);
const FAILED = new Set(["failed", "error", "errored", "timeout", "timed_out", "cancelled", "canceled", "aborted"]);
const RUNNING = new Set(["running", "active", "in_progress", "inprogress", "started", "executing"]);
const SKIPPED = new Set(["skipped", "skip"]);

const norm = (s: string) => s.toLowerCase().trim();

export const isDoneStatus = (s: string) => DONE.has(norm(s));
export const isFailedStatus = (s: string) => FAILED.has(norm(s));
export const isRunningStatus = (s: string) => RUNNING.has(norm(s));
export const isSkippedStatus = (s: string) => SKIPPED.has(norm(s));

/** A run is terminal once it is no longer progressing — stop polling. */
export function isTerminalRunStatus(s: string): boolean {
  const n = norm(s);
  return DONE.has(n) || FAILED.has(n);
}

export type StatusMeta = { color: string; label: string };

export function statusMeta(status: string): StatusMeta {
  const n = norm(status);
  if (isDoneStatus(n)) return { color: "success", label: "Xong" };
  if (isRunningStatus(n)) return { color: "processing", label: "Đang chạy" };
  if (isSkippedStatus(n)) return { color: "default", label: "Bỏ qua" };
  if (n === "timeout" || n === "timed_out") return { color: "warning", label: "Timeout" };
  if (n === "cancelled" || n === "canceled" || n === "aborted") return { color: "warning", label: "Đã huỷ" };
  if (isFailedStatus(n)) return { color: "error", label: "Lỗi" };
  if (n === "pending" || n === "queued" || n === "registered" || n === "waiting")
    return { color: "default", label: "Chờ" };
  if (n === "unknown") return { color: "default", label: "—" };
  return { color: "default", label: status };
}

// ---- grouping ----

const IMG_RE = /^(?:img|image)[_-]?(\d+)$/i;
const VID_RE = /^(?:vid|video)[_-]?(\d+)$/i;

const PLANNING_IDS = new Set(["parse", "refs", "bridge", "entities", "script", "plan"]);
const POST_IDS = new Set(["catchup", "audio", "download", "concat", "critic", "merge", "publish"]);

export type SceneRowGroup = {
  index: number;
  image?: NormalizedStep;
  video?: NormalizedStep;
};

export type GroupedRun = {
  planning: NormalizedStep[];
  scenes: SceneRowGroup[];
  post: NormalizedStep[];
};

export function groupSteps(steps: NormalizedStep[]): GroupedRun {
  const scenes = new Map<number, SceneRowGroup>();
  const planning: NormalizedStep[] = [];
  const post: NormalizedStep[] = [];

  const firstSceneAt = steps.findIndex((s) => IMG_RE.test(s.id) || VID_RE.test(s.id));

  steps.forEach((step, i) => {
    const img = IMG_RE.exec(step.id);
    const vid = VID_RE.exec(step.id);
    if (img || vid) {
      const idx = Number((img ?? vid)![1]);
      const row = scenes.get(idx) ?? { index: idx };
      if (img) row.image = step;
      else row.video = step;
      scenes.set(idx, row);
      return;
    }
    const id = step.id.toLowerCase();
    if (PLANNING_IDS.has(id)) planning.push(step);
    else if (POST_IDS.has(id)) post.push(step);
    // Unknown id: fall back to position relative to the first per-scene node.
    else if (firstSceneAt >= 0 && i < firstSceneAt) planning.push(step);
    else post.push(step);
  });

  return {
    planning,
    scenes: [...scenes.values()].sort((a, b) => a.index - b.index),
    post,
  };
}

export function countProgress(steps: NormalizedStep[]) {
  let done = 0;
  let running = 0;
  let failed = 0;
  for (const s of steps) {
    if (isDoneStatus(s.status) || isSkippedStatus(s.status)) done++;
    else if (isRunningStatus(s.status)) running++;
    else if (isFailedStatus(s.status)) failed++;
  }
  return { total: steps.length, done, running, failed };
}
