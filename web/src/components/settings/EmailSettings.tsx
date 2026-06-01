import React, { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Form,
  Input,
  InputNumber,
  message,
  Popconfirm,
  Space,
  Spin,
  Switch,
  Tag,
  Typography,
  theme,
} from 'antd';
import { DeleteOutlined, MailOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import type { SpaceEmailAccount, SpaceEmailAccountCreate } from '../../hooks/useSpace';

const { Title, Text, Paragraph } = Typography;

async function apiFetch<T>(path: string, opts?: RequestInit): Promise<T> {
  const res = await fetch(path, { headers: { 'Content-Type': 'application/json' }, ...opts });
  if (!res.ok) {
    const text = await res.text().catch(() => '');
    throw new Error(text || `${res.status} ${res.statusText}`);
  }
  return res.json() as Promise<T>;
}

export const EmailSettings: React.FC = () => {
  const { token } = theme.useToken();
  const [form] = Form.useForm<SpaceEmailAccountCreate>();
  const [accounts, setAccounts] = useState<SpaceEmailAccount[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [showForm, setShowForm] = useState(false);

  const loadAccounts = async () => {
    setLoading(true);
    try {
      const data = await apiFetch<SpaceEmailAccount[]>('/api/space/email/accounts');
      setAccounts(Array.isArray(data) ? data : []);
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Failed to load email accounts');
      setAccounts([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadAccounts();
  }, []);

  const addAccount = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      await apiFetch('/api/space/email/accounts', {
        method: 'POST',
        body: JSON.stringify(values),
      });
      message.success('Email account saved');
      form.resetFields();
      setShowForm(false);
      await loadAccounts();
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Failed to save email account');
    } finally {
      setSaving(false);
    }
  };

  const deleteAccount = async (id: string) => {
    try {
      await apiFetch(`/api/space/email/accounts/${encodeURIComponent(id)}`, { method: 'DELETE' });
      message.success('Email account removed');
      setAccounts(prev => prev.filter(a => a.id !== id));
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Failed to remove email account');
    }
  };

  return (
    <div style={{ maxWidth: 900 }}>
      <Space style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Space Email</Title>
          <Text type="secondary">Manage IMAP/SMTP accounts used by the Space email inbox and composer.</Text>
        </div>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={loadAccounts} loading={loading}>
            Refresh
          </Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setShowForm(v => !v)}>
            Add account
          </Button>
        </Space>
      </Space>

      <Alert
        type="warning"
        showIcon
        style={{ marginBottom: 16 }}
        message="Email transport is still in MVP mode"
        description="Accounts can be configured and used by the Space UI. Real IMAP sync and SMTP delivery still need the transport layer; current send flow records the outgoing email locally."
      />

      {showForm && (
        <Card
          size="small"
          style={{ marginBottom: 16, borderColor: token.colorBorderSecondary }}
          styles={{ body: { padding: 16 } }}
        >
          <Form
            form={form}
            layout="vertical"
            initialValues={{ imap_port: 993, smtp_port: 587, use_tls: true }}
          >
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 12 }}>
              <Form.Item name="label" label="Label" rules={[{ required: true, message: 'Enter a label' }]}>
                <Input placeholder="Work Gmail" />
              </Form.Item>
              <Form.Item name="email" label="Email" rules={[{ required: true, type: 'email', message: 'Enter a valid email' }]}>
                <Input placeholder="you@example.com" />
              </Form.Item>
              <Form.Item name="imap_host" label="IMAP host" rules={[{ required: true, message: 'Enter IMAP host' }]}>
                <Input placeholder="imap.gmail.com" />
              </Form.Item>
              <Form.Item name="imap_port" label="IMAP port" rules={[{ required: true, message: 'Enter IMAP port' }]}>
                <InputNumber min={1} max={65535} style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item name="smtp_host" label="SMTP host" rules={[{ required: true, message: 'Enter SMTP host' }]}>
                <Input placeholder="smtp.gmail.com" />
              </Form.Item>
              <Form.Item name="smtp_port" label="SMTP port" rules={[{ required: true, message: 'Enter SMTP port' }]}>
                <InputNumber min={1} max={65535} style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item name="username" label="Username" rules={[{ required: true, message: 'Enter username' }]}>
                <Input placeholder="usually your email address" />
              </Form.Item>
              <Form.Item name="password" label="Password / app password" rules={[{ required: true, message: 'Enter password' }]}>
                <Input.Password placeholder="app-specific password recommended" />
              </Form.Item>
            </div>
            <Form.Item name="use_tls" label="TLS" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Space>
              <Button onClick={() => setShowForm(false)}>Cancel</Button>
              <Button type="primary" loading={saving} onClick={addAccount}>Save account</Button>
            </Space>
          </Form>
        </Card>
      )}

      {loading ? (
        <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}>
          <Spin />
        </div>
      ) : accounts.length === 0 ? (
        <Card style={{ borderColor: token.colorBorderSecondary }}>
          <Space align="start">
            <MailOutlined style={{ fontSize: 22, color: token.colorTextQuaternary }} />
            <div>
              <Text strong>No email accounts configured</Text>
              <Paragraph type="secondary" style={{ margin: '4px 0 0' }}>
                Add an IMAP/SMTP account so Space can show an inbox and prepare outgoing messages.
              </Paragraph>
            </div>
          </Space>
        </Card>
      ) : (
        <Space direction="vertical" style={{ width: '100%' }} size="middle">
          {accounts.map(account => (
            <Card
              key={account.id}
              size="small"
              style={{ borderColor: token.colorBorderSecondary }}
              styles={{ body: { padding: 16 } }}
            >
              <Space style={{ width: '100%', justifyContent: 'space-between' }} align="start">
                <Space align="start">
                  <MailOutlined style={{ fontSize: 20, color: token.colorPrimary, marginTop: 2 }} />
                  <div>
                    <Space size={8} wrap>
                      <Text strong>{account.label}</Text>
                      <Tag>{account.email}</Tag>
                      {account.use_tls && <Tag color="green">TLS</Tag>}
                    </Space>
                    <Paragraph type="secondary" style={{ margin: '6px 0 0', fontSize: 12 }}>
                      IMAP {account.imap_host}:{account.imap_port} · SMTP {account.smtp_host}:{account.smtp_port}
                    </Paragraph>
                  </div>
                </Space>
                <Popconfirm
                  title="Delete this email account?"
                  description="Cached messages for this account will also be removed."
                  okText="Delete"
                  okButtonProps={{ danger: true }}
                  onConfirm={() => deleteAccount(account.id)}
                >
                  <Button danger type="text" icon={<DeleteOutlined />} />
                </Popconfirm>
              </Space>
            </Card>
          ))}
        </Space>
      )}
    </div>
  );
};
