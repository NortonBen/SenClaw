import { useCallback, useRef } from 'react';

const DEFAULT_MIN_MS = 160;

/**
 * Tránh gửi/trigger submit 2 lần trong cùng một thao tác (IME, sự kiện Enter lặp, double-fire).
 */
export function useGuardedChatSubmit(onSubmit: (() => void) | undefined, minIntervalMs = DEFAULT_MIN_MS) {
  const lastRef = useRef(0);
  const inFlightRef = useRef(false);

  return useCallback(() => {
    if (!onSubmit || inFlightRef.current) return;
    const t = performance.now();
    if (t - lastRef.current < minIntervalMs) return;
    lastRef.current = t;
    inFlightRef.current = true;
    try {
      onSubmit();
    } finally {
      inFlightRef.current = false;
    }
  }, [onSubmit, minIntervalMs]);
}

/** Gọi trước khi xử lý Enter: không submit khi đang gõ IME hoặc gửi nhắn lặp do giữ phím. */
export function shouldIgnoreEnterSubmit(e: React.KeyboardEvent): boolean {
  if (e.key !== 'Enter' || e.shiftKey) return true;
  if (e.repeat) return true;
  if (e.nativeEvent.isComposing) return true;
  // Một số trình duyệt dùng 229 khi IME đang xử lý
  if ((e.nativeEvent as KeyboardEvent).keyCode === 229) return true;
  return false;
}
