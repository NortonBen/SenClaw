import { useState, useEffect, useCallback } from 'react';
import {
  Typography, Button, List, Space, Tag, Badge, Input, Divider,
  Spin, Empty, Popconfirm, theme, Tooltip, Flex
} from 'antd';
import {
  PlusOutlined, ProjectOutlined, TeamOutlined, MessageOutlined,
  AppstoreOutlined, SearchOutlined, DeleteOutlined, ReloadOutlined,
  RightOutlined, DownOutlined, CalendarOutlined, ClockCircleOutlined,
  ExclamationCircleOutlined
} from '@ant-design/icons';
import type { CoworkWorkspace } from '../types';

const { Text, Title } = Typography;

interface Props {
  workspaces: CoworkWorkspace[];
  selectedWs: CoworkWorkspace | null;
  onSelectWorkspace: (ws: CoworkWorkspace) => void;
  onCreateWorkspace: () => void;
  onDeleteWorkspace: (id: string) => void;
  onRefresh: () => void;
  loading?: boolean;
}

export function CoworkSidebar({
  workspaces,
  selectedWs,
  onSelectWorkspace,
  onCreateWorkspace,
  onDeleteWorkspace,
  onRefresh,
  loading,
}: Props) {
  const { token } = theme.useToken();
  const [search, setSearch] = useState('');
  const [expanded, setExpanded] = useState(true);

  const filtered = workspaces.filter(w =>
    w.name.toLowerCase().includes(search.toLowerCase())
  );

  const activeCount = workspaces.filter(w => w.status === 'active').length;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{ padding: '12px 16px 8px' }}>
        <Flex justify="space-between" align="center" style={{ marginBottom: 8 }}>
          <Flex align="center" gap={8}>
            <ProjectOutlined style={{ fontSize: 16, color: token.colorPrimary }} />
            <Text strong style={{ fontSize: 13 }}>Cowork</Text>
          </Flex>
          <Space size={4}>
            <Tooltip title="Refresh">
              <Button type="text" size="small" icon={<ReloadOutlined />} onClick={onRefresh} />
            </Tooltip>
          </Space>
        </Flex>
        <Input
          prefix={<SearchOutlined style={{ color: token.colorTextTertiary }} />}
          placeholder="Search workspaces..."
          variant="filled"
          size="small"
          value={search}
          onChange={e => setSearch(e.target.value)}
          style={{
            borderRadius: 8,
            background: token.colorFillAlter,
            border: 'none',
          }}
        />
      </div>

      {/* Workspace list */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '0 8px' }}>
        <Flex
          justify="space-between"
          align="center"
          style={{ padding: '4px 8px', cursor: 'pointer' }}
          onClick={() => setExpanded(!expanded)}
        >
          <Text style={{ fontSize: 11, textTransform: 'uppercase', color: token.colorTextTertiary, letterSpacing: '0.5px' }}>
            Workspaces ({workspaces.length})
          </Text>
          {expanded ? <DownOutlined style={{ fontSize: 10, color: token.colorTextTertiary }} /> : <RightOutlined style={{ fontSize: 10, color: token.colorTextTertiary }} />}
        </Flex>

        {expanded && (
          <>
            {loading && workspaces.length === 0 ? (
              <Spin size="small" style={{ display: 'block', padding: 16, textAlign: 'center' }} />
            ) : filtered.length === 0 ? (
              <div style={{ padding: 16, textAlign: 'center' }}>
                <Text type="secondary" style={{ fontSize: 12 }}>
                  {search ? 'No matching workspaces' : 'No workspaces'}
                </Text>
                <br />
                {!search && (
                  <Button type="link" size="small" onClick={onCreateWorkspace} style={{ marginTop: 4 }}>
                    Create one
                  </Button>
                )}
              </div>
            ) : (
              <List
                dataSource={filtered}
                renderItem={ws => {
                  const isSelected = selectedWs?.id === ws.id;
                  return (
                    <div
                      key={ws.id}
                      onClick={() => onSelectWorkspace(ws)}
                      style={{
                        padding: '8px 10px',
                        cursor: 'pointer',
                        borderRadius: 10,
                        marginBottom: 2,
                        background: isSelected ? `${token.colorPrimary}12` : 'transparent',
                        border: isSelected ? `1px solid ${token.colorPrimary}25` : '1px solid transparent',
                        transition: 'all 0.2s',
                      }}
                      onMouseEnter={e => {
                        if (!isSelected) e.currentTarget.style.background = token.colorFillAlter;
                      }}
                      onMouseLeave={e => {
                        if (!isSelected) e.currentTarget.style.background = 'transparent';
                      }}
                    >
                      <Flex justify="space-between" align="center">
                        <Flex align="center" gap={8} style={{ flex: 1, minWidth: 0 }}>
                          <div style={{
                            width: 8, height: 8, borderRadius: '50%',
                            backgroundColor: ws.status === 'active' ? token.colorSuccess : token.colorTextQuaternary,
                            flexShrink: 0,
                          }} />
                          <div style={{ minWidth: 0 }}>
                            <Text
                              style={{
                                fontSize: 12,
                                fontWeight: isSelected ? 600 : 400,
                                color: isSelected ? token.colorPrimary : token.colorText,
                                display: 'block',
                                overflow: 'hidden',
                                textOverflow: 'ellipsis',
                                whiteSpace: 'nowrap',
                              }}
                            >
                              {ws.name}
                            </Text>
                            {ws.description && (
                              <Text type="secondary" style={{ fontSize: 10, display: 'block', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                                {ws.description}
                              </Text>
                            )}
                          </div>
                        </Flex>
                        {!isSelected && (
                          <Popconfirm
                            title="Delete workspace?"
                            onConfirm={e => { e?.stopPropagation(); onDeleteWorkspace(ws.id); }}
                            onCancel={e => e?.stopPropagation()}
                          >
                            <Button
                              type="text"
                              size="small"
                              danger
                              icon={<DeleteOutlined style={{ fontSize: 11 }} />}
                              onClick={e => e.stopPropagation()}
                              style={{ opacity: 0, transition: 'opacity 0.2s' }}
                              className="cowork-delete-btn"
                            />
                          </Popconfirm>
                        )}
                      </Flex>
                    </div>
                  );
                }}
              />
            )}
          </>
        )}
      </div>

      {/* Selected workspace navigation */}
      {selectedWs && (
        <>
          <Divider style={{ margin: '4px 0' }} />
          <div style={{ padding: '4px 8px 8px' }}>
            <Text style={{ fontSize: 11, textTransform: 'uppercase', color: token.colorTextTertiary, letterSpacing: '0.5px', padding: '0 8px' }}>
              {selectedWs.name}
            </Text>
          </div>
        </>
      )}

      {/* Schedule quick view */}
      <div style={{ borderTop: `1px solid ${token.colorBorderSecondary}`, padding: '8px 16px' }}>
        <Flex align="center" gap={6} style={{ marginBottom: 4 }}>
          <CalendarOutlined style={{ fontSize: 11, color: token.colorTextTertiary }} />
          <Text style={{ fontSize: 10, color: token.colorTextTertiary, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
            Schedule
          </Text>
        </Flex>
        {selectedWs ? (
          <div style={{
            padding: '6px 8px',
            borderRadius: 6,
            background: token.colorFillAlter,
            fontSize: 11,
            color: token.colorTextSecondary,
            textAlign: 'center',
          }}>
            <ClockCircleOutlined style={{ marginRight: 4 }} />
            No upcoming deadlines
          </div>
        ) : (
          <Text style={{ fontSize: 10, color: token.colorTextQuaternary }}>
            Select a workspace to see schedule
          </Text>
        )}
      </div>
    </div>
  );
}
