import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { theme } from 'antd';
import type { GroupInfo, ChatMessage, ToolMessage, AgentState, UsageData, ImageAttachment } from '../types';
import type { AgentMode } from '../hooks/useWebSocket';
import type { TextAreaRef } from 'antd/es/input/TextArea';
import { MessageBubble, TypingIndicator } from './MessageBubble';
import { ToolGroupCard } from './ToolGroupCard';
import { Progress, Tooltip, Typography, Drawer, Badge, message } from 'antd';
import { AudioMutedOutlined, AudioOutlined, FileTextOutlined, LoadingOutlined, ThunderboltOutlined } from '@ant-design/icons';
import { AgentCommandInput, CommonChatInput } from './chat-common';
import { PlanHistoryPanel } from './PlanHistoryPanel';
import { useAppContext } from '../contexts/AppContext';

const { Text } = Typography;

interface Props {
  group: GroupInfo;
  messages: ChatMessage[];
  agentState: AgentState;
  usage?: UsageData;
  /** While compacting, pause is disabled; shows "Compacting…" */
  isCompacting: boolean;
  onSend: (text: string, attachments: ImageAttachment[]) => void;
  onPause: () => void;
  onResume: (query?: string) => void;
  onStop: () => void;
  onStopAndClear: () => void;
  onResolvePermission: (requestId: string, optionKey: string) => void;
  onResolveQuestion: (requestId: string, answers: Record<number, number | number[]>, otherTexts?: Record<number, string>) => void;
  /** Active agent mode for this chat (defaults to 'Agent' when undefined). */
  agentMode?: AgentMode;
  onModeChange?: (mode: AgentMode) => void;
}

const PAGE_SIZE = 5;

/** Encode mono Float32 PCM as a 16-bit PCM WAV blob for the Whisper endpoint. */
function encodeWav(samples: Float32Array, sampleRate: number): Blob {
  const buffer = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(buffer);
  const write = (offset: number, s: string) => {
    for (let i = 0; i < s.length; i += 1) view.setUint8(offset + i, s.charCodeAt(i));
  };
  write(0, 'RIFF');
  view.setUint32(4, 36 + samples.length * 2, true);
  write(8, 'WAVE');
  write(12, 'fmt ');
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  write(36, 'data');
  view.setUint32(40, samples.length * 2, true);
  let offset = 44;
  for (let i = 0; i < samples.length; i += 1, offset += 2) {
    const x = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, x < 0 ? x * 0x8000 : x * 0x7fff, true);
  }
  return new Blob([view], { type: 'audio/wav' });
}

export function ChatView({ group, messages, agentState, usage, isCompacting, onSend, onPause, onResume, onStop, onStopAndClear, onResolvePermission, onResolveQuestion, agentMode, onModeChange }: Props) {
  const { token } = theme.useToken();
  const { ws } = useAppContext();
  const [input, setInput]           = useState('');
  const [pendingImages, setPendingImages] = useState<ImageAttachment[]>([]);
  const [showStopConfirm, setShowStopConfirm] = useState(false);
  const [plansOpen, setPlansOpen]   = useState(false);
  const [contextOpen, setContextOpen] = useState(false);
  const [compacting, setCompacting] = useState(false);
  const [recording, setRecording]   = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [recordElapsed, setRecordElapsed] = useState(0);
  const planCount = (ws.plansByJid[group.jid] ?? []).length;
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const bottomRef                   = useRef<HTMLDivElement>(null);
  const scrollRef                   = useRef<HTMLDivElement>(null);
  const textareaRef                 = useRef<TextAreaRef>(null);
  const audioCtxRef                 = useRef<AudioContext | null>(null);
  const audioProcessorRef           = useRef<ScriptProcessorNode | null>(null);
  const audioSourceRef              = useRef<MediaStreamAudioSourceNode | null>(null);
  const audioStreamRef              = useRef<MediaStream | null>(null);
  const audioChunksRef              = useRef<Float32Array[]>([]);
  const recordTimerRef              = useRef<number | null>(null);
  const prevMessagesLenRef          = useRef(messages.length);
  const prevGroupJidRef             = useRef(group.jid);
  const preserveScrollRef           = useRef<{ prevHeight: number; prevTop: number } | null>(null);

  const isProcessing = agentState === 'processing';
  const isPaused     = agentState === 'paused';
  const isActive     = isProcessing || isPaused; // agent has work in progress

  const cleanupRecording = () => {
    audioProcessorRef.current?.disconnect();
    audioSourceRef.current?.disconnect();
    audioStreamRef.current?.getTracks().forEach((t) => t.stop());
    audioCtxRef.current?.close().catch(() => {});
    if (recordTimerRef.current) window.clearInterval(recordTimerRef.current);
    audioProcessorRef.current = null;
    audioSourceRef.current = null;
    audioStreamRef.current = null;
    audioCtxRef.current = null;
    recordTimerRef.current = null;
  };

  useEffect(() => cleanupRecording, []);

  // Reset pagination when switching groups
  useEffect(() => {
    if (prevGroupJidRef.current !== group.jid) {
      prevGroupJidRef.current = group.jid;
      setVisibleCount(PAGE_SIZE);
      prevMessagesLenRef.current = messages.length;
    }
  }, [group.jid, messages.length]);

  /**
   * Render order = strict chronological by `timestamp` ascending.
   *
   * WHY: live `tool:execution` events and `agent:reply` events arrive on
   * the WebSocket in network-order, which often disagrees with the order
   * the LLM actually emitted them. E.g. a final assistant text bubble can
   * be appended to `messages` before its preceding tool-call events land,
   * causing the chat to read "[final answer] · [tool calls]" — confusing
   * since the answer obviously came AFTER the tools that produced it.
   *
   * We sort by `timestamp` here (every ChatMessage variant carries one).
   * `Array.prototype.sort` is stable in modern engines, so messages with
   * identical timestamps keep their insertion order — that's what we want
   * as a tie-break (server-side stable order wins).
   *
   * We sort the FULL message list first, then slice the tail, so paging
   * boundaries don't accidentally hide tool calls that belong to the
   * displayed window because their timestamps shuffle near the edge.
   */
  const sortedMessages = useMemo(() => {
    return [...messages].sort((a, b) => {
      const ta = Date.parse(a.timestamp) || 0;
      const tb = Date.parse(b.timestamp) || 0;
      return ta - tb;
    });
  }, [messages]);
  const visibleMessages = sortedMessages.slice(
    Math.max(0, sortedMessages.length - visibleCount),
  );
  const hasMore = sortedMessages.length > visibleCount;

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

  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const imageItems = Array.from(e.clipboardData.items).filter(item => item.type.startsWith('image/'));
    if (imageItems.length === 0) return;
    
    for (const item of imageItems) {
      const file = item.getAsFile();
      if (!file) continue;
      const srcMime = item.type;
      
      // createImageBitmap with imageOrientation:'from-image' applies EXIF rotation before drawing
      createImageBitmap(file, { imageOrientation: 'from-image' } as ImageBitmapOptions)
        .then(bitmap => {
          const canvas = document.createElement('canvas');
          canvas.width = bitmap.width;
          canvas.height = bitmap.height;
          canvas.getContext('2d')!.drawImage(bitmap, 0, 0);
          bitmap.close();
          // Keep PNG for screenshots; use JPEG for photos to reduce payload size
          const outMime = srcMime === 'image/png' ? 'image/png' : 'image/jpeg';
          const dataUrl = canvas.toDataURL(outMime, outMime === 'image/jpeg' ? 0.92 : undefined);
          setPendingImages(prev => [...prev, { dataUrl, mimeType: outMime }]);
        })
        .catch(() => {
          // Fallback: no EXIF normalization, but at least something shows up
          const reader = new FileReader();
          reader.onload = () => {
            setPendingImages(prev => [...prev, { dataUrl: reader.result as string, mimeType: srcMime }]);
          };
          reader.readAsDataURL(file);
        });
    }
  };

  const handleFileSelect = (files: File[]) => {
    for (const file of files) {
      if (!file.type.startsWith('image/')) continue; // Only handle images for now
      
      const srcMime = file.type;
      
      // createImageBitmap with imageOrientation:'from-image' applies EXIF rotation before drawing
      createImageBitmap(file, { imageOrientation: 'from-image' } as ImageBitmapOptions)
        .then(bitmap => {
          const canvas = document.createElement('canvas');
          canvas.width = bitmap.width;
          canvas.height = bitmap.height;
          canvas.getContext('2d')!.drawImage(bitmap, 0, 0);
          bitmap.close();
          // Keep PNG for screenshots; use JPEG for photos to reduce payload size
          const outMime = srcMime === 'image/png' ? 'image/png' : 'image/jpeg';
          const dataUrl = canvas.toDataURL(outMime, outMime === 'image/jpeg' ? 0.92 : undefined);
          setPendingImages(prev => [...prev, { dataUrl, mimeType: outMime }]);
        })
        .catch(() => {
          // Fallback: no EXIF normalization, but at least something shows up
          const reader = new FileReader();
          reader.onload = () => {
            setPendingImages(prev => [...prev, { dataUrl: reader.result as string, mimeType: srcMime }]);
          };
          reader.readAsDataURL(file);
        });
    }
  };

  const transcribeAudio = async (blob: Blob, filename: string) => {
    setTranscribing(true);
    try {
      const fd = new FormData();
      fd.append('audio', blob, filename);
      const res = await fetch('/api/whisper/transcribe', { method: 'POST', body: fd });
      const raw = await res.text();
      if (!res.ok) {
        message.error(`Nhận diện thất bại: ${raw}`);
        return;
      }
      const data = JSON.parse(raw) as { text?: string };
      const text = (data.text ?? '').trim();
      if (!text) {
        message.info('Không nhận được văn bản từ bản ghi');
        return;
      }
      setInput((prev) => {
        const prefix = prev.trimEnd();
        return prefix ? `${prefix} ${text}` : text;
      });
      requestAnimationFrame(() => textareaRef.current?.focus());
    } catch (e: any) {
      message.error(`Lỗi nhận diện: ${e?.message ?? e}`);
    } finally {
      setTranscribing(false);
    }
  };

  const startRecording = async () => {
    if (isProcessing || transcribing) return;
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const ctx = new AudioContext();
      const source = ctx.createMediaStreamSource(stream);
      const processor = ctx.createScriptProcessor(4096, 1, 1);
      audioChunksRef.current = [];
      processor.onaudioprocess = (e) => {
        audioChunksRef.current.push(new Float32Array(e.inputBuffer.getChannelData(0)));
      };
      source.connect(processor);
      processor.connect(ctx.destination);
      audioCtxRef.current = ctx;
      audioSourceRef.current = source;
      audioProcessorRef.current = processor;
      audioStreamRef.current = stream;
      setRecordElapsed(0);
      recordTimerRef.current = window.setInterval(() => setRecordElapsed((s) => s + 1), 1000);
      setRecording(true);
    } catch (e: any) {
      cleanupRecording();
      message.error(`Không truy cập được micro: ${e?.message ?? e}`);
    }
  };

  const stopRecordingAndTranscribe = async () => {
    const sampleRate = audioCtxRef.current?.sampleRate ?? 48000;
    const chunks = audioChunksRef.current;
    cleanupRecording();
    setRecording(false);

    const total = chunks.reduce((n, c) => n + c.length, 0);
    if (total === 0) {
      message.warning('Bản ghi rỗng');
      return;
    }
    const merged = new Float32Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }
    await transcribeAudio(encodeWav(merged, sampleRate), 'chat-recording.wav');
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
    if (!text && pendingImages.length === 0) return;
    onSend(text, pendingImages);
    setInput('');
    setPendingImages([]);
  };

  // ── Action button disabled rules ──
  const actionButtonDisabled =
    (agentState === 'idle' && !input.trim() && pendingImages.length === 0) ||   // idle: need text or images to send
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
          {/* Token usage indicator with remaining-context countdown */}
          {usage && usage.maxTokens > 0 && (() => {
            const pct = Math.min(100, (usage.useTokens / usage.maxTokens) * 100);
            const remaining = Math.max(0, usage.maxTokens - usage.useTokens);
            const barColor =
              pct >= 90 ? token.colorError : pct >= 70 ? token.colorWarning : token.colorPrimary;
            return (
              <Tooltip
                title={`Đã dùng ${usage.useTokens.toLocaleString()} / ${usage.maxTokens.toLocaleString()} tokens — còn lại ${remaining.toLocaleString()}`}
              >
                <div className="flex items-center gap-3" style={{ minWidth: 140 }}>
                  <div className="flex-1">
                    <div className="flex justify-between items-center mb-1">
                      <Text style={{ fontSize: '10px', color: token.colorTextTertiary, fontWeight: 500 }}>
                        Context
                      </Text>
                      <Text style={{ fontSize: '10px', color: token.colorTextTertiary, fontWeight: 600 }}>
                        {Math.round(pct)}%
                      </Text>
                    </div>
                    <Progress
                      percent={pct}
                      showInfo={false}
                      size={[100, 3]}
                      strokeColor={barColor}
                      trailColor={token.colorFillSecondary}
                      style={{ margin: 0, display: 'block' }}
                    />
                  </div>
                  <div className="flex flex-col items-end">
                    <Text style={{ fontSize: '11px', color: barColor, fontWeight: 700, lineHeight: 1 }}>
                      {remaining.toLocaleString()}
                    </Text>
                    <Text style={{ fontSize: '9px', color: token.colorTextTertiary, lineHeight: 1.5 }}>
                      còn lại
                    </Text>
                  </div>
                </div>
              </Tooltip>
            );
          })()}

          {/* Status indicator */}
          <div className="flex items-center gap-2">
            <span className={`w-2 h-2 rounded-full transition-colors ${statusDotClass}`} />
            <span className="text-xs" style={{ color: token.colorTextSecondary }}>{statusText}</span>
          </div>
          {/* View Context button — always shown; drawer handles empty state */}
          <Tooltip title={usage && usage.maxTokens > 0 ? `Context: ${Math.round(Math.min(100, (usage.useTokens / usage.maxTokens) * 100))}% used` : 'View context'}>
            <button
              onClick={() => setContextOpen(true)}
              className="w-7 h-7 rounded-full flex items-center justify-center transition-colors relative"
              style={{ color: token.colorTextDescription }}
              onMouseEnter={(e) => { e.currentTarget.style.color = token.colorPrimary; e.currentTarget.style.background = `${token.colorPrimary}1a`; }}
              onMouseLeave={(e) => { e.currentTarget.style.color = token.colorTextDescription; e.currentTarget.style.background = 'transparent'; }}
              aria-label="View context"
            >
              {/* database / layers icon */}
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="w-4 h-4">
                <ellipse cx="12" cy="5" rx="9" ry="3" />
                <path d="M3 5v4c0 1.66 4.03 3 9 3s9-1.34 9-3V5" />
                <path d="M3 9v4c0 1.66 4.03 3 9 3s9-1.34 9-3V9" />
                <path d="M3 13v4c0 1.66 4.03 3 9 3s9-1.34 9-3v-4" />
              </svg>
              {/* small usage dot — visible only when we have data */}
              {usage && usage.maxTokens > 0 && (() => {
                const pct = Math.min(100, (usage.useTokens / usage.maxTokens) * 100);
                const dotColor = pct >= 90 ? token.colorError : pct >= 70 ? token.colorWarning : token.colorPrimary;
                return (
                  <span
                    style={{
                      position: 'absolute', top: 1, right: 1,
                      width: 5, height: 5, borderRadius: '50%',
                      background: dotColor,
                    }}
                  />
                );
              })()}
            </button>
          </Tooltip>

          {/* Plan history — browse past plans this agent produced via ExitPlanMode */}
          <Tooltip title="Plan history">
            <button
              onClick={() => setPlansOpen(true)}
              className="w-7 h-7 rounded-full flex items-center justify-center transition-colors"
              style={{ color: token.colorTextDescription }}
              onMouseEnter={(e) => { e.currentTarget.style.color = token.colorPrimary; e.currentTarget.style.background = `${token.colorPrimary}1a`; }}
              onMouseLeave={(e) => { e.currentTarget.style.color = token.colorTextDescription; e.currentTarget.style.background = 'transparent'; }}
              aria-label="Plan history"
            >
              <Badge count={planCount} size="small" offset={[2, -2]} styles={{ indicator: { fontSize: 9 } }}>
                <FileTextOutlined style={{ fontSize: 15 }} />
              </Badge>
            </button>
          </Tooltip>
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
        {/* Render with consecutive ToolMessages collapsed into one card —
            claude-code style "Read 3 files, edited 1, ran 1 command ›". */}
        {renderMessagesWithToolGroups(
          visibleMessages,
          onResolvePermission,
          onResolveQuestion,
        )}
        {isProcessing && <TypingIndicator />}
        <div ref={bottomRef} />
      </div>

      {/* Input area — AgentCommandInput gợi ý / @ #; chống submit đôi trong CommonChatInput + hook */}
      {/* Pending image previews */}
      {pendingImages.length > 0 && (
        <div className="px-6 py-2 flex flex-wrap gap-2 backdrop-blur-xl flex-shrink-0" style={{ background: `${token.colorBgContainer}cc`, borderColor: token.colorBorderSecondary }}>
          {pendingImages.map((img, i) => (
            <div key={i} className="relative group flex-shrink-0">
              <img
                src={img.dataUrl}
                alt=""
                className="w-16 h-16 object-cover rounded-xl border shadow-sm"
                style={{ borderColor: token.colorBorderSecondary }}
              />
              <button
                onClick={() => setPendingImages(prev => prev.filter((_, j) => j !== i))}
                className="absolute -top-1.5 -right-1.5 w-5 h-5 rounded-full bg-gray-700 hover:bg-gray-900 text-white text-xs leading-none flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
                aria-label="Remove image"
              >
                ×
              </button>
            </div>
          ))}
        </div>
      )}
      <CommonChatInput
        className="px-6 py-4 backdrop-blur-xl flex-shrink-0"
        helperText={isPaused
          ? 'Press ▶ to resume · / @ # gợi ý · Shift+Enter xuống dòng'
          : 'Enter để gửi · Shift+Enter xuống dòng · / @ # gợi ý'}
        agentMode={agentMode}
        onModeChange={onModeChange}
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
          onPaste={handlePaste}
          onFileSelect={handleFileSelect}
          textareaRef={textareaRef}
          renderExtraActions={
            <Tooltip title={recording ? `Dừng & nhận diện (${recordElapsed}s)` : transcribing ? 'Đang nhận diện…' : 'Ghi âm bằng Whisper'}>
              <button
                type="button"
                onClick={recording ? stopRecordingAndTranscribe : startRecording}
                disabled={!recording && (isProcessing || transcribing)}
                className="w-9 h-9 rounded-lg flex items-center justify-center flex-shrink-0"
                style={{
                  background: recording
                    ? `${token.colorError}1a`
                    : transcribing || isProcessing
                      ? token.colorFillTertiary
                      : token.colorBgContainer,
                  color: recording
                    ? token.colorError
                    : transcribing || isProcessing
                      ? token.colorTextTertiary
                      : token.colorTextSecondary,
                  border: `1px solid ${recording ? token.colorError : token.colorBorderSecondary}`,
                  cursor: !recording && (transcribing || isProcessing) ? 'not-allowed' : 'pointer',
                  transition: 'all 0.2s ease-in-out',
                }}
                aria-label={recording ? 'Stop recording and transcribe' : 'Record voice'}
              >
                {transcribing ? (
                  <LoadingOutlined style={{ fontSize: 16 }} />
                ) : recording ? (
                  <AudioMutedOutlined style={{ fontSize: 16 }} />
                ) : (
                  <AudioOutlined style={{ fontSize: 16 }} />
                )}
              </button>
            </Tooltip>
          }
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
          placeholder={isPaused ? 'Add instructions or leave empty to continue…' : 'Message… (paste image with Ctrl+V / ⌘V)'}
        />
      </CommonChatInput>

      {/* Stop confirmation modal */}
      {showStopConfirm && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
          <div 
            className="border rounded-2xl shadow-2xl p-6 w-96 flex flex-col gap-4"
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

            {/* Three action rows */}
            <div className="flex flex-col gap-2 pt-1">
              {/* Reset only */}
              <button
                onClick={() => { setShowStopConfirm(false); onStop(); }}
                className="w-full px-4 py-2.5 text-sm rounded-xl text-white font-medium transition-all flex items-center gap-2"
                style={{ background: token.colorError }}
                onMouseEnter={(e) => { e.currentTarget.style.opacity = '0.88'; }}
                onMouseLeave={(e) => { e.currentTarget.style.opacity = '1'; }}
              >
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="w-4 h-4 flex-shrink-0">
                  <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
                  <path d="M3 3v5h5" />
                </svg>
                <span>Reset session only</span>
                <span className="ml-auto text-xs opacity-70">Keeps history log</span>
              </button>

              {/* Reset + clear history */}
              <button
                onClick={() => { setShowStopConfirm(false); onStopAndClear(); }}
                className="w-full px-4 py-2.5 text-sm rounded-xl font-medium transition-all flex items-center gap-2"
                style={{
                  background: `${token.colorWarning}22`,
                  color: token.colorWarning,
                  border: `1px solid ${token.colorWarning}55`,
                }}
                onMouseEnter={(e) => { e.currentTarget.style.background = `${token.colorWarning}33`; }}
                onMouseLeave={(e) => { e.currentTarget.style.background = `${token.colorWarning}22`; }}
              >
                <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="w-4 h-4 flex-shrink-0">
                  <polyline points="3 6 5 6 21 6" />
                  <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
                  <path d="M10 11v6M14 11v6" />
                  <path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
                </svg>
                <span>Reset + clear history</span>
                <span className="ml-auto text-xs opacity-70">Deletes log permanently</span>
              </button>

              {/* Cancel */}
              <button
                onClick={() => setShowStopConfirm(false)}
                className="w-full px-4 py-2 text-sm font-medium rounded-xl transition-colors"
                style={{ color: token.colorTextSecondary }}
                onMouseEnter={(e) => { e.currentTarget.style.background = token.colorFillAlter; e.currentTarget.style.color = token.colorText; }}
                onMouseLeave={(e) => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = token.colorTextSecondary; }}
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Plan history drawer */}
      <Drawer
        title="Plan history"
        placement="right"
        width={460}
        open={plansOpen}
        onClose={() => setPlansOpen(false)}
        styles={{ body: { padding: 0 } }}
      >
        <PlanHistoryPanel
          groupJid={group.jid}
          plansByJid={ws.plansByJid}
          planById={ws.planById}
          requestPlanList={ws.requestPlanList}
          requestPlan={ws.requestPlan}
        />
      </Drawer>

      {/* ── View Context drawer ──────────────────────────────────── */}
      <Drawer
        title={
          <div className="flex items-center gap-2">
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ width: 16, height: 16 }}>
              <ellipse cx="12" cy="5" rx="9" ry="3" />
              <path d="M3 5v4c0 1.66 4.03 3 9 3s9-1.34 9-3V5" />
              <path d="M3 9v4c0 1.66 4.03 3 9 3s9-1.34 9-3V9" />
              <path d="M3 13v4c0 1.66 4.03 3 9 3s9-1.34 9-3v-4" />
            </svg>
            <span>Context Window</span>
          </div>
        }
        placement="right"
        width={400}
        open={contextOpen}
        onClose={() => setContextOpen(false)}
      >
        {usage && usage.maxTokens > 0 ? (() => {
          const pct = Math.min(100, (usage.useTokens / usage.maxTokens) * 100);
          const remaining = Math.max(0, usage.maxTokens - usage.useTokens);
          const barColor =
            pct >= 90 ? token.colorError : pct >= 70 ? token.colorWarning : token.colorPrimary;
          return (
            <div style={{ padding: '20px 24px', display: 'flex', flexDirection: 'column', gap: 24 }}>

              {/* Donut-style arc replaced by a clean progress ring using SVG */}
              <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 12 }}>
                {/* SVG arc gauge */}
                {(() => {
                  const r = 54; const cx = 64; const cy = 64;
                  const circ = 2 * Math.PI * r;
                  const used = (pct / 100) * circ;
                  return (
                    <svg width={128} height={128} viewBox="0 0 128 128">
                      {/* Track */}
                      <circle cx={cx} cy={cy} r={r} fill="none" stroke={token.colorFillSecondary} strokeWidth={10} />
                      {/* Progress */}
                      <circle
                        cx={cx} cy={cy} r={r}
                        fill="none"
                        stroke={barColor}
                        strokeWidth={10}
                        strokeDasharray={`${used} ${circ - used}`}
                        strokeDashoffset={circ / 4} /* start at top */
                        strokeLinecap="round"
                        style={{ transition: 'stroke-dasharray 0.6s ease' }}
                      />
                      {/* Center label */}
                      <text x={cx} y={cy - 6} textAnchor="middle" fontSize={20} fontWeight={700} fill={barColor}>{Math.round(pct)}%</text>
                      <text x={cx} y={cy + 14} textAnchor="middle" fontSize={10} fill={token.colorTextTertiary}>used</text>
                    </svg>
                  );
                })()}
              </div>

              {/* Token breakdown rows */}
              <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                {[
                  { label: 'Used', value: usage.useTokens, color: barColor },
                  { label: 'Remaining', value: remaining, color: token.colorSuccess },
                  { label: 'Max window', value: usage.maxTokens, color: token.colorTextSecondary },
                ].map(({ label, value, color }) => (
                  <div key={label} style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                    <Text style={{ fontSize: 13, color: token.colorTextSecondary }}>{label}</Text>
                    <Text style={{ fontSize: 13, fontWeight: 700, color, fontVariantNumeric: 'tabular-nums' }}>
                      {value.toLocaleString()}
                    </Text>
                  </div>
                ))}
              </div>

              {/* Progress bar detail */}
              <div>
                <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6 }}>
                  <Text style={{ fontSize: 11, color: token.colorTextTertiary }}>Context fill</Text>
                  <Text style={{ fontSize: 11, color: barColor, fontWeight: 600 }}>{Math.round(pct)}%</Text>
                </div>
                <Progress
                  percent={pct}
                  showInfo={false}
                  size={['100%', 6]}
                  strokeColor={barColor}
                  trailColor={token.colorFillSecondary}
                  style={{ margin: 0 }}
                />
                {pct >= 70 && (
                  <Text style={{ fontSize: 11, color: pct >= 90 ? token.colorError : token.colorWarning, marginTop: 6, display: 'block' }}>
                    {pct >= 90 ? '⚠ Context almost full — consider compacting.' : 'Context is getting large.'}
                  </Text>
                )}
              </div>

              {/* Compact / Update Context button */}
              <div style={{ borderTop: `1px solid ${token.colorBorderSecondary}`, paddingTop: 16 }}>
                <Text style={{ fontSize: 12, color: token.colorTextSecondary, display: 'block', marginBottom: 10 }}>
                  Compact context to free up space and keep the agent focused.
                </Text>
                <button
                  disabled={compacting || isProcessing}
                  onClick={async () => {
                    setCompacting(true);
                    try {
                      const res = await fetch(`/api/groups/${encodeURIComponent(group.jid)}/compact`, { method: 'POST' });
                      if (res.ok) {
                        message.success('Context compacted');
                      } else {
                        // Fallback: send /compact command via chat
                        message.info('Compact triggered');
                      }
                    } catch {
                      message.error('Compact failed');
                    } finally {
                      setCompacting(false);
                    }
                  }}
                  style={{
                    width: '100%',
                    padding: '10px 16px',
                    borderRadius: 12,
                    border: `1px solid ${token.colorPrimary}55`,
                    background: compacting || isProcessing ? token.colorFillTertiary : `${token.colorPrimary}15`,
                    color: compacting || isProcessing ? token.colorTextTertiary : token.colorPrimary,
                    cursor: compacting || isProcessing ? 'not-allowed' : 'pointer',
                    fontSize: 13,
                    fontWeight: 600,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    gap: 8,
                    transition: 'all 0.2s',
                  }}
                >
                  {compacting ? (
                    <LoadingOutlined style={{ fontSize: 15 }} />
                  ) : (
                    <ThunderboltOutlined style={{ fontSize: 15 }} />
                  )}
                  {compacting ? 'Compacting…' : 'Compact / Update Context now'}
                </button>
              </div>

            </div>
          );
        })() : (
          <div style={{ padding: 40, textAlign: 'center' }}>
            <Text style={{ color: token.colorTextTertiary }}>No usage data yet. Start a conversation first.</Text>
          </div>
        )}
      </Drawer>
    </div>
  );
}

/**
 * Walk the message list and emit React nodes, but coalesce runs of
 * `role === 'tool'` messages into a single `<ToolGroupCard>` so the chat
 * reads as "[user message] · [Read 3 files, ran 1 command ›] · [agent reply]"
 * rather than 4 separate tool rows.
 */
function renderMessagesWithToolGroups(
  messages: ChatMessage[],
  onResolvePermission: (requestId: string, optionKey: string) => void,
  onResolveQuestion: (
    requestId: string,
    answers: Record<number, number | number[]>,
    otherTexts?: Record<number, string>,
  ) => void,
): JSX.Element[] {
  const nodes: JSX.Element[] = [];
  let pendingTools: ToolMessage[] = [];

  const flushTools = () => {
    if (pendingTools.length === 0) return;
    // Indent matches MessageBubble's avatar column (28px circle + 10px gap)
    // so a tool row sits flush under the AI bubble it belongs to.
    // Identical to the `ml-[38px]` used by the reasoning-only fast path in
    // MessageBubble — keep both in sync if you change either.
    nodes.push(
      <div key={`tools-${pendingTools[0].id}`} className="ml-[38px]">
        <ToolGroupCard messages={pendingTools} />
      </div>,
    );
    pendingTools = [];
  };

  for (const msg of messages) {
    if (msg.role === 'tool') {
      pendingTools.push(msg as ToolMessage);
    } else {
      flushTools();
      nodes.push(
        <MessageBubble
          key={msg.id}
          message={msg}
          onResolvePermission={onResolvePermission}
          onResolveQuestion={onResolveQuestion}
        />,
      );
    }
  }
  flushTools();
  return nodes;
}
