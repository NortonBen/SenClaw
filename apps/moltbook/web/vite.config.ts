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
        'widget-moltbook-feed': resolve(__dirname, 'widget/moltbook-feed.html'),
        'widget-moltbook-drafts': resolve(__dirname, 'widget/moltbook-drafts.html'),
      },
    },
  },
  server: {
    proxy: {
      // `npm run dev` → proxy API + SSE to the running moltbook backend.
      '/api': { target: 'http://127.0.0.1:4430', changeOrigin: true, ws: true },
    },
  },
})
