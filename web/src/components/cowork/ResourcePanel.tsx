import React, { useState, useEffect, useCallback } from 'react';
import {
  Tabs, Tree, Button, Empty, Spin, Tag, Tooltip, Modal, Form,
  Input, Select, Space, Typography, message,
} from 'antd';
import {
  FolderOutlined, FileOutlined, ReloadOutlined, PlusOutlined,
  DeleteOutlined, FolderOpenOutlined, EditOutlined,
} from '@ant-design/icons';
import type { WorkspaceResource, ResourceKind } from '../../types';

const { Text } = Typography;

const KIND_CONFIG: Record<ResourceKind, { label: string; color: string; desc: string }> = {
  raw:       { label: 'Raw',       color: 'blue',    desc: 'Tài liệu gốc chưa xử lý' },
  wiki:      { label: 'Wiki',      color: 'green',   desc: 'Tài liệu đã biên soạn' },
  reference: { label: 'Reference', color: 'purple',  desc: 'Tài liệu tham khảo hệ thống' },
  workdir:   { label: 'Workdir',   color: 'orange',  desc: 'File làm việc & kết quả cuối' },
};

interface FileNode {
  key: string;
  title: string;
  isLeaf?: boolean;
  children?: FileNode[];
}

interface ResourcePanelProps {
  workspaceId: string;
  resourceChanged?: number;
}

async function fetchResources(workspaceId: string): Promise<WorkspaceResource[]> {
  const res = await fetch(`/api/cowork/workspaces/${workspaceId}/resources`);
  if (!res.ok) return [];
  const data = await res.json();
  return (data.resources ?? []) as WorkspaceResource[];
}

async function browseDir(workspaceId: string, path: string): Promise<FileNode[]> {
  const res = await fetch(
    `/api/cowork/workspaces/${workspaceId}/browse?path=${encodeURIComponent(path)}`,
  );
  if (!res.ok) return [];
  const data = await res.json();
  const entries: { name: string; isDir: boolean; path: string }[] = data.entries ?? [];
  return entries.map(e => ({
    key: e.path,
    title: e.name,
    isLeaf: !e.isDir,
    children: e.isDir ? [] : undefined,
  }));
}

export function ResourcePanel({ workspaceId, resourceChanged }: ResourcePanelProps) {
  const [resources, setResources] = useState<WorkspaceResource[]>([]);
  const [loading, setLoading]     = useState(false);
  const [treeData, setTreeData]   = useState<Record<ResourceKind, FileNode[]>>({
    raw: [], wiki: [], reference: [], workdir: [],
  });
  const [treeLoading, setTreeLoading] = useState<Record<ResourceKind, boolean>>({
    raw: false, wiki: false, reference: false, workdir: false,
  });
  const [activeKind, setActiveKind] = useState<ResourceKind>('workdir');
  const [editModal, setEditModal]   = useState(false);
  const [form] = Form.useForm();

  const load = useCallback(async () => {
    setLoading(true);
    const res = await fetchResources(workspaceId);
    setResources(res);
    setLoading(false);
  }, [workspaceId]);

  useEffect(() => { load(); }, [load, resourceChanged]);

  const loadTree = useCallback(async (kind: ResourceKind) => {
    const r = resources.find(x => x.kind === kind);
    if (!r) return;
    setTreeLoading(p => ({ ...p, [kind]: true }));
    const nodes = await browseDir(workspaceId, r.path);
    setTreeData(p => ({ ...p, [kind]: nodes }));
    setTreeLoading(p => ({ ...p, [kind]: false }));
  }, [workspaceId, resources]);

  useEffect(() => {
    if (resources.length > 0) loadTree(activeKind);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [resources, activeKind]);

  const onLoadData = useCallback(async ({ key, children }: { key: React.Key; children?: FileNode[] }) => {
    if (children && children.length > 0) return;
    const nodes = await browseDir(workspaceId, key as string);
    setTreeData(p => {
      const patch = (list: FileNode[]): FileNode[] =>
        list.map(n => n.key === key ? { ...n, children: nodes } : { ...n, children: n.children ? patch(n.children) : undefined });
      return { ...p, [activeKind]: patch(p[activeKind]) };
    });
  }, [workspaceId, activeKind]);

  const handleUpsert = async (values: { kind: ResourceKind; path: string }) => {
    const res = await fetch(`/api/cowork/workspaces/${workspaceId}/resources`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(values),
    });
    if (res.ok) {
      message.success('Resource path saved');
      setEditModal(false);
      form.resetFields();
      load();
    } else {
      const e = await res.json().catch(() => ({}));
      message.error(e.error ?? 'Failed to save');
    }
  };

  const handleDelete = async (kind: ResourceKind) => {
    const res = await fetch(`/api/cowork/workspaces/${workspaceId}/resources/${kind}`, {
      method: 'DELETE',
    });
    if (res.ok) {
      message.success('Resource removed');
      load();
    }
  };

  const tabs = (Object.keys(KIND_CONFIG) as ResourceKind[]).map(kind => {
    const cfg   = KIND_CONFIG[kind];
    const res   = resources.find(r => r.kind === kind);
    const nodes = treeData[kind];
    const busy  = treeLoading[kind];

    return {
      key: kind,
      label: (
        <Space size={4}>
          <Tag color={cfg.color} style={{ margin: 0 }}>{cfg.label}</Tag>
        </Space>
      ),
      children: (
        <div style={{ height: '100%', display: 'flex', flexDirection: 'column', gap: 8 }}>
          <Space style={{ padding: '0 4px' }} wrap>
            {res ? (
              <>
                <Text type="secondary" style={{ fontSize: 11 }} ellipsis>
                  <FolderOpenOutlined /> {res.path}
                </Text>
                <Button
                  size="small"
                  icon={<ReloadOutlined />}
                  onClick={() => loadTree(kind)}
                  loading={busy}
                />
                <Tooltip title="Change path">
                  <Button
                    size="small"
                    icon={<EditOutlined />}
                    onClick={() => {
                      form.setFieldsValue({ kind, path: res.path });
                      setEditModal(true);
                    }}
                  />
                </Tooltip>
                <Tooltip title="Remove resource">
                  <Button
                    size="small"
                    danger
                    icon={<DeleteOutlined />}
                    onClick={() => handleDelete(kind)}
                  />
                </Tooltip>
              </>
            ) : (
              <Button
                size="small"
                icon={<PlusOutlined />}
                onClick={() => {
                  form.setFieldsValue({ kind, path: '' });
                  setEditModal(true);
                }}
              >
                Set {cfg.label} path
              </Button>
            )}
          </Space>

          {loading || busy ? (
            <div style={{ textAlign: 'center', paddingTop: 40 }}>
              <Spin />
            </div>
          ) : nodes.length === 0 ? (
            <Empty
              image={Empty.PRESENTED_IMAGE_SIMPLE}
              description={res ? 'Empty directory' : cfg.desc}
              style={{ marginTop: 40 }}
            />
          ) : (
            <Tree
              treeData={nodes}
              loadData={onLoadData as any}
              showIcon
              icon={({ isLeaf }: { isLeaf?: boolean }) =>
                isLeaf ? <FileOutlined /> : <FolderOutlined />
              }
              style={{ fontSize: 12, overflow: 'auto', flex: 1 }}
            />
          )}
        </div>
      ),
    };
  });

  return (
    <>
      <Tabs
        size="small"
        activeKey={activeKind}
        onChange={k => setActiveKind(k as ResourceKind)}
        items={tabs}
        style={{ height: '100%' }}
      />

      <Modal
        title="Set resource path"
        open={editModal}
        onCancel={() => setEditModal(false)}
        footer={null}
        width={480}
      >
        <Form form={form} layout="vertical" onFinish={handleUpsert}>
          <Form.Item name="kind" label="Kind" rules={[{ required: true }]}>
            <Select
              options={(Object.keys(KIND_CONFIG) as ResourceKind[]).map(k => ({
                value: k,
                label: KIND_CONFIG[k].label,
              }))}
            />
          </Form.Item>
          <Form.Item name="path" label="Absolute path" rules={[{ required: true }]}>
            <Input placeholder="/path/to/directory" />
          </Form.Item>
          <Form.Item>
            <Space>
              <Button type="primary" htmlType="submit">Save</Button>
              <Button onClick={() => setEditModal(false)}>Cancel</Button>
            </Space>
          </Form.Item>
        </Form>
      </Modal>
    </>
  );
}
