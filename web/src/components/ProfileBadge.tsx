import { Dropdown, Button, theme, Avatar } from 'antd';
import { UserOutlined, DownOutlined } from '@ant-design/icons';
import type { AgentInfo } from '../types';

interface Props {
  profiles: AgentInfo[];
  activeProfileId?: number | null;
  onChange: (profileId: number | null) => void;
}

export function ProfileBadge({ profiles, activeProfileId, onChange }: Props) {
  const { token } = theme.useToken();
  const active = profiles.find(p => p.id === activeProfileId);

  const items = [
    { key: '0', label: 'No profile' },
    ...profiles.map(p => ({
      key: String(p.id),
      label: (
        <div className="flex items-center gap-2">
          <Avatar size={18} icon={<UserOutlined />} style={{ background: token.colorPrimary, fontSize: 10 }} />
          <span>{p.name}</span>
        </div>
      ),
    })),
  ];

  return (
    <Dropdown
      menu={{
        items,
        selectedKeys: [String(activeProfileId ?? '0')],
        onClick: ({ key }) => onChange(key === '0' ? null : Number(key)),
      }}
      trigger={['click']}
      placement="topLeft"
    >
      <Button
        type="text"
        size="small"
        style={{ color: token.colorTextSecondary, fontSize: 11, padding: '0 4px', display: 'flex', alignItems: 'center', gap: 4 }}
      >
        <Avatar
          size={16}
          icon={<UserOutlined />}
          style={{ background: active ? token.colorPrimary : token.colorFillSecondary, fontSize: 9 }}
        />
        {active?.name ?? 'Profile'}
        <DownOutlined style={{ fontSize: 9 }} />
      </Button>
    </Dropdown>
  );
}
