import { useCallback, useRef } from 'react';

const DEFAULT_MIN_MS = 400;

/**
 * Tránh gửi/trigger submit 2 lần trong cùng một thao tác (IME, sự kiện Enter lặp, double-fire).
 * Submit được defer qua microtask để giá trị textarea kịp commit sau compositionend.
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
    queueMicrotask(() => {
      try {
        onSubmit();
      } finally {
        inFlightRef.current = false;
      }
    });
  }, [onSubmit, minIntervalMs]);
}

/** Gọi trước khi xử lý Enter: không submit khi đang gõ IME hoặc gửi nhắn lặp do giữ phím. */
export function shouldIgnoreEnterSubmit(e: React.KeyboardEvent): boolean {
  if (e.key !== 'Enter' || e.shiftKey) return true;
  if (e.repeat) return true;
  if (e.nativeEvent.isComposing) return true;
  // Một số trình duyệt dùng 229 khi IME đang xử lý
  if ((e.nativeEvent as KeyboardEvent).keyCode === 229) return true;
  // Windows IME
  if ((e.nativeEvent as KeyboardEvent).key === 'Process') return true;
  return false;
}

/**
 * Theo dõi IME composition: Enter xác nhận gõ không được gửi chat,
 * và Enter ngay sau compositionend cũng bị chặn (tránh gửi 2 lần / tách tin).
 */
export function useChatCompositionGuard() {
  const composingRef = useRef(false);
  /** Enter bấm trong lúc đang composition — chặn Enter kế tiếp sau compositionend. */
  const suppressNextEnterRef = useRef(false);

  const onCompositionStart = useCallback(() => {
    composingRef.current = true;
  }, []);

  const onCompositionEnd = useCallback(() => {
    composingRef.current = false;
  }, []);

  const shouldBlockEnterSubmit = useCallback((e: React.KeyboardEvent) => {
    if (shouldIgnoreEnterSubmit(e)) return true;

    if (composingRef.current) {
      if (e.key === 'Enter' && !e.shiftKey) suppressNextEnterRef.current = true;
      return true;
    }

    if (suppressNextEnterRef.current && e.key === 'Enter' && !e.shiftKey) {
      suppressNextEnterRef.current = false;
      return true;
    }

    return false;
  }, []);

  return { onCompositionStart, onCompositionEnd, shouldBlockEnterSubmit };
}
