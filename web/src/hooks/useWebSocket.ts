import { useCallback, useEffect, useRef, useState, useMemo } from 'react';
import type { GroupInfo, ChatMessage, TextMessage, ToolMessage, AgentState, WsStatus, PermissionMessage, QuestionMessage, RegisterGroupPayload, UpdateGroupPayload, DispatchParent, AgentTodosEntry, UsageData, ChannelInfo, AgentInfo, BindingInfo, BindingWithRelationsInfo, RegisterChannelPayload, RegisterAgentPayload, RegisterBindingPayload, UpdateChannelPayload, UpdateAgentPayload, UpdateBindingPayload, ToolAutoAcceptRule, TaskResultEvent, ImageAttachment, EventNotification, WorkbenchState, WorkbenchArtifact } from '../types';

const TOOL_RULES_KEY = 'senclaw:tool-rules';
const ACCEPT_ALL_KEY = 'senclaw:dangerously-accept-all';

function loadRules(): ToolAutoAcceptRule[] {
  try {
    const raw = localStorage.getItem(TOOL_RULES_KEY);
    return raw ? (JSON.parse(raw) as ToolAutoAcceptRule[]) : [];
  } catch { return []; }
}

function saveRules(rules: ToolAutoAcceptRule[]) {
  try { localStorage.setItem(TOOL_RULES_KEY, JSON.stringify(rules)); } catch {}
}

function loadAcceptAll(): boolean {
  try { return localStorage.getItem(ACCEPT_ALL_KEY) === 'true'; } catch { return false; }
}

function saveAcceptAll(v: boolean) {
  try { localStorage.setItem(ACCEPT_ALL_KEY, String(v)); } catch {}
}

// ===== Workbench local persistence (survive page refresh of launched UI tabs) =====
// Frontend-only: while the daemon is alive a refresh fully restores; after a
// daemon restart, dead backend artifacts still show until the user closes them.
const WORKBENCH_STORAGE_KEY = 'senclaw:workbench:v1';

function loadWorkbench(): Record<string, WorkbenchState> {
  try {
    const raw = window.localStorage.getItem(WORKBENCH_STORAGE_KEY);
    return raw ? (JSON.parse(raw) as Record<string, WorkbenchState>) : {};
  } catch { return {}; }
}

function saveWorkbench(state: Record<string, WorkbenchState>): void {
  try { window.localStorage.setItem(WORKBENCH_STORAGE_KEY, JSON.stringify(state)); } catch {}
}

// ===== Agent usage local persistence (context bar survives page reload) =====
const AGENT_USAGE_STORAGE_KEY = 'senclaw:agent-usage:v1';

function loadAgentUsage(): Record<string, import('../types').UsageData> {
  try {
    const raw = window.localStorage.getItem(AGENT_USAGE_STORAGE_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch { return {}; }
}

function saveAgentUsage(state: Record<string, import('../types').UsageData>): void {
  try { window.localStorage.setItem(AGENT_USAGE_STORAGE_KEY, JSON.stringify(state)); } catch {}
}

interface WsConfig {
  wsPort: number;
  token: string;
}

export interface WsHook {
  status: WsStatus;
  groups: GroupInfo[];
  messages: Record<string, ChatMessage[]>;
  agentStates: Record<string, AgentState>;
  /** jid → context compacting in progress (disables pause while compacting) */
  agentCompacting: Record<string, boolean>;
  subscribed: Set<string>;
  subscribe: (jid: string) => void;
  sendMessage: (jid: string, text: string, attachments?: ImageAttachment[]) => void;
  /** Pause agent (sends agent:control pause) */
  pauseAgent: (jid: string) => void;
  /** Resume agent, optional follow-up text (sends agent:control resume) */
  resumeAgent: (jid: string, query?: string) => void;
  /** Stop and reset agent session (sends agent:control stop) */
  stopAgent: (jid: string) => void;
  /** Stop agent and permanently delete all persisted history for this JID. */
  stopAndClearHistory: (jid: string) => void;
  resolvePermission: (requestId: string, optionKey: string) => void;
  resolveQuestion: (requestId: string, answers: Record<number, number | number[]>, otherTexts?: Record<number, string>) => void;
  registerGroup: (data: RegisterGroupPayload) => void;
  registerFeishuApp: (appId: string, appSecret: string, domain?: string) => void;
  registerQQApp: (appId: string, appSecret: string, sandbox?: boolean) => void;
  unregisterGroup: (jid: string) => void;
  updateGroup: (jid: string, updates: UpdateGroupPayload) => void;
  dispatchParents: DispatchParent[];
  agentTodos: Record<string, AgentTodosEntry>; // keyed by agentJid
  agentUsage: Record<string, UsageData>; // keyed by agentJid
  subscribeAll: () => void;
  /** Incremented on every cowork:changed event so the CoworkPage can auto-refresh. */
  coworkChanged: number;
  /** Latest task result event from cowork:task:result — use for live result display. */
  lastTaskResult: TaskResultEvent | null;
  /** Incremented on cowork:resource:changed so resource panels auto-refresh. */
  coworkResourceChanged: number;
  // Entity model
  channels: ChannelInfo[];
  agents: AgentInfo[];
  bindings: BindingWithRelationsInfo[];
  registerChannel: (data: RegisterChannelPayload) => void;
  registerAgent: (data: RegisterAgentPayload) => void;
  registerBinding: (data: RegisterBindingPayload) => void;
  unregisterChannel: (id: number) => void;
  unregisterAgent: (id: number) => void;
  unregisterBinding: (id: number) => void;
  updateChannel: (id: number, updates: UpdateChannelPayload) => void;
  updateAgent: (id: number, updates: UpdateAgentPayload) => void;
  updateBinding: (id: number, updates: UpdateBindingPayload) => void;
  // Tool auto-accept rules
  toolRules: ToolAutoAcceptRule[];
  dangerouslyAcceptAll: boolean;
  addToolRule: (rule: ToolAutoAcceptRule) => void;
  removeToolRule: (id: string) => void;
  toggleToolRule: (id: string) => void;
  setDangerouslyAcceptAll: (enabled: boolean) => void;
  // Event notifications
  notifications: EventNotification[];
  markNotificationRead: (id: string) => void;
  clearAllNotifications: () => void;
  // Plan mode (AgentMode::Plan workflow)
  /** Active plan-exit request — non-null when an agent is awaiting plan approval. */
  planExitRequest: PlanExitRequest | null;
  /** Send the user's plan-exit decision back to the engine. */
  resolvePlanExit: (selected: PlanExitOption) => void;
  /** Dismiss the pending plan-exit dialog without responding (UI-only). */
  dismissPlanExit: () => void;
  // Agent mode toggle (per chat JID)
  /** Per-JID active agent mode (defaults to `Agent` when absent). */
  agentModes: Record<string, AgentMode>;
  /** Switch the engine's mode for a specific chat JID. */
  setAgentMode: (jid: string, mode: AgentMode) => void;
  /** Incremented every time the server broadcasts `space:events:changed`
   *  (i.e. an agent created / updated / deleted a Space event via MCP).
   *  Calendar views watch this to re-fetch — otherwise agent-driven
   *  mutations stay invisible until the user navigates. */
  spaceEventsRev: number;
  // Plan history (persisted ExitPlanMode requests)
  /** Per-jid list of plan summaries fetched via requestPlanList. */
  plansByJid: Record<string, PlanSummary[]>;
  /** Last full plan fetched via requestPlan (keyed by plan id). */
  planById: Record<string, PlanFull>;
  /** Request the persisted plan list for a group. */
  requestPlanList: (jid: string) => void;
  /** Request the full markdown for a single plan by id. */
  requestPlan: (id: string) => void;
  // ===== Workbench (LaunchUI) =====
  /** jid → workbench panel state ({ current, history }). */
  workbench: Record<string, WorkbenchState>;
  /** Latest arrived (jid, artifactId) — drives the auto-foreground effect. */
  workbenchLatest: { jid: string; artifactId: string; at: number } | null;
  /** Currently-foregrounded chat jid (set by ChatPage); used by the dock. */
  activeJid: string | null;
  /** Set the active chat jid so the shared layout can show its workbench. */
  setActiveJid: (jid: string | null) => void;
  /** Tell the backend the user foregrounded an artifact (updates last_active). */
  workbenchMarkViewed: (jid: string, artifactId: string) => void;
  /** Close an artifact (drop from registry; kill service process). */
  workbenchClose: (jid: string, artifactId: string) => void;
  /** Read an artifact file's content (request/response → Promise). */
  workbenchReadFile: (jid: string, artifactId: string, path: string) => Promise<{ content?: string; error?: string }>;
  /** Tail the last N lines of a service artifact's log. */
  workbenchFetchLogs: (jid: string, artifactId: string, tailLines?: number) => Promise<string>;
  /** Promote a history artifact to current (frontend-only state swap). */
  workbenchSetCurrent: (jid: string, artifactId: string) => void;
}

export interface PlanSummary {
  id: string;
  chatJid: string;
  agentId: string;
  title: string;
  filePath: string;
  approval: string;
  createdAt: string;
  approvedAt?: string | null;
}

export interface PlanFull extends PlanSummary {
  contentMd: string;
}

export type AgentMode = 'Agent' | 'Plan';

export type PlanExitOption = 'startEditing' | 'clearContextAndStart';

export interface PlanExitRequest {
  /** Group/agent JID that originated the request. */
  groupJid: string;
  /** Internal agent id (usually `main`). */
  agentId: string;
  /** Absolute path of the plan file written by the agent. */
  planFilePath: string;
  /** Markdown plan content. */
  planContent: string;
  /** Display labels for the two approval options. */
  options: { startEditing: string; clearContextAndStart: string };
  /** UTC timestamp the request arrived (used for cache invalidation). */
  receivedAt: number;
}

export function useWebSocket(): WsHook {
  const [status, setStatus]           = useState<WsStatus>('connecting');
  const [groups, setGroups]           = useState<GroupInfo[]>([]);
  const [messages, setMessages]       = useState<Record<string, ChatMessage[]>>({});
  const [agentStates, setAgentStates]       = useState<Record<string, AgentState>>({});
  const [agentCompacting, setAgentCompacting] = useState<Record<string, boolean>>({});
  const [subscribed, setSubscribed]   = useState<Set<string>>(new Set());
  const [dispatchParents, setDispatchParents] = useState<DispatchParent[]>([]);
  const [agentTodos, setAgentTodos]           = useState<Record<string, AgentTodosEntry>>({});
  const [agentUsage, setAgentUsage]           = useState<Record<string, UsageData>>(loadAgentUsage);
  const [coworkChanged, setCoworkChanged]     = useState(0);
  const [lastTaskResult, setLastTaskResult]   = useState<TaskResultEvent | null>(null);
  const [coworkResourceChanged, setCoworkResourceChanged] = useState(0);
  const [spaceEventsRev, setSpaceEventsRev] = useState(0);
  const [plansByJid, setPlansByJid] = useState<Record<string, PlanSummary[]>>({});
  const [planById, setPlanById] = useState<Record<string, PlanFull>>({});
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [agents, setAgents]     = useState<AgentInfo[]>([]);
  const [bindings, setBindings] = useState<BindingWithRelationsInfo[]>([]);
  const [toolRules, setToolRules]             = useState<ToolAutoAcceptRule[]>(loadRules);
  const [dangerouslyAcceptAll, setAcceptAllState] = useState<boolean>(loadAcceptAll);
  const [notifications, setNotifications]     = useState<EventNotification[]>([]);
  const [planExitRequest, setPlanExitRequest] = useState<PlanExitRequest | null>(null);
  const [agentModes, setAgentModes] = useState<Record<string, AgentMode>>({});
  const [workbench, setWorkbench]             = useState<Record<string, WorkbenchState>>(loadWorkbench);
  const [workbenchLatest, setWorkbenchLatest] = useState<{ jid: string; artifactId: string; at: number } | null>(null);
  const [activeJid, setActiveJid]             = useState<string | null>(null);

  // Mirror workbench state to localStorage so a refresh restores launched tabs.
  useEffect(() => { saveWorkbench(workbench); }, [workbench]);

  // Persist context usage so the header bar survives page reloads.
  useEffect(() => { saveAgentUsage(agentUsage); }, [agentUsage]);

  const wsRef        = useRef<WebSocket | null>(null);
  const configRef    = useRef<WsConfig | null>(null);
  const reconnectRef = useRef<ReturnType<typeof setTimeout>>();
  const retryCountRef = useRef(0);
  const subscribedRef = useRef<Set<string>>(new Set());
  // jid → delayed clear timer after all todos complete
  const todosClearTimers = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  const addMessage = useCallback((jid: string, msg: ChatMessage) => {
    setMessages(prev => ({ ...prev, [jid]: [...(prev[jid] ?? []), msg] }));
  }, []);

  const updateMessage = useCallback((jid: string, id: string, updater: (m: ChatMessage) => ChatMessage) => {
    setMessages(prev => ({
      ...prev,
      [jid]: (prev[jid] ?? []).map(m => m.id === id ? updater(m) : m),
    }));
  }, []);

  const rawSend = useCallback((data: object) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(data));
    }
  }, []);

  const subscribe = useCallback((jid: string) => {
    if (subscribedRef.current.has(jid)) return;
    rawSend({ type: 'subscribe', groupJid: jid });
    subscribedRef.current.add(jid);
    setSubscribed(prev => new Set([...prev, jid]));
  }, [rawSend]);

  const sendMessage = useCallback((jid: string, text: string, attachments: ImageAttachment[] = []) => {
    addMessage(jid, {
      id:        `local-${Date.now()}`,
      role:      'user',
      text,
      attachments: attachments.length > 0 ? attachments : undefined,
      timestamp: new Date().toISOString(),
    });
    rawSend({ type: 'message', groupJid: jid, text, attachments: attachments.length > 0 ? attachments : undefined });
  }, [addMessage, rawSend]);

  // Find which jid owns a requestId — scan all message lists
  const findRequestJid = useCallback((requestId: string): string | null => {
    let found: string | null = null;
    setMessages(prev => {
      for (const [jid, msgs] of Object.entries(prev)) {
        if (msgs.some(m => (m.role === 'permission' || m.role === 'question') && (m as PermissionMessage | QuestionMessage).requestId === requestId)) {
          found = jid;
          break;
        }
      }
      return prev; // no change
    });
    return found;
  }, []);

  const resolvePermission = useCallback((requestId: string, optionKey: string) => {
    rawSend({ type: 'permission:response', requestId, optionKey });
    // Update local card to show resolved state
    setMessages(prev => {
      const next = { ...prev };
      for (const [jid, msgs] of Object.entries(prev)) {
        const idx = msgs.findIndex(m => m.role === 'permission' && (m as PermissionMessage).requestId === requestId);
        if (idx >= 0) {
          const perm = msgs[idx] as PermissionMessage;
          const option = perm.options.find(o => o.key === optionKey);
          const updated: PermissionMessage = { ...perm, resolved: option ?? { key: optionKey, label: optionKey } };
          next[jid] = [...msgs.slice(0, idx), updated, ...msgs.slice(idx + 1)];
          break;
        }
      }
      return next;
    });
  }, [rawSend]);

  const resolveQuestion = useCallback((requestId: string, answers: Record<number, number | number[]>, otherTexts?: Record<number, string>) => {
    rawSend({ type: 'question:response', requestId, answers, ...(otherTexts ? { otherTexts } : {}) });
    // Update local card to show resolved state
    setMessages(prev => {
      const next = { ...prev };
      for (const [jid, msgs] of Object.entries(prev)) {
        const idx = msgs.findIndex(m => m.role === 'question' && (m as QuestionMessage).requestId === requestId);
        if (idx >= 0) {
          const q = msgs[idx] as QuestionMessage;
          const updated: QuestionMessage = { ...q, selections: answers, otherTexts, resolved: true };
          next[jid] = [...msgs.slice(0, idx), updated, ...msgs.slice(idx + 1)];
          break;
        }
      }
      return next;
    });
  }, [rawSend]);

  const registerGroup = useCallback((data: RegisterGroupPayload) => {
    rawSend({ type: 'register:group', ...data });
  }, [rawSend]);

  const registerFeishuApp = useCallback((appId: string, appSecret: string, domain?: string) => {
    rawSend({ type: 'register:feishu-app', appId, appSecret, ...(domain ? { domain } : {}) });
  }, [rawSend]);

  const registerQQApp = useCallback((appId: string, appSecret: string, sandbox?: boolean) => {
    rawSend({ type: 'register:qq-app', appId, appSecret, ...(sandbox ? { sandbox } : {}) });
  }, [rawSend]);

  const unregisterGroup = useCallback((jid: string) => {
    rawSend({ type: 'unregister:group', jid });
  }, [rawSend]);

  const updateGroup = useCallback((jid: string, updates: UpdateGroupPayload) => {
    rawSend({ type: 'update:group', jid, ...updates });
  }, [rawSend]);

  const pauseAgent = useCallback((jid: string) => {
    rawSend({ type: 'agent:control', groupJid: jid, action: 'pause' });
  }, [rawSend]);

  const resumeAgent = useCallback((jid: string, query?: string) => {
    if (query?.trim()) {
      addMessage(jid, {
        id:        `local-${Date.now()}`,
        role:      'user',
        text:      query.trim(),
        timestamp: new Date().toISOString(),
      });
    }
    rawSend({ type: 'agent:control', groupJid: jid, action: 'resume', ...(query ? { query } : {}) });
  }, [addMessage, rawSend]);

  const stopAgent = useCallback((jid: string) => {
    rawSend({ type: 'agent:control', groupJid: jid, action: 'stop' });
  }, [rawSend]);

  const stopAndClearHistory = useCallback((jid: string) => {
    rawSend({ type: 'agent:control', groupJid: jid, action: 'stop_and_clear' });

    // ── Immediate local wipe ──────────────────────────────────────────────────
    // Don't wait for the backend history:load echo — clear all local state for
    // this JID right now so the UI feels instant.

    // 1. Clear chat messages (tool actions, browser actions, text, etc.)
    setMessages(prev => {
      const next = { ...prev };
      delete next[jid];
      return next;
    });

    // 2. Clear workbench artifacts (sidebar panels, code previews, etc.)
    setWorkbench(prev => {
      const next = { ...prev };
      delete next[jid];
      return next;
    });

    // 3. Clear token usage indicator.
    setAgentUsage(prev => {
      const next = { ...prev };
      delete next[jid];
      return next;
    });

    // 4. Reset agent state to idle.
    setAgentStates(prev => ({ ...prev, [jid]: 'idle' as AgentState }));
  }, [rawSend, setMessages, setWorkbench, setAgentUsage, setAgentStates]);

  const subscribeAll = useCallback(() => {
    const toSubscribe: string[] = [];
    setGroups(prev => {
      for (const g of prev) {
        if (!subscribedRef.current.has(g.jid)) {
          toSubscribe.push(g.jid);
          subscribedRef.current.add(g.jid);
        }
      }
      return prev;
    });
    for (const jid of toSubscribe) {
      rawSend({ type: 'subscribe', groupJid: jid });
    }
    if (toSubscribe.length > 0) {
      setSubscribed(prev => new Set([...prev, ...toSubscribe]));
    }
  }, [rawSend]);

  const registerChannel = useCallback((data: RegisterChannelPayload) => {
    rawSend({ type: 'register:channel', ...data });
  }, [rawSend]);

  const registerAgent = useCallback((data: RegisterAgentPayload) => {
    rawSend({ type: 'register:agent', ...data });
  }, [rawSend]);

  const registerBinding = useCallback((data: RegisterBindingPayload) => {
    rawSend({ type: 'register:binding', ...data });
  }, [rawSend]);

  const unregisterChannel = useCallback((id: number) => {
    rawSend({ type: 'unregister:channel', id });
  }, [rawSend]);

  const unregisterAgent = useCallback((id: number) => {
    rawSend({ type: 'unregister:agent', id });
  }, [rawSend]);

  const unregisterBinding = useCallback((id: number) => {
    rawSend({ type: 'unregister:binding', id });
  }, [rawSend]);

  const updateChannel = useCallback((id: number, updates: UpdateChannelPayload) => {
    rawSend({ type: 'update:channel', id, ...updates });
  }, [rawSend]);

  const updateAgent = useCallback((id: number, updates: UpdateAgentPayload) => {
    rawSend({ type: 'update:agent', id, ...updates });
  }, [rawSend]);

  const updateBinding = useCallback((id: number, updates: UpdateBindingPayload) => {
    rawSend({ type: 'update:binding', id, ...updates });
  }, [rawSend]);

  const addToolRule = useCallback((rule: ToolAutoAcceptRule) => {
    setToolRules(prev => {
      const next = [...prev, rule];
      saveRules(next);
      rawSend({ type: 'permission:rule:add', rule });
      return next;
    });
  }, [rawSend]);

  const removeToolRule = useCallback((id: string) => {
    setToolRules(prev => {
      const next = prev.filter(r => r.id !== id);
      saveRules(next);
      rawSend({ type: 'permission:rule:remove', ruleId: id });
      return next;
    });
  }, [rawSend]);

  const requestPlanList = useCallback((jid: string) => {
    rawSend({ type: 'plan:list', groupJid: jid });
  }, [rawSend]);

  const requestPlan = useCallback((id: string) => {
    rawSend({ type: 'plan:get', id });
  }, [rawSend]);

  const toggleToolRule = useCallback((id: string) => {
    setToolRules(prev => {
      const next = prev.map(r => r.id === id ? { ...r, enabled: !r.enabled } : r);
      saveRules(next);
      const updated = next.find(r => r.id === id);
      if (updated) rawSend({ type: 'permission:rule:update', rule: updated });
      return next;
    });
  }, [rawSend]);

  const setDangerouslyAcceptAll = useCallback((enabled: boolean) => {
    saveAcceptAll(enabled);
    setAcceptAllState(enabled);
    rawSend({ type: 'permission:accept-all', enabled });
  }, [rawSend]);

  const upsertNotification = useCallback((incoming: EventNotification) => {
    setNotifications(prev => {
      const withoutPending = incoming.kind !== 'pending'
        ? prev.filter(n => !(n.kind === 'pending' && n.eventId === incoming.eventId))
        : prev;
      const idx = withoutPending.findIndex(n => n.id === incoming.id);
      if (idx >= 0) {
        const next = [...withoutPending];
        next[idx] = { ...next[idx], ...incoming };
        return next;
      }
      return [...withoutPending, incoming];
    });
  }, []);

  const markNotificationRead = useCallback((id: string) => {
    setNotifications(prev => prev.map(n => n.id === id ? { ...n, read: true } : n));
    if (!id.startsWith('pending-')) {
      rawSend({ type: 'notification:read', id });
    }
  }, [rawSend]);

  const clearAllNotifications = useCallback(() => {
    setNotifications([]);
  }, []);

  useEffect(() => {
    let destroyed = false;

    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const handleMsg = (raw: MessageEvent) => {
      try {
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        const msg = JSON.parse(raw.data as string) as Record<string, any>;
        switch (msg.type) {
          case 'auth:ok':
            setStatus('connected');
            retryCountRef.current = 0;
            rawSend({ type: 'list:groups' });
            rawSend({ type: 'list:channels' });
            rawSend({ type: 'list:agents' });
            rawSend({ type: 'list:bindings' });
            // Re-subscribe to all previously-subscribed groups after reconnect
            for (const jid of subscribedRef.current) {
              rawSend({ type: 'subscribe', groupJid: jid });
            }
            // Re-sync tool auto-accept rules to backend (in-memory, lost on restart)
            {
              const rules = loadRules();
              const acceptAll = loadAcceptAll();
              if (acceptAll) rawSend({ type: 'permission:accept-all', enabled: true });
              for (const rule of rules) {
                rawSend({ type: 'permission:rule:add', rule });
              }
            }
            break;
          case 'groups': {
            const incoming = (msg.groups as GroupInfo[]) ?? [];
            setGroups(incoming);
            // Auto-subscribe to admin groups so requireAdmin checks pass for settings operations
            for (const g of incoming) {
              if (g.isAdmin) subscribe(g.jid);
            }
            break;
          }
          case 'subscribed':
            setSubscribed(prev => new Set([...prev, msg.groupJid as string]));
            break;
          case 'history:load': {
            const hjid = msg.groupJid as string;
            // history:load carries polymorphic entries: text messages
            // (role: 'user' | 'agent') and persisted tool executions
            // (role: 'tool'). Discriminate by role so ChatView's
            // consecutive-tool grouping (ToolGroupCard) lights up
            // identically to the live `tool:execution` path.
            const msgs = msg.messages as Array<Record<string, unknown>>;
            if (Array.isArray(msgs)) {
              const hydrated: ChatMessage[] = msgs.map((m): ChatMessage => {
                if (m.role === 'tool') {
                  return {
                    id:        m.id as string,
                    role:      'tool',
                    agentId:   (m.agentId as string) ?? 'main',
                    toolName:  (m.toolName as string) ?? '',
                    title:     (m.title as string) ?? '',
                    summary:   (m.summary as string) ?? '',
                    content:   m.content,
                    ok:        m.ok !== false,
                    timestamp: m.timestamp as string,
                  } as ToolMessage;
                }
                return {
                  id:         m.id as string,
                  role:       (m.role === 'agent' ? 'agent' : 'user'),
                  senderName: m.senderName as string | undefined,
                  text:       (m.text as string) ?? '',
                  timestamp:  m.timestamp as string,
                } as TextMessage;
              });
              setMessages(prev => ({
                ...prev,
                [hjid]: hydrated,
              }));
            }
            break;
          }
          case 'chat:history': {
            // Replay of ephemeral chat events (agent:state + permission/
            // question request/resolved pairs). Server emits this right
            // after history:load on every subscribe so the UI rebuilds
            // in-flight interactions after a page reload.
            const hjid = msg.groupJid as string;
            const events = msg.events as Array<{
              id: number;
              eventType: string;
              requestId?: string | null;
              payload: Record<string, unknown> | null;
              timestamp: string;
            }>;
            if (!Array.isArray(events)) break;

            // Pre-scan for resolved request IDs so we don't re-add pending
            // PermissionMessage / QuestionMessage entries that have already
            // been answered.
            const resolved = new Set<string>();
            for (const e of events) {
              if ((e.eventType === 'permission:resolved' || e.eventType === 'question:resolved') && e.requestId) {
                resolved.add(`${e.eventType}|${e.requestId}`);
              }
            }

            for (const e of events) {
              const p = (e.payload ?? {}) as Record<string, unknown>;
              if (e.eventType === 'agent:state') {
                const s = typeof p.state === 'string' ? p.state : undefined;
                if (s) {
                  setAgentStates(prev => ({ ...prev, [hjid]: s as AgentState }));
                  if (s === 'idle') setAgentCompacting(prev => ({ ...prev, [hjid]: false }));
                }
              } else if (e.eventType === 'permission:request' && e.requestId) {
                if (resolved.has(`permission:resolved|${e.requestId}`)) continue;
                addMessage(hjid, {
                  id:        `perm-${e.requestId}`,
                  role:      'permission',
                  requestId: e.requestId,
                  toolName:  (p.toolName as string) ?? '',
                  title:     (p.title as string) ?? '',
                  content:   (p.content as string) ?? '',
                  options:   (p.options as PermissionMessage['options']) ?? [],
                  timestamp: e.timestamp,
                });
              } else if (e.eventType === 'question:request' && e.requestId) {
                if (resolved.has(`question:resolved|${e.requestId}`)) continue;
                addMessage(hjid, {
                  id:         `q-${e.requestId}`,
                  role:       'question',
                  requestId:  e.requestId,
                  agentId:    (p.agentId as string) ?? 'main',
                  questions:  (p.questions as QuestionMessage['questions']) ?? [],
                  selections: {},
                  resolved:   false,
                  timestamp:  e.timestamp,
                });
              }
              // *:resolved events are handled implicitly via the pre-scan.
            }
            break;
          }
          case 'incoming': {
            const inJid = msg.groupJid as string;
            if (!msg.isFromMe) {
              addMessage(inJid, {
                id:         `in-${Date.now()}-${Math.random()}`,
                role:       'other',
                senderName: msg.senderName as string,
                text:       msg.text as string,
                timestamp:  msg.timestamp as string,
              });
            }
            // Ensure the group appears in the sidebar even if it wasn't in the
            // initial list:groups response (e.g. chat started from the channel
            // side before the UI loaded).
            setGroups(prev => {
              if (prev.some(g => g.jid === inJid)) return prev;
              return [...prev, {
                jid:             inJid,
                folder:          '',
                name:            msg.senderName as string || inJid,
                isAdmin:         false,
                channel:         inJid.split(':')[0] ?? 'unknown',
                requiresTrigger: false,
              } as GroupInfo];
            });
            break;
          }
          case 'agent:reply':
            // Prefer server-stamped `ts` so this bubble interleaves with
            // tool:execution events (which also carry server `ts`).
            // Falls back to client clock only when the server didn't send one,
            // which keeps us compatible with older daemons.
            addMessage(msg.groupJid as string, {
              id:        `agent-${Date.now()}-${Math.random()}`,
              role:      'agent',
              text:      msg.text as string,
              timestamp: (msg.ts as string) ?? new Date().toISOString(),
            });
            // State is managed solely by agent:state events — do not override here.
            // agent:reply can fire for intermediate replies during multi-turn dispatch,
            // and incorrectly setting idle would cause the pause button to disappear.
            break;
          case 'agent:state':
            setAgentStates(prev => ({ ...prev, [msg.groupJid as string]: msg.state as string }));
            // Clear compacting flag on idle (avoids stuck "Compacting…" if stopped mid-compact)
            if (msg.state === 'idle') {
              setAgentCompacting(prev => ({ ...prev, [msg.groupJid as string]: false }));
            }
            break;
          case 'agent:compacting':
            setAgentCompacting(prev => ({ ...prev, [msg.groupJid as string]: msg.isCompacting as boolean }));
            break;
          case 'permission:request':
            addMessage(msg.groupJid as string, {
              id:        `perm-${msg.requestId as string}`,
              role:      'permission',
              requestId: msg.requestId as string,
              toolName:  msg.toolName as string,
              title:     msg.title as string,
              content:   msg.content as string,
              options:   msg.options as PermissionMessage['options'],
              timestamp: new Date().toISOString(),
            });
            break;
          case 'question:request':
            addMessage(msg.groupJid as string, {
              id:         `q-${msg.requestId as string}`,
              role:       'question',
              requestId:  msg.requestId as string,
              agentId:    msg.agentId as string,
              questions:  msg.questions as QuestionMessage['questions'],
              selections: {},
              resolved:   false,
              timestamp:  new Date().toISOString(),
            });
            break;
          case 'permission:resolved': {
            const rJid = msg.groupJid as string;
            const rId  = msg.requestId as string;
            updateMessage(rJid, `perm-${rId}`, (m) => {
              const perm = m as PermissionMessage;
              if (perm.resolved) return m; // already resolved locally
              const option = perm.options?.find(o => o.key === (msg.optionKey as string));
              return { ...perm, resolved: option ?? { key: msg.optionKey as string, label: msg.optionLabel as string } };
            });
            break;
          }
          case 'question:resolved': {
            const qJid = msg.groupJid as string;
            const qId  = msg.requestId as string;
            updateMessage(qJid, `q-${qId}`, (m) => {
              const q = m as QuestionMessage;
              if (q.resolved) return m; // already resolved locally
              return { ...q, resolved: true };
            });
            break;
          }
          case 'plans:list': {
            const jid = msg.groupJid as string;
            const plans = (msg.plans as PlanSummary[]) ?? [];
            setPlansByJid(prev => ({ ...prev, [jid]: plans }));
            break;
          }
          case 'plans:get': {
            const plan = msg.plan as PlanFull | undefined;
            if (plan?.id) setPlanById(prev => ({ ...prev, [plan.id]: plan }));
            break;
          }
          case 'tool:execution': {
            // Inline tool-call activity for the chat UI. ChatView groups
            // consecutive ToolMessages from the same agent turn into one
            // collapsible card. Each message is one tool call.
            const jid = msg.groupJid as string;
            const toolName = (msg.toolName as string) ?? '';
            const title = (msg.title as string) ?? toolName;
            addMessage(jid, {
              id: `tool-${jid}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
              role: 'tool',
              agentId: (msg.agentId as string) ?? 'main',
              toolName,
              title,
              summary: (msg.summary as string) ?? '',
              content: msg.content,
              ok: msg.ok !== false,
              timestamp: (msg.ts as string) ?? new Date().toISOString(),
            });
            break;
          }
          case 'plan:exit:request': {
            // Engine has prepared a plan and is awaiting user approval.
            // Surface a modal to the user; resolvePlanExit() posts the choice back.
            const opts = (msg.options as { startEditing?: string; clearContextAndStart?: string } | undefined) ?? {};
            setPlanExitRequest({
              groupJid: (msg.groupJid as string) ?? '',
              agentId: (msg.agentId as string) ?? 'main',
              planFilePath: (msg.planFilePath as string) ?? '',
              planContent: (msg.planContent as string) ?? '',
              options: {
                startEditing: opts.startEditing ?? 'Approve plan and start editing',
                clearContextAndStart: opts.clearContextAndStart ?? 'Clear context and start fresh',
              },
              receivedAt: Date.now(),
            });
            break;
          }
          case 'plan:exit:response': {
            // Server-side confirmation that the engine accepted our choice.
            // Clearing local state collapses the dialog if still open.
            setPlanExitRequest(null);
            break;
          }
          case 'plan:implement': {
            // Engine moved into implementation phase — close any pending dialog.
            setPlanExitRequest(null);
            break;
          }
          case 'agent:mode:changed': {
            const jid = msg.groupJid as string;
            const mode = msg.mode as AgentMode;
            if (jid && (mode === 'Agent' || mode === 'Plan')) {
              setAgentModes((prev) => ({ ...prev, [jid]: mode }));
            }
            break;
          }
          case 'group:registered':
            setGroups(prev => {
              const g = msg.group as GroupInfo;
              return prev.some(x => x.jid === g.jid) ? prev.map(x => x.jid === g.jid ? g : x) : [...prev, g];
            });
            break;
          case 'group:unregistered':
            setGroups(prev => prev.filter(g => g.jid !== (msg.jid as string)));
            break;
          case 'group:updated':
            setGroups(prev => prev.map(g => g.jid === (msg.group as GroupInfo).jid ? msg.group as GroupInfo : g));
            break;
          case 'dispatch:update': {
            const newParents = (msg.parents as DispatchParent[]) ?? [];
            const TERMINAL = ['done', 'error', 'timeout'];
            // When a task reaches a terminal state, clear that agent's todos
            setDispatchParents(prev => {
              for (const np of newParents) {
                for (const nt of np.tasks) {
                  if (!TERMINAL.includes(nt.status)) continue;
                  const op = prev.find(p => p.id === np.id);
                  const ot = op?.tasks.find(t => t.id === nt.id);
                  if (ot && !TERMINAL.includes(ot.status)) {
                    // Task just became terminal
                    setAgentTodos(prevTodos => {
                      const next = { ...prevTodos };
                      delete next[nt.agentJid];
                      return next;
                    });
                  }
                }
              }
              return newParents;
            });
            break;
          }
          case 'workbench:new': {
            const wbJid = msg.groupJid as string;
            const artifact = msg.artifact as WorkbenchArtifact;
            setWorkbench(prev => {
              const cur = prev[wbJid] ?? { current: null, history: [] };
              const newHistory = cur.current
                ? [cur.current, ...cur.history.filter(a => a.id !== cur.current!.id)]
                : cur.history;
              return {
                ...prev,
                [wbJid]: { current: artifact, history: newHistory.filter(a => a.id !== artifact.id) },
              };
            });
            setWorkbenchLatest({ jid: wbJid, artifactId: artifact.id, at: Date.now() });
            break;
          }
          case 'workbench:service_ready':
          case 'workbench:service_crashed':
          case 'workbench:service_stopped': {
            const wbJid = msg.groupJid as string;
            const aid = msg.artifactId as string;
            const statusUpdate = (a: WorkbenchArtifact): WorkbenchArtifact => {
              if (a.id !== aid || !a.process) return a;
              const status = msg.type === 'workbench:service_ready' ? 'ready'
                : msg.type === 'workbench:service_crashed' ? 'crashed'
                : 'stopped';
              return { ...a, process: { ...a.process, status } };
            };
            setWorkbench(prev => {
              const cur = prev[wbJid];
              if (!cur) return prev;
              return {
                ...prev,
                [wbJid]: {
                  current: cur.current ? statusUpdate(cur.current) : null,
                  history: cur.history.map(statusUpdate),
                },
              };
            });
            break;
          }
          case 'agent:todos': {
            const todoJid = msg.agentJid as string;
            const todosArr = ((msg.todos as AgentTodosEntry['todos']) ?? []);
            // Cancel previous delayed clear for this agent (new todos invalidate the old timer)
            const prev = todosClearTimers.current.get(todoJid);
            if (prev) { clearTimeout(prev); todosClearTimers.current.delete(todoJid); }
            setAgentTodos(prev => ({
              ...prev,
              [todoJid]: { agentName: msg.agentName as string, todos: todosArr },
            }));
            // When all complete, clear after 3s so the user can see the done state
            if (todosArr.length > 0 && todosArr.every(t => t.status === 'completed')) {
              const t = setTimeout(() => {
                todosClearTimers.current.delete(todoJid);
                setAgentTodos(prev => { const n = { ...prev }; delete n[todoJid]; return n; });
              }, 3000);
              todosClearTimers.current.set(todoJid, t);
            }
            break;
          }
          case 'agent:usage': {
            const usageJid = msg.agentJid as string;
            setAgentUsage(prev => ({
              ...prev,
              [usageJid]: msg.usage as UsageData,
            }));
            break;
          }
          // Entity model events
          case 'channels':
            setChannels((msg.channels as ChannelInfo[]) ?? []);
            break;
          case 'agents':
            setAgents((msg.agents as AgentInfo[]) ?? []);
            break;
          case 'bindings':
            setBindings((msg.bindings as BindingWithRelationsInfo[]) ?? []);
            break;
          case 'channel:registered':
            setChannels(prev => {
              const ch = msg.channel as ChannelInfo;
              return prev.some(c => c.id === ch.id) ? prev.map(c => c.id === ch.id ? ch : c) : [...prev, ch];
            });
            break;
          case 'agent:registered':
            setAgents(prev => {
              const a = msg.agent as AgentInfo;
              return prev.some(x => x.id === a.id) ? prev.map(x => x.id === a.id ? a : x) : [...prev, a];
            });
            break;
          case 'binding:registered':
            setBindings(prev => {
              const b = msg.binding as BindingWithRelationsInfo;
              return prev.some(x => x.id === b.id) ? prev.map(x => x.id === b.id ? b : x) : [...prev, b];
            });
            break;
          case 'channel:unregistered':
            setChannels(prev => prev.filter(c => c.id !== (msg.id as number)));
            break;
          case 'agent:unregistered':
            setAgents(prev => prev.filter(a => a.id !== (msg.id as number)));
            break;
          case 'binding:unregistered':
            setBindings(prev => prev.filter(b => b.id !== (msg.id as number)));
            break;
          case 'channel:updated':
            setChannels(prev => prev.map(c => c.id === (msg.channel as ChannelInfo).id ? msg.channel as ChannelInfo : c));
            break;
          case 'agent:updated':
            setAgents(prev => prev.map(a => a.id === (msg.agent as AgentInfo).id ? msg.agent as AgentInfo : a));
            break;
          case 'binding:updated':
            setBindings(prev => prev.map(b => b.id === (msg.binding as BindingWithRelationsInfo).id ? msg.binding as BindingWithRelationsInfo : b));
            break;
          case 'cowork:changed':
            setCoworkChanged(prev => prev + 1);
            break;
          case 'cowork:task:result':
            setLastTaskResult(msg as unknown as TaskResultEvent);
            setCoworkChanged(prev => prev + 1);
            break;
          case 'cowork:resource:changed':
            setCoworkResourceChanged(prev => prev + 1);
            break;
          case 'space:events:changed':
            // Kick from server when an agent mutated Space (create / update
            // / delete via MCP). CalendarView watches this counter to
            // re-fetch — see `useEffect` on `ws.spaceEventsRev`.
            setSpaceEventsRev(prev => prev + 1);
            break;
          case 'space:event:reminder':
            upsertNotification({
              id: (msg.id as string) ?? `notif-${Date.now()}`,
              eventId: msg.eventId as string,
              title: msg.title as string,
              startAt: msg.startAt as number,
              kind: (msg.kind as 'start' | 'reminder' | 'renotify') ?? 'reminder',
              receivedAt: Date.now(),
              read: Boolean(msg.read),
              firedAt: msg.firedAt as number | undefined,
              delayedMs: (msg.delayedMs as number) ?? 0,
            });
            break;
          case 'space:event:pending':
            upsertNotification({
              id: `pending-${msg.eventId as string}`,
              eventId: msg.eventId as string,
              title: msg.title as string,
              startAt: msg.startAt as number,
              kind: 'pending',
              receivedAt: Date.now(),
              read: false,
              triggerAt: msg.triggerAt as number,
              reminderMin: msg.reminderMin as number,
            });
            break;
          case 'permission:rules':
            setToolRules(() => {
              const rules = (msg.rules as ToolAutoAcceptRule[]) ?? [];
              saveRules(rules);
              return rules;
            });
            break;
          case 'permission:rule:added':
            setToolRules(prev => {
              const rule = msg.rule as ToolAutoAcceptRule;
              if (prev.some(r => r.id === rule.id)) return prev;
              const next = [...prev, rule];
              saveRules(next);
              return next;
            });
            break;
          case 'permission:rule:removed':
            setToolRules(prev => {
              const next = prev.filter(r => r.id !== (msg.ruleId as string));
              saveRules(next);
              return next;
            });
            break;
          case 'permission:rule:updated':
            setToolRules(prev => {
              const rule = msg.rule as ToolAutoAcceptRule;
              const next = prev.map(r => r.id === rule.id ? rule : r);
              saveRules(next);
              return next;
            });
            break;
        }
      } catch { /* ignore */ }
    };

    const connect = async () => {
      if (destroyed) return;
      // Close any existing connection to prevent overlapping sockets (e.g. StrictMode remount)
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
      if (!configRef.current) {
        try {
          const res = await fetch('/api/config');
          configRef.current = await res.json() as WsConfig;
        } catch {
          configRef.current = { wsPort: 18789, token: '' };
        }
      }
      setStatus('connecting');
      const { wsPort, token } = configRef.current;
      const ws = new WebSocket(`ws://127.0.0.1:${wsPort}`);
      wsRef.current = ws;
      ws.onopen = () => { if (token) rawSend({ type: 'connect', token }); };
      ws.onmessage = handleMsg;
      ws.onclose = () => {
        if (destroyed) return;
        setStatus('disconnected');
        const delay = Math.min(3000 * 2 ** retryCountRef.current, 15000);
        retryCountRef.current++;
        reconnectRef.current = setTimeout(connect, delay);
      };
    };

    const onFocus = () => {
      if (destroyed) return;
      if (wsRef.current?.readyState === WebSocket.OPEN || wsRef.current?.readyState === WebSocket.CONNECTING) return;
      clearTimeout(reconnectRef.current);
      retryCountRef.current = 0;
      connect();
    };
    const onVisibility = () => { if (document.visibilityState === 'visible') onFocus(); };
    window.addEventListener('focus', onFocus);
    document.addEventListener('visibilitychange', onVisibility);

    connect();

    return () => {
      destroyed = true;
      clearTimeout(reconnectRef.current);
      window.removeEventListener('focus', onFocus);
      document.removeEventListener('visibilitychange', onVisibility);
      wsRef.current?.close();
      // Clear all pending todo clear timers
      for (const t of todosClearTimers.current.values()) clearTimeout(t);
      todosClearTimers.current.clear();
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // suppress unused warning — findRequestJid is available for future use
  void findRequestJid;

  const resolvePlanExit = useCallback((selected: PlanExitOption) => {
    const req = planExitRequest;
    if (!req) return;
    rawSend({
      type: 'plan:exit:response',
      groupJid: req.groupJid,
      agentId: req.agentId,
      selected,
    });
    setPlanExitRequest(null);
  }, [planExitRequest, rawSend]);

  const dismissPlanExit = useCallback(() => {
    setPlanExitRequest(null);
  }, []);

  const setAgentMode = useCallback((jid: string, mode: AgentMode) => {
    // Optimistic UI update — the server echoes `agent:mode:changed` to confirm.
    setAgentModes((prev) => ({ ...prev, [jid]: mode }));
    rawSend({ type: 'agent:mode', groupJid: jid, mode });
  }, [rawSend]);

  // ===== Workbench reverse-ops (HTTP REST → Rust /api/workbench/...) =====
  // Unlike the old all-WS bridge, the Rust backend exposes these as REST
  // endpoints on the UI-server origin; WS is used only for push events.
  const wbUrl = (jid: string, id: string, action: string) =>
    `/api/workbench/${encodeURIComponent(jid)}/${encodeURIComponent(id)}/${action}`;

  const workbenchMarkViewed = useCallback((jid: string, artifactId: string) => {
    void fetch(wbUrl(jid, artifactId, 'mark-viewed'), { method: 'POST' }).catch(() => {});
  }, []);

  const workbenchClose = useCallback((jid: string, artifactId: string) => {
    void fetch(wbUrl(jid, artifactId, 'close'), { method: 'POST' }).catch(() => {});
    // Remove locally immediately.
    setWorkbench(prev => {
      const cur = prev[jid];
      if (!cur) return prev;
      const isCurrent = cur.current?.id === artifactId;
      return {
        ...prev,
        [jid]: {
          current: isCurrent ? null : cur.current,
          history: cur.history.filter(a => a.id !== artifactId),
        },
      };
    });
  }, []);

  const workbenchReadFile = useCallback(async (jid: string, artifactId: string, path: string): Promise<{ content?: string; error?: string }> => {
    try {
      const res = await fetch(`${wbUrl(jid, artifactId, 'read-file')}?path=${encodeURIComponent(path)}`);
      const data = await res.json() as { content?: string; error?: string };
      // If the artifact is truly gone (closed elsewhere / daemon restart),
      // prune local state. NOTE: engine_not_found is transient (agent not yet
      // running) — do NOT prune on it.
      if (data.error === 'artifact_not_found') {
        setWorkbench(prev => {
          const cur = prev[jid];
          if (!cur) return prev;
          const inState = cur.current?.id === artifactId || cur.history.some(a => a.id === artifactId);
          if (!inState) return prev;
          const isCurrent = cur.current?.id === artifactId;
          return {
            ...prev,
            [jid]: {
              current: isCurrent ? null : cur.current,
              history: cur.history.filter(a => a.id !== artifactId),
            },
          };
        });
      }
      return data;
    } catch {
      return { error: 'request_failed' };
    }
  }, []);

  const workbenchFetchLogs = useCallback(async (jid: string, artifactId: string, tailLines: number = 200): Promise<string> => {
    try {
      const res = await fetch(`${wbUrl(jid, artifactId, 'logs')}?tail=${tailLines}`);
      const data = await res.json() as { logs?: string };
      return data.logs ?? '';
    } catch {
      return '';
    }
  }, []);

  const workbenchSetCurrent = useCallback((jid: string, artifactId: string) => {
    // Promote a history artifact to current; old current goes to history top.
    setWorkbench(prev => {
      const cur = prev[jid];
      if (!cur) return prev;
      if (cur.current?.id === artifactId) return prev;
      const target = cur.history.find(a => a.id === artifactId);
      if (!target) return prev;
      const newHistory = cur.current
        ? [cur.current, ...cur.history.filter(a => a.id !== artifactId)]
        : cur.history.filter(a => a.id !== artifactId);
      return { ...prev, [jid]: { current: target, history: newHistory } };
    });
  }, []);

  return useMemo(() => ({
    status, groups, messages, agentStates, agentCompacting, agentUsage, subscribed, subscribe, sendMessage, pauseAgent, resumeAgent, stopAgent, stopAndClearHistory, resolvePermission, resolveQuestion, registerGroup, registerFeishuApp, registerQQApp, unregisterGroup, updateGroup, dispatchParents, agentTodos, subscribeAll, coworkChanged, lastTaskResult, coworkResourceChanged,
    channels, agents, bindings,
    registerChannel, registerAgent, registerBinding,
    unregisterChannel, unregisterAgent, unregisterBinding,
    updateChannel, updateAgent, updateBinding,
    toolRules, dangerouslyAcceptAll, addToolRule, removeToolRule, toggleToolRule, setDangerouslyAcceptAll,
    notifications, markNotificationRead, clearAllNotifications,
    planExitRequest, resolvePlanExit, dismissPlanExit,
    agentModes, setAgentMode,
    plansByJid, planById, requestPlanList, requestPlan,
    spaceEventsRev,
    workbench, workbenchLatest, activeJid, setActiveJid,
    workbenchMarkViewed, workbenchClose, workbenchReadFile, workbenchFetchLogs, workbenchSetCurrent,
  }), [
    status, groups, messages, agentStates, agentCompacting, agentUsage, subscribed, subscribe, sendMessage, pauseAgent, resumeAgent, stopAgent, stopAndClearHistory, resolvePermission, resolveQuestion, registerGroup, registerFeishuApp, registerQQApp, unregisterGroup, updateGroup, dispatchParents, agentTodos, subscribeAll, coworkChanged, lastTaskResult, coworkResourceChanged,
    channels, agents, bindings,
    registerChannel, registerAgent, registerBinding,
    unregisterChannel, unregisterAgent, unregisterBinding,
    updateChannel, updateAgent, updateBinding,
    toolRules, dangerouslyAcceptAll, addToolRule, removeToolRule, toggleToolRule, setDangerouslyAcceptAll,
    notifications, markNotificationRead, clearAllNotifications,
    planExitRequest, resolvePlanExit, dismissPlanExit,
    agentModes, setAgentMode,
    plansByJid, planById, requestPlanList, requestPlan,
    spaceEventsRev,
    workbench, workbenchLatest, activeJid, setActiveJid,
    workbenchMarkViewed, workbenchClose, workbenchReadFile, workbenchFetchLogs, workbenchSetCurrent,
  ]);
}
