import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  App as AntApp,
  Button,
  Card,
  Divider,
  Form,
  Input,
  InputNumber,
  List,
  Skeleton,
  Slider,
  Space,
  Statistic,
  Tag,
  Typography,
} from "antd";

import { DownloadOutlined } from "@ant-design/icons";

import { api, type StartRewriteReq } from "../lib/api";

const { Paragraph } = Typography;

/** Rough wall-clock estimate so the user isn't surprised by a long run. */
function estimateMinutes(chunks: number) {
  const low = Math.round((chunks * 20) / 60);
  const high = Math.round((chunks * 50) / 60);
  return `${Math.max(low, 1)}–${Math.max(high, 2)} phút`;
}

export default function StoryDetail() {
  const { id } = useParams();
  const storyId = Number(id);
  const nav = useNavigate();
  const qc = useQueryClient();
  const { message } = AntApp.useApp();
  const [form] = Form.useForm<Omit<StartRewriteReq, "story_id">>();
  const [showText, setShowText] = useState(false);
  const [sceneChars, setSceneChars] = useState(900);

  const story = useQuery({
    queryKey: ["stories", storyId],
    queryFn: () => api.getStory(storyId),
    enabled: Number.isFinite(storyId),
  });
  const versions = useQuery({
    queryKey: ["stories", storyId, "versions"],
    queryFn: () => api.listVersions(storyId),
    enabled: Number.isFinite(storyId),
  });
  const chunks = useQuery({
    queryKey: ["stories", storyId, "chunks"],
    queryFn: () => api.storyChunks(storyId),
    enabled: Number.isFinite(storyId),
  });

  const start = useMutation({
    mutationFn: (v: Omit<StartRewriteReq, "story_id">) =>
      api.startRewrite({ ...v, story_id: storyId }),
    onSuccess: () => {
      message.success("Đã đưa vào hàng chờ — theo dõi ở tab Tiến trình");
      qc.invalidateQueries({ queryKey: ["processes"] });
      // The run is about to freeze this story's split.
      qc.invalidateQueries({ queryKey: ["stories", storyId, "chunks"] });
      nav("/processes");
    },
    onError: (e: Error) => message.error(e.message),
  });

  if (story.isError) return <Alert type="error" title={String(story.error)} />;
  // Guard on the data itself, not on `isLoading`. A *disabled* query (which is
  // what a non-numeric :id produces) reports `isLoading === false` and
  // `isError === false` in react-query v5, so trusting those and using
  // `story.data!` threw on undefined and blanked the whole app.
  if (!story.data) {
    return Number.isFinite(storyId) ? (
      <Skeleton active />
    ) : (
      <Alert type="error" title={`Id truyện không hợp lệ: ${id}`} />
    );
  }
  const s = story.data;

  const longest = Math.max(0, ...(chunks.data?.chunks.map((c) => c.length) ?? [0]));
  const chunkCount = chunks.data?.total ?? 0;

  return (
    <>
      <Space style={{ marginBottom: 16 }}>
        <Button onClick={() => nav("/stories")}>← Kho truyện</Button>
        <Typography.Title level={4} style={{ margin: 0 }}>
          {s.name}
        </Typography.Title>
        <Tag color={s.source_type === "ai" ? "purple" : "blue"}>
          {s.source_type === "ai" ? `Bản viết lại v${s.version_number}` : "Bản gốc"}
        </Tag>
      </Space>

      <Card size="small" style={{ marginBottom: 16 }}>
        <Space size="large" wrap>
          <Statistic
            title="Độ dài"
            value={s.original_length}
            suffix="ký tự"
            formatter={(v) => Number(v).toLocaleString("vi-VN")}
          />
          <Statistic title="Số chunk" value={chunkCount || "—"} />
          <Statistic
            title="Chunk dài nhất"
            value={longest || "—"}
            formatter={(v) => (longest ? Number(v).toLocaleString("vi-VN") : "—")}
          />
          <Statistic title="Bản viết lại" value={versions.data?.length ?? 0} />
        </Space>
        {chunks.data && !chunks.data.persisted && (
          <Alert
            style={{ marginTop: 12 }}
            type="info"
            showIcon
            title="Đây mới là xem thử cách cắt chunk. Chunk sẽ được lưu cố định khi bắt đầu viết lại lần đầu — đổi cấu hình sau đó sẽ không cắt lại."
          />
        )}
        {longest > 20000 && (
          <Alert
            style={{ marginTop: 12 }}
            type="warning"
            showIcon
            title="Chunk dài bất thường"
            description="Chunk quá dài dễ khiến model cắt output giữa chừng. Cân nhắc giảm hybrid_split_max_size trong Cấu hình rồi nhập lại truyện."
          />
        )}
      </Card>

      {s.source_type === "human" && (
        <Card title="Viết lại truyện này" style={{ marginBottom: 16 }}>
          <Form
            form={form}
            layout="vertical"
            initialValues={{ creativity_ratio: 40, target_length_variance: 5 }}
            onFinish={(v) => start.mutate(v)}
          >
            <Form.Item
              name="version_plan"
              label="Phong cách / kế hoạch cho bản mới"
              tooltip="Chỉ dẫn quan trọng nhất — quyết định bản mới khác bản cũ ra sao."
              rules={[{ required: true, message: "Hãy mô tả phong cách mong muốn" }]}
            >
              <Input.TextArea
                rows={3}
                placeholder="Ví dụ: giọng cổ trang, tiết tấu nhanh hơn, giảm miêu tả nội tâm…"
              />
            </Form.Item>
            <Form.Item name="user_prompt" label="Yêu cầu thêm">
              <Input.TextArea
                rows={2}
                placeholder="Ví dụ: giữ nguyên tên nhân vật, bỏ các cảnh bạo lực…"
              />
            </Form.Item>
            <Form.Item
              name="creativity_ratio"
              label="Mức sáng tạo — được phép xa bản gốc tới đâu"
            >
              <Slider
                min={0}
                max={100}
                marks={{
                  20: "Trau chuốt",
                  40: "Giữ cốt truyện",
                  60: "Đổi chi tiết",
                  85: "Tự do",
                }}
              />
            </Form.Item>
            <Form.Item name="target_length_variance" label="Dung sai độ dài (%)">
              <Slider min={0} max={50} />
            </Form.Item>
            <Space>
              <Button type="primary" htmlType="submit" loading={start.isPending}>
                Bắt đầu viết lại
              </Button>
              {chunkCount > 0 && (
                <Typography.Text type="secondary">
                  {chunkCount} chunk — ước tính {estimateMinutes(chunkCount)}
                </Typography.Text>
              )}
            </Space>
          </Form>
        </Card>
      )}

      {(versions.data?.length ?? 0) > 0 && (
        <Card title="Các bản viết lại" style={{ marginBottom: 16 }}>
          <List
            dataSource={versions.data ?? []}
            rowKey="id"
            renderItem={(v) => (
              <List.Item
                actions={[
                  <Button key="open" onClick={() => nav(`/stories/${v.id}`)}>
                    Mở
                  </Button>,
                ]}
              >
                <List.Item.Meta
                  title={`Phiên bản ${v.version_number}`}
                  description={`${v.original_length.toLocaleString("vi-VN")} ký tự · ${v.created_at}`}
                />
              </List.Item>
            )}
          />
        </Card>
      )}

      <Card title="Xuất sang app làm video" style={{ marginBottom: 16 }}>
        <Paragraph type="secondary">
          Kịch bản (.md) cắt truyện thành các cảnh theo heading <code>#&nbsp;Cảnh&nbsp;N</code> —
          đúng định dạng Video Flow nạp vào <code>vf_pipeline_create</code> (mode
          <code> production</code>) hoặc <code>/api/script/parse</code>. Tải file rồi
          kéo sang app làm video, hoặc bảo trợ lý: “xuất truyện này sang Video Flow”.
        </Paragraph>
        <Space wrap align="center">
          <span>
            Số ký tự mỗi cảnh:{" "}
            <InputNumber
              min={200}
              max={5000}
              step={100}
              value={sceneChars}
              onChange={(v) => setSceneChars(Number(v) || 900)}
              style={{ width: 110 }}
            />
          </span>
          <Button
            type="primary"
            icon={<DownloadOutlined />}
            href={`/api/stories/${s.id}/export?format=screenplay&scene_chars=${sceneChars}`}
            download
          >
            Tải kịch bản (.md)
          </Button>
          <Button
            icon={<DownloadOutlined />}
            href={`/api/stories/${s.id}/export?format=json&scene_chars=${sceneChars}`}
            download
          >
            JSON cảnh
          </Button>
          <Button
            icon={<DownloadOutlined />}
            href={`/api/stories/${s.id}/export?format=txt`}
            download
          >
            Toàn văn (.txt)
          </Button>
        </Space>
        <Paragraph type="secondary" style={{ marginTop: 12, marginBottom: 0 }}>
          Ước tính {Math.max(1, Math.ceil(s.total_length / sceneChars))} cảnh ·{" "}
          {Math.max(1, Math.round((Math.ceil(s.total_length / sceneChars) * 8) / 60))} phút video
          (8 giây mỗi cảnh).
        </Paragraph>
      </Card>

      <Card
        title="Nội dung"
        extra={
          <Button onClick={() => setShowText((x) => !x)}>
            {showText ? "Ẩn" : "Đọc"}
          </Button>
        }
      >
        {showText ? (
          <>
            {/* The server already windowed this — see GET /api/stories/:id. */}
            <Paragraph className="story-text">{s.original_text}</Paragraph>
            {s.has_more && (
              <>
                <Divider />
                <Typography.Text type="secondary">
                  Đang hiển thị {s.original_text.length.toLocaleString("vi-VN")} ký
                  tự đầu trên tổng {s.total_length.toLocaleString("vi-VN")}.
                </Typography.Text>
              </>
            )}
          </>
        ) : (
          <Typography.Text type="secondary">
            {s.original_length.toLocaleString("vi-VN")} ký tự — bấm "Đọc" để xem.
          </Typography.Text>
        )}
      </Card>
    </>
  );
}
