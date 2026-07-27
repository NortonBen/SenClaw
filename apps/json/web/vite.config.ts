import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// base '/' (not './') — the SPA uses BrowserRouter, so deep routes such as
// /json-to-csv must resolve their asset URLs from the server root. The Rust
// server falls back to index.html for unknown paths.
export default defineConfig({
  base: "/",
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  publicDir: "public",
  server: {
    proxy: {
      "/api": { target: "http://127.0.0.1:4540", changeOrigin: true },
    },
  },
});
