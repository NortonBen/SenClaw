export interface GroupInfo {
  jid: string;
  folder: string;
  name: string;
  isAdmin: boolean;
  channel: string;
  groupType: string;
  requiresTrigger: boolean;
  allowedTools: string[] | null;
  allowedPaths: string[] | null;
  allowedWorkDirs: string[] | null;
  maxMessages: number | null;
  agentId?: number;
  channelId?: number;
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
}

export interface UpdateGroupPayload {
  name?: string;
  channel?: string;
  groupType?: string;
  isAdmin?: boolean;
  requiresTrigger?: boolean;
  allowedTools?: string[] | null;
  allowedPaths?: string[] | null;
  allowedWorkDirs?: string[] | null;
  maxMessages?: number | null;
}

// ===== Message types =====

export interface TextMessage {
  id: string;
  role: 'user' | 'agent' | 'other';
  senderName?: string;
  text: string;
  timestamp: string;
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

export type ChatMessage = TextMessage | PermissionMessage | QuestionMessage;

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
  isAdmin: boolean;
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
  isAdmin?: boolean;
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
  | 'mcp_server'
  | 'mcp_glob'
  | 'tool_category'
  | 'always';

export type ToolCategory = 'file_edit' | 'bash' | 'skill' | 'mcp' | 'all';

export interface RuleMatcher {
  type: RuleMatcherType;
  /** bash_glob / bash_regex / mcp_glob pattern */
  pattern?: string;
  /** tool_exact: exact tool name */
  tool_name?: string;
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
