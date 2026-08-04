import React, { useMemo, useState } from 'react';
import { Layout, Typography, Card, theme, Menu } from 'antd';
import type { MenuProps } from 'antd';
import {
  SafetyOutlined,
  ApiOutlined,
  UserOutlined,
  ThunderboltOutlined,
  FilterOutlined,
  DatabaseOutlined,
  CloudDownloadOutlined,
  ExperimentOutlined,
  AudioOutlined,
  SoundOutlined,
  AppstoreOutlined,
  ControlOutlined,
  ScanOutlined,
  LinkOutlined,
} from '@ant-design/icons';
import { useAppContext } from '../contexts/AppContext';
import { AppLayout } from '../components/AppLayout';
import { GeneralSettings } from '../components/settings/GeneralSettings';
import { ChannelSettings } from '../components/settings/ChannelSettings';
import { AgentSettings } from '../components/settings/AgentSettings';
import { AgentBehaviorSettings } from '../components/settings/AgentBehaviorSettings';
import { LLMSettings } from '../components/settings/LLMSettings';
import { OAuthSettings } from '../components/settings/OAuthSettings';
import { ToolRulesSettings } from '../components/settings/ToolRulesSettings';
import { EmbeddingSettings } from '../components/settings/EmbeddingSettings';
import { LocalModelsSettings } from '../components/settings/LocalModelsSettings';
import { CognitiveSettings } from '../components/settings/CognitiveSettings';
import { WhisperSettings } from '../components/settings/WhisperSettings';
import { TtsSettings } from '../components/settings/TtsSettings';
import { OcrSettings } from '../components/settings/OcrSettings';

const { Content } = Layout;
const { Title, Text } = Typography;

type SettingsSection =
  | 'general'
  | 'tool-rules'
  | 'channels'
  | 'agents'
  | 'llm'
  | 'provider-signin'
  | 'embedding'
  | 'local-models'
  | 'whisper'
  | 'tts'
  | 'ocr'
  | 'cognitive'
  | 'agent-behavior';

export const SettingsPage: React.FC = () => {
  const { ws } = useAppContext();
  const { token } = theme.useToken();
  const [selectedJid, setSelectedJid] = useState<string | null>(null);
  const [activeSection, setActiveSection] = useState<SettingsSection>('agents');

  const handleSelect = (jid: string) => {
    setSelectedJid(jid);
    if (!ws.subscribed.has(jid)) ws.subscribe(jid);
  };

  const menuItems: MenuProps['items'] = useMemo(
    () => [
      { key: 'general', icon: <SafetyOutlined />, label: 'Permissions' },
      { key: 'tool-rules', icon: <FilterOutlined />, label: 'Tool Rules' },
      { key: 'channels', icon: <ApiOutlined />, label: 'Channels' },
      { key: 'agents', icon: <UserOutlined />, label: 'Profile' },
      { key: 'agent-behavior', icon: <ControlOutlined />, label: 'Agent Behavior' },
      { key: 'llm', icon: <ThunderboltOutlined />, label: 'LLM' },
      { key: 'provider-signin', icon: <LinkOutlined />, label: 'Provider Sign-in' },
      { key: 'embedding', icon: <DatabaseOutlined />, label: 'Embedding' },
      { key: 'local-models', icon: <CloudDownloadOutlined />, label: 'Local Models' },
      { key: 'whisper', icon: <AudioOutlined />, label: 'Whisper ASR' },
      { key: 'tts', icon: <SoundOutlined />, label: 'Text-to-Speech' },
      { key: 'ocr', icon: <ScanOutlined />, label: 'OCR' },
      { key: 'cognitive', icon: <ExperimentOutlined />, label: 'Cognitive' },
    ],
    []
  );

  const panelContent = useMemo(() => {
    switch (activeSection) {
      case 'general':
        return <GeneralSettings />;
      case 'tool-rules':
        return <ToolRulesSettings />;
      case 'channels':
        return (
          <ChannelSettings
            channels={ws.channels}
            onRegister={ws.registerChannel}
            onUnregister={ws.unregisterChannel}
            onUpdate={ws.updateChannel}
          />
        );
      case 'agents':
        return (
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
        );
      case 'llm':
        return <LLMSettings />;
      case 'provider-signin':
        return <OAuthSettings />;
      case 'embedding':
        return <EmbeddingSettings />;
      case 'local-models':
        return <LocalModelsSettings />;
      case 'whisper':
        return <WhisperSettings />;
      case 'tts':
        return <TtsSettings />;
      case 'ocr':
        return <OcrSettings />;
      case 'cognitive':
        return <CognitiveSettings />;
      case 'agent-behavior':
        return <AgentBehaviorSettings />;
      default:
        return null;
    }
  }, [activeSection, ws]);

  return (
    <AppLayout
      sidebar={
        <Menu
        mode="inline"
        selectedKeys={[activeSection]}
        items={menuItems}
        onClick={({ key }) => setActiveSection(key as SettingsSection)}
        style={{ border: 'none', padding: '8px 0' }}
      />
      }
    >
      <Content style={{ padding: '24px 40px', maxWidth: 1280, margin: '0 auto', width: '100%', overflowY: 'auto' }}>
        <div style={{ marginBottom: 24 }}>
          <Title level={2} style={{ margin: 0, fontWeight: 700 }}>Settings</Title>
          <Text type="secondary">Manage your application configurations, agents, and external integrations.</Text>
        </div>

        <Layout
          style={{
            background: 'transparent',
            gap: 0,
            alignItems: 'stretch',
            minHeight: 480,
          }}
          hasSider
        >
          <Layout.Content style={{ marginLeft: 24, minWidth: 0 }}>
            <Card
              style={{
                borderRadius: 16,
                boxShadow: token.boxShadowSecondary,
                border: `1px solid ${token.colorBorderSecondary}`,
                background: token.colorBgContainer,
                minHeight: 480,
              }}
              styles={{ body: { padding: '24px 28px 32px' } }}
            >
              {panelContent}
            </Card>
          </Layout.Content>
        </Layout>
      </Content>
    </AppLayout>
  );
};
