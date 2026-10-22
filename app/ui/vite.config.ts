/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The desktop shell loads the built files; `npm run dev:mock` serves the same
// UI in a browser with a scripted backend so it can be previewed anywhere.
export default defineConfig(({ mode }) => ({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  define: { __MOCK__: JSON.stringify(mode === "mock") },
  build: { target: "es2022", outDir: "dist", sourcemap: false, chunkSizeWarningLimit: 1500 },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: false,
  },
}));
