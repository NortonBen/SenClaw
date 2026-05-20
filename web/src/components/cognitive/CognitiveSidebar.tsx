/**
 * Sidebar for the Cognitive page.
 *
 * Two responsibilities:
 *   1. Section nav (Search/Graph · DataPoints · Decay log) — mirrors how
 *      SpacePage's left rail switches sub-views.
 *   2. Live knowledge-graph stats so the user gets a one-glance sense of
 *      how much memory exists without having to scroll through the main
 *      panel. Polled lazily from `/api/cognitive/stats` and refreshed when
 *      the parent tells us the data changed (via a `refreshKey` prop).
 *
 * Kept dumb — no fetch logic of its own beyond stats. Section state lives
 * in the parent so it can deep-link / restore on reload.
 */

import React, { useEffect, useState } from 'react';
import { Typography, Tag, theme, Tooltip, Empty } from 'antd';
import {
  SearchOutlined,
  DatabaseOutlined,
  ClockCircleOutlined,
  ApartmentOutlined,
  ClusterOutlined,
} from '@ant-design/icons';

const { Text } = Typography;

export type CognitiveSection = 'search' | 'explorer' | 'datapoints' | 'decay';

interface NavItem {
  key: CognitiveSection;
  icon: React.ReactNode;
  label: string;
}

interface StatsResponse {
  edges: number;
  nodes_total: number;
  nodes_by_kind: [string, number][];
}

interface Props {
  activeSection: CognitiveSection;
  onSelect: (s: CognitiveSection) => void;
  /** Bump this whenever the main panel mutates memory (forget / add)
   *  so the sidebar re-fetches stats without polling. */
  refreshKey?: number;
}

export function CognitiveSidebar({ activeSection, onSelect, refreshKey = 0 }: Props) {
  const { token } = theme.useToken();
  const [stats, setStats] = useState<StatsResponse | null>(null);
  const [dormant, setDormant] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await fetch('/api/cognitive/stats');
        if (r.status === 503) {
          if (!cancelled) setDormant(true);
          return;
        }
        if (!r.ok) return;
        const body = (await r.json()) as StatsResponse;
        if (!cancelled) {
          setStats(body);
          setDormant(false);
        }
      } catch {
        /* non-fatal */
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refreshKey]);

  const navItems: NavItem[] = [
    { key: 'search', icon: <SearchOutlined />, label: 'Search & Graph' },
    { key: 'explorer', icon: <ClusterOutlined />, label: 'Graph explorer' },
    { key: 'datapoints', icon: <DatabaseOutlined />, label: 'Data memory' },
    { key: 'decay', icon: <ClockCircleOutlined />, label: 'Decay log' },
  ];

  return (
    <div className="flex flex-col h-full">
      {/* Stats summary */}
      <div
        className="px-4 py-3 border-b"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <Text type="secondary" className="text-xs uppercase tracking-wide">
          Knowledge graph
        </Text>

        {dormant ? (
          <div className="mt-2">
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={
                <span style={{ fontSize: 11 }}>
                  Dormant — configure an embedding provider
                </span>
              }
              style={{ margin: 0 }}
            />
          </div>
        ) : (
          <>
            <div className="mt-2 flex gap-4">
              <Tooltip title="Total nodes (entities + chunks + summaries)">
                <span
                  className="text-xs flex items-center gap-1"
                  style={{ color: token.colorTextSecondary }}
                >
                  <ApartmentOutlined />
                  <strong style={{ color: token.colorText }}>
                    {stats?.nodes_total ?? '—'}
                  </strong>{' '}
                  nodes
                </span>
              </Tooltip>
              <Tooltip title="Total edges across all tiers">
                <span
                  className="text-xs flex items-center gap-1"
                  style={{ color: token.colorTextSecondary }}
                >
                  <DatabaseOutlined />
                  <strong style={{ color: token.colorText }}>
                    {stats?.edges ?? '—'}
                  </strong>{' '}
                  edges
                </span>
              </Tooltip>
            </div>
            {stats && stats.nodes_by_kind.length > 0 && (
              <div className="mt-2 flex flex-wrap gap-1">
                {stats.nodes_by_kind.map(([kind, n]) => (
                  <Tag key={kind} style={{ fontSize: 10, margin: 0 }}>
                    {kind}: {n}
                  </Tag>
                ))}
              </div>
            )}
          </>
        )}
      </div>

      {/* Section nav */}
      <nav className="flex-1 py-2">
        {navItems.map(item => {
          const active = activeSection === item.key;
          return (
            <button
              key={item.key}
              onClick={() => onSelect(item.key)}
              className="w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors"
              style={{
                background: active ? token.colorPrimaryBg : 'transparent',
                color: active ? token.colorPrimary : token.colorText,
                borderLeft: active
                  ? `3px solid ${token.colorPrimary}`
                  : '3px solid transparent',
                cursor: 'pointer',
                border: 'none',
                outline: 'none',
              }}
            >
              {item.icon}
              <span className="text-sm">{item.label}</span>
            </button>
          );
        })}
      </nav>
    </div>
  );
}
