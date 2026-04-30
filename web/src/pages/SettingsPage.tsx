import React, { useState } from 'react';
import { Layout, Typography, Tabs, Card, theme } from 'antd';
import {
  SafetyOutlined,
  ApiOutlined,
  UserOutlined,
  ThunderboltOutlined,
} from '@ant-design/icons';
import { useAppContext } from '../contexts/AppContext';
import { AppLayout } from '../components/AppLayout';
import { AgentSidebar } from '../components/AgentSidebar';
import { GeneralSettings } from '../components/settings/GeneralSettings';
import { ChannelSettings } from '../components/settings/ChannelSettings';
import { AgentSettings } from '../components/settings/AgentSettings';
import { LLMSettings } from '../components/settings/LLMSettings';

const { Content } = Layout;
const { Title, Text } = Typography;

export const SettingsPage: React.FC = () => {
  const { ws } = useAppContext();
  const { token } = theme.useToken();
  const [selectedJid, setSelectedJid] = useState<string | null>(null);

  const handleSelect = (jid: string) => {
    setSelectedJid(jid);
    if (!ws.subscribed.has(jid)) ws.subscribe(jid);
  };

  const items = [
    {
      key: 'general',
      label: <span><SafetyOutlined />Permissions</span>,
      children: <GeneralSettings />,
    },
    {
      key: 'channels',
      label: <span><ApiOutlined />Channels</span>,
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
      label: <span><UserOutlined />Agents</span>,
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
      label: <span><ThunderboltOutlined />LLM</span>,
      children: <LLMSettings />,
    },
  ];

  return (
    <AppLayout
      sidebar={
        <AgentSidebar ws={ws} selectedJid={selectedJid} onSelect={handleSelect} />
      }
    >
      <Content style={{ padding: '24px 40px', maxWidth: 1200, margin: '0 auto', width: '100%', overflowY: 'auto' }}>
        <div style={{ marginBottom: 32 }}>
          <Title level={2} style={{ margin: 0, fontWeight: 700 }}>Settings</Title>
          <Text type="secondary">Manage your application configurations, agents, and external integrations.</Text>
        </div>
        <Card
          style={{
            borderRadius: 16,
            boxShadow: token.boxShadowSecondary,
            border: `1px solid ${token.colorBorderSecondary}`,
            background: token.colorBgContainer,
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
    </AppLayout>
  );
};
