/**
 * Mở URL ngoài trên trình duyệt THẬT của máy người dùng.
 *
 * Trong desktop SenClaw, Space App chạy trong WKWebView/WebView2 nhúng — một
 * <a href> thường sẽ điều hướng chính webview đó, "nuốt" luôn UI của app. Thứ
 * tự ưu tiên (xem docs/space-app-open-external.md):
 *   1. Bridge Flutter `senclawOpenExternal` → mở browser hệ thống.
 *   2. Trình duyệt thật (app chạy standalone) → window.open tab mới.
 * Webview desktop còn một lưới an toàn (shouldOverrideUrlLoading) chặn mọi
 * điều hướng ra origin ngoài, nên link nào lọt hook này vẫn không kẹt trong app.
 *
 * OAuth BẮT BUỘC đi đường này: Google từ chối webview nhúng
 * (disallowed_useragent), nên màn hình cấp quyền phải mở trên browser thật.
 */
export function openExternal(url: string) {
  const w = window as unknown as {
    flutter_inappwebview?: { callHandler?: (name: string, ...args: unknown[]) => unknown };
  };
  const fiw = w.flutter_inappwebview;
  if (fiw && typeof fiw.callHandler === "function") {
    try {
      fiw.callHandler("senclawOpenExternal", url);
      return;
    } catch {
      /* fall through to window.open */
    }
  }
  const win = window.open(url, "_blank", "noopener");
  if (!win) {
    // Popup blocked (sandboxed iframe / old webview): last resort — ask our
    // backend to have the daemon open it on the host browser.
    void fetch("/api/open-url", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ url }),
    }).catch(() => {});
  }
}

/**
 * Hook toàn cục (capture phase): mọi click trái không kèm phím bổ trợ vào
 * <a href> http(s) khác origin app → chặn điều hướng mặc định và chuyển cho
 * openExternal. Gắn một lần trong main.tsx; các component không cần sửa gì.
 */
export function installExternalLinkHook() {
  document.addEventListener(
    "click",
    (ev) => {
      if (ev.defaultPrevented || ev.button !== 0) return;
      if (ev.metaKey || ev.ctrlKey || ev.shiftKey || ev.altKey) return;
      const target = ev.target as Element | null;
      const anchor = target?.closest?.("a[href]");
      if (!anchor) return;
      let url: URL;
      try {
        url = new URL(anchor.getAttribute("href")!, window.location.href);
      } catch {
        return;
      }
      if (url.protocol !== "http:" && url.protocol !== "https:") return;
      if (url.origin === window.location.origin) return;
      ev.preventDefault();
      openExternal(url.href);
    },
    true,
  );
}
