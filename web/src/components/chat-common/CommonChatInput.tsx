import React from 'react';
import { theme } from 'antd';
import { getChatActionButtonStyle, getChatTextareaStyle } from './chatInputStyles';
import { useChatCompositionGuard, useGuardedChatSubmit } from './useGuardedChatSubmit';
import { AgentModeSelector } from './AgentModeSelector';
import type { AgentMode } from '../../hooks/useWebSocket';

export interface CommonChatInputProps {
  className?: string;
  helperText?: string;
  placeholder?: string;
  disabled?: boolean;
  actionDisabled?: boolean;
  actionTitle?: string;
  actionAriaLabel?: string;
  renderActionIcon?: React.ReactNode;
  value?: string;
  onChange?: (value: string) => void;
  onSubmit?: () => void;
  /** Custom input row (e.g. AgentCommandInput). When set, default textarea + send are omitted. */
  children?: React.ReactNode;
  /**
   * Active agent mode for this chat. When provided alongside `onModeChange`,
   * a Plan/Agent mode dropdown is rendered below the textarea. Omit the props
   * to hide the selector entirely (cowork / virtual JIDs etc.).
   */
  agentMode?: AgentMode;
  onModeChange?: (mode: AgentMode) => void;
}

function DefaultPlaneIcon() {
  return (
    <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
      <path d="M3.478 2.404a.75.75 0 0 0-.926.941l2.432 7.905H13.5a.75.75 0 0 1 0 1.5H4.984l-2.432 7.905a.75.75 0 0 0 .926.94 60.519 60.519 0 0 0 18.445-8.986.75.75 0 0 0 0-1.218A60.517 60.517 0 0 0 3.478 2.404Z" />
    </svg>
  );
}

export function CommonChatInput({
  className = '',
  helperText,
  placeholder = 'Message…',
  disabled = false,
  actionDisabled = false,
  actionTitle = 'Send',
  actionAriaLabel = 'Send',
  renderActionIcon,
  value = '',
  onChange,
  onSubmit,
  children,
  agentMode,
  onModeChange,
}: CommonChatInputProps) {
  const { token } = theme.useToken();
  const placeholderClass = `common-chat-ph-${React.useId().replace(/[^a-zA-Z0-9_-]/g, '') || 'x'}`;
  const guardedSubmit = useGuardedChatSubmit(onSubmit);
  const { onCompositionStart, onCompositionEnd, shouldBlockEnterSubmit } = useChatCompositionGuard();

  const submit = () => {
    if (actionDisabled) return;
    guardedSubmit();
  };

  const keyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      if (shouldBlockEnterSubmit(e)) return;
      e.preventDefault();
      submit();
    }
  };

  return (
    <div className={className}>
      {children ? (
        <div className="w-full">{children}</div>
      ) : (
        <div className="flex w-full items-end gap-3">
          <style>{`.${placeholderClass}::placeholder { color: ${token.colorTextPlaceholder}; opacity: 1; }`}</style>
          <textarea
            value={value}
            onChange={e => onChange?.(e.target.value)}
            onCompositionStart={onCompositionStart}
            onCompositionEnd={onCompositionEnd}
            onKeyDown={keyDown}
            disabled={disabled}
            placeholder={placeholder}
            rows={1}
            className={`${placeholderClass} min-h-[44px] max-h-32 min-w-0 flex-1 resize-none text-sm outline-none`}
            style={{
              ...getChatTextareaStyle(token),
              borderRadius: 16,
              padding: '8px 12px',
            }}
          />
          <button
            type="button"
            onClick={submit}
            disabled={actionDisabled}
            title={actionTitle}
            aria-label={actionAriaLabel}
            className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-full transition-opacity disabled:cursor-not-allowed"
            style={getChatActionButtonStyle(token, actionDisabled)}
          >
            {renderActionIcon ?? <DefaultPlaneIcon />}
          </button>
        </div>
      )}
      {(agentMode && onModeChange) || helperText ? (
        <div
          className="flex items-center justify-between gap-2 mt-1.5 px-1"
          style={{ color: token.colorTextTertiary, fontSize: 11 }}
        >
          {agentMode && onModeChange ? (
            <AgentModeSelector mode={agentMode} onChange={onModeChange} disabled={disabled} />
          ) : (
            <span />
          )}
          {helperText ? <span>{helperText}</span> : <span />}
        </div>
      ) : null}
    </div>
  );
}
