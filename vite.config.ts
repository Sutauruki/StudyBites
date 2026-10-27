import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built files from ../dist; in Codespaces the same build runs
// in the browser against the mock backend (Brief 11.3).
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: true,
  },
  build: {
    target: "es2021",
    sourcemap: false,
    // Keep the installer small: one small chunk, no legacy polyfills.
    chunkSizeWarningLimit: 600,
  },
});
