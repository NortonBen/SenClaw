import { useState, useEffect, useMemo, useRef } from 'react';
import { theme, Input, message, Select, Segmented, Dropdown, Modal } from 'antd';
import { WorkflowQuickStart } from './WorkflowQuickStart';
import { useCommandSuggestions } from './chat-common';
import type { MenuProps } from 'antd';
import {
  FolderOpenOutlined,
  UserOutlined, ThunderboltOutlined, BulbOutlined, ApartmentOutlined,
  CheckOutlined, FolderAddOutlined, FolderOutlined, SearchOutlined, MinusCircleOutlined,
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
  /** Select a workflow "session" (wfrun:<id>) in the chat surface after a
   *  run started from the quick-start — keeps the user in Chat. */
  onWorkflowRunSelected?: (jid: string) => void;
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

interface LlmConfig { id: string; label: string; provider: string; modelName: string; }

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

// --- Native folder picker (Tauri desktop shell only) ----------------------
// The packaged macOS app exposes the dialog plugin via `withGlobalTauri`, so a
// real OS folder dialog is available at `window.__TAURI__.dialog.open`. In a
// plain browser `window.__TAURI__` is undefined and we fall back to the
// typed-path modal. We never use the browser File System Access API (it pops a
// permission prompt and isn't available in WKWebView).

interface TauriGlobal {
  dialog?: { open?: (opts: unknown) => Promise<string | string[] | null> };
  core?: { invoke?: (cmd: string, args?: unknown) => Promise<unknown> };
}
function tauriGlobal(): TauriGlobal | undefined {
  return (window as unknown as { __TAURI__?: TauriGlobal }).__TAURI__;
}

/** True when running inside the Tauri desktop shell (native dialog available). */
function hasNativeDialog(): boolean {
  return !!tauriGlobal();
}

/** Open the native OS folder dialog. Returns the chosen absolute path, or
 *  null if the user cancelled or the dialog isn't reachable. */
async function pickFolderNative(): Promise<string | null> {
  const t = tauriGlobal();
  if (!t) return null;
  const opts = { directory: true, multiple: false, title: 'Choose a workspace folder' };
  try {
    if (t.dialog?.open) {
      const res = await t.dialog.open(opts);
      return typeof res === 'string' ? res : null;
    }
    // Fallback to the raw plugin command if the high-level global is absent.
    if (t.core?.invoke) {
      const res = await t.core.invoke('plugin:dialog|open', { options: opts });
      return typeof res === 'string' ? res : null;
    }
  } catch {
    return null;
  }
  return null;
}

export function NewChatScreen({ onStart, projectName, profiles, onWorkflowRunSelected }: Props) {
  const { token } = theme.useToken();
  const [input, setInput] = useState('');
  const [workDir, setWorkDir] = useState('');
  const [profileId, setProfileId] = useState<number | undefined>(undefined);
  const [modelId, setModelId] = useState<string | undefined>(undefined);
  const [chatType, setChatType] = useState<ChatType>('Agent');
  const [models, setModels] = useState<LlmConfig[]>([]);
  const [activeModelId, setActiveModelId] = useState<string | null>(null);
  const [recentPaths, setRecentPaths] = useState<string[]>(loadRecentPaths);
  // 'workflow' is not a chat kind — it swaps the composer for the workflow
  // quick-start (pick & run, or create with the agent).
  const [kind, setKind] = useState<ChatKind | 'workflow'>('chat');
  // Project-picker dropdown + path modal (Codex-style flow).
  //   - `pickerSearch` filters the recent-projects list.
  //   - `pathModalMode` opens a small modal for either creating a new folder
  //     (mkdir) or pointing at an existing absolute path.
  const [pickerSearch, setPickerSearch] = useState('');
  const [pathModalMode, setPathModalMode] = useState<null | 'create' | 'open'>(null);
  const [newFolderPath, setNewFolderPath] = useState('');
  const [creatingFolder, setCreatingFolder] = useState(false);

  useEffect(() => {
    fetch('/api/llm-config')
      .then(r => r.json())
      .then(data => { setModels(data.configs ?? []); setActiveModelId(data.activeId ?? null); })
      .catch(() => {});
  }, []);

  // Note: the New Chat picker never reads the filesystem from the browser.
  // Selecting a folder only captures its *path string* (see `submitPathModal`);
  // resolving and validating that path — tilde expansion, existence, access —
  // is the backend's job when the chat opens the workspace.

  /** Commit a chosen workspace path: set it active, remember it, notify. */
  const commitWorkDir = (path: string) => {
    setWorkDir(path);
    pushRecentPath(path);
    setRecentPaths(loadRecentPaths());
    message.success(`Workspace set to ${path}`, 2);
  };

  /** Submit the modal — either `mkdir` (create) or just `setWorkDir` (open).
   *  Both flows validate the path and push it onto the recents list. */
  const submitPathModal = async () => {
    const path = newFolderPath.trim();
    if (!path) {
      message.warning('Enter the folder path.', 2);
      return;
    }
    if (pathModalMode === 'create') {
      setCreatingFolder(true);
      try {
        const res = await fetch('/api/workspace/mkdir', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ path, recursive: true }),
        });
        const data = await res.json().catch(() => ({}));
        if (!res.ok) throw new Error(data?.error ?? data?.message ?? `HTTP ${res.status}`);
        const canonical = data.path ?? path;
        setWorkDir(canonical);
        pushRecentPath(canonical);
        setRecentPaths(loadRecentPaths());
        message.success(
          data.created === false ? `Already exists — using ${canonical}` : `Created ${canonical}`,
          2,
        );
        setPathModalMode(null);
        setNewFolderPath('');
      } catch (e: unknown) {
        message.error(`Create failed: ${String((e as Error)?.message ?? e)}`, 3);
      } finally {
        setCreatingFolder(false);
      }
    } else if (pathModalMode === 'open') {
      // Just capture the path the user pointed at — the UI no longer touches
      // the filesystem here. Validation/resolution (incl. `~` expansion and
      // existence) is the backend's job when the chat actually opens the
      // workspace, so we only do a cheap client-side format check.
      if (!path.startsWith('/') && !path.startsWith('~')) {
        message.warning('Path must be absolute (start with / or ~).', 2);
        return;
      }
      commitWorkDir(path);
      setPathModalMode(null);
      setNewFolderPath('');
    }
  };

  /** Build the Dropdown menu items for the Codex-style project picker.
   *  Layout: search input → recent projects → divider → "Add new project"
   *  submenu → "Don't work in a project". Search filters the recent list
   *  in-place; clicking a project sets it as the active workDir. */
  const projectMenuItems: MenuProps['items'] = useMemo(() => {
    const items: MenuProps['items'] = [];
    const basename = (p: string): string => p.split('/').filter(Boolean).pop() || p;

    items.push({
      key: 'search',
      type: 'group',
      label: (
        <Input
          size="small"
          prefix={<SearchOutlined style={{ fontSize: 11, color: token.colorTextTertiary }} />}
          placeholder="Search projects"
          value={pickerSearch}
          onChange={e => setPickerSearch(e.target.value)}
          onClick={e => e.stopPropagation()}
          onKeyDown={e => e.stopPropagation()}
          style={{ borderRadius: 6 }}
          allowClear
        />
      ),
    });

    const filtered = recentPaths.filter(p =>
      !pickerSearch || p.toLowerCase().includes(pickerSearch.toLowerCase()),
    );
    if (filtered.length === 0) {
      items.push({
        key: 'empty',
        disabled: true,
        label: (
          <span style={{ fontSize: 11, color: token.colorTextTertiary }}>
            {pickerSearch ? 'No matches' : 'No recent projects yet'}
          </span>
        ),
      });
    } else {
      for (const p of filtered) {
        const isCurrent = workDir === p;
        items.push({
          key: `path:${p}`,
          icon: <FolderOpenOutlined style={{ color: token.colorPrimary }} />,
          onClick: () => { setWorkDir(p); setPickerSearch(''); },
          label: (
            <span style={{ display: 'inline-flex', alignItems: 'center', justifyContent: 'space-between', width: '100%' }}>
              <span title={p} style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: 240 }}>
                {basename(p)}
              </span>
              {isCurrent && <CheckOutlined style={{ marginLeft: 8, color: token.colorPrimary }} />}
            </span>
          ),
        });
      }
    }

    items.push({ type: 'divider' });

    items.push({
      key: 'add',
      icon: <FolderAddOutlined />,
      label: 'Add new project',
      children: [
        {
          key: 'add-scratch',
          icon: <FolderAddOutlined />,
          label: 'Start from scratch',
          onClick: () => {
            setNewFolderPath(
              workDir
                ? `${workDir.replace(/\/+$/, '')}/new-folder`
                : '~/projects/new-project',
            );
            setPathModalMode('create');
          },
        },
        {
          key: 'add-existing',
          icon: <FolderOutlined />,
          label: 'Use an existing folder',
          onClick: async () => {
            // In the desktop app, open the native OS folder dialog (real path,
            // no permission prompt). In a plain browser there's no native
            // dialog, so fall back to the typed-path modal — the backend
            // resolves/validates whatever path we hand it when the chat opens.
            if (hasNativeDialog()) {
              const picked = await pickFolderNative();
              if (picked) commitWorkDir(picked);
              return; // cancelled → leave things as-is, no modal.
            }
            setNewFolderPath(workDir || '');
            setPathModalMode('open');
          },
        },
      ],
    });

    items.push({
      key: 'none',
      icon: <MinusCircleOutlined />,
      label: "Don't work in a project",
      disabled: !workDir,
      onClick: () => { setWorkDir(''); message.info('Workspace cleared — chatting without a folder.', 2); },
    });

    return items;
  }, [recentPaths, pickerSearch, workDir, token]);

  // Same `/ @ #` composer affordances as an open conversation. Files are only
  // offered once a workspace is picked — a plain chat has nothing to list.
  const suggest = useCommandSuggestions({
    value: input,
    onChange: setInput,
    fileScope: kind === 'code' && workDir ? { path: workDir } : undefined,
  });

  const handleSubmit = () => {
    const text = input.trim();
    if (!text || kind === 'workflow') return;
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
    if (suggest.handleKeyDown(e)) return;
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
    kind === 'workflow'
      ? 'Chạy một workflow'
      : kind === 'code'
      ? (workDirBasename ? `What should we build in ${workDirBasename}?` : 'Pick a workspace folder to start')
      : (selectedProfile ? `Chat with ${selectedProfile.name}` : 'How can I help today?');
  const subheading =
    kind === 'workflow'
      ? 'Chọn quy trình có sẵn, hoặc mô tả để AI agent tạo mới rồi chạy ngay.'
      : kind === 'code'
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
          <Segmented<ChatKind | 'workflow'>
            size="middle"
            value={kind}
            onChange={(v) => {
              setKind(v);
              // Reset workspace state when leaving Code so it's not silently
              // carried over to the next code session.
              if (v === 'chat') { setWorkDir(''); }
            }}
            options={[
              { value: 'chat', label: <span style={{ padding: '0 8px' }}>💬 Chat</span> },
              { value: 'code', label: <span style={{ padding: '0 8px' }}>⌨️ Code</span> },
              { value: 'workflow', label: <span style={{ padding: '0 8px' }}>🔁 Workflow</span> },
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

        {kind === 'workflow' ? (
          <WorkflowQuickStart onRunSelected={onWorkflowRunSelected} />
        ) : (
        <>
        {/* Unified input card — textarea + toolbar all in one rounded surface.
            The suggestion popup sits outside the card: the card clips overflow,
            which would otherwise cut the dropdown off. */}
        <div style={{ position: 'relative' }}>
        {suggest.popup}
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
            placeholder="Ask anything, or describe a task… (/ skill, @ file)"
            rows={3}
            className="w-full resize-none outline-none text-sm px-5 pt-4 pb-2 bg-transparent"
            style={{ color: token.colorText, lineHeight: 1.55 }}
          />

          {/* The Codex-style project dropdown is rendered on the toolbar pill
              below — no inline panel here. The path picker / create-folder
              flow opens in a small modal triggered from the dropdown so it
              doesn't push the textarea around. */}

          {/* Toolbar row */}
          <div
            className="flex items-center gap-1.5 px-2 py-1.5"
            style={{ borderTop: `1px solid ${token.colorBorderSecondary}` }}
          >
            {/* Folder pill — only meaningful for Code chats. Codex-style
                dropdown: search + recent projects + "Add new project" submenu
                (Start from scratch / Use an existing folder) + "Don't work in
                a project". Selection sets workDir and closes the dropdown. */}
            {kind === 'code' && (
              <Dropdown
                trigger={['click']}
                menu={{ items: projectMenuItems }}
                placement="topLeft"
              >
                <button
                  type="button"
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
                    {workDir ? workDir.split('/').filter(Boolean).pop() || workDir : 'Folder'}
                  </span>
                </button>
              </Dropdown>
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
        </>
        )}
      </div>

      {/* Path modal — opened from the project dropdown's "Add new project"
          submenu. Single modal handles both flows:
            • mode='create' → POST /api/workspace/mkdir, then set as workDir
            • mode='open'   → GET /api/workspace/files to validate existence,
                              then set as workDir
          Keeps the New Chat screen quiet — no inline panel pushes the
          textarea around mid-flow. */}
      <Modal
        title={pathModalMode === 'create' ? 'Start a new project folder' : 'Use an existing folder'}
        open={pathModalMode !== null}
        onCancel={() => { setPathModalMode(null); setNewFolderPath(''); }}
        onOk={submitPathModal}
        okText={pathModalMode === 'create' ? (creatingFolder ? 'Creating…' : 'Create') : 'Open'}
        okButtonProps={{ disabled: creatingFolder || !newFolderPath.trim() }}
        cancelButtonProps={{ disabled: creatingFolder }}
        confirmLoading={creatingFolder}
        width={520}
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <Input
            size="middle"
            placeholder={
              pathModalMode === 'create'
                ? '/absolute/path/to/new-folder (parents are auto-created)'
                : '/absolute/path/to/existing-folder'
            }
            value={newFolderPath}
            onChange={e => setNewFolderPath(e.target.value)}
            onPressEnter={submitPathModal}
            autoFocus
            disabled={creatingFolder}
            style={{
              fontFamily: 'ui-monospace, SFMono-Regular, monospace',
              fontSize: 13,
            }}
          />
          <div style={{ fontSize: 11, color: token.colorTextTertiary, lineHeight: 1.6 }}>
            {pathModalMode === 'create' ? (
              <>
                Path must be absolute (e.g. <code>/Users/you/projects/my-app</code>) or
                <code> ~/path</code> (tilde-expanded). Missing parent directories will be
                created automatically.
              </>
            ) : (
              <>
                Path must be absolute or <code>~/</code>-prefixed. Folder must already exist —
                use <strong>Start from scratch</strong> if you need to create it first.
              </>
            )}
          </div>
        </div>
      </Modal>
    </div>
  );
}
