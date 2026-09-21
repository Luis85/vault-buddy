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
    //
    // `--host 127.0.0.1` is LOAD-BEARING, not tidiness. Vite's preview host
    // defaults to the NAME `localhost`, and Node resolves that name before
    // binding: in this dev container it comes back 127.0.0.1, on a GitHub
    // ubuntu runner it comes back `::1` first. So Vite bound IPv6-only there
    // while Playwright polled the IPv4 address below, and the job died with
    // `Timed out waiting 60000ms from config.webServer` — reproduced exactly,
    // by pointing this `url` at `[::1]` here, where the families are the other
    // way round. Naming the literal address on BOTH sides removes the
    // resolver from the question entirely.
    command: "npm run preview -- --port 4173 --strictPort --host 127.0.0.1",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
    // The failure above reported only the timeout: the server's own output
    // went nowhere, so the CI log said nothing about whether Vite had started,
    // crashed, or bound somewhere else. Piping it costs a few lines per run
    // and is the difference between reading the answer and guessing it.
    stdout: "pipe",
    stderr: "pipe",
  },
});
