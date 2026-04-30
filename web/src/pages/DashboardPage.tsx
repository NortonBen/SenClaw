import { Layout, Typography, Row, Col, Card, Statistic } from 'antd';
import { RobotOutlined, MessageOutlined, FileTextOutlined, ApiOutlined } from '@ant-design/icons';
import type { WsHook } from '../hooks/useWebSocket';

const { Content } = Layout;
const { Title, Text } = Typography;

interface Props {
  ws: WsHook;
}

export function DashboardPage({ ws }: Props) {
  const activeAgents = ws.groups.filter(g => !g.isAdmin).length;
  const totalChats = ws.groups.length;

  return (
    <Layout style={{ background: 'transparent', height: '100%', padding: '24px', overflowY: 'auto' }}>
      <Content style={{ maxWidth: 1200, margin: '0 auto', width: '100%' }}>
        <div style={{ marginBottom: 32 }}>
          <Title level={2} style={{ color: 'rgba(255,255,255,0.85)', margin: 0 }}>Dashboard</Title>
          <Text style={{ color: 'rgba(255,255,255,0.45)' }}>Overview of your SemaClaw agents and activity.</Text>
        </div>

        <Row gutter={[24, 24]}>
          <Col xs={24} sm={12} lg={6}>
            <Card 
              style={{ background: 'rgba(13, 13, 31, 0.4)', borderColor: 'rgba(255,255,255,0.05)', borderRadius: '12px' }}
              bodyStyle={{ padding: '24px' }}
            >
              <Statistic 
                title={<Text style={{ color: 'rgba(255,255,255,0.45)' }}>Active Agents</Text>} 
                value={activeAgents} 
                prefix={<RobotOutlined style={{ color: '#5BBFE8', marginRight: 8 }} />}
                valueStyle={{ color: 'rgba(255,255,255,0.85)', fontSize: 32 }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card 
              style={{ background: 'rgba(13, 13, 31, 0.4)', borderColor: 'rgba(255,255,255,0.05)', borderRadius: '12px' }}
              bodyStyle={{ padding: '24px' }}
            >
              <Statistic 
                title={<Text style={{ color: 'rgba(255,255,255,0.45)' }}>Total Chats</Text>} 
                value={totalChats} 
                prefix={<MessageOutlined style={{ color: '#5BBFE8', marginRight: 8 }} />}
                valueStyle={{ color: 'rgba(255,255,255,0.85)', fontSize: 32 }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card 
              style={{ background: 'rgba(13, 13, 31, 0.4)', borderColor: 'rgba(255,255,255,0.05)', borderRadius: '12px' }}
              bodyStyle={{ padding: '24px' }}
            >
              <Statistic 
                title={<Text style={{ color: 'rgba(255,255,255,0.45)' }}>Wiki Documents</Text>} 
                value={6} 
                prefix={<FileTextOutlined style={{ color: '#5BBFE8', marginRight: 8 }} />}
                valueStyle={{ color: 'rgba(255,255,255,0.85)', fontSize: 32 }}
              />
            </Card>
          </Col>
          <Col xs={24} sm={12} lg={6}>
            <Card 
              style={{ background: 'rgba(13, 13, 31, 0.4)', borderColor: 'rgba(255,255,255,0.05)', borderRadius: '12px' }}
              bodyStyle={{ padding: '24px' }}
            >
              <Statistic 
                title={<Text style={{ color: 'rgba(255,255,255,0.45)' }}>Active Plugins</Text>} 
                value={3} 
                prefix={<ApiOutlined style={{ color: '#5BBFE8', marginRight: 8 }} />}
                valueStyle={{ color: 'rgba(255,255,255,0.85)', fontSize: 32 }}
              />
            </Card>
          </Col>
        </Row>

        <div style={{ marginTop: 32 }}>
          <Title level={4} style={{ color: 'rgba(255,255,255,0.85)', marginBottom: 16 }}>Recent Activity</Title>
          <Card style={{ background: 'rgba(13, 13, 31, 0.4)', borderColor: 'rgba(255,255,255,0.05)', borderRadius: '12px', minHeight: 300 }}>
            <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: 250 }}>
              <Text style={{ color: 'rgba(255,255,255,0.3)' }}>No recent activity to display.</Text>
            </div>
          </Card>
        </div>
      </Content>
    </Layout>
  );
}
