import React, { useState, useEffect } from 'react';
import { Layout, Table, Button, Input, Row, Col, Typography, Space, message } from 'antd';
import { FolderOutlined, FileOutlined, DesktopOutlined, ArrowLeftOutlined, CloudServerOutlined } from '@ant-design/icons';
import type { Host, FileNode } from '../types';

const { Header, Content } = Layout;
const { Text } = Typography;

interface SftpViewProps {
  hosts: Host[];
}

export const SftpView: React.FC<SftpViewProps> = ({ hosts }) => {
  const [localPath, setLocalPath] = useState('/');
  const [localFiles, setLocalFiles] = useState<FileNode[]>([]);
  const [remotePath, setRemotePath] = useState('/');
  const [remoteFiles, setRemoteFiles] = useState<FileNode[]>([]);
  
  const [connId, setConnId] = useState<string | null>(null);
  const [selectedHost, setSelectedHost] = useState<Host | null>(null);
  const [loadingLocal, setLoadingLocal] = useState(false);
  const [loadingRemote, setLoadingRemote] = useState(false);

  useEffect(() => {
    fetchLocalFiles(localPath);
  }, [localPath]);

  useEffect(() => {
    if (connId) {
      fetchRemoteFiles(connId, remotePath);
    }
  }, [connId, remotePath]);

  const fetchLocalFiles = async (path: string) => {
    setLoadingLocal(true);
    try {
      const res = await fetch(`./api/sftp/local/ls?path=${encodeURIComponent(path)}`);
      if (res.ok) {
        const data = await res.json();
        // Sort directories first
        data.sort((a: FileNode, b: FileNode) => {
          if (a.is_dir === b.is_dir) return a.name.localeCompare(b.name);
          return a.is_dir ? -1 : 1;
        });
        setLocalFiles(data);
      } else {
        message.error('Failed to load local files');
      }
    } catch (err) {
      message.error('Failed to load local files');
    } finally {
      setLoadingLocal(false);
    }
  };

  const fetchRemoteFiles = async (id: string, path: string) => {
    setLoadingRemote(true);
    try {
      const res = await fetch(`./api/sftp/remote/${id}/ls?path=${encodeURIComponent(path)}`);
      if (res.ok) {
        const data = await res.json();
        data.sort((a: FileNode, b: FileNode) => {
          if (a.is_dir === b.is_dir) return a.name.localeCompare(b.name);
          return a.is_dir ? -1 : 1;
        });
        setRemoteFiles(data);
      } else {
        message.error('Failed to load remote files');
      }
    } catch (err) {
      message.error('Failed to load remote files');
    } finally {
      setLoadingRemote(false);
    }
  };

  const handleConnect = async (host: Host) => {
    try {
      setLoadingRemote(true);
      const res = await fetch('./api/sftp/connect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ host_id: host.id })
      });
      if (res.ok) {
        const data = await res.json();
        setConnId(data.conn_id);
        setSelectedHost(host);
        setRemotePath('.'); // Start at home directory for remote
      } else {
        message.error('Failed to connect via SFTP');
      }
    } catch (err) {
      message.error('SFTP connection error');
    } finally {
      setLoadingRemote(false);
    }
  };

  const renderFileTable = (
    files: FileNode[], 
    loading: boolean, 
    currentPath: string, 
    onNavigate: (path: string) => void
  ) => {
    const columns = [
      {
        title: 'Name',
        dataIndex: 'name',
        key: 'name',
        render: (text: string, record: FileNode) => (
          <Space style={{ cursor: record.is_dir ? 'pointer' : 'default' }} onClick={() => {
            if (record.is_dir) {
              const separator = currentPath.endsWith('/') ? '' : '/';
              onNavigate(currentPath + separator + text);
            }
          }}>
            {record.is_dir ? <FolderOutlined style={{ color: '#3b82f6' }} /> : <FileOutlined />}
            {text}
          </Space>
        )
      },
      {
        title: 'Date Modified',
        dataIndex: 'modified_time',
        key: 'modified_time',
        width: 150,
        render: (time: number) => new Date(time * 1000).toLocaleDateString()
      },
      {
        title: 'Size',
        dataIndex: 'size',
        key: 'size',
        width: 100,
        render: (size: number, record: FileNode) => record.is_dir ? '--' : `${(size / 1024).toFixed(1)} KB`
      }
    ];

    return (
      <Table 
        dataSource={files} 
        columns={columns} 
        rowKey="name"
        loading={loading}
        size="small"
        pagination={false}
        scroll={{ y: 'calc(100vh - 200px)' }}
        className="sftp-table"
      />
    );
  };

  return (
    <div style={{ display: 'flex', width: '100%', height: '100%', background: '#111827' }}>
      {/* Local Pane */}
      <div style={{ flex: 1, borderRight: '1px solid #374151', display: 'flex', flexDirection: 'column' }}>
        <Header style={{ background: '#1f2937', padding: '0 16px', display: 'flex', alignItems: 'center', borderBottom: '1px solid #374151', height: 48 }}>
          <Space>
            <DesktopOutlined style={{ color: '#3b82f6' }} />
            <Text style={{ color: '#fff', fontWeight: 600 }}>Local</Text>
          </Space>
        </Header>
        <div style={{ padding: '8px 16px', background: '#111827', borderBottom: '1px solid #374151', display: 'flex', gap: 8 }}>
          <Button icon={<ArrowLeftOutlined />} size="small" onClick={() => {
            const parts = localPath.split('/').filter(Boolean);
            parts.pop();
            setLocalPath('/' + parts.join('/'));
          }} />
          <Input size="small" value={localPath} onChange={e => setLocalPath(e.target.value)} onPressEnter={() => fetchLocalFiles(localPath)} style={{ flex: 1 }} />
        </div>
        <Content style={{ overflow: 'hidden' }}>
          {renderFileTable(localFiles, loadingLocal, localPath, setLocalPath)}
        </Content>
      </div>

      {/* Remote Pane */}
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column' }}>
        <Header style={{ background: '#1f2937', padding: '0 16px', display: 'flex', alignItems: 'center', borderBottom: '1px solid #374151', height: 48 }}>
          <Space>
            <CloudServerOutlined style={{ color: '#f59e0b' }} />
            <Text style={{ color: '#fff', fontWeight: 600 }}>
              {selectedHost ? selectedHost.name || selectedHost.host : 'Remote'}
            </Text>
          </Space>
          {connId && (
            <Button size="small" type="text" danger style={{ marginLeft: 'auto' }} onClick={() => {
              setConnId(null);
              setSelectedHost(null);
              setRemoteFiles([]);
            }}>
              Disconnect
            </Button>
          )}
        </Header>

        {!connId ? (
          <Content style={{ padding: 24, overflowY: 'auto' }}>
            <div style={{ marginBottom: 16, color: '#9ca3af' }}>Select a host to connect</div>
            <Row gutter={[16, 16]}>
              {hosts.map(host => (
                <Col span={24} key={host.id}>
                  <div 
                    onClick={() => handleConnect(host)}
                    style={{ 
                      padding: 12, 
                      background: '#1f2937', 
                      borderRadius: 8, 
                      cursor: 'pointer',
                      border: '1px solid #374151',
                      display: 'flex',
                      alignItems: 'center',
                      gap: 12
                    }}
                    className="host-hover"
                  >
                    <div style={{ background: '#f59e0b', padding: 8, borderRadius: '50%', color: '#fff' }}>
                      <CloudServerOutlined />
                    </div>
                    <div>
                      <div style={{ color: '#fff', fontWeight: 500 }}>{host.name || host.host}</div>
                      <div style={{ color: '#9ca3af', fontSize: 12 }}>{host.user}@{host.host}:{host.port}</div>
                    </div>
                  </div>
                </Col>
              ))}
            </Row>
          </Content>
        ) : (
          <>
            <div style={{ padding: '8px 16px', background: '#111827', borderBottom: '1px solid #374151', display: 'flex', gap: 8 }}>
              <Button icon={<ArrowLeftOutlined />} size="small" onClick={() => {
                const parts = remotePath.split('/').filter(Boolean);
                parts.pop();
                setRemotePath('/' + parts.join('/'));
              }} />
              <Input size="small" value={remotePath} onChange={e => setRemotePath(e.target.value)} onPressEnter={() => fetchRemoteFiles(connId, remotePath)} style={{ flex: 1 }} />
            </div>
            <Content style={{ overflow: 'hidden' }}>
              {renderFileTable(remoteFiles, loadingRemote, remotePath, setRemotePath)}
            </Content>
          </>
        )}
      </div>
    </div>
  );
};
