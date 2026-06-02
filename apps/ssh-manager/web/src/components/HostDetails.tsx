import React, { useEffect } from 'react';
import { Form, Input, InputNumber, Button, Space, Typography, Divider } from 'antd';
import type { Host, KeychainItem } from '../types';
import { Select } from 'antd';

const { Title, Text } = Typography;

interface HostDetailsProps {
  host: Host | null;
  onSave: (host: Host) => void;
  onDelete: (id: string) => void;
  onConnect: (host: Host) => void;
}

export const HostDetails: React.FC<HostDetailsProps> = ({ host, onSave, onDelete, onConnect }) => {
  const [form] = Form.useForm();
  const [keychainItems, setKeychainItems] = React.useState<KeychainItem[]>([]);

  useEffect(() => {
    fetch('./api/keychain')
      .then(res => res.json())
      .then(data => setKeychainItems(data))
      .catch(err => console.error('Failed to fetch keychain', err));
  }, []);
  
  useEffect(() => {
    if (host) {
      form.setFieldsValue(host);
    } else {
      form.resetFields();
      form.setFieldsValue({ port: 22 });
    }
  }, [host, form]);

  const handleSubmit = (values: any) => {
    onSave({
      ...values,
      id: host?.id || '',
      tags: [],
    });
  };

  return (
    <div style={{ padding: '24px', height: '100%', backgroundColor: '#1f2937', color: '#fff' }}>
      <Title level={4} style={{ color: '#fff', marginTop: 0 }}>
        {host ? 'Edit Host' : 'New Host'}
      </Title>
      
      <Form
        form={form}
        layout="vertical"
        onFinish={handleSubmit}
        initialValues={{ port: 22 }}
      >
        <Form.Item
          name="name"
          label={<span style={{ color: '#9ca3af' }}>Alias</span>}
          rules={[{ required: true, message: 'Please enter a name' }]}
        >
          <Input placeholder="e.g. Production Server" style={{ backgroundColor: '#111827', color: '#fff', borderColor: '#374151' }} />
        </Form.Item>
        
        <Space style={{ display: 'flex', marginBottom: 8 }} align="baseline">
          <Form.Item
            name="host"
            label={<span style={{ color: '#9ca3af' }}>Hostname or IP</span>}
            rules={[{ required: true, message: 'Please enter hostname' }]}
            style={{ flex: 1 }}
          >
            <Input placeholder="192.168.1.1" style={{ backgroundColor: '#111827', color: '#fff', borderColor: '#374151', minWidth: 200 }} />
          </Form.Item>
          
          <Form.Item
            name="port"
            label={<span style={{ color: '#9ca3af' }}>Port</span>}
            rules={[{ required: true, message: 'Port is required' }]}
          >
            <InputNumber min={1} max={65535} style={{ backgroundColor: '#111827', color: '#fff', borderColor: '#374151', width: 80 }} />
          </Form.Item>
        </Space>
        
        <Divider style={{ borderColor: '#374151', margin: '12px 0' }} />
        <Text style={{ color: '#9ca3af', display: 'block', marginBottom: 16 }}>Credentials</Text>
        
        <Form.Item
          name="user"
          label={<span style={{ color: '#9ca3af' }}>Username</span>}
          rules={[{ required: true, message: 'Please enter username' }]}
        >
          <Input placeholder="root" style={{ backgroundColor: '#111827', color: '#fff', borderColor: '#374151' }} />
        </Form.Item>
        
        <Form.Item
          name="password"
          label={<span style={{ color: '#9ca3af' }}>Password</span>}
        >
          <Input.Password placeholder="Password (if not using Keychain)" style={{ backgroundColor: '#111827', color: '#fff', borderColor: '#374151' }} />
        </Form.Item>
        
        <Form.Item
          name="keychain_id"
          label={<span style={{ color: '#9ca3af' }}>Use Keychain Credential</span>}
        >
          <Select 
            placeholder="Select a credential (optional)" 
            allowClear
            dropdownStyle={{ backgroundColor: '#1f2937', color: '#fff' }}
            style={{ width: '100%' }}
          >
            {keychainItems.map(item => (
              <Select.Option key={item.id} value={item.id}>
                {item.name} ({item.item_type})
              </Select.Option>
            ))}
          </Select>
        </Form.Item>

        <Form.Item style={{ marginTop: 32 }}>
          <Button type="primary" htmlType="submit" block style={{ backgroundColor: '#3b82f6' }}>
            Save Host
          </Button>
        </Form.Item>
        
        {host && (
          <Space direction="vertical" style={{ width: '100%' }}>
            <Button 
              type="primary" 
              block 
              onClick={() => onConnect(host)}
              style={{ backgroundColor: '#10b981', borderColor: '#10b981' }}
            >
              Connect
            </Button>
            <Button 
              danger 
              block 
              type="text" 
              onClick={() => onDelete(host.id)}
            >
              Delete Host
            </Button>
          </Space>
        )}
      </Form>
    </div>
  );
};
