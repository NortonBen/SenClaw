import type { FlowAction } from "../../types/api";

export function branchSummary(step: FlowAction): string {
  const cfg = step.config ?? {};
  const parts: string[] = [];
  if (cfg._next_on_success) parts.push(`ok->S${cfg._next_on_success}`);
  if (cfg._next_on_error) parts.push(`err->S${cfg._next_on_error}`);
  return parts.join(" | ");
}

