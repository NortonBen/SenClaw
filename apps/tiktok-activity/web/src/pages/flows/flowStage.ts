import type { FlowAction } from "../../types/api";

/** Stage hiển thị / sắp xếp step (từ config._stage). */
export function getActionStage(s: FlowAction, fallback: number): number {
  const raw = s.config?._stage;
  if (!raw) return fallback;
  const n = Number(raw);
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : fallback;
}

/** Gán lại _stage theo thứ tự và giới hạn song song mỗi stage. */
export function normalizeFlowStages(input: FlowAction[], maxParallelPerStage: number): FlowAction[] {
  if (input.length === 0) return input;
  const sorted = [...input].sort((a, b) => {
    const sa = getActionStage(a, 1);
    const sb = getActionStage(b, 1);
    if (sa !== sb) return sa - sb;
    return 0;
  });
  const out: FlowAction[] = [];
  let stage = 1;
  let inStage = 0;
  for (const s of sorted) {
    if (inStage >= maxParallelPerStage) {
      stage += 1;
      inStage = 0;
    }
    out.push({ ...s, config: { ...(s.config ?? {}), _stage: String(stage) } });
    inStage += 1;
  }
  return out;
}
