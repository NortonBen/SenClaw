/**
 * SkillsPanel — manage Skills
 *
 * Browse: card grid by source, detail with SKILL.md view/edit; search hits local + remote ClaWHub
 * Manage: installed skills by source with per-skill enable/disable
 */

import { useState, useEffect, useRef, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import { theme, Button, Input, Switch, Tag, Typography, Spin, Space, Tooltip, Tabs, message, Card, Flex } from 'antd';
import { SearchOutlined, ReloadOutlined, ArrowLeftOutlined, EditOutlined, PlusOutlined, ThunderboltOutlined, DownloadOutlined } from '@ant-design/icons';
import 'highlight.js/styles/github.css';

const { Text, Title, Paragraph } = Typography;

// ─── Types ─────────────────────────────────────────────────────────────────────

interface LocalSkill {
  name: string;
  description: string;
  version?: string;
  source: string;
  dir: string;
  disabled: boolean;
}

interface RemoteResult {
  slug: string;
  displayName?: string;
  summary?: string | null;
  version?: string | null;
  score: number;
  installed: boolean;
}

type Tab = 'browse' | 'manage';

// ─── Constants ───────────────────────────────────────────────────────────────

const SOURCE_LABEL: Record<string, string> = {
  bundled: 'Bundled',
  'global-compat': 'Global',
  'global-sema': 'Global',
  'clawhub-managed': 'ClaWHub',
  workspace: 'Workspace',
};

const SOURCE_ORDER = ['bundled', 'clawhub-managed', 'global-compat', 'global-sema', 'workspace'];

const SOURCE_COLOR: Record<string, string> = {
  bundled: 'purple',
  'clawhub-managed': 'blue',
  'global-compat': 'default',
  'global-sema': 'default',
  workspace: 'orange',
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

async function apiFetch<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, init);
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(text || `HTTP ${res.status}`);
  }
  return res.json() as Promise<T>;
}

function groupBySource(skills: LocalSkill[]) {
  const knownSources = new Set(SOURCE_ORDER);
  const groups = SOURCE_ORDER
    .map(src => ({ source: src, skills: skills.filter(s => s.source === src) }))
    .filter(g => g.skills.length > 0);
  const others = skills.filter(s => !knownSources.has(s.source));
  if (others.length > 0) groups.push({ source: 'other', skills: others });
  return groups;
}

// ─── Small components ────────────────────────────────────────────────────────

function SourceBadge({ source, className = '' }: { source: string; className?: string }) {
  return (
    <Tag color={SOURCE_COLOR[source] ?? 'default'} className={className}>
      {SOURCE_LABEL[source] ?? source}
    </Tag>
  );
}

// Skill icon (using antd icon)
function SkillIcon({ className = '', style = {} }: { className?: string; style?: React.CSSProperties }) {
  return <ThunderboltOutlined className={className} style={style} />;
}

// ─── Card grid ────────────────────────────────────────────────────────────────

function SkillCard({ skill, onClick }: { skill: LocalSkill; onClick: () => void }) {
  const { token } = theme.useToken();
  return (
    <Card
      hoverable
      size="small"
      onClick={onClick}
      styles={{ body: { padding: '12px', height: '100%', display: 'flex', flexDirection: 'column' } }}
      style={{
        height: '100%',
        backgroundColor: token.colorBgContainer,
        borderColor: token.colorBorderSecondary,
      }}
    >
      <Flex vertical gap={8} style={{ height: '100%' }}>
        {/* Header: icon + name */}
        <Flex align="center" gap={10}>
          <div
            style={{
              backgroundColor: token.colorPrimaryBg,
              width: 32,
              height: 32,
              borderRadius: 8,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0
            }}
          >
            <SkillIcon style={{ color: token.colorPrimary, fontSize: 16 }} />
          </div>
          <Text strong style={{ fontSize: token.fontSizeSM }} ellipsis={{ tooltip: skill.name }}>
            {skill.name}
          </Text>
        </Flex>

        {/* Description */}
        <div style={{ flex: 1 }}>
          <Paragraph type="secondary" style={{ fontSize: 12, margin: 0 }} ellipsis={{ rows: 2 }}>
            {skill.description || <span style={{ fontStyle: 'italic', opacity: 0.5 }}>No description</span>}
          </Paragraph>
        </div>

        {/* Footer */}
        <Flex align="center" gap={6} wrap="wrap">
          <SourceBadge source={skill.source} />
          {skill.version && <Text type="secondary" style={{ fontSize: '10px' }}>v{skill.version}</Text>}
          {skill.disabled && <Tag color="error">off</Tag>}
        </Flex>
      </Flex>
    </Card>
  );
}

// ─── Detail view ──────────────────────────────────────────────────────────────

function SkillDetail({ skill, onBack, onToggleDisabled }: {
  skill: LocalSkill;
  onBack: () => void;
  onToggleDisabled: (name: string, disabled: boolean) => void;
}) {
  const { token } = theme.useToken();
  const [editing, setEditing] = useState(false);
  const [readme, setReadme] = useState<string | null>(null);
  const [draftContent, setDraftContent] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    fetch(`/api/skills/${encodeURIComponent(skill.name)}/readme`)
      .then(r => r.ok ? r.text() : '')
      .then(setReadme)
      .catch(() => setReadme(''));
  }, [skill.name]);

  const handleSave = async () => {
    setSaving(true);
    try {
      await fetch(`/api/skills/${encodeURIComponent(skill.name)}/readme`, {
        method: 'PUT', headers: { 'Content-Type': 'text/plain' }, body: draftContent,
      });
      setReadme(draftContent);
      setEditing(false);
      message.success('Skill updated');
    } catch (err) {
      message.error('Failed to save skill');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Flex vertical style={{ height: '100%', overflow: 'hidden' }}>
      {/* Top bar */}
      <Flex
        align="center"
        gap={12}
        style={{
          padding: '12px 20px',
          backgroundColor: token.colorBgContainer,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          flexShrink: 0
        }}
      >
        <Button
          type="text"
          icon={<ArrowLeftOutlined />}
          onClick={onBack}
          size="small"
        >
          Back
        </Button>
        <div style={{ width: 1, height: 16, backgroundColor: token.colorBorderSecondary }} />

        {/* Name + meta */}
        <Flex align="center" gap={8} style={{ flex: 1, minWidth: 0 }}>
          <Text strong ellipsis={{ tooltip: skill.name }}>{skill.name}</Text>
          {skill.version && <Text type="secondary" style={{ fontSize: 12 }}>v{skill.version}</Text>}
          <SourceBadge source={skill.source} />
          {skill.disabled && <Tag color="error">disabled</Tag>}
        </Flex>

        {/* Actions */}
        <Space size="small">
          {editing ? (
            <>
              <Button size="small" onClick={() => setEditing(false)}>Cancel</Button>
              <Button size="small" type="primary" onClick={handleSave} loading={saving}>
                Save
              </Button>
            </>
          ) : (
            <Button
              size="small"
              icon={<EditOutlined />}
              onClick={() => { setDraftContent(readme ?? ''); setEditing(true); }}
            >
              Edit
            </Button>
          )}
          <Switch
            checked={!skill.disabled}
            onChange={() => onToggleDisabled(skill.name, skill.disabled)}
            size="small"
          />
        </Space>
      </Flex>

      {/* Path */}
      <div
        style={{
          backgroundColor: token.colorFillAlter,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          padding: '6px 20px',
          flexShrink: 0
        }}
      >
        <Text type="secondary" style={{ fontSize: '10px', fontFamily: 'monospace' }}>{skill.dir}</Text>
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflowY: 'auto', backgroundColor: token.colorBgContainer }}>
        {readme === null ? (
          <Flex align="center" justify="center" style={{ height: 200 }}>
            <Spin />
          </Flex>
        ) : editing ? (
          <Input.TextArea
            style={{
              height: '100%',
              fontFamily: 'monospace',
              fontSize: token.fontSizeSM,
              padding: 20,
              border: 'none',
              backgroundColor: 'transparent',
              resize: 'none'
            }}
            value={draftContent}
            onChange={e => setDraftContent(e.target.value)}
            spellCheck={false}
          />
        ) : readme ? (
          <div style={{ padding: '24px 28px', maxWidth: 900 }}>
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              rehypePlugins={[rehypeHighlight]}
              components={{
                h1: ({ children }) => <Title level={2} style={{ marginTop: 32, marginBottom: 12, fontWeight: 700, borderBottom: `1px solid ${token.colorBorderSecondary}`, paddingBottom: 8 }}>{children}</Title>,
                h2: ({ children }) => <Title level={3} style={{ marginTop: 28, marginBottom: 10, fontWeight: 600 }}>{children}</Title>,
                h3: ({ children }) => <Title level={4} style={{ marginTop: 24, marginBottom: 8, fontWeight: 600 }}>{children}</Title>,
                h4: ({ children }) => <Title level={5} style={{ marginTop: 20, marginBottom: 6 }}>{children}</Title>,
                p: ({ children }) => <Paragraph style={{ fontSize: token.fontSize, lineHeight: 1.75, marginBottom: 12, color: token.colorText }}>{children}</Paragraph>,
                code: ({ className, children, ...props }: any) => {
                  const isInline = !className;
                  if (isInline) {
                    return <code style={{ background: token.colorFillSecondary, padding: '2px 6px', borderRadius: 4, fontSize: '0.9em', fontFamily: 'Menlo, Monaco, monospace', color: token.colorError }} {...props}>{children}</code>;
                  }
                  return <code className={className} {...props}>{children}</code>;
                },
                pre: ({ children }) => <pre style={{ background: token.colorFillAlter, padding: 16, borderRadius: 8, overflow: 'auto', fontSize: 13, fontFamily: 'Menlo, Monaco, monospace', lineHeight: 1.6, border: `1px solid ${token.colorBorderSecondary}`, marginBottom: 16 }}>{children}</pre>,
                ul: ({ children }) => <ul style={{ paddingLeft: 24, marginBottom: 12, lineHeight: 1.7, color: token.colorText }}>{children}</ul>,
                ol: ({ children }) => <ol style={{ paddingLeft: 24, marginBottom: 12, lineHeight: 1.7, color: token.colorText }}>{children}</ol>,
                li: ({ children }) => <li style={{ marginBottom: 4, fontSize: token.fontSize }}>{children}</li>,
                blockquote: ({ children }) => <blockquote style={{ borderLeft: `4px solid ${token.colorPrimary}`, paddingLeft: 16, margin: '16px 0', color: token.colorTextSecondary, background: token.colorFillAlter, padding: '12px 16px', borderRadius: '0 8px 8px 0' }}>{children}</blockquote>,
                table: ({ children }) => <table style={{ width: '100%', borderCollapse: 'collapse', marginBottom: 16, fontSize: token.fontSizeSM }}>{children}</table>,
                th: ({ children }) => <th style={{ border: `1px solid ${token.colorBorderSecondary}`, padding: '8px 12px', background: token.colorFillAlter, fontWeight: 600, textAlign: 'left' }}>{children}</th>,
                td: ({ children }) => <td style={{ border: `1px solid ${token.colorBorderSecondary}`, padding: '8px 12px', color: token.colorText }}>{children}</td>,
                hr: () => <hr style={{ border: 'none', borderTop: `1px solid ${token.colorBorderSecondary}`, margin: '24px 0' }} />,
                a: ({ href, children }) => <a href={href} target="_blank" rel="noopener noreferrer" style={{ color: token.colorPrimary, textDecoration: 'underline' }}>{children}</a>,
                img: ({ src, alt }) => <img src={src} alt={alt} style={{ maxWidth: '100%', borderRadius: 8, marginBottom: 12 }} />,
              }}
            >
              {readme}
            </ReactMarkdown>
          </div>
        ) : (
          <Flex align="center" justify="center" style={{ height: 200 }}>
            <Text type="secondary" style={{ fontSize: 12 }}>No SKILL.md found</Text>
          </Flex>
        )}
      </div>
    </Flex>
  );
}

// ─── Browse tab ───────────────────────────────────────────────────────────────

function BrowseTab({ skills, onRefreshSkills, onReloadSuccess }: { skills: LocalSkill[]; onRefreshSkills: () => void; onReloadSuccess: () => void }) {
  const { token } = theme.useToken();
  const [query, setQuery] = useState('');
  const [selectedSkill, setSelectedSkill] = useState<LocalSkill | null>(null);
  const [remoteResults, setRemoteResults] = useState<RemoteResult[]>([]);
  const [remoteLoading, setRemoteLoading] = useState(false);
  const [remoteError, setRemoteError] = useState('');
  const [installingSlug, setInstallingSlug] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Sync disabled state when leaving detail
  const handleBack = () => setSelectedSkill(null);

  const handleToggleDisabled = async (name: string, currentlyDisabled: boolean) => {
    const action = currentlyDisabled ? 'enable' : 'disable';
    try {
      await apiFetch(`/api/skills/${encodeURIComponent(name)}/${action}`, { method: 'POST' });
      onRefreshSkills();
      onReloadSuccess();
      // Update skill object in detail view
      if (selectedSkill?.name === name) {
        setSelectedSkill(prev => prev ? { ...prev, disabled: !currentlyDisabled } : null);
      }
    } catch { /* ignore */ }
  };

  // Local filter
  const localMatched = query.trim()
    ? skills.filter(s =>
      s.name.toLowerCase().includes(query.toLowerCase()) ||
      s.description.toLowerCase().includes(query.toLowerCase())
    )
    : skills;

  // Remote search, debounced 500ms
  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    if (!query.trim()) { setRemoteResults([]); setRemoteError(''); return; }
    debounceRef.current = setTimeout(async () => {
      setRemoteLoading(true);
      setRemoteError('');
      try {
        const data = await apiFetch<{ results: RemoteResult[] }>(
          `/api/skills/remote-search?q=${encodeURIComponent(query)}`
        );
        setRemoteResults(data.results);
      } catch (err) {
        setRemoteError(err instanceof Error ? err.message : String(err));
        setRemoteResults([]);
      } finally {
        setRemoteLoading(false);
      }
    }, 500);
    return () => { if (debounceRef.current) clearTimeout(debounceRef.current); };
  }, [query]);

  const handleInstall = async (slug: string) => {
    setInstallingSlug(slug);
    try {
      await apiFetch('/api/skills/install', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ slug }),
      });
      message.success(`Skill ${slug} installed`);
      onRefreshSkills();
      setRemoteResults(prev => prev.map(r => r.slug === slug ? { ...r, installed: true } : r));
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Failed to install skill');
    } finally {
      setInstallingSlug(null);
    }
  };

  // Detail view
  if (selectedSkill) {
    return (
      <SkillDetail
        skill={selectedSkill}
        onBack={handleBack}
        onToggleDisabled={handleToggleDisabled}
      />
    );
  }

  // Card grid view
  const groups = groupBySource(localMatched);

  return (
    <Flex vertical style={{ height: '100%', overflow: 'hidden' }}>
      {/* Search bar */}
      <div
        style={{
          padding: '12px 20px',
          backgroundColor: token.colorBgContainer,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          flexShrink: 0
        }}
      >
        <Input
          prefix={<SearchOutlined style={{ color: token.colorTextQuaternary }} />}
          value={query}
          onChange={e => setQuery(e.target.value)}
          placeholder="Search skill name or description (local + ClaWHub)…"
          allowClear
          variant="filled"
          style={{
            borderRadius: '8px',
            background: token.colorFillAlter,
            border: 'none'
          }}
        />
      </div>

      <div style={{ flex: 1, overflowY: 'auto', padding: '20px' }}>
        <Flex vertical gap={28}>
          {/* Local skills: cards by source */}
          {groups.length === 0 && !query.trim() && (
            <Flex vertical align="center" justify="center" style={{ padding: '60px 0' }}>
              <div
                style={{
                  backgroundColor: token.colorPrimaryBg,
                  width: 48,
                  height: 48,
                  borderRadius: 16,
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  marginBottom: 16
                }}
              >
                <SkillIcon style={{ color: token.colorPrimary, fontSize: 24 }} />
              </div>
              <Text type="secondary">No skills installed.</Text>
            </Flex>
          )}

          {groups.length === 0 && query.trim() && !remoteLoading && (
            <div style={{ textAlign: 'center', padding: '16px 0' }}>
              <Text type="secondary" style={{ fontSize: 12 }}>No local skills match "{query}".</Text>
            </div>
          )}

          {groups.map(({ source, skills: groupSkills }) => (
            <section key={source}>
              <Flex align="center" gap={8} style={{ marginBottom: 12 }}>
                <SourceBadge source={source} />
                <Text type="secondary" style={{ fontSize: '10px' }}>{groupSkills.length}</Text>
              </Flex>
              <div style={{ display: 'grid', gap: 12, gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))' }}>
                {groupSkills.map(s => (
                  <SkillCard key={s.name} skill={s} onClick={() => setSelectedSkill(s)} />
                ))}
              </div>
            </section>
          ))}

          {/* Remote search results */}
          {query.trim() && (
            <section>
              <Flex align="center" gap={12} style={{ marginBottom: 16 }}>
                <Text strong type="secondary" style={{ fontSize: 12, textTransform: 'uppercase', letterSpacing: '0.05em' }}>ClaWHub</Text>
                {remoteLoading && <Spin size="small" />}
              </Flex>

              {remoteError && (
                <div style={{ marginBottom: 12 }}>
                  <Text type="danger" style={{ fontSize: 12 }}>{remoteError}</Text>
                </div>
              )}

              {!remoteLoading && remoteResults.length === 0 && !remoteError && (
                <div style={{ textAlign: 'center', padding: '12px 0' }}>
                  <Text type="secondary" style={{ fontSize: 12 }}>No remote results found on ClaWHub.</Text>
                </div>
              )}

              {/* Remote results: consistent card design */}
              <div style={{ display: 'grid', gap: 12, gridTemplateColumns: 'repeat(auto-fill, minmax(240px, 1fr))' }}>
                {remoteResults.map(r => (
                  <Card
                    key={r.slug}
                    size="small"
                    styles={{ body: { padding: '12px', height: '100%', display: 'flex', flexDirection: 'column' } }}
                    style={{
                      height: '100%',
                      backgroundColor: token.colorBgContainer,
                      borderColor: token.colorBorderSecondary,
                    }}
                  >
                    <Flex vertical gap={8} style={{ height: '100%' }}>
                      <Flex align="center" gap={10}>
                        <div
                          style={{
                            backgroundColor: token.colorInfoBg,
                            width: 32,
                            height: 32,
                            borderRadius: 8,
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            flexShrink: 0
                          }}
                        >
                          <SkillIcon style={{ color: token.colorInfo, fontSize: 16 }} />
                        </div>
                        <Text strong style={{ fontSize: token.fontSizeSM }} ellipsis={{ tooltip: r.displayName ?? r.slug }}>
                          {r.displayName ?? r.slug}
                        </Text>
                      </Flex>

                      <div style={{ flex: 1 }}>
                        <Paragraph type="secondary" style={{ fontSize: 12, margin: 0 }} ellipsis={{ rows: 2 }}>
                          {r.summary || <span style={{ fontStyle: 'italic', opacity: 0.5 }}>No description</span>}
                        </Paragraph>
                      </div>

                      <Flex align="center" justify="space-between" gap={8}>
                        <Flex align="center" gap={6}>
                          <Tag color="blue">ClaWHub</Tag>
                          {r.version && <Text type="secondary" style={{ fontSize: '10px' }}>v{r.version}</Text>}
                        </Flex>
                        {r.installed ? (
                          <Tag color="success">Installed</Tag>
                        ) : (
                          <Button
                            type="primary"
                            size="small"
                            icon={<DownloadOutlined />}
                            onClick={() => handleInstall(r.slug)}
                            loading={installingSlug === r.slug}
                          >
                            Install
                          </Button>
                        )}
                      </Flex>
                    </Flex>
                  </Card>
                ))}
              </div>
            </section>
          )}
        </Flex>
      </div>
    </Flex>
  );
}
function ManageTab({ skills, onRefreshSkills, onReloadSuccess }: { skills: LocalSkill[]; onRefreshSkills: () => void; onReloadSuccess: () => void }) {
  const { token } = theme.useToken();
  const [toggling, setToggling] = useState<string | null>(null);

  const handleToggle = async (name: string, currentlyDisabled: boolean) => {
    setToggling(name);
    const action = currentlyDisabled ? 'enable' : 'disable';
    try {
      await apiFetch(`/api/skills/${encodeURIComponent(name)}/${action}`, { method: 'POST' });
      onRefreshSkills();
      onReloadSuccess();
    } catch { /* ignore */ } finally {
      setToggling(null);
    }
  };

  const groups = groupBySource(skills);

  if (skills.length === 0) {
    return (
      <Flex vertical align="center" justify="center" style={{ padding: '80px 0' }}>
        <div
          style={{
            backgroundColor: token.colorPrimaryBg,
            width: 48,
            height: 48,
            borderRadius: 16,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            marginBottom: 16
          }}
        >
          <SkillIcon style={{ color: token.colorPrimary, fontSize: 24 }} />
        </div>
        <Text type="secondary">No skills installed yet.</Text>
      </Flex>
    );
  }

  return (
    <div style={{ flex: 1, overflowY: 'auto', padding: '20px' }}>
      <Flex vertical gap={24}>
        {groups.map(({ source, skills: groupSkills }) => (
          <section key={source}>
            <Flex align="center" gap={8} style={{ marginBottom: 12 }}>
              <SourceBadge source={source} />
              <Text type="secondary" style={{ fontSize: '10px' }}>
                {groupSkills.length} skill{groupSkills.length !== 1 ? 's' : ''}
              </Text>
            </Flex>
            <Card
              size="small"
              styles={{ body: { padding: 0 } }}
              style={{
                backgroundColor: token.colorBgContainer,
                borderColor: token.colorBorderSecondary,
                overflow: 'hidden'
              }}
            >
              {groupSkills.map((skill, idx) => (
                <div
                  key={skill.name}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: 12,
                    padding: '12px 16px',
                    borderBottom: idx < groupSkills.length - 1 ? `1px solid ${token.colorBorderSecondary}` : 'none',
                    transition: 'background-color 0.2s'
                  }}
                  onMouseEnter={(e) => { e.currentTarget.style.backgroundColor = token.colorFillAlter; }}
                  onMouseLeave={(e) => { e.currentTarget.style.backgroundColor = 'transparent'; }}
                >
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <Flex align="center" gap={8}>
                      <Text
                        strong={!skill.disabled}
                        type={skill.disabled ? "secondary" : undefined}
                        style={{ fontSize: token.fontSizeSM }}
                      >
                        {skill.name}
                      </Text>
                      {skill.version && <Text type="secondary" style={{ fontSize: '10px' }}>v{skill.version}</Text>}
                    </Flex>
                    {skill.description && (
                      <Text
                        type="secondary"
                        style={{ fontSize: 12, display: 'block', marginTop: 2 }}
                        ellipsis
                      >
                        {skill.description}
                      </Text>
                    )}
                  </div>
                  <Switch
                    checked={!skill.disabled}
                    onChange={() => handleToggle(skill.name, skill.disabled)}
                    disabled={toggling === skill.name}
                    size="small"
                  />
                </div>
              ))}
            </Card>
          </section>
        ))}
      </Flex>
    </div>
  );
}

// ─── Root ─────────────────────────────────────────────────────────────────────

export function SkillsPanel() {
  const { token } = theme.useToken();
  const [tab, setTab] = useState<Tab>('browse');
  const [skills, setSkills] = useState<LocalSkill[]>([]);
  const [loading, setLoading] = useState(true);

  const fetchSkills = useCallback(async () => {
    try {
      const data = await apiFetch<{ skills: LocalSkill[] }>('/api/skills');
      setSkills(data.skills);
    } catch { /* ignore */ } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchSkills(); }, [fetchSkills]);

  return (
    <Flex vertical style={{ height: '100%', overflow: 'hidden' }}>
      {/* Tab bar */}
      <Flex
        align="center"
        justify="space-between"
        style={{
          padding: '0 20px',
          backgroundColor: token.colorBgContainer,
          borderBottom: `1px solid ${token.colorBorderSecondary}`,
          flexShrink: 0
        }}
      >
        <Tabs
          activeKey={tab}
          onChange={k => setTab(k as Tab)}
          style={{ marginBottom: -1 }} // Align with borderBottom
          items={[
            {
              key: 'browse',
              label: 'Browse',
            },
            {
              key: 'manage',
              label: (
                <Space size={6}>
                  Manage
                  {skills.length > 0 && (
                    <span
                      style={{
                        backgroundColor: token.colorFillAlter,
                        color: token.colorTextSecondary,
                        fontSize: '10px',
                        padding: '1px 6px',
                        borderRadius: 10
                      }}
                    >
                      {skills.length}
                    </span>
                  )}
                </Space>
              ),
            },
          ]}
        />
        <Button
          type="text"
          icon={<ReloadOutlined />}
          onClick={() => {
            setLoading(true);
            fetchSkills().then(() => message.success('Refreshed'));
          }}
          title="Refresh list (does not affect running agents)"
          size="small"
        />
      </Flex>

      {loading ? (
        <Flex align="center" justify="center" style={{ flex: 1 }}>
          <Spin size="large" />
        </Flex>
      ) : tab === 'browse' ? (
        <BrowseTab
          skills={skills}
          onRefreshSkills={fetchSkills}
          onReloadSuccess={() => { }}
        />
      ) : (
        <ManageTab
          skills={skills}
          onRefreshSkills={fetchSkills}
          onReloadSuccess={() => { }}
        />
      )}
    </Flex>
  );
}
