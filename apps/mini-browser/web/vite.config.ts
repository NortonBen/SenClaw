import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Served from the Rust binary under an arbitrary base → use relative asset URLs.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4360', changeOrigin: true, ws: true },
    },
  },
})
