import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  test: {
    environment: "happy-dom",
    coverage: {
      // Istanbul over v8: portable coverage/ artifact and stable numbers for
      // the rise-only floors below.
      provider: "istanbul",
      reporter: ["json-summary", "text"],
      include: ["src/**/*.{ts,vue}"],
      // Rise-only floors: floored (Math.floor) from the 2026-07-12
      // settings-autosave+tabs run (95.15/91.09/93.15/96.92); the original
      // 2026-07-10 adoption run was 93.78/90.85/90.96/95.18. When coverage
      // rises, re-floor in the same PR so the gain can't regress; never lower
      // without a reviewed reason.
      thresholds: {
        statements: 95,
        branches: 91,
        functions: 93,
        lines: 96,
      },
    },
  },
});
