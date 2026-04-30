import { useState } from 'react';
import { theme, Typography } from 'antd';
import type { ChatMessage } from '../types';
import { PermissionCard, QuestionCard } from './PermissionCard';

const { Text } = Typography;

function formatTime(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  } catch { return ''; }
}

function CopyIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/>
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="20 6 9 17 4 12"/>
    </svg>
  );
}

function SaveIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/>
      <polyline points="7 10 12 15 17 10"/>
      <line x1="12" y1="15" x2="12" y2="3"/>
    </svg>
  );
}

function AgentBubble({ text, timestamp }: { text: string; timestamp: string }) {
  const [copyState, setCopyState] = useState<'idle' | 'copied'>('idle');
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved' | 'error'>('idle');
  const { token } = theme.useToken();

  const handleCopy = () => {
    navigator.clipboard.writeText(text).then(() => {
      setCopyState('copied');
      setTimeout(() => setCopyState('idle'), 2000);
    }).catch(() => {/* ignore */});
  };

  const handleSave = async () => {
    if (saveState === 'saving') return;
    setSaveState('saving');
    try {
      await fetch('/api/quicknotes', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ text }),
      });
      setSaveState('saved');
      setTimeout(() => setSaveState('idle'), 2000);
    } catch {
      setSaveState('error');
      setTimeout(() => setSaveState('idle'), 2000);
    }
  };

  return (
    <div className="max-w-[72%] group">
      <div 
        className="px-4 py-2.5 rounded-2xl rounded-tl-sm text-sm leading-relaxed whitespace-pre-wrap break-words shadow-sm border"
        style={{ 
          background: token.colorFillQuaternary,
          color: token.colorText,
          borderColor: token.colorBorderSecondary
        }}
      >
        {text}
      </div>
      <div className="flex items-center mt-1 gap-1">
        <Text type="secondary" className="text-[11px] ml-1 flex-1">{formatTime(timestamp)}</Text>
        <div className="flex gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
          <button
            onClick={handleCopy}
            title="Copy"
            className="p-1 rounded transition-colors"
            style={{
              color: copyState === 'copied' ? token.colorSuccess : token.colorTextDescription,
            }}
          >
            {copyState === 'copied' ? <CheckIcon /> : <CopyIcon />}
          </button>
          <button
            onClick={handleSave}
            title={saveState === 'error' ? 'Save failed' : 'Save as note'}
            className="p-1 rounded transition-colors"
            style={{
              color: saveState === 'saved' ? token.colorSuccess :
                     saveState === 'error' ? token.colorError :
                     saveState === 'saving' ? token.colorTextDisabled :
                     token.colorTextDescription,
              cursor: saveState === 'saving' ? 'not-allowed' : 'pointer'
            }}
          >
            {saveState === 'saved' ? <CheckIcon /> : <SaveIcon />}
          </button>
        </div>
      </div>
    </div>
  );
}

interface MessageBubbleProps {
  message: ChatMessage;
  onResolvePermission: (requestId: string, optionKey: string) => void;
  onResolveQuestion: (requestId: string, answers: Record<number, number | number[]>, otherTexts?: Record<number, string>) => void;
}

export function MessageBubble({ message, onResolvePermission, onResolveQuestion }: MessageBubbleProps) {
  const { token } = theme.useToken();

  if (message.role === 'permission') {
    return (
      <div className="flex justify-start">
        <PermissionCard message={message} onResolve={onResolvePermission} />
      </div>
    );
  }

  if (message.role === 'question') {
    return (
      <div className="flex justify-start">
        <QuestionCard message={message} onResolve={onResolveQuestion} />
      </div>
    );
  }

  const { role, text, timestamp, senderName } = message;

  if (role === 'user') {
    return (
      <div className="flex justify-end">
        <div className="max-w-[72%]">
          <div 
            className="px-4 py-2.5 rounded-2xl rounded-tr-sm text-sm leading-relaxed whitespace-pre-wrap break-words shadow-lg"
            style={{
              background: token.colorPrimary,
              color: '#fff', // User messages often look good with white text on primary color
              boxShadow: `0 4px 14px 0 ${token.colorPrimary}33`
            }}
          >
            {text}
          </div>
          <Text type="secondary" className="text-[10px] font-medium mt-1 text-right pr-1 block">
            {formatTime(timestamp)}
          </Text>
        </div>
      </div>
    );
  }

  const isAgent = role === 'agent';

  return (
    <div className="flex gap-2.5 items-end">
      {/* Avatar */}
      <div 
        className="w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0 mb-5 text-[10px] font-bold shadow-lg"
        style={{
          background: isAgent ? token.colorPrimary : token.colorFillSecondary,
          color: isAgent ? '#fff' : token.colorText,
          boxShadow: isAgent ? `0 4px 14px 0 ${token.colorPrimary}33` : undefined
        }}
      >
        {isAgent ? 'AI' : (senderName?.charAt(0).toUpperCase() ?? '?')}
      </div>
      {isAgent ? (
        <AgentBubble text={text} timestamp={timestamp} />
      ) : (
        <div className="max-w-[72%]">
          {senderName && (
            <Text type="secondary" className="text-[10px] font-bold tracking-wider mb-1 ml-1 uppercase block">
              {senderName}
            </Text>
          )}
          <div 
            className="px-4 py-2.5 rounded-2xl rounded-tl-sm text-sm leading-relaxed whitespace-pre-wrap break-words shadow-sm border"
            style={{ 
              background: token.colorFillQuaternary,
              color: token.colorText,
              borderColor: token.colorBorderSecondary
            }}
          >
            {text}
          </div>
          <Text type="secondary" className="text-[10px] font-medium mt-1 ml-1 block">
            {formatTime(timestamp)}
          </Text>
        </div>
      )}
    </div>
  );
}

export function TypingIndicator() {
  const { token } = theme.useToken();

  return (
    <div className="flex gap-2.5 items-end">
      <div 
        className="w-7 h-7 rounded-full flex items-center justify-center flex-shrink-0 mb-5 text-[10px] font-bold shadow-lg"
        style={{
          background: token.colorPrimary,
          color: '#fff',
          boxShadow: `0 4px 14px 0 ${token.colorPrimary}33`
        }}
      >
        AI
      </div>
      <div 
        className="px-4 py-3 rounded-2xl rounded-tl-sm shadow-sm border"
        style={{ 
          background: token.colorFillQuaternary,
          borderColor: token.colorBorderSecondary
        }}
      >
        <div className="flex gap-1 items-center h-4">
          {[0, 150, 300].map(delay => (
            <span
              key={delay}
              className="w-1.5 h-1.5 rounded-full animate-bounce"
              style={{ 
                animationDelay: `${delay}ms`,
                background: token.colorPrimary,
                opacity: 0.6
              }}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
