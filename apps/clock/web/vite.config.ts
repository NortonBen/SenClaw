import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'path'

export default defineConfig({
  base: './',
  plugins: [react()],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        'widget-clock-analog': resolve(__dirname, 'widget/clock-analog.html'),
        'widget-clock-world': resolve(__dirname, 'widget/clock-world.html'),
      },
    },
  },
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:4380', changeOrigin: true, ws: true },
    },
  },
})
