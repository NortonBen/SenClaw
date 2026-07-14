import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

// base './' — the Rust binary serves the dist under an arbitrary iframe base.
export default defineConfig({
  base: './',
  plugins: [react()],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        'widget-office-status': resolve(__dirname, 'widget/office-status.html'),
      },
    },
  },
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4420', changeOrigin: true, ws: true },
    },
  },
})
