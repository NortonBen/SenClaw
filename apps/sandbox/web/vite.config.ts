import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Absolute base: the app is served at the root of its own origin (its own
// port, manifest integration url "/"). Relative asset URLs break on a hard
// refresh of a deep route — the bug apps/kaen hit and documented.
export default defineConfig({
  base: '/',
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4730', changeOrigin: true, ws: true },
    },
  },
})
