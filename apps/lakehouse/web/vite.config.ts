import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Served by the Rust binary both directly (:4560) and under the daemon proxy
// (/api/space/apps/lakehouse/proxy/) → relative asset URLs are mandatory.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    proxy: {
      // ws: true also covers /api/ws/dashboard.
      '/api': { target: 'http://127.0.0.1:4560', changeOrigin: true, ws: true },
    },
  },
})
