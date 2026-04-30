import React from 'react';
import { Flex, Typography, theme, Card, Space, Tag, Progress, Timeline } from 'antd';
import { 
  CoffeeOutlined, 
  TeamOutlined, 
  MessageOutlined, 
  FileTextOutlined,
  VideoCameraOutlined,
  ClockCircleOutlined,
  CheckCircleOutlined,
  SyncOutlined,
  ExperimentOutlined
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

// ─── Feature Card ─────────────────────────────────────────────────────────────

function FeatureCard({ 
  icon, 
  title, 
  description, 
  status 
}: { 
  icon: React.ReactNode; 
  title: string; 
  description: string; 
  status: 'planned' | 'in-progress' | 'completed';
}) {
  const { token } = theme.useToken();

  const statusConfig = {
    planned: { color: token.colorTextQuaternary, bg: token.colorFillAlter, label: 'Planned', icon: <ClockCircleOutlined /> },
    'in-progress': { color: token.colorPrimary, bg: token.colorPrimaryBg, label: 'In Progress', icon: <SyncOutlined spin /> },
    completed: { color: token.colorSuccess, bg: token.colorSuccessBg, label: 'Done', icon: <CheckCircleOutlined /> },
  };

  const cfg = statusConfig[status];

  return (
    <Card
      size="small"
      style={{
        backgroundColor: token.colorBgContainer,
        borderColor: token.colorBorderSecondary,
        borderRadius: 12,
        transition: 'all 0.2s',
      }}
      hoverable
      styles={{ body: { padding: '16px' } }}
    >
      <Flex vertical gap={12}>
        <Flex align="center" justify="space-between">
          <Flex align="center" gap={10}>
            <div style={{
              backgroundColor: token.colorPrimaryBg,
              width: 36,
              height: 36,
              borderRadius: 10,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
            }}>
              <span style={{ color: token.colorPrimary, fontSize: 18 }}>{icon}</span>
            </div>
            <Text strong style={{ fontSize: 14 }}>{title}</Text>
          </Flex>
          <Tag
            icon={cfg.icon}
            style={{
              margin: 0,
              fontSize: '10px',
              borderColor: cfg.color,
              color: cfg.color,
              backgroundColor: cfg.bg,
              borderRadius: '6px',
            }}
          >
            {cfg.label}
          </Tag>
        </Flex>
        <Paragraph
          type="secondary"
          style={{ margin: 0, fontSize: 12, lineHeight: 1.6 }}
        >
          {description}
        </Paragraph>
      </Flex>
    </Card>
  );
}

// ─── Main Panel ───────────────────────────────────────────────────────────────

const CoworkPanel: React.FC = () => {
  const { token } = theme.useToken();

  const features = [
    {
      icon: <TeamOutlined />,
      title: 'Shared Agent Workspace',
      description: 'Assign multiple agents to a shared workspace where they can collaborate, share context, and work on complex tasks together in real-time.',
      status: 'in-progress' as const,
    },
    {
      icon: <MessageOutlined />,
      title: 'Inter-Agent Messaging',
      description: 'Agents can communicate with each other through structured message channels, request reviews, and coordinate task handoffs.',
      status: 'planned' as const,
    },
    {
      icon: <FileTextOutlined />,
      title: 'Shared Knowledge Boards',
      description: 'Create pinned documents and notes visible to all agents in a workspace. Perfect for project briefs, guidelines, and accumulated context.',
      status: 'planned' as const,
    },
    {
      icon: <VideoCameraOutlined />,
      title: 'Session Recording & Replay',
      description: 'Record agent collaboration sessions for review, auditing, and training. Replay step-by-step to understand decision processes.',
      status: 'planned' as const,
    },
  ];

  return (
    <Flex vertical style={{ height: '100%', background: token.colorBgLayout }}>
      {/* Hero Section */}
      <div style={{
        padding: '32px 32px 24px',
        background: `linear-gradient(135deg, ${token.colorPrimaryBg} 0%, ${token.colorBgContainer} 100%)`,
        borderBottom: `1px solid ${token.colorBorderSecondary}`,
      }}>
        <Flex vertical gap={16} style={{ maxWidth: 720 }}>
          <Flex align="center" gap={12}>
            <div style={{
              width: 48,
              height: 48,
              borderRadius: 14,
              background: `linear-gradient(135deg, ${token.colorPrimary}, ${token.colorPrimaryActive})`,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              boxShadow: `0 4px 12px ${token.colorPrimary}40`,
            }}>
              <CoffeeOutlined style={{ color: '#fff', fontSize: 24 }} />
            </div>
            <div>
              <Title level={3} style={{ margin: 0 }}>Cowork Space</Title>
              <Text type="secondary" style={{ fontSize: 13 }}>
                Multi-agent collaborative workspace
              </Text>
            </div>
          </Flex>

          {/* Progress Overview */}
          <Card
            size="small"
            style={{
              backgroundColor: token.colorBgContainer,
              borderColor: token.colorBorderSecondary,
              borderRadius: 10,
            }}
            styles={{ body: { padding: '12px 16px' } }}
          >
            <Flex align="center" justify="space-between" style={{ marginBottom: 6 }}>
              <Text type="secondary" style={{ fontSize: 12 }}>Development Progress</Text>
              <Text strong style={{ fontSize: 12, color: token.colorPrimary }}>25%</Text>
            </Flex>
            <Progress
              percent={25}
              showInfo={false}
              strokeColor={token.colorPrimary}
              trailColor={token.colorFillSecondary}
              size="small"
            />
          </Card>
        </Flex>
      </div>

      {/* Features Grid */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '24px 32px' }}>
        <Flex vertical gap={24} style={{ maxWidth: 720 }}>
          {/* Section title */}
          <div>
            <Text strong style={{
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: '1px',
              color: token.colorTextTertiary,
            }}>
              Planned Features
            </Text>
          </div>

          {/* Feature Cards */}
          <div style={{
            display: 'grid',
            gap: '12px',
            gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))',
          }}>
            {features.map((f, i) => (
              <FeatureCard key={i} {...f} />
            ))}
          </div>

          {/* Roadmap Timeline */}
          <div style={{ marginTop: 8 }}>
            <Text strong style={{
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: '1px',
              color: token.colorTextTertiary,
              display: 'block',
              marginBottom: 16,
            }}>
              Roadmap
            </Text>
            <Card
              size="small"
              style={{
                backgroundColor: token.colorBgContainer,
                borderColor: token.colorBorderSecondary,
                borderRadius: 12,
              }}
              styles={{ body: { padding: '20px 24px' } }}
            >
              <Timeline
                items={[
                  {
                    color: token.colorPrimary,
                    children: (
                      <Flex vertical gap={2}>
                        <Text strong style={{ fontSize: 13 }}>Q2 2026 — Foundation</Text>
                        <Text type="secondary" style={{ fontSize: 12 }}>Core workspace architecture and agent assignment engine</Text>
                      </Flex>
                    ),
                  },
                  {
                    color: token.colorTextQuaternary,
                    children: (
                      <Flex vertical gap={2}>
                        <Text strong style={{ fontSize: 13 }}>Q3 2026 — Communication</Text>
                        <Text type="secondary" style={{ fontSize: 12 }}>Inter-agent messaging, shared boards, and context sync</Text>
                      </Flex>
                    ),
                  },
                  {
                    color: token.colorTextQuaternary,
                    children: (
                      <Flex vertical gap={2}>
                        <Text strong style={{ fontSize: 13 }}>Q4 2026 — Intelligence</Text>
                        <Text type="secondary" style={{ fontSize: 12 }}>Session replay, analytics, and automated coordination</Text>
                      </Flex>
                    ),
                  },
                ]}
              />
            </Card>
          </div>
        </Flex>
      </div>
    </Flex>
  );
};

export default CoworkPanel;
