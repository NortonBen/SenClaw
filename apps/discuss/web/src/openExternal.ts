/**
 * Mở URL trên trình duyệt THẬT của máy (chuẩn Space App —
 * docs/space-app-open-external.md, helper canonical apps/zeach):
 *   1. Bridge Flutter `senclawOpenExternal` → browser hệ thống (desktop webview).
 *   2. App chạy standalone → window.open tab mới.
 */
export function openExternal(url: string) {
  const w = window as unknown as {
    flutter_inappwebview?: { callHandler?: (name: string, ...args: unknown[]) => unknown }
  }
  const fiw = w.flutter_inappwebview
  if (fiw && typeof fiw.callHandler === 'function') {
    try {
      fiw.callHandler('senclawOpenExternal', url)
      return
    } catch {
      /* fall through to window.open */
    }
  }
  window.open(url, '_blank', 'noopener')
}

/** Có đang chạy trong desktop webview không (có bridge Flutter). */
function inDesktopWebview(): boolean {
  const w = window as unknown as { flutter_inappwebview?: unknown }
  return Boolean(w.flutter_inappwebview)
}

/**
 * Tải một endpoint download của app (Content-Disposition attachment).
 * - Browser thật: <a download> cùng origin — tải thẳng.
 * - Desktop webview: webview thường nuốt download → mở bằng browser hệ thống,
 *   header attachment sẽ khiến browser lưu file.
 */
export function downloadPath(apiPath: string) {
  const url = new URL(apiPath, window.location.origin).href
  if (inDesktopWebview()) {
    openExternal(url)
    return
  }
  const a = document.createElement('a')
  a.href = url
  a.download = ''
  document.body.appendChild(a)
  a.click()
  a.remove()
}

/**
 * Hook toàn cục (capture): click trái thường vào <a href> http(s) KHÁC origin
 * → chuyển cho openExternal, khỏi kẹt trong webview. Gắn một lần ở main.tsx.
 */
export function installExternalLinkHook() {
  document.addEventListener(
    'click',
    (ev) => {
      if (ev.defaultPrevented || ev.button !== 0) return
      if (ev.metaKey || ev.ctrlKey || ev.shiftKey || ev.altKey) return
      const target = ev.target as Element | null
      const anchor = target?.closest?.('a[href]')
      if (!anchor) return
      let url: URL
      try {
        url = new URL(anchor.getAttribute('href')!, window.location.href)
      } catch {
        return
      }
      if (url.protocol !== 'http:' && url.protocol !== 'https:') return
      if (url.origin === window.location.origin) return
      ev.preventDefault()
      openExternal(url.href)
    },
    true,
  )
}
