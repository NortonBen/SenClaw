import { defineConfig } from 'wxt'

// SenClaw Social — token-capturing MV3 bridge. Headless (no popup UI): all the
// work happens in the background service worker + content scripts.
export default defineConfig({
  srcDir: 'src',
  outDir: 'dist',
  manifest: {
    name: 'SenClaw Social',
    description:
      'Cầu nối SenClaw Social: bắt token phiên đăng nhập thật và replay web-API cho Facebook / TikTok / X / Instagram / YouTube. Chạy trong Chrome đã đăng nhập của bạn.',
    version: '0.1.0',
    permissions: ['storage', 'alarms', 'tabs', 'cookies', 'webRequest', 'scripting', 'declarativeNetRequest'],
    host_permissions: [
      '*://*.facebook.com/*',
      '*://*.tiktok.com/*',
      '*://*.x.com/*',
      '*://*.twitter.com/*',
      '*://*.instagram.com/*',
      '*://*.threads.net/*',
      '*://*.threads.com/*',
      '*://*.youtube.com/*',
      'http://127.0.0.1/*',
    ],
  },
})
