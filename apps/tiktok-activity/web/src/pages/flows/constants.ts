import type { ActionType, PaletteAction } from "../../types/api";
import {
  PRESET_ATOMICS_LIKE_VIDEO,
  PRESET_ATOMICS_NEXT_VIDEO_ARROWDOWN,
  PRESET_ATOMICS_NEXT_VIDEO_PAGEDOWN,
  PRESET_ATOMICS_NEXT_VIDEO_WHEEL,
} from "./actionPresets";

export const ACTIONS: readonly PaletteAction[] = [
  { paletteId: "check_login", type: "check_login", name: "Check Login", implementation: "engine" },
  { paletteId: "if_condition", type: "if_condition", name: "If (điều kiện)", implementation: "engine" },
  { paletteId: "login", type: "login", name: "Login", implementation: "engine" },
  { paletteId: "open_home", type: "open_home", name: "Open Home", implementation: "engine" },
  { paletteId: "open_url", type: "open_url", name: "Open URL", implementation: "engine" },
  {
    paletteId: "wait_page_ready",
    type: "wait_page_ready",
    name: "Chờ trang tải xong (load state)",
    implementation: "engine",
  },
  { paletteId: "search", type: "search", name: "Search Keyword", implementation: "engine" },
  { paletteId: "watch_video", type: "watch_video", name: "Watch Video", implementation: "engine" },
  {
    paletteId: "playwright_atomics__like_video",
    type: "playwright_atomics",
    name: "Like video (atomic)",
    implementation: "atomics",
    presetAtomics: PRESET_ATOMICS_LIKE_VIDEO,
  },
  { paletteId: "comment_video", type: "comment_video", name: "Comment Video", implementation: "engine" },
  { paletteId: "share_video", type: "share_video", name: "Share Video", implementation: "engine" },
  { paletteId: "reply_comment", type: "reply_comment", name: "Reply Comment", implementation: "engine" },
  { paletteId: "get_info_post", type: "get_info_post", name: "Get Info Post", implementation: "engine" },
  { paletteId: "get_comments_in_page", type: "get_comments_in_page", name: "Load & Extract Comments", implementation: "engine" },
  { paletteId: "reply_comment_ai", type: "reply_comment_ai", name: "AI Reply Comment", implementation: "engine" },
  { paletteId: "ai_gent_comment", type: "ai_gent_comment", name: "AI Gent Comment", implementation: "engine" },
  {
    paletteId: "ai_playwright_agent",
    type: "ai_playwright_agent",
    name: "AI Agent",
    implementation: "engine",
  },
  { paletteId: "follow_user", type: "follow_user", name: "Follow User", implementation: "engine" },
  { paletteId: "random_delay", type: "random_delay", name: "Random Delay", implementation: "engine" },
  { paletteId: "random_yes_no", type: "random_yes_no", name: "Random Yes / No (%)", implementation: "engine" },
  {
    paletteId: "playwright_atomics__next_wheel",
    type: "playwright_atomics",
    name: "Next video — scroll wheel (atomic)",
    implementation: "atomics",
    presetAtomics: PRESET_ATOMICS_NEXT_VIDEO_WHEEL,
  },
  {
    paletteId: "playwright_atomics__next_pagedown",
    type: "playwright_atomics",
    name: "Next video — PageDown (atomic)",
    implementation: "atomics",
    presetAtomics: PRESET_ATOMICS_NEXT_VIDEO_PAGEDOWN,
  },
  {
    paletteId: "playwright_atomics__next_arrowdown",
    type: "playwright_atomics",
    name: "Next video — ArrowDown (atomic)",
    implementation: "atomics",
    presetAtomics: PRESET_ATOMICS_NEXT_VIDEO_ARROWDOWN,
  },
  { paletteId: "loop_repeat", type: "loop_repeat", name: "Loop Repeat", implementation: "engine" },
  { paletteId: "loop_if", type: "loop_if", name: "Loop If (thoát theo param)", implementation: "engine" },
  {
    paletteId: "check_scroll_end",
    type: "check_scroll_end",
    name: "Check element scroll end -> set param",
    implementation: "engine",
  },
  { paletteId: "run_next_flow", type: "run_next_flow", name: "Run next flow", implementation: "engine" },
  { paletteId: "set_params", type: "set_params", name: "Set run params", implementation: "engine" },
  {
    paletteId: "record_post_interaction",
    type: "record_post_interaction",
    name: "Ghi nhớ tương tác post (store)",
    implementation: "engine",
  },
  {
    paletteId: "record_friend_event",
    type: "record_friend_event",
    name: "Ghi nhận follow / bạn bè (store)",
    implementation: "engine",
  },
  {
    paletteId: "account_meta",
    type: "account_meta",
    name: "Metadata account — thêm/xóa key (store)",
    implementation: "engine",
  },
  { paletteId: "log", type: "log", name: "Log", implementation: "engine" },
  { paletteId: "notification", type: "notification", name: "Notification", implementation: "engine" },
  {
    paletteId: "playwright_atomics_blank",
    type: "playwright_atomics",
    name: "Playwright atomics (chuỗi trống — tự ghép)",
    implementation: "atomics",
  },
];

/** Chỉ các step có handler Go cố định (palette + catalog tab “code”). */
export const ENGINE_ACTIONS: readonly PaletteAction[] = ACTIONS.filter((a) => a.implementation === "engine");

/** Mục “playwright_atomics” mặc định cho catalog (step trống). */
export const PLAYWRIGHT_ATOMICS_PALETTE_ACTION: PaletteAction =
  ACTIONS.find((a) => a.paletteId === "playwright_atomics_blank") ?? {
    paletteId: "playwright_atomics_blank",
    type: "playwright_atomics",
    name: "Playwright atomics (chuỗi trống — tự ghép)",
    implementation: "atomics",
  };

export interface BranchPortRule {
  id: string;
  label: string;
  color: string;
  configKey: string;
}

const DEFAULT_BRANCH_PORTS: BranchPortRule[] = [
  { id: "ok", label: "ok", color: "#16a34a", configKey: "_next_on_success" },
  { id: "err", label: "err", color: "#dc2626", configKey: "_next_on_error" },
];

const ACTION_BRANCH_PORTS: Partial<Record<ActionType, BranchPortRule[]>> = {
  start: [
    { id: "ok", label: "ok", color: "#16a34a", configKey: "_next_on_success" },
  ],
  check_login: [
    { id: "ok", label: "ok", color: "#16a34a", configKey: "_next_on_success" },
    { id: "err", label: "err", color: "#dc2626", configKey: "_next_on_error" },
  ],
  if_condition: [
    { id: "ok", label: "ok (đúng)", color: "#16a34a", configKey: "_next_on_success" },
    { id: "err", label: "err (sai)", color: "#dc2626", configKey: "_next_on_error" },
  ],
  random_yes_no: [
    { id: "yes", label: "yes (ok)", color: "#16a34a", configKey: "_next_on_success" },
    { id: "no", label: "no (err)", color: "#dc2626", configKey: "_next_on_error" },
  ],
  get_info_post: [
    { id: "ok", label: "ok", color: "#16a34a", configKey: "_next_on_success" },
    { id: "err", label: "err", color: "#dc2626", configKey: "_next_on_error" },
    { id: "empty", label: "empty", color: "#7c3aed", configKey: "_next_empty" },
  ],
  get_comments_in_page: [
    { id: "ok", label: "ok", color: "#16a34a", configKey: "_next_on_success" },
    { id: "err", label: "err", color: "#dc2626", configKey: "_next_on_error" },
    { id: "empty", label: "empty", color: "#7c3aed", configKey: "_next_empty" },
    { id: "limited", label: "limited", color: "#ea580c", configKey: "_next_limited" },
  ],
  loop_repeat: [
    { id: "loop", label: "loop", color: "#16a34a", configKey: "_next_on_success" },
    { id: "done", label: "done", color: "#0ea5e9", configKey: "_next_on_error" },
  ],
  loop_if: [
    { id: "loop", label: "loop (continue)", color: "#16a34a", configKey: "_next_on_success" },
    { id: "done", label: "done (exit)", color: "#0ea5e9", configKey: "_next_on_error" },
  ],
  run_next_flow: [
    { id: "ok", label: "ok (flow con xong)", color: "#16a34a", configKey: "_next_on_success" },
    { id: "err", label: "err (lỗi / không tải flow)", color: "#dc2626", configKey: "_next_on_error" },
  ],
};

export function getBranchPortsByType(type: ActionType): BranchPortRule[] {
  const ports = ACTION_BRANCH_PORTS[type] ?? DEFAULT_BRANCH_PORTS;
  return ports.map((p) => ({ ...p }));
}

/** Màu theo theme qua CSS variables (index.css :root[data-theme]). */
export const ui = {
  page: {
    display: "flex",
    flexDirection: "column" as const,
    gap: 12,
  },
  editorHeader: {
    display: "flex",
    alignItems: "flex-end",
    justifyContent: "space-between",
    gap: 12,
    paddingBottom: 6,
  },
  editorBody: {
    display: "grid",
    gridTemplateColumns: "320px 1fr",
    gap: 12,
    alignItems: "start",
    minHeight: 640,
  },
  leftPanel: {
    border: "1px solid var(--flow-panel-border)",
    borderRadius: 12,
    background: "var(--flow-panel-bg)",
    overflow: "hidden" as const,
  },
  leftPanelHeader: {
    padding: "12px 12px 10px",
    borderBottom: "1px solid var(--flow-panel-header-border)",
  },
  paletteList: {
    padding: 12,
    display: "flex",
    flexDirection: "column" as const,
    gap: 10,
    maxHeight: 560,
    overflow: "auto" as const,
  },
  paletteItem: {
    border: "1px solid var(--flow-palette-item-border)",
    borderRadius: 12,
    padding: "10px 12px",
    cursor: "grab",
    background: "var(--flow-palette-item-bg)",
    userSelect: "none" as const,
  },
  canvasWrap: {
    border: "1px solid var(--flow-panel-border)",
    borderRadius: 12,
    background: "var(--flow-canvas-bg)",
    overflow: "hidden" as const,
    minHeight: 640,
    position: "relative" as const,
  },
  canvasTopBar: {
    padding: 12,
    borderBottom: "1px solid var(--flow-panel-header-border)",
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    gap: 12,
  },
  lane: {
    padding: 16,
  },
  laneInner: {
    minHeight: 520,
    border: "1px dashed var(--flow-panel-border)",
    borderRadius: 12,
    background: "var(--flow-lane-inner-bg)",
    padding: 14,
    overflow: "auto" as const,
  },
  laneRow: {
    display: "grid",
    gridTemplateColumns: "140px 1fr",
    alignItems: "start",
    gap: 12,
    width: "100%",
    marginBottom: 12,
  },
  stageLabel: {
    border: "1px solid var(--flow-panel-border)",
    borderRadius: 10,
    padding: "10px 12px",
    fontWeight: 700,
    background: "var(--flow-stage-label-bg)",
    textAlign: "center" as const,
    color: "var(--text)",
  },
  stageSteps: {
    display: "flex",
    alignItems: "center",
    gap: 12,
    minHeight: 96,
    flexWrap: "wrap" as const,
  },
  stepCard: {
    width: 240,
    borderRadius: 12,
    border: "1px solid var(--flow-chain-card-border)",
    background: "var(--flow-step-card-bg)",
    boxShadow: "0 8px 20px rgba(0, 0, 0, 0.12)",
    overflow: "hidden" as const,
    cursor: "grab",
  },
  stepCardHeader: {
    padding: "10px 12px",
    borderBottom: "1px solid var(--flow-panel-header-border)",
    fontWeight: 700,
    fontSize: 13,
    color: "var(--text)",
  },
  stepCardBody: {
    padding: "10px 12px",
    fontSize: 12,
    color: "var(--muted-text)",
  },
  outputCard: {
    width: 260,
    borderRadius: 12,
    border: "1px solid var(--flow-panel-border)",
    background: "var(--flow-output-card-bg)",
    boxShadow: "0 10px 28px rgba(0, 0, 0, 0.15)",
    overflow: "hidden" as const,
  },
};

