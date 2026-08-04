import React from 'react';
import { Input, theme } from 'antd';
import type { TextAreaRef } from 'antd/es/input/TextArea';
import { getChatTextareaStyle } from './chatInputStyles';
import { useChatCompositionGuard, useGuardedChatSubmit } from './useGuardedChatSubmit';
import { useCommandSuggestions } from './useCommandSuggestions';
import type { AgentCommandItem, FileScope } from './useCommandSuggestions';

export type { AgentCommandItem } from './useCommandSuggestions';

export interface AgentCommandInputProps {
  value: string;
  disabled?: boolean;
  sending?: boolean;
  commands: AgentCommandItem[];
  mentionItems: AgentCommandItem[];
  onChange: (value: string) => void;
  onSubmit: () => void;
  /** Workspace the `@` picker lists files from. Omit to hide file suggestions. */
  fileScope?: FileScope;
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
  /**
   * Previously-sent messages in this conversation, chronological (oldest → newest).
   * Pressing ArrowUp on the first line recalls the newest, then walks backwards;
   * ArrowDown walks forward and finally restores the in-progress draft — like a
   * shell history. Omit to disable history recall.
   */
  history?: string[];
}

export function AgentCommandInput({
  value,
  disabled,
  sending,
  commands,
  mentionItems,
  onChange,
  onSubmit,
  fileScope,
  actionButtonDisabled,
  actionTitle = 'Send',
  actionAriaLabel = 'Send',
  renderActionIcon,
  placeholder = 'Nhap yeu cau... (/ command, @ file/folder, # skill)',
  onPaste,
  onFileSelect,
  renderExtraActions,
  textareaRef,
  history = [],
}: AgentCommandInputProps) {
  const { token } = theme.useToken();
  // Shell-style history recall. `histIdx === null` means "editing the live
  // draft"; a number is the position within `history` currently shown.
  const [histIdx, setHistIdx] = React.useState<number | null>(null);
  const draftRef = React.useRef('');
  const fileInputRef = React.useRef<HTMLInputElement>(null);
  const guardedSubmit = useGuardedChatSubmit(onSubmit);
  const { onCompositionStart, onCompositionEnd, shouldBlockEnterSubmit } = useChatCompositionGuard();
  const suggest = useCommandSuggestions({ value, onChange, commands, mentionItems, fileScope });

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

  // Leaving the field empty (e.g. right after a send) drops us out of history
  // recall so the next ArrowUp starts fresh from the newest message.
  React.useEffect(() => {
    if (value === '') setHistIdx(null);
  }, [value]);

  // Genuine typing exits history-recall mode; the edited text becomes the draft.
  const handleChange = (v: string) => {
    setHistIdx(null);
    onChange(v);
  };

  const navigateHistory = (dir: 'up' | 'down', el: HTMLTextAreaElement): boolean => {
    if (history.length === 0) return false;
    let nextIdx: number | null;
    if (dir === 'up') {
      if (histIdx === null) {
        draftRef.current = value; // stash the live draft before diving into history
        nextIdx = history.length - 1;
      } else {
        nextIdx = Math.max(0, histIdx - 1);
      }
    } else {
      if (histIdx === null) return false; // nothing newer than the draft
      nextIdx = histIdx >= history.length - 1 ? null : histIdx + 1;
    }
    const nextVal = nextIdx === null ? draftRef.current : history[nextIdx];
    setHistIdx(nextIdx);
    onChange(nextVal);
    // Caret to end once the controlled value has re-rendered.
    requestAnimationFrame(() => {
      try {
        el.selectionStart = el.selectionEnd = el.value.length;
      } catch {
        /* element detached */
      }
    });
    return true;
  };

  const defaultButtonDisabled = !value.trim() || !!disabled || !!sending;
  const buttonDisabled = actionButtonDisabled !== undefined ? actionButtonDisabled : defaultButtonDisabled;

  return (
    <div style={{ position: 'relative' }}>
      {suggest.popup}
      <div style={{ width: '100%', display: 'flex', gap: 12, alignItems: 'flex-end' }}>
        <Input.TextArea
          ref={textareaRef}
          value={value}
          onChange={e => handleChange(e.target.value)}
          onPaste={onPaste}
          placeholder={placeholder}
          autoSize={{ minRows: 1, maxRows: 4 }}
          disabled={disabled}
          onCompositionStart={onCompositionStart}
          onCompositionEnd={onCompositionEnd}
          onKeyDown={(e) => {
            if (suggest.handleKeyDown(e)) return;
            // History recall (only when no suggestion popup is open and no
            // modifier is held). ArrowUp works on the first line, ArrowDown on
            // the last line, so multi-line caret movement is preserved.
            if (!suggest.open && !e.shiftKey && !e.metaKey && !e.ctrlKey && !e.altKey) {
              const el = e.currentTarget;
              const caretStart = el.selectionStart ?? 0;
              const caretEnd = el.selectionEnd ?? 0;
              const collapsed = caretStart === caretEnd;
              if (e.key === 'ArrowUp' && collapsed && !value.slice(0, caretStart).includes('\n')) {
                if (navigateHistory('up', el)) {
                  e.preventDefault();
                  return;
                }
              }
              if (e.key === 'ArrowDown' && collapsed && !value.slice(caretEnd).includes('\n')) {
                if (navigateHistory('down', el)) {
                  e.preventDefault();
                  return;
                }
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
