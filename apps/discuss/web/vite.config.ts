import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// base '/' (KHÔNG './'): app chạy ở root origin riêng và được mở kèm query
// deep-link (?discussion=…) — asset tương đối vỡ khi hard refresh (bài học kaen).
export default defineConfig({
  base: '/',
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4760', changeOrigin: true, ws: true },
    },
  },
})
