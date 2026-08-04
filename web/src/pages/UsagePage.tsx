import { Layout, Typography } from 'antd';
import { AppLayout } from '../components/AppLayout';
import { TokenUsagePanel } from '../components/usage/TokenUsagePanel';
import { PricingEditor } from '../components/usage/PricingEditor';

const { Content } = Layout;
const { Title, Text } = Typography;

/** Token accounting dashboard — /usage. Data from /api/usage/* (llm_usage_log
 * / llm_usage_daily / model_pricing on the daemon). */
export function UsagePage() {
  return (
    <AppLayout sidebar={null}>
      <Layout
        style={{ background: 'transparent', height: '100%', padding: '24px', overflowY: 'auto' }}
      >
        <Content style={{ maxWidth: 1200, margin: '0 auto', width: '100%' }}>
          <div>
            <Title level={2} style={{ color: 'rgba(255,255,255,0.85)', margin: 0 }}>
              Token Usage
            </Title>
            <Text style={{ color: 'rgba(255,255,255,0.45)' }}>
              Token in/out and estimated cost across the agent, Space Apps, cognitive memory and
              embeddings.
            </Text>
          </div>
          <TokenUsagePanel showTitle={false} />
          <PricingEditor />
        </Content>
      </Layout>
    </AppLayout>
  );
}
