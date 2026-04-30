import React, { useEffect, useState } from 'react';
import { Typography, Switch, Card, Space, message, Spin, theme } from 'antd';
import { SafetyCertificateOutlined, AlertOutlined } from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

interface PermissionsState {
  skipMainAgentPermissions: boolean;
  skipAllAgentsPermissions: boolean;
}

export const GeneralSettings: React.FC = () => {
  const { token } = theme.useToken();
  const [perms, setPerms] = useState<PermissionsState>({
    skipMainAgentPermissions: false,
    skipAllAgentsPermissions: false,
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    fetch('/api/admin-permissions')
      .then((r) => r.json())
      .then((d: PermissionsState) => {
        setPerms(d);
        setLoading(false);
      })
      .catch((err) => {
        console.error('Failed to fetch permissions:', err);
        setLoading(false);
      });
  }, []);

  const save = async (next: PermissionsState) => {
    setSaving(true);
    try {
      const r = await fetch('/api/admin-permissions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(next),
      });
      if (!r.ok) throw new Error('failed');
      setPerms(next);
      message.success('Settings saved successfully');
    } catch (err) {
      message.error('Failed to save settings');
    } finally {
      setSaving(false);
    }
  };

  const toggleMain = (checked: boolean) => {
    save({ ...perms, skipMainAgentPermissions: checked });
  };

  const toggleAll = (checked: boolean) => {
    // When enabling "all agents", main agent toggle is also on (superset)
    save({
      skipMainAgentPermissions: checked ? true : perms.skipMainAgentPermissions,
      skipAllAgentsPermissions: checked,
    });
  };

  if (loading) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', padding: '40px' }}>
        <Spin size="large" />
      </div>
    );
  }

  return (
    <div style={{ maxWidth: 800 }}>
      <Title level={4} style={{ marginBottom: 24 }}>General Settings</Title>
      
      <Space direction="vertical" size="large" style={{ width: '100%' }}>
        <Card 
          hoverable 
          style={{ 
            borderRadius: 12, 
            border: `1px solid ${token.colorBorderSecondary}`,
            background: token.colorBgContainer
          }}
          styles={{ body: { padding: 24 } }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
            <Space align="start" size="middle">
              <div style={{ 
                padding: 10, 
                borderRadius: 10, 
                backgroundColor: token.colorPrimaryBg, 
                color: token.colorPrimary,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center'
              }}>
                <SafetyCertificateOutlined style={{ fontSize: 20 }} />
              </div>
              <div>
                <Text strong style={{ fontSize: 16 }}>Skip approval for main agent</Text>
                <Paragraph type="secondary" style={{ marginTop: 4, marginBottom: 0 }}>
                  When enabled, the main agent does not require step-by-step approval for file edits or system commands.
                </Paragraph>
              </div>
            </Space>
            <Switch 
              checked={perms.skipMainAgentPermissions || perms.skipAllAgentsPermissions} 
              onChange={toggleMain}
              disabled={saving || perms.skipAllAgentsPermissions}
            />
          </div>
        </Card>

        <Card 
          hoverable 
          style={{ 
            borderRadius: 12, 
            border: perms.skipAllAgentsPermissions ? `1px solid ${token.colorErrorBorder}` : `1px solid ${token.colorBorderSecondary}`,
            backgroundColor: perms.skipAllAgentsPermissions ? token.colorErrorBg : token.colorBgContainer
          }}
          styles={{ body: { padding: 24 } }}
        >
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
            <Space align="start" size="middle">
              <div style={{ 
                padding: 10, 
                borderRadius: 10, 
                backgroundColor: perms.skipAllAgentsPermissions ? token.colorErrorBg : token.colorWarningBg, 
                color: perms.skipAllAgentsPermissions ? token.colorError : token.colorWarning,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center'
              }}>
                <AlertOutlined style={{ fontSize: 20 }} />
              </div>
              <div>
                <Text strong style={{ fontSize: 16 }}>Skip approval for all agents</Text>
                <Paragraph type="secondary" style={{ marginTop: 4, marginBottom: 0 }}>
                  When enabled, every agent runs tools without approval. <Text type="danger" strong>Use only in fully trusted local setups.</Text>
                </Paragraph>
              </div>
            </Space>
            <Switch 
              checked={perms.skipAllAgentsPermissions} 
              onChange={toggleAll}
              disabled={saving}
            />
          </div>
        </Card>
      </Space>
    </div>
  );
};
