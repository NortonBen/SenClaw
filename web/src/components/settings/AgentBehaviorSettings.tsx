import React, { useEffect, useState } from 'react';
import { Typography, Switch, Card, Space, Spin, message, Divider } from 'antd';
import { ThunderboltOutlined, BulbOutlined, SyncOutlined } from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

interface AgentBehavior {
  preTriggerSkill: boolean;
  preCognitive: boolean;
  afterProcess: boolean;
}

/**
 * Toggles for the process stages:
 *
 * **Pre-process stages** — run before the main agent turn.
 *   - preTriggerSkill: deterministic skill force-load by trigger matching.
 *   - preCognitive: cognitive-graph memory injection into the prompt.
 *
 * **After-process stages** — run after the main agent turn.
 *   - afterProcess: context update / conversation synthesis (Claude-Code style).
 *
 * Persisted globally (~/.senclaw/config.json) via `/api/agent-behavior` and
 * read per-turn by the daemon, so flips take effect on the next message.
 */
export const AgentBehaviorSettings: React.FC = () => {
  const [state, setState] = useState<AgentBehavior>({
    preTriggerSkill: false,
    preCognitive: false,
    afterProcess: false,
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
      <Title level={4}>Agent Behavior</Title>
      <Paragraph type="secondary">
        Optional processing stages that run before and after the main agent turn. Changes take effect on the
        next message.
      </Paragraph>

      {/* ── Pre-process ─────────────────────────────────────── */}
      <Title level={5} style={{ marginTop: 16, marginBottom: 8 }}>
        Pre-process Stages
      </Title>
      <Paragraph type="secondary" style={{ marginBottom: 12 }}>
        Run before the main agent turn to enrich the prompt.
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

      <Divider />

      {/* ── After-process ────────────────────────────────────── */}
      <Title level={5} style={{ marginTop: 0, marginBottom: 8 }}>
        After-process Stages
      </Title>
      <Paragraph type="secondary" style={{ marginBottom: 12 }}>
        Run after the main agent turn to keep context optimised.
      </Paragraph>
      <Space direction="vertical" size="middle" style={{ width: '100%' }}>
        {row(
          <SyncOutlined />,
          'Update context',
          'After each turn, synthesise and update the conversation context (inspired by Claude Code). The agent analyses recent messages, extracts key information, and compacts the context so future turns stay accurate and token-efficient without losing important history.',
          'afterProcess'
        )}
      </Space>
    </div>
  );
};
