import { Layout, Typography, Breadcrumb, theme, Space } from 'antd';
import { SkillsPanel } from './SkillsPanel';
import { SubagentsPanel } from './SubagentsPanel';
import { HooksPanel } from './HooksPanel';
import { MCPSettings } from '../components/settings/MCPSettings';
import { PluginsNavItem } from './PluginsSidebar';
import { Content } from 'antd/es/layout/layout';
import { ApiOutlined } from '@ant-design/icons';
import CoworkPanel from './cowork/CoworkPanel';
import CodePanel from './code/CodePanel';

const { Text } = Typography;

interface Props {
  activeNav: PluginsNavItem;
}

const NAV_LABEL: Record<PluginsNavItem, string> = {
  skills: 'Skills',
  subagents: 'Virtual Agents',
  hooks: 'System Hooks',
  mcp: 'MCP Servers',
  cowork: 'Cowork',
  code: 'Code Executor',
};

export default function PluginsView({ activeNav }: Props) {
  const { token } = theme.useToken();

  return (
    <Layout style={{ background: 'transparent', height: '100%', display: 'flex', flexDirection: 'column' }}>

      {/* Main content */}
      <Content style={{ flex: 1, overflowY: 'auto', display: 'flex', flexDirection: 'column' }}>
        {activeNav === 'skills' && <SkillsPanel />}
        {activeNav === 'subagents' && <SubagentsPanel />}
        {activeNav === 'hooks' && <HooksPanel />}
        {activeNav === 'mcp' && <MCPSettings />}
        {activeNav === 'cowork' && <CoworkPanel />}
        {activeNav === 'code' && <CodePanel />}
      </Content>
    </Layout>
  );
}
