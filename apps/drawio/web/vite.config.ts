import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Served from the Rust binary under an arbitrary base → use relative asset URLs.
// Single page, no router — './' is safe both on the app's own origin and under
// the daemon's /api/space/apps/drawio/proxy/ fallback.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    proxy: {
      // `npm run dev` → proxy API + SSE and the downloaded editor webapp to the
      // running drawio backend.
      '/api': { target: 'http://127.0.0.1:4610', changeOrigin: true, ws: true },
      '/drawio': { target: 'http://127.0.0.1:4610', changeOrigin: true },
    },
  },
})
