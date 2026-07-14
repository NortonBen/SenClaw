import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Served by the Rust binary under an arbitrary base → relative asset URLs.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    proxy: {
      // `npm run dev` → proxy API + SSE to the running ontology backend.
      '/api': { target: 'http://127.0.0.1:4410', changeOrigin: true, ws: true },
    },
  },
})
