import {
  ArrowDownOutlined,
  ArrowUpOutlined,
  DeleteOutlined,
  PlusOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Alert, Button, Card, Select, Space, Tag, Tooltip, Typography } from "antd";
import { useState } from "react";
import { api } from "@/lib/api/client";

const { Text } = Typography;

/** Agents the builder can place, with friendly labels + a hint of what they do.
 *  Must stay a subset of the backend's `known_agent_types()`. */
const AGENTS: { key: string; label: string; hint: string; group: string }[] = [
  { key: "director", label: "Director", hint: "Blueprint tự sự từ story", group: "Tiền kỳ" },
  { key: "screenwriter", label: "Screenwriter", hint: "Viết kịch bản", group: "Tiền kỳ" },
  { key: "scene_plan", label: "Scene Plan", hint: "Thiết kế bối cảnh/không gian", group: "Tiền kỳ" },
  { key: "shot_design", label: "Shot Design", hint: "Thiết kế shot/máy quay (DoP)", group: "Tiền kỳ" },
  { key: "visual_asset", label: "Visual Asset", hint: "DNA hình ảnh nhân vật", group: "Tiền kỳ" },
  { key: "script_parser", label: "Script Parser", hint: "Tách kịch bản thành cảnh", group: "Tiền kỳ" },
  { key: "scene_builder", label: "Scene Builder", hint: "Tổng hợp dữ liệu cảnh", group: "Tiền kỳ" },
  { key: "gen_ref", label: "Gen Ref", hint: "Khớp ảnh tham chiếu cho cảnh", group: "Tiền kỳ" },
  { key: "director_frame", label: "Director Frame", hint: "Cầu nối liên tục giữa cảnh", group: "Tiền kỳ" },
  { key: "character", label: "Character", hint: "Sinh ảnh tham chiếu nhân vật", group: "Nhân vật" },
  { key: "image", label: "Image (mỗi cảnh)", hint: "Sinh ảnh từng cảnh — chạy song song", group: "Sản xuất" },
  { key: "video", label: "Video (mỗi cảnh)", hint: "Sinh clip từng cảnh — chờ ảnh cùng cảnh", group: "Sản xuất" },
  { key: "audio", label: "Audio", hint: "Lồng tiếng / TTS", group: "Hậu kỳ" },
  { key: "media_download", label: "Media Download", hint: "Tải media về máy", group: "Hậu kỳ" },
  { key: "concat", label: "Concat", hint: "Ghép clip thành video cuối", group: "Hậu kỳ" },
  { key: "critic", label: "Critic", hint: "Pre-flight kiểm tra đầu vào", group: "QA" },
];
const LABEL = Object.fromEntries(AGENTS.map((a) => [a.key, a.label]));

/** The standard end-to-end pipeline, as a starting template. */
const PRESET: string[][] = [
  ["director"],
  ["screenwriter"],
  ["scene_plan", "visual_asset"],
  ["shot_design"],
  ["script_parser"],
  ["gen_ref"],
  ["director_frame"],
  ["character"],
  ["critic"],
  ["image"],
  ["video"],
  ["audio"],
  ["media_download"],
  ["concat"],
];

export function CustomWorkflowBuilder({
  projectId,
  orientation,
}: {
  projectId: string;
  orientation: "VERTICAL" | "HORIZONTAL";
}) {
  const qc = useQueryClient();
  const [stages, setStages] = useState<string[][]>([["director"], ["screenwriter"]]);
  const [msg, setMsg] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const addStage = () => setStages((s) => [...s, []]);
  const removeStage = (i: number) => setStages((s) => s.filter((_, idx) => idx !== i));
  const moveStage = (i: number, dir: -1 | 1) =>
    setStages((s) => {
      const j = i + dir;
      if (j < 0 || j >= s.length) return s;
      const next = [...s];
      [next[i], next[j]] = [next[j], next[i]];
      return next;
    });
  const addAgent = (i: number, key: string) =>
    setStages((s) => s.map((st, idx) => (idx === i && !st.includes(key) ? [...st, key] : st)));
  const removeAgent = (i: number, key: string) =>
    setStages((s) => s.map((st, idx) => (idx === i ? st.filter((a) => a !== key) : st)));

  const runM = useMutation({
    mutationFn: () => {
      const clean = stages.map((s) => s.filter(Boolean)).filter((s) => s.length > 0);
      if (!clean.length) throw new Error("Workflow rỗng — thêm ít nhất một agent.");
      return api.startCustomWorkflow({ project_id: projectId, orientation, stages: clean });
    },
    onSuccess: () => {
      setErr(null);
      setMsg("Đã chạy workflow tùy chỉnh. Xem tiến độ ở panel bên dưới.");
      // Let the run panel pick up the new run.
      void qc.invalidateQueries({ queryKey: ["workflow-project-run", projectId] });
    },
    onError: (e: Error) => {
      setMsg(null);
      setErr(e.message);
    },
  });

  const agentOptions = AGENTS.map((a) => ({
    value: a.key,
    label: `${a.label} — ${a.hint}`,
  }));

  return (
    <Card
      size="small"
      style={{ marginBottom: 16 }}
      title={
        <Space>
          <ThunderboltOutlined />
          <span>Tự dựng workflow</span>
          <Text type="secondary" style={{ fontSize: 12, fontWeight: 400 }}>
            agent trong một stage chạy song song · các stage chạy tuần tự
          </Text>
        </Space>
      }
      extra={
        <Space>
          <Button size="small" onClick={() => setStages(PRESET.map((s) => [...s]))}>
            Nạp mẫu đầy đủ
          </Button>
          <Button size="small" onClick={() => setStages([[]])}>
            Xóa hết
          </Button>
        </Space>
      }
    >
      {err && <Alert type="error" message={err} showIcon closable style={{ marginBottom: 12 }} />}
      {msg && <Alert type="success" message={msg} showIcon closable style={{ marginBottom: 12 }} />}

      <Space direction="vertical" size={10} style={{ width: "100%" }}>
        {stages.map((stage, i) => (
          <Card
            key={i}
            size="small"
            styles={{ body: { padding: "10px 12px" } }}
            style={{ background: "var(--surface-raised, rgba(0,0,0,0.02))" }}
          >
            <Space direction="vertical" size={8} style={{ width: "100%" }}>
              <Space style={{ width: "100%", justifyContent: "space-between" }}>
                <Text strong style={{ fontSize: 12 }}>
                  Stage {i + 1}
                  {stage.length > 1 && (
                    <Text type="secondary" style={{ fontWeight: 400 }}>
                      {"  "}({stage.length} agent song song)
                    </Text>
                  )}
                </Text>
                <Space size={2}>
                  <Tooltip title="Lên">
                    <Button size="small" type="text" icon={<ArrowUpOutlined />} disabled={i === 0} onClick={() => moveStage(i, -1)} />
                  </Tooltip>
                  <Tooltip title="Xuống">
                    <Button size="small" type="text" icon={<ArrowDownOutlined />} disabled={i === stages.length - 1} onClick={() => moveStage(i, 1)} />
                  </Tooltip>
                  <Tooltip title="Xóa stage">
                    <Button size="small" type="text" danger icon={<DeleteOutlined />} onClick={() => removeStage(i)} />
                  </Tooltip>
                </Space>
              </Space>

              <Space wrap size={6}>
                {stage.map((key) => (
                  <Tag key={key} closable onClose={() => removeAgent(i, key)} color="blue" style={{ marginInlineEnd: 0 }}>
                    {LABEL[key] ?? key}
                  </Tag>
                ))}
                {!stage.length && (
                  <Text type="secondary" style={{ fontSize: 12 }}>
                    (trống)
                  </Text>
                )}
              </Space>

              <Select
                size="small"
                style={{ width: 280 }}
                placeholder="+ thêm agent vào stage này"
                value={null}
                onChange={(v) => v && addAgent(i, v)}
                options={agentOptions.filter((o) => !stage.includes(o.value))}
                showSearch
                filterOption={(input, opt) => String(opt?.label ?? "").toLowerCase().includes(input.toLowerCase())}
              />
            </Space>
          </Card>
        ))}

        <Button icon={<PlusOutlined />} onClick={addStage} block>
          Thêm stage (tuần tự)
        </Button>

        <Button type="primary" icon={<ThunderboltOutlined />} loading={runM.isPending} onClick={() => runM.mutate()} block>
          Chạy workflow tùy chỉnh
        </Button>
      </Space>
    </Card>
  );
}
