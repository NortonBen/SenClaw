import React, { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  Form,
  Input,
  message,
  Popconfirm,
  Space,
  Tag,
  Typography,
  Upload,
  theme,
} from 'antd';
import { AppstoreOutlined, DeleteOutlined, LinkOutlined, UploadOutlined } from '@ant-design/icons';

const { Title, Text, Paragraph } = Typography;

interface SpaceAppRow {
  id: string;
  manifest: any;
  enabled: boolean;
  installed_at: number;
}

export const SpaceAppsSettings: React.FC = () => {
  const { token } = theme.useToken();
  const [apps, setApps] = useState<SpaceAppRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [registering, setRegistering] = useState(false);
  const [form] = Form.useForm();

  const loadApps = async () => {
    setLoading(true);
    try {
      const data = await fetch('/api/space/apps').then(r => r.ok ? r.json() : []);
      setApps(Array.isArray(data) ? data : []);
    } catch {
      setApps([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadApps();
  }, []);

  const installZip = async (file: File) => {
    setInstalling(true);
    try {
      const formData = new FormData();
      formData.append('file', file);
      const res = await fetch('/api/space/apps/install-zip', { method: 'POST', body: formData });
      if (!res.ok) throw new Error(await res.text());
      message.success('Space App installed');
      await loadApps();
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Install failed');
    } finally {
      setInstalling(false);
    }
  };

  const registerManifest = async () => {
    const values = await form.validateFields();
    setRegistering(true);
    try {
      const res = await fetch('/api/space/apps/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ manifest_url: values.manifest_url }),
      });
      if (!res.ok) throw new Error(await res.text());
      message.success('Space App registered');
      form.resetFields();
      await loadApps();
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Register failed');
    } finally {
      setRegistering(false);
    }
  };

  const uninstall = async (id: string) => {
    try {
      const res = await fetch(`/api/space/apps/${encodeURIComponent(id)}`, { method: 'DELETE' });
      if (!res.ok) throw new Error(await res.text());
      message.success('Space App uninstalled');
      setApps(prev => prev.filter(app => app.id !== id));
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Uninstall failed');
    }
  };

  return (
    <div style={{ maxWidth: 980 }}>
      <Space style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Space Apps</Title>
          <Text type="secondary">Install, register, and remove embedded Space Apps.</Text>
        </div>
        <Upload
          accept=".zip"
          showUploadList={false}
          beforeUpload={file => {
            installZip(file);
            return false;
          }}
        >
          <Button type="primary" icon={<UploadOutlined />} loading={installing}>
            Install ZIP
          </Button>
        </Upload>
      </Space>

      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="Space App package contract"
        description="A ZIP app must contain senclaw-manifest.json or senclaw-app.json at the archive root. Static Next.js exports are served from /api/space/apps/:id/static/index.html and appear as child items under Apps in the Space sidebar."
      />

      <Card size="small" style={{ marginBottom: 16, borderColor: token.colorBorderSecondary }}>
        <Form form={form} layout="inline" style={{ gap: 8 }}>
          <Form.Item
            name="manifest_url"
            rules={[{ required: true, type: 'url', message: 'Enter a manifest URL' }]}
            style={{ flex: 1, marginBottom: 0 }}
          >
            <Input prefix={<LinkOutlined />} placeholder="https://app.example.com/senclaw-manifest.json" />
          </Form.Item>
          <Button onClick={registerManifest} loading={registering}>
            Register URL
          </Button>
        </Form>
      </Card>

      <Space direction="vertical" style={{ width: '100%' }} size="middle">
        {apps.map(app => {
          const manifest = app.manifest ?? {};
          const integration = manifest.integration ?? {};
          return (
            <Card
              key={app.id}
              size="small"
              style={{ borderColor: token.colorBorderSecondary }}
              title={
                <Space>
                  <AppstoreOutlined />
                  <span>{manifest.name ?? app.id}</span>
                  <Tag>{app.id}</Tag>
                  {manifest.install?.type === 'zip' && <Tag color="green">ZIP</Tag>}
                </Space>
              }
              extra={
                <Popconfirm
                  title="Uninstall this Space App?"
                  description="Local files installed from ZIP will be removed."
                  okText="Uninstall"
                  okButtonProps={{ danger: true }}
                  onConfirm={() => uninstall(app.id)}
                >
                  <Button danger type="text" icon={<DeleteOutlined />} />
                </Popconfirm>
              }
            >
              <Paragraph type="secondary" style={{ marginBottom: 8 }}>
                {manifest.description ?? 'No description'}
              </Paragraph>
              <Space wrap>
                <Tag color={integration.type === 'iframe' ? 'blue' : 'purple'}>
                  {integration.type ?? 'iframe'}
                </Tag>
                <Tag>{integration.url ?? 'no url'}</Tag>
                {manifest.bridge?.postMessage && <Tag color="cyan">SemaClaw bridge</Tag>}
              </Space>
            </Card>
          );
        })}
        {!loading && apps.length === 0 && (
          <Card style={{ borderColor: token.colorBorderSecondary }}>
            <Text type="secondary">No Space Apps installed.</Text>
          </Card>
        )}
      </Space>
    </div>
  );
};
