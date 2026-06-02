import React, { useState, useEffect } from 'react';
import { Table, Button, Modal, Form, Input, Select, message, Space, Popconfirm, Tag, InputNumber } from 'antd';
import { PlusOutlined, DeleteOutlined, EditOutlined, ApiOutlined, PlayCircleOutlined, StopOutlined } from '@ant-design/icons';
import type { PortForwardingRule, Host } from '../types';

export const PortForwardingView: React.FC = () => {
  const [rules, setRules] = useState<PortForwardingRule[]>([]);
  const [hosts, setHosts] = useState<Host[]>([]);
  const [isModalVisible, setIsModalVisible] = useState(false);
  const [editingRule, setEditingRule] = useState<PortForwardingRule | null>(null);
  const [form] = Form.useForm();

  useEffect(() => {
    fetchRules();
    fetchHosts();
  }, []);

  const fetchRules = async () => {
    try {
      const response = await fetch('./api/port-forwarding');
      const data = await response.json();
      setRules(data);
    } catch (err) {
      message.error('Failed to load port forwarding rules');
    }
  };

  const fetchHosts = async () => {
    try {
      const response = await fetch('./api/hosts');
      const data = await response.json();
      setHosts(data);
    } catch (err) {
      message.error('Failed to load hosts');
    }
  };

  const handleSave = async (values: any) => {
    try {
      const url = editingRule ? `./api/port-forwarding/${editingRule.id}` : './api/port-forwarding';
      const method = editingRule ? 'PUT' : 'POST';
      
      const payload = {
        ...values,
        id: editingRule?.id || '',
        active: editingRule?.active || false,
      };

      await fetch(url, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
      });
      
      message.success(editingRule ? 'Rule updated' : 'Rule added');
      setIsModalVisible(false);
      fetchRules();
    } catch (err) {
      message.error('Failed to save rule');
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await fetch(`./api/port-forwarding/${id}`, { method: 'DELETE' });
      message.success('Rule deleted');
      fetchRules();
    } catch (err) {
      message.error('Failed to delete rule');
    }
  };

  const toggleStatus = async (rule: PortForwardingRule) => {
    try {
      const action = rule.active ? 'stop' : 'start';
      const response = await fetch(`./api/port-forwarding/${rule.id}/${action}`, { method: 'POST' });
      
      if (!response.ok) {
         throw new Error("Failed to toggle status");
      }
      
      message.success(`Tunnel ${rule.active ? 'stopped' : 'started'}`);
      fetchRules();
    } catch (err) {
      message.error(`Failed to ${rule.active ? 'stop' : 'start'} tunnel`);
    }
  };

  const columns = [
    {
      title: 'Name',
      dataIndex: 'name',
      key: 'name',
      render: (text: string) => <><ApiOutlined style={{ marginRight: 8, color: '#3b82f6' }} />{text}</>,
    },
    {
      title: 'Host',
      dataIndex: 'host_id',
      key: 'host_id',
      render: (hostId: string) => {
        const host = hosts.find(h => h.id === hostId);
        return host ? host.name : 'Unknown';
      },
    },
    {
      title: 'Local',
      key: 'local',
      render: (_: any, record: PortForwardingRule) => `${record.bind_address}:${record.local_port}`,
    },
    {
      title: 'Remote',
      key: 'remote',
      render: (_: any, record: PortForwardingRule) => `${record.destination_address}:${record.destination_port}`,
    },
    {
      title: 'Status',
      key: 'active',
      dataIndex: 'active',
      render: (active: boolean) => (
        <Tag color={active ? 'green' : 'default'}>{active ? 'Active' : 'Inactive'}</Tag>
      ),
    },
    {
      title: 'Actions',
      key: 'actions',
      render: (_: any, record: PortForwardingRule) => (
        <Space>
          <Button 
            type="text" 
            icon={record.active ? <StopOutlined /> : <PlayCircleOutlined />} 
            style={{ color: record.active ? '#ef4444' : '#10b981' }}
            onClick={() => toggleStatus(record)}
            title={record.active ? "Stop Tunnel" : "Start Tunnel"}
          />
          <Button 
            type="text" 
            icon={<EditOutlined />} 
            style={{ color: '#3b82f6' }}
            onClick={() => {
              setEditingRule(record);
              form.setFieldsValue(record);
              setIsModalVisible(true);
            }}
          />
          <Popconfirm title="Delete this rule?" onConfirm={() => handleDelete(record.id)}>
            <Button type="text" danger icon={<DeleteOutlined />} disabled={record.active} />
          </Popconfirm>
        </Space>
      ),
    },
  ];

  return (
    <div style={{ padding: 24, height: '100%', overflowY: 'auto', color: '#fff' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 24 }}>
        <div style={{ fontSize: 20, fontWeight: 500 }}>Port Forwarding</div>
        <Button 
          type="primary" 
          icon={<PlusOutlined />}
          onClick={() => {
            setEditingRule(null);
            form.resetFields();
            setIsModalVisible(true);
          }}
        >
          Add Rule
        </Button>
      </div>

      <Table 
        dataSource={rules} 
        columns={columns} 
        rowKey="id"
        pagination={false}
        style={{ background: '#1f2937', borderRadius: 8 }}
      />

      <Modal
        title={editingRule ? "Edit Rule" : "New Rule"}
        open={isModalVisible}
        onCancel={() => setIsModalVisible(false)}
        footer={null}
        styles={{ body: { backgroundColor: '#1f2937', color: '#fff' }, header: { backgroundColor: '#1f2937' } }}
      >
        <Form form={form} layout="vertical" onFinish={handleSave} initialValues={{ bind_address: '127.0.0.1' }}>
          <Form.Item name="name" label={<span style={{ color: '#fff' }}>Rule Name</span>} rules={[{ required: true }]}>
            <Input placeholder="e.g. Database Tunnel" style={{ backgroundColor: '#111827', color: '#fff', borderColor: '#374151' }} />
          </Form.Item>
          <Form.Item name="host_id" label={<span style={{ color: '#fff' }}>SSH Host</span>} rules={[{ required: true }]}>
            <Select 
              placeholder="Select an SSH host to tunnel through"
              dropdownStyle={{ backgroundColor: '#1f2937', color: '#fff' }}
              style={{ width: '100%' }}
            >
              {hosts.map(host => (
                <Select.Option key={host.id} value={host.id}>{host.name}</Select.Option>
              ))}
            </Select>
          </Form.Item>
          <div style={{ display: 'flex', gap: '16px' }}>
            <Form.Item name="bind_address" label={<span style={{ color: '#fff' }}>Local Bind Address</span>} rules={[{ required: true }]} style={{ flex: 1 }}>
              <Input placeholder="127.0.0.1" style={{ backgroundColor: '#111827', color: '#fff', borderColor: '#374151' }} />
            </Form.Item>
            <Form.Item name="local_port" label={<span style={{ color: '#fff' }}>Local Port</span>} rules={[{ required: true }]} style={{ width: '120px' }}>
              <InputNumber min={1} max={65535} style={{ width: '100%', backgroundColor: '#111827', color: '#fff', borderColor: '#374151' }} />
            </Form.Item>
          </div>
          <div style={{ display: 'flex', gap: '16px' }}>
            <Form.Item name="destination_address" label={<span style={{ color: '#fff' }}>Remote Target Address</span>} rules={[{ required: true }]} style={{ flex: 1 }}>
              <Input placeholder="127.0.0.1" style={{ backgroundColor: '#111827', color: '#fff', borderColor: '#374151' }} />
            </Form.Item>
            <Form.Item name="destination_port" label={<span style={{ color: '#fff' }}>Remote Port</span>} rules={[{ required: true }]} style={{ width: '120px' }}>
              <InputNumber min={1} max={65535} style={{ width: '100%', backgroundColor: '#111827', color: '#fff', borderColor: '#374151' }} />
            </Form.Item>
          </div>
          <div style={{ display: 'flex', justifyContent: 'flex-end', marginTop: 24 }}>
            <Button onClick={() => setIsModalVisible(false)} style={{ marginRight: 8, backgroundColor: 'transparent', color: '#fff' }}>Cancel</Button>
            <Button type="primary" htmlType="submit">Save</Button>
          </div>
        </Form>
      </Modal>
    </div>
  );
};
