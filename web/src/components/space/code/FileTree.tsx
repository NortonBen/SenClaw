import React, { useState } from 'react';
import { Tree, theme, Spin, Empty } from 'antd';
import { FolderOutlined, FolderOpenOutlined, FileOutlined } from '@ant-design/icons';
import type { FileNode } from '../../../hooks/useCode';

interface Props {
  tree: FileNode[];
  loading?: boolean;
  onSelect?: (path: string) => void;
  selectedPath?: string | null;
}

function buildTreeData(nodes: FileNode[], parentPath = ''): any[] {
  return nodes.map(node => ({
    key: node.path || `${parentPath}/${node.name}`,
    title: node.name,
    icon: ({ expanded }: { expanded: boolean }) =>
      node.type === 'dir'
        ? expanded ? <FolderOpenOutlined /> : <FolderOutlined />
        : <FileOutlined />,
    isLeaf: node.type === 'file',
    children: node.children ? buildTreeData(node.children, node.path) : undefined,
    _isFile: node.type === 'file',
    _path: node.path,
  }));
}

export function FileTree({ tree, loading, onSelect, selectedPath }: Props) {
  const { token } = theme.useToken();
  const [expandedKeys, setExpandedKeys] = useState<string[]>([]);

  if (loading) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', padding: 24 }}>
        <Spin size="small" />
      </div>
    );
  }

  if (!tree || tree.length === 0) {
    return <Empty description="No files" image={Empty.PRESENTED_IMAGE_SIMPLE} style={{ padding: '16px 0' }} />;
  }

  const treeData = buildTreeData(tree);

  return (
    <>
      <Tree
        showIcon
        blockNode
        className="code-file-tree"
        treeData={treeData}
        expandedKeys={expandedKeys}
        selectedKeys={selectedPath ? [selectedPath] : []}
        onExpand={(keys) => setExpandedKeys(keys as string[])}
        onSelect={(keys, info) => {
          const node = info.node as any;
          if (node._isFile && onSelect) {
            onSelect(node._path);
          }
        }}
        style={{
          background: 'transparent',
          fontSize: 13,
          userSelect: 'none',
        }}
      />
      <style>{`
        .code-file-tree .ant-tree-treenode {
          margin-bottom: 2px;
        }
        .code-file-tree .ant-tree-node-content-wrapper {
          border-radius: 8px;
          min-height: 28px;
          display: inline-flex;
          align-items: center;
          transition: background 0.2s ease;
        }
        .code-file-tree .ant-tree-node-content-wrapper:hover {
          background: ${token.colorFillAlter};
        }
        .code-file-tree .ant-tree-node-selected {
          background: ${token.colorPrimaryBg} !important;
          color: ${token.colorPrimary} !important;
        }
      `}</style>
    </>
  );
}
