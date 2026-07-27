const base = () => (import.meta.env.VITE_API_BASE ?? "").replace(/\/$/, "");

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${base()}${path}`, {
    ...init,
    headers: { "Content-Type": "application/json", ...(init?.headers ?? {}) },
  });
  const text = await res.text();
  if (!res.ok) {
    let detail = text;
    try {
      // The Rust side answers `{"error": "..."}` on every failure path.
      const j = JSON.parse(text) as { error?: string };
      if (j.error) detail = j.error;
    } catch {
      /* keep the raw body */
    }
    throw new Error(detail || `HTTP ${res.status}`);
  }
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

export type ProcessStatus =
  | "queued"
  | "processing"
  | "completed"
  | "failed"
  | "cancelled";

export interface StorySummary {
  id: number;
  name: string;
  parent_story_id: number | null;
  version_number: number;
  original_length: number;
  source_type: "human" | "ai";
  created_at: string;
  preview: string;
  version_count: number;
}

export interface Story extends Omit<StorySummary, "preview" | "version_count"> {
  /** A **window** of the text, not the whole novel — see `offset`/`has_more`. */
  original_text: string;
  total_length: number;
  offset: number;
  has_more: boolean;
  creativity_ratio: number | null;
  target_length_variance: number | null;
  processing_time: number | null;
}

export interface RewriteProcess {
  id: number;
  story_id: number;
  status: ProcessStatus;
  current_stage: string;
  progress_percentage: number;
  total_chunks: number;
  current_chunk: number;
  error_message: string | null;
  creativity_ratio: number;
  target_length_variance: number;
  user_prompt: string | null;
  version_plan: string | null;
  result_story_id: number | null;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
}

export interface ChunkPreview {
  persisted: boolean;
  total: number;
  chunks: { chunk_index: number; length: number; preview: string }[];
}

export interface StartRewriteReq {
  story_id: number;
  version_plan?: string;
  user_prompt?: string;
  creativity_ratio?: number;
  target_length_variance?: number;
}

export const api = {
  health: () => req<Record<string, unknown>>("/api/status"),

  listStories: () => req<StorySummary[]>("/api/stories"),
  getStory: (id: number, offset = 0, limit = 20_000) =>
    req<Story>(`/api/stories/${id}?offset=${offset}&limit=${limit}`),
  /** Returns metadata only — the server does not echo the uploaded text back. */
  createStory: (name: string, text: string) =>
    req<StorySummary>("/api/stories", {
      method: "POST",
      body: JSON.stringify({ name, text }),
    }),
  deleteStory: (id: number) =>
    req<void>(`/api/stories/${id}`, { method: "DELETE" }),
  listVersions: (id: number) => req<StorySummary[]>(`/api/stories/${id}/versions`),
  storyChunks: (id: number) => req<ChunkPreview>(`/api/stories/${id}/chunks`),

  listProcesses: (status?: string) =>
    req<RewriteProcess[]>(`/api/processes${status ? `?status=${status}` : ""}`),
  getProcess: (id: number) => req<RewriteProcess>(`/api/processes/${id}`),
  startRewrite: (body: StartRewriteReq) =>
    req<RewriteProcess>("/api/processes", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  cancelProcess: (id: number) =>
    req<void>(`/api/processes/${id}/cancel`, { method: "PUT" }),
  retryProcess: (id: number) =>
    req<{ process: RewriteProcess; resuming_from_chunk: number }>(
      `/api/processes/${id}/retry`,
      { method: "POST" }
    ),
  deleteProcess: (id: number) =>
    req<void>(`/api/processes/${id}`, { method: "DELETE" }),

  getSettings: () => req<Record<string, string>>("/api/settings"),
  putSettings: (patch: Record<string, string>) =>
    req<Record<string, string>>("/api/settings", {
      method: "PUT",
      body: JSON.stringify(patch),
    }),
};
