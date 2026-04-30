import React from 'react';
import { Layout, Typography, Tabs, Card, Breadcrumb, theme } from 'antd';
import {
  SettingOutlined,
  SafetyOutlined,
  ApiOutlined,
  UserOutlined,
  ThunderboltOutlined,
  HomeOutlined
} from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { GeneralSettings } from '../components/settings/GeneralSettings';
import { ChannelSettings } from '../components/settings/ChannelSettings';
import { AgentSettings } from '../components/settings/AgentSettings';
import { LLMSettings } from '../components/settings/LLMSettings';

import type { WsHook } from '../hooks/useWebSocket';

const { Content } = Layout;
const { Title, Text } = Typography;

interface Props {
  ws: WsHook;
}

export const SettingsPage: React.FC<Props> = ({ ws }) => {
  const navigate = useNavigate();

  const { token } = theme.useToken();

  const items = [
    {
      key: 'general',
      label: (
        <span>
          <SafetyOutlined />
          Permissions
        </span>
      ),
      children: <GeneralSettings />,
    },
    {
      key: 'channels',
      label: (
        <span>
          <ApiOutlined />
          Channels
        </span>
      ),
      children: (
        <ChannelSettings
          channels={ws.channels}
          onRegister={ws.registerChannel}
          onUnregister={ws.unregisterChannel}
          onUpdate={ws.updateChannel}
        />
      ),
    },
    {
      key: 'agents',
      label: (
        <span>
          <UserOutlined />
          Agents
        </span>
      ),
      children: (
        <AgentSettings
          agents={ws.agents}
          channels={ws.channels}
          bindings={ws.bindings}
          onRegister={ws.registerAgent}
          onUnregister={ws.unregisterAgent}
          onUpdate={ws.updateAgent}
          onRegisterBinding={ws.registerBinding}
          onUnregisterBinding={ws.unregisterBinding}
        />
      ),
    },
    {
      key: 'llm',
      label: (
        <span>
          <ThunderboltOutlined />
          LLM
        </span>
      ),
      children: <LLMSettings />,
    },
  ];

  return (
    <Content style={{ padding: '24px 40px', maxWidth: 1200, margin: '0 auto', width: '100%' }}>

      <div style={{ marginBottom: 32 }}>
        <Title level={2} style={{ margin: 0, fontWeight: 700 }}>Settings</Title>
        <Text type="secondary">Manage your application configurations, agents, and external integrations.</Text>
      </div>

      <Card
        style={{
          borderRadius: 16,
          boxShadow: token.boxShadowSecondary,
          border: `1px solid ${token.colorBorderSecondary}`,
          background: token.colorBgContainer
        }}
        styles={{ body: { padding: '8px 24px 24px 24px' } }}
      >
        <Tabs
          defaultActiveKey="agents"
          items={items}
          size="large"
          tabBarStyle={{ marginBottom: 24 }}
        />
      </Card>
    </Content>
  );
};
