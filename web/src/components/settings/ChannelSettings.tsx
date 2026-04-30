import React, { useState } from 'react';
import {
  Typography, Button, Card, Space, Table, Tag, Modal,
  Form, Input, Select, Popconfirm, message, Tooltip, Switch, Divider, theme, Flex, Alert, Spin,
} from 'antd';
import {
  PlusOutlined, DeleteOutlined, EditOutlined,
  CheckCircleOutlined, CloseCircleOutlined, SyncOutlined, MobileOutlined, QrcodeOutlined, CopyOutlined,
} from '@ant-design/icons';
import { QRCodeSVG } from 'qrcode.react';
import type { ChannelInfo, RegisterChannelPayload, UpdateChannelPayload } from '../../types';

const { Title, Text, Paragraph } = Typography;
const { Option } = Select;

// ===== Types =====

type Platform = 'telegram' | 'feishu' | 'qq' | 'wechat' | 'senclaw';

const PLATFORM_LABELS: Record<string, string> = {
  telegram: 'Telegram',
  feishu: 'Feishu / Lark',
  qq: 'QQ',
  wechat: 'WeChat',
  senclaw: 'Senclaw Connector',
  web: 'Web',
};

const PLATFORM_COLOR: Record<string, string> = {
  telegram: 'blue',
  feishu: 'cyan',
  qq: 'purple',
  wechat: 'green',
  senclaw: 'geekblue',
  web: 'default',
};

// ===== Credential builders / parsers =====

function buildCredentials(platform: Platform, fields: any): Record<string, unknown> {
  switch (platform) {
    case 'senclaw':
      return {
        hubUrl: fields.hubUrl?.trim() ?? 'https://hub.senclaw.ai',
        // Preserve hidden fields if they are passed in (e.g. from editing form merge)
        ...(fields.channelId ? { channelId: fields.channelId } : {}),
        ...(fields.encryptionKey ? { encryptionKey: fields.encryptionKey } : {}),
        ...(fields.accessToken ? { accessToken: fields.accessToken } : {}),
      };
    case 'telegram':
      return {
        ...(fields.botToken?.trim() ? { botToken: fields.botToken.trim() } : {}),
        chatType: fields.chatType ?? 'group',
        requiresTrigger: fields.requiresTrigger ?? false,
      };
    case 'feishu':
      return {
        ...(fields.chatJid?.trim() ? { chatJid: fields.chatJid.trim() } : {}),
        ...(fields.appId?.trim() ? { appId: fields.appId.trim() } : {}),
        ...(fields.appSecret?.trim() ? { appSecret: fields.appSecret.trim() } : {}),
        requiresTrigger: fields.requiresTrigger ?? false,
      };
    case 'qq':
      return {
        appId: fields.appId?.trim() ?? '',
        appSecret: fields.appSecret?.trim() ?? '',
        sandbox: fields.sandbox ?? false,
      };
    case 'wechat':
      return {
        ...(fields.appId?.trim() ? { appId: fields.appId.trim() } : {}),
        ...(fields.appSecret?.trim() ? { appSecret: fields.appSecret.trim() } : {}),
        ...(fields.token?.trim() ? { token: fields.token.trim() } : {}),
      };
    default:
      return {};
  }
}

function parseCredentials(credJson: string): Record<string, any> {
  try { return JSON.parse(credJson); } catch { return {}; }
}

// ===== Platform-specific form fields =====

function TelegramFields() {
  return (
    <>
      <Form.Item name="botToken" label="Bot Token" extra="Leave empty to use the global default bot from .env">
        <Input.Password placeholder="123456:ABCdef..." style={{ fontFamily: 'monospace' }} />
      </Form.Item>
      <Form.Item name="chatType" label="Chat Type">
        <Select>
          <Option value="group">Group</Option>
          <Option value="user">User (DM)</Option>
        </Select>
      </Form.Item>
      <Form.Item name="requiresTrigger" label="Require @mention to trigger" valuePropName="checked">
        <Switch size="small" />
      </Form.Item>
    </>
  );
}

function FeishuFields() {
  return (
    <>
      <Form.Item
        name="chatJid"
        label="Chat JID"
        extra="Optional — auto-binding completes after bot receives first message"
      >
        <Input placeholder="feishu:group:oc_xxx or feishu:user:ou_xxx" style={{ fontFamily: 'monospace' }} />
      </Form.Item>
      <Form.Item name="requiresTrigger" label="Require @mention to trigger" valuePropName="checked">
        <Switch size="small" />
      </Form.Item>
      <Divider style={{ margin: '8px 0' }}>
        <Text type="secondary" style={{ fontSize: 11 }}>App Credentials (optional — uses global default if empty)</Text>
      </Divider>
      <Flex gap={12}>
        <Form.Item name="appId" label="App ID" style={{ flex: 1 }}>
          <Input placeholder="cli_xxx" style={{ fontFamily: 'monospace' }} />
        </Form.Item>
        <Form.Item name="appSecret" label="App Secret" style={{ flex: 1 }}>
          <Input.Password placeholder="Required when App ID is set" />
        </Form.Item>
      </Flex>
    </>
  );
}

function QQFields() {
  return (
    <>
      <Flex gap={12}>
        <Form.Item name="appId" label="App ID" rules={[{ required: true, message: 'Required' }]} style={{ flex: 1 }}>
          <Input placeholder="QQ Open Platform AppID" style={{ fontFamily: 'monospace' }} />
        </Form.Item>
        <Form.Item name="appSecret" label="App Secret" rules={[{ required: true, message: 'Required' }]} style={{ flex: 1 }}>
          <Input.Password placeholder="QQ Open Platform AppSecret" />
        </Form.Item>
      </Flex>
      <Form.Item name="sandbox" label="Sandbox Mode" valuePropName="checked">
        <Switch size="small" />
      </Form.Item>
    </>
  );
}

function SenclawFields() {
  return (
    <>
      <Alert
        type="info"
        showIcon
        icon={<MobileOutlined />}
        message="QR Pairing"
        description="After registering, a QR code will appear. Scan it with the Senclaw mobile app to establish an end-to-end encrypted connection via ClawHub relay."
        style={{ marginBottom: 16 }}
      />
      <Form.Item
        name="hubUrl"
        label="ClawHub Relay URL"
        extra="Local hub URL for testing"
        initialValue="http://localhost:18080"
      >
        <Input placeholder="http://localhost:18080" style={{ fontFamily: 'monospace' }} />
      </Form.Item>
    </>
  );
}

// ===== QR Pairing Modal =====

interface QRPairingData {
  channelId: string;
  encryptionKey: string;
  hubUrl: string;
  token: string;
}

function QRPairingModal({ data, onClose }: { data: QRPairingData; onClose: () => void }) {
  const { token } = theme.useToken();
  // QR payload: semaclaw://connect protocol that the mobile app will parse
  const qrPayload = `semaclaw://connect?hub=${encodeURIComponent(data.hubUrl)}&cid=${encodeURIComponent(data.channelId)}&key=${encodeURIComponent(data.encryptionKey)}&token=${encodeURIComponent(data.token)}`;

  const copyToClipboard = () => {
    navigator.clipboard.writeText(qrPayload);
    message.success('Pairing data copied');
  };

  return (
    <Modal
      title={<Space><QrcodeOutlined />Scan to Connect</Space>}
      open
      onCancel={onClose}
      footer={<Button type="primary" onClick={onClose}>Done</Button>}
      width={420}
      centered
    >
      <Flex vertical align="center" gap={20} style={{ padding: '16px 0' }}>
        {/* QR Code */}
        <div style={{
          padding: 16,
          background: '#fff',
          borderRadius: 12,
          border: `1px solid ${token.colorBorderSecondary}`,
          boxShadow: token.boxShadowSecondary,
        }}>
          <QRCodeSVG value={qrPayload} size={220} level="M" />
        </div>

        {/* Steps */}
        <div style={{ width: '100%' }}>
          <Text type="secondary" style={{ fontSize: 12, display: 'block', marginBottom: 8 }}>
            How to connect:
          </Text>
          {[
            'Open the Senclaw app on your mobile device',
            'Tap  +  →  Scan QR  to pair a new agent',
            'Point the camera at the QR code above',
          ].map((step, i) => (
            <Flex key={i} gap={8} align="flex-start" style={{ marginBottom: 6 }}>
              <div style={{
                width: 18, height: 18, borderRadius: '50%', flexShrink: 0,
                background: token.colorPrimary,
                display: 'flex', alignItems: 'center', justifyContent: 'center',
                marginTop: 1,
              }}>
                <span style={{ color: '#fff', fontSize: 10, fontWeight: 700 }}>{i + 1}</span>
              </div>
              <Text style={{ fontSize: 13 }}>{step}</Text>
            </Flex>
          ))}
        </div>

        {/* Pairing details */}
        <Card
          size="small"
          style={{ width: '100%', background: token.colorFillAlter, border: `1px solid ${token.colorBorderSecondary}` }}
          styles={{ body: { padding: '10px 14px' } }}
        >
          <Flex justify="space-between" align="center" style={{ marginBottom: 4 }}>
            <Text type="secondary" style={{ fontSize: 11, textTransform: 'uppercase', letterSpacing: 1 }}>
              Pairing Info
            </Text>
            <Button type="text" size="small" icon={<CopyOutlined />} onClick={copyToClipboard} style={{ fontSize: 11 }}>
              Copy
            </Button>
          </Flex>
          <Text style={{ fontSize: 11, fontFamily: 'monospace', wordBreak: 'break-all', color: token.colorTextSecondary }}>
            Channel: {data.channelId}
          </Text>
          <br />
          <Text style={{ fontSize: 11, fontFamily: 'monospace', color: token.colorTextSecondary }}>
            Hub: {data.hubUrl}
          </Text>
        </Card>

        <Alert
          type="warning"
          message="Keep this QR code private — it grants full access to this channel."
          style={{ width: '100%', fontSize: 12 }}
        />
      </Flex>
    </Modal>
  );
}

function WeChatFields() {
  return (
    <>
      <Flex gap={12}>
        <Form.Item name="appId" label="AppID" rules={[{ required: true, message: 'Required' }]} style={{ flex: 1 }}>
          <Input placeholder="wx_xxx" style={{ fontFamily: 'monospace' }} />
        </Form.Item>
        <Form.Item name="appSecret" label="AppSecret" rules={[{ required: true, message: 'Required' }]} style={{ flex: 1 }}>
          <Input.Password placeholder="WeChat AppSecret" />
        </Form.Item>
      </Flex>
      <Form.Item name="token" label="Token" extra="Webhook verification token">
        <Input placeholder="Your webhook token" style={{ fontFamily: 'monospace' }} />
      </Form.Item>
    </>
  );
}

// ===== Main component =====

interface Props {
  channels: ChannelInfo[];
  onRegister: (data: RegisterChannelPayload) => void;
  onUnregister: (id: number) => void;
  onUpdate: (id: number, updates: UpdateChannelPayload) => void;
}

export const ChannelSettings: React.FC<Props> = ({ channels, onRegister, onUnregister, onUpdate }) => {
  const { token } = theme.useToken();
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<ChannelInfo | null>(null);
  const [platform, setPlatform] = useState<Platform>('telegram');
  const [form] = Form.useForm();
  const [qrData, setQrData] = useState<QRPairingData | null>(null);
  const [registering, setRegistering] = useState(false);

  const openAdd = () => {
    setEditing(null);
    setPlatform('telegram');
    form.resetFields();
    form.setFieldsValue({ platformType: 'telegram', chatType: 'group', requiresTrigger: false, sandbox: false });
    setModalOpen(true);
  };

  const openEdit = (ch: ChannelInfo) => {
    setEditing(ch);
    const creds = parseCredentials(ch.credentialsJson);
    const p = ch.platformType as Platform;
    setPlatform(p);
    form.resetFields();
    form.setFieldsValue({
      name: ch.name,
      platformType: p,
      // flat credential fields
      botToken: creds.botToken ?? '',
      chatType: creds.chatType ?? 'group',
      requiresTrigger: creds.requiresTrigger ?? false,
      chatJid: creds.chatJid ?? '',
      appId: creds.appId ?? '',
      appSecret: creds.appSecret ?? '',
      token: creds.token ?? '',
      sandbox: creds.sandbox ?? false,
      hubUrl: creds.hubUrl ?? 'http://localhost:18080',
    });
    setModalOpen(true);
  };

  const onFinish = async (values: any) => {
    const p = (editing ? editing.platformType : values.platformType) as Platform;
    const credentials = buildCredentials(p, values);

    if (editing) {
      const oldCreds = parseCredentials(editing.credentialsJson);
      onUpdate(editing.id, { 
        name: values.name, 
        credentials: { ...oldCreds, ...credentials } 
      });
      message.success('Channel updated');
      setModalOpen(false);
      return;
    }

    if (p === 'senclaw') {
      setRegistering(true);
      try {
        const hubUrl = credentials.hubUrl as string || 'http://localhost:18080';

        // Fetch QR pairing data from backend after registration
        const res = await fetch(`${hubUrl}/v1/channels/register`, {
          method: 'POST',
        });

        if (res.ok) {
          const data = await res.json();
          // generate random key 32 length (true binary random)
          const keyArray = new Uint8Array(32);
          crypto.getRandomValues(keyArray);
          const encryptionKey = btoa(Array.from(keyArray, b => String.fromCharCode(b)).join(''));

          const fullCredentials = {
            ...credentials,
            channelId: data.channel_id,
            encryptionKey,
            accessToken: data.access_token
          };

          setQrData({
            channelId: data.channel_id,
            encryptionKey,
            hubUrl: hubUrl,
            token: data.access_token
          });

          // Save the channel in local db after getting the info
          onRegister({ 
            platformType: p, 
            name: values.name, 
            credentials: fullCredentials 
          });
        } else {
          message.error('Failed to register with ClawHub backend');
        }
        setModalOpen(false);
      } catch (err) {
        console.error(err);
        message.error('Failed to generate pairing QR');
      } finally {
        setRegistering(false);
      }
      return;
    }

    onRegister({ platformType: p, name: values.name, credentials });
    message.success('Channel registered');
    setModalOpen(false);
  };

  const handlePlatformChange = (p: Platform) => {
    setPlatform(p);
    form.resetFields(['botToken', 'chatType', 'requiresTrigger', 'chatJid', 'appId', 'appSecret', 'token', 'sandbox']);
    form.setFieldsValue({ chatType: 'group', requiresTrigger: false, sandbox: false });
  };

  const columns = [
    {
      title: 'Platform',
      dataIndex: 'platformType',
      key: 'platform',
      render: (p: string) => (
        <Tag color={PLATFORM_COLOR[p] ?? 'default'} style={{ borderRadius: 6 }}>
          {PLATFORM_LABELS[p] ?? p}
        </Tag>
      ),
    },
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (t: string) => <Text strong>{t}</Text>,
    },
    {
      title: 'Status',
      dataIndex: 'connectionState',
      key: 'status',
      render: (state: string) => {
        if (state === 'connected') return (
          <Space size={4}><CheckCircleOutlined style={{ color: token.colorSuccess }} /><Text type="success">Connected</Text></Space>
        );
        if (state === 'connecting') return (
          <Space size={4}><SyncOutlined spin style={{ color: token.colorPrimary }} /><Text type="secondary">Connecting</Text></Space>
        );
        return (
          <Space size={4}><CloseCircleOutlined style={{ color: token.colorTextQuaternary }} /><Text type="secondary">{state}</Text></Space>
        );
      },
    },
    {
      title: 'Created At',
      dataIndex: 'createdAt',
      key: 'createdAt',
      render: (d: string) => <Text type="secondary" style={{ fontSize: 12 }}>{new Date(d).toLocaleDateString()}</Text>,
    },
    {
      title: 'Enabled',
      key: 'enabled',
      width: 90,
      render: (_: any, record: ChannelInfo) => (
        <Switch
          size="small"
          checked={record.enabled !== false}
          onChange={(checked) => onUpdate(record.id, { enabled: checked })}
        />
      ),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: any, record: ChannelInfo) => (
        <Space>
          {record.platformType !== 'web' && (
            <Tooltip title="Edit">
              <Button type="text" icon={<EditOutlined />} onClick={() => openEdit(record)} />
            </Tooltip>
          )}
          {record.platformType === 'senclaw' && (
            <Tooltip title="Pair Mobile App">
              <Button
                type="text"
                icon={<QrcodeOutlined />}
                onClick={() => {
                  const creds = parseCredentials(record.credentialsJson);
                  setQrData({
                    channelId: creds.channelId,
                    encryptionKey: creds.encryptionKey,
                    hubUrl: creds.hubUrl,
                    token: creds.accessToken
                  });
                }}
              />
            </Tooltip>
          )}
          <Popconfirm
            title="Unregister this channel?"
            description="This will affect all agents bound to it."
            onConfirm={() => onUnregister(record.id)}
            okText="Unregister"
            cancelText="Cancel"
            okButtonProps={{ danger: true }}
          >
            <Tooltip title="Delete">
              <Button type="text" danger icon={<DeleteOutlined />} />
            </Tooltip>
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ maxWidth: 1000 }}>
      <style>{`.channel-row-disabled td { opacity: 0.45; }`}</style>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 32 }}>
        <div>
          <Title level={4} style={{ margin: 0 }}>Channels</Title>
          <Text type="secondary">Manage your communication platforms and credentials.</Text>
        </div>
        <Button type="primary" icon={<PlusOutlined />} onClick={openAdd} style={{ borderRadius: 8, height: 40 }}>
          Add Channel
        </Button>
      </div>

      <Card style={{ borderRadius: 12 }} styles={{ body: { padding: 0 } }}>
        <Table
          columns={columns}
          dataSource={channels}
          rowKey="id"
          pagination={false}
          locale={{ emptyText: 'No channels configured yet.' }}
          rowClassName={(record) => record.enabled === false ? 'channel-row-disabled' : ''}
        />
      </Card>

      <Modal
        title={editing ? `Edit — ${PLATFORM_LABELS[editing.platformType] ?? editing.platformType}` : 'Add New Channel'}
        open={modalOpen}
        onCancel={() => setModalOpen(false)}
        footer={null}
        width={520}
        destroyOnClose
      >
        <Form
          form={form}
          layout="vertical"
          onFinish={onFinish}
          style={{ marginTop: 20 }}
        >
          {/* Platform selector — hidden when editing */}
          {!editing && (
            <Form.Item name="platformType" label="Platform" rules={[{ required: true }]}>
              <Select onChange={handlePlatformChange}>
                <Option value="telegram">Telegram</Option>
                <Option value="feishu">Feishu / Lark</Option>
                <Option value="qq">QQ</Option>
                <Option value="wechat">WeChat</Option>
                <Option value="senclaw">Senclaw Connector</Option>
              </Select>
            </Form.Item>
          )}

          <Form.Item name="name" label="Channel Name" rules={[{ required: true, message: 'Please enter a name' }]}>
            <Input placeholder="e.g. My Telegram Bot" />
          </Form.Item>

          <Divider style={{ margin: '4px 0 16px' }} />

          {/* Platform-specific fields */}
          {platform === 'telegram' && <TelegramFields />}
          {platform === 'feishu' && <FeishuFields />}
          {platform === 'qq' && <QQFields />}
          {platform === 'wechat' && <WeChatFields />}
          {platform === 'senclaw' && <SenclawFields />}

          <Flex justify="flex-end" gap={8} style={{ marginTop: 8 }}>
            <Button onClick={() => setModalOpen(false)}>Cancel</Button>
            <Button type="primary" htmlType="submit" loading={registering}>
              {editing ? 'Save Changes' : platform === 'senclaw' ? 'Register & Get QR' : 'Register Channel'}
            </Button>
          </Flex>
        </Form>
      </Modal>

      {qrData && <QRPairingModal data={qrData} onClose={() => setQrData(null)} />}
    </div>
  );
};



