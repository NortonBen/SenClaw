import { useState, useEffect, useCallback } from 'react';
import { Typography, Space, theme, Button, Input, Badge, Spin } from 'antd';
import {
  ApiOutlined,
  CodeOutlined,
  RobotOutlined,
  LinkOutlined,
  SearchOutlined,
  ReloadOutlined,
  CoffeeOutlined,
  ThunderboltOutlined,
  RightOutlined,
  DownOutlined,
  FolderOutlined,
  ExperimentOutlined,
  CloudServerOutlined
} from '@ant-design/icons';

const { Text } = Typography;

// ─── Types ────────────────────────────────────────────────────────────────────

export type PluginsNavItem = 'skills' | 'subagents' | 'hooks' | 'mcp' | 'cowork' | 'code';

interface SkillSummary {
  name: string;
  source: string;
  disabled: boolean;
}

interface SubagentSummary {
  name: string;
  disabled: boolean;
}

interface Props {
  activeNav: PluginsNavItem;
  onSelect: (id: PluginsNavItem) => void;
}

// ─── Constants ────────────────────────────────────────────────────────────────

const SOURCE_LABEL: Record<string, string> = {
  bundled: 'Bundled',
  'global-compat': 'Global',
  'global-sema': 'Global',
  'clawhub-managed': 'ClaWHub',
  workspace: 'Workspace',
};

const SOURCE_ORDER = ['bundled', 'clawhub-managed', 'global-compat', 'global-sema', 'workspace'];

// ─── Helpers ──────────────────────────────────────────────────────────────────

function groupSkillsBySource(skills: SkillSummary[]): { source: string; count: number }[] {
  const knownSources = new Set(SOURCE_ORDER);
  const counts: Record<string, number> = {};
  skills.forEach(s => {
    const src = s.source;
    counts[src] = (counts[src] || 0) + 1;
  });

  const groups = SOURCE_ORDER
    .filter(src => counts[src] > 0)
    .map(src => ({ source: src, count: counts[src] }));

  // Unknown sources
  Object.entries(counts).forEach(([src, count]) => {
    if (!knownSources.has(src)) {
      groups.push({ source: src, count });
    }
  });

  return groups;
}

// ─── Tree Item Components ─────────────────────────────────────────────────────

function SectionItem({
  icon,
  label,
  count,
  isSelected,
  isExpanded,
  onToggle,
  onClick,
  badge,
}: {
  icon: React.ReactNode;
  label: string;
  count?: number;
  isSelected: boolean;
  isExpanded: boolean;
  onToggle: () => void;
  onClick: () => void;
  badge?: React.ReactNode;
}) {
  const { token } = theme.useToken();

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: '8px',
        padding: '8px 12px',
        cursor: 'pointer',
        borderRadius: '8px',
        margin: '2px 8px',
        background: isSelected ? `${token.colorPrimary}15` : 'transparent',
        color: isSelected ? token.colorPrimary : token.colorText,
        transition: 'all 0.2s',
        fontWeight: isSelected ? 600 : 500,
      }}
      onClick={onClick}
      onMouseEnter={(e) => {
        if (!isSelected) e.currentTarget.style.background = token.colorFillAlter;
      }}
      onMouseLeave={(e) => {
        if (!isSelected) e.currentTarget.style.background = 'transparent';
      }}
    >
      <span
        onClick={(e) => { e.stopPropagation(); onToggle(); }}
        style={{
          fontSize: '8px',
          width: '14px',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          color: token.colorTextTertiary,
          flexShrink: 0,
          cursor: 'pointer',
        }}
      >
        {isExpanded ? <DownOutlined /> : <RightOutlined />}
      </span>
      <span style={{ fontSize: 14, opacity: isSelected ? 1 : 0.75, flexShrink: 0 }}>{icon}</span>
      <span style={{ flex: 1, fontSize: '13px' }}>{label}</span>
      {badge}
      {count !== undefined && count > 0 && (
        <span
          style={{
            fontSize: '10px',
            backgroundColor: isSelected ? `${token.colorPrimary}25` : token.colorFillSecondary,
            color: isSelected ? token.colorPrimary : token.colorTextTertiary,
            padding: '1px 7px',
            borderRadius: '10px',
            fontWeight: 500,
            lineHeight: '16px',
          }}
        >
          {count}
        </span>
      )}
    </div>
  );
}

function LeafItem({
  label,
  isActive,
  onClick,
  disabled,
  badge,
}: {
  label: string;
  isActive?: boolean;
  onClick: () => void;
  disabled?: boolean;
  badge?: React.ReactNode;
}) {
  const { token } = theme.useToken();

  return (
    <div
      onClick={disabled ? undefined : onClick}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: '8px',
        padding: '5px 12px',
        paddingLeft: '42px',
        cursor: disabled ? 'not-allowed' : 'pointer',
        borderRadius: '6px',
        margin: '1px 8px',
        transition: 'all 0.15s',
        color: disabled
          ? token.colorTextQuaternary
          : isActive
            ? token.colorPrimary
            : token.colorTextSecondary,
        fontSize: '12px',
        fontWeight: isActive ? 500 : 400,
        background: isActive ? `${token.colorPrimary}10` : 'transparent',
      }}
      onMouseEnter={(e) => {
        if (!disabled && !isActive) e.currentTarget.style.background = token.colorFillAlter;
      }}
      onMouseLeave={(e) => {
        if (!disabled && !isActive) e.currentTarget.style.background = 'transparent';
      }}
    >
      <FolderOutlined style={{ fontSize: 11, opacity: 0.5 }} />
      <span style={{ flex: 1 }}>{label}</span>
      {badge}
    </div>
  );
}

function StaticNavItem({
  icon,
  label,
  isSelected,
  onClick,
  badge,
}: {
  icon: React.ReactNode;
  label: string;
  isSelected: boolean;
  onClick: () => void;
  badge?: React.ReactNode;
}) {
  const { token } = theme.useToken();

  return (
    <div
      onClick={onClick}
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: '8px',
        padding: '8px 12px',
        paddingLeft: '34px',
        cursor: 'pointer',
        borderRadius: '8px',
        margin: '2px 8px',
        background: isSelected ? `${token.colorPrimary}15` : 'transparent',
        color: isSelected ? token.colorPrimary : token.colorTextSecondary,
        transition: 'all 0.2s',
        fontWeight: isSelected ? 600 : 400,
      }}
      onMouseEnter={(e) => {
        if (!isSelected) {
          e.currentTarget.style.background = token.colorFillAlter;
          e.currentTarget.style.color = token.colorText;
        }
      }}
      onMouseLeave={(e) => {
        if (!isSelected) {
          e.currentTarget.style.background = 'transparent';
          e.currentTarget.style.color = token.colorTextSecondary;
        }
      }}
    >
      <span style={{ fontSize: 14, opacity: isSelected ? 1 : 0.7 }}>{icon}</span>
      <span style={{ flex: 1, fontSize: '13px' }}>{label}</span>
      {badge}
    </div>
  );
}

// ─── Main Sidebar ─────────────────────────────────────────────────────────────

export function PluginsSidebar({ activeNav, onSelect }: Props) {
  const { token } = theme.useToken();
  const [query, setQuery] = useState('');
  const [skills, setSkills] = useState<SkillSummary[]>([]);
  const [subagents, setSubagents] = useState<SubagentSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [expanded, setExpanded] = useState<Set<string>>(new Set(['skills', 'subagents']));

  const fetchData = useCallback(async () => {
    setLoading(true);
    try {
      const [skillsRes, agentsRes] = await Promise.all([
        fetch('/api/skills').then(r => r.ok ? r.json() : { skills: [] }),
        fetch('/api/subagents').then(r => r.ok ? r.json() : { subagents: [] }),
      ]);
      setSkills(skillsRes.skills || []);
      setSubagents(agentsRes.subagents || []);
    } catch { /* ignore */ } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { fetchData(); }, [fetchData]);

  const toggleExpand = (key: string) => {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  // Filter tree items by query
  const q = query.toLowerCase().trim();
  const skillGroups = groupSkillsBySource(
    q ? skills.filter(s => s.name.toLowerCase().includes(q)) : skills
  );
  const filteredAgents = q
    ? subagents.filter(a => a.name.toLowerCase().includes(q))
    : subagents;

  const devBadge = (
    <span
      style={{
        fontSize: '9px',
        fontWeight: 600,
        color: token.colorWarning,
        backgroundColor: token.colorWarningBg,
        border: `1px solid ${token.colorWarningBorder}`,
        padding: '0 5px',
        borderRadius: '4px',
        lineHeight: '16px',
        letterSpacing: '0.5px',
      }}
    >
      DEV
    </span>
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', background: 'transparent' }}>
      {/* Header */}
      <div style={{ padding: '16px 16px 8px' }}>
        <Space direction="vertical" style={{ width: '100%' }} size="middle">
          <Button
            type="text"
            style={{ padding: 0, height: 'auto', textAlign: 'left' }}
            onClick={() => onSelect('skills')}
          >
            <Space>
              <ApiOutlined style={{ color: token.colorPrimary, fontSize: 16 }} />
              <Text strong style={{ letterSpacing: '1px', textTransform: 'uppercase', fontSize: '12px', opacity: 0.8 }}>
                Plugins & Tools
              </Text>
            </Space>
          </Button>

          <Input
            prefix={<SearchOutlined style={{ color: token.colorTextTertiary }} />}
            placeholder="Search plugins..."
            variant="filled"
            size="small"
            value={query}
            onChange={e => setQuery(e.target.value)}
            allowClear
            style={{
              borderRadius: '8px',
              background: token.colorFillAlter,
              border: 'none'
            }}
          />
        </Space>
      </div>

      {/* Tree Content */}
      <nav style={{ flex: 1, overflowY: 'auto', padding: '4px 0' }}>
        {loading ? (
          <div style={{ display: 'flex', justifyContent: 'center', padding: '24px 0' }}>
            <Spin size="small" />
          </div>
        ) : (
          <>
            {/* ─── Skills Section ─── */}
            <StaticNavItem
              icon={<ThunderboltOutlined />}
              label="Skills"
              isSelected={activeNav === 'skills'}
              onClick={() => onSelect('skills')}
            />

            <StaticNavItem
              icon={<CoffeeOutlined />}
              label="Subagents"
              isSelected={activeNav === 'subagents'}
              onClick={() => onSelect('subagents')}
            />


            {/* ─── Hooks (flat item) ─── */}
            <StaticNavItem
              icon={<LinkOutlined />}
              label="System Hooks"
              isSelected={activeNav === 'hooks'}
              onClick={() => onSelect('hooks')}
            />

            {/* ─── MCP Servers ─── */}
            <StaticNavItem
              icon={<CloudServerOutlined />}
              label="MCP Servers"
              isSelected={activeNav === 'mcp'}
              onClick={() => onSelect('mcp')}
            />


            {/* ─── Separator ─── */}
            <div style={{
              margin: '8px 20px',
              borderTop: `1px solid ${token.colorBorderSecondary}`,
            }} />

            {/* ─── Cowork (under development) ─── */}
            <StaticNavItem
              icon={<CoffeeOutlined />}
              label="Cowork"
              isSelected={activeNav === 'cowork'}
              onClick={() => onSelect('cowork')}
              badge={devBadge}
            />

            {/* ─── Code Executor (under development) ─── */}
            <StaticNavItem
              icon={<ExperimentOutlined />}
              label="Code Executor"
              isSelected={activeNav === 'code'}
              onClick={() => onSelect('code')}
              badge={devBadge}
            />

          </>
        )}
      </nav>

      {/* Bottom Actions */}
      <div style={{
        padding: '12px',
        borderTop: `1px solid ${token.colorBorderSecondary}`,
        display: 'flex',
        gap: '8px'
      }}>
        <Button
          block
          size="small"
          icon={<ReloadOutlined />}
          onClick={fetchData}
          loading={loading}
          style={{ borderRadius: '8px', fontSize: '12px' }}
        >
          Refresh
        </Button>
      </div>
    </div>
  );
}
