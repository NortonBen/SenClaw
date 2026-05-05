import React, { useState } from 'react';
import {
  Card, Button, Tag, Empty, Modal, Form, Input, Typography, theme,
  Tooltip, Popconfirm, Alert,
} from 'antd';
import { PlusOutlined, DeleteOutlined, AppstoreOutlined, LinkOutlined } from '@ant-design/icons';

const { Text, Paragraph } = Typography;

interface SpaceApp {
  id: string;
  name: string;
  description?: string;
  icon?: string;
  integration: { type: 'iframe' | 'esm'; url: string };
  enabled: boolean;
}

// Hardcoded sample apps — real data will come from /api/space/apps
const DEMO_APPS: SpaceApp[] = [];

interface Props {
  groupFolder: string;
}

export function AppsGallery({ groupFolder }: Props) {
  const { token } = theme.useToken();
  const [apps, setApps] = useState<SpaceApp[]>(DEMO_APPS);
  const [showRegister, setShowRegister] = useState(false);
  const [registering, setRegistering] = useState(false);
  const [form] = Form.useForm();

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
        const app = await res.json() as SpaceApp;
        setApps(prev => [...prev, app]);
        setShowRegister(false);
        form.resetFields();
      }
    } catch {
      // validation error
    } finally {
      setRegistering(false);
    }
  };

  const handleRemove = (id: string) => {
    setApps(prev => prev.filter(a => a.id !== id));
    fetch(`/api/space/apps/${id}`, { method: 'DELETE' }).catch(() => {});
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
      </div>

      <div className="flex-1 overflow-y-auto px-4 py-3">
        <Alert
          type="info"
          showIcon
          className="mb-4"
          title="Micro-Frontend Platform (Phase 4)"
          description="Tích hợp NestJS apps, React modules hoặc bất kỳ web app nào vào Space qua iframe hoặc ESM remote. Mỗi app có thể expose MCP tools để agent sử dụng."
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
                <Tooltip title={app.integration.url}>
                  <Button
                    type="link"
                    size="small"
                    icon={<LinkOutlined />}
                    href={app.integration.url}
                    target="_blank"
                  >
                    Mở
                  </Button>
                </Tooltip>
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
    </div>
  );
}
