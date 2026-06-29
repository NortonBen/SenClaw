import React, { useState, useEffect } from 'react';
import { Flex, Typography, theme, Card, Tag, Button, Input, Segmented, message, Empty, Spin } from 'antd';
import {
  CodeOutlined,
  ThunderboltOutlined,
  SafetyCertificateOutlined,
  CloudUploadOutlined,
  BugOutlined,
  PlayCircleOutlined,
  ClockCircleOutlined,
  CheckCircleOutlined,
  ApiOutlined,
  ExperimentOutlined,
  SaveOutlined,
  DeleteOutlined,
  ReloadOutlined,
  FolderOpenOutlined
} from '@ant-design/icons';

interface Artifact {
  id: string;
  name: string;
  language: 'javascript' | 'typescript' | 'bash';
  code: string;
  description: string;
  tags: string[];
  created_at: string;
  updated_at: string;
}

const LANG_LABEL: Record<string, string> = {
  javascript: 'JS',
  typescript: 'TS',
  bash: 'Bash',
};

const { Title, Text, Paragraph } = Typography;

// ─── Language Badge ───────────────────────────────────────────────────────────

function LangBadge({ name, color, live, host }: { name: string; color: string; live?: boolean; host?: boolean }) {
  const { token } = theme.useToken();
  // `host` = live but runs on the host (Bash) — flagged amber, not green.
  const accent = host ? token.colorWarning : token.colorSuccess;
  const bg = host ? token.colorWarningBg : token.colorSuccessBg;
  const borderOn = host ? token.colorWarningBorder : token.colorSuccessBorder;
  const on = live || host;
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 6,
        fontSize: '11px',
        fontWeight: 500,
        color: on ? token.colorText : token.colorTextTertiary,
        backgroundColor: on ? bg : token.colorFillAlter,
        border: `1px solid ${on ? borderOn : token.colorBorderSecondary}`,
        padding: '3px 10px',
        borderRadius: '6px',
        opacity: on ? 1 : 0.7,
      }}
    >
      <span style={{
        width: 8,
        height: 8,
        borderRadius: '50%',
        backgroundColor: color,
        flexShrink: 0,
      }} />
      {name}
      {on && (
        <span style={{
          fontSize: 9,
          fontWeight: 700,
          letterSpacing: '0.5px',
          color: accent,
        }}>
          {host ? 'HOST' : 'LIVE'}
        </span>
      )}
    </span>
  );
}

// ─── Feature Card ─────────────────────────────────────────────────────────────

function FeatureCard({
  icon,
  title,
  description,
  status
}: {
  icon: React.ReactNode;
  title: string;
  description: string;
  status: 'planned' | 'in-progress' | 'completed';
}) {
  const { token } = theme.useToken();

  const statusConfig = {
    planned: { color: token.colorTextQuaternary, bg: token.colorFillAlter, label: 'Planned', icon: <ClockCircleOutlined /> },
    'in-progress': { color: token.colorPrimary, bg: token.colorPrimaryBg, label: 'In Progress', icon: <PlayCircleOutlined /> },
    completed: { color: token.colorSuccess, bg: token.colorSuccessBg, label: 'Live', icon: <CheckCircleOutlined /> },
  };

  const cfg = statusConfig[status];

  return (
    <Card
      size="small"
      style={{
        backgroundColor: token.colorBgContainer,
        borderColor: status === 'completed' ? token.colorSuccessBorder : token.colorBorderSecondary,
        borderRadius: 12,
        transition: 'all 0.2s',
      }}
      hoverable
      styles={{ body: { padding: '16px' } }}
    >
      <Flex vertical gap={12}>
        <Flex align="center" justify="space-between">
          <Flex align="center" gap={10}>
            <div style={{
              backgroundColor: status === 'completed' ? token.colorSuccessBg : token.colorPrimaryBg,
              width: 36,
              height: 36,
              borderRadius: 10,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
            }}>
              <span style={{ color: status === 'completed' ? token.colorSuccess : token.colorPrimary, fontSize: 18 }}>{icon}</span>
            </div>
            <Text strong style={{ fontSize: 14 }}>{title}</Text>
          </Flex>
          <Tag
            icon={cfg.icon}
            style={{
              margin: 0,
              fontSize: '10px',
              borderColor: cfg.color,
              color: cfg.color,
              backgroundColor: cfg.bg,
              borderRadius: '6px',
            }}
          >
            {cfg.label}
          </Tag>
        </Flex>
        <Paragraph
          type="secondary"
          style={{ margin: 0, fontSize: 12, lineHeight: 1.6 }}
        >
          {description}
        </Paragraph>
      </Flex>
    </Card>
  );
}

// ─── Interactive REPL ─────────────────────────────────────────────────────────

const MONO = 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace';

interface RunOutcome {
  ok: boolean;
  result?: string;
  result_type?: string;
  logs?: string[];
  error?: string;
  timed_out?: boolean;
  duration_ms?: number;
}

function ReplOutput({ out }: { out: RunOutcome }) {
  const { token } = theme.useToken();
  const block: React.CSSProperties = {
    fontFamily: MONO,
    fontSize: 12,
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
    margin: 0,
  };
  return (
    <Flex vertical gap={8} style={{
      background: token.colorFillQuaternary,
      border: `1px solid ${token.colorBorderSecondary}`,
      borderRadius: 8,
      padding: '10px 12px',
    }}>
      {out.logs && out.logs.length > 0 && (
        <Flex vertical gap={2}>
          <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.5px' }}>Console</Text>
          {out.logs.map((l, i) => (
            <pre key={i} style={{ ...block, color: token.colorTextSecondary }}>{l}</pre>
          ))}
        </Flex>
      )}
      {out.ok ? (
        <Flex vertical gap={2}>
          <Flex align="center" gap={6}>
            <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.5px' }}>Result</Text>
            {out.result_type && (
              <Tag style={{ margin: 0, fontSize: 9, lineHeight: '14px' }}>{out.result_type}</Tag>
            )}
          </Flex>
          <pre style={{ ...block, color: token.colorSuccess }}>{out.result ?? 'undefined'}</pre>
        </Flex>
      ) : (
        <Flex vertical gap={2}>
          <Text type="secondary" style={{ fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
            {out.timed_out ? 'Timed out' : 'Error'}
          </Text>
          <pre style={{ ...block, color: token.colorError }}>{out.error ?? 'unknown error'}</pre>
        </Flex>
      )}
      {typeof out.duration_ms === 'number' && (
        <Text type="secondary" style={{ fontSize: 10 }}>{out.duration_ms} ms</Text>
      )}
    </Flex>
  );
}

type Lang = 'javascript' | 'typescript' | 'bash';

const SAMPLES: Record<Lang, string> = {
  javascript: `// Sandboxed JavaScript — no fs, network, or process access.\nconst xs = [1, 2, 3, 4];\nconsole.log('sum', xs.reduce((a, b) => a + b, 0));\nxs.map(x => x * x)`,
  typescript: `// Sandboxed TypeScript — transpiled to JS, then run in the sandbox.\ninterface Point { x: number; y: number }\nconst pts: Point[] = [{ x: 1, y: 2 }, { x: 3, y: 4 }];\nconst dist = (p: Point): number => Math.hypot(p.x, p.y);\nconsole.log('distances', pts.map(dist));\npts.length`,
  bash: `# Bash in the brush sandbox (pure-Rust): no env, empty PATH (external\n# programs like ls/curl are blocked), temp dir, kill-enforced timeout.\nname="brush"\nfor i in 1 2 3; do echo "hello $name #$i"; done\necho "sum = $((2 + 3 * 4))"`,
};
const SAMPLE_SET = new Set(Object.values(SAMPLES));

function JsRepl({
  code,
  setCode,
  lang,
  setLang,
  onSaved,
}: {
  code: string;
  setCode: React.Dispatch<React.SetStateAction<string>>;
  lang: Lang;
  setLang: React.Dispatch<React.SetStateAction<Lang>>;
  onSaved: () => void;
}) {
  const { token } = theme.useToken();
  const [running, setRunning] = useState(false);
  const [out, setOut] = useState<RunOutcome | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const switchLang = (next: Lang) => {
    // Swap the sample when the editor still holds an untouched sample, so
    // toggling shows a relevant example without clobbering real edits.
    setCode((cur) => (SAMPLE_SET.has(cur) ? SAMPLES[next] : cur));
    setLang(next);
  };

  const save = async () => {
    const name = window.prompt('Save snippet as artifact — name:');
    if (!name || !name.trim()) return;
    setSaving(true);
    try {
      const r = await fetch('/api/code/artifacts', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name: name.trim(), language: lang, code }),
      });
      if (!r.ok) throw new Error((await r.text()) || `HTTP ${r.status}`);
      message.success(`Saved "${name.trim()}"`);
      onSaved();
    } catch (e) {
      message.error(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const run = async () => {
    setRunning(true);
    setErr(null);
    try {
      const r = await fetch('/api/code/run', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ code, language: lang }),
      });
      if (!r.ok) {
        const t = await r.text();
        throw new Error(t || `HTTP ${r.status}`);
      }
      setOut((await r.json()) as RunOutcome);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
      setOut(null);
    } finally {
      setRunning(false);
    }
  };

  return (
    <Card
      size="small"
      style={{
        backgroundColor: token.colorBgContainer,
        borderColor: token.colorSuccessBorder,
        borderRadius: 12,
      }}
      styles={{ body: { padding: '16px' } }}
    >
      <Flex vertical gap={10}>
        <Flex align="center" justify="space-between">
          <Flex align="center" gap={8}>
            <PlayCircleOutlined style={{ color: token.colorSuccess }} />
            <Text strong style={{ fontSize: 14 }}>Interactive REPL</Text>
          </Flex>
          <Segmented
            size="small"
            value={lang}
            onChange={(v) => switchLang(v as Lang)}
            options={[
              { label: 'JavaScript', value: 'javascript' },
              { label: 'TypeScript', value: 'typescript' },
              { label: 'Bash', value: 'bash' },
            ]}
          />
        </Flex>
        {lang === 'bash' && (
          <Flex
            align="center"
            gap={8}
            style={{
              background: token.colorWarningBg,
              border: `1px solid ${token.colorWarningBorder}`,
              borderRadius: 8,
              padding: '6px 10px',
            }}
          >
            <SafetyCertificateOutlined style={{ color: token.colorWarning }} />
            <Text style={{ fontSize: 11, color: token.colorWarningText }}>
              Bash runs in the <strong>brush sandbox</strong> (pure-Rust): no environment,
              empty PATH (external programs like <code>ls</code>/<code>curl</code> are blocked),
              a temp working dir, and a kill-enforced timeout. Note: in-process isolation, not an OS jail.
            </Text>
          </Flex>
        )}
        <Input.TextArea
          value={code}
          onChange={(e) => setCode(e.target.value)}
          onKeyDown={(e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
              e.preventDefault();
              if (!running) run();
            }
          }}
          autoSize={{ minRows: 5, maxRows: 16 }}
          spellCheck={false}
          style={{ fontFamily: MONO, fontSize: 12 }}
        />
        <Flex align="center" justify="space-between">
          <Text type="secondary" style={{ fontSize: 11 }}>
            ⌘/Ctrl+Enter to run · 5s / 128&nbsp;MiB limits
          </Text>
          <Flex align="center" gap={8}>
            <Button icon={<SaveOutlined />} loading={saving} onClick={save}>
              Save as artifact
            </Button>
            <Button
              type="primary"
              icon={<PlayCircleOutlined />}
              loading={running}
              onClick={run}
            >
              Run
            </Button>
          </Flex>
        </Flex>
        {err && (
          <Text type="danger" style={{ fontSize: 12 }}>{err}</Text>
        )}
        {out && <ReplOutput out={out} />}
      </Flex>
    </Card>
  );
}

// ─── Artifacts (published snippets) ───────────────────────────────────────────

function ArtifactsList({ refresh, onLoad }: { refresh: number; onLoad: (a: Artifact) => void }) {
  const { token } = theme.useToken();
  const [items, setItems] = useState<Artifact[]>([]);
  const [loading, setLoading] = useState(true);
  const [runOut, setRunOut] = useState<Record<string, RunOutcome>>({});
  const [busy, setBusy] = useState<string | null>(null);

  const fetchItems = async () => {
    setLoading(true);
    try {
      const r = await fetch('/api/code/artifacts');
      const d = await r.json();
      setItems((d.artifacts || []) as Artifact[]);
    } catch {
      /* ignore */
    } finally {
      setLoading(false);
    }
  };
  useEffect(() => { fetchItems(); }, [refresh]); // eslint-disable-line react-hooks/exhaustive-deps

  const runItem = async (a: Artifact) => {
    setBusy(a.id);
    try {
      const r = await fetch(`/api/code/artifacts/${a.id}/run`, { method: 'POST' });
      const d = (await r.json()) as RunOutcome;
      setRunOut((prev) => ({ ...prev, [a.id]: d }));
    } catch {
      message.error('Run failed');
    } finally {
      setBusy(null);
    }
  };

  const del = async (a: Artifact) => {
    if (!window.confirm(`Delete artifact "${a.name}"?`)) return;
    try {
      await fetch(`/api/code/artifacts/${a.id}`, { method: 'DELETE' });
      message.success('Deleted');
      fetchItems();
    } catch {
      message.error('Delete failed');
    }
  };

  if (loading) {
    return <Flex justify="center" style={{ padding: 16 }}><Spin size="small" /></Flex>;
  }
  if (items.length === 0) {
    return (
      <Empty
        image={Empty.PRESENTED_IMAGE_SIMPLE}
        description={<Text type="secondary" style={{ fontSize: 12 }}>No artifacts yet — write code above and “Save as artifact”.</Text>}
        style={{ margin: '8px 0' }}
      />
    );
  }

  return (
    <Flex vertical gap={8}>
      {items.map((a) => (
        <Card key={a.id} size="small" styles={{ body: { padding: '10px 12px' } }}
          style={{ backgroundColor: token.colorBgContainer, borderColor: token.colorBorderSecondary, borderRadius: 10 }}>
          <Flex vertical gap={6}>
            <Flex align="center" justify="space-between" gap={8}>
              <Flex align="center" gap={8} style={{ minWidth: 0 }}>
                <Tag style={{ margin: 0, fontSize: 10 }}>{LANG_LABEL[a.language] ?? a.language}</Tag>
                <Text strong style={{ fontSize: 13 }} ellipsis={{ tooltip: a.name }}>{a.name}</Text>
              </Flex>
              <Flex align="center" gap={4}>
                <Button size="small" type="text" icon={<FolderOpenOutlined />} onClick={() => onLoad(a)} title="Load into editor" />
                <Button size="small" type="text" icon={<PlayCircleOutlined />} loading={busy === a.id} onClick={() => runItem(a)} title="Run" />
                <Button size="small" type="text" danger icon={<DeleteOutlined />} onClick={() => del(a)} title="Delete" />
              </Flex>
            </Flex>
            {a.description && (
              <Text type="secondary" style={{ fontSize: 12 }} ellipsis={{ tooltip: a.description }}>{a.description}</Text>
            )}
            {runOut[a.id] && <ReplOutput out={runOut[a.id]} />}
          </Flex>
        </Card>
      ))}
    </Flex>
  );
}

// ─── Main Panel ───────────────────────────────────────────────────────────────

const CodePanel: React.FC = () => {
  const { token } = theme.useToken();

  // Shared editor state so artifacts can be loaded into the REPL and saved out.
  const [code, setCode] = useState(SAMPLES.javascript);
  const [lang, setLang] = useState<Lang>('javascript');
  const [artifactRefresh, setArtifactRefresh] = useState(0);

  // JavaScript is live today via the `senclaw-js` sandbox (QuickJS).
  // The rest are on the roadmap.
  const languages = [
    { name: 'JavaScript', color: '#F7DF1E', live: true },
    { name: 'TypeScript', color: '#3178C6', live: true },
    { name: 'Bash', color: '#4EAA25', live: true },
    { name: 'Python', color: '#3776AB', live: false },
    { name: 'Go', color: '#00ADD8', live: false },
    { name: 'Rust', color: '#DEA584', live: false },
  ];

  // The three tools exposed by the `senclaw-js` MCP server.
  const tools = [
    { name: 'js_eval', desc: 'Run a JavaScript snippet; returns the value, captured console output, and any error.' },
    { name: 'js_eval_file', desc: 'Read a .js / .mjs file from disk and run it in the same sandbox.' },
    { name: 'js_capabilities', desc: 'Describe the sandbox policy: limits and available vs. blocked globals.' },
  ];

  const features = [
    {
      icon: <CodeOutlined />,
      title: 'JavaScript Sandbox (QuickJS)',
      description: 'Agents execute JavaScript through the senclaw-js MCP server. Standard ECMAScript intrinsics (Object, Array, JSON, Math, Date, RegExp, Map/Set, BigInt) plus a captured console — no filesystem, network, or process access.',
      status: 'completed' as const,
    },
    {
      icon: <SafetyCertificateOutlined />,
      title: 'Sandboxed Execution (JS / TS)',
      description: 'JavaScript and TypeScript runs are fully isolated and bounded by a wall-clock timeout (default 5s, max 60s) and a memory cap (default 128 MiB, max 1 GiB). Infinite loops and over-allocation are killed — zero risk to the host.',
      status: 'completed' as const,
    },
    {
      icon: <CodeOutlined />,
      title: 'Bash (brush sandbox)',
      description: 'Bash runs in brush — a pure-Rust shell — with no env, an empty PATH (external programs blocked), a temp working dir, and a kill-enforced timeout (runs in a killable child process). In-process isolation, not an OS jail.',
      status: 'completed' as const,
    },
    {
      icon: <CodeOutlined />,
      title: 'TypeScript Support',
      description: 'TypeScript snippets are transpiled to JavaScript (types stripped, no type-checking) and run in the same sandbox — interfaces, generics, enums, and casts all work.',
      status: 'completed' as const,
    },
    {
      icon: <ThunderboltOutlined />,
      title: 'More Language Runtimes',
      description: 'Python, Go, Rust, and Bash runtimes are on the roadmap, reusing the same isolation + resource-limit model.',
      status: 'planned' as const,
    },
    {
      icon: <BugOutlined />,
      title: 'Integrated Debugging',
      description: 'Set breakpoints, inspect variables, and step through execution. Supports stack-trace visualization and memory profiling.',
      status: 'planned' as const,
    },
    {
      icon: <CloudUploadOutlined />,
      title: 'Artifact Publishing',
      description: 'Save snippets (JS/TS/Bash) as reusable artifacts — name them, browse them, load them back into the editor, re-run, or delete. Stored in SQLite and shared across agents.',
      status: 'completed' as const,
    },
  ];

  return (
    <Flex vertical style={{ height: '100%', background: token.colorBgLayout }}>
      {/* Hero Section */}
      <div style={{
        padding: '12px 32px 20px',
        background: `linear-gradient(135deg, ${token.colorPrimaryBg} 0%, ${token.colorBgContainer} 100%)`,
        borderBottom: `1px solid ${token.colorBorderSecondary}`,
      }}>
        <Flex vertical gap={16} style={{ maxWidth: 720 }}>
          <Flex align="center" gap={12}>
            <div style={{
              width: 48,
              height: 48,
              borderRadius: 14,
              background: `linear-gradient(135deg, ${token.colorPrimary}, ${token.colorPrimaryActive})`,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              boxShadow: `0 4px 12px ${token.colorPrimary}40`,
            }}>
              <ExperimentOutlined style={{ color: '#fff', fontSize: 24 }} />
            </div>
            <div>
              <Flex align="center" gap={8}>
                <Title level={3} style={{ margin: 0 }}>Code Executor</Title>
                <Tag
                  icon={<CheckCircleOutlined />}
                  color="success"
                  style={{ margin: 0, fontSize: 10, borderRadius: 6 }}
                >
                  JS · TS · Bash
                </Tag>
              </Flex>
              <Text type="secondary" style={{ fontSize: 13 }}>
                Sandboxed code execution via the <code>senclaw-js</code> MCP server
              </Text>
            </div>
          </Flex>

          {/* Language Support */}
          <Card
            size="small"
            style={{
              backgroundColor: token.colorBgContainer,
              borderColor: token.colorBorderSecondary,
              borderRadius: 10,
            }}
            styles={{ body: { padding: '12px 16px' } }}
          >
            <Flex vertical gap={8}>
              <Text type="secondary" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
                Languages
              </Text>
              <Flex wrap="wrap" gap={6}>
                {languages.map(l => (
                  <LangBadge key={l.name} name={l.name} color={l.color} live={l.live} />
                ))}
              </Flex>
              <Text type="secondary" style={{ fontSize: 11 }}>
                JavaScript, TypeScript &amp; Bash run sandboxed; the rest are planned.
              </Text>
            </Flex>
          </Card>

          {/* Tools exposed by senclaw-js */}
          <Card
            size="small"
            style={{
              backgroundColor: token.colorBgContainer,
              borderColor: token.colorBorderSecondary,
              borderRadius: 10,
            }}
            styles={{ body: { padding: '12px 16px' } }}
          >
            <Flex vertical gap={10}>
              <Flex align="center" gap={6}>
                <ApiOutlined style={{ color: token.colorPrimary, fontSize: 13 }} />
                <Text type="secondary" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
                  MCP tools · senclaw-js
                </Text>
              </Flex>
              {tools.map(t => (
                <Flex key={t.name} align="flex-start" gap={8}>
                  <ThunderboltOutlined style={{ color: token.colorTextTertiary, fontSize: 11, marginTop: 3 }} />
                  <Text style={{ fontSize: 12 }}>
                    <code style={{ color: token.colorPrimary }}>{t.name}</code>
                    <Text type="secondary" style={{ fontSize: 12 }}> — {t.desc}</Text>
                  </Text>
                </Flex>
              ))}
            </Flex>
          </Card>
        </Flex>
      </div>

      {/* Features Grid */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '24px 32px' }}>
        <Flex vertical gap={24} style={{ maxWidth: 720 }}>
          {/* Live REPL */}
          <div>
            <Text strong style={{
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: '1px',
              color: token.colorTextTertiary,
              display: 'block',
              marginBottom: 12,
            }}>
              Try it
            </Text>
            <JsRepl
              code={code}
              setCode={setCode}
              lang={lang}
              setLang={setLang}
              onSaved={() => setArtifactRefresh((n) => n + 1)}
            />
          </div>

          {/* Artifacts */}
          <div>
            <Flex align="center" justify="space-between" style={{ marginBottom: 12 }}>
              <Text strong style={{
                fontSize: 11,
                textTransform: 'uppercase',
                letterSpacing: '1px',
                color: token.colorTextTertiary,
              }}>
                Artifacts
              </Text>
              <Button
                size="small"
                type="text"
                icon={<ReloadOutlined />}
                onClick={() => setArtifactRefresh((n) => n + 1)}
              >
                Refresh
              </Button>
            </Flex>
            <ArtifactsList
              refresh={artifactRefresh}
              onLoad={(a) => {
                setLang(a.language);
                setCode(a.code);
                message.success(`Loaded "${a.name}" into the editor`);
              }}
            />
          </div>

          {/* Section title */}
          <div>
            <Text strong style={{
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: '1px',
              color: token.colorTextTertiary,
            }}>
              Core Capabilities
            </Text>
          </div>

          {/* Feature Cards */}
          <div style={{
            display: 'grid',
            gap: '12px',
            gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))',
          }}>
            {features.map((f, i) => (
              <FeatureCard key={i} {...f} />
            ))}
          </div>

          {/* Architecture Preview */}
          <div style={{ marginTop: 8 }}>
            <Text strong style={{
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: '1px',
              color: token.colorTextTertiary,
              display: 'block',
              marginBottom: 16,
            }}>
              How a run works
            </Text>
            <Card
              size="small"
              style={{
                backgroundColor: token.colorBgContainer,
                borderColor: token.colorBorderSecondary,
                borderRadius: 12,
              }}
              styles={{ body: { padding: '20px 24px' } }}
            >
              <Flex vertical gap={16}>
                {[
                  { label: 'Agent calls js_eval', desc: 'Agent submits a JavaScript snippet (optionally with timeout / memory overrides)', icon: <ThunderboltOutlined />, color: token.colorPrimary },
                  { label: 'Fresh QuickJS runtime', desc: 'A new engine is built per run with the memory cap and an interrupt-based timeout armed', icon: <SafetyCertificateOutlined />, color: token.colorWarning },
                  { label: 'Execution', desc: 'Code runs with no host bindings; console output is captured, runaway loops are killed', icon: <PlayCircleOutlined />, color: token.colorSuccess },
                  { label: 'Result', desc: 'Final value, console logs, error/timeout flag, and duration are returned to the agent', icon: <CodeOutlined />, color: token.colorInfo },
                ].map((step, i) => (
                  <Flex key={i} align="flex-start" gap={12}>
                    <div style={{
                      width: 32,
                      height: 32,
                      borderRadius: 8,
                      backgroundColor: `${step.color}15`,
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'center',
                      flexShrink: 0,
                      marginTop: 2,
                    }}>
                      <span style={{ color: step.color, fontSize: 14 }}>{step.icon}</span>
                    </div>
                    <Flex vertical gap={2}>
                      <Flex align="center" gap={8}>
                        <span style={{
                          fontSize: '10px',
                          fontWeight: 700,
                          color: token.colorTextQuaternary,
                          width: 16,
                        }}>
                          {i + 1}.
                        </span>
                        <Text strong style={{ fontSize: 13 }}>{step.label}</Text>
                      </Flex>
                      <Text type="secondary" style={{ fontSize: 12, paddingLeft: 24 }}>{step.desc}</Text>
                    </Flex>
                  </Flex>
                ))}
              </Flex>
            </Card>
          </div>
        </Flex>
      </div>
    </Flex>
  );
};

export default CodePanel;
