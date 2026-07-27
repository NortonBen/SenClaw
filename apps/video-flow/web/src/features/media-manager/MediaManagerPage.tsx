import {
  AudioOutlined,
  CopyOutlined,
  DeleteOutlined,
  FileOutlined,
  PictureOutlined,
  UploadOutlined,
  VideoCameraOutlined,
} from "@ant-design/icons";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Button,
  Card,
  Checkbox,
  Col,
  Empty,
  Input,
  message,
  Modal,
  Row,
  Segmented,
  Select,
  Space,
  Spin,
  Tag,
  Tooltip,
  Typography,
  Upload,
} from "antd";
import type { UploadFile } from "antd";
import { useEffect, useState } from "react";
import { api, type MediaRow } from "@/lib/api/client";

const { Text, Title } = Typography;

type MediaFilter = "all" | "image" | "audio" | "video" | "other";

/** Lọc theo chiều khung hình (ảnh/video có width_px/height_px) */
type OrientationFilter = "all" | "portrait" | "landscape" | "square" | "unknown";

const FILTER_OPTIONS = [
  { label: "Tất cả", value: "all" },
  { label: "Hình ảnh", value: "image" },
  { label: "Âm thanh", value: "audio" },
  { label: "Video", value: "video" },
  { label: "Khác", value: "other" },
] as const;

const ORIENTATION_OPTIONS: { label: string; value: OrientationFilter }[] = [
  { label: "Mọi chiều", value: "all" },
  { label: "Dọc", value: "portrait" },
  { label: "Ngang", value: "landscape" },
  { label: "Vuông", value: "square" },
  { label: "Chưa đo", value: "unknown" },
];

function orientationLabel(item: MediaRow): string {
  const w = item.width_px ?? 0;
  const h = item.height_px ?? 0;
  if (w <= 0 || h <= 0) return "—";
  if (h > w) return "Dọc";
  if (w > h) return "Ngang";
  return "Vuông";
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function MediaIcon({ type }: { type: MediaRow["media_type"] }) {
  switch (type) {
    case "image":
      return <PictureOutlined style={{ fontSize: 28, color: "var(--accent)" }} />;
    case "audio":
      return <AudioOutlined style={{ fontSize: 28, color: "#52c41a" }} />;
    case "video":
      return <VideoCameraOutlined style={{ fontSize: 28, color: "#fa8c16" }} />;
    default:
      return <FileOutlined style={{ fontSize: 28, color: "var(--muted)" }} />;
  }
}

function MediaPreview({ item }: { item: MediaRow }) {
  const src = api.mediaFileUrl(item.id);
  if (item.media_type === "image") {
    return (
      <img
        src={src}
        alt={item.file_name}
        style={{
          width: "100%",
          height: 140,
          objectFit: "cover",
          borderRadius: 6,
          display: "block",
        }}
      />
    );
  }
  if (item.media_type === "audio") {
    return (
      <div
        style={{
          height: 140,
          display: "flex",
          flexDirection: "column",
          alignItems: "center",
          justifyContent: "center",
          gap: 12,
          background: "var(--bg-alt, #f5f5f5)",
          borderRadius: 6,
        }}
      >
        <MediaIcon type="audio" />
        {/* eslint-disable-next-line jsx-a11y/media-has-caption */}
        <audio controls src={src} style={{ width: "90%" }} />
      </div>
    );
  }
  if (item.media_type === "video") {
    return (
      // eslint-disable-next-line jsx-a11y/media-has-caption
      <video
        src={src}
        controls
        style={{
          width: "100%",
          height: 140,
          borderRadius: 6,
          display: "block",
          background: "#000",
        }}
      />
    );
  }
  return (
    <div
      style={{
        height: 140,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: "var(--bg-alt, #f5f5f5)",
        borderRadius: 6,
      }}
    >
      <MediaIcon type={item.media_type} />
    </div>
  );
}

function typeColor(type: MediaRow["media_type"]) {
  switch (type) {
    case "image":
      return "blue";
    case "audio":
      return "green";
    case "video":
      return "orange";
    default:
      return "default";
  }
}

export function MediaManagerPage() {
  const qc = useQueryClient();
  const [filter, setFilter] = useState<MediaFilter>("all");
  const [orientation, setOrientation] = useState<OrientationFilter>("all");
  const [search, setSearch] = useState("");
  const [projectId, setProjectId] = useState<string | undefined>(undefined);
  const [uploading, setUploading] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set());

  const { data: mediaList, isLoading } = useQuery({
    queryKey: ["media", filter, search, projectId, orientation],
    queryFn: () =>
      api.listMedia({
        type: filter === "all" ? undefined : filter,
        search: search.trim() || undefined,
        projectId,
        orientation: orientation === "all" ? undefined : orientation,
      }),
  });

  useEffect(() => {
    setSelectedIds(new Set());
  }, [filter, search, projectId, orientation]);
  const { data: projects = [] } = useQuery({
    queryKey: ["projects"],
    queryFn: api.listProjects,
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => api.deleteMedia(id),
    onSuccess: (_, id) => {
      setSelectedIds((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
      void qc.invalidateQueries({ queryKey: ["media"] });
      void message.success("Đã xóa media");
    },
    onError: (e: Error) => void message.error(e.message),
  });

  const bulkDeleteMut = useMutation({
    mutationFn: (ids: string[]) => api.deleteMediaBatch(ids),
    onSuccess: (res) => {
      setSelectedIds(new Set());
      void qc.invalidateQueries({ queryKey: ["media"] });
      const miss = res.missing_ids?.length ?? 0;
      void message.success(
        miss > 0
          ? `Đã xóa ${res.deleted} mục (${miss} không còn trong DB)`
          : `Đã xóa ${res.deleted} mục`,
      );
    },
    onError: (e: Error) => void message.error(e.message),
  });

  const toggleSelect = (id: string, checked: boolean) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (checked) next.add(id);
      else next.delete(id);
      return next;
    });
  };

  const handleBulkDelete = () => {
    const ids = [...selectedIds];
    if (ids.length === 0) return;
    Modal.confirm({
      title: `Xóa ${ids.length} file đã chọn?`,
      okText: "Xóa",
      okType: "danger",
      cancelText: "Hủy",
      onOk: () => bulkDeleteMut.mutateAsync(ids),
    });
  };

  const handleUpload = async (file: UploadFile) => {
    if (!(file instanceof File) && file.originFileObj) {
      const f = file.originFileObj;
      setUploading(true);
      try {
        await api.uploadMedia(f);
        void qc.invalidateQueries({ queryKey: ["media"] });
        void message.success(`Đã upload: ${f.name}`);
      } catch (e) {
        void message.error((e as Error).message);
      } finally {
        setUploading(false);
      }
    }
    return false;
  };

  const copyId = (id: string) => {
    void navigator.clipboard.writeText(id).then(() => {
      void message.success("Đã copy media_id");
    });
  };

  const items = mediaList ?? [];

  const selectAllOnPage = () => {
    setSelectedIds(new Set(items.map((i) => i.id)));
  };

  return (
    <div style={{ padding: "16px 0" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          marginBottom: 16,
        }}
      >
        <Title level={4} style={{ margin: 0 }}>
          Media Manager
        </Title>

        <Upload
          multiple
          showUploadList={false}
          accept="image/*,audio/*,video/*"
          beforeUpload={(file) => {
            void handleUpload(file as unknown as UploadFile);
            return false;
          }}
        >
          <Button icon={<UploadOutlined />} type="primary" loading={uploading}>
            Upload Media
          </Button>
        </Upload>
      </div>

      <Segmented
        options={FILTER_OPTIONS.map((o) => ({ label: o.label, value: o.value }))}
        value={filter}
        onChange={(v) => setFilter(v as MediaFilter)}
        style={{ marginBottom: 12 }}
      />
      <div style={{ marginBottom: 16 }}>
        <Text type="secondary" style={{ marginRight: 8 }}>
          Chiều (ảnh/video):
        </Text>
        <Segmented
          options={ORIENTATION_OPTIONS.map((o) => ({ label: o.label, value: o.value }))}
          value={orientation}
          onChange={(v) => setOrientation(v as OrientationFilter)}
        />
      </div>
      <Space wrap size={12} style={{ marginBottom: 16 }}>
        <Input.Search
          allowClear
          placeholder="Tìm theo tên file..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          style={{ width: 280 }}
        />
        <Select
          allowClear
          placeholder="Lọc theo project"
          value={projectId}
          onChange={(v) => setProjectId(v)}
          style={{ width: 280 }}
          options={projects
            .map((p) => ({
              value: String((p as { id?: unknown }).id ?? ""),
              label: String((p as { name?: unknown }).name ?? ""),
            }))
            .filter((p) => p.value && p.label)}
        />
      </Space>

      {!isLoading && items.length > 0 && (
        <Space wrap style={{ marginBottom: 16 }}>
          <Button size="small" onClick={selectAllOnPage}>
            Chọn tất cả
          </Button>
          {selectedIds.size > 0 && (
            <>
              <Text type="secondary">Đã chọn {selectedIds.size}</Text>
              <Button
                size="small"
                danger
                icon={<DeleteOutlined />}
                loading={bulkDeleteMut.isPending}
                onClick={handleBulkDelete}
              >
                Xóa đã chọn
              </Button>
              <Button size="small" type="link" onClick={() => setSelectedIds(new Set())}>
                Bỏ chọn
              </Button>
            </>
          )}
        </Space>
      )}

      {isLoading ? (
        <div style={{ textAlign: "center", padding: 48 }}>
          <Spin size="large" />
        </div>
      ) : items.length === 0 ? (
        <Empty description="Chưa có media nào" />
      ) : (
        <Row gutter={[16, 16]}>
          {items.map((item) => (
            <Col key={item.id} xs={24} sm={12} md={8} lg={6} xl={4}>
              <Card
                size="small"
                styles={{ body: { padding: 8 } }}
                style={{ overflow: "hidden" }}
              >
                <div style={{ position: "relative" }}>
                  <Checkbox
                    checked={selectedIds.has(item.id)}
                    onChange={(e) => toggleSelect(item.id, e.target.checked)}
                    style={{
                      position: "absolute",
                      top: 6,
                      left: 6,
                      zIndex: 2,
                    }}
                  />
                  <div style={{ borderRadius: 6, overflow: "hidden" }}>
                    <MediaPreview item={item} />
                  </div>
                </div>

                <div style={{ marginTop: 8 }}>
                  <div
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      gap: 4,
                    }}
                  >
                    <Tag color={typeColor(item.media_type)} style={{ margin: 0, fontSize: 11 }}>
                      {item.media_type}
                    </Tag>
                    <Text style={{ fontSize: 11, color: "var(--muted)" }}>
                      {formatBytes(item.size_bytes)}
                    </Text>
                  </div>

                  {(item.media_type === "image" || item.media_type === "video") && (
                    <Text
                      type="secondary"
                      style={{ fontSize: 11, display: "block", marginTop: 4 }}
                    >
                      {(item.width_px ?? 0) > 0 && (item.height_px ?? 0) > 0
                        ? `${item.width_px}×${item.height_px} · ${orientationLabel(item)}`
                        : "Chưa đo kích thước"}
                    </Text>
                  )}

                  <Tooltip title={item.file_name}>
                    <Text
                      ellipsis
                      style={{ fontSize: 12, display: "block", marginTop: 4, maxWidth: "100%" }}
                    >
                      {item.file_name}
                    </Text>
                  </Tooltip>

                  <Space size={4} style={{ marginTop: 6, width: "100%", justifyContent: "flex-end" }}>
                    <Tooltip title="Copy media_id">
                      <Button
                        size="small"
                        icon={<CopyOutlined />}
                        onClick={() => copyId(item.id)}
                      />
                    </Tooltip>
                    <Tooltip title="Tải về">
                      <Button
                        size="small"
                        icon={<UploadOutlined style={{ transform: "rotate(180deg)" }} />}
                        href={api.mediaFileUrl(item.id)}
                        download={item.file_name}
                      />
                    </Tooltip>
                    <Tooltip title="Xóa">
                      <Button
                        size="small"
                        danger
                        icon={<DeleteOutlined />}
                        loading={deleteMut.isPending && deleteMut.variables === item.id}
                        onClick={() => deleteMut.mutate(item.id)}
                      />
                    </Tooltip>
                  </Space>
                </div>
              </Card>
            </Col>
          ))}
        </Row>
      )}
    </div>
  );
}
