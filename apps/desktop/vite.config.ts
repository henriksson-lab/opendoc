import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    host: "0.0.0.0",
    port: 10084,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    minify: false,
  },
});
