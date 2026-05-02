/**
 * WikiStats — knowledge distribution / weights page
 */

import { useEffect } from 'react';
import { theme } from 'antd';
import type { WikiStats as WikiStatsType, TagEntry } from '../../hooks/useWiki';

interface Props {
  stats: WikiStats | null;
  tags: TagEntry[];
  fetchStats: () => void;
  fetchTags: () => void;
}

type WikiStats = WikiStatsType;

export function WikiStats({ stats, tags, fetchStats, fetchTags }: Props) {
  const { token } = theme.useToken();
  useEffect(() => {
    fetchStats();
    fetchTags();
  }, [fetchStats, fetchTags]);

  if (!stats) {
    return (
      <div style={{ flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', fontSize: 14, color: token.colorTextQuaternary }}>Loading...</div>
    );
  }

  const maxCat = Math.max(...stats.byCategory.map(c => c.count), 1);

  return (
    <div style={{ flex: 1, overflowY: 'auto' }}>
      <div style={{ maxWidth: 672, margin: '0 auto', padding: '32px' }}>
        <h1 style={{ fontSize: 16, fontWeight: 600, color: token.colorText, marginBottom: 24 }}>Knowledge stats</h1>

        {/* Summary */}
        <div style={{ display: 'flex', gap: 16, marginBottom: 32, fontSize: 14, color: token.colorTextSecondary }}>
          <span><strong style={{ color: token.colorText }}>{stats.totalFiles}</strong> pages</span>
          <span><strong style={{ color: token.colorText }}>{stats.totalDirs}</strong> folders</span>
          <span><strong style={{ color: token.colorText }}>{tags.length}</strong> tags</span>
        </div>

        {/* Category bars */}
        {stats.byCategory.length > 0 && (
          <div style={{ marginBottom: 32 }}>
            <h2 style={{ fontSize: 12, fontWeight: 600, color: token.colorTextTertiary, textTransform: 'uppercase', letterSpacing: 1, marginBottom: 16 }}>By folder</h2>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
              {stats.byCategory.map(cat => {
                const pct = Math.round((cat.count / maxCat) * 100);
                return (
                  <div key={cat.dir}>
                    <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontSize: 14, marginBottom: 4 }}>
                      <span style={{ fontWeight: 500, color: token.colorTextSecondary }}>{cat.dir}</span>
                      <span style={{ color: token.colorTextTertiary, fontSize: 12 }}>{cat.count} pages</span>
                    </div>
                    <div style={{ height: 8, background: token.colorFillAlter, borderRadius: 100, overflow: 'hidden' }}>
                      <div
                        style={{ height: '100%', background: token.colorWarning, borderRadius: 100, transition: 'width 0.5s', width: `${pct}%` }}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}

        {/* Tag cloud */}
        {tags.length > 0 && (
          <div>
            <h2 style={{ fontSize: 12, fontWeight: 600, color: token.colorTextTertiary, textTransform: 'uppercase', letterSpacing: 1, marginBottom: 16 }}>Popular tags</h2>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
              {tags.map(t => {
                const size = Math.max(11, Math.min(16, 11 + Math.log(t.count + 1) * 2));
                return (
                  <span
                    key={t.name}
                    style={{
                      padding: '4px 10px', background: token.colorFillAlter, color: token.colorTextSecondary,
                      borderRadius: 100, cursor: 'default', fontSize: size
                    }}
                    title={`${t.count} pages`}
                  >
                    {t.name}
                    <span style={{ marginLeft: 4, color: token.colorTextQuaternary, fontSize: 10 }}>×{t.count}</span>
                  </span>
                );
              })}
            </div>
          </div>
        )}

        {stats.totalFiles === 0 && (
          <div style={{ textAlign: 'center', padding: '48px 0', color: token.colorTextQuaternary, fontSize: 14 }}>No data yet</div>
        )}
      </div>
    </div>
  );
}
