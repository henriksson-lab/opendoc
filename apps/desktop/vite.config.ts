import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    host: "0.0.0.0",
    port: 10084,
    strictPort: true,
    // This public development host resolves directly to this machine. Keep
    // Vite's DNS-rebinding protection enabled while allowing only this name.
    allowedHosts: ["beagle.henlab.org"],
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    minify: false,
  },
});
