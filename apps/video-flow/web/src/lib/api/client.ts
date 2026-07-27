const base = () =>
  (import.meta.env.VITE_API_BASE ?? "").replace(/\/$/, "");

async function req<T>(
  path: string,
  init?: RequestInit
): Promise<T> {
  const url = `${base()}${path}`;
  const res = await fetch(url, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
  });
  const text = await res.text();
  if (!res.ok) {
    let detail = text;
    try {
      const j = JSON.parse(text) as { detail?: string };
      if (j.detail) detail = j.detail;
    } catch {
      /* ignore */
    }
    throw new Error(`${res.status}: ${detail}`);
  }
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

export type ProjectRow = Record<string, unknown>;
export type VideoRow = Record<string, unknown>;
export type SceneRow = Record<string, unknown>;
export type CharacterRow = Record<string, unknown>;
export type RequestRow = Record<string, unknown>;

export type BatchStatus = {
  total: number;
  pending: number;
  processing: number;
  completed: number;
  failed: number;
  done: boolean;
  all_succeeded: boolean;
  orientation?: string;
};

export type AIProviderInfo = {
  id: string;
  label: string;
  env_api_key: string;
  env_model: string;
  default_base_url?: string;
  notes?: string;
};

export type AISuggestEntity = {
  name: string;
  entity_type: string;
  description: string;
};

export type AISceneHint = {
  order: number;
  prompt: string;
  video_prompt?: string;
  camera_movement?: string;
  character_names?: string[];
};

export type AISuggestedProject = {
  name: string;
  story: string;
  material: string;
  language: string;
  entities: AISuggestEntity[];
  scene_hints: AISceneHint[];
};

export type AISuggestResponse = {
  suggestion: AISuggestedProject;
  provider: string;
};

export type AISuggestScenesResponse = {
  scene_hints: AISceneHint[];
  provider: string;
  /** Slug đã đọc từ DB (project) hoặc từ skill_slugs — đưa vào prompt LLM */
  skills_used?: string[];
};

export type AISuggestEntityItem = {
  name: string;
  entity_type: string;
  description: string;
};

export type AISuggestEntitiesResponse = {
  entities: AISuggestEntityItem[];
  provider: string;
};

export type CreativePlannedInsert = {
  temp_id: string;
  parent_scene_id: string;
  camera_angle_tag: string;
  edit_prompt: string;
  transition_from_prev: "smooth" | "hard_cut";
  chain?: { start?: string; end?: string };
  duration_sec: number;
  risk_flags?: string[];
};

export type CreativePlanResponse = {
  root_scene_id: string;
  summary: string;
  inserts: CreativePlannedInsert[];
  estimated_total_duration_sec: number;
};

export type CreativeApplyResponse = {
  root_scene_id: string;
  created_scenes?: Array<{ temp_id: string; scene_id: string }>;
  requests?: Array<{ type: string; request_id: string }>;
  timeline_preview?: Array<{ scene_id: string; label: string }>;
  chain_warnings?: Array<{ code: string; message: string; temp_id?: string }>;
};

export type AIProvidersResponse = {
  providers: AIProviderInfo[];
  library: string;
};

export type MaterialEntry = {
  id: string;
  name: string;
  style_instruction: string;
  negative_prompt?: string;
  scene_prefix?: string;
  lighting?: string;
  is_builtin?: boolean;
};

export type MaterialsResponse = {
  materials: MaterialEntry[];
};

export type MaterialImportResponse = {
  inserted: number;
  skipped: number;
  message: string;
};

/** One entry from the SenClaw daemon's /api/llm-config "available" list. */
export type LLMProfileInfo = {
  id?: string;
  label?: string;
  model?: string;
};

export type LLMSettingsResponse = {
  /** Always "senclaw" — LLM calls are delegated to the SenClaw daemon. */
  provider: string;
  /** Selected profile id ("" = daemon's currently active model). */
  profile: string;
  model: string;
  profiles: LLMProfileInfo[];
  /** Flow video model tier: "auto" | "lite" | "fast". */
  video_model?: string;
  /** The exact model key the extension last learned from Flow, if any. */
  video_model_learned?: string;
};

export type ToolSpec = {
  name: string;
  description: string;
  input_schema?: Record<string, unknown>;
};

export type ToolsSettingsResponse = {
  tools: ToolSpec[];
};

export type SkillCatalogEntry = {
  id: string;
  file: string;
  title: string;
  summary: string;
  size_bytes: number;
};

export type SkillsListResponse = {
  skills: SkillCatalogEntry[];
  skills_dir: string;
  skills_error?: string;
};

export type SkillSelectionResponse = {
  project_id: string;
  enabled_slugs: string[];
};

export type PipelineSkillCard = {
  skill_id: string;
  description: string;
};

export type PipelineSkillGroup = {
  id: string;
  title: string;
  cards: PipelineSkillCard[];
};

export type PipelineSkillGroupsResponse = {
  groups: PipelineSkillGroup[];
};

export type PipeSkillPromptRow = {
  id: string;
  slug: string;
  title: string;
  group_id: string;
  group_title: string;
  display_order: number;
  description: string;
  applies_to: string;
  prompt_template: string;
  is_active: boolean;
  version: number;
  created_at: string;
  updated_at: string;
};

export type PipeSkillsListResponse = {
  skills: PipeSkillPromptRow[];
};

export type PromptMigrateSummary = {
  export_dir: string;
  exported_files: number;
  imported_rows: number;
};

// ---- Agent log ----

export type AgentLogEntry = {
  pipeline_id: string;
  project_id: string;
  pipeline_status: string;
  task_id: string;
  task_label: string;
  agent_type: string;
  /** "active" | "registered" | "error" | "timeout" | "blocked" | "done" */
  status: string;
  started_at: string | null;
  completed_at: string | null;
  error_message?: string;
  /** label of the dependency that caused this task to be blocked */
  blocked_by?: string;
  /** raw JSON result from the agent (present for done/error tasks) */
  result?: string;
};

// ---- Multi-agent DAG pipeline (new backend) ----

export type AgentInfo = {
  type: string;
  name?: string;
  description?: string;
  soul_summary?: string;
  prompt?: string;
  /** Canonical markdown filename under backend/souls (built-in only). */
  soul_file?: string;
  /** @deprecated use skill_ids; kept for API compatibility */
  skill_id?: string;
  /** Catalog skills linked to this skill agent (empty = custom prompt only). */
  skill_ids?: string[];
  /** "built-in" | "skill" */
  kind?: string;
  enabled?: boolean;
};

export type DagTaskRow = {
  id: string;
  parent_id: string;
  label: string;
  agent_type: string;
  depends_on: string[];
  status: "registered" | "active" | "done" | "error" | "timeout";
  result: string | null;
  timeout_seconds: number;
  started_at: string | null;
  completed_at: string | null;
};

export type PipelineRow = {
  id: string;
  project_id: string;
  status: "queued" | "active" | "paused" | "done" | "failed";
  orientation: string;
  script_md: string;
  created_at: string;
  updated_at: string;
  tasks: DagTaskRow[];
};

// ---- SenClaw workflow engine (parallel per-scene pipeline) ----

export type WorkflowPipelineRequest = {
  project_id: string;
  video_id?: string;
  orientation?: string;
  with_audio?: boolean;
  with_critic?: boolean;
};

export type WorkflowStartResponse = {
  /** The workflow definition/run object as returned by the daemon. */
  workflow: WorkflowRunJson;
  run_id: string;
};

export type ProjectWorkflowRunResponse = {
  run_id: string | null;
  workflow?: WorkflowRunJson | null;
};

/**
 * The raw workflow-run JSON from SenClaw core. Its exact shape is not part of
 * this app's contract, so it stays opaque here — see `normalizeWorkflowRun`
 * in the Smart Pipeline page for the defensive projection we actually render.
 */
export type WorkflowRunJson = Record<string, unknown>;

export type ParsedSceneResult = {
  display_order: number;
  prompt: string;
  video_prompt: string;
  character_names: string[];
  duration: number;
  shot_type: string;
  camera_movement: string;
  narrator_text: string;
};

export type ParsedCharacterResult = {
  name: string;
  entity_type: string;
  description: string;
  image_prompt: string;
};

export type ParseScriptResult = {
  scenes: ParsedSceneResult[];
  characters: ParsedCharacterResult[];
};

export type ParseScriptResponse = ParseScriptResult & { provider?: string };

// ---- Skill catalog (new backend, read from disk) ----

export type SkillEntry = {
  id: string;
  name: string;
  description: string;
  body: string;
};

// ---- Skill agents (user-created, DB-backed) ----

export type SkillAgentEntry = {
  id: string;
  name: string;
  skill_id: string;
  skill_ids?: string[];
  prompt: string;
  enabled: boolean | number;
  created_at: string;
};

// ---- Bash process ----

// ---- Media ----

export type MediaRow = {
  id: string;
  file_name: string;
  file_path: string;
  mime_type: string;
  size_bytes: number;
  media_type: "image" | "audio" | "video" | "other";
  created_at: string;
  /** Kích thước ảnh/video (px), 0 nếu chưa đo được */
  width_px?: number;
  height_px?: number;
};

export type BashProcessRow = {
  id: string;
  command: string;
  cwd?: string;
  pid: number;
  status: "running" | "exited" | "killed";
  exit_code: number | null;
  started_at: string;
  finished_at: string | null;
  output_tail: string;
};

export type BashProcessListResponse = {
  processes: BashProcessRow[];
};

export const api = {
  health: () => req<Record<string, unknown>>("/health"),

  listBashProcesses: () =>
    req<BashProcessListResponse>("/api/processes"),

  getBashProcess: (id: string) =>
    req<BashProcessRow>(
      `/api/processes/${encodeURIComponent(id)}`
    ),

  startBashProcess: (body: { command: string; cwd?: string }) =>
    req<BashProcessRow>("/api/processes", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  killBashProcess: (id: string) =>
    req<{ ok: boolean }>(
      `/api/processes/${encodeURIComponent(id)}`,
      { method: "DELETE" }
    ),

  /** Ask the extension to load the Flow project page so media URLs get scraped,
   *  then pull whatever it found into local storage. */
  fetchMediaUrls: (project_id: string) =>
    req<{ downloaded: number; failed: number; scenes_still_without_url: number }>(
      "/api/media/fetch-urls",
      { method: "POST", body: JSON.stringify({ project_id }) }
    ),

  /** Download every still-remote asset of a project into local media. */
  localizeMedia: (project_id?: string) =>
    req<{ downloaded: number; skipped: number; failed: number }>("/api/media/localize", {
      method: "POST",
      body: JSON.stringify(project_id ? { project_id } : {}),
    }),

  listAIProviders: () => req<AIProvidersResponse>("/api/ai/providers"),

  suggestProject: (body: {
    prompt: string;
    provider?: string;
    api_key?: string;
    model?: string;
    base_url?: string;
  }) =>
    req<AISuggestResponse>("/api/ai/suggest-project", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  suggestScenes: (body: {
    prompt?: string;
    story?: string;
    characters_hint?: string;
    /** Khi có — backend nạp skill đã bật từ project_skill */
    project_id?: string;
    skill_slugs?: string[];
    pipe_skill_slugs?: string[];
    provider?: string;
    api_key?: string;
    model?: string;
    base_url?: string;
  }) =>
    req<AISuggestScenesResponse>("/api/ai/suggest-scenes", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  suggestEntities: (body: { story?: string; prompt?: string; project_id?: string; provider?: string }) =>
    req<AISuggestEntitiesResponse>("/api/ai/suggest-entities", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  listProjects: () => req<ProjectRow[]>("/api/projects"),

  createProject: (body: unknown) =>
    req<ProjectRow>("/api/projects", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  patchProject: (projectId: string, body: unknown) =>
    req<ProjectRow>(`/api/projects/${encodeURIComponent(projectId)}`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),

  updateProject: (projectId: string, body: unknown) =>
    req<ProjectRow>(`/api/projects/${encodeURIComponent(projectId)}`, {
      method: "PUT",
      body: JSON.stringify(body),
    }),

  duplicateProject: (projectId: string) =>
    req<ProjectRow>(
      `/api/projects/${encodeURIComponent(projectId)}/duplicate`,
      { method: "POST" }
    ),

  cloneProjectAI: (projectId: string, body?: { prompt?: string; provider?: string }) =>
    req<ProjectRow>(
      `/api/projects/${encodeURIComponent(projectId)}/clone-ai`,
      {
        method: "POST",
        body: JSON.stringify(body ?? {}),
      }
    ),

  getProject: (projectId: string) =>
    req<ProjectRow>(
      `/api/projects/${encodeURIComponent(projectId)}`
    ),

  deleteProject: (projectId: string) =>
    req<{ ok: boolean }>(
      `/api/projects/${encodeURIComponent(projectId)}`,
      { method: "DELETE" }
    ),

  listMaterials: () => req<MaterialsResponse>("/api/materials"),

  createMaterial: (body: {
    id: string;
    name: string;
    style_instruction: string;
    negative_prompt?: string;
    scene_prefix?: string;
    lighting?: string;
  }) =>
    req<MaterialEntry>("/api/materials", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  deleteMaterial: (materialId: string) =>
    req<{ ok: boolean }>(`/api/materials/${encodeURIComponent(materialId)}`, {
      method: "DELETE",
    }),

  importMaterialsByPath: (path: string) =>
    req<MaterialImportResponse>("/api/materials/import", {
      method: "POST",
      body: JSON.stringify({ path }),
    }),

  importMaterialsFromJSON: (jsonContent: string) =>
    req<MaterialImportResponse>("/api/materials/import", {
      method: "POST",
      body: jsonContent,
    }),

  getLLMSettings: () => req<LLMSettingsResponse>("/api/settings/llm"),
  getToolsSettings: () => req<ToolsSettingsResponse>("/api/settings/tools"),

  putLLMSettings: (body: { profile: string; video_model?: string }) =>
    req<LLMSettingsResponse>("/api/settings/llm", {
      method: "PUT",
      body: JSON.stringify(body),
    }),

  listSkills: () => req<SkillsListResponse>("/api/skills"),
  listPipelineSkillGroups: () =>
    req<PipelineSkillGroupsResponse>("/api/skills/pipeline-groups"),
  listPipeSkills: () =>
    req<PipeSkillsListResponse>("/api/pipe-skills"),
  createPipeSkill: (body: {
    id: string;
    slug: string;
    title: string;
    group_id?: string;
    group_title?: string;
    display_order?: number;
    description?: string;
    applies_to?: string;
    prompt_template: string;
    is_active?: boolean;
  }) =>
    req<PipeSkillPromptRow>("/api/pipe-skills", {
      method: "POST",
      body: JSON.stringify(body),
    }),
  patchPipeSkill: (
    slug: string,
    body: {
      title?: string;
      group_id?: string;
      group_title?: string;
      display_order?: number;
      description?: string;
      applies_to?: string;
      prompt_template?: string;
      is_active?: boolean;
    }
  ) =>
    req<PipeSkillPromptRow>(`/api/pipe-skills/${encodeURIComponent(slug)}`, {
      method: "PATCH",
      body: JSON.stringify(body),
    }),
  deletePipeSkill: (slug: string) =>
    req<{ ok: boolean; slug: string }>(
      `/api/pipe-skills/${encodeURIComponent(slug)}`,
      { method: "DELETE" }
    ),
  importPipeSkills: (body?: { path?: string }) =>
    req<{ path: string; imported_rows: number }>("/api/pipe-skills/import", {
      method: "POST",
      body: JSON.stringify(body ?? {}),
    }),
  importPipeSkillUpload: (body: { file_name?: string; content: string }) =>
    req<{ ok: boolean; file_name: string; imported_rows: number }>(
      "/api/pipe-skills/import-upload",
      {
        method: "POST",
        body: JSON.stringify(body),
      }
    ),
  migratePipeSkills: (body?: { export_dir?: string; import_db?: boolean }) =>
    req<PromptMigrateSummary>("/api/pipe-skills/migrate", {
      method: "POST",
      body: JSON.stringify(body ?? {}),
    }),

  getSkillSelection: (projectId: string) =>
    req<SkillSelectionResponse>(
      `/api/projects/${encodeURIComponent(projectId)}/skill-selection`
    ),

  putSkillSelection: (projectId: string, enabledSlugs: string[]) =>
    req<SkillSelectionResponse>(
      `/api/projects/${encodeURIComponent(projectId)}/skill-selection`,
      {
        method: "PUT",
        body: JSON.stringify({ enabled_slugs: enabledSlugs }),
      }
    ),

  getProjectPipeSkills: (projectId: string) =>
    req<SkillSelectionResponse>(
      `/api/projects/${encodeURIComponent(projectId)}/pipe-skills`
    ),

  putProjectPipeSkills: (projectId: string, enabledSlugs: string[]) =>
    req<SkillSelectionResponse>(
      `/api/projects/${encodeURIComponent(projectId)}/pipe-skills`,
      {
        method: "PUT",
        body: JSON.stringify({ enabled_slugs: enabledSlugs }),
      }
    ),

  listProjectCharacters: (projectId: string) =>
    req<CharacterRow[]>(`/api/projects/${encodeURIComponent(projectId)}/characters`),

  patchProjectCharacter: (
    projectId: string,
    characterId: string,
    body: {
      name?: string;
      entity_type?: string;
      description?: string;
      image_prompt?: string;
      voice_description?: string;
      reference_image_url?: string;
      media_id?: string;
    }
  ) =>
    req<CharacterRow>(
      `/api/projects/${encodeURIComponent(projectId)}/characters/${encodeURIComponent(characterId)}`,
      {
        method: "PATCH",
        body: JSON.stringify(body),
      }
    ),

  createProjectCharacter: (
    projectId: string,
    body: {
      name: string;
      entity_type?: string;
      description?: string | null;
      voice_description?: string | null;
    }
  ) =>
    req<CharacterRow>(`/api/projects/${encodeURIComponent(projectId)}/characters`, {
      method: "POST",
      body: JSON.stringify(body),
    }),

  unlinkProjectCharacter: (
    projectId: string,
    characterId: string,
    deleteRow?: boolean
  ) => {
    const q =
      deleteRow === true
        ? "?delete_row=1"
        : "";
    return req<{
      ok: boolean;
      character_deleted?: boolean;
      still_linked_to_other_projects?: boolean;
    }>(
      `/api/projects/${encodeURIComponent(projectId)}/characters/${encodeURIComponent(characterId)}${q}`,
      { method: "DELETE" }
    );
  },

  createVideo: (body: unknown) =>
    req<VideoRow>("/api/videos", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  patchVideo: (videoId: string, body: Record<string, unknown>) =>
    req<VideoRow>(
      `/api/videos/${encodeURIComponent(videoId)}`,
      {
        method: "PATCH",
        body: JSON.stringify(body),
      }
    ),

  listVideos: (projectId: string) =>
    req<VideoRow[]>(
      `/api/videos?project_id=${encodeURIComponent(projectId)}`
    ),

  listScenes: (videoId: string) =>
    req<SceneRow[]>(
      `/api/scenes?video_id=${encodeURIComponent(videoId)}`
    ),

  createScene: (body: unknown) =>
    req<SceneRow>("/api/scenes", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  planCreativeBreakdown: (
    sceneId: string,
    body?: {
      style?: string;
      max_inserts?: number;
      pacing?: "slow" | "medium" | "fast";
      allow_branching?: boolean;
      character_ids?: string[];
      constraints?: {
        prefer_identity_safe_closeups?: boolean;
        max_total_extra_seconds?: number;
      };
    }
  ) =>
    req<CreativePlanResponse>(
      `/api/scenes/${encodeURIComponent(sceneId)}/creative-breakdown/plan`,
      {
        method: "POST",
        body: JSON.stringify(body ?? {}),
      }
    ),

  applyCreativeBreakdown: (
    sceneId: string,
    body: {
      plan: {
        root_scene_id?: string;
        inserts: CreativePlannedInsert[];
      };
      execution?: {
        create_scenes?: boolean;
        generate_images?: boolean;
        generate_videos?: boolean;
        auto_fix_closeup_anchor?: boolean;
      };
    }
  ) =>
    req<CreativeApplyResponse>(
      `/api/scenes/${encodeURIComponent(sceneId)}/creative-breakdown/apply`,
      {
        method: "POST",
        body: JSON.stringify(body),
      }
    ),

  patchScene: (sceneId: string, body: Record<string, unknown>) =>
    req<SceneRow>(
      `/api/scenes/${encodeURIComponent(sceneId)}`,
      {
        method: "PATCH",
        body: JSON.stringify(body),
      }
    ),

  deleteScene: (sceneId: string) =>
    req<{ ok: boolean }>(
      `/api/scenes/${encodeURIComponent(sceneId)}`,
      { method: "DELETE" }
    ),

  createRequestBatch: (requests: unknown[]) =>
    req<RequestRow[]>("/api/requests/batch", {
      method: "POST",
      body: JSON.stringify({ requests }),
    }),

  batchStatus: (q: {
    video_id?: string;
    project_id?: string;
    type?: string;
    orientation?: string;
  }) => {
    const p = new URLSearchParams();
    if (q.video_id) p.set("video_id", q.video_id);
    if (q.project_id) p.set("project_id", q.project_id);
    if (q.type) p.set("type", q.type);
    if (q.orientation) p.set("orientation", q.orientation);
    return req<BatchStatus>(`/api/requests/batch-status?${p}`);
  },

  listRequests: (q: {
    video_id?: string;
    project_id?: string;
    scene_id?: string;
    status?: string;
    /** Lọc theo cột request.type (GENERATE_CHARACTER_IMAGE, …) */
    type?: string;
    character_id?: string;
  }) => {
    const p = new URLSearchParams();
    if (q.video_id) p.set("video_id", q.video_id);
    if (q.project_id) p.set("project_id", q.project_id);
    if (q.scene_id) p.set("scene_id", q.scene_id);
    if (q.status) p.set("status", q.status);
    if (q.type) p.set("type", q.type);
    if (q.character_id) p.set("character_id", q.character_id);
    return req<RequestRow[]>(`/api/requests?${p}`);
  },

  /** Toàn bộ job PENDING trong DB — dùng để thấy hàng đợi chờ worker. */
  listPendingRequests: () =>
    req<RequestRow[]>("/api/requests/pending"),

  deleteRequest: (id: string) =>
    req<{ status: string }>(`/api/requests/${encodeURIComponent(id)}`, { method: "DELETE" }),

  clearRequests: (videoId?: string) => {
    const p = videoId ? `?video_id=${encodeURIComponent(videoId)}` : "";
    return req<{ status: string }>(`/api/requests${p}`, { method: "DELETE" });
  },

  // ---- Multi-agent DAG pipeline ----

  listAgents: () => req<AgentInfo[]>("/api/agents"),

  listAgentLog: () => req<AgentLogEntry[]>("/api/agents/log"),

  listAgentHistory: () => req<AgentLogEntry[]>("/api/agents/history"),

  clearAgentHistory: () => req<{ ok: string }>("/api/agents/history", { method: "DELETE" }),

  putAgentSoul: (agentType: string, prompt: string) =>
    req<{ ok: string }>(`/api/agents/${encodeURIComponent(agentType)}/soul`, {
      method: "PUT",
      body: JSON.stringify({ prompt }),
    }),

  patchBuiltinAgent: (agentType: string, enabled: boolean) =>
    req<{ type: string; enabled: boolean }>(`/api/agents/${encodeURIComponent(agentType)}`, {
      method: "PATCH",
      body: JSON.stringify({ enabled }),
    }),

  parseScript: (body: { script: string; provider?: string }) =>
    req<ParseScriptResponse>("/api/script/parse", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  createPipeline: (body: {
    project_id: string;
    script: string;
    orientation?: string;
    pipeline_mode?: "production" | "full";
    goal?: string;
  }) =>
    req<PipelineRow>("/api/pipeline/create", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  getPipeline: (id: string) =>
    req<PipelineRow>(`/api/pipeline/${encodeURIComponent(id)}`),

  startPipeline: (id: string) =>
    req<{ ok: boolean }>(`/api/pipeline/${encodeURIComponent(id)}/start`, {
      method: "POST",
    }),

  pausePipeline: (id: string) =>
    req<{ ok: boolean }>(`/api/pipeline/${encodeURIComponent(id)}/pause`, {
      method: "POST",
    }),

  cancelPipeline: (id: string) =>
    req<{ ok: boolean }>(`/api/pipeline/${encodeURIComponent(id)}/cancel`, {
      method: "POST",
    }),

  deletePipeline: (id: string, projectId: string) =>
    req<void>(`/api/pipeline/${encodeURIComponent(id)}?project_id=${encodeURIComponent(projectId)}`, {
      method: "DELETE",
    }),

  retryTask: (pipelineId: string, taskId: string) =>
    req<{ ok: boolean }>(
      `/api/pipeline/${encodeURIComponent(pipelineId)}/task/${encodeURIComponent(taskId)}/retry`,
      { method: "POST" }
    ),

  stopTask: (pipelineId: string, taskId: string) =>
    req<{ ok: boolean }>(
      `/api/pipeline/${encodeURIComponent(pipelineId)}/task/${encodeURIComponent(taskId)}/stop`,
      { method: "POST" }
    ),

  listProjectPipelines: (projectId: string) =>
    req<Array<{ id: string; status: string; orientation: string; created_at: string; updated_at: string }>>(
      `/api/pipeline/project/${encodeURIComponent(projectId)}`
    ),

  // ---- SenClaw workflow engine (parallel per-scene pipeline) ----

  startWorkflowPipeline: (body: WorkflowPipelineRequest) =>
    req<WorkflowStartResponse>("/api/pipeline/workflow", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  /** Run a user-assembled workflow: ordered stages of agents (parallel within,
   *  sequential across). */
  startCustomWorkflow: (body: {
    project_id: string;
    video_id?: string;
    orientation?: "VERTICAL" | "HORIZONTAL";
    stages: string[][];
  }) =>
    req<WorkflowStartResponse>("/api/pipeline/custom-workflow", {
      method: "POST",
      body: JSON.stringify(body),
    }),

  getWorkflowRun: (runId: string) =>
    req<WorkflowRunJson>(`/api/pipeline/workflow/${encodeURIComponent(runId)}`),

  cancelWorkflowRun: (runId: string) =>
    req<void>(`/api/pipeline/workflow/${encodeURIComponent(runId)}/cancel`, {
      method: "POST",
    }),

  getProjectWorkflowRun: (projectId: string) =>
    req<ProjectWorkflowRunResponse>(
      `/api/pipeline/workflow/project/${encodeURIComponent(projectId)}`
    ),

  // ---- Skill catalog (reads markdown files from disk) ----

  listSkillCatalog: () => req<SkillEntry[]>("/api/skills"),

  // ---- Skill agents (user-created, DB-backed) ----

  listSkillAgents: () => req<SkillAgentEntry[]>("/api/skill-agents"),

  createSkillAgent: (body: {
    id?: string;
    name: string;
    skill_ids?: string[];
    skill_id?: string;
    prompt: string;
  }) =>
    req<{ id: string }>("/api/skill-agents", { method: "POST", body: JSON.stringify(body) }),

  updateSkillAgent: (
    id: string,
    body: { name?: string; prompt?: string; enabled?: boolean; skill_ids?: string[] }
  ) =>
    req<{ ok: string }>(`/api/skill-agents/${encodeURIComponent(id)}`, {
      method: "PUT",
      body: JSON.stringify(body),
    }),

  deleteSkillAgent: (id: string) =>
    req<void>(`/api/skill-agents/${encodeURIComponent(id)}`, { method: "DELETE" }),

  // ---- Media ----

  listMedia: (filters?: {
    type?: string;
    search?: string;
    projectId?: string;
    /** portrait | landscape | square | unknown */
    orientation?: string;
  }) => {
    const params = new URLSearchParams();
    if (filters?.type) params.set("type", filters.type);
    if (filters?.search) params.set("search", filters.search);
    if (filters?.projectId) params.set("project_id", filters.projectId);
    if (filters?.orientation) params.set("orientation", filters.orientation);
    const q = params.toString();
    return req<MediaRow[]>(`/api/media${q ? `?${q}` : ""}`);
  },

  getMedia: (id: string) =>
    req<MediaRow>(`/api/media/${encodeURIComponent(id)}`),

  uploadMedia: (file: File): Promise<MediaRow> => {
    const form = new FormData();
    form.append("file", file);
    return fetch(`${(import.meta.env.VITE_API_BASE ?? "").replace(/\/$/, "")}/api/media/upload`, {
      method: "POST",
      body: form,
    }).then(async (res) => {
      const text = await res.text();
      if (!res.ok) throw new Error(`${res.status}: ${text}`);
      return JSON.parse(text) as MediaRow;
    });
  },

  mediaFileUrl: (id: string) =>
    `${(import.meta.env.VITE_API_BASE ?? "").replace(/\/$/, "")}/api/media/${encodeURIComponent(id)}/file`,

  deleteMedia: (id: string) =>
    req<{ ok: boolean }>(`/api/media/${encodeURIComponent(id)}`, { method: "DELETE" }),

  /** Xóa nhiều bản ghi media (file trên đĩa + row DB). */
  deleteMediaBatch: (ids: string[]) =>
    req<{ deleted: number; requested: number; missing_ids: string[] }>(`/api/media/batch-delete`, {
      method: "POST",
      body: JSON.stringify({ ids }),
    }),
};
