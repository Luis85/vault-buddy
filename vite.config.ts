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
      // Rise-only floors: floored from the 2026-07-10 adoption run
      // (93.78/90.85/90.96/95.18). When coverage rises, re-floor in the same
      // PR so the gain can't regress; never lower without a reviewed reason.
      thresholds: {
        statements: 93,
        branches: 90,
        functions: 90,
        lines: 95,
      },
    },
  },
});
