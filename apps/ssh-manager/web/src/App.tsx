import { useState, useEffect } from 'react';
import { ConfigProvider, Layout, Menu, Button, Row, Col, Empty, message, Input, Tabs } from 'antd';
import { DesktopOutlined, KeyOutlined, SwapOutlined, CodeOutlined, PlusOutlined, HomeOutlined, FolderOutlined } from '@ant-design/icons';
import type { Host, AppTab } from './types';
import { HostCard } from './components/HostCard';
import { HostDetails } from './components/HostDetails';
import { KeychainView } from './components/KeychainView';
import { PortForwardingView } from './components/PortForwardingView';
import { TerminalView } from './TerminalView';
import { SftpView } from './components/SftpView';
import './App.css';

const { Header, Sider, Content } = Layout;

function App() {
  const [hosts, setHosts] = useState<Host[]>([]);
  const [selectedHost, setSelectedHost] = useState<Host | null>(null);
  const [isEditing, setIsEditing] = useState(false);
  
  const [tabs, setTabs] = useState<AppTab[]>([
    { id: 'home', type: 'home', title: 'Vaults' },
    { id: 'sftp', type: 'sftp', title: 'SFTP' }
  ]);
  const [activeTabId, setActiveTabId] = useState<string>('home');
  const [currentMenu, setCurrentMenu] = useState<string>('hosts');

  useEffect(() => {
    fetchHosts();
  }, []);

  const fetchHosts = async () => {
    try {
      const response = await fetch('./api/hosts');
      const data = await response.json();
      setHosts(data);
    } catch (err) {
      console.error('Failed to fetch hosts', err);
      message.error('Failed to load hosts');
    }
  };

  const handleSaveHost = async (host: Host) => {
    try {
      if (host.id) {
        await fetch(`./api/hosts/${host.id}`, {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(host),
        });
        message.success('Host updated');
      } else {
        await fetch('./api/hosts', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(host),
        });
        message.success('Host added');
      }
      fetchHosts();
      setIsEditing(false);
      setSelectedHost(null);
    } catch (err) {
      message.error('Failed to save host');
    }
  };

  const handleDeleteHost = async (id: string) => {
    try {
      await fetch(`./api/hosts/${id}`, { method: 'DELETE' });
      message.success('Host deleted');
      fetchHosts();
      setIsEditing(false);
      setSelectedHost(null);
    } catch (err) {
      message.error('Failed to delete host');
    }
  };

  const handleConnect = (host: Host) => {
    const tabId = `term-${host.id}-${Date.now()}`;
    setTabs([...tabs, { id: tabId, type: 'terminal', title: host.name || host.host, host }]);
    setActiveTabId(tabId);
  };

  const removeTab = (targetKey: string) => {
    const newTabs = tabs.filter(tab => tab.id !== targetKey);
    setTabs(newTabs);
    if (activeTabId === targetKey) {
      setActiveTabId(newTabs[newTabs.length - 1].id);
    }
  };

  const onEditTab = (targetKey: any, action: 'add' | 'remove') => {
    if (action === 'remove') {
      removeTab(targetKey as string);
    }
  };

  const renderHomeContent = () => {
    switch (currentMenu) {
      case 'hosts':
        return (
          <Layout style={{ height: '100%', backgroundColor: '#111827' }}>
            <Header style={{ padding: '0 24px', display: 'flex', alignItems: 'center', borderBottom: '1px solid #374151', backgroundColor: '#111827' }}>
              <Input.Search 
                placeholder="Find a host or ssh user@hostname..." 
                style={{ maxWidth: 400 }} 
                className="custom-search"
              />
              <Button 
                type="primary" 
                icon={<PlusOutlined />} 
                style={{ marginLeft: 'auto' }}
                onClick={() => {
                  setSelectedHost(null);
                  setIsEditing(true);
                }}
              >
                New Host
              </Button>
            </Header>
            <Content style={{ padding: '24px', overflowY: 'auto' }}>
              <div style={{ marginBottom: 24, color: '#9ca3af', fontSize: 16 }}>
                Hosts ({hosts.length})
              </div>
              {hosts.length === 0 ? (
                <Empty description={<span style={{ color: '#9ca3af' }}>No hosts found. Add one to get started.</span>} />
              ) : (
                <Row gutter={[16, 16]}>
                  {hosts.map(host => (
                    <Col xs={24} sm={12} md={8} lg={6} key={host.id}>
                      <HostCard 
                        host={host} 
                        selected={selectedHost?.id === host.id}
                        onClick={(h) => {
                          setSelectedHost(h);
                          setIsEditing(true);
                        }}
                      />
                    </Col>
                  ))}
                </Row>
              )}
            </Content>
          </Layout>
        );
      case 'keychain':
        return <KeychainView />;
      case 'port-forwarding':
        return <PortForwardingView />;
      case 'logs':
        return <div style={{ padding: 24, color: '#fff' }}>Logs feature coming soon...</div>;
      default:
        return null;
    }
  };

  return (
    <ConfigProvider
      theme={{
        token: {
          colorPrimary: '#3b82f6',
          colorBgBase: '#111827',
          colorTextBase: '#f9fafb',
          colorBorder: '#374151',
        },
        components: {
          Layout: {
            siderBg: '#1f2937',
            headerBg: '#1f2937',
            bodyBg: '#111827',
          },
          Menu: {
            itemBg: '#1f2937',
            itemColor: '#9ca3af',
            itemSelectedBg: '#374151',
            itemSelectedColor: '#fff',
          },
          Card: {
            colorBgContainer: '#1f2937',
          },
          Tabs: {
            itemColor: '#9ca3af',
            itemHoverColor: '#fff',
            itemSelectedColor: '#fff',
            cardBg: '#111827',
          }
        },
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: '#1e1e1e' }}>
        {/* Top Tab Bar - Termius Style */}
        <div className="termius-tab-bar" style={{ 
          background: '#111827', 
          borderBottom: '1px solid #1e1e1e',
          paddingTop: 8,
          paddingLeft: 80, // Space for window controls if any
        }}>
          <Tabs
            hideAdd
            type="editable-card"
            onChange={(key) => setActiveTabId(key)}
            activeKey={activeTabId}
            onEdit={onEditTab}
            items={tabs.map(tab => ({
              label: (
                <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                  {tab.type === 'home' ? <HomeOutlined /> : tab.type === 'sftp' ? <FolderOutlined /> : <DesktopOutlined />}
                  {tab.title}
                </span>
              ),
              key: tab.id,
              closable: tab.type !== 'home' && tab.type !== 'sftp',
            }))}
            style={{ marginBottom: -1 }} // Hide bottom border
          />
        </div>

        {/* Main Area */}
        <div style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>
          {/* Sidebar */}
          <div style={{ display: activeTabId === 'home' ? 'block' : 'none' }}>
            <Sider width={250} style={{ borderRight: '1px solid #111827', background: '#1f2937', height: '100%' }}>
            <Menu
              mode="inline"
              selectedKeys={[activeTabId === 'home' ? currentMenu : '']}
              style={{ borderRight: 0, marginTop: 16, background: 'transparent' }}
              onClick={({ key }) => {
                if (key !== 'terminal') {
                  setCurrentMenu(key);
                  setActiveTabId('home');
                }
              }}
              items={[
                { key: 'hosts', icon: <DesktopOutlined />, label: 'Hosts' },
                { key: 'keychain', icon: <KeyOutlined />, label: 'Keychain' },
                { key: 'port-forwarding', icon: <SwapOutlined />, label: 'Port Forwarding' },
                { key: 'logs', icon: <CodeOutlined />, label: 'Logs' },
              ]}
            />
            </Sider>
          </div>

          {/* Content Area */}
          <div style={{ flex: 1, display: 'flex', overflow: 'hidden', position: 'relative' }}>
            
            {/* Home Views */}
            <div style={{ 
              display: activeTabId === 'home' ? 'flex' : 'none', 
              flex: 1, 
              width: '100%',
              height: '100%' 
            }}>
              <Layout style={{ height: '100%', background: '#111827' }}>
                {renderHomeContent()}
              </Layout>
              {isEditing && currentMenu === 'hosts' && activeTabId === 'home' && (
                <Sider width={350} style={{ borderLeft: '1px solid #374151', background: '#1f2937' }}>
                  <HostDetails 
                    host={selectedHost} 
                    onSave={handleSaveHost}
                    onDelete={handleDeleteHost}
                    onConnect={handleConnect}
                  />
                </Sider>
              )}
            </div>

            {/* SFTP View */}
            <div style={{ 
              display: activeTabId === 'sftp' ? 'block' : 'none', 
              flex: 1, 
              width: '100%',
              height: '100%' 
            }}>
              <SftpView hosts={hosts} />
            </div>

            {/* Terminal Views */}
            {tabs.filter(t => t.type === 'terminal').map(tab => (
              <div 
                key={tab.id}
                style={{ 
                  display: activeTabId === tab.id ? 'block' : 'none', 
                  width: '100%', 
                  height: '100%',
                  background: '#0f172a'
                }}
              >
                {tab.host && <TerminalView host={tab.host} />}
              </div>
            ))}
            
          </div>
        </div>
      </div>
    </ConfigProvider>
  );
}

export default App;
