export interface GroupInfo {
  jid: string;
  folder: string;
  name: string;
  channel: string;
  groupType: string;
  requiresTrigger: boolean;
  allowedTools: string[] | null;
  allowedPaths: string[] | null;
  allowedWorkDirs: string[] | null;
  maxMessages: number | null;
  /** Per-group LLM override (id of an entry in the global LLM config list). */
  modelId?: string | null;
  agentId?: number;
  channelId?: number;
  /** Last chat message/agent response time (ms since epoch), from the server. */
  lastActivity?: number | null;
}

export interface RegisterGroupPayload {
  jid?: string;  // Feishu pending: omit; backend may assign feishu:pending:{appId}
  folder: string;
  name: string;
  channel?: 'telegram' | 'feishu' | 'whatsapp' | 'qq' | 'app';
  groupType?: string;
  requiresTrigger?: boolean;
  allowedTools?: string[] | null;
  allowedPaths?: string[] | null;
  allowedWorkDirs?: string[] | null;
  /** Per-group LLM override (id of an entry in the global LLM config list). */
  modelId?: string | null;
}

export interface UpdateGroupPayload {
  name?: string;
  channel?: string;
  groupType?: string;
  requiresTrigger?: boolean;
  allowedTools?: string[] | null;
  allowedPaths?: string[] | null;
  allowedWorkDirs?: string[] | null;
  maxMessages?: number | null;
  /** Per-group LLM override; null/'' clears it (→ global active model). */
  modelId?: string | null;
}

// ===== Message types =====

export interface ImageAttachment {
  dataUrl: string;
  mimeType: string;
}

export interface TextMessage {
  id: string;
  role: 'user' | 'agent' | 'other';
  senderName?: string;
  text: string;
  attachments?: ImageAttachment[];
  timestamp: string;
  /** Output (completion) tokens this agent message cost. Shown per-message. */
  tokens?: number;
}

export interface PermissionMessage {
  id: string;
  role: 'permission';
  requestId: string;
  toolName: string;
  title: string;
  content: string;
  options: { key: string; label: string }[];
  /** Set when resolved: which option was chosen */
  resolved?: { key: string; label: string };
  timestamp: string;
}

export interface QuestionItem {
  question: string;
  header: string;
  options: { label: string; description?: string }[];
  multiSelect?: boolean;
}

export interface QuestionMessage {
  id: string;
  role: 'question';
  requestId: string;
  agentId: string;
  questions: QuestionItem[];
  /** qi → oi (single) or oi[] (multi), filled as user selects. -1 = Other */
  selections: Record<number, number | number[]>;
  /** qi → user-typed text for "Other" option */
  otherTexts?: Record<number, string>;
  resolved: boolean;
  timestamp: string;
}

/**
 * One tool invocation rendered inline in the chat.
 *
 * The ChatView aggregates **consecutive** ToolMessages from the same agent
 * turn into a single collapsible card ("Read 2 files, edited 1 file, ran 1
 * command ›"). Each row inside the card maps back to one ToolMessage.
 */
export interface ToolMessage {
  id: string;
  role: 'tool';
  agentId: string;
  /** Internal tool id like `Bash`, `Read`, `mcp__browser__search`. */
  toolName: string;
  /** Display title (e.g. file path, search query, URL). */
  title: string;
  /** One-line summary (e.g. "+12 -3 lines", "8 results"). */
  summary: string;
  /** Raw structured payload for the expanded detail view. */
  content: unknown;
  /** `false` when the call errored — UI shows a red badge. */
  ok: boolean;
  /** Optional milliseconds the tool took (claude-code parity). */
  duration?: number;
  /** Optional structured error message for failed calls. */
  errorMessage?: string;
  timestamp: string;
}

// ===== FormUI =====

export interface FormFieldOption {
  label: string;
  value: string;
}

export interface FormTableColumn {
  key: string;
  label: string;
  type?: 'text' | 'number';
}

interface FormFieldBase {
  key: string;
  label: string;
  required?: boolean;
  help?: string;
}

/** Closed widget catalog — TS mirror of `zen_core::FormField` in Rust. */
export type FormFieldDef =
  | (FormFieldBase & { type: 'text'; placeholder?: string; maxLength?: number; default?: string })
  | (FormFieldBase & { type: 'textarea'; placeholder?: string; maxLength?: number; rows?: number; default?: string })
  | (FormFieldBase & { type: 'number'; min?: number; max?: number; step?: number; default?: number })
  | (FormFieldBase & { type: 'slider'; min: number; max: number; step?: number; default?: number })
  | (FormFieldBase & { type: 'select'; options: FormFieldOption[]; default?: string })
  | (FormFieldBase & { type: 'radio'; options: FormFieldOption[]; default?: string })
  | (FormFieldBase & { type: 'multiselect'; options: FormFieldOption[]; default?: string[] })
  | (FormFieldBase & { type: 'checkbox'; default?: boolean })
  | (FormFieldBase & { type: 'date'; min?: string; max?: string; default?: string })
  | { type: 'static_text'; text: string; variant?: 'heading' | 'body' | 'divider' }
  | (FormFieldBase & { type: 'editable_table'; columns: FormTableColumn[]; rows?: Record<string, string | number>[]; allowAddRow?: boolean });

export interface FormMessage {
  id: string;
  role: 'form';
  requestId: string;
  agentId: string;
  title: string;
  surface: 'inline' | 'dock';
  submitLabel: string;
  fields: FormFieldDef[];
  /** Initial values seeded from each field's `default` on arrival. */
  values: Record<string, unknown>;
  resolved: boolean;
  timestamp: string;
}

// ===== Chat Widgets (one-way inline rich cards) =====

export type WidgetKind = 'chart' | 'image' | 'clock' | 'weather' | 'video' | 'audio' | 'app';

export type ChartType = 'bar' | 'line' | 'area' | 'pie' | 'scatter';

export interface ChartPoint {
  x: string | number;
  y: number;
}

export interface ChartSeries {
  name: string;
  /** Optional explicit color; client palette used when absent. */
  color?: string;
  points: ChartPoint[];
}

export interface ChartData {
  /** Absent → 'bar' (renderer default, matches the daemon normalizer). */
  chartType?: ChartType;
  /** Canonical form; alternatively pass `rows` or `labels`+`values`. */
  series?: ChartSeries[];
  /** Tabular shortcut: one object per x, every numeric column = a series. */
  rows?: Array<Record<string, unknown>>;
  /** Names the x column of `rows` (else auto-detected). */
  x?: string;
  /** With `values`: a single-series shortcut. */
  labels?: Array<string | number>;
  values?: Array<number | string>;
  /** Series name for the labels/values shortcut. */
  name?: string;
  xLabel?: string;
  yLabel?: string;
  /** bar/area only */
  stacked?: boolean;
}

export interface ImageData {
  url?: string;
  dataUrl?: string;
  caption?: string;
  alt?: string;
}

export interface VideoData {
  /** http(s) URL the client can fetch; local filesystem paths won't play. */
  url: string;
  /** Poster frame shown before playback starts. */
  poster?: string;
  caption?: string;
  /** Explicit MIME (e.g. `video/mp4`) when the URL has no useful extension. */
  mime?: string;
  autoplay?: boolean;
}

export interface ClockData {
  /** IANA tz; defaults to local when absent. */
  tz?: string;
  label?: string;
  showSeconds?: boolean;
  showDate?: boolean;
  format24h?: boolean;
}

export type WeatherIcon =
  | 'sunny'
  | 'partly_cloudy'
  | 'cloudy'
  | 'rain'
  | 'thunderstorm'
  | 'snow'
  | 'fog'
  | 'wind';

export interface WeatherCurrent {
  temp: number;
  condition: string;
  icon: WeatherIcon;
  humidity?: number;
  wind?: number;
}

export interface WeatherDay {
  day: string;
  hi: number;
  lo: number;
  icon: WeatherIcon;
}

export interface WeatherData {
  location: string;
  unit?: 'C' | 'F';
  current: WeatherCurrent;
  daily?: WeatherDay[];
}

export interface AudioData {
  url: string;
  caption?: string;
  mime?: string;
}

/** kind `app` — a Space-App / plugin widget resolved by the daemon registry. */
export interface AppWidgetData {
  /** App id (deep link target: /space/app/<app>). */
  app: string;
  /** Short widget id within the app. */
  widget: string;
  /** Full registry id, e.g. "crm.pipeline". */
  id: string;
  params?: Record<string, unknown>;
  /** Resolved iframe entry (absolute origin or daemon-proxy path) + params qs. */
  entry?: string;
  size?: 'small' | 'medium' | 'large' | 'tall' | string;
  refreshMs?: number;
  textFallback?: string;
}

export interface WidgetSpec {
  kind: WidgetKind;
  title?: string;
  /** kind-specific payload; validated/narrowed inside WidgetCard. */
  data:
    | ChartData
    | ImageData
    | ClockData
    | WeatherData
    | VideoData
    | AudioData
    | AppWidgetData
    | Record<string, unknown>;
}

export interface WidgetMessage {
  id: string;
  role: 'widget';
  widget: WidgetSpec;
  timestamp: string;
}

export type ChatMessage = TextMessage | PermissionMessage | QuestionMessage | ToolMessage | FormMessage | WidgetMessage;

/** One row of GET /api/widgets — the daemon's widget catalog. */
export interface WidgetCatalogEntry {
  id: string;
  source: string; // "builtin" | "app:<id>" | "plugin:<name>"
  kind: 'template' | 'url';
  name: string;
  description: string;
  surfaces: string[];
  params?: Record<string, unknown>;
  entryUrl?: string;
  entry?: string;
  size?: string;
  refreshMs?: number;
  textFallback?: string;
  intents?: string[];
  enabled: boolean;
}

/** GET/PUT /api/defaults — effective default-flow settings. */
export interface FlowDefaults {
  openLink: 'system-browser' | 'mini-browser' | 'new-tab' | string;
  media: 'inline-widget' | 'mini-browser' | 'system-browser' | string;
  search: 'browser' | 'search-app' | string;
  searchEngine: 'google' | 'bing' | string;
  note: 'space-notes' | 'wiki' | 'memory' | string;
  disabledWidgets: string[];
}

export type AgentState = 'idle' | 'processing' | string;

export type WsStatus = 'connecting' | 'connected' | 'disconnected';

// ===== Dispatch types (multi-agent console) =====

export type TaskStatus = 'registered' | 'processing' | 'done' | 'error' | 'timeout';

export interface DispatchTask {
  id: string;
  label: string;
  agentId: string;   // persisted: folder; virtual: "persona:code-reviewer"
  agentJid: string;  // persisted: jid; virtual: ""
  dependsOn: string[];
  status: TaskStatus;
  prompt: string;
  result: string | null;
  createdAt: string;
  startedAt: string | null;
  timeoutAt: string;
  completedAt: string | null;
  /** Virtual agent task */
  isVirtual?: boolean;
  /** Persona name for virtual agent */
  personaName?: string;
}

export interface DispatchParent {
  id: string;
  adminFolder: string;
  sharedWorkspace: string | null;
  goal: string;
  status: 'queued' | 'active' | 'done';
  createdAt: string;
  completedAt: string | null;
  tasks: DispatchTask[];
}

/** A single activity event from a virtual sub-agent (tool call, message). */
export interface SubAgentActivityEntry {
  entryType: 'tool' | 'think' | 'message';
  toolName?: string;
  title?: string;
  summary?: string;
  content?: unknown;
  ok?: boolean;
  text?: string;
  ts: string;
}

export interface AgentTodoItem {
  content: string;
  status: 'pending' | 'in_progress' | 'completed';
  activeForm?: string;
}

export interface AgentTodosEntry {
  agentName: string;
  todos: AgentTodoItem[];
}

export interface UsageData {
  useTokens: number;
  maxTokens: number;
  promptTokens: number;
}

// ===== Entity model types =====

export interface ChannelInfo {
  id: number;
  platformType: string;
  name: string;
  credentialsJson: string;
  connectionState: string;
  enabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface AgentInfo {
  id: number;
  folder: string;
  name: string;
  requiresTrigger: boolean;
  allowedTools: string[] | null;
  allowedWorkDirs: string[] | null;
  corePrompt: string;
  modelId?: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface BindingInfo {
  id: number;
  jid: string | null;
  agentId: number;
  channelId: number;
  botTokenOverride: string | null;
  maxMessages: number | null;
  lastActive: string | null;
  createdAt: string;
}

export interface BindingWithRelationsInfo extends BindingInfo {
  agent: AgentInfo;
  channel: ChannelInfo;
}

export interface RegisterChannelPayload {
  platformType: string;
  name: string;
  credentials: Record<string, unknown>;
}

export interface RegisterAgentPayload {
  folder: string;
  name: string;
  requiresTrigger?: boolean;
  allowedTools?: string[] | null;
  allowedWorkDirs?: string[] | null;
  corePrompt?: string;
  modelId?: string | null;
}

export interface RegisterBindingPayload {
  agentId: number;
  channelId: number;
  jid?: string;
  botTokenOverride?: string;
  maxMessages?: number | null;
}

export interface UpdateChannelPayload {
  name?: string;
  credentials?: Record<string, unknown>;
  enabled?: boolean;
}

export interface UpdateAgentPayload {
  name?: string;
  requiresTrigger?: boolean;
  allowedTools?: string[] | null;
  allowedWorkDirs?: string[] | null;
  corePrompt?: string;
  modelId?: string | null;
}

export interface UpdateBindingPayload {
  jid?: string;
  botTokenOverride?: string;
  maxMessages?: number | null;
  /** Re-link this binding to a different Agent (profile) by id. */
  agentId?: number;
}

// ===== Cowork entity types =====

export interface CoworkWorkspace {
  id: string;
  name: string;
  description: string | null;
  status: string;
  rootDir: string;
  workingDir?: string | null;
  createdAt: string;
  updatedAt: string;
}

export type ResourceKind = 'raw' | 'wiki' | 'reference' | 'workdir';

export interface WorkspaceResource {
  workspaceId: string;
  kind: ResourceKind;
  path: string;
}

export interface TaskResultEvent {
  taskId: string;
  workspaceId: string;
  title: string;
  inputSummary: string | null;
  resultOutput: string | null;
  references: string | null;
  artifacts: string | null;
  completedAt: string | null;
  outputValidation: OutputValidation | null;
}

export interface OutputValidation {
  formatValid: boolean;
  expectedFormat: string | null;
  requiredSectionsPresent: string[];
  requiredSectionsMissing: string[];
  overallCompliant: boolean;
}

export interface CoworkMember {
  workspaceId: string;
  memberId: string;
  role: string;
  jid: string | null;
  subdir: string | null;
  persona: string | null;
  responsibilities: string | null;
  triggers: string | null;
  handoffRules: string | null;
  acceptanceCriteria: string | null;
  outputFormat: string | null;
  sla: string | null;
  limits: string | null;
  joinedAt: string;
  updatedAt: string;
}

export interface CoworkBoardEntry {
  id: string;
  workspaceId: string;
  section: string;
  title: string | null;
  content: string;
  author: string;
  pinned: boolean;
  tags: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface CoworkTask {
  id: string;
  workspaceId: string;
  title: string;
  description: string | null;
  status: 'backlog' | 'todo' | 'in_progress' | 'review' | 'done' | 'blocked';
  assignee: string | null;
  reviewer: string | null;
  priority: 'low' | 'medium' | 'high' | 'critical';
  dependsOn: string | null;
  attachments: string | null;
  createdBy: string;
  createdAt: string;
  updatedAt: string;
  dueAt: string | null;
  completedAt: string | null;
  inputSummary: string | null;
  resultOutput: string | null;
  references: string | null;
  artifacts: string | null;
}

export interface CoworkTaskComment {
  id: number;
  taskId: string;
  author: string;
  content: string;
  createdAt: string;
}

export interface CoworkMessage {
  id: string;
  workspaceId: string;
  fromMember: string;
  toMember: string | null;
  messageType: 'handoff' | 'review_request' | 'clarification' | 'result' | 'status' | 'alert';
  content: string;
  attachments: string | null;
  taskId: string | null;
  isRead: boolean;
  createdAt: string;
}

// ===== Tool Auto-Accept Rules =====

export type RuleAction = 'auto_accept' | 'auto_deny' | 'force_request' | 'auto_accept_and_allow';

export type RuleMatcherType =
  | 'bash_glob'
  | 'bash_regex'
  | 'tool_exact'
  | 'skill_exact'
  | 'mcp_server'
  | 'mcp_glob'
  | 'tool_category'
  | 'always';

export type ToolCategory = 'file_edit' | 'bash' | 'skill' | 'agent' | 'mcp' | 'all';

export interface RuleMatcher {
  type: RuleMatcherType;
  /** bash_glob / bash_regex / mcp_glob pattern */
  pattern?: string;
  /** tool_exact: exact tool name */
  tool_name?: string;
  /** skill_exact: exact Skill input name */
  skill_name?: string;
  /** mcp_server: server name */
  server?: string;
  /** mcp_server: specific tool (null = all tools of server) */
  tool?: string | null;
  /** tool_category */
  category?: ToolCategory;
}

export interface RuleScope {
  group_jid?: string;
  agent_id?: string;
}

export interface ToolAutoAcceptRule {
  id: string;
  matcher: RuleMatcher;
  action: RuleAction;
  scope?: RuleScope | null;
  enabled: boolean;
  description?: string | null;
}

// ===== Cowork Template types =====

export interface CoworkTemplate {
  name: string;
  description: string;
  icon?: string;
  members: TemplateMember[];
  board?: TemplateBoard;
}

export interface TemplateMember {
  agentFolder: string;
  role: string;
  subdir?: string;
  persona?: string;
  responsibilities?: string[];
  triggers?: TemplateTrigger[];
  handoff?: TemplateHandoffRule[];
  acceptanceCriteria?: string[];
  output?: TemplateOutput;
  sla?: TemplateSla;
  limits?: TemplateLimits;
}

export interface TemplateTrigger {
  type: string;
  condition?: string;
  from?: string;
  messageType?: string;
  status?: string;
  assignee?: string;
  cron?: string;
}

export interface TemplateHandoffRule {
  when: string;
  to: string;
  type: string;
  messageTemplate?: string;
}

export interface TemplateOutput {
  format?: string;
  requiredSections?: string[];
  attachDiff?: boolean;
}

export interface TemplateSla {
  maxDurationPerTaskMinutes?: number;
  maxTokenPerTask?: number;
  escalateAfterBlockedMinutes?: number;
}

export interface TemplateLimits {
  maxFileSizeWriteKb?: number;
  allowedBashCommands?: string[];
  deniedTools?: string[];
}

export interface TemplateBoard {
  sections: TemplateBoardSection[];
}

export interface TemplateBoardSection {
  type: string;
  title: string;
  template?: string;
}

export interface EventNotification {
  id: string;
  eventId: string;
  title: string;
  startAt: number;
  /** "start" = sự kiện bắt đầu, "reminder" = nhắc trước, "renotify" = nhắc lại khi đang diễn ra, "pending" = chưa phát */
  kind: 'start' | 'reminder' | 'renotify' | 'pending';
  receivedAt: number;
  read: boolean;
  /** Thời điểm thông báo thực sự phát (epoch ms). Không có với pending. */
  firedAt?: number;
  /** >0 khi daemon trễ so với trigger */
  delayedMs?: number;
  /** Với pending: thời điểm sẽ phát nhắc */
  triggerAt?: number;
  reminderMin?: number;
}

// ===== Workbench =====

export interface WorkbenchFile {
  path: string;
  content?: string;
  mimeType?: string;
  /** Optional content hash used for cache invalidation in renderers. */
  hash?: string;
  /** Resolved extension (for legacy renderers). Inferred from path if omitted. */
  extension?: 'html' | 'md' | string;
}

export interface WorkbenchProcess {
  status: 'starting' | 'ready' | 'crashed' | 'stopped';
  logPath?: string;
}

export interface WorkbenchArtifact {
  id: string;
  title: string;
  mode: 'static' | 'web' | 'backend';
  files?: WorkbenchFile[];
  url?: string;
  process?: WorkbenchProcess;
  usage?: string;
  createdAt: number;
}

/** Per-groupJid workbench frontend state. */
export interface WorkbenchState {
  current: WorkbenchArtifact | null;
  /** History excludes current. */
  history: WorkbenchArtifact[];
}
