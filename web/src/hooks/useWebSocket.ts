import { useCallback, useEffect, useRef, useState, useMemo } from 'react';
import type { GroupInfo, ChatMessage, TextMessage, AgentState, WsStatus, PermissionMessage, QuestionMessage, RegisterGroupPayload, UpdateGroupPayload, DispatchParent, AgentTodosEntry, UsageData, ChannelInfo, AgentInfo, BindingInfo, BindingWithRelationsInfo, RegisterChannelPayload, RegisterAgentPayload, RegisterBindingPayload, UpdateChannelPayload, UpdateAgentPayload, UpdateBindingPayload } from '../types';

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
  sendMessage: (jid: string, text: string) => void;
  /** Pause agent (sends agent:control pause) */
  pauseAgent: (jid: string) => void;
  /** Resume agent, optional follow-up text (sends agent:control resume) */
  resumeAgent: (jid: string, query?: string) => void;
  /** Stop and reset agent session (sends agent:control stop) */
  stopAgent: (jid: string) => void;
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
  const [agentUsage, setAgentUsage]           = useState<Record<string, UsageData>>({});
  const [channels, setChannels] = useState<ChannelInfo[]>([]);
  const [agents, setAgents]     = useState<AgentInfo[]>([]);
  const [bindings, setBindings] = useState<BindingWithRelationsInfo[]>([]);

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

  const sendMessage = useCallback((jid: string, text: string) => {
    addMessage(jid, {
      id:        `local-${Date.now()}`,
      role:      'user',
      text,
      timestamp: new Date().toISOString(),
    });
    rawSend({ type: 'message', groupJid: jid, text });
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
            const msgs = msg.messages as Array<{ id: string; role: string; senderName?: string; text: string; timestamp: string }>;
            if (Array.isArray(msgs)) {
              setMessages(prev => ({
                ...prev,
                [hjid]: msgs.map(m => ({
                  id:         m.id as string,
                  role:       (m.role === 'agent' ? 'agent' : 'user'),
                  senderName: m.senderName,
                  text:       m.text as string,
                  timestamp:  m.timestamp as string,
                } as TextMessage)) as ChatMessage[],
              }));
            }
            break;
          }
          case 'incoming':
            if (msg.isFromMe) break;
            addMessage(msg.groupJid as string, {
              id:         `in-${Date.now()}-${Math.random()}`,
              role:       'other',
              senderName: msg.senderName as string,
              text:       msg.text as string,
              timestamp:  msg.timestamp as string,
            });
            break;
          case 'agent:reply':
            addMessage(msg.groupJid as string, {
              id:        `agent-${Date.now()}-${Math.random()}`,
              role:      'agent',
              text:      msg.text as string,
              timestamp: new Date().toISOString(),
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
            // eslint-disable-next-line no-console
            console.debug('[ws] dispatch:update', {
              parents: newParents.length,
              tasks: newParents.reduce((n, p) => n + p.tasks.length, 0),
              statuses: newParents.map(p => `${p.id}:${p.status}(${p.tasks.length})`),
            });
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
          case 'agent:todos': {
            const todoJid = msg.agentJid as string;
            const todosArr = msg.todos as AgentTodosEntry['todos'];
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

  return useMemo(() => ({ 
    status, groups, messages, agentStates, agentCompacting, agentUsage, subscribed, subscribe, sendMessage, pauseAgent, resumeAgent, stopAgent, resolvePermission, resolveQuestion, registerGroup, registerFeishuApp, registerQQApp, unregisterGroup, updateGroup, dispatchParents, agentTodos, subscribeAll,
    channels, agents, bindings,
    registerChannel, registerAgent, registerBinding,
    unregisterChannel, unregisterAgent, unregisterBinding,
    updateChannel, updateAgent, updateBinding,
  }), [
    status, groups, messages, agentStates, agentCompacting, agentUsage, subscribed, subscribe, sendMessage, pauseAgent, resumeAgent, stopAgent, resolvePermission, resolveQuestion, registerGroup, registerFeishuApp, registerQQApp, unregisterGroup, updateGroup, dispatchParents, agentTodos, subscribeAll,
    channels, agents, bindings,
    registerChannel, registerAgent, registerBinding,
    unregisterChannel, unregisterAgent, unregisterBinding,
    updateChannel, updateAgent, updateBinding,
  ]);
}
