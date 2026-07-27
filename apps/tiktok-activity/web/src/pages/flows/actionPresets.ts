import type { FlowAtomic } from "../../types/api";

/** Tương đương engageLike (engage.go): thử lần lượt các selector like TikTok. */
export const PRESET_ATOMICS_LIKE_VIDEO: FlowAtomic[] = [
  {
    kind: "click",
    params: {
      selectors: [
        `[data-e2e="like-icon"]`,
        `[data-e2e="browse-like-icon"]`,
        `button[aria-label*="Like" i]`,
        `[aria-label*="likes" i]`,
      ].join("\n"),
      timeout_ms: "20000",
      click_timeout_ms: "8000",
    },
  },
];

/** Tương đương next_video_post method=wheel (engage.go). */
export const PRESET_ATOMICS_NEXT_VIDEO_WHEEL: FlowAtomic[] = [
  { kind: "scroll", params: { delta_x: "0", delta_y: "1400", method: "wheel" } },
  { kind: "wait_ms", params: { ms: "1200" } },
];

/** Tương đương next_video_post method=pagedown. */
export const PRESET_ATOMICS_NEXT_VIDEO_PAGEDOWN: FlowAtomic[] = [
  { kind: "press", params: { key: "PageDown" } },
  { kind: "wait_ms", params: { ms: "1200" } },
];

/** Tương đương next_video_post method=arrowdown. */
export const PRESET_ATOMICS_NEXT_VIDEO_ARROWDOWN: FlowAtomic[] = [
  { kind: "press", params: { key: "ArrowDown" } },
  { kind: "wait_ms", params: { ms: "1200" } },
];
