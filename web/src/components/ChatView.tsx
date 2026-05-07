import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { theme } from 'antd';
import type { GroupInfo, ChatMessage, AgentState, UsageData } from '../types';
import { MessageBubble, TypingIndicator } from './MessageBubble';
import { Progress, Space, Typography } from 'antd';
import { AgentCommandInput, CommonChatInput } from './chat-common';

const { Text } = Typography;

interface Props {
  group: GroupInfo;
  messages: ChatMessage[];
  agentState: AgentState;
  usage?: UsageData;
  /** While compacting, pause is disabled; shows "Compacting…" */
  isCompacting: boolean;
  onSend: (text: string) => void;
  onPause: () => void;
  onResume: (query?: string) => void;
  onStop: () => void;
  onResolvePermission: (requestId: string, optionKey: string) => void;
  onResolveQuestion: (requestId: string, answers: Record<number, number | number[]>, otherTexts?: Record<number, string>) => void;
}

const PAGE_SIZE = 5;

export function ChatView({ group, messages, agentState, usage, isCompacting, onSend, onPause, onResume, onStop, onResolvePermission, onResolveQuestion }: Props) {
  const { token } = theme.useToken();
  const [input, setInput]           = useState('');
  const [showStopConfirm, setShowStopConfirm] = useState(false);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const bottomRef                   = useRef<HTMLDivElement>(null);
  const scrollRef                   = useRef<HTMLDivElement>(null);
  const prevMessagesLenRef          = useRef(messages.length);
  const prevGroupJidRef             = useRef(group.jid);
  const preserveScrollRef           = useRef<{ prevHeight: number; prevTop: number } | null>(null);

  const isProcessing = agentState === 'processing';
  const isPaused     = agentState === 'paused';
  const isActive     = isProcessing || isPaused; // agent has work in progress

  // Reset pagination when switching groups
  useEffect(() => {
    if (prevGroupJidRef.current !== group.jid) {
      prevGroupJidRef.current = group.jid;
      setVisibleCount(PAGE_SIZE);
      prevMessagesLenRef.current = messages.length;
    }
  }, [group.jid, messages.length]);

  const visibleMessages = messages.slice(Math.max(0, messages.length - visibleCount));
  const hasMore = messages.length > visibleCount;

  // On group switch / initial mount: jump scroll to bottom synchronously after layout
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [group.jid]);

  // First time messages arrive for a group (0 → N), pin to bottom
  useLayoutEffect(() => {
    if (prevMessagesLenRef.current === 0 && messages.length > 0) {
      const el = scrollRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    }
  }, [messages.length]);

  // Restore scroll position after loading older messages (prepended at top)
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && preserveScrollRef.current) {
      const { prevHeight, prevTop } = preserveScrollRef.current;
      el.scrollTop = el.scrollHeight - prevHeight + prevTop;
      preserveScrollRef.current = null;
    }
  }, [visibleCount]);

  // Auto-scroll to bottom only when a NEW message is appended (not when loading older)
  useEffect(() => {
    const prevLen = prevMessagesLenRef.current;
    if (messages.length > prevLen) {
      // grow visible count so the new message is shown
      const delta = messages.length - prevLen;
      setVisibleCount(c => c + delta);
      requestAnimationFrame(() => {
        const el = scrollRef.current;
        if (el) el.scrollTop = el.scrollHeight;
      });
    }
    prevMessagesLenRef.current = messages.length;
  }, [messages.length]);

  // Auto-scroll on typing indicator toggle
  useEffect(() => {
    if (isProcessing) {
      const el = scrollRef.current;
      if (el) el.scrollTop = el.scrollHeight;
    }
  }, [isProcessing]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    if (el.scrollTop <= 0 && hasMore) {
      preserveScrollRef.current = { prevHeight: el.scrollHeight, prevTop: el.scrollTop };
      setVisibleCount(c => Math.min(messages.length, c + PAGE_SIZE));
    }
  };

  // ── Send / pause / resume single handler ──
  const handleActionButton = () => {
    if (isProcessing) {
      // No-op while compacting (button disabled; belt-and-suspenders)
      if (isCompacting) return;
      onPause();
      return;
    }
    if (isPaused) {
      const text = input.trim();
      onResume(text || undefined);
      setInput('');
      return;
    }
    // idle: normal send
    const text = input.trim();
    if (!text) return;
    onSend(text);
    setInput('');
  };

  // ── Action button disabled rules ──
  const actionButtonDisabled =
    (agentState === 'idle' && !input.trim()) ||   // idle: need text to send
    (isProcessing && isCompacting);               // compacting: pause disabled

  const actionButtonTitle =
    isProcessing
      ? (isCompacting ? 'Compacting context, please wait…' : 'Pause')
      : isPaused
      ? 'Resume'
      : 'Send';

  // ── Status line ──
  const statusText =
    isCompacting  ? 'Compacting…'
    : isProcessing ? 'Thinking…'
    : isPaused     ? 'Paused'
    : 'Ready';

  const statusDotClass =
    isProcessing ? 'bg-yellow-400 animate-pulse'
    : isPaused   ? 'bg-orange-400'
    : 'bg-green-400';

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div 
        className="flex items-center px-6 py-4 backdrop-blur-xl border-b flex-shrink-0"
        style={{ 
          background: `${token.colorBgContainer}cc`, // transparent background
          borderColor: token.colorBorderSecondary 
        }}
      >
        <div>
          <h2 className="font-semibold" style={{ color: token.colorText }}>{group.name}</h2>
          <p className="text-xs mt-0.5" style={{ color: token.colorTextSecondary }}>{group.folder}</p>
        </div>
        <div className="ml-auto flex items-center gap-6">
          {/* Token usage indicator */}
          {usage && (
            <div className="flex items-center gap-3" style={{ minWidth: 120 }}>
              <div className="flex-1">
                <div className="flex justify-between items-center mb-1">
                  <Text style={{ fontSize: '10px', color: token.colorTextTertiary, fontWeight: 500 }}>
                    Tokens
                  </Text>
                  <Text style={{ fontSize: '10px', color: token.colorTextTertiary, fontWeight: 600 }}>
                    {Math.round((usage.useTokens / usage.maxTokens) * 100)}%
                  </Text>
                </div>
                <Progress
                  percent={(usage.useTokens / usage.maxTokens) * 100}
                  showInfo={false}
                  size={[100, 3]}
                  strokeColor={
                    usage.useTokens > usage.maxTokens * 0.9 
                      ? token.colorError 
                      : usage.useTokens > usage.maxTokens * 0.7 
                        ? token.colorWarning 
                        : token.colorPrimary
                  }
                  trailColor={token.colorFillSecondary}
                  style={{ margin: 0, display: 'block' }}
                />
              </div>
              <div className="flex flex-col items-end">
                <Text style={{ fontSize: '10px', color: token.colorTextSecondary, fontWeight: 600, lineHeight: 1 }}>
                  {usage.useTokens.toLocaleString()}
                </Text>
                <Text style={{ fontSize: '9px', color: token.colorTextTertiary, lineHeight: 1.5 }}>
                  / {usage.maxTokens.toLocaleString()}
                </Text>
              </div>
            </div>
          )}

          {/* Status indicator */}
          <div className="flex items-center gap-2">
            <span className={`w-2 h-2 rounded-full transition-colors ${statusDotClass}`} />
            <span className="text-xs" style={{ color: token.colorTextSecondary }}>{statusText}</span>
          </div>
          {/* Stop / reset — always shown; in idle clears session context */}
          <button
            onClick={() => setShowStopConfirm(true)}
            className="w-7 h-7 rounded-full flex items-center justify-center transition-colors"
            style={{ color: token.colorTextDescription }}
            onMouseEnter={(e) => { e.currentTarget.style.color = token.colorError; e.currentTarget.style.background = `${token.colorError}1a`; }}
            onMouseLeave={(e) => { e.currentTarget.style.color = token.colorTextDescription; e.currentTarget.style.background = 'transparent'; }}
            title="Reset session"
            aria-label="Reset session"
          >
            {/* Refresh / reset icon */}
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="w-4 h-4">
              <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
              <path d="M3 3v5h5" />
            </svg>
          </button>
        </div>
      </div>

      {/* Messages */}
      <div ref={scrollRef} onScroll={handleScroll} className="flex-1 overflow-y-auto px-6 py-5 space-y-4 bg-transparent">
        {messages.length === 0 && (
          <div className="flex flex-col items-center justify-center h-full gap-4 select-none">
            <div className="relative">
              <div 
                className="absolute inset-0 blur-[40px] opacity-10 rounded-full" 
                style={{ background: token.colorPrimary }} 
              />
              <img src="/logo.png" alt="" className="w-12 h-12 opacity-20 relative z-10" />
            </div>
            <p 
              className="text-xs font-bold tracking-widest uppercase"
              style={{ color: token.colorTextDescription }}
            >
              Start a conversation
            </p>
          </div>
        )}
        {hasMore && (
          <div className="flex justify-center">
            <button
              onClick={() => {
                const el = scrollRef.current;
                if (el) preserveScrollRef.current = { prevHeight: el.scrollHeight, prevTop: el.scrollTop };
                setVisibleCount(c => Math.min(messages.length, c + PAGE_SIZE));
              }}
              className="text-[10px] font-bold tracking-wider uppercase px-4 py-2 rounded-full border transition-colors"
              style={{
                color: token.colorTextSecondary,
                background: token.colorFillAlter,
                borderColor: token.colorBorderSecondary,
              }}
              onMouseEnter={(e) => { e.currentTarget.style.color = token.colorText; e.currentTarget.style.background = token.colorFillSecondary; }}
              onMouseLeave={(e) => { e.currentTarget.style.color = token.colorTextSecondary; e.currentTarget.style.background = token.colorFillAlter; }}
            >
              Load older messages
            </button>
          </div>
        )}
        {visibleMessages.map(msg => (
          <MessageBubble
            key={msg.id}
            message={msg}
            onResolvePermission={onResolvePermission}
            onResolveQuestion={onResolveQuestion}
          />
        ))}
        {isProcessing && <TypingIndicator />}
        <div ref={bottomRef} />
      </div>

      {/* Input area — AgentCommandInput gợi ý / @ #; chống submit đôi trong CommonChatInput + hook */}
      <CommonChatInput
        className="px-6 py-4 backdrop-blur-xl flex-shrink-0"
        helperText={isPaused
          ? 'Press ▶ to resume · / @ # gợi ý · Shift+Enter xuống dòng'
          : 'Enter để gửi · Shift+Enter xuống dòng · / @ # gợi ý'}
      >
        <AgentCommandInput
          value={input}
          onChange={setInput}
          onSubmit={handleActionButton}
          disabled={isProcessing}
          sending={false}
          commands={[]}
          mentionItems={[]}
          actionButtonDisabled={actionButtonDisabled}
          actionTitle={actionButtonTitle}
          actionAriaLabel={actionButtonTitle}
          renderActionIcon={
            isProcessing ? (
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
                <path fillRule="evenodd" d="M6.75 5.25a.75.75 0 0 1 .75-.75H9a.75.75 0 0 1 .75.75v13.5a.75.75 0 0 1-.75.75H7.5a.75.75 0 0 1-.75-.75V5.25zm7.5 0A.75.75 0 0 1 15 4.5h1.5a.75.75 0 0 1 .75.75v13.5a.75.75 0 0 1-.75.75H15a.75.75 0 0 1-.75-.75V5.25z" clipRule="evenodd" />
              </svg>
            ) : isPaused ? (
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" className="w-4 h-4">
                <path fillRule="evenodd" d="M4.5 5.653c0-1.427 1.529-2.33 2.779-1.643l11.54 6.347c1.295.712 1.295 2.573 0 3.286L7.28 19.99c-1.25.687-2.779-.217-2.779-1.643V5.653z" clipRule="evenodd" />
              </svg>
            ) : undefined
          }
          placeholder={isPaused ? 'Add instructions or leave empty to continue…' : 'Message…'}
        />
      </CommonChatInput>

      {/* Stop confirmation modal */}
      {showStopConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
          <div 
            className="border rounded-2xl shadow-2xl p-6 w-80 flex flex-col gap-4"
            style={{ 
              background: token.colorBgElevated,
              borderColor: token.colorBorderSecondary 
            }}
          >
            <div className="flex items-center gap-3">
              <span className="w-9 h-9 rounded-full bg-red-500/10 flex items-center justify-center flex-shrink-0">
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="w-5 h-5 text-red-500">
                  <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                  <path d="M3 3v5h5" />
                </svg>
              </span>
              <h3 className="font-semibold" style={{ color: token.colorText }}>Reset session?</h3>
            </div>
            <p className="text-sm leading-relaxed" style={{ color: token.colorTextSecondary }}>
              {isActive
                ? 'Current task will be terminated and all conversation context will be discarded. This cannot be undone.'
                : 'All conversation context will be cleared and a new session will start. This cannot be undone.'}
            </p>
            <div className="flex gap-3 justify-end">
              <button
                onClick={() => setShowStopConfirm(false)}
                className="px-4 py-2 text-sm font-medium rounded-xl transition-colors"
                style={{ color: token.colorTextSecondary }}
                onMouseEnter={(e) => { e.currentTarget.style.background = token.colorFillAlter; e.currentTarget.style.color = token.colorText; }}
                onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = token.colorTextSecondary; }}
              >
                Cancel
              </button>
              <button
                onClick={() => { setShowStopConfirm(false); onStop(); }}
                className="px-4 py-2 text-sm rounded-xl text-white transition-colors"
                style={{ background: token.colorError }}
              >
                Terminate
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
