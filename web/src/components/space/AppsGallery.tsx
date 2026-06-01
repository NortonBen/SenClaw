import React, { useEffect, useState } from 'react';
import {
  Card, Button, Tag, Empty, Modal, Form, Input, Typography, theme,
  Tooltip, Popconfirm, Alert, message, Upload,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, AppstoreOutlined, LinkOutlined, InfoCircleOutlined,
} from '@ant-design/icons';
import { SpaceAppDetailModal } from './SpaceAppDetailModal';

const { Paragraph } = Typography;

interface SpaceApp {
  id: string;
  name: string;
  description?: string;
  icon?: string;
  integration: { type: 'iframe' | 'esm'; url: string };
  enabled: boolean;
  manifest?: any;
}

const DEMO_APPS: SpaceApp[] = [];

function normalizeApp(row: { id: string; manifest: any; enabled: boolean }): SpaceApp {
  return {
    id: row.id,
    name: row.manifest?.name ?? row.id,
    description: row.manifest?.description,
    icon: row.manifest?.icon,
    integration: row.manifest?.integration ?? { type: 'iframe', url: row.manifest?.url ?? '#' },
    enabled: row.enabled,
    manifest: row.manifest,
  };
}

interface Props {
  groupFolder: string;
  onAppsChanged?: () => void;
  onOpenApp?: (appId: string) => void;
}

export function AppsGallery({ groupFolder, onAppsChanged, onOpenApp }: Props) {
  const { token } = theme.useToken();
  const [apps, setApps] = useState<SpaceApp[]>(DEMO_APPS);
  const [showRegister, setShowRegister] = useState(false);
  const [registering, setRegistering] = useState(false);
  const [installingZip, setInstallingZip] = useState(false);
  const [detailApp, setDetailApp] = useState<SpaceApp | null>(null);
  const [form] = Form.useForm();

  useEffect(() => {
    fetch('/api/space/apps')
      .then(r => r.ok ? r.json() : [])
      .then((rows: Array<{ id: string; manifest: any; enabled: boolean }>) => {
        const loaded = rows.map(normalizeApp);
        setApps(loaded);
      })
      .catch(() => {});
  }, []);

  const handleRegister = async () => {
    try {
      const vals = await form.validateFields();
      setRegistering(true);
      const res = await fetch('/api/space/apps/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ manifest_url: vals.manifest_url }),
      });
      if (res.ok) {
        const row = await res.json() as { id: string; manifest: any; enabled: boolean };
        const app = normalizeApp(row);
        setApps(prev => [...prev.filter(a => a.id !== app.id), app]);
        setShowRegister(false);
        form.resetFields();
        onAppsChanged?.();
      }
    } catch {
      // validation error
    } finally {
      setRegistering(false);
    }
  };

  const handleRemove = (id: string) => {
    setApps(prev => prev.filter(a => a.id !== id));
    fetch(`/api/space/apps/${id}`, { method: 'DELETE' })
      .then(() => onAppsChanged?.())
      .catch(() => {});
  };

  const installZip = async (file: File) => {
    setInstallingZip(true);
    try {
      const formData = new FormData();
      formData.append('file', file);
      const res = await fetch('/api/space/apps/install-zip', {
        method: 'POST',
        body: formData,
      });
      if (!res.ok) throw new Error(await res.text());
      const row = await res.json() as { id: string; manifest: any; enabled: boolean };
      const app = normalizeApp(row);
      setApps(prev => [...prev.filter(a => a.id !== app.id), app]);
      onAppsChanged?.();
      message.success(`${app.name} installed`);
      onOpenApp?.(app.id);
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Install zip failed');
    } finally {
      setInstallingZip(false);
    }
  };

  return (
    <div className="flex flex-col h-full">
      {/* Toolbar */}
      <div
        className="flex items-center gap-2 px-4 py-2 border-b flex-shrink-0"
        style={{ borderColor: token.colorBorderSecondary }}
      >
        <span className="font-semibold text-sm flex-1" style={{ color: token.colorText }}>
          Micro-Frontend Apps
        </span>
        <Button
          type="primary"
          size="small"
          icon={<PlusOutlined />}
          onClick={() => setShowRegister(true)}
        >
          Đăng ký App
        </Button>
        <Upload
          accept=".zip"
          showUploadList={false}
          beforeUpload={file => {
            installZip(file);
            return false;
          }}
        >
          <Button size="small" loading={installingZip}>
            Cài từ ZIP
          </Button>
        </Upload>
      </div>

      <div className="flex-1 overflow-y-auto px-4 py-3">
        <Alert
          type="info"
          showIcon
          className="mb-4"
          title="Space Apps"
          description="Kết nối dịch vụ năng suất như Google Workspace hoặc nhúng micro-frontend app qua manifest."
        />

        {apps.length === 0 && (
          <Empty
            image={<AppstoreOutlined style={{ fontSize: 48, color: token.colorTextQuaternary }} />}
            description={
              <span>
                Chưa có app nào.{' '}
                <Button type="link" size="small" onClick={() => setShowRegister(true)}>
                  Đăng ký app đầu tiên
                </Button>
              </span>
            }
            className="py-8"
          />
        )}

        <div className="grid grid-cols-2 gap-3">
          {apps.map(app => (
            <Card
              key={app.id}
              size="small"
              hoverable
              extra={
                <Popconfirm
                  title="Gỡ app này?"
                  onConfirm={() => handleRemove(app.id)}
                  okText="Gỡ"
                  cancelText="Hủy"
                >
                  <Button type="text" size="small" danger icon={<DeleteOutlined />} />
                </Popconfirm>
              }
              title={
                <div className="flex items-center gap-2">
                  <span>{app.icon ?? '🔌'}</span>
                  <span>{app.name}</span>
                </div>
              }
            >
              <Paragraph type="secondary" className="text-xs mb-2" ellipsis={{ rows: 2 }}>
                {app.description ?? '—'}
              </Paragraph>
              <div className="flex items-center justify-between">
                <Tag color={app.integration.type === 'iframe' ? 'blue' : 'purple'}>
                  {app.integration.type}
                </Tag>
                <div className="flex items-center">
                  <Button
                    type="link"
                    size="small"
                    icon={<InfoCircleOutlined />}
                    onClick={() => setDetailApp(app)}
                  >
                    Chi tiết
                  </Button>
                  <Tooltip title={app.integration.url}>
                    <Button
                      type="link"
                      size="small"
                      icon={<LinkOutlined />}
                      onClick={() => onOpenApp?.(app.id)}
                    >
                      Mở
                    </Button>
                  </Tooltip>
                </div>
              </div>
            </Card>
          ))}
        </div>
      </div>

      {/* Register modal */}
      <Modal
        title="Đăng ký Micro-Frontend App"
        open={showRegister}
        onCancel={() => { setShowRegister(false); form.resetFields(); }}
        onOk={handleRegister}
        okText="Đăng ký"
        cancelText="Hủy"
        confirmLoading={registering}
      >
        <Alert
          type="warning"
          className="mb-3"
          showIcon
          title="App sẽ được nhúng qua iframe — đảm bảo tin tưởng nguồn gốc trước khi đăng ký."
        />
        <Form form={form} layout="vertical">
          <Form.Item
            name="manifest_url"
            label="URL Manifest"
            tooltip="App cần serve file senclaw-manifest.json tại endpoint này"
            rules={[{ required: true, type: 'url', message: 'Nhập URL hợp lệ' }]}
          >
            <Input
              placeholder="http://localhost:3100/senclaw-manifest.json"
              prefix={<LinkOutlined />}
            />
          </Form.Item>
        </Form>
      </Modal>

      <SpaceAppDetailModal
        app={detailApp}
        open={!!detailApp}
        onClose={() => setDetailApp(null)}
      />
    </div>
  );
}
