const BASE = import.meta.env.VITE_API_BASE ?? "";

export interface Project {
  id: number;
  name: string;
  video_mime: string;
  video_size: number;
  video_filename: string;
  has_char_image: boolean;
  style: string;
  model: string;
  char_description: string;
  custom_dialogue: string;
  bg_description: string;
  auto_magic: boolean;
  visual_similarity: number;
  created_at: string;
  updated_at: string;
  scene_count?: number;
  running?: boolean;
}

export interface Character {
  id: string;
  name: string;
  has_dialogue: boolean;
}

export interface Job {
  id: number;
  project_id: number;
  kind: string;
  status: "queued" | "processing" | "completed" | "failed" | "cancelled";
  scenes_added: number;
  error: string;
  total_scenes?: number;
}

export interface Scene {
  id: number;
  position: number;
  scene_id: string;
  json: Record<string, unknown>;
  job_id: number;
}

export interface Snapshot {
  id: number;
  project_id: number;
  reason: "analyze_start" | "analyze_regenerate" | "replace" | "restore";
  label: string;
  scene_count: number;
  created_at: string;
}

export interface Presets {
  styles: string[];
  models: { id: string; name: string }[];
  characters: { name: string; desc: string }[];
  backgrounds: { name: string; desc: string }[];
}

export interface CloneConfig {
  style?: string;
  model?: string;
  char_description?: string;
  custom_dialogue?: string;
  bg_description?: string;
  auto_magic?: boolean;
  visual_similarity?: number;
}

/** Unwraps the Rust side's `{"error": "..."}` convention into a thrown Error. */
async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, init);
  const text = await res.text();
  let body: unknown = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    if (!res.ok) throw new Error(text || `HTTP ${res.status}`);
    return text as unknown as T;
  }
  if (!res.ok) {
    const msg =
      (body as { error?: string })?.error ?? `HTTP ${res.status}`;
    throw new Error(msg);
  }
  return body as T;
}

export const api = {
  status: () => req<{ ok: boolean; has_api_key: boolean }>("/api/status"),

  presets: () => req<Presets>("/api/presets"),

  settings: () =>
    req<{ has_api_key: boolean; api_key_from_env: boolean; default_model: string }>(
      "/api/settings",
    ),

  saveSettings: (body: { gemini_api_key?: string; default_model?: string }) =>
    req("/api/settings", {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),

  listProjects: () => req<{ projects: Project[] }>("/api/projects"),

  createProject: (form: FormData) =>
    req<{ project: Project }>("/api/projects", { method: "POST", body: form }),

  getProject: (id: number) =>
    req<{
      project: Project;
      scene_count: number;
      characters: Character[];
      running: boolean;
      latest_job: Job | null;
    }>(`/api/projects/${id}`),

  deleteProject: (id: number) =>
    req(`/api/projects/${id}`, { method: "DELETE" }),

  updateConfig: (id: number, patch: CloneConfig & { name?: string }) =>
    req(`/api/projects/${id}/config`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(patch),
    }),

  uploadCharImage: (id: number, form: FormData) =>
    req(`/api/projects/${id}/char-image`, { method: "POST", body: form }),

  clearCharImage: (id: number) =>
    req(`/api/projects/${id}/char-image`, { method: "DELETE" }),

  scenes: (id: number) =>
    req<{ scenes: Scene[]; characters: Character[]; text: string }>(
      `/api/projects/${id}/scenes`,
    ),

  analyze: (id: number, body: CloneConfig & { mode: string }) =>
    req<{ job_id: number; status: string }>(`/api/projects/${id}/analyze`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),

  job: (id: number) => req<{ job: Job }>(`/api/jobs/${id}`),

  replace: (
    id: number,
    body: {
      find?: string;
      replace?: string;
      only_with_dialogue?: boolean;
      voice_overrides?: Record<string, string>;
    },
  ) =>
    req<{ replaced_text: boolean; voices_applied: number; characters: Character[] }>(
      `/api/projects/${id}/replace`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      },
    ),

  jobs: (id: number) => req<{ jobs: Job[] }>(`/api/projects/${id}/jobs`),

  jobRaw: (id: number) =>
    req<{ job_id: number; raw: string; chars: number }>(`/api/jobs/${id}/raw`),

  snapshots: (id: number) =>
    req<{ snapshots: Snapshot[] }>(`/api/projects/${id}/snapshots`),

  restore: (id: number, snapshotId: number) =>
    req<{ restored_scenes: number; undo_snapshot_id: number | null }>(
      `/api/projects/${id}/restore`,
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ snapshot_id: snapshotId }),
      },
    ),

  exportToFile: (id: number) =>
    req<{ dir: string; bundle: string; markdown: string; scene_count: number }>(
      `/api/projects/${id}/export/file`,
      { method: "POST" },
    ),

  exportToWiki: (id: number, path?: string) =>
    req<{ path: string; scene_count: number }>(`/api/projects/${id}/export/wiki`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ path: path ?? "" }),
    }),

  handoffVideoFlow: (
    id: number,
    body: { orientation?: string; translate?: boolean; dry_run?: boolean },
  ) =>
    req<{
      dry_run?: boolean;
      plan?: { scenes: unknown[]; entities: unknown[] };
      project_id?: string;
      video_id?: string;
      entities_created?: number;
      scenes_created?: number;
      translated_scenes?: number;
    }>(`/api/projects/${id}/handoff/video-flow`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }),

  bundleUrl: (id: number) => `${BASE}/api/projects/${id}/export/bundle?download=true`,

  markdownUrl: (id: number) => `${BASE}/api/projects/${id}/export/markdown?download=true`,

  videoUrl: (id: number) => `${BASE}/api/projects/${id}/video`,

  downloadUrl: (id: number) => `${BASE}/api/projects/${id}/export?download=true`,
};
