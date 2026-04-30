import React from 'react';
import { Result, Button, Typography, Space, theme, Breadcrumb, Layout, Flex } from 'antd';
import { CalendarOutlined, RocketOutlined, HomeOutlined } from '@ant-design/icons';
import { useNavigate } from 'react-router-dom';
import { useAppContext } from '../contexts/AppContext';
import { AppLayout } from '../components/AppLayout';
import { AgentSidebar } from '../components/AgentSidebar';

const { Title, Text } = Typography;
const { Content } = Layout;

export function CoworkPage() {
  const { ws } = useAppContext();
  const { token } = theme.useToken();
  const navigate = useNavigate();

  return (
    <AppLayout sidebar={<AgentSidebar ws={ws} selectedJid={null} onSelect={() => navigate('/chats')} />}>
    <Layout style={{ background: 'transparent', height: '100%', display: 'flex', flexDirection: 'column' }}>
      {/* Main Header */}
      <header style={{ 
        padding: '0 24px', 
        height: 56, 
        display: 'flex', 
        alignItems: 'center', 
        borderBottom: `1px solid ${token.colorBorder}`,
        background: token.colorBgElevated,
        backdropFilter: 'blur(10px)',
        flexShrink: 0
      }}>
        <Breadcrumb
          items={[
            { 
              title: <Space onClick={() => navigate('/chats')} style={{ cursor: 'pointer' }}><HomeOutlined /><span>Home</span></Space>,
              className: 'opacity-80'
            },
            { 
              title: <Text type="secondary" style={{ fontSize: '13px' }}>Cowork Space</Text> 
            }
          ]}
        />
      </header>

      {/* Main content */}
      <Content style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
        <Flex align="center" justify="center" style={{ flex: 1, padding: '24px' }}>
          <div style={{ 
            maxWidth: '600px',
            width: '100%',
            background: token.colorBgContainer,
            borderRadius: token.borderRadiusLG,
            padding: '48px',
            border: `1px solid ${token.colorBorderSecondary}`,
            boxShadow: '0 8px 32px rgba(0,0,0,0.1)'
          }}>
            <Result
              icon={<CalendarOutlined style={{ fontSize: '72px', color: token.colorPrimary }} />}
              title={
                <Title level={2} style={{ margin: 0 }}>Cowork Space</Title>
              }
              subTitle={
                <Space direction="vertical" align="center" size="small">
                  <Text type="secondary" style={{ fontSize: '16px' }}>
                    We are building a collaborative space for your team.
                  </Text>
                  <div style={{ 
                    marginTop: 16, 
                    padding: '8px 16px', 
                    background: token.colorPrimaryBg, 
                    borderRadius: '20px',
                    border: `1px solid ${token.colorPrimaryBorder}`
                  }}>
                    <Space>
                      <RocketOutlined style={{ color: token.colorPrimary }} />
                      <Text strong style={{ color: token.colorPrimary }}>Currently under development</Text>
                    </Space>
                  </div>
                </Space>
              }
              extra={[
                <Button 
                  key="home" 
                  type="primary" 
                  size="large" 
                  onClick={() => navigate('/chats')}
                  style={{ borderRadius: '8px', minWidth: '120px' }}
                >
                  Go Back to Chats
                </Button>
              ]}
            />
          </div>
        </Flex>
      </Content>
    </Layout>
    </AppLayout>
  );
}
