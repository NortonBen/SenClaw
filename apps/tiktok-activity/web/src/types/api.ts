export type ActionType =
  | "open_home"
  | "open_url"
  | "wait_page_ready"
  | "search"
  | "watch_video"
  | "like_video"
  | "comment_video"
  | "share_video"
  | "reply_comment"
  | "check_login"
  | "if_condition"
  | "login"
  | "get_info_post"
  | "get_comments_in_page"
  | "reply_comment_ai"
  | "ai_gent_comment"
  | "ai_playwright_agent"
  | "follow_user"
  | "random_delay"
  | "random_yes_no"
  | "next_video_post"
  | "loop_repeat"
  | "loop_if"
  | "check_scroll_end"
  | "run_next_flow"
  | "set_params"
  | "record_post_interaction"
  | "record_friend_event"
  | "account_meta"
  | "start"
  | "log"
  | "notification"
  | "playwright_atomics";

/** Một bước nhỏ Playwright trong action playwright_atomics */
export interface FlowAtomic {
  id?: string;
  /** Optional display label (ignored by engine). */
  name?: string;
  kind: string;
  params?: Record<string, string>;
}

export interface ManagedProxy {
  id: string;
  name: string;
  url: string;
  notes: string;
  createdAt?: string;
}

export interface BrowserProfile {
  id: string;
  name: string;
  userDataDir: string;
  userAgent: string;
  viewportWidth: number;
  viewportHeight: number;
  locale: string;
  timezoneId: string;
  accountId: string;
  notes: string;
  createdAt?: string;
  updatedAt?: string;
}

export interface TikTokAccount {
  id: string;
  username: string;
  password: string;
  proxy: string;
  profilePath: string;
  userAgent: string;
  proxyId?: string;
  browserProfileId?: string;
  createdAt?: string;
}

export interface FlowAction {
  id: string;
  type: ActionType;
  name: string;
  config: Record<string, string>;
  timeoutSeconds: number;
  /** Tham số tùy chọn của step (playwright_atomics: bind từ atomic fill/goto) */
  params?: Record<string, string>;
  /** Chuỗi atomic khi type === playwright_atomics */
  atomics?: FlowAtomic[];
}

export interface Flow {
  id: string;
  name: string;
  params?: Record<string, string>;
  actions: FlowAction[];
  updatedAt?: string;
}

/** Step playwright_atomics lưu từ trang builder (SQLite). */
export interface SavedFlowAction {
  id: string;
  name: string;
  step: FlowAction;
  updatedAt?: string;
}

/** Một bước do POST /api/flows/generate-ai trả về (map paletteId → step thật ở client). */
export interface FlowGenerateAIStep {
  paletteId: string;
  name?: string;
  config?: Record<string, string>;
  timeoutSeconds?: number;
  params?: Record<string, string>;
  atomics?: FlowAtomic[];
}

export interface FlowGenerateAIResponse {
  name: string;
  params: Record<string, string>;
  actions: FlowGenerateAIStep[];
}

export type RunStatus = "queued" | "running" | "done" | "failed";

export interface FlowRun {
  id: string;
  accountId: string;
  flowId: string;
  scheduleId?: string;
  status: RunStatus;
  /** Danh sách run từ GET /api/runs có thể rỗng (logs chỉ đầy đủ qua GET /api/runs/:id). */
  logs?: string[];
  startedAt?: string;
  endedAt?: string;
}

/** GET danh sách phân trang: `/api/runs`, `/api/accounts`, `/api/proxies`, `/api/browser-profiles` */
export interface PagedList<T> {
  items: T[];
  total: number;
}

/** GET /api/runs?page=&pageSize=&q= */
export type RunsListResponse = PagedList<FlowRun>;

/** Server cho phép pageSize tối đa 500 cho account / proxy / profile (dropdown). */
export const ENTITY_LIST_MAX_PAGE_SIZE = 500;

export function pagedEntityQuery(page: number, pageSize: number, q?: string): string {
  const p = new URLSearchParams({ page: String(page), pageSize: String(pageSize) });
  const s = q?.trim();
  if (s) p.set("q", s);
  return p.toString();
}

/** GET /api/dashboard/run-stats */
export interface DailyRunCount {
  date: string;
  done: number;
  failed: number;
  running: number;
  queued: number;
  total: number;
}

export interface FlowRunRank {
  flowId: string;
  count: number;
}

export interface DashboardRunStats {
  last7Days: DailyRunCount[];
  statusTotals7d: Record<string, number>;
  topFlows7d: FlowRunRank[];
}

export type ScheduleType = "run_now" | "daily_at" | "once_at";

export interface Schedule {
  id: string;
  name: string;
  enabled: boolean;
  flowId: string;
  params?: Record<string, string>;
  allAccounts: boolean;
  accountIds: string[];
  type: ScheduleType;
  dailyAt: string;
  onceAt: string;
  timezoneId: string;
  lastRunAt?: string;
  nextRunAt?: string;
  createdAt?: string;
  updatedAt?: string;
}

/** engine: handler Go cố định. atomics: step playwright_atomics — hành vi chỉnh bằng chuỗi atomic (không sửa code engine). */
export type ActionImplementationKind = "engine" | "atomics";

export interface PaletteAction {
  /** Khóa duy nhất trong palette (nhiều dòng có thể cùng `type`, vd preset playwright_atomics). */
  paletteId: string;
  type: ActionType;
  name: string;
  implementation: ActionImplementationKind;
  /** Kéo vào canvas: khởi tạo sẵn chuỗi atomic (chỉ áp dụng khi `type === playwright_atomics`). */
  presetAtomics?: FlowAtomic[];
  /** Step `/api/saved-flow-actions` (paletteId dạng `saved_sfa_*`). */
  savedStepTemplate?: FlowAction;
}
