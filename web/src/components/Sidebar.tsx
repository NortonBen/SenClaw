import { Layout, Tooltip, theme, Button, Badge, Popover, List, Typography } from 'antd';
import {
  SettingOutlined,
  ApiOutlined,
  MessageOutlined,
  CoffeeOutlined,
  BulbOutlined,
  BookOutlined,
  BulbFilled,
  CodeOutlined,
  AppstoreOutlined,
  BellOutlined,
  BellFilled,
  CheckOutlined,
  ClockCircleOutlined,
  DeploymentUnitOutlined,
} from '@ant-design/icons';
import { useNavigate, useLocation } from 'react-router-dom';
import type { WsStatus, EventNotification } from '../types';

const { Sider } = Layout;

interface Props {
  status: WsStatus;
  sidebarContent: React.ReactNode;
  isDarkMode: boolean;
  toggleTheme: () => void;
  notifications: EventNotification[];
  onMarkRead: (id: string) => void;
  onClearAll: () => void;
}

const STATUS_MAP: Record<WsStatus, { color: string; label: string; animate: boolean }> = {
  connected: { color: '#52c41a', label: 'Connected', animate: false },
  connecting: { color: '#faad14', label: 'Connecting…', animate: true },
  disconnected: { color: '#f5222d', label: 'Disconnected', animate: false },
};

export function Sidebar({ status, isDarkMode, toggleTheme, sidebarContent, notifications, onMarkRead, onClearAll }: Props) {
  const navigate = useNavigate();
  const location = useLocation();
  const { token } = theme.useToken();
  const { label, color, animate } = STATUS_MAP[status];

  const unreadCount = notifications.filter(n => !n.read).length;

  const notifContent = (
    <div style={{ width: 300, maxHeight: 400, overflowY: 'auto' }}>
      <div className="flex items-center justify-between mb-2 px-1">
        <Typography.Text strong>Thông báo sự kiện</Typography.Text>
        {notifications.length > 0 && (
          <Button size="small" type="text" icon={<CheckOutlined />} onClick={onClearAll}>
            Xóa tất cả
          </Button>
        )}
      </div>
      {notifications.length === 0 ? (
        <div className="text-center py-6" style={{ color: token.colorTextSecondary }}>
          Không có thông báo
        </div>
      ) : (
        <List
          size="small"
          dataSource={[...notifications].reverse()}
          renderItem={(n) => {
            const startDate = new Date(n.startAt);
            const timeStr = startDate.toLocaleTimeString('vi-VN', { hour: '2-digit', minute: '2-digit' });
            const dateStr = startDate.toLocaleDateString('vi-VN', { day: '2-digit', month: '2-digit' });
            const isRenotify = n.kind === 'renotify';
            const isPending = n.kind === 'pending';
            const isStart = n.kind === 'start';
            const isLate = (n.delayedMs ?? 0) > 60_000;
            return (
              <List.Item
                style={{
                  background: n.read ? 'transparent' : token.colorPrimaryBg,
                  borderRadius: 6,
                  marginBottom: 4,
                  padding: '6px 10px',
                  cursor: 'pointer',
                }}
                onClick={() => onMarkRead(n.id)}
              >
                <div className="flex gap-2 w-full">
                  <ClockCircleOutlined style={{ color: isRenotify ? token.colorWarning : isPending ? token.colorTextSecondary : isStart ? token.colorSuccess : token.colorPrimary, marginTop: 2, flexShrink: 0 }} />
                  <div className="flex-1 min-w-0">
                    <div className="font-medium truncate" style={{ color: token.colorText }}>{n.title}</div>
                    <div className="text-xs" style={{ color: token.colorTextSecondary }}>
                      {isPending
                        ? `📅 Sắp nhắc · ${dateStr} ${timeStr}`
                        : isStart
                          ? '🔔 Bắt đầu ngay bây giờ'
                          : isRenotify
                            ? '🔁 Đang diễn ra'
                            : '⏰ Nhắc nhở'}
                      {!isPending && ` · ${dateStr} ${timeStr}`}
                      {isLate && ' · trễ'}
                    </div>
                  </div>
                  {!n.read && (
                    <span className="w-2 h-2 rounded-full flex-shrink-0 mt-1" style={{ background: token.colorPrimary }} />
                  )}
                </div>
              </List.Item>
            );
          }}
        />
      )}
    </div>
  );

  const getSelectedKey = () => {
    const path = location.pathname;
    if (path.startsWith('/wiki')) return 'wiki';
    if (path === '/' || path.startsWith('/chats')) return 'chats';
    if (path.startsWith('/settings')) return 'settings';
    if (path.startsWith('/plugins')) return 'plugins';
    if (path.startsWith('/cowork')) return 'cowork';
    if (path.startsWith('/code')) return 'code';
    if (path.startsWith('/space')) return 'space';
    if (path.startsWith('/cognitive')) return 'cognitive';
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
          <img src="/logo.svg" alt="SenClaw" className="w-8 h-8 object-contain" />
          <span className="font-bold text-lg tracking-tight" style={{ color: token.colorTextHeading }}>SenClaw</span>
          <Tooltip title={label}>
            <span className={`w-2 h-2 rounded-full flex-shrink-0 ${animate ? 'animate-pulse' : ''}`} style={{ background: color }} />
          </Tooltip>
          <div className="ml-auto">
            <Popover content={notifContent} trigger="click" placement="bottomRight" arrow={false}>
              <Badge count={unreadCount} size="small" offset={[-2, 2]}>
                <Button
                  type="text"
                  size="small"
                  icon={unreadCount > 0 ? <BellFilled style={{ color: token.colorPrimary }} /> : <BellOutlined />}
                />
              </Badge>
            </Popover>
          </div>
        </div>

        {/* TOP MENU: Horizontal Tabs */}
        <div className="flex items-center justify-around py-2 px-2" style={{ borderColor: token.colorBorderSecondary }}>
          <Tooltip title="Chat">
            <Button
              type={currentKey === 'chats' ? 'primary' : 'text'}
              icon={<MessageOutlined />}
              onClick={() => navigate('/chats')}
              className="flex-1 flex justify-center"
              title="Chat"
            ></Button>
          </Tooltip>
          <Tooltip title="Space">
            <Button
              type={currentKey === 'space' ? 'primary' : 'text'}
              icon={<AppstoreOutlined />}
              onClick={() => navigate('/space')}
              className="flex-1 flex justify-center"
            >
            </Button>
          </Tooltip>
          <Tooltip title="Cowork">
            <Button
              type={currentKey === 'cowork' ? 'primary' : 'text'}
              icon={<CoffeeOutlined />}
              onClick={() => navigate('/cowork')}
              className="flex-1 flex justify-center"
            ></Button>
          </Tooltip>
          <Tooltip title="Code">
            <Button
              type={currentKey === 'code' ? 'primary' : 'text'}
              icon={<CodeOutlined />}
              onClick={() => navigate('/code')}
              className="flex-1 flex justify-center"
            >
            </Button>
          </Tooltip>
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
            <Tooltip title="Cognitive memory">
              <Button
                type={currentKey === 'cognitive' ? 'primary' : 'text'}
                icon={<DeploymentUnitOutlined />}
                onClick={() => navigate('/cognitive')}
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
