import React, { useEffect, useState, useCallback } from 'react';
import {
  Modal, Button, Breadcrumb, List, Spin, theme, Typography, Space, Input,
} from 'antd';
import {
  FolderOutlined, FolderOpenOutlined, ArrowLeftOutlined, HomeOutlined, CheckOutlined,
} from '@ant-design/icons';

const { Text } = Typography;

interface DirEntry {
  name: string;
  path: string;
}

interface BrowseResult {
  current: string;
  parent: string | null;
  dirs: DirEntry[];
}

interface Props {
  open: boolean;
  value?: string;
  onSelect: (path: string) => void;
  onCancel: () => void;
}

export function FolderPicker({ open, value, onSelect, onCancel }: Props) {
  const { token } = theme.useToken();
  const [result, setResult] = useState<BrowseResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [inputPath, setInputPath] = useState('');
  const [inputError, setInputError] = useState<string | null>(null);

  const browse = useCallback(async (path?: string) => {
    setLoading(true);
    setInputError(null);
    try {
      const url = path ? `/api/fs/ls?path=${encodeURIComponent(path)}` : '/api/fs/ls';
      const res = await fetch(url);
      if (!res.ok) {
        const text = await res.text();
        setInputError(text || 'Cannot open directory');
        return;
      }
      const data: BrowseResult = await res.json();
      setResult(data);
      setInputPath(data.current);
    } catch {
      setInputError('Network error');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open) browse(value || undefined);
  }, [open]);

  const handleInputSubmit = () => {
    if (inputPath.trim()) browse(inputPath.trim());
  };

  const segments = result
    ? result.current.split('/').filter(Boolean)
    : [];

  const buildBreadcrumb = () => {
    const items = [
      {
        title: (
          <span
            style={{ cursor: 'pointer', color: token.colorPrimary }}
            onClick={() => browse('/')}
          >
            <HomeOutlined />
          </span>
        ),
      },
    ];
    let acc = '';
    segments.forEach((seg, i) => {
      acc += '/' + seg;
      const path = acc;
      items.push({
        title: (
          <span
            key={path}
            style={{
              cursor: i < segments.length - 1 ? 'pointer' : undefined,
              color: i < segments.length - 1 ? token.colorPrimary : undefined,
              fontWeight: i === segments.length - 1 ? 600 : undefined,
            }}
            onClick={i < segments.length - 1 ? () => browse(path) : undefined}
          >
            {seg}
          </span>
        ),
      });
    });
    return items;
  };

  return (
    <Modal
      title={
        <Space>
          <FolderOpenOutlined style={{ color: token.colorPrimary }} />
          <span>Choose Workspace Folder</span>
        </Space>
      }
      open={open}
      onCancel={onCancel}
      width={560}
      footer={
        <Space style={{ width: '100%', justifyContent: 'space-between' }}>
          <Text type="secondary" style={{ fontSize: 12, fontFamily: 'monospace' }}>
            {result?.current ?? ''}
          </Text>
          <Space>
            <Button onClick={onCancel}>Cancel</Button>
            <Button
              type="primary"
              icon={<CheckOutlined />}
              disabled={!result}
              onClick={() => result && onSelect(result.current)}
            >
              Select This Folder
            </Button>
          </Space>
        </Space>
      }
    >
      {/* Manual path input */}
      <Input.Search
        value={inputPath}
        onChange={e => setInputPath(e.target.value)}
        onSearch={handleInputSubmit}
        onPressEnter={handleInputSubmit}
        placeholder="/path/to/folder"
        enterButton="Go"
        status={inputError ? 'error' : undefined}
        style={{ marginBottom: 8, fontFamily: 'monospace', fontSize: 12 }}
      />
      {inputError && (
        <Text type="danger" style={{ fontSize: 12, display: 'block', marginBottom: 6 }}>
          {inputError}
        </Text>
      )}

      {/* Breadcrumb */}
      {result && (
        <div style={{ marginBottom: 8 }}>
          <Breadcrumb items={buildBreadcrumb()} />
        </div>
      )}

      {/* Directory listing */}
      <div
        style={{
          height: 320,
          overflowY: 'auto',
          border: `1px solid ${token.colorBorder}`,
          borderRadius: token.borderRadius,
          background: token.colorBgLayout,
        }}
      >
        {loading && (
          <div style={{ display: 'flex', justifyContent: 'center', padding: 40 }}>
            <Spin />
          </div>
        )}
        {!loading && result && (
          <List
            size="small"
            dataSource={[
              ...(result.parent !== null
                ? [{ name: '..', path: result.parent! }]
                : []),
              ...result.dirs,
            ]}
            locale={{ emptyText: 'No subdirectories' }}
            renderItem={item => (
              <List.Item
                style={{
                  cursor: 'pointer',
                  padding: '6px 12px',
                  transition: 'background 0.15s',
                }}
                onMouseEnter={e => (e.currentTarget.style.background = token.colorBgTextHover)}
                onMouseLeave={e => (e.currentTarget.style.background = 'transparent')}
                onClick={() => browse(item.path)}
              >
                <Space>
                  {item.name === '..' ? (
                    <ArrowLeftOutlined style={{ color: token.colorTextTertiary }} />
                  ) : (
                    <FolderOutlined style={{ color: token.colorWarning }} />
                  )}
                  <Text style={{ fontSize: 13 }}>{item.name}</Text>
                </Space>
              </List.Item>
            )}
          />
        )}
      </div>
    </Modal>
  );
}
