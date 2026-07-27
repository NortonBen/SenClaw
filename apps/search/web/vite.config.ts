import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Served from the Rust binary both directly (:4530) and under the daemon proxy
// (/api/space/apps/search/proxy/) → relative asset URLs are mandatory.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4530', changeOrigin: true, ws: true },
    },
  },
})
