import React from 'react';
import { Typography, theme } from 'antd';

const { Text } = Typography;

export interface AgentCommandItem {
  key: string;
  desc?: string;
  kind?: 'command' | 'file' | 'folder' | 'skill' | 'agent' | 'subagent' | 'mcp-server' | 'mcp-tool';
  insertText?: string;
}

/** Workspace the `@` picker lists files from. `path` wins over `jid`, matching
 *  `/api/chat/files`. Omit entirely to disable file suggestions. */
export interface FileScope {
  jid?: string;
  path?: string;
}

type TriggerKind = '/' | '@' | '#';

interface TriggerState {
  trigger: TriggerKind;
  query: string;
}

interface SubagentApiItem {
  name?: string;
  description?: string;
  disabled?: boolean;
}

interface McpToolApiItem {
  name?: string;
  description?: string | null;
}

interface McpServerApiItem {
  name?: string;
  description?: string | null;
  enabled?: boolean;
  builtin?: boolean;
  status?: string;
  use_tools?: string[] | null;
  tools?: McpToolApiItem[] | null;
}

function mcpToolName(serverName: string, toolName: string): string {
  return toolName.startsWith('mcp__') ? toolName : `mcp__${serverName}__${toolName}`;
}

const LABEL_BY_KIND: Record<NonNullable<AgentCommandItem['kind']>, string> = {
  command: 'Command',
  file: 'File',
  folder: 'Folder',
  skill: 'Skill',
  agent: 'Agent',
  subagent: 'Subagent',
  'mcp-server': 'MCP',
  'mcp-tool': 'MCP tool',
};

export interface UseCommandSuggestionsOptions {
  value: string;
  onChange: (value: string) => void;
  /** Extra `/` entries beyond the skill list (e.g. admin commands). */
  commands?: AgentCommandItem[];
  /** Extra `@` entries beyond files, agents and MCP targets. */
  mentionItems?: AgentCommandItem[];
  fileScope?: FileScope;
}

export interface CommandSuggestions {
  /** True while the popup is showing — callers use it to suppress their own
   *  Enter/Arrow handling so selection doesn't also submit the message. */
  open: boolean;
  /** Returns true when the key was consumed by the popup. */
  handleKeyDown: (e: React.KeyboardEvent<HTMLTextAreaElement>) => boolean;
  popup: React.ReactNode;
}

/**
 * Backing logic for the composer's `/` `@` `#` popup, shared by every chat
 * surface so the New Chat screen and an open conversation behave identically.
 *
 * `/` and `#` both list skills — `/` is what users reach for out of habit, and
 * the backend accepts either form. `@` lists workspace files first (the common
 * case) followed by agent and MCP targets.
 */
export function useCommandSuggestions({
  value,
  onChange,
  commands = [],
  mentionItems = [],
  fileScope,
}: UseCommandSuggestionsOptions): CommandSuggestions {
  const { token } = theme.useToken();
  const [activeIndex, setActiveIndex] = React.useState(0);
  const [skills, setSkills] = React.useState<AgentCommandItem[]>([]);
  const [agentMentions, setAgentMentions] = React.useState<AgentCommandItem[]>([]);
  const [mcpMentions, setMcpMentions] = React.useState<AgentCommandItem[]>([]);
  const [fileMentions, setFileMentions] = React.useState<AgentCommandItem[]>([]);

  React.useEffect(() => {
    let cancelled = false;
    fetch('/api/skills')
      .then(r => (r.ok ? r.json() : { skills: [] }))
      .then(data => {
        if (cancelled) return;
        const items: AgentCommandItem[] = (data.skills ?? []).map((s: { name?: string; description?: string }) => ({
          key: String(s.name ?? ''),
          kind: 'skill' as const,
          desc: typeof s.description === 'string' ? s.description : undefined,
        }));
        setSkills(items.filter(i => i.key));
      })
      .catch(() => {
        if (!cancelled) setSkills([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const scopeJid = fileScope?.jid ?? '';
  const scopePath = fileScope?.path ?? '';

  React.useEffect(() => {
    if (!scopeJid && !scopePath) {
      setFileMentions([]);
      return;
    }
    let cancelled = false;
    const params = new URLSearchParams();
    if (scopePath) params.set('path', scopePath);
    else params.set('jid', scopeJid);
    fetch(`/api/chat/files?${params.toString()}`)
      .then(r => (r.ok ? r.json() : { entries: [] }))
      .then(data => {
        if (cancelled) return;
        const items: AgentCommandItem[] = (data.entries ?? [])
          .filter((e: { rel?: string }) => e?.rel)
          .map((e: { rel: string; is_dir?: boolean }) => ({
            key: e.rel,
            insertText: e.is_dir ? `${e.rel}/` : e.rel,
            kind: e.is_dir ? ('folder' as const) : ('file' as const),
            desc: e.is_dir ? 'Thư mục trong workspace' : 'Đính kèm nội dung tệp vào tin nhắn',
          }));
        setFileMentions(items);
      })
      .catch(() => {
        if (!cancelled) setFileMentions([]);
      });
    return () => {
      cancelled = true;
    };
  }, [scopeJid, scopePath]);

  React.useEffect(() => {
    let cancelled = false;
    Promise.all([
      fetch('/api/subagents')
        .then(r => (r.ok ? r.json() : { subagents: [] }))
        .catch(() => ({ subagents: [] })),
      fetch('/api/mcp-servers')
        .then(r => (r.ok ? r.json() : { servers: [] }))
        .catch(() => ({ servers: [] })),
    ]).then(([agentsData, mcpData]) => {
      if (cancelled) return;

      const builtInAgents: AgentCommandItem[] = [
        {
          key: 'agent:general-purpose',
          insertText: 'agent:general-purpose',
          kind: 'agent',
          desc: 'Built-in Task subagent for research, code search, and multi-step work.',
        },
      ];

      const subagents: AgentCommandItem[] = (agentsData.subagents ?? [])
        .filter((a: SubagentApiItem) => a?.name && !a.disabled)
        .map((a: SubagentApiItem) => ({
          key: `subagent:${a.name}`,
          insertText: `subagent:${a.name}`,
          kind: 'subagent' as const,
          desc: a.description || 'Virtual subagent persona.',
        }));

      const mcpItems: AgentCommandItem[] = [];
      for (const server of (mcpData.servers ?? []) as McpServerApiItem[]) {
        const serverName = String(server.name ?? '').trim();
        if (!serverName) continue;
        const enabled = server.enabled !== false;
        if (!server.builtin && (!enabled || server.status !== 'connected')) continue;
        const status = server.status ? ` · ${server.status}` : '';
        mcpItems.push({
          key: `mcp:${serverName}`,
          insertText: `mcp:${serverName}`,
          kind: 'mcp-server',
          desc: `${enabled ? 'MCP server' : 'Disabled MCP server'}${status}${server.description ? ` · ${server.description}` : ''}`,
        });
        for (const tool of server.tools ?? []) {
          const toolName = String(tool.name ?? '').trim();
          if (!toolName) continue;
          if (server.use_tools && !server.use_tools.includes(toolName)) continue;
          const fullName = mcpToolName(serverName, toolName);
          mcpItems.push({
            key: fullName,
            insertText: fullName,
            kind: 'mcp-tool',
            desc: tool.description || `MCP tool from ${serverName}.`,
          });
        }
      }

      setAgentMentions([...builtInAgents, ...subagents]);
      setMcpMentions(mcpItems);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const triggerState = React.useMemo<TriggerState | null>(() => {
    const m = value.match(/(?:^|\s)([/@#])([^\s]*)$/);
    if (!m) return null;
    return { trigger: m[1] as TriggerKind, query: (m[2] ?? '').toLowerCase() };
  }, [value]);

  const suggestions = React.useMemo(() => {
    if (!triggerState) return [];
    const source =
      triggerState.trigger === '@'
        ? [...fileMentions, ...agentMentions, ...mcpMentions, ...mentionItems]
        : [...commands, ...skills];
    const query = triggerState.query;
    return source
      .filter(i =>
        i.key.toLowerCase().includes(query) ||
        (i.desc ?? '').toLowerCase().includes(query) ||
        (i.kind ?? '').toLowerCase().includes(query),
      )
      .slice(0, 14);
  }, [triggerState, commands, mentionItems, skills, agentMentions, mcpMentions, fileMentions]);

  React.useEffect(() => {
    setActiveIndex(0);
  }, [triggerState?.trigger, triggerState?.query]);

  const applySuggestion = (item: AgentCommandItem) => {
    if (!triggerState) return;
    const replacement = item.insertText ?? item.key;
    // Folders keep the popup open so the user can drill into a subpath; files
    // and skills get a trailing space to close it.
    const suffix = item.kind === 'folder' ? '' : ' ';
    const replaced = value.replace(/([/@#])[^\s]*$/, `${triggerState.trigger}${replacement}${suffix}`);
    onChange(replaced);
    setActiveIndex(0);
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>): boolean => {
    if (!triggerState) return false;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (suggestions.length > 0) setActiveIndex(i => Math.min(suggestions.length - 1, i + 1));
      return true;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (suggestions.length > 0) setActiveIndex(i => Math.max(0, i - 1));
      return true;
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      if (suggestions.length > 0) {
        e.preventDefault();
        applySuggestion(suggestions[activeIndex]);
      }
      return true;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      onChange(value.replace(/([/@#])[^\s]*$/, ''));
      return true;
    }
    return false;
  };

  const titleByTrigger = triggerState?.trigger === '@' ? 'Mention' : 'Skill / Command';

  const popup = triggerState ? (
    <div
      style={{
        position: 'absolute',
        left: 0,
        right: 0,
        bottom: 'calc(100% + 8px)',
        border: `1px solid ${token.colorBorderSecondary}`,
        borderRadius: 12,
        background: token.colorBgElevated,
        boxShadow: '0 12px 30px rgba(0,0,0,0.25)',
        overflow: 'hidden',
        zIndex: 40,
        display: 'grid',
        gridTemplateColumns: 'minmax(220px, 280px) 1fr',
        minHeight: 220,
        maxHeight: 320,
      }}
    >
      <div style={{ overflowY: 'auto', borderRight: `1px solid ${token.colorBorderSecondary}` }}>
        {suggestions.length === 0 ? (
          <div style={{ padding: '10px 12px' }}>
            <Text type="secondary" style={{ fontSize: 12 }}>Không tìm thấy kết quả</Text>
          </div>
        ) : (
          suggestions.map((item, idx) => {
            const active = idx === activeIndex;
            return (
              <div
                key={`${item.kind ?? ''}:${item.key}`}
                onMouseEnter={() => setActiveIndex(idx)}
                onMouseDown={e => e.preventDefault()}
                onClick={() => applySuggestion(item)}
                style={{
                  padding: '8px 12px',
                  cursor: 'pointer',
                  background: active ? token.colorFillSecondary : 'transparent',
                  color: active ? token.colorText : token.colorTextSecondary,
                  fontSize: 16,
                  fontWeight: 500,
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
                  {item.kind && (
                    <span
                      style={{
                        flex: '0 0 auto',
                        border: `1px solid ${token.colorBorderSecondary}`,
                        borderRadius: 4,
                        color: token.colorTextTertiary,
                        fontSize: 10,
                        fontWeight: 700,
                        lineHeight: '16px',
                        padding: '0 5px',
                        textTransform: 'uppercase',
                      }}
                    >
                      {LABEL_BY_KIND[item.kind]}
                    </span>
                  )}
                  <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {item.key}
                  </span>
                </div>
              </div>
            );
          })
        )}
      </div>
      <div style={{ padding: '12px 14px', overflowY: 'auto' }}>
        {suggestions[activeIndex] ? (
          <>
            <Text style={{ fontSize: 12, color: token.colorTextTertiary }}>{titleByTrigger}</Text>
            <div style={{ marginTop: 2, marginBottom: 10 }}>
              <Text strong style={{ fontSize: 18 }}>{suggestions[activeIndex].key}</Text>
            </div>
            {suggestions[activeIndex].kind && (
              <div style={{ marginBottom: 8 }}>
                <Text style={{ fontSize: 12, color: token.colorTextTertiary }}>
                  {LABEL_BY_KIND[suggestions[activeIndex].kind!]}
                </Text>
              </div>
            )}
            <Text style={{ fontSize: 14, color: token.colorTextSecondary }}>
              {suggestions[activeIndex].desc ?? 'Không có mô tả'}
            </Text>
          </>
        ) : (
          <Text type="secondary" style={{ fontSize: 13 }}>Chọn mục ở danh sách bên trái.</Text>
        )}
      </div>
    </div>
  ) : null;

  return { open: !!triggerState, handleKeyDown, popup };
}
