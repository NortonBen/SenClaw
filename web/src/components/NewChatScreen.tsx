import { useState, useEffect } from 'react';
import { theme, Input, Spin, message, Select, Segmented } from 'antd';
import {
  FolderOpenOutlined, FileOutlined, FolderFilled,
  UserOutlined, ThunderboltOutlined, BulbOutlined, ApartmentOutlined,
} from '@ant-design/icons';
import type { AgentInfo } from '../types';

export type ChatType = 'Agent' | 'Plan' | 'DAG';

export type ChatKind = 'chat' | 'code';

export interface StartChatOptions {
  message: string;
  workDir?: string;
  profileId?: number;
  modelId?: string;
  chatType: ChatType;
  /** New: 'code' chats are tied to a workspace folder; 'chat' has no folder. */
  kind: ChatKind;
}

interface Props {
  onStart: (opts: StartChatOptions) => void;
  /** Display name shown in the welcome heading when no folder is picked. */
  projectName?: string;
  profiles: AgentInfo[];
}

const CHAT_SUGGESTIONS = [
  'Brainstorm ideas with me',
  'Explain a concept simply',
  'Help me draft a message',
  'Plan my day',
  'Summarize this article (paste it)',
  'Write a short story',
];

const CODE_SUGGESTIONS = [
  'Explain the codebase structure',
  'Fix the failing tests',
  'Add a new feature',
  'Review the last commit',
  'Refactor this module',
  'Write documentation',
];

interface WorkspaceEntry { name: string; path: string; is_dir: boolean; size?: number; }
interface LlmConfig { id: string; label: string; provider: string; modelName: string; }

declare global {
  interface Window {
    showDirectoryPicker?: () => Promise<FileSystemDirectoryHandle & { name: string }>;
  }
}

const RECENT_PATHS_KEY = 'senclaw:recent-workdirs';
const MAX_RECENT = 8;

function loadRecentPaths(): string[] {
  try { return JSON.parse(localStorage.getItem(RECENT_PATHS_KEY) ?? '[]'); } catch { return []; }
}
function pushRecentPath(p: string) {
  if (!p) return;
  try {
    const cur = loadRecentPaths().filter(x => x !== p);
    cur.unshift(p);
    localStorage.setItem(RECENT_PATHS_KEY, JSON.stringify(cur.slice(0, MAX_RECENT)));
  } catch {}
}

export function NewChatScreen({ onStart, projectName, profiles }: Props) {
  const { token } = theme.useToken();
  const [input, setInput] = useState('');
  const [workDir, setWorkDir] = useState('');
  const [showDirInput, setShowDirInput] = useState(false);
  const [entries, setEntries] = useState<WorkspaceEntry[]>([]);
  const [loadingFiles, setLoadingFiles] = useState(false);
  const [profileId, setProfileId] = useState<number | undefined>(undefined);
  const [modelId, setModelId] = useState<string | undefined>(undefined);
  const [chatType, setChatType] = useState<ChatType>('Agent');
  const [models, setModels] = useState<LlmConfig[]>([]);
  const [activeModelId, setActiveModelId] = useState<string | null>(null);
  const [recentPaths, setRecentPaths] = useState<string[]>(loadRecentPaths);
  const [kind, setKind] = useState<ChatKind>('chat');

  useEffect(() => {
    fetch('/api/llm-config')
      .then(r => r.json())
      .then(data => { setModels(data.configs ?? []); setActiveModelId(data.activeId ?? null); })
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!workDir) { setEntries([]); return; }
    const handle = setTimeout(() => {
      setLoadingFiles(true);
      fetch(`/api/workspace/files?path=${encodeURIComponent(workDir)}&depth=1`)
        .then(async r => { if (!r.ok) throw new Error((await r.text()) || 'Failed to list files'); return r.json(); })
        .then((data: { root: string; entries: WorkspaceEntry[] }) => setEntries(data.entries))
        .catch(err => { setEntries([]); if (workDir.length > 2) message.error(String(err?.message ?? err), 2); })
        .finally(() => setLoadingFiles(false));
    }, 300);
    return () => clearTimeout(handle);
  }, [workDir]);

  // Toggle the inline path input. Native picker (showDirectoryPicker) is
  // best-effort — even when supported it returns only the folder NAME, not
  // an absolute path, so we always need the user to confirm/type the path.
  const togglePicker = () => {
    setShowDirInput(v => !v);
  };

  const tryNativePicker = async () => {
    if (!window.showDirectoryPicker) {
      message.info('Browser-native picker unavailable. Paste an absolute path below.', 3);
      return;
    }
    try {
      const handle = await window.showDirectoryPicker();
      // The API only exposes the folder name. Prefill it as a hint, but the
      // user must still type the absolute path because the backend needs one.
      setWorkDir(handle.name);
      message.info(`Picked "${handle.name}" — edit below to add the absolute path.`, 4);
    } catch { /* user cancelled */ }
  };

  const handleSubmit = () => {
    const text = input.trim();
    if (!text) return;
    if (workDir) {
      pushRecentPath(workDir);
      setRecentPaths(loadRecentPaths());
    }
    onStart({
      message: text,
      // Workspace is only relevant for code chats — plain chats ignore it.
      workDir: kind === 'code' ? (workDir || undefined) : undefined,
      profileId,
      modelId,
      chatType,
      kind,
    });
  };

  const handleKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  // "Local Gemma 4 E2B-it 4-bit — text-only (vision/audio towers skipped) (MLX) (default)"
  //   → "Gemma 4 E2B-it 4-bit"   (chip-friendly)
  const shortModel = (s?: string) => {
    if (!s) return '';
    return s
      .replace(/\([^)]*\)/g, '')        // strip "(MLX)", "(default)", etc.
      .replace(/^\s*Local\s+/i, '')     // strip leading "Local "
      .split(/[—–-]/)[0]                // keep text before em/en/hyphen dash
      .trim();
  };

  const activeDefaultLabel = models.find(m => m.id === activeModelId)?.label;

  // Full labels in dropdown options (so the user can identify exact variants),
  // short labels in the Select trigger (via labelRender).
  const modelOptions = [
    {
      value: '',
      label: activeDefaultLabel ? `Default · ${activeDefaultLabel}` : 'Default model',
      shortLabel: activeDefaultLabel ? `Default · ${shortModel(activeDefaultLabel)}` : 'Default',
    },
    ...models.map(m => ({
      value: m.id,
      label: `${m.label} · ${m.modelName}`,
      shortLabel: shortModel(m.label) || m.label,
    })),
  ];

  const profileOptions = [
    { value: 0, label: 'No profile' },
    ...profiles.map(p => ({ value: p.id, label: p.name })),
  ];

  // Heading text adapts to mode + folder + explicit profile pick.
  // Code + folder picked  → "What should we build in <basename>?"
  // Code + no folder      → "Pick a workspace folder to start"
  // Chat + explicit profile pick → "Chat with <profileName>"
  // Chat + no profile     → "How can I help today?"  (don't show fallback agent name)
  const workDirBasename = workDir ? (workDir.split('/').filter(Boolean).pop() || workDir) : '';
  const selectedProfile = profileId ? profiles.find(p => p.id === profileId) : undefined;
  const heading =
    kind === 'code'
      ? (workDirBasename ? `What should we build in ${workDirBasename}?` : 'Pick a workspace folder to start')
      : (selectedProfile ? `Chat with ${selectedProfile.name}` : 'How can I help today?');
  const subheading =
    kind === 'code'
      ? (workDirBasename ? '' : 'Click "Folder" below to choose your project root.')
      : 'No workspace needed — just a conversation.';
  // `projectName` prop kept for backward compat but no longer drives the heading.
  void projectName;

  const SUGGESTIONS = kind === 'code' ? CODE_SUGGESTIONS : CHAT_SUGGESTIONS;

  return (
    <div className="flex flex-col h-full items-center justify-center px-8 overflow-y-auto py-8" style={{ background: 'transparent' }}>
      <div className="w-full max-w-2xl">
        {/* Kind selector — Chat vs Code */}
        <div className="flex justify-center mb-5">
          <Segmented<ChatKind>
            size="middle"
            value={kind}
            onChange={(v) => {
              setKind(v);
              // Reset workspace state when switching to plain Chat so it's
              // not silently carried over to the next code session.
              if (v === 'chat') { setWorkDir(''); setShowDirInput(false); }
            }}
            options={[
              { value: 'chat', label: <span style={{ padding: '0 8px' }}>💬 Chat</span> },
              { value: 'code', label: <span style={{ padding: '0 8px' }}>⌨️ Code</span> },
            ]}
          />
        </div>

        <div className="text-center mb-6">
          <div className="relative inline-block mb-3">
            <div className="absolute inset-0 bg-[#5BBFE8] blur-[60px] opacity-15 rounded-full" />
            <img src="/logo.png" alt="" className="w-14 h-14 mx-auto relative z-10 opacity-70" />
          </div>
          <h1 className="text-xl font-semibold mb-1" style={{ color: token.colorText }}>
            {heading}
          </h1>
          {subheading && (
            <p className="text-sm" style={{ color: token.colorTextSecondary }}>
              {subheading}
            </p>
          )}
        </div>

        {/* Unified input card — textarea + toolbar all in one rounded surface */}
        <div
          className="rounded-2xl overflow-hidden transition-shadow"
          style={{
            background: token.colorBgContainer,
            border: `1px solid ${token.colorBorderSecondary}`,
            boxShadow: '0 8px 32px -8px rgba(0,0,0,0.08)',
          }}
        >
          <textarea
            autoFocus
            value={input}
            onChange={e => setInput(e.target.value)}
            onKeyDown={handleKey}
            placeholder="Ask anything, or describe a task…"
            rows={3}
            className="w-full resize-none outline-none text-sm px-5 pt-4 pb-2 bg-transparent"
            style={{ color: token.colorText, lineHeight: 1.55 }}
          />

          {showDirInput && (
            <div className="px-4 pb-2 space-y-2">
              <div className="flex items-center gap-2">
                <Input
                  size="small"
                  placeholder="Absolute path (e.g. /Users/you/code/my-project) or ~/code/my-project"
                  value={workDir}
                  onChange={e => setWorkDir(e.target.value)}
                  onPressEnter={() => setShowDirInput(false)}
                  style={{ borderRadius: 8, fontFamily: 'ui-monospace, SFMono-Regular, monospace', fontSize: 12 }}
                  autoFocus
                  allowClear
                />
                {!!window.showDirectoryPicker && (
                  <button
                    type="button"
                    onClick={tryNativePicker}
                    className="px-2 py-1 rounded-md text-xs flex-shrink-0"
                    style={{
                      border: `1px solid ${token.colorBorderSecondary}`,
                      background: token.colorBgContainer,
                      color: token.colorTextSecondary,
                      cursor: 'pointer',
                    }}
                    title="Open native folder picker (folder name only — you'll still need to confirm the absolute path)"
                  >
                    Browse…
                  </button>
                )}
              </div>
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-[10px] uppercase tracking-widest" style={{ color: token.colorTextTertiary }}>Recent:</span>
                {recentPaths.length === 0 ? (
                  <span className="text-[11px]" style={{ color: token.colorTextQuaternary }}>
                    no recent folders yet
                  </span>
                ) : (
                  recentPaths.map(p => (
                    <button
                      key={p}
                      type="button"
                      onClick={() => { setWorkDir(p); }}
                      className="text-[11px] px-2 py-0.5 rounded-full"
                      style={{
                        border: `1px solid ${token.colorBorderSecondary}`,
                        background: workDir === p ? `${token.colorPrimary}15` : 'transparent',
                        color: workDir === p ? token.colorPrimary : token.colorTextSecondary,
                        cursor: 'pointer',
                        fontFamily: 'ui-monospace, SFMono-Regular, monospace',
                      }}
                      title={p}
                    >
                      {p.split('/').slice(-2).join('/') || p}
                    </button>
                  ))
                )}
              </div>

              {/* Live file preview directly under the path input */}
              <div className="rounded-md overflow-hidden" style={{ border: `1px solid ${token.colorBorderSecondary}` }}>
                <div
                  className="px-2 py-1 text-[10px] uppercase tracking-widest flex items-center justify-between"
                  style={{ color: token.colorTextTertiary, background: token.colorFillAlter }}
                >
                  <span>
                    {workDir
                      ? `Preview · ${entries.length} item${entries.length === 1 ? '' : 's'}`
                      : 'Preview'}
                  </span>
                  {loadingFiles && <Spin size="small" />}
                </div>
                <div className="max-h-40 overflow-y-auto">
                  {!workDir ? (
                    <div className="px-2 py-2 text-[11px]" style={{ color: token.colorTextTertiary }}>
                      Type or paste an absolute path above to preview files.
                    </div>
                  ) : entries.length === 0 && !loadingFiles ? (
                    <div className="px-2 py-2 text-[11px]" style={{ color: token.colorTextTertiary }}>
                      No files (or path invalid).
                    </div>
                  ) : (
                    entries.slice(0, 100).map(e => (
                      <div
                        key={e.path}
                        className="px-2 py-0.5 text-[11px] flex items-center gap-2"
                        style={{ color: token.colorTextSecondary }}
                      >
                        {e.is_dir
                          ? <FolderFilled style={{ color: token.colorPrimary, fontSize: 11 }} />
                          : <FileOutlined style={{ fontSize: 11 }} />}
                        <span className="truncate">{e.name}</span>
                      </div>
                    ))
                  )}
                </div>
              </div>
            </div>
          )}

          {/* Toolbar row */}
          <div
            className="flex items-center gap-1.5 px-2 py-1.5"
            style={{ borderTop: `1px solid ${token.colorBorderSecondary}` }}
          >
            {/* Folder pill — only meaningful for Code chats */}
            {kind === 'code' && (
              <button
                type="button"
                onClick={togglePicker}
                className="flex items-center gap-1.5 px-2 py-1 rounded-md text-xs transition-colors"
                style={{
                  color: workDir ? token.colorPrimary : token.colorTextSecondary,
                  background: workDir ? `${token.colorPrimary}10` : 'transparent',
                  border: 'none', cursor: 'pointer',
                }}
                onMouseEnter={e => { if (!workDir) (e.currentTarget as HTMLButtonElement).style.background = token.colorFillAlter; }}
                onMouseLeave={e => { if (!workDir) (e.currentTarget as HTMLButtonElement).style.background = 'transparent'; }}
                title={workDir || 'Pick a workspace folder'}
              >
                <FolderOpenOutlined style={{ fontSize: 13 }} />
                <span className="truncate" style={{ maxWidth: 160 }}>
                  {workDir ? workDir.split('/').pop() || workDir : 'Folder'}
                </span>
              </button>
            )}

            {kind === 'code' && (
              <span style={{ width: 1, height: 16, background: token.colorBorderSecondary }} />
            )}

            {/* Profile */}
            <Select
              size="small"
              variant="borderless"
              value={profileId ?? 0}
              onChange={(v) => setProfileId(v === 0 ? undefined : v)}
              options={profileOptions}
              style={{ flex: 1, minWidth: 0 }}
              placeholder="Profile"
              suffixIcon={<UserOutlined style={{ fontSize: 11 }} />}
            />

            {/* Model */}
            <Select
              size="small"
              variant="borderless"
              value={modelId ?? ''}
              onChange={(v) => setModelId(v || undefined)}
              options={modelOptions}
              style={{ flex: '0 0 auto', width: 160 }}
              placeholder="Model"
              labelRender={({ value }) => {
                const opt = modelOptions.find(o => o.value === value);
                const text = opt?.shortLabel ?? opt?.label ?? '';
                return (
                  <span title={opt?.label} style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', display: 'inline-block', maxWidth: '100%' }}>
                    {text}
                  </span>
                );
              }}
            />

            {/* Chat type */}
            <Segmented<ChatType>
              size="small"
              value={chatType}
              onChange={setChatType}
              options={[
                { value: 'Agent', label: <span><ThunderboltOutlined /></span>, title: 'Agent — full tool access' },
                { value: 'Plan',  label: <span><BulbOutlined /></span>,       title: 'Plan — research then propose plan' },
                { value: 'DAG',   label: <span><ApartmentOutlined /></span>,  title: 'DAG — multi-agent dispatch' },
              ]}
            />

            {/* Send */}
            <button
              type="button"
              onClick={handleSubmit}
              disabled={!input.trim()}
              className="w-8 h-8 rounded-full flex items-center justify-center transition-all flex-shrink-0"
              style={{
                background: input.trim() ? token.colorPrimary : token.colorFillSecondary,
                color: input.trim() ? '#fff' : token.colorTextTertiary,
                cursor: input.trim() ? 'pointer' : 'not-allowed',
                border: 'none',
                marginLeft: 4,
              }}
              aria-label="Start chat"
              title="Start (Enter)"
            >
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="currentColor" style={{ width: 14, height: 14 }}>
                <path fillRule="evenodd" d="M12 20a.75.75 0 0 1-.75-.75V5.56l-3.97 3.97a.75.75 0 1 1-1.06-1.06l5.25-5.25a.75.75 0 0 1 1.06 0l5.25 5.25a.75.75 0 1 1-1.06 1.06L12.75 5.56V19.25A.75.75 0 0 1 12 20z" clipRule="evenodd" />
              </svg>
            </button>
          </div>
        </div>

        {/* Suggestions */}
        <div className="flex flex-wrap gap-2 mt-5 justify-center">
          {SUGGESTIONS.map(s => (
            <button
              key={s}
              onClick={() => setInput(s)}
              className="text-xs px-3 py-1.5 rounded-full border transition-colors"
              style={{
                borderColor: token.colorBorderSecondary,
                color: token.colorTextSecondary,
                background: 'transparent',
                cursor: 'pointer',
              }}
              onMouseEnter={e => { (e.currentTarget as HTMLButtonElement).style.borderColor = token.colorPrimary; (e.currentTarget as HTMLButtonElement).style.color = token.colorPrimary; }}
              onMouseLeave={e => { (e.currentTarget as HTMLButtonElement).style.borderColor = token.colorBorderSecondary; (e.currentTarget as HTMLButtonElement).style.color = token.colorTextSecondary; }}
            >
              {s}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
