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
        'widget-luna-today': resolve(__dirname, 'widget/luna-today.html'),
        'widget-luna-almanac': resolve(__dirname, 'widget/luna-almanac.html'),
      },
    },
  },
  server: {
    proxy: {
      // `npm run dev` → proxy API + SSE to the running luna-calendar backend.
      '/api': { target: 'http://127.0.0.1:4351', changeOrigin: true, ws: true },
    },
  },
})
