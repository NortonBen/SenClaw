import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Alert,
  Badge,
  Button,
  Card,
  Collapse,
  Empty,
  Input,
  Modal,
  Space,
  Table,
  Tabs,
  Tag,
  Typography,
  message,
} from "antd";
import { Link } from "react-router-dom";
import {
  AppstoreOutlined,
  BranchesOutlined,
  CloudOutlined,
  CodeOutlined,
  DeleteOutlined,
  DragOutlined,
  EditOutlined,
  HistoryOutlined,
  NodeIndexOutlined,
  SaveOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import { api } from "../api";
import type { FlowAction, FlowAtomic, SavedFlowAction } from "../types/api";
import { AtomicChainEditor } from "./flows/AtomicChainEditor";
import { StepParamsEditor } from "./flows/StepParamsEditor";
import { ATOMIC_PALETTE } from "./flows/atomicPalette";
import { ACTIONS, ENGINE_ACTIONS, PLAYWRIGHT_ATOMICS_PALETTE_ACTION } from "./flows/constants";

const { Title, Paragraph, Text } = Typography;

const FLOW_ACTION_NOTES: Partial<Record<string, string>> = {
  check_login: "Kiểm tra đã đăng nhập; nhánh err thường nối tới bước login.",
  if_condition:
    "Nhánh theo điều kiện (assert): đúng → ok, sai → err. Tham số giống atomic assert (expect, selector, value, timeout_ms…).",
  login: "Đăng nhập TikTok web (profile persist).",
  open_home: "Mở trang chủ / FYP.",
  open_url: "Mở URL tùy chỉnh (http/https). Config: url, wait_until, timeout_ms.",
  wait_page_ready:
    "Chờ trang hiện tại đạt load state (Playwright WaitForLoadState). Config: state (load | domcontentloaded | networkidle), timeout_ms. Đặt sau open_home/open_url khi cần chắc trang đã tải xong.",
  search: "Tìm kiếm từ khóa.",
  watch_video: "Xem video (chờ).",
  like_video:
    "Legacy engine (flow cũ). Flow mới: preset palette «Like video (atomic)» — cùng selector như handler Go.",
  comment_video: "Bình luận (config text).",
  share_video: "Chia sẻ (copy link / repost / messages).",
  reply_comment: "Trả lời comment theo index / filter.",
  get_info_post: "AI / extract thông tin bài đăng.",
  get_comments_in_page: "Cuộn và lấy danh sách comment.",
  reply_comment_ai: "Trả lời comment bằng AI.",
  ai_gent_comment:
    "Sinh comment cho bài post. mode=ai: phân tích HTML + LLM; mode=select_comment: random từ comments_list (không dùng AI). Kết quả set vào run param (output_param_key).",
  ai_playwright_agent:
    "Agent LLM gọi tool đầy đủ: Page (reload/back/screenshot/evaluate), Locator (dblclick/hover/drag/select/aria_snapshot/…), Mouse/Keyboard, iframe frame_locator_*, wait_for_locator. Mục tiêu = instruction/goal; LLM từ Settings.",
  follow_user: "Follow user trên trang hiện tại.",
  random_delay: "Chờ ngẫu nhiên min–max ms.",
  random_yes_no:
    "Nhánh ngẫu nhiên theo %: yes → _next_on_success, no → _next_on_error. Config yes_percent (0–100), alias probability / percent / p.",
  next_video_post:
    "Legacy engine (wheel / PageDown / ArrowDown). Flow mới: preset «Next video — … (atomic)» trên palette.",
  loop_repeat: "Vòng lặp nhánh (loop / done).",
  loop_if:
    "Loop theo điều kiện run param: điều kiện đúng -> thoát nhánh done; sai -> tiếp tục nhánh loop. Config: param_key, operator, value, max_loops.",
  check_scroll_end:
    "Kiểm tra element đã scroll cuối chưa rồi set vào run params (output_param_key). Dùng kèm loop_if để dừng vòng lặp khi chạm cuối.",
  run_next_flow:
    "Chạy nối một flow khác trong cùng phiên browser (cùng log). Config: next_flow_id hoặc flow_id. Nhánh ok/err theo kết quả flow con.",
  set_params:
    "Cập nhật run params khi chạy (config key/value hoặc updates nhiều dòng key=value). Dùng trong action khác qua template {{param.KEY}}.",
  record_post_interaction:
    "Ghi nhận account đã tương tác với một post (post_key / video_id, interaction, post_url, author…). Lưu store khi chạy qua server.",
  record_friend_event:
    "Ghi nhận follow / unfollow hoặc thêm-xóa bạn bè (event, target_username, notes). Dùng để thống kê; lưu store khi chạy qua server.",
  account_meta:
    "Metadata key-value theo account đang chạy: operation upsert|delete, meta_key, meta_value (hỗ trợ template).",
  log: "Ghi log run.",
  notification: "Gửi notification hệ thống.",
  playwright_atomics:
    "Một step duy nhất trên flow: hành vi = chuỗi atomic (click, fill, goto, scroll, assert…). Đổi logic bằng cách sửa atomics — không cần sửa handler Go.",
};

/** Nhóm chỉ cho actions engine (đã loại playwright_atomics). */
const ENGINE_FLOW_ACTION_GROUPS: { key: string; title: string; hint: string; types: readonly string[] }[] = [
  {
    key: "session",
    title: "Phiên & điều hướng",
    hint: "Đăng nhập, mở trang, tìm kiếm",
    types: ["check_login", "if_condition", "login", "open_home", "open_url", "wait_page_ready", "search"],
  },
  {
    key: "engage",
    title: "Tương tác nội dung",
    hint: "Video, comment, follow — like & next video dùng preset atomic trên palette",
    types: ["watch_video", "comment_video", "share_video", "follow_user"],
  },
  {
    key: "comments",
    title: "Comment & trích xuất",
    hint: "Reply, load comment, AI",
    types: ["reply_comment", "get_info_post", "get_comments_in_page", "reply_comment_ai", "ai_gent_comment"],
  },
  {
    key: "ai_agent",
    title: "AI agent (LLM + Playwright)",
    hint: "Tool-calling qua langchaingo; cấu hình LLM trong Settings",
    types: ["ai_playwright_agent"],
  },
  {
    key: "control",
    title: "Điều khiển flow",
    hint: "Delay, vòng lặp, log, thông báo",
    types: ["random_delay", "random_yes_no", "loop_repeat", "loop_if", "check_scroll_end", "run_next_flow", "set_params", "log", "notification"],
  },
  {
    key: "account_store",
    title: "Lưu hoạt động account",
    hint: "Tương tác post, follow/bạn bè, metadata — chỉ khi run qua server",
    types: ["record_post_interaction", "record_friend_event", "account_meta"],
  },
];

function flowActionTagColor(type: string): string {
  if (type === "playwright_atomics") return "purple";
  if (type === "ai_playwright_agent") return "geekblue";
  if (type === "check_login" || type === "if_condition" || type === "login") return "blue";
  if (type.startsWith("get_") || type.includes("reply") || type.includes("ai")) return "cyan";
  if (
    type.includes("video") ||
    type.includes("like") ||
    type.includes("comment") ||
    type.includes("share") ||
    type === "follow_user" ||
    type === "search"
  ) {
    return "magenta";
  }
  if (
    type.includes("delay") ||
    type === "random_yes_no" ||
    type.includes("loop") ||
    type === "run_next_flow" ||
    type === "log" ||
    type === "notification"
  )
    return "gold";
  if (type === "record_post_interaction" || type === "record_friend_event" || type === "account_meta") return "volcano";
  if (type === "open_home" || type === "open_url" || type === "wait_page_ready") return "geekblue";
  return "default";
}

function atomicKindColor(kind: string): string {
  switch (kind) {
    case "click":
    case "click_button_text":
      return "blue";
    case "fill":
      return "green";
    case "goto":
      return "geekblue";
    case "press":
      return "orange";
    case "wait_ms":
    case "wait_load":
      return "gold";
    case "scroll":
      return "cyan";
    case "assert":
      return "purple";
    default:
      return "default";
  }
}

function summarizeParams(p: Record<string, string>): string {
  return Object.entries(p)
    .map(([k, v]) => `${k}=${v.length > 40 ? `${v.slice(0, 40)}…` : v}`)
    .join(" · ");
}

function cloneFlowAtomicList(list: FlowAtomic[] | undefined): FlowAtomic[] {
  return (list ?? []).map((a) => ({
    id: a.id,
    name: a.name,
    kind: a.kind,
    params: { ...(a.params ?? {}) },
  }));
}

function formatSavedAt(iso?: string): string {
  if (!iso) return "—";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString("vi-VN");
}

const ATOMICS_PALETTE_ACTIONS = ACTIONS.filter((a) => a.implementation === "atomics");

export function FlowActionsCatalogPage() {
  const [tab, setTab] = useState<string>("engine");
  const [qEngine, setQEngine] = useState("");
  const [qAtomic, setQAtomic] = useState("");
  const [savedList, setSavedList] = useState<SavedFlowAction[]>([]);
  const [savedLoading, setSavedLoading] = useState(false);
  const [editOpen, setEditOpen] = useState(false);
  const [editSaving, setEditSaving] = useState(false);
  const [editSavedId, setEditSavedId] = useState("");
  const [editName, setEditName] = useState("");
  const [editStepId, setEditStepId] = useState("");
  const [editTimeout, setEditTimeout] = useState(60);
  const [editStepParams, setEditStepParams] = useState<Record<string, string>>({});
  const [editAtomics, setEditAtomics] = useState<FlowAtomic[]>([]);

  const refreshSavedList = useCallback(async () => {
    try {
      setSavedLoading(true);
      const list = await api<SavedFlowAction[] | null>("/api/saved-flow-actions");
      setSavedList(Array.isArray(list) ? list : []);
    } catch {
      setSavedList([]);
    } finally {
      setSavedLoading(false);
    }
  }, []);

  useEffect(() => {
    void refreshSavedList();
  }, [refreshSavedList]);

  useEffect(() => {
    if (tab === "saved-atomics") void refreshSavedList();
  }, [tab, refreshSavedList]);

  const openEditSaved = (s: SavedFlowAction) => {
    setEditSavedId(s.id);
    setEditName(s.name);
    setEditStepId(s.step.id || "");
    setEditTimeout(s.step.timeoutSeconds > 0 ? s.step.timeoutSeconds : 60);
    setEditStepParams({ ...(s.step.params ?? {}) });
    setEditAtomics(cloneFlowAtomicList(s.step.atomics));
    setEditOpen(true);
  };

  const closeEditSaved = () => {
    setEditOpen(false);
    setEditSavedId("");
    setEditName("");
    setEditStepId("");
    setEditStepParams({});
    setEditAtomics([]);
  };

  const saveEditSaved = async () => {
    const name = editName.trim();
    if (!name) {
      message.error("Nhập tên action");
      return;
    }
    if (!editSavedId) return;
    const sid = editStepId.trim() || `step_saved_${editSavedId}`;
    const step: FlowAction = {
      id: sid,
      type: "playwright_atomics",
      name,
      config: {},
      timeoutSeconds: editTimeout,
      params: { ...editStepParams },
      atomics: cloneFlowAtomicList(editAtomics),
    };
    try {
      setEditSaving(true);
      await api<SavedFlowAction>("/api/saved-flow-actions", "POST", { id: editSavedId, name, step });
      message.success("Đã lưu action");
      closeEditSaved();
      await refreshSavedList();
    } catch (e) {
      message.error(String(e));
    } finally {
      setEditSaving(false);
    }
  };

  const confirmDeleteSaved = (s: SavedFlowAction) => {
    Modal.confirm({
      title: `Xóa «${s.name}»?`,
      content: "Không hoàn tác.",
      okText: "Xóa",
      okType: "danger",
      cancelText: "Hủy",
      onOk: async () => {
        try {
          await api(`/api/saved-flow-actions/${encodeURIComponent(s.id)}`, "DELETE");
          message.success("Đã xóa");
          if (editOpen && editSavedId === s.id) closeEditSaved();
          await refreshSavedList();
        } catch (e) {
          message.error(String(e));
        }
      },
    });
  };

  const filteredEngineGroups = useMemo(() => {
    const s = qEngine.trim().toLowerCase();
    return ENGINE_FLOW_ACTION_GROUPS.map((g) => {
      const items = ENGINE_ACTIONS.filter((a) => (g.types as readonly string[]).includes(a.type)).filter((a) => {
        if (!s) return true;
        const note = FLOW_ACTION_NOTES[a.type] ?? "";
        return (
          a.type.toLowerCase().includes(s) ||
          a.name.toLowerCase().includes(s) ||
          note.toLowerCase().includes(s)
        );
      });
      return { ...g, items };
    }).filter((g) => g.items.length > 0);
  }, [qEngine]);

  const filteredAtomics = useMemo(() => {
    const s = qAtomic.trim().toLowerCase();
    if (!s) return [...ATOMIC_PALETTE];
    return ATOMIC_PALETTE.filter((item) => {
      const blob = `${item.label} ${item.kind} ${JSON.stringify(item.defaultParams)}`.toLowerCase();
      return blob.includes(s);
    });
  }, [qAtomic]);

  const atomicsByKind = useMemo(() => {
    const m = new Map<string, typeof filteredAtomics>();
    for (const item of filteredAtomics) {
      const arr = m.get(item.kind) ?? [];
      arr.push(item);
      m.set(item.kind, arr);
    }
    return Array.from(m.entries()).sort(([a], [b]) => a.localeCompare(b));
  }, [filteredAtomics]);

  const collapseItems = filteredEngineGroups.map((g) => ({
    key: g.key,
    label: (
      <Space size={8} wrap align="center">
        <Text strong>{g.title}</Text>
        <Tag bordered={false} style={{ margin: 0 }}>
          {g.items.length}
        </Tag>
        <Text type="secondary" style={{ fontSize: 12 }}>
          {g.hint}
        </Text>
      </Space>
    ),
    children: (
      <div className="flow-actions-catalog-group-body">
        {g.items.map((a) => (
          <div key={a.type} className="flow-actions-catalog-row">
            <div className="flow-actions-catalog-row-tags">
              <Tag color={flowActionTagColor(a.type)}>{a.type}</Tag>
              <Tag color="processing" style={{ marginTop: 6 }}>
                engine
              </Tag>
            </div>
            <div>
              <Text strong style={{ display: "block", marginBottom: 4 }}>
                {a.name}
              </Text>
              <Text type="secondary" style={{ fontSize: 13, lineHeight: 1.5 }}>
                {FLOW_ACTION_NOTES[a.type] ?? "—"}
              </Text>
            </div>
          </div>
        ))}
      </div>
    ),
  }));

  const pa = PLAYWRIGHT_ATOMICS_PALETTE_ACTION;

  return (
    <div className="page flow-actions-catalog-page">
      <section className="flow-actions-catalog-hero">
        <div className="flow-actions-catalog-hero-text">
          <Text type="secondary" className="flow-actions-catalog-kicker">
            Automation · Flow
          </Text>
          <Title level={3} style={{ margin: "0 0 8px" }}>
            Thư viện actions
          </Title>
          <Paragraph type="secondary" style={{ marginBottom: 0, maxWidth: 560 }}>
            Hai loại: <Text strong>(1)</Text> step <Text code>engine</Text> — logic cố định trong Go;{" "}
            <Text strong>(2)</Text> step <Text code>playwright_atomics</Text> — chỉnh chuỗi atomic. Like và chuyển video (FYP)
            dùng preset atomic trên palette (thay cho <Text code>like_video</Text> / <Text code>next_video_post</Text> engine). Flow
            cũ vẫn chạy type legacy nếu còn lưu.
          </Paragraph>
        </div>
        <Space wrap className="flow-actions-catalog-hero-actions">
          <Link to="/flows">
            <Button icon={<BranchesOutlined />}>Danh sách flow</Button>
          </Link>
          <Link to="/flows/actions/build">
            <Button type="primary" icon={<DragOutlined />} size="large">
              Tạo / sửa action (atomic)
            </Button>
          </Link>
        </Space>
      </section>

      <Card className="flow-actions-catalog-tabs-card" styles={{ body: { paddingTop: 8 } }}>
        <Tabs
          activeKey={tab}
          onChange={setTab}
          items={[
            {
              key: "engine",
              label: (
                <span>
                  <CodeOutlined style={{ marginRight: 8 }} />
                  Actions code (engine)
                  <Badge count={ENGINE_ACTIONS.length} overflowCount={99} style={{ marginLeft: 10 }} />
                </span>
              ),
              children: (
                <div style={{ paddingTop: 8 }}>
                  <Alert
                    type="info"
                    showIcon
                    style={{ marginBottom: 16 }}
                    message="Handler Go cố định"
                    description={
                      <>
                        Mỗi type map tới executor trong backend. Đổi hành vi cần sửa code Go và deploy lại. Các type{" "}
                        <Text code>like_video</Text>, <Text code>next_video_post</Text> vẫn được engine hỗ trợ cho flow đã lưu;
                        flow mới nên dùng preset <Text strong>Like video (atomic)</Text> và <Text strong>Next video — … (atomic)</Text>.
                      </>
                    }
                  />
                  <div className="flow-actions-catalog-toolbar">
                    <Input
                      allowClear
                      size="large"
                      prefix={<SearchOutlined style={{ color: "var(--muted-text)" }} />}
                      placeholder="Lọc theo type, tên hoặc mô tả…"
                      value={qEngine}
                      onChange={(e) => setQEngine(e.target.value)}
                      style={{ maxWidth: 420 }}
                    />
                  </div>
                  {filteredEngineGroups.length === 0 ? (
                    <Text type="secondary">Không khớp bộ lọc.</Text>
                  ) : (
                    <Collapse
                      bordered={false}
                      className="flow-actions-catalog-collapse"
                      defaultActiveKey={filteredEngineGroups.map((g) => g.key)}
                      items={collapseItems}
                    />
                  )}
                </div>
              ),
            },
            {
              key: "legacy-atomics",
              label: (
                <span>
                  <HistoryOutlined style={{ marginRight: 8 }} />
                  Legacy atomic
                  <Badge count={ATOMICS_PALETTE_ACTIONS.length} overflowCount={999} style={{ marginLeft: 10 }} />
                </span>
              ),
              children: (
                <div style={{ paddingTop: 8 }}>
                  <Alert
                    type="success"
                    showIcon
                    style={{ marginBottom: 16 }}
                    message="Một step trên flow — hành vi = danh sách atomic"
                    description={
                      <>
                        Kéo <Text code>playwright_atomics</Text> vào canvas, rồi mở config và ghép các bước nhỏ. Cập nhật{" "}
                        <Text strong>atomic</Text> (trong flow editor hoặc trang builder) là đủ; engine chạy tuần tự theo{" "}
                        <Text code>kind</Text> đã đăng ký. <Text strong>Mẫu có sẵn</Text> nằm tab <Text strong>Mẫu atomic</Text>.{" "}
                        Action tự tạo và lưu SQLite nằm tab <Text strong>Action atomic đã tạo (lưu server)</Text>.
                      </>
                    }
                  />

                  <div
                    className="flow-actions-catalog-row flow-actions-playwright-atomics-highlight"
                    style={{ marginBottom: 20, padding: "14px 16px", borderRadius: 12 }}
                  >
                    <div className="flow-actions-catalog-row-tags">
                      <Tag color="purple">{pa.type}</Tag>
                      <Tag color="success" style={{ marginTop: 6 }}>
                        atomics
                      </Tag>
                    </div>
                    <div>
                      <Text strong style={{ display: "block", marginBottom: 4 }}>
                        {pa.name}
                      </Text>
                      <Text type="secondary" style={{ fontSize: 13, lineHeight: 1.55 }}>
                        {FLOW_ACTION_NOTES.playwright_atomics}
                      </Text>
                    </div>
                  </div>

                  <Title level={5} style={{ margin: "0 0 12px" }}>
                    <NodeIndexOutlined style={{ marginRight: 8 }} />
                    Các step <Text code>playwright_atomics</Text> trên palette (cố định trong code)
                  </Title>
                  <Paragraph type="secondary" style={{ fontSize: 13, marginBottom: 12 }}>
                    Số trên tab = {ATOMICS_PALETTE_ACTIONS.length} preset kéo-thả trong editor (Like / Next video / chuỗi trống…).
                  </Paragraph>
                  <div className="flow-actions-catalog-group-body" style={{ marginBottom: 8 }}>
                    {ATOMICS_PALETTE_ACTIONS.filter((a) => a.paletteId !== "playwright_atomics_blank").map((a) => (
                      <div key={a.paletteId} className="flow-actions-catalog-row">
                        <div className="flow-actions-catalog-row-tags">
                          <Tag color={flowActionTagColor(a.type)}>{a.type}</Tag>
                          <Tag color="success" style={{ marginTop: 6 }}>
                            atomics
                          </Tag>
                        </div>
                        <div>
                          <Text strong style={{ display: "block", marginBottom: 4 }}>
                            {a.name}
                          </Text>
                          <Text type="secondary" style={{ fontSize: 13, lineHeight: 1.5 }}>
                            {a.presetAtomics?.length
                              ? `Preset gồm ${a.presetAtomics.length} atomic sẵn — chỉnh trong flow sau khi thả.`
                              : "Chuỗi trống — tự ghép atomic trong config."}
                          </Text>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ),
            },
            {
              key: "saved-atomics",
              label: (
                <span>
                  <CloudOutlined style={{ marginRight: 8 }} />
                  Action atomic đã tạo (lưu server)
                  <Badge count={savedList.length} overflowCount={999} style={{ marginLeft: 10 }} />
                </span>
              ),
              children: (
                <div style={{ paddingTop: 8 }}>
                  <Alert
                    type="info"
                    showIcon
                    style={{ marginBottom: 16 }}
                    message="Bản ghi SQLite từ builder"
                    description="Số trên tab = tổng action đã lưu. Danh sách có thể phân trang; làm mới sau khi tạo/xóa ở builder."
                  />
                  <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, flexWrap: "wrap", marginBottom: 12 }}>
                    <Title level={5} style={{ margin: 0 }}>
                      Danh sách đã lưu
                    </Title>
                    <Space wrap>
                      <Button size="small" onClick={() => void refreshSavedList()} loading={savedLoading}>
                        Làm mới danh sách
                      </Button>
                      <Link to="/flows/actions/build">
                        <Button type="primary" size="small" icon={<DragOutlined />}>
                          Tạo mới trong builder
                        </Button>
                      </Link>
                    </Space>
                  </div>
                  <Paragraph type="secondary" style={{ fontSize: 13, marginBottom: 12 }}>
                    Các bản ghi từ trang builder (SQLite). Sửa nhanh tại đây hoặc mở builder để làm việc fullscreen.
                  </Paragraph>
                  <Table<SavedFlowAction>
                    size="small"
                    rowKey="id"
                    loading={savedLoading}
                    dataSource={savedList}
                    pagination={{ pageSize: 10, showSizeChanger: true }}
                    locale={{
                      emptyText: (
                        <Empty description="Chưa có action nào — tạo ở builder rồi lưu server">
                          <Link to="/flows/actions/build">
                            <Button type="primary" icon={<DragOutlined />}>
                              Mở builder
                            </Button>
                          </Link>
                        </Empty>
                      ),
                    }}
                    columns={[
                      { title: "Tên", dataIndex: "name", key: "name", ellipsis: true },
                      {
                        title: "ID",
                        dataIndex: "id",
                        key: "id",
                        width: 200,
                        ellipsis: true,
                        render: (id: string) => (
                          <Text code style={{ fontSize: 11 }}>
                            {id}
                          </Text>
                        ),
                      },
                      {
                        title: "Số atomic",
                        key: "n",
                        width: 96,
                        align: "center",
                        render: (_, r) => r.step.atomics?.length ?? 0,
                      },
                      {
                        title: "Cập nhật",
                        key: "u",
                        width: 168,
                        render: (_, r) => (
                          <Text type="secondary" style={{ fontSize: 12 }}>
                            {formatSavedAt(r.updatedAt)}
                          </Text>
                        ),
                      },
                      {
                        title: "",
                        key: "a",
                        width: 280,
                        render: (_, r) => (
                          <Space wrap size="small">
                            <Button size="small" type="primary" icon={<EditOutlined />} onClick={() => openEditSaved(r)}>
                              Sửa & lưu
                            </Button>
                            <Link to={`/flows/actions/build?id=${encodeURIComponent(r.id)}`}>
                              <Button size="small">Builder</Button>
                            </Link>
                            <Button size="small" danger icon={<DeleteOutlined />} onClick={() => confirmDeleteSaved(r)} aria-label="Xóa" />
                          </Space>
                        ),
                      },
                    ]}
                  />
                </div>
              ),
            },
            {
              key: "atomic-templates",
              label: (
                <span>
                  <AppstoreOutlined style={{ marginRight: 8 }} />
                  Mẫu atomic
                  <Badge count={ATOMIC_PALETTE.length} overflowCount={999} style={{ marginLeft: 10 }} />
                </span>
              ),
              children: (
                <div style={{ paddingTop: 8 }}>
                  <Alert
                    type="info"
                    showIcon
                    style={{ marginBottom: 16 }}
                    message="Tham chiếu & kéo thả trong builder"
                    description={
                      <>
                        Đây là <Text strong>mẫu cố định trong code</Text> (khác preset tab <Text strong>Legacy atomic</Text> và khác bản đã lưu tab{" "}
                        <Text strong>Action atomic đã tạo (lưu server)</Text>). Dùng để tham chiếu tham số
                        hoặc kéo vào builder / flow editor khi ghép <Text code>playwright_atomics</Text>.
                      </>
                    }
                  />
                  <Title level={5} style={{ marginBottom: 12 }}>
                    <AppstoreOutlined style={{ marginRight: 8 }} />
                    Mẫu theo kind
                  </Title>
                  <div className="flow-actions-catalog-toolbar">
                    <Input
                      allowClear
                      size="large"
                      prefix={<SearchOutlined style={{ color: "var(--muted-text)" }} />}
                      placeholder="Lọc mẫu theo nhãn, kind hoặc params…"
                      value={qAtomic}
                      onChange={(e) => setQAtomic(e.target.value)}
                      style={{ maxWidth: 420 }}
                    />
                  </div>
                  {filteredAtomics.length === 0 ? (
                    <Text type="secondary">Không khớp bộ lọc.</Text>
                  ) : (
                    <div className="flow-actions-catalog-atomic-sections">
                      {atomicsByKind.map(([kind, items]) => (
                        <div key={kind} className="flow-actions-catalog-kind-block">
                          <div className="flow-actions-catalog-kind-heading">
                            <Tag color={atomicKindColor(kind)}>{kind}</Tag>
                            <Text type="secondary" style={{ fontSize: 12 }}>
                              {items.length} mẫu
                            </Text>
                          </div>
                          <div className="flow-actions-atomic-grid">
                            {items.map((item) => (
                              <div key={item.id} className="flow-actions-atomic-card">
                                <Text strong style={{ display: "block", marginBottom: 6, lineHeight: 1.35 }}>
                                  {item.label}
                                </Text>
                                <Text type="secondary" className="flow-actions-atomic-params" title={summarizeParams(item.defaultParams)}>
                                  {summarizeParams(item.defaultParams)}
                                </Text>
                              </div>
                            ))}
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              ),
            },
          ]}
        />
      </Card>

      <Modal
        title="Sửa action atomic đã lưu"
        open={editOpen}
        onCancel={closeEditSaved}
        width={1200}
        style={{ maxWidth: "calc(100vw - 24px)" }}
        destroyOnClose
        footer={
          <Space wrap>
            <Button onClick={closeEditSaved}>Hủy</Button>
            <Button
              danger
              icon={<DeleteOutlined />}
              onClick={() => {
                const s = savedList.find((x) => x.id === editSavedId);
                if (s) confirmDeleteSaved(s);
              }}
            >
              Xóa trên server
            </Button>
            <Button type="primary" icon={<SaveOutlined />} loading={editSaving} onClick={() => void saveEditSaved()}>
              Lưu
            </Button>
          </Space>
        }
      >
        <Space direction="vertical" size="middle" style={{ width: "100%" }}>
          <div>
            <Text strong style={{ display: "block", marginBottom: 6 }}>
              Tên
            </Text>
            <Input value={editName} onChange={(e) => setEditName(e.target.value)} placeholder="Tên action" maxLength={240} />
          </div>
          <div>
            <Text strong style={{ display: "block", marginBottom: 6 }}>
              Id step (giữ nguyên nếu đã dùng trong flow / JSON)
            </Text>
            <Input value={editStepId} onChange={(e) => setEditStepId(e.target.value)} placeholder="step_saved_…" />
          </div>
          <div>
            <Text strong style={{ display: "block", marginBottom: 6 }}>
              Timeout (giây)
            </Text>
            <Input
              type="number"
              min={1}
              value={editTimeout}
              onChange={(e) => setEditTimeout(Number(e.target.value) || 60)}
              style={{ maxWidth: 140 }}
            />
          </div>
          <StepParamsEditor params={editStepParams} onChange={setEditStepParams} />
          <div>
            <Text strong style={{ display: "block", marginBottom: 8 }}>
              Chuỗi atomic
            </Text>
            <AtomicChainEditor atomics={editAtomics} onChange={setEditAtomics} />
          </div>
        </Space>
      </Modal>
    </div>
  );
}
