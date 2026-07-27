import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // Absolute, NOT "./". The binary serves this SPA at its own origin root and
  // falls back to index.html for client-side routes, so a relative base resolves
  // assets against the current route: loading /stories/4 directly requested
  // /stories/assets/index-*.js, got index.html back from the fallback with a 200,
  // and the page rendered blank. Every deep link and every refresh was broken.
  base: "/",
  resolve: { alias: { "@": path.resolve(__dirname, "src") } },
  server: {
    port: 5175,
    proxy: {
      "/api": { target: "http://127.0.0.1:4470", changeOrigin: true },
      "/health": { target: "http://127.0.0.1:4470", changeOrigin: true },
      "/ws": { target: "ws://127.0.0.1:4470", ws: true, changeOrigin: true },
    },
  },
});
