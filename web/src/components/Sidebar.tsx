import { Layout, Tooltip, theme, Button } from 'antd';
import {
  SettingOutlined,
  ApiOutlined,
  MessageOutlined,
  CoffeeOutlined,
  BulbOutlined,
  BookOutlined,
  BulbFilled,
  CodeOutlined,
} from '@ant-design/icons';
import { useNavigate, useLocation } from 'react-router-dom';
import type { WsStatus } from '../types';

const { Sider } = Layout;

interface Props {
  status: WsStatus;
  sidebarContent: React.ReactNode;
  isDarkMode: boolean;
  toggleTheme: () => void;
}

const STATUS_MAP: Record<WsStatus, { color: string; label: string; animate: boolean }> = {
  connected: { color: '#52c41a', label: 'Connected', animate: false },
  connecting: { color: '#faad14', label: 'Connecting…', animate: true },
  disconnected: { color: '#f5222d', label: 'Disconnected', animate: false },
};

export function Sidebar({ status, isDarkMode, toggleTheme, sidebarContent }: Props) {
  const navigate = useNavigate();
  const location = useLocation();
  const { token } = theme.useToken();
  const { label, color, animate } = STATUS_MAP[status];

  const getSelectedKey = () => {
    const path = location.pathname;
    if (path.startsWith('/wiki')) return 'wiki';
    if (path === '/' || path.startsWith('/chats')) return 'chats';
    if (path.startsWith('/settings')) return 'settings';
    if (path.startsWith('/plugins')) return 'plugins';
    if (path.startsWith('/cowork')) return 'cowork';
    if (path.startsWith('/code')) return 'code';
    return '';
  };

  const currentKey = getSelectedKey();

  return (
    <Sider
      width={300}
      className="h-screen flex flex-col select-none"
      style={{
        background: token.colorBgContainer,
        borderRight: `1px solid ${token.colorBorderSecondary}`,
      }}
    >
      <div className="flex flex-col h-full">
        {/* TOP: Logo & Name */}
        <div className="px-5 py-4 flex items-center gap-3 border-b" style={{ borderColor: token.colorBorderSecondary }}>
          <div className="w-8 h-8 rounded-lg bg-[#5BBFE8] flex items-center justify-center text-white font-bold">
            S
          </div>
          <span className="font-bold text-lg tracking-tight" style={{ color: token.colorTextHeading }}>SenClaw</span>
          <span className={`w-2 h-2 rounded-full flex-shrink-0 ${animate ? 'animate-pulse' : ''}`} style={{ background: color }} />
        </div>

        {/* TOP MENU: Horizontal Tabs */}
        <div className="flex items-center gap-1 p-2 border-b" style={{ borderColor: token.colorBorderSecondary }}>
          <Button
            type={currentKey === 'chats' ? 'primary' : 'text'}
            icon={<MessageOutlined />}
            onClick={() => navigate('/chats')}
            className="flex-1 flex justify-center"
          >
            Chat
          </Button>
          <Button
            type={currentKey === 'cowork' ? 'primary' : 'text'}
            icon={<CoffeeOutlined />}
            onClick={() => navigate('/cowork')}
            className="flex-1 flex justify-center"
          >
            Cowork
          </Button>
          <Button
            type={currentKey === 'code' ? 'primary' : 'text'}
            icon={<CodeOutlined />}
            onClick={() => navigate('/code')}
            className="flex-1 flex justify-center"
          >
            Code
          </Button>
        </div>

        {/* MIDDLE: Dynamic Injected Content */}
        <div className="flex-1 overflow-y-auto w-full py-2 min-h-0">
          {sidebarContent}
        </div>

        {/* BOTTOM MENU: Horizontal Actions */}
        <div className="mt-auto border-t flex flex-col" style={{ borderColor: token.colorBorderSecondary }}>
          <div className="flex items-center justify-around py-2 px-2">
            <Tooltip title="Wiki">
              <Button
                type={currentKey === 'wiki' ? 'primary' : 'text'}
                icon={<BookOutlined />}
                onClick={() => navigate('/wiki')}
              />
            </Tooltip>
            <Tooltip title="Plugins">
              <Button
                type={currentKey === 'plugins' ? 'primary' : 'text'}
                icon={<ApiOutlined />}
                onClick={() => navigate('/plugins')}
              />
            </Tooltip>
            <Tooltip title="Settings">
              <Button
                type={currentKey === 'settings' ? 'primary' : 'text'}
                icon={<SettingOutlined />}
                onClick={() => navigate('/settings')}
              />
            </Tooltip>
            <Tooltip title="Toggle Theme">
              <Button type="text" icon={isDarkMode ? <BulbFilled /> : <BulbOutlined />} onClick={toggleTheme} />
            </Tooltip>
          </div>
        </div>
      </div>
    </Sider>
  );
}
