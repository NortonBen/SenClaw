import React from 'react';
import { Typography, Badge, theme, Tooltip } from 'antd';
import {
  FileTextOutlined,
  CalendarOutlined,
  ClockCircleOutlined,
} from '@ant-design/icons';
import type { TodaySummary } from '../../hooks/useSpace';

const { Text } = Typography;

export type SpaceSection = 'notes' | 'calendar' | 'apps' | 'schedules';

export interface SpaceSidebarApp {
  id: string;
  name: string;
  icon?: string;
}

interface NavItem {
  key: SpaceSection | `app:${string}`;
  icon: React.ReactNode;
  label: string;
  badge?: number;
}

interface Props {
  activeSection: SpaceSection | `app:${string}`;
  onSelect: (s: SpaceSection | `app:${string}`) => void;
  todaySummary: TodaySummary | null;
  apps?: SpaceSidebarApp[];
}

export function SpaceSidebar({ activeSection, onSelect, todaySummary, apps = [] }: Props) {
  const { token } = theme.useToken();

  // Static, built-in sections.
  const staticItems: NavItem[] = [
    { key: 'notes', icon: <FileTextOutlined />, label: 'Ghi chú' },
    {
      key: 'calendar',
      icon: <CalendarOutlined />,
      label: 'Lịch trình',
      badge: todaySummary?.events?.length ?? 0,
    },
    { key: 'schedules', icon: <ClockCircleOutlined />, label: 'Định kỳ' },
  ];

  // Installed apps — rendered at the same level as the static items (no "Apps" parent).
  const appItems: NavItem[] = apps.map(app => ({
    key: `app:${app.id}` as `app:${string}`,
    icon: (
      <span
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          justifyContent: 'center',
          width: 24,
          height: 24,
          borderRadius: 7,
          background: token.colorFillSecondary,
          fontSize: 14,
          lineHeight: 1,
        }}
      >
        {app.icon ?? '▣'}
      </span>
    ),
    label: app.name,
  }));

  const renderItem = (item: NavItem) => {
    const active = activeSection === item.key;
    return (
      <button
        key={item.key}
        onClick={() => onSelect(item.key)}
        className="w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors"
        style={{
          background: active ? token.colorPrimaryBg : 'transparent',
          color: active ? token.colorPrimary : token.colorText,
          borderLeft: active ? `3px solid ${token.colorPrimary}` : '3px solid transparent',
          cursor: 'pointer',
          border: 'none',
          outline: 'none',
          paddingLeft: 16,
        }}
      >
        <span style={{ fontSize: 16 }}>{item.icon}</span>
        <span className="flex-1 text-sm font-medium">{item.label}</span>
        {item.badge !== undefined && item.badge > 0 && (
          <Badge count={item.badge} size="small" />
        )}
      </button>
    );
  };

  return (
    <div className="flex flex-col h-full">
      {/* Today brief */}
      {todaySummary && (
        <div
          className="px-4 py-3 border-b"
          style={{ borderColor: token.colorBorderSecondary }}
        >
          <Text type="secondary" className="text-xs uppercase tracking-wide">
            Hôm nay · {todaySummary.date}
          </Text>
          <div className="mt-1 flex gap-3">
            <Tooltip title="Sự kiện hôm nay">
              <span className="text-xs flex items-center gap-1" style={{ color: token.colorTextSecondary }}>
                <CalendarOutlined />
                {todaySummary.events?.length ?? 0} sự kiện
              </span>
            </Tooltip>
            <Tooltip title="Ghi chú gần đây">
              <span className="text-xs flex items-center gap-1" style={{ color: token.colorTextSecondary }}>
                <FileTextOutlined />
                {todaySummary.recent_notes?.length ?? 0} ghi chú
              </span>
            </Tooltip>
          </div>
        </div>
      )}

      {/* Nav items */}
      <nav className="flex-1 py-2">
        {staticItems.map(renderItem)}

        {appItems.length > 0 && (
          <>
            <div
              className="my-2 mx-4"
              style={{ borderTop: `1px solid ${token.colorBorderSecondary}` }}
            />
            {appItems.map(renderItem)}
          </>
        )}
      </nav>
    </div>
  );
}
