import React from 'react';
import { Card, Space } from 'antd';
import { DesktopOutlined } from '@ant-design/icons';
import type { Host } from '../types';

interface HostCardProps {
  host: Host;
  selected: boolean;
  onClick: (host: Host) => void;
}

export const HostCard: React.FC<HostCardProps> = ({ host, selected, onClick }) => {
  return (
    <Card
      hoverable
      onClick={() => onClick(host)}
      style={{
        backgroundColor: selected ? '#1f2937' : '#111827',
        borderColor: selected ? '#3b82f6' : '#374151',
        cursor: 'pointer',
        borderRadius: 8,
      }}
      bodyStyle={{ padding: '16px' }}
    >
      <Space align="start">
        <div style={{
          backgroundColor: '#f97316',
          borderRadius: '50%',
          width: 32,
          height: 32,
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'center',
          color: '#fff',
          marginTop: 2
        }}>
          <DesktopOutlined />
        </div>
        <div>
          <div style={{ color: '#fff', fontWeight: 600, fontSize: 16 }}>
            {host.name || host.host}
          </div>
          <div style={{ color: '#9ca3af', fontSize: 12 }}>
            ssh, {host.user}
          </div>
        </div>
      </Space>
    </Card>
  );
};
