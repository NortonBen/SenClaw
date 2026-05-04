import React from 'react';
import { Typography, theme } from 'antd';
import { useAppContext } from '../../contexts/AppContext';
import { ToolRulesPanel } from '../ToolRulesPanel';

const { Title, Paragraph } = Typography;

/**
 * Cài đặt auto-accept / force-request cho tool (Bash, MCP, …).
 * State lưu localStorage + đồng bộ WS khi backend hỗ trợ.
 */
export const ToolRulesSettings: React.FC = () => {
  const { ws } = useAppContext();
  const { token } = theme.useToken();

  return (
    <div>
      <Title level={4} style={{ marginBottom: 8 }}>
        Tool Rules
      </Title>
      <Paragraph type="secondary" style={{ marginBottom: 24, maxWidth: 720 }}>
        Tự động chấp nhận hoặc bắt buộc hỏi theo lệnh Bash, MCP server, hoặc nhóm tool.
        Cấu hình được lưu trên trình duyệt; khi daemon hỗ trợ, thay đổi sẽ đồng bộ qua WebSocket.
      </Paragraph>
      <div
        style={{
          maxWidth: 640,
          padding: 16,
          borderRadius: token.borderRadiusLG,
          border: `1px solid ${token.colorBorderSecondary}`,
          background: token.colorFillAlter,
        }}
      >
        <ToolRulesPanel
          embedded
          rules={ws.toolRules}
          dangerouslyAcceptAll={ws.dangerouslyAcceptAll}
          onAddRule={ws.addToolRule}
          onRemoveRule={ws.removeToolRule}
          onToggleRule={ws.toggleToolRule}
          onToggleAcceptAll={ws.setDangerouslyAcceptAll}
        />
      </div>
    </div>
  );
};
