import { useEffect, useState } from 'react';
import {
  Alert, App, Button, Card, Form, Input, InputNumber,
  Popconfirm, Space, Spin, Switch, Tag, Typography, theme,
} from 'antd';
import { DeleteOutlined, MailOutlined, PlusOutlined, ReloadOutlined } from '@ant-design/icons';
import { api, type Account, type AccountCreate } from '../api';

const { Title, Text, Paragraph } = Typography;

export function AccountsView() {
  const { token } = theme.useToken();
  const { message } = App.useApp();
  const [form] = Form.useForm<AccountCreate>();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [showForm, setShowForm] = useState(false);

  const loadAccounts = async () => {
    setLoading(true);
    try {
      const data = await api.listAccounts();
      setAccounts(Array.isArray(data) ? data : []);
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Không tải được tài khoản');
      setAccounts([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { loadAccounts(); }, []);

  const addAccount = async () => {
    const values = await form.validateFields();
    setSaving(true);
    try {
      await api.createAccount(values);
      message.success('Đã lưu tài khoản');
      form.resetFields();
      setShowForm(false);
      await loadAccounts();
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Lưu tài khoản thất bại');
    } finally {
      setSaving(false);
    }
  };

  const deleteAccount = async (id: string) => {
    try {
      await api.deleteAccount(id);
      message.success('Đã xóa tài khoản');
      setAccounts(prev => prev.filter(a => a.id !== id));
    } catch (err) {
      message.error(err instanceof Error ? err.message : 'Xóa tài khoản thất bại');
    }
  };

  return (
    <div style={{ maxWidth: 900, margin: '0 auto', padding: 24 }}>
      <Space style={{ width: '100%', justifyContent: 'space-between', marginBottom: 16 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Tài khoản Email</Title>
          <Text type="secondary">Quản lý tài khoản IMAP/SMTP dùng cho hộp thư và trình soạn thảo.</Text>
        </div>
        <Space>
          <Button icon={<ReloadOutlined />} onClick={loadAccounts} loading={loading}>Làm mới</Button>
          <Button type="primary" icon={<PlusOutlined />} onClick={() => setShowForm(v => !v)}>Thêm tài khoản</Button>
        </Space>
      </Space>

      <Alert
        type="info"
        showIcon
        style={{ marginBottom: 16 }}
        message="Gợi ý cấu hình"
        description="Với Gmail: IMAP imap.gmail.com:993, SMTP smtp.gmail.com:587, bật TLS, và dùng App Password thay cho mật khẩu thường."
      />

      {showForm && (
        <Card size="small" style={{ marginBottom: 16, borderColor: token.colorBorderSecondary }} styles={{ body: { padding: 16 } }}>
          <Form form={form} layout="vertical" initialValues={{ imap_port: 993, smtp_port: 587, use_tls: true }}>
            <div style={{ display: 'grid', gridTemplateColumns: 'repeat(2, minmax(0, 1fr))', gap: 12 }}>
              <Form.Item name="label" label="Nhãn" rules={[{ required: true, message: 'Nhập nhãn' }]}>
                <Input placeholder="Work Gmail" />
              </Form.Item>
              <Form.Item name="email" label="Email" rules={[{ required: true, type: 'email', message: 'Nhập email hợp lệ' }]}>
                <Input placeholder="you@example.com" />
              </Form.Item>
              <Form.Item name="imap_host" label="IMAP host" rules={[{ required: true, message: 'Nhập IMAP host' }]}>
                <Input placeholder="imap.gmail.com" />
              </Form.Item>
              <Form.Item name="imap_port" label="IMAP port" rules={[{ required: true, message: 'Nhập IMAP port' }]}>
                <InputNumber min={1} max={65535} style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item name="smtp_host" label="SMTP host" rules={[{ required: true, message: 'Nhập SMTP host' }]}>
                <Input placeholder="smtp.gmail.com" />
              </Form.Item>
              <Form.Item name="smtp_port" label="SMTP port" rules={[{ required: true, message: 'Nhập SMTP port' }]}>
                <InputNumber min={1} max={65535} style={{ width: '100%' }} />
              </Form.Item>
              <Form.Item name="username" label="Username" rules={[{ required: true, message: 'Nhập username' }]}>
                <Input placeholder="thường là địa chỉ email" />
              </Form.Item>
              <Form.Item name="password" label="Mật khẩu / App password" rules={[{ required: true, message: 'Nhập mật khẩu' }]}>
                <Input.Password placeholder="khuyến nghị dùng app password" />
              </Form.Item>
            </div>
            <Form.Item name="use_tls" label="TLS" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Space>
              <Button onClick={() => setShowForm(false)}>Hủy</Button>
              <Button type="primary" loading={saving} onClick={addAccount}>Lưu tài khoản</Button>
            </Space>
          </Form>
        </Card>
      )}

      {loading ? (
        <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}><Spin /></div>
      ) : accounts.length === 0 ? (
        <Card style={{ borderColor: token.colorBorderSecondary }}>
          <Space align="start">
            <MailOutlined style={{ fontSize: 22, color: token.colorTextQuaternary }} />
            <div>
              <Text strong>Chưa có tài khoản email</Text>
              <Paragraph type="secondary" style={{ margin: '4px 0 0' }}>
                Thêm tài khoản IMAP/SMTP để hiển thị hộp thư và soạn email.
              </Paragraph>
            </div>
          </Space>
        </Card>
      ) : (
        <Space direction="vertical" style={{ width: '100%' }} size="middle">
          {accounts.map(account => (
            <Card key={account.id} size="small" style={{ borderColor: token.colorBorderSecondary }} styles={{ body: { padding: 16 } }}>
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
                  title="Xóa tài khoản này?"
                  description="Email đã cache của tài khoản cũng sẽ bị xóa."
                  okText="Xóa"
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
}
