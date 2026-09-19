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
      // The include glob is NOT root-anchored, so it also matches a
      // nested `.../src/` — e.g. docs/concepts/<name>/implementation-
      // starter/src/. That counted 191 statements of prototype code
      // (one file at 2.7%) as app source and pushed the global figure
      // under the floors below, on a commit that touched no src/ file
      // at all. Concept drops are reference material, never bundled and
      // never imported by the app, so they are not what these floors
      // measure. Excluding them restores the set the floors were
      // calibrated against; it does NOT relax the floors themselves.
      exclude: ["docs/**"],
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
