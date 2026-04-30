import { List, Typography, Badge, Space, theme, Input } from 'antd';
import { RobotOutlined, UserOutlined, SearchOutlined } from '@ant-design/icons';
import { useState } from 'react';
import type { WsHook } from '../hooks/useWebSocket';
import type { AgentState } from '../types';

const { Text } = Typography;

interface Props {
  ws: WsHook;
  selectedJid: string | null;
  onSelect: (jid: string) => void;
}

export function AgentSidebar({ ws, selectedJid, onSelect }: Props) {
  const { token } = theme.useToken();
  const [search, setSearch] = useState('');

  const getAgentStatusColor = (state: AgentState) => {
    switch (state) {
      case 'idle': return token.colorSuccess;
      case 'thinking': return token.colorInfo;
      case 'executing': return token.colorWarning;
      case 'waiting_permission': return token.colorError;
      case 'waiting_question': return token.colorError;
      case 'error': return token.colorError;
      default: return token.colorTextTertiary;
    }
  };

  const getAgentStatusText = (state: AgentState) => {
    switch (state) {
      case 'idle': return 'Online';
      case 'thinking': return 'Thinking...';
      case 'executing': return 'Working...';
      case 'waiting_permission': return 'Permission Needed';
      case 'waiting_question': return 'Action Required';
      case 'error': return 'Error';
      default: return 'Idle';
    }
  };

  const filteredGroups = ws.groups.filter(g => 
    (g.name || '').toLowerCase().includes(search.toLowerCase())
  );

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div style={{ padding: '16px 16px 8px' }}>
        <Input
          prefix={<SearchOutlined style={{ color: token.colorTextTertiary }} />}
          placeholder="Search agents..."
          variant="filled"
          size="small"
          value={search}
          onChange={e => setSearch(e.target.value)}
          style={{ 
            borderRadius: '8px',
            background: token.colorFillAlter,
            border: 'none'
          }}
        />
      </div>

      <div style={{ flex: 1, overflowY: 'auto', padding: '8px' }}>
        <List
          itemLayout="horizontal"
          dataSource={filteredGroups}
          renderItem={(group) => {
            const isSelected = group.jid === selectedJid;
            const state = ws.agentStates[group.jid] || 'idle';
            const statusColor = getAgentStatusColor(state);
            
            return (
              <List.Item
                onClick={() => onSelect(group.jid)}
                style={{
                  padding: '8px 12px',
                  cursor: 'pointer',
                  borderBottom: 'none',
                  borderRadius: '12px',
                  marginBottom: '4px',
                  background: isSelected ? `${token.colorPrimary}15` : 'transparent',
                  border: isSelected ? `1px solid ${token.colorPrimary}33` : '1px solid transparent',
                  transition: 'all 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
                  position: 'relative',
                  overflow: 'hidden'
                }}
                onMouseEnter={(e) => {
                  if (!isSelected) {
                    e.currentTarget.style.background = token.colorFillAlter;
                  }
                }}
                onMouseLeave={(e) => {
                  if (!isSelected) {
                    e.currentTarget.style.background = 'transparent';
                  }
                }}
              >
                {isSelected && (
                  <div style={{
                    position: 'absolute',
                    left: 0,
                    top: '20%',
                    bottom: '20%',
                    width: '3px',
                    background: token.colorPrimary,
                    borderRadius: '0 4px 4px 0'
                  }} />
                )}
                <List.Item.Meta
                  avatar={
                    <div style={{ position: 'relative' }}>
                      <div style={{
                        width: 36, height: 36, borderRadius: '10px',
                        background: group.isAdmin ? `${token.colorPrimary}22` : token.colorFillSecondary,
                        display: 'flex', alignItems: 'center', justifyContent: 'center',
                        color: group.isAdmin ? token.colorPrimary : token.colorTextSecondary,
                        fontSize: '16px'
                      }}>
                        {group.isAdmin ? <UserOutlined /> : <RobotOutlined />}
                      </div>
                      {!group.isAdmin && (
                        <div style={{
                          position: 'absolute',
                          bottom: -2,
                          right: -2,
                          width: 10,
                          height: 10,
                          borderRadius: '50%',
                          background: statusColor,
                          border: `2px solid ${token.colorBgContainer}`,
                          boxShadow: state === 'thinking' || state === 'executing' ? `0 0 6px ${statusColor}` : 'none'
                        }} className={state === 'thinking' || state === 'executing' ? 'animate-pulse' : ''} />
                      )}
                    </div>
                  }
                  title={
                    <Text style={{
                      color: isSelected ? token.colorPrimary : token.colorText,
                      fontWeight: isSelected ? 600 : 500,
                      fontSize: '13px',
                      display: 'block'
                    }}>
                      {group.name || 'Unnamed Agent'}
                    </Text>
                  }
                  description={
                    <Text style={{ fontSize: '11px', color: token.colorTextDescription, opacity: 0.8 }}>
                      {group.isAdmin ? 'Main System' : getAgentStatusText(state)}
                    </Text>
                  }
                />
              </List.Item>
            );
          }}
        />
      </div>
    </div>
  );
}
