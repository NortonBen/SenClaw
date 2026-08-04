import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Absolute base, not './'. BA Studio is served at the root of its own origin
// (its own port, manifest integration url "/"), and it is opened with
// deep-link queries (`?project=…&feature=…`) from chat. Relative asset URLs
// break on a hard refresh of such a URL — the bug apps/kaen hit and documented.
export default defineConfig({
  base: '/',
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4740', changeOrigin: true, ws: true },
    },
  },
})
