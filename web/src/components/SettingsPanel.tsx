import { useEffect, useState } from 'react';
import type { GroupInfo, RegisterGroupPayload, UpdateGroupPayload } from '../types';

// ===== Types =====

type Tab = 'permissions' | 'agents' | 'llm';

// ===== LLM Config types & constants =====

interface LLMConfig {
  id: string;
  label: string;
  provider: string;
  baseURL: string;
  apiKey: string;
  modelName: string;
  adapt: 'openai' | 'anthropic';
  maxTokens: number;
  contextLength: number;
}

interface ProviderDef {
  name: string;
  baseURL: string;
  /** Override URL for fetching model list (some providers use a different endpoint) */
  modelsUrl?: string;
  baseURLPlaceholder?: string;
  apiKeyPlaceholder?: string;
  defaultAdapt: 'openai' | 'anthropic';
  defaultMaxTokens?: number;
  defaultContextLength?: number;
}

const PROVIDERS: Record<string, ProviderDef> = {
  anthropic:  { name: 'Anthropic',          baseURL: 'https://api.anthropic.com',                         defaultAdapt: 'anthropic', apiKeyPlaceholder: 'Your Anthropic API key' },
  openai:     { name: 'OpenAI',             baseURL: 'https://api.openai.com/v1',                         defaultAdapt: 'openai',    apiKeyPlaceholder: 'Your OpenAI API key' },
  kimi:       { name: 'Kimi (Moonshot)',     baseURL: 'https://api.moonshot.cn/v1',                        defaultAdapt: 'openai',    apiKeyPlaceholder: 'Your Moonshot API key' },
  minimax:    { name: 'MiniMax',            baseURL: 'https://api.minimaxi.com/anthropic',                 defaultAdapt: 'anthropic', apiKeyPlaceholder: 'Your MiniMax API key' },
  deepseek:   { name: 'DeepSeek',           baseURL: 'https://api.deepseek.com/anthropic', modelsUrl: 'https://api.deepseek.com/v1',        defaultAdapt: 'anthropic', apiKeyPlaceholder: 'Your DeepSeek API key' },
  glm:        { name: 'GLM (Zhipu)',          baseURL: 'https://open.bigmodel.cn/api/paas/v4',              defaultAdapt: 'openai',    apiKeyPlaceholder: 'Your Zhipu API key' },
  openrouter: { name: 'OpenRouter',         baseURL: 'https://openrouter.ai/api',          modelsUrl: 'https://openrouter.ai/api/v1',       defaultAdapt: 'openai', apiKeyPlaceholder: 'Your OpenRouter API key' },
  qwen:       { name: 'Qwen (Alibaba)',      baseURL: 'https://dashscope.aliyuncs.com/compatible-mode/v1', defaultAdapt: 'openai',   apiKeyPlaceholder: 'Your Alibaba Cloud API key' },
  custom:     { name: 'Custom LLM endpoint',    baseURL: '',                                                   defaultAdapt: 'openai',    baseURLPlaceholder: 'https://your-api.com/v1', apiKeyPlaceholder: 'Your API key' },
};
const PROVIDER_ORDER = ['anthropic','openai','kimi','minimax','deepseek','glm','openrouter','qwen','custom'];
const DEFAULT_MAX_TOKENS_OPTIONS = [4096, 8192, 16000, 32000, 64000, 100000];
const DEFAULT_CONTEXT_LENGTH_OPTIONS = [8000, 16000, 32000, 64000, 128000, 200000, 256000, 512000, 1000000];

// Model limits matched by prefix (more specific prefixes first)
const MODEL_LIMITS_TABLE: Array<[string, { maxTokens: number; contextLength: number }]> = [
  // Anthropic Claude
  ['claude-opus-4',       { maxTokens: 32000,   contextLength: 200000  }],
  ['claude-sonnet-4',     { maxTokens: 64000,   contextLength: 200000  }],
  ['claude-haiku-4',      { maxTokens: 16000,   contextLength: 200000  }],
  ['claude-3-7-sonnet',   { maxTokens: 64000,   contextLength: 200000  }],
  ['claude-3-5-sonnet',   { maxTokens: 8192,    contextLength: 200000  }],
  ['claude-3-5-haiku',    { maxTokens: 8192,    contextLength: 200000  }],
  ['claude-3-opus',       { maxTokens: 4096,    contextLength: 200000  }],
  ['claude-3-sonnet',     { maxTokens: 4096,    contextLength: 200000  }],
  ['claude-3-haiku',      { maxTokens: 4096,    contextLength: 200000  }],
  // OpenAI
  ['o3-mini',             { maxTokens: 65536,   contextLength: 200000  }],
  ['o3',                  { maxTokens: 100000,  contextLength: 200000  }],
  ['o1-mini',             { maxTokens: 65536,   contextLength: 128000  }],
  ['o1',                  { maxTokens: 32768,   contextLength: 200000  }],
  ['gpt-4o-mini',         { maxTokens: 16384,   contextLength: 128000  }],
  ['gpt-4o',              { maxTokens: 16384,   contextLength: 128000  }],
  ['gpt-4-turbo',         { maxTokens: 4096,    contextLength: 128000  }],
  ['gpt-4',               { maxTokens: 8192,    contextLength: 8192    }],
  ['gpt-3.5-turbo',       { maxTokens: 4096,    contextLength: 16384   }],
  // DeepSeek
  ['deepseek-r1',         { maxTokens: 32000,   contextLength: 64000   }],
  ['deepseek-v3',         { maxTokens: 32000,   contextLength: 64000   }],
  ['deepseek-chat',       { maxTokens: 8192,    contextLength: 64000   }],
  ['deepseek-reasoner',   { maxTokens: 8192,    contextLength: 64000   }],
  ['deepseek-coder',      { maxTokens: 8192,    contextLength: 16000   }],
  // Kimi / Moonshot
  ['kimi-k2',             { maxTokens: 32000,   contextLength: 131072  }],
  ['moonshot-v1-128k',    { maxTokens: 8192,    contextLength: 128000  }],
  ['moonshot-v1-32k',     { maxTokens: 8192,    contextLength: 32000   }],
  ['moonshot-v1-8k',      { maxTokens: 8192,    contextLength: 8000    }],
  // MiniMax
  ['minimax-m1',          { maxTokens: 40960,   contextLength: 1000000 }],
  ['abab6.5',             { maxTokens: 8192,    contextLength: 245760  }],
  // GLM / Zhipu
  ['glm-4-long',          { maxTokens: 8192,    contextLength: 1000000 }],
  ['glm-4-flash',         { maxTokens: 8192,    contextLength: 128000  }],
  ['glm-4',               { maxTokens: 8192,    contextLength: 128000  }],
  ['glm-z1',              { maxTokens: 32768,   contextLength: 32768   }],
  // Qwen
  ['qwen3',               { maxTokens: 32768,   contextLength: 32768   }],
  ['qwen-long',           { maxTokens: 8192,    contextLength: 1000000 }],
  ['qwen-max',            { maxTokens: 8192,    contextLength: 32000   }],
  ['qwen-plus',           { maxTokens: 8192,    contextLength: 131072  }],
  ['qwen-turbo',          { maxTokens: 8192,    contextLength: 131072  }],
  ['qwq',                 { maxTokens: 32768,   contextLength: 131072  }],
  // Gemini
  ['gemini-2.5-pro',      { maxTokens: 65536,   contextLength: 1000000 }],
  ['gemini-2.5-flash',    { maxTokens: 65536,   contextLength: 1000000 }],
  ['gemini-2.0-flash',    { maxTokens: 8192,    contextLength: 1000000 }],
  ['gemini-1.5-pro',      { maxTokens: 8192,    contextLength: 1000000 }],
  ['gemini-1.5-flash',    { maxTokens: 8192,    contextLength: 1000000 }],
  // Llama
  ['llama-3.3',           { maxTokens: 32768,   contextLength: 131072  }],
  ['llama-3.1',           { maxTokens: 32768,   contextLength: 131072  }],
  ['llama-3',             { maxTokens: 8192,    contextLength: 8192    }],
];

function lookupModelLimits(modelName: string): { maxTokens: number; contextLength: number } | null {
  const lower = modelName.toLowerCase();
  for (const [prefix, limits] of MODEL_LIMITS_TABLE) {
    if (lower.startsWith(prefix)) return limits;
  }
  return null;
}

/** Ensure `options` includes `value`; append if missing */
function ensureOption(options: number[], value: number): number[] {
  return options.includes(value) ? options : [...options, value].sort((a, b) => a - b);
}

interface Props {
  onClose: () => void;
  groups: GroupInfo[];
  onRegisterGroup: (data: RegisterGroupPayload) => void;
  onRegisterFeishuApp: (appId: string, appSecret: string, domain?: string) => void;
  onRegisterQQApp: (appId: string, appSecret: string, sandbox?: boolean) => void;
  onUnregisterGroup: (jid: string) => void;
  onUpdateGroup: (jid: string, updates: UpdateGroupPayload) => void;
}

// ===== Helpers =====

/** Validate folder: alphanumeric + hyphens only */
function isValidFolder(s: string): boolean {
  return /^[a-z0-9-]+$/.test(s);
}

function slugify(s: string): string {
  return s.toLowerCase().replace(/\s+/g, '-').replace(/[^a-z0-9-]/g, '').slice(0, 32);
}

// ===== Sub-components =====

function Toggle({ value, onChange, disabled }: { value: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      type="button"
      onClick={() => !disabled && onChange(!value)}
      disabled={disabled}
      className={`relative flex-shrink-0 rounded-full transition-colors disabled:opacity-40 ${value ? 'bg-[#5BBFE8]' : 'bg-gray-200'}`}
      style={{ width: 40, height: 22 }}
      aria-pressed={value}
    >
      <span
        className={`absolute top-0.5 rounded-full bg-white shadow transition-transform`}
        style={{ width: 18, height: 18, transform: value ? 'translateX(19px)' : 'translateX(2px)' }}
      />
    </button>
  );
}

// ===== Permission Tab =====

interface PermissionsState {
  skipMainAgentPermissions: boolean;
  skipAllAgentsPermissions: boolean;
}

function PermissionsTab() {
  const [perms, setPerms]       = useState<PermissionsState>({ skipMainAgentPermissions: false, skipAllAgentsPermissions: false });
  const [loading, setLoading]   = useState(true);
  const [saving, setSaving]     = useState(false);
  const [feedback, setFeedback] = useState<{ ok: boolean; msg: string } | null>(null);

  useEffect(() => {
    fetch('/api/admin-permissions')
      .then(r => r.json())
      .then((d: PermissionsState) => { setPerms(d); setLoading(false); })
      .catch(() => setLoading(false));
  }, []);

  const save = async (next: PermissionsState) => {
    setSaving(true);
    setFeedback(null);
    try {
      const r = await fetch('/api/admin-permissions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(next),
      });
      if (!r.ok) throw new Error('failed');
      setPerms(next);
      setFeedback({ ok: true, msg: 'Saved' });
    } catch {
      setFeedback({ ok: false, msg: 'Something went wrong. Try again.' });
    } finally {
      setSaving(false);
    }
  };

  const toggleMain = () => save({ ...perms, skipMainAgentPermissions: !perms.skipMainAgentPermissions });
  const toggleAll  = () => {
    const next = !perms.skipAllAgentsPermissions;
    // When enabling "all agents", main agent toggle is also on (superset)
    save({ skipMainAgentPermissions: next ? true : perms.skipMainAgentPermissions, skipAllAgentsPermissions: next });
  };

  return (
    <section className="space-y-3">
      <p className="text-[11px] font-semibold text-[#5BBFE8] uppercase tracking-wide">Permissions</p>

      {/* Main agent */}
      <div className="bg-gray-50 rounded-xl p-3.5">
        <div className="flex items-center justify-between mb-2">
          <span className="text-sm font-medium text-gray-800">Skip approval for main agent</span>
          <Toggle
            value={perms.skipMainAgentPermissions || perms.skipAllAgentsPermissions}
            onChange={toggleMain}
            disabled={loading || saving || perms.skipAllAgentsPermissions}
          />
        </div>
        <p className="text-[11px] text-gray-400 leading-relaxed">
          When on, the main agent does not require step-by-step approval for file edits or Bash.
          {perms.skipAllAgentsPermissions && <span className="text-[#5BBFE8]"> (overridden by &quot;all agents&quot;)</span>}
        </p>
        {!loading && (
          <p className={`mt-1.5 text-[11px] font-medium ${(perms.skipMainAgentPermissions || perms.skipAllAgentsPermissions) ? 'text-[#5BBFE8]' : 'text-gray-400'}`}>
            {(perms.skipMainAgentPermissions || perms.skipAllAgentsPermissions) ? '● On' : '○ Off'}
          </p>
        )}
      </div>

      {/* All agents */}
      <div className="bg-gray-50 rounded-xl p-3.5">
        <div className="flex items-center justify-between mb-2">
          <span className="text-sm font-medium text-gray-800">Skip approval for all agents</span>
          <Toggle value={perms.skipAllAgentsPermissions} onChange={toggleAll} disabled={loading || saving} />
        </div>
        <p className="text-[11px] text-gray-400 leading-relaxed">
          When on, every agent (including dispatch workers) runs tools without approval. Use only in fully trusted local setups.
        </p>
        {!loading && (
          <p className={`mt-1.5 text-[11px] font-medium ${perms.skipAllAgentsPermissions ? 'text-amber-500' : 'text-gray-400'}`}>
            {perms.skipAllAgentsPermissions ? '● On' : '○ Off'}
          </p>
        )}
      </div>

      {feedback && (
        <p className={`text-[11px] px-1 ${feedback.ok ? 'text-green-600' : 'text-red-500'}`}>
          {feedback.msg}
        </p>
      )}
    </section>
  );
}

// ===== Agent Row =====

interface AgentRowProps {
  group: GroupInfo;
  onDelete: (jid: string) => void;
  onEditName: (jid: string, name: string) => void;
}

function AgentRow({ group, onDelete, onEditName }: AgentRowProps) {
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [editingName, setEditingName]     = useState(false);
  const [nameInput, setNameInput]         = useState(group.name);

  const handleSaveName = () => {
    const trimmed = nameInput.trim();
    if (trimmed && trimmed !== group.name) {
      onEditName(group.jid, trimmed);
    }
    setEditingName(false);
  };

  const avatar = (
    <div className={`w-9 h-9 rounded-full flex items-center justify-center text-white font-semibold text-sm flex-shrink-0 select-none ${
      group.isAdmin ? 'bg-amber-400' : 'bg-[#5BBFE8]'
    }`}>
      {group.name.charAt(0).toUpperCase()}
    </div>
  );

  return (
    <div className={`rounded-xl p-3 flex items-start gap-3 ${
      group.isAdmin ? 'bg-amber-50 border border-amber-100' : 'bg-gray-50 border border-gray-100'
    }`}>
      {avatar}

      <div className="flex-1 min-w-0">
        {/* Name row */}
        <div className="flex items-center gap-1.5 mb-0.5">
          {editingName ? (
            <input
              className="text-sm font-medium text-gray-800 border border-[#5BBFE8] rounded px-1.5 py-0.5 w-full outline-none"
              value={nameInput}
              onChange={e => setNameInput(e.target.value)}
              onBlur={handleSaveName}
              onKeyDown={e => { if (e.key === 'Enter') handleSaveName(); if (e.key === 'Escape') { setEditingName(false); setNameInput(group.name); } }}
              autoFocus
            />
          ) : (
            <>
              <span className="text-sm font-medium text-gray-800 truncate">{group.name}</span>
              {group.isAdmin && (
                <span className="text-[10px] bg-amber-100 text-amber-700 px-1.5 py-0.5 rounded font-semibold flex-shrink-0">Main</span>
              )}
              {/* Pencil icon to edit name */}
              <button
                onClick={() => { setNameInput(group.name); setEditingName(true); }}
                className="opacity-0 group-hover:opacity-100 hover:!opacity-100 flex-shrink-0 text-gray-400 hover:text-[#5BBFE8] transition-all"
                title="Edit name"
              >
                <svg xmlns="http://www.w3.org/2000/svg" className="w-3 h-3" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="m16.862 4.487 1.687-1.688a1.875 1.875 0 1 1 2.652 2.652L10.582 16.07a4.5 4.5 0 0 1-1.897 1.13L6 18l.8-2.685a4.5 4.5 0 0 1 1.13-1.897l8.932-8.931Z" />
                </svg>
              </button>
            </>
          )}
        </div>

        {/* ID + JID */}
        <p className="text-[11px] text-gray-400 font-mono truncate">ID: {group.folder}</p>
        <p className="text-[11px] text-gray-400 truncate">{group.jid}</p>

        {/* Tags */}
        <div className="flex gap-1.5 mt-1.5 flex-wrap">
          {(group.jid.startsWith('feishu:pending:') || group.jid.startsWith('qq:pending:')) && (
            <span className="text-[10px] bg-amber-50 text-amber-600 px-1.5 py-0.5 rounded">Pending bind</span>
          )}
          {group.channel ? (
            <span className="text-[10px] bg-gray-100 text-gray-500 px-1.5 py-0.5 rounded">
              {group.channel === 'telegram' ? 'Telegram' : group.channel === 'feishu' ? 'Feishu' : group.channel === 'qq' ? 'QQ' : group.channel}
            </span>
          ) : (
            <span className="text-[10px] bg-purple-50 text-purple-500 px-1.5 py-0.5 rounded">Web only</span>
          )}
          {group.requiresTrigger && group.channel && (
            <span className="text-[10px] bg-gray-100 text-gray-500 px-1.5 py-0.5 rounded">@mention</span>
          )}
          {group.allowedWorkDirs !== null && (
            <span className="text-[10px] bg-blue-50 text-blue-500 px-1.5 py-0.5 rounded">Workdir limits</span>
          )}
        </div>
      </div>

      {/* Delete (non-admin only) */}
      {!group.isAdmin && (
        <div className="flex-shrink-0">
          {confirmDelete ? (
            <div className="flex items-center gap-1">
              <button
                onClick={() => onDelete(group.jid)}
                className="text-[11px] bg-red-500 text-white px-2 py-1 rounded-lg hover:bg-red-600 transition-colors"
              >Confirm</button>
              <button
                onClick={() => setConfirmDelete(false)}
                className="text-[11px] bg-gray-100 text-gray-600 px-2 py-1 rounded-lg hover:bg-gray-200 transition-colors"
              >Cancel</button>
            </div>
          ) : (
            <button
              onClick={() => setConfirmDelete(true)}
              className="w-7 h-7 flex items-center justify-center rounded-lg hover:bg-red-50 text-gray-300 hover:text-red-400 transition-colors"
              title="Remove agent"
            >
              <svg xmlns="http://www.w3.org/2000/svg" className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0" />
              </svg>
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// ===== Create Agent Form =====

interface CreateFormProps {
  groups: GroupInfo[];
  onSubmit: (data: RegisterGroupPayload) => void;
  onRegisterFeishuApp: (appId: string, appSecret: string, domain?: string) => void;
  onRegisterQQApp: (appId: string, appSecret: string, sandbox?: boolean) => void;
  onCancel: () => void;
}

function CreateAgentForm({ groups, onSubmit, onRegisterFeishuApp, onRegisterQQApp, onCancel }: CreateFormProps) {
  const [name, setName]           = useState('');
  const [folder, setFolder]       = useState('');
  const [jid, setJid]             = useState('');
  const [channel, setChannel]     = useState<'telegram' | 'feishu' | 'qq' | ''>('');
  const [botToken, setBotToken]   = useState('');
  const [appSecret, setAppSecret] = useState('');
  const [tgChatId, setTgChatId]   = useState('');
  const [tgChatType, setTgChatType] = useState<'group' | 'user'>('group');
  const [qqSandbox, setQQSandbox]             = useState(false);
  const [requiresTrigger, setRequiresTrigger] = useState(true);
  const [submitting, setSubmitting]           = useState(false);
  const [errors, setErrors]                   = useState<Record<string, string>>({});

  const isWebOnly = channel === '';

  // Auto-derive folder from name
  const handleNameChange = (v: string) => {
    setName(v);
    const newSlug = slugify(v);
    if (!folder || folder === slugify(name)) {
      setFolder(newSlug);
      if (isWebOnly && (!jid || jid.startsWith('web:'))) {
        setJid(newSlug ? `web:${newSlug}` : '');
      }
    }
  };

  const handleFolderChange = (v: string) => {
    const clean = v.toLowerCase().replace(/[^a-z0-9-]/g, '');
    setFolder(clean);
    if (isWebOnly && (!jid || jid.startsWith('web:'))) {
      setJid(clean ? `web:${clean}` : '');
    }
  };

  const handleChannelChange = (v: 'telegram' | 'feishu' | 'qq' | '') => {
    setChannel(v);
    if (v === '') {
      if (!jid || jid.startsWith('tg:') || jid.startsWith('feishu:') || jid.startsWith('qq:') || jid === 'tg:group:' || jid === 'feishu:group:') {
        setJid(folder ? `web:${folder}` : '');
      }
    } else if (v === 'telegram') {
      setTgChatId('');
      setTgChatType('group');
      setJid('');
    } else if (v === 'feishu') {
      if (!jid || jid.startsWith('web:') || jid.startsWith('tg:') || jid.startsWith('qq:')) {
        setJid('');  // Feishu JID optional (bound on first message)
      }
    } else if (v === 'qq') {
      setJid(''); // QQ: pending until first message binds JID
    }
  };

  const handleTgChange = (chatId: string, chatType: 'group' | 'user') => {
    setTgChatId(chatId);
    setTgChatType(chatType);
    setJid(chatId.trim() ? `tg:${chatType}:${chatId.trim()}` : '');
  };

  const validate = (): boolean => {
    const errs: Record<string, string> = {};
    const trimName   = name.trim();
    const trimFolder = folder.trim();
    const trimJid    = jid.trim();

    if (!trimName) errs.name = 'Name is required';
    else if (groups.some(g => g.name.toLowerCase() === trimName.toLowerCase())) errs.name = 'Name already exists';

    if (!trimFolder) errs.folder = 'Agent ID is required';
    else if (!isValidFolder(trimFolder)) errs.folder = 'Only lowercase letters, digits, and hyphens';
    else if (groups.some(g => g.folder === trimFolder)) errs.folder = 'Agent ID already exists';

    if (channel === 'telegram') {
      const trimId = tgChatId.trim();
      if (!trimId) errs.tgChatId = 'Chat ID is required';
      else if (!/^-?\d+$/.test(trimId)) errs.tgChatId = 'Chat ID must be numeric';
      else if (groups.some(g => g.jid === trimJid)) errs.tgChatId = 'This chat is already registered';
      if (tgChatType === 'user' && !botToken.trim()) errs.botToken = 'Bot token is required for user binding';
    } else if (channel === 'feishu') {
      const hasAppId = !!botToken.trim();
      const hasAppSecret = !!appSecret.trim();
      if (hasAppId && !hasAppSecret) errs.appSecret = 'App Secret is required when App ID is set';
      if (!hasAppId && hasAppSecret) errs.appId = 'App ID is required when App Secret is set';
      // Feishu JID optional (stored as feishu:pending:{appId})
      if (trimJid && !trimJid.startsWith('feishu:')) errs.jid = 'Invalid Feishu JID (must start with feishu:)';
      else if (trimJid && groups.some(g => g.jid === trimJid)) errs.jid = 'This JID is already registered';
      if (!trimJid && !botToken.trim()) errs.appId = 'App ID is required when Chat JID is empty';
    } else if (channel === 'qq') {
      if (!botToken.trim()) errs.appId = 'App ID is required';
      if (!appSecret.trim()) errs.appSecret = 'App Secret is required';
      // Pending JID uniqueness
      const pendingJid = botToken.trim() ? `qq:pending:${botToken.trim()}` : '';
      if (pendingJid && groups.some(g => g.jid === pendingJid)) errs.appId = 'This QQ app is already bound';
    } else {
      // Web-only: auto JID; check uniqueness
      if (groups.some(g => g.jid === trimJid)) errs.jid = 'Agent JID already exists (change Agent ID)';
    }

    setErrors(errs);
    return Object.keys(errs).length === 0;
  };

  const handleSubmit = () => {
    if (!validate()) return;
    setSubmitting(true);
    const trimmedJid = jid.trim();
    const payload: RegisterGroupPayload = {
      folder: folder.trim(),
      name:   name.trim(),
    };
    // Pending bind (Feishu/QQ): omit JID; backend assigns {channel}:pending:{appId}
    if (trimmedJid) payload.jid = trimmedJid;
    if (channel) payload.channel = channel;
    if (channel) payload.requiresTrigger = requiresTrigger;
    if (botToken.trim()) payload.botToken = botToken.trim();
    // Feishu: register app credentials before registering group
    if (channel === 'feishu' && botToken.trim() && appSecret.trim()) {
      onRegisterFeishuApp(botToken.trim(), appSecret.trim());
    }
    // QQ: register app credentials before registering group
    if (channel === 'qq' && botToken.trim() && appSecret.trim()) {
      onRegisterQQApp(botToken.trim(), appSecret.trim(), qqSandbox || undefined);
    }
    onSubmit(payload);
    setTimeout(() => setSubmitting(false), 2000);
  };

  return (
    <div className="border border-[#5BBFE8]/30 rounded-xl p-4 bg-[#EEF7FD]/40 space-y-3">
      <p className="text-xs font-semibold text-gray-700">New agent</p>

      {/* Name */}
      <div>
        <label className="text-[11px] text-gray-500 mb-1 block">Display name <span className="text-red-400">*</span></label>
        <input
          className={`w-full text-sm border rounded-lg px-3 py-2 outline-none transition-colors ${errors.name ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
          placeholder="Team assistant"
          value={name}
          onChange={e => handleNameChange(e.target.value)}
        />
        {errors.name && <p className="text-[11px] text-red-500 mt-0.5">{errors.name}</p>}
      </div>

      {/* Folder / Agent ID */}
      <div>
        <label className="text-[11px] text-gray-500 mb-1 block">Agent ID <span className="text-red-400">*</span></label>
        <input
          className={`w-full text-sm font-mono border rounded-lg px-3 py-2 outline-none transition-colors ${errors.folder ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
          placeholder="work-group"
          value={folder}
          onChange={e => handleFolderChange(e.target.value)}
        />
        {errors.folder
          ? <p className="text-[11px] text-red-500 mt-0.5">{errors.folder}</p>
          : <p className="text-[11px] text-gray-400 mt-0.5">Lowercase letters, digits, hyphens only; cannot change after create</p>
        }
      </div>

      {/* Channel */}
      <div>
        <label className="text-[11px] text-gray-500 mb-1 block">Channel</label>
        <select
          className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] bg-white"
          value={channel}
          onChange={e => handleChannelChange(e.target.value as 'telegram' | 'feishu' | '')}
        >
          <option value="">None (Web only)</option>
          <option value="telegram">Telegram</option>
          <option value="feishu">Feishu</option>
          <option value="qq">QQ</option>
        </select>
        {isWebOnly && (
          <p className="text-[11px] text-gray-400 mt-0.5">Receives tasks from the main agent via the web UI only</p>
        )}
      </div>

      {/* Channel-specific binding fields */}
      {channel === 'telegram' && (
        <>
          {/* Chat ID + Type */}
          <div className="flex gap-2.5">
            <div className="flex-1">
              <label className="text-[11px] text-gray-500 mb-1 block">Chat ID <span className="text-red-400">*</span></label>
              <input
                className={`w-full text-sm font-mono border rounded-lg px-3 py-2 outline-none transition-colors ${errors.tgChatId ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
                placeholder="-100123456789"
                value={tgChatId}
                onChange={e => handleTgChange(e.target.value, tgChatType)}
              />
              {errors.tgChatId
                ? <p className="text-[11px] text-red-500 mt-0.5">{errors.tgChatId}</p>
                : <p className="text-[11px] text-gray-400 mt-0.5">Use @userinfobot to look it up</p>
              }
            </div>
            <div className="w-28 flex-shrink-0">
              <label className="text-[11px] text-gray-500 mb-1 block">Type</label>
              <select
                className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] bg-white"
                value={tgChatType}
                onChange={e => handleTgChange(tgChatId, e.target.value as 'group' | 'user')}
              >
                <option value="group">Group</option>
                <option value="user">User</option>
              </select>
            </div>
          </div>
          {/* Bot Token */}
          <div>
            <label className="text-[11px] text-gray-500 mb-1 block">
              Bot Token{tgChatType === 'user' ? <span className="text-red-400"> *</span> : ' (optional)'}
            </label>
            <input
              className={`w-full text-sm font-mono border rounded-lg px-3 py-2 outline-none transition-colors ${errors.botToken ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
              placeholder={tgChatType === 'user' ? 'Required: dedicated bot for this user' : 'Leave empty to use default bot from .env'}
              value={botToken}
              onChange={e => setBotToken(e.target.value)}
            />
            {errors.botToken && <p className="text-[11px] text-red-500 mt-0.5">{errors.botToken}</p>}
          </div>
          {/* Trigger */}
          <div>
            <label className="text-[11px] text-gray-500 mb-1 block">Require @mention</label>
            <div className="flex items-center gap-2 py-1">
              <Toggle value={requiresTrigger} onChange={setRequiresTrigger} />
              <span className="text-[11px] text-gray-500">{requiresTrigger ? 'On' : 'Off'}</span>
            </div>
          </div>
        </>
      )}

      {channel === 'feishu' && (
        <>
          {/* Feishu Chat JID (optional) */}
          <div>
            <label className="text-[11px] text-gray-500 mb-1 block">Chat JID <span className="text-gray-400">(optional)</span></label>
            <input
              className={`w-full text-sm font-mono border rounded-lg px-3 py-2 outline-none transition-colors ${errors.jid ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
              placeholder="feishu:group:oc_xxx or feishu:user:ou_xxx (optional)"
              value={jid}
              onChange={e => setJid(e.target.value)}
            />
            {errors.jid
              ? <p className="text-[11px] text-red-500 mt-0.5">{errors.jid}</p>
              : <p className="text-[11px] text-gray-400 mt-0.5">Leave empty to bind automatically when the bot receives the first message</p>
            }
          </div>
          {/* Trigger */}
          <div>
            <label className="text-[11px] text-gray-500 mb-1 block">Require @mention</label>
            <div className="flex items-center gap-2 py-1">
              <Toggle value={requiresTrigger} onChange={setRequiresTrigger} />
              <span className="text-[11px] text-gray-500">{requiresTrigger ? 'On' : 'Off'}</span>
            </div>
          </div>
          {/* App ID + App Secret */}
          <div className="bg-gray-50 rounded-lg p-3 space-y-2.5">
            <p className="text-[11px] font-medium text-gray-600">Feishu app credentials (optional; falls back to global defaults)</p>
            <div className="flex gap-2.5">
              <div className="flex-1">
                <label className="text-[11px] text-gray-500 mb-1 block">App ID</label>
                <input
                  className={`w-full text-sm font-mono border rounded-lg px-3 py-2 outline-none transition-colors ${errors.appId ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
                  placeholder="cli_xxx"
                  value={botToken}
                  onChange={e => setBotToken(e.target.value)}
                />
                {errors.appId && <p className="text-[11px] text-red-500 mt-0.5">{errors.appId}</p>}
              </div>
              <div className="flex-1">
                <label className="text-[11px] text-gray-500 mb-1 block">App Secret</label>
                <input
                  className={`w-full text-sm font-mono border rounded-lg px-3 py-2 outline-none transition-colors ${errors.appSecret ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
                  placeholder="Required if App ID is set"
                  type="password"
                  value={appSecret}
                  onChange={e => setAppSecret(e.target.value)}
                />
                {errors.appSecret && <p className="text-[11px] text-red-500 mt-0.5">{errors.appSecret}</p>}
              </div>
            </div>
          </div>
        </>
      )}

      {channel === 'qq' && (
        <>
          {/* QQ App ID + App Secret (required) */}
          <div className="bg-gray-50 rounded-lg p-3 space-y-2.5">
            <p className="text-[11px] font-medium text-gray-600">QQ app credentials</p>
            <div className="flex gap-2.5">
              <div className="flex-1">
                <label className="text-[11px] text-gray-500 mb-1 block">App ID <span className="text-red-400">*</span></label>
                <input
                  className={`w-full text-sm font-mono border rounded-lg px-3 py-2 outline-none transition-colors ${errors.appId ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
                  placeholder="QQ Open Platform App ID"
                  value={botToken}
                  onChange={e => setBotToken(e.target.value)}
                />
                {errors.appId && <p className="text-[11px] text-red-500 mt-0.5">{errors.appId}</p>}
              </div>
              <div className="flex-1">
                <label className="text-[11px] text-gray-500 mb-1 block">App Secret <span className="text-red-400">*</span></label>
                <input
                  className={`w-full text-sm font-mono border rounded-lg px-3 py-2 outline-none transition-colors ${errors.appSecret ? 'border-red-300 bg-red-50' : 'border-gray-200 focus:border-[#5BBFE8]'}`}
                  placeholder="QQ Open Platform App Secret"
                  type="password"
                  value={appSecret}
                  onChange={e => setAppSecret(e.target.value)}
                />
                {errors.appSecret && <p className="text-[11px] text-red-500 mt-0.5">{errors.appSecret}</p>}
              </div>
            </div>
            {/* Sandbox toggle */}
            <div className="flex items-center gap-2">
              <Toggle value={qqSandbox} onChange={setQQSandbox} />
              <span className="text-[11px] text-gray-500">Sandbox mode</span>
            </div>
          </div>
          {/* Trigger */}
          <div>
            <label className="text-[11px] text-gray-500 mb-1 block">Require @mention</label>
            <div className="flex items-center gap-2 py-1">
              <Toggle value={requiresTrigger} onChange={setRequiresTrigger} />
              <span className="text-[11px] text-gray-500">{requiresTrigger ? 'On' : 'Off'}</span>
            </div>
          </div>
          <p className="text-[11px] text-gray-400">
            Chat JID binds automatically when the bot receives the first QQ message; no manual entry needed.
          </p>
        </>
      )}

      {/* Actions */}
      <div className="flex justify-end gap-2 pt-1">
        <button
          onClick={onCancel}
          className="text-sm px-3 py-1.5 rounded-lg border border-gray-200 text-gray-600 hover:bg-gray-50 transition-colors"
        >Cancel</button>
        <button
          onClick={handleSubmit}
          disabled={submitting}
          className="text-sm px-4 py-1.5 rounded-lg bg-[#5BBFE8] text-white hover:bg-[#3AAAD4] transition-colors disabled:opacity-50"
        >
          {submitting ? 'Creating…' : 'Create agent'}
        </button>
      </div>
    </div>
  );
}

// ===== Agent Management Tab =====

interface AgentsTabProps {
  groups: GroupInfo[];
  onRegisterGroup: (data: RegisterGroupPayload) => void;
  onRegisterFeishuApp: (appId: string, appSecret: string, domain?: string) => void;
  onRegisterQQApp: (appId: string, appSecret: string, sandbox?: boolean) => void;
  onUnregisterGroup: (jid: string) => void;
  onUpdateGroup: (jid: string, updates: UpdateGroupPayload) => void;
}

function AgentsTab({ groups, onRegisterGroup, onRegisterFeishuApp, onRegisterQQApp, onUnregisterGroup, onUpdateGroup }: AgentsTabProps) {
  const [showCreate, setShowCreate] = useState(false);

  const adminGroup = groups.find(g => g.isAdmin);
  const otherGroups = groups.filter(g => !g.isAdmin);

  const handleCreate = (data: RegisterGroupPayload) => {
    onRegisterGroup(data);
    setShowCreate(false);
  };

  const handleEditName = (jid: string, name: string) => {
    // Uniqueness check
    if (groups.some(g => g.jid !== jid && g.name.toLowerCase() === name.toLowerCase())) {
      return; // silently skip duplicate name
    }
    onUpdateGroup(jid, { name });
  };

  return (
    <div className="space-y-3">
      {/* Section header */}
      <div className="flex items-center justify-between">
        <p className="text-[11px] font-semibold text-[#5BBFE8] uppercase tracking-wide">
          Agents <span className="text-gray-400 font-normal normal-case">({groups.length})</span>
        </p>
        {!showCreate && (
          <button
            onClick={() => setShowCreate(true)}
            className="flex items-center gap-1 text-[11px] text-[#5BBFE8] hover:text-[#3AAAD4] font-medium transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2.5} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
            Add
          </button>
        )}
      </div>

      {/* Create form */}
      {showCreate && (
        <CreateAgentForm
          groups={groups}
          onSubmit={handleCreate}
          onRegisterFeishuApp={onRegisterFeishuApp}
          onRegisterQQApp={onRegisterQQApp}
          onCancel={() => setShowCreate(false)}
        />
      )}

      {/* Admin agent — always at top */}
      {adminGroup && (
        <div className="group">
          <AgentRow
            group={adminGroup}
            onDelete={() => {/* admin cannot be deleted */}}
            onEditName={handleEditName}
          />
        </div>
      )}

      {/* Regular agents */}
      {otherGroups.length > 0 && (
        <div className="space-y-2">
          {otherGroups.map(g => (
            <div key={g.jid} className="group">
              <AgentRow
                group={g}
                onDelete={onUnregisterGroup}
                onEditName={handleEditName}
              />
            </div>
          ))}
        </div>
      )}

      {groups.length === 0 && (
        <p className="text-[11px] text-gray-400 text-center py-4">No agents yet</p>
      )}

      {/* Dispatch note */}
      {otherGroups.length > 0 && (
        <div className="bg-blue-50/60 rounded-xl p-3 mt-1">
          <p className="text-[11px] text-[#3AAAD4] leading-relaxed">
            💡 The main agent can message other agents&apos; chats via the <span className="font-mono bg-white px-1 rounded">send_message</span> tool,
            or dispatch by naming agents in natural language.
          </p>
        </div>
      )}
    </div>
  );
}

// ===== LLM Add Model Slide Panel =====

interface AddModelPanelProps {
  onClose: () => void;
  onSaved: () => void;
}

function AddModelPanel({ onClose, onSaved }: AddModelPanelProps) {
  const [provider, setProvider]         = useState('anthropic');
  const [baseURL, setBaseURL]           = useState(PROVIDERS['anthropic'].baseURL);
  const [apiKey, setApiKey]             = useState('');
  const [adapt, setAdapt]               = useState<'openai' | 'anthropic'>('anthropic');
  const [modelName, setModelName]       = useState('');
  const [selectedModel, setSelectedModel] = useState('');
  const [isManual, setIsManual]         = useState(false);
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [maxTokens, setMaxTokens]       = useState(8192);
  const [contextLength, setContextLength] = useState(128000);
  const [showKey, setShowKey]           = useState(false);
  const [fetching, setFetching]         = useState(false);
  const [testStatus, setTestStatus]     = useState<{ msg: string; type: 'ok' | 'err' | 'loading' | '' }>({ msg: '', type: '' });
  const [saving, setSaving]             = useState(false);
  const [connTested, setConnTested]     = useState(false);
  const [connOk, setConnOk]             = useState(false);

  const currentModel = isManual ? modelName : selectedModel;

  // Auto-fill known limits when model name changes
  useEffect(() => {
    if (!currentModel) return;
    const limits = lookupModelLimits(currentModel);
    if (limits) {
      setMaxTokens(limits.maxTokens);
      setContextLength(limits.contextLength);
    }
  }, [currentModel]);

  const handleProviderChange = (p: string) => {
    const def = PROVIDERS[p];
    setProvider(p);
    setBaseURL(def.baseURL);
    setAdapt(def.defaultAdapt);
    setApiKey('');
    setModelName('');
    setSelectedModel('');
    setAvailableModels([]);
    setConnTested(false); setConnOk(false);
    setTestStatus({ msg: '', type: '' });
    setMaxTokens(def.defaultMaxTokens ?? 8192);
    setContextLength(def.defaultContextLength ?? 128000);
  };

  const handleFetchModels = async () => {
    if (!baseURL) { setTestStatus({ msg: 'Enter model base URL', type: 'err' }); return; }
    if (!apiKey)  { setTestStatus({ msg: 'Enter API key', type: 'err' }); return; }
    setFetching(true);
    setTestStatus({ msg: 'Fetching model list…', type: 'loading' });
    // Use provider-specific models URL if available (some providers use a different endpoint for listing models)
    const fetchBaseURL = PROVIDERS[provider]?.modelsUrl ?? baseURL;
    try {
      const r = await fetch('/api/llm-config/models', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ baseURL: fetchBaseURL, apiKey, adapt }),
      });
      const data = await r.json() as { success: boolean; models?: string[]; message?: string };
      if (data.success && data.models?.length) {
        setAvailableModels(data.models);
        setSelectedModel(data.models[0]);
        setTestStatus({ msg: `✓ Loaded ${data.models.length} model(s)`, type: 'ok' });
        setTimeout(() => setTestStatus({ msg: '', type: '' }), 3000);
      } else {
        setTestStatus({ msg: `✗ ${data.message ?? 'Failed to fetch'}`, type: 'err' });
      }
    } catch {
      setTestStatus({ msg: '✗ Network error', type: 'err' });
    } finally {
      setFetching(false);
    }
  };

  const handleTest = async () => {
    if (!baseURL || !apiKey) { setTestStatus({ msg: 'Enter base URL and API key', type: 'err' }); return; }
    setTestStatus({ msg: 'Testing connection…', type: 'loading' });
    // Use provider-specific models URL for connection test (avoids auth mismatch on models endpoint)
    const testBaseURL = PROVIDERS[provider]?.modelsUrl ?? baseURL;
    try {
      const r = await fetch('/api/llm-config/test', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ baseURL: testBaseURL, apiKey, adapt }),
      });
      const data = await r.json() as { success: boolean; message?: string };
      setConnTested(true); setConnOk(data.success);
      setTestStatus({ msg: data.success ? '✓ Connection OK' : `✗ ${data.message ?? 'Connection failed'}`, type: data.success ? 'ok' : 'err' });
    } catch {
      setConnTested(true); setConnOk(false);
      setTestStatus({ msg: '✗ Network error', type: 'err' });
    }
  };

  const handleSave = async () => {
    if (!apiKey)         { setTestStatus({ msg: 'Enter API key', type: 'err' }); return; }
    if (!currentModel)   { setTestStatus({ msg: 'Select or enter a model name', type: 'err' }); return; }
    if (!baseURL)        { setTestStatus({ msg: 'Enter model base URL', type: 'err' }); return; }
    if (!connTested)     { setTestStatus({ msg: '⚠ Run "Test connection" first', type: 'err' }); return; }
    if (!connOk)         { setTestStatus({ msg: '⚠ Connection test failed; fix settings and test again', type: 'err' }); return; }
    setSaving(true);
    const label = `${currentModel} (${PROVIDERS[provider]?.name ?? provider})`;
    try {
      const r = await fetch('/api/llm-config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ label, provider, baseURL, apiKey, modelName: currentModel, adapt, maxTokens, contextLength }),
      });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      onSaved();
      onClose();
    } catch {
      setTestStatus({ msg: 'Save failed, try again', type: 'err' });
    } finally {
      setSaving(false);
    }
  };

  const def = PROVIDERS[provider];

  return (
    <aside
      className="relative h-full bg-white border-l border-gray-100 flex flex-col shadow-lg overflow-y-auto"
      style={{ width: 480 }}
      onClick={e => e.stopPropagation()}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-5 py-4 border-b border-gray-100 flex-shrink-0 sticky top-0 bg-white z-10">
        <span className="font-semibold text-gray-800 text-sm">Add model</span>
        <button onClick={onClose} className="w-6 h-6 flex items-center justify-center rounded-lg hover:bg-gray-100 text-gray-400 hover:text-gray-600 transition-colors">
          <svg xmlns="http://www.w3.org/2000/svg" className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" d="M6 18 18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      <div className="flex-1 p-5 space-y-4">
        {/* Provider */}
        <div>
          <label className="text-[11px] text-gray-500 mb-1 block">Provider</label>
          <select
            className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] bg-white"
            value={provider}
            onChange={e => handleProviderChange(e.target.value)}
          >
            {PROVIDER_ORDER.map(k => (
              <option key={k} value={k}>{PROVIDERS[k].name}</option>
            ))}
          </select>
        </div>

        {/* Base URL */}
        <div>
          <label className="text-[11px] text-gray-500 mb-1 block">Base URL</label>
          <input
            className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] transition-colors"
            placeholder={def.baseURLPlaceholder ?? def.baseURL}
            value={baseURL}
            onChange={e => { setBaseURL(e.target.value); setConnTested(false); setConnOk(false); }}
          />
        </div>

        {/* API Key */}
        <div>
          <label className="text-[11px] text-gray-500 mb-1 block">API Key</label>
          <div className="relative">
            <input
              type={showKey ? 'text' : 'password'}
              className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 pr-10 outline-none focus:border-[#5BBFE8] transition-colors"
              placeholder={def.apiKeyPlaceholder ?? 'Your API key'}
              value={apiKey}
              onChange={e => { setApiKey(e.target.value.trim()); setConnTested(false); setConnOk(false); }}
            />
            <button
              type="button"
              onClick={() => setShowKey(v => !v)}
              className="absolute right-2.5 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
            >
              {showKey ? (
                <svg xmlns="http://www.w3.org/2000/svg" className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M3.98 8.223A10.477 10.477 0 0 0 1.934 12C3.226 16.338 7.244 19.5 12 19.5c.993 0 1.953-.138 2.863-.395M6.228 6.228A10.451 10.451 0 0 1 12 4.5c4.756 0 8.773 3.162 10.065 7.498a10.522 10.522 0 0 1-4.293 5.774M6.228 6.228 3 3m3.228 3.228 3.65 3.65m7.894 7.894L21 21m-3.228-3.228-3.65-3.65m0 0a3 3 0 1 0-4.243-4.243m4.242 4.242L9.88 9.88" />
                </svg>
              ) : (
                <svg xmlns="http://www.w3.org/2000/svg" className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" d="M2.036 12.322a1.012 1.012 0 0 1 0-.639C3.423 7.51 7.36 4.5 12 4.5c4.638 0 8.573 3.007 9.963 7.178.07.207.07.431 0 .639C20.577 16.49 16.64 19.5 12 19.5c-4.638 0-8.573-3.007-9.963-7.178Z" />
                  <path strokeLinecap="round" strokeLinejoin="round" d="M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z" />
                </svg>
              )}
            </button>
          </div>
        </div>

        {/* Model name */}
        <div>
          <div className="flex items-center justify-between mb-1">
            <label className="text-[11px] text-gray-500">Model name</label>
            <button
              type="button"
              className="text-[11px] text-[#5BBFE8] hover:text-[#3AAAD4]"
              onClick={() => setIsManual(v => !v)}
            >
              {isManual ? 'Pick from list' : 'Enter manually'}
            </button>
          </div>
          {isManual ? (
            <input
              className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] transition-colors"
              placeholder="Enter model name"
              value={modelName}
              onChange={e => { setModelName(e.target.value); setConnTested(false); setConnOk(false); }}
            />
          ) : (
            <div className="flex gap-2">
              <select
                className="flex-1 min-w-0 text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] bg-white"
                value={selectedModel}
                onChange={e => { setSelectedModel(e.target.value); setConnTested(false); setConnOk(false); }}
              >
                {availableModels.length === 0
                  ? <option value="">-- Fetch model list first --</option>
                  : availableModels.map(m => <option key={m} value={m}>{m}</option>)
                }
              </select>
              <button
                type="button"
                onClick={handleFetchModels}
                disabled={fetching}
                className="px-3 py-2 text-xs rounded-lg border border-gray-200 text-gray-600 hover:bg-gray-50 disabled:opacity-50 transition-colors whitespace-nowrap"
              >
                {fetching ? 'Fetching…' : 'Fetch models'}
              </button>
            </div>
          )}
        </div>

        {/* API type */}
        <div>
          <label className="text-[11px] text-gray-500 mb-1 block">API style</label>
          <select
            className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] bg-white"
            value={adapt}
            onChange={e => setAdapt(e.target.value as 'openai' | 'anthropic')}
          >
            <option value="openai">OpenAI-compatible</option>
            <option value="anthropic">Anthropic-compatible</option>
          </select>
        </div>

        {/* Max tokens + context length */}
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="text-[11px] text-gray-500 mb-1 block">Max output tokens</label>
            <select
              className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] bg-white"
              value={maxTokens}
              onChange={e => setMaxTokens(Number(e.target.value))}
            >
              {ensureOption(DEFAULT_MAX_TOKENS_OPTIONS, maxTokens).map(v => (
                <option key={v} value={v}>{Math.round(v / 1000)}k</option>
              ))}
            </select>
          </div>
          <div>
            <label className="text-[11px] text-gray-500 mb-1 block">Context window</label>
            <select
              className="w-full text-sm border border-gray-200 rounded-lg px-3 py-2 outline-none focus:border-[#5BBFE8] bg-white"
              value={contextLength}
              onChange={e => setContextLength(Number(e.target.value))}
            >
              {ensureOption(DEFAULT_CONTEXT_LENGTH_OPTIONS, contextLength).map(v => (
                <option key={v} value={v}>{Math.round(v / 1000)}k</option>
              ))}
            </select>
          </div>
        </div>

        {/* Status */}
        {testStatus.type && (
          <p className={`text-[11px] px-1 ${testStatus.type === 'ok' ? 'text-green-600' : testStatus.type === 'err' ? 'text-red-500' : 'text-gray-400'}`}>
            {testStatus.msg}
          </p>
        )}

        {/* Actions */}
        <div className="flex gap-2 pt-1">
          <button
            type="button"
            onClick={handleTest}
            disabled={testStatus.type === 'loading' || saving}
            className="flex-1 text-sm py-2 rounded-lg border border-gray-200 text-gray-600 hover:bg-gray-50 disabled:opacity-50 transition-colors"
          >
            {testStatus.type === 'loading' ? 'Testing…' : 'Test connection'}
          </button>
          <button
            type="button"
            onClick={handleSave}
            disabled={saving || testStatus.type === 'loading'}
            className="flex-1 text-sm py-2 rounded-lg bg-[#5BBFE8] text-white hover:bg-[#3AAAD4] disabled:opacity-50 transition-colors"
          >
            {saving ? 'Adding…' : 'Add model'}
          </button>
        </div>
      </div>
    </aside>
  );
}

// ===== LLM Config Tab =====

interface LLMTabProps {
  onOpenAdd: () => void;
  refreshKey: number;
}

function LLMTab({ onOpenAdd, refreshKey }: LLMTabProps) {
  const [configs, setConfigs]         = useState<LLMConfig[]>([]);
  const [activeId, setActiveId]       = useState<string | null>(null);
  const [activeQuickId, setActiveQuickId] = useState<string | null>(null);
  const [semaModel, setSemaModel]     = useState<{ modelName: string; provider: string } | null>(null);
  const [semaQuickModel, setSemaQuickModel] = useState<{ modelName: string; provider: string } | null>(null);
  const [thinkingEnabled, setThinkingEnabled] = useState(true);

  const load = async () => {
    try {
      const r = await fetch('/api/llm-config');
      const data = await r.json() as {
        configs: LLMConfig[];
        activeId: string | null;
        activeQuickId: string | null;
        semaModel?: { modelName: string; provider: string } | null;
        semaQuickModel?: { modelName: string; provider: string } | null;
        thinkingEnabled?: boolean;
      };
      setConfigs(data.configs);
      setActiveId(data.activeId);
      setActiveQuickId(data.activeQuickId);
      setSemaModel(data.semaModel ?? null);
      setSemaQuickModel(data.semaQuickModel ?? null);
      setThinkingEnabled(data.thinkingEnabled ?? true);
    } catch { /* ignore */ }
  };

  const handleThinkingToggle = async () => {
    const next = !thinkingEnabled;
    setThinkingEnabled(next);
    await fetch('/api/thinking', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ enabled: next }),
    });
  };

  useEffect(() => { load(); }, [refreshKey]);

  const handleSetMain = async (id: string) => {
    await fetch('/api/llm-config/active', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, type: 'main' }),
    });
    setActiveId(id);
    const cfg = configs.find(c => c.id === id);
    if (cfg) setSemaModel({ modelName: cfg.modelName, provider: cfg.provider });
  };

  const handleSetQuick = async (id: string) => {
    await fetch('/api/llm-config/active', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ id, type: 'quick' }),
    });
    setActiveQuickId(id);
    const cfg = configs.find(c => c.id === id);
    if (cfg) setSemaQuickModel({ modelName: cfg.modelName, provider: cfg.provider });
  };

  const handleDelete = async (id: string) => {
    await fetch(`/api/llm-config/${encodeURIComponent(id)}`, { method: 'DELETE' });
    load();
  };

  const activeConfig      = configs.find(c => c.id === activeId);
  const activeQuickConfig = configs.find(c => c.id === activeQuickId);

  const displayMain = activeConfig
    ? { modelName: activeConfig.modelName, providerLabel: PROVIDERS[activeConfig.provider]?.name ?? activeConfig.provider }
    : semaModel
    ? { modelName: semaModel.modelName, providerLabel: PROVIDERS[semaModel.provider]?.name ?? semaModel.provider }
    : null;

  const displayQuick = activeQuickConfig
    ? { modelName: activeQuickConfig.modelName, providerLabel: PROVIDERS[activeQuickConfig.provider]?.name ?? activeQuickConfig.provider }
    : semaQuickModel
    ? { modelName: semaQuickModel.modelName, providerLabel: PROVIDERS[semaQuickModel.provider]?.name ?? semaQuickModel.provider }
    : displayMain; // fallback: same as main model

  return (
    <div className="space-y-3">
      {/* Current active models */}
      <div>
        <p className="text-[11px] font-semibold text-[#5BBFE8] uppercase tracking-wide mb-2">Active models</p>
        {displayMain || displayQuick ? (
          <div className="bg-[#EEF7FD]/60 border border-[#5BBFE8]/20 rounded-xl p-3.5 space-y-2.5">
            {/* Main model row */}
            <div className="flex items-center gap-2">
              <div className="w-2 h-2 rounded-full bg-[#5BBFE8] flex-shrink-0" />
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-gray-800 truncate">{displayMain?.modelName ?? '—'}</p>
                <p className="text-[11px] text-gray-400">{displayMain?.providerLabel ?? ''}</p>
              </div>
              <span className="text-[10px] text-[#5BBFE8] font-medium bg-[#5BBFE8]/10 px-1.5 py-0.5 rounded-md flex-shrink-0">Main</span>
            </div>
            {/* Quick model row */}
            <div className="flex items-center gap-2">
              <div className="w-2 h-2 rounded-full bg-[#A78BFA] flex-shrink-0" />
              <div className="min-w-0 flex-1">
                <p className="text-sm font-medium text-gray-800 truncate">{displayQuick?.modelName ?? '—'}</p>
                <p className="text-[11px] text-gray-400">{displayQuick?.providerLabel ?? ''}</p>
              </div>
              <span className="text-[10px] text-[#A78BFA] font-medium bg-[#A78BFA]/10 px-1.5 py-0.5 rounded-md flex-shrink-0">Quick</span>
            </div>
            {/* Thinking toggle row */}
            <div className="flex items-center justify-between pt-1.5 border-t border-[#5BBFE8]/10">
              <span className="text-[11px] text-gray-500">Chain-of-thought (Thinking)</span>
              <button
                onClick={handleThinkingToggle}
                className={`relative inline-flex h-4 w-7 items-center rounded-full transition-colors flex-shrink-0 ${
                  thinkingEnabled ? 'bg-[#5BBFE8]' : 'bg-gray-200'
                }`}
                role="switch"
                aria-checked={thinkingEnabled}
              >
                <span className={`inline-block h-3 w-3 transform rounded-full bg-white shadow transition-transform ${
                  thinkingEnabled ? 'translate-x-3.5' : 'translate-x-0.5'
                }`} />
              </button>
            </div>
          </div>
        ) : (
          <div className="bg-gray-50 rounded-xl p-3.5">
            <p className="text-[11px] text-gray-400">No model configured</p>
          </div>
        )}
      </div>

      {/* Saved configs list */}
      <div>
        <div className="flex items-center justify-between mb-2">
          <p className="text-[11px] font-semibold text-[#5BBFE8] uppercase tracking-wide">
            Saved models <span className="text-gray-400 font-normal normal-case">({configs.length})</span>
          </p>
          <button
            onClick={onOpenAdd}
            className="flex items-center gap-1 text-[11px] text-[#5BBFE8] hover:text-[#3AAAD4] font-medium transition-colors"
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={2.5} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
            </svg>
            Add
          </button>
        </div>

        {configs.length === 0 ? (
          <p className="text-[11px] text-gray-400 text-center py-6">No saved configs yet — click Add to add a model</p>
        ) : (
          <div className="space-y-2">
            {configs.map(c => {
              const isMain  = c.id === activeId;
              const isQuick = c.id === activeQuickId;
              return (
                <div
                  key={c.id}
                  className={`rounded-xl p-3 border transition-all ${
                    isMain || isQuick
                      ? 'bg-[#EEF7FD]/60 border-[#5BBFE8]/30'
                      : 'bg-gray-50 border-gray-100'
                  }`}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5 flex-wrap">
                        <p className="text-sm font-medium text-gray-800 truncate">{c.modelName}</p>
                        {isMain  && <span className="text-[10px] text-[#5BBFE8] font-medium bg-[#5BBFE8]/10 px-1.5 py-0.5 rounded-md">Main</span>}
                        {isQuick && <span className="text-[10px] text-[#A78BFA] font-medium bg-[#A78BFA]/10 px-1.5 py-0.5 rounded-md">Quick</span>}
                      </div>
                      <p className="text-[11px] text-gray-400 mt-0.5">{PROVIDERS[c.provider]?.name ?? c.provider} · {c.adapt === 'anthropic' ? 'Anthropic API' : 'OpenAI API'}</p>
                      <p className="text-[11px] text-gray-300 font-mono truncate mt-0.5">{c.baseURL}</p>
                    </div>
                    <button
                      onClick={() => handleDelete(c.id)}
                      className="flex-shrink-0 w-6 h-6 flex items-center justify-center rounded-lg hover:bg-red-50 text-gray-300 hover:text-red-400 transition-colors"
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" strokeWidth={1.5} stroke="currentColor">
                        <path strokeLinecap="round" strokeLinejoin="round" d="m14.74 9-.346 9m-4.788 0L9.26 9m9.968-3.21c.342.052.682.107 1.022.166m-1.022-.165L18.16 19.673a2.25 2.25 0 0 1-2.244 2.077H8.084a2.25 2.25 0 0 1-2.244-2.077L4.772 5.79m14.456 0a48.108 48.108 0 0 0-3.478-.397m-12 .562c.34-.059.68-.114 1.022-.165m0 0a48.11 48.11 0 0 1 3.478-.397m7.5 0v-.916c0-1.18-.91-2.164-2.09-2.201a51.964 51.964 0 0 0-3.32 0c-1.18.037-2.09 1.022-2.09 2.201v.916m7.5 0a48.667 48.667 0 0 0-7.5 0" />
                      </svg>
                    </button>
                  </div>
                  {/* Action buttons */}
                  <div className="flex gap-1.5 mt-2">
                    <button
                      onClick={() => handleSetMain(c.id)}
                      className={`flex-1 py-1 text-[11px] font-medium rounded-lg transition-colors ${
                        isMain
                          ? 'bg-[#5BBFE8]/15 text-[#5BBFE8] cursor-default'
                          : 'bg-gray-100 text-gray-500 hover:bg-[#5BBFE8]/10 hover:text-[#5BBFE8]'
                      }`}
                      disabled={isMain}
                    >
                      {isMain ? '● Main model' : 'Set as main'}
                    </button>
                    <button
                      onClick={() => handleSetQuick(c.id)}
                      className={`flex-1 py-1 text-[11px] font-medium rounded-lg transition-colors ${
                        isQuick
                          ? 'bg-[#A78BFA]/15 text-[#A78BFA] cursor-default'
                          : 'bg-gray-100 text-gray-500 hover:bg-[#A78BFA]/10 hover:text-[#A78BFA]'
                      }`}
                      disabled={isQuick}
                    >
                      {isQuick ? '● Quick model' : 'Set as quick'}
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}

// ===== Tab Bar =====

function TabBar({ active, onChange }: { active: Tab; onChange: (t: Tab) => void }) {
  const tabs: { id: Tab; label: string }[] = [
    { id: 'permissions', label: 'Permissions' },
    { id: 'agents',      label: 'Agents' },
    { id: 'llm',         label: 'LLM' },
  ];
  return (
    <div className="flex border-b border-gray-100">
      {tabs.map(t => (
        <button
          key={t.id}
          onClick={() => onChange(t.id)}
          className={`flex-1 py-2.5 text-xs font-medium transition-colors border-b-2 -mb-px ${
            active === t.id
              ? 'text-[#5BBFE8] border-[#5BBFE8]'
              : 'text-gray-400 border-transparent hover:text-gray-600'
          }`}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

// ===== Main SettingsPanel =====

export function SettingsPanel({ onClose, groups, onRegisterGroup, onRegisterFeishuApp, onRegisterQQApp, onUnregisterGroup, onUpdateGroup }: Props) {
  const [tab, setTab]           = useState<Tab>('agents');
  const [showAddLLM, setShowAddLLM] = useState(false);
  const [llmRefreshKey, setLlmRefreshKey] = useState(0);

  return (
    <div className="fixed inset-0 z-50 flex">
      <div className="absolute inset-0 bg-black/20" onClick={onClose} />

      <aside
        className="relative h-full bg-white border-r border-gray-100 flex flex-col shadow-lg"
        style={{ width: 360 }}
        onClick={e => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-100 flex-shrink-0">
          <span className="font-semibold text-gray-800 text-sm">Settings</span>
          <button
            onClick={onClose}
            className="w-6 h-6 flex items-center justify-center rounded-lg hover:bg-gray-100 text-gray-400 hover:text-gray-600 transition-colors"
            aria-label="Close"
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18 18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        {/* Tabs */}
        <div className="px-4 flex-shrink-0">
          <TabBar active={tab} onChange={t => { setTab(t); setShowAddLLM(false); }} />
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          {tab === 'permissions' && <PermissionsTab />}
          {tab === 'agents' && (
            <AgentsTab
              groups={groups}
              onRegisterGroup={onRegisterGroup}
              onRegisterFeishuApp={onRegisterFeishuApp}
              onRegisterQQApp={onRegisterQQApp}
              onUnregisterGroup={onUnregisterGroup}
              onUpdateGroup={onUpdateGroup}
            />
          )}
          {tab === 'llm' && (
            <LLMTab
              onOpenAdd={() => setShowAddLLM(true)}
              refreshKey={llmRefreshKey}
            />
          )}
        </div>
      </aside>

      {/* LLM add model slide panel */}
      {showAddLLM && (
        <AddModelPanel
          onClose={() => setShowAddLLM(false)}
          onSaved={() => setLlmRefreshKey(k => k + 1)}
        />
      )}
    </div>
  );
}
