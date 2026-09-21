import { defineConfig, devices } from "@playwright/test";

/**
 * The real-browser layout checks (docs/Gaps.md GAP-162).
 *
 * Vitest runs on happy-dom, which has NO layout engine: it cannot tell a
 * 64px-tall element from a collapsed one, and every geometry assertion the
 * suite makes is really a class assertion. Two window-sizing defects shipped
 * through it. This config drives the REAL built bundle in Chromium so the
 * assertions can measure pixels.
 *
 * It deliberately serves `dist/`, not the dev server: the production build is
 * what ships, and Tailwind's purge runs only there — a class that survives in
 * dev and is purged in the build would pass a dev-server check and fail the
 * app. `npm run build` is the prerequisite, which is also the order CI uses.
 */
export default defineConfig({
  testDir: "./tests/e2e",
  // The suite measures geometry, so parallel workers sharing a display are
  // fine but a retry that masks a real layout regression is not.
  retries: 0,
  reporter: process.env.CI ? [["list"]] : [["list"]],
  use: {
    baseURL: "http://127.0.0.1:4173",
    // A failure here is a PICTURE problem, so keep the picture.
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    // `vite preview` serves the built dist/ on 4173.
    command: "npm run preview -- --port 4173 --strictPort",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
