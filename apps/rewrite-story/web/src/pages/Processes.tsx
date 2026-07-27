import { useNavigate } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  App as AntApp,
  Button,
  Card,
  Empty,
  Progress,
  Space,
  Tag,
  Typography,
} from "antd";
import {
  CheckCircleOutlined,
  CloseOutlined,
  DeleteOutlined,
  EyeOutlined,
  RedoOutlined,
} from "@ant-design/icons";

import { api, type ProcessStatus, type RewriteProcess } from "../lib/api";

const { Title, Text, Paragraph } = Typography;

const STATUS_META: Record<
  ProcessStatus,
  { color: string; label: string }
> = {
  queued: { color: "default", label: "Đang chờ" },
  processing: { color: "processing", label: "Đang chạy" },
  completed: { color: "success", label: "Hoàn tất" },
  failed: { color: "error", label: "Thất bại" },
  cancelled: { color: "warning", label: "Đã huỷ" },
};

export default function Processes() {
  const qc = useQueryClient();
  const nav = useNavigate();
  const { message } = AntApp.useApp();

  const processes = useQuery({
    queryKey: ["processes"],
    queryFn: () => api.listProcesses(),
    // The WebSocket drives updates; this is only a safety net in case the
    // socket drops without reconnecting.
    refetchInterval: 30_000,
  });

  const invalidate = () => qc.invalidateQueries({ queryKey: ["processes"] });

  const cancel = useMutation({
    mutationFn: api.cancelProcess,
    onSuccess: () => {
      message.success("Đã huỷ — các chunk đã xong vẫn được giữ");
      invalidate();
    },
    onError: (e: Error) => message.error(e.message),
  });

  const retry = useMutation({
    mutationFn: api.retryProcess,
    onSuccess: (r) => {
      message.success(`Chạy tiếp từ chunk ${r.resuming_from_chunk + 1}`);
      invalidate();
    },
    onError: (e: Error) => message.error(e.message),
  });

  const remove = useMutation({
    mutationFn: api.deleteProcess,
    onSuccess: () => {
      message.success("Đã xoá tiến trình");
      invalidate();
    },
    onError: (e: Error) => message.error(e.message),
  });

  const items = processes.data ?? [];

  return (
    <>
      <Title level={3} style={{ marginTop: 0, marginBottom: 4 }}>
        Tiến trình viết lại
      </Title>
      <Paragraph type="secondary" style={{ marginBottom: 20 }}>
        Mỗi tiến trình viết lại một truyện, lưu từng chunk khi xong — hỏng giữa
        chừng vẫn chạy tiếp được.
      </Paragraph>

      {items.length === 0 ? (
        <Card style={{ padding: "40px 0" }}>
          <Empty
            image={Empty.PRESENTED_IMAGE_SIMPLE}
            description="Chưa có tiến trình nào. Mở một truyện và bấm “Bắt đầu viết lại”."
          />
        </Card>
      ) : (
        <Space orientation="vertical" size={12} style={{ width: "100%" }}>
          {items.map((p: RewriteProcess) => {
            const meta = STATUS_META[p.status];
            const active = p.status === "queued" || p.status === "processing";
            const resumable = p.status === "failed" || p.status === "cancelled";
            return (
              <Card key={p.id} size="small">
                <Space
                  align="center"
                  style={{ width: "100%", justifyContent: "space-between", marginBottom: 8 }}
                >
                  <Space size={10} wrap>
                    <Text strong>#{p.id}</Text>
                    <Tag color={meta.color}>{meta.label}</Tag>
                    {p.total_chunks > 0 && (
                      <Text type="secondary">
                        phần {p.current_chunk}/{p.total_chunks}
                      </Text>
                    )}
                  </Space>
                  <Space>
                    {active && (
                      <Button size="small" danger icon={<CloseOutlined />} onClick={() => cancel.mutate(p.id)}>
                        Huỷ
                      </Button>
                    )}
                    {resumable && (
                      <Button
                        size="small"
                        type="primary"
                        icon={<RedoOutlined />}
                        onClick={() => retry.mutate(p.id)}
                      >
                        Chạy tiếp
                      </Button>
                    )}
                    {p.result_story_id && (
                      <Button
                        size="small"
                        icon={<EyeOutlined />}
                        onClick={() => nav(`/stories/${p.result_story_id}`)}
                      >
                        Xem kết quả
                      </Button>
                    )}
                    {!active && (
                      <Button
                        size="small"
                        type="text"
                        icon={<DeleteOutlined />}
                        onClick={() => remove.mutate(p.id)}
                      />
                    )}
                  </Space>
                </Space>

                <Progress
                  percent={p.progress_percentage}
                  status={
                    p.status === "failed"
                      ? "exception"
                      : p.status === "processing"
                        ? "active"
                        : p.status === "completed"
                          ? "success"
                          : "normal"
                  }
                />

                {p.version_plan && (
                  <Paragraph type="secondary" ellipsis={{ rows: 2 }} style={{ marginBottom: 0, marginTop: 4 }}>
                    Phong cách: {p.version_plan}
                  </Paragraph>
                )}

                {p.status === "completed" && (
                  <Text type="success">
                    <CheckCircleOutlined /> Đã tạo bản viết lại mới.
                  </Text>
                )}

                {p.error_message && (
                  <Alert
                    style={{ marginTop: 8 }}
                    type={p.status === "cancelled" ? "warning" : "error"}
                    showIcon
                    title={p.error_message}
                    description={
                      resumable
                        ? 'Các chunk đã hoàn thành vẫn được giữ — bấm "Chạy tiếp" để tiếp tục thay vì làm lại từ đầu.'
                        : undefined
                    }
                  />
                )}
              </Card>
            );
          })}
        </Space>
      )}
    </>
  );
}
