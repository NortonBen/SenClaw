import React, { useEffect, useState } from 'react';
import {
  Typography,
  Form,
  Switch,
  InputNumber,
  Button,
  Alert,
  Spin,
  Divider,
  Space,
  Tag,
  message,
} from 'antd';
import {
  SaveOutlined,
  ExperimentOutlined,
  ReloadOutlined,
  ClearOutlined,
} from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

interface CognitiveFormValues {
  enabled: boolean;
  maxConcurrent: number;
  maxOutputChars: number;
  reflectMinChars: number;
  reflectMaxChars: number;
  reflectCooldownMs: number;
  autoReflection: boolean;
  maintenanceIntervalHours: number;
}

interface EffectiveValues {
  enabled: boolean;
  maxConcurrent: number;
  maxOutputChars: number;
  reflectMinChars: number;
  reflectMaxChars: number;
  reflectCooldownMs: number;
  autoReflection: boolean;
  maintenanceIntervalHours: number;
}

const DEFAULTS: CognitiveFormValues = {
  enabled: true,
  maxConcurrent: 2,
  maxOutputChars: 4096,
  reflectMinChars: 80,
  reflectMaxChars: 4000,
  reflectCooldownMs: 0,
  autoReflection: true,
  maintenanceIntervalHours: 24,
};

export const CognitiveSettings: React.FC = () => {
  const [form] = Form.useForm<CognitiveFormValues>();
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [effective, setEffective] = useState<EffectiveValues | null>(null);
  const [dirty, setDirty] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const res = await fetch('/api/cognitive-config');
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const json = await res.json();
      const saved = json.saved || {};
      const merged: CognitiveFormValues = {
        enabled: saved.enabled ?? json.effective?.enabled ?? DEFAULTS.enabled,
        maxConcurrent:
          saved.maxConcurrent ?? json.effective?.maxConcurrent ?? DEFAULTS.maxConcurrent,
        maxOutputChars:
          saved.maxOutputChars ?? json.effective?.maxOutputChars ?? DEFAULTS.maxOutputChars,
        reflectMinChars:
          saved.reflectMinChars ?? json.effective?.reflectMinChars ?? DEFAULTS.reflectMinChars,
        reflectMaxChars:
          saved.reflectMaxChars ?? json.effective?.reflectMaxChars ?? DEFAULTS.reflectMaxChars,
        reflectCooldownMs:
          saved.reflectCooldownMs ??
          json.effective?.reflectCooldownMs ??
          DEFAULTS.reflectCooldownMs,
        autoReflection:
          saved.autoReflection ?? json.effective?.autoReflection ?? DEFAULTS.autoReflection,
        maintenanceIntervalHours:
          saved.maintenanceIntervalHours ??
          json.effective?.maintenanceIntervalHours ??
          DEFAULTS.maintenanceIntervalHours,
      };
      form.setFieldsValue(merged);
      setEffective(json.effective || null);
      setDirty(false);
    } catch (err) {
      message.error(`Failed to load cognitive config: ${(err as Error).message}`);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSave = async () => {
    try {
      const values = await form.validateFields();
      setSaving(true);
      const res = await fetch('/api/cognitive-config', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(values),
      });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `HTTP ${res.status}`);
      }
      message.success('Cognitive config saved — restart the daemon to apply.');
      setDirty(false);
    } catch (err) {
      message.error(`Save failed: ${(err as Error).message}`);
    } finally {
      setSaving(false);
    }
  };

  const restartNeeded = (() => {
    if (!effective) return false;
    const vals = form.getFieldsValue();
    return (
      vals.enabled !== effective.enabled ||
      vals.maxConcurrent !== effective.maxConcurrent ||
      vals.maxOutputChars !== effective.maxOutputChars ||
      vals.reflectMinChars !== effective.reflectMinChars ||
      vals.reflectMaxChars !== effective.reflectMaxChars ||
      vals.reflectCooldownMs !== effective.reflectCooldownMs ||
      vals.autoReflection !== effective.autoReflection ||
      vals.maintenanceIntervalHours !== effective.maintenanceIntervalHours
    );
  })();

  const [maintRunning, setMaintRunning] = useState(false);
  const runMaintenance = async () => {
    setMaintRunning(true);
    try {
      const res = await fetch('/api/cognitive/maintenance', { method: 'POST' });
      if (!res.ok) {
        const text = await res.text();
        throw new Error(text || `HTTP ${res.status}`);
      }
      const r = await res.json();
      message.success(
        `Maintenance done — cleaned ${r.envelope_chunks_removed} chunks / ` +
          `${r.orphan_entities_removed} orphans, merged ${r.entities_merged} entities ` +
          `across ${r.groups_merged} groups, inferred ${r.associations_inferred ?? 0} ` +
          `association(s) (${r.duration_ms} ms)`
      );
    } catch (err) {
      message.error(`Maintenance failed: ${(err as Error).message}`);
    } finally {
      setMaintRunning(false);
    }
  };

  if (loading) {
    return (
      <div style={{ padding: 48, textAlign: 'center' }}>
        <Spin size="large" />
      </div>
    );
  }

  return (
    <div>
      <Title level={4} style={{ margin: 0 }}>
        <ExperimentOutlined /> Cognitive Memory
      </Title>
      <Paragraph type="secondary" style={{ marginTop: 8 }}>
        Governs the cognitive graph layer — extraction concurrency, per-message
        budget, and auto-reflection on user input. Changes are persisted
        immediately but only take effect after a daemon restart.
      </Paragraph>

      {restartNeeded && (
        <Alert
          type="warning"
          showIcon
          style={{ marginBottom: 16 }}
          message="Restart required"
          description="Saved values differ from what the live daemon is using. Restart senclaw to apply."
        />
      )}

      <Form
        form={form}
        layout="vertical"
        onValuesChange={() => setDirty(true)}
        initialValues={DEFAULTS}
      >
        <Form.Item
          name="enabled"
          label="Enable cognitive layer"
          valuePropName="checked"
          tooltip="Master switch. When off, cognify/reflection no-ops and tools return early."
        >
          <Switch />
        </Form.Item>

        <Form.Item
          name="autoReflection"
          label="Auto-reflect on every user message"
          valuePropName="checked"
          tooltip="When enabled, each incoming user message is cognified automatically. Off = manual CogAdd only."
        >
          <Switch />
        </Form.Item>

        <Divider style={{ margin: '12px 0' }} />

        <Form.Item
          name="maxConcurrent"
          label="Max concurrent extractions"
          tooltip="Semaphore size for in-flight LLM cognify calls. Keep low on local models."
          rules={[{ required: true, type: 'number', min: 1, max: 16 }]}
        >
          <InputNumber min={1} max={16} style={{ width: 200 }} />
        </Form.Item>

        <Form.Item
          name="maxOutputChars"
          label="Max LLM output chars"
          tooltip="Hard cap on cognify-LLM output. Streams are aborted past this length (guards runaway local decoding)."
          rules={[{ required: true, type: 'number', min: 256, max: 65536 }]}
        >
          <InputNumber min={256} max={65536} step={256} style={{ width: 200 }} />
        </Form.Item>

        <Divider style={{ margin: '12px 0' }} />

        <Form.Item
          name="reflectMinChars"
          label="Reflection: min chars"
          tooltip="Skip auto-reflection for messages shorter than this (typically chit-chat)."
          rules={[{ required: true, type: 'number', min: 0, max: 10000 }]}
        >
          <InputNumber min={0} max={10000} step={10} style={{ width: 200 }} />
        </Form.Item>

        <Form.Item
          name="reflectMaxChars"
          label="Reflection: max chars"
          tooltip="Truncate auto-reflection input above this length. Prevents long pastes from dominating."
          rules={[{ required: true, type: 'number', min: 100, max: 100000 }]}
        >
          <InputNumber min={100} max={100000} step={100} style={{ width: 200 }} />
        </Form.Item>

        <Form.Item
          name="reflectCooldownMs"
          label="Reflection cooldown (ms)"
          tooltip="Minimum gap between auto-reflections in the same group. 0 = no throttle."
          rules={[{ required: true, type: 'number', min: 0, max: 600000 }]}
        >
          <InputNumber min={0} max={600000} step={500} style={{ width: 200 }} />
        </Form.Item>

        <Divider style={{ margin: '12px 0' }} />

        <Form.Item
          name="maintenanceIntervalHours"
          label="Maintenance sweep interval (hours)"
          tooltip="Cadence for the periodic janitor: drops envelope-wrapped chunks, removes orphan entities, merges duplicate entities sharing a normalised name, and infers associative links between entities that co-occur across chunks. 0 disables the sweep (you can still trigger it manually)."
          rules={[{ required: true, type: 'number', min: 0, max: 720 }]}
        >
          <InputNumber min={0} max={720} step={1} style={{ width: 200 }} addonAfter="h" />
        </Form.Item>

        <div style={{ marginBottom: 12 }}>
          <Button
            icon={<ClearOutlined />}
            loading={maintRunning}
            onClick={runMaintenance}
          >
            Run maintenance now
          </Button>
          <Text type="secondary" style={{ marginLeft: 12, fontSize: 12 }}>
            Runs cleanup + merge once, immediately. Independent of the schedule.
          </Text>
        </div>

        <Space style={{ marginTop: 16 }}>
          <Button
            type="primary"
            icon={<SaveOutlined />}
            loading={saving}
            disabled={!dirty}
            onClick={handleSave}
          >
            Save
          </Button>
          <Button icon={<ReloadOutlined />} onClick={load} disabled={saving}>
            Reload
          </Button>
        </Space>
      </Form>

      {effective && (
        <>
          <Divider />
          <Title level={5}>Live daemon (effective)</Title>
          <Space wrap>
            <Tag color={effective.enabled ? 'green' : 'default'}>
              enabled: {String(effective.enabled)}
            </Tag>
            <Tag>maxConcurrent: {effective.maxConcurrent}</Tag>
            <Tag>maxOutputChars: {effective.maxOutputChars}</Tag>
            <Tag>reflectMinChars: {effective.reflectMinChars}</Tag>
            <Tag>reflectMaxChars: {effective.reflectMaxChars}</Tag>
            <Tag>reflectCooldownMs: {effective.reflectCooldownMs}</Tag>
            <Tag color={effective.autoReflection ? 'blue' : 'default'}>
              autoReflection: {String(effective.autoReflection)}
            </Tag>
            <Tag color={effective.maintenanceIntervalHours > 0 ? 'purple' : 'default'}>
              maintenanceIntervalHours: {effective.maintenanceIntervalHours}
            </Tag>
          </Space>
          <Text type="secondary" style={{ display: 'block', marginTop: 12, fontSize: 12 }}>
            These are the values the daemon is using right now. Saved values
            above only take effect after a restart.
          </Text>
        </>
      )}
    </div>
  );
};
