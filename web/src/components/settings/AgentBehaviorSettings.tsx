import React, { useEffect, useState } from 'react';
import { Typography, Switch, Card, Space, Spin, message } from 'antd';
import { ThunderboltOutlined, BulbOutlined } from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

interface AgentBehavior {
  preTriggerSkill: boolean;
  preCognitive: boolean;
}

/**
 * Toggles for the two pre-process stages that run before the main agent turn.
 * Persisted globally (~/.senclaw/config.json) via `/api/agent-behavior` and read
 * per-turn by the daemon, so flips take effect on the next message.
 */
export const AgentBehaviorSettings: React.FC = () => {
  const [state, setState] = useState<AgentBehavior>({
    preTriggerSkill: false,
    preCognitive: false,
  });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState<keyof AgentBehavior | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const res = await fetch('/api/agent-behavior');
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        setState(await res.json());
      } catch (err) {
        message.error(`Failed to load agent behavior: ${(err as Error).message}`);
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const toggle = async (key: keyof AgentBehavior, value: boolean) => {
    const prev = state;
    setState({ ...state, [key]: value }); // optimistic
    setSaving(key);
    try {
      const res = await fetch('/api/agent-behavior', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ [key]: value }),
      });
      if (!res.ok) throw new Error((await res.text()) || `HTTP ${res.status}`);
      setState(await res.json());
      message.success('Saved');
    } catch (err) {
      setState(prev); // revert on failure
      message.error(`Save failed: ${(err as Error).message}`);
    } finally {
      setSaving(null);
    }
  };

  if (loading) return <Spin />;

  const row = (
    icon: React.ReactNode,
    title: string,
    desc: string,
    key: keyof AgentBehavior
  ) => (
    <Card size="small">
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'flex-start',
          gap: 16,
        }}
      >
        <div>
          <Text strong>
            {icon} {title}
          </Text>
          <Paragraph type="secondary" style={{ marginBottom: 0, marginTop: 4 }}>
            {desc}
          </Paragraph>
        </div>
        <Switch
          checked={state[key]}
          loading={saving === key}
          onChange={(v) => toggle(key, v)}
        />
      </div>
    </Card>
  );

  return (
    <div style={{ maxWidth: 760 }}>
      <Title level={4}>Pre-process Stages</Title>
      <Paragraph type="secondary">
        Two optional stages that run before the main agent turn. Changes take effect on the
        next message.
      </Paragraph>
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        {row(
          <ThunderboltOutlined />,
          'Pre-trigger skill',
          "Match the message to a skill by its triggers / when-to-use and force-load that skill's instructions before the main turn — so a small model doesn't have to decide to call the Skill tool itself.",
          'preTriggerSkill'
        )}
        {row(
          <BulbOutlined />,
          'Pre-cognitive',
          'Retrieve relevant entries from the cognitive-graph memory for the message and inject them into the prompt as context before the main turn.',
          'preCognitive'
        )}
      </Space>
    </div>
  );
};
