import { useEffect, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import { api, type ChatMsg, type Conversation, type ModelInfo, type Pin, type PlanStep, type RunMode } from '../api';
import { basename, timeAgo } from '../lib';

const MODES: { id: RunMode; label: string; hint: string }[] = [
  { id: 'chat', label: 'Chat', hint: 'Hỏi đáp thường' },
  { id: 'plan', label: 'Plan', hint: 'Lập kế hoạch từng bước' },
  { id: 'agent', label: 'Agent', hint: 'Tự chủ: nêu tool call + edit áp dụng được' },
  { id: 'dag', label: 'DAG', hint: 'Phân rã thành đồ thị phụ thuộc' },
];

interface Cmd { cmd: string; desc: string; mode?: RunMode; template?: string }

// Devin-style slash commands (type "/") and playbooks (type "!").
const COMMANDS: Cmd[] = [
  { cmd: '/plan', desc: 'Lập kế hoạch từng bước (Plan mode)', mode: 'plan' },
  { cmd: '/dag', desc: 'Phân rã thành đồ thị phụ thuộc (DAG mode)', mode: 'dag' },
  { cmd: '/review', desc: 'Review code: bug, bảo mật, cải thiện', template: 'Review đoạn code đã ghim: tìm bug, vấn đề bảo mật, và đề xuất cải thiện. Trích dẫn path:line.' },
  { cmd: '/test', desc: 'Viết unit test', template: 'Viết unit test đầy đủ cho đoạn code đã ghim.' },
  { cmd: '/explain', desc: 'Giải thích từng bước', template: 'Giải thích đoạn code đã ghim làm gì, luồng chạy từng bước, cite path:line.' },
  { cmd: '/fix', desc: 'Tìm & sửa lỗi (applyable)', template: 'Tìm và sửa lỗi trong đoạn code đã ghim; trả về bản sửa đầy đủ với header `// file: path`.' },
];
const PLAYBOOKS: Cmd[] = [
  { cmd: '!triage', desc: 'Điều tra & khoanh vùng lỗi', template: 'Điều tra vấn đề: nêu giả thuyết nguyên nhân gốc, các file liên quan (path:line), và cách sửa cụ thể.' },
  { cmd: '!refactor', desc: 'Refactor an toàn (giữ hành vi)', template: 'Refactor đoạn code đã ghim cho sạch/dễ đọc hơn, GIỮ NGUYÊN hành vi. Nêu rủi ro và test cần cập nhật.' },
];

interface Props {
  messages: ChatMsg[];
  pins: Pin[];
  sending: boolean;
  model: string | null;
  models: ModelInfo[];
  activeModelId: string | null;
  onSelectModel: (id: string) => void;
  mode: RunMode;
  onSelectMode: (m: RunMode) => void;
  onSend: (text: string) => void;
  onRemovePin: (index: number) => void;
  onClearPins: () => void;
  onClear: () => void;
  onApply: (code: string, targetFile: string | null) => void;
  onAddFile: () => void;
  onMentionFile: (path: string) => void;
  onToggleTerminal: () => void;
  onExecutePlan: (msgIndex: number) => void;
  rootName: string | null;
  conversations: Conversation[];
  onNewChat: () => void;
  onLoadConversation: (id: string) => void;
  onDeleteConversation: (id: string) => void;
  onClearAllConversations: () => void;
  onCollapse: () => void;
}

/** `// file: path` header at the top of a code block. */
function targetFromCode(code: string): string | null {
  const first = code.split('\n', 1)[0]?.trim() ?? '';
  const m = first.match(/^(?:\/\/|#|<!--)\s*file:\s*(.+?)\s*(?:-->)?$/i);
  return m ? m[1] : null;
}

/** All fenced code blocks in a markdown string that target a file. */
function fileBlocks(md: string): { target: string; code: string }[] {
  const out: { target: string; code: string }[] = [];
  const re = /```[^\n]*\n([\s\S]*?)```/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(md))) {
    const code = m[1].replace(/\n$/, '');
    const target = targetFromCode(code);
    if (target) out.push({ target, code });
  }
  return out;
}

export function ChatPanel({
  messages, pins, sending, model, models, activeModelId, onSelectModel, mode, onSelectMode,
  onSend, onRemovePin, onClearPins, onClear, onApply, onAddFile, onMentionFile, onToggleTerminal, onExecutePlan,
  rootName, conversations, onNewChat, onLoadConversation, onDeleteConversation, onClearAllConversations, onCollapse,
}: Props) {
  const [text, setText] = useState('');
  const [pop, setPop] = useState(false);
  const [showPast, setShowPast] = useState(false);
  const [moreOpen, setMoreOpen] = useState(false);
  const [fileList, setFileList] = useState<string[]>([]);
  useEffect(() => { api.files().then(setFileList).catch(() => setFileList([])); }, []);
  const logRef = useRef<HTMLDivElement>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [messages, sending]);

  function autosize() {
    const ta = taRef.current;
    if (!ta) return;
    ta.style.height = 'auto';
    ta.style.height = Math.min(ta.scrollHeight, 180) + 'px';
  }

  function submit() {
    const t = text.trim();
    if (!t || sending) return;
    onSend(t);
    setText('');
    requestAnimationFrame(autosize);
  }

  // Slash/playbook autocomplete: active while the input is a single "/…" or "!…" token.
  const token = !text.includes(' ') && (text.startsWith('/') || text.startsWith('!')) ? text : null;
  const menu: Cmd[] = token
    ? (token[0] === '/' ? COMMANDS : PLAYBOOKS).filter((c) => c.cmd.startsWith(token))
    : [];

  function applyCmd(item: Cmd) {
    if (item.mode) onSelectMode(item.mode);
    setText(item.template ?? '');
    requestAnimationFrame(() => { taRef.current?.focus(); autosize(); });
  }

  // @-mention file picker (active while typing "@…" at the end of the input).
  const atMatch = text.match(/@([^\s@]*)$/);
  const atFiles = atMatch
    ? fileList.filter((f) => f.toLowerCase().includes(atMatch[1].toLowerCase())).slice(0, 12)
    : [];
  function pickFile(path: string) {
    setText((t) => t.replace(/@[^\s@]*$/, `@${path} `));
    onMentionFile(path);
    requestAnimationFrame(() => { taRef.current?.focus(); autosize(); });
  }

  const modelLabel = model || (activeModelId ? models.find((m) => m.id === activeModelId)?.modelName : null) || 'Model';
  const modeLabel = MODES.find((m) => m.id === mode)?.label ?? 'Chat';

  return (
    <div className="chat ag">
      <div className="ag-head">
        <span className="ag-title">💬 AI Chat</span>
        <div className="ag-head-actions">
          <button data-tip="Hội thoại mới" onClick={() => { onNewChat(); setShowPast(false); }}>＋</button>
          <button data-tip="Hội thoại trước" className={showPast ? 'on' : ''} onClick={() => setShowPast((v) => !v)}>↺</button>
          <div className="more-wrap">
            <button data-tip="Tùy chọn" onClick={() => setMoreOpen((v) => !v)}>⋯</button>
            {moreOpen && (
              <div className="more-menu" onMouseLeave={() => setMoreOpen(false)}>
                <button onClick={() => { onClear(); setMoreOpen(false); }}>🧹 Xoá hội thoại hiện tại</button>
                <button onClick={() => { onAddFile(); setMoreOpen(false); }}>📄 Thêm file vào chat</button>
                <button onClick={() => { onClearAllConversations(); setMoreOpen(false); }}>🗑 Xoá tất cả lịch sử</button>
              </div>
            )}
          </div>
          <button data-tip="Thu nhỏ" onClick={onCollapse}>✕</button>
        </div>
      </div>

      <div className="chat-log" ref={logRef}>
        {showPast ? (
          <PastConversations conversations={conversations} onLoad={(id) => { onLoadConversation(id); setShowPast(false); }} onDelete={onDeleteConversation} onClose={() => setShowPast(false)} />
        ) : messages.length === 0 ? (
          <NewChatLanding rootName={rootName} conversations={conversations} onLoad={onLoadConversation} onSeeAll={() => setShowPast(true)} />
        ) : (
          <>
            {messages.map((m, i) => (
              <MessageRow key={i} index={i} msg={m} onApply={onApply} onExecutePlan={onExecutePlan} />
            ))}
            {sending && (
              <div className="msg assistant">
                <div className="bubble"><span className="spin">◐</span> đang suy nghĩ…</div>
              </div>
            )}
          </>
        )}
      </div>

      <div className="ag-footer">
        {pins.length > 0 && (
          <div className="pins">
            {pins.map((p, i) => (
              <span className="pin" key={i}>
                <span className="loc">{basename(p.path)}{p.end_line ? `:${p.start_line}-${p.end_line}` : ''}</span>
                <button onClick={() => onRemovePin(i)} title="Bỏ ghim">×</button>
              </span>
            ))}
            <button className="pin" onClick={onClearPins}>xoá hết</button>
          </div>
        )}

        <div className="ag-attach">
          <button data-tip="Thêm file hiện tại vào chat" onClick={onAddFile}>📄</button>
          <button data-tip="Bật/tắt terminal" onClick={onToggleTerminal}>▷_</button>
          {pins.length > 0 && <span className="ag-pincount">📌 {pins.length}</span>}
        </div>

        {menu.length > 0 && (
          <div className="cmd-menu">
            {menu.map((c) => (
              <button key={c.cmd} onClick={() => applyCmd(c)}>
                <span className="cmd-name">{c.cmd}</span>
                <span className="cmd-desc">{c.desc}</span>
              </button>
            ))}
          </div>
        )}
        {menu.length === 0 && atFiles.length > 0 && (
          <div className="cmd-menu at-menu">
            {atFiles.map((f) => (
              <button key={f} onClick={() => pickFile(f)}>
                <span className="cmd-name">📄 {f.split('/').pop()}</span>
                <span className="cmd-desc">{f}</span>
              </button>
            ))}
          </div>
        )}
        <textarea
          ref={taRef}
          rows={1}
          placeholder="Ask anything, @ to mention, / for actions, ! for playbooks"
          value={text}
          onChange={(e) => { setText(e.target.value); autosize(); }}
          onKeyDown={(e) => {
            if (menu.length > 0 && (e.key === 'Enter' || e.key === 'Tab')) { e.preventDefault(); applyCmd(menu[0]); return; }
            if (atFiles.length > 0 && (e.key === 'Enter' || e.key === 'Tab')) { e.preventDefault(); pickFile(atFiles[0]); return; }
            if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); submit(); }
          }}
        />

        <div className="ag-controls">
          <button className="ag-plus" data-tip="Đính kèm" onClick={onAddFile}>＋</button>
          <div className="ag-model-wrap">
            <button className="ag-model" onClick={() => setPop((v) => !v)}>
              {modelLabel} <span className="dim">({modeLabel})</span> <span className="caret">⌃</span>
            </button>
            {pop && (
              <div className="ag-popover" onMouseLeave={() => setPop(false)}>
                <div className="ag-pop-sec">Chế độ</div>
                {MODES.map((m) => (
                  <button key={m.id} className={mode === m.id ? 'sel' : ''} title={m.hint}
                    onClick={() => { onSelectMode(m.id); }}>
                    {mode === m.id ? '● ' : '○ '}{m.label}
                  </button>
                ))}
                {models.length > 0 && <div className="ag-pop-sec">Model</div>}
                {models.map((m) => (
                  <button key={m.id} className={activeModelId === m.id ? 'sel' : ''}
                    onClick={() => { onSelectModel(m.id); setPop(false); }}>
                    {activeModelId === m.id ? '● ' : '○ '}{m.modelName || m.id}
                    {m.provider ? <span className="dim"> · {m.provider}</span> : null}
                  </button>
                ))}
              </div>
            )}
          </div>
          <button className="ag-send" disabled={sending || !text.trim()} onClick={submit} title="Gửi">➤</button>
        </div>
      </div>
    </div>
  );
}

function NewChatLanding({ rootName, conversations, onLoad, onSeeAll }: {
  rootName: string | null; conversations: Conversation[]; onLoad: (id: string) => void; onSeeAll: () => void;
}) {
  return (
    <div className="nc">
      <div className="nc-hero">
        <div className="nc-logo">💬</div>
        <div className="nc-name">{rootName ?? 'Workspace'}</div>
        <div className="nc-sub">Hỏi bất cứ điều gì về code. Bôi chọn code rồi <b>Cmd/Ctrl+L</b> để ghim, hoặc chuột phải → <b>Hỏi AI</b>.</div>
      </div>
      {conversations.length > 0 && (
        <div className="nc-convs">
          <div className="nc-convs-head">CUỘC TRÒ CHUYỆN TRƯỚC</div>
          {conversations.slice(0, 4).map((c) => (
            <div key={c.id} className="nc-conv" onClick={() => onLoad(c.id)}>
              <span className="nc-conv-title">{c.title}</span>
              <span className="nc-conv-time">{timeAgo(Math.floor(c.at / 1000))}</span>
            </div>
          ))}
          {conversations.length > 4 && <button className="nc-seeall" onClick={onSeeAll}>Xem tất cả</button>}
        </div>
      )}
    </div>
  );
}

function PastConversations({ conversations, onLoad, onDelete, onClose }: {
  conversations: Conversation[]; onLoad: (id: string) => void; onDelete: (id: string) => void; onClose: () => void;
}) {
  return (
    <div className="past">
      <div className="past-head">
        <span>Hội thoại trước · {conversations.length}</span>
        <button onClick={onClose}>✕</button>
      </div>
      {conversations.length === 0 && <div className="chat-empty">Chưa có hội thoại nào đã lưu.<br />Bấm <b>＋</b> để bắt đầu và lưu hội thoại.</div>}
      {conversations.map((c) => (
        <div key={c.id} className="past-row" onClick={() => onLoad(c.id)}>
          <div className="past-info">
            <div className="past-title">{c.title}</div>
            <div className="past-meta">{c.messages.length} tin · {timeAgo(Math.floor(c.at / 1000))}</div>
          </div>
          <button className="past-del" title="Xoá" onClick={(e) => { e.stopPropagation(); onDelete(c.id); }}>🗑</button>
        </div>
      ))}
    </div>
  );
}

function MessageRow({ index, msg, onApply, onExecutePlan }: {
  index: number; msg: ChatMsg; onApply: Props['onApply']; onExecutePlan: Props['onExecutePlan'];
}) {
  const [copied, setCopied] = useState(false);
  if (msg.role === 'user') {
    return (
      <div className="msg user">
        <div className="bubble"><span style={{ whiteSpace: 'pre-wrap' }}>{msg.content}</span></div>
      </div>
    );
  }
  const edits = fileBlocks(msg.content);
  function copy() {
    navigator.clipboard?.writeText(msg.content).then(() => { setCopied(true); setTimeout(() => setCopied(false), 1200); });
  }
  return (
    <div className="msg assistant">
      {msg.ms ? <div className="msg-worked">✓ Worked for {msg.ms}s</div> : null}
      <div className="bubble"><Markdown text={msg.content} onApply={onApply} /></div>
      {msg.steps && (
        <PlanTimeline steps={msg.steps} executing={!!msg.executing} onExecute={() => onExecutePlan(index)} onApply={onApply} />
      )}
      {edits.length > 0 && (
        <div className="edits-card">
          <span>✎ {edits.length} file thay đổi</span>
          <button onClick={() => edits.forEach((e) => onApply(e.code, e.target))}>Review</button>
        </div>
      )}
      <div className="msg-actions">
        <button onClick={copy} data-tip={copied ? 'Đã chép' : 'Sao chép'}>{copied ? '✓' : '⧉'}</button>
        <button data-tip="Hữu ích">👍</button>
        <button data-tip="Chưa tốt">👎</button>
      </div>
    </div>
  );
}

function PlanTimeline({ steps, executing, onExecute, onApply }: {
  steps: PlanStep[]; executing: boolean; onExecute: () => void; onApply: Props['onApply'];
}) {
  const icon = (s: PlanStep['status']) =>
    s === 'done' ? '✓' : s === 'running' ? <span className="spin">◐</span> : s === 'error' ? '✕' : '○';
  const ranAny = steps.some((s) => s.status !== 'pending');
  const done = steps.filter((s) => s.status === 'done').length;
  return (
    <div className="plan">
      <div className="plan-head">
        <span>🗺 Kế hoạch · {steps.length} bước{ranAny ? ` · ${done}/${steps.length} xong` : ''}</span>
        {!executing && (
          <button className="btn" onClick={onExecute}>{ranAny ? '↻ Chạy lại' : '▶ Duyệt & Chạy'}</button>
        )}
        {executing && <span className="plan-running"><span className="spin">◐</span> đang chạy…</span>}
      </div>
      <div className="plan-steps">
        {steps.map((s, i) => (
          <div key={i} className={`plan-step ${s.status}`}>
            <div className="plan-step-head">
              <span className="plan-step-ico">{icon(s.status)}</span>
              <span className="plan-step-title">{i + 1}. {s.title}</span>
            </div>
            {s.result && (
              <div className="plan-step-result">
                {s.status === 'error' ? <span className="plan-err">⚠️ {s.result}</span> : <Markdown text={s.result} onApply={onApply} />}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function Markdown({ text, onApply }: { text: string; onApply: Props['onApply'] }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code(props) {
            const { className, children } = props as { className?: string; children?: React.ReactNode };
            const match = /language-(\w+)/.exec(className ?? '');
            const raw = String(children ?? '').replace(/\n$/, '');
            if (!match && !raw.includes('\n')) return <code className={className}>{children}</code>;
            const target = targetFromCode(raw);
            return (
              <div className="code-block">
                <button className="apply" onClick={() => onApply(raw, target)}
                  title={target ? `Xem/ghi vào ${target}` : 'Xem/ghi vào file đang mở'}>⤵ Apply</button>
                <SyntaxHighlighter language={match?.[1] ?? 'text'} style={oneDark}
                  customStyle={{ margin: 0, fontSize: 12, background: '#1b1b1b' }}>
                  {raw}
                </SyntaxHighlighter>
              </div>
            );
          },
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}
