import React from 'react';
import { Flex, Typography, theme, Card, Tag, Progress, Space } from 'antd';
import { 
  CodeOutlined,
  ThunderboltOutlined,
  SafetyCertificateOutlined,
  CloudUploadOutlined,
  BugOutlined,
  PlayCircleOutlined,
  ClockCircleOutlined,
  CheckCircleOutlined,
  SyncOutlined,
  ExperimentOutlined
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

// ─── Language Badge ───────────────────────────────────────────────────────────

function LangBadge({ name, color }: { name: string; color: string }) {
  const { token } = theme.useToken();
  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        gap: 4,
        fontSize: '11px',
        fontWeight: 500,
        color: token.colorText,
        backgroundColor: token.colorFillAlter,
        border: `1px solid ${token.colorBorderSecondary}`,
        padding: '3px 10px',
        borderRadius: '6px',
      }}
    >
      <span style={{
        width: 8,
        height: 8,
        borderRadius: '50%',
        backgroundColor: color,
        flexShrink: 0,
      }} />
      {name}
    </span>
  );
}

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

const CodePanel: React.FC = () => {
  const { token } = theme.useToken();

  const languages = [
    { name: 'Python', color: '#3776AB' },
    { name: 'JavaScript', color: '#F7DF1E' },
    { name: 'TypeScript', color: '#3178C6' },
    { name: 'Go', color: '#00ADD8' },
    { name: 'Rust', color: '#DEA584' },
    { name: 'Bash', color: '#4EAA25' },
  ];

  const features = [
    {
      icon: <PlayCircleOutlined />,
      title: 'Interactive REPL',
      description: 'Run code snippets in a sandboxed environment with real-time output streaming. Supports stdin/stdout interaction and environment variables.',
      status: 'in-progress' as const,
    },
    {
      icon: <SafetyCertificateOutlined />,
      title: 'Sandboxed Execution',
      description: 'Each code execution runs in an isolated container with configurable resource limits (CPU, memory, timeout). Zero risk to your host system.',
      status: 'in-progress' as const,
    },
    {
      icon: <BugOutlined />,
      title: 'Integrated Debugging',
      description: 'Set breakpoints, inspect variables, and step through code execution. Supports stack trace visualization and memory profiling.',
      status: 'planned' as const,
    },
    {
      icon: <CloudUploadOutlined />,
      title: 'Artifact Publishing',
      description: 'Package and publish code outputs as reusable artifacts. Share scripts, notebooks, and utilities across your agent network.',
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
              <ExperimentOutlined style={{ color: '#fff', fontSize: 24 }} />
            </div>
            <div>
              <Title level={3} style={{ margin: 0 }}>Code Executor</Title>
              <Text type="secondary" style={{ fontSize: 13 }}>
                Sandboxed code execution environment
              </Text>
            </div>
          </Flex>

          {/* Language Support */}
          <Card
            size="small"
            style={{
              backgroundColor: token.colorBgContainer,
              borderColor: token.colorBorderSecondary,
              borderRadius: 10,
            }}
            styles={{ body: { padding: '12px 16px' } }}
          >
            <Flex vertical gap={8}>
              <Text type="secondary" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
                Planned Language Support
              </Text>
              <Flex wrap="wrap" gap={6}>
                {languages.map(l => (
                  <LangBadge key={l.name} name={l.name} color={l.color} />
                ))}
              </Flex>
            </Flex>
          </Card>

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
              <Text strong style={{ fontSize: 12, color: token.colorPrimary }}>30%</Text>
            </Flex>
            <Progress
              percent={30}
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
              Core Capabilities
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

          {/* Architecture Preview */}
          <div style={{ marginTop: 8 }}>
            <Text strong style={{
              fontSize: 11,
              textTransform: 'uppercase',
              letterSpacing: '1px',
              color: token.colorTextTertiary,
              display: 'block',
              marginBottom: 16,
            }}>
              Architecture
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
              <Flex vertical gap={16}>
                {[
                  { label: 'Agent Request', desc: 'Agent submits code snippet with language and config', icon: <ThunderboltOutlined />, color: token.colorPrimary },
                  { label: 'Sandbox Init', desc: 'Isolated container spun up with resource limits', icon: <SafetyCertificateOutlined />, color: token.colorWarning },
                  { label: 'Execution', desc: 'Code runs with real-time stdout/stderr streaming', icon: <PlayCircleOutlined />, color: token.colorSuccess },
                  { label: 'Result', desc: 'Output, exit code, and artifacts returned to agent', icon: <CodeOutlined />, color: token.colorInfo },
                ].map((step, i) => (
                  <Flex key={i} align="flex-start" gap={12}>
                    <div style={{
                      width: 32,
                      height: 32,
                      borderRadius: 8,
                      backgroundColor: `${step.color}15`,
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'center',
                      flexShrink: 0,
                      marginTop: 2,
                    }}>
                      <span style={{ color: step.color, fontSize: 14 }}>{step.icon}</span>
                    </div>
                    <Flex vertical gap={2}>
                      <Flex align="center" gap={8}>
                        <span style={{
                          fontSize: '10px',
                          fontWeight: 700,
                          color: token.colorTextQuaternary,
                          width: 16,
                        }}>
                          {i + 1}.
                        </span>
                        <Text strong style={{ fontSize: 13 }}>{step.label}</Text>
                      </Flex>
                      <Text type="secondary" style={{ fontSize: 12, paddingLeft: 24 }}>{step.desc}</Text>
                    </Flex>
                  </Flex>
                ))}
              </Flex>
            </Card>
          </div>
        </Flex>
      </div>
    </Flex>
  );
};

export default CodePanel;
