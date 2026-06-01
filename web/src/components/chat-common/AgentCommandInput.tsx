import React from 'react';
import { Input, Typography, theme } from 'antd';
import type { TextAreaRef } from 'antd/es/input/TextArea';
import { getChatActionButtonStyle, getChatTextareaStyle } from './chatInputStyles';
import { useChatCompositionGuard, useGuardedChatSubmit } from './useGuardedChatSubmit';

const { Text } = Typography;

export interface AgentCommandItem {
  key: string;
  desc?: string;
  kind?: 'command' | 'file' | 'skill' | 'agent' | 'subagent' | 'mcp-server' | 'mcp-tool';
  insertText?: string;
}

type TriggerKind = '/' | '@' | '#';

interface TriggerState {
  trigger: TriggerKind;
  query: string;
}

export interface AgentCommandInputProps {
  value: string;
  disabled?: boolean;
  sending?: boolean;
  commands: AgentCommandItem[];
  mentionItems: AgentCommandItem[];
  onChange: (value: string) => void;
  onSubmit: () => void;
  /** Khi có: override điều kiện disabled nút (vd. pause/resume không cần text). */
  actionButtonDisabled?: boolean;
  actionTitle?: string;
  actionAriaLabel?: string;
  renderActionIcon?: React.ReactNode;
  placeholder?: string;
  onPaste?: (e: React.ClipboardEvent<HTMLTextAreaElement>) => void;
  onFileSelect?: (files: File[]) => void;
  renderExtraActions?: React.ReactNode;
  textareaRef?: React.Ref<TextAreaRef>;
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

export function AgentCommandInput({
  value,
  disabled,
  sending,
  commands,
  mentionItems,
  onChange,
  onSubmit,
  actionButtonDisabled,
  actionTitle = 'Send',
  actionAriaLabel = 'Send',
  renderActionIcon,
  placeholder = 'Nhap yeu cau... (/ command, @ file/folder, # skill)',
  onPaste,
  onFileSelect,
  renderExtraActions,
  textareaRef,
}: AgentCommandInputProps) {
  const { token } = theme.useToken();
  const [activeIndex, setActiveIndex] = React.useState(0);
  const [skills, setSkills] = React.useState<AgentCommandItem[]>([]);
  const [agentMentions, setAgentMentions] = React.useState<AgentCommandItem[]>([]);
  const [mcpMentions, setMcpMentions] = React.useState<AgentCommandItem[]>([]);
  const fileInputRef = React.useRef<HTMLInputElement>(null);
  const guardedSubmit = useGuardedChatSubmit(onSubmit);
  const { onCompositionStart, onCompositionEnd, shouldBlockEnterSubmit } = useChatCompositionGuard();

  const handleFileButtonClick = () => {
    fileInputRef.current?.click();
  };

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files;
    if (files && files.length > 0 && onFileSelect) {
      onFileSelect(Array.from(files));
    }
    // Reset input so same file can be selected again
    e.target.value = '';
  };

  React.useEffect(() => {
    let cancelled = false;
    fetch('/api/skills')
      .then(r => (r.ok ? r.json() : { skills: [] }))
      .then(data => {
        if (cancelled) return;
        const items: AgentCommandItem[] = (data.skills ?? []).map((s: { name?: string; description?: string }) => ({
          key: String(s.name ?? ''),
          kind: 'skill',
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
      triggerState.trigger === '/'
        ? commands
        : triggerState.trigger === '@'
          ? [...agentMentions, ...mcpMentions, ...mentionItems]
          : skills;
    return source
      .filter(i => {
        const query = triggerState.query;
        return (
          i.key.toLowerCase().includes(query) ||
          (i.desc ?? '').toLowerCase().includes(query) ||
          (i.kind ?? '').toLowerCase().includes(query)
        );
      })
      .slice(0, 14);
  }, [triggerState, commands, mentionItems, skills, agentMentions, mcpMentions]);

  React.useEffect(() => {
    setActiveIndex(0);
  }, [triggerState?.trigger, triggerState?.query]);

  const applySuggestion = (item: AgentCommandItem) => {
    if (!triggerState) return;
    const replacement = item.insertText ?? item.key;
    const replaced = value.replace(/([/@#])[^\s]*$/, `${triggerState.trigger}${replacement} `);
    onChange(replaced);
    setActiveIndex(0);
  };

  const titleByTrigger = triggerState?.trigger === '/' ? 'Command' : triggerState?.trigger === '@' ? 'Mention' : 'Skill';
  const labelByKind: Record<NonNullable<AgentCommandItem['kind']>, string> = {
    command: 'Command',
    file: 'File',
    skill: 'Skill',
    agent: 'Agent',
    subagent: 'Subagent',
    'mcp-server': 'MCP',
    'mcp-tool': 'MCP tool',
  };

  const defaultButtonDisabled = !value.trim() || !!disabled || !!sending;
  const buttonDisabled = actionButtonDisabled !== undefined ? actionButtonDisabled : defaultButtonDisabled;

  return (
    <div style={{ position: 'relative' }}>
      {triggerState && (
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
                <Text type="secondary" style={{ fontSize: 12 }}>Khong tim thay ket qua</Text>
              </div>
            ) : (
              suggestions.map((item, idx) => {
                const active = idx === activeIndex;
                return (
                  <div
                    key={item.key}
                    onMouseEnter={() => setActiveIndex(idx)}
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
                          {labelByKind[item.kind]}
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
                      {labelByKind[suggestions[activeIndex].kind!]}
                    </Text>
                  </div>
                )}
                <Text style={{ fontSize: 14, color: token.colorTextSecondary }}>
                  {suggestions[activeIndex].desc ?? 'Khong co mo ta'}
                </Text>
              </>
            ) : (
              <Text type="secondary" style={{ fontSize: 13 }}>Chon muc o danh sach ben trai.</Text>
            )}
          </div>
        </div>
      )}
      <div style={{ width: '100%', display: 'flex', gap: 12, alignItems: 'flex-end' }}>
        <Input.TextArea
          ref={textareaRef}
          value={value}
          onChange={e => onChange(e.target.value)}
          onPaste={onPaste}
          placeholder={placeholder}
          autoSize={{ minRows: 1, maxRows: 4 }}
          disabled={disabled}
          onCompositionStart={onCompositionStart}
          onCompositionEnd={onCompositionEnd}
          onKeyDown={(e) => {
            if (triggerState) {
              if (e.key === 'ArrowDown') {
                e.preventDefault();
                if (suggestions.length > 0) setActiveIndex(i => Math.min(suggestions.length - 1, i + 1));
                return;
              }
              if (e.key === 'ArrowUp') {
                e.preventDefault();
                if (suggestions.length > 0) setActiveIndex(i => Math.max(0, i - 1));
                return;
              }
              if (e.key === 'Enter' || e.key === 'Tab') {
                if (suggestions.length > 0) {
                  e.preventDefault();
                  applySuggestion(suggestions[activeIndex]);
                }
                return;
              }
              if (e.key === 'Escape') {
                e.preventDefault();
                onChange(value.replace(/([/@#])[^\s]*$/, ''));
                return;
              }
            }
            if (e.key === 'Enter' && !e.shiftKey) {
              if (shouldBlockEnterSubmit(e)) return;
              e.preventDefault();
              if (buttonDisabled) return;
              guardedSubmit();
            }
          }}
          style={{
            ...getChatTextareaStyle(token),
            borderRadius: 12,
            resize: 'none',
            minHeight: 44,
            border: `1px solid ${token.colorBorderSecondary}`,
            transition: 'all 0.2s ease-in-out',
          }}
          onFocus={(e) => {
            e.currentTarget.style.borderColor = token.colorPrimary;
            e.currentTarget.style.boxShadow = `0 0 0 3px ${token.colorPrimaryBg}`;
          }}
          onBlur={(e) => {
            e.currentTarget.style.borderColor = token.colorBorderSecondary;
            e.currentTarget.style.boxShadow = 'none';
          }}
        />
        {onFileSelect && (
          <>
            <input
              ref={fileInputRef}
              type="file"
              onChange={handleFileChange}
              style={{ display: 'none' }}
              accept="image/*"
              multiple
            />
            <button
              type="button"
              onClick={handleFileButtonClick}
              disabled={disabled}
              className="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0"
              style={{
                background: disabled ? token.colorFillTertiary : token.colorBgContainer,
                color: disabled ? token.colorTextTertiary : token.colorTextSecondary,
                border: `1px solid ${disabled ? token.colorBorder : token.colorBorderSecondary}`,
                cursor: disabled ? 'not-allowed' : 'pointer',
                transition: 'all 0.2s ease-in-out',
              }}
              onMouseEnter={(e) => {
                if (!disabled) {
                  e.currentTarget.style.background = token.colorFillSecondary;
                  e.currentTarget.style.borderColor = token.colorPrimary;
                  e.currentTarget.style.color = token.colorPrimary;
                }
              }}
              onMouseLeave={(e) => {
                if (!disabled) {
                  e.currentTarget.style.background = token.colorBgContainer;
                  e.currentTarget.style.borderColor = token.colorBorderSecondary;
                  e.currentTarget.style.color = token.colorTextSecondary;
                }
              }}
              aria-label="Attach file"
              title="Attach image file"
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                fill="none"
                viewBox="0 0 24 24"
                strokeWidth={1.5}
                stroke="currentColor"
                className="w-5 h-5"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  d="M18.375 12.739l-7.693 7.693a4.5 4.5 0 01-6.364-6.364l10.94-10.94A3 3 0 1120.5 7.372V8.25M17 19.5v-2.25m-5.625-5.625h3.75"
                />
              </svg>
            </button>
          </>
        )}
        {renderExtraActions}
        <button
          type="button"
          onClick={() => {
            if (buttonDisabled) return;
            guardedSubmit();
          }}
          disabled={buttonDisabled}
          className="w-10 h-10 rounded-full flex items-center justify-center flex-shrink-0"
          style={{
            background: buttonDisabled ? token.colorFillTertiary : token.colorPrimary,
            color: buttonDisabled ? token.colorTextTertiary : '#ffffff',
            cursor: buttonDisabled ? 'not-allowed' : 'pointer',
            border: buttonDisabled ? `1px solid ${token.colorBorder}` : 'none',
            transition: 'all 0.2s ease-in-out',
            boxShadow: buttonDisabled ? 'none' : '0 2px 8px rgba(0, 0, 0, 0.15)',
          }}
          onMouseEnter={(e) => {
            if (!buttonDisabled) {
              e.currentTarget.style.background = token.colorPrimaryHover;
              e.currentTarget.style.transform = 'scale(1.05)';
              e.currentTarget.style.boxShadow = '0 4px 12px rgba(0, 0, 0, 0.2)';
            }
          }}
          onMouseLeave={(e) => {
            if (!buttonDisabled) {
              e.currentTarget.style.background = token.colorPrimary;
              e.currentTarget.style.transform = 'scale(1)';
              e.currentTarget.style.boxShadow = '0 2px 8px rgba(0, 0, 0, 0.15)';
            }
          }}
          aria-label={actionAriaLabel}
          title={actionTitle}
        >
          {renderActionIcon ?? (
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
              <path d="M3.478 2.405a.75.75 0 00-.926.94l2.432 7.905H13.5a.75.75 0 010 1.5H4.984l-2.432 7.905a.75.75 0 00.926.94 60.519 60.519 0 0018.445-8.986.75.75 0 000-1.218A60.517 60.517 0 003.478 2.405z" />
            </svg>
          )}
        </button>
      </div>
    </div>
  );
}
