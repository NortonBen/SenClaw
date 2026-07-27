import { AutoComplete } from "antd";
import type { FlowAction } from "../../types/api";

export function StepConfigFields({
  step,
  onChange,
  onMergeConfig,
  readOnly,
  flowPickerOptions,
  excludeFlowId,
}: {
  step: FlowAction;
  onChange: (key: string, value: string) => void;
  /** Gộp nhiều khóa config (undefined = xóa khóa). Dùng cho run_next_flow: ghi next_flow_id và bỏ flow_id cũ. */
  onMergeConfig?: (patch: Record<string, string | undefined>) => void;
  readOnly?: boolean;
  /** Danh sách flow để chọn (Run next flow); loại trừ excludeFlowId ở ngoài. */
  flowPickerOptions?: { id: string; name: string }[];
  /** Flow đang sửa — không hiện trong picker (tránh gọi chính nó). */
  excludeFlowId?: string | null;
}) {
  const type = step.type;
  const config = step.config ?? {};
  const v = (k: string) => config[k] ?? "";
  const ro = Boolean(readOnly);
  const rop = ro ? ({ disabled: true, readOnly: true } as const) : {};

  switch (type) {
    case "start":
    case "playwright_atomics":
      return null;
    case "login":
      return (
        <div className="step-config">
          <input {...rop} value={v("username")} onChange={(e) => onChange("username", e.target.value)} placeholder="username (optional, default from account)" />
          <input {...rop} value={v("password")} onChange={(e) => onChange("password", e.target.value)} placeholder="password (optional, default from account)" />
        </div>
      );
    case "if_condition":
      return (
        <div className="step-config">
          <label>expect</label>
          <select disabled={ro} value={v("expect") || "visible"} onChange={(e) => onChange("expect", e.target.value)}>
            <option value="visible">visible (selector hiện)</option>
            <option value="hidden">hidden (selector ẩn)</option>
            <option value="url_contains">url_contains</option>
            <option value="url_regex">url_regex</option>
            <option value="text_contains">text_contains</option>
            <option value="always_true">always_true (stub / test)</option>
            <option value="always_false">always_false (luôn nhánh err)</option>
          </select>
          <input {...rop} value={v("selector")} onChange={(e) => onChange("selector", e.target.value)} placeholder="selector (visible|hidden|text_contains)" />
          <textarea
            {...rop}
            value={v("selectors")}
            onChange={(e) => onChange("selectors", e.target.value)}
            placeholder="selectors (mỗi dòng một CSS, thay cho selector)"
            rows={2}
          />
          <input {...rop} value={v("value")} onChange={(e) => onChange("value", e.target.value)} placeholder="value (url_contains | url_regex | text_contains)" />
          <input {...rop} value={v("pattern")} onChange={(e) => onChange("pattern", e.target.value)} placeholder="pattern (url_regex, tùy chọn nếu đã có value)" />
          <input {...rop} value={v("text")} onChange={(e) => onChange("text", e.target.value)} placeholder="text (text_contains nếu không dùng value)" />
          <input {...rop} value={v("timeout_ms")} onChange={(e) => onChange("timeout_ms", e.target.value)} placeholder="timeout_ms (default 10000)" />
        </div>
      );
    case "open_url":
      return (
        <div className="step-config">
          <input {...rop} value={v("url")} onChange={(e) => onChange("url", e.target.value)} placeholder="https://…" />
          <select disabled={ro} value={v("wait_until") || "domcontentloaded"} onChange={(e) => onChange("wait_until", e.target.value)}>
            <option value="domcontentloaded">domcontentloaded</option>
            <option value="load">load</option>
            <option value="networkidle">networkidle</option>
            <option value="commit">commit</option>
          </select>
          <input {...rop} value={v("timeout_ms")} onChange={(e) => onChange("timeout_ms", e.target.value)} placeholder="timeout_ms (default 60000)" />
        </div>
      );
    case "wait_page_ready":
      return (
        <div className="step-config">
          <label>state (Playwright load)</label>
          <select disabled={ro} value={v("state") || "load"} onChange={(e) => onChange("state", e.target.value)}>
            <option value="load">load (mặc định — tài nguyên chính đã tải)</option>
            <option value="domcontentloaded">domcontentloaded</option>
            <option value="networkidle">networkidle</option>
          </select>
          <input {...rop} value={v("timeout_ms")} onChange={(e) => onChange("timeout_ms", e.target.value)} placeholder="timeout_ms (default 30000)" />
        </div>
      );
    case "search":
      return (
        <div className="step-config">
          <input {...rop} value={v("query")} onChange={(e) => onChange("query", e.target.value)} placeholder="query" />
          <input {...rop} value={v("keyword")} onChange={(e) => onChange("keyword", e.target.value)} placeholder="keyword" />
        </div>
      );
    case "watch_video":
      return (
        <div className="step-config">
          <input {...rop} value={v("duration_ms")} onChange={(e) => onChange("duration_ms", e.target.value)} placeholder="duration_ms (optional)" />
        </div>
      );
    case "comment_video":
    case "reply_comment":
      return (
        <div className="step-config">
          <textarea {...rop} value={v("text")} onChange={(e) => onChange("text", e.target.value)} placeholder="text" rows={2} />
          {type === "reply_comment" && (
            <>
              <input {...rop} value={v("comment_index")} onChange={(e) => onChange("comment_index", e.target.value)} placeholder="comment_index (default 0)" />
              <input {...rop} value={v("reply_contains")} onChange={(e) => onChange("reply_contains", e.target.value)} placeholder="reply_contains (optional)" />
            </>
          )}
        </div>
      );
    case "share_video":
      return (
        <div className="step-config">
          <select disabled={ro} value={v("share_mode") || "copy_link"} onChange={(e) => onChange("share_mode", e.target.value)}>
            <option value="copy_link">copy_link</option>
            <option value="repost">repost</option>
            <option value="messages">messages</option>
          </select>
        </div>
      );
    case "random_delay":
      return (
        <div className="step-config">
          <input {...rop} value={v("min_ms")} onChange={(e) => onChange("min_ms", e.target.value)} placeholder="min_ms" />
          <input {...rop} value={v("max_ms")} onChange={(e) => onChange("max_ms", e.target.value)} placeholder="max_ms" />
        </div>
      );
    case "random_yes_no":
      return (
        <div className="step-config">
          <label style={{ fontSize: 12, color: "var(--muted-text, #888)" }}>yes_percent (0–100)</label>
          <input
            {...rop}
            value={v("yes_percent")}
            onChange={(e) => onChange("yes_percent", e.target.value)}
            placeholder="vd: 30 = 30% nhánh yes (ok), mặc định 50"
            type="number"
            min={0}
            max={100}
          />
          <span style={{ fontSize: 11, color: "var(--muted-text, #888)", display: "block", marginTop: 6 }}>
            Có thể dùng khóa alias: <code>probability</code>, <code>percent</code>, <code>p</code> (cùng ý nghĩa).
          </span>
        </div>
      );
    case "next_video_post":
      return (
        <div className="step-config">
          <select disabled={ro} value={v("method") || "wheel"} onChange={(e) => onChange("method", e.target.value)}>
            <option value="wheel">wheel scroll</option>
            <option value="pagedown">PageDown key</option>
            <option value="arrowdown">ArrowDown key</option>
          </select>
          <input {...rop} value={v("wait_ms")} onChange={(e) => onChange("wait_ms", e.target.value)} placeholder="wait_ms (default 1200)" />
        </div>
      );
    case "loop_repeat":
      return (
        <div className="step-config">
          <input {...rop} value={v("repeat_times")} onChange={(e) => onChange("repeat_times", e.target.value)} placeholder="repeat_times (default 3)" />
        </div>
      );
    case "loop_if":
      return (
        <div className="step-config">
          <input {...rop} value={v("param_key") || v("key")} onChange={(e) => onChange("param_key", e.target.value)} placeholder="param_key (run param cần kiểm tra)" />
          <select disabled={ro} value={v("operator") || "equals"} onChange={(e) => onChange("operator", e.target.value)}>
            <option value="equals">equals</option>
            <option value="not_equals">not_equals</option>
            <option value="contains">contains</option>
            <option value="truthy">truthy</option>
            <option value="falsy">falsy</option>
            <option value="empty">empty</option>
            <option value="not_empty">not_empty</option>
            <option value="regex">regex</option>
            <option value="gt">gt</option>
            <option value="gte">gte</option>
            <option value="lt">lt</option>
            <option value="lte">lte</option>
          </select>
          <input {...rop} value={v("value")} onChange={(e) => onChange("value", e.target.value)} placeholder="value (khi operator cần so sánh)" />
          <input {...rop} value={v("max_loops")} onChange={(e) => onChange("max_loops", e.target.value)} placeholder="max_loops (optional safety cap)" />
          <span style={{ fontSize: 11, color: "var(--muted-text, #888)", display: "block", marginTop: 6 }}>
            Điều kiện đúng - nhánh <code>done</code> (error), điều kiện sai - nhánh <code>loop</code> (success).
          </span>
        </div>
      );
    case "check_scroll_end":
      return (
        <div className="step-config">
          <input {...rop} value={v("selector")} onChange={(e) => onChange("selector", e.target.value)} placeholder="selector của vùng scroll (bắt buộc)" />
          <input {...rop} value={v("tolerance_px")} onChange={(e) => onChange("tolerance_px", e.target.value)} placeholder="tolerance_px (default 1)" />
          <input
            {...rop}
            value={v("output_param_key")}
            onChange={(e) => onChange("output_param_key", e.target.value)}
            placeholder="output_param_key (default: is_scroll_end)"
          />
          <input {...rop} value={v("value_true")} onChange={(e) => onChange("value_true", e.target.value)} placeholder="value_true (default true)" />
          <input {...rop} value={v("value_false")} onChange={(e) => onChange("value_false", e.target.value)} placeholder="value_false (default false)" />
        </div>
      );
    case "run_next_flow": {
      const currentTarget = v("next_flow_id") || v("flow_id");
      const canPick = !ro && typeof onMergeConfig === "function" && Array.isArray(flowPickerOptions);
      const pickerRows = canPick
        ? flowPickerOptions.filter((f) => !excludeFlowId || f.id !== excludeFlowId)
        : [];
      const acOptions = pickerRows.map((f) => ({
        value: f.id,
        label: `${f.name?.trim() || "(không tên)"} — ${f.id}`,
      }));

      const applyTarget = (raw: string) => {
        const t = raw.trim();
        if (!t) {
          onMergeConfig?.({ next_flow_id: undefined, flow_id: undefined });
          return;
        }
        onMergeConfig?.({ next_flow_id: t, flow_id: undefined });
      };

      return (
        <div className="step-config">
          {canPick ? (
            <AutoComplete
              style={{ width: "100%" }}
              value={currentTarget}
              options={acOptions}
              allowClear
              placeholder="Gõ để tìm theo tên hoặc ID — hoặc nhập template"
              filterOption={(input, option) => {
                const q = input.trim().toLowerCase();
                if (!q) return true;
                const label = String(option?.label ?? "").toLowerCase();
                const val = String(option?.value ?? "").toLowerCase();
                return label.includes(q) || val.includes(q);
              }}
              onChange={(val) => applyTarget(String(val ?? ""))}
              onSelect={(val) => applyTarget(String(val))}
              disabled={ro}
            />
          ) : (
            <input
              {...rop}
              value={currentTarget}
              onChange={(e) => onChange("next_flow_id", e.target.value)}
              placeholder="next_flow_id (id flow trong store)"
            />
          )}
          <span style={{ fontSize: 11, color: "var(--muted-text, #888)", display: "block", marginTop: 6 }}>
            {canPick
              ? "Chọn flow từ danh sách (đã loại flow đang sửa) hoặc nhập ID / template. Alias cũ: "
              : "Alias: "}
            <code>flow_id</code> nếu chỉ có khóa cũ. Hỗ trợ template trong giá trị.
          </span>
        </div>
      );
    }
    case "set_params":
      return (
        <div className="step-config">
          <input {...rop} value={v("key")} onChange={(e) => onChange("key", e.target.value)} placeholder="key (optional, update 1 key)" />
          <input
            {...rop}
            value={v("value")}
            onChange={(e) => onChange("value", e.target.value)}
            placeholder="value (hỗ trợ template: {{param.xxx}}, {{prev.xxx}}, {{step.id.xxx}})"
          />
          <textarea
            {...rop}
            value={v("updates")}
            onChange={(e) => onChange("updates", e.target.value)}
            placeholder={"updates (mỗi dòng key=value)\ncomment_text=hello\ntarget_url=https://..."}
            rows={4}
          />
        </div>
      );
    case "record_post_interaction":
      return (
        <div className="step-config">
          <input {...rop} value={v("post_key")} onChange={(e) => onChange("post_key", e.target.value)} placeholder="post_key (hoặc video_id) — template OK" />
          <input
            {...rop}
            value={v("interaction")}
            onChange={(e) => onChange("interaction", e.target.value)}
            placeholder="interaction / interaction_type (mặc định: interaction)"
          />
          <input {...rop} value={v("post_url")} onChange={(e) => onChange("post_url", e.target.value)} placeholder="post_url (optional)" />
          <input {...rop} value={v("author_username")} onChange={(e) => onChange("author_username", e.target.value)} placeholder="author_username (optional)" />
          <textarea {...rop} value={v("extra_json")} onChange={(e) => onChange("extra_json", e.target.value)} placeholder="extra_json (optional)" rows={2} />
        </div>
      );
    case "record_friend_event":
      return (
        <div className="step-config">
          <label>event</label>
          <select disabled={ro} value={v("event") || "follow"} onChange={(e) => onChange("event", e.target.value)}>
            <option value="follow">follow / friend_add / add</option>
            <option value="unfollow">unfollow / friend_remove / remove</option>
            <option value="friend_add">friend_add (lưu follow)</option>
            <option value="friend_remove">friend_remove (lưu unfollow)</option>
          </select>
          <input {...rop} value={v("target_username")} onChange={(e) => onChange("target_username", e.target.value)} placeholder="target_username / peer_username" />
          <input {...rop} value={v("target_user_id")} onChange={(e) => onChange("target_user_id", e.target.value)} placeholder="target_user_id (optional)" />
          <input {...rop} value={v("notes")} onChange={(e) => onChange("notes", e.target.value)} placeholder="notes (optional)" />
        </div>
      );
    case "account_meta":
      return (
        <div className="step-config">
          <label>operation</label>
          <select disabled={ro} value={v("operation") || "upsert"} onChange={(e) => onChange("operation", e.target.value)}>
            <option value="upsert">upsert — ghi/ghi đè giá trị</option>
            <option value="delete">delete — xóa key</option>
          </select>
          <input {...rop} value={v("meta_key")} onChange={(e) => onChange("meta_key", e.target.value)} placeholder="meta_key (hoặc key)" />
          <input
            {...rop}
            value={v("meta_value")}
            onChange={(e) => onChange("meta_value", e.target.value)}
            placeholder="meta_value / value (upsert; hỗ trợ template run)"
          />
        </div>
      );
    case "get_comments_in_page":
      return (
        <div className="step-config">
          <select disabled={ro} value={v("extract_mode") || "all"} onChange={(e) => onChange("extract_mode", e.target.value)}>
            <option value="all">extract all comments</option>
            <option value="limit">extract by limit</option>
          </select>
          {(v("extract_mode") || "all") === "limit" ? (
            <input {...rop} value={v("limit")} onChange={(e) => onChange("limit", e.target.value)} placeholder="limit (default 100)" />
          ) : null}
          <input {...rop} value={v("max_scrolls")} onChange={(e) => onChange("max_scrolls", e.target.value)} placeholder="max_scrolls (default 20)" />
        </div>
      );
    case "reply_comment_ai":
      return (
        <div className="step-config">
          <textarea {...rop} value={v("text")} onChange={(e) => onChange("text", e.target.value)} placeholder="text (AI output)" rows={2} />
          <input {...rop} value={v("comment_index")} onChange={(e) => onChange("comment_index", e.target.value)} placeholder="comment_index (default 0)" />
          <input {...rop} value={v("reply_contains")} onChange={(e) => onChange("reply_contains", e.target.value)} placeholder="reply_contains (optional)" />
        </div>
      );
    case "ai_gent_comment":
      return (
        <div className="step-config">
          <label>mode</label>
          <select disabled={ro} value={v("mode") || "ai"} onChange={(e) => onChange("mode", e.target.value)}>
            <option value="ai">ai - phân tích HTML post và sinh comment</option>
            <option value="select_comment">select_comment - random từ danh sách, không dùng AI</option>
          </select>
          <input
            {...rop}
            value={v("output_param_key")}
            onChange={(e) => onChange("output_param_key", e.target.value)}
            placeholder="output_param_key (default: comment_text)"
          />
          <textarea
            {...rop}
            value={v("comments_list")}
            onChange={(e) => onChange("comments_list", e.target.value)}
            placeholder={"comments_list (mỗi dòng 1 comment)\nBài hay quá!\nNội dung cuốn thật"}
            rows={4}
          />
          <textarea
            {...rop}
            value={v("instruction")}
            onChange={(e) => onChange("instruction", e.target.value)}
            placeholder="instruction (optional) - hướng dẫn thêm cho AI"
            rows={2}
          />
          <input {...rop} value={v("post_hint")} onChange={(e) => onChange("post_hint", e.target.value)} placeholder="post_hint (optional)" />
          <span style={{ fontSize: 11, color: "var(--muted-text, #888)", display: "block", marginTop: 6 }}>
            Step sau dùng comment qua template <code>{`{{param.comment_text}}`}</code> hoặc key bạn đặt ở <code>output_param_key</code>.
          </span>
        </div>
      );
    case "ai_playwright_agent":
      return (
        <div className="step-config">
          <textarea
            {...rop}
            value={v("instruction") || v("goal")}
            onChange={(e) => onChange("instruction", e.target.value)}
            placeholder="instruction / goal — mô tả việc LLM cần làm trên trang (bắt buộc)"
            rows={4}
          />
          <input {...rop} value={v("max_steps")} onChange={(e) => onChange("max_steps", e.target.value)} placeholder="max_steps (default 18, max 80)" />
          <input {...rop} value={v("temperature")} onChange={(e) => onChange("temperature", e.target.value)} placeholder="temperature (0–2, default 0.25)" />
          <input
            {...rop}
            value={v("max_completion_tokens")}
            onChange={(e) => onChange("max_completion_tokens", e.target.value)}
            placeholder="max_completion_tokens — giới hạn token sinh mỗi lượt (256–20000, default 2048)"
          />
          <label style={{ fontSize: 12, display: "block", marginTop: 6 }}>tool_choice (OpenAI-compatible)</label>
          <select disabled={ro} value={v("tool_choice") || "auto"} onChange={(e) => onChange("tool_choice", e.target.value)}>
            <option value="auto">auto — mặc định API</option>
            <option value="required">required — bắt buộc gọi tool mỗi lượt (khuyến nghị với model nhỏ / Qwen)</option>
          </select>
          <textarea
            {...rop}
            value={v("system_extra")}
            onChange={(e) => onChange("system_extra", e.target.value)}
            placeholder="system_extra — quy tắc thêm cho system prompt (optional)"
            rows={2}
          />
          <span style={{ fontSize: 11, color: "var(--muted-text, #888)", display: "block", marginTop: 8 }}>
            Agent chỉ gửi text (FlatDomTree / <code>interactive_dom_outline</code>), không upload ảnh chụp màn hình lên LLM. LLM trong Settings. Output:{" "}
            <code>ai_agent_summary</code>, <code>ai_agent_transcript</code>.
          </span>
        </div>
      );
    case "log":
      return (
        <div className="step-config">
          <textarea {...rop} value={v("message")} onChange={(e) => onChange("message", e.target.value)} placeholder="message" rows={2} />
        </div>
      );
    case "notification":
      return (
        <div className="step-config">
          <input {...rop} value={v("title")} onChange={(e) => onChange("title", e.target.value)} placeholder="title (optional)" />
          <textarea {...rop} value={v("message")} onChange={(e) => onChange("message", e.target.value)} placeholder="message/body" rows={2} />
        </div>
      );
    default:
      return null;
  }
}

