import { Layout, Typography, Breadcrumb, theme, Space } from 'antd';
import { SkillsPanel } from './SkillsPanel';
import { SubagentsPanel } from './SubagentsPanel';
import { HooksPanel } from './HooksPanel';
import { MCPSettings } from './MCPSettings';
import { PluginsNavItem } from './PluginsSidebar';
import { Content } from 'antd/es/layout/layout';
import { ApiOutlined } from '@ant-design/icons';
import CoworkPanel from './CoworkPanel';
import CodePanel from './CodePanel';
import MarketplacePanel from './MarketplacePanel';
import { SpaceAppsSettings } from '../settings/SpaceAppsSettings';
import WorkflowsPanel from './WorkflowsPanel';
import AliasPanel from './AliasPanel';
import WidgetsPanel from './WidgetsPanel';
import { SandboxPanel } from './SandboxPanel';

const { Text } = Typography;

interface Props {
  activeNav: PluginsNavItem;
}

const NAV_LABEL: Record<PluginsNavItem, string> = {
  skills: 'Skills',
  subagents: 'Virtual Agents',
  hooks: 'System Hooks',
  mcp: 'MCP Servers',
  alias: 'Alias',
  cowork: 'Cowork',
  code: 'Code Executor',
  marketplace: 'Marketplace',
  'space-apps': 'Space Apps',
  workflows: 'Workflow',
  widgets: 'Widget',
  sandbox: 'Sandbox',
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
        {activeNav === 'alias' && <AliasPanel />}
        {activeNav === 'cowork' && <CoworkPanel />}
        {activeNav === 'code' && <CodePanel />}
        {activeNav === 'marketplace' && <MarketplacePanel />}
        {activeNav === 'space-apps' && <SpaceAppsSettings />}
        {activeNav === 'workflows' && <WorkflowsPanel />}
        {activeNav === 'widgets' && <WidgetsPanel />}
        {activeNav === 'sandbox' && <SandboxPanel />}
      </Content>
    </Layout>
  );
}
