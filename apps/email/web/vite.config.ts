import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'path'

// https://vite.dev/config/
export default defineConfig({
  base: "./",
  plugins: [react()],
  build: {
    rollupOptions: {
      input: {
        main: resolve(__dirname, 'index.html'),
        'widget-email-unread': resolve(__dirname, 'widget/email-unread.html'),
        'widget-email-inbox': resolve(__dirname, 'widget/email-inbox.html'),
      },
    },
  },
})
