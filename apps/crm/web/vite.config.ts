import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'path'

// Served from the Rust binary under an arbitrary base → use relative asset URLs.
export default defineConfig({
  base: './',
  plugins: [react()],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        'widget-crm-overview': resolve(__dirname, 'widget/crm-overview.html'),
        'widget-crm-pipeline': resolve(__dirname, 'widget/crm-pipeline.html'),
      },
    },
  },
  server: {
    proxy: {
      // `npm run dev` → proxy API + SSE to the running crm backend.
      // Override the port with CRM_API_PORT when running a second instance.
      '/api': {
        target: `http://127.0.0.1:${process.env.CRM_API_PORT ?? 4390}`,
        changeOrigin: true,
        ws: true,
      },
    },
  },
})
