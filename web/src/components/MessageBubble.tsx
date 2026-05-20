import { useState } from 'react';
import { theme, Typography } from 'antd';
import { BulbFilled } from '@ant-design/icons';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import 'highlight.js/styles/github-dark.css'; 
import 'highlight.js/styles/github.css'; 
import type { ChatMessage, ImageAttachment } from '../types';
import { PermissionCard, QuestionCard } from './PermissionCard';
import { useAppContext } from '../contexts/AppContext';
import { extractLeadingReasoningBlocks } from '../utils/reasoningBlocks';

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

function SparkleIcon({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden
    >
      <path
        d="M12 2l1.2 4.2L18 8l-4.8 1.8L12 14l-1.2-4.2L6 8l4.8-1.8L12 2zM19 14l.7 2.5 2.5.7-2.5.7-.7 2.5-.7-2.5-2.5-.7 2.5-.7.7-2.5zM5 16l.6 2.1 2.1.6-2.1.6-.6 2.1-.6-2.1-2.1-.6 2.1-.6.6-2.1z"
        fill="currentColor"
        opacity="0.9"
      />
    </svg>
  );
}

/**
 * Compact "thinking" indicator — visually matches `ToolGroupCard` so a
 * conversation reads as one consistent timeline:
 *
 *   ● Read 3 files, ran 1 command  ›
 *   ◐ think                         ›
 *   ● cog add                       ›
 *
 * Collapsed = single inline row (icon + "think" + chevron) at the same
 * font size / spacing as a tool group. Expanded = indented italic body
 * with the same left-border treatment ToolGroupCard uses.
 *
 * Kept here (not in ToolGroupCard) because reasoning isn't a real tool
 * call — no result/status semantics — but the visual language is shared.
 */
function ReasoningCollapsible({
  markdown,
  isDarkMode,
  embedded: _embedded,
}: {
  markdown: string;
  isDarkMode: boolean;
  embedded?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const { token } = theme.useToken();

  return (
    <div style={{ margin: '4px 0', padding: 0, background: 'transparent' }}>
      <button
        type="button"
        onClick={() => setOpen(v => !v)}
        aria-expanded={open}
        title={open ? 'Thu gọn phần suy luận' : 'Mở xem tiến trình tư duy'}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 8,
          background: 'transparent',
          border: 'none',
          padding: '4px 0',
          cursor: 'pointer',
          color: token.colorTextSecondary,
          fontSize: 13,
          textAlign: 'left',
          width: '100%',
        }}
      >
        {/* Same icon family + sizing as ToolGroupCard's CheckCircleFilled
            so a "think" row is visually indistinguishable from a tool row in
            the timeline. Color uses colorInfo (soft blue) to mark this as
            reasoning, not a green success — same shape, different semantic. */}
        <BulbFilled style={{ color: token.colorInfo, fontSize: 11 }} />
        <span style={{ color: 'inherit' }}>think</span>
        <span style={{ color: token.colorTextQuaternary }}>
          {open ? '▾' : '›'}
        </span>
      </button>

      {open && (
        <div
          style={{
            marginTop: 6,
            marginLeft: 18,
            paddingLeft: 12,
            borderLeft: `2px solid ${token.colorBorderSecondary}`,
          }}
        >
          <div
            className={`prose prose-sm max-w-none italic opacity-95 ${isDarkMode ? 'prose-invert' : ''}`}
            style={{ color: token.colorTextSecondary, fontSize: 12 }}
          >
            <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
              {markdown}
            </ReactMarkdown>
          </div>
        </div>
      )}
    </div>
  );
}

function MarkdownContent({ content, isDarkMode }: { content: string, isDarkMode: boolean }) {
  if (typeof content !== 'string') return null;
  
  return (
    <div className={`prose ${isDarkMode ? 'prose-invert' : ''} max-w-none`}>
      <ReactMarkdown 
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          a: ({ ...props }) => <a {...props} target="_blank" rel="noopener noreferrer" className="text-blue-400 hover:underline" />,
          p: ({ children }) => <p className="mb-2 last:mb-0">{children}</p>,
          pre: ({ children }) => <pre className="p-0 m-0 bg-transparent">{children}</pre>
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

function ImageAttachments({ attachments }: { attachments: ImageAttachment[] }) {
  if (!attachments || attachments.length === 0) return null;
  
  return (
    <div className="flex flex-wrap gap-2 mt-2">
      {attachments.map((img, i) => (
        <img
          key={i}
          src={img.dataUrl}
          alt=""
          className="max-w-[200px] max-h-[200px] object-cover rounded-lg border"
        />
      ))}
    </div>
  );
}

function AgentBubble({ text, timestamp, isDarkMode, attachments }: { text: string; timestamp: string; isDarkMode: boolean; attachments?: ImageAttachment[] }) {
  const [copyState, setCopyState] = useState<'idle' | 'copied'>('idle');
  const [saveState, setSaveState] = useState<'idle' | 'saving' | 'saved' | 'error'>('idle');
  const { token } = theme.useToken();
  const { reasoning, body } = extractLeadingReasoningBlocks(text);

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
    <div className="max-w-[85%] group">
      <div 
        className="px-4 py-2.5 rounded-2xl rounded-tl-sm shadow-sm border"
        style={{ 
          background: token.colorFillQuaternary,
          color: token.colorText,
          borderColor: token.colorBorderSecondary
        }}
      >
        {reasoning ? (
          <ReasoningCollapsible markdown={reasoning} isDarkMode={isDarkMode} embedded />
        ) : null}
        {reasoning && body ? (
          <div
            className="my-2 border-t"
            style={{ borderColor: token.colorBorderSecondary }}
            aria-hidden
          />
        ) : null}
        {body ? <MarkdownContent content={body} isDarkMode={isDarkMode} /> : null}
        <ImageAttachments attachments={attachments || []} />
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
  const { isDarkMode } = useAppContext();

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

  const { role, text, timestamp, senderName, attachments } = message;

  if (role === 'user') {
    return (
      <div className="flex justify-end">
        <div className="max-w-[85%]">
          <div 
            className="px-4 py-2.5 rounded-2xl rounded-tr-sm shadow-lg"
            style={{
              background: token.colorPrimary,
              color: '#fff',
              boxShadow: `0 4px 14px 0 ${token.colorPrimary}33`
            }}
          >
            <MarkdownContent content={text} isDarkMode={true} />
            <ImageAttachments attachments={attachments || []} />
          </div>
          <Text type="secondary" className="text-[10px] font-medium mt-1 text-right pr-1 block">
            {formatTime(timestamp)}
          </Text>
        </div>
      </div>
    );
  }

  const isAgent = role === 'agent';

  // Reasoning-only fast path: an agent message that's pure <thinking> with
  // no follow-on text and no attachments shouldn't carry the full avatar +
  // chat-bubble chrome — it'd look like a "message" the model said when in
  // fact it's just reasoning preamble that belongs in the tool-row
  // timeline. Render the inline ToolGroupCard-style "think" row instead.
  if (isAgent && (!attachments || attachments.length === 0)) {
    const { reasoning, body } = extractLeadingReasoningBlocks(text);
    if (reasoning && !body.trim()) {
      return (
        <div className="ml-[38px]" /* matches avatar (28px) + gap (10px) so it aligns with bubbles */>
          <ReasoningCollapsible markdown={reasoning} isDarkMode={isDarkMode} />
        </div>
      );
    }
  }

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
        <AgentBubble text={text} timestamp={timestamp} isDarkMode={isDarkMode} attachments={attachments} />
      ) : (
        <div className="max-w-[85%]">
          {senderName && (
            <Text type="secondary" className="text-[10px] font-bold tracking-wider mb-1 ml-1 uppercase block">
              {senderName}
            </Text>
          )}
          <div 
            className="px-4 py-2.5 rounded-2xl rounded-tl-sm shadow-sm border"
            style={{ 
              background: token.colorFillQuaternary,
              color: token.colorText,
              borderColor: token.colorBorderSecondary
            }}
          >
            <MarkdownContent content={text} isDarkMode={isDarkMode} />
            <ImageAttachments attachments={attachments || []} />
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
