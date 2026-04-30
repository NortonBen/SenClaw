/**
 * WikiHome — wiki home
 * Recent edits + tag cloud; search-first when file count > 100
 */

import { useEffect } from 'react';
import { theme } from 'antd';
import type { WikiStats, TagEntry, DirNode } from '../hooks/useWiki';

interface Props {
  stats: WikiStats | null;
  tags: TagEntry[];
  tree: DirNode[];
  onSelectDoc: (path: string) => void;
  onSearch: (q: string) => void;
  fetchStats: () => void;
  fetchTags: () => void;
}

function relativeTime(iso: string): string {
  if (!iso) return '';
  const diff = Date.now() - new Date(iso).getTime();
  const m = Math.floor(diff / 60000);
  if (m < 1) return 'just now';
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  return `${d}d ago`;
}

export function WikiHome({ stats, tags, onSelectDoc, onSearch, fetchStats, fetchTags }: Props) {
  const { token } = theme.useToken();
  useEffect(() => {
    fetchStats();
    fetchTags();
  }, [fetchStats, fetchTags]);

  const isLarge = (stats?.totalFiles ?? 0) > 100;

  return (
    <div style={{ flex: 1, overflowY: 'auto' }}>
      <div style={{ maxWidth: 768, margin: '0 auto', padding: '32px' }}>

        {/* Search-first header (when large) */}
        {isLarge && (
          <div style={{ marginBottom: 32 }}>
            <input
              type="text"
              placeholder="Search wiki..."
              onFocus={() => {/* handled by sidebar */}}
              onClick={() => onSearch('')}
              readOnly
              style={{
                width: '100%', padding: '12px 16px', fontSize: 14,
                background: token.colorFillAlter, borderRadius: 12,
                border: `1px solid ${token.colorBorder}`, outline: 'none',
                color: token.colorText, transition: 'all 0.2s'
              }}
            />
          </div>
        )}

        {/* Stats overview */}
        {stats && (
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 16, marginBottom: 32 }}>
            <div style={{ background: token.colorWarningBg, borderRadius: 12, padding: 16, textAlign: 'center' }}>
              <div style={{ fontSize: 24, fontWeight: 'bold', color: token.colorWarning }}>{stats.totalFiles}</div>
              <div style={{ fontSize: 12, color: token.colorWarningTextActive, marginTop: 2 }}>pages</div>
            </div>
            <div style={{ background: token.colorInfoBg, borderRadius: 12, padding: 16, textAlign: 'center' }}>
              <div style={{ fontSize: 24, fontWeight: 'bold', color: token.colorInfo }}>{stats.totalDirs}</div>
              <div style={{ fontSize: 12, color: token.colorInfoTextActive, marginTop: 2 }}>folders</div>
            </div>
            <div style={{ background: token.colorSuccessBg, borderRadius: 12, padding: 16, textAlign: 'center' }}>
              <div style={{ fontSize: 24, fontWeight: 'bold', color: token.colorSuccess }}>{tags.length}</div>
              <div style={{ fontSize: 12, color: token.colorSuccessTextActive, marginTop: 2 }}>tags</div>
            </div>
          </div>
        )}

        <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, 1fr)', gap: 24 }}>
          {/* Recent files */}
          {stats && stats.recentFiles.length > 0 && (
            <div>
              <h2 style={{ fontSize: 12, fontWeight: 600, color: token.colorTextTertiary, textTransform: 'uppercase', letterSpacing: 1, marginBottom: 12 }}>Recently updated</h2>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                {stats.recentFiles.slice(0, 8).map(f => (
                  <button
                    key={f.path}
                    onClick={() => onSelectDoc(f.path)}
                    style={{
                      width: '100%', textAlign: 'left', padding: '10px 12px', borderRadius: 8,
                      background: 'transparent', border: 'none', cursor: 'pointer', transition: 'background 0.2s',
                    }}
                    onMouseEnter={(e) => e.currentTarget.style.background = token.colorFillAlter}
                    onMouseLeave={(e) => e.currentTarget.style.background = 'transparent'}
                  >
                    <div style={{ fontSize: 14, color: token.colorText, fontWeight: 500, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{f.title}</div>
                    <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginTop: 2 }}>
                      <span style={{ fontSize: 11, color: token.colorTextTertiary, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', flex: 1 }}>{f.path}</span>
                      <span style={{ fontSize: 11, color: token.colorTextQuaternary, flexShrink: 0 }}>{relativeTime(f.updated)}</span>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}

          {/* Tag cloud */}
          {tags.length > 0 && (
            <div>
              <h2 style={{ fontSize: 12, fontWeight: 600, color: token.colorTextTertiary, textTransform: 'uppercase', letterSpacing: 1, marginBottom: 12 }}>Tags</h2>
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 8 }}>
                {tags.slice(0, 24).map(t => (
                  <button
                    key={t.name}
                    onClick={() => onSearch(t.name)}
                    style={{
                      padding: '4px 10px', background: token.colorFillAlter, color: token.colorTextSecondary,
                      borderRadius: 100, fontSize: 12, border: 'none', cursor: 'pointer', transition: 'all 0.2s'
                    }}
                    onMouseEnter={(e) => { e.currentTarget.style.background = token.colorWarningBgHover; e.currentTarget.style.color = token.colorWarning; }}
                    onMouseLeave={(e) => { e.currentTarget.style.background = token.colorFillAlter; e.currentTarget.style.color = token.colorTextSecondary; }}
                    title={`${t.count} pages`}
                  >
                    {t.name}
                    <span style={{ marginLeft: 4, color: token.colorTextQuaternary, fontSize: 10 }}>{t.count}</span>
                  </button>
                ))}
              </div>

              {/* Category breakdown */}
              {stats && stats.byCategory.length > 0 && (
                <div style={{ marginTop: 24 }}>
                  <h2 style={{ fontSize: 12, fontWeight: 600, color: token.colorTextTertiary, textTransform: 'uppercase', letterSpacing: 1, marginBottom: 12 }}>Categories</h2>
                  <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
                    {stats.byCategory.slice(0, 8).map(cat => {
                      const max = stats.byCategory[0]?.count ?? 1;
                      const pct = Math.round((cat.count / max) * 100);
                      return (
                        <div key={cat.dir}>
                          <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', fontSize: 12, marginBottom: 4 }}>
                            <span style={{ color: token.colorTextSecondary, fontWeight: 500 }}>{cat.dir}</span>
                            <span style={{ color: token.colorTextTertiary }}>{cat.count}</span>
                          </div>
                          <div style={{ height: 6, background: token.colorFillAlter, borderRadius: 100, overflow: 'hidden' }}>
                            <div
                              style={{ height: '100%', background: token.colorWarning, borderRadius: 100, transition: 'width 0.3s', width: `${pct}%` }}
                            />
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Empty state */}
        {stats && stats.totalFiles === 0 && (
          <div style={{ textAlign: 'center', padding: '64px 0', color: token.colorTextQuaternary }}>
            <svg style={{ width: 48, height: 48, margin: '0 auto 16px auto', opacity: 0.3 }} fill="none" viewBox="0 0 24 24" strokeWidth={1} stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" d="M12 6.042A8.967 8.967 0 0 0 6 3.75c-1.052 0-2.062.18-3 .512v14.25A8.987 8.987 0 0 1 6 18c2.305 0 4.408.867 6 2.292m0-14.25a8.966 8.966 0 0 1 6-2.292c1.052 0 2.062.18 3 .512v14.25A8.987 8.987 0 0 0 18 18a8.967 8.967 0 0 0-6 2.292m0-14.25v14.25" />
            </svg>
            <p style={{ fontSize: 14 }}>Your wiki is empty</p>
            <p style={{ fontSize: 12, marginTop: 4 }}>Ask the Agent to add content to the wiki to get started</p>
          </div>
        )}
      </div>
    </div>
  );
}
