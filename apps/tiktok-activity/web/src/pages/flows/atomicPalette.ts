/** Mẫu kéo từ palette → thả vào chuỗi atomic (kind + params mặc định). */
export interface AtomicPaletteItem {
  id: string;
  label: string;
  kind: string;
  defaultParams: Record<string, string>;
}

export const ATOMIC_PALETTE: readonly AtomicPaletteItem[] = [
  {
    id: "goto_tiktok",
    label: "Goto TikTok",
    kind: "goto",
    defaultParams: {
      url: "https://www.tiktok.com/",
      wait_until: "domcontentloaded",
      timeout_ms: "45000",
    },
  },
  {
    id: "click_login_candidates",
    label: "Click đăng nhập (nhiều selector)",
    kind: "click",
    defaultParams: {
      selectors: ['a[href*="/login"]', 'button:has-text("Log in")', 'button:has-text("Sign in")'].join("\n"),
      timeout_ms: "20000",
    },
  },
  {
    id: "fill_username_field",
    label: "Fill username (selector)",
    kind: "fill",
    defaultParams: {
      selector: 'input[name="username"]',
      value_source: "account_username",
    },
  },
  {
    id: "fill_password_field",
    label: "Fill password (account)",
    kind: "fill",
    defaultParams: {
      selector: 'input[type="password"]',
      value_source: "account_password",
    },
  },
  {
    id: "fill_from_step_param",
    label: "Fill từ param của step (nhập key)",
    kind: "fill",
    defaultParams: {
      selector: "",
      value_source: "action_param",
      param_key: "my_key",
    },
  },
  {
    id: "press_enter",
    label: "Phím Enter (toàn trang)",
    kind: "press",
    defaultParams: { key: "Enter" },
  },
  {
    id: "press_enter_focus",
    label: "Focus input + Enter",
    kind: "press",
    defaultParams: {
      selector: 'input[type="password"]',
      key: "Enter",
      timeout_ms: "15000",
    },
  },
  {
    id: "wait_ms_short",
    label: "Chờ (ms)",
    kind: "wait_ms",
    defaultParams: { ms: "800" },
  },
  {
    id: "wait_load",
    label: "Chờ trang load",
    kind: "wait_load",
    defaultParams: { state: "load", timeout_ms: "30000" },
  },
  {
    id: "scroll_down_wheel",
    label: "Scroll xuống (wheel, viewport)",
    kind: "scroll",
    defaultParams: { delta_x: "0", delta_y: "800", method: "wheel" },
  },
  {
    id: "scroll_right_js",
    label: "Scroll phải (scroll_by window)",
    kind: "scroll",
    defaultParams: { delta_x: "400", delta_y: "0", method: "scroll_by" },
  },
  {
    id: "assert_visible",
    label: "Assert: phần tử hiện",
    kind: "assert",
    defaultParams: { expect: "visible", selector: "", timeout_ms: "10000" },
  },
  {
    id: "assert_url_tiktok",
    label: "Assert: URL chứa tiktok.com",
    kind: "assert",
    defaultParams: { expect: "url_contains", value: "tiktok.com", timeout_ms: "15000" },
  },
  {
    id: "assert_text",
    label: "Assert: trang có chứa text",
    kind: "assert",
    defaultParams: { expect: "text_contains", value: "", timeout_ms: "10000" },
  },
  {
    id: "click_unless_contains",
    label: "Click nếu nút chưa chứa text (bỏ qua nếu đã Bạn bè / Following)",
    kind: "click_unless_contains",
    defaultParams: {
      selectors: 'button[data-testid="tux-web-button"]',
      unless_substrings: "bạn bè\nđang follow\nfollowing\nfriends",
      timeout_ms: "20000",
      click_timeout_ms: "8000",
    },
  },
  {
    id: "click_single",
    label: "Click một selector",
    kind: "click",
    defaultParams: { selector: "", timeout_ms: "20000" },
  },
  {
    id: "click_button_nested_text",
    label: "Click nút theo text (span/div lồng nhau)",
    kind: "click_button_text",
    defaultParams: {
      text: "",
      mode: "nested",
      match: "contains",
      base_selector: `button, [role="button"], input[type="button"], input[type="submit"]`,
      timeout_ms: "20000",
      click_timeout_ms: "8000",
    },
  },
  {
    id: "click_button_a11y_name",
    label: "Click theo accessible name (GetByRole)",
    kind: "click_button_text",
    defaultParams: {
      text: "",
      mode: "role",
      role: "button",
      match: "contains",
      timeout_ms: "20000",
      click_timeout_ms: "8000",
    },
  },
  {
    id: "fill_text",
    label: "Fill text tĩnh",
    kind: "fill",
    defaultParams: { selector: "", value_source: "literal", text: "" },
  },
] as const;

export const MIME_ATOMIC_TEMPLATE = "application/x-flow-atomic-template";
export const MIME_ATOMIC_REORDER = "application/x-flow-atomic-reorder";
