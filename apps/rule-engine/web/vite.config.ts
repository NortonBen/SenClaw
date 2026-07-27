import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// `base: './'` is mandatory: the UI is served both at http://127.0.0.1:4550/
// and behind /api/space/apps/rule-engine/proxy/ inside the SenClaw shell.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4550', changeOrigin: true, ws: true },
    },
  },
})
