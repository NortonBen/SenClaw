import { Button, Tag, Typography, theme } from 'antd';

const { Text } = Typography;

export interface CommonPermissionOption {
  key: string;
  label: string;
}

export interface CommonPermissionRequestCardProps {
  title?: string;
  toolName: string;
  content: string;
  requestId: string;
  options: CommonPermissionOption[];
  resolved?: { key: string; label: string };
  onResolve: (requestId: string, optionKey: string) => void;
}

export function CommonPermissionRequestCard({
  title,
  toolName,
  content,
  requestId,
  options,
  resolved,
  onResolve,
}: CommonPermissionRequestCardProps) {
  const { token } = theme.useToken();
  const heading = title?.trim() || `Permission request: ${toolName}`;

  return (
    <div
      style={{
        padding: '10px 12px',
        borderRadius: 12,
        background: token.colorBgContainer,
        border: `1px solid ${token.colorPrimaryBorder}`,
        fontSize: 13,
      }}
    >
      <Text strong style={{ display: 'block', marginBottom: 6 }}>
        {heading}
      </Text>
      <div
        style={{
          whiteSpace: 'pre-wrap',
          fontFamily: 'monospace',
          fontSize: 12,
          color: token.colorTextSecondary,
          padding: 8,
          borderRadius: 8,
          background: token.colorFillAlter,
          marginBottom: 8,
        }}
      >
        {content}
      </div>
      {resolved ? (
        <Tag color="default">Resolved: {resolved.label}</Tag>
      ) : (
        <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
          {options.map(opt => (
            <Button key={opt.key} size="small" onClick={() => onResolve(requestId, opt.key)}>
              {opt.label}
            </Button>
          ))}
        </div>
      )}
    </div>
  );
}
