import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Alert,
  Avatar,
  Badge,
  Button,
  Card,
  Col,
  Form,
  Input,
  Modal,
  Popconfirm,
  Radio,
  Row,
  Select,
  Space,
  Spin,
  Tabs,
  Tag,
  Typography,
} from "antd";
import { BranchesOutlined, NodeIndexOutlined } from "@ant-design/icons";
import {
  ArrowLeftOutlined,
  EditOutlined,
  PlusOutlined,
  UserOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { useState } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { api, type CharacterRow, type ProjectRow, type VideoRow } from "@/lib/api/client";

function str(v: unknown): string {
  return typeof v === "string" ? v : v == null ? "" : String(v);
}

const ENTITY_TYPE_LABELS: Record<string, string> = {
  character: "Nhân vật",
  location: "Địa điểm",
  creature: "Sinh vật",
  visual_asset: "Tài sản",
  generic_troop: "Quân đội",
  faction: "Phe",
};

const VIDEO_STATUS: Record<string, { color: "default" | "processing" | "success" | "error"; label: string }> = {
  DRAFT: { color: "default", label: "Draft" },
  PROCESSING: { color: "processing", label: "Đang xử lý" },
  COMPLETED: { color: "success", label: "Hoàn thành" },
  FAILED: { color: "error", label: "Lỗi" },
};

type Props = {
  onOpenPipeline: (projectId: string, opts?: { videoId?: string }) => void;
  onOpenSmartPipeline: (projectId: string) => void;
};

export function ProjectDetailPage({ onOpenPipeline, onOpenSmartPipeline }: Props) {
  const { id: projectId = "" } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const qc = useQueryClient();

  const projectQ = useQuery({
    queryKey: ["projects", projectId],
    queryFn: () => api.getProject(projectId),
    enabled: !!projectId,
  });

  const charsQ = useQuery({
    queryKey: ["project-characters", projectId],
    queryFn: () => api.listProjectCharacters(projectId),
    enabled: !!projectId,
  });

  const videosQ = useQuery({
    queryKey: ["videos", projectId],
    queryFn: () => api.listVideos(projectId),
    enabled: !!projectId,
  });

  const [editOpen, setEditOpen] = useState(false);
  const [editName, setEditName] = useState("");
  const [editStory, setEditStory] = useState("");

  const [charModal, setCharModal] = useState(false);
  const [charName, setCharName] = useState("");
  const [charType, setCharType] = useState("character");
  const [charDesc, setCharDesc] = useState("");

  const [videoModal, setVideoModal] = useState(false);
  const [videoTitle, setVideoTitle] = useState("");
  const [videoMode, setVideoMode] = useState<"manual" | "smart">("manual");

  const [err, setErr] = useState<string | null>(null);

  const project = projectQ.data as ProjectRow | undefined;
  const chars = (charsQ.data ?? []) as CharacterRow[];
  const videos = (videosQ.data ?? []) as VideoRow[];

  const patchM = useMutation({
    mutationFn: () =>
      api.patchProject(projectId, {
        name: editName.trim(),
        story: editStory.trim() || null,
      }),
    onSuccess: () => {
      setEditOpen(false);
      setErr(null);
      void qc.invalidateQueries({ queryKey: ["projects", projectId] });
      void qc.invalidateQueries({ queryKey: ["projects"] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const createCharM = useMutation({
    mutationFn: () =>
      api.createProjectCharacter(projectId, {
        name: charName.trim(),
        entity_type: charType,
        description: charDesc.trim() || null,
      }),
    onSuccess: () => {
      setCharModal(false);
      setCharName("");
      setCharType("character");
      setCharDesc("");
      setErr(null);
      void qc.invalidateQueries({ queryKey: ["project-characters", projectId] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const unlinkCharM = useMutation({
    mutationFn: (charId: string) =>
      api.unlinkProjectCharacter(projectId, charId, true),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["project-characters", projectId] });
    },
    onError: (e: Error) => setErr(e.message),
  });

  const createVideoM = useMutation({
    mutationFn: () =>
      api.createVideo({
        project_id: projectId,
        title: videoTitle.trim() || "Video mới",
        status: "DRAFT",
      }),
    onSuccess: (newVideo: VideoRow) => {
      const mode = videoMode;
      setVideoModal(false);
      setVideoTitle("");
      setErr(null);
      void qc.invalidateQueries({ queryKey: ["videos", projectId] });
      if (mode === "smart") {
        onOpenSmartPipeline(projectId);
      } else {
        onOpenPipeline(projectId, { videoId: str(newVideo.id) });
      }
    },
    onError: (e: Error) => setErr(e.message),
  });

  if (projectQ.isLoading) {
    return (
      <div style={{ display: "flex", justifyContent: "center", paddingTop: 80 }}>
        <Spin size="large" />
      </div>
    );
  }

  if (!project) {
    return (
      <div className="layout">
        <Alert type="error" message="Không tìm thấy project" />
      </div>
    );
  }

  const sortedVideos = videos
    .slice()
    .sort(
      (a, b) =>
        Number((a as VideoRow).display_order ?? 0) -
        Number((b as VideoRow).display_order ?? 0)
    );

  return (
    <div className="layout layout-wide">
      {err && (
        <Alert
          type="error"
          message={err}
          showIcon
          closable
          onClose={() => setErr(null)}
          style={{ marginBottom: 16 }}
        />
      )}

      {/* Header */}
      <Space direction="vertical" size={4} style={{ width: "100%", marginBottom: 24 }}>
        <Button
          type="text"
          icon={<ArrowLeftOutlined />}
          onClick={() => navigate("/projects")}
          size="small"
          style={{ paddingLeft: 0 }}
        >
          Projects
        </Button>

        <Space align="center" style={{ width: "100%", justifyContent: "space-between" }}>
          <Space align="center" size={10} wrap>
            <Typography.Title level={3} style={{ margin: 0 }}>
              {str(project.name)}
            </Typography.Title>
            {str(project.material) && <Tag color="blue">{str(project.material)}</Tag>}
            {str(project.language) && (
              <Tag>{str(project.language).toUpperCase()}</Tag>
            )}
            {str(project.orientation) && (
              <Tag color="purple">{str(project.orientation)}</Tag>
            )}
          </Space>
          <Button
            icon={<EditOutlined />}
            onClick={() => {
              setEditName(str(project.name));
              setEditStory(str(project.story ?? ""));
              setEditOpen(true);
            }}
          >
            Sửa
          </Button>
        </Space>

        {str(project.story) && (
          <Typography.Text
            type="secondary"
            style={{ maxWidth: 700, display: "block", marginTop: 4 }}
          >
            {str(project.story)}
          </Typography.Text>
        )}
      </Space>

      {/* Tabs */}
      <Tabs
        defaultActiveKey="videos"
        items={[
          {
            key: "videos",
            label: `Videos (${videos.length})`,
            icon: <VideoCameraOutlined />,
            children: (
              <Space direction="vertical" size={12} style={{ width: "100%" }}>
                <div style={{ display: "flex", justifyContent: "flex-end" }}>
                  <Button
                    type="primary"
                    icon={<PlusOutlined />}
                    onClick={() => setVideoModal(true)}
                  >
                    New Video
                  </Button>
                </div>

                {videosQ.isLoading ? (
                  <Spin />
                ) : sortedVideos.length === 0 ? (
                  <Card>
                    <div
                      style={{
                        textAlign: "center",
                        padding: "40px 0",
                        color: "var(--muted)",
                      }}
                    >
                      Chưa có video. Nhấn <strong>New Video</strong> để bắt đầu.
                    </div>
                  </Card>
                ) : (
                  <Row gutter={[12, 12]}>
                    {sortedVideos.map((v) => {
                      const vid = v as VideoRow;
                      const statusKey = str(vid.status || "DRAFT");
                      const statusInfo = VIDEO_STATUS[statusKey] ?? {
                        color: "default" as const,
                        label: statusKey,
                      };
                      return (
                        <Col key={str(vid.id)} xs={24} sm={12} lg={8}>
                          <Card
                            size="small"
                            title={
                              <Space>
                                <VideoCameraOutlined />
                                <span>{str(vid.title) || "Video"}</span>
                              </Space>
                            }
                            extra={
                              <Badge status={statusInfo.color} text={statusInfo.label} />
                            }
                            actions={[
                              <Button
                                key="open"
                                type="link"
                                onClick={() =>
                                  onOpenPipeline(projectId, { videoId: str(vid.id) })
                                }
                              >
                                Mở Studio
                              </Button>,
                            ]}
                          >
                            <Typography.Text
                              type="secondary"
                              style={{ fontSize: 12 }}
                            >
                              {str(vid.id).slice(0, 8)}…
                            </Typography.Text>
                          </Card>
                        </Col>
                      );
                    })}
                  </Row>
                )}
              </Space>
            ),
          },
          {
            key: "characters",
            label: `Characters (${chars.length})`,
            icon: <UserOutlined />,
            children: (
              <Space direction="vertical" size={12} style={{ width: "100%" }}>
                <div style={{ display: "flex", justifyContent: "flex-end" }}>
                  <Button
                    icon={<PlusOutlined />}
                    onClick={() => setCharModal(true)}
                  >
                    Thêm nhân vật
                  </Button>
                </div>

                {charsQ.isLoading ? (
                  <Spin />
                ) : chars.length === 0 ? (
                  <Card>
                    <div
                      style={{
                        textAlign: "center",
                        padding: "40px 0",
                        color: "var(--muted)",
                      }}
                    >
                      Chưa có nhân vật nào trong project này.
                    </div>
                  </Card>
                ) : (
                  <Row gutter={[12, 12]}>
                    {chars.map((c) => {
                      const char = c as CharacterRow;
                      const imgUrl = str(char.reference_image_url);
                      const typeLabel =
                        ENTITY_TYPE_LABELS[str(char.entity_type)] ??
                        str(char.entity_type);
                      return (
                        <Col key={str(char.id)} xs={24} sm={12} lg={8}>
                          <Card
                            size="small"
                            title={
                              <Space>
                                {imgUrl ? (
                                  <Avatar src={imgUrl} size={28} />
                                ) : (
                                  <Avatar icon={<UserOutlined />} size={28} />
                                )}
                                <span>{str(char.name)}</span>
                              </Space>
                            }
                            extra={<Tag>{typeLabel}</Tag>}
                            actions={[
                              <Popconfirm
                                key="unlink"
                                title={`Xoá "${str(char.name)}" khỏi project?`}
                                onConfirm={() => unlinkCharM.mutate(str(char.id))}
                              >
                                <Button type="link" danger size="small">
                                  Xoá
                                </Button>
                              </Popconfirm>,
                            ]}
                          >
                            {str(char.description) && (
                              <Typography.Text
                                type="secondary"
                                style={{ fontSize: 12 }}
                              >
                                {str(char.description).slice(0, 100)}
                                {str(char.description).length > 100 ? "…" : ""}
                              </Typography.Text>
                            )}
                          </Card>
                        </Col>
                      );
                    })}
                  </Row>
                )}
              </Space>
            ),
          },
        ]}
      />

      {/* Edit Project Modal */}
      <Modal
        open={editOpen}
        title="Sửa project"
        onCancel={() => setEditOpen(false)}
        onOk={() => patchM.mutate()}
        confirmLoading={patchM.isPending}
        okButtonProps={{ disabled: !editName.trim() }}
      >
        <Form layout="vertical" style={{ marginTop: 16 }}>
          <Form.Item label="Tên project">
            <Input
              value={editName}
              onChange={(e) => setEditName(e.target.value)}
            />
          </Form.Item>
          <Form.Item label="Story">
            <Input.TextArea
              rows={4}
              value={editStory}
              onChange={(e) => setEditStory(e.target.value)}
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* Add Character Modal */}
      <Modal
        open={charModal}
        title="Thêm nhân vật"
        onCancel={() => {
          setCharModal(false);
          setCharName("");
          setCharType("character");
          setCharDesc("");
        }}
        onOk={() => createCharM.mutate()}
        confirmLoading={createCharM.isPending}
        okButtonProps={{ disabled: !charName.trim() }}
      >
        <Form layout="vertical" style={{ marginTop: 16 }}>
          <Form.Item label="Tên" required>
            <Input
              value={charName}
              onChange={(e) => setCharName(e.target.value)}
              placeholder="Tên nhân vật..."
            />
          </Form.Item>
          <Form.Item label="Loại">
            <Select
              value={charType}
              onChange={setCharType}
              options={[
                { value: "character", label: "Nhân vật" },
                { value: "location", label: "Địa điểm" },
                { value: "creature", label: "Sinh vật" },
                { value: "visual_asset", label: "Tài sản hình ảnh" },
                { value: "generic_troop", label: "Quân đội" },
                { value: "faction", label: "Phe" },
              ]}
            />
          </Form.Item>
          <Form.Item label="Mô tả">
            <Input.TextArea
              rows={3}
              value={charDesc}
              onChange={(e) => setCharDesc(e.target.value)}
            />
          </Form.Item>
        </Form>
      </Modal>

      {/* New Video Modal */}
      <Modal
        open={videoModal}
        title="Tạo video mới"
        onCancel={() => {
          setVideoModal(false);
          setVideoTitle("");
          setVideoMode("manual");
        }}
        onOk={() => createVideoM.mutate()}
        confirmLoading={createVideoM.isPending}
        okText={videoMode === "smart" ? "Tạo & Mở Smart Pipeline" : "Tạo & Mở Studio"}
      >
        <Form layout="vertical" style={{ marginTop: 16 }}>
          <Form.Item label="Tên video">
            <Input
              value={videoTitle}
              onChange={(e) => setVideoTitle(e.target.value)}
              placeholder="Video 1"
            />
          </Form.Item>
          <Form.Item label="Chọn mode làm việc">
            <Radio.Group
              value={videoMode}
              onChange={(e) => setVideoMode(e.target.value as "manual" | "smart")}
              style={{ width: "100%" }}
            >
              <Space direction="vertical" size={8} style={{ width: "100%" }}>
                <Radio value="manual">
                  <Space>
                    <NodeIndexOutlined />
                    <div>
                      <div style={{ fontWeight: 500 }}>Manual Studio</div>
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                        Tạo và chỉnh từng scene thủ công, trigger generation theo ý muốn
                      </Typography.Text>
                    </div>
                  </Space>
                </Radio>
                <Radio value="smart">
                  <Space>
                    <BranchesOutlined />
                    <div>
                      <div style={{ fontWeight: 500 }}>Smart Pipeline</div>
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                        AI phân tích script qua agent script_parser → tự động tạo scenes, characters, images, video
                      </Typography.Text>
                    </div>
                  </Space>
                </Radio>
              </Space>
            </Radio.Group>
          </Form.Item>
        </Form>
      </Modal>
    </div>
  );
}
