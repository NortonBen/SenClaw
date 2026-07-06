import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  base: "/deepwiki/",
  plugins: [react()],
  server: {
    proxy: { '/api': { target: 'http://127.0.0.1:4340', changeOrigin: true } },
  },
})
