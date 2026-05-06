import React, { useState } from 'react';
import { Typography, Button, Tooltip, Spin, Input, theme } from 'antd';
import {
  FolderOutlined, SearchOutlined, EditOutlined, DeleteOutlined,
  CodeOutlined, BranchesOutlined,
} from '@ant-design/icons';
import type { CodeSession } from '../../../hooks/useCode';

const { Text } = Typography;

interface Props {
  sessions: CodeSession[];
  loading: boolean;
  activeId: string | null;
  onOpen: (session: CodeSession) => void;
  onArchive: (id: string) => void;
  onNew: () => void;
}

export function SessionSidebar({ sessions, loading, activeId, onOpen, onArchive, onNew }: Props) {
  const { token } = theme.useToken();
  const [search, setSearch] = useState('');

  const filtered = sessions.filter(s =>
    s.name.toLowerCase().includes(search.toLowerCase()) ||
    s.workspace.toLowerCase().includes(search.toLowerCase())
  );

  const langColor: Record<string, string> = {
    rust: '#CE422B', typescript: '#3178C6', javascript: '#F7DF1E',
    python: '#3572A5', go: '#00ADD8', java: '#B07219',
  };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Search + New */}
      <div style={{ padding: '12px 12px 8px', display: 'flex', gap: 8, alignItems: 'center' }}>
        <Input
          prefix={<SearchOutlined style={{ color: token.colorTextTertiary }} />}
          placeholder="Search sessions…"
          placeholder="Search projects…"
          variant="filled"
          size="small"
          value={search}
          onChange={e => setSearch(e.target.value)}
          style={{ borderRadius: 10, background: token.colorFillAlter, border: 'none', flex: 1 }}
        />
        <Tooltip title="New project">
          <Button
            type="text"
            size="small"
            icon={<EditOutlined />}
            onClick={onNew}
            style={{ flexShrink: 0, borderRadius: 8, color: token.colorTextSecondary }}
          />
        </Tooltip>
      </div>

      {/* Section label */}
      <div style={{ padding: '4px 16px 6px', display: 'flex', alignItems: 'center', gap: 6 }}>
        <Text style={{ fontSize: 11, color: token.colorTextTertiary, textTransform: 'uppercase', letterSpacing: '0.06em', fontWeight: 600 }}>
          Projects
        </Text>
        {loading && filtered.length > 0 && <Spin size="small" />}
      </div>

      {/* List */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '0 8px 8px' }}>
        {loading && filtered.length === 0 && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 24 }}>
            <Spin size="small" />
          </div>
        )}

        {!loading && filtered.length === 0 && (
          <div style={{ textAlign: 'center', padding: '32px 16px' }}>
            <CodeOutlined style={{ fontSize: 28, color: token.colorTextQuaternary, marginBottom: 8 }} />
            <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 12 }}>
              {search ? 'No projects match' : 'No projects yet'}
            </Text>
            {!search && (
              <Button size="small" type="primary" icon={<EditOutlined />} onClick={onNew} style={{ borderRadius: 8 }}>
                New Project
              </Button>
            )}
          </div>
        )}

        {filtered.map(session => {
          const isActive = activeId === session.id;
          const lang = session.language ?? '';
          const dot = langColor[lang] ?? token.colorTextQuaternary;

          return (
            <div
              key={session.id}
              className="session-item"
              onClick={() => onOpen(session)}
              style={{
                padding: '8px 12px',
                cursor: 'pointer',
                borderRadius: 12,
                marginBottom: 4,
                background: isActive ? `${token.colorPrimary}12` : 'transparent',
                border: `1px solid ${isActive ? `${token.colorPrimary}30` : 'transparent'}`,
                transition: 'all 0.2s cubic-bezier(0.4, 0, 0.2, 1)',
                position: 'relative',
                display: 'flex',
                alignItems: 'center',
                gap: 10,
              }}
              onMouseEnter={e => { if (!isActive) e.currentTarget.style.background = token.colorFillAlter; }}
              onMouseLeave={e => { if (!isActive) e.currentTarget.style.background = 'transparent'; }}
            >
              {/* Active indicator bar */}
              {isActive && (
                <div style={{
                  position: 'absolute',
                  left: 0,
                  top: '16%',
                  bottom: '16%',
                  width: 3,
                  background: token.colorPrimary,
                  borderRadius: '0 4px 4px 0',
                }} />
              )}

              {/* Avatar */}
              <div style={{
                width: 36,
                height: 36,
                borderRadius: 10,
                background: isActive ? `${token.colorPrimary}22` : token.colorFillSecondary,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: isActive ? token.colorPrimary : token.colorTextSecondary,
                fontSize: 16,
                flexShrink: 0,
                position: 'relative',
              }}>
                <FolderOutlined />
                {/* Lang dot */}
                {lang && (
                  <div style={{
                    position: 'absolute',
                    bottom: -2,
                    right: -2,
                    width: 10,
                    height: 10,
                    borderRadius: '50%',
                    background: dot,
                    border: `2px solid ${token.colorBgContainer}`,
                  }} />
                )}
              </div>

              {/* Text */}
              <div style={{ flex: 1, minWidth: 0 }}>
                <Text
                  style={{
                    fontSize: 13,
                    fontWeight: isActive ? 600 : 500,
                    color: isActive ? token.colorPrimary : token.colorText,
                    display: 'block',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                >
                  {session.name}
                </Text>
                <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginTop: 1 }}>
                  {session.git_enabled && (
                    <BranchesOutlined style={{ fontSize: 10, color: token.colorTextTertiary }} />
                  )}
                  <Text style={{
                    fontSize: 11,
                    color: token.colorTextDescription,
                    opacity: 0.8,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                    flex: 1,
                  }}>
                    {session.workspace.replace(/^\/Users\/[^/]+/, '~')}
                  </Text>
                </div>
              </div>

              {/* Archive on hover */}
              <Tooltip title="Archive">
                <Button
                  size="small"
                  type="text"
                  danger
                  icon={<DeleteOutlined style={{ fontSize: 11 }} />}
                  onClick={e => { e.stopPropagation(); onArchive(session.id); }}
                  style={{ opacity: 0, flexShrink: 0, transition: 'opacity 0.15s' }}
                  className="session-archive-btn"
                />
              </Tooltip>
            </div>
          );
        })}
      </div>

      {/* CSS for hover-reveal archive button */}
      <style>{`
        .session-item:hover .session-archive-btn { opacity: 0.5 !important; }
        .session-archive-btn:hover { opacity: 1 !important; }
      `}</style>
    </div>
  );
}
