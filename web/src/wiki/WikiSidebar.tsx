import { useState, useEffect } from 'react';
import { Typography, Input, List, theme, Space, Button } from 'antd';
import { 
  FileOutlined, 
  FolderOutlined, 
  FolderOpenOutlined, 
  SearchOutlined, 
  LineChartOutlined, 
  ClusterOutlined,
  BookOutlined
} from '@ant-design/icons';
import type { DirNode, SearchResult } from '../hooks/useWiki';

const { Text } = Typography;

interface Props {
  tree: DirNode[];
  treeLoading: boolean;
  searchResults: SearchResult[];
  searching: boolean;
  selectedPath: string | null;
  activeView: 'home' | 'doc' | 'stats' | 'categories';
  onSelectDoc: (path: string) => void;
  onSearch: (q: string) => void;
  onClearSearch: () => void;
  onShowStats: () => void;
  onShowCategories: () => void;
  onShowHome: () => void;
}

function TreeNode({
  node, depth, selectedPath, onSelectDoc,
}: {
  node: DirNode;
  depth: number;
  selectedPath: string | null;
  onSelectDoc: (path: string) => void;
}) {
  const { token } = theme.useToken();
  const [open, setOpen] = useState(depth === 0);
  const indent = depth * 12;

  if (node.type === 'file') {
    const isSelected = node.path === selectedPath;
    const name = node.name.replace(/\.md$/, '');
    return (
      <div
        onClick={() => onSelectDoc(node.path)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '8px',
          padding: '6px 12px',
          paddingLeft: `${indent + 12}px`,
          cursor: 'pointer',
          borderRadius: '8px',
          margin: '2px 8px',
          transition: 'all 0.2s',
          background: isSelected ? `${token.colorPrimary}15` : 'transparent',
          color: isSelected ? token.colorPrimary : token.colorTextSecondary,
        }}
        onMouseEnter={(e) => {
          if (!isSelected) e.currentTarget.style.background = token.colorFillAlter;
        }}
        onMouseLeave={(e) => {
          if (!isSelected) e.currentTarget.style.background = 'transparent';
        }}
      >
        <FileOutlined style={{ fontSize: '14px', opacity: 0.7 }} />
        <Text ellipsis style={{ 
          fontSize: '13px', 
          color: 'inherit',
          fontWeight: isSelected ? 600 : 400 
        }}>
          {name}
        </Text>
      </div>
    );
  }

  const fileCount = countFiles(node);
  return (
    <div>
      <div
        onClick={() => setOpen(o => !o)}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '8px',
          padding: '6px 12px',
          paddingLeft: `${indent + 12}px`,
          cursor: 'pointer',
          borderRadius: '8px',
          margin: '2px 8px',
          transition: 'all 0.2s',
          color: token.colorText,
        }}
        onMouseEnter={(e) => {
          e.currentTarget.style.background = token.colorFillAlter;
        }}
        onMouseLeave={(e) => {
          e.currentTarget.style.background = 'transparent';
        }}
      >
        <span style={{ fontSize: '10px', width: '12px', color: token.colorTextTertiary }}>
          {open ? '▼' : '▶'}
        </span>
        {open ? <FolderOpenOutlined style={{ color: token.colorPrimary, opacity: 0.8 }} /> : <FolderOutlined style={{ color: token.colorTextTertiary }} />}
        <Text strong ellipsis style={{ fontSize: '13px', flex: 1 }}>{node.name}</Text>
        {fileCount > 0 && (
          <Text type="secondary" style={{ fontSize: '11px', opacity: 0.5 }}>{fileCount}</Text>
        )}
      </div>
      {open && node.children && (
        <div>
          {node.children.map(child => (
            <TreeNode
              key={child.path}
              node={child}
              depth={depth + 1}
              selectedPath={selectedPath}
              onSelectDoc={onSelectDoc}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function countFiles(node: DirNode): number {
  if (node.type === 'file') return 1;
  return (node.children ?? []).reduce((s, c) => s + countFiles(c), 0);
}

export function WikiSidebar({
  tree, treeLoading, searchResults, searching,
  selectedPath, activeView,
  onSelectDoc, onSearch, onClearSearch, onShowStats, onShowCategories, onShowHome,
}: Props) {
  const { token } = theme.useToken();
  const [query, setQuery] = useState('');

  useEffect(() => {
    if (!query.trim()) { onClearSearch(); return; }
    const t = setTimeout(() => onSearch(query.trim()), 250);
    return () => clearTimeout(t);
  }, [query, onSearch, onClearSearch]);

  const isSearching = query.trim().length > 0;

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%', background: 'transparent' }}>
      {/* Header */}
      <div style={{ padding: '16px 16px 8px' }}>
        <Space orientation="vertical" style={{ width: '100%' }} size="middle">
          <Button 
            type="text" 
            onClick={onShowHome}
            style={{ padding: 0, height: 'auto' }}
          >
            <Space>
              <BookOutlined style={{ color: token.colorPrimary, fontSize: 16 }} />
              <Text strong style={{ letterSpacing: '1px', textTransform: 'uppercase', fontSize: '12px', opacity: 0.8 }}>
                Knowledge Base
              </Text>
            </Space>
          </Button>

          <Input
            prefix={<SearchOutlined style={{ color: token.colorTextTertiary }} />}
            placeholder="Search wiki..."
            variant="filled"
            size="small"
            value={query}
            onChange={e => setQuery(e.target.value)}
            allowClear
            style={{ 
              borderRadius: '8px',
              background: token.colorFillAlter,
              border: 'none'
            }}
          />
        </Space>
      </div>

      {/* Content */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '8px 0' }}>
        {isSearching ? (
          <div style={{ padding: '0 8px' }}>
            {searching && <Text type="secondary" style={{ padding: '8px 16px', display: 'block', fontSize: '12px' }}>Searching...</Text>}
            {!searching && searchResults.length === 0 && (
              <Text type="secondary" style={{ padding: '8px 16px', display: 'block', fontSize: '12px' }}>No matches</Text>
            )}
            {searchResults.map(r => (
              <div
                key={r.path}
                onClick={() => { onSelectDoc(r.path); setQuery(''); }}
                style={{
                  padding: '10px 16px',
                  cursor: 'pointer',
                  borderRadius: '12px',
                  marginBottom: '4px',
                  background: r.path === selectedPath ? `${token.colorPrimary}15` : 'transparent',
                  transition: 'all 0.2s'
                }}
                onMouseEnter={(e) => {
                  if (r.path !== selectedPath) e.currentTarget.style.background = token.colorFillAlter;
                }}
                onMouseLeave={(e) => {
                  if (r.path !== selectedPath) e.currentTarget.style.background = 'transparent';
                }}
              >
                <Text strong style={{ display: 'block', fontSize: '13px', color: r.path === selectedPath ? token.colorPrimary : token.colorText }}>{r.title}</Text>
                <Text type="secondary" style={{ fontSize: '11px', display: 'block', opacity: 0.6 }} ellipsis>{r.path}</Text>
              </div>
            ))}
          </div>
        ) : (
          <div>
            {treeLoading && <Text type="secondary" style={{ padding: '16px', display: 'block', fontSize: '12px' }}>Loading...</Text>}
            {!treeLoading && tree.length === 0 && (
              <Text type="secondary" style={{ padding: '16px', display: 'block', fontSize: '12px' }}>Wiki is empty</Text>
            )}
            {tree.map(node => (
              <TreeNode
                key={node.path}
                node={node}
                depth={0}
                selectedPath={selectedPath}
                onSelectDoc={onSelectDoc}
              />
            ))}
          </div>
        )}
      </div>

      {/* Bottom Actions */}
      <div style={{ 
        padding: '12px', 
        borderTop: `1px solid ${token.colorBorderSecondary}`,
        display: 'flex',
        gap: '8px'
      }}>
        <Button 
          block
          size="small"
          icon={<LineChartOutlined />}
          onClick={onShowStats}
          type={activeView === 'stats' ? 'primary' : 'default'}
          style={{ borderRadius: '8px', fontSize: '12px' }}
        >
          Stats
        </Button>
        <Button 
          block
          size="small"
          icon={<ClusterOutlined />}
          onClick={onShowCategories}
          type={activeView === 'categories' ? 'primary' : 'default'}
          style={{ borderRadius: '8px', fontSize: '12px' }}
        >
          Tree
        </Button>
      </div>
    </div>
  );
}
