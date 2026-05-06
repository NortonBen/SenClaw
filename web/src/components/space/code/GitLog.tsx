import React from 'react';
import { Timeline, Typography, Tag, Popconfirm, Button, theme } from 'antd';
import { RollbackOutlined } from '@ant-design/icons';
import type { GitCommit } from '../../../hooks/useCode';

const { Text } = Typography;

interface Props {
  log: GitCommit[];
  onRollback?: (steps: number) => void;
}

export function GitLog({ log, onRollback }: Props) {
  const { token } = theme.useToken();

  if (!log || log.length === 0) {
    return <Text type="secondary" style={{ fontSize: 12 }}>No commits yet.</Text>;
  }

  return (
    <Timeline
      style={{ marginTop: 8 }}
      items={log.map((commit, idx) => ({
        color: idx === 0 ? 'green' : 'gray',
        children: (
          <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 8 }}>
            <div style={{ flex: 1, minWidth: 0 }}>
              <Text style={{ fontSize: 12, display: 'block', marginBottom: 2 }}>
                {commit.message}
              </Text>
              <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                <Tag style={{ fontSize: 10, margin: 0, fontFamily: 'monospace' }}>
                  {commit.hash.slice(0, 7)}
                </Tag>
                <Text type="secondary" style={{ fontSize: 10 }}>
                  {new Date(commit.date).toLocaleString()}
                </Text>
              </div>
            </div>
            {idx > 0 && onRollback && (
              <Popconfirm
                title={`Roll back ${idx} commit${idx > 1 ? 's' : ''}?`}
                onConfirm={() => onRollback(idx)}
                okText="Rollback"
                okButtonProps={{ danger: true }}
              >
                <Button
                  size="small"
                  type="text"
                  icon={<RollbackOutlined />}
                  style={{ color: token.colorTextTertiary, flexShrink: 0 }}
                />
              </Popconfirm>
            )}
          </div>
        ),
      }))}
    />
  );
}
