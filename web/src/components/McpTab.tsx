import { useState, useEffect, useCallback } from 'react';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface McpToolDef {
  name: string;
  description?: string | null;
}

interface McpServerItem {
  name: string;
  transport: 'stdio' | 'sse' | 'http';
  description?: string | null;
  enabled: boolean;
  use_tools?: string[] | null;
  command?: string | null;
  args: string[];
  env: Record<string, string>;
  url?: string | null;
  headers: Record<string, string>;
  scope: 'user' | 'project';
  status: 'disconnected' | 'connecting' | 'connected' | 'error';
  tools?: McpToolDef[] | null;
  error?: string | null;
  builtin: boolean;
}

type Transport = 'stdio' | 'sse' | 'http';
type Feedback = { ok: boolean; msg: string } | null;

// ---------------------------------------------------------------------------
// Status badge
// ---------------------------------------------------------------------------

function statusColor(s: string) {
  switch (s) {
    case 'connected':    return 'bg-emerald-100 text-emerald-700';
    case 'connecting':   return 'bg-amber-100 text-amber-700';
    case 'error':        return 'bg-red-100 text-red-700';
    case 'disconnected':
    default:             return 'bg-gray-100 text-gray-500';
  }
}

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

export function McpTab() {
  const [servers, setServers]       = useState<McpServerItem[]>([]);
  const [loading, setLoading]       = useState(true);
  const [feedback, setFeedback]     = useState<Feedback>(null);
  const [showAdd, setShowAdd]       = useState(false);
  const [editing, setEditing]       = useState<string | null>(null); // server name being edited (tools filter)
  const [toolFilter, setToolFilter] = useState('');

  // ---- load ----

  const load = useCallback(async () => {
    try {
      const r = await fetch('/api/mcp-servers');
      const data = await r.json();
      setServers((data.servers || []) as McpServerItem[]);
    } catch {
      /* ignore */
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  // ---- actions ----

  const flash = (ok: boolean, msg: string) => {
    setFeedback({ ok, msg });
    setTimeout(() => setFeedback(null), 2500);
  };

  const connect = async (name: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/connect`, { method: 'POST' });
      if (!r.ok) throw new Error();
      await load();
      flash(true, `${name} connected`);
    } catch {
      flash(false, `Failed to connect ${name}`);
    }
  };

  const disconnect = async (name: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/disconnect`, { method: 'POST' });
      if (!r.ok) throw new Error();
      await load();
      flash(true, `${name} disconnected`);
    } catch {
      flash(false, `Failed to disconnect ${name}`);
    }
  };

  const toggleEnabled = async (name: string, enabled: boolean, scope: string) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/enabled`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ enabled, scope }),
      });
      if (!r.ok) throw new Error();
      await load();
      flash(true, `${name} ${enabled ? 'enabled' : 'disabled'}`);
    } catch {
      flash(false, `Failed to update ${name}`);
    }
  };

  const removeServer = async (name: string, scope: string) => {
    if (!confirm(`Delete MCP server "${name}"?`)) return;
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}?scope=${encodeURIComponent(scope)}`, { method: 'DELETE' });
      if (!r.ok) throw new Error();
      await load();
      flash(true, `${name} removed`);
    } catch {
      flash(false, `Failed to remove ${name}`);
    }
  };

  const saveTools = async (name: string, scope: string, toolNames: string[]) => {
    try {
      const r = await fetch(`/api/mcp-servers/${encodeURIComponent(name)}/tools`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ toolNames, scope }),
      });
      if (!r.ok) throw new Error();
      await load();
      setEditing(null);
      flash(true, `Tool filter updated for ${name}`);
    } catch {
      flash(false, `Failed to update tools for ${name}`);
    }
  };

  // ---- render ----

  const builtins = servers.filter(s => s.builtin);
  const externals = servers.filter(s => !s.builtin);

  return (
    <div className="space-y-4">
      {/* Feedback toast */}
      {feedback && (
        <div className={`text-xs px-3 py-2 rounded-lg ${feedback.ok ? 'bg-emerald-50 text-emerald-700' : 'bg-red-50 text-red-600'}`}>
          {feedback.msg}
        </div>
      )}

      {loading ? (
        <div className="text-xs text-gray-400 py-8 text-center">Loading...</div>
      ) : (
        <>
          {/* ---- Built-in servers ---- */}
          <section>
            <h3 className="text-xs font-semibold text-gray-500 uppercase tracking-wider mb-2">Built-in</h3>
            <div className="space-y-2">
              {builtins.map(s => (
                <div key={s.name} className="border border-gray-100 rounded-lg p-3">
                  <div className="flex items-center justify-between mb-1">
                    <span className="text-sm font-medium text-gray-800">{s.name}</span>
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-blue-50 text-blue-600">{s.transport}</span>
                  </div>
                  {s.description && <p className="text-xs text-gray-500 mb-2">{s.description}</p>}
                  {s.tools && s.tools.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {s.tools.map(t => (
                        <span key={t.name} className="text-[10px] bg-gray-50 text-gray-500 px-1.5 py-0.5 rounded">
                          {t.name}
                        </span>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </section>

          {/* ---- External servers ---- */}
          <section>
            <div className="flex items-center justify-between mb-2">
              <h3 className="text-xs font-semibold text-gray-500 uppercase tracking-wider">External</h3>
              <button
                onClick={() => setShowAdd(true)}
                className="text-xs text-[#5BBFE8] hover:text-[#3a9fc8] font-medium"
              >
                + Add
              </button>
            </div>

            {externals.length === 0 ? (
              <p className="text-xs text-gray-400 py-4 text-center">
                No external MCP servers configured.
              </p>
            ) : (
              <div className="space-y-2">
                {externals.map(s => (
                  <div key={s.name} className="border border-gray-100 rounded-lg p-3">
                    {/* Header row */}
                    <div className="flex items-center justify-between mb-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-gray-800">{s.name}</span>
                        <span className={`text-[10px] px-1.5 py-0.5 rounded ${statusColor(s.status)}`}>
                          {s.status}
                        </span>
                      </div>
                      <div className="flex items-center gap-1">
                        <span className="text-[10px] px-1.5 py-0.5 rounded bg-gray-50 text-gray-400">{s.transport}</span>
                        <span className="text-[10px] text-gray-400">{s.scope}</span>
                      </div>
                    </div>

                    {s.description && <p className="text-xs text-gray-500 mb-2">{s.description}</p>}
                    {s.error && <p className="text-[10px] text-red-500 mb-2">{s.error}</p>}

                    {/* Tools */}
                    {s.tools && s.tools.length > 0 && (
                      <div className="flex flex-wrap gap-1 mb-2">
                        {s.tools.map(t => (
                          <span key={t.name} className="text-[10px] bg-gray-50 text-gray-500 px-1.5 py-0.5 rounded" title={t.description ?? undefined}>
                            {t.name}
                          </span>
                        ))}
                      </div>
                    )}

                    {/* Connection info */}
                    {s.command && <p className="text-[10px] text-gray-400 truncate">cmd: {s.command} {s.args.join(' ')}</p>}
                    {s.url && <p className="text-[10px] text-gray-400 truncate">url: {s.url}</p>}

                    {/* Actions */}
                    <div className="flex items-center gap-1 mt-2 pt-2 border-t border-gray-50">
                      {s.status === 'connected' ? (
                        <button
                          onClick={() => disconnect(s.name)}
                          className="text-[10px] px-2 py-1 rounded text-amber-600 hover:bg-amber-50 transition-colors"
                        >
                          Disconnect
                        </button>
                      ) : (
                        <button
                          onClick={() => connect(s.name)}
                          className="text-[10px] px-2 py-1 rounded text-emerald-600 hover:bg-emerald-50 transition-colors"
                        >
                          Connect
                        </button>
                      )}
                      <button
                        onClick={() => toggleEnabled(s.name, !s.enabled, s.scope)}
                        className={`text-[10px] px-2 py-1 rounded transition-colors ${s.enabled ? 'text-amber-600 hover:bg-amber-50' : 'text-emerald-600 hover:bg-emerald-50'}`}
                      >
                        {s.enabled ? 'Disable' : 'Enable'}
                      </button>
                      {s.tools && s.tools.length > 0 && (
                        <button
                          onClick={() => {
                            setEditing(editing === s.name ? null : s.name);
                            setToolFilter((s.use_tools || []).join('\n'));
                          }}
                          className="text-[10px] px-2 py-1 rounded text-gray-500 hover:bg-gray-100 transition-colors"
                        >
                          Filters
                        </button>
                      )}
                      <div className="flex-1" />
                      <button
                        onClick={() => removeServer(s.name, s.scope)}
                        className="text-[10px] px-2 py-1 rounded text-red-400 hover:bg-red-50 transition-colors"
                      >
                        Delete
                      </button>
                    </div>

                    {/* Tool filter editor (inline) */}
                    {editing === s.name && (
                      <div className="mt-2 pt-2 border-t border-gray-50">
                        <p className="text-[10px] text-gray-400 mb-1">
                          One tool name per line. Empty = allow all.
                        </p>
                        <textarea
                          className="w-full text-[10px] font-mono border border-gray-200 rounded p-2 mb-1"
                          rows={4}
                          value={toolFilter}
                          onChange={e => setToolFilter(e.target.value)}
                        />
                        <div className="flex gap-1">
                          <button
                            onClick={() => {
                              const names = toolFilter.split('\n').map(l => l.trim()).filter(Boolean);
                              saveTools(s.name, s.scope, names.length > 0 ? names : []);
                            }}
                            className="text-[10px] px-2 py-1 rounded bg-[#5BBFE8] text-white hover:bg-[#3a9fc8]"
                          >
                            Save filters
                          </button>
                          <button
                            onClick={() => {
                              saveTools(s.name, s.scope, []);
                              setToolFilter('');
                            }}
                            className="text-[10px] px-2 py-1 rounded text-gray-400 hover:bg-gray-100"
                          >
                            Clear (allow all)
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </section>
        </>
      )}

      {/* ---- Add / Edit overlay ---- */}
      {showAdd && (
        <AddMcpPanel
          onClose={() => setShowAdd(false)}
          onSaved={() => { setShowAdd(false); load(); }}
        />
      )}
    </div>
  );
}

// ===========================================================================
// Add / Edit panel
// ===========================================================================

interface AddPanelProps {
  onClose: () => void;
  onSaved: () => void;
}

function AddMcpPanel({ onClose, onSaved }: AddPanelProps) {
  const [name, setName]             = useState('');
  const [transport, setTransport]   = useState<Transport>('stdio');
  const [description, setDescription] = useState('');
  const [scope, setScope]           = useState<'user' | 'project'>('user');
  const [enabled, setEnabled]       = useState(true);

  // stdio fields
  const [command, setCommand] = useState('');
  const [args, setArgs]       = useState('');
  const [envStr, setEnvStr]   = useState('');

  // sse/http fields
  const [url, setUrl]         = useState('');
  const [headersStr, setHeadersStr] = useState('');

  const [saving, setSaving]  = useState(false);
  const [error, setError]    = useState<string | null>(null);

  const save = async () => {
    if (!name.trim()) { setError('Name is required'); return; }

    if (transport === 'stdio' && !command.trim()) {
      setError('Command is required for stdio transport');
      return;
    }
    if ((transport === 'sse' || transport === 'http') && !url.trim()) {
      setError('URL is required for sse/http transports');
      return;
    }

    setSaving(true);
    setError(null);

    // Parse env string: KEY=VALUE per line
    const env: Record<string, string> = {};
    envStr.split('\n').forEach(line => {
      const idx = line.indexOf('=');
      if (idx > 0) env[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
    });

    // Parse headers string
    const headers: Record<string, string> = {};
    headersStr.split('\n').forEach(line => {
      const idx = line.indexOf(':');
      if (idx > 0) headers[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
    });

    try {
      const r = await fetch('/api/mcp-servers', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          name: name.trim(),
          transport,
          description: description.trim() || null,
          enabled,
          scope,
          command: transport === 'stdio' ? command.trim() : null,
          args: transport === 'stdio' ? args.split(' ').filter(Boolean) : [],
          env,
          url: transport !== 'stdio' ? url.trim() : null,
          headers,
        }),
      });
      if (!r.ok) {
        const data = await r.json().catch(() => ({}));
        throw new Error((data as { error?: string }).error || `HTTP ${r.status}`);
      }
      onSaved();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-[60] flex">
      <div className="absolute inset-0 bg-black/20" onClick={onClose} />
      <aside
        className="relative h-full bg-white border-l border-gray-100 flex flex-col shadow-lg ml-auto"
        style={{ width: 400 }}
      >
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-100 flex-shrink-0">
          <span className="font-semibold text-gray-800 text-sm">Add MCP Server</span>
          <button
            onClick={onClose}
            className="w-6 h-6 flex items-center justify-center rounded-lg hover:bg-gray-100 text-gray-400"
          >
            <svg xmlns="http://www.w3.org/2000/svg" className="w-4 h-4" fill="none" viewBox="0 0 24 24" strokeWidth={2} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M6 18 18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          {error && <div className="text-xs text-red-600 bg-red-50 px-3 py-2 rounded-lg">{error}</div>}

          {/* Name */}
          <label className="block">
            <span className="text-[10px] font-medium text-gray-500">Name</span>
            <input
              type="text"
              className="mt-0.5 w-full text-xs border border-gray-200 rounded-lg px-2.5 py-1.5 outline-none focus:border-[#5BBFE8]"
              placeholder="my-mcp-server"
              value={name}
              onChange={e => setName(e.target.value)}
            />
          </label>

          {/* Transport */}
          <label className="block">
            <span className="text-[10px] font-medium text-gray-500">Transport</span>
            <div className="flex gap-1 mt-0.5">
              {(['stdio', 'sse', 'http'] as Transport[]).map(t => (
                <button
                  key={t}
                  onClick={() => setTransport(t)}
                  className={`flex-1 text-xs py-1.5 rounded-lg border transition-colors ${
                    transport === t ? 'border-[#5BBFE8] bg-blue-50 text-[#5BBFE8]' : 'border-gray-200 text-gray-500 hover:border-gray-300'
                  }`}
                >
                  {t}
                </button>
              ))}
            </div>
          </label>

          {/* Description */}
          <label className="block">
            <span className="text-[10px] font-medium text-gray-500">Description</span>
            <input
              type="text"
              className="mt-0.5 w-full text-xs border border-gray-200 rounded-lg px-2.5 py-1.5 outline-none focus:border-[#5BBFE8]"
              placeholder="Optional description"
              value={description}
              onChange={e => setDescription(e.target.value)}
            />
          </label>

          {/* Scope */}
          <label className="block">
            <span className="text-[10px] font-medium text-gray-500">Scope</span>
            <div className="flex gap-1 mt-0.5">
              <button
                onClick={() => setScope('user')}
                className={`flex-1 text-xs py-1.5 rounded-lg border transition-colors ${
                  scope === 'user' ? 'border-[#5BBFE8] bg-blue-50 text-[#5BBFE8]' : 'border-gray-200 text-gray-500 hover:border-gray-300'
                }`}
              >
                User (~/.senclaw)
              </button>
              <button
                onClick={() => setScope('project')}
                className={`flex-1 text-xs py-1.5 rounded-lg border transition-colors ${
                  scope === 'project' ? 'border-[#5BBFE8] bg-blue-50 text-[#5BBFE8]' : 'border-gray-200 text-gray-500 hover:border-gray-300'
                }`}
              >
                Project (.senclaw)
              </button>
            </div>
          </label>

          {/* Enabled toggle */}
          <label className="flex items-center gap-2 cursor-pointer">
            <input
              type="checkbox"
              checked={enabled}
              onChange={e => setEnabled(e.target.checked)}
              className="w-3.5 h-3.5 rounded accent-[#5BBFE8]"
            />
            <span className="text-xs text-gray-700">Enabled</span>
          </label>

          {/* Stdio fields */}
          {transport === 'stdio' && (
            <>
              <label className="block">
                <span className="text-[10px] font-medium text-gray-500">Command *</span>
                <input
                  type="text"
                  className="mt-0.5 w-full text-xs border border-gray-200 rounded-lg px-2.5 py-1.5 outline-none focus:border-[#5BBFE8] font-mono"
                  placeholder="npx -y @modelcontextprotocol/server-filesystem"
                  value={command}
                  onChange={e => setCommand(e.target.value)}
                />
              </label>
              <label className="block">
                <span className="text-[10px] font-medium text-gray-500">Arguments (space-separated)</span>
                <input
                  type="text"
                  className="mt-0.5 w-full text-xs border border-gray-200 rounded-lg px-2.5 py-1.5 outline-none focus:border-[#5BBFE8] font-mono"
                  placeholder="/path/to/allowed"
                  value={args}
                  onChange={e => setArgs(e.target.value)}
                />
              </label>
              <label className="block">
                <span className="text-[10px] font-medium text-gray-500">Environment variables (KEY=VALUE per line)</span>
                <textarea
                  className="mt-0.5 w-full text-[10px] font-mono border border-gray-200 rounded-lg p-2 outline-none focus:border-[#5BBFE8]"
                  rows={3}
                  placeholder="API_KEY=xxx"
                  value={envStr}
                  onChange={e => setEnvStr(e.target.value)}
                />
              </label>
            </>
          )}

          {/* SSE/HTTP fields */}
          {(transport === 'sse' || transport === 'http') && (
            <>
              <label className="block">
                <span className="text-[10px] font-medium text-gray-500">URL *</span>
                <input
                  type="text"
                  className="mt-0.5 w-full text-xs border border-gray-200 rounded-lg px-2.5 py-1.5 outline-none focus:border-[#5BBFE8] font-mono"
                  placeholder="http://localhost:8080/sse"
                  value={url}
                  onChange={e => setUrl(e.target.value)}
                />
              </label>
              <label className="block">
                <span className="text-[10px] font-medium text-gray-500">Headers (Name: Value per line)</span>
                <textarea
                  className="mt-0.5 w-full text-[10px] font-mono border border-gray-200 rounded-lg p-2 outline-none focus:border-[#5BBFE8]"
                  rows={3}
                  placeholder="Authorization: Bearer xxx"
                  value={headersStr}
                  onChange={e => setHeadersStr(e.target.value)}
                />
              </label>
            </>
          )}
        </div>

        {/* Footer */}
        <div className="px-4 py-3 border-t border-gray-100 flex-shrink-0 flex gap-2">
          <button
            onClick={onClose}
            className="flex-1 text-xs py-1.5 rounded-lg border border-gray-200 text-gray-500 hover:bg-gray-50"
          >
            Cancel
          </button>
          <button
            onClick={save}
            disabled={saving}
            className="flex-1 text-xs py-1.5 rounded-lg bg-[#5BBFE8] text-white hover:bg-[#3a9fc8] disabled:opacity-50"
          >
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>
      </aside>
    </div>
  );
}
