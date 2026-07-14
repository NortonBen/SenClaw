import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'path'

// Served from the Rust binary under an arbitrary base → use relative asset URLs.
export default defineConfig({
  base: './',
  plugins: [react()],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        'widget-mindmap-count': resolve(__dirname, 'widget/mindmap-count.html'),
        'widget-mindmap-recent': resolve(__dirname, 'widget/mindmap-recent.html'),
      },
    },
  },
  server: {
    proxy: {
      // `npm run dev` → proxy API + SSE to the running mindmap backend.
      '/api': { target: 'http://127.0.0.1:4350', changeOrigin: true, ws: true },
    },
  },
})
