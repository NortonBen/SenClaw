import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { EllipsisOutlined } from "@ant-design/icons";
import {
  Alert,
  Button,
  Card,
  Dropdown,
  Form,
  Input,
  Modal,
  Space,
  Table,
  Typography,
  type MenuProps,
  message,
} from "antd";
import { useCallback, useState } from "react";
import { api, type ProjectRow } from "@/lib/api/client";

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

type Props = {
  onOpenPipeline: (projectId: string) => void;
  onOpenCreateProject: () => void;
  onOpenDetail: (projectId: string) => void;
};

export function ProjectsPage({
  onOpenPipeline,
  onOpenCreateProject,
  onOpenDetail,
}: Props) {
  const qc = useQueryClient();
  const [err, setErr] = useState<string | null>(null);
  const [editId, setEditId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editStory, setEditStory] = useState("");

  const q = useQuery({
    queryKey: ["projects"],
    queryFn: () => api.listProjects(),
  });

  const patchM = useMutation({
    mutationFn: (args: { id: string; name: string; story: string }) =>
      api.updateProject(args.id, {
        name: args.name.trim(),
        story: args.story.trim() !== "" ? args.story.trim() : null,
      }),
    onSuccess: () => {
      setErr(null);
      setEditId(null);
      void qc.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const delM = useMutation({
    mutationFn: (id: string) => api.deleteProject(id),
    onSuccess: () => {
      setErr(null);
      void qc.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const duplicateM = useMutation({
    mutationFn: (id: string) => api.duplicateProject(id),
    onSuccess: () => {
      setErr(null);
      message.success("Đã duplicate project.");
      void qc.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const cloneAIM = useMutation({
    mutationFn: (id: string) => api.cloneProjectAI(id),
    onSuccess: () => {
      setErr(null);
      message.success("Đã clone project bằng AI.");
      void qc.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const startEdit = useCallback((row: ProjectRow) => {
    setEditId(str(row.id));
    setEditName(str(row.name));
    setEditStory(str(row.story ?? ""));
  }, []);

  const rows = (q.data ?? []) as ProjectRow[];

  return (
    <div className="layout layout-wide">
      <Space direction="vertical" size={16} style={{ width: "100%" }}>
        <Space style={{ width: "100%", justifyContent: "space-between" }}>
          <Typography.Title level={3} style={{ margin: 0 }}>
            Projects
          </Typography.Title>
          <Button type="primary" onClick={onOpenCreateProject}>
            Tạo project mới
          </Button>
        </Space>
        {err && <Alert type="error" message={err} showIcon />}
        <Card title="Danh sách">
          <Table
            rowKey={(row) => str((row as ProjectRow).id)}
            loading={q.isLoading}
            dataSource={rows}
            pagination={{ pageSize: 10 }}
            columns={[
              { title: "Tên", dataIndex: "name", key: "name" },
              { title: "Material", dataIndex: "material", key: "material", render: (v: unknown) => <Typography.Text code>{str(v)}</Typography.Text> },
              { title: "ID", dataIndex: "id", key: "id", render: (v: unknown) => <Typography.Text code>{str(v).slice(0, 8)}…</Typography.Text> },
              {
                title: "Hành động",
                key: "actions",
                render: (_: unknown, row: ProjectRow) => {
                  const id = str(row.id);
                  const actionItems: MenuProps["items"] = [
                    {
                      key: "edit",
                      label: "Sửa",
                      onClick: () => startEdit(row),
                    },
                    {
                      key: "duplicate",
                      label: "Duplicate",
                      onClick: () => duplicateM.mutate(id),
                    },
                    {
                      key: "clone-ai",
                      label: "Clone bằng AI",
                      onClick: () => cloneAIM.mutate(id),
                    },
                    {
                      key: "delete",
                      danger: true,
                      label: "Xoá",
                      onClick: () => {
                        Modal.confirm({
                          title: `Xoá project "${str(row.name)}"?`,
                          okText: "Xoá",
                          okButtonProps: { danger: true },
                          cancelText: "Huỷ",
                          onOk: () => delM.mutate(id),
                        });
                      },
                    },
                  ];
                  return (
                    <Space>
                      <Button type="primary" onClick={() => onOpenDetail(id)}>Mở</Button>
                      <Button onClick={() => onOpenPipeline(id)}>Studio</Button>
                      <Dropdown menu={{ items: actionItems }} trigger={["click"]}>
                        <Button icon={<EllipsisOutlined />} loading={duplicateM.isPending || cloneAIM.isPending || delM.isPending}>
                        </Button>
                      </Dropdown>
                    </Space>
                  );
                },
              },
            ]}
          />
        </Card>
      </Space>

      <Modal
        open={!!editId}
        title={editId ? `Sửa project ${editId.slice(0, 8)}…` : "Sửa project"}
        onCancel={() => setEditId(null)}
        onOk={() => {
          if (!editId) return;
          patchM.mutate({ id: editId, name: editName, story: editStory });
        }}
        confirmLoading={patchM.isPending}
        okButtonProps={{ disabled: !editName.trim() }}
      >
        <Form layout="vertical">
          <Form.Item label="Tên project">
            <Input value={editName} onChange={(e) => setEditName(e.target.value)} />
          </Form.Item>
          <Form.Item label="Story">
            <Input.TextArea rows={4} value={editStory} onChange={(e) => setEditStory(e.target.value)} />
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
