import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// base '/' — the Rust server falls back to index.html for unknown
// extension-less paths, so absolute asset URLs always resolve.
export default defineConfig({
  base: "/",
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  server: {
    proxy: {
      "/api": { target: "http://127.0.0.1:4310", changeOrigin: true },
      "/health": { target: "http://127.0.0.1:4310", changeOrigin: true },
    },
  },
});
