import React from 'react';
import { Input, Typography, theme } from 'antd';
import { getChatActionButtonStyle, getChatTextareaStyle } from '../../chat-common';

const { Text } = Typography;

export interface AgentCommandItem {
  key: string;
  desc?: string;
}

type TriggerKind = '/' | '@' | '#';

interface TriggerState {
  trigger: TriggerKind;
  query: string;
}

interface Props {
  value: string;
  disabled?: boolean;
  sending?: boolean;
  commands: AgentCommandItem[];
  mentionItems: AgentCommandItem[];
  onChange: (value: string) => void;
  onSubmit: () => void;
}

export function AgentCommandInput({
  value,
  disabled,
  sending,
  commands,
  mentionItems,
  onChange,
  onSubmit,
}: Props) {
  const { token } = theme.useToken();
  const [activeIndex, setActiveIndex] = React.useState(0);
  const [skills, setSkills] = React.useState<AgentCommandItem[]>([]);

  React.useEffect(() => {
    let cancelled = false;
    fetch('/api/skills')
      .then(r => (r.ok ? r.json() : { skills: [] }))
      .then(data => {
        if (cancelled) return;
        const items: AgentCommandItem[] = (data.skills ?? []).map((s: any) => ({
          key: String(s.name ?? ''),
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

  const triggerState = React.useMemo<TriggerState | null>(() => {
    const m = value.match(/(?:^|\s)([\/@#])([^\s]*)$/);
    if (!m) return null;
    return { trigger: m[1] as TriggerKind, query: (m[2] ?? '').toLowerCase() };
  }, [value]);

  const suggestions = React.useMemo(() => {
    if (!triggerState) return [];
    const source =
      triggerState.trigger === '/'
        ? commands
        : triggerState.trigger === '@'
          ? mentionItems
          : skills;
    return source
      .filter(i => i.key.toLowerCase().includes(triggerState.query))
      .slice(0, 14);
  }, [triggerState, commands, mentionItems, skills]);

  React.useEffect(() => {
    setActiveIndex(0);
  }, [triggerState?.trigger, triggerState?.query]);

  const applySuggestion = (item: AgentCommandItem) => {
    if (!triggerState) return;
    const replaced = value.replace(/([\/@#])[^\s]*$/, `${triggerState.trigger}${item.key} `);
    onChange(replaced);
    setActiveIndex(0);
  };

  const titleByTrigger = triggerState?.trigger === '/' ? 'Command' : triggerState?.trigger === '@' ? 'File/Folder' : 'Skill';

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
                    {item.key}
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
          value={value}
          onChange={e => onChange(e.target.value)}
          placeholder="Nhap yeu cau... (/ command, @ file/folder, # skill)"
          autoSize={{ minRows: 1, maxRows: 4 }}
          disabled={disabled}
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
                onChange(value.replace(/([\/@#])[^\s]*$/, ''));
                return;
              }
            }
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              onSubmit();
            }
          }}
          style={{
            ...getChatTextareaStyle(token),
            borderRadius: 16,
            resize: 'none',
            minHeight: 44,
          }}
        />
        <button
          onClick={onSubmit}
          disabled={!value.trim() || !!disabled || !!sending}
          className="w-10 h-10 rounded-full flex items-center justify-center transition-colors flex-shrink-0"
          style={getChatActionButtonStyle(token, !value.trim() || !!disabled || !!sending)}
          aria-label="Send"
          title="Send"
        >
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
            <path d="M3.478 2.405a.75.75 0 00-.926.94l2.432 7.905H13.5a.75.75 0 010 1.5H4.984l-2.432 7.905a.75.75 0 00.926.94 60.519 60.519 0 0018.445-8.986.75.75 0 000-1.218A60.517 60.517 0 003.478 2.405z" />
          </svg>
        </button>
      </div>
    </div>
  );
}
