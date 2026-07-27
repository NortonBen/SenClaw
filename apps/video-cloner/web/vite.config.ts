import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // The SenClaw Space App binary serves the build under an arbitrary base path
  // inside an iframe — assets must be referenced relatively.
  base: "./",
  resolve: { alias: { "@": path.resolve(__dirname, "src") } },
  server: {
    port: 5177,
    proxy: {
      "/api": { target: "http://127.0.0.1:4480", changeOrigin: true },
      "/health": { target: "http://127.0.0.1:4480", changeOrigin: true },
      "/ws": { target: "ws://127.0.0.1:4480", ws: true, changeOrigin: true },
    },
  },
});
