import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// base './' — the Rust binary serves the dist under an arbitrary iframe base.
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4440', changeOrigin: true, ws: true },
    },
  },
})
