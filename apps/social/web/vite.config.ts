import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Served from the Rust binary under an arbitrary base → relative asset URLs.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    proxy: {
      // `npm run dev` → proxy the API to a running social backend.
      '/api': { target: 'http://127.0.0.1:4520', changeOrigin: true, ws: true },
    },
  },
})
