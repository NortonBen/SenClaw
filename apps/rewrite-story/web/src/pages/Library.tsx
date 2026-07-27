import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  App as AntApp,
  Button,
  Card,
  Col,
  Empty,
  Form,
  Input,
  Modal,
  Popconfirm,
  Row,
  Skeleton,
  Space,
  Tag,
  Typography,
  Upload,
} from "antd";
import {
  DeleteOutlined,
  FileTextOutlined,
  InboxOutlined,
  PlusOutlined,
} from "@ant-design/icons";

import { api, type StorySummary } from "../lib/api";

const { Paragraph, Text, Title } = Typography;

export default function Library() {
  const nav = useNavigate();
  const qc = useQueryClient();
  const { message } = AntApp.useApp();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<{ name: string; text: string }>();

  const stories = useQuery({ queryKey: ["stories"], queryFn: api.listStories });

  const create = useMutation({
    mutationFn: (v: { name: string; text: string }) => api.createStory(v.name, v.text),
    onSuccess: (s) => {
      message.success(`Đã nhập "${s.name}"`);
      setOpen(false);
      form.resetFields();
      qc.invalidateQueries({ queryKey: ["stories"] });
      nav(`/stories/${s.id}`);
    },
    onError: (e: Error) => message.error(e.message),
  });

  const remove = useMutation({
    mutationFn: api.deleteStory,
    onSuccess: () => {
      message.success("Đã xoá truyện");
      qc.invalidateQueries({ queryKey: ["stories"] });
    },
    onError: (e: Error) => message.error(e.message),
  });

  const list = stories.data ?? [];

  return (
    <>
      <Row align="middle" justify="space-between" style={{ marginBottom: 20 }}>
        <Col>
          <Title level={3} style={{ margin: 0 }}>
            Kho truyện
          </Title>
          <Text type="secondary">
            {list.length > 0
              ? `${list.length} truyện gốc`
              : "Nhập truyện gốc để bắt đầu viết lại"}
          </Text>
        </Col>
        <Col>
          <Button type="primary" size="large" icon={<PlusOutlined />} onClick={() => setOpen(true)}>
            Nhập truyện
          </Button>
        </Col>
      </Row>

      {stories.isLoading ? (
        <Row gutter={[16, 16]}>
          {[0, 1, 2].map((i) => (
            <Col xs={24} md={12} xl={8} key={i}>
              <Card>
                <Skeleton active paragraph={{ rows: 2 }} />
              </Card>
            </Col>
          ))}
        </Row>
      ) : list.length === 0 ? (
        <Card style={{ padding: "48px 0" }}>
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description={
              <Space orientation="vertical" size={4}>
                <Text strong>Chưa có truyện nào</Text>
                <Text type="secondary">
                  Nhập một truyện gốc, chọn phong cách, rồi để AI viết lại từng chương.
                </Text>
              </Space>
            }
          >
            <Button type="primary" icon={<PlusOutlined />} onClick={() => setOpen(true)}>
              Nhập truyện đầu tiên
            </Button>
          </Empty>
        </Card>
      ) : (
        <Row gutter={[16, 16]}>
          {list.map((s: StorySummary) => (
            <Col xs={24} md={12} xl={8} key={s.id}>
              <Card
                className="rs-card-clickable"
                style={{ height: "100%" }}
                onClick={() => nav(`/stories/${s.id}`)}
                actions={[
                  <span key="open">
                    <FileTextOutlined /> Mở
                  </span>,
                  <Popconfirm
                    key="del"
                    title="Xoá truyện này?"
                    description="Xoá luôn mọi bản viết lại và tiến trình của nó."
                    okText="Xoá"
                    okButtonProps={{ danger: true }}
                    cancelText="Huỷ"
                    onConfirm={() => remove.mutate(s.id)}
                  >
                    <span
                      style={{ color: "#ff7875" }}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <DeleteOutlined /> Xoá
                    </span>
                  </Popconfirm>,
                ]}
              >
                <Card.Meta
                  title={s.name}
                  description={
                    <Space orientation="vertical" size={10} style={{ width: "100%" }}>
                      <Space size={[6, 6]} wrap>
                        <Tag color="blue">
                          {s.original_length.toLocaleString("vi-VN")} ký tự
                        </Tag>
                        {s.version_count > 0 && (
                          <Tag color="purple">{s.version_count} bản viết lại</Tag>
                        )}
                      </Space>
                      <Paragraph
                        type="secondary"
                        ellipsis={{ rows: 3 }}
                        style={{ margin: 0, minHeight: 66 }}
                      >
                        {s.preview}
                      </Paragraph>
                    </Space>
                  }
                />
              </Card>
            </Col>
          ))}
        </Row>
      )}

      <Modal
        title="Nhập truyện"
        open={open}
        onCancel={() => setOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={create.isPending}
        okText="Nhập"
        cancelText="Huỷ"
        width={720}
        destroyOnHidden
      >
        <Form form={form} layout="vertical" onFinish={(v) => create.mutate(v)}>
          <Form.Item name="name" label="Tên truyện">
            <Input placeholder="Truyện chưa đặt tên" allowClear />
          </Form.Item>
          <Form.Item label="Tải từ file .txt">
            <Upload.Dragger
              accept=".txt,.md"
              maxCount={1}
              beforeUpload={async (file) => {
                const text = await file.text();
                form.setFieldsValue({
                  text,
                  name: form.getFieldValue("name") || file.name.replace(/\.[^.]+$/, ""),
                });
                message.success(
                  `Đã đọc ${text.length.toLocaleString("vi-VN")} ký tự từ ${file.name}`
                );
                // Never upload — the file is read entirely in the browser.
                return Upload.LIST_IGNORE;
              }}
            >
              <p className="ant-upload-drag-icon">
                <InboxOutlined />
              </p>
              <p className="ant-upload-text">Kéo file vào đây hoặc bấm để chọn</p>
              <p className="ant-upload-hint">Hỗ trợ .txt và .md</p>
            </Upload.Dragger>
          </Form.Item>
          <Form.Item
            name="text"
            label="Nội dung"
            rules={[{ required: true, message: "Nội dung không được rỗng" }]}
          >
            <Input.TextArea rows={10} placeholder="Dán toàn văn truyện gốc vào đây…" />
          </Form.Item>
          <Text type="secondary">
            Truyện dài hàng triệu ký tự đều nhập được — việc cắt chunk diễn ra khi
            bắt đầu viết lại.
          </Text>
        </Form>
      </Modal>
    </>
  );
}
